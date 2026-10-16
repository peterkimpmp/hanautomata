use hanautomata_proofread::{Checker, HunspellLexicon, Request};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_normalization::UnicodeNormalization;

fn temporary() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("hanautomata-proofread-{}-{n}-{sequence}", std::process::id()));
    fs::create_dir(&p).unwrap();
    p
}

#[test]
fn missing_dictionary_fails_before_entering_native_engine() {
    let p = temporary();
    assert_eq!(HunspellLexicon::load_pinned(&p).err().unwrap().0, "dictionary_file_missing");
    fs::remove_dir(p).unwrap();
}

#[test]
fn modified_dictionary_is_rejected_instead_of_treated_as_empty() {
    let p = temporary();
    fs::write(p.join("ko.aff"), b"SET UTF-8\n").unwrap();
    assert_eq!(
        HunspellLexicon::load_pinned(&p).err().unwrap().0,
        "dictionary_integrity_failed"
    );
    fs::remove_dir_all(p).unwrap();
}

#[test]
#[ignore = "requires prepare_dictionary.py and HANAUTOMATA_KO_DATA"]
fn pinned_korean_dictionary_smoke() {
    let p = PathBuf::from(std::env::var_os("HANAUTOMATA_KO_DATA").expect("set dictionary path"));
    let mut checker = Checker::new(HunspellLexicon::load_pinned(&p).unwrap());
    for (word, candidate) in [
        ("안녕하세요", None),
        ("돼요", None),
        ("됐다", None),
        ("학교에", None),
        ("먹었습니다", None),
        ("됬다", Some("됐다")),
        ("웬지", Some("왠지")),
        ("개발됬다", Some("개발됐다")),
    ] {
        for text in [word.to_string(), word.nfd().collect()] {
            let r = checker
                .check(&Request {
                    request_id: "smoke".into(),
                    document_id: "synthetic".into(),
                    revision: "1".into(),
                    text,
                    protected_words: vec![],
                })
                .unwrap();
            assert_eq!(r.checked_words, 1);
            assert!(!r.limited);
            if let Some(expected) = candidate {
                assert_eq!(r.diagnostics.len(), 1, "{word}");
                assert!(
                    r.diagnostics[0].suggestions.iter().any(|s| s == expected),
                    "{word}: {:?}",
                    r.diagnostics[0].suggestions
                );
            } else {
                assert!(r.diagnostics.is_empty(), "{word}");
            }
        }
    }
}

#[test]
#[ignore = "requires HANAUTOMATA_KO_DATA pinned dictionary"]
fn unicode_dictionary_path_supports_cli_and_supervisor() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, Command, Stdio};
    use serde_json::{json, Value};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }
    struct Running(Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn read(reader: &mut impl BufRead) -> Value {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "checker exited before response");
        serde_json::from_str(&line).unwrap()
    }
    fn assert_suggestion(report: &Value) {
        assert_eq!(report["checked_words"], 1);
        assert!(report["diagnostics"].as_array().unwrap().iter().any(|d|
            d["rule_id"] == "ko-doe-yo" && d["suggestions"][0] == "돼요"), "{report}");
    }

    let source = PathBuf::from(std::env::var_os("HANAUTOMATA_KO_DATA").expect("set dictionary path"));
    let fixture = Fixture(temporary());
    let data = fixture.0.join("한글 사전 📝");
    fs::create_dir(&data).unwrap();
    for name in ["ko.aff", "ko.dic", "LICENSE.md", "LICENSE.GPL-3"] {
        fs::copy(source.join(name), data.join(name)).unwrap();
    }
    let binary = env!("CARGO_BIN_EXE_hanautomata-proofread");
    let mut once = Command::new(binary).arg("--dictionary").arg(&data).arg("--rules")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    once.stdin.take().unwrap().write_all("되요".as_bytes()).unwrap();
    let output = once.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_suggestion(&serde_json::from_slice(&output.stdout).unwrap());

    // A relative Unicode path also has to survive supervisor child creation.
    let mut running = Running(Command::new(binary).current_dir(&fixture.0)
        .arg("--dictionary").arg(data.file_name().unwrap()).args(["--serve", "--rules"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
    let mut reader = BufReader::new(running.0.stdout.take().unwrap());
    assert_eq!(read(&mut reader)["event"], "ready");
    writeln!(running.0.stdin.as_mut().unwrap(), "{}", json!({"schema_version":1,"command":"check",
        "request":{"request_id":"unicode","document_id":"fixture","revision":"1","text":"되요"}})).unwrap();
    let reply = read(&mut reader);
    assert_eq!(reply["event"], "result", "{reply}");
    assert_eq!(reply["request_id"], "unicode");
    assert_suggestion(&reply["report"]);
    writeln!(running.0.stdin.as_mut().unwrap(), "{}", json!({"schema_version":1,"command":"shutdown"})).unwrap();
    assert!(running.0.wait().unwrap().success());
    drop(running);

    // Unicode support must retain the pinned-data integrity gate.
    fs::write(data.join("ko.aff"), b"SET UTF-8\n").unwrap();
    let output = Command::new(binary).arg("--dictionary").arg(&data)
        .stdin(Stdio::null()).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("dictionary_integrity_failed"));
}
