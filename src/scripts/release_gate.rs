use serde::{Deserialize, Serialize};

use crate::scripts::model_benchmark::LocalModelBenchmarkReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseGateSummary {
    pub mode: String,
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub evaluated_rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_benchmark: Option<ReleaseGateModelBenchmark>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseGateModelBenchmark {
    pub backend: String,
    pub promotable: bool,
    pub mode: String,
    pub reasons: Vec<String>,
}

pub fn evaluate_release_gate(report: &serde_json::Value, require_live: bool) -> ReleaseGateSummary {
    evaluate_release_gate_with_model_benchmark(report, require_live, None, false)
}

pub fn evaluate_release_gate_with_model_benchmark(
    report: &serde_json::Value,
    require_live: bool,
    model_benchmark: Option<&serde_json::Value>,
    require_model_benchmark: bool,
) -> ReleaseGateSummary {
    let rows = report
        .get("rows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let docs_missing = report
        .get("docs_missing")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let blocking_rows = rows
        .iter()
        .filter(|row| row.get("blocking").and_then(|v| v.as_bool()) == Some(true))
        .count();

    let mut reasons = Vec::new();
    if !docs_missing.is_empty() {
        reasons.push("required contract documents are missing".to_string());
    }
    if blocking_rows > 0 {
        reasons.push("drift matrix contains blocking rows".to_string());
    }
    if require_live && report.get("mode").and_then(|v| v.as_str()) != Some("live-integration") {
        reasons.push("live middleware or midcli probes were not executed".to_string());
    }

    let parsed_model_benchmark = model_benchmark.map(|value| {
        serde_json::from_value::<LocalModelBenchmarkReport>(value.clone())
            .map_err(|error| format!("local model benchmark report is invalid: {error}"))
    });

    let model_benchmark_summary = match parsed_model_benchmark {
        Some(Ok(report)) => Some(ReleaseGateModelBenchmark {
            backend: report.backend,
            promotable: report.gate.promotable,
            mode: report.mode,
            reasons: report.gate.reasons,
        }),
        Some(Err(error)) => {
            reasons.push(error);
            None
        }
        None => None,
    };

    if require_model_benchmark {
        match model_benchmark_summary.as_ref() {
            Some(summary) if summary.promotable => {}
            Some(_) => reasons.push("local model benchmark gate did not pass".to_string()),
            None => reasons.push("local model benchmark evidence is missing".to_string()),
        }
    }

    ReleaseGateSummary {
        mode: report
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("spec-scaffold")
            .to_string(),
        allowed: reasons.is_empty(),
        reasons,
        evaluated_rows: rows.len(),
        model_benchmark: model_benchmark_summary,
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate_release_gate, evaluate_release_gate_with_model_benchmark};

    #[test]
    fn release_gate_requires_live_when_requested() {
        let report = serde_json::json!({"mode": "spec-scaffold", "rows": [], "docs_missing": []});
        let payload = evaluate_release_gate(&report, true);
        assert!(!payload.allowed);
        assert!(payload
            .reasons
            .iter()
            .any(|r| r == "live middleware or midcli probes were not executed"));
    }

    #[test]
    fn release_gate_requires_model_benchmark_when_requested() {
        let report = serde_json::json!({"mode": "live-integration", "rows": [], "docs_missing": []});
        let payload = evaluate_release_gate_with_model_benchmark(&report, true, None, true);
        assert!(!payload.allowed);
        assert!(payload
            .reasons
            .iter()
            .any(|r| r == "local model benchmark evidence is missing"));
    }

    #[test]
    fn release_gate_records_model_benchmark_summary() {
        let report = serde_json::json!({"mode": "live-integration", "rows": [], "docs_missing": []});
        let model_benchmark = serde_json::json!({
            "schema_version": 1,
            "contract": "local-openai-compatible-api",
            "mode": "spawned",
            "backend": "candle",
            "base_url": "http://127.0.0.1:4176/v1",
            "healthz_url": "http://127.0.0.1:4176/healthz",
            "generated_at_utc": "unix-epoch-ms:1",
            "cold_start": {
                "measured": true,
                "ready_within_timeout": true,
                "timeout_ms": 45000,
                "attempts": 2,
                "elapsed_ms": 1000
            },
            "health": {
                "ok": true,
                "ready": true,
                "http_status": 200,
                "service": "harbor-model-api",
                "status": "ok",
                "backend_kind": "candle",
                "backend_ready": true
            },
            "chat": {
                "passed": true,
                "passed_count": 3,
                "required_passes": 3,
                "probes": []
            },
            "embeddings": {
                "passed": true,
                "vector_probe_ok": true,
                "retrieval_probe_ok": true,
                "consistent_dimensions": true,
                "vector_dimensions": 384,
                "lexical_mrr": 0.3,
                "embedding_mrr": 0.9,
                "improvement_over_lexical": 0.6,
                "probes": [],
                "retrieval_cases": []
            },
            "gate": {
                "promotable": true,
                "reasons": [],
                "recommendation": "ok"
            }
        });

        let payload =
            evaluate_release_gate_with_model_benchmark(&report, true, Some(&model_benchmark), true);
        assert!(payload.allowed);
        let summary = payload.model_benchmark.expect("summary");
        assert_eq!(summary.backend, "candle");
        assert!(summary.promotable);
    }
}
