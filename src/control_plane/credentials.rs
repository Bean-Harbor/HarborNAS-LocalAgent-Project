//! Provider account, credential governance, and usage metering schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    AiCloud,
    VendorCloud,
    VendorLocal,
    Bridge,
    HarborOs,
    Standard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOwnerScope {
    #[default]
    Workspace,
    User,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAccountStatus {
    #[default]
    Active,
    NeedsReauth,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProviderAccount {
    pub provider_account_id: String,
    pub workspace_id: String,
    pub provider_key: String,
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub owner_scope: ProviderOwnerScope,
    #[serde(default)]
    pub owner_user_id: Option<String>,
    pub status: ProviderAccountStatus,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    #[default]
    ApiKey,
    OauthToken,
    RefreshToken,
    DeviceToken,
    SessionSecret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CredentialRotationState {
    #[default]
    Valid,
    Expiring,
    Revoked,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CredentialRecord {
    pub credential_id: String,
    pub provider_account_id: String,
    pub credential_kind: CredentialKind,
    pub vault_key: String,
    #[serde(default)]
    pub scope: Value,
    #[serde(default)]
    pub expires_at: Option<String>,
    pub rotation_state: CredentialRotationState,
    #[serde(default)]
    pub last_verified_at: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LeaseIssuedToKind {
    #[default]
    Task,
    Inference,
    ProviderBinding,
    ShareSession,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CredentialLease {
    pub lease_id: String,
    pub credential_id: String,
    pub issued_to_kind: LeaseIssuedToKind,
    pub issued_to_id: String,
    #[serde(default)]
    pub lease_scope: Value,
    #[serde(default)]
    pub issued_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UsageType {
    #[default]
    ModelInference,
    VendorApi,
    MediaProxy,
    Webhook,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UsageLedgerEntry {
    pub ledger_id: String,
    pub workspace_id: String,
    pub provider_account_id: String,
    #[serde(default)]
    pub credential_id: Option<String>,
    pub usage_type: UsageType,
    #[serde(default)]
    pub request_units: f64,
    #[serde(default)]
    pub input_bytes: u64,
    #[serde(default)]
    pub output_bytes: u64,
    #[serde(default)]
    pub cost_amount: f64,
    #[serde(default)]
    pub cost_currency: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub inference_run_id: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}
