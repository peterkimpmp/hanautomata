//! Owner-thread deferred work; no activation, input hook, timer or cross-process messages.
//! HWND messages carry a nonce, never a Rust pointer. The hidden window is owned by Dispatcher.
use std::{cell::{Cell, RefCell}, collections::VecDeque, rc::Rc, sync::atomic::{AtomicUsize, Ordering}};
use windows::core::{w, PCWSTR, Result};
use windows::Win32::{Foundation::{E_FAIL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId}, UI::WindowsAndMessaging::*};

type Task = Box<dyn FnOnce()>;
const RUN: u32 = WM_APP + 0x37;
static SERIAL: AtomicUsize = AtomicUsize::new(1);
struct Queue {
    tasks: RefCell<VecDeque<Task>>, nonce: usize, held: Cell<u32>, window: Cell<HWND>,
    failed: RefCell<Option<Rc<dyn Fn()>>>,
}
pub(crate) struct Dispatcher { queue: Rc<Queue>, class: Vec<u16>, instance: HINSTANCE }
pub(crate) struct Hold(Rc<Queue>);
/// A numeric HWND/nonce only: no apartment interface or Rust pointer crosses threads.
pub(crate) struct Wake { window: isize, nonce: usize }
impl Wake {
    pub fn wake(self) {
        unsafe { let _ = PostMessageW(Some(HWND(self.window as *mut _)), RUN, WPARAM(self.nonce), LPARAM(0)); }
    }
}
impl Drop for Hold {
    fn drop(&mut self) {
        self.0.held.set(self.0.held.get() - 1);
        if self.0.post().is_err() { self.0.fail(); }
    }
}
impl Queue {
    fn fail(&self) {
        let tasks = std::mem::take(&mut *self.tasks.borrow_mut()); drop(tasks);
        let failed = self.failed.borrow().clone();
        if let Some(failed) = failed { failed(); }
    }
    fn post(&self) -> Result<()> {
        // A host may remove a message without dispatching it. A later enqueue must still be able to wake us.
        if self.held.get() == 0 && !self.tasks.borrow().is_empty() {
            if self.window.get().is_invalid() { return Err(E_FAIL.into()); }
            unsafe { PostMessageW(Some(self.window.get()), RUN, WPARAM(self.nonce), LPARAM(0))?; }
        }
        Ok(())
    }
    fn run(&self) {
        if self.held.get() != 0 { return; }
        let task = self.tasks.borrow_mut().pop_front();
        if let Some(task) = task { task(); }
        if self.post().is_err() { self.fail(); }
    }
}
impl Dispatcher {
    pub fn new() -> Result<Self> {
        let nonce = SERIAL.fetch_add(1, Ordering::Relaxed);
        let queue = Rc::new(Queue { tasks: RefCell::new(VecDeque::new()), nonce,
            held: Cell::new(0), window: Cell::new(HWND::default()), failed: RefCell::new(None) });
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            // Procedure address distinguishes concurrently loaded DLL revisions in the same host process.
            let class: Vec<u16> = format!("Hanautomata.Candidate.Dispatch.{}.{}.{}\0", procedure as *const () as usize,
                GetCurrentThreadId(), nonce).encode_utf16().collect();
            let definition = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: instance,
                lpszClassName: PCWSTR(class.as_ptr()), ..Default::default() };
            if RegisterClassW(&definition) == 0 { return Err(E_FAIL.into()); }
            let window = match CreateWindowExW(Default::default(), PCWSTR(class.as_ptr()), w!(""),
                Default::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(instance), Some(Rc::as_ptr(&queue).cast())) {
                Ok(window) => window,
                Err(e) => { let _ = UnregisterClassW(PCWSTR(class.as_ptr()), Some(instance)); return Err(e); }
            };
            queue.window.set(window);
            Ok(Self { queue, class, instance })
        }
    }
    pub fn enqueue(&self, task: impl FnOnce() + 'static) -> Result<()> {
        self.queue.tasks.borrow_mut().push_back(Box::new(task));
        self.wake()
    }
    pub fn defer(&self, task: impl FnOnce() + 'static) -> Wake {
        self.queue.tasks.borrow_mut().push_back(Box::new(task));
        Wake { window: self.queue.window.get().0 as isize, nonce: self.queue.nonce }
    }
    pub fn on_failure(&self, handler: impl Fn() + 'static) { *self.queue.failed.borrow_mut() = Some(Rc::new(handler)); }
    pub fn wake(&self) -> Result<()> {
        if let Err(e) = self.queue.post() { self.queue.fail(); return Err(e); }
        Ok(())
    }
    pub fn hold(&self) -> Hold {
        self.queue.held.set(self.queue.held.get() + 1); Hold(self.queue.clone())
    }
    pub fn cancel(&self) {
        let tasks = std::mem::take(&mut *self.queue.tasks.borrow_mut()); drop(tasks);
    }
    #[cfg(test)]
    pub fn drain_for_test(&self) {
        // Drain only this owned message-only HWND, never another application's input or queue.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(self.queue.window.get()), RUN, RUN, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
    }
    #[cfg(test)]
    pub fn discard_messages_for_test(&self) {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(self.queue.window.get()), RUN, RUN, PM_REMOVE).as_bool() {}
        }
    }
    #[cfg(test)]
    pub fn destroy_for_test(&self) { unsafe { DestroyWindow(self.queue.window.get()).unwrap(); } }
}
impl Drop for Dispatcher {
    fn drop(&mut self) {
        self.cancel();
        unsafe {
            let window = self.queue.window.get();
            if GetWindowThreadProcessId(window, None) == GetCurrentThreadId()
                && GetWindowLongPtrW(window, GWLP_USERDATA) as isize == Rc::as_ptr(&self.queue) as isize {
                let _ = DestroyWindow(window);
            }
            self.queue.window.set(HWND::default());
            let _ = UnregisterClassW(PCWSTR(self.class.as_ptr()), Some(self.instance));
        }
    }
}
unsafe extern "system" fn procedure(window: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        if message == WM_NCCREATE {
            let create = &*(lp.0 as *const CREATESTRUCTW);
            let raw = create.lpCreateParams as *const Queue;
            if raw.is_null() { return LRESULT(0); }
            // The window owns a reference independently of Dispatcher until WM_NCDESTROY.
            Rc::increment_strong_count(raw);
            (*raw).window.set(window);
            // windows-rs aliases LongPtr to Long on x86; preserve pointer width on both targets.
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize as _);
            return LRESULT(1);
        }
        if message == WM_NCDESTROY {
            let raw = GetWindowLongPtrW(window, GWLP_USERDATA) as *const Queue;
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            if !raw.is_null() {
                let queue = Rc::from_raw(raw);
                queue.window.set(HWND::default());
            }
        }
        if message == RUN {
            let raw = GetWindowLongPtrW(window, GWLP_USERDATA) as *const Queue;
            if !raw.is_null() {
                // The window owns the stored Rc. Pin it before work may destroy that very window.
                Rc::increment_strong_count(raw);
                let queue = Rc::from_raw(raw);
                if queue.nonce == wp.0 { queue.run(); }
            }
            return LRESULT(0);
        }
        DefWindowProcW(window, message, wp, lp)
    })).unwrap_or(LRESULT(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deferred_work_waits_for_cross_thread_wake_and_can_be_cancelled() {
        let d = Dispatcher::new().unwrap(); let count = Rc::new(Cell::new(0)); let seen = count.clone();
        let wake = d.defer(move || seen.set(1));
        d.drain_for_test(); assert_eq!(count.get(), 0);
        std::thread::spawn(move || wake.wake()).join().unwrap();
        d.drain_for_test(); assert_eq!(count.get(), 1);
        let seen = count.clone(); let wake = d.defer(move || seen.set(2));
        d.cancel(); std::thread::spawn(move || wake.wake()).join().unwrap();
        d.drain_for_test(); assert_eq!(count.get(), 1);
    }
    #[test]
    fn candidate_dispatch_defers_while_held_and_cancellation_drops_work() {
        let d = Dispatcher::new().unwrap();
        let count = Rc::new(Cell::new(0)); let seen = count.clone();
        let held = d.hold();
        d.enqueue(move || seen.set(1)).unwrap();
        d.drain_for_test(); assert_eq!(count.get(), 0);
        drop(held); d.drain_for_test(); assert_eq!(count.get(), 1);
        let seen = count.clone(); d.enqueue(move || seen.set(2)).unwrap();
        d.cancel(); d.drain_for_test(); assert_eq!(count.get(), 1);
    }
    #[test]
    fn candidate_dispatch_can_destroy_itself_in_a_callback_and_rejects_an_old_nonce() {
        let slot = Rc::new(RefCell::new(Some(Dispatcher::new().unwrap())));
        let owner = Rc::downgrade(&slot);
        let window = slot.borrow().as_ref().unwrap().queue.window.get();
        let nonce = slot.borrow().as_ref().unwrap().queue.nonce;
        slot.borrow().as_ref().unwrap().enqueue(move || {
            let slot = owner.upgrade().unwrap();
            let dispatcher = slot.borrow_mut().take(); drop(dispatcher);
        }).unwrap();
        unsafe {
            SendMessageW(window, RUN, Some(WPARAM(nonce.wrapping_add(1))), Some(LPARAM(0)));
            assert!(slot.borrow().is_some(), "another window's message cannot invoke this callback");
            let mut msg = MSG::default();
            assert!(PeekMessageW(&mut msg, Some(window), RUN, RUN, PM_REMOVE).as_bool());
            DispatchMessageW(&msg);
            assert!(slot.borrow().is_none());
            assert!(!IsWindow(Some(window)).as_bool());
        }
    }
}
