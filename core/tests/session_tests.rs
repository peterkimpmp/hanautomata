// Input-method session (OS-neutral key handling) and learning-file store (C-OPS-09).

use hanautomata_core::{Detector, Key, LearnedWord, Mode, Response, Session, Store, WordPreferences, MAX_WORD};
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
