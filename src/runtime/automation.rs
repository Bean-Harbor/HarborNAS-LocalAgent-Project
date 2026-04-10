//! Event-driven automation execution.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationTrigger {
    Event,
    Schedule,
    Manual,
}
