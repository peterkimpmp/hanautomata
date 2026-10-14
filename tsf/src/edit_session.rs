//! Apartment-bound edit callback. windows-implement 0.59 always advertises IAgileObject,
//! which is invalid for closures holding TSF apartment interfaces, Rc or RefCell.
//! This single-interface COM object exposes only IUnknown and ITfEditSession.
use std::{cell::RefCell, ffi::c_void, mem::ManuallyDrop, sync::atomic::{AtomicU32, Ordering}};
use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, Result};
use windows::Win32::{Foundation::{E_FAIL, E_NOINTERFACE, E_POINTER, RPC_E_WRONG_THREAD, S_OK},
    System::Threading::GetCurrentThreadId, UI::TextServices::{ITfEditSession, ITfEditSession_Vtbl}};

type Action = Box<dyn FnOnce(u32) -> Result<()>>;
#[repr(C)]
struct Session {
    vtable: *const ITfEditSession_Vtbl,
    refs: AtomicU32,
    thread: u32,
    action: ManuallyDrop<RefCell<Option<Action>>>,
}
impl Drop for Session {
    fn drop(&mut self) {
        // Even a broken host bypassing non-agile marshaling must not drop captured apartment resources abroad.
        // The exceptional raw foreign Release intentionally retains its closure; normal release frees it.
        if unsafe { GetCurrentThreadId() } == self.thread { unsafe { ManuallyDrop::drop(&mut self.action); } }
    }
}
static VTABLE: ITfEditSession_Vtbl = ITfEditSession_Vtbl {
    base__: IUnknown_Vtbl { QueryInterface: query, AddRef: add_ref, Release: release },
    DoEditSession: run,
};
pub(super) fn new(action: Action) -> ITfEditSession {
    let object = Box::new(Session { vtable: &VTABLE, refs: AtomicU32::new(1),
        thread: unsafe { GetCurrentThreadId() }, action: ManuallyDrop::new(RefCell::new(Some(action))) });
    // repr(C), vtable first; the initial reference is transferred to this interface.
    unsafe { ITfEditSession::from_raw(Box::into_raw(object).cast()) }
}
unsafe extern "system" fn query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() { return E_POINTER; }
    unsafe { *out = std::ptr::null_mut(); }
    if iid.is_null() { return E_POINTER; }
    unsafe {
        if *iid != IUnknown::IID && *iid != ITfEditSession::IID { return E_NOINTERFACE; }
        add_ref(this); *out = this;
    }
    S_OK
}
unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    unsafe { (*(this as *mut Session)).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let remaining = unsafe { (*(this as *mut Session)).refs.fetch_sub(1, Ordering::AcqRel) - 1 };
    if remaining == 0 {
        // Standard apartment marshaling keeps the non-agile object's final Release on its apartment.
        // Contain even a captured value's destructor panic at the ABI boundary.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(this as *mut Session));
        }));
    }
    remaining
}
unsafe extern "system" fn run(this: *mut c_void, ec: u32) -> HRESULT {
    let object = unsafe { &*(this as *mut Session) };
    if unsafe { GetCurrentThreadId() } != object.thread { return RPC_E_WRONG_THREAD; }
    // A callback may release the host's reference reentrantly.
    unsafe { add_ref(this); }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let action = object.action.borrow_mut().take();
        match action { Some(f) => f(ec), None => Ok(()) }
    })).unwrap_or_else(|_| Err(E_FAIL.into()));
    unsafe { release(this); }
    result.map(|_| S_OK).unwrap_or_else(|e| e.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    #[test]
    fn edit_callback_is_not_agile_and_runs_only_once() {
        let count = Rc::new(Cell::new(0)); let seen = count.clone();
        let session = new(Box::new(move |ec| { seen.set(ec); Ok(()) }));
        for iid in [GUID::from_u128(0x94ea2b94_e9cc_49e0_c0ff_ee64ca8f5b90), // IAgileObject
            GUID::from_u128(0x00000003_0000_0000_c000_000000000046)] { // IMarshal
            let mut out = std::ptr::null_mut();
            unsafe { assert_eq!(session.query(&iid, &mut out), E_NOINTERFACE); }
            assert!(out.is_null());
        }
        let identity: IUnknown = session.cast().unwrap();
        assert_eq!(identity.cast::<ITfEditSession>().unwrap(), session);
        unsafe { session.DoEditSession(23).unwrap(); session.DoEditSession(99).unwrap(); }
        assert_eq!(count.get(), 23);
    }
    #[test]
    fn edit_callback_rejects_foreign_call_without_consuming_action_and_contains_panics() {
        let session = new(Box::new(|_| panic!("test ABI guard")));
        let raw = session.as_raw() as usize; // owner keeps the reference, foreign caller only probes the guard.
        let code = std::thread::spawn(move || unsafe {
            (VTABLE.DoEditSession)(raw as *mut _, 0)
        }).join().unwrap();
        assert_eq!(code, RPC_E_WRONG_THREAD);
        unsafe { assert_eq!(session.DoEditSession(0).unwrap_err().code(), E_FAIL); session.DoEditSession(0).unwrap(); }
    }
    #[test]
    fn edit_callback_raw_foreign_final_release_retains_unrun_apartment_closure() {
        struct Marker(std::sync::Arc<AtomicU32>);
        impl Drop for Marker { fn drop(&mut self) { self.0.fetch_add(1, Ordering::SeqCst); } }
        let drops = std::sync::Arc::new(AtomicU32::new(0));
        let marker = Marker(drops.clone());
        let session = new(Box::new(move |_| { drop(marker); Ok(()) }));
        let raw = Interface::into_raw(session) as usize;
        std::thread::spawn(move || unsafe { drop(ITfEditSession::from_raw(raw as *mut _)); }).join().unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let marker = Marker(drops.clone());
        drop(new(Box::new(move |_| { drop(marker); Ok(()) })));
        assert_eq!(drops.load(Ordering::SeqCst), 1, "normal uncalled owner release frees the action");
    }
}
