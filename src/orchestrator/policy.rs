use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::orchestrator::contracts::{Action, RiskLevel};

const SUPPORTED_SERVICE_OPS: [&str; 5] = ["status", "start", "stop", "restart", "enable"];
const SUPPORTED_FILE_OPS: [&str; 7] = [
    "search",
    "copy",
    "move",
    "archive",
    "list",
    "stat",
    "read_text",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalContext {
    pub token: Option<String>,
    pub required_token: Option<String>,
    pub approver_id: Option<String>,
}

#[derive(Debug, Error)]
#[error("{code}: {message}")]
pub struct PolicyViolation {
    pub code: String,
    pub message: String,
}

pub fn enforce(action: &Action, approval: Option<&ApprovalContext>) -> Result<(), PolicyViolation> {
    check_operation(action)?;
    check_service_name(action)?;
    check_risk_gate(action, approval)?;
    Ok(())
}

pub fn effective_risk_level(action: &Action) -> RiskLevel {
    match action.risk_level {
        RiskLevel::Low => default_risk_level(action),
        other => other,
    }
}

pub fn action_requires_approval(action: &Action) -> bool {
    action.requires_approval
        || matches!(
            effective_risk_level(action),
            RiskLevel::High | RiskLevel::Critical
        )
        || default_requires_approval(action)
}

pub fn apply_governance_defaults(mut action: Action) -> Action {
    action.risk_level = effective_risk_level(&action);
    action.normalize();
    action.requires_approval = action_requires_approval(&action);
    action
}

fn check_operation(action: &Action) -> Result<(), PolicyViolation> {
    match action.domain.as_str() {
        "service" => {
            if SUPPORTED_SERVICE_OPS.contains(&action.operation.as_str()) {
                Ok(())
            } else {
                Err(PolicyViolation {
                    code: "UNSUPPORTED_OPERATION".to_string(),
                    message: format!("Unsupported service operation: {}", action.operation),
                })
            }
        }
        "files" => {
            if SUPPORTED_FILE_OPS.contains(&action.operation.as_str()) {
                Ok(())
            } else {
                Err(PolicyViolation {
                    code: "UNSUPPORTED_OPERATION".to_string(),
                    message: format!("Unsupported file operation: {}", action.operation),
                })
            }
        }
        _ => Ok(()),
    }
}

fn check_service_name(action: &Action) -> Result<(), PolicyViolation> {
    if action.domain != "service" {
        return Ok(());
    }
    let Some(service_name) = action.resource.get("service_name").and_then(|v| v.as_str()) else {
        return Err(PolicyViolation {
            code: "INVALID_SERVICE_NAME".to_string(),
            message: "service_name is required".to_string(),
        });
    };

    if service_name.is_empty() || service_name.len() > 64 {
        return Err(PolicyViolation {
            code: "INVALID_SERVICE_NAME".to_string(),
            message: format!("Invalid service name: {service_name}"),
        });
    }

    let valid = service_name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-');

    if !valid {
        return Err(PolicyViolation {
            code: "INVALID_SERVICE_NAME".to_string(),
            message: format!("Invalid service name: {service_name}"),
        });
    }

    Ok(())
}

fn check_risk_gate(
    action: &Action,
    approval: Option<&ApprovalContext>,
) -> Result<(), PolicyViolation> {
    if !action_requires_approval(action) {
        return Ok(());
    }
    let risk_level = effective_risk_level(action);

    let Some(ctx) = approval else {
        return Err(PolicyViolation {
            code: "APPROVAL_REQUIRED".to_string(),
            message: format!(
                "{}.{} (risk={:?}) requires approval",
                action.domain, action.operation, risk_level
            ),
        });
    };

    let Some(token) = &ctx.token else {
        return Err(PolicyViolation {
            code: "APPROVAL_REQUIRED".to_string(),
            message: format!(
                "{}.{} (risk={:?}) requires approval",
                action.domain, action.operation, risk_level
            ),
        });
    };

    if let Some(required_token) = &ctx.required_token {
        if token != required_token {
            return Err(PolicyViolation {
                code: "APPROVAL_TOKEN_MISMATCH".to_string(),
                message: format!(
                    "Approval token does not match for {}.{}",
                    action.domain, action.operation
                ),
            });
        }
    }

    Ok(())
}

fn default_requires_approval(action: &Action) -> bool {
    matches!(
        (
            action.domain.trim().to_lowercase().as_str(),
            action.operation.trim().to_lowercase().as_str(),
        ),
        ("camera", "connect")
    )
}

fn default_risk_level(action: &Action) -> RiskLevel {
    match (
        action.domain.trim().to_lowercase().as_str(),
        action.operation.trim().to_lowercase().as_str(),
    ) {
        ("service", "status") => RiskLevel::Low,
        ("service", "start") | ("service", "enable") => RiskLevel::Medium,
        ("service", "stop") | ("service", "restart") => RiskLevel::High,
        ("files", "search") | ("files", "list") | ("files", "stat") | ("files", "read_text") => {
            RiskLevel::Low
        }
        ("files", "copy") => {
            let overwrite = action
                .args
                .get("overwrite")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if overwrite {
                RiskLevel::High
            } else {
                RiskLevel::Medium
            }
        }
        ("files", "move") => RiskLevel::High,
        ("files", "archive") => RiskLevel::Medium,
        ("camera", "connect") => RiskLevel::Medium,
        _ => RiskLevel::Low,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::orchestrator::contracts::{Action, RiskLevel};

    use super::{
        action_requires_approval, apply_governance_defaults, effective_risk_level, enforce,
        ApprovalContext,
    };

    #[test]
    fn high_risk_requires_approval() {
        let action = Action {
            domain: "service".to_string(),
            operation: "restart".to_string(),
            resource: json!({"service_name": "ssh"}),
            args: json!({}),
            risk_level: RiskLevel::High,
            requires_approval: true,
            dry_run: false,
        };

        let err = enforce(&action, None).unwrap_err();
        assert_eq!(err.code, "APPROVAL_REQUIRED");

        let ctx = ApprovalContext {
            token: Some("approved".to_string()),
            required_token: Some("approved".to_string()),
            approver_id: Some("admin".to_string()),
        };
        assert!(enforce(&action, Some(&ctx)).is_ok());
    }

    #[test]
    fn camera_connect_requires_approval_by_default() {
        let action = Action {
            domain: "camera".to_string(),
            operation: "connect".to_string(),
            resource: json!({"ip": "192.168.1.10"}),
            args: json!({}),
            risk_level: RiskLevel::Medium,
            requires_approval: false,
            dry_run: false,
        };

        assert!(action_requires_approval(&action));
        let err = enforce(&action, None).unwrap_err();
        assert_eq!(err.code, "APPROVAL_REQUIRED");
    }

    #[test]
    fn service_restart_uses_default_high_risk() {
        let action = Action {
            domain: "service".to_string(),
            operation: "restart".to_string(),
            resource: json!({"service_name": "ssh"}),
            args: json!({}),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };

        assert_eq!(effective_risk_level(&action), RiskLevel::High);
        assert!(action_requires_approval(&action));
    }

    #[test]
    fn governance_defaults_raise_file_move_to_high_risk() {
        let action = apply_governance_defaults(Action {
            domain: "files".to_string(),
            operation: "move".to_string(),
            resource: json!({"source": "/tmp/a", "target": "/tmp/b"}),
            args: json!({}),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        });

        assert_eq!(action.risk_level, RiskLevel::High);
        assert!(action.requires_approval);
    }

    #[test]
    fn read_only_file_ops_stay_low_risk() {
        let list_action = Action {
            domain: "files".to_string(),
            operation: "list".to_string(),
            resource: json!({"path": "/mnt/library"}),
            args: json!({}),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let read_action = Action {
            domain: "files".to_string(),
            operation: "read_text".to_string(),
            resource: json!({"path": "/mnt/library/brief.txt"}),
            args: json!({"max_bytes": 1024}),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };

        assert_eq!(effective_risk_level(&list_action), RiskLevel::Low);
        assert_eq!(effective_risk_level(&read_action), RiskLevel::Low);
        assert!(enforce(&list_action, None).is_ok());
        assert!(enforce(&read_action, None).is_ok());
    }
}
