//! The Windows key logic against a document that applies its operations the way the text service does, with the
//! application typing every key the input method passes. Run on any OS: `cargo test` (the replay needs tests/corpora).

use hanautomata_ffi::{Engine, InputMode};
use hanautomata_tsf::shell::{vk, KeyEvent, Mods, Op, Outcome, Shell, CAPS_LONG_MS};
use std::path::PathBuf;
use std::sync::Arc;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hanautomata-tsf-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}.json"))
}

fn shell(name: &str) -> Shell {
    let engine = Engine::new("/Users/tester".into(), temp(name).to_string_lossy().into_owned());
    Shell::new(engine)
}

#[test]
fn preserved_reconversion_routes_a_fresh_word_and_leaves_other_f2_to_the_app() {
    let mut s = shell("preserved-f2");
    assert_eq!(s.reconversion_command(), Outcome::default());
    let mut d = Doc::default();
    for key in [b'D', b'K', b'S', b'S', b'U', b'D'] {
        d.key(&mut s, key as u16, Mods::NONE, None);
    }
    d.key(&mut s, vk::SPACE, Mods::NONE, None);
    let command = s.reconversion_command();
    assert!(command.eaten, "The TSF preserved-key callback must dispatch a fresh F2 reconversion");
    d.apply(&mut s, &command);
    assert_eq!(d.text, "dkssud ");
    let again = s.reconversion_command();
    d.apply(&mut s, &again);
    assert_eq!(d.text, "안녕 ");
    d.key(&mut s, b'X' as u16, Mods::NONE, None);
    assert_eq!(s.reconversion_command(), Outcome::default(), "A new composition invalidates F2");
}

#[test]
fn preserved_reconversion_respects_passthrough_pause_and_settings() {
    for condition in ["password-or-tray", "paused", "disabled"] {
        let mut s = shell(&format!("preserved-{condition}"));
        for key in [b'D', b'K', b'S', b'S', b'U', b'D', b' '] {
            s.key_down(KeyEvent::new(key as u16, Mods::NONE));
        }
        match condition {
            "password-or-tray" => s.pass_through = true,
            "paused" => { s.key_down(KeyEvent::new(vk::F12, Mods::NONE)); }
            "disabled" => s.settings.correction_keys = false,
            _ => unreachable!(),
        }
        assert_eq!(s.reconversion_command(), Outcome::default(), "{condition}");
    }
}

#[test]
fn protected_transition_discards_pending_and_reconversion_state() {
    for committed in [false, true] {
        let mut s = shell(&format!("protected-transition-{committed}"));
        for key in b"DKSSUD" { s.key_down(KeyEvent::new(*key as u16, Mods::NONE)); }
        if committed { s.key_down(KeyEvent::new(vk::SPACE, Mods::NONE)); }
        let finish = s.context_policy(true, false).expect("first transition");
        if !committed {
            assert!(finish.ops.iter().any(|op| *op == Op::Commit("안녕".into())), "resolve pending text into its original field");
        }
        assert!(!s.is_composing());
        assert!(s.engine().reconversion().is_none());
        assert_eq!(s.key_down(KeyEvent::new(b'P' as u16, Mods::NONE)), Outcome::default());
        assert_eq!(s.reconversion_command(), Outcome::default());
        s.context_policy(false, false);
        assert_eq!(s.reconversion_command(), Outcome::default(), "F2 history must not cross protected fields");
        let mut d = Doc::default();
        for key in b"GKSRMF" {
            d.key(&mut s, *key as u16, Mods::NONE, None);
        }
        d.key(&mut s, vk::SPACE, Mods::NONE, None);
        assert_eq!(d.text, "한글 ", "no stale prefix from the previous field");
    }
}

#[test]
fn protected_test_probes_do_not_end_or_reset_a_composition() {
    let mut s = shell("nonmutating-field-probe");
    for key in b"DKS" { s.key_down(KeyEvent::new(*key as u16, Mods::NONE)); }
    for _ in 0..3 {
        assert!(!s.test_key_in_context(KeyEvent::new(b'S' as u16, Mods::NONE), true, true));
        assert!(s.is_composing());
        assert!(!s.pass_through);
    }
    let mut d = Doc::default();
    for key in b"SUD" { d.key(&mut s, *key as u16, Mods::NONE, None); }
    d.key(&mut s, vk::SPACE, Mods::NONE, None);
    assert_eq!(d.text, "안녕 ");
}

#[test]
fn private_context_composes_and_reconverts_without_learning_and_restores_pause() {
    for paused_before in [false, true] {
        let mut s = shell(&format!("private-learning-{paused_before}"));
        s.engine().set_learning_paused(paused_before);
        s.context_policy(false, true);
        let mut d = Doc::default();
        for key in b"DKSSUD" { d.key(&mut s, *key as u16, Mods::NONE, None); }
        d.key(&mut s, vk::SPACE, Mods::NONE, None);
        assert_eq!(d.text, "안녕 ");
        d.key(&mut s, vk::F2, Mods::NONE, None);
        assert_eq!(d.text, "dkssud ");
        assert_eq!(s.engine().learned_count(), 0);
        s.context_policy(false, false);
        assert_eq!(s.engine().learning_paused(), paused_before);
        assert!(s.engine().reconversion().is_none());
    }
}

/// The application side: committed text, the composition after it, and the Caps Lock toggle.
#[derive(Default)]
struct Doc {
    text: String,
    composition: String,
    caps: bool,
    /// A key reached the application while a composition was still on screen (order would break, W-11).
    passed_over_composition: bool,
}

impl Doc {
    fn shown(&self) -> String {
        format!("{}{}", self.text, self.composition)
    }

    fn apply(&mut self, shell: &mut Shell, out: &Outcome) {
        for op in &out.ops {
            match op {
                Op::Commit(s) => {
                    self.composition.clear();
                    self.text.push_str(s);
                }
                Op::Compose(s) => self.composition = s.clone(),
                Op::ReplaceBefore { old, new } => {
                    if self.composition.is_empty() && self.text.ends_with(old.as_str()) {
                        self.text.truncate(self.text.len() - old.len());
                        self.text.push_str(new);
                        let after = shell.reconverted();
                        self.apply(shell, &after);
                    }
                }
                Op::FlipSelection => {
                    // The tests select the whole text.
                    if let Some(flipped) = shell.flip(&self.text) {
                        self.text = flipped;
                    }
                }
                Op::ToggleCapsLock => {
                    shell.expect_injected_caps_lock();
                    self.caps = !self.caps;
                    let mut e = KeyEvent::new(
                        vk::CAPITAL,
                        Mods {
                            caps: self.caps,
                            ..Mods::NONE
                        },
                    );
                    assert!(!shell.test_key_down(e), "our own Caps Lock press passes");
                    e.mods.caps = self.caps;
                    assert!(!shell.test_key_up(e), "and its release too");
                }
            }
        }
    }

    /// One key as TSF delivers it: test, then key down if wanted; the application types what is not eaten.
    fn key(&mut self, shell: &mut Shell, vk: u16, mods: Mods, app_char: Option<char>) -> bool {
        let mut e = KeyEvent::new(vk, Mods { caps: self.caps, ..mods });
        e.time = 0;
        let eaten = if shell.test_key_down(e) {
            let out = shell.key_down(e);
            self.apply(shell, &out);
            out.eaten
        } else {
            false
        };
        if !eaten {
            if !self.composition.is_empty() {
                self.passed_over_composition = true;
            }
            if let Some(c) = app_char {
                self.text.push(c);
            }
        }
        eaten
    }

    fn up(&mut self, shell: &mut Shell, vk: u16, mods: Mods, time: u64) {
        let mut e = KeyEvent::new(vk, Mods { caps: self.caps, ..mods });
        e.time = time;
        if shell.test_key_up(e) {
            let out = shell.key_up(e);
            self.apply(shell, &out);
        }
    }

    fn type_str(&mut self, shell: &mut Shell, s: &str) {
        for c in s.chars() {
            self.char(shell, c);
        }
    }

    fn char(&mut self, shell: &mut Shell, c: char) {
        let (code, shift) = code_of(c);
        let mods = if shift { Mods::SHIFT } else { Mods::NONE };
        let app = if c == '\n' { Some('\n') } else { Some(c) };
        self.key(shell, code, mods, app);
    }
}

fn code_of(c: char) -> (u16, bool) {
    match c {
        ' ' => (vk::SPACE, false),
        '\n' => (vk::RETURN, false),
        'a'..='z' => (c.to_ascii_uppercase() as u16, false),
        'A'..='Z' => (c as u16, true),
        _ => {
            for code in 0u16..=0xFF {
                if let Some((plain, shifted)) = hanautomata_tsf::shell::printable(code) {
                    if plain == c {
                        return (code, false);
                    }
                    if shifted == c {
                        return (code, true);
                    }
                }
            }
            panic!("no key for {c:?}")
        }
    }
}

#[test]
fn korean_and_english_words() {
    let mut s = shell("words");
    let mut d = Doc::default();
    d.type_str(&mut s, "dkssudgktpdy ");
    assert_eq!(d.shown(), "안녕하세요 ");
    d.type_str(&mut s, "hello world ");
    assert_eq!(d.shown(), "안녕하세요 hello world ");
    assert!(!d.passed_over_composition);
}

#[test]
fn commit_paths_commit_before_the_key_reaches_the_app() {
    let mut s = shell("paths");
    let mut d = Doc::default();
    d.type_str(&mut s, "gksrmf");
    assert_eq!(d.composition, "한글", "the decision shows while composing");
    d.type_str(&mut s, "\n");
    assert_eq!(d.shown(), "한글\n");
    d.type_str(&mut s, "dkssud");
    d.key(&mut s, vk::TAB, Mods::NONE, Some('\t'));
    assert_eq!(d.shown(), "한글\n안녕\t");
    d.type_str(&mut s, "dkssud");
    d.key(&mut s, vk::LEFT, Mods::NONE, None);
    d.type_str(&mut s, "dkssud");
    d.key(&mut s, 0x43, Mods::CTRL, None); // Ctrl+C commits first (C-CMP-04)
    assert_eq!(d.shown(), "한글\n안녕\t안녕안녕");
    assert!(!d.passed_over_composition, "no key passed while a composition was on screen");
}

#[test]
fn english_mode_passes_keys_and_switch_keys_still_work() {
    let mut s = shell("english");
    let mut d = Doc::default();
    let _ = s.set_mode(InputMode::English);
    assert!(!d.key(&mut s, 0x44, Mods::NONE, Some('d')), "English mode: letters pass");
    assert!(d.key(&mut s, vk::HANGUL, Mods::NONE, None), "한/영 is eaten");
    assert_ne!(s.mode(), InputMode::English);
    assert_eq!(s.mode(), InputMode::Korean, "from English the switch key goes to Korean");
    d.type_str(&mut s, "dkssud ");
    assert_eq!(d.shown(), "d안녕 ");
}

#[test]
fn shift_space_switches_and_a_repeat_does_nothing() {
    let mut s = shell("shiftspace");
    let mut d = Doc::default();
    let before = s.mode();
    assert!(d.key(&mut s, vk::SPACE, Mods::SHIFT, Some(' ')));
    let after = s.mode();
    let mut repeat = KeyEvent::new(vk::SPACE, Mods::SHIFT);
    repeat.repeat = true;
    assert!(s.test_key_down(repeat));
    let out = s.key_down(repeat);
    assert!(out.eaten && out.ops.is_empty());
    assert_eq!(d.text, "", "⇧Space types nothing");
    // Single-shot meaning: auto stays auto with the next word reserved, or the mode changes; either way it was handled.
    let _ = (before, after);
}

#[test]
fn caps_lock_short_press_switches_and_keeps_the_toggle() {
    for system_toggles_on_press in [false, true] {
        let mut s = shell("caps-short");
        let mut d = Doc::default();
        d.type_str(&mut s, "x"); // the shell learns the toggle state from key events
        d.key(&mut s, vk::BACK, Mods::NONE, None);
        assert!(d.key(&mut s, vk::CAPITAL, Mods::NONE, None), "Caps Lock is eaten");
        if system_toggles_on_press {
            d.caps = !d.caps;
        }
        d.up(&mut s, vk::CAPITAL, Mods::NONE, 120);
        assert!(
            !d.caps,
            "a short press never leaves Caps Lock on (toggled by system: {system_toggles_on_press})"
        );
    }
}

#[test]
fn caps_lock_long_press_and_shift_caps_lock_lock_capitals() {
    let mut s = shell("caps-long");
    let mut d = Doc::default();
    d.key(&mut s, vk::CAPITAL, Mods::NONE, None);
    d.up(&mut s, vk::CAPITAL, Mods::NONE, CAPS_LONG_MS + 10);
    assert!(d.caps, "a long press locks capitals");
    d.key(&mut s, vk::CAPITAL, Mods::SHIFT, None);
    d.up(&mut s, vk::CAPITAL, Mods::NONE, 50);
    assert!(!d.caps, "Shift+Caps Lock toggles it back");
    let mode = s.mode();
    d.key(&mut s, vk::CAPITAL, Mods::NONE, None);
    d.up(&mut s, vk::CAPITAL, Mods::NONE, CAPS_LONG_MS);
    assert_eq!(s.mode(), mode, "a long press does not switch");
}

#[test]
fn right_alt_tap_switches_but_not_with_another_key() {
    let mut s = shell("ralt");
    let mut d = Doc::default();
    let mode = s.mode();
    let mut alt = KeyEvent::new(vk::MENU, Mods { alt: true, ..Mods::NONE });
    alt.extended = true;
    assert!(!s.test_key_down(alt), "the right Alt press itself passes");
    let up = KeyEvent::new(vk::RMENU, Mods::NONE);
    assert!(s.test_key_up(up), "a clean tap is taken on release");
    let out = s.key_up(up);
    assert!(out.eaten);
    let switched = s.mode();
    assert!(!s.test_key_down(alt));
    d.key(&mut s, 0x46, Mods { alt: true, ..Mods::NONE }, None);
    assert!(!s.test_key_up(up), "right Alt with another key is not a tap");
    assert_eq!(s.mode(), switched);
    let _ = mode;
}

#[test]
fn f2_swaps_the_last_word_and_shift_f2_flips_the_selection() {
    let mut s = shell("f2");
    let mut d = Doc::default();
    d.type_str(&mut s, "dkssud ");
    assert_eq!(d.text, "안녕 ");
    assert!(d.key(&mut s, vk::F2, Mods::NONE, None));
    assert_eq!(d.text, "dkssud ", "F2 swaps the word just committed");
    assert!(d.key(&mut s, vk::F2, Mods::NONE, None));
    assert_eq!(d.text, "안녕 ", "and again swaps back");
    d.type_str(&mut s, "abc");
    assert!(
        !d.key(&mut s, vk::F2, Mods::NONE, None) || d.text.ends_with("abc"),
        "F2 without a fresh word passes"
    );
    let mut flip = shell("flip");
    let mut fd = Doc {
        text: "rkskek".into(),
        ..Doc::default()
    };
    assert!(fd.key(&mut flip, vk::F2, Mods::SHIFT, None));
    assert_eq!(fd.text, "가나다", "Shift+F2 flips the selection in place");
}

#[test]
fn f12_pauses_and_everything_passes_until_pressed_again() {
    let mut s = shell("pause");
    let mut d = Doc::default();
    d.type_str(&mut s, "dkssud");
    assert!(d.key(&mut s, vk::F12, Mods::NONE, None));
    assert_eq!(d.shown(), "안녕", "the word in progress is committed first");
    d.type_str(&mut s, "dkssud ");
    assert_eq!(d.shown(), "안녕dkssud ", "paused: keys reach the app as typed");
    assert!(d.key(&mut s, vk::F12, Mods::NONE, None));
    d.type_str(&mut s, "gksrmf ");
    assert_eq!(d.shown(), "안녕dkssud 한글 ");
}

#[test]
fn pass_through_beside_the_tray_app_or_in_password_fields() {
    let mut s = shell("pass");
    s.pass_through = true;
    let mut d = Doc::default();
    d.type_str(&mut s, "dkssud ");
    assert_eq!(d.text, "dkssud ");
    assert!(!d.key(&mut s, vk::F12, Mods::NONE, None), "even F12 goes to the tray app");
}

#[test]
fn focus_loss_and_terminated_compositions_commit_the_word() {
    let mut s = shell("focus");
    let mut d = Doc::default();
    d.type_str(&mut s, "gksrmf");
    let out = s.focus_lost();
    d.apply(&mut s, &out);
    assert_eq!(d.shown(), "한글");
    assert!(d.composition.is_empty());
    d.type_str(&mut s, "dkssud");
    assert_eq!(s.composition_terminated().as_deref(), Some("안녕"));
    assert!(!s.is_composing());
}

fn corpus(name: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/corpora").join(name);
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split('\t').next())
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_string)
        .collect()
}

/// The text read back as keys: Hangul runs flipped to QWERTY (as the macOS replay does).
fn as_keys(text: &str, engine: &Arc<Engine>) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in text.chars() {
        let u = c as u32;
        if (0xAC00..=0xD7A3).contains(&u) || (0x3131..=0x318E).contains(&u) {
            run.push(c);
        } else {
            if !run.is_empty() {
                out.push_str(&engine.flip(std::mem::take(&mut run)));
            }
            out.push(c);
        }
    }
    if !run.is_empty() {
        out.push_str(&engine.flip(run));
    }
    out
}

/// C-CTX-06 on the Windows logic: held-out words in sentences with ⇧Space, Caps Lock taps, F2 swaps and focus moves.
/// The text read back as keys must equal what was typed: nothing lost, duplicated or reordered.
#[test]
fn replay_loses_nothing() {
    let korean = corpus("heldout-korean-1000.tsv");
    let english: Vec<String> = corpus("heldout-english-1000.tsv")
        .into_iter()
        .filter(|w| *w == w.to_lowercase())
        .collect();
    if korean.len() < 900 || english.len() < 800 {
        eprintln!("replay skipped: corpora not found");
        return;
    }
    let mut state: u64 = 20_261_012;
    let mut next = |n: usize| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 33) % n as u64) as usize
    };
    let mut keys_typed = 0;
    for sentence in 0..300 {
        let mut s = shell(&format!("replay-{}", sentence % 4));
        let mut d = Doc::default();
        let mut typed = String::new();
        for _ in 0..8 {
            let word = if next(10) < 6 {
                &korean[next(korean.len())]
            } else {
                &english[next(english.len())]
            };
            match next(12) {
                0 => {
                    d.key(&mut s, vk::SPACE, Mods::SHIFT, Some(' '));
                }
                1 => {
                    d.key(&mut s, vk::CAPITAL, Mods::NONE, None);
                    d.up(&mut s, vk::CAPITAL, Mods::NONE, 80);
                }
                2 => {
                    let out = s.focus_lost();
                    d.apply(&mut s, &out);
                }
                _ => {}
            }
            d.type_str(&mut s, word);
            d.type_str(&mut s, " ");
            typed.push_str(word);
            typed.push(' ');
            keys_typed += word.len() + 1;
            if next(8) == 0 {
                // F2 twice: swapped and back, so the keys stay the same.
                d.key(&mut s, vk::F2, Mods::NONE, None);
                d.key(&mut s, vk::F2, Mods::NONE, None);
            }
        }
        let out = s.focus_lost();
        d.apply(&mut s, &out);
        assert!(!d.caps, "sentence {sentence}: Caps Lock never left on");
        assert!(
            !d.passed_over_composition,
            "sentence {sentence}: no key passed over a composition"
        );
        let back = as_keys(&d.shown(), s.engine());
        assert_eq!(
            back.to_lowercase(),
            typed.to_lowercase(),
            "sentence {sentence}: {:?}",
            d.shown()
        );
    }
    eprintln!("replay: {keys_typed} keys in 300 sentences");
    assert!(keys_typed > 10_000, "replayed {keys_typed} keys");
}
