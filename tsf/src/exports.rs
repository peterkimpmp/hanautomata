//! The DLL's COM entry points. Registration delegates to the setup policy; creating a service does not register it.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::core::{implement, IUnknown, Interface, Ref, Result, GUID, HRESULT};
use windows::Win32::Foundation::{BOOL, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, HINSTANCE, S_FALSE, S_OK};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

use crate::guids::CLSID_TEXT_SERVICE;
use crate::registration;
use crate::service::TextService;

static MODULE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Resolve this DLL (never the host EXE). Preserve UTF-16 paths and reject truncation or missing DllMain state.
pub(crate) fn module_path() -> Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::E_FAIL;
    let module = MODULE.load(Ordering::Relaxed);
    if module.is_null() { return Err(E_FAIL.into()); }
    let mut size = 512;
    loop {
        let mut buf = vec![0u16; size];
        let n = unsafe { GetModuleFileNameW(Some(HINSTANCE(module).into()), &mut buf) } as usize;
        if n == 0 { return Err(windows::core::Error::from_win32()); }
        if n < buf.len() {
            let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
            return if path.is_absolute() { Ok(path) } else { Err(E_FAIL.into()) };
        }
        if size == 32768 { return Err(E_FAIL.into()); }
        size = (size * 2).min(32768);
    }
}

#[no_mangle]
pub extern "system" fn DllMain(module: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module.0, Ordering::Relaxed);
        unsafe {
            let _ = DisableThreadLibraryCalls(module.into());
        }
    }
    true.into()
}

#[implement(IClassFactory)]
struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(&self, punkouter: Ref<IUnknown>, riid: *const GUID, ppvobject: *mut *mut c_void) -> Result<()> {
        if ppvobject.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppvobject = std::ptr::null_mut() };
        if punkouter.as_ref().is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let service: IUnknown = TextService::new().into();
        unsafe { service.query(riid, ppvobject).ok() }
    }

    fn LockServer(&self, _flock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// # Safety
/// COM calls it with valid pointers.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() || rclsid.is_null() || riid.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();
    if *rclsid != CLSID_TEXT_SERVICE {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = ClassFactory.into();
    factory.query(riid, ppv)
}

/// Text services stay loaded for the life of the application thread; never unload.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    let path = match module_path() { Ok(path) => path, Err(error) => return error.code() };
    let Some(path) = path.to_str() else { return windows::Win32::Foundation::E_FAIL; };
    let report = registration::register(path, false);
    if report.ok() {
        S_OK
    } else {
        report.first_error()
    }
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    let report = registration::unregister();
    if report.ok() {
        S_OK
    } else {
        report.first_error()
    }
}
