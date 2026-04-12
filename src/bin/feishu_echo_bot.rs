//! Feishu Echo Bot — connects via WebSocket long connection, echoes messages.
//!
//! Feishu WS v2 uses protobuf binary frames (not JSON).
//! Protocol reverse-engineered from lark-oapi Python SDK.
//!
//! Flow:
//! 1. Get tenant_access_token
//! 2. POST /callback/ws/endpoint with PascalCase {AppID, AppSecret} → get WSS URL
//! 3. Connect WSS, receive protobuf Frame messages
//! 4. On DATA frame with type=event → parse JSON payload → echo reply via REST API
//!
//! Protobuf schema (from lark_oapi.ws.pb.pbbp2_pb2):
//!   message Header { string key=1; string value=2; }
//!   message Frame  { uint64 SeqID=1; uint64 LogID=2; int32 service=3;
//!                     int32 method=4; repeated Header headers=5;
//!                     string payload_encoding=6; string payload_type=7;
//!                     bytes payload=8; string LogIDNew=9; }

use clap::Parser;
use prost::Message;
use serde_json::{json, Value};
use std::net::TcpStream;
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::WebSocket;

// ---------------------------------------------------------------------------
// Protobuf definitions (matching lark-oapi pbbp2.proto)
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Message)]
struct PbHeader {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, Message)]
struct PbFrame {
    #[prost(uint64, tag = "1")]
    seq_id: u64,
    #[prost(uint64, tag = "2")]
    log_id: u64,
    #[prost(int32, tag = "3")]
    service: i32,
    #[prost(int32, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<PbHeader>,
    #[prost(string, tag = "6")]
    payload_encoding: String,
    #[prost(string, tag = "7")]
    payload_type: String,
    #[prost(bytes = "vec", tag = "8")]
    payload: Vec<u8>,
    #[prost(string, tag = "9")]
    log_id_new: String,
}

impl PbFrame {
    fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
    }
}

/// method=0 → CONTROL, method=1 → DATA
const METHOD_CONTROL: i32 = 0;
const METHOD_DATA: i32 = 1;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "feishu-echo-bot")]
#[command(about = "Feishu echo bot via WebSocket long connection (protobuf)")]
struct Cli {
    #[arg(long, help = "Feishu app_id")]
    app_id: String,

    #[arg(long, help = "Feishu app_secret")]
    app_secret: String,

    #[arg(
        long,
        default_value = "https://open.feishu.cn",
        help = "Feishu Open API domain"
    )]
    domain: String,
}

// ---------------------------------------------------------------------------
// Feishu Bot
// ---------------------------------------------------------------------------

struct FeishuBot {
    domain: String,
    token: String,
    http: reqwest::blocking::Client,
}

impl FeishuBot {
    fn new(domain: &str) -> Self {
        Self {
            domain: domain.trim_end_matches('/').to_string(),
            token: String::new(),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    fn acquire_token(&mut self, app_id: &str, app_secret: &str) -> Result<(), String> {
        let url = format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            self.domain
        );
        let body = json!({"app_id": app_id, "app_secret": app_secret});
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| format!("token request failed: {e}"))?;
        let data: Value = resp
            .json()
            .map_err(|e| format!("token parse failed: {e}"))?;

        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = data.get("msg").and_then(|v| v.as_str()).unwrap_or("?");
            return Err(format!("code={code}, msg={msg}"));
        }
        self.token = data
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .ok_or("no token")?
            .to_string();
        Ok(())
    }

    fn get_ws_endpoint(&self, app_id: &str, app_secret: &str) -> Result<(String, i32), String> {
        let url = format!("{}/callback/ws/endpoint", self.domain);
        println!("  POST {url}");

        let body = json!({"AppID": app_id, "AppSecret": app_secret});
        let resp = self
            .http
            .post(&url)
            .header("locale", "zh")
            .json(&body)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;

        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        println!("  HTTP {status}");

        let resp_body: Value =
            serde_json::from_str(&text).map_err(|e| format!("parse failed: {e}"))?;

        let code = resp_body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = resp_body.get("msg").and_then(|v| v.as_str()).unwrap_or("?");
            return Err(format!("code={code}, msg=\"{msg}\""));
        }

        let data = resp_body.get("data").ok_or("no data")?;
        let ws_url = data
            .get("URL")
            .or_else(|| data.get("url"))
            .and_then(|v| v.as_str())
            .ok_or("no URL in data")?;

        let service_id = ws_url
            .split("service_id=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        if let Some(config) = data.get("ClientConfig") {
            println!("  ClientConfig: {config}");
        }
        println!("  service_id: {service_id}");

        Ok((ws_url.to_string(), service_id))
    }

    fn reply_text(&self, message_id: &str, text: &str) -> Result<(), String> {
        let url = format!(
            "{}/open-apis/im/v1/messages/{}/reply",
            self.domain, message_id
        );
        let body = json!({
            "content": json!({"text": text}).to_string(),
            "msg_type": "text",
        });
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .map_err(|e| format!("reply failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;

        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = data.get("msg").and_then(|v| v.as_str()).unwrap_or("?");
            return Err(format!("code={code}, msg={msg}"));
        }
        Ok(())
    }

    fn send_to_chat(&self, chat_id: &str, text: &str) -> Result<(), String> {
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
            self.domain
        );
        let body = json!({
            "receive_id": chat_id,
            "content": json!({"text": text}).to_string(),
            "msg_type": "text",
        });
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .map_err(|e| format!("send failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;

        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = data.get("msg").and_then(|v| v.as_str()).unwrap_or("?");
            return Err(format!("code={code}, msg={msg}"));
        }
        Ok(())
    }

    fn handle_frame(&self, frame: &PbFrame, ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
        let msg_type = frame.header("type").unwrap_or("");

        match frame.method {
            METHOD_CONTROL => match msg_type {
                "pong" => println!("[PONG]"),
                "ping" => println!("[SERVER-PING]"),
                other => println!("[CONTROL] type={other}"),
            },
            METHOD_DATA => {
                println!("[DATA] type={msg_type} payload_len={}", frame.payload.len());
                if msg_type == "event" {
                    self.handle_event(frame, ws);
                }
            }
            other => {
                println!("[FRAME] method={other} type={msg_type}");
            }
        }
    }

    fn handle_event(&self, frame: &PbFrame, ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
        let payload_str = String::from_utf8_lossy(&frame.payload);
        let payload: Value = match serde_json::from_str(&payload_str) {
            Ok(v) => v,
            Err(e) => {
                println!("[WARN] event payload not JSON: {e}");
                println!("  raw: {payload_str}");
                return;
            }
        };

        let event_type = payload
            .pointer("/header/event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        println!("[EVENT] type={event_type}");
        println!(
            "  payload: {}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );

        // Send ACK (protobuf frame with payload = {"code": 200})
        let mut resp_frame = frame.clone();
        resp_frame.payload = json!({"code": 200}).to_string().into_bytes();
        let mut buf = Vec::new();
        if resp_frame.encode(&mut buf).is_ok() {
            let _ = ws.send(tungstenite::Message::Binary(buf.into()));
            println!("[ACK] sent");
        }

        if event_type.contains("im.message.receive") {
            self.handle_message_event(&payload);
        }
    }

    fn handle_message_event(&self, payload: &Value) {
        let event = payload.get("event").unwrap_or(payload);
        let message = match event.get("message") {
            Some(m) => m,
            None => {
                println!("[WARN] no 'message' in event");
                return;
            }
        };

        let message_id = message
            .get("message_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let chat_id = message
            .get("chat_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let msg_type = message
            .get("message_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let content_str = message
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("{}");
        let content: Value = serde_json::from_str(content_str).unwrap_or_default();
        let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");

        let clean_text = text
            .split_whitespace()
            .filter(|w| !w.starts_with("@_"))
            .collect::<Vec<_>>()
            .join(" ");
        let clean_text = clean_text.trim();

        println!("[MSG] id={message_id} chat={chat_id} type={msg_type} text=\"{clean_text}\"");

        if clean_text.is_empty() {
            println!("[SKIP] empty text");
            return;
        }

        println!("[REPLY] \"{clean_text}\" → message_id={message_id}");

        if !message_id.is_empty() {
            match self.reply_text(message_id, clean_text) {
                Ok(()) => {
                    println!("[REPLY] OK (reply API)");
                    return;
                }
                Err(e) => println!("[REPLY] reply API failed: {e}, trying send_to_chat..."),
            }
        }

        if !chat_id.is_empty() {
            match self.send_to_chat(chat_id, clean_text) {
                Ok(()) => println!("[REPLY] OK (send-to-chat)"),
                Err(e) => println!("[REPLY] send-to-chat failed: {e}"),
            }
        } else {
            println!("[REPLY] FAILED — no message_id or chat_id");
        }
    }
}

// ---------------------------------------------------------------------------
// Ping frame builder
// ---------------------------------------------------------------------------

fn build_ping_frame(service_id: i32) -> Vec<u8> {
    let frame = PbFrame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: METHOD_CONTROL,
        headers: vec![PbHeader {
            key: "type".to_string(),
            value: "ping".to_string(),
        }],
        payload_encoding: String::new(),
        payload_type: String::new(),
        payload: Vec::new(),
        log_id_new: String::new(),
    };
    let mut buf = Vec::new();
    frame.encode(&mut buf).unwrap();
    buf
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    println!("=== Feishu Echo Bot (protobuf) ===");
    println!("Domain : {}", cli.domain);
    println!("App ID : {}", cli.app_id);
    println!();

    let mut bot = FeishuBot::new(&cli.domain);

    println!("[1/3] Getting tenant_access_token ...");
    bot.acquire_token(&cli.app_id, &cli.app_secret)
        .unwrap_or_else(|e| {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        });
    println!("  OK — token length={}", bot.token.len());
    println!();

    println!("[2/3] Getting WebSocket endpoint ...");
    let (ws_url, service_id) = bot
        .get_ws_endpoint(&cli.app_id, &cli.app_secret)
        .unwrap_or_else(|e| {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        });
    println!("  OK — URL obtained");
    println!();

    println!("[3/3] Connecting to Feishu WebSocket ...");
    let (mut ws, resp) = tungstenite::connect(&ws_url).unwrap_or_else(|e| {
        eprintln!("FAIL: WebSocket connect: {e}");
        std::process::exit(1);
    });
    println!("  OK — connected (HTTP {})", resp.status());
    println!();
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║  Bot is running! Send a message in Feishu to test.  ║");
    println!("║  Press Ctrl+C to stop.                              ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    // Send initial ping
    let ping = build_ping_frame(service_id);
    ws.send(tungstenite::Message::Binary(ping.into())).ok();
    println!("[PING] initial ping sent");

    let mut msg_count = 0u64;

    loop {
        match ws.read() {
            Ok(tungstenite::Message::Binary(data)) => {
                match PbFrame::decode(data.as_ref()) {
                    Ok(frame) => bot.handle_frame(&frame, &mut ws),
                    Err(e) => println!("[WARN] protobuf decode failed: {e} ({} bytes)", data.len()),
                }
                println!("---");

                msg_count += 1;
                if msg_count % 10 == 0 {
                    let ping = build_ping_frame(service_id);
                    ws.send(tungstenite::Message::Binary(ping.into())).ok();
                    println!("[PING] keepalive");
                }
            }
            Ok(tungstenite::Message::Text(text)) => {
                println!("[TEXT] {text}");
                println!("---");
            }
            Ok(tungstenite::Message::Ping(data)) => {
                ws.send(tungstenite::Message::Pong(data)).ok();
            }
            Ok(tungstenite::Message::Close(frame)) => {
                println!("[CLOSE] {:?}", frame);
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("[ERROR] WebSocket: {e}");
                break;
            }
        }
    }

    println!("Bot stopped.");
}
