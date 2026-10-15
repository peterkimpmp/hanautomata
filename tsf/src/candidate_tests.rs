use super::*;
use windows::Win32::Foundation::E_NOTIMPL;

fn snapshot() -> Snapshot {
    Snapshot { preview: Preview { text: "안녕".into(), alternative: "dkssud".into(), label: "".into(), correction: true },
        learning_notice: "이 입력 세션의 선택 기억: 준비됨 · 저장 확인 대기 1건".into(),
        document: None, context: None, source: None,
        geometry: Geometry { owner: HWND::default(), caret: Some(RECT { left: 10, top: 10, right: 10, bottom: 30 }) } }
}

#[test]
fn candidate_learning_notice_is_descriptive_and_retirement_removes_it() {
    let element = Element::new(snapshot());
    let list = element.to_interface::<ITfCandidateListUIElement>();
    unsafe {
        assert!(list.GetDescription().unwrap().to_string().contains("선택 기억"), "host-rendered preview needs a separate learning-state description");
        assert_eq!(list.GetCount().unwrap(), 2, "status must not become a third selectable candidate");
        assert_eq!(list.GetString(0).unwrap().to_string(), "안녕");
        assert_eq!(list.GetString(1).unwrap().to_string(), "dkssud");
        element.clear();
        assert!(!list.GetDescription().unwrap().to_string().contains("선택 기억"), "retired host elements must not retain the previous field's state");
        assert_eq!(list.GetCount().unwrap(), 0);
    }
}

#[derive(Default)]
struct Host {
    retained: RefCell<Option<ITfCandidateListUIElement>>,
    events: RefCell<Vec<&'static str>>,
    presenter: RefCell<Weak<Presenter>>,
    cancel_at: Cell<u8>,
    fail_at: Cell<u8>,
}
#[implement(ITfUIElementMgr)]
struct Manager(Rc<Host>);
impl ITfUIElementMgr_Impl for Manager_Impl {
    fn BeginUIElement(&self, element: Ref<ITfUIElement>, show: *mut BOOL, id: *mut u32) -> Result<()> {
        let list = element.as_ref().ok_or(E_POINTER)?.cast::<ITfCandidateListUIElement>()?;
        *self.0.retained.borrow_mut() = Some(list);
        self.0.events.borrow_mut().push("begin");
        unsafe { *show = BOOL(0); *id = 17; }
        if self.0.cancel_at.get() == 1 { self.0.presenter.borrow().upgrade().unwrap().clear(); }
        if self.0.fail_at.get() == 1 { return Err(E_FAIL.into()); }
        Ok(())
    }
    fn UpdateUIElement(&self, id: u32) -> Result<()> {
        assert_eq!(id, 17);
        self.0.events.borrow_mut().push("update");
        let list = self.0.retained.borrow().clone().unwrap();
        unsafe {
            assert_eq!(list.GetCount()?, 2);
            assert_eq!(list.GetUpdatedFlags()?, ALL_CHANGED);
            assert!(!list.IsShown()?.as_bool());
        }
        if self.0.cancel_at.get() == 2 { self.0.presenter.borrow().upgrade().unwrap().clear(); }
        if self.0.fail_at.get() == 2 { return Err(E_FAIL.into()); }
        Ok(())
    }
    fn EndUIElement(&self, id: u32) -> Result<()> {
        assert_eq!(id, 17);
        self.0.events.borrow_mut().push("end");
        let list = self.0.retained.borrow().clone().unwrap();
        unsafe {
            assert_eq!(list.GetCount()?, 0, "clear before calling external code");
            assert!(list.GetString(0).is_err());
            list.Show(true)?;
            assert!(!list.IsShown()?.as_bool(), "retired elements cannot reopen");
        }
        if self.0.cancel_at.get() == 3 { self.0.presenter.borrow().upgrade().unwrap().clear(); }
        Ok(())
    }
    fn GetUIElement(&self, _: u32) -> Result<ITfUIElement> { Err(E_NOTIMPL.into()) }
    fn EnumUIElements(&self) -> Result<IEnumTfUIElements> { Err(E_NOTIMPL.into()) }
}

fn fixture() -> (Rc<Presenter>, Rc<Host>, ITfUIElementMgr) {
    let p = Rc::new(Presenter::default());
    let host = Rc::new(Host::default());
    *host.presenter.borrow_mut() = Rc::downgrade(&p);
    let manager = Manager(host.clone()).into();
    (p, host, manager)
}

#[test]
fn candidate_host_draws_two_strings_and_retained_element_is_empty_after_end() {
    let (p, host, manager) = fixture();
    p.present(manager, snapshot(), p.ticket()).unwrap();
    let list = host.retained.borrow().clone().unwrap();
    unsafe {
        assert_eq!(list.GetString(0).unwrap().to_string(), "안녕");
        assert_eq!(list.GetString(1).unwrap().to_string(), "dkssud");
        assert_eq!(list.GetSelection().unwrap(), 0);
        assert!(list.GetString(2).is_err());
    }
    p.clear();
    assert_eq!(*host.events.borrow(), ["begin", "update", "end"]);
    unsafe { assert_eq!(list.GetCount().unwrap(), 0); }
}

#[test]
fn candidate_reentrant_focus_loss_at_begin_or_update_cannot_resurrect() {
    for stage in [1, 2] {
        let (p, host, manager) = fixture();
        host.cancel_at.set(stage);
        p.present(manager, snapshot(), p.ticket()).unwrap();
        assert!(p.active.borrow().is_none());
        let list = host.retained.borrow().clone().unwrap();
        unsafe { assert_eq!(list.GetCount().unwrap(), 0); }
        assert_eq!(host.events.borrow().iter().filter(|e| **e == "end").count(), 1);
    }
}

#[test]
fn candidate_reentrant_end_does_not_double_retire_or_hold_the_active_borrow() {
    let (p, host, manager) = fixture();
    p.present(manager, snapshot(), p.ticket()).unwrap();
    host.cancel_at.set(3);
    let before = p.ticket();
    p.clear();
    assert_eq!(p.ticket(), before + 2);
    assert!(!p.active());
    assert_eq!(*host.events.borrow(), ["begin", "update", "end"]);
}

#[test]
fn candidate_foreign_thread_getter_is_rejected_before_accessing_apartment_state() {
    let element = Element::new(snapshot());
    let list: ITfCandidateListUIElement = element.to_interface();
    let raw = Interface::into_raw(list.clone()) as usize;
    let result = std::thread::spawn(move || unsafe {
        let foreign = ITfCandidateListUIElement::from_raw(raw as *mut _);
        foreign.GetCount().err().map(|e| e.code())
    }).join().unwrap();
    assert_eq!(result, Some(windows::Win32::Foundation::RPC_E_WRONG_THREAD));
}

#[test]
fn candidate_foreign_thread_sink_is_rejected_and_retired_element_can_release_there() {
    let (p, host, manager) = fixture();
    p.present(manager, snapshot(), p.ticket()).unwrap();
    let observer = Observer::new(&p);
    let sink: ITfTextLayoutSink = observer.to_interface();
    let raw = Interface::into_raw(sink) as usize;
    let result = std::thread::spawn(move || unsafe {
        let foreign = ITfTextLayoutSink::from_raw(raw as *mut _);
        foreign.OnLayoutChange(None, TF_LC_CHANGE, None).err().map(|e| e.code())
    }).join().unwrap();
    assert_eq!(result, Some(windows::Win32::Foundation::RPC_E_WRONG_THREAD));
    assert!(p.active());
    p.clear();
    let list = host.retained.borrow_mut().take().unwrap();
    unsafe { assert_eq!(list.GetCount().unwrap(), 0); }
    let raw = Interface::into_raw(list) as usize;
    std::thread::spawn(move || unsafe { drop(ITfCandidateListUIElement::from_raw(raw as *mut _)); }).join().unwrap();
}

#[test]
fn candidate_failed_begin_or_update_scrubs_host_retained_text() {
    for stage in [1, 2] {
        let (p, host, manager) = fixture();
        host.fail_at.set(stage);
        assert!(p.present(manager, snapshot(), p.ticket()).is_err());
        let list = host.retained.borrow().clone().unwrap();
        unsafe { assert_eq!(list.GetCount().unwrap(), 0); }
        assert!(p.active.borrow().is_none());
        assert_eq!(host.events.borrow().iter().filter(|e| **e == "end").count(), usize::from(stage == 2));
    }
}

#[test]
fn candidate_stale_ticket_never_reaches_host_and_drop_retires_active_element() {
    let (p, host, manager) = fixture();
    let stale = p.ticket(); p.clear();
    p.present(manager.clone(), snapshot(), stale).unwrap();
    assert!(host.events.borrow().is_empty());
    p.present(manager, snapshot(), p.ticket()).unwrap();
    drop(p);
    assert_eq!(*host.events.borrow(), ["begin", "update", "end"]);
}

#[test]
fn candidate_pages_validate_pointers_bounds_and_partition() {
    let element = Element::new(snapshot());
    let list: ITfCandidateListUIElement = element.to_interface();
    unsafe {
        assert!(list.GetPageIndex(&mut [0], std::ptr::null_mut()).is_err());
        let mut count = 0;
        list.GetPageIndex(&mut [], &mut count).unwrap(); // S_FALSE is success, required size is returned.
        assert_eq!(count, 1);
        list.SetPageIndex(&[0, 1]).unwrap();
        let mut pages = [99u32; 2];
        list.GetPageIndex(&mut pages, &mut count).unwrap();
        assert_eq!((pages, count), ([0, 1], 2));
        for bad in [&[][..], &[1][..], &[0, 0][..], &[0, 2][..], &[0, 1, 2][..]] {
            assert!(list.SetPageIndex(bad).is_err());
        }
    }
}

#[test]
fn candidate_caret_rejects_hidden_clipped_inverted_and_overflow_ranges() {
    let rect = RECT { left: -900, top: 20, right: -900, bottom: 40 };
    assert!(valid_caret(rect, false));
    assert!(!valid_caret(rect, true));
    assert!(!valid_caret(RECT::default(), false));
    assert!(!valid_caret(RECT { right: -901, ..rect }, false));
    assert!(!valid_caret(RECT { left: i32::MIN, right: i32::MAX, ..rect }, false));
}

#[test]
fn candidate_edit_and_layout_observers_retire_without_reading_document_text() {
    for layout in [false, true] {
        let (p, host, manager) = fixture();
        p.present(manager, snapshot(), p.ticket()).unwrap();
        let observer = Observer::new(&p);
        unsafe {
            if layout {
                let sink: ITfTextLayoutSink = observer.to_interface();
                sink.OnLayoutChange(None, TF_LC_CHANGE, None).unwrap();
            } else {
                let sink: ITfTextEditSink = observer.to_interface();
                sink.OnEndEdit(None, 0, None).unwrap();
            }
            assert_eq!(host.retained.borrow().as_ref().unwrap().GetCount().unwrap(), 0);
        }
    }
}

// Controllable host read locks: a result may arrive after a focus change or another layout event.
#[derive(Default)]
struct LayoutHost {
    pending: RefCell<Vec<LayoutReply>>,
    invalid: Cell<bool>,
    geometry: Cell<Option<RECT>>,
    samples: Cell<u32>,
}
impl LayoutSource for LayoutHost {
    fn matches(&self, _: Option<&ITfContext>) -> bool { true }
    fn sample(&self, _: u32) -> Result<Geometry> {
        self.samples.set(self.samples.get() + 1);
        if self.invalid.get() { Err(E_FAIL.into()) }
        else { Ok(Geometry { owner: HWND::default(), caret: self.geometry.get() }) }
    }
    fn request(&self, reply: LayoutReply) -> Result<()> {
        self.pending.borrow_mut().push(reply); Ok(())
    }
}
impl LayoutHost {
    fn complete(&self) {
        let reply = self.pending.borrow_mut().remove(0);
        reply(7);
    }
}
fn layout_fixture() -> (Rc<Presenter>, Rc<Host>, Rc<LayoutHost>) {
    let (p, host, manager) = fixture();
    let source = Rc::new(LayoutHost::default());
    let mut data = snapshot();
    data.geometry.caret = None;
    data.source = Some(source.clone());
    p.present(manager, data, p.ticket()).unwrap();
    (p, host, source)
}
fn drain(p: &Presenter) { p.dispatch.borrow().as_ref().unwrap().drain_for_test(); }

#[test]
fn candidate_pending_layout_becomes_ready_without_losing_host_candidates() {
    let (p, host, source) = layout_fixture();
    p.layout_changed(None, TF_LC_CHANGE).unwrap();
    p.layout_changed(None, TF_LC_CHANGE).unwrap();
    drain(&p);
    assert_eq!(source.pending.borrow().len(), 1, "coalesce layout notices");
    source.complete(); // still TS_E_NOLAYOUT; retain candidates, never guess a caret.
    drain(&p);
    assert!(p.active());
    let list = host.retained.borrow().clone().unwrap();
    unsafe { assert_eq!(list.GetCount().unwrap(), 2); assert!(!list.IsShown().unwrap().as_bool()); }
    source.geometry.set(Some(RECT { left: -800, top: 100, right: -800, bottom: 120 }));
    p.layout_changed(None, TF_LC_CHANGE).unwrap(); drain(&p); source.complete(); drain(&p);
    assert!(p.active());
    assert!(p.active.borrow().as_ref().unwrap().element.snapshot.borrow().as_ref().unwrap().geometry.caret.is_some());
    assert_eq!(*host.events.borrow(), ["begin", "update"]);
}

#[test]
fn candidate_read_completion_cannot_resurrect_after_focus_loss_or_clear_new_word() {
    let (p, host, source) = layout_fixture();
    p.layout_changed(None, TF_LC_CHANGE).unwrap(); drain(&p);
    let old = host.retained.borrow().clone().unwrap();
    p.clear();
    let manager: ITfUIElementMgr = Manager(host.clone()).into();
    p.present(manager, snapshot(), p.ticket()).unwrap();
    source.invalid.set(true); source.complete(); drain(&p);
    assert!(p.active(), "stale error cannot retire the new word");
    unsafe { assert_eq!(old.GetCount().unwrap(), 0); }
}

#[test]
fn candidate_unchanged_edit_keeps_preview_but_invalid_anchor_retires_it() {
    let (p, host, source) = layout_fixture();
    p.edited(None, 7).unwrap(); drain(&p);
    assert!(p.active(), "proofing or formatting outside our text must not erase candidates");
    source.invalid.set(true); // production source checks field scope, focus, selection and owned text.
    p.edited(None, 7).unwrap();
    assert!(!p.active());
    unsafe { assert_eq!(host.retained.borrow().as_ref().unwrap().GetCount().unwrap(), 0); }
}

#[test]
fn candidate_destroy_and_changed_layout_discard_queued_geometry() {
    let (p, _, source) = layout_fixture();
    p.layout_changed(None, TF_LC_CHANGE).unwrap(); drain(&p);
    p.layout_changed(None, TF_LC_CHANGE).unwrap();
    source.complete(); drain(&p);
    assert_eq!(source.pending.borrow().len(), 1, "stale geometry needs a fresh read");
    p.layout_changed(None, TF_LC_DESTROY).unwrap();
    source.complete(); drain(&p);
    assert!(!p.active());
}

#[test]
fn candidate_extent_errors_suppress_popup_without_discarding_valid_host_data() {
    let rect = RECT { left: 10, top: 10, right: 10, bottom: 30 };
    assert!(layout_extent(Err(TS_E_NOLAYOUT.into()), rect, false).unwrap().is_none());
    assert!(layout_extent(Ok(()), rect, true).unwrap().is_none());
    assert!(layout_extent(Ok(()), RECT::default(), false).unwrap().is_none());
    assert!(layout_extent(Ok(()), rect, false).unwrap().is_some());
    assert!(layout_extent(Err(E_FAIL.into()), rect, false).unwrap().is_none());
}

#[test]
fn candidate_initial_registration_retries_layout_and_old_observer_cannot_end_new_word() {
    let (p, host, source) = layout_fixture();
    drain(&p); assert_eq!(source.pending.borrow().len(), 1);
    let observer = Observer::new(&p);
    let sink: ITfTextLayoutSink = observer.to_interface();
    p.clear();
    let manager: ITfUIElementMgr = Manager(host.clone()).into();
    p.present(manager, snapshot(), p.ticket()).unwrap();
    unsafe { sink.OnLayoutChange(None, TF_LC_DESTROY, None).unwrap(); }
    assert!(p.active());
    source.complete(); drain(&p);
    assert!(p.active());
}

#[test]
fn candidate_old_pending_read_does_not_block_new_word_or_retain_its_source() {
    let (p, host, old) = layout_fixture(); drain(&p);
    p.clear();
    let next = Rc::new(LayoutHost::default());
    let mut data = snapshot(); data.geometry.caret = None; data.source = Some(next.clone());
    p.present(Manager(host).into(), data, p.ticket()).unwrap(); drain(&p);
    assert_eq!(next.pending.borrow().len(), 1, "old asynchronous read must not freeze this word");
    let samples = old.samples.get();
    old.complete();
    assert_eq!(old.samples.get(), samples, "retired callbacks must not read old document text");
    assert!(p.reading.get(), "old completion must not alter new reading state");
    next.complete(); drain(&p); assert!(p.active());
}

#[test]
fn candidate_pending_callback_does_not_keep_retired_anchor_alive() {
    let (p, _, source) = layout_fixture(); drain(&p);
    let reply = source.pending.borrow_mut().pop().unwrap();
    let weak = Rc::downgrade(&source);
    p.clear(); drop(source);
    assert!(weak.upgrade().is_none(), "pending read keeps only a Weak anchor");
    reply(7); drain(&p); assert!(!p.active());
}

#[test]
fn candidate_abandoned_read_retires_instead_of_waiting_forever() {
    let (p, _, source) = layout_fixture(); drain(&p);
    let reply = source.pending.borrow_mut().pop().unwrap(); drop(reply);
    assert!(!p.active(), "a released uncalled session is cancellation, not a pending success");
}

#[test]
fn candidate_edit_with_missing_geometry_requests_a_fresh_read() {
    let (p, _, source) = layout_fixture(); drain(&p); source.complete(); drain(&p);
    assert!(source.pending.borrow().is_empty());
    p.edited(None, 7).unwrap(); drain(&p);
    assert_eq!(source.pending.borrow().len(), 1, "do not rely on another layout notice after OnEndEdit");
}

#[test]
fn candidate_layout_only_notices_recover_a_removed_wake_message() {
    let (p, _, source) = layout_fixture();
    p.dispatch.borrow().as_ref().unwrap().discard_messages_for_test();
    p.layout_changed(None, TF_LC_CHANGE).unwrap(); drain(&p);
    assert_eq!(source.pending.borrow().len(), 1, "layout-only notices must re-wake an already queued read");
}

#[test]
fn candidate_failed_post_when_hold_ends_retires_pending_preview() {
    let (p, _, source) = layout_fixture(); drain(&p); source.complete(); drain(&p);
    p.dispatch.borrow().as_ref().unwrap().destroy_for_test();
    p.edited(None, 7).unwrap();
    assert!(!p.active(), "deferred post failure must not leave queued=true forever");
}
