//! UniFFI surface of the Hanautomata engine for the input-method shells (macOS Swift today, Windows later).
//! One `Engine` per input-method process: the key session, the personal dictionary and its file. Key handling never
//! touches the disk; the shell calls `save()` off the key path when a result reports `learned`.
//! A panic inside the engine never crosses into the shell (C-CMP-11): the word typed so far is returned as literal text in
//! `commit`, the key passes through untouched and `failed` is set, so the shell can fall back to pass-through without
//! losing what the user typed.

use hanautomata_core::{
    bundle, hangul, hanja, parse_list, preset_thresholds, Confidence, Decision, Detector, Key,
    Layout, Mode, Response, Session, Store, SwitchMeaning, WordPreferences,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Printable ASCII; `raw` and `key` carry one character each.
    Text,
    Space,
    Switch,
    Candidate,
    Backspace,
    Escape,
    Enter,
    Tab,
    Navigation,
    Shortcut,
    Other,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Auto,
    Korean,
    English,
}

/// What a switch key does (C-MOD-02).
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchKind {
    SingleShot,
    Cycle,
    Toggle,
}

/// Korean keyboard layout (C-LAY-02). Sebeolsik composes in both Hangul modes; the automatic decision needs Dubeolsik.
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardKind {
    Dubeolsik,
    Sebeolsik390,
    SebeolsikFinal,
}

/// One remembered word for the settings window (C-LEX-03): its keys as typed, its Hangul reading, the remembered side and
/// in how many contexts it was remembered besides the global entry.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnedItem {
    pub raw: String,
    pub keys: String,
    pub hangul: String,
    pub korean: bool,
    pub contexts: u32,
}

/// Result of `import_bundle` (C-LEX-04): counts, the bundle's settings object as JSON, or the failure.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportResult {
    pub words: u32,
    pub keep: u32,
    pub convert: u32,
    pub settings: String,
    pub error: String,
}

/// Apply in order: insert `commit`, set the composition to `marked` (empty clears it), then consume the key when
/// `handled`, else pass it on.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyResult {
    pub commit: String,
    pub marked: String,
    pub handled: bool,
    /// The other candidate for the word being composed (empty when none).
    pub alternative: String,
    /// The decision is uncertain: the shell may show the alternative as a candidate.
    pub uncertain: bool,
    /// Short Korean explanation of the decision (for the candidate window and diagnostics).
    pub reason: String,
    /// Evidence label for the candidate window, as the Windows app shows it: 확인 필요 · 음절 추정 · 개인 학습 · 문맥 학습,
    /// or empty.
    pub label: String,
    /// Strength of the decision behind `marked` for diagnostics: Uncertain · Pattern · Known · Personal · Explicit, or
    /// empty without a decision.
    pub confidence: String,
    /// The word on screen is Hangul.
    pub korean: bool,
    /// The personal dictionary changed; call `save()` off the key path.
    pub learned: bool,
    /// The engine failed on this key; the key was passed through untouched.
    pub failed: bool,
}

fn label(d: &Decision) -> &'static str {
    match d.confidence {
        Confidence::Personal if d.reason.starts_with("앞 단어") => "문맥 학습",
        Confidence::Personal => "개인 학습",
        Confidence::Uncertain => "확인 필요",
        Confidence::Pattern => "음절 추정",
        _ => "",
    }
}

impl From<Response> for KeyResult {
    fn from(r: Response) -> Self {
        let (alternative, uncertain, reason, label, confidence, korean) = match &r.decision {
            Some(d) => (
                d.alternative.clone(),
                d.needs_confirmation(),
                d.reason.clone(),
                label(d).to_string(),
                d.confidence.as_str().to_string(),
                d.is_korean,
            ),
            None => (
                String::new(),
                false,
                String::new(),
                String::new(),
                String::new(),
                false,
            ),
        };
        KeyResult {
            commit: r.commit,
            marked: r.marked,
            handled: r.handled,
            alternative,
            uncertain,
            reason,
            label,
            confidence,
            korean,
            learned: r.learned,
            failed: false,
        }
    }
}

/// F2 right after a Space commit (C-COR-02): replace `old` right before the caret with `new`, then call `reconvert()`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconversion {
    pub old: String,
    pub new: String,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct Engine {
    session: Mutex<Session>,
    store: Mutex<Store>,
    preferences: Arc<WordPreferences>,
    sensitivity: Mutex<String>,
    /// Fault injection for the shell's tests: the next engine call panics.
    fail_next: AtomicBool,
}

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn single(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

impl Engine {
    fn run(&self, action: impl FnOnce(&mut Session) -> Response) -> KeyResult {
        let mut session = guard(&self.session);
        // The literal word before this key: on a fault it is committed as typed and the key itself passes, so the
        // application still receives every character in order.
        let typed = session.raw_pending();
        let fail = self.fail_next.swap(false, Ordering::Relaxed);
        match catch_unwind(AssertUnwindSafe(|| {
            if fail {
                panic!("injected engine fault");
            }
            action(&mut session)
        })) {
            Ok(response) => response.into(),
            Err(_) => {
                session.cancel();
                KeyResult {
                    commit: typed,
                    failed: true,
                    ..KeyResult::default()
                }
            }
        }
    }
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl Engine {
    /// `home` is the user's home directory (its keyed alias is restored, never guessed); `store_path` is the learning
    /// file. A missing file starts empty; an unreadable one is reported by `store_error()` and kept until a change.
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    pub fn new(home: String, store_path: String) -> Arc<Self> {
        let preferences = Arc::new(WordPreferences::new());
        let store = Store::open(store_path, &preferences);
        let detector = Arc::new(Detector::with_preferences(&home, preferences.clone()));
        Arc::new(Engine {
            session: Mutex::new(Session::new(detector)),
            store: Mutex::new(store),
            preferences,
            sensitivity: Mutex::new("balanced".into()),
            fail_next: AtomicBool::new(false),
        })
    }

    pub fn handle(&self, kind: KeyKind, raw: String, key: String) -> KeyResult {
        let key = match kind {
            KeyKind::Text => match (single(&raw), single(&key)) {
                (Some(raw), Some(key)) if raw.is_ascii_graphic() && key.is_ascii_graphic() => {
                    Key::Text { raw, key }
                }
                _ => Key::Other,
            },
            KeyKind::Space => Key::Space,
            KeyKind::Switch => Key::Switch,
            KeyKind::Candidate => Key::Candidate,
            KeyKind::Backspace => Key::Backspace,
            KeyKind::Escape => Key::Escape,
            KeyKind::Enter => Key::Enter,
            KeyKind::Tab => Key::Tab,
            KeyKind::Navigation => Key::Navigation,
            KeyKind::Shortcut => Key::Shortcut,
            KeyKind::Other => Key::Other,
        };
        self.run(|s| s.handle(key))
    }

    /// Focus left the field or the input method was deactivated.
    pub fn commit_pending(&self) -> KeyResult {
        self.run(|s| s.commit_pending())
    }

    pub fn cancel(&self) {
        guard(&self.session).cancel();
    }

    /// The swap F2 would make now, if the last key committed a word with Space.
    pub fn reconversion(&self) -> Option<Reconversion> {
        guard(&self.session).reconversion().map(|r| Reconversion {
            old: r.old,
            new: r.new,
        })
    }

    /// The shell replaced the word: remember the choice (`learned`) and continue from the new word.
    pub fn reconvert(&self) -> KeyResult {
        self.run(|s| s.reconvert())
    }

    /// The word could not be found before the caret; F2 no longer swaps it.
    pub fn forget_reconversion(&self) {
        guard(&self.session).forget_reconversion();
    }

    pub fn is_composing(&self) -> bool {
        guard(&self.session).is_composing()
    }

    /// Language reserved for the next word by a switch key (`Some(true)` = Korean).
    pub fn reserved(&self) -> Option<bool> {
        guard(&self.session).reserved()
    }

    pub fn mode(&self) -> InputMode {
        match guard(&self.session).mode() {
            Mode::Auto => InputMode::Auto,
            Mode::Korean => InputMode::Korean,
            Mode::English => InputMode::English,
        }
    }

    pub fn set_mode(&self, mode: InputMode) -> KeyResult {
        let mode = match mode {
            InputMode::Auto => Mode::Auto,
            InputMode::Korean => Mode::Korean,
            InputMode::English => Mode::English,
        };
        self.run(|s| s.set_mode(mode))
    }

    pub fn paused(&self) -> bool {
        guard(&self.session).paused()
    }

    pub fn set_paused(&self, paused: bool) -> KeyResult {
        self.run(|s| s.set_paused(paused))
    }

    pub fn set_semicolon_shortcut(&self, enabled: bool) {
        guard(&self.session).set_semicolon_shortcut(enabled);
    }

    pub fn switch_kind(&self) -> SwitchKind {
        match guard(&self.session).switch_meaning() {
            SwitchMeaning::SingleShot => SwitchKind::SingleShot,
            SwitchMeaning::Cycle => SwitchKind::Cycle,
            SwitchMeaning::Toggle => SwitchKind::Toggle,
        }
    }

    pub fn set_switch_kind(&self, kind: SwitchKind) {
        guard(&self.session).set_switch_meaning(match kind {
            SwitchKind::SingleShot => SwitchMeaning::SingleShot,
            SwitchKind::Cycle => SwitchMeaning::Cycle,
            SwitchKind::Toggle => SwitchMeaning::Toggle,
        });
    }

    /// Test hook (fault injection for C-CMP-11): the next key, commit or mode call fails as if the engine panicked.
    pub fn fail_next_call_for_testing(&self) {
        self.fail_next.store(true, Ordering::Relaxed);
    }

    pub fn sensitivity(&self) -> String {
        guard(&self.sensitivity).clone()
    }

    /// conservative · balanced · aggressive; anything else is balanced. Returns the name applied.
    pub fn set_sensitivity(&self, name: String) -> String {
        let (applied, korean, english) = preset_thresholds(Some(&name));
        guard(&self.session)
            .detector()
            .scorer()
            .configure(korean, english);
        *guard(&self.sensitivity) = applied.to_string();
        applied.to_string()
    }

    pub fn learning_enabled(&self) -> bool {
        self.preferences.enabled()
    }

    pub fn set_learning_enabled(&self, enabled: bool) {
        self.preferences.set_enabled(enabled);
    }

    pub fn learning_paused(&self) -> bool {
        !self.preferences.recording()
    }

    /// Learning pause (C-LEX-09) and secure fields (C-CTX-01): remembered choices still apply, nothing new is recorded.
    pub fn set_learning_paused(&self, paused: bool) {
        self.preferences.set_recording(!paused);
    }

    pub fn learned_count(&self) -> u32 {
        self.preferences.count() as u32
    }

    pub fn can_undo_learning(&self) -> bool {
        self.preferences.can_undo()
    }

    pub fn undo_learning(&self) -> bool {
        self.preferences.undo_last()
    }

    pub fn clear_learning(&self) {
        self.preferences.clear();
    }

    /// Persists the dictionary if it changed. Returns "" on success or nothing to do, otherwise the failure.
    pub fn save(&self) -> String {
        match guard(&self.store).flush(&self.preferences) {
            Ok(_) => String::new(),
            Err(error) => error,
        }
    }

    pub fn store_error(&self) -> String {
        guard(&self.store).last_error().to_string()
    }

    /// Hangul ↔ QWERTY layout flip of already typed text (selection conversion).
    pub fn flip(&self, text: String) -> String {
        hangul::flip(&text)
    }

    pub fn keyboard(&self) -> KeyboardKind {
        match guard(&self.session).layout() {
            Layout::Dubeolsik => KeyboardKind::Dubeolsik,
            Layout::Sebeolsik390 => KeyboardKind::Sebeolsik390,
            Layout::SebeolsikFinal => KeyboardKind::SebeolsikFinal,
        }
    }

    /// Changing the layout commits the word in progress.
    pub fn set_keyboard(&self, kind: KeyboardKind) -> KeyResult {
        let layout = match kind {
            KeyboardKind::Dubeolsik => Layout::Dubeolsik,
            KeyboardKind::Sebeolsik390 => Layout::Sebeolsik390,
            KeyboardKind::SebeolsikFinal => Layout::SebeolsikFinal,
        };
        self.run(|s| s.set_layout(layout))
    }

    /// The text before the caret as a word is about to start (C-DET-08, optional): its dominant language breaks
    /// statistical near-ties when no word was committed just before.
    pub fn set_caret_text(&self, text: String) {
        guard(&self.session).set_caret_text(&text);
    }

    /// Backspace removes a whole Hangul syllable (C-CMP-06).
    pub fn set_delete_syllable(&self, enabled: bool) {
        guard(&self.session).set_delete_syllable(enabled);
    }

    /// MS Windows consonant clusters: ㄱ+ㅅ without a vowel shows ㄳ (C-CMP-08).
    pub fn set_ms_clusters(&self, enabled: bool) {
        guard(&self.session).set_ms_clusters(enabled);
    }

    /// Strict protection (app profiles, C-MOD-06): statistical Hangul estimates stay as typed.
    pub fn set_strict(&self, strict: bool) {
        guard(&self.session).set_strict(strict);
    }

    /// Exception lists (C-LEX-02): words never to convert and words always to convert (Hangul or keys).
    pub fn set_exceptions(&self, keep: Vec<String>, convert: Vec<String>) {
        guard(&self.session).exceptions().set(&keep, &convert);
    }

    pub fn keep_list(&self) -> Vec<String> {
        guard(&self.session).exceptions().keep()
    }

    pub fn convert_list(&self) -> Vec<String> {
        guard(&self.session).exceptions().convert()
    }

    /// One entry per line; blank lines and `#` comments are skipped (the lists' text-file form).
    pub fn parse_list(&self, text: String) -> Vec<String> {
        parse_list(&text)
    }

    /// Remembered words, newest first (C-LEX-03).
    pub fn learned_words(&self) -> Vec<LearnedItem> {
        let (entries, _) = self.preferences.export();
        let mut items: Vec<LearnedItem> = Vec::new();
        for word in entries.iter().rev() {
            if let Some(item) = items
                .iter_mut()
                .find(|i| i.raw == word.raw && i.keys == word.keys)
            {
                if !word.context.is_empty() {
                    item.contexts += 1;
                }
                continue;
            }
            items.push(LearnedItem {
                raw: word.raw.clone(),
                keys: word.keys.clone(),
                hangul: hangul::compose(&word.keys),
                korean: word.korean,
                contexts: u32::from(!word.context.is_empty()),
            });
        }
        items
    }

    /// Forgets one word everywhere; returns how many entries went. Call `save()` afterwards.
    pub fn forget_word(&self, raw: String, keys: String) -> u32 {
        self.preferences.forget(&raw, &keys) as u32
    }

    /// The dictionary, the exception lists and `settings` (a JSON object of shell settings) as one bundle (C-LEX-04).
    pub fn export_bundle(&self, settings: String) -> String {
        let session = guard(&self.session);
        bundle::export(&self.preferences, session.exceptions(), &settings).unwrap_or_default()
    }

    /// Merges a bundle. The shell applies the returned settings it knows and saves.
    pub fn import_bundle(&self, text: String) -> ImportResult {
        let session = guard(&self.session);
        match bundle::import(&text, &self.preferences, session.exceptions()) {
            Ok(imported) => ImportResult {
                words: imported.words as u32,
                keep: imported.keep as u32,
                convert: imported.convert as u32,
                settings: imported.settings,
                error: String::new(),
            },
            Err(error) => ImportResult {
                error,
                ..ImportResult::default()
            },
        }
    }

    /// Hanja read as the syllable (C-LEX-06), common ones first; `extended` adds KS X 1002 and name hanja.
    pub fn hanja(&self, syllable: String, extended: bool) -> Vec<String> {
        match single(&syllable) {
            Some(c) => hanja(c, extended).into_iter().map(String::from).collect(),
            None => Vec::new(),
        }
    }
}
