//! Automation, scene, and cross-device collaboration schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRuleStatus {
    #[default]
    Draft,
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTriggerType {
    #[default]
    Event,
    Schedule,
    Manual,
    Scene,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutomationRule {
    pub rule_id: String,
    pub workspace_id: String,
    pub name: String,
    pub status: AutomationRuleStatus,
    pub trigger_type: AutomationTriggerType,
    #[serde(default)]
    pub trigger_definition: Value,
    #[serde(default)]
    pub condition_definition: Value,
    #[serde(default)]
    pub action_plan: Value,
    #[serde(default)]
    pub created_by_user_id: Option<String>,
    #[serde(default)]
    pub published_version: Option<u32>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutomationVersion {
    pub automation_version_id: String,
    pub rule_id: String,
    pub version_no: u32,
    #[serde(default)]
    pub definition: Value,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    #[default]
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutomationRun {
    pub automation_run_id: String,
    pub rule_id: String,
    #[serde(default)]
    pub trigger_event_id: Option<String>,
    pub status: AutomationRunStatus,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub result_summary: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SceneActivationMode {
    #[default]
    Manual,
    Event,
    Schedule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SceneStatus {
    #[default]
    Active,
    Inactive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SceneDefinition {
    pub scene_id: String,
    pub workspace_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub activation_mode: SceneActivationMode,
    #[serde(default)]
    pub desired_state: Value,
    pub status: SceneStatus,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SceneMemberKind {
    #[default]
    Device,
    Room,
    Rule,
    Service,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SceneMember {
    pub scene_member_id: String,
    pub scene_id: String,
    pub entity_kind: SceneMemberKind,
    pub entity_id: String,
    #[serde(default)]
    pub desired_patch: Value,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AutomationRule, AutomationRuleStatus, AutomationTriggerType};

    #[test]
    fn automation_rule_round_trips_json_fields() {
        let rule = AutomationRule {
            rule_id: "rule-1".to_string(),
            workspace_id: "home-1".to_string(),
            name: "门口检测后开灯".to_string(),
            status: AutomationRuleStatus::Active,
            trigger_type: AutomationTriggerType::Event,
            trigger_definition: json!({"event_type": "motion.detected"}),
            condition_definition: json!({"room": "entrance"}),
            action_plan: json!({"actions": ["light.turn_on"]}),
            created_by_user_id: Some("user-1".to_string()),
            published_version: Some(3),
            metadata: json!({"source": "webui"}),
        };

        let payload = serde_json::to_string(&rule).expect("serialize");
        let decoded: AutomationRule = serde_json::from_str(&payload).expect("deserialize");
        assert_eq!(decoded.published_version, Some(3));
        assert_eq!(decoded.trigger_definition["event_type"], "motion.detected");
    }
}
