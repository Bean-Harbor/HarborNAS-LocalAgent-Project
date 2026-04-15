//! Role and permission bindings for workspaces, rooms, and resources.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    #[default]
    Platform,
    Workspace,
    Room,
    Resource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionEffect {
    #[default]
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PermissionBinding {
    pub permission_binding_id: String,
    pub workspace_id: String,
    pub role_kind: String,
    pub scope_kind: ScopeKind,
    pub resource_pattern: String,
    pub action_pattern: String,
    pub effect: PermissionEffect,
    #[serde(default)]
    pub constraints: Value,
}
