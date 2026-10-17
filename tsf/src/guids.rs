//! Identities of the text service. The CLSID and profile never change once released: Windows stores them in each user's
//! input-method list.

use windows::core::GUID;

pub const CLSID_TEXT_SERVICE: GUID = GUID::from_u128(0xAC481E74_0295_436A_B0B8_AFA81FE9CBF1);
pub const PROFILE: GUID = GUID::from_u128(0x9E616573_9F68_41C2_95B5_B9F2B66AFD98);
/// The composition's underline (display attribute, W-01).
pub const DISPLAY_ATTRIBUTE_INPUT: GUID = GUID::from_u128(0x5B156B54_B148_4B17_82AE_81F35D81A83A);
/// The context-sensitive F2 command, also delivered through TSF's preserved-key route.
pub const PRESERVED_RECONVERT: GUID = GUID::from_u128(0x38CE7FA9_60EF_473D_B87F_9DF9BEE61521);
pub const PRESERVED_PROOFREAD: GUID = GUID::from_u128(0x8a42a145_16ef_4dd0_995b_7c3061a5f407);

pub const LANGID_KOREAN: u16 = 0x0412;
pub const DESCRIPTION: &str = "다듬";
pub const DLL_NAME: &str = "hanautomata_tsf.dll";

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` as the registry and InstallLayoutOrTip write it.
pub fn braced(g: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1, g.data2, g.data3, g.data4[0], g.data4[1], g.data4[2], g.data4[3], g.data4[4], g.data4[5], g.data4[6], g.data4[7]
    )
}

/// The input-method list entry: `0412:{CLSID}{PROFILE}`.
pub fn tip_string() -> String {
    format!("{:04X}:{}{}", LANGID_KOREAN, braced(&CLSID_TEXT_SERVICE), braced(&PROFILE))
}
