//! Hanautomata for Windows as a TSF input method (plan 21 S0 → P3). The DLL is a TSF text service: TSF calls it in each
//! application thread, `service` turns key events into `shell` calls and applies the result through edit sessions, and
//! the shared Rust engine decides each word as on macOS. `registration` holds the four registration steps (W-02) used by
//! both `DllRegisterServer` and the `hanautomata-ime-setup` tool.
//!
//! S0 scope: registration step by step, composition and commit in real applications, the switch keys (⇧Space, Caps Lock
//! short/long, 한/영, right Alt tap), F2/Shift+F2, F12 pause, the system mode indicator (가/A), pass-through in password
//! fields and beside the tray app (W-17). Elevation/unknown-token policy alone does not block input, but disables
//! learning and diagnostics (W-14); the other field/coexistence policies still apply.
//! Candidate preview and a separate settings editor
//! are development implementations awaiting real-app/visual acceptance. Ordinary desktop learning now has an
//! asynchronous companion; restricted-host persistence and full product acceptance remain incomplete.

pub mod shell;
pub mod settings;

// Worker-only authenticated transport; ordinary desktop peers only.
#[cfg(windows)]
pub mod learning_pipe;
#[cfg(windows)]
pub mod learning_runtime;
#[cfg(windows)]
mod learning_process;
#[cfg(windows)]
mod learning_wake;

#[cfg(windows)]
mod display;
#[cfg(windows)]
mod candidate;
#[cfg(windows)]
mod configuration;
#[cfg(windows)]
mod edit;
#[cfg(windows)]
mod exports;
#[cfg(windows)]
pub mod guids;
#[cfg(windows)]
pub mod registration;
#[cfg(windows)]
mod service;
#[cfg(windows)]
pub mod system;

#[cfg(all(windows, test))]
mod configuration_tests;
