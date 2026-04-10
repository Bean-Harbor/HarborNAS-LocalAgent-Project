//! Workflow definitions managed by the control plane.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowStatus {
    Draft,
    Published,
    Disabled,
}
