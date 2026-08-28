//! End-to-end pairing over a real `Channel` (`daemon/todos.json` G7):
//! two independent `DaemonService` instances complete a real
//! `pair_with_candidate` handshake over an actual `TcpChannel`
//! (loopback), each ending up with the other in its own devices list —
//! this task's own stated acceptance criterion, exercised against
//! genuine sockets rather than the in-memory `ChannelPair` test double
//! `G1`'s own tests use.

use flow_core::channel::{Channel, ChannelAddress};
use flow_core::device::HostOs;
use flow_core::pairing::{PairingDecision, PairingStage};
use flow_daemon::channel::noise::NoiseChannel;
use flow_daemon::channel::tcp::TcpChannel;
use flow_daemon::discovery::DiscoveredPeer;
use flow_daemon::identity::DeviceIdentity;
use flow_daemon::service::DaemonService;
use flow_daemon::storage::device_repo::DeviceRepo;
use flow_daemon::storage::Storage;
use tokio::net::TcpListener;

/// A `DaemonService` alongside the `Storage` handle it was built on, so a
/// test can look past the service's own façade at what actually landed
/// in the device repository (the real acceptance criterion for the
/// review's identity/trust fix: not just "pairing still works," but
/// "the peer's real public key got persisted, not None").
#[derive(Clone)]
struct TestDaemon {
    service: DaemonService,
    storage: Storage,
}

async fn service() -> TestDaemon {
    let storage = Storage::open_in_memory().await.expect("open in-memory db");
    // The mock-parity fixture, deliberately: this test's own name-collision
    // assertion below relies on both sides' local device sharing the seed's
    // hardcoded "MacBook" name.
    let service = DaemonService::new_seeded_for_test(storage.clone()).await;
    TestDaemon { service, storage }
}

/// A standalone identity for a test's own hand-rolled responder task,
/// independent of any `DaemonService` — the real responder path
/// (`DaemonService::accept_pairing_request`) uses the service's own
/// persisted identity, but a test driving the wire protocol directly
/// just needs *a* valid one to complete the Noise handshake with.
async fn a_standalone_identity() -> DeviceIdentity {
    let storage = Storage::open_in_memory().await.expect("open identity db");
    DeviceIdentity::load_or_generate(storage).await
}

#[tokio::test]
async fn two_daemons_complete_a_real_pairing_handshake_over_tcp() {
    let TestDaemon {
        service: service_a,
        storage: storage_a,
    } = service().await;
    let TestDaemon {
        service: service_b,
        storage: storage_b,
    } = service().await;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind b's listener");
    let b_addr = listener.local_addr().expect("local addr");

    // The responder only surfaces an incoming request while a UI is
    // connected to prompt (`DaemonService::register_ipc_client`) — an
    // unattended daemon rejects outright, which is what stops any host on
    // the network from pairing itself in uninvited. Hold the guard for
    // the rest of the test to keep B's "UI" connected.
    let _b_ui = service_b.register_ipc_client();

    let responder = {
        let service_b = service_b.clone();
        tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.expect("accept");
            let channel = TcpChannel::accept(stream).await.expect("accept ws");
            let channel: Box<dyn Channel> = Box::new(channel);
            service_b
                .accept_pairing_request(channel, Some(peer))
                .await
                .expect("responder side of the handshake");
        })
    };

    // B's UI: watch for the incoming pairing request the daemon publishes
    // and accept it on the user's behalf, unblocking the handshake.
    let ui_b = {
        let service_b = service_b.clone();
        tokio::spawn(async move {
            let mut reqs = service_b.watch_incoming_request();
            loop {
                // Bind the clone in its own statement so the non-`Send`
                // `watch::Ref` is dropped before the `.await` below
                // (edition-2021 `if let` scrutinee temporaries otherwise
                // live to the end of the block).
                let current = reqs.borrow_and_update().clone();
                if let Some(req) = current {
                    service_b
                        .respond_to_pairing_request(&req.request_id, PairingDecision::Accept)
                        .await
                        .expect("ui accepts");
                    return;
                }
                reqs.changed().await.expect("incoming request stream");
            }
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
    ui_b.await.expect("ui task");

    let a_devices = service_a.watch_devices().borrow().clone();
    let b_in_a = a_devices
        .iter()
        .find(|d| d.name == "Studio B")
        .unwrap_or_else(|| panic!("device A's list should contain device B: {a_devices:?}"));

    // Found by id prefix, not by name: B's own seed data independently
    // includes a local device also named "MacBook" (every fresh
    // DaemonService seeds one) — a real name collision between "B's own
    // idea of itself" and "the actual remote device A," exactly the
    // review's point that a name is never a safe way to pick out a
    // specific device. Only the newly-paired entry's id carries the
    // "pk:" prefix this fix adds.
    let b_devices = service_b.watch_devices().borrow().clone();
    let a_in_b = b_devices
        .iter()
        .find(|d| d.id.0.starts_with("pk:"))
        .unwrap_or_else(|| panic!("device B's list should contain device A: {b_devices:?}"));
    assert_eq!(a_in_b.name, "MacBook");

    // The actual fix this test now proves, not just "pairing still
    // works": each side's identity for the other is the real, proven H1
    // public key from the Noise handshake (`pk:<hex>`), and that same
    // key — not None — is what's sitting in the device repository, which
    // is what H2's trust gate will check on any future connection.
    assert!(
        b_in_a.id.0.starts_with("pk:"),
        "device B's id on A's side should be its public key, not its name: {}",
        b_in_a.id.0
    );
    assert!(
        a_in_b.id.0.starts_with("pk:"),
        "device A's id on B's side should be its public key, not its name: {}",
        a_in_b.id.0
    );

    let b_record = DeviceRepo::new(storage_a)
        .find_by_id(b_in_a.id.clone())
        .await
        .expect("device B's record persisted on A's side");
    assert_eq!(
        b_record.public_key.as_ref().map(Vec::len),
        Some(32),
        "device B's real 32-byte ed25519 public key should be persisted, not None"
    );

    let a_record = DeviceRepo::new(storage_b)
        .find_by_id(a_in_b.id.clone())
        .await
        .expect("device A's record persisted on B's side");
    assert_eq!(
        a_record.public_key.as_ref().map(Vec::len),
        Some(32),
        "device A's real 32-byte ed25519 public key should be persisted, not None"
    );
}

/// The mirror-image failure path: the responder rejects, so the
/// initiator's session lands in `Failed` (not `Paired`) and neither
/// side gains a new device.
#[tokio::test]
async fn a_rejected_real_pairing_request_lands_in_failed_not_paired() {
    let TestDaemon {
        service: service_a, ..
    } = service().await;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind b's listener");
    let b_addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await.expect("accept");
        let channel = TcpChannel::accept(stream).await.expect("accept ws");
        let identity = a_standalone_identity().await;
        let mut noise_channel = NoiseChannel::accept(channel, &identity)
            .await
            .expect("responder side of the Noise handshake");
        let _ = flow_daemon::channel::handshake::respond_to_pairing(&mut noise_channel, |_req| {
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

/// The same real two-`DaemonService` wiring as
/// `two_daemons_complete_a_real_pairing_handshake_over_tcp`, but B's UI
/// *rejects* the incoming request via `respond_to_pairing_request`: the
/// initiator's session lands in `Failed` (not `Paired`) and B never
/// persists the initiator as a device.
#[tokio::test]
async fn a_rejected_incoming_request_fails_the_initiator() {
    let TestDaemon {
        service: service_a, ..
    } = service().await;
    let TestDaemon {
        service: service_b,
        storage: storage_b,
    } = service().await;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind b's listener");
    let b_addr = listener.local_addr().expect("local addr");

    // A connected UI is what makes the daemon surface the request for a
    // decision rather than rejecting it outright; keep the guard alive.
    let _b_ui = service_b.register_ipc_client();

    let responder = {
        let service_b = service_b.clone();
        tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.expect("accept");
            let channel = TcpChannel::accept(stream).await.expect("accept ws");
            let channel: Box<dyn Channel> = Box::new(channel);
            service_b
                .accept_pairing_request(channel, Some(peer))
                .await
                .expect("responder side of the handshake");
        })
    };

    // B's UI: watch for the incoming pairing request and reject it.
    let ui_b = {
        let service_b = service_b.clone();
        tokio::spawn(async move {
            let mut reqs = service_b.watch_incoming_request();
            loop {
                // See the accept-path test for why the clone is bound in
                // its own statement here.
                let current = reqs.borrow_and_update().clone();
                if let Some(req) = current {
                    service_b
                        .respond_to_pairing_request(&req.request_id, PairingDecision::Reject)
                        .await
                        .expect("ui rejects");
                    return;
                }
                reqs.changed().await.expect("incoming request stream");
            }
        })
    };

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
    sessions.changed().await.expect("found");
    let candidate = sessions
        .borrow_and_update()
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
    sessions.changed().await.expect("failed");
    let failed = sessions.borrow_and_update().clone();
    assert_eq!(failed.stage, PairingStage::Failed);
    assert!(failed.error.is_some());

    responder.await.expect("responder task");
    ui_b.await.expect("ui task");

    // B never turned the rejected initiator into a trusted device: no
    // public-key-keyed record in the repo, nothing new in the list.
    let b_records = DeviceRepo::new(storage_b).list().await;
    assert!(
        !b_records.iter().any(|r| r.device.id.0.starts_with("pk:")),
        "B must not persist a rejected initiator: {b_records:?}"
    );
    let b_devices = service_b.watch_devices().borrow().clone();
    assert!(
        !b_devices.iter().any(|d| d.id.0.starts_with("pk:")),
        "B's devices list must not gain the rejected initiator: {b_devices:?}"
    );
}
