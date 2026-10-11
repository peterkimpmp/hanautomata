// Port of tests/CoreTests.cs (in-memory parts). The C# suite's file-store checks (PreferenceStore, Windows sharing locks)
// stay with the platform shells. Labels match the C# assertions so failures can be traced back.

use hanautomata_core::{
    can_capture, hangul, preset_thresholds, Composition, Confidence, Detector, LearnedWord, WordPreferences, CAPACITY,
    DEFAULT_ENGLISH_THRESHOLD, DEFAULT_KOREAN_THRESHOLD, MIXED_MARGIN,
};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
struct T {
    count: usize,
    failures: Vec<String>,
}

impl T {
    fn eq(&mut self, expected: impl AsRef<str>, actual: impl AsRef<str>, label: impl AsRef<str>) {
        self.count += 1;
        if expected.as_ref() != actual.as_ref() {
            self.failures.push(format!(
                "{}: expected [{}] actual [{}]",
                label.as_ref(),
                expected.as_ref(),
                actual.as_ref()
            ));
        }
    }
    fn truth(&mut self, actual: bool, label: impl AsRef<str>) {
        self.eq("true", actual.to_string(), label);
    }
    fn finish(self, name: &str) {
        println!("{name}: {} assertions, {} failures", self.count, self.failures.len());
        assert!(self.failures.is_empty(), "{name}:\n{}", self.failures.join("\n"));
    }
}

fn detector() -> Arc<Detector> {
    Arc::new(Detector::new(""))
}
fn shared(prefs: &Arc<WordPreferences>) -> Arc<Detector> {
    Arc::new(Detector::with_preferences("", prefs.clone()))
}
fn type_text(c: &mut Composition, text: &str) {
    for ch in text.chars() {
        c.append(ch, ch);
    }
}
fn text(d: &Detector, raw: &str) -> String {
    d.decide(raw).text
}
fn text_with(d: &Detector, raw: &str, keys: &str, context: &str, prev: Option<bool>) -> String {
    d.decide_with(raw, keys, context, prev).text
}
/// NFD of a precomposed syllable (conjoining jamo), without a normalization library.
fn nfd(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let u = c as u32;
        if (0xac00..=0xd7a3).contains(&u) {
            let cp = u - 0xac00;
            out.push(char::from_u32(0x1100 + cp / 588).unwrap());
            out.push(char::from_u32(0x1161 + cp % 588 / 28).unwrap());
            if cp % 28 > 0 {
                out.push(char::from_u32(0x11a7 + cp % 28).unwrap());
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn main_suite() {
    let mut t = T::default();
    for (keys, expected, label) in [
        ("dkssudgktpdy", "안녕하세요", "greeting"),
        ("gksrmf", "한글", "basic"),
        ("rkqtdjqtsms", "값없는", "compound finals"),
        ("ekfrrkfql", "닭갈비", "compound final"),
        ("ekfrl", "달기", "split compound final before vowel"),
        ("dkswk", "안자", "carry final"),
        ("Enlek", "뛰다", "shift and compound vowel"),
        ("rhoscksgdkdy", "괜찮아요", "mixed compound vowels and finals"),
        ("zzz", "ㅋㅋㅋ", "laughter"),
        ("bb", "ㅠㅠ", "standalone vowels"),
        ("dkssudgktpdy! 123", "안녕하세요! 123", "punctuation"),
    ] {
        t.eq(expected, hangul::compose(keys), label);
    }
    t.eq("hello", hangul::to_keys("ㅗ디ㅣㅐ"), "wrong-mode recovery");
    for cp in 0xac00..=0xd7a3u32 {
        let s = char::from_u32(cp).unwrap().to_string();
        t.eq(&s, hangul::compose(&hangul::to_keys(&s)), format!("syllable {cp}"));
    }
    let d = detector();
    for (raw, expected) in [
        ("gksrmffh", "한글로"),
        ("dleofh", "이대로"),
        ("skdml", "나의"),
        ("ahems", "모든"),
        ("rjtdmf", "것을"),
    ] {
        t.eq(expected, text(&d, raw), format!("user screenshot lowercase regression {raw}"));
    }
    for (raw, keys, expected, label) in [
        ("DKSSUDGKTPDY", "dkssudgktpdy", "안녕하세요", "Caps Lock Korean greeting"),
        ("EHDWKRDKSGKA", "ehdwkrdksgka", "동작안함", "Caps Lock user report"),
        ("EHDWKRGKEHFHR", "ehdwkrgkehfhr", "동작하도록", "Caps Lock user request"),
        ("GOWNJ", "gownj", "해줘", "Caps Lock short request"),
        (
            "hanautomataRK",
            "hanautomatark",
            "hanautomata가",
            "Caps Lock mixed product name",
        ),
        ("API", "api", "API", "Caps Lock known acronym"),
        ("NASA", "nasa", "NASA", "Caps Lock unrecognized acronym"),
        ("GO", "go", "GO", "Caps Lock ambiguous English"),
    ] {
        t.eq(expected, text_with(&d, raw, keys, "", None), label);
    }
    t.eq(
        "EHDWKRDKSGKA",
        text(&d, "EHDWKRDKSGKA"),
        "Shift uppercase acronym policy remains",
    );
    for (raw, expected) in [
        ("APIrotjs", "API개선"),
        ("AIrjawmd", "AI검증"),
        ("tjfrPAPI", "설계API"),
        ("AIrotjsAPIrjawmd", "AI개선API검증"),
        ("ghldmlmeetingwnsql", "회의meeting준비"),
        ("tndbfreport", "수율report"),
    ] {
        t.eq(expected, text(&d, raw), format!("intelligent mixed segmentation {raw}"));
    }
    for (raw, expected, label) in [
        ("dkssudgktpdy", "안녕하세요", "automatic Korean"),
        ("qksrkqtmqslek", "반갑습니다", "second Korean"),
        ("ghldml", "회의", "short common Korean"),
        ("vmfhwprxm", "프로젝트", "project Korean"),
        ("hello", "hello", "English greeting"),
        ("world", "world", "valid Korean-shaped English"),
        ("for", "for", "short English collision"),
        ("go", "go", "go versus hae ambiguity"),
        ("API", "API", "acronym"),
        ("OpenAI", "OpenAI", "product name"),
        ("github.com", "github.com", "domain"),
        ("peter@example.com", "peter@example.com", "email"),
        ("C:\\work\\report", "C:\\work\\report", "path"),
        ("user_name", "user_name", "identifier"),
        ("dkssudgktpdy!", "안녕하세요!", "sentence punctuation"),
        ("vy", "vy", "ambiguous one syllable retained"),
        ("zzz", "ㅋㅋㅋ", "Korean chat laughter"),
        ("dkssudzz", "안녕ㅋㅋ", "known Korean followed by laughter"),
        ("rkatkgkqslekbb!", "감사합니다ㅠㅠ!", "complete Korean followed by tears"),
        ("zgzg", "ㅋㅎㅋㅎ", "mixed Korean chat consonants"),
        ("bnbn", "ㅠㅜㅠㅜ", "mixed Korean chat vowels"),
        ("zgbn", "zgbn", "ambiguous consonant vowel chat remains literal"),
        ("dkssudz", "안녕ㅋ", "single chat suffix after known Korean"),
        ("hellozzz", "hellozzz", "English with chat-like suffix stays literal"),
        ("bb.com", "bb.com", "chat-like domain stays literal"),
        ("z", "z", "one isolated letter stays literal"),
        ("안녕ㅠㅠ", "안녕ㅠㅠ", "existing Korean chat stays unchanged"),
        ("doWork", "doWork", "English camelCase identifier stays literal"),
        ("DoWork", "DoWork", "English PascalCase identifier stays literal"),
        ("qkRnjdy", "바꿔요", "required Korean shift is not mistaken for camelCase"),
    ] {
        t.eq(expected, text(&d, raw), label);
    }
    for (raw, expected) in [
        ("dndb", "우유"),
        ("rotjs", "개선"),
        ("tndbf", "수율"),
        ("wjsfir", "전략"),
        ("dml", "의"),
        ("fmf", "를"),
        ("eogks", "대한"),
        ("tjfrP", "설계"),
        ("wheh", "조도"),
        ("didtks", "양산"),
        ("AIdml", "AI의"),
        ("flarefmf", "flare를"),
        ("thwochange,", "소재change,"),
        ("1988rhkwp", "1988과제"),
        ("4wja", "4점"),
        ("wkrdjqwltltj(tkdidtj)", "작업지시서(사양서)"),
        ("rjawmd/wjrdyd", "검증/적용"),
        ("roqkf/didtks", "개발/양산"),
        ("tlgja/rjawmd,", "시험/검증,"),
        ("wbs;tlwkdqnstjrdmfgksek,", "wbs;시장분석을한다,"),
        (
            "wjsfirtnflq-ASIS-aorfkrqnstjr-eodmdwjsfir-20261007",
            "전략수립-ASIS-맥락분석-대응전략-20261007",
        ),
        ("APIdml", "API의"),
        ("reportfmf", "report를"),
        ("(ghldml/wnsql)", "(회의/준비)"),
        ("10wja", "10점"),
        ("dnjsdlschange", "원인change"),
    ] {
        t.eq(expected, text(&d, raw), format!("mixed-input regression {raw}"));
    }
    for literal in [
        "rotjs@example.com",
        "https://example.com/rotjs",
        "C:\\work\\rotjs.txt",
        "user_rotjs",
        "sub1-sub2",
        "50mm",
        "95%",
        ">=",
        "clock-prd.md",
        "01-type-slug.md/html",
        "who.what.why",
        "Verification/Validation",
        "dori",
        "sudoku",
        "Café",
        "wbs",
        "DoD",
        "P.E",
    ] {
        t.eq(literal, text(&d, literal), format!("protected literal {literal}"));
    }
    let profile = Detector::new("C:\\Users\\Test사용자");
    t.eq(
        "C:\\Users\\Test사용자\\GitHub\\project",
        text(&profile, "C:\\Users\\Testtkdydwk\\GitHub\\project"),
        "exact known profile alias",
    );
    t.eq(
        "C:\\Users\\Testtkdydwk2\\GitHub",
        text(&profile, "C:\\Users\\Testtkdydwk2\\GitHub"),
        "profile alias requires segment boundary",
    );

    let mut c = Composition::new(d.clone());
    type_text(&mut c, "dkssud");
    t.eq("안녕", c.current().text, "preedit");
    c.backspace();
    t.eq("안녀", c.korean(), "one keystroke backspace");
    c.append('d', 'd');
    c.toggle();
    t.eq("dkssud", c.current().text, "manual English override");
    c.toggle();
    t.eq("안녕", c.current().text, "manual Korean override");
    c.clear();
    t.truth(c.is_empty(), "composition cleared");
    type_text(&mut c, "dkssud");
    c.toggle();
    while !c.is_empty() {
        c.backspace();
    }
    type_text(&mut c, "dkssud");
    t.eq(
        "안녕",
        c.current().text,
        "deleting everything resets manual English selection",
    );
    c.clear();
    c.toggle();
    type_text(&mut c, "hello");
    t.eq("hello", c.current().text, "toggle on empty buffer has no effect");
    c.clear();
    type_text(&mut c, "go");
    c.toggle();
    c.backspace();
    c.backspace();
    c.backspace();
    type_text(&mut c, "hello");
    t.eq(
        "hello",
        c.current().text,
        "deleting everything resets manual Korean selection",
    );
    c.clear();
    type_text(&mut c, "rkqtk");
    t.eq("갑사", c.korean(), "compound final carried before deletion");
    for expected in ["값", "갑", "가", "ㄱ", ""] {
        c.backspace();
        t.eq(expected, c.korean(), format!("backspace to {expected:?}"));
    }
    type_text(&mut c, "rhk");
    t.eq("과", c.korean(), "compound vowel before deletion");
    c.backspace();
    t.eq("고", c.korean(), "compound vowel loses one key");
    c.clear();
    type_text(&mut c, "akfrh");
    t.eq("말고", c.korean(), "final carry across pending word");
    c.backspace();
    t.eq("맑", c.korean(), "whole pending word undo restores prior final");
    c.clear();
    // es-hangul boundary cases, adapted to Hanautomata's whole-word physical-key undo.
    for (word, after) in [
        ("전화", "전호"),
        ("값", "갑"),
        ("깎", "까"),
        ("신세계", "신섹"),
        ("끓", "끌"),
        ("왅", "완"),
    ] {
        type_text(&mut c, &hangul::to_keys(word));
        c.backspace();
        t.eq(after, c.korean(), format!("reference deletion {word}"));
        c.clear();
    }
    let personal = detector();
    let mut p = Composition::new(personal.clone());
    type_text(&mut p, "vy");
    p.toggle();
    p.confirm_selection();
    p.clear();
    t.eq("표", text(&personal, "vy"), "confirmed F2 choice is remembered");
    type_text(&mut p, "rm");
    p.semicolon();
    t.eq("그", p.current().text, "semicolon selects the other candidate");
    p.confirm_selection();
    p.clear();
    t.eq("그", text(&personal, "rm"), "confirmed semicolon choice is remembered");
    type_text(&mut p, "hello");
    p.semicolon();
    p.semicolon();
    t.eq("hello;", p.current().text, "double semicolon escapes literal punctuation");
    t.finish("main");
}

#[test]
fn preference_suite() {
    let mut t = T::default();
    let words = Arc::new(WordPreferences::new());
    let d = shared(&words);
    let mut input = Composition::new(d.clone());
    // "rm" (그) is a statistical near-tie, so it stays literal until the user chooses; "gksms" converts automatically.
    type_text(&mut input, "rm");
    input.toggle();
    input.clear();
    t.eq("0", words.count().to_string(), "cancelled manual choice is not learned");
    type_text(&mut input, "rm");
    input.toggle();
    while !input.is_empty() {
        input.backspace();
    }
    type_text(&mut input, "hello");
    input.confirm_selection();
    input.clear();
    t.eq("0", words.count().to_string(), "deleted choice cannot train the next word");
    type_text(&mut input, "rm");
    input.semicolon();
    input.backspace();
    t.eq("rm", input.current().text, "backspace undoes pending semicolon toggle");
    t.truth(!input.confirm_selection(), "undone semicolon is not learned");
    input.clear();
    type_text(&mut input, "hello");
    input.semicolon();
    input.semicolon();
    t.eq("hello;", input.current().text, "literal semicolon restores previous choice");
    t.truth(!input.confirm_selection(), "literal escape cannot train the dictionary");
    input.clear();
    input.semicolon();
    t.eq(";", input.current().text, "semicolon without a word stays literal");
    input.clear();
    type_text(&mut input, "rm");
    input.semicolon();
    input.confirm_selection();
    input.clear();
    t.eq("그", text(&d, "rm"), "learned choice used without another shortcut");
    t.eq(
        "(그!)",
        text(&d, "(rm!)"),
        "surrounding punctuation preserved with learned word",
    );
    type_text(&mut input, "rm");
    input.toggle();
    input.confirm_selection();
    input.clear();
    t.eq("rm", text(&d, "rm"), "opposite confirmed choice replaces earlier preference");
    t.truth(words.undo_last(), "recent learning can be undone");
    t.eq("그", text(&d, "rm"), "undo restores earlier preference");
    words.set_enabled(false);
    t.eq("rm", text(&d, "rm"), "disabled personal choices do not apply");
    t.truth(
        !words.remember_global("ej", "ej", true),
        "disabled learning cannot add entries",
    );
    words.set_enabled(true);
    t.eq("그", text(&d, "rm"), "re-enabled choices remain available");
    let long = "a".repeat(65);
    for value in [
        "mail@example.com",
        "C:\\work\\file",
        "https://host/test",
        "api_key",
        "12345",
        long.as_str(),
    ] {
        t.truth(
            !words.remember_global(value, value, true),
            "paths identifiers and oversized inputs are not learned",
        );
    }
    words.remember_global("wbs", "wbs", true);
    t.eq("쥰", text(&d, "wbs"), "explicit choice can override a known English word");
    t.eq("WBS", text(&d, "WBS"), "case remains significant for learned keys");
    type_text(&mut input, "wbs");
    t.eq("쥰", input.current().text, "candidate cache initially learned");
    words.clear();
    t.eq("wbs", input.current().text, "clearing learning invalidates candidate cache");
    input.clear();
    for i in 0..=CAPACITY {
        let key = format!(
            "term{}{}{}",
            (b'a' + (i / 676) as u8) as char,
            (b'a' + (i / 26 % 26) as u8) as char,
            (b'a' + (i % 26) as u8) as char
        );
        words.remember_global(&key, &key, true);
    }
    t.eq(
        CAPACITY.to_string(),
        words.count().to_string(),
        "learning capacity is bounded",
    );
    t.truth(
        words.try_get_global("termaaa", "termaaa").is_none(),
        "oldest entry is evicted at capacity",
    );
    words.undo_last();
    t.truth(
        words.try_get_global("termaaa", "termaaa").is_some(),
        "undo also restores evicted entry",
    );
    t.finish("preferences");
}

#[test]
fn intelligence_suite() {
    let mut t = T::default();
    let words = Arc::new(WordPreferences::new());
    let d = shared(&words);
    let mut input = Composition::new(d.clone());
    let korean_context = hanautomata_core::context_key("나는");
    let english_context = hanautomata_core::context_key("please");
    words.remember("go", "go", true, &korean_context);
    words.remember("go", "go", false, &english_context);
    t.eq(
        "해",
        text_with(&d, "go", "go", &korean_context, None),
        "Korean context keeps its selected meaning",
    );
    t.eq(
        "go",
        text_with(&d, "go", "go", &english_context, None),
        "English context keeps its selected meaning",
    );
    t.eq(
        "go",
        text(&d, "go"),
        "conflicting global evidence does not spread a local choice",
    );
    t.truth(
        d.decide("go").needs_confirmation(),
        "conflicting evidence is visibly uncertain",
    );
    words.undo_last();
    t.eq(
        "해",
        text(&d, "go"),
        "one undo restores global and contextual learning together",
    );
    t.truth(words.count() == 2, "undo removes the English context as one transaction");
    words.remember("go", "go", false, &korean_context);
    t.eq(
        "go",
        text_with(&d, "go", "go", &korean_context, None),
        "same context immediately honors the latest correction",
    );
    words.clear();
    for _ in 0..6 {
        words.remember_global("go", "go", true);
    }
    words.remember_global("go", "go", false);
    t.eq(
        "해",
        text(&d, "go"),
        "one contradictory event does not erase repeated global evidence",
    );
    for _ in 0..8 {
        words.remember_global("go", "go", false);
    }
    t.eq("go", text(&d, "go"), "discounted history adapts to repeated new choices");
    words.clear();
    type_text(&mut input, "sksms");
    input.accept_commit(true);
    t.eq(
        &korean_context,
        input.context(),
        "space establishes preceding committed word context",
    );
    type_text(&mut input, "go");
    input.toggle();
    t.truth(input.confirm_selection(), "explicit contextual choice recorded");
    let revision = words.version();
    t.truth(
        !input.confirm_selection() && revision == words.version(),
        "a commit cannot count the same confirmation twice",
    );
    input.accept_commit(true);
    input.clear();
    type_text(&mut input, "sksms");
    input.accept_commit(true);
    type_text(&mut input, "go");
    t.eq("해", input.current().text, "composition uses its own preceding word");
    input.clear();
    t.eq("", input.context(), "cancel and focus reset remove context");
    type_text(&mut input, "hello");
    input.accept_commit(false);
    t.eq("", input.context(), "navigation or Enter clears context");
    type_text(&mut input, "hello!");
    input.accept_commit(true);
    t.eq("", input.context(), "sentence punctuation clears context");
    words.clear();
    for _ in 0..10 {
        type_text(&mut input, "hello");
        input.accept_commit(true);
    }
    t.eq(
        "0",
        words.count().to_string(),
        "automatic output is never fed back as a training label",
    );
    input.clear();
    t.truth(d.decide("rm").needs_confirmation(), "near-tie short input requires a choice");
    t.truth(!d.decide("world").needs_confirmation(), "known English stays stable");
    t.eq(
        "Pattern",
        d.decide("dkssudgktlqslRk").confidence.as_str(),
        "syllable heuristic is not claimed as lexical certainty",
    );
    let long = "a".repeat(65);
    for value in ["mail@example.com", "user_id", "hello!", "12", long.as_str()] {
        t.eq("", hanautomata_core::context_key(value), "unsafe context is not retained");
    }
    t.truth(
        !words.remember("go", "go", true, "plain text context"),
        "only bounded context fingerprints can be stored",
    );
    let invalid = LearnedWord {
        raw: "go".into(),
        keys: "go".into(),
        korean: true,
        sequence: 0,
        context: String::new(),
        korean_weight: f64::NAN,
        english_weight: 0.0,
    };
    words.restore(&[invalid]);
    t.eq("0", words.count().to_string(), "invalid numeric model data is rejected");
    // Format 1 entries carry only the last choice and migrate as one explicit observation.
    let legacy = LearnedWord {
        raw: "gksms".into(),
        keys: "gksms".into(),
        korean: true,
        sequence: 1,
        context: String::new(),
        korean_weight: 0.0,
        english_weight: 0.0,
    };
    let migrated = Arc::new(WordPreferences::new());
    migrated.restore(&[legacy]);
    t.eq(
        "하는",
        text(&Detector::with_preferences("", migrated.clone()), "gksms"),
        "format 1 personal dictionary migrates without losing choices",
    );
    migrated.set_enabled(false);
    migrated.remember("go", "go", true, &korean_context);
    t.eq(
        "go",
        text_with(&Detector::with_preferences("", migrated), "go", "go", &korean_context, None),
        "learning off also disables contextual prediction",
    );
    t.finish("intelligence");
}

#[test]
fn round2_suite() {
    let mut t = T::default();
    let d = detector();
    for value in ["핵심", "개선을", "회의에서", "개선합니다", "검증하고", "설계해주세요"] {
        let raw = hangul::to_keys(value);
        t.eq(value, text(&d, &raw), format!("round2 lexical/morphological {value}"));
        t.eq(
            "Known",
            d.decide(&raw).confidence.as_str(),
            format!("round2 lexical evidence {value}"),
        );
    }
    let k = hangul::to_keys;
    for (raw, expected) in [
        (format!("API{}", k("개선합니다")), "API개선합니다"),
        (format!("report{}", k("검증하고")), "report검증하고"),
        (format!("{}report", k("회의에서")), "회의에서report"),
        (
            format!("AI{}API{}SDK{}HTML{}", k("핵심"), k("개선"), k("검증"), k("결과")),
            "AI핵심API개선SDK검증HTML결과",
        ),
    ] {
        t.eq(expected, text(&d, &raw), format!("round2 mixed morphology {raw}"));
    }
    for word in ["각", "값", "뷁", "한글", "힣"] {
        t.eq(
            hangul::to_keys(word),
            hangul::to_keys(&nfd(word)),
            format!("canonical jamo keys {word}"),
        );
    }
    t.eq(
        format!("Cafe\u{301}-{}", k("한글")),
        hangul::to_keys(&nfd("Cafe\u{301}-한글")),
        "non-Hangul combining marks preserved",
    );
    for cp in 0xac00..=0xd7a3u32 {
        let s = char::from_u32(cp).unwrap().to_string();
        t.eq(
            &s,
            hangul::compose(&hangul::to_keys(&nfd(&s))),
            format!("NFD roundtrip U+{cp:04X}"),
        );
    }
    let long_mixed = format!("AI{}API{}SDK{}HTML{}API", k("핵심"), k("개선"), k("검증"), k("결과"));
    t.eq(
        &long_mixed,
        text(&d, &long_mixed),
        "ninth alternating span is outside automatic decoding bound",
    );
    for gap in [0, 1, 4] {
        let mut colliding = Detector::new("");
        colliding.add_lexeme_for_test("가나", true, 10);
        colliding.add_lexeme_for_test("rk", false, 10);
        colliding.add_lexeme_for_test("가", true, 10);
        colliding.add_lexeme_for_test("rksk", false, 10 + gap);
        let choice = colliding.decide("rkrksk");
        let abstains = gap < MIXED_MARGIN;
        t.eq(
            if abstains { "rkrksk" } else { "rk가나" },
            &choice.text,
            format!("candidate cost gap {gap}"),
        );
        t.eq(
            abstains.to_string(),
            choice.needs_confirmation().to_string(),
            format!("abstention gap {gap}"),
        );
        if abstains {
            t.truth(
                choice.alternative == "rk가나" || choice.alternative == "가rksk",
                "abstained mixed candidate remains selectable",
            );
            let mut input = Composition::new(Arc::new(colliding));
            type_text(&mut input, "rkrksk");
            input.toggle();
            t.eq(
                &choice.alternative,
                input.current().text,
                "F2 selects the best mixed alternative after abstention",
            );
        }
    }
    t.finish("round2");
}

#[test]
fn developer_and_chat_suite() {
    let mut t = T::default();
    let preferences = Arc::new(WordPreferences::new());
    let d = shared(&preferences);
    for token in [
        "gh",
        "pr",
        "mcp",
        "rag",
        "llm",
        "gpt",
        "cli",
        "tui",
        "ci",
        "cd",
        "qa",
        "ocr",
        "npx",
        "pnpm",
        "bun",
        "deno",
        "uv",
        "ssh",
        "scp",
        "rsync",
        "curl",
        "wget",
        "grep",
        "sed",
        "awk",
        "jq",
        "yq",
        "fd",
        "rg",
        "fzf",
        "tmux",
        "zsh",
        "sh",
        "ls",
        "cp",
        "mv",
        "dd",
        "tr",
        "gcc",
        "cmake",
        "chmod",
        "chown",
        "sudo",
        "grpc",
        "lsp",
        "lgtm",
        "sha256",
        "sha512",
        "sha256sum",
        "md5",
        "utf8",
        "base64",
        "k8s",
        "OAuth2",
    ] {
        t.eq(token, text(&d, token), format!("developer token stays literal {token}"));
        t.eq(
            token,
            text_with(&d, token, token, "", Some(true)),
            format!("Korean context cannot convert developer token {token}"),
        );
        t.truth(
            !d.decide(token).needs_confirmation(),
            format!("known developer token needs no confirmation {token}"),
        );
    }
    for token in [
        "(gh!)",
        "sha256,",
        "mcp/gh",
        "/gh",
        "--mcp",
        "gh.exe",
        "user_mcp",
        "mcp@example.com",
    ] {
        t.eq(
            token,
            text(&d, token),
            format!("developer token keeps surrounding syntax {token}"),
        );
    }
    for (raw, expected) in [
        ("ee", "ㄷㄷ"),
        ("eee", "ㄷㄷㄷ"),
        ("ddd", "ㅇㅇㅇ"),
        ("ss", "ㄴㄴ"),
        ("rr", "ㄱㄱ"),
        ("qq", "ㅂㅂ"),
        ("tt", "ㅅㅅ"),
        ("ww", "ㅈㅈ"),
        ("cc", "ㅊㅊ"),
        ("dz", "ㅇㅋ"),
        ("cz", "ㅊㅋ"),
        ("rt", "ㄱㅅ"),
        ("wt", "ㅈㅅ"),
        ("gd", "ㅎㅇ"),
        ("qd", "ㅂㅇ"),
        ("dw", "ㅇㅈ"),
        ("(dz!)", "(ㅇㅋ!)"),
        ("ee!", "ㄷㄷ!"),
    ] {
        let decision = d.decide(raw);
        t.eq(expected, &decision.text, format!("standalone Korean chat {raw}"));
        t.eq(
            raw,
            &decision.alternative,
            format!("chat keeps the literal alternative {raw}"),
        );
    }
    for literal in [
        "e",
        "d",
        "r",
        "rts",
        "dzdz",
        "dzez",
        "EE",
        "DZ",
        "ee.com",
        "dz@example.com",
        "user_ee",
        "helloee",
        "hellodz",
    ] {
        t.eq(
            literal,
            text(&d, literal),
            format!("chat recognition does not spread to unrelated text {literal}"),
        );
    }
    t.eq(
        "ㄷㄷ",
        text_with(&d, "EE", "ee", "", None),
        "Caps Lock still permits unshifted Korean chat",
    );
    for literal in ["dd", "tr", "fd"] {
        t.eq(
            literal,
            text(&d, literal),
            format!("CLI command wins a chat collision {literal}"),
        );
        t.truth(
            preferences.remember_global(literal, literal, true),
            format!("explicit chat choice is learnable {literal}"),
        );
        t.eq(
            hangul::compose(literal),
            text(&d, literal),
            format!("explicit choice overrides CLI protection {literal}"),
        );
    }
    preferences.clear();
    let mut input = Composition::new(d.clone());
    type_text(&mut input, "ee");
    input.toggle();
    input.accept_commit(true);
    t.eq("ee", text(&d, "ee"), "confirmed English choice overrides automatic chat");
    t.truth(preferences.undo_last(), "chat preference can be undone");
    t.eq("ㄷㄷ", text(&d, "ee"), "undo restores chat default");
    input.clear();
    type_text(&mut input, "dzz");
    t.eq("dzz", input.current().text, "unlisted abbreviation extension remains literal");
    input.backspace();
    t.eq("ㅇㅋ", input.current().text, "backspace recomputes the complete chat token");
    input.backspace();
    t.eq("d", input.current().text, "single key is not a standalone chat word");
    input.clear();
    type_text(&mut input, "gh");
    input.toggle();
    input.accept_commit(true);
    t.eq("호", text(&d, "gh"), "confirmed Korean choice overrides developer protection");
    t.finish("developer-and-chat");
}

fn corpus(name: &str) -> Vec<Vec<String>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/corpora").join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect()
}

#[test]
fn scorer_suite() {
    let mut t = T::default();
    let d = detector();
    let scorer = d.scorer();
    t.truth(
        scorer.bigram_count() > 10000,
        format!("Korean language model loaded ({} bigrams)", scorer.bigram_count()),
    );
    t.truth(
        d.korean_word_count() > 10000 && d.english_word_count() > 10000,
        "word lists loaded",
    );
    t.eq("야간", text(&d, "dirks"), "listed Korean word inside the band converts");
    t.eq(
        "Pattern",
        d.decide("dirks").confidence.as_str(),
        "word-list evidence is still a pattern estimate",
    );
    t.eq(
        "gown",
        text(&d, "gown"),
        "listed English word inside the band stays literal even though 해주 is also a listed Korean word",
    );
    t.eq(
        "torch",
        text(&d, "torch"),
        "listed English word with no Korean reading stays literal",
    );
    t.truth(
        !d.decide("torch").needs_confirmation(),
        "listed English word with no Korean reading needs no confirmation",
    );
    t.truth(
        d.decide("rory").needs_confirmation() || text(&d, "rory") == "rory",
        "unlisted band token is never converted by the word list below the margin",
    );
    t.eq("그런", text(&d, "rmfjs"), "short Korean outside the hand lists converts");
    t.eq(
        "Pattern",
        d.decide("rmfjs").confidence.as_str(),
        "statistical evidence is a pattern estimate, not lexical certainty",
    );
    t.eq(
        "하는",
        text(&d, "gksms"),
        "common two-syllable word converts without a shortcut",
    );
    t.eq("더", text(&d, "ej"), "common one-syllable word converts without a shortcut");
    t.eq("rm", text(&d, "rm"), "near-tie stays literal without context");
    t.truth(d.decide("rm").needs_confirmation(), "near-tie asks for a choice");
    t.eq(
        "오",
        text_with(&d, "dh", "dh", "", Some(true)),
        "Korean context breaks a near-tie toward Korean",
    );
    t.eq(
        "dh",
        text_with(&d, "dh", "dh", "", Some(false)),
        "English context breaks a near-tie toward English",
    );
    t.eq("dh", text(&d, "dh"), "no context keeps a near-tie literal");
    t.eq(
        "hello",
        text_with(&d, "hello", "hello", "", Some(true)),
        "context never overrides clear English",
    );
    t.eq(
        "dori",
        text_with(&d, "dori", "dori", "", Some(true)),
        "context never overrides clear non-lexicon English",
    );
    t.eq(
        "figjam",
        text(&d, "figjam"),
        "English statistics protect words composing into three Hangul syllables",
    );
    t.truth(
        !d.decide("figjam").needs_confirmation(),
        "clear English statistics stay decisive",
    );
    t.eq(
        "workauto",
        text(&d, "workauto"),
        "uncertain statistics preserve complete-syllable English input",
    );
    t.truth(
        !d.decide("workauto").is_korean,
        "complete-syllable English input is never converted automatically",
    );
    t.eq(
        "안녕하세요",
        text_with(&d, "dkssudgktpdy", "dkssudgktpdy", "", Some(false)),
        "context never overrides clear Korean",
    );
    t.eq(
        "go",
        text_with(&d, "go", "go", "", Some(true)),
        "known English word stays English even after a Korean word",
    );
    t.eq(
        "그런ㅋㅋ",
        text(&d, "rmfjszz"),
        "statistically Korean body accepts a chat suffix",
    );
    t.eq(
        "EHDWKRDKSGKA",
        text(&d, "EHDWKRDKSGKA"),
        "Shift acronym policy survives the statistical fallback",
    );
    t.eq(
        "동작안함",
        text_with(&d, "EHDWKRDKSGKA", "ehdwkrdksgka", "", None),
        "Caps Lock Korean still converts through the statistical fallback",
    );
    let mut input = Composition::new(d.clone());
    type_text(&mut input, "sksms");
    input.accept_commit(true);
    type_text(&mut input, "dh");
    t.eq("오", input.current().text, "composition carries the previous word's language");
    t.truth(
        input.previous_korean() == Some(true),
        "Korean continuity recorded after Space",
    );
    input.accept_commit(true);
    type_text(&mut input, "dh");
    t.eq("오", input.current().text, "Korean continuity persists across Korean words");
    input.clear();
    type_text(&mut input, "dh");
    t.eq("dh", input.current().text, "cancel clears continuity");
    input.clear();
    type_text(&mut input, "hello");
    input.accept_commit(true);
    type_text(&mut input, "dh");
    t.eq("dh", input.current().text, "an English word sets English continuity");
    t.truth(
        input.previous_korean() == Some(false),
        "English continuity recorded after Space",
    );
    input.clear();
    type_text(&mut input, "sksms");
    input.accept_commit(false);
    type_text(&mut input, "dh");
    t.eq("dh", input.current().text, "Enter clears continuity");
    t.truth(input.previous_korean().is_none(), "no continuity after Enter");
    input.clear();
    type_text(&mut input, "123");
    input.accept_commit(true);
    t.truth(input.previous_korean().is_none(), "symbols carry no language");
    // Fixtures generated by the Python reference implementation (tests/lm/detector.py).
    let fixtures = corpus("scorer-fixtures.tsv");
    for f in &fixtures {
        let previous = match f[2].as_str() {
            "K" => Some(true),
            "E" => Some(false),
            _ => None,
        };
        let v = scorer.score(&f[0], &f[1], previous);
        let kind = if v.is_korean {
            "K"
        } else if v.is_english {
            "E"
        } else {
            "U"
        };
        t.eq(
            &f[3],
            if v.is_korean { v.composed.as_str() } else { f[0].as_str() },
            format!("reference text {}/{}", f[0], f[2]),
        );
        t.eq(&f[4], kind, format!("reference verdict {}/{}", f[0], f[2]));
        let expected: f64 = f[5].parse().unwrap();
        t.truth(
            (v.llr - expected).abs() < 0.05,
            format!("reference llr {}/{} expected {} actual {:.2}", f[0], f[2], f[5], v.llr),
        );
    }
    t.truth(
        fixtures.len() >= 800,
        format!("reference fixture rows loaded: {}", fixtures.len()),
    );
    // Held-out gates: Korean words absent from the hand lists, English tokens from the same corpus.
    let (mut korean_ok, mut korean_wrong) = (0, 0);
    let korean = corpus("heldout-korean-1000.tsv");
    for f in &korean {
        let d = d.decide(&f[0]);
        if d.is_korean && d.text == f[1] {
            korean_ok += 1;
        } else if d.is_korean {
            korean_wrong += 1;
        }
    }
    t.truth(korean.len() >= 900, "held-out Korean rows loaded");
    t.truth(
        korean_ok * 100 >= korean.len() * 96,
        format!("held-out Korean conversion rate {korean_ok}/{}", korean.len()),
    );
    t.truth(
        korean_wrong * 200 <= korean.len(),
        format!("held-out Korean wrong conversions {korean_wrong}/{}", korean.len()),
    );
    let (mut english_ok, mut english_wrong) = (0, 0);
    let english = corpus("heldout-english-1000.tsv");
    for f in &english {
        let d = d.decide(&f[0]);
        if !d.is_korean && !d.needs_confirmation() {
            english_ok += 1;
        } else if d.is_korean {
            english_wrong += 1;
        }
    }
    t.truth(english.len() >= 900, "held-out English rows loaded");
    t.truth(
        english_ok * 100 >= english.len() * 97,
        format!("held-out English retention rate {english_ok}/{}", english.len()),
    );
    t.truth(
        english_wrong * 200 <= english.len(),
        format!("held-out English wrong conversions {english_wrong}/{}", english.len()),
    );
    println!(
        "scorer gates: korean {korean_ok}/{} (wrong {korean_wrong}), english {english_ok}/{} (wrong {english_wrong}), fixtures {}",
        korean.len(),
        english.len(),
        fixtures.len()
    );
    t.finish("scorer");
}

#[test]
fn flip_suite() {
    let mut t = T::default();
    for (input, expected, label) in [
        ("dkssud", "안녕", "flip Latin run to Hangul"),
        ("안녕", "dkssud", "flip Hangul run to keys"),
        ("dkssud hello!", "안녕 ㅗ디ㅣㅐ!", "flip keeps spaces and punctuation"),
        ("Rhk 3 Enlek", "꽈 3 뛰다", "shifted letters compose doubled jamo"),
        ("뛰다", "Enlek", "doubled jamo decompose to shifted keys"),
        ("한글 2026", "gksrmf 2026", "digits stay"),
        ("", "", "empty flip"),
        ("가\n나", "rk\nsk", "line breaks stay"),
    ] {
        t.eq(expected, hangul::flip(input), label);
    }
    t.eq(
        "dkssud hello!",
        hangul::flip(&hangul::flip("dkssud hello!")),
        "flip is its own inverse for letters",
    );
    // Sensitivity presets move only the abstention band; clear evidence is unaffected.
    let d = detector();
    let scorer = d.scorer();
    let (name, ko, en) = preset_thresholds(Some("conservative"));
    t.eq("conservative", name, "conservative preset name");
    scorer.configure(ko, en);
    t.truth(d.decide("gksms").is_korean, "conservative still converts clear Korean");
    t.truth(
        !d.decide("tp").is_korean && d.decide("tp").needs_confirmation(),
        "conservative abstains below its threshold",
    );
    t.truth(
        !d.decide("hello").is_korean && !d.decide("hello").needs_confirmation(),
        "conservative keeps clear English certain",
    );
    let (name, ko, en) = preset_thresholds(Some("aggressive"));
    t.eq("aggressive", name, "aggressive preset name");
    scorer.configure(ko, en);
    t.eq("지", text(&d, "wl"), "aggressive converts a band word");
    let (name, ko, en) = preset_thresholds(Some("whatever"));
    t.eq("balanced", name, "unknown preset falls back to balanced");
    scorer.configure(ko, en);
    t.eq("wl", text(&d, "wl"), "balanced keeps the band word literal");
    t.truth(d.decide("wl").needs_confirmation(), "balanced abstention is visible");
    t.truth(
        scorer.korean_threshold() == DEFAULT_KOREAN_THRESHOLD && scorer.english_threshold() == DEFAULT_ENGLISH_THRESHOLD,
        "balanced equals the defaults",
    );
    t.finish("flip");
}

#[test]
fn focus_policy() {
    let mut t = T::default();
    t.truth(can_capture(true, false, true, false, true, 100, true), "safe target");
    t.truth(!can_capture(true, true, true, false, true, 100, true), "password bypass");
    t.truth(!can_capture(true, false, false, false, true, 100, true), "read only bypass");
    t.truth(!can_capture(false, false, true, false, true, 100, true), "unknown bypass");
    t.truth(!can_capture(true, false, true, true, true, 100, true), "elevated bypass");
    t.truth(!can_capture(true, false, true, false, true, 900, true), "stale bypass");
    t.truth(
        !can_capture(true, false, true, false, true, 100, false),
        "changed target bypass",
    );
    t.eq(
        "Explicit",
        Confidence::Explicit.as_str(),
        "confidence names match the C# enum",
    );
    t.finish("focus");
}
