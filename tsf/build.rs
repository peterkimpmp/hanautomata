use embed_manifest::manifest::{ExecutionLevel, SupportedOS};
use embed_manifest::{embed_manifest, empty_manifest};

fn main() {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") != "windows" {
        return;
    }
    // The setup tool asks for no elevation. Without a manifest Windows treats a 32-bit exe named "...setup..." as an
    // installer and refuses to start it unelevated (ERROR_ELEVATION_REQUIRED, Windows 365, 2026-10-12).
    embed_manifest(
        empty_manifest()
            .name("Hanautomata.ImeSetup")
            .supported_os(SupportedOS::Windows7..=SupportedOS::Windows10)
            .requested_execution_level(ExecutionLevel::AsInvoker),
    )
    .expect("embed the setup tool's manifest");
    // 32-bit (GNU): COM and regsvr32 look the exports up by plain name (DllGetClassObject), the stdcall symbols are
    // decorated (_DllGetClassObject@12). ld maps one to the other anyway, with a warning per export; say it is intended.
    if var("CARGO_CFG_TARGET_ARCH") == "x86" && var("CARGO_CFG_TARGET_ENV") == "gnu" {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--enable-stdcall-fixup");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
