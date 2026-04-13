//! Minimal Assistant Task API service for HarborBeacon integration.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::domains::vision::OP_ANALYZE_CAMERA;
use crate::orchestrator::contracts::{Action, RiskLevel, StepStatus};
use crate::orchestrator::executors::vision::VisionExecutor;
use crate::orchestrator::router::Executor;
use crate::runtime::admin_console::AdminConsoleStore;
use crate::runtime::hub::{
    looks_like_auth_error, CameraConnectRequest, CameraHubService, HubScanRequest, HubScanResultItem,
};
use crate::runtime::registry::CameraDevice;
use crate::runtime::task_session::{
    PendingTaskCandidate, PendingTaskConnect, TaskConversationState, TaskConversationStore,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskAutonomy {
    #[serde(default)]
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskRequest {
    #[serde(default = "new_task_id")]
    pub task_id: String,
    #[serde(default)]
    pub trace_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
pub struct TaskApiService {
    admin_store: AdminConsoleStore,
    conversation_store: TaskConversationStore,
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

    pub fn handle_task(&self, mut request: TaskRequest) -> TaskResponse {
        if request.task_id.trim().is_empty() {
            request.task_id = new_task_id();
        }
        if request.trace_id.trim().is_empty() {
            request.trace_id = request.task_id.clone();
        }

        match (
            request.intent.domain.trim().to_lowercase(),
            request.intent.action.trim().to_lowercase(),
        ) {
            (domain, action) if domain == "camera" && action == "scan" => self.handle_camera_scan(&request),
            (domain, action) if domain == "camera" && action == "connect" => {
                self.handle_camera_connect(&request)
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
        }
    }

    fn handle_camera_scan(&self, request: &TaskRequest) -> TaskResponse {
        let hub = self.hub();
        let scan_request = HubScanRequest {
            cidr: string_at_paths(&request.args, &["/cidr"]),
            protocol: protocol_string(&request.args),
        };

        match hub.scan(scan_request, None) {
            Ok(summary) => {
                let pending_candidates = pending_candidates_from_results(&summary.results);
                if let Some(mut conversation) = self.load_conversation(request) {
                    conversation.pending_candidates = pending_candidates.clone();
                    conversation.pending_connect = None;
                    conversation.last_scan_cidr = summary.defaults.cidr.clone();
                    let _ = self.conversation_store.save(&conversation);
                }

                let message = format_scan_message(&summary.defaults.cidr, &summary.results, &pending_candidates, summary.devices.len());
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
        let Some(mut conversation) = self.load_conversation(request) else {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "当前没有可继续的候选设备列表，请先发送“扫描摄像头”。".to_string(),
            );
        };

        if index == 0 || index > conversation.pending_candidates.len() {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "当前没有这个序号的候选设备，请先发送“扫描摄像头”刷新列表。".to_string(),
            );
        }

        let candidate = conversation.pending_candidates[index - 1].clone();
        let connect_request = candidate_to_connect_request(&candidate, None);
        match self.hub().manual_add(connect_request, None) {
            Ok(summary) => {
                conversation.pending_connect = None;
                conversation
                    .pending_candidates
                    .retain(|item| item.candidate_id != candidate.candidate_id);
                let _ = self.conversation_store.save(&conversation);
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
                conversation.pending_connect = Some(PendingTaskConnect {
                    resume_token: resume_token.clone(),
                    name: candidate.name.clone(),
                    ip: candidate.ip.clone(),
                    port: candidate.port,
                    rtsp_paths: candidate.rtsp_paths.clone(),
                    requires_auth: true,
                    vendor: candidate.vendor.clone(),
                    model: candidate.model.clone(),
                });
                let _ = self.conversation_store.save(&conversation);
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
            port: first_u16(&[&request.entity_refs, &request.args], &["/port"]).unwrap_or(554),
            rtsp_paths: first_string_vec(&[&request.entity_refs, &request.args], &["/path_candidates", "/rtsp_paths"]),
            requires_auth: false,
            vendor: first_string(&[&request.entity_refs, &request.args], &["/vendor"]),
            model: first_string(&[&request.entity_refs, &request.args], &["/model"]),
        };
        let connect_request = pending_connect_to_request(
            &pending,
            first_string(&[&request.args], &["/password"]),
        );

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
                let Some(mut conversation) = self.load_conversation(request) else {
                    return self.needs_input(
                        request,
                        "camera_hub_service",
                        RiskLevel::Medium,
                        "这台摄像头需要密码，请回复：密码 xxxxxx".to_string(),
                        vec!["password".to_string()],
                        ensure_resume_token(),
                    );
                };
                let resume_token = ensure_resume_token();
                let mut pending_with_token = pending.clone();
                pending_with_token.resume_token = resume_token.clone();
                conversation.pending_connect = Some(pending_with_token);
                let _ = self.conversation_store.save(&conversation);
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
        let Some(mut conversation) = self.load_conversation(request) else {
            return self.failed(
                request,
                "camera_hub_service",
                RiskLevel::Medium,
                "接入流程已过期，请重新发送“扫描摄像头”。".to_string(),
            );
        };
        let Some(pending) = conversation.pending_connect.clone() else {
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
                conversation.pending_connect = None;
                conversation
                    .pending_candidates
                    .retain(|candidate| candidate.ip != pending.ip);
                let _ = self.conversation_store.save(&conversation);
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
        let device = match self.resolve_camera_device(request) {
            Ok(device) => device,
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

        let action = Action {
            domain: "vision".to_string(),
            operation: OP_ANALYZE_CAMERA.to_string(),
            resource: json!({ "device_id": device.device_id }),
            args: json!({
                "detect_label": detect_label,
                "min_confidence": min_confidence,
                "prompt": prompt,
            }),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };

        let vision = VisionExecutor::new(self.admin_store.registry_store().clone());
        match vision.execute(&action, &request.task_id, "s1") {
            Ok(result) if result.status == StepStatus::Success => {
                let summary = string_at_paths(
                    &result.result_payload,
                    &["/summary", "/detection_summary"],
                )
                .unwrap_or_else(|| "分析完成".to_string());
                let artifacts = build_vision_artifacts(&result.result_payload);
                self.completed(
                    request,
                    "vision_executor",
                    RiskLevel::Low,
                    format!("{} 分析完成：{}", device.name, summary),
                    result.result_payload,
                    artifacts,
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

    fn resolve_camera_device(&self, request: &TaskRequest) -> Result<CameraDevice, String> {
        let devices = self.hub().load_registered_cameras()?;
        if devices.is_empty() {
            return Err("当前还没有已注册的摄像头，请先完成接入。".to_string());
        }

        if let Some(device_id) = first_string(&[&request.entity_refs, &request.args], &["/device_id"]) {
            if let Some(device) = devices.iter().find(|device| device.device_id == device_id) {
                return Ok(device.clone());
            }
        }

        let hint = first_string(
            &[&request.entity_refs, &request.args],
            &["/device_hint", "/room", "/name"],
        )
        .or_else(|| (!request.intent.raw_text.trim().is_empty()).then(|| request.intent.raw_text.clone()))
        .unwrap_or_default();
        let normalized = normalize_command_text(&hint);

        for device in &devices {
            let name = device.name.as_str();
            let room = device.room.as_deref().unwrap_or_default();
            if !name.is_empty() && normalized.contains(&name.replace(' ', "").to_lowercase()) {
                return Ok(device.clone());
            }
            if !room.is_empty() && normalized.contains(&room.replace(' ', "").to_lowercase()) {
                return Ok(device.clone());
            }
            for alias in room_aliases(name, room) {
                if normalized.contains(alias) {
                    return Ok(device.clone());
                }
            }
        }

        devices
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
                events: Vec::new(),
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
        TaskResponse {
            task_id: request.task_id.clone(),
            trace_id: request.trace_id.clone(),
            status: TaskStatus::NeedsInput,
            executor_used: executor_used.to_string(),
            risk_level,
            result: TaskResultEnvelope {
                message: prompt.clone(),
                data: Value::Null,
                artifacts: Vec::new(),
                events: Vec::new(),
                next_actions: vec!["密码 xxxxxx".to_string()],
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
        TaskResponse {
            task_id: request.task_id.clone(),
            trace_id: request.trace_id.clone(),
            status: TaskStatus::Failed,
            executor_used: executor_used.to_string(),
            risk_level,
            result: TaskResultEnvelope {
                message,
                data: Value::Null,
                artifacts: Vec::new(),
                events: Vec::new(),
                next_actions: Vec::new(),
            },
            audit_ref: new_audit_ref(),
            missing_fields: Vec::new(),
            prompt: None,
            resume_token: None,
        }
    }

    fn hub(&self) -> CameraHubService {
        CameraHubService::new(self.admin_store.clone())
    }

    fn load_conversation(&self, request: &TaskRequest) -> Option<TaskConversationState> {
        let key = conversation_key(request)?;
        self.conversation_store.load(&key).ok()
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
        room: None,
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
        room: None,
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
    if let Some(path) = string_at_paths(payload, &["/snapshot/image_path"]) {
        artifacts.push(TaskArtifact {
            kind: "image".to_string(),
            label: "抓拍图片".to_string(),
            mime_type: "image/jpeg".to_string(),
            path: Some(path),
            url: None,
            metadata: Value::Null,
        });
    }
    if let Some(path) = string_at_paths(payload, &["/snapshot/annotated_image_path"]) {
        artifacts.push(TaskArtifact {
            kind: "image".to_string(),
            label: "标注图片".to_string(),
            mime_type: "image/jpeg".to_string(),
            path: Some(path),
            url: None,
            metadata: Value::Null,
        });
    }
    artifacts
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
        value.pointer(path)
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

fn first_string(values: &[&Value], paths: &[&str]) -> Option<String> {
    values.iter().find_map(|value| string_at_paths(value, paths))
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
    values.iter().copied().find(|value| !value.trim().is_empty())
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

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{
        conversation_key, format_pending_candidates, normalize_command_text, pending_candidates_from_results,
        protocol_string, room_aliases, PendingTaskCandidate, TaskIntent, TaskRequest, TaskSource,
    };
    use crate::runtime::hub::HubScanResultItem;

    #[test]
    fn conversation_key_prefers_conversation_id() {
        let request = TaskRequest {
            task_id: "task-1".to_string(),
            trace_id: "trace-1".to_string(),
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
        assert_eq!(protocol_string(&args), Some("onvif + rtsp_probe".to_string()));
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
            port: 554,
            rtsp_paths: vec!["/live".to_string()],
            requires_auth: true,
            vendor: None,
            model: None,
        }]);

        assert!(rendered.contains("需要密码"));
    }

    #[test]
    fn normalize_command_text_strips_punctuation() {
        assert_eq!(normalize_command_text("分析 客厅摄像头！"), "分析客厅摄像头");
    }

    #[test]
    fn room_aliases_cover_living_room() {
        let aliases = room_aliases("Living Room Cam", "living room");
        assert!(aliases.contains(&"客厅"));
    }
}
