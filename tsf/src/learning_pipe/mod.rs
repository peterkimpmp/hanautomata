//! Authenticated transport for a single-writer learning authority, ordinary desktop peers only.
//! All calls belong on a worker, never a key callback.
//! The caller must obtain `Peer` from its trusted child/launcher; a wire PID or pipe name is not a trust anchor.
mod identity;
mod lifetime;
mod management;
pub use management::management_main;
pub use identity::Peer;
use hanautomata_core::learning::{Authority, Request, MAX_STATE_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{mem::size_of, os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle}, time::{Duration, Instant}};
use windows::{core::{HSTRING, PCWSTR}, Win32::{
    Foundation::{HANDLE, HLOCAL, LocalFree, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, ERROR_PIPE_BUSY, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1}, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
    Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE,
        OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT},
    System::{Com::CoCreateGuid, IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Threading::{CreateEventW, WaitForSingleObject, WaitForMultipleObjects}, Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
            PIPE_TYPE_BYTE, PIPE_READMODE_BYTE, PIPE_WAIT, PIPE_REJECT_REMOTE_CLIENTS}},
}};
// A one-time, authenticated legacy seed may contain up to CAPACITY entries. Ordinary operations retain
// the core's small word/identity bounds; the frame itself is still capped before allocation.
const MAX_COMMAND: usize = MAX_STATE_BYTES;
const MAX_REPLY: usize = MAX_STATE_BYTES * 2;
// FILE_GENERIC_READ | FILE_WRITE_DATA, excluding FILE_CREATE_PIPE_INSTANCE, WRITE_DAC and WRITE_OWNER.
const CLIENT_ACCESS: u32 = 0x0012_008b;
#[cfg(test)]
thread_local! {
    static ACCEPT_OBSERVER: std::cell::RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>> = const { std::cell::RefCell::new(None) };
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error { Io, Timeout, Protocol, Unauthorized, Cancelled }
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "learning pipe: {self:?}") }
}
impl std::error::Error for Error {}
impl From<windows::core::Error> for Error { fn from(_: windows::core::Error) -> Self { Self::Io } }
fn valid_token(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) && value.bytes().any(|b| b != b'0')
}
#[derive(Clone)]
pub struct Endpoint(String);
impl Endpoint {
    pub fn random() -> Result<Self> { Self::from_token(&format!("{:032x}", unsafe { CoCreateGuid() }?.to_u128())) }
    pub fn from_token(token: &str) -> Result<Self> {
        if !valid_token(token) { return Err(Error::Protocol); } Ok(Self(token.into()))
    }
    pub fn token(&self) -> &str { &self.0 }
    fn name(&self) -> HSTRING { HSTRING::from(format!(r"\\.\pipe\Hanautomata.Learning.Session.1.{}", self.0)) }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command { Register {}, RegisterSeeded { entries: Vec<hanautomata_core::LearnedWord> }, Snapshot {},
    Apply { request: Request }, Close { lease_revision: u64 } }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope { protocol: u32, nonce: String, command: Command }
impl Envelope {
    pub fn new(nonce: &str, command: Command) -> Result<Self> {
        if !valid_token(nonce) { return Err(Error::Protocol); } Ok(Self { protocol: 1, nonce: nonce.into(), command })
    }
}
struct LocalMemory(HLOCAL);
impl Drop for LocalMemory { fn drop(&mut self) { unsafe { LocalFree(Some(self.0)); } } }
fn owned(raw: HANDLE) -> OwnedHandle { unsafe { OwnedHandle::from_raw_handle(raw.0) } }
fn deadline(duration: Duration) -> Result<Instant> {
    if duration.is_zero() || duration > Duration::from_secs(30) { return Err(Error::Protocol); }
    Ok(Instant::now() + duration)
}
fn remaining(end: Instant) -> Result<u32> {
    let duration = end.checked_duration_since(Instant::now()).ok_or(Error::Timeout)?;
    Ok(duration.as_millis().max(1).min(u32::MAX as u128 - 1) as u32)
}
// Event, OVERLAPPED and caller's buffer stay alive until the operation has finished, including cancellation.
// CancelIoEx is a request, not completion: always drain it before releasing the stack/buffer.
fn io(handle: HANDLE, end: Instant, start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>) -> Result<u32> {
    io_until_peer_exit(handle, end, None, start)
}
fn io_until_peer_exit(handle: HANDLE, end: Instant, peer: Option<HANDLE>,
    start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>) -> Result<u32> {
    remaining(end)?;
    let event = owned(unsafe { CreateEventW(None, true, false, PCWSTR::null()) }?);
    let mut overlapped = OVERLAPPED { hEvent: HANDLE(event.as_raw_handle()), ..Default::default() };
    match start(&mut overlapped) {
        Ok(()) => {},
        Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(0),
        Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
            let wait = remaining(end).and_then(|ms| {
                let event = HANDLE(event.as_raw_handle());
                if let Some(peer) = peer {
                    match unsafe { WaitForMultipleObjects(&[peer, event], false, ms) } {
                        WAIT_OBJECT_0 => Err(Error::Cancelled), // Parent exit wins a simultaneous pipe completion.
                        value if value.0 == WAIT_OBJECT_0.0 + 1 => Ok(()),
                        WAIT_TIMEOUT => Err(Error::Timeout),
                        _ => Err(Error::Io),
                    }
                } else {
                    match unsafe { WaitForSingleObject(event, ms) } {
                        WAIT_OBJECT_0 => Ok(()), WAIT_TIMEOUT => Err(Error::Timeout), _ => Err(Error::Io),
                    }
                }
            });
            if let Err(error) = wait {
                unsafe {
                    let _ = CancelIoEx(handle, Some(&overlapped));
                    let mut ignored = 0; let _ = GetOverlappedResult(handle, &overlapped, &mut ignored, true);
                }
                return Err(error);
            }
        },
        Err(error) => return Err(error.into()),
    }
    let mut transferred = 0; unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, false) }?; Ok(transferred)
}
fn read_exact(handle: HANDLE, mut buffer: &mut [u8], end: Instant) -> Result<()> {
    while !buffer.is_empty() {
        let count = io(handle, end, |overlapped| unsafe { ReadFile(handle, Some(buffer), None, Some(overlapped)) })? as usize;
        if count == 0 || count > buffer.len() { return Err(Error::Protocol); } buffer = &mut buffer[count..];
    } Ok(())
}
fn write_all(handle: HANDLE, mut bytes: &[u8], end: Instant) -> Result<()> {
    while !bytes.is_empty() {
        let count = io(handle, end, |overlapped| unsafe { WriteFile(handle, Some(bytes), None, Some(overlapped)) })? as usize;
        if count == 0 || count > bytes.len() { return Err(Error::Protocol); } bytes = &bytes[count..];
    } Ok(())
}
fn read_packet(handle: HANDLE, maximum: usize, end: Instant) -> Result<Vec<u8>> {
    let mut length = [0u8; 4]; read_exact(handle, &mut length, end)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > maximum { return Err(Error::Protocol); }
    let mut bytes = vec![0; length]; read_exact(handle, &mut bytes, end)?; Ok(bytes)
}
fn write_packet(handle: HANDLE, bytes: &[u8], maximum: usize, end: Instant) -> Result<()> {
    if bytes.is_empty() || bytes.len() > maximum { return Err(Error::Protocol); }
    write_all(handle, &(bytes.len() as u32).to_le_bytes(), end)?; write_all(handle, bytes, end)
}

pub struct Listener { pipe: OwnedHandle, owner: Peer }
impl Listener {
    pub fn bind(endpoint: &Endpoint) -> Result<Self> {
        identity::no_thread_token()?; let owner = Peer::process(std::process::id())?;
        // Only this fresh synthetic endpoint. No Everyone, all-packages, low-integrity label, or machine changes.
        let sddl = HSTRING::from(format!("D:P(A;;0x12008b;;;{})", owner.logon_sid()));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut descriptor, None) }?;
        let _descriptor = LocalMemory(HLOCAL(descriptor.0));
        let attributes = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0, bInheritHandle: false.into() };
        let raw = unsafe { CreateNamedPipeW(&endpoint.name(), PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1, 8192, 8192, 0, Some(&attributes)) };
        if raw.is_invalid() { return Err(Error::Io); } Ok(Self { pipe: owned(raw), owner })
    }
    /// One authenticated exchange. A missing receipt means the reply may be lost, not that the write rolled back.
    /// Reconnect with exactly the same request to learn the committed result.
    pub fn serve_once(&mut self, authority: &mut Authority, timeout: Duration) -> Result<()> {
        self.exchange(None, timeout, |peer, envelope| dispatch(authority, peer, envelope))
    }
    /// Product companions admit only their launching process and nonce; no caller-supplied store paths.
    pub(crate) fn exchange(&mut self, expected: Option<&Peer>, timeout: Duration,
        dispatch: impl FnOnce(&Peer, Envelope) -> Value) -> Result<()> {
        self.exchange_with_budget(expected, timeout, timeout, dispatch)
    }
    fn exchange_with_budget(&mut self, expected: Option<&Peer>, accept_timeout: Duration, transport_timeout: Duration,
        dispatch: impl FnOnce(&Peer, Envelope) -> Value) -> Result<()> {
        identity::no_thread_token()?;
        let raw = HANDLE(self.pipe.as_raw_handle()); let end = deadline(accept_timeout)?;
        let result = (|| {
            #[cfg(test)] ACCEPT_OBSERVER.with(|observer| {
                if let Some(count) = observer.borrow().as_ref() { count.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
            });
            io_until_peer_exit(raw, end, expected.map(Peer::handle), |overlapped| unsafe { ConnectNamedPipe(raw, Some(overlapped)) })?;
            let end = deadline(transport_timeout)?; // Accept wait must not consume a connected request's budget.
            let peer = Peer::connected_client(raw, &self.owner)?;
            if expected.is_some_and(|expected| !expected.same_process(&peer)) { return Err(Error::Unauthorized); }
            let bytes = read_packet(raw, MAX_COMMAND, end)?;
            peer.check_client_message(raw)?;
            let result = match serde_json::from_slice::<Envelope>(&bytes) {
                Ok(envelope) if envelope.protocol == 1 && valid_token(&envelope.nonce) => dispatch(&peer, envelope),
                _ => json!({"ok":false,"error":"invalid_protocol"}),
            };
            let bytes = serde_json::to_vec(&result).map_err(|_| Error::Protocol)?;
            let end = deadline(transport_timeout)?; // Disk work does not consume the response's transfer budget.
            write_packet(raw, &bytes, MAX_REPLY, end)?;
            // Do not FlushFileBuffers (unbounded wait), or discard the reply by disconnecting immediately.
            let mut receipt = [0u8]; read_exact(raw, &mut receipt, end)?;
            if receipt != [1] { return Err(Error::Protocol); } Ok(())
        })();
        unsafe { let _ = DisconnectNamedPipe(raw); } result
    }
}
fn dispatch(authority: &mut Authority, peer: &Peer, envelope: Envelope) -> Value {
    if envelope.protocol != 1 { return json!({"ok":false,"error":"invalid_protocol"}); }
    let Ok(client_id) = peer.bound_client(&envelope.nonce) else { return json!({"ok":false,"error":"invalid_protocol"}); };
    let result = match envelope.command {
        Command::Register {} => authority.register(&client_id).map(|lease| json!({"ok":true,"lease":lease})),
        Command::RegisterSeeded { .. } => Err(hanautomata_core::learning::Error::InvalidRequest), // Only product initialization owns a path.
        Command::Snapshot {} => authority.snapshot().map(|snapshot| json!({"ok":true,"snapshot":snapshot})),
        Command::Close { lease_revision } => authority.retire(&client_id, lease_revision).map(|retired| json!({"ok":true,"retired":retired})),
        Command::Apply { request } => {
            if request.client_id != client_id { return json!({"ok":false,"error":"unauthorized"}); }
            authority.apply(&request).map(|reply| json!({"ok":true,"reply":reply}))
        },
    };
    result.unwrap_or_else(|error| json!({"ok":false,"error":error}))
}

/// A per-service companion takes the store lock for each transaction, never while waiting for a client.
/// All companions re-read committed state under that lock; an old in-process dictionary is never saved.
pub(crate) fn companion(endpoint: Endpoint, parent: Peer, nonce: String, path: std::path::PathBuf) -> Result<()> {
    use hanautomata_core::learning::{Error as StoreError, Lease, LeaseLifetime, Operation};
    if !valid_token(&nonce) || !path.is_absolute() { return Err(Error::Protocol); }
    let mut listener = Listener::bind(&endpoint)?;
    let mut lease: Option<Lease> = None;
    let mut lifetime: Option<(String, lifetime::Guard)> = None;
    let mut registration_attempted = false;
    let mut closed = false;
    let store_id = Endpoint::random()?;
    while parent.is_alive() && !closed {
        let result = listener.exchange_with_budget(Some(&parent), Duration::from_secs(30), Duration::from_secs(5), |peer, envelope| {
            if envelope.nonce != nonce { return json!({"ok":false,"error":"unauthorized"}); }
            if matches!(envelope.command, Command::Close { lease_revision: 0 }) && !registration_attempted {
                closed = true; return json!({"ok":true,"retired":false});
            }
            // No reset/forget/reconciliation through the input process's endpoint.
            if matches!(&envelope.command, Command::Apply { request } if !matches!(request.operation, Operation::Learn { .. })) {
                return json!({"ok":false,"error":"unauthorized"});
            }
            if lease.is_none() && !matches!(envelope.command, Command::Register {} | Command::RegisterSeeded { .. } | Command::Close { lease_revision: 0 }) {
                return json!({"ok":false,"error":"invalid_request"});
            }
            let outcome = (|| {
                let mut owner = if !registration_attempted && matches!(envelope.command, Command::Register {} | Command::RegisterSeeded { .. }) {
                    let entries = match &envelope.command { Command::RegisterSeeded { entries } => entries.as_slice(), _ => &[] };
                    if entries.len() > hanautomata_core::CAPACITY { return Err(StoreError::InvalidRequest); }
                    Authority::open_seeded(&path, store_id.token(), entries)?
                } else { Authority::open_existing(&path)? };
                let client = peer.bound_client(&nonce).map_err(|_| StoreError::InvalidRequest)?;
                match envelope.command {
                    Command::Register {} | Command::RegisterSeeded { .. } => {
                        let store = owner.snapshot()?.version.store_id;
                        if lifetime.as_ref().is_some_and(|(id, _)| id != &store) { return Err(StoreError::WrongStore); }
                        lifetime::reclaim(&mut owner, &store, &client)?;
                        if lifetime.is_none() { lifetime = Some((store.clone(), lifetime::acquire(&store, &client, peer)?)); }
                        registration_attempted = true;
                        let registered = owner.register_with_lifetime(&client, LeaseLifetime::WindowsMutexV1)?;
                        lease = Some(registered.clone()); Ok(json!({"ok":true,"lease":registered})) },
                    Command::Snapshot {} if lease.is_some() => Ok(json!({"ok":true,"snapshot":owner.snapshot()?})),
                    Command::Apply { request } if lease.as_ref().is_some_and(|l| l.client_id == request.client_id && l.lease_revision == request.lease_revision) =>
                        owner.apply(&request).map(|reply| json!({"ok":true,"reply":reply})),
                    Command::Close { lease_revision } if lease_revision == 0 || lease.as_ref().is_some_and(|l| l.lease_revision == lease_revision) => {
                        // A Register write may have committed before returning an I/O failure. Resolve only this
                        // authenticated session's ID; do not leak a lease merely because its reply was uncertain.
                        if let Some(registered) = owner.clients()?.iter().find(|l| l.key.client_id == client) {
                            owner.retire(&client, registered.key.lease_revision)?;
                        }
                        lease = None; registration_attempted = false; closed = true; Ok(json!({"ok":true,"retired":true}))
                    },
                    _ => Err(StoreError::InvalidRequest),
                }
            })();
            outcome.unwrap_or_else(|error| json!({"ok":false,"error":error}))
        });
        // Disconnects/timeouts do not discard the lease or change an uncertain operation's identity.
        if result == Err(Error::Cancelled) { break; }
    }
    if registration_attempted {
        let client = parent.bound_client(&nonce)?;
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            match Authority::open_existing(&path) {
                Ok(mut owner) => {
                    if let Ok(clients) = owner.clients() {
                        if let Some(registered) = clients.iter().find(|l| l.key.client_id == client) {
                            let _ = owner.retire(&client, registered.key.lease_revision);
                        }
                    }
                    break;
                },
                Err(StoreError::Busy) if Instant::now() < end => std::thread::sleep(Duration::from_millis(10)),
                _ => break,
            }
        }
    }
    Ok(())
}

/// Explicit management operation, never offered through an input companion's pipe.
/// No host is stopped. Existing-only open does not create/adopt/prune files. Busy/uncertain liveness leaves them intact.
/// This is a point-in-time check: keep TSF hosts closed until the binary rollback has finished.
pub fn prepare_rollback(path: &std::path::Path) -> std::result::Result<usize, hanautomata_core::learning::Error> {
    use hanautomata_core::learning::Error as StoreError;
    identity::no_thread_token().map_err(|_| StoreError::InvalidRequest)?;
    let _user = Peer::process(std::process::id()).map_err(|_| StoreError::InvalidRequest)?;
    if !path.is_absolute() { return Err(StoreError::InvalidRequest); }
    let mut owner = Authority::open_existing(path)?;
    let store = owner.snapshot()?.version.store_id;
    lifetime::prepare_rollback(&mut owner, &store)
}

/// Bounded worker-only round trip; authenticates the exact expected server before writing any request bytes.
pub fn call(endpoint: &Endpoint, expected: &Peer, envelope: &Envelope, timeout: Duration) -> Result<Value> {
    identity::no_thread_token()?;
    let bytes = serde_json::to_vec(envelope).map_err(|_| Error::Protocol)?;
    if bytes.len() > MAX_COMMAND || envelope.protocol != 1 || !valid_token(&envelope.nonce) { return Err(Error::Protocol); }
    let end = deadline(timeout)?;
    let pipe = loop {
        remaining(end)?;
        match unsafe { CreateFileW(&endpoint.name(), CLIENT_ACCESS, FILE_SHARE_MODE(0), None, OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, None) } {
            Ok(raw) => break owned(raw),
            Err(error) if error.code() == ERROR_PIPE_BUSY.to_hresult() => std::thread::sleep(Duration::from_millis(2)),
            Err(error) => return Err(error.into()),
        }
    };
    let raw = HANDLE(pipe.as_raw_handle()); expected.check_server(raw)?;
    let end = deadline(timeout)?;
    write_packet(raw, &bytes, MAX_COMMAND, end)?;
    let reply = read_packet(raw, MAX_REPLY, end)?;
    let result = serde_json::from_slice(&reply).map_err(|_| Error::Protocol)?;
    let _ = write_all(raw, &[1], end); Ok(result) // A complete authenticated response remains valid if its receipt is lost.
}

#[cfg(test)]
mod tests;
