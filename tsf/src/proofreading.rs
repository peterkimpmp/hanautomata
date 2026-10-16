//! Out-of-process review. No COM/Rc crosses threads; only an opaque owner-window wake does.
use std::{io::{Read, Write}, path::PathBuf, process::{Command, Stdio}, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc}, time::{Duration, Instant}};
use windows::core::Result;
use serde_json::Value;

pub(super) struct Session {
    dispatcher: crate::candidate::dispatch::Dispatcher,
    cancel: Arc<AtomicBool>,
}
impl Session {
    pub fn start(helper: PathBuf, request: Value, apply: impl FnOnce(Option<Value>) + 'static) -> Result<Self> {
        crate::exports::pin_for_worker()?;
        let dispatcher = crate::candidate::dispatch::Dispatcher::new()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let wake = dispatcher.defer(move || { apply(receiver.try_recv().ok().flatten()); });
        std::thread::spawn(move || {
            let result = run(helper, request, &stop);
            let _ = sender.send(result);
            wake.wake();
        });
        Ok(Self { dispatcher, cancel })
    }
}
impl Drop for Session {
    fn drop(&mut self) { self.cancel.store(true, Ordering::Release); self.dispatcher.cancel(); }
}
fn run(helper: PathBuf, request: Value, cancel: &AtomicBool) -> Option<Value> {
    use std::os::windows::process::CommandExt;
    let encoded = serde_json::to_vec(&request).ok()?;
    if encoded.len() > 65_536 || cancel.load(Ordering::Acquire) { return None; }
    let mut child = Command::new(helper).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .creation_flags(0x08000000).spawn().ok()?;
    let result = (|| {
        let mut stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let (send, receive) = mpsc::sync_channel(1);
        let write_failed = send.clone();
        // A helper that never reads stdin must not prevent focus cancellation or the deadline.
        std::thread::spawn(move || {
            if stdin.write_all(&encoded).and_then(|_| stdin.write_all(b"\n")).is_err() {
                let _ = write_failed.send(None);
            }
        });
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let value = stdout.take(65_537).read_to_end(&mut bytes).ok()
                .filter(|_| bytes.len() <= 65_536).and_then(|_| serde_json::from_slice::<Value>(&bytes).ok());
            let _ = send.send(value);
        });
        let start = Instant::now();
        loop {
            if cancel.load(Ordering::Acquire) || start.elapsed() > Duration::from_secs(300) { return None; }
            match receive.recv_timeout(Duration::from_millis(25)) {
                Ok(value) => return value,
                Err(mpsc::RecvTimeoutError::Disconnected) => return None,
                Err(mpsc::RecvTimeoutError::Timeout) => {},
            }
        }
    })();
    let _ = child.kill(); let _ = child.wait(); result
}
