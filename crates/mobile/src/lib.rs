//! Zeren mobile core — the UniFFI surface shared by the iOS and Android apps.
//!
//! - [`client_ffi`]: account, workspace and session state (wraps `zeren-client`).
//! - [`layout`]: analytic transcript layout — markdown → measured display lists
//!   (wraps `zeren-markdown` + `zeren-text`).
//! - [`orb`]: the desktop's voice orb as paintable frames (wraps `zeren-orb`).
//! - [`caption`]: the voice caption's streaming veil (wraps `zeren-veil`).

uniffi::setup_scaffolding!("zeren_core");

mod client_ffi;
pub mod layout;
pub mod caption;
pub mod orb;
pub mod wallpaper;

/// Version handshake: the Swift/Kotlin bindings must match the linked library.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
