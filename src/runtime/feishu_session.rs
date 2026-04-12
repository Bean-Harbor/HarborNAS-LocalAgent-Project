//! Persistent Feishu conversation state for multi-step onboarding flows.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runtime::admin_console::default_rtsp_port;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PendingFeishuCandidate {
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
pub struct PendingFeishuAdd {
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
pub struct FeishuConversationState {
    pub key: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub pending_candidates: Vec<PendingFeishuCandidate>,
    #[serde(default)]
    pub pending_add: Option<PendingFeishuAdd>,
    #[serde(default)]
    pub last_scan_cidr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct FeishuConversationFile {
    #[serde(default)]
    conversations: HashMap<String, FeishuConversationState>,
}

#[derive(Debug, Clone)]
pub struct FeishuConversationStore {
    path: PathBuf,
}

impl FeishuConversationStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self, key: &str) -> Result<FeishuConversationState, String> {
        let file = self.load_file()?;
        Ok(file
            .conversations
            .get(key)
            .cloned()
            .unwrap_or_else(|| FeishuConversationState {
                key: key.to_string(),
                ..Default::default()
            }))
    }

    pub fn save(&self, state: &FeishuConversationState) -> Result<(), String> {
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

    fn load_file(&self) -> Result<FeishuConversationFile, String> {
        if !self.path.exists() {
            return Ok(FeishuConversationFile::default());
        }

        let text = fs::read_to_string(&self.path).map_err(|error| {
            format!(
                "failed to read Feishu conversation state {}: {error}",
                self.path.display()
            )
        })?;
        serde_json::from_str(&text).map_err(|error| {
            format!(
                "failed to parse Feishu conversation state {}: {error}",
                self.path.display()
            )
        })
    }

    fn save_file(&self, file: &FeishuConversationFile) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create Feishu conversation directory {}: {error}",
                    parent.display()
                )
            })?;
        }

        let payload = serde_json::to_string_pretty(file).map_err(|error| {
            format!(
                "failed to serialize Feishu conversation state {}: {error}",
                self.path.display()
            )
        })?;
        fs::write(&self.path, payload).map_err(|error| {
            format!(
                "failed to write Feishu conversation state {}: {error}",
                self.path.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{FeishuConversationState, FeishuConversationStore, PendingFeishuCandidate};

    #[test]
    fn conversation_store_round_trips_state() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("harbornas-feishu-conversations-{unique}.json"));
        let store = FeishuConversationStore::new(&path);
        let state = FeishuConversationState {
            key: "chat-demo".to_string(),
            display_name: "Bean".to_string(),
            pending_candidates: vec![PendingFeishuCandidate {
                candidate_id: "cand-1".to_string(),
                name: "Gate Cam".to_string(),
                ip: "192.168.1.20".to_string(),
                port: 554,
                rtsp_paths: vec!["/live".to_string()],
                requires_auth: true,
                vendor: Some("Demo".to_string()),
                model: Some("X1".to_string()),
            }],
            pending_add: None,
            last_scan_cidr: "192.168.1.0/24".to_string(),
        };

        store.save(&state).expect("save");
        let loaded = store.load("chat-demo").expect("load");

        assert_eq!(loaded, state);
        let _ = fs::remove_file(store.path());
    }
}
