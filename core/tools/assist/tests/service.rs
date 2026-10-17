use hanautomata_assist::{store::Store, Request};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "hanautomata-assist-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> PathBuf {
        self.0.join("personal-expressions.json")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request(command: &str, fields: Value) -> Request {
    let mut r = json!({"schema_version":1,"request_id":"test-1","command":command});
    for (k, v) in fields.as_object().unwrap() {
        r[k] = v.clone();
    }
    serde_json::from_value(r).unwrap()
}

#[test]
fn persists_and_clear_never_resurrects_on_restart() {
    let temp = Temp::new();
    let mut s = Store::open(&temp.file()).unwrap();
    assert!(s.handle(request("list", json!({})), 1).items.is_empty());
    assert!(!temp.file().exists()); // Queries and initial opening don't create an empty data file.
    assert!(s.handle(request("add", json!({"text":"내 표현을 저장합니다."})), 1).ok);
    assert!(s.handle(request("set_enabled", json!({"enabled":true})), 1).ok);
    drop(s);
    let mut s = Store::open(&temp.file()).unwrap();
    let r = s.handle(request("query", json!({"text":"내 표현"})), 1);
    assert_eq!(r.items.len(), 1);
    assert!(r.enabled);
    assert!(s.handle(request("clear", json!({})), 1).ok);
    drop(s);
    let mut s = Store::open(&temp.file()).unwrap();
    assert!(s.handle(request("list", json!({})), 1).items.is_empty());
    assert!(!fs::read_to_string(temp.file()).unwrap().contains("내 표현"));
}

#[test]
fn exclusive_writer_and_os_release_after_close() {
    let temp = Temp::new();
    let s = Store::open(&temp.file()).unwrap();
    assert!(Store::open(&temp.file()).is_err());
    drop(s);
    assert!(Store::open(&temp.file()).is_ok());
}

#[test]
fn invalid_store_is_never_overwritten() {
    let temp = Temp::new();
    for bytes in [
        b"broken".to_vec(),
        br#"{"schema_version":2,"enabled":true,"entries":[]}"#.to_vec(),
        br#"{"schema_version":1,"enabled":false,"entries":[{"id":"fake","text":"private","copies":0,"last_copied_day":0}]}"#
            .to_vec(),
        vec![b' '; 1024 * 1024 + 1],
    ] {
        fs::write(temp.file(), &bytes).unwrap();
        assert!(Store::open(&temp.file()).is_err());
        assert_eq!(fs::read(temp.file()).unwrap(), bytes);
    }
}

#[test]
fn failed_save_does_not_change_memory_or_existing_file() {
    let temp = Temp::new();
    let mut s = Store::open(&temp.file()).unwrap();
    assert!(s.handle(request("add", json!({"text":"기존 표현"})), 1).ok);
    let before = fs::read(temp.file()).unwrap();
    let blocked = temp.file().with_extension("json.tmp");
    fs::create_dir(&blocked).unwrap();
    let r = s.handle(request("add", json!({"text":"저장하면 안 되는 표현"})), 1);
    assert!(!r.ok);
    assert_eq!(r.error, "store_unavailable");
    assert_eq!(s.handle(request("list", json!({})), 1).items.len(), 1);
    assert_eq!(fs::read(temp.file()).unwrap(), before);
    fs::remove_dir(blocked).unwrap();
}

fn cli(temp: &Temp, input: &[u8]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hanautomata-assist"))
        .arg("--store")
        .arg(temp.file())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.stderr.is_empty());
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[test]
fn jsonl_recovers_from_bad_request_and_keeps_ids() {
    let temp = Temp::new();
    let replies = cli(
        &temp,
        concat!(
            "{bad}\n",
            "{\"schema_version\":1,\"request_id\":\"a\",\"command\":\"add\",\"text\":\"검토 후 회신\"}\n",
            "{\"schema_version\":1,\"request_id\":\"b\",\"command\":\"set_enabled\",\"enabled\":true}\n",
            "{\"schema_version\":1,\"request_id\":\"c\",\"command\":\"query\",\"text\":\"검토\"}\n"
        )
        .as_bytes(),
    );
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[0]["ok"], false);
    assert_eq!(replies[3]["request_id"], "c");
    assert_eq!(replies[3]["items"][0]["text"], "검토 후 회신");
    assert!(!fs::read_to_string(temp.file()).unwrap().contains("\"query\""));
}

#[test]
fn oversized_partial_unknown_and_invalid_schema_frames_are_rejected() {
    let temp = Temp::new();
    for input in [
        vec![b'x'; 17 * 1024],
        b"{\"schema_version\":1}".to_vec(),
        b"{\"schema_version\":3,\"request_id\":\"x\",\"command\":\"list\"}\n".to_vec(),
        b"{\"schema_version\":1,\"request_id\":\"x\",\"command\":\"list\",\"secret\":1}\n".to_vec(),
    ] {
        assert_eq!(cli(&temp, &input)[0]["ok"], false);
    }
}

#[cfg(unix)]
#[test]
fn private_permissions_and_symlink_refusal() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = Temp::new();
    let mut s = Store::open(&temp.file()).unwrap();
    assert!(s.handle(request("add", json!({"text":"평문 저장 동의"})), 1).ok);
    assert_eq!(fs::metadata(temp.file()).unwrap().permissions().mode() & 0o777, 0o600);
    drop(s);
    let link = temp.0.join("link.json");
    symlink(temp.file(), &link).unwrap();
    assert!(Store::open(&link).is_err());
}
