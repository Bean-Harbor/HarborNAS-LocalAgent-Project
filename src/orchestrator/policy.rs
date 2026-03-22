use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::orchestrator::contracts::{Action, RiskLevel};

const SUPPORTED_SERVICE_OPS: [&str; 5] = ["status", "start", "stop", "restart", "enable"];
const SUPPORTED_FILE_OPS: [&str; 4] = ["search", "copy", "move", "archive"];

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
    if !matches!(action.risk_level, RiskLevel::High | RiskLevel::Critical) {
        return Ok(());
    }

    let Some(ctx) = approval else {
        return Err(PolicyViolation {
            code: "APPROVAL_REQUIRED".to_string(),
            message: format!(
                "{}.{} (risk={:?}) requires approval",
                action.domain, action.operation, action.risk_level
            ),
        });
    };

    let Some(token) = &ctx.token else {
        return Err(PolicyViolation {
            code: "APPROVAL_REQUIRED".to_string(),
            message: format!(
                "{}.{} (risk={:?}) requires approval",
                action.domain, action.operation, action.risk_level
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::orchestrator::contracts::{Action, RiskLevel};

    use super::{enforce, ApprovalContext};

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
}
