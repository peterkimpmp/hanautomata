//! Edit sessions: TSF lets a text service read or change the document only inside one. `request` runs a closure as an
//! edit session (synchronously from a key event, asynchronously elsewhere); the helpers below are the document
//! operations the shell asks for.

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use windows::core::{implement, Interface, Result, HRESULT};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_EMPTY, VT_I4, VT_UNKNOWN};
use windows::Win32::UI::TextServices::*;

type Action = Box<dyn FnOnce(u32) -> Result<()>>;

#[implement(ITfEditSession)]
struct EditSession {
    action: RefCell<Option<Action>>,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        let action = self.action.borrow_mut().take();
        match action {
            Some(f) => f(ec),
            None => Ok(()),
        }
    }
}

/// Runs `f` in an edit session of `context`. With TF_ES_SYNC the closure has run when this returns.
pub fn request(
    context: &ITfContext,
    client: u32,
    flags: TF_CONTEXT_EDIT_CONTEXT_FLAGS,
    f: impl FnOnce(u32) -> Result<()> + 'static,
) -> Result<()> {
    let session: ITfEditSession = EditSession {
        action: RefCell::new(Some(Box::new(f))),
    }
    .into();
    let hr: HRESULT = unsafe { context.RequestEditSession(client, &session, flags)? };
    hr.ok()
}

pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn variant_i4(v: i32) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        let inner = &mut *var.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = v;
    }
    var
}

pub fn variant_as_i32(var: &VARIANT) -> Option<i32> {
    unsafe {
        let inner = &*var.Anonymous.Anonymous;
        (inner.vt == VT_I4).then_some(inner.Anonymous.lVal)
    }
}

/// The current selection (the caret when collapsed).
pub fn selection(ec: u32, context: &ITfContext) -> Result<ITfRange> {
    let mut sel = [TF_SELECTION::default()];
    let mut fetched = 0u32;
    unsafe { context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched)? };
    let range = unsafe { ManuallyDrop::take(&mut sel[0].range) };
    match (fetched, range) {
        (1, Some(r)) => Ok(r),
        _ => Err(windows::Win32::Foundation::E_FAIL.into()),
    }
}

/// Puts the caret at the end of `range`.
pub fn caret_after(ec: u32, context: &ITfContext, range: &ITfRange) -> Result<()> {
    let caret = unsafe { range.Clone()? };
    unsafe { caret.Collapse(ec, TF_ANCHOR_END)? };
    let sel = TF_SELECTION {
        range: ManuallyDrop::new(Some(caret)),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: false.into(),
        },
    };
    let result = unsafe { context.SetSelection(ec, std::slice::from_ref(&sel)) };
    drop(ManuallyDrop::into_inner(sel.range));
    result
}

pub fn text(ec: u32, range: &ITfRange, limit: usize) -> Result<String> {
    let mut buf = vec![0u16; limit];
    let mut n = 0u32;
    unsafe { range.GetText(ec, 0, &mut buf, &mut n)? };
    Ok(String::from_utf16_lossy(&buf[..n as usize]))
}

/// Selection conversion must either read the whole selection or leave it untouched.
pub fn selection_text(ec: u32, range: &ITfRange, limit: usize) -> Result<Option<String>> {
    let scan = unsafe { range.Clone()? };
    read_bounded_selection(limit, |buf| {
        let mut n = 0;
        unsafe { scan.GetText(ec, TF_TF_MOVESTART, buf, &mut n)?; }
        Ok(n as usize)
    })
}

fn read_bounded_selection(limit: usize, mut read: impl FnMut(&mut [u16]) -> Result<usize>) -> Result<Option<String>> {
    let mut all = Vec::new();
    loop {
        // Read one unit beyond the limit to distinguish an exact-size selection from a truncated prefix.
        let mut buf = vec![0u16; 512.min(limit.saturating_sub(all.len()).saturating_add(1))];
        let n = read(&mut buf)?;
        if n > buf.len() { return Err(windows::Win32::Foundation::E_INVALIDARG.into()); }
        if n == 0 {
            return String::from_utf16(&all).map(Some).map_err(|_| windows::Win32::Foundation::E_INVALIDARG.into());
        }
        if n > limit.saturating_sub(all.len()) { return Ok(None); }
        all.extend_from_slice(&buf[..n]);
    }
}

/// Set the language of a deliberately corrected single-script word. TF_ST_CORRECTION preserves the OLD language;
/// after a Korean-to-Latin F2 that can contradict the new word. Do not relabel mixed or unknown scripts.
/// This is a host interoperability candidate, not a promise that Word will suppress keyboard AutoCorrect.
pub fn set_correction_language(ec: u32, context: &ITfContext, range: &ITfRange, text: &str) -> Result<bool> {
    let Some(langid) = correction_language(text) else { return Ok(false) };
    let property = unsafe { context.GetProperty(&GUID_PROP_LANGID)? };
    unsafe { property.SetValue(ec, range, &variant_i4(langid as i32))?; }
    Ok(true)
}

fn correction_language(text: &str) -> Option<u16> {
    let mut language = None;
    for c in text.chars() {
        let current = match c {
            'a'..='z' | 'A'..='Z' => 0x0409,
            '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' | '\u{a960}'..='\u{a97f}' |
            '\u{ac00}'..='\u{d7af}' | '\u{d7b0}'..='\u{d7ff}' | '\u{ffa0}'..='\u{ffdc}' => 0x0412,
            c if c.is_ascii_whitespace() || c.is_ascii_digit() || c.is_ascii_punctuation() => continue,
            _ => return None,
        };
        if language.is_some() && language != Some(current) { return None; }
        language = Some(current);
    }
    language
}

/// Marks `range` with the composition underline (`atom` from ITfCategoryMgr::RegisterGUID), or clears it with 0.
pub fn set_attribute(ec: u32, context: &ITfContext, range: &ITfRange, atom: u32) -> Result<()> {
    let prop = unsafe { context.GetProperty(&GUID_PROP_ATTRIBUTE)? };
    if atom == 0 {
        unsafe { prop.Clear(ec, range) }
    } else {
        let var = variant_i4(atom as i32);
        unsafe { prop.SetValue(ec, range, &var) }
    }
}

/// Protected or unreadable fields must not feed the engine (C-CTX-01). VT_EMPTY is an ordinary field without a hint;
/// an unsuccessful query is different, and must not be remembered as permission to process its keys.
pub fn is_password_field(ec: u32, context: &ITfContext) -> bool {
    let read = || -> Result<bool> {
        let status = unsafe { context.GetStatus()? };
        if status.dwDynamicFlags & (TF_SD_READONLY | TF_SD_LOADING) != 0 { return Ok(true); }
        let compartments: ITfCompartmentMgr = context.cast()?;
        for guid in [GUID_COMPARTMENT_KEYBOARD_DISABLED, GUID_COMPARTMENT_EMPTYCONTEXT] {
            let compartment = unsafe { compartments.GetCompartment(&guid)? };
            let mut value = unsafe { compartment.GetValue()? };
            let disabled = compartment_disabled(&value);
            unsafe { let _ = VariantClear(&mut value); }
            if disabled? { return Ok(true); }
        }
        let range = selection(ec, context)?;
        let prop = unsafe { context.GetAppProperty(&GUID_PROP_INPUTSCOPE)? };
        let mut var = unsafe { prop.GetValue(ec, &range)? };
        let result = read_scope_variant(&var);
        unsafe { let _ = VariantClear(&mut var); }
        result
    };
    protected_query(read())
}

fn protected_query(result: Result<bool>) -> bool {
    result.unwrap_or(true)
}

fn compartment_disabled(value: &VARIANT) -> Result<bool> {
    if let Some(value) = variant_as_i32(value) { return Ok(value != 0); }
    if unsafe { value.Anonymous.Anonymous.vt } == VT_EMPTY { return Ok(false); }
    Err(windows::Win32::Foundation::E_INVALIDARG.into())
}

fn protected_scope(scopes: &[InputScope]) -> bool {
    scopes.iter().any(|scope| matches!(*scope,
        IS_PASSWORD | IS_PRIVATE | IS_NUMERIC_PASSWORD | IS_NUMERIC_PIN | IS_ALPHANUMERIC_PIN | IS_ALPHANUMERIC_PIN_SET))
}

fn read_scope_variant(var: &VARIANT) -> Result<bool> {
    unsafe {
        let inner = &*var.Anonymous.Anonymous;
        if inner.vt == VT_EMPTY { return Ok(false); }
        if inner.vt != VT_UNKNOWN { return Err(windows::Win32::Foundation::E_INVALIDARG.into()); }
        let unknown = inner.Anonymous.punkVal.as_ref().ok_or(windows::Win32::Foundation::E_POINTER)?;
        let scope = unknown.cast::<ITfInputScope>()?;
        let mut scopes = std::ptr::null_mut();
        let mut count = 0u32;
        let read = scope.GetInputScopes(&mut scopes, &mut count);
        let result = read.and_then(|_| {
            if count == 0 { return Ok(false); }
            if scopes.is_null() || count > 256 { return Err(windows::Win32::Foundation::E_INVALIDARG.into()); }
            Ok(protected_scope(std::slice::from_raw_parts(scopes, count as usize)))
        });
        if !scopes.is_null() {
            windows::Win32::System::Com::CoTaskMemFree(Some(scopes as *const _));
        }
        result
    }
}

#[cfg(test)]
mod protection_tests {
    use super::*;

    #[test]
    fn failed_scope_query_must_not_allow_the_engine_to_read_keys() {
        for failure in [windows::Win32::Foundation::E_FAIL, TF_E_NOLOCK, TF_E_DISCONNECTED] {
            assert!(protected_query(Err(failure.into())), "failed field query must pass keys to the application");
        }
        assert!(!protected_query(Ok(false)), "ordinary fields remain usable");
        assert!(protected_query(Ok(true)));
    }

    #[test]
    fn password_pin_and_private_fields_never_enter_composition_or_learning() {
        for scope in [IS_PASSWORD, IS_PRIVATE, IS_NUMERIC_PASSWORD, IS_NUMERIC_PIN, IS_ALPHANUMERIC_PIN, IS_ALPHANUMERIC_PIN_SET] {
            assert!(protected_scope(&[IS_DEFAULT, scope]), "protected input scope {}", scope.0);
        }
    }

    #[test]
    fn ordinary_fields_keep_the_input_method_available() {
        for scope in [IS_DEFAULT, IS_TEXT, IS_SEARCH, IS_EMAIL_SMTPEMAILADDRESS, IS_NUMBER, IS_HANGUL_FULLWIDTH] {
            assert!(!protected_scope(&[scope]));
        }
        assert!(!protected_scope(&[]));
    }

    #[test]
    fn absent_scope_is_normal_but_malformed_scope_is_protected() {
        assert!(!protected_query(read_scope_variant(&VARIANT::default())));
        assert!(protected_query(read_scope_variant(&variant_i4(0))));
        let mut null_scope = VARIANT::default();
        unsafe { (*null_scope.Anonymous.Anonymous).vt = VT_UNKNOWN; }
        assert!(protected_query(read_scope_variant(&null_scope)));
    }

    #[test]
    fn disabled_context_flags_and_missing_flags_are_distinct() {
        assert!(!compartment_disabled(&VARIANT::default()).unwrap());
        assert!(!compartment_disabled(&variant_i4(0)).unwrap());
        assert!(compartment_disabled(&variant_i4(1)).unwrap());
        assert!(compartment_disabled(&variant_i4(-1)).unwrap());
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn read_selection(text: &str, chunk: usize, limit: usize) -> Option<String> {
        let input = utf16(text);
        let mut offset = 0;
        read_bounded_selection(limit, |buf| {
            let n = buf.len().min(input.len() - offset).min(chunk);
            buf[..n].copy_from_slice(&input[offset..offset + n]);
            offset += n;
            Ok(n)
        }).unwrap()
    }

    #[test]
    fn selection_over_limit_is_never_replaced_with_its_prefix() {
        assert_eq!(read_selection(&"a".repeat(4097), 4097, 4096), None);
        let exact = "한".repeat(4096);
        assert_eq!(read_selection(&exact, 4096, 4096), Some(exact));
    }

    #[test]
    fn partial_host_reads_and_split_surrogates_keep_the_entire_selection() {
        let selected = "한글🙂abcdef";
        assert_eq!(read_selection(selected, 1, 4096), Some(selected.to_string()));
    }

    #[test]
    fn failed_read_or_malformed_utf16_never_becomes_a_replacement() {
        assert!(read_bounded_selection(4096, |_| Err(TF_E_NOLOCK.into())).is_err());
        let mut first = true;
        assert!(read_bounded_selection(4096, |buf| {
            if first { first = false; buf[0] = 0xd800; Ok(1) } else { Ok(0) }
        }).is_err());
        assert_eq!(read_selection("", 10, 0), Some(String::new()));
        assert_eq!(read_selection("a", 10, 0), None);
    }

    #[test]
    fn f2_language_follows_the_replacement_not_the_old_korean_profile() {
        assert_eq!(correction_language("gksrmf "), Some(0x0409));
        assert_eq!(correction_language("한글 "), Some(0x0412));
        assert_eq!(correction_language("ㄱㅏ"), Some(0x0412));
        assert_eq!(correction_language("API-123 "), Some(0x0409));
    }

    #[test]
    fn mixed_or_unknown_selection_keeps_its_existing_language_properties() {
        for selected in ["한글 API", "123 !", "", "🙂", "café", "東京", "漢字 한글"] {
            assert_eq!(correction_language(selected), None, "do not guess the language of {selected}");
        }
    }
}
