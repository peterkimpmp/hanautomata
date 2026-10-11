//! UniFFI surface of the Hanautomata engine for the input-method shells (macOS Swift today, Windows later).
//! One `Engine` per input-method process: the key session, the personal dictionary and its file. Key handling never
//! touches the disk; the shell calls `save()` off the key path when a result reports `learned`.
//! A panic inside the engine never crosses into the shell: the key passes through untouched and `failed` is set
//! (C-CMP-11), so the shell can fall back to pass-through and keep the user's text.

use hanautomata_core::{
    hangul, preset_thresholds, Detector, Key, Mode, Response, Session, Store, WordPreferences,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard};

uniffi::setup_scaffolding!();

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Auto,
    Korean,
    English,
}

/// Apply in order: insert `commit`, set the composition to `marked` (empty clears it), then consume the key when
/// `handled`, else pass it on.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
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
    /// The personal dictionary changed; call `save()` off the key path.
    pub learned: bool,
    /// The engine failed on this key; the key was passed through untouched.
    pub failed: bool,
}

impl From<Response> for KeyResult {
    fn from(r: Response) -> Self {
        let (alternative, uncertain, reason) = match &r.decision {
            Some(d) => (
                d.alternative.clone(),
                d.needs_confirmation(),
                d.reason.clone(),
            ),
            None => (String::new(), false, String::new()),
        };
        KeyResult {
            commit: r.commit,
            marked: r.marked,
            handled: r.handled,
            alternative,
            uncertain,
            reason,
            learned: r.learned,
            failed: false,
        }
    }
}

#[derive(uniffi::Object)]
pub struct Engine {
    session: Mutex<Session>,
    store: Mutex<Store>,
    preferences: Arc<WordPreferences>,
    sensitivity: Mutex<String>,
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
        match catch_unwind(AssertUnwindSafe(|| action(&mut session))) {
            Ok(response) => response.into(),
            Err(_) => {
                session.cancel();
                KeyResult {
                    failed: true,
                    ..KeyResult::default()
                }
            }
        }
    }
}

#[uniffi::export]
impl Engine {
    /// `home` is the user's home directory (its keyed alias is restored, never guessed); `store_path` is the learning
    /// file. A missing file starts empty; an unreadable one is reported by `store_error()` and kept until a change.
    #[uniffi::constructor]
    pub fn new(home: String, store_path: String) -> Arc<Self> {
        let preferences = Arc::new(WordPreferences::new());
        let store = Store::open(store_path, &preferences);
        let detector = Arc::new(Detector::with_preferences(&home, preferences.clone()));
        Arc::new(Engine {
            session: Mutex::new(Session::new(detector)),
            store: Mutex::new(store),
            preferences,
            sensitivity: Mutex::new("balanced".into()),
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
}
