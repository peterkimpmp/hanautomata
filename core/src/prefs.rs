// Explicit, confirmed choices only. No filesystem work is allowed here; persistence belongs to the platform shell.

use crate::unicode::{string, utf16};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

pub const CAPACITY: usize = 1024;
const UNDO_DEPTH: usize = 20;
const EDGES: &[u16] = &[
    b'.' as u16,
    b',' as u16,
    b'!' as u16,
    b'?' as u16,
    b';' as u16,
    b':' as u16,
    b'(' as u16,
    b')' as u16,
    b'[' as u16,
    b']' as u16,
    b'{' as u16,
    b'}' as u16,
    b'"' as u16,
    b'\'' as u16,
];

/// One remembered choice. `context` is empty for the global entry or a 64-hex fingerprint of the preceding word.
#[derive(Clone, Debug, PartialEq)]
pub struct LearnedWord {
    pub raw: String,
    pub keys: String,
    pub korean: bool,
    pub sequence: i64,
    pub context: String,
    pub korean_weight: f64,
    pub english_weight: f64,
}

/// Result of [`WordPreferences::try_get`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lookup {
    pub found: bool,
    pub korean: bool,
    pub context_match: bool,
    pub conflict: bool,
}

struct Change {
    key: String,
    previous: Option<LearnedWord>,
    evicted: Option<LearnedWord>,
}

struct Inner {
    words: HashMap<String, LearnedWord>,
    undo: Vec<Vec<Change>>,
    sequence: i64,
    version: i32,
    enabled: bool,
}

/// Bounded personal dictionary of confirmed Korean/English choices with exponentially discounted evidence and one-step undo
/// per confirmation (global and contextual entries change together).
pub struct WordPreferences {
    inner: Mutex<Inner>,
}

impl Default for WordPreferences {
    fn default() -> Self {
        Self::new()
    }
}

fn is_letter(c: u16) -> bool {
    (b'a' as u16..=b'z' as u16).contains(&c) || (b'A' as u16..=b'Z' as u16).contains(&c)
}

/// Returns (identity, body, key body) for an eligible word: letters only once edge punctuation is trimmed, 1..=64 long.
fn normalize(raw: &str, keys: &str) -> Option<(String, String, String)> {
    let (raw, keys) = (utf16(raw), utf16(keys));
    if raw.is_empty() || raw.len() != keys.len() {
        return None;
    }
    let (mut start, mut end) = (0, raw.len());
    while start < end && EDGES.contains(&raw[start]) {
        start += 1;
    }
    while end > start && EDGES.contains(&raw[end - 1]) {
        end -= 1;
    }
    if end == start || end - start > 64 {
        return None;
    }
    if (start..end).any(|i| !is_letter(raw[i]) || !is_letter(keys[i])) {
        return None;
    }
    let (body, key_body) = (string(&raw[start..end]), string(&keys[start..end]));
    Some((format!("{body}\t{key_body}"), body, key_body))
}

/// Fingerprint of the preceding word committed by this app (letters and Hangul syllables only, at most 64 code units).
/// Never inspect surrounding UI text. Returns "" for anything else.
pub fn context_key(text: &str) -> String {
    let units = utf16(text);
    if units.is_empty() || units.len() > 64 || units.iter().any(|&c| !is_letter(c) && !(0xac00..=0xd7a3).contains(&c)) {
        return String::new();
    }
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_context(value: &str) -> bool {
    value.is_empty() || (value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
}

fn identity(word: &LearnedWord) -> String {
    format!("{}\t{}\t{}", word.raw, word.keys, word.context)
}

impl WordPreferences {
    pub fn new() -> Self {
        WordPreferences {
            inner: Mutex::new(Inner {
                words: HashMap::new(),
                undo: Vec::new(),
                sequence: 0,
                version: 0,
                enabled: true,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Changes on every mutation; caches compare it.
    pub fn version(&self) -> i32 {
        self.lock().version
    }
    pub fn count(&self) -> usize {
        self.lock().words.len()
    }
    pub fn can_undo(&self) -> bool {
        !self.lock().undo.is_empty()
    }
    pub fn enabled(&self) -> bool {
        self.lock().enabled
    }
    pub fn set_enabled(&self, value: bool) {
        let mut inner = self.lock();
        if inner.enabled != value {
            inner.enabled = value;
            inner.version = inner.version.wrapping_add(1);
        }
    }

    /// Global lookup only (no context).
    pub fn try_get_global(&self, raw: &str, keys: &str) -> Option<bool> {
        let lookup = self.try_get(raw, keys, "");
        lookup.found.then_some(lookup.korean)
    }

    /// A contextual entry wins outright; otherwise the global evidence must be clear (score ≥ 0.72 or ≤ 0.28), or the
    /// lookup reports a conflict.
    pub fn try_get(&self, raw: &str, keys: &str, context: &str) -> Lookup {
        let mut lookup = Lookup::default();
        let Some((identity, _, _)) = normalize(raw, keys) else {
            return lookup;
        };
        let inner = self.lock();
        if !inner.enabled {
            return lookup;
        }
        if !context.is_empty() {
            if let Some(value) = inner.words.get(&format!("{identity}\t{context}")) {
                lookup.found = true;
                lookup.korean = value.korean;
                lookup.context_match = true;
                return lookup;
            }
        }
        let Some(value) = inner.words.get(&format!("{identity}\t")) else {
            return lookup;
        };
        // Smoothed, exponentially discounted evidence; this is a selection score, not measured accuracy.
        let score = (value.korean_weight + 0.5) / (value.korean_weight + value.english_weight + 1.0);
        if score >= 0.72 || score <= 0.28 {
            lookup.found = true;
            lookup.korean = score >= 0.72;
        } else {
            lookup.conflict = true;
        }
        lookup
    }

    pub fn remember_global(&self, raw: &str, keys: &str, korean: bool) -> bool {
        self.remember(raw, keys, korean, "")
    }

    /// Records one confirmed choice: the global entry and, with a context, the contextual entry, as one undo step.
    pub fn remember(&self, raw: &str, keys: &str, korean: bool, context: &str) -> bool {
        let Some((_, body, key_body)) = normalize(raw, keys) else {
            return false;
        };
        if !valid_context(context) {
            return false;
        }
        let mut inner = self.lock();
        if !inner.enabled {
            return false;
        }
        let mut record = Vec::new();
        inner.update(&body, &key_body, korean, "", &mut record);
        if !context.is_empty() {
            inner.update(&body, &key_body, korean, context, &mut record);
        }
        inner.undo.push(record);
        if inner.undo.len() > UNDO_DEPTH {
            inner.undo.remove(0);
        }
        inner.version = inner.version.wrapping_add(1);
        true
    }

    pub fn undo_last(&self) -> bool {
        let mut inner = self.lock();
        let Some(record) = inner.undo.pop() else { return false };
        for change in record.into_iter().rev() {
            match change.previous {
                None => {
                    inner.words.remove(&change.key);
                }
                Some(previous) => {
                    inner.words.insert(change.key, previous);
                }
            }
            if let Some(evicted) = change.evicted {
                inner.words.insert(identity(&evicted), evicted);
            }
        }
        inner.version = inner.version.wrapping_add(1);
        true
    }

    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.words.clear();
        inner.undo.clear();
        inner.version = inner.version.wrapping_add(1);
    }

    /// Snapshot ordered by sequence, with the version it reflects.
    pub fn export(&self) -> (Vec<LearnedWord>, i32) {
        let inner = self.lock();
        let mut entries: Vec<LearnedWord> = inner.words.values().cloned().collect();
        entries.sort_by_key(|word| word.sequence);
        (entries, inner.version)
    }

    /// Replaces everything with the newest `CAPACITY` entries; invalid entries are dropped (after the capacity cut, as in
    /// the C# engine). Format 1 entries (no weights) migrate as one explicit observation.
    pub fn restore(&self, entries: &[LearnedWord]) {
        let mut inner = self.lock();
        inner.words.clear();
        inner.undo.clear();
        inner.sequence = 0;
        let mut ordered: Vec<&LearnedWord> = entries.iter().collect();
        ordered.sort_by_key(|word| std::cmp::Reverse(word.sequence)); // stable, like LINQ OrderByDescending
        ordered.truncate(CAPACITY);
        ordered.sort_by_key(|word| word.sequence);
        for entry in ordered {
            let Some((_, body, key_body)) = normalize(&entry.raw, &entry.keys) else {
                continue;
            };
            let invalid = |w: f64| w.is_nan() || w.is_infinite() || w < 0.0;
            if !valid_context(&entry.context) || invalid(entry.korean_weight) || invalid(entry.english_weight) {
                continue;
            }
            inner.sequence += 1;
            let mut value = entry.clone();
            value.raw = body;
            value.keys = key_body;
            value.sequence = inner.sequence;
            value.korean_weight = net_min(16.0, value.korean_weight);
            value.english_weight = net_min(16.0, value.english_weight);
            if value.korean_weight + value.english_weight == 0.0 {
                value.korean_weight = if value.korean { 1.0 } else { 0.0 };
                value.english_weight = if value.korean { 0.0 } else { 1.0 };
            }
            inner.words.insert(identity(&value), value);
        }
        inner.version = inner.version.wrapping_add(1);
    }
}

impl Inner {
    fn update(&mut self, raw: &str, keys: &str, korean: bool, context: &str, changes: &mut Vec<Change>) {
        let key = format!("{raw}\t{keys}\t{context}");
        let previous = self.words.get(&key).cloned();
        let mut evicted = None;
        if previous.is_none() && self.words.len() >= CAPACITY {
            if let Some(oldest) = self
                .words
                .iter()
                .min_by_key(|(_, word)| word.sequence)
                .map(|(k, _)| k.clone())
            {
                evicted = self.words.remove(&oldest);
            }
        }
        self.sequence += 1;
        let (korean_weight, english_weight) = match &previous {
            None => (0.0, 0.0),
            Some(p) => (p.korean_weight * 0.85, p.english_weight * 0.85),
        };
        self.words.insert(
            key.clone(),
            LearnedWord {
                raw: raw.to_string(),
                keys: keys.to_string(),
                korean,
                context: context.to_string(),
                sequence: self.sequence,
                korean_weight: korean_weight + if korean { 1.0 } else { 0.0 },
                english_weight: english_weight + if korean { 0.0 } else { 1.0 },
            },
        );
        changes.push(Change { key, previous, evicted });
    }
}

/// `Math.Min` for non-NaN operands (NaN never reaches it here): equal zeros prefer -0.0, as .NET does.
fn net_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else if b < a {
        b
    } else if a.is_sign_negative() {
        a
    } else {
        b
    }
}
