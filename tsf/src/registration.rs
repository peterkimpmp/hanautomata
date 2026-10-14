//! Registration in the four steps of W-02, each reported on its own so S0 can record which work for a standard user:
//!   1. COM in-process server under HKCU\Software\Classes (per user)
//!   2. TSF profile (ITfInputProcessorProfileMgr::RegisterProfile)
//!   3. Categories: keyboard, display attributes, immersive (AppContainer), system tray, input-mode compartment
//!   4. Turn it on for this user (InstallLayoutOrTip)
//!
//! Microsoft asks text services not to write TSF registrations to the registry directly. `--ctf-hkcu` (the TextVN route:
//! steps 2–3 written under HKCU\Software\Microsoft\CTF\TIP, again after step 4, then 5 the value in the language list
//! Settings reads) is only an S0 experiment for when the API refuses.
//!
//! `cli` is the `hanautomata-ime-setup` tool: register · unregister · status · selftest · active · activate.

use windows::core::{Interface, GUID, HRESULT, HSTRING, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, E_ACCESSDENIED, E_FAIL, E_INVALIDARG, LPARAM, WIN32_ERROR, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};

use crate::guids::{braced, tip_string, CLSID_TEXT_SERVICE, DESCRIPTION, DLL_NAME, LANGID_KOREAN, PROFILE};

pub const CATEGORIES: [(&str, GUID); 5] = [
    ("keyboard", GUID_TFCAT_TIP_KEYBOARD),
    ("display attributes", GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER),
    ("immersive (AppContainer)", GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT),
    ("system tray", GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT),
    ("input-mode compartment", GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT),
];

/// One step's outcome for the S0 record.
pub struct Step {
    pub name: String,
    pub result: std::result::Result<String, HRESULT>,
}

#[derive(Default)]
pub struct Report {
    pub steps: Vec<Step>,
}

impl Report {
    fn push(&mut self, name: impl Into<String>, result: std::result::Result<String, HRESULT>) {
        self.steps.push(Step {
            name: name.into(),
            result,
        });
    }

    pub fn ok(&self) -> bool {
        self.steps.iter().all(|s| s.result.is_ok())
    }

    pub fn first_error(&self) -> HRESULT {
        self.steps
            .iter()
            .find_map(|s| s.result.as_ref().err().copied())
            .unwrap_or(E_FAIL)
    }

    pub fn print(&self) {
        for s in &self.steps {
            match &s.result {
                Ok(detail) => println!("  ok    {} {}", s.name, detail),
                Err(hr) => println!("  FAIL  {} 0x{:08X} {}", s.name, hr.0 as u32, hr.message()),
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn win32(e: WIN32_ERROR) -> std::result::Result<(), HRESULT> {
    if e == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(e.to_hresult())
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn create_key(root: HKEY, path: &str) -> std::result::Result<Key, HRESULT> {
    let mut key = HKEY::default();
    let path = HSTRING::from(path);
    win32(unsafe {
        RegCreateKeyExW(
            root,
            &path,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    })?;
    Ok(Key(key))
}

fn key_exists(root: HKEY, path: &str) -> bool {
    let mut key = HKEY::default();
    let path = HSTRING::from(path);
    let opened = unsafe { RegOpenKeyExW(root, &path, None, KEY_READ, &mut key) } == ERROR_SUCCESS;
    if opened {
        drop(Key(key));
    }
    opened
}

fn open_key(root: HKEY, path: &str) -> Option<Key> {
    let mut key = HKEY::default();
    let path = HSTRING::from(path);
    (unsafe { RegOpenKeyExW(root, &path, None, KEY_READ, &mut key) } == ERROR_SUCCESS).then_some(Key(key))
}

fn subkeys(key: &Key) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0u32.. {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        if unsafe { RegEnumKeyExW(key.0, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) }
            != ERROR_SUCCESS
        {
            break;
        }
        names.push(String::from_utf16_lossy(&name[..len as usize]));
    }
    names
}

/// Value names, with the data of the DWORD ones.
fn values(key: &Key) -> Vec<(String, Option<u32>)> {
    let mut found = Vec::new();
    for index in 0u32.. {
        let mut name = [0u16; 512];
        let mut len = name.len() as u32;
        let mut kind = 0u32;
        if unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                Some(&mut kind),
                None,
                None,
            )
        } != ERROR_SUCCESS
        {
            break;
        }
        let name = String::from_utf16_lossy(&name[..len as usize]);
        let dword = (kind == REG_DWORD.0).then(|| {
            let mut data = [0u8; 4];
            let mut size = data.len() as u32;
            let value = HSTRING::from(name.as_str());
            unsafe {
                let _ = RegQueryValueExW(key.0, &value, None, None, Some(data.as_mut_ptr()), Some(&mut size));
            }
            u32::from_le_bytes(data)
        });
        found.push((name, dword));
    }
    found
}

fn set_string(key: &Key, name: Option<&str>, value: &str) -> std::result::Result<(), HRESULT> {
    let mut data: Vec<u16> = wide(value);
    data.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
    let name = name.map(HSTRING::from);
    let name_ptr = name.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null());
    win32(unsafe { RegSetValueExW(key.0, name_ptr, None, REG_SZ, Some(bytes)) })
}

fn set_dword(key: &Key, name: &str, value: u32) -> std::result::Result<(), HRESULT> {
    let name = HSTRING::from(name);
    win32(unsafe { RegSetValueExW(key.0, &name, None, REG_DWORD, Some(&value.to_le_bytes())) })
}

fn com_key() -> String {
    format!("Software\\Classes\\CLSID\\{}", braced(&CLSID_TEXT_SERVICE))
}

fn ctf_key() -> String {
    format!("Software\\Microsoft\\CTF\\TIP\\{}", braced(&CLSID_TEXT_SERVICE))
}

/// Step 1: the in-process COM server for this user.
fn register_com(dll: &str) -> std::result::Result<String, HRESULT> {
    register_com_at(HKEY_CURRENT_USER, "HKCU", dll)
}

fn register_com_at(root: HKEY, label: &str, dll: &str) -> std::result::Result<String, HRESULT> {
    let base = create_key(root, &com_key())?;
    set_string(&base, None, DESCRIPTION)?;
    let server = create_key(root, &format!("{}\\InprocServer32", com_key()))?;
    set_string(&server, None, dll)?;
    set_string(&server, Some("ThreadingModel"), "Apartment")?;
    Ok(format!("{label}\\{}", com_key()))
}

fn profiles() -> windows::core::Result<ITfInputProcessorProfileMgr> {
    unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }
}

fn categories() -> windows::core::Result<ITfCategoryMgr> {
    unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }
}

/// Step 2: the TSF profile through the API.
fn register_profile(dll: &str, enabled_by_default: bool) -> std::result::Result<String, HRESULT> {
    let manager = profiles().map_err(|e| e.code())?;
    unsafe {
        manager
            .RegisterProfile(
                &CLSID_TEXT_SERVICE,
                LANGID_KOREAN,
                &PROFILE,
                &wide(DESCRIPTION),
                &wide(dll),
                0,
                HKL::default(),
                0,
                enabled_by_default,
                0,
            )
            .map_err(|e| e.code())?;
    }
    Ok(format!("{} {}", braced(&PROFILE), DESCRIPTION))
}

/// Step 2 alternative (S0 experiment): the profile written under HKCU\Software\Microsoft\CTF\TIP.
fn register_profile_hkcu(dll: &str) -> std::result::Result<String, HRESULT> {
    let path = format!(
        "{}\\LanguageProfile\\0x{:08x}\\{}",
        ctf_key(),
        LANGID_KOREAN,
        braced(&PROFILE)
    );
    let key = create_key(HKEY_CURRENT_USER, &path)?;
    set_string(&key, Some("Description"), DESCRIPTION)?;
    set_string(&key, Some("IconFile"), dll)?;
    set_dword(&key, "IconIndex", 0)?;
    set_dword(&key, "Enable", 1)?;
    Ok(format!("HKCU\\{path}"))
}

/// Step 3: categories through the API.
fn register_categories() -> Vec<(String, std::result::Result<String, HRESULT>)> {
    let manager = match categories() {
        Ok(m) => m,
        Err(e) => return vec![("categories".into(), Err(e.code()))],
    };
    CATEGORIES
        .iter()
        .map(|(name, cat)| {
            let r = unsafe { manager.RegisterCategory(&CLSID_TEXT_SERVICE, cat, &CLSID_TEXT_SERVICE) };
            (format!("category {name}"), r.map(|_| braced(cat)).map_err(|e| e.code()))
        })
        .collect()
}

/// Step 3 alternative (S0 experiment).
fn register_categories_hkcu() -> Vec<(String, std::result::Result<String, HRESULT>)> {
    CATEGORIES
        .iter()
        .map(|(name, cat)| {
            let a = format!(
                "{}\\Category\\Category\\{}\\{}",
                ctf_key(),
                braced(cat),
                braced(&CLSID_TEXT_SERVICE)
            );
            let b = format!(
                "{}\\Category\\Item\\{}\\{}",
                ctf_key(),
                braced(&CLSID_TEXT_SERVICE),
                braced(cat)
            );
            let r = create_key(HKEY_CURRENT_USER, &a)
                .and_then(|_| create_key(HKEY_CURRENT_USER, &b))
                .map(|_| "HKCU".to_string());
            (format!("category {name} (HKCU)"), r)
        })
        .collect()
}

type InstallLayoutOrTip = unsafe extern "system" fn(PCWSTR, u32) -> windows::Win32::Foundation::BOOL;
const ILOT_UNINSTALL: u32 = 0x0000_0001;

/// Step 4: add (or remove) the input method in this user's list, as Settings > Language does.
fn install_layout(flags: u32) -> std::result::Result<String, HRESULT> {
    unsafe {
        let module = LoadLibraryW(&HSTRING::from("input.dll")).map_err(|e| e.code())?;
        let proc = GetProcAddress(module, PCSTR(c"InstallLayoutOrTip".as_ptr() as *const u8)).ok_or(E_FAIL)?;
        let install: InstallLayoutOrTip = std::mem::transmute(proc);
        let tip = HSTRING::from(tip_string());
        if install(PCWSTR(tip.as_ptr()), flags).as_bool() {
            Ok(tip_string())
        } else {
            Err(windows::core::Error::from_win32().code())
        }
    }
}

/// The language list Settings and Win+Space read: one subkey per language (`ko`, `en-US`), one value per keyboard
/// (`0412:{CLSID}{profile}` = its order). InstallLayoutOrTip writes a different store; TextVN adds its value here too.
const USER_PROFILE: &str = "Control Panel\\International\\User Profile";

/// Languages in this user's list that hold our keyboard value, or with `korean` the ones that hold any Korean keyboard.
fn language_tags(korean: bool) -> Vec<String> {
    let Some(root) = open_key(HKEY_CURRENT_USER, USER_PROFILE) else {
        return Vec::new();
    };
    let tip = tip_string();
    let prefix = format!("{LANGID_KOREAN:04X}:");
    subkeys(&root)
        .into_iter()
        .filter(|tag| {
            open_key(HKEY_CURRENT_USER, &format!("{USER_PROFILE}\\{tag}")).is_some_and(|key| {
                values(&key).iter().any(|(name, _)| {
                    if korean {
                        name.to_uppercase().starts_with(&prefix)
                    } else {
                        name.eq_ignore_ascii_case(&tip)
                    }
                })
            })
        })
        .collect()
}

/// Tells running programs the language settings changed, as Settings does after editing the list.
fn broadcast_international() {
    let area = HSTRING::from("International");
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
    }
}

/// `--ctf-hkcu` step 5: our value in the Korean language the user already has (never adds a language).
fn add_to_language_list() -> std::result::Result<String, HRESULT> {
    let tag = language_tags(true).into_iter().next().ok_or(E_FAIL)?;
    let path = format!("{USER_PROFILE}\\{tag}");
    let existing = open_key(HKEY_CURRENT_USER, &path).map(|key| values(&key)).unwrap_or_default();
    if existing.iter().any(|(name, _)| name.eq_ignore_ascii_case(&tip_string())) {
        return Ok(format!("{tag}: already listed"));
    }
    let order = existing
        .iter()
        .filter(|(name, _)| name.contains(':'))
        .filter_map(|(_, order)| *order)
        .max()
        .unwrap_or(0)
        + 1;
    let key = create_key(HKEY_CURRENT_USER, &path)?;
    set_dword(&key, &tip_string(), order)?;
    broadcast_international();
    Ok(format!("HKCU\\{path} {}={order}", tip_string()))
}

/// Removes only our value, from every language that has it.
fn remove_from_language_list() -> Option<std::result::Result<String, HRESULT>> {
    let tags = language_tags(false);
    if tags.is_empty() {
        return None;
    }
    let value = HSTRING::from(tip_string());
    for tag in &tags {
        let path = HSTRING::from(format!("{USER_PROFILE}\\{tag}"));
        if let Err(hr) = win32(unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &path, &value) }) {
            return Some(Err(hr));
        }
    }
    broadcast_international();
    Some(Ok(tags.join(",")))
}

fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// Steps 1–4. With `ctf_hkcu`, steps 2–3 fall back to the HKCU registry route when the API refuses, in TextVN's order:
/// InstallLayoutOrTip rewrites this user's CTF tree (on Windows 11 26H2 it removed every HKCU key written before it,
/// `probe-hkcu --enable` 2026-10-12), so the HKCU profile is written again after step 4, the categories only after it, and
/// step 5 puts the keyboard in the language list.
pub fn register(dll: &str, ctf_hkcu: bool) -> Report {
    init_com();
    let mut report = Report::default();
    report.push("1 COM server (HKCU)", register_com(dll));
    let profile = register_profile(dll, true);
    let cats = register_categories();
    if !ctf_hkcu {
        report.push("2 TSF profile (API)", profile);
        for (name, r) in cats {
            report.push(format!("3 {name}"), r);
        }
        report.push("4 enable for this user (InstallLayoutOrTip)", install_layout(0));
        return report;
    }
    let refused = |r: &std::result::Result<String, HRESULT>| r.as_ref().err().map(|hr| format!("0x{:08X}", hr.0 as u32));
    let profile_refused = refused(&profile);
    match &profile_refused {
        Some(hr) => {
            report.push("2 TSF profile (API) — refused, using HKCU", Ok(hr.clone()));
            report.push("2 TSF profile (HKCU)", register_profile_hkcu(dll));
        }
        None => report.push("2 TSF profile (API)", profile),
    }
    let cats_refused = cats.iter().any(|(_, r)| r.is_err());
    for (name, r) in cats {
        match refused(&r) {
            Some(hr) => report.push(format!("3 {name} (API) — refused, using HKCU after step 4"), Ok(hr)),
            None => report.push(format!("3 {name} (API)"), r),
        }
    }
    report.push("4 enable for this user (InstallLayoutOrTip)", install_layout(0));
    if profile_refused.is_some() {
        report.push("2 TSF profile (HKCU) written again after step 4", register_profile_hkcu(dll));
    }
    if cats_refused {
        for (name, r) in register_categories_hkcu() {
            report.push(format!("3 {name} after step 4"), r);
        }
    }
    report.push("5 language list (User Profile)", add_to_language_list());
    report
}

/// D2: machine registration must not alter the administrator account's language list.
/// The native x64/x86 tools each write their own COM registry view.
pub fn register_machine(dll: &str) -> Report {
    let mut report = Report::default();
    if !crate::system::elevated() {
        report.push("machine registration requires elevation; no changes made", Err(E_ACCESSDENIED));
        return report;
    }
    // Validate before making any registration changes.
    report.push(
        "DLL exists",
        if std::path::Path::new(dll).is_file() { Ok(dll.into()) } else { Err(E_INVALIDARG) },
    );
    if !report.ok() {
        return report;
    }
    init_com();
    report.push("COM server (HKLM)", register_com_at(HKEY_LOCAL_MACHINE, "HKLM", dll));
    if !report.ok() {
        return report;
    }
    // Match Microsoft's SampleIME: eligible by default, without changing the admin's selected input method.
    report.push("TSF profile (API; enabled by default)", register_profile(dll, true));
    if !report.ok() {
        return report;
    }
    for (name, result) in register_categories() {
        report.push(name, result);
    }
    report
}

/// Run in the intended user's normal session, after machine registration.
pub fn enable_user(enable: bool) -> Report {
    let mut report = Report::default();
    if crate::system::elevated() {
        report.push("user activation requires the intended user's non-elevated session", Err(E_ACCESSDENIED));
        return report;
    }
    if enable && (!key_exists(HKEY_LOCAL_MACHINE, &com_key()) || !key_exists(HKEY_LOCAL_MACHINE, &ctf_key())) {
        report.push("machine registration must exist before user activation", Err(E_FAIL));
        return report;
    }
    if enable {
        report.steps.extend(options_command(true).steps);
        if !report.ok() { return report; } // Do not add a profile whose settings could not be prepared.
    }
    init_com();
    // InstallLayoutOrTip can rewrite per-user CTF state; apply the requested enable state last.
    report.push("InstallLayoutOrTip (current user)", install_layout(if enable { 0 } else { ILOT_UNINSTALL }));
    if enable && !report.ok() {
        return report;
    }
    let result = profiles().and_then(|m| {
        let p: ITfInputProcessorProfiles = m.cast()?;
        unsafe { p.EnableLanguageProfile(&CLSID_TEXT_SERVICE, LANGID_KOREAN, &PROFILE, enable) }
    });
    report.push("EnableLanguageProfile (current user)", result.map(|_| enable.to_string()).map_err(|e| e.code()));
    if !enable {
        if let Some(result) = remove_from_language_list() {
            report.push("remove own language-list entry", result);
        }
    } else if report.ok() {
        report.steps.extend(user_readiness().steps);
    }
    report
}

pub(crate) fn options_executable(setup: &std::path::Path) -> Option<std::path::PathBuf> {
    let arch = setup.parent()?;
    let name = arch.file_name()?.to_str()?;
    if !name.eq_ignore_ascii_case("x64") && !name.eq_ignore_ascii_case("x86") { return None; }
    Some(arch.parent()?.join("ui").join("Hanautomata.ImeOptions.exe"))
}
fn registered_options_executable(setup: &std::path::Path, registered_dll: &std::path::Path) -> Option<std::path::PathBuf> {
    if !setup.file_name()?.to_str()?.eq_ignore_ascii_case("hanautomata-ime-setup.exe") { return None; }
    let setup_real = std::fs::canonicalize(setup).ok()?;
    let dll_real = std::fs::canonicalize(registered_dll).ok()?;
    if !same_file_path(setup_real.parent()?, dll_real.parent()?) { return None; }
    options_executable(&registered_dll.with_file_name("hanautomata-ime-setup.exe"))
}

pub(crate) fn same_file_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
    let (Ok(left), Ok(right)) = (std::fs::canonicalize(left), std::fs::canonicalize(right)) else { return false; };
    let left: Vec<u16> = left.as_os_str().encode_wide().collect();
    let right: Vec<u16> = right.as_os_str().encode_wide().collect();
    unsafe { CompareStringOrdinal(&left, &right, true) == CSTR_EQUAL }
}

pub(crate) fn registered_machine_dll() -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};
    let key = HSTRING::from(format!("{}\\InprocServer32", com_key()));
    let mut data = vec![0u16; 32768]; let mut bytes = (data.len() * 2) as u32;
    if unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, &key, PCWSTR::null(), RRF_RT_REG_SZ, None,
        Some(data.as_mut_ptr().cast()), Some(&mut bytes)) } != ERROR_SUCCESS { return None; }
    if bytes < 4 || bytes % 2 != 0 || bytes as usize > data.len() * 2 { return None; }
    data.truncate(bytes as usize / 2);
    if data.pop() != Some(0) || data.contains(&0) { return None; }
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&data));
    path.is_absolute().then_some(path)
}

/// Explicit utility command only; no editor or child process is launched from the input service.
fn options_command(prepare: bool) -> Report {
    use std::os::windows::process::CommandExt;
    let mut report = Report::default();
    if !crate::system::desktop_settings_allowed() {
        report.push("settings require an interactive, unpackaged, unrestricted, medium-integrity user session", Err(E_ACCESSDENIED));
        return report;
    }
    let exe = std::env::current_exe().ok().zip(registered_machine_dll())
        .and_then(|(setup, dll)| registered_options_executable(&setup, &dll));
    let Some(exe) = exe.filter(|p| p.is_file() && p.with_extension("exe.config").is_file()) else {
        report.push("use the setup tool in the currently registered revision with its complete ui payload", Err(E_FAIL));
        return report;
    };
    let mut command = std::process::Command::new(&exe);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if prepare { command.arg("--prepare-user").creation_flags(0x08000000); } // CREATE_NO_WINDOW
    let mut step = if prepare { "prepare current-user settings" } else { "open settings editor" }.to_string();
    let result = (|| -> Result<String, HRESULT> {
        let mut child = command.spawn().map_err(|_| E_FAIL)?;
        if !prepare { return Ok(format!("opened {}", exe.display())); }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            match child.try_wait() {
                Ok(Some(exit)) if exit.success() => return Ok("existing options preserved; settings prepared for this user".into()),
                Ok(Some(exit)) => { step = format!("settings preparation exited {exit}; open options to repair settings"); return Err(E_FAIL); }
                Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(25)),
                _ => {
                    // Do not terminate a writer mid-publication. No activation is attempted; the owned
                    // headless child may finish normally and a later enable-user can retry.
                    step = format!("preparation not confirmed complete (pid {}); no activation performed; allow it to finish before retrying", child.id());
                    return Err(E_FAIL);
                }
            }
        }
    })();
    report.push(step, result);
    report
}

/// D2 rollback: removes this service's machine registration only, without editing the admin user's list.
pub fn unregister_machine() -> Report {
    let mut report = Report::default();
    if !crate::system::elevated() {
        report.push("machine unregistration requires elevation; no changes made", Err(E_ACCESSDENIED));
        return report;
    }
    init_com();
    if key_exists(HKEY_LOCAL_MACHINE, &ctf_key()) {
        match categories() {
            Ok(manager) => {
                for (name, cat) in CATEGORIES {
                    let result = unsafe { manager.UnregisterCategory(&CLSID_TEXT_SERVICE, &cat, &CLSID_TEXT_SERVICE) };
                    report.push(format!("category {name}"), result.map(|_| String::new()).map_err(|e| e.code()));
                }
            }
            Err(e) => report.push("categories", Err(e.code())),
        }
        let result = profiles().and_then(|m| unsafe { m.UnregisterProfile(&CLSID_TEXT_SERVICE, LANGID_KOREAN, &PROFILE, 0) });
        report.push("TSF profile", result.map(|_| String::new()).map_err(|e| e.code()));
    }
    if key_exists(HKEY_LOCAL_MACHINE, &com_key()) {
        let path = HSTRING::from(com_key());
        report.push("COM server (HKLM)", win32(unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, &path) }).map(|_| String::new()));
    }
    report
}

pub fn unregister() -> Report {
    init_com();
    let mut report = Report::default();
    report.push("4 remove from this user's list", install_layout(ILOT_UNINSTALL));
    if let Some(r) = remove_from_language_list() {
        report.push("5 language list (User Profile)", r);
    }
    // The API only removes what it registered (HKLM); for a standard user it answers E_FAIL even with nothing to remove
    // (Windows 365, 2026-10-12), so it is called only when there is something there.
    if key_exists(HKEY_LOCAL_MACHINE, &ctf_key()) {
        if let Ok(manager) = categories() {
            for (name, cat) in CATEGORIES {
                let r = unsafe { manager.UnregisterCategory(&CLSID_TEXT_SERVICE, &cat, &CLSID_TEXT_SERVICE) };
                report.push(format!("3 category {name}"), r.map(|_| String::new()).map_err(|e| e.code()));
            }
        }
        let r = profiles().and_then(|m| unsafe { m.UnregisterProfile(&CLSID_TEXT_SERVICE, LANGID_KOREAN, &PROFILE, 0) });
        report.push("2 TSF profile", r.map(|_| String::new()).map_err(|e| e.code()));
    }
    if key_exists(HKEY_CURRENT_USER, &ctf_key()) {
        let path = HSTRING::from(ctf_key());
        report.push(
            "2-3 HKCU CTF keys",
            win32(unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &path) }).map(|_| String::new()),
        );
    }
    let path = HSTRING::from(com_key());
    report.push(
        "1 COM server (HKCU)",
        win32(unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &path) }).map(|_| String::new()),
    );
    report
}

/// Read-only: what is registered where, and whether TSF lists the profile.
pub fn status() -> Report {
    init_com();
    let mut report = Report::default();
    let yes = |b: bool| if b { "present" } else { "absent" }.to_string();
    report.push("1 COM server HKCU", Ok(yes(key_exists(HKEY_CURRENT_USER, &com_key()))));
    report.push("1 COM server HKLM", Ok(yes(key_exists(HKEY_LOCAL_MACHINE, &com_key()))));
    report.push("2-3 HKCU CTF keys", Ok(yes(key_exists(HKEY_CURRENT_USER, &ctf_key()))));
    report.push("2-3 HKLM CTF keys", Ok(yes(key_exists(HKEY_LOCAL_MACHINE, &ctf_key()))));
    let listed = profiles().and_then(|m| unsafe {
        let mut p = TF_INPUTPROCESSORPROFILE::default();
        m.GetProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            LANGID_KOREAN,
            &CLSID_TEXT_SERVICE,
            &PROFILE,
            HKL::default(),
            &mut p,
        )?;
        Ok(p.dwFlags)
    });
    // GetProfile answers E_FAIL for a profile that is not registered (Windows 365, 2026-10-12): that is "absent" here.
    report.push(
        "2 TSF profile (GetProfile)",
        match listed {
            Ok(flags) => Ok(format!("flags=0x{flags:x} (1=active, 2=enabled)")),
            Err(e) if e.code() == E_FAIL => Ok("absent (GetProfile 0x80004005)".to_string()),
            Err(e) => Err(e.code()),
        },
    );
    let listed = profiles().and_then(|m| unsafe {
        let profiles = m.EnumProfiles(LANGID_KOREAN)?;
        let mut found = Vec::new();
        loop {
            let mut p = [TF_INPUTPROCESSORPROFILE::default()];
            let mut fetched = 0u32;
            profiles.Next(&mut p, &mut fetched)?;
            if fetched == 0 {
                break;
            }
            found.push((p[0].clsid, p[0].guidProfile, p[0].dwFlags));
        }
        Ok(found)
    });
    report.push(
        "2 TSF profiles for Korean (EnumProfiles)",
        listed.map_err(|e| e.code()).map(|found| {
            let ours = found
                .iter()
                .find(|(clsid, profile, _)| *clsid == CLSID_TEXT_SERVICE && *profile == PROFILE);
            match ours {
                Some((_, _, flags)) => format!("{} listed; ours flags=0x{flags:x}", found.len()),
                None => format!("{} listed; ours not listed", found.len()),
            }
        }),
    );
    let tags = language_tags(false);
    report.push(
        "5 language list (User Profile)",
        Ok(if tags.is_empty() {
            "absent".to_string()
        } else {
            format!("present in {}", tags.join(","))
        }),
    );
    report
}

/// The keyboard input method active in this session, as a token `activate --to` takes back:
/// `type:langid:{clsid}:{profile}:hkl` (type 1 = text service, 2 = keyboard layout).
pub fn active() -> Report {
    init_com();
    let mut report = Report::default();
    let current = profiles().and_then(|m| unsafe {
        let mut p = TF_INPUTPROCESSORPROFILE::default();
        m.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut p)?;
        Ok(format!(
            "{}:{:04X}:{}:{}:{:X}",
            p.dwProfileType,
            p.langid,
            braced(&p.clsid),
            braced(&p.guidProfile),
            p.hkl.0 as usize
        ))
    });
    report.push("active keyboard input method", current.map_err(|e| e.code()));
    report
}

fn parse_token(token: &str) -> std::result::Result<(u32, u16, GUID, GUID, usize), HRESULT> {
    let parts: Vec<&str> = token.split(':').collect();
    let guid = |s: &str| GUID::try_from(s.trim_matches(|c| c == '{' || c == '}')).map_err(|_| E_INVALIDARG);
    match parts.as_slice() {
        [kind, lang, clsid, profile, hkl] => Ok((
            kind.parse().map_err(|_| E_INVALIDARG)?,
            u16::from_str_radix(lang, 16).map_err(|_| E_INVALIDARG)?,
            guid(clsid)?,
            guid(profile)?,
            usize::from_str_radix(hkl, 16).map_err(|_| E_INVALIDARG)?,
        )),
        _ => Err(E_INVALIDARG),
    }
}

/// S0-4 without the screen: switches this session's keyboard input method to ours, or with `to` back to what `active`
/// printed before. The same switch Win+Space makes.
pub fn activate(to: Option<&str>) -> Report {
    init_com();
    let mut report = Report::default();
    let target = match to {
        None => Ok((TF_PROFILETYPE_INPUTPROCESSOR, LANGID_KOREAN, CLSID_TEXT_SERVICE, PROFILE, 0)),
        Some(token) => parse_token(token),
    };
    let switched = target.and_then(|(kind, lang, clsid, profile, hkl)| unsafe {
        profiles()
            .map_err(|e| e.code())
            .and_then(|m| {
                let hr = (m.vtable().ActivateProfile)(
                    m.as_raw(),
                    kind,
                    lang,
                    &clsid,
                    &profile,
                    HKL(hkl as *mut core::ffi::c_void),
                    TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE
                        | if to.is_none() { TF_IPPMF_ENABLEPROFILE } else { 0 },
                );
                activation_result(hr)
            })
            .map(|_| format!("HRESULT=0x00000000 {lang:04X} {} {}", braced(&clsid), braced(&profile)))
    });
    report.push("activate for this session", switched);
    report
}

fn activation_result(hr: HRESULT) -> std::result::Result<(), HRESULT> {
    // S_FALSE (1) is a successful HRESULT, but ActivateProfile defines it as "profile not enabled".
    if hr.0 == 0 {
        Ok(())
    } else {
        Err(hr)
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    #[test]
    fn editor_belongs_to_the_same_architecture_payload_revision() {
        for arch in ["x64", "x86"] {
            let root = std::path::Path::new(r"C:\Program Files\한오토마타\revisions\test");
            assert_eq!(options_executable(&root.join(arch).join("hanautomata-ime-setup.exe")), Some(root.join("ui").join("Hanautomata.ImeOptions.exe")));
        }
        assert!(options_executable(std::path::Path::new(r"C:\unrelated\hanautomata-ime-setup.exe")).is_none());
    }
    #[test]
    fn an_old_setup_cannot_prepare_options_for_a_new_registered_revision() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("hanautomata-revision-test-{}-{nonce}", std::process::id()));
        let old = root.join("old").join("x64"); let new = root.join("new").join("x64");
        std::fs::create_dir_all(&old).unwrap(); std::fs::create_dir_all(&new).unwrap();
        let setup = old.join("hanautomata-ime-setup.exe"); let dll = new.join(DLL_NAME);
        std::fs::write(&setup, b"fixture").unwrap(); std::fs::write(&dll, b"fixture").unwrap();
        let result = registered_options_executable(&setup, &dll);
        let same_dll = old.join(DLL_NAME); std::fs::write(&same_dll, b"fixture").unwrap();
        let same_revision = registered_options_executable(&setup, &same_dll);
        std::fs::remove_file(same_dll).unwrap();
        std::fs::remove_file(setup).unwrap(); std::fs::remove_file(dll).unwrap();
        std::fs::remove_dir(&old).unwrap(); std::fs::remove_dir(old.parent().unwrap()).unwrap();
        std::fs::remove_dir(&new).unwrap(); std::fs::remove_dir(new.parent().unwrap()).unwrap(); std::fs::remove_dir(root).unwrap();
        assert!(result.is_none(), "a stale editor must not prepare the active revision's settings");
        assert!(same_revision.is_some(), "the registered revision must retain access to its own editor");
    }

    #[test]
    fn disabled_profile_is_not_successful_activation() {
        assert_eq!(activation_result(HRESULT(1)), Err(HRESULT(1))); // S_FALSE: profile not enabled
        assert_eq!(activation_result(HRESULT(0)), Ok(()));
        assert_eq!(activation_result(E_FAIL), Err(E_FAIL));
    }

    #[test]
    fn disabled_user_profile_fails_readiness_even_when_query_succeeds() {
        let report = user_readiness_report(Ok(false), &["ko".into()]);
        assert!(!report.ok(), "A successful query of false must not pass selftest-registered");
    }

    #[test]
    fn enabled_profile_missing_from_language_list_fails_readiness() {
        assert!(!user_readiness_report(Ok(true), &[]).ok());
        assert!(user_readiness_report(Ok(true), &["ko".into()]).ok());
        assert!(!user_readiness_report(Err(E_ACCESSDENIED), &["ko".into()]).ok());
    }
}

fn user_readiness_report(enabled: std::result::Result<bool, HRESULT>, tags: &[String]) -> Report {
    let mut report = Report::default();
    match enabled {
        Ok(true) => report.push("IsEnabledLanguageProfile (current user)", Ok("true".into())),
        Ok(false) => report.push("current user is disabled; run enable-user in the intended user's normal session", Err(E_FAIL)),
        Err(hr) => report.push("IsEnabledLanguageProfile (current user)", Err(hr)),
    }
    if tags.is_empty() {
        report.push("current user's language list is missing Hanautomata; run enable-user", Err(E_FAIL));
    } else {
        report.push("language list (current user)", Ok(format!("present in {}", tags.join(","))));
    }
    report
}

fn user_readiness() -> Report {
    let enabled = profiles().and_then(|m| {
        let p: ITfInputProcessorProfiles = m.cast()?;
        unsafe { p.IsEnabledLanguageProfile(&CLSID_TEXT_SERVICE, LANGID_KOREAN, &PROFILE) }
    });
    user_readiness_report(enabled.map(|b| b.as_bool()).map_err(|e| e.code()), &language_tags(false))
}

/// D2 diagnostic: load via registered COM rather than directly loading a DLL file.
pub fn selftest_registered() -> Report {
    init_com();
    let mut report = Report::default();
    let service: windows::core::Result<ITfTextInputProcessorEx> = unsafe {
        CoCreateInstance(&CLSID_TEXT_SERVICE, None, CLSCTX_INPROC_SERVER)
    };
    report.push("CoCreateInstance registered text service", service.map(|_| "loaded".into()).map_err(|e| e.code()));
    report.steps.extend(user_readiness().steps);
    report
}

/// S0 experiment: writes the HKCU CTF keys (and the HKCU COM server), optionally enables the profile for the user, and
/// watches for 10 s whether the CTF keys stay; then removes everything it wrote. On Windows 365 (2026-10-12) the keys
/// written by `register --ctf-hkcu` were gone by the next `status`: this tells whether the system removes them by itself
/// or only after InstallLayoutOrTip.
pub fn probe_hkcu(dll: &str, enable: bool) -> Report {
    init_com();
    let mut report = Report::default();
    let present = |b: bool| if b { "present" } else { "absent" }.to_string();
    report.push("probe COM server (HKCU)", register_com(dll));
    report.push("probe profile (HKCU)", register_profile_hkcu(dll));
    for (name, r) in register_categories_hkcu() {
        report.push(format!("probe {name}"), r);
    }
    if enable {
        report.push("probe InstallLayoutOrTip", install_layout(0));
    }
    for second in 0..=10 {
        let stays = key_exists(HKEY_CURRENT_USER, &ctf_key());
        report.push(format!("probe HKCU CTF keys after {second} s"), Ok(present(stays)));
        if !stays {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    if enable {
        report.push("cleanup InstallLayoutOrTip remove", install_layout(ILOT_UNINSTALL));
    }
    for key in [ctf_key(), com_key()] {
        if key_exists(HKEY_CURRENT_USER, &key) {
            let path = HSTRING::from(key.as_str());
            report.push(
                format!("cleanup HKCU\\{key}"),
                win32(unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &path) }).map(|_| String::new()),
            );
        }
    }
    report
}

/// Loads the DLL as COM would and creates the text service; then checks the engine on one word.
pub fn selftest(dll: &str) -> Report {
    init_com();
    let mut report = Report::default();
    type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut core::ffi::c_void) -> HRESULT;
    let created = (|| -> std::result::Result<String, HRESULT> {
        unsafe {
            let module = LoadLibraryW(&HSTRING::from(dll)).map_err(|e| e.code())?;
            let proc = GetProcAddress(module, PCSTR(c"DllGetClassObject".as_ptr() as *const u8)).ok_or(E_FAIL)?;
            let get: GetClassObject = std::mem::transmute(proc);
            let mut factory: *mut core::ffi::c_void = std::ptr::null_mut();
            get(
                &CLSID_TEXT_SERVICE,
                &windows::Win32::System::Com::IClassFactory::IID,
                &mut factory,
            )
            .ok()?;
            let factory = windows::Win32::System::Com::IClassFactory::from_raw(factory);
            let service: ITfTextInputProcessorEx = factory.CreateInstance(None).map_err(|e| e.code())?;
            let _keys: ITfKeyEventSink = service.cast().map_err(|e| e.code())?;
            let _attributes: ITfDisplayAttributeProvider = service.cast().map_err(|e| e.code())?;
            let configure: ITfFnConfigure = factory.CreateInstance(None).map_err(|e| e.code())?;
            let name = configure.GetDisplayName().map_err(|e| e.code())?;
            if name.to_string() != crate::configuration::DISPLAY_NAME { return Err(E_FAIL); }
            Ok("text service created; key sink, display attributes and Properties interface present (UI not opened)".into())
        }
    })();
    report.push("load DLL and create the text service", created);
    // The names COM and regsvr32 look up. The 32-bit link resolves stdcall-decorated symbols (_DllCanUnloadNow@0) to
    // these names; a missing one here means COM cannot use the DLL.
    let exports = unsafe {
        match LoadLibraryW(&HSTRING::from(dll)) {
            Ok(module) => {
                let missing: Vec<&str> = [
                    c"DllGetClassObject",
                    c"DllCanUnloadNow",
                    c"DllRegisterServer",
                    c"DllUnregisterServer",
                ]
                .iter()
                .filter(|name| GetProcAddress(module, PCSTR(name.as_ptr() as *const u8)).is_none())
                .map(|name| name.to_str().unwrap_or("?"))
                .collect();
                if missing.is_empty() {
                    Ok("DllGetClassObject DllCanUnloadNow DllRegisterServer DllUnregisterServer".to_string())
                } else {
                    eprintln!("missing exports: {}", missing.join(" "));
                    Err(E_FAIL)
                }
            }
            Err(e) => Err(e.code()),
        }
    };
    report.push("COM exports by name", exports);
    let engine = hanautomata_ffi::Engine::new(
        String::new(),
        std::env::temp_dir()
            .join("hanautomata-selftest.json")
            .to_string_lossy()
            .into_owned(),
    );
    let mut shell = crate::shell::Shell::new(engine);
    let mut text = String::new();
    for c in "dkssud ".chars() {
        let vk = if c == ' ' {
            crate::shell::vk::SPACE
        } else {
            c.to_ascii_uppercase() as u16
        };
        let e = crate::shell::KeyEvent::new(vk, crate::shell::Mods::NONE);
        if shell.test_key_down(e) {
            for op in shell.key_down(e).ops {
                if let crate::shell::Op::Commit(s) = op {
                    text.push_str(&s);
                }
            }
        }
    }
    report.push("engine: dkssud + Space", if text == "안녕 " { Ok(text) } else { Err(E_FAIL) });
    report
}

fn default_dll() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(DLL_NAME).to_string_lossy().into_owned()
}

/// `hanautomata-ime-setup <register [--ctf-hkcu] | unregister | status | selftest> [--dll PATH]`
pub fn cli(args: Vec<String>) -> i32 {
    let dll = args
        .iter()
        .position(|a| a == "--dll")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(default_dll);
    let command = args.first().map(String::as_str).unwrap_or("status");
    println!("Hanautomata input method setup {} · {} · {}", env!("CARGO_PKG_VERSION"), tip_string(), dll);
    let report = match command {
        "register" => register(&dll, args.iter().any(|a| a == "--ctf-hkcu")),
        "unregister" => unregister(),
        "register-machine" => register_machine(&dll),
        "unregister-machine" => unregister_machine(),
        "enable-user" => enable_user(true),
        "disable-user" => enable_user(false),
        "options" => options_command(false),
        "status" => status(),
        "selftest" => selftest(&dll),
        "selftest-registered" => selftest_registered(),
        "probe-hkcu" => probe_hkcu(&dll, args.iter().any(|a| a == "--enable")),
        "active" => active(),
        "activate" => activate(args.windows(2).find(|w| w[0] == "--to").map(|w| w[1].as_str())),
        _ => {
            eprintln!(
                "usage: hanautomata-ime-setup <register [--ctf-hkcu] | unregister | register-machine | unregister-machine | enable-user | disable-user | options | status | selftest | selftest-registered | probe-hkcu [--enable] | active | activate [--to TOKEN]> [--dll PATH]"
            );
            eprintln!("https://github.com/peterkimpmp/hanautomata");
            return 2;
        }
    };
    report.print();
    if report.ok() {
        0
    } else {
        1
    }
}
