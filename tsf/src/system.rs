//! What the text service needs from Windows outside TSF: the process it runs in (elevated, AppContainer), the tray app,
//! key state, the S0 log and the Caps Lock press it sends back.

use std::io::Write;
use std::path::PathBuf;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TokenIsAppContainer, TokenUser, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::Storage::FileSystem::GetLongPathNameW;
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

/// User settings are shared with the desktop editor. An AppContainer's LOCALAPPDATA is redirected.
/// This only resolves the location; the editor must publish read access. Never fall back to defaults at another path.
pub fn settings_dir() -> Result<PathBuf, &'static str> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::{System::Com::CoTaskMemFree, UI::Shell::{SHGetKnownFolderPath, FOLDERID_LocalAppData,
        KF_FLAG_DONT_VERIFY, KF_FLAG_NO_PACKAGE_REDIRECTION}};
    let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_NO_PACKAGE_REDIRECTION | KF_FLAG_DONT_VERIFY, None) }
        .map_err(|_| "user settings directory unavailable")?;
    if raw.0.is_null() { return Err("empty user settings directory"); }
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe { raw.as_wide() }));
    unsafe { CoTaskMemFree(Some(raw.0.cast())); }
    if !path.is_absolute() { return Err("user settings directory is not absolute"); }
    Ok(path.join("Hanautomata"))
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

/// Pass keys when the tray marker exists or its state cannot be established (W-17).
/// An older tray's private DACL may deny AppContainers; that is not evidence that the hook is absent.
pub fn tray_running() -> bool {
    let Some(sid) = user_sid() else { return true };
    tray_marker_blocks_input(&format!("Local\\Hanautomata.Instance.{sid}"))
}

pub fn tray_marker_blocks_input(name: &str) -> bool {
    let name = HSTRING::from(name);
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) } {
        Ok(handle) => {
            unsafe {
                let _ = CloseHandle(handle);
            }
            true
        }
        Err(error) => tray_probe_error_blocks_input(error.code()),
    }
}

fn tray_probe_error_blocks_input(error: windows::core::HRESULT) -> bool {
    error != windows::core::HRESULT::from_win32(2) // ERROR_FILE_NOT_FOUND is the only confirmed absence.
}

pub fn process_name() -> String {
    process_identity().unwrap_or_else(|| "<unknown>".into())
}

/// A truncated, unreadable or invalid identity must not silently bypass a configured app exclusion.
pub fn process_identity() -> Option<String> {
    static IDENTITY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    // The executable does not change during the process lifetime. In particular, never expand an 8.3 name per key.
    IDENTITY.get_or_init(resolve_process_identity).clone()
}

fn resolve_process_identity() -> Option<String> {
    let path = module_path_with(|buf| unsafe { GetModuleFileNameW(None, buf) })?;
    identity_from_path(path, |short| {
        let mut terminated = short.to_vec(); terminated.push(0);
        module_path_with(|buf| unsafe { GetLongPathNameW(PCWSTR(terminated.as_ptr()), Some(buf)) })
    })
}

fn identity_from_path(path: Vec<u16>, expand: impl FnOnce(&[u16]) -> Option<Vec<u16>>) -> Option<String> {
    fn basename(path: &[u16]) -> Option<String> {
        // NTFS may contain an unpaired surrogate in a directory. Only the executable basename is compared.
        let name = path.rsplit(|unit| matches!(*unit, 0x5c | 0x2f)).next()?;
        let name = String::from_utf16(name).ok()?;
        if name.is_empty() || name.chars().any(char::is_control) { return None; }
        Some(name)
    }
    let name = basename(&path)?;
    // Module paths retain the format used at launch, including 8.3 names. Expand a possible short basename.
    if name.contains('~') { basename(&expand(&path)?) } else { Some(name) }
}

fn module_path_with(mut read: impl FnMut(&mut [u16]) -> u32) -> Option<Vec<u16>> {
    let mut size = 520;
    loop {
        let mut buffer = vec![0u16; size];
        let count = read(&mut buffer) as usize;
        if count == 0 { return None; }
        if count < buffer.len() {
            buffer.truncate(count);
            return if buffer.contains(&0) { None } else { Some(buffer) };
        }
        if size == 32768 { return None; }
        size = (size * 2).max(count.saturating_add(1)).min(32768);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn inaccessible_tray_markers_are_not_mistaken_for_absent_trays() {
        use windows::core::HRESULT;
        assert!(!tray_probe_error_blocks_input(HRESULT::from_win32(2)), "confirmed absent marker must allow TSF");
        for error in [5, 6, 8, 123] {
            assert!(tray_probe_error_blocks_input(HRESULT::from_win32(error)), "unknown marker state must prevent double input: {error}");
        }
    }

    #[test]
    fn full_module_path_is_read_past_the_old_520_unit_limit() {
        let expected: Vec<u16> = format!("C:\\{}\\code.exe", "long-directory\\".repeat(80)).encode_utf16().collect();
        let actual = module_path_with(|buffer| {
            let count = expected.len().min(buffer.len());
            buffer[..count].copy_from_slice(&expected[..count]);
            count as u32
        }).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn unreadable_or_persistently_truncated_module_path_is_unknown() {
        assert!(module_path_with(|_| 0).is_none());
        assert!(module_path_with(|buffer| buffer.len() as u32).is_none());
    }

    #[test]
    fn aliases_expand_and_invalid_identities_are_not_lossily_repaired() {
        let wide = |text: &str| text.encode_utf16().collect::<Vec<_>>();
        assert_eq!(identity_from_path(wide(r"C:\apps\HANGL~1.EXE"), |_| Some(wide(r"C:\apps\한글 도구.exe"))), Some("한글 도구.exe".into()));
        assert!(identity_from_path(wide(r"C:\apps\HANGL~1.EXE"), |_| None).is_none());
        assert!(identity_from_path(vec![0xd800], |_| None).is_none());
        assert!(module_path_with(|buffer| { buffer[..3].copy_from_slice(&[65, 0, 66]); 3 }).is_none());
        assert_eq!(identity_from_path(wide(r"C:\apps\code.exe"), |_| panic!("long name must not query filesystem")), Some("code.exe".into()));
    }

    #[test]
    fn current_executable_has_a_complete_identity() {
        let name = process_identity().expect("current executable identity");
        assert!(name.ends_with(".exe") && !name.contains('\\'));
    }

    #[test]
    fn invalid_utf16_in_a_parent_directory_does_not_erase_a_valid_basename() {
        let mut path: Vec<u16> = r"C:\".encode_utf16().collect();
        path.push(0xd800);
        path.extend(r"\code.exe".encode_utf16());
        assert_eq!(identity_from_path(path, |_| None), Some("code.exe".into()));
    }
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
