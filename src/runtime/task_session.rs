//! Persistent conversation state for Task API multi-step flows.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runtime::admin_console::default_rtsp_port;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PendingTaskCandidate {
    pub candidate_id: String,
    pub name: String,
    pub ip: String,
    #[serde(default = "default_rtsp_port")]
    pub port: u16,
    #[serde(default)]
    pub rtsp_paths: Vec<String>,
    #[serde(default)]
    pub requires_auth: bool,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PendingTaskConnect {
    #[serde(default)]
    pub resume_token: String,
    pub name: String,
    pub ip: String,
    #[serde(default = "default_rtsp_port")]
    pub port: u16,
    #[serde(default)]
    pub rtsp_paths: Vec<String>,
    #[serde(default)]
    pub requires_auth: bool,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskConversationState {
    pub key: String,
    #[serde(default)]
    pub pending_candidates: Vec<PendingTaskCandidate>,
    #[serde(default)]
    pub pending_connect: Option<PendingTaskConnect>,
    #[serde(default)]
    pub last_scan_cidr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct TaskConversationFile {
    #[serde(default)]
    conversations: HashMap<String, TaskConversationState>,
}

#[derive(Debug, Clone)]
pub struct TaskConversationStore {
    path: PathBuf,
}

impl TaskConversationStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self, key: &str) -> Result<TaskConversationState, String> {
        let file = self.load_file()?;
        Ok(file
            .conversations
            .get(key)
            .cloned()
            .unwrap_or_else(|| TaskConversationState {
                key: key.to_string(),
                ..Default::default()
            }))
    }

    pub fn save(&self, state: &TaskConversationState) -> Result<(), String> {
        if state.key.trim().is_empty() {
            return Err("conversation key 不能为空".to_string());
        }
        let mut file = self.load_file()?;
        file.conversations.insert(state.key.clone(), state.clone());
        self.save_file(&file)
    }

    pub fn clear(&self, key: &str) -> Result<(), String> {
        let mut file = self.load_file()?;
        file.conversations.remove(key);
        self.save_file(&file)
    }

    fn load_file(&self) -> Result<TaskConversationFile, String> {
        if !self.path.exists() {
            return Ok(TaskConversationFile::default());
        }

        let text = fs::read_to_string(&self.path).map_err(|error| {
            format!(
                "failed to read Task conversation state {}: {error}",
                self.path.display()
            )
        })?;
        serde_json::from_str(&text).map_err(|error| {
            format!(
                "failed to parse Task conversation state {}: {error}",
                self.path.display()
            )
        })
    }

    fn save_file(&self, file: &TaskConversationFile) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create Task conversation directory {}: {error}",
                    parent.display()
                )
            })?;
        }

        let payload = serde_json::to_string_pretty(file).map_err(|error| {
            format!(
                "failed to serialize Task conversation state {}: {error}",
                self.path.display()
            )
        })?;
        fs::write(&self.path, payload).map_err(|error| {
            format!(
                "failed to write Task conversation state {}: {error}",
                self.path.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        PendingTaskCandidate, PendingTaskConnect, TaskConversationState, TaskConversationStore,
    };

    #[test]
    fn conversation_store_round_trips_state() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("harbornas-task-conversations-{unique}.json"));
        let store = TaskConversationStore::new(&path);
        let state = TaskConversationState {
            key: "chat-demo".to_string(),
            pending_candidates: vec![PendingTaskCandidate {
                candidate_id: "cand-1".to_string(),
                name: "Gate Cam".to_string(),
                ip: "192.168.1.20".to_string(),
                port: 554,
                rtsp_paths: vec!["/live".to_string()],
                requires_auth: true,
                vendor: Some("Demo".to_string()),
                model: Some("X1".to_string()),
            }],
            pending_connect: Some(PendingTaskConnect {
                resume_token: "resume-1".to_string(),
                name: "Gate Cam".to_string(),
                ip: "192.168.1.20".to_string(),
                port: 554,
                rtsp_paths: vec!["/live".to_string()],
                requires_auth: true,
                vendor: Some("Demo".to_string()),
                model: Some("X1".to_string()),
            }),
            last_scan_cidr: "192.168.1.0/24".to_string(),
        };

        store.save(&state).expect("save");
        let loaded = store.load("chat-demo").expect("load");

        assert_eq!(loaded, state);
        let _ = fs::remove_file(store.path());
    }
}
