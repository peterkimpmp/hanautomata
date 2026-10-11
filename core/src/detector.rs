// Per-word Korean/English decision: protected literals → developer tokens → per-atom lexical rules → mixed-word decoder →
// chat → statistics → word lists, then a confirmed personal choice overrides the default.

use crate::hangul;
use crate::lists::{CHAT_ABBREVIATIONS, DEVELOPER_TOKENS, HADA_ENDINGS, HADA_NOUNS, KOREAN, NOUNS, PARTICLES};
use crate::prefs::WordPreferences;
use crate::resources::{gunzip, read_lines, ENGLISH, ENGLISH_WORDS, KOREAN_WORDS};
use crate::scorer::LanguageScorer;
use crate::unicode::{fold_key, is_letter, is_lower, is_upper, lower_str, starts_with_ignore_case, string, to_lower, utf16};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

/// Ordered from weakest to strongest evidence; a word's confidence is the weakest of its atoms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    Uncertain,
    Pattern,
    Known,
    Personal,
    Explicit,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Uncertain => "Uncertain",
            Confidence::Pattern => "Pattern",
            Confidence::Known => "Known",
            Confidence::Personal => "Personal",
            Confidence::Explicit => "Explicit",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub text: String,
    pub alternative: String,
    pub is_korean: bool,
    pub reason: String,
    pub confidence: Confidence,
}

impl Decision {
    pub(crate) fn new(
        text: String,
        alternative: String,
        is_korean: bool,
        reason: impl Into<String>,
        confidence: Confidence,
    ) -> Self {
        Decision {
            text,
            alternative,
            is_korean,
            reason: reason.into(),
            confidence,
        }
    }
    pub fn needs_confirmation(&self) -> bool {
        self.confidence == Confidence::Uncertain
    }
}

/// Word lists act only inside the scorer's abstention band: a listed Korean word with the Korean side ahead by this margin
/// converts; a listed English word stays English without a prompt.
pub const WORD_LIST_MARGIN: f64 = 1.0;
pub const MIXED_KEY_LIMIT: usize = 64;
pub const MIXED_PART_LIMIT: usize = 8;
pub const MIXED_MARGIN: i32 = 3;
const DEVELOPER_REASON: &str = "개발 도구·약어 유지";
const EDGE_PUNCTUATION: &str = ".,!?;:()[]{}\"'";
const ENGLISH_SUFFIXES: [&str; 5] = ["s", "es", "ed", "ing", "ly"];

#[derive(Clone)]
struct Edge {
    end: usize,
    language: usize,
    cost: i32,
    text: String,
    particle: bool,
}

#[derive(Clone, Default)]
struct Node {
    next: HashMap<u16, u32>,
    text: Option<String>,
    cost: i32,
}

/// Key trie; on equal cost the first text added wins.
#[derive(Clone)]
struct Trie {
    nodes: Vec<Node>,
}

impl Trie {
    fn new() -> Self {
        Trie {
            nodes: vec![Node::default()],
        }
    }

    fn add(&mut self, keys: &str, text: &str, cost: i32) {
        let mut node = 0usize;
        for c in keys.encode_utf16() {
            node = match self.nodes[node].next.get(&c) {
                Some(&next) => next as usize,
                None => {
                    let id = self.nodes.len();
                    self.nodes.push(Node::default());
                    self.nodes[node].next.insert(c, id as u32);
                    id
                }
            };
        }
        let node = &mut self.nodes[node];
        if node.text.is_none() || cost < node.cost {
            node.text = Some(text.to_string());
            node.cost = cost;
        }
    }

    fn matches(&self, input: &[u16], raw: &[u16], start: usize, language: usize, particle: bool, edges: &mut Vec<Edge>) {
        let mut node = 0usize;
        for end in start..input.len() {
            let mut c = input[end];
            if language == 0 || !matches!(c, 0x52 | 0x45 | 0x51 | 0x54 | 0x57 | 0x4f | 0x50) {
                c = to_lower(c);
            }
            match self.nodes[node].next.get(&c) {
                Some(&next) => node = next as usize,
                None => break,
            }
            if let Some(text) = &self.nodes[node].text {
                if end > start {
                    edges.push(Edge {
                        end: end + 1,
                        language,
                        cost: self.nodes[node].cost,
                        particle,
                        text: if language == 0 {
                            string(&raw[start..=end])
                        } else {
                            text.clone()
                        },
                    });
                }
            }
        }
    }
}

#[derive(Clone)]
struct MixedPath {
    text: String,
    cost: i32,
}

/// Two distinct surfaces with the lowest costs.
#[derive(Clone, Default)]
struct BestTwo {
    first: Option<MixedPath>,
    second: Option<MixedPath>,
}

impl BestTwo {
    fn add(&mut self, item: MixedPath) {
        if let Some(first) = &self.first {
            if first.text == item.text {
                if item.cost < first.cost {
                    self.first = Some(item);
                }
                return;
            }
        }
        if let Some(second) = &self.second {
            if second.text == item.text {
                if item.cost >= second.cost {
                    return;
                }
                self.second = None;
            }
        }
        if self.first.as_ref().is_none_or(|first| item.cost < first.cost) {
            self.second = self.first.take();
            self.first = Some(item);
        } else if self.second.as_ref().is_none_or(|second| item.cost < second.cost) {
            self.second = Some(item);
        }
    }
}

/// An insertion-ordered set (the C# engine enumerates HashSet/Dictionary in insertion order; trie ties depend on it).
#[derive(Default)]
struct OrderedSet {
    items: Vec<String>,
    index: HashSet<String>,
}

impl OrderedSet {
    fn add(&mut self, value: String) {
        if self.index.insert(value.clone()) {
            self.items.push(value);
        }
    }
    fn contains(&self, value: &str) -> bool {
        self.index.contains(value)
    }
}

/// Profile-independent vocabularies, models and tries, built once per process.
struct Shared {
    english: Vec<String>,
    english_folded: HashSet<String>,
    korean: OrderedSet,
    particles: Vec<(String, String)>,
    morphology: OrderedSet,
    korean_words: HashSet<String>,
    english_words: HashSet<String>,
    developer: HashSet<String>,
    chat: HashSet<&'static str>,
    english_trie: Arc<Trie>,
    korean_trie: Arc<Trie>,
    particle_trie: Arc<Trie>,
}

fn word_list(bytes: &[u8]) -> HashSet<String> {
    let text = gunzip(bytes);
    read_lines(&text)
        .into_iter()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.trim().to_string())
        .collect()
}

/// Small, typed surface-form rule, not a full Korean morphological analyzer.
fn particle_fits(noun: &str, particle: &str) -> bool {
    let last = noun.encode_utf16().last().unwrap_or(0) as i32;
    let last_final = (last - 0xac00) % 28;
    match particle {
        "은" | "이" | "을" | "과" => last_final != 0,
        "는" | "가" | "를" | "와" => last_final == 0,
        "으로" | "으로부터" => last_final != 0 && last_final != 8,
        "로" => last_final == 0 || last_final == 8,
        _ => true,
    }
}

impl Shared {
    fn get() -> Arc<Shared> {
        static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(Shared::build())).clone()
    }

    fn build() -> Shared {
        let mut english = Vec::new();
        let mut english_folded = HashSet::new();
        for word in ENGLISH.split_whitespace() {
            if english_folded.insert(fold_key(word)) {
                english.push(word.to_string());
            }
        }
        let mut korean = OrderedSet::default();
        for word in KOREAN {
            korean.add(word.to_string());
        }
        let mut particles: Vec<(String, String)> = Vec::new();
        for particle in PARTICLES {
            let keys = hangul::to_keys(particle);
            match particles.iter_mut().find(|(k, _)| *k == keys) {
                Some(entry) => entry.1 = particle.to_string(),
                None => particles.push((keys, particle.to_string())),
            }
        }
        let mut morphology = OrderedSet::default();
        for noun in NOUNS {
            for (_, particle) in &particles {
                if particle_fits(noun, particle) {
                    morphology.add(format!("{noun}{particle}"));
                }
            }
        }
        for stem in HADA_NOUNS {
            for ending in HADA_ENDINGS {
                morphology.add(format!("{stem}{ending}"));
            }
        }
        let mut korean_trie = Trie::new();
        for word in &korean.items {
            if word.encode_utf16().count() >= 2 {
                korean_trie.add(&hangul::to_keys(word), word, 10);
            }
        }
        for word in &morphology.items {
            korean_trie.add(&hangul::to_keys(word), word, 13);
        }
        let mut particle_trie = Trie::new();
        for (keys, particle) in &particles {
            particle_trie.add(keys, particle, 8);
        }
        let mut english_trie = Trie::new();
        for word in &english {
            let lower = lower_str(word);
            english_trie.add(&lower, &lower, 10);
            let length = lower.encode_utf16().count();
            for suffix in ENGLISH_SUFFIXES {
                if length > 2 {
                    let form = format!("{lower}{suffix}");
                    english_trie.add(&form, &form, 10);
                }
                if length > 3 && lower.ends_with('e') {
                    let form = format!("{}{suffix}", &lower[..lower.len() - 1]);
                    english_trie.add(&form, &form, 10);
                }
            }
        }
        Shared {
            english,
            english_folded,
            korean,
            particles,
            morphology,
            korean_words: word_list(KOREAN_WORDS),
            english_words: word_list(ENGLISH_WORDS),
            developer: DEVELOPER_TOKENS.iter().map(|token| fold_key(token)).collect(),
            chat: CHAT_ABBREVIATIONS.iter().copied().collect(),
            english_trie: Arc::new(english_trie),
            korean_trie: Arc::new(korean_trie),
            particle_trie: Arc::new(particle_trie),
        }
    }

    fn is_english(&self, word: &str) -> bool {
        if self.english_folded.contains(&fold_key(word)) {
            return true;
        }
        let lower: Vec<u16> = word.encode_utf16().map(to_lower).collect();
        for suffix in ENGLISH_SUFFIXES {
            let suffix = utf16(suffix);
            if lower.len() > suffix.len() + 2 && lower.ends_with(&suffix) {
                let stem = string(&lower[..lower.len() - suffix.len()]);
                if self.english_folded.contains(&fold_key(&stem)) || self.english_folded.contains(&fold_key(&format!("{stem}e")))
                {
                    return true;
                }
            }
        }
        false
    }

    /// Requires known English components; Shift also produces Korean double consonants.
    fn is_english_identifier(&self, word: &[u16]) -> bool {
        let (mut start, mut parts) = (0, 0);
        for i in 1..word.len() {
            let boundary = is_upper(word[i])
                && (is_lower(word[i - 1]) || (i + 1 < word.len() && is_upper(word[i - 1]) && is_lower(word[i + 1])));
            if !boundary {
                continue;
            }
            if !self.is_english(&string(&word[start..i])) {
                return false;
            }
            start = i;
            parts += 1;
        }
        parts > 0 && self.is_english(&string(&word[start..]))
    }
}

/// The detector: shared vocabularies plus this user's home-directory alias, personal choices and sensitivity.
pub struct Detector {
    preferences: Arc<WordPreferences>,
    profile_path: Vec<u16>,
    profile_keys: Vec<u16>,
    shared: Arc<Shared>,
    english_trie: Arc<Trie>,
    korean_trie: Arc<Trie>,
    scorer: LanguageScorer,
}

fn is_ascii_letter(c: u16) -> bool {
    (b'a' as u16..=b'z' as u16).contains(&c) || (b'A' as u16..=b'Z' as u16).contains(&c)
}

fn is_syllable(c: u16) -> bool {
    (0xac00..=0xd7a3).contains(&c)
}

fn is_chat_jamo(c: u16) -> bool {
    matches!(char::from_u32(c as u32), Some('ㅋ' | 'ㅎ' | 'ㅠ' | 'ㅜ'))
}

/// `TokenParts`: maximal runs of ASCII letters and of everything else, as (start, end) code-unit ranges.
fn token_parts(raw: &[u16]) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..=raw.len() {
        if i == raw.len() || is_ascii_letter(raw[i]) != is_ascii_letter(raw[start]) {
            parts.push((start, i));
            start = i;
        }
    }
    parts
}

impl Detector {
    /// `user_profile` is the OS home directory (for example `C:\Users\홍길동` or `/Users/홍길동`); only its exact keyed
    /// alias is restored, never guessed path names.
    pub fn new(user_profile: &str) -> Self {
        Self::with_preferences(user_profile, Arc::new(WordPreferences::new()))
    }

    pub fn with_preferences(user_profile: &str, preferences: Arc<WordPreferences>) -> Self {
        let profile = user_profile.trim_end_matches(['\\', '/']);
        let shared = Shared::get();
        let lexicon_shared = shared.clone();
        Detector {
            preferences,
            profile_path: utf16(profile),
            profile_keys: utf16(&hangul::to_keys(profile)),
            english_trie: shared.english_trie.clone(),
            korean_trie: shared.korean_trie.clone(),
            scorer: LanguageScorer::new(Some(Box::new(move |word| lexicon_shared.is_english(word)))),
            shared,
        }
    }

    pub fn preferences(&self) -> &Arc<WordPreferences> {
        &self.preferences
    }
    pub fn scorer(&self) -> &LanguageScorer {
        &self.scorer
    }
    pub fn korean_word_count(&self) -> usize {
        self.shared.korean_words.len()
    }
    pub fn english_word_count(&self) -> usize {
        self.shared.english_words.len()
    }

    /// Synthetic colliding lexemes exercise the decision contract without production data (tests only).
    #[doc(hidden)]
    pub fn add_lexeme_for_test(&mut self, surface: &str, is_korean: bool, cost: i32) {
        if is_korean {
            Arc::make_mut(&mut self.korean_trie).add(&hangul::to_keys(surface), surface, cost);
        } else {
            Arc::make_mut(&mut self.english_trie).add(&lower_str(surface), surface, cost);
        }
    }

    /// Vocabularies in enumeration order, for the golden `lists` check (tests only).
    #[doc(hidden)]
    pub fn vocabularies_for_test(&self) -> Vec<(&'static str, Vec<String>)> {
        let s = &self.shared;
        vec![
            ("korean", s.korean.items.clone()),
            ("morphology", s.morphology.items.clone()),
            ("particles", s.particles.iter().map(|(k, v)| format!("{k}\t{v}")).collect()),
            ("developer", DEVELOPER_TOKENS.iter().map(|t| t.to_string()).collect()),
            ("chat", CHAT_ABBREVIATIONS.iter().map(|t| t.to_string()).collect()),
            ("english", s.english.clone()),
        ]
    }

    pub fn decide(&self, raw: &str) -> Decision {
        self.decide_with(raw, raw, "", None)
    }

    /// `previous_korean`: language of the word this app committed just before; it only breaks statistical near-ties.
    /// `hangul_keys` must have the same UTF-16 length as `raw` (the C# engine throws otherwise); a mismatch falls back to
    /// `raw` as the keys.
    pub fn decide_with(&self, raw: &str, hangul_keys: &str, context: &str, previous_korean: Option<bool>) -> Decision {
        let keys = if raw.encode_utf16().count() == hangul_keys.encode_utf16().count() {
            hangul_keys
        } else {
            raw
        };
        let automatic = self.decide_default(raw, keys, previous_korean);
        let lookup = self.preferences.try_get(raw, keys, context);
        if !lookup.found || automatic.text == automatic.alternative {
            if lookup.conflict {
                let reason = format!("선택 이력 충돌 · {}", automatic.reason);
                return Decision::new(
                    automatic.text,
                    automatic.alternative,
                    automatic.is_korean,
                    reason,
                    Confidence::Uncertain,
                );
            }
            return automatic;
        }
        let same = lookup.korean == automatic.is_korean;
        let reason = if lookup.context_match {
            "앞 단어에 맞춘 개인 선택"
        } else {
            "기억한 개인 선택"
        };
        let (text, alternative) = if same {
            (automatic.text, automatic.alternative)
        } else {
            (automatic.alternative, automatic.text)
        };
        Decision::new(text, alternative, lookup.korean, reason, Confidence::Personal)
    }

    fn decide_default(&self, raw_text: &str, keys_text: &str, previous_korean: Option<bool>) -> Decision {
        let raw = utf16(raw_text);
        let keys = utf16(keys_text);
        // Restore only the exact OS-provided home-directory alias; never guess path names.
        let (profile, alias) = (&self.profile_path, &self.profile_keys);
        if !profile.is_empty()
            && profile != alias
            && starts_with_ignore_case(&raw, alias)
            && (raw.len() == alias.len() || raw[alias.len()] == b'\\' as u16 || raw[alias.len()] == b'/' as u16)
        {
            let mut text = profile.clone();
            text.extend_from_slice(&raw[alias.len()..]);
            return Decision::new(
                string(&text),
                raw_text.to_string(),
                true,
                "현재 사용자 홈 경로 복원",
                Confidence::Known,
            );
        }
        let body = raw_text.trim_matches(|c| EDGE_PUNCTUATION.contains(c));
        let body16 = utf16(body);
        if body.contains('\\')
            || body.starts_with('/')
            || (body16.len() >= 2 && is_letter(body16[0]) && body16[1] == b':' as u16)
            || body.contains('@')
            || body.contains('_')
            || body.contains('.')
            || body.contains("://")
        {
            return Decision::new(
                raw_text.to_string(),
                hangul::compose(keys_text),
                false,
                "주소·파일명·식별자 유지",
                Confidence::Known,
            );
        }
        // Check complete tokens before letter/digit segmentation (sha256, k8s, OAuth2).
        if self.shared.developer.contains(&fold_key(body)) {
            return Decision::new(
                raw_text.to_string(),
                hangul::compose(keys_text),
                false,
                DEVELOPER_REASON,
                Confidence::Known,
            );
        }
        let (mut output, mut alternative) = (String::new(), String::new());
        let mut changed = false;
        let mut reason = String::from("기호 유지");
        let mut confidence = Confidence::Known;
        for (start, end) in token_parts(&raw) {
            let part = string(&raw[start..end]);
            if !is_ascii_letter(raw[start]) {
                output.push_str(&part);
                alternative.push_str(&part);
                continue;
            }
            let decision = self.decide_atom(&raw[start..end], &keys[start..end], previous_korean);
            output.push_str(&decision.text);
            alternative.push_str(&decision.alternative);
            confidence = confidence.min(decision.confidence);
            changed |= decision.is_korean;
            reason = decision.reason;
        }
        Decision::new(
            output,
            if changed { raw_text.to_string() } else { alternative },
            changed,
            reason,
            confidence,
        )
    }

    fn is_korean_chat(&self, word: &[u16], raw: &[u16], keys: &[u16]) -> bool {
        // Extend standalone chat only. Arbitrary consonants or a suffix attached to English are not chat words.
        let repeated =
            word.len() >= 2 && "ㄷㅇㄴㄱㅂㅅㅈㅊ".encode_utf16().any(|c| c == word[0]) && word.iter().all(|&c| c == word[0]);
        if self.shared.chat.contains(string(word).as_str()) || repeated {
            return true;
        }
        let mut prefix = word.len();
        while prefix > 0 && is_chat_jamo(word[prefix - 1]) {
            prefix -= 1;
        }
        if prefix == word.len() {
            return false;
        }
        if prefix == 0 {
            return word.len() >= 2;
        }
        if self.shared.korean.contains(&string(&word[..prefix])) {
            return true;
        }
        // Every trailing chat jamo came from exactly one key, so the body's keys are the remaining prefix.
        let key_length = keys.len() as isize - (word.len() - prefix) as isize;
        key_length > 0 && key_length as usize <= raw.len() && {
            let n = key_length as usize;
            self.scorer.score(&string(&raw[..n]), &string(&keys[..n]), None).is_korean
        }
    }

    /// A bounded DAG decoder. Keeps two distinct surfaces per complete state, including span count: pruning a longer state
    /// must not discard a shorter feasible completion.
    fn mixed_word(&self, raw: &[u16], keys: &[u16]) -> Option<Decision> {
        let n = raw.len();
        if !(4..=MIXED_KEY_LIMIT).contains(&n) {
            return None;
        }
        const LANGS: usize = 2;
        const MASKS: usize = 4;
        const PARTS: usize = MIXED_PART_LIMIT + 1;
        let at = |end: usize, lang: usize, mask: usize, parts: usize| ((end * LANGS + lang) * MASKS + mask) * PARTS + parts;
        let mut paths: Vec<Option<BestTwo>> = vec![None; (n + 1) * LANGS * MASKS * PARTS];
        let mut origin = BestTwo::default();
        origin.add(MixedPath {
            text: String::new(),
            cost: 0,
        });
        paths[at(0, 0, 0, 0)] = Some(origin);
        for start in 0..n {
            let mut edges: Option<Vec<Edge>> = None;
            for lang in 0..LANGS {
                for mask in 0..MASKS {
                    for parts in 0..MIXED_PART_LIMIT {
                        let Some(previous) = paths[at(start, lang, mask, parts)].clone() else {
                            continue;
                        };
                        let edges = edges.get_or_insert_with(|| {
                            let mut edges = Vec::new();
                            self.english_trie.matches(raw, raw, start, 0, false, &mut edges);
                            self.korean_trie.matches(keys, raw, start, 1, false, &mut edges);
                            self.shared.particle_trie.matches(keys, raw, start, 1, true, &mut edges);
                            // Known camel/PascalCase components retain their literal case.
                            for end in start + 3..=n {
                                if self.shared.is_english_identifier(&raw[start..end]) {
                                    edges.push(Edge {
                                        end,
                                        language: 0,
                                        cost: 12,
                                        text: string(&raw[start..end]),
                                        particle: false,
                                    });
                                }
                            }
                            edges
                        });
                        for edge in edges.iter() {
                            if (mask != 0 && edge.language == lang) || (edge.particle && (mask == 0 || lang != 0)) {
                                continue;
                            }
                            let next_mask = mask | if edge.language == 0 { 1 } else { 2 };
                            let next =
                                paths[at(edge.end, edge.language, next_mask, parts + 1)].get_or_insert_with(BestTwo::default);
                            for path in [&previous.first, &previous.second].into_iter().flatten() {
                                next.add(MixedPath {
                                    text: format!("{}{}", path.text, edge.text),
                                    cost: path.cost + edge.cost + if parts == 0 { 0 } else { 2 },
                                });
                            }
                        }
                    }
                }
            }
        }
        let mut winners = BestTwo::default();
        for lang in 0..LANGS {
            for parts in 2..=MIXED_PART_LIMIT {
                if let Some(pair) = &paths[at(n, lang, 3, parts)] {
                    if let Some(first) = &pair.first {
                        winners.add(first.clone());
                    }
                    if let Some(second) = &pair.second {
                        winners.add(second.clone());
                    }
                }
            }
        }
        let first = winners.first?;
        let raw_text = string(raw);
        // These costs are hand-set ranking evidence, not calibrated probabilities.
        if winners.second.is_some_and(|second| second.cost - first.cost < MIXED_MARGIN) {
            return Some(Decision::new(
                raw_text,
                first.text,
                false,
                "혼합어 후보 경합 · 확인 필요",
                Confidence::Uncertain,
            ));
        }
        Some(Decision::new(
            first.text,
            raw_text,
            true,
            "어휘·활용형 비용으로 나눈 한영 혼합어",
            Confidence::Known,
        ))
    }

    fn decide_atom(&self, raw: &[u16], keys: &[u16], previous_korean: Option<bool>) -> Decision {
        let raw_text = string(raw);
        let keys_text = string(keys);
        let composed = hangul::compose(&keys_text);
        let ko_body = utf16(&composed);
        let body = raw;
        let mut korean_choice = false;
        let mut reason = "모호한 입력 · 원문 유지";
        let mut confidence = Confidence::Known;
        let technical = body.iter().any(|&c| !is_letter(c));
        let shared = &self.shared;
        if body.is_empty() || technical {
            reason = "기호·주소·식별자 유지";
        } else if shared.developer.contains(&fold_key(&raw_text)) {
            reason = DEVELOPER_REASON;
        } else if shared.is_english(&raw_text) {
            reason = "영어 단어 유지";
        } else if shared.is_english_identifier(body) {
            reason = "영어 식별자 유지";
        }
        // Caps Lock changes literal case, but Korean keys retain the actual Shift state.
        // Keep deliberate Shift acronyms; do not suppress Korean merely because Caps Lock is on.
        else if body.len() >= 2 && body.iter().all(|&c| is_upper(c)) && raw == keys {
            reason = "영문 약어 유지";
        }
        // Bare one-syllable words can be near-ties; let statistics and continuity decide them.
        else if ko_body.len() > 1 && shared.korean.contains(&composed) {
            korean_choice = true;
            reason = "자주 쓰는 한국어";
        } else if shared.morphology.contains(&composed) {
            korean_choice = true;
            reason = "한국어 조사·활용형";
        } else if let Some(mixed) = self.mixed_word(raw, keys) {
            return mixed;
        } else if self.is_korean_chat(&ko_body, raw, keys) {
            korean_choice = true;
            reason = "한국어 채팅 표현";
        } else {
            // Statistical fallback for everything the hand lists miss (most one- and two-syllable words).
            // The previous committed word's language only breaks near-ties; clear evidence is never overridden.
            // Model-based results are reported as a pattern estimate, not lexical certainty.
            let verdict = self.scorer.score(&raw_text, &keys_text, previous_korean);
            let lower_body = lower_str(&raw_text);
            if verdict.is_korean {
                korean_choice = true;
                reason = if verdict.nudged {
                    "앞 단어 문맥 · 음절 통계"
                } else {
                    "음절 통계"
                };
                confidence = Confidence::Pattern;
            } else if verdict.is_english {
                reason = if verdict.nudged {
                    "앞 단어 문맥 · 영문 통계"
                } else {
                    "영문 통계"
                };
            }
            // Inside the band, a real word tips the balance: listed Korean (2+ syllables, Korean side ahead) converts,
            // a listed English word (3+ letters) stays literal without asking. One-syllable words stay statistical.
            else if ko_body.len() >= 2
                && ko_body.iter().all(|&c| is_syllable(c))
                && verdict.llr >= WORD_LIST_MARGIN
                && shared.korean_words.contains(&composed)
                && !shared.english_words.contains(&lower_body)
            {
                korean_choice = true;
                reason = "단어 목록 · 음절 통계";
                confidence = Confidence::Pattern;
            } else if body.len() >= 3 && shared.english_words.contains(&lower_body) && !shared.korean_words.contains(&composed) {
                reason = "영어 단어 목록";
            } else {
                confidence = Confidence::Uncertain;
            }
        }
        let (text, alternative) = if korean_choice {
            (composed, raw_text)
        } else {
            (raw_text, composed)
        };
        Decision::new(text, alternative, korean_choice, reason, confidence)
    }
}
