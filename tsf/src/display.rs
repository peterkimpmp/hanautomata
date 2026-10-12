//! The composition's look (W-01 display attribute): a solid underline in the text colour, as Windows' own IMEs show input
//! being composed.

use std::cell::Cell;
use windows::core::{implement, Error, OutRef, Result, BSTR, GUID};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, S_FALSE};
use windows::Win32::UI::TextServices::*;

use crate::guids::DISPLAY_ATTRIBUTE_INPUT;

#[implement(ITfDisplayAttributeInfo)]
pub struct InputAttribute;

fn attribute() -> TF_DISPLAYATTRIBUTE {
    TF_DISPLAYATTRIBUTE {
        lsStyle: TF_LS_SOLID,
        bAttr: TF_ATTR_INPUT,
        ..Default::default()
    }
}

impl ITfDisplayAttributeInfo_Impl for InputAttribute_Impl {
    fn GetGUID(&self) -> Result<GUID> {
        Ok(DISPLAY_ATTRIBUTE_INPUT)
    }

    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from("Hanautomata composition"))
    }

    fn GetAttributeInfo(&self, pda: *mut TF_DISPLAYATTRIBUTE) -> Result<()> {
        if pda.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe { *pda = attribute() };
        Ok(())
    }

    fn SetAttributeInfo(&self, _pda: *const TF_DISPLAYATTRIBUTE) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Reset(&self) -> Result<()> {
        Ok(())
    }
}

#[implement(IEnumTfDisplayAttributeInfo)]
pub struct Attributes {
    index: Cell<u32>,
}

impl Attributes {
    pub fn new() -> Attributes {
        Attributes { index: Cell::new(0) }
    }
}

impl IEnumTfDisplayAttributeInfo_Impl for Attributes_Impl {
    fn Clone(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(Attributes {
            index: Cell::new(self.index.get()),
        }
        .into())
    }

    fn Next(&self, ulcount: u32, rginfo: OutRef<'_, ITfDisplayAttributeInfo>, pcfetched: *mut u32) -> Result<()> {
        let mut fetched = 0u32;
        if ulcount > 0 && self.index.get() == 0 && !rginfo.is_null() {
            rginfo.write(Some(InputAttribute.into()))?;
            self.index.set(1);
            fetched = 1;
        }
        if !pcfetched.is_null() {
            unsafe { *pcfetched = fetched };
        }
        if fetched == ulcount {
            Ok(())
        } else {
            Err(Error::from_hresult(S_FALSE))
        }
    }

    fn Reset(&self) -> Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Skip(&self, ulcount: u32) -> Result<()> {
        let next = self.index.get().saturating_add(ulcount);
        self.index.set(next.min(1));
        if next <= 1 {
            Ok(())
        } else {
            Err(Error::from_hresult(S_FALSE))
        }
    }
}
