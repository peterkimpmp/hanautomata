// The word being typed: literal characters and the keys the Korean layout saw, the user's explicit choice (F2 / semicolon),
// and the preceding committed word's context. Trains the personal dictionary only on confirmed commits.
// Input-method policies on top of the C# decision (no C# counterpart, all off by default so G1 parity holds): exception
// lists (C-LEX-02), strict protection for app profiles (C-MOD-06), MS Windows consonant clusters (C-CMP-08).

use crate::detector::{Confidence, Decision, Detector};
use crate::exceptions::Exceptions;
use crate::hangul;
use crate::prefs::context_key;
use std::sync::Arc;

pub struct Composition {
    detector: Arc<Detector>,
    literal: Vec<char>,
    keys: Vec<char>,
    forced_korean: Option<bool>,
    semicolon_armed: bool,
    before_semicolon: Option<bool>,
    context: String,
    previous_korean: Option<bool>,
    confirmed: bool,
    cached: Option<Decision>,
    cached_version: (i32, u32),
    exceptions: Arc<Exceptions>,
    /// Statistical (pattern) Hangul estimates stay as typed, for terminals and code editors.
    strict: bool,
    ms_clusters: bool,
    /// Language of the text before the caret when the word started (C-DET-08, off unless the shell sets it): used like the
    /// preceding committed word, only when there is none.
    caret_korean: Option<bool>,
}

/// `Some(true)` for Hangul-only text, `Some(false)` for Latin-only text, `None` for both, neither or symbols.
fn language_of(text: &str) -> Option<bool> {
    let (mut hangul, mut latin) = (false, false);
    for c in text.chars() {
        if ('\u{ac00}'..='\u{d7a3}').contains(&c) {
            hangul = true;
        } else if c.is_ascii_alphabetic() {
            latin = true;
        }
    }
    if hangul == latin {
        None
    } else {
        Some(hangul)
    }
}

impl Composition {
    pub fn new(detector: Arc<Detector>) -> Self {
        Composition {
            detector,
            literal: Vec::new(),
            keys: Vec::new(),
            forced_korean: None,
            semicolon_armed: false,
            before_semicolon: None,
            context: String::new(),
            previous_korean: None,
            confirmed: false,
            cached: None,
            cached_version: (0, 0),
            exceptions: Arc::new(Exceptions::default()),
            strict: false,
            ms_clusters: false,
            caret_korean: None,
        }
    }

    /// Sets the caret-text language for the word about to start; cleared with the word.
    pub fn set_caret_language(&mut self, korean: Option<bool>) {
        self.caret_korean = korean;
        self.cached = None;
    }

    pub fn exceptions(&self) -> &Arc<Exceptions> {
        &self.exceptions
    }
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
        self.cached = None;
    }
    pub fn set_ms_clusters(&mut self, enabled: bool) {
        self.ms_clusters = enabled;
        self.cached = None;
    }

    pub fn detector(&self) -> &Arc<Detector> {
        &self.detector
    }
    /// Fingerprint of the word this composition committed just before ("" after Enter, cancel, navigation or symbols).
    pub fn context(&self) -> &str {
        &self.context
    }
    /// Language of the word this composition committed just before (`None` after Enter, cancel, navigation or symbols).
    pub fn previous_korean(&self) -> Option<bool> {
        self.previous_korean
    }
    pub fn is_empty(&self) -> bool {
        self.literal.is_empty()
    }
    pub fn len(&self) -> usize {
        self.literal.len()
    }
    pub fn raw(&self) -> String {
        self.literal.iter().collect()
    }
    pub fn keys(&self) -> String {
        self.keys.iter().collect()
    }

    /// The Hangul reading, whichever side it is on.
    pub fn korean(&mut self) -> String {
        let current = self.current();
        if current.is_korean {
            current.text
        } else {
            current.alternative
        }
    }

    /// The decision for the current word, cached until the word, the choice or the personal dictionary changes.
    pub fn current(&mut self) -> Decision {
        let version = (self.detector.preferences().version(), self.exceptions.version());
        if let Some(cached) = &self.cached {
            if self.cached_version == version {
                return cached.clone();
            }
        }
        self.cached_version = version;
        let (raw, keys) = (self.raw(), self.keys());
        let automatic = match self.exceptions.lookup(&raw, &keys) {
            Some(korean) => {
                let composed = hangul::compose(&keys);
                let (text, alternative, reason) = if korean {
                    (composed, raw.clone(), "항상 바꿀 단어")
                } else {
                    (raw.clone(), composed, "안 바꿀 단어")
                };
                Decision::new(text, alternative, korean, reason, Confidence::Explicit)
            }
            None => {
                let previous = self.previous_korean.or(self.caret_korean);
                let decision = self.detector.decide_with(&raw, &keys, &self.context, previous);
                if self.strict && decision.is_korean && decision.confidence == Confidence::Pattern {
                    Decision::new(
                        decision.alternative,
                        decision.text,
                        false,
                        "보호 강화 · 원문 유지",
                        Confidence::Uncertain,
                    )
                } else {
                    decision
                }
            }
        };
        let mut decision = match self.forced_korean {
            None => automatic,
            Some(forced) => {
                let same = forced == automatic.is_korean;
                let (text, alternative) = if same {
                    (automatic.text, automatic.alternative)
                } else {
                    (automatic.alternative, automatic.text)
                };
                Decision::new(text, alternative, forced, "직접 선택", Confidence::Explicit)
            }
        };
        if self.ms_clusters {
            decision.text = hangul::ms_clusters(&decision.text);
            decision.alternative = hangul::ms_clusters(&decision.alternative);
        }
        self.cached = Some(decision.clone());
        decision
    }

    pub fn append(&mut self, raw: char, key: char) {
        self.semicolon_armed = false;
        self.literal.push(raw);
        self.keys.push(key);
        self.cached = None;
    }

    /// First press flips the current word's language; a second press restores it and types a literal semicolon.
    pub fn semicolon(&mut self) {
        if self.semicolon_armed {
            self.forced_korean = self.before_semicolon;
            self.semicolon_armed = false;
            self.append(';', ';');
            return;
        }
        if self.is_empty() || {
            let current = self.current();
            current.text == current.alternative
        } {
            self.append(';', ';');
            return;
        }
        self.before_semicolon = self.forced_korean;
        self.toggle();
        self.semicolon_armed = true;
    }

    /// Called only after the input controller has accepted a commit into the checked target.
    pub fn confirm_selection(&mut self) -> bool {
        if self.confirmed || self.forced_korean.is_none() || self.is_empty() {
            return false;
        }
        self.confirmed = true;
        let (raw, keys) = (self.raw(), self.keys());
        let korean = self.current().is_korean;
        self.detector.preferences().remember(&raw, &keys, korean, &self.context)
    }

    pub fn accept_commit(&mut self, continue_context: bool) {
        let committed = self.current().text;
        self.confirm_selection();
        self.clear();
        self.reset_context(&committed, continue_context);
    }

    /// Re-establishes the preceding-word context after a commit or after the previous word was swapped.
    pub fn reset_context(&mut self, committed_text: &str, continue_context: bool) {
        self.context = if continue_context {
            context_key(committed_text)
        } else {
            String::new()
        };
        self.previous_korean = if continue_context { language_of(committed_text) } else { None };
    }

    /// Explicit choices made outside a live composition (for example F2 right after Space) still train only on confirmation.
    pub fn remember_explicit(&self, raw: &str, keys: &str, korean: bool, context: &str) -> bool {
        self.detector.preferences().remember(raw, keys, korean, context)
    }

    pub fn backspace(&mut self) {
        if self.semicolon_armed {
            self.forced_korean = self.before_semicolon;
            self.semicolon_armed = false;
            self.cached = None;
            return;
        }
        if self.is_empty() {
            return;
        }
        self.literal.pop();
        self.keys.pop();
        self.cached = None;
        if self.is_empty() {
            self.forced_korean = None;
        }
    }

    pub fn toggle(&mut self) {
        self.semicolon_armed = false;
        if !self.is_empty() {
            self.forced_korean = Some(!self.current().is_korean);
            self.cached = None;
        }
    }

    /// Input-method extension (no C# counterpart): forces the word's language, as a choice the user made explicitly
    /// (a reserved next word). Learned on a confirmed commit like F2.
    pub fn force(&mut self, korean: bool) {
        self.semicolon_armed = false;
        if !self.is_empty() {
            self.forced_korean = Some(korean);
            self.cached = None;
        }
    }

    /// Input-method extension (no C# counterpart): drops the explicit choice, returning to the automatic decision, so the
    /// word is not learned.
    pub fn clear_choice(&mut self) {
        self.semicolon_armed = false;
        self.forced_korean = None;
        self.cached = None;
    }

    /// Whether the user made an explicit choice for the current word.
    pub fn has_choice(&self) -> bool {
        self.forced_korean.is_some()
    }

    pub fn clear(&mut self) {
        self.caret_korean = None;
        self.literal.clear();
        self.keys.clear();
        self.forced_korean = None;
        self.semicolon_armed = false;
        self.before_semicolon = None;
        self.cached = None;
        self.context.clear();
        self.previous_korean = None;
        self.confirmed = false;
    }
}
