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

pub fn action_allowed(
    bindings: &[PermissionBinding],
    workspace_id: &str,
    role_kind: &str,
    resource: &str,
    action: &str,
) -> bool {
    let workspace_id = normalize_token(workspace_id);
    let role_kind = normalize_token(role_kind);
    let resource = normalize_token(resource);
    let action = normalize_token(action);

    let mut allowed = false;
    for binding in bindings.iter().filter(|binding| {
        normalize_token(&binding.workspace_id) == workspace_id
            && normalize_token(&binding.role_kind) == role_kind
            && pattern_matches(&binding.resource_pattern, &resource)
            && pattern_matches(&binding.action_pattern, &action)
    }) {
        match binding.effect {
            PermissionEffect::Allow => allowed = true,
            PermissionEffect::Deny => return false,
        }
    }

    allowed
}

fn pattern_matches(pattern: &str, value: &str) -> bool {
    let pattern = normalize_token(pattern);
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    pattern == value
}

fn normalize_token(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace('-', "_")
        .replace(' ', "_")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{action_allowed, PermissionBinding, PermissionEffect, ScopeKind};

    #[test]
    fn wildcard_permission_binding_allows_matching_action() {
        let bindings = vec![PermissionBinding {
            permission_binding_id: "perm-1".to_string(),
            workspace_id: "home-1".to_string(),
            role_kind: "admin".to_string(),
            scope_kind: ScopeKind::Workspace,
            resource_pattern: "*".to_string(),
            action_pattern: "camera.*".to_string(),
            effect: PermissionEffect::Allow,
            constraints: json!({}),
        }];

        assert!(action_allowed(
            &bindings,
            "home-1",
            "admin",
            "camera:cam-1",
            "camera.view"
        ));
        assert!(!action_allowed(
            &bindings,
            "home-1",
            "viewer",
            "camera:cam-1",
            "camera.view"
        ));
    }

    #[test]
    fn deny_binding_overrides_allow_binding() {
        let bindings = vec![
            PermissionBinding {
                permission_binding_id: "perm-allow".to_string(),
                workspace_id: "home-1".to_string(),
                role_kind: "viewer".to_string(),
                scope_kind: ScopeKind::Workspace,
                resource_pattern: "*".to_string(),
                action_pattern: "camera.view".to_string(),
                effect: PermissionEffect::Allow,
                constraints: json!({}),
            },
            PermissionBinding {
                permission_binding_id: "perm-deny".to_string(),
                workspace_id: "home-1".to_string(),
                role_kind: "viewer".to_string(),
                scope_kind: ScopeKind::Resource,
                resource_pattern: "camera:front_door".to_string(),
                action_pattern: "camera.view".to_string(),
                effect: PermissionEffect::Deny,
                constraints: json!({}),
            },
        ];

        assert!(!action_allowed(
            &bindings,
            "home-1",
            "viewer",
            "camera:front_door",
            "camera.view"
        ));
        assert!(action_allowed(
            &bindings,
            "home-1",
            "viewer",
            "camera:living_room",
            "camera.view"
        ));
    }
}
