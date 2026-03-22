use std::process::Command;
use std::time::Instant;

use serde_json::json;

use crate::orchestrator::contracts::{Action, ExecutionResult, Route, StepStatus};
use crate::orchestrator::router::Executor;

pub struct MiddlewareExecutor {
    available: bool,
}

impl MiddlewareExecutor {
    pub fn new(available: bool) -> Self {
        Self { available }
    }
}

impl Executor for MiddlewareExecutor {
    fn route(&self) -> Route {
        Route::MiddlewareApi
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String> {
        let started = Instant::now();

        if std::env::var("HARBOR_FORCE_MIDDLEWARE_ERROR").ok().as_deref() == Some("1") {
            return Err("forced middleware failure".to_string());
        }

        let service_name = action
            .resource
            .get("service_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (method, call_args) = map_service_operation(&action.operation, service_name, &action.args)?;

        Ok(ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: Route::MiddlewareApi.as_str().to_string(),
            fallback_used: false,
            status: StepStatus::Success,
            duration_ms: started.elapsed().as_millis() as u64,
            error_code: None,
            error_message: None,
            audit_ref: String::new(),
            result_payload: json!({
                "method": method,
                "args": call_args,
                "note": "middleware_api preview mode",
            }),
        })
    }
}

pub struct MidcliExecutor {
    available: bool,
    bin: String,
    passthrough: bool,
}

impl MidcliExecutor {
    pub fn new(available: bool, bin: String, passthrough: bool) -> Self {
        Self {
            available,
            bin,
            passthrough,
        }
    }
}

impl Executor for MidcliExecutor {
    fn route(&self) -> Route {
        Route::Midcli
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String> {
        let started = Instant::now();
        let service_name = action
            .resource
            .get("service_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let command = build_midcli_command(&action.operation, service_name, &action.args)?;

        let payload = if self.passthrough {
            let output = Command::new(&self.bin)
                .args(command.iter())
                .output()
                .map_err(|e| format!("midcli spawn error: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "midcli command failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            json!({
                "stdout": String::from_utf8_lossy(&output.stdout),
                "command": command,
                "passthrough": true,
            })
        } else {
            json!({
                "command": command,
                "passthrough": false,
                "note": "midcli preview mode",
            })
        };

        Ok(ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: Route::Midcli.as_str().to_string(),
            fallback_used: false,
            status: StepStatus::Success,
            duration_ms: started.elapsed().as_millis() as u64,
            error_code: None,
            error_message: None,
            audit_ref: String::new(),
            result_payload: payload,
        })
    }
}

fn map_service_operation(
    operation: &str,
    service_name: &str,
    args: &serde_json::Value,
) -> Result<(String, serde_json::Value), String> {
    match operation {
        "status" => Ok(("service.query".to_string(), json!([service_name]))),
        "start" | "stop" | "restart" => Ok((
            "service.control".to_string(),
            json!([operation.to_uppercase(), service_name, {}]),
        )),
        "enable" => {
            let enable_val = args.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
            Ok((
                "service.update".to_string(),
                json!([service_name, {"enable": enable_val}]),
            ))
        }
        _ => Err(format!("Unmapped service operation: {operation}")),
    }
}

fn build_midcli_command(
    operation: &str,
    service_name: &str,
    args: &serde_json::Value,
) -> Result<Vec<String>, String> {
    match operation {
        "status" => Ok(vec!["service".to_string(), service_name.to_string(), "show".to_string()]),
        "start" => Ok(vec![
            "service".to_string(),
            "start".to_string(),
            format!("service={service_name}"),
        ]),
        "stop" => Ok(vec![
            "service".to_string(),
            "stop".to_string(),
            format!("service={service_name}"),
        ]),
        "restart" => Ok(vec![
            "service".to_string(),
            "restart".to_string(),
            format!("service={service_name}"),
        ]),
        "enable" => {
            let enable = args.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
            Ok(vec![
                "service".to_string(),
                "update".to_string(),
                format!("id_or_name={service_name}"),
                format!("enable={enable}"),
            ])
        }
        _ => Err(format!("Unmapped midcli operation: {operation}")),
    }
}
