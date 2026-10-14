//! The TSF text service: one per application thread that uses the input method. It forwards key events to the shell,
//! applies the shell's operations in edit sessions, keeps the composition, follows focus, and mirrors the mode on the
//! system indicator (가/A) through the conversion-mode compartment (W-07).

use std::cell::{Cell, RefCell};
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

struct CheckedKey {
    context: ITfContext,
    wparam: usize,
    lparam: isize,
    down: bool,
    observed: u64,
    access: edit::FieldAccess,
    tray: bool,
}

/// A finalization owns the old range until EndComposition succeeds. Async callbacks may run after host edits or
/// after a newer synchronous session, so they must check both completion and the exact text we last wrote.
struct PendingFinish {
    composition: ITfComposition,
    expected: RefCell<String>,
    replacement: RefCell<Option<String>>,
    queued: Cell<bool>,
    done: Cell<bool>,
}

impl PendingFinish {
    fn context(&self) -> Result<ITfContext> { unsafe { self.composition.GetRange()?.GetContext() } }

    fn finish(&self, ec: u32) -> Result<()> {
        if self.done.get() { return Ok(()); }
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
            }
        } else {
            // Host typing/AutoCorrect wins. Never move the caret back to this stale range.
            self.replacement.borrow_mut().take();
            system::log("composition finalization: host change preserved");
        }
        if let Ok(context) = unsafe { range.GetContext() } { let _ = edit::set_attribute(ec, &context, &range, 0); }
        unsafe { self.composition.EndComposition(ec)?; }
        self.done.set(true);
        Ok(())
    }
}

const RECONVERSION_KEY: TF_PRESERVEDKEY = TF_PRESERVEDKEY {
    uVKey: crate::shell::vk::F2 as u32,
    uModifiers: 0,
};

static NEXT_SERVICE_ID: AtomicU64 = AtomicU64::new(1);

/// Each text-service instance owns its composition session. A process can host multiple services: sharing an Engine
/// makes one service's activation/reset discard another service's in-progress word (observed in Word during D2).
/// S0 reads the same learning file but does not save; shared learning needs a separate design after S0.
fn engine(home: String, store: std::path::PathBuf) -> Arc<Engine> {
    Engine::new(home, store.to_string_lossy().into_owned())
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::shell::{vk, Mods};

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
    compartment_sink_cookie: u32,
    reconversion_key_registered: bool,
    composition: Option<ITfComposition>,
    composition_text: String,
    write_serial: u64,
    pending_finishes: Vec<Rc<PendingFinish>>,
    shell: Option<Shell>,
    attribute_atom: u32,
    /// Password/private scope or a failed field query. Never trace these contexts.
    password_field: bool,
    tray_running: bool,
    checked_key: Option<CheckedKey>,
    last_query_failure: Option<(ITfContext, windows::core::HRESULT)>,
    /// Our own composition sink, for StartComposition (released on Deactivate, which breaks the cycle).
    sink: Option<ITfCompositionSink>,
}

#[implement(
    ITfTextInputProcessorEx,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfThreadMgrEventSink,
    ITfDisplayAttributeProvider,
    ITfCompartmentEventSink
)]
pub struct TextService {
    state: RefCell<State>,
    diagnostic_id: u64,
    diagnostics: Cell<bool>,
    /// We are writing the conversion compartment ourselves: ignore the change notice.
    writing_compartment: Cell<bool>,
}

impl TextService {
    pub fn new() -> TextService {
        TextService {
            state: RefCell::new(State::default()),
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
        if st.password_field {
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
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => {
            system::log("panic caught in key handling; key passed");
            Ok(fallback)
        }
    }
}

impl TextService_Impl {
    fn this(&self) -> windows::core::ComObject<TextService> {
        self.to_object()
    }

    /// A read-only probe. In particular, OnTestKeyDown must not flush/reset the engine or end a composition.
    fn probe_access(&self, context: &ITfContext) -> edit::FieldAccess {
        let client = self.state.borrow().client;
        let result = std::rc::Rc::new(Cell::new(edit::FieldAccess::Unknown(E_FAIL)));
        let out = result.clone();
        let ctx = context.clone();
        let query = edit::request(context, client, TF_ES_SYNC | TF_ES_READ, move |ec| {
            out.set(edit::field_access(ec, &ctx));
            Ok(())
        });
        let access = match query { Ok(()) => result.get(), Err(error) => edit::FieldAccess::Unknown(error.code()) };
        if let edit::FieldAccess::Unknown(code) = access {
            let mut st = self.state.borrow_mut();
            if self.diagnostics.get() && st.last_query_failure.as_ref() != Some(&(context.clone(), code)) {
                system::log(&format!("field query unavailable: {:?}; current key will pass without entering engine", code));
                st.last_query_failure = Some((context.clone(), code));
            }
        }
        access
    }

    /// Applies the shell's operations to `context` in one synchronous edit session, then sends any Caps Lock press.
    fn apply(&self, context: &ITfContext, out: &Outcome) -> Result<()> {
        let document_ops: Vec<Op> = out
            .ops
            .iter()
            .filter(|op| !matches!(op, Op::ToggleCapsLock))
            .cloned()
            .collect();
        if !document_ops.is_empty() {
            let this = self.this();
            let ctx = context.clone();
            let client = self.state.borrow().client;
            edit::request(context, client, TF_ES_SYNC | TF_ES_READWRITE, move |ec| {
                this.run_ops(ec, &ctx, document_ops)
            })?;
        }
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
    }

    fn handle_key(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM, down: bool, test: bool) -> Result<BOOL> {
        let Some(context) = pic.as_ref() else {
            return Ok(false.into());
        };
        let e = key_event(wparam, lparam);
        let checked = if test {
            CheckedKey { context: context.clone(), wparam: wparam.0, lparam: lparam.0, down, observed: e.time,
                access: self.probe_access(context), tray: system::tray_running() }
        } else {
            let paired = self.state.borrow_mut().checked_key.take().filter(|key|
                key.context == *context && key.wparam == wparam.0 && key.lparam == lparam.0 && key.down == down
                && e.time.saturating_sub(key.observed) <= 100);
            paired.unwrap_or_else(|| CheckedKey { context: context.clone(), wparam: wparam.0, lparam: lparam.0, down,
                observed: e.time, access: self.probe_access(context), tray: system::tray_running() })
        };
        let pass = checked.access.blocks_keys() || checked.tray;
        self.state.borrow_mut().password_field = checked.access.suppresses_trace();
        let trace_key = down && (e.vk == crate::shell::vk::SPACE || e.vk == crate::shell::vk::F2);
        if trace_key {
            self.trace(if test { "test-key-before" } else { "key-before" });
        }
        if test {
            let mut st = self.state.borrow_mut();
            st.checked_key = Some(checked);
            let Some(shell) = st.shell.as_mut() else {
                return Ok(false.into());
            };
            let wanted = shell.test_key_in_context(e, down, pass);
            // Request a real callback to end the old composition before passing an unknown/protected key.
            // No document or engine mutation takes place in the test probe.
            let wanted = wanted || (pass && (st.composition.is_some() || !st.pending_finishes.is_empty()));
            return Ok(wanted.into());
        }
        self.key_in_session(context, checked.access, checked.tray, move |shell| {
            if down { shell.key_down(e) } else { shell.key_up(e) }
        })
    }

    /// The engine must not accept a key until the application grants the write lock.
    fn key_in_session(&self, context: &ITfContext, access: edit::FieldAccess, tray: bool,
        key: impl FnOnce(&mut Shell) -> Outcome + 'static) -> Result<BOOL> {
        let result = Rc::new(RefCell::new(None));
        let output = result.clone();
        let before_write = Rc::new(Cell::new(0));
        let write_baseline = before_write.clone();
        let this = self.this();
        let ctx = context.clone();
        let client = self.state.borrow().client;
        let requested = edit::request(context, client, TF_ES_SYNC | TF_ES_READWRITE, move |ec| {
            this.drain_finishes(Some((&ctx, ec)))?;
            this.validate_composition(ec, &ctx)?;
            this.apply_context_policy(access, tray, Some((&ctx, ec)))?;
            if access.blocks_keys() || tray { return Ok(()); }
            write_baseline.set(this.state.borrow().write_serial);
            let out = match this.state.borrow_mut().shell.as_mut() {
                Some(shell) => key(shell), None => return Ok(()),
            };
            *output.borrow_mut() = Some(out.clone());
            this.trace("key-after-shell");
            this.run_ops(ec, &ctx, out.ops.iter().filter(|op| !matches!(op, Op::ToggleCapsLock)).cloned().collect())?;
            this.trace("key-after-document");
            *output.borrow_mut() = Some(out);
            Ok(())
        });
        if let Err(error) = requested {
            let attempted = result.borrow_mut().take();
            let written = self.state.borrow().write_serial != before_write.get();
            let eaten = attempted.as_ref().is_some_and(|out| out.eaten && written);
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
            system::log(&format!("key edit unavailable: {:?}; visible_write={} eaten={}", error.code(), written, eaten));
            return Ok(eaten.into());
        }
        let out = result.borrow_mut().take().unwrap_or_default();
        // Only non-document effects remain; do not request a second document edit session.
        let effects = Outcome { ops: out.ops.iter().filter(|op| matches!(op, Op::ToggleCapsLock)).cloned().collect(), ..Outcome::default() };
        self.apply(context, &effects)?;
        Ok(out.eaten.into())
    }

    fn update_indicator(&self) {
        let (thread_mgr, client, mode) = {
            let st = self.state.borrow();
            let Some(tm) = st.thread_mgr.clone() else { return };
            let Some(shell) = st.shell.as_ref() else { return };
            (tm, st.client, shell.mode())
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
        let mut st = self.state.borrow_mut();
        st.password_field = access.suppresses_trace();
        st.tray_running = tray;
        let finish = st.shell.as_mut().and_then(|shell| {
            let private = if matches!(access, edit::FieldAccess::Unknown(_)) { shell.private_context() }
                else { access == edit::FieldAccess::Private };
            shell.context_policy(access.blocks_keys() || tray, private)
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
        let unchanged = same_context && edit::selection_text(ec, &range, expected.encode_utf16().count())?.as_deref() == Some(&expected);
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
    fn finish_original_composition(&self, out: Outcome, session: Option<(&ITfContext, u32)>) -> Result<()> {
        {
            let mut st = self.state.borrow_mut();
            if let Some(composition) = st.composition.take() {
                let expected = std::mem::take(&mut st.composition_text);
                let replacement = out.ops.into_iter().find_map(|op| if let Op::Commit(text) = op { Some(text) } else { None });
                st.pending_finishes.push(Rc::new(PendingFinish { composition, expected: RefCell::new(expected),
                    replacement: RefCell::new(replacement), queued: Cell::new(false), done: Cell::new(false) }));
            }
        }
        self.drain_finishes(session)
    }

    fn drain_finishes(&self, session: Option<(&ITfContext, u32)>) -> Result<()> {
        let (pending, client) = { let st = self.state.borrow(); (st.pending_finishes.clone(), st.client) };
        for finish in pending {
            if finish.done.get() { continue; }
            let context = finish.context()?;
            if let Some((current, ec)) = session.filter(|(current, _)| **current == context) {
                let _ = current;
                finish.finish(ec)?;
            } else if !finish.queued.replace(true) {
                let deferred = finish.clone();
                let requested = edit::request(&context, client, TF_ES_ASYNCDONTCARE | TF_ES_READWRITE, move |ec| {
                    let result = deferred.finish(ec);
                    deferred.queued.set(false);
                    if let Err(error) = &result { system::log(&format!("composition finish failed; retained for retry: {:?}", error.code())); }
                    result
                });
                if let Err(error) = requested { finish.queued.set(false); return Err(error); }
            }
        }
        self.state.borrow_mut().pending_finishes.retain(|finish| !finish.done.get());
        Ok(())
    }
}

impl TextService {
    /// Runs inside the edit session: the document operations of one key, in order.
    fn run_ops(&self, ec: u32, context: &ITfContext, ops: Vec<Op>) -> Result<()> {
        for op in ops {
            match op {
                Op::Commit(text) => self.commit(ec, context, &text)?,
                Op::Compose(text) => self.compose(ec, context, &text)?,
                Op::ReplaceBefore { old, new } => {
                    if self.replace_before(ec, context, &old, &new)? {
                        let after = match self.state.borrow_mut().shell.as_mut() {
                            Some(shell) => shell.reconverted(),
                            None => Outcome::default(),
                        };
                        let rest: Vec<Op> = after.ops;
                        self.run_ops(ec, context, rest)?;
                    } else {
                        system::log("reconversion skipped: text before the caret changed");
                    }
                }
                Op::FlipSelection => self.flip_selection(ec, context)?,
                Op::ToggleCapsLock => {}
            }
        }
        Ok(())
    }

    fn commit(&self, ec: u32, context: &ITfContext, text: &str) -> Result<()> {
        let composition = self.state.borrow().composition.clone();
        match composition {
            Some(composition) => unsafe {
                let range = composition.GetRange()?;
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
            edit::caret_after(ec, context, &range)
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
                // Preserve formatting while correcting the committed word, then update only its language hint.
                // Word AutoCorrect ON still needs real-app validation; TF_ST_CORRECTION alone failed in D2.
                unsafe { range.SetText(ec, TF_ST_CORRECTION, &edit::utf16(new))? };
                { let mut st = self.state.borrow_mut(); st.write_serial = st.write_serial.wrapping_add(1); }
                self.corrected_language(ec, context, &range, new);
                edit::caret_after(ec, context, &range)?;
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
            unsafe { range.SetText(ec, TF_ST_CORRECTION, &edit::utf16(&flipped))? };
            { let mut st = self.state.borrow_mut(); st.write_serial = st.write_serial.wrapping_add(1); }
            self.corrected_language(ec, context, &range, &flipped);
        }
        Ok(())
    }

    fn corrected_language(&self, ec: u32, context: &ITfContext, range: &ITfRange, text: &str) {
        match edit::set_correction_language(ec, context, range, text) {
            Ok(true) => self.trace("correction-language-set"),
            Ok(false) => {},
            // Text was already changed. A host without language properties must not cause the key to be replayed.
            Err(error) => system::log(&format!("correction language property unavailable: {:?}", error.code())),
        }
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        self.trace("deactivate");
        let st = std::mem::take(&mut *self.state.borrow_mut());
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
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, _flags: u32) -> Result<()> {
        self.diagnostics.set(
            system::process_name().eq_ignore_ascii_case("WINWORD.EXE")
                && !system::elevated()
                && system::data_dir().join("tsf-diagnostics.enabled").is_file()
        );
        let thread_mgr = ptim.ok()?.clone();
        let mut shell = Shell::new(engine(system::home_dir(), system::data_dir().join("words.json")));
        shell.reset(system::caps_lock_on());
        let tray = system::tray_running();
        {
            let mut st = self.state.borrow_mut();
            st.thread_mgr = Some(thread_mgr.clone());
            st.client = tid;
            st.shell = Some(shell);
            st.tray_running = tray;
            st.sink = Some(self.to_interface::<ITfCompositionSink>());
        }
        unsafe {
            let keystrokes: ITfKeystrokeMgr = thread_mgr.cast()?;
            keystrokes.AdviseKeyEventSink(tid, &self.to_interface::<ITfKeyEventSink>(), true)?;
            let source: ITfSource = thread_mgr.cast()?;
            let cookie = source.AdviseSink(&ITfThreadMgrEventSink::IID, &self.to_interface::<ITfThreadMgrEventSink>())?;
            self.state.borrow_mut().thread_sink_cookie = cookie;
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
        system::log(&format!(
            "activate elevated={} appcontainer={} tray={} caps={}",
            system::elevated(),
            system::app_container(),
            tray,
            system::caps_lock_on()
        ));
        Ok(())
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
            if rguid.is_null() || unsafe { *rguid } != PRESERVED_RECONVERT {
                return Ok(false.into());
            }
            let Some(context) = pic.as_ref() else { return Ok(false.into()) };
            // A preserved command need not be preceded by OnTestKeyDown. Recheck password scope and tray coexistence.
            let access = self.probe_access(context);
            let tray = system::tray_running();
            self.state.borrow_mut().checked_key = None;
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
        // A detached composition can finish asynchronously after a new field has acquired focus.
        // Its callback must not clear or commit that field's new session.
        if self.state.borrow().composition.as_ref() != pcomposition.as_ref() || pcomposition.is_none() {
            return Ok(());
        }
        self.trace("terminated-before");
        let expected = {
            let mut st = self.state.borrow_mut();
            st.composition = None;
            std::mem::take(&mut st.composition_text)
        };
        let range = pcomposition.as_ref().and_then(|c| unsafe { c.GetRange().ok() });
        let unchanged = range.as_ref().is_some_and(|range| {
            edit::selection_text(ecwrite, range, expected.encode_utf16().count()).ok().flatten().as_deref() == Some(&expected)
        });
        let text = self.state.borrow_mut().shell.as_mut().and_then(|shell| {
            if unchanged { shell.composition_terminated() }
            else { shell.reset(system::caps_lock_on()); shell.engine().forget_reconversion(); None }
        });
        self.trace("terminated-after");
        if let (Some(text), Some(range)) = (text, range) {
            unsafe {
                range.SetText(ecwrite, 0, &edit::utf16(&text))?;
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
        Ok(())
    }

    fn OnSetFocus(&self, pdimfocus: Ref<ITfDocumentMgr>, pdimprevfocus: Ref<ITfDocumentMgr>) -> Result<()> {
        if pdimfocus.as_ref() == pdimprevfocus.as_ref() { return Ok(()); }
        self.commit_on_focus_change();
        let mut st = self.state.borrow_mut();
        st.tray_running = system::tray_running();
        drop(st);
        self.update_indicator();
        Ok(())
    }

    fn OnPushContext(&self, pic: Ref<ITfContext>) -> Result<()> {
        if self.context_event_affects_focus(pic) { self.commit_on_focus_change(); }
        Ok(())
    }

    fn OnPopContext(&self, pic: Ref<ITfContext>) -> Result<()> {
        if self.context_event_affects_focus(pic) { self.commit_on_focus_change(); }
        Ok(())
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
        if self.writing_compartment.get() || rguid.is_null() {
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
    }
}
