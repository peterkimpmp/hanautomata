//! Native, read-only preview. A standard static control exposes its text to Windows accessibility.
//! No keyboard hook, activation, timer, disk access, or fallback to a guessed cursor position.
use windows::core::{w, PCWSTR, Result};
use windows::Win32::Foundation::{E_FAIL, HANDLE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::shell::Preview;

pub(super) fn foreground(owner: HWND) -> bool {
    unsafe {
        let root = GetAncestor(owner, GA_ROOT);
        !owner.is_invalid() && IsWindow(Some(owner)).as_bool() && IsWindowVisible(owner).as_bool()
            && GetWindowThreadProcessId(owner, None) == GetCurrentThreadId()
            && GetWindowThreadProcessId(root, None) == GetCurrentThreadId()
            && root == GetForegroundWindow()
    }
}

struct Font(HFONT);
impl Drop for Font { fn drop(&mut self) { unsafe { let _ = DeleteObject(self.0.into()); } } }
struct Dc(HDC);
impl Drop for Dc { fn drop(&mut self) { unsafe { ReleaseDC(None, self.0); } } }

const TOKEN: PCWSTR = w!("Hanautomata.Candidate.Instance");
pub(super) struct Popup { window: HWND, owner: HWND, dpi: u32, _font: Font, token: Box<u8> }
impl Popup {
    /// Construct hidden; the presenter checks its generation again before showing.
    pub fn new(owner: HWND, caret: RECT, preview: &Preview) -> Result<Self> {
        if !foreground(owner) || !crate::system::desktop_settings_allowed() { return Err(E_FAIL.into()); }
        let dpi = unsafe { GetDpiForWindow(owner) };
        Self::build_hidden(owner, caret, preview, dpi)
    }
    fn build_hidden(owner: HWND, caret: RECT, preview: &Preview, dpi: u32) -> Result<Self> {
        if !(48..=768).contains(&dpi) { return Err(E_FAIL.into()); }
        let text = format!("{}\r\n{}{}{}{}\r\nSpace 확정", preview.text, preview.label,
            if preview.label.is_empty() { "" } else { " · " },
            if preview.correction { "F2 → " } else { "다른 후보: " }, preview.alternative);
        let mut utf16: Vec<u16> = text.encode_utf16().collect();
        // Engine bounds are 128 keys, but never trust an unexpected oversized result inside a host process.
        if utf16.len() > 1024 || utf16.contains(&0) { return Err(E_FAIL.into()); }
        unsafe {
            let font = Font(CreateFontW(-((16 * dpi / 96) as i32), 0, 0, 0, 400, 0, 0, 0,
                DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                0, w!("Malgun Gothic")));
            if font.0.is_invalid() { return Err(E_FAIL.into()); }
            let dc = Dc(GetDC(None));
            if dc.0.is_invalid() { return Err(E_FAIL.into()); }
            let old = SelectObject(dc.0, font.0.into());
            if old.is_invalid() { return Err(E_FAIL.into()); }
            let mut monitor = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let monitor_ok = GetMonitorInfoW(MonitorFromRect(&caret, MONITOR_DEFAULTTONEAREST), &mut monitor).as_bool();
            if !monitor_ok { SelectObject(dc.0, old); return Err(E_FAIL.into()); }
            let work = monitor.rcWork;
            let width = ((420 * dpi / 96) as i32).min(work.right.saturating_sub(work.left));
            if width < 64 { SelectObject(dc.0, old); return Err(E_FAIL.into()); }
            let mut measured = RECT { right: width - 8, ..Default::default() };
            let height = DrawTextW(dc.0, &mut utf16, &mut measured, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
            SelectObject(dc.0, old);
            if height <= 0 { return Err(E_FAIL.into()); }
            let rect = placement(caret, work, width, height.saturating_add(8)).ok_or(E_FAIL)?;
            utf16.push(0);
            let window = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"), PCWSTR(utf16.as_ptr()), WS_POPUP | WS_BORDER | WINDOW_STYLE(0x80), // SS_NOPREFIX
                rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top,
                Some(GetAncestor(owner, GA_ROOT)), None, None, None)?;
            let token = Box::new(0u8);
            if let Err(error) = SetPropW(window, TOKEN, Some(HANDLE((&*token as *const u8).cast_mut().cast()))) {
                let _ = DestroyWindow(window);
                return Err(error);
            }
            let popup = Self { window, owner, dpi, _font: font, token };
            SendMessageW(window, WM_SETFONT, Some(WPARAM(popup._font.0.0 as usize)), Some(LPARAM(0)));
            Ok(popup)
        }
    }
    pub fn show(&self) -> Result<()> {
        if !self.owns_window() || !foreground(self.owner) { return Err(E_FAIL.into()); }
        unsafe { SetWindowPos(self.window, Some(HWND_TOP), 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW) }
    }
    pub fn reposition(&self, owner: HWND, caret: RECT) -> Result<bool> {
        if !self.owns_window() || owner != self.owner || !foreground(owner)
            || unsafe { GetDpiForWindow(owner) } != self.dpi { return Ok(false); }
        unsafe {
            let mut old = RECT::default(); GetWindowRect(self.window, &mut old)?;
            let mut monitor = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(MonitorFromRect(&caret, MONITOR_DEFAULTTONEAREST), &mut monitor).as_bool() { return Err(E_FAIL.into()); }
            let Some(rect) = placement(caret, monitor.rcWork, old.right - old.left, old.bottom - old.top) else { return Ok(false); };
            SetWindowPos(self.window, None, rect.left, rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)?;
        }
        Ok(true)
    }
    fn owns_window(&self) -> bool {
        unsafe { IsWindow(Some(self.window)).as_bool()
            && GetWindowThreadProcessId(self.window, None) == GetCurrentThreadId()
            && GetPropW(self.window, TOKEN) == HANDLE((&*self.token as *const u8).cast_mut().cast()) }
    }
    pub fn is_visible(&self) -> bool { self.owns_window() && unsafe { IsWindowVisible(self.window).as_bool() } }
}
impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            // Owner teardown may already have destroyed this window. Never destroy a recycled foreign-thread HWND.
            if self.owns_window() {
                let _ = RemovePropW(self.window, TOKEN);
                let _ = DestroyWindow(self.window);
            }
        }
    }
}

fn placement(caret: RECT, work: RECT, width: i32, height: i32) -> Option<RECT> {
    if width <= 0 || height <= 0 || i64::from(work.right) - i64::from(work.left) < i64::from(width)
        || i64::from(work.bottom) - i64::from(work.top) < i64::from(height) { return None; }
    let left = caret.left.clamp(work.left, work.right - width);
    let below = i64::from(caret.bottom) + 3;
    let top = if below + i64::from(height) <= i64::from(work.bottom) { below as i32 }
        else { caret.top.saturating_sub(height).saturating_sub(3) };
    let top = top.clamp(work.top, work.bottom - height);
    Some(RECT { left, top, right: left + width, bottom: top + height })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_native_hidden_window_has_noactivate_style_and_scales_font_at_three_dpis() {
        // Own hidden test windows only. No activation, capture or messages to user applications.
        unsafe {
            let owner = CreateWindowExW(Default::default(), w!("STATIC"), w!("candidate fixture"), WS_POPUP,
                0, 0, 100, 100, None, None, None, None).unwrap();
            for dpi in [96, 144, 192] {
                let preview = Preview { text: "안녕".into(), alternative: "dkssud".into(), label: "확인 필요".into(), correction: true };
                let popup = Popup::build_hidden(owner, RECT { left: 10, top: 10, right: 10, bottom: 30 }, &preview, dpi).unwrap();
                assert!(!popup.is_visible());
                let style = GetWindowLongW(popup.window, GWL_EXSTYLE) as u32;
                assert_eq!(style & (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0, (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0);
                let mut font = LOGFONTW::default();
                assert_ne!(GetObjectW(popup._font.0.into(), std::mem::size_of::<LOGFONTW>() as i32,
                    Some((&mut font as *mut LOGFONTW).cast())), 0);
                assert_eq!(font.lfHeight, -((16 * dpi / 96) as i32));
                let window = popup.window;
                drop(popup);
                assert!(!IsWindow(Some(window)).as_bool());
            }
            DestroyWindow(owner).unwrap();
        }
    }
    #[test]
    fn candidate_placement_stays_on_negative_monitor_and_moves_above_bottom_edge() {
        let work = RECT { left: -1920, top: 0, right: 0, bottom: 1040 };
        let caret = RECT { left: -20, top: 1000, right: -20, bottom: 1020 };
        let r = placement(caret, work, 420, 100).unwrap();
        assert_eq!((r.left, r.right, r.top, r.bottom), (-420, 0, 897, 997));
        assert!(placement(caret, work, 420, 2000).is_none());
    }
    #[test]
    fn candidate_placement_handles_large_coordinates_without_overflow() {
        let work = RECT { left: i32::MAX - 1000, top: i32::MAX - 1000, right: i32::MAX, bottom: i32::MAX };
        let caret = RECT { left: i32::MAX, top: i32::MAX - 2, right: i32::MAX, bottom: i32::MAX };
        let r = placement(caret, work, 420, 100).unwrap();
        assert_eq!(r.right, i32::MAX);
        assert!(r.top >= work.top && r.bottom <= work.bottom);
    }
}
