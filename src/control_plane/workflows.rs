//! Workflow definitions managed by the control plane.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    #[default]
    Draft,
    Published,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkflowDefinition {
    pub workflow_id: String,
    pub workspace_id: String,
    pub name: String,
    pub status: WorkflowStatus,
    #[serde(default)]
    pub definition: Value,
}
