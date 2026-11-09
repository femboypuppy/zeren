//! Terminal panel (feature-inventory §1.10): an `alacritty_terminal`-backed
//! emulator fed by the engine's PTY stream over the generic RPC client.
//!
//! - [`emulator`] — pure Term + vte state machine (bytes in, grid out),
//!   shared with the mobile apps (`zeren-term`);
//! - [`view`] — cell palette, key/pointer/focus encodings, the ordered input
//!   queue, and the custom grid-painting element;
//! - [`predict`] — local echo prediction for terminals on other devices
//!   (`zeren-term` too);
//! - [`panel`] — session-scoped tabs, subscriptions with reconnect backoff,
//!   drag-reorder, and the Cmd/Ctrl+J toggle action.
//!
//! Method names come from `zeren_rpc::methods` and wire types from
//! `zeren_proto` (`TerminalSession`, `TerminalEvent`) — the same contract the
//! engine serves (feature-inventory §2.1).

pub(crate) mod dock;
pub mod panel;
pub mod view;
pub use zeren_term::{emulator, predict};
