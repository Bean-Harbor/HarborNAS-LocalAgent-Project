//! Workflow node execution contracts.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Source,
    Ai,
    Transform,
    Condition,
    Action,
    Sink,
}
