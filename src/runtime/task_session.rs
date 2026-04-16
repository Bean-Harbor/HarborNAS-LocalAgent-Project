//! Persistent conversation state for Task API multi-step flows.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::control_plane::approvals::{ApprovalStatus, ApprovalTicket};
use crate::control_plane::events::EventRecord;
use crate::control_plane::tasks::{ArtifactRecord, ConversationSession, TaskRun, TaskStepRun};
use crate::runtime::admin_console::default_rtsp_port;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PendingTaskCandidate {
    pub candidate_id: String,
    pub name: String,
    pub ip: String,
    #[serde(default)]
    pub room: Option<String>,
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
    #[serde(default)]
    pub room: Option<String>,
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
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub pending_candidates: Vec<PendingTaskCandidate>,
    #[serde(default)]
    pub pending_connect: Option<PendingTaskConnect>,
    #[serde(default)]
    pub last_scan_cidr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskSessionStateEnvelope {
    #[serde(default = "default_task_session_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_task_session_namespace")]
    pub namespace: String,
    #[serde(default = "default_task_session_flow_type")]
    pub flow_type: String,
    #[serde(default)]
    pub flow_state: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
struct TaskConversationFile {
    #[serde(default)]
    conversations: HashMap<String, TaskConversationState>,
    #[serde(default)]
    sessions: HashMap<String, ConversationSession>,
    #[serde(default)]
    task_runs: HashMap<String, TaskRun>,
    #[serde(default)]
    task_steps: HashMap<String, TaskStepRun>,
    #[serde(default)]
    artifacts: HashMap<String, ArtifactRecord>,
    #[serde(default)]
    approvals: HashMap<String, ApprovalTicket>,
    #[serde(default)]
    events: HashMap<String, EventRecord>,
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

    pub fn load_for_session(
        &self,
        session_id: &str,
        key_hint: Option<&str>,
    ) -> Result<Option<TaskConversationState>, String> {
        let file = self.load_file()?;
        if let Some(session) = file.sessions.get(session_id) {
            if let Some(state) = conversation_state_from_session(session, key_hint) {
                return Ok(Some(state));
            }
        }

        let Some(key) = non_empty_string(key_hint) else {
            return Ok(None);
        };
        Ok(file.conversations.get(key).cloned().map(|mut state| {
            if state.key.trim().is_empty() {
                state.key = key.to_string();
            }
            state
        }))
    }

    pub fn save_for_session(
        &self,
        session: &ConversationSession,
        state: &TaskConversationState,
    ) -> Result<(), String> {
        if session.session_id.trim().is_empty() {
            return Err("session_id 不能为空".to_string());
        }
        if state.key.trim().is_empty() {
            return Err("conversation key 不能为空".to_string());
        }

        let mut file = self.load_file()?;
        let mut session = session.clone();
        let envelope = envelope_from_conversation_state(state, Some(&session))?;
        session.state = serde_json::to_value(&envelope).map_err(|error| {
            format!(
                "failed to serialize Task conversation session state {}: {error}",
                self.path.display()
            )
        })?;
        file.sessions.insert(session.session_id.clone(), session);
        file.conversations.insert(state.key.clone(), state.clone());
        self.save_file(&file)
    }

    pub fn clear_for_session(
        &self,
        session_id: &str,
        key_hint: Option<&str>,
    ) -> Result<(), String> {
        let mut file = self.load_file()?;
        if let Some(session) = file.sessions.get_mut(session_id) {
            session.state = Value::Null;
            session.resume_token = None;
        }
        if let Some(key) = non_empty_string(key_hint) {
            file.conversations.remove(key);
        }
        self.save_file(&file)
    }

    pub fn load_session(&self, session_id: &str) -> Result<Option<ConversationSession>, String> {
        let file = self.load_file()?;
        Ok(file.sessions.get(session_id).cloned())
    }

    pub fn save_session(&self, session: &ConversationSession) -> Result<(), String> {
        if session.session_id.trim().is_empty() {
            return Err("session_id 不能为空".to_string());
        }
        let mut file = self.load_file()?;
        file.sessions
            .insert(session.session_id.clone(), session.clone());
        self.save_file(&file)
    }

    pub fn load_task_run(&self, task_id: &str) -> Result<Option<TaskRun>, String> {
        let file = self.load_file()?;
        Ok(file.task_runs.get(task_id).cloned())
    }

    pub fn save_task_run(&self, task_run: &TaskRun) -> Result<(), String> {
        if task_run.task_id.trim().is_empty() {
            return Err("task_id 不能为空".to_string());
        }
        let mut file = self.load_file()?;
        file.task_runs
            .insert(task_run.task_id.clone(), task_run.clone());
        self.save_file(&file)
    }

    pub fn load_task_step(&self, step_id: &str) -> Result<Option<TaskStepRun>, String> {
        let file = self.load_file()?;
        Ok(file.task_steps.get(step_id).cloned())
    }

    pub fn save_task_step(&self, task_step: &TaskStepRun) -> Result<(), String> {
        if task_step.step_id.trim().is_empty() {
            return Err("step_id 不能为空".to_string());
        }
        let mut file = self.load_file()?;
        file.task_steps
            .insert(task_step.step_id.clone(), task_step.clone());
        self.save_file(&file)
    }

    pub fn artifacts_for_task(&self, task_id: &str) -> Result<Vec<ArtifactRecord>, String> {
        let file = self.load_file()?;
        let mut artifacts = file
            .artifacts
            .values()
            .filter(|artifact| artifact.task_id == task_id)
            .cloned()
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
        Ok(artifacts)
    }

    pub fn replace_artifacts_for_step(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        artifacts: &[ArtifactRecord],
    ) -> Result<(), String> {
        let mut file = self.load_file()?;
        file.artifacts.retain(|_, artifact| {
            !(artifact.task_id == task_id && artifact.step_id.as_deref() == step_id)
        });
        for artifact in artifacts {
            if artifact.artifact_id.trim().is_empty() {
                return Err("artifact_id 不能为空".to_string());
            }
            file.artifacts
                .insert(artifact.artifact_id.clone(), artifact.clone());
        }
        self.save_file(&file)
    }

    pub fn approvals_for_task(&self, task_id: &str) -> Result<Vec<ApprovalTicket>, String> {
        let file = self.load_file()?;
        let mut approvals = file
            .approvals
            .values()
            .filter(|approval| approval.task_id == task_id)
            .cloned()
            .collect::<Vec<_>>();
        approvals.sort_by(|left, right| {
            left.requested_at
                .cmp(&right.requested_at)
                .then(left.approval_id.cmp(&right.approval_id))
        });
        Ok(approvals)
    }

    pub fn pending_approvals(&self) -> Result<Vec<ApprovalTicket>, String> {
        let file = self.load_file()?;
        let mut approvals = file
            .approvals
            .values()
            .filter(|approval| approval.status == ApprovalStatus::Pending)
            .cloned()
            .collect::<Vec<_>>();
        approvals.sort_by(|left, right| {
            left.requested_at
                .cmp(&right.requested_at)
                .then(left.approval_id.cmp(&right.approval_id))
        });
        Ok(approvals)
    }

    pub fn load_approval(&self, approval_id: &str) -> Result<Option<ApprovalTicket>, String> {
        let file = self.load_file()?;
        Ok(file.approvals.get(approval_id).cloned())
    }

    pub fn save_approval(&self, approval: &ApprovalTicket) -> Result<(), String> {
        if approval.approval_id.trim().is_empty() {
            return Err("approval_id 不能为空".to_string());
        }
        let mut file = self.load_file()?;
        file.approvals
            .insert(approval.approval_id.clone(), approval.clone());
        self.save_file(&file)
    }

    pub fn resolve_pending_approvals(
        &self,
        task_id: &str,
        approver_user_id: Option<String>,
        decided_at: Option<String>,
    ) -> Result<Vec<ApprovalTicket>, String> {
        let mut file = self.load_file()?;
        let mut updated = Vec::new();
        for approval in file.approvals.values_mut() {
            if approval.task_id != task_id || approval.status != ApprovalStatus::Pending {
                continue;
            }
            approval.status = ApprovalStatus::Approved;
            if approver_user_id.is_some() {
                approval.approver_user_id = approver_user_id.clone();
            }
            if decided_at.is_some() {
                approval.decided_at = decided_at.clone();
            }
            updated.push(approval.clone());
        }
        self.save_file(&file)?;
        Ok(updated)
    }

    pub fn update_approval_status(
        &self,
        approval_id: &str,
        status: ApprovalStatus,
        approver_user_id: Option<String>,
        decided_at: Option<String>,
    ) -> Result<Option<ApprovalTicket>, String> {
        let mut file = self.load_file()?;
        let Some(approval) = file.approvals.get_mut(approval_id) else {
            return Ok(None);
        };
        approval.status = status;
        if approver_user_id.is_some() {
            approval.approver_user_id = approver_user_id;
        }
        if decided_at.is_some() {
            approval.decided_at = decided_at;
        }
        let updated = approval.clone();
        self.save_file(&file)?;
        Ok(Some(updated))
    }

    pub fn events_for_task(&self, task_id: &str) -> Result<Vec<EventRecord>, String> {
        let file = self.load_file()?;
        let mut events = file
            .events
            .values()
            .filter(|event| event.source_id == task_id)
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then(left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    pub fn replace_events_for_step(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        events: &[EventRecord],
    ) -> Result<(), String> {
        let mut file = self.load_file()?;
        file.events.retain(|_, event| {
            !(event.source_id == task_id && event.causation_id.as_deref() == step_id)
        });
        for event in events {
            if event.event_id.trim().is_empty() {
                return Err("event_id 不能为空".to_string());
            }
            file.events.insert(event.event_id.clone(), event.clone());
        }
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

fn conversation_state_from_session(
    session: &ConversationSession,
    key_hint: Option<&str>,
) -> Option<TaskConversationState> {
    if session.state.is_null() {
        return None;
    }

    if let Ok(envelope) = serde_json::from_value::<TaskSessionStateEnvelope>(session.state.clone())
    {
        if !envelope.flow_state.is_null() {
            if let Ok(mut state) =
                serde_json::from_value::<TaskConversationState>(envelope.flow_state)
            {
                backfill_conversation_key(&mut state, session, key_hint);
                return Some(state);
            }
        }
    }

    let mut state: TaskConversationState = serde_json::from_value(session.state.clone()).ok()?;
    backfill_conversation_key(&mut state, session, key_hint);
    Some(state)
}

fn envelope_from_conversation_state(
    state: &TaskConversationState,
    session: Option<&ConversationSession>,
) -> Result<TaskSessionStateEnvelope, String> {
    let mut flow_state = serde_json::to_value(state)
        .map_err(|error| format!("failed to serialize Task conversation flow state: {error}"))?;

    if let Some(flow_state_object) = flow_state.as_object_mut() {
        if state.key.trim().is_empty() {
            flow_state_object.remove("key");
        } else if let Some(session) = session {
            let matches_conversation = non_empty_string(Some(session.conversation_id.as_str()))
                .map(|value| value == state.key)
                .unwrap_or(false);
            let matches_session = non_empty_string(Some(session.session_id.as_str()))
                .map(|value| value == state.key)
                .unwrap_or(false);
            let matches_user = non_empty_string(Some(session.user_id.as_str()))
                .map(|value| value == state.key)
                .unwrap_or(false);
            if matches_conversation || matches_session || matches_user {
                flow_state_object.remove("key");
            }
        }
    }

    Ok(TaskSessionStateEnvelope {
        schema_version: default_task_session_schema_version(),
        namespace: default_task_session_namespace(),
        flow_type: default_task_session_flow_type(),
        flow_state,
    })
}

pub fn session_state_value_from_conversation(
    state: &TaskConversationState,
    session: Option<&ConversationSession>,
) -> Result<Value, String> {
    let envelope = envelope_from_conversation_state(state, session)?;
    serde_json::to_value(envelope)
        .map_err(|error| format!("failed to serialize Task conversation session state: {error}"))
}

fn backfill_conversation_key(
    state: &mut TaskConversationState,
    session: &ConversationSession,
    key_hint: Option<&str>,
) {
    if state.key.trim().is_empty() {
        state.key = non_empty_string(key_hint)
            .or_else(|| non_empty_string(Some(session.conversation_id.as_str())))
            .or_else(|| non_empty_string(Some(session.session_id.as_str())))
            .or_else(|| non_empty_string(Some(session.user_id.as_str())))
            .unwrap_or_default()
            .to_string();
    }
}

fn default_task_session_schema_version() -> u32 {
    1
}

fn default_task_session_namespace() -> String {
    "task_api".to_string()
}

fn default_task_session_flow_type() -> String {
    "camera_onboarding".to_string()
}

fn non_empty_string(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{json, Value};

    use super::{
        PendingTaskCandidate, PendingTaskConnect, TaskConversationState, TaskConversationStore,
        TaskSessionStateEnvelope,
    };
    use crate::control_plane::approvals::{ApprovalStatus, ApprovalTicket};
    use crate::control_plane::events::{EventRecord, EventSeverity, EventSourceKind};
    use crate::control_plane::tasks::{
        ArtifactKind, ArtifactRecord, ConversationSession, ExecutionRoute, TaskRun, TaskRunStatus,
        TaskStepRun, TaskStepRunStatus,
    };
    use crate::orchestrator::contracts::RiskLevel;

    fn unique_store_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}.json"))
    }

    #[test]
    fn conversation_store_round_trips_state() {
        let path = unique_store_path("harbornas-task-conversations");
        let store = TaskConversationStore::new(&path);
        let state = TaskConversationState {
            key: "chat-demo".to_string(),
            pending_candidates: vec![PendingTaskCandidate {
                candidate_id: "cand-1".to_string(),
                name: "Gate Cam".to_string(),
                ip: "192.168.1.20".to_string(),
                room: Some("Entry".to_string()),
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
                room: Some("Entry".to_string()),
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

    #[test]
    fn runtime_records_round_trip() {
        let path = unique_store_path("harbornas-task-runtime");
        let store = TaskConversationStore::new(&path);

        let session = ConversationSession {
            session_id: "sess-1".to_string(),
            workspace_id: "home-1".to_string(),
            channel: "feishu".to_string(),
            surface: "harborbeacon".to_string(),
            conversation_id: "chat-1".to_string(),
            user_id: "user-1".to_string(),
            state: json!({"pending_candidates": 1}),
            resume_token: Some("resume-1".to_string()),
            expires_at: None,
        };
        store.save_session(&session).expect("save session");

        let task_run = TaskRun {
            task_id: "task-1".to_string(),
            workspace_id: "home-1".to_string(),
            session_id: "sess-1".to_string(),
            source_channel: "feishu".to_string(),
            domain: "camera".to_string(),
            action: "connect".to_string(),
            intent_text: "接入 1".to_string(),
            entity_refs: json!({"candidate_index": 1}),
            args: json!({"resume_token": "resume-1"}),
            autonomy_level: "supervised".to_string(),
            status: TaskRunStatus::NeedsInput,
            risk_level: RiskLevel::Medium,
            requires_approval: false,
            started_at: Some("1710000000".to_string()),
            completed_at: None,
            metadata: json!({"trace_id": "trace-1"}),
        };
        store.save_task_run(&task_run).expect("save task");

        let task_step = TaskStepRun {
            step_id: "step-1".to_string(),
            task_id: "task-1".to_string(),
            domain: "camera".to_string(),
            operation: "connect".to_string(),
            route: ExecutionRoute::Local,
            executor_used: "camera_hub_service".to_string(),
            status: TaskStepRunStatus::Blocked,
            input_payload: json!({"candidate_index": 1}),
            output_payload: json!({"prompt": "密码 xxxxxx"}),
            error_code: None,
            error_message: None,
            audit_ref: Some("audit-1".to_string()),
            started_at: Some("1710000000".to_string()),
            ended_at: Some("1710000001".to_string()),
        };
        store.save_task_step(&task_step).expect("save step");

        let artifacts = vec![ArtifactRecord {
            artifact_id: "artifact-1".to_string(),
            task_id: "task-1".to_string(),
            step_id: Some("step-1".to_string()),
            artifact_kind: ArtifactKind::Json,
            label: "候选设备".to_string(),
            mime_type: "application/json".to_string(),
            media_asset_id: None,
            path: None,
            url: None,
            metadata: Value::Null,
        }];
        store
            .replace_artifacts_for_step("task-1", Some("step-1"), &artifacts)
            .expect("save artifacts");

        assert_eq!(
            store.load_session("sess-1").expect("load session"),
            Some(session)
        );
        assert_eq!(
            store.load_task_run("task-1").expect("load task"),
            Some(task_run)
        );
        assert_eq!(
            store.load_task_step("step-1").expect("load step"),
            Some(task_step)
        );
        assert_eq!(
            store.artifacts_for_task("task-1").expect("load artifacts"),
            artifacts
        );

        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn load_for_session_prefers_session_state_and_backfills_key() {
        let path = unique_store_path("harbornas-task-session-state");
        let store = TaskConversationStore::new(&path);
        let session = ConversationSession {
            session_id: "sess-1".to_string(),
            workspace_id: "home-1".to_string(),
            channel: "im_bridge".to_string(),
            surface: "harborbeacon".to_string(),
            conversation_id: "chat-1".to_string(),
            user_id: "user-1".to_string(),
            state: json!({
                "pending_candidates": [{
                    "candidate_id": "cand-1",
                    "name": "Gate Cam",
                    "ip": "192.168.1.20",
                    "port": 554
                }],
                "last_scan_cidr": "192.168.1.0/24"
            }),
            resume_token: None,
            expires_at: None,
        };
        store.save_session(&session).expect("save session");

        let loaded = store
            .load_for_session("sess-1", Some("chat-1"))
            .expect("load")
            .expect("state");

        assert_eq!(loaded.key, "chat-1");
        assert_eq!(loaded.pending_candidates.len(), 1);
        assert_eq!(loaded.last_scan_cidr, "192.168.1.0/24");
        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn load_for_session_reads_envelope_state() {
        let path = unique_store_path("harbornas-task-session-envelope");
        let store = TaskConversationStore::new(&path);
        let session = ConversationSession {
            session_id: "sess-1".to_string(),
            workspace_id: "home-1".to_string(),
            channel: "im_bridge".to_string(),
            surface: "harborbeacon".to_string(),
            conversation_id: "chat-1".to_string(),
            user_id: "user-1".to_string(),
            state: serde_json::to_value(TaskSessionStateEnvelope {
                schema_version: 1,
                namespace: "task_api".to_string(),
                flow_type: "camera_onboarding".to_string(),
                flow_state: json!({
                    "pending_candidates": [{
                        "candidate_id": "cand-1",
                        "name": "Gate Cam",
                        "ip": "192.168.1.20",
                        "port": 554
                    }],
                    "last_scan_cidr": "192.168.1.0/24"
                }),
            })
            .expect("encode envelope"),
            resume_token: None,
            expires_at: None,
        };
        store.save_session(&session).expect("save session");

        let loaded = store
            .load_for_session("sess-1", Some("chat-1"))
            .expect("load")
            .expect("state");

        assert_eq!(loaded.key, "chat-1");
        assert_eq!(loaded.pending_candidates.len(), 1);
        assert_eq!(loaded.last_scan_cidr, "192.168.1.0/24");
        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn save_for_session_updates_session_state_and_legacy_map() {
        let path = unique_store_path("harbornas-task-session-save");
        let store = TaskConversationStore::new(&path);
        let session = ConversationSession {
            session_id: "sess-1".to_string(),
            workspace_id: "home-1".to_string(),
            channel: "im_bridge".to_string(),
            surface: "harborbeacon".to_string(),
            conversation_id: "chat-1".to_string(),
            user_id: "user-1".to_string(),
            state: Value::Null,
            resume_token: Some("resume-1".to_string()),
            expires_at: None,
        };
        let state = TaskConversationState {
            key: "chat-1".to_string(),
            pending_candidates: vec![PendingTaskCandidate {
                candidate_id: "cand-1".to_string(),
                name: "Gate Cam".to_string(),
                ip: "192.168.1.20".to_string(),
                room: None,
                port: 554,
                rtsp_paths: vec!["/live".to_string()],
                requires_auth: false,
                vendor: None,
                model: None,
            }],
            pending_connect: None,
            last_scan_cidr: "192.168.1.0/24".to_string(),
        };

        store
            .save_for_session(&session, &state)
            .expect("save for session");

        let saved_session = store
            .load_session("sess-1")
            .expect("load session")
            .expect("session");
        assert_eq!(saved_session.state["schema_version"], 1);
        assert_eq!(saved_session.state["namespace"], "task_api");
        assert_eq!(saved_session.state["flow_type"], "camera_onboarding");
        assert_eq!(
            saved_session.state["flow_state"]["last_scan_cidr"],
            "192.168.1.0/24"
        );
        assert_eq!(saved_session.resume_token.as_deref(), Some("resume-1"));
        assert_eq!(store.load("chat-1").expect("legacy load"), state);
        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn approval_and_event_records_round_trip() {
        let path = unique_store_path("harbornas-task-governance");
        let store = TaskConversationStore::new(&path);
        let approval = ApprovalTicket {
            approval_id: "approval-1".to_string(),
            task_id: "task-1".to_string(),
            policy_ref: "camera.connect".to_string(),
            requester_user_id: "user-1".to_string(),
            approver_user_id: None,
            status: ApprovalStatus::Pending,
            reason: "camera.connect requires approval".to_string(),
            requested_at: Some("1710000000".to_string()),
            decided_at: None,
        };
        store.save_approval(&approval).expect("save approval");
        store
            .replace_events_for_step(
                "task-1",
                Some("step-1"),
                &[EventRecord {
                    event_id: "event-1".to_string(),
                    workspace_id: "home-1".to_string(),
                    source_kind: EventSourceKind::Task,
                    source_id: "task-1".to_string(),
                    event_type: "task.needs_input".to_string(),
                    severity: EventSeverity::Warning,
                    payload: json!({"message": "需要审批"}),
                    correlation_id: Some("trace-1".to_string()),
                    causation_id: Some("step-1".to_string()),
                    occurred_at: Some("1710000001".to_string()),
                    ingested_at: Some("1710000001".to_string()),
                }],
            )
            .expect("save events");

        let approvals = store.approvals_for_task("task-1").expect("load approvals");
        let events = store.events_for_task("task-1").expect("load events");

        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].status, ApprovalStatus::Pending);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "task.needs_input");

        let updated = store
            .resolve_pending_approvals(
                "task-1",
                Some("approver-1".to_string()),
                Some("1710000002".to_string()),
            )
            .expect("resolve approvals");
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].status, ApprovalStatus::Approved);
        let _ = fs::remove_file(store.path());
    }
}
