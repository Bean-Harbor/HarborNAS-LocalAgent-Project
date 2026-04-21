//! Model-center helpers for admin redaction, endpoint tests, OCR routing, and
//! VLM summary execution.

use base64::Engine as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::connectors::ai_provider::{
    OpenAiCompatibleConfig, OpenAiCompatibleTextClient, OpenAiCompatibleVisionClient,
    TextCompletionRequest, VisionSummaryRequest,
};
use crate::control_plane::models::{ModelEndpoint, ModelEndpointStatus, ModelKind};
use crate::runtime::admin_console::{
    sanitize_model_center_state, AdminConsoleState, AdminModelCenterState,
};

pub const ADMIN_STATE_PATH_ENV: &str = "HARBOR_ADMIN_STATE_PATH";
pub const OCR_TESSERACT_PATH_ENV: &str = "HARBOR_OCR_TESSERACT_PATH";
pub const OCR_TESSERACT_LANGS_ENV: &str = "HARBOR_OCR_LANGS";
const OCR_POLICY_ID: &str = "retrieval.ocr";
const LLM_POLICY_ID: &str = "retrieval.answer";
const VLM_POLICY_ID: &str = "retrieval.vision_summary";
const DEFAULT_ADMIN_STATE_PATH: &str = ".harborbeacon/admin-console.json";
const DEFAULT_TESSERACT_LANGS: &str = "chi_sim+eng";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEndpointTestResult {
    pub ok: bool,
    pub status: String,
    pub summary: String,
    pub endpoint: ModelEndpoint,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OcrExecution {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub provider_key: String,
    #[serde(default)]
    pub model_endpoint_id: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VlmSummaryExecution {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub provider_key: String,
    #[serde(default)]
    pub model_endpoint_id: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LlmTextExecution {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub provider_key: String,
    #[serde(default)]
    pub model_endpoint_id: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub details: Value,
}

pub fn default_admin_state_path() -> PathBuf {
    std::env::var(ADMIN_STATE_PATH_ENV)
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ADMIN_STATE_PATH))
}

pub fn load_model_center_state() -> AdminModelCenterState {
    load_model_center_state_from_path(&default_admin_state_path())
}

pub fn load_model_center_state_from_path(path: &Path) -> AdminModelCenterState {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return AdminModelCenterState::default(),
    };
    let state = match serde_json::from_str::<AdminConsoleState>(&text) {
        Ok(state) => state,
        Err(_) => return AdminModelCenterState::default(),
    };
    sanitize_model_center_state(state.models)
}

pub fn redact_model_center_state(state: &AdminModelCenterState) -> AdminModelCenterState {
    AdminModelCenterState {
        endpoints: state.endpoints.iter().map(redact_model_endpoint).collect(),
        route_policies: state.route_policies.clone(),
    }
}

pub fn redact_model_endpoint(endpoint: &ModelEndpoint) -> ModelEndpoint {
    let mut redacted = endpoint.clone();
    redact_secret_value(&mut redacted.metadata);
    redacted
}

pub fn test_model_endpoint(endpoint: &ModelEndpoint) -> ModelEndpointTestResult {
    if let Some(mock_text) = metadata_string(&endpoint.metadata, "mock_text") {
        return ModelEndpointTestResult {
            ok: !mock_text.trim().is_empty(),
            status: "active".to_string(),
            summary: "Mock model endpoint is configured for local tests.".to_string(),
            endpoint: redact_model_endpoint(endpoint),
            details: json!({
                "mock_text_length": mock_text.chars().count(),
            }),
        };
    }

    if endpoint.model_kind == ModelKind::Ocr
        && endpoint.provider_key.eq_ignore_ascii_case("tesseract")
    {
        return test_tesseract_endpoint(&endpoint);
    }

    test_http_endpoint(&endpoint)
}

pub fn run_ocr(image_path: &Path) -> OcrExecution {
    let state = load_model_center_state();
    run_ocr_with_state(image_path, &state)
}

pub fn run_ocr_with_state(image_path: &Path, state: &AdminModelCenterState) -> OcrExecution {
    let Some(endpoint) = resolve_endpoint(state, ModelKind::Ocr, OCR_POLICY_ID) else {
        return OcrExecution {
            available: false,
            status: "disabled".to_string(),
            summary: "No OCR endpoint is enabled.".to_string(),
            provider_key: String::new(),
            model_endpoint_id: None,
            text: String::new(),
            details: json!({}),
        };
    };

    if let Some(mock_text) = metadata_string(&endpoint.metadata, "mock_text") {
        return OcrExecution {
            available: !mock_text.trim().is_empty(),
            status: "active".to_string(),
            summary: "Mock OCR endpoint resolved.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: mock_text,
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    if !endpoint.provider_key.eq_ignore_ascii_case("tesseract") {
        return OcrExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!(
                "OCR endpoint {} is configured, but provider {} is not implemented yet.",
                endpoint.model_endpoint_id, endpoint.provider_key
            ),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    let Some(binary_path) = resolve_tesseract_binary(&endpoint) else {
        return OcrExecution {
            available: false,
            status: "degraded".to_string(),
            summary: "Tesseract is not available on this host.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "languages": resolve_tesseract_languages(&endpoint),
            }),
        };
    };

    let output = Command::new(&binary_path)
        .arg(image_path)
        .arg("stdout")
        .arg("-l")
        .arg(resolve_tesseract_languages(&endpoint))
        .arg("--psm")
        .arg("3")
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                OcrExecution {
                    available: false,
                    status: "degraded".to_string(),
                    summary: "OCR completed, but no text was extracted.".to_string(),
                    provider_key: endpoint.provider_key.clone(),
                    model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
                    text,
                    details: json!({
                        "binary_path": binary_path.to_string_lossy(),
                    }),
                }
            } else {
                OcrExecution {
                    available: true,
                    status: "active".to_string(),
                    summary: "OCR text extracted from image.".to_string(),
                    provider_key: endpoint.provider_key.clone(),
                    model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
                    text,
                    details: json!({
                        "binary_path": binary_path.to_string_lossy(),
                        "languages": resolve_tesseract_languages(&endpoint),
                    }),
                }
            }
        }
        Ok(output) => OcrExecution {
            available: false,
            status: "degraded".to_string(),
            summary: "Tesseract command failed.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "binary_path": binary_path.to_string_lossy(),
                "stderr": String::from_utf8_lossy(&output.stderr).trim(),
            }),
        },
        Err(error) => OcrExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!("Failed to start tesseract: {error}"),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "binary_path": binary_path.to_string_lossy(),
            }),
        },
    }
}

pub fn run_vlm_summary(image_path: &Path) -> VlmSummaryExecution {
    let state = load_model_center_state();
    run_vlm_summary_with_state(image_path, &state)
}

pub fn run_vlm_summary_with_state(
    image_path: &Path,
    state: &AdminModelCenterState,
) -> VlmSummaryExecution {
    let Some(endpoint) = resolve_endpoint(state, ModelKind::Vlm, VLM_POLICY_ID) else {
        return VlmSummaryExecution {
            available: false,
            status: "disabled".to_string(),
            summary: "No VLM endpoint is enabled.".to_string(),
            provider_key: String::new(),
            model_endpoint_id: None,
            text: String::new(),
            details: json!({}),
        };
    };

    if let Some(mock_text) = metadata_string(&endpoint.metadata, "mock_text") {
        return VlmSummaryExecution {
            available: !mock_text.trim().is_empty(),
            status: "active".to_string(),
            summary: "Mock VLM endpoint resolved.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: mock_text,
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    if !endpoint
        .provider_key
        .eq_ignore_ascii_case("openai_compatible")
    {
        return VlmSummaryExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!(
                "VLM endpoint {} is configured, but provider {} is not implemented yet.",
                endpoint.model_endpoint_id, endpoint.provider_key
            ),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    let Some(config) = openai_compatible_config_from_endpoint(&endpoint) else {
        return VlmSummaryExecution {
            available: false,
            status: "degraded".to_string(),
            summary: "VLM endpoint base_url / api_key / model_name are not configured.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    };

    let image_data_url = match build_image_data_url(image_path) {
        Ok(value) => value,
        Err(error) => {
            return VlmSummaryExecution {
                available: false,
                status: "degraded".to_string(),
                summary: error,
                provider_key: endpoint.provider_key.clone(),
                model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
                text: String::new(),
                details: json!({
                    "image_path": image_path.to_string_lossy(),
                }),
            };
        }
    };

    let prompt = metadata_string(&endpoint.metadata, "prompt").or_else(|| {
        Some(
            "请用中文概括这张图片、截图或摄像头静帧的主要内容，提取主体、场景、可检索文本线索和需要关注的信号，保持在 80 个汉字以内。"
                .to_string(),
        )
    });

    let client = match OpenAiCompatibleVisionClient::new(config) {
        Ok(client) => client,
        Err(error) => {
            return VlmSummaryExecution {
                available: false,
                status: "degraded".to_string(),
                summary: format!("Failed to build VLM client: {error}"),
                provider_key: endpoint.provider_key.clone(),
                model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
                text: String::new(),
                details: json!({
                    "image_path": image_path.to_string_lossy(),
                }),
            };
        }
    };

    match client.describe_frame(&VisionSummaryRequest {
        image_data_url,
        detection_summary: "No detector summary is attached for retrieval-side still images."
            .to_string(),
        user_prompt: prompt,
    }) {
        Ok(response) => VlmSummaryExecution {
            available: true,
            status: "active".to_string(),
            summary: "VLM summary extracted from image.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: response.summary,
            details: json!({
                "raw_response": response.raw_response,
            }),
        },
        Err(error) => VlmSummaryExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!("VLM request failed: {error}"),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "image_path": image_path.to_string_lossy(),
            }),
        },
    }
}

pub fn run_llm_text(prompt: &str) -> LlmTextExecution {
    let state = load_model_center_state();
    run_llm_text_with_state(prompt, &state)
}

pub fn run_llm_text_with_state(prompt: &str, state: &AdminModelCenterState) -> LlmTextExecution {
    let Some(endpoint) = resolve_endpoint(state, ModelKind::Llm, LLM_POLICY_ID) else {
        return LlmTextExecution {
            available: false,
            status: "disabled".to_string(),
            summary: "No LLM endpoint is enabled.".to_string(),
            provider_key: String::new(),
            model_endpoint_id: None,
            text: String::new(),
            details: json!({}),
        };
    };

    if let Some(mock_text) = metadata_string(&endpoint.metadata, "mock_text") {
        return LlmTextExecution {
            available: !mock_text.trim().is_empty(),
            status: "active".to_string(),
            summary: "Mock LLM endpoint resolved.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: mock_text,
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    if !endpoint
        .provider_key
        .eq_ignore_ascii_case("openai_compatible")
    {
        return LlmTextExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!(
                "LLM endpoint {} is configured, but provider {} is not implemented yet.",
                endpoint.model_endpoint_id, endpoint.provider_key
            ),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    let Some(config) = openai_compatible_config_from_endpoint(&endpoint) else {
        return LlmTextExecution {
            available: false,
            status: "degraded".to_string(),
            summary: "LLM endpoint base_url / api_key / model_name are not configured.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    };

    let system_prompt = metadata_string(&endpoint.metadata, "system_prompt").or_else(|| {
        Some(
            "You are a strict HarborBeacon planning translator. Return only valid JSON that follows the requested schema."
                .to_string(),
        )
    });

    let client = match OpenAiCompatibleTextClient::new(config) {
        Ok(client) => client,
        Err(error) => {
            return LlmTextExecution {
                available: false,
                status: "degraded".to_string(),
                summary: format!("Failed to build LLM client: {error}"),
                provider_key: endpoint.provider_key.clone(),
                model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
                text: String::new(),
                details: json!({}),
            };
        }
    };

    match client.complete_text(&TextCompletionRequest {
        system_prompt,
        user_prompt: prompt.to_string(),
        temperature: Some(0.1),
    }) {
        Ok(response) => LlmTextExecution {
            available: true,
            status: "active".to_string(),
            summary: "LLM text planning completed.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: response.text,
            details: json!({
                "raw_response": response.raw_response,
            }),
        },
        Err(error) => LlmTextExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!("LLM request failed: {error}"),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: String::new(),
            details: json!({}),
        },
    }
}

fn resolve_endpoint(
    state: &AdminModelCenterState,
    model_kind: ModelKind,
    route_policy_id: &str,
) -> Option<ModelEndpoint> {
    let policy = state
        .route_policies
        .iter()
        .find(|policy| policy.route_policy_id == route_policy_id);
    let fallback_order = policy
        .map(|policy| policy.fallback_order.clone())
        .unwrap_or_else(|| {
            vec![
                "local".to_string(),
                "sidecar".to_string(),
                "cloud".to_string(),
            ]
        });

    let mut candidates = state
        .endpoints
        .iter()
        .filter(|endpoint| {
            endpoint.model_kind == model_kind && endpoint.status != ModelEndpointStatus::Disabled
        })
        .cloned()
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        endpoint_priority(left, &fallback_order)
            .cmp(&endpoint_priority(right, &fallback_order))
            .then(status_priority(left.status).cmp(&status_priority(right.status)))
            .then(left.model_endpoint_id.cmp(&right.model_endpoint_id))
    });

    candidates.into_iter().next()
}

fn endpoint_priority(endpoint: &ModelEndpoint, fallback_order: &[String]) -> usize {
    fallback_order
        .iter()
        .position(|item| item.eq_ignore_ascii_case(endpoint.endpoint_kind.as_str()))
        .unwrap_or(fallback_order.len())
}

fn status_priority(status: ModelEndpointStatus) -> usize {
    match status {
        ModelEndpointStatus::Active => 0,
        ModelEndpointStatus::Degraded => 1,
        ModelEndpointStatus::Disabled => 2,
    }
}

fn test_tesseract_endpoint(endpoint: &ModelEndpoint) -> ModelEndpointTestResult {
    let Some(binary_path) = resolve_tesseract_binary(endpoint) else {
        return ModelEndpointTestResult {
            ok: false,
            status: "degraded".to_string(),
            summary: "Tesseract binary is not available.".to_string(),
            endpoint: redact_model_endpoint(endpoint),
            details: json!({
                "languages": resolve_tesseract_languages(endpoint),
            }),
        };
    };

    match Command::new(&binary_path).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version_line = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("tesseract")
                .trim()
                .to_string();
            ModelEndpointTestResult {
                ok: true,
                status: "active".to_string(),
                summary: "Tesseract endpoint is ready.".to_string(),
                endpoint: redact_model_endpoint(endpoint),
                details: json!({
                    "binary_path": binary_path.to_string_lossy(),
                    "version": version_line,
                    "languages": resolve_tesseract_languages(endpoint),
                }),
            }
        }
        Ok(output) => ModelEndpointTestResult {
            ok: false,
            status: "degraded".to_string(),
            summary: "Tesseract command returned a non-zero exit code.".to_string(),
            endpoint: redact_model_endpoint(endpoint),
            details: json!({
                "binary_path": binary_path.to_string_lossy(),
                "stderr": String::from_utf8_lossy(&output.stderr).trim(),
            }),
        },
        Err(error) => ModelEndpointTestResult {
            ok: false,
            status: "degraded".to_string(),
            summary: format!("Failed to launch tesseract: {error}"),
            endpoint: redact_model_endpoint(endpoint),
            details: json!({
                "binary_path": binary_path.to_string_lossy(),
            }),
        },
    }
}

fn test_http_endpoint(endpoint: &ModelEndpoint) -> ModelEndpointTestResult {
    let Some(base_url) = metadata_string(&endpoint.metadata, "base_url") else {
        return ModelEndpointTestResult {
            ok: false,
            status: "degraded".to_string(),
            summary: "Endpoint base_url is not configured.".to_string(),
            endpoint: redact_model_endpoint(endpoint),
            details: json!({}),
        };
    };

    let url = connectivity_url(&endpoint.provider_key, &base_url);
    let client = match Client::builder().timeout(Duration::from_secs(4)).build() {
        Ok(client) => client,
        Err(error) => {
            return ModelEndpointTestResult {
                ok: false,
                status: "degraded".to_string(),
                summary: format!("Failed to build HTTP client: {error}"),
                endpoint: redact_model_endpoint(endpoint),
                details: json!({
                    "base_url": base_url,
                }),
            }
        }
    };

    let mut request = client.get(url.as_str());
    if let Some(api_key) = metadata_string(&endpoint.metadata, "api_key") {
        if !api_key.trim().is_empty() {
            request = request.bearer_auth(api_key);
        }
    }

    match request.send() {
        Ok(response) => ModelEndpointTestResult {
            ok: response.status().is_success() || response.status().is_redirection(),
            status: if response.status().is_success() {
                "active".to_string()
            } else {
                "degraded".to_string()
            },
            summary: format!(
                "Endpoint responded with HTTP {}.",
                response.status().as_u16()
            ),
            endpoint: redact_model_endpoint(endpoint),
            details: json!({
                "base_url": base_url,
                "connectivity_url": url,
                "http_status": response.status().as_u16(),
            }),
        },
        Err(error) => ModelEndpointTestResult {
            ok: false,
            status: "degraded".to_string(),
            summary: format!("HTTP probe failed: {error}"),
            endpoint: redact_model_endpoint(endpoint),
            details: json!({
                "base_url": base_url,
                "connectivity_url": url,
            }),
        },
    }
}

fn connectivity_url(provider_key: &str, base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if provider_key.eq_ignore_ascii_case("ollama") {
        format!("{trimmed}/api/tags")
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/models")
    } else {
        trimmed.to_string()
    }
}

fn resolve_tesseract_binary(endpoint: &ModelEndpoint) -> Option<PathBuf> {
    metadata_string(&endpoint.metadata, "binary_path")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .filter(|path| path.exists())
        .or_else(|| {
            std::env::var(OCR_TESSERACT_PATH_ENV)
                .ok()
                .map(PathBuf::from)
                .filter(|path| path.exists())
        })
        .or_else(|| which::which("tesseract").ok())
}

fn resolve_tesseract_languages(endpoint: &ModelEndpoint) -> String {
    metadata_string(&endpoint.metadata, "languages")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var(OCR_TESSERACT_LANGS_ENV)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| DEFAULT_TESSERACT_LANGS.to_string())
}

fn openai_compatible_config_from_endpoint(
    endpoint: &ModelEndpoint,
) -> Option<OpenAiCompatibleConfig> {
    let base_url = metadata_string(&endpoint.metadata, "base_url")?;
    let api_key = metadata_string(&endpoint.metadata, "api_key")?;
    let model = metadata_string(&endpoint.metadata, "model").or_else(|| {
        (!endpoint.model_name.trim().is_empty()).then_some(endpoint.model_name.clone())
    })?;
    Some(OpenAiCompatibleConfig {
        base_url: base_url.trim_end_matches('/').to_string(),
        api_key,
        model,
    })
}

fn build_image_data_url(image_path: &Path) -> Result<String, String> {
    let bytes = fs::read(image_path)
        .map_err(|error| format!("Failed to read image {}: {error}", image_path.display()))?;
    let mime = image_mime_type(image_path);
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

fn image_mime_type(image_path: &Path) -> &'static str {
    match image_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn metadata_string(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn redact_secret_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let mut configured_flags = Vec::new();
            for (key, nested) in map.iter_mut() {
                if is_secret_key(key.as_str()) {
                    let configured = secret_present(nested);
                    *nested = Value::String(String::new());
                    configured_flags.push((format!("{key}_configured"), Value::Bool(configured)));
                    continue;
                }
                redact_secret_value(nested);
            }
            for (key, value) in configured_flags {
                map.entry(key).or_insert(value);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_secret_value(item);
            }
        }
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    matches!(
        key,
        "api_key" | "token" | "secret" | "password" | "authorization" | "bearer_token"
    )
}

fn secret_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::{redact_model_endpoint, run_vlm_summary_with_state, test_model_endpoint};
    use crate::control_plane::models::{
        ModelEndpoint, ModelEndpointKind, ModelEndpointStatus, ModelKind, ModelRoutePolicy,
        PrivacyLevel,
    };
    use crate::runtime::admin_console::AdminModelCenterState;

    #[test]
    fn redact_model_endpoint_masks_api_keys() {
        let endpoint = ModelEndpoint {
            model_endpoint_id: "cloud-llm".to_string(),
            workspace_id: Some("home-1".to_string()),
            provider_account_id: None,
            model_kind: ModelKind::Llm,
            endpoint_kind: ModelEndpointKind::Cloud,
            provider_key: "custom".to_string(),
            model_name: "gpt-like".to_string(),
            capability_tags: vec!["chat".to_string()],
            cost_policy: json!({}),
            status: ModelEndpointStatus::Active,
            metadata: json!({
                "base_url": "https://api.example.com/v1",
                "api_key": "secret_value",
            }),
        };

        let redacted = redact_model_endpoint(&endpoint);

        assert_eq!(redacted.metadata["api_key"], json!(""));
        assert_eq!(redacted.metadata["api_key_configured"], json!(true));
        assert_eq!(
            redacted.metadata["base_url"],
            json!("https://api.example.com/v1")
        );
    }

    #[test]
    fn test_model_endpoint_supports_mock_mode() {
        let endpoint = ModelEndpoint {
            model_endpoint_id: "ocr-mock".to_string(),
            workspace_id: Some("home-1".to_string()),
            provider_account_id: None,
            model_kind: ModelKind::Ocr,
            endpoint_kind: ModelEndpointKind::Local,
            provider_key: "tesseract".to_string(),
            model_name: "mock".to_string(),
            capability_tags: vec!["ocr".to_string()],
            cost_policy: json!({}),
            status: ModelEndpointStatus::Active,
            metadata: json!({
                "mock_text": "front gate camera",
            }),
        };

        let result = test_model_endpoint(&endpoint);

        assert!(result.ok);
        assert_eq!(result.status, "active");
        assert_eq!(result.details["mock_text_length"], json!(17));
    }

    #[test]
    fn run_vlm_summary_supports_mock_mode() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("harborbeacon-vlm-mock-{unique}"));
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let image_path = temp_dir.join("frame.jpg");
        fs::write(&image_path, b"fake-image").expect("write image");

        let state = AdminModelCenterState {
            endpoints: vec![ModelEndpoint {
                model_endpoint_id: "vlm-mock".to_string(),
                workspace_id: Some("home-1".to_string()),
                provider_account_id: None,
                model_kind: ModelKind::Vlm,
                endpoint_kind: ModelEndpointKind::Local,
                provider_key: "openai_compatible".to_string(),
                model_name: "vision".to_string(),
                capability_tags: vec!["multimodal".to_string()],
                cost_policy: json!({}),
                status: ModelEndpointStatus::Active,
                metadata: json!({
                    "mock_text": "画面里有一台放在门口的快递箱",
                }),
            }],
            route_policies: vec![ModelRoutePolicy {
                route_policy_id: "retrieval.vision_summary".to_string(),
                workspace_id: "home-1".to_string(),
                domain_scope: "retrieval".to_string(),
                modality: "multimodal".to_string(),
                privacy_level: PrivacyLevel::AllowRedactedCloud,
                local_preferred: true,
                max_cost_per_run: None,
                fallback_order: vec!["local".to_string(), "cloud".to_string()],
                status: "active".to_string(),
                metadata: json!({}),
            }],
        };

        let result = run_vlm_summary_with_state(&image_path, &state);
        assert!(result.available);
        assert_eq!(result.status, "active");
        assert_eq!(result.text, "画面里有一台放在门口的快递箱");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
