//! Event stream schemas for device, task, media, and inference collaboration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventSourceKind {
    #[default]
    Device,
    Media,
    Task,
    Automation,
    System,
    Inference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    #[default]
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EventRecord {
    pub event_id: String,
    pub workspace_id: String,
    pub source_kind: EventSourceKind,
    pub source_id: String,
    pub event_type: String,
    pub severity: EventSeverity,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub causation_id: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub ingested_at: Option<String>,
}
