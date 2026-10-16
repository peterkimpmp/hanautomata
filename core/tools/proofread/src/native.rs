use crate::{digest, Error, Lexicon};
use hunspell_rs::{CheckResult, Hunspell};
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};

pub const DICTIONARY_LOCK: &str = include_str!("../dictionary.lock.json");

#[derive(Deserialize)]
struct FileLock {
    name: String,
    size: usize,
    sha256: String,
}
#[derive(Deserialize)]
struct Lock {
    id: String,
    files: Vec<FileLock>,
}

/// Only the exact reviewed dictionary and its license files are accepted.
/// No downloads, logging, or native-handle sharing occur in this runtime.
pub struct HunspellLexicon {
    native: Hunspell,
    identity: String,
}

impl HunspellLexicon {
    pub fn load_pinned(directory: &Path) -> Result<Self, Error> {
        let lock: Lock = serde_json::from_str(DICTIONARY_LOCK).map_err(|_| Error("invalid_embedded_manifest"))?;
        for expected in &lock.files {
            let file = File::open(directory.join(&expected.name)).map_err(|_| Error("dictionary_file_missing"))?;
            let mut bytes = Vec::new();
            file.take(expected.size as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error("dictionary_read_failed"))?;
            if bytes.len() != expected.size || digest(&bytes) != expected.sha256 {
                return Err(Error("dictionary_integrity_failed"));
            }
        }
        let aff = directory.join("ko.aff");
        let dic = directory.join("ko.dic");
        let aff = aff
            .to_str()
            .filter(|s| !s.contains('\0'))
            .ok_or(Error("invalid_dictionary_path"))?;
        let dic = dic
            .to_str()
            .filter(|s| !s.contains('\0'))
            .ok_or(Error("invalid_dictionary_path"))?;
        let native = Hunspell::new(aff, dic);
        // The wrapper offers no Result at construction. Fail closed if loading/encoding fails.
        if ["안녕하세요", "돼요", "학교에"]
            .iter()
            .any(|w| native.check(w) != CheckResult::FoundInDictionary)
        {
            return Err(Error("dictionary_self_check_failed"));
        }
        Ok(Self {
            native,
            identity: lock.id,
        })
    }
}

impl Lexicon for HunspellLexicon {
    fn contains(&mut self, word: &str) -> bool {
        !word.contains('\0') && self.native.check(word) == CheckResult::FoundInDictionary
    }
    fn suggest(&mut self, word: &str) -> Vec<String> {
        if word.contains('\0') {
            vec![]
        } else {
            self.native.suggest(word)
        }
    }
    fn engine_id(&self) -> String {
        "hunspell-1.7.1/hunspell-rs-0.4.0/hunspell-sys-0.3.1".into()
    }
    fn dictionary_id(&self) -> String {
        self.identity.clone()
    }
}
