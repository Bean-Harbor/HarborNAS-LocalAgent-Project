use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::scripts::integration::{
    default_midcli_service_query, IntegrationConfig, MidcliClient, MiddlewareClient,
};

const REQUIRED_FILES: [&str; 16] = [
    "HarborBeacon-LocalAgent-V2-Assistant-Skills-Roadmap.md",
    "HarborBeacon-Middleware-Endpoint-Contract-v1.md",
    "HarborBeacon-Files-BatchOps-Contract-v1.md",
    "HarborBeacon-Planner-TaskDecompose-Contract-v1.md",
    "HarborBeacon-Contract-E2E-Test-Plan-v1.md",
    "HarborBeacon-CI-Contract-Pipeline-Checklist-v1.md",
    "HarborBeacon-GitHub-Actions-Workflow-Draft-v1.md",
    "HarborBeacon-HarborGate-v2.0-Upgrade-Runbook.md",
    "docs/im-v2.0-cutover-rollback-observability-gates.md",
    "docs/hos-system-domain-cutover-smoke.md",
    "docs/cited-retrieval-reply-pack.md",
    "docs/knowledge-indexing-pack.md",
    "docs/retrieval-canary-roundtrip-evidence.md",
    "docs/retrieval-roundtrip-launch-pack.md",
    "docs/document-rag-mvp.md",
    "docs/local-model-backend-benchmark-gate.md",
];

const REQUIRED_MIDDLEWARE_METHODS: [&str; 5] = [
    "service.query",
    "service.control",
    "filesystem.listdir",
    "filesystem.copy",
    "filesystem.move",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub details: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateReport {
    pub mode: String,
    pub passed: bool,
    pub check_count: usize,
    pub checks: Vec<CheckResult>,
}

pub fn build_checks(root: &Path) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    for relative_path in REQUIRED_FILES {
        let path = root.join(relative_path);
        checks.push(CheckResult {
            name: format!("exists:{relative_path}"),
            passed: path.exists(),
            details: path.display().to_string(),
            skipped: None,
        });
    }

    let v2_doc =
        fs::read_to_string(root.join("HarborBeacon-LocalAgent-V2-Assistant-Skills-Roadmap.md"))
            .unwrap_or_default();
    let files_doc = fs::read_to_string(root.join("HarborBeacon-Files-BatchOps-Contract-v1.md"))
        .unwrap_or_default();
    let planner_doc =
        fs::read_to_string(root.join("HarborBeacon-Planner-TaskDecompose-Contract-v1.md"))
            .unwrap_or_default();
    let v20_upgrade_doc =
        fs::read_to_string(root.join("HarborBeacon-HarborGate-v2.0-Upgrade-Runbook.md"))
            .unwrap_or_default();
    let v20_gates_doc =
        fs::read_to_string(root.join("docs/im-v2.0-cutover-rollback-observability-gates.md"))
            .unwrap_or_default();
    let rollback_doc =
        fs::read_to_string(root.join("docs/im-v1.5-cutover-rollback-observability-gates.md"))
            .unwrap_or_default();
    let hos_smoke_doc = fs::read_to_string(root.join("docs/hos-system-domain-cutover-smoke.md"))
        .unwrap_or_default();
    let cited_reply_doc =
        fs::read_to_string(root.join("docs/cited-retrieval-reply-pack.md")).unwrap_or_default();
    let knowledge_index_doc =
        fs::read_to_string(root.join("docs/knowledge-indexing-pack.md")).unwrap_or_default();
    let canary_doc = fs::read_to_string(root.join("docs/retrieval-canary-roundtrip-evidence.md"))
        .unwrap_or_default();
    let retrieval_launch_doc =
        fs::read_to_string(root.join("docs/retrieval-roundtrip-launch-pack.md"))
            .unwrap_or_default();
    let document_rag_doc =
        fs::read_to_string(root.join("docs/document-rag-mvp.md")).unwrap_or_default();
    let local_model_benchmark_doc =
        fs::read_to_string(root.join("docs/local-model-backend-benchmark-gate.md"))
            .unwrap_or_default();

    checks.push(CheckResult {
        name: "route-priority:control-plane-first".to_string(),
        passed: [
            "1. Middleware API executor",
            "2. MidCLI executor (CLI via `midcli`)",
            "3. Browser executor",
            "4. MCP executor (fallback only)",
        ]
        .iter()
        .all(|item| v2_doc.contains(item)),
        details: "V2 roadmap must define the strict executor order.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "files-contract:path-policy".to_string(),
        passed: [
            "Allowed read roots",
            "Allowed write roots",
            "Denied roots",
            "command template allowlist",
        ]
        .iter()
        .all(|item| files_doc.contains(item)),
        details: "Files contract must define path policy and allowlist constraints.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "planner-contract:route-priority".to_string(),
        passed: planner_doc
            .contains("\"route_priority\": [\"middleware_api\", \"midcli\", \"browser\", \"mcp\"]"),
        details: "Planner contract must preserve the approved route priority order.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "v20-upgrade-control-pack:active-seam-coverage".to_string(),
        passed: [
            "POST /api/web/turns",
            "POST /api/turns",
            "POST /api/notifications/deliveries",
            "GET /api/gateway/status",
            "X-Contract-Version: 2.0",
            "conversation.handle",
            "active_frame",
            "continuation",
            "delivery_hints",
            "v1.5 documents are historical",
        ]
        .iter()
        .all(|item| v20_upgrade_doc.contains(item) || v20_gates_doc.contains(item)),
        details: "HarborBeacon v2.0 control pack must cover active endpoints, conversation state, delivery hints, and historical v1.5 status.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "rollback-doc:legacy-fallback-removed".to_string(),
        passed: [
            "legacy recipient fallback remains removed during rollback",
            "rollback notes must say that legacy recipient fallback stayed disabled",
        ]
        .iter()
        .all(|item| rollback_doc.contains(item)),
        details: "Rollback gate doc must keep legacy recipient fallback removed and document that clearly.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "hos-system-domain-cutover-smoke:boundary-and-fallback".to_string(),
        passed: [
            "Middleware API -> MidCLI -> Browser/MCP fallback",
            "Browser and MCP remain fallback-only for non-system domains",
            "HarborOS executors do not claim device-native domains",
            "discover",
            "snapshot",
            "share_link",
            "inspect",
            "control",
            "keep `Browser/MCP` as fallback only for non-system domains",
            "do not route IM or notification concerns back into HarborOS system control",
        ]
        .iter()
        .all(|item| hos_smoke_doc.contains(item)),
        details: "HarborOS smoke pack must document the frozen system-domain route order, fallback-only Browser/MCP behavior, and rollback boundary limits.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "retrieval-launch-pack:operator-handoff".to_string(),
        passed: [
            "Retrieval Round-Trip Launch Pack",
            "explicit `knowledge.search`",
            "returns `failed` from `task_api`",
            "No legacy retrieval fallback exists to toggle during rollback.",
            "Rollback",
        ]
        .iter()
        .all(|item| retrieval_launch_doc.contains(item)),
        details: "Retrieval launch pack must show explicit search, non-opportunistic general messages, and operator-facing rollback notes.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "cited-retrieval-reply-pack:boundary-and-packaging".to_string(),
        passed: [
            "HarborBeacon owns retrieval semantics, ranking, citation packaging, and reply meaning.",
            "HarborOS remains a read-only file substrate and may host the local model,",
            "OCR and vector search are supported upstream in the retrieval pipeline; this",
        ]
        .iter()
        .all(|item| cited_reply_doc.contains(item)),
        details: "Reply-pack doc must preserve the HarborBeacon boundary while acknowledging OCR/vector-backed packaging upstream.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "knowledge-indexing-pack:image-ocr-and-vector-loop".to_string(),
        passed: [
            "Documents are indexed from file text or normalization output.",
            "Images are indexed from the image file plus OCR text and the first matching",
            "OCR and vector search are part of the indexed document/image loop in this",
        ]
        .iter()
        .all(|item| knowledge_index_doc.contains(item)),
        details: "Knowledge indexing doc must describe OCR-backed image ingestion and local vector-aware retrieval.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "retrieval-canary-roundtrip:planner-routed-nl-retrieval".to_string(),
        passed: [
            "Planner-routed natural-language retrieval for retrieval-intent",
            "Opportunistic natural-language retrieval is now a supported route when the",
            "Explicit `knowledge.search` remains the direct retrieval path.",
        ]
        .iter()
        .all(|item| canary_doc.contains(item)),
        details: "Canary doc must describe the supported natural-language retrieval route and preserve explicit search as the direct path.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "retrieval-launch-pack:general-message-route".to_string(),
        passed: [
            "General Message Can Route To Retrieval",
            "result.status = completed",
            "natural-language retrieval routing is allowed when the planner recognizes",
        ]
        .iter()
        .all(|item| retrieval_launch_doc.contains(item)),
        details: "Launch pack must describe the general-message retrieval path and the expected completed reply shape.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "document-rag-mvp:grounding-and-boundary".to_string(),
        passed: [
            "Document RAG MVP",
            "chunk/snippet level",
            "Image retrieval uses OCR-derived text",
            "Retrieval combines lexical and vector signals",
            "Natural-language `general.message` can also enter retrieval",
        ]
        .iter()
        .all(|item| document_rag_doc.contains(item)),
        details: "Document RAG note must describe chunk grounding, OCR-backed image retrieval, and the planner-routed retrieval path.".to_string(),
        skipped: None,
    });

    checks.push(CheckResult {
        name: "local-model-benchmark-gate:openai-seam-and-candle-policy".to_string(),
        passed: [
            "Local Model Backend Benchmark Gate",
            "local OpenAI-compatible API seam is frozen",
            "Candle is a preferred backend candidate, not the frozen contract.",
            "keep `openai_proxy` as the default backend",
            "cold start",
            "Chinese chat probes",
            "embedding retrieval quality relative to a lexical baseline",
        ]
        .iter()
        .all(|item| local_model_benchmark_doc.contains(item)),
        details: "Local model benchmark gate doc must freeze the OpenAI-compatible seam and document the Candle promotion criteria.".to_string(),
        skipped: None,
    });

    checks
}

pub fn build_live_checks(config: &IntegrationConfig) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    let middleware = MiddlewareClient::new(config.clone());
    if middleware.is_available() {
        match middleware.get_methods("REST") {
            Ok((methods, _)) => {
                for method_name in REQUIRED_MIDDLEWARE_METHODS {
                    checks.push(CheckResult {
                        name: format!("middleware-method:{method_name}"),
                        passed: methods.contains_key(method_name),
                        skipped: Some(false),
                        details: "Checked with core.get_methods target=REST.".to_string(),
                    });
                }
            }
            Err(err) => checks.push(CheckResult {
                name: "middleware-live-probe".to_string(),
                passed: false,
                skipped: Some(false),
                details: err.to_string(),
            }),
        }
    } else {
        checks.push(CheckResult {
            name: "middleware-live-probe".to_string(),
            passed: false,
            skipped: Some(true),
            details: format!("middleware binary not found: {}", config.middleware_bin),
        });
    }

    let midcli = MidcliClient::new(config.clone());
    if midcli.is_available() {
        match midcli.run_csv_query(&default_midcli_service_query(config)) {
            Ok((rows, result)) => checks.push(CheckResult {
                name: "midcli-service-query".to_string(),
                passed: !rows.is_empty() || result.stdout.to_ascii_lowercase().contains("service"),
                skipped: Some(false),
                details: default_midcli_service_query(config),
            }),
            Err(err) => checks.push(CheckResult {
                name: "midcli-service-query".to_string(),
                passed: false,
                skipped: Some(false),
                details: err.to_string(),
            }),
        }
    } else {
        checks.push(CheckResult {
            name: "midcli-service-query".to_string(),
            passed: false,
            skipped: Some(true),
            details: format!("midcli binary not found: {}", config.midcli_bin),
        });
    }

    checks
}

pub fn run_validate(
    root: &Path,
    config: &IntegrationConfig,
    skip_live: bool,
    require_live: bool,
) -> ValidateReport {
    let mut checks = build_checks(root);
    if !skip_live {
        checks.extend(build_live_checks(config));
    }

    let mut passed = checks
        .iter()
        .all(|check| check.passed || check.skipped == Some(true));
    let live_executed = checks.iter().any(|check| {
        (check.name.starts_with("middleware-") || check.name.starts_with("midcli-"))
            && check.skipped != Some(true)
    });

    if require_live && !live_executed {
        passed = false;
        checks.push(CheckResult {
            name: "live-probe-required".to_string(),
            passed: false,
            skipped: Some(false),
            details: "--require-live was set but no live middleware/midcli probe executed."
                .to_string(),
        });
    }

    ValidateReport {
        mode: if live_executed {
            "live-integration".to_string()
        } else {
            "spec-scaffold".to_string()
        },
        passed,
        check_count: checks.len(),
        checks,
    }
}
