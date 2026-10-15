//! The TSF text service: one per application thread that uses the input method. It forwards key events to the shell,
//! applies the shell's operations in edit sessions, keeps the composition, follows focus, and mirrors the mode on the
//! system indicator (가/A) through the conversion-mode compartment (W-07).

use std::cell::{Cell, RefCell};
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use windows::core::{implement, IUnknownImpl, Interface, Ref, Result, GUID};
use windows::Win32::Foundation::{BOOL, E_FAIL, E_INVALIDARG, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::TextServices::*;

use hanautomata_ffi::{Engine, InputMode};

use crate::display::{Attributes, InputAttribute};
use crate::edit;
use crate::guids::{DISPLAY_ATTRIBUTE_INPUT, PRESERVED_RECONVERT};
use crate::shell::{KeyEvent, Op, Outcome, Shell};
use crate::system;

#[cfg(test)]
#[path = "service_tests.rs"]
mod edit_failure_tests;

#[path = "service_learning.rs"]
mod learning;

struct CheckedKey {
    context: ITfContext,
    wparam: usize,
    lparam: isize,
    down: bool,
    observed: u64,
    access: edit::FieldAccess,
    tray: bool,
    eligible: Option<bool>,
}

/// A finalization owns the old range until EndComposition succeeds. Async callbacks may run after host edits or
/// after a newer synchronous session, so they must check both completion and the exact text we last wrote.
struct PendingFinish {
    composition: ITfComposition,
    context: Option<ITfContext>,
    expected: RefCell<String>,
    replacement: RefCell<Option<String>>,
    queued: Cell<bool>,
    done: Cell<bool>,
    failures: Cell<u8>,
    learning: RefCell<Option<learning::Confirmation>>,
}

/// Deactivation may be refused a final write lock. In all cases, queued callbacks must stop editing on return.
struct FinishRetirement(Vec<Rc<PendingFinish>>);
impl Drop for FinishRetirement {
    fn drop(&mut self) {
        for finish in &self.0 { finish.done.set(true); }
    }
}

impl PendingFinish {
    fn finish(&self, ec: u32) -> Result<()> {
        if self.done.get() { return Ok(()); }
        self.update_text(ec)?;
        unsafe { self.composition.EndComposition(ec)?; }
        self.done.set(true);
        Ok(())
    }

    fn failed(&self, error: &windows::core::Error) {
        self.failures.set(self.failures.get().saturating_add(1));
        let terminal = matches!(error.code(), TF_E_DISCONNECTED | windows::Win32::Foundation::E_UNEXPECTED);
        if terminal || self.failures.get() >= 3 { self.done.set(true); }
        system::log(&format!("composition finish unavailable: {:?}; attempts={} retired={}", error.code(), self.failures.get(), self.done.get()));
    }

    fn update_text(&self, ec: u32) -> Result<()> {
        let range = unsafe { self.composition.GetRange()? };
        let expected = self.expected.borrow().clone();
        let unchanged = edit::selection_text(ec, &range, expected.encode_utf16().count())?.as_deref() == Some(&expected);
        if unchanged {
            let replacement = self.replacement.borrow_mut().take();
            if let Some(text) = replacement {
                if let Err(error) = unsafe { range.SetText(ec, 0, &edit::utf16(&text)) } {
                    *self.replacement.borrow_mut() = Some(text);
                    return Err(error);
                }
                *self.expected.borrow_mut() = text;
                self.confirm_learning();
            }
        } else {
            // Host typing/AutoCorrect wins. Never move the caret back to this stale range.
            self.replacement.borrow_mut().take();
            self.learning.borrow_mut().take();
            system::log("composition finalization: host change preserved");
        }
        if let Ok(context) = unsafe { range.GetContext() } { let _ = edit::set_attribute(ec, &context, &range, 0); }
        Ok(())
    }

    fn confirm_learning(&self) {
        if let Some(confirmation) = self.learning.borrow_mut().take() { confirmation.confirm(); }
    }
}

const RECONVERSION_KEY: TF_PRESERVEDKEY = TF_PRESERVEDKEY {
    uVKey: crate::shell::vk::F2 as u32,
    uModifiers: 0,
};

static NEXT_SERVICE_ID: AtomicU64 = AtomicU64::new(1);

/// Each text-service instance owns its composition session. A process can host multiple services: sharing an Engine
/// makes one service's activation/reset discard another service's in-progress word (observed in Word during D2).
/// The legacy dictionary is read-only. An asynchronous companion supplies the separate TSF dictionary after startup.
fn engine(home: String, store: std::path::PathBuf) -> Arc<Engine> {
    Engine::new(home, store.to_string_lossy().into_owned())
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::shell::{vk, Mods};

    #[test]
    fn thread_focus_loss_invalidates_candidate_generation() {
        let service = windows::core::ComObject::new(TextService::new());
        let tip: ITfTextInputProcessorEx = service.to_interface();
        let focus = tip.cast::<ITfThreadFocusSink>().expect("thread focus must be observable");
        let before = service.candidates.ticket();
        unsafe { focus.OnKillThreadFocus().unwrap(); }
        assert_ne!(service.candidates.ticket(), before);
        unsafe { focus.OnSetThreadFocus().unwrap(); }
        assert!(!service.candidates.active());
    }

    #[test]
    fn foreign_final_release_does_not_destroy_apartment_owned_candidate_state() {
        let service = windows::core::ComObject::new(TextService::new());
        let candidates = Rc::clone(&service.candidates);
        let tip: ITfTextInputProcessorEx = service.to_interface();
        drop(service);
        let raw = Interface::into_raw(tip) as usize;
        std::thread::spawn(move || unsafe {
            drop(ITfTextInputProcessorEx::from_raw(raw as *mut _));
        }).join().unwrap();
        assert_eq!(Rc::strong_count(&candidates), 2, "foreign final Release must retain rather than free owner-only resources");
        // Intentional process-lifetime retention is the fallback for windows-implement's false agility.
        // Normal owner-thread destruction still releases its resources.
        let service = windows::core::ComObject::new(TextService::new());
        let normal = Rc::clone(&service.candidates);
        drop(service); assert_eq!(Rc::strong_count(&normal), 1);
    }

    #[test]
    fn another_service_activation_does_not_discard_an_in_progress_word() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let store = std::env::temp_dir().join(format!("hanautomata-service-isolation-{nonce}.json"));
        let mut first = Shell::new(engine(String::new(), store.clone()));
        first.reset(false);
        first.key_down(KeyEvent::new(b'D' as u16, Mods::NONE));

        // Word can activate another service while its document already has a composition.
        let mut second = Shell::new(engine(String::new(), store.clone()));
        second.reset(false);

        let mut committed = String::new();
        for key in [b'K' as u16, b'S' as u16, b'S' as u16, b'U' as u16, b'D' as u16, vk::SPACE] {
            for op in first.key_down(KeyEvent::new(key, Mods::NONE)).ops {
                if let Op::Commit(text) = op {
                    committed.push_str(&text);
                }
            }
        }
        assert_eq!(committed, "안녕 ", "A second service must not cancel the first service's leading d");
        assert!(!store.exists(), "S0 composition must not persist learning");
    }
}

#[derive(Default)]
struct State {
    thread_mgr: Option<ITfThreadMgr>,
    client: u32,
    thread_sink_cookie: u32,
    thread_focus_cookie: Option<u32>,
    compartment_sink_cookie: u32,
    reconversion_key_registered: bool,
    composition: Option<ITfComposition>,
    composition_text: String,
    write_serial: u64,
    pending_finishes: Vec<Rc<PendingFinish>>,
    learning: learning::Link,
    word_learning: Option<learning::Stamp>,
    learning_runtime: Option<crate::learning_runtime::Runtime>,
    learning_ready: Option<crate::learning_runtime::Ready>,
    shell: Option<Shell>,
    attribute_atom: u32,
    /// Password/private scope or a failed field query. Never trace these contexts.
    password_field: bool,
    tray_running: bool,
    excluded_app: bool,
    /// Process-wide privacy cannot be lifted by moving from a private field to an ordinary field.
    host_private: bool,
    checked_key: Option<CheckedKey>,
    last_query_failure: Option<(ITfContext, edit::FieldQueryFailure)>,
    /// Our own composition sink, for StartComposition (released on Deactivate, which breaks the cycle).
    sink: Option<ITfCompositionSink>,
}

#[implement(
    ITfTextInputProcessorEx,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfThreadMgrEventSink,
    ITfThreadFocusSink,
    ITfDisplayAttributeProvider,
    ITfCompartmentEventSink,
    ITfFnConfigure
)]
pub struct TextService {
    thread: u32,
    state: ManuallyDrop<RefCell<State>>,
    candidates: ManuallyDrop<Rc<crate::candidate::Presenter>>,
    diagnostic_id: u64,
    diagnostics: Cell<bool>,
    /// We are writing the conversion compartment ourselves: ignore the change notice.
    writing_compartment: Cell<bool>,
}

impl TextService {
    /// A bounded mailbox poll. No IPC, file access, process creation, waiting, or COM calls on the key path.
    /// Install worker-prepared dictionaries at clean word boundaries. Older words retain their original stamps.
    #[cfg(test)]
    fn poll_learning(&self) { self.poll_learning_for(None); }
    fn poll_learning_for(&self, context: Option<&ITfContext>) {
        let mut st = self.state.borrow_mut();
        let waiting = st.learning_ready.is_some();
        let Some(runtime) = st.learning_runtime.as_mut() else { return; };
        let status = runtime.status();
        let arrived = if waiting { None } else { runtime.try_ready() };
        st.learning.storage_status(status, None);
        if status.phase == crate::learning_runtime::Phase::Failed {
            st.learning.disconnect(); return;
        }
        if let Some(ready) = arrived {
            // Reconnecting the same queue would invalidate live word stamps and pause its producer.
            if !st.learning.report().connected { st.learning.connect(ready.producer.clone()); }
            st.learning_ready = Some(ready);
        }
        let Some(engine) = st.shell.as_ref().map(|shell| shell.engine().clone()) else { return; };
        // An old context may retain a deferred document callback. Only its text remains pending; it cannot
        // block a clean new context forever. Snapshot installation invalidates its old unconfirmed learning.
        if st.word_learning.is_some() || st.pending_finishes.iter().any(|f| !f.done.get()
                && context.is_none_or(|current| f.context.as_ref() == Some(current)))
            || st.shell.as_ref().is_none_or(|shell| shell.is_composing() || shell.private_context()) { return; }
        // Historical rejection is evidence, not a permanent gate. Retry only unaccepted original actions;
        // the checkpoint prevents replacing choices while their newly accepted writes are still in flight.
        st.learning.retry();
        let Some(mut ready) = st.learning_ready.take() else { return; };
        let version = engine.learning_version();
        let local = st.learning.local_choices(&ready.snapshot);
        let installed = ready.install(|prepared| engine.try_install_learning_with_local(version, prepared, &local));
        if matches!(installed, Ok(hanautomata_core::SnapshotInstall::Busy) | Err(hanautomata_core::learning::client::QueueError::Busy)) {
            st.learning_ready = Some(ready); return;
        }
        if let Ok(installed) = installed {
            if installed == hanautomata_core::SnapshotInstall::Applied {
                st.learning.snapshot_applied(&ready.snapshot, ready.origin.take()); st.learning.retry();
            }
            st.learning.storage_status(status, Some(installed == hanautomata_core::SnapshotInstall::Applied));
        }
        if let Some(runtime) = &st.learning_runtime { runtime.recycle(ready); }
    }

    pub fn new() -> TextService {
        TextService {
            thread: unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
            state: ManuallyDrop::new(RefCell::new(State::default())),
            candidates: ManuallyDrop::new(Rc::default()),
            diagnostic_id: NEXT_SERVICE_ID.fetch_add(1, Ordering::Relaxed),
            diagnostics: Cell::new(false),
            writing_compartment: Cell::new(false),
        }
    }

    /// Opt-in Word diagnosis: state flags only, never key codes, text, document names or ranges.
    fn trace(&self, event: &'static str) {
        if !self.diagnostics.get() {
            return;
        }
        let Ok(st) = self.state.try_borrow() else { return };
        let Some(shell) = st.shell.as_ref() else { return };
        if st.password_field || st.excluded_app {
            return;
        }
        system::log(&format!(
            "d2 service={} event={} client={} mode={:?} composing={} composition={} fresh={} pass={} paused={} preserved={}",
            self.diagnostic_id, event, st.client, shell.mode(), shell.is_composing(), st.composition.is_some(),
            shell.engine().reconversion().is_some(), shell.pass_through, shell.engine().paused(),
            st.reconversion_key_registered
        ));
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        // windows-implement 0.59 advertises agility even for this apartment-bound service. A foreign
        // final Release must not drop Rc/RefCell/COM/windows on that thread. Retain those resources
        // for process lifetime in this exceptional path; normal Deactivate/owner destruction releases them.
        // Removing false agility from the remaining multi-interface COM surfaces is a separate audit.
        if unsafe { windows::Win32::System::Threading::GetCurrentThreadId() } != self.thread { return; }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            ManuallyDrop::drop(&mut self.candidates);
            ManuallyDrop::drop(&mut self.state);
        }));
    }
}

fn key_event(wparam: WPARAM, lparam: LPARAM) -> KeyEvent {
    let flags = lparam.0 as u32;
    KeyEvent {
        vk: wparam.0 as u16,
        extended: flags & (1 << 24) != 0,
        mods: system::modifiers(),
        repeat: flags & (1 << 30) != 0,
        time: system::now_ms(),
    }
}

/// Never let a panic cross into the application (C-CMP-11): the key passes.
fn guarded<T>(fallback: T, f: impl FnOnce() -> Result<T>) -> Result<T> {
    guarded_result(Ok(fallback), f)
}

fn guarded_result<T>(fallback: Result<T>, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => {
            system::log("panic caught at text-service callback boundary");
            fallback
        }
    }
}

impl TextService_Impl {
    fn this(&self) -> windows::core::ComObject<TextService> {
        self.to_object()
    }

    /// Read-only field classification. Protected/unknown-field test probes never edit a document.
    fn probe_access(&self, context: &ITfContext) -> edit::FieldAccess {
        let client = self.state.borrow().client;
        let result = std::rc::Rc::new(Cell::new(edit::FieldProbe::failed(edit::FieldQueryStage::CallbackNotRun, E_FAIL)));
        let out = result.clone();
        let ctx = context.clone();
        let query = edit::request(context, client, TF_ES_SYNC | TF_ES_READ, move |ec| {
            out.set(edit::field_probe(ec, &ctx));
            Ok(())
        });
        let probe = match query {
            Ok(()) => result.get(),
            Err(error) => edit::FieldProbe::failed(edit::FieldQueryStage::EditSession, error.code()),
        };
        if let Some(failure) = probe.failure {
            let mut st = self.state.borrow_mut();
            if self.diagnostics.get() && st.last_query_failure.as_ref() != Some(&(context.clone(), failure)) {
                let policy = if probe.access == edit::FieldAccess::Private { "private composition; learning, preview and input trace disabled" }
                    else { "current key will pass without entering engine" };
                system::log(&format!("field query unavailable stage={:?} code={:?}; {}", failure.stage, failure.code, policy));
                st.last_query_failure = Some((context.clone(), failure));
            }
        } else {
            self.state.borrow_mut().last_query_failure = None;
        }
        probe.access
    }

    /// Control effects follow the shell even if its accompanying document commit failed.
    fn apply_effects(&self, out: &Outcome) {
        // Document edits are already durable. A panic in an indicator callback must never reach the outer
        // key guard, which would report not-eaten and replay that key in the host.
        let result = guarded_result(Err(E_FAIL.into()), || {
        if out.ops.iter().any(|op| matches!(op, Op::ToggleCapsLock)) {
            if let Some(shell) = self.state.borrow_mut().shell.as_mut() {
                shell.expect_injected_caps_lock();
            }
            if !system::send_caps_lock() {
                system::log("caps-lock restore failed");
            }
        }
        self.update_indicator();
        Ok(())
        });
        if result.is_err() {
            self.writing_compartment.set(false);
            system::log("control effect unavailable after key handling; document result retained");
        }
    }

    fn handle_key(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM, down: bool, test: bool) -> Result<BOOL> {
        if self.state.borrow().excluded_app { return Ok(false.into()); }
        let Some(context) = pic.as_ref() else {
            return Ok(false.into());
        };
        let e = key_event(wparam, lparam);
        let mut checked = if test {
            CheckedKey { context: context.clone(), wparam: wparam.0, lparam: lparam.0, down, observed: e.time,
                access: self.probe_access(context), tray: system::tray_running(), eligible: None }
        } else {
            let paired = self.state.borrow_mut().checked_key.take().filter(|key|
                key.context == *context && key.wparam == wparam.0 && key.lparam == lparam.0 && key.down == down
                && e.time.saturating_sub(key.observed) <= 100);
            paired.unwrap_or_else(|| CheckedKey { context: context.clone(), wparam: wparam.0, lparam: lparam.0, down,
                observed: e.time, access: self.probe_access(context), tray: system::tray_running(), eligible: None })
        };
        let pass = checked.access.blocks_keys() || checked.tray || self.state.borrow().excluded_app;
        // A read-only test probe may hide stale UI without mutating the engine or document.
        if checked.access != edit::FieldAccess::Ordinary || checked.tray { self.candidates.clear(); }
        self.state.borrow_mut().password_field = checked.access.suppresses_trace();
        let trace_key = down && (e.vk == crate::shell::vk::SPACE || e.vk == crate::shell::vk::F2);
        if trace_key {
            self.trace(if test { "test-key-before" } else { "key-before" });
        }
        if test {
            let mut st = self.state.borrow_mut();
            let Some(shell) = st.shell.as_mut() else {
                return Ok(false.into());
            };
            let wanted = shell.test_key_in_context(e, down, pass);
            let navigation = down && matches!(e.vk, crate::shell::vk::PRIOR..=crate::shell::vk::DOWN);
            let pending = st.pending_finishes.iter().any(|f| !f.done.get() && f.context.as_ref() == Some(context));
            // Request a real callback to end the old composition before passing an unknown/protected key.
            // Protected/unknown-field test probes never edit the document or engine.
            let cleanup = pass && down && !crate::shell::is_modifier(e.vk)
                && (st.composition.is_some() || pending);
            let wanted = wanted || cleanup || (!pass && navigation && pending);
            let finalize_navigation = wanted && !pass && navigation && (st.composition.is_some() || pending);
            drop(st);
            if finalize_navigation {
                // Some hosts route movement from TestKeyDown's answer, even when OnKeyDown later declines.
                // Like Microsoft's SampleIME, finalize a pass-through composition before declining here.
                // Eligibility above also updates modifier bookkeeping. An invalidated edit must keep its
                // claim: allowing host movement with a still-live composition could snap the next key back.
                // Navigation and shortcut outcomes are pass-through by the core contract; no key is injected.
                let _ = self.key_in_session(context, checked.access, checked.tray, move |shell| shell.key_down(e))?;
                let st = self.state.borrow();
                let finished = st.composition.is_none() && !st.pending_finishes.iter()
                    .any(|f| !f.done.get() && f.context.as_ref() == Some(context));
                drop(st);
                if finished {
                    checked.eligible = Some(false); // Do not process an optional paired OnKeyDown twice.
                    self.state.borrow_mut().checked_key = Some(checked);
                    return Ok(false.into());
                }
            }
            checked.eligible = Some(wanted);
            self.state.borrow_mut().checked_key = Some(checked);
            return Ok(wanted.into());
        }
        let wanted = checked.eligible.unwrap_or_else(|| {
            let mut st = self.state.borrow_mut();
            let wanted = st.shell.as_mut().is_some_and(|shell| shell.test_key_in_context(e, down, pass));
            wanted || (pass && down && !crate::shell::is_modifier(e.vk)
                && (st.composition.is_some() || st.pending_finishes.iter().any(|f| !f.done.get() && f.context.as_ref() == Some(context))))
        });
        if !wanted { return Ok(false.into()); }
        self.key_in_session(context, checked.access, checked.tray, move |shell| {
            if down { shell.key_down(e) } else { shell.key_up(e) }
        })
    }

    /// The engine must not accept a key until the application grants the write lock.
    fn key_in_session(&self, context: &ITfContext, access: edit::FieldAccess, tray: bool,
        key: impl FnOnce(&mut Shell) -> Outcome + 'static) -> Result<BOOL> {
        self.poll_learning_for(Some(context));
        // Capture before external callbacks/write-lock acquisition. An existing word keeps its older stamp;
        // this new stamp is for a new word or an F2 reconversion, never a re-stamp of delayed old learning.
        let incoming_learning = self.state.borrow().learning.capture();
        // EndUIElement is external code and may change focus. Retire the old preview BEFORE the engine
        // accepts this key or we take the write lock, then reject a target invalidated by that callback.
        let had_preview = self.candidates.active();
        self.candidates.clear();
        let preview_ticket = self.candidates.ticket();
        if had_preview {
            let tm = self.state.borrow().thread_mgr.clone();
            if !tm.as_ref().is_some_and(|tm| crate::candidate::focused(context, tm).unwrap_or(false)) {
                return Ok(false.into());
            }
        }
        let result = Rc::new(RefCell::new(None));
        let preview = Rc::new(RefCell::new(None));
        let preview_output = preview.clone();
        let output = result.clone();
        let materialized = Rc::new(Cell::new(false));
        let written_key = materialized.clone();
        let this = self.this();
        let ctx = context.clone();
        let client = self.state.borrow().client;
        let requested = edit::request(context, client, TF_ES_SYNC | TF_ES_READWRITE, move |ec| {
            if this.candidates.ticket() != preview_ticket { return Ok(()); }
            // A refused read-only probe is not proof of a field transition. The real write lock can provide a
            // valid scope now; if it still fails, keep the current key out of the engine and finish only old text.
            let access = if matches!(access, edit::FieldAccess::Unknown(_)) { edit::field_access(ec, &ctx) } else { access };
            this.drain_finishes(Some((&ctx, ec)))?;
            this.validate_composition(ec, &ctx)?;
            this.apply_context_policy(access, tray, Some((&ctx, ec)))?;
            if access.blocks_keys() || tray || this.state.borrow().excluded_app { return Ok(()); }
            let (stamp, next_word_stamp) = {
                let mut st = this.state.borrow_mut();
                if let Some(stamp) = st.word_learning.take() { (stamp, Some(incoming_learning)) }
                else if st.composition.is_some() { (st.learning.missing_origin(), Some(incoming_learning)) }
                else { (incoming_learning, None) }
            };
            let out = match this.state.borrow_mut().shell.as_mut() {
                Some(shell) => key(shell), None => return Ok(()),
            };
            *output.borrow_mut() = Some(out.clone());
            this.trace("key-after-shell");
            let mut batches = out.learning.clone();
            this.run_ops(ec, &ctx, out.ops.iter().filter(|op| !matches!(op, Op::ToggleCapsLock)).cloned().collect(), !out.eaten, &written_key, &mut batches)?;
            let has_composition = this.state.borrow().composition.is_some();
            let ended_word = out.ops.iter().any(|op| matches!(op, Op::Commit(_) | Op::ReplaceBefore { .. }));
            if has_composition && !ended_word && batches.is_empty() {
                this.state.borrow_mut().word_learning = Some(stamp);
            } else {
                this.confirm_learning(&batches, stamp);
                if has_composition {
                    let mut st = this.state.borrow_mut();
                    st.word_learning = Some(next_word_stamp.unwrap_or_else(|| st.learning.missing_origin()));
                }
            }
            if let Some(Some(data)) = &out.preview {
                let (tm, composition, learning_notice) = {
                    let st = this.state.borrow();
                    let mut report = st.learning.report();
                    // Sample counters after document confirmation, without I/O or waiting for any worker.
                    if let Some(runtime) = &st.learning_runtime { report.storage = Some(runtime.status()); }
                    let flags = st.shell.as_ref().and_then(|shell| shell.engine().try_learning_flags());
                    (st.thread_focus_cookie.and(st.thread_mgr.clone()), st.composition.clone(), report.notice(flags))
                };
                if let (Some(tm), Some(composition)) = (tm, composition) {
                    // A failed/unknown candidate query must never undo an already successful document edit.
                    if let Ok(snapshot) = crate::candidate::capture(ec, &ctx, &tm, client, &composition, data.clone(), learning_notice) {
                        if let Ok(manager) = tm.cast::<ITfUIElementMgr>() {
                            *preview_output.borrow_mut() = Some((manager, snapshot, preview_ticket));
                        }
                    }
                }
            }
            this.trace("key-after-document");
            *output.borrow_mut() = Some(out);
            Ok(())
        });
        if let Err(error) = requested {
            self.candidates.clear();
            let attempted = result.borrow_mut().take();
            let written = materialized.get();
            let eaten = attempted.as_ref().is_some_and(|out| out.eaten && (written || out.control_handled));
            if attempted.is_some() {
                // A failed SetText must not replay its speculative key through focus_lost. Keep exactly what is
                // already visible. If SetText succeeded but caret/end failed, consuming the key prevents duplication.
                if let Some(shell) = self.state.borrow_mut().shell.as_mut() {
                    shell.reset(system::caps_lock_on());
                    shell.engine().forget_reconversion();
                }
                if let Err(finish_error) = self.finish_original_composition(Outcome::default(), None) {
                    system::log(&format!("failed key cleanup retained: {:?}", finish_error.code()));
                }
            } else {
                self.commit_on_focus_change();
            }
            if let Some(out) = &attempted { self.apply_effects(out); }
            system::log(&format!("key edit unavailable: {:?}; visible_write={} eaten={}", error.code(), written, eaten));
            return Ok(eaten.into());
        }
        let out = result.borrow_mut().take().unwrap_or_default();
        if let Some((manager, snapshot, ticket)) = preview.borrow_mut().take() {
            // UI callbacks run outside the edit lock and without a borrow of the service state.
            if guarded_result(Err(E_FAIL.into()), || self.candidates.present(manager, snapshot, ticket)).is_err() {
                self.candidates.clear();
            }
        }
        // Only non-document effects remain; do not request a second document edit session.
        self.apply_effects(&out);
        let has_document_op = out.ops.iter().any(|op| !matches!(op, Op::ToggleCapsLock));
        Ok((out.eaten && (out.control_handled || !has_document_op || materialized.get())).into())
    }

    fn update_indicator(&self) {
        let (thread_mgr, client, mode) = {
            let st = self.state.borrow();
            let Some(tm) = st.thread_mgr.clone() else { return };
            let Some(shell) = st.shell.as_ref() else { return };
            (tm, st.client, if shell.pass_through { InputMode::English } else { shell.mode() })
        };
        let Ok(manager) = thread_mgr.cast::<ITfCompartmentMgr>() else {
            return;
        };
        self.writing_compartment.set(true);
        unsafe {
            if let Ok(open) = manager.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) {
                let _ = open.SetValue(client, &edit::variant_i4(1));
            }
            if let Ok(conversion) = manager.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION) {
                let value = if mode == InputMode::English {
                    TF_CONVERSIONMODE_ALPHANUMERIC
                } else {
                    TF_CONVERSIONMODE_NATIVE
                };
                let _ = conversion.SetValue(client, &edit::variant_i4(value as i32));
            }
        }
        self.writing_compartment.set(false);
    }

    /// Focus leaves the composition's context: commit the word there, asynchronously (it may not allow a sync session).
    fn commit_on_focus_change(&self) {
        self.candidates.clear();
        self.trace("focus-before");
        let out = {
            let mut st = self.state.borrow_mut();
            st.checked_key = None;
            let out = match st.shell.as_mut() {
                Some(shell) => {
                    let out = shell.focus_lost();
                    shell.engine().forget_reconversion();
                    out
                },
                None => return,
            };
            out
        };
        self.trace("focus-after-shell");
        if let Err(error) = self.finish_original_composition(out, None) {
            system::log(&format!("focus finalization deferred after error: {:?}", error.code()));
        }
    }

    fn context_event_affects_focus(&self, context: Ref<ITfContext>) -> bool {
        let Some(context) = context.as_ref() else { return false };
        let (composition, manager) = {
            let st = self.state.borrow();
            (st.composition.clone(), st.thread_mgr.clone())
        };
        unsafe {
            if composition.and_then(|c| c.GetRange().ok()).and_then(|r| r.GetContext().ok()).as_ref() == Some(context) {
                return true;
            }
            match (manager.and_then(|m| m.GetFocus().ok()), context.GetDocumentMgr().ok()) {
                (Some(focused), Some(owner)) => focused == owner,
                _ => false,
            }
        }
    }

}

impl TextService {
    fn apply_context_policy(&self, access: edit::FieldAccess, tray: bool, session: Option<(&ITfContext, u32)>) -> Result<()> {
        if access != edit::FieldAccess::Ordinary || tray { self.candidates.clear(); }
        let mut st = self.state.borrow_mut();
        st.password_field = access.suppresses_trace();
        st.tray_running = tray;
        let pass = access.blocks_keys() || tray || st.excluded_app;
        let host_private = st.host_private;
        let finish = st.shell.as_mut().and_then(|shell| {
            let private = host_private || if matches!(access, edit::FieldAccess::Unknown(_)) { shell.private_context() }
                else { access == edit::FieldAccess::Private };
            shell.context_policy(pass, private)
        });
        drop(st);
        if let Some(out) = finish { self.finish_original_composition(out, session)?; }
        Ok(())
    }

    fn validate_composition(&self, ec: u32, context: &ITfContext) -> Result<()> {
        let (composition, expected) = {
            let st = self.state.borrow(); (st.composition.clone(), st.composition_text.clone())
        };
        let Some(composition) = composition else { return Ok(()) };
        let range = unsafe { composition.GetRange()? };
        let same_context = unsafe { range.GetContext()? } == *context;
        if !same_context {
            let out = self.state.borrow_mut().shell.as_mut().map(|shell| {
                let out = shell.focus_lost(); shell.engine().forget_reconversion(); out
            }).unwrap_or_default();
            return self.finish_original_composition(out, Some((context, ec)));
        }
        let unchanged = edit::selection_text(ec, &range, expected.encode_utf16().count())?.as_deref() == Some(&expected);
        if !unchanged {
            // Never advance an old word over text inserted by the host after a refused or unclaimed key.
            if let Some(shell) = self.state.borrow_mut().shell.as_mut() {
                shell.reset(system::caps_lock_on());
                shell.engine().forget_reconversion();
            }
            self.finish_original_composition(Outcome::default(), Some((context, ec)))?;
        }
        Ok(())
    }

    /// Flush into the original range. Same-context key handling uses its existing synchronous lock; focus callbacks
    /// queue a guarded edit. Failed requests retain ownership and are retried before the next edit or focus callback.
    fn capture_finish(&self, out: Outcome) {
        let learning = {
            let mut st = self.state.borrow_mut();
            let stamp = st.word_learning.take().unwrap_or_else(|| st.learning.missing_origin());
            st.shell.as_ref().map(|shell| learning::Confirmation { engine: shell.engine().clone(), batches: out.learning, stamp })
        };
        let captured = {
            let mut st = self.state.borrow_mut();
            st.composition.take().map(|composition| {
                let expected = std::mem::take(&mut st.composition_text);
                (composition, expected)
            })
        };
        if let Some((composition, expected)) = captured {
            let replacement = out.ops.into_iter().find_map(|op| if let Op::Commit(text) = op { Some(text) } else { None });
            let context = unsafe { composition.GetRange().and_then(|range| range.GetContext()).ok() };
            self.state.borrow_mut().pending_finishes.push(Rc::new(PendingFinish { composition, context, expected: RefCell::new(expected),
                replacement: RefCell::new(replacement), queued: Cell::new(false), done: Cell::new(false), failures: Cell::new(0),
                learning: RefCell::new(learning) }));
        }
    }

    fn finish_original_composition(&self, out: Outcome, session: Option<(&ITfContext, u32)>) -> Result<()> {
        self.capture_finish(out);
        self.drain_finishes(session)
    }

    fn drain_finishes(&self, session: Option<(&ITfContext, u32)>) -> Result<()> {
        let (pending, client) = { let st = self.state.borrow(); (st.pending_finishes.clone(), st.client) };
        let mut current_error = None;
        for finish in pending {
            if finish.done.get() { continue; }
            let Some(context) = finish.context.as_ref() else {
                finish.done.set(true);
                system::log("composition owner unavailable; visible text preserved");
                continue;
            };
            if let Some((_, ec)) = session.filter(|(current, _)| *current == context) {
                if let Err(error) = finish.finish(ec) { finish.failed(&error); current_error = Some(error); }
            } else if !finish.queued.replace(true) {
                let deferred = finish.clone();
                let called = Rc::new(Cell::new(false));
                let ran = called.clone();
                let requested = edit::request(context, client, TF_ES_ASYNCDONTCARE | TF_ES_READWRITE, move |ec| {
                    ran.set(true);
                    let result = deferred.finish(ec);
                    deferred.queued.set(false);
                    if let Err(error) = &result { deferred.failed(error); }
                    result
                });
                if let Err(error) = requested {
                    finish.queued.set(false);
                    if !called.get() { finish.failed(&error); }
                    // A failure in another context must never abort the current key or skip other finishes.
                }
            }
        }
        self.state.borrow_mut().pending_finishes.retain(|finish| !finish.done.get());
        current_error.map_or(Ok(()), Err)
    }
}

impl TextService {
    /// Document success first, then memory confirmation and a nonblocking handoff of confirmed deltas only.
    /// Never save this service's whole dictionary over another application's newer state.
    fn confirm_learning(&self, batches: &[hanautomata_core::LearningBatch], stamp: learning::Stamp) {
        let engine = self.state.borrow().shell.as_ref().map(|shell| shell.engine().clone());
        if let Some(engine) = engine { learning::Confirmation { engine, batches: batches.to_vec(), stamp }.confirm(); }
    }

    /// Runs inside the edit session: the document operations of one key, in order.
    fn run_ops(&self, ec: u32, context: &ITfContext, ops: Vec<Op>, pass_to_host: bool, materialized: &Cell<bool>,
        learning: &mut Vec<hanautomata_core::LearningBatch>) -> Result<()> {
        // The final document operation represents this key; earlier operations only finish previous text. Count
        // its SetText even when later caret/end bookkeeping fails, but never count an earlier word's write.
        let last = ops.iter().rposition(|op| !matches!(op, Op::ToggleCapsLock));
        for (index, op) in ops.into_iter().enumerate() {
            let before = self.state.borrow().write_serial;
            let result = guarded_result(Err(E_FAIL.into()), || -> Result<()> {
            match op {
                Op::Commit(text) => self.commit(ec, context, &text, pass_to_host)?,
                Op::Compose(text) => self.compose(ec, context, &text)?,
                Op::ReplaceBefore { old, new } => {
                    if self.replace_before(ec, context, &old, &new)? {
                        let after = match self.state.borrow_mut().shell.as_mut() {
                            Some(shell) => shell.reconverted(),
                            None => Outcome::default(),
                        };
                        let rest: Vec<Op> = after.ops.clone();
                        self.run_ops(ec, context, rest, false, &Cell::new(false), learning)?;
                        learning.extend(after.learning);
                    } else {
                        if let Some(shell) = self.state.borrow().shell.as_ref() { shell.engine().forget_reconversion(); }
                        system::log("reconversion skipped: text before the caret changed");
                    }
                }
                Op::FlipSelection => self.flip_selection(ec, context)?,
                Op::ToggleCapsLock => {}
            }
            Ok(())
            });
            if Some(index) == last && self.state.borrow().write_serial != before { materialized.set(true); }
            result?;
        }
        Ok(())
    }

    fn commit(&self, ec: u32, context: &ITfContext, text: &str, pass_to_host: bool) -> Result<()> {
        let composition = self.state.borrow().composition.clone();
        match composition {
            Some(composition) => unsafe {
                let range = composition.GetRange()?;
                // Enter/Tab/navigation belong to the host after finalization. If their commit is already
                // visible, end the composition without rewriting it or resetting the host's selection.
                // Consumed keys keep their write accounting, and changed final text still follows SetText.
                if pass_to_host && self.state.borrow().composition_text == text &&
                    edit::caret_is_after(ec, context, &range).unwrap_or(false) &&
                    edit::selection_text(ec, &range, text.encode_utf16().count())?.as_deref() == Some(text) {
                    return self.finish_original_composition(Outcome::default(), Some((context, ec)));
                }
                range.SetText(ec, 0, &edit::utf16(text))?;
                {
                    let mut st = self.state.borrow_mut();
                    st.composition_text = text.to_owned();
                    st.write_serial = st.write_serial.wrapping_add(1);
                }
                let _ = edit::set_attribute(ec, context, &range, 0);
                edit::caret_after(ec, context, &range)?;
                self.finish_original_composition(Outcome::default(), Some((context, ec)))
            },
            None => unsafe {
                let insert: ITfInsertAtSelection = context.cast()?;
                let range = insert.InsertTextAtSelection(ec, INSERT_TEXT_AT_SELECTION_FLAGS(0), &edit::utf16(text))?;
                { let mut st = self.state.borrow_mut(); st.write_serial = st.write_serial.wrapping_add(1); }
                edit::caret_after(ec, context, &range)
            },
        }
    }

    fn compose(&self, ec: u32, context: &ITfContext, text: &str) -> Result<()> {
        let existing = self.state.borrow().composition.clone();
        if text.is_empty() {
            if let Some(composition) = existing {
                unsafe {
                    let range = composition.GetRange()?;
                    range.SetText(ec, 0, &[])?;
                    {
                        let mut st = self.state.borrow_mut();
                        st.composition_text.clear();
                        st.write_serial = st.write_serial.wrapping_add(1);
                    }
                    self.finish_original_composition(Outcome::default(), Some((context, ec)))?;
                }
            }
            return Ok(());
        }
        let composition = match existing {
            Some(c) => c,
            None => unsafe {
                let insert: ITfInsertAtSelection = context.cast()?;
                let at = insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])?;
                let contexts: ITfContextComposition = context.cast()?;
                let sink = self
                    .state
                    .borrow()
                    .sink
                    .clone()
                    .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
                let c = contexts.StartComposition(ec, &at, &sink)?;
                self.state.borrow_mut().composition = Some(c.clone());
                c
            },
        };
        let atom = self.state.borrow().attribute_atom;
        unsafe {
            let range = composition.GetRange()?;
            range.SetText(ec, 0, &edit::utf16(text))?;
            {
                let mut st = self.state.borrow_mut();
                st.composition_text = text.to_owned();
                st.write_serial = st.write_serial.wrapping_add(1);
            }
            let _ = edit::set_attribute(ec, context, &range, atom);
            if let Err(error) = edit::caret_after(ec, context, &range) {
                system::log(&format!("composition caret update unavailable; text retained in live range: {:?}", error.code()));
            }
            Ok(())
        }
    }

    fn replace_before(&self, ec: u32, context: &ITfContext, old: &str, new: &str) -> Result<bool> {
        let caret = edit::selection(ec, context)?;
        if unsafe { caret.IsEmpty(ec)?.as_bool() } {
            let range = unsafe { caret.Clone()? };
            let want = old.encode_utf16().count() as i32;
            let mut moved = 0i32;
            unsafe { range.ShiftStart(ec, -want, &mut moved, std::ptr::null())? };
            if moved == -want && edit::text(ec, &range, want as usize + 1)? == old {
                // Space belongs to the already committed document. Replacing it again lets hosts run their
                // automatic word correction on the user's explicit F2 choice. Verify the complete old text,
                // then replace only the word and retain the original caret after the untouched separator.
                if let Some(word) = new.strip_suffix(' ').filter(|_| old.ends_with(' ')) {
                    let mut trimmed = 0;
                    unsafe { range.ShiftEnd(ec, -1, &mut trimmed, std::ptr::null())?; }
                    if trimmed != -1 { return Err(E_FAIL.into()); }
                    self.corrected_text(ec, context, &range, word)?;
                    edit::caret_after(ec, context, &caret)?;
                } else {
                    self.corrected_text(ec, context, &range, new)?;
                    edit::caret_after(ec, context, &range)?;
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn flip_selection(&self, ec: u32, context: &ITfContext) -> Result<()> {
        let range = edit::selection(ec, context)?;
        if unsafe { range.IsEmpty(ec)?.as_bool() } {
            system::log("flip skipped: nothing selected");
            return Ok(());
        }
        let text = match edit::selection_text(ec, &range, 4096) {
            Ok(Some(text)) => text,
            Ok(None) => {
                system::log("flip skipped: oversized or unsupported selection; original preserved");
                return Ok(());
            },
            Err(error) => {
                system::log(&format!("flip skipped: selection could not be read completely: {:?}", error.code()));
                return Ok(());
            },
        };
        let flipped = self.state.borrow().shell.as_ref().and_then(|s| s.flip(&text));
        if let Some(flipped) = flipped {
            // Word selections can include trailing spaces or tabs. Preserve unchanged whitespace
            // outside SetText so AutoCorrect does not undo the explicit selection choice.
            let suffix = &text[text.trim_end_matches(char::is_whitespace).len()..];
            if !suffix.is_empty() && flipped.ends_with(suffix) {
                let word_range = unsafe { range.Clone()? };
                let trailing_units = suffix.encode_utf16().count() as i32;
                let mut trimmed = 0;
                unsafe { word_range.ShiftEnd(ec, -trailing_units, &mut trimmed, std::ptr::null())?; }
                if trimmed != -trailing_units { return Err(E_FAIL.into()); }
                self.corrected_text(ec, context, &word_range, &flipped[..flipped.len() - suffix.len()])?;
            } else {
                self.corrected_text(ec, context, &range, &flipped)?;
            }
        }
        Ok(())
    }

    fn corrected_text(&self, ec: u32, context: &ITfContext, range: &ITfRange, text: &str) -> Result<()> {
        let language = match edit::prepare_correction_language(ec, context, range, text) {
            Ok(change) => change,
            Err(error) => {
                system::log(&format!("correction language property unavailable: {:?}", error.code()));
                None
            },
        };
        if let Err(error) = unsafe { range.SetText(ec, TF_ST_CORRECTION, &edit::utf16(text)) } {
            if let Some(change) = language {
                if let Err(restore) = change.restore(ec, range) {
                    system::log(&format!("refused correction language restore failed: {:?}", restore.code()));
                }
            }
            return Err(error);
        }
        { let mut st = self.state.borrow_mut(); st.write_serial = st.write_serial.wrapping_add(1); }
        if language.is_some() { self.trace("correction-language-set-before-text"); }
        Ok(())
    }
}

impl ITfFunction_Impl for TextService_Impl {
    fn GetDisplayName(&self) -> Result<windows::core::BSTR> {
        guarded_result(Err(E_FAIL.into()), || Ok(crate::configuration::DISPLAY_NAME.into()))
    }
}

impl ITfFnConfigure_Impl for TextService_Impl {
    fn Show(&self, parent: windows::Win32::Foundation::HWND, langid: u16, profile: *const GUID) -> Result<()> {
        // COM supplies a readable profile pointer. A null pointer is rejected before any UI or disk access.
        // No service state is touched during/after the nested pump. DllCanUnloadNow keeps the callback code mapped.
        guarded_result(Err(E_FAIL.into()), || crate::configuration::show(parent, langid, unsafe { profile.as_ref() }))
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        guarded((), || {
        self.candidates.clear();
        self.trace("deactivate");
        let out = self.state.borrow_mut().shell.as_mut().map(Shell::focus_lost).unwrap_or_default();
        self.capture_finish(out);
        let st = std::mem::take(&mut *self.state.borrow_mut());
        let retiring = FinishRetirement(st.pending_finishes.clone());
        for finish in &retiring.0 {
            if finish.done.get() { continue; }
            if let Some(context) = finish.context.as_ref() {
                let pending = finish.clone();
                // Try only synchronously. Teardown cannot leave an edit that may overwrite the next input method.
                if let Err(error) = edit::request(context, st.client, TF_ES_SYNC | TF_ES_READWRITE, move |ec| pending.finish(ec)) {
                    system::log(&format!("deactivation final write unavailable; visible text preserved: {:?}", error.code()));
                }
            }
        }
        drop(retiring);
        if let Some(tm) = st.thread_mgr.as_ref() {
            unsafe {
                if let Ok(keystrokes) = tm.cast::<ITfKeystrokeMgr>() {
                    if st.reconversion_key_registered {
                        if let Err(error) = keystrokes.UnpreserveKey(&PRESERVED_RECONVERT, &RECONVERSION_KEY) {
                            system::log(&format!("unpreserve F2 failed: {:?}", error.code()));
                        }
                    }
                    let _ = keystrokes.UnadviseKeyEventSink(st.client);
                }
                if let Ok(source) = tm.cast::<ITfSource>() {
                    let _ = source.UnadviseSink(st.thread_sink_cookie);
                    if let Some(cookie) = st.thread_focus_cookie { let _ = source.UnadviseSink(cookie); }
                }
                if let Ok(manager) = tm.cast::<ITfCompartmentMgr>() {
                    if let Ok(conversion) = manager.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION) {
                        if let Ok(source) = conversion.cast::<ITfSource>() {
                            let _ = source.UnadviseSink(st.compartment_sink_cookie);
                        }
                    }
                }
            }
        }
        system::log("deactivate");
        Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, _flags: u32) -> Result<()> {
        let result = guarded_result(Err(E_FAIL.into()), || {
        let host_private = system::requires_host_privacy();
        self.diagnostics.set(
            system::process_name().eq_ignore_ascii_case("WINWORD.EXE")
                && !host_private
                && system::data_dir().join("tsf-diagnostics.enabled").is_file()
        );
        let thread_mgr = ptim.ok()?.clone();
        let mut shell = Shell::new(engine(system::home_dir(), system::data_dir().join("words.json")));
        shell.reset(system::caps_lock_on());
        // Read once at activation. Never perform configuration I/O on the key callback path. Invalid/unreadable
        // configuration passes keys rather than silently turning on options the user might have disabled.
        let (options, unavailable) = match system::settings_dir()
            .and_then(|directory| crate::settings::Settings::load(&directory.join("tsf-settings.json"))) {
            Ok(options) => (options, false),
            Err(_) => { system::log("settings unavailable; passing keys until configuration is repaired");
                (crate::settings::Settings::default(), true) }
        };
        options.apply_new_session(&mut shell);
        let excluded = unavailable || options.excludes_process(system::process_identity().as_deref());
        let tray = system::tray_running();
        // Fresh shell: no document composition exists to finish. Host privacy survives later field transitions.
        let _ = shell.context_policy(tray || excluded, host_private);
        {
            let mut st = self.state.borrow_mut();
            st.thread_mgr = Some(thread_mgr.clone());
            st.client = tid;
            st.learning.disconnect();
            st.learning = learning::Link::default();
            st.learning_runtime = None;
            st.learning_ready = None;
            st.word_learning = None;
            st.shell = Some(shell);
            st.tray_running = tray;
            st.excluded_app = excluded;
            st.host_private = host_private;
            st.sink = Some(self.to_interface::<ITfCompositionSink>());
        }
        unsafe {
            let keystrokes: ITfKeystrokeMgr = thread_mgr.cast()?;
            keystrokes.AdviseKeyEventSink(tid, &self.to_interface::<ITfKeyEventSink>(), true)?;
            let source: ITfSource = thread_mgr.cast()?;
            let cookie = source.AdviseSink(&ITfThreadMgrEventSink::IID, &self.to_interface::<ITfThreadMgrEventSink>())?;
            self.state.borrow_mut().thread_sink_cookie = cookie;
            match source.AdviseSink(&ITfThreadFocusSink::IID, &self.to_interface::<ITfThreadFocusSink>()) {
                Ok(cookie) => self.state.borrow_mut().thread_focus_cookie = Some(cookie),
                Err(_) => system::log("candidate UI disabled: thread focus notifications unavailable"),
            }
            if let Ok(manager) = thread_mgr.cast::<ITfCompartmentMgr>() {
                if let Ok(conversion) = manager.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION) {
                    if let Ok(source) = conversion.cast::<ITfSource>() {
                        if let Ok(cookie) =
                            source.AdviseSink(&ITfCompartmentEventSink::IID, &self.to_interface::<ITfCompartmentEventSink>())
                        {
                            self.state.borrow_mut().compartment_sink_cookie = cookie;
                        }
                    }
                }
            }
            if let Ok(categories) = CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
                if let Ok(atom) = categories.RegisterGUID(&DISPLAY_ATTRIBUTE_INPUT) {
                    self.state.borrow_mut().attribute_atom = atom;
                }
            }
            // Word can process F2 before delivering an ordinary OnKeyDown. The preserved command returns false when
            // no word can be reconverted, so the application's own F2 remains available in that case.
            match keystrokes.PreserveKey(tid, &PRESERVED_RECONVERT, &RECONVERSION_KEY, &edit::utf16("Hanautomata reconvert")) {
                Ok(()) => self.state.borrow_mut().reconversion_key_registered = true,
                Err(error) => system::log(&format!("preserve F2 failed; ordinary key route retained: {:?}", error.code())),
            }
        }
        self.trace("activate-before-indicator");
        self.update_indicator();
        self.trace("activate-after-indicator");
        // Launch only after activation succeeded. I/O/process creation happen on the worker, never a key callback.
        // Restricted/elevated hosts retain in-memory input; the worker's exact token check refuses persistence.
        if !host_private && !excluded && !tray && options.learn_choices && system::desktop_settings_allowed()
            && !crate::learning_runtime::spawn_denied() && crate::exports::pin_for_worker().is_ok() {
            if let (Ok(module), Ok(directory)) = (crate::exports::module_path(), system::settings_dir()) {
                let legacy = self.state.borrow().shell.as_ref().unwrap().engine().learning_entries();
                match crate::learning_runtime::Runtime::start_with_legacy(module.with_file_name("hanautomata-ime-setup.exe"), directory.join("tsf-learning.db"), legacy) {
                    Ok(runtime) => self.state.borrow_mut().learning_runtime = Some(runtime),
                    Err(_) => system::log("learning worker unavailable; input remains in memory"),
                }
            }
        }
        system::log(&format!(
            "activate host_private={} appcontainer={} tray={} caps={}",
            host_private,
            system::app_container(),
            tray,
            system::caps_lock_on()
        ));
        Ok(())
        });
        if result.is_err() { let _ = self.Deactivate(); }
        result
    }
}

impl ITfThreadFocusSink_Impl for TextService_Impl {
    fn OnSetThreadFocus(&self) -> Result<()> {
        if unsafe { windows::Win32::System::Threading::GetCurrentThreadId() } != self.thread {
            return Err(windows::Win32::Foundation::RPC_E_WRONG_THREAD.into());
        }
        Ok(())
    }
    fn OnKillThreadFocus(&self) -> Result<()> {
        if unsafe { windows::Win32::System::Threading::GetCurrentThreadId() } != self.thread {
            return Err(windows::Win32::Foundation::RPC_E_WRONG_THREAD.into());
        }
        let _keep_alive = self.this();
        guarded((), || { self.candidates.clear(); Ok(()) })
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        guarded((), || {
            if !fforeground.as_bool() { self.commit_on_focus_change(); }
            Ok(())
        })
    }

    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(false.into(), || self.handle_key(pic, wparam, lparam, true, true))
    }

    fn OnTestKeyUp(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(false.into(), || self.handle_key(pic, wparam, lparam, false, true))
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(false.into(), || self.handle_key(pic, wparam, lparam, true, false))
    }

    fn OnKeyUp(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(false.into(), || self.handle_key(pic, wparam, lparam, false, false))
    }

    fn OnPreservedKey(&self, pic: Ref<ITfContext>, rguid: *const GUID) -> Result<BOOL> {
        guarded(false.into(), || {
            if self.state.borrow().excluded_app { return Ok(false.into()); }
            if rguid.is_null() || unsafe { *rguid } != PRESERVED_RECONVERT {
                return Ok(false.into());
            }
            let Some(context) = pic.as_ref() else { return Ok(false.into()) };
            // A preserved command need not be preceded by OnTestKeyDown. Recheck password scope and tray coexistence.
            let access = self.probe_access(context);
            let tray = system::tray_running();
            {
                let mut st = self.state.borrow_mut();
                st.checked_key = None;
                st.password_field = access.suppresses_trace();
            }
            self.trace("preserved-before");
            let eaten = self.key_in_session(context, access, tray, |shell| shell.reconversion_command())?;
            self.trace("preserved-after");
            system::log(&format!("preserved F2 handled={}", eaten.as_bool()));
            Ok(eaten)
        })
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(&self, ecwrite: u32, pcomposition: Ref<ITfComposition>) -> Result<()> {
        guarded((), || {
        let pending = self.state.borrow().pending_finishes.iter()
            .find(|finish| Some(&finish.composition) == pcomposition.as_ref()).cloned();
        if let Some(finish) = pending {
            // TSF has already ended this composition. Retire the queued callback and apply a guarded replacement
            // using the host-provided lock; never call EndComposition again or touch a new engine session.
            if !finish.done.replace(true) {
                if let Err(error) = finish.update_text(ecwrite) {
                    system::log(&format!("terminated composition preserved after final write failure: {:?}", error.code()));
                }
            }
            return Ok(());
        }
        // A detached composition can finish asynchronously after a new field has acquired focus.
        // Its callback must not clear or commit that field's new session.
        if self.state.borrow().composition.as_ref() != pcomposition.as_ref() || pcomposition.is_none() {
            return Ok(());
        }
        self.candidates.clear();
        self.trace("terminated-before");
        let (expected, stamp) = {
            let mut st = self.state.borrow_mut();
            st.composition = None;
            let stamp = st.word_learning.take().unwrap_or_else(|| st.learning.missing_origin());
            (std::mem::take(&mut st.composition_text), stamp)
        };
        let range = pcomposition.as_ref().and_then(|c| unsafe { c.GetRange().ok() });
        let unchanged = range.as_ref().is_some_and(|range| {
            edit::selection_text(ecwrite, range, expected.encode_utf16().count()).ok().flatten().as_deref() == Some(&expected)
        });
        let out = self.state.borrow_mut().shell.as_mut().and_then(|shell| {
            if unchanged { shell.composition_terminated() }
            else { shell.reset(system::caps_lock_on()); shell.engine().forget_reconversion(); None }
        });
        self.trace("terminated-after");
        if let (Some(out), Some(range)) = (out, range) {
            let Some(text) = out.ops.iter().find_map(|op| if let Op::Commit(text) = op { Some(text) } else { None }) else { return Ok(()) };
            unsafe {
                range.SetText(ecwrite, 0, &edit::utf16(&text))?;
                self.confirm_learning(&out.learning, stamp);
                if let Ok(context) = range.GetContext() {
                    let _ = edit::set_attribute(ecwrite, &context, &range, 0);
                }
            }
        }
        Ok(())
        })
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        guarded((), || { self.candidates.clear(); Ok(()) })
    }

    fn OnSetFocus(&self, pdimfocus: Ref<ITfDocumentMgr>, pdimprevfocus: Ref<ITfDocumentMgr>) -> Result<()> {
        guarded((), || {
        if pdimfocus.as_ref() == pdimprevfocus.as_ref() { return Ok(()); }
        self.commit_on_focus_change();
        let mut st = self.state.borrow_mut();
        st.tray_running = system::tray_running();
        drop(st);
        self.update_indicator();
        Ok(())
        })
    }

    fn OnPushContext(&self, pic: Ref<ITfContext>) -> Result<()> {
        guarded((), || {
        if self.context_event_affects_focus(pic) { self.commit_on_focus_change(); }
        Ok(())
        })
    }

    fn OnPopContext(&self, pic: Ref<ITfContext>) -> Result<()> {
        guarded((), || {
        if self.context_event_affects_focus(pic) { self.commit_on_focus_change(); }
        Ok(())
        })
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(Attributes::new().into())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if guid.is_null() {
            return Err(E_INVALIDARG.into());
        }
        if unsafe { *guid } == DISPLAY_ATTRIBUTE_INPUT {
            Ok(InputAttribute.into())
        } else {
            Err(E_INVALIDARG.into())
        }
    }
}

impl ITfCompartmentEventSink_Impl for TextService_Impl {
    /// The user clicked the system indicator (가/A): follow it. A, English; 가, automatic unless Korean was fixed.
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        guarded((), || {
        if self.writing_compartment.get() || rguid.is_null() || self.state.borrow().excluded_app {
            return Ok(());
        }
        if unsafe { *rguid } != GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION {
            return Ok(());
        }
        self.trace("indicator-before");
        let thread_mgr = self.state.borrow().thread_mgr.clone();
        let Some(thread_mgr) = thread_mgr else { return Ok(()) };
        let value = unsafe {
            let manager: ITfCompartmentMgr = thread_mgr.cast()?;
            let conversion = manager.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION)?;
            edit::variant_as_i32(&conversion.GetValue()?).unwrap_or(0)
        };
        let native = value as u32 & TF_CONVERSIONMODE_NATIVE != 0;
        let mut st = self.state.borrow_mut();
        let Some(shell) = st.shell.as_mut() else { return Ok(()) };
        if shell.is_composing() {
            // A click while composing: the composition ends through OnCompositionTerminated or the next key.
            return Ok(());
        }
        let target = match (native, shell.mode()) {
            (false, _) => InputMode::English,
            (true, InputMode::English) => InputMode::Auto,
            (true, mode) => mode,
        };
        if target != shell.mode() {
            let _ = shell.set_mode(target);
            system::log(&format!("indicator mode {:?}", target));
        }
        drop(st);
        self.trace("indicator-after");
        Ok(())
        })
    }
}
