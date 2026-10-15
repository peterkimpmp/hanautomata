//! Apartment-local handoff. No I/O, worker creation, waiting for a queue lock, or borrowed COM objects.
//! A word retains its original storage generation across keystrokes and delayed finalization.
use hanautomata_core::{learning::{client::{Action, InstalledOrigin, Producer, QueueError, MAX_QUEUED_CHOICES}, Snapshot, Version}, LearningBatch, LearningChoice};
use hanautomata_ffi::Engine;
use std::{cell::{Cell, RefCell}, rc::Rc, sync::Arc};

#[path = "service_learning_notice.rs"]
mod notice;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure { Disconnected, MissingOrigin, Queue(QueueError) }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Report {
    pub storage: Option<crate::learning_runtime::Status>,
    /// None until the first snapshot is considered. Later applications have a separate counter.
    pub initial_snapshot_applied: Option<bool>,
    pub snapshots_applied: u64,
    /// A producer is attached; this is not a live broker connection or a successful disk write.
    pub connected: bool,
    /// Cumulative choices accepted by the memory queue, not a disk ACK or a durability claim.
    pub queued: u64,
    pub rejected: u64,
    /// Confirmed document operations whose learning ticket was invalidated before the callback arrived.
    pub invalidated: u64,
    pub recovery_pending: usize,
    pub recovery_queued: u64,
    pub recovery_invalidated: u64,
    pub recovery_overflow: u64,
    /// Replaced by a later document confirmation in this Link.
    pub recovery_superseded: u64,
    /// Unpersistable fallback yielded to an authoritative stored word, regardless of wall-clock order.
    pub recovery_yielded_to_store: u64,
    /// Sticky until the owning session ends. A later successful queue submission cannot hide a lost choice.
    pub last_failure: Option<Failure>,
}
#[derive(Default)]
pub(super) struct Link { producer: Option<Producer>, report: Rc<Cell<Report>>, recovery: Rc<RefCell<Vec<LocalBatch>>>,
    installed: Option<Version>, origin: Option<InstalledOrigin> }
struct LocalBatch { origin: Option<Version>, action: Option<Action>, producer: Option<Producer>, choices: Vec<LearningChoice> }
pub(super) struct Stamp {
    producer: Option<Producer>, action: std::result::Result<Action, Failure>, report: Rc<Cell<Report>>,
    origin: Option<Version>, recovery: Rc<RefCell<Vec<LocalBatch>>>,
}
pub(super) struct Confirmation { pub engine: Arc<Engine>, pub batches: Vec<LearningBatch>, pub stamp: Stamp }

impl Link {
    /// Called only with a freshly authenticated, service-specific lease/worker. Discovery remains a platform task.
    pub fn connect(&mut self, producer: Producer) {
        self.disconnect(); self.producer = Some(producer);
        let mut report = self.report.get(); report.connected = true; self.report.set(report);
    }
    pub fn disconnect(&mut self) {
        if let Some(producer) = self.producer.take() { producer.set_recording(false); }
        self.origin = None; self.installed = None;
        let mut report = self.report.get(); report.connected = false; self.report.set(report);
    }
    pub fn capture(&self) -> Stamp {
        let action = match &self.origin { Some(origin) => origin.capture().map_err(Failure::Queue),
            None => self.producer.as_ref().ok_or(Failure::Disconnected).and_then(|p| p.begin().map_err(Failure::Queue)) };
        let origin = action.as_ref().ok().map(|a| a.origin().clone()).or_else(|| self.installed.clone());
        Stamp { producer: self.producer.clone(), report: self.report.clone(), action, origin, recovery: self.recovery.clone() }
    }
    /// A connection cannot assign a fresh generation to a word that began before it existed.
    pub fn missing_origin(&self) -> Stamp {
        Stamp { producer: self.producer.clone(), report: self.report.clone(), origin: None, recovery: self.recovery.clone(),
            action: Err(if self.producer.is_some() { Failure::MissingOrigin } else { Failure::Disconnected }) }
    }
    pub fn report(&self) -> Report { self.report.get() }
    /// Retry only the original, unaccepted action. Unknown-origin choices stay local; a new lease is never used.
    pub fn retry(&self) {
        let mut recovery = self.recovery.borrow_mut(); let mut report = self.report.get(); let mut attempts = 0;
        for batch in recovery.iter_mut() {
            if attempts == 8 { break; }
            let Some(action) = batch.action.take() else { continue; }; attempts += 1;
            let Some(producer) = &batch.producer else { continue; };
            match producer.submit_recoverable(action, &batch.choices) {
                Ok(()) => { let n = batch.choices.len() as u64; report.queued = report.queued.saturating_add(n);
                    report.recovery_queued = report.recovery_queued.saturating_add(n); batch.choices.clear(); },
                Err((error, action)) => {
                    if retryable(error) { batch.action = Some(action); break; }
                    report.last_failure = Some(Failure::Queue(error));
                },
            }
        }
        recovery.retain(|batch| !batch.choices.is_empty());
        report.recovery_pending = recovery.iter().map(|b| b.choices.len()).sum(); self.report.set(report);
    }
    pub fn local_choices(&self, snapshot: &Snapshot) -> Vec<LearningChoice> {
        self.recovery.borrow().iter().flat_map(|batch| batch.choices.iter().filter(|choice|
            snapshot.permits_local(batch.origin.as_ref(), choice)
                && !stored_supersedes_local(snapshot, batch.action.is_some(), choice)).cloned()).collect()
    }
    /// Called only after the queue and engine accepted the same snapshot. Local-only origins may acquire a
    /// deletion baseline on first import, but never acquire an Action or permission to persist.
    pub fn snapshot_applied(&mut self, snapshot: &Snapshot, origin: Option<InstalledOrigin>) {
        self.installed = Some(snapshot.version.clone()); self.origin = origin;
        let mut recovery = self.recovery.borrow_mut(); let mut report = self.report.get();
        for batch in recovery.iter_mut() {
            let can_retry = batch.action.is_some();
            batch.choices.retain(|choice| {
                if !snapshot.permits_local(batch.origin.as_ref(), choice) {
                    report.recovery_invalidated = report.recovery_invalidated.saturating_add(1); false
                } else if stored_supersedes_local(snapshot, can_retry, choice) {
                    report.recovery_yielded_to_store = report.recovery_yielded_to_store.saturating_add(1); false
                } else { true }
            });
            if batch.origin.is_none() { batch.origin = Some(snapshot.version.clone()); }
        }
        recovery.retain(|batch| !batch.choices.is_empty());
        report.recovery_pending = recovery.iter().map(|b| b.choices.len()).sum(); self.report.set(report);
    }
    pub fn storage_status(&self, status: crate::learning_runtime::Status, imported: Option<bool>) {
        let mut report = self.report.get(); report.storage = Some(status);
        if let Some(imported) = imported {
            if report.initial_snapshot_applied.is_none() { report.initial_snapshot_applied = Some(imported); }
            if imported { report.snapshots_applied = report.snapshots_applied.saturating_add(1); }
        }
        self.report.set(report);
    }
}
impl Drop for Link { fn drop(&mut self) { self.disconnect(); } }
fn retryable(error: QueueError) -> bool { matches!(error, QueueError::Busy | QueueError::Full | QueueError::NeedsRefresh) }
/// The authority linearizes persistent decisions. A fallback with no reusable Action cannot establish
/// ordering against another process, so a stored decision wins for that word (across all contexts).
/// Retryable choices keep their original Action and are ordered when the authority commits them.
/// This changes future preferences only, never text already confirmed in a document.
fn stored_supersedes_local(snapshot: &Snapshot, can_retry: bool, choice: &LearningChoice) -> bool {
    !can_retry && snapshot.entries.iter().any(|word| word.raw == choice.raw && word.keys == choice.keys)
}
impl Confirmation {
    /// Consume only after the document accepted the corresponding edit. Batch confirmation is one-use, and
    /// private/paused/reset/foreign batches return no choices. Queue failure never rolls the document back.
    pub fn confirm(self) {
        let mut choices = Vec::new(); let mut invalidated = 0;
        for batch in &self.batches { match self.engine.confirm_learning_with_status(batch) {
            hanautomata_core::LearningConfirmation::Confirmed(mut confirmed) => choices.append(&mut confirmed),
            hanautomata_core::LearningConfirmation::Invalidated(count) => invalidated += count as u64,
            hanautomata_core::LearningConfirmation::ConsumedOrForeign => {},
        } }
        let mut report = self.stamp.report.get(); report.invalidated = report.invalidated.saturating_add(invalidated); self.stamp.report.set(report);
        if choices.is_empty() { return; }
        // A newer confirmed decision for a word supersedes that word's older, still-unsaved decisions
        // (including their context). Never replay the old preference over the later persisted decision.
        let mut recovery = self.stamp.recovery.borrow_mut(); let mut report = self.stamp.report.get();
        for batch in recovery.iter_mut() {
            let old = batch.choices.len();
            batch.choices.retain(|old| !choices.iter().any(|new| old.raw == new.raw && old.keys == new.keys));
            report.recovery_superseded = report.recovery_superseded.saturating_add((old - batch.choices.len()) as u64);
        }
        recovery.retain(|batch| !batch.choices.is_empty());
        report.recovery_pending = recovery.iter().map(|b| b.choices.len()).sum(); self.stamp.report.set(report); drop(recovery);
        let mut retry = None;
        let result = match self.stamp.action {
            Err(failure) => Err(failure),
            Ok(action) => match &self.stamp.producer {
                None => Err(Failure::Disconnected),
                Some(producer) => producer.submit_recoverable(action, &choices).map_err(|(error, action)| {
                    if retryable(error) { retry = Some(action); } Failure::Queue(error)
                }),
            },
        };
        let mut report = self.stamp.report.get();
        match result {
            Ok(()) => report.queued = report.queued.saturating_add(choices.len() as u64),
            Err(failure) => {
                report.rejected = report.rejected.saturating_add(choices.len() as u64); report.last_failure = Some(failure);
                let mut recovery = self.stamp.recovery.borrow_mut();
                let mut count: usize = recovery.iter().map(|b| b.choices.len()).sum();
                // Keep the newest bounded local evidence; overflow is explicit and never blocks future storage.
                while !recovery.is_empty() && count + choices.len() > MAX_QUEUED_CHOICES {
                    let removed = recovery.remove(0).choices.len(); count -= removed;
                    report.recovery_overflow = report.recovery_overflow.saturating_add(removed as u64);
                }
                if choices.len() <= MAX_QUEUED_CHOICES {
                    count += choices.len(); recovery.push(LocalBatch { origin: self.stamp.origin,
                        producer: self.stamp.producer, action: retry, choices });
                } else { report.recovery_overflow = report.recovery_overflow.saturating_add(choices.len() as u64); }
                report.recovery_pending = count;
            },
        }
        self.stamp.report.set(report);
    }
}
