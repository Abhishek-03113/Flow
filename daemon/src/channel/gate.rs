//! The connection-accept gate (`daemon/todos.json` H4): rejects an
//! incoming connection whose Noise-handshake-proven `H1` identity isn't
//! already trusted (`H2`, backed by `P4`'s device repository) — before
//! it can ever reach the input-streaming pipeline (`G8`) or exchange a
//! single `ChannelMessage::Input`. Medium-agnostic by construction: it's
//! generic over `Channel`, not any concrete `TcpChannel`/`BluetoothChannel`.
//!
//! Sits one level above `channel::noise`: an unauthenticated connection
//! can't be checked against the trust store at all (it has no proven
//! identity yet), so this gate runs `NoiseChannel::accept` first — the
//! Noise handshake plus its identity-proof exchange (`H3`) — and only
//! *then* consults `H2`. That ordering is why the originally-drafted
//! task description ("reject... before the Noise handshake completes")
//! isn't literally achievable: there is no peer identity to check before
//! the handshake produces one. What *is* achievable, and what this
//! module delivers, is this task's own acceptance criterion: an
//! untrusted peer is rejected before any `InputEvent` can flow.

use flow_core::channel::{Channel, ChannelError};

use crate::channel::noise::NoiseChannel;
use crate::identity::DeviceIdentity;
use crate::trust::TrustGate;

/// Completes the Noise handshake over `inner` and checks the resulting
/// peer identity against `trust` before returning a usable channel. An
/// untrusted peer's connection is refused here — the caller never gets
/// a channel it could call `recv()` on, so no `InputEvent` from an
/// unrecognized device can ever reach the pipeline.
pub async fn accept_trusted<C: Channel>(
    inner: C,
    local_identity: &DeviceIdentity,
    trust: &TrustGate,
) -> Result<NoiseChannel<C>, ChannelError> {
    let channel = NoiseChannel::accept(inner, local_identity).await?;
    if !trust.is_trusted(&channel.peer_identity().to_bytes()).await {
        return Err(ChannelError::AuthenticationFailed);
    }
    Ok(channel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use crate::storage::device_repo::{DeviceRecord, DeviceRepo};
    use crate::storage::Storage;
    use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
    use tokio::net::TcpListener;

    async fn connected_tcp_pair() -> (TcpChannel, TcpChannel) {
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

    async fn an_identity() -> DeviceIdentity {
        let storage = Storage::open_in_memory().await.expect("open db");
        DeviceIdentity::load_or_generate(storage).await
    }

    #[tokio::test]
    async fn a_connection_from_a_trusted_identity_is_accepted() {
        let (tcp_client, tcp_server) = connected_tcp_pair().await;
        let identity_a = an_identity().await;
        let identity_b = an_identity().await;

        let trust_storage = Storage::open_in_memory().await.expect("open trust db");
        DeviceRepo::new(trust_storage.clone())
            .upsert(DeviceRecord {
                device: Device {
                    id: DeviceId("peer-a".to_string()),
                    name: "Device A".to_string(),
                    os: HostOs::Linux,
                    state: DeviceState::Inactive,
                    last_seen: chrono::Utc::now(),
                },
                public_key: Some(identity_a.public_key_bytes().to_vec()),
                removable: true,
            })
            .await;
        let trust = TrustGate::new(trust_storage);

        tokio::spawn(async move {
            let _ = NoiseChannel::initiate(tcp_client, &identity_a).await;
        });

        let accepted = accept_trusted(tcp_server, &identity_b, &trust).await;
        assert!(accepted.is_ok(), "a trusted peer must be accepted");
    }

    #[tokio::test]
    async fn a_connection_from_an_untrusted_identity_is_rejected_before_any_input_can_flow() {
        let (tcp_client, tcp_server) = connected_tcp_pair().await;
        let identity_a = an_identity().await;
        let identity_b = an_identity().await;

        // Empty trust store - identity_a was never paired.
        let trust_storage = Storage::open_in_memory().await.expect("open trust db");
        let trust = TrustGate::new(trust_storage);

        tokio::spawn(async move {
            let _ = NoiseChannel::initiate(tcp_client, &identity_a).await;
        });

        let accepted = accept_trusted(tcp_server, &identity_b, &trust).await;
        assert_eq!(accepted.err(), Some(ChannelError::AuthenticationFailed));
    }
}
