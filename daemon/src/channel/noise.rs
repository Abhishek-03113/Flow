//! [`NoiseChannel`]: an encrypted `Channel` decorator (`daemon/todos.json`
//! H3) wrapping any other `Channel` with an authenticated [Noise
//! protocol](http://www.noiseprotocol.org/) session (the `snow` crate —
//! confined to this module per the wrap-third-party-deps rule), so
//! `ChannelMessage`s never travel in plaintext between daemons.
//!
//! ## Design note: identity binding without unsafe key reuse
//!
//! `H1`'s `DeviceIdentity` is an ed25519 *signing* key; Noise's `XX`
//! pattern needs an X25519 *Diffie-Hellman* key. These are different
//! cryptographic primitives — deriving one from the other needs a
//! birational-map conversion this codebase doesn't implement (and
//! shouldn't hand-roll for a security-critical path without a
//! well-audited library to lean on). So `NoiseChannel` generates a
//! fresh, ephemeral X25519 static keypair per connection (via `snow`'s
//! own keygen) for the Noise handshake itself — this gives forward
//! secrecy and an encrypted channel, but the Noise handshake alone does
//! *not* prove which `H1` identity is on the other end.
//!
//! To still tie the session to `H1` identity — this task's own stated
//! intent ("layers an authenticated Noise protocol handshake keyed by
//! each side's H1 identity") — the first message sent over the
//! freshly-established transport, in both directions, is an identity
//! proof: each side signs the Noise handshake hash
//! (`get_handshake_hash()`, a transcript unique to this one session,
//! per Noise's own anti-replay guarantee) with its `H1` `DeviceIdentity`
//! and sends `{public_key, signature}`. The peer verifies it before the
//! channel is usable and exposes the proven identity via
//! [`NoiseChannel::peer_identity`] — what `H4`'s trust gate consults.
//! This is a reasonable, standard technique (binding a DH handshake to
//! a separate signing identity via a transcript signature), but it has
//! not been independently security-reviewed; treat it as this project's
//! own best-effort implementation, not a substitute for a professional
//! cryptographic audit.

use ed25519_dalek::{Signature, VerifyingKey};
use flow_core::channel::{Channel, ChannelError, ChannelKind, ChannelMessage};
use serde::{Deserialize, Serialize};
use snow::{Builder, TransportState};

use crate::identity::{self, DeviceIdentity};

const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// The identity proof exchanged as the first message over a freshly
/// established Noise transport: this side's `H1` public key and a
/// signature over the handshake transcript, proving possession of the
/// matching private key for *this specific session* (the handshake hash
/// differs every connection, so a captured proof can't be replayed on a
/// different one).
#[derive(Serialize, Deserialize)]
struct IdentityProof {
    public_key: [u8; 32],
    // `Vec<u8>` rather than `[u8; 64]`: this serde version's derive only
    // supports fixed-size array (de)serialization up to length 32.
    signature: Vec<u8>,
}

/// A `Channel` decorator that encrypts and authenticates every message
/// sent over some inner `Channel`. Works identically whether that inner
/// `Channel` is a `TcpChannel` or a `BluetoothChannel` — this module
/// names neither, only the `Channel` trait, per `docs/architecture/channels.md`'s
/// design.
pub struct NoiseChannel<C: Channel> {
    inner: C,
    transport: TransportState,
    peer_identity: VerifyingKey,
}

impl<C: Channel> NoiseChannel<C> {
    /// Initiator side: performs the Noise handshake and identity-proof
    /// exchange over `inner`, keyed by `local_identity`.
    pub async fn initiate(inner: C, local_identity: &DeviceIdentity) -> Result<Self, ChannelError> {
        Self::handshake(inner, local_identity, true).await
    }

    /// Responder side.
    pub async fn accept(inner: C, local_identity: &DeviceIdentity) -> Result<Self, ChannelError> {
        Self::handshake(inner, local_identity, false).await
    }

    /// The `H1` public key `local_identity`'s peer proved it holds,
    /// verified against this specific session's handshake transcript.
    /// What `H4`'s trust gate checks against `P4`'s device repository.
    pub fn peer_identity(&self) -> VerifyingKey {
        self.peer_identity
    }

    async fn handshake(
        mut inner: C,
        local_identity: &DeviceIdentity,
        is_initiator: bool,
    ) -> Result<Self, ChannelError> {
        let params: snow::params::NoiseParams = NOISE_PARAMS
            .parse()
            .expect("NOISE_PARAMS is a valid, fixed Noise pattern string");
        let builder = Builder::new(params);
        let static_key = builder.generate_keypair().map_err(noise_err)?.private;
        let builder = builder.local_private_key(&static_key).map_err(noise_err)?;
        let mut handshake = if is_initiator {
            builder.build_initiator().map_err(noise_err)?
        } else {
            builder.build_responder().map_err(noise_err)?
        };

        let mut buf = vec![0u8; 65535];
        if is_initiator {
            // -> e
            let len = handshake.write_message(&[], &mut buf).map_err(noise_err)?;
            send_frame(&mut inner, &buf[..len]).await?;
            // <- e, ee, s, es
            let msg = recv_frame(&mut inner).await?;
            handshake.read_message(&msg, &mut buf).map_err(noise_err)?;
            // -> s, se
            let len = handshake.write_message(&[], &mut buf).map_err(noise_err)?;
            send_frame(&mut inner, &buf[..len]).await?;
        } else {
            // <- e
            let msg = recv_frame(&mut inner).await?;
            handshake.read_message(&msg, &mut buf).map_err(noise_err)?;
            // -> e, ee, s, es
            let len = handshake.write_message(&[], &mut buf).map_err(noise_err)?;
            send_frame(&mut inner, &buf[..len]).await?;
            // <- s, se
            let msg = recv_frame(&mut inner).await?;
            handshake.read_message(&msg, &mut buf).map_err(noise_err)?;
        }

        let handshake_hash = handshake.get_handshake_hash().to_vec();
        let mut transport = handshake.into_transport_mode().map_err(noise_err)?;

        let proof = IdentityProof {
            public_key: local_identity.public_key_bytes(),
            signature: local_identity.sign(&handshake_hash).to_bytes().to_vec(),
        };
        let proof_bytes = serde_json::to_vec(&proof)
            .map_err(|err| ChannelError::Serialization(err.to_string()))?;
        let ciphertext = encrypt(&mut transport, &proof_bytes)?;
        send_frame(&mut inner, &ciphertext).await?;

        let received_ciphertext = recv_frame(&mut inner).await?;
        let received_plaintext = decrypt(&mut transport, &received_ciphertext)?;
        let peer_proof: IdentityProof = serde_json::from_slice(&received_plaintext)
            .map_err(|err| ChannelError::Serialization(err.to_string()))?;

        let peer_public_key = VerifyingKey::from_bytes(&peer_proof.public_key)
            .map_err(|_| ChannelError::AuthenticationFailed)?;
        let signature_bytes: [u8; 64] = peer_proof
            .signature
            .try_into()
            .map_err(|_| ChannelError::AuthenticationFailed)?;
        let peer_signature = Signature::from_bytes(&signature_bytes);
        if !identity::verify(&peer_public_key, &handshake_hash, &peer_signature) {
            return Err(ChannelError::AuthenticationFailed);
        }

        Ok(Self {
            inner,
            transport,
            peer_identity: peer_public_key,
        })
    }
}

/// Sends one handshake or transport frame over `inner` as an opaque
/// `ChannelMessage::Noise` — the inner `Channel` already frames whole
/// messages (a WebSocket text frame, an RFCOMM length-prefixed frame),
/// so nothing extra is needed here.
async fn send_frame<C: Channel>(inner: &mut C, bytes: &[u8]) -> Result<(), ChannelError> {
    inner.send(ChannelMessage::Noise(bytes.to_vec())).await
}

/// Receives one handshake or transport frame. Anything that isn't a
/// `ChannelMessage::Noise` frame during the handshake or transport
/// phases is a protocol violation, not tolerable traffic — unlike
/// `channel::handshake`'s pairing exchange (which shares a connection
/// with other message kinds and can just skip them), a `NoiseChannel`
/// *is* the connection at this point, so anything else means the peer
/// isn't speaking this protocol.
async fn recv_frame<C: Channel>(inner: &mut C) -> Result<Vec<u8>, ChannelError> {
    match inner.recv().await? {
        ChannelMessage::Noise(bytes) => Ok(bytes),
        _ => Err(ChannelError::AuthenticationFailed),
    }
}

fn encrypt(transport: &mut TransportState, plaintext: &[u8]) -> Result<Vec<u8>, ChannelError> {
    // Noise appends a 16-byte authentication tag to every transport
    // message.
    let mut buf = vec![0u8; plaintext.len() + 16];
    let len = transport
        .write_message(plaintext, &mut buf)
        .map_err(noise_err)?;
    buf.truncate(len);
    Ok(buf)
}

fn decrypt(transport: &mut TransportState, ciphertext: &[u8]) -> Result<Vec<u8>, ChannelError> {
    let mut buf = vec![0u8; ciphertext.len()];
    let len = transport
        .read_message(ciphertext, &mut buf)
        .map_err(noise_err)?;
    buf.truncate(len);
    Ok(buf)
}

/// Maps any `snow` failure — a corrupt handshake message, a failed
/// decryption — onto `AuthenticationFailed`. Noise itself doesn't
/// distinguish "malformed" from "tampered with," so neither does this;
/// conflating them defensively is intentional.
fn noise_err(_: snow::Error) -> ChannelError {
    ChannelError::AuthenticationFailed
}

#[async_trait::async_trait]
impl<C: Channel> Channel for NoiseChannel<C> {
    fn kind(&self) -> ChannelKind {
        self.inner.kind()
    }

    async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError> {
        let plaintext =
            serde_json::to_vec(&msg).map_err(|err| ChannelError::Serialization(err.to_string()))?;
        let ciphertext = encrypt(&mut self.transport, &plaintext)?;
        send_frame(&mut self.inner, &ciphertext).await
    }

    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError> {
        let ciphertext = recv_frame(&mut self.inner).await?;
        let plaintext = decrypt(&mut self.transport, &ciphertext)?;
        serde_json::from_slice(&plaintext)
            .map_err(|err| ChannelError::Serialization(err.to_string()))
    }

    async fn close(&mut self) -> Result<(), ChannelError> {
        self.inner.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use crate::storage::Storage;
    use flow_core::protocol::{InputEvent, KeyboardEvent};
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
    async fn a_handshake_completes_and_each_side_proves_the_others_h1_identity() {
        let (tcp_a, tcp_b) = connected_tcp_pair().await;
        let identity_a = an_identity().await;
        let identity_b = an_identity().await;

        let responder = {
            let identity_b = identity_b.clone();
            tokio::spawn(async move { NoiseChannel::accept(tcp_b, &identity_b).await })
        };
        let initiator = NoiseChannel::initiate(tcp_a, &identity_a)
            .await
            .expect("initiator handshake");
        let responder = responder
            .await
            .expect("responder task")
            .expect("responder handshake");

        assert_eq!(initiator.peer_identity(), identity_b.public_key());
        assert_eq!(responder.peer_identity(), identity_a.public_key());
    }

    #[tokio::test]
    async fn a_message_sent_over_the_noise_channel_arrives_intact() {
        let (tcp_a, tcp_b) = connected_tcp_pair().await;
        let identity_a = an_identity().await;
        let identity_b = an_identity().await;

        let responder = {
            let identity_b = identity_b.clone();
            tokio::spawn(async move {
                let mut channel = NoiseChannel::accept(tcp_b, &identity_b)
                    .await
                    .expect("accept");
                channel.recv().await.expect("recv")
            })
        };
        let mut initiator = NoiseChannel::initiate(tcp_a, &identity_a)
            .await
            .expect("initiate");

        let sent = ChannelMessage::Input(InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "A".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }));
        initiator.send(sent.clone()).await.expect("send");

        let received = responder.await.expect("responder task");
        assert_eq!(received, sent);
    }

    #[tokio::test]
    async fn reports_the_wrapped_channels_kind() {
        let (tcp_a, tcp_b) = connected_tcp_pair().await;
        let identity_a = an_identity().await;
        let identity_b = an_identity().await;

        tokio::spawn(async move {
            let _ = NoiseChannel::accept(tcp_b, &identity_b).await;
        });
        let initiator = NoiseChannel::initiate(tcp_a, &identity_a)
            .await
            .expect("initiate");
        assert_eq!(initiator.kind(), ChannelKind::Tcp);
    }
}
