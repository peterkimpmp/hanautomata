// Export/import bundle (C-LEX-04): the personal dictionary, the exception lists and the shell's settings in one JSON
// document that both platforms read and write:
//   {"Format":1,"Kind":"hanautomata-bundle","Words":{"Format":2,"Entries":[…]},"Keep":[…],"Convert":[…],"Settings":{…}}
// "Words" is the learning-file document (store.rs). "Settings" is an object of shell settings by name; each shell applies
// the names it knows and ignores the rest.

use crate::exceptions::Exceptions;
use crate::prefs::{LearnedWord, WordPreferences};
use crate::store::{Document, Entry};
use serde::{Deserialize, Serialize};

const KIND: &str = "hanautomata-bundle";

#[derive(Serialize, Deserialize)]
struct Bundle {
    #[serde(rename = "Format")]
    format: i32,
    #[serde(rename = "Kind")]
    kind: String,
    #[serde(rename = "Words")]
    words: Document,
    #[serde(rename = "Keep", default)]
    keep: Vec<String>,
    #[serde(rename = "Convert", default)]
    convert: Vec<String>,
    #[serde(rename = "Settings", default)]
    settings: serde_json::Map<String, serde_json::Value>,
}

/// What an import brought in.
#[derive(Debug, PartialEq)]
pub struct Imported {
    pub words: usize,
    pub keep: usize,
    pub convert: usize,
    /// The bundle's settings object as JSON text ("{}" when it has none).
    pub settings: String,
}

/// `settings` is a JSON object of shell settings (anything else is stored as {}).
pub fn export(preferences: &WordPreferences, exceptions: &Exceptions, settings: &str) -> Result<String, String> {
    let (entries, _) = preferences.export();
    let bundle = Bundle {
        format: 1,
        kind: KIND.into(),
        words: Document {
            format: 2,
            entries: Some(entries.iter().map(Entry::from).collect()),
        },
        keep: exceptions.keep(),
        convert: exceptions.convert(),
        settings: serde_json::from_str(settings).unwrap_or_default(),
    };
    serde_json::to_string_pretty(&bundle).map_err(|e| format!("export: {e}"))
}

/// Merges a bundle: imported words are added after the existing ones (they win where both have the same word and context,
/// and the dictionary keeps its newest 1,024 entries); exception entries are added to the lists.
pub fn import(text: &str, preferences: &WordPreferences, exceptions: &Exceptions) -> Result<Imported, String> {
    let bundle: Bundle = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("parse: {e}"))?;
    if bundle.kind != KIND || bundle.format != 1 {
        return Err(format!(
            "not a Hanautomata bundle (kind {}, format {})",
            bundle.kind, bundle.format
        ));
    }
    if bundle.words.format != 1 && bundle.words.format != 2 {
        return Err(format!("unknown word format {}", bundle.words.format));
    }
    let (mut entries, _) = preferences.export();
    let base = entries.iter().map(|w| w.sequence).max().unwrap_or(0);
    let mut imported: Vec<LearnedWord> = bundle
        .words
        .entries
        .unwrap_or_default()
        .into_iter()
        .map(LearnedWord::from)
        .collect();
    imported.sort_by_key(|w| w.sequence);
    let words = imported.len();
    for (i, word) in imported.iter_mut().enumerate() {
        word.sequence = base + 1 + i as i64;
    }
    entries.extend(imported);
    preferences.restore(&entries);
    let merge = |current: Vec<String>, added: &[String]| current.into_iter().chain(added.iter().cloned()).collect::<Vec<_>>();
    exceptions.set(
        &merge(exceptions.keep(), &bundle.keep),
        &merge(exceptions.convert(), &bundle.convert),
    );
    Ok(Imported {
        words,
        keep: bundle.keep.len(),
        convert: bundle.convert.len(),
        settings: serde_json::to_string(&bundle.settings).unwrap_or_else(|_| "{}".into()),
    })
}
