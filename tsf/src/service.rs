//! The TSF text service: one per application thread that uses the input method. It forwards key events to the shell,
//! applies the shell's operations in edit sessions, keeps the composition, follows focus, and mirrors the mode on the
//! system indicator (가/A) through the conversion-mode compartment (W-07).

use std::cell::{Cell, RefCell};
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
    shell: Option<Shell>,
    attribute_atom: u32,
    /// The context whose field kind (password or not) was last read.
    checked_context: Option<ITfContext>,
    password_field: bool,
    tray_running: bool,
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

    /// Re-reads what decides pass-through for this context: the tray app (W-17) and a password field (C-CTX-01).
    fn refresh_pass_through(&self, context: &ITfContext) {
        let changed = {
            let st = self.state.borrow();
            st.checked_context.as_ref() != Some(context)
        };
        if changed {
            let client = self.state.borrow().client;
            let result = std::rc::Rc::new(Cell::new(false));
            let out = result.clone();
            let ctx = context.clone();
            let _ = edit::request(context, client, TF_ES_SYNC | TF_ES_READ, move |ec| {
                out.set(edit::is_password_field(ec, &ctx));
                Ok(())
            });
            let mut st = self.state.borrow_mut();
            st.checked_context = Some(context.clone());
            st.password_field = result.get();
            st.tray_running = system::tray_running();
            if st.password_field {
                system::log("context password-field pass-through");
            }
        }
        let mut st = self.state.borrow_mut();
        let pass = st.password_field || st.tray_running;
        if let Some(shell) = st.shell.as_mut() {
            shell.pass_through = pass;
        }
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
        if down && test {
            self.refresh_pass_through(context);
        }
        let trace_key = down && (e.vk == crate::shell::vk::SPACE || e.vk == crate::shell::vk::F2);
        if trace_key {
            self.trace(if test { "test-key-before" } else { "key-before" });
        }
        if test {
            let mut st = self.state.borrow_mut();
            let Some(shell) = st.shell.as_mut() else {
                return Ok(false.into());
            };
            let wanted = if down { shell.test_key_down(e) } else { shell.test_key_up(e) };
            return Ok(wanted.into());
        }
        let (out, had_fresh_word) = {
            let mut st = self.state.borrow_mut();
            let Some(shell) = st.shell.as_mut() else {
                return Ok(false.into());
            };
            let had_fresh_word = self.diagnostics.get() && shell.engine().reconversion().is_some();
            let out = if down {
                shell.key_down(e)
            } else {
                shell.key_up(e)
            };
            (out, had_fresh_word)
        };
        if trace_key || had_fresh_word {
            self.trace("key-after-shell");
        }
        self.apply(context, &out)?;
        if trace_key || had_fresh_word {
            self.trace("key-after-document");
        }
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
        let (composition, out, client) = {
            let mut st = self.state.borrow_mut();
            let client = st.client;
            let out = match st.shell.as_mut() {
                Some(shell) => shell.focus_lost(),
                None => return,
            };
            (st.composition.take(), out, client)
        };
        self.trace("focus-after-shell");
        let Some(composition) = composition else { return };
        let Ok(range) = (unsafe { composition.GetRange() }) else {
            return;
        };
        let Ok(context) = (unsafe { range.GetContext() }) else {
            return;
        };
        let atom_clear = 0;
        let ops = out.ops;
        let ctx = context.clone();
        let _ = edit::request(&context, client, TF_ES_ASYNCDONTCARE | TF_ES_READWRITE, move |ec| {
            let range = unsafe { composition.GetRange()? };
            for op in ops {
                if let Op::Commit(text) = op {
                    unsafe { range.SetText(ec, 0, &edit::utf16(&text))? };
                }
            }
            let _ = edit::set_attribute(ec, &ctx, &range, atom_clear);
            unsafe { composition.EndComposition(ec) }
        });
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
        let composition = self.state.borrow_mut().composition.take();
        match composition {
            Some(composition) => unsafe {
                let range = composition.GetRange()?;
                range.SetText(ec, 0, &edit::utf16(text))?;
                let _ = edit::set_attribute(ec, context, &range, 0);
                edit::caret_after(ec, context, &range)?;
                composition.EndComposition(ec)
            },
            None => unsafe {
                let insert: ITfInsertAtSelection = context.cast()?;
                let range = insert.InsertTextAtSelection(ec, INSERT_TEXT_AT_SELECTION_FLAGS(0), &edit::utf16(text))?;
                edit::caret_after(ec, context, &range)
            },
        }
    }

    fn compose(&self, ec: u32, context: &ITfContext, text: &str) -> Result<()> {
        let existing = self.state.borrow().composition.clone();
        if text.is_empty() {
            if let Some(composition) = existing {
                self.state.borrow_mut().composition = None;
                unsafe {
                    let range = composition.GetRange()?;
                    range.SetText(ec, 0, &[])?;
                    composition.EndComposition(ec)?;
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
                unsafe { range.SetText(ec, 0, &edit::utf16(new))? };
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
        let text = edit::text(ec, &range, 4096)?;
        let flipped = self.state.borrow().shell.as_ref().and_then(|s| s.flip(&text));
        if let Some(flipped) = flipped {
            unsafe { range.SetText(ec, 0, &edit::utf16(&flipped))? };
        }
        Ok(())
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
        if !fforeground.as_bool() {
            self.commit_on_focus_change();
        }
        Ok(())
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
            self.state.borrow_mut().checked_context = None;
            self.refresh_pass_through(context);
            self.trace("preserved-before");
            let out = match self.state.borrow_mut().shell.as_mut() {
                Some(shell) => shell.reconversion_command(),
                None => return Ok(false.into()),
            };
            self.apply(context, &out)?;
            self.trace("preserved-after");
            system::log(&format!("preserved F2 handled={}", out.eaten));
            Ok(out.eaten.into())
        })
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(&self, ecwrite: u32, pcomposition: Ref<ITfComposition>) -> Result<()> {
        self.trace("terminated-before");
        let text = {
            let mut st = self.state.borrow_mut();
            st.composition = None;
            st.shell.as_mut().and_then(|s| s.composition_terminated())
        };
        self.trace("terminated-after");
        if let (Some(text), Some(composition)) = (text, pcomposition.as_ref()) {
            unsafe {
                let range = composition.GetRange()?;
                range.SetText(ecwrite, 0, &edit::utf16(&text))?;
                if let Ok(context) = range.GetContext() {
                    let _ = edit::set_attribute(ecwrite, &context, &range, 0);
                }
            }
        }
        Ok(())
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnSetFocus(&self, _pdimfocus: Ref<ITfDocumentMgr>, _pdimprevfocus: Ref<ITfDocumentMgr>) -> Result<()> {
        self.commit_on_focus_change();
        let mut st = self.state.borrow_mut();
        st.checked_context = None;
        st.tray_running = system::tray_running();
        drop(st);
        self.update_indicator();
        Ok(())
    }

    fn OnPushContext(&self, _pic: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }

    fn OnPopContext(&self, _pic: Ref<ITfContext>) -> Result<()> {
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
