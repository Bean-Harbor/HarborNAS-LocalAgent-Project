use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread;

use qrcodegen::{QrCode, QrCodeEcc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};

use harborbeacon_local_agent::control_plane::media::{MediaSession, MediaSessionStatus, ShareLink};
use harborbeacon_local_agent::control_plane::users::{MembershipStatus, RoleKind};
use harborbeacon_local_agent::runtime::access_control::{
    authorize_access, AccessAction, AccessIdentityHints, AccessPrincipal,
};
use harborbeacon_local_agent::runtime::admin_console::{
    AdminConsoleState, AdminConsoleStore, AdminDefaults, BridgeProviderConfig,
    IdentityBindingRecord,
};
use harborbeacon_local_agent::runtime::hub::{
    build_mobile_setup_url, CameraHubService, HubManualAddSummary, HubScanRequest, HubScanSummary,
    HubStateSnapshot,
};
use harborbeacon_local_agent::runtime::registry::{CameraDevice, DeviceRegistryStore};
use harborbeacon_local_agent::runtime::remote_view;
use harborbeacon_local_agent::runtime::task_api::{
    TaskApiService, TaskApprovalSummary, TaskIntent, TaskRequest, TaskResponse, TaskSource,
    TaskStatus,
};
use harborbeacon_local_agent::runtime::task_session::TaskConversationStore;

#[derive(Debug, Clone)]
struct Cli {
    bind: String,
    admin_state: PathBuf,
    device_registry: PathBuf,
    conversations: PathBuf,
    public_origin: String,
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> String {
    *index += 1;
    if *index >= args.len() {
        fail(&format!("missing value for {flag}"));
    }
    args[*index].clone()
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn print_usage() {
    eprintln!(
        "Usage: agent-hub-admin-api [--bind ADDR] [--admin-state PATH] [--device-registry PATH] [--conversations PATH] [--public-origin URL]"
    );
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:4174".to_string(),
            admin_state: PathBuf::from(".harborbeacon/admin-console.json"),
            device_registry: PathBuf::from(".harborbeacon/device-registry.json"),
            conversations: PathBuf::from(".harborbeacon/task-api-conversations.json"),
            public_origin: "http://harborbeacon.local:4174".to_string(),
        }
    }
}

impl Cli {
    fn parse() -> Self {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
            print_usage();
            std::process::exit(0);
        }

        let mut cli = Self::default();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            match arg.as_str() {
                "--bind" => cli.bind = take_value(&args, &mut index, "--bind"),
                value if value.starts_with("--bind=") => {
                    cli.bind = value["--bind=".len()..].to_string();
                }
                "--admin-state" => {
                    cli.admin_state = PathBuf::from(take_value(&args, &mut index, "--admin-state"))
                }
                value if value.starts_with("--admin-state=") => {
                    cli.admin_state = PathBuf::from(value["--admin-state=".len()..].to_string())
                }
                "--device-registry" => {
                    cli.device_registry =
                        PathBuf::from(take_value(&args, &mut index, "--device-registry"))
                }
                value if value.starts_with("--device-registry=") => {
                    cli.device_registry =
                        PathBuf::from(value["--device-registry=".len()..].to_string())
                }
                "--conversations" => {
                    cli.conversations =
                        PathBuf::from(take_value(&args, &mut index, "--conversations"))
                }
                value if value.starts_with("--conversations=") => {
                    cli.conversations =
                        PathBuf::from(value["--conversations=".len()..].to_string())
                }
                "--public-origin" => {
                    cli.public_origin =
                        take_value(&args, &mut index, "--public-origin")
                }
                value if value.starts_with("--public-origin=") => {
                    cli.public_origin = value["--public-origin=".len()..].to_string();
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                value if value.starts_with('-') => {
                    fail(&format!("unknown flag: {value}"));
                }
                value => {
                    fail(&format!("unexpected positional argument: {value}"));
                }
            }
            index += 1;
        }

        cli
    }
}

#[derive(Debug, Clone)]
struct AdminApi {
    admin_store: AdminConsoleStore,
    task_service: TaskApiService,
    public_origin: String,
}

#[derive(Debug, Deserialize)]
struct BindingTestRequest {
    #[serde(default)]
    binding_code: Option<String>,
    display_name: String,
    #[serde(default)]
    open_id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    union_id: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManualAddRequest {
    name: String,
    room: Option<String>,
    ip: String,
    path: Option<String>,
    username: Option<String>,
    password: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct DefaultsRequest {
    cidr: String,
    discovery: String,
    recording: String,
    capture: String,
    ai: String,
    #[serde(alias = "feishu_group")]
    notification_channel: String,
    #[serde(default)]
    rtsp_username: String,
    #[serde(default)]
    rtsp_password: String,
    #[serde(default)]
    rtsp_port: Option<u16>,
    #[serde(default)]
    rtsp_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BridgeConfigRequest {}

#[derive(Debug, Deserialize, Default)]
struct ApprovalDecisionRequest {
    #[serde(default)]
    approver_user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MembershipRoleUpdateRequest {
    role_kind: String,
}

#[derive(Debug, Serialize)]
struct CameraTaskResponse {
    task_response: TaskResponse,
}

#[derive(Debug, Serialize)]
struct ApprovalDecisionResponse {
    approval: TaskApprovalSummary,
    #[serde(default)]
    task_response: Option<TaskResponse>,
}

#[derive(Debug, Serialize)]
struct AccessMemberSummary {
    user_id: String,
    display_name: String,
    role_kind: String,
    membership_status: String,
    source: String,
    #[serde(default)]
    open_id: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
    can_edit: bool,
    is_owner: bool,
}

#[derive(Debug, Serialize)]
struct ShareLinkSummary {
    share_link_id: String,
    media_session_id: String,
    device_id: String,
    device_name: String,
    #[serde(default)]
    opened_by_user_id: Option<String>,
    access_scope: String,
    session_status: String,
    status: String,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    revoked_at: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    ended_at: Option<String>,
    can_revoke: bool,
}

type StateResponse = HubStateSnapshot;
type ScanRequest = HubScanRequest;
type ScanResponse = HubScanSummary;
type ManualAddResponse = HubManualAddSummary;

impl AdminApi {
    fn new(
        admin_store: AdminConsoleStore,
        task_service: TaskApiService,
        public_origin: String,
    ) -> Self {
        Self {
            admin_store,
            task_service,
            public_origin,
        }
    }

    fn hub(&self) -> CameraHubService {
        CameraHubService::new(self.admin_store.clone())
    }

    fn authorize_admin_action(
        &self,
        hints: &AccessIdentityHints,
        action: AccessAction,
    ) -> Result<AccessPrincipal, String> {
        let state = self.admin_store.load_or_create_state()?;
        let workspace_id = state
            .platform
            .workspaces
            .first()
            .map(|workspace| workspace.workspace_id.clone())
            .unwrap_or_else(|| "home-1".to_string());
        authorize_access(
            &state,
            hints,
            action,
            &format!("workspace:{workspace_id}"),
            true,
        )
    }

    fn authorize_camera_action(
        &self,
        hints: &AccessIdentityHints,
        device_id: &str,
        action: AccessAction,
    ) -> Result<AccessPrincipal, String> {
        let state = self.admin_store.load_or_create_state()?;
        authorize_access(&state, hints, action, &format!("camera:{device_id}"), true)
    }

    fn handle(&self, mut request: Request) {
        let method = request.method().clone();
        let raw_url = request.url().to_string();
        let path = raw_url.split('?').next().unwrap_or("/").to_string();
        let remote_addr = request.remote_addr().copied();
        let headers = request.headers().to_vec();
        let identity_hints = request_identity_hints(&raw_url, &headers);

        if is_admin_surface_path(path.as_str()) {
            if let Err(error) = ensure_local_admin_access(remote_addr, &headers) {
                let _ = request.respond(error_json(StatusCode(403), &error).boxed());
                return;
            }
        }

        let response = match method {
            Method::Get if path == "/healthz" => ok_json(&json!({"status":"ok"})).boxed(),
            Method::Get if path == "/api/state" => self.handle_state(&identity_hints).boxed(),
            Method::Get if path == "/api/access/members" => {
                self.handle_access_members(&identity_hints).boxed()
            }
            Method::Get if path == "/api/share-links" => {
                self.handle_share_links(&raw_url, &identity_hints).boxed()
            }
            Method::Get if path == "/api/tasks/approvals" => {
                self.handle_pending_approvals(&identity_hints).boxed()
            }
            Method::Get if path == "/api/binding/qr.svg" => {
                self.handle_binding_qr_svg(&identity_hints).boxed()
            }
            Method::Get if path == "/api/binding/static-qr.svg" => {
                self.handle_static_binding_qr_svg(&identity_hints).boxed()
            }
            Method::Get if path == "/setup/mobile" => self
                .handle_mobile_setup_page(&raw_url, &identity_hints)
                .boxed(),
            Method::Get
                if path.starts_with("/shared/cameras/") && path.ends_with("/live.mjpeg") =>
            {
                self.handle_shared_camera_live_mjpeg(&path)
            }
            Method::Get if path.starts_with("/shared/cameras/") => {
                self.handle_shared_live_view_page(&path).boxed()
            }
            Method::Get if path.starts_with("/live/cameras/") => self
                .handle_live_view_page(&raw_url, &path, remote_addr, &headers, &identity_hints)
                .boxed(),
            Method::Get if path.starts_with("/api/cameras/") && path.ends_with("/live.mjpeg") => {
                self.handle_camera_live_mjpeg(&path, remote_addr, &headers, &identity_hints)
            }
            Method::Get if path.starts_with("/api/cameras/") && path.ends_with("/snapshot.jpg") => {
                self.handle_camera_snapshot(&path, remote_addr, &headers, &identity_hints)
                    .boxed()
            }
            Method::Post if path == "/api/binding/refresh" => {
                self.handle_refresh_binding(&identity_hints).boxed()
            }
            Method::Post if path == "/api/binding/demo-bind" => {
                self.handle_demo_bind(&identity_hints).boxed()
            }
            Method::Post if path == "/api/binding/test-bind" => {
                self.handle_test_bind(&mut request, &identity_hints).boxed()
            }
            Method::Post if path == "/api/bridge/configure" => self
                .handle_configure_bridge(&mut request, &identity_hints)
                .boxed(),
            Method::Post
                if path.starts_with("/api/tasks/approvals/") && path.ends_with("/approve") =>
            {
                self.handle_approve_approval(path.as_str(), &mut request, &identity_hints)
                    .boxed()
            }
            Method::Post if path.starts_with("/api/access/members/") && path.ends_with("/role") => {
                self.handle_update_member_role(path.as_str(), &mut request, &identity_hints)
                    .boxed()
            }
            Method::Post
                if path.starts_with("/api/tasks/approvals/") && path.ends_with("/reject") =>
            {
                self.handle_reject_approval(path.as_str(), &mut request, &identity_hints)
                    .boxed()
            }
            Method::Post if path == "/api/discovery/scan" => {
                self.handle_scan(&mut request, &identity_hints).boxed()
            }
            Method::Post if path == "/api/devices/manual" => self
                .handle_manual_add(&mut request, &identity_hints)
                .boxed(),
            Method::Post if path.starts_with("/api/cameras/") && path.ends_with("/share-link") => {
                self.handle_camera_share_link(&path, &identity_hints)
                    .boxed()
            }
            Method::Post if path.starts_with("/api/share-links/") && path.ends_with("/revoke") => {
                self.handle_revoke_share_link(&path, &identity_hints)
                    .boxed()
            }
            Method::Post if path.starts_with("/api/cameras/") && path.ends_with("/snapshot") => {
                self.handle_camera_task_snapshot(&path, &identity_hints)
                    .boxed()
            }
            Method::Post if path.starts_with("/api/cameras/") && path.ends_with("/analyze") => {
                self.handle_camera_analyze(&path, &identity_hints).boxed()
            }
            Method::Post if path == "/api/defaults" => self
                .handle_save_defaults(&mut request, &identity_hints)
                .boxed(),
            Method::Options => no_content().boxed(),
            _ => error_json(StatusCode(404), "route not found").boxed(),
        };

        let _ = request.respond(response);
    }

    fn handle_state(&self, hints: &AccessIdentityHints) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.current_state() {
            Ok(payload) => ok_json(&redact_state_snapshot(payload)),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_access_members(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }

        let state = match self.admin_store.load_or_create_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };

        ok_json(&build_access_member_summaries(&state))
    }

    fn handle_share_links(
        &self,
        url: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }

        let device_filter = parse_query_param(url, "device_id")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        match self.list_share_links(device_filter.as_deref()) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_pending_approvals(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::ApprovalReview) {
            return error_json(StatusCode(403), &error);
        }
        match self.task_service.pending_approvals() {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_approve_approval(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let principal = match self.authorize_admin_action(hints, AccessAction::ApprovalReview) {
            Ok(principal) => principal,
            Err(error) => return error_json(StatusCode(403), &error),
        };
        let approval_id = match parse_approval_decision_path(path, "approve") {
            Some(approval_id) => approval_id,
            None => return error_json(StatusCode(400), "invalid approval approve path"),
        };
        let body: ApprovalDecisionRequest = match read_json_body_or_default(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        let approver_user_id = body
            .approver_user_id
            .filter(|user_id| user_id == &principal.user_id)
            .or_else(|| Some(principal.user_id.clone()));

        match self
            .task_service
            .approve_pending_approval(&approval_id, approver_user_id)
        {
            Ok((approval, task_response)) => ok_json(&ApprovalDecisionResponse {
                approval,
                task_response: Some(task_response),
            }),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_reject_approval(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let principal = match self.authorize_admin_action(hints, AccessAction::ApprovalReview) {
            Ok(principal) => principal,
            Err(error) => return error_json(StatusCode(403), &error),
        };
        let approval_id = match parse_approval_decision_path(path, "reject") {
            Some(approval_id) => approval_id,
            None => return error_json(StatusCode(400), "invalid approval reject path"),
        };
        let body: ApprovalDecisionRequest = match read_json_body_or_default(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        let approver_user_id = body
            .approver_user_id
            .filter(|user_id| user_id == &principal.user_id)
            .or_else(|| Some(principal.user_id.clone()));

        match self
            .task_service
            .reject_pending_approval(&approval_id, approver_user_id)
        {
            Ok(approval) => ok_json(&ApprovalDecisionResponse {
                approval,
                task_response: None,
            }),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_update_member_role(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }

        let user_id = match parse_member_role_update_path(path) {
            Some(user_id) => user_id,
            None => return error_json(StatusCode(400), "invalid member role path"),
        };
        let body: MembershipRoleUpdateRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        let role_kind = match parse_role_kind(&body.role_kind) {
            Ok(role_kind) => role_kind,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        match self
            .admin_store
            .set_member_role(&user_id, role_kind)
            .map(|state| build_access_member_summaries(&state))
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_binding_qr_svg(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let state = match self.current_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let setup_url = state.binding.setup_url.clone();
        let qr = match QrCode::encode_text(&setup_url, QrCodeEcc::Medium) {
            Ok(qr) => qr,
            Err(error) => {
                return error_json(StatusCode(500), &format!("qr encode failed: {error}"))
            }
        };
        let svg = qr_to_svg(&qr, 4);
        let mut response = Response::from_string(svg).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"image/svg+xml; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_static_binding_qr_svg(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let state = match self.current_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let setup_url = state.binding.static_setup_url.clone();
        let qr = match QrCode::encode_text(&setup_url, QrCodeEcc::Medium) {
            Ok(qr) => qr,
            Err(error) => {
                return error_json(StatusCode(500), &format!("qr encode failed: {error}"))
            }
        };
        let svg = qr_to_svg(&qr, 4);
        let mut response = Response::from_string(svg).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"image/svg+xml; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_mobile_setup_page(
        &self,
        url: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let state = match self.admin_store.load_or_create_state() {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let session_code =
            parse_query_param(url, "session").unwrap_or_else(|| state.binding.session_code.clone());
        let body = render_mobile_setup_page(
            &state,
            &build_mobile_setup_url(&self.public_origin, Some(&session_code)),
            &build_mobile_setup_url(&self.public_origin, None),
            &session_code,
        );
        let mut response = Response::from_string(body).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"text/html; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_live_view_page(
        &self,
        url: &str,
        path: &str,
        remote_addr: Option<SocketAddr>,
        headers: &[Header],
        hints: &AccessIdentityHints,
    ) -> Response<Cursor<Vec<u8>>> {
        if let Err(error) = ensure_local_camera_access(remote_addr, headers) {
            return error_json(StatusCode(403), &error);
        }

        let device_id = match parse_camera_live_page_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid live view page path"),
        };

        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };

        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraView)
        {
            return error_json(StatusCode(403), &error);
        }

        let body = render_live_view_page(&self.public_origin, &device, &identity_query_suffix(url));
        let mut response = Response::from_string(body).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"text/html; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_shared_live_view_page(&self, path: &str) -> Response<Cursor<Vec<u8>>> {
        let token = match parse_shared_camera_live_page_path(path) {
            Some(token) => token,
            None => return error_json(StatusCode(400), "invalid shared live view path"),
        };
        let claims = match self.verify_shared_camera_token(&token) {
            Ok(claims) => claims,
            Err(error) => return error_json(StatusCode(403), &error),
        };
        let device = match self.load_camera_device(&claims.device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };

        let body = render_shared_live_view_page(&token, &device);
        let mut response = Response::from_string(body).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"text/html; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_refresh_binding(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        match self
            .admin_store
            .refresh_binding_qr()
            .and_then(|_| self.current_state())
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_demo_bind(&self, hints: &AccessIdentityHints) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        match self
            .admin_store
            .mark_demo_bound("Bean / 本地管理员")
            .and_then(|_| self.current_state())
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_test_bind(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let body: BindingTestRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        let display_name = body.display_name.trim();
        if display_name.is_empty() {
            return error_json(StatusCode(400), "display_name 不能为空");
        }

        let state = match self.admin_store.load_or_create_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };

        let binding_code = body
            .binding_code
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(state.binding.session_code.as_str())
            .to_string();
        let open_id = body
            .open_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .unwrap_or_else(|| slugify_identity(display_name));

        let user = IdentityBindingRecord {
            open_id,
            user_id: body
                .user_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            union_id: body
                .union_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            display_name: display_name.to_string(),
            chat_id: body
                .chat_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        };

        match self
            .admin_store
            .bind_identity_user(&binding_code, user)
            .and_then(|_| self.current_state())
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_configure_bridge(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let body: BridgeConfigRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        let _ = body;

        match self
            .hub()
            .refresh_bridge_provider_status(Some(&self.public_origin))
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_scan(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let principal = match self.authorize_admin_action(hints, AccessAction::AdminManage) {
            Ok(principal) => principal,
            Err(error) => return error_json(StatusCode(403), &error),
        };
        let body: ScanRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        match self.scan(&principal, body) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_manual_add(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let principal = match self.authorize_admin_action(hints, AccessAction::AdminManage) {
            Ok(principal) => principal,
            Err(error) => return error_json(StatusCode(403), &error),
        };
        let body: ManualAddRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        match self.manual_add(&principal, body) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_save_defaults(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let body: DefaultsRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        let defaults = AdminDefaults {
            cidr: body.cidr,
            discovery: body.discovery,
            recording: body.recording,
            capture: body.capture,
            ai: body.ai,
            notification_channel: body.notification_channel,
            rtsp_username: body.rtsp_username,
            rtsp_password: body.rtsp_password,
            rtsp_port: body.rtsp_port.unwrap_or(554),
            rtsp_paths: body.rtsp_paths,
        };

        match self
            .hub()
            .save_defaults(defaults, Some(&self.public_origin))
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_camera_analyze(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_camera_analyze_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid camera analyze path"),
        };
        let principal =
            match self.authorize_camera_action(hints, &device_id, AccessAction::CameraOperate) {
                Ok(principal) => principal,
                Err(error) => return error_json(StatusCode(403), &error),
            };

        ok_json(&CameraTaskResponse {
            task_response: self.analyze_camera(&principal, &device_id),
        })
    }

    fn handle_camera_task_snapshot(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_camera_task_snapshot_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid camera snapshot task path"),
        };
        let principal =
            match self.authorize_camera_action(hints, &device_id, AccessAction::CameraOperate) {
                Ok(principal) => principal,
                Err(error) => return error_json(StatusCode(403), &error),
            };

        ok_json(&CameraTaskResponse {
            task_response: self.snapshot_camera(&principal, &device_id),
        })
    }

    fn handle_camera_share_link(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_camera_share_link_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid camera share-link path"),
        };
        let principal =
            match self.authorize_camera_action(hints, &device_id, AccessAction::CameraOperate) {
                Ok(principal) => principal,
                Err(error) => return error_json(StatusCode(403), &error),
            };

        ok_json(&CameraTaskResponse {
            task_response: self.share_camera_link(&principal, &device_id),
        })
    }

    fn handle_revoke_share_link(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }

        let share_link_id = match parse_share_link_revoke_path(path) {
            Some(share_link_id) => share_link_id,
            None => return error_json(StatusCode(400), "invalid share-link revoke path"),
        };

        let store = self.task_service.conversation_store();
        let share_link = match store.load_share_link(&share_link_id) {
            Ok(Some(share_link)) => share_link,
            Ok(None) => return error_json(StatusCode(404), "share link not found"),
            Err(error) => return error_json(StatusCode(500), &error),
        };

        let revoked_at = remote_view::now_unix_secs().to_string();
        let revoked = match store.revoke_share_link(&share_link_id, Some(revoked_at.clone())) {
            Ok(Some(share_link)) => share_link,
            Ok(None) => return error_json(StatusCode(404), "share link not found"),
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let media_session =
            match store.close_media_session(&share_link.media_session_id, Some(revoked_at)) {
                Ok(media_session) => media_session,
                Err(error) => return error_json(StatusCode(500), &error),
            };

        ok_json(&json!({
            "share_link": revoked,
            "media_session": media_session,
        }))
    }

    fn handle_camera_snapshot(
        &self,
        path: &str,
        remote_addr: Option<SocketAddr>,
        headers: &[Header],
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = ensure_local_camera_access(remote_addr, headers) {
            return error_json(StatusCode(403), &error);
        }

        let device_id = match parse_camera_snapshot_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid camera snapshot path"),
        };
        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraView)
        {
            return error_json(StatusCode(403), &error);
        }

        match self.capture_camera_snapshot(&device_id) {
            Ok(bytes) => image_response(StatusCode(200), bytes, "image/jpeg"),
            Err(error) if error.contains("device not found") => error_json(StatusCode(404), &error),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_camera_live_mjpeg(
        &self,
        path: &str,
        remote_addr: Option<SocketAddr>,
        headers: &[Header],
        hints: &AccessIdentityHints,
    ) -> ResponseBox {
        if let Err(error) = ensure_local_camera_access(remote_addr, headers) {
            return error_json(StatusCode(403), &error).boxed();
        }

        let device_id = match parse_camera_live_stream_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid live stream path").boxed(),
        };
        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraView)
        {
            return error_json(StatusCode(403), &error).boxed();
        }

        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error).boxed()
            }
            Err(error) => return error_json(StatusCode(422), &error).boxed(),
        };

        let stream = match FfmpegMjpegStream::spawn(&device.primary_stream.url) {
            Ok(stream) => stream,
            Err(error) => {
                return error_json(StatusCode(422), &format!("打开实时画面失败: {error}")).boxed()
            }
        };

        let headers = vec![
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"multipart/x-mixed-replace;boundary=ffmpeg".as_slice(),
            )
            .expect("header"),
            Header::from_bytes(b"X-Accel-Buffering".as_slice(), b"no".as_slice()).expect("header"),
        ];
        let mut response = Response::new(StatusCode(200), headers, stream, None, None).boxed();
        add_common_headers(&mut response);
        response
    }

    fn handle_shared_camera_live_mjpeg(&self, path: &str) -> ResponseBox {
        let token = match parse_shared_camera_live_stream_path(path) {
            Some(token) => token,
            None => return error_json(StatusCode(400), "invalid shared live stream path").boxed(),
        };
        let claims = match self.verify_shared_camera_token(&token) {
            Ok(claims) => claims,
            Err(error) => return error_json(StatusCode(403), &error).boxed(),
        };
        let device = match self.load_camera_device(&claims.device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error).boxed()
            }
            Err(error) => return error_json(StatusCode(422), &error).boxed(),
        };

        let stream = match FfmpegMjpegStream::spawn(&device.primary_stream.url) {
            Ok(stream) => stream,
            Err(error) => {
                return error_json(StatusCode(422), &format!("打开共享实时画面失败: {error}"))
                    .boxed()
            }
        };

        let headers = vec![
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"multipart/x-mixed-replace;boundary=ffmpeg".as_slice(),
            )
            .expect("header"),
            Header::from_bytes(b"X-Accel-Buffering".as_slice(), b"no".as_slice()).expect("header"),
        ];
        let mut response = Response::new(StatusCode(200), headers, stream, None, None).boxed();
        add_common_headers(&mut response);
        response
    }

    fn current_state(&self) -> Result<StateResponse, String> {
        self.hub().state_snapshot(Some(&self.public_origin))
    }

    fn scan(
        &self,
        principal: &AccessPrincipal,
        request: ScanRequest,
    ) -> Result<ScanResponse, String> {
        let response = self
            .task_service
            .handle_task(self.build_camera_task_request(
                principal,
                "scan",
                "扫描摄像头",
                json!({
                    "cidr": request.cidr,
                    "protocol": request.protocol,
                }),
            ));
        if response.status != TaskStatus::Completed {
            return Err(task_error_message(&response));
        }

        let state = self.current_state()?;
        let results = parse_scan_results(&response.result.data)?;
        let scanned_hosts = response
            .result
            .data
            .pointer("/summary/scanned_hosts")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or_default();

        Ok(HubScanSummary {
            binding: state.binding,
            defaults: state.defaults,
            devices: state.devices,
            results,
            scanned_hosts,
        })
    }

    fn manual_add(
        &self,
        principal: &AccessPrincipal,
        request: ManualAddRequest,
    ) -> Result<ManualAddResponse, String> {
        let path_candidates = request
            .path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                if value.starts_with('/') {
                    value.to_string()
                } else {
                    format!("/{value}")
                }
            })
            .map(|value| vec![value])
            .unwrap_or_default();

        let response = self
            .task_service
            .handle_task(self.build_camera_task_request(
                principal,
                "connect",
                "手动接入摄像头",
                json!({
                    "name": request.name,
                    "room": request.room,
                    "ip": request.ip,
                    "path_candidates": path_candidates,
                    "username": request.username,
                    "password": request.password,
                    "port": request.port,
                    "discovery_source": "admin_console_manual_add",
                }),
            ));
        if response.status != TaskStatus::Completed {
            return Err(task_error_message(&response));
        }

        let state = self.current_state()?;
        let device = parse_connected_device(&response.result.data)?;
        Ok(HubManualAddSummary {
            binding: state.binding,
            defaults: state.defaults,
            device,
            devices: state.devices,
            note: response.result.message,
        })
    }

    fn analyze_camera(&self, principal: &AccessPrincipal, device_id: &str) -> TaskResponse {
        self.task_service
            .handle_task(self.build_camera_task_request(
                principal,
                "analyze",
                "分析摄像头画面",
                json!({
                    "device_id": device_id,
                }),
            ))
    }

    fn snapshot_camera(&self, principal: &AccessPrincipal, device_id: &str) -> TaskResponse {
        self.task_service
            .handle_task(self.build_camera_task_request(
                principal,
                "snapshot",
                "抓拍摄像头画面",
                json!({
                    "device_id": device_id,
                }),
            ))
    }

    fn share_camera_link(&self, principal: &AccessPrincipal, device_id: &str) -> TaskResponse {
        self.task_service
            .handle_task(self.build_camera_task_request(
                principal,
                "share_link",
                "生成共享观看链接",
                json!({
                    "device_id": device_id,
                }),
            ))
    }

    fn capture_camera_snapshot(&self, device_id: &str) -> Result<Vec<u8>, String> {
        self.hub().capture_camera_snapshot(device_id)
    }

    fn load_camera_device(
        &self,
        device_id: &str,
    ) -> Result<harborbeacon_local_agent::runtime::registry::CameraDevice, String> {
        self.hub()
            .load_registered_cameras()?
            .into_iter()
            .find(|device| device.device_id == device_id)
            .ok_or_else(|| format!("device not found: {device_id}"))
    }

    fn verify_shared_camera_token(
        &self,
        token: &str,
    ) -> Result<remote_view::CameraShareClaims, String> {
        let remote_view_config = self.admin_store.load_remote_view_config()?;
        let claims =
            remote_view::verify_camera_share_token(&remote_view_config.share_secret, token)?;
        let token_hash = remote_view::camera_share_token_hash(token);
        let share_link = self
            .task_service
            .conversation_store()
            .find_share_link_by_token_hash(&token_hash)?
            .ok_or_else(|| "share token is not registered".to_string())?;
        if share_link
            .revoked_at
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
        {
            return Err("share token revoked".to_string());
        }
        if let Some(expires_at) = share_link.expires_at.as_deref() {
            let expires_at = expires_at
                .trim()
                .parse::<u64>()
                .map_err(|_| "share token expiry is invalid".to_string())?;
            if remote_view::now_unix_secs() > expires_at {
                return Err("share token expired".to_string());
            }
        }

        let media_session = self
            .task_service
            .conversation_store()
            .load_media_session(&share_link.media_session_id)?
            .ok_or_else(|| {
                format!(
                    "share media session not found: {}",
                    share_link.media_session_id
                )
            })?;
        if media_session.device_id != claims.device_id {
            return Err("share token device mismatch".to_string());
        }
        if media_session.share_link_id.as_deref() != Some(share_link.share_link_id.as_str()) {
            return Err("share token session mismatch".to_string());
        }
        if !matches!(
            media_session.status,
            MediaSessionStatus::Opening | MediaSessionStatus::Active
        ) {
            return Err("share session is no longer active".to_string());
        }

        Ok(claims)
    }

    fn list_share_links(
        &self,
        device_filter: Option<&str>,
    ) -> Result<Vec<ShareLinkSummary>, String> {
        let share_links = self.task_service.conversation_store().list_share_links()?;
        let media_sessions = self
            .task_service
            .conversation_store()
            .list_media_sessions()?;
        let media_session_map: HashMap<String, MediaSession> = media_sessions
            .into_iter()
            .map(|media_session| (media_session.media_session_id.clone(), media_session))
            .collect();
        let device_name_map: HashMap<String, String> = self
            .hub()
            .load_registered_cameras()?
            .into_iter()
            .map(|device| (device.device_id.clone(), device.name))
            .collect();
        let now = remote_view::now_unix_secs();

        let mut summaries = share_links
            .into_iter()
            .filter_map(|share_link| {
                let media_session = media_session_map.get(&share_link.media_session_id)?;
                if let Some(device_filter) = device_filter {
                    if media_session.device_id != device_filter {
                        return None;
                    }
                }
                Some(build_share_link_summary(
                    share_link,
                    media_session,
                    device_name_map.get(&media_session.device_id),
                    now,
                ))
            })
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then(right.share_link_id.cmp(&left.share_link_id))
        });
        Ok(summaries)
    }

    fn build_camera_task_request(
        &self,
        principal: &AccessPrincipal,
        action: &str,
        raw_text: &str,
        args: Value,
    ) -> TaskRequest {
        TaskRequest {
            task_id: String::new(),
            trace_id: String::new(),
            step_id: String::new(),
            source: TaskSource {
                channel: "admin_api".to_string(),
                surface: "agent_hub_admin_api".to_string(),
                conversation_id: format!("admin-console:{}", principal.user_id),
                user_id: principal.user_id.clone(),
                session_id: format!("admin-console:{}", principal.user_id),
                route_key: String::new(),
            },
            intent: TaskIntent {
                domain: "camera".to_string(),
                action: action.to_string(),
                raw_text: raw_text.to_string(),
            },
            entity_refs: Value::Null,
            args,
            autonomy: Default::default(),
            message: None,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let device_registry_path = resolve_state_path(&cli.device_registry);
    let admin_state_path = resolve_state_path(&cli.admin_state);
    let conversation_path = resolve_state_path(&cli.conversations);
    let registry_store = DeviceRegistryStore::new(device_registry_path);
    let admin_store = AdminConsoleStore::new(admin_state_path, registry_store);
    let conversation_store = TaskConversationStore::new(conversation_path);
    let task_service = TaskApiService::new(admin_store.clone(), conversation_store);
    let api = AdminApi::new(admin_store, task_service, cli.public_origin);

    let server = Server::http(&cli.bind).unwrap_or_else(|error| {
        eprintln!("failed to start admin api on {}: {}", cli.bind, error);
        std::process::exit(1);
    });

    println!("HarborBeacon admin API listening on http://{}", cli.bind);
    for request in server.incoming_requests() {
        let api = api.clone();
        thread::spawn(move || {
            api.handle(request);
        });
    }
}

fn resolve_state_path(preferred: &Path) -> PathBuf {
    if preferred.exists() || !is_harborbeacon_state_path(preferred) {
        return preferred.to_path_buf();
    }

    let legacy = legacy_state_path(preferred);
    if legacy.exists() {
        eprintln!(
            "warning: legacy .harbornas state path {} is deprecated; prefer {}",
            legacy.display(),
            preferred.display()
        );
        legacy
    } else {
        preferred.to_path_buf()
    }
}

fn is_harborbeacon_state_path(path: &Path) -> bool {
    path.to_string_lossy().contains(".harborbeacon")
}

fn legacy_state_path(path: &Path) -> PathBuf {
    PathBuf::from(
        path.to_string_lossy()
            .replace(".harborbeacon", ".harbornas"),
    )
}

fn parse_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=')?;
        if name == key {
            return Some(value.to_string());
        }
    }
    None
}

fn request_identity_hints(url: &str, headers: &[Header]) -> AccessIdentityHints {
    AccessIdentityHints {
        user_id: header_value(headers, "X-Harbor-User-Id")
            .or_else(|| parse_query_param(url, "user_id"))
            .and_then(percent_decode_optional_query_value),
        open_id: header_value(headers, "X-Harbor-Open-Id")
            .or_else(|| parse_query_param(url, "open_id"))
            .and_then(percent_decode_optional_query_value),
    }
}

fn header_value(headers: &[Header], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.field.as_str().to_string().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

fn percent_decode_optional_query_value(value: String) -> Option<String> {
    percent_decode_path_segment(&value).ok().or(Some(value))
}

fn identity_query_suffix(url: &str) -> String {
    let mut pairs = Vec::new();
    if let Some(open_id) = parse_query_param(url, "open_id").filter(|value| !value.is_empty()) {
        pairs.push(format!("open_id={open_id}"));
    }
    if let Some(user_id) = parse_query_param(url, "user_id").filter(|value| !value.is_empty()) {
        pairs.push(format!("user_id={user_id}"));
    }
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

fn parse_approval_decision_path(path: &str, action: &str) -> Option<String> {
    let prefix = "/api/tasks/approvals/";
    let suffix = format!("/{action}");
    let approval_id = path.strip_prefix(prefix)?.strip_suffix(&suffix)?.trim();
    if approval_id.is_empty() {
        return None;
    }
    percent_decode_path_segment(approval_id).ok()
}

fn parse_member_role_update_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/access/members/")?;
    let user_id = trimmed.strip_suffix("/role")?.trim();
    if user_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(user_id).ok()
    }
}

fn parse_role_kind(value: &str) -> Result<RoleKind, String> {
    match value.trim().to_lowercase().replace('-', "_").as_str() {
        "admin" => Ok(RoleKind::Admin),
        "operator" => Ok(RoleKind::Operator),
        "member" => Ok(RoleKind::Member),
        "viewer" => Ok(RoleKind::Viewer),
        "guest" => Ok(RoleKind::Guest),
        "owner" => Err("当前入口不支持直接设置 owner 角色".to_string()),
        _ => Err(format!("unknown role_kind: {}", value.trim())),
    }
}

fn build_access_member_summaries(state: &AdminConsoleState) -> Vec<AccessMemberSummary> {
    let workspace = state
        .platform
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == "home-1")
        .or_else(|| state.platform.workspaces.first());
    let owner_user_id = workspace
        .map(|workspace| workspace.owner_user_id.as_str())
        .unwrap_or("local-owner");

    let mut members: Vec<AccessMemberSummary> = state
        .platform
        .memberships
        .iter()
        .filter(|membership| membership.workspace_id == "home-1")
        .map(|membership| {
            let user = state
                .platform
                .users
                .iter()
                .find(|user| user.user_id == membership.user_id);
            let identity_binding = state
                .platform
                .identity_bindings
                .iter()
                .find(|binding| binding.user_id == membership.user_id);

            AccessMemberSummary {
                user_id: membership.user_id.clone(),
                display_name: user
                    .map(|user| user.display_name.clone())
                    .or_else(|| {
                        identity_binding
                            .and_then(|binding| binding.profile_snapshot.get("display_name"))
                            .and_then(Value::as_str)
                            .map(|value| value.to_string())
                    })
                    .unwrap_or_else(|| membership.user_id.clone()),
                role_kind: role_kind_value(membership.role_kind).to_string(),
                membership_status: membership_status_value(membership.status).to_string(),
                source: identity_binding
                    .map(|binding| binding.provider_key.clone())
                    .unwrap_or_else(|| "local_console".to_string()),
                open_id: identity_binding.map(|binding| binding.external_user_id.clone()),
                chat_id: identity_binding.and_then(|binding| binding.external_chat_id.clone()),
                can_edit: membership.user_id != owner_user_id,
                is_owner: membership.user_id == owner_user_id
                    || membership.role_kind == RoleKind::Owner,
            }
        })
        .collect();

    members.sort_by(|left, right| {
        right
            .is_owner
            .cmp(&left.is_owner)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    members
}

fn role_kind_value(role_kind: RoleKind) -> &'static str {
    match role_kind {
        RoleKind::Owner => "owner",
        RoleKind::Admin => "admin",
        RoleKind::Operator => "operator",
        RoleKind::Member => "member",
        RoleKind::Viewer => "viewer",
        RoleKind::Guest => "guest",
    }
}

fn membership_status_value(status: MembershipStatus) -> &'static str {
    match status {
        MembershipStatus::Active => "active",
        MembershipStatus::Pending => "pending",
        MembershipStatus::Revoked => "revoked",
    }
}

fn render_mobile_setup_page(
    state: &AdminConsoleState,
    setup_url: &str,
    static_setup_url: &str,
    session_code: &str,
) -> String {
    let bot_name = if state.bridge_provider.app_name.trim().is_empty() {
        "尚未配置".to_string()
    } else {
        html_escape(&state.bridge_provider.app_name)
    };
    let platform = if state.bridge_provider.platform.trim().is_empty() {
        "未配置".to_string()
    } else {
        html_escape(&state.bridge_provider.platform)
    };
    let gateway_url = if state.bridge_provider.gateway_base_url.trim().is_empty() {
        "尚未配置".to_string()
    } else {
        html_escape(&state.bridge_provider.gateway_base_url)
    };
    let capability_summary = format!(
        "reply={} / update={} / attachments={}",
        yes_no(state.bridge_provider.capabilities.reply),
        yes_no(state.bridge_provider.capabilities.update),
        yes_no(state.bridge_provider.capabilities.attachments)
    );
    let capability_summary = html_escape(&capability_summary);
    let status = html_escape(&state.binding.metric);
    let session_code = html_escape(session_code);
    let setup_url = html_escape(setup_url);
    let static_setup_url = html_escape(static_setup_url);
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>HarborBeacon HarborGate 状态</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, sans-serif; background: #f4efe7; color: #1e1b18; margin: 0; }}
    .wrap {{ max-width: 560px; margin: 0 auto; padding: 24px 18px 40px; }}
    .card {{ background: rgba(255,255,255,0.9); border-radius: 20px; padding: 20px; box-shadow: 0 18px 48px rgba(51,36,18,0.12); }}
    h1 {{ margin: 0 0 8px; font-size: 28px; }}
    p {{ line-height: 1.5; }}
    .meta {{ color: #6b5a49; font-size: 14px; margin-bottom: 18px; }}
    label {{ display: block; margin: 14px 0 8px; font-weight: 600; }}
    input {{ width: 100%; box-sizing: border-box; padding: 14px 12px; border-radius: 12px; border: 1px solid #d9c6ae; font-size: 16px; }}
    button {{ width: 100%; margin-top: 18px; padding: 14px 16px; border: 0; border-radius: 999px; background: #1f7a6f; color: white; font-size: 16px; font-weight: 700; }}
    .status {{ margin: 16px 0; padding: 12px 14px; border-radius: 14px; background: #f6f2ec; }}
    .hint {{ font-size: 13px; color: #766757; }}
    .ok {{ color: #1f7a6f; }}
    .err {{ color: #b94739; }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="card">
      <div class="meta">HarborBeacon · 手机配置页</div>
      <h1>查看 HarborGate 状态</h1>
      <p>HarborBeacon 不再保存平台侧 <code>app_id</code> / <code>app_secret</code>。这个页面只会让 HarborBeacon 后端去读取 HarborGate 的脱敏状态，并同步显示连接结果。</p>
      <div class="status">
        <div><strong>当前状态：</strong><span id="status-text">{status}</span></div>
        <div><strong>当前会话：</strong>{session_code}</div>
        <div><strong>连接显示名：</strong><span id="bot-name">{bot_name}</span></div>
        <div><strong>平台：</strong><span id="platform-name">{platform}</span></div>
        <div><strong>Gateway：</strong><span id="gateway-url">{gateway_url}</span></div>
        <div><strong>能力：</strong><span id="capabilities">{capability_summary}</span></div>
      </div>
      <p class="hint">当前打开的是本地配置入口：<code>{setup_url}</code></p>
      <button id="submit-btn">刷新 Gateway 状态</button>
      <p class="hint">点击后，HarborBeacon 会使用服务端配置的 HarborGate 地址与服务 token 访问 <code>GET /api/gateway/status</code>。返回结果必须是脱敏状态，不会把原始平台凭据写进 HarborBeacon。</p>
      <p class="hint">如果这里刷新失败，请检查 HarborBeacon 机器上的 <code>HARBORGATE_BASE_URL</code> 和 <code>HARBORGATE_BEARER_TOKEN</code>，以及 HarborGate 是否已实现推荐的状态接口。静态配置页仍然可以固定在 <code>{static_setup_url}</code>。</p>
      <p id="result" class="hint"></p>
    </div>
  </div>
  <script>
    document.getElementById('submit-btn').addEventListener('click', async () => {{
      const result = document.getElementById('result');
      result.className = 'hint';
      result.textContent = '正在刷新 HarborGate 状态...';
      try {{
        const response = await fetch('/api/bridge/configure', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: JSON.stringify({{}})
        }});
        const payload = await response.json();
        if (!response.ok) {{
          throw new Error(payload.error || '刷新失败');
        }}
        document.getElementById('status-text').textContent = payload.binding.metric || 'Gateway 在线';
        document.getElementById('bot-name').textContent = payload.bridge_provider?.app_name || '未配置';
        document.getElementById('platform-name').textContent = payload.bridge_provider?.platform || '未配置';
        document.getElementById('gateway-url').textContent = payload.bridge_provider?.gateway_base_url || '尚未配置';
        const caps = payload.bridge_provider?.capabilities || {{}};
        document.getElementById('capabilities').textContent = `reply=${{caps.reply ? 'yes' : 'no'}} / update=${{caps.update ? 'yes' : 'no'}} / attachments=${{caps.attachments ? 'yes' : 'no'}}`;
        result.className = 'hint ok';
        result.textContent = 'HarborGate 状态已刷新，HarborBeacon 只保存了脱敏状态。';
      }} catch (error) {{
        result.className = 'hint err';
        result.textContent = error.message;
      }}
    }});
  </script>
</body>
</html>"#
    )
}

fn render_live_view_page(
    public_origin: &str,
    device: &harborbeacon_local_agent::runtime::registry::CameraDevice,
    identity_query: &str,
) -> String {
    let device_label = device.room.as_deref().unwrap_or(device.name.as_str());
    let device_label = html_escape(device_label);
    let device_name = html_escape(&device.name);
    let ip_address = html_escape(device.ip_address.as_deref().unwrap_or("未知 IP"));
    let device_id = url_encode_path_segment(&device.device_id);
    let origin = public_origin.trim_end_matches('/');
    let live_stream_url = format!("{origin}/api/cameras/{device_id}/live.mjpeg{identity_query}");
    let snapshot_url = format!("{origin}/api/cameras/{device_id}/snapshot.jpg{identity_query}");

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{device_label} 实时观看</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #0f1720;
      --card: rgba(10, 18, 28, 0.82);
      --line: rgba(255,255,255,0.12);
      --text: #f3f7fb;
      --muted: #98a8bb;
      --accent: #4fd1c5;
      --danger: #ff8f70;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif;
      background:
        radial-gradient(circle at top, rgba(79,209,197,0.22), transparent 35%),
        linear-gradient(180deg, #0c1218 0%, #101927 100%);
      color: var(--text);
      min-height: 100vh;
    }}
    .wrap {{ max-width: 880px; margin: 0 auto; padding: 20px 16px 28px; }}
    .topbar {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      margin-bottom: 14px;
    }}
    .title {{ margin: 0; font-size: 28px; line-height: 1.1; }}
    .meta {{ color: var(--muted); font-size: 14px; margin-top: 6px; }}
    .status {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      border-radius: 999px;
      background: rgba(255,255,255,0.08);
      border: 1px solid rgba(255,255,255,0.1);
      font-size: 13px;
      white-space: nowrap;
    }}
    .status-dot {{
      width: 10px;
      height: 10px;
      border-radius: 999px;
      background: var(--danger);
      box-shadow: 0 0 0 4px rgba(255,143,112,0.18);
    }}
    .status.live .status-dot {{
      background: var(--accent);
      box-shadow: 0 0 0 4px rgba(79,209,197,0.18);
    }}
    .panel {{
      background: var(--card);
      border: 1px solid var(--line);
      border-radius: 24px;
      padding: 14px;
      box-shadow: 0 24px 60px rgba(0,0,0,0.28);
      backdrop-filter: blur(12px);
    }}
    .viewer {{
      position: relative;
      overflow: hidden;
      border-radius: 18px;
      background: #060b11;
      aspect-ratio: 16 / 9;
    }}
    .viewer img {{
      width: 100%;
      height: 100%;
      display: block;
      object-fit: contain;
      background: #060b11;
    }}
    .overlay {{
      position: absolute;
      left: 12px;
      right: 12px;
      bottom: 12px;
      display: flex;
      justify-content: space-between;
      gap: 12px;
      flex-wrap: wrap;
    }}
    .chip {{
      padding: 8px 10px;
      border-radius: 12px;
      background: rgba(3, 10, 17, 0.72);
      border: 1px solid rgba(255,255,255,0.1);
      font-size: 13px;
      color: var(--muted);
    }}
    .actions {{
      display: flex;
      gap: 10px;
      flex-wrap: wrap;
      margin-top: 14px;
    }}
    .actions button, .actions a {{
      appearance: none;
      border: 0;
      text-decoration: none;
      color: var(--text);
      background: rgba(255,255,255,0.08);
      border: 1px solid rgba(255,255,255,0.1);
      border-radius: 999px;
      padding: 11px 16px;
      font-size: 14px;
      font-weight: 600;
    }}
    .actions .primary {{
      background: linear-gradient(135deg, #27c4b3, #1f8f85);
      border-color: transparent;
    }}
    .hint {{
      margin-top: 14px;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.6;
    }}
    @media (max-width: 640px) {{
      .title {{ font-size: 22px; }}
      .topbar {{ align-items: flex-start; flex-direction: column; }}
      .actions button, .actions a {{ flex: 1 1 calc(50% - 10px); text-align: center; }}
    }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="topbar">
      <div>
        <h1 class="title">{device_label} 实时观看</h1>
        <div class="meta">{device_name} · {ip_address} · 浏览器内低延迟 MJPEG 预览</div>
      </div>
      <div id="status" class="status">
        <span class="status-dot"></span>
        <span id="status-text">正在连接画面…</span>
      </div>
    </div>

    <div class="panel">
      <div class="viewer">
        <img id="stream" src="{live_stream_url}" alt="{device_label} 实时画面" />
        <div class="overlay">
          <div class="chip">链路：RTSP → 本地 ffmpeg → MJPEG</div>
          <div class="chip" id="last-frame">等待首帧…</div>
        </div>
      </div>

      <div class="actions">
        <button id="reload-btn" class="primary" type="button">重连画面</button>
        <a href="{snapshot_url}" target="_blank" rel="noreferrer">打开当前截图</a>
      </div>

      <div class="hint">
        如果画面没有出来，先确认手机和 HarborBeacon 在同一个局域网，再点击“重连画面”。
        这个页面只负责看实时视频；拍照、录像、云台控制仍然建议继续在统一 IM 入口里完成。
      </div>
    </div>
  </div>

  <script>
    const streamUrl = {live_stream_url:?};
    const reloadSeparator = streamUrl.includes('?') ? '&' : '?';
    const streamEl = document.getElementById('stream');
    const statusEl = document.getElementById('status');
    const statusTextEl = document.getElementById('status-text');
    const lastFrameEl = document.getElementById('last-frame');

    function setStatus(isLive, text) {{
      statusEl.classList.toggle('live', isLive);
      statusTextEl.textContent = text;
    }}

    function reloadStream() {{
      setStatus(false, '正在重连画面…');
      streamEl.src = `${{streamUrl}}${{reloadSeparator}}ts=${{Date.now()}}`;
    }}

    streamEl.addEventListener('load', () => {{
      setStatus(true, '实时画面连接中');
      lastFrameEl.textContent = `最后更新：${{new Date().toLocaleTimeString()}}`;
    }});

    streamEl.addEventListener('error', () => {{
      setStatus(false, '画面连接失败，请重试');
    }});

    document.getElementById('reload-btn').addEventListener('click', reloadStream);
  </script>
</body>
</html>"#
    )
}

fn render_shared_live_view_page(
    share_token: &str,
    device: &harborbeacon_local_agent::runtime::registry::CameraDevice,
) -> String {
    let device_label = html_escape(device.room.as_deref().unwrap_or(device.name.as_str()));
    let device_name = html_escape(&device.name);
    let ip_address = html_escape(device.ip_address.as_deref().unwrap_or("未知 IP"));
    let live_stream_url = format!(
        "/shared/cameras/{}/live.mjpeg",
        url_encode_path_segment(share_token)
    );

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{device_label} 远程观看</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #0d1119;
      --card: rgba(7, 14, 24, 0.86);
      --line: rgba(255,255,255,0.12);
      --text: #eff6ff;
      --muted: #98a7ba;
      --accent: #58d6c2;
      --danger: #ff9f79;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      min-height: 100vh;
      color: var(--text);
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif;
      background:
        radial-gradient(circle at top, rgba(88,214,194,0.16), transparent 34%),
        linear-gradient(180deg, #0b1118 0%, #0e1723 100%);
    }}
    .wrap {{ max-width: 880px; margin: 0 auto; padding: 18px 14px 28px; }}
    .header {{ display: flex; justify-content: space-between; gap: 12px; align-items: flex-start; margin-bottom: 14px; }}
    .title {{ margin: 0; font-size: 28px; line-height: 1.1; }}
    .meta {{ margin-top: 8px; color: var(--muted); font-size: 14px; line-height: 1.5; }}
    .status {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      border-radius: 999px;
      background: rgba(255,255,255,0.08);
      border: 1px solid rgba(255,255,255,0.1);
      font-size: 13px;
      white-space: nowrap;
    }}
    .status-dot {{
      width: 10px;
      height: 10px;
      border-radius: 999px;
      background: var(--danger);
      box-shadow: 0 0 0 4px rgba(255,159,121,0.16);
    }}
    .status.live .status-dot {{
      background: var(--accent);
      box-shadow: 0 0 0 4px rgba(88,214,194,0.18);
    }}
    .panel {{
      padding: 14px;
      border-radius: 24px;
      background: var(--card);
      border: 1px solid var(--line);
      box-shadow: 0 24px 64px rgba(0,0,0,0.28);
      backdrop-filter: blur(12px);
    }}
    .viewer {{
      position: relative;
      overflow: hidden;
      border-radius: 18px;
      background: #05080d;
      aspect-ratio: 16 / 9;
    }}
    .viewer img {{
      width: 100%;
      height: 100%;
      object-fit: contain;
      display: block;
      background: #05080d;
    }}
    .overlay {{
      position: absolute;
      left: 12px;
      right: 12px;
      bottom: 12px;
      display: flex;
      justify-content: space-between;
      gap: 12px;
      flex-wrap: wrap;
    }}
    .chip {{
      padding: 8px 10px;
      border-radius: 12px;
      background: rgba(3, 10, 17, 0.74);
      border: 1px solid rgba(255,255,255,0.08);
      color: var(--muted);
      font-size: 13px;
    }}
    .actions {{ display: flex; gap: 10px; flex-wrap: wrap; margin-top: 14px; }}
    .actions button {{
      appearance: none;
      border: 0;
      border-radius: 999px;
      padding: 11px 16px;
      font-size: 14px;
      font-weight: 600;
      color: var(--text);
      background: linear-gradient(135deg, #28c6b5, #1d8d82);
    }}
    .hint {{ margin-top: 14px; color: var(--muted); font-size: 13px; line-height: 1.6; }}
    @media (max-width: 640px) {{
      .title {{ font-size: 22px; }}
      .header {{ flex-direction: column; }}
    }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="header">
      <div>
        <h1 class="title">{device_label} 远程观看</h1>
        <div class="meta">{device_name} · {ip_address} · 这是一个带签名的临时分享链接，仅用于看实时画面。</div>
      </div>
      <div id="status" class="status">
        <span class="status-dot"></span>
        <span id="status-text">正在连接画面…</span>
      </div>
    </div>

    <div class="panel">
      <div class="viewer">
        <img id="stream" src="{live_stream_url}" alt="{device_label} 远程实时画面" />
        <div class="overlay">
          <div class="chip">链路：公网入口 → 本地 ffmpeg → MJPEG</div>
          <div class="chip" id="last-frame">等待首帧…</div>
        </div>
      </div>

      <div class="actions">
        <button id="reload-btn" type="button">重连画面</button>
      </div>

      <div class="hint">
        这个链接默认会在一段时间后自动过期。分享出去时请只发给需要查看的人，不要长期公开传播。
      </div>
    </div>
  </div>

  <script>
    const streamUrl = {live_stream_url:?};
    const streamEl = document.getElementById('stream');
    const statusEl = document.getElementById('status');
    const statusTextEl = document.getElementById('status-text');
    const lastFrameEl = document.getElementById('last-frame');

    function setStatus(isLive, text) {{
      statusEl.classList.toggle('live', isLive);
      statusTextEl.textContent = text;
    }}

    function reloadStream() {{
      setStatus(false, '正在重连画面…');
      streamEl.src = `${{streamUrl}}?ts=${{Date.now()}}`;
    }}

    streamEl.addEventListener('load', () => {{
      setStatus(true, '远程画面连接中');
      lastFrameEl.textContent = `最后更新：${{new Date().toLocaleTimeString()}}`;
    }});

    streamEl.addEventListener('error', () => {{
      setStatus(false, '画面连接失败，请重试');
    }});

    document.getElementById('reload-btn').addEventListener('click', reloadStream);
  </script>
</body>
</html>"#
    )
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => {
                let _ = escaped.write_char(ch);
            }
        }
    }
    escaped
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn qr_to_svg(qr: &QrCode, border: i32) -> String {
    let size = qr.size();
    let dimension = size + border * 2;
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                let _ = write!(&mut path, "M{},{}h1v1h-1z ", x + border, y + border);
            }
        }
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {d} {d}" shape-rendering="crispEdges"><rect width="100%" height="100%" fill="#f8f3ec"/><path d="{path}" fill="#1b1814"/></svg>"##,
        d = dimension,
        path = path.trim()
    )
}

fn slugify_identity(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        format!("ou_local_{}", value.chars().count())
    } else {
        format!("ou_local_{slug}")
    }
}

fn read_json_body<T: for<'de> Deserialize<'de>>(request: &mut Request) -> Result<T, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|e| format!("failed to read request body: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("invalid JSON body: {e}"))
}

fn read_json_body_or_default<T>(request: &mut Request) -> Result<T, String>
where
    T: for<'de> Deserialize<'de> + Default,
{
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|e| format!("failed to read request body: {e}"))?;
    if body.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(&body).map_err(|e| format!("invalid JSON body: {e}"))
}

fn ok_json(payload: &impl Serialize) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(StatusCode(200), payload)
}

fn image_response(
    status: StatusCode,
    bytes: Vec<u8>,
    mime_type: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_data(bytes).with_status_code(status);
    add_common_headers(&mut response);
    response.add_header(
        Header::from_bytes(b"Content-Type".as_slice(), mime_type.as_bytes()).expect("header"),
    );
    response
}

fn no_content() -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(StatusCode(204));
    add_common_headers(&mut response);
    response
}

fn error_json(status: StatusCode, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(status, &json!({ "error": message }))
}

fn json_response(
    status: StatusCode,
    payload: &impl Serialize,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec_pretty(payload)
        .unwrap_or_else(|_| b"{\"error\":\"serialize failed\"}".to_vec());
    let mut response = Response::from_data(body).with_status_code(status);
    add_common_headers(&mut response);
    response.add_header(
        Header::from_bytes(
            b"Content-Type".as_slice(),
            b"application/json; charset=utf-8".as_slice(),
        )
        .expect("header"),
    );
    response
}

fn add_common_headers<R: Read>(response: &mut Response<R>) {
    for header in [
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Headers", "Content-Type"),
        ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        ("Cache-Control", "no-store"),
    ] {
        response.add_header(
            Header::from_bytes(header.0.as_bytes(), header.1.as_bytes()).expect("header"),
        );
    }
}

fn is_admin_surface_path(path: &str) -> bool {
    path == "/api/state"
        || path == "/api/access/members"
        || path == "/api/share-links"
        || path == "/api/binding/qr.svg"
        || path == "/api/binding/static-qr.svg"
        || path == "/setup/mobile"
        || path == "/api/binding/refresh"
        || path == "/api/binding/demo-bind"
        || path == "/api/binding/test-bind"
        || path == "/api/bridge/configure"
        || (path.starts_with("/api/access/members/") && path.ends_with("/role"))
        || path == "/api/tasks/approvals"
        || path.starts_with("/api/tasks/approvals/")
        || path == "/api/discovery/scan"
        || path == "/api/devices/manual"
        || (path.starts_with("/api/cameras/") && path.ends_with("/share-link"))
        || (path.starts_with("/api/share-links/") && path.ends_with("/revoke"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/snapshot"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/analyze"))
        || path == "/api/defaults"
}

fn build_share_link_summary(
    share_link: ShareLink,
    media_session: &MediaSession,
    device_name: Option<&String>,
    now_unix_secs: u64,
) -> ShareLinkSummary {
    let status = share_link_status(&share_link, media_session, now_unix_secs);
    ShareLinkSummary {
        share_link_id: share_link.share_link_id.clone(),
        media_session_id: media_session.media_session_id.clone(),
        device_id: media_session.device_id.clone(),
        device_name: device_name
            .cloned()
            .unwrap_or_else(|| media_session.device_id.clone()),
        opened_by_user_id: media_session.opened_by_user_id.clone(),
        access_scope: serde_json::to_value(share_link.access_scope)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "public_link".to_string()),
        session_status: serde_json::to_value(media_session.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string()),
        status: status.to_string(),
        expires_at: share_link.expires_at.clone(),
        revoked_at: share_link.revoked_at.clone(),
        started_at: media_session.started_at.clone(),
        ended_at: media_session.ended_at.clone(),
        can_revoke: status == "active",
    }
}

fn share_link_status(
    share_link: &ShareLink,
    media_session: &MediaSession,
    now_unix_secs: u64,
) -> &'static str {
    if share_link
        .revoked_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return "revoked";
    }

    if let Some(expires_at) = share_link.expires_at.as_deref() {
        if let Ok(expires_at) = expires_at.trim().parse::<u64>() {
            if now_unix_secs > expires_at {
                return "expired";
            }
        }
    }

    match media_session.status {
        MediaSessionStatus::Opening | MediaSessionStatus::Active => "active",
        MediaSessionStatus::Closed => "closed",
        MediaSessionStatus::Failed => "failed",
    }
}

fn ensure_local_admin_access(
    remote_addr: Option<SocketAddr>,
    headers: &[Header],
) -> Result<(), String> {
    if has_forwarding_headers(headers) {
        return Err(
            "当前管理后台接口只允许在本机或局域网内直连访问，不能通过公网反向代理转发。"
                .to_string(),
        );
    }

    if remote_addr.is_none() || remote_addr.is_some_and(is_local_socket_addr) {
        return Ok(());
    }

    Err("当前管理后台接口只允许本机或局域网内访问。".to_string())
}

fn ensure_local_camera_access(
    remote_addr: Option<SocketAddr>,
    headers: &[Header],
) -> Result<(), String> {
    if has_forwarding_headers(headers) {
        return Err("当前摄像头直连预览只允许本机或局域网直连访问；如果要给外网用户观看，请使用带签名的共享链接。".to_string());
    }

    if remote_addr.is_none() || remote_addr.is_some_and(is_local_socket_addr) {
        return Ok(());
    }

    Err(
        "当前摄像头直连预览只允许本机或局域网访问；如果要给外网用户观看，请使用带签名的共享链接。"
            .to_string(),
    )
}

fn is_local_socket_addr(addr: SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(ip) => ip.is_loopback() || is_private_ipv4(ip),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
    }
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

fn has_forwarding_headers(headers: &[Header]) -> bool {
    headers.iter().any(|header| {
        header.field.equiv("Forwarded")
            || header.field.equiv("X-Forwarded-For")
            || header.field.equiv("X-Forwarded-Host")
            || header.field.equiv("X-Forwarded-Proto")
            || header.field.equiv("X-Real-Ip")
    })
}

fn redact_state_snapshot(mut state: StateResponse) -> StateResponse {
    state.defaults.rtsp_password.clear();
    state.bridge_provider = redact_bridge_provider_config(state.bridge_provider);
    state
}

fn redact_bridge_provider_config(mut config: BridgeProviderConfig) -> BridgeProviderConfig {
    config.app_id.clear();
    config.app_secret.clear();
    config.bot_open_id.clear();
    config
}

fn task_error_message(response: &TaskResponse) -> String {
    response
        .prompt
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| response.result.message.clone())
}

fn parse_scan_results(
    data: &Value,
) -> Result<Vec<harborbeacon_local_agent::runtime::hub::HubScanResultItem>, String> {
    let value = data
        .pointer("/candidates")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    serde_json::from_value(value)
        .map_err(|error| format!("failed to parse camera scan results from task response: {error}"))
}

fn parse_connected_device(data: &Value) -> Result<CameraDevice, String> {
    let value = data
        .pointer("/device")
        .cloned()
        .ok_or_else(|| "task response missing connected device payload".to_string())?;
    serde_json::from_value(value)
        .map_err(|error| format!("failed to parse connected camera from task response: {error}"))
}

fn parse_camera_snapshot_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/snapshot.jpg")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_camera_live_stream_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/live.mjpeg")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_camera_analyze_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/analyze")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_camera_task_snapshot_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/snapshot")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_camera_share_link_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/share-link")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_share_link_revoke_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/share-links/")?;
    let share_link_id = trimmed.strip_suffix("/revoke")?;
    if share_link_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(share_link_id).ok()
    }
}

fn parse_camera_live_page_path(path: &str) -> Option<String> {
    let device_id = path.strip_prefix("/live/cameras/")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_shared_camera_live_page_path(path: &str) -> Option<String> {
    let token = path.strip_prefix("/shared/cameras/")?;
    if token.is_empty() || token.contains('/') {
        None
    } else {
        percent_decode_path_segment(token).ok()
    }
}

fn parse_shared_camera_live_stream_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/shared/cameras/")?;
    let token = trimmed.strip_suffix("/live.mjpeg")?;
    if token.is_empty() || token.contains('/') {
        None
    } else {
        percent_decode_path_segment(token).ok()
    }
}

fn url_encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn percent_decode_path_segment(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("incomplete percent escape".to_string());
            }
            let hi = decode_hex(bytes[index + 1]).ok_or_else(|| "invalid hex digit".to_string())?;
            let lo = decode_hex(bytes[index + 2]).ok_or_else(|| "invalid hex digit".to_string())?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).map_err(|error| format!("invalid utf-8 path segment: {error}"))
}

fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

struct FfmpegMjpegStream {
    child: Child,
    stdout: ChildStdout,
}

impl FfmpegMjpegStream {
    fn spawn(stream_url: &str) -> Result<Self, String> {
        if which::which("ffmpeg").is_err() {
            return Err("当前机器缺少 ffmpeg".to_string());
        }

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-rtsp_transport",
                "tcp",
                "-fflags",
                "nobuffer",
                "-flags",
                "low_delay",
                "-i",
                stream_url,
                "-an",
                "-vf",
                "fps=5,scale=960:-2:flags=fast_bilinear",
                "-q:v",
                "6",
                "-f",
                "mpjpeg",
                "-boundary_tag",
                "ffmpeg",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("启动实时转码 ffmpeg 失败: {error}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "无法获取 ffmpeg 输出管道".to_string())?;

        Ok(Self { child, stdout })
    }
}

impl Read for FfmpegMjpegStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stdout.read(buf)
    }
}

impl Drop for FfmpegMjpegStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_local_admin_access, ensure_local_camera_access, has_forwarding_headers,
        identity_query_suffix, is_admin_surface_path, parse_approval_decision_path,
        parse_camera_analyze_path, parse_camera_live_page_path, parse_camera_live_stream_path,
        parse_camera_share_link_path, parse_camera_snapshot_path, parse_camera_task_snapshot_path,
        parse_member_role_update_path, parse_share_link_revoke_path,
        parse_shared_camera_live_page_path, parse_shared_camera_live_stream_path,
        percent_decode_path_segment, redact_bridge_provider_config, request_identity_hints,
        url_encode_path_segment, AdminApi,
    };
    use harborbeacon_local_agent::control_plane::media::{
        MediaDeliveryMode, MediaSession, MediaSessionKind, MediaSessionStatus, ShareAccessScope,
        ShareLink,
    };
    use harborbeacon_local_agent::runtime::admin_console::{AdminConsoleStore, RemoteViewConfig};
    use harborbeacon_local_agent::runtime::registry::DeviceRegistryStore;
    use harborbeacon_local_agent::runtime::remote_view;
    use harborbeacon_local_agent::runtime::task_api::TaskApiService;
    use harborbeacon_local_agent::runtime::task_session::TaskConversationStore;
    use serde_json::json;
    use std::fs;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tiny_http::Header;

    fn unique_store_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}.json"))
    }

    #[test]
    fn camera_paths_decode_percent_encoded_device_ids() {
        let encoded = "camera%201%2Fleft";
        assert_eq!(
            parse_camera_snapshot_path(&format!("/api/cameras/{encoded}/snapshot.jpg")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_task_snapshot_path(&format!("/api/cameras/{encoded}/snapshot")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_share_link_path(&format!("/api/cameras/{encoded}/share-link")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_analyze_path(&format!("/api/cameras/{encoded}/analyze")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_live_stream_path(&format!("/api/cameras/{encoded}/live.mjpeg")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_live_page_path(&format!("/live/cameras/{encoded}")),
            Some("camera 1/left".to_string())
        );
    }

    #[test]
    fn shared_camera_paths_decode_tokens() {
        let encoded = "abc.def%2D123";
        assert_eq!(
            parse_shared_camera_live_page_path(&format!("/shared/cameras/{encoded}")),
            Some("abc.def-123".to_string())
        );
        assert_eq!(
            parse_shared_camera_live_stream_path(&format!("/shared/cameras/{encoded}/live.mjpeg")),
            Some("abc.def-123".to_string())
        );
        assert_eq!(
            parse_share_link_revoke_path("/api/share-links/share-link-1/revoke"),
            Some("share-link-1".to_string())
        );
    }

    #[test]
    fn path_segment_round_trips_utf8_content() {
        let raw = "客厅 camera #1";
        let encoded = url_encode_path_segment(raw);
        assert_eq!(percent_decode_path_segment(&encoded).as_deref(), Ok(raw));
    }

    #[test]
    fn direct_camera_access_is_restricted_to_local_clients() {
        let local = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 3, 12), 4567));
        let remote = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(8, 8, 8, 8), 4567));
        assert!(ensure_local_camera_access(Some(local), &[]).is_ok());
        assert!(ensure_local_camera_access(Some(remote), &[]).is_err());
    }

    #[test]
    fn forwarded_headers_block_local_only_routes() {
        let forwarded =
            vec![
                Header::from_bytes(b"X-Forwarded-For".as_slice(), b"198.51.100.10".as_slice())
                    .expect("header"),
            ];
        let local = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4567));
        assert!(has_forwarding_headers(&forwarded));
        assert!(ensure_local_admin_access(Some(local), &forwarded).is_err());
        assert!(ensure_local_camera_access(Some(local), &forwarded).is_err());
    }

    #[test]
    fn bridge_provider_config_redacts_secret() {
        let redacted = redact_bridge_provider_config(
            harborbeacon_local_agent::runtime::admin_console::BridgeProviderConfig {
                configured: true,
                connected: true,
                platform: "feishu".to_string(),
                gateway_base_url: "http://gateway.local:4180".to_string(),
                app_id: "cli_xxx".to_string(),
                app_secret: "super-secret".to_string(),
                app_name: "HarborBeacon Bot".to_string(),
                bot_open_id: "ou_xxx".to_string(),
                status: "已连接".to_string(),
                last_checked_at: "2026-04-18T10:00:00Z".to_string(),
                capabilities:
                    harborbeacon_local_agent::runtime::admin_console::BridgeProviderCapabilities {
                        reply: true,
                        update: true,
                        attachments: true,
                    },
            },
        );
        assert_eq!(redacted.app_id, "");
        assert_eq!(redacted.app_secret, "");
        assert_eq!(redacted.bot_open_id, "");
    }

    #[test]
    fn approval_decision_paths_decode_ids() {
        let encoded = "approval%2F1";
        assert_eq!(
            parse_approval_decision_path(
                &format!("/api/tasks/approvals/{encoded}/approve"),
                "approve"
            ),
            Some("approval/1".to_string())
        );
        assert_eq!(
            parse_approval_decision_path(
                &format!("/api/tasks/approvals/{encoded}/reject"),
                "reject"
            ),
            Some("approval/1".to_string())
        );
    }

    #[test]
    fn approval_routes_are_admin_surface_paths() {
        assert!(is_admin_surface_path("/api/tasks/approvals"));
        assert!(is_admin_surface_path("/api/access/members"));
        assert!(is_admin_surface_path("/api/share-links"));
        assert!(is_admin_surface_path("/api/access/members/user-1/role"));
        assert!(is_admin_surface_path(
            "/api/tasks/approvals/approval-1/approve"
        ));
        assert!(is_admin_surface_path(
            "/api/tasks/approvals/approval-1/reject"
        ));
        assert!(is_admin_surface_path("/api/cameras/camera-1/share-link"));
        assert!(is_admin_surface_path(
            "/api/share-links/share-link-1/revoke"
        ));
        assert!(is_admin_surface_path("/api/cameras/camera-1/snapshot"));
        assert!(is_admin_surface_path("/api/cameras/camera-1/analyze"));
    }

    #[test]
    fn member_role_paths_decode_ids() {
        let encoded = "user%2F1";
        assert_eq!(
            parse_member_role_update_path(&format!("/api/access/members/{encoded}/role")),
            Some("user/1".to_string())
        );
    }

    #[test]
    fn request_identity_hints_prefer_headers_then_query() {
        let headers = vec![
            Header::from_bytes(b"X-Harbor-Open-Id".as_slice(), b"ou_header".as_slice())
                .expect("header"),
            Header::from_bytes(b"X-Harbor-User-Id".as_slice(), b"user-header".as_slice())
                .expect("header"),
        ];

        let hints = request_identity_hints(
            "/live/cameras/cam-1?open_id=ou_query&user_id=user-query",
            &headers,
        );
        assert_eq!(hints.open_id.as_deref(), Some("ou_header"));
        assert_eq!(hints.user_id.as_deref(), Some("user-header"));
    }

    #[test]
    fn identity_query_suffix_keeps_open_id_and_user_id() {
        assert_eq!(
            identity_query_suffix("/live/cameras/cam-1?open_id=ou_demo&user_id=u_demo"),
            "?open_id=ou_demo&user_id=u_demo"
        );
        assert!(identity_query_suffix("/live/cameras/cam-1").is_empty());
    }

    #[test]
    fn verify_shared_camera_token_requires_persisted_active_share_link() {
        let admin_path = unique_store_path("harborbeacon-admin-state");
        let registry_path = unique_store_path("harborbeacon-device-registry");
        let conversation_path = unique_store_path("harborbeacon-task-runtime");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        admin_store
            .save_remote_view_config(RemoteViewConfig {
                share_secret: "platform-share-secret".to_string(),
                share_link_ttl_minutes: 45,
            })
            .expect("save remote view");
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let task_service = TaskApiService::new(admin_store.clone(), conversation_store.clone());
        let api = AdminApi::new(
            admin_store,
            task_service,
            "http://harborbeacon.local:4174".to_string(),
        );

        let issued = remote_view::issue_camera_share_token("platform-share-secret", "cam-1", 15)
            .expect("issue token");
        let media_session = MediaSession {
            media_session_id: "media-session-1".to_string(),
            device_id: "cam-1".to_string(),
            stream_profile_id: "cam-1::stream::primary".to_string(),
            session_kind: MediaSessionKind::Share,
            delivery_mode: MediaDeliveryMode::Hls,
            opened_by_user_id: Some("user-1".to_string()),
            status: MediaSessionStatus::Active,
            share_link_id: Some("share-link-1".to_string()),
            started_at: Some(remote_view::now_unix_secs().to_string()),
            ended_at: None,
            metadata: json!({
                "task_id": "task-1",
            }),
        };
        let share_link = ShareLink {
            share_link_id: "share-link-1".to_string(),
            media_session_id: media_session.media_session_id.clone(),
            token_hash: remote_view::camera_share_token_hash(&issued.token),
            access_scope: ShareAccessScope::PublicLink,
            expires_at: Some(issued.expires_at_unix_secs.to_string()),
            revoked_at: None,
        };
        conversation_store
            .save_share_link_bundle(&media_session, &share_link)
            .expect("save share bundle");

        let claims = api
            .verify_shared_camera_token(&issued.token)
            .expect("claims");
        assert_eq!(claims.device_id, "cam-1");

        conversation_store
            .revoke_share_link(
                "share-link-1",
                Some(remote_view::now_unix_secs().to_string()),
            )
            .expect("revoke");
        assert!(api.verify_shared_camera_token(&issued.token).is_err());

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn list_share_links_surfaces_registered_status() {
        let admin_path = unique_store_path("harborbeacon-admin-state");
        let registry_path = unique_store_path("harborbeacon-device-registry");
        let conversation_path = unique_store_path("harborbeacon-task-runtime");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let task_service = TaskApiService::new(admin_store.clone(), conversation_store.clone());
        let api = AdminApi::new(
            admin_store,
            task_service,
            "http://harborbeacon.local:4174".to_string(),
        );

        conversation_store
            .save_share_link_bundle(
                &MediaSession {
                    media_session_id: "media-session-active".to_string(),
                    device_id: "cam-1".to_string(),
                    stream_profile_id: "cam-1::stream::primary".to_string(),
                    session_kind: MediaSessionKind::Share,
                    delivery_mode: MediaDeliveryMode::Hls,
                    opened_by_user_id: Some("user-1".to_string()),
                    status: MediaSessionStatus::Active,
                    share_link_id: Some("share-link-active".to_string()),
                    started_at: Some(remote_view::now_unix_secs().to_string()),
                    ended_at: None,
                    metadata: json!({}),
                },
                &ShareLink {
                    share_link_id: "share-link-active".to_string(),
                    media_session_id: "media-session-active".to_string(),
                    token_hash: "hash-active".to_string(),
                    access_scope: ShareAccessScope::PublicLink,
                    expires_at: Some((remote_view::now_unix_secs() + 600).to_string()),
                    revoked_at: None,
                },
            )
            .expect("save active share link");
        conversation_store
            .save_share_link_bundle(
                &MediaSession {
                    media_session_id: "media-session-revoked".to_string(),
                    device_id: "cam-1".to_string(),
                    stream_profile_id: "cam-1::stream::primary".to_string(),
                    session_kind: MediaSessionKind::Share,
                    delivery_mode: MediaDeliveryMode::Hls,
                    opened_by_user_id: Some("user-1".to_string()),
                    status: MediaSessionStatus::Closed,
                    share_link_id: Some("share-link-revoked".to_string()),
                    started_at: Some((remote_view::now_unix_secs() - 300).to_string()),
                    ended_at: Some(remote_view::now_unix_secs().to_string()),
                    metadata: json!({}),
                },
                &ShareLink {
                    share_link_id: "share-link-revoked".to_string(),
                    media_session_id: "media-session-revoked".to_string(),
                    token_hash: "hash-revoked".to_string(),
                    access_scope: ShareAccessScope::PublicLink,
                    expires_at: Some((remote_view::now_unix_secs() + 600).to_string()),
                    revoked_at: Some(remote_view::now_unix_secs().to_string()),
                },
            )
            .expect("save revoked share link");
        conversation_store
            .save_share_link_bundle(
                &MediaSession {
                    media_session_id: "media-session-expired".to_string(),
                    device_id: "cam-2".to_string(),
                    stream_profile_id: "cam-2::stream::primary".to_string(),
                    session_kind: MediaSessionKind::Share,
                    delivery_mode: MediaDeliveryMode::Hls,
                    opened_by_user_id: Some("user-2".to_string()),
                    status: MediaSessionStatus::Active,
                    share_link_id: Some("share-link-expired".to_string()),
                    started_at: Some((remote_view::now_unix_secs() - 1200).to_string()),
                    ended_at: None,
                    metadata: json!({}),
                },
                &ShareLink {
                    share_link_id: "share-link-expired".to_string(),
                    media_session_id: "media-session-expired".to_string(),
                    token_hash: "hash-expired".to_string(),
                    access_scope: ShareAccessScope::PublicLink,
                    expires_at: Some((remote_view::now_unix_secs() - 30).to_string()),
                    revoked_at: None,
                },
            )
            .expect("save expired share link");

        let all_links = api.list_share_links(None).expect("list share links");
        assert_eq!(all_links.len(), 3);
        assert!(all_links
            .iter()
            .any(|link| link.share_link_id == "share-link-active" && link.status == "active"));
        assert!(all_links
            .iter()
            .any(|link| link.share_link_id == "share-link-revoked" && link.status == "revoked"));
        assert!(all_links
            .iter()
            .any(|link| link.share_link_id == "share-link-expired" && link.status == "expired"));

        let cam1_links = api.list_share_links(Some("cam-1")).expect("filter links");
        assert_eq!(cam1_links.len(), 2);
        assert!(cam1_links.iter().all(|link| link.device_id == "cam-1"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }
}
