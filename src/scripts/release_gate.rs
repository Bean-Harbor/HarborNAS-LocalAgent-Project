use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseGateSummary {
    pub mode: String,
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub evaluated_rows: usize,
}

pub fn evaluate_release_gate(report: &serde_json::Value, require_live: bool) -> ReleaseGateSummary {
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

    ReleaseGateSummary {
        mode: report
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("spec-scaffold")
            .to_string(),
        allowed: reasons.is_empty(),
        reasons,
        evaluated_rows: rows.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate_release_gate;

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
}
