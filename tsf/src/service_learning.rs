//! Apartment-local handoff. No I/O, worker creation, waiting for a queue lock, or borrowed COM objects.
//! A word retains its original storage generation across keystrokes and delayed finalization.
use hanautomata_core::{learning::client::{Action, Producer, QueueError}, LearningBatch};
use hanautomata_ffi::Engine;
use std::{cell::Cell, rc::Rc, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure { Disconnected, MissingOrigin, Queue(QueueError) }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Report {
    /// A producer is attached; this is not a live broker connection or a successful disk write.
    pub connected: bool,
    /// Cumulative choices accepted by the memory queue, not a disk ACK or a durability claim.
    pub queued: u64,
    pub rejected: u64,
    /// Sticky until the owning session ends. A later successful queue submission cannot hide a lost choice.
    pub last_failure: Option<Failure>,
}
#[derive(Default)]
pub(super) struct Link { producer: Option<Producer>, report: Rc<Cell<Report>> }
pub(super) struct Stamp {
    producer: Option<Producer>, action: std::result::Result<Action, Failure>, report: Rc<Cell<Report>>,
}
pub(super) struct Confirmation { pub engine: Arc<Engine>, pub batches: Vec<LearningBatch>, pub stamp: Stamp }

impl Link {
    /// Called only with a freshly authenticated, service-specific lease/worker. Discovery remains a platform task.
    #[allow(dead_code)] // Product worker launch is not enabled yet; production callbacks are exercised with an owned test worker.
    pub fn connect(&mut self, producer: Producer) {
        self.disconnect(); self.producer = Some(producer);
        let mut report = self.report.get(); report.connected = true; self.report.set(report);
    }
    pub fn disconnect(&mut self) {
        if let Some(producer) = self.producer.take() { producer.set_recording(false); }
        let mut report = self.report.get(); report.connected = false; self.report.set(report);
    }
    pub fn capture(&self) -> Stamp {
        Stamp { producer: self.producer.clone(), report: self.report.clone(),
            action: self.producer.as_ref().ok_or(Failure::Disconnected).and_then(|p| p.begin().map_err(Failure::Queue)) }
    }
    /// A connection cannot assign a fresh generation to a word that began before it existed.
    pub fn missing_origin(&self) -> Stamp {
        Stamp { producer: self.producer.clone(), report: self.report.clone(),
            action: Err(if self.producer.is_some() { Failure::MissingOrigin } else { Failure::Disconnected }) }
    }
    #[allow(dead_code)] // Status UI will consume this; tests assert rejected choices are observable now.
    pub fn report(&self) -> Report { self.report.get() }
}
impl Drop for Link { fn drop(&mut self) { self.disconnect(); } }
impl Confirmation {
    /// Consume only after the document accepted the corresponding edit. Batch confirmation is one-use, and
    /// private/paused/reset/foreign batches return no choices. Queue failure never rolls the document back.
    pub fn confirm(self) {
        let choices: Vec<_> = self.batches.iter().flat_map(|batch| self.engine.confirm_learning(batch)).collect();
        if choices.is_empty() { return; }
        let result = self.stamp.action.and_then(|action| self.stamp.producer.as_ref().ok_or(Failure::Disconnected)
            .and_then(|producer| producer.submit(action, &choices).map_err(Failure::Queue)));
        let mut report = self.stamp.report.get();
        match result {
            Ok(()) => report.queued = report.queued.saturating_add(choices.len() as u64),
            Err(failure) => { report.rejected = report.rejected.saturating_add(choices.len() as u64); report.last_failure = Some(failure); },
        }
        self.stamp.report.set(report);
    }
}
