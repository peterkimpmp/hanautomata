//! A read-only candidate preview. Never edits a document, handles keys or persists text.
//! TSF gets first refusal of our window; a host can draw the two candidates itself.
//! Every retained COM element is emptied before EndUIElement (including failure/teardown).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use windows::core::{implement, ComObject, IUnknownImpl, Interface, Ref, Result, BSTR, GUID};
use windows::Win32::Foundation::{BOOL, E_FAIL, E_INVALIDARG, E_POINTER, HWND, RECT, RPC_E_WRONG_THREAD};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::TextServices::*;
use crate::{edit, shell::Preview};

#[path = "candidate_window.rs"]
mod window;
#[path = "candidate_dispatch.rs"]
pub(crate) mod dispatch;

const ID: GUID = GUID::from_u128(0x6d194788_a43b_4ee4_b64e_d1ff8e76e81c);
const ALL_CHANGED: u32 = TF_CLUIE_DOCUMENTMGR | TF_CLUIE_COUNT | TF_CLUIE_SELECTION
    | TF_CLUIE_STRING | TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE;

fn boundary<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_FAIL.into()))
}

pub(crate) struct Snapshot {
    preview: Preview,
    learning_notice: String,
    document: Option<ITfDocumentMgr>,
    context: Option<ITfContext>,
    geometry: Geometry,
    source: Option<Rc<dyn LayoutSource>>,
}

#[derive(Clone, Copy, Default)]
struct Geometry { owner: HWND, caret: Option<RECT> }
type LayoutReply = Box<dyn FnOnce(u32)>;
trait LayoutSource {
    fn matches(&self, context: Option<&ITfContext>) -> bool;
    fn sample(&self, ec: u32) -> Result<Geometry>;
    fn request(&self, reply: LayoutReply) -> Result<()>;
}
struct Anchor {
    context: ITfContext,
    manager: ITfThreadMgr,
    client: u32,
    selection: ITfRange,
    composition: ITfRange,
    text: String,
}
impl LayoutSource for Anchor {
    fn matches(&self, context: Option<&ITfContext>) -> bool { context == Some(&self.context) }
    fn sample(&self, ec: u32) -> Result<Geometry> {
        if edit::field_access(ec, &self.context) != edit::FieldAccess::Ordinary
            || !focused(&self.context, &self.manager)? { return Err(E_FAIL.into()); }
        unsafe {
            let selection = edit::selection(ec, &self.context)?;
            if !selection.IsEmpty(ec)?.as_bool()
                || selection.CompareStart(ec, &self.selection, TF_ANCHOR_START)? != 0
                || selection.CompareEnd(ec, &self.selection, TF_ANCHOR_END)? != 0
                || self.composition.GetContext()? != self.context
                || self.composition.CompareEnd(ec, &selection, TF_ANCHOR_START)? != 0
                || edit::selection_text(ec, &self.composition, 1024)?.as_deref() != Some(self.text.as_str()) {
                return Err(E_FAIL.into());
            }
            // Geometry is optional even in a windowless host. The validated text above remains usable by its UI.
            let Ok(view) = self.context.GetActiveView() else { return Ok(Geometry::default()); };
            let mut rect = RECT::default(); let mut clipped = BOOL(0);
            let extent = view.GetTextExt(ec, &selection, &mut rect, &mut clipped);
            let caret = layout_extent(extent, rect, clipped.as_bool())?;
            Ok(Geometry { owner: view.GetWnd().unwrap_or_default(), caret })
        }
    }
    fn request(&self, reply: LayoutReply) -> Result<()> {
        edit::request(&self.context, self.client, TF_ES_ASYNCDONTCARE | TF_ES_READ, move |ec| {
            // The caller checks liveness before reading. Queued sessions retain no text or anchor ranges.
            reply(ec); Ok(())
        })
    }
}
fn layout_extent(result: Result<()>, rect: RECT, clipped: bool) -> Result<Option<RECT>> {
    match result {
        Ok(()) => Ok(valid_caret(rect, clipped).then_some(rect)),
        Err(_) => Ok(None),
    }
}

pub(crate) fn focused(context: &ITfContext, tm: &ITfThreadMgr) -> Result<bool> {
    unsafe {
        let document = context.GetDocumentMgr()?;
        Ok(tm.GetFocus()? == document && document.GetTop()? == *context)
    }
}

/// Called only under the original document's edit lock, after successful composition writes.
/// No guessed position: a valid preview can await layout; stale/protected/unreadable text cannot.
pub(crate) fn capture(ec: u32, context: &ITfContext, tm: &ITfThreadMgr, client: u32,
    composition: &ITfComposition, preview: Preview, learning_notice: String) -> Result<Snapshot> {
    if learning_notice.encode_utf16().count() > 512 || learning_notice.contains('\0') { return Err(E_INVALIDARG.into()); }
    unsafe {
        let document = context.GetDocumentMgr()?;
        let source = Rc::new(Anchor { context: context.clone(), manager: tm.clone(), client,
            selection: edit::selection(ec, context)?.Clone()?, composition: composition.GetRange()?.Clone()?,
            text: preview.text.clone() });
        let geometry = source.sample(ec)?;
        Ok(Snapshot { preview, learning_notice, document: Some(document), context: Some(context.clone()), geometry, source: Some(source) })
    }
}

fn valid_caret(rect: RECT, clipped: bool) -> bool {
    !clipped && rect.right >= rect.left && rect.bottom > rect.top
        && i64::from(rect.right) - i64::from(rect.left) < 10000
        && i64::from(rect.bottom) - i64::from(rect.top) < 10000
}

#[implement(ITfCandidateListUIElement)]
struct Element {
    thread: u32,
    snapshot: RefCell<Option<Snapshot>>,
    pages: RefCell<Vec<u32>>,
    registered: Cell<bool>,
    wanted: Cell<bool>,
    popup: RefCell<Option<window::Popup>>,
    render: Cell<u64>,
}

impl Element {
    fn new(snapshot: Snapshot) -> ComObject<Self> {
        ComObject::new(Self { thread: unsafe { GetCurrentThreadId() }, snapshot: RefCell::new(Some(snapshot)), pages: RefCell::new(vec![0]),
            registered: Cell::new(false), wanted: Cell::new(true), popup: RefCell::new(None), render: Cell::new(0) })
    }
    fn on_thread<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        // windows-implement 0.59 advertises IAgileObject unconditionally. Reject foreign callbacks
        // BEFORE touching RefCell/COM/window state. The owning Active reference retires it on this thread.
        if unsafe { GetCurrentThreadId() } != self.thread { return Err(RPC_E_WRONG_THREAD.into()); }
        boundary(f)
    }
    fn clear(&self) {
        self.render.set(self.render.get().wrapping_add(1));
        self.registered.set(false);
        self.wanted.set(false);
        // Drop document/text references before any native or COM callback can reenter us.
        let snapshot = self.snapshot.borrow_mut().take();
        drop(snapshot);
        self.pages.borrow_mut().clear();
        let popup = self.popup.borrow_mut().take();
        drop(popup);
    }
    fn visible(&self, show: bool) -> Result<()> {
        let render = self.render.get().wrapping_add(1); self.render.set(render);
        self.wanted.set(show);
        if !show {
            let popup = self.popup.borrow_mut().take();
            drop(popup);
            return Ok(());
        }
        if !self.registered.get() { return Ok(()); }
        let info = self.snapshot.borrow().as_ref().and_then(|s| s.geometry.caret.map(|caret|
            (s.geometry.owner, caret, s.preview.clone(), s.learning_notice.clone())));
        let Some((owner, caret, preview, learning_notice)) = info else {
            let popup = self.popup.borrow_mut().take(); drop(popup); return Ok(());
        };
        if !window::foreground(owner) || !crate::system::desktop_settings_allowed() {
            let popup = self.popup.borrow_mut().take(); drop(popup);
            return Ok(());
        }
        let previous = self.popup.borrow_mut().take();
        let popup = match previous {
            Some(popup) if popup.reposition(owner, caret)? => popup,
            previous => { drop(previous); window::Popup::new(owner, caret, &preview, &learning_notice)? }
        };
        // Native window creation may reenter TSF. Never resurrect a retired preview.
        if self.render.get() != render || !self.registered.get() || !self.wanted.get() || self.snapshot.borrow().is_none() { return Ok(()); }
        popup.show()?;
        if self.render.get() != render || !self.registered.get() || !self.wanted.get() { return Ok(()); }
        let previous = self.popup.borrow_mut().replace(popup);
        drop(previous);
        Ok(())
    }
}

impl ITfUIElement_Impl for Element_Impl {
    fn GetDescription(&self) -> Result<BSTR> {
        self.on_thread(|| Ok(BSTR::from(self.snapshot.borrow().as_ref().map_or_else(
            || "한오토마타 후보".to_string(), |s| format!("한오토마타 후보 · {}", s.learning_notice)))))
    }
    fn GetGUID(&self) -> Result<GUID> { self.on_thread(|| Ok(ID)) }
    fn Show(&self, show: BOOL) -> Result<()> { self.on_thread(|| self.visible(show.as_bool())) }
    fn IsShown(&self) -> Result<BOOL> {
        self.on_thread(|| Ok(self.popup.borrow().as_ref().is_some_and(window::Popup::is_visible).into()))
    }
}
impl ITfCandidateListUIElement_Impl for Element_Impl {
    fn GetUpdatedFlags(&self) -> Result<u32> { self.on_thread(|| Ok(ALL_CHANGED)) }
    fn GetDocumentMgr(&self) -> Result<ITfDocumentMgr> {
        self.on_thread(|| self.snapshot.borrow().as_ref().and_then(|s| s.document.clone()).ok_or_else(|| E_FAIL.into()))
    }
    fn GetCount(&self) -> Result<u32> { self.on_thread(|| Ok(if self.snapshot.borrow().is_some() { 2 } else { 0 })) }
    fn GetSelection(&self) -> Result<u32> {
        self.on_thread(|| if self.snapshot.borrow().is_some() { Ok(0) } else { Err(E_FAIL.into()) })
    }
    fn GetString(&self, index: u32) -> Result<BSTR> {
        self.on_thread(|| {
            let data = self.snapshot.borrow();
            let s = data.as_ref().ok_or(E_INVALIDARG)?;
            match index { 0 => Ok(BSTR::from(&s.preview.text)), 1 => Ok(BSTR::from(&s.preview.alternative)),
                _ => Err(E_INVALIDARG.into()) }
        })
    }
    fn GetPageIndex(&self, index: *mut u32, size: u32, count: *mut u32) -> Result<()> {
        self.on_thread(|| {
            if count.is_null() || (size > 0 && index.is_null()) { return Err(E_POINTER.into()); }
            let pages = self.pages.borrow();
            unsafe {
                *count = pages.len() as u32;
                for (i, page) in pages.iter().take(size as usize).enumerate() { *index.add(i) = *page; }
            }
            if size < pages.len() as u32 { Err(windows::core::HRESULT(1).into()) } else { Ok(()) }
        })
    }
    fn SetPageIndex(&self, index: *const u32, count: u32) -> Result<()> {
        self.on_thread(|| {
            if index.is_null() { return Err(E_POINTER.into()); }
            // Exactly two candidates: either one page [0] or two pages [0,1].
            if self.snapshot.borrow().is_none() || !(1..=2).contains(&count) { return Err(E_INVALIDARG.into()); }
            let pages = unsafe { std::slice::from_raw_parts(index, count as usize) };
            if pages[0] != 0 || (count == 2 && pages[1] != 1) { return Err(E_INVALIDARG.into()); }
            *self.pages.borrow_mut() = pages.to_vec();
            Ok(())
        })
    }
    fn GetCurrentPage(&self) -> Result<u32> {
        self.on_thread(|| if self.snapshot.borrow().is_some() { Ok(0) } else { Err(E_FAIL.into()) })
    }
}

struct Watch {
    source: ITfSource,
    observer: ComObject<Observer>,
    edit: Option<u32>,
    layout: Option<u32>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        // A foreign host may retain the COM sink. Empty its non-Send Weak before releasing our reference.
        let owner = self.observer.presenter.borrow_mut().take(); drop(owner);
        unsafe {
            if let Some(cookie) = self.edit { let _ = self.source.UnadviseSink(cookie); }
            if let Some(cookie) = self.layout { let _ = self.source.UnadviseSink(cookie); }
        }
    }
}
struct Active {
    manager: ITfUIElementMgr,
    id: u32,
    element: ComObject<Element>,
    watch: Option<Watch>,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.element.clear();
        self.watch.take();
        unsafe { let _ = self.manager.EndUIElement(self.id); }
    }
}

struct ReadRetirement { owner: Weak<Presenter>, ticket: u64, completed: bool }
impl Drop for ReadRetirement {
    fn drop(&mut self) {
        if !self.completed {
            let _ = boundary(|| {
                if let Some(p) = self.owner.upgrade().filter(|p| p.ticket() == self.ticket) { p.clear(); }
                Ok(())
            });
        }
    }
}

#[derive(Default)]
pub(crate) struct Presenter {
    generation: Cell<u64>,
    active: RefCell<Option<Active>>,
    dispatch: RefCell<Option<dispatch::Dispatcher>>,
    revision: Cell<u64>,
    queued: Cell<bool>,
    reading: Cell<bool>,
}
impl Presenter {
    pub fn ticket(&self) -> u64 { self.generation.get() }
    pub fn active(&self) -> bool { self.active.borrow().is_some() }
    pub fn clear(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.queued.set(false); self.reading.set(false);
        if let Some(dispatch) = self.dispatch.borrow().as_ref() { dispatch.cancel(); }
        let active = self.active.borrow_mut().take();
        drop(active);
    }
    fn source(&self) -> Option<Rc<dyn LayoutSource>> {
        self.active.borrow().as_ref().and_then(|a| a.element.snapshot.borrow().as_ref().and_then(|s| s.source.clone()))
    }
    fn enqueue(&self, f: impl FnOnce() + 'static) -> Result<()> {
        self.dispatch.borrow().as_ref().ok_or(E_FAIL)?.enqueue(f)
    }
    fn hold(&self) -> Result<dispatch::Hold> { Ok(self.dispatch.borrow().as_ref().ok_or(E_FAIL)?.hold()) }
    fn queue_read(self: &Rc<Self>) -> Result<()> {
        if self.queued.get() { return self.dispatch.borrow().as_ref().ok_or(E_FAIL)?.wake(); }
        if self.reading.get() { return Ok(()); }
        self.queued.set(true);
        let owner = Rc::downgrade(self); let ticket = self.ticket();
        if let Err(e) = self.enqueue(move || {
            if let Some(p) = owner.upgrade().filter(|p| p.ticket() == ticket) {
                p.queued.set(false);
                if boundary(|| p.start_read()).is_err() { p.clear(); }
            }
        }) { self.clear(); return Err(e); }
        Ok(())
    }
    fn start_read(self: &Rc<Self>) -> Result<()> {
        if self.reading.get() { return Ok(()); }
        let Some(source) = self.source() else { self.clear(); return Ok(()) };
        let ticket = self.ticket(); let revision = self.revision.get();
        self.reading.set(true);
        let mut retirement = ReadRetirement { owner: Rc::downgrade(self), ticket, completed: false };
        let source_weak = Rc::downgrade(&source);
        // Keep deferred native work out of synchronous reentry while requesting/reading the document.
        let _request_hold = self.hold()?;
        let requested = source.request(Box::new(move |ec| {
            retirement.completed = true;
            if let Some(p) = retirement.owner.upgrade().filter(|p| p.ticket() == ticket) {
                let result = boundary(|| {
                    // Never hold the shared queue while an ASYNC session merely waits for a lock.
                    let _hold = p.hold()?;
                    let geometry = source_weak.upgrade().ok_or(E_FAIL).and_then(|s| s.sample(ec).map_err(|e| e.code()));
                    if p.ticket() != ticket { return Ok(()); }
                    p.reading.set(false);
                    if p.revision.get() != revision { p.queue_read() }
                    else { p.accept_geometry(geometry.map_err(Into::into), revision) }
                });
                if result.is_err() && p.ticket() == ticket { p.clear(); }
            }
        }));
        if requested.is_err() && self.ticket() == ticket { self.clear(); }
        requested
    }
    fn accept_geometry(self: &Rc<Self>, geometry: Result<Geometry>, revision: u64) -> Result<()> {
        let geometry = match geometry { Ok(g) => g, Err(_) => { self.clear(); return Ok(()); } };
        let element = self.active.borrow().as_ref().map(|a| a.element.clone());
        let Some(element) = element else { return Ok(()) };
        if let Some(snapshot) = element.snapshot.borrow_mut().as_mut() { snapshot.geometry = geometry; }
        let owner = Rc::downgrade(self); let ticket = self.ticket();
        self.enqueue(move || {
            if let Some(p) = owner.upgrade().filter(|p| p.ticket() == ticket && p.revision.get() == revision) {
                if boundary(|| element.visible(element.wanted.get())).is_err() { p.clear(); }
            }
        })
    }
    fn layout_changed(self: &Rc<Self>, context: Option<&ITfContext>, code: TfLayoutCode) -> Result<()> {
        if code == TF_LC_DESTROY || !self.source().is_some_and(|s| s.matches(context)) {
            self.clear(); return Ok(());
        }
        self.revision.set(self.revision.get().wrapping_add(1));
        self.queue_read()
    }
    fn edited(self: &Rc<Self>, context: Option<&ITfContext>, ec: u32) -> Result<()> {
        let Some(source) = self.source().filter(|s| s.matches(context)) else { self.clear(); return Ok(()) };
        let ticket = self.ticket();
        let revision = self.revision.get().wrapping_add(1); self.revision.set(revision);
        let _hold = self.hold()?;
        let geometry = source.sample(ec);
        let needs_layout = geometry.as_ref().is_ok_and(|g| g.caret.is_none());
        if self.ticket() != ticket || self.revision.get() != revision { return Ok(()); }
        if let Err(e) = self.accept_geometry(geometry, revision) { self.clear(); return Err(e); }
        if needs_layout { self.queue_read()?; }
        Ok(())
    }
    pub fn present(self: &Rc<Self>, manager: ITfUIElementMgr, snapshot: Snapshot, ticket: u64) -> Result<()> {
        if self.ticket() != ticket { return Ok(()); }
        if self.active.borrow().is_some() { return Err(E_FAIL.into()); }
        if self.dispatch.borrow().is_none() {
            let dispatch = dispatch::Dispatcher::new()?;
            if self.ticket() != ticket { return Ok(()); }
            let owner = Rc::downgrade(self);
            dispatch.on_failure(move || {
                let _ = boundary(|| { if let Some(p) = owner.upgrade() { p.clear(); } Ok(()) });
            });
            *self.dispatch.borrow_mut() = Some(dispatch);
        }
        let context = snapshot.context.clone();
        let element = Element::new(snapshot);
        let list: ITfCandidateListUIElement = element.to_interface();
        let ui: ITfUIElement = list.cast()?;
        let watch = if let Some(context) = context {
            let source: ITfSource = context.cast()?;
            let observer = Observer::new(self);
            let edit: ITfTextEditSink = observer.to_interface();
            let layout: ITfTextLayoutSink = observer.to_interface();
            let mut watch = Watch { source, observer, edit: None, layout: None };
            watch.edit = Some(unsafe { watch.source.AdviseSink(&ITfTextEditSink::IID, &edit)? });
            watch.layout = Some(unsafe { watch.source.AdviseSink(&ITfTextLayoutSink::IID, &layout)? });
            Some(watch)
        } else { None };
        if self.ticket() != ticket { element.clear(); return Ok(()); }
        let mut show = BOOL(1);
        let mut id = 0;
        if let Err(error) = unsafe { manager.BeginUIElement(&ui, &mut show, &mut id) } {
            element.clear();
            return Err(error);
        }
        let active = Active { manager: manager.clone(), id, element: element.clone(), watch };
        if self.ticket() != ticket { return Ok(()); } // Active drop also empties a host-retained element.
        *self.active.borrow_mut() = Some(active);
        element.registered.set(true);
        element.wanted.set(show.as_bool() && element.wanted.get());
        // First update is required when the host draws instead of us. Always send it so retained UI can refresh.
        if let Err(error) = unsafe { manager.UpdateUIElement(id) } { self.clear(); return Err(error); }
        if self.ticket() != ticket { return Ok(()); }
        if let Err(error) = element.visible(element.wanted.get()) { self.clear(); return Err(error); }
        // Layout may have become ready between capture and sink registration. Do not depend on another notice.
        if self.ticket() == ticket && self.source().is_some() { self.queue_read()?; }
        Ok(())
    }
}

#[implement(ITfTextEditSink, ITfTextLayoutSink)]
struct Observer { thread: u32, ticket: u64, presenter: RefCell<Option<Weak<Presenter>>> }
impl Observer {
    fn new(presenter: &Rc<Presenter>) -> ComObject<Self> {
        ComObject::new(Self { thread: unsafe { GetCurrentThreadId() }, ticket: presenter.ticket(),
            presenter: RefCell::new(Some(Rc::downgrade(presenter))) })
    }
}
impl Observer_Impl {
    fn notify(&self, callback: impl FnOnce(&Rc<Presenter>) -> Result<()>) -> Result<()> {
        if unsafe { GetCurrentThreadId() } != self.thread { return Err(RPC_E_WRONG_THREAD.into()); }
        // UnadviseSink can release the source's last reference to this very callback object.
        let _keep_alive = self.to_object();
        boundary(|| {
            let owner = self.presenter.borrow().as_ref().and_then(Weak::upgrade);
            // Context notices can arrive while BeginUIElement has not returned its id yet.
            // present() schedules a fresh read after registration, covering those early notices.
            if let Some(p) = owner.filter(|p| p.ticket() == self.ticket && p.active()) { callback(&p)?; }
            Ok(())
        })
    }
}
impl ITfTextEditSink_Impl for Observer_Impl {
    fn OnEndEdit(&self, context: Ref<ITfContext>, ec: u32, _: Ref<ITfEditRecord>) -> Result<()> {
        self.notify(|p| p.edited(context.as_ref(), ec))
    }
}
impl ITfTextLayoutSink_Impl for Observer_Impl {
    fn OnLayoutChange(&self, context: Ref<ITfContext>, code: TfLayoutCode, _: Ref<ITfContextView>) -> Result<()> {
        self.notify(|p| p.layout_changed(context.as_ref(), code))
    }
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
mod tests;
