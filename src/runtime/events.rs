//! Event bus primitives shared by devices, workflows, and notifications.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    DeviceState,
    Detection,
    WorkflowRun,
    Approval,
}
