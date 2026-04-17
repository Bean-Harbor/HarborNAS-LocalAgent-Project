//! Minimal Assistant Task API service for HarborBeacon integration.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::connectors::notifications::{
    NotificationAttachment, NotificationAttachmentKind, NotificationBridgeConfig,
    NotificationChannel, NotificationDeliveryService, NotificationPayloadFormat,
    NotificationRecipient, NotificationRecipientIdType, NotificationRequest,
};
use crate::connectors::storage::StorageTarget;
use crate::control_plane::approvals::{ApprovalStatus, ApprovalTicket};
use crate::control_plane::events::{EventRecord, EventSeverity, EventSourceKind};
use crate::control_plane::media::{
    MediaAsset, MediaAssetKind, MediaDeliveryMode, MediaSession, MediaSessionKind,
    MediaSessionStatus, ShareAccessScope, ShareLink, StorageTargetKind,
};
use crate::control_plane::tasks::{
    ArtifactKind, ArtifactRecord, ConversationSession, ExecutionRoute, TaskRun, TaskRunStatus,
    TaskStepRun, TaskStepRunStatus,
};
use crate::domains::vision::OP_ANALYZE_CAMERA;
use crate::orchestrator::approval::{ApprovalManager, AutonomyConfig, AutonomyLevel};
use crate::orchestrator::contracts::{Action, RiskLevel, StepStatus};
use crate::orchestrator::executors::vision::VisionExecutor;
use crate::orchestrator::policy::{
    action_requires_approval, apply_governance_defaults, effective_risk_level, enforce,
    ApprovalContext,
};
use crate::orchestrator::router::Executor;
use crate::runtime::admin_console::{
    resolved_identity_binding_records, AdminConsoleState, AdminConsoleStore, IdentityBindingRecord,
};
use crate::runtime::hub::{
    looks_like_auth_error, CameraConnectRequest, CameraHubService, HubScanRequest,
    HubScanResultItem,
};
use crate::runtime::media::SnapshotCaptureResult;
use crate::runtime::registry::ResolvedCameraTarget;
use crate::runtime::remote_view;
use crate::runtime::task_session::{
    session_state_value_from_conversation, PendingTaskCandidate, PendingTaskConnect,
    TaskConversationState, TaskConversationStore,
};

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
        let tracking = self.begin_task_tracking(&request);

        let mut response = match (
            request.intent.domain.trim().to_lowercase(),
            request.intent.action.trim().to_lowercase(),
        ) {
            (domain, action) if domain == "camera" && action == "scan" => {
                self.handle_camera_scan(&request)
            }
            (domain, action) if domain == "camera" && action == "connect" => {
                self.handle_camera_connect(&request)
            }
            (domain, action) if domain == "camera" && action == "snapshot" => {
                self.handle_camera_snapshot(&request)
            }
            (domain, action) if domain == "camera" && action == "share_link" => {
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
                let notification_request =
                    self.build_notification_request(request, &target, &payload, &artifacts);
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
                    let delivery_outcome =
                        self.deliver_notification_request(request, &notification_request);
                    if let Some(object) = payload.as_object_mut() {
                        object.insert(
                            "notification_delivery".to_string(),
                            delivery_outcome.payload.clone(),
                        );
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

        targets
            .first()
            .cloned()
            .ok_or_else(|| "未找到可分析的摄像头设备。".to_string())
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
        target: &ResolvedCameraTarget,
        payload: &Value,
        artifacts: &[TaskArtifact],
    ) -> Option<NotificationRequest> {
        let admin_state = self.admin_store.load_state().ok()?;
        let destination = first_string(
            &[&request.args],
            &["/notification/destination", "/notification_channel"],
        )
        .unwrap_or_else(|| admin_state.defaults.notification_channel.clone());
        if destination.trim().is_empty() {
            return None;
        }

        let channel = notification_channel_from_value(
            payload
                .pointer("/notification_channel")
                .and_then(Value::as_str)
                .unwrap_or("im_bridge"),
        )?;
        let payload_format = notification_payload_format_from_value(
            payload
                .pointer("/notification_format")
                .and_then(Value::as_str)
                .unwrap_or("plain_text"),
        );
        let title = format!("{} AI 分析", target.display_name);
        let body = string_at_paths(payload, &["/summary", "/detection_summary"])
            .unwrap_or_else(|| format!("{} 分析完成", target.display_name));

        Some(NotificationRequest {
            channel,
            destination,
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
            correlation_id: Some(request.trace_id.clone()),
        })
    }

    fn deliver_notification_request(
        &self,
        request: &TaskRequest,
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
        let admin_state = match self.admin_store.load_state() {
            Ok(state) => state,
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

        let bridge_provider = bridge_provider_config_from_state(&admin_state);
        let recipient = resolve_notification_recipient(
            notification_request,
            &admin_state,
            request.source.user_id.as_str(),
        );
        match service.deliver(
            notification_request,
            bridge_provider.as_ref(),
            recipient.as_ref(),
        ) {
            Ok(record) => NotificationDeliveryOutcome {
                event_type: "task.notification_delivered",
                severity: EventSeverity::Info,
                payload: serde_json::to_value(record).unwrap_or(Value::Null),
            },
            Err(error) => NotificationDeliveryOutcome {
                event_type: "task.notification_failed",
                severity: EventSeverity::Warning,
                payload: json!({
                    "status": "failed",
                    "channel": notification_request.channel,
                    "destination": notification_request.destination,
                    "recipient": recipient,
                    "error": error,
                }),
            },
        }
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
        let mut task_step = self
            .conversation_store
            .load_task_step(&tracking.step_id)?
            .unwrap_or_else(|| TaskStepRun {
                step_id: tracking.step_id.clone(),
                task_id: request.task_id.clone(),
                domain: step_domain.clone(),
                operation: step_operation.clone(),
                route: ExecutionRoute::Local,
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
        task_step.domain = step_domain;
        task_step.operation = step_operation;
        task_step.route = ExecutionRoute::Local;
        task_step.executor_used = response.executor_used.clone();
        task_step.status = task_step_status_from_response(response.status);
        task_step.input_payload = build_step_input_payload(request);
        task_step.output_payload = build_step_output_payload(response);
        task_step.error_code = match response.status {
            TaskStatus::Failed => Some(format!("{}_failed", response.executor_used)),
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
                state: Value::Null,
                resume_token: None,
                expires_at: None,
            });
        session.workspace_id = workspace_id_for_request(request);
        session.channel = request.source.channel.clone();
        session.surface = request.source.surface.clone();
        session.conversation_id = request.source.conversation_id.clone();
        session.user_id = request.source.user_id.clone();
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
        }),
    }
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
            step_id: Some(step_id.to_string()),
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
    first_non_empty(&[request.step_id.as_str()])
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("{}:s1", request.task_id))
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

fn notification_channel_from_value(value: &str) -> Option<NotificationChannel> {
    match value.trim().to_lowercase().as_str() {
        "im_bridge" | "feishu" => Some(NotificationChannel::ImBridge),
        "wecom" => Some(NotificationChannel::Wecom),
        "telegram" => Some(NotificationChannel::Telegram),
        "webhook" => Some(NotificationChannel::Webhook),
        "local_ui" => Some(NotificationChannel::LocalUi),
        _ => None,
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

fn bridge_provider_config_from_state(
    state: &AdminConsoleState,
) -> Option<NotificationBridgeConfig> {
    if state.bridge_provider.app_id.trim().is_empty()
        || state.bridge_provider.app_secret.trim().is_empty()
    {
        return None;
    }
    Some(NotificationBridgeConfig {
        app_id: state.bridge_provider.app_id.clone(),
        app_secret: state.bridge_provider.app_secret.clone(),
        bot_open_id: state.bridge_provider.bot_open_id.clone(),
    })
}

fn resolve_notification_recipient(
    request: &NotificationRequest,
    state: &AdminConsoleState,
    requester_user_id: &str,
) -> Option<NotificationRecipient> {
    let bindings = resolved_identity_binding_records(state);
    if request.destination.trim().is_empty() {
        return None;
    }

    if request.channel == NotificationChannel::LocalUi {
        return Some(NotificationRecipient {
            receive_id_type: NotificationRecipientIdType::ChatId,
            receive_id: request.destination.clone(),
            label: request.destination.clone(),
        });
    }

    if let Some(recipient) = recipient_from_literal_destination(&request.destination, &bindings) {
        return Some(recipient);
    }

    if let Some(recipient) = recipient_from_binding_match(&request.destination, &bindings) {
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

fn recipient_from_literal_destination(
    destination: &str,
    bindings: &[IdentityBindingRecord],
) -> Option<NotificationRecipient> {
    if destination.starts_with("oc_") {
        return Some(NotificationRecipient {
            receive_id_type: NotificationRecipientIdType::ChatId,
            receive_id: destination.to_string(),
            label: destination.to_string(),
        });
    }
    if destination.starts_with("ou_") {
        let label = bindings
            .iter()
            .find(|binding| binding.open_id == destination)
            .map(|binding| binding.display_name.clone())
            .unwrap_or_else(|| destination.to_string());
        return Some(NotificationRecipient {
            receive_id_type: NotificationRecipientIdType::OpenId,
            receive_id: destination.to_string(),
            label,
        });
    }
    None
}

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

fn recipient_from_binding(binding: &IdentityBindingRecord) -> Option<NotificationRecipient> {
    if let Some(chat_id) = binding
        .chat_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(NotificationRecipient {
            receive_id_type: NotificationRecipientIdType::ChatId,
            receive_id: chat_id.clone(),
            label: binding.display_name.clone(),
        });
    }
    if !binding.open_id.trim().is_empty() {
        return Some(NotificationRecipient {
            receive_id_type: NotificationRecipientIdType::OpenId,
            receive_id: binding.open_id.clone(),
            label: binding.display_name.clone(),
        });
    }
    None
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::Engine as _;
    use serde_json::{json, Value};

    use super::{
        artifact_kind_from_name, build_artifact_records, conversation_key,
        effective_autonomy_level, effective_autonomy_level_for_task_run,
        effective_requires_approval, format_pending_candidates, normalize_command_text,
        pending_candidates_from_results, protocol_string, resolve_notification_recipient,
        room_aliases, PendingTaskCandidate, TaskApiService, TaskArtifact, TaskIntent, TaskRequest,
        TaskSource, TaskStatus,
    };
    use crate::connectors::notifications::{
        NotificationChannel, NotificationPayloadFormat, NotificationRecipientIdType,
        NotificationRequest,
    };
    use crate::connectors::storage::StorageTarget;
    use crate::control_plane::approvals::ApprovalStatus;
    use crate::control_plane::auth::{AuthSource, IdentityBinding};
    use crate::control_plane::media::{MediaAssetKind, StorageTargetKind};
    use crate::control_plane::tasks::{ArtifactKind, TaskRunStatus, TaskStepRunStatus};
    use crate::runtime::admin_console::{
        AdminConsoleState, AdminConsoleStore, BridgeProviderConfig, IdentityBindingRecord,
        RemoteViewConfig,
    };
    use crate::runtime::hub::HubScanResultItem;
    use crate::runtime::media::{SnapshotCaptureResult, SnapshotFormat};
    use crate::runtime::registry::{
        CameraCapabilities, CameraDevice, CameraStreamRef, DeviceRegistryStore, DeviceStatus,
        ResolvedCameraTarget, StreamTransport,
    };
    use crate::runtime::task_session::TaskConversationStore;

    fn unique_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}.json"))
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
            },
            intent: TaskIntent::default(),
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
        };

        assert_eq!(conversation_key(&request), Some("chat-1".to_string()));
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
    }

    #[test]
    fn normalize_command_text_strips_punctuation() {
        assert_eq!(
            normalize_command_text("分析 客厅摄像头！"),
            "分析客厅摄像头"
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
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "snapshot".to_string(),
                raw_text: "抓拍门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
        let snapshot_path = unique_path("harbornas-vision-snapshot").with_extension("jpg");
        let annotated_path = unique_path("harbornas-vision-annotated").with_extension("jpg");
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
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "分析门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
    fn build_notification_request_uses_generic_contract() {
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "分析门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
            .expect("notification request");

        assert_eq!(notification.channel, NotificationChannel::ImBridge);
        assert_eq!(
            notification.payload_format,
            NotificationPayloadFormat::LarkCard
        );
        assert_eq!(notification.destination, "家庭通知频道");
        assert_eq!(notification.attachments.len(), 1);
        assert_eq!(notification.title, "Front Door AI 分析");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn handle_camera_share_link_returns_link_artifact() {
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
    fn resolve_notification_recipient_prefers_bound_chat_id() {
        let state = AdminConsoleState {
            bridge_provider: BridgeProviderConfig {
                configured: true,
                app_id: "cli_xxx".to_string(),
                app_secret: "secret".to_string(),
                app_name: "Harbor Bridge".to_string(),
                bot_open_id: "ou_bot".to_string(),
                status: "已连接".to_string(),
            },
            identity_bindings: vec![IdentityBindingRecord {
                open_id: "ou_demo".to_string(),
                user_id: Some("user-1".to_string()),
                union_id: None,
                display_name: "家庭通知频道".to_string(),
                chat_id: Some("oc_demo".to_string()),
            }],
            ..Default::default()
        };
        let request = NotificationRequest {
            channel: NotificationChannel::ImBridge,
            destination: "家庭通知频道".to_string(),
            title: "AI 分析".to_string(),
            body: "检测到人员活动".to_string(),
            payload_format: NotificationPayloadFormat::PlainText,
            structured_payload: Value::Null,
            attachments: Vec::new(),
            correlation_id: Some("trace-1".to_string()),
        };

        let recipient =
            resolve_notification_recipient(&request, &state, "user-1").expect("recipient");

        assert_eq!(
            recipient.receive_id_type,
            NotificationRecipientIdType::ChatId
        );
        assert_eq!(recipient.receive_id, "oc_demo");
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

        let request = NotificationRequest {
            channel: NotificationChannel::ImBridge,
            destination: "平台通知频道".to_string(),
            title: "AI 分析".to_string(),
            body: "检测到人员活动".to_string(),
            payload_format: NotificationPayloadFormat::PlainText,
            structured_payload: Value::Null,
            attachments: Vec::new(),
            correlation_id: Some("trace-1".to_string()),
        };

        let recipient =
            resolve_notification_recipient(&request, &state, "user-1").expect("recipient");

        assert_eq!(
            recipient.receive_id_type,
            NotificationRecipientIdType::ChatId
        );
        assert_eq!(recipient.receive_id, "oc_platform");
    }

    #[test]
    fn deliver_notification_request_reports_failure_without_bridge_config() {
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
        let admin_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let conversation_store = TaskConversationStore::new(conversation_path);
        let service = TaskApiService::new(admin_store, conversation_store);
        let request = TaskRequest {
            task_id: "task-notify".to_string(),
            trace_id: "trace-notify".to_string(),
            step_id: "step-notify".to_string(),
            source: TaskSource {
                channel: "im_bridge".to_string(),
                surface: "harborbeacon".to_string(),
                conversation_id: "chat-1".to_string(),
                user_id: "user-1".to_string(),
                session_id: "sess-1".to_string(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "analyze".to_string(),
                raw_text: "分析门口摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
        };
        let outcome = service.deliver_notification_request(
            &request,
            &NotificationRequest {
                channel: NotificationChannel::ImBridge,
                destination: "家庭通知频道".to_string(),
                title: "AI 分析".to_string(),
                body: "检测到人员活动".to_string(),
                payload_format: NotificationPayloadFormat::PlainText,
                structured_payload: Value::Null,
                attachments: Vec::new(),
                correlation_id: Some("trace-notify".to_string()),
            },
        );

        assert_eq!(outcome.event_type, "task.notification_failed");
        assert_eq!(outcome.payload["status"], "failed");
        assert!(outcome.payload["error"].is_string());

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
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
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-reject");
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
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: "connect".to_string(),
                raw_text: "接入摄像头".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
        let admin_path = unique_path("harbornas-admin-state");
        let registry_path = unique_path("harbornas-device-registry");
        let conversation_path = unique_path("harbornas-task-runtime");
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
            },
            intent: TaskIntent {
                domain: "system".to_string(),
                action: "ping".to_string(),
                raw_text: "测试一下".to_string(),
            },
            entity_refs: Value::Null,
            args: Value::Null,
            autonomy: Default::default(),
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
}
