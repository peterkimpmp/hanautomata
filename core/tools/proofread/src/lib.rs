//! Local spelling suggestions over immutable text. This crate is not on the IME input path.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

mod native;
mod resource;
mod rules;
pub mod worker;
pub use native::HunspellLexicon;

pub const MAX_TEXT_BYTES: usize = 16_384;
pub const MAX_WORDS: usize = 128;
pub const MAX_DIAGNOSTICS: usize = 32;
pub const MAX_SUGGESTIONS: usize = 3;
pub const MAX_WORD_CHARS: usize = 32;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub request_id: String,
    pub document_id: String,
    pub revision: String,
    pub text: String,
    #[serde(default)]
    pub protected_words: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Diagnostic {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub utf8: Span,
    pub utf16: Span,
    pub original: String,
    pub normalized: String,
    pub suggestions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Skipped {
    pub utf8: Span,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub request_id: String,
    pub document_id: String,
    pub revision: String,
    pub snapshot_sha256: String,
    pub engine: String,
    pub dictionary: String,
    pub checked_words: usize,
    pub limited: bool,
    pub skipped: Vec<Skipped>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Error {}

/// Implementations must not transmit or retain the submitted text.
/// Mutable access keeps native engine handles confined to one worker.
pub trait Lexicon {
    fn contains(&mut self, word: &str) -> bool;
    fn suggest(&mut self, word: &str) -> Vec<String>;
    fn engine_id(&self) -> String;
    fn dictionary_id(&self) -> String;
}

pub struct Checker<L> {
    lexicon: L,
    rules: bool,
}
impl<L: Lexicon> Checker<L> {
    pub fn new(lexicon: L) -> Self {
        Self { lexicon, rules: false }
    }
    pub fn with_rules(mut self, enabled: bool) -> Self { self.rules = enabled; self }
    pub fn check(&mut self, request: &Request) -> Result<Report, Error> {
        validate(request)?;
        let protected: HashSet<String> = request.protected_words.iter().map(|s| s.nfc().collect()).collect();
        let text = &request.text;
        let mut report = Report {
            schema_version: 1,
            request_id: request.request_id.clone(),
            document_id: request.document_id.clone(),
            revision: request.revision.clone(),
            snapshot_sha256: digest(text.as_bytes()),
            engine: self.lexicon.engine_id(),
            dictionary: self.lexicon.dictionary_id(),
            checked_words: 0,
            limited: false,
            skipped: vec![],
            diagnostics: vec![],
        };
        let code = backtick_ranges(text);
        // Request-local only: repeated text never becomes persistent user data.
        let mut memo: HashMap<String, Option<Vec<String>>> = HashMap::new();
        let mut cursor = 0;
        let mut segments = 0;
        for chunk in text.split_whitespace() {
            let start = cursor + text[cursor..].find(chunk).expect("split slice exists");
            let end = start + chunk.len();
            cursor = end;
            // Bound output and work even if all input is punctuation or protected text.
            if segments >= 512 {
                report.limited = true;
                report.skipped.push(Skipped {
                    utf8: Span { start, end: text.len() },
                    reason: "segment_limit".into(),
                });
                break;
            }
            if code.iter().any(|r| start < r.end && end > r.start) {
                report.skipped.push(Skipped {
                    utf8: Span { start, end },
                    reason: "code".into(),
                });
                segments += 1;
                continue;
            }
            // Fail closed for mixed scripts, markup, identifiers, paths and URLs.
            // This intentionally sacrifices recall; it is not a Markdown parser.
            if chunk.chars().any(|c| !hangul_input(c) && !punctuation(c)) {
                report.skipped.push(Skipped {
                    utf8: Span { start, end },
                    reason: "mixed_or_non_korean".into(),
                });
                segments += 1;
                continue;
            }
            let mut local = 0;
            for word in chunk.split(punctuation).filter(|s| !s.is_empty()) {
                let begin = start + local + chunk[local..].find(word).expect("split slice exists");
                let finish = begin + word.len();
                local = finish - start;
                segments += 1;
                if segments > 512 || report.checked_words >= MAX_WORDS || report.diagnostics.len() >= MAX_DIAGNOSTICS {
                    report.limited = true;
                    report.skipped.push(Skipped {
                        utf8: Span {
                            start: begin,
                            end: text.len(),
                        },
                        reason: "work_limit".into(),
                    });
                    return Ok(report);
                }
                let normalized: String = word.nfc().collect();
                let reason = if protected.contains(&normalized) {
                    Some("protected_word")
                } else if !normalized.chars().all(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c)) {
                    Some("uncomposed_hangul")
                } else if normalized.chars().count() > MAX_WORD_CHARS {
                    report.limited = true;
                    Some("word_length_limit")
                } else {
                    None
                };
                if let Some(reason) = reason {
                    report.skipped.push(Skipped {
                        utf8: Span {
                            start: begin,
                            end: finish,
                        },
                        reason: reason.into(),
                    });
                    continue;
                }
                report.checked_words += 1;
                let rule = self.rules.then(|| rules::lookup(&normalized)).flatten();
                let candidates = memo.entry(normalized.clone()).or_insert_with(|| {
                    if let Some(rule) = &rule {
                        Some(vec![rule.replacement.into()])
                    } else if self.lexicon.contains(&normalized) {
                        None
                    } else {
                        Some(self.lexicon.suggest(&normalized))
                    }
                });
                let Some(candidates) = candidates else {
                    continue;
                };
                let mut suggestions = Vec::new();
                for candidate in candidates.iter() {
                    let candidate: String = candidate.nfc().collect();
                    if candidate.is_empty()
                        || candidate == normalized
                        || candidate.len() > 256
                        || !candidate.chars().all(|c| c == ' ' || ('\u{ac00}'..='\u{d7a3}').contains(&c))
                        || suggestions.contains(&candidate)
                    {
                        continue;
                    }
                    suggestions.push(candidate);
                    if suggestions.len() == MAX_SUGGESTIONS {
                        break;
                    }
                }
                report.diagnostics.push(Diagnostic {
                    kind: if rule.is_some() { "rule_suggestion" } else { "unknown_word" }.into(),
                    rule_id: rule.as_ref().map(|r| r.id.into()),
                    source: rule.as_ref().map(|r| r.source.into()),
                    utf8: Span {
                        start: begin,
                        end: finish,
                    },
                    utf16: Span {
                        start: text[..begin].encode_utf16().count(),
                        end: text[..finish].encode_utf16().count(),
                    },
                    original: word.into(),
                    normalized,
                    suggestions,
                });
            }
        }
        Ok(report)
    }
}

/// Pure preview only. Hosts must also validate focus, selection, composition and undo.
/// Reports are trusted local results, not signed capabilities for remote clients.
pub fn apply_suggestion(current: &Request, report: &Report, diagnostic: usize, candidate: usize) -> Result<String, Error> {
    validate(current)?;
    if report.schema_version != 1
        || current.request_id != report.request_id
        || current.document_id != report.document_id
        || current.revision != report.revision
        || digest(current.text.as_bytes()) != report.snapshot_sha256
    {
        return Err(Error("stale_snapshot"));
    }
    let d = report.diagnostics.get(diagnostic).ok_or(Error("invalid_diagnostic"))?;
    let replacement = d.suggestions.get(candidate).ok_or(Error("invalid_candidate"))?;
    if current.text.get(d.utf8.start..d.utf8.end) != Some(d.original.as_str())
        || current.text[..d.utf8.start].encode_utf16().count() != d.utf16.start
        || current.text[..d.utf8.end].encode_utf16().count() != d.utf16.end
    {
        return Err(Error("invalid_span"));
    }
    if current.protected_words.iter().any(|s| s.nfc().eq(d.original.nfc())) {
        return Err(Error("protected_word"));
    }
    let mut preview = current.text.clone();
    preview.replace_range(d.utf8.start..d.utf8.end, replacement);
    Ok(preview)
}

fn validate(request: &Request) -> Result<(), Error> {
    if request.text.len() > MAX_TEXT_BYTES {
        return Err(Error("text_too_large"));
    }
    if request.text.contains('\0') {
        return Err(Error("nul_in_text"));
    }
    for id in [&request.request_id, &request.document_id, &request.revision] {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err(Error("invalid_identity"));
        }
    }
    if request.protected_words.len() > 256
        || request
            .protected_words
            .iter()
            .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
    {
        return Err(Error("invalid_protected_words"));
    }
    Ok(())
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hangul_input(c: char) -> bool {
    ('\u{ac00}'..='\u{d7a3}').contains(&c) || ('\u{1100}'..='\u{11ff}').contains(&c) || ('\u{3130}'..='\u{318f}').contains(&c)
}
fn punctuation(c: char) -> bool {
    ".,!?;:()[]{}\"'“”‘’…。、！？".contains(c)
}

fn backtick_ranges(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0;
    let mut open = None;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let n = i - start;
        match open {
            None => open = Some((start, n)),
            Some((begin, count)) if count == n => {
                ranges.push(Span { start: begin, end: i });
                open = None;
            }
            _ => (),
        }
    }
    if let Some((start, _)) = open {
        ranges.push(Span { start, end: text.len() });
    }
    ranges
}
