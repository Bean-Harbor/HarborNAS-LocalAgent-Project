use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub const MODEL_BENCHMARK_SCHEMA_VERSION: u32 = 1;
pub const MODEL_BENCHMARK_CONTRACT: &str = "local-openai-compatible-api";
pub const BENCHMARK_RUN_ROLE_BUILDER_COMPATIBILITY: &str = "builder-compatibility";
pub const BENCHMARK_RUN_ROLE_TARGET_RUNTIME_PROMOTION: &str = "target-runtime-promotion";
pub const MIN_EMBEDDING_MRR: f32 = 0.66;
pub const MIN_EMBEDDING_IMPROVEMENT: f32 = 0.05;
pub const MIN_EMBEDDING_DIMENSIONS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalModelBenchmarkReport {
    pub schema_version: u32,
    pub contract: String,
    pub mode: String,
    #[serde(default)]
    pub run_role: String,
    pub backend: String,
    pub base_url: String,
    pub healthz_url: String,
    pub generated_at_utc: String,
    pub cold_start: ColdStartProbeResult,
    pub health: HealthProbeResult,
    pub chat: ChatBenchmarkSummary,
    pub embeddings: EmbeddingBenchmarkSummary,
    pub gate: ModelBenchmarkGate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColdStartProbeResult {
    pub measured: bool,
    pub ready_within_timeout: bool,
    pub timeout_ms: u64,
    pub attempts: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthProbeResult {
    pub ok: bool,
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatBenchmarkSummary {
    pub passed: bool,
    pub passed_count: usize,
    pub required_passes: usize,
    #[serde(default)]
    pub semantic_ok_passed: usize,
    #[serde(default)]
    pub output_clean_passed: usize,
    pub probes: Vec<ChatProbeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatProbeResult {
    pub name: String,
    pub prompt: String,
    pub ok: bool,
    #[serde(default)]
    pub semantic_ok: bool,
    #[serde(default)]
    pub output_clean: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_expectation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingBenchmarkSummary {
    pub passed: bool,
    pub vector_probe_ok: bool,
    pub retrieval_probe_ok: bool,
    pub consistent_dimensions: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_dimensions: Option<usize>,
    pub lexical_mrr: f32,
    pub embedding_mrr: f32,
    pub improvement_over_lexical: f32,
    pub probes: Vec<EmbeddingProbeResult>,
    pub retrieval_cases: Vec<EmbeddingRetrievalCaseResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingProbeResult {
    pub name: String,
    pub input: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<usize>,
    pub non_zero_dimensions: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingRetrievalCase {
    pub case_id: String,
    pub query: String,
    pub relevant_candidate_id: String,
    pub candidates: Vec<EmbeddingRetrievalCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingRetrievalCandidate {
    pub candidate_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingRetrievalCaseResult {
    pub case_id: String,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_top_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_top_candidate_id: Option<String>,
    pub lexical_rr: f32,
    pub embedding_rr: f32,
    pub improved_over_lexical: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelBenchmarkGate {
    pub promotable: bool,
    pub reasons: Vec<String>,
    pub recommendation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatBenchmarkSpec {
    pub name: &'static str,
    pub prompt: &'static str,
    pub expected_any: &'static [&'static str],
}

pub fn default_chat_specs() -> Vec<ChatBenchmarkSpec> {
    vec![
        ChatBenchmarkSpec {
            name: "zh-yes-no",
            prompt: "请只回答“是”或“否”：摄像头能用于抓拍吗？",
            expected_any: &["是"],
        },
        ChatBenchmarkSpec {
            name: "zh-short-classification",
            prompt: "请只回答一个词：“樱花”更像植物、工具还是地点？",
            expected_any: &["植物"],
        },
        ChatBenchmarkSpec {
            name: "zh-action-contrast",
            prompt: "请只回答一个词：在“录像”和“抓拍”里，哪一个更像持续动作？",
            expected_any: &["录像"],
        },
    ]
}

pub fn default_embedding_cases() -> Vec<EmbeddingRetrievalCase> {
    vec![
        EmbeddingRetrievalCase {
            case_id: "flower-records".to_string(),
            query: "帮我找一下春天开花的记录".to_string(),
            relevant_candidate_id: "doc-flower".to_string(),
            candidates: vec![
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-flower".to_string(),
                    text: "樱花观测记录与花期照片归档".to_string(),
                },
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-export".to_string(),
                    text: "记录导出与资料归档规则".to_string(),
                },
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-camera".to_string(),
                    text: "摄像头安装手册与接线说明".to_string(),
                },
            ],
        },
        EmbeddingRetrievalCase {
            case_id: "snapshot-help".to_string(),
            query: "帮我找一下拍照画面的说明".to_string(),
            relevant_candidate_id: "doc-snapshot".to_string(),
            candidates: vec![
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-snapshot".to_string(),
                    text: "摄像头抓拍使用说明与图片归档策略".to_string(),
                },
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-record".to_string(),
                    text: "录像保存时长与导出步骤".to_string(),
                },
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-flower".to_string(),
                    text: "植物识别备忘与花期观察日志".to_string(),
                },
            ],
        },
        EmbeddingRetrievalCase {
            case_id: "video-export".to_string(),
            query: "帮我找一下录像导出的办法".to_string(),
            relevant_candidate_id: "doc-record".to_string(),
            candidates: vec![
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-record".to_string(),
                    text: "录像保存时长与导出步骤".to_string(),
                },
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-snapshot".to_string(),
                    text: "摄像头抓拍使用说明与图片归档策略".to_string(),
                },
                EmbeddingRetrievalCandidate {
                    candidate_id: "doc-export".to_string(),
                    text: "文件命名规范与日志轮转说明".to_string(),
                },
            ],
        },
    ]
}

pub fn summarize_chat_probes(probes: Vec<ChatProbeResult>) -> ChatBenchmarkSummary {
    let required_passes = probes.len();
    let passed_count = probes.iter().filter(|probe| probe.ok).count();
    let semantic_ok_passed = probes.iter().filter(|probe| probe.semantic_ok).count();
    let output_clean_passed = probes.iter().filter(|probe| probe.output_clean).count();
    ChatBenchmarkSummary {
        passed: passed_count == required_passes
            && semantic_ok_passed == required_passes
            && output_clean_passed == required_passes,
        passed_count,
        required_passes,
        semantic_ok_passed,
        output_clean_passed,
        probes,
    }
}

pub fn evaluate_embedding_benchmark(
    probes: Vec<EmbeddingProbeResult>,
    retrieval_cases: Vec<EmbeddingRetrievalCaseResult>,
) -> EmbeddingBenchmarkSummary {
    let ok_dimensions = probes
        .iter()
        .filter_map(|probe| probe.dimensions)
        .collect::<Vec<_>>();
    let vector_dimensions = ok_dimensions.first().copied();
    let consistent_dimensions = !ok_dimensions.is_empty()
        && ok_dimensions.iter().all(|value| Some(*value) == vector_dimensions);
    let vector_probe_ok = consistent_dimensions
        && vector_dimensions.unwrap_or_default() >= MIN_EMBEDDING_DIMENSIONS
        && probes.iter().all(|probe| probe.ok);

    let lexical_mrr = mean_rr(retrieval_cases.iter().map(|case| case.lexical_rr));
    let embedding_mrr = mean_rr(retrieval_cases.iter().map(|case| case.embedding_rr));
    let improvement_over_lexical = embedding_mrr - lexical_mrr;
    let retrieval_probe_ok = retrieval_cases.iter().all(|case| case.error.is_none())
        && embedding_mrr >= MIN_EMBEDDING_MRR
        && improvement_over_lexical >= MIN_EMBEDDING_IMPROVEMENT;

    EmbeddingBenchmarkSummary {
        passed: vector_probe_ok && retrieval_probe_ok,
        vector_probe_ok,
        retrieval_probe_ok,
        consistent_dimensions,
        vector_dimensions,
        lexical_mrr,
        embedding_mrr,
        improvement_over_lexical,
        probes,
        retrieval_cases,
    }
}

pub fn evaluate_embedding_case(
    case: &EmbeddingRetrievalCase,
    vectors: &HashMap<String, Vec<f32>>,
) -> EmbeddingRetrievalCaseResult {
    let Some(query_vector) = vectors.get(&case.query) else {
        return EmbeddingRetrievalCaseResult {
            case_id: case.case_id.clone(),
            query: case.query.clone(),
            lexical_top_candidate_id: None,
            embedding_top_candidate_id: None,
            lexical_rr: 0.0,
            embedding_rr: 0.0,
            improved_over_lexical: false,
            error: Some("query embedding missing".to_string()),
        };
    };

    let mut lexical_scores = Vec::new();
    let mut embedding_scores = Vec::new();

    for candidate in &case.candidates {
        lexical_scores.push((
            candidate.candidate_id.clone(),
            lexical_similarity(&case.query, &candidate.text),
        ));

        let Some(candidate_vector) = vectors.get(&candidate.text) else {
            return EmbeddingRetrievalCaseResult {
                case_id: case.case_id.clone(),
                query: case.query.clone(),
                lexical_top_candidate_id: lexical_top_candidate_id(&lexical_scores),
                embedding_top_candidate_id: None,
                lexical_rr: reciprocal_rank(&lexical_scores, &case.relevant_candidate_id),
                embedding_rr: 0.0,
                improved_over_lexical: false,
                error: Some(format!(
                    "candidate embedding missing for {}",
                    candidate.candidate_id
                )),
            };
        };

        let Some(score) = cosine_similarity(query_vector, candidate_vector) else {
            return EmbeddingRetrievalCaseResult {
                case_id: case.case_id.clone(),
                query: case.query.clone(),
                lexical_top_candidate_id: lexical_top_candidate_id(&lexical_scores),
                embedding_top_candidate_id: None,
                lexical_rr: reciprocal_rank(&lexical_scores, &case.relevant_candidate_id),
                embedding_rr: 0.0,
                improved_over_lexical: false,
                error: Some(format!(
                    "embedding dimensions do not match for {}",
                    candidate.candidate_id
                )),
            };
        };

        embedding_scores.push((candidate.candidate_id.clone(), score));
    }

    let lexical_rr = reciprocal_rank(&lexical_scores, &case.relevant_candidate_id);
    let embedding_rr = reciprocal_rank(&embedding_scores, &case.relevant_candidate_id);

    EmbeddingRetrievalCaseResult {
        case_id: case.case_id.clone(),
        query: case.query.clone(),
        lexical_top_candidate_id: lexical_top_candidate_id(&lexical_scores),
        embedding_top_candidate_id: lexical_top_candidate_id(&embedding_scores),
        lexical_rr,
        embedding_rr,
        improved_over_lexical: embedding_rr > lexical_rr,
        error: None,
    }
}

pub fn build_model_benchmark_gate(
    run_role: &str,
    backend: &str,
    cold_start: &ColdStartProbeResult,
    health: &HealthProbeResult,
    chat: &ChatBenchmarkSummary,
    embeddings: &EmbeddingBenchmarkSummary,
) -> ModelBenchmarkGate {
    let mut reasons = Vec::new();

    let target_runtime_run = run_role
        .trim()
        .eq_ignore_ascii_case(BENCHMARK_RUN_ROLE_TARGET_RUNTIME_PROMOTION);
    if !target_runtime_run {
        let recommendation = format!(
            "rerun with --spawn-binary on the target runtime to produce '{}' evidence before promoting '{}'",
            BENCHMARK_RUN_ROLE_TARGET_RUNTIME_PROMOTION, backend
        );
        return ModelBenchmarkGate {
            promotable: false,
            reasons: vec![format!(
                "builder compatibility evidence cannot promote a backend; {}",
                recommendation
            )],
            recommendation,
        };
    }

    if !cold_start.measured {
        reasons.push(
            "cold start was not measured on the target runtime; rerun with --spawn-binary before promoting a backend"
                .to_string(),
        );
    } else if !cold_start.ready_within_timeout {
        reasons.push("backend did not become ready within the cold start timeout".to_string());
    }

    if !health.ok || !health.ready {
        reasons.push("healthz did not report a ready local model service".to_string());
    }

    if let Some(kind) = health.backend_kind.as_deref() {
        if !backend.trim().is_empty() && !kind.eq_ignore_ascii_case(backend.trim()) {
            reasons.push(format!(
                "healthz reported backend '{}' but benchmark expected '{}'",
                kind, backend
            ));
        }
    }

    if !chat.passed {
        reasons.push("Chinese chat probes did not pass the semantic stability gate".to_string());
    }

    if !embeddings.passed {
        reasons.push(
            "embedding probes did not clear the dimensions and lexical-improvement gate"
                .to_string(),
        );
    }

    let promotable = reasons.is_empty();
    let recommendation = if promotable {
        format!(
            "backend '{}' is promotable from target-runtime evidence behind the local OpenAI-compatible seam",
            backend
        )
    } else {
        format!(
            "keep the current default backend until '{}' clears the target-runtime benchmark gate",
            backend
        )
    };

    ModelBenchmarkGate {
        promotable,
        reasons,
        recommendation,
    }
}

pub fn lexical_similarity(query: &str, candidate: &str) -> f32 {
    let left = lexical_units(query);
    let right = lexical_units(candidate);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let left_set = left.into_iter().collect::<HashSet<_>>();
    let right_set = right.into_iter().collect::<HashSet<_>>();
    let overlap = left_set.intersection(&right_set).count() as f32;
    (2.0 * overlap) / ((left_set.len() + right_set.len()) as f32)
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return None;
    }

    let mut dot = 0.0f32;
    let mut left_norm = 0.0f32;
    let mut right_norm = 0.0f32;
    for (l, r) in left.iter().zip(right.iter()) {
        dot += l * r;
        left_norm += l * l;
        right_norm += r * r;
    }
    if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
        return None;
    }

    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

pub fn trim_excerpt(value: &str, limit: usize) -> String {
    let chars = value.trim().chars().collect::<Vec<_>>();
    if chars.len() <= limit {
        return chars.into_iter().collect();
    }
    chars.into_iter().take(limit).collect::<String>() + "..."
}

pub fn benchmark_run_role(spawned: bool) -> &'static str {
    if spawned {
        BENCHMARK_RUN_ROLE_TARGET_RUNTIME_PROMOTION
    } else {
        BENCHMARK_RUN_ROLE_BUILDER_COMPATIBILITY
    }
}

pub fn is_output_clean(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains("<think") || trimmed.contains("</think>") {
        return false;
    }
    if trimmed.contains('\n') || trimmed.contains('\r') || trimmed.contains("```") {
        return false;
    }
    let normalized = trimmed.trim_start_matches(|character: char| {
        matches!(character, '"' | '\'' | '“' | '”' | ' ' | '\t')
    });
    for prefix in [
        "答案",
        "答：",
        "答:",
        "分析",
        "思考",
        "推理",
        "结论",
        "最终答案",
        "final answer",
        "analysis",
        "reasoning",
    ] {
        if normalized.starts_with(prefix) {
            return false;
        }
    }
    if trimmed.contains('，')
        || trimmed.contains('。')
        || trimmed.contains('！')
        || trimmed.contains('？')
        || trimmed.contains(',')
        || trimmed.contains('.')
        || trimmed.contains('!')
        || trimmed.contains('?')
        || trimmed.contains(':')
        || trimmed.contains('：')
        || trimmed.contains(';')
        || trimmed.contains('；')
    {
        return false;
    }
    if trimmed.starts_with('-')
        || trimmed.starts_with('*')
        || trimmed.starts_with('>')
        || trimmed.starts_with("1.")
        || trimmed.starts_with("1)")
    {
        return false;
    }
    trimmed.chars().count() <= 16
}

fn lexical_units(value: &str) -> Vec<String> {
    let chars = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }
    if chars.len() == 1 {
        return vec![chars[0].to_string()];
    }
    chars
        .windows(2)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn lexical_top_candidate_id(scores: &[(String, f32)]) -> Option<String> {
    let mut sorted = scores.to_vec();
    sorted.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    sorted.first().map(|item| item.0.clone())
}

fn reciprocal_rank(scores: &[(String, f32)], relevant_candidate_id: &str) -> f32 {
    let mut sorted = scores.to_vec();
    sorted.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    sorted
        .iter()
        .position(|item| item.0 == relevant_candidate_id)
        .map(|index| 1.0 / ((index + 1) as f32))
        .unwrap_or(0.0)
}

fn mean_rr(values: impl Iterator<Item = f32>) -> f32 {
    let values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / (values.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::{
        benchmark_run_role, build_model_benchmark_gate, cosine_similarity, default_embedding_cases,
        evaluate_embedding_benchmark, evaluate_embedding_case, is_output_clean,
        lexical_similarity, summarize_chat_probes, ChatProbeResult, ColdStartProbeResult,
        EmbeddingProbeResult, EmbeddingRetrievalCaseResult, HealthProbeResult,
        BENCHMARK_RUN_ROLE_BUILDER_COMPATIBILITY, BENCHMARK_RUN_ROLE_TARGET_RUNTIME_PROMOTION,
    };
    use std::collections::HashMap;

    #[test]
    fn lexical_similarity_prefers_related_text() {
        let related = lexical_similarity("帮我找一下录像导出的办法", "录像保存时长与导出步骤");
        let unrelated = lexical_similarity("帮我找一下录像导出的办法", "植物识别备忘与花期观察日志");
        assert!(related > unrelated);
    }

    #[test]
    fn cosine_similarity_requires_matching_dimensions() {
        assert!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]).unwrap() > 0.99);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), None);
    }

    #[test]
    fn embedding_case_uses_vectors_to_beat_lexical_baseline() {
        let case = default_embedding_cases()
            .into_iter()
            .find(|entry| entry.case_id == "flower-records")
            .expect("case");

        let vectors = HashMap::from([
            (case.query.clone(), vec![1.0, 0.0]),
            (case.candidates[0].text.clone(), vec![0.99, 0.01]),
            (case.candidates[1].text.clone(), vec![0.10, 0.90]),
            (case.candidates[2].text.clone(), vec![-0.50, 0.10]),
        ]);

        let result = evaluate_embedding_case(&case, &vectors);
        assert_eq!(result.embedding_top_candidate_id.as_deref(), Some("doc-flower"));
        assert!(result.embedding_rr >= result.lexical_rr);
        assert!(result.error.is_none());
    }

    #[test]
    fn embedding_benchmark_requires_dimensions_and_retrieval_gain() {
        let summary = evaluate_embedding_benchmark(
            vec![
                EmbeddingProbeResult {
                    name: "a".to_string(),
                    input: "alpha".to_string(),
                    ok: true,
                    latency_ms: Some(10),
                    dimensions: Some(16),
                    non_zero_dimensions: 12,
                    error: None,
                },
                EmbeddingProbeResult {
                    name: "b".to_string(),
                    input: "beta".to_string(),
                    ok: true,
                    latency_ms: Some(11),
                    dimensions: Some(16),
                    non_zero_dimensions: 11,
                    error: None,
                },
            ],
            vec![
                EmbeddingRetrievalCaseResult {
                    case_id: "one".to_string(),
                    query: "q1".to_string(),
                    lexical_top_candidate_id: Some("b".to_string()),
                    embedding_top_candidate_id: Some("a".to_string()),
                    lexical_rr: 0.5,
                    embedding_rr: 1.0,
                    improved_over_lexical: true,
                    error: None,
                },
                EmbeddingRetrievalCaseResult {
                    case_id: "two".to_string(),
                    query: "q2".to_string(),
                    lexical_top_candidate_id: Some("b".to_string()),
                    embedding_top_candidate_id: Some("a".to_string()),
                    lexical_rr: 0.5,
                    embedding_rr: 1.0,
                    improved_over_lexical: true,
                    error: None,
                },
            ],
        );
        assert!(summary.passed);
        assert_eq!(summary.vector_dimensions, Some(16));
        assert!(summary.improvement_over_lexical > 0.05);
    }

    #[test]
    fn chat_probes_track_semantic_and_output_clean_dimensions() {
        let summary = summarize_chat_probes(vec![
            ChatProbeResult {
                name: "clean".to_string(),
                prompt: "prompt".to_string(),
                ok: true,
                semantic_ok: true,
                output_clean: true,
                latency_ms: Some(10),
                matched_expectation: Some("ok".to_string()),
                response_excerpt: Some("ok".to_string()),
                error: None,
            },
            ChatProbeResult {
                name: "semantic-but-not-clean".to_string(),
                prompt: "prompt".to_string(),
                ok: false,
                semantic_ok: true,
                output_clean: false,
                latency_ms: Some(10),
                matched_expectation: Some("ok".to_string()),
                response_excerpt: Some("ok, and here is extra".to_string()),
                error: None,
            },
        ]);
        assert_eq!(summary.passed_count, 1);
        assert_eq!(summary.semantic_ok_passed, 2);
        assert_eq!(summary.output_clean_passed, 1);
        assert!(!summary.passed);
    }

    #[test]
    fn output_clean_heuristic_rejects_wrapped_answers() {
        assert!(is_output_clean("是"));
        assert!(!is_output_clean("是，而且我还可以继续解释"));
        assert!(!is_output_clean("```text\n是\n```"));
    }

    #[test]
    fn benchmark_run_role_distinguishes_builder_from_target_runtime() {
        assert_eq!(
            benchmark_run_role(false),
            BENCHMARK_RUN_ROLE_BUILDER_COMPATIBILITY
        );
        assert_eq!(
            benchmark_run_role(true),
            BENCHMARK_RUN_ROLE_TARGET_RUNTIME_PROMOTION
        );
    }

    #[test]
    fn benchmark_gate_blocks_builder_compatibility_evidence_for_promotion() {
        let chat = summarize_chat_probes(vec![ChatProbeResult {
            name: "probe".to_string(),
            prompt: "prompt".to_string(),
            ok: true,
            semantic_ok: true,
            output_clean: true,
            latency_ms: Some(10),
            matched_expectation: Some("ok".to_string()),
            response_excerpt: Some("ok".to_string()),
            error: None,
        }]);
        let embeddings = evaluate_embedding_benchmark(
            vec![EmbeddingProbeResult {
                name: "a".to_string(),
                input: "alpha".to_string(),
                ok: true,
                latency_ms: Some(10),
                dimensions: Some(16),
                non_zero_dimensions: 12,
                error: None,
            }],
            vec![EmbeddingRetrievalCaseResult {
                case_id: "one".to_string(),
                query: "q1".to_string(),
                lexical_top_candidate_id: Some("b".to_string()),
                embedding_top_candidate_id: Some("a".to_string()),
                lexical_rr: 0.5,
                embedding_rr: 1.0,
                improved_over_lexical: true,
                error: None,
            }],
        );
        let gate = build_model_benchmark_gate(
            BENCHMARK_RUN_ROLE_BUILDER_COMPATIBILITY,
            "candle",
            &ColdStartProbeResult {
                measured: false,
                ready_within_timeout: false,
                timeout_ms: 30_000,
                attempts: 0,
                elapsed_ms: None,
                note: None,
            },
            &HealthProbeResult {
                ok: true,
                ready: true,
                http_status: Some(200),
                service: Some("harbor-model-api".to_string()),
                status: Some("ok".to_string()),
                backend_kind: Some("candle".to_string()),
                backend_ready: Some(true),
                note: None,
                error: None,
            },
            &chat,
            &embeddings,
        );
        assert!(!gate.promotable);
        assert!(gate
            .reasons
            .iter()
            .any(|reason| reason.contains("builder compatibility evidence cannot promote")));
    }

    #[test]
    fn benchmark_gate_allows_target_runtime_promotion_when_checks_pass() {
        let chat = summarize_chat_probes(vec![ChatProbeResult {
            name: "probe".to_string(),
            prompt: "prompt".to_string(),
            ok: true,
            semantic_ok: true,
            output_clean: true,
            latency_ms: Some(10),
            matched_expectation: Some("ok".to_string()),
            response_excerpt: Some("ok".to_string()),
            error: None,
        }]);
        let embeddings = evaluate_embedding_benchmark(
            vec![EmbeddingProbeResult {
                name: "a".to_string(),
                input: "alpha".to_string(),
                ok: true,
                latency_ms: Some(10),
                dimensions: Some(16),
                non_zero_dimensions: 12,
                error: None,
            }],
            vec![EmbeddingRetrievalCaseResult {
                case_id: "one".to_string(),
                query: "q1".to_string(),
                lexical_top_candidate_id: Some("b".to_string()),
                embedding_top_candidate_id: Some("a".to_string()),
                lexical_rr: 0.5,
                embedding_rr: 1.0,
                improved_over_lexical: true,
                error: None,
            }],
        );
        let gate = build_model_benchmark_gate(
            BENCHMARK_RUN_ROLE_TARGET_RUNTIME_PROMOTION,
            "candle",
            &ColdStartProbeResult {
                measured: true,
                ready_within_timeout: true,
                timeout_ms: 30_000,
                attempts: 2,
                elapsed_ms: Some(1000),
                note: None,
            },
            &HealthProbeResult {
                ok: true,
                ready: true,
                http_status: Some(200),
                service: Some("harbor-model-api".to_string()),
                status: Some("ok".to_string()),
                backend_kind: Some("candle".to_string()),
                backend_ready: Some(true),
                note: None,
                error: None,
            },
            &chat,
            &embeddings,
        );
        assert!(gate.promotable);
    }
}
