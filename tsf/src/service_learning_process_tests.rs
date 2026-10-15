//! The real TextService callbacks in a synthetic document, then worker I/O to an owned child process.
//! Does not load/register an IME in an application or launch the product persistence service.
use super::*;
use crate::learning_pipe::{call, Command as WireCommand, Endpoint, Envelope, Listener, Peer};
use hanautomata_core::learning::{Authority, Lease, client::channel};
use std::{fs, io::{BufRead, BufReader, Write}, os::windows::process::CommandExt, path::PathBuf,
    process::{Child, Command, Stdio}, sync::mpsc, thread, time::{Duration, Instant}};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
const NONCE: &str = "cccccccccccccccccccccccccccccccc";
const CHILD: &str = "service::edit_failure_tests::learning_handoff_tests::process_tests::owned_service_broker_fixture";
const TIMEOUT: Duration = Duration::from_secs(3);

struct Fixture { root: PathBuf, child: Option<Child>, reader: Option<thread::JoinHandle<()>> }
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hanautomata-service-pipe-{}", Endpoint::random().unwrap().token()));
        fs::create_dir(&root).unwrap(); Self { root, child: None, reader: None }
    }
    fn start(&mut self, exchanges: u32) -> (Endpoint, Peer) {
        assert!(self.child.is_none());
        let endpoint = Endpoint::random().unwrap();
        let child = Command::new(std::env::current_exe().unwrap()).args(["--ignored", "--exact", CHILD, "--nocapture"])
            .env("HANAUTOMATA_SERVICE_TEST_STORE", self.root.join("learning.db"))
            .env("HANAUTOMATA_SERVICE_TEST_PIPE", endpoint.token())
            .env("HANAUTOMATA_SERVICE_TEST_COUNT", exchanges.to_string())
            .creation_flags(CREATE_NO_WINDOW.0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        self.child = Some(child); // Own it before readiness/authentication can fail.
        let child = self.child.as_mut().unwrap(); let output = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        self.reader = Some(thread::spawn(move || {
            for line in BufReader::new(output).lines().take(32) {
                if matches!(line.as_deref(), Ok("SERVICE_BROKER_READY")) { let _ = send.send(()); }
            }
        }));
        let ready = receive.recv_timeout(Duration::from_secs(5));
        if ready.is_err() { let _ = child.kill(); let _ = child.wait(); }
        ready.expect("owned broker readiness");
        let peer = Peer::process(child.id()).unwrap(); (endpoint, peer)
    }
    fn finish(&mut self) {
        let end = Instant::now() + Duration::from_secs(5);
        loop { match self.child.as_mut().unwrap().try_wait().unwrap() {
            Some(status) => { assert!(status.success()); self.child = None; self.reader.take().unwrap().join().unwrap(); return; },
            None if Instant::now() < end => thread::sleep(Duration::from_millis(10)),
            None => panic!("owned service broker exit deadline"),
        } }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child { if child.try_wait().ok().flatten().is_none() { let _ = child.kill(); let _ = child.wait(); } }
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
        let root = self.root.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("hanautomata-service-pipe-"));
        for name in ["learning.db", "learning.db.owner.lock"] {
            let path = root.join(name); if path.is_file() { fs::remove_file(path).unwrap(); }
        }
        fs::remove_dir(root).unwrap();
    }
}
fn request(endpoint: &Endpoint, peer: &Peer, command: WireCommand) -> serde_json::Value {
    let result = call(endpoint, peer, &Envelope::new(NONCE, command).unwrap(), TIMEOUT).unwrap();
    assert_eq!(result["ok"], true); result
}

#[test]
fn confirmed_service_edit_reaches_a_separate_authenticated_store_after_the_key_callback() {
    let mut broker = Fixture::new(); let (endpoint, peer) = broker.start(4);
    let lease: Lease = serde_json::from_value(request(&endpoint, &peer, WireCommand::Register {})["lease"].clone()).unwrap();
    let (producer, mut worker) = channel(lease).unwrap();
    let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    service.state.borrow_mut().learning.connect(producer);
    type_choice(&service, &context, b"DKSSUD"); assert!(worker.next().unwrap().is_none());
    commit(&service, &context); assert_eq!(&*host.text.borrow(), "dkssud ");
    assert_eq!(service.state.borrow().learning.report().queued, 1);
    // The document callback has returned, but no worker has sent its choice yet.
    assert!(request(&endpoint, &peer, WireCommand::Snapshot {})["snapshot"]["entries"].as_array().unwrap().is_empty());
    let caller = thread::current().id(); let (send, receive) = mpsc::channel();
    let drain = thread::spawn(move || {
        assert_ne!(thread::current().id(), caller);
        let dispatch = worker.next().unwrap().unwrap();
        let reply = request(&endpoint, &peer, WireCommand::Apply { request: dispatch.request().clone() });
        assert_eq!(worker.finish(&dispatch, Ok(serde_json::from_value(reply["reply"].clone()).unwrap())).unwrap(), Progress::Committed);
        let _ = send.send((worker, endpoint, peer));
    });
    let (mut worker, endpoint, peer) = receive.recv_timeout(Duration::from_secs(5)).unwrap(); drain.join().unwrap();
    assert!(worker.next().unwrap().is_none());
    type_choice(&service, &context, b"HELLO"); host.fail_set.set(true); let _ = paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary);
    assert!(worker.next().unwrap().is_none(), "a rejected second edit cannot persist");
    let snapshot = request(&endpoint, &peer, WireCommand::Snapshot {})["snapshot"].clone();
    assert_eq!(snapshot["entries"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["entries"][0]["english_weight"], 1.0);
    assert_eq!(snapshot["entries"][0]["korean_weight"], 0.0);
    broker.finish();
    // A second actual broker process opens the file; neither TSF nor this test writes a whole dictionary.
    let (endpoint, peer) = broker.start(1);
    assert_eq!(request(&endpoint, &peer, WireCommand::Snapshot {})["snapshot"]["entries"], snapshot["entries"]);
    broker.finish();
    println!("service document callbacks -> memory queue -> other thread -> authenticated child store -> restarted child snapshot; one accepted choice, rejected edit absent");
}

#[test]
#[ignore = "explicit owned child only"]
fn owned_service_broker_fixture() {
    let path = PathBuf::from(std::env::var_os("HANAUTOMATA_SERVICE_TEST_STORE").unwrap());
    let endpoint = Endpoint::from_token(&std::env::var("HANAUTOMATA_SERVICE_TEST_PIPE").unwrap()).unwrap();
    let count: u32 = std::env::var("HANAUTOMATA_SERVICE_TEST_COUNT").unwrap().parse().unwrap();
    assert!((1..=8).contains(&count));
    let mut listener = Listener::bind(&endpoint).unwrap();
    let mut authority = Authority::open(path, Endpoint::random().unwrap().token()).unwrap();
    println!("SERVICE_BROKER_READY"); std::io::stdout().flush().unwrap();
    for _ in 0..count { listener.serve_once(&mut authority, Duration::from_secs(5)).unwrap(); }
}
