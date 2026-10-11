// Gate G1: replays the golden suite extracted from the C# engine (tests/dotnet/Hanautomata.Golden) and requires every
// output to match. Full suite: core/target/golden (regenerate with core/golden.sh). Without it, the committed lite sample
// in tests/golden-lite runs instead. HANAUTOMATA_GOLDEN=<dir> selects another directory.

use flate2::read::GzDecoder;
use hanautomata_core::{hangul, preset_thresholds, Composition, Decision, Detector, LearnedWord, WordPreferences};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Arc;

const PRESETS: [&str; 3] = ["balanced", "conservative", "aggressive"];
const PREVS: [Option<bool>; 3] = [None, Some(true), Some(false)];

fn golden_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HANAUTOMATA_GOLDEN") {
        return dir.into();
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join("target/golden");
    if full.join("manifest.json").exists() {
        full
    } else {
        root.join("tests/golden-lite")
    }
}

fn manifest() -> Value {
    let path = golden_dir().join("manifest.json");
    serde_json::from_reader(File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

fn rows(name: &str) -> Vec<Value> {
    let path = golden_dir().join(format!("{name}.jsonl.gz"));
    let file = File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    BufReader::new(GzDecoder::new(file))
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect()
}

struct Report {
    name: &'static str,
    checked: usize,
    failures: Vec<String>,
}

impl Report {
    fn new(name: &'static str) -> Self {
        Report {
            name,
            checked: 0,
            failures: Vec::new(),
        }
    }
    fn check(&mut self, expected: &Value, actual: &Value, label: impl FnOnce() -> String) {
        self.checked += 1;
        if expected != actual {
            self.failures
                .push(format!("{}: expected {expected} actual {actual}", label()));
        }
    }
    fn finish(self) {
        let dir = golden_dir();
        println!(
            "{} · {} checks · {} mismatches · {}",
            self.name,
            self.checked,
            self.failures.len(),
            dir.display()
        );
        assert!(self.checked > 0, "{}: no golden rows", self.name);
        if !self.failures.is_empty() {
            let shown: Vec<&String> = self.failures.iter().take(40).collect();
            panic!(
                "{}: {} of {} checks differ from the C# engine:\n{:#?}",
                self.name,
                self.failures.len(),
                self.checked,
                shown
            );
        }
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}
fn opt(v: &Value) -> Option<bool> {
    match s(v) {
        "T" => Some(true),
        "F" => Some(false),
        _ => None,
    }
}
fn bits(x: f64) -> Value {
    json!(x.to_bits() as i64)
}
fn decision(d: &Decision) -> Value {
    json!([d.text, d.alternative, d.is_korean, d.reason, d.confidence.as_str()])
}
fn configure(detector: &Detector, preset: &str) {
    let (_, korean, english) = preset_thresholds(Some(preset));
    detector.scorer().configure(korean, english);
}

#[test]
fn hangul() {
    let mut report = Report::new("hangul");
    for row in rows("hangul") {
        let (f, input) = (s(&row["f"]), s(&row["i"]));
        let actual = match f {
            "compose" => hangul::compose(input),
            "tokeys" => hangul::to_keys(input),
            "flip" => hangul::flip(input),
            other => panic!("unknown function {other}"),
        };
        report.check(&row["o"], &json!(actual), || format!("{f}({input:?})"));
    }
    report.finish();
}

#[test]
fn lists() {
    let mut report = Report::new("lists");
    let detector = Detector::new("");
    let ours: HashMap<&str, Vec<String>> = detector.vocabularies_for_test().into_iter().collect();
    for row in rows("lists") {
        let name = s(&row["name"]);
        report.check(&row["items"], &json!(ours[name]), || format!("list {name}"));
    }
    let m = manifest();
    report.check(&m["bigrams"], &json!(detector.scorer().bigram_count()), || {
        "bigram count".into()
    });
    report.check(&m["koreanWords"], &json!(detector.korean_word_count()), || {
        "Korean word list".into()
    });
    report.check(&m["englishWords"], &json!(detector.english_word_count()), || {
        "English word list".into()
    });
    report.finish();
}

#[test]
fn scorer() {
    let mut report = Report::new("scorer");
    let detector = Detector::new(s(&manifest()["profile"]));
    let scorer = detector.scorer();
    for row in rows("scorer") {
        let (raw, keys) = (s(&row["raw"]), s(&row["keys"]));
        let (korean, composed, stray) = scorer.korean(keys);
        let actual = json!({ "ko": bits(korean), "stray": stray, "composed": composed, "en": bits(scorer.english(raw)) });
        let expected = json!({ "ko": row["ko"], "stray": row["stray"], "composed": row["composed"], "en": row["en"] });
        report.check(&expected, &actual, || format!("models({raw:?}, {keys:?})"));
        let mut codes = String::new();
        let mut first = None;
        for preset in PRESETS {
            configure(&detector, preset);
            for prev in PREVS {
                let v = scorer.score(raw, keys, prev);
                let code = if v.is_korean {
                    'K'
                } else if v.is_english {
                    'E'
                } else {
                    'U'
                };
                codes.push(if v.nudged { code.to_ascii_lowercase() } else { code });
                first.get_or_insert((v.llr, v.lexical));
            }
        }
        let (llr, lexical) = first.unwrap();
        let actual = json!({ "llr": bits(llr), "lex": lexical, "v": codes });
        let expected = json!({ "llr": row["llr"], "lex": row["lex"], "v": row["v"] });
        report.check(&expected, &actual, || format!("score({raw:?}, {keys:?})"));
    }
    configure(&detector, "balanced");
    report.finish();
}

#[test]
fn decide() {
    let mut report = Report::new("decide");
    let profile = s(&manifest()["profile"]).to_string();
    let mut detectors: HashMap<String, Detector> = HashMap::new();
    for row in rows("decide") {
        let p = row.get("p").map(|v| s(v).to_string()).unwrap_or_else(|| profile.clone());
        let detector = detectors.entry(p.clone()).or_insert_with(|| Detector::new(&p));
        let (raw, keys) = (s(&row["raw"]), s(&row["keys"]));
        let results = row["r"].as_array().unwrap();
        let map = s(&row["m"]).as_bytes();
        let mut i = 0;
        for preset in PRESETS {
            configure(detector, preset);
            for prev in PREVS {
                let expected = &results[(map[i] - b'0') as usize];
                let actual = decision(&detector.decide_with(raw, keys, "", prev));
                report.check(expected, &actual, || {
                    format!("decide({raw:?}, {keys:?}, {preset}, {prev:?}) profile {p:?}")
                });
                i += 1;
            }
        }
    }
    report.finish();
}

/// Sensitivity presets on the 4,000 + 4,000 held-out samples. The samples are regenerated by tests/lm/build_lm.py and not
/// committed, so this runs only where the extractor saw them (the manifest then carries the C# counts).
#[test]
fn sensitivity() {
    let m = manifest();
    let Some(expected) = m.get("sensitivity") else {
        println!("sensitivity · skipped: golden extracted without tests/lm/out-* samples");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/lm");
    let read = |name: &str| -> Option<Vec<Vec<String>>> {
        let text = std::fs::read_to_string(root.join(name)).ok()?;
        Some(
            text.lines()
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| l.split('\t').map(str::to_string).collect())
                .collect(),
        )
    };
    let (Some(korean), Some(english)) = (read("out-ko/heldout-korean.txt"), read("out-en/heldout-english.txt")) else {
        println!("sensitivity · skipped: tests/lm/out-* samples absent on this machine");
        return;
    };
    let mut report = Report::new("sensitivity");
    let detector = Detector::new(s(&m["profile"]));
    let mut summary = Vec::new();
    for preset in PRESETS {
        configure(&detector, preset);
        let (mut korean_ok, mut korean_wrong, mut english_ok, mut english_wrong) = (0, 0, 0, 0);
        for f in &korean {
            let d = detector.decide(&f[0]);
            if d.is_korean && d.text == f[1] {
                korean_ok += 1;
            } else if d.is_korean {
                korean_wrong += 1;
            }
        }
        for f in &english {
            let d = detector.decide(&f[0]);
            if !d.is_korean && !d.needs_confirmation() {
                english_ok += 1;
            } else if d.is_korean {
                english_wrong += 1;
            }
        }
        let actual =
            json!({ "koreanOk": korean_ok, "koreanWrong": korean_wrong, "englishOk": english_ok, "englishWrong": english_wrong });
        summary.push(format!("{preset} {actual}"));
        report.check(&expected[preset], &actual, || format!("preset {preset}"));
    }
    println!(
        "sensitivity · {} Korean + {} English · {}",
        korean.len(),
        english.len(),
        summary.join(" · ")
    );
    configure(&detector, "balanced");
    report.finish();
}

/// One scenario's objects. The composition is created on first use so that `lex` ops (which need the detector exclusively)
/// can run first, as the generator emits them.
struct Replay {
    prefs: Arc<WordPreferences>,
    detector: Option<Detector>,
    shared: Option<Arc<Detector>>,
    composition: Option<Composition>,
}

impl Replay {
    fn new(profile: &str) -> Self {
        let prefs = Arc::new(WordPreferences::new());
        let detector = Detector::with_preferences(profile, prefs.clone());
        Replay {
            prefs,
            detector: Some(detector),
            shared: None,
            composition: None,
        }
    }
    fn detector(&self) -> &Detector {
        self.shared.as_deref().or(self.detector.as_ref()).unwrap()
    }
    fn arc(&mut self) -> Arc<Detector> {
        if self.shared.is_none() {
            self.shared = Some(Arc::new(self.detector.take().unwrap()));
        }
        self.shared.clone().unwrap()
    }
    fn composition(&mut self) -> &mut Composition {
        if self.composition.is_none() {
            let detector = self.arc();
            self.composition = Some(Composition::new(detector));
        }
        self.composition.as_mut().unwrap()
    }
    fn state(&self) -> [Value; 3] {
        [
            json!(self.prefs.count()),
            json!(self.prefs.version()),
            json!(self.prefs.can_undo()),
        ]
    }
}

fn entry_in(v: &Value) -> LearnedWord {
    let text = |x: &Value| x.as_str().unwrap_or("").to_string();
    LearnedWord {
        raw: text(&v[0]),
        keys: text(&v[1]),
        korean: v[2].as_bool().unwrap(),
        sequence: v[3].as_i64().unwrap(),
        context: text(&v[4]),
        korean_weight: f64::from_bits(v[5].as_i64().unwrap() as u64),
        english_weight: f64::from_bits(v[6].as_i64().unwrap() as u64),
    }
}

fn entry_out(e: &LearnedWord) -> Value {
    json!([
        e.raw,
        e.keys,
        e.korean,
        e.sequence,
        e.context,
        bits(e.korean_weight),
        bits(e.english_weight)
    ])
}

fn char_of(v: &Value) -> char {
    let text = s(v);
    let mut chars = text.chars();
    let c = chars.next().unwrap();
    assert!(chars.next().is_none(), "one char expected: {text:?}");
    c
}

fn step(r: &mut Replay, op: &str, a: &[Value], entries: Option<&Vec<Value>>) -> Value {
    let flag = |i: usize| s(&a[i]) == "1";
    match op {
        "rem" => {
            let ok = r.prefs.remember(s(&a[0]), s(&a[1]), flag(2), s(&a[3]));
            let [c, v, u] = r.state();
            json!([ok, c, v, u])
        }
        "get" => {
            let l = r.prefs.try_get(s(&a[0]), s(&a[1]), s(&a[2]));
            json!([l.found, l.korean, l.context_match, l.conflict])
        }
        "get2" => {
            let found = r.prefs.try_get_global(s(&a[0]), s(&a[1]));
            json!([found.is_some(), found.unwrap_or(false)])
        }
        "undo" => {
            let ok = r.prefs.undo_last();
            let [c, v, u] = r.state();
            json!([ok, c, v, u])
        }
        "pclear" => {
            r.prefs.clear();
            json!(r.state())
        }
        "enable" => {
            r.prefs.set_enabled(flag(0));
            let [c, v, u] = r.state();
            json!([r.prefs.enabled(), c, v, u])
        }
        "export" => {
            let (entries, revision) = r.prefs.export();
            json!({ "rev": revision, "entries": entries.iter().map(entry_out).collect::<Vec<_>>() })
        }
        "restore" => {
            let entries: Vec<LearnedWord> = entries.unwrap().iter().map(entry_in).collect();
            r.prefs.restore(&entries);
            json!(r.state())
        }
        "ctxkey" => json!(hanautomata_core::context_key(s(&a[0]))),
        "dec" => decision(&r.detector().decide_with(s(&a[0]), s(&a[1]), s(&a[2]), opt(&a[3]))),
        "cfg" => {
            let (name, korean, english) = preset_thresholds(Some(s(&a[0])));
            let scorer = r.detector().scorer();
            scorer.configure(korean, english);
            json!([name, bits(scorer.korean_threshold()), bits(scorer.english_threshold())])
        }
        "cfgv" => {
            // .NET's double.Parse("NaN") yields the negative quiet NaN; the engine passes the operand through unchanged.
            let parse = |i: usize| match s(&a[i]) {
                "NaN" => f64::from_bits(0xfff8_0000_0000_0000),
                text => text.parse::<f64>().unwrap(),
            };
            let scorer = r.detector().scorer();
            scorer.configure(parse(0), parse(1));
            json!([bits(scorer.korean_threshold()), bits(scorer.english_threshold())])
        }
        "type" => {
            let c = r.composition();
            for ch in s(&a[0]).chars() {
                c.append(ch, ch);
            }
            Value::Null
        }
        "app" => {
            let (raw, key) = (char_of(&a[0]), char_of(&a[1]));
            r.composition().append(raw, key);
            Value::Null
        }
        "semi" => {
            r.composition().semicolon();
            Value::Null
        }
        "bs" => {
            r.composition().backspace();
            Value::Null
        }
        "tog" => {
            r.composition().toggle();
            Value::Null
        }
        "conf" => json!(r.composition().confirm_selection()),
        "acc" => {
            r.composition().accept_commit(flag(0));
            Value::Null
        }
        "reset" => {
            let cont = flag(1);
            r.composition().reset_context(s(&a[0]), cont);
            Value::Null
        }
        "cclear" => {
            r.composition().clear();
            Value::Null
        }
        "explicit" => json!(r.composition().remember_explicit(s(&a[0]), s(&a[1]), flag(2), s(&a[3]))),
        "lex" => {
            assert!(r.shared.is_none(), "lex after the detector was shared");
            let cost: i32 = s(&a[2]).parse().unwrap();
            r.detector.as_mut().unwrap().add_lexeme_for_test(s(&a[0]), flag(1), cost);
            Value::Null
        }
        "newc" => {
            let detector = r.arc();
            r.composition = Some(Composition::new(detector));
            Value::Null
        }
        "st" => {
            let (count, version) = (r.prefs.count(), r.prefs.version());
            let c = r.composition();
            let current = c.current();
            let prev = match c.previous_korean() {
                None => "-",
                Some(true) => "T",
                Some(false) => "F",
            };
            let (raw, keys, context, empty) = (c.raw(), c.keys(), c.context().to_string(), c.is_empty());
            let korean = c.korean();
            let len = c.len();
            json!([
                current.text,
                current.alternative,
                current.is_korean,
                current.reason,
                current.confidence.as_str(),
                raw,
                keys,
                context,
                prev,
                empty,
                korean,
                len,
                count,
                version
            ])
        }
        other => panic!("unknown op {other}"),
    }
}

#[test]
fn scenarios() {
    let mut report = Report::new("scenarios");
    let profile = s(&manifest()["profile"]).to_string();
    for row in rows("scenarios") {
        let name = s(&row["name"]).to_string();
        let mut replay = Replay::new(&profile);
        for (i, st) in row["steps"].as_array().unwrap().iter().enumerate() {
            let op = s(&st["op"]);
            let args = st["a"].as_array().unwrap();
            let actual = step(&mut replay, op, args, st.get("e").and_then(Value::as_array));
            report.check(&st["x"], &actual, || format!("{name} step {i} {op} {args:?}"));
        }
    }
    report.finish();
}
