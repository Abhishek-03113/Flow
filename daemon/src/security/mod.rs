//! The peer-connection security profile the daemon runs with, chosen
//! once at startup from `FLOW_SECURITY` (see [`crate::devmode`]).
//!
//! **Nothing about the secure path was removed to add the dev one.** The
//! `Secure` arm is the exact `NoiseChannel` + trust-store + user-consent
//! flow that was here before; `Insecure` is a sibling arm selected at
//! runtime, compiled in alongside it and reachable only with
//! `FLOW_SECURITY=insecure` *and* `FLOW_DEV=1`. Its purpose is narrow:
//! take encryption, the identity handshake, the trust gate and the
//! pairing prompt out of the picture so two headless daemons on one
//! machine can be brought up and watched end to end, then switch back to
//! `Secure` for anything real.

mod plaintext;

use flow_core::channel::{Channel, ChannelError};

use crate::channel::noise::NoiseChannel;
use crate::identity::DeviceIdentity;

pub use crate::devmode::SecurityMode;

/// The resolved security profile. Cheap to copy — it is just the mode
/// tag; the behaviour hangs off `match`es on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Security {
    mode: SecurityMode,
}

impl Security {
    /// The production profile: real Noise encryption, ed25519 identity
    /// proof, trust-store gate, user pairing consent.
    pub fn secure() -> Self {
        Self {
            mode: SecurityMode::Secure,
        }
    }

    /// Build from the mode `devmode` resolved from the environment.
    pub fn from_mode(mode: SecurityMode) -> Self {
        Self { mode }
    }

    pub fn mode(&self) -> SecurityMode {
        self.mode
    }

    /// Short label for `flow::hop` records and the startup banner.
    pub fn label(&self) -> &'static str {
        match self.mode {
            SecurityMode::Secure => "secure",
            SecurityMode::Insecure => "insecure-dev",
        }
    }

    /// Establish the daemon-to-daemon session as the **initiator** over
    /// an already-negotiated transport `channel`. Returns the session
    /// channel — encrypted under `Secure`, the raw transport under
    /// `Insecure` — and the peer's 32-byte ed25519 identity key, *proven*
    /// under `Secure` and merely *claimed* under `Insecure`.
    pub async fn initiate(
        &self,
        channel: Box<dyn Channel>,
        identity: &DeviceIdentity,
    ) -> Result<(Box<dyn Channel>, [u8; 32]), ChannelError> {
        match self.mode {
            SecurityMode::Secure => {
                let noise = NoiseChannel::initiate(channel, identity).await?;
                let peer_key = noise.peer_identity().to_bytes();
                Ok((Box::new(noise), peer_key))
            }
            SecurityMode::Insecure => plaintext::exchange(channel, identity).await,
        }
    }

    /// The **responder** counterpart to [`Self::initiate`].
    pub async fn accept(
        &self,
        channel: Box<dyn Channel>,
        identity: &DeviceIdentity,
    ) -> Result<(Box<dyn Channel>, [u8; 32]), ChannelError> {
        match self.mode {
            SecurityMode::Secure => {
                let noise = NoiseChannel::accept(channel, identity).await?;
                let peer_key = noise.peer_identity().to_bytes();
                Ok((Box::new(noise), peer_key))
            }
            SecurityMode::Insecure => plaintext::exchange(channel, identity).await,
        }
    }

    /// Whether the trust-store check is bypassed — every peer that
    /// completes [`Self::accept`]/[`Self::initiate`] is treated as
    /// already paired. `Insecure` only.
    pub fn trust_bypassed(&self) -> bool {
        matches!(self.mode, SecurityMode::Insecure)
    }

    /// Whether an incoming pairing request is auto-accepted with no UI
    /// prompt and no consent window. `Insecure` only.
    pub fn pairing_auto_accepts(&self) -> bool {
        matches!(self.mode, SecurityMode::Insecure)
    }
}

impl Default for Security {
    fn default() -> Self {
        Self::secure()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use crate::storage::Storage;
    use flow_core::channel::ChannelMessage;
    use flow_core::protocol::{InputEvent, KeyboardEvent};
    use tokio::net::TcpListener;

    #[test]
    fn secure_profile_gates_everything() {
        let s = Security::secure();
        assert_eq!(s.label(), "secure");
        assert!(!s.trust_bypassed());
        assert!(!s.pairing_auto_accepts());
        assert_eq!(Security::default(), Security::secure());
    }

    #[test]
    fn insecure_profile_opens_everything() {
        let s = Security::from_mode(SecurityMode::Insecure);
        assert_eq!(s.label(), "insecure-dev");
        assert!(s.trust_bypassed());
        assert!(s.pairing_auto_accepts());
    }

    async fn connected_tcp_pair() -> (TcpChannel, TcpChannel) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            TcpChannel::accept(stream).await.expect("ws accept")
        });
        let client = TcpChannel::connect(addr).await.expect("connect");
        (client, server.await.expect("server task"))
    }

    async fn an_identity() -> DeviceIdentity {
        let storage = Storage::open_in_memory().await.expect("open db");
        DeviceIdentity::load_or_generate(storage).await
    }

    #[tokio::test]
    async fn insecure_exchange_swaps_real_claimed_keys_and_leaves_the_channel_usable() {
        let (client_tcp, server_tcp) = connected_tcp_pair().await;
        let id_a = an_identity().await;
        let id_b = an_identity().await;
        let sec = Security::from_mode(SecurityMode::Insecure);

        let (id_b_key, sec_b) = (id_b.public_key_bytes(), sec);
        let server = tokio::spawn(async move {
            sec_b
                .accept(Box::new(server_tcp), &id_b)
                .await
                .map(|(mut ch, key)| async move {
                    // The raw channel must still carry ordinary traffic
                    // after the key swap.
                    let msg = ch.recv().await.expect("recv input");
                    (key, msg)
                })
        });

        let (mut channel_a, claimed_b) = sec
            .initiate(Box::new(client_tcp), &id_a)
            .await
            .expect("initiate");
        assert_eq!(claimed_b, id_b_key, "A learns B's claimed key");

        let event = ChannelMessage::Input {
            sequence: 1,
            event: InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "H".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            }),
        };
        channel_a
            .send(event.clone())
            .await
            .expect("send after swap");

        let (claimed_a, echoed) = server.await.expect("task").expect("server ok").await;
        assert_eq!(
            claimed_a,
            id_a.public_key_bytes(),
            "B learns A's claimed key"
        );
        assert_eq!(
            echoed, event,
            "post-swap traffic flows over the raw channel"
        );
    }
}
