// OS-neutral input-method session: turns abstract keys into "commit this text, show this composition, consume or pass
// the key". The macOS (IMK) and Windows (TSF) shells translate their key events into `Key` and apply the `Response`.
// Key rules follow the Windows tray app (src/InputController.cs): printable ASCII joins the word, Space commits with a
// space and keeps the preceding-word context, Enter/Tab/navigation/shortcuts commit and then pass, Esc cancels.
// Switch-key semantics follow the feature spec (22 §5.1, C-MOD-02): single-shot by default (§5.1a table), or cycle /
// toggle between the modes.
// F2 right after a word committed with Space swaps that word for its other candidate and learns the choice (C-COR-02, the
// Windows app's SwapLastCommit). The shell checks that the caret is still right after the word before replacing it.

use crate::composition::Composition;
use crate::detector::{Decision, Detector};
use crate::exceptions::Exceptions;
use crate::hangul::{self, Layout};
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

/// What a switch key does (C-MOD-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchMeaning {
    /// Automatic mode: flip the word being composed, or reserve the next word; fixed modes toggle (22 §5.1a).
    SingleShot,
    /// Automatic → Korean → English → automatic.
    Cycle,
    /// Korean ↔ English; from the automatic mode, to the language opposite the current word.
    Toggle,
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

/// The word just committed with Space, as F2 can still swap it.
#[derive(Clone, Debug, PartialEq)]
struct CommittedWord {
    raw: String,
    keys: String,
    text: String,
    alternative: String,
    korean: bool,
    context: String,
}

/// A pending swap of the word just committed: the shell replaces `old` right before the caret with `new` (both include
/// the trailing space), then calls [`Session::reconvert`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconversion {
    pub old: String,
    pub new: String,
}

pub struct Session {
    composition: Composition,
    mode: Mode,
    paused: bool,
    semicolon_shortcut: bool,
    meaning: SwitchMeaning,
    layout: Layout,
    /// Backspace removes a whole Hangul syllable instead of one key (C-CMP-06).
    delete_syllable: bool,
    /// Language reserved for the next word by a switch key pressed with nothing composed.
    reserved: Option<bool>,
    /// The switch key (not F2 or `;`) made the current word's choice.
    switched: bool,
    /// The word just committed with Space; any key other than F2 forgets it.
    last: Option<CommittedWord>,
}

impl Session {
    pub fn new(detector: Arc<Detector>) -> Self {
        Session {
            composition: Composition::new(detector),
            mode: Mode::Auto,
            paused: false,
            semicolon_shortcut: true,
            meaning: SwitchMeaning::SingleShot,
            layout: Layout::Dubeolsik,
            delete_syllable: false,
            reserved: None,
            switched: false,
            last: None,
        }
    }

    pub fn detector(&self) -> &Arc<Detector> {
        self.composition.detector()
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    /// The mode keys are handled in: the automatic decision reads Dubeolsik keys, so with a Sebeolsik layout the
    /// automatic mode composes Hangul like the Korean mode.
    fn effective_mode(&self) -> Mode {
        if self.mode == Mode::Auto && self.layout != Layout::Dubeolsik {
            Mode::Korean
        } else {
            self.mode
        }
    }
    pub fn layout(&self) -> Layout {
        self.layout
    }
    /// Changing the layout commits the word in progress (its keys belong to the old layout).
    pub fn set_layout(&mut self, layout: Layout) -> Response {
        let response = self.commit(false, true);
        self.layout = layout;
        self.reserved = None;
        self.last = None;
        response
    }
    pub fn set_delete_syllable(&mut self, enabled: bool) {
        self.delete_syllable = enabled;
    }
    /// Pattern-only Hangul estimates stay as typed (app profiles for terminals and code editors).
    pub fn set_strict(&mut self, strict: bool) {
        self.composition.set_strict(strict);
    }
    pub fn set_ms_clusters(&mut self, enabled: bool) {
        self.composition.set_ms_clusters(enabled);
    }
    pub fn exceptions(&self) -> &Arc<Exceptions> {
        self.composition.exceptions()
    }
    /// The text before the caret as a word is about to start (C-DET-08): its dominant language breaks statistical
    /// near-ties when no word was committed just before. Ignored while a word is being composed.
    pub fn set_caret_text(&mut self, text: &str) {
        if self.composition.is_empty() {
            self.composition.set_caret_language(hangul::dominant_language(text));
        }
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
    pub fn switch_meaning(&self) -> SwitchMeaning {
        self.meaning
    }
    /// Changing the meaning drops a reservation the old meaning made.
    pub fn set_switch_meaning(&mut self, meaning: SwitchMeaning) {
        self.meaning = meaning;
        self.reserved = None;
    }
    /// The literal characters typed for the word in progress, as the user would see them with no conversion. The FFI
    /// layer keeps a copy before each key so an engine fault never loses them (C-CMP-11).
    pub fn raw_pending(&self) -> String {
        self.composition.raw()
    }

    /// Changes the mode; a word in progress is committed first (its text is returned).
    pub fn set_mode(&mut self, mode: Mode) -> Response {
        let response = self.commit(false, true);
        self.mode = mode;
        self.reserved = None;
        self.last = None;
        response
    }

    /// Pausing commits the word in progress; while paused every key passes untouched.
    pub fn set_paused(&mut self, paused: bool) -> Response {
        let response = self.commit(false, true);
        self.paused = paused;
        self.reserved = None;
        self.last = None;
        response
    }

    /// Focus left the text field or the input method was deactivated: commit what is composed, without context.
    pub fn commit_pending(&mut self) -> Response {
        self.reserved = None;
        self.last = None;
        self.commit(false, true)
    }

    /// Abandons the word in progress (nothing is inserted).
    pub fn cancel(&mut self) {
        self.composition.clear();
        self.reserved = None;
        self.switched = false;
        self.last = None;
    }

    /// The swap F2 would make now: the word committed by the last key (Space) and its other candidate.
    pub fn reconversion(&self) -> Option<Reconversion> {
        if self.paused || !self.composition.is_empty() {
            return None;
        }
        self.last.as_ref().map(|word| Reconversion {
            old: format!("{} ", word.text),
            new: format!("{} ", word.alternative),
        })
    }

    /// The shell replaced the word before the caret: remember the new choice in the word's context and continue from it.
    /// Calling it again swaps back (and remembers the original choice).
    pub fn reconvert(&mut self) -> Response {
        if self.paused || !self.composition.is_empty() {
            return Response::default();
        }
        let Some(word) = self.last.as_mut() else {
            return Response::default();
        };
        std::mem::swap(&mut word.text, &mut word.alternative);
        word.korean = !word.korean;
        let word = word.clone();
        let version = self.detector().preferences().version();
        self.composition
            .remember_explicit(&word.raw, &word.keys, word.korean, &word.context);
        self.composition.reset_context(&word.text, true);
        Response {
            handled: true,
            learned: self.detector().preferences().version() != version,
            ..Response::default()
        }
    }

    /// The caret is no longer right after the word (the shell could not find it).
    pub fn forget_reconversion(&mut self) {
        self.last = None;
    }

    pub fn handle(&mut self, key: Key) -> Response {
        if self.paused {
            return Response::default();
        }
        if key != Key::Candidate {
            self.last = None;
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
                if self.composition.is_empty() || self.effective_mode() != Mode::Auto {
                    return Response::default();
                }
                self.composition.toggle();
                self.switched = false;
                self.composing(true)
            }
            Key::Backspace => {
                if self.composition.is_empty() {
                    // Like every key but Space with nothing composed, the preceding-word context ends here (the Windows
                    // app's PreservePending).
                    self.composition.clear();
                    return Response::default();
                }
                if self.delete_syllable {
                    self.backspace_syllable();
                } else {
                    self.composition.backspace();
                }
                if self.composition.is_empty() {
                    self.switched = false;
                }
                self.composing(true)
            }
            Key::Escape => {
                if self.composition.is_empty() {
                    // Esc also drops a reservation and the context, but still reaches the application (vi, dialogs).
                    self.reserved = None;
                    self.composition.clear();
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
        if raw == ';' && self.semicolon_shortcut && self.effective_mode() == Mode::Auto {
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
        if self.effective_mode() != Mode::Auto || self.meaning != SwitchMeaning::SingleShot {
            let next = match (self.meaning, self.effective_mode()) {
                (SwitchMeaning::Cycle, Mode::Auto) => Mode::Korean,
                (SwitchMeaning::Cycle, Mode::Korean) => Mode::English,
                (SwitchMeaning::Cycle, Mode::English) => Mode::Auto,
                (_, Mode::Korean) => Mode::English,
                (_, Mode::English) => Mode::Korean,
                // Toggle from the automatic mode: away from the language on screen (the word being composed, else the
                // word just committed; Korean when unknown).
                (_, Mode::Auto) => {
                    let korean = if self.composition.is_empty() {
                        self.composition.previous_korean().unwrap_or(false)
                    } else {
                        self.composition.current().is_korean
                    };
                    if korean {
                        Mode::English
                    } else {
                        Mode::Korean
                    }
                }
            };
            let mut response = self.commit(false, true);
            self.mode = next;
            self.reserved = None;
            response.handled = true;
            return response;
        }
        if self.composition.is_empty() {
            // Reserve the next word in the opposite language of the word just committed (Korean when unknown);
            // pressing again before typing cancels.
            self.reserved = match self.reserved {
                Some(_) => None,
                None => Some(!self.composition.previous_korean().unwrap_or(false)),
            };
            return Response {
                handled: true,
                ..Response::default()
            };
        }
        if self.switched {
            self.composition.clear_choice();
            self.switched = false;
        } else {
            self.composition.toggle();
            self.switched = true;
        }
        self.composing(true)
    }

    /// Removes the last Hangul syllable on screen (all its keys); a word shown as typed loses one key.
    fn backspace_syllable(&mut self) {
        let korean = match self.text_now() {
            (_, Some(decision)) => decision.is_korean,
            (_, None) => true,
        };
        let before = self.composition.len();
        let shown = self.hangul_of_keys().chars().count();
        self.composition.backspace();
        if !korean || self.composition.len() == before {
            return; // one key, or the `;` choice was undone
        }
        while !self.composition.is_empty() && self.hangul_of_keys().chars().count() >= shown {
            self.composition.backspace();
        }
    }

    fn hangul_of_keys(&self) -> String {
        hangul::compose_layout(self.layout, &self.composition.keys())
    }

    /// The text of the word as it would be committed now.
    fn text_now(&mut self) -> (String, Option<Decision>) {
        if self.composition.is_empty() {
            return (String::new(), None);
        }
        match self.effective_mode() {
            Mode::Korean if self.layout != Layout::Dubeolsik => (self.hangul_of_keys(), None),
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
            if !continue_context {
                // The caret moved or focus changed: the preceding word is no longer the context.
                self.composition.clear();
            }
            return Response {
                handled: false,
                ..Response::default()
            };
        }
        let (text, decision) = self.text_now();
        let version = self.detector().preferences().version();
        if self.effective_mode() == Mode::Korean {
            // A fixed mode is not a per-word choice: never learn from it.
            self.composition.clear();
            self.composition.reset_context(&text, continue_context);
        } else {
            // Only a Space commit leaves the caret right after the word (plus its space), so only it can be swapped.
            self.last = decision
                .filter(|d| continue_context && !d.alternative.is_empty() && d.alternative != d.text)
                .map(|d| CommittedWord {
                    raw: self.composition.raw(),
                    keys: self.composition.keys(),
                    text: d.text,
                    alternative: d.alternative,
                    korean: d.is_korean,
                    context: self.composition.context().to_string(),
                });
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
