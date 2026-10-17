use hanautomata_assist::Library;
use unicode_normalization::UnicodeNormalization;

#[test]
fn explicit_registration_nfc_and_duplicate_preserve_identity() {
    let mut lib = Library::default();
    let id = lib.add("  검토  후 회신드리겠습니다. ").unwrap();
    assert_eq!(id, lib.add(&"검토 후 회신드리겠습니다.".nfd().collect::<String>()).unwrap());
    assert_eq!(lib.list().len(), 1);
    assert_eq!(lib.list()[0].text, "검토 후 회신드리겠습니다.");
    assert!(lib.query("검토", 1).is_empty());
    lib.set_enabled(true);
    assert_eq!(lib.query("검토", 1)[0].id, id);
    assert!(lib.query("검", 1).is_empty());
    assert!(lib.query("", 1).is_empty());
    assert!(lib.query("검토 후 회신드리겠습니다.", 1).is_empty());
}

#[test]
fn top_three_and_copy_feedback_can_be_undone_once() {
    let mut lib = Library::default();
    lib.set_enabled(true);
    for text in ["검토 결과 가", "검토 결과 나", "검토 결과 다", "검토 결과 라"] {
        lib.add(text).unwrap();
    }
    let before = lib.query("검토", 100);
    assert_eq!(before.len(), 3);
    let id = lib.list()[3].id.clone();
    let token = lib.copied(&id, 100).unwrap();
    assert_eq!(lib.query("검토", 100)[0].id, id);
    lib.undo(&token).unwrap();
    assert_eq!(lib.query("검토", 100)[0].id, before[0].id);
    assert!(lib.undo(&token).is_err());
}

#[test]
fn deleting_clearing_and_pausing_invalidate_feedback() {
    let mut lib = Library::default();
    lib.set_enabled(true);
    let id = lib.add("회의 자료를 보내드립니다.").unwrap();
    let token = lib.copied(&id, 10).unwrap();
    lib.set_enabled(false);
    assert!(lib.copied(&id, 10).is_err());
    assert!(lib.undo(&token).is_err());
    lib.set_enabled(true);
    let token = lib.copied(&id, 10).unwrap();
    lib.delete(&id).unwrap();
    assert!(lib.undo(&token).is_err());
    assert!(lib.query("회의", 10).is_empty());
    lib.add("회의 시간입니다.").unwrap();
    lib.clear();
    assert!(lib.list().is_empty());
}

#[test]
fn limits_control_characters_and_unicode_prefix() {
    let mut lib = Library::default();
    lib.set_enabled(true);
    for bad in ["", "가", "ab\ncd", "ab\tcd", "ab\u{202e}cd", "ab\0cd"] {
        assert!(lib.add(bad).is_err(), "{bad:?}");
    }
    assert!(lib.add(&"가".repeat(257)).is_err());
    let id = lib.add("Hello World").unwrap();
    assert_eq!(lib.query("heLLo", 1)[0].id, id);
    lib.add("😀 한글 표현").unwrap();
    assert_eq!(lib.query("😀 한", 1).len(), 1);
    assert!(lib.query(&"a".repeat(257), 1).is_empty());
    for i in 2..512 {
        lib.add(&format!("표현 {i}")).unwrap();
    }
    assert!(lib.add("용량 초과").is_err());
    assert_eq!(lib.list().len(), 512);
}

#[test]
fn recent_feedback_outweighs_decayed_history() {
    let mut lib = Library::default();
    lib.set_enabled(true);
    let old = lib.add("자료 가").unwrap();
    let recent = lib.add("자료 나").unwrap();
    for _ in 0..8 {
        lib.copied(&old, 1).unwrap();
    }
    lib.copied(&recent, 181).unwrap();
    assert_eq!(lib.query("자료", 181)[0].id, recent);
    let token = lib.copied(&old, 181).unwrap();
    // A later registration invalidates an older feedback token, preventing resurrection.
    lib.add("자료 다").unwrap();
    assert!(lib.undo(&token).is_err());
}
