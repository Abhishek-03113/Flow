//! Concrete `flow_core::channel::Channel` implementations
//! (`docs/architecture/channels.md`) — the daemon-to-daemon transport
//! layer, distinct from `crate::ipc` (the local Flutter<->daemon
//! boundary).

#[cfg(all(target_os = "linux", feature = "bluetooth"))]
pub mod bluetooth;
pub mod gate;
pub mod handshake;
pub mod negotiate;
pub mod noise;
pub mod reconnect;
pub mod tcp;
