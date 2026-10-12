//! What the text service needs from Windows outside TSF: the process it runs in (elevated, AppContainer), the tray app,
//! key state, the S0 log and the Caps Lock press it sends back.

use std::io::Write;
use std::path::PathBuf;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TokenIsAppContainer, TokenUser, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenMutexW, OpenProcessToken, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

use crate::shell::Mods;

/// `%LOCALAPPDATA%\Hanautomata` — the tray app's folder; the learning file is shared with it (W-17).
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Hanautomata")
}

pub fn home_dir() -> String {
    std::env::var("USERPROFILE").unwrap_or_default()
}

pub fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

fn down(key: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(key.0 as i32) < 0 }
}

pub fn caps_lock_on() -> bool {
    unsafe { GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 }
}

pub fn modifiers() -> Mods {
    Mods {
        shift: down(VK_SHIFT),
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        win: down(VK_LWIN) || down(VK_RWIN),
        caps: caps_lock_on(),
    }
}

/// One Caps Lock press and release (W-22: the short press must leave the toggle as it was).
pub fn send_caps_lock() -> bool {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_CAPITAL,
                wScan: 0x3A,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == 2 }
}

struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn token() -> Option<Token> {
    let mut handle = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle).ok()? };
    Some(Token(handle))
}

fn token_u32(class: windows::Win32::Security::TOKEN_INFORMATION_CLASS) -> Option<u32> {
    let t = token()?;
    let mut value = 0u32;
    let mut len = 0u32;
    unsafe {
        GetTokenInformation(t.0, class, Some(&mut value as *mut u32 as *mut _), 4, &mut len).ok()?;
    }
    Some(value)
}

/// The process runs elevated (W-14: it learns nothing and records nothing there).
pub fn elevated() -> bool {
    let Some(t) = token() else { return false };
    let mut e = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    unsafe {
        GetTokenInformation(
            t.0,
            TokenElevation,
            Some(&mut e as *mut TOKEN_ELEVATION as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok()
            && e.TokenIsElevated != 0
    }
}

/// The process is an AppContainer (Start search, Store apps; W-10).
pub fn app_container() -> bool {
    token_u32(TokenIsAppContainer).unwrap_or(0) != 0
}

fn user_sid() -> Option<String> {
    let t = token()?;
    let mut len = 0u32;
    unsafe {
        let _ = GetTokenInformation(t.0, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        GetTokenInformation(t.0, TokenUser, Some(buf.as_mut_ptr() as *mut _), len, &mut len).ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(text.0 as *mut _)));
        sid
    }
}

/// The tray app (hook build) is running for this user: then it judges and the input method passes keys (W-17). It holds
/// `Local\Hanautomata.Instance.<SID>` while it runs (src/App.cs).
pub fn tray_running() -> bool {
    let Some(sid) = user_sid() else { return false };
    let name = HSTRING::from(format!("Local\\Hanautomata.Instance.{sid}"));
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) } {
        Ok(handle) => {
            unsafe {
                let _ = CloseHandle(handle);
            }
            true
        }
        Err(_) => false,
    }
}

pub fn process_name() -> String {
    let mut buf = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
    let path = String::from_utf16_lossy(&buf[..n]);
    path.rsplit('\\').next().unwrap_or("").to_string()
}

/// S0 record: one line per event, no typed text (C-OPS-02). Writes fail silently where the folder is not writable
/// (AppContainer).
pub fn log(event: &str) {
    let dir = data_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("ime-s0.log"))
    {
        let _ = writeln!(
            f,
            "{} pid={} tid={} {} {}",
            now_ms(),
            unsafe { GetCurrentProcessId() },
            unsafe { GetCurrentThreadId() },
            process_name(),
            event
        );
    }
}
