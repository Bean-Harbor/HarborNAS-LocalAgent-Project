//! Audit event metadata used by the control plane.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditActor {
    pub user_id: String,
    pub source: String,
}
