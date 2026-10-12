//! The DLL's COM entry points. `DllRegisterServer` registers for the current user only (regsvr32 without elevation);
//! `hanautomata-ime-setup` does the same steps with a report.

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

/// This DLL's path, for its own registration.
pub fn dll_path() -> String {
    let module = HINSTANCE(MODULE.load(Ordering::Relaxed));
    let mut buf = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(Some(module.into()), &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
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
    let report = registration::register(&dll_path(), false);
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
