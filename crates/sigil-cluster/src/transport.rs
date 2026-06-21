//! Transport abstraction (DESIGN §5.3, ADR 3).
//!
//! Roles communicate over a topic-based byte bus. In monolith mode this is the
//! in-process [`InProcTransport`]; in scale-out mode it is Kafka/Redpanda. The
//! Kafka implementation is deferred (needs a broker); [`build_transport`] falls
//! back to in-process with a warning so a node still runs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// A topic-based, at-most-once byte transport.
pub trait Transport: Send + Sync {
    /// Publish `payload` to all current subscribers of `topic`.
    fn publish(&self, topic: &str, payload: &[u8]) -> anyhow::Result<()>;
    /// Subscribe to `topic`, receiving every message published after this call.
    fn subscribe(&self, topic: &str) -> UnboundedReceiver<Vec<u8>>;
    /// Identifier of the concrete transport (`inproc`, `kafka`, ...).
    fn kind(&self) -> &'static str;
}

/// In-process fan-out transport (channels). Cheap and synchronous to publish.
#[derive(Default)]
pub struct InProcTransport {
    subscribers: Mutex<HashMap<String, Vec<UnboundedSender<Vec<u8>>>>>,
}

impl InProcTransport {
    pub fn new() -> Self {
        InProcTransport::default()
    }
}

impl Transport for InProcTransport {
    fn publish(&self, topic: &str, payload: &[u8]) -> anyhow::Result<()> {
        let mut subs = self.subscribers.lock().expect("transport poisoned");
        if let Some(senders) = subs.get_mut(topic) {
            // Drop senders whose receiver has gone away.
            senders.retain(|s| s.send(payload.to_vec()).is_ok());
        }
        Ok(())
    }

    fn subscribe(&self, topic: &str) -> UnboundedReceiver<Vec<u8>> {
        let (tx, rx) = unbounded_channel();
        self.subscribers
            .lock()
            .expect("transport poisoned")
            .entry(topic.to_string())
            .or_default()
            .push(tx);
        rx
    }

    fn kind(&self) -> &'static str {
        "inproc"
    }
}

/// Build a transport from a config `kind`. Unknown/unimplemented kinds fall back
/// to the in-process transport with a warning.
pub fn build_transport(kind: &str) -> anyhow::Result<Arc<dyn Transport>> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "" | "inproc" | "in-proc" | "memory" => Ok(Arc::new(InProcTransport::new())),
        "kafka" | "redpanda" | "nats" => {
            eprintln!(
                "[sigil-cluster] transport '{kind}' is not implemented yet; \
                 using in-process transport (monolith mode)"
            );
            Ok(Arc::new(InProcTransport::new()))
        }
        other => anyhow::bail!("unknown transport kind '{other}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_proc_pub_sub() {
        let t = InProcTransport::new();
        let mut a = t.subscribe("events");
        let mut b = t.subscribe("events");
        t.publish("events", b"hello").unwrap();
        assert_eq!(a.recv().await.unwrap(), b"hello");
        assert_eq!(b.recv().await.unwrap(), b"hello");
        // A different topic gets nothing.
        let mut other = t.subscribe("other");
        t.publish("events", b"world").unwrap();
        assert_eq!(a.recv().await.unwrap(), b"world");
        assert!(other.try_recv().is_err());
    }
}
