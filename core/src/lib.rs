//! Platform-, transport-, and UI-independent domain model for Flow.
//!
//! This crate is the shared vocabulary the daemon, platform adapters, and
//! any future transport implementations build on: the input event
//! protocol, device/pairing state, and the traits that decouple the
//! daemon's core loop from OS-specific input handling and the transport
//! currently in use. See `docs/product/vision.md` for the architecture
//! this maps to.

pub mod channel;
pub mod device;
pub mod error;
pub mod input;
pub mod ipc;
pub mod link;
pub mod pairing;
pub mod permission;
pub mod protocol;
pub mod settings;
pub mod state;
pub mod switch_key;
