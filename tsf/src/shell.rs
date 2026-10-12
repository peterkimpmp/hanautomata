//! The Windows key logic over the shared engine. It knows virtual keys and modifier state, not TSF: the text service turns
//! each TSF key event into one of these calls and applies the returned operations in a single edit session, so the same
//! logic runs in tests on any OS. Where the behaviour is shared it follows the macOS shell (InputCore).
//!
//! Keys: printable keys on the US positions (the Korean keyboard shares them), Space/Enter/Tab/Backspace/Esc, cursor keys,
//! Ctrl/Alt/Win combinations. Switch keys (C-MOD-03 · W-04): 한/영, a right Alt tap, ⇧Space and a short Caps Lock press;
//! a long Caps Lock press (or Shift+Caps Lock) keeps the real Caps Lock (C-MOD-10). F2 swaps the word just committed
//! (C-COR-02), Shift+F2 flips the selection (C-COR-03), F12 pauses (C-MOD-04, as in the tray app).

use hanautomata_ffi::{Engine, InputMode, KeyKind, KeyResult};
use std::sync::Arc;

/// Virtual-key codes used here (winuser.h).
pub mod vk {
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const RETURN: u16 = 0x0D;
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12;
    pub const CAPITAL: u16 = 0x14;
    pub const HANGUL: u16 = 0x15;
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;
    pub const PRIOR: u16 = 0x21;
    pub const NEXT: u16 = 0x22;
    pub const END: u16 = 0x23;
    pub const HOME: u16 = 0x24;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const INSERT: u16 = 0x2D;
    pub const DELETE: u16 = 0x2E;
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;
    pub const F2: u16 = 0x71;
    pub const F12: u16 = 0x7B;
    pub const LSHIFT: u16 = 0xA0;
    pub const RSHIFT: u16 = 0xA1;
    pub const LCONTROL: u16 = 0xA2;
    pub const RCONTROL: u16 = 0xA3;
    pub const LMENU: u16 = 0xA4;
    pub const RMENU: u16 = 0xA5;
}

/// Modifier state when the key arrived (`caps` is the Caps Lock toggle).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub win: bool,
    pub caps: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        shift: false,
        ctrl: false,
        alt: false,
        win: false,
        caps: false,
    };
    pub const SHIFT: Mods = Mods {
        shift: true,
        ctrl: false,
        alt: false,
        win: false,
        caps: false,
    };
    pub const CTRL: Mods = Mods {
        shift: false,
        ctrl: true,
        alt: false,
        win: false,
        caps: false,
    };

    fn chord(&self) -> bool {
        self.ctrl || self.alt || self.win
    }
}

/// One key event as the text service sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub vk: u16,
    /// The extended-key flag (lParam bit 24): tells the right Alt from the left one when the key arrives as VK_MENU.
    pub extended: bool,
    pub mods: Mods,
    /// Auto-repeat (lParam bit 30, the key was already down).
    pub repeat: bool,
    /// Milliseconds on any monotonic clock (the message time), for the Caps Lock long press.
    pub time: u64,
}

impl KeyEvent {
    pub fn new(vk: u16, mods: Mods) -> KeyEvent {
        KeyEvent {
            vk,
            extended: false,
            mods,
            repeat: false,
            time: 0,
        }
    }

    fn is_right_alt(&self) -> bool {
        self.vk == vk::RMENU || (self.vk == vk::MENU && self.extended)
    }
}

/// What the document must do, in order, before the key is eaten or passed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Insert at the caret, replacing the composition (which ends).
    Commit(String),
    /// Show as the composition; empty removes it.
    Compose(String),
    /// Replace `old` right before the caret with `new` if it is still there; then call `Shell::reconverted` (F2).
    ReplaceBefore { old: String, new: String },
    /// Read the selection, pass it to `Shell::flip` and put the result in its place (Shift+F2).
    FlipSelection,
    /// Send one Caps Lock press so the toggle ends where the gesture wants it (W-22; the sender calls
    /// `Shell::expect_injected_caps_lock` first).
    ToggleCapsLock,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Consume the key; otherwise the application receives it after the operations.
    pub eaten: bool,
    pub ops: Vec<Op>,
    /// The personal dictionary changed.
    pub learned: bool,
}

impl Outcome {
    fn pass() -> Outcome {
        Outcome::default()
    }

    fn eaten() -> Outcome {
        Outcome {
            eaten: true,
            ..Outcome::default()
        }
    }
}

/// Which keys are on (first-run choices, C-OPS-07).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub shift_space: bool,
    pub caps_lock: bool,
    pub hangul_key: bool,
    pub right_alt: bool,
    pub pause_key: bool,
    pub correction_keys: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            shift_space: true,
            caps_lock: true,
            hangul_key: true,
            right_alt: true,
            pause_key: true,
            correction_keys: true,
        }
    }
}

/// A Caps Lock press at least this long keeps the real Caps Lock (C-MOD-10 `[제안] 0.5초`).
pub const CAPS_LONG_MS: u64 = 500;

/// How a key is read before the engine sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Text(char, char),
    Space,
    Enter,
    Tab,
    Backspace,
    Escape,
    Navigation,
    Shortcut,
    Modifier,
    Other,
}

/// The character a key types on the US positions without and with Shift.
pub fn printable(vk: u16) -> Option<(char, char)> {
    let pair = match vk {
        0x41..=0x5A => {
            let upper = vk as u8 as char;
            return Some((upper.to_ascii_lowercase(), upper));
        }
        0x30 => ('0', ')'),
        0x31 => ('1', '!'),
        0x32 => ('2', '@'),
        0x33 => ('3', '#'),
        0x34 => ('4', '$'),
        0x35 => ('5', '%'),
        0x36 => ('6', '^'),
        0x37 => ('7', '&'),
        0x38 => ('8', '*'),
        0x39 => ('9', '('),
        0xBA => (';', ':'),
        0xBB => ('=', '+'),
        0xBC => (',', '<'),
        0xBD => ('-', '_'),
        0xBE => ('.', '>'),
        0xBF => ('/', '?'),
        0xC0 => ('`', '~'),
        0xDB => ('[', '{'),
        0xDC => ('\\', '|'),
        0xDD => (']', '}'),
        0xDE => ('\'', '"'),
        _ => return None,
    };
    Some(pair)
}

fn is_modifier(vk: u16) -> bool {
    matches!(
        vk,
        vk::SHIFT
            | vk::CONTROL
            | vk::MENU
            | vk::LSHIFT
            | vk::RSHIFT
            | vk::LCONTROL
            | vk::RCONTROL
            | vk::LMENU
            | vk::RMENU
            | vk::LWIN
            | vk::RWIN
    )
}

fn classify(vk: u16, mods: Mods) -> Class {
    if is_modifier(vk) {
        return Class::Modifier;
    }
    if mods.chord() {
        return Class::Shortcut;
    }
    match vk {
        vk::SPACE => return Class::Space,
        vk::RETURN => return Class::Enter,
        vk::TAB => return Class::Tab,
        vk::BACK => return Class::Backspace,
        vk::ESCAPE => return Class::Escape,
        vk::PRIOR | vk::NEXT | vk::END | vk::HOME | vk::LEFT | vk::UP | vk::RIGHT | vk::DOWN | vk::INSERT | vk::DELETE => {
            return Class::Navigation
        }
        _ => {}
    }
    match printable(vk) {
        Some((plain, shifted)) if plain.is_ascii_alphabetic() => {
            // Caps Lock changes what English types but not the Dubeolsik key (C-DET-06).
            let raw = if mods.shift != mods.caps { shifted } else { plain };
            Class::Text(raw, if mods.shift { shifted } else { plain })
        }
        Some((plain, shifted)) => {
            let c = if mods.shift { shifted } else { plain };
            Class::Text(c, c)
        }
        None => Class::Other,
    }
}

pub struct Shell {
    engine: Arc<Engine>,
    pub settings: Settings,
    /// Keys pass untouched: the tray app is judging (W-17) or a password field has focus (C-CTX-01).
    pub pass_through: bool,
    /// A composition is on screen.
    shown: bool,
    /// The Caps Lock toggle as the last key event reported it, i.e. before the press being handled.
    last_caps: bool,
    caps_before: bool,
    caps_down_at: Option<u64>,
    caps_shift: bool,
    /// Caps Lock events we sent ourselves that are still to arrive (down and up).
    caps_injected: u8,
    ralt_down: bool,
    /// No other key went down since the right Alt did.
    ralt_clean: bool,
}

impl Shell {
    pub fn new(engine: Arc<Engine>) -> Shell {
        Shell {
            engine,
            settings: Settings::default(),
            pass_through: false,
            shown: false,
            last_caps: false,
            caps_before: false,
            caps_down_at: None,
            caps_shift: false,
            caps_injected: 0,
            ralt_down: false,
            ralt_clean: false,
        }
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub fn mode(&self) -> InputMode {
        self.engine.mode()
    }

    pub fn is_composing(&self) -> bool {
        self.shown || self.engine.is_composing()
    }

    /// The context changed: the caller removed or kept the composition on its side; the engine starts clean.
    pub fn reset(&mut self, caps: bool) {
        self.shown = false;
        self.last_caps = caps;
        self.caps_down_at = None;
        self.ralt_down = false;
        self.engine.cancel();
    }

    fn inactive(&self) -> bool {
        self.pass_through
    }

    /// Whether the key event goes to `key_down`. No engine state changes here: an application may ask more than once.
    pub fn test_key_down(&mut self, e: KeyEvent) -> bool {
        if e.vk != vk::CAPITAL {
            self.last_caps = e.mods.caps;
        }
        if e.is_right_alt() {
            if !self.ralt_down {
                self.ralt_down = true;
                self.ralt_clean = true;
            }
            return false;
        }
        if self.ralt_down && !is_modifier(e.vk) {
            self.ralt_clean = false;
        }
        if self.inactive() {
            return false;
        }
        let s = self.settings;
        if e.vk == vk::F12 && s.pause_key && !e.mods.chord() && !e.mods.shift {
            return true;
        }
        if self.engine.paused() {
            return false;
        }
        match e.vk {
            vk::CAPITAL => {
                if self.caps_injected > 0 {
                    self.caps_injected -= 1;
                    return false;
                }
                return s.caps_lock && !e.mods.chord();
            }
            vk::HANGUL => return s.hangul_key,
            vk::SPACE if e.mods.shift && !e.mods.chord() && s.shift_space => return true,
            vk::F2 if s.correction_keys && !e.mods.chord() => {
                // F2 alone only right after a Space commit (Excel and others use it); Shift+F2 always (C-COR-03).
                return e.mods.shift || self.engine.reconversion().is_some();
            }
            _ => {}
        }
        let composing = self.is_composing();
        match classify(e.vk, e.mods) {
            Class::Modifier => false,
            Class::Text(..) => self.engine.mode() != InputMode::English || composing,
            _ => composing,
        }
    }

    pub fn key_down(&mut self, e: KeyEvent) -> Outcome {
        if self.inactive() {
            return Outcome::pass();
        }
        let s = self.settings;
        if e.vk == vk::F12 && s.pause_key && !e.mods.chord() && !e.mods.shift {
            if e.repeat {
                return Outcome::eaten();
            }
            let mut out = Outcome::eaten();
            if !self.engine.paused() {
                let r = self.engine.commit_pending();
                self.apply(r, &mut out);
            }
            let r = self.engine.set_paused(!self.engine.paused());
            self.apply(r, &mut out);
            out.eaten = true;
            return out;
        }
        if self.engine.paused() {
            return Outcome::pass();
        }
        match e.vk {
            vk::CAPITAL if s.caps_lock && !e.mods.chord() => {
                if !e.repeat {
                    self.caps_down_at = Some(e.time);
                    self.caps_before = self.last_caps;
                    self.caps_shift = e.mods.shift;
                }
                return Outcome::eaten();
            }
            vk::HANGUL if s.hangul_key => {
                return if e.repeat { Outcome::eaten() } else { self.switch() };
            }
            vk::SPACE if e.mods.shift && !e.mods.chord() && s.shift_space => {
                return if e.repeat { Outcome::eaten() } else { self.switch() };
            }
            vk::F2 if s.correction_keys && !e.mods.chord() => {
                if e.mods.shift {
                    if self.is_composing() {
                        let mut out = Outcome::eaten();
                        let r = self.engine.commit_pending();
                        self.apply(r, &mut out);
                        out.eaten = true;
                        out.ops.push(Op::FlipSelection);
                        return out;
                    }
                    return Outcome {
                        eaten: true,
                        ops: vec![Op::FlipSelection],
                        learned: false,
                    };
                }
                return match self.engine.reconversion() {
                    Some(swap) => Outcome {
                        eaten: true,
                        ops: vec![Op::ReplaceBefore {
                            old: swap.old,
                            new: swap.new,
                        }],
                        learned: false,
                    },
                    None => Outcome::pass(),
                };
            }
            _ => {}
        }
        if e.is_right_alt() {
            return Outcome::pass();
        }
        match classify(e.vk, e.mods) {
            Class::Modifier => Outcome::pass(),
            Class::Text(raw, key) => self.run(KeyKind::Text, raw, key),
            Class::Space => self.run(KeyKind::Space, ' ', ' '),
            Class::Enter => self.run(KeyKind::Enter, ' ', ' '),
            Class::Tab => self.run(KeyKind::Tab, ' ', ' '),
            Class::Backspace => self.run(KeyKind::Backspace, ' ', ' '),
            Class::Escape => self.run(KeyKind::Escape, ' ', ' '),
            Class::Navigation => self.run(KeyKind::Navigation, ' ', ' '),
            Class::Shortcut => self.run(KeyKind::Shortcut, ' ', ' '),
            Class::Other => self.run(KeyKind::Other, ' ', ' '),
        }
    }

    pub fn test_key_up(&mut self, e: KeyEvent) -> bool {
        if e.vk == vk::CAPITAL {
            if self.caps_injected > 0 {
                self.caps_injected -= 1;
                return false;
            }
            return self.caps_down_at.is_some();
        }
        if e.is_right_alt() {
            let tap = self.ralt_down && self.ralt_clean && self.settings.right_alt;
            if !tap {
                self.ralt_down = false;
            }
            return tap && !self.inactive() && !self.engine.paused();
        }
        false
    }

    pub fn key_up(&mut self, e: KeyEvent) -> Outcome {
        if e.vk == vk::CAPITAL {
            let Some(at) = self.caps_down_at.take() else {
                return Outcome::pass();
            };
            let long = self.caps_shift || e.time.saturating_sub(at) >= CAPS_LONG_MS;
            let want = if long { !self.caps_before } else { self.caps_before };
            let mut out = if long { Outcome::eaten() } else { self.switch() };
            if e.mods.caps != want {
                out.ops.push(Op::ToggleCapsLock);
            }
            self.last_caps = want;
            return out;
        }
        if e.is_right_alt() {
            let tap = self.ralt_down && self.ralt_clean && self.settings.right_alt;
            self.ralt_down = false;
            if tap && !self.inactive() && !self.engine.paused() {
                return self.switch();
            }
        }
        Outcome::pass()
    }

    /// The service is about to send the Caps Lock press from `Op::ToggleCapsLock`: its two events pass untouched.
    pub fn expect_injected_caps_lock(&mut self) {
        self.caps_injected = 2;
    }

    /// The ReplaceBefore of F2 was applied: remember the new choice.
    pub fn reconverted(&mut self) -> Outcome {
        let mut out = Outcome::eaten();
        let r = self.engine.reconvert();
        self.apply(r, &mut out);
        out.eaten = true;
        out
    }

    /// The selection's text with Hangul and QWERTY swapped, or None when nothing changes (C-COR-03).
    pub fn flip(&self, text: &str) -> Option<String> {
        let flipped = self.engine.flip(text.to_string());
        (flipped != text).then_some(flipped)
    }

    /// Focus is leaving the context: what to write into it before the composition ends.
    pub fn focus_lost(&mut self) -> Outcome {
        let mut out = Outcome::pass();
        if self.is_composing() {
            let r = self.engine.commit_pending();
            self.apply(r, &mut out);
        }
        out.eaten = false;
        out
    }

    /// The application or TSF ended the composition (a click, a menu): the text the composition should hold now.
    pub fn composition_terminated(&mut self) -> Option<String> {
        if !self.is_composing() {
            return None;
        }
        self.shown = false;
        let r = self.engine.commit_pending();
        (!r.commit.is_empty()).then_some(r.commit)
    }

    pub fn set_mode(&mut self, mode: InputMode) -> Outcome {
        let mut out = Outcome::pass();
        let r = self.engine.set_mode(mode);
        self.apply(r, &mut out);
        out
    }

    fn switch(&mut self) -> Outcome {
        let mut out = self.run(KeyKind::Switch, ' ', ' ');
        out.eaten = true;
        out
    }

    fn run(&mut self, kind: KeyKind, raw: char, key: char) -> Outcome {
        let (raw, key) = if kind == KeyKind::Text {
            (raw.to_string(), key.to_string())
        } else {
            (String::new(), String::new())
        };
        let r = self.engine.handle(kind, raw, key);
        let mut out = Outcome::pass();
        out.eaten = r.handled;
        self.apply(r, &mut out);
        out
    }

    /// Insert the commit, then show the composition (an empty one removes it), as on macOS.
    fn apply(&mut self, r: KeyResult, out: &mut Outcome) {
        if r.failed {
            // C-CMP-11: the engine gave the word back as typed; keep it and fall back to pass-through.
            let _ = self.engine.set_paused(true);
        }
        if !r.commit.is_empty() {
            out.ops.push(Op::Commit(r.commit));
            self.shown = false;
        }
        if !r.marked.is_empty() {
            out.ops.push(Op::Compose(r.marked));
            self.shown = true;
        } else if self.shown {
            out.ops.push(Op::Compose(String::new()));
            self.shown = false;
        }
        out.learned |= r.learned;
    }
}
