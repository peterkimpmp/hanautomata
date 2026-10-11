/// Capture is allowed only for a known, writable, non-password, non-elevated, focused target that was seen within 300 ms
/// and has not changed since.
pub fn can_capture(
    known: bool,
    password: bool,
    writable: bool,
    elevated: bool,
    focused: bool,
    age_ms: i64,
    same_target: bool,
) -> bool {
    known && !password && writable && !elevated && focused && (0..=300).contains(&age_ms) && same_target
}
