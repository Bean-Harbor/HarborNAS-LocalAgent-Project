use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::orchestrator::contracts::{Action, ExecutionResult, StepStatus};

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub audit_ref: String,
    pub task_id: String,
    pub step_id: String,
    pub domain: String,
    pub operation: String,
    pub route_selected: String,
    pub fallback_used: bool,
    pub risk_level: String,
    pub status: String,
    pub duration_ms: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub dry_run: bool,
    pub inputs: Value,
    pub outputs: Value,
}

pub struct AuditLog {
    events: Vec<AuditEvent>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.clone()
    }

    pub fn record_start(&mut self, task_id: &str, step_id: &str, action: &Action) -> usize {
        let event = AuditEvent {
            audit_ref: Uuid::new_v4().as_simple().to_string()[..12].to_string(),
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            domain: action.domain.clone(),
            operation: action.operation.clone(),
            route_selected: String::new(),
            fallback_used: false,
            risk_level: format!("{:?}", action.risk_level).to_uppercase(),
            status: "PENDING".to_string(),
            duration_ms: 0,
            error_code: None,
            error_message: None,
            dry_run: action.dry_run,
            inputs: serde_json::to_value(action).unwrap_or_else(|_| serde_json::json!({})),
            outputs: serde_json::json!({}),
        };
        self.events.push(event);
        self.events.len() - 1
    }

    pub fn audit_ref_for(&self, event_id: usize) -> String {
        self.events[event_id].audit_ref.clone()
    }

    pub fn record_complete(&mut self, event_id: usize, result: &ExecutionResult) {
        if let Some(event) = self.events.get_mut(event_id) {
            event.route_selected = result.executor_used.clone();
            event.fallback_used = result.fallback_used;
            event.status = format!("{:?}", result.status).to_uppercase();
            event.duration_ms = result.duration_ms;
            event.error_code = result.error_code.clone();
            event.error_message = result.error_message.clone();
            event.outputs = serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({}));
        }
    }

    pub fn record_policy_block(&mut self, event_id: usize, code: &str, message: &str) {
        if let Some(event) = self.events.get_mut(event_id) {
            event.status = format!("{:?}", StepStatus::Blocked).to_uppercase();
            event.error_code = Some(code.to_string());
            event.error_message = Some(message.to_string());
        }
    }
}
