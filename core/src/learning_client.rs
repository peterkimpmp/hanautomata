//! Bounded handoff of confirmed document choices to an off-key persistence worker.
//! The platform must authenticate the broker/lease, capture Action before the document edit, and submit only
//! Engine::confirm_learning deltas. This module performs no I/O, installs nothing, and does not authenticate IPC.
//! Producer calls never wait for the worker's mutex. The worker releases it before any transport/disk operation.
use super::{valid_id, Error, Lease, Operation, Reply, Request, Version};
use crate::LearningChoice;
use std::{collections::VecDeque, sync::{Arc, Mutex, MutexGuard, Weak, TryLockError,
    atomic::{AtomicU64, Ordering}}};

pub const MAX_QUEUED_CHOICES: usize = 64;
const MAX_BATCH_CHOICES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueError { Busy, Paused, Closed, NeedsRefresh, ForeignAction, InvalidChoice, Full,
    InvalidLease, InvalidReply, StaleDispatch, StaleSnapshot, Poisoned }
impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "learning client: {self:?}") }
}
impl std::error::Error for QueueError {}
type Result<T> = std::result::Result<T, QueueError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress { Committed, DroppedStale, Retry, Stopped }

struct Pending { generation: u64, operation: Operation }
struct Flight { identity: Arc<()>, request: Request }
struct State {
    lease: Lease, observed: Version, next_sequence: u64, last_committed: u64,
    pending: VecDeque<Pending>, flight: Option<Flight>, refresh_after: Option<u64>, closed: bool,
    gated_snapshots: bool, installed: Option<Version>, accepted: u64,
}
struct Shared { state: Mutex<State>, epoch: AtomicU64, waker: Option<std::task::Waker> }

/// A one-use capture of the storage generation before the user/document operation. It contains no input text.
pub struct Action { owner: Weak<Shared>, epoch: u64, origin: Version }
impl Action { pub fn origin(&self) -> &Version { &self.origin } }
/// Issued only when the engine and queue install the same authenticated snapshot. Capturing from this
/// immutable receipt needs no worker lock; submission still validates ownership, epoch and queue state.
#[derive(Clone)]
pub struct InstalledOrigin { owner: Weak<Shared>, version: Version }
impl InstalledOrigin {
    pub fn capture(&self) -> Result<Action> {
        let shared = self.owner.upgrade().ok_or(QueueError::Closed)?;
        let epoch = shared.epoch.load(Ordering::Acquire);
        if epoch & 1 != 0 { return Err(QueueError::Paused); }
        Ok(Action { owner: self.owner.clone(), epoch, origin: self.version.clone() })
    }
}
/// Returned to the transport worker. Retries retain the same identity and exact request, even after lost replies.
pub struct Dispatch { identity: Arc<()>, request: Request }
impl Dispatch { pub fn request(&self) -> &Request { &self.request } }
#[derive(Clone)]
pub struct Producer { shared: Arc<Shared> }
pub struct Worker { shared: Arc<Shared> }
/// Worker-issued checkpoint for a snapshot read. Its contents are deliberately private to this queue.
pub struct SnapshotTicket { owner: Weak<Shared>, accepted: u64 }

/// Snapshot-aware channels keep newly captured actions on the installed dictionary's generation.
pub fn channel_for_snapshots(lease: Lease, waker: std::task::Waker) -> Result<(Producer, Worker)> {
    make_channel(lease, Some(waker), true)
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("LearningAction { redacted }") }
}
impl std::fmt::Debug for Dispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.request.fmt(f) }
}
impl std::fmt::Debug for Producer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("LearningProducer { redacted }") }
}
impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("LearningWorker { redacted }") }
}

fn valid_version(version: &Version) -> bool {
    valid_id(&version.store_id) && version.generation > 0 && version.generation <= version.revision
}
fn merge_version(old: &Version, new: &Version) -> Result<Version> {
    if !valid_version(new) || old.store_id != new.store_id
        || (new.revision >= old.revision && new.generation < old.generation)
        || (new.revision <= old.revision && new.generation > old.generation) { return Err(QueueError::InvalidReply); }
    Ok(if new.revision > old.revision { new.clone() } else { old.clone() })
}
fn ready(state: &State) -> Result<()> {
    if state.closed { Err(QueueError::Closed) }
    else if state.refresh_after.is_some() { Err(QueueError::NeedsRefresh) } else { Ok(()) }
}
fn close(state: &mut State) { state.closed = true; state.pending.clear(); state.flight = None; }

/// One authenticated lease per queue. Never reuse queued work under a different store or lease.
pub fn channel(lease: Lease) -> Result<(Producer, Worker)> {
    make_channel(lease, None, false)
}
/// An optional nonblocking, panic-free in-process notification supplied by the platform worker.
/// Notification happens after releasing the queue lock; it must never perform file/transport work.
pub fn channel_with_waker(lease: Lease, waker: std::task::Waker) -> Result<(Producer, Worker)> {
    make_channel(lease, Some(waker), false)
}
fn make_channel(lease: Lease, waker: Option<std::task::Waker>, gated_snapshots: bool) -> Result<(Producer, Worker)> {
    if !valid_version(&lease.version) || !valid_id(&lease.client_id) || lease.lease_revision == 0
        || lease.lease_revision > lease.version.revision || lease.next_sequence == 0
        || lease.next_sequence == u64::MAX
        || lease.next_sequence - 1 > lease.version.revision - lease.lease_revision { return Err(QueueError::InvalidLease); }
    let shared = Arc::new(Shared { epoch: AtomicU64::new(2), waker, state: Mutex::new(State {
        observed: lease.version.clone(), next_sequence: lease.next_sequence, last_committed: lease.lease_revision,
        lease, pending: VecDeque::with_capacity(MAX_QUEUED_CHOICES), flight: None, refresh_after: None, closed: false,
        gated_snapshots, installed: None, accepted: 0,
    }) });
    Ok((Producer { shared: shared.clone() }, Worker { shared }))
}

impl Producer {
    /// Only call with an authenticated worker snapshot and a bounded, nonblocking, memory-only install closure.
    /// The queue lock couples the actual dictionary swap to the generation used by the next action. Newly
    /// accepted choices invalidate the checkpoint even if their disk writes have completed in the meantime.
    pub fn install_snapshot(&self, ticket: &SnapshotTicket, version: &Version,
        install: impl FnOnce() -> crate::SnapshotInstall) -> Result<crate::SnapshotInstall> {
        self.install_snapshot_with_origin(ticket, version, install).map(|(result, _)| result)
    }
    pub fn install_snapshot_with_origin(&self, ticket: &SnapshotTicket, version: &Version,
        install: impl FnOnce() -> crate::SnapshotInstall) -> Result<(crate::SnapshotInstall, Option<InstalledOrigin>)> {
        if !Weak::ptr_eq(&ticket.owner, &Arc::downgrade(&self.shared)) { return Err(QueueError::ForeignAction); }
        let mut state = self.try_state()?;
        if state.closed { return Err(QueueError::Closed); }
        if !state.gated_snapshots { return Err(QueueError::InvalidLease); }
        if ticket.accepted != state.accepted || !state.pending.is_empty() || state.flight.is_some()
            || version.revision < state.observed.revision || version.revision < state.last_committed
            || state.installed.as_ref().is_some_and(|old| version.revision <= old.revision) {
            return Err(QueueError::StaleSnapshot);
        }
        let observed = merge_version(&state.observed, version)?;
        let result = install();
        if result == crate::SnapshotInstall::Applied {
            state.installed = Some(version.clone()); state.observed = observed;
            if state.refresh_after.is_some_and(|old| version.generation > old) { state.refresh_after = None; }
        }
        let origin = (result == crate::SnapshotInstall::Applied).then(|| InstalledOrigin { owner: Arc::downgrade(&self.shared), version: version.clone() });
        Ok((result, origin))
    }
    fn try_state(&self) -> Result<MutexGuard<'_, State>> {
        self.shared.state.try_lock().map_err(|e| match e {
            TryLockError::WouldBlock => QueueError::Busy, TryLockError::Poisoned(_) => QueueError::Poisoned,
        })
    }
    /// No wait, disk access, IPC, or worker creation on the input thread.
    pub fn begin(&self) -> Result<Action> {
        let epoch = self.shared.epoch.load(Ordering::Acquire);
        if epoch & 1 != 0 { return Err(QueueError::Paused); }
        let state = self.try_state()?;
        if state.gated_snapshots { if state.closed { return Err(QueueError::Closed); } }
        else { ready(&state)?; }
        if self.shared.epoch.load(Ordering::Acquire) != epoch { return Err(QueueError::Paused); }
        let origin = if state.gated_snapshots {
            let installed = state.installed.as_ref().ok_or(QueueError::NeedsRefresh)?;
            installed
        }
            else { &state.observed };
        Ok(Action { owner: Arc::downgrade(&self.shared), epoch, origin: origin.clone() })
    }
    /// Invalidates unsubmitted actions across pause/resume. Already accepted, confirmed choices remain queued;
    /// pause is not an erase or cancellation of an operation already sent to storage.
    pub fn set_recording(&self, enabled: bool) {
        #[allow(deprecated)] // Rust 1.89 supports fetch_update; try_update requires a newer toolchain.
        let _ = self.shared.epoch.fetch_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
            if (epoch & 1 == 0) == enabled { None } else { epoch.checked_add(1) }
        }); // Exhaustion leaves the odd (paused) epoch permanently paused rather than reusing a ticket.
    }
    pub fn submit(&self, action: Action, choices: &[LearningChoice]) -> Result<()> {
        self.submit_recoverable(action, choices).map_err(|(error, _)| error)
    }
    /// On rejection ownership stays with the caller. Retrying this same action cannot change its origin,
    /// and no rejected attempt has accepted any choices. Success consumes it exactly once.
    pub fn submit_recoverable(&self, action: Action, choices: &[LearningChoice]) -> std::result::Result<(), (QueueError, Action)> {
        match self.submit_borrowed(&action, choices) { Ok(()) => Ok(()), Err(error) => Err((error, action)) }
    }
    fn submit_borrowed(&self, action: &Action, choices: &[LearningChoice]) -> Result<()> {
        if !Weak::ptr_eq(&action.owner, &Arc::downgrade(&self.shared)) { return Err(QueueError::ForeignAction); }
        if self.shared.epoch.load(Ordering::Acquire) != action.epoch { return Err(QueueError::Paused); }
        if choices.len() > MAX_BATCH_CHOICES || choices.iter().any(|c| c.raw.len() > 256 || c.keys.len() > 256
            || !crate::prefs::valid_context(&c.context) || crate::prefs::normalize(&c.raw, &c.keys).is_none()) {
            return Err(QueueError::InvalidChoice);
        }
        let mut state = self.try_state()?; ready(&state)?;
        // Capture the original action even while a deletion is pending, but defer acceptance until its
        // dictionary is installed. Rejected submissions do not invalidate the snapshot checkpoint.
        if state.gated_snapshots && state.installed.as_ref().is_none_or(|installed| installed.generation < state.observed.generation) {
            return Err(QueueError::NeedsRefresh);
        }
        if state.pending.len() + usize::from(state.flight.is_some()) + choices.len() > MAX_QUEUED_CHOICES {
            return Err(QueueError::Full);
        }
        // This last epoch load is the submission's linearization point relative to pause/resume.
        if self.shared.epoch.load(Ordering::Acquire) != action.epoch { return Err(QueueError::Paused); }
        let Some(accepted) = state.accepted.checked_add(choices.len() as u64) else {
            close(&mut state); return Err(QueueError::Closed);
        };
        state.accepted = accepted;
        state.pending.extend(choices.iter().map(|c| Pending { generation: action.origin.generation,
            operation: Operation::Learn { raw: c.raw.clone(), keys: c.keys.clone(), korean: c.korean, context: c.context.clone() } }));
        drop(state);
        if !choices.is_empty() { if let Some(waker) = &self.shared.waker { waker.wake_by_ref(); } }
        Ok(())
    }
}

impl Worker {
    pub fn snapshot_ticket(&self) -> Result<SnapshotTicket> {
        let state = self.state()?;
        if state.closed { return Err(QueueError::Closed); }
        if !state.gated_snapshots { return Err(QueueError::InvalidLease); }
        if !state.pending.is_empty() || state.flight.is_some() { return Err(QueueError::Busy); }
        Ok(SnapshotTicket { owner: Arc::downgrade(&self.shared), accepted: state.accepted })
    }
    fn state(&self) -> Result<MutexGuard<'_, State>> { self.shared.state.lock().map_err(|_| QueueError::Poisoned) }
    /// Take the next immutable request; a missing transport response must call this again without changing its ID.
    pub fn next(&mut self) -> Result<Option<Dispatch>> {
        let mut state = self.state()?; ready(&state)?;
        if state.flight.is_none() {
            let Some(pending) = state.pending.pop_front() else { return Ok(None) };
            let request = Request { store_id: state.lease.version.store_id.clone(), client_id: state.lease.client_id.clone(),
                lease_revision: state.lease.lease_revision, sequence: state.next_sequence, generation: pending.generation,
                operation: pending.operation };
            if request.validate().is_err() { close(&mut state); return Err(QueueError::InvalidChoice); }
            state.flight = Some(Flight { identity: Arc::new(()), request });
        }
        let flight = state.flight.as_ref().unwrap();
        Ok(Some(Dispatch { identity: flight.identity.clone(), request: flight.request.clone() }))
    }
    /// Only versions obtained from the authenticated authority may be supplied. Old snapshots cannot lower the
    /// observed version. Snapshot-aware channels change the new-action generation only after install_snapshot
    /// succeeds; a different store requires a new channel with no replay of old queued actions.
    pub fn observe(&mut self, version: Version) -> Result<()> {
        let mut state = self.state()?;
        if state.closed { return Err(QueueError::Closed); }
        state.observed = merge_version(&state.observed, &version)?;
        if state.refresh_after.is_some_and(|old| state.observed.generation > old) { state.refresh_after = None; }
        if state.refresh_after.is_some() { Err(QueueError::NeedsRefresh) } else { Ok(()) }
    }
    pub fn finish(&mut self, dispatched: &Dispatch, response: std::result::Result<Reply, Error>) -> Result<Progress> {
        let mut state = self.state()?;
        if state.closed { return Err(QueueError::Closed); }
        let flight = state.flight.as_ref().ok_or(QueueError::StaleDispatch)?;
        if !Arc::ptr_eq(&flight.identity, &dispatched.identity) { return Err(QueueError::StaleDispatch); }
        match response {
            Ok(reply) => {
                if reply.sequence != flight.request.sequence || reply.committed_revision <= state.last_committed
                    || reply.committed_revision > reply.version.revision || reply.version.generation < flight.request.generation {
                    return Err(QueueError::InvalidReply);
                }
                let version = merge_version(&state.observed, &reply.version)?;
                state.observed = version; state.last_committed = reply.committed_revision; state.flight = None;
                if let Some(next) = reply.sequence.checked_add(1) { state.next_sequence = next; }
                else { close(&mut state); }
                Ok(Progress::Committed)
            },
            Err(Error::Io | Error::Busy | Error::Faulted) => Ok(Progress::Retry),
            Err(Error::StaleGeneration) => {
                state.refresh_after = Some(flight.request.generation); state.flight = None;
                // No sequence was committed. The following action keeps its own original generation and may
                // reuse this sequence; never relabel a stale choice with the newer generation.
                Ok(Progress::DroppedStale)
            },
            Err(_) => { close(&mut state); Ok(Progress::Stopped) },
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        close(&mut self.shared.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lease() -> Lease { Lease { client_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(), lease_revision: 2,
        version: Version { store_id: "11111111111111111111111111111111".into(), generation: 1, revision: 2 }, next_sequence: 1 } }
    fn choice() -> LearningChoice { LearningChoice { raw: "privateword".into(), keys: "privateword".into(), korean: true, context: String::new() } }

    #[test]
    fn only_accepted_nonempty_submissions_notify_after_releasing_the_queue_lock() {
        struct Signal { queue: std::sync::OnceLock<Weak<Shared>>, count: AtomicU64 }
        impl std::task::Wake for Signal {
            fn wake(self: Arc<Self>) { self.wake_by_ref(); }
            fn wake_by_ref(self: &Arc<Self>) {
                let queue = self.queue.get().unwrap().upgrade().unwrap();
                assert!(queue.state.try_lock().is_ok(), "notification must not hold the input queue lock");
                self.count.fetch_add(1, Ordering::Relaxed);
            }
        }
        let signal = Arc::new(Signal { queue: std::sync::OnceLock::new(), count: AtomicU64::new(0) });
        let (producer, worker) = channel_with_waker(lease(), std::task::Waker::from(signal.clone())).unwrap();
        signal.queue.set(Arc::downgrade(&producer.shared)).ok().unwrap();
        producer.submit(producer.begin().unwrap(), &[]).unwrap();
        producer.submit(producer.begin().unwrap(), &[choice()]).unwrap();
        assert_eq!(signal.count.load(Ordering::Relaxed), 1);
        let action = producer.begin().unwrap(); let held = worker.shared.state.lock().unwrap();
        assert_eq!(producer.submit(action, &[choice()]), Err(QueueError::Busy)); drop(held);
        assert_eq!(signal.count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn input_calls_never_wait_for_a_worker_lock() {
        let (producer, worker) = channel(lease()).unwrap();
        let action = producer.begin().unwrap();
        let held = worker.shared.state.lock().unwrap();
        assert!(matches!(producer.begin(), Err(QueueError::Busy)));
        assert_eq!(producer.submit(action, &[choice()]), Err(QueueError::Busy));
        drop(held);
    }
    #[test]
    fn installed_origin_retains_recoverable_actions_during_contention_without_bypassing_pause() {
        let (producer, worker) = channel_for_snapshots(lease(), std::task::Waker::noop().clone()).unwrap();
        let ticket = worker.snapshot_ticket().unwrap();
        let (_, no_origin) = producer.install_snapshot_with_origin(&ticket, &lease().version, || crate::SnapshotInstall::Changed).unwrap();
        assert!(no_origin.is_none());
        let (_, origin) = producer.install_snapshot_with_origin(&ticket, &lease().version, || crate::SnapshotInstall::Applied).unwrap();
        let origin = origin.unwrap(); let held = worker.shared.state.lock().unwrap();
        let captured = origin.capture().unwrap(); assert_eq!(captured.origin(), &lease().version);
        let (error, captured) = producer.submit_recoverable(captured, &[choice()]).unwrap_err();
        assert_eq!(error, QueueError::Busy); drop(held);
        producer.set_recording(false); assert!(matches!(origin.capture(), Err(QueueError::Paused)));
        assert!(matches!(producer.submit_recoverable(captured, &[choice()]), Err((QueueError::Paused, _))));
        producer.set_recording(true); producer.submit_recoverable(origin.capture().unwrap(), &[choice()]).unwrap();
    }
    #[test]
    fn snapshot_swap_never_waits_for_the_worker_or_reuses_an_installed_revision() {
        let (producer, worker) = channel_for_snapshots(lease(), std::task::Waker::noop().clone()).unwrap();
        let ticket = worker.snapshot_ticket().unwrap(); let called = std::cell::Cell::new(false);
        let held = worker.shared.state.lock().unwrap();
        assert_eq!(producer.install_snapshot(&ticket, &lease().version, || { called.set(true); crate::SnapshotInstall::Applied }), Err(QueueError::Busy));
        assert!(!called.get()); drop(held);
        assert_eq!(producer.install_snapshot(&ticket, &lease().version, || crate::SnapshotInstall::Applied), Ok(crate::SnapshotInstall::Applied));
        assert_eq!(producer.install_snapshot(&ticket, &lease().version, || { called.set(true); crate::SnapshotInstall::Applied }), Err(QueueError::StaleSnapshot));
        assert!(!called.get());
    }
    #[test]
    fn exhausted_snapshot_sequence_cannot_make_an_old_checkpoint_current_again() {
        let (producer, worker) = channel_for_snapshots(lease(), std::task::Waker::noop().clone()).unwrap();
        let ticket = worker.snapshot_ticket().unwrap();
        producer.install_snapshot(&ticket, &lease().version, || crate::SnapshotInstall::Applied).unwrap();
        worker.shared.state.lock().unwrap().accepted = u64::MAX;
        assert_eq!(producer.submit(producer.begin().unwrap(), &[choice()]), Err(QueueError::Closed));
        assert!(matches!(producer.begin(), Err(QueueError::Closed)));
    }
    #[test]
    fn diagnostics_do_not_include_queued_words_or_peer_identifiers() {
        let (producer, mut worker) = channel(lease()).unwrap();
        let action = producer.begin().unwrap(); let debug = format!("{producer:?} {worker:?} {action:?}");
        producer.submit(action, &[choice()]).unwrap();
        let debug = format!("{debug} {:?}", worker.next().unwrap().unwrap());
        for secret in ["privateword", &lease().client_id, &lease().version.store_id] { assert!(!debug.contains(secret)); }
    }
    #[test]
    fn invalid_lease_and_exhausted_pause_epoch_are_fail_closed() {
        for mode in 0..6 {
            let mut invalid = lease();
            match mode { 0 => invalid.client_id.clear(), 1 => invalid.version.generation = 0,
                2 => invalid.lease_revision = 3, 3 => invalid.next_sequence = 0,
                4 => invalid.next_sequence = 2, _ => invalid.next_sequence = u64::MAX }
            assert!(matches!(channel(invalid), Err(QueueError::InvalidLease)));
        }
        let (producer, _worker) = channel(lease()).unwrap();
        producer.shared.epoch.store(u64::MAX - 1, Ordering::Release);
        let old = producer.begin().unwrap(); producer.set_recording(false); producer.set_recording(true);
        assert!(matches!(producer.begin(), Err(QueueError::Paused)));
        assert_eq!(producer.submit(old, &[choice()]), Err(QueueError::Paused));
    }
    #[test]
    fn poisoned_queue_rejects_new_work_without_panicking_on_the_input_thread() {
        let (producer, mut worker) = channel(lease()).unwrap();
        let _ = std::panic::catch_unwind(|| { let _held = worker.shared.state.lock().unwrap(); panic!("synthetic worker failure"); });
        assert!(matches!(producer.begin(), Err(QueueError::Poisoned)));
        assert!(matches!(worker.next(), Err(QueueError::Poisoned)));
        drop(worker);
    }
}
