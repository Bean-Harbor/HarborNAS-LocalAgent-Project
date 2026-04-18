use std::net::TcpStream;
use std::process::Command;
use std::time::Instant;

use base64::Engine as _;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use crate::orchestrator::contracts::{Action, ExecutionResult, Route, StepStatus};
use crate::orchestrator::router::Executor;

const ALLOWED_READ_ROOTS: [&str; 2] = ["/mnt", "/data"];
const ALLOWED_WRITE_ROOTS: [&str; 3] = ["/mnt", "/data", "/tmp/agent"];
const DENIED_ROOTS: [&str; 5] = ["/", "/etc", "/boot", "/root", "/var/lib"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileOperationContext {
    source_path: String,
    target_path: String,
    recursive: bool,
}

pub struct MiddlewareExecutor {
    available: bool,
}

impl MiddlewareExecutor {
    pub fn new(available: bool) -> Self {
        Self { available }
    }
}

impl Executor for MiddlewareExecutor {
    fn route(&self) -> Route {
        Route::MiddlewareApi
    }

    fn supports(&self, action: &Action) -> bool {
        matches!(action.domain.as_str(), "service" | "files")
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String> {
        let started = Instant::now();

        if std::env::var("HARBOR_FORCE_MIDDLEWARE_ERROR")
            .ok()
            .as_deref()
            == Some("1")
        {
            return Err("forced middleware failure".to_string());
        }

        let (method, call_args, context) = match action.domain.as_str() {
            "service" => {
                let service_name = extract_service_name(action)?;
                let (method, call_args) =
                    map_service_operation(&action.operation, service_name, &action.args)?;
                (
                    method,
                    call_args,
                    json!({
                        "service_name": service_name,
                    }),
                )
            }
            "files" => {
                let file_ctx = extract_file_operation_context(action)?;
                let (method, call_args) = map_files_operation(&action.operation, &file_ctx)?;
                (
                    method,
                    call_args,
                    json!({
                        "source_path": file_ctx.source_path,
                        "target_path": file_ctx.target_path,
                        "recursive": file_ctx.recursive,
                    }),
                )
            }
            other => return Err(format!("unsupported harbor domain: {other}")),
        };

        Ok(ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: Route::MiddlewareApi.as_str().to_string(),
            fallback_used: false,
            status: StepStatus::Success,
            duration_ms: started.elapsed().as_millis() as u64,
            error_code: None,
            error_message: None,
            audit_ref: String::new(),
            result_payload: json!({
                "method": method,
                "args": call_args,
                "context": context,
                "note": "middleware_api preview mode",
            }),
        })
    }
}

pub struct MidcliExecutor {
    available: bool,
    bin: String,
    passthrough: bool,
}

impl MidcliExecutor {
    pub fn new(available: bool, bin: String, passthrough: bool) -> Self {
        Self {
            available,
            bin,
            passthrough,
        }
    }
}

impl Executor for MidcliExecutor {
    fn route(&self) -> Route {
        Route::Midcli
    }

    fn supports(&self, action: &Action) -> bool {
        matches!(action.domain.as_str(), "service" | "files")
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String> {
        let started = Instant::now();
        let (command, context) = match action.domain.as_str() {
            "service" => {
                let service_name = extract_service_name(action)?;
                (
                    build_midcli_service_command(&action.operation, service_name, &action.args)?,
                    json!({
                        "service_name": service_name,
                    }),
                )
            }
            "files" => {
                let file_ctx = extract_file_operation_context(action)?;
                (
                    build_midcli_files_command(&action.operation, &file_ctx)?,
                    json!({
                        "source_path": file_ctx.source_path,
                        "target_path": file_ctx.target_path,
                        "recursive": file_ctx.recursive,
                    }),
                )
            }
            other => return Err(format!("unsupported harbor domain: {other}")),
        };

        let payload = if self.passthrough {
            let output = Command::new(&self.bin)
                .args(command.iter())
                .output()
                .map_err(|e| format!("midcli spawn error: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "midcli command failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            json!({
                "stdout": String::from_utf8_lossy(&output.stdout),
                "command": command,
                "passthrough": true,
            })
        } else {
            json!({
                "command": command,
                "context": context,
                "passthrough": false,
                "note": "midcli preview mode",
            })
        };

        Ok(ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: Route::Midcli.as_str().to_string(),
            fallback_used: false,
            status: StepStatus::Success,
            duration_ms: started.elapsed().as_millis() as u64,
            error_code: None,
            error_message: None,
            audit_ref: String::new(),
            result_payload: payload,
        })
    }
}

fn map_service_operation(
    operation: &str,
    service_name: &str,
    args: &serde_json::Value,
) -> Result<(String, serde_json::Value), String> {
    match operation {
        "status" => Ok(("service.query".to_string(), json!([service_name]))),
        "start" | "stop" | "restart" => Ok((
            "service.control".to_string(),
            json!([operation.to_uppercase(), service_name, {}]),
        )),
        "enable" => {
            let enable_val = args.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
            Ok((
                "service.update".to_string(),
                json!([service_name, {"enable": enable_val}]),
            ))
        }
        _ => Err(format!("Unmapped service operation: {operation}")),
    }
}

fn map_files_operation(
    operation: &str,
    file_ctx: &FileOperationContext,
) -> Result<(String, serde_json::Value), String> {
    match operation {
        "copy" => Ok((
            "filesystem.copy".to_string(),
            json!([
                file_ctx.source_path,
                file_ctx.target_path,
                {
                    "recursive": file_ctx.recursive,
                    "preserve_attrs": false
                }
            ]),
        )),
        "move" => Ok((
            "filesystem.move".to_string(),
            json!([[file_ctx.source_path], file_ctx.target_path, {"recursive": file_ctx.recursive}]),
        )),
        _ => Err(format!("Unmapped file operation: {operation}")),
    }
}

fn build_midcli_service_command(
    operation: &str,
    service_name: &str,
    args: &serde_json::Value,
) -> Result<Vec<String>, String> {
    match operation {
        "status" => Ok(vec![
            "service".to_string(),
            service_name.to_string(),
            "show".to_string(),
        ]),
        "start" => Ok(vec![
            "service".to_string(),
            "start".to_string(),
            format!("service={service_name}"),
        ]),
        "stop" => Ok(vec![
            "service".to_string(),
            "stop".to_string(),
            format!("service={service_name}"),
        ]),
        "restart" => Ok(vec![
            "service".to_string(),
            "restart".to_string(),
            format!("service={service_name}"),
        ]),
        "enable" => {
            let enable = args.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
            Ok(vec![
                "service".to_string(),
                "update".to_string(),
                format!("id_or_name={service_name}"),
                format!("enable={enable}"),
            ])
        }
        _ => Err(format!("Unmapped midcli operation: {operation}")),
    }
}

fn build_midcli_files_command(
    operation: &str,
    file_ctx: &FileOperationContext,
) -> Result<Vec<String>, String> {
    match operation {
        "copy" => {
            let mut command = vec![
                "filesystem".to_string(),
                "copy".to_string(),
                format!("src={}", file_ctx.source_path),
                format!("dst={}", file_ctx.target_path),
            ];
            if file_ctx.recursive {
                command.push("recursive=true".to_string());
            }
            Ok(command)
        }
        "move" => {
            let mut command = vec![
                "filesystem".to_string(),
                "move".to_string(),
                format!("src={}", file_ctx.source_path),
                format!("dst={}", file_ctx.target_path),
            ];
            if file_ctx.recursive {
                command.push("recursive=true".to_string());
            }
            Ok(command)
        }
        _ => Err(format!("Unmapped midcli operation: {operation}")),
    }
}

fn extract_service_name<'a>(action: &'a Action) -> Result<&'a str, String> {
    let service_name = action
        .resource
        .get("service_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "service_name is required".to_string())?;
    validate_service_name(service_name)?;
    Ok(service_name)
}

fn validate_service_name(service_name: &str) -> Result<(), String> {
    if service_name.is_empty() {
        return Err("service_name is required".to_string());
    }
    if service_name.len() > 64
        || !service_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(format!("invalid service name: {service_name:?}"));
    }
    Ok(())
}

fn extract_file_operation_context(action: &Action) -> Result<FileOperationContext, String> {
    let source_path = action
        .resource
        .get("paths")
        .and_then(|value| value.as_array())
        .and_then(|paths| paths.first())
        .and_then(|value| value.as_str())
        .or_else(|| {
            action
                .resource
                .get("source")
                .and_then(|value| value.as_str())
        })
        .or_else(|| action.resource.get("src").and_then(|value| value.as_str()))
        .ok_or_else(|| "files action requires resource.paths[0] or resource.source".to_string())?;
    let target_path = action
        .resource
        .get("target")
        .and_then(|value| value.as_str())
        .or_else(|| {
            action
                .resource
                .get("destination")
                .and_then(|value| value.as_str())
        })
        .or_else(|| action.resource.get("dst").and_then(|value| value.as_str()))
        .ok_or_else(|| {
            "files action requires resource.target or resource.destination".to_string()
        })?;

    let source_path = normalize_contract_path(source_path)?;
    let target_path = normalize_contract_path(target_path)?;
    validate_file_paths(&source_path, &target_path)?;

    Ok(FileOperationContext {
        source_path,
        target_path,
        recursive: action
            .args
            .get("recursive")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    })
}

fn normalize_contract_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("path is required".to_string());
    }

    let unix_like = trimmed.replace('\\', "/");
    if !unix_like.starts_with('/') {
        return Err(format!("path must be absolute: {trimmed:?}"));
    }

    let mut segments: Vec<&str> = Vec::new();
    for segment in unix_like.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if segments.pop().is_none() {
                return Err(format!("path escapes root: {trimmed:?}"));
            }
            continue;
        }
        segments.push(segment);
    }

    if segments.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", segments.join("/")))
    }
}

fn validate_file_paths(source_path: &str, target_path: &str) -> Result<(), String> {
    for path in [source_path, target_path] {
        if DENIED_ROOTS.iter().any(|root| is_under_root(path, root)) {
            return Err(format!("denied path: {path}"));
        }
    }

    if !ALLOWED_READ_ROOTS
        .iter()
        .any(|root| is_under_root(source_path, root))
    {
        return Err(format!("read path outside allowlist: {source_path}"));
    }

    if !ALLOWED_WRITE_ROOTS
        .iter()
        .any(|root| is_under_root(target_path, root))
    {
        return Err(format!("write path outside allowlist: {target_path}"));
    }

    Ok(())
}

fn is_under_root(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .map(|suffix| suffix.starts_with('/'))
            .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// MiddlewareHttpExecutor — calls TrueNAS/HarborOS REST API over HTTP
// Route priority: this is a MiddlewareApi executor, takes precedence over Midcli.
// ---------------------------------------------------------------------------

pub struct MiddlewareHttpExecutor {
    base_url: String,
    client: Client,
    auth_header: HeaderValue,
}

impl MiddlewareHttpExecutor {
    /// Create with API key auth: `Authorization: Bearer <api_key>`
    pub fn with_api_key(base_url: &str, api_key: &str) -> Result<Self, String> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true) // HarborOS often uses self-signed certs
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        let header_val = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|e| format!("invalid api key header: {e}"))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            auth_header: header_val,
        })
    }

    /// Create with basic auth (username:password)
    pub fn with_basic_auth(base_url: &str, username: &str, password: &str) -> Result<Self, String> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        let header_val = HeaderValue::from_str(&format!("Basic {encoded}"))
            .map_err(|e| format!("invalid basic auth header: {e}"))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            auth_header: header_val,
        })
    }

    fn default_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, self.auth_header.clone());
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }
}

impl Executor for MiddlewareHttpExecutor {
    fn route(&self) -> Route {
        Route::MiddlewareApi
    }

    fn supports(&self, action: &Action) -> bool {
        action.domain == "service"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String> {
        let started = Instant::now();

        let service_name = extract_service_name(action)?;

        let (method, url, body) = match action.operation.as_str() {
            "status" => (
                "GET",
                format!("{}/api/v2.0/service/id/{service_name}", self.base_url),
                None,
            ),
            "start" => (
                "POST",
                format!("{}/api/v2.0/service/start", self.base_url),
                Some(json!({"service": service_name})),
            ),
            "stop" => (
                "POST",
                format!("{}/api/v2.0/service/stop", self.base_url),
                Some(json!({"service": service_name})),
            ),
            "restart" => (
                "POST",
                format!("{}/api/v2.0/service/restart", self.base_url),
                Some(json!({"service": service_name})),
            ),
            "enable" => {
                let enable_val = action
                    .args
                    .get("enable")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                (
                    "PUT",
                    format!("{}/api/v2.0/service/id/{service_name}", self.base_url),
                    Some(json!({"enable": enable_val})),
                )
            }
            other => return Err(format!("unsupported service operation: {other}")),
        };

        if action.dry_run {
            return Ok(ExecutionResult {
                task_id: task_id.to_string(),
                step_id: step_id.to_string(),
                executor_used: Route::MiddlewareApi.as_str().to_string(),
                fallback_used: false,
                status: StepStatus::Success,
                duration_ms: started.elapsed().as_millis() as u64,
                error_code: None,
                error_message: None,
                audit_ref: String::new(),
                result_payload: json!({
                    "dry_run": true,
                    "method": method,
                    "url": url,
                    "body": body,
                }),
            });
        }

        let headers = self.default_headers();
        let response = match method {
            "GET" => self
                .client
                .get(&url)
                .headers(headers)
                .send()
                .map_err(|e| format!("HTTP GET failed: {e}"))?,
            "POST" => self
                .client
                .post(&url)
                .headers(headers)
                .json(&body)
                .send()
                .map_err(|e| format!("HTTP POST failed: {e}"))?,
            "PUT" => self
                .client
                .put(&url)
                .headers(headers)
                .json(&body)
                .send()
                .map_err(|e| format!("HTTP PUT failed: {e}"))?,
            _ => unreachable!(),
        };

        let status_code = response.status().as_u16();
        let resp_text = response.text().unwrap_or_else(|_| String::new());

        let resp_json: serde_json::Value =
            serde_json::from_str(&resp_text).unwrap_or(json!({"raw": resp_text}));

        if status_code >= 400 {
            return Err(format!(
                "API returned HTTP {status_code}: {}",
                resp_text.chars().take(500).collect::<String>()
            ));
        }

        Ok(ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: Route::MiddlewareApi.as_str().to_string(),
            fallback_used: false,
            status: StepStatus::Success,
            duration_ms: started.elapsed().as_millis() as u64,
            error_code: None,
            error_message: None,
            audit_ref: String::new(),
            result_payload: json!({
                "http_status": status_code,
                "method": method,
                "url": url,
                "response": resp_json,
            }),
        })
    }
}

// ---------------------------------------------------------------------------
// MiddlewareWsExecutor — calls HarborOS via WebSocket middleware protocol
// This is the primary live integration path when REST API is not available.
// Protocol: connect to ws://host/websocket, auth.login, then call methods.
// ---------------------------------------------------------------------------

pub struct MiddlewareWsExecutor {
    ws_url: String,
    username: String,
    password: String,
}

impl MiddlewareWsExecutor {
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        // Convert http://host to ws://host/websocket
        let ws_url = base_url
            .trim_end_matches('/')
            .replace("https://", "wss://")
            .replace("http://", "ws://")
            + "/websocket";
        Self {
            ws_url,
            username: username.to_string(),
            password: password.to_string(),
        }
    }

    fn connect_and_auth(&self) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
        let (mut ws, _resp) =
            connect(&self.ws_url).map_err(|e| format!("ws connect failed: {e}"))?;

        // Handshake: {"msg":"connect","version":"1","support":["1"]}
        ws.send(Message::Text(
            json!({"msg": "connect", "version": "1", "support": ["1"]})
                .to_string()
                .into(),
        ))
        .map_err(|e| format!("ws handshake send failed: {e}"))?;

        let hs_resp = ws
            .read()
            .map_err(|e| format!("ws handshake recv failed: {e}"))?;
        let hs_json: serde_json::Value =
            serde_json::from_str(hs_resp.to_text().unwrap_or("")).unwrap_or_default();
        if hs_json.get("msg").and_then(|v| v.as_str()) != Some("connected") {
            return Err(format!("ws handshake unexpected: {hs_json}"));
        }

        // Auth: auth.login
        ws.send(Message::Text(
            json!({
                "id": 1,
                "msg": "method",
                "method": "auth.login",
                "params": [self.username, self.password]
            })
            .to_string()
            .into(),
        ))
        .map_err(|e| format!("ws auth send failed: {e}"))?;

        let auth_resp = ws.read().map_err(|e| format!("ws auth recv failed: {e}"))?;
        let auth_json: serde_json::Value =
            serde_json::from_str(auth_resp.to_text().unwrap_or("")).unwrap_or_default();
        if auth_json.get("msg").and_then(|v| v.as_str()) != Some("result") {
            return Err(format!("ws auth failed: {auth_json}"));
        }
        // auth.login returns result=true on success
        let auth_result = &auth_json["result"];
        if auth_result != &serde_json::Value::Bool(true) {
            return Err(format!("ws auth rejected: {auth_json}"));
        }

        Ok(ws)
    }

    fn ws_call(
        &self,
        ws: &mut WebSocket<MaybeTlsStream<TcpStream>>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        ws.send(Message::Text(
            json!({
                "id": 2,
                "msg": "method",
                "method": method,
                "params": params
            })
            .to_string()
            .into(),
        ))
        .map_err(|e| format!("ws call send failed: {e}"))?;

        let resp = ws.read().map_err(|e| format!("ws call recv failed: {e}"))?;
        let resp_json: serde_json::Value =
            serde_json::from_str(resp.to_text().unwrap_or("")).unwrap_or_default();

        if resp_json.get("msg").and_then(|v| v.as_str()) != Some("result") {
            return Err(format!("ws method call failed: {resp_json}"));
        }
        if let Some(err) = resp_json.get("error") {
            return Err(format!("ws method error: {err}"));
        }

        Ok(resp_json["result"].clone())
    }
}

impl Executor for MiddlewareWsExecutor {
    fn route(&self) -> Route {
        Route::MiddlewareApi
    }

    fn supports(&self, action: &Action) -> bool {
        matches!(action.domain.as_str(), "service" | "files")
    }

    fn is_available(&self) -> bool {
        true
    }

    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String> {
        let started = Instant::now();

        let (method, params, context) = match action.domain.as_str() {
            "service" => {
                let service_name = extract_service_name(action)?;
                let (method, params) =
                    map_ws_service_operation(&action.operation, service_name, &action.args)?;
                (
                    method,
                    params,
                    json!({
                        "service_name": service_name,
                    }),
                )
            }
            "files" => {
                let file_ctx = extract_file_operation_context(action)?;
                let (method, params) = map_ws_file_operation(&action.operation, &file_ctx)?;
                (
                    method,
                    params,
                    json!({
                        "source_path": file_ctx.source_path,
                        "target_path": file_ctx.target_path,
                        "recursive": file_ctx.recursive,
                    }),
                )
            }
            other => return Err(format!("unsupported harbor domain: {other}")),
        };

        if action.dry_run {
            return Ok(ExecutionResult {
                task_id: task_id.to_string(),
                step_id: step_id.to_string(),
                executor_used: Route::MiddlewareApi.as_str().to_string(),
                fallback_used: false,
                status: StepStatus::Success,
                duration_ms: started.elapsed().as_millis() as u64,
                error_code: None,
                error_message: None,
                audit_ref: String::new(),
                result_payload: json!({
                    "dry_run": true,
                    "transport": "websocket",
                    "method": method,
                    "params": params,
                    "context": context,
                }),
            });
        }

        let mut ws = self.connect_and_auth()?;
        let result = self.ws_call(&mut ws, &method, params)?;
        let _ = ws.close(None);

        Ok(ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: Route::MiddlewareApi.as_str().to_string(),
            fallback_used: false,
            status: StepStatus::Success,
            duration_ms: started.elapsed().as_millis() as u64,
            error_code: None,
            error_message: None,
            audit_ref: String::new(),
            result_payload: json!({
                "transport": "websocket",
                "method": method,
                "context": context,
                "response": result,
            }),
        })
    }
}

fn map_ws_service_operation(
    operation: &str,
    service_name: &str,
    args: &serde_json::Value,
) -> Result<(String, serde_json::Value), String> {
    match operation {
        "status" => Ok((
            "service.query".to_string(),
            json!([[["service", "=", service_name]], {"select": ["service", "state", "enable"]}]),
        )),
        "start" | "stop" | "restart" => Ok((
            "service.control".to_string(),
            json!([operation.to_uppercase(), service_name, {}]),
        )),
        "enable" => {
            let enable_val = args.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
            Ok((
                "service.update".to_string(),
                json!([service_name, {"enable": enable_val}]),
            ))
        }
        _ => Err(format!("unsupported ws service operation: {operation}")),
    }
}

fn map_ws_file_operation(
    operation: &str,
    file_ctx: &FileOperationContext,
) -> Result<(String, serde_json::Value), String> {
    map_files_operation(operation, file_ctx)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::orchestrator::contracts::{Action, RiskLevel};
    use crate::orchestrator::router::Executor;

    use super::{MidcliExecutor, MiddlewareExecutor, MiddlewareWsExecutor};

    #[test]
    fn middleware_preview_maps_file_copy_from_contract_shape() {
        let executor = MiddlewareExecutor::new(true);
        let action = Action {
            domain: "files".to_string(),
            operation: "copy".to_string(),
            resource: json!({
                "paths": ["/mnt/data/../inbox/file.txt"],
                "target": "/tmp/agent/output/file.txt"
            }),
            args: json!({
                "recursive": true
            }),
            risk_level: RiskLevel::Medium,
            requires_approval: false,
            dry_run: false,
        };

        let result = executor
            .execute(&action, "task-1", "step-1")
            .expect("preview result");

        assert_eq!(result.executor_used, "middleware_api");
        assert_eq!(result.result_payload["method"], "filesystem.copy");
        assert_eq!(result.result_payload["args"][0], "/mnt/inbox/file.txt");
        assert_eq!(
            result.result_payload["args"][1],
            "/tmp/agent/output/file.txt"
        );
        assert_eq!(result.result_payload["args"][2]["recursive"], true);
        assert_eq!(
            result.result_payload["context"]["source_path"],
            "/mnt/inbox/file.txt"
        );
    }

    #[test]
    fn midcli_preview_builds_file_move_command() {
        let executor = MidcliExecutor::new(true, "midcli".to_string(), false);
        let action = Action {
            domain: "files".to_string(),
            operation: "move".to_string(),
            resource: json!({
                "source": "/mnt/source.txt",
                "destination": "/tmp/agent/archive"
            }),
            args: json!({
                "recursive": true
            }),
            risk_level: RiskLevel::High,
            requires_approval: true,
            dry_run: false,
        };

        let result = executor
            .execute(&action, "task-2", "step-1")
            .expect("midcli preview");

        assert_eq!(result.executor_used, "midcli");
        assert_eq!(
            result.result_payload["command"],
            json!([
                "filesystem",
                "move",
                "src=/mnt/source.txt",
                "dst=/tmp/agent/archive",
                "recursive=true"
            ])
        );
        assert_eq!(
            result.result_payload["context"]["target_path"],
            "/tmp/agent/archive"
        );
    }

    #[test]
    fn file_paths_are_rejected_before_midcli_execution() {
        let executor = MidcliExecutor::new(true, "midcli".to_string(), false);
        let action = Action {
            domain: "files".to_string(),
            operation: "copy".to_string(),
            resource: json!({
                "source": "/etc/passwd",
                "target": "/mnt/agent-ci/out.txt"
            }),
            args: json!({}),
            risk_level: RiskLevel::Medium,
            requires_approval: false,
            dry_run: false,
        };

        let err = executor
            .execute(&action, "task-3", "step-1")
            .expect_err("denied path");

        assert!(err.contains("denied path"));
    }

    #[test]
    fn websocket_dry_run_previews_files_without_network_io() {
        let executor = MiddlewareWsExecutor::new("http://nas.local", "root", "secret");
        let action = Action {
            domain: "files".to_string(),
            operation: "move".to_string(),
            resource: json!({
                "source": "/mnt/source.txt",
                "target": "/tmp/agent/archive"
            }),
            args: json!({
                "recursive": false
            }),
            risk_level: RiskLevel::High,
            requires_approval: true,
            dry_run: true,
        };

        let result = executor
            .execute(&action, "task-4", "step-1")
            .expect("ws dry-run preview");

        assert_eq!(result.executor_used, "middleware_api");
        assert_eq!(result.result_payload["dry_run"], true);
        assert_eq!(result.result_payload["transport"], "websocket");
        assert_eq!(result.result_payload["method"], "filesystem.move");
        assert_eq!(result.result_payload["params"][0][0], "/mnt/source.txt");
        assert_eq!(result.result_payload["params"][1], "/tmp/agent/archive");
    }

    #[test]
    fn harbor_file_actions_are_supported_by_harbor_executors() {
        let action = Action {
            domain: "files".to_string(),
            operation: "copy".to_string(),
            resource: json!({
                "source": "/mnt/source.txt",
                "target": "/tmp/agent/out.txt"
            }),
            args: json!({}),
            risk_level: RiskLevel::Medium,
            requires_approval: false,
            dry_run: false,
        };

        assert!(MiddlewareExecutor::new(true).supports(&action));
        assert!(MidcliExecutor::new(true, "midcli".to_string(), false).supports(&action));
        assert!(MiddlewareWsExecutor::new("http://nas.local", "root", "secret").supports(&action));
    }
}
