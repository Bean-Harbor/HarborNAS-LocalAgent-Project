//! Feishu HarborOS Command Bot — receives commands from Feishu, executes on HarborOS.
//!
//! Commands:
//!   关闭ssh / 停止ssh  → service.control STOP ssh  → reply "已关闭"
//!   开启ssh / 启动ssh  → service.control START ssh  → reply "已开启"
//!   重启ssh            → service.control RESTART ssh → reply "已重启"
//!   查询ssh / ssh状态  → service.query ssh          → reply status
//!   分析摄像头 / 分析客厅 → vision.analyze_camera → reply summary
//!   Other text           → echo back (fallback)
//!
//! HarborOS interaction: WebSocket at ws://<host>/websocket
//! Feishu interaction: WS v2 protobuf long connection

use base64::Engine as _;
use clap::Parser;
use harbornas_local_agent::runtime::feishu_session::{
    FeishuConversationState, FeishuConversationStore, PendingFeishuAdd, PendingFeishuCandidate,
};
use harbornas_local_agent::runtime::hub::{
    looks_like_auth_error, CameraConnectRequest, CameraHubService, HubScanResultItem,
};
use harbornas_local_agent::runtime::registry::DeviceRegistryStore;
use prost::Message;
use reqwest::blocking::multipart::{Form, Part};
use serde_json::{json, Value};
use std::fs;
use std::net::{Ipv4Addr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::WebSocket;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Protobuf (Feishu WS v2)
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

const METHOD_CONTROL: i32 = 0;
const METHOD_DATA: i32 = 1;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "feishu-harbor-bot")]
#[command(about = "Feishu → HarborOS command bot")]
struct Cli {
    #[arg(long, help = "Feishu app_id")]
    app_id: Option<String>,
    #[arg(long, help = "Feishu app_secret")]
    app_secret: Option<String>,
    #[arg(long, default_value = "https://open.feishu.cn")]
    domain: String,

    #[arg(long, default_value = "192.168.3.172", help = "HarborOS host")]
    harbor_host: String,
    #[arg(long, default_value = "harboros_admin", help = "HarborOS user")]
    harbor_user: String,
    #[arg(long, default_value = "123456", help = "HarborOS password")]
    harbor_password: String,
}

// ---------------------------------------------------------------------------
// HarborOS WebSocket Client
// ---------------------------------------------------------------------------

struct HarborOsClient {
    host: String,
    user: String,
    password: String,
}

impl HarborOsClient {
    fn new(host: &str, user: &str, password: &str) -> Self {
        Self {
            host: host.to_string(),
            user: user.to_string(),
            password: password.to_string(),
        }
    }

    /// Connect, authenticate, execute one service operation, disconnect.
    fn service_control(&self, action: &str, service_name: &str) -> Result<Value, String> {
        let url = format!("ws://{}/websocket", self.host);
        let (mut ws, _) = tungstenite::connect(&url)
            .map_err(|e| format!("WS connect to HarborOS failed: {e}"))?;

        // Handshake
        let connect_msg = json!({"msg": "connect", "version": "1", "support": ["1"]});
        ws.send(tungstenite::Message::Text(connect_msg.to_string().into()))
            .map_err(|e| format!("handshake send failed: {e}"))?;
        let handshake_resp = self.read_json(&mut ws)?;
        if handshake_resp.get("msg").and_then(|v| v.as_str()) != Some("connected") {
            return Err(format!("handshake failed: {handshake_resp}"));
        }

        // Auth
        let auth_msg = json!({
            "id": 1, "msg": "method", "method": "auth.login",
            "params": [self.user, self.password]
        });
        ws.send(tungstenite::Message::Text(auth_msg.to_string().into()))
            .map_err(|e| format!("auth send failed: {e}"))?;
        let auth_resp = self.read_json(&mut ws)?;
        let auth_ok = auth_resp
            .get("result")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !auth_ok {
            return Err(format!("auth failed: {auth_resp}"));
        }

        // Service control
        let ctrl_msg = json!({
            "id": 2, "msg": "method", "method": "service.control",
            "params": [action, service_name, {}]
        });
        ws.send(tungstenite::Message::Text(ctrl_msg.to_string().into()))
            .map_err(|e| format!("control send failed: {e}"))?;
        let result = self.read_json(&mut ws)?;

        // Close cleanly
        let _ = ws.close(None);
        Ok(result)
    }

    fn service_query(&self, service_name: &str) -> Result<Value, String> {
        let url = format!("ws://{}/websocket", self.host);
        let (mut ws, _) = tungstenite::connect(&url)
            .map_err(|e| format!("WS connect to HarborOS failed: {e}"))?;

        // Handshake
        let connect_msg = json!({"msg": "connect", "version": "1", "support": ["1"]});
        ws.send(tungstenite::Message::Text(connect_msg.to_string().into()))
            .map_err(|e| format!("handshake send failed: {e}"))?;
        let handshake_resp = self.read_json(&mut ws)?;
        if handshake_resp.get("msg").and_then(|v| v.as_str()) != Some("connected") {
            return Err(format!("handshake failed: {handshake_resp}"));
        }

        // Auth
        let auth_msg = json!({
            "id": 1, "msg": "method", "method": "auth.login",
            "params": [self.user, self.password]
        });
        ws.send(tungstenite::Message::Text(auth_msg.to_string().into()))
            .map_err(|e| format!("auth send failed: {e}"))?;
        let auth_resp = self.read_json(&mut ws)?;
        let auth_ok = auth_resp
            .get("result")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !auth_ok {
            return Err(format!("auth failed: {auth_resp}"));
        }

        // Service query
        let query_msg = json!({
            "id": 2, "msg": "method", "method": "service.query",
            "params": [
                [["service", "=", service_name]],
                {"select": ["service", "state", "enable"]}
            ]
        });
        ws.send(tungstenite::Message::Text(query_msg.to_string().into()))
            .map_err(|e| format!("query send failed: {e}"))?;
        let result = self.read_json(&mut ws)?;

        let _ = ws.close(None);
        Ok(result)
    }

    fn read_json(&self, ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<Value, String> {
        loop {
            match ws.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    return serde_json::from_str(&text)
                        .map_err(|e| format!("JSON parse failed: {e}"));
                }
                Ok(tungstenite::Message::Ping(data)) => {
                    let _ = ws.send(tungstenite::Message::Pong(data));
                }
                Ok(_) => continue,
                Err(e) => return Err(format!("WS read failed: {e}")),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Feishu Bot
// ---------------------------------------------------------------------------

struct FeishuBot {
    domain: String,
    token: String,
    http: reqwest::blocking::Client,
    harbor: HarborOsClient,
    workspace_root: PathBuf,
}

struct CameraAnalysisReply {
    text: String,
    image_path: Option<String>,
}

struct CardReply {
    text: String,
    card: Value,
}

enum BotReply {
    Text(String),
    CameraAnalysis(CameraAnalysisReply),
    Card(CardReply),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CameraIntent {
    Snapshot,
    Analyze,
}

#[derive(Debug, Clone)]
struct FeishuSenderIdentity {
    open_id: String,
    user_id: Option<String>,
    union_id: Option<String>,
    display_name: String,
    chat_id: Option<String>,
}

#[derive(Debug, Clone)]
enum CandidateCommand {
    Connect(usize),
    Ignore(usize),
}

impl FeishuBot {
    fn new(domain: &str, harbor: HarborOsClient) -> Self {
        Self {
            domain: domain.trim_end_matches('/').to_string(),
            token: String::new(),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("failed to build HTTP client"),
            harbor,
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    fn admin_store(&self) -> harbornas_local_agent::runtime::admin_console::AdminConsoleStore {
        harbornas_local_agent::runtime::admin_console::AdminConsoleStore::new(
            self.workspace_root.join(".harbornas/admin-console.json"),
            DeviceRegistryStore::new(self.workspace_root.join(".harbornas/device-registry.json")),
        )
    }

    fn hub_service(&self) -> CameraHubService {
        CameraHubService::new(self.admin_store())
    }

    fn conversation_store(&self) -> FeishuConversationStore {
        FeishuConversationStore::new(
            self.workspace_root
                .join(".harbornas/feishu-conversations.json"),
        )
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
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "code={code}, msg={}",
                data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
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
        let body = json!({"AppID": app_id, "AppSecret": app_secret});
        let resp = self
            .http
            .post(&url)
            .header("locale", "zh")
            .json(&body)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
        let text = resp.text().unwrap_or_default();
        let resp_body: Value =
            serde_json::from_str(&text).map_err(|e| format!("parse failed: {e}"))?;
        let code = resp_body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "code={code}, msg=\"{}\"",
                resp_body.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
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
            return Err(format!(
                "code={code}, msg={}",
                data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
        }
        Ok(())
    }

    fn reply_card(&self, message_id: &str, card: &Value) -> Result<(), String> {
        let url = format!(
            "{}/open-apis/im/v1/messages/{}/reply",
            self.domain, message_id
        );
        let body = json!({
            "content": serde_json::to_string(card)
                .map_err(|e| format!("card serialize failed: {e}"))?,
            "msg_type": "interactive",
            "uuid": Uuid::new_v4().to_string(),
        });
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .map_err(|e| format!("card reply failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "code={code}, msg={}",
                data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
        }
        Ok(())
    }

    fn reply_image(&self, message_id: &str, image_key: &str) -> Result<(), String> {
        let url = format!(
            "{}/open-apis/im/v1/messages/{}/reply",
            self.domain, message_id
        );
        let body = json!({
            "content": json!({"image_key": image_key}).to_string(),
            "msg_type": "image",
        });
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .map_err(|e| format!("image reply failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "code={code}, msg={}",
                data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
        }
        Ok(())
    }

    fn upload_message_image(&self, image_path: &str) -> Result<String, String> {
        let url = format!("{}/open-apis/im/v1/images", self.domain);
        let bytes = fs::read(image_path)
            .map_err(|e| format!("failed to read image {}: {e}", image_path))?;
        let filename = PathBuf::from(image_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("camera.jpg")
            .to_string();

        let part = Part::bytes(bytes)
            .file_name(filename)
            .mime_str("image/jpeg")
            .map_err(|e| format!("failed to build image upload part: {e}"))?;
        let form = Form::new()
            .text("image_type", "message")
            .part("image", part);

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .multipart(form)
            .send()
            .map_err(|e| format!("image upload failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "code={code}, msg={}",
                data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
        }

        data.pointer("/data/image_key")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
            .ok_or_else(|| "image upload response missing image_key".to_string())
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
            return Err(format!(
                "code={code}, msg={}",
                data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
        }
        Ok(())
    }

    fn send_card_to_chat(&self, chat_id: &str, card: &Value) -> Result<(), String> {
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
            self.domain
        );
        let body = json!({
            "receive_id": chat_id,
            "content": serde_json::to_string(card)
                .map_err(|e| format!("card serialize failed: {e}"))?,
            "msg_type": "interactive",
            "uuid": Uuid::new_v4().to_string(),
        });
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .map_err(|e| format!("send card failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "code={code}, msg={}",
                data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")
            ));
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
            other => println!("[FRAME] method={other} type={msg_type}"),
        }
    }

    fn handle_event(&self, frame: &PbFrame, ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
        let payload_str = String::from_utf8_lossy(&frame.payload);
        let payload: Value = match serde_json::from_str(&payload_str) {
            Ok(v) => v,
            Err(e) => {
                println!("[WARN] payload not JSON: {e}");
                return;
            }
        };

        let event_type = payload
            .pointer("/header/event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        println!("[EVENT] {event_type}");

        // ACK
        let mut ack = frame.clone();
        ack.payload = json!({"code": 200}).to_string().into_bytes();
        let mut buf = Vec::new();
        if ack.encode(&mut buf).is_ok() {
            let _ = ws.send(tungstenite::Message::Binary(buf.into()));
            println!("[ACK]");
        }

        if event_type.contains("im.message.receive") {
            self.handle_message_event(&payload);
        }
    }

    fn handle_message_event(&self, payload: &Value) {
        let event = payload.get("event").unwrap_or(payload);
        let sender = self.extract_sender_identity(event);
        let message = match event.get("message") {
            Some(m) => m,
            None => {
                println!("[WARN] no message field");
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
        let content_str = message
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("{}");
        let content: Value = serde_json::from_str(content_str).unwrap_or_default();
        let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");

        // Strip @mentions
        let clean = text
            .split_whitespace()
            .filter(|w| !w.starts_with("@_"))
            .collect::<Vec<_>>()
            .join(" ");
        let clean = clean.trim();

        println!("[MSG] \"{clean}\" (msg_id={message_id})");

        if clean.is_empty() {
            return;
        }

        let reply = if is_binding_command(clean) {
            self.cmd_bind_feishu_user(clean, &sender)
        } else {
            self.dispatch_command(clean, &sender)
        };

        self.deliver_reply(message_id, chat_id, reply);
    }

    fn deliver_reply(&self, message_id: &str, chat_id: &str, reply: BotReply) {
        match reply {
            BotReply::Text(text) => {
                println!("[REPLY] \"{text}\"");
                if !message_id.is_empty() {
                    match self.reply_text(message_id, &text) {
                        Ok(()) => {
                            println!("[REPLY] OK");
                            return;
                        }
                        Err(e) => println!("[REPLY] reply API failed: {e}"),
                    }
                }
                if !chat_id.is_empty() {
                    match self.send_to_chat(chat_id, &text) {
                        Ok(()) => println!("[REPLY] OK (send-to-chat)"),
                        Err(e) => println!("[REPLY] send-to-chat failed: {e}"),
                    }
                }
            }
            BotReply::CameraAnalysis(result) => {
                println!("[REPLY] \"{}\"", result.text);
                if !message_id.is_empty() {
                    if let Some(image_path) = result.image_path.as_deref() {
                        match self.upload_message_image(image_path) {
                            Ok(image_key) => match self.reply_image(message_id, &image_key) {
                                Ok(()) => println!("[REPLY] image OK"),
                                Err(e) => println!("[REPLY] image reply failed: {e}"),
                            },
                            Err(e) => println!("[REPLY] image upload failed: {e}"),
                        }
                    }

                    match self.reply_text(message_id, &result.text) {
                        Ok(()) => {
                            println!("[REPLY] text OK");
                            return;
                        }
                        Err(e) => println!("[REPLY] reply API failed: {e}"),
                    }
                }
                if !chat_id.is_empty() {
                    match self.send_to_chat(chat_id, &result.text) {
                        Ok(()) => println!("[REPLY] OK (send-to-chat)"),
                        Err(e) => println!("[REPLY] send-to-chat failed: {e}"),
                    }
                }
            }
            BotReply::Card(result) => {
                println!("[REPLY] card -> {}", result.text);
                if !message_id.is_empty() {
                    match self.reply_card(message_id, &result.card) {
                        Ok(()) => {
                            println!("[REPLY] card OK");
                            return;
                        }
                        Err(e) => println!("[REPLY] card reply failed: {e}"),
                    }

                    match self.reply_text(message_id, &result.text) {
                        Ok(()) => {
                            println!("[REPLY] text fallback OK");
                            return;
                        }
                        Err(e) => println!("[REPLY] text fallback failed: {e}"),
                    }
                }

                if !chat_id.is_empty() {
                    match self.send_card_to_chat(chat_id, &result.card) {
                        Ok(()) => {
                            println!("[REPLY] card OK (send-to-chat)");
                            return;
                        }
                        Err(e) => println!("[REPLY] send card failed: {e}"),
                    }

                    match self.send_to_chat(chat_id, &result.text) {
                        Ok(()) => println!("[REPLY] text fallback OK (send-to-chat)"),
                        Err(e) => println!("[REPLY] text fallback failed (send-to-chat): {e}"),
                    }
                }
            }
        }
    }

    fn extract_sender_identity(&self, event: &Value) -> FeishuSenderIdentity {
        let open_id = event
            .pointer("/sender/sender_id/open_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let user_id = event
            .pointer("/sender/sender_id/user_id")
            .and_then(|v| v.as_str())
            .map(|value| value.to_string());
        let union_id = event
            .pointer("/sender/sender_id/union_id")
            .and_then(|v| v.as_str())
            .map(|value| value.to_string());
        let chat_id = event
            .pointer("/message/chat_id")
            .and_then(|v| v.as_str())
            .map(|value| value.to_string());
        let display_name = if !open_id.is_empty() {
            let suffix: String = open_id
                .chars()
                .rev()
                .take(6)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            format!("飞书用户 {suffix}")
        } else {
            "飞书用户".to_string()
        };

        FeishuSenderIdentity {
            open_id,
            user_id,
            union_id,
            display_name,
            chat_id,
        }
    }

    fn dispatch_command(&self, text: &str, sender: &FeishuSenderIdentity) -> BotReply {
        let t = normalize_command_text(text);

        if let Some(reply) = self.try_continue_pending_camera_add(text, sender) {
            return BotReply::Text(reply);
        }

        if let Some(reply) = self.try_handle_candidate_command(text, sender) {
            return BotReply::Text(reply);
        }

        // SSH commands
        if t.contains("关闭ssh") || t.contains("停止ssh") || t.contains("stopssh") {
            return BotReply::Text(self.cmd_service_control("STOP", "ssh", "已关闭"));
        }
        if t.contains("开启ssh") || t.contains("启动ssh") || t.contains("startssh") {
            return BotReply::Text(self.cmd_service_control("START", "ssh", "已开启"));
        }
        if t.contains("重启ssh") || t.contains("restartssh") {
            return BotReply::Text(self.cmd_service_control("RESTART", "ssh", "已重启"));
        }
        if t.contains("查询ssh") || t.contains("ssh状态") || t.contains("sshstatus") {
            return BotReply::Text(self.cmd_service_query("ssh"));
        }

        if t.contains("菜单")
            || t.contains("帮助")
            || t.contains("开始")
            || t.contains("能力")
            || t.contains("能做什么")
        {
            return self.cmd_show_capabilities();
        }

        if is_scan_camera_command(&t) {
            return BotReply::Text(self.cmd_scan_cameras(sender));
        }

        if is_manual_add_camera_command(&t) {
            return BotReply::Text(self.cmd_manual_add_camera(text, sender));
        }

        if let Some(intent) = classify_camera_intent(&t) {
            return match intent {
                CameraIntent::Snapshot => self.cmd_snapshot_camera(text),
                CameraIntent::Analyze => self.cmd_analyze_camera(text),
            };
        }

        // Fallback: echo
        BotReply::Text(text.to_string())
    }

    fn cmd_bind_feishu_user(&self, text: &str, sender: &FeishuSenderIdentity) -> BotReply {
        let binding_code = match extract_binding_code(text) {
            Some(code) => code,
            None => return BotReply::Text(
                "绑定格式不对。请发送“绑定 5B86-A98F”或把完整的 hub://bind/feishu/... 链接发给我。"
                    .to_string(),
            ),
        };

        if sender.open_id.trim().is_empty() {
            return BotReply::Text(
                "当前消息里没有带上飞书用户身份，暂时无法完成绑定。".to_string(),
            );
        }

        let store = self.admin_store();

        let user = harbornas_local_agent::runtime::admin_console::FeishuUserBinding {
            open_id: sender.open_id.clone(),
            user_id: sender.user_id.clone(),
            union_id: sender.union_id.clone(),
            display_name: sender.display_name.clone(),
            chat_id: sender.chat_id.clone(),
        };

        match store.bind_feishu_user(&binding_code, user) {
            Ok(_) => match self.build_binding_success_card(&sender.display_name) {
                Ok(card) => BotReply::Card(card),
                Err(error) => BotReply::Text(format!(
                    "绑定成功，当前账号已连接 HarborNAS Agent Hub，但欢迎卡片生成失败: {error}"
                )),
            },
            Err(error) => BotReply::Text(format!("绑定失败: {error}")),
        }
    }

    fn cmd_show_capabilities(&self) -> BotReply {
        match self.build_capability_card() {
            Ok(reply) => BotReply::Card(reply),
            Err(e) => BotReply::Text(format!("无法生成欢迎卡片: {e}")),
        }
    }

    fn cmd_snapshot_camera(&self, text: &str) -> BotReply {
        let device_id = match self.resolve_camera_device_id(text) {
            Ok(device_id) => device_id,
            Err(e) => return BotReply::Text(format!("摄像头抓拍失败: {e}")),
        };

        println!("[SNAPSHOT] capture camera {device_id} ...");
        match self.run_local_camera_snapshot(&device_id) {
            Ok(reply) => BotReply::CameraAnalysis(reply),
            Err(e) => BotReply::Text(format!("摄像头抓拍失败: {e}")),
        }
    }

    fn cmd_service_control(&self, action: &str, service: &str, success_msg: &str) -> String {
        println!("[HARBOR] service.control {action} {service} ...");
        match self.harbor.service_control(action, service) {
            Ok(resp) => {
                println!("[HARBOR] response: {resp}");
                let has_error = resp.get("error").is_some();
                if has_error {
                    let reason = resp
                        .get("error")
                        .and_then(|e| e.get("reason"))
                        .and_then(|v| v.as_str())
                        .or_else(|| resp.get("error").and_then(|v| v.as_str()))
                        .unwrap_or("unknown error");
                    format!("操作失败: {reason}")
                } else {
                    success_msg.to_string()
                }
            }
            Err(e) => {
                println!("[HARBOR] ERROR: {e}");
                format!("操作失败: {e}")
            }
        }
    }

    fn cmd_service_query(&self, service: &str) -> String {
        println!("[HARBOR] service.query {service} ...");
        match self.harbor.service_query(service) {
            Ok(resp) => {
                println!("[HARBOR] response: {resp}");
                if let Some(result) = resp.get("result") {
                    if let Some(arr) = result.as_array() {
                        if let Some(svc) = arr.first() {
                            let state = svc
                                .get("state")
                                .and_then(|v| v.as_str())
                                .unwrap_or("UNKNOWN");
                            let enable =
                                svc.get("enable").and_then(|v| v.as_bool()).unwrap_or(false);
                            return format!(
                                "SSH 状态: {state}, 自启: {}",
                                if enable { "是" } else { "否" }
                            );
                        }
                    }
                }
                format!("SSH 状态: 未知 (raw: {resp})")
            }
            Err(e) => format!("查询失败: {e}"),
        }
    }

    fn cmd_analyze_camera(&self, text: &str) -> BotReply {
        let device_id = match self.resolve_camera_device_id(text) {
            Ok(device_id) => device_id,
            Err(e) => return BotReply::Text(format!("摄像头分析失败: {e}")),
        };

        println!("[VISION] analyze camera {device_id} ...");
        match self.run_local_camera_analysis(&device_id) {
            Ok(reply) => BotReply::CameraAnalysis(reply),
            Err(e) => BotReply::Text(format!("摄像头分析失败: {e}")),
        }
    }

    fn cmd_scan_cameras(&self, sender: &FeishuSenderIdentity) -> String {
        match self.scan_cameras_with_defaults(sender) {
            Ok(summary) => summary,
            Err(error) => format!("扫描摄像头失败: {error}"),
        }
    }

    fn cmd_manual_add_camera(&self, text: &str, sender: &FeishuSenderIdentity) -> String {
        match self.manual_add_camera_with_defaults(text, None) {
            Ok(summary) => summary,
            Err(error) => self.handle_manual_add_failure(text, sender, &error),
        }
    }

    fn resolve_camera_device_id(&self, text: &str) -> Result<String, String> {
        let devices = self.load_registered_cameras()?;

        if devices.is_empty() {
            return Err("当前没有已注册的摄像头".to_string());
        }

        let normalized = normalize_command_text(text);
        for device in &devices {
            let device_id = device.device_id.as_str();
            let name = device.name.as_str();
            let room = device.room.as_deref().unwrap_or_default();

            if !name.is_empty() && normalized.contains(&name.replace(' ', "").to_lowercase()) {
                return Ok(device_id.to_string());
            }
            if !room.is_empty() && normalized.contains(&room.replace(' ', "").to_lowercase()) {
                return Ok(device_id.to_string());
            }
            for alias in room_aliases(name, room) {
                if normalized.contains(alias) {
                    return Ok(device_id.to_string());
                }
            }
        }

        devices
            .first()
            .map(|device| device.device_id.clone())
            .ok_or_else(|| "未找到可分析的摄像头设备 ID".to_string())
    }

    fn run_local_camera_snapshot(&self, device_id: &str) -> Result<CameraAnalysisReply, String> {
        let devices = self.load_registered_cameras()?;
        let device = devices
            .into_iter()
            .find(|device| device.device_id == device_id)
            .ok_or_else(|| format!("未找到摄像头设备 {device_id}"))?;

        let request = harbornas_local_agent::runtime::media::SnapshotCaptureRequest::new(
            device.device_id.clone(),
            device.primary_stream.url.clone(),
            harbornas_local_agent::runtime::media::SnapshotFormat::Jpeg,
            harbornas_local_agent::connectors::storage::StorageTarget::LocalDisk,
        );
        let adapter = harbornas_local_agent::adapters::rtsp::CommandRtspAdapter::default();
        let result = harbornas_local_agent::adapters::rtsp::RtspProbeAdapter::capture_snapshot(
            &adapter, &request,
        )?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result.bytes_base64.as_bytes())
            .map_err(|e| format!("抓拍结果无法解码: {e}"))?;

        let output_dir = self.workspace_root.join(".harbornas/tmp/feishu-snapshots");
        fs::create_dir_all(&output_dir)
            .map_err(|e| format!("无法创建抓拍目录 {}: {e}", output_dir.display()))?;
        let output_path = output_dir.join(format!("{}-latest.jpg", device.device_id));
        fs::write(&output_path, bytes)
            .map_err(|e| format!("无法写入抓拍图片 {}: {e}", output_path.display()))?;

        let room = device.room.clone().unwrap_or_else(|| device.name.clone());
        Ok(CameraAnalysisReply {
            text: format!("{} 当前画面如下\n已为你抓拍 1 张图片。", room),
            image_path: Some(output_path.to_string_lossy().to_string()),
        })
    }

    fn run_local_camera_analysis(&self, device_id: &str) -> Result<CameraAnalysisReply, String> {
        let plan_path = self
            .workspace_root
            .join(".harbornas/tmp/feishu-vision-plan.json");
        if let Some(parent) = plan_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建分析计划目录 {}: {e}", parent.display()))?;
        }

        let plan = json!({
            "task_id": "feishu-camera-analysis",
            "goal": "analyze camera frame",
            "steps": [
                {
                    "domain": "vision",
                    "operation": "analyze_camera",
                    "resource": {"device_id": device_id},
                    "args": {"detect_label": "person", "min_confidence": 0.25}
                }
            ]
        });

        fs::write(
            &plan_path,
            serde_json::to_vec_pretty(&plan).map_err(|e| format!("分析计划生成失败: {e}"))?,
        )
        .map_err(|e| format!("分析计划写入失败 {}: {e}", plan_path.display()))?;

        let agent_bin = self.workspace_root.join("target/debug/harbornas-agent");
        if !agent_bin.exists() {
            return Err(format!(
                "未找到本地执行器 {}，请先编译项目",
                agent_bin.display()
            ));
        }

        let mut command = Command::new(&agent_bin);
        command
            .current_dir(&self.workspace_root)
            .env_remove("HARBOR_VISION_SIDECAR_URL")
            .arg("--plan")
            .arg(&plan_path);

        if let Some(python_bin) = self.local_vision_python_path() {
            command.env("HARBOR_VISION_PYTHON", python_bin);
        }

        let output = command
            .output()
            .map_err(|e| format!("执行本地摄像头分析失败: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "分析执行器退出但没有返回错误细节".to_string()
            } else {
                stderr
            };
            return Err(detail);
        }

        let report: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| format!("分析结果解析失败: {e}"))?;
        let result = report
            .pointer("/task_result/results/0")
            .ok_or("分析结果缺少 step 输出")?;
        let status = result
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("FAILED");
        if status != "SUCCESS" {
            let error_message = result
                .get("error_message")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误");
            return Err(error_message.to_string());
        }

        let payload = result.get("result_payload").ok_or("分析结果缺少 payload")?;
        let device_name = payload
            .pointer("/device/name")
            .and_then(|v| v.as_str())
            .unwrap_or(device_id);
        let summary = payload
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("未生成摘要");
        let detection_summary = payload
            .get("detection_summary")
            .and_then(|v| v.as_str())
            .unwrap_or("未生成检测摘要");
        let image_path = payload
            .pointer("/snapshot/annotated_image_path")
            .and_then(|v| v.as_str())
            .or_else(|| {
                payload
                    .pointer("/snapshot/image_path")
                    .and_then(|v| v.as_str())
            })
            .map(|path| self.workspace_root.join(path).to_string_lossy().to_string());

        Ok(CameraAnalysisReply {
            text: format!(
                "{} 分析完成\n{}\n{}\n抓拍已随消息发送",
                device_name, summary, detection_summary
            ),
            image_path,
        })
    }

    fn build_capability_card(&self) -> Result<CardReply, String> {
        let defaults = self.load_admin_console_defaults()?;
        let admin_state = self.load_admin_console_state()?;
        let devices = self.load_registered_cameras()?;
        let device_count = devices.len();
        let first_device = devices
            .first()
            .map(|device| device.name.as_str())
            .unwrap_or("Living Room Cam");
        let delivery_target = admin_state
            .feishu_users
            .first()
            .map(|user| format!("已绑定用户 {}", user.display_name))
            .unwrap_or_else(|| defaults.feishu_group.clone());

        let card = json!({
            "config": {
                "enable_forward": true,
                "width_mode": "fill"
            },
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": "HarborNAS Agent Hub 已连接"
                },
                "subtitle": {
                    "tag": "plain_text",
                    "content": "主入口在飞书，WebUI 负责配置、验流和审计"
                },
                "text_tag_list": [
                    {
                        "tag": "text_tag",
                        "text": {
                            "tag": "plain_text",
                            "content": format!("已接入 {} 台", device_count)
                        },
                        "color": "green"
                    },
                    {
                        "tag": "text_tag",
                        "text": {
                            "tag": "plain_text",
                            "content": "IM First"
                        },
                        "color": "blue"
                    }
                ],
                "template": "blue"
            },
            "elements": [
                {
                    "tag": "markdown",
                    "content": "你已经完成绑定，后续大多数动作都可以直接在飞书里完成。"
                },
                {
                    "tag": "markdown",
                    "content": format!(
                        "**默认扫描网段**：`{}`\n**默认截图回传**：{}\n**默认分析动作**：{}\n**默认飞书去向**：{}\n**默认 RTSP 探测**：`{}@{} + {} 条路径候选`",
                        defaults.cidr,
                        defaults.capture,
                        defaults.ai,
                        delivery_target,
                        defaults.rtsp_username,
                        defaults.rtsp_port,
                        defaults.rtsp_paths.len()
                    )
                },
                {
                    "tag": "hr"
                },
                {
                    "tag": "markdown",
                    "content": "**现在可以直接说：**\n- `扫描摄像头`\n- `接入 1`\n- `密码 xxxxxx`\n- `看看客厅摄像头`\n- `分析客厅摄像头`"
                },
                {
                    "tag": "note",
                    "elements": [
                        {
                            "tag": "plain_text",
                            "content": format!(
                                "当前设备库：{} 台 | 示例设备：{} | 后台配置台已就绪",
                                device_count, first_device
                            )
                        }
                    ]
                }
            ]
        });

        Ok(CardReply {
            text: format!(
                "HarborNAS Agent Hub 已连接\n默认网段: {}\n默认截图回传: {}\n默认分析动作: {}\n默认飞书去向: {}\n当前设备库: {} 台\n可以直接说：扫描摄像头 / 接入 1 / 密码 xxxxxx / 看看客厅摄像头 / 分析客厅摄像头",
                defaults.cidr, defaults.capture, defaults.ai, delivery_target, device_count
            ),
            card,
        })
    }

    fn build_binding_success_card(&self, user_name: &str) -> Result<CardReply, String> {
        let mut reply = self.build_capability_card()?;
        if let Some(header) = reply
            .card
            .get_mut("header")
            .and_then(|value| value.as_object_mut())
        {
            header.insert("template".to_string(), Value::String("green".to_string()));
            header.insert(
                "subtitle".to_string(),
                json!({
                    "tag": "plain_text",
                    "content": format!("{user_name} 已完成绑定，可以直接在飞书里查看和分析摄像头")
                }),
            );
        }

        if let Some(elements) = reply
            .card
            .get_mut("elements")
            .and_then(|value| value.as_array_mut())
        {
            elements.insert(
                0,
                json!({
                    "tag": "markdown",
                    "content": format!("**绑定成功**\n当前账号：`{}`\n接下来你可以直接发送命令，不需要再回到后台重复配置。", user_name)
                }),
            );
        }

        reply.text = format!(
            "{} 绑定成功。\n你现在可以直接说：扫描摄像头 / 接入 1 / 密码 xxxxxx / 看看客厅摄像头 / 分析客厅摄像头",
            user_name
        );
        Ok(reply)
    }

    fn load_admin_console_defaults(
        &self,
    ) -> Result<harbornas_local_agent::runtime::admin_console::AdminDefaults, String> {
        Ok(self.load_admin_console_state()?.defaults)
    }

    fn load_admin_console_state(
        &self,
    ) -> Result<harbornas_local_agent::runtime::admin_console::AdminConsoleState, String> {
        self.admin_store().load_or_create_state()
    }

    fn load_registered_cameras(
        &self,
    ) -> Result<Vec<harbornas_local_agent::runtime::registry::CameraDevice>, String> {
        self.hub_service().load_registered_cameras()
    }

    fn local_vision_python_path(&self) -> Option<String> {
        let python_bin = self
            .workspace_root
            .join(".harbornas/.venv-vision/bin/python");
        python_bin
            .exists()
            .then(|| python_bin.to_string_lossy().to_string())
    }

    fn scan_cameras_with_defaults(&self, sender: &FeishuSenderIdentity) -> Result<String, String> {
        let scan = self.hub_service().scan(Default::default(), None)?;
        let pending_candidates = pending_candidates_from_results(&scan.results);

        if let Some(mut conversation) = self.load_conversation_state(sender) {
            conversation.pending_candidates = pending_candidates.clone();
            conversation.pending_add = None;
            conversation.last_scan_cidr = scan.defaults.cidr.clone();
            let _ = self.save_conversation_state(sender, &mut conversation);
        }

        let connected = scan.results.iter().filter(|item| item.reachable).count();
        if scan.results.is_empty() {
            return Ok(format!(
                "已按后台默认策略扫描 {}，但当前没有发现可确认的摄像头候选设备。你也可以直接发送：添加摄像头 192.168.x.x",
                scan.defaults.cidr
            ));
        }

        if pending_candidates.is_empty() {
            return Ok(if connected == 0 {
                format!(
                    "已按后台默认策略扫描 {}，共发现 {} 个候选设备，但都还不能直接接入。你可以直接发送：添加摄像头 192.168.x.x",
                    scan.defaults.cidr,
                    scan.results.len()
                )
            } else {
                format!(
                    "已按后台默认策略扫描 {}，成功接入 {} 台摄像头，设备库现在共有 {} 台。接下来可以直接说：看看客厅摄像头 / 分析客厅摄像头",
                    scan.defaults.cidr,
                    connected,
                    scan.devices.len()
                )
            });
        }

        Ok(format!(
            "已按后台默认策略扫描 {}，共发现 {} 台候选设备，已自动接入 {} 台，还剩 {} 台待你确认：\n{}\n请直接回复：`接入 1` 或 `忽略 2`。如果提示需要密码，再回复：`密码 你的RTSP密码`。",
            scan.defaults.cidr,
            scan.results.len(),
            connected,
            pending_candidates.len(),
            format_pending_candidates(&pending_candidates)
        ))
    }

    fn manual_add_camera_with_defaults(
        &self,
        text: &str,
        password_override: Option<&str>,
    ) -> Result<String, String> {
        let ip = extract_ipv4(text)
            .ok_or_else(|| "没有识别到 IP 地址。请直接发送：添加摄像头 192.168.3.73".to_string())?;
        let summary = self.hub_service().manual_add(
            CameraConnectRequest {
                name: format!("Camera {ip}"),
                room: None,
                ip: ip.clone(),
                path_candidates: Vec::new(),
                username: None,
                password: password_override.map(|value| value.trim().to_string()),
                port: None,
                discovery_source: "feishu_manual_add".to_string(),
                vendor: None,
                model: None,
            },
            None,
        )?;

        Ok(format!(
            "已按后台默认策略验证并写入摄像头 {}，设备库现在共有 {} 台。接下来你可以直接说：看看这个摄像头 / 分析这个摄像头 / 给我拍一张客厅",
            summary.device.ip_address.clone().unwrap_or(ip),
            summary.devices.len()
        ))
    }

    fn handle_manual_add_failure(
        &self,
        text: &str,
        sender: &FeishuSenderIdentity,
        error: &str,
    ) -> String {
        if !looks_like_auth_error(error) {
            return format!("添加摄像头失败: {error}");
        }

        let Some(ip) = extract_ipv4(text) else {
            return format!("添加摄像头失败: {error}");
        };

        if let Some(mut conversation) = self.load_conversation_state(sender) {
            let defaults = self.load_admin_console_defaults().ok();
            conversation.pending_add = Some(PendingFeishuAdd {
                name: format!("Camera {ip}"),
                ip: ip.clone(),
                port: defaults
                    .as_ref()
                    .map(|value| value.rtsp_port)
                    .unwrap_or(554),
                rtsp_paths: defaults
                    .map(|value| value.rtsp_paths)
                    .unwrap_or_else(|| vec!["/ch1/main".to_string()]),
                requires_auth: true,
                vendor: None,
                model: None,
            });
            let _ = self.save_conversation_state(sender, &mut conversation);
        }

        format!(
            "摄像头 {} 需要密码才能接入。请直接回复：`密码 你的RTSP密码`。\n我会继续用后台默认用户名和这次提供的密码重试，不需要你重新发一遍 IP。",
            ip
        )
    }

    fn try_continue_pending_camera_add(
        &self,
        text: &str,
        sender: &FeishuSenderIdentity,
    ) -> Option<String> {
        let password = extract_password_reply(text)?;
        let mut conversation = self.load_conversation_state(sender)?;
        let pending = conversation.pending_add.clone()?;

        match self.hub_service().manual_add(
            CameraConnectRequest {
                name: pending.name.clone(),
                room: None,
                ip: pending.ip.clone(),
                path_candidates: pending.rtsp_paths.clone(),
                username: None,
                password: Some(password.clone()),
                port: Some(pending.port),
                discovery_source: "feishu_password_retry".to_string(),
                vendor: pending.vendor.clone(),
                model: pending.model.clone(),
            },
            None,
        ) {
            Ok(summary) => {
                conversation.pending_add = None;
                conversation
                    .pending_candidates
                    .retain(|candidate| candidate.ip != pending.ip);
                let _ = self.save_conversation_state(sender, &mut conversation);
                Some(format!(
                    "密码已收到。\n已接入摄像头 {}，设备库现在共有 {} 台。",
                    summary.device.ip_address.unwrap_or(pending.ip),
                    summary.devices.len()
                ))
            }
            Err(error) if looks_like_auth_error(&error) => {
                let _ = self.save_conversation_state(sender, &mut conversation);
                Some(format!(
                    "这个密码还是不对，摄像头 {} 仍然拒绝认证。请再回复一次：`密码 你的RTSP密码`。",
                    pending.ip
                ))
            }
            Err(error) => Some(format!("添加摄像头失败: {error}")),
        }
    }

    fn try_handle_candidate_command(
        &self,
        text: &str,
        sender: &FeishuSenderIdentity,
    ) -> Option<String> {
        let command = parse_candidate_command(text)?;
        let mut conversation = self.load_conversation_state(sender)?;

        match command {
            CandidateCommand::Connect(index) => {
                if index == 0 || index > conversation.pending_candidates.len() {
                    return Some(
                        "当前没有这个序号的候选设备，请先发送“扫描摄像头”刷新列表。".to_string(),
                    );
                }

                let candidate = conversation.pending_candidates[index - 1].clone();
                conversation.pending_add = Some(PendingFeishuAdd {
                    name: candidate.name.clone(),
                    ip: candidate.ip.clone(),
                    port: candidate.port,
                    rtsp_paths: candidate.rtsp_paths.clone(),
                    requires_auth: candidate.requires_auth,
                    vendor: candidate.vendor.clone(),
                    model: candidate.model.clone(),
                });
                let _ = self.save_conversation_state(sender, &mut conversation);

                match self.hub_service().manual_add(
                    CameraConnectRequest {
                        name: candidate.name.clone(),
                        room: None,
                        ip: candidate.ip.clone(),
                        path_candidates: candidate.rtsp_paths.clone(),
                        username: None,
                        password: None,
                        port: Some(candidate.port),
                        discovery_source: "feishu_candidate_confirm".to_string(),
                        vendor: candidate.vendor.clone(),
                        model: candidate.model.clone(),
                    },
                    None,
                ) {
                    Ok(summary) => {
                        conversation.pending_add = None;
                        conversation
                            .pending_candidates
                            .retain(|item| item.candidate_id != candidate.candidate_id);
                        let _ = self.save_conversation_state(sender, &mut conversation);
                        Some(format!(
                            "已接入 {}（{}），设备库现在共有 {} 台。",
                            candidate.name,
                            candidate.ip,
                            summary.devices.len()
                        ))
                    }
                    Err(error) if looks_like_auth_error(&error) => Some(format!(
                        "摄像头 {}（{}）需要密码。请直接回复：`密码 你的RTSP密码`。",
                        candidate.name, candidate.ip
                    )),
                    Err(error) => Some(format!("接入候选设备失败: {error}")),
                }
            }
            CandidateCommand::Ignore(index) => {
                if index == 0 || index > conversation.pending_candidates.len() {
                    return Some(
                        "当前没有这个序号的候选设备，请先发送“扫描摄像头”刷新列表。".to_string(),
                    );
                }

                let ignored = conversation.pending_candidates.remove(index - 1);
                let _ = self.save_conversation_state(sender, &mut conversation);

                Some(if conversation.pending_candidates.is_empty() {
                    format!(
                        "已忽略 {}（{}）。当前没有待确认候选设备了。",
                        ignored.name, ignored.ip
                    )
                } else {
                    format!(
                        "已忽略 {}（{}）。剩余待确认设备：\n{}",
                        ignored.name,
                        ignored.ip,
                        format_pending_candidates(&conversation.pending_candidates)
                    )
                })
            }
        }
    }

    fn load_conversation_state(
        &self,
        sender: &FeishuSenderIdentity,
    ) -> Option<FeishuConversationState> {
        let key = sender_key(sender)?;
        let mut state = self.conversation_store().load(&key).ok()?;
        state.key = key;
        if state.display_name.trim().is_empty() {
            state.display_name = sender.display_name.clone();
        }
        Some(state)
    }

    fn save_conversation_state(
        &self,
        sender: &FeishuSenderIdentity,
        state: &mut FeishuConversationState,
    ) -> Result<(), String> {
        let key = sender_key(sender).ok_or_else(|| "缺少会话标识".to_string())?;
        state.key = key;
        state.display_name = sender.display_name.clone();
        self.conversation_store().save(state)
    }
}

// ---------------------------------------------------------------------------
// Ping
// ---------------------------------------------------------------------------

fn build_ping_frame(service_id: i32) -> Vec<u8> {
    let frame = PbFrame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: METHOD_CONTROL,
        headers: vec![PbHeader {
            key: "type".into(),
            value: "ping".into(),
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

fn normalize_command_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| {
            !ch.is_whitespace() && !matches!(ch, '，' | '。' | ',' | '.' | '？' | '?' | '！' | '!')
        })
        .collect()
}

fn is_binding_command(text: &str) -> bool {
    let normalized = normalize_command_text(text);
    normalized.contains("绑定") || normalized.contains("bind")
}

fn is_scan_camera_command(normalized: &str) -> bool {
    [
        "扫描摄像头",
        "扫描相机",
        "扫描设备",
        "发现摄像头",
        "scancamera",
        "scancamera",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword))
}

fn is_manual_add_camera_command(normalized: &str) -> bool {
    [
        "添加摄像头",
        "新增摄像头",
        "接入摄像头",
        "addcamera",
        "addcam",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword))
}

fn extract_password_reply(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let patterns = ["摄像头密码", "rtsp密码", "密码"];
    for pattern in patterns {
        if let Some(rest) = trimmed.strip_prefix(pattern) {
            let password = rest.trim().trim_start_matches([':', '：']).trim();
            if !password.is_empty() {
                return Some(password.to_string());
            }
        }
    }
    if looks_like_plain_password(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

fn extract_binding_code(text: &str) -> Option<String> {
    harbornas_local_agent::runtime::admin_console::normalize_binding_code(text).or_else(|| {
        text.split_whitespace()
            .find_map(harbornas_local_agent::runtime::admin_console::normalize_binding_code)
    })
}

fn classify_camera_intent(normalized: &str) -> Option<CameraIntent> {
    let wants_analyze = ["分析", "识别", "检测", "看看有没有人", "有人吗"]
        .iter()
        .any(|keyword| normalized.contains(keyword));
    if wants_analyze {
        return Some(CameraIntent::Analyze);
    }

    let wants_snapshot = [
        "给我拍一张",
        "拍一张",
        "抓拍",
        "截图",
        "快照",
        "看看",
        "看下",
        "查看",
        "看一眼",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword));
    let mentions_camera = [
        "摄像头",
        "相机",
        "cam",
        "camera",
        "客厅",
        "门口",
        "玄关",
        "车库",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword));

    if wants_snapshot && mentions_camera {
        return Some(CameraIntent::Snapshot);
    }

    None
}

fn room_aliases<'a>(name: &'a str, room: &'a str) -> Vec<&'static str> {
    let normalized = format!("{} {}", name.to_lowercase(), room.to_lowercase());
    let mut aliases = Vec::new();
    if normalized.contains("living room") {
        aliases.extend(["客厅", "大厅", "起居室"]);
    }
    if normalized.contains("front door") || normalized.contains("entry") {
        aliases.extend(["门口", "玄关", "入户"]);
    }
    if normalized.contains("garage") {
        aliases.extend(["车库"]);
    }
    aliases
}

fn extract_ipv4(text: &str) -> Option<String> {
    let normalized = text
        .replace('[', " ")
        .replace(']', " ")
        .replace('(', " ")
        .replace(')', " ")
        .replace("http://", " ")
        .replace("https://", " ");
    for token in
        normalized.split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | '，' | ';' | '；'))
    {
        let candidate = token.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.');
        if candidate.parse::<Ipv4Addr>().is_ok() {
            return Some(candidate.to_string());
        }
    }
    None
}

fn sender_key(sender: &FeishuSenderIdentity) -> Option<String> {
    sender
        .chat_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| (!sender.open_id.trim().is_empty()).then(|| sender.open_id.clone()))
}

fn looks_like_plain_password(text: &str) -> bool {
    if text.len() < 4 || text.len() > 64 {
        return false;
    }
    !text.contains(char::is_whitespace)
        && !extract_ipv4(text).is_some()
        && !is_binding_command(text)
        && !is_scan_camera_command(&normalize_command_text(text))
        && !is_manual_add_camera_command(&normalize_command_text(text))
        && parse_candidate_command(text).is_none()
}

fn parse_candidate_command(text: &str) -> Option<CandidateCommand> {
    let compact = normalize_command_text(text);
    if let Some(index) = compact
        .strip_prefix("接入")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return Some(CandidateCommand::Connect(index));
    }
    if let Some(index) = compact
        .strip_prefix("连接")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return Some(CandidateCommand::Connect(index));
    }
    if let Some(index) = compact
        .strip_prefix("ignore")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return Some(CandidateCommand::Ignore(index));
    }
    compact
        .strip_prefix("忽略")
        .and_then(|value| value.parse::<usize>().ok())
        .map(CandidateCommand::Ignore)
}

fn pending_candidates_from_results(results: &[HubScanResultItem]) -> Vec<PendingFeishuCandidate> {
    results
        .iter()
        .filter(|item| !item.reachable)
        .map(|item| PendingFeishuCandidate {
            candidate_id: item.candidate_id.clone(),
            name: item.name.clone(),
            ip: item.ip.clone(),
            port: item.port,
            rtsp_paths: item.rtsp_paths.clone(),
            requires_auth: item.requires_auth,
            vendor: item.vendor.clone(),
            model: item.model.clone(),
        })
        .collect()
}

fn format_pending_candidates(candidates: &[PendingFeishuCandidate]) -> String {
    candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            format!(
                "{}. {}（{}，{}）",
                index + 1,
                candidate.name,
                candidate.ip,
                if candidate.requires_auth {
                    "需要密码"
                } else {
                    "待确认"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    println!("=== Feishu HarborOS Command Bot ===");
    println!("Feishu  : {}", cli.domain);
    println!("HarborOS: {}", cli.harbor_host);
    println!();

    let harbor = HarborOsClient::new(&cli.harbor_host, &cli.harbor_user, &cli.harbor_password);
    let mut bot = FeishuBot::new(&cli.domain, harbor);

    println!("[CHECK] Skipping HarborOS preflight; bot will start immediately.");
    println!();

    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║  HarborOS Command Bot running!                           ║");
    println!("║  Commands: 关闭ssh / 开启ssh / 重启ssh / 查询ssh        ║");
    println!("║            扫描摄像头 / 接入 1 / 密码 xxxxxx            ║");
    println!("║  Other text → echo. Press Ctrl+C to stop.               ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();

    loop {
        let Some((app_id, app_secret)) = resolve_feishu_credentials(&cli) else {
            eprintln!(
                "[WAIT] Feishu 凭证尚未配置。请在手机配置页保存 app_id/app_secret，5 秒后重试。"
            );
            thread::sleep(Duration::from_secs(5));
            continue;
        };

        println!("[1/3] Getting Feishu token ...");
        if let Err(error) = bot.acquire_token(&app_id, &app_secret) {
            eprintln!("FAIL: {error}; retrying in 5s...");
            thread::sleep(Duration::from_secs(5));
            continue;
        }
        println!("  OK — token length={}", bot.token.len());

        println!("[2/3] Getting Feishu WS endpoint ...");
        let (ws_url, service_id) = match bot.get_ws_endpoint(&app_id, &app_secret) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("FAIL: {error}; retrying in 5s...");
                thread::sleep(Duration::from_secs(5));
                continue;
            }
        };
        println!("  OK");

        println!("[3/3] Connecting to Feishu WebSocket ...");
        println!();

        let (mut ws, resp) = match tungstenite::connect(&ws_url) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("[ERROR] Connect failed: {e}; retrying in 3s...");
                thread::sleep(Duration::from_secs(3));
                continue;
            }
        };

        println!("[WS] connected (HTTP {})", resp.status());
        let ping = build_ping_frame(service_id);
        ws.send(tungstenite::Message::Binary(ping.into())).ok();

        let mut msg_count = 0u64;

        let should_reconnect = loop {
            match ws.read() {
                Ok(tungstenite::Message::Binary(data)) => {
                    match PbFrame::decode(data.as_ref()) {
                        Ok(frame) => bot.handle_frame(&frame, &mut ws),
                        Err(e) => println!("[WARN] decode failed: {e}"),
                    }
                    println!("---");
                    msg_count += 1;
                    if msg_count % 10 == 0 {
                        let ping = build_ping_frame(service_id);
                        ws.send(tungstenite::Message::Binary(ping.into())).ok();
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
                    break true;
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[ERROR] WebSocket: {e}");
                    break true;
                }
            }
        };

        if should_reconnect {
            println!("[WS] disconnected; reconnecting in 3s...");
            thread::sleep(Duration::from_secs(3));
            continue;
        }
    }
}

fn resolve_feishu_credentials(cli: &Cli) -> Option<(String, String)> {
    let app_id = cli
        .app_id
        .clone()
        .or_else(|| std::env::var("FEISHU_APP_ID").ok())
        .or_else(load_feishu_app_id_from_admin_state)
        .filter(|value| !value.trim().is_empty())?;
    let app_secret = cli
        .app_secret
        .clone()
        .or_else(|| std::env::var("FEISHU_APP_SECRET").ok())
        .or_else(load_feishu_app_secret_from_admin_state)
        .filter(|value| !value.trim().is_empty())?;
    Some((app_id, app_secret))
}

fn load_feishu_app_id_from_admin_state() -> Option<String> {
    load_feishu_config_from_admin_state().map(|config| config.app_id)
}

fn load_feishu_app_secret_from_admin_state() -> Option<String> {
    load_feishu_config_from_admin_state().map(|config| config.app_secret)
}

fn load_feishu_config_from_admin_state(
) -> Option<harbornas_local_agent::runtime::admin_console::FeishuBotConfig> {
    let workspace_root = std::env::current_dir().ok()?;
    let admin_path = workspace_root.join(".harbornas/admin-console.json");
    let text = fs::read_to_string(admin_path).ok()?;
    let state: harbornas_local_agent::runtime::admin_console::AdminConsoleState =
        serde_json::from_str(&text).ok()?;
    (!state.feishu_bot.app_id.trim().is_empty() && !state.feishu_bot.app_secret.trim().is_empty())
        .then_some(state.feishu_bot)
}
