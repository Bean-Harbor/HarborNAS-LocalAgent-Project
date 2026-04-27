//! HarborBeacon-local knowledge retrieval over NAS-backed documents and images.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::control_plane::models::{
    ModelEndpointKind, ModelEndpointStatus, ModelKind, PrivacyLevel,
};
use crate::runtime::admin_console::{
    path_is_same_or_inside, AdminModelCenterState, RagResourceProfile,
};
use crate::runtime::knowledge_index::{
    load_embedding_store, save_embedding_store, KnowledgeEmbeddingEntry, KnowledgeEmbeddingStore,
    KnowledgeIndexChunk, KnowledgeIndexConfig, KnowledgeIndexEntry, KnowledgeIndexService,
    KnowledgeModality,
};
use crate::runtime::model_center;

const DEFAULT_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSearchRequest {
    pub query: String,
    pub configured_roots: Vec<String>,
    pub index_root: Option<String>,
    pub roots: Vec<String>,
    pub include_documents: bool,
    pub include_images: bool,
    pub limit: usize,
    pub privacy_level: PrivacyLevel,
    pub resource_profile: RagResourceProfile,
    pub require_embeddings: bool,
    pub latency_budget_ms: Option<u64>,
}

impl KnowledgeSearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            configured_roots: Vec::new(),
            index_root: None,
            roots: Vec::new(),
            include_documents: true,
            include_images: true,
            limit: DEFAULT_LIMIT,
            privacy_level: PrivacyLevel::StrictLocal,
            resource_profile: RagResourceProfile::CpuOnly,
            require_embeddings: false,
            latency_budget_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeSearchHit {
    pub modality: String,
    pub path: String,
    pub title: String,
    pub score: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lexical_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hybrid_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_end: Option<usize>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub matched_terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeSearchCitation {
    pub title: String,
    pub path: String,
    pub modality: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_end: Option<usize>,
    #[serde(default)]
    pub matched_terms: Vec<String>,
    #[serde(default)]
    pub preview: Option<String>,
    pub score: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lexical_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hybrid_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KnowledgeSearchReplyPack {
    pub summary: String,
    #[serde(default)]
    pub citations: Vec<KnowledgeSearchCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeSearchResponse {
    pub query: String,
    pub roots: Vec<String>,
    pub total_matches: usize,
    #[serde(default)]
    pub documents: Vec<KnowledgeSearchHit>,
    #[serde(default)]
    pub images: Vec<KnowledgeSearchHit>,
    #[serde(default)]
    pub reply_pack: KnowledgeSearchReplyPack,
    #[serde(default)]
    pub supported_modalities: Vec<String>,
    #[serde(default)]
    pub pending_modalities: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub source_scope: Vec<String>,
    #[serde(default)]
    pub privacy_level: String,
    #[serde(default)]
    pub resource_profile: String,
}

impl KnowledgeSearchResponse {
    pub fn degraded(
        query: impl Into<String>,
        roots: Vec<String>,
        privacy_level: PrivacyLevel,
        resource_profile: RagResourceProfile,
        reason: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let query = query.into();
        let roots = normalize_scope_strings(roots);
        let reason = reason.into();
        let message = message.into();
        let (supported_modalities, pending_modalities) = modality_support_matrix();
        Self {
            query,
            roots: roots.clone(),
            total_matches: 0,
            documents: Vec::new(),
            images: Vec::new(),
            reply_pack: KnowledgeSearchReplyPack {
                summary: message.clone(),
                citations: Vec::new(),
            },
            supported_modalities,
            pending_modalities,
            status: "degraded".to_string(),
            degraded: true,
            degraded_reason: Some(reason),
            blockers: vec![message],
            warnings: Vec::new(),
            source_scope: roots,
            privacy_level: privacy_level_as_str(privacy_level).to_string(),
            resource_profile: resource_profile.as_str().to_string(),
        }
    }
}

pub struct KnowledgeSearchService;

#[derive(Debug, Clone)]
struct SearchCandidate {
    hit: KnowledgeSearchHit,
    embedding_text: String,
}

impl KnowledgeSearchService {
    pub fn search(request: KnowledgeSearchRequest) -> Result<KnowledgeSearchResponse, String> {
        let query = request.query.trim().to_string();
        if query.is_empty() {
            return Err(
                "缺少知识库检索关键词，请提供 query 或更明确的自然语言检索请求。".to_string(),
            );
        }
        if !request.include_documents && !request.include_images {
            return Ok(KnowledgeSearchResponse::degraded(
                query,
                Vec::new(),
                request.privacy_level,
                request.resource_profile,
                "unsupported_modalities",
                "当前检索请求没有启用可支持的模态，至少需要文档或图片之一。",
            ));
        }

        if let Some(blocker) = request_policy_blocker(&request) {
            return Ok(KnowledgeSearchResponse::degraded(
                query,
                Vec::new(),
                request.privacy_level,
                request.resource_profile,
                "blocked_resource_profile",
                blocker,
            ));
        }

        let roots = match resolve_roots(&request.configured_roots, &request.roots) {
            Ok(roots) => roots,
            Err(error) => {
                return Ok(KnowledgeSearchResponse::degraded(
                    query,
                    Vec::new(),
                    request.privacy_level,
                    request.resource_profile,
                    "source_scope_blocked",
                    error,
                ))
            }
        };
        let root_strings = roots
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let query_terms = build_query_terms(&query);
        let index_service = match knowledge_index_service(request.index_root.as_deref()) {
            Ok(service) => service,
            Err(error) => {
                return Ok(KnowledgeSearchResponse::degraded(
                    query,
                    root_strings,
                    request.privacy_level,
                    request.resource_profile,
                    "index_root_unavailable",
                    error,
                ))
            }
        };
        let model_center_state = model_center::load_model_center_state();
        if let Some(blocker) = resource_profile_runtime_blocker(
            request.resource_profile,
            request.privacy_level,
            &model_center_state,
        ) {
            return Ok(KnowledgeSearchResponse::degraded(
                query,
                root_strings,
                request.privacy_level,
                request.resource_profile,
                "blocked_resource_profile",
                blocker,
            ));
        }
        let query_embedding = model_center::run_embedding_with_state(&query, &model_center_state);
        let query_embedding_vector =
            (!query_embedding.vector.is_empty() && query_embedding.available)
                .then_some(query_embedding.vector.clone());
        if request.require_embeddings && query_embedding_vector.is_none() {
            return Ok(KnowledgeSearchResponse::degraded(
                query,
                root_strings,
                request.privacy_level,
                request.resource_profile,
                "embedding_unavailable",
                format!(
                    "当前检索要求 embedding，但 embedding 模型不可用：{}",
                    query_embedding.summary
                ),
            ));
        }
        let mut warnings = Vec::new();
        if query_embedding_vector.is_none() {
            warnings.push(format!(
                "Embedding 模型不可用，已降级为本地词法检索：{}",
                query_embedding.summary
            ));
        }
        let mut seen_hits = HashSet::new();

        let mut documents = Vec::new();
        let mut images = Vec::new();
        for root in &roots {
            let snapshot = match index_service.load_existing(root) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return Ok(KnowledgeSearchResponse::degraded(
                        query,
                        root_strings,
                        request.privacy_level,
                        request.resource_profile,
                        "index_manifest_unavailable",
                        error,
                    ))
                }
            };
            let embedding_store_path = index_service.embedding_store_path_for_root(root);
            let mut embedding_store = if query_embedding_vector.is_some() {
                match load_embedding_store(&embedding_store_path) {
                    Ok(store) => store,
                    Err(error) => {
                        warnings.push(format!(
                            "Embedding cache 读取失败，已继续使用词法分数：{error}"
                        ));
                        KnowledgeEmbeddingStore::default()
                    }
                }
            } else {
                KnowledgeEmbeddingStore::default()
            };
            if embedding_store.schema_version == 0 {
                embedding_store.schema_version = 1;
            }
            embedding_store.root = root.to_string_lossy().into_owned();
            embedding_store.provider_key = (!query_embedding.provider_key.trim().is_empty())
                .then_some(query_embedding.provider_key.clone());
            embedding_store.model_endpoint_id = query_embedding.model_endpoint_id.clone();
            embedding_store.model_name = query_embedding.model_name.clone();
            let mut embedding_store_dirty = false;
            for entry in &snapshot.manifest.entries {
                let modality = entry.modality;
                match modality {
                    KnowledgeModality::Document if request.include_documents => {
                        for mut candidate in build_hit_candidates_from_index_entry(entry, &query_terms)
                        {
                            apply_hybrid_scores(
                                &mut candidate,
                                query_embedding_vector.as_deref(),
                                &model_center_state,
                                &mut embedding_store,
                                &mut embedding_store_dirty,
                            );
                            let hit = candidate.hit;
                            let dedupe_key = (
                                hit.modality.clone(),
                                hit.path.clone(),
                                hit.chunk_id.clone().unwrap_or_default(),
                            );
                            if seen_hits.insert(dedupe_key) {
                                documents.push(hit);
                            }
                        }
                    }
                    KnowledgeModality::Image if request.include_images => {
                        for mut candidate in build_hit_candidates_from_index_entry(entry, &query_terms)
                        {
                            apply_hybrid_scores(
                                &mut candidate,
                                query_embedding_vector.as_deref(),
                                &model_center_state,
                                &mut embedding_store,
                                &mut embedding_store_dirty,
                            );
                            let hit = candidate.hit;
                            let dedupe_key = (
                                hit.modality.clone(),
                                hit.path.clone(),
                                hit.chunk_id.clone().unwrap_or_default(),
                            );
                            if seen_hits.insert(dedupe_key) {
                                images.push(hit);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if embedding_store_dirty {
                if let Err(error) = save_embedding_store(&embedding_store_path, &embedding_store) {
                    return Ok(KnowledgeSearchResponse::degraded(
                        query,
                        root_strings,
                        request.privacy_level,
                        request.resource_profile,
                        "embedding_cache_write_failed",
                        error,
                    ));
                }
            }
        }

        sort_hits(&mut documents);
        sort_hits(&mut images);

        let total_matches = documents.len() + images.len();
        let limit = request.limit.clamp(1, 10);
        documents.truncate(limit);
        images.truncate(limit);
        let reply_pack = build_reply_pack(&query, total_matches, &documents, &images);
        let (supported_modalities, pending_modalities) = modality_support_matrix();

        Ok(KnowledgeSearchResponse {
            query,
            roots: root_strings.clone(),
            total_matches,
            documents,
            images,
            reply_pack,
            supported_modalities,
            pending_modalities,
            status: if warnings.is_empty() {
                "completed".to_string()
            } else {
                "degraded".to_string()
            },
            degraded: !warnings.is_empty(),
            degraded_reason: (!warnings.is_empty()).then(|| "embedding_unavailable".to_string()),
            blockers: Vec::new(),
            warnings,
            source_scope: root_strings,
            privacy_level: privacy_level_as_str(request.privacy_level).to_string(),
            resource_profile: request.resource_profile.as_str().to_string(),
        })
    }
}

fn request_policy_blocker(request: &KnowledgeSearchRequest) -> Option<String> {
    if request.resource_profile == RagResourceProfile::CloudAllowed
        && request.privacy_level == PrivacyLevel::StrictLocal
    {
        return Some(
            "resource_profile=cloud_allowed 与 workspace strict_local 隐私策略冲突；请先在 HarborDesk 明确启用可审计的云策略。"
                .to_string(),
        );
    }
    if let Some(budget) = request.latency_budget_ms {
        if budget == 0 {
            return Some("latency_budget_ms 必须大于 0，不能静默回退到无预算检索。".to_string());
        }
    }
    None
}

fn resource_profile_runtime_blocker(
    resource_profile: RagResourceProfile,
    privacy_level: PrivacyLevel,
    model_center_state: &AdminModelCenterState,
) -> Option<String> {
    match resource_profile {
        RagResourceProfile::CpuOnly | RagResourceProfile::LocalGpu => None,
        RagResourceProfile::SidecarGpu => {
            if endpoint_kind_available(model_center_state, ModelEndpointKind::Sidecar) {
                None
            } else {
                Some(
                    "resource_profile=sidecar_gpu 需要可用的 sidecar 模型端点；当前模型设置未通过 readiness。"
                        .to_string(),
                )
            }
        }
        RagResourceProfile::CloudAllowed => {
            if privacy_level == PrivacyLevel::StrictLocal {
                Some(
                    "resource_profile=cloud_allowed 与 strict_local 隐私策略冲突。".to_string(),
                )
            } else if endpoint_kind_available(model_center_state, ModelEndpointKind::Cloud) {
                None
            } else {
                Some(
                    "resource_profile=cloud_allowed 需要可用的 cloud 模型端点；当前模型设置未通过 readiness。"
                        .to_string(),
                )
            }
        }
    }
}

fn endpoint_kind_available(
    model_center_state: &AdminModelCenterState,
    endpoint_kind: ModelEndpointKind,
) -> bool {
    model_center_state.endpoints.iter().any(|endpoint| {
        endpoint.endpoint_kind == endpoint_kind
            && endpoint.status != ModelEndpointStatus::Disabled
            && matches!(
                endpoint.model_kind,
                ModelKind::Embedder | ModelKind::Llm | ModelKind::Ocr | ModelKind::Vlm
            )
    })
}

fn privacy_level_as_str(level: PrivacyLevel) -> &'static str {
    match level {
        PrivacyLevel::StrictLocal => "strict_local",
        PrivacyLevel::AllowRedactedCloud => "allow_redacted_cloud",
        PrivacyLevel::AllowCloud => "allow_cloud",
    }
}

fn normalize_scope_strings(mut roots: Vec<String>) -> Vec<String> {
    roots.iter_mut().for_each(|root| *root = root.trim().to_string());
    roots.retain(|root| !root.is_empty());
    roots.sort();
    roots.dedup();
    roots
}

fn modality_support_matrix() -> (Vec<String>, Vec<String>) {
    let mut supported = vec![
        "document".to_string(),
        "image".to_string(),
        "ocr".to_string(),
    ];
    let mut pending = vec!["audio".to_string(), "video".to_string()];

    let model_center_state = model_center::load_model_center_state();
    let embed_ready = model_center_state.endpoints.iter().any(|endpoint| {
        endpoint.model_kind == ModelKind::Embedder
            && endpoint.status != ModelEndpointStatus::Disabled
    });
    if embed_ready {
        supported.push("embedding".to_string());
        supported.push("hybrid_retrieval".to_string());
    } else {
        pending.push("embedding".to_string());
        pending.push("hybrid_retrieval".to_string());
    }
    let vlm_ready = model_center_state.endpoints.iter().any(|endpoint| {
        endpoint.model_kind == ModelKind::Vlm && endpoint.status != ModelEndpointStatus::Disabled
    });
    if vlm_ready {
        supported.push("vlm".to_string());
    } else {
        pending.push("vlm".to_string());
    }

    (supported, pending)
}

fn knowledge_index_service(index_root: Option<&str>) -> Result<KnowledgeIndexService, String> {
    let Some(index_root) = index_root.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }) else {
        return Err("请先在 HarborDesk 配置 knowledge.index_root，再运行知识库检索。".to_string());
    };
    KnowledgeIndexService::from_config(KnowledgeIndexConfig::new(PathBuf::from(index_root))?)
}

fn resolve_roots(configured_roots: &[String], request_roots: &[String]) -> Result<Vec<PathBuf>, String> {
    let configured = configured_roots
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if configured.is_empty() {
        return Err("请先在 HarborDesk 配置并启用至少一个知识源目录。".to_string());
    }

    let requested = request_roots
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    let raw_roots = if requested.is_empty() {
        configured.clone()
    } else {
        let mut allowed = Vec::new();
        for requested_root in requested {
            let inside_configured = configured
                .iter()
                .any(|configured_root| path_is_same_or_inside(&requested_root, configured_root));
            if !inside_configured {
                return Err(format!(
                    "请求的知识源目录未在 HarborDesk 启用，不能扩权检索：{requested_root}"
                ));
            }
            allowed.push(requested_root);
        }
        allowed
    };

    let mut roots = Vec::new();
    for root in raw_roots {
        let root = PathBuf::from(root);
        if root.as_os_str().is_empty() {
            continue;
        }
        if root.exists() {
            roots.push(root.canonicalize().unwrap_or(root));
        }
    }

    if roots.is_empty() {
        return Err("未找到可检索的已配置知识源目录；请先通过 HarborDesk 配置并确认目录存在。".to_string());
    }

    roots.sort();
    roots.dedup();
    Ok(roots)
}

/// Build a HarborBeacon-owned hit from an indexed entry, preserving the stable
/// response shape used by `TaskResponse.result.data`.
fn build_hit_candidates_from_index_entry(
    entry: &KnowledgeIndexEntry,
    query_terms: &[String],
) -> Vec<SearchCandidate> {
    let path = Path::new(&entry.path);
    let chunks = if entry.chunks.is_empty() {
        vec![KnowledgeIndexChunk {
            chunk_id: "chunk-0001".to_string(),
            line_start: 1,
            line_end: entry.searchable_text.lines().count().max(1),
            text: entry.searchable_text.clone(),
            source_kind: entry.modality.as_str().to_string(),
            source_path: entry.sidecar_path.clone(),
        }]
    } else {
        entry.chunks.clone()
    };

    chunks
        .iter()
        .filter_map(|chunk| {
            build_hit_candidate(
                path,
                entry.modality,
                Some(chunk.text.as_str()),
                query_terms,
                Some(chunk),
            )
        })
        .collect()
}

fn build_reply_pack(
    query: &str,
    total_matches: usize,
    documents: &[KnowledgeSearchHit],
    images: &[KnowledgeSearchHit],
) -> KnowledgeSearchReplyPack {
    let citations = documents
        .iter()
        .chain(images.iter())
        .map(|hit| KnowledgeSearchCitation {
            title: hit.title.clone(),
            path: hit.path.clone(),
            modality: hit.modality.clone(),
            chunk_id: hit.chunk_id.clone(),
            line_start: hit.line_start,
            line_end: hit.line_end,
            matched_terms: hit.matched_terms.clone(),
            preview: hit.snippet.clone(),
            score: hit.score,
            lexical_score: hit.lexical_score,
            embedding_score: hit.embedding_score,
            hybrid_score: hit.hybrid_score,
            provenance: hit.provenance.clone(),
            source_path: hit.source_path.clone(),
        })
        .collect::<Vec<_>>();
    let summary = build_reply_summary(query, total_matches, documents, images);
    KnowledgeSearchReplyPack { summary, citations }
}

fn build_reply_summary(
    query: &str,
    total_matches: usize,
    documents: &[KnowledgeSearchHit],
    images: &[KnowledgeSearchHit],
) -> String {
    if total_matches == 0 {
        return format!(
            "已检索知识库，但暂时没有找到与“{}”相关的文档、图片或 OCR 线索。",
            query
        );
    }

    let mut parts = Vec::new();
    if !documents.is_empty() {
        parts.push(format!("{} 个文档片段", documents.len()));
    }
    if !images.is_empty() {
        parts.push(format!("{} 张图片", images.len()));
    }
    let visible = documents.len() + images.len();
    if visible < total_matches {
        format!(
            "已找到与“{}”相关的 {}，当前展示 {} 条可引用结果。",
            query,
            parts.join("和"),
            visible
        )
    } else {
        format!("已找到与“{}”相关的 {}。", query, parts.join("和"))
    }
}

fn build_hit_candidate(
    path: &Path,
    modality: KnowledgeModality,
    searchable_text: Option<&str>,
    query_terms: &[String],
    chunk: Option<&KnowledgeIndexChunk>,
) -> Option<SearchCandidate> {
    let display_path = path.to_string_lossy().into_owned();
    let title = path
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or(display_path.as_str())
        .to_string();
    let path_lower = display_path.to_lowercase();
    let title_lower = title.to_lowercase();
    let searchable_lower = searchable_text.map(str::to_lowercase);

    let mut score = 0;
    let mut matched_terms = Vec::new();
    for term in query_terms {
        let normalized = term.to_lowercase();
        let mut matched = false;
        if title_lower.contains(&normalized) {
            score += 32;
            matched = true;
        } else if path_lower.contains(&normalized) {
            score += 18;
            matched = true;
        }
        if let Some(text) = searchable_lower.as_ref() {
            if text.contains(&normalized) {
                score += match modality {
                    KnowledgeModality::Document => 24,
                    KnowledgeModality::Image => 20,
                    KnowledgeModality::Audio => 18,
                    KnowledgeModality::Video => 18,
                };
                matched = true;
            }
        }
        if matched {
            matched_terms.push(term.clone());
        }
    }
    matched_terms.sort();
    matched_terms.dedup();

    if score == 0 {
        return None;
    }

    let lexical_score = (score as f32 / 100.0).clamp(0.0, 1.0);

    Some(SearchCandidate {
        embedding_text: searchable_text.unwrap_or_default().to_string(),
        hit: KnowledgeSearchHit {
            modality: modality.as_str().to_string(),
            path: display_path,
            title,
            score,
            lexical_score: Some(lexical_score),
            embedding_score: None,
            hybrid_score: Some(lexical_score),
            chunk_id: chunk.map(|item| item.chunk_id.clone()),
            line_start: chunk.map(|item| item.line_start),
            line_end: chunk.map(|item| item.line_end),
            snippet: searchable_text.and_then(|text| build_snippet(text, &matched_terms)),
            matched_terms,
            provenance: chunk
                .map(|item| item.source_kind.clone())
                .filter(|value| !value.trim().is_empty()),
            source_path: chunk.and_then(|item| item.source_path.clone()),
        },
    })
}

fn apply_hybrid_scores(
    candidate: &mut SearchCandidate,
    query_embedding: Option<&[f32]>,
    model_center_state: &AdminModelCenterState,
    embedding_store: &mut KnowledgeEmbeddingStore,
    embedding_store_dirty: &mut bool,
) {
    let lexical_score = candidate.hit.lexical_score.unwrap_or_default();
    let mut embedding_score = None;

    if let Some(query_vector) = query_embedding {
        if let Some(chunk_vector) = embedding_vector_for_candidate(
            candidate,
            model_center_state,
            embedding_store,
            embedding_store_dirty,
        ) {
            embedding_score = Some(cosine_similarity(query_vector, &chunk_vector).clamp(0.0, 1.0));
        }
    }

    let hybrid_score = match embedding_score {
        Some(value) => 0.55 * lexical_score + 0.45 * value,
        None => lexical_score,
    };

    candidate.hit.embedding_score = embedding_score;
    candidate.hit.hybrid_score = Some(hybrid_score);
    candidate.hit.score = (hybrid_score * 1000.0).round() as u32;
}

fn embedding_vector_for_candidate(
    candidate: &SearchCandidate,
    model_center_state: &AdminModelCenterState,
    embedding_store: &mut KnowledgeEmbeddingStore,
    embedding_store_dirty: &mut bool,
) -> Option<Vec<f32>> {
    let text = candidate.embedding_text.trim();
    if text.is_empty() {
        return None;
    }
    let key = embedding_key(&candidate.hit.path, candidate.hit.chunk_id.as_deref());
    let text_hash = text_hash(text);
    if let Some(entry) = embedding_store
        .entries
        .iter()
        .find(|entry| entry.key == key && entry.text_hash == text_hash && !entry.vector.is_empty())
    {
        return Some(entry.vector.clone());
    }

    let execution = model_center::run_embedding_with_state(text, model_center_state);
    if !execution.available || execution.vector.is_empty() {
        return None;
    }

    let vector = execution.vector.clone();
    embedding_store.provider_key = (!execution.provider_key.trim().is_empty())
        .then_some(execution.provider_key.clone());
    embedding_store.model_endpoint_id = execution.model_endpoint_id.clone();
    embedding_store.model_name = execution.model_name.clone();

    if let Some(existing) = embedding_store.entries.iter_mut().find(|entry| entry.key == key) {
        existing.text_hash = text_hash;
        existing.vector = vector.clone();
        existing.path = candidate.hit.path.clone();
        existing.chunk_id = candidate.hit.chunk_id.clone();
    } else {
        embedding_store.entries.push(KnowledgeEmbeddingEntry {
            key,
            path: candidate.hit.path.clone(),
            chunk_id: candidate.hit.chunk_id.clone(),
            text_hash,
            vector: vector.clone(),
        });
    }
    *embedding_store_dirty = true;
    Some(vector)
}

fn embedding_key(path: &str, chunk_id: Option<&str>) -> String {
    format!("{}::{}", path, chunk_id.unwrap_or("chunk-0001"))
}

fn text_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut left_norm = 0.0f32;
    let mut right_norm = 0.0f32;
    for (lhs, rhs) in left.iter().zip(right.iter()) {
        dot += lhs * rhs;
        left_norm += lhs * lhs;
        right_norm += rhs * rhs;
    }
    if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
        0.0
    } else {
        dot / (left_norm.sqrt() * right_norm.sqrt())
    }
}

fn sort_hits(hits: &mut [KnowledgeSearchHit]) {
    hits.sort_by(|left, right| {
        right
            .hybrid_score
            .unwrap_or_default()
            .total_cmp(&left.hybrid_score.unwrap_or_default())
            .then_with(|| {
                right
                    .lexical_score
                    .unwrap_or_default()
                    .total_cmp(&left.lexical_score.unwrap_or_default())
            })
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| left.line_start.cmp(&right.line_start))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn build_query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut ascii = String::new();
    let mut cjk = String::new();

    for ch in query.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            if !cjk.is_empty() {
                push_cjk_terms(&cjk, &mut terms);
                cjk.clear();
            }
            ascii.push(ch);
            continue;
        }
        if !ascii.is_empty() {
            push_ascii_term(&ascii, &mut terms);
            ascii.clear();
        }
        if is_cjk(ch) {
            cjk.push(ch);
        } else if !cjk.is_empty() {
            push_cjk_terms(&cjk, &mut terms);
            cjk.clear();
        }
    }

    if !ascii.is_empty() {
        push_ascii_term(&ascii, &mut terms);
    }
    if !cjk.is_empty() {
        push_cjk_terms(&cjk, &mut terms);
    }

    if terms.is_empty() {
        let fallback = query.trim().to_string();
        if !fallback.is_empty() {
            terms.push(fallback);
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

fn push_ascii_term(term: &str, terms: &mut Vec<String>) {
    let value = term.trim();
    if value.len() >= 2 && !is_stop_term(value) {
        terms.push(value.to_string());
    }
}

fn push_cjk_terms(term: &str, terms: &mut Vec<String>) {
    let chars = term.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return;
    }

    let full = chars.iter().collect::<String>();
    if !is_stop_term(&full) {
        terms.push(full.clone());
    }

    if chars.len() < 2 {
        return;
    }

    for window in chars.windows(2) {
        let token = window.iter().collect::<String>();
        if !is_stop_term(&token) {
            terms.push(token);
        }
    }
}

fn is_stop_term(term: &str) -> bool {
    matches!(
        term.trim(),
        "文件"
            | "文档"
            | "图片"
            | "照片"
            | "资料"
            | "内容"
            | "搜索"
            | "检索"
            | "查找"
            | "找到"
            | "相关"
            | "有关"
            | "search"
            | "find"
            | "files"
            | "file"
            | "image"
            | "images"
            | "photo"
            | "photos"
            | "document"
            | "documents"
    )
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF
    )
}

fn build_snippet(text: &str, matched_terms: &[String]) -> Option<String> {
    let lowercase = text.to_lowercase();
    let first_match = matched_terms
        .iter()
        .filter_map(|term| lowercase.find(&term.to_lowercase()))
        .min()?;
    let start = clamp_to_char_boundary(text, first_match.saturating_sub(24));
    let end = clamp_to_char_boundary(text, (first_match + 72).min(text.len()));
    let snippet = text[start..end]
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string();
    (!snippet.is_empty()).then_some(snippet)
}

fn clamp_to_char_boundary(text: &str, index: usize) -> usize {
    let mut candidate = index.min(text.len());
    while candidate > 0 && !text.is_char_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{json, Value};

    use crate::control_plane::models::{
        ModelEndpoint, ModelEndpointKind, ModelEndpointStatus, ModelKind, ModelRoutePolicy,
        PrivacyLevel,
    };
    use crate::runtime::admin_console::{AdminConsoleState, AdminModelCenterState};

    use super::{
        KnowledgeIndexConfig, KnowledgeIndexService, KnowledgeSearchRequest, KnowledgeSearchService,
    };

    static INDEX_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn unique_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }

    fn cleanup_dir(path: &Path) {
        if path.exists() {
            let _ = fs::remove_dir_all(path);
        }
    }

    fn build_search_index(root: &Path, index_root: &Path) {
        KnowledgeIndexService::from_config(
            KnowledgeIndexConfig::new(index_root.to_path_buf()).expect("knowledge index config"),
        )
        .expect("knowledge index service")
        .load_or_refresh(root)
        .expect("build knowledge index");
    }

    #[test]
    fn search_requires_existing_index_manifest_instead_of_refreshing() {
        let _guard = INDEX_TEST_LOCK.lock().expect("lock");
        let root = unique_dir("harborbeacon-knowledge-no-manifest");
        let index_root = unique_dir("harborbeacon-knowledge-index-no-manifest");
        fs::create_dir_all(root.join("docs")).expect("create docs");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(root.join("docs").join("sakura.md"), "樱花计划").expect("write doc");

        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "樱花".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: true,
            include_images: false,
            limit: 5,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("knowledge search");

        assert!(response.degraded);
        assert_eq!(
            response.degraded_reason.as_deref(),
            Some("index_manifest_unavailable")
        );
        assert_eq!(response.total_matches, 0);
        assert!(response
            .blockers
            .iter()
            .any(|item| item.contains("/api/knowledge/index/run")));
        assert!(!index_root
            .read_dir()
            .expect("list index root")
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".json")));

        cleanup_dir(&root);
        cleanup_dir(&index_root);
    }

    fn write_mock_model_center_state(
        path: &Path,
        mock_ocr_text: &str,
        mock_vlm_text: Option<&str>,
    ) {
        let mut endpoints = vec![ModelEndpoint {
            model_endpoint_id: "ocr-mock".to_string(),
            workspace_id: Some("home-1".to_string()),
            provider_account_id: None,
            model_kind: ModelKind::Ocr,
            endpoint_kind: ModelEndpointKind::Local,
            provider_key: "tesseract".to_string(),
            model_name: "mock-ocr".to_string(),
            capability_tags: vec!["ocr".to_string()],
            cost_policy: json!({}),
            status: ModelEndpointStatus::Active,
            metadata: json!({
                "mock_text": mock_ocr_text,
            }),
        }];
        let mut route_policies = vec![ModelRoutePolicy {
            route_policy_id: "retrieval.ocr".to_string(),
            workspace_id: "home-1".to_string(),
            domain_scope: "retrieval".to_string(),
            modality: "image".to_string(),
            privacy_level: PrivacyLevel::StrictLocal,
            local_preferred: true,
            max_cost_per_run: None,
            fallback_order: vec!["local".to_string(), "cloud".to_string()],
            status: "active".to_string(),
            metadata: json!({}),
        }];
        if let Some(mock_vlm_text) = mock_vlm_text {
            endpoints.push(ModelEndpoint {
                model_endpoint_id: "vlm-mock".to_string(),
                workspace_id: Some("home-1".to_string()),
                provider_account_id: None,
                model_kind: ModelKind::Vlm,
                endpoint_kind: ModelEndpointKind::Local,
                provider_key: "openai_compatible".to_string(),
                model_name: "mock-vlm".to_string(),
                capability_tags: vec!["vlm".to_string(), "multimodal".to_string()],
                cost_policy: json!({}),
                status: ModelEndpointStatus::Active,
                metadata: json!({
                    "mock_text": mock_vlm_text,
                }),
            });
            route_policies.push(ModelRoutePolicy {
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
            });
        }
        let state = AdminConsoleState {
            models: AdminModelCenterState {
                endpoints,
                route_policies,
            },
            ..AdminConsoleState::default()
        };
        fs::write(
            path,
            serde_json::to_vec_pretty(&state).expect("serialize admin state"),
        )
        .expect("write admin state");
    }

    fn write_mock_model_center_state_with_embed(
        path: &Path,
        mock_embeddings: Value,
    ) {
        let state = AdminConsoleState {
            models: AdminModelCenterState {
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
                        "mock_embeddings": mock_embeddings,
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
            },
            ..AdminConsoleState::default()
        };
        fs::write(
            path,
            serde_json::to_vec_pretty(&state).expect("serialize admin state"),
        )
        .expect("write admin state");
    }

    #[test]
    fn search_returns_document_and_image_matches() {
        let _guard = INDEX_TEST_LOCK.lock().expect("lock");
        let root = unique_dir("harborbeacon-knowledge");
        let index_root = unique_dir("harborbeacon-knowledge-index-store");
        fs::create_dir_all(root.join("docs")).expect("create docs");
        fs::create_dir_all(root.join("images")).expect("create images");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(
            root.join("docs").join("spring-sakura.md"),
            "今年花园里的樱花开得很盛，适合做春季归档。",
        )
        .expect("write doc");
        fs::write(root.join("images").join("garden.jpg"), b"not-really-a-jpeg")
            .expect("write image");
        fs::write(
            root.join("images").join("garden.json"),
            r#"{"caption":"花园里的樱花树","tags":["spring","sakura"]}"#,
        )
        .expect("write sidecar");
        build_search_index(&root, &index_root);

        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "樱花".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: true,
            include_images: true,
            limit: 5,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("knowledge search");

        assert_eq!(response.total_matches, 2);
        assert_eq!(response.documents.len(), 1);
        assert_eq!(response.images.len(), 1);
        assert!(response.documents[0].path.ends_with("spring-sakura.md"));
        assert!(response.images[0].path.ends_with("garden.jpg"));
        assert_eq!(response.images[0].matched_terms, vec!["樱花".to_string()]);
        assert_eq!(response.reply_pack.citations.len(), 2);
        assert_eq!(response.reply_pack.citations[0].title, "spring-sakura.md");
        assert_eq!(response.reply_pack.citations[0].modality, "document");
        assert!(response.reply_pack.citations[0]
            .preview
            .as_deref()
            .unwrap_or_default()
            .contains("樱花"));
        assert!(response.reply_pack.citations[0].chunk_id.is_some());
        assert_eq!(response.reply_pack.citations[0].line_start, Some(1));
        assert_eq!(response.reply_pack.citations[1].title, "garden.jpg");
        assert_eq!(response.reply_pack.citations[1].modality, "image");

        cleanup_dir(&root);
        cleanup_dir(&index_root);
    }

    #[test]
    fn search_returns_chunk_grounded_document_snippet() {
        let _guard = INDEX_TEST_LOCK.lock().expect("lock");
        let root = unique_dir("harborbeacon-knowledge-rag");
        let index_root = unique_dir("harborbeacon-knowledge-index-store");
        fs::create_dir_all(root.join("docs")).expect("create docs");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(
            root.join("docs").join("multi-section.md"),
            "第一段是背景介绍。\n第二段仍然是背景。\n第三段继续铺垫。\n第四段保持上下文。\n第五段明确提到樱花季文档整理与引用。\n第六段补充引用来源。",
        )
        .expect("write doc");
        build_search_index(&root, &index_root);

        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "樱花".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: true,
            include_images: false,
            limit: 5,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("knowledge search");

        assert_eq!(response.documents.len(), 1);
        let hit = &response.documents[0];
        assert_eq!(hit.title, "multi-section.md");
        assert_eq!(hit.chunk_id.as_deref(), Some("chunk-0002"));
        assert_eq!(hit.line_start, Some(5));
        assert_eq!(hit.line_end, Some(6));
        assert!(hit
            .snippet
            .as_deref()
            .unwrap_or_default()
            .contains("樱花季"));
        assert_eq!(response.reply_pack.citations.len(), 1);
        assert_eq!(
            response.reply_pack.citations[0].chunk_id.as_deref(),
            Some("chunk-0002")
        );
        assert_eq!(response.reply_pack.citations[0].line_start, Some(5));
        assert_eq!(response.reply_pack.citations[0].line_end, Some(6));
        assert!(response.reply_pack.citations[0]
            .preview
            .as_deref()
            .unwrap_or_default()
            .contains("樱花季"));

        cleanup_dir(&root);
        cleanup_dir(&index_root);
    }

    #[test]
    fn search_deduplicates_repeated_roots_and_keeps_stable_order() {
        let _guard = INDEX_TEST_LOCK.lock().expect("lock");
        let root = unique_dir("harborbeacon-knowledge-dedupe");
        let index_root = unique_dir("harborbeacon-knowledge-index-store");
        fs::create_dir_all(root.join("docs")).expect("create docs");
        fs::create_dir_all(root.join("images")).expect("create images");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(
            root.join("docs").join("alpha-note.md"),
            "alpha note about spring",
        )
        .expect("doc");
        fs::write(
            root.join("docs").join("beta-note.md"),
            "beta note about spring",
        )
        .expect("doc");
        fs::write(root.join("images").join("alpha.png"), b"image").expect("image");
        fs::write(
            root.join("images").join("alpha.json"),
            r#"{"caption":"alpha spring view"}"#,
        )
        .expect("sidecar");
        build_search_index(&root, &index_root);

        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "spring".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![
                root.to_string_lossy().into_owned(),
                root.to_string_lossy().into_owned(),
            ],
            include_documents: true,
            include_images: true,
            limit: 10,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("knowledge search");

        assert_eq!(response.documents.len(), 2);
        assert_eq!(response.images.len(), 1);
        assert_eq!(response.total_matches, 3);
        assert_eq!(response.documents[0].title, "alpha-note.md");
        assert_eq!(response.documents[1].title, "beta-note.md");
        assert_eq!(response.images[0].title, "alpha.png");
        assert_eq!(response.reply_pack.citations.len(), 3);
        assert_eq!(response.reply_pack.citations[0].title, "alpha-note.md");
        assert_eq!(response.reply_pack.citations[1].title, "beta-note.md");
        assert_eq!(response.reply_pack.citations[2].title, "alpha.png");

        cleanup_dir(&root);
        cleanup_dir(&index_root);
    }

    #[test]
    fn hybrid_retrieval_uses_embedding_store_to_break_lexical_ties() {
        let _guard = INDEX_TEST_LOCK.lock().expect("lock");
        let root = unique_dir("harborbeacon-knowledge-hybrid");
        let index_root = unique_dir("harborbeacon-knowledge-index-store");
        let admin_state_path = unique_dir("harborbeacon-admin-model-center-embed").join("state.json");
        fs::create_dir_all(root.join("docs")).expect("create docs");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::create_dir_all(
            admin_state_path
                .parent()
                .expect("admin state path parent directory"),
        )
        .expect("create admin state dir");
        fs::write(root.join("docs").join("a-note.md"), "樱花 会议 纪要").expect("doc a");
        fs::write(root.join("docs").join("b-note.md"), "整理 计划 清单").expect("doc b");
        build_search_index(&root, &index_root);

        write_mock_model_center_state_with_embed(
            &admin_state_path,
            json!({
                "樱花整理": [1.0, 0.0],
                "樱花 会议 纪要": [0.05, 0.95],
                "整理 计划 清单": [0.98, 0.02]
            }),
        );

        std::env::set_var("HARBOR_ADMIN_STATE_PATH", &admin_state_path);
        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "樱花整理".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: true,
            include_images: false,
            limit: 10,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("hybrid search");
        std::env::remove_var("HARBOR_ADMIN_STATE_PATH");

        assert_eq!(response.documents.len(), 2);
        assert_eq!(response.documents[0].title, "b-note.md");
        assert_eq!(response.documents[1].title, "a-note.md");
        assert!(response.documents[0].embedding_score.unwrap_or_default() > 0.9);
        assert!(response.documents[0].hybrid_score.unwrap_or_default() > 0.5);
        assert!(response.reply_pack.citations[0]
            .embedding_score
            .unwrap_or_default()
            > 0.9);
        assert!(response
            .supported_modalities
            .iter()
            .any(|item| item == "hybrid_retrieval"));
        assert!(
            index_root
                .read_dir()
                .expect("list index root")
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().ends_with(".embeddings.json"))
        );

        cleanup_dir(&root);
        cleanup_dir(&index_root);
        cleanup_dir(
            admin_state_path
                .parent()
                .expect("admin state path parent directory"),
        );
    }

    #[test]
    fn search_surfaces_sidecar_and_ocr_provenance_for_image_hits() {
        let _guard = INDEX_TEST_LOCK.lock().expect("lock");
        let root = unique_dir("harborbeacon-knowledge-image-provenance");
        let index_root = unique_dir("harborbeacon-knowledge-index-store");
        let admin_state_path = unique_dir("harborbeacon-admin-model-center").join("state.json");
        fs::create_dir_all(root.join("images")).expect("create images");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::create_dir_all(
            admin_state_path
                .parent()
                .expect("admin state path parent directory"),
        )
        .expect("create admin state dir");
        fs::write(root.join("images").join("gate.jpg"), b"fake-image").expect("write image");
        fs::write(
            root.join("images").join("gate.txt"),
            "front gate camera overview",
        )
        .expect("write sidecar");
        write_mock_model_center_state(&admin_state_path, "plate ABC123 from OCR", None);

        std::env::set_var("HARBOR_ADMIN_STATE_PATH", &admin_state_path);
        build_search_index(&root, &index_root);
        let sidecar_response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "front".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: false,
            include_images: true,
            limit: 5,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("sidecar search");
        let ocr_response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "ABC123".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: false,
            include_images: true,
            limit: 5,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("ocr search");
        std::env::remove_var("HARBOR_ADMIN_STATE_PATH");

        assert_eq!(sidecar_response.images.len(), 1);
        assert_eq!(
            sidecar_response.images[0].provenance.as_deref(),
            Some("sidecar")
        );
        assert!(sidecar_response.images[0]
            .source_path
            .as_deref()
            .unwrap_or_default()
            .ends_with("gate.txt"));
        assert_eq!(
            sidecar_response.reply_pack.citations[0]
                .provenance
                .as_deref(),
            Some("sidecar")
        );
        assert!(sidecar_response
            .supported_modalities
            .iter()
            .any(|item| item == "ocr"));

        assert_eq!(ocr_response.images.len(), 1);
        assert_eq!(ocr_response.images[0].provenance.as_deref(), Some("ocr"));
        assert!(ocr_response.images[0].source_path.is_none());
        assert_eq!(
            ocr_response.reply_pack.citations[0].provenance.as_deref(),
            Some("ocr")
        );
        assert!(ocr_response
            .pending_modalities
            .iter()
            .any(|item| item == "vlm"));

        cleanup_dir(&root);
        cleanup_dir(&index_root);
        cleanup_dir(
            admin_state_path
                .parent()
                .expect("admin state path parent directory"),
        );
    }

    #[test]
    fn search_surfaces_vlm_provenance_for_image_hits() {
        let _guard = INDEX_TEST_LOCK.lock().expect("lock");
        let root = unique_dir("harborbeacon-knowledge-vlm");
        let index_root = unique_dir("harborbeacon-knowledge-index-store");
        let admin_state_path = unique_dir("harborbeacon-admin-model-center-vlm").join("state.json");
        fs::create_dir_all(root.join("images")).expect("create images");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::create_dir_all(
            admin_state_path
                .parent()
                .expect("admin state path parent directory"),
        )
        .expect("create admin state dir");
        fs::write(root.join("images").join("porch.jpg"), b"fake-image").expect("write image");
        write_mock_model_center_state(
            &admin_state_path,
            "",
            Some("门口地面有一个快递箱和一把折叠雨伞"),
        );

        std::env::set_var("HARBOR_ADMIN_STATE_PATH", &admin_state_path);
        build_search_index(&root, &index_root);
        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "快递箱".to_string(),
            configured_roots: vec![root.to_string_lossy().into_owned()],
            index_root: Some(index_root.to_string_lossy().into_owned()),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: false,
            include_images: true,
            limit: 5,
            ..KnowledgeSearchRequest::new("")
        })
        .expect("vlm search");
        std::env::remove_var("HARBOR_ADMIN_STATE_PATH");

        assert_eq!(response.images.len(), 1);
        assert_eq!(response.images[0].provenance.as_deref(), Some("vlm"));
        assert_eq!(
            response.reply_pack.citations[0].provenance.as_deref(),
            Some("vlm")
        );
        assert!(response
            .supported_modalities
            .iter()
            .any(|item| item == "vlm"));
        assert!(!response.pending_modalities.iter().any(|item| item == "vlm"));

        cleanup_dir(&root);
        cleanup_dir(&index_root);
        cleanup_dir(
            admin_state_path
                .parent()
                .expect("admin state path parent directory"),
        );
    }
}
