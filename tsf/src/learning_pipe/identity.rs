//! OS-observed peer identity, never a SID/PID claimed in a request.
use super::{Error, Result};
use sha2::{Digest, Sha256};
use std::{mem::size_of, os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle}, sync::Arc};
use windows::{core::PWSTR, Win32::{Foundation::{HANDLE, HLOCAL, FILETIME, LocalFree, ERROR_NO_TOKEN},
    Security::{Authorization::ConvertSidToStringSidW, GetTokenInformation, IsValidSid, GetSidSubAuthority,
        GetSidSubAuthorityCount, RevertToSelf, TokenUser, TokenLogonSid, TokenSessionId, TokenStatistics,
        TokenElevation, TokenIsAppContainer, TokenRestrictedSids, TokenIntegrityLevel, TokenImpersonationLevel,
        TOKEN_INFORMATION_CLASS, TOKEN_USER, TOKEN_GROUPS, TOKEN_STATISTICS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY},
    System::{Threading::{OpenProcess, OpenProcessToken, OpenThreadToken, GetCurrentThread, GetProcessTimes,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE},
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeClientSessionId, GetNamedPipeServerProcessId,
            GetNamedPipeServerSessionId, ImpersonateNamedPipeClient}},
}};

fn owned(raw: HANDLE) -> OwnedHandle { unsafe { OwnedHandle::from_raw_handle(raw.0) } }
struct LocalMemory(HLOCAL);
impl Drop for LocalMemory { fn drop(&mut self) { unsafe { LocalFree(Some(self.0)); } } }
fn sid(sid: windows::Win32::Security::PSID) -> Result<String> {
    if !unsafe { IsValidSid(sid) }.as_bool() { return Err(Error::Unauthorized); }
    let mut text = PWSTR::null(); unsafe { ConvertSidToStringSidW(sid, &mut text) }?;
    let _memory = LocalMemory(HLOCAL(text.0.cast())); Ok(unsafe { text.to_string() }.map_err(|_| Error::Unauthorized)?)
}
fn token_data(handle: HANDLE, class: TOKEN_INFORMATION_CLASS, minimum: usize) -> Result<Vec<usize>> {
    let mut bytes = 0;
    unsafe { let _ = GetTokenInformation(handle, class, None, 0, &mut bytes); }
    if bytes < minimum as u32 || bytes > 65536 { return Err(Error::Unauthorized); }
    let capacity = bytes; let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    unsafe { GetTokenInformation(handle, class, Some(buffer.as_mut_ptr().cast()), capacity, &mut bytes) }?;
    if bytes < minimum as u32 || bytes > capacity { return Err(Error::Unauthorized); } Ok(buffer)
}
fn scalar(handle: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<u32> {
    let buffer = token_data(handle, class, size_of::<u32>())?; Ok(unsafe { *buffer.as_ptr().cast::<u32>() })
}
#[derive(Clone, PartialEq, Eq)]
struct Token {
    user: String, logon: String, session: u32, authentication: (u32, i32),
    elevated: u32, container: u32, restricted: u32, integrity: u32,
}
impl Token {
    fn read(handle: HANDLE) -> Result<Self> {
        let user = token_data(handle, TokenUser, size_of::<TOKEN_USER>())?;
        let user = sid(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid })?;
        let groups = token_data(handle, TokenLogonSid, size_of::<TOKEN_GROUPS>())?;
        let groups = unsafe { &*groups.as_ptr().cast::<TOKEN_GROUPS>() };
        if groups.GroupCount != 1 || groups.Groups[0].Attributes & 0xc000_0000 != 0xc000_0000 { return Err(Error::Unauthorized); }
        let logon = sid(groups.Groups[0].Sid)?;
        let statistics = token_data(handle, TokenStatistics, size_of::<TOKEN_STATISTICS>())?;
        let authentication = unsafe { (*statistics.as_ptr().cast::<TOKEN_STATISTICS>()).AuthenticationId };
        let label = token_data(handle, TokenIntegrityLevel, size_of::<TOKEN_MANDATORY_LABEL>())?;
        let integrity_sid = unsafe { (*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
        if !unsafe { IsValidSid(integrity_sid) }.as_bool() { return Err(Error::Unauthorized); }
        let count = unsafe { *GetSidSubAuthorityCount(integrity_sid) };
        if count == 0 { return Err(Error::Unauthorized); }
        Ok(Self { user, logon, authentication: (authentication.LowPart, authentication.HighPart),
            session: scalar(handle, TokenSessionId)?, elevated: scalar(handle, TokenElevation)?,
            container: scalar(handle, TokenIsAppContainer)?, restricted: scalar(handle, TokenRestrictedSids)?,
            integrity: unsafe { *GetSidSubAuthority(integrity_sid, (count - 1) as u32) } })
    }
    fn allowed(&self) -> bool { self.session != 0 && self.elevated == 0 && self.container == 0 && self.restricted == 0 && self.integrity == 0x2000 }
    fn same_logon(&self, other: &Self) -> bool {
        self.user == other.user && self.logon == other.logon && self.session == other.session && self.authentication == other.authentication
    }
}

/// Pin the exact OS process object supplied by a trusted launcher, not a PID from the wire.
/// No automatic discovery or executable trust policy is implied by this development API.
#[derive(Clone)]
pub struct Peer { _process: Arc<OwnedHandle>, pid: u32, born: u64, token: Token }
impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Peer { authenticated OS identity, redacted }") }
}
impl Peer {
    pub fn process(pid: u32) -> Result<Self> {
        if pid == 0 { return Err(Error::Unauthorized); }
        let process = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid) }?);
        let raw = HANDLE(process.as_raw_handle()); let mut token = HANDLE::default();
        unsafe { OpenProcessToken(raw, TOKEN_QUERY, &mut token) }?; let token = owned(token);
        let token = Token::read(HANDLE(token.as_raw_handle()))?;
        if !token.allowed() { return Err(Error::Unauthorized); }
        let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) }?;
        let born = (created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64;
        if born == 0 { return Err(Error::Unauthorized); }
        Ok(Self { _process: Arc::new(process), pid, born, token })
    }
    pub(super) fn logon_sid(&self) -> &str { &self.token.logon }
    pub(super) fn bound_client(&self, nonce: &str) -> Result<String> {
        if !super::valid_token(nonce) { return Err(Error::Protocol); }
        // Delimited, versioned values; the user supplies only the nonce. OS identity binds the lease across reconnects.
        let identity = format!("Hanautomata.LearningPeer/1\n{}\n{}\n{}\n{}:{}\n{}\n{}\n{}",
            self.token.user, self.token.logon, self.token.session, self.token.authentication.0, self.token.authentication.1,
            self.pid, self.born, nonce);
        let digest = Sha256::digest(identity.as_bytes());
        let id: String = digest[..16].iter().map(|byte| format!("{byte:02x}")).collect();
        if !super::valid_token(&id) { return Err(Error::Unauthorized); } Ok(id)
    }
    pub(super) fn check_server(&self, pipe: HANDLE) -> Result<()> {
        let (mut pid, mut session) = (0, 0);
        unsafe { GetNamedPipeServerProcessId(pipe, &mut pid)?; GetNamedPipeServerSessionId(pipe, &mut session)?; }
        let actual = Self::process(pid)?; let caller = Self::process(std::process::id())?;
        if pid != self.pid || actual.born != self.born || actual.token != self.token
            || session != self.token.session || !caller.token.same_logon(&self.token) { return Err(Error::Unauthorized); } Ok(())
    }
    pub(super) fn connected_client(pipe: HANDLE, owner: &Self) -> Result<Self> {
        no_thread_token()?;
        let (mut pid, mut session) = (0, 0);
        unsafe { GetNamedPipeClientProcessId(pipe, &mut pid)?; GetNamedPipeClientSessionId(pipe, &mut session)?; }
        let peer = Self::process(pid)?;
        if session != peer.token.session || !owner.token.same_logon(&peer.token) { return Err(Error::Unauthorized); }
        Ok(peer) // Keep this exact process object alive while receiving the message.
    }
    pub(super) fn check_client_message(&self, pipe: HANDLE) -> Result<()> {
        no_thread_token()?;
        let (mut pid, mut session) = (0, 0);
        unsafe { GetNamedPipeClientProcessId(pipe, &mut pid)?; GetNamedPipeClientSessionId(pipe, &mut session)?; }
        if pid != self.pid || session != self.token.session { return Err(Error::Unauthorized); }
        unsafe { ImpersonateNamedPipeClient(pipe) }?;
        let revert = Revert;
        let mut raw = HANDLE::default(); unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut raw) }?;
        let token = owned(raw); let identity = Token::read(HANDLE(token.as_raw_handle()))?;
        let level = scalar(HANDLE(token.as_raw_handle()), TokenImpersonationLevel)?;
        drop(revert); // Before any persistent-file access, even if the identity will be refused.
        if level < 1 || identity != self.token { return Err(Error::Unauthorized); } Ok(())
    }
}
pub(super) fn no_thread_token() -> Result<()> {
    let mut raw = HANDLE::default();
    match unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut raw) } {
        Ok(()) => { drop(owned(raw)); Err(Error::Unauthorized) },
        Err(error) if error.code() == ERROR_NO_TOKEN.to_hresult() => Ok(()), Err(_) => Err(Error::Unauthorized),
    }
}
struct Revert;
impl Drop for Revert {
    fn drop(&mut self) {
        // This module is confined to the development broker. Continuing under the peer's identity is unsafe.
        if unsafe { RevertToSelf() }.is_err() { std::process::abort(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_binding_changes_for_every_security_and_lifetime_boundary() {
        let peer = Peer::process(std::process::id()).unwrap(); let nonce = "11111111111111111111111111111111";
        let original = peer.bound_client(nonce).unwrap(); assert_eq!(original, peer.clone().bound_client(nonce).unwrap());
        for index in 0..7 {
            let mut changed = peer.clone();
            match index { 0 => changed.pid += 1, 1 => changed.born += 1, 2 => changed.token.user.push('1'),
                3 => changed.token.logon.push('1'), 4 => changed.token.session += 1,
                5 => changed.token.authentication.0 += 1, _ => changed.token.authentication.1 += 1 }
            assert_ne!(original, changed.bound_client(nonce).unwrap());
            assert_eq!(peer.token.same_logon(&changed.token), index < 2,
                "same PID or nonce must never authorize a different user/logon/session");
        }
        assert_ne!(original, peer.bound_client("22222222222222222222222222222222").unwrap());
        assert!(!format!("{peer:?}").contains(&peer.token.user));
    }
    #[test]
    fn unknown_or_non_desktop_context_is_not_a_writer() {
        let peer = Peer::process(std::process::id()).unwrap(); assert!(peer.token.allowed());
        for index in 0..5 { let mut changed = peer.token.clone();
            match index { 0 => changed.session = 0, 1 => changed.elevated = 1, 2 => changed.container = 1,
                3 => changed.restricted = 1, _ => changed.integrity = 0x1000 }
            assert!(!changed.allowed());
        }
    }
}
