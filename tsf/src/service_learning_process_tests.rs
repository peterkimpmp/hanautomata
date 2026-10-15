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

fn until(mut predicate: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(4);
    while !predicate() { if Instant::now() >= end { return false; } thread::sleep(Duration::from_millis(10)); }
    true
}

#[test]
#[ignore = "requires HANAUTOMATA_TEST_SETUP; owned store and synthetic document only"]
fn live_reset_waits_for_word_boundary_and_new_words_use_the_installed_generation() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new(); let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    let legacy = hanautomata_core::WordPreferences::new(); legacy.remember_global("hello", "hello", false);
    let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").unwrap());
    service.state.borrow_mut().learning_runtime = Some(Runtime::start_with_legacy(setup, files.root.join("learning.db"), legacy.export().0).unwrap());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().initial_snapshot_applied == Some(true) }));
        let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone(); assert_eq!(engine.learned_count(), 1);
        type_choice(&service, &context, b"DKSSUD"); let composing = host.text.borrow().clone();
        let mut owner = files.open_store();
        owner.manage(&owner.snapshot().unwrap().version, hanautomata_core::learning::Operation::Reset {}).unwrap(); drop(owner);
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning_ready.as_ref().is_some_and(|r| r.snapshot.version.generation > 1) }), "hold the incoming Reset while the word is composing");
        assert_eq!(&*host.text.borrow(), &composing); assert_eq!(engine.learned_count(), 1);
        commit(&service, &context); assert_eq!(&*host.text.borrow(), "dkssud ");
        assert!(until(|| { service.poll_learning(); engine.learned_count() == 0 }), "apply Reset after the old word is finished");
        type_choice(&service, &context, b"HELLO"); commit(&service, &context);
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().storage.is_some_and(|s| s.committed == 1) }));
        let owner = files.open_store(); let entries = owner.snapshot().unwrap().entries;
        assert_eq!(entries.len(), 2, "the fresh word has global and preceding-word context entries");
        assert!(entries.iter().all(|word| word.raw == "hello" && word.keys == "hello"
            && word.korean_weight + word.english_weight == 1.0));
        assert!(entries.iter().any(|word| word.context.is_empty()));
        assert!(entries.iter().any(|word| word.context == hanautomata_core::context_key("dkssud")));
        assert_eq!(service.state.borrow().learning.report().rejected, 1);
        assert_eq!(service.state.borrow().learning.report().recovery_invalidated, 1);
    }));
    let runtime = service.state.borrow_mut().learning_runtime.take().unwrap();
    unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    runtime.stop(); assert!(until(|| matches!(runtime.status().phase, Phase::Stopped | Phase::Failed)));
    if let Err(error) = outcome { std::panic::resume_unwind(error); }
}

#[test]
#[ignore = "requires HANAUTOMATA_TEST_SETUP; preserves an unpersisted local choice in a synthetic document"]
fn live_snapshot_cannot_discard_a_local_choice_rejected_before_connection() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new(); let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    type_choice(&service, &context, b"DKSSUD"); commit(&service, &context);
    let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone();
    assert_eq!(engine.learned_count(), 1); assert_eq!(service.state.borrow().learning.report().rejected, 1);
    let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").unwrap());
    service.state.borrow_mut().learning_runtime = Some(Runtime::start(setup, files.root.join("learning.db")).unwrap());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().connected }));
        assert_eq!(engine.learned_count(), 1); assert_eq!(&*host.text.borrow(), "dkssud ");
        assert_eq!(service.state.borrow().learning.report().initial_snapshot_applied, Some(true));
        assert_eq!(service.state.borrow().learning.report().recovery_pending, 1);
        assert!(service.state.borrow().learning_ready.is_none(), "install the snapshot with the bounded local-only overlay");
        let owner = files.open_store(); assert!(owner.snapshot().unwrap().entries.is_empty());
    }));
    let runtime = service.state.borrow_mut().learning_runtime.take().unwrap();
    unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    runtime.stop(); assert!(until(|| matches!(runtime.status().phase, Phase::Stopped | Phase::Failed)));
    if let Err(error) = outcome { std::panic::resume_unwind(error); }
}

#[test]
#[ignore = "requires HANAUTOMATA_TEST_SETUP; startup rejection must recover without restamping the old word"]
fn startup_choice_does_not_disable_later_persistence_or_live_reset() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new(); let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    type_choice(&service, &context, b"DKSSUD"); commit(&service, &context);
    let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone(); assert_eq!(engine.learned_count(), 1);
    let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").unwrap());
    service.state.borrow_mut().learning_runtime = Some(Runtime::start(setup, files.root.join("learning.db")).unwrap());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().connected }));
        assert_eq!(engine.learned_count(), 1, "retain the startup choice locally");
        type_choice(&service, &context, b"HELLO"); commit(&service, &context);
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().storage.is_some_and(|s| s.committed == 1) }),
            "a single startup rejection must not disable persistence for all later words");
        let mut owner = files.open_store();
        assert!(owner.snapshot().unwrap().entries.iter().all(|word| word.raw == "hello"), "never re-stamp the startup word for storage");
        owner.manage(&owner.snapshot().unwrap().version, hanautomata_core::learning::Operation::Forget { raw: "hello".into(), keys: "hello".into() }).unwrap(); drop(owner);
        assert!(until(|| { service.poll_learning(); let entries = engine.learning_entries();
            entries.len() == 1 && entries[0].raw == "dkssud" }), "Forget must preserve the unrelated local-only startup choice");
        assert_eq!(service.state.borrow().learning.report().recovery_pending, 1);
        assert_eq!(service.state.borrow().learning.report().recovery_invalidated, 0);
        let before_reset_text = host.text.borrow().clone(); let mut owner = files.open_store();
        owner.manage(&owner.snapshot().unwrap().version, hanautomata_core::learning::Operation::Reset {}).unwrap(); drop(owner);
        assert!(until(|| { service.poll_learning(); engine.learned_count() == 0 }), "an explicit Reset wins over older unsaved local memory");
        assert_eq!(&*host.text.borrow(), &before_reset_text); assert_eq!(service.state.borrow().learning.report().recovery_pending, 0);
        assert_eq!(service.state.borrow().learning.report().recovery_invalidated, 1);
        type_choice(&service, &context, b"WORLD"); commit(&service, &context);
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().storage.is_some_and(|s| s.committed == 2) }));
        let owner = files.open_store(); let entries = owner.snapshot().unwrap().entries;
        assert!(!entries.is_empty()); assert!(entries.iter().all(|word| word.raw == "world"));
    }));
    let runtime = service.state.borrow_mut().learning_runtime.take().unwrap();
    unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    runtime.stop(); assert!(until(|| matches!(runtime.status().phase, Phase::Stopped | Phase::Failed)));
    if let Err(error) = outcome { std::panic::resume_unwind(error); }
}

#[test]
#[ignore = "requires HANAUTOMATA_TEST_SETUP; two service workers and one owned store, synthetic documents only"]
fn another_services_committed_choice_supersedes_local_only_fallback_for_that_word() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new(); let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").unwrap());
    let (first, first_host, first_context) = fixture(); first_host.caret_selection.set(true);
    first.state.borrow_mut().sink = Some(TestCompositionSink.into());
    // These choices predate any connection, so they cannot later acquire persistence permission.
    type_choice(&first, &first_context, b"DKSSUD"); commit(&first, &first_context);
    type_choice(&first, &first_context, b"HELLO"); commit(&first, &first_context);
    let document_before = first_host.text.borrow().clone();
    let engine = first.state.borrow().shell.as_ref().unwrap().engine().clone();
    first.state.borrow_mut().learning_runtime = Some(Runtime::start(setup.clone(), files.root.join("learning.db")).unwrap());
    let (second, second_host, second_context) = fixture(); second_host.caret_selection.set(true);
    second.state.borrow_mut().sink = Some(TestCompositionSink.into());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(until(|| { first.poll_learning(); first.state.borrow().learning.report().initial_snapshot_applied == Some(true) }));
        assert_eq!(first.state.borrow().learning.report().recovery_pending, 2);
        second.state.borrow_mut().learning_runtime = Some(Runtime::start(setup, files.root.join("learning.db")).unwrap());
        assert!(until(|| { second.poll_learning(); second.state.borrow().learning.report().initial_snapshot_applied == Some(true) }));
        // The other service confirms Korean and persists it through its own authenticated child.
        type_choice(&second, &second_context, b"DKSSUD");
        assert!(paired_key(&second, &second_context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
        commit(&second, &second_context); assert_eq!(&*second_host.text.borrow(), "안녕 ");
        assert!(until(|| { second.poll_learning(); second.state.borrow().learning.report().storage.is_some_and(|s| s.committed == 1) }));
        let owner = files.open_store(); let saved = owner.snapshot().unwrap(); drop(owner);
        assert_eq!(saved.entries.len(), 1); assert!(saved.entries[0].korean); assert_eq!(saved.entries[0].english_weight, 0.0);
        assert!(until(|| { first.poll_learning(); first.state.borrow().learning_ready.as_ref().is_none()
            && first.state.borrow().learning.report().snapshots_applied >= 2
            && engine.learning_entries().iter().any(|w| w.raw == "dkssud" && w.korean_weight > 0.0) }));
        let entries = engine.learning_entries(); let word = entries.iter().find(|w| w.raw == "dkssud" && w.context.is_empty()).unwrap();
        assert!(word.korean, "an unpersistable fallback must not override another service's committed choice");
        assert_eq!(word.english_weight, 0.0, "do not replay the stale local choice over the stored decision");
        assert!(entries.iter().any(|w| w.raw == "hello"), "unrelated local fallback must survive");
        let report = first.state.borrow().learning.report();
        assert_eq!(report.recovery_pending, 1); assert_eq!(report.recovery_yielded_to_store, 1);
        assert_eq!(report.recovery_superseded, 0); assert_eq!(report.recovery_invalidated, 0);
        assert_eq!(&*first_host.text.borrow(), &document_before, "a preference update never rewrites document text");
    }));
    for service in [&first, &second] {
        let runtime = service.state.borrow_mut().learning_runtime.take();
        unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
        if let Some(runtime) = runtime { runtime.stop(); assert!(until(|| matches!(runtime.status().phase, Phase::Stopped | Phase::Failed))); }
    }
    if let Err(error) = outcome { std::panic::resume_unwind(error); }
}

#[test]
#[ignore = "requires HANAUTOMATA_TEST_SETUP; pins authority precedence for an existing store at startup"]
fn preconnection_fallback_yields_to_existing_store_and_preserves_its_document() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new();
    let stored = hanautomata_core::WordPreferences::new(); stored.remember_global("dkssud", "dkssud", true);
    drop(Authority::open_seeded(files.root.join("learning.db"), Endpoint::random().unwrap().token(), &stored.export().0).unwrap());
    let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    // Keep a normal English preceding word. F2 here would turn it into Hangul jamo, which do not
    // form the intended context fixture. Only the following DKSSUD is an explicit learned choice.
    for key in b"HELLO" { assert!(paired_key(&service, &context, *key, edit::FieldAccess::Ordinary).as_bool()); }
    commit(&service, &context); assert_eq!(&*host.text.borrow(), "hello ");
    type_choice(&service, &context, b"DKSSUD"); commit(&service, &context);
    let document_before = host.text.borrow().clone();
    let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone();
    assert!(engine.learning_entries().iter().any(|w| w.raw == "dkssud" && !w.context.is_empty()), "startup choice carries a context");
    let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").unwrap());
    service.state.borrow_mut().learning_runtime = Some(Runtime::start(setup, files.root.join("learning.db")).unwrap());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().initial_snapshot_applied == Some(true) }));
        let entries = engine.learning_entries();
        let matching: Vec<_> = entries.iter().filter(|w| w.raw == "dkssud").collect();
        assert_eq!(matching.len(), 1, "no stale contextual overlay may outvote the stored global preference");
        assert!(matching[0].korean); assert_eq!(matching[0].english_weight, 0.0);
        let report = service.state.borrow().learning.report();
        assert_eq!(report.recovery_pending, 0); assert_eq!(report.recovery_yielded_to_store, 1);
        assert_eq!(report.recovery_superseded, 0); assert_eq!(report.recovery_invalidated, 0);
        assert_eq!(&*host.text.borrow(), &document_before);
        let owner = files.open_store(); assert_eq!(owner.snapshot().unwrap().entries.len(), 1, "the startup fallback was not persisted");
    }));
    let runtime = service.state.borrow_mut().learning_runtime.take().unwrap();
    unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    runtime.stop(); assert!(until(|| matches!(runtime.status().phase, Phase::Stopped | Phase::Failed)));
    if let Err(error) = outcome { std::panic::resume_unwind(error); }
}

struct Fixture { root: PathBuf, child: Option<Child>, reader: Option<thread::JoinHandle<()>> }

#[test]
#[ignore = "requires HANAUTOMATA_TEST_SETUP; continuous synthetic input and owned store only"]
fn continuous_words_do_not_starve_an_observed_live_reset() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new(); let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    let legacy = hanautomata_core::WordPreferences::new(); legacy.remember_global("legacy", "legacy", false);
    let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").unwrap());
    service.state.borrow_mut().learning_runtime = Some(Runtime::start_with_legacy(setup, files.root.join("learning.db"), legacy.export().0).unwrap());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning.report().snapshots_applied == 1 }));
        type_choice(&service, &context, b"DKSSUD");
        let mut owner = files.open_store(); owner.manage(&owner.snapshot().unwrap().version, Operation::Reset {}).unwrap(); drop(owner);
        assert!(until(|| { service.poll_learning(); service.state.borrow().learning_ready.as_ref().is_some_and(|r| r.snapshot.version.generation == 2) }));
        commit(&service, &context);
        // No sleeps or artificial idle boundary: poll only through real key callbacks between successive words.
        for _ in 0..2048 {
            type_choice(&service, &context, b"HELLO"); commit(&service, &context);
            if service.state.borrow().learning.report().snapshots_applied > 1 { break; }
        }
        assert!(service.state.borrow().learning.report().snapshots_applied > 1, "continuous input cannot strand the observed Reset");
        let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone();
        assert!(!engine.learning_entries().iter().any(|w| w.raw == "legacy"));
        assert!(host.text.borrow().starts_with("dkssud "));
    }));
    let runtime = service.state.borrow_mut().learning_runtime.take().unwrap();
    unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    runtime.stop(); assert!(until(|| matches!(runtime.status().phase, Phase::Stopped | Phase::Failed)));
    if let Err(error) = outcome { std::panic::resume_unwind(error); }
}

impl Fixture {
    fn open_store(&self) -> Authority {
        let end = Instant::now() + Duration::from_secs(4);
        loop { match Authority::open_existing(self.root.join("learning.db")) {
            Ok(owner) => return owner,
            Err(hanautomata_core::learning::Error::Busy) if Instant::now() < end => thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("owned test store did not become readable: {error:?}"),
        } }
    }
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
#[ignore = "requires HANAUTOMATA_TEST_SETUP pointing to this build's setup executable; owned test store only"]
fn product_worker_connects_real_callbacks_and_restores_the_next_service() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new();
    let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").expect("explicit matching setup build"));
    let wait = |action: &mut dyn FnMut() -> bool| {
        let end = Instant::now() + Duration::from_secs(10);
        while !action() { assert!(Instant::now() < end, "product worker deadline"); thread::sleep(Duration::from_millis(10)); }
    };
    for pass in 0..3 {
        let (service, host, context) = fixture(); host.caret_selection.set(true);
        service.state.borrow_mut().sink = Some(TestCompositionSink.into());
        if pass > 0 {
            // Simulate the old legacy dictionary loaded before the worker. After management Reset it must
            // be replaced by the empty authoritative store, not used to re-seed or keep stale predictions.
            type_choice(&service, &context, b"DKSSUD"); commit(&service, &context);
            assert_eq!(service.state.borrow().shell.as_ref().unwrap().engine().learned_count(), 1);
            // The keys above only construct a legacy fixture; a real new activation starts a fresh Link.
            service.state.borrow_mut().learning = learning::Link::default();
        }
        {
            let mut state = service.state.borrow_mut();
            let legacy = state.shell.as_ref().unwrap().engine().learning_entries();
            state.learning_runtime = Some(Runtime::start_with_legacy(setup.clone(), files.root.join("learning.db"), legacy).unwrap());
        }
        wait(&mut || { service.poll_learning(); service.state.borrow().learning.report().connected });
        assert_eq!(service.state.borrow().learning.report().initial_snapshot_applied, Some(true));
        assert_eq!(service.state.borrow().shell.as_ref().unwrap().engine().learned_count(), if pass == 1 { 1 } else { 0 });
        if pass == 0 {
            type_choice(&service, &context, b"DKSSUD"); commit(&service, &context);
            assert_eq!(&*host.text.borrow(), "dkssud ");
            wait(&mut || { service.poll_learning(); service.state.borrow().learning.report().storage.unwrap().committed == 1 });
            // A refused document edit still must not create another persistent choice.
            type_choice(&service, &context, b"HELLO"); host.fail_set.set(true);
            let _ = paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary);
        }
        if pass == 1 {
            let mut owner = files.open_store();
            let persisted = owner.snapshot().unwrap();
            assert_eq!(persisted.entries.len(), 1);
            assert_eq!(persisted.entries[0].raw, "dkssud");
            assert_eq!(persisted.entries[0].english_weight, 1.0);
            assert_eq!(persisted.entries[0].korean_weight, 0.0);
            owner.manage(&persisted.version, hanautomata_core::learning::Operation::Reset {}).unwrap();
        }
        let runtime = service.state.borrow_mut().learning_runtime.take().unwrap();
        unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
        runtime.stop(); wait(&mut || runtime.status().phase == Phase::Stopped);
    }
    let owner = Authority::open(files.root.join("learning.db"), Endpoint::random().unwrap().token()).unwrap();
    assert!(owner.snapshot().unwrap().entries.is_empty(), "legacy input cannot resurrect managed reset after service reactivation");
    assert!(owner.clients().unwrap().is_empty());
}

#[test]
#[ignore = "requires HANAUTOMATA_TEST_SETUP; owned store and synthetic document contexts only"]
fn abandoned_old_context_cannot_block_a_new_contexts_persistence_connection() {
    use crate::learning_runtime::{Runtime, Phase};
    let files = Fixture::new(); let setup = PathBuf::from(std::env::var_os("HANAUTOMATA_TEST_SETUP").unwrap());
    let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    type_choice(&service, &context, b"DKSSUD"); host.defer.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert!(service.state.borrow().pending_finishes.iter().any(|f| !f.done.get()));
    {
        let mut state = service.state.borrow_mut();
        state.learning_runtime = Some(Runtime::start(setup, files.root.join("learning.db")).unwrap());
    }
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        service.poll_learning();
        if service.state.borrow().learning.report().connected { break; }
        assert!(Instant::now() < end, "old callback cannot prevent queue connection"); thread::sleep(Duration::from_millis(10));
    }
    assert!(service.state.borrow().pending_finishes.iter().any(|f| !f.done.get()));
    let (_, new_host, new_context) = fixture(); new_host.caret_selection.set(true);
    type_choice(&service, &new_context, b"HELLO"); commit(&service, &new_context);
    let runtime = service.state.borrow_mut().learning_runtime.take().unwrap();
    while runtime.status().committed != 1 { assert!(Instant::now() < end); thread::sleep(Duration::from_millis(10)); }
    host.deny.set(true); unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    host.pending.borrow_mut().clear(); runtime.stop();
    while runtime.status().phase != Phase::Stopped { assert!(Instant::now() < end); thread::sleep(Duration::from_millis(10)); }
    let owner = Authority::open(files.root.join("learning.db"), Endpoint::random().unwrap().token()).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries.len(), 1);
    assert_eq!(owner.snapshot().unwrap().entries[0].raw, "hello");
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
