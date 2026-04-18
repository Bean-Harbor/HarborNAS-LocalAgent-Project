//! HarborBeacon-local knowledge retrieval over NAS-backed documents and images.

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runtime::knowledge_index::{
    KnowledgeIndexChunk, KnowledgeIndexEntry, KnowledgeIndexService, KnowledgeModality,
};

pub const KNOWLEDGE_ROOTS_ENV: &str = "HARBOR_KNOWLEDGE_ROOTS";

const DEFAULT_LIMIT: usize = 5;
const DEFAULT_KNOWLEDGE_DIR: &str = "knowledge";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSearchRequest {
    pub query: String,
    pub roots: Vec<String>,
    pub include_documents: bool,
    pub include_images: bool,
    pub limit: usize,
}

impl KnowledgeSearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            roots: Vec::new(),
            include_documents: true,
            include_images: true,
            limit: DEFAULT_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeSearchHit {
    pub modality: String,
    pub path: String,
    pub title: String,
    pub score: u32,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KnowledgeSearchReplyPack {
    pub summary: String,
    #[serde(default)]
    pub citations: Vec<KnowledgeSearchCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
}

pub struct KnowledgeSearchService;

impl KnowledgeSearchService {
    pub fn search(request: KnowledgeSearchRequest) -> Result<KnowledgeSearchResponse, String> {
        let query = request.query.trim().to_string();
        if query.is_empty() {
            return Err(
                "缺少知识库检索关键词，请提供 query 或更明确的自然语言检索请求。".to_string(),
            );
        }
        if !request.include_documents && !request.include_images {
            return Err("当前检索请求没有启用可支持的模态，至少需要文档或图片之一。".to_string());
        }

        let roots = resolve_roots(&request.roots)?;
        let query_terms = build_query_terms(&query);
        let index_service = KnowledgeIndexService::new()?;
        let mut seen_hits = HashSet::new();

        let mut documents = Vec::new();
        let mut images = Vec::new();
        for root in &roots {
            let snapshot = index_service.load_or_refresh(root)?;
            for entry in &snapshot.manifest.entries {
                let modality = entry.modality;
                match modality {
                    KnowledgeModality::Document if request.include_documents => {
                        for hit in build_hit_from_index_entry(entry, &query_terms) {
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
                        for hit in build_hit_from_index_entry(entry, &query_terms) {
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
        }

        sort_hits(&mut documents);
        sort_hits(&mut images);

        let total_matches = documents.len() + images.len();
        let limit = request.limit.clamp(1, 10);
        documents.truncate(limit);
        images.truncate(limit);
        let reply_pack = build_reply_pack(&query, total_matches, &documents, &images);

        Ok(KnowledgeSearchResponse {
            query,
            roots: roots
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            total_matches,
            documents,
            images,
            reply_pack,
            supported_modalities: vec!["document".to_string(), "image".to_string()],
            pending_modalities: vec!["audio".to_string(), "video".to_string()],
        })
    }
}

fn resolve_roots(request_roots: &[String]) -> Result<Vec<PathBuf>, String> {
    let raw_roots = if request_roots.is_empty() {
        default_roots()
    } else {
        request_roots
            .iter()
            .map(|item| PathBuf::from(item.trim()))
            .collect()
    };

    let mut roots = Vec::new();
    for root in raw_roots {
        if root.as_os_str().is_empty() {
            continue;
        }
        if root.exists() {
            roots.push(root);
        }
    }

    if roots.is_empty() {
        return Err(format!(
            "未找到可检索的知识库目录；请通过请求参数 roots 或环境变量 {KNOWLEDGE_ROOTS_ENV} 配置 NAS 检索根目录。"
        ));
    }

    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn default_roots() -> Vec<PathBuf> {
    if let Ok(value) = env::var(KNOWLEDGE_ROOTS_ENV) {
        let paths = env::split_paths(&value)
            .filter(|path| !path.as_os_str().is_empty())
            .collect::<Vec<_>>();
        if !paths.is_empty() {
            return paths;
        }
    }

    let default_dir = PathBuf::from(DEFAULT_KNOWLEDGE_DIR);
    default_dir
        .exists()
        .then_some(default_dir)
        .into_iter()
        .collect()
}

/// Build a HarborBeacon-owned hit from an indexed entry, preserving the stable
/// response shape used by `TaskResponse.result.data`.
fn build_hit_from_index_entry(
    entry: &KnowledgeIndexEntry,
    query_terms: &[String],
) -> Vec<KnowledgeSearchHit> {
    let path = Path::new(&entry.path);
    let chunks = if entry.chunks.is_empty() {
        vec![KnowledgeIndexChunk {
            chunk_id: "chunk-0001".to_string(),
            line_start: 1,
            line_end: entry.searchable_text.lines().count().max(1),
            text: entry.searchable_text.clone(),
        }]
    } else {
        entry.chunks.clone()
    };

    chunks
        .iter()
        .filter_map(|chunk| {
            build_hit(
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
            "已检索知识库，但暂时没有找到与“{}”相关的文档或图片。",
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

fn build_hit(
    path: &Path,
    modality: KnowledgeModality,
    searchable_text: Option<&str>,
    query_terms: &[String],
    chunk: Option<&KnowledgeIndexChunk>,
) -> Option<KnowledgeSearchHit> {
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

    Some(KnowledgeSearchHit {
        modality: modality.as_str().to_string(),
        path: display_path,
        title,
        score,
        chunk_id: chunk.map(|item| item.chunk_id.clone()),
        line_start: chunk.map(|item| item.line_start),
        line_end: chunk.map(|item| item.line_end),
        snippet: searchable_text.and_then(|text| build_snippet(text, &matched_terms)),
        matched_terms,
    })
}

fn sort_hits(hits: &mut [KnowledgeSearchHit]) {
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
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

    use super::{KnowledgeSearchRequest, KnowledgeSearchService};

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

        std::env::set_var("HARBOR_KNOWLEDGE_INDEX_ROOT", &index_root);
        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "樱花".to_string(),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: true,
            include_images: true,
            limit: 5,
        })
        .expect("knowledge search");
        std::env::remove_var("HARBOR_KNOWLEDGE_INDEX_ROOT");

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

        std::env::set_var("HARBOR_KNOWLEDGE_INDEX_ROOT", &index_root);
        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "樱花".to_string(),
            roots: vec![root.to_string_lossy().into_owned()],
            include_documents: true,
            include_images: false,
            limit: 5,
        })
        .expect("knowledge search");
        std::env::remove_var("HARBOR_KNOWLEDGE_INDEX_ROOT");

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

        std::env::set_var("HARBOR_KNOWLEDGE_INDEX_ROOT", &index_root);
        let response = KnowledgeSearchService::search(KnowledgeSearchRequest {
            query: "spring".to_string(),
            roots: vec![
                root.to_string_lossy().into_owned(),
                root.to_string_lossy().into_owned(),
            ],
            include_documents: true,
            include_images: true,
            limit: 10,
        })
        .expect("knowledge search");
        std::env::remove_var("HARBOR_KNOWLEDGE_INDEX_ROOT");

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
}
