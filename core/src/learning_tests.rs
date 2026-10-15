use super::*;
use std::{fs, time::{SystemTime, UNIX_EPOCH}};
const STORE: &str = "11111111111111111111111111111111";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture { root: PathBuf, path: PathBuf }
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hanautomata-learning-test-{}-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&root).unwrap();
        Self { path: root.join("learning.db"), root }
    }
    fn open(&self) -> Authority {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            match Authority::open(&self.path, STORE) {
                Err(Error::Busy) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(10)),
                result => return result.unwrap(),
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let base = std::env::temp_dir().canonicalize().unwrap();
        let root = self.root.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(base.as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("hanautomata-learning-test-"));
        for item in fs::read_dir(&root).unwrap().flatten() {
            if item.file_type().unwrap().is_file() { fs::remove_file(item.path()).unwrap(); }
        }
        fs::remove_dir(&root).unwrap();
    }
}
fn request(lease: &Lease, operation: Operation) -> Request {
    Request { store_id: lease.version.store_id.clone(), client_id: lease.client_id.clone(), lease_revision: lease.lease_revision,
        sequence: lease.next_sequence, generation: lease.version.generation, operation }
}
fn learn(lease: &Lease, raw: &str, korean: bool) -> Request {
    request(lease, Operation::Learn { raw: raw.into(), keys: raw.into(), korean, context: String::new() })
}

#[test]
fn existing_open_never_initializes_a_missing_directory_database_or_owner_marker() {
    let fixture = Fixture::new();
    let nested = fixture.root.join("missing").join("learning.db");
    let result = Authority::open_existing(&nested);
    let outcome = result.as_ref().err().copied(); drop(result);
    let incorrectly_created = nested.parent().unwrap().exists();
    if incorrectly_created {
        // Keep a failing assertion from triggering a second panic in the flat fixture's Drop.
        for file in [&nested, &suffix(&nested, ".owner.lock")] { if file.exists() { fs::remove_file(file).unwrap(); } }
        fs::remove_dir(nested.parent().unwrap()).unwrap();
    }
    assert_eq!(outcome, Some(Error::MissingState)); assert!(!incorrectly_created);
    assert_eq!(Authority::open_existing(&fixture.path).unwrap_err(), Error::MissingState);
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
    drop(fixture.open()); let data = fs::read(&fixture.path).unwrap();
    fs::remove_file(suffix(&fixture.path, ".owner.lock")).unwrap();
    assert_eq!(Authority::open_existing(&fixture.path).unwrap_err(), Error::MissingState);
    assert_eq!(fs::read(&fixture.path).unwrap(), data);
    assert!(!suffix(&fixture.path, ".owner.lock").exists());
}

#[test]
fn existing_open_preserves_pending_files_and_rejects_an_unowned_database() {
    let fixture = Fixture::new(); drop(fixture.open());
    let pending = suffix(&fixture.path, &format!(".pending-{STORE}-1-123-1")); fs::write(&pending, b"recovery evidence").unwrap();
    let bytes = fs::read(&fixture.path).unwrap(); let marker = fs::read(suffix(&fixture.path, ".owner.lock")).unwrap();
    drop(Authority::open_existing(&fixture.path).unwrap());
    assert_eq!(fs::read(&pending).unwrap(), b"recovery evidence");
    assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    assert_eq!(fs::read(suffix(&fixture.path, ".owner.lock")).unwrap(), marker);
    fs::write(suffix(&fixture.path, ".owner.lock"), []).unwrap();
    assert_eq!(Authority::open_existing(&fixture.path).unwrap_err(), Error::InvalidState);
    assert_eq!(fs::read(suffix(&fixture.path, ".owner.lock")).unwrap(), b"");
    assert_eq!(fs::read(&pending).unwrap(), b"recovery evidence");
}

fn legacy_seed() -> Vec<LearnedWord> {
    let prefs = WordPreferences::new(); prefs.remember_global("legacy", "legacy", true); prefs.export().0
}

#[test]
fn seeded_open_imports_once_and_cannot_resurrect_entries_after_management_reset() {
    let fixture = Fixture::new(); let seed = legacy_seed();
    let mut owner = Authority::open_seeded(&fixture.path, STORE, &seed).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries, seed);
    let version = owner.snapshot().unwrap().version;
    let reset = owner.manage(&version, Operation::Reset {}).unwrap(); assert!(reset.entries.is_empty()); drop(owner);
    let reopened = Authority::open_seeded(&fixture.path, STORE, &seed).unwrap();
    assert!(reopened.snapshot().unwrap().entries.is_empty()); assert_eq!(reopened.snapshot().unwrap().version, reset.version);
}

#[test]
fn seeded_open_rejects_invalid_seed_without_creating_database_and_ignores_it_for_existing_data() {
    let fixture = Fixture::new(); let mut invalid = legacy_seed(); invalid[0].korean_weight = -1.0;
    assert_eq!(Authority::open_seeded(&fixture.path, STORE, &invalid).unwrap_err(), Error::InvalidState);
    assert!(!fixture.path.exists());
    let seed = legacy_seed(); drop(Authority::open_seeded(&fixture.path, STORE, &seed).unwrap());
    let existing = Authority::open_seeded(&fixture.path, STORE, &invalid).unwrap();
    assert_eq!(existing.snapshot().unwrap().entries, seed);
}

#[test]
fn management_requires_exact_snapshot_and_never_accepts_input_learning() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let version = owner.snapshot().unwrap().version; let lease = owner.register(ALICE).unwrap();
    let before = fs::read(&fixture.path).unwrap();
    assert_eq!(owner.manage(&version, Operation::Reset {}).unwrap_err(), Error::ExternalChange);
    let version = owner.snapshot().unwrap().version;
    assert_eq!(owner.manage(&version, learn(&lease, "hello", true).operation).unwrap_err(), Error::InvalidRequest);
    assert_eq!(owner.manage(&version, Operation::Forget { raw: "password 123".into(), keys: "password 123".into() }).unwrap_err(), Error::InvalidRequest);
    assert_eq!(fs::read(&fixture.path).unwrap(), before);
}

#[test]
fn management_forget_and_reset_fence_old_queues_without_changing_their_receipts() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
    owner.apply(&learn(&a, "hello", true)).unwrap();
    let before_clients = serde_json::to_vec(&owner.clients().unwrap()).unwrap();
    let before = owner.snapshot().unwrap().version;
    let deleted = owner.manage(&before, Operation::Forget { raw: "hello".into(), keys: "hello".into() }).unwrap();
    assert!(deleted.entries.is_empty()); assert_eq!(deleted.version.revision, before.revision + 1);
    assert_eq!(serde_json::to_vec(&owner.clients().unwrap()).unwrap(), before_clients);
    assert_eq!(owner.apply(&learn(&b, "hello", true)).unwrap_err(), Error::StaleGeneration);
    owner.apply(&learn(&b, "unrelated", false)).unwrap();
    let fresh = owner.register(ALICE).unwrap();
    let reset = owner.manage(&owner.snapshot().unwrap().version, Operation::Reset {}).unwrap();
    assert!(reset.entries.is_empty());
    assert_eq!(owner.apply(&learn(&fresh, "different", true)).unwrap_err(), Error::StaleGeneration);
    assert_eq!(owner.manage(&before, Operation::Reset {}).unwrap_err(), Error::StaleGeneration);
}

#[test]
fn managed_lease_format_is_versioned_and_returns_to_legacy_after_drain() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let old = owner.register(ALICE).unwrap();
    let managed = owner.register_with_lifetime(BOB, LeaseLifetime::WindowsMutexV1).unwrap();
    owner.apply(&learn(&managed, "retained", true)).unwrap();
    let words = owner.snapshot().unwrap().entries;
    let managed_bytes = fs::read(&fixture.path).unwrap();
    assert!(managed_bytes.starts_with(b"Hanautomata.TsfLearning/2\n"), "a strict legacy reader must see an explicit format change");
    drop(owner); let mut owner = fixture.open();
    owner.retire(BOB, managed.lease_revision).unwrap();
    let bytes = fs::read(&fixture.path).unwrap();
    assert!(bytes.starts_with(b"Hanautomata.TsfLearning/1\n"));
    assert!(!String::from_utf8(bytes).unwrap().contains("lifetime"));
    assert_eq!(owner.snapshot().unwrap().entries, words);
    assert_eq!(owner.clients().unwrap()[0].key.client_id, ALICE);
    // Existing unmanaged sessions and their monotonic identities survive a downgrade.
    owner.apply(&learn(&old, "after", false)).unwrap();
    assert_eq!(owner.snapshot().unwrap().entries.len(), 2);
}

#[test]
fn managed_metadata_under_a_legacy_header_is_rejected_without_overwrite() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    owner.register_with_lifetime(BOB, LeaseLifetime::WindowsMutexV1).unwrap(); drop(owner);
    let bytes = fs::read(&fixture.path).unwrap();
    let newline = bytes.iter().position(|b| *b == b'\n').unwrap();
    let mut misleading = b"Hanautomata.TsfLearning/1\n".to_vec(); misleading.extend_from_slice(&bytes[newline + 1..]);
    fs::write(&fixture.path, &misleading).unwrap();
    assert_eq!(Authority::open(&fixture.path, STORE).unwrap_err(), Error::InvalidState);
    assert_eq!(fs::read(&fixture.path).unwrap(), misleading);
}

#[test]
fn lifetime_policy_survives_reopen_and_apply_and_cannot_reclassify_an_existing_lease() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    owner.register(ALICE).unwrap();
    assert!(!String::from_utf8(fs::read(&fixture.path).unwrap()).unwrap().contains("lifetime"), "older unmanaged records retain their encoding");
    let managed = owner.register_with_lifetime(BOB, LeaseLifetime::WindowsMutexV1).unwrap();
    owner.apply(&learn(&managed, "hello", true)).unwrap(); drop(owner);
    let mut owner = fixture.open(); let clients = owner.clients().unwrap();
    assert_eq!(clients[0].lifetime, LeaseLifetime::Unmanaged);
    assert_eq!(clients[1].lifetime, LeaseLifetime::WindowsMutexV1);
    let disk = fs::read(&fixture.path).unwrap();
    assert_eq!(owner.register(BOB).unwrap_err(), Error::InvalidRequest);
    assert_eq!(owner.register_with_lifetime(ALICE, LeaseLifetime::WindowsMutexV1).unwrap_err(), Error::InvalidRequest);
    let retry = owner.register_with_lifetime(BOB, LeaseLifetime::WindowsMutexV1).unwrap();
    assert_eq!(retry.lease_revision, managed.lease_revision); assert_eq!(retry.next_sequence, 2);
    assert_eq!(fs::read(&fixture.path).unwrap(), disk, "retries and refused policy changes must not write");
}

#[test]
fn independent_clients_contribute_without_stale_snapshot_overwrites() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
    owner.apply(&learn(&a, "hello", true)).unwrap();
    owner.apply(&learn(&b, "hello", false)).unwrap();
    let words = owner.snapshot().unwrap().entries;
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].korean_weight, 0.85);
    assert_eq!(words[0].english_weight, 1.0);
    drop(owner);
    assert_eq!(fixture.open().snapshot().unwrap().entries, words);
}

#[test]
fn lost_ack_retry_is_idempotent_even_after_restarting_the_owner() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let command = learn(&a, "hello", true);
    let applied = owner.apply(&command).unwrap(); let disk = fs::read(&fixture.path).unwrap();
    drop(owner); let mut restarted = fixture.open();
    let duplicate = restarted.apply(&command).unwrap();
    assert!(duplicate.duplicate); assert_eq!(duplicate.committed_revision, applied.committed_revision);
    assert_eq!(restarted.snapshot().unwrap().entries[0].korean_weight, 1.0);
    assert_eq!(fs::read(&fixture.path).unwrap(), disk);
    let mut altered = command; altered.operation = Operation::Reset {};
    assert_eq!(restarted.apply(&altered).unwrap_err(), Error::SequenceConflict);
}

#[test]
fn reset_fences_queued_learning_and_its_own_retry_does_not_reset_again() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
    let reset = request(&b, Operation::Reset {});
    let reply = owner.apply(&reset).unwrap(); assert!(!reply.changed);
    assert_eq!(owner.apply(&learn(&a, "hello", true)).unwrap_err(), Error::StaleGeneration);
    assert!(owner.apply(&reset).unwrap().duplicate);
    assert_eq!(owner.snapshot().unwrap().version.generation, reply.version.generation);
    let mut fresh = a; fresh.version = reply.version;
    owner.apply(&learn(&fresh, "world", true)).unwrap();
}

#[test]
fn forget_fences_a_word_even_when_it_has_not_arrived_yet() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
    let reply = owner.apply(&request(&b, Operation::Forget { raw: "hello".into(), keys: "hello".into() })).unwrap();
    assert!(!reply.changed);
    assert_eq!(owner.apply(&learn(&a, "hello", true)).unwrap_err(), Error::StaleGeneration);
    assert!(owner.snapshot().unwrap().entries.is_empty());
}

#[test]
fn out_of_order_unknown_client_and_wrong_store_requests_do_not_write() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let initial = fs::read(&fixture.path).unwrap();
    let mut command = learn(&a, "hello", true); command.sequence = 2;
    assert_eq!(owner.apply(&command).unwrap_err(), Error::OutOfOrder);
    command.sequence = 1; command.client_id = BOB.into();
    assert_eq!(owner.apply(&command).unwrap_err(), Error::UnknownClient);
    command.client_id = ALICE.into(); command.store_id = BOB.into();
    assert_eq!(owner.apply(&command).unwrap_err(), Error::WrongStore);
    assert_eq!(fs::read(&fixture.path).unwrap(), initial);
}

#[test]
fn retired_client_cannot_reintroduce_old_operations() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap();
    assert!(owner.retire(ALICE, a.lease_revision).unwrap()); assert!(!owner.retire(ALICE, a.lease_revision).unwrap());
    assert_eq!(owner.apply(&learn(&a, "hello", true)).unwrap_err(), Error::UnknownClient);
    let reused = owner.register(ALICE).unwrap();
    assert_ne!(reused.lease_revision, a.lease_revision);
    assert_eq!(owner.apply(&learn(&a, "hello", true)).unwrap_err(), Error::ExpiredLease);
    assert_eq!(owner.retire(ALICE, a.lease_revision).unwrap_err(), Error::ExpiredLease);
}

#[test]
fn second_owner_is_rejected_and_drop_releases_the_file_lock() {
    let fixture = Fixture::new(); let owner = fixture.open();
    assert_eq!(Authority::open(&fixture.path, STORE).unwrap_err(), Error::Busy);
    drop(owner); fixture.open();
}

#[test]
fn malformed_or_legacy_files_are_preserved_and_missing_committed_file_is_not_reset() {
    for bytes in [b"broken".as_slice(), br#"{"Format":2,"Entries":[]}"#] {
        let fixture = Fixture::new(); fs::write(&fixture.path, bytes).unwrap();
        assert_eq!(Authority::open(&fixture.path, STORE).unwrap_err(), Error::InvalidState);
        assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    }
    let fixture = Fixture::new(); drop(fixture.open()); fs::remove_file(&fixture.path).unwrap();
    assert_eq!(Authority::open(&fixture.path, BOB).unwrap_err(), Error::MissingState);
    assert!(!fixture.path.exists());
}

#[test]
fn invalid_context_and_oversized_wire_requests_never_become_learning() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let mut command = learn(&a, "hello", true);
    if let Operation::Learn { context, .. } = &mut command.operation { *context = "typed-private-text".into(); }
    let error = owner.apply(&command).unwrap_err(); assert_eq!(error, Error::InvalidRequest);
    assert!(!error.to_string().contains("typed-private-text"));
    assert!(Request::decode(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
    let valid = serde_json::to_vec(&learn(&a, "hello", true)).unwrap();
    assert!(Request::decode(&valid).is_ok());
    assert!(owner.snapshot().unwrap().entries.is_empty());
}

#[test]
fn reset_with_unrecognized_fields_is_rejected_instead_of_ignoring_them() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    let mut value = serde_json::to_value(learn(&a, "hello", true)).unwrap();
    value["operation"] = serde_json::json!({"operation": "reset", "unexpected": true});
    assert!(Request::decode(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn duplicate_from_before_another_clients_reset_cannot_restore_erased_words() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
    let choice = learn(&a, "hello", true); owner.apply(&choice).unwrap();
    let reset = owner.apply(&request(&b, Operation::Reset {})).unwrap();
    let replay = owner.apply(&choice).unwrap();
    assert!(replay.duplicate); assert_eq!(replay.version, reset.version);
    assert!(owner.snapshot().unwrap().entries.is_empty());
}

#[test]
fn forget_removes_global_and_contextual_learning_as_one_fence() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
    let mut choice = learn(&a, "hello", true);
    if let Operation::Learn { context, .. } = &mut choice.operation { *context = crate::context_key("안녕"); }
    owner.apply(&choice).unwrap(); assert_eq!(owner.snapshot().unwrap().entries.len(), 2);
    owner.apply(&request(&b, Operation::Forget { raw: "hello".into(), keys: "hello".into() })).unwrap();
    assert!(owner.snapshot().unwrap().entries.is_empty());
}

#[test]
fn storage_failure_faults_the_owner_and_reopen_resolves_ambiguous_commit() {
    for stage in [Stage::BeforeWrite, Stage::AfterSync, Stage::AfterReplace] {
        let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
        let choice = learn(&a, "hello", true);
        owner.injection = Some((stage, false));
        assert_eq!(owner.apply(&choice).unwrap_err(), Error::Io);
        assert_eq!(owner.snapshot().unwrap_err(), Error::Faulted);
        assert_eq!(owner.apply(&choice).unwrap_err(), Error::Faulted);
        drop(owner);
        let mut restarted = fixture.open(); let reply = restarted.apply(&choice).unwrap();
        assert_eq!(reply.duplicate, stage == Stage::AfterReplace);
        assert_eq!(restarted.snapshot().unwrap().entries[0].korean_weight, 1.0);
    }
}

#[test]
fn external_edits_are_preserved_instead_of_overwritten_by_cached_state() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    fs::write(&fixture.path, b"externally edited").unwrap();
    assert_eq!(owner.apply(&learn(&a, "hello", true)).unwrap_err(), Error::ExternalChange);
    assert_eq!(fs::read(&fixture.path).unwrap(), b"externally edited");
    assert_eq!(owner.snapshot().unwrap_err(), Error::Faulted);
}

#[test]
fn checksum_and_semantic_corruption_are_rejected_without_sanitizing_the_file() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    owner.apply(&learn(&a, "hello", true)).unwrap();
    let state = owner.state.clone(); drop(owner);
    for invalid in 0..3 {
        let mut value = serde_json::to_value(&state).unwrap();
        match invalid { 0 => value["version"]["generation"] = serde_json::json!(0),
            1 => value["entries"][0]["korean_weight"] = serde_json::json!(-1),
            _ => value["clients"][0]["committed_revision"] = serde_json::json!(999999), }
        let body = serde_json::to_vec(&value).unwrap();
        let mut bytes = PREFIX.to_vec(); bytes.extend_from_slice(hex(&Sha256::digest(&body)).as_bytes());
        bytes.push(b'\n'); bytes.extend_from_slice(&body); fs::write(&fixture.path, &bytes).unwrap();
        assert_eq!(Authority::open(&fixture.path, STORE).unwrap_err(), Error::InvalidState);
        assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    }
    let mut bad_checksum = encode_state(&state).unwrap();
    let last = bad_checksum.len() - 1; bad_checksum[last] ^= 1; fs::write(&fixture.path, &bad_checksum).unwrap();
    assert_eq!(Authority::open(&fixture.path, STORE).unwrap_err(), Error::InvalidState);
    fs::write(&fixture.path, vec![b' '; MAX_STATE_BYTES + 1]).unwrap();
    assert_eq!(Authority::open(&fixture.path, STORE).unwrap_err(), Error::InvalidState);
}

#[test]
fn client_capacity_never_evicts_a_live_lease_and_retirement_allows_a_new_one() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let first = owner.register(ALICE).unwrap();
    for n in 1..MAX_CLIENTS { owner.register(&format!("{n:032x}")).unwrap(); }
    assert_eq!(owner.register(BOB).unwrap_err(), Error::TooManyClients);
    owner.apply(&learn(&first, "hello", true)).unwrap();
    owner.retire(ALICE, first.lease_revision).unwrap();
    owner.register(BOB).unwrap();
    assert_eq!(owner.apply(&learn(&first, "hello", true)).unwrap_err(), Error::UnknownClient);
}

#[test]
fn concurrent_deliveries_through_one_owner_keep_every_choice_once() {
    use std::sync::{Arc, Barrier, Mutex};
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let commands: Vec<_> = (1..=16).map(|n| {
        let lease = owner.register(&format!("{n:032x}")).unwrap();
        learn(&lease, &format!("word{}", char::from(b'a' + n as u8)), true)
    }).collect();
    let owner = Arc::new(Mutex::new(owner)); let ready = Arc::new(Barrier::new(32));
    let mut threads = Vec::new();
    for command in commands.iter().chain(commands.iter()) {
        let command = command.clone(); let owner = owner.clone(); let ready = ready.clone();
        threads.push(std::thread::spawn(move || { ready.wait(); owner.lock().unwrap().apply(&command).unwrap() }));
    }
    let replies: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(replies.iter().filter(|r| r.duplicate).count(), 16);
    let owner = Arc::try_unwrap(owner).unwrap().into_inner().unwrap();
    let words = owner.snapshot().unwrap().entries;
    assert_eq!(words.len(), 16); assert!(words.iter().all(|w| w.korean_weight == 1.0));
}

#[test]
#[ignore = "child-only fixture, executed by process_interruption_recovers_before_and_after_replace"]
fn crash_child() {
    let Some(root) = std::env::var_os("HANAUTOMATA_LEARNING_CHILD_ROOT") else { return; };
    let root = PathBuf::from(root);
    let canonical = root.canonicalize().unwrap();
    assert_eq!(canonical.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
    assert!(canonical.file_name().unwrap().to_string_lossy().starts_with("hanautomata-learning-test-"));
    let stage = match std::env::var("HANAUTOMATA_LEARNING_CHILD_STAGE").unwrap().as_str() {
        "sync" => Stage::AfterSync, "replace" => Stage::AfterReplace, _ => panic!("invalid fixture stage"),
    };
    let mut owner = Authority::open(root.join("learning.db"), STORE).unwrap();
    let request = Request::decode(&fs::read(root.join("request.json")).unwrap()).unwrap();
    owner.injection = Some((stage, true)); owner.apply(&request).unwrap();
    panic!("the owned child did not exit at its injection point");
}

#[test]
fn process_interruption_recovers_before_and_after_replace() {
    use std::{process::{Command, Stdio}, time::{Duration, Instant}};
    for stage in ["sync", "replace"] {
        let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
        let command = learn(&a, "hello", true); drop(owner);
        fs::write(fixture.root.join("request.json"), serde_json::to_vec(&command).unwrap()).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--ignored", "--exact", "learning::tests::crash_child"])
            .env("HANAUTOMATA_LEARNING_CHILD_ROOT", &fixture.root).env("HANAUTOMATA_LEARNING_CHILD_STAGE", stage)
            .stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)] { use std::os::windows::process::CommandExt; child.creation_flags(0x08000000); }
        let mut child = child.spawn().unwrap(); let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() { break status; }
            if Instant::now() >= deadline { child.kill().unwrap(); child.wait().unwrap(); panic!("owned child timeout"); }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(91));
        let mut restarted = fixture.open(); let reply = restarted.apply(&command).unwrap();
        assert_eq!(reply.duplicate, stage == "replace");
        assert_eq!(restarted.snapshot().unwrap().entries[0].korean_weight, 1.0);
    }
}

#[test]
fn registration_retry_after_uncertain_commit_recovers_the_existing_lease() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    owner.injection = Some((Stage::AfterReplace, false));
    assert_eq!(owner.register(ALICE).unwrap_err(), Error::Io); drop(owner);
    let mut restarted = fixture.open(); let bytes = fs::read(&fixture.path).unwrap();
    let recovered = restarted.register(ALICE).unwrap();
    assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    restarted.apply(&learn(&recovered, "hello", true)).unwrap();
    assert_eq!(restarted.register(ALICE).unwrap().next_sequence, 2);
}

#[test]
fn explicit_reconciliation_recovers_stranded_leases_without_erasing_words() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); owner.register(BOB).unwrap(); owner.apply(&learn(&a, "hello", true)).unwrap();
    drop(owner); let mut restarted = fixture.open();
    let live = LeaseKey { client_id: ALICE.into(), lease_revision: a.lease_revision };
    assert_eq!(restarted.clients().unwrap().len(), 2);
    assert_eq!(restarted.retire_absent(&[live.clone()]).unwrap(), 1);
    assert_eq!(restarted.retire_absent(&[live]).unwrap(), 0);
    assert_eq!(restarted.clients().unwrap().len(), 1);
    assert_eq!(restarted.snapshot().unwrap().entries[0].raw, "hello");
    assert!(restarted.apply(&learn(&a, "hello", true)).unwrap().duplicate);
}

#[test]
fn forgetting_one_word_preserves_queued_choices_for_other_words() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
    owner.apply(&request(&b, Operation::Forget { raw: "hello".into(), keys: "hello".into() })).unwrap();
    owner.apply(&learn(&a, "world", true)).unwrap();
    let mut old = learn(&a, "hello", true); old.sequence = 2;
    assert_eq!(owner.apply(&old).unwrap_err(), Error::StaleGeneration);
    assert_eq!(owner.snapshot().unwrap().entries[0].raw, "world");
}

#[test]
fn repeated_interrupted_writes_keep_bounded_pending_files_and_preserve_unrelated_files() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap(); drop(owner);
    let unrelated = fixture.root.join("learning.db.pending-unrelated"); fs::write(&unrelated, b"keep").unwrap();
    for _ in 0..8 {
        let mut owner = fixture.open(); owner.injection = Some((Stage::AfterSync, false));
        assert_eq!(owner.apply(&learn(&a, "hello", true)).unwrap_err(), Error::Io);
    }
    let owner = fixture.open();
    let count = fs::read_dir(&fixture.root).unwrap().flatten().filter(|entry| {
        entry.file_name().to_string_lossy().starts_with("learning.db.pending-") && entry.path() != unrelated
    }).count();
    assert_eq!(count, 0, "abandoned files must not retain forgotten text");
    assert_eq!(fs::read(unrelated).unwrap(), b"keep");
    assert!(owner.snapshot().unwrap().entries.is_empty());
}

#[cfg(windows)]
#[test]
fn a_live_owner_lock_cannot_be_deleted_to_create_a_second_owner() {
    let fixture = Fixture::new(); let owner = fixture.open();
    let deletion = fs::remove_file(suffix(&fixture.path, ".owner.lock"));
    assert!(deletion.is_err(), "the live ownership marker was deleted");
    assert_eq!(Authority::open(&fixture.path, STORE).unwrap_err(), Error::Busy);
    drop(owner); fixture.open();
}

#[cfg(windows)]
#[test]
fn a_brief_scanner_open_does_not_fault_a_valid_transaction() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    let held = OpenOptions::new().read(true).share_mode(1).open(&fixture.path).unwrap();
    let release = release_after_retry(held);
    let result = owner.apply(&learn(&a, "hello", true));
    let observed = release.join().unwrap(); RETRY_OBSERVER.with(|observer| *observer.borrow_mut() = None);
    assert!(observed, "the test did not reach an I/O retry");
    assert!(result.is_ok(), "{result:?}");
}

#[cfg(windows)]
fn release_after_retry(held: File) -> std::thread::JoinHandle<bool> {
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    RETRY_OBSERVER.with(|observer| *observer.borrow_mut() = Some(count.clone()));
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while count.load(Ordering::Relaxed) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let observed = count.load(Ordering::Relaxed) != 0; drop(held); observed
    })
}

#[cfg(windows)]
#[test]
fn a_brief_exclusive_file_open_retries_the_read_before_committing() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    let held = OpenOptions::new().write(true).share_mode(0).open(&fixture.path).unwrap();
    assert!(File::open(&fixture.path).is_err());
    let release = release_after_retry(held);
    let result = owner.apply(&learn(&a, "hello", true));
    let observed = release.join().unwrap(); RETRY_OBSERVER.with(|observer| *observer.borrow_mut() = None);
    assert!(observed, "the test did not reach a read retry");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn learning_weights_round_trip_exactly_across_serialization() {
    let mut weight = 0.0_f64;
    let mut choices = 0x1234567890abcdef_u64;
    for _ in 0..20000 {
        choices = choices.wrapping_mul(6364136223846793005).wrapping_add(1);
        weight = weight * 0.85 + if choices >> 63 == 0 { 1.0 } else { 0.0 };
        let text = serde_json::to_string(&weight).unwrap(); let restored: f64 = serde_json::from_str(&text).unwrap();
        assert_eq!(restored.to_bits(), weight.to_bits(), "{text}");
    }
}

#[test]
fn truncated_fence_history_and_resets_never_admit_unproven_old_choices() {
    let fixture = Fixture::new(); let mut owner = fixture.open();
    let a = owner.register(ALICE).unwrap(); let mut b = owner.register(BOB).unwrap();
    for _ in 0..=MAX_FENCES {
        owner.apply(&request(&b, Operation::Forget { raw: "hello".into(), keys: "hello".into() })).unwrap();
        b = owner.register(BOB).unwrap();
    }
    assert_eq!(owner.apply(&learn(&a, "world", true)).unwrap_err(), Error::StaleGeneration);
    let current_a = owner.register(ALICE).unwrap();
    owner.apply(&request(&b, Operation::Reset {})).unwrap();
    assert_eq!(owner.apply(&learn(&current_a, "world", true)).unwrap_err(), Error::StaleGeneration);
    drop(owner); assert!(fixture.open().snapshot().unwrap().entries.is_empty());
}

#[test]
fn stale_reconciliation_cannot_retire_a_reused_identity() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    owner.retire(ALICE, a.lease_revision).unwrap(); let renewed = owner.register(ALICE).unwrap();
    let before = fs::read(&fixture.path).unwrap();
    assert_eq!(owner.retire_absent(&[LeaseKey { client_id: ALICE.into(), lease_revision: a.lease_revision }]).unwrap_err(), Error::ExpiredLease);
    assert_eq!(fs::read(&fixture.path).unwrap(), before);
    owner.apply(&learn(&renewed, "hello", true)).unwrap();
}

#[test]
fn old_sequence_is_superseded_and_a_future_gap_remains_out_of_order() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    let first = learn(&a, "hello", true); owner.apply(&first).unwrap();
    let mut second = first.clone(); second.sequence = 2; owner.apply(&second).unwrap();
    assert_eq!(owner.apply(&first).unwrap_err(), Error::Superseded);
    second.sequence = 4; assert_eq!(owner.apply(&second).unwrap_err(), Error::OutOfOrder);
}

#[test]
fn debug_output_does_not_disclose_learned_text_or_local_paths() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap();
    let command = learn(&a, "dontlogthis", true); owner.apply(&command).unwrap();
    let rendered = format!("{owner:?} {command:?} {:?} {:?}", owner.snapshot().unwrap(), owner.state);
    assert!(!rendered.contains("dontlogthis"));
    assert!(!rendered.contains("hanautomata-learning-test-"));
}

#[test]
fn reset_discards_old_word_fence_hashes() {
    let fixture = Fixture::new(); let mut owner = fixture.open(); let mut a = owner.register(ALICE).unwrap();
    owner.apply(&request(&a, Operation::Forget { raw: "hello".into(), keys: "hello".into() })).unwrap();
    assert!(owner.state.fences[0].word.is_some()); a = owner.register(ALICE).unwrap();
    owner.apply(&request(&a, Operation::Reset {})).unwrap();
    assert!(owner.state.fences.iter().all(|f| f.word.is_none()));
}

#[test]
fn stale_deletion_requires_new_confirmation_without_consuming_its_sequence() {
    for operation in [Operation::Forget { raw: "world".into(), keys: "world".into() }, Operation::Reset {}] {
        let fixture = Fixture::new(); let mut owner = fixture.open(); let a = owner.register(ALICE).unwrap(); let b = owner.register(BOB).unwrap();
        owner.apply(&request(&a, Operation::Forget { raw: "hello".into(), keys: "hello".into() })).unwrap();
        let before = fs::read(&fixture.path).unwrap();
        assert_eq!(owner.apply(&request(&b, operation.clone())).unwrap_err(), Error::StaleGeneration);
        assert_eq!(fs::read(&fixture.path).unwrap(), before);
        let refreshed = owner.register(BOB).unwrap(); assert_eq!(refreshed.next_sequence, 1);
        // This is a newly confirmed deletion, never automatic restamping of the rejected request.
        owner.apply(&request(&refreshed, operation)).unwrap();
    }
}
