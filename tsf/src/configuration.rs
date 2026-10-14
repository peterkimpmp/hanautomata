//! Explicit desktop Properties callback. Never called from activation or key processing.
//! No broker/elevation: a restricted or unknown host cannot launch the desktop settings writer.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{Result, GUID};
use windows::Win32::Foundation::{E_ABORT, E_ACCESSDENIED, E_FAIL, E_INVALIDARG, E_POINTER, HANDLE, HWND, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};

pub const DISPLAY_NAME: &str = "한오토마타 입력기 설정";
static SHOWING: AtomicBool = AtomicBool::new(false);

struct Showing<'a>(&'a AtomicBool);
impl Drop for Showing<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

fn enter(flag: &AtomicBool) -> Result<Showing<'_>> {
    flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| Showing(flag)).map_err(|_| E_FAIL.into())
}

fn validate_request(langid: u16, profile: Option<&GUID>) -> Result<()> {
    let profile = profile.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
    if langid != crate::guids::LANGID_KOREAN || *profile != crate::guids::PROFILE {
        return Err(E_INVALIDARG.into());
    }
    Ok(())
}

fn regular_file(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
}

fn editor_for_registered_module(module: &Path, registered: &Path) -> Option<PathBuf> {
    if !module.file_name()?.to_str()?.eq_ignore_ascii_case(crate::guids::DLL_NAME)
        || !crate::registration::same_file_path(module, registered) { return None; }
    let registered = std::fs::canonicalize(registered).ok()?;
    let exe = crate::registration::options_executable(&registered.with_file_name("hanautomata-ime-setup.exe"))?;
    use std::os::windows::fs::MetadataExt;
    let ui = std::fs::symlink_metadata(exe.parent()?).ok()?;
    if !ui.is_dir() || ui.file_attributes() & 0x400 != 0 { return None; }
    if !regular_file(&exe) || !regular_file(&exe.with_extension("exe.config")) { return None; }
    Some(exe)
}

struct ModalOwner { root: Option<HWND>, restore: bool }
impl ModalOwner {
    fn new(parent: HWND) -> Result<Self> {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        if parent.is_invalid() { return Ok(Self { root: None, restore: false }); }
        unsafe {
            if !IsWindow(Some(parent)).as_bool() { return Err(E_INVALIDARG.into()); }
            let root = GetAncestor(parent, GA_ROOT);
            if root.is_invalid() || GetWindowThreadProcessId(root, None) != GetCurrentThreadId() { return Err(E_INVALIDARG.into()); }
            if !IsWindowEnabled(root).as_bool() { return Ok(Self { root: Some(root), restore: false }); }
            let guard = Self { root: Some(root), restore: true };
            let _ = EnableWindow(root, false);
            if !IsWindow(Some(root)).as_bool() || IsWindowEnabled(root).as_bool() { return Err(E_FAIL.into()); }
            Ok(guard)
        }
    }
}
impl Drop for ModalOwner {
    fn drop(&mut self) {
        if !self.restore { return; }
        if let Some(root) = self.root {
            unsafe {
                if IsWindow(Some(root)).as_bool()
                    && GetWindowThreadProcessId(root, None) == windows::Win32::System::Threading::GetCurrentThreadId() {
                    let _ = EnableWindow(root, true);
                }
            }
        }
    }
}

fn owner_input(message: &MSG, owner: Option<HWND>) -> bool {
    let Some(root) = owner else { return false; };
    let input = (WM_KEYFIRST..=WM_KEYLAST).contains(&message.message)
        || (WM_MOUSEFIRST..=WM_MOUSELAST).contains(&message.message)
        || (WM_NCMOUSEMOVE..=WM_NCXBUTTONDBLCLK).contains(&message.message);
    input && !message.hwnd.is_invalid() && unsafe { GetAncestor(message.hwnd, GA_ROOT) == root }
}

fn wait_for_editor(child: &mut std::process::Child, owner: Option<HWND>) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    // The disabled owner cannot accept another Properties/OK/Cancel click. Dispatch other messages (including
    // COM's hidden windows), never retaining a text-service RefCell or filesystem lock across a reentrant call.
    loop {
        if let Some(exit) = child.try_wait().map_err(|_| windows::core::Error::from(E_FAIL))? {
            return if exit.success() { Ok(()) } else { Err(E_FAIL.into()) };
        }
        let wake = unsafe { MsgWaitForMultipleObjectsEx(Some(&[HANDLE(child.as_raw_handle())]), u32::MAX,
            QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
        if wake == WAIT_FAILED { return Err(windows::core::Error::from_win32()); }
        if wake == WAIT_OBJECT_0 { continue; }
        if wake.0 != WAIT_OBJECT_0.0 + 1 { return Err(E_FAIL.into()); }
        // Drain a bounded batch, then check process exit again. This prevents a message flood from starving exit.
        for _ in 0..64 {
            let mut message = MSG::default();
            if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() { break; }
            if message.message == WM_QUIT {
                unsafe { PostQuitMessage(message.wParam.0 as i32); }
                return Err(E_ABORT.into()); // Respect the host shutdown; the editor is not terminated here.
            }
            // Disabling input alone does not stop queued system keys delivered to an active root without focus.
            // Drop only this owner's input; keep paint, COM, timers and unrelated windows responsive.
            if owner_input(&message, owner) { continue; }
            unsafe { let _ = TranslateMessage(&message); DispatchMessageW(&message); }
        }
    }
}

pub fn show(parent: HWND, langid: u16, profile: Option<&GUID>) -> Result<()> {
    validate_request(langid, profile)?;
    if !crate::system::desktop_settings_allowed() { return Err(E_ACCESSDENIED.into()); }
    let _showing = enter(&SHOWING)?;
    let module = crate::exports::module_path()?;
    let registered = crate::registration::registered_machine_dll().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let exe = editor_for_registered_module(&module, &registered).ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let _owner = ModalOwner::new(parent)?;
    // A fixed absolute payload path, no shell, user-supplied arguments, registry command line, or inherited pipes.
    let mut child = std::process::Command::new(&exe).current_dir(exe.parent().ok_or(E_FAIL)?)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().map_err(|_| windows::core::Error::from(E_FAIL))?;
    // If waiting fails, leave the writer alive; do not terminate a user editing or saving settings.
    wait_for_editor(&mut child, _owner.root)
}

#[cfg(test)]
#[path = "configuration_window_tests.rs"]
mod window_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_language_profile_and_missing_pointer_are_rejected() {
        assert_eq!(validate_request(crate::guids::LANGID_KOREAN, None).unwrap_err().code(), E_POINTER);
        assert_eq!(validate_request(0x409, Some(&crate::guids::PROFILE)).unwrap_err().code(), E_INVALIDARG);
        assert_eq!(validate_request(crate::guids::LANGID_KOREAN, Some(&GUID::zeroed())).unwrap_err().code(), E_INVALIDARG);
        assert!(validate_request(crate::guids::LANGID_KOREAN, Some(&crate::guids::PROFILE)).is_ok());
    }

    #[test]
    fn nested_properties_are_rejected_and_unwind_releases_the_guard() {
        let flag = AtomicBool::new(false);
        let _ = std::panic::catch_unwind(|| {
            let _guard = enter(&flag).unwrap();
            assert!(enter(&flag).is_err());
            panic!("injected callback failure");
        });
        assert!(enter(&flag).is_ok());
    }

    #[test]
    fn old_module_or_incomplete_payload_cannot_open_settings() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("hanautomata-config-한글 space-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let result = std::panic::catch_unwind(|| {
            for revision in ["old", "new"] {
                for dir in ["x64", "x86", "ui"] { std::fs::create_dir_all(root.join(revision).join(dir)).unwrap(); }
                for arch in ["x64", "x86"] { std::fs::write(root.join(revision).join(arch).join(crate::guids::DLL_NAME), b"fixture").unwrap(); }
            }
            let old = root.join("old/x64").join(crate::guids::DLL_NAME);
            let new = root.join("new/x64").join(crate::guids::DLL_NAME);
            let x86 = root.join("new/x86").join(crate::guids::DLL_NAME);
            let exe = root.join("new/ui/Hanautomata.ImeOptions.exe");
            std::fs::write(&exe, b"fixture").unwrap();
            assert!(editor_for_registered_module(&new, &new).is_none(), "config is required");
            std::fs::write(exe.with_extension("exe.config"), b"fixture").unwrap();
            assert!(editor_for_registered_module(&old, &new).is_none(), "stale loaded module");
            assert!(editor_for_registered_module(&x86, &new).is_none(), "other architecture");
            let exe = std::fs::canonicalize(exe).unwrap();
            assert_eq!(editor_for_registered_module(&new, &new), Some(exe.clone()));
            assert_eq!(editor_for_registered_module(&x86, &x86), Some(exe));
        });
        // Only fixed files inside this newly created fixture; no recursive deletion or shared test directory.
        for revision in ["old", "new"] {
            for arch in ["x64", "x86"] {
                let dir = root.join(revision).join(arch);
                std::fs::remove_file(dir.join(crate::guids::DLL_NAME)).unwrap(); std::fs::remove_dir(dir).unwrap();
            }
            let ui = root.join(revision).join("ui");
            if revision == "new" {
                std::fs::remove_file(ui.join("Hanautomata.ImeOptions.exe")).unwrap();
                std::fs::remove_file(ui.join("Hanautomata.ImeOptions.exe.config")).unwrap();
            }
            std::fs::remove_dir(ui).unwrap(); std::fs::remove_dir(root.join(revision)).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    }

    #[test]
    #[ignore = "headless child fixture; invoked explicitly by the wait tests"]
    fn child_process_marker() {
        // A headless child only; the main test process never receives this environment variable.
        if std::env::var_os("HANAUTOMATA_TEST_CONFIG_CHILD_DELAY").is_some() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if std::env::var_os("HANAUTOMATA_TEST_CONFIG_CHILD_FAIL").is_some() { std::process::exit(7); }
    }

    #[test]
    fn modal_wait_observes_process_exit() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, PM_REMOVE, WM_NULL};
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap(); }
        let mut results = Vec::new();
        for fail in [false, true] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--ignored", "--exact", "configuration::tests::child_process_marker"])
                .env("HANAUTOMATA_TEST_CONFIG_CHILD_DELAY", "1")
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            if fail { command.env("HANAUTOMATA_TEST_CONFIG_CHILD_FAIL", "1"); }
            let mut child = command.spawn().unwrap();
            // An input-queue wake must not be mistaken for process completion. Only this test thread's queue.
            let mut message = MSG::default();
            unsafe {
                let _ = PeekMessageW(&mut message, None, WM_NULL, WM_NULL, PM_NOREMOVE);
                PostThreadMessageW(GetCurrentThreadId(), WM_NULL, Default::default(), Default::default()).unwrap();
            }
            let waited = wait_for_editor(&mut child, None);
            unsafe { let _ = PeekMessageW(&mut message, None, WM_NULL, WM_NULL, PM_REMOVE); }
            let exited = child.try_wait().unwrap();
            if exited.is_none() { let _ = child.wait(); }
            assert!(exited.is_some(), "modal wait returned before exit: {waited:?}");
            results.push((waited.is_ok(), exited.unwrap().success(), !fail));
        }
        unsafe { CoUninitialize(); }
        for (wait, exit, expected) in results { assert_eq!(wait, expected); assert_eq!(exit, expected); }
    }
}
