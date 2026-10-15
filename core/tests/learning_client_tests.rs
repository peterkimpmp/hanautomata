use hanautomata_core::{LearningChoice, WordPreferences};
use hanautomata_core::learning::{Authority, Error, Lease, Operation, Request};
use hanautomata_core::learning::client::{channel, Progress, QueueError, MAX_QUEUED_CHOICES};
use std::{fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};

const STORE: &str = "11111111111111111111111111111111";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture { root: PathBuf, file: PathBuf }
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hanautomata-client-test-{}-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&root).unwrap(); Self { file: root.join("learning.db"), root }
    }
    fn open(&self) -> Authority { Authority::open(&self.file, STORE).unwrap() }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("hanautomata-client-test-"));
        for name in ["learning.db", "learning.db.owner.lock"] {
            let path = root.join(name); if path.is_file() { fs::remove_file(path).unwrap(); }
        }
        fs::remove_dir(root).unwrap();
    }
}
fn choice(raw: &str) -> LearningChoice {
    LearningChoice { raw: raw.into(), keys: raw.into(), korean: true, context: String::new() }
}

#[test]
fn rejected_submission_returns_the_original_action_without_restamping_or_double_acceptance() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    let action = producer.begin().unwrap(); let origin = action.origin().clone();
    for _ in 0..MAX_QUEUED_CHOICES { producer.submit(producer.begin().unwrap(), &[choice("filler")]).unwrap(); }
    let (error, action) = producer.submit_recoverable(action, &[choice("hello")]).unwrap_err();
    assert_eq!(error, QueueError::Full); assert_eq!(action.origin(), &origin);
    while let Some(next) = worker.next().unwrap() { worker.finish(&next, owner.apply(next.request())).unwrap(); }
    let reset = owner.manage(&owner.snapshot().unwrap().version, Operation::Reset {}).unwrap(); worker.observe(reset.version).unwrap();
    producer.submit_recoverable(action, &[choice("hello")]).unwrap();
    let old = worker.next().unwrap().unwrap(); assert_eq!(old.request().generation, origin.generation);
    assert_eq!(worker.finish(&old, owner.apply(old.request())).unwrap(), Progress::DroppedStale);
    assert!(owner.snapshot().unwrap().entries.is_empty());
}

fn snapshot_channel(lease: Lease) -> (hanautomata_core::learning::client::Producer, hanautomata_core::learning::client::Worker) {
    struct Signal;
    impl std::task::Wake for Signal { fn wake(self: std::sync::Arc<Self>) {} }
    hanautomata_core::learning::client::channel_for_snapshots(lease, std::task::Waker::from(std::sync::Arc::new(Signal))).unwrap()
}

#[test]
fn snapshot_channel_cannot_learn_before_dictionary_installation() {
    use hanautomata_core::SnapshotInstall;
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, worker) = snapshot_channel(owner.register(ALICE).unwrap());
    assert!(matches!(producer.begin(), Err(QueueError::NeedsRefresh)));
    let ticket = worker.snapshot_ticket().unwrap();
    producer.install_snapshot(&ticket, &owner.snapshot().unwrap().version, || SnapshotInstall::Applied).unwrap();
    assert!(producer.begin().is_ok());
}

#[test]
fn an_observed_deletion_cannot_be_starved_by_new_old_generation_actions() {
    use hanautomata_core::SnapshotInstall;
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = snapshot_channel(owner.register(ALICE).unwrap());
    let before = owner.snapshot().unwrap();
    producer.install_snapshot(&worker.snapshot_ticket().unwrap(), &before.version, || SnapshotInstall::Applied).unwrap();
    let old_word = producer.begin().unwrap();
    let reset = owner.manage(&before.version, Operation::Reset {}).unwrap(); worker.observe(reset.version.clone()).unwrap();
    let ticket = worker.snapshot_ticket().unwrap();
    for _ in 0..32 { let original = producer.begin().unwrap();
        assert_eq!(original.origin(), &before.version);
        assert!(matches!(producer.submit_recoverable(original, &[choice("old")]), Err((QueueError::NeedsRefresh, _))),
            "new words must retain their action without invalidating the pending Reset checkpoint"); }
    producer.install_snapshot(&ticket, &reset.version, || SnapshotInstall::Applied).unwrap();
    producer.submit(old_word, &[choice("old")]).unwrap();
    let old = worker.next().unwrap().unwrap(); assert_eq!(old.request().generation, before.version.generation);
    worker.finish(&old, owner.apply(old.request())).unwrap(); worker.observe(reset.version.clone()).unwrap();
    assert_eq!(producer.begin().unwrap().origin(), &reset.version);
}

#[test]
fn observing_reset_cannot_restamp_old_memory_before_snapshot_installation() {
    use hanautomata_core::SnapshotInstall;
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = snapshot_channel(owner.register(ALICE).unwrap());
    let before = owner.snapshot().unwrap();
    producer.install_snapshot(&worker.snapshot_ticket().unwrap(), &before.version, || SnapshotInstall::Applied).unwrap();
    let old_word = producer.begin().unwrap();
    let reset = owner.manage(&before.version, Operation::Reset {}).unwrap();
    worker.observe(reset.version.clone()).unwrap();
    for result in [SnapshotInstall::Busy, SnapshotInstall::Changed] {
        let ticket = worker.snapshot_ticket().unwrap();
        assert_eq!(producer.install_snapshot(&ticket, &reset.version, || result).unwrap(), result);
        let action = producer.begin().unwrap(); assert_eq!(action.origin(), &before.version);
        assert!(matches!(producer.submit_recoverable(action, &[choice("stale")]), Err((QueueError::NeedsRefresh, _))));
        assert!(worker.next().unwrap().is_none());
    }
    producer.install_snapshot(&worker.snapshot_ticket().unwrap(), &reset.version, || SnapshotInstall::Applied).unwrap();
    producer.submit(old_word, &[choice("late")]).unwrap();
    let late = worker.next().unwrap().unwrap(); assert_eq!(late.request().generation, before.version.generation);
    worker.finish(&late, owner.apply(late.request())).unwrap(); worker.observe(reset.version.clone()).unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("fresh")]).unwrap();
    let fresh = worker.next().unwrap().unwrap(); assert_eq!(fresh.request().generation, reset.version.generation);
    worker.finish(&fresh, owner.apply(fresh.request())).unwrap();
    let entries = owner.snapshot().unwrap().entries; assert_eq!(entries.len(), 1); assert_eq!(entries[0].raw, "fresh");
}

#[test]
fn unrelated_word_during_observed_forget_is_persisted_with_its_original_generation() {
    use hanautomata_core::SnapshotInstall;
    let f = Fixture::new(); let mut owner = f.open(); let (producer, mut worker) = snapshot_channel(owner.register(ALICE).unwrap());
    let before = owner.snapshot().unwrap();
    producer.install_snapshot(&worker.snapshot_ticket().unwrap(), &before.version, || SnapshotInstall::Applied).unwrap();
    let forgotten = owner.manage(&before.version, Operation::Forget { raw: "other".into(), keys: "other".into() }).unwrap();
    worker.observe(forgotten.version.clone()).unwrap(); let ticket = worker.snapshot_ticket().unwrap();
    let action = producer.begin().unwrap(); let (error, action) = producer.submit_recoverable(action, &[choice("hello")]).unwrap_err();
    assert_eq!(error, QueueError::NeedsRefresh); assert_eq!(action.origin(), &before.version);
    producer.install_snapshot(&ticket, &forgotten.version, || SnapshotInstall::Applied).unwrap();
    producer.submit_recoverable(action, &[choice("hello")]).unwrap(); let next = worker.next().unwrap().unwrap();
    assert_eq!(next.request().generation, before.version.generation);
    assert_eq!(worker.finish(&next, owner.apply(next.request())).unwrap(), Progress::Committed);
    assert_eq!(owner.snapshot().unwrap().entries[0].raw, "hello");
}

#[test]
fn snapshot_install_rejects_choices_queued_or_committed_after_capture() {
    use hanautomata_core::SnapshotInstall;
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = snapshot_channel(owner.register(ALICE).unwrap());
    let before = owner.snapshot().unwrap();
    producer.install_snapshot(&worker.snapshot_ticket().unwrap(), &before.version, || SnapshotInstall::Applied).unwrap();
    let old_ticket = worker.snapshot_ticket().unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("local")]).unwrap();
    assert!(worker.snapshot_ticket().is_err());
    let called = std::cell::Cell::new(false);
    assert!(producer.install_snapshot(&old_ticket, &before.version, || { called.set(true); SnapshotInstall::Applied }).is_err());
    let next = worker.next().unwrap().unwrap(); assert!(worker.snapshot_ticket().is_err());
    worker.finish(&next, owner.apply(next.request())).unwrap();
    let latest = owner.snapshot().unwrap();
    assert!(producer.install_snapshot(&old_ticket, &latest.version, || { called.set(true); SnapshotInstall::Applied }).is_err());
    assert!(!called.get(), "an obsolete snapshot must not reach the engine swap");
    producer.install_snapshot(&worker.snapshot_ticket().unwrap(), &latest.version, || SnapshotInstall::Applied).unwrap();
}

#[test]
fn snapshot_install_rejects_foreign_and_regressing_versions_before_engine_swap() {
    use hanautomata_core::SnapshotInstall;
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = snapshot_channel(owner.register(ALICE).unwrap());
    let initial = owner.snapshot().unwrap();
    producer.install_snapshot(&worker.snapshot_ticket().unwrap(), &initial.version, || SnapshotInstall::Applied).unwrap();
    let (_other, other_worker) = snapshot_channel(owner.register(BOB).unwrap());
    let latest = owner.snapshot().unwrap(); worker.observe(latest.version.clone()).unwrap();
    let called = std::cell::Cell::new(false);
    assert!(producer.install_snapshot(&other_worker.snapshot_ticket().unwrap(), &latest.version, || { called.set(true); SnapshotInstall::Applied }).is_err());
    let ticket = worker.snapshot_ticket().unwrap();
    let mut foreign = latest.version.clone(); foreign.store_id = BOB.into();
    let mut invalid = latest.version.clone(); invalid.generation = 0;
    for version in [&initial.version, &foreign, &invalid] {
        assert!(producer.install_snapshot(&ticket, version, || { called.set(true); SnapshotInstall::Applied }).is_err());
    }
    assert!(!called.get());
    drop(worker);
    assert!(producer.install_snapshot(&ticket, &latest.version, || { called.set(true); SnapshotInstall::Applied }).is_err());
    assert!(!called.get());
}
fn management(lease: &Lease, operation: Operation) -> Request {
    Request { store_id: lease.version.store_id.clone(), client_id: lease.client_id.clone(), lease_revision: lease.lease_revision,
        sequence: lease.next_sequence, generation: lease.version.generation, operation }
}

#[test]
fn confirmed_choice_survives_lost_ack_and_storage_owner_restart_once() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    let action = producer.begin().unwrap();
    let preferences = WordPreferences::new(); preferences.defer_learning(true);
    preferences.remember_global("dkssud", "dkssud", true);
    let batch = preferences.take_learning().unwrap();
    assert!(worker.next().unwrap().is_none(), "unconfirmed document writes must not reach storage");
    producer.submit(action, &preferences.confirm_learning(&batch)).unwrap();
    let dispatched = worker.next().unwrap().unwrap();
    let original = serde_json::to_vec(dispatched.request()).unwrap();
    owner.apply(dispatched.request()).unwrap();
    assert_eq!(worker.finish(&dispatched, Err(Error::Io)).unwrap(), Progress::Retry);
    drop(owner); let mut owner = f.open();
    let retry = worker.next().unwrap().unwrap();
    assert_eq!(serde_json::to_vec(retry.request()).unwrap(), original);
    let reply = owner.apply(retry.request()).unwrap(); assert!(reply.duplicate);
    assert_eq!(worker.finish(&retry, Ok(reply)).unwrap(), Progress::Committed);
    assert!(worker.next().unwrap().is_none());
    let words = owner.snapshot().unwrap().entries;
    assert_eq!(words.len(), 1); assert_eq!(words[0].korean_weight, 1.0);
}

#[test]
fn late_document_completion_cannot_restamp_a_choice_after_a_reset() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    let before_reset = producer.begin().unwrap();
    let admin = owner.register(BOB).unwrap();
    owner.apply(&management(&admin, Operation::Reset {})).unwrap();
    worker.observe(owner.snapshot().unwrap().version).unwrap();
    producer.submit(before_reset, &[choice("dkssud")]).unwrap();
    let stale = worker.next().unwrap().unwrap();
    assert_eq!(stale.request().generation, 1);
    assert_eq!(worker.finish(&stale, owner.apply(stale.request())).unwrap(), Progress::DroppedStale);
    assert!(owner.snapshot().unwrap().entries.is_empty());
    worker.observe(owner.snapshot().unwrap().version).unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("hello")]).unwrap();
    let fresh = worker.next().unwrap().unwrap();
    assert_eq!(fresh.request().sequence, 1, "rejected actions do not consume authority sequences");
    assert_eq!(fresh.request().generation, 2);
    worker.finish(&fresh, owner.apply(fresh.request())).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries[0].raw, "hello");
}

#[test]
fn word_deletion_discards_only_stale_choice_without_stranding_other_queued_words() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("dkssud"), choice("hello")]).unwrap();
    let admin = owner.register(BOB).unwrap();
    owner.apply(&management(&admin, Operation::Forget { raw: "dkssud".into(), keys: "dkssud".into() })).unwrap();
    let stale = worker.next().unwrap().unwrap();
    assert_eq!(worker.finish(&stale, owner.apply(stale.request())).unwrap(), Progress::DroppedStale);
    assert!(matches!(producer.begin(), Err(QueueError::NeedsRefresh)));
    worker.observe(owner.snapshot().unwrap().version).unwrap();
    let unaffected = worker.next().unwrap().unwrap();
    assert_eq!(unaffected.request().sequence, 1); assert_eq!(unaffected.request().generation, 1);
    worker.finish(&unaffected, owner.apply(unaffected.request())).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries[0].raw, "hello");
}

#[test]
fn queue_overflow_and_invalid_batch_are_atomic_and_leave_no_sequence_holes() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    for _ in 0..MAX_QUEUED_CHOICES - 1 { producer.submit(producer.begin().unwrap(), &[choice("hello")]).unwrap(); }
    assert_eq!(producer.submit(producer.begin().unwrap(), &[choice("good"), choice("bye")]), Err(QueueError::Full));
    let first = worker.next().unwrap().unwrap();
    assert_eq!(producer.submit(producer.begin().unwrap(), &[choice("good"), choice("bye")]), Err(QueueError::Full), "in-flight memory counts toward the bound");
    worker.finish(&first, owner.apply(first.request())).unwrap();
    let mut invalid = choice("secret"); invalid.context = "plain surrounding document text".into();
    assert_eq!(producer.submit(producer.begin().unwrap(), &[choice("valid"), invalid]), Err(QueueError::InvalidChoice));
    producer.submit(producer.begin().unwrap(), &[choice("new")]).unwrap();
    let mut sequence = 2;
    while let Some(next) = worker.next().unwrap() {
        assert_eq!(next.request().sequence, sequence); sequence += 1;
        worker.finish(&next, owner.apply(next.request())).unwrap();
    }
    assert_eq!(sequence as usize, MAX_QUEUED_CHOICES + 1);
    assert!(!owner.snapshot().unwrap().entries.iter().any(|w| w.raw == "valid" || w.raw == "good"));
}

#[test]
fn pause_invalidates_unsubmitted_actions_but_does_not_replay_confirmed_work() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    let unconfirmed = producer.begin().unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("hello")]).unwrap();
    producer.set_recording(false);
    assert!(matches!(producer.begin(), Err(QueueError::Paused)));
    producer.set_recording(true);
    assert_eq!(producer.submit(unconfirmed, &[choice("secret")]), Err(QueueError::Paused));
    let confirmed = worker.next().unwrap().unwrap();
    worker.finish(&confirmed, owner.apply(confirmed.request())).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries.len(), 1);
}

#[test]
fn wrong_or_late_acknowledgement_cannot_advance_the_queue() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("hello"), choice("world")]).unwrap();
    let first = worker.next().unwrap().unwrap();
    let valid = owner.apply(first.request()).unwrap();
    for mode in 0..4 {
        let mut bad = valid.clone();
        match mode { 0 => bad.sequence += 1, 1 => bad.version.store_id = BOB.into(),
            2 => bad.committed_revision = 0, _ => bad.version.generation = 0 }
        assert_eq!(worker.finish(&first, Ok(bad)), Err(QueueError::InvalidReply));
        assert_eq!(worker.next().unwrap().unwrap().request().sequence, 1);
    }
    worker.finish(&first, Ok(valid.clone())).unwrap();
    let second = worker.next().unwrap().unwrap();
    assert_eq!(worker.finish(&first, Ok(valid)), Err(QueueError::StaleDispatch));
    worker.finish(&second, owner.apply(second.request())).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries.len(), 2);
}

#[test]
fn delayed_success_cannot_roll_back_a_newer_observed_reset_generation() {
    let f = Fixture::new(); let mut owner = f.open();
    let (producer, mut worker) = channel(owner.register(ALICE).unwrap()).unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("hello")]).unwrap();
    let first = worker.next().unwrap().unwrap(); let delayed_reply = owner.apply(first.request()).unwrap();
    let admin = owner.register(BOB).unwrap(); owner.apply(&management(&admin, Operation::Reset {})).unwrap();
    worker.observe(owner.snapshot().unwrap().version).unwrap();
    worker.finish(&first, Ok(delayed_reply)).unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("world")]).unwrap();
    let next = worker.next().unwrap().unwrap(); assert_eq!(next.request().generation, 2);
    worker.finish(&next, owner.apply(next.request())).unwrap();
    let words = owner.snapshot().unwrap().entries; assert_eq!(words.len(), 1); assert_eq!(words[0].raw, "world");
}

#[test]
fn retired_connection_closes_without_transferring_old_actions_to_a_new_lease() {
    let f = Fixture::new(); let mut owner = f.open();
    let lease = owner.register(ALICE).unwrap();
    let (producer, mut worker) = channel(lease.clone()).unwrap();
    producer.submit(producer.begin().unwrap(), &[choice("hello")]).unwrap();
    let old = worker.next().unwrap().unwrap();
    owner.retire(ALICE, lease.lease_revision).unwrap();
    let replacement = owner.register(ALICE).unwrap();
    assert_ne!(replacement.lease_revision, lease.lease_revision);
    assert_eq!(worker.finish(&old, owner.apply(old.request())).unwrap(), Progress::Stopped);
    assert!(matches!(producer.begin(), Err(QueueError::Closed)));
    let (_, mut new_worker) = channel(replacement).unwrap();
    assert!(new_worker.next().unwrap().is_none());
    assert!(owner.snapshot().unwrap().entries.is_empty());
}

#[test]
fn foreign_action_and_worker_shutdown_cannot_accept_learning() {
    let f = Fixture::new(); let mut owner = f.open();
    let (a, aw) = channel(owner.register(ALICE).unwrap()).unwrap();
    let (b, _bw) = channel(owner.register(BOB).unwrap()).unwrap();
    assert_eq!(b.submit(a.begin().unwrap(), &[choice("secret")]), Err(QueueError::ForeignAction));
    drop(aw); assert!(matches!(a.begin(), Err(QueueError::Closed)));
}
