//! OS limits live in the child, never in an IME or its supervisor.
use crate::Error;

#[cfg(unix)]
pub fn limit(mib: u64) -> Result<(), Error> {
    // The tested macOS 26.6.2 host rejects finite DATA/AS/RSS limits (EINVAL); sample RSS instead.
    #[cfg(not(target_os = "macos"))] {
        let bytes = mib * 1024 * 1024;
        let budget = libc::rlimit { rlim_cur: bytes as libc::rlim_t, rlim_max: bytes as libc::rlim_t };
        if unsafe { libc::setrlimit(libc::RLIMIT_DATA, &budget) } != 0 { return Err(Error("resource_limit_failed")); }
    }
    #[cfg(target_os = "macos")] let _ = mib;
    let core = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &core) } != 0 { return Err(Error("resource_limit_failed")); }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn resident_bytes(pid: u32) -> Option<u64> {
    let mut usage: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V2, (&mut usage as *mut libc::rusage_info_v2).cast()) };
    (result == 0).then_some(usage.ri_resident_size)
}
#[cfg(not(target_os = "macos"))]
pub fn resident_bytes(_: u32) -> Option<u64> { None }

pub fn policy() -> &'static str {
    if cfg!(target_os = "macos") { "sampled_rss_5ms_not_hard_cap" }
    else if cfg!(windows) { "job_process_commit_limit" } else { "rlimit_data" }
}

#[cfg(windows)]
pub fn limit(mib: u64) -> Result<(), Error> {
    use windows_sys::Win32::System::{JobObjects::*, Threading::GetCurrentProcess};
    // Keep the job handle for the child lifetime. Windows releases it on process termination.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() { return Err(Error("resource_limit_failed")); }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    limits.ProcessMemoryLimit = (mib * 1024 * 1024) as usize;
    let ok = unsafe {
        SetInformationJobObject(job, JobObjectExtendedLimitInformation, (&limits as *const _) as _, std::mem::size_of_val(&limits) as u32) != 0
            && AssignProcessToJobObject(job, GetCurrentProcess()) != 0
    };
    if ok { Ok(()) } else { Err(Error("resource_limit_failed")) }
}
