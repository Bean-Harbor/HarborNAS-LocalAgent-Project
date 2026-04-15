//! Audit event metadata used by the control plane.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    pub user_id: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuditActorKind {
    #[default]
    User,
    System,
    Model,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AuditRecord {
    pub audit_id: String,
    pub workspace_id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub action: String,
    pub actor_kind: AuditActorKind,
    pub actor_id: String,
    #[serde(default)]
    pub request_snapshot: Value,
    #[serde(default)]
    pub result_snapshot: Value,
    #[serde(default)]
    pub created_at: Option<String>,
}
