#![cfg(all(windows, feature = "quality-probes"))]
//! Synthetic data, explicitly owned processes/files, no installed endpoint or TSF registration.
use hanautomata_core::{LearningChoice, learning::{client::{channel, Progress}, Error as StoreError, Lease, Operation, Request}};
use hanautomata_tsf::learning_pipe::{call, Command as WireCommand, Endpoint, Envelope, Peer};
use serde_json::{json, Value};
use std::{fs, io::{BufRead, BufReader}, os::windows::process::CommandExt, path::{Path, PathBuf},
    process::{Child, Command, Stdio}, sync::mpsc, thread, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
const NONCE: &str = "cccccccccccccccccccccccccccccccc";
const TIMEOUT: Duration = Duration::from_secs(3);

struct Broker { child: Child, endpoint: Endpoint, peer: Peer }
impl Broker {
    fn new(path: &Path, count: u32) -> Self {
        let endpoint = Endpoint::random().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_hanautomata-learning-store"))
            .args(["--development-pipe", "--store"]).arg(path).args(["--endpoint", endpoint.token(), "--exchanges", &count.to_string()])
            .creation_flags(CREATE_NO_WINDOW.0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let output = child.stdout.take().unwrap(); let (send, receive) = mpsc::channel();
        let reader = thread::spawn(move || { let mut line = String::new(); let _ = BufReader::new(output).read_line(&mut line); let _ = send.send(line); });
        let ready = receive.recv_timeout(Duration::from_secs(5));
        if ready.as_deref() != Ok("ready\n") { let _ = child.kill(); let _ = child.wait(); reader.join().unwrap(); panic!("owned pipe broker did not become ready: {ready:?}"); }
        reader.join().unwrap();
        let peer = match Peer::process(child.id()) {
            Ok(peer) => peer,
            Err(error) => { let _ = child.kill(); let _ = child.wait(); panic!("owned broker identity refused: {error}"); },
        };
        Self { child, endpoint, peer }
    }
    fn request(&self, command: WireCommand) -> Value {
        call(&self.endpoint, &self.peer, &Envelope::new(NONCE, command).unwrap(), TIMEOUT).unwrap()
    }
    fn register(&self) -> Lease {
        let result = self.request(WireCommand::Register {}); assert_eq!(result["ok"], true);
        serde_json::from_value(result["lease"].clone()).unwrap()
    }
    fn finish(&mut self) {
        let end = Instant::now() + Duration::from_secs(5);
        loop { match self.child.try_wait().unwrap() {
            Some(status) => { assert!(status.success(), "broker exit: {status}"); break; },
            None if Instant::now() < end => thread::sleep(Duration::from_millis(10)),
            None => panic!("owned broker did not exit"),
        } }
    }
}
impl Drop for Broker { fn drop(&mut self) { if self.child.try_wait().ok().flatten().is_none() { let _ = self.child.kill(); let _ = self.child.wait(); } } }
struct Files { root: PathBuf, path: PathBuf }
impl Files {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hanautomata-auth-pipe-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap(); Self { path: root.join("learning.db"), root }
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("hanautomata-auth-pipe-"));
        for name in ["learning.db", "learning.db.owner.lock"] { let path = root.join(name); if path.is_file() { fs::remove_file(path).unwrap(); } }
        fs::remove_dir(root).unwrap();
    }
}
fn learn(lease: &Lease) -> Request {
    Request { store_id: lease.version.store_id.clone(), client_id: lease.client_id.clone(), lease_revision: lease.lease_revision,
        sequence: lease.next_sequence, generation: lease.version.generation,
        operation: Operation::Learn { raw: "dkssud".into(), keys: "dkssud".into(), korean: true, context: String::new() } }
}

#[test]
fn actual_authenticated_pipe_restart_replays_only_its_bound_lease_once() {
    let files = Files::new(); let mut broker = Broker::new(&files.path, 2); let first_pid = broker.child.id();
    let lease = broker.register(); let (producer, mut worker) = channel(lease.clone()).unwrap();
    let choice = LearningChoice { raw: "dkssud".into(), keys: "dkssud".into(), korean: true, context: String::new() };
    producer.submit(producer.begin().unwrap(), &[choice.clone()]).unwrap();
    let original = worker.next().unwrap().unwrap(); let serialized = serde_json::to_vec(original.request()).unwrap();
    let result = broker.request(WireCommand::Apply { request: original.request().clone() });
    assert_eq!(result["ok"], true); assert_eq!(result["reply"]["duplicate"], false);
    // The caller deliberately leaves its client state unacknowledged before the actual server restart.
    broker.finish(); drop(broker); let mut broker = Broker::new(&files.path, 7);
    assert_ne!(broker.child.id(), first_pid);
    let resumed = broker.register(); assert_eq!(resumed.client_id, lease.client_id); assert_eq!(resumed.next_sequence, 2);
    let retry = worker.next().unwrap().unwrap(); assert_eq!(serde_json::to_vec(retry.request()).unwrap(), serialized);
    let result = broker.request(WireCommand::Apply { request: retry.request().clone() }); assert_eq!(result["reply"]["duplicate"], true);
    assert_eq!(worker.finish(&retry, Ok(serde_json::from_value(result["reply"].clone()).unwrap())).unwrap(), Progress::Committed);
    let result = broker.request(WireCommand::Snapshot {});
    assert_eq!(result["snapshot"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(result["snapshot"]["entries"][0]["korean_weight"], 1.0);
    let delayed = producer.begin().unwrap();
    let manager_nonce = "dddddddddddddddddddddddddddddddd";
    let manager = |command| call(&broker.endpoint, &broker.peer, &Envelope::new(manager_nonce, command).unwrap(), TIMEOUT).unwrap();
    let admin: Lease = serde_json::from_value(manager(WireCommand::Register {})["lease"].clone()).unwrap();
    let mut reset = learn(&admin); reset.operation = Operation::Reset {};
    let result = manager(WireCommand::Apply { request: reset }); assert_eq!(result["ok"], true);
    worker.observe(serde_json::from_value(result["reply"]["version"].clone()).unwrap()).unwrap();
    producer.submit(delayed, &[choice]).unwrap(); let stale = worker.next().unwrap().unwrap();
    let result = broker.request(WireCommand::Apply { request: stale.request().clone() }); assert_eq!(result["error"], "stale_generation");
    assert_eq!(worker.finish(&stale, Err(StoreError::StaleGeneration)).unwrap(), Progress::DroppedStale);
    assert!(broker.request(WireCommand::Snapshot {})["snapshot"]["entries"].as_array().unwrap().is_empty());
    broker.finish(); println!("authenticated server PIDs {first_pid} -> {}; duplicate once; pre-reset delayed choice refused", broker.child.id());
}

#[test]
fn different_nonce_and_process_cannot_replay_a_known_lease() {
    let files = Files::new(); let mut broker = Broker::new(&files.path, 4); let lease = broker.register(); let request = learn(&lease);
    let other = Envelope::new("dddddddddddddddddddddddddddddddd", WireCommand::Apply { request: request.clone() }).unwrap();
    assert_eq!(call(&broker.endpoint, &broker.peer, &other, TIMEOUT).unwrap()["error"], "unauthorized");
    let mut child = Command::new(std::env::current_exe().unwrap()).args(["--ignored", "--exact", "foreign_client_fixture", "--nocapture"])
        .env("HANAUTOMATA_TEST_PIPE", broker.endpoint.token()).env("HANAUTOMATA_TEST_SERVER", broker.child.id().to_string())
        .env("HANAUTOMATA_TEST_REQUEST", serde_json::to_string(&request).unwrap())
        .creation_flags(CREATE_NO_WINDOW.0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop { match child.try_wait().unwrap() {
        Some(status) => { assert!(status.success()); break; }, None if Instant::now() < end => thread::sleep(Duration::from_millis(10)),
        None => { let _ = child.kill(); let _ = child.wait(); panic!("owned foreign client timed out"); },
    } }
    assert!(broker.request(WireCommand::Snapshot {})["snapshot"]["entries"].as_array().unwrap().is_empty()); broker.finish();
}

#[test]
fn client_checks_the_expected_server_before_disclosing_a_request() {
    let files = Files::new(); let mut broker = Broker::new(&files.path, 1);
    let wrong = Peer::process(std::process::id()).unwrap();
    let result = call(&broker.endpoint, &wrong, &Envelope::new(NONCE, WireCommand::Snapshot {}).unwrap(), TIMEOUT);
    assert_eq!(result, Err(hanautomata_tsf::learning_pipe::Error::Unauthorized)); broker.finish();
}

#[test]
#[ignore = "explicit owned child only"]
fn foreign_client_fixture() {
    let endpoint = Endpoint::from_token(&std::env::var("HANAUTOMATA_TEST_PIPE").unwrap()).unwrap();
    let peer = Peer::process(std::env::var("HANAUTOMATA_TEST_SERVER").unwrap().parse().unwrap()).unwrap();
    let request = serde_json::from_str(&std::env::var("HANAUTOMATA_TEST_REQUEST").unwrap()).unwrap();
    let result = call(&endpoint, &peer, &Envelope::new(NONCE, WireCommand::Apply { request }).unwrap(), TIMEOUT).unwrap();
    assert_eq!(result, json!({"ok":false,"error":"unauthorized"}));
}
