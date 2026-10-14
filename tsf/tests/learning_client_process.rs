#![cfg(all(windows, feature = "quality-probes"))]
//! Real owned child-process transport, with synthetic data and explicitly selected storage only.
//! This verifies the client/authority handoff over private stdio, not authenticated production IPC or TSF hosts.
use hanautomata_core::{WordPreferences, learning::{client::{channel, Progress}, Error, Lease, Operation, Reply, Request}};
use serde_json::{json, Value};
use std::{fs, io::{BufRead, BufReader, Read, Write}, os::windows::process::CommandExt, path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio}, sync::mpsc::{self, Receiver}, thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

struct Driver { child: Child, input: Option<ChildStdin>, replies: Receiver<Vec<u8>>, reader: Option<JoinHandle<()>> }
impl Driver {
    fn new(path: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hanautomata-learning-store"))
            .args(["--development-stdio", "--store"]).arg(path).creation_flags(CREATE_NO_WINDOW.0)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let input = child.stdin.take(); let output = child.stdout.take().unwrap();
        let (send, replies) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut input = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let read = (&mut input).take(2_097_153).read_until(b'\n', &mut bytes);
                if read.is_err() || bytes.is_empty() { break; }
                let valid = bytes.len() <= 2_097_152 && bytes.last() == Some(&b'\n');
                if send.send(bytes).is_err() || !valid { break; }
            }
        });
        Self { child, input, replies, reader: Some(reader) }
    }
    fn call(&mut self, value: Value) -> Value {
        let input = self.input.as_mut().unwrap(); serde_json::to_writer(&mut *input, &value).unwrap();
        input.write_all(b"\n").unwrap(); input.flush().unwrap();
        let bytes = self.replies.recv_timeout(Duration::from_secs(5)).expect("owned storage process did not reply");
        assert!(bytes.len() <= 2_097_152 && bytes.last() == Some(&b'\n'));
        serde_json::from_slice(&bytes).unwrap()
    }
    fn apply(&mut self, request: &Request) -> std::result::Result<Reply, Error> {
        let value = self.call(json!({"command":"apply", "request":request}));
        if value["ok"] == true { return Ok(serde_json::from_value(value["reply"].clone()).unwrap()); }
        assert_eq!(value["error"], "stale_generation", "unexpected development driver error");
        Err(Error::StaleGeneration)
    }
    fn register(&mut self) -> Lease {
        let value = self.call(json!({"command":"register"})); assert_eq!(value["ok"], true);
        serde_json::from_value(value["lease"].clone()).unwrap()
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => { let _ = self.child.kill(); let _ = self.child.wait(); break; }, // only this fixture's child
            }
        }
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
    }
}
struct Files { root: PathBuf, path: PathBuf }
impl Files {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hanautomata-client-process-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap(); Self { path: root.join("학습 데이터.db"), root }
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("hanautomata-client-process-"));
        for name in ["학습 데이터.db", "학습 데이터.db.owner.lock"] {
            let path = root.join(name); if path.is_file() { fs::remove_file(path).unwrap(); }
        }
        fs::remove_dir(root).unwrap();
    }
}

#[test]
fn actual_storage_process_restart_keeps_exact_retry_and_rejects_old_document_generation() {
    let files = Files::new(); let mut driver = Driver::new(&files.path); let old_pid = driver.child.id();
    let lease = driver.register(); let (producer, mut worker) = channel(lease.clone()).unwrap();
    let preferences = WordPreferences::new(); preferences.defer_learning(true);
    let action = producer.begin().unwrap(); preferences.remember_global("dkssud", "dkssud", true);
    let batch = preferences.take_learning().unwrap();
    producer.submit(action, &preferences.confirm_learning(&batch)).unwrap();
    let request = worker.next().unwrap().unwrap(); let serialized = serde_json::to_vec(request.request()).unwrap();
    let acknowledged = driver.apply(request.request()).unwrap(); assert!(!acknowledged.duplicate);
    // The authority committed, but the worker never received that acknowledgement.
    drop(driver); let mut driver = Driver::new(&files.path); assert_ne!(driver.child.id(), old_pid);
    let resume = driver.call(json!({"command":"resume", "key":{"client_id":lease.client_id,"lease_revision":lease.lease_revision}}));
    assert_eq!(resume["lease"]["next_sequence"], 2);
    let retry = worker.next().unwrap().unwrap(); assert_eq!(serde_json::to_vec(retry.request()).unwrap(), serialized);
    let reply = driver.apply(retry.request()).unwrap(); assert!(reply.duplicate);
    assert_eq!(worker.finish(&retry, Ok(reply)).unwrap(), Progress::Committed);
    let snapshot = driver.call(json!({"command":"snapshot"}));
    assert_eq!(snapshot["snapshot"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["snapshot"]["entries"][0]["korean_weight"], 1.0);

    let delayed = producer.begin().unwrap(); preferences.remember_global("hello", "hello", true);
    let delayed_batch = preferences.take_learning().unwrap();
    let admin = driver.register();
    let reset = Request { store_id: admin.version.store_id, client_id: admin.client_id, lease_revision: admin.lease_revision,
        sequence: admin.next_sequence, generation: admin.version.generation, operation: Operation::Reset {} };
    driver.apply(&reset).unwrap();
    producer.submit(delayed, &preferences.confirm_learning(&delayed_batch)).unwrap();
    let stale = worker.next().unwrap().unwrap();
    assert_eq!(worker.finish(&stale, driver.apply(stale.request())).unwrap(), Progress::DroppedStale);
    let snapshot = driver.call(json!({"command":"snapshot"}));
    assert!(snapshot["snapshot"]["entries"].as_array().unwrap().is_empty());
    worker.observe(serde_json::from_value(snapshot["snapshot"]["version"].clone()).unwrap()).unwrap();
    assert!(worker.next().unwrap().is_none());
    println!("owned storage PIDs {old_pid} -> {}; exact retry once; delayed pre-reset choice rejected", driver.child.id());
}
