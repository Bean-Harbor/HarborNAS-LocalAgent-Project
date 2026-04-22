use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};

use harborbeacon_local_agent::connectors::im_gateway::GatewayPlatformStatus;
use harborbeacon_local_agent::control_plane::media::{MediaSession, MediaSessionStatus, ShareLink};
use harborbeacon_local_agent::control_plane::models::{ModelEndpoint, ModelRoutePolicy};
use harborbeacon_local_agent::control_plane::users::{MembershipStatus, RoleKind};
use harborbeacon_local_agent::runtime::access_control::{
    authorize_access, AccessAction, AccessIdentityHints, AccessPrincipal,
};
use harborbeacon_local_agent::runtime::admin_console::{
    account_management_snapshot, default_capture_subdirectory, default_clip_length_seconds,
    default_keyframe_count, default_keyframe_interval_seconds, normalize_delivery_surface,
    user_default_delivery_surface, user_recent_interactive_surface, AccountManagementSnapshot,
    AdminConsoleState, AdminConsoleStore, AdminDefaults, BridgeProviderConfig,
    GatewayStatusSummary,
};
use harborbeacon_local_agent::runtime::hub::{
    CameraConnectRequest, CameraHubService, HubManualAddSummary, HubScanRequest, HubScanSummary,
    HubStateSnapshot,
};
use harborbeacon_local_agent::runtime::media_tools::{ffmpeg_resolution_hint, resolve_ffmpeg_bin};
use harborbeacon_local_agent::runtime::model_center::{
    redact_model_center_state, redact_model_endpoint, test_model_endpoint, ModelEndpointTestResult,
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
    harbordesk_dist: PathBuf,
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
        "Usage: agent-hub-admin-api [--bind ADDR] [--admin-state PATH] [--device-registry PATH] [--conversations PATH] [--harbordesk-dist PATH] [--public-origin URL]"
    );
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:4174".to_string(),
            admin_state: PathBuf::from(".harborbeacon/admin-console.json"),
            device_registry: PathBuf::from(".harborbeacon/device-registry.json"),
            conversations: PathBuf::from(".harborbeacon/task-api-conversations.json"),
            harbordesk_dist: PathBuf::from("frontend/harbordesk/dist/harbordesk"),
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
                    cli.conversations = PathBuf::from(value["--conversations=".len()..].to_string())
                }
                "--harbordesk-dist" => {
                    cli.harbordesk_dist =
                        PathBuf::from(take_value(&args, &mut index, "--harbordesk-dist"))
                }
                value if value.starts_with("--harbordesk-dist=") => {
                    cli.harbordesk_dist =
                        PathBuf::from(value["--harbordesk-dist=".len()..].to_string())
                }
                "--public-origin" => {
                    cli.public_origin = take_value(&args, &mut index, "--public-origin")
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
    harbordesk_dist: PathBuf,
    public_origin: String,
}

#[derive(Debug, Deserialize)]
struct ManualAddRequest {
    name: String,
    room: Option<String>,
    ip: String,
    path: Option<String>,
    snapshot_url: Option<String>,
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
    #[serde(default)]
    selected_camera_device_id: Option<String>,
    #[serde(default)]
    capture_subdirectory: Option<String>,
    #[serde(default)]
    clip_length_seconds: Option<u32>,
    #[serde(default)]
    keyframe_count: Option<u32>,
    #[serde(default)]
    keyframe_interval_seconds: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BridgeConfigRequest {}

#[derive(Debug, Deserialize)]
struct NotificationTargetUpsertRequest {
    #[serde(default)]
    target_id: Option<String>,
    label: String,
    route_key: String,
    #[serde(default)]
    platform_hint: String,
    #[serde(default)]
    is_default: bool,
}

#[derive(Debug, Deserialize)]
struct NotificationTargetDefaultRequest {
    target_id: String,
}

#[derive(Debug, Serialize)]
struct NotificationTargetsResponse {
    targets: Vec<harborbeacon_local_agent::runtime::admin_console::NotificationTargetRecord>,
}

#[derive(Debug, Deserialize, Default)]
struct ApprovalDecisionRequest {
    #[serde(default)]
    approver_user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MembershipRoleUpdateRequest {
    role_kind: String,
}

#[derive(Debug, Deserialize)]
struct DefaultDeliverySurfaceUpdateRequest {
    surface: String,
}

#[derive(Debug, Serialize)]
struct ModelEndpointsResponse {
    endpoints: Vec<ModelEndpoint>,
}

#[derive(Debug, Serialize)]
struct ModelPoliciesResponse {
    route_policies: Vec<ModelRoutePolicy>,
}

#[derive(Debug, Deserialize)]
struct ModelPoliciesRequest {
    #[serde(default)]
    route_policies: Vec<ModelRoutePolicy>,
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
struct AdminStateResponse {
    #[serde(flatten)]
    state: StateResponse,
    account_management: AccountManagementSnapshot,
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
    #[serde(default)]
    proactive_delivery_surface: String,
    #[serde(default)]
    proactive_delivery_default: bool,
    #[serde(default)]
    binding_availability: String,
    #[serde(default)]
    binding_available: bool,
    #[serde(default)]
    binding_availability_note: String,
    #[serde(default)]
    recent_interactive_surface: Option<String>,
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
        harbordesk_dist: PathBuf,
        public_origin: String,
    ) -> Self {
        Self {
            admin_store,
            task_service,
            harbordesk_dist,
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

    fn authorize_workspace_camera_action(
        &self,
        hints: &AccessIdentityHints,
    ) -> Result<AccessPrincipal, String> {
        self.authorize_admin_action(hints, AccessAction::CameraOperate)
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

    fn handle_harbordesk(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }

        let dist_root = resolve_state_path(&self.harbordesk_dist);
        if !dist_root.exists() {
            return harbordesk_build_missing_response(&dist_root);
        }

        if let Some(asset_path) = resolve_harbordesk_asset_path(&dist_root, path) {
            if asset_path.is_file() {
                return static_file_response(&asset_path);
            }
        }

        if is_harbordesk_client_route(path) {
            let index_path = dist_root.join("index.html");
            if index_path.is_file() {
                return static_file_response(&index_path);
            }
            return harbordesk_build_missing_response(&dist_root);
        }

        error_json(StatusCode(404), "route not found")
    }

    fn handle(&self, mut request: Request) {
        let method = request.method().clone();
        let raw_url = request.url().to_string();
        let path = raw_url.split('?').next().unwrap_or("/").to_string();
        let remote_addr = request.remote_addr().copied();
        let headers = request.headers().to_vec();
        let identity_hints = request_identity_hints(&raw_url, &headers);

        if is_admin_surface_path(path.as_str()) || is_harbordesk_surface_path(path.as_str()) {
            if let Err(error) = ensure_local_admin_access(remote_addr, &headers) {
                let _ = request.respond(error_json(StatusCode(403), &error).boxed());
                return;
            }
        }

        let response = match method {
            Method::Get if path == "/healthz" => ok_json(&json!({"status":"ok"})).boxed(),
            Method::Get if path == "/api/state" => self.handle_state(&identity_hints).boxed(),
            Method::Get if path == "/api/account-management" => {
                self.handle_account_management(&identity_hints).boxed()
            }
            Method::Get if path == "/api/gateway/status" => {
                self.handle_gateway_status(&identity_hints).boxed()
            }
            Method::Get if path == "/api/models/endpoints" => {
                self.handle_model_endpoints(&identity_hints).boxed()
            }
            Method::Get if path == "/api/models/policies" => {
                self.handle_model_policies(&identity_hints).boxed()
            }
            Method::Get if path == "/admin/models" => {
                self.handle_models_page(&identity_hints).boxed()
            }
            Method::Get if path == "/api/access/members" => {
                self.handle_access_members(&identity_hints).boxed()
            }
            Method::Get if path == "/api/share-links" => {
                self.handle_share_links(&raw_url, &identity_hints).boxed()
            }
            Method::Get if path == "/api/tasks/approvals" => {
                self.handle_pending_approvals(&identity_hints).boxed()
            }
            Method::Get if path == "/api/admin/notification-targets" => {
                self.handle_notification_targets(&identity_hints).boxed()
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
            Method::Post if path == "/api/admin/notification-targets" => self
                .handle_upsert_notification_target(&mut request, &headers)
                .boxed(),
            Method::Post if path == "/api/admin/notification-targets/default" => self
                .handle_set_default_notification_target(&mut request, &identity_hints)
                .boxed(),
            Method::Post if path == "/api/models/endpoints" => self
                .handle_create_model_endpoint(&mut request, &identity_hints)
                .boxed(),
            Method::Post
                if path.starts_with("/api/models/endpoints/") && path.ends_with("/test") =>
            {
                self.handle_test_model_endpoint(path.as_str(), &identity_hints)
                    .boxed()
            }
            Method::Post if path == "/api/bridge/configure" => self
                .handle_configure_bridge(&mut request, &identity_hints)
                .boxed(),
            Method::Patch if path.starts_with("/api/models/endpoints/") => self
                .handle_patch_model_endpoint(path.as_str(), &mut request, &identity_hints)
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
                if path.starts_with("/api/access/members/")
                    && path.ends_with("/default-delivery-surface") =>
            {
                self.handle_update_member_default_delivery_surface(
                    path.as_str(),
                    &mut request,
                    &identity_hints,
                )
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
            Method::Put if path == "/api/models/policies" => self
                .handle_save_model_policies(&mut request, &identity_hints)
                .boxed(),
            Method::Delete if path.starts_with("/api/admin/notification-targets/") => self
                .handle_delete_notification_target(path.as_str(), &identity_hints)
                .boxed(),
            Method::Get if is_harbordesk_surface_path(path.as_str()) => self
                .handle_harbordesk(path.as_str(), &identity_hints)
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
        self.refresh_gateway_projection_best_effort();
        let live_bridge_provider = fetch_remote_gateway_status()
            .ok()
            .and_then(|payload| live_bridge_provider_from_setup_status(&payload));
        match self.admin_store.load_or_create_state() {
            Ok(state) => match self.current_state() {
                Ok(mut payload) => {
                    let mut account_management =
                        account_management_snapshot(&state, Some(&self.public_origin));
                    if let Some(provider) = live_bridge_provider.as_ref() {
                        apply_bridge_provider_projection_to_state(&mut payload, provider);
                        apply_bridge_provider_projection_to_gateway_summary(
                            &mut account_management.gateway,
                            provider,
                        );
                    }
                    ok_json(&AdminStateResponse {
                        state: redact_state_snapshot(payload),
                        account_management: redact_account_management_snapshot(account_management),
                    })
                }
                Err(error) => error_json(StatusCode(500), &error),
            },
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_account_management(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }

        self.refresh_gateway_projection_best_effort();
        let live_bridge_provider = fetch_remote_gateway_status()
            .ok()
            .and_then(|payload| live_bridge_provider_from_setup_status(&payload));
        match self.admin_store.load_or_create_state() {
            Ok(state) => {
                let mut snapshot = account_management_snapshot(&state, Some(&self.public_origin));
                if let Some(provider) = live_bridge_provider.as_ref() {
                    apply_bridge_provider_projection_to_gateway_summary(
                        &mut snapshot.gateway,
                        provider,
                    );
                }
                ok_json(&redact_account_management_snapshot(snapshot))
            }
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_gateway_status(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }

        self.refresh_gateway_projection_best_effort();
        match self.admin_store.load_or_create_state() {
            Ok(state) => {
                if let Ok(payload) = fetch_remote_gateway_status() {
                    ok_json(&payload)
                } else {
                    ok_json(&redact_gateway_status_summary(
                        account_management_snapshot(&state, Some(&self.public_origin)).gateway,
                    ))
                }
            }
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_model_endpoints(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.load_or_create_state() {
            Ok(state) => ok_json(&ModelEndpointsResponse {
                endpoints: redact_model_center_state(&state.models).endpoints,
            }),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_model_policies(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.load_or_create_state() {
            Ok(state) => ok_json(&ModelPoliciesResponse {
                route_policies: state.models.route_policies,
            }),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_models_page(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        let mut response =
            Response::from_string(render_models_admin_page()).with_status_code(StatusCode(200));
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

    fn handle_create_model_endpoint(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let endpoint: ModelEndpoint = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self.admin_store.save_model_endpoint(endpoint) {
            Ok(state) => ok_json(&redact_model_endpoint_response(&state.models.endpoints)),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_patch_model_endpoint(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let endpoint_id = match parse_model_endpoint_path(path) {
            Some(endpoint_id) => endpoint_id,
            None => return error_json(StatusCode(400), "invalid model endpoint path"),
        };
        let patch: Value = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self.admin_store.patch_model_endpoint(&endpoint_id, patch) {
            Ok(state) => ok_json(&redact_model_endpoint_response(&state.models.endpoints)),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_test_model_endpoint(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let endpoint_id = match parse_model_endpoint_test_path(path) {
            Some(endpoint_id) => endpoint_id,
            None => return error_json(StatusCode(400), "invalid model endpoint test path"),
        };
        let state = match self.admin_store.load_or_create_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let Some(endpoint) = state
            .models
            .endpoints
            .iter()
            .find(|endpoint| endpoint.model_endpoint_id == endpoint_id)
        else {
            return error_json(StatusCode(404), &format!("未找到模型端点 {endpoint_id}"));
        };
        let result: ModelEndpointTestResult = test_model_endpoint(endpoint);
        if let Err(error) = self.admin_store.record_model_endpoint_test_result(
            &endpoint_id,
            result.ok,
            &result.status,
            &result.summary,
            result.details.clone(),
        ) {
            return error_json(StatusCode(500), &error);
        }
        ok_json(&result)
    }

    fn handle_save_model_policies(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let payload: ModelPoliciesRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self
            .admin_store
            .save_model_route_policies(payload.route_policies)
        {
            Ok(state) => ok_json(&ModelPoliciesResponse {
                route_policies: state.models.route_policies,
            }),
            Err(error) => error_json(StatusCode(422), &error),
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

    fn handle_update_member_default_delivery_surface(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }

        let user_id = match parse_member_default_delivery_surface_update_path(path) {
            Some(user_id) => user_id,
            None => {
                return error_json(
                    StatusCode(400),
                    "invalid member default delivery surface path",
                )
            }
        };
        let body: DefaultDeliverySurfaceUpdateRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        let surface = match normalize_delivery_surface(&body.surface) {
            Some(surface) => surface,
            None => return error_json(StatusCode(400), "surface 只能是 feishu 或 weixin"),
        };

        match self
            .admin_store
            .set_member_default_delivery_surface(&user_id, &surface)
            .map(|state| build_access_member_summaries(&state))
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_notification_targets(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.load_or_create_state() {
            Ok(state) => ok_json(&NotificationTargetsResponse {
                targets: state.notification_targets,
            }),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_upsert_notification_target(
        &self,
        request: &mut Request,
        headers: &[Header],
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = authorize_gateway_service_request(headers) {
            return error_json(StatusCode(401), &error);
        }

        let body: NotificationTargetUpsertRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self
            .admin_store
            .upsert_notification_target(
                body.target_id.as_deref(),
                &body.label,
                &body.route_key,
                &body.platform_hint,
                body.is_default,
            )
            .map(|state| NotificationTargetsResponse {
                targets: state.notification_targets,
            })
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_set_default_notification_target(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let body: NotificationTargetDefaultRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self
            .admin_store
            .set_default_notification_target(&body.target_id)
            .map(|state| NotificationTargetsResponse {
                targets: state.notification_targets,
            })
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_delete_notification_target(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let target_id = match parse_notification_target_delete_path(path) {
            Some(target_id) => target_id,
            None => return error_json(StatusCode(400), "invalid notification target path"),
        };
        match self.admin_store.delete_notification_target(&target_id) {
            Ok(_) => no_content(),
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
        deprecated_im_binding_response_json(&self.current_gateway_manage_url())
    }

    fn handle_static_binding_qr_svg(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        deprecated_im_binding_response_json(&self.current_gateway_manage_url())
    }

    fn handle_mobile_setup_page(
        &self,
        _url: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        deprecated_im_binding_response_html(&self.current_gateway_manage_url())
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
        deprecated_im_binding_response_json(&self.current_gateway_manage_url())
    }

    fn handle_demo_bind(&self, hints: &AccessIdentityHints) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        deprecated_im_binding_response_json(&self.current_gateway_manage_url())
    }

    fn handle_test_bind(
        &self,
        _request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        deprecated_im_binding_response_json(&self.current_gateway_manage_url())
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
        let principal = match self.authorize_workspace_camera_action(hints) {
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
        let principal = match self.authorize_workspace_camera_action(hints) {
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
            selected_camera_device_id: body.selected_camera_device_id,
            capture_subdirectory: body
                .capture_subdirectory
                .unwrap_or_else(default_capture_subdirectory),
            clip_length_seconds: body
                .clip_length_seconds
                .unwrap_or_else(default_clip_length_seconds),
            keyframe_count: body.keyframe_count.unwrap_or_else(default_keyframe_count),
            keyframe_interval_seconds: body
                .keyframe_interval_seconds
                .unwrap_or_else(default_keyframe_interval_seconds),
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

    fn current_gateway_manage_url(&self) -> String {
        self.admin_store
            .load_or_create_state()
            .map(|state| {
                harborbeacon_local_agent::runtime::admin_console::gateway_manage_url(
                    &state.bridge_provider.gateway_base_url,
                )
            })
            .unwrap_or_default()
    }

    fn refresh_gateway_projection_best_effort(&self) {
        if self
            .hub()
            .refresh_bridge_provider_status(Some(&self.public_origin))
            .is_ok()
        {
            return;
        }
        if let Ok(payload) = fetch_remote_gateway_status() {
            if let Some(provider) = live_bridge_provider_from_setup_status(&payload) {
                let _ = self.admin_store.save_bridge_provider_status(provider);
            }
        }
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
        let ManualAddRequest {
            name,
            room,
            ip,
            path,
            snapshot_url,
            username,
            password,
            port,
        } = request;
        let path_candidates = path
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

        if principal_skips_manual_camera_connect_approval(principal) {
            return self.hub().manual_add(
                CameraConnectRequest {
                    name,
                    room,
                    ip,
                    path_candidates,
                    username,
                    password,
                    port,
                    snapshot_url,
                    discovery_source: "admin_console_manual_add".to_string(),
                    vendor: None,
                    model: None,
                },
                Some(&self.public_origin),
            );
        }

        let response = self
            .task_service
            .handle_task(self.build_camera_task_request(
                principal,
                "connect",
                "手动接入摄像头",
                json!({
                    "name": name,
                    "room": room,
                    "ip": ip,
                    "path_candidates": path_candidates,
                    "snapshot_url": snapshot_url,
                    "username": username,
                    "password": password,
                    "port": port,
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

fn principal_skips_manual_camera_connect_approval(principal: &AccessPrincipal) -> bool {
    matches!(principal.role_kind, RoleKind::Owner | RoleKind::Admin)
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
    let api = AdminApi::new(
        admin_store,
        task_service,
        cli.harbordesk_dist,
        cli.public_origin,
    );

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
    preferred.to_path_buf()
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
        harboros_user_id: header_value(headers, "X-HarborOS-User")
            .or_else(|| header_value(headers, "X-Harbor-OS-User"))
            .or_else(|| parse_query_param(url, "harboros_user"))
            .and_then(percent_decode_optional_query_value)
            .or_else(|| std::env::var("HARBOR_HARBOROS_USER").ok()),
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

fn parse_member_default_delivery_surface_update_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/access/members/")?;
    let user_id = trimmed.strip_suffix("/default-delivery-surface")?.trim();
    if user_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(user_id).ok()
    }
}

fn parse_notification_target_delete_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/admin/notification-targets/")?.trim();
    if trimmed.is_empty() {
        None
    } else {
        percent_decode_path_segment(trimmed).ok()
    }
}

fn parse_model_endpoint_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/models/endpoints/")?;
    if trimmed.trim().is_empty() || trimmed.ends_with("/test") {
        return None;
    }
    percent_decode_path_segment(trimmed.trim()).ok()
}

fn parse_model_endpoint_test_path(path: &str) -> Option<String> {
    let endpoint_id = path
        .strip_prefix("/api/models/endpoints/")?
        .strip_suffix("/test")?
        .trim();
    if endpoint_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(endpoint_id).ok()
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
                proactive_delivery_surface: user
                    .and_then(user_default_delivery_surface)
                    .unwrap_or_else(|| "feishu".to_string()),
                proactive_delivery_default: true,
                binding_availability: if identity_binding.is_some() {
                    "available".to_string()
                } else {
                    "blocked".to_string()
                },
                binding_available: identity_binding.is_some(),
                binding_availability_note: if identity_binding.is_some() {
                    "HarborGate identity binding is available for member-default proactive delivery."
                        .to_string()
                } else {
                    "HarborGate identity binding is missing; proactive delivery will remain queued until a binding exists."
                        .to_string()
                },
                recent_interactive_surface: user.and_then(user_recent_interactive_surface),
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

fn render_models_admin_page() -> String {
    r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>HarborBeacon 模型中心</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #f4efe7;
      --card: rgba(255,255,255,0.9);
      --line: #d9c6ae;
      --text: #1e1b18;
      --muted: #6b5a49;
      --accent: #1f7a6f;
      --danger: #b94739;
    }
    * { box-sizing: border-box; }
    body { margin: 0; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif; background: var(--bg); color: var(--text); }
    .wrap { max-width: 980px; margin: 0 auto; padding: 24px 18px 48px; }
    .grid { display: grid; gap: 18px; grid-template-columns: 1.3fr 1fr; }
    .card { background: var(--card); border-radius: 22px; padding: 18px; box-shadow: 0 18px 48px rgba(51,36,18,0.12); }
    h1, h2 { margin: 0 0 10px; }
    .meta { color: var(--muted); font-size: 14px; line-height: 1.6; margin-bottom: 14px; }
    table { width: 100%; border-collapse: collapse; font-size: 14px; }
    th, td { text-align: left; padding: 10px 8px; border-bottom: 1px solid #eadcca; vertical-align: top; }
    th { color: var(--muted); font-weight: 600; }
    .chip { display: inline-block; padding: 4px 9px; border-radius: 999px; background: #f6f2ec; border: 1px solid #eadcca; margin-right: 6px; margin-bottom: 6px; font-size: 12px; }
    label { display: block; margin: 12px 0 6px; font-weight: 600; }
    input, select, textarea { width: 100%; padding: 11px 12px; border-radius: 12px; border: 1px solid var(--line); background: white; font: inherit; }
    textarea { min-height: 96px; resize: vertical; }
    .row { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
    button { border: 0; border-radius: 999px; padding: 11px 16px; background: var(--accent); color: white; font-weight: 700; font-size: 14px; cursor: pointer; }
    button.secondary { background: #f6f2ec; color: var(--text); border: 1px solid var(--line); }
    .actions { display: flex; gap: 10px; flex-wrap: wrap; margin-top: 14px; }
    pre { background: #181512; color: #f5efe7; border-radius: 16px; padding: 14px; overflow: auto; font-size: 12px; }
    .ok { color: var(--accent); }
    .err { color: var(--danger); }
    @media (max-width: 860px) {
      .grid { grid-template-columns: 1fr; }
      .row { grid-template-columns: 1fr; }
    }
  </style>
</head>
<body>
  <div class="wrap">
    <div class="card" style="margin-bottom:18px;">
      <h1>模型中心</h1>
      <div class="meta">HarborBeacon 负责 OCR / embedder / LLM / VLM 的路由与红acted 管理。前端只看到脱敏状态，secret 只保存在后端状态文件里。</div>
      <div class="actions">
        <button id="refresh-btn" class="secondary">刷新状态</button>
      </div>
    </div>
    <div class="grid">
      <div class="card">
        <h2>端点列表</h2>
        <div class="meta">默认会显示本地 OCR / OpenAI-compatible 槽位。`api_key_configured=true` 表示后端已持有 secret，但不会回显明文。</div>
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>种类</th>
              <th>路由</th>
              <th>状态</th>
              <th>配置</th>
            </tr>
          </thead>
          <tbody id="endpoint-body">
            <tr><td colspan="5">加载中...</td></tr>
          </tbody>
        </table>
        <div class="actions" id="endpoint-actions"></div>
      </div>
      <div class="card">
        <h2>新增 / 更新端点</h2>
        <div class="row">
          <div>
            <label for="endpoint-id">Endpoint ID</label>
            <input id="endpoint-id" placeholder="ocr-local-tesseract" />
          </div>
          <div>
            <label for="model-kind">Model Kind</label>
            <select id="model-kind">
              <option value="ocr">ocr</option>
              <option value="embedder">embedder</option>
              <option value="llm">llm</option>
              <option value="vlm">vlm</option>
            </select>
          </div>
        </div>
        <div class="row">
          <div>
            <label for="endpoint-kind">Endpoint Kind</label>
            <select id="endpoint-kind">
              <option value="local">local</option>
              <option value="sidecar">sidecar</option>
              <option value="cloud">cloud</option>
            </select>
          </div>
          <div>
            <label for="provider-key">Provider</label>
            <input id="provider-key" placeholder="tesseract / ollama / custom" />
          </div>
        </div>
        <div class="row">
          <div>
            <label for="model-name">Model Name</label>
            <input id="model-name" placeholder="qwen2.5:7b / tesseract-cli" />
          </div>
          <div>
            <label for="status">Status</label>
            <select id="status">
              <option value="active">active</option>
              <option value="degraded">degraded</option>
              <option value="disabled">disabled</option>
            </select>
          </div>
        </div>
        <label for="capability-tags">Capability Tags（逗号分隔）</label>
        <input id="capability-tags" placeholder="ocr,image,local_first" />
        <label for="base-url">Base URL</label>
        <input id="base-url" placeholder="http://127.0.0.1:11434/v1" />
        <label for="api-key">API Key</label>
        <input id="api-key" placeholder="可留空；只会写入后端，不会回显" />
        <label for="binary-path">Tesseract Binary</label>
        <input id="binary-path" placeholder="留空则自动查找 PATH" />
        <label for="languages">OCR Languages</label>
        <input id="languages" value="chi_sim+eng" />
        <label for="metadata-json">Metadata JSON（可选）</label>
        <textarea id="metadata-json" placeholder='{"mock_text":"front gate"}'></textarea>
        <div class="actions">
          <button id="save-endpoint-btn">保存端点</button>
        </div>
      </div>
    </div>
    <div class="grid" style="margin-top:18px;">
      <div class="card">
        <h2>路由策略</h2>
        <div class="meta">这里直接编辑 `retrieval.ocr / retrieval.embed / retrieval.answer / retrieval.vision_summary` 的 JSON 数组。</div>
        <textarea id="policies-json" style="min-height:260px;"></textarea>
        <div class="actions">
          <button id="save-policies-btn">保存策略</button>
        </div>
      </div>
      <div class="card">
        <h2>连通性测试</h2>
        <div class="meta">点击端点表中的测试按钮后，这里会显示结果。</div>
        <pre id="test-result">等待测试</pre>
      </div>
    </div>
  </div>
  <script>
    const endpointBody = document.getElementById("endpoint-body");
    const endpointActions = document.getElementById("endpoint-actions");
    const policiesJson = document.getElementById("policies-json");
    const testResult = document.getElementById("test-result");

    async function fetchJson(path, options = {}) {
      const response = await fetch(path, {
        headers: { "Content-Type": "application/json", ...(options.headers || {}) },
        ...options,
      });
      const payload = await response.json().catch(() => ({}));
      if (!response.ok) {
        throw new Error(payload.error || payload.message || `Request failed: ${response.status}`);
      }
      return payload;
    }

    function endpointConfigSummary(endpoint) {
      const metadata = endpoint.metadata || {};
      const summary = [];
      if (metadata.base_url) summary.push(`base_url=${metadata.base_url}`);
      if (metadata.binary_path) summary.push(`binary=${metadata.binary_path}`);
      if (metadata.languages) summary.push(`langs=${metadata.languages}`);
      if (metadata.api_key_configured) summary.push("api_key=configured");
      return summary.join(" | ") || "未配置";
    }

    function renderEndpoints(endpoints) {
      endpointBody.innerHTML = "";
      endpointActions.innerHTML = "";
      if (!endpoints.length) {
        endpointBody.innerHTML = '<tr><td colspan="5">还没有模型端点。</td></tr>';
        return;
      }
      for (const endpoint of endpoints) {
        const row = document.createElement("tr");
        row.innerHTML = `
          <td><strong>${endpoint.model_endpoint_id}</strong></td>
          <td>${endpoint.model_kind}</td>
          <td>${endpoint.endpoint_kind}<br /><span class="chip">${endpoint.provider_key}</span></td>
          <td>${endpoint.status}</td>
          <td>${endpointConfigSummary(endpoint)}</td>
        `;
        endpointBody.appendChild(row);

        const button = document.createElement("button");
        button.className = "secondary";
        button.textContent = `测试 ${endpoint.model_endpoint_id}`;
        button.addEventListener("click", async () => {
          testResult.textContent = "测试中...";
          try {
            const payload = await fetchJson(`/api/models/endpoints/${encodeURIComponent(endpoint.model_endpoint_id)}/test`, {
              method: "POST",
              body: JSON.stringify({}),
            });
            testResult.textContent = JSON.stringify(payload, null, 2);
          } catch (error) {
            testResult.textContent = error.message;
          }
        });
        endpointActions.appendChild(button);
      }
    }

    async function loadState() {
      const [endpointPayload, policyPayload] = await Promise.all([
        fetchJson("/api/models/endpoints"),
        fetchJson("/api/models/policies"),
      ]);
      renderEndpoints(endpointPayload.endpoints || []);
      policiesJson.value = JSON.stringify(policyPayload.route_policies || [], null, 2);
    }

    function collectEndpointPayload() {
      let metadata = {};
      const rawMetadata = document.getElementById("metadata-json").value.trim();
      if (rawMetadata) {
        metadata = JSON.parse(rawMetadata);
      }
      const baseUrl = document.getElementById("base-url").value.trim();
      const apiKey = document.getElementById("api-key").value.trim();
      const binaryPath = document.getElementById("binary-path").value.trim();
      const languages = document.getElementById("languages").value.trim();
      if (baseUrl) metadata.base_url = baseUrl;
      if (apiKey) metadata.api_key = apiKey;
      if (binaryPath) metadata.binary_path = binaryPath;
      if (languages) metadata.languages = languages;
      return {
        model_endpoint_id: document.getElementById("endpoint-id").value.trim(),
        workspace_id: "home-1",
        provider_account_id: null,
        model_kind: document.getElementById("model-kind").value,
        endpoint_kind: document.getElementById("endpoint-kind").value,
        provider_key: document.getElementById("provider-key").value.trim() || "custom",
        model_name: document.getElementById("model-name").value.trim() || "custom",
        capability_tags: document.getElementById("capability-tags").value.split(",").map((item) => item.trim()).filter(Boolean),
        cost_policy: {},
        status: document.getElementById("status").value,
        metadata,
      };
    }

    document.getElementById("refresh-btn").addEventListener("click", loadState);
    document.getElementById("save-endpoint-btn").addEventListener("click", async () => {
      try {
        const payload = collectEndpointPayload();
        await fetchJson("/api/models/endpoints", {
          method: "POST",
          body: JSON.stringify(payload),
        });
        await loadState();
      } catch (error) {
        testResult.textContent = error.message;
      }
    });
    document.getElementById("save-policies-btn").addEventListener("click", async () => {
      try {
        const route_policies = JSON.parse(policiesJson.value || "[]");
        await fetchJson("/api/models/policies", {
          method: "PUT",
          body: JSON.stringify({ route_policies }),
        });
        await loadState();
      } catch (error) {
        testResult.textContent = error.message;
      }
    });
    loadState().catch((error) => {
      endpointBody.innerHTML = `<tr><td colspan="5" class="err">${error.message}</td></tr>`;
      testResult.textContent = error.message;
    });
  </script>
</body>
</html>"#
        .to_string()
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

fn deprecated_im_binding_message() -> &'static str {
    "IM configuration has moved to HarborGate. HarborBeacon no longer serves IM setup, binding, or QR flows."
}

fn deprecated_im_binding_response_json(
    manage_url: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(
        StatusCode(410),
        &json!({
            "error": deprecated_im_binding_message(),
            "manage_url": manage_url,
        }),
    )
}

fn deprecated_im_binding_response_html(
    manage_url: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let manage_url = html_escape(manage_url);
    let body = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>IM 配置已迁移</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, sans-serif; background: #f4efe7; color: #1e1b18; margin: 0; }}
    .wrap {{ max-width: 560px; margin: 0 auto; padding: 36px 18px 48px; }}
    .card {{ background: rgba(255,255,255,0.92); border-radius: 20px; padding: 24px; box-shadow: 0 18px 48px rgba(51,36,18,0.12); }}
    h1 {{ margin-top: 0; }}
    code, a {{ word-break: break-all; }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="card">
      <h1>IM 配置已迁移到 HarborGate</h1>
      <p>{}</p>
      <p>HarborBeacon 现在只保留业务后台与 HarborGate 状态读取，不再提供任何 IM 扫码、绑定或登录入口。</p>
      <p>HarborGate 管理页：<a href="{manage_url}">{manage_url}</a></p>
    </div>
  </div>
</body>
</html>"#,
        html_escape(deprecated_im_binding_message())
    );
    let mut response = Response::from_string(body).with_status_code(StatusCode(410));
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

fn authorize_gateway_service_request(headers: &[Header]) -> Result<(), String> {
    let expected = env_var_with_legacy_alias("HARBORGATE_BEARER_TOKEN", "HARBOR_IM_GATEWAY_BEARER_TOKEN")
        .or_else(|| env::var("IM_AGENT_SERVICE_TOKEN").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "gateway service token is not configured".to_string())?;
    let actual = header_value(headers, "Authorization")
        .and_then(|value| parse_bearer_token(&value))
        .ok_or_else(|| "missing or invalid bearer token".to_string())?;
    if actual != expected {
        return Err("missing or invalid bearer token".to_string());
    }
    Ok(())
}

fn parse_bearer_token(value: &str) -> Option<String> {
    let prefix = "bearer ";
    value
        .trim()
        .to_ascii_lowercase()
        .strip_prefix(prefix)
        .map(|_| value.trim()[prefix.len()..].trim().to_string())
        .filter(|token| !token.is_empty())
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
        (
            "Access-Control-Allow-Methods",
            "GET, POST, PATCH, PUT, OPTIONS",
        ),
        ("Cache-Control", "no-store"),
    ] {
        response.add_header(
            Header::from_bytes(header.0.as_bytes(), header.1.as_bytes()).expect("header"),
        );
    }
}

fn is_admin_surface_path(path: &str) -> bool {
    path == "/api/state"
        || path == "/api/account-management"
        || path == "/api/gateway/status"
        || path == "/api/models/endpoints"
        || path == "/api/models/policies"
        || path == "/admin/models"
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
        || (path.starts_with("/api/access/members/") && path.ends_with("/default-delivery-surface"))
        || path == "/api/tasks/approvals"
        || path.starts_with("/api/tasks/approvals/")
        || path == "/api/discovery/scan"
        || path == "/api/devices/manual"
        || (path.starts_with("/api/cameras/") && path.ends_with("/share-link"))
        || (path.starts_with("/api/share-links/") && path.ends_with("/revoke"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/snapshot"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/analyze"))
        || path == "/api/defaults"
        || path.starts_with("/api/models/endpoints/")
}

fn is_harbordesk_client_route(path: &str) -> bool {
    matches!(
        path,
        "/" | "/overview"
            | "/im-gateway"
            | "/account-management"
            | "/tasks-approvals"
            | "/devices-aiot"
            | "/harboros"
            | "/models-policies"
            | "/system-settings"
    )
}

fn looks_like_harbordesk_asset_path(path: &str) -> bool {
    path.starts_with("/assets/")
        || [
            ".js",
            ".css",
            ".map",
            ".json",
            ".png",
            ".svg",
            ".ico",
            ".txt",
            ".webmanifest",
            ".woff",
            ".woff2",
        ]
        .iter()
        .any(|extension| path.ends_with(extension))
}

fn is_harbordesk_surface_path(path: &str) -> bool {
    is_harbordesk_client_route(path) || looks_like_harbordesk_asset_path(path)
}

fn resolve_harbordesk_asset_path(dist_root: &Path, request_path: &str) -> Option<PathBuf> {
    if !looks_like_harbordesk_asset_path(request_path) {
        return None;
    }

    let relative = request_path.trim_start_matches('/');
    if relative.is_empty() {
        return None;
    }

    let mut resolved = dist_root.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            std::path::Component::Normal(segment) => resolved.push(segment),
            _ => return None,
        }
    }
    Some(resolved)
}

fn mime_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        Some("map") => "application/json; charset=utf-8",
        Some("webmanifest") => "application/manifest+json; charset=utf-8",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn static_file_response(path: &Path) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = match fs::read(path) {
        Ok(payload) => payload,
        Err(error) => {
            return error_json(
                StatusCode(500),
                &format!("failed to read static file {}: {error}", path.display()),
            )
        }
    };
    let mut response = Response::from_data(body).with_status_code(StatusCode(200));
    add_common_headers(&mut response);
    response.add_header(
        Header::from_bytes(
            b"Content-Type".as_slice(),
            mime_type_for_path(path).as_bytes(),
        )
        .expect("header"),
    );
    response
}

fn harbordesk_build_missing_response(dist_root: &Path) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>HarborDesk build missing</title></head><body><h1>HarborDesk build missing</h1><p>Angular build output was not found at <code>{}</code>.</p><p>Run <code>npm install</code> and <code>npm run build</code> under <code>frontend/harbordesk</code>, or pass <code>--harbordesk-dist</code>.</p></body></html>",
        dist_root.display()
    );
    let mut response = Response::from_string(body).with_status_code(StatusCode(503));
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
    state.binding.session_code.clear();
    state.binding.qr_token.clear();
    state.binding.setup_url.clear();
    state.binding.static_setup_url.clear();
    state.bridge_provider = redact_bridge_provider_config(state.bridge_provider);
    state
}

fn redact_account_management_snapshot(
    mut snapshot: AccountManagementSnapshot,
) -> AccountManagementSnapshot {
    snapshot.gateway = redact_gateway_status_summary(snapshot.gateway);
    snapshot
}

fn redact_gateway_status_summary(mut gateway: GatewayStatusSummary) -> GatewayStatusSummary {
    gateway.setup_url.clear();
    gateway.static_setup_url.clear();
    gateway.bridge_provider = redact_bridge_provider_config(gateway.bridge_provider);
    gateway
}

fn apply_bridge_provider_binding_projection(
    status: &mut String,
    metric: &mut String,
    bound_user: &mut Option<String>,
    provider: &BridgeProviderConfig,
) {
    if provider.connected {
        *status = "Gateway 已连接".to_string();
        *metric = "Gateway 在线".to_string();
    } else if provider.configured {
        *status = "Gateway 已启用".to_string();
        *metric = "Gateway 未连通".to_string();
    } else {
        *status = "等待 Gateway".to_string();
        *metric = "Gateway 未配置".to_string();
    }

    *bound_user = if !provider.app_name.trim().is_empty() {
        Some(provider.app_name.clone())
    } else if !provider.platform.trim().is_empty() {
        Some(format!("{} gateway", provider.platform))
    } else {
        None
    };
}

fn apply_bridge_provider_projection_to_state(
    state: &mut StateResponse,
    provider: &BridgeProviderConfig,
) {
    state.bridge_provider = provider.clone();
    apply_bridge_provider_binding_projection(
        &mut state.binding.status,
        &mut state.binding.metric,
        &mut state.binding.bound_user,
        provider,
    );
}

fn apply_bridge_provider_projection_to_gateway_summary(
    gateway: &mut GatewayStatusSummary,
    provider: &BridgeProviderConfig,
) {
    gateway.bridge_provider = provider.clone();
    apply_bridge_provider_binding_projection(
        &mut gateway.binding_status,
        &mut gateway.binding_metric,
        &mut gateway.binding_bound_user,
        provider,
    );
}

fn bridge_provider_config_from_platforms(
    gateway_base_url: &str,
    platforms: &[GatewayPlatformStatus],
) -> BridgeProviderConfig {
    let selected = platforms
        .iter()
        .find(|platform| platform.connected)
        .or_else(|| platforms.iter().find(|platform| platform.enabled))
        .or_else(|| platforms.first());
    let mut provider = BridgeProviderConfig {
        gateway_base_url: gateway_base_url.trim().to_string(),
        ..Default::default()
    };
    let Some(selected) = selected else {
        provider.status = "HarborGate 未配置平台".to_string();
        return provider;
    };

    provider.configured = selected.enabled;
    provider.connected = selected.connected;
    provider.platform = selected.platform.trim().to_string();
    provider.app_name = selected.display_name.trim().to_string();
    provider.status = if selected.connected {
        "已连接".to_string()
    } else if selected.enabled {
        "已启用，待连接".to_string()
    } else {
        "未启用".to_string()
    };
    provider.capabilities.reply = selected.capabilities.reply;
    provider.capabilities.update = selected.capabilities.update;
    provider.capabilities.attachments = selected.capabilities.attachments;
    provider
}

fn env_var_with_legacy_alias(primary: &str, legacy: &str) -> Option<String> {
    if let Ok(value) = env::var(primary) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Ok(value) = env::var(legacy) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    None
}

fn gateway_status_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/api/gateway/status") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/api/gateway/status")
    }
}

fn live_bridge_provider_from_setup_status(payload: &Value) -> Option<BridgeProviderConfig> {
    const PRIMARY_BASE_URL: &str = "HARBORGATE_BASE_URL";
    const LEGACY_BASE_URL: &str = "HARBOR_IM_GATEWAY_BASE_URL";

    let channels = payload
        .get("channels")
        .cloned()
        .or_else(|| payload.pointer("/gateway_status/channels").cloned())?;
    let platforms: Vec<GatewayPlatformStatus> = serde_json::from_value(channels).ok()?;
    if platforms.is_empty() {
        return None;
    }

    let gateway_base_url = env_var_with_legacy_alias(PRIMARY_BASE_URL, LEGACY_BASE_URL)
        .or_else(|| {
            payload
                .get("public_origin")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_default();
    Some(bridge_provider_config_from_platforms(
        &gateway_base_url,
        &platforms,
    ))
}

fn fetch_remote_gateway_status() -> Result<Value, String> {
    const PRIMARY_BASE_URL: &str = "HARBORGATE_BASE_URL";
    const LEGACY_BASE_URL: &str = "HARBOR_IM_GATEWAY_BASE_URL";
    const PRIMARY_TOKEN: &str = "HARBORGATE_BEARER_TOKEN";
    const LEGACY_TOKEN: &str = "HARBOR_IM_GATEWAY_BEARER_TOKEN";

    let base_url = env_var_with_legacy_alias(PRIMARY_BASE_URL, LEGACY_BASE_URL)
        .ok_or_else(|| format!("missing required env var {PRIMARY_BASE_URL}"))?;
    let endpoint = gateway_status_endpoint(&base_url);
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| format!("failed to build HarborGate status client: {error}"))?;

    let mut request = client.get(endpoint).header("X-Contract-Version", "1.5");
    if let Some(token) = env_var_with_legacy_alias(PRIMARY_TOKEN, LEGACY_TOKEN) {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let response = request
        .send()
        .map_err(|error| format!("HarborGate status request failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("failed to read HarborGate status response: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "HarborGate status request failed with HTTP {}: {}",
            status.as_u16(),
            body
        ));
    }

    serde_json::from_str(&body)
        .map_err(|error| format!("failed to parse HarborGate status response: {error}"))
}

fn redact_model_endpoint_response(endpoints: &[ModelEndpoint]) -> ModelEndpointsResponse {
    ModelEndpointsResponse {
        endpoints: endpoints.iter().map(redact_model_endpoint).collect(),
    }
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
        let ffmpeg_bin = resolve_ffmpeg_bin()
            .ok_or_else(|| format!("当前机器缺少 ffmpeg，{}", ffmpeg_resolution_hint()))?;

        let mut child = Command::new(&ffmpeg_bin)
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
        apply_bridge_provider_binding_projection, authorize_gateway_service_request,
        ensure_local_admin_access,
        ensure_local_camera_access, harbordesk_build_missing_response, has_forwarding_headers,
        identity_query_suffix, is_admin_surface_path, is_harbordesk_client_route,
        is_harbordesk_surface_path, live_bridge_provider_from_setup_status, mime_type_for_path,
        ManualAddRequest,
        parse_approval_decision_path, parse_camera_analyze_path, parse_camera_live_page_path,
        parse_camera_live_stream_path, parse_camera_share_link_path, parse_camera_snapshot_path,
        parse_camera_task_snapshot_path, parse_member_default_delivery_surface_update_path,
        parse_member_role_update_path, parse_model_endpoint_path, parse_model_endpoint_test_path,
        parse_notification_target_delete_path, parse_share_link_revoke_path,
        parse_shared_camera_live_page_path,
        parse_shared_camera_live_stream_path, percent_decode_path_segment,
        redact_account_management_snapshot, redact_bridge_provider_config,
        redact_model_endpoint_response, request_identity_hints, resolve_harbordesk_asset_path,
        url_encode_path_segment, AdminApi,
    };
    use harborbeacon_local_agent::control_plane::media::{
        MediaDeliveryMode, MediaSession, MediaSessionKind, MediaSessionStatus, ShareAccessScope,
        ShareLink,
    };
    use harborbeacon_local_agent::control_plane::models::{
        ModelEndpoint, ModelEndpointKind, ModelEndpointStatus, ModelKind,
    };
    use harborbeacon_local_agent::control_plane::users::{MembershipStatus, RoleKind};
    use harborbeacon_local_agent::runtime::access_control::{
        AccessAction, AccessIdentityHints, AccessPrincipal,
    };
    use harborbeacon_local_agent::runtime::admin_console::{
        AdminConsoleStore, BridgeProviderConfig, RemoteViewConfig,
    };
    use harborbeacon_local_agent::runtime::registry::DeviceRegistryStore;
    use harborbeacon_local_agent::runtime::remote_view;
    use harborbeacon_local_agent::runtime::task_api::TaskApiService;
    use harborbeacon_local_agent::runtime::task_session::TaskConversationStore;
    use serde_json::json;
    use std::fs;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tiny_http::{Header, StatusCode};

    fn unique_store_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}.json"))
    }

    struct EnvGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => unsafe {
                    std::env::set_var(&self.key, value);
                },
                None => unsafe {
                    std::env::remove_var(&self.key);
                },
            }
        }
    }

    fn build_manual_add_request(ip: &str) -> ManualAddRequest {
        ManualAddRequest {
            name: "Test Camera".to_string(),
            room: None,
            ip: ip.to_string(),
            path: None,
            snapshot_url: None,
            username: None,
            password: None,
            port: None,
        }
    }

    #[test]
    fn owner_and_admin_manual_add_skip_camera_connect_approval_queue() {
        for (role_kind, user_id) in [
            (RoleKind::Owner, "local-owner"),
            (RoleKind::Admin, "admin-1"),
        ] {
            let admin_path = unique_store_path("harborbeacon-manual-add-state");
            let registry_path = unique_store_path("harborbeacon-manual-add-registry");
            let conversation_path = unique_store_path("harborbeacon-manual-add-runtime");
            let registry_store = DeviceRegistryStore::new(registry_path.clone());
            let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
            let conversation_store = TaskConversationStore::new(conversation_path.clone());
            let task_service = TaskApiService::new(admin_store.clone(), conversation_store.clone());
            let api = AdminApi::new(
                admin_store,
                task_service,
                PathBuf::from("frontend/harbordesk/dist/harbordesk"),
                "http://harborbeacon.local:4174".to_string(),
            );

            let error = api
                .manual_add(
                    &AccessPrincipal {
                        workspace_id: "home-1".to_string(),
                        user_id: user_id.to_string(),
                        display_name: user_id.to_string(),
                        role_kind,
                    },
                    build_manual_add_request(""),
                )
                .expect_err("owner/admin manual add should fail validation before approval");

            assert_eq!(error, "IP 地址不能为空");
            assert!(
                conversation_store
                    .pending_approvals()
                    .expect("load pending approvals")
                    .is_empty()
            );

            let _ = fs::remove_file(admin_path);
            let _ = fs::remove_file(registry_path);
            let _ = fs::remove_file(conversation_path);
        }
    }

    #[test]
    fn operator_manual_add_still_routes_into_camera_connect_approval_queue() {
        let admin_path = unique_store_path("harborbeacon-operator-add-state");
        let registry_path = unique_store_path("harborbeacon-operator-add-registry");
        let conversation_path = unique_store_path("harborbeacon-operator-add-runtime");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let task_service = TaskApiService::new(admin_store.clone(), conversation_store.clone());
        let api = AdminApi::new(
            admin_store,
            task_service,
            PathBuf::from("frontend/harbordesk/dist/harbordesk"),
            "http://harborbeacon.local:4174".to_string(),
        );

        let error = api
            .manual_add(
                &AccessPrincipal {
                    workspace_id: "home-1".to_string(),
                    user_id: "operator-1".to_string(),
                    display_name: "operator-1".to_string(),
                    role_kind: RoleKind::Operator,
                },
                build_manual_add_request(""),
            )
            .expect_err("operator manual add should still require approval");

        assert!(error.contains("approval_token"));
        let approvals = conversation_store
            .pending_approvals()
            .expect("load pending approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].policy_ref, "camera.connect");
        assert_eq!(approvals[0].requester_user_id, "operator-1");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn operator_camera_workspace_authorization_reaches_manual_add_approval_path() {
        let admin_path = unique_store_path("harborbeacon-operator-http-state");
        let registry_path = unique_store_path("harborbeacon-operator-http-registry");
        let conversation_path = unique_store_path("harborbeacon-operator-http-runtime");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let conversation_store = TaskConversationStore::new(conversation_path.clone());
        let task_service = TaskApiService::new(admin_store.clone(), conversation_store.clone());

        let mut state = admin_store.load_or_create_state().expect("state");
        state
            .platform
            .users
            .push(harborbeacon_local_agent::control_plane::users::UserAccount {
                user_id: "operator-1".to_string(),
                display_name: "operator-1".to_string(),
                email: None,
                phone: None,
                status: harborbeacon_local_agent::control_plane::users::UserStatus::Active,
                default_workspace_id: Some("home-1".to_string()),
                preferences: json!({
                    "auth_source": "harbor_os",
                    "channel": "harbor_os",
                }),
            });
        state
            .platform
            .memberships
            .push(harborbeacon_local_agent::control_plane::users::Membership {
                membership_id: "membership-operator-1".to_string(),
                workspace_id: "home-1".to_string(),
                user_id: "operator-1".to_string(),
                role_kind: RoleKind::Operator,
                status: MembershipStatus::Active,
                granted_by_user_id: Some("local-owner".to_string()),
                granted_at: None,
            });
        fs::write(
            &admin_path,
            serde_json::to_vec_pretty(&state).expect("serialize state"),
        )
        .expect("write state");

        let api = AdminApi::new(
            admin_store,
            task_service,
            PathBuf::from("frontend/harbordesk/dist/harbordesk"),
            "http://harborbeacon.local:4174".to_string(),
        );
        let hints = AccessIdentityHints {
            user_id: Some("operator-1".to_string()),
            ..AccessIdentityHints::default()
        };

        let principal = api
            .authorize_workspace_camera_action(&hints)
            .expect("operator should be allowed to operate cameras");
        assert_eq!(principal.user_id, "operator-1");
        assert_eq!(principal.role_kind, RoleKind::Operator);
        assert!(api
            .authorize_admin_action(&hints, AccessAction::AdminManage)
            .is_err());

        let error = api
            .manual_add(&principal, build_manual_add_request(""))
            .expect_err("operator manual add should still require approval");
        assert!(error.contains("approval_token"));
        let approvals = conversation_store
            .pending_approvals()
            .expect("load pending approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].policy_ref, "camera.connect");
        assert_eq!(approvals[0].requester_user_id, "operator-1");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
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
    fn live_bridge_provider_from_setup_status_prefers_connected_channel() {
        let payload = json!({
            "public_origin": "http://192.168.3.169:8787",
            "channels": [
                {
                    "platform": "webhook",
                    "enabled": true,
                    "connected": false,
                    "display_name": "Webhook",
                    "capabilities": {
                        "reply": true,
                        "update": false,
                        "attachments": true
                    }
                },
                {
                    "platform": "weixin",
                    "enabled": true,
                    "connected": true,
                    "display_name": "Weixin",
                    "capabilities": {
                        "reply": true,
                        "update": false,
                        "attachments": false
                    }
                }
            ]
        });

        let provider = live_bridge_provider_from_setup_status(&payload).expect("provider");
        assert!(provider.configured);
        assert!(provider.connected);
        assert_eq!(provider.platform, "weixin");
        assert_eq!(provider.app_name, "Weixin");
        assert_eq!(provider.status, "已连接");
    }

    #[test]
    fn bridge_provider_binding_projection_marks_connected_gateway() {
        let provider = harborbeacon_local_agent::runtime::admin_console::BridgeProviderConfig {
            configured: true,
            connected: true,
            platform: "weixin".to_string(),
            app_name: "Weixin".to_string(),
            ..Default::default()
        };
        let mut status = String::new();
        let mut metric = String::new();
        let mut bound_user = None;

        apply_bridge_provider_binding_projection(
            &mut status,
            &mut metric,
            &mut bound_user,
            &provider,
        );

        assert_eq!(status, "Gateway 已连接");
        assert_eq!(metric, "Gateway 在线");
        assert_eq!(bound_user, Some("Weixin".to_string()));
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
        assert!(is_admin_surface_path("/api/account-management"));
        assert!(is_admin_surface_path("/api/gateway/status"));
        assert!(is_admin_surface_path("/api/share-links"));
        assert!(is_admin_surface_path("/api/models/endpoints"));
        assert!(is_admin_surface_path("/api/models/policies"));
        assert!(is_admin_surface_path("/admin/models"));
        assert!(is_admin_surface_path("/api/models/endpoints/ocr-local"));
        assert!(is_admin_surface_path(
            "/api/models/endpoints/ocr-local/test"
        ));
        assert!(is_admin_surface_path("/api/access/members/user-1/role"));
        assert!(is_admin_surface_path(
            "/api/access/members/user-1/default-delivery-surface"
        ));
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
    fn harbordesk_client_routes_are_identified() {
        assert!(is_harbordesk_client_route("/"));
        assert!(is_harbordesk_client_route("/overview"));
        assert!(is_harbordesk_client_route("/models-policies"));
        assert!(is_harbordesk_surface_path("/assets/runtime.js"));
        assert!(is_harbordesk_surface_path("/main.js"));
        assert!(!is_harbordesk_surface_path("/api/state"));
        assert!(!is_harbordesk_surface_path("/setup/mobile"));
    }

    #[test]
    fn harbordesk_asset_paths_reject_parent_segments() {
        let root = PathBuf::from("C:/harbordesk-dist");
        assert_eq!(
            resolve_harbordesk_asset_path(&root, "/assets/main.js"),
            Some(root.join("assets").join("main.js"))
        );
        assert_eq!(resolve_harbordesk_asset_path(&root, "/../secret.txt"), None);
        assert_eq!(resolve_harbordesk_asset_path(&root, "/overview"), None);
    }

    #[test]
    fn static_file_helpers_set_expected_mime_types() {
        assert_eq!(
            mime_type_for_path(Path::new("C:/tmp/index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            mime_type_for_path(Path::new("C:/tmp/main.js")),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(
            mime_type_for_path(Path::new("C:/tmp/icon.svg")),
            "image/svg+xml"
        );
    }

    #[test]
    fn harbordesk_build_missing_response_mentions_dist_path() {
        let response =
            harbordesk_build_missing_response(Path::new("frontend/harbordesk/dist/harbordesk"));
        assert_eq!(response.status_code(), StatusCode(503));
    }

    #[test]
    fn account_management_redaction_clears_gateway_credentials() {
        let registry_path = unique_store_path("harborbeacon-account-registry");
        let admin_path = unique_store_path("harborbeacon-account-state");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let state = admin_store.load_or_create_state().expect("state");
        let snapshot =
            harborbeacon_local_agent::runtime::admin_console::account_management_snapshot(
                &state,
                Some("http://harborbeacon.local:4174"),
            );
        let redacted = redact_account_management_snapshot(snapshot);

        assert_eq!(redacted.gateway.bridge_provider.app_id, "");
        assert_eq!(redacted.gateway.bridge_provider.app_secret, "");
        assert_eq!(redacted.gateway.bridge_provider.bot_open_id, "");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
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
    fn member_default_delivery_surface_paths_decode_ids() {
        let encoded = "user%2F1";
        assert_eq!(
            parse_member_default_delivery_surface_update_path(&format!(
                "/api/access/members/{encoded}/default-delivery-surface"
            )),
            Some("user/1".to_string())
        );
    }

    #[test]
    fn notification_target_delete_paths_decode_ids() {
        let encoded = "target%2F1";
        assert_eq!(
            parse_notification_target_delete_path(&format!(
                "/api/admin/notification-targets/{encoded}"
            )),
            Some("target/1".to_string())
        );
    }

    #[test]
    fn model_endpoint_paths_decode_ids() {
        let encoded = "ocr%2Flocal";
        assert_eq!(
            parse_model_endpoint_path(&format!("/api/models/endpoints/{encoded}")),
            Some("ocr/local".to_string())
        );
        assert_eq!(
            parse_model_endpoint_test_path(&format!("/api/models/endpoints/{encoded}/test")),
            Some("ocr/local".to_string())
        );
    }

    #[test]
    fn model_endpoint_response_redacts_secret_metadata() {
        let payload = redact_model_endpoint_response(&[ModelEndpoint {
            model_endpoint_id: "llm-cloud".to_string(),
            workspace_id: Some("home-1".to_string()),
            provider_account_id: None,
            model_kind: ModelKind::Llm,
            endpoint_kind: ModelEndpointKind::Cloud,
            provider_key: "custom".to_string(),
            model_name: "demo".to_string(),
            capability_tags: vec!["chat".to_string()],
            cost_policy: json!({}),
            status: ModelEndpointStatus::Active,
            metadata: json!({
                "base_url": "https://api.example.com/v1",
                "api_key": "super-secret",
                "nested": {
                    "token": "hidden-token"
                }
            }),
        }]);

        assert_eq!(payload.endpoints.len(), 1);
        assert_eq!(payload.endpoints[0].metadata["api_key"], json!(""));
        assert_eq!(
            payload.endpoints[0].metadata["api_key_configured"],
            json!(true)
        );
        assert_eq!(payload.endpoints[0].metadata["nested"]["token"], json!(""));
        assert_eq!(
            payload.endpoints[0].metadata["nested"]["token_configured"],
            json!(true)
        );
    }

    #[test]
    fn request_identity_hints_prefer_headers_then_query() {
        let headers = vec![
            Header::from_bytes(b"X-Harbor-Open-Id".as_slice(), b"ou_header".as_slice())
                .expect("header"),
            Header::from_bytes(b"X-Harbor-User-Id".as_slice(), b"user-header".as_slice())
                .expect("header"),
            Header::from_bytes(b"X-HarborOS-User".as_slice(), b"harbor".as_slice())
                .expect("header"),
        ];

        let hints = request_identity_hints(
            "/live/cameras/cam-1?open_id=ou_query&user_id=user-query&harboros_user=harbor-query",
            &headers,
        );
        assert_eq!(hints.open_id.as_deref(), Some("ou_header"));
        assert_eq!(hints.user_id.as_deref(), Some("user-header"));
        assert_eq!(hints.harboros_user_id.as_deref(), Some("harbor"));
    }

    #[test]
    fn deprecated_binding_routes_return_gone() {
        let admin_path = unique_store_path("harborbeacon-binding-gone-state");
        let registry_path = unique_store_path("harborbeacon-binding-gone-registry");
        let conversation_path = unique_store_path("harborbeacon-binding-gone-runtime");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        admin_store
            .save_bridge_provider_status(BridgeProviderConfig {
                gateway_base_url: "http://gateway.local:8787".to_string(),
                ..Default::default()
            })
            .expect("save bridge provider");
        let task_service = TaskApiService::new(
            admin_store.clone(),
            TaskConversationStore::new(conversation_path.clone()),
        );
        let api = AdminApi::new(
            admin_store,
            task_service,
            PathBuf::from("frontend/harbordesk/dist/harbordesk"),
            "http://harborbeacon.local:4174".to_string(),
        );

        let qr_response = api.handle_binding_qr_svg(&AccessIdentityHints::default());
        let page_response = api.handle_mobile_setup_page("/setup/mobile", &AccessIdentityHints::default());

        assert_eq!(qr_response.status_code(), StatusCode(410));
        assert_eq!(page_response.status_code(), StatusCode(410));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
    }

    #[test]
    fn gateway_service_request_requires_matching_bearer_token() {
        let headers = vec![Header::from_bytes(
            b"Authorization".as_slice(),
            b"Bearer shared-token".as_slice(),
        )
        .expect("header")];
        let _guard = EnvGuard::set("HARBORGATE_BEARER_TOKEN", "shared-token");

        assert!(authorize_gateway_service_request(&headers).is_ok());
        assert!(authorize_gateway_service_request(&[]).is_err());
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
            PathBuf::from("frontend/harbordesk/dist/harbordesk"),
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
            PathBuf::from("frontend/harbordesk/dist/harbordesk"),
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
