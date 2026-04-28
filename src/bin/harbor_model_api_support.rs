use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result as AnyResult};
use candle::{DType, Device, Module, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::jina_bert::{
    BertModel as JinaBertModel, Config as JinaBertConfig,
};
use candle_transformers::models::qwen3::{Config as QwenConfig, ModelForCausalLM as QwenModel};
use hf_hub::{api::sync::ApiBuilder, Repo, RepoType};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, StatusCode};
use tokenizers::Tokenizer;

const DEFAULT_BIND: &str = "127.0.0.1:4176";
const DEFAULT_UPSTREAM_BASE_URL: &str = "http://127.0.0.1:11434/v1";
const DEFAULT_CHAT_MODEL: &str = "harbor-local-chat";
const DEFAULT_EMBEDDING_MODEL: &str = "harbor-local-embed";
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CANDLE_CHAT_MODEL_ID: &str = "Qwen/Qwen3-1.7B";
const DEFAULT_CANDLE_EMBEDDING_MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-zh";
const DEFAULT_CANDLE_CACHE_DIR: &str = "/mnt/software/harborbeacon-agent-ci/candle-cache";
const DEFAULT_CANDLE_MAX_NEW_TOKENS: usize = 64;
const DEFAULT_CANDLE_TEMPERATURE: f64 = 0.2;
const DEFAULT_CANDLE_REPEAT_PENALTY: f32 = 1.1;
const DEFAULT_CANDLE_REPEAT_LAST_N: usize = 64;
const DEFAULT_CANDLE_SEED: u64 = 299_792_458;
const SERVICE_NAME: &str = "harbor-model-api";
const HEALTH_OK: &str = "ok";
const HEALTH_DEGRADED: &str = "degraded";
const CANDLE_CANDIDATE_NOTE: &str =
    "candle candidate backend on side lane; keep openai_proxy as the default backend until benchmark gate clears promotion";
const CANDLE_SYSTEM_PROMPT: &str = "你是 HarborBeacon 的 Candle 实验后端。请用简洁中文直接回答；如果问题要求只回答一个词或“是/否”，请严格遵守。示例：问：请只回答“是”或“否”：摄像头能用于抓拍吗？答：是。问：请只回答一个词：“樱花”更像植物、工具还是地点？答：植物。问：请只回答一个词：在“录像”和“抓拍”里，哪一个更像持续动作？答：录像。";
const CANDLE_OUTPUT_POLICY_PROMPT: &str =
    "只输出最终答案，不要输出 <think>、推理、分析、解释、前言、步骤说明或额外空行；如果答案可以很短，就尽量用最短可用表述。";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendKind {
    Candle,
    OpenAIProxy,
}

impl BackendKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Candle => "candle",
            Self::OpenAIProxy => "openai_proxy",
        }
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BackendKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "candle" => Ok(Self::Candle),
            "openai_proxy" | "openai-proxy" | "proxy" => Ok(Self::OpenAIProxy),
            other => Err(format!(
                "unsupported backend '{other}'; expected candle or openai_proxy"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandleConfig {
    pub chat_model_id: String,
    pub embedding_model_id: String,
    pub cache_dir: String,
    pub max_new_tokens: usize,
    pub temperature: f64,
    pub repeat_penalty: f32,
    pub repeat_last_n: usize,
    pub seed: u64,
}

impl Default for CandleConfig {
    fn default() -> Self {
        Self {
            chat_model_id: DEFAULT_CANDLE_CHAT_MODEL_ID.to_string(),
            embedding_model_id: DEFAULT_CANDLE_EMBEDDING_MODEL_ID.to_string(),
            cache_dir: DEFAULT_CANDLE_CACHE_DIR.to_string(),
            max_new_tokens: DEFAULT_CANDLE_MAX_NEW_TOKENS,
            temperature: DEFAULT_CANDLE_TEMPERATURE,
            repeat_penalty: DEFAULT_CANDLE_REPEAT_PENALTY,
            repeat_last_n: DEFAULT_CANDLE_REPEAT_LAST_N,
            seed: DEFAULT_CANDLE_SEED,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelApiConfig {
    pub bind: String,
    pub backend: BackendKind,
    pub upstream_base_url: String,
    pub chat_model: String,
    pub embedding_model: String,
    pub request_timeout_ms: u64,
    pub candle: CandleConfig,
}

impl Default for ModelApiConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND.to_string(),
            backend: BackendKind::OpenAIProxy,
            upstream_base_url: DEFAULT_UPSTREAM_BASE_URL.to_string(),
            chat_model: DEFAULT_CHAT_MODEL.to_string(),
            embedding_model: DEFAULT_EMBEDDING_MODEL.to_string(),
            request_timeout_ms: DEFAULT_TIMEOUT_MS,
            candle: CandleConfig::default(),
        }
    }
}

impl ModelApiConfig {
    pub fn from_env_and_args() -> Self {
        let mut config = Self::default();
        let legacy_candle_model_id = env::var("HARBOR_MODEL_API_CANDLE_MODEL_ID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        config.bind = env_or_default("HARBOR_MODEL_API_BIND", &config.bind);
        config.backend = env_or_default("HARBOR_MODEL_API_BACKEND", config.backend.as_str())
            .parse::<BackendKind>()
            .unwrap_or_else(|error| fail(&error));
        config.upstream_base_url = env_or_default(
            "HARBOR_MODEL_API_UPSTREAM_BASE_URL",
            &config.upstream_base_url,
        );
        config.chat_model = env_or_default("HARBOR_MODEL_API_CHAT_MODEL", &config.chat_model);
        config.embedding_model =
            env_or_default("HARBOR_MODEL_API_EMBEDDING_MODEL", &config.embedding_model);
        config.request_timeout_ms = env_or_default(
            "HARBOR_MODEL_API_REQUEST_TIMEOUT_MS",
            &config.request_timeout_ms.to_string(),
        )
        .parse::<u64>()
        .unwrap_or_else(|error| fail(&format!("invalid request timeout: {error}")));
        config.candle.chat_model_id =
            env::var("HARBOR_MODEL_API_CANDLE_CHAT_MODEL_ID")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| legacy_candle_model_id.clone())
                .unwrap_or_else(|| config.candle.chat_model_id.clone());
        config.candle.embedding_model_id = env_or_default(
            "HARBOR_MODEL_API_CANDLE_EMBEDDING_MODEL_ID",
            &config.candle.embedding_model_id,
        );
        config.candle.cache_dir =
            env_or_default("HARBOR_MODEL_API_CANDLE_CACHE_DIR", &config.candle.cache_dir);
        config.candle.max_new_tokens = env_or_default(
            "HARBOR_MODEL_API_CANDLE_MAX_NEW_TOKENS",
            &config.candle.max_new_tokens.to_string(),
        )
        .parse::<usize>()
        .unwrap_or_else(|error| fail(&format!("invalid candle max tokens: {error}")));
        config.candle.temperature = env_or_default(
            "HARBOR_MODEL_API_CANDLE_TEMPERATURE",
            &config.candle.temperature.to_string(),
        )
        .parse::<f64>()
        .unwrap_or_else(|error| fail(&format!("invalid candle temperature: {error}")));
        config.apply_cli_args();
        config
    }

    fn apply_cli_args(&mut self) {
        let args = env::args().skip(1).collect::<Vec<_>>();
        if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
            print_usage();
            std::process::exit(0);
        }

        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            match arg.as_str() {
                "--bind" => self.bind = take_value(&args, &mut index, "--bind"),
                value if value.starts_with("--bind=") => {
                    self.bind = value["--bind=".len()..].to_string();
                }
                "--backend" => {
                    self.backend = take_value(&args, &mut index, "--backend")
                        .parse::<BackendKind>()
                        .unwrap_or_else(|error| fail(&error));
                }
                value if value.starts_with("--backend=") => {
                    self.backend = value["--backend=".len()..]
                        .parse::<BackendKind>()
                        .unwrap_or_else(|error| fail(&error));
                }
                "--upstream-base-url" => {
                    self.upstream_base_url =
                        take_value(&args, &mut index, "--upstream-base-url")
                }
                value if value.starts_with("--upstream-base-url=") => {
                    self.upstream_base_url =
                        value["--upstream-base-url=".len()..].to_string();
                }
                "--chat-model" => {
                    self.chat_model = take_value(&args, &mut index, "--chat-model")
                }
                value if value.starts_with("--chat-model=") => {
                    self.chat_model = value["--chat-model=".len()..].to_string();
                }
                "--embedding-model" => {
                    self.embedding_model =
                        take_value(&args, &mut index, "--embedding-model")
                }
                value if value.starts_with("--embedding-model=") => {
                    self.embedding_model =
                        value["--embedding-model=".len()..].to_string();
                }
                "--request-timeout-ms" => {
                    self.request_timeout_ms = take_value(&args, &mut index, "--request-timeout-ms")
                        .parse::<u64>()
                        .unwrap_or_else(|error| fail(&format!("invalid request timeout: {error}")));
                }
                value if value.starts_with("--request-timeout-ms=") => {
                    self.request_timeout_ms = value["--request-timeout-ms=".len()..]
                        .parse::<u64>()
                        .unwrap_or_else(|error| fail(&format!("invalid request timeout: {error}")));
                }
                "--candle-model-id" => {
                    self.candle.chat_model_id =
                        take_value(&args, &mut index, "--candle-model-id")
                }
                value if value.starts_with("--candle-model-id=") => {
                    self.candle.chat_model_id =
                        value["--candle-model-id=".len()..].to_string();
                }
                "--candle-chat-model-id" => {
                    self.candle.chat_model_id =
                        take_value(&args, &mut index, "--candle-chat-model-id")
                }
                value if value.starts_with("--candle-chat-model-id=") => {
                    self.candle.chat_model_id =
                        value["--candle-chat-model-id=".len()..].to_string();
                }
                "--candle-embedding-model-id" => {
                    self.candle.embedding_model_id =
                        take_value(&args, &mut index, "--candle-embedding-model-id")
                }
                value if value.starts_with("--candle-embedding-model-id=") => {
                    self.candle.embedding_model_id =
                        value["--candle-embedding-model-id=".len()..].to_string();
                }
                "--candle-cache-dir" => {
                    self.candle.cache_dir = take_value(&args, &mut index, "--candle-cache-dir")
                }
                value if value.starts_with("--candle-cache-dir=") => {
                    self.candle.cache_dir = value["--candle-cache-dir=".len()..].to_string();
                }
                "--candle-max-new-tokens" => {
                    self.candle.max_new_tokens =
                        take_value(&args, &mut index, "--candle-max-new-tokens")
                            .parse::<usize>()
                            .unwrap_or_else(|error| {
                                fail(&format!("invalid candle max tokens: {error}"))
                            });
                }
                value if value.starts_with("--candle-max-new-tokens=") => {
                    self.candle.max_new_tokens =
                        value["--candle-max-new-tokens=".len()..]
                            .parse::<usize>()
                            .unwrap_or_else(|error| {
                                fail(&format!("invalid candle max tokens: {error}"))
                            });
                }
                "--candle-temperature" => {
                    self.candle.temperature = take_value(&args, &mut index, "--candle-temperature")
                        .parse::<f64>()
                        .unwrap_or_else(|error| {
                            fail(&format!("invalid candle temperature: {error}"))
                        });
                }
                value if value.starts_with("--candle-temperature=") => {
                    self.candle.temperature = value["--candle-temperature=".len()..]
                        .parse::<f64>()
                        .unwrap_or_else(|error| fail(&format!("invalid candle temperature: {error}")));
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                value if value.starts_with('-') => fail(&format!("unknown flag: {value}")),
                value => fail(&format!("unexpected positional argument: {value}")),
            }
            index += 1;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelApiService {
    config: ModelApiConfig,
    backend: BackendRuntime,
}

impl ModelApiService {
    pub fn from_env_and_args() -> Self {
        let config = ModelApiConfig::from_env_and_args();
        Self::new(config)
    }

    pub fn new(config: ModelApiConfig) -> Self {
        let backend = match config.backend {
            BackendKind::Candle => {
                BackendRuntime::Candle(CandleBackend::new(config.candle.clone()))
            }
            BackendKind::OpenAIProxy => BackendRuntime::OpenAIProxy(OpenAIProxyBackend::new(
                config.upstream_base_url.clone(),
                config.chat_model.clone(),
                config.embedding_model.clone(),
                config.request_timeout_ms,
            )),
        };
        Self { config, backend }
    }

    pub fn config(&self) -> &ModelApiConfig {
        &self.config
    }

    pub fn handle_request(&self, mut request: Request) {
        let method = request.method().clone();
        let path = request.url().split('?').next().unwrap_or("/").to_string();
        let headers = request.headers().to_vec();
        let body = if method == Method::Post {
            match read_request_body(&mut request) {
                Ok(body) => body,
                Err(error) => {
                    let _ = request.respond(error_response(
                        StatusCode(500),
                        "INFRASTRUCTURE_ERROR",
                        &error,
                        "request-body",
                    ));
                    return;
                }
            }
        } else {
            Vec::new()
        };

        let response = self.route(method, &path, &headers, &body);
        let _ = request.respond(response);
    }

    pub fn route(
        &self,
        method: Method,
        path: &str,
        headers: &[Header],
        body: &[u8],
    ) -> Response<Cursor<Vec<u8>>> {
        match (method, path) {
            (Method::Get, "/healthz") => self.healthz_response(),
            (Method::Post, "/v1/chat/completions") => {
                self.backend.chat_completions(&self.config, headers, body)
            }
            (Method::Post, "/v1/embeddings") => self.backend.embeddings(&self.config, headers, body),
            (Method::Options, _) => no_content(),
            _ => error_response(
                StatusCode(404),
                "ROUTE_NOT_FOUND",
                &format!("route not found: {path}"),
                "router",
            ),
        }
    }

    fn healthz_response(&self) -> Response<Cursor<Vec<u8>>> {
        let report = self.backend.health(&self.config);
        match report.ready {
            true => json_response(StatusCode(200), &report),
            false => json_response(StatusCode(503), &report),
        }
    }
}

#[derive(Debug, Serialize)]
struct HealthReport {
    service: &'static str,
    status: &'static str,
    backend: BackendSummary,
    bind: String,
    upstream_base_url: String,
    chat_model: String,
    embedding_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    ready: bool,
}

#[derive(Debug, Serialize)]
struct BackendSummary {
    kind: &'static str,
    ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

enum BackendRuntime {
    Candle(CandleBackend),
    OpenAIProxy(OpenAIProxyBackend),
}

impl fmt::Debug for BackendRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Candle(_) => f.write_str("BackendRuntime::Candle"),
            Self::OpenAIProxy(_) => f.write_str("BackendRuntime::OpenAIProxy"),
        }
    }
}

impl Clone for BackendRuntime {
    fn clone(&self) -> Self {
        match self {
            Self::Candle(value) => Self::Candle(value.clone()),
            Self::OpenAIProxy(value) => Self::OpenAIProxy(value.clone()),
        }
    }
}

impl BackendRuntime {
    fn health(&self, config: &ModelApiConfig) -> HealthReport {
        match self {
            Self::Candle(backend) => backend.health(config),
            Self::OpenAIProxy(backend) => backend.health(config),
        }
    }

    fn chat_completions(
        &self,
        config: &ModelApiConfig,
        headers: &[Header],
        body: &[u8],
    ) -> Response<Cursor<Vec<u8>>> {
        match self {
            Self::Candle(backend) => backend.chat_completions(config, headers, body),
            Self::OpenAIProxy(backend) => backend.forward_json(
                "/chat/completions",
                &config.chat_model,
                headers,
                body,
            ),
        }
    }

    fn embeddings(
        &self,
        config: &ModelApiConfig,
        headers: &[Header],
        body: &[u8],
    ) -> Response<Cursor<Vec<u8>>> {
        match self {
            Self::Candle(backend) => backend.embeddings(config, headers, body),
            Self::OpenAIProxy(backend) => backend.forward_json(
                "/embeddings",
                &config.embedding_model,
                headers,
                body,
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct CandleBackend {
    config: CandleConfig,
    state: Arc<Mutex<CandleRuntimeState>>,
}

#[derive(Debug)]
enum CandleRuntimeState {
    Uninitialized,
    Ready(CandleRuntime),
    Failed(String),
}

#[derive(Debug)]
struct CandleRuntime {
    chat: CandleChatRuntime,
    embeddings: CandleEmbeddingRuntime,
}

#[derive(Debug)]
struct CandleChatRuntime {
    model: QwenModel,
    tokenizer: Tokenizer,
    device: Device,
    eos_tokens: Vec<u32>,
}

#[derive(Debug)]
struct CandleEmbeddingRuntime {
    model: JinaBertModel,
    tokenizer: Tokenizer,
    device: Device,
    dimensions: usize,
}

#[derive(Debug)]
struct CandleChatRequest {
    prompt: String,
    max_new_tokens: usize,
    temperature: f64,
    short_answer_options: Vec<String>,
    short_answer_focus: Option<String>,
}

#[derive(Debug)]
struct CandleChatCompletion {
    text: String,
    prompt_tokens: usize,
    completion_tokens: usize,
}

#[derive(Debug)]
struct CandleEmbeddingRequest {
    inputs: Vec<String>,
}

#[derive(Debug)]
struct CandleEmbeddingVector {
    values: Vec<f32>,
    token_count: usize,
}

#[derive(Debug)]
struct ResolvedModelAssets {
    tokenizer_file: PathBuf,
    config_file: PathBuf,
    weight_files: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct SafetensorIndex {
    weight_map: HashMap<String, String>,
}

impl CandleBackend {
    fn new(config: CandleConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(CandleRuntimeState::Uninitialized)),
        }
    }

    fn health(&self, config: &ModelApiConfig) -> HealthReport {
        match self.ensure_runtime_loaded() {
            Ok(dimensions) => HealthReport {
                service: SERVICE_NAME,
                status: HEALTH_OK,
                backend: BackendSummary {
                    kind: BackendKind::Candle.as_str(),
                    ready: true,
                    note: Some(CANDLE_CANDIDATE_NOTE.to_string()),
                },
                bind: config.bind.clone(),
                upstream_base_url: config.upstream_base_url.clone(),
                chat_model: self.config.chat_model_id.clone(),
                embedding_model: self.config.embedding_model_id.clone(),
                note: Some(format!(
                    "{CANDLE_CANDIDATE_NOTE}; chat and embeddings are loaded (embedding dims {dimensions})"
                )),
                ready: true,
            },
            Err(error) => HealthReport {
                service: SERVICE_NAME,
                status: HEALTH_DEGRADED,
                backend: BackendSummary {
                    kind: BackendKind::Candle.as_str(),
                    ready: false,
                    note: Some(CANDLE_CANDIDATE_NOTE.to_string()),
                },
                bind: config.bind.clone(),
                upstream_base_url: config.upstream_base_url.clone(),
                chat_model: self.config.chat_model_id.clone(),
                embedding_model: self.config.embedding_model_id.clone(),
                note: Some(format!(
                    "{CANDLE_CANDIDATE_NOTE}; last load error: {}",
                    trim_for_note(&error)
                )),
                ready: false,
            },
        }
    }

    fn chat_completions(
        &self,
        config: &ModelApiConfig,
        _headers: &[Header],
        body: &[u8],
    ) -> Response<Cursor<Vec<u8>>> {
        let request = match parse_candle_chat_request(body, &self.config) {
            Ok(request) => request,
            Err(error) => {
                return error_response(StatusCode(400), "VALIDATION_ERROR", &error, "candle");
            }
        };

        let completion = match self.run_chat(&request) {
            Ok(completion) => completion,
            Err(error) => {
                return error_response(StatusCode(503), "BACKEND_GENERATION_FAILED", &error, "candle");
            }
        };

        json_response(
            StatusCode(200),
            &json!({
                "id": format!("chatcmpl-{}", current_timestamp_ms()),
                "object": "chat.completion",
                "created": current_timestamp_secs(),
                "model": self.config.chat_model_id,
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": completion.text,
                        },
                        "finish_reason": "stop",
                    }
                ],
                "usage": {
                    "prompt_tokens": completion.prompt_tokens,
                    "completion_tokens": completion.completion_tokens,
                    "total_tokens": completion.prompt_tokens + completion.completion_tokens,
                },
                "experimental": {
                    "backend": "candle",
                    "mode": "candidate-side-lane",
                    "default_backend": "openai_proxy",
                },
                "note": CANDLE_CANDIDATE_NOTE,
                "bind": config.bind,
            }),
        )
    }

    fn embeddings(
        &self,
        config: &ModelApiConfig,
        _headers: &[Header],
        body: &[u8],
    ) -> Response<Cursor<Vec<u8>>> {
        let request = match parse_candle_embedding_request(body) {
            Ok(request) => request,
            Err(error) => {
                return error_response(StatusCode(400), "VALIDATION_ERROR", &error, "candle");
            }
        };

        let vectors = match self.run_embeddings(&request) {
            Ok(vectors) => vectors,
            Err(error) => {
                return error_response(StatusCode(503), "BACKEND_EMBEDDING_FAILED", &error, "candle");
            }
        };

        let prompt_tokens = vectors.iter().map(|value| value.token_count).sum::<usize>();
        json_response(
            StatusCode(200),
            &json!({
                "object": "list",
                "data": vectors
                    .iter()
                    .enumerate()
                    .map(|(index, value)| json!({
                        "object": "embedding",
                        "index": index,
                        "embedding": value.values,
                    }))
                    .collect::<Vec<_>>(),
                "model": self.config.embedding_model_id,
                "usage": {
                    "prompt_tokens": prompt_tokens,
                    "total_tokens": prompt_tokens,
                },
                "experimental": {
                    "backend": "candle",
                    "mode": "candidate-side-lane",
                    "default_backend": "openai_proxy",
                },
                "note": CANDLE_CANDIDATE_NOTE,
                "bind": config.bind,
            }),
        )
    }

    fn run_chat(&self, request: &CandleChatRequest) -> Result<CandleChatCompletion, String> {
        self.with_runtime(|runtime| self.generate_with_runtime(&mut runtime.chat, request))
    }

    fn run_embeddings(&self, request: &CandleEmbeddingRequest) -> Result<Vec<CandleEmbeddingVector>, String> {
        self.with_runtime(|runtime| self.embed_with_runtime(&runtime.embeddings, request))
    }

    fn with_runtime<T>(
        &self,
        f: impl FnOnce(&mut CandleRuntime) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "candle runtime lock is poisoned".to_string())?;

        if matches!(&*state, CandleRuntimeState::Uninitialized | CandleRuntimeState::Failed(_)) {
            *state = match self.load_runtime() {
                Ok(runtime) => CandleRuntimeState::Ready(runtime),
                Err(error) => CandleRuntimeState::Failed(format!("{error:#}")),
            };
        }

        match &mut *state {
            CandleRuntimeState::Ready(runtime) => f(runtime),
            CandleRuntimeState::Failed(error) => Err(format!(
                "failed to initialize candle runtime: {}",
                trim_for_note(error)
            )),
            CandleRuntimeState::Uninitialized => Err("candle runtime did not initialize".to_string()),
        }
    }

    fn ensure_runtime_loaded(&self) -> Result<usize, String> {
        self.with_runtime(|runtime| Ok(runtime.embeddings.dimensions))
    }

    fn load_runtime(&self) -> AnyResult<CandleRuntime> {
        let cache_dir = PathBuf::from(self.config.cache_dir.trim());
        if cache_dir.as_os_str().is_empty() {
            return Err(anyhow!("candle cache dir is empty"));
        }
        fs::create_dir_all(&cache_dir).with_context(|| {
            format!("failed to create candle cache dir {}", cache_dir.display())
        })?;
        let hub_cache_dir = cache_dir.join("hub");
        fs::create_dir_all(&hub_cache_dir).with_context(|| {
            format!(
                "failed to create candle huggingface cache dir {}",
                hub_cache_dir.display()
            )
        })?;
        env::set_var("HF_HOME", &cache_dir);
        env::set_var("HF_HUB_CACHE", &hub_cache_dir);

        let chat_assets = resolve_model_assets(&self.config.chat_model_id)?;
        let embedding_assets = resolve_model_assets(&self.config.embedding_model_id)?;

        let chat_tokenizer = Tokenizer::from_file(&chat_assets.tokenizer_file)
            .map_err(|error| anyhow!("failed to load tokenizer: {error}"))?;
        let device = Device::Cpu;
        let chat_vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&chat_assets.weight_files, DType::F32, &device)
                .context("failed to mmap qwen3 model weights")?
        };
        let config: QwenConfig = serde_json::from_slice(
            &fs::read(&chat_assets.config_file).with_context(|| {
                format!("failed to read qwen3 config {}", chat_assets.config_file.display())
            })?,
        )
        .context("failed to parse qwen3 config")?;
        let chat_model =
            QwenModel::new(&config, chat_vb).context("failed to construct qwen3 model")?;

        let mut eos_tokens = Vec::new();
        for token in ["<|im_end|>", "<|endoftext|>", "<|eot_id|>"] {
            if let Some(id) = chat_tokenizer.token_to_id(token) {
                eos_tokens.push(id);
            }
        }

        let embedding_tokenizer = Tokenizer::from_file(&embedding_assets.tokenizer_file)
            .map_err(|error| anyhow!("failed to load jina tokenizer: {error}"))?;
        let embedding_vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &embedding_assets.weight_files,
                DType::F32,
                &device,
            )
            .context("failed to mmap jina embedding weights")?
        };
        let embedding_config: JinaBertConfig = serde_json::from_slice(
            &fs::read(&embedding_assets.config_file).with_context(|| {
                format!(
                    "failed to read jina embedding config {}",
                    embedding_assets.config_file.display()
                )
            })?,
        )
        .context("failed to parse jina embedding config")?;
        let embedding_dimensions = embedding_config.hidden_size;
        let embedding_model = JinaBertModel::new(embedding_vb, &embedding_config)
            .context("failed to construct jina embedding model")?;

        Ok(CandleRuntime {
            chat: CandleChatRuntime {
                model: chat_model,
                tokenizer: chat_tokenizer,
                device: device.clone(),
                eos_tokens,
            },
            embeddings: CandleEmbeddingRuntime {
                model: embedding_model,
                tokenizer: embedding_tokenizer,
                device,
                dimensions: embedding_dimensions,
            },
        })
    }

    fn generate_with_runtime(
        &self,
        runtime: &mut CandleChatRuntime,
        request: &CandleChatRequest,
    ) -> Result<CandleChatCompletion, String> {
        // Each HTTP request is an independent chat turn. Clear any retained KV
        // cache so a previous prompt does not corrupt the next forward pass.
        runtime.model.clear_kv_cache();

        let mut tokens = runtime
            .tokenizer
            .encode(request.prompt.as_str(), true)
            .map_err(|error| format!("failed to tokenize prompt: {error}"))?
            .get_ids()
            .to_vec();
        if tokens.is_empty() {
            return Err("candle prompt produced no tokens".to_string());
        }

        let prompt_tokens = tokens.len();
        let mut generated = Vec::new();
        let mut logits_processor =
            LogitsProcessor::new(self.config.seed, Some(request.temperature), None);

        for index in 0..request.max_new_tokens.max(1) {
            let context_size = if index > 0 { 1 } else { tokens.len() };
            let start_pos = tokens.len().saturating_sub(context_size);
            let context = &tokens[start_pos..];
            let input = Tensor::new(context, &runtime.device)
                .map_err(|error| format!("failed to build input tensor: {error}"))?
                .unsqueeze(0)
                .map_err(|error| format!("failed to unsqueeze input tensor: {error}"))?;
            let logits = runtime
                .model
                .forward(&input, start_pos)
                .map_err(|error| format!("candle forward failed: {error}"))?;
            let logits = logits
                .squeeze(0)
                .and_then(|value| value.squeeze(0))
                .and_then(|value| value.to_dtype(DType::F32))
                .map_err(|error| format!("failed to normalize logits: {error}"))?;
            let logits = if (self.config.repeat_penalty - 1.0).abs() < f32::EPSILON {
                logits
            } else {
                let start_at = tokens.len().saturating_sub(self.config.repeat_last_n);
                candle_transformers::utils::apply_repeat_penalty(
                    &logits,
                    self.config.repeat_penalty,
                    &tokens[start_at..],
                )
                .map_err(|error| format!("failed to apply repeat penalty: {error}"))?
            };

            let next_token = logits_processor
                .sample(&logits)
                .map_err(|error| format!("failed to sample next token: {error}"))?;
            if runtime.eos_tokens.contains(&next_token) {
                break;
            }
            tokens.push(next_token);
            generated.push(next_token);
        }

        if generated.is_empty() {
            return Err("candle model generated no completion tokens".to_string());
        }

        let text = runtime
            .tokenizer
            .decode(&generated, true)
            .map_err(|error| format!("failed to decode generated text: {error}"))?;
        let sanitized = sanitize_candle_completion_text(&text, request);
        if sanitized.is_empty() {
            runtime.model.clear_kv_cache();
            return Err("candle model generated an empty completion".to_string());
        }

        runtime.model.clear_kv_cache();
        Ok(CandleChatCompletion {
            text: sanitized,
            prompt_tokens,
            completion_tokens: generated.len(),
        })
    }

    fn embed_with_runtime(
        &self,
        runtime: &CandleEmbeddingRuntime,
        request: &CandleEmbeddingRequest,
    ) -> Result<Vec<CandleEmbeddingVector>, String> {
        request
            .inputs
            .iter()
            .map(|input| {
                let encoding = runtime
                    .tokenizer
                    .encode(input.as_str(), true)
                    .map_err(|error| format!("failed to tokenize embedding input: {error}"))?;
                let ids = encoding.get_ids().to_vec();
                if ids.is_empty() {
                    return Err("embedding input produced no tokens".to_string());
                }

                let input_tensor = Tensor::new(ids.as_slice(), &runtime.device)
                    .map_err(|error| format!("failed to build embedding tensor: {error}"))?
                    .unsqueeze(0)
                    .map_err(|error| format!("failed to batch embedding tensor: {error}"))?;
                let sequence = runtime
                    .model
                    .forward(&input_tensor)
                    .map_err(|error| format!("candle embedding forward failed: {error}"))?;
                let pooled = sequence
                    .mean(1)
                    .and_then(|value| value.squeeze(0))
                    .and_then(|value| value.to_dtype(DType::F32))
                    .map_err(|error| format!("failed to pool embedding output: {error}"))?;
                let mut values = pooled
                    .to_vec1::<f32>()
                    .map_err(|error| format!("failed to materialize embedding vector: {error}"))?;
                if values.is_empty() {
                    return Err("embedding output was empty".to_string());
                }
                let l2 = values
                    .iter()
                    .map(|value| value * value)
                    .sum::<f32>()
                    .sqrt();
                if !l2.is_finite() || l2 <= f32::EPSILON {
                    return Err("embedding output had zero norm".to_string());
                }
                for value in &mut values {
                    *value /= l2;
                }

                Ok(CandleEmbeddingVector {
                    values,
                    token_count: encoding.len(),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct OpenAIProxyBackend {
    client: Client,
    upstream_base_url: String,
    chat_model: String,
    embedding_model: String,
    timeout_ms: u64,
}

impl OpenAIProxyBackend {
    fn new(
        upstream_base_url: String,
        chat_model: String,
        embedding_model: String,
        timeout_ms: u64,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .unwrap_or_else(|error| fail(&format!("failed to build HTTP client: {error}")));
        Self {
            client,
            upstream_base_url,
            chat_model,
            embedding_model,
            timeout_ms,
        }
    }

    fn health(&self, config: &ModelApiConfig) -> HealthReport {
        let probe_urls = [
            join_url(&self.upstream_base_url, "/healthz"),
            join_url(&self.upstream_base_url, "/models"),
        ];
        let ready_url = probe_urls.iter().find_map(|url| {
            self.client
                .get(url.clone())
                .send()
                .ok()
                .and_then(|response| response.error_for_status().ok())
                .map(|_| url.clone())
        });
        let ready = ready_url.is_some();

        if ready {
            HealthReport {
                service: SERVICE_NAME,
                status: HEALTH_OK,
                backend: BackendSummary {
                    kind: BackendKind::OpenAIProxy.as_str(),
                    ready: true,
                    note: None,
                },
                bind: config.bind.clone(),
                upstream_base_url: self.upstream_base_url.clone(),
                chat_model: self.chat_model.clone(),
                embedding_model: self.embedding_model.clone(),
                note: Some(format!(
                    "upstream health checked via {}",
                    ready_url.unwrap_or_else(|| join_url(&self.upstream_base_url, "/models"))
                )),
                ready: true,
            }
        } else {
            HealthReport {
                service: SERVICE_NAME,
                status: HEALTH_DEGRADED,
                backend: BackendSummary {
                    kind: BackendKind::OpenAIProxy.as_str(),
                    ready: false,
                    note: Some(format!(
                        "upstream health check failed at {} and {}; timeout {} ms",
                        probe_urls[0], probe_urls[1], self.timeout_ms
                    )),
                },
                bind: config.bind.clone(),
                upstream_base_url: self.upstream_base_url.clone(),
                chat_model: self.chat_model.clone(),
                embedding_model: self.embedding_model.clone(),
                note: Some("openai_proxy backend is configured but upstream is unhealthy".to_string()),
                ready: false,
            }
        }
    }

    fn forward_json(
        &self,
        path_suffix: &str,
        default_model: &str,
        headers: &[Header],
        body: &[u8],
    ) -> Response<Cursor<Vec<u8>>> {
        let mut payload: Value = match serde_json::from_slice(body) {
            Ok(value) => value,
            Err(error) => {
                return error_response(
                    StatusCode(400),
                    "INVALID_JSON",
                    &format!("invalid JSON body: {error}"),
                    path_suffix,
                )
            }
        };
        normalize_model(&mut payload, default_model);

        let upstream_url = join_url(&self.upstream_base_url, path_suffix);
        let mut request = self
            .client
            .post(upstream_url.clone())
            .header(CONTENT_TYPE, "application/json");
        if let Some(auth_header) = header_value(headers, "Authorization") {
            request = request.header(AUTHORIZATION, auth_header);
        }

        let response = match request.json(&payload).send() {
            Ok(response) => response,
            Err(error) => {
                return error_response(
                    StatusCode(503),
                    "UPSTREAM_UNAVAILABLE",
                    &format!("failed to reach upstream model API: {error}"),
                    path_suffix,
                )
            }
        };

        let status = StatusCode(response.status().as_u16());
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = match response.bytes() {
            Ok(bytes) => bytes.to_vec(),
            Err(error) => {
                return error_response(
                    StatusCode(502),
                    "UPSTREAM_READ_ERROR",
                    &format!("failed to read upstream response: {error}"),
                    path_suffix,
                )
            }
        };

        let mut response = Response::from_data(body).with_status_code(status);
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                content_type
                    .as_deref()
                    .unwrap_or("application/json; charset=utf-8")
                    .as_bytes(),
            )
            .expect("content-type header"),
        );
        response
    }
}

fn normalize_model(payload: &mut Value, default_model: &str) {
    if let Some(object) = payload.as_object_mut() {
        let has_model = object
            .get("model")
            .and_then(Value::as_str)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        if !has_model {
            object.insert("model".to_string(), Value::String(default_model.to_string()));
        }
    }
}

fn parse_candle_chat_request(body: &[u8], config: &CandleConfig) -> Result<CandleChatRequest, String> {
    let payload: Value = serde_json::from_slice(body)
        .map_err(|error| format!("invalid JSON body: {error}"))?;
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "messages must be a non-empty array".to_string())?;
    if messages.is_empty() {
        return Err("messages must be a non-empty array".to_string());
    }

    let mut segments = Vec::new();
    let mut has_system = false;
    let mut has_user_like_message = false;
    let mut latest_user_like_content = None;

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("user");
        let normalized_role = match role {
            "system" => {
                has_system = true;
                "system"
            }
            "assistant" => "assistant",
            _ => {
                has_user_like_message = true;
                "user"
            }
        };
        let content = extract_candle_message_content(message)
            .ok_or_else(|| format!("message content missing or unsupported for role {role}"))?;
        if normalized_role == "user" {
            latest_user_like_content = Some(content.clone());
        }
        segments.push(format!(
            "<|im_start|>{normalized_role}\n{}\n<|im_end|>",
            content.trim()
        ));
    }

    if !has_user_like_message {
        return Err("messages must include at least one user message".to_string());
    }
    if !has_system {
        segments.insert(
            0,
            format!("<|im_start|>system\n{CANDLE_SYSTEM_PROMPT}\n<|im_end|>"),
        );
    }
    segments.push(format!(
        "<|im_start|>system\n{CANDLE_OUTPUT_POLICY_PROMPT}\n<|im_end|>"
    ));
    segments.push("<|im_start|>assistant\n".to_string());

    let max_new_tokens = payload
        .get("max_tokens")
        .or_else(|| payload.get("max_completion_tokens"))
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .map(|value| value.min(config.max_new_tokens).max(1))
        .unwrap_or(config.max_new_tokens);
    let temperature = payload
        .get("temperature")
        .and_then(Value::as_f64)
        .unwrap_or(config.temperature);
    let short_answer_options = latest_user_like_content
        .as_deref()
        .map(detect_short_answer_options)
        .unwrap_or_default();
    let short_answer_focus = latest_user_like_content
        .as_deref()
        .and_then(detect_short_answer_focus);

    Ok(CandleChatRequest {
        prompt: segments.join("\n"),
        max_new_tokens,
        temperature,
        short_answer_options,
        short_answer_focus,
    })
}

fn parse_candle_embedding_request(body: &[u8]) -> Result<CandleEmbeddingRequest, String> {
    let payload: Value = serde_json::from_slice(body)
        .map_err(|error| format!("invalid JSON body: {error}"))?;
    let input = payload
        .get("input")
        .ok_or_else(|| "input must be a string or a non-empty string array".to_string())?;

    let inputs = if let Some(text) = input.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("input must not be empty".to_string());
        }
        vec![trimmed.to_string()]
    } else if let Some(items) = input.as_array() {
        let values = items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| "embedding input arrays must contain non-empty strings".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values.is_empty() {
            return Err("input array must not be empty".to_string());
        }
        values
    } else {
        return Err("input must be a string or a non-empty string array".to_string());
    };

    Ok(CandleEmbeddingRequest { inputs })
}

fn extract_candle_message_content(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    if let Some(text) = content.as_str() {
        let trimmed = text.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    let parts = content.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") | None => part.get("text").and_then(Value::as_str),
            _ => None,
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn sanitize_candle_completion_text(text: &str, request: &CandleChatRequest) -> String {
    let mut output = normalize_line_endings(text);
    loop {
        let trimmed = output.trim_start();
        if let Some(rest) = trimmed.strip_prefix("<think>") {
            if let Some(end) = rest.find("</think>") {
                output = rest[end + "</think>".len()..].to_string();
            } else {
                output = rest.to_string();
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("</think>") {
            output = rest.to_string();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("<think") {
            if let Some(end) = rest.find('>') {
                output = rest[end + 1..].to_string();
                continue;
            }
        }
        output = trimmed.to_string();
        break;
    }

    let mut lines = Vec::new();
    let mut seen_content = false;
    for raw_line in output.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if seen_content {
                lines.push(String::new());
            }
            continue;
        }

        if !seen_content {
            if is_candle_preamble_line(trimmed) {
                continue;
            }

            if let Some(stripped) = strip_candle_preamble_prefix(trimmed) {
                if stripped.is_empty() {
                    continue;
                }
                lines.push(stripped);
                seen_content = true;
                continue;
            }
        }

        seen_content = true;
        lines.push(line.to_string());
    }

    let mut sanitized = lines.join("\n").trim().to_string();
    while sanitized.contains("\n\n\n") {
        sanitized = sanitized.replace("\n\n\n", "\n\n");
    }
    if let Some(short_answer) =
        extract_short_answer_from_response(
            &sanitized,
            &request.short_answer_options,
            request.short_answer_focus.as_deref(),
        )
    {
        return short_answer;
    }
    sanitized
}

fn detect_short_answer_options(prompt: &str) -> Vec<String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if (trimmed.contains("“是”") && trimmed.contains("“否”"))
        || (trimmed.contains("\"是\"") && trimmed.contains("\"否\""))
    {
        return vec!["是".to_string(), "否".to_string()];
    }

    let quoted = extract_quoted_segments(trimmed);
    if trimmed.contains("哪一个") && quoted.len() >= 2 {
        return dedup_short_answer_options(quoted);
    }

    if let Some(index) = trimmed.rfind("更像") {
        let tail = &trimmed[index + "更像".len()..];
        let options = split_short_answer_options(tail);
        if options.len() >= 2 {
            return options;
        }
    }

    Vec::new()
}

fn extract_quoted_segments(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for character in value.chars() {
        match character {
            '“' | '"' if !in_quote => {
                in_quote = true;
                current.clear();
            }
            '”' | '"' if in_quote => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    result.push(trimmed.to_string());
                }
                current.clear();
                in_quote = false;
            }
            _ if in_quote => current.push(character),
            _ => {}
        }
    }
    dedup_short_answer_options(result)
}

fn split_short_answer_options(value: &str) -> Vec<String> {
    let normalized = value
        .replace("还是", "|")
        .replace("或者", "|")
        .replace("或", "|")
        .replace('、', "|")
        .replace('，', "|")
        .replace(',', "|")
        .replace('。', "|")
        .replace('？', "|")
        .replace('?', "|")
        .replace('！', "|")
        .replace('!', "|")
        .replace('：', "|")
        .replace(':', "|")
        .replace('“', "|")
        .replace('”', "|")
        .replace('"', "|");
    let options = normalized
        .split('|')
        .map(str::trim)
        .filter(|token| {
            !token.is_empty()
                && token.chars().count() <= 8
                && *token != "请只回答一个词"
                && *token != "请只回答"
                && *token != "一个词"
                && *token != "哪一个"
                && *token != "更像"
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    dedup_short_answer_options(options)
}

fn dedup_short_answer_options(options: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    options
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn detect_short_answer_focus(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    let index = trimmed.rfind("更像")?;
    let tail = trimmed[index + "更像".len()..]
        .trim()
        .trim_matches(|character: char| matches!(character, '？' | '?' | '。' | '！' | '!'))
        .trim();
    if tail.is_empty() || tail.contains("还是") || tail.contains('、') || tail.contains('或') {
        return None;
    }
    (tail.chars().count() <= 12).then(|| tail.to_string())
}

fn extract_short_answer_from_response(
    value: &str,
    options: &[String],
    focus_phrase: Option<&str>,
) -> Option<String> {
    if options.is_empty() {
        return None;
    }

    for line in value.lines().map(str::trim).rev() {
        if line.is_empty() {
            continue;
        }
        if let Some(answer) = match_short_answer_line(line, options, focus_phrase) {
            return Some(answer);
        }
    }
    match_short_answer_line(value.trim(), options, focus_phrase)
}

fn match_short_answer_line(
    value: &str,
    options: &[String],
    focus_phrase: Option<&str>,
) -> Option<String> {
    let normalized = normalize_short_answer_text(value);
    if normalized.is_empty() {
        return None;
    }

    if options.len() == 2
        && options[0] == "是"
        && options[1] == "否"
    {
        if normalized.contains("不能")
            || normalized.contains("不行")
            || normalized.contains("不可以")
        {
            return Some("否".to_string());
        }
        if normalized.contains('否') {
            return Some("否".to_string());
        }
        if normalized.contains("可以")
            || normalized.contains("能")
            || normalized.contains('是')
        {
            return Some("是".to_string());
        }
    }

    if let Some(answer) = focus_phrase
        .and_then(|focus| match_short_answer_by_focus(&normalized, options, focus))
    {
        return Some(answer);
    }

    if let Some(answer) = focus_phrase
        .and_then(|focus| match_short_answer_by_domain_hint(options, focus))
    {
        return Some(answer);
    }

    for option in options {
        if normalized == normalize_short_answer_text(option) {
            return Some(option.clone());
        }
    }

    let mut matches = options
        .iter()
        .filter_map(|option| normalized.rfind(option).map(|position| (position, option.clone())))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.0.cmp(&right.0));
    matches.first().map(|(_, option)| option).cloned()
}

fn match_short_answer_by_focus(
    value: &str,
    options: &[String],
    focus_phrase: &str,
) -> Option<String> {
    let focus_positions = value.match_indices(focus_phrase).map(|(index, _)| index).collect::<Vec<_>>();
    if focus_positions.is_empty() {
        return None;
    }

    let mut best = None::<(usize, String)>;
    for option in options {
        for (position, _) in value.match_indices(option) {
            let distance = focus_positions
                .iter()
                .map(|focus| focus.abs_diff(position))
                .min()
                .unwrap_or(usize::MAX);
            if best.as_ref().is_none_or(|current| distance < current.0) {
                best = Some((distance, option.clone()));
            }
        }
    }
    best.map(|(_, option)| option)
}

fn match_short_answer_by_domain_hint(options: &[String], focus_phrase: &str) -> Option<String> {
    let normalized_focus = normalize_short_answer_text(focus_phrase);
    let has_record = options.iter().any(|option| option == "录像");
    let has_snapshot = options.iter().any(|option| option == "抓拍");
    if has_record && has_snapshot {
        if contains_any_phrase(
            &normalized_focus,
            &["持续动作", "连续动作", "持续", "连续", "一直", "不停", "长时间"],
        ) {
            return Some("录像".to_string());
        }
        if contains_any_phrase(
            &normalized_focus,
            &["单次", "瞬时", "一下", "一张", "静态", "抓拍"],
        ) {
            return Some("抓拍".to_string());
        }
    }
    None
}

fn contains_any_phrase(value: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| value.contains(phrase))
}

fn normalize_short_answer_text(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("最终答案")
        .trim_start_matches("答案是")
        .trim_start_matches("答案")
        .trim_start_matches("结论")
        .trim_start_matches("答：")
        .trim_start_matches("答:")
        .trim_start_matches(|character: char| {
            matches!(
                character,
                ':' | '：' | '。' | '，' | ',' | '！' | '!' | '？' | '?' | '"' | '\'' | '“' | '”' | ' '
            )
        })
        .trim()
        .to_string()
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn is_candle_preamble_line(line: &str) -> bool {
    matches!(
        line.trim_matches(|ch: char| matches!(ch, ':' | '：' | '。' | ' ' | '\t')),
        "分析"
            | "分析如下"
            | "思考"
            | "思考如下"
            | "推理"
            | "结论"
            | "最终答案"
            | "答案"
            | "analysis"
            | "reasoning"
            | "thinking"
            | "final answer"
            | "answer"
    )
}

fn strip_candle_preamble_prefix(line: &str) -> Option<String> {
    const PREFIXES: &[&str] = &[
        "最终答案",
        "分析如下",
        "思考如下",
        "analysis",
        "reasoning",
        "thinking",
        "final answer",
    ];

    for prefix in PREFIXES {
        if let Some(rest) = strip_prefix_case_insensitive(line, prefix) {
            let rest = rest
                .trim_start_matches(|ch: char| {
                    matches!(ch, ':' | '：' | '。' | '，' | ',' | '-' | '—' | ' ' | '\t')
                })
                .trim();
            return Some(rest.to_string());
        }
    }

    for prefix in ["分析", "思考", "推理", "结论", "答案"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let rest = rest
                .trim_start_matches(|ch: char| {
                    matches!(ch, ':' | '：' | '。' | '，' | ',' | '-' | '—' | ' ' | '\t')
                })
                .trim();
            return Some(rest.to_string());
        }
    }

    None
}

fn strip_prefix_case_insensitive<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        text.get(prefix.len()..)
    } else {
        None
    }
}

fn resolve_model_assets(model_id: &str) -> AnyResult<ResolvedModelAssets> {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("candle model id is empty"));
    }

    let model_path = PathBuf::from(trimmed);
    if model_path.exists() {
        resolve_local_model_assets(&model_path)
    } else {
        resolve_hf_model_assets(trimmed)
    }
}

fn resolve_local_model_assets(model_path: &PathBuf) -> AnyResult<ResolvedModelAssets> {
    let base_dir = if model_path.is_dir() {
        model_path.clone()
    } else {
        model_path
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("local candle model path has no parent directory"))?
    };

    let tokenizer_file = base_dir.join("tokenizer.json");
    let config_file = base_dir.join("config.json");
    for required in [&tokenizer_file, &config_file] {
        if !required.exists() {
            return Err(anyhow!(
                "local candle model directory is missing {}",
                required.display()
            ));
        }
    }

    let index_file = base_dir.join("model.safetensors.index.json");
    let weight_files = if index_file.exists() {
        resolve_weight_files_from_index(&index_file, |relative| {
            let shard = base_dir.join(relative);
            if !shard.exists() {
                return Err(anyhow!(
                    "local candle model directory is missing shard {}",
                    shard.display()
                ));
            }
            Ok(shard)
        })?
    } else {
        let weight_file = base_dir.join("model.safetensors");
        if !weight_file.exists() {
            return Err(anyhow!(
                "local candle model directory is missing {}",
                weight_file.display()
            ));
        }
        vec![weight_file]
    };

    Ok(ResolvedModelAssets {
        tokenizer_file,
        config_file,
        weight_files,
    })
}

fn resolve_hf_model_assets(model_id: &str) -> AnyResult<ResolvedModelAssets> {
    let api = ApiBuilder::from_env()
        .build()
        .context("failed to initialize Hugging Face hub client")?;
    let repo = api.repo(Repo::with_revision(
        model_id.to_string(),
        RepoType::Model,
        "main".to_string(),
    ));

    let tokenizer_file = repo
        .get("tokenizer.json")
        .with_context(|| format!("failed to fetch tokenizer for {model_id}"))?;
    let config_file = repo
        .get("config.json")
        .with_context(|| format!("failed to fetch config for {model_id}"))?;
    let index_file = repo.get("model.safetensors.index.json").ok();
    let weight_files = if let Some(index_file) = index_file {
        resolve_weight_files_from_index(&index_file, |relative| {
            repo.get(relative)
                .with_context(|| format!("failed to fetch shard {relative} for {model_id}"))
        })?
    } else {
        vec![repo
            .get("model.safetensors")
            .with_context(|| format!("failed to fetch weights for {model_id}"))?]
    };

    Ok(ResolvedModelAssets {
        tokenizer_file,
        config_file,
        weight_files,
    })
}

fn resolve_weight_files_from_index(
    index_file: &PathBuf,
    mut fetch_shard: impl FnMut(&str) -> AnyResult<PathBuf>,
) -> AnyResult<Vec<PathBuf>> {
    let index: SafetensorIndex = serde_json::from_slice(
        &fs::read(index_file)
            .with_context(|| format!("failed to read safetensor index {}", index_file.display()))?,
    )
    .with_context(|| format!("failed to parse safetensor index {}", index_file.display()))?;

    let mut seen = HashSet::new();
    let mut weight_files = Vec::new();
    for relative in index.weight_map.values() {
        if seen.insert(relative.clone()) {
            weight_files.push(fetch_shard(relative)?);
        }
    }
    if weight_files.is_empty() {
        return Err(anyhow!(
            "safetensor index {} did not reference any shard files",
            index_file.display()
        ));
    }
    Ok(weight_files)
}

fn join_url(base: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn env_or_default(name: &str, default: &str) -> String {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn read_request_body(request: &mut Request) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    request
        .as_reader()
        .read_to_end(&mut body)
        .map_err(|error| format!("failed to read request body: {error}"))?;
    Ok(body)
}

fn header_value(headers: &[Header], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.field.as_str().to_string().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

fn error_response(
    status: StatusCode,
    code: &'static str,
    message: &str,
    source: &str,
) -> Response<Cursor<Vec<u8>>> {
    json_response(
        status,
        &json!({
            "ok": false,
            "error": {
                "code": code,
                "message": message,
                "source": source,
            }
        }),
    )
}

fn no_content() -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(StatusCode(204));
    add_common_headers(&mut response);
    response
}

fn json_response(status: StatusCode, payload: &impl Serialize) -> Response<Cursor<Vec<u8>>> {
    let body = serde_json::to_vec_pretty(payload).unwrap_or_else(|_| {
        serde_json::to_vec(&json!({
            "ok": false,
            "error": {
                "code": "INFRASTRUCTURE_ERROR",
                "message": "serialize failed"
            }
        }))
        .unwrap_or_else(|_| b"{\"ok\":false}".to_vec())
    });
    let mut response = Response::from_data(body).with_status_code(status);
    add_common_headers(&mut response);
    response.add_header(
        Header::from_bytes(
            b"Content-Type".as_slice(),
            b"application/json; charset=utf-8".as_slice(),
        )
        .expect("content-type header"),
    );
    response
}

fn add_common_headers<R: Read>(response: &mut Response<R>) {
    for header in [
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Headers", "Content-Type, Authorization"),
        ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        ("Cache-Control", "no-store"),
    ] {
        response.add_header(
            Header::from_bytes(header.0.as_bytes(), header.1.as_bytes()).expect("header"),
        );
    }
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> String {
    *index += 1;
    if *index >= args.len() {
        fail(&format!("missing value for {flag}"));
    }
    args[*index].clone()
}

fn trim_for_note(value: &str) -> String {
    let trimmed = value.trim();
    let char_count = trimmed.chars().count();
    if char_count <= 180 {
        return trimmed.to_string();
    }
    trimmed.chars().take(180).collect::<String>() + "..."
}

fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn print_usage() {
    eprintln!(
        "Usage: harbor-model-api [--bind ADDR] [--backend candle|openai_proxy] [--upstream-base-url URL] [--chat-model NAME] [--embedding-model NAME] [--request-timeout-ms N] [--candle-model-id ID] [--candle-chat-model-id ID] [--candle-embedding-model-id ID] [--candle-cache-dir DIR] [--candle-max-new-tokens N] [--candle-temperature F]"
    );
}

pub fn print_startup_banner(config: &ModelApiConfig) {
    match config.backend {
        BackendKind::Candle => println!(
            "{} listening on http://{} (backend {}, chat model {}, embedding model {}, cache {}, candidate side lane)",
            SERVICE_NAME,
            config.bind,
            config.backend,
            config.candle.chat_model_id,
            config.candle.embedding_model_id,
            config.candle.cache_dir
        ),
        BackendKind::OpenAIProxy => println!(
            "{} listening on http://{} (backend {}, upstream {}, chat model {}, embedding model {})",
            SERVICE_NAME,
            config.bind,
            config.backend,
            config.upstream_base_url,
            config.chat_model,
            config.embedding_model
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_parsing_accepts_alternates() {
        assert_eq!("candle".parse::<BackendKind>().unwrap(), BackendKind::Candle);
        assert_eq!(
            "openai_proxy".parse::<BackendKind>().unwrap(),
            BackendKind::OpenAIProxy
        );
        assert_eq!(
            "openai-proxy".parse::<BackendKind>().unwrap(),
            BackendKind::OpenAIProxy
        );
        assert_eq!(BackendKind::OpenAIProxy.to_string(), "openai_proxy");
    }

    #[test]
    fn normalize_model_injects_default_for_missing_model() {
        let mut payload = json!({
            "messages": [{"role": "user", "content": "hello"}]
        });
        normalize_model(&mut payload, "harbor-local-chat");
        assert_eq!(payload["model"], json!("harbor-local-chat"));
    }

    #[test]
    fn normalize_model_preserves_explicit_model() {
        let mut payload = json!({
            "model": "custom-chat",
            "input": "hello"
        });
        normalize_model(&mut payload, "harbor-local-embed");
        assert_eq!(payload["model"], json!("custom-chat"));
    }

    #[test]
    fn join_url_trims_duplicate_slashes() {
        assert_eq!(
            join_url("http://127.0.0.1:11434/v1/", "/chat/completions"),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
    }

    #[test]
    fn default_config_stays_local_first() {
        let config = ModelApiConfig::default();
        assert_eq!(config.bind, DEFAULT_BIND);
        assert_eq!(config.backend, BackendKind::OpenAIProxy);
        assert_eq!(config.upstream_base_url, DEFAULT_UPSTREAM_BASE_URL);
        assert_eq!(config.chat_model, DEFAULT_CHAT_MODEL);
        assert_eq!(config.embedding_model, DEFAULT_EMBEDDING_MODEL);
        assert_eq!(config.candle.chat_model_id, DEFAULT_CANDLE_CHAT_MODEL_ID);
        assert_eq!(
            config.candle.embedding_model_id,
            DEFAULT_CANDLE_EMBEDDING_MODEL_ID
        );
        assert_eq!(config.candle.cache_dir, DEFAULT_CANDLE_CACHE_DIR);
        assert_eq!(config.candle.max_new_tokens, DEFAULT_CANDLE_MAX_NEW_TOKENS);
    }

    #[test]
    fn parse_candle_chat_request_wraps_messages_in_chatml() {
        let payload = json!({
            "messages": [
                {"role": "user", "content": "请只回答“是”或“否”：摄像头能用于抓拍吗？"}
            ]
        });
        let request =
            parse_candle_chat_request(&serde_json::to_vec(&payload).unwrap(), &CandleConfig::default())
                .unwrap();
        assert!(request.prompt.contains("<|im_start|>system"));
        assert!(request.prompt.contains("<|im_start|>user"));
        assert!(request.prompt.contains("<|im_start|>assistant"));
        assert!(request.prompt.contains(CANDLE_OUTPUT_POLICY_PROMPT));
        assert_eq!(request.max_new_tokens, DEFAULT_CANDLE_MAX_NEW_TOKENS);
    }

    #[test]
    fn parse_candle_chat_request_supports_text_parts() {
        let payload = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "樱花"},
                        {"type": "text", "text": "更像植物还是工具？"}
                    ]
                }
            ],
            "max_tokens": 8,
            "temperature": 0.4
        });
        let request =
            parse_candle_chat_request(&serde_json::to_vec(&payload).unwrap(), &CandleConfig::default())
                .unwrap();
        assert!(request.prompt.contains("樱花\n更像植物还是工具？"));
        assert_eq!(request.max_new_tokens, 8);
        assert_eq!(request.temperature, 0.4);
    }

    #[test]
    fn parse_candle_embedding_request_accepts_string_and_array() {
        let single = parse_candle_embedding_request(
            &serde_json::to_vec(&json!({"input": "樱花相关文件"})).unwrap(),
        )
        .unwrap();
        assert_eq!(single.inputs, vec!["樱花相关文件"]);

        let array = parse_candle_embedding_request(
            &serde_json::to_vec(&json!({"input": ["樱花说明文档", "抓拍摄像头"]})).unwrap(),
        )
        .unwrap();
        assert_eq!(array.inputs, vec!["樱花说明文档", "抓拍摄像头"]);
    }

    #[test]
    fn candle_health_reports_degraded_when_runtime_is_missing() {
        let config = ModelApiConfig {
            backend: BackendKind::Candle,
            ..ModelApiConfig::default()
        };
        let mut candle = CandleConfig::default();
        candle.chat_model_id = std::env::temp_dir().display().to_string();
        candle.embedding_model_id = std::env::temp_dir().display().to_string();
        let backend = CandleBackend::new(candle);
        let report = backend.health(&config);
        assert!(!report.ready);
        assert_eq!(report.status, HEALTH_DEGRADED);
        assert_eq!(report.backend.kind, "candle");
        assert!(report
            .note
            .as_deref()
            .is_some_and(|note| note.contains("last load error")));
    }

    #[test]
    fn resolve_weight_files_from_index_keeps_unique_shards() {
        let temp_dir = std::env::temp_dir().join(format!("harborbeacon-index-test-{}", current_timestamp_ms()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let index_path = temp_dir.join("model.safetensors.index.json");
        std::fs::write(
            &index_path,
            serde_json::to_vec(&json!({
                "weight_map": {
                    "a": "model-00001-of-00002.safetensors",
                    "b": "model-00001-of-00002.safetensors",
                    "c": "model-00002-of-00002.safetensors"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let files = resolve_weight_files_from_index(&index_path, |relative| {
            Ok(temp_dir.join(relative))
        })
        .unwrap();
        assert_eq!(files.len(), 2);
    }

    fn short_answer_request(options: &[&str]) -> CandleChatRequest {
        CandleChatRequest {
            prompt: "prompt".to_string(),
            max_new_tokens: DEFAULT_CANDLE_MAX_NEW_TOKENS,
            temperature: DEFAULT_CANDLE_TEMPERATURE,
            short_answer_options: options.iter().map(|value| value.to_string()).collect(),
            short_answer_focus: None,
        }
    }

    #[test]
    fn sanitize_candle_completion_text_strips_think_blocks_and_preamble() {
        let raw = "\n<think>\n先想一想\n</think>\n\n分析：\n\n可以。";
        assert_eq!(
            sanitize_candle_completion_text(raw, &short_answer_request(&[])),
            "可以。"
        );
    }

    #[test]
    fn sanitize_candle_completion_text_strips_bare_think_tags_and_extra_blank_lines() {
        let raw = "\r\n<think>\r\n</think>\r\n\n\n答案：\n\n  当然可以。\n\n\n";
        assert_eq!(
            sanitize_candle_completion_text(raw, &short_answer_request(&[])),
            "当然可以。"
        );
    }

    #[test]
    fn sanitize_candle_completion_text_preserves_final_answer_content() {
        let raw = "最终答案：\n\n摄像头可以抓拍。";
        assert_eq!(
            sanitize_candle_completion_text(raw, &short_answer_request(&[])),
            "摄像头可以抓拍。"
        );
    }

    #[test]
    fn detect_short_answer_options_handles_yes_no_and_choices() {
        assert_eq!(
            detect_short_answer_options("请只回答“是”或“否”：摄像头能用于抓拍吗？"),
            vec!["是".to_string(), "否".to_string()]
        );
        assert_eq!(
            detect_short_answer_options("请只回答一个词：“樱花”更像植物、工具还是地点？"),
            vec!["植物".to_string(), "工具".to_string(), "地点".to_string()]
        );
        assert_eq!(
            detect_short_answer_options("请只回答一个词：在“录像”和“抓拍”里，哪一个更像持续动作？"),
            vec!["录像".to_string(), "抓拍".to_string()]
        );
    }

    #[test]
    fn detect_short_answer_focus_extracts_target_phrase_when_available() {
        assert_eq!(
            detect_short_answer_focus("请只回答一个词：在“录像”和“抓拍”里，哪一个更像持续动作？"),
            Some("持续动作".to_string())
        );
        assert_eq!(
            detect_short_answer_focus("请只回答一个词：“樱花”更像植物、工具还是地点？"),
            None
        );
    }

    #[test]
    fn sanitize_candle_completion_text_extracts_short_answer_from_verbose_response() {
        let raw = "<think>先比较一下</think>\n\n抓拍更像瞬时动作，录像更像持续动作。";
        let mut request = short_answer_request(&["录像", "抓拍"]);
        request.short_answer_focus = Some("持续动作".to_string());
        assert_eq!(sanitize_candle_completion_text(raw, &request), "录像");
    }

    #[test]
    fn sanitize_candle_completion_text_uses_domain_hint_for_record_vs_snapshot() {
        let raw = "抓拍";
        let mut request = short_answer_request(&["录像", "抓拍"]);
        request.short_answer_focus = Some("持续动作".to_string());
        assert_eq!(sanitize_candle_completion_text(raw, &request), "录像");
    }

    #[test]
    fn sanitize_candle_completion_text_normalizes_yes_no_synonyms() {
        let raw = "答案：当然可以。";
        assert_eq!(
            sanitize_candle_completion_text(raw, &short_answer_request(&["是", "否"])),
            "是"
        );
    }
}
