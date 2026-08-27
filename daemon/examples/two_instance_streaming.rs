//! Manual, local-only sanity check for the end-to-end input streaming
//! pipeline (`daemon/todos.json` G8): real capture -> the switch-aware
//! gate -> a real (loopback) `TcpChannel` -> real injection, all in one
//! process standing in for "two instances on one host" — the minimum
//! way to exercise `pipeline::send_while_active`/`::receive_and_inject`
//! against genuine input devices without a second machine.
//!
//! Needs `/dev/input` (read) and `/dev/uinput` (write) access — see
//! `daemon/README.md`'s "E1"/"E2" manual verification notes; this
//! repo's own development container has neither, so this example is
//! unverified beyond `cargo build --example two_instance_streaming`.
//! Run with:
//!
//! ```sh
//! cargo run -p flow-daemon --example two_instance_streaming
//! ```
//!
//! Type on the real keyboard: every event is captured, streamed over a
//! genuine loopback TCP/WebSocket connection, and re-injected into a
//! Flow-owned virtual device (watch it with `evtest` on the new
//! `/dev/input/eventN` node named "Flow Virtual Input"). This example
//! reports the stand-in *peer* as the `Active` device — the state in
//! which real input is forwarded to it (`vision.md` §22: "only the
//! active device should receive input") — proving capture/Channel/
//! injection, not the device-switching gate itself, which `pipeline`'s
//! own automated tests already cover with synthetic events. It also
//! never calls `set_suppress_local`, so events still reach this
//! machine's own applications as usual; that keeps the example safe to
//! run interactively rather than grabbing the keyboard it's being
//! driven from. Not part of the automated test suite — kill the process
//! (Ctrl+C) to stop it.

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::mpsc as std_mpsc;
    use std::thread;

    use flow_core::channel::Channel;
    use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
    use flow_core::input::InputCapture;
    use flow_daemon::channel::tcp::TcpChannel;
    use flow_daemon::pipeline;
    use flow_daemon::service::LOCAL_DEVICE_ID;
    use flow_platform::DefaultInputCapture;
    use tokio::net::TcpListener;
    use tokio::sync::{mpsc, watch};

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let receiver_side = tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await.expect("accept");
        let channel = TcpChannel::accept(stream).await.expect("accept ws");
        let injector = flow_platform::new_default_input_injector().expect("construct injector");
        pipeline::receive_and_inject(Box::new(channel), injector).await;
    });

    let sender_side: Box<dyn Channel> = Box::new(TcpChannel::connect(addr).await?);

    let (std_tx, std_rx) = std_mpsc::channel();
    let mut capture = DefaultInputCapture::new(std_tx);
    capture.start()?;

    // Bridges the capture thread's std::sync::mpsc onto the async
    // channel `send_while_active` expects — the same pattern
    // `hotkey::runner::spawn` (F2) already uses for the same reason
    // (`InputCapture`'s contract is synchronous).
    let (bridge_tx, bridge_rx) = mpsc::unbounded_channel();
    thread::spawn(move || {
        for event in std_rx {
            if bridge_tx.send(event).is_err() {
                break;
            }
        }
    });

    // One process stands in for two, so the "peer" here is fictional —
    // but it has to be the Active one for anything to be forwarded at
    // all, since input flows *toward* the active device.
    let peer_id = DeviceId("example-peer".to_string());
    let local_device = Device {
        id: DeviceId(LOCAL_DEVICE_ID.to_string()),
        name: "This Machine".to_string(),
        os: HostOs::Linux,
        state: DeviceState::Inactive,
        last_seen: chrono::Utc::now(),
    };
    let peer_device = Device {
        id: peer_id.clone(),
        name: "Example Peer".to_string(),
        os: HostOs::Linux,
        state: DeviceState::Active,
        last_seen: chrono::Utc::now(),
    };
    let (_devices_tx, devices_rx) = watch::channel(vec![local_device, peer_device]);

    println!(
        "Streaming real input over a loopback TcpChannel onto \"Flow Virtual Input\". Ctrl+C to stop."
    );
    pipeline::send_while_active(bridge_rx, devices_rx, sender_side, peer_id).await;

    capture.stop()?;
    receiver_side.await?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("two_instance_streaming is Linux-only (it binds to evdev/uinput)");
}
