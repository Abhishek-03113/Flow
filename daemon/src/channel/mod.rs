//! Concrete `flow_core::channel::Channel` implementations
//! (`docs/architecture/channels.md`) — the daemon-to-daemon transport
//! layer, distinct from `crate::ipc` (the local Flutter<->daemon
//! boundary).

pub mod tcp;
