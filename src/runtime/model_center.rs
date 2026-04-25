//! Model-center helpers for admin redaction, endpoint tests, OCR routing, and
//! VLM summary execution.

use base64::Engine as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::connectors::ai_provider::{
    EmbeddingRequest, OpenAiCompatibleConfig, OpenAiCompatibleEmbeddingClient,
    OpenAiCompatibleTextClient, OpenAiCompatibleVisionClient, TextCompletionRequest,
    VisionSummaryRequest,
};
use crate::control_plane::models::{ModelEndpoint, ModelEndpointStatus, ModelKind};
use crate::runtime::admin_console::{
    default_model_endpoints, sanitize_model_center_state, AdminConsoleState,
    AdminModelCenterState,
};

pub const ADMIN_STATE_PATH_ENV: &str = "HARBOR_ADMIN_STATE_PATH";
pub const OCR_TESSERACT_PATH_ENV: &str = "HARBOR_OCR_TESSERACT_PATH";
pub const OCR_TESSERACT_LANGS_ENV: &str = "HARBOR_OCR_LANGS";
const OCR_POLICY_ID: &str = "retrieval.ocr";
const EMBED_POLICY_ID: &str = "retrieval.embed";
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LlmTextOptions {
    pub purpose: Option<String>,
    pub system_prompt: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EmbeddingExecution {
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
    pub model_name: Option<String>,
    #[serde(default)]
    pub vector: Vec<f32>,
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

pub fn run_embedding(text: &str) -> EmbeddingExecution {
    let state = load_model_center_state();
    run_embedding_with_state(text, &state)
}

pub fn run_llm_text_with_state(prompt: &str, state: &AdminModelCenterState) -> LlmTextExecution {
    run_llm_text_with_state_and_options(prompt, state, &LlmTextOptions::default())
}

pub fn run_llm_text_with_state_and_options(
    prompt: &str,
    state: &AdminModelCenterState,
    options: &LlmTextOptions,
) -> LlmTextExecution {
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

    let system_prompt = options.system_prompt.clone().or_else(|| {
        metadata_string(&endpoint.metadata, "system_prompt").or_else(|| {
            Some(
                "You are a strict HarborBeacon planning translator. Return only valid JSON that follows the requested schema."
                    .to_string(),
            )
        })
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
        temperature: options.temperature.or(Some(0.1)),
        max_tokens: options.max_tokens,
        timeout: options.timeout,
    }) {
        Ok(response) => LlmTextExecution {
            available: true,
            status: "active".to_string(),
            summary: format!(
                "LLM {} completed.",
                options
                    .purpose
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("text completion")
            ),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            text: response.text,
            details: json!({
                "purpose": options.purpose.clone(),
                "max_tokens": options.max_tokens,
                "timeout_ms": options.timeout.map(|value| value.as_millis() as u64),
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

pub fn run_embedding_with_state(text: &str, state: &AdminModelCenterState) -> EmbeddingExecution {
    let input = text.trim();
    if input.is_empty() {
        return EmbeddingExecution {
            available: false,
            status: "disabled".to_string(),
            summary: "Embedding input is empty.".to_string(),
            provider_key: String::new(),
            model_endpoint_id: None,
            model_name: None,
            vector: Vec::new(),
            details: json!({}),
        };
    }

    let Some(endpoint) = resolve_endpoint(state, ModelKind::Embedder, EMBED_POLICY_ID) else {
        return EmbeddingExecution {
            available: false,
            status: "disabled".to_string(),
            summary: "No embedding endpoint is enabled.".to_string(),
            provider_key: String::new(),
            model_endpoint_id: None,
            model_name: None,
            vector: Vec::new(),
            details: json!({}),
        };
    };

    if let Some(vector) = mock_embedding_vector_from_endpoint(&endpoint, input) {
        return EmbeddingExecution {
            available: !vector.is_empty(),
            status: "active".to_string(),
            summary: "Mock embedding endpoint resolved.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            model_name: Some(endpoint.model_name.clone()),
            vector,
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    if !endpoint
        .provider_key
        .eq_ignore_ascii_case("openai_compatible")
    {
        return EmbeddingExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!(
                "Embedding endpoint {} is configured, but provider {} is not implemented yet.",
                endpoint.model_endpoint_id, endpoint.provider_key
            ),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            model_name: Some(endpoint.model_name.clone()),
            vector: Vec::new(),
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    }

    let Some(config) = openai_compatible_config_from_endpoint(&endpoint) else {
        return EmbeddingExecution {
            available: false,
            status: "degraded".to_string(),
            summary: "Embedding endpoint base_url / api_key / model_name are not configured.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            model_name: Some(endpoint.model_name.clone()),
            vector: Vec::new(),
            details: json!({
                "endpoint_kind": endpoint.endpoint_kind.as_str(),
            }),
        };
    };

    let client = match OpenAiCompatibleEmbeddingClient::new(config) {
        Ok(client) => client,
        Err(error) => {
            return EmbeddingExecution {
                available: false,
                status: "degraded".to_string(),
                summary: format!("Failed to build embedding client: {error}"),
                provider_key: endpoint.provider_key.clone(),
                model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
                model_name: Some(endpoint.model_name.clone()),
                vector: Vec::new(),
                details: json!({}),
            };
        }
    };

    match client.embed_text(&EmbeddingRequest {
        input: input.to_string(),
    }) {
        Ok(response) => EmbeddingExecution {
            available: !response.embedding.is_empty(),
            status: "active".to_string(),
            summary: "Embedding request completed.".to_string(),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            model_name: Some(endpoint.model_name.clone()),
            vector: response.embedding,
            details: json!({
                "raw_response": response.raw_response,
            }),
        },
        Err(error) => EmbeddingExecution {
            available: false,
            status: "degraded".to_string(),
            summary: format!("Embedding request failed: {error}"),
            provider_key: endpoint.provider_key.clone(),
            model_endpoint_id: Some(endpoint.model_endpoint_id.clone()),
            model_name: Some(endpoint.model_name.clone()),
            vector: Vec::new(),
            details: json!({}),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct LocalRuntimeProjection {
    base_url: String,
    healthz_url: String,
    api_key: String,
    api_key_configured: bool,
    ready: bool,
    backend_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct LocalRuntimeProbeTarget {
    cache_key: String,
    base_url: String,
    healthz_url: String,
    api_key: String,
    api_key_configured: bool,
}

#[derive(Debug, Clone)]
struct CachedLocalRuntimeProjection {
    target_cache_key: String,
    expires_at: Instant,
    projection: LocalRuntimeProjection,
}

const LOCAL_RUNTIME_PROJECTION_CACHE_TTL: Duration = Duration::from_secs(2);

fn local_runtime_projection_cache() -> &'static Mutex<Option<CachedLocalRuntimeProjection>> {
    static CACHE: OnceLock<Mutex<Option<CachedLocalRuntimeProjection>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn clear_local_runtime_projection_cache() {
    *local_runtime_projection_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

fn runtime_wired_model_center_state(state: &AdminModelCenterState) -> AdminModelCenterState {
    let runtime = probe_local_runtime(&state.endpoints);
    AdminModelCenterState {
        endpoints: overlay_endpoints_with_runtime_truth(&state.endpoints, &runtime),
        route_policies: state.route_policies.clone(),
    }
}

fn resolve_endpoint(
    state: &AdminModelCenterState,
    model_kind: ModelKind,
    route_policy_id: &str,
) -> Option<ModelEndpoint> {
    let state = runtime_wired_model_center_state(state);
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

fn probe_local_runtime(endpoints: &[ModelEndpoint]) -> LocalRuntimeProjection {
    let Some(target) = resolve_local_runtime_probe_target(endpoints) else {
        return LocalRuntimeProjection::default();
    };

    let now = Instant::now();
    if let Some(cached) = local_runtime_projection_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .filter(|cached| cached.target_cache_key == target.cache_key && cached.expires_at > now)
        .cloned()
    {
        return cached.projection;
    }

    let projection = probe_local_runtime_target(&target);
    *local_runtime_projection_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some(CachedLocalRuntimeProjection {
            target_cache_key: target.cache_key,
            expires_at: now + LOCAL_RUNTIME_PROJECTION_CACHE_TTL,
            projection: projection.clone(),
        });
    projection
}

fn resolve_local_runtime_probe_target(
    endpoints: &[ModelEndpoint],
) -> Option<LocalRuntimeProbeTarget> {
    let builtin_defaults = default_model_endpoints();
    let preferred = endpoints
        .iter()
        .find(|endpoint| is_builtin_local_openai_endpoint(endpoint))
        .cloned()
        .or_else(|| {
            builtin_defaults
                .iter()
                .find(|endpoint| is_builtin_local_openai_endpoint(endpoint))
                .cloned()
        });
    let template = preferred?;
    let fallback = builtin_defaults
        .iter()
        .find(|endpoint| endpoint.model_endpoint_id == template.model_endpoint_id)
        .or_else(|| {
            builtin_defaults
                .iter()
                .find(|endpoint| is_builtin_local_openai_endpoint(endpoint))
        });

    let base_url = metadata_string(&template.metadata, "base_url")
        .or_else(|| fallback.and_then(|endpoint| metadata_string(&endpoint.metadata, "base_url")))
        .unwrap_or_default();
    let healthz_url = metadata_string(&template.metadata, "healthz_url")
        .or_else(|| {
            fallback.and_then(|endpoint| metadata_string(&endpoint.metadata, "healthz_url"))
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| infer_healthz_url(&base_url));
    let api_key = metadata_string(&template.metadata, "api_key")
        .or_else(|| fallback.and_then(|endpoint| metadata_string(&endpoint.metadata, "api_key")))
        .unwrap_or_default();
    let api_key_configured = metadata_bool(&template.metadata, "api_key_configured")
        || !api_key.trim().is_empty()
        || fallback
            .map(|endpoint| metadata_bool(&endpoint.metadata, "api_key_configured"))
            .unwrap_or(false);

    Some(LocalRuntimeProbeTarget {
        cache_key: format!(
            "{}|{}|{}|{}",
            template.model_endpoint_id,
            base_url,
            healthz_url,
            api_key,
        ),
        base_url,
        healthz_url,
        api_key,
        api_key_configured,
    })
}

fn probe_local_runtime_target(target: &LocalRuntimeProbeTarget) -> LocalRuntimeProjection {
    if target.healthz_url.trim().is_empty() {
        return LocalRuntimeProjection {
            base_url: target.base_url.clone(),
            healthz_url: target.healthz_url.clone(),
            api_key: target.api_key.clone(),
            api_key_configured: target.api_key_configured,
            ready: false,
            backend_ready: false,
        };
    }

    let client = match Client::builder().timeout(Duration::from_secs(3)).build() {
        Ok(client) => client,
        Err(_) => {
            return LocalRuntimeProjection {
                base_url: target.base_url.clone(),
                healthz_url: target.healthz_url.clone(),
                api_key: target.api_key.clone(),
                api_key_configured: target.api_key_configured,
                ready: false,
                backend_ready: false,
            }
        }
    };

    let response = match client.get(&target.healthz_url).send() {
        Ok(response) => response,
        Err(_) => {
            return LocalRuntimeProjection {
                base_url: target.base_url.clone(),
                healthz_url: target.healthz_url.clone(),
                api_key: target.api_key.clone(),
                api_key_configured: target.api_key_configured,
                ready: false,
                backend_ready: false,
            }
        }
    };
    let body = match response.text() {
        Ok(body) => body,
        Err(_) => {
            return LocalRuntimeProjection {
                base_url: target.base_url.clone(),
                healthz_url: target.healthz_url.clone(),
                api_key: target.api_key.clone(),
                api_key_configured: target.api_key_configured,
                ready: false,
                backend_ready: false,
            }
        }
    };
    let payload = match serde_json::from_str::<Value>(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return LocalRuntimeProjection {
                base_url: target.base_url.clone(),
                healthz_url: target.healthz_url.clone(),
                api_key: target.api_key.clone(),
                api_key_configured: target.api_key_configured,
                ready: false,
                backend_ready: false,
            }
        }
    };

    LocalRuntimeProjection {
        base_url: target.base_url.clone(),
        healthz_url: target.healthz_url.clone(),
        api_key: target.api_key.clone(),
        api_key_configured: target.api_key_configured,
        ready: payload.get("ready").and_then(Value::as_bool).unwrap_or(false),
        backend_ready: payload
            .get("backend")
            .and_then(|value| value.get("ready"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn overlay_endpoints_with_runtime_truth(
    endpoints: &[ModelEndpoint],
    runtime: &LocalRuntimeProjection,
) -> Vec<ModelEndpoint> {
    let builtin_defaults = default_model_endpoints()
        .into_iter()
        .map(|endpoint| (endpoint.model_endpoint_id.clone(), endpoint))
        .collect::<std::collections::HashMap<_, _>>();

    endpoints
        .iter()
        .map(|endpoint| {
            let mut overlayed = endpoint.clone();
            if let Some(default_endpoint) = builtin_defaults.get(&overlayed.model_endpoint_id) {
                if is_builtin_local_openai_endpoint(default_endpoint) {
                    if metadata_missing_or_empty(&overlayed.metadata, "base_url") {
                        set_metadata_string(
                            &mut overlayed.metadata,
                            "base_url",
                            runtime
                                .base_url
                                .clone()
                                .if_empty_then(|| metadata_string(&default_endpoint.metadata, "base_url"))
                                .unwrap_or_default(),
                        );
                    }
                    if metadata_missing_or_empty(&overlayed.metadata, "healthz_url") {
                        set_metadata_string(
                            &mut overlayed.metadata,
                            "healthz_url",
                            runtime
                                .healthz_url
                                .clone()
                                .if_empty_then(|| {
                                    metadata_string(&default_endpoint.metadata, "healthz_url")
                                })
                                .unwrap_or_else(|| infer_healthz_url(&runtime.base_url)),
                        );
                    }
                    if metadata_missing_or_empty(&overlayed.metadata, "api_key") {
                        set_metadata_string(
                            &mut overlayed.metadata,
                            "api_key",
                            runtime
                                .api_key
                                .clone()
                                .if_empty_then(|| metadata_string(&default_endpoint.metadata, "api_key"))
                                .unwrap_or_default(),
                        );
                    }
                    if !metadata_bool(&overlayed.metadata, "api_key_configured")
                        && runtime.api_key_configured
                    {
                        set_metadata_bool(&mut overlayed.metadata, "api_key_configured", true);
                    }
                    if matches!(overlayed.model_kind, ModelKind::Llm | ModelKind::Embedder)
                        && runtime.ready
                        && runtime.backend_ready
                    {
                        overlayed.status = ModelEndpointStatus::Active;
                    }
                }
            }
            overlayed
        })
        .collect()
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

    let url = connectivity_url(endpoint, &base_url);
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

fn connectivity_url(endpoint: &ModelEndpoint, base_url: &str) -> String {
    if let Some(healthz_url) = metadata_string(&endpoint.metadata, "healthz_url") {
        return healthz_url;
    }
    let trimmed = base_url.trim().trim_end_matches('/');
    if endpoint.provider_key.eq_ignore_ascii_case("ollama") {
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

fn metadata_bool(metadata: &Value, key: &str) -> bool {
    metadata.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn metadata_missing_or_empty(metadata: &Value, key: &str) -> bool {
    metadata_string(metadata, key).is_none()
}

fn set_metadata_string(metadata: &mut Value, key: &str, value: String) {
    if value.trim().is_empty() {
        return;
    }
    if !metadata.is_object() {
        *metadata = json!({});
    }
    if let Some(map) = metadata.as_object_mut() {
        map.insert(key.to_string(), Value::String(value));
    }
}

fn set_metadata_bool(metadata: &mut Value, key: &str, value: bool) {
    if !metadata.is_object() {
        *metadata = json!({});
    }
    if let Some(map) = metadata.as_object_mut() {
        map.insert(key.to_string(), Value::Bool(value));
    }
}

fn infer_healthz_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(prefix) = trimmed.strip_suffix("/v1") {
        format!("{prefix}/healthz")
    } else {
        format!("{trimmed}/healthz")
    }
}

fn is_builtin_local_openai_endpoint(endpoint: &ModelEndpoint) -> bool {
    matches!(
        endpoint.model_kind,
        ModelKind::Llm | ModelKind::Embedder | ModelKind::Vlm
    ) && endpoint.endpoint_kind == crate::control_plane::models::ModelEndpointKind::Local
        && endpoint.provider_key.eq_ignore_ascii_case("openai_compatible")
        && metadata_bool(&endpoint.metadata, "builtin")
}

trait EmptyStringFallback {
    fn if_empty_then<F>(self, fallback: F) -> Option<String>
    where
        F: FnOnce() -> Option<String>;
}

impl EmptyStringFallback for String {
    fn if_empty_then<F>(self, fallback: F) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        if self.trim().is_empty() {
            fallback()
        } else {
            Some(self)
        }
    }
}

fn mock_embedding_vector_from_endpoint(endpoint: &ModelEndpoint, input: &str) -> Option<Vec<f32>> {
    if let Some(vector) = endpoint
        .metadata
        .get("mock_embeddings")
        .and_then(Value::as_object)
        .and_then(|map| map.get(input))
        .and_then(parse_embedding_vector)
    {
        return Some(vector);
    }

    let dimensions = endpoint
        .metadata
        .get("mock_embedding_dimensions")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .filter(|value| *value > 0)
        .or_else(|| {
            endpoint
                .metadata
                .get("mock_embedding")
                .and_then(Value::as_bool)
                .filter(|value| *value)
                .map(|_| 8usize)
        })?;

    Some(build_mock_embedding(input, dimensions))
}

fn parse_embedding_vector(value: &Value) -> Option<Vec<f32>> {
    let items = value.as_array()?;
    let mut vector = Vec::with_capacity(items.len());
    for item in items {
        vector.push(item.as_f64()? as f32);
    }
    (!vector.is_empty()).then_some(vector)
}

fn build_mock_embedding(input: &str, dimensions: usize) -> Vec<f32> {
    let mut vector = vec![0.0f32; dimensions.max(1)];
    for (index, ch) in input.chars().enumerate() {
        let slot = index % vector.len();
        let weight = ((ch as u32 % 17) + 1) as f32;
        vector[slot] += weight;
    }

    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
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
    use std::sync::Mutex;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use tiny_http::{Header, Method, Response, Server};

    use super::{
        clear_local_runtime_projection_cache, connectivity_url, redact_model_endpoint,
        run_embedding_with_state, run_llm_text_with_state, run_llm_text_with_state_and_options,
        run_vlm_summary_with_state, test_model_endpoint, LlmTextOptions,
    };
    use crate::control_plane::models::{
        ModelEndpoint, ModelEndpointKind, ModelEndpointStatus, ModelKind, ModelRoutePolicy,
        PrivacyLevel,
    };
    use crate::runtime::admin_console::AdminModelCenterState;

    static MODEL_RUNTIME_ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn run_embedding_supports_mock_dimensions_and_overrides() {
        let state = AdminModelCenterState {
            endpoints: vec![ModelEndpoint {
                model_endpoint_id: "embed-mock".to_string(),
                workspace_id: Some("home-1".to_string()),
                provider_account_id: None,
                model_kind: ModelKind::Embedder,
                endpoint_kind: ModelEndpointKind::Local,
                provider_key: "openai_compatible".to_string(),
                model_name: "mock-embed".to_string(),
                capability_tags: vec!["embeddings".to_string()],
                cost_policy: json!({}),
                status: ModelEndpointStatus::Active,
                metadata: json!({
                    "mock_embedding_dimensions": 4,
                    "mock_embeddings": {
                        "樱花整理": [1.0, 0.0, 0.0, 0.0]
                    }
                }),
            }],
            route_policies: vec![ModelRoutePolicy {
                route_policy_id: "retrieval.embed".to_string(),
                workspace_id: "home-1".to_string(),
                domain_scope: "retrieval".to_string(),
                modality: "text".to_string(),
                privacy_level: PrivacyLevel::StrictLocal,
                local_preferred: true,
                max_cost_per_run: None,
                fallback_order: vec!["local".to_string(), "cloud".to_string()],
                status: "active".to_string(),
                metadata: json!({}),
            }],
        };

        let exact = run_embedding_with_state("樱花整理", &state);
        assert!(exact.available);
        assert_eq!(exact.vector, vec![1.0, 0.0, 0.0, 0.0]);

        let generated = run_embedding_with_state("整理计划", &state);
        assert!(generated.available);
        assert_eq!(generated.vector.len(), 4);
    }

    #[test]
    fn connectivity_url_prefers_explicit_healthz_metadata() {
        let endpoint = ModelEndpoint {
            model_endpoint_id: "llm-local".to_string(),
            workspace_id: Some("home-1".to_string()),
            provider_account_id: None,
            model_kind: ModelKind::Llm,
            endpoint_kind: ModelEndpointKind::Local,
            provider_key: "openai_compatible".to_string(),
            model_name: "chat".to_string(),
            capability_tags: vec!["chat".to_string()],
            cost_policy: json!({}),
            status: ModelEndpointStatus::Degraded,
            metadata: json!({
                "base_url": "http://127.0.0.1:4176/v1",
                "healthz_url": "http://127.0.0.1:4176/healthz",
            }),
        };

        assert_eq!(
            connectivity_url(&endpoint, "http://127.0.0.1:4176/v1"),
            "http://127.0.0.1:4176/healthz"
        );
    }

    #[test]
    fn run_llm_text_with_state_uses_runtime_overlay_for_stale_builtin_local_endpoint() {
        let _guard = MODEL_RUNTIME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_local_runtime_projection_cache();
        let server = Server::http("127.0.0.1:0").expect("server");
        let base_url = format!("http://{}/v1", server.server_addr());
        let healthz_header =
            Header::from_bytes(b"Content-Type", b"application/json").expect("health header");
        let chat_header =
            Header::from_bytes(b"Content-Type", b"application/json").expect("chat header");

        let server_thread = thread::spawn(move || {
            for _ in 0..2 {
                let request = server.recv().expect("request");
                match (request.method(), request.url()) {
                    (&Method::Get, "/healthz") => request
                        .respond(
                            Response::from_string(
                                r#"{"ready":true,"backend":{"ready":true,"kind":"candle"}}"#,
                            )
                            .with_header(healthz_header.clone()),
                        )
                        .expect("health response"),
                    (&Method::Post, "/v1/chat/completions") => request
                        .respond(
                            Response::from_string(
                                r#"{"choices":[{"message":{"content":"{\"decision\":\"capability_summary\",\"reply_text\":\"我可以帮你抓拍最新画面。\"}"}}]}"#,
                            )
                            .with_header(chat_header.clone()),
                        )
                        .expect("chat response"),
                    _ => request
                        .respond(Response::from_string("not found").with_status_code(404))
                        .expect("404 response"),
                }
            }
        });

        std::env::set_var("HARBOR_MODEL_API_BASE_URL", &base_url);
        std::env::set_var("HARBOR_MODEL_API_TOKEN", "runtime-overlay-token");

        let state = AdminModelCenterState {
            endpoints: vec![ModelEndpoint {
                model_endpoint_id: "llm-local-openai-compatible".to_string(),
                workspace_id: Some("home-1".to_string()),
                provider_account_id: None,
                model_kind: ModelKind::Llm,
                endpoint_kind: ModelEndpointKind::Local,
                provider_key: "openai_compatible".to_string(),
                model_name: "harbor-local-chat".to_string(),
                capability_tags: vec!["chat".to_string(), "local_first".to_string()],
                cost_policy: json!({}),
                status: ModelEndpointStatus::Disabled,
                metadata: json!({
                    "builtin": true,
                    "base_url": "",
                    "healthz_url": "",
                    "api_key": "",
                    "api_key_configured": false,
                }),
            }],
            route_policies: vec![ModelRoutePolicy {
                route_policy_id: "retrieval.answer".to_string(),
                workspace_id: "home-1".to_string(),
                domain_scope: "retrieval".to_string(),
                modality: "text".to_string(),
                privacy_level: PrivacyLevel::AllowRedactedCloud,
                local_preferred: true,
                max_cost_per_run: None,
                fallback_order: vec!["local".to_string(), "cloud".to_string()],
                status: "active".to_string(),
                metadata: json!({}),
            }],
        };

        let result = run_llm_text_with_state("摄像头能干什么", &state);

        std::env::remove_var("HARBOR_MODEL_API_BASE_URL");
        std::env::remove_var("HARBOR_MODEL_API_TOKEN");
        clear_local_runtime_projection_cache();
        server_thread.join().expect("server thread");

        assert!(result.available);
        assert_eq!(result.status, "active");
        assert!(result.text.contains("\"decision\":\"capability_summary\""));
        assert!(result.text.contains("我可以帮你抓拍最新画面。"));
    }

    #[test]
    fn run_llm_text_with_state_and_options_forwards_max_tokens() {
        let _guard = MODEL_RUNTIME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = Server::http("127.0.0.1:0").expect("server");
        let base_url = format!("http://{}/v1", server.server_addr());
        let healthz_url = format!("http://{}/healthz", server.server_addr());
        let chat_header =
            Header::from_bytes(b"Content-Type", b"application/json").expect("chat header");

        let server_thread = thread::spawn(move || {
            let mut request = server.recv().expect("request");
            assert_eq!(request.method(), &Method::Post);
            assert_eq!(request.url(), "/v1/chat/completions");
            let mut body = String::new();
            request
                .as_reader()
                .read_to_string(&mut body)
                .expect("read body");
            let payload: serde_json::Value =
                serde_json::from_str(&body).expect("payload json");
            assert_eq!(payload["max_tokens"], json!(12), "{body}");
            request
                .respond(
                    Response::from_string(
                        r#"{"choices":[{"message":{"content":"capability_summary"}}]}"#,
                    )
                    .with_header(chat_header.clone()),
                )
                .expect("chat response");
        });

        let state = AdminModelCenterState {
            endpoints: vec![ModelEndpoint {
                model_endpoint_id: "llm-local-openai-compatible".to_string(),
                workspace_id: Some("home-1".to_string()),
                provider_account_id: None,
                model_kind: ModelKind::Llm,
                endpoint_kind: ModelEndpointKind::Local,
                provider_key: "openai_compatible".to_string(),
                model_name: "harbor-local-chat".to_string(),
                capability_tags: vec!["chat".to_string(), "local_first".to_string()],
                cost_policy: json!({}),
                status: ModelEndpointStatus::Active,
                metadata: json!({
                    "builtin": false,
                    "base_url": base_url,
                    "healthz_url": healthz_url,
                    "api_key": "runtime-overlay-token",
                    "api_key_configured": true,
                }),
            }],
            route_policies: vec![ModelRoutePolicy {
                route_policy_id: "retrieval.answer".to_string(),
                workspace_id: "home-1".to_string(),
                domain_scope: "retrieval".to_string(),
                modality: "text".to_string(),
                privacy_level: PrivacyLevel::AllowRedactedCloud,
                local_preferred: true,
                max_cost_per_run: None,
                fallback_order: vec!["local".to_string(), "cloud".to_string()],
                status: "active".to_string(),
                metadata: json!({}),
            }],
        };

        let result = run_llm_text_with_state_and_options(
            "摄像头能干什么",
            &state,
            &LlmTextOptions {
                purpose: Some("router".to_string()),
                max_tokens: Some(12),
                ..Default::default()
            },
        );

        clear_local_runtime_projection_cache();
        server_thread.join().expect("server thread");

        assert!(result.available);
        assert_eq!(result.text, "capability_summary");
        assert_eq!(result.details["max_tokens"], json!(12));
    }

    #[test]
    fn run_llm_text_with_state_reuses_runtime_probe_within_ttl() {
        let _guard = MODEL_RUNTIME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_local_runtime_projection_cache();
        let server = Server::http("127.0.0.1:0").expect("server");
        let base_url = format!("http://{}/v1", server.server_addr());
        let healthz_header =
            Header::from_bytes(b"Content-Type", b"application/json").expect("health header");
        let chat_header =
            Header::from_bytes(b"Content-Type", b"application/json").expect("chat header");

        let server_thread = thread::spawn(move || {
            for _ in 0..3 {
                let request = server.recv().expect("request");
                match (request.method(), request.url()) {
                    (&Method::Get, "/healthz") => request
                        .respond(
                            Response::from_string(
                                r#"{"ready":true,"backend":{"ready":true,"kind":"candle"}}"#,
                            )
                            .with_header(healthz_header.clone()),
                        )
                        .expect("health response"),
                    (&Method::Post, "/v1/chat/completions") => request
                        .respond(
                            Response::from_string(
                                r#"{"choices":[{"message":{"content":"capability_summary"}}]}"#,
                            )
                            .with_header(chat_header.clone()),
                        )
                        .expect("chat response"),
                    _ => request
                        .respond(Response::from_string("not found").with_status_code(404))
                        .expect("404 response"),
                }
            }
        });

        std::env::set_var("HARBOR_MODEL_API_BASE_URL", &base_url);
        std::env::set_var("HARBOR_MODEL_API_TOKEN", "runtime-overlay-token");
        let state = AdminModelCenterState {
            endpoints: vec![ModelEndpoint {
                model_endpoint_id: "llm-local-openai-compatible".to_string(),
                workspace_id: Some("home-1".to_string()),
                provider_account_id: None,
                model_kind: ModelKind::Llm,
                endpoint_kind: ModelEndpointKind::Local,
                provider_key: "openai_compatible".to_string(),
                model_name: "harbor-local-chat".to_string(),
                capability_tags: vec!["chat".to_string(), "local_first".to_string()],
                cost_policy: json!({}),
                status: ModelEndpointStatus::Disabled,
                metadata: json!({
                    "builtin": true,
                    "base_url": "",
                    "healthz_url": "",
                    "api_key": "",
                    "api_key_configured": false,
                }),
            }],
            route_policies: vec![ModelRoutePolicy {
                route_policy_id: "retrieval.answer".to_string(),
                workspace_id: "home-1".to_string(),
                domain_scope: "retrieval".to_string(),
                modality: "text".to_string(),
                privacy_level: PrivacyLevel::AllowRedactedCloud,
                local_preferred: true,
                max_cost_per_run: None,
                fallback_order: vec!["local".to_string(), "cloud".to_string()],
                status: "active".to_string(),
                metadata: json!({}),
            }],
        };

        let first = run_llm_text_with_state("摄像头能干什么", &state);
        let second = run_llm_text_with_state("再说一遍", &state);

        std::env::remove_var("HARBOR_MODEL_API_BASE_URL");
        std::env::remove_var("HARBOR_MODEL_API_TOKEN");
        clear_local_runtime_projection_cache();
        server_thread.join().expect("server thread");

        assert!(first.available);
        assert!(second.available);
    }
}
