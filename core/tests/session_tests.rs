// Input-method session (OS-neutral key handling) and learning-file store (C-OPS-09).

use hanautomata_core::{
    Detector, Key, LearnedWord, Mode, Reconversion, Response, Session, Store, SwitchMeaning, WordPreferences, MAX_WORD,
};
use std::sync::Arc;

fn session() -> (Session, Arc<WordPreferences>) {
    let prefs = Arc::new(WordPreferences::new());
    (Session::new(Arc::new(Detector::with_preferences("", prefs.clone()))), prefs)
}

fn key(c: char) -> Key {
    // Dubeolsik keys keep Shift but ignore Caps Lock; in these tests nothing is held, so raw == key.
    Key::Text { raw: c, key: c }
}

fn typed(s: &mut Session, text: &str) -> Response {
    let mut last = Response::default();
    for c in text.chars() {
        last = s.handle(key(c));
        assert!(last.handled, "printable key is consumed while composing: {c}");
        assert!(last.commit.is_empty(), "typing never commits below the bound: {c}");
    }
    last
}

#[test]
fn space_commits_with_space_and_keeps_context() {
    let (mut s, _) = session();
    assert_eq!(typed(&mut s, "dkssud").marked, "안녕");
    let r = s.handle(Key::Space);
    assert_eq!((r.commit.as_str(), r.marked.as_str(), r.handled), ("안녕 ", "", true));
    // Continuity: the near-tie "dh" leans Korean after a Korean word.
    assert_eq!(typed(&mut s, "dh").marked, "오");
    s.cancel();
    assert_eq!(typed(&mut s, "dh").marked, "dh", "cancel drops continuity");
}

#[test]
fn space_with_nothing_composed_passes() {
    let (mut s, _) = session();
    assert_eq!(s.handle(Key::Space), Response::default());
}

#[test]
fn enter_tab_navigation_shortcut_commit_then_pass() {
    for k in [Key::Enter, Key::Tab, Key::Navigation, Key::Shortcut, Key::Other] {
        let (mut s, _) = session();
        typed(&mut s, "gksrmf");
        let r = s.handle(k);
        assert_eq!(
            (r.commit.as_str(), r.marked.as_str(), r.handled),
            ("한글", "", false),
            "{k:?}"
        );
        assert_eq!(typed(&mut s, "dh").marked, "dh", "{k:?} clears continuity");
        assert_eq!(
            s.handle(k),
            Response {
                commit: "dh".into(),
                ..Response::default()
            }
        );
        assert_eq!(s.handle(k), Response::default(), "{k:?} with nothing composed just passes");
    }
}

#[test]
fn escape_cancels_and_backspace_edits() {
    let (mut s, _) = session();
    typed(&mut s, "rkqtk");
    assert_eq!(s.handle(Key::Backspace).marked, "값");
    let r = s.handle(Key::Escape);
    assert_eq!((r.commit.as_str(), r.marked.as_str(), r.handled), ("", "", true));
    assert!(!s.is_composing());
    assert_eq!(
        s.handle(Key::Escape),
        Response::default(),
        "Esc passes when nothing is composed"
    );
    assert_eq!(
        s.handle(Key::Backspace),
        Response::default(),
        "Backspace passes when nothing is composed"
    );
    typed(&mut s, "r");
    let r = s.handle(Key::Backspace);
    assert_eq!(
        (r.marked.as_str(), r.handled),
        ("", true),
        "deleting the last key clears the composition"
    );
}

#[test]
fn punctuation_and_digits_join_the_word() {
    let (mut s, _) = session();
    typed(&mut s, "dkssudgktpdy!");
    assert_eq!(s.handle(Key::Space).commit, "안녕하세요! ");
    typed(&mut s, "4wja");
    assert_eq!(s.handle(Key::Space).commit, "4점 ");
}

#[test]
fn switch_flips_the_word_and_learns_on_commit() {
    let (mut s, prefs) = session();
    assert_eq!(typed(&mut s, "rm").marked, "rm", "near-tie stays literal");
    let r = s.handle(Key::Switch);
    assert_eq!((r.marked.as_str(), r.handled), ("그", true));
    let r = s.handle(Key::Space);
    assert_eq!((r.commit.as_str(), r.learned), ("그 ", true));
    assert_eq!(prefs.count(), 1);
    assert_eq!(s.detector().decide("rm").text, "그", "the switched choice is remembered");
}

#[test]
fn second_switch_restores_the_automatic_choice_without_learning() {
    let (mut s, prefs) = session();
    typed(&mut s, "rm");
    s.handle(Key::Switch);
    let r = s.handle(Key::Switch);
    assert_eq!(r.marked, "rm");
    let r = s.handle(Key::Space);
    assert_eq!((r.commit.as_str(), r.learned), ("rm ", false));
    assert_eq!(prefs.count(), 0);
}

#[test]
fn switch_with_nothing_composed_reserves_the_next_word() {
    let (mut s, _) = session();
    // No preceding word: reserve Korean.
    assert!(s.handle(Key::Switch).handled);
    assert_eq!(s.reserved(), Some(true));
    assert_eq!(typed(&mut s, "rm").marked, "그");
    assert_eq!(s.reserved(), None);
    s.handle(Key::Space);
    // After a Korean word, the reservation is English; a second press cancels it.
    s.handle(Key::Switch);
    assert_eq!(s.reserved(), Some(false));
    s.handle(Key::Switch);
    assert_eq!(s.reserved(), None);
    s.handle(Key::Switch);
    assert_eq!(typed(&mut s, "gksrmf").marked, "gksrmf", "reserved English keeps the literal");
    // The reserved word's own second switch returns to the automatic decision.
    assert_eq!(s.handle(Key::Switch).marked, "한글");
}

#[test]
fn fixed_modes() {
    let (mut s, prefs) = session();
    s.set_mode(Mode::Korean);
    assert_eq!(typed(&mut s, "dh").marked, "오", "Korean mode composes even a near-tie");
    assert_eq!(typed(&mut s, "hello").marked, "오ㅗ디ㅣㅐ");
    let r = s.handle(Key::Space);
    assert_eq!(
        (r.commit.as_str(), r.learned),
        ("오ㅗ디ㅣㅐ ", false),
        "fixed modes never learn"
    );
    assert_eq!(prefs.count(), 0);
    // Switch toggles the fixed modes, committing first.
    typed(&mut s, "gksrmf");
    let r = s.handle(Key::Switch);
    assert_eq!((r.commit.as_str(), r.handled), ("한글", true));
    assert_eq!(s.mode(), Mode::English);
    assert_eq!(s.handle(key('a')), Response::default(), "English mode passes keys");
    assert_eq!(s.handle(Key::Space), Response::default());
    s.handle(Key::Switch);
    assert_eq!(s.mode(), Mode::Korean);
    // Changing mode commits a word in progress.
    typed(&mut s, "rk");
    assert_eq!(s.set_mode(Mode::Auto).commit, "가");
}

#[test]
fn cycle_meaning_walks_the_three_modes_and_commits_first() {
    let (mut s, prefs) = session();
    s.set_switch_meaning(SwitchMeaning::Cycle);
    typed(&mut s, "gksrmf");
    let r = s.handle(Key::Switch);
    assert_eq!((r.commit.as_str(), r.marked.as_str(), r.handled), ("한글", "", true));
    assert_eq!(s.mode(), Mode::Korean);
    assert!(s.handle(Key::Switch).handled);
    assert_eq!(s.mode(), Mode::English);
    s.handle(Key::Switch);
    assert_eq!(s.mode(), Mode::Auto);
    assert_eq!(s.reserved(), None, "cycling never reserves a word");
    assert_eq!(prefs.count(), 0, "a mode change is not a per-word choice");
}

#[test]
fn toggle_meaning_leaves_the_automatic_mode_away_from_the_language_on_screen() {
    let (mut s, _) = session();
    s.set_switch_meaning(SwitchMeaning::Toggle);
    typed(&mut s, "gksrmf");
    assert_eq!(s.handle(Key::Switch).commit, "한글");
    assert_eq!(s.mode(), Mode::English, "a Korean word on screen: to English");
    s.handle(Key::Switch);
    assert_eq!(s.mode(), Mode::Korean);
    s.handle(Key::Switch);
    assert_eq!(s.mode(), Mode::English);

    let (mut s, _) = session();
    s.set_switch_meaning(SwitchMeaning::Toggle);
    typed(&mut s, "hello");
    assert_eq!(s.handle(Key::Space).commit, "hello ");
    s.handle(Key::Switch);
    assert_eq!(s.mode(), Mode::Korean, "after an English word: to Korean");

    let (mut s, _) = session();
    s.set_switch_meaning(SwitchMeaning::Toggle);
    s.handle(Key::Switch);
    assert_eq!(s.mode(), Mode::Korean, "nothing known: Korean");
}

#[test]
fn changing_the_meaning_drops_a_reservation() {
    let (mut s, _) = session();
    s.handle(Key::Switch);
    assert_eq!(s.reserved(), Some(true));
    s.set_switch_meaning(SwitchMeaning::Toggle);
    assert_eq!(s.reserved(), None);
    assert_eq!(s.switch_meaning(), SwitchMeaning::Toggle);
}

#[test]
fn semicolon_and_candidate_keys() {
    let (mut s, _) = session();
    typed(&mut s, "rm");
    assert_eq!(s.handle(key(';')).marked, "그", "semicolon selects the other candidate");
    assert_eq!(s.handle(key(';')).marked, "rm;", "double semicolon types a literal semicolon");
    s.cancel();
    typed(&mut s, "rm");
    assert_eq!(s.handle(Key::Candidate).marked, "그", "F2 selects the other candidate");
    s.cancel();
    s.set_semicolon_shortcut(false);
    typed(&mut s, "rm");
    assert_eq!(s.handle(key(';')).marked, "rm;", "semicolon shortcut can be turned off");
    let r = s.handle(Key::Candidate);
    assert_eq!(
        (r.marked.as_str(), r.handled),
        ("그;", true),
        "F2 still switches when the shortcut is off"
    );
}

#[test]
fn pause_passes_everything_and_commits_first() {
    let (mut s, _) = session();
    typed(&mut s, "gksrmf");
    assert_eq!(s.set_paused(true).commit, "한글");
    for k in [key('a'), Key::Space, Key::Switch, Key::Backspace, Key::Enter] {
        assert_eq!(s.handle(k), Response::default(), "{k:?}");
    }
    s.set_paused(false);
    assert_eq!(typed(&mut s, "gksrmf").marked, "한글");
}

#[test]
fn focus_loss_commits_and_long_words_commit_at_the_bound() {
    let (mut s, _) = session();
    typed(&mut s, "gksrmf");
    assert_eq!(s.commit_pending().commit, "한글");
    assert_eq!(s.commit_pending(), Response::default());
    for i in 0..MAX_WORD - 1 {
        assert!(s.handle(key('a')).commit.is_empty(), "key {i}");
    }
    let r = s.handle(key('a'));
    assert_eq!((r.commit.len(), r.handled), (MAX_WORD, true));
    assert!(!s.is_composing());
}

#[test]
fn caps_lock_literal_with_lowercase_keys_converts() {
    let (mut s, _) = session();
    for (raw, k) in "DKSSUD".chars().zip("dkssud".chars()) {
        s.handle(Key::Text { raw, key: k });
    }
    assert_eq!(s.handle(Key::Space).commit, "안녕 ");
}

#[test]
fn keys_with_nothing_composed_end_the_context() {
    // The Windows app clears the composition context on any non-Space key with nothing composed (PreservePending).
    for k in [
        Key::Enter,
        Key::Tab,
        Key::Navigation,
        Key::Shortcut,
        Key::Other,
        Key::Backspace,
        Key::Escape,
    ] {
        let (mut s, _) = session();
        typed(&mut s, "dkssud");
        s.handle(Key::Space);
        assert!(!s.handle(k).handled, "{k:?} passes");
        assert_eq!(typed(&mut s, "dh").marked, "dh", "{k:?} ends continuity");
    }
    let (mut s, _) = session();
    typed(&mut s, "dkssud");
    s.handle(Key::Space);
    s.commit_pending();
    assert_eq!(typed(&mut s, "dh").marked, "dh", "a focus change ends continuity");
    s.cancel();
    typed(&mut s, "dkssud");
    s.handle(Key::Space);
    assert_eq!(s.handle(Key::Space), Response::default(), "a second Space passes");
    assert_eq!(
        typed(&mut s, "dh").marked,
        "오",
        "and keeps the context, as in the Windows app"
    );
}

#[test]
fn f2_after_space_swaps_the_committed_word_and_learns() {
    let (mut s, prefs) = session();
    typed(&mut s, "dkssud");
    s.handle(Key::Space);
    let swap = Reconversion {
        old: "안녕 ".into(),
        new: "dkssud ".into(),
    };
    assert_eq!(s.reconversion(), Some(swap.clone()));
    // The shell replaced the text; the session learns and continues from the new word.
    let r = s.reconvert();
    assert!(r.handled && r.learned && r.commit.is_empty());
    assert_eq!(prefs.count(), 1);
    assert_eq!(
        s.reconversion(),
        Some(Reconversion {
            old: swap.new.clone(),
            new: swap.old.clone()
        }),
        "F2 again swaps back"
    );
    // Remembered in the word's (empty) context: the same word now stays English.
    s.cancel();
    assert_eq!(s.reconversion(), None, "cancel forgets the word");
    assert_eq!(typed(&mut s, "dkssud").marked, "dkssud");
}

#[test]
fn second_f2_swaps_back_and_remembers_the_original_choice() {
    let (mut s, prefs) = session();
    typed(&mut s, "dkssud");
    s.handle(Key::Space);
    s.reconvert();
    let r = s.reconvert();
    assert!(r.learned);
    assert_eq!(s.reconversion().unwrap().old, "안녕 ");
    assert_eq!(prefs.count(), 1, "the same word and context is one entry");
    s.cancel();
    assert_eq!(typed(&mut s, "dkssud").marked, "안녕");
}

#[test]
fn only_space_commits_are_swappable_and_other_keys_forget_them() {
    for k in [Key::Enter, Key::Tab, Key::Navigation, Key::Shortcut] {
        let (mut s, _) = session();
        typed(&mut s, "dkssud");
        s.handle(k);
        assert_eq!(s.reconversion(), None, "{k:?}");
    }
    for k in [Key::Backspace, Key::Space, Key::Escape, Key::Switch, key('a')] {
        let (mut s, _) = session();
        typed(&mut s, "dkssud");
        s.handle(Key::Space);
        s.handle(k);
        assert_eq!(s.reconversion(), None, "{k:?} forgets the swappable word");
    }
    let (mut s, _) = session();
    typed(&mut s, "dkssud");
    s.handle(Key::Space);
    assert_eq!(
        s.handle(Key::Candidate),
        Response::default(),
        "F2 with nothing composed passes to the shell"
    );
    assert!(s.reconversion().is_some(), "and keeps the word");
    s.forget_reconversion();
    assert_eq!(s.reconversion(), None);
    assert_eq!(s.reconvert(), Response::default(), "nothing to swap");
    // A word whose candidates are identical (digits) is never swappable.
    typed(&mut s, "2026");
    s.handle(Key::Space);
    assert_eq!(s.reconversion(), None);
    // Fixed modes and focus changes.
    let (mut s, _) = session();
    s.set_mode(Mode::Korean);
    typed(&mut s, "dkssud");
    s.handle(Key::Space);
    assert_eq!(s.reconversion(), None, "fixed Korean mode is not a per-word choice");
    let (mut s, _) = session();
    typed(&mut s, "dkssud");
    s.handle(Key::Space);
    s.commit_pending();
    assert_eq!(s.reconversion(), None, "focus change forgets the word");
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hanautomata-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn store_round_trip_and_unchanged_saves() {
    let dir = temp_dir("roundtrip");
    let path = dir.join("words.json");
    let prefs = WordPreferences::new();
    let mut store = Store::open(&path, &prefs);
    assert_eq!(store.last_error(), "");
    assert_eq!(store.flush(&prefs), Ok(false), "nothing changed, nothing written");
    assert!(!path.exists());
    let context = hanautomata_core::context_key("나는");
    prefs.remember("go", "go", true, &context);
    prefs.remember_global("ej", "ej", true);
    assert!(store.is_dirty(&prefs));
    assert_eq!(store.flush(&prefs), Ok(true));
    assert!(!store.is_dirty(&prefs));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("\"Format\":2") && text.contains("\"KoreanWeight\""),
        "Windows document shape: {text}"
    );
    assert!(!text.contains("나는"), "context is stored as a fingerprint only");
    let reloaded = WordPreferences::new();
    Store::open(&path, &reloaded);
    assert_eq!(reloaded.export().0, prefs.export().0);
    let detector = Detector::with_preferences("", Arc::new(reloaded));
    assert_eq!(detector.decide_with("go", "go", &context, None).text, "해");
    assert_eq!(detector.decide("ej").text, "더");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn store_reads_format_1_and_lowercase_names() {
    let dir = temp_dir("format1");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("words.json");
    std::fs::write(
        &path,
        r#"{"Format":1,"Entries":[{"Raw":"gksms","Keys":"gksms","Korean":true,"Sequence":1}]}"#,
    )
    .unwrap();
    let prefs = WordPreferences::new();
    let store = Store::open(&path, &prefs);
    assert_eq!(store.last_error(), "");
    assert_eq!(Detector::with_preferences("", Arc::new(prefs)).decide("gksms").text, "하는");
    std::fs::write(&path, "\u{feff}{\"format\":2,\"entries\":[{\"raw\":\"ej\",\"keys\":\"ej\",\"korean\":true,\"sequence\":3,\"koreanWeight\":2,\"englishWeight\":0,\"context\":null}]}").unwrap();
    let prefs = WordPreferences::new();
    assert_eq!(Store::open(&path, &prefs).last_error(), "");
    assert_eq!(
        prefs.export().0[0],
        LearnedWord {
            raw: "ej".into(),
            keys: "ej".into(),
            korean: true,
            sequence: 1,
            context: String::new(),
            korean_weight: 2.0,
            english_weight: 0.0
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn store_keeps_an_unreadable_file_until_something_changes() {
    let dir = temp_dir("malformed");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("words.json");
    for content in [
        "not valid json".to_string(),
        "{\"Format\":3,\"Entries\":[]}".to_string(),
        format!("{{\"Format\":2,\"Entries\":[],\"Pad\":\"{}\"}}", "x".repeat(600_000)),
    ] {
        std::fs::write(&path, &content).unwrap();
        let prefs = WordPreferences::new();
        let mut store = Store::open(&path, &prefs);
        assert!(!store.last_error().is_empty(), "reported");
        assert_eq!(prefs.count(), 0, "falls back to an empty dictionary");
        assert_eq!(store.flush(&prefs), Ok(false));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content, "not silently overwritten");
        prefs.remember_global("ej", "ej", true);
        assert_eq!(store.flush(&prefs), Ok(true), "a real change replaces it");
        assert_eq!(store.last_error(), "");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn store_saves_are_atomic_under_concurrent_writers() {
    let dir = temp_dir("concurrent");
    let path = dir.join("words.json");
    let writers: Vec<_> = (0..4)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                let prefs = WordPreferences::new();
                let mut store = Store::open(&path, &prefs);
                for i in 0..60 {
                    let word = format!("w{}{}", (b'a' + t as u8) as char, (b'a' + (i % 26) as u8) as char);
                    prefs.remember_global(&word, &word, i % 2 == 0);
                    store.flush(&prefs).unwrap();
                }
            })
        })
        .collect();
    let reader = {
        let path = path.clone();
        std::thread::spawn(move || {
            let mut reads = 0;
            for _ in 0..200 {
                let prefs = WordPreferences::new();
                let store = Store::open(&path, &prefs);
                assert_eq!(store.last_error(), "", "a reader never sees a half-written file");
                reads += 1;
            }
            reads
        })
    };
    for writer in writers {
        writer.join().unwrap();
    }
    assert_eq!(reader.join().unwrap(), 200);
    assert!(!path.with_extension("json.tmp").exists(), "no temporary file remains");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn paused_learning_applies_choices_but_records_nothing() {
    let (mut s, prefs) = session();
    typed(&mut s, "rm");
    s.handle(Key::Candidate);
    s.handle(Key::Space);
    assert_eq!(
        prefs.count(),
        1,
        "a confirmed choice is remembered (no preceding word: global entry only)"
    );
    prefs.set_recording(false);
    let before = prefs.version();
    typed(&mut s, "dh");
    s.handle(Key::Candidate);
    let r = s.handle(Key::Space);
    assert!(
        !r.learned && prefs.version() == before,
        "nothing new is recorded while paused"
    );
    s.cancel();
    assert_eq!(typed(&mut s, "rm").marked, "그", "remembered choices still apply");
    prefs.set_recording(true);
    assert!(prefs.recording());
}
