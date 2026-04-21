//! Minimal Assistant Task API service for HarborBeacon integration.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::adapters::rtsp::CommandRtspAdapter;
use crate::connectors::notifications::{
    NotificationAttachment, NotificationAttachmentKind, NotificationContent, NotificationDelivery,
    NotificationDeliveryError, NotificationDeliveryMode, NotificationDeliveryService,
    NotificationDestination, NotificationDestinationKind, NotificationMetadata,
    NotificationPayloadFormat, NotificationRequest, NotificationSource,
};
#[cfg(test)]
use crate::connectors::notifications::{NotificationRecipient, NotificationRecipientIdType};
use crate::connectors::storage::StorageTarget;
use crate::control_plane::approvals::{ApprovalStatus, ApprovalTicket};
use crate::control_plane::events::{EventRecord, EventSeverity, EventSourceKind};
use crate::control_plane::media::{
    MediaAsset, MediaAssetKind, MediaDeliveryMode, MediaSession, MediaSessionKind,
    MediaSessionStatus, RecordingPolicy, ShareAccessScope, ShareLink, StorageTargetKind,
};
use crate::control_plane::tasks::{
    ArtifactKind, ArtifactRecord, ConversationSession, ExecutionRoute, TaskRun, TaskRunStatus,
    TaskStepRun, TaskStepRunStatus,
};
use crate::domains::knowledge::{DOMAIN as KNOWLEDGE_DOMAIN, OP_SEARCH as KNOWLEDGE_OP_SEARCH};
use crate::domains::vision::OP_ANALYZE_CAMERA;
use crate::orchestrator::approval::{ApprovalManager, AutonomyConfig, AutonomyLevel};
use crate::orchestrator::contracts::{Action, ExecutionResult, RiskLevel, StepStatus};
use crate::orchestrator::executors::harbor_ops::{register_harbor_executors, HarborExecutorConfig};
use crate::orchestrator::executors::vision::VisionExecutor;
use crate::orchestrator::policy::{
    action_requires_approval, apply_governance_defaults, effective_risk_level, enforce,
    ApprovalContext,
};
use crate::orchestrator::router::{Executor, Router};
use crate::runtime::admin_console::{
    harboros_writable_root, AdminConsoleState, AdminConsoleStore, NotificationTargetRecord,
};
#[cfg(test)]
use crate::runtime::admin_console::{resolved_identity_binding_records, IdentityBindingRecord};
use crate::runtime::hub::{
    looks_like_auth_error, CameraConnectRequest, CameraHubService, HubScanRequest,
    HubScanResultItem,
};
use crate::runtime::knowledge::{
    KnowledgeSearchRequest, KnowledgeSearchResponse, KnowledgeSearchService,
};
use crate::runtime::media::{ClipCaptureRequest, ClipCaptureResult, SnapshotCaptureResult};
use crate::runtime::model_center::{
    run_llm_text_with_state, run_ocr_with_state, run_vlm_summary_with_state,
};
use crate::runtime::registry::ResolvedCameraTarget;
use crate::runtime::remote_view;
use crate::runtime::task_session::{
    session_state_value_from_conversation, PendingTaskCandidate, PendingTaskConnect,
    TaskConversationState, TaskConversationStore,
};

const ALLOW_NON_HARBOROS_CAPTURE_ROOT_ENV: &str = "HARBOR_ALLOW_NON_HARBOROS_CAPTURE_ROOT";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskSource {
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub surface: String,
    #[serde(default)]
    pub conversation_id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub route_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskMessageMention {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskMessageAttachmentDownloadAuth {
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskMessageAttachmentDownload {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub headers: Value,
    #[serde(default)]
    pub auth: Option<TaskMessageAttachmentDownloadAuth>,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub max_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskMessageAttachment {
    #[serde(default)]
    pub attachment_id: String,
    #[serde(rename = "type", default)]
    pub attachment_type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub download: Option<TaskMessageAttachmentDownload>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskMessage {
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub chat_type: String,
    #[serde(default)]
    pub mentions: Vec<TaskMessageMention>,
    #[serde(default)]
    pub attachments: Vec<TaskMessageAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskIntent {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub raw_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskAutonomy {
    #[serde(default = "default_task_autonomy_level")]
    pub level: String,
}

impl Default for TaskAutonomy {
    fn default() -> Self {
        Self {
            level: default_task_autonomy_level(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskRequest {
    #[serde(default = "new_task_id")]
    pub task_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub step_id: String,
    #[serde(default)]
    pub source: TaskSource,
    #[serde(default)]
    pub intent: TaskIntent,
    #[serde(default)]
    pub entity_refs: Value,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub autonomy: TaskAutonomy,
    #[serde(default)]
    pub message: Option<TaskMessage>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Completed,
    NeedsInput,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskArtifact {
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub media_asset_id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskResultEnvelope {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub artifacts: Vec<TaskArtifact>,
    #[serde(default)]
    pub events: Vec<Value>,
    #[serde(default)]
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskResponse {
    pub task_id: String,
    pub trace_id: String,
    pub status: TaskStatus,
    pub executor_used: String,
    pub risk_level: RiskLevel,
    #[serde(default)]
    pub result: TaskResultEnvelope,
    pub audit_ref: String,
    #[serde(default)]
    pub missing_fields: Vec<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub resume_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskApprovalSummary {
    pub approval_ticket: ApprovalTicket,
    pub source_channel: String,
    pub surface: String,
    pub conversation_id: String,
    pub user_id: String,
    pub session_id: String,
    pub domain: String,
    pub action: String,
    pub intent_text: String,
    pub autonomy_level: String,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskRequestAcceptance {
    Accept,
    Replay(TaskResponse),
    Conflict(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GeneralMessagePlanKind {
    CameraSnapshot,
    CameraRecordClip,
    KnowledgeSearch,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneralMessagePlan {
    kind: GeneralMessagePlanKind,
    camera_hint: Option<String>,
    query: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
struct GeneralMessagePlanPayload {
    #[serde(default)]
    action: String,
    #[serde(default)]
    camera_hint: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskApiService {
    admin_store: AdminConsoleStore,
    conversation_store: TaskConversationStore,
}

#[derive(Debug, Clone)]
struct TaskRuntimeTracking {
    session_id: String,
    step_id: String,
    started_at: String,
}

#[derive(Debug, Clone)]
struct NotificationDeliveryOutcome {
    event_type: &'static str,
    severity: EventSeverity,
    payload: Value,
}

impl TaskApiService {
    pub fn new(admin_store: AdminConsoleStore, conversation_store: TaskConversationStore) -> Self {
        Self {
            admin_store,
            conversation_store,
        }
    }

    pub fn conversation_store(&self) -> &TaskConversationStore {
        &self.conversation_store
    }

    pub fn accept_or_replay_task(
        &self,
        request: &TaskRequest,
    ) -> Result<TaskRequestAcceptance, String> {
        if request.task_id.trim().is_empty() {
            return Ok(TaskRequestAcceptance::Accept);
        }

        let Some(task_run) = self.conversation_store.load_task_run(&request.task_id)? else {
            return Ok(TaskRequestAcceptance::Accept);
        };

        let incoming_identity = task_request_identity(request);
        let existing_identity = persisted_task_request_identity(&task_run);
        if existing_identity != incoming_identity {
            return Ok(TaskRequestAcceptance::Conflict(
                "task_id already exists with a different request identity".to_string(),
            ));
        }

        Ok(TaskRequestAcceptance::Replay(
            self.replay_task_response(&task_run)?,
        ))
    }

    pub fn pending_approvals(&self) -> Result<Vec<TaskApprovalSummary>, String> {
        self.conversation_store
            .pending_approvals()?
            .into_iter()
            .map(|approval| self.load_approval_summary(&approval))
            .collect()
    }

    pub fn approve_pending_approval(
        &self,
        approval_id: &str,
        approver_user_id: Option<String>,
    ) -> Result<(TaskApprovalSummary, TaskResponse), String> {
        let (approval, task_run, session) = self.load_approval_context(approval_id)?;
        if approval.status != ApprovalStatus::Pending {
            return Err(format!("approval is not pending: {}", approval.approval_id));
        }

        let request = self.build_approval_resume_request(
            &approval,
            &task_run,
            session.as_ref(),
            approver_user_id.clone(),
        );
        let response = self.handle_task(request);

        let updated_approval = self
            .conversation_store
            .load_approval(approval_id)?
            .unwrap_or(approval.clone());
        self.record_approval_decision_event(
            &updated_approval,
            &task_run,
            session.as_ref(),
            "task.approval_approved",
            EventSeverity::Info,
            approver_user_id,
        )?;
        Ok((self.load_approval_summary(&updated_approval)?, response))
    }

    pub fn reject_pending_approval(
        &self,
        approval_id: &str,
        approver_user_id: Option<String>,
    ) -> Result<TaskApprovalSummary, String> {
        let (approval, mut task_run, session) = self.load_approval_context(approval_id)?;
        if approval.status != ApprovalStatus::Pending {
            return Err(format!("approval is not pending: {}", approval.approval_id));
        }

        let decided_at = Some(current_timestamp());
        let updated_approval = self
            .conversation_store
            .update_approval_status(
                approval_id,
                ApprovalStatus::Rejected,
                approver_user_id.clone(),
                decided_at.clone(),
            )?
            .ok_or_else(|| format!("approval not found: {approval_id}"))?;

        task_run.status = TaskRunStatus::Failed;
        task_run.completed_at = decided_at;
        self.conversation_store.save_task_run(&task_run)?;

        if let Some(mut session) = session.clone() {
            session.resume_token = None;
            self.conversation_store.save_session(&session)?;
        }

        self.record_approval_decision_event(
            &updated_approval,
            &task_run,
            session.as_ref(),
            "task.approval_rejected",
            EventSeverity::Warning,
            approver_user_id,
        )?;
        Ok(self.load_approval_summary(&updated_approval)?)
    }

    pub fn handle_task(&self, mut request: TaskRequest) -> TaskResponse {
        if request.task_id.trim().is_empty() {
            request.task_id = new_task_id();
        }
        if request.trace_id.trim().is_empty() {
            request.trace_id = request.task_id.clone();
        }
        let _ = self.admin_store.record_member_interactive_surface(
            &request.source.user_id,
            &request.source.channel,
            Some(&request.source.route_key),
        );
        let tracking = self.begin_task_tracking(&request);

        let mut response = match (
            request.intent.domain.trim().to_lowercase(),
            request.intent.action.trim().to_lowercase(),
        ) {
            (domain, action) if domain == KNOWLEDGE_DOMAIN && action == KNOWLEDGE_OP_SEARCH => {
                self.handle_knowledge_search(&request)
            }
            (domain, action) if domain == "general" && action == "message" => {
                self.handle_general_message(&request)
            }
            (domain, action) if is_supported_harbor_task(&domain, &action) => {
                self.handle_harbor_system_action(&request)
            }
            (domain, action) if domain == "camera" && action == "scan" => {
                self.handle_camera_scan(&request)
            }
            (domain, action) if domain == "camera" && action == "connect" => {
                self.handle_camera_connect(&request)
            }
            (domain, action) if domain == "camera" && action == "snapshot" => {
                self.handle_camera_snapshot(&request)
            }
            (domain, action)
                if domain == "camera" && (action == "share_link" || action == "live_view") =>
            {
                self.handle_camera_share_link(&request)
            }
            (domain, action) if domain == "camera" && action == "analyze" => {
                self.handle_camera_analyze(&request)
            }
            (domain, action) => self.failed(
                &request,
                "task_api",
                RiskLevel::Low,
                format!("unsupported task action: {domain}.{action}"),
            ),
        };
        self.append_task_lifecycle_event(&request, &tracking, &mut response);
        let _ = self.finish_task_tracking(&request, &response, &tracking);
        response
    }

    fn replay_task_response(&self, task_run: &TaskRun) -> Result<TaskResponse, String> {
        let trace_id = task_run
            .metadata
            .pointer("/trace_id")
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| task_run.task_id.clone());
        let step_id = task_run
            .metadata
            .pointer("/step_id")
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .filter(|value| !value.trim().is_empty());
        let task_step = step_id
            .as_deref()
            .map(|value| self.conversation_store.load_task_step(value))
            .transpose()?
            .flatten();

        let artifacts = self
            .conversation_store
            .artifacts_for_task(&task_run.task_id)?
            .into_iter()
            .filter(|artifact| {
                step_id.is_none() || artifact.step_id.as_deref() == step_id.as_deref()
            })
            .map(task_artifact_from_record)
            .collect::<Vec<_>>();
        let events = self
            .conversation_store
            .events_for_task(&task_run.task_id)?
            .into_iter()
            .filter(|event| {
                step_id.is_none() || event.causation_id.as_deref() == step_id.as_deref()
            })
            .map(|event| serde_json::to_value(event).unwrap_or(Value::Null))
            .collect::<Vec<_>>();

        let (executor_used, audit_ref, output_payload) = if let Some(task_step) = task_step {
            (
                task_step.executor_used,
                task_step.audit_ref.unwrap_or_default(),
                task_step.output_payload,
            )
        } else {
            ("task_api_dispatch".to_string(), String::new(), Value::Null)
        };

        Ok(TaskResponse {
            task_id: task_run.task_id.clone(),
            trace_id,
            status: task_status_from_task_run_status(task_run.status),
            executor_used,
            risk_level: task_run.risk_level,
            result: TaskResultEnvelope {
                message: string_at_paths(&output_payload, &["/message"]).unwrap_or_default(),
                data: output_payload
                    .pointer("/data")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                artifacts,
                events,
                next_actions: string_vec_at_paths(&output_payload, &["/next_actions"]),
            },
            audit_ref,
            missing_fields: string_vec_at_paths(&output_payload, &["/missing_fields"]),
            prompt: string_at_paths(&output_payload, &["/prompt"]),
            resume_token: string_at_paths(&output_payload, &["/resume_token"]),
        })
    }

    fn handle_harbor_system_action(&self, request: &TaskRequest) -> TaskResponse {
        let action = match build_harbor_action_from_request(request) {
            Ok(action) => action,
            Err(error) => {
                return self.failed(
                    request,
                    "harboros_router",
                    expected_risk_level(request),
                    error,
                );
            }
        };

        if let Err(response) = self.ensure_action_allowed(request, &action, "harboros_router") {
            return response;
        }

        let mut router = Router::new();
        if let Err(error) =
            register_harbor_executors(&mut router, &HarborExecutorConfig::from_env())
        {
            let error_message = error.clone();
            let data = json!({
                "domain": action.domain.clone(),
                "operation": action.operation.clone(),
                "resource": action.resource.clone(),
                "executor_used": "harboros_router",
                "route_fallback_used": false,
                "error_code": "EXECUTOR_CONFIG_ERROR",
                "error_message": error_message,
            });
            let event = self.serialize_event_record(&build_task_event_record(
                request,
                &step_id_for_request(request),
                "task.harboros_failed",
                EventSeverity::Error,
                data.clone(),
            ));
            return self.failed_with_context(
                request,
                "harboros_router",
                action.risk_level,
                format!("HarborOS executor configuration error: {error}"),
                data,
                vec![event],
            );
        }

        let execution = router.execute(&action, &request.task_id, &step_id_for_request(request));
        self.harbor_response_from_execution(request, &action, execution)
    }

    fn harbor_response_from_execution(
        &self,
        request: &TaskRequest,
        action: &Action,
        execution: ExecutionResult,
    ) -> TaskResponse {
        let preview = harbor_execution_is_preview(&execution.result_payload);
        let data = json!({
            "domain": action.domain.clone(),
            "operation": action.operation.clone(),
            "resource": action.resource.clone(),
            "executor_used": execution.executor_used.clone(),
            "route_fallback_used": execution.fallback_used,
            "duration_ms": execution.duration_ms,
            "preview": preview,
            "result": execution.result_payload.clone(),
            "error_code": execution.error_code.clone(),
            "error_message": execution.error_message.clone(),
        });
        let (status, event_type, severity, message) = if execution.ok() {
            (
                TaskStatus::Completed,
                "task.harboros_dispatched",
                EventSeverity::Info,
                format!(
                    "HarborOS {}.{} 已通过 {} 执行",
                    action.domain, action.operation, execution.executor_used
                ),
            )
        } else {
            (
                TaskStatus::Failed,
                "task.harboros_failed",
                EventSeverity::Error,
                format!(
                    "HarborOS {}.{} 执行失败: {}",
                    action.domain,
                    action.operation,
                    execution
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "unknown error".to_string())
                ),
            )
        };
        let event = self.serialize_event_record(&build_task_event_record(
            request,
            &step_id_for_request(request),
            event_type,
            severity,
            data.clone(),
        ));

        TaskResponse {
            task_id: request.task_id.clone(),
            trace_id: request.trace_id.clone(),
            status,
            executor_used: execution.executor_used.clone(),
            risk_level: action.risk_level,
            result: TaskResultEnvelope {
                message,
                data,
                artifacts: Vec::new(),
                events: vec![event],
                next_actions: Vec::new(),
            },
            audit_ref: non_empty_audit_ref(&execution.audit_ref),
            missing_fields: Vec::new(),
            prompt: None,
            resume_token: None,
        }
    }

    fn handle_camera_scan(&self, request: &TaskRequest) -> TaskResponse {
        let hub = self.hub();
        let scan_request = HubScanRequest {
            cidr: string_at_paths(&request.args, &["/cidr"]),
            protocol: protocol_string(&request.args),
        };
        let action = apply_governance_defaults(Action {
            domain: "camera".to_string(),
            operation: "scan".to_string(),
            resource: json!({
                "workspace_id": workspace_id_for_request(request),
            }),
            args: json!({
                "cidr": scan_request.cidr.clone(),
                "protocol": scan_request.protocol.clone(),
            }),
            risk_level: RiskLevel::Low,
            requires_approval: request_requires_approval(request),
            dry_run: false,
        });
        if let Err(response) = self.ensure_action_allowed(request, &action, "camera_hub_service") {
            return response;
        }

        match hub.scan(scan_request, None) {
            Ok(summary) => {
                let pending_candidates = pending_candidates_from_results(&summary.results);
                let mut conversation = self.load_or_create_conversation(request);
                conversation.set_camera_pending_candidates(pending_candidates.clone());
                conversation.set_camera_pending_connect(None);
                conversation.last_scan_cidr = summary.defaults.cidr.clone();
                let _ = self.save_conversation(request, &conversation);

                let message = format_scan_message(
                    &summary.defaults.cidr,
                    &summary.results,
                    &pending_candidates,
                    summary.devices.len(),
                );
                let next_actions = if pending_candidates.is_empty() {
                    vec!["分析客厅摄像头".to_string()]
                } else {
                    vec!["接入 1".to_string(), "密码 xxxxxx".to_string()]
                };
                self.completed(
                    request,
                    "camera_hub_service",
                    RiskLevel::Low,
                    message,
                    json!({
                        "summary": {
                            "scanned_hosts": summary.scanned_hosts,
                            "devices": summary.devices.len(),
                            "results": summary.results.len(),
                        },
                        "candidates": summary.results,
                    }),
                    Vec::new(),
                    next_actions,
                )
            }
            Err(error) => self.failed(request, "camera_hub_service", RiskLevel::Low, error),
        }
    }

    fn handle_camera_connect(&self, request: &TaskRequest) -> TaskResponse {
        let action = apply_governance_defaults(Action {
            domain: "camera".to_string(),
            operation: "connect".to_string(),
            resource: json!({
                "candidate_index": usize_at_paths(&request.entity_refs, &["/candidate_index"])
                    .or_else(|| usize_at_paths(&request.args, &["/candidate_index"])),
                "ip": first_string(&[&request.entity_refs, &request.args], &["/ip"]),
                "resume_token": string_at_paths(&request.args, &["/resume_token"]),
            }),
            args: request.args.clone(),
            risk_level: RiskLevel::Low,
            requires_approval: request_requires_approval(request),
            dry_run: false,
        });
        if let Err(response) = self.ensure_action_allowed(request, &action, "camera_hub_service") {
            return response;
        }

        if let Some(resume_token) = string_at_paths(&request.args, &["/resume_token"]) {
            return self.resume_camera_connect(request, &resume_token);
        }

        if let Some(index) = usize_at_paths(&request.entity_refs, &["/candidate_index"]) {
            return self.connect_camera_candidate(request, index);
        }
        if let Some(index) = usize_at_paths(&request.args, &["/candidate_index"]) {
            return self.connect_camera_candidate(request, index);
        }

        self.connect_camera_direct(request)
    }

    fn connect_camera_candidate(&self, request: &TaskRequest, index: usize) -> TaskResponse {
        let mut conversation = self.load_or_create_conversation(request);
        let pending_candidates = conversation.camera_pending_candidates();
        if pending_candidates.is_empty() {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "当前没有可继续的候选设备列表，请先发送“扫描摄像头”。".to_string(),
            );
        }

        if index == 0 || index > pending_candidates.len() {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "当前没有这个序号的候选设备，请先发送“扫描摄像头”刷新列表。".to_string(),
            );
        }

        let candidate = pending_candidates[index - 1].clone();
        let connect_request = candidate_to_connect_request(&candidate, None);
        match self.hub().manual_add(connect_request, None) {
            Ok(summary) => {
                conversation.set_camera_pending_connect(None);
                conversation.retain_camera_pending_candidates(|item| {
                    item.candidate_id != candidate.candidate_id
                });
                let _ = self.save_conversation(request, &conversation);
                self.completed(
                    request,
                    "camera_hub_service",
                    RiskLevel::Medium,
                    format!(
                        "已接入 {}（{}），设备库现在共有 {} 台。",
                        candidate.name,
                        candidate.ip,
                        summary.devices.len()
                    ),
                    json!({
                        "device": summary.device,
                        "devices": summary.devices.len(),
                    }),
                    Vec::new(),
                    vec!["分析客厅摄像头".to_string()],
                )
            }
            Err(error) if looks_like_auth_error(&error) => {
                let resume_token = ensure_resume_token();
                conversation.set_camera_pending_connect(Some(PendingTaskConnect {
                    resume_token: resume_token.clone(),
                    name: candidate.name.clone(),
                    ip: candidate.ip.clone(),
                    room: candidate.room.clone(),
                    port: candidate.port,
                    snapshot_url: None,
                    rtsp_paths: candidate.rtsp_paths.clone(),
                    requires_auth: true,
                    vendor: candidate.vendor.clone(),
                    model: candidate.model.clone(),
                }));
                let _ = self.save_conversation(request, &conversation);
                self.needs_input(
                    request,
                    "camera_hub_service",
                    RiskLevel::Medium,
                    "这台摄像头需要密码，请回复：密码 xxxxxx".to_string(),
                    vec!["password".to_string()],
                    resume_token,
                )
            }
            Err(error) => self.failed(request, "camera_hub_service", RiskLevel::Medium, error),
        }
    }

    fn connect_camera_direct(&self, request: &TaskRequest) -> TaskResponse {
        let Some(ip) = first_string(&[&request.entity_refs, &request.args], &["/ip"]) else {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "缺少摄像头 IP 地址，当前最小实现只支持“接入 1”或显式提供 IP。".to_string(),
            );
        };

        let pending = PendingTaskConnect {
            resume_token: String::new(),
            name: first_string(&[&request.entity_refs, &request.args], &["/name"])
                .unwrap_or_else(|| format!("Camera {ip}")),
            ip: ip.clone(),
            room: first_string(&[&request.entity_refs, &request.args], &["/room"]),
            port: first_u16(&[&request.entity_refs, &request.args], &["/port"]).unwrap_or(554),
            snapshot_url: first_string(&[&request.entity_refs, &request.args], &["/snapshot_url"]),
            rtsp_paths: first_string_vec(
                &[&request.entity_refs, &request.args],
                &["/path_candidates", "/rtsp_paths"],
            ),
            requires_auth: false,
            vendor: first_string(&[&request.entity_refs, &request.args], &["/vendor"]),
            model: first_string(&[&request.entity_refs, &request.args], &["/model"]),
        };
        let connect_request =
            pending_connect_to_request(&pending, first_string(&[&request.args], &["/password"]));

        match self.hub().manual_add(connect_request, None) {
            Ok(summary) => self.completed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                format!(
                    "已接入摄像头 {}，设备库现在共有 {} 台。",
                    summary.device.ip_address.clone().unwrap_or(ip),
                    summary.devices.len()
                ),
                json!({
                    "device": summary.device,
                    "devices": summary.devices.len(),
                }),
                Vec::new(),
                vec!["分析客厅摄像头".to_string()],
            ),
            Err(error) if looks_like_auth_error(&error) => {
                let mut conversation = self.load_or_create_conversation(request);
                let resume_token = ensure_resume_token();
                let mut pending_with_token = pending.clone();
                pending_with_token.resume_token = resume_token.clone();
                conversation.set_camera_pending_connect(Some(pending_with_token));
                let _ = self.save_conversation(request, &conversation);
                self.needs_input(
                    request,
                    "camera_hub_service",
                    RiskLevel::Medium,
                    "这台摄像头需要密码，请回复：密码 xxxxxx".to_string(),
                    vec!["password".to_string()],
                    resume_token,
                )
            }
            Err(error) => self.failed(request, "camera_hub_service", RiskLevel::Medium, error),
        }
    }

    fn resume_camera_connect(&self, request: &TaskRequest, resume_token: &str) -> TaskResponse {
        let Some(password) = string_at_paths(&request.args, &["/password"]) else {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "缺少 password，无法继续接入流程。".to_string(),
            );
        };
        let mut conversation = self.load_or_create_conversation(request);
        let Some(pending) = conversation.camera_pending_connect() else {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "当前没有待继续的接入流程，请重新发送“扫描摄像头”。".to_string(),
            );
        };
        if pending.resume_token != resume_token {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "接入令牌已失效，请重新发送“扫描摄像头”。".to_string(),
            );
        }

        match self
            .hub()
            .manual_add(pending_connect_to_request(&pending, Some(password)), None)
        {
            Ok(summary) => {
                conversation.set_camera_pending_connect(None);
                conversation
                    .retain_camera_pending_candidates(|candidate| candidate.ip != pending.ip);
                let _ = self.save_conversation(request, &conversation);
                self.completed(
                    request,
                    "camera_hub_service",
                    RiskLevel::Medium,
                    format!(
                        "密码已收到。\n已接入摄像头 {}，设备库现在共有 {} 台。",
                        summary.device.ip_address.clone().unwrap_or(pending.ip),
                        summary.devices.len()
                    ),
                    json!({
                        "device": summary.device,
                        "devices": summary.devices.len(),
                    }),
                    Vec::new(),
                    vec!["分析客厅摄像头".to_string()],
                )
            }
            Err(error) if looks_like_auth_error(&error) => self.needs_input(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "这个密码还是不对，请再回复一次：密码 xxxxxx".to_string(),
                vec!["password".to_string()],
                pending.resume_token,
            ),
            Err(error) => self.failed(request, "camera_hub_service", RiskLevel::Medium, error),
        }
    }

    fn handle_camera_analyze(&self, request: &TaskRequest) -> TaskResponse {
        let target = match self.resolve_camera_target(request) {
            Ok(target) => target,
            Err(error) => {
                return self.failed(request, "vision_executor", RiskLevel::Low, error);
            }
        };

        let detect_label = first_string(&[&request.args], &["/detect_label"])
            .unwrap_or_else(|| "person".to_string());
        let min_confidence = request
            .args
            .pointer("/min_confidence")
            .and_then(Value::as_f64)
            .unwrap_or(0.25);
        let prompt = first_string(&[&request.args], &["/prompt"]);

        let action = apply_governance_defaults(Action {
            domain: "vision".to_string(),
            operation: OP_ANALYZE_CAMERA.to_string(),
            resource: json!({ "device_id": target.device_id }),
            args: json!({
                "detect_label": detect_label,
                "min_confidence": min_confidence,
                "prompt": prompt,
            }),
            risk_level: RiskLevel::Low,
            requires_approval: request_requires_approval(request),
            dry_run: false,
        });
        if let Err(response) = self.ensure_action_allowed(request, &action, "vision_executor") {
            return response;
        }

        let vision = VisionExecutor::new(self.admin_store.registry_store().clone());
        match vision.execute(&action, &request.task_id, &step_id_for_request(request)) {
            Ok(result) if result.status == StepStatus::Success => {
                let summary =
                    string_at_paths(&result.result_payload, &["/summary", "/detection_summary"])
                        .unwrap_or_else(|| "分析完成".to_string());
                let mut payload = result.result_payload;
                if let Err(error) = self.persist_vision_media_assets(request, &target, &mut payload)
                {
                    return self.failed(
                        request,
                        "vision_executor",
                        RiskLevel::Low,
                        format!("分析已完成，但保存媒体记录失败: {error}"),
                    );
                }
                let artifacts = build_vision_artifacts(&payload);
                let notification_request = self.build_notification_request(
                    request,
                    "task.completed",
                    &target,
                    &payload,
                    &artifacts,
                );
                let mut events = Vec::new();
                if let Some(notification_request) = notification_request {
                    let encoded =
                        serde_json::to_value(&notification_request).unwrap_or(Value::Null);
                    if let Some(object) = payload.as_object_mut() {
                        object.insert("notification_request".to_string(), encoded.clone());
                    }
                    events.push(self.serialize_event_record(&build_task_event_record(
                        request,
                        &step_id_for_request(request),
                        "task.notification_requested",
                        EventSeverity::Info,
                        json!({
                            "executor_used": "vision_executor",
                            "notification": encoded,
                        }),
                    )));
                    let delivery_outcome = self.deliver_notification_request(&notification_request);
                    if let Some(object) = payload.as_object_mut() {
                        object.insert(
                            "notification_delivery".to_string(),
                            delivery_outcome.payload.clone(),
                        );
                        if notification_request.destination.kind
                            == NotificationDestinationKind::Conversation
                        {
                            object.insert(
                                "interaction_reply".to_string(),
                                delivery_outcome.payload.clone(),
                            );
                        }
                        if notification_request.destination.kind
                            == NotificationDestinationKind::Recipient
                            && delivery_outcome.event_type == "task.proactive_delivery_failed"
                        {
                            object.insert(
                                "proactive_delivery_failure".to_string(),
                                delivery_outcome.payload.clone(),
                            );
                        }
                    }
                    events.push(self.serialize_event_record(&build_task_event_record(
                        request,
                        &step_id_for_request(request),
                        delivery_outcome.event_type,
                        delivery_outcome.severity,
                        json!({
                            "executor_used": "vision_executor",
                            "notification_request": notification_request,
                            "delivery": delivery_outcome.payload,
                        }),
                    )));
                }
                self.completed_with_context(
                    request,
                    "vision_executor",
                    RiskLevel::Low,
                    format!("{} 分析完成：{}", target.display_name, summary),
                    payload,
                    artifacts,
                    events,
                    Vec::new(),
                )
            }
            Ok(result) => self.failed(
                request,
                "vision_executor",
                RiskLevel::Low,
                result
                    .error_message
                    .unwrap_or_else(|| "vision executor failed".to_string()),
            ),
            Err(error) => self.failed(request, "vision_executor", RiskLevel::Low, error),
        }
    }

    fn handle_general_message(&self, request: &TaskRequest) -> TaskResponse {
        let plan = match self.general_message_plan(request) {
            Ok(plan) => plan,
            Err(error) => {
                return self.failed(request, "agentic_interpreter", RiskLevel::Low, error);
            }
        };

        match plan.kind {
            GeneralMessagePlanKind::CameraSnapshot => {
                let mut routed = request.clone();
                routed.intent.domain = "camera".to_string();
                routed.intent.action = "snapshot".to_string();
                if let Some(camera_hint) = plan.camera_hint {
                    upsert_json_string(&mut routed.args, "/device_hint", &camera_hint);
                }
                self.handle_camera_snapshot(&routed)
            }
            GeneralMessagePlanKind::CameraRecordClip => {
                let mut routed = request.clone();
                routed.intent.domain = "camera".to_string();
                routed.intent.action = "record_clip".to_string();
                if let Some(camera_hint) = plan.camera_hint {
                    upsert_json_string(&mut routed.args, "/device_hint", &camera_hint);
                }
                self.handle_camera_record_clip(&routed)
            }
            GeneralMessagePlanKind::KnowledgeSearch => {
                let mut routed = request.clone();
                routed.intent.domain = KNOWLEDGE_DOMAIN.to_string();
                routed.intent.action = KNOWLEDGE_OP_SEARCH.to_string();
                if let Some(query) = plan.query {
                    upsert_json_string(&mut routed.args, "/query", &query);
                }
                if routed.args.pointer("/roots").is_none() {
                    if let Some(root) = self.default_capture_search_root() {
                        upsert_json_string_vec(&mut routed.args, "/roots", &[root]);
                    }
                }
                self.handle_knowledge_search(&routed)
            }
            GeneralMessagePlanKind::Unsupported => self.failed(
                request,
                "agentic_interpreter",
                RiskLevel::Low,
                plan.reason.unwrap_or_else(|| {
                    "当前自然语义没有被解释成可执行的抓拍、短视频或检索任务。".to_string()
                }),
            ),
        }
    }

    fn general_message_plan(&self, request: &TaskRequest) -> Result<GeneralMessagePlan, String> {
        let admin_state = self.admin_store.load_or_create_state()?;
        let camera_targets = self
            .admin_store
            .registry_store()
            .load_camera_targets()
            .unwrap_or_default();
        let selected_camera = admin_state.defaults.selected_camera_device_id.clone();
        let writable_root = harboros_writable_root();
        let capture_subdirectory = admin_state.defaults.capture_subdirectory.clone();
        let clip_length_seconds = admin_state.defaults.clip_length_seconds;
        let llm_prompt = build_general_message_planner_prompt(
            request,
            &camera_targets,
            selected_camera.as_deref(),
            writable_root.as_str(),
            capture_subdirectory.as_str(),
            clip_length_seconds,
        );

        let llm_plan = run_llm_text_with_state(&llm_prompt, &admin_state.models);
        if llm_plan.available && !llm_plan.text.trim().is_empty() {
            if let Some(plan) = parse_general_message_plan(&llm_plan.text) {
                return Ok(plan);
            }
        }

        fallback_general_message_plan(
            request.intent.raw_text.as_str(),
            selected_camera.as_deref(),
        )
        .ok_or_else(|| {
            if llm_plan.summary.trim().is_empty() {
                "当前没有可用的自然语义解释器，请先在 Model Center 配置可用的 LLM endpoint。"
                    .to_string()
            } else {
                format!("自然语义解释失败：{}", llm_plan.summary)
            }
        })
    }

    fn handle_camera_record_clip(&self, request: &TaskRequest) -> TaskResponse {
        let target = match self.resolve_camera_target(request) {
            Ok(target) => target,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Low, error);
            }
        };

        let action = apply_governance_defaults(Action {
            domain: "camera".to_string(),
            operation: "record_clip".to_string(),
            resource: json!({ "device_id": target.device_id.clone() }),
            args: json!({ "device_id": target.device_id.clone() }),
            risk_level: RiskLevel::Low,
            requires_approval: request_requires_approval(request),
            dry_run: false,
        });
        if let Err(response) = self.ensure_action_allowed(request, &action, "camera_hub_service") {
            return response;
        }

        let admin_state = match self.admin_store.load_or_create_state() {
            Ok(state) => state,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Low, error);
            }
        };
        let recording_policy = resolved_recording_policy(&admin_state, Some(&target));
        let capture_root = match resolved_capture_directory(&admin_state, recording_policy.as_ref()) {
            Ok(path) => path,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Low, error);
            }
        };
        let clip_length_seconds = recording_policy
            .as_ref()
            .and_then(RecordingPolicy::clip_length_seconds_hint)
            .unwrap_or_else(|| admin_state.defaults.clip_length_seconds)
            .clamp(3, 300);
        let keyframe_count = recording_policy
            .as_ref()
            .and_then(RecordingPolicy::keyframe_count_hint)
            .or(Some(admin_state.defaults.keyframe_count));
        let keyframe_interval_seconds = recording_policy
            .as_ref()
            .and_then(RecordingPolicy::keyframe_interval_seconds_hint)
            .or(Some(admin_state.defaults.keyframe_interval_seconds));

        let clip_path = build_clip_output_path(&capture_root, &target, current_epoch_ms());
        let adapter = CommandRtspAdapter::default();
        let clip_request = ClipCaptureRequest::new(
            target.device_id.clone(),
            target.primary_stream.url.clone(),
            clip_length_seconds,
            StorageTarget::HarborOsPool,
        )
        .with_keyframe_hints(keyframe_count, keyframe_interval_seconds);

        let clip = match adapter.capture_clip_to_path(&clip_request, &clip_path) {
            Ok(result) => result,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Low, error);
            }
        };
        let keyframes_dir = build_keyframe_directory(&capture_root, &clip_path);
        let keyframes = match adapter.extract_keyframes(
            &clip_path,
            &keyframes_dir,
            keyframe_count,
            keyframe_interval_seconds,
        ) {
            Ok(paths) => paths,
            Err(error) => {
                return self.failed(
                    request,
                    "camera_hub_service",
                    RiskLevel::Low,
                    format!("短视频已保存，但关键帧抽取失败: {error}"),
                );
            }
        };
        if let Err(error) = self.persist_clip_ingest(&admin_state, &target, &clip, &keyframes) {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Low,
                format!("短视频已保存，但写入索引副产物失败: {error}"),
            );
        }

        let media_asset = build_clip_media_asset(request, &target, &clip);
        if let Err(error) = self.conversation_store.save_media_asset(&media_asset) {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Low,
                format!("短视频已保存，但保存媒体记录失败: {error}"),
            );
        }

        self.completed(
            request,
            "camera_hub_service",
            RiskLevel::Low,
            format!("已录制 {} 的短视频片段。", target.display_name),
            build_clip_payload(&target, &clip, &keyframes, &media_asset),
            build_clip_artifacts(&clip, &keyframes, &media_asset),
            vec!["检索这段视频".to_string()],
        )
    }

    fn handle_camera_snapshot(&self, request: &TaskRequest) -> TaskResponse {
        let target = match self.resolve_camera_target(request) {
            Ok(target) => target,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Low, error);
            }
        };

        let action = apply_governance_defaults(Action {
            domain: "camera".to_string(),
            operation: "snapshot".to_string(),
            resource: json!({ "device_id": target.device_id.clone() }),
            args: json!({ "device_id": target.device_id.clone() }),
            risk_level: RiskLevel::Low,
            requires_approval: request_requires_approval(request),
            dry_run: false,
        });
        if let Err(response) = self.ensure_action_allowed(request, &action, "camera_hub_service") {
            return response;
        }

        match self.hub().capture_camera_snapshot_result(&target.device_id) {
            Ok(snapshot) => {
                let admin_state = match self.admin_store.load_or_create_state() {
                    Ok(state) => state,
                    Err(error) => {
                        return self.failed(request, "camera_hub_service", RiskLevel::Low, error);
                    }
                };
                let recording_policy = resolved_recording_policy(&admin_state, Some(&target));
                let snapshot = match self.persist_snapshot_capture(
                    &admin_state,
                    recording_policy.as_ref(),
                    &target,
                    snapshot,
                ) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        return self.failed(
                            request,
                            "camera_hub_service",
                            RiskLevel::Low,
                            format!("抓拍已完成，但保存图片失败: {error}"),
                        );
                    }
                };
                let media_asset = build_snapshot_media_asset(request, &target, &snapshot);
                if let Err(error) = self.conversation_store.save_media_asset(&media_asset) {
                    return self.failed(
                        request,
                        "camera_hub_service",
                        RiskLevel::Low,
                        format!("抓拍已完成，但保存媒体记录失败: {error}"),
                    );
                }

                self.completed(
                    request,
                    "camera_hub_service",
                    RiskLevel::Low,
                    format!("已抓拍 {} 当前画面。", target.display_name),
                    build_snapshot_payload(&target, &snapshot, &media_asset),
                    vec![build_snapshot_artifact(&snapshot, &media_asset)],
                    vec![format!("分析 {}", target.display_name)],
                )
            }
            Err(error) => self.failed(request, "camera_hub_service", RiskLevel::Low, error),
        }
    }

    fn handle_camera_share_link(&self, request: &TaskRequest) -> TaskResponse {
        let target = match self.resolve_camera_target(request) {
            Ok(target) => target,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Medium, error);
            }
        };

        let action = apply_governance_defaults(Action {
            domain: "camera".to_string(),
            operation: "share_link".to_string(),
            resource: json!({ "device_id": target.device_id.clone() }),
            args: json!({ "device_id": target.device_id.clone() }),
            risk_level: RiskLevel::Medium,
            requires_approval: request_requires_approval(request),
            dry_run: false,
        });
        if let Err(response) = self.ensure_action_allowed(request, &action, "camera_hub_service") {
            return response;
        }

        let remote_view_config = match self.admin_store.load_remote_view_config() {
            Ok(config) => config,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Medium, error);
            }
        };
        let issued = match remote_view::issue_camera_share_token(
            &remote_view_config.share_secret,
            &target.device_id,
            remote_view_config.share_link_ttl_minutes,
        ) {
            Ok(issued) => issued,
            Err(error) => {
                return self.failed(request, "camera_hub_service", RiskLevel::Medium, error);
            }
        };

        let share_link_id = new_share_link_id();
        let media_session_id = new_media_session_id();
        let media_session =
            build_share_media_session(request, &target, &media_session_id, &share_link_id);
        let share_link_record = build_share_link_record(&issued, &media_session_id, &share_link_id);
        if let Err(error) = self
            .conversation_store
            .save_share_link_bundle(&media_session, &share_link_record)
        {
            return self.failed(request, "camera_hub_service", RiskLevel::Medium, error);
        }

        let share_link =
            build_share_link_payload(&target, &issued, &media_session, &share_link_record);
        let event = self.serialize_event_record(&build_task_event_record(
            request,
            &step_id_for_request(request),
            "task.share_link_issued",
            EventSeverity::Info,
            share_link.clone(),
        ));
        self.completed_with_context(
            request,
            "camera_hub_service",
            RiskLevel::Medium,
            format!(
                "已为 {} 生成 {} 分钟共享观看链接。",
                target.display_name, issued.ttl_minutes
            ),
            json!({
                "camera_target": target,
                "share_link": share_link,
            }),
            vec![build_share_link_artifact(&share_link)],
            vec![event],
            vec!["打开共享观看页".to_string()],
        )
    }

    fn handle_knowledge_search(&self, request: &TaskRequest) -> TaskResponse {
        let action = apply_governance_defaults(Action {
            domain: KNOWLEDGE_DOMAIN.to_string(),
            operation: KNOWLEDGE_OP_SEARCH.to_string(),
            resource: json!({
                "roots": knowledge_search_roots(request),
            }),
            args: request.args.clone(),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        });
        if let Err(response) =
            self.ensure_action_allowed(request, &action, "knowledge_search_service")
        {
            return response;
        }

        let Some(query) = knowledge_search_query(request) else {
            return self.failed(
                request,
                "knowledge_search_service",
                RiskLevel::Low,
                "缺少可检索的主题，请提供 query 或更明确地说明要找什么内容。".to_string(),
            );
        };
        let (include_documents, include_images) = knowledge_modalities(request);
        let search_request = KnowledgeSearchRequest {
            query,
            roots: knowledge_search_roots(request),
            include_documents,
            include_images,
            limit: knowledge_result_limit(request),
        };

        match KnowledgeSearchService::search(search_request) {
            Ok(result) => self.completed(
                request,
                "knowledge_search_service",
                RiskLevel::Low,
                format_knowledge_search_message(&result),
                serde_json::to_value(&result).unwrap_or_else(|_| json!({})),
                build_knowledge_search_artifacts(&result),
                knowledge_search_next_actions(&result),
            ),
            Err(error) => self.failed(request, "knowledge_search_service", RiskLevel::Low, error),
        }
    }

    fn resolve_camera_target(&self, request: &TaskRequest) -> Result<ResolvedCameraTarget, String> {
        let targets = self.admin_store.registry_store().load_camera_targets()?;
        if targets.is_empty() {
            return Err("当前还没有已注册的摄像头，请先完成接入。".to_string());
        }

        if let Some(device_id) =
            first_string(&[&request.entity_refs, &request.args], &["/device_id"])
        {
            if let Some(target) = targets.iter().find(|target| target.device_id == device_id) {
                return Ok(target.clone());
            }
        }

        let hint = first_string(
            &[&request.entity_refs, &request.args],
            &["/device_hint", "/room", "/name"],
        )
        .or_else(|| {
            (!request.intent.raw_text.trim().is_empty()).then(|| request.intent.raw_text.clone())
        })
        .unwrap_or_default();
        let normalized = normalize_command_text(&hint);

        for target in &targets {
            let name = target.display_name.as_str();
            let room = target.room_name.as_deref().unwrap_or_default();
            if !name.is_empty() && normalized.contains(&name.replace(' ', "").to_lowercase()) {
                return Ok(target.clone());
            }
            if !room.is_empty() && normalized.contains(&room.replace(' ', "").to_lowercase()) {
                return Ok(target.clone());
            }
            for alias in room_aliases(name, room) {
                if normalized.contains(alias) {
                    return Ok(target.clone());
                }
            }
        }

        if let Ok(state) = self.admin_store.load_or_create_state() {
            if let Some(selected) = state.defaults.selected_camera_device_id.as_deref() {
                if let Some(target) = targets.iter().find(|target| target.device_id == selected) {
                    return Ok(target.clone());
                }
            }
        }

        targets
            .first()
            .cloned()
            .ok_or_else(|| "未找到可分析的摄像头设备。".to_string())
    }

    fn default_capture_search_root(&self) -> Option<String> {
        let state = self.admin_store.load_or_create_state().ok()?;
        resolved_capture_directory(&state, resolved_recording_policy(&state, None).as_ref())
            .ok()
            .map(|path| path.to_string_lossy().to_string())
    }

    fn persist_snapshot_capture(
        &self,
        state: &AdminConsoleState,
        recording_policy: Option<&RecordingPolicy>,
        target: &ResolvedCameraTarget,
        snapshot: SnapshotCaptureResult,
    ) -> Result<SnapshotCaptureResult, String> {
        let capture_root = resolved_capture_directory(state, recording_policy)?;
        let image_bytes = BASE64_STANDARD
            .decode(snapshot.bytes_base64.as_bytes())
            .map_err(|error| format!("failed to decode snapshot bytes: {error}"))?;
        let output_path = build_snapshot_output_path(
            &capture_root,
            target,
            snapshot.captured_at_epoch_ms,
            snapshot.format.file_extension(),
        );
        fs::write(&output_path, &image_bytes)
            .map_err(|error| format!("failed to write snapshot {}: {error}", output_path.display()))?;

        let mut persisted = snapshot;
        persisted.storage.target = StorageTarget::HarborOsPool;
        persisted.storage.relative_path = output_path.to_string_lossy().to_string();
        persisted.index_sidecar_relative_path = output_path
            .with_extension("json")
            .to_string_lossy()
            .to_string();

        let ocr = run_ocr_with_state(&output_path, &state.models);
        let vlm = run_vlm_summary_with_state(&output_path, &state.models);
        let snapshot_tags = vec!["camera".to_string(), "snapshot".to_string()];
        write_media_index_sidecar(
            &output_path.with_extension("json"),
            &persisted.storage.relative_path,
            None,
            target,
            &ocr.text,
            &vlm.text,
            &snapshot_tags,
        )?;

        Ok(persisted)
    }

    fn persist_clip_ingest(
        &self,
        state: &AdminConsoleState,
        target: &ResolvedCameraTarget,
        clip: &ClipCaptureResult,
        keyframes: &[PathBuf],
    ) -> Result<(), String> {
        let clip_path = PathBuf::from(&clip.storage.relative_path);
        let clip_tags = vec!["video".to_string(), "clip".to_string()];
        write_media_index_sidecar(
            &clip_path.with_extension("json"),
            &clip.storage.relative_path,
            None,
            target,
            "",
            &format!(
                "短视频片段，时长 {} 秒，共提取 {} 张关键帧。",
                clip.clip_length_seconds,
                keyframes.len()
            ),
            &clip_tags,
        )?;

        for keyframe in keyframes {
            let ocr = run_ocr_with_state(keyframe, &state.models);
            let vlm = run_vlm_summary_with_state(keyframe, &state.models);
            let keyframe_tags = vec!["video".to_string(), "keyframe".to_string()];
            write_media_index_sidecar(
                &keyframe.with_extension("json"),
                keyframe.to_string_lossy().as_ref(),
                Some(&clip.storage.relative_path),
                target,
                &ocr.text,
                &vlm.text,
                &keyframe_tags,
            )?;
        }

        Ok(())
    }

    fn completed(
        &self,
        request: &TaskRequest,
        executor_used: &str,
        risk_level: RiskLevel,
        message: String,
        data: Value,
        artifacts: Vec<TaskArtifact>,
        next_actions: Vec<String>,
    ) -> TaskResponse {
        self.completed_with_context(
            request,
            executor_used,
            risk_level,
            message,
            data,
            artifacts,
            Vec::new(),
            next_actions,
        )
    }

    fn completed_with_context(
        &self,
        request: &TaskRequest,
        executor_used: &str,
        risk_level: RiskLevel,
        message: String,
        data: Value,
        artifacts: Vec<TaskArtifact>,
        events: Vec<Value>,
        next_actions: Vec<String>,
    ) -> TaskResponse {
        TaskResponse {
            task_id: request.task_id.clone(),
            trace_id: request.trace_id.clone(),
            status: TaskStatus::Completed,
            executor_used: executor_used.to_string(),
            risk_level,
            result: TaskResultEnvelope {
                message,
                data,
                artifacts,
                events,
                next_actions,
            },
            audit_ref: new_audit_ref(),
            missing_fields: Vec::new(),
            prompt: None,
            resume_token: None,
        }
    }

    fn needs_input(
        &self,
        request: &TaskRequest,
        executor_used: &str,
        risk_level: RiskLevel,
        prompt: String,
        missing_fields: Vec<String>,
        resume_token: String,
    ) -> TaskResponse {
        self.needs_input_with_context(
            request,
            executor_used,
            risk_level,
            prompt,
            missing_fields,
            resume_token,
            Value::Null,
            Vec::new(),
            vec!["密码 xxxxxx".to_string()],
        )
    }

    fn needs_input_with_context(
        &self,
        request: &TaskRequest,
        executor_used: &str,
        risk_level: RiskLevel,
        prompt: String,
        missing_fields: Vec<String>,
        resume_token: String,
        data: Value,
        events: Vec<Value>,
        next_actions: Vec<String>,
    ) -> TaskResponse {
        TaskResponse {
            task_id: request.task_id.clone(),
            trace_id: request.trace_id.clone(),
            status: TaskStatus::NeedsInput,
            executor_used: executor_used.to_string(),
            risk_level,
            result: TaskResultEnvelope {
                message: prompt.clone(),
                data,
                artifacts: Vec::new(),
                events,
                next_actions,
            },
            audit_ref: new_audit_ref(),
            missing_fields,
            prompt: Some(prompt),
            resume_token: Some(resume_token),
        }
    }

    fn failed(
        &self,
        request: &TaskRequest,
        executor_used: &str,
        risk_level: RiskLevel,
        message: String,
    ) -> TaskResponse {
        self.failed_with_context(
            request,
            executor_used,
            risk_level,
            message,
            Value::Null,
            Vec::new(),
        )
    }

    fn failed_with_context(
        &self,
        request: &TaskRequest,
        executor_used: &str,
        risk_level: RiskLevel,
        message: String,
        data: Value,
        events: Vec<Value>,
    ) -> TaskResponse {
        TaskResponse {
            task_id: request.task_id.clone(),
            trace_id: request.trace_id.clone(),
            status: TaskStatus::Failed,
            executor_used: executor_used.to_string(),
            risk_level,
            result: TaskResultEnvelope {
                message,
                data,
                artifacts: Vec::new(),
                events,
                next_actions: Vec::new(),
            },
            audit_ref: new_audit_ref(),
            missing_fields: Vec::new(),
            prompt: None,
            resume_token: None,
        }
    }

    fn ensure_action_allowed(
        &self,
        request: &TaskRequest,
        action: &Action,
        executor_used: &str,
    ) -> Result<(), TaskResponse> {
        let autonomy_level = effective_autonomy_level(request);
        let approval_manager = approval_manager_for_level(autonomy_level);
        if !approval_manager.risk_allowed(effective_risk_level(action)) {
            let event = self.serialize_event_record(&build_task_event_record(
                request,
                &step_id_for_request(request),
                "task.autonomy_blocked",
                EventSeverity::Warning,
                json!({
                    "executor_used": executor_used,
                    "autonomy_level": autonomy_level_label(autonomy_level),
                    "policy_ref": format!("{}.{}", action.domain, action.operation),
                    "risk_level": serde_json::to_value(effective_risk_level(action)).unwrap_or(Value::Null),
                }),
            ));
            return Err(self.failed_with_context(
                request,
                executor_used,
                effective_risk_level(action),
                format!(
                    "当前任务处于 {} 模式，无法执行需要写入或变更的操作。",
                    autonomy_level_label(autonomy_level)
                ),
                json!({
                    "error": "AUTONOMY_BLOCKED",
                    "autonomy_level": autonomy_level_label(autonomy_level),
                    "policy_ref": format!("{}.{}", action.domain, action.operation),
                }),
                vec![event],
            ));
        }

        let approval_tickets = self
            .conversation_store
            .approvals_for_task(&request.task_id)
            .unwrap_or_default();
        let pending_approval = approval_tickets
            .iter()
            .find(|approval| approval.status == ApprovalStatus::Pending)
            .cloned();
        let approval_context = approval_context_for_request(request, pending_approval.as_ref());
        let approval_context_ref = approval_context.as_ref();

        if let Err(violation) = enforce(action, approval_context_ref) {
            let approval_id = pending_approval
                .as_ref()
                .map(|approval| approval.approval_id.clone())
                .unwrap_or_else(new_approval_id);
            let ticket = ApprovalTicket {
                approval_id: approval_id.clone(),
                task_id: request.task_id.clone(),
                trace_id: request.trace_id.clone(),
                route_key: request.source.route_key.clone(),
                policy_ref: format!("{}.{}", action.domain, action.operation),
                requester_user_id: request.source.user_id.clone(),
                approver_user_id: None,
                status: ApprovalStatus::Pending,
                reason: violation.message.clone(),
                requested_at: Some(current_timestamp()),
                decided_at: None,
            };
            let _ = self.conversation_store.save_approval(&ticket);
            let policy_ref = format!("{}.{}", action.domain, action.operation);
            let event = self.serialize_event_record(&build_task_event_record(
                request,
                &step_id_for_request(request),
                "task.approval_required",
                EventSeverity::Warning,
                json!({
                    "executor_used": executor_used,
                    "policy_violation": {
                        "code": violation.code.clone(),
                        "message": violation.message.clone(),
                    },
                    "approval_ticket": ticket.clone(),
                }),
            ));
            return Err(self.needs_input_with_context(
                request,
                executor_used,
                action.risk_level,
                "这个操作需要审批，请带 approval_token 重新提交。".to_string(),
                vec!["approval_token".to_string()],
                approval_id.clone(),
                json!({
                    "approval_ticket": ticket,
                    "policy_ref": policy_ref,
                }),
                vec![event],
                vec![format!("approval_token {approval_id}")],
            ));
        }

        if (action_requires_approval(action) || pending_approval.is_some())
            && request_approval_token(request).is_some()
        {
            let _ = self.conversation_store.resolve_pending_approvals(
                &request.task_id,
                request_approver_id(request),
                Some(current_timestamp()),
            );
        }

        Ok(())
    }

    fn append_task_lifecycle_event(
        &self,
        request: &TaskRequest,
        tracking: &TaskRuntimeTracking,
        response: &mut TaskResponse,
    ) {
        let (event_type, severity) = match response.status {
            TaskStatus::Completed => ("task.completed", EventSeverity::Info),
            TaskStatus::NeedsInput => ("task.needs_input", EventSeverity::Warning),
            TaskStatus::Failed => ("task.failed", EventSeverity::Error),
        };
        response
            .result
            .events
            .push(self.serialize_event_record(&build_task_event_record(
                request,
                &tracking.step_id,
                event_type,
                severity,
                json!({
                    "executor_used": response.executor_used.clone(),
                    "risk_level": serde_json::to_value(response.risk_level).unwrap_or(Value::Null),
                    "message": response.result.message.clone(),
                    "missing_fields": response.missing_fields.clone(),
                    "resume_token": response.resume_token.clone(),
                    "audit_ref": response.audit_ref.clone(),
                }),
            )));
    }

    fn build_notification_request(
        &self,
        request: &TaskRequest,
        event_type: &str,
        target: &ResolvedCameraTarget,
        payload: &Value,
        artifacts: &[TaskArtifact],
    ) -> Option<NotificationRequest> {
        let route_key = first_string(
            &[&request.args],
            &["/notification/route_key", "/destination/route_key"],
        )
        .or_else(|| {
            let value = request.source.route_key.trim();
            (!value.is_empty()).then(|| value.to_string())
        })
        .or_else(|| {
            self.conversation_store
                .load_session(&session_id_for_request(request))
                .ok()
                .flatten()
                .map(|session| session.route_key.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_default();
        let legacy_destination = first_string(
            &[&request.args],
            &["/notification/destination", "/notification_channel"],
        );
        let platform_hint = notification_platform_from_value(
            payload
                .pointer("/notification_channel")
                .and_then(Value::as_str)
                .unwrap_or("im_bridge"),
        );
        let payload_format = notification_payload_format_from_value(
            payload
                .pointer("/notification_format")
                .and_then(Value::as_str)
                .unwrap_or("plain_text"),
        );
        let title = string_at_paths(payload, &["/notification_card/header/title/content"])
            .unwrap_or_else(|| format!("{} AI 分析", target.display_name));
        let body = string_at_paths(payload, &["/summary", "/detection_summary"])
            .unwrap_or_else(|| format!("{} 分析完成", target.display_name));
        let requested_mode = first_string(
            &[&request.args],
            &["/notification/delivery/mode", "/notification/mode"],
        )
        .map(|value| notification_delivery_mode_from_value(&value))
        .unwrap_or(NotificationDeliveryMode::Send);
        let reply_to_message_id = first_string(
            &[&request.args],
            &[
                "/notification/delivery/reply_to_message_id",
                "/notification/reply_to_message_id",
            ],
        )
        .or_else(|| {
            let message_id = task_message_id(request);
            (!message_id.is_empty()).then_some(message_id)
        })
        .unwrap_or_default();
        let update_message_id = first_string(
            &[&request.args],
            &[
                "/notification/delivery/update_message_id",
                "/notification/update_message_id",
            ],
        )
        .unwrap_or_default();
        let (delivery_mode, reply_to_message_id, update_message_id) = match requested_mode {
            NotificationDeliveryMode::Reply if !reply_to_message_id.is_empty() => (
                NotificationDeliveryMode::Reply,
                reply_to_message_id,
                String::new(),
            ),
            NotificationDeliveryMode::Update if !update_message_id.is_empty() => (
                NotificationDeliveryMode::Update,
                String::new(),
                update_message_id,
            ),
            _ => (NotificationDeliveryMode::Send, String::new(), String::new()),
        };
        let destination = if matches!(platform_hint.as_deref(), Some("local_ui")) {
            NotificationDestination {
                kind: NotificationDestinationKind::LocalUi,
                route_key: String::new(),
                id: legacy_destination
                    .clone()
                    .unwrap_or_else(|| request.source.conversation_id.clone()),
                platform: "local_ui".to_string(),
                recipient: None,
            }
        } else if !route_key.is_empty() {
            NotificationDestination {
                kind: NotificationDestinationKind::Conversation,
                route_key,
                id: String::new(),
                platform: String::new(),
                recipient: None,
            }
        } else {
            let state = self.admin_store.load_or_create_state().ok()?;
            proactive_notification_destination(request, &state)?
        };

        let mut notification_request = NotificationRequest {
            notification_id: String::new(),
            trace_id: request.trace_id.clone(),
            source: NotificationSource {
                service: "harborbeacon".to_string(),
                module: "task_api".to_string(),
                event_type: event_type.to_string(),
            },
            destination,
            content: NotificationContent {
                title,
                body,
                payload_format,
                structured_payload: payload
                    .pointer("/notification_card")
                    .cloned()
                    .unwrap_or(Value::Null),
                attachments: artifacts
                    .iter()
                    .filter_map(task_artifact_to_notification_attachment)
                    .collect(),
            },
            delivery: NotificationDelivery {
                mode: delivery_mode,
                reply_to_message_id,
                update_message_id,
                idempotency_key: String::new(),
            },
            metadata: NotificationMetadata {
                correlation_id: request.trace_id.clone(),
            },
        };
        let notification_hash = notification_request_hash(&notification_request);
        notification_request.notification_id = format!("notif_{}", &notification_hash[..24]);
        notification_request.delivery.idempotency_key =
            format!("idem_{}", &notification_hash[..24]);
        Some(notification_request)
    }

    fn deliver_notification_request(
        &self,
        notification_request: &NotificationRequest,
    ) -> NotificationDeliveryOutcome {
        let service = match NotificationDeliveryService::new() {
            Ok(service) => service,
            Err(error) => {
                return NotificationDeliveryOutcome {
                    event_type: "task.notification_failed",
                    severity: EventSeverity::Error,
                    payload: json!({
                        "status": "failed",
                        "error": error,
                    }),
                };
            }
        };

        notification_delivery_outcome(notification_request, service.deliver(notification_request))
    }

    fn serialize_event_record(&self, event: &EventRecord) -> Value {
        serde_json::to_value(event).unwrap_or(Value::Null)
    }

    fn persist_vision_media_assets(
        &self,
        request: &TaskRequest,
        target: &ResolvedCameraTarget,
        payload: &mut Value,
    ) -> Result<(), String> {
        let snapshot_image_path = string_at_paths(payload, &["/snapshot/image_path"]);
        let annotated_image_path = string_at_paths(payload, &["/snapshot/annotated_image_path"]);
        if snapshot_image_path.is_none() && annotated_image_path.is_none() {
            return Ok(());
        }

        let snapshot_mime_type =
            string_at_paths(payload, &["/snapshot/mime_type"]).unwrap_or_else(|| {
                snapshot_image_path
                    .as_deref()
                    .and_then(mime_type_from_path)
                    .unwrap_or_else(|| "image/jpeg".to_string())
            });
        let captured_at = u64_at_paths(payload, &["/snapshot/captured_at_epoch_ms"])
            .map(|value| value.to_string())
            .unwrap_or_else(current_timestamp_millis);
        let source_storage = payload
            .pointer("/snapshot/source_storage")
            .cloned()
            .unwrap_or(Value::Null);
        let snapshot_ingest_metadata = payload
            .pointer("/snapshot/ingest_metadata")
            .cloned()
            .unwrap_or(Value::Null);
        let snapshot_byte_size = u64_at_paths(payload, &["/snapshot/byte_size"]);
        let detection_summary = string_at_paths(payload, &["/detection_summary"]);
        let summary = string_at_paths(payload, &["/summary"]);
        let summary_source = string_at_paths(payload, &["/summary_source"]);

        let snapshot_media_asset_id = if let Some(path) = snapshot_image_path.as_deref() {
            let media_asset = build_vision_image_media_asset(
                request,
                target,
                path,
                snapshot_mime_type.as_str(),
                MediaAssetKind::Snapshot,
                None,
                "analysis_snapshot",
                &captured_at,
                snapshot_byte_size,
                source_storage.clone(),
                snapshot_ingest_metadata.clone(),
                detection_summary.as_deref(),
                summary.as_deref(),
                summary_source.as_deref(),
            );
            let asset_id = media_asset.asset_id.clone();
            self.conversation_store.save_media_asset(&media_asset)?;
            Some(asset_id)
        } else {
            None
        };

        let annotated_media_asset_id = if let Some(path) = annotated_image_path.as_deref() {
            let media_asset = build_vision_image_media_asset(
                request,
                target,
                path,
                snapshot_mime_type.as_str(),
                MediaAssetKind::Derived,
                snapshot_media_asset_id.clone(),
                "analysis_annotation",
                &captured_at,
                None,
                source_storage.clone(),
                snapshot_ingest_metadata.clone(),
                detection_summary.as_deref(),
                summary.as_deref(),
                summary_source.as_deref(),
            );
            let asset_id = media_asset.asset_id.clone();
            self.conversation_store.save_media_asset(&media_asset)?;
            Some(asset_id)
        } else {
            None
        };

        if let Some(snapshot_object) = payload
            .pointer_mut("/snapshot")
            .and_then(Value::as_object_mut)
        {
            if let Some(asset_id) = snapshot_media_asset_id {
                snapshot_object.insert("media_asset_id".to_string(), Value::String(asset_id));
            }
            if let Some(asset_id) = annotated_media_asset_id {
                snapshot_object.insert(
                    "annotated_media_asset_id".to_string(),
                    Value::String(asset_id),
                );
            }
        }

        Ok(())
    }

    fn begin_task_tracking(&self, request: &TaskRequest) -> TaskRuntimeTracking {
        let started_at = current_timestamp();
        let tracking = TaskRuntimeTracking {
            session_id: session_id_for_request(request),
            step_id: step_id_for_request(request),
            started_at: started_at.clone(),
        };
        let session = self.build_session_record(request, &tracking, None);
        let _ = self.conversation_store.save_session(&session);

        let mut task_run = self
            .conversation_store
            .load_task_run(&request.task_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| TaskRun {
                task_id: request.task_id.clone(),
                workspace_id: workspace_id_for_request(request),
                session_id: tracking.session_id.clone(),
                source_channel: request.source.channel.clone(),
                domain: request.intent.domain.clone(),
                action: request.intent.action.clone(),
                intent_text: request.intent.raw_text.clone(),
                entity_refs: request.entity_refs.clone(),
                args: request.args.clone(),
                autonomy_level: effective_autonomy_level_for_task_run(request),
                status: TaskRunStatus::Queued,
                risk_level: expected_risk_level(request),
                requires_approval: effective_requires_approval(request),
                started_at: Some(started_at.clone()),
                completed_at: None,
                metadata: Value::Null,
            });
        task_run.workspace_id = workspace_id_for_request(request);
        task_run.session_id = tracking.session_id.clone();
        task_run.source_channel = request.source.channel.clone();
        task_run.domain = request.intent.domain.clone();
        task_run.action = request.intent.action.clone();
        task_run.intent_text = request.intent.raw_text.clone();
        task_run.entity_refs = request.entity_refs.clone();
        task_run.args = request.args.clone();
        task_run.autonomy_level = effective_autonomy_level_for_task_run(request);
        task_run.status = TaskRunStatus::Running;
        task_run.risk_level = expected_risk_level(request);
        task_run.requires_approval = effective_requires_approval(request);
        if task_run.started_at.is_none() {
            task_run.started_at = Some(started_at.clone());
        }
        task_run.completed_at = None;
        task_run.metadata = build_task_run_metadata(request, &tracking.step_id);
        let _ = self.conversation_store.save_task_run(&task_run);

        let mut task_step = self
            .conversation_store
            .load_task_step(&tracking.step_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| TaskStepRun {
                step_id: tracking.step_id.clone(),
                task_id: request.task_id.clone(),
                trace_id: request.trace_id.clone(),
                route_key: request.source.route_key.clone(),
                domain: request.intent.domain.clone(),
                operation: request.intent.action.clone(),
                route: ExecutionRoute::Local,
                executor_used: "task_api_dispatch".to_string(),
                status: TaskStepRunStatus::Pending,
                input_payload: Value::Null,
                output_payload: Value::Null,
                error_code: None,
                error_message: None,
                audit_ref: None,
                started_at: Some(started_at.clone()),
                ended_at: None,
            });
        task_step.task_id = request.task_id.clone();
        task_step.trace_id = request.trace_id.clone();
        task_step.route_key = request.source.route_key.clone();
        task_step.domain = request.intent.domain.clone();
        task_step.operation = request.intent.action.clone();
        task_step.route = ExecutionRoute::Local;
        task_step.executor_used = "task_api_dispatch".to_string();
        task_step.status = TaskStepRunStatus::Executing;
        task_step.input_payload = build_step_input_payload(request);
        task_step.output_payload = Value::Null;
        task_step.error_code = None;
        task_step.error_message = None;
        task_step.audit_ref = None;
        if task_step.started_at.is_none() {
            task_step.started_at = Some(started_at);
        }
        task_step.ended_at = None;
        let _ = self.conversation_store.save_task_step(&task_step);

        tracking
    }

    fn finish_task_tracking(
        &self,
        request: &TaskRequest,
        response: &TaskResponse,
        tracking: &TaskRuntimeTracking,
    ) -> Result<(), String> {
        let finished_at = current_timestamp();
        let mut task_run = self
            .conversation_store
            .load_task_run(&request.task_id)?
            .unwrap_or_else(|| TaskRun {
                task_id: request.task_id.clone(),
                workspace_id: workspace_id_for_request(request),
                session_id: tracking.session_id.clone(),
                source_channel: request.source.channel.clone(),
                domain: request.intent.domain.clone(),
                action: request.intent.action.clone(),
                intent_text: request.intent.raw_text.clone(),
                entity_refs: request.entity_refs.clone(),
                args: request.args.clone(),
                autonomy_level: effective_autonomy_level_for_task_run(request),
                status: TaskRunStatus::Queued,
                risk_level: response.risk_level,
                requires_approval: effective_requires_approval(request),
                started_at: Some(tracking.started_at.clone()),
                completed_at: None,
                metadata: build_task_run_metadata(request, &tracking.step_id),
            });
        task_run.workspace_id = workspace_id_for_request(request);
        task_run.session_id = tracking.session_id.clone();
        task_run.source_channel = request.source.channel.clone();
        task_run.domain = request.intent.domain.clone();
        task_run.action = request.intent.action.clone();
        task_run.intent_text = request.intent.raw_text.clone();
        task_run.entity_refs = request.entity_refs.clone();
        task_run.args = request.args.clone();
        task_run.autonomy_level = effective_autonomy_level_for_task_run(request);
        task_run.status = task_run_status_from_response(response.status);
        task_run.risk_level = response.risk_level;
        task_run.requires_approval = effective_requires_approval(request);
        if task_run.started_at.is_none() {
            task_run.started_at = Some(tracking.started_at.clone());
        }
        task_run.completed_at = task_run_completed_at(response.status, &finished_at);
        task_run.metadata = build_task_run_metadata(request, &tracking.step_id);
        self.conversation_store.save_task_run(&task_run)?;

        let (step_domain, step_operation) = step_identity(request, response);
        let execution_route = execution_route_for_executor(&response.executor_used);
        let mut task_step = self
            .conversation_store
            .load_task_step(&tracking.step_id)?
            .unwrap_or_else(|| TaskStepRun {
                step_id: tracking.step_id.clone(),
                task_id: request.task_id.clone(),
                trace_id: request.trace_id.clone(),
                route_key: request.source.route_key.clone(),
                domain: step_domain.clone(),
                operation: step_operation.clone(),
                route: execution_route,
                executor_used: response.executor_used.clone(),
                status: TaskStepRunStatus::Pending,
                input_payload: build_step_input_payload(request),
                output_payload: Value::Null,
                error_code: None,
                error_message: None,
                audit_ref: Some(response.audit_ref.clone()),
                started_at: Some(tracking.started_at.clone()),
                ended_at: None,
            });
        task_step.task_id = request.task_id.clone();
        task_step.trace_id = request.trace_id.clone();
        task_step.route_key = request.source.route_key.clone();
        task_step.domain = step_domain;
        task_step.operation = step_operation;
        task_step.route = execution_route;
        task_step.executor_used = response.executor_used.clone();
        task_step.status = task_step_status_from_response(response.status);
        task_step.input_payload = build_step_input_payload(request);
        task_step.output_payload = build_step_output_payload(response);
        task_step.error_code = match response.status {
            TaskStatus::Failed => response_error_code(response)
                .or_else(|| Some(format!("{}_failed", response.executor_used))),
            _ => None,
        };
        task_step.error_message = match response.status {
            TaskStatus::Failed => Some(response.result.message.clone()),
            _ => None,
        };
        task_step.audit_ref = Some(response.audit_ref.clone());
        if task_step.started_at.is_none() {
            task_step.started_at = Some(tracking.started_at.clone());
        }
        task_step.ended_at = Some(finished_at.clone());
        self.conversation_store.save_task_step(&task_step)?;

        let artifact_records =
            build_artifact_records(request, &tracking.step_id, &response.result.artifacts);
        self.conversation_store.replace_artifacts_for_step(
            &request.task_id,
            Some(&tracking.step_id),
            &artifact_records,
        )?;
        let event_records =
            build_event_records(request, &tracking.step_id, &response.result.events);
        self.conversation_store.replace_events_for_step(
            &request.task_id,
            Some(&tracking.step_id),
            &event_records,
        )?;

        let session = self.build_session_record(request, tracking, response.resume_token.clone());
        self.conversation_store.save_session(&session)?;
        Ok(())
    }

    fn build_session_record(
        &self,
        request: &TaskRequest,
        tracking: &TaskRuntimeTracking,
        resume_token: Option<String>,
    ) -> ConversationSession {
        let mut session = self
            .conversation_store
            .load_session(&tracking.session_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| ConversationSession {
                session_id: tracking.session_id.clone(),
                workspace_id: workspace_id_for_request(request),
                channel: request.source.channel.clone(),
                surface: request.source.surface.clone(),
                conversation_id: request.source.conversation_id.clone(),
                user_id: request.source.user_id.clone(),
                route_key: request.source.route_key.clone(),
                last_message_id: task_message_id(request),
                chat_type: task_chat_type(request),
                state: Value::Null,
                resume_token: None,
                expires_at: None,
            });
        session.workspace_id = workspace_id_for_request(request);
        session.channel = request.source.channel.clone();
        session.surface = request.source.surface.clone();
        session.conversation_id = request.source.conversation_id.clone();
        session.user_id = request.source.user_id.clone();
        if !request.source.route_key.trim().is_empty() {
            session.route_key = request.source.route_key.clone();
        }
        let message_id = task_message_id(request);
        if !message_id.is_empty() {
            session.last_message_id = message_id;
        }
        let chat_type = task_chat_type(request);
        if !chat_type.is_empty() {
            session.chat_type = chat_type;
        }
        session.state = self
            .load_conversation(request)
            .and_then(|conversation| {
                session_state_value_from_conversation(&conversation, Some(&session)).ok()
            })
            .unwrap_or(Value::Null);
        session.resume_token = resume_token;
        session.expires_at = None;
        session
    }

    fn hub(&self) -> CameraHubService {
        CameraHubService::new(self.admin_store.clone())
    }

    fn load_conversation(&self, request: &TaskRequest) -> Option<TaskConversationState> {
        let session_id = session_id_for_request(request);
        let key = conversation_key(request).unwrap_or_else(|| session_id.clone());
        self.conversation_store
            .load_for_session(&session_id, Some(&key))
            .ok()
            .flatten()
    }

    fn load_or_create_conversation(&self, request: &TaskRequest) -> TaskConversationState {
        let session_id = session_id_for_request(request);
        let key = conversation_key(request).unwrap_or(session_id);
        self.load_conversation(request)
            .unwrap_or(TaskConversationState {
                key,
                ..Default::default()
            })
    }

    fn save_conversation(
        &self,
        request: &TaskRequest,
        conversation: &TaskConversationState,
    ) -> Result<(), String> {
        let session_id = session_id_for_request(request);
        let session = self
            .conversation_store
            .load_session(&session_id)?
            .unwrap_or_else(|| ConversationSession {
                session_id,
                workspace_id: workspace_id_for_request(request),
                channel: request.source.channel.clone(),
                surface: request.source.surface.clone(),
                conversation_id: request.source.conversation_id.clone(),
                user_id: request.source.user_id.clone(),
                route_key: request.source.route_key.clone(),
                last_message_id: task_message_id(request),
                chat_type: task_chat_type(request),
                state: Value::Null,
                resume_token: None,
                expires_at: None,
            });
        self.conversation_store
            .save_for_session(&session, conversation)
    }

    fn load_approval_context(
        &self,
        approval_id: &str,
    ) -> Result<(ApprovalTicket, TaskRun, Option<ConversationSession>), String> {
        let approval = self
            .conversation_store
            .load_approval(approval_id)?
            .ok_or_else(|| format!("approval not found: {approval_id}"))?;
        let task_run = self
            .conversation_store
            .load_task_run(&approval.task_id)?
            .ok_or_else(|| format!("task run not found for approval: {}", approval.task_id))?;
        let session = if task_run.session_id.trim().is_empty() {
            None
        } else {
            self.conversation_store.load_session(&task_run.session_id)?
        };
        Ok((approval, task_run, session))
    }

    fn load_approval_summary(
        &self,
        approval: &ApprovalTicket,
    ) -> Result<TaskApprovalSummary, String> {
        let task_run = self
            .conversation_store
            .load_task_run(&approval.task_id)?
            .ok_or_else(|| format!("task run not found for approval: {}", approval.task_id))?;
        let session = if task_run.session_id.trim().is_empty() {
            None
        } else {
            self.conversation_store.load_session(&task_run.session_id)?
        };
        Ok(build_approval_summary(
            approval,
            &task_run,
            session.as_ref(),
        ))
    }

    fn build_approval_resume_request(
        &self,
        approval: &ApprovalTicket,
        task_run: &TaskRun,
        session: Option<&ConversationSession>,
        approver_user_id: Option<String>,
    ) -> TaskRequest {
        let mut args = task_run.args.clone();
        inject_approval_args(
            &mut args,
            &approval.approval_id,
            approver_user_id.as_deref(),
        );
        let trace_id = task_run
            .metadata
            .pointer("/trace_id")
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| task_run.task_id.clone());
        let surface = session
            .map(|session| session.surface.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                task_run
                    .metadata
                    .pointer("/surface")
                    .and_then(Value::as_str)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_else(|| "task_api".to_string());

        TaskRequest {
            task_id: task_run.task_id.clone(),
            trace_id,
            step_id: approval_resume_step_id(&approval.approval_id),
            source: TaskSource {
                channel: task_run.source_channel.clone(),
                surface,
                conversation_id: session
                    .map(|session| session.conversation_id.clone())
                    .unwrap_or_default(),
                user_id: session
                    .map(|session| session.user_id.clone())
                    .unwrap_or_else(|| approval.requester_user_id.clone()),
                session_id: task_run.session_id.clone(),
                route_key: source_route_key_from_context(task_run, session),
            },
            intent: TaskIntent {
                domain: task_run.domain.clone(),
                action: task_run.action.clone(),
                raw_text: task_run.intent_text.clone(),
            },
            entity_refs: task_run.entity_refs.clone(),
            args,
            autonomy: TaskAutonomy {
                level: normalize_task_autonomy_level(&task_run.autonomy_level),
            },
            message: None,
        }
    }

    fn record_approval_decision_event(
        &self,
        approval: &ApprovalTicket,
        task_run: &TaskRun,
        session: Option<&ConversationSession>,
        event_type: &str,
        severity: EventSeverity,
        approver_user_id: Option<String>,
    ) -> Result<(), String> {
        let request = TaskRequest {
            task_id: task_run.task_id.clone(),
            trace_id: task_run
                .metadata
                .pointer("/trace_id")
                .and_then(Value::as_str)
                .unwrap_or(task_run.task_id.as_str())
                .to_string(),
            step_id: approval_event_step_id(&approval.approval_id),
            source: TaskSource {
                channel: task_run.source_channel.clone(),
                surface: session
                    .map(|session| session.surface.clone())
                    .unwrap_or_else(|| {
                        task_run
                            .metadata
                            .pointer("/surface")
                            .and_then(Value::as_str)
                            .unwrap_or("task_api")
                            .to_string()
                    }),
                conversation_id: session
                    .map(|session| session.conversation_id.clone())
                    .unwrap_or_default(),
                user_id: session
                    .map(|session| session.user_id.clone())
                    .unwrap_or_else(|| approval.requester_user_id.clone()),
                session_id: task_run.session_id.clone(),
                route_key: source_route_key_from_context(task_run, session),
            },
            intent: TaskIntent {
                domain: task_run.domain.clone(),
                action: task_run.action.clone(),
                raw_text: task_run.intent_text.clone(),
            },
            entity_refs: task_run.entity_refs.clone(),
            args: task_run.args.clone(),
            autonomy: TaskAutonomy {
                level: normalize_task_autonomy_level(&task_run.autonomy_level),
            },
            message: None,
        };
        let step_id = approval_event_step_id(&approval.approval_id);
        let event = build_task_event_record(
            &request,
            &step_id,
            event_type,
            severity,
            json!({
                "approval_ticket": approval,
                "approver_user_id": approver_user_id,
                "policy_ref": approval.policy_ref.clone(),
            }),
        );
        self.conversation_store
            .replace_events_for_step(&task_run.task_id, Some(&step_id), &[event])
    }
}

fn pending_candidates_from_results(results: &[HubScanResultItem]) -> Vec<PendingTaskCandidate> {
    results
        .iter()
        .filter(|item| !item.reachable)
        .map(|item| PendingTaskCandidate {
            candidate_id: item.candidate_id.clone(),
            name: item.name.clone(),
            ip: item.ip.clone(),
            room: (!item.room.trim().is_empty()).then(|| item.room.clone()),
            port: item.port,
            rtsp_paths: item.rtsp_paths.clone(),
            requires_auth: item.requires_auth,
            vendor: item.vendor.clone(),
            model: item.model.clone(),
        })
        .collect()
}

fn candidate_to_connect_request(
    candidate: &PendingTaskCandidate,
    password: Option<String>,
) -> CameraConnectRequest {
    CameraConnectRequest {
        name: candidate.name.clone(),
        room: candidate.room.clone(),
        ip: candidate.ip.clone(),
        path_candidates: candidate.rtsp_paths.clone(),
        username: None,
        password,
        port: Some(candidate.port),
        snapshot_url: None,
        discovery_source: "task_api_candidate_confirm".to_string(),
        vendor: candidate.vendor.clone(),
        model: candidate.model.clone(),
    }
}

fn pending_connect_to_request(
    pending: &PendingTaskConnect,
    password: Option<String>,
) -> CameraConnectRequest {
    CameraConnectRequest {
        name: pending.name.clone(),
        room: pending.room.clone(),
        ip: pending.ip.clone(),
        path_candidates: pending.rtsp_paths.clone(),
        username: None,
        password,
        port: Some(pending.port),
        snapshot_url: pending.snapshot_url.clone(),
        discovery_source: "task_api_password_retry".to_string(),
        vendor: pending.vendor.clone(),
        model: pending.model.clone(),
    }
}

fn format_scan_message(
    cidr: &str,
    results: &[HubScanResultItem],
    pending_candidates: &[PendingTaskCandidate],
    device_count: usize,
) -> String {
    let connected = results.iter().filter(|item| item.reachable).count();
    if results.is_empty() {
        return format!(
            "已按后台默认策略扫描 {}，但当前没有发现可确认的摄像头候选设备。你也可以直接发送：添加摄像头 192.168.x.x",
            cidr
        );
    }
    if pending_candidates.is_empty() {
        if connected == 0 {
            return format!(
                "已按后台默认策略扫描 {}，共发现 {} 个候选设备，但都还不能直接接入。你也可以直接发送：添加摄像头 192.168.x.x",
                cidr,
                results.len()
            );
        }
        return format!(
            "已按后台默认策略扫描 {}，成功接入 {} 台摄像头，设备库现在共有 {} 台。接下来可以直接说：分析客厅摄像头",
            cidr,
            connected,
            device_count
        );
    }
    format!(
        "已按后台默认策略扫描 {}，共发现 {} 台候选设备，已自动接入 {} 台，还剩 {} 台待你确认：\n{}\n请直接回复：接入 1。如果提示需要密码，再回复：密码 xxxxxx。",
        cidr,
        results.len(),
        connected,
        pending_candidates.len(),
        format_pending_candidates(pending_candidates)
    )
}

fn format_pending_candidates(candidates: &[PendingTaskCandidate]) -> String {
    candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            format!(
                "{}. {}（{}，{}）",
                index + 1,
                candidate.name,
                candidate.ip,
                if candidate.requires_auth {
                    "需要密码"
                } else {
                    "待确认"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_vision_artifacts(payload: &Value) -> Vec<TaskArtifact> {
    let mut artifacts = Vec::new();
    let snapshot_mime_type = string_at_paths(payload, &["/snapshot/mime_type"])
        .unwrap_or_else(|| "image/jpeg".to_string());
    let snapshot_media_asset_id =
        string_at_paths(payload, &["/snapshot/media_asset_id", "/snapshot/asset_id"]);
    let annotated_media_asset_id =
        string_at_paths(payload, &["/snapshot/annotated_media_asset_id"]);
    if let Some(path) = string_at_paths(payload, &["/snapshot/image_path"]) {
        artifacts.push(TaskArtifact {
            kind: "image".to_string(),
            label: "抓拍图片".to_string(),
            mime_type: snapshot_mime_type.clone(),
            media_asset_id: snapshot_media_asset_id.clone(),
            path: Some(path),
            url: None,
            metadata: json!({
                "media_asset_id": snapshot_media_asset_id,
                "artifact_role": "analysis_snapshot",
            }),
        });
    }
    if let Some(path) = string_at_paths(payload, &["/snapshot/annotated_image_path"]) {
        artifacts.push(TaskArtifact {
            kind: "image".to_string(),
            label: "标注图片".to_string(),
            mime_type: snapshot_mime_type,
            media_asset_id: annotated_media_asset_id.clone(),
            path: Some(path),
            url: None,
            metadata: json!({
                "media_asset_id": annotated_media_asset_id,
                "artifact_role": "analysis_annotation",
            }),
        });
    }
    artifacts
}

fn build_snapshot_payload(
    target: &ResolvedCameraTarget,
    snapshot: &SnapshotCaptureResult,
    media_asset: &MediaAsset,
) -> Value {
    json!({
        "camera_target": target,
        "snapshot": {
            "media_asset_id": media_asset.asset_id.clone(),
            "mime_type": snapshot.mime_type.clone(),
            "byte_size": snapshot.byte_size,
            "captured_at_epoch_ms": snapshot.captured_at_epoch_ms,
            "storage": snapshot.storage.clone(),
        }
    })
}

fn build_snapshot_artifact(
    snapshot: &SnapshotCaptureResult,
    media_asset: &MediaAsset,
) -> TaskArtifact {
    TaskArtifact {
        kind: "image".to_string(),
        label: "抓拍图片".to_string(),
        mime_type: snapshot.mime_type.clone(),
        media_asset_id: Some(media_asset.asset_id.clone()),
        path: Some(snapshot.storage.relative_path.clone()),
        url: None,
        metadata: json!({
            "media_asset_id": media_asset.asset_id.clone(),
            "storage_target": snapshot.storage.target,
            "captured_at_epoch_ms": snapshot.captured_at_epoch_ms,
            "byte_size": snapshot.byte_size,
        }),
    }
}

fn build_snapshot_media_asset(
    request: &TaskRequest,
    target: &ResolvedCameraTarget,
    snapshot: &SnapshotCaptureResult,
) -> MediaAsset {
    MediaAsset {
        asset_id: new_media_asset_id(),
        workspace_id: workspace_id_for_request(request),
        device_id: Some(target.device_id.clone()),
        asset_kind: MediaAssetKind::Snapshot,
        storage_target: storage_target_kind_from_snapshot(snapshot.storage.target),
        storage_uri: snapshot.storage.relative_path.clone(),
        mime_type: snapshot.mime_type.clone(),
        byte_size: snapshot.byte_size as u64,
        checksum: snapshot_checksum(snapshot),
        captured_at: Some(snapshot.captured_at_epoch_ms.to_string()),
        started_at: None,
        ended_at: None,
        derived_from_asset_id: None,
        tags: vec!["snapshot".to_string(), "camera".to_string()],
        metadata: json!({
            "task_id": request.task_id.clone(),
            "step_id": step_id_for_request(request),
            "trace_id": request.trace_id.clone(),
            "source_channel": request.source.channel.clone(),
            "source_surface": request.source.surface.clone(),
            "camera_display_name": target.display_name.clone(),
            "room_name": target.room_name.clone(),
            "storage_relative_path": snapshot.storage.relative_path.clone(),
            "device_ingest_metadata": snapshot.ingest_metadata.clone(),
        }),
    }
}

fn build_clip_media_asset(
    request: &TaskRequest,
    target: &ResolvedCameraTarget,
    clip: &ClipCaptureResult,
) -> MediaAsset {
    MediaAsset {
        asset_id: new_media_asset_id(),
        workspace_id: workspace_id_for_request(request),
        device_id: Some(target.device_id.clone()),
        asset_kind: MediaAssetKind::Clip,
        storage_target: storage_target_kind_from_snapshot(clip.storage.target),
        storage_uri: clip.storage.relative_path.clone(),
        mime_type: clip.mime_type.clone(),
        byte_size: clip.byte_size as u64,
        checksum: file_checksum(&clip.storage.relative_path),
        captured_at: Some(clip.captured_at_epoch_ms.to_string()),
        started_at: Some(clip.started_at_epoch_ms.to_string()),
        ended_at: Some(clip.ended_at_epoch_ms.to_string()),
        derived_from_asset_id: None,
        tags: vec!["clip".to_string(), "camera".to_string()],
        metadata: json!({
            "task_id": request.task_id.clone(),
            "step_id": step_id_for_request(request),
            "trace_id": request.trace_id.clone(),
            "source_channel": request.source.channel.clone(),
            "source_surface": request.source.surface.clone(),
            "camera_display_name": target.display_name.clone(),
            "room_name": target.room_name.clone(),
            "storage_relative_path": clip.storage.relative_path.clone(),
            "clip_length_seconds": clip.clip_length_seconds,
            "keyframe_count": clip.keyframe_count,
            "keyframe_interval_seconds": clip.keyframe_interval_seconds,
            "device_ingest_metadata": clip.ingest_metadata.clone(),
        }),
    }
}

fn build_clip_payload(
    target: &ResolvedCameraTarget,
    clip: &ClipCaptureResult,
    keyframes: &[PathBuf],
    media_asset: &MediaAsset,
) -> Value {
    json!({
        "camera_target": target,
        "clip": {
            "media_asset_id": media_asset.asset_id.clone(),
            "mime_type": clip.mime_type.clone(),
            "byte_size": clip.byte_size,
            "captured_at_epoch_ms": clip.captured_at_epoch_ms,
            "started_at_epoch_ms": clip.started_at_epoch_ms,
            "ended_at_epoch_ms": clip.ended_at_epoch_ms,
            "clip_length_seconds": clip.clip_length_seconds,
            "storage": clip.storage.clone(),
            "keyframe_count": keyframes.len(),
            "keyframes": keyframes
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>(),
        }
    })
}

fn build_clip_artifacts(
    clip: &ClipCaptureResult,
    keyframes: &[PathBuf],
    media_asset: &MediaAsset,
) -> Vec<TaskArtifact> {
    let mut artifacts = vec![TaskArtifact {
        kind: "video".to_string(),
        label: "短视频".to_string(),
        mime_type: clip.mime_type.clone(),
        media_asset_id: Some(media_asset.asset_id.clone()),
        path: Some(clip.storage.relative_path.clone()),
        url: None,
        metadata: json!({
            "media_asset_id": media_asset.asset_id.clone(),
            "storage_target": clip.storage.target,
            "captured_at_epoch_ms": clip.captured_at_epoch_ms,
            "byte_size": clip.byte_size,
            "keyframe_count": keyframes.len(),
        }),
    }];
    artifacts.extend(keyframes.iter().take(3).map(|path| TaskArtifact {
        kind: "image".to_string(),
        label: "视频关键帧".to_string(),
        mime_type: "image/jpeg".to_string(),
        media_asset_id: None,
        path: Some(path.to_string_lossy().to_string()),
        url: None,
        metadata: json!({
            "artifact_role": "video_keyframe",
            "source_video_path": clip.storage.relative_path.clone(),
        }),
    }));
    artifacts
}

fn resolved_recording_policy(
    state: &AdminConsoleState,
    target: Option<&ResolvedCameraTarget>,
) -> Option<RecordingPolicy> {
    state
        .platform
        .recording_policies
        .iter()
        .find(|policy| {
            target
                .and_then(|target| {
                    policy
                        .device_id
                        .as_deref()
                        .map(|device_id| device_id == target.device_id.as_str())
                })
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| {
            state
                .platform
                .recording_policies
                .first()
                .cloned()
        })
}

fn resolved_capture_directory(
    state: &AdminConsoleState,
    recording_policy: Option<&RecordingPolicy>,
) -> Result<PathBuf, String> {
    let root = PathBuf::from(harboros_writable_root());
    ensure_safe_capture_root(&root)?;
    let subdirectory = recording_policy
        .and_then(RecordingPolicy::capture_subdirectory)
        .unwrap_or(state.defaults.capture_subdirectory.as_str());
    let subdirectory = sanitize_relative_subdirectory(subdirectory)
        .ok_or_else(|| "capture 子目录不合法，必须是 writable root 下的相对路径。".to_string())?;
    let capture_root = root.join(subdirectory);
    fs::create_dir_all(&capture_root).map_err(|error| {
        format!(
            "failed to create capture directory {}: {error}",
            capture_root.display()
        )
    })?;
    Ok(capture_root)
}

fn ensure_safe_capture_root(root: &Path) -> Result<(), String> {
    let normalized = root.to_string_lossy().replace('\\', "/");
    if normalized.starts_with("/mnt/software/harborbeacon-agent-ci") {
        Ok(())
    } else if std::env::var(ALLOW_NON_HARBOROS_CAPTURE_ROOT_ENV)
        .ok()
        .is_some_and(|value| env_flag_enabled(&value))
        && root.is_absolute()
        && normalized.ends_with("/harborbeacon-agent-ci")
    {
        Ok(())
    } else {
        Err(format!(
            "capture writable root {} is outside the approved HarborOS root",
            root.display()
        ))
    }
}

fn sanitize_relative_subdirectory(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return None;
    }
    let mut sanitized = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::Normal(segment) => sanitized.push(segment),
            _ => return None,
        }
    }
    (!sanitized.as_os_str().is_empty()).then_some(sanitized)
}

fn build_snapshot_output_path(
    capture_root: &Path,
    target: &ResolvedCameraTarget,
    captured_at_epoch_ms: u128,
    extension: &str,
) -> PathBuf {
    capture_root.join(format!(
        "{}-{}.{}",
        sanitize_path_segment(&target.device_id),
        captured_at_epoch_ms,
        extension
    ))
}

fn build_clip_output_path(
    capture_root: &Path,
    target: &ResolvedCameraTarget,
    captured_at_epoch_ms: u128,
) -> PathBuf {
    capture_root.join(format!(
        "{}-{}.mp4",
        sanitize_path_segment(&target.device_id),
        captured_at_epoch_ms
    ))
}

fn build_keyframe_directory(capture_root: &Path, clip_path: &Path) -> PathBuf {
    let stem = clip_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("clip");
    capture_root.join("keyframes").join(stem)
}

fn current_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn write_media_index_sidecar(
    sidecar_path: &Path,
    media_path: &str,
    source_video_path: Option<&str>,
    target: &ResolvedCameraTarget,
    ocr_text: &str,
    vlm_summary: &str,
    tags: &[String],
) -> Result<(), String> {
    if let Some(parent) = sidecar_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create sidecar directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let searchable = [ocr_text.trim(), vlm_summary.trim()]
        .iter()
        .filter(|value| !value.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let content = serde_json::to_string_pretty(&json!({
        "caption": vlm_summary.trim(),
        "derived_text": searchable,
        "ocr_text": ocr_text.trim(),
        "source_video_path": source_video_path,
        "camera": {
            "device_id": target.device_id,
            "display_name": target.display_name,
            "room_name": target.room_name,
            "vendor": target.vendor,
            "model": target.model,
        },
        "source_path": media_path,
        "labels": tags,
    }))
    .map_err(|error| format!("failed to serialize media sidecar: {error}"))?;
    fs::write(sidecar_path, content).map_err(|error| {
        format!(
            "failed to write media sidecar {}: {error}",
            sidecar_path.display()
        )
    })
}

fn build_vision_image_media_asset(
    request: &TaskRequest,
    target: &ResolvedCameraTarget,
    image_path: &str,
    mime_type: &str,
    asset_kind: MediaAssetKind,
    derived_from_asset_id: Option<String>,
    artifact_role: &str,
    captured_at: &str,
    byte_size_override: Option<u64>,
    source_storage: Value,
    ingest_metadata: Value,
    detection_summary: Option<&str>,
    summary: Option<&str>,
    summary_source: Option<&str>,
) -> MediaAsset {
    let tags = if asset_kind == MediaAssetKind::Derived {
        vec![
            "derived".to_string(),
            "annotated".to_string(),
            "camera".to_string(),
            "vision_analysis".to_string(),
        ]
    } else {
        vec![
            "snapshot".to_string(),
            "camera".to_string(),
            "vision_analysis".to_string(),
        ]
    };

    MediaAsset {
        asset_id: new_media_asset_id(),
        workspace_id: workspace_id_for_request(request),
        device_id: Some(target.device_id.clone()),
        asset_kind,
        storage_target: StorageTargetKind::LocalDisk,
        storage_uri: image_path.to_string(),
        mime_type: mime_type.to_string(),
        byte_size: byte_size_override.unwrap_or_else(|| file_byte_size(image_path)),
        checksum: file_checksum(image_path),
        captured_at: Some(captured_at.to_string()),
        started_at: None,
        ended_at: None,
        derived_from_asset_id,
        tags,
        metadata: json!({
            "task_id": request.task_id.clone(),
            "step_id": step_id_for_request(request),
            "trace_id": request.trace_id.clone(),
            "source_channel": request.source.channel.clone(),
            "source_surface": request.source.surface.clone(),
            "camera_display_name": target.display_name.clone(),
            "room_name": target.room_name.clone(),
            "artifact_role": artifact_role,
            "detection_summary": detection_summary,
            "summary": summary,
            "summary_source": summary_source,
            "storage_path": image_path,
            "source_storage": source_storage,
            "ingest_metadata": ingest_metadata,
        }),
    }
}

fn storage_target_kind_from_snapshot(target: StorageTarget) -> StorageTargetKind {
    match target {
        StorageTarget::LocalDisk => StorageTargetKind::LocalDisk,
        StorageTarget::HarborOsPool => StorageTargetKind::HarborOsPool,
        StorageTarget::ExternalShare => StorageTargetKind::Nas,
    }
}

fn snapshot_checksum(snapshot: &SnapshotCaptureResult) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(snapshot.bytes_base64.as_bytes())
        .ok()?;
    let digest = Sha256::digest(&bytes);
    Some(format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn file_byte_size(path: &str) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn file_checksum(path: &str) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let digest = Sha256::digest(&bytes);
    Some(format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn mime_type_from_path(path: &str) -> Option<String> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg".to_string()),
        "png" => Some("image/png".to_string()),
        "webp" => Some("image/webp".to_string()),
        "mp4" => Some("video/mp4".to_string()),
        _ => None,
    }
}

fn build_share_link_payload(
    target: &ResolvedCameraTarget,
    issued: &remote_view::IssuedCameraShareToken,
    media_session: &MediaSession,
    share_link: &ShareLink,
) -> Value {
    let encoded_token = url_encode_path_segment(&issued.token);
    let relative_url = format!("/shared/cameras/{encoded_token}");
    let stream_url = format!("{relative_url}/live.mjpeg");
    json!({
        "share_link_id": share_link.share_link_id,
        "media_session_id": media_session.media_session_id,
        "device_id": target.device_id,
        "display_name": target.display_name,
        "url": relative_url,
        "stream_url": stream_url,
        "access_scope": share_link.access_scope,
        "expires_at_unix_secs": issued.expires_at_unix_secs,
        "ttl_minutes": issued.ttl_minutes,
    })
}

fn build_share_link_artifact(share_link: &Value) -> TaskArtifact {
    TaskArtifact {
        kind: "link".to_string(),
        label: "共享观看链接".to_string(),
        mime_type: "text/uri-list".to_string(),
        media_asset_id: None,
        path: None,
        url: share_link
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string),
        metadata: json!({
            "share_link_id": share_link.get("share_link_id").cloned().unwrap_or(Value::Null),
            "media_session_id": share_link
                .get("media_session_id")
                .cloned()
                .unwrap_or(Value::Null),
            "access_scope": share_link.get("access_scope").cloned().unwrap_or(Value::Null),
            "stream_url": share_link.get("stream_url").cloned().unwrap_or(Value::Null),
            "expires_at_unix_secs": share_link
                .get("expires_at_unix_secs")
                .cloned()
                .unwrap_or(Value::Null),
            "ttl_minutes": share_link.get("ttl_minutes").cloned().unwrap_or(Value::Null),
        }),
    }
}

fn build_knowledge_search_artifacts(response: &KnowledgeSearchResponse) -> Vec<TaskArtifact> {
    response
        .documents
        .iter()
        .chain(response.images.iter())
        .take(6)
        .map(|hit| {
            let proxied_video_path = resolved_video_proxy_path(hit);
            let is_video_proxy = proxied_video_path.is_some();
            let path = proxied_video_path.unwrap_or_else(|| hit.path.clone());
            TaskArtifact {
                kind: if is_video_proxy {
                    "video".to_string()
                } else if hit.modality.as_str() == "image" {
                    "image".to_string()
                } else {
                    "text".to_string()
                },
                label: hit.title.clone(),
                mime_type: mime_type_from_path(&path).unwrap_or_else(|| {
                    if is_video_proxy {
                        "video/mp4".to_string()
                    } else if hit.modality.as_str() == "image" {
                        "image/*".to_string()
                    } else {
                        "text/plain".to_string()
                    }
                }),
                media_asset_id: None,
                path: Some(path),
                url: None,
                metadata: json!({
                    "modality": if is_video_proxy { "video" } else { hit.modality.as_str() },
                    "score": hit.score,
                    "source_image_path": if is_video_proxy { Some(hit.path.clone()) } else { None::<String> },
                    "citation": {
                        "title": hit.title.clone(),
                        "path": hit.path.clone(),
                        "modality": hit.modality.clone(),
                        "chunk_id": hit.chunk_id.clone(),
                        "line_start": hit.line_start,
                        "line_end": hit.line_end,
                        "matched_terms": hit.matched_terms.clone(),
                        "preview": hit.snippet.clone(),
                        "score": hit.score,
                        "source_path": hit.source_path.clone(),
                    },
                }),
            }
        })
        .collect()
}

fn resolved_video_proxy_path(hit: &crate::runtime::knowledge::KnowledgeSearchHit) -> Option<String> {
    let sidecar_path = hit
        .source_path
        .as_deref()
        .and_then(|path| {
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .filter(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "json" | "yaml" | "yml" | "txt" | "md" | "markdown" | "csv"
                    )
                })
                .map(|_| PathBuf::from(path))
        })
        .or_else(|| {
            let candidate = Path::new(&hit.path).with_extension("json");
            candidate.exists().then_some(candidate)
        })?;
    let value = fs::read_to_string(sidecar_path).ok()?;
    let json = serde_json::from_str::<Value>(&value).ok()?;
    json.get("source_video_path")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn format_knowledge_search_message(response: &KnowledgeSearchResponse) -> String {
    response.reply_pack.summary.clone()
}

fn knowledge_search_next_actions(response: &KnowledgeSearchResponse) -> Vec<String> {
    let mut actions = Vec::new();
    if !response.documents.is_empty() {
        actions.push("只看文档结果".to_string());
    }
    if !response.images.is_empty() {
        actions.push("只看图片结果".to_string());
    }
    if actions.is_empty() {
        actions.push("换个关键词再搜".to_string());
    }
    actions
}

fn build_share_media_session(
    request: &TaskRequest,
    target: &ResolvedCameraTarget,
    media_session_id: &str,
    share_link_id: &str,
) -> MediaSession {
    MediaSession {
        media_session_id: media_session_id.to_string(),
        device_id: target.device_id.clone(),
        stream_profile_id: format!("{}::stream::primary", target.device_id),
        session_kind: MediaSessionKind::Share,
        delivery_mode: share_delivery_mode(target),
        opened_by_user_id: (!request.source.user_id.trim().is_empty())
            .then(|| request.source.user_id.clone()),
        status: MediaSessionStatus::Active,
        share_link_id: Some(share_link_id.to_string()),
        started_at: Some(current_timestamp()),
        ended_at: None,
        metadata: json!({
            "task_id": request.task_id.clone(),
            "step_id": step_id_for_request(request),
            "source_channel": request.source.channel.clone(),
            "source_surface": request.source.surface.clone(),
            "conversation_id": request.source.conversation_id.clone(),
            "delivery_proxy": "mjpeg",
            "stream_transport": serde_json::to_value(target.primary_stream.transport).unwrap_or(Value::Null),
        }),
    }
}

fn build_share_link_record(
    issued: &remote_view::IssuedCameraShareToken,
    media_session_id: &str,
    share_link_id: &str,
) -> ShareLink {
    ShareLink {
        share_link_id: share_link_id.to_string(),
        media_session_id: media_session_id.to_string(),
        token_hash: remote_view::camera_share_token_hash(&issued.token),
        access_scope: ShareAccessScope::PublicLink,
        expires_at: Some(issued.expires_at_unix_secs.to_string()),
        revoked_at: None,
    }
}

fn share_delivery_mode(target: &ResolvedCameraTarget) -> MediaDeliveryMode {
    match target.primary_stream.transport {
        crate::runtime::registry::StreamTransport::Webrtc => MediaDeliveryMode::Webrtc,
        crate::runtime::registry::StreamTransport::Hls => MediaDeliveryMode::Hls,
        crate::runtime::registry::StreamTransport::Rtsp
        | crate::runtime::registry::StreamTransport::Unknown => MediaDeliveryMode::Hls,
    }
}

fn build_task_run_metadata(request: &TaskRequest, step_id: &str) -> Value {
    json!({
        "trace_id": request.trace_id.clone(),
        "step_id": step_id,
        "surface": request.source.surface.clone(),
        "conversation_id": request.source.conversation_id.clone(),
        "route_key": request.source.route_key.clone(),
        "message_id": request
            .message
            .as_ref()
            .map(|message| message.message_id.clone())
            .unwrap_or_default(),
        "chat_type": request
            .message
            .as_ref()
            .map(|message| message.chat_type.clone())
            .unwrap_or_default(),
        "attachments": task_attachment_transport_contract(request),
        "request_identity": task_request_identity(request),
    })
}

fn build_approval_summary(
    approval: &ApprovalTicket,
    task_run: &TaskRun,
    session: Option<&ConversationSession>,
) -> TaskApprovalSummary {
    TaskApprovalSummary {
        approval_ticket: approval.clone(),
        source_channel: task_run.source_channel.clone(),
        surface: session
            .map(|session| session.surface.clone())
            .unwrap_or_else(|| {
                task_run
                    .metadata
                    .pointer("/surface")
                    .and_then(Value::as_str)
                    .unwrap_or("task_api")
                    .to_string()
            }),
        conversation_id: session
            .map(|session| session.conversation_id.clone())
            .unwrap_or_default(),
        user_id: session
            .map(|session| session.user_id.clone())
            .unwrap_or_else(|| approval.requester_user_id.clone()),
        session_id: task_run.session_id.clone(),
        domain: task_run.domain.clone(),
        action: task_run.action.clone(),
        intent_text: task_run.intent_text.clone(),
        autonomy_level: normalize_task_autonomy_level(&task_run.autonomy_level),
        risk_level: task_run.risk_level,
    }
}

fn inject_approval_args(args: &mut Value, approval_id: &str, approver_user_id: Option<&str>) {
    if !args.is_object() {
        *args = json!({});
    }
    if let Some(object) = args.as_object_mut() {
        let approval_entry = object
            .entry("approval".to_string())
            .or_insert_with(|| json!({}));
        if !approval_entry.is_object() {
            *approval_entry = json!({});
        }
        if let Some(approval_object) = approval_entry.as_object_mut() {
            approval_object.insert("token".to_string(), Value::String(approval_id.to_string()));
            if let Some(approver_user_id) = approver_user_id {
                approval_object.insert(
                    "approver_id".to_string(),
                    Value::String(approver_user_id.to_string()),
                );
            }
        }
    }
}

fn approval_resume_step_id(approval_id: &str) -> String {
    format!("approval:{approval_id}:resume")
}

fn approval_event_step_id(approval_id: &str) -> String {
    format!("approval:{approval_id}:event")
}

fn normalize_task_autonomy_level(level: &str) -> String {
    match level.trim().to_lowercase().as_str() {
        "" => default_task_autonomy_level(),
        "readonly" | "read_only" | "read-only" => "readonly".to_string(),
        "full" => "full".to_string(),
        _ => "supervised".to_string(),
    }
}

fn build_step_input_payload(request: &TaskRequest) -> Value {
    json!({
        "trace_id": request.trace_id.clone(),
        "source": request.source.clone(),
        "intent": request.intent.clone(),
        "entity_refs": request.entity_refs.clone(),
        "args": request.args.clone(),
        "message": request.message.clone(),
    })
}

fn build_step_output_payload(response: &TaskResponse) -> Value {
    json!({
        "message": response.result.message.clone(),
        "data": response.result.data.clone(),
        "events": response.result.events.clone(),
        "next_actions": response.result.next_actions.clone(),
        "missing_fields": response.missing_fields.clone(),
        "prompt": response.prompt.clone(),
        "resume_token": response.resume_token.clone(),
    })
}

fn is_supported_harbor_task(domain: &str, action: &str) -> bool {
    (domain == "service" && matches!(action, "status" | "start" | "stop" | "restart" | "enable"))
        || (domain == "files" && matches!(action, "list" | "copy" | "move"))
}

fn build_harbor_action_from_request(request: &TaskRequest) -> Result<Action, String> {
    let domain = request.intent.domain.trim().to_lowercase();
    let operation = request.intent.action.trim().to_lowercase();

    match (domain.as_str(), operation.as_str()) {
        ("service", _) => build_harbor_service_action(request, &operation),
        ("files", _) => build_harbor_files_action(request, &operation),
        _ => Err(format!(
            "unsupported HarborOS task action: {domain}.{operation}"
        )),
    }
}

fn build_harbor_service_action(request: &TaskRequest, operation: &str) -> Result<Action, String> {
    let service_name = first_string(
        &[&request.args, &request.entity_refs],
        &[
            "/service_name",
            "/resource/service_name",
            "/service/name",
            "/service/id_or_name",
            "/service",
            "/resource/id_or_name",
            "/resource/name",
            "/id_or_name",
            "/name",
        ],
    )
    .ok_or_else(|| "service action requires service_name or resource.id_or_name".to_string())?;

    let mut args = serde_json::Map::new();
    if operation == "enable" {
        args.insert(
            "enable".to_string(),
            json!(
                bool_at_paths(&request.args, &["/enable", "/resource/enable"])
                    .or_else(|| {
                        bool_at_paths(&request.entity_refs, &["/enable", "/resource/enable"])
                    })
                    .unwrap_or(true)
            ),
        );
    }

    Ok(apply_governance_defaults(Action {
        domain: "service".to_string(),
        operation: operation.to_string(),
        resource: json!({
            "service_name": service_name,
        }),
        args: Value::Object(args),
        risk_level: RiskLevel::Low,
        requires_approval: request_requires_approval(request),
        dry_run: request_preview_flag(request),
    }))
}

fn build_harbor_files_action(request: &TaskRequest, operation: &str) -> Result<Action, String> {
    let recursive = bool_at_paths(&request.args, &["/recursive", "/resource/recursive"])
        .or_else(|| bool_at_paths(&request.entity_refs, &["/recursive", "/resource/recursive"]))
        .unwrap_or(false);
    let overwrite = bool_at_paths(&request.args, &["/overwrite", "/resource/overwrite"])
        .or_else(|| bool_at_paths(&request.entity_refs, &["/overwrite", "/resource/overwrite"]))
        .unwrap_or(false);
    let max_bytes = u64_at_paths(&request.args, &["/max_bytes", "/resource/max_bytes"])
        .or_else(|| u64_at_paths(&request.entity_refs, &["/max_bytes", "/resource/max_bytes"]));

    let mut args = serde_json::Map::new();
    if recursive {
        args.insert("recursive".to_string(), json!(true));
    }
    if overwrite {
        args.insert("overwrite".to_string(), json!(true));
    }
    if let Some(max_bytes) = max_bytes {
        args.insert("max_bytes".to_string(), json!(max_bytes));
    }

    let resource = match operation {
        "list" => {
            let path = first_string(
                &[&request.args, &request.entity_refs],
                &[
                    "/path",
                    "/resource/path",
                    "/paths/0",
                    "/resource/paths/0",
                    "/source",
                    "/resource/source",
                    "/src",
                    "/resource/src",
                ],
            )
            .ok_or_else(|| "files.list requires path or resource.path".to_string())?;
            json!({
                "path": path.clone(),
                "paths": [path],
            })
        }
        "copy" | "move" => {
            let source = first_string(
                &[&request.args, &request.entity_refs],
                &[
                    "/source",
                    "/resource/source",
                    "/src",
                    "/resource/src",
                    "/paths/0",
                    "/resource/paths/0",
                    "/path",
                    "/resource/path",
                ],
            )
            .ok_or_else(|| "files action requires source or resource.paths[0]".to_string())?;
            let target = first_string(
                &[&request.args, &request.entity_refs],
                &[
                    "/target",
                    "/resource/target",
                    "/destination",
                    "/resource/destination",
                    "/dst",
                    "/resource/dst",
                    "/paths/1",
                    "/resource/paths/1",
                ],
            )
            .ok_or_else(|| "files action requires target or resource.destination".to_string())?;
            json!({
                "source": source.clone(),
                "target": target,
                "paths": [source],
            })
        }
        _ => return Err(format!("unsupported HarborOS files operation: {operation}")),
    };

    Ok(apply_governance_defaults(Action {
        domain: "files".to_string(),
        operation: operation.to_string(),
        resource,
        args: Value::Object(args),
        risk_level: RiskLevel::Low,
        requires_approval: request_requires_approval(request),
        dry_run: request_preview_flag(request),
    }))
}

fn request_preview_flag(request: &TaskRequest) -> bool {
    bool_at_paths(
        &request.args,
        &[
            "/dry_run",
            "/preview",
            "/resource/dry_run",
            "/resource/preview",
        ],
    )
    .or_else(|| {
        bool_at_paths(
            &request.entity_refs,
            &[
                "/dry_run",
                "/preview",
                "/resource/dry_run",
                "/resource/preview",
            ],
        )
    })
    .unwrap_or(false)
}

fn harbor_execution_is_preview(payload: &Value) -> bool {
    bool_at_paths(payload, &["/dry_run"]).unwrap_or(false)
        || matches!(payload.pointer("/passthrough"), Some(&Value::Bool(false)))
        || string_at_paths(payload, &["/note"])
            .map(|value| value.to_ascii_lowercase().contains("preview"))
            .unwrap_or(false)
}

fn non_empty_audit_ref(audit_ref: &str) -> String {
    let trimmed = audit_ref.trim();
    if trimmed.is_empty() {
        new_audit_ref()
    } else {
        trimmed.to_string()
    }
}

fn build_artifact_records(
    request: &TaskRequest,
    step_id: &str,
    artifacts: &[TaskArtifact],
) -> Vec<ArtifactRecord> {
    artifacts
        .iter()
        .enumerate()
        .map(|(index, artifact)| ArtifactRecord {
            artifact_id: format!("{}:{}:artifact-{}", request.task_id, step_id, index + 1),
            task_id: request.task_id.clone(),
            trace_id: request.trace_id.clone(),
            step_id: Some(step_id.to_string()),
            route_key: request.source.route_key.clone(),
            artifact_kind: artifact_kind_from_name(&artifact.kind),
            label: artifact.label.clone(),
            mime_type: artifact.mime_type.clone(),
            media_asset_id: artifact.media_asset_id.clone(),
            path: artifact.path.clone(),
            url: artifact.url.clone(),
            metadata: artifact.metadata.clone(),
        })
        .collect()
}

fn build_event_records(request: &TaskRequest, step_id: &str, events: &[Value]) -> Vec<EventRecord> {
    events
        .iter()
        .filter_map(|event| serde_json::from_value::<EventRecord>(event.clone()).ok())
        .map(|mut event| {
            if event.workspace_id.trim().is_empty() {
                event.workspace_id = workspace_id_for_request(request);
            }
            if event.source_id.trim().is_empty() {
                event.source_id = request.task_id.clone();
            }
            if event.correlation_id.is_none() && !request.trace_id.trim().is_empty() {
                event.correlation_id = Some(request.trace_id.clone());
            }
            if event.causation_id.is_none() {
                event.causation_id = Some(step_id.to_string());
            }
            if event.occurred_at.is_none() {
                event.occurred_at = Some(current_timestamp());
            }
            if event.ingested_at.is_none() {
                event.ingested_at = event.occurred_at.clone();
            }
            event
        })
        .collect()
}

fn build_task_event_record(
    request: &TaskRequest,
    step_id: &str,
    event_type: &str,
    severity: EventSeverity,
    payload: Value,
) -> EventRecord {
    let occurred_at = current_timestamp();
    EventRecord {
        event_id: new_event_id(),
        workspace_id: workspace_id_for_request(request),
        source_kind: EventSourceKind::Task,
        source_id: request.task_id.clone(),
        event_type: event_type.to_string(),
        severity,
        payload,
        correlation_id: (!request.trace_id.trim().is_empty()).then(|| request.trace_id.clone()),
        causation_id: Some(step_id.to_string()),
        occurred_at: Some(occurred_at.clone()),
        ingested_at: Some(occurred_at),
    }
}

fn artifact_kind_from_name(kind: &str) -> ArtifactKind {
    match kind.trim().to_lowercase().as_str() {
        "image" => ArtifactKind::Image,
        "video" => ArtifactKind::Video,
        "link" => ArtifactKind::Link,
        "card" => ArtifactKind::Card,
        "json" => ArtifactKind::Json,
        _ => ArtifactKind::Text,
    }
}

fn task_artifact_from_record(record: ArtifactRecord) -> TaskArtifact {
    TaskArtifact {
        kind: task_artifact_kind_name(record.artifact_kind).to_string(),
        label: record.label,
        mime_type: record.mime_type,
        media_asset_id: record.media_asset_id,
        path: record.path,
        url: record.url,
        metadata: record.metadata,
    }
}

fn task_artifact_kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Text => "text",
        ArtifactKind::Image => "image",
        ArtifactKind::Video => "video",
        ArtifactKind::Link => "link",
        ArtifactKind::Card => "card",
        ArtifactKind::Json => "json",
    }
}

fn task_request_identity(request: &TaskRequest) -> Value {
    json!({
        "route_key": request.source.route_key.trim(),
        "conversation_id": request.source.conversation_id.trim(),
        "message_id": task_message_id(request),
        "intent": {
            "domain": request.intent.domain.trim(),
            "action": request.intent.action.trim(),
            "raw_text": request.intent.raw_text.trim(),
        },
        "entity_refs": normalized_contract_value(&request.entity_refs),
        "args": normalized_contract_value(&request.args),
    })
}

fn persisted_task_request_identity(task_run: &TaskRun) -> Value {
    if let Some(identity) = task_run.metadata.pointer("/request_identity") {
        return identity.clone();
    }

    json!({
        "route_key": task_run
            .metadata
            .pointer("/route_key")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "conversation_id": task_run
            .metadata
            .pointer("/conversation_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "message_id": task_run
            .metadata
            .pointer("/message_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "intent": {
            "domain": task_run.domain.trim(),
            "action": task_run.action.trim(),
            "raw_text": task_run.intent_text.trim(),
        },
        "entity_refs": normalized_contract_value(&task_run.entity_refs),
        "args": normalized_contract_value(&task_run.args),
    })
}

fn normalized_contract_value(value: &Value) -> Value {
    match value {
        Value::Null => json!({}),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(normalized_contract_value)
                .collect::<Vec<_>>(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), normalized_contract_value(value)))
                .collect(),
        ),
        Value::String(value) => Value::String(value.trim().to_string()),
        _ => value.clone(),
    }
}

fn upsert_json_string(target: &mut Value, pointer: &str, value: &str) {
    ensure_json_pointer_parent(target, pointer);
    if let Some((parent_pointer, leaf)) = split_json_pointer(pointer) {
        if let Some(parent) = target.pointer_mut(parent_pointer) {
            if let Some(map) = parent.as_object_mut() {
                map.insert(leaf.to_string(), Value::String(value.to_string()));
            }
        }
    }
}

fn upsert_json_string_vec(target: &mut Value, pointer: &str, values: &[String]) {
    ensure_json_pointer_parent(target, pointer);
    if let Some((parent_pointer, leaf)) = split_json_pointer(pointer) {
        if let Some(parent) = target.pointer_mut(parent_pointer) {
            if let Some(map) = parent.as_object_mut() {
                map.insert(
                    leaf.to_string(),
                    Value::Array(values.iter().cloned().map(Value::String).collect()),
                );
            }
        }
    }
}

fn ensure_json_pointer_parent(target: &mut Value, pointer: &str) {
    if !target.is_object() {
        *target = json!({});
    }
    let Some((parent_pointer, _)) = split_json_pointer(pointer) else {
        return;
    };
    let mut current = target;
    for segment in parent_pointer.split('/').filter(|segment| !segment.is_empty()) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        if !current.is_object() {
            *current = json!({});
        }
        let map = current.as_object_mut().expect("object");
        current = map.entry(segment).or_insert_with(|| json!({}));
    }
}

fn split_json_pointer(pointer: &str) -> Option<(&str, &str)> {
    pointer.rsplit_once('/')
}

fn task_message_id(request: &TaskRequest) -> String {
    request
        .message
        .as_ref()
        .map(|message| message.message_id.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn task_chat_type(request: &TaskRequest) -> String {
    request
        .message
        .as_ref()
        .map(|message| message.chat_type.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn task_attachment_transport_contract(request: &TaskRequest) -> Value {
    let Some(message) = request.message.as_ref() else {
        return Value::Array(Vec::new());
    };

    Value::Array(
        message
            .attachments
            .iter()
            .map(|attachment| {
                let download = attachment
                    .download
                    .as_ref()
                    .map(|download| {
                        json!({
                            "mode": download.mode.trim(),
                            "url": download.url.trim(),
                            "method": download.method.trim(),
                            "headers": normalized_contract_value(&download.headers),
                            "auth": download
                                .auth
                                .as_ref()
                                .map(|auth| json!({"type": auth.kind.trim()}))
                                .unwrap_or(Value::Null),
                            "expires_at": download.expires_at.trim(),
                            "max_size_bytes": download.max_size_bytes,
                        })
                    })
                    .unwrap_or(Value::Null);

                json!({
                    "attachment_id": attachment.attachment_id.trim(),
                    "type": attachment.attachment_type.trim(),
                    "name": attachment.name.trim(),
                    "mime_type": attachment.mime_type.trim(),
                    "size_bytes": attachment.size_bytes,
                    "download": download,
                    "metadata": normalized_contract_value(&attachment.metadata),
                })
            })
            .collect(),
    )
}

fn string_vec_at_paths(value: &Value, paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .find_map(|path| {
            value.pointer(path).and_then(Value::as_array).map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default()
}

fn source_route_key_from_context(
    task_run: &TaskRun,
    session: Option<&ConversationSession>,
) -> String {
    session
        .map(|session| session.route_key.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            task_run
                .metadata
                .pointer("/route_key")
                .and_then(Value::as_str)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_default()
}

fn task_status_from_task_run_status(status: TaskRunStatus) -> TaskStatus {
    match status {
        TaskRunStatus::Completed => TaskStatus::Completed,
        TaskRunStatus::NeedsInput | TaskRunStatus::Blocked => TaskStatus::NeedsInput,
        TaskRunStatus::Queued | TaskRunStatus::Running | TaskRunStatus::Failed => {
            TaskStatus::Failed
        }
    }
}

fn session_id_for_request(request: &TaskRequest) -> String {
    first_non_empty(&[
        request.source.session_id.as_str(),
        request.source.conversation_id.as_str(),
        request.source.user_id.as_str(),
    ])
    .map(|value| value.to_string())
    .unwrap_or_else(|| format!("task-{}", request.task_id))
}

fn step_id_for_request(request: &TaskRequest) -> String {
    let step_id = first_non_empty(&[request.step_id.as_str()])
        .map(|value| value.to_string())
        .unwrap_or_else(|| "s1".to_string());
    if looks_like_turn_local_step_id(&step_id) {
        format!("{}:{step_id}", request.task_id)
    } else {
        step_id
    }
}

fn looks_like_turn_local_step_id(step_id: &str) -> bool {
    step_id
        .strip_prefix("step_")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
}

fn workspace_id_for_request(request: &TaskRequest) -> String {
    first_string(&[&request.entity_refs, &request.args], &["/workspace_id"])
        .unwrap_or_else(|| "home-1".to_string())
}

fn url_encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn default_task_autonomy_level() -> String {
    "supervised".to_string()
}

fn expected_risk_level(request: &TaskRequest) -> RiskLevel {
    effective_risk_level(&Action {
        domain: request.intent.domain.trim().to_lowercase(),
        operation: request.intent.action.trim().to_lowercase(),
        resource: Value::Null,
        args: request.args.clone(),
        risk_level: RiskLevel::Low,
        requires_approval: request_requires_approval(request),
        dry_run: false,
    })
}

fn effective_autonomy_level(request: &TaskRequest) -> AutonomyLevel {
    let normalized = request.autonomy.level.trim().to_lowercase();
    match normalized.as_str() {
        "" | "supervised" => AutonomyLevel::Supervised,
        "readonly" | "read_only" | "read-only" => AutonomyLevel::ReadOnly,
        "full" => AutonomyLevel::Full,
        _ => AutonomyLevel::Supervised,
    }
}

fn effective_autonomy_level_for_task_run(request: &TaskRequest) -> String {
    autonomy_level_label(effective_autonomy_level(request)).to_string()
}

fn autonomy_level_label(level: AutonomyLevel) -> &'static str {
    match level {
        AutonomyLevel::ReadOnly => "readonly",
        AutonomyLevel::Supervised => "supervised",
        AutonomyLevel::Full => "full",
    }
}

fn approval_manager_for_level(level: AutonomyLevel) -> ApprovalManager {
    ApprovalManager::for_non_interactive(&AutonomyConfig {
        level,
        ..AutonomyConfig::default()
    })
}

fn effective_requires_approval(request: &TaskRequest) -> bool {
    let action = apply_governance_defaults(Action {
        domain: request.intent.domain.trim().to_lowercase(),
        operation: request.intent.action.trim().to_lowercase(),
        resource: Value::Null,
        args: request.args.clone(),
        risk_level: RiskLevel::Low,
        requires_approval: request_requires_approval(request),
        dry_run: false,
    });
    action.requires_approval
}

fn request_requires_approval(request: &TaskRequest) -> bool {
    bool_at_paths(&request.args, &["/approval/required", "/requires_approval"]).unwrap_or(false)
}

fn request_approval_token(request: &TaskRequest) -> Option<String> {
    first_string(
        &[&request.args, &request.entity_refs],
        &["/approval/token", "/approval_token"],
    )
}

fn request_approver_id(request: &TaskRequest) -> Option<String> {
    first_string(
        &[&request.args, &request.entity_refs],
        &["/approval/approver_id", "/approver_id"],
    )
}

fn approval_context_for_request(
    request: &TaskRequest,
    pending_approval: Option<&ApprovalTicket>,
) -> Option<ApprovalContext> {
    let token = request_approval_token(request);
    let required_token = pending_approval.map(|approval| approval.approval_id.clone());
    let approver_id = request_approver_id(request);
    if token.is_none() && required_token.is_none() && approver_id.is_none() {
        return None;
    }
    Some(ApprovalContext {
        token,
        required_token,
        approver_id,
    })
}

fn task_run_status_from_response(status: TaskStatus) -> TaskRunStatus {
    match status {
        TaskStatus::Completed => TaskRunStatus::Completed,
        TaskStatus::NeedsInput => TaskRunStatus::NeedsInput,
        TaskStatus::Failed => TaskRunStatus::Failed,
    }
}

fn task_step_status_from_response(status: TaskStatus) -> TaskStepRunStatus {
    match status {
        TaskStatus::Completed => TaskStepRunStatus::Success,
        TaskStatus::NeedsInput => TaskStepRunStatus::Blocked,
        TaskStatus::Failed => TaskStepRunStatus::Failed,
    }
}

fn execution_route_for_executor(executor_used: &str) -> ExecutionRoute {
    match executor_used.trim().to_lowercase().as_str() {
        "middleware_api" => ExecutionRoute::MiddlewareApi,
        "midcli" => ExecutionRoute::Midcli,
        "browser" => ExecutionRoute::Browser,
        "mcp" => ExecutionRoute::Mcp,
        _ => ExecutionRoute::Local,
    }
}

fn response_error_code(response: &TaskResponse) -> Option<String> {
    string_at_paths(
        &response.result.data,
        &[
            "/error_code",
            "/error/code",
            "/result/error_code",
            "/result/error/code",
        ],
    )
}

fn task_run_completed_at(status: TaskStatus, finished_at: &str) -> Option<String> {
    match status {
        TaskStatus::Completed | TaskStatus::Failed => Some(finished_at.to_string()),
        TaskStatus::NeedsInput => None,
    }
}

fn step_identity(request: &TaskRequest, response: &TaskResponse) -> (String, String) {
    if response.executor_used == "vision_executor" {
        return ("vision".to_string(), OP_ANALYZE_CAMERA.to_string());
    }
    (request.intent.domain.clone(), request.intent.action.clone())
}

fn protocol_string(args: &Value) -> Option<String> {
    if let Some(value) = string_at_paths(args, &["/protocol"]) {
        return Some(value);
    }
    args.pointer("/protocols")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .map(|values| values.join(" + "))
}

fn conversation_key(request: &TaskRequest) -> Option<String> {
    first_non_empty(&[
        request.source.conversation_id.as_str(),
        request.source.session_id.as_str(),
        request.source.user_id.as_str(),
    ])
    .map(|value| value.to_string())
}

fn normalize_command_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| {
            !ch.is_whitespace() && !matches!(ch, '，' | '。' | ',' | '.' | '？' | '?' | '！' | '!')
        })
        .collect()
}

#[cfg(test)]
fn should_route_general_message_to_knowledge(request: &TaskRequest) -> bool {
    fallback_general_message_plan(
        request.intent.raw_text.as_str(),
        first_string(&[&request.args], &["/device_hint"]).as_deref(),
    )
    .is_some_and(|plan| matches!(plan.kind, GeneralMessagePlanKind::KnowledgeSearch))
}

fn knowledge_search_query(request: &TaskRequest) -> Option<String> {
    first_string(
        &[&request.args],
        &[
            "/query",
            "/keyword",
            "/keywords/0",
            "/search/query",
            "/knowledge/query",
        ],
    )
    .or_else(|| infer_query_from_raw_text(&request.intent.raw_text))
}

fn infer_query_from_raw_text(raw_text: &str) -> Option<String> {
    let trimmed = raw_text
        .trim()
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '，' | '。' | ',' | '.' | '？' | '?' | '！' | '!' | '：' | ':'
                )
        })
        .to_string();
    if trimmed.is_empty() {
        return None;
    }

    let mut candidate = trimmed.clone();
    for pattern in [
        "请帮我",
        "帮我",
        "找到",
        "找一下",
        "找出",
        "查一下",
        "查找",
        "搜索",
        "搜一下",
        "搜",
        "检索",
        "和",
        "关于",
        "有关的",
        "相关的",
        "有关",
        "文件",
        "文档",
        "图片",
        "照片",
        "资料",
        "内容",
        "file",
        "files",
        "document",
        "documents",
        "image",
        "images",
        "photo",
        "photos",
        "picture",
        "pictures",
        "search for",
        "search",
        "find",
        "lookup",
        "look up",
    ] {
        candidate = candidate.replace(pattern, " ");
    }

    let candidate = candidate
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    if candidate.is_empty() {
        Some(trimmed)
    } else {
        Some(candidate)
    }
}

fn parse_general_message_plan(text: &str) -> Option<GeneralMessagePlan> {
    let payload = parse_json_object_from_text(text)?;
    let payload = serde_json::from_value::<GeneralMessagePlanPayload>(payload).ok()?;
    let kind = match payload.action.trim().to_ascii_lowercase().as_str() {
        "camera_snapshot" | "snapshot" => GeneralMessagePlanKind::CameraSnapshot,
        "camera_record_clip" | "record_clip" | "clip" => GeneralMessagePlanKind::CameraRecordClip,
        "knowledge_search" | "search" => GeneralMessagePlanKind::KnowledgeSearch,
        _ => GeneralMessagePlanKind::Unsupported,
    };
    Some(GeneralMessagePlan {
        kind,
        camera_hint: payload
            .camera_hint
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        query: payload
            .query
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        reason: payload
            .reason
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
}

fn parse_json_object_from_text(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }

    let fenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)?;
    serde_json::from_str::<Value>(fenced).ok()
}

fn fallback_general_message_plan(
    raw_text: &str,
    default_camera_hint: Option<&str>,
) -> Option<GeneralMessagePlan> {
    let normalized = normalize_command_text(raw_text);
    if normalized.is_empty() {
        return None;
    }

    if matches_any(&normalized, &["录一段", "录视频", "拍视频", "录个视频", "录像"]) {
        return Some(GeneralMessagePlan {
            kind: GeneralMessagePlanKind::CameraRecordClip,
            camera_hint: default_camera_hint.map(str::to_string),
            query: None,
            reason: Some("fallback rule inferred a short clip request".to_string()),
        });
    }
    if matches_any(&normalized, &["抓拍", "拍照", "拍一张", "看一眼", "截一张"]) {
        return Some(GeneralMessagePlan {
            kind: GeneralMessagePlanKind::CameraSnapshot,
            camera_hint: default_camera_hint.map(str::to_string),
            query: None,
            reason: Some("fallback rule inferred a snapshot request".to_string()),
        });
    }
    if matches_any(
        &normalized,
        &["找一下", "找到", "查一下", "搜索", "检索", "找照片", "找视频"],
    ) {
        return Some(GeneralMessagePlan {
            kind: GeneralMessagePlanKind::KnowledgeSearch,
            camera_hint: None,
            query: infer_query_from_raw_text(raw_text),
            reason: Some("fallback rule inferred a knowledge search request".to_string()),
        });
    }
    None
}

fn matches_any(normalized: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .map(|candidate| normalize_command_text(candidate))
        .any(|candidate| normalized.contains(&candidate))
}

fn build_general_message_planner_prompt(
    request: &TaskRequest,
    camera_targets: &[ResolvedCameraTarget],
    selected_camera_device_id: Option<&str>,
    writable_root: &str,
    capture_subdirectory: &str,
    clip_length_seconds: u32,
) -> String {
    let cameras = camera_targets
        .iter()
        .map(|target| {
            json!({
                "device_id": target.device_id,
                "display_name": target.display_name,
                "room_name": target.room_name,
                "vendor": target.vendor,
                "model": target.model,
            })
        })
        .collect::<Vec<_>>();
    let available_camera_hint = selected_camera_device_id.unwrap_or_default();
    format!(
        concat!(
            "You are a strict HarborBeacon planner. ",
            "Interpret the Chinese user message and return ONLY JSON with fields: ",
            "{{\"action\":\"camera_snapshot|camera_record_clip|knowledge_search|unsupported\",",
            "\"camera_hint\":\"string|null\",\"query\":\"string|null\",\"reason\":\"string\"}}.\n",
            "Do not invent new actions. ",
            "A snapshot means taking one image. ",
            "A clip means a short video clip only, with configured duration {clip_length_seconds} seconds. ",
            "Search means searching the configured capture directory using the described visual feature.\n",
            "Selected camera device: {available_camera_hint}\n",
            "Writable root: {writable_root}\n",
            "Capture subdirectory: {capture_subdirectory}\n",
            "Known cameras: {cameras}\n",
            "User message: {message}\n"
        ),
        clip_length_seconds = clip_length_seconds,
        available_camera_hint = available_camera_hint,
        writable_root = writable_root,
        capture_subdirectory = capture_subdirectory,
        cameras = serde_json::to_string(&cameras).unwrap_or_else(|_| "[]".to_string()),
        message = request.intent.raw_text,
    )
}

fn knowledge_search_roots(request: &TaskRequest) -> Vec<String> {
    first_string_vec(
        &[&request.args],
        &["/roots", "/search/roots", "/knowledge/roots"],
    )
}

fn knowledge_result_limit(request: &TaskRequest) -> usize {
    usize_at_paths(
        &request.args,
        &["/limit", "/search/limit", "/knowledge/limit"],
    )
    .unwrap_or(5)
    .clamp(1, 10)
}

fn knowledge_modalities(request: &TaskRequest) -> (bool, bool) {
    let requested = first_string_vec(
        &[&request.args],
        &["/modalities", "/search/modalities", "/knowledge/modalities"],
    )
    .into_iter()
    .map(|item| item.to_lowercase())
    .collect::<Vec<_>>();
    if !requested.is_empty() {
        let include_documents = requested.iter().any(|item| {
            matches!(
                item.as_str(),
                "document" | "documents" | "doc" | "docs" | "text"
            )
        });
        let include_images = requested.iter().any(|item| {
            matches!(
                item.as_str(),
                "image" | "images" | "photo" | "photos" | "picture" | "pictures"
            )
        });
        return (include_documents, include_images);
    }

    if request
        .intent
        .domain
        .trim()
        .eq_ignore_ascii_case(KNOWLEDGE_DOMAIN)
        && request
            .intent
            .action
            .trim()
            .eq_ignore_ascii_case(KNOWLEDGE_OP_SEARCH)
    {
        return (true, true);
    }

    let normalized = request.intent.raw_text.to_lowercase();
    let asks_for_documents = [
        "文档",
        "文件",
        "资料",
        "document",
        "documents",
        "file",
        "files",
    ]
    .iter()
    .any(|token| normalized.contains(token));
    let asks_for_images = [
        "图片", "照片", "image", "images", "photo", "photos", "picture",
    ]
    .iter()
    .any(|token| normalized.contains(token));

    match (asks_for_documents, asks_for_images) {
        (true, false) => (true, false),
        (false, true) => (false, true),
        _ => (true, true),
    }
}

fn room_aliases<'a>(name: &'a str, room: &'a str) -> Vec<&'static str> {
    let normalized = format!("{} {}", name.to_lowercase(), room.to_lowercase());
    let mut aliases = Vec::new();
    if normalized.contains("living room") {
        aliases.extend(["客厅", "大厅", "起居室"]);
    }
    if normalized.contains("front door") || normalized.contains("entry") {
        aliases.extend(["门口", "玄关", "入户"]);
    }
    if normalized.contains("garage") {
        aliases.extend(["车库"]);
    }
    aliases
}

fn string_at_paths(value: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        value
            .pointer(path)
            .and_then(Value::as_str)
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
    })
}

fn usize_at_paths(value: &Value, paths: &[&str]) -> Option<usize> {
    paths.iter().find_map(|path| {
        let item = value.pointer(path)?;
        if let Some(number) = item.as_u64() {
            return usize::try_from(number).ok();
        }
        item.as_str()?.trim().parse::<usize>().ok()
    })
}

fn u64_at_paths(value: &Value, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|path| {
        let item = value.pointer(path)?;
        if let Some(number) = item.as_u64() {
            return Some(number);
        }
        item.as_str()?.trim().parse::<u64>().ok()
    })
}

fn bool_at_paths(value: &Value, paths: &[&str]) -> Option<bool> {
    paths.iter().find_map(|path| {
        let item = value.pointer(path)?;
        if let Some(flag) = item.as_bool() {
            return Some(flag);
        }
        match item.as_str()?.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        }
    })
}

fn first_string(values: &[&Value], paths: &[&str]) -> Option<String> {
    values
        .iter()
        .find_map(|value| string_at_paths(value, paths))
}

fn first_u16(values: &[&Value], paths: &[&str]) -> Option<u16> {
    values.iter().find_map(|value| {
        paths.iter().find_map(|path| {
            let item = value.pointer(path)?;
            if let Some(number) = item.as_u64() {
                return u16::try_from(number).ok();
            }
            item.as_str()?.trim().parse::<u16>().ok()
        })
    })
}

fn first_string_vec(values: &[&Value], paths: &[&str]) -> Vec<String> {
    for value in values {
        for path in paths {
            if let Some(array) = value.pointer(path).and_then(Value::as_array) {
                let collected = array
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>();
                if !collected.is_empty() {
                    return collected;
                }
            }
        }
    }
    Vec::new()
}

fn first_non_empty<'a>(values: &[&'a str]) -> Option<&'a str> {
    values
        .iter()
        .copied()
        .find(|value| !value.trim().is_empty())
}

fn notification_platform_from_value(value: &str) -> Option<String> {
    match value.trim().to_lowercase().as_str() {
        "im_bridge" | "feishu" => Some("feishu".to_string()),
        "wecom" => Some("wecom".to_string()),
        "telegram" => Some("telegram".to_string()),
        "webhook" => Some("webhook".to_string()),
        "local_ui" => Some("local_ui".to_string()),
        _ => None,
    }
}

fn notification_delivery_mode_from_value(value: &str) -> NotificationDeliveryMode {
    match value.trim().to_lowercase().as_str() {
        "reply" => NotificationDeliveryMode::Reply,
        "update" => NotificationDeliveryMode::Update,
        _ => NotificationDeliveryMode::Send,
    }
}

fn notification_payload_format_from_value(value: &str) -> NotificationPayloadFormat {
    match value.trim().to_lowercase().as_str() {
        "markdown" => NotificationPayloadFormat::Markdown,
        "lark_card" | "card" => NotificationPayloadFormat::LarkCard,
        "json" => NotificationPayloadFormat::Json,
        _ => NotificationPayloadFormat::PlainText,
    }
}

fn task_artifact_to_notification_attachment(
    artifact: &TaskArtifact,
) -> Option<NotificationAttachment> {
    let kind = match artifact.kind.trim().to_lowercase().as_str() {
        "image" => NotificationAttachmentKind::Image,
        "video" => NotificationAttachmentKind::Video,
        "link" => NotificationAttachmentKind::Link,
        "json" | "card" | "text" => NotificationAttachmentKind::Json,
        _ => return None,
    };
    Some(NotificationAttachment {
        kind,
        label: artifact.label.clone(),
        mime_type: artifact.mime_type.clone(),
        path: artifact.path.clone(),
        url: artifact.url.clone(),
        metadata: artifact.metadata.clone(),
    })
}

#[cfg(test)]
fn resolve_notification_recipient(
    destination: &str,
    state: &AdminConsoleState,
    requester_user_id: &str,
) -> Option<NotificationRecipient> {
    let bindings = resolved_identity_binding_records(state);
    if destination.trim().is_empty() {
        return None;
    }

    if let Some(recipient) = recipient_from_literal_destination(destination, &bindings) {
        return Some(recipient);
    }

    if let Some(recipient) = recipient_from_binding_match(destination, &bindings) {
        return Some(recipient);
    }

    if !requester_user_id.trim().is_empty() {
        if let Some(binding) = bindings.iter().find(|binding| {
            binding
                .user_id
                .as_deref()
                .map(|value| value == requester_user_id)
                .unwrap_or(false)
        }) {
            if let Some(recipient) = recipient_from_binding(binding) {
                return Some(recipient);
            }
        }
    }

    let chat_bindings = bindings
        .iter()
        .filter_map(recipient_from_binding)
        .collect::<Vec<_>>();
    if chat_bindings.len() == 1 {
        return chat_bindings.into_iter().next();
    }

    None
}

fn proactive_notification_destination(
    _request: &TaskRequest,
    state: &AdminConsoleState,
) -> Option<NotificationDestination> {
    let target = default_notification_target(state)?;
    Some(NotificationDestination {
        kind: NotificationDestinationKind::Conversation,
        route_key: target.route_key.clone(),
        id: String::new(),
        platform: target.platform_hint.clone(),
        recipient: None,
    })
}

fn default_notification_target(state: &AdminConsoleState) -> Option<&NotificationTargetRecord> {
    state
        .notification_targets
        .iter()
        .find(|target| target.is_default)
        .or_else(|| state.notification_targets.first())
        .filter(|target| !target.route_key.trim().is_empty())
}

#[cfg(test)]
fn recipient_from_literal_destination(
    destination: &str,
    bindings: &[IdentityBindingRecord],
) -> Option<NotificationRecipient> {
    if destination.starts_with("oc_") {
        return Some(NotificationRecipient {
            recipient_id: destination.to_string(),
            recipient_type: NotificationRecipientIdType::ChatId,
        });
    }
    if destination.starts_with("ou_") {
        let _label = bindings
            .iter()
            .find(|binding| binding.open_id == destination)
            .map(|binding| binding.display_name.clone())
            .unwrap_or_else(|| destination.to_string());
        return Some(NotificationRecipient {
            recipient_id: destination.to_string(),
            recipient_type: NotificationRecipientIdType::OpenId,
        });
    }
    None
}

#[cfg(test)]
fn recipient_from_binding_match(
    destination: &str,
    bindings: &[IdentityBindingRecord],
) -> Option<NotificationRecipient> {
    let normalized = destination.trim();
    bindings
        .iter()
        .find(|binding| {
            binding.display_name == normalized
                || binding.open_id == normalized
                || binding
                    .chat_id
                    .as_deref()
                    .map(|value| value == normalized)
                    .unwrap_or(false)
                || binding
                    .user_id
                    .as_deref()
                    .map(|value| value == normalized)
                    .unwrap_or(false)
        })
        .and_then(recipient_from_binding)
}

#[cfg(test)]
fn recipient_from_binding(binding: &IdentityBindingRecord) -> Option<NotificationRecipient> {
    if let Some(chat_id) = binding
        .chat_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(NotificationRecipient {
            recipient_id: chat_id.clone(),
            recipient_type: NotificationRecipientIdType::ChatId,
        });
    }
    if !binding.open_id.trim().is_empty() {
        return Some(NotificationRecipient {
            recipient_id: binding.open_id.clone(),
            recipient_type: NotificationRecipientIdType::OpenId,
        });
    }
    None
}

fn notification_request_hash(request: &NotificationRequest) -> String {
    let identity = notification_request_identity(request);
    let bytes = serde_json::to_vec(&identity).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn notification_request_identity(request: &NotificationRequest) -> Value {
    json!({
        "trace_id": request.trace_id.trim(),
        "source": {
            "service": request.source.service.trim(),
            "module": request.source.module.trim(),
            "event_type": request.source.event_type.trim(),
        },
        "destination": {
            "kind": serde_json::to_value(request.destination.kind).unwrap_or(Value::Null),
            "route_key": request.destination.route_key.trim(),
            "id": request.destination.id.trim(),
            "platform": request.destination.platform.trim(),
            "recipient": request.destination.recipient.as_ref().map(|recipient| json!({
                "recipient_id": recipient.recipient_id.trim(),
                "recipient_type": serde_json::to_value(recipient.recipient_type).unwrap_or(Value::Null),
            })).unwrap_or(Value::Null),
        },
        "content": {
            "title": request.content.title.trim(),
            "body": request.content.body.trim(),
            "payload_format": serde_json::to_value(request.content.payload_format).unwrap_or(Value::Null),
            "structured_payload": normalized_contract_value(&request.content.structured_payload),
            "attachments": request.content.attachments.iter().map(|attachment| {
                json!({
                    "kind": serde_json::to_value(attachment.kind).unwrap_or(Value::Null),
                    "label": attachment.label.trim(),
                    "mime_type": attachment.mime_type.trim(),
                    "path": attachment.path.clone().unwrap_or_default(),
                    "url": attachment.url.clone().unwrap_or_default(),
                    "metadata": normalized_contract_value(&attachment.metadata),
                })
            }).collect::<Vec<_>>(),
        },
        "delivery": {
            "mode": serde_json::to_value(request.delivery.mode).unwrap_or(Value::Null),
            "reply_to_message_id": request.delivery.reply_to_message_id.trim(),
            "update_message_id": request.delivery.update_message_id.trim(),
        },
        "metadata": {
            "correlation_id": request.metadata.correlation_id.trim(),
        },
    })
}

fn notification_delivery_outcome(
    notification_request: &NotificationRequest,
    result: Result<
        crate::connectors::notifications::NotificationDeliveryRecord,
        NotificationDeliveryError,
    >,
) -> NotificationDeliveryOutcome {
    let is_proactive = notification_request.destination.kind == NotificationDestinationKind::Recipient
        || (notification_request.destination.kind == NotificationDestinationKind::Conversation
            && !notification_request.destination.route_key.trim().is_empty()
            && notification_request.destination.recipient.is_none()
            && notification_request.destination.id.trim().is_empty()
            && !notification_request.destination.platform.trim().is_empty()
            && notification_request.delivery.reply_to_message_id.trim().is_empty()
            && notification_request.delivery.update_message_id.trim().is_empty());
    match result {
        Ok(record) if record.ok => NotificationDeliveryOutcome {
            event_type: "task.notification_delivered",
            severity: EventSeverity::Info,
            payload: serde_json::to_value(record).unwrap_or(Value::Null),
        },
        Ok(record) => NotificationDeliveryOutcome {
            event_type: if is_proactive {
                "task.proactive_delivery_failed"
            } else {
                "task.notification_failed"
            },
            severity: EventSeverity::Warning,
            payload: serde_json::to_value(record).unwrap_or(Value::Null),
        },
        Err(NotificationDeliveryError::RequestRejected {
            status_code,
            envelope,
        }) => NotificationDeliveryOutcome {
            event_type: if is_proactive {
                "task.proactive_delivery_failed"
            } else {
                "task.notification_rejected"
            },
            severity: if status_code >= 500 {
                EventSeverity::Error
            } else {
                EventSeverity::Warning
            },
            payload: json!({
                "status": "rejected",
                "http_status": status_code,
                "notification_id": notification_request.notification_id,
                "idempotency_key": notification_request.delivery.idempotency_key,
                "destination": notification_request.destination,
                "route_mode": if is_proactive { "proactive" } else { "source_bound" },
                "error": envelope.error,
                "trace_id": envelope.trace_id,
            }),
        },
        Err(error) => NotificationDeliveryOutcome {
            event_type: if is_proactive {
                "task.proactive_delivery_failed"
            } else {
                "task.notification_failed"
            },
            severity: EventSeverity::Error,
            payload: json!({
                "status": "failed",
                "notification_id": notification_request.notification_id,
                "idempotency_key": notification_request.delivery.idempotency_key,
                "destination": notification_request.destination,
                "route_mode": if is_proactive { "proactive" } else { "source_bound" },
                "error": error.to_string(),
            }),
        },
    }
}

fn env_flag_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn current_timestamp_millis() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn new_event_id() -> String {
    Uuid::new_v4().as_simple().to_string()
}

fn new_approval_id() -> String {
    Uuid::new_v4().as_simple().to_string()
}

fn ensure_resume_token() -> String {
    Uuid::new_v4().as_simple().to_string()
}

fn new_task_id() -> String {
    Uuid::new_v4().as_simple().to_string()
}

fn new_audit_ref() -> String {
    Uuid::new_v4().as_simple().to_string()[..12].to_string()
}

fn new_media_asset_id() -> String {
    format!("asset-{}", Uuid::new_v4().as_simple())
}

fn new_media_session_id() -> String {
    format!("media-session-{}", Uuid::new_v4().as_simple())
}

fn new_share_link_id() -> String {
    format!("share-link-{}", Uuid::new_v4().as_simple())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::Engine as _;
    use serde_json::{json, Value};

    use super::{
        artifact_kind_from_name, build_artifact_records, conversation_key,
        effective_autonomy_level, effective_autonomy_level_for_task_run,
        effective_requires_approval, ensure_safe_capture_root, env_flag_enabled,
        format_pending_candidates, infer_query_from_raw_text, normalize_command_text,
        notification_delivery_outcome, pending_candidates_from_results, protocol_string,
        resolve_notification_recipient, room_aliases, should_route_general_message_to_knowledge,
        PendingTaskCandidate, TaskApiService, TaskArtifact, TaskIntent, TaskMessage, TaskRequest,
        TaskRequestAcceptance, TaskSource, TaskStatus, ALLOW_NON_HARBOROS_CAPTURE_ROOT_ENV,
    };
    use crate::connectors::notifications::{
        NotificationContent, NotificationDelivery, NotificationDeliveryError,
        NotificationDeliveryMode, NotificationDestination, NotificationDestinationKind,
        NotificationMetadata, NotificationPayloadFormat, NotificationRecipientIdType,
        NotificationRequest, NotificationSource,
        SharedHttpErrorDetail, SharedHttpErrorEnvelope,
    };
    use crate::connectors::storage::StorageTarget;
    use crate::control_plane::approvals::ApprovalStatus;
    use crate::control_plane::auth::{AuthSource, IdentityBinding};
    use crate::control_plane::media::{MediaAssetKind, StorageTargetKind};
    use crate::control_plane::tasks::{
        ArtifactKind, ConversationSession, ExecutionRoute, TaskRunStatus, TaskStepRunStatus,
    };
    use crate::runtime::admin_console::{
        AdminConsoleState, AdminConsoleStore, IdentityBindingRecord, RemoteViewConfig,
    };
    use crate::runtime::hub::HubScanResultItem;
    use crate::runtime::media::{SnapshotCaptureResult, SnapshotFormat};
    use crate::runtime::registry::{
        CameraCapabilities, CameraDevice, CameraStreamRef, DeviceRegistryStore, DeviceStatus,
        ResolvedCameraTarget, StreamTransport,
    };
    use crate::runtime::task_session::{
        PendingTaskConnect, TaskConversationState, TaskConversationStore,
    };

    static RETRIEVAL_GATE_TEST_LOCK: Mutex<()> = Mutex::new(());
    static HARBOROS_TASK_API_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn unique_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}.json"))
    }

    fn unique_dir(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }

    fn reset_harbor_task_api_env() {
        for name in [
            "HARBOR_FORCE_MIDDLEWARE_ERROR",
            "HARBOR_URL",
            "HARBOR_MIDDLEWARE_URL",
            "HARBOR_API_KEY",
            "HARBOR_MIDDLEWARE_API_KEY",
            "HARBOR_USER",
            "HARBOR_PASSWORD",
            "HARBOR_MIDCLI_URL",
            "HARBOR_MIDCLI_USER",
            "HARBOR_MIDCLI_PASSWORD",
            "HARBOR_DISABLE_MIDDLEWARE",
            "HARBOR_DISABLE_MIDCLI",
            "HARBOR_MIDCLI_BIN",
            "HARBOR_MIDCLI_PASSTHROUGH",
        ] {
            std::env::remove_var(name);
        }
    }

    fn build_task_api_service(
        prefix: &str,
    ) -> (
        TaskApiService,
        TaskConversationStore,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let admin_path = unique_path(&format!("{prefix}-admin"));
        let registry_path = unique_path(&format!("{prefix}-registry"));
        let conversation_path = unique_path(&format!("{prefix}-conversation"));
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        (
            service,
            conversation_store,
            admin_path,
            registry_path,
            conversation_path,
        )
    }

    fn cleanup_task_api_service(
        admin_path: std::path::PathBuf,
        registry_path: std::path::PathBuf,
        conversation_path: std::path::PathBuf,
    ) {
        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn conversation_key_prefers_conversation_id() {
        let request = TaskRequest {
            task_id: "task-1".to_string(),
            trace_id: "trace-1".to_string(),
            step_id: "step-1".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "session-1".to_string(),
                route_key: String::new(),
            },
            intent: TaskIntent::default(),
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };

        assert_eq!(conversation_key(&request), Some("chat-1".to_string()));
    }

    #[test]
    fn handle_task_persists_route_key_and_message_summary() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-route-message");
        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let request = TaskRequest {
            task_id: "task-route-message".to_string(),
            trace_id: "trace-route-message".to_string(),
            step_id: "step-route-message".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-route-message".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-route-message".to_string(),
                route_key: "gw_route_01".to_string(),
            },
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "ping".to_string(),
                raw_text: "ping".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_01".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: vec![super::TaskMessageAttachment {
                    attachment_id: "att_01".to_string(),
                    attachment_type: "file".to_string(),
                    name: "front-door.jpg".to_string(),
                    mime_type: "image/jpeg".to_string(),
                    size_bytes: Some(2048),
                    download: Some(super::TaskMessageAttachmentDownload {
                        mode: "proxy".to_string(),
                        url: "https://gateway.local/files/att_01".to_string(),
                        method: "GET".to_string(),
                        headers: json!({
                            "Authorization": "Bearer opaque-download-token"
                        }),
                        auth: Some(super::TaskMessageAttachmentDownloadAuth {
                            kind: "bearer".to_string(),
                        }),
                        expires_at: "2026-04-18T12:00:00Z".to_string(),
                        max_size_bytes: Some(4096),
                    }),
                    metadata: json!({
                        "transport": "opaque",
                        "provider_file_key": "file_key_01"
                    }),
                }],
            }),
        };

        let response = service.handle_task(request);
        assert_eq!(response.status, TaskStatus::Failed);

        let session = service
            .conversation_store()
            .load_session("sess-route-message")
            .expect("load session")
            .expect("session");
        assert_eq!(session.route_key, "gw_route_01");
        assert_eq!(session.last_message_id, "om_01");
        assert_eq!(session.chat_type, "group");

        let task_run = service
            .conversation_store()
            .load_task_run("task-route-message")
            .expect("load task run")
            .expect("task run");
        assert_eq!(task_run.metadata["route_key"], "gw_route_01");
        assert_eq!(task_run.metadata["message_id"], "om_01");
        assert_eq!(task_run.metadata["chat_type"], "group");
        assert_eq!(
            task_run.metadata["attachments"][0]["attachment_id"],
            "att_01"
        );
        assert_eq!(
            task_run.metadata["attachments"][0]["download"]["headers"]["Authorization"],
            "Bearer opaque-download-token"
        );
        assert_eq!(
            task_run.metadata["attachments"][0]["metadata"]["provider_file_key"],
            "file_key_01"
        );

        let task_step = service
            .conversation_store()
            .load_task_step("step-route-message")
            .expect("load task step")
            .expect("task step");
        assert_eq!(task_step.trace_id, "trace-route-message");
        assert_eq!(task_step.route_key, "gw_route_01");
        assert_eq!(
            task_step.input_payload["source"]["route_key"],
            "gw_route_01"
        );
        assert_eq!(task_step.input_payload["message"]["message_id"], "om_01");
        assert_eq!(task_step.input_payload["message"]["chat_type"], "group");
        assert_eq!(
            task_step.input_payload["message"]["attachments"][0]["download"]["mode"],
            "proxy"
        );

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn accept_or_replay_task_returns_replayed_response_for_identical_task_id() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-idempotent-replay");
        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let request = TaskRequest {
            task_id: "task-idempotent".to_string(),
            trace_id: "trace-idempotent".to_string(),
            step_id: "step-idempotent".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-idempotent".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-idempotent".to_string(),
                route_key: "gw_route_idempotent".to_string(),
            },
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "ping".to_string(),
                raw_text: "ping".to_string(),
            },
            entity_refs: json!({}),
            args: json!({}),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_idempotent".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };

        let initial = service.handle_task(request.clone());
        assert_eq!(initial.status, TaskStatus::Failed);

        let replay = service
            .accept_or_replay_task(&request)
            .expect("idempotency decision");
        match replay {
            TaskRequestAcceptance::Replay(response) => {
                assert_eq!(response.task_id, "task-idempotent");
                assert_eq!(response.trace_id, "trace-idempotent");
                assert_eq!(response.status, TaskStatus::Failed);
                assert_eq!(response.executor_used, initial.executor_used);
            }
            other => panic!("expected replay, got {other:?}"),
        }

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn accept_or_replay_task_rejects_conflicting_task_identity() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-idempotent-conflict");
        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let request = TaskRequest {
            task_id: "task-idempotent-conflict".to_string(),
            trace_id: "trace-idempotent-conflict".to_string(),
            step_id: "step-idempotent-conflict".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-idempotent-conflict".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-idempotent-conflict".to_string(),
                route_key: "gw_route_conflict".to_string(),
            },
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "ping".to_string(),
                raw_text: "ping".to_string(),
            },
            entity_refs: json!({}),
            args: json!({}),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_conflict".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        let conflicting = TaskRequest {
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "ping".to_string(),
                raw_text: "ping again".to_string(),
            },
            ..request.clone()
        };

        let initial = service.handle_task(request);
        assert_eq!(initial.status, TaskStatus::Failed);

        let replay = service
            .accept_or_replay_task(&conflicting)
            .expect("idempotency decision");
        match replay {
            TaskRequestAcceptance::Conflict(message) => {
                assert!(message.contains("different request identity"));
            }
            other => panic!("expected conflict, got {other:?}"),
        }

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn accept_or_replay_task_preserves_original_response_when_turn_local_step_id_is_reused() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-idempotent-step-scope");
        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let first = TaskRequest {
            task_id: "task-step-scope-a".to_string(),
            trace_id: "trace-step-scope-a".to_string(),
            step_id: "step_01".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-step-scope".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-step-scope".to_string(),
                route_key: "gw_route_step_scope".to_string(),
            },
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "ping".to_string(),
                raw_text: "ping".to_string(),
            },
            entity_refs: json!({}),
            args: json!({}),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_step_scope_a".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        let second = TaskRequest {
            task_id: "task-step-scope-b".to_string(),
            trace_id: "trace-step-scope-b".to_string(),
            step_id: "step_01".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-step-scope".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-step-scope".to_string(),
                route_key: "gw_route_step_scope".to_string(),
            },
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "status".to_string(),
                raw_text: "status".to_string(),
            },
            entity_refs: json!({}),
            args: json!({}),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_step_scope_b".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };

        let first_response = service.handle_task(first.clone());
        assert_eq!(first_response.status, TaskStatus::Failed);
        assert!(first_response.result.message.contains("system.ping"));

        let second_response = service.handle_task(second);
        assert_eq!(second_response.status, TaskStatus::Failed);
        assert!(second_response.result.message.contains("system.status"));

        assert!(service
            .conversation_store()
            .load_task_step("step_01")
            .expect("load raw step id")
            .is_none());
        let first_step = service
            .conversation_store()
            .load_task_step("task-step-scope-a:step_01")
            .expect("load first scoped step")
            .expect("first scoped step");
        let second_step = service
            .conversation_store()
            .load_task_step("task-step-scope-b:step_01")
            .expect("load second scoped step")
            .expect("second scoped step");
        assert_eq!(first_step.task_id, "task-step-scope-a");
        assert_eq!(second_step.task_id, "task-step-scope-b");

        let replay = service
            .accept_or_replay_task(&first)
            .expect("idempotency decision");
        match replay {
            TaskRequestAcceptance::Replay(response) => {
                assert_eq!(response.status, TaskStatus::Failed);
                assert!(response.result.message.contains("system.ping"));
                assert!(!response.result.message.contains("system.status"));
            }
            other => panic!("expected replay, got {other:?}"),
        }

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn protocol_string_joins_protocol_arrays() {
        let args = json!({"protocols":["onvif", "rtsp_probe"]});
        assert_eq!(
            protocol_string(&args),
            Some("onvif + rtsp_probe".to_string())
        );
    }

    #[test]
    fn pending_candidates_only_keep_unreachable_items() {
        let pending = pending_candidates_from_results(&[
            HubScanResultItem {
                candidate_id: "cand-1".to_string(),
                device_id: None,
                name: "Cam 1".to_string(),
                room: String::new(),
                ip: "192.168.1.20".to_string(),
                port: 554,
                protocol: "RTSP".to_string(),
                note: String::new(),
                reachable: false,
                registered: false,
                requires_auth: true,
                vendor: None,
                model: None,
                rtsp_paths: vec!["/live".to_string()],
            },
            HubScanResultItem {
                candidate_id: "cand-2".to_string(),
                device_id: None,
                name: "Cam 2".to_string(),
                room: String::new(),
                ip: "192.168.1.21".to_string(),
                port: 554,
                protocol: "RTSP".to_string(),
                note: String::new(),
                reachable: true,
                registered: true,
                requires_auth: false,
                vendor: None,
                model: None,
                rtsp_paths: vec!["/live".to_string()],
            },
        ]);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].candidate_id, "cand-1");
    }

    #[test]
    fn format_pending_candidates_mentions_auth() {
        let rendered = format_pending_candidates(&[PendingTaskCandidate {
            candidate_id: "cand-1".to_string(),
            name: "Living Room Cam".to_string(),
            ip: "192.168.1.20".to_string(),
            room: Some("living room".to_string()),
            port: 554,
            rtsp_paths: vec!["/live".to_string()],
            requires_auth: true,
            vendor: None,
            model: None,
        }]);

        assert!(rendered.contains("需要密码"));
    }

    #[test]
    fn connect_request_preserves_room_hint() {
        let request = super::candidate_to_connect_request(
            &PendingTaskCandidate {
                candidate_id: "cand-1".to_string(),
                name: "Living Room Cam".to_string(),
                ip: "192.168.1.20".to_string(),
                room: Some("Living Room".to_string()),
                port: 554,
                rtsp_paths: vec!["/live".to_string()],
                requires_auth: false,
                vendor: None,
                model: None,
            },
            None,
        );

        assert_eq!(request.room.as_deref(), Some("Living Room"));
        assert!(request.snapshot_url.is_none());
    }

    #[test]
    fn normalize_command_text_strips_punctuation() {
        assert_eq!(
            normalize_command_text("分析 客厅摄像头！"),
            "分析客厅摄像头"
        );
    }

    #[test]
    fn infer_query_from_raw_text_keeps_search_subject() {
        assert_eq!(
            infer_query_from_raw_text("帮我找到和樱花有关的文件"),
            Some("樱花".to_string())
        );
    }

    #[test]
    fn room_aliases_cover_living_room() {
        let aliases = room_aliases("Living Room Cam", "living room");
        assert!(aliases.contains(&"客厅"));
    }

    #[test]
    fn build_artifact_records_maps_image_kind() {
        let request = TaskRequest {
            task_id: "task-1".to_string(),
            trace_id: "trace-1".to_string(),
            step_id: "step-1".to_string(),
            source: TaskSource::default(),
            intent: TaskIntent::default(),
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };
        let artifacts = build_artifact_records(
            &request,
            "step-1",
            &[super::TaskArtifact {
                kind: "image".to_string(),
                label: "抓拍图片".to_string(),
                mime_type: "image/jpeg".to_string(),
                media_asset_id: Some("asset-1".to_string()),
                path: Some("snap.jpg".to_string()),
                url: None,
                metadata: Value::Null,
            }],
        );

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_kind, ArtifactKind::Image);
        assert_eq!(artifacts[0].media_asset_id.as_deref(), Some("asset-1"));
        assert_eq!(artifacts[0].trace_id, "trace-1");
        assert_eq!(artifacts[0].route_key, "");
        assert_eq!(artifact_kind_from_name("json"), ArtifactKind::Json);
    }

    #[test]
    fn build_snapshot_media_asset_populates_platform_fields() {
        let request = TaskRequest {
            task_id: "task-snapshot".to_string(),
            trace_id: "trace-snapshot".to_string(),
            step_id: "step-snapshot".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_snapshot".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "snapshot".to_string(),
                raw_text: "抓拍门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_snapshot".to_string(),
                chat_type: "p2p".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        let target = ResolvedCameraTarget {
            device_id: "cam-1".to_string(),
            display_name: "Front Door".to_string(),
            status: DeviceStatus::Online,
            room_name: Some("Entry".to_string()),
            vendor: Some("DemoCam".to_string()),
            model: Some("C1".to_string()),
            ip_address: Some("192.168.1.10".to_string()),
            mac_address: None,
            discovery_source: "manual_entry".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: "rtsp://192.168.1.10/live".to_string(),
                requires_auth: false,
            },
            snapshot_url: None,
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities {
                snapshot: true,
                stream: true,
                ptz: false,
                audio: false,
            },
            last_seen_at: None,
        };
        let bytes = b"fake-jpeg";
        let snapshot = SnapshotCaptureResult::new(
            "cam-1",
            SnapshotFormat::Jpeg,
            base64::engine::general_purpose::STANDARD.encode(bytes),
            bytes.len(),
            StorageTarget::LocalDisk,
        );
        let expected_captured_at = snapshot.captured_at_epoch_ms.to_string();

        let media_asset = super::build_snapshot_media_asset(&request, &target, &snapshot);
        assert!(media_asset.asset_id.starts_with("asset-"));
        assert_eq!(
            media_asset.workspace_id,
            super::workspace_id_for_request(&request)
        );
        assert_eq!(media_asset.device_id.as_deref(), Some("cam-1"));
        assert_eq!(media_asset.asset_kind, MediaAssetKind::Snapshot);
        assert_eq!(media_asset.storage_target, StorageTargetKind::LocalDisk);
        assert_eq!(media_asset.storage_uri, snapshot.storage.relative_path);
        assert_eq!(media_asset.mime_type, "image/jpeg");
        assert_eq!(media_asset.byte_size, bytes.len() as u64);
        assert_eq!(
            media_asset.captured_at.as_deref(),
            Some(expected_captured_at.as_str())
        );
        assert!(media_asset
            .checksum
            .as_deref()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert_eq!(
            media_asset
                .metadata
                .pointer("/task_id")
                .and_then(Value::as_str),
            Some("task-snapshot")
        );
        assert_eq!(
            media_asset
                .metadata
                .pointer("/device_ingest_metadata/provenance")
                .and_then(Value::as_str),
            Some("media")
        );
        assert_eq!(
            media_asset
                .metadata
                .pointer("/device_ingest_metadata/ingest_disposition")
                .and_then(Value::as_str),
            Some("knowledge_index_candidate")
        );

        let payload = super::build_snapshot_payload(&target, &snapshot, &media_asset);
        assert_eq!(
            payload
                .pointer("/snapshot/media_asset_id")
                .and_then(Value::as_str),
            Some(media_asset.asset_id.as_str())
        );

        let artifact = super::build_snapshot_artifact(&snapshot, &media_asset);
        assert_eq!(
            artifact.media_asset_id.as_deref(),
            Some(media_asset.asset_id.as_str())
        );
        assert_eq!(
            artifact
                .metadata
                .pointer("/media_asset_id")
                .and_then(Value::as_str),
            Some(media_asset.asset_id.as_str())
        );

        let records = build_artifact_records(&request, "step-snapshot", &[artifact]);
        assert_eq!(
            records[0].media_asset_id.as_deref(),
            Some(media_asset.asset_id.as_str())
        );
    }

    #[test]
    fn persist_vision_media_assets_creates_snapshot_and_derived_records() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let snapshot_path = unique_path("harborbeacon-vision-snapshot").with_extension("jpg");
        let annotated_path = unique_path("harborbeacon-vision-annotated").with_extension("jpg");
        fs::write(&snapshot_path, b"snapshot-bytes").expect("write snapshot image");
        fs::write(&annotated_path, b"annotated-bytes").expect("write annotated image");

        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let request = TaskRequest {
            task_id: "task-vision".to_string(),
            trace_id: "trace-vision".to_string(),
            step_id: "step-vision".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_vision".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "分析门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_vision".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        let target = ResolvedCameraTarget {
            device_id: "cam-1".to_string(),
            display_name: "Front Door".to_string(),
            status: DeviceStatus::Online,
            room_name: Some("Entry".to_string()),
            vendor: None,
            model: None,
            ip_address: Some("192.168.1.10".to_string()),
            mac_address: None,
            discovery_source: "manual_entry".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: "rtsp://192.168.1.10/live".to_string(),
                requires_auth: false,
            },
            snapshot_url: None,
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities {
                snapshot: true,
                stream: true,
                ptz: false,
                audio: false,
            },
            last_seen_at: None,
        };
        let mut payload = json!({
            "summary": "检测到门口有人活动",
            "summary_source": "heuristic_fallback",
            "detection_summary": "检测到 1 个 person",
            "snapshot": {
                "image_path": snapshot_path.to_string_lossy().to_string(),
                "annotated_image_path": annotated_path.to_string_lossy().to_string(),
                "mime_type": "image/jpeg",
                "source_storage": {
                    "target": "local_disk",
                    "relative_path": "snapshots/cam-1/1710000000000.jpg"
                },
                "byte_size": 14,
                "captured_at_epoch_ms": 1710000000000u64
            }
        });

        service
            .persist_vision_media_assets(&request, &target, &mut payload)
            .expect("persist vision media assets");

        let snapshot_asset_id = payload
            .pointer("/snapshot/media_asset_id")
            .and_then(Value::as_str)
            .expect("snapshot media asset id");
        let annotated_asset_id = payload
            .pointer("/snapshot/annotated_media_asset_id")
            .and_then(Value::as_str)
            .expect("annotated media asset id");

        let snapshot_asset = service
            .conversation_store()
            .load_media_asset(snapshot_asset_id)
            .expect("load snapshot media asset")
            .expect("snapshot media asset");
        let annotated_asset = service
            .conversation_store()
            .load_media_asset(annotated_asset_id)
            .expect("load annotated media asset")
            .expect("annotated media asset");

        assert_eq!(snapshot_asset.asset_kind, MediaAssetKind::Snapshot);
        assert_eq!(snapshot_asset.storage_target, StorageTargetKind::LocalDisk);
        assert_eq!(snapshot_asset.byte_size, 14);
        assert_eq!(snapshot_asset.captured_at.as_deref(), Some("1710000000000"));
        assert!(snapshot_asset
            .checksum
            .as_deref()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert_eq!(
            snapshot_asset
                .metadata
                .pointer("/source_storage/relative_path")
                .and_then(Value::as_str),
            Some("snapshots/cam-1/1710000000000.jpg")
        );

        assert_eq!(annotated_asset.asset_kind, MediaAssetKind::Derived);
        assert_eq!(
            annotated_asset.derived_from_asset_id.as_deref(),
            Some(snapshot_asset_id)
        );
        assert_eq!(
            annotated_asset.captured_at.as_deref(),
            Some("1710000000000")
        );
        assert_eq!(
            annotated_asset
                .metadata
                .pointer("/artifact_role")
                .and_then(Value::as_str),
            Some("analysis_annotation")
        );

        let artifacts = super::build_vision_artifacts(&payload);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(
            artifacts[0].media_asset_id.as_deref(),
            Some(snapshot_asset_id)
        );
        assert_eq!(
            artifacts[1].media_asset_id.as_deref(),
            Some(annotated_asset_id)
        );

        let _ = fs::remove_file(snapshot_path);
        let _ = fs::remove_file(annotated_path);
        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn build_notification_request_prefers_route_key_contract_shape() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path);
        let service = TaskApiService::new(admin_store, conversation_store);
        let request = TaskRequest {
            task_id: "task-vision".to_string(),
            trace_id: "trace-vision".to_string(),
            step_id: "step-vision".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_notify".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "分析门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_notify".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        let target = ResolvedCameraTarget {
            device_id: "cam-1".to_string(),
            display_name: "Front Door".to_string(),
            status: DeviceStatus::Online,
            room_name: Some("Entry".to_string()),
            vendor: None,
            model: None,
            ip_address: Some("192.168.1.10".to_string()),
            mac_address: None,
            discovery_source: "onvif".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: "rtsp://192.168.1.10/live".to_string(),
                requires_auth: false,
            },
            snapshot_url: None,
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities {
                snapshot: true,
                stream: true,
                ptz: false,
                audio: false,
            },
            last_seen_at: None,
        };
        let notification = service
            .build_notification_request(
                &request,
                "task.completed",
                &target,
                &json!({
                    "summary": "检测到门口有人活动",
                    "notification_channel": "im_bridge",
                    "notification_format": "lark_card",
                    "notification/destination/recipient/recipient_id": "ou_platform_should_not_be_needed",
                    "notification/destination/recipient/recipient_type": "open_id",
                    "notification_card": {
                        "header": {"title": {"content": "Front Door AI 分析"}}
                    }
                }),
                &[TaskArtifact {
                    kind: "image".to_string(),
                    label: "抓拍图片".to_string(),
                    mime_type: "image/jpeg".to_string(),
                    media_asset_id: None,
                    path: Some("snap.jpg".to_string()),
                    url: None,
                    metadata: Value::Null,
                }],
            )
            .expect("notification request");
        let replay_notification = service
            .build_notification_request(
                &request,
                "task.completed",
                &target,
                &json!({
                    "summary": "检测到门口有人活动",
                    "notification_channel": "im_bridge",
                    "notification_format": "lark_card",
                    "notification_card": {
                        "header": {"title": {"content": "Front Door AI 分析"}}
                    }
                }),
                &[TaskArtifact {
                    kind: "image".to_string(),
                    label: "抓拍图片".to_string(),
                    mime_type: "image/jpeg".to_string(),
                    media_asset_id: None,
                    path: Some("snap.jpg".to_string()),
                    url: None,
                    metadata: Value::Null,
                }],
            )
            .expect("replayed notification request");

        assert_eq!(
            notification.content.payload_format,
            NotificationPayloadFormat::LarkCard
        );
        assert_eq!(
            notification.destination.kind,
            NotificationDestinationKind::Conversation
        );
        assert_eq!(notification.destination.route_key, "gw_route_notify");
        assert_eq!(notification.destination.platform, "");
        assert!(notification.destination.recipient.is_none());
        assert_eq!(notification.content.attachments.len(), 1);
        assert_eq!(notification.content.title, "Front Door AI 分析");
        assert_eq!(notification.source.service, "harborbeacon");
        assert_eq!(notification.source.module, "task_api");
        assert_eq!(notification.source.event_type, "task.completed");
        assert_eq!(notification.delivery.mode, NotificationDeliveryMode::Send);
        assert!(notification.notification_id.starts_with("notif_"));
        assert!(notification.delivery.idempotency_key.starts_with("idem_"));
        assert_eq!(
            notification.notification_id,
            replay_notification.notification_id
        );
        assert_eq!(
            notification.delivery.idempotency_key,
            replay_notification.delivery.idempotency_key
        );

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn build_notification_request_ignores_legacy_recipient_hints_when_route_key_exists() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path);
        let service = TaskApiService::new(admin_store, conversation_store);
        let request = TaskRequest {
            task_id: "task-route-opaque".to_string(),
            trace_id: "trace-route-opaque".to_string(),
            step_id: "step-route-opaque".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-opaque".to_string(),
                user_id: "user-opaque".to_string(),
                session_id: "sess-opaque".to_string(),
                route_key: "gw_route_opaque".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "分析门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "notification/destination/recipient/recipient_id": "ou_should_be_ignored",
                "notification/destination/recipient/recipient_type": "open_id",
                "notification_channel": "im_bridge",
            }),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_route_opaque".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        let target = ResolvedCameraTarget {
            device_id: "cam-opaque".to_string(),
            display_name: "Front Door".to_string(),
            status: DeviceStatus::Online,
            room_name: Some("Entry".to_string()),
            vendor: None,
            model: None,
            ip_address: Some("192.168.1.10".to_string()),
            mac_address: None,
            discovery_source: "onvif".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: "rtsp://192.168.1.10/live".to_string(),
                requires_auth: false,
            },
            snapshot_url: None,
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities {
                snapshot: true,
                stream: true,
                ptz: false,
                audio: false,
            },
            last_seen_at: None,
        };

        let notification = service
            .build_notification_request(
                &request,
                "task.completed",
                &target,
                &json!({
                    "summary": "检测到门口有人活动",
                    "notification_channel": "im_bridge",
                }),
                &[],
            )
            .expect("notification request");

        assert_eq!(
            notification.destination.kind,
            NotificationDestinationKind::Conversation
        );
        assert_eq!(notification.destination.route_key, "gw_route_opaque");
        assert!(notification.destination.recipient.is_none());
        assert_eq!(notification.destination.platform, "");
        assert!(notification.destination.id.is_empty());

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn build_notification_request_retires_legacy_platform_fallback_without_route_key() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path);
        let service = TaskApiService::new(admin_store, conversation_store);
        let request = TaskRequest {
            task_id: "task-legacy-fallback".to_string(),
            trace_id: "trace-legacy-fallback".to_string(),
            step_id: "step-legacy-fallback".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-legacy".to_string(),
                user_id: "user-legacy".to_string(),
                session_id: "sess-legacy".to_string(),
                route_key: String::new(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "分析门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "notification/destination/recipient/recipient_id": "ou_legacy_should_not_send",
                "notification/destination/recipient/recipient_type": "open_id",
                "notification_channel": "im_bridge",
            }),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_legacy".to_string(),
                chat_type: "group".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        let target = ResolvedCameraTarget {
            device_id: "cam-legacy".to_string(),
            display_name: "Front Door".to_string(),
            status: DeviceStatus::Online,
            room_name: Some("Entry".to_string()),
            vendor: None,
            model: None,
            ip_address: Some("192.168.1.10".to_string()),
            mac_address: None,
            discovery_source: "onvif".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: "rtsp://192.168.1.10/live".to_string(),
                requires_auth: false,
            },
            snapshot_url: None,
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities {
                snapshot: true,
                stream: true,
                ptz: false,
                audio: false,
            },
            last_seen_at: None,
        };

        assert!(service
            .build_notification_request(
                &request,
                "task.completed",
                &target,
                &json!({
                    "summary": "检测到门口有人活动",
                    "notification_channel": "im_bridge",
                }),
                &[],
            )
            .is_none());

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn general_message_search_like_queries_are_interpreted() {
        let _guard = RETRIEVAL_GATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let request = TaskRequest {
            intent: TaskIntent {
                raw_text: "帮我找到和樱花有关的文件".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(should_route_general_message_to_knowledge(&request));
    }

    #[test]
    fn env_flag_enabled_accepts_common_truthy_strings() {
        assert!(env_flag_enabled("1"));
        assert!(env_flag_enabled("true"));
        assert!(env_flag_enabled("YES"));
        assert!(env_flag_enabled(" on "));
        assert!(!env_flag_enabled("0"));
        assert!(!env_flag_enabled("false"));
        assert!(!env_flag_enabled(""));
    }

    #[test]
    fn ensure_safe_capture_root_allows_explicit_non_harboros_root_with_guard() {
        let original = std::env::var(ALLOW_NON_HARBOROS_CAPTURE_ROOT_ENV).ok();
        let allowed_root = if cfg!(windows) {
            Path::new("C:/tmp/harborbeacon-agent-ci")
        } else {
            Path::new("/home/harbor/work/.tmp-live/harborbeacon-agent-ci")
        };
        unsafe {
            std::env::set_var(ALLOW_NON_HARBOROS_CAPTURE_ROOT_ENV, "1");
        }

        let result = ensure_safe_capture_root(allowed_root);

        match original {
            Some(value) => unsafe {
                std::env::set_var(ALLOW_NON_HARBOROS_CAPTURE_ROOT_ENV, value);
            },
            None => unsafe {
                std::env::remove_var(ALLOW_NON_HARBOROS_CAPTURE_ROOT_ENV);
            },
        }

        assert!(result.is_ok());
    }

    #[test]
    fn handle_camera_connect_resume_token_routes_into_resume_flow_without_platform_identity() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());

        let session = ConversationSession {
            session_id: "sess-resume".to_string(),
            workspace_id: "home-1".to_string(),
            channel: "feishu".to_string(),
            surface: "harborbeacon".to_string(),
            conversation_id: "chat-resume".to_string(),
            user_id: "user-1".to_string(),
            route_key: "gw_route_resume_opaque".to_string(),
            last_message_id: "om_resume".to_string(),
            chat_type: "p2p".to_string(),
            state: Value::Null,
            resume_token: None,
            expires_at: None,
        };
        let mut conversation = TaskConversationState {
            key: "chat-resume".to_string(),
            ..Default::default()
        };
        conversation.set_camera_pending_connect(Some(PendingTaskConnect {
            resume_token: "resume-opaque-1".to_string(),
            name: "Gate Cam".to_string(),
            ip: "192.168.1.20".to_string(),
            room: Some("Entry".to_string()),
            port: 554,
            snapshot_url: Some("http://192.168.1.20/snapshot.jpg".to_string()),
            rtsp_paths: vec!["/live".to_string()],
            requires_auth: true,
            vendor: Some("Demo".to_string()),
            model: Some("X1".to_string()),
        }));
        conversation_store
            .save_for_session(&session, &conversation)
            .expect("save conversation");

        let request = TaskRequest {
            task_id: "task-resume-opaque".to_string(),
            trace_id: "trace-resume-opaque".to_string(),
            step_id: "step-resume-opaque".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-resume".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-resume".to_string(),
                route_key: "gw_route_resume_opaque".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "密码 xxxxxx".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "resume_token": "resume-opaque-1",
                "approval": {
                    "token": "approval-opaque-1",
                    "approver_id": "user-1"
                }
            }),
            autonomy: super::TaskAutonomy {
                level: "full".to_string(),
            },
            message: Some(TaskMessage {
                message_id: "om_resume_followup".to_string(),
                chat_type: "p2p".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Failed);
        assert_eq!(response.result.message, "缺少 password，无法继续接入流程。");

        let loaded = conversation_store
            .load_for_session("sess-resume", Some("chat-resume"))
            .expect("load conversation")
            .expect("conversation");
        assert_eq!(
            loaded
                .camera_pending_connect()
                .map(|pending| pending.resume_token),
            Some("resume-opaque-1".to_string())
        );
        assert_eq!(loaded.key, "chat-resume");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn handle_camera_share_link_returns_link_artifact() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store.clone());
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store);

        let mut device = CameraDevice::new("cam-share", "Front Door", "rtsp://192.168.1.10/live");
        device.status = DeviceStatus::Online;
        device.room = Some("Entry".to_string());
        device.discovery_source = "manual_entry".to_string();
        device.capabilities.snapshot = true;
        device.capabilities.stream = true;
        registry_store
            .save_devices(&[device])
            .expect("save registry device");
        service
            .clone()
            .admin_store
            .save_remote_view_config(RemoteViewConfig {
                share_secret: "platform-share-secret".to_string(),
                share_link_ttl_minutes: 45,
            })
            .expect("save remote view config");

        let response = service.handle_task(TaskRequest {
            task_id: "task-share".to_string(),
            trace_id: "trace-share".to_string(),
            step_id: "step-share".to_string(),
            source: TaskSource {
                channel: "admin_api".to_string(),
                surface: "agent_hub_admin_api".to_string(),
                conversation_id: "admin-console".to_string(),
                user_id: "local-admin".to_string(),
                session_id: "admin-console".to_string(),
                route_key: String::new(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "share_link".to_string(),
                raw_text: "生成共享观看链接".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "device_id": "cam-share",
            }),
            autonomy: Default::default(),
            message: None,
        });

        assert_eq!(response.status, TaskStatus::Completed);
        assert_eq!(
            response.risk_level,
            crate::orchestrator::contracts::RiskLevel::Medium
        );
        let share_link_id = response.result.data["share_link"]["share_link_id"]
            .as_str()
            .expect("share link id");
        let media_session_id = response.result.data["share_link"]["media_session_id"]
            .as_str()
            .expect("media session id");
        assert_eq!(response.result.artifacts.len(), 1);
        assert_eq!(response.result.artifacts[0].kind, "link");
        assert!(response.result.artifacts[0]
            .url
            .as_deref()
            .expect("share url")
            .starts_with("/shared/cameras/"));
        assert_eq!(response.result.data["share_link"]["ttl_minutes"], 45);
        assert_eq!(
            response.result.artifacts[0].metadata["share_link_id"],
            json!(share_link_id)
        );
        assert_eq!(
            response.result.events[0]["event_type"],
            "task.share_link_issued"
        );
        let share_url = response.result.artifacts[0]
            .url
            .as_deref()
            .expect("share url");
        let share_token = share_url.trim_start_matches("/shared/cameras/");
        let share_link = service
            .conversation_store()
            .load_share_link(share_link_id)
            .expect("load share link")
            .expect("share link");
        let media_session = service
            .conversation_store()
            .load_media_session(media_session_id)
            .expect("load media session")
            .expect("media session");
        assert_eq!(
            share_link.token_hash,
            crate::runtime::remote_view::camera_share_token_hash(share_token)
        );
        assert_eq!(share_link.media_session_id, media_session.media_session_id);
        assert_eq!(media_session.device_id, "cam-share");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn handle_camera_live_view_alias_returns_link_artifact() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store.clone());
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store);

        let mut device = CameraDevice::new("cam-share", "Front Door", "rtsp://192.168.1.10/live");
        device.status = DeviceStatus::Online;
        device.room = Some("Entry".to_string());
        device.discovery_source = "manual_entry".to_string();
        device.capabilities.snapshot = true;
        device.capabilities.stream = true;
        registry_store
            .save_devices(&[device])
            .expect("save registry device");
        service
            .clone()
            .admin_store
            .save_remote_view_config(RemoteViewConfig {
                share_secret: "platform-share-secret".to_string(),
                share_link_ttl_minutes: 45,
            })
            .expect("save remote view config");

        let response = service.handle_task(TaskRequest {
            task_id: "task-live-view".to_string(),
            trace_id: "trace-live-view".to_string(),
            step_id: "step-live-view".to_string(),
            source: TaskSource {
                channel: "admin_api".to_string(),
                surface: "agent_hub_admin_api".to_string(),
                conversation_id: "admin-console".to_string(),
                user_id: "local-admin".to_string(),
                session_id: "admin-session".to_string(),
                route_key: String::new(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "live_view".to_string(),
                raw_text: "生成共享观看链接".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "device_id": "cam-share"
            }),
            autonomy: Default::default(),
            message: None,
        });

        assert_eq!(response.status, TaskStatus::Completed);
        assert_eq!(response.executor_used, "camera_hub_service");
        assert_eq!(
            response.result.data["camera_target"]["device_id"],
            "cam-share"
        );
        assert_eq!(response.result.data["share_link"]["device_id"], "cam-share");
        assert_eq!(response.result.artifacts.len(), 1);
        assert_eq!(response.result.artifacts[0].kind, "link");
        assert_eq!(
            response.result.events[0]["event_type"],
            "task.share_link_issued"
        );

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn resolve_notification_recipient_prefers_bound_chat_id() {
        let state = AdminConsoleState {
            identity_bindings: vec![IdentityBindingRecord {
                open_id: "ou_demo".to_string(),
                user_id: Some("user-1".to_string()),
                union_id: None,
                display_name: "家庭通知频道".to_string(),
                chat_id: Some("oc_demo".to_string()),
            }],
            ..Default::default()
        };
        let recipient =
            resolve_notification_recipient("家庭通知频道", &state, "user-1").expect("recipient");

        assert_eq!(
            recipient.recipient_type,
            NotificationRecipientIdType::ChatId
        );
        assert_eq!(recipient.recipient_id, "oc_demo");
    }

    #[test]
    fn resolve_notification_recipient_prefers_platform_binding_when_legacy_empty() {
        let mut state = AdminConsoleState::default();
        state.platform.identity_bindings.push(IdentityBinding {
            identity_id: "identity-ou_platform".to_string(),
            user_id: "user-1".to_string(),
            auth_source: AuthSource::ImChannel,
            provider_key: "im_bridge".to_string(),
            external_user_id: "ou_platform".to_string(),
            external_union_id: None,
            external_chat_id: Some("oc_platform".to_string()),
            profile_snapshot: json!({
                "display_name": "平台通知频道",
            }),
            last_seen_at: None,
        });

        let recipient =
            resolve_notification_recipient("平台通知频道", &state, "user-1").expect("recipient");

        assert_eq!(
            recipient.recipient_type,
            NotificationRecipientIdType::ChatId
        );
        assert_eq!(recipient.recipient_id, "oc_platform");
    }

    #[test]
    fn build_notification_request_uses_member_default_surface_for_proactive_delivery() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path);
        let service = TaskApiService::new(admin_store.clone(), conversation_store);
        admin_store
            .upsert_notification_target(
                None,
                "我的微信",
                "gw_route_weixin_default",
                "weixin",
                true,
            )
            .expect("save notification target");

        let request = TaskRequest {
            task_id: "task-proactive".to_string(),
            trace_id: "trace-proactive".to_string(),
            step_id: "step-proactive".to_string(),
            source: TaskSource {
                channel: "admin_api".to_string(),
                surface: "harbordesk".to_string(),
                conversation_id: String::new(),
                user_id: "user-weixin".to_string(),
                session_id: "sess-proactive".to_string(),
                route_key: String::new(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "后台分析告警".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };
        let target = ResolvedCameraTarget {
            device_id: "cam-proactive".to_string(),
            display_name: "Front Door".to_string(),
            status: DeviceStatus::Online,
            room_name: Some("Entry".to_string()),
            vendor: None,
            model: None,
            ip_address: Some("192.168.1.10".to_string()),
            mac_address: None,
            discovery_source: "onvif".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: "rtsp://192.168.1.10/live".to_string(),
                requires_auth: false,
            },
            snapshot_url: None,
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities {
                snapshot: true,
                stream: true,
                ptz: false,
                audio: false,
            },
            last_seen_at: None,
        };

        let notification = service
            .build_notification_request(
                &request,
                "task.completed",
                &target,
                &json!({
                    "summary": "独立系统提醒",
                    "notification_channel": "im_bridge",
                }),
                &[],
            )
            .expect("proactive notification");

        assert_eq!(
            notification.destination.kind,
            NotificationDestinationKind::Conversation
        );
        assert_eq!(notification.destination.platform, "weixin");
        assert_eq!(notification.destination.route_key, "gw_route_weixin_default");
        assert!(notification.destination.id.is_empty());
        assert!(notification.destination.recipient.is_none());

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn notification_delivery_outcome_marks_rejected_requests() {
        let request = NotificationRequest {
            notification_id: "notif_01JABC".to_string(),
            trace_id: "trace_01JABC".to_string(),
            source: NotificationSource {
                service: "harborbeacon".to_string(),
                module: "task_api".to_string(),
                event_type: "task.completed".to_string(),
            },
            destination: NotificationDestination {
                kind: NotificationDestinationKind::Conversation,
                route_key: "gw_route_notify_fail".to_string(),
                id: String::new(),
                platform: String::new(),
                recipient: None,
            },
            content: crate::connectors::notifications::NotificationContent {
                title: "AI 分析".to_string(),
                body: "检测到人员活动".to_string(),
                payload_format: NotificationPayloadFormat::PlainText,
                structured_payload: Value::Null,
                attachments: Vec::new(),
            },
            delivery: NotificationDelivery {
                mode: NotificationDeliveryMode::Send,
                reply_to_message_id: String::new(),
                update_message_id: String::new(),
                idempotency_key: "idem_01JABC".to_string(),
            },
            metadata: NotificationMetadata {
                correlation_id: "trace_01JABC".to_string(),
            },
        };
        let outcome = notification_delivery_outcome(
            &request,
            Err(NotificationDeliveryError::RequestRejected {
                status_code: 404,
                envelope: SharedHttpErrorEnvelope {
                    ok: false,
                    error: SharedHttpErrorDetail {
                        code: "ROUTE_NOT_FOUND".to_string(),
                        message: "route expired".to_string(),
                    },
                    trace_id: Some("trace_01JABC".to_string()),
                },
            }),
        );

        assert_eq!(outcome.event_type, "task.notification_rejected");
        assert_eq!(outcome.payload["status"], "rejected");
        assert_eq!(outcome.payload["http_status"], 404);
        assert_eq!(outcome.payload["error"]["code"], "ROUTE_NOT_FOUND");
    }

    #[test]
    fn proactive_notification_delivery_outcome_uses_proactive_failure_event() {
        let request = NotificationRequest {
            notification_id: "notif_proactive".to_string(),
            trace_id: "trace_proactive".to_string(),
            source: NotificationSource {
                service: "harborbeacon".to_string(),
                module: "task_api".to_string(),
                event_type: "task.completed".to_string(),
            },
            destination: NotificationDestination {
                kind: NotificationDestinationKind::Conversation,
                route_key: "gw_route_weixin_default".to_string(),
                id: String::new(),
                platform: "weixin".to_string(),
                recipient: None,
            },
            content: NotificationContent {
                title: "系统提醒".to_string(),
                body: "请检查状态".to_string(),
                payload_format: NotificationPayloadFormat::PlainText,
                structured_payload: Value::Null,
                attachments: Vec::new(),
            },
            delivery: NotificationDelivery {
                mode: NotificationDeliveryMode::Send,
                reply_to_message_id: String::new(),
                update_message_id: String::new(),
                idempotency_key: "idem_proactive".to_string(),
            },
            metadata: NotificationMetadata {
                correlation_id: "trace_proactive".to_string(),
            },
        };

        let outcome = notification_delivery_outcome(
            &request,
            Err(NotificationDeliveryError::Transport(
                "context token missing".to_string(),
            )),
        );

        assert_eq!(outcome.event_type, "task.proactive_delivery_failed");
        assert_eq!(outcome.payload["route_mode"], "proactive");
        assert_eq!(outcome.payload["destination"]["platform"], "weixin");
    }

    #[test]
    fn effective_requires_approval_defaults_camera_connect_only() {
        let connect_request = TaskRequest {
            task_id: "task-connect".to_string(),
            trace_id: "trace-connect".to_string(),
            step_id: "step-connect".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: String::new(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };
        let scan_request = TaskRequest {
            task_id: "task-scan".to_string(),
            trace_id: "trace-scan".to_string(),
            step_id: "step-scan".to_string(),
            source: connect_request.source.clone(),
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "scan".to_string(),
                raw_text: "扫描摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };

        assert!(effective_requires_approval(&connect_request));
        assert!(!effective_requires_approval(&scan_request));
    }

    #[test]
    fn effective_autonomy_defaults_to_supervised_and_normalizes_aliases() {
        let default_request = TaskRequest {
            task_id: "task-autonomy-default".to_string(),
            trace_id: "trace-autonomy-default".to_string(),
            step_id: "step-autonomy-default".to_string(),
            source: TaskSource::default(),
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "scan".to_string(),
                raw_text: "扫描摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };
        let readonly_request = TaskRequest {
            task_id: "task-autonomy-readonly".to_string(),
            trace_id: "trace-autonomy-readonly".to_string(),
            step_id: "step-autonomy-readonly".to_string(),
            source: TaskSource::default(),
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: super::TaskAutonomy {
                level: "ReadOnly".to_string(),
            },
            message: None,
        };

        assert_eq!(
            format!("{:?}", effective_autonomy_level(&default_request)),
            "Supervised"
        );
        assert_eq!(
            effective_autonomy_level_for_task_run(&default_request),
            "supervised"
        );
        assert_eq!(
            format!("{:?}", effective_autonomy_level(&readonly_request)),
            "ReadOnly"
        );
        assert_eq!(
            effective_autonomy_level_for_task_run(&readonly_request),
            "readonly"
        );
    }

    #[test]
    fn handle_camera_connect_blocks_by_default_until_approved() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        let request = TaskRequest {
            task_id: "task-connect-approval".to_string(),
            trace_id: "trace-connect-approval".to_string(),
            step_id: "step-connect-approval".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_connect_approval".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::NeedsInput);
        assert_eq!(response.missing_fields, vec!["approval_token".to_string()]);
        assert_eq!(
            response.result.data["approval_ticket"]["policy_ref"],
            "camera.connect"
        );

        let task_run = conversation_store
            .load_task_run("task-connect-approval")
            .expect("load task run")
            .expect("task run");
        assert_eq!(task_run.status, TaskRunStatus::NeedsInput);
        assert!(task_run.requires_approval);

        let approvals = conversation_store
            .approvals_for_task("task-connect-approval")
            .expect("load approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].policy_ref, "camera.connect");
        assert_eq!(approvals[0].status, ApprovalStatus::Pending);

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn handle_camera_connect_fails_under_readonly_autonomy_before_approval() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        let request = TaskRequest {
            task_id: "task-connect-readonly".to_string(),
            trace_id: "trace-connect-readonly".to_string(),
            step_id: "step-connect-readonly".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_connect_readonly".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: super::TaskAutonomy {
                level: "ReadOnly".to_string(),
            },
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Failed);
        assert_eq!(response.result.data["error"], "AUTONOMY_BLOCKED");
        assert_eq!(response.result.data["autonomy_level"], "readonly");

        let task_run = conversation_store
            .load_task_run("task-connect-readonly")
            .expect("load task run")
            .expect("task run");
        assert_eq!(task_run.status, TaskRunStatus::Failed);
        assert_eq!(task_run.autonomy_level, "readonly");

        let approvals = conversation_store
            .approvals_for_task("task-connect-readonly")
            .expect("load approvals");
        assert!(approvals.is_empty());

        let events = conversation_store
            .events_for_task("task-connect-readonly")
            .expect("load events");
        assert!(events
            .iter()
            .any(|event| event.event_type == "task.autonomy_blocked"));
        assert!(events.iter().any(|event| event.event_type == "task.failed"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn handle_camera_connect_with_full_autonomy_and_token_skips_approval_prompt() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        let request = TaskRequest {
            task_id: "task-connect-full".to_string(),
            trace_id: "trace-connect-full".to_string(),
            step_id: "step-connect-full".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_connect_full".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "approval": {
                    "token": "approved-token",
                    "approver_id": "user-1"
                }
            }),
            autonomy: super::TaskAutonomy {
                level: "full".to_string(),
            },
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Failed);
        assert_ne!(response.missing_fields, vec!["approval_token".to_string()]);
        assert!(
            response.result.message.contains("缺少摄像头 IP 地址"),
            "unexpected response: {}",
            response.result.message
        );

        let task_run = conversation_store
            .load_task_run("task-connect-full")
            .expect("load task run")
            .expect("task run");
        assert_eq!(task_run.status, TaskRunStatus::Failed);
        assert_eq!(task_run.autonomy_level, "full");
        assert!(task_run.requires_approval);

        let approvals = conversation_store
            .approvals_for_task("task-connect-full")
            .expect("load approvals");
        assert!(approvals.is_empty());

        let events = conversation_store
            .events_for_task("task-connect-full")
            .expect("load events");
        assert!(!events
            .iter()
            .any(|event| event.event_type == "task.approval_required"));
        assert!(events.iter().any(|event| event.event_type == "task.failed"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn approve_pending_approval_replays_task_request() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        let request = TaskRequest {
            task_id: "task-approve-replay".to_string(),
            trace_id: "trace-approve-replay".to_string(),
            step_id: "step-approve-replay".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_approve_replay".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };

        let initial = service.handle_task(request);
        let approval_id = initial.result.data["approval_ticket"]["approval_id"]
            .as_str()
            .expect("approval id")
            .to_string();
        let pending = service.pending_approvals().expect("pending approvals");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].approval_ticket.approval_id, approval_id);

        let (approval, resumed) = service
            .approve_pending_approval(&approval_id, Some("approver-1".to_string()))
            .expect("approve");

        assert_eq!(approval.approval_ticket.status, ApprovalStatus::Approved);
        assert_eq!(
            approval.approval_ticket.approver_user_id.as_deref(),
            Some("approver-1")
        );
        assert_eq!(resumed.status, TaskStatus::Failed);
        assert!(resumed.result.message.contains("缺少摄像头 IP 地址"));

        let approvals = conversation_store
            .approvals_for_task("task-approve-replay")
            .expect("load approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].status, ApprovalStatus::Approved);

        let events = conversation_store
            .events_for_task("task-approve-replay")
            .expect("load events");
        assert!(events
            .iter()
            .any(|event| event.event_type == "task.approval_approved"));
        assert!(events.iter().any(|event| event.event_type == "task.failed"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn reject_pending_approval_closes_task() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-reject");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        let request = TaskRequest {
            task_id: "task-reject-approval".to_string(),
            trace_id: "trace-reject-approval".to_string(),
            step_id: "step-reject-approval".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_reject_approval".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };

        let initial = service.handle_task(request);
        let approval_id = initial.result.data["approval_ticket"]["approval_id"]
            .as_str()
            .expect("approval id")
            .to_string();

        let approval = service
            .reject_pending_approval(&approval_id, Some("approver-2".to_string()))
            .expect("reject");

        assert_eq!(approval.approval_ticket.status, ApprovalStatus::Rejected);
        assert_eq!(
            approval.approval_ticket.approver_user_id.as_deref(),
            Some("approver-2")
        );

        let task_run = conversation_store
            .load_task_run("task-reject-approval")
            .expect("load task run")
            .expect("task run");
        assert_eq!(task_run.status, TaskRunStatus::Failed);
        assert!(task_run.completed_at.is_some());

        let session = conversation_store
            .load_session("sess-1")
            .expect("load session")
            .expect("session");
        assert!(session.resume_token.is_none());

        let events = conversation_store
            .events_for_task("task-reject-approval")
            .expect("load events");
        assert!(events
            .iter()
            .any(|event| event.event_type == "task.approval_rejected"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn handle_task_blocks_when_approval_required_without_token() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        let request = TaskRequest {
            task_id: "task-approval".to_string(),
            trace_id: "trace-approval".to_string(),
            step_id: "step-approval".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_task_approval".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "scan".to_string(),
                raw_text: "扫描摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "approval": {
                    "required": true
                }
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::NeedsInput);
        assert_eq!(response.missing_fields, vec!["approval_token".to_string()]);
        assert_eq!(
            response.result.data["approval_ticket"]["task_id"],
            "task-approval"
        );

        let task_run = conversation_store
            .load_task_run("task-approval")
            .expect("load task run")
            .expect("task run");
        assert_eq!(task_run.status, TaskRunStatus::NeedsInput);
        assert!(task_run.requires_approval);

        let approvals = conversation_store
            .approvals_for_task("task-approval")
            .expect("load approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].status, ApprovalStatus::Pending);
        assert_eq!(approvals[0].trace_id, "trace-approval");
        assert_eq!(approvals[0].route_key, "gw_route_task_approval");

        let events = conversation_store
            .events_for_task("task-approval")
            .expect("load events");
        assert!(events
            .iter()
            .any(|event| event.event_type == "task.approval_required"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "task.needs_input"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn handle_task_persists_runtime_records_for_failures() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let service = TaskApiService::new(admin_store, conversation_store.clone());
        let request = TaskRequest {
            task_id: "task-unsupported".to_string(),
            trace_id: "trace-unsupported".to_string(),
            step_id: "step-unsupported".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
                route_key: "gw_route_unsupported".to_string(),
            },
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "ping".to_string(),
                raw_text: "测试一下".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Failed);
        let task_run = conversation_store
            .load_task_run("task-unsupported")
            .expect("load task run")
            .expect("task run");
        assert_eq!(task_run.status, TaskRunStatus::Failed);
        assert_eq!(task_run.session_id, "sess-1");
        assert_eq!(task_run.autonomy_level, "supervised");

        let task_step = conversation_store
            .load_task_step("step-unsupported")
            .expect("load task step")
            .expect("task step");
        assert_eq!(task_step.status, TaskStepRunStatus::Failed);
        assert_eq!(task_step.executor_used, "task_api");

        let session = conversation_store
            .load_session("sess-1")
            .expect("load session")
            .expect("session");
        assert_eq!(session.channel, "feishu");
        let events = conversation_store
            .events_for_task("task-unsupported")
            .expect("load events");
        assert!(events.iter().any(|event| event.event_type == "task.failed"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn handle_service_status_dispatches_to_harboros_router() {
        let _guard = HARBOROS_TASK_API_TEST_LOCK.lock().expect("lock");
        reset_harbor_task_api_env();
        let (service, conversation_store, admin_path, registry_path, conversation_path) =
            build_task_api_service("harbor-service-status");
        let request = TaskRequest {
            task_id: "task-service-status".to_string(),
            trace_id: "trace-service-status".to_string(),
            step_id: "step-service-status".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-service-status".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-service-status".to_string(),
                route_key: "gw_route_service_status".to_string(),
            },
            intent: TaskIntent {
                domain: "service".to_string(),
                action: "status".to_string(),
                raw_text: "查看 ssh 状态".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "resource": {
                    "service_name": "ssh"
                }
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Completed);
        assert_eq!(response.executor_used, "middleware_api");
        assert_eq!(response.result.data["route_fallback_used"], false);
        assert_eq!(response.result.data["preview"], true);

        let task_step = conversation_store
            .load_task_step("step-service-status")
            .expect("load task step")
            .expect("task step");
        assert_eq!(task_step.route, ExecutionRoute::MiddlewareApi);
        assert_eq!(task_step.executor_used, "middleware_api");
        assert_eq!(task_step.status, TaskStepRunStatus::Success);

        cleanup_task_api_service(admin_path, registry_path, conversation_path);
        reset_harbor_task_api_env();
    }

    #[test]
    fn handle_service_status_falls_back_to_midcli_when_middleware_fails() {
        let _guard = HARBOROS_TASK_API_TEST_LOCK.lock().expect("lock");
        reset_harbor_task_api_env();
        std::env::set_var("HARBOR_FORCE_MIDDLEWARE_ERROR", "1");
        let (service, conversation_store, admin_path, registry_path, conversation_path) =
            build_task_api_service("harbor-service-fallback");
        let request = TaskRequest {
            task_id: "task-service-fallback".to_string(),
            trace_id: "trace-service-fallback".to_string(),
            step_id: "step-service-fallback".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-service-fallback".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-service-fallback".to_string(),
                route_key: "gw_route_service_fallback".to_string(),
            },
            intent: TaskIntent {
                domain: "service".to_string(),
                action: "status".to_string(),
                raw_text: "查看 ssh 状态".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "service_name": "ssh"
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Completed);
        assert_eq!(response.executor_used, "midcli");
        assert_eq!(response.result.data["route_fallback_used"], true);

        let task_step = conversation_store
            .load_task_step("step-service-fallback")
            .expect("load task step")
            .expect("task step");
        assert_eq!(task_step.route, ExecutionRoute::Midcli);
        assert_eq!(task_step.executor_used, "midcli");

        cleanup_task_api_service(admin_path, registry_path, conversation_path);
        reset_harbor_task_api_env();
    }

    #[test]
    fn handle_service_restart_requires_approval_before_execution() {
        let _guard = HARBOROS_TASK_API_TEST_LOCK.lock().expect("lock");
        reset_harbor_task_api_env();
        let (service, conversation_store, admin_path, registry_path, conversation_path) =
            build_task_api_service("harbor-service-restart");
        let request = TaskRequest {
            task_id: "task-service-restart".to_string(),
            trace_id: "trace-service-restart".to_string(),
            step_id: "step-service-restart".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-service-restart".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-service-restart".to_string(),
                route_key: "gw_route_service_restart".to_string(),
            },
            intent: TaskIntent {
                domain: "service".to_string(),
                action: "restart".to_string(),
                raw_text: "重启 ssh".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "service_name": "ssh"
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::NeedsInput);
        assert_eq!(response.executor_used, "harboros_router");
        assert_eq!(response.missing_fields, vec!["approval_token".to_string()]);
        assert_eq!(
            response.result.data["approval_ticket"]["policy_ref"],
            "service.restart"
        );

        let approvals = conversation_store
            .approvals_for_task("task-service-restart")
            .expect("load approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].status, ApprovalStatus::Pending);

        cleanup_task_api_service(admin_path, registry_path, conversation_path);
        reset_harbor_task_api_env();
    }

    #[test]
    fn approve_pending_service_restart_executes_harboros_route() {
        let _guard = HARBOROS_TASK_API_TEST_LOCK.lock().expect("lock");
        reset_harbor_task_api_env();
        let (service, conversation_store, admin_path, registry_path, conversation_path) =
            build_task_api_service("harbor-service-restart-approve");
        let request = TaskRequest {
            task_id: "task-service-restart-approve".to_string(),
            trace_id: "trace-service-restart-approve".to_string(),
            step_id: "step-service-restart-approve".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-service-restart-approve".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-service-restart-approve".to_string(),
                route_key: "gw_route_service_restart_approve".to_string(),
            },
            intent: TaskIntent {
                domain: "service".to_string(),
                action: "restart".to_string(),
                raw_text: "重启 ssh".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "service_name": "ssh"
            }),
            autonomy: Default::default(),
            message: None,
        };

        let initial = service.handle_task(request);
        let approval_id = initial.result.data["approval_ticket"]["approval_id"]
            .as_str()
            .expect("approval id")
            .to_string();

        let (approval, resumed) = service
            .approve_pending_approval(&approval_id, Some("approver-1".to_string()))
            .expect("approve");

        assert_eq!(approval.approval_ticket.status, ApprovalStatus::Approved);
        assert_eq!(resumed.status, TaskStatus::Completed);
        assert_eq!(resumed.executor_used, "middleware_api");
        assert_eq!(resumed.result.data["route_fallback_used"], false);
        assert!(!resumed.audit_ref.is_empty());

        let resume_step_id = format!("approval:{approval_id}:resume");
        let task_step = conversation_store
            .load_task_step(&resume_step_id)
            .expect("load task step")
            .expect("task step");
        assert_eq!(task_step.route, ExecutionRoute::MiddlewareApi);
        assert_eq!(task_step.executor_used, "middleware_api");

        let events = conversation_store
            .events_for_task("task-service-restart-approve")
            .expect("load events");
        assert!(events
            .iter()
            .any(|event| event.event_type == "task.approval_approved"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "task.harboros_dispatched"));

        cleanup_task_api_service(admin_path, registry_path, conversation_path);
        reset_harbor_task_api_env();
    }

    #[test]
    fn handle_files_list_dispatches_to_harboros_router() {
        let _guard = HARBOROS_TASK_API_TEST_LOCK.lock().expect("lock");
        reset_harbor_task_api_env();
        let (service, conversation_store, admin_path, registry_path, conversation_path) =
            build_task_api_service("harbor-files-list");
        let request = TaskRequest {
            task_id: "task-files-list".to_string(),
            trace_id: "trace-files-list".to_string(),
            step_id: "step-files-list".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-files-list".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-files-list".to_string(),
                route_key: "gw_route_files_list".to_string(),
            },
            intent: TaskIntent {
                domain: "files".to_string(),
                action: "list".to_string(),
                raw_text: "列出 agent-ci".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "resource": {
                    "path": "/mnt/agent-ci"
                },
                "recursive": true
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Completed);
        assert_eq!(response.executor_used, "middleware_api");
        assert_eq!(response.result.data["route_fallback_used"], false);

        let task_step = conversation_store
            .load_task_step("step-files-list")
            .expect("load task step")
            .expect("task step");
        assert_eq!(task_step.route, ExecutionRoute::MiddlewareApi);
        assert_eq!(task_step.executor_used, "middleware_api");

        cleanup_task_api_service(admin_path, registry_path, conversation_path);
        reset_harbor_task_api_env();
    }

    #[test]
    fn handle_files_move_requires_approval_before_execution() {
        let _guard = HARBOROS_TASK_API_TEST_LOCK.lock().expect("lock");
        reset_harbor_task_api_env();
        let (service, conversation_store, admin_path, registry_path, conversation_path) =
            build_task_api_service("harbor-files-move");
        let request = TaskRequest {
            task_id: "task-files-move".to_string(),
            trace_id: "trace-files-move".to_string(),
            step_id: "step-files-move".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-files-move".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-files-move".to_string(),
                route_key: "gw_route_files_move".to_string(),
            },
            intent: TaskIntent {
                domain: "files".to_string(),
                action: "move".to_string(),
                raw_text: "移动文件".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "source": "/mnt/agent-ci/inbox.txt",
                "target": "/mnt/agent-ci/archive/inbox.txt"
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::NeedsInput);
        assert_eq!(response.executor_used, "harboros_router");
        assert_eq!(
            response.result.data["approval_ticket"]["policy_ref"],
            "files.move"
        );

        let approvals = conversation_store
            .approvals_for_task("task-files-move")
            .expect("load approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].status, ApprovalStatus::Pending);

        cleanup_task_api_service(admin_path, registry_path, conversation_path);
        reset_harbor_task_api_env();
    }

    #[test]
    fn handle_files_copy_denied_path_surfaces_router_failure_details() {
        let _guard = HARBOROS_TASK_API_TEST_LOCK.lock().expect("lock");
        reset_harbor_task_api_env();
        let (service, conversation_store, admin_path, registry_path, conversation_path) =
            build_task_api_service("harbor-files-copy-denied");
        let request = TaskRequest {
            task_id: "task-files-copy-denied".to_string(),
            trace_id: "trace-files-copy-denied".to_string(),
            step_id: "step-files-copy-denied".to_string(),
            source: TaskSource {
                channel: "feishu".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-files-copy-denied".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-files-copy-denied".to_string(),
                route_key: "gw_route_files_copy_denied".to_string(),
            },
            intent: TaskIntent {
                domain: "files".to_string(),
                action: "copy".to_string(),
                raw_text: "复制 passwd".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "source": "/etc/passwd",
                "target": "/mnt/agent-ci/out.txt"
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Failed);
        assert_eq!(response.executor_used, "none");
        assert_eq!(response.result.data["error_code"], "NO_EXECUTOR_AVAILABLE");
        assert!(response.result.message.contains("denied path"));

        let task_step = conversation_store
            .load_task_step("step-files-copy-denied")
            .expect("load task step")
            .expect("task step");
        assert_eq!(task_step.route, ExecutionRoute::Local);
        assert_eq!(
            task_step.error_code.as_deref(),
            Some("NO_EXECUTOR_AVAILABLE")
        );
        assert!(task_step
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("denied path"));

        cleanup_task_api_service(admin_path, registry_path, conversation_path);
        reset_harbor_task_api_env();
    }

    #[test]
    fn handle_knowledge_search_returns_document_and_image_hits() {
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let knowledge_root = unique_dir("harborbeacon-knowledge-runtime");
        fs::create_dir_all(knowledge_root.join("docs")).expect("create docs");
        fs::create_dir_all(knowledge_root.join("images")).expect("create images");
        fs::write(
            knowledge_root.join("docs").join("sakura-notes.md"),
            "樱花季整理计划，记录花园图片和说明。",
        )
        .expect("write doc");
        fs::write(
            knowledge_root.join("images").join("spring-garden.jpg"),
            b"not-an-image",
        )
        .expect("write image");
        fs::write(
            knowledge_root.join("images").join("spring-garden.json"),
            r#"{"caption":"春天盛开的樱花树"}"#,
        )
        .expect("write sidecar");

        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let request = TaskRequest {
            task_id: "task-knowledge-search".to_string(),
            trace_id: "trace-knowledge-search".to_string(),
            step_id: "step-knowledge-search".to_string(),
            source: TaskSource::default(),
            intent: TaskIntent {
                domain: "knowledge".to_string(),
                action: "search".to_string(),
                raw_text: "搜索樱花文件".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "query": "樱花",
                "roots": [knowledge_root.to_string_lossy().to_string()]
            }),
            autonomy: Default::default(),
            message: None,
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Completed);
        assert_eq!(response.executor_used, "knowledge_search_service");
        assert_eq!(
            response.result.message,
            response.result.data["reply_pack"]["summary"]
        );
        assert_eq!(
            response.result.data["documents"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            response.result.data["images"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            response.result.data["reply_pack"]["citations"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            response.result.data["reply_pack"]["citations"][0]["title"],
            "sakura-notes.md"
        );
        assert!(
            response.result.data["reply_pack"]["citations"][0]["preview"]
                .as_str()
                .unwrap_or_default()
                .contains("樱花")
        );
        assert_eq!(response.result.artifacts.len(), 2);
        assert_eq!(response.result.artifacts[0].kind, "text");
        assert_eq!(response.result.artifacts[1].kind, "image");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
        let _ = fs::remove_dir_all(knowledge_root);
    }

    #[test]
    fn general_message_routes_retrieval_query_to_knowledge_search() {
        let _guard = RETRIEVAL_GATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let knowledge_root = unique_dir("harborbeacon-knowledge-general-message");
        fs::create_dir_all(knowledge_root.join("docs")).expect("create docs");
        fs::write(
            knowledge_root.join("docs").join("sakura-journal.md"),
            "我把樱花相关的文档放在这里，方便后续整理。",
        )
        .expect("write doc");

        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let request = TaskRequest {
            task_id: "task-general-message-search".to_string(),
            trace_id: "trace-general-message-search".to_string(),
            step_id: "step-general-message-search".to_string(),
            source: TaskSource {
                channel: "wechat".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-search".to_string(),
                user_id: "user-1".to_string(),
                session_id: "session-search".to_string(),
                route_key: "gw_route_search".to_string(),
            },
            intent: TaskIntent {
                domain: "general".to_string(),
                action: "message".to_string(),
                raw_text: "帮我找到和樱花有关的文件".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "roots": [knowledge_root.to_string_lossy().to_string()]
            }),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_knowledge_01".to_string(),
                chat_type: "p2p".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };

        let response = service.handle_task(request);

        assert_eq!(response.status, TaskStatus::Completed);
        assert_eq!(response.executor_used, "knowledge_search_service");
        assert_eq!(
            response.result.message,
            response.result.data["reply_pack"]["summary"]
        );
        assert_eq!(
            response.result.data["reply_pack"]["citations"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(response.result.artifacts.len(), 1);
        assert_eq!(
            response.result.artifacts[0].metadata["citation"]["title"],
            "sakura-journal.md"
        );
        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
        let _ = fs::remove_dir_all(knowledge_root);
    }

    #[test]
    fn retrieval_round_trip_launch_pack_covers_explicit_enabled_and_disabled_paths() {
        let _guard = RETRIEVAL_GATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let admin_path = unique_path("harborbeacon-admin-state");
        let registry_path = unique_path("harborbeacon-device-registry");
        let conversation_path = unique_path("harborbeacon-task-runtime");
        let knowledge_root = unique_dir("harborbeacon-knowledge-launch-pack");
        let index_root = unique_dir("harborbeacon-knowledge-index-launch-pack");
        fs::create_dir_all(knowledge_root.join("docs")).expect("create docs");
        fs::create_dir_all(knowledge_root.join("images")).expect("create images");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(
            knowledge_root.join("docs").join("sakura-notes.md"),
            "今年花园里的樱花开得很盛，适合做春季归档。",
        )
        .expect("write doc");
        fs::write(
            knowledge_root.join("images").join("spring-garden.jpg"),
            b"fake-image",
        )
        .expect("write image");
        fs::write(
            knowledge_root.join("images").join("spring-garden.json"),
            r#"{"caption":"春天盛开的樱花树","labels":["sakura","spring"]}"#,
        )
        .expect("write sidecar");

        std::env::set_var("HARBOR_KNOWLEDGE_INDEX_ROOT", &index_root);
        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );

        let explicit_request = TaskRequest {
            task_id: "task-launch-explicit".to_string(),
            trace_id: "trace-launch-explicit".to_string(),
            step_id: "step-launch-explicit".to_string(),
            source: TaskSource::default(),
            intent: TaskIntent {
                domain: "knowledge".to_string(),
                action: "search".to_string(),
                raw_text: "搜索樱花文件".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "query": "樱花",
                "roots": [knowledge_root.to_string_lossy().to_string()]
            }),
            autonomy: Default::default(),
            message: None,
        };
        let explicit_response = service.handle_task(explicit_request);
        assert_eq!(explicit_response.status, TaskStatus::Completed);
        assert_eq!(explicit_response.executor_used, "knowledge_search_service");
        assert_eq!(
            explicit_response.result.message,
            explicit_response.result.data["reply_pack"]["summary"]
        );
        assert_eq!(
            explicit_response.result.data["reply_pack"]["citations"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            explicit_response.result.data["reply_pack"]["citations"][0]["line_start"],
            1
        );
        assert_eq!(explicit_response.result.artifacts.len(), 2);
        assert_eq!(
            explicit_response.result.artifacts[0].metadata["citation"]["title"],
            "sakura-notes.md"
        );
        assert_eq!(
            explicit_response.result.artifacts[0].metadata["citation"]["line_start"],
            1
        );

        let general_message_request = TaskRequest {
            task_id: "task-launch-enabled".to_string(),
            trace_id: "trace-launch-enabled".to_string(),
            step_id: "step-launch-enabled".to_string(),
            source: TaskSource {
                channel: "wechat".to_string(),
                surface: "harborgate".to_string(),
                conversation_id: "chat-launch".to_string(),
                user_id: "user-1".to_string(),
                session_id: "session-launch".to_string(),
                route_key: "gw_route_launch".to_string(),
            },
            intent: TaskIntent {
                domain: "general".to_string(),
                action: "message".to_string(),
                raw_text: "帮我找到和樱花有关的文件".to_string(),
            },
            entity_refs: Value::Null,
            args: json!({
                "roots": [knowledge_root.to_string_lossy().to_string()]
            }),
            autonomy: Default::default(),
            message: Some(TaskMessage {
                message_id: "om_launch_01".to_string(),
                chat_type: "p2p".to_string(),
                mentions: Vec::new(),
                attachments: Vec::new(),
            }),
        };
        assert!(should_route_general_message_to_knowledge(
            &general_message_request
        ));
        let general_message_response = service.handle_task(general_message_request);
        assert_eq!(general_message_response.status, TaskStatus::Completed);
        assert_eq!(general_message_response.executor_used, "knowledge_search_service");
        assert_eq!(
            general_message_response.result.message,
            general_message_response.result.data["reply_pack"]["summary"]
        );
        assert_eq!(
            general_message_response.result.data["reply_pack"]["citations"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(general_message_response.result.artifacts.len(), 2);

        std::env::remove_var("HARBOR_KNOWLEDGE_INDEX_ROOT");
        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
        let _ = fs::remove_dir_all(knowledge_root);
        let _ = fs::remove_dir_all(index_root);
    }
}
