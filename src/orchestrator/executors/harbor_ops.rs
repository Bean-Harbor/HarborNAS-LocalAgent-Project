use std::process::Command;
use std::time::Instant;

use base64::Engine as _;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;

use crate::orchestrator::contracts::{Action, ExecutionResult, Route, StepStatus};
use crate::orchestrator::router::Executor;

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

        if std::env::var("HARBOR_FORCE_MIDDLEWARE_ERROR").ok().as_deref() == Some("1") {
            return Err("forced middleware failure".to_string());
        }

        let service_name = action
            .resource
            .get("service_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (method, call_args) = map_service_operation(&action.operation, service_name, &action.args)?;

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
        let service_name = action
            .resource
            .get("service_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let command = build_midcli_command(&action.operation, service_name, &action.args)?;

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

fn build_midcli_command(
    operation: &str,
    service_name: &str,
    args: &serde_json::Value,
) -> Result<Vec<String>, String> {
    match operation {
        "status" => Ok(vec!["service".to_string(), service_name.to_string(), "show".to_string()]),
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
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("{username}:{password}"));
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

        let service_name = action
            .resource
            .get("service_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if service_name.is_empty() {
            return Err("service_name is required".to_string());
        }
        // Validate service name (alphanumeric + underscore/hyphen, max 64 chars)
        if service_name.len() > 64
            || !service_name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            return Err(format!("invalid service name: {service_name:?}"));
        }

        let (method, url, body) = match action.operation.as_str() {
            "status" => (
                "GET",
                format!("{}/api/v2.0/service?service={service_name}", self.base_url),
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
        let resp_text = response
            .text()
            .unwrap_or_else(|_| String::new());

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
