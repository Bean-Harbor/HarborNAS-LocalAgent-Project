//! Channel abstraction: how messages enter and leave the assistant.
//!
//! Channels convert raw user input into a normalized `InboundMessage` and
//! format results into `OutboundMessage`. The runtime drives channels through
//! `recv() → process → send()`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: String,
    pub sender: String,
    pub text: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: String,
    pub text: String,
    #[serde(default)]
    pub metadata: Value,
}

// ---------------------------------------------------------------------------
// Channel trait
// ---------------------------------------------------------------------------

pub trait Channel: Send + Sync {
    fn name(&self) -> &str;

    /// Receive one inbound message (blocking).
    /// Returns None when channel is closed.
    fn recv(&self) -> Option<InboundMessage>;

    /// Send a response.
    fn send(&self, msg: OutboundMessage);
}

// ---------------------------------------------------------------------------
// CLI channel: reads from stdin, writes to stdout
// ---------------------------------------------------------------------------

pub struct CliChannel {
    sender_name: String,
}

impl CliChannel {
    pub fn new(sender: &str) -> Self {
        Self {
            sender_name: sender.to_string(),
        }
    }
}

impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    fn recv(&self) -> Option<InboundMessage> {
        let mut buf = String::new();
        match std::io::stdin().read_line(&mut buf) {
            Ok(0) => None, // EOF
            Ok(_) => {
                let text = buf.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                Some(InboundMessage {
                    channel: "cli".into(),
                    sender: self.sender_name.clone(),
                    text,
                    metadata: Value::Null,
                })
            }
            Err(_) => None,
        }
    }

    fn send(&self, msg: OutboundMessage) {
        println!("{}", msg.text);
    }
}

// ---------------------------------------------------------------------------
// HarborBeacon stub — will connect via MCP/webhook in production
// ---------------------------------------------------------------------------

pub struct HarborBeaconChannel {
    /// In production: webhook URL or MCP socket.
    /// For now: buffer for testing.
    inbound_buf: std::sync::Mutex<Vec<InboundMessage>>,
    outbound_buf: std::sync::Mutex<Vec<OutboundMessage>>,
}

impl HarborBeaconChannel {
    pub fn new() -> Self {
        Self {
            inbound_buf: std::sync::Mutex::new(Vec::new()),
            outbound_buf: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Inject a message (used in tests and by the webhook handler).
    pub fn inject(&self, msg: InboundMessage) {
        self.inbound_buf.lock().unwrap().push(msg);
    }

    /// Drain sent responses (used in tests).
    pub fn drain_outbound(&self) -> Vec<OutboundMessage> {
        let mut buf = self.outbound_buf.lock().unwrap();
        buf.drain(..).collect()
    }
}

impl Channel for HarborBeaconChannel {
    fn name(&self) -> &str {
        "harbor_beacon"
    }

    fn recv(&self) -> Option<InboundMessage> {
        let mut buf = self.inbound_buf.lock().unwrap();
        if buf.is_empty() {
            None
        } else {
            Some(buf.remove(0))
        }
    }

    fn send(&self, msg: OutboundMessage) {
        self.outbound_buf.lock().unwrap().push(msg);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn beacon_inject_recv_send() {
        let ch = HarborBeaconChannel::new();
        ch.inject(InboundMessage {
            channel: "harbor_beacon".into(),
            sender: "user1".into(),
            text: "hello".into(),
            metadata: Value::Null,
        });

        let msg = ch.recv().unwrap();
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.sender, "user1");

        ch.send(OutboundMessage {
            channel: "harbor_beacon".into(),
            text: "hi back".into(),
            metadata: Value::Null,
        });

        let out = ch.drain_outbound();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hi back");
    }

    #[test]
    fn beacon_empty_recv_returns_none() {
        let ch = HarborBeaconChannel::new();
        assert!(ch.recv().is_none());
    }

    #[test]
    fn inbound_message_serialization() {
        let msg = InboundMessage {
            channel: "cli".into(),
            sender: "admin".into(),
            text: "list services".into(),
            metadata: serde_json::json!({"source": "terminal"}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"text\":\"list services\""));
        let parsed: InboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sender, "admin");
    }

    #[test]
    fn outbound_message_default_metadata() {
        let json_str = r#"{"channel":"cli","text":"ok"}"#;
        let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
        assert_eq!(msg.text, "ok");
        assert_eq!(msg.metadata, Value::Null);
    }
}
