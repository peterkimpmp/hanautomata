// Input-method features with no C# counterpart (all off by default, so the parity gate G1 is unaffected): Sebeolsik
// layouts (C-LAY-02), syllable deletion (C-CMP-06), MS Windows consonant clusters (C-CMP-08), strict protection for app
// profiles (C-MOD-06), exception lists (C-LEX-02), forgetting one learned word (C-LEX-03), the export/import bundle
// (C-LEX-04), hanja for one syllable (C-LEX-06) and the Hangul ↔ QWERTY flip round trip (C-COR-06).

use hanautomata_core::hangul::{compose_layout, flip, ms_clusters, sebeol_jamo};
use hanautomata_core::{bundle, hanja, parse_list, Detector, Exceptions, Key, Layout, Mode, Session, WordPreferences};
use std::sync::Arc;

fn session() -> (Session, Arc<WordPreferences>) {
    let prefs = Arc::new(WordPreferences::new());
    (Session::new(Arc::new(Detector::with_preferences("", prefs.clone()))), prefs)
}

fn typed(s: &mut Session, text: &str) -> String {
    let mut marked = String::new();
    for c in text.chars() {
        marked = s.handle(Key::Text { raw: c, key: c }).marked;
    }
    marked
}

fn syllable(l: u32, v: u32, t: u32) -> String {
    char::from_u32(0xac00 + (l * 21 + v) * 28 + t).unwrap().to_string()
}

/// Key sequence typing one conjoining jamo in `layout`: a single key, or two keys that combine.
fn jamo_keys(layout: Layout, jamo: u32, probe: impl Fn(&str) -> bool) -> String {
    let keys: Vec<char> = ('!'..='~').collect();
    if let Some(k) = keys.iter().find(|&&k| sebeol_jamo(layout, k) as u32 == jamo) {
        return k.to_string();
    }
    for &a in &keys {
        for &b in &keys {
            let pair = format!("{a}{b}");
            if probe(&pair) {
                return pair;
            }
        }
    }
    panic!("{layout:?}: no keys for U+{jamo:04X}");
}

#[test]
fn sebeolsik_layouts_type_all_11172_syllables() {
    for layout in [Layout::Sebeolsik390, Layout::SebeolsikFinal] {
        let ieung = jamo_keys(layout, 0x110b, |_| false);
        let a = jamo_keys(layout, 0x1161, |_| false);
        let cho: Vec<String> = (0..19)
            .map(|l| {
                jamo_keys(layout, 0x1100 + l, |k| {
                    compose_layout(layout, &format!("{k}{a}")) == syllable(l, 0, 0)
                })
            })
            .collect();
        let jung: Vec<String> = (0..21)
            .map(|v| {
                jamo_keys(layout, 0x1161 + v, |k| {
                    compose_layout(layout, &format!("{ieung}{k}")) == syllable(11, v, 0)
                })
            })
            .collect();
        let jong: Vec<String> = (1..28)
            .map(|t| {
                jamo_keys(layout, 0x11a7 + t, |k| {
                    compose_layout(layout, &format!("{ieung}{a}{k}")) == syllable(11, 0, t)
                })
            })
            .collect();
        let mut count = 0;
        for l in 0..19u32 {
            for v in 0..21u32 {
                for t in 0..28u32 {
                    let keys = format!(
                        "{}{}{}",
                        cho[l as usize],
                        jung[v as usize],
                        if t == 0 { "" } else { &jong[t as usize - 1] }
                    );
                    assert_eq!(compose_layout(layout, &keys), syllable(l, v, t), "{layout:?} {keys}");
                    count += 1;
                }
            }
        }
        assert_eq!(count, 11_172);
    }
}

#[test]
fn sebeolsik_keeps_finals_in_their_syllable() {
    // 390: k ㄱ · f ㅏ · s ㄴ(final) · j ㅇ — a final never moves to the next syllable (unlike Dubeolsik).
    assert_eq!(compose_layout(Layout::Sebeolsik390, "kfsjf"), "간아");
    assert_eq!(compose_layout(Layout::Sebeolsik390, "kk"), "ㄲ", "doubled initial");
    assert_eq!(compose_layout(Layout::Sebeolsik390, "f"), "ㅏ", "a vowel alone");
    assert_eq!(
        compose_layout(Layout::Sebeolsik390, "kvf"),
        "과",
        "compound vowel from two keys"
    );
    assert_eq!(
        compose_layout(Layout::Sebeolsik390, "kfwx"),
        "갉",
        "compound final from two keys"
    );
    assert_eq!(compose_layout(Layout::Sebeolsik390, "kf."), "가.", "symbols pass");
    assert_eq!(compose_layout(Layout::Dubeolsik, "dkssud"), "안녕");
}

#[test]
fn sebeolsik_composes_in_both_hangul_modes_and_switches_like_a_fixed_mode() {
    let (mut s, prefs) = session();
    s.set_layout(Layout::Sebeolsik390);
    assert_eq!(
        typed(&mut s, "kfs"),
        "간",
        "the automatic mode composes Sebeolsik keys as Hangul"
    );
    assert_eq!(s.handle(Key::Space).commit, "간 ");
    assert_eq!(prefs.count(), 0, "nothing is learned from a fixed layout");
    typed(&mut s, "kf");
    let r = s.handle(Key::Switch);
    assert_eq!(
        (r.commit.as_str(), s.mode()),
        ("가", Mode::English),
        "a switch key toggles to English"
    );
    s.set_mode(Mode::Korean);
    assert_eq!(typed(&mut s, "kf;"), "가ㅂ", "`;` is a key (ㅂ), not the candidate shortcut");
    let r = s.set_layout(Layout::Dubeolsik);
    assert_eq!(r.commit, "가ㅂ", "changing the layout commits first");
}

#[test]
fn backspace_can_remove_a_whole_syllable() {
    let (mut s, _) = session();
    s.set_delete_syllable(true);
    typed(&mut s, "dkssud");
    assert_eq!(s.handle(Key::Backspace).marked, "안");
    assert_eq!(s.handle(Key::Backspace).marked, "");
    assert!(!s.is_composing());
    typed(&mut s, "hello");
    assert_eq!(s.handle(Key::Backspace).marked, "hell", "a word shown as typed loses one key");
    s.cancel();
    s.set_delete_syllable(false);
    s.set_mode(Mode::Korean);
    typed(&mut s, "dkssud");
    assert_eq!(s.handle(Key::Backspace).marked, "안녀", "default: one key");
    s.set_delete_syllable(true);
    assert_eq!(s.handle(Key::Backspace).marked, "안", "Korean mode: the syllable");
}

#[test]
fn ms_windows_clusters_join_lone_consonants() {
    assert_eq!(ms_clusters("ㄱㅅ"), "ㄳ");
    assert_eq!(ms_clusters("ㄹㄱㄱ"), "ㄺㄱ");
    assert_eq!(ms_clusters("ㄱ사"), "ㄱ사", "a consonant followed by a syllable stays");
    assert_eq!(ms_clusters("ㅋㅋㅋ"), "ㅋㅋㅋ");
    let (mut s, _) = session();
    s.set_mode(Mode::Korean);
    assert_eq!(typed(&mut s, "rt"), "ㄱㅅ", "off by default (macOS style)");
    s.set_ms_clusters(true);
    assert_eq!(s.handle(Key::Space).commit, "ㄳ ");
    assert_eq!(typed(&mut s, "rtk"), "ㄱ사");
}

#[test]
fn strict_protection_keeps_statistical_estimates_as_typed() {
    let (mut s, _) = session();
    // Find a held-out style word the engine converts on statistics alone (a pattern estimate, "음절 통계").
    let words = ["qhanf", "vmflsxj", "rkrhddml", "djsdjrk", "dkrldi", "wjdqh", "rjsrkd"];
    let detector = Detector::new("");
    let word = words
        .iter()
        .find(|w| detector.decide(w).confidence == hanautomata_core::Confidence::Pattern)
        .expect("a pattern-only word");
    let korean = typed(&mut s, word);
    assert_ne!(korean, *word, "converted by default");
    s.set_strict(true);
    s.cancel();
    assert_eq!(typed(&mut s, word), *word, "strict: stays as typed");
    assert_eq!(s.handle(Key::Candidate).marked, korean, "and F2 still offers the Hangul");
    s.cancel();
    assert_eq!(typed(&mut s, "dkssud"), "안녕", "listed words still convert");
}

#[test]
fn exception_lists_outrank_the_decision_and_learning() {
    let (mut s, prefs) = session();
    assert_eq!(typed(&mut s, "rm"), "rm", "a near-tie stays raw");
    s.cancel();
    s.exceptions().set(&parse_list("# keep\ndkssud\n\n"), &["그".into()]);
    assert_eq!(typed(&mut s, "rm"), "그", "always-convert entry written in Hangul");
    s.cancel();
    assert_eq!(typed(&mut s, "rm,"), "그,", "edge punctuation is ignored");
    s.cancel();
    assert_eq!(typed(&mut s, "dkssud"), "dkssud", "never-convert entry");
    assert_eq!(s.handle(Key::Candidate).marked, "안녕", "F2 still overrides for this word");
    s.cancel();
    prefs.remember_global("rm", "rm", false);
    prefs.remember_global("rm", "rm", false);
    assert_eq!(typed(&mut s, "rm"), "그", "the list outranks a learned choice");
    s.cancel();
    assert_eq!(s.exceptions().keep(), vec!["dkssud".to_string()]);
    s.exceptions().set(&[], &[]);
    assert_eq!(
        typed(&mut s, "rm"),
        "rm",
        "cleared lists apply at once (the learned English choice returns)"
    );
}

#[test]
fn one_learned_word_can_be_forgotten() {
    let (mut s, prefs) = session();
    typed(&mut s, "gksrmf");
    s.handle(Key::Space);
    typed(&mut s, "rm");
    s.handle(Key::Candidate);
    s.handle(Key::Space);
    assert_eq!(prefs.count(), 2, "global and after 한글");
    assert_eq!(prefs.forget("rm", "rm"), 2);
    assert_eq!(prefs.count(), 0);
    assert!(!prefs.can_undo(), "undo cannot bring it back");
    assert_eq!(prefs.forget("rm", "rm"), 0);
}

#[test]
fn bundle_round_trip_moves_words_lists_and_settings() {
    let (mut a, prefs_a) = session();
    typed(&mut a, "rm");
    a.handle(Key::Candidate);
    a.handle(Key::Space);
    a.exceptions().set(&["dkssud".into()], &["고".into()]);
    let text = bundle::export(&prefs_a, a.exceptions(), r#"{"mode":"auto","capsLock":true}"#).unwrap();
    assert!(text.contains("\"Kind\": \"hanautomata-bundle\"") && text.contains("\"Format\": 2"));

    let (mut b, prefs_b) = session();
    let imported = bundle::import(&text, &prefs_b, b.exceptions()).unwrap();
    assert_eq!((imported.words, imported.keep, imported.convert), (1, 1, 1));
    assert!(imported.settings.contains("\"capsLock\":true"));
    assert_eq!(prefs_b.count(), 1);
    assert_eq!(typed(&mut b, "rm"), "그", "the same decision after import");
    b.cancel();
    assert_eq!(typed(&mut b, "rh"), "고");
    // Importing again does not duplicate.
    bundle::import(&text, &prefs_b, b.exceptions()).unwrap();
    assert_eq!((prefs_b.count(), b.exceptions().convert().len()), (1, 1));
    assert!(bundle::import("{\"Format\":1}", &prefs_b, b.exceptions()).is_err());
    assert!(bundle::import(
        "{\"Format\":1,\"Kind\":\"other\",\"Words\":{\"Format\":2}}",
        &prefs_b,
        b.exceptions()
    )
    .is_err());
    let empty = Exceptions::default();
    assert!(bundle::export(&WordPreferences::new(), &empty, "not json")
        .unwrap()
        .contains("\"Settings\": {}"));
}

#[test]
fn hanja_for_one_syllable() {
    let common = hanja('한', false);
    assert!(common.contains(&'韓') && common.contains(&'漢') && common.contains(&'寒'));
    assert_eq!(common[0], '寒', "basic educational hanja first, in KS X 1001 order");
    let all = hanja('한', true);
    assert!(all.len() > common.len() && all.starts_with(&common));
    assert!(hanja('a', true).is_empty() && hanja('똠', false).is_empty());
    assert!(hanja('천', false).starts_with(&['千', '天', '川']));
}

#[test]
fn flip_round_trips_1000_characters() {
    // Lowercase QWERTY and modern Hangul (C-COR-06 acceptance), from a fixed pseudo-random sequence.
    let mut state: u64 = 7;
    let mut next = |n: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % n
    };
    let mut latin = String::new();
    while latin.chars().count() < 1000 {
        for _ in 0..(1 + next(8)) {
            latin.push((b'a' + next(26) as u8) as char);
        }
        latin.push(' ');
    }
    assert_eq!(flip(&flip(&latin)), latin);
    let mut hangul = String::new();
    while hangul.chars().count() < 1000 {
        for _ in 0..(1 + next(4)) {
            hangul.push(char::from_u32(0xac00 + next(11_172) as u32).unwrap());
        }
        hangul.push(' ');
    }
    assert_eq!(flip(&flip(&hangul)), hangul);
}

#[test]
fn caret_text_language_breaks_near_ties_like_a_preceding_word() {
    use hanautomata_core::hangul::dominant_language;
    assert_eq!(dominant_language("오늘 회의는 세 시에 "), Some(true));
    assert_eq!(dominant_language("see you at the meeting "), Some(false));
    assert_eq!(dominant_language("오늘 회의 meeting "), None, "mixed text decides nothing");
    assert_eq!(dominant_language("네 "), None, "too short");
    let (mut s, _) = session();
    assert_eq!(typed(&mut s, "dh"), "dh", "a near-tie stays raw without context");
    s.cancel();
    s.set_caret_text("어제 보낸 메일 확인했어요 ");
    assert_eq!(typed(&mut s, "dh"), "오", "Korean text before the caret tips it");
    s.handle(Key::Enter);
    assert_eq!(typed(&mut s, "dh"), "dh", "the caret language lasts one word");
}

/// Held-out effect of the caret-text language (C-DET-08 gate data, reported in macos/README.md): same-language context
/// never adds errors; the opposite context is the risk that keeps the option off by default.
#[test]
fn caret_context_effect_on_held_out_words() {
    let detector = Detector::new("");
    let read = |path: &str| -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split('\t').next().map(str::to_string))
            .collect()
    };
    let korean = read("../tests/corpora/heldout-korean-1000.tsv");
    let english = read("../tests/corpora/heldout-english-1000.tsv");
    let wrong = |words: &[String], truth: bool, context: Option<bool>| {
        words
            .iter()
            .filter(|w| detector.decide_with(w, w, "", context).is_korean != truth)
            .count()
    };
    let (ko_none, ko_same, ko_opposite) = (
        wrong(&korean, true, None),
        wrong(&korean, true, Some(true)),
        wrong(&korean, true, Some(false)),
    );
    let (en_none, en_same, en_opposite) = (
        wrong(&english, false, None),
        wrong(&english, false, Some(false)),
        wrong(&english, false, Some(true)),
    );
    println!("korean misses: none {ko_none} · Korean text {ko_same} · English text {ko_opposite}");
    println!("english wrong: none {en_none} · English text {en_same} · Korean text {en_opposite}");
    assert!(
        ko_same <= ko_none && en_same <= en_none,
        "same-language context never adds errors"
    );
    assert!(
        ko_opposite <= ko_none + 5 && en_opposite <= 10,
        "the opposite-context risk stays small"
    );
}
