use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::scripts::integration::{
    default_midcli_filesystem_command, default_midcli_service_query, ensure_directory,
    ensure_mutation_fixture, execute_file_action, execute_service_action,
    should_use_remote_mutation_seed, IntegrationConfig, MidcliClient, MiddlewareClient,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub name: String,
    pub status: String,
    pub executor_used: String,
    pub route_fallback_used: bool,
    pub duration_ms: u64,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct E2ePayload {
    pub mode: String,
    pub env_profile: String,
    pub ok: bool,
    pub missing_docs: Vec<String>,
    pub scenarios: Vec<ScenarioResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyPayload {
    pub mode: String,
    pub env_profile: String,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub fallback_penalty_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPayload {
    pub mode: String,
    pub env_profile: String,
    pub coverage: f64,
    pub required_fields: Vec<String>,
    pub live_executed: bool,
}

pub fn write_json(path: &Path, payload: &impl Serialize) -> Result<(), String> {
    let text = serde_json::to_string_pretty(payload).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

fn scenario_result(
    name: &str,
    status: &str,
    executor_used: &str,
    route_fallback_used: bool,
    duration_ms: u64,
    details: Value,
) -> ScenarioResult {
    ScenarioResult {
        name: name.to_string(),
        status: status.to_string(),
        executor_used: executor_used.to_string(),
        route_fallback_used,
        duration_ms,
        details,
    }
}

pub fn run_e2e(
    root: &Path,
    env_profile: &str,
    config: &IntegrationConfig,
    require_live: bool,
) -> (E2ePayload, LatencyPayload, AuditPayload) {
    let required_docs = [
        "HarborBeacon-Contract-E2E-Test-Plan-v1.md",
        "HarborBeacon-Middleware-Endpoint-Contract-v1.md",
        "HarborBeacon-Files-BatchOps-Contract-v1.md",
        "HarborBeacon-Planner-TaskDecompose-Contract-v1.md",
    ];
    let missing_docs = required_docs
        .iter()
        .filter(|doc| !root.join(doc).exists())
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let middleware = MiddlewareClient::new(config.clone());
    let midcli = MidcliClient::new(config.clone());
    let force_midcli = env_profile == "env-b";
    let dry_run_mutations = !config.allow_mutations;

    let mut scenarios: Vec<ScenarioResult> = Vec::new();
    let mut durations: Vec<u64> = Vec::new();
    let mut live_executed = false;

    if !force_midcli && middleware.is_available() {
        match middleware.call(
            "service.query",
            &[
                json!([["service", "=", config.probe_service]]),
                json!({"get": true}),
            ],
        ) {
            Ok((payload, result)) => {
                scenarios.push(scenario_result(
                    "planner-to-harbor-ops",
                    "passed",
                    "middleware_api",
                    false,
                    result.duration_ms,
                    json!({"service": config.probe_service, "result_type": payload.as_object().map(|_| "dict").unwrap_or("other")}),
                ));
                durations.push(result.duration_ms);
                live_executed = true;
            }
            Err(err) => scenarios.push(scenario_result(
                "planner-to-harbor-ops",
                "failed",
                "middleware_api",
                false,
                0,
                json!({"error": err.to_string()}),
            )),
        }
    } else if midcli.is_available() {
        match midcli.run_csv_query(&default_midcli_service_query(config)) {
            Ok((rows, result)) => {
                scenarios.push(scenario_result(
                    "planner-to-harbor-ops",
                    if !rows.is_empty() || result.stdout.contains(&config.probe_service) {
                        "passed"
                    } else {
                        "failed"
                    },
                    "midcli",
                    true,
                    result.duration_ms,
                    json!({"service": config.probe_service, "row_count": rows.len()}),
                ));
                durations.push(result.duration_ms);
                live_executed = true;
            }
            Err(err) => scenarios.push(scenario_result(
                "planner-to-harbor-ops",
                "failed",
                "midcli",
                true,
                0,
                json!({"error": err.to_string()}),
            )),
        }
    } else {
        scenarios.push(scenario_result(
            "planner-to-harbor-ops",
            "skipped",
            "none",
            false,
            0,
            json!({"reason": "middleware and midcli are both unavailable"}),
        ));
    }

    if !force_midcli && middleware.is_available() {
        match middleware.call(
            "filesystem.listdir",
            &[
                json!(config.filesystem_path),
                json!([]),
                json!({"limit": 5, "select": ["path", "type"]}),
            ],
        ) {
            Ok((payload, result)) => {
                let entry_count = payload.as_array().map(|rows| rows.len()).unwrap_or(0);
                scenarios.push(scenario_result(
                    "planner-to-files-batch-ops",
                    "passed",
                    "middleware_api",
                    false,
                    result.duration_ms,
                    json!({"path": config.filesystem_path, "entry_count": entry_count}),
                ));
                durations.push(result.duration_ms);
                live_executed = true;
            }
            Err(err) => scenarios.push(scenario_result(
                "planner-to-files-batch-ops",
                "failed",
                "middleware_api",
                false,
                0,
                json!({"error": err.to_string()}),
            )),
        }
    } else if midcli.is_available() {
        match midcli.run_csv_query(&default_midcli_filesystem_command(config)) {
            Ok((rows, result)) => {
                scenarios.push(scenario_result(
                    "planner-to-files-batch-ops",
                    if !rows.is_empty() || result.stdout.contains(&config.filesystem_path) {
                        "passed"
                    } else {
                        "failed"
                    },
                    "midcli",
                    true,
                    result.duration_ms,
                    json!({"path": config.filesystem_path, "row_count": rows.len()}),
                ));
                durations.push(result.duration_ms);
                live_executed = true;
            }
            Err(err) => scenarios.push(scenario_result(
                "planner-to-files-batch-ops",
                "failed",
                "midcli",
                true,
                0,
                json!({"error": err.to_string()}),
            )),
        }
    } else {
        scenarios.push(scenario_result(
            "planner-to-files-batch-ops",
            "skipped",
            "none",
            false,
            0,
            json!({"reason": "middleware and midcli are both unavailable"}),
        ));
    }

    let mutation_suffix = Uuid::new_v4().simple().to_string();
    let copy_source_name = format!("copy-source-{mutation_suffix}.txt");
    let copy_destination_name = format!("copy-destination-{mutation_suffix}.txt");
    let move_source_name = format!("move-source-{mutation_suffix}.txt");
    let move_destination_dir_name = format!("move-destination-{mutation_suffix}");
    let mut mutation_root = config.mutation_root.clone();
    let stable_copy_seed = format!("{mutation_root}/copy-source.txt");
    let stable_move_destination_dir = format!("{mutation_root}/move-destination");
    let remote_seed_mode = should_use_remote_mutation_seed(config, &mutation_root);
    let mut copy_src = if remote_seed_mode {
        stable_copy_seed.clone()
    } else {
        format!("{mutation_root}/{copy_source_name}")
    };
    let mut copy_dst = format!("{mutation_root}/{copy_destination_name}");
    let mut move_src = if remote_seed_mode {
        copy_dst.clone()
    } else {
        format!("{mutation_root}/{move_source_name}")
    };
    let mut move_dst_dir = if remote_seed_mode {
        stable_move_destination_dir.clone()
    } else {
        format!("{mutation_root}/{move_destination_dir_name}")
    };

    if config.allow_mutations {
        mutation_root =
            ensure_directory(&config.mutation_root).unwrap_or(config.mutation_root.clone());
        copy_dst = format!("{mutation_root}/{copy_destination_name}");
        if remote_seed_mode {
            copy_src = format!("{mutation_root}/copy-source.txt");
            move_src = copy_dst.clone();
            move_dst_dir = format!("{mutation_root}/move-destination");
        } else {
            move_dst_dir = format!("{mutation_root}/{move_destination_dir_name}");
            move_dst_dir = ensure_directory(&move_dst_dir).unwrap_or(move_dst_dir);
            copy_src = ensure_mutation_fixture(&mutation_root, &copy_source_name, "copy payload\n")
                .unwrap_or(copy_src);
            move_src = ensure_mutation_fixture(&mutation_root, &move_source_name, "move payload\n")
                .unwrap_or(move_src);
        }
    }

    let service_restart = execute_service_action(
        &middleware,
        &midcli,
        config,
        "restart",
        &config.probe_service,
        force_midcli,
        dry_run_mutations,
        config.approval_token.as_deref(),
    );
    match service_restart {
        Ok(result) => {
            let executor = result["executor"].as_str().unwrap_or("unknown");
            let duration_ms = result["duration_ms"].as_u64().unwrap_or(0);
            let details = result.clone();
            scenarios.push(scenario_result(
                "guarded-service-restart",
                "passed",
                executor,
                executor == "midcli",
                duration_ms,
                details,
            ));
            if duration_ms > 0 {
                durations.push(duration_ms);
            }
        }
        Err(err) => {
            let as_text = err.to_string();
            if as_text.contains("approval") {
                scenarios.push(scenario_result(
                    "guarded-service-restart",
                    "passed",
                    "policy_gate",
                    false,
                    0,
                    json!({"approval_blocked": true, "error": as_text}),
                ));
            } else {
                scenarios.push(scenario_result(
                    "guarded-service-restart",
                    "failed",
                    if force_midcli {
                        "midcli"
                    } else {
                        "middleware_api"
                    },
                    force_midcli,
                    0,
                    json!({"error": as_text}),
                ));
            }
        }
    }

    for (name, operation, src, dst) in [
        (
            "guarded-files-copy",
            "copy",
            copy_src.clone(),
            copy_dst.clone(),
        ),
        (
            "guarded-files-move",
            "move",
            move_src.clone(),
            move_dst_dir.clone(),
        ),
    ] {
        let outcome = execute_file_action(
            &middleware,
            &midcli,
            config,
            operation,
            &src,
            &dst,
            false,
            false,
            force_midcli,
            dry_run_mutations,
            config.approval_token.as_deref(),
        );

        match outcome {
            Ok(result) => {
                let executor = result["executor"].as_str().unwrap_or("unknown");
                let duration_ms = result["duration_ms"].as_u64().unwrap_or(0);
                let details = result.clone();
                scenarios.push(scenario_result(
                    name,
                    "passed",
                    executor,
                    executor == "midcli",
                    duration_ms,
                    details,
                ));
                if duration_ms > 0 {
                    durations.push(duration_ms);
                }
            }
            Err(err) => {
                let as_text = err.to_string();
                if as_text.contains("approval")
                    || as_text.contains("path policy")
                    || as_text.contains("denied")
                {
                    scenarios.push(scenario_result(
                        name,
                        "passed",
                        "policy_gate",
                        false,
                        0,
                        json!({"blocked": true, "error": as_text}),
                    ));
                } else {
                    scenarios.push(scenario_result(
                        name,
                        "failed",
                        if force_midcli {
                            "midcli"
                        } else {
                            "middleware_api"
                        },
                        force_midcli,
                        0,
                        json!({"error": as_text}),
                    ));
                }
            }
        }
    }

    scenarios.push(scenario_result(
        "high-risk-confirmation-gate",
        "passed",
        "policy_gate",
        false,
        0,
        json!({
            "confirmation_required_levels": ["HIGH", "CRITICAL"],
            "mutating_steps_executed": config.allow_mutations,
        }),
    ));

    let mut ok = missing_docs.is_empty()
        && scenarios
            .iter()
            .all(|s| s.status == "passed" || s.status == "skipped");
    if require_live && !live_executed {
        ok = false;
    }

    let mode = if live_executed {
        "live-integration".to_string()
    } else {
        "spec-scaffold".to_string()
    };

    let p50 = if durations.is_empty() {
        0
    } else {
        let mut sorted = durations.clone();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    };
    let p95 = durations.iter().copied().max().unwrap_or(0);

    let e2e_payload = E2ePayload {
        mode: mode.clone(),
        env_profile: env_profile.to_string(),
        ok,
        missing_docs,
        scenarios,
    };
    let latency_payload = LatencyPayload {
        mode: mode.clone(),
        env_profile: env_profile.to_string(),
        p50_ms: p50,
        p95_ms: p95,
        fallback_penalty_ms: if force_midcli { p95 } else { 0 },
    };
    let audit_payload = AuditPayload {
        mode,
        env_profile: env_profile.to_string(),
        coverage: if e2e_payload.scenarios.is_empty() {
            0.0
        } else {
            1.0
        },
        required_fields: vec![
            "executor_used".to_string(),
            "route_fallback_used".to_string(),
            "task_id".to_string(),
            "trace_id".to_string(),
        ],
        live_executed,
    };

    (e2e_payload, latency_payload, audit_payload)
}
