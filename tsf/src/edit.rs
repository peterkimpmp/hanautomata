//! Edit sessions: TSF lets a text service read or change the document only inside one. `request` runs a closure as an
//! edit session (synchronously from a key event, asynchronously elsewhere); the helpers below are the document
//! operations the shell asks for.

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use windows::core::{implement, Interface, Result, HRESULT};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_I4, VT_UNKNOWN};
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

/// The field asks for no IME help: a password (C-CTX-01). Read from the application's InputScope property.
pub fn is_password_field(ec: u32, context: &ITfContext) -> bool {
    let read = || -> Result<bool> {
        let range = selection(ec, context)?;
        let prop = unsafe { context.GetAppProperty(&GUID_PROP_INPUTSCOPE)? };
        let mut var = unsafe { prop.GetValue(ec, &range)? };
        let mut password = false;
        unsafe {
            let inner = &*var.Anonymous.Anonymous;
            if inner.vt == VT_UNKNOWN {
                if let Some(unknown) = inner.Anonymous.punkVal.as_ref() {
                    if let Ok(scope) = unknown.cast::<ITfInputScope>() {
                        let mut scopes: *mut InputScope = std::ptr::null_mut();
                        let mut count = 0u32;
                        if scope.GetInputScopes(&mut scopes, &mut count).is_ok() && !scopes.is_null() {
                            let list = std::slice::from_raw_parts(scopes, count as usize);
                            password = list.contains(&IS_PASSWORD);
                            windows::Win32::System::Com::CoTaskMemFree(Some(scopes as *const _));
                        }
                    }
                }
            }
            let _ = VariantClear(&mut var);
        }
        Ok(password)
    };
    read().unwrap_or(false)
}
