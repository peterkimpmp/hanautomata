//! Owned companion processes and dedicated files only; no installation or user learning data.
#![cfg(windows)]
use hanautomata_tsf::{learning_pipe::Endpoint, learning_runtime::{Runtime, Phase}};
use hanautomata_core::{LearningChoice, learning::Authority};
use std::{path::PathBuf, thread, time::{Duration, Instant}};

fn wait(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(12);
    while !predicate() { assert!(Instant::now() < end, "bounded companion deadline"); thread::sleep(Duration::from_millis(10)); }
}
struct Files(PathBuf);
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard { fn drop(&mut self) {
    if self.0.try_wait().ok().flatten().is_none() { let _ = self.0.kill(); }
    let _ = self.0.wait();
} }
impl Files {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hanautomata-runtime-{}", Endpoint::random().unwrap().token()));
        std::fs::create_dir(&root).unwrap(); Self(root)
    }
    fn store(&self) -> PathBuf { self.0.join("learning.db") }
}
impl Drop for Files { fn drop(&mut self) {
    for name in ["learning.db", "learning.db.owner.lock"] {
        let path = self.0.join(name); if path.exists() { std::fs::remove_file(path).unwrap(); }
    }
    std::fs::remove_dir(&self.0).unwrap();
} }
fn start(files: &Files) -> Runtime {
    Runtime::start(PathBuf::from(env!("CARGO_BIN_EXE_hanautomata-ime-setup")), files.store()).unwrap()
}
fn ready(runtime: &mut Runtime) -> hanautomata_tsf::learning_runtime::Ready {
    let mut result = None;
    wait(|| { result = runtime.try_ready(); result.is_some() }); result.unwrap()
}

fn registered_child(files: &Files) -> (ChildGuard, hanautomata_core::learning::Lease) {
    use hanautomata_tsf::learning_pipe::{call, Envelope, Command as WireCommand, Peer};
    use std::{process::{Command, Stdio}, os::windows::process::CommandExt};
    use windows::Win32::{System::Threading::{GetCurrentProcess, GetProcessTimes}, Foundation::FILETIME};
    let endpoint = Endpoint::random().unwrap(); let nonce = Endpoint::random().unwrap();
    let (mut born, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut born, &mut exited, &mut kernel, &mut user).unwrap(); }
    let born = (born.dwHighDateTime as u64) << 32 | born.dwLowDateTime as u64;
    let child = ChildGuard(Command::new(env!("CARGO_BIN_EXE_hanautomata-ime-setup")).arg("--learning-companion")
        .args([endpoint.token(), nonce.token(), &std::process::id().to_string(), &born.to_string()]).arg(files.store())
        .creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    let peer = Peer::process(child.0.id()).unwrap(); let mut lease = None;
    wait(|| {
        if let Ok(value) = call(&endpoint, &peer, &Envelope::new(nonce.token(), WireCommand::Register {}).unwrap(), Duration::from_secs(1)) {
            lease = serde_json::from_value(value["lease"].clone()).ok();
        }
        lease.is_some()
    });
    (child, lease.unwrap())
}

#[test]
fn rollback_management_refuses_a_live_process_and_preserves_words_after_its_exit() {
    use hanautomata_core::learning::{Error, Request, Operation};
    use hanautomata_tsf::learning_pipe::prepare_rollback;
    let files = Files::new();
    assert_eq!(prepare_rollback(&files.store()), Err(Error::MissingState));
    assert!(!files.store().exists());
    let (mut child, lease) = registered_child(&files);
    let mut owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    owner.apply(&Request { store_id: lease.version.store_id.clone(), client_id: lease.client_id.clone(),
        lease_revision: lease.lease_revision, sequence: 1, generation: lease.version.generation,
        operation: Operation::Learn { raw: "retained".into(), keys: "retained".into(), korean: true, context: String::new() } }).unwrap();
    let words = owner.snapshot().unwrap().entries; drop(owner);
    let before = std::fs::read(files.store()).unwrap();
    assert_eq!(prepare_rollback(&files.store()), Err(Error::Busy));
    assert_eq!(std::fs::read(files.store()).unwrap(), before);
    child.0.kill().unwrap(); child.0.wait().unwrap();
    assert_eq!(prepare_rollback(&files.store()).unwrap(), 1);
    assert!(std::fs::read(files.store()).unwrap().starts_with(b"Hanautomata.TsfLearning/1\n"));
    let owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries, words); assert!(owner.clients().unwrap().is_empty());
}

#[test]
fn killed_companion_is_reclaimed_while_live_and_unmanaged_leases_survive() {
    let files = Files::new(); let mut live = start(&files); let live_client = ready(&mut live);
    let mut owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    let unmanaged = owner.register(Endpoint::random().unwrap().token()).unwrap(); drop(owner);
    let (mut child, dead) = registered_child(&files);
    child.0.kill().unwrap(); child.0.wait().unwrap();
    // Fill the remaining capacity with explicitly managed orphan records. One orphan above is an actual kill.
    let mut owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    for _ in owner.clients().unwrap().len()..hanautomata_core::learning::MAX_CLIENTS {
        owner.register_with_lifetime(Endpoint::random().unwrap().token(), hanautomata_core::learning::LeaseLifetime::WindowsMutexV1).unwrap();
    }
    assert_eq!(owner.clients().unwrap().len(), hanautomata_core::learning::MAX_CLIENTS); drop(owner);
    let mut fresh = start(&files); let _fresh_client = ready(&mut fresh);
    // A concurrent active companion must still be able to commit after reclamation.
    live_client.producer.submit(live_client.producer.begin().unwrap(), &[LearningChoice {
        raw: "alive".into(), keys: "alive".into(), korean: true, context: String::new(),
    }]).unwrap();
    wait(|| live.status().committed == 1);
    live.stop(); fresh.stop(); wait(|| live.status().phase == Phase::Stopped && fresh.status().phase == Phase::Stopped);
    let owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    let clients = owner.clients().unwrap();
    assert!(clients.iter().all(|c| c.key.client_id != dead.client_id), "a dead companion lease must not accumulate");
    assert_eq!(clients.len(), 1); assert_eq!(clients[0].key.client_id, unmanaged.client_id);
    assert_eq!(owner.snapshot().unwrap().entries[0].raw, "alive");
}

#[test]
fn a_lease_left_by_busy_shutdown_is_reclaimed_on_next_start() {
    let files = Files::new(); let mut first = start(&files); let _client = ready(&mut first);
    let lock = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    first.stop(); wait(|| first.status().phase == Phase::Failed); // the owned companion could not retire under this lock
    assert_eq!(lock.clients().unwrap().len(), 1); drop(lock);
    let mut next = start(&files); let _next_client = ready(&mut next);
    next.stop(); wait(|| next.status().phase == Phase::Stopped);
    let owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    assert!(owner.clients().unwrap().is_empty());
}

#[test]
fn two_real_companions_merge_choices_retire_leases_and_reload_without_key_thread_io() {
    let files = Files::new();
    let mut first = start(&files); let a = ready(&mut first);
    let mut second = start(&files); let b = ready(&mut second);
    assert!(a.snapshot.entries.is_empty()); assert!(b.snapshot.entries.is_empty());
    for (producer, raw) in [(&a.producer, "alpha"), (&b.producer, "bravo")] {
        producer.submit(producer.begin().unwrap(), &[LearningChoice { raw: raw.into(), keys: raw.into(), korean: true, context: String::new() }]).unwrap();
    }
    wait(|| first.status().committed == 1 && second.status().committed == 1);
    first.stop(); second.stop();
    wait(|| first.status().phase == Phase::Stopped && second.status().phase == Phase::Stopped);
    let authority = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    assert_eq!(authority.snapshot().unwrap().entries.len(), 2);
    assert!(authority.clients().unwrap().is_empty(), "normal shutdown retires both leases"); drop(authority);
    let mut restarted = start(&files); let restored = ready(&mut restarted);
    assert_eq!(restored.snapshot.entries.len(), 2);
    restarted.stop(); wait(|| restarted.status().phase == Phase::Stopped);
}

#[test]
fn unavailable_companion_fails_without_hanging_or_creating_a_store() {
    let files = Files::new();
    let runtime = Runtime::start(files.0.join("missing.exe"), files.store()).unwrap();
    wait(|| runtime.status().phase == Phase::Failed);
    assert!(!files.store().exists());
}

#[test]
fn idle_worker_waits_for_work_instead_of_polling_and_wakes_for_a_choice() {
    let files = Files::new(); let mut runtime = start(&files); let client = ready(&mut runtime);
    thread::sleep(Duration::from_millis(100));
    let before = runtime.status().idle_waits; thread::sleep(Duration::from_millis(300));
    let repeated = runtime.status().idle_waits - before;
    client.producer.submit(client.producer.begin().unwrap(), &[LearningChoice {
        raw: "wakeup".into(), keys: "wakeup".into(), korean: true, context: String::new(),
    }]).unwrap();
    wait(|| runtime.status().committed == 1); runtime.stop(); wait(|| runtime.status().phase == Phase::Stopped);
    assert!(repeated <= 1, "idle worker woke {repeated} times without input");
}

#[test]
fn an_exclusive_store_lock_does_not_block_submission_and_shutdown_drains_after_release() {
    let files = Files::new(); let mut runtime = start(&files); let client = ready(&mut runtime);
    let lock = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    let action = client.producer.begin().unwrap();
    client.producer.submit(action, &[LearningChoice { raw: "blockedstore".into(), keys: "blockedstore".into(), korean: false, context: String::new() }]).unwrap();
    wait(|| runtime.status().failures > 0);
    thread::sleep(Duration::from_secs(1));
    assert_eq!(runtime.status().phase, Phase::Running, "ordinary contention must not permanently disable learning");
    assert_eq!(runtime.status().committed, 0);
    runtime.stop(); assert!(client.producer.begin().is_err(), "stop rejects late submissions"); drop(lock);
    wait(|| runtime.status().phase == Phase::Stopped);
    assert_eq!(runtime.status().committed, 1);
    assert!(runtime.status().failures > 0, "a later success retains the diagnostic failure count");
    let owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries.len(), 1); assert!(owner.clients().unwrap().is_empty());
}

#[test]
fn dropping_during_busy_startup_releases_its_owned_companion() {
    let files = Files::new(); let lock = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    let mut runtime = start(&files);
    thread::sleep(Duration::from_millis(100)); assert!(runtime.try_ready().is_none()); runtime.stop();
    drop(lock);
    wait(|| matches!(runtime.status().phase, Phase::Failed | Phase::Stopped));
    let owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    assert!(owner.clients().unwrap().is_empty());
}

#[test]
fn product_companion_refuses_management_operations_and_wrong_session_nonce() {
    use hanautomata_tsf::learning_pipe::{call, Envelope, Command as WireCommand, Peer};
    use hanautomata_core::learning::{Lease, Request, Operation};
    use std::{process::{Command, Stdio}, os::windows::process::CommandExt};
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use windows::Win32::Foundation::FILETIME;
    let files = Files::new(); let endpoint = Endpoint::random().unwrap(); let nonce = Endpoint::random().unwrap();
    let (mut born, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut born, &mut exited, &mut kernel, &mut user).unwrap(); }
    let born = (born.dwHighDateTime as u64) << 32 | born.dwLowDateTime as u64;
    let mut child = ChildGuard(Command::new(env!("CARGO_BIN_EXE_hanautomata-ime-setup")).arg("--learning-companion")
        .args([endpoint.token(), nonce.token(), &std::process::id().to_string(), &born.to_string()]).arg(files.store())
        .creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    let peer = Peer::process(child.0.id()).unwrap();
    let mut lease = None;
    wait(|| {
        if let Ok(value) = call(&endpoint, &peer, &Envelope::new(nonce.token(), WireCommand::Register {}).unwrap(), Duration::from_secs(1)) {
            lease = serde_json::from_value::<Lease>(value["lease"].clone()).ok();
        }
        lease.is_some()
    });
    let lease = lease.unwrap();
    let request = Request { store_id: lease.version.store_id, client_id: lease.client_id, lease_revision: lease.lease_revision,
        sequence: lease.next_sequence, generation: lease.version.generation, operation: Operation::Reset {} };
    let denied = call(&endpoint, &peer, &Envelope::new(nonce.token(), WireCommand::Apply { request }).unwrap(), Duration::from_secs(1)).unwrap();
    assert_eq!(denied["error"], "unauthorized");
    let denied = call(&endpoint, &peer, &Envelope::new(Endpoint::random().unwrap().token(), WireCommand::Snapshot {}).unwrap(), Duration::from_secs(1)).unwrap();
    assert_eq!(denied["error"], "unauthorized");
    let closed = call(&endpoint, &peer, &Envelope::new(nonce.token(), WireCommand::Close { lease_revision: 0 }).unwrap(), Duration::from_secs(1)).unwrap();
    assert_eq!(closed["ok"], true);
    wait(|| child.0.try_wait().unwrap().is_some()); assert!(child.0.wait().unwrap().success());
    let owner = Authority::open(files.store(), Endpoint::random().unwrap().token()).unwrap();
    assert!(owner.snapshot().unwrap().entries.is_empty()); assert!(owner.clients().unwrap().is_empty());
}
