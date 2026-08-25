//! Flow daemon entry point.
//!
//! Per vision.md §8, this binary owns the real-time input pipeline and must
//! keep working with no UI attached. Capture, transport, and pairing are
//! not wired up yet — this is crate-boundary scaffolding only.

mod service;
mod storage;

use flow_core::state::AppState;

fn main() {
    let _state = AppState::new();
    println!("flow-daemon: scaffolding only, input capture and networking are not yet implemented");
}
