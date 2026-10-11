// OS-neutral input-method session: turns abstract keys into "commit this text, show this composition, consume or pass
// the key". The macOS (IMK) and Windows (TSF) shells translate their key events into `Key` and apply the `Response`.
// Key rules follow the Windows tray app (src/InputController.cs): printable ASCII joins the word, Space commits with a
// space and keeps the preceding-word context, Enter/Tab/navigation/shortcuts commit and then pass, Esc cancels.
// Switch-key semantics follow the single-shot table in the feature spec (22 §5.1a, C-MOD-02).

use crate::composition::Composition;
use crate::detector::{Decision, Detector};
use std::sync::Arc;

/// Words longer than this commit on their own, as in the Windows app.
pub const MAX_WORD: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Per-word automatic Korean/English decision (default).
    Auto,
    /// Always compose Hangul.
    Korean,
    /// No composition; keys pass to the application.
    English,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable ASCII key: `raw` is the literal character (Caps Lock and Shift applied), `key` the Dubeolsik key
    /// (Shift only), as `Native.TryAscii` produces them.
    Text {
        raw: char,
        key: char,
    },
    /// Space without a switch gesture.
    Space,
    /// A switch key (⇧Space, Caps Lock tap, …): the shell has already recognised the gesture.
    Switch,
    /// F2: the other candidate for the word being composed.
    Candidate,
    Backspace,
    Escape,
    Enter,
    Tab,
    /// Arrows, Home/End, Page Up/Down and other caret moves.
    Navigation,
    /// A key combined with ⌘/Ctrl/Alt/⌥.
    Shortcut,
    /// Any other key (function keys, …).
    Other,
}

/// What the shell must do for one key, in this order: insert `commit`, replace the composition with `marked` (empty
/// clears it), then consume the key if `handled` or pass it to the application otherwise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Response {
    pub commit: String,
    pub marked: String,
    pub handled: bool,
    /// The decision behind `marked`, for candidate and confidence display.
    pub decision: Option<Decision>,
    /// The personal dictionary changed; the shell should persist it off the key path.
    pub learned: bool,
}

pub struct Session {
    composition: Composition,
    mode: Mode,
    paused: bool,
    semicolon_shortcut: bool,
    /// Language reserved for the next word by a switch key pressed with nothing composed.
    reserved: Option<bool>,
    /// The switch key (not F2 or `;`) made the current word's choice.
    switched: bool,
}

impl Session {
    pub fn new(detector: Arc<Detector>) -> Self {
        Session {
            composition: Composition::new(detector),
            mode: Mode::Auto,
            paused: false,
            semicolon_shortcut: true,
            reserved: None,
            switched: false,
        }
    }

    pub fn detector(&self) -> &Arc<Detector> {
        self.composition.detector()
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn paused(&self) -> bool {
        self.paused
    }
    pub fn reserved(&self) -> Option<bool> {
        self.reserved
    }
    pub fn is_composing(&self) -> bool {
        !self.composition.is_empty()
    }
    pub fn set_semicolon_shortcut(&mut self, enabled: bool) {
        self.semicolon_shortcut = enabled;
    }

    /// Changes the mode; a word in progress is committed first (its text is returned).
    pub fn set_mode(&mut self, mode: Mode) -> Response {
        let response = self.commit(false, true);
        self.mode = mode;
        self.reserved = None;
        response
    }

    /// Pausing commits the word in progress; while paused every key passes untouched.
    pub fn set_paused(&mut self, paused: bool) -> Response {
        let response = self.commit(false, true);
        self.paused = paused;
        self.reserved = None;
        response
    }

    /// Focus left the text field or the input method was deactivated: commit what is composed, without context.
    pub fn commit_pending(&mut self) -> Response {
        self.reserved = None;
        self.commit(false, true)
    }

    /// Abandons the word in progress (nothing is inserted).
    pub fn cancel(&mut self) {
        self.composition.clear();
        self.reserved = None;
        self.switched = false;
    }

    pub fn handle(&mut self, key: Key) -> Response {
        if self.paused {
            return Response::default();
        }
        match key {
            Key::Text { raw, key } => self.text(raw, key),
            Key::Space => {
                if self.composition.is_empty() {
                    return Response::default();
                }
                let mut response = self.commit(true, true);
                response.commit.push(' ');
                response
            }
            Key::Switch => self.switch(),
            Key::Candidate => {
                if self.composition.is_empty() || self.mode != Mode::Auto {
                    return Response::default();
                }
                self.composition.toggle();
                self.switched = false;
                self.composing(true)
            }
            Key::Backspace => {
                if self.composition.is_empty() {
                    return Response::default();
                }
                self.composition.backspace();
                if self.composition.is_empty() {
                    self.switched = false;
                }
                self.composing(true)
            }
            Key::Escape => {
                if self.composition.is_empty() {
                    // Esc also drops a reservation, but still reaches the application (vi, dialogs).
                    self.reserved = None;
                    return Response::default();
                }
                self.cancel();
                Response {
                    handled: true,
                    ..Response::default()
                }
            }
            Key::Enter | Key::Tab | Key::Navigation | Key::Shortcut | Key::Other => {
                self.reserved = None;
                self.commit(false, false)
            }
        }
    }

    fn text(&mut self, raw: char, key: char) -> Response {
        if self.mode == Mode::English {
            return Response::default();
        }
        if raw == ';' && self.semicolon_shortcut && self.mode == Mode::Auto {
            self.composition.semicolon();
            return self.composing(true);
        }
        let starting = self.composition.is_empty();
        self.composition.append(raw, key);
        if starting {
            if let Some(korean) = self.reserved.take() {
                self.composition.force(korean);
                self.switched = true;
            }
        }
        if self.composition.len() >= MAX_WORD {
            return self.commit(false, true);
        }
        self.composing(true)
    }

    fn switch(&mut self) -> Response {
        match self.mode {
            Mode::Korean | Mode::English => {
                let mut response = self.commit(false, true);
                self.mode = if self.mode == Mode::Korean {
                    Mode::English
                } else {
                    Mode::Korean
                };
                response.handled = true;
                response
            }
            Mode::Auto if self.composition.is_empty() => {
                // Reserve the next word in the opposite language of the word just committed (Korean when unknown);
                // pressing again before typing cancels.
                self.reserved = match self.reserved {
                    Some(_) => None,
                    None => Some(!self.composition.previous_korean().unwrap_or(false)),
                };
                Response {
                    handled: true,
                    ..Response::default()
                }
            }
            Mode::Auto => {
                if self.switched {
                    self.composition.clear_choice();
                    self.switched = false;
                } else {
                    self.composition.toggle();
                    self.switched = true;
                }
                self.composing(true)
            }
        }
    }

    /// The text of the word as it would be committed now.
    fn text_now(&mut self) -> (String, Option<Decision>) {
        if self.composition.is_empty() {
            return (String::new(), None);
        }
        match self.mode {
            Mode::Korean => (self.composition.korean(), None),
            _ => {
                let decision = self.composition.current();
                (decision.text.clone(), Some(decision))
            }
        }
    }

    fn composing(&mut self, handled: bool) -> Response {
        let (marked, decision) = self.text_now();
        Response {
            commit: String::new(),
            marked,
            handled,
            decision,
            learned: false,
        }
    }

    /// Commits the word in progress. `continue_context` keeps the preceding-word context (Space only); `handled` decides
    /// whether the key that caused the commit is consumed.
    fn commit(&mut self, continue_context: bool, handled: bool) -> Response {
        if self.composition.is_empty() {
            return Response {
                handled: false,
                ..Response::default()
            };
        }
        let (text, _) = self.text_now();
        let version = self.detector().preferences().version();
        if self.mode == Mode::Korean {
            // A fixed mode is not a per-word choice: never learn from it.
            self.composition.clear();
            self.composition.reset_context(&text, continue_context);
        } else {
            self.composition.accept_commit(continue_context);
        }
        self.switched = false;
        let learned = self.detector().preferences().version() != version;
        Response {
            commit: text,
            marked: String::new(),
            handled,
            decision: None,
            learned,
        }
    }
}
