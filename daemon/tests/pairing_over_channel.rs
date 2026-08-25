//! End-to-end pairing over a real `Channel` (`daemon/todos.json` G7):
//! two independent `DaemonService` instances complete a real
//! `pair_with_candidate` handshake over an actual `TcpChannel`
//! (loopback), each ending up with the other in its own devices list —
//! this task's own stated acceptance criterion, exercised against
//! genuine sockets rather than the in-memory `ChannelPair` test double
//! `G1`'s own tests use.

use flow_core::channel::{Channel, ChannelAddress};
use flow_core::device::HostOs;
use flow_core::pairing::PairingStage;
use flow_daemon::channel::tcp::TcpChannel;
use flow_daemon::discovery::DiscoveredPeer;
use flow_daemon::service::DaemonService;
use flow_daemon::storage::Storage;
use tokio::net::TcpListener;

async fn service() -> DaemonService {
    let storage = Storage::open_in_memory().await.expect("open in-memory db");
    DaemonService::new(storage).await
}

#[tokio::test]
async fn two_daemons_complete_a_real_pairing_handshake_over_tcp() {
    let service_a = service().await;
    let service_b = service().await;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind b's listener");
    let b_addr = listener.local_addr().expect("local addr");

    let responder = {
        let service_b = service_b.clone();
        tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            let channel = TcpChannel::accept(stream).await.expect("accept ws");
            let channel: Box<dyn Channel> = Box::new(channel);
            service_b
                .accept_pairing_request(channel)
                .await
                .expect("responder side of the handshake");
        })
    };

    // In production this would come from `discovery::tcp`/`::bluetooth`
    // actually observing device B on the network; this test injects it
    // directly since standing up real broadcast/inquiry discovery isn't
    // this task's concern (that's G3/G5, already tested standalone).
    service_a
        .note_discovered_peer(DiscoveredPeer {
            name: "Studio B".to_string(),
            os: HostOs::Linux,
            address: ChannelAddress::Tcp(b_addr),
        })
        .await;

    let mut sessions = service_a.watch_pairing_session();
    let _ = sessions.borrow_and_update();
    service_a.start_pairing().await.expect("start pairing");

    sessions.changed().await.expect("searching");
    assert_eq!(sessions.borrow_and_update().stage, PairingStage::Searching);

    sessions.changed().await.expect("found");
    let found = sessions.borrow_and_update().clone();
    assert_eq!(found.stage, PairingStage::Found);
    let candidate = found
        .candidates
        .iter()
        .find(|c| c.name == "Studio B")
        .expect("the live-discovered candidate is offered")
        .clone();

    service_a
        .pair_with_candidate(&candidate.id)
        .await
        .expect("pair with the live candidate");

    sessions.changed().await.expect("requesting");
    assert_eq!(sessions.borrow_and_update().stage, PairingStage::Requesting);

    sessions.changed().await.expect("paired");
    assert_eq!(sessions.borrow_and_update().stage, PairingStage::Paired);

    responder.await.expect("responder task");

    let a_devices = service_a.watch_devices().borrow().clone();
    assert!(
        a_devices.iter().any(|d| d.name == "Studio B"),
        "device A's list should contain device B: {a_devices:?}"
    );

    let b_devices = service_b.watch_devices().borrow().clone();
    assert!(
        b_devices.iter().any(|d| d.name == "MacBook"),
        "device B's list should contain device A: {b_devices:?}"
    );
}

/// The mirror-image failure path: the responder rejects, so the
/// initiator's session lands in `Failed` (not `Paired`) and neither
/// side gains a new device.
#[tokio::test]
async fn a_rejected_real_pairing_request_lands_in_failed_not_paired() {
    let service_a = service().await;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind b's listener");
    let b_addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await.expect("accept");
        let mut channel = TcpChannel::accept(stream).await.expect("accept ws");
        let _ = flow_daemon::channel::handshake::respond_to_pairing(&mut channel, |_req| {
            flow_core::pairing::PairingDecision::Reject
        })
        .await;
    });

    service_a
        .note_discovered_peer(DiscoveredPeer {
            name: "Studio C".to_string(),
            os: HostOs::Linux,
            address: ChannelAddress::Tcp(b_addr),
        })
        .await;

    let mut sessions = service_a.watch_pairing_session();
    let _ = sessions.borrow_and_update();
    service_a.start_pairing().await.expect("start pairing");

    sessions.changed().await.expect("searching");
    sessions.changed().await.expect("found");
    let candidate = sessions
        .borrow_and_update()
        .candidates
        .iter()
        .find(|c| c.name == "Studio C")
        .expect("the live-discovered candidate is offered")
        .clone();

    service_a
        .pair_with_candidate(&candidate.id)
        .await
        .expect("pair with the live candidate");

    sessions.changed().await.expect("requesting");
    sessions.changed().await.expect("failed");
    let failed = sessions.borrow_and_update().clone();
    assert_eq!(failed.stage, PairingStage::Failed);
    assert!(failed.error.is_some());

    let a_devices = service_a.watch_devices().borrow().clone();
    assert!(!a_devices.iter().any(|d| d.name == "Studio C"));
}
