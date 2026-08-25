//! Library surface for `flow-daemon`'s internal modules.
//!
//! `main.rs` builds on this crate rather than declaring these modules
//! itself, so integration tests (`daemon/tests/*.rs`) — which link
//! against the crate, not the binary — can reach `service`/`storage` the
//! same way `main.rs` does.

pub mod hotkey;
pub mod ipc;
pub mod service;
pub mod storage;
