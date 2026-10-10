//! An in-process [`Channel`] pair, for tests and the harness: whatever one
//! end sends the other receives, with no sockets involved.

use async_trait::async_trait;
use flow_core::channel::{Channel, ChannelError, ChannelKind, ChannelMessage};
use tokio::sync::mpsc;

pub struct MemoryChannel {
    tx: mpsc::UnboundedSender<ChannelMessage>,
    rx: mpsc::UnboundedReceiver<ChannelMessage>,
}

/// Two connected ends. Dropping one makes the other's `recv` report
/// `ConnectionLost`.
pub fn pair() -> (MemoryChannel, MemoryChannel) {
    let (a_tx, b_rx) = mpsc::unbounded_channel();
    let (b_tx, a_rx) = mpsc::unbounded_channel();
    (
        MemoryChannel { tx: a_tx, rx: a_rx },
        MemoryChannel { tx: b_tx, rx: b_rx },
    )
}

#[async_trait]
impl Channel for MemoryChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Tcp
    }

    async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError> {
        self.tx.send(msg).map_err(|_| ChannelError::ConnectionLost)
    }

    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError> {
        self.rx.recv().await.ok_or(ChannelError::ConnectionLost)
    }

    async fn close(&mut self) -> Result<(), ChannelError> {
        self.rx.close();
        Ok(())
    }
}
