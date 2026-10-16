// Original synthetic examples, MIT. These test our contract, not Korean accuracy.
use hanautomata_proofread::*;
use unicode_normalization::UnicodeNormalization;

struct TinyLexicon;
impl Lexicon for TinyLexicon {
    fn contains(&mut self, s: &str) -> bool {
        matches!(s, "됐다" | "안녕하세요" | "돼요" | "학교에" | "먹었습니다")
    }
    fn suggest(&mut self, s: &str) -> Vec<String> {
        if s == "됬다" {
            vec![
                "됐다".into(),
                "됐다".nfd().collect(),
                "됬다".into(),
                "학교에".into(),
                "돼요".into(),
                "안녕하세요".into(),
            ]
        } else {
            vec![]
        }
    }
    fn engine_id(&self) -> String {
        "synthetic-fixture".into()
    }
    fn dictionary_id(&self) -> String {
        "original-MIT".into()
    }
}
fn request(s: &str) -> Request {
    Request {
        request_id: "r1".into(),
        document_id: "doc1".into(),
        revision: "1".into(),
        text: s.into(),
        protected_words: vec![],
    }
}
fn check(r: &Request) -> Report {
    Checker::new(TinyLexicon).check(r).unwrap()
}

#[test]
fn known_words_are_not_diagnostics() {
    let r = check(&request("안녕하세요. 돼요! 학교에 먹었습니다"));
    assert!(r.diagnostics.is_empty());
    assert_eq!(r.checked_words, 4);
    assert!(!r.limited);
}

#[test]
fn utf16_ranges_and_preview_preserve_surrounding_text() {
    let q = request("😀\r\n'됬다', 안녕하세요");
    let r = check(&q);
    let d = &r.diagnostics[0];
    assert_eq!(d.utf8, Span { start: 7, end: 13 });
    assert_eq!(d.utf16, Span { start: 5, end: 7 });
    assert_eq!(d.kind, "unknown_word");
    assert_eq!(d.suggestions, ["됐다", "학교에", "돼요"]);
    assert_eq!(apply_suggestion(&q, &r, 0, 0).unwrap(), "😀\r\n'됐다', 안녕하세요");
    assert_eq!(q.text, "😀\r\n'됬다', 안녕하세요");
}

#[test]
fn decomposed_hangul_keeps_original_span() {
    let word: String = "됬다".nfd().collect();
    let q = request(&format!("😀 {word}."));
    let r = check(&q);
    let d = &r.diagnostics[0];
    assert_eq!(d.original, word);
    assert_eq!(d.normalized, "됬다");
    assert_eq!(d.utf8, Span { start: 5, end: 20 });
    assert_eq!(d.utf16, Span { start: 3, end: 8 });
    assert_eq!(apply_suggestion(&q, &r, 0, 0).unwrap(), "😀 됐다.");
}

#[test]
fn urls_email_paths_mixed_words_and_code_are_protected() {
    for s in [
        "https://example.test/됬다",
        "mailto:됬다@example.test",
        "API됬다",
        "2026됬다",
        "됬다_test",
        "/tmp/됬다",
        "`됬다`",
        "```ko\n됬다\n```",
        "`unclosed 됬다",
        "<a title=\"됬다\">",
        "[됬다](url)",
    ] {
        let r = check(&request(s));
        assert!(r.diagnostics.is_empty(), "{s}");
        assert!(!r.skipped.is_empty(), "{s}");
    }
}

#[test]
fn normalized_protected_words_are_request_local() {
    let mut q = request("됬다 됬다는");
    q.protected_words.push("됬다".nfd().collect());
    let r = check(&q);
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].original, "됬다는");
    assert_eq!(check(&request("됬다")).diagnostics.len(), 1);
}

#[test]
fn unknown_names_are_uncertain_even_without_candidates() {
    let r = check(&request("가나다람"));
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].kind, "unknown_word");
    assert!(r.diagnostics[0].suggestions.is_empty());
}

#[test]
fn stale_or_foreign_snapshots_and_invalid_indices_are_rejected() {
    let q = request("됬다");
    let r = check(&q);
    for field in 0..4 {
        let mut current = q.clone();
        match field {
            0 => current.request_id.push('2'),
            1 => current.document_id.push('2'),
            2 => current.revision.push('2'),
            _ => current.text.push(' '),
        };
        assert!(apply_suggestion(&current, &r, 0, 0).is_err());
    }
    assert!(apply_suggestion(&q, &r, 8, 0).is_err());
    assert!(apply_suggestion(&q, &r, 0, 8).is_err());
    let mut bad = r.clone();
    bad.diagnostics[0].utf8.start = 1;
    assert!(apply_suggestion(&q, &bad, 0, 0).is_err());
    let mut bad = r.clone();
    bad.diagnostics[0].utf16.end = 8;
    assert!(apply_suggestion(&q, &bad, 0, 0).is_err());
}

#[test]
fn resource_limits_are_visible_and_invalid_input_is_not_clean() {
    assert!(Checker::new(TinyLexicon)
        .check(&request(&"a".repeat(MAX_TEXT_BYTES + 1)))
        .is_err());
    assert!(Checker::new(TinyLexicon).check(&request("됬다\0")).is_err());
    let mut q = request("됬다");
    q.request_id.clear();
    assert!(Checker::new(TinyLexicon).check(&q).is_err());
    let r = check(&request(&"안녕하세요 ".repeat(MAX_WORDS + 1)));
    assert_eq!(r.checked_words, MAX_WORDS);
    assert!(r.limited);
    let r = check(&request(&"됬다 ".repeat(MAX_DIAGNOSTICS + 1)));
    assert_eq!(r.diagnostics.len(), MAX_DIAGNOSTICS);
    assert!(r.limited);
    let r = check(&request(&"가".repeat(MAX_WORD_CHARS + 1)));
    assert_eq!(r.checked_words, 0);
    assert!(r.limited);
}

#[test]
fn empty_text_and_uncomposed_jamo_do_not_crash_or_claim_correction() {
    assert_eq!(check(&request("")).checked_words, 0);
    let r = check(&request("ㄱㄴ ᄀ 😀"));
    assert!(r.diagnostics.is_empty());
    assert_eq!(r.checked_words, 0);
}

#[test]
fn repeated_words_reuse_work_only_within_one_request_and_keep_distinct_ranges() {
    use std::{cell::Cell, rc::Rc};
    struct Counting(Rc<Cell<usize>>);
    impl Lexicon for Counting {
        fn contains(&mut self, _: &str) -> bool {
            self.0.set(self.0.get() + 1);
            false
        }
        fn suggest(&mut self, _: &str) -> Vec<String> {
            vec!["됐다".into()]
        }
        fn engine_id(&self) -> String {
            "counting".into()
        }
        fn dictionary_id(&self) -> String {
            "synthetic".into()
        }
    }
    let count = Rc::new(Cell::new(0));
    let mut checker = Checker::new(Counting(Rc::clone(&count)));
    let report = checker.check(&request("됬다 됬다")).unwrap();
    assert_eq!(report.checked_words, 2);
    assert_eq!(report.diagnostics.len(), 2);
    assert_ne!(report.diagnostics[0].utf8, report.diagnostics[1].utf8);
    assert_eq!(count.get(), 1);
    checker.check(&request("됬다")).unwrap();
    assert_eq!(count.get(), 2);
}
