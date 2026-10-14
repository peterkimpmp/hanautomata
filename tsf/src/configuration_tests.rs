use windows::core::Interface;
use windows::Win32::UI::TextServices::{ITfFnConfigure, ITfTextInputProcessorEx};

#[test]
fn configuration_is_available_before_input_service_activation() {
    // The Text Services control panel creates the service directly for Properties, without ActivateEx.
    let service: ITfTextInputProcessorEx = crate::service::TextService::new().into();
    let configure: ITfFnConfigure = service.cast().expect("the registered COM class must expose Properties");
    let name = unsafe { configure.GetDisplayName() }.unwrap();
    assert_eq!(name.to_string(), "한오토마타 입력기 설정");
}

#[test]
fn invalid_properties_calls_fail_without_opening_a_window() {
    use windows::Win32::Foundation::{E_INVALIDARG, E_POINTER, HWND};
    let service: ITfTextInputProcessorEx = crate::service::TextService::new().into();
    let configure: ITfFnConfigure = service.cast().unwrap();
    unsafe {
        assert_eq!(configure.Show(HWND::default(), crate::guids::LANGID_KOREAN, std::ptr::null()).unwrap_err().code(), E_POINTER);
        assert_eq!(configure.Show(HWND::default(), 0x409, &crate::guids::PROFILE).unwrap_err().code(), E_INVALIDARG);
    }
}
