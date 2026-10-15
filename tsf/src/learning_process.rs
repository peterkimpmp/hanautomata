//! Narrow child launcher: no inherited host handles, explicit executable and working directory, no shell or UAC.
use std::{ffi::{OsStr, OsString}, os::windows::{ffi::OsStrExt, io::{AsRawHandle, FromRawHandle, OwnedHandle}}, path::Path};
use windows::{core::{PCWSTR, PWSTR}, Win32::{Foundation::{HANDLE, WAIT_OBJECT_0}, System::Threading::{
    CreateProcessW, GetExitCodeProcess, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW}}};
pub(super) struct Child { process: OwnedHandle }
fn quote(value: &OsStr, command: &mut Vec<u16>) -> std::io::Result<()> {
    command.push(34); let mut slashes = 0;
    for c in value.encode_wide() {
        if c == 0 { return Err(std::io::ErrorKind::InvalidInput.into()); }
        if c == 92 { slashes += 1; continue; }
        command.extend(std::iter::repeat_n(92, if c == 34 { slashes * 2 + 1 } else { slashes }));
        slashes = 0; command.push(c);
    }
    command.extend(std::iter::repeat_n(92, slashes * 2)); command.push(34); Ok(())
}
impl Child {
    pub fn spawn(executable: &Path, args: &[OsString]) -> std::io::Result<Self> { Self::launch(executable, args, false) }
    fn launch(executable: &Path, args: &[OsString], inherit: bool) -> std::io::Result<Self> {
        if !executable.is_absolute() { return Err(std::io::ErrorKind::InvalidInput.into()); }
        let mut command = Vec::new(); quote(executable.as_os_str(), &mut command)?;
        for arg in args { command.push(32); quote(arg, &mut command)?; }
        command.push(0); if command.len() > 32767 { return Err(std::io::ErrorKind::InvalidInput.into()); }
        let application: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
        let directory: Vec<u16> = executable.parent().ok_or(std::io::ErrorKind::InvalidInput)?.as_os_str().encode_wide().chain([0]).collect();
        let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
        let mut process = PROCESS_INFORMATION::default();
        unsafe { CreateProcessW(PCWSTR(application.as_ptr()), Some(PWSTR(command.as_mut_ptr())), None, None,
            inherit, CREATE_NO_WINDOW, None, PCWSTR(directory.as_ptr()), &startup, &mut process) }.map_err(std::io::Error::from)?;
        let handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess.0) };
        drop(unsafe { OwnedHandle::from_raw_handle(process.hThread.0) });
        Ok(Self { process: handle })
    }
    pub(super) fn handle(&self) -> HANDLE { HANDLE(self.process.as_raw_handle()) }
    pub(super) fn peer(&self) -> Result<crate::learning_pipe::Peer, crate::learning_pipe::Error> {
        crate::learning_pipe::Peer::from_owned_process(self.process.try_clone().map_err(|_| crate::learning_pipe::Error::Io)?)
    }
    pub fn try_wait(&self) -> std::io::Result<Option<u32>> {
        let handle = HANDLE(self.process.as_raw_handle());
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_OBJECT_0 => { let mut code = 0; unsafe { GetExitCodeProcess(handle, &mut code) }.map_err(std::io::Error::from)?; Ok(Some(code)) },
            windows::Win32::Foundation::WAIT_TIMEOUT => Ok(None),
            _ => Err(std::io::Error::last_os_error()),
        }
    }
}
pub(super) fn child_policy_allows_spawn() -> bool {
    use windows::Win32::System::{Threading::{GetCurrentProcess, GetProcessMitigationPolicy, ProcessChildProcessPolicy},
        SystemServices::PROCESS_MITIGATION_CHILD_PROCESS_POLICY};
    let mut policy = PROCESS_MITIGATION_CHILD_PROCESS_POLICY::default();
    if unsafe { GetProcessMitigationPolicy(GetCurrentProcess(), ProcessChildProcessPolicy,
        (&mut policy as *mut PROCESS_MITIGATION_CHILD_PROCESS_POLICY).cast(), std::mem::size_of_val(&policy)) }.is_err() { return false; }
    // No restriction or audit-only. Secure-child exceptions and unknown future flags are not a bypass.
    matches!(unsafe { policy.Anonymous.Flags }, 0 | 2)
}
impl Drop for Child { fn drop(&mut self) {
    let handle = HANDLE(self.process.as_raw_handle());
    if self.try_wait().ok().flatten().is_none() {
        unsafe { let _ = TerminateProcess(handle, 2); let _ = WaitForSingleObject(handle, 2000); }
    }
} }

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{Security::SECURITY_ATTRIBUTES, System::Threading::{CreateEventW, SetEvent}, Foundation::WAIT_TIMEOUT};
    #[test]
    #[ignore = "tightens child policy only in an owned disposable test process"]
    fn restricted_child_fixture() {
        use windows::Win32::System::{Threading::{SetProcessMitigationPolicy, ProcessChildProcessPolicy},
            SystemServices::{PROCESS_MITIGATION_CHILD_PROCESS_POLICY, PROCESS_MITIGATION_CHILD_PROCESS_POLICY_0}};
        let policy = PROCESS_MITIGATION_CHILD_PROCESS_POLICY { Anonymous: PROCESS_MITIGATION_CHILD_PROCESS_POLICY_0 { Flags: 1 } };
        unsafe { SetProcessMitigationPolicy(ProcessChildProcessPolicy, (&policy as *const PROCESS_MITIGATION_CHILD_PROCESS_POLICY).cast(),
            std::mem::size_of_val(&policy)).unwrap(); }
        assert!(!child_policy_allows_spawn());
        let result = crate::learning_runtime::Runtime::start(std::env::current_exe().unwrap(), std::env::temp_dir().join("must-not-create-learning.db"));
        assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied));
        assert!(crate::learning_runtime::spawn_denied());
    }
    #[test]
    fn actual_child_policy_is_checked_before_attempting_to_spawn_the_companion() {
        let child = Child::spawn(&std::env::current_exe().unwrap(),
            &["--ignored".into(), "--exact".into(), "learning_process::tests::restricted_child_fixture".into()]).unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < end); std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(child.try_wait().unwrap(), Some(0));
    }
    #[test]
    #[ignore = "owned UTF-16 argv round-trip fixture"]
    fn argument_fixture() {
        let arguments: Vec<Vec<u16>> = std::env::args_os().map(|arg| arg.encode_wide().collect()).collect();
        std::fs::write(std::env::var_os("HANAUTOMATA_ARGV_TEST").unwrap(), serde_json::to_vec(&arguments).unwrap()).unwrap();
    }
    #[test]
    fn command_line_round_trips_quotes_trailing_slashes_and_empty_arguments() {
        let output = std::env::temp_dir().join(format!("hanautomata-argv-{}.json", crate::learning_pipe::Endpoint::random().unwrap().token()));
        let values = vec![OsString::from(""), OsString::from("한 글"), OsString::from("a\"b"), OsString::from(r"tail\"),
            OsString::from(r#"two\\\"quote"#)];
        let mut arguments: Vec<OsString> = vec!["--ignored".into(), "--exact".into(), "learning_process::tests::argument_fixture".into(), "--".into()];
        arguments.extend(values.iter().cloned());
        std::env::set_var("HANAUTOMATA_ARGV_TEST", &output);
        let child = Child::spawn(&std::env::current_exe().unwrap(), &arguments).unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < end); std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::env::remove_var("HANAUTOMATA_ARGV_TEST");
        assert_eq!(child.try_wait().unwrap(), Some(0));
        let received: Vec<Vec<u16>> = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        std::fs::remove_file(output).unwrap();
        assert_eq!(&received[received.len() - values.len()..], values.iter().map(|value| value.encode_wide().collect::<Vec<_>>()).collect::<Vec<_>>());
    }
    #[test]
    fn native_argument_parser_preserves_unpaired_utf16() {
        use std::os::windows::ffi::OsStringExt;
        use windows::Win32::{UI::Shell::CommandLineToArgvW, Foundation::{HLOCAL, LocalFree}};
        let value = OsString::from_wide(&[0xd800, 32, 34, 92, 0x61, 92]);
        let mut command = vec![]; quote(OsStr::new(r"C:\bin\test.exe"), &mut command).unwrap(); command.push(32);
        quote(&value, &mut command).unwrap(); command.push(0);
        let mut count = 0; let arguments = unsafe { CommandLineToArgvW(PCWSTR(command.as_ptr()), &mut count) };
        assert!(!arguments.is_null()); assert_eq!(count, 2);
        let actual = unsafe {
            let argument = *arguments.add(1); let mut length = 0; while *argument.0.add(length) != 0 { length += 1; }
            let actual = std::slice::from_raw_parts(argument.0, length).to_vec(); let _ = LocalFree(Some(HLOCAL(arguments.cast()))); actual
        };
        assert_eq!(actual, value.encode_wide().collect::<Vec<_>>());
    }
    #[test]
    #[ignore = "explicit child fixture for handle inheritance and working-directory probe"]
    fn child_fixture() {
        let handle: usize = std::env::var("HANAUTOMATA_HANDLE_TEST").unwrap().parse().unwrap();
        unsafe { let _ = SetEvent(HANDLE(handle as *mut _)); }
        let output = std::env::var_os("HANAUTOMATA_DIRECTORY_TEST").unwrap();
        std::fs::write(output, std::env::current_dir().unwrap().to_string_lossy().as_bytes()).unwrap();
    }
    #[test]
    fn launcher_does_not_inherit_host_handles_or_working_directory() {
        let executable = std::env::current_exe().unwrap();
        let output = std::env::temp_dir().join(format!("hanautomata-child-{}.txt", crate::learning_pipe::Endpoint::random().unwrap().token()));
        let security = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            bInheritHandle: true.into(), ..Default::default() };
        let event = unsafe { CreateEventW(Some(&security), true, false, PCWSTR::null()).unwrap() };
        let event = unsafe { OwnedHandle::from_raw_handle(event.0) };
        std::env::set_var("HANAUTOMATA_HANDLE_TEST", (event.as_raw_handle() as usize).to_string());
        std::env::set_var("HANAUTOMATA_DIRECTORY_TEST", &output);
        for inherited in [false, true] {
            let child = Child::launch(&executable, &["--ignored".into(), "--exact".into(), "learning_process::tests::child_fixture".into()], inherited).unwrap();
            let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while child.try_wait().unwrap().is_none() {
                assert!(std::time::Instant::now() < end); std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(child.try_wait().unwrap(), Some(0));
            assert_eq!(unsafe { WaitForSingleObject(HANDLE(event.as_raw_handle()), 0) }, if inherited { WAIT_OBJECT_0 } else { WAIT_TIMEOUT });
            assert_eq!(std::path::PathBuf::from(std::fs::read_to_string(&output).unwrap()), executable.parent().unwrap());
        }
        std::env::remove_var("HANAUTOMATA_HANDLE_TEST"); std::env::remove_var("HANAUTOMATA_DIRECTORY_TEST");
        std::fs::remove_file(output).unwrap();
    }
}
