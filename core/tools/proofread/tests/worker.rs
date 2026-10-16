use std::{io::{BufRead, BufReader, Write}, process::{Command, Stdio}};
use serde_json::{json, Value};

fn start() -> (std::process::Child, BufReader<std::process::ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hanautomata-proofread"))
        .args(["--dictionary", "/missing-hanautomata-dictionary", "--serve"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let output = BufReader::new(child.stdout.take().unwrap());
    (child, output)
}
fn read(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0, "worker exited before response");
    serde_json::from_str(&line).unwrap()
}
fn check(id: &str) -> Value {
    json!({"schema_version":1,"command":"check","request":{
        "request_id":id,"document_id":"doc","revision":"1","text":"비밀 입력"}})
}

#[test]
fn supervisor_recovers_after_missing_data_and_never_echoes_text() {
    let (mut child, mut output) = start();
    assert_eq!(read(&mut output)["event"], "ready");
    for id in ["one", "two"] {
        writeln!(child.stdin.as_mut().unwrap(), "{}", check(id)).unwrap();
        let reply = read(&mut output);
        assert_eq!(reply["event"], "error");
        assert_eq!(reply["request_id"], id);
        assert!(!reply.to_string().contains("비밀"));
    }
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn malformed_frame_is_bounded_and_does_not_disable_next_request() {
    let (mut child, mut output) = start();
    read(&mut output);
    writeln!(child.stdin.as_mut().unwrap(), "{{\"private\":\"do-not-echo\"}}").unwrap();
    let reply = read(&mut output);
    assert_eq!(reply["code"], "invalid_command");
    assert!(!reply.to_string().contains("do-not-echo"));
    writeln!(child.stdin.as_mut().unwrap(), "{}", check("next")).unwrap();
    assert_eq!(read(&mut output)["request_id"], "next");
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn oversized_frame_closes_the_session_without_echoing_or_allocating_the_rest() {
    let (mut child, mut output) = start();
    read(&mut output);
    // The supervisor rejects at the byte limit even without a line terminator.
    let _ = child.stdin.as_mut().unwrap().write_all(&vec![b'x'; 65_537]);
    let reply = read(&mut output);
    assert_eq!(reply["code"], "frame_too_large");
    assert!(reply.to_string().len() < 160);
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn cancel_has_request_identity_and_shutdown_reaps_worker() {
    let (mut child, mut output) = start();
    read(&mut output);
    writeln!(child.stdin.as_mut().unwrap(), "{}\n{}", check("cancel-me"),
        json!({"schema_version":1,"command":"cancel","request_id":"cancel-me"})).unwrap();
    let reply = read(&mut output);
    assert_eq!(reply["request_id"], "cancel-me");
    // A native load failure may win the race; cancellation must never produce a result.
    assert!(reply["event"] == "cancelled" || reply["event"] == "error");
    writeln!(child.stdin.as_mut().unwrap(), "{}", json!({"schema_version":1,"command":"shutdown"})).unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
#[ignore = "requires HANAUTOMATA_KO_DATA pinned dictionary"]
fn native_supervisor_cancels_supersedes_recovers_and_rejects_late_results() {
    let data=std::env::var("HANAUTOMATA_KO_DATA").unwrap();
    let mut child=Command::new(env!("CARGO_BIN_EXE_hanautomata-proofread"))
        .args(["--dictionary", &data, "--serve", "--rules"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut output=BufReader::new(child.stdout.take().unwrap()); read(&mut output);
    writeln!(child.stdin.as_mut().unwrap(), "{}\n{}",check("old"),check("new")).unwrap();
    let old=read(&mut output); assert_eq!(old["event"],"cancelled"); assert_eq!(old["request_id"],"old");
    let new=read(&mut output); assert_eq!(new["event"],"result"); assert_eq!(new["request_id"],"new");
    writeln!(child.stdin.as_mut().unwrap(), "{}\n{}",check("cancel"),json!({"schema_version":1,"command":"cancel","request_id":"cancel"})).unwrap();
    let cancelled=read(&mut output); assert_eq!(cancelled["event"],"cancelled");
    writeln!(child.stdin.as_mut().unwrap(),"{}",check("after")).unwrap();
    assert_eq!(read(&mut output)["event"],"result");
    drop(child.stdin.take()); assert!(child.wait().unwrap().success());
}

#[test]
#[ignore = "requires HANAUTOMATA_KO_DATA pinned dictionary"]
fn native_deadline_kills_cold_worker_and_keeps_supervisor_available() {
    let data=std::env::var("HANAUTOMATA_KO_DATA").unwrap();
    let mut child=Command::new(env!("CARGO_BIN_EXE_hanautomata-proofread"))
        .args(["--dictionary", &data,"--serve","--timeout-ms","1"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut output=BufReader::new(child.stdout.take().unwrap()); read(&mut output);
    let start=std::time::Instant::now();
    for id in ["deadline-one","deadline-two"] {
        writeln!(child.stdin.as_mut().unwrap(),"{}",check(id)).unwrap();
        let reply=read(&mut output); assert_eq!(reply["code"],"timeout"); assert_eq!(reply["request_id"],id);
    }
    assert!(start.elapsed()<std::time::Duration::from_secs(2));
    drop(child.stdin.take()); assert!(child.wait().unwrap().success());
}
