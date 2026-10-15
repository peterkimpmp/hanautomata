//! In-process queue/stop signal plus exact child exit. No per-keystroke timers, polling, or inherited handles.
use std::{os::windows::{io::{AsRawHandle, FromRawHandle, OwnedHandle}, ffi::OsStrExt}, path::Path, sync::Arc, task::Wake, time::Duration};
use windows::{core::PCWSTR, Win32::{Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects},
    Storage::FileSystem::{FindFirstChangeNotificationW, FindNextChangeNotification, FindCloseChangeNotification,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE}}};
pub(super) struct Signal(OwnedHandle);
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reason { Work, StoreChanged, ChildExit, Timeout }
/// Worker-local notification only. The directory event is a hint; authoritative data still comes from the child.
pub(super) struct ChangeWatch(HANDLE);
impl ChangeWatch {
    pub fn new(directory: &Path) -> std::io::Result<Self> {
        let mut path: Vec<_> = directory.as_os_str().encode_wide().collect();
        if !directory.is_absolute() || path.contains(&0) { return Err(std::io::ErrorKind::InvalidInput.into()); }
        path.push(0);
        unsafe { FindFirstChangeNotificationW(PCWSTR(path.as_ptr()), false,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE) }
            .map(Self).map_err(std::io::Error::from)
    }
    pub fn rearm(&self) -> std::io::Result<()> {
        unsafe { FindNextChangeNotification(self.0) }.map_err(std::io::Error::from)
    }
}
impl Drop for ChangeWatch { fn drop(&mut self) { unsafe { let _ = FindCloseChangeNotification(self.0); } } }
impl Signal {
    pub fn new() -> std::io::Result<Self> {
        let handle = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(std::io::Error::from)?;
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle.0) }))
    }
    pub fn notify(&self) { unsafe { let _ = SetEvent(HANDLE(self.0.as_raw_handle())); } }
    pub fn wait(&self, child: &crate::learning_process::Child, timeout: Duration) -> std::io::Result<Reason> {
        self.wait_handle(child.handle(), timeout)
    }
    pub fn wait_with_store(&self, child: &crate::learning_process::Child, watch: &ChangeWatch, timeout: Duration) -> std::io::Result<Reason> {
        self.wait_handles(child.handle(), Some(watch.0), timeout)
    }
    fn wait_handle(&self, child: HANDLE, timeout: Duration) -> std::io::Result<Reason> {
        self.wait_handles(child, None, timeout)
    }
    fn wait_handles(&self, child: HANDLE, store: Option<HANDLE>, timeout: Duration) -> std::io::Result<Reason> {
        let handles = [child, HANDLE(self.0.as_raw_handle()), store.unwrap_or_default()];
        let handles = &handles[..if store.is_some() { 3 } else { 2 }];
        match unsafe { WaitForMultipleObjects(handles, false, timeout.as_millis().min(u32::MAX as u128 - 1) as u32) } {
            WAIT_OBJECT_0 => Ok(Reason::ChildExit),
            result if result.0 == WAIT_OBJECT_0.0 + 1 => Ok(Reason::Work),
            result if store.is_some() && result.0 == WAIT_OBJECT_0.0 + 2 => Ok(Reason::StoreChanged),
            WAIT_TIMEOUT => Ok(Reason::Timeout),
            _ => Err(std::io::Error::last_os_error()),
        }
    }
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) { self.notify(); }
    fn wake_by_ref(self: &Arc<Self>) { self.notify(); }
}
#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
    #[test]
    #[ignore = "owned child exit fixture"]
    fn exit_fixture() {}
    #[test]
    fn signal_remembers_early_notification_coalesces_and_observes_exact_child_exit() {
        let signal = Arc::new(Signal::new().unwrap());
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, std::process::id()).unwrap() };
        let process = unsafe { OwnedHandle::from_raw_handle(raw.0) }; let process = HANDLE(process.as_raw_handle());
        signal.notify(); signal.notify();
        assert_eq!(signal.wait_handle(process, Duration::ZERO).unwrap(), Reason::Work);
        assert_eq!(signal.wait_handle(process, Duration::ZERO).unwrap(), Reason::Timeout);
        let other = signal.clone(); let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30)); other.notify();
        });
        assert_eq!(signal.wait_handle(process, Duration::from_secs(2)).unwrap(), Reason::Work); worker.join().unwrap();
        let child = crate::learning_process::Child::spawn(&std::env::current_exe().unwrap(),
            &["--ignored".into(), "--exact".into(), "learning_wake::tests::exit_fixture".into()]).unwrap();
        assert_eq!(signal.wait(&child, Duration::from_secs(5)).unwrap(), Reason::ChildExit);
        assert_eq!(child.try_wait().unwrap(), Some(0));
    }
}
