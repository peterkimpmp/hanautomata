// 32-bit Windows (GNU): COM and regsvr32 look the exports up by plain name (DllGetClassObject), while the stdcall symbols
// are decorated (_DllGetClassObject@12). ld maps one to the other anyway, with a warning per export; say it is intended.
fn main() {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") == "windows" && var("CARGO_CFG_TARGET_ARCH") == "x86" && var("CARGO_CFG_TARGET_ENV") == "gnu" {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--enable-stdcall-fixup");
    }
}
