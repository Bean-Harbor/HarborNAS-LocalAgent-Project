//! Authentication entrypoints for local, HarborOS, and IM identities.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthSource {
    #[default]
    Local,
    HarborOs,
    ImChannel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IdentityBinding {
    pub identity_id: String,
    pub user_id: String,
    pub auth_source: AuthSource,
    pub provider_key: String,
    pub external_user_id: String,
    #[serde(default)]
    pub external_union_id: Option<String>,
    #[serde(default)]
    pub external_chat_id: Option<String>,
    #[serde(default)]
    pub profile_snapshot: Value,
    #[serde(default)]
    pub last_seen_at: Option<String>,
}
