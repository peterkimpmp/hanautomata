//! Opt-in, unprivileged AppContainer boundary probe. No IME registration, live settings writes or UI.
//! Optional --shared-read grants read/synchronize only on the newly created synthetic file/mutex, never a live resource.
//! Optional --pipe-transport tests fresh, fixed-message pipes with explicit diagnostic ACLs; it does not save learning.
//! The parent creates one uniquely named profile, copies only this probe into it, and removes that owned profile.
//! Reports contain paths/results for a synthetic configuration, never typed text or live configuration contents.

#[cfg(not(windows))]
fn main() { eprintln!("This probe requires Windows."); std::process::exit(2); }

#[cfg(windows)]
#[path = "config_probe/pipe.rs"]
mod pipe_probe;

#[cfg(windows)]
fn main() {
    let code = match probe::run() { Ok(code) => code, Err(e) => { eprintln!("probe error: {e}"); 2 } };
    std::process::exit(code);
}

#[cfg(windows)]
mod probe {
    use crate::pipe_probe;
    use std::{fs, io::{Read, Seek, SeekFrom, Write}, mem::size_of, path::{Component, Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};
    use hanautomata_tsf::{settings::Settings, system};
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{process::{Child, Command, Stdio}, os::windows::process::CommandExt, time::{Duration, Instant}};
    use windows::{core::{HSTRING, PWSTR}, Win32::{
        Foundation::{CloseHandle, LocalFree, GetLastError, SetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE, HLOCAL, WAIT_OBJECT_0},
        Security::{Authorization::{ConvertSidToStringSidW, BuildTrusteeWithSidW, GetSecurityInfo, SetSecurityInfo,
            SetEntriesInAclW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT, SE_KERNEL_OBJECT, SE_OBJECT_TYPE},
            Isolation::{CreateAppContainerProfile, DeleteAppContainerProfile,
            GetAppContainerFolderPath}, FreeSid, GetTokenInformation, PSID, SECURITY_CAPABILITIES, TOKEN_INFORMATION_CLASS,
            TOKEN_QUERY, TokenElevation, TokenIsAppContainer, ACL, PSECURITY_DESCRIPTOR, CreateWellKnownSid,
            DACL_SECURITY_INFORMATION, NO_INHERITANCE, WinBuiltinAnyPackageSid},
        Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE},
        System::{Com::CoTaskMemFree, Threading::{CreateMutexW, CreateProcessW, DeleteProcThreadAttributeList,
            GetCurrentProcess, GetExitCodeProcess, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
            OpenMutexW, OpenProcessToken, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
            CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            STARTF_USESHOWWINDOW, STARTUPINFOEXW, STARTUPINFOW, SYNCHRONIZATION_SYNCHRONIZE, MUTEX_MODIFY_STATE,
            SYNCHRONIZATION_ACCESS_RIGHTS}},
        UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, KF_FLAG_DONT_VERIFY, KF_FLAG_NO_PACKAGE_REDIRECTION,
            KNOWN_FOLDER_FLAG, SHGetKnownFolderPath},
    }};
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    const FIXTURE: &[u8] = br#"{"Format":1,"InputMode":"Korean","ExcludedApps":["probe-only.exe"]}"#;

    struct Handle(HANDLE);
    impl Drop for Handle { fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } } }
    struct Sid(PSID);
    impl Drop for Sid { fn drop(&mut self) { unsafe { FreeSid(self.0); } } }
    struct LocalMemory(HLOCAL);
    impl Drop for LocalMemory { fn drop(&mut self) { unsafe { LocalFree(Some(self.0)); } } }

    // The only callers pass handles returned by create_new / CreateMutexW for this run's unique resources.
    fn grant_synthetic_read(handle: HANDLE, kind: SE_OBJECT_TYPE, rights: u32) -> Result<()> {
        let mut sid_storage = [0usize; 16];
        let mut sid_bytes = std::mem::size_of_val(&sid_storage) as u32;
        let sid = PSID(sid_storage.as_mut_ptr().cast());
        unsafe { CreateWellKnownSid(WinBuiltinAnyPackageSid, None, Some(sid), &mut sid_bytes)?; }
        let mut old_acl = std::ptr::null_mut::<ACL>();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe { GetSecurityInfo(handle, kind, DACL_SECURITY_INFORMATION, None, None, Some(&mut old_acl), None,
            Some(&mut descriptor)).ok()?; }
        let _descriptor = LocalMemory(HLOCAL(descriptor.0));
        if old_acl.is_null() { return Err("refusing to experiment on a null DACL".into()); }
        let mut entry = EXPLICIT_ACCESS_W { grfAccessPermissions: rights, grfAccessMode: GRANT_ACCESS,
            grfInheritance: NO_INHERITANCE, ..Default::default() };
        unsafe { BuildTrusteeWithSidW(&mut entry.Trustee, Some(sid)); }
        let mut new_acl = std::ptr::null_mut::<ACL>();
        unsafe { SetEntriesInAclW(Some(&[entry]), Some(old_acl), &mut new_acl).ok()?; }
        let _acl = LocalMemory(HLOCAL(new_acl.cast()));
        if new_acl.is_null() { return Err("no merged DACL returned".into()); }
        unsafe { SetSecurityInfo(handle, kind, DACL_SECURITY_INFORMATION, None, None, Some(new_acl), None).ok()?; }
        Ok(())
    }
    struct Attributes { ptr: LPPROC_THREAD_ATTRIBUTE_LIST, _storage: Vec<usize> }
    impl Attributes {
        fn new() -> Result<Self> {
            let mut bytes = 0;
            unsafe { let _ = InitializeProcThreadAttributeList(None, 1, None, &mut bytes); }
            if bytes == 0 || bytes > 65536 { return Err("invalid process attribute allocation".into()); }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let ptr = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
            unsafe { InitializeProcThreadAttributeList(Some(ptr), 1, None, &mut bytes)?; }
            Ok(Self { ptr, _storage: storage })
        }
    }
    impl Drop for Attributes { fn drop(&mut self) { unsafe { DeleteProcThreadAttributeList(self.ptr); } } }

    fn task_string(ptr: PWSTR) -> Result<String> {
        let value = unsafe { ptr.to_string() };
        unsafe { CoTaskMemFree(Some(ptr.0.cast())); }
        Ok(value?)
    }
    fn known_folder(flags: KNOWN_FOLDER_FLAG) -> Result<PathBuf> {
        Ok(PathBuf::from(task_string(unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, flags, None)? })?))
    }
    fn sid_text(sid: PSID) -> Result<String> {
        let mut text = PWSTR::null();
        unsafe { ConvertSidToStringSidW(sid, &mut text)?; }
        let value = unsafe { text.to_string() };
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))); }
        Ok(value?)
    }
    fn token_flag(class: TOKEN_INFORMATION_CLASS) -> Result<bool> {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?; }
        let token = Handle(token);
        let (mut value, mut length) = (0u32, 0u32);
        unsafe { GetTokenInformation(token.0, class, Some((&mut value as *mut u32).cast()), 4, &mut length)?; }
        if length != 4 { return Err("unexpected token result length".into()); }
        Ok(value != 0)
    }
    fn is_app_container() -> Result<bool> { token_flag(TokenIsAppContainer) }
    fn valid_fixture_name(name: &str) -> bool {
        name.starts_with("quality-probe-") && name.ends_with(".json") && name.len() < 100
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            && !name.contains("..")
    }
    fn owned_profile_path(path: &Path, packages: &Path, name: &str) -> bool {
        let Ok(relative) = path.strip_prefix(packages) else { return false; };
        matches!(relative.components().next(), Some(Component::Normal(part)) if part.to_string_lossy().eq_ignore_ascii_case(name))
    }
    fn verify_profile_folder(folder: &Path, base: &Path, name: &str) -> Result<PathBuf> {
        let canonical = folder.canonicalize()?;
        let packages = base.join("Packages").canonicalize()?;
        if !owned_profile_path(&canonical, &packages, name) {
            return Err("unexpected profile folder; kept for inspection, no deletion".into());
        }
        Ok(canonical)
    }
    fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
        let file = fs::File::open(path)?;
        if !file.metadata()?.is_file() { return Err(std::io::Error::other("expected a regular probe file")); }
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > limit { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "probe file exceeds size limit")); }
        Ok(bytes)
    }
    fn read_fixture(base: &Path, name: &str) -> Value {
        let path = base.join(name);
        let raw = read_bounded(&path, Settings::MAX_BYTES);
        let loaded = Settings::load(&path);
        // Open only: no create/truncate/write. A successful shared-read trial must still deny this in the child.
        let write_open = fs::OpenOptions::new().write(true).open(&path);
        json!({"path": path, "open_error": raw.as_ref().err().map(|e| e.raw_os_error()),
            "sha256": raw.as_ref().ok().map(|bytes| format!("{:x}", Sha256::digest(bytes))),
            "write_open_succeeded": write_open.is_ok(), "write_open_error": write_open.err().and_then(|e| e.raw_os_error()),
            "bytes_match": raw.as_ref().map(|b| b.as_slice() == FIXTURE).unwrap_or(false),
            "settings_match": loaded.as_ref().map(|s| *s == Settings::parse(FIXTURE).expect("constant fixture is valid")).unwrap_or(false),
            "loader_error": loaded.err()})
    }
    fn mutex_probe(name: &str, rights: SYNCHRONIZATION_ACCESS_RIGHTS) -> Value {
        match unsafe { OpenMutexW(rights, false, &HSTRING::from(name)) } {
            Ok(handle) => { let _held = Handle(handle); json!({"opened": true}) },
            Err(e) => json!({"opened": false, "hresult": format!("0x{:08x}", e.code().0 as u32)}),
        }
    }
    fn folder_probe(flags: KNOWN_FOLDER_FLAG, fixture: &str) -> Value {
        match known_folder(flags) {
            Ok(base) => json!({"folder": base, "file": read_fixture(&base.join("Hanautomata"), fixture)}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
    fn snapshot(fixture: &str, mutex: &str) -> Result<Value> {
        if !valid_fixture_name(fixture) || !mutex.starts_with("Local\\Hanautomata.QualityProbe.") {
            return Err("unexpected probe input".into());
        }
        Ok(json!({"pid": std::process::id(), "app_container": is_app_container()?,
            "elevated": token_flag(TokenElevation)?,
            "env_local_app_data": std::env::var_os("LOCALAPPDATA").map(|v| v.to_string_lossy().into_owned()),
            "legacy": read_fixture(&system::data_dir(), fixture),
            "product_settings": match system::settings_dir() {
                Ok(base) => read_fixture(&base, fixture), Err(error) => json!({"path_error": error}),
            },
            "known_default": folder_probe(KF_FLAG_DEFAULT | KF_FLAG_DONT_VERIFY, fixture),
            "known_unredirected": folder_probe(KF_FLAG_NO_PACKAGE_REDIRECTION | KF_FLAG_DONT_VERIFY, fixture),
            "controlled_mutex": mutex_probe(mutex, SYNCHRONIZATION_SYNCHRONIZE),
            "controlled_mutex_modify": mutex_probe(mutex, MUTEX_MODIFY_STATE),
            "controlled_marker_blocks_input": system::tray_marker_blocks_input(mutex),
            "missing_controlled_mutex": mutex_probe(&format!("{mutex}.absent"), SYNCHRONIZATION_SYNCHRONIZE),
            "missing_marker_blocks_input": system::tray_marker_blocks_input(&format!("{mutex}.absent")),
            "production_tray_blocks_input": system::tray_running()}))
    }
    fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(bytes)?; file.sync_all()?; Ok(())
    }
    fn update_report(file: &mut fs::File, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&bytes)?;
        file.set_len(bytes.len() as u64)?;
        file.sync_all()?;
        Ok(())
    }
    fn evaluation(shared_read: bool, successful: bool, legacy: bool, boundary: bool) -> (&'static str, i32) {
        if !successful { ("probe_failed", 2) }
        else if shared_read { if boundary { ("shared_read_pass", 0) } else { ("shared_read_failed", 1) } }
        else if legacy { ("compatible", 0) } else { ("incompatible", 1) }
    }
    fn wait_managed_ready(process: &mut Child, ready: &Path) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(code) = process.try_wait()? { return Err(format!("managed writer exited before ready: {code}").into()); }
            match read_bounded(ready, 128) {
                Ok(bytes) if bytes == b"ready" => return Ok(()),
                Ok(_) => {}, // It may still be flushing the newly created ready file.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                Err(e) => return Err(e.into()),
            }
            if Instant::now() >= deadline { return Err("managed writer readiness deadline".into()); }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn stop_managed(process: &mut Child, terminal: &mut bool) -> Result<()> {
        drop(process.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(code) = process.try_wait()? {
                *terminal = true;
                if code.success() { return Ok(()); }
                return Err(format!("managed writer/cleanup failed: {code}").into());
            }
            if Instant::now() >= deadline {
                process.kill()?; process.wait()?; *terminal = true;
                return Err("managed writer had to be terminated; resource cleanup unverified".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn child(exe: &Path, output: &Path, fixture: &str, mutex: &str, sid: PSID, terminal: &mut bool,
        child_pid: &mut Option<u32>, pipe_token: Option<(&str, pipe_probe::Namespace)>) -> Result<u32> {
        let attributes = Attributes::new()?;
        let security = SECURITY_CAPABILITIES { AppContainerSid: sid, ..Default::default() };
        unsafe { UpdateProcThreadAttribute(attributes.ptr, 0, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            Some((&security as *const SECURITY_CAPABILITIES).cast()), size_of::<SECURITY_CAPABILITIES>(), None, None)?; }
        // All arguments are generated paths/names; Windows filenames cannot contain a quote. Never invoke a shell.
        let command = match pipe_token {
            Some((token, namespace)) => format!("\"{}\" --child-pipe \"{}\" \"{fixture}\" \"{mutex}\" \"{token}\" {}", exe.display(), output.display(), namespace.argument()),
            None => format!("\"{}\" --child \"{}\" \"{fixture}\" \"{mutex}\"", exe.display(), output.display()),
        };
        let mut command: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
        let mut start = STARTUPINFOEXW::default();
        start.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        start.StartupInfo.dwFlags = STARTF_USESHOWWINDOW; start.StartupInfo.wShowWindow = 0;
        start.lpAttributeList = attributes.ptr;
        let mut info = PROCESS_INFORMATION::default();
        unsafe { CreateProcessW(&HSTRING::from(exe.to_string_lossy().as_ref()), Some(PWSTR(command.as_mut_ptr())),
            None, None, false, EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW, None,
            &HSTRING::from(exe.parent().ok_or("missing child folder")?.to_string_lossy().as_ref()),
            (&start as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(), &mut info)?; }
        *terminal = false;
        *child_pid = Some(info.dwProcessId);
        let process = Handle(info.hProcess); let _thread = Handle(info.hThread);
        if unsafe { WaitForSingleObject(process.0, 20000) } != WAIT_OBJECT_0 {
            unsafe { TerminateProcess(process.0, 124)?; }
            *terminal = unsafe { WaitForSingleObject(process.0, 5000) } == WAIT_OBJECT_0;
            return Err("owned probe child exceeded its deadline; termination requested".into());
        }
        *terminal = true;
        let mut code = 0; unsafe { GetExitCodeProcess(process.0, &mut code)?; }
        Ok(code)
    }
    fn parent(output: &Path, shared_read: bool, managed_writer: Option<&Path>, pipe_transport: Option<pipe_probe::Namespace>) -> Result<i32> {
        if output.exists() { return Err("refusing to overwrite an existing probe report".into()); }
        if is_app_container()? || token_flag(TokenElevation)? { return Err("launch the parent from a non-elevated ordinary user process".into()); }
        let token = format!("{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos());
        let name = format!("hanautomata.quality.{token}");
        let fixture_name = format!("quality-probe-{token}.json");
        let mutex_name = format!("Local\\Hanautomata.QualityProbe.{token}");
        let base = known_folder(KF_FLAG_NO_PACKAGE_REDIRECTION | KF_FLAG_DONT_VERIFY)?;
        let fixture = base.join("Hanautomata").join(&fixture_name);
        let mut report = json!({"phase": "pending", "parent_pid": std::process::id(), "profile_name": name, "fixture": fixture,
            "shared_read_experiment": shared_read,
            "pipe_transport_experiment": pipe_transport.map(|namespace| namespace.argument()),
            "managed_writer": managed_writer,
            "container_kind": "non-packaged AppContainer; zero capabilities; not LPAC; inherited environment",
            "scope": "owned synthetic profile/fixture/mutex and optional fixed-message pipes; optional ACLs affect only this run's resources; no live settings or ACL changes; real packaged hosts need separate verification"});
        let mut report_file = fs::OpenOptions::new().write(true).create_new(true).open(output)?;
        update_report(&mut report_file, &report)?;
        println!("pending_report={} profile={} fixture={}", output.display(), name, fixture.display());
        let mut profile_sid = None;
        let mut profile_dir = None;
        let mut fixture_created = false;
        let mut directory_created = false;
        let mut child_terminal = true;
        let mut child_pid = None;
        let mut managed_child = None;
        let mut managed_terminal = true;
        let mut pipes = None;
        let mut outcome = (|| -> Result<()> {
            let directory = fixture.parent().ok_or("missing fixture folder")?;
            match fs::create_dir(directory) {
                Ok(()) => directory_created = true,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
                Err(e) => return Err(e.into()),
            }
            if directory.canonicalize()? != system::data_dir().canonicalize()? {
                return Err("baseline fixture path differs from the product path; no compatibility verdict".into());
            }
            let mut fixture_options = fs::OpenOptions::new();
            fixture_options.write(true).create_new(true);
            if shared_read { fixture_options.access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | 0x0004_0000); } // WRITE_DAC, owned new file only
            let mut fixture_file = fixture_options.open(&fixture)?;
            fixture_created = true;
            fixture_file.write_all(FIXTURE)?; fixture_file.sync_all()?;
            let mutex = if managed_writer.is_none() {
                unsafe { SetLastError(ERROR_SUCCESS); }
                let raw_mutex = unsafe { CreateMutexW(None, false, &HSTRING::from(&mutex_name))? };
                let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
                let mutex = Handle(raw_mutex);
                if already_exists { return Err("refusing to modify or reuse an existing probe mutex".into()); }
                report["parent"] = snapshot(&fixture_name, &mutex_name)?;
                if report["parent"]["legacy"]["settings_match"] != true || report["parent"]["controlled_mutex"]["opened"] != true {
                    return Err("ordinary-process baseline failed; no compatibility verdict".into());
                }
                Some(mutex)
            } else { None };
            if shared_read {
                grant_synthetic_read(HANDLE(fixture_file.as_raw_handle()), SE_FILE_OBJECT, FILE_GENERIC_READ.0)?;
                grant_synthetic_read(mutex.as_ref().ok_or("missing synthetic mutex")?.0, SE_KERNEL_OBJECT, SYNCHRONIZATION_SYNCHRONIZE.0)?;
                report["synthetic_read_aces_applied"] = json!(true);
            }
            drop(fixture_file);
            let sid = Sid(unsafe { CreateAppContainerProfile(&HSTRING::from(&name), &HSTRING::from("Hanautomata quality probe"),
                &HSTRING::from("Temporary local configuration boundary test; no capabilities"), None)? });
            profile_sid = Some(sid);
            let sid_string = sid_text(profile_sid.as_ref().ok_or("profile SID missing")?.0)?;
            report["profile_sid"] = json!(sid_string);
            let folder = PathBuf::from(task_string(unsafe { GetAppContainerFolderPath(&HSTRING::from(&sid_string))? })?);
            verify_profile_folder(&folder, &base, &name)?;
            profile_dir = Some(folder.clone()); report["profile_folder"] = json!(folder);
            update_report(&mut report_file, &report)?;
            if let Some(writer) = managed_writer {
                let ready = folder.join("managed-ready.txt");
                let error_log = folder.join("managed-writer.log");
                let stderr = fs::OpenOptions::new().write(true).create_new(true).open(&error_log)?;
                managed_child = Some(Command::new(writer.canonicalize()?).arg("--shared-resources").arg(&fixture)
                    .arg(&mutex_name).arg(&ready).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::from(stderr))
                    .creation_flags(CREATE_NO_WINDOW.0).spawn()?);
                managed_terminal = false;
                wait_managed_ready(managed_child.as_mut().unwrap(), &ready)?;
                report["managed_stages"] = json!(["save", "replace", "restore"]);
                report["parent"] = snapshot(&fixture_name, &mutex_name)?;
                if report["parent"]["product_settings"]["settings_match"] != true || report["parent"]["controlled_mutex"]["opened"] != true {
                    return Err("managed writer baseline failed".into());
                }
            }
            let exe = folder.join("hanautomata-config-probe.exe");
            if exe.exists() { return Err("unexpected existing executable in newly created profile".into()); }
            let mut source = fs::File::open(std::env::current_exe()?)?;
            let mut copied = fs::OpenOptions::new().write(true).create_new(true).open(&exe)?;
            std::io::copy(&mut source, &mut copied)?; copied.sync_all()?; drop(copied); drop(source);
            if let Some(namespace) = pipe_transport {
                pipes = Some(pipe_probe::Experiment::start(&token, &sid_string, namespace)?);
                report["pipe_transport"] = json!({"desktop": pipe_probe::clients(&token, false, namespace)?,
                    "scope": "four unique local diagnostic pipes; fixed nonce messages only; current user DACL; allowed variant adds narrow data access for this exact profile SID; both variants use a low-integrity label; not a broker authorization design; direct namespace is a diagnostic comparison, not a supported packaged-app claim",
                    "namespace": namespace.argument(), "authorized_profile_sid": sid_string, "expected_parent_pid": std::process::id(),
                    "token": token, "client_access": "0x0012008b", "remote_clients": "rejected"});
                update_report(&mut report_file, &report)?;
            }
            let child_output = folder.join("probe-result.json");
            let exit = child(&exe, &child_output, &fixture_name, &mutex_name,
                profile_sid.as_ref().ok_or("profile SID missing")?.0, &mut child_terminal, &mut child_pid,
                pipe_transport.map(|namespace| (token.as_str(), namespace)))?;
            report["child_exit"] = json!(exit);
            if !child_output.is_file() { return Err(format!("child exited {exit:#x} without its report").into()); }
            report["child"] = serde_json::from_slice(&read_bounded(&child_output, 65536)?)?;
            drop(mutex);
            if exit != 0 || report["child"]["app_container"] != true || report["child"]["pid"] != json!(child_pid) {
                return Err("child PID and AppContainer result were not confirmed".into());
            }
            Ok(())
        })();
        if let Some(pipes) = pipes.as_mut() {
            report["pipe_transport"]["servers"] = pipes.finish();
            report["pipe_transport"]["container"] = report["child"]["pipe_transport"].clone();
        }
        let mut pipe_cleanup_ok = true;
        if let Some(namespace) = pipe_transport {
            report["pipe_transport"]["expected_child_pid"] = json!(child_pid);
            report["pipe_transport"]["post_cleanup"] = pipe_probe::absence(&token, namespace)
                .unwrap_or_else(|e| json!({"all_absent": false, "error": e.to_string()}));
            pipe_cleanup_ok = report["pipe_transport"]["post_cleanup"]["all_absent"] == true && child_terminal;
            report["pipe_transport"]["handles_closed"] = json!(pipe_cleanup_ok);
        }
        let mut managed_ok = true;
        if let Some(process) = managed_child.as_mut() {
            let result = stop_managed(process, &mut managed_terminal);
            managed_ok = result.is_ok();
            report["managed_cleanup_error"] = json!(result.as_ref().err().map(|e| e.to_string()));
            if outcome.is_ok() { outcome = result; }
            if let Some(folder) = profile_dir.as_ref() {
                report["managed_log"] = json!(read_bounded(&folder.join("managed-writer.log"), 8192)
                    .ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned()));
            }
        }
        report["probe_error"] = json!(outcome.as_ref().err().map(|e| e.to_string()));
        report["child_terminal"] = json!(child_terminal);
        report["managed_terminal"] = json!(managed_terminal);
        let mut cleanup_ok = true;
        if profile_sid.is_some() {
            cleanup_ok = false;
            let root = base.join("Packages").join(&name);
            // DeleteAppContainerProfile owns the entire profile root, not just the returned AC subdirectory.
            for attempt in 1..=3 {
                let scope = profile_dir.as_ref().ok_or("profile folder not established".into())
                    .and_then(|_| verify_profile_folder(&root, &base, &name));
                report["cleanup_scope_error"] = json!(scope.as_ref().err().map(|e| e.to_string()));
                if scope.is_err() || !child_terminal || !managed_terminal { break; }
                let result = unsafe { DeleteAppContainerProfile(&HSTRING::from(&name)) };
                report["cleanup_attempts"] = json!(attempt);
                report["cleanup_profile_error"] = json!(result.as_ref().err().map(|e| e.to_string()));
                let absent = matches!(fs::symlink_metadata(&root), Err(e) if e.kind() == std::io::ErrorKind::NotFound);
                report["profile_root_absent"] = json!(absent);
                if result.is_ok() && absent { cleanup_ok = true; break; }
                if attempt < 3 { std::thread::sleep(std::time::Duration::from_millis(500)); }
            }
            // Record the supported API's post-delete observation separately; the filesystem postcondition is the whole root.
            if let Some(sid) = report["profile_sid"].as_str() {
                report["folder_lookup_after_cleanup"] = json!(match unsafe { GetAppContainerFolderPath(&HSTRING::from(sid)) } {
                    Ok(path) => task_string(path).map(|p| json!({"path": p})).unwrap_or_else(|e| json!({"error": e.to_string()})),
                    Err(e) => json!({"error": e.to_string(), "hresult": format!("0x{:08x}", e.code().0 as u32)}),
                });
            }
            report["cleanup_profile"] = json!(if cleanup_ok { "removed" } else { "not confirmed; inspect manual_cleanup" });
            if !cleanup_ok { report["manual_cleanup"] = json!({"profile_name": name, "profile_root": root,
                "instruction": "Confirm the owned probe child has exited and the root still resolves to this exact profile; then use DeleteAppContainerProfile for this name only. No broad directory deletion."}); }
        }
        cleanup_ok &= managed_ok && pipe_cleanup_ok;
        if fixture_created {
            let result = fs::remove_file(&fixture); // exact create_new file from this run; never recursive cleanup
            report["cleanup_fixture"] = json!(match result { Ok(()) => "removed".to_string(), Err(e) => { cleanup_ok = false; e.to_string() } });
        }
        report["fixture_directory_created"] = json!(directory_created);
        if directory_created {
            let result = fs::remove_dir(fixture.parent().unwrap()); // empty owned leaf only, never recursive
            report["cleanup_fixture_directory"] = json!(match result { Ok(()) => "removed".to_string(), Err(e) => { cleanup_ok = false; e.to_string() } });
        }
        let compatible = outcome.is_ok() && report["parent"]["legacy"]["settings_match"] == true
            && report["child"]["legacy"]["settings_match"] == true && report["child"]["controlled_mutex"]["opened"] == true;
        report["legacy_implementation_compatible"] = if shared_read || managed_writer.is_some() || pipe_transport.is_some() { Value::Null } else { json!(compatible) };
        let boundary = outcome.is_ok()
            && report["child"]["product_settings"]["settings_match"] == true
            && report["child"]["product_settings"]["sha256"] == report["parent"]["product_settings"]["sha256"]
            && report["child"]["product_settings"]["write_open_error"] == 5
            && report["child"]["controlled_mutex"]["opened"] == true
            && report["child"]["controlled_mutex_modify"]["hresult"] == "0x80070005"
            && report["child"]["controlled_marker_blocks_input"] == true
            && report["child"]["missing_controlled_mutex"]["hresult"] == "0x80070002"
            && report["child"]["missing_marker_blocks_input"] == false;
        if shared_read || managed_writer.is_some() {
            report["shared_read_boundary_pass"] = json!(boundary);
        }
        report["cleanup_ok"] = json!(cleanup_ok);
        let (mut verdict, mut code) = evaluation(shared_read || managed_writer.is_some(), outcome.is_ok() && cleanup_ok, compatible, boundary);
        if pipe_transport.is_some() {
            let pass = pipe_probe::passed(&report["pipe_transport"], report["child"]["app_container"] == true);
            report["pipe_transport_boundary_pass"] = json!(pass);
            (verdict, code) = if outcome.is_err() || !cleanup_ok { ("probe_failed", 2) }
                else if pass { ("pipe_transport_pass", 0) } else { ("pipe_transport_failed", 1) };
        }
        if managed_writer.is_some() {
            verdict = match code { 0 => "product_resources_pass", 1 => "product_resources_failed", _ => "probe_failed" };
        }
        report["phase"] = json!("finished");
        report["verdict"] = json!(verdict);
        report["exit_code"] = json!(code);
        update_report(&mut report_file, &report)?;
        println!("report={} verdict={} cleanup_ok={}", output.display(), verdict, cleanup_ok);
        Ok(code)
    }
    pub fn run() -> Result<i32> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.as_slice() {
            [flag, output] if flag == "--output" => parent(Path::new(output), false, None, None),
            [flag, output, mode] if flag == "--output" && mode == "--shared-read" => parent(Path::new(output), true, None, None),
            [flag, output, mode] if flag == "--output" && mode == "--pipe-transport" => parent(Path::new(output), false, None, Some(pipe_probe::Namespace::Local)),
            [flag, output, mode] if flag == "--output" && mode == "--pipe-transport-direct" => parent(Path::new(output), false, None, Some(pipe_probe::Namespace::Direct)),
            [flag, output, mode, writer] if flag == "--output" && mode == "--managed-writer" => parent(Path::new(output), false, Some(Path::new(writer)), None),
            [flag, output, fixture, mutex, token, namespace] if flag == "--child-pipe" => {
                if !is_app_container()? { return Err("worker mode requires a real AppContainer token".into()); }
                let mut report = snapshot(fixture, mutex)?;
                report["pipe_transport"] = pipe_probe::clients(token, true, pipe_probe::Namespace::parse(namespace)?)?;
                write_new(Path::new(output), &serde_json::to_vec_pretty(&report)?)?;
                Ok(0)
            },
            [flag, output, fixture, mutex] if flag == "--child" => {
                if !is_app_container()? { return Err("worker mode requires a real AppContainer token".into()); }
                write_new(Path::new(output), &serde_json::to_vec_pretty(&snapshot(fixture, mutex)?)?)?;
                Ok(0)
            },
            _ => Err("usage: hanautomata-config-probe --output <new-report.json> [--shared-read | --managed-writer <test-helper.exe> | --pipe-transport | --pipe-transport-direct]".into()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn experimental_access_never_becomes_a_production_compatibility_verdict() {
            assert_eq!(evaluation(true, true, true, false), ("shared_read_failed", 1));
            assert_eq!(evaluation(true, true, false, true), ("shared_read_pass", 0));
            assert_eq!(evaluation(false, true, false, true), ("incompatible", 1));
            assert_eq!(evaluation(false, true, true, false), ("compatible", 0));
            assert_eq!(evaluation(true, false, true, true), ("probe_failed", 2));
        }
        #[test]
        fn oversized_probe_report_is_rejected_without_truncating_valid_reads() {
            let path = std::env::temp_dir().join(format!("hanautomata-probe-test-{}-{}.json", std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
            write_new(&path, b"12345").unwrap();
            let exact = read_bounded(&path, 5);
            let too_large = read_bounded(&path, 4);
            fs::remove_file(&path).unwrap();
            assert_eq!(exact.unwrap(), b"12345");
            assert_eq!(too_large.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        }
        #[test]
        fn probe_names_cannot_select_live_settings_or_escape_the_fixture_folder() {
            assert!(valid_fixture_name("quality-probe-123-456.json"));
            for name in ["tsf-settings.json", "quality-probe-../settings.json", "quality-probe-a/b.json", "quality-probe-a:b.json"] {
                assert!(!valid_fixture_name(name));
            }
        }
        #[test]
        fn cleanup_requires_the_exact_new_profile_under_packages() {
            let packages = Path::new(r"C:\Users\test\AppData\Local\Packages");
            assert!(owned_profile_path(&packages.join("hanautomata.quality.123").join("AC"), packages, "hanautomata.quality.123"));
            assert!(!owned_profile_path(packages, packages, "hanautomata.quality.123"));
            assert!(!owned_profile_path(&packages.join("hanautomata.quality.1234"), packages, "hanautomata.quality.123"));
            assert!(!owned_profile_path(Path::new(r"C:\Windows"), packages, "hanautomata.quality.123"));
        }
    }
}
