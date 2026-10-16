use super::*;
use serde_json::json;

static SERIAL: AtomicU64 = AtomicU64::new(1);
impl TextService_Impl {
    pub(super) fn cancel_proofreading(&self) {
        let old = {
            let mut st = self.state.borrow_mut(); st.proofreading_token = 0; st.proofreading.take()
        };
        drop(old);
    }
    pub(super) fn start_proofreading(&self, context: &ITfContext) -> Result<BOOL> {
        if self.probe_access(context) != edit::FieldAccess::Ordinary || system::tray_running()
            || !system::desktop_settings_allowed() { return Ok(false.into()); }
        let (client, serial, manager) = {
            let st = self.state.borrow();
            if st.host_private || st.excluded_app || st.composition.is_some() || st.pending_finishes.iter().any(|p| !p.done.get())
                || st.shell.as_ref().is_none_or(|s| s.is_composing() || s.engine().paused()) { return Ok(false.into()); }
            (st.client, st.write_serial, st.thread_mgr.clone())
        };
        let Some(manager) = manager else { return Ok(false.into()); };
        if !crate::candidate::focused(context, &manager)? { return Ok(false.into()); }
        let captured = Rc::new(RefCell::new(None)); let output = captured.clone(); let ctx = context.clone();
        edit::request(context, client, TF_ES_SYNC | TF_ES_READ, move |ec| {
            if edit::field_access(ec, &ctx) != edit::FieldAccess::Ordinary { return Ok(()); }
            let range = edit::selection(ec, &ctx)?;
            if unsafe { range.IsEmpty(ec)?.as_bool() } { return Ok(()); }
            if let Some(text) = edit::selection_text(ec, &range, 4096)? { *output.borrow_mut() = Some((range, text)); }
            Ok(())
        })?;
        let Some((range, text)) = captured.borrow_mut().take() else { return Ok(false.into()); };
        if text.is_empty() || text.len() > 16_384 { return Ok(false.into()); }
        self.cancel_proofreading();
        let token = SERIAL.fetch_add(1, Ordering::Relaxed);
        let id = format!("tsf-{}-{token}", self.diagnostic_id);
        let revision = serial.to_string();
        let request = json!({"request_id":id,"document_id":id,"revision":revision,"text":text,"protected_words":[]});
        let helper = crate::exports::module_path()?.with_file_name("Hanautomata.Proofreading.exe");
        let this = self.this(); let ctx = context.clone();
        let session = crate::proofreading::Session::start(helper, request, move |reply| {
            let Some(reply) = reply else { this.cancel_proofreading(); return; };
            if reply["schema_version"] != 1 || reply["request_id"] != id || reply["document_id"] != id
                || reply["revision"] != revision || reply["original"] != text { this.cancel_proofreading(); return; }
            let Some(replacement) = reply["text"].as_str().filter(|s| !s.contains('\0') && s.len() <= 16_640) else { this.cancel_proofreading(); return; };
            let replacement = replacement.to_owned();
            let owner = this.clone(); let target = ctx.clone();
            // The callback and all retained interfaces remain in the TSF apartment.
            let result = edit::request(&ctx, client, TF_ES_ASYNCDONTCARE | TF_ES_READWRITE, move |ec| {
                let result = (|| {
                let st = owner.state.borrow();
                if st.proofreading_token != token || st.write_serial != serial || st.composition.is_some()
                    || st.shell.as_ref().is_none_or(|s| s.is_composing() || s.engine().paused()) || st.host_private || st.excluded_app
                    || st.thread_mgr.as_ref().is_none_or(|tm| !crate::candidate::focused(&target, tm).unwrap_or(false)) { return Ok(()); }
                drop(st);
                if edit::field_access(ec, &target) != edit::FieldAccess::Ordinary { return Ok(()); }
                let selected = edit::selection(ec, &target)?;
                if unsafe { selected.CompareStart(ec, &range, TF_ANCHOR_START)? } != 0
                    || unsafe { selected.CompareEnd(ec, &range, TF_ANCHOR_END)? } != 0
                    || edit::selection_text(ec, &range, 4096)?.as_deref() != Some(&text) { return Ok(()); }
                owner.corrected_text(ec, &target, &range, &replacement)?;
                Ok(())
                })();
                owner.cancel_proofreading();
                result
            });
            if result.is_err() { this.cancel_proofreading(); }
        })?;
        let mut st = self.state.borrow_mut(); st.proofreading_token = token; st.proofreading = Some(session);
        Ok(true.into())
    }
}
