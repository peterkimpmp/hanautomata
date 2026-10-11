// Learning-file storage (C-OPS-09). The file format is the Windows app's PreferenceStore document, so a dictionary can
// move between the tray app and the input methods: {"Format":2,"Entries":[{"Raw","Keys","Korean","Sequence","Context",
// "KoreanWeight","EnglishWeight"}]}. Format 1 (no weights, no context) is read and migrates on the next save.
//
// Contract: one process owns the file on each platform (macOS: the input-method process, which also hosts the settings
// window). Saves are atomic (temporary file in the same folder, then rename) and serialized across processes by an
// advisory lock on "<file>.lock". An unreadable file is reported and left untouched until the user changes something.
// The multi-process Windows design (one DLL per application) is decided after the S0 spike.

use crate::prefs::{LearnedWord, WordPreferences};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Same bound as the Windows app (JavaScriptSerializer MaxJsonLength).
pub const MAX_FILE_BYTES: u64 = 524_288;

#[derive(Serialize, Deserialize)]
struct Document {
    #[serde(rename = "Format", alias = "format")]
    format: i32,
    #[serde(rename = "Entries", alias = "entries", default)]
    entries: Option<Vec<Entry>>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    #[serde(rename = "Raw", alias = "raw", default)]
    raw: Option<String>,
    #[serde(rename = "Keys", alias = "keys", default)]
    keys: Option<String>,
    #[serde(rename = "Korean", alias = "korean", default)]
    korean: bool,
    #[serde(rename = "Sequence", alias = "sequence", default)]
    sequence: i64,
    #[serde(rename = "Context", alias = "context", default)]
    context: Option<String>,
    #[serde(rename = "KoreanWeight", alias = "koreanWeight", alias = "koreanweight", default)]
    korean_weight: f64,
    #[serde(rename = "EnglishWeight", alias = "englishWeight", alias = "englishweight", default)]
    english_weight: f64,
}

impl From<Entry> for LearnedWord {
    fn from(e: Entry) -> Self {
        LearnedWord {
            raw: e.raw.unwrap_or_default(),
            keys: e.keys.unwrap_or_default(),
            korean: e.korean,
            sequence: e.sequence,
            context: e.context.unwrap_or_default(),
            korean_weight: e.korean_weight,
            english_weight: e.english_weight,
        }
    }
}

impl From<&LearnedWord> for Entry {
    fn from(w: &LearnedWord) -> Self {
        Entry {
            raw: Some(w.raw.clone()),
            keys: Some(w.keys.clone()),
            korean: w.korean,
            sequence: w.sequence,
            context: Some(w.context.clone()),
            korean_weight: w.korean_weight,
            english_weight: w.english_weight,
        }
    }
}

/// Persists one [`WordPreferences`] to one file.
pub struct Store {
    path: PathBuf,
    saved_version: i32,
    last_error: String,
}

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Holds the cross-process advisory lock for the duration of a read or a save.
fn lock(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(path))?;
    file.lock()?;
    Ok(file)
}

fn read(path: &Path) -> Result<Option<Vec<LearnedWord>>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read: {e}")),
    };
    if metadata.len() > MAX_FILE_BYTES {
        return Err("file larger than 512 KB".into());
    }
    let text = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let document: Document = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("parse: {e}"))?;
    if document.format != 1 && document.format != 2 {
        return Err(format!("unknown format {}", document.format));
    }
    Ok(Some(
        document
            .entries
            .unwrap_or_default()
            .into_iter()
            .map(LearnedWord::from)
            .collect(),
    ))
}

impl Store {
    /// Loads `path` into `preferences`. A missing file is an empty dictionary; an unreadable one leaves `preferences`
    /// empty, sets [`Store::last_error`] and is not overwritten until the dictionary changes.
    pub fn open(path: impl Into<PathBuf>, preferences: &WordPreferences) -> Store {
        let path = path.into();
        let mut store = Store {
            path,
            saved_version: 0,
            last_error: String::new(),
        };
        let loaded = lock(&store.path)
            .map_err(|e| format!("lock: {e}"))
            .and_then(|_guard| read(&store.path));
        match loaded {
            Ok(Some(entries)) => preferences.restore(&entries),
            Ok(None) => {}
            Err(error) => store.last_error = error,
        }
        store.saved_version = preferences.version();
        store
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Empty after a successful load or save; otherwise the last failure (phase and reason, never learned text).
    pub fn last_error(&self) -> &str {
        &self.last_error
    }

    /// Whether `preferences` changed since the last load or save.
    pub fn is_dirty(&self, preferences: &WordPreferences) -> bool {
        preferences.version() != self.saved_version
    }

    /// Writes the dictionary if it changed. Returns Ok(true) when written, Ok(false) when nothing changed.
    pub fn flush(&mut self, preferences: &WordPreferences) -> Result<bool, String> {
        let (entries, revision) = preferences.export();
        if revision == self.saved_version {
            return Ok(false);
        }
        let document = Document {
            format: 2,
            entries: Some(entries.iter().map(Entry::from).collect()),
        };
        let result = (|| -> Result<(), String> {
            let json = serde_json::to_vec(&document).map_err(|e| format!("serialize: {e}"))?;
            let _guard = lock(&self.path).map_err(|e| format!("lock: {e}"))?;
            let temporary = temporary_path(&self.path);
            let write = (|| -> std::io::Result<()> {
                let mut file = File::create(&temporary)?;
                file.write_all(&json)?;
                file.sync_all()?;
                fs::rename(&temporary, &self.path)
            })();
            if write.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            write.map_err(|e| format!("write: {e}"))
        })();
        match result {
            Ok(()) => {
                self.saved_version = revision;
                self.last_error.clear();
                Ok(true)
            }
            Err(error) => {
                self.last_error = error.clone();
                Err(error)
            }
        }
    }
}
