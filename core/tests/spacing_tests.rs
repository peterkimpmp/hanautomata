use flate2::read::GzDecoder;
use hanautomata_core::spacing::correct;
use std::io::Read;

#[test]
fn shared_windows_examples_and_counterexamples() {
    let mut count = 0;
    for row in include_str!("../../tests/corpora/spacing.tsv")
        .lines()
        .filter(|row| !row.starts_with('#'))
    {
        let mut columns = row.splitn(3, '\t');
        let input = columns.next().unwrap();
        let expected = columns.next().expect("input and expected columns; optional note");
        assert_eq!(correct(input).text, expected, "{input:?}");
        assert_eq!(correct(expected).text, expected, "idempotent: {input:?}");
        assert_eq!(expected.replace(' ', ""), input.replace(' ', ""), "characters preserved");
        count += 1;
    }
    assert_eq!(count, 133, "all shared spacing cases must be loaded");
}

#[test]
fn wikipedia_and_heldout_precision_gate() {
    let mut words = String::new();
    GzDecoder::new(&include_bytes!("../../data/korean-words.txt.gz")[..])
        .read_to_string(&mut words)
        .unwrap();
    let mut count = 0;
    let mut changed = 0;
    for word in words.lines().filter(|word| !word.is_empty() && !word.starts_with('#')) {
        let expected = match word {
            "해야한다" => "해야 한다",
            "수있다" => "수 있다",
            "수있는" => "수 있는",
            "살때" => "살 때",
            "볼때" => "볼 때",
            "이때문에" => "이 때문에",
            _ => word,
        };
        let actual = correct(word);
        assert_eq!(actual.text, expected, "Wikipedia: {word}");
        assert_eq!(actual.text.replace(' ', ""), word, "only insert spaces");
        if actual.text != word {
            changed += 1;
        }
        count += 1;
    }
    assert_eq!(count, 61_759);
    assert_eq!(changed, 6);
    let mut heldout = 0;
    for row in include_str!("../../tests/corpora/heldout-korean-1000.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
    {
        let word = row.split('\t').nth(1).unwrap();
        assert_eq!(correct(word).text, word, "heldout: {word}");
        heldout += 1;
    }
    assert_eq!(heldout, 1_000);
}

#[test]
fn unicode_bound_and_rule_provenance() {
    let r = correct("할수있다 하는것");
    assert_eq!(r.text, "할 수 있다 하는 것");
    assert_eq!(r.rules, ["ko-space-su", "ko-space-su", "ko-space-geot"]);
    let exact = format!("{}할수있다", "😀".repeat(126)); // 256 UTF-16 units, as in C#.
    assert_eq!(correct(&exact).text, format!("{}할 수 있다", "😀".repeat(126)));
    let over = format!("x{exact}");
    assert_eq!(correct(&over).text, over);
    for input in ["", "😀", "한글\tEnglish\n할 수 있다", "할\u{200b}수있다", "할\u{00a0}수있다"] {
        assert_eq!(correct(&correct(input).text).text, correct(input).text);
    }
}
