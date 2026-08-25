//! The pairing handshake (`daemon/todos.json` G7): the `PairingRequest`/
//! `PairingDecision` exchange carried as `ChannelMessage::Pairing`
//! frames. Written once against the `Channel` trait — everything here
//! takes `&mut dyn Channel` and never inspects `ChannelKind` — so the
//! same code runs the handshake whether the underlying connection is a
//! `TcpChannel` or a `BluetoothChannel`, per
//! `docs/architecture/channels.md`'s stated design goal for everything
//! above `channel::negotiate`.

use flow_core::channel::{Channel, ChannelError, ChannelMessage, PairingWireMessage};
use flow_core::pairing::{PairingDecision, PairingRequest};

/// Initiator side (`DaemonService::pair_with_candidate`'s real-handshake
/// path): sends `request` and waits for the peer's decision. A
/// non-pairing message received while waiting (e.g. a stray
/// `ChannelMessage::Heartbeat`) is ignored rather than treated as a
/// protocol error — this connection may carry other traffic once G8's
/// input streaming shares it.
pub async fn request_pairing(
    channel: &mut dyn Channel,
    request: PairingRequest,
) -> Result<PairingDecision, ChannelError> {
    channel
        .send(ChannelMessage::Pairing(PairingWireMessage::Request(
            request,
        )))
        .await?;
    loop {
        if let ChannelMessage::Pairing(PairingWireMessage::Decision(decision)) =
            channel.recv().await?
        {
            return Ok(decision);
        }
    }
}

/// Responder side (`DaemonService::accept_pairing_request`): waits for
/// the peer's `PairingRequest`, decides via `decide`, and sends the
/// corresponding `PairingDecision` back. Returns the request alongside
/// the decision so the caller can record who asked regardless of the
/// outcome.
pub async fn respond_to_pairing(
    channel: &mut dyn Channel,
    decide: impl FnOnce(&PairingRequest) -> PairingDecision,
) -> Result<(PairingRequest, PairingDecision), ChannelError> {
    loop {
        if let ChannelMessage::Pairing(PairingWireMessage::Request(request)) =
            channel.recv().await?
        {
            let decision = decide(&request);
            channel
                .send(ChannelMessage::Pairing(PairingWireMessage::Decision(
                    decision,
                )))
                .await?;
            return Ok((request, decision));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use flow_core::device::HostOs;
    use tokio::net::TcpListener;

    /// A real, loopback-connected `TcpChannel` pair — the same pattern
    /// `channel::tcp`'s own tests use — so these tests exercise the
    /// handshake over an actual `Channel` implementation, not a
    /// hand-rolled test double.
    async fn connected_pair() -> (TcpChannel, TcpChannel) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            TcpChannel::accept(stream).await.expect("accept ws")
        });
        let client = TcpChannel::connect(addr).await.expect("connect");
        let server = server.await.expect("server task");
        (client, server)
    }

    fn a_request() -> PairingRequest {
        PairingRequest {
            device_name: "Laptop".to_string(),
            device_os: HostOs::Linux,
            address: String::new(),
        }
    }

    #[tokio::test]
    async fn an_accepted_request_round_trips_the_decision_back_to_the_initiator() {
        let (mut initiator, mut responder) = connected_pair().await;
        let request = a_request();

        let responder_task = tokio::spawn(async move {
            respond_to_pairing(&mut responder, |_req| PairingDecision::Accept).await
        });

        let decision = request_pairing(&mut initiator, request.clone())
            .await
            .expect("decision");
        assert_eq!(decision, PairingDecision::Accept);

        let (received_request, sent_decision) = responder_task
            .await
            .expect("responder task")
            .expect("responded");
        assert_eq!(received_request, request);
        assert_eq!(sent_decision, PairingDecision::Accept);
    }

    #[tokio::test]
    async fn a_rejected_request_is_reported_to_the_initiator() {
        let (mut initiator, mut responder) = connected_pair().await;

        tokio::spawn(async move {
            let _ = respond_to_pairing(&mut responder, |_req| PairingDecision::Reject).await;
        });

        let decision = request_pairing(&mut initiator, a_request())
            .await
            .expect("decision");
        assert_eq!(decision, PairingDecision::Reject);
    }

    #[tokio::test]
    async fn a_stray_heartbeat_before_the_decision_is_ignored_by_the_initiator() {
        let (mut initiator, mut responder) = connected_pair().await;

        let responder_task = tokio::spawn(async move {
            responder
                .send(ChannelMessage::Heartbeat)
                .await
                .expect("send stray heartbeat");
            respond_to_pairing(&mut responder, |_req| PairingDecision::Accept).await
        });

        let decision = request_pairing(&mut initiator, a_request())
            .await
            .expect("decision");
        assert_eq!(decision, PairingDecision::Accept);
        responder_task
            .await
            .expect("responder task")
            .expect("responded");
    }
}
