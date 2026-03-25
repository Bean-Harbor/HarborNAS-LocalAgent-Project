//! Feishu HarborOS Command Bot — receives commands from Feishu, executes on HarborOS.
//!
//! Commands:
//!   关闭ssh / 停止ssh  → service.control STOP ssh  → reply "已关闭"
//!   开启ssh / 启动ssh  → service.control START ssh  → reply "已开启"
//!   重启ssh            → service.control RESTART ssh → reply "已重启"
//!   查询ssh / ssh状态  → service.query ssh          → reply status
//!   Other text         → echo back (fallback)
//!
//! HarborOS interaction: WebSocket at ws://<host>/websocket
//! Feishu interaction: WS v2 protobuf long connection

use clap::Parser;
use prost::Message;
use serde_json::{json, Value};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::WebSocket;

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
        self.headers.iter().find(|h| h.key == key).map(|h| h.value.as_str())
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
    app_id: String,
    #[arg(long, help = "Feishu app_secret")]
    app_secret: String,
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
        let auth_ok = auth_resp.get("result").and_then(|v| v.as_bool()).unwrap_or(false);
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
        let auth_ok = auth_resp.get("result").and_then(|v| v.as_bool()).unwrap_or(false);
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
        }
    }

    fn acquire_token(&mut self, app_id: &str, app_secret: &str) -> Result<(), String> {
        let url = format!("{}/open-apis/auth/v3/tenant_access_token/internal", self.domain);
        let body = json!({"app_id": app_id, "app_secret": app_secret});
        let resp = self.http.post(&url).json(&body).send()
            .map_err(|e| format!("token request failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("code={code}, msg={}", data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")));
        }
        self.token = data.get("tenant_access_token")
            .and_then(|v| v.as_str()).ok_or("no token")?.to_string();
        Ok(())
    }

    fn get_ws_endpoint(&self, app_id: &str, app_secret: &str) -> Result<(String, i32), String> {
        let url = format!("{}/callback/ws/endpoint", self.domain);
        let body = json!({"AppID": app_id, "AppSecret": app_secret});
        let resp = self.http.post(&url).header("locale", "zh").json(&body).send()
            .map_err(|e| format!("request failed: {e}"))?;
        let text = resp.text().unwrap_or_default();
        let resp_body: Value = serde_json::from_str(&text)
            .map_err(|e| format!("parse failed: {e}"))?;
        let code = resp_body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("code={code}, msg=\"{}\"", resp_body.get("msg").and_then(|v| v.as_str()).unwrap_or("?")));
        }
        let data = resp_body.get("data").ok_or("no data")?;
        let ws_url = data.get("URL").or_else(|| data.get("url"))
            .and_then(|v| v.as_str()).ok_or("no URL in data")?;
        let service_id = ws_url.split("service_id=")
            .nth(1).and_then(|s| s.split('&').next())
            .and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
        Ok((ws_url.to_string(), service_id))
    }

    fn reply_text(&self, message_id: &str, text: &str) -> Result<(), String> {
        let url = format!("{}/open-apis/im/v1/messages/{}/reply", self.domain, message_id);
        let body = json!({
            "content": json!({"text": text}).to_string(),
            "msg_type": "text",
        });
        let resp = self.http.post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body).send()
            .map_err(|e| format!("reply failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("code={code}, msg={}", data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")));
        }
        Ok(())
    }

    fn send_to_chat(&self, chat_id: &str, text: &str) -> Result<(), String> {
        let url = format!("{}/open-apis/im/v1/messages?receive_id_type=chat_id", self.domain);
        let body = json!({
            "receive_id": chat_id,
            "content": json!({"text": text}).to_string(),
            "msg_type": "text",
        });
        let resp = self.http.post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body).send()
            .map_err(|e| format!("send failed: {e}"))?;
        let data: Value = resp.json().map_err(|e| format!("parse failed: {e}"))?;
        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("code={code}, msg={}", data.get("msg").and_then(|v| v.as_str()).unwrap_or("?")));
        }
        Ok(())
    }

    fn handle_frame(&self, frame: &PbFrame, ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
        let msg_type = frame.header("type").unwrap_or("");
        match frame.method {
            METHOD_CONTROL => {
                match msg_type {
                    "pong" => println!("[PONG]"),
                    "ping" => println!("[SERVER-PING]"),
                    other => println!("[CONTROL] type={other}"),
                }
            }
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
            Err(e) => { println!("[WARN] payload not JSON: {e}"); return; }
        };

        let event_type = payload.pointer("/header/event_type")
            .and_then(|v| v.as_str()).unwrap_or("(unknown)");
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
        let message = match event.get("message") {
            Some(m) => m,
            None => { println!("[WARN] no message field"); return; }
        };

        let message_id = message.get("message_id").and_then(|v| v.as_str()).unwrap_or("");
        let chat_id = message.get("chat_id").and_then(|v| v.as_str()).unwrap_or("");
        let content_str = message.get("content").and_then(|v| v.as_str()).unwrap_or("{}");
        let content: Value = serde_json::from_str(content_str).unwrap_or_default();
        let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");

        // Strip @mentions
        let clean = text.split_whitespace()
            .filter(|w| !w.starts_with("@_"))
            .collect::<Vec<_>>().join(" ");
        let clean = clean.trim();

        println!("[MSG] \"{clean}\" (msg_id={message_id})");

        if clean.is_empty() { return; }

        // Dispatch command
        let reply = self.dispatch_command(clean);
        println!("[REPLY] \"{reply}\"");

        if !message_id.is_empty() {
            match self.reply_text(message_id, &reply) {
                Ok(()) => { println!("[REPLY] OK"); return; }
                Err(e) => println!("[REPLY] reply API failed: {e}"),
            }
        }
        if !chat_id.is_empty() {
            match self.send_to_chat(chat_id, &reply) {
                Ok(()) => println!("[REPLY] OK (send-to-chat)"),
                Err(e) => println!("[REPLY] send-to-chat failed: {e}"),
            }
        }
    }

    fn dispatch_command(&self, text: &str) -> String {
        let t = text.to_lowercase().replace(' ', "");

        // SSH commands
        if t.contains("关闭ssh") || t.contains("停止ssh") || t.contains("stopssh") {
            return self.cmd_service_control("STOP", "ssh", "已关闭");
        }
        if t.contains("开启ssh") || t.contains("启动ssh") || t.contains("startssh") {
            return self.cmd_service_control("START", "ssh", "已开启");
        }
        if t.contains("重启ssh") || t.contains("restartssh") {
            return self.cmd_service_control("RESTART", "ssh", "已重启");
        }
        if t.contains("查询ssh") || t.contains("ssh状态") || t.contains("sshstatus") {
            return self.cmd_service_query("ssh");
        }

        // Fallback: echo
        text.to_string()
    }

    fn cmd_service_control(&self, action: &str, service: &str, success_msg: &str) -> String {
        println!("[HARBOR] service.control {action} {service} ...");
        match self.harbor.service_control(action, service) {
            Ok(resp) => {
                println!("[HARBOR] response: {resp}");
                let has_error = resp.get("error").is_some();
                if has_error {
                    let reason = resp.get("error")
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
                            let state = svc.get("state").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                            let enable = svc.get("enable").and_then(|v| v.as_bool()).unwrap_or(false);
                            return format!("SSH 状态: {state}, 自启: {}", if enable { "是" } else { "否" });
                        }
                    }
                }
                format!("SSH 状态: 未知 (raw: {resp})")
            }
            Err(e) => format!("查询失败: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Ping
// ---------------------------------------------------------------------------

fn build_ping_frame(service_id: i32) -> Vec<u8> {
    let frame = PbFrame {
        seq_id: 0, log_id: 0,
        service: service_id, method: METHOD_CONTROL,
        headers: vec![PbHeader { key: "type".into(), value: "ping".into() }],
        payload_encoding: String::new(), payload_type: String::new(),
        payload: Vec::new(), log_id_new: String::new(),
    };
    let mut buf = Vec::new();
    frame.encode(&mut buf).unwrap();
    buf
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

    // Pre-check HarborOS connectivity
    println!("[CHECK] Testing HarborOS connectivity ...");
    match bot.harbor.service_query("ssh") {
        Ok(resp) => println!("[CHECK] HarborOS OK — {resp}"),
        Err(e) => {
            eprintln!("[CHECK] WARNING: HarborOS not reachable: {e}");
            eprintln!("        Bot will start anyway; commands may fail at runtime.");
        }
    }
    println!();

    println!("[1/3] Getting Feishu token ...");
    bot.acquire_token(&cli.app_id, &cli.app_secret).unwrap_or_else(|e| {
        eprintln!("FAIL: {e}"); std::process::exit(1);
    });
    println!("  OK — token length={}", bot.token.len());

    println!("[2/3] Getting Feishu WS endpoint ...");
    let (ws_url, service_id) = bot.get_ws_endpoint(&cli.app_id, &cli.app_secret)
        .unwrap_or_else(|e| { eprintln!("FAIL: {e}"); std::process::exit(1); });
    println!("  OK");

    println!("[3/3] Connecting to Feishu WebSocket ...");
    println!();
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║  HarborOS Command Bot running!                           ║");
    println!("║  Commands: 关闭ssh / 开启ssh / 重启ssh / 查询ssh        ║");
    println!("║  Other text → echo. Press Ctrl+C to stop.               ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();

    loop {
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
