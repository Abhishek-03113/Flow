//! Transport abstraction (vision.md §10, Connectivity).
//!
//! Concrete transports (WebSocket first, Bluetooth/QUIC later) implement
//! this trait so the daemon's input pipeline never depends on how events
//! actually move between devices.

use crate::protocol::InputEvent;

pub trait Transport {
    type Error;

    fn send(&mut self, event: &InputEvent) -> Result<(), Self::Error>;
    fn recv(&mut self) -> Result<InputEvent, Self::Error>;
}
