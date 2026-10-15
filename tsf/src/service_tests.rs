//! In-process COM host, not a user application. Inject lock denial, delayed edit sessions and host edits into
//! the production service callbacks; never register a TIP, change a profile, focus a window or send input.
use super::*;
use std::rc::Rc;
use windows::core::{ComObject, HRESULT, IUnknown, OutRef, PCWSTR, PWSTR};
use windows::Win32::Foundation::{E_NOTIMPL, S_OK};
use windows::Win32::System::Com::{IDataObject, IEnumGUID};
use windows::Win32::System::Variant::VARIANT;
use crate::shell::Mods;

#[derive(Default)]
struct Host {
    text: RefCell<String>,
    deny: Cell<bool>,
    defer: Cell<bool>,
    ended: Cell<u32>,
    writes: Cell<u32>,
    caret_updates: Cell<u32>,
    fail_set: Cell<bool>,
    fail_end: Cell<bool>,
    fail_caret: Cell<bool>,
    scope_valid: Cell<bool>,
    app_property_result: Cell<Option<HRESULT>>,
    status_failure: Cell<bool>,
    read_only: Cell<bool>,
    selection_failure: Cell<bool>,
    scope_value_failure: Cell<bool>,
    scope_value_error: Cell<Option<HRESULT>>,
    keyboard_disabled: Cell<bool>,
    empty_context: Cell<bool>,
    language_property: Cell<bool>,
    language: Cell<Option<i32>>,
    language_at_write: Cell<Option<i32>>,
    last_write: RefCell<String>,
    autocorrect_space: Cell<bool>,
    refuse_end_shift: Cell<bool>,
    deny_read: Cell<bool>,
    deny_write: Cell<bool>,
    fail_text_read: Cell<bool>,
    fail_read_after_caret_query: Cell<bool>,
    write_requests: Cell<u32>,
    caret_selection: Cell<bool>,
    interim_selection: Cell<bool>,
    caret_at_start: Cell<bool>,
    normalized_selection: Cell<bool>,
    pending: RefCell<Vec<ITfEditSession>>,
    before_set: RefCell<Option<Box<dyn FnOnce()>>>,
    before_selection: RefCell<Option<Box<dyn FnOnce()>>>,
    before_write: RefCell<Option<Box<dyn FnOnce()>>>,
}

#[implement(ITfContext, ITfCompartmentMgr, ITfContextComposition, ITfInsertAtSelection)]
struct Context(Rc<Host>);
impl ITfContextComposition_Impl for Context_Impl {
    fn StartComposition(&self, _: u32, range: Ref<ITfRange>, _: Ref<ITfCompositionSink>) -> Result<ITfComposition> {
        Ok(Composition { host: self.0.clone(), range: range.ok()?.clone() }.into())
    }
    fn EnumCompositions(&self) -> Result<IEnumITfCompositionView> { Err(E_NOTIMPL.into()) }
    fn FindComposition(&self, _: u32, _: Ref<ITfRange>) -> Result<IEnumITfCompositionView> { Err(E_NOTIMPL.into()) }
    fn TakeOwnership(&self, _: u32, _: Ref<ITfCompositionView>, _: Ref<ITfCompositionSink>) -> Result<ITfComposition> { Err(E_NOTIMPL.into()) }
}
impl ITfInsertAtSelection_Impl for Context_Impl {
    fn InsertTextAtSelection(&self, ec: u32, flags: INSERT_TEXT_AT_SELECTION_FLAGS, text: &PCWSTR, count: i32) -> Result<ITfRange> {
        let range: ITfRange = Range { host: self.0.clone(), context: self.to_interface(),
            offset: Cell::new(self.0.text.borrow().encode_utf16().count()), tail: Cell::new(0) }.into();
        if flags != TF_IAS_QUERYONLY {
            if count < 0 { return Err(E_INVALIDARG.into()); }
            let text = if count == 0 { &[] } else { unsafe { std::slice::from_raw_parts(text.0, count as usize) } };
            unsafe { range.SetText(ec, 0, text)?; }
        }
        Ok(range)
    }
    fn InsertEmbeddedAtSelection(&self, _: u32, _: u32, _: Ref<IDataObject>) -> Result<ITfRange> { Err(E_NOTIMPL.into()) }
}
impl ITfContext_Impl for Context_Impl {
    fn RequestEditSession(&self, _: u32, session: Ref<ITfEditSession>, flags: TF_CONTEXT_EDIT_CONTEXT_FLAGS) -> Result<HRESULT> {
        if flags.0 & TF_ES_READWRITE.0 == TF_ES_READWRITE.0 {
            self.0.write_requests.set(self.0.write_requests.get() + 1);
            if self.0.deny_write.get() { return Ok(TF_E_SYNCHRONOUS); }
            if let Some(callback) = self.0.before_write.borrow_mut().take() { callback(); }
        }
        else if self.0.deny_read.get() { return Ok(TF_E_SYNCHRONOUS); }
        if self.0.deny.get() { return Ok(TF_E_SYNCHRONOUS); }
        if self.0.defer.get() && flags.0 & TF_ES_SYNC.0 == 0 {
            self.0.pending.borrow_mut().push(session.ok()?.clone());
            return Ok(TF_S_ASYNC);
        }
        unsafe { session.ok()?.DoEditSession(1)?; }
        Ok(S_OK)
    }
    fn InWriteSession(&self, _: u32) -> Result<BOOL> { Ok(true.into()) }
    fn GetSelection(&self, _: u32, _: u32, count: u32, selection: *mut TF_SELECTION, fetched: *mut u32) -> Result<()> {
        if let Some(callback) = self.0.before_selection.borrow_mut().take() { callback(); }
        if self.0.selection_failure.get() { return Err(E_FAIL.into()); }
        if count == 0 { return Err(E_INVALIDARG.into()); }
        let offset = if self.0.caret_selection.get() { self.0.text.borrow().encode_utf16().count() } else { 0 };
        let (offset, tail) = if self.0.caret_at_start.get() { (0, self.0.text.borrow().encode_utf16().count()) } else { (offset, 0) };
        let range: ITfRange = Range { host: self.0.clone(), context: self.to_interface(), offset: Cell::new(offset), tail: Cell::new(tail) }.into();
        unsafe { *selection = TF_SELECTION { range: std::mem::ManuallyDrop::new(Some(range)), style: TF_SELECTIONSTYLE {
            fInterimChar: self.0.interim_selection.get().into(), ..TF_SELECTIONSTYLE::default() } }; *fetched = 1; }
        Ok(())
    }
    fn SetSelection(&self, _: u32, count: u32, selection: *const TF_SELECTION) -> Result<()> {
        self.0.caret_updates.set(self.0.caret_updates.get() + 1);
        if self.0.fail_caret.replace(false) { return Err(E_FAIL.into()); }
        if count == 1 {
            let selection = unsafe { &*selection };
            let range = selection.range.as_ref().unwrap().cast_object_ref::<Range>()?;
            self.0.normalized_selection.set(range.offset.get() == self.0.text.borrow().encode_utf16().count()
                && range.tail.get() == 0 && !selection.style.fInterimChar.as_bool());
        }
        Ok(())
    }
    fn GetStart(&self, _: u32) -> Result<ITfRange> { Err(E_NOTIMPL.into()) }
    fn GetEnd(&self, _: u32) -> Result<ITfRange> { Err(E_NOTIMPL.into()) }
    fn GetActiveView(&self) -> Result<ITfContextView> { Err(E_NOTIMPL.into()) }
    fn EnumViews(&self) -> Result<IEnumTfContextViews> { Err(E_NOTIMPL.into()) }
    fn GetStatus(&self) -> Result<TS_STATUS> {
        if self.0.status_failure.get() { return Err(E_FAIL.into()); }
        Ok(TS_STATUS { dwDynamicFlags: if self.0.read_only.get() { TF_SD_READONLY } else { 0 }, ..TS_STATUS::default() })
    }
    fn GetProperty(&self, guid: *const GUID) -> Result<ITfProperty> {
        if unsafe { *guid == GUID_PROP_LANGID } && self.0.language_property.get() { Ok(LanguageProperty(self.0.clone()).into()) }
        else { Err(E_NOTIMPL.into()) }
    }
    fn GetAppProperty(&self, _: *const GUID) -> Result<ITfReadOnlyProperty> {
        // The generated COM adapter returns this HRESULT without writing the caller's null output.
        // S_FALSE is the documented "owner does not support the property" result.
        if let Some(code) = self.0.app_property_result.get() {
            return Err(windows::core::Error::from_hresult(code));
        }
        Ok(ScopeProperty(self.0.clone()).into())
    }
    fn TrackProperties(&self, _: *const *const GUID, _: u32, _: *const *const GUID, _: u32) -> Result<ITfReadOnlyProperty> { Err(E_NOTIMPL.into()) }
    fn EnumProperties(&self) -> Result<IEnumTfProperties> { Err(E_NOTIMPL.into()) }
    fn GetDocumentMgr(&self) -> Result<ITfDocumentMgr> { Err(E_NOTIMPL.into()) }
    fn CreateRangeBackup(&self, _: u32, _: Ref<ITfRange>) -> Result<ITfRangeBackup> { Err(E_NOTIMPL.into()) }
}

impl ITfCompartmentMgr_Impl for Context_Impl {
    fn GetCompartment(&self, guid: *const GUID) -> Result<ITfCompartment> {
        let disabled = unsafe { if *guid == GUID_COMPARTMENT_KEYBOARD_DISABLED {
            self.0.keyboard_disabled.get()
        } else if *guid == GUID_COMPARTMENT_EMPTYCONTEXT { self.0.empty_context.get() } else { false } };
        if self.0.scope_valid.get() { Ok(Compartment(disabled).into()) } else { Err(E_FAIL.into()) }
    }
    fn ClearCompartment(&self, _: u32, _: *const GUID) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn EnumCompartments(&self) -> Result<IEnumGUID> { Err(E_NOTIMPL.into()) }
}
#[implement(ITfCompartment)]
struct Compartment(bool);
impl ITfCompartment_Impl for Compartment_Impl {
    fn SetValue(&self, _: u32, _: *const VARIANT) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn GetValue(&self) -> Result<VARIANT> { Ok(edit::variant_i4(self.0 as i32)) }
}
#[implement(ITfReadOnlyProperty)]
struct ScopeProperty(Rc<Host>);
impl ITfReadOnlyProperty_Impl for ScopeProperty_Impl {
    fn GetType(&self) -> Result<GUID> { Ok(GUID_PROP_INPUTSCOPE) }
    fn EnumRanges(&self, _: u32, _: OutRef<IEnumTfRanges>, _: Ref<ITfRange>) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn GetValue(&self, _: u32, _: Ref<ITfRange>) -> Result<VARIANT> {
        if self.0.scope_value_failure.get() { return Err(E_FAIL.into()); }
        if let Some(code) = self.0.scope_value_error.get() { return Err(code.into()); }
        Ok(VARIANT::default())
    }
    fn GetContext(&self) -> Result<ITfContext> { Err(E_NOTIMPL.into()) }
}

#[implement(ITfProperty)]
struct LanguageProperty(Rc<Host>);
impl ITfReadOnlyProperty_Impl for LanguageProperty_Impl {
    fn GetType(&self) -> Result<GUID> { Ok(GUID_PROP_LANGID) }
    fn EnumRanges(&self, _: u32, _: OutRef<IEnumTfRanges>, _: Ref<ITfRange>) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn GetValue(&self, _: u32, _: Ref<ITfRange>) -> Result<VARIANT> {
        Ok(self.0.language.get().map(edit::variant_i4).unwrap_or_default())
    }
    fn GetContext(&self) -> Result<ITfContext> { Err(E_NOTIMPL.into()) }
}
impl ITfProperty_Impl for LanguageProperty_Impl {
    fn FindRange(&self, _: u32, _: Ref<ITfRange>, _: OutRef<ITfRange>, _: TfAnchor) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn SetValueStore(&self, _: u32, _: Ref<ITfRange>, _: Ref<ITfPropertyStore>) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn SetValue(&self, _: u32, _: Ref<ITfRange>, value: *const VARIANT) -> Result<()> {
        self.0.language.set(edit::variant_as_i32(unsafe { &*value })); Ok(())
    }
    fn Clear(&self, _: u32, _: Ref<ITfRange>) -> Result<()> { self.0.language.set(None); Ok(()) }
}

#[implement(ITfRange)]
struct Range { host: Rc<Host>, context: ITfContext, offset: Cell<usize>, tail: Cell<usize> }
impl ITfRange_Impl for Range_Impl {
    fn GetText(&self, _: u32, flags: u32, buffer: PWSTR, max: u32, count: *mut u32) -> Result<()> {
        if self.host.fail_text_read.replace(false) { return Err(E_FAIL.into()); }
        let units: Vec<u16> = self.host.text.borrow().encode_utf16().collect();
        let start = self.offset.get().min(units.len());
        let n = (units.len().saturating_sub(self.tail.get()).saturating_sub(start)).min(max as usize);
        unsafe { std::ptr::copy_nonoverlapping(units[start..].as_ptr(), buffer.0, n); *count = n as u32; }
        if flags & TF_TF_MOVESTART != 0 { self.offset.set(start + n); }
        Ok(())
    }
    fn SetText(&self, _: u32, _: u32, value: &PCWSTR, n: i32) -> Result<()> {
        if let Some(callback) = self.host.before_set.borrow_mut().take() { callback(); }
        self.host.language_at_write.set(self.host.language.get());
        if self.host.fail_set.replace(false) { return Err(E_FAIL.into()); }
        assert!(n >= 0);
        let mut units: Vec<u16> = self.host.text.borrow().encode_utf16().collect();
        let suffix = units[units.len().saturating_sub(self.tail.get())..].to_vec();
        units.truncate(self.offset.get());
        let text = if n > 0 { String::from_utf16(unsafe { std::slice::from_raw_parts(value.0, n as usize) }).unwrap() } else { String::new() };
        *self.host.last_write.borrow_mut() = text.clone();
        let inserted = if self.host.autocorrect_space.get() && text == "dkssud " { "안녕 " }
            else if self.host.autocorrect_space.get() && text == "dkssud\t" { "안녕\t" } else { &text };
        units.extend(inserted.encode_utf16());
        units.extend(suffix);
        *self.host.text.borrow_mut() = String::from_utf16(&units).unwrap();
        self.host.writes.set(self.host.writes.get() + 1);
        Ok(())
    }
    fn GetFormattedText(&self, _: u32) -> Result<IDataObject> { Err(E_NOTIMPL.into()) }
    fn GetEmbedded(&self, _: u32, _: *const GUID, _: *const GUID) -> Result<IUnknown> { Err(E_NOTIMPL.into()) }
    fn InsertEmbedded(&self, _: u32, _: u32, _: Ref<IDataObject>) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftStart(&self, _: u32, shift: i32, moved: *mut i32, _: *const TF_HALTCOND) -> Result<()> {
        let before = self.offset.get() as i32;
        let end = self.host.text.borrow().encode_utf16().count() as i32;
        let after = (before + shift).clamp(0, end);
        self.offset.set(after as usize);
        unsafe { *moved = after - before; }
        Ok(())
    }
    fn ShiftEnd(&self, _: u32, shift: i32, moved: *mut i32, _: *const TF_HALTCOND) -> Result<()> {
        if self.host.refuse_end_shift.get() { return Err(E_FAIL.into()); }
        let len = self.host.text.borrow().encode_utf16().count() as i32;
        let before = len - self.tail.get() as i32;
        let after = (before + shift).clamp(self.offset.get() as i32, len);
        self.tail.set((len - after) as usize);
        unsafe { *moved = after - before; }
        Ok(())
    }
    fn ShiftStartToRange(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftEndToRange(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShiftStartRegion(&self, _: u32, _: TfShiftDir) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn ShiftEndRegion(&self, _: u32, _: TfShiftDir) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn IsEmpty(&self, _: u32) -> Result<BOOL> { Ok((self.offset.get() >= self.host.text.borrow().encode_utf16().count().saturating_sub(self.tail.get())).into()) }
    fn Collapse(&self, _: u32, anchor: TfAnchor) -> Result<()> {
        if anchor == TF_ANCHOR_END { self.offset.set(self.host.text.borrow().encode_utf16().count().saturating_sub(self.tail.get())); }
        Ok(())
    }
    fn IsEqualStart(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn IsEqualEnd(&self, _: u32, other: Ref<ITfRange>, anchor: TfAnchor) -> Result<BOOL> {
        if self.host.fail_read_after_caret_query.replace(false) { self.host.fail_text_read.set(true); }
        let other = other.ok()?.cast_object_ref::<Range>()?;
        let len = self.host.text.borrow().encode_utf16().count();
        let endpoint = if anchor == TF_ANCHOR_END { len.saturating_sub(other.tail.get()) } else { other.offset.get() };
        Ok((Rc::ptr_eq(&self.host, &other.host) && len.saturating_sub(self.tail.get()) == endpoint).into())
    }
    fn CompareStart(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<i32> { Err(E_NOTIMPL.into()) }
    fn CompareEnd(&self, _: u32, _: Ref<ITfRange>, _: TfAnchor) -> Result<i32> { Err(E_NOTIMPL.into()) }
    fn AdjustForInsert(&self, _: u32, _: u32) -> Result<BOOL> { Err(E_NOTIMPL.into()) }
    fn GetGravity(&self, _: *mut TfGravity, _: *mut TfGravity) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn SetGravity(&self, _: u32, _: TfGravity, _: TfGravity) -> Result<()> { Ok(()) }
    fn Clone(&self) -> Result<ITfRange> { Ok(Range { host: self.host.clone(), context: self.context.clone(), offset: Cell::new(self.offset.get()), tail: Cell::new(self.tail.get()) }.into()) }
    fn GetContext(&self) -> Result<ITfContext> { Ok(self.context.clone()) }
}

#[implement(ITfComposition)]
struct Composition { host: Rc<Host>, range: ITfRange }
#[implement(ITfCompositionSink)]
struct TestCompositionSink;
impl ITfCompositionSink_Impl for TestCompositionSink_Impl {
    fn OnCompositionTerminated(&self, _: u32, _: Ref<ITfComposition>) -> Result<()> { Ok(()) }
}
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
    let range: ITfRange = Range { host: host.clone(), context: context.clone(), offset: Cell::new(0), tail: Cell::new(0) }.into();
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
        lparam: 0, down: true, observed: system::now_ms(), access: edit::FieldAccess::Ordinary, tray: false, eligible: None });
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
        lparam: 0, down: true, observed: system::now_ms(), access, tray: false, eligible: None });
    unsafe { service.to_interface::<ITfKeyEventSink>().OnKeyDown(context, WPARAM(key as usize), LPARAM(0)).unwrap() }
}

#[test]
fn host_navigation_is_finalized_before_the_test_callback_passes_it_on() {
    use crate::shell::vk;
    for key in [vk::END, vk::HOME, vk::LEFT, vk::RIGHT, vk::UP, vk::DOWN, vk::PRIOR, vk::NEXT] {
        let (service, host, context) = fixture();
        host.scope_valid.set(true);
        host.caret_selection.set(true);
        seed(&service, &host, &context);
        let writes = host.writes.get();
        let sink = service.to_interface::<ITfKeyEventSink>();
        // Model a host that dispatches its navigation command only when TestKeyDown says false.
        // It is not sufficient to claim the key here and return false only from OnKeyDown.
        for _ in 0..2 {
            assert!(!unsafe { sink.OnTestKeyDown(&context, WPARAM(key as usize), LPARAM(0)).unwrap().as_bool() });
            assert_eq!(host.ended.get(), 1);
            assert_eq!(&*host.text.borrow(), "안녕");
            assert_eq!(host.writes.get(), writes);
        }
        assert!(!unsafe { sink.OnKeyDown(&context, WPARAM(key as usize), LPARAM(0)).unwrap().as_bool() });
        assert_eq!(host.ended.get(), 1, "a host that also invokes OnKeyDown must not finalize twice");
    }
}

#[test]
fn navigation_test_keeps_private_typing_private_and_denied_writes_unchanged() {
    for deny in [false, true] {
        let (service, host, context) = fixture();
        host.scope_valid.set(true);
        host.scope_value_failure.set(true);
        host.caret_selection.set(true);
        service.apply_context_policy(edit::FieldAccess::Private, false, Some((&context, 1))).unwrap();
        seed(&service, &host, &context);
        host.deny_write.set(deny);
        let writes = host.writes.get();
        let sink = service.to_interface::<ITfKeyEventSink>();
        for _ in 0..2 {
            assert_eq!(unsafe { sink.OnTestKeyDown(&context, WPARAM(crate::shell::vk::END as usize), LPARAM(0)).unwrap().as_bool() }, deny,
                "pending finalization must keep the host navigation claim until it actually ends");
        }
        assert_eq!(&*host.text.borrow(), "안녕");
        assert_eq!(host.writes.get(), writes);
        assert_eq!(host.ended.get(), if deny { 0 } else { 1 });
        assert!(service.state.borrow().password_field, "do not enable private-field input traces");
        assert!(service.state.borrow().shell.as_ref().unwrap().engine().learned_words().is_empty());
        host.deny_write.set(false);
        service.drain_finishes(Some((&context, 1))).unwrap();
        assert_eq!(host.ended.get(), 1, "a refused finalization remains recoverable");
    }
}

#[test]
fn navigation_test_does_not_edit_unknown_or_protected_fields() {
    for protected in [false, true] {
        let (service, host, context) = fixture();
        host.scope_valid.set(protected);
        host.read_only.set(protected);
        seed(&service, &host, &context);
        let writes = host.writes.get();
        let sink = service.to_interface::<ITfKeyEventSink>();
        assert!(unsafe { sink.OnTestKeyDown(&context, WPARAM(crate::shell::vk::END as usize), LPARAM(0)).unwrap().as_bool() });
        assert_eq!(host.ended.get(), 0);
        assert_eq!(host.writes.get(), writes);
        assert_eq!(&*host.text.borrow(), "안녕");
    }
}

#[test]
fn navigation_probe_keeps_claim_when_reentry_cancels_finalization() {
    let (service, host, context) = fixture();
    host.scope_valid.set(true);
    seed(&service, &host, &context);
    let sink = service.to_interface::<ITfKeyEventSink>();
    let reentrant = sink.clone();
    *host.before_write.borrow_mut() = Some(Box::new(move || {
        reentrant.cast_object_ref::<TextService>().unwrap().candidates.clear();
    }));
    assert!(unsafe { sink.OnTestKeyDown(&context, WPARAM(crate::shell::vk::END as usize), LPARAM(0)).unwrap().as_bool() });
    assert_eq!(host.ended.get(), 0);
    assert!(service.state.borrow().composition.is_some());
    assert!(!unsafe { sink.OnKeyDown(&context, WPARAM(crate::shell::vk::END as usize), LPARAM(0)).unwrap().as_bool() });
    assert_eq!(host.ended.get(), 1);
    assert_eq!(&*host.text.borrow(), "안녕");
}

#[test]
fn navigation_probe_disarms_a_right_alt_tap() {
    let (service, host, context) = fixture();
    host.scope_valid.set(true);
    seed(&service, &host, &context);
    let mode = service.state.borrow().shell.as_ref().unwrap().mode();
    let alt = KeyEvent::new(crate::shell::vk::RMENU, Mods::NONE);
    let _ = service.state.borrow_mut().shell.as_mut().unwrap().test_key_down(alt);
    let sink = service.to_interface::<ITfKeyEventSink>();
    assert!(!unsafe { sink.OnTestKeyDown(&context, WPARAM(crate::shell::vk::END as usize), LPARAM(0)).unwrap().as_bool() });
    let out = service.state.borrow_mut().shell.as_mut().unwrap().key_up(alt);
    assert!(!out.eaten);
    assert_eq!(service.state.borrow().shell.as_ref().unwrap().mode(), mode);
}

#[test]
fn navigation_commits_visible_text_without_rewriting_or_repositioning_it() {
    use crate::shell::vk;
    for key in [vk::END, vk::HOME, vk::LEFT, vk::RIGHT, vk::UP, vk::DOWN, vk::PRIOR, vk::NEXT, vk::RETURN, vk::TAB] {
        let (service, host, context) = fixture();
        seed(&service, &host, &context);
        host.caret_selection.set(true);
        let writes = host.writes.get();
        let caret_updates = host.caret_updates.get();
        assert!(!paired_key(&service, &context, key as u8, edit::FieldAccess::Ordinary).as_bool(), "host must receive {key}");
        assert_eq!(&*host.text.borrow(), "안녕");
        assert_eq!(host.ended.get(), 1, "composition must end before the host handles {key}");
        assert_eq!(host.writes.get(), writes, "already visible text must not be rewritten for {key}");
        assert_eq!(host.caret_updates.get(), caret_updates, "leave the caret to the host for {key}");
    }
}

#[test]
fn unchanged_navigation_commit_confirms_learning_only_after_successful_end() {
    for refused in [false, true] {
        let (service, host, context, engine) = explicit_choice_fixture();
        host.caret_selection.set(true);
        host.fail_end.set(refused);
        let writes = host.writes.get();
        assert!(!paired_key(&service, &context, crate::shell::vk::END as u8, edit::FieldAccess::Ordinary).as_bool());
        assert_eq!(&*host.text.borrow(), "dkssud");
        assert_eq!(host.writes.get(), writes);
        assert_eq!(engine.learned_count(), if refused { 0 } else { 1 });
        service.state.borrow_mut().sink = Some(TestCompositionSink.into());
        assert!(paired_key(&service, &context, b'D', edit::FieldAccess::Ordinary).as_bool());
        assert_eq!(&*host.text.borrow(), "dkssudd", "the same English-mode next key must append once after either successful or refused End");
    }
}

#[test]
fn navigation_normalizes_nonempty_interim_or_unavailable_selection_before_host_key() {
    for kind in 0..4 {
        let (service, host, context) = fixture();
        seed(&service, &host, &context);
        host.caret_selection.set(kind != 0);
        host.interim_selection.set(kind == 1);
        host.selection_failure.set(kind == 2);
        host.caret_at_start.set(kind == 3);
        host.normalized_selection.set(false);
        let caret_updates = host.caret_updates.get();
        assert!(!paired_key(&service, &context, crate::shell::vk::RETURN as u8, edit::FieldAccess::Ordinary).as_bool());
        assert_eq!(&*host.text.borrow(), "안녕");
        assert_eq!(host.ended.get(), 1);
        assert_eq!(host.caret_updates.get(), caret_updates + 1, "preserve existing caret normalization for selection kind {kind}");
        assert!(host.normalized_selection.get(), "host key must see an ordinary caret at the composition end");
    }
}

#[test]
fn navigation_read_failure_preserves_text_and_does_not_train_an_unverified_commit() {
    let (service, host, context, engine) = explicit_choice_fixture();
    host.caret_selection.set(true);
    host.fail_read_after_caret_query.set(true);
    let writes = host.writes.get();
    assert!(!paired_key(&service, &context, crate::shell::vk::END as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(host.writes.get(), writes, "a failed safety read must not fall through to an unverified overwrite");
    assert_eq!(&*host.text.borrow(), "dkssud");
    assert_eq!(engine.learned_count(), 0);
    assert_eq!(host.ended.get(), 1, "guarded cleanup can finish after the transient read failure");
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    assert!(paired_key(&service, &context, b'D', edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "dkssudd");
}

#[test]
fn navigation_selection_query_allows_host_reentry_without_discarding_the_commit() {
    let (service, host, context, engine) = explicit_choice_fixture();
    host.caret_selection.set(true);
    let sink = service.to_interface::<ITfKeyEventSink>();
    *host.before_selection.borrow_mut() = Some(Box::new(move || {
        let service = sink.cast_object_ref::<TextService>().unwrap();
        let _state = service.state.borrow_mut();
    }));
    assert!(!paired_key(&service, &context, crate::shell::vk::END as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "dkssud");
    assert_eq!(engine.learned_count(), 1, "a successful reentrant selection query must not turn finalization into failure");
    assert_eq!(host.ended.get(), 1);
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

#[test]
fn a_failed_old_context_must_not_block_another_context() {
    let (service, old, a) = fixture();
    seed(&service, &old, &a);
    old.deny.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    let next = Rc::new(Host::default());
    let b: ITfContext = Context(next.clone()).into();
    seed(&service, &next, &b);
    assert!(paired_key(&service, &b, b'X', edit::FieldAccess::Ordinary).as_bool(), "an unrelated denied context must not pass this key raw");
    assert!(service.state.borrow().shell.as_ref().unwrap().is_composing());
    assert_eq!(next.ended.get(), 0);
}

#[test]
fn host_termination_retires_a_queued_finish() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    let composition = service.state.borrow().composition.clone().unwrap();
    host.defer.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    unsafe { service.to_interface::<ITfCompositionSink>().OnCompositionTerminated(1, &composition).unwrap(); }
    assert!(service.state.borrow().pending_finishes.iter().all(|finish| finish.done.get()));
    for session in std::mem::take(&mut *host.pending.borrow_mut()) { unsafe { session.DoEditSession(1).unwrap(); } }
    assert_eq!(host.ended.get(), 0, "the host already ended it; the delayed callback must not end it again");
}

#[test]
fn composing_caret_failure_does_not_split_the_word() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.fail_caret.set(true);
    assert!(paired_key(&service, &context, b'X', edit::FieldAccess::Ordinary).as_bool());
    assert!(service.state.borrow().shell.as_ref().unwrap().is_composing());
    assert_eq!(host.ended.get(), 0);
}

#[test]
fn preserved_probe_suppresses_trace_even_when_the_write_lock_is_denied() {
    let (service, host, context) = fixture();
    service.state.borrow_mut().password_field = false;
    host.deny.set(true);
    unsafe { let _ = service.to_interface::<ITfKeyEventSink>().OnPreservedKey(&context, &PRESERVED_RECONVERT).unwrap(); }
    assert!(service.state.borrow().password_field, "unknown scope must suppress both before/after traces even without a write callback");
}

#[test]
fn unknown_key_up_never_claims_a_write_or_splits_a_word() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    let sink = service.to_interface::<ITfKeyEventSink>();
    assert!(!unsafe { sink.OnTestKeyUp(&context, WPARAM(b'D' as usize), LPARAM(0)).unwrap().as_bool() });
    assert!(!unsafe { sink.OnKeyUp(&context, WPARAM(b'D' as usize), LPARAM(0)).unwrap().as_bool() });
    assert_eq!(host.write_requests.get(), 0);
    assert_eq!(host.ended.get(), 0);
    assert!(service.state.borrow().shell.as_ref().unwrap().is_composing());
}

#[test]
fn transient_read_denial_is_rechecked_under_the_write_lock() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.scope_valid.set(true);
    assert_eq!(edit::field_access(1, &context), edit::FieldAccess::Ordinary, "exercise the real field query through the mock COM interfaces");
    host.deny_read.set(true);
    let sink = service.to_interface::<ITfKeyEventSink>();
    assert!(unsafe { sink.OnTestKeyDown(&context, WPARAM(b'X' as usize), LPARAM(0)).unwrap().as_bool() });
    {
        let mut st = service.state.borrow_mut();
        let checked = st.checked_key.as_mut().unwrap();
        assert!(matches!(checked.access, edit::FieldAccess::Unknown(_)), "the read-only probe was actually denied");
        // Coexistence has its own integration test. This COM fixture must not depend on the user's live tray.
        checked.tray = false;
    }
    assert!(unsafe { sink.OnKeyDown(&context, WPARAM(b'X' as usize), LPARAM(0)).unwrap().as_bool() });
    assert_eq!(host.ended.get(), 0);
    assert!(service.state.borrow().shell.as_ref().unwrap().is_composing());
    assert_eq!(host.write_requests.get(), 1);
}

#[test]
fn unsupported_optional_input_scope_does_not_disable_an_ordinary_context() {
    let (_, host, context) = fixture();
    host.scope_valid.set(true);
    host.app_property_result.set(Some(windows::Win32::Foundation::S_FALSE));
    let mut raw = std::ptr::null_mut();
    let hr = unsafe { (context.vtable().GetAppProperty)(context.as_raw(), &GUID_PROP_INPUTSCOPE, &mut raw) };
    assert_eq!(hr, windows::Win32::Foundation::S_FALSE);
    assert!(raw.is_null(), "reproduce the native optional-property ABI, not a Rust None");
    assert_eq!(edit::field_access(1, &context), edit::FieldAccess::Ordinary);
    host.read_only.set(true);
    assert_eq!(edit::field_access(1, &context), edit::FieldAccess::Protected, "absent scope must not override the host's protection");
    host.read_only.set(false);
    host.scope_valid.set(false);
    assert!(edit::field_access(1, &context).blocks_keys(), "absent scope must not hide a compartment failure");
}

#[test]
fn failed_or_invalid_scope_property_never_allows_input() {
    let (_, host, context) = fixture();
    host.scope_valid.set(true);
    for code in [E_FAIL, E_NOTIMPL, E_INVALIDARG, TF_E_DISCONNECTED, S_OK, HRESULT(2)] {
        host.app_property_result.set(Some(code));
        let probe = edit::field_probe(1, &context);
        assert!(probe.access.blocks_keys(), "unexpected property result {code:?}");
        let failure = probe.failure.unwrap();
        assert_eq!(failure.stage, edit::FieldQueryStage::ScopeProperty);
        assert_eq!(failure.code, if code.is_err() { code } else { windows::Win32::Foundation::E_UNEXPECTED });
    }
}

#[test]
fn field_diagnostics_distinguish_identical_hresult_without_document_contents() {
    use edit::FieldQueryStage as Stage;
    let (_, host, context) = fixture();
    for expected in [Stage::Status, Stage::DisabledCompartment, Stage::Selection, Stage::ScopeValue] {
        host.status_failure.set(expected == Stage::Status);
        host.scope_valid.set(expected != Stage::DisabledCompartment);
        host.selection_failure.set(expected == Stage::Selection);
        host.scope_value_failure.set(expected == Stage::ScopeValue);
        let probe = edit::field_probe(1, &context);
        assert_eq!(probe.access, if expected == Stage::ScopeValue { edit::FieldAccess::Private } else { edit::FieldAccess::Unknown(E_FAIL) });
        assert_eq!(probe.failure, Some(edit::FieldQueryFailure { stage: expected, code: E_FAIL }));
        assert_eq!(host.writes.get(), 0);
    }
}

#[test]
fn word_scope_value_failure_keeps_typing_private_after_positive_host_checks() {
    let (service, host, context) = fixture();
    host.scope_valid.set(true);
    host.scope_value_failure.set(true);
    let probe = edit::field_probe(1, &context);
    assert_eq!(probe.access, edit::FieldAccess::Private);
    assert_eq!(probe.failure, Some(edit::FieldQueryFailure { stage: edit::FieldQueryStage::ScopeValue, code: E_FAIL }));
    service.apply_context_policy(probe.access, false, Some((&context, 1))).unwrap();
    assert!(service.state.borrow().password_field, "no input trace in an unknown scope");
    seed(&service, &host, &context);
    let shell_state = service.state.borrow();
    let shell = shell_state.shell.as_ref().unwrap();
    assert!(shell.private_context());
    assert!(shell.engine().learning_paused());
    drop(shell_state);
    assert!(paired_key(&service, &context, b' ', probe.access).as_bool());
    assert_eq!(&*host.text.borrow(), "안녕 ");
    assert!(service.state.borrow().shell.as_ref().unwrap().engine().learned_words().is_empty());
}

#[test]
fn optional_scope_failure_never_overrides_disabled_empty_or_readonly_host() {
    let (_, host, context) = fixture();
    host.scope_valid.set(true);
    host.scope_value_failure.set(true);
    for flag in [&host.keyboard_disabled, &host.empty_context, &host.read_only] {
        flag.set(true);
        assert_eq!(edit::field_access(1, &context), edit::FieldAccess::Protected);
        flag.set(false);
    }
    host.scope_valid.set(false);
    assert!(edit::field_access(1, &context).blocks_keys());
    assert_eq!(host.writes.get(), 0);
}

#[test]
fn scope_lock_disconnect_and_invalid_argument_errors_still_block_keys() {
    let (_, host, context) = fixture();
    host.scope_valid.set(true);
    for code in [TF_E_NOLOCK, TF_E_DISCONNECTED, E_INVALIDARG, E_NOTIMPL] {
        host.scope_value_error.set(Some(code));
        let probe = edit::field_probe(1, &context);
        assert_eq!(probe.access, edit::FieldAccess::Unknown(code));
        assert!(probe.access.blocks_keys());
    }
}

#[test]
fn skipped_reconversion_passes_f2_and_discards_stale_history() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    assert!(service.state.borrow().shell.as_ref().unwrap().engine().reconversion().is_some());
    // The host now has a non-collapsed selection, so the previous word cannot be replaced safely.
    let before = host.text.borrow().clone();
    let writes = host.writes.get();
    assert!(!paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool(),
        "a skipped F2 must remain available to the application");
    assert_eq!(&*host.text.borrow(), &before);
    assert_eq!(host.writes.get(), writes);
    assert!(service.state.borrow().shell.as_ref().unwrap().engine().reconversion().is_none());
}

#[test]
fn deactivation_finishes_a_live_composition_before_releasing_state() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    assert_eq!(host.ended.get(), 1);
    assert_eq!(&*host.text.borrow(), "안녕");
    assert!(service.state.borrow().shell.is_none());
}

#[test]
fn deactivation_invalidates_queued_edits_when_final_lock_is_denied() {
    let (service, host, context) = fixture();
    let preview = seed(&service, &host, &context);
    host.defer.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(host.pending.borrow().len(), 1);
    host.deny.set(true);
    unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
    let writes = host.writes.get();
    for session in std::mem::take(&mut *host.pending.borrow_mut()) { unsafe { session.DoEditSession(1).unwrap(); } }
    assert_eq!(host.writes.get(), writes, "an old edit must not write after service deactivation");
    assert_eq!(host.ended.get(), 0);
    assert_eq!(&*host.text.borrow(), &preview);
}

#[test]
fn an_earlier_commit_cannot_consume_a_later_unwritten_character() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    let sink = service.to_interface::<ITfKeyEventSink>();
    let implementation = sink.cast_object_ref::<TextService>().unwrap();
    // Exercise the future-compatible multi-operation contract. The existing composition commits successfully,
    // then this host refuses the new composition (no ITfInsertAtSelection). The new key must pass exactly once.
    let eaten = implementation.key_in_session(&context, edit::FieldAccess::Ordinary, false, |_| Outcome {
        eaten: true, ops: vec![Op::Commit("안녕".into()), Op::Compose("ㅌ".into())], ..Outcome::default()
    }).unwrap();
    assert!(!eaten.as_bool(), "writing the previous word is not proof the current character was inserted");
    assert_eq!(&*host.text.borrow(), "안녕");
    host.text.borrow_mut().push('x');
    assert_eq!(&*host.text.borrow(), "안녕x");
}

#[test]
fn a_completed_pause_command_is_not_passed_again_after_commit_failure() {
    let (service, host, context) = fixture();
    let preview = seed(&service, &host, &context);
    host.fail_set.set(true);
    let eaten = paired_key(&service, &context, crate::shell::vk::F12 as u8, edit::FieldAccess::Ordinary);
    assert!(service.state.borrow().shell.as_ref().unwrap().engine().paused());
    assert!(eaten.as_bool(), "the pause was already applied; do not run the host's F12 too");
    assert_eq!(&*host.text.borrow(), &preview);
}

#[test]
fn reconversion_round_trip_preserves_the_prefix_without_language_property_support() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    *host.text.borrow_mut() = "prefix 안녕 ".into();
    host.caret_selection.set(true);
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "prefix dkssud ");
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "prefix 안녕 ");
}

#[test]
fn reconversion_preserves_the_existing_word_separator_without_retyping_it() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    *host.text.borrow_mut() = "prefix 안녕 ".into();
    host.caret_selection.set(true);
    host.autocorrect_space.set(true);
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "prefix dkssud ", "retyping the delimiter gives host AutoCorrect a new word boundary");
    assert_eq!(&*host.last_write.borrow(), "dkssud");
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "prefix 안녕 ");
    assert_eq!(&*host.last_write.borrow(), "안녕");
}

#[test]
fn refused_reconversion_range_trim_keeps_the_original_word() {
    let (service, host, context) = fixture();
    *host.text.borrow_mut() = "안녕 ".into();
    host.caret_selection.set(true);
    host.refuse_end_shift.set(true);
    assert!(service.replace_before(1, &context, "안녕 ", "dkssud ").is_err());
    assert_eq!(&*host.text.borrow(), "안녕 ");
    assert_eq!(host.writes.get(), 0);
}

#[test]
fn selection_flip_preserves_trailing_whitespace_without_retyping_the_word_boundary() {
    for spaces in [" ", "  ", "\t", "\t  ", "\r\n", "\u{00a0}", "\u{3000}"] {
        let (service, host, context) = fixture();
        *host.text.borrow_mut() = format!("안녕{spaces}");
        host.autocorrect_space.set(true);
        service.flip_selection(1, &context).unwrap();
        assert_eq!(&*host.text.borrow(), &format!("dkssud{spaces}"));
        assert_eq!(&*host.last_write.borrow(), "dkssud");
        service.flip_selection(1, &context).unwrap();
        assert_eq!(&*host.text.borrow(), &format!("안녕{spaces}"));
        assert_eq!(&*host.last_write.borrow(), "안녕");
    }
}

#[test]
fn refused_selection_flip_trim_preserves_the_original_document() {
    for suffix in [" ", "\t", "\u{3000}"] {
        let (service, host, context) = fixture();
        *host.text.borrow_mut() = format!("안녕{suffix}");
        host.refuse_end_shift.set(true);
        assert!(service.flip_selection(1, &context).is_err());
        assert_eq!(&*host.text.borrow(), &format!("안녕{suffix}"));
        assert_eq!(host.writes.get(), 0);
    }
}

#[test]
fn selection_flip_preserves_leading_and_internal_whitespace() {
    let (service, host, context) = fixture();
    *host.text.borrow_mut() = "\t안녕 \t가바\t\u{3000}".into();
    service.flip_selection(1, &context).unwrap();
    assert_eq!(&*host.text.borrow(), "\tdkssud \trkqk\t\u{3000}");
    assert_eq!(&*host.last_write.borrow(), "\tdkssud \trkqk");
}

#[test]
fn correction_language_is_ready_before_the_hosts_implicit_composition_ends() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    host.caret_selection.set(true);
    host.language_property.set(true);
    host.language.set(Some(0x0412));
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(host.language_at_write.get(), Some(0x0409), "SetText ends its implicit composition before returning to the caller");
    assert_eq!(&*host.text.borrow(), "dkssud ");
}

#[test]
fn rejected_correction_restores_the_original_language_including_absent_property() {
    for original in [None, Some(0x0412), Some(0x0809)] {
        let (service, host, context) = fixture();
        seed(&service, &host, &context);
        assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
        host.caret_selection.set(true);
        host.language_property.set(true);
        host.language.set(original);
        host.fail_set.set(true);
        assert!(!paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
        assert_eq!(host.language.get(), original);
        if original.is_none() { assert!(host.language_at_write.get().is_none(), "absent or mixed language spans must not be flattened"); }
        assert_eq!(&*host.text.borrow(), "안녕 ");
    }
}

#[test]
fn refused_explicit_choice_commit_must_not_train_the_dictionary() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone();
    assert_eq!(engine.learned_count(), 0);
    host.fail_set.set(true);
    assert!(!paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(engine.learned_count(), 0, "a refused document commit is not a confirmed learning choice");
}

fn explicit_choice_fixture() -> (ComObject<TextService>, Rc<Host>, ITfContext, Arc<Engine>) {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone();
    assert_eq!(engine.learned_count(), 0);
    (service, host, context, engine)
}

fn queue_pair() -> (hanautomata_core::learning::client::Producer, hanautomata_core::learning::client::Worker) {
    use hanautomata_core::learning::{client::channel, Lease, Version};
    channel(Lease { client_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(), lease_revision: 2,
        version: Version { store_id: "11111111111111111111111111111111".into(), generation: 1, revision: 2 }, next_sequence: 1 }).unwrap()
}

fn connected_fixture() -> (ComObject<TextService>, Rc<Host>, ITfContext, hanautomata_core::learning::client::Producer, hanautomata_core::learning::client::Worker) {
    let (service, host, context) = fixture();
    host.caret_selection.set(true);
    let (producer, worker) = queue_pair();
    service.state.borrow_mut().learning.connect(producer.clone());
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    (service, host, context, producer, worker)
}

fn queued_choice_fixture() -> (ComObject<TextService>, Rc<Host>, ITfContext, hanautomata_core::learning::client::Worker) {
    let (service, host, context, _, worker) = connected_fixture();
    for key in b"DKSSUD" { assert!(paired_key(&service, &context, *key, edit::FieldAccess::Ordinary).as_bool()); }
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    (service, host, context, worker)
}

#[path = "service_learning_tests.rs"]
mod learning_handoff_tests;

#[test]
fn confirmed_service_choice_reaches_its_queue_only_after_the_document_write() {
    let (service, host, context, mut worker) = queued_choice_fixture();
    assert!(worker.next().unwrap().is_none(), "an uncommitted choice cannot reach persistence");
    assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "dkssud ");
    let request = worker.next().unwrap().expect("confirmed document choice was discarded instead of queued");
    assert_eq!(request.request().generation, 1);
    assert!(matches!(&request.request().operation, hanautomata_core::learning::Operation::Learn { raw, korean: false, .. } if raw == "dkssud"));
    assert_eq!(service.state.borrow().learning.report().queued, 1);
}

#[test]
fn successful_explicit_commit_and_f2_reconversion_confirm_one_choice_each() {
    let (service, host, context, engine) = explicit_choice_fixture();
    assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "dkssud ");
    assert_eq!(engine.learned_count(), 1);
    assert!(engine.can_undo_learning());
    host.caret_selection.set(true);
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "안녕 ");
    assert!(engine.undo_learning());
    assert_eq!(engine.learned_count(), 1, "reconversion was a second confirmed choice");
    assert!(engine.undo_learning());
    assert_eq!(engine.learned_count(), 0);
}

#[test]
fn delayed_finalization_trains_only_after_the_original_range_accepts_the_choice() {
    let (service, host, _, engine) = explicit_choice_fixture();
    host.defer.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert_eq!(engine.learned_count(), 0);
    let pending = std::mem::take(&mut *host.pending.borrow_mut());
    assert_eq!(pending.len(), 1);
    unsafe { pending[0].DoEditSession(1).unwrap(); pending[0].DoEditSession(1).unwrap(); }
    assert_eq!(engine.learned_count(), 1);
    assert!(engine.undo_learning());
    assert_eq!(engine.learned_count(), 0, "a replayed callback must not add a second observation");
}

#[test]
fn host_changes_or_learning_pause_cancel_delayed_learning() {
    for host_changed in [true, false] {
        let (service, host, _, engine) = explicit_choice_fixture();
        host.defer.set(true);
        unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
        if host_changed { host.text.borrow_mut().push_str(" host edit"); }
        else { engine.set_learning_paused(true); engine.set_learning_paused(false); }
        for pending in std::mem::take(&mut *host.pending.borrow_mut()) { unsafe { pending.DoEditSession(1).unwrap(); } }
        assert_eq!(engine.learned_count(), 0);
    }
}

#[test]
fn terminated_composition_confirms_only_a_successful_final_write() {
    for rejected in [false, true] {
        let (service, host, _, engine) = explicit_choice_fixture();
        let composition = service.state.borrow().composition.clone().unwrap();
        host.fail_set.set(rejected);
        let result = unsafe { service.to_interface::<ITfCompositionSink>().OnCompositionTerminated(1, &composition) };
        assert_eq!(result.is_err(), rejected, "a failed host write remains an API failure");
        assert_eq!(engine.learned_count(), if rejected { 0 } else { 1 });
    }
}

#[test]
fn post_write_failure_preserves_text_but_does_not_confirm_incomplete_learning() {
    for failure in 0..2 {
        let (service, host, context, engine) = explicit_choice_fixture();
        if failure == 0 { host.fail_caret.set(true); } else { host.fail_end.set(true); }
        assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
        assert_eq!(&*host.text.borrow(), "dkssud ", "the accepted text is never replayed");
        assert_eq!(engine.learned_count(), 0, "a partial operation conservatively omits this learning choice");
    }
}

#[test]
fn failed_finalization_retry_does_not_train_before_a_write_or_train_twice_after_it() {
    let (service, host, _, engine) = explicit_choice_fixture();
    host.fail_set.set(true);
    let sink = service.to_interface::<ITfKeyEventSink>();
    unsafe { sink.OnSetFocus(false).unwrap(); }
    assert_eq!(engine.learned_count(), 0);
    unsafe { sink.OnSetFocus(false).unwrap(); sink.OnSetFocus(false).unwrap(); }
    assert_eq!(engine.learned_count(), 1);
    assert!(engine.undo_learning());
    assert_eq!(engine.learned_count(), 0);
}

#[test]
fn reconversion_never_overwrites_host_changed_text_at_the_caret() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    assert!(paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary).as_bool());
    *host.text.borrow_mut() = "prefix host-changed ".into();
    host.caret_selection.set(true);
    let writes = host.writes.get();
    assert!(!paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(&*host.text.borrow(), "prefix host-changed ");
    assert_eq!(host.writes.get(), writes);
    assert!(service.state.borrow().shell.as_ref().unwrap().engine().reconversion().is_none());
}

#[test]
fn shift_f2_keeps_its_command_when_the_selection_has_nothing_to_flip() {
    let (service, host, context) = fixture();
    seed(&service, &host, &context);
    host.caret_selection.set(true);
    let sink = service.to_interface::<ITfKeyEventSink>();
    let implementation = sink.cast_object_ref::<TextService>().unwrap();
    let mut event = KeyEvent::new(crate::shell::vk::F2, Mods::NONE);
    event.mods.shift = true;
    assert!(implementation.key_in_session(&context, edit::FieldAccess::Ordinary, false,
        move |shell| shell.key_down(event)).unwrap().as_bool());
    assert_eq!(&*host.text.borrow(), "안녕");
    assert_eq!(host.ended.get(), 1);
}

#[test]
fn post_write_control_effect_panics_do_not_escape_to_the_key_fallback() {
    let (service, _, _) = fixture();
    let sink = service.to_interface::<ITfKeyEventSink>();
    let implementation = sink.cast_object_ref::<TextService>().unwrap();
    let _held = service.state.borrow_mut();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| implementation.apply_effects(&Outcome::default())));
    assert!(result.is_ok(), "a post-write indicator failure must not make the outer callback replay a written key");
}

#[test]
fn excluded_app_stays_pass_through_after_context_changes_and_edit_dispatch() {
    let (service, host, context) = fixture();
    service.state.borrow_mut().excluded_app = true;
    service.apply_context_policy(edit::FieldAccess::Ordinary, false, None).unwrap();
    assert!(service.state.borrow().shell.as_ref().unwrap().pass_through);
    let sink = service.to_interface::<ITfKeyEventSink>();
    let implementation = sink.cast_object_ref::<TextService>().unwrap();
    let eaten = implementation.key_in_session(&context, edit::FieldAccess::Ordinary, false,
        |_| panic!("excluded application must never run the engine command")).unwrap();
    assert!(!eaten.as_bool());
    assert_eq!(host.writes.get(), 0);
}

#[test]
fn excluded_app_passes_ordinary_and_preserved_keys_without_requesting_a_write() {
    let (service, host, context) = fixture();
    service.state.borrow_mut().excluded_app = true;
    let sink = service.to_interface::<ITfKeyEventSink>();
    for key in [b'D' as u16, crate::shell::vk::F2, crate::shell::vk::F12, crate::shell::vk::HANGUL] {
        assert!(!unsafe { sink.OnTestKeyDown(&context, WPARAM(key as usize), LPARAM(0)).unwrap().as_bool() });
        assert!(!paired_key(&service, &context, key as u8, edit::FieldAccess::Ordinary).as_bool());
    }
    assert!(!unsafe { sink.OnPreservedKey(&context, &PRESERVED_RECONVERT).unwrap().as_bool() });
    assert_eq!(host.write_requests.get(), 0);
    assert_eq!(host.writes.get(), 0);
    assert!(!service.state.borrow().shell.as_ref().unwrap().is_composing());
}
