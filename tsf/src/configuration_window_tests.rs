//! Own hidden windows and headless child processes only; no existing application or user settings are touched.
use super::*;
use std::cell::Cell;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId};

thread_local! { static MESSAGES: Cell<(u32, u32)> = const { Cell::new((0, 0)) }; }
unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    MESSAGES.with(|counts| {
        let (keys, paints) = counts.get();
        counts.set((keys + u32::from(message == WM_KEYDOWN || message == WM_SYSKEYDOWN), paints + u32::from(message == WM_PAINT)));
    });
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}

struct TestWindow { hwnd: HWND, class: HSTRING, instance: HINSTANCE }
impl TestWindow {
    fn new() -> Self {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).unwrap().into();
            let class = HSTRING::from(format!("HanautomataPropertiesTest{}", GetCurrentThreadId()));
            let wc = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance, lpszClassName: windows::core::PCWSTR(class.as_ptr()), ..Default::default() };
            assert_ne!(RegisterClassW(&wc), 0);
            let hwnd = CreateWindowExW(Default::default(), &class, &HSTRING::from("test"), WS_OVERLAPPED,
                0, 0, 100, 100, None, None, Some(instance), None).unwrap();
            Self { hwnd, class, instance }
        }
    }
}
impl Drop for TestWindow {
    fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.hwnd); let _ = UnregisterClassW(&self.class, Some(self.instance)); } }
}

fn child() -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "configuration::tests::child_process_marker"])
        .env("HANAUTOMATA_TEST_CONFIG_CHILD_DELAY", "1")
        .env_remove("HANAUTOMATA_TEST_CONFIG_CHILD_FAIL")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().unwrap()
}

#[test]
fn own_owner_is_modal_and_restored_on_unwind_without_enabling_a_disabled_owner() {
    let window = TestWindow::new();
    assert!(unsafe { IsWindowEnabled(window.hwnd) }.as_bool());
    let outcome = std::panic::catch_unwind(|| {
        let _owner = ModalOwner::new(window.hwnd).unwrap();
        assert!(!unsafe { IsWindowEnabled(window.hwnd) }.as_bool());
        panic!("injected failure after disabling owner");
    });
    assert!(outcome.is_err());
    assert!(unsafe { IsWindowEnabled(window.hwnd) }.as_bool());
    unsafe { let _ = EnableWindow(window.hwnd, false); }
    drop(ModalOwner::new(window.hwnd).unwrap());
    assert!(!unsafe { IsWindowEnabled(window.hwnd) }.as_bool());
    let raw = window.hwnd.0 as usize;
    assert_eq!(std::thread::spawn(move || ModalOwner::new(HWND(raw as *mut _)).err().unwrap().code()).join().unwrap(), E_INVALIDARG);
}

#[test]
fn wait_dispatches_owned_window_key_and_paint_messages_before_child_exit() {
    let window = TestWindow::new();
    MESSAGES.with(|counts| counts.set((0, 0)));
    let mut child = child();
    unsafe {
        PostMessageW(Some(window.hwnd), WM_KEYDOWN, WPARAM(0x41), LPARAM(0)).unwrap();
        PostMessageW(Some(window.hwnd), WM_PAINT, WPARAM(0), LPARAM(0)).unwrap();
    }
    let waited = wait_for_editor(&mut child, None);
    let ended = child.try_wait().unwrap();
    if ended.is_none() { let _ = child.wait(); }
    assert!(waited.is_ok()); assert!(ended.unwrap().success());
    MESSAGES.with(|counts| { let (keys, paints) = counts.get(); assert!(keys >= 1 && paints >= 1, "not dispatched: {keys}/{paints}"); });
}

#[test]
fn host_quit_is_reposted_without_killing_the_editor() {
    let mut child = child();
    unsafe { PostQuitMessage(73); }
    let waited = wait_for_editor(&mut child, None);
    let mut message = MSG::default();
    let quit = unsafe { PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE) }.as_bool();
    let ended = child.wait().unwrap();
    assert_eq!(waited.unwrap_err().code(), E_ABORT);
    assert!(quit); assert_eq!(message.wParam.0, 73); assert!(ended.success());
}

#[test]
fn disabled_owner_does_not_receive_alt_f4_while_waiting() {
    let window = TestWindow::new();
    let owner = ModalOwner::new(window.hwnd).unwrap();
    MESSAGES.with(|counts| counts.set((0, 0)));
    let mut child = child();
    unsafe {
        PostMessageW(Some(window.hwnd), WM_SYSKEYDOWN, WPARAM(0x73), LPARAM(1 << 29)).unwrap();
        PostMessageW(Some(window.hwnd), WM_PAINT, WPARAM(0), LPARAM(0)).unwrap();
    }
    let waited = wait_for_editor(&mut child, owner.root);
    let _ = child.wait();
    assert!(waited.is_ok());
    MESSAGES.with(|counts| { let (keys, paints) = counts.get(); assert_eq!(keys, 0, "disabled owner received keyboard input"); assert!(paints >= 1); });
    assert!(unsafe { IsWindow(Some(window.hwnd)) }.as_bool());
    drop(owner);
}
