//! User, workspace, room, and membership domain types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoleKind {
    #[default]
    Owner,
    Admin,
    Operator,
    Member,
    Viewer,
    Guest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceType {
    #[default]
    Home,
    Lab,
    Managed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    #[default]
    Active,
    Suspended,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Workspace {
    pub workspace_id: String,
    pub workspace_type: WorkspaceType,
    pub display_name: String,
    pub timezone: String,
    pub locale: String,
    pub owner_user_id: String,
    pub status: WorkspaceStatus,
    #[serde(default)]
    pub settings: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    #[default]
    Active,
    Invited,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UserAccount {
    pub user_id: String,
    pub display_name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    pub status: UserStatus,
    #[serde(default)]
    pub default_workspace_id: Option<String>,
    #[serde(default)]
    pub preferences: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    #[default]
    Active,
    Pending,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Membership {
    pub membership_id: String,
    pub workspace_id: String,
    pub user_id: String,
    pub role_kind: RoleKind,
    pub status: MembershipStatus,
    #[serde(default)]
    pub granted_by_user_id: Option<String>,
    #[serde(default)]
    pub granted_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoomZoneType {
    #[default]
    LivingRoom,
    Bedroom,
    Entrance,
    Kitchen,
    Outdoor,
    Office,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Room {
    pub room_id: String,
    pub workspace_id: String,
    pub name: String,
    #[serde(default)]
    pub floor: Option<String>,
    pub zone_type: RoomZoneType,
    #[serde(default)]
    pub aliases: Vec<String>,
}
