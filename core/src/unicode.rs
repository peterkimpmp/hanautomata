// .NET character semantics over UTF-16 code units, backed by tables generated from the C# runtime (unicode_tables.rs).
// Supplementary-plane text reaches .NET as surrogate pairs: non-letters with unchanged case. The helpers mirror that.

use crate::unicode_tables::{FOLD, LETTER, LOWER, TO_LOWER, UPPER};
use std::cmp::Ordering;

fn in_ranges(ranges: &[(u16, u16)], c: u16) -> bool {
    ranges
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                Ordering::Less
            } else if lo > c {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

fn mapped(pairs: &[(u16, u16)], c: u16) -> u16 {
    match pairs.binary_search_by_key(&c, |&(from, _)| from) {
        Ok(i) => pairs[i].1,
        Err(_) => c,
    }
}

/// `char.IsLetter`
pub(crate) fn is_letter(c: u16) -> bool {
    in_ranges(LETTER, c)
}
/// `char.IsUpper`
pub(crate) fn is_upper(c: u16) -> bool {
    in_ranges(UPPER, c)
}
/// `char.IsLower`
pub(crate) fn is_lower(c: u16) -> bool {
    in_ranges(LOWER, c)
}
/// `char.ToLowerInvariant`
pub(crate) fn to_lower(c: u16) -> u16 {
    mapped(TO_LOWER, c)
}
/// The per-character key behind `StringComparer.OrdinalIgnoreCase`.
pub(crate) fn fold(c: u16) -> u16 {
    mapped(FOLD, c)
}

pub(crate) fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub(crate) fn string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `char.ToLowerInvariant` for one Rust char (BMP only; others are unchanged, as their surrogate units are in .NET).
pub(crate) fn lower_char(c: char) -> char {
    let v = c as u32;
    if v > 0xffff {
        return c;
    }
    char::from_u32(to_lower(v as u16) as u32).unwrap_or(c)
}

/// `string.ToLowerInvariant` for the BMP.
pub(crate) fn lower_str(s: &str) -> String {
    s.chars().map(lower_char).collect()
}

/// Hash key under which two strings are equal exactly when `OrdinalIgnoreCase` says so.
pub(crate) fn fold_key(s: &str) -> String {
    let units: Vec<u16> = s.encode_utf16().map(fold).collect();
    string(&units)
}

/// `a.StartsWith(b, StringComparison.OrdinalIgnoreCase)` over code units.
pub(crate) fn starts_with_ignore_case(a: &[u16], b: &[u16]) -> bool {
    a.len() >= b.len() && a.iter().zip(b).all(|(&x, &y)| fold(x) == fold(y))
}
