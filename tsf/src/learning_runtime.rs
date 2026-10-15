//! Desktop persistence worker. Activation may start it; key callbacks only poll readiness and submit to Producer.
//! One exact child per active service, random endpoint, no discovery or shared PID file. Companions serialize
//! short file transactions, so apps cannot overwrite each other's stale dictionaries. Restricted hosts fail closed.
use crate::learning_pipe::{self, call, Command as WireCommand, Endpoint, Envelope, Peer};
use hanautomata_core::learning::{client::{channel_with_waker, Producer, Progress, QueueError}, Error as StoreError, Lease, Snapshot};
use crate::learning_wake::{Signal, Reason};
use std::{os::windows::fs::MetadataExt, path::PathBuf,
    sync::{Arc, mpsc, atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering}},
    thread, time::{Duration, Instant}};
const EXCHANGE: Duration = Duration::from_secs(5);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static SPAWN_DENIED: AtomicBool = AtomicBool::new(false);
pub(crate) fn spawn_denied() -> bool { SPAWN_DENIED.load(Ordering::Acquire) }
fn record_spawn_denial(error: &std::io::Error, denied: &AtomicBool) {
    // Values verified against Windows SDK bindings (not inferred from the review's reputation-code labels).
    if matches!(error.raw_os_error(), Some(5 | 225 | 367 | 577 | 637 | 740 | 1260 | 4550..=4559 | 4580..=4582)) {
        denied.store(true, Ordering::Release);
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Starting, Running, Stopped, Failed }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status { pub phase: Phase, pub committed: u64, pub failures: u64, pub idle_waits: u64 }
struct Shared { stop: AtomicBool, phase: AtomicU8, committed: AtomicU64, failures: AtomicU64, idle_waits: AtomicU64, signal: Arc<Signal> }
pub struct Ready { pub producer: Producer, pub snapshot: Snapshot, pub prepared: hanautomata_core::PreparedLearning }
pub struct Runtime { shared: Arc<Shared>, ready: mpsc::Receiver<Ready>, producer: Option<Producer>,
    recycle: mpsc::SyncSender<Ready> }
struct Slot;
impl Drop for Slot { fn drop(&mut self) { WORKERS.fetch_sub(1, Ordering::AcqRel); } }
impl Runtime {
    /// Paths must come from the owning installation/known-folder policy, never from an IPC message.
    /// The worker performs path/token checks and process creation. This function never waits for its startup.
    pub fn start(executable: PathBuf, store: PathBuf) -> std::io::Result<Self> {
        Self::start_with_legacy(executable, store, vec![])
    }
    pub fn start_with_legacy(executable: PathBuf, store: PathBuf, legacy: Vec<hanautomata_core::LearnedWord>) -> std::io::Result<Self> {
        if spawn_denied() { return Err(std::io::ErrorKind::PermissionDenied.into()); }
        if !crate::learning_process::child_policy_allows_spawn() {
            SPAWN_DENIED.store(true, Ordering::Release); return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        let old = WORKERS.fetch_add(1, Ordering::AcqRel);
        let slot = Slot;
        if old >= 16 { return Err(std::io::ErrorKind::WouldBlock.into()); }
        let shared = Arc::new(Shared { stop: AtomicBool::new(false), phase: AtomicU8::new(Phase::Starting as u8),
            committed: AtomicU64::new(0), failures: AtomicU64::new(0), idle_waits: AtomicU64::new(0), signal: Arc::new(Signal::new()?) });
        let (send, ready) = mpsc::sync_channel(1); let (recycle, disposal) = mpsc::sync_channel(1); let state = shared.clone();
        thread::Builder::new().name("hanautomata-learning".into()).spawn(move || {
            let _slot = slot;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(executable, store, legacy, send, disposal, &state)));
            let phase = if matches!(outcome, Ok(Ok(()))) { Phase::Stopped } else {
                state.failures.fetch_add(1, Ordering::Relaxed);
                crate::system::log("learning worker failed; document input preserved, persistence unavailable");
                Phase::Failed
            };
            state.phase.store(phase as u8, Ordering::Release);
        })?;
        Ok(Self { shared, ready, producer: None, recycle })
    }
    pub fn try_ready(&mut self) -> Option<Ready> {
        let ready = self.ready.try_recv().ok()?; self.producer = Some(ready.producer.clone()); Some(ready)
    }
    /// The old dictionary after a swap is normally dropped off the key thread. At most one initial snapshot.
    pub fn recycle(&self, ready: Ready) { let _ = self.recycle.try_send(ready); }
    pub fn stop(&self) {
        if let Some(producer) = &self.producer { producer.set_recording(false); }
        self.shared.stop.store(true, Ordering::Release);
        self.shared.signal.notify();
    }
    pub fn status(&self) -> Status {
        let phase = match self.shared.phase.load(Ordering::Acquire) { 0 => Phase::Starting, 1 => Phase::Running, 2 => Phase::Stopped, _ => Phase::Failed };
        Status { phase, committed: self.shared.committed.load(Ordering::Relaxed), failures: self.shared.failures.load(Ordering::Relaxed),
            idle_waits: self.shared.idle_waits.load(Ordering::Relaxed) }
    }
}
impl Drop for Runtime { fn drop(&mut self) { self.stop(); } }
fn request(endpoint: &Endpoint, peer: &Peer, nonce: &str, command: WireCommand) -> Result<serde_json::Value, StoreError> {
    let envelope = Envelope::new(nonce, command).map_err(|_| StoreError::InvalidRequest)?;
    let value = call(endpoint, peer, &envelope, EXCHANGE).map_err(|error| match error {
        learning_pipe::Error::Io | learning_pipe::Error::Timeout => StoreError::Io,
        _ => StoreError::InvalidRequest,
    })?;
    if value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(value.get("error").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or(StoreError::InvalidRequest));
    }
    Ok(value)
}
fn retry<T>(mut action: impl FnMut() -> Result<T, StoreError>, shared: &Shared, peer: &Peer) -> Result<T, StoreError> {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        if shared.stop.load(Ordering::Acquire) || !peer.is_alive() { return Err(StoreError::Io); }
        match action() {
            Err(StoreError::Io | StoreError::Busy | StoreError::Faulted) if Instant::now() < end => thread::sleep(Duration::from_millis(25)),
            result => return result,
        }
    }
}
fn run(executable: PathBuf, store: PathBuf, legacy: Vec<hanautomata_core::LearnedWord>, send: mpsc::SyncSender<Ready>, disposal: mpsc::Receiver<Ready>, shared: &Shared) -> Result<(), StoreError> {
    if !crate::system::desktop_settings_allowed() || spawn_denied() { return Err(StoreError::InvalidRequest); }
    let parent = Peer::process(std::process::id()).map_err(|_| StoreError::InvalidRequest)?;
    if !executable.is_absolute() || !store.is_absolute() { return Err(StoreError::InvalidRequest); }
    let metadata = std::fs::symlink_metadata(&executable).map_err(|_| StoreError::Io)?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 { return Err(StoreError::InvalidRequest); }
    let endpoint = Endpoint::random().map_err(|_| StoreError::Io)?;
    let nonce = Endpoint::random().map_err(|_| StoreError::Io)?;
    let child = crate::learning_process::Child::spawn(&executable, &["--learning-companion".into(), endpoint.token().into(), nonce.token().into(),
        std::process::id().to_string().into(), parent.birth().to_string().into(), store.into_os_string()]).map_err(|error| {
            record_spawn_denial(&error, &SPAWN_DENIED);
            StoreError::Io
        })?;
    let peer = child.peer().map_err(|_| StoreError::Io)?;
    let result = (|| {
        let lease: Lease = serde_json::from_value(retry(|| request(&endpoint, &peer, nonce.token(), WireCommand::RegisterSeeded { entries: legacy.clone() }), shared, &peer)?["lease"].clone())
            .map_err(|_| StoreError::InvalidState)?;
        let snapshot: Snapshot = serde_json::from_value(retry(|| request(&endpoint, &peer, nonce.token(), WireCommand::Snapshot {}), shared, &peer)?["snapshot"].clone())
            .map_err(|_| StoreError::InvalidState)?;
        let (producer, mut worker) = channel_with_waker(lease.clone(), std::task::Waker::from(shared.signal.clone())).map_err(|_| StoreError::InvalidState)?;
        worker.observe(snapshot.version.clone()).map_err(|_| StoreError::InvalidState)?;
        let prepared = hanautomata_core::PreparedLearning::new(&snapshot.entries);
        send.try_send(Ready { producer, snapshot, prepared }).map_err(|_| StoreError::Io)?;
        shared.phase.store(Phase::Running as u8, Ordering::Release);
        let mut stopping = None; let mut consecutive_errors = 0;
        loop {
            drop(disposal.try_recv());
            if shared.stop.load(Ordering::Acquire) { stopping.get_or_insert_with(Instant::now); }
            if stopping.is_some_and(|at: Instant| at.elapsed() >= Duration::from_secs(3)) { return Err(StoreError::Io); }
            match worker.next() {
                Ok(Some(dispatch)) => {
                    let result = request(&endpoint, &peer, nonce.token(), WireCommand::Apply { request: dispatch.request().clone() })
                        .and_then(|v| serde_json::from_value(v["reply"].clone()).map_err(|_| StoreError::InvalidState));
                    match worker.finish(&dispatch, result).map_err(|_| StoreError::InvalidState)? {
                        Progress::Committed => { shared.committed.fetch_add(1, Ordering::Relaxed); consecutive_errors = 0; },
                        Progress::Retry => { shared.failures.fetch_add(1, Ordering::Relaxed); consecutive_errors = (consecutive_errors + 1).min(8);
                            if !peer.is_alive() { return Err(StoreError::Io); }
                            wait_for_signal(shared, &child, retry_delay(consecutive_errors))?; },
                        Progress::DroppedStale => { shared.failures.fetch_add(1, Ordering::Relaxed); },
                        Progress::Stopped => return Err(StoreError::InvalidState),
                    }
                },
                Ok(None) if stopping.is_some() => break,
                Ok(None) => { if !peer.is_alive() { return Err(StoreError::Io); }
                    shared.idle_waits.fetch_add(1, Ordering::Relaxed); wait_for_signal(shared, &child, Duration::from_secs(30))?; },
                Err(QueueError::NeedsRefresh) => {
                    let value = match request(&endpoint, &peer, nonce.token(), WireCommand::Snapshot {}) {
                        Err(StoreError::Io | StoreError::Busy | StoreError::Faulted) if peer.is_alive() => {
                            shared.failures.fetch_add(1, Ordering::Relaxed); consecutive_errors = (consecutive_errors + 1).min(8);
                            wait_for_signal(shared, &child, retry_delay(consecutive_errors))?; continue;
                        },
                        result => result?,
                    };
                    let snapshot: Snapshot = serde_json::from_value(value["snapshot"].clone()).map_err(|_| StoreError::InvalidState)?;
                    worker.observe(snapshot.version).map_err(|_| StoreError::InvalidState)?;
                    consecutive_errors = 0;
                },
                Err(_) => return Err(StoreError::InvalidState),
            }
        }
        Ok(())
    })();
    let end = Instant::now() + Duration::from_secs(2);
    let mut closed = false;
    loop {
        // Zero means this companion's own lease, including an uncertain Register response; never another client's.
        let close = request(&endpoint, &peer, nonce.token(), WireCommand::Close { lease_revision: 0 });
        if close.is_ok() { closed = true; break; }
        if !peer.is_alive() || Instant::now() >= end { break; }
        thread::sleep(Duration::from_millis(25));
    }
    while child.try_wait().map_err(|_| StoreError::Io)?.is_none() && Instant::now() < end { thread::sleep(Duration::from_millis(10)); }
    closed |= child.try_wait().ok() == Some(Some(0)); // Close may retire successfully before its response is lost.
    if !closed && result.is_ok() { Err(StoreError::Io) } else { result }
}
fn retry_delay(failures: u32) -> Duration { Duration::from_millis((25u64 << failures.min(8)).min(5000)) }
fn wait_for_signal(shared: &Shared, child: &crate::learning_process::Child, timeout: Duration) -> Result<(), StoreError> {
    let timeout = if shared.stop.load(Ordering::Acquire) { timeout.min(Duration::from_millis(250)) } else { timeout };
    match shared.signal.wait(child, timeout).map_err(|_| StoreError::Io)? {
        Reason::ChildExit => Err(StoreError::Io), Reason::Work | Reason::Timeout => Ok(()),
    }
}

/// Internal setup-tool mode. Explicit ordered arguments avoid environment/working-directory discovery.
pub fn companion_main(arguments: &[std::ffi::OsString]) -> i32 {
    let result = (|| {
        if !crate::system::desktop_settings_allowed() { return Err(learning_pipe::Error::Unauthorized); }
        if arguments.len() != 6 || arguments[0] != "--learning-companion" { return Err(learning_pipe::Error::Protocol); }
        let text = |index: usize| arguments[index].to_str().ok_or(learning_pipe::Error::Protocol);
        let endpoint = Endpoint::from_token(text(1)?)?;
        let parent = Peer::process(text(3)?.parse().map_err(|_| learning_pipe::Error::Protocol)?)?;
        if text(4)?.parse::<u64>().ok() != Some(parent.birth()) || !parent.is_alive() { return Err(learning_pipe::Error::Unauthorized); }
        learning_pipe::companion(endpoint, parent, text(2)?.to_owned(), PathBuf::from(&arguments[5]))
    })();
    if result.is_ok() { 0 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn security_policy_spawn_denial_is_sticky_but_missing_development_files_are_not() {
        for code in [5, 225, 367, 577, 637, 740, 1260, 4550, 4551, 4552, 4553, 4554, 4555, 4556, 4557, 4558, 4559, 4580, 4581, 4582] {
            let denied = AtomicBool::new(false); record_spawn_denial(&std::io::Error::from_raw_os_error(code), &denied);
            assert!(denied.load(Ordering::Acquire));
            record_spawn_denial(&std::io::Error::from_raw_os_error(2), &denied); assert!(denied.load(Ordering::Acquire));
        }
        let denied = AtomicBool::new(false); record_spawn_denial(&std::io::Error::from_raw_os_error(2), &denied);
        assert!(!denied.load(Ordering::Acquire));
    }
}
