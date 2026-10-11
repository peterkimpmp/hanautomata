// Two character-statistics models score the same physical key sequence: a Korean syllable bigram model over the composed
// Hangul and an English letter bigram model over the literal letters. The log-likelihood ratio decides; a band around zero
// abstains. Tables hold aggregate counts only (no words, no sentences). Reference implementation: tests/lm/detector.py.
// Thresholds are design constants.

use crate::hangul;
use crate::resources::{gunzip, read_lines, ENGLISH_LM, KOREAN_LM};
use crate::unicode::{is_upper, to_lower, utf16};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

pub const DEFAULT_KOREAN_THRESHOLD: f64 = 2.0;
pub const DEFAULT_ENGLISH_THRESHOLD: f64 = 2.0;
pub const LEXICON_BONUS: f64 = 1.5;
pub const CONTINUITY_PRIOR: f64 = 1.5;
const STRAY_JAMO_PENALTY: f64 = 1.0;
const BOUNDARY_BACKOFF: f64 = -1.0969;
// The C# design constant, not log10(2): parity requires the same rounded value.
#[allow(clippy::approx_constant)]
const DEFAULT_BACKOFF: f64 = -0.30103;
const UNKNOWN_LETTER: f64 = -4.0;
const START: u16 = b'^' as u16;
const END: u16 = b'$' as u16;

pub(crate) struct LanguageModel {
    unigram: HashMap<u16, f64>,
    backoff: HashMap<u16, f64>,
    bigram: HashMap<u32, f64>,
    english: [[f64; 27]; 27],
    floor: f64,
}

fn parse(value: &str) -> f64 {
    value.trim().parse().expect("model value is a number")
}

fn pair(a: u16, b: u16) -> u32 {
    ((a as u32) << 16) | b as u32
}

impl LanguageModel {
    pub(crate) fn shared() -> Arc<LanguageModel> {
        static MODEL: OnceLock<Arc<LanguageModel>> = OnceLock::new();
        MODEL.get_or_init(|| Arc::new(LanguageModel::load())).clone()
    }

    fn load() -> LanguageModel {
        let mut model = LanguageModel {
            unigram: HashMap::new(),
            backoff: HashMap::new(),
            bigram: HashMap::new(),
            english: [[0.0; 27]; 27],
            floor: -8.268,
        };
        let korean = gunzip(KOREAN_LM);
        for line in read_lines(&korean) {
            if let Some(rest) = line.strip_prefix("#floor ") {
                model.floor = parse(rest);
                continue;
            }
            let units = utf16(line);
            if units.len() < 4 || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(' ').collect();
            if line.starts_with('U') && parts.len() == 4 {
                let symbol = utf16(parts[1]);
                if symbol.len() == 1 {
                    model.unigram.insert(symbol[0], parse(parts[2]));
                    model.backoff.insert(symbol[0], parse(parts[3]));
                }
            } else if line.starts_with('B') && parts.len() == 3 {
                let symbol = utf16(parts[1]);
                if symbol.len() == 2 {
                    model.bigram.insert(pair(symbol[0], symbol[1]), parse(parts[2]));
                }
            }
        }
        for line in read_lines(ENGLISH_LM) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(' ').collect();
            let row = match parts[0].chars().next() {
                Some('^') => 0,
                Some(c @ 'a'..='z') => c as usize - 'a' as usize + 1,
                _ => continue,
            };
            if parts.len() != 28 {
                continue;
            }
            for i in 0..27 {
                model.english[row][i] = parse(parts[i + 1]);
            }
        }
        assert!(
            !model.unigram.is_empty() && !model.bigram.is_empty(),
            "Korean language model empty."
        );
        model
    }

    fn transition(&self, previous: u16, current: u16) -> f64 {
        if let Some(&value) = self.bigram.get(&pair(previous, current)) {
            return value;
        }
        let weight = self.backoff.get(&previous).copied().unwrap_or(DEFAULT_BACKOFF);
        let single = self
            .unigram
            .get(&current)
            .copied()
            .unwrap_or(if current == END { BOUNDARY_BACKOFF } else { self.floor });
        weight + single
    }
}

/// Outcome of [`LanguageScorer::score`]. `llr` is log10 P_ko − log10 P_en (with the lexicon bonus on the English side).
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageVerdict {
    pub llr: f64,
    pub is_korean: bool,
    pub is_english: bool,
    pub lexical: bool,
    pub nudged: bool,
    pub composed: String,
}

impl LanguageVerdict {
    pub fn uncertain(&self) -> bool {
        !self.is_korean && !self.is_english
    }
}

/// Predicate marking a literal as a known English word (adds [`LEXICON_BONUS`] to the English side).
pub type EnglishLexicon = Box<dyn Fn(&str) -> bool + Send + Sync>;

pub struct LanguageScorer {
    model: Arc<LanguageModel>,
    lexicon: Option<EnglishLexicon>,
    korean_threshold: AtomicU64,
    english_threshold: AtomicU64,
}

/// Sensitivity presets move only the abstention band. Measured on held-out 4,000 + 4,000 words of the public-corpus tables
/// (2026-10-09): conservative T=3.0 → Korean 3,859 converted · English wrong 1; balanced T=2.0 → 3,927 · 2;
/// aggressive T=1.5 → 3,958 · 2. Returns (normalized name, Korean threshold, English threshold).
pub fn preset_thresholds(preset: Option<&str>) -> (&'static str, f64, f64) {
    let name = crate::unicode::lower_str(preset.unwrap_or("").trim());
    match name.as_str() {
        "conservative" => ("conservative", 3.0, 3.0),
        "aggressive" => ("aggressive", 1.5, 1.5),
        _ => ("balanced", DEFAULT_KOREAN_THRESHOLD, DEFAULT_ENGLISH_THRESHOLD),
    }
}

/// `Math.Max(0.5, value)` with .NET semantics: NaN propagates.
fn at_least_half(value: f64) -> f64 {
    if value.is_nan() || value > 0.5 {
        value
    } else {
        0.5
    }
}

impl LanguageScorer {
    pub fn new(lexicon: Option<EnglishLexicon>) -> Self {
        LanguageScorer {
            model: LanguageModel::shared(),
            lexicon,
            korean_threshold: AtomicU64::new(DEFAULT_KOREAN_THRESHOLD.to_bits()),
            english_threshold: AtomicU64::new(DEFAULT_ENGLISH_THRESHOLD.to_bits()),
        }
    }

    pub fn korean_threshold(&self) -> f64 {
        f64::from_bits(self.korean_threshold.load(Ordering::Relaxed))
    }
    pub fn english_threshold(&self) -> f64 {
        f64::from_bits(self.english_threshold.load(Ordering::Relaxed))
    }
    pub fn configure(&self, korean: f64, english: f64) {
        self.korean_threshold
            .store(at_least_half(korean).to_bits(), Ordering::Relaxed);
        self.english_threshold
            .store(at_least_half(english).to_bits(), Ordering::Relaxed);
    }
    pub fn bigram_count(&self) -> usize {
        self.model.bigram.len()
    }

    /// log10 P_ko of the Hangul composed from the keys, with the composed text and the number of stray jamo. Stray jamo
    /// (keys that never formed a syllable) are worse than a rare syllable.
    pub fn korean(&self, keys: &str) -> (f64, String, i32) {
        let composed = hangul::compose(keys);
        let (mut total, mut stray, mut previous) = (0.0, 0, START);
        for c in composed.encode_utf16() {
            if !(0xac00..=0xd7a3).contains(&c) {
                stray += 1;
                total += self.model.floor - STRAY_JAMO_PENALTY;
                previous = c;
                continue;
            }
            total += self.model.transition(previous, c);
            previous = c;
        }
        (total + self.model.transition(previous, END), composed, stray)
    }

    /// log10 P_en of the literal letters under a case-insensitive letter bigram model with word boundaries.
    pub fn english(&self, raw: &str) -> f64 {
        let (mut total, mut previous) = (0.0, 0usize);
        for c in raw.encode_utf16() {
            let lower = to_lower(c);
            if !(b'a' as u16..=b'z' as u16).contains(&lower) {
                total += UNKNOWN_LETTER;
                previous = 0;
                continue;
            }
            let column = (lower - b'a' as u16) as usize;
            total += self.model.english[previous][column];
            previous = column + 1;
        }
        total + self.model.english[previous][26]
    }

    pub fn score(&self, raw: &str, keys: &str, previous_korean: Option<bool>) -> LanguageVerdict {
        let (korean, composed, _) = self.korean(keys);
        let mut letters = self.english(raw);
        let lexical = self.lexicon.as_ref().is_some_and(|lexicon| lexicon(raw));
        if lexical {
            letters += LEXICON_BONUS;
        }
        let llr = korean - letters;
        let verdict = |is_korean, is_english, nudged| LanguageVerdict {
            llr,
            is_korean,
            is_english,
            lexical,
            nudged,
            composed: composed.clone(),
        };
        let units = utf16(raw);
        if units.len() >= 2 && raw == keys && units.iter().all(|&c| is_upper(c)) {
            return verdict(false, true, false);
        }
        let (korean_threshold, english_threshold) = (self.korean_threshold(), self.english_threshold());
        let (mut nudged, mut nudged_flag) = (llr, false);
        if let Some(previous) = previous_korean {
            if llr > -english_threshold && llr < korean_threshold {
                nudged = llr + if previous { CONTINUITY_PRIOR } else { -CONTINUITY_PRIOR };
                nudged_flag = true;
            }
        }
        if nudged >= korean_threshold {
            return verdict(true, false, nudged_flag);
        }
        if nudged <= -english_threshold {
            return verdict(false, true, nudged_flag);
        }
        verdict(false, false, false)
    }
}
