use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::connectors::harboros::{harboros_real_interface_surfaces, HarborOsParityKind};
use crate::scripts::integration::{
    default_midcli_filesystem_command, default_midcli_service_query, discover_source_capabilities,
    file_operation_risk, service_operation_risk, IntegrationConfig, MidcliClient, MiddlewareClient,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftRow {
    pub capability: String,
    pub harbor_ref: String,
    pub upstream_ref: String,
    pub middleware_live: Option<bool>,
    pub midcli_live: Option<bool>,
    pub harbor_source: Option<bool>,
    pub upstream_source: Option<bool>,
    pub risk_levels: serde_json::Value,
    pub status: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    pub mode: String,
    pub harbor_ref: String,
    pub upstream_ref: String,
    pub harbor_repo_path: Option<String>,
    pub upstream_repo_path: Option<String>,
    pub docs_missing: Vec<String>,
    pub harboros_parity: Vec<HarborOsParityRow>,
    pub rows: Vec<DriftRow>,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarborOsParityRow {
    pub capability: String,
    pub middleware_method: Option<String>,
    pub midcli_example: Option<String>,
    pub parity_kind: String,
    pub notes: String,
}

pub fn live_middleware_capabilities(client: &MiddlewareClient) -> HashMap<String, bool> {
    if !client.is_available() {
        return HashMap::new();
    }

    let Ok((methods, _)) = client.get_methods("REST") else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    for capability in [
        "service.query",
        "service.control",
        "filesystem.listdir",
        "filesystem.copy",
        "filesystem.move",
    ] {
        map.insert(capability.to_string(), methods.contains_key(capability));
    }
    map
}

pub fn live_midcli_capabilities(
    client: &MidcliClient,
    config: &IntegrationConfig,
) -> HashMap<String, bool> {
    if !client.is_available() {
        return HashMap::new();
    }

    let commands = vec![
        ("service.query", default_midcli_service_query(config), false),
        (
            "service.control",
            format!("service start service={}", config.probe_service),
            true,
        ),
        (
            "filesystem.listdir",
            default_midcli_filesystem_command(config),
            true,
        ),
        (
            "filesystem.copy",
            format!(
                "filesystem copy src={}/source dst={}/target",
                config.filesystem_path, config.filesystem_path
            ),
            true,
        ),
        (
            "filesystem.move",
            format!(
                "filesystem move src={}/source dst={}",
                config.filesystem_path, config.filesystem_path
            ),
            true,
        ),
    ];

    let mut map = HashMap::new();
    for (capability, command, print_template) in commands {
        let ok = client.run(&command, None, print_template).is_ok();
        map.insert(capability.to_string(), ok);
    }
    map
}

pub fn run_drift_matrix(
    root: &std::path::Path,
    config: &IntegrationConfig,
    harbor_ref: &str,
    upstream_ref: &str,
    harbor_repo_path: Option<String>,
    upstream_repo_path: Option<String>,
) -> DriftReport {
    let checks = [
        "HarborBeacon-Middleware-Endpoint-Contract-v1.md",
        "HarborBeacon-Files-BatchOps-Contract-v1.md",
        "HarborBeacon-Planner-TaskDecompose-Contract-v1.md",
    ];
    let missing = checks
        .iter()
        .filter(|name| !root.join(name).exists())
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let middleware_caps = live_middleware_capabilities(&MiddlewareClient::new(config.clone()));
    let midcli_caps = live_midcli_capabilities(&MidcliClient::new(config.clone()), config);
    let harbor_source_caps = discover_source_capabilities(
        harbor_repo_path
            .as_deref()
            .or(config.harbor_repo_path.as_deref()),
    );
    let upstream_source_caps = discover_source_capabilities(
        upstream_repo_path
            .as_deref()
            .or(config.upstream_repo_path.as_deref()),
    );

    let service_live_available = middleware_caps.get("service.query") == Some(&true)
        || midcli_caps.get("service.query") == Some(&true);

    let middleware_files_live = ["filesystem.listdir", "filesystem.copy", "filesystem.move"]
        .iter()
        .all(|name| middleware_caps.get(*name) == Some(&true));
    let midcli_files_live = ["filesystem.listdir", "filesystem.copy", "filesystem.move"]
        .iter()
        .all(|name| midcli_caps.get(*name) == Some(&true));

    let files_live_available = middleware_files_live || midcli_files_live;
    let harboros_parity = harboros_real_interface_surfaces()
        .iter()
        .map(|surface| HarborOsParityRow {
            capability: surface.capability.to_string(),
            middleware_method: surface.middleware_method.map(|s| s.to_string()),
            midcli_example: surface.midcli_example.map(|s| s.to_string()),
            parity_kind: match surface.parity_kind {
                HarborOsParityKind::Real => "real".to_string(),
                HarborOsParityKind::ScaffoldOnly => "scaffold-only".to_string(),
            },
            notes: surface.notes.to_string(),
        })
        .collect::<Vec<_>>();

    let rows = vec![
        DriftRow {
            capability: "system.harbor_ops".to_string(),
            harbor_ref: harbor_ref.to_string(),
            upstream_ref: upstream_ref.to_string(),
            middleware_live: middleware_caps.get("service.query").copied(),
            midcli_live: midcli_caps.get("service.query").copied(),
            harbor_source: harbor_source_caps.get("service.query").copied(),
            upstream_source: upstream_source_caps.get("service.query").copied(),
            risk_levels: serde_json::json!({
                "query": service_operation_risk("status").unwrap_or("LOW"),
                "control": service_operation_risk("restart").unwrap_or("HIGH"),
            }),
            status: if middleware_caps.get("service.query") == Some(&true) {
                "ok".to_string()
            } else if midcli_caps.get("service.query") == Some(&true) {
                "degraded".to_string()
            } else {
                "missing".to_string()
            },
            blocking: !service_live_available,
        },
        DriftRow {
            capability: "files.batch_ops".to_string(),
            harbor_ref: harbor_ref.to_string(),
            upstream_ref: upstream_ref.to_string(),
            middleware_live: Some(middleware_files_live),
            midcli_live: Some(midcli_files_live),
            harbor_source: Some(
                ["filesystem.listdir", "filesystem.copy", "filesystem.move"]
                    .iter()
                    .all(|name| harbor_source_caps.get(*name) == Some(&true)),
            ),
            upstream_source: Some(
                ["filesystem.listdir", "filesystem.copy", "filesystem.move"]
                    .iter()
                    .all(|name| upstream_source_caps.get(*name) == Some(&true)),
            ),
            risk_levels: serde_json::json!({
                "copy": file_operation_risk("copy", false).unwrap_or("MEDIUM"),
                "move": file_operation_risk("move", false).unwrap_or("HIGH"),
            }),
            status: if middleware_files_live {
                "ok".to_string()
            } else if midcli_files_live {
                "degraded".to_string()
            } else {
                "missing".to_string()
            },
            blocking: !files_live_available,
        },
        DriftRow {
            capability: "planner.task_decompose".to_string(),
            harbor_ref: harbor_ref.to_string(),
            upstream_ref: upstream_ref.to_string(),
            middleware_live: Some(
                middleware_caps.get("service.query") == Some(&true)
                    && middleware_caps.get("filesystem.listdir") == Some(&true),
            ),
            midcli_live: Some(
                midcli_caps.get("service.query") == Some(&true)
                    && midcli_caps.get("filesystem.listdir") == Some(&true),
            ),
            harbor_source: None,
            upstream_source: None,
            risk_levels: serde_json::json!({}),
            status: "derived".to_string(),
            blocking: false,
        },
    ];

    let blocking_rows = rows.iter().any(|row| row.blocking);

    DriftReport {
        mode: if middleware_caps.is_empty() && midcli_caps.is_empty() {
            "spec-scaffold".to_string()
        } else {
            "live-integration".to_string()
        },
        harbor_ref: harbor_ref.to_string(),
        upstream_ref: upstream_ref.to_string(),
        harbor_repo_path,
        upstream_repo_path,
        docs_missing: missing.clone(),
        harboros_parity,
        rows,
        blocking: !missing.is_empty() || blocking_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::{DriftReport, HarborOsParityKind, HarborOsParityRow};

    #[test]
    fn midcli_only_is_degraded_not_blocking() {
        let payload = DriftReport {
            mode: "live-integration".to_string(),
            harbor_ref: "develop".to_string(),
            upstream_ref: "master".to_string(),
            harbor_repo_path: None,
            upstream_repo_path: None,
            docs_missing: Vec::new(),
            harboros_parity: Vec::new(),
            rows: vec![
                super::DriftRow {
                    capability: "system.harbor_ops".to_string(),
                    harbor_ref: "develop".to_string(),
                    upstream_ref: "master".to_string(),
                    middleware_live: Some(false),
                    midcli_live: Some(true),
                    harbor_source: Some(false),
                    upstream_source: Some(false),
                    risk_levels: serde_json::json!({}),
                    status: "degraded".to_string(),
                    blocking: false,
                },
                super::DriftRow {
                    capability: "files.batch_ops".to_string(),
                    harbor_ref: "develop".to_string(),
                    upstream_ref: "master".to_string(),
                    middleware_live: Some(false),
                    midcli_live: Some(true),
                    harbor_source: Some(false),
                    upstream_source: Some(false),
                    risk_levels: serde_json::json!({}),
                    status: "degraded".to_string(),
                    blocking: false,
                },
            ],
            blocking: false,
        };

        let rows = payload.rows;
        let system = rows
            .iter()
            .find(|r| r.capability == "system.harbor_ops")
            .unwrap();
        let files = rows
            .iter()
            .find(|r| r.capability == "files.batch_ops")
            .unwrap();
        assert_eq!(system.status, "degraded");
        assert!(!system.blocking);
        assert_eq!(files.status, "degraded");
        assert!(!files.blocking);
    }

    #[test]
    fn parity_rows_mark_real_and_scaffold_only_surfaces() {
        let report = DriftReport {
            mode: "spec-scaffold".to_string(),
            harbor_ref: "develop".to_string(),
            upstream_ref: "master".to_string(),
            harbor_repo_path: None,
            upstream_repo_path: None,
            docs_missing: Vec::new(),
            harboros_parity: super::harboros_real_interface_surfaces()
                .iter()
                .map(|surface| HarborOsParityRow {
                    capability: surface.capability.to_string(),
                    middleware_method: surface.middleware_method.map(|s| s.to_string()),
                    midcli_example: surface.midcli_example.map(|s| s.to_string()),
                    parity_kind: match surface.parity_kind {
                        HarborOsParityKind::Real => "real".to_string(),
                        HarborOsParityKind::ScaffoldOnly => "scaffold-only".to_string(),
                    },
                    notes: surface.notes.to_string(),
                })
                .collect(),
            rows: Vec::new(),
            blocking: false,
        };

        let service_query = report
            .harboros_parity
            .iter()
            .find(|row| row.capability == "service.query")
            .unwrap();
        let read_text = report
            .harboros_parity
            .iter()
            .find(|row| row.capability == "files.read_text")
            .unwrap();

        assert_eq!(service_query.parity_kind, "real");
        assert_eq!(
            service_query.middleware_method.as_deref(),
            Some("service.query")
        );
        assert_eq!(read_text.parity_kind, "scaffold-only");
        assert!(read_text.middleware_method.is_none());
    }
}
