//! Dev-only stand-in for [`crate::channel::noise::NoiseChannel`],
//! selected by `FLOW_SECURITY=insecure` (guarded by `FLOW_DEV=1`).
//!
//! **No encryption and no identity proof.** Each side sends its ed25519
//! public key over the transport in the clear, reads the peer's, and the
//! raw `Channel` is then used unchanged for every subsequent message.
//! The peer key is *claimed*, never verified — a peer can send whatever
//! 32 bytes it likes. This exists only to take encryption and the Noise
//! handshake out of the picture while the two-daemon streaming path is
//! brought up on one machine; it must never run outside local dev.

use flow_core::channel::{Channel, ChannelError, ChannelMessage};

use crate::identity::DeviceIdentity;

/// Swap identity public keys over `channel` and hand it back untouched.
/// Both ends send before either reads, so there is no initiator /
/// responder split and no deadlock — the transport buffers the write.
/// The key frame reuses [`ChannelMessage::Noise`] (the "raw bytes for
/// session establishment" variant) so the wire framing matches the
/// encrypted path's.
pub async fn exchange(
    mut channel: Box<dyn Channel>,
    identity: &DeviceIdentity,
) -> Result<(Box<dyn Channel>, [u8; 32]), ChannelError> {
    channel
        .send(ChannelMessage::Noise(identity.public_key_bytes().to_vec()))
        .await?;

    match channel.recv().await? {
        ChannelMessage::Noise(bytes) => {
            let peer_key: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| ChannelError::AuthenticationFailed)?;
            crate::hop_note!(
                stage = "insecure_key_swap",
                "plaintext dev handshake: exchanged CLAIMED identity keys, NO encryption"
            );
            Ok((channel, peer_key))
        }
        // Anything else at this point is a peer not speaking the dev
        // handshake — fail rather than misinterpret it.
        _ => Err(ChannelError::AuthenticationFailed),
    }
}
