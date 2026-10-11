// Model and word-list files, embedded from the repository's data/ folder (the single source shared with the Windows build).

use flate2::read::GzDecoder;
use std::io::Read;

pub(crate) const KOREAN_LM: &[u8] = include_bytes!("../../data/korean-lm.txt.gz");
pub(crate) const ENGLISH_LM: &str = include_str!("../../data/english-lm.txt");
pub(crate) const ENGLISH: &str = include_str!("../../data/english.txt");
pub(crate) const KOREAN_WORDS: &[u8] = include_bytes!("../../data/korean-words.txt.gz");
pub(crate) const ENGLISH_WORDS: &[u8] = include_bytes!("../../data/english-words.txt.gz");

pub(crate) fn gunzip(bytes: &[u8]) -> String {
    let mut text = String::new();
    GzDecoder::new(bytes)
        .read_to_string(&mut text)
        .expect("embedded resource is valid gzip UTF-8");
    text
}

/// `StreamReader.ReadLine` semantics: lines end at "\r\n", "\n" or "\r"; a final terminator adds no empty line.
pub(crate) fn read_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&text[start..i]);
                start = i + 1;
            }
            b'\r' => {
                lines.push(&text[start..i]);
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}
