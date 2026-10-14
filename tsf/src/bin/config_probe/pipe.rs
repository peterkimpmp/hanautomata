//! Synthetic IPC experiment, not a learning broker or a product endpoint.
use serde_json::{json, Value};
use std::{mem::size_of, os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{Arc, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
use windows::{core::{HSTRING, PWSTR}, Win32::{
    Foundation::{HANDLE, HLOCAL, LocalFree, GetLastError, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING},
    Security::{Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
    Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_MODE,
        OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT},
    System::{Threading::{GetCurrentProcess, OpenProcessToken}, Pipes::{ConnectNamedPipe, CreateNamedPipeW,
        GetNamedPipeServerProcessId, GetNamedPipeClientProcessId, PeekNamedPipe, WaitNamedPipeW,
        PIPE_TYPE_MESSAGE, PIPE_READMODE_MESSAGE, PIPE_NOWAIT, PIPE_REJECT_REMOTE_CLIENTS}},
}};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
// No FILE_APPEND_DATA (also FILE_CREATE_PIPE_INSTANCE), WRITE_DAC or WRITE_OWNER.
const CLIENT_ACCESS: u32 = 0x0012_008b;
const LABELS: [&str; 4] = ["desktop-denied", "desktop-allowed", "container-denied", "container-allowed"];
const POLL: Duration = Duration::from_millis(5);
// Parent desktop exchanges (<=10s) plus the child process deadline (20s), with a margin.
const DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
pub(crate) enum Namespace { Local, Direct }
impl Namespace {
    pub(crate) fn argument(self) -> &'static str { match self { Self::Local => "local", Self::Direct => "direct" } }
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value { "local" => Ok(Self::Local), "direct" => Ok(Self::Direct), _ => Err("invalid pipe namespace experiment".into()) }
    }
}

fn valid_token(token: &str) -> bool {
    let Some((pid, time)) = token.split_once('-') else { return false; };
    token.len() <= 64 && !time.is_empty() && time.bytes().all(|c| c.is_ascii_digit())
        && !pid.is_empty() && pid.bytes().all(|c| c.is_ascii_digit())
        && pid.parse::<u32>().is_ok_and(|pid| pid != 0)
}

pub(crate) fn passed(report: &Value, child_is_container: bool) -> bool {
    let pid = |value: &Value| value.as_u64().filter(|p| *p != 0 && *p <= u32::MAX as u64);
    let Some(parent) = pid(&report["expected_parent_pid"]) else { return false; };
    let Some(child) = pid(&report["expected_child_pid"]) else { return false; };
    child_is_container && report["handles_closed"] == true
        && pid(&report["servers"]["desktop-denied"]["client_pid"]) == Some(parent)
        && pid(&report["servers"]["desktop-allowed"]["client_pid"]) == Some(parent)
        && pid(&report["servers"]["container-allowed"]["client_pid"]) == Some(child)
        && report["desktop"]["denied"]["round_trip"] == true
        && report["desktop"]["allowed"]["round_trip"] == true
        && report["container"]["denied"]["open_error"] == 5
        && report["container"]["allowed"]["round_trip"] == true
        && report["servers"]["desktop-denied"]["round_trip"] == true
        && report["servers"]["desktop-allowed"]["round_trip"] == true
        && report["servers"]["container-allowed"]["round_trip"] == true
        && report["servers"]["container-denied"]["connected"] == false
        && report["servers"]["container-denied"]["cancelled"] == true
}

fn name(token: &str, label: &str, namespace: Namespace) -> Result<String> {
    if !valid_token(token) || !LABELS.contains(&label) { return Err("invalid synthetic pipe endpoint".into()); }
    let prefix = match namespace { Namespace::Local => r"\\.\pipe\LOCAL\", Namespace::Direct => r"\\.\pipe\" };
    Ok(format!("{prefix}Hanautomata.QualityPipe.{token}.{label}"))
}

fn valid_profile_sid(sid: &str) -> bool {
    let Some(tail) = sid.strip_prefix("S-1-15-2-") else { return false; };
    let parts: Vec<_> = tail.split('-').collect();
    parts.len() == 7 && parts.iter().all(|part| !part.is_empty()
        && part.bytes().all(|b| b.is_ascii_digit()) && part.parse::<u32>().is_ok())
}

struct LocalMemory(HLOCAL);
impl Drop for LocalMemory { fn drop(&mut self) { unsafe { LocalFree(Some(self.0)); } } }
fn user_sid() -> Result<String> {
    let mut raw = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw)?; }
    let token = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut bytes = 0;
    unsafe { let _ = GetTokenInformation(HANDLE(token.as_raw_handle()), TokenUser, None, 0, &mut bytes); }
    if bytes < size_of::<TOKEN_USER>() as u32 || bytes > 65536 { return Err("invalid token user allocation".into()); }
    let capacity = bytes;
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    unsafe { GetTokenInformation(HANDLE(token.as_raw_handle()), TokenUser, Some(buffer.as_mut_ptr().cast()), capacity, &mut bytes)?; }
    if bytes < size_of::<TOKEN_USER>() as u32 || bytes > capacity { return Err("invalid token user length".into()); }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text)?; }
    let _text = LocalMemory(HLOCAL(text.0.cast()));
    Ok(unsafe { text.to_string()? })
}

fn create(token: &str, label: &str, user: &str, profile_sid: &str, namespace: Namespace) -> Result<OwnedHandle> {
    let endpoint = HSTRING::from(name(token, label, namespace)?);
    if !valid_profile_sid(profile_sid) { return Err("only one specific AppContainer profile SID is permitted".into()); }
    let package = if label.ends_with("-allowed") { format!("(A;;0x12008b;;;{profile_sid})") } else { String::new() };
    // Only fresh synthetic objects receive these ACLs and the low-integrity label.
    // This does not establish production authorization for arbitrary AppContainer clients.
    let sddl = HSTRING::from(format!("D:P(A;;GA;;;{user}){package}S:(ML;;NW;;;LW)"));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut descriptor, None)?; }
    let _descriptor = LocalMemory(HLOCAL(descriptor.0));
    let attributes = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0, bInheritHandle: false.into() };
    let raw = unsafe { CreateNamedPipeW(&endpoint, PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS, 1, 4096, 4096, 0, Some(&attributes)) };
    if raw.is_invalid() { return Err(windows::core::Error::from_win32().into()); }
    let owned = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    // Establish listening before returning. NOWAIT is used only for this bounded diagnostic;
    // a product broker needs cancellable overlapped I/O, not this polling implementation.
    match unsafe { ConnectNamedPipe(raw, None) } {
        Ok(()) => {},
        Err(e) if e.code() == ERROR_PIPE_LISTENING.to_hresult() => {},
        Err(e) => return Err(e.into()),
    }
    Ok(owned)
}

fn packet(token: &str, label: &str) -> Vec<u8> { format!("HANAUTOMATA-PROBE/1 {token} {label}").into_bytes() }
fn error_value(stage: &str, error: windows::core::Error, connected: bool) -> Value {
    json!({"stage": stage, "hresult": format!("0x{:08x}", error.code().0 as u32), "connected": connected, "round_trip": false})
}
fn write(handle: HANDLE, bytes: &[u8]) -> windows::core::Result<()> {
    let mut written = 0;
    unsafe { WriteFile(handle, Some(bytes), Some(&mut written), None)?; }
    if written as usize != bytes.len() { return Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x8007001d_u32 as i32))); }
    Ok(())
}

fn serve(owned: OwnedHandle, expected: Vec<u8>, stop: Arc<AtomicBool>) -> Value {
    let handle = HANDLE(owned.as_raw_handle());
    let deadline = Instant::now() + DEADLINE;
    let mut connected = false;
    let mut client_pid = 0;
    let mut acknowledged = false;
    loop {
        if !connected {
            match unsafe { ConnectNamedPipe(handle, None) } {
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => {
                    connected = true;
                    if let Err(error) = unsafe { GetNamedPipeClientProcessId(handle, &mut client_pid) } {
                        return error_value("client PID", error, true);
                    }
                },
                Ok(()) => {},
                Err(e) if e.code() == ERROR_PIPE_LISTENING.to_hresult() => {},
                Err(e) => return error_value("connect", e, false),
            }
        }
        if connected {
            let mut buffer = [0u8; 256];
            let mut count = 0;
            match unsafe { ReadFile(handle, Some(&mut buffer), Some(&mut count), None) } {
                Ok(()) if count != 0 => {
                    let received = &buffer[..count as usize];
                    if acknowledged {
                        return json!({"connected": true, "client_pid": client_pid, "round_trip": received == b"done", "client_finished": received == b"done"});
                    }
                    if received != expected { return json!({"connected": true, "round_trip": false, "stage": "request mismatch"}); }
                    let mut ack = b"ack:".to_vec(); ack.extend_from_slice(&expected);
                    if let Err(e) = write(handle, &ack) { return error_value("ack", e, true); }
                    acknowledged = true;
                    continue;
                },
                Ok(()) => {},
                Err(e) if e.code() == ERROR_NO_DATA.to_hresult() => {},
                Err(e) => return error_value("read", e, true),
            }
        }
        // Inspect available messages before cancellation so a just-finished child can be accounted for.
        if stop.load(Ordering::Acquire) { return json!({"connected": connected, "cancelled": true, "round_trip": false}); }
        if Instant::now() >= deadline { return json!({"connected": connected, "deadline": true, "round_trip": false}); }
        thread::sleep(POLL);
    }
}

fn exchange(token: &str, label: &str, namespace: Namespace) -> Result<Value> {
    let endpoint = name(token, label, namespace)?;
    let handle = match unsafe { CreateFileW(&HSTRING::from(endpoint), CLIENT_ACCESS, FILE_SHARE_MODE(0), None,
        OPEN_EXISTING, SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, None) } {
        Ok(raw) => unsafe { OwnedHandle::from_raw_handle(raw.0) },
        Err(e) => return Ok(json!({"open_error": e.code().0 as u32 & 0xffff, "round_trip": false})),
    };
    let raw = HANDLE(handle.as_raw_handle());
    let mut pid = 0;
    unsafe { GetNamedPipeServerProcessId(raw, &mut pid)?; }
    if pid.to_string() != token.split_once('-').unwrap().0 { return Err("unexpected synthetic pipe server process".into()); }
    let request = packet(token, label);
    write(raw, &request)?; // <256 bytes, one freshly opened 4KiB pipe; never writes to a product endpoint.
    let mut expected = b"ack:".to_vec(); expected.extend_from_slice(&request);
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut available = 0;
        unsafe { PeekNamedPipe(raw, None, 0, None, Some(&mut available), None)?; }
        if available > 256 { return Err("oversized synthetic pipe reply".into()); }
        if available != 0 {
            let mut buffer = [0u8; 256];
            let mut count = 0;
            unsafe { ReadFile(raw, Some(&mut buffer[..available as usize]), Some(&mut count), None)?; }
            received.extend_from_slice(&buffer[..count as usize]);
            if received.len() >= expected.len() {
                if received != expected { return Err("synthetic pipe reply mismatch".into()); }
                write(raw, b"done")?;
                return Ok(json!({"round_trip": true, "server_pid": pid, "client_access": format!("0x{CLIENT_ACCESS:08x}")}));
            }
        }
        if Instant::now() >= deadline { return Err("synthetic pipe reply deadline".into()); }
        thread::sleep(POLL);
    }
}

pub(crate) fn clients(token: &str, container: bool, namespace: Namespace) -> Result<Value> {
    if !valid_token(token) { return Err("invalid synthetic pipe token".into()); }
    let prefix = if container { "container" } else { "desktop" };
    let mut result = json!({});
    for access in ["denied", "allowed"] {
        result[access] = match exchange(token, &format!("{prefix}-{access}"), namespace) {
            Ok(value) => value,
            Err(e) => json!({"error": e.to_string(), "round_trip": false}),
        };
    }
    Ok(result)
}

pub(crate) fn absence(token: &str, namespace: Namespace) -> Result<Value> {
    let mut result = json!({"all_absent": true});
    for label in LABELS {
        let endpoint = HSTRING::from(name(token, label, namespace)?);
        // Do not connect or expose even an identification token to a surviving/recreated listener.
        // Capture the raw API's last error before JSON allocation or temporary-string destruction.
        let available = unsafe { WaitNamedPipeW(&endpoint, 1) }.as_bool();
        let error = if available { 0 } else { unsafe { GetLastError() }.0 };
        result[label] = if available {
            json!({"available": true})
        } else {
            json!({"wait_error": error})
        };
        if result[label]["wait_error"] != 2 { result["all_absent"] = json!(false); }
    }
    Ok(result)
}

struct Server { label: &'static str, task: Option<JoinHandle<Value>> }
pub(crate) struct Experiment { stop: Arc<AtomicBool>, servers: Vec<Server> }
impl Experiment {
    pub(crate) fn start(token: &str, profile_sid: &str, namespace: Namespace) -> Result<Self> {
        if !valid_token(token) || token.split_once('-').unwrap().0 != std::process::id().to_string() {
            return Err("only this process can create its synthetic endpoints".into());
        }
        let user = user_sid()?;
        let mut experiment = Self { stop: Arc::new(AtomicBool::new(false)), servers: Vec::new() };
        for label in LABELS {
            let owned = create(token, label, &user, profile_sid, namespace)?;
            let expected = packet(token, label);
            let stop = experiment.stop.clone();
            let task = thread::Builder::new().name(format!("probe-{label}")).spawn(move || serve(owned, expected, stop))?;
            experiment.servers.push(Server { label, task: Some(task) });
        }
        Ok(experiment)
    }

    pub(crate) fn finish(&mut self) -> Value {
        self.stop.store(true, Ordering::Release);
        let mut result = json!({});
        for server in &mut self.servers {
            if let Some(task) = server.task.take() {
                result[server.label] = task.join().unwrap_or_else(|_| json!({"panic": true, "round_trip": false}));
            }
        }
        result
    }
}
impl Drop for Experiment { fn drop(&mut self) { self.finish(); } }

#[cfg(test)]
mod tests {
    use super::*;

    fn complete() -> Value {
        json!({"expected_parent_pid": 123, "expected_child_pid": 456,
            "desktop": {"denied": {"round_trip": true}, "allowed": {"round_trip": true}},
            "container": {"denied": {"open_error": 5}, "allowed": {"round_trip": true}},
            "servers": {"desktop-denied": {"round_trip": true, "client_pid": 123}, "desktop-allowed": {"round_trip": true, "client_pid": 123},
                "container-denied": {"connected": false, "cancelled": true}, "container-allowed": {"round_trip": true, "client_pid": 456}},
            "handles_closed": true})
    }

    #[test]
    fn endpoint_tokens_cannot_choose_live_or_remote_pipes() {
        assert!(valid_token("123-456789"));
        for token in ["", "0-42", "123", "123-", "-456", "1-2-3", "1-..", "1-\\remote", "1-abc"] {
            assert!(!valid_token(token), "{token}");
        }
        assert!(!valid_token(&format!("1-{}", "1".repeat(70))));
    }

    #[test]
    fn client_access_excludes_server_creation_and_acl_mutation() {
        use windows::Win32::Storage::FileSystem::{FILE_APPEND_DATA, FILE_GENERIC_READ, FILE_WRITE_DATA};
        assert_eq!(CLIENT_ACCESS, FILE_GENERIC_READ.0 | FILE_WRITE_DATA.0);
        assert_eq!(CLIENT_ACCESS & (FILE_APPEND_DATA.0 | 0x0004_0000 | 0x0008_0000), 0);
    }

    #[test]
    fn profile_acl_cannot_name_all_packages_or_accept_sddl_injection() {
        assert!(valid_profile_sid("S-1-15-2-100-200-300-400-500-600-700"));
        for sid in ["AC", "S-1-15-2-1", "S-1-15-2-2", "S-1-5-18", "S-1-15-2-1-2-3-4-5-6-4294967296",
            "S-1-15-2-1-2-3-4-5-6-7)(A;;GA;;;WD)"] { assert!(!valid_profile_sid(sid)); }
        assert!(Namespace::parse("\\remote").is_err());
    }

    #[test]
    fn transport_verdict_requires_container_confirmation_both_directions_and_cleanup() {
        let good = complete();
        assert!(passed(&good, true));
        assert!(!passed(&good, false));
        for pointer in ["/desktop/denied/round_trip", "/desktop/allowed/round_trip",
            "/container/allowed/round_trip", "/servers/desktop-denied/round_trip",
            "/servers/desktop-allowed/round_trip", "/servers/container-allowed/round_trip", "/handles_closed"] {
            let mut incomplete = good.clone();
            *incomplete.pointer_mut(pointer).unwrap() = json!(false);
            assert!(!passed(&incomplete, true), "{pointer}");
        }
        let mut wrong_denial = good.clone();
        wrong_denial["container"]["denied"]["open_error"] = json!(2);
        assert!(!passed(&wrong_denial, true));
        let mut unexpected_connection = good;
        unexpected_connection["servers"]["container-denied"]["connected"] = json!(true);
        assert!(!passed(&unexpected_connection, true));
    }

    #[test]
    fn transport_verdict_rejects_receipts_from_a_different_peer() {
        for label in ["desktop-denied", "desktop-allowed", "container-allowed"] {
            let mut report = complete();
            report["servers"][label]["client_pid"] = json!(789);
            assert!(!passed(&report, true), "wrong peer on {label}");
        }
        let mut missing = complete();
        missing["expected_child_pid"] = Value::Null;
        missing["servers"]["container-allowed"]["client_pid"] = Value::Null;
        assert!(!passed(&missing, true));
    }

    #[test]
    fn desktop_round_trip_and_unconnected_servers_release_owned_handles() {
        let token = format!("{}-{}", std::process::id(), std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        for namespace in [Namespace::Local, Namespace::Direct] {
        let mut experiment = Experiment::start(&token, "S-1-15-2-100-200-300-400-500-600-700", namespace).unwrap();
        let result = clients(&token, false, namespace).unwrap();
        let servers = experiment.finish();
        assert_eq!(result["denied"]["round_trip"], true, "{result}");
        assert_eq!(result["allowed"]["round_trip"], true, "{result}");
        assert_eq!(servers["desktop-denied"]["round_trip"], true, "{servers}");
        assert_eq!(servers["desktop-allowed"]["round_trip"], true, "{servers}");
        assert_eq!(servers["container-denied"]["cancelled"], true);
        assert_eq!(servers["container-allowed"]["cancelled"], true);
        // A successful join alone is not the external postcondition: no endpoint may remain open.
        assert_eq!(absence(&token, namespace).unwrap()["all_absent"], true);
        }
    }

    #[test]
    fn absence_check_does_not_connect_to_an_existing_listener() {
        let token = format!("{}-{}", std::process::id(), std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let mut experiment = Experiment::start(&token, "S-1-15-2-100-200-300-400-500-600-700", Namespace::Direct).unwrap();
        assert_eq!(absence(&token, Namespace::Direct).unwrap()["all_absent"], false);
        let servers = experiment.finish();
        for label in LABELS {
            assert_eq!(servers[label]["connected"], false, "{servers}");
            assert_eq!(servers[label]["cancelled"], true, "{servers}");
        }
        assert_eq!(absence(&token, Namespace::Direct).unwrap()["all_absent"], true);
    }
}
