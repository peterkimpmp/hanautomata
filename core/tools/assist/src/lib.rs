//! User-registered expressions only. No document capture, networking or model inference.
pub mod store;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const CAPACITY: usize = 512;
pub const MAX_CHARS: usize = 256;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub copies: u32,
    pub last_copied_day: u64,
}

#[derive(Clone, Debug)]
struct Undo {
    token: String,
    entry: Entry,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    schema_version: u32,
    pub enabled: bool,
    entries: Vec<Entry>,
    #[serde(skip)]
    undo: Option<Undo>,
    #[serde(skip)]
    sequence: u64,
    #[serde(skip, default = "nonce")]
    nonce: String,
}

fn nonce() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: false,
            entries: vec![],
            undo: None,
            sequence: 0,
            nonce: nonce(),
        }
    }
}

fn normalized(text: &str) -> Result<String, &'static str> {
    // Bound before allocating normalized data. Reject controls and direction overrides in user-visible labels.
    if text.len() > 4096
        || text.chars().any(|c| {
            c.is_control()
                || matches!(c,
        '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
        })
    {
        return Err("invalid_text");
    }
    let text = text
        .nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !(2..=MAX_CHARS).contains(&text.chars().count()) {
        return Err("invalid_text");
    }
    Ok(text)
}

fn identity(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn score(entry: &Entry, day: u64) -> u32 {
    entry.copies >> (day.saturating_sub(entry.last_copied_day) / 30).min(16)
}

impl Library {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 || self.entries.len() > CAPACITY {
            return Err("store_unavailable");
        }
        let mut ids = std::collections::HashSet::new();
        for e in &self.entries {
            if normalized(&e.text).ok().as_deref() != Some(e.text.as_str())
                || e.id != identity(&e.text)
                || !ids.insert(&e.id)
                || e.copies > 65535
                || e.last_copied_day > 3_650_000
            {
                return Err("store_unavailable");
            }
        }
        Ok(())
    }
    pub fn add(&mut self, text: &str) -> Result<String, &'static str> {
        let text = normalized(text)?;
        let id = identity(&text);
        if !self.entries.iter().any(|e| e.id == id) {
            if self.entries.len() >= CAPACITY {
                return Err("full");
            }
            self.entries.push(Entry {
                id: id.clone(),
                text,
                copies: 0,
                last_copied_day: 0,
            });
        }
        self.undo = None;
        Ok(id)
    }
    pub fn list(&self) -> Vec<Entry> {
        let mut entries = self.entries.clone();
        entries.sort_by(|a, b| a.text.cmp(&b.text));
        entries
    }
    pub fn query(&self, prefix: &str, day: u64) -> Vec<Entry> {
        if !self.enabled {
            return vec![];
        }
        let Ok(prefix) = normalized(prefix) else {
            return vec![];
        };
        let key = prefix.to_lowercase();
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| {
                let text = e.text.to_lowercase();
                text.starts_with(&key) && text != key
            })
            .cloned()
            .collect();
        entries.sort_by(|a, b| {
            score(b, day)
                .cmp(&score(a, day))
                .then(b.last_copied_day.cmp(&a.last_copied_day))
                .then(a.text.cmp(&b.text))
        });
        entries.truncate(3);
        entries
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.undo = None;
        }
    }
    pub fn copied(&mut self, id: &str, day: u64) -> Result<String, &'static str> {
        if !self.enabled {
            return Err("disabled");
        }
        let entry = self.entries.iter_mut().find(|e| e.id == id).ok_or("not_found")?;
        self.sequence = self.sequence.wrapping_add(1);
        let token = format!("{}-{}", self.nonce, self.sequence);
        self.undo = Some(Undo {
            token: token.clone(),
            entry: entry.clone(),
        });
        entry.copies = score(entry, day).saturating_add(1).min(65535);
        entry.last_copied_day = day.min(3_650_000);
        Ok(token)
    }
    pub fn undo(&mut self, token: &str) -> Result<(), &'static str> {
        let undo = self.undo.as_ref().filter(|u| u.token == token).ok_or("stale_undo")?;
        let entry = self.entries.iter_mut().find(|e| e.id == undo.entry.id).ok_or("stale_undo")?;
        *entry = undo.entry.clone();
        self.undo = None;
        Ok(())
    }
    pub fn delete(&mut self, id: &str) -> Result<(), &'static str> {
        let index = self.entries.iter().position(|e| e.id == id).ok_or("not_found")?;
        self.entries.remove(index);
        self.undo = None;
        Ok(())
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.undo = None;
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub request_id: String,
    pub command: String,
    pub text: Option<String>,
    pub enabled: Option<bool>,
    pub entry_id: Option<String>,
    pub undo_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub schema_version: u32,
    pub request_id: String,
    pub ok: bool,
    pub enabled: bool,
    pub items: Vec<Entry>,
    pub error: String,
    pub undo_id: String,
}
impl Reply {
    pub fn error(request_id: &str, error: &str) -> Self {
        Self {
            schema_version: 1,
            request_id: request_id.into(),
            ok: false,
            enabled: false,
            items: vec![],
            error: error.into(),
            undo_id: String::new(),
        }
    }
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
