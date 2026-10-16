use std::{
    io::Write,
    process::{Command, Stdio},
};

fn run(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hanautomata-proofread"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn invalid_json_does_not_echo_text_or_return_success() {
    let out = run(&["--dictionary", ".", "--json"], br#"{"private_input":"DO_NOT_ECHO"}"#);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("invalid_request_json"));
    assert!(!stderr.contains("DO_NOT_ECHO"));
}

#[test]
fn missing_data_is_an_error_not_a_clean_report() {
    let out = run(&["--dictionary", "."], "됬다".as_bytes());
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8(out.stderr).unwrap().contains("dictionary_file_missing"));
}

#[test]
fn input_must_be_utf8_and_bounded() {
    for input in [vec![0xff], vec![b'a'; 65_537]] {
        let out = run(&["--dictionary", "."], &input);
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn help_does_not_load_data() {
    let out = run(&["--help"], b"");
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .contains("Usage: hanautomata-proofread"));
}

#[cfg(unix)]
#[test]
fn non_utf8_option_is_a_structured_error_not_a_panic() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let out = Command::new(env!("CARGO_BIN_EXE_hanautomata-proofread"))
        .arg(OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(value["error"], "invalid_arguments");
}
