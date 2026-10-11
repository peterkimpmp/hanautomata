// User exception lists (C-LEX-02): words never to convert and words always to convert, edited in the settings window or as
// text files. They outrank the automatic decision and the personal dictionary; only an explicit choice for the word being
// composed (F2, `;`, a switch key) outranks them.

use crate::hangul;
use std::collections::HashSet;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Edge punctuation ignored when matching, as the personal dictionary trims it.
const EDGES: &[char] = &['.', ',', '!', '?', ';', ':', '(', ')', '[', ']', '{', '}', '"', '\''];

#[derive(Default)]
struct Lists {
    /// Lowercase literal keys of words that stay as typed.
    keep: HashSet<String>,
    /// Dubeolsik keys (case-sensitive: Shift types ㄲ) of words that always become Hangul.
    convert: HashSet<String>,
    keep_entries: Vec<String>,
    convert_entries: Vec<String>,
    version: u32,
}

#[derive(Default)]
pub struct Exceptions {
    lists: RwLock<Lists>,
}

fn is_hangul(text: &str) -> bool {
    text.chars()
        .any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c) || ('\u{3131}'..='\u{318e}').contains(&c))
}

/// Dubeolsik keys of an entry written as Hangul (안녕) or as keys (dkssud).
fn keys_of(entry: &str) -> String {
    if is_hangul(entry) {
        hangul::to_keys(entry)
    } else {
        entry.to_string()
    }
}

/// One entry per line; blank lines and lines starting with `#` are ignored; surrounding spaces are trimmed.
pub fn parse_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

impl Exceptions {
    fn read(&self) -> RwLockReadGuard<'_, Lists> {
        self.lists.read().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn write(&self) -> RwLockWriteGuard<'_, Lists> {
        self.lists.write().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Replaces both lists. Entries may be Hangul or keys; duplicates and blanks are dropped, order is kept for display.
    pub fn set(&self, keep: &[String], convert: &[String]) {
        let clean = |entries: &[String]| {
            let mut seen = HashSet::new();
            entries
                .iter()
                .map(|e| e.trim().trim_matches(EDGES).to_string())
                .filter(|e| !e.is_empty() && !e.contains(char::is_whitespace) && seen.insert(e.clone()))
                .collect::<Vec<_>>()
        };
        let (keep_entries, convert_entries) = (clean(keep), clean(convert));
        let mut lists = self.write();
        lists.keep = keep_entries.iter().map(|e| keys_of(e).to_lowercase()).collect();
        lists.convert = convert_entries.iter().map(|e| keys_of(e)).collect();
        lists.keep_entries = keep_entries;
        lists.convert_entries = convert_entries;
        lists.version = lists.version.wrapping_add(1);
    }

    pub fn keep(&self) -> Vec<String> {
        self.read().keep_entries.clone()
    }
    pub fn convert(&self) -> Vec<String> {
        self.read().convert_entries.clone()
    }
    /// Changes on every `set`; decision caches compare it.
    pub fn version(&self) -> u32 {
        self.read().version
    }

    /// `Some(false)`: keep the word as typed · `Some(true)`: convert it · `None`: not listed. Edge punctuation is ignored.
    pub fn lookup(&self, raw: &str, keys: &str) -> Option<bool> {
        let lists = self.read();
        if lists.keep.is_empty() && lists.convert.is_empty() {
            return None;
        }
        if lists.convert.contains(keys.trim_matches(EDGES)) {
            return Some(true);
        }
        if lists.keep.contains(&raw.trim_matches(EDGES).to_lowercase()) {
            return Some(false);
        }
        None
    }
}
