//! Approval requests and policy hooks for high-risk actions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    #[default]
    Pending,
    Approved,
    Rejected,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ApprovalTicket {
    pub approval_id: String,
    pub task_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub route_key: String,
    pub policy_ref: String,
    pub requester_user_id: String,
    #[serde(default)]
    pub approver_user_id: Option<String>,
    pub status: ApprovalStatus,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub requested_at: Option<String>,
    #[serde(default)]
    pub decided_at: Option<String>,
}
