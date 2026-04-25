//! Conversation, task, step, and artifact schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::orchestrator::contracts::RiskLevel;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ConversationSession {
    pub session_id: String,
    pub workspace_id: String,
    pub channel: String,
    pub surface: String,
    pub conversation_id: String,
    pub user_id: String,
    #[serde(default)]
    pub route_key: String,
    #[serde(default)]
    pub last_message_id: String,
    #[serde(default)]
    pub chat_type: String,
    #[serde(default)]
    pub state: Value,
    #[serde(default)]
    pub resume_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunStatus {
    #[default]
    Queued,
    Running,
    NeedsInput,
    Blocked,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TaskRun {
    pub task_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub source_channel: String,
    pub domain: String,
    pub action: String,
    #[serde(default)]
    pub intent_text: String,
    #[serde(default)]
    pub entity_refs: Value,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub autonomy_level: String,
    pub status: TaskRunStatus,
    pub risk_level: RiskLevel,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRoute {
    #[default]
    Local,
    Cloud,
    MiddlewareApi,
    Midcli,
    Browser,
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStepRunStatus {
    #[default]
    Pending,
    Approved,
    Executing,
    Success,
    Failed,
    Skipped,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TaskStepRun {
    pub step_id: String,
    pub task_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub route_key: String,
    pub domain: String,
    pub operation: String,
    pub route: ExecutionRoute,
    pub executor_used: String,
    pub status: TaskStepRunStatus,
    #[serde(default)]
    pub input_payload: Value,
    #[serde(default)]
    pub output_payload: Value,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub audit_ref: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    #[default]
    Text,
    Image,
    Video,
    Link,
    Card,
    Json,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub task_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub route_key: String,
    pub artifact_kind: ArtifactKind,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub media_asset_id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ConversationSession, ExecutionRoute, TaskRun, TaskRunStatus, TaskStepRun, TaskStepRunStatus,
    };
    use crate::orchestrator::contracts::RiskLevel;

    #[test]
    fn task_run_can_store_structured_entities() {
        let task = TaskRun {
            task_id: "task-1".to_string(),
            workspace_id: "home-1".to_string(),
            session_id: "sess-1".to_string(),
            source_channel: "feishu".to_string(),
            domain: "camera".to_string(),
            action: "analyze".to_string(),
            intent_text: "分析客厅摄像头".to_string(),
            entity_refs: json!({"device_id": "cam-1"}),
            args: json!({"mode": "summary"}),
            autonomy_level: "supervised".to_string(),
            status: TaskRunStatus::Queued,
            risk_level: RiskLevel::Low,
            requires_approval: false,
            started_at: None,
            completed_at: None,
            metadata: json!({}),
        };

        let session = ConversationSession {
            session_id: "sess-1".to_string(),
            workspace_id: "home-1".to_string(),
            channel: "feishu".to_string(),
            surface: "im".to_string(),
            conversation_id: "conv-1".to_string(),
            user_id: "user-1".to_string(),
            route_key: "gw_route_1".to_string(),
            last_message_id: "om_1".to_string(),
            chat_type: "p2p".to_string(),
            state: json!({"pending": true}),
            resume_token: Some("resume-1".to_string()),
            expires_at: None,
        };

        let step = TaskStepRun {
            step_id: "step-1".to_string(),
            task_id: "task-1".to_string(),
            trace_id: "trace-1".to_string(),
            route_key: "gw_route_1".to_string(),
            domain: "vision".to_string(),
            operation: "analyze".to_string(),
            route: ExecutionRoute::Local,
            executor_used: "vision_local".to_string(),
            status: TaskStepRunStatus::Pending,
            input_payload: json!({"asset_id": "asset-1"}),
            output_payload: json!({}),
            error_code: None,
            error_message: None,
            audit_ref: None,
            started_at: None,
            ended_at: None,
        };

        assert_eq!(task.entity_refs["device_id"], "cam-1");
        assert_eq!(session.resume_token.as_deref(), Some("resume-1"));
        assert_eq!(session.route_key, "gw_route_1");
        assert_eq!(session.last_message_id, "om_1");
        assert_eq!(step.trace_id, "trace-1");
        assert_eq!(step.route_key, "gw_route_1");
        assert_eq!(step.route, ExecutionRoute::Local);
    }
}
