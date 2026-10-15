// Explicit, confirmed choices only. No filesystem work is allowed here; persistence belongs to the platform shell.

use crate::unicode::{string, utf16};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub const CAPACITY: usize = 1024;
const UNDO_DEPTH: usize = 20;
const MAX_PENDING_CHOICES: usize = 8;

/// A normalized explicit choice, not surrounding document text. Debug output never includes its contents.
#[derive(Clone, PartialEq, Eq)]
pub struct LearningChoice {
    pub raw: String, pub keys: String, pub korean: bool, pub context: String,
}
impl std::fmt::Debug for LearningChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("LearningChoice { redacted }") }
}

/// An in-memory, single-use confirmation ticket for a document edit. Clones share consumption, so a repeated
/// asynchronous callback cannot train twice. A reset/delete/privacy change invalidates outstanding tickets.
#[derive(Clone)]
pub struct LearningBatch {
    owner: Arc<()>, epoch: u64, choices: Arc<Mutex<Option<Vec<LearningChoice>>>>,
}
impl std::fmt::Debug for LearningBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("LearningBatch { redacted }") }
}
impl PartialEq for LearningBatch {
    fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.choices, &other.choices) }
}
impl Eq for LearningBatch {}
/// Confirmation evidence is distinct from an empty delta. Invalidated counts are consumed once, too.
#[derive(Debug, PartialEq, Eq)]
pub enum LearningConfirmation { Confirmed(Vec<LearningChoice>), Invalidated(usize), ConsumedOrForeign }
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
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Off while learning is paused (C-LEX-09) or a secure field has focus (C-CTX-01): choices still apply, nothing new
    /// is recorded.
    recording: bool,
    deferred: bool,
    pending: Vec<LearningChoice>,
    pending_overflow: bool,
    epoch: u64,
}

/// Bounded personal dictionary of confirmed Korean/English choices with exponentially discounted evidence and one-step undo
/// per confirmation (global and contextual entries change together).
pub struct WordPreferences {
    inner: Mutex<Inner>,
    confirmation_owner: Arc<()>,
}

/// Construct on a worker. Installation swaps bounded containers; the returned old containers can be disposed
/// on that worker too. Never apply to an engine whose local choices changed since the snapshot was requested.
pub struct PreparedLearning { words: HashMap<String, LearnedWord>, undo: Vec<Vec<Change>>, sequence: i64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotInstall { Applied, Changed, Busy }
impl PreparedLearning {
    /// Preserve legacy choices in memory; the new store wins matching identities. Never write legacy data implicitly.
    pub fn with_legacy(legacy: &[LearnedWord], stored: &[LearnedWord]) -> Self {
        let mut merged = HashMap::new(); let mut sequence = 0;
        for source in [legacy, stored] {
            let mut words: Vec<_> = Self::new(source).words.into_values().collect();
            words.sort_by_key(|word| word.sequence);
            for mut word in words {
                sequence += 1; word.sequence = sequence;
                merged.insert(identity(&word), word);
            }
        }
        // Capacity applies to unique identities, after validating each source and preserving its recency.
        Self::new(&merged.into_values().collect::<Vec<_>>())
    }
    pub fn new(entries: &[LearnedWord]) -> Self {
        let preferences = WordPreferences::new(); preferences.restore(entries);
        let mut inner = preferences.lock();
        Self { words: std::mem::take(&mut inner.words), undo: vec![], sequence: inner.sequence }
    }
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
pub(crate) fn normalize(raw: &str, keys: &str) -> Option<(String, String, String)> {
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

pub(crate) fn valid_context(value: &str) -> bool {
    value.is_empty() || (value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
}

fn identity(word: &LearnedWord) -> String {
    format!("{}\t{}\t{}", word.raw, word.keys, word.context)
}

impl WordPreferences {
    pub fn try_install_snapshot(&self, version: i32, prepared: &mut PreparedLearning) -> SnapshotInstall {
        self.try_install_snapshot_with_local(version, prepared, &[])
    }
    /// Bounded local-only recovery at a clean word boundary. The full dictionary is already worker-prepared;
    /// at most 64 validated deltas are replayed in memory, never persisted or added to pending confirmations.
    pub fn try_install_snapshot_with_local(&self, version: i32, prepared: &mut PreparedLearning, local: &[LearningChoice]) -> SnapshotInstall {
        if local.len() > crate::learning::client::MAX_QUEUED_CHOICES || local.iter().any(|c|
            !valid_context(&c.context) || normalize(&c.raw, &c.keys).is_none_or(|(_, raw, keys)| raw != c.raw || keys != c.keys)) {
            return SnapshotInstall::Changed;
        }
        let Ok(mut inner) = self.inner.try_lock() else { return SnapshotInstall::Busy; };
        if inner.version != version || !inner.pending.is_empty() || inner.pending_overflow { return SnapshotInstall::Changed; }
        inner.invalidate_confirmations();
        std::mem::swap(&mut inner.words, &mut prepared.words);
        std::mem::swap(&mut inner.undo, &mut prepared.undo);
        std::mem::swap(&mut inner.sequence, &mut prepared.sequence);
        for choice in local { inner.remember(choice); }
        inner.version = inner.version.wrapping_add(1);
        SnapshotInstall::Applied
    }
    pub fn new() -> Self {
        WordPreferences {
            inner: Mutex::new(Inner {
                words: HashMap::new(),
                undo: Vec::new(),
                sequence: 0,
                version: 0,
                enabled: true,
                recording: true,
                deferred: false,
                pending: Vec::new(),
                pending_overflow: false,
                epoch: 0,
            }),
            confirmation_owner: Arc::new(()),
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
            inner.invalidate_confirmations();
            inner.enabled = value;
            inner.version = inner.version.wrapping_add(1);
        }
    }

    pub fn recording(&self) -> bool {
        self.lock().recording
    }
    /// Pausing keeps applying remembered choices but records no new ones (C-LEX-09).
    pub fn set_recording(&self, value: bool) {
        let mut inner = self.lock();
        if inner.recording != value { inner.invalidate_confirmations(); }
        inner.recording = value;
    }

    /// TSF opts in; existing platform callers retain immediate learning. This performs no I/O.
    pub fn defer_learning(&self, enabled: bool) {
        let mut inner = self.lock();
        if inner.deferred != enabled { inner.invalidate_confirmations(); }
        inner.deferred = enabled;
    }

    /// Transfer choices to the document operation that caused them. Overflow rejects the entire batch.
    pub fn take_learning(&self) -> Option<LearningBatch> {
        let mut inner = self.lock();
        let choices = std::mem::take(&mut inner.pending);
        if std::mem::take(&mut inner.pending_overflow) || choices.is_empty() { return None; }
        Some(LearningBatch { owner: self.confirmation_owner.clone(), epoch: inner.epoch,
            choices: Arc::new(Mutex::new(Some(choices))) })
    }

    /// Call only after the associated document edit succeeds. Returns the actual confirmed deltas for an off-key
    /// persistence client; it never writes a file. Stale/private/foreign/replayed tickets produce no deltas.
    pub fn confirm_learning(&self, batch: &LearningBatch) -> Vec<LearningChoice> {
        match self.confirm_learning_with_status(batch) { LearningConfirmation::Confirmed(choices) => choices, _ => Vec::new() }
    }
    pub fn confirm_learning_with_status(&self, batch: &LearningBatch) -> LearningConfirmation {
        if !Arc::ptr_eq(&batch.owner, &self.confirmation_owner) { return LearningConfirmation::ConsumedOrForeign; }
        let mut inner = self.lock();
        let Some(choices) = batch.choices.lock().unwrap_or_else(|p| p.into_inner()).take() else { return LearningConfirmation::ConsumedOrForeign; };
        if batch.epoch != inner.epoch || inner.epoch == u64::MAX || !inner.deferred || !inner.enabled || !inner.recording {
            return LearningConfirmation::Invalidated(choices.len());
        }
        for choice in &choices { inner.remember(choice); }
        LearningConfirmation::Confirmed(choices)
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
        if !inner.enabled || !inner.recording || inner.epoch == u64::MAX {
            return false;
        }
        let choice = LearningChoice { raw: body, keys: key_body, korean, context: context.into() };
        if inner.deferred {
            if inner.pending_overflow { return false; }
            if inner.pending.len() == MAX_PENDING_CHOICES {
                inner.pending.clear(); inner.pending_overflow = true; return false;
            }
            inner.pending.push(choice);
            // Session uses this serial to report that a choice was proposed. The lookup data is still unchanged.
            inner.version = inner.version.wrapping_add(1);
        } else {
            inner.remember(&choice);
        }
        true
    }

    /// Forgets one word everywhere (its global entry and every contextual one); returns how many entries went (C-LEX-03).
    /// The undo history is cleared so an older step cannot bring the word back.
    pub fn forget(&self, raw: &str, keys: &str) -> usize {
        let Some((_, body, key_body)) = normalize(raw, keys) else {
            return 0;
        };
        let mut inner = self.lock();
        inner.invalidate_confirmations();
        let before = inner.words.len();
        inner.words.retain(|_, word| !(word.raw == body && word.keys == key_body));
        let removed = before - inner.words.len();
        if removed > 0 {
            inner.undo.clear();
            inner.version = inner.version.wrapping_add(1);
        }
        removed
    }

    pub fn undo_last(&self) -> bool {
        let mut inner = self.lock();
        inner.invalidate_confirmations();
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
        inner.invalidate_confirmations();
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
        inner.invalidate_confirmations();
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
    fn invalidate_confirmations(&mut self) {
        self.pending.clear(); self.pending_overflow = false;
        // Exhaustion is unreachable in normal use; stop accepting rather than reusing a generation.
        if let Some(epoch) = self.epoch.checked_add(1) { self.epoch = epoch; }
        else { self.enabled = false; self.recording = false; }
    }

    fn remember(&mut self, choice: &LearningChoice) {
        let mut record = Vec::new();
        self.update(&choice.raw, &choice.keys, choice.korean, "", &mut record);
        if !choice.context.is_empty() { self.update(&choice.raw, &choice.keys, choice.korean, &choice.context, &mut record); }
        self.undo.push(record);
        if self.undo.len() > UNDO_DEPTH { self.undo.remove(0); }
        self.version = self.version.wrapping_add(1);
    }

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

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    fn prepared() -> PreparedLearning {
        let source = WordPreferences::new(); source.remember_global("saved", "saved", true);
        PreparedLearning::new(&source.export().0)
    }
    #[test]
    fn initial_store_keeps_legacy_choices_and_new_store_wins_only_matching_entries() {
        let legacy = WordPreferences::new(); legacy.remember_global("legacy", "legacy", true); legacy.remember_global("same", "same", false);
        let fresh = WordPreferences::new(); fresh.remember_global("same", "same", true);
        for stored in [vec![], fresh.export().0] {
            let preferences = WordPreferences::new(); let mut snapshot = PreparedLearning::with_legacy(&legacy.export().0, &stored);
            assert_eq!(preferences.try_install_snapshot(preferences.version(), &mut snapshot), SnapshotInstall::Applied);
            assert_eq!(preferences.try_get_global("legacy", "legacy"), Some(true));
            assert_eq!(preferences.try_get_global("same", "same"), Some(!stored.is_empty()));
            assert_eq!(legacy.try_get_global("same", "same"), Some(false));
        }
    }
    #[test]
    fn startup_merge_deduplicates_before_capacity_and_preserves_source_recency() {
        fn word(index: usize) -> String { format!("word{}{}{}", (b'a' + (index / 676) as u8) as char,
            (b'a' + ((index / 26) % 26) as u8) as char, (b'a' + (index % 26) as u8) as char) }
        let legacy = WordPreferences::new(); let stored = WordPreferences::new();
        for index in 0..CAPACITY { let raw = word(index); legacy.remember_global(&raw, &raw, false); }
        for index in CAPACITY / 2..CAPACITY { let raw = word(index); stored.remember_global(&raw, &raw, true); }
        let mut entries = stored.export().0; entries.reverse(); // on-disk iteration order is not recency
        let restored = WordPreferences::new(); let mut snapshot = PreparedLearning::with_legacy(&legacy.export().0, &entries);
        assert_eq!(restored.try_install_snapshot(restored.version(), &mut snapshot), SnapshotInstall::Applied);
        assert_eq!(restored.count(), CAPACITY, "duplicates must not evict distinct legacy choices");
        assert_eq!(restored.try_get_global(&word(0), &word(0)), Some(false));
        assert_eq!(restored.try_get_global(&word(CAPACITY - 1), &word(CAPACITY - 1)), Some(true));
        assert_eq!(restored.export().0.last().unwrap().raw, word(CAPACITY - 1));
    }
    #[test]
    fn prepared_snapshot_swaps_without_waiting_and_preserves_privacy_flags() {
        let preferences = WordPreferences::new(); preferences.defer_learning(true); preferences.set_recording(false);
        let mut snapshot = prepared(); let version = preferences.version();
        let held = preferences.lock();
        assert_eq!(preferences.try_install_snapshot(version, &mut snapshot), SnapshotInstall::Busy); drop(held);
        assert_eq!(preferences.try_install_snapshot(version, &mut snapshot), SnapshotInstall::Applied);
        assert_eq!(preferences.try_get_global("saved", "saved"), Some(true));
        assert!(!preferences.recording()); assert!(preferences.lock().deferred);
        assert!(snapshot.words.is_empty(), "old dictionary is returned for off-thread disposal");
    }
    #[test]
    fn startup_snapshot_cannot_overwrite_new_local_choices_or_pending_confirmation() {
        for deferred in [false, true] {
            let preferences = WordPreferences::new(); preferences.defer_learning(deferred);
            let version = preferences.version(); let mut snapshot = prepared();
            preferences.remember_global("recent", "recent", false);
            assert_eq!(preferences.try_install_snapshot(version, &mut snapshot), SnapshotInstall::Changed);
            assert!(!preferences.try_get_global("saved", "saved").is_some());
            if deferred { assert!(preferences.take_learning().is_some()); }
            else { assert_eq!(preferences.try_get_global("recent", "recent"), Some(false)); }
        }
    }
}
