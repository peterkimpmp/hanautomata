// Hanja for one Hangul syllable (C-LEX-06), from Unicode Unihan readings (data/hanja.txt.gz, built by
// core/tools/build_hanja.py). Common hanja are KS X 1001's, the basic educational hanja first; extended adds KS X 1002 and
// name hanja.

use crate::resources::{gunzip, HANJA};
use std::collections::HashMap;
use std::sync::OnceLock;

struct Entry {
    common: Vec<char>,
    extended: Vec<char>,
}

fn table() -> &'static HashMap<char, Entry> {
    static TABLE: OnceLock<HashMap<char, Entry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = HashMap::new();
        for line in gunzip(HANJA).lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(reading), Some(common), Some(extended)) = (fields.next(), fields.next(), fields.next()) else {
                continue;
            };
            let mut chars = reading.chars();
            if let (Some(syllable), None) = (chars.next(), chars.next()) {
                table.insert(
                    syllable,
                    Entry {
                        common: common.chars().collect(),
                        extended: extended.chars().collect(),
                    },
                );
            }
        }
        table
    })
}

/// Hanja read as `syllable`, common ones first; `extended` adds the rest.
pub fn hanja(syllable: char, extended: bool) -> Vec<char> {
    match table().get(&syllable) {
        None => Vec::new(),
        Some(entry) => {
            let mut result = entry.common.clone();
            if extended {
                result.extend(&entry.extended);
            }
            result
        }
    }
}
