//! In-process COM host, not a user application. Inject lock denial, delayed edit sessions and host edits into
//! the production service callbacks; never register a TIP, change a profile, focus a window or send input.
use super::*;
use std::rc::Rc;
use windows::core::{ComObject, HRESULT, IUnknown, PCWSTR, PWSTR};
use windows::Win32::Foundation::{E_NOTIMPL, S_OK};
use windows::Win32::System::Com::IDataObject;
use crate::shell::Mods;

#[derive(Default)]
struct Host {
    text: RefCell<String>,
    deny: Cell<bool>,
    defer: Cell<bool>,
    ended: Cell<u32>,
    writes: Cell<u32>,
    fail_set: Cell<bool>,
    fail_end: Cell<bool>,
    fail_caret: Cell<bool>,
    pending: RefCell<Vec<ITfEditSession>>,
}

#[implement(ITfContext)]
struct Context(Rc<Host>);
impl ITfContext_Impl for Context_Impl {
    fn RequestEditSession(&self, _: u32, session: Ref<ITfEditSession>, flags: TF_CONTEXT_EDIT_CONTEXT_FLAGS) -> Result<HRESULT> {
        if self.0.deny.get() { return Ok(TF_E_SYNCHRONOUS); }
        if self.0.defer.get() && flags.0 & TF_ES_SYNC.0 == 0 {
            self.0.pending.borrow_mut().push(session.ok()?.clone());
            return Ok(TF_S_ASYNC);
        }
        unsafe { session.ok()?.DoEditSession(1)?; }
        Ok(S_OK)
    }
    fn InWriteSession(&self, _: u32) -> Result<BOOL> { Ok(true.into()) }
    fn GetSelection(&self, _: u32, _: u32, _: u32, _: *mut TF_SELECTION, _: *mut u32) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn SetSelection(&self, _: u32, _: u32, _: *const TF_SELECTION) -> Result<()> {
        if self.0.fail_caret.replace(false) { return Err(E_FAIL.into()); }
        Ok(())
    }
    fn GetStart(&self, _: u32) -> Result<ITfRange> { Err(E_NOTIMPL.into()) }
    fn GetEnd(&self, _: u32) -> Result<ITfRange> { Err(E_NOTIMPL.into()) }
    fn GetActiveView(&self) -> Result<ITfContextView> { Err(E_NOTIMPL.into()) }
    fn EnumViews(&self) -> Result<IEnumTfContextViews> { Err(E_NOTIMPL.into()) }
    fn GetStatus(&self) -> Result<TS_STATUS> { Ok(TS_STATUS::default()) }
    fn GetProperty(&self, _: *const GUID) -> Result<ITfProperty> { Err(E_NOTIMPL.into()) }
    fn GetAppProperty(&self, _: *const GUID) -> Result<ITfReadOnlyProperty> { Err(E_NOTIMPL.into()) }
    fn TrackProperties(&self, _: *const *const GUID, _: u32, _: *const *const GUID, _: u32) -> Result<ITfReadOnlyProperty> { Err(E_NOTIMPL.into()) }
    fn EnumProperties(&self) -> Result<IEnumTfProperties> { Err(E_NOTIMPL.into()) }
    fn GetDocumentMgr(&self) -> Result<ITfDocumentMgr> { Err(E_NOTIMPL.into()) }
    fn CreateRangeBackup(&self, _: u32, _: Ref<ITfRange>) -> Result<ITfRangeBackup> { Err(E_NOTIMPL.into()) }
}

#[implement(ITfRange)]
struct Range { host: Rc<Host>, context: ITfContext, offset: Cell<usize> }
impl ITfRange_Impl for Range_Impl {
    fn GetText(&self, _: u32, flags: u32, buffer: PWSTR, max: u32, count: *mut u32) -> Result<()> {
        let units: Vec<u16> = self.host.text.borrow().encode_utf16().collect();
        let start = self.offset.get().min(units.len());
        let n = (units.len() - start).min(max as usize);
        unsafe { std::ptr::copy_nonoverlapping(units[start..].as_ptr(), buffer.0, n); *count = n as u32; }
        if flags & TF_TF_MOVESTART != 0 { self.offset.set(start + n); }
        Ok(())
    }
    fn SetText(&self, _: u32, _: u32, value: &PCWSTR, n: i32) -> Result<()> {
        if self.host.fail_set.replace(false) { return Err(E_FAIL.into()); }
        assert!(n >= 0);
        *self.host.text.borrow_mut() = if n == 0 { String::new() }
            else { String::from_utf16(unsafe { std::slice::from_raw_parts(value.0, n as usize) }).unwrap() };
        self.host.writes.set(self.host.writes.get() + 1);
        Ok(())
    }
    fn GetFormattedText(&self, _: u32) -> Result<IDataObject> { Err(E_NOTIMPL.into()) }
    fn GetEmbedded(&self, _: u32, _: *const GUID, _: *const GUID) -> Result<IUnknown> { Err(E_NOTIMPL.into()) }
    fn InsertEmbedded(&self, _: u32, _: u32, _: Ref<IDataObject>) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftStart(&self, _: u32, _: i32, _: *mut i32, _: *const TF_HALTCOND) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftEnd(&self, _: u32, _: i32, _: *mut i32, _: *const TF_HALTCOND) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftStartToRange(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftEndToRange(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftStartRegion(&self, _: u32, _: TfShiftDir) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn ShiftEndRegion(&self, _: u32, _: TfShiftDir) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn IsEmpty(&self, _: u32) -> Result<BOOL> { Ok((self.offset.get() >= self.host.text.borrow().encode_utf16().count()).into()) }
    fn Collapse(&self, _: u32, _: TfAnchor) -> Result<()> { Ok(()) }
    fn IsEqualStart(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn IsEqualEnd(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn CompareStart(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<i32> { Err(E_NOTIMPL.into()) }
    fn CompareEnd(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<i32> { Err(E_NOTIMPL.into()) }
    fn AdjustForInsert(&self, _: u32, _: u32) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn GetGravity(&self, _: *mut TfGravity, _: *mut TfGravity) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn SetGravity(&self, _: u32, _: TfGravity, _: TfGravity) -> Result<()> { Ok(()) }
    fn Clone(&self) -> Result<ITfRange> { Ok(Range { host: self.host.clone(), context: self.context.clone(), offset: Cell::new(self.offset.get()) }.into()) }
    fn GetContext(&self) -> Result<ITfContext> { Ok(self.context.clone()) }
}

#[implement(ITfComposition)]
struct Composition { host: Rc<Host>, range: ITfRange }
impl ITfComposition_Impl for Composition_Impl {
    fn GetRange(&self) -> Result<ITfRange> { Ok(self.range.clone()) }
    fn ShiftStart(&self, _: u32, _: Ref<ITfRange>) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftEnd(&self, _: u32, _: Ref<ITfRange>) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn EndComposition(&self, _: u32) -> Result<()> {
        if self.host.fail_end.replace(false) { return Err(E_FAIL.into()); }
        self.host.ended.set(self.host.ended.get() + 1); Ok(())
    }
}

fn fixture() -> (ComObject<TextService>, Rc<Host>, ITfContext) {
    let host = Rc::new(Host::default());
    let context: ITfContext = Context(host.clone()).into();
    let service = ComObject::new(TextService::new());
    service.state.borrow_mut().shell = Some(Shell::new(engine(String::new(), std::env::temp_dir().join("hanautomata-com-fault-unused.json"))));
    (service, host, context)
}

fn seed(service: &ComObject<TextService>, host: &Rc<Host>, context: &ITfContext) -> String {
    let range: ITfRange = Range { host: host.clone(), context: context.clone(), offset: Cell::new(0) }.into();
    service.state.borrow_mut().composition = Some(Composition { host: host.clone(), range }.into());
    let mut preview = String::new();
    for key in b"DKSSUD" {
        let out = service.state.borrow_mut().shell.as_mut().unwrap().key_down(KeyEvent::new(*key as u16, Mods::NONE));
        for op in out.ops { if let Op::Compose(text) = op { preview = text; } }
    }
    service.compose(1, context, &preview).unwrap();
    preview
}

#[test]
fn denied_write_lock_does_not_advance_the_engine() {
    let (service, host, context) = fixture();
    host.deny.set(true);
    service.state.borrow_mut().checked_key = Some(CheckedKey { context: context.clone(), wparam: b'D' as usize,
        lparam: 0, down: true, observed: system::now_ms(), access: edit::FieldAccess::Ordinary, tray: false });
    let sink = service.to_interface::<ITfKeyEventSink>();
    let _ = unsafe { sink.OnKeyDown(&context, WPARAM(b'D' as usize), LPARAM(0)) };
    assert!(!service.state.borrow().shell.as_ref().unwrap().is_composing(), "denied lock must not leave an unseen first character in the engine");
    assert_eq!(&*host.text.borrow(), "");
}

#[test]
fn delayed_focus_commit_preserves_text_changed_by_the_host() {
    let (service, host, context) = fixture();
    let preview = seed(&service, &host, &context);
    host.defer.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(host.pending.borrow().len(), 1);
    *host.text.borrow_mut() = format!("{preview}x");
    let pending = std::mem::take(&mut *host.pending.borrow_mut());
    for session in pending { unsafe { session.DoEditSession(1).unwrap(); } }
    assert_eq!(&*host.text.borrow(), &format!("{preview}x"), "an asynchronous commit must not overwrite a raw key or a host edit");
    assert_eq!(host.ended.get(), 1);
}

#[test]
fn refused_focus_commit_can_be_retried_without_losing_the_word() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.deny.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(host.ended.get(), 0);
    host.deny.set(false);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(&*host.text.borrow(), "안녕");
    assert_eq!(host.ended.get(), 1, "a denied finalization must stay owned until retried");
}

fn paired_key(service: &ComObject<TextService>, context: &ITfContext, key: u8, access: edit::FieldAccess) -> BOOL {
    service.state.borrow_mut().checked_key = Some(CheckedKey { context: context.clone(), wparam: key as usize,
        lparam: 0, down: true, observed: system::now_ms(), access, tray: false });
    unsafe { service.to_interface::<ITfKeyEventSink>().OnKeyDown(context, WPARAM(key as usize), LPARAM(0)).unwrap() }
}

#[test]
fn unknown_probe_only_claims_cleanup_and_never_accepts_the_secret_key() {
    let (service, host, context) = fixture();
    let preview = seed(&service, &host, &context);
    let sink = service.to_interface::<ITfKeyEventSink>();
    // This mock context deliberately has no input-scope interface: the production probe returns Unknown.
    assert!(unsafe { sink.OnTestKeyDown(&context, WPARAM(b'X' as usize), LPARAM(0)).unwrap().as_bool() });
    assert_eq!(host.ended.get(), 0, "a probe must not end the composition");
    assert_eq!(&*host.text.borrow(), &preview);
    let eaten = paired_key(&service, &context, b'X', edit::FieldAccess::Unknown(E_FAIL));
    assert!(!eaten.as_bool());
    assert_eq!(host.ended.get(), 1, "end the old range before the host receives X");
    assert!(!service.state.borrow().shell.as_ref().unwrap().is_composing());
    assert_eq!(&*host.text.borrow(), "안녕", "X was not given to the engine");
}

#[test]
fn synchronous_cleanup_preempts_queued_async_work_exactly_once() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.defer.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(host.pending.borrow().len(), 1);
    assert!(!paired_key(&service, &context, b'X', edit::FieldAccess::Unknown(E_FAIL)).as_bool());
    assert_eq!(host.ended.get(), 1);
    host.text.borrow_mut().push('x');
    for session in std::mem::take(&mut *host.pending.borrow_mut()) { unsafe { session.DoEditSession(1).unwrap(); } }
    assert_eq!(&*host.text.borrow(), "안녕x");
    assert_eq!(host.ended.get(), 1, "the obsolete callback must do nothing");
}

#[test]
fn rejected_final_text_write_is_retained_without_a_refcell_panic() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.fail_set.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(host.ended.get(), 0);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(host.ended.get(), 1);
    assert_eq!(&*host.text.borrow(), "안녕");
}

#[test]
fn failed_composition_write_does_not_replay_the_unwritten_key() {
    let (service, host, context) = fixture();
    let preview = seed(&service, &host, &context);
    host.fail_set.set(true);
    assert!(!paired_key(&service, &context, b'X', edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), &preview, "X must pass once, never reappear in a fallback commit");
    host.text.borrow_mut().push('x');
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(&*host.text.borrow(), &format!("{preview}x"));
}

#[test]
fn successful_text_write_followed_by_caret_failure_does_not_duplicate_space() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.fail_caret.set(true);
    let eaten = paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary);
    assert!(eaten.as_bool(), "Space is already in the document; passing it duplicates it");
    assert_eq!(&*host.text.borrow(), "안녕 ");
    assert_eq!(host.ended.get(), 1, "the old range must still be ended after caret failure");
}

#[test]
fn failed_end_does_not_repeat_an_already_written_commit() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.fail_end.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    let writes = host.writes.get();
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(host.writes.get(), writes);
    assert_eq!(host.ended.get(), 1);
}

#[test]
fn host_termination_preserves_its_intervening_edit() {
    let (service, host, context) = fixture();
    let preview = seed(&service, &host, &context);
    let composition = service.state.borrow().composition.clone().unwrap();
    host.text.borrow_mut().push('x');
    unsafe { service.to_interface::<ITfCompositionSink>().OnCompositionTerminated(1, &composition).unwrap(); }
    assert_eq!(&*host.text.borrow(), &format!("{preview}x"));
    assert!(!service.state.borrow().shell.as_ref().unwrap().is_composing());
}

#[test]
fn panic_inside_an_edit_session_returns_failure_across_com() {
    let (_, _, context) = fixture();
    let error = edit::request(&context, 1, TF_ES_SYNC | TF_ES_READWRITE, |_| panic!("test-only edit fault")).unwrap_err();
    assert_eq!(error.code(), E_FAIL);
}

#[test]
fn private_transition_finishes_old_word_inside_the_current_lock() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.defer.set(true);
    assert!(!paired_key(&service, &context, b' ', edit::FieldAccess::Private).as_bool());
    assert_eq!(host.ended.get(), 1);
    assert!(host.pending.borrow().is_empty(), "a same-context boundary must not enqueue an out-of-order edit");
    assert_eq!(&*host.text.borrow(), "안녕");
    assert!(service.state.borrow().shell.as_ref().unwrap().engine().learning_paused());
}
