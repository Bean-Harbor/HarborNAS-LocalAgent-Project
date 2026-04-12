use serde::{Deserialize, Serialize};

use crate::orchestrator::contracts::{Action, RiskLevel, Route};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerIntent {
    pub domain: String,
    pub operation: String,
    #[serde(default)]
    pub resource: serde_json::Value,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedStep {
    pub action: Action,
    pub route_candidates: Vec<Route>,
}

pub fn plan_task(intent: PlannerIntent) -> Vec<PlannedStep> {
    let mut action = Action {
        domain: intent.domain,
        operation: intent.operation,
        resource: intent.resource,
        args: intent.args,
        risk_level: RiskLevel::Low,
        requires_approval: false,
        dry_run: false,
    };

    action.risk_level = match action.domain.as_str() {
        "service" => match action.operation.as_str() {
            "status" => RiskLevel::Low,
            "start" | "enable" => RiskLevel::Medium,
            "stop" | "restart" => RiskLevel::High,
            _ => RiskLevel::Low,
        },
        "files" => match action.operation.as_str() {
            "search" => RiskLevel::Low,
            "copy" => {
                let overwrite = action
                    .args
                    .get("overwrite")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if overwrite {
                    RiskLevel::High
                } else {
                    RiskLevel::Medium
                }
            }
            "move" => RiskLevel::High,
            "archive" => RiskLevel::Medium,
            _ => RiskLevel::Low,
        },
        _ => RiskLevel::Low,
    };
    action.normalize();

    let route_candidates = if matches!(action.domain.as_str(), "service" | "files") {
        vec![Route::MiddlewareApi, Route::Midcli]
    } else {
        vec![
            Route::MiddlewareApi,
            Route::Midcli,
            Route::Browser,
            Route::Mcp,
        ]
    };

    vec![PlannedStep {
        action,
        route_candidates,
    }]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{plan_task, PlannerIntent};
    use crate::orchestrator::contracts::{RiskLevel, Route};

    #[test]
    fn planner_keeps_control_plane_route_priority_for_service() {
        let steps = plan_task(PlannerIntent {
            domain: "service".to_string(),
            operation: "status".to_string(),
            resource: serde_json::json!({"service_name": "ssh"}),
            args: json!({}),
        });

        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].route_candidates,
            vec![Route::MiddlewareApi, Route::Midcli]
        );
    }

    #[test]
    fn planner_marks_restart_as_high_risk() {
        let steps = plan_task(PlannerIntent {
            domain: "service".to_string(),
            operation: "restart".to_string(),
            resource: serde_json::json!({"service_name": "ssh"}),
            args: json!({}),
        });

        assert_eq!(steps[0].action.risk_level, RiskLevel::High);
        assert!(steps[0].action.requires_approval);
    }
}
