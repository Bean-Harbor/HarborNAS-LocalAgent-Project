use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

use hf_hub::{
    api::{sync::ApiBuilder as HfApiBuilder, Progress as HfProgress},
    Cache as HfCache, Repo, RepoType,
};
use reqwest::blocking::Client;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};
use uuid::Uuid;

use harborbeacon_local_agent::adapters::rtsp::{CommandRtspAdapter, RtspProbeAdapter};
use harborbeacon_local_agent::connectors::im_gateway::GatewayPlatformStatus;
use harborbeacon_local_agent::control_plane::media::{
    MediaAsset, MediaAssetKind, MediaSession, MediaSessionStatus, ShareLink,
};
use harborbeacon_local_agent::control_plane::models::{
    ModelEndpoint, ModelEndpointKind, ModelEndpointStatus, ModelKind, ModelRoutePolicy,
    PrivacyLevel,
};
use harborbeacon_local_agent::control_plane::users::{MembershipStatus, RoleKind};
use harborbeacon_local_agent::runtime::access_control::{
    authorize_access, AccessAction, AccessIdentityHints, AccessPrincipal,
};
use harborbeacon_local_agent::runtime::admin_console::{
    account_management_snapshot, dedupe_rtsp_paths, default_capture_subdirectory,
    default_clip_length_seconds, default_keyframe_count, default_keyframe_interval_seconds,
    default_model_endpoints, device_rtsp_credential_id, harboros_writable_root,
    normalize_delivery_surface, path_is_same_or_inside, user_default_delivery_surface,
    user_recent_interactive_surface, validate_knowledge_settings, AccountManagementSnapshot,
    AdminConsoleState, AdminConsoleStore, AdminDefaults, BridgeProviderConfig,
    DeviceCredentialSecret, DeviceEvidenceRecord, GatewayStatusSummary, KnowledgeIndexJobRecord,
    KnowledgeSettings, KnowledgeSourceRoot, ModelDownloadJobRecord, RagResourceProfile,
};
use harborbeacon_local_agent::runtime::discovery::RtspProbeRequest;
use harborbeacon_local_agent::runtime::dvr::{
    apply_retention_policy, build_status_response, scan_timeline, DvrRecordingSettings, DvrRuntime,
};
use harborbeacon_local_agent::runtime::hub::{
    CameraConnectRequest, CameraHubService, HubManualAddSummary, HubScanRequest, HubScanSummary,
    HubStateSnapshot,
};
use harborbeacon_local_agent::runtime::knowledge::{
    KnowledgeSearchRequest, KnowledgeSearchService,
};
use harborbeacon_local_agent::runtime::knowledge_index::{
    load_embedding_store, KnowledgeIndexConfig, KnowledgeIndexManifest, KnowledgeIndexService,
    KnowledgeModality,
};
use harborbeacon_local_agent::runtime::media_tools::{ffmpeg_resolution_hint, resolve_ffmpeg_bin};
use harborbeacon_local_agent::runtime::model_center::{
    redact_model_endpoint, test_model_endpoint, ModelEndpointTestResult, ADMIN_STATE_PATH_ENV,
};
use harborbeacon_local_agent::runtime::registry::{
    CameraCapabilities, CameraDevice, DeviceRegistryStore,
};
use harborbeacon_local_agent::runtime::remote_view;
use harborbeacon_local_agent::runtime::task_api::{
    TaskApiService, TaskApprovalSummary, TaskIntent, TaskRequest, TaskResponse, TaskSource,
    TaskStatus,
};
use harborbeacon_local_agent::runtime::task_session::TaskConversationStore;

const DEFAULT_HF_ENDPOINT: &str = "https://hf-mirror.com";

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
pub struct AdminApi {
    admin_store: AdminConsoleStore,
    task_service: TaskApiService,
    dvr_runtime: DvrRuntime,
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
struct DefaultCameraRequest {
    #[serde(default)]
    device_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct DeviceCredentialsRequest {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    rtsp_port: Option<u16>,
    #[serde(default)]
    rtsp_paths: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RtspCheckRequest {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    rtsp_port: Option<u16>,
    #[serde(default)]
    rtsp_paths: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct DeviceMetadataPatchRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    room: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    ip_address: Option<String>,
    #[serde(default)]
    snapshot_url: Option<String>,
    #[serde(default)]
    primary_stream_url: Option<String>,
    #[serde(default)]
    rtsp_path: Option<String>,
    #[serde(default)]
    rtsp_port: Option<u16>,
    #[serde(default)]
    requires_auth: Option<bool>,
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

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct FeatureAvailabilityResponse {
    groups: Vec<FeatureAvailabilityGroup>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct FeatureAvailabilityGroup {
    group_id: String,
    label: String,
    items: Vec<FeatureAvailabilityItem>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct FeatureAvailabilityItem {
    feature_id: String,
    label: String,
    owner_lane: String,
    status: String,
    source_of_truth: String,
    current_option: String,
    fallback_order: Vec<String>,
    blocker: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReleaseReadinessResponse {
    generated_at: String,
    checked_at: String,
    status: String,
    summary: String,
    overall_status: String,
    harbor_desk: ReadinessSurfaceSummary,
    groups: Vec<ReleaseReadinessGroup>,
    checklist: Vec<ReleaseReadinessItem>,
    status_cards: Vec<ReleaseReadinessStatusCard>,
    deep_links: Vec<ReleaseReadinessDeepLink>,
    blockers: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReadinessSurfaceSummary {
    admin_origin: String,
    admin_port: u16,
    harboros_webui: String,
    note: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReleaseReadinessGroup {
    group_id: String,
    label: String,
    owner_lane: String,
    status: String,
    items: Vec<ReleaseReadinessItem>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReleaseReadinessItem {
    id: String,
    item_id: String,
    label: String,
    lane: String,
    owner_lane: String,
    status: String,
    summary: String,
    detail: String,
    endpoint: String,
    source_of_truth: String,
    deep_link: String,
    next_action: String,
    action_path: String,
    last_verified_at: Option<String>,
    blocking_reason: String,
    blockers: Vec<String>,
    evidence: Vec<String>,
    evidence_records: Vec<ReadinessEvidenceRecord>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReadinessEvidenceRecord {
    generated_at: String,
    lane: String,
    status: String,
    action_path: String,
    blocking_reason: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReleaseReadinessStatusCard {
    id: String,
    label: String,
    value: String,
    status: String,
    detail: String,
    endpoint: String,
    deep_link: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReleaseReadinessDeepLink {
    label: String,
    href: String,
    detail: String,
    endpoint: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ReleaseReadinessHistoryResponse {
    generated_at: String,
    entries: Vec<ReleaseReadinessResponse>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct HardwareReadinessResponse {
    generated_at: String,
    status: String,
    cpu: HardwareComponentReadiness,
    memory: HardwareComponentReadiness,
    gpu: HardwareComponentReadiness,
    npu: HardwareComponentReadiness,
    recommended_model_profile: String,
    blockers: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct HardwareComponentReadiness {
    status: String,
    summary: String,
    detail: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct HarborOsStatusResponse {
    generated_at: String,
    status: String,
    version: String,
    webui_url: String,
    system_domain_only: bool,
    services: Vec<HarborOsServiceStatus>,
    jobs_alerts: HarborOsServiceStatus,
    storage_files_entry: HarborOsServiceStatus,
    evidence: Vec<String>,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct HarborOsServiceStatus {
    service_id: String,
    label: String,
    status: String,
    detail: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct HarborOsImCapabilityMapResponse {
    generated_at: String,
    source: String,
    items: Vec<HarborOsImCapabilityItem>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct HarborOsImCapabilityItem {
    capability_id: String,
    label: String,
    capability_class: String,
    im_ready: bool,
    risk_level: String,
    approval_required: bool,
    harboros_surface: String,
    notes: String,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
struct RagReadinessResponse {
    generated_at: String,
    status: String,
    summary: String,
    source_roots: RagReadinessComponent,
    index_directory: RagReadinessComponent,
    embedding_model: RagReadinessComponent,
    model_readiness: Vec<RagModelReadinessCard>,
    resource_profiles: Vec<RagResourceProfileStatus>,
    capability_profiles: Vec<RagCapabilityReadinessCard>,
    privacy_policy: RagReadinessComponent,
    media_parser: RagReadinessComponent,
    storage_writable: RagReadinessComponent,
    index_jobs: Vec<KnowledgeIndexJobRecord>,
    blockers: Vec<String>,
    warnings: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct RagReadinessComponent {
    status: String,
    summary: String,
    detail: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct RagModelReadinessCard {
    model_kind: String,
    label: String,
    status: String,
    endpoint_id: Option<String>,
    endpoint_kind: Option<String>,
    provider_key: Option<String>,
    model_name: Option<String>,
    detail: String,
    blocker: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct RagResourceProfileStatus {
    profile: String,
    label: String,
    status: String,
    detail: String,
    blockers: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct RagCapabilityReadinessCard {
    capability_id: String,
    label: String,
    status: String,
    summary: String,
    blockers: Vec<String>,
    warnings: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct KnowledgeIndexRunResponse {
    generated_at: String,
    job_ids: Vec<String>,
    status: String,
    index_root: String,
    root_count: usize,
    indexed_roots: Vec<KnowledgeIndexRootStatus>,
    errors: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
struct KnowledgeSearchApiRequest {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    include_documents: Option<bool>,
    #[serde(default)]
    include_images: Option<bool>,
    #[serde(default)]
    include_videos: Option<bool>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct KnowledgeIndexStatusResponse {
    generated_at: String,
    status: String,
    settings: KnowledgeSettings,
    index_root_exists: bool,
    index_root_writable: bool,
    manifest_count: usize,
    manifest_entry_count: usize,
    document_count: usize,
    image_count: usize,
    audio_count: usize,
    video_count: usize,
    content_indexed_image_count: usize,
    vlm_indexed_image_count: usize,
    ocr_indexed_image_count: usize,
    image_content_missing_count: usize,
    image_text_source_counts: BTreeMap<String, usize>,
    embedding_cache_count: usize,
    embedding_entry_count: usize,
    storage_usage_bytes: u64,
    last_indexed_at: Option<String>,
    source_roots: Vec<KnowledgeIndexRootStatus>,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct KnowledgeIndexRootStatus {
    root_id: String,
    label: String,
    path: String,
    enabled: bool,
    exists: bool,
    last_indexed_at: Option<String>,
    status: String,
    detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct KnowledgeIndexStorageSummary {
    manifest_count: usize,
    manifest_entry_count: usize,
    document_count: usize,
    image_count: usize,
    audio_count: usize,
    video_count: usize,
    content_indexed_image_count: usize,
    vlm_indexed_image_count: usize,
    ocr_indexed_image_count: usize,
    image_content_missing_count: usize,
    image_text_source_counts: BTreeMap<String, usize>,
    embedding_cache_count: usize,
    embedding_entry_count: usize,
    storage_usage_bytes: u64,
    last_indexed_at: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct FilesBrowseResponse {
    path: String,
    parent: Option<String>,
    readonly: bool,
    allowed_roots: Vec<String>,
    entries: Vec<FileBrowseEntry>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct FileBrowseEntry {
    name: String,
    path: String,
    is_dir: bool,
    size_bytes: Option<u64>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct LocalModelCatalogResponse {
    generated_at: String,
    checked_at: String,
    status: String,
    cache_roots: Vec<String>,
    models: Vec<LocalModelCatalogItem>,
    download_jobs: Vec<ModelDownloadJobRecord>,
    downloads: Vec<ModelDownloadJobRecord>,
    blockers: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct LocalModelCatalogItem {
    model_id: String,
    label: String,
    display_name: String,
    provider: String,
    provider_key: String,
    model_kind: String,
    recommended_hardware: String,
    status: String,
    installed: bool,
    local_path: Option<String>,
    size_bytes: Option<u64>,
    download_job_id: Option<String>,
    download_size_hint: String,
    detail: String,
    source_kind: String,
    repo_id: Option<String>,
    revision: Option<String>,
    file_policy: String,
    default_hf_endpoint: Option<String>,
    runtime_profiles: Vec<String>,
    expected_capabilities: Vec<String>,
    acceptance_note: Option<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ModelDownloadJobResponse {
    #[serde(flatten)]
    record: ModelDownloadJobRecord,
    job: ModelDownloadJobRecord,
}

impl ModelDownloadJobResponse {
    fn new(job: ModelDownloadJobRecord) -> Self {
        Self {
            record: job.clone(),
            job,
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct ModelDownloadJobsResponse {
    generated_at: String,
    checked_at: String,
    status: String,
    jobs: Vec<ModelDownloadJobRecord>,
    downloads: Vec<ModelDownloadJobRecord>,
    blockers: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalModelRuntimeProjection {
    base_url: String,
    healthz_url: String,
    api_key_configured: bool,
    ready: bool,
    backend_ready: bool,
    backend_kind: Option<String>,
    chat_model: Option<String>,
    embedding_model: Option<String>,
    note: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelPoliciesRequest {
    #[serde(default)]
    route_policies: Vec<ModelRoutePolicy>,
}

#[derive(Debug, Deserialize, Default)]
struct ModelDownloadRequest {
    model_id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    provider_key: Option<String>,
    #[serde(default)]
    target_path: Option<String>,
    #[serde(default)]
    hf_endpoint: Option<String>,
    #[serde(default)]
    metadata: Value,
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
    #[serde(default)]
    device_credential_statuses: Vec<DeviceCredentialStatusResponse>,
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

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct DeviceCredentialStatusResponse {
    device_id: String,
    configured: bool,
    redacted: bool,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    rtsp_port: Option<u16>,
    path_count: usize,
    source: String,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    last_verified_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct RtspCheckResponse {
    device_id: String,
    reachable: bool,
    #[serde(default)]
    stream_url: Option<String>,
    transport: String,
    requires_auth: bool,
    capabilities: CameraCapabilities,
    #[serde(default)]
    error_message: Option<String>,
    checked_at: String,
}

#[derive(Debug, Serialize)]
struct DeviceEvidenceResponse {
    device_id: String,
    generated_at: String,
    credential_status: DeviceCredentialStatusResponse,
    #[serde(default)]
    recent_rtsp_check: Option<DeviceEvidenceRecord>,
    #[serde(default)]
    recent_snapshot_check: Option<DeviceEvidenceRecord>,
    #[serde(default)]
    share_links: Vec<ShareLinkSummary>,
    #[serde(default)]
    evidence: Vec<DeviceEvidenceRecord>,
}

#[derive(Debug, Serialize)]
struct DeviceValidationRunResponse {
    validation_id: String,
    device_id: String,
    status: String,
    rtsp_check: DeviceEvidenceRecord,
    snapshot_check: DeviceEvidenceRecord,
    evidence: DeviceEvidenceResponse,
}

type StateResponse = HubStateSnapshot;
type ScanRequest = HubScanRequest;
type ScanResponse = HubScanSummary;
type ManualAddResponse = HubManualAddSummary;

impl AdminApi {
    pub fn new(
        admin_store: AdminConsoleStore,
        task_service: TaskApiService,
        harbordesk_dist: PathBuf,
        public_origin: String,
    ) -> Self {
        Self {
            admin_store,
            task_service,
            dvr_runtime: DvrRuntime::default(),
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

    pub fn handle(&self, mut request: Request) {
        let method = request.method().clone();
        let raw_url = normalize_unified_admin_url(&request.url().to_string());
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
            Method::Get if path == "/api/release/readiness" => {
                self.handle_release_readiness(&identity_hints).boxed()
            }
            Method::Get if path == "/api/release/readiness/history" => self
                .handle_release_readiness_history(&identity_hints)
                .boxed(),
            Method::Get if path == "/api/hardware/readiness" => {
                self.handle_hardware_readiness(&identity_hints).boxed()
            }
            Method::Get if path == "/api/rag/readiness" => {
                self.handle_rag_readiness(&identity_hints).boxed()
            }
            Method::Get if path == "/api/knowledge/settings" => {
                self.handle_knowledge_settings(&identity_hints).boxed()
            }
            Method::Put if path == "/api/knowledge/settings" => self
                .handle_save_knowledge_settings(&mut request, &identity_hints)
                .boxed(),
            Method::Post if path == "/api/knowledge/search" => self
                .handle_knowledge_search(&mut request, &identity_hints)
                .boxed(),
            Method::Get if path == "/api/knowledge/preview" => self
                .handle_knowledge_preview(&raw_url, &identity_hints)
                .boxed(),
            Method::Post if path == "/api/knowledge/index/run" => {
                self.handle_run_knowledge_index(&identity_hints).boxed()
            }
            Method::Get if path == "/api/knowledge/index/status" => {
                self.handle_knowledge_index_status(&identity_hints).boxed()
            }
            Method::Get if path == "/api/knowledge/index/jobs" => {
                self.handle_knowledge_index_jobs(&identity_hints).boxed()
            }
            Method::Post
                if path.starts_with("/api/knowledge/index/jobs/") && path.ends_with("/cancel") =>
            {
                self.handle_cancel_knowledge_index_job(&path, &identity_hints)
                    .boxed()
            }
            Method::Get if path == "/api/files/browse" => {
                self.handle_files_browse(&raw_url, &identity_hints).boxed()
            }
            Method::Get if path == "/api/cameras/recording-settings" => {
                self.handle_dvr_recording_settings(&identity_hints).boxed()
            }
            Method::Put if path == "/api/cameras/recording-settings" => self
                .handle_save_dvr_recording_settings(&mut request, &identity_hints)
                .boxed(),
            Method::Get if path == "/api/cameras/recordings/status" => {
                self.handle_dvr_recordings_status(&identity_hints).boxed()
            }
            Method::Get if path == "/api/cameras/recordings/timeline" => self
                .handle_dvr_recordings_timeline(&raw_url, &identity_hints)
                .boxed(),
            Method::Get if path == "/api/harboros/status" => {
                self.handle_harboros_status(&identity_hints).boxed()
            }
            Method::Get if path == "/api/harboros/im-capability-map" => self
                .handle_harboros_im_capability_map(&identity_hints)
                .boxed(),
            Method::Get if path == "/api/models/endpoints" => {
                self.handle_model_endpoints(&identity_hints).boxed()
            }
            Method::Get if path == "/api/models/local-catalog" => {
                self.handle_local_model_catalog(&identity_hints).boxed()
            }
            Method::Get if path == "/api/models/local-downloads" => {
                self.handle_model_download_jobs(&identity_hints).boxed()
            }
            Method::Get if path.starts_with("/api/models/local-downloads/") => self
                .handle_model_download_job(&path, &identity_hints)
                .boxed(),
            Method::Get if path == "/api/feature-availability" => {
                self.handle_feature_availability(&identity_hints).boxed()
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
            Method::Get if path.starts_with("/api/devices/") && path.ends_with("/evidence") => {
                self.handle_device_evidence(&path, &identity_hints).boxed()
            }
            Method::Get
                if path.starts_with("/api/devices/") && path.ends_with("/credential-status") =>
            {
                self.handle_device_credential_status(&path, &identity_hints)
                    .boxed()
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
            Method::Post if path == "/api/release/readiness/run" => {
                self.handle_run_release_readiness(&identity_hints).boxed()
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
            Method::Post if path == "/api/models/local-downloads" => self
                .handle_create_model_download(&mut request, &identity_hints)
                .boxed(),
            Method::Post
                if path.starts_with("/api/models/local-downloads/")
                    && path.ends_with("/cancel") =>
            {
                self.handle_cancel_model_download(&path, &identity_hints)
                    .boxed()
            }
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
            Method::Patch if path.starts_with("/api/devices/") => self
                .handle_patch_device_metadata(path.as_str(), &mut request, &identity_hints)
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
            Method::Post if path == "/api/devices/default-camera" => self
                .handle_set_default_camera(&mut request, &identity_hints)
                .boxed(),
            Method::Post if path.starts_with("/api/devices/") && path.ends_with("/credentials") => {
                self.handle_save_device_credentials(&path, &mut request, &identity_hints)
                    .boxed()
            }
            Method::Post if path.starts_with("/api/devices/") && path.ends_with("/rtsp-check") => {
                self.handle_rtsp_check(&path, &mut request, &identity_hints)
                    .boxed()
            }
            Method::Post
                if path.starts_with("/api/devices/") && path.ends_with("/validation/run") =>
            {
                self.handle_device_validation_run(&path, &identity_hints)
                    .boxed()
            }
            Method::Post if path.starts_with("/api/cameras/") && path.ends_with("/share-link") => {
                self.handle_camera_share_link(&path, &identity_hints)
                    .boxed()
            }
            Method::Post
                if path.starts_with("/api/cameras/") && path.ends_with("/recordings/start") =>
            {
                self.handle_dvr_recording_start(&path, &identity_hints)
                    .boxed()
            }
            Method::Post
                if path.starts_with("/api/cameras/") && path.ends_with("/recordings/stop") =>
            {
                self.handle_dvr_recording_stop(&path, &identity_hints)
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
                    let device_credential_statuses =
                        build_device_credential_statuses(&state, &payload.devices);
                    ok_json(&AdminStateResponse {
                        state: redact_state_snapshot(payload),
                        account_management: redact_account_management_snapshot(account_management),
                        device_credential_statuses,
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

    fn handle_release_readiness(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }

        self.refresh_gateway_projection_best_effort();
        let live_gateway_status = fetch_remote_gateway_status().ok();
        let live_bridge_provider = live_gateway_status
            .as_ref()
            .and_then(live_bridge_provider_from_setup_status);

        match self.admin_store.load_or_create_state() {
            Ok(state) => {
                let state_snapshot = self.current_state().ok().map(redact_state_snapshot);
                let runtime_projection = probe_local_model_runtime(&state.models.endpoints);
                let endpoints = overlay_model_endpoints_with_runtime_truth(
                    &state.models.endpoints,
                    &runtime_projection,
                );
                let mut account_management =
                    account_management_snapshot(&state, Some(&self.public_origin));
                if let Some(provider) = live_bridge_provider.as_ref() {
                    apply_bridge_provider_projection_to_gateway_summary(
                        &mut account_management.gateway,
                        provider,
                    );
                }
                let account_management = redact_account_management_snapshot(account_management);
                let feature_availability = build_feature_availability_response(
                    &endpoints,
                    &state.models.route_policies,
                    &account_management,
                    live_gateway_status.as_ref(),
                    &runtime_projection,
                );
                let hardware = build_hardware_readiness_response();
                let harboros = build_harboros_status_response(&self.public_origin);
                let rag = build_rag_readiness_response(
                    &runtime_projection,
                    &state.knowledge,
                    &state.models.endpoints,
                    &state.knowledge_index_jobs,
                );
                let response = build_release_readiness_response(
                    &self.public_origin,
                    state_snapshot.as_ref(),
                    &account_management,
                    &feature_availability,
                    &hardware,
                    &harboros,
                    &rag,
                    &runtime_projection,
                );
                ok_json(&response)
            }
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_run_release_readiness(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        self.handle_release_readiness(hints)
    }

    fn handle_release_readiness_history(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }

        self.refresh_gateway_projection_best_effort();
        let live_gateway_status = fetch_remote_gateway_status().ok();
        let live_bridge_provider = live_gateway_status
            .as_ref()
            .and_then(live_bridge_provider_from_setup_status);

        match self.admin_store.load_or_create_state() {
            Ok(state) => {
                let state_snapshot = self.current_state().ok().map(redact_state_snapshot);
                let runtime_projection = probe_local_model_runtime(&state.models.endpoints);
                let endpoints = overlay_model_endpoints_with_runtime_truth(
                    &state.models.endpoints,
                    &runtime_projection,
                );
                let mut account_management =
                    account_management_snapshot(&state, Some(&self.public_origin));
                if let Some(provider) = live_bridge_provider.as_ref() {
                    apply_bridge_provider_projection_to_gateway_summary(
                        &mut account_management.gateway,
                        provider,
                    );
                }
                let account_management = redact_account_management_snapshot(account_management);
                let feature_availability = build_feature_availability_response(
                    &endpoints,
                    &state.models.route_policies,
                    &account_management,
                    live_gateway_status.as_ref(),
                    &runtime_projection,
                );
                let hardware = build_hardware_readiness_response();
                let harboros = build_harboros_status_response(&self.public_origin);
                let rag = build_rag_readiness_response(
                    &runtime_projection,
                    &state.knowledge,
                    &state.models.endpoints,
                    &state.knowledge_index_jobs,
                );
                let current = build_release_readiness_response(
                    &self.public_origin,
                    state_snapshot.as_ref(),
                    &account_management,
                    &feature_availability,
                    &hardware,
                    &harboros,
                    &rag,
                    &runtime_projection,
                );
                ok_json(&ReleaseReadinessHistoryResponse {
                    generated_at: now_unix_string(),
                    entries: vec![current],
                })
            }
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_hardware_readiness(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        ok_json(&build_hardware_readiness_response())
    }

    fn handle_rag_readiness(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.load_or_create_state() {
            Ok(state) => {
                let runtime_projection = probe_local_model_runtime(&state.models.endpoints);
                ok_json(&build_rag_readiness_response(
                    &runtime_projection,
                    &state.knowledge,
                    &state.models.endpoints,
                    &state.knowledge_index_jobs,
                ))
            }
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_knowledge_settings(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.knowledge_settings() {
            Ok(settings) => ok_json(&settings),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_save_knowledge_settings(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let settings: KnowledgeSettings = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match validate_knowledge_settings(settings)
            .and_then(|settings| self.admin_store.save_knowledge_settings(settings))
        {
            Ok(state) => ok_json(&state.knowledge),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_knowledge_search(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        let payload: KnowledgeSearchApiRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        let settings = match self.admin_store.knowledge_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let search_request = match build_admin_knowledge_search_request(payload, &settings) {
            Ok(request) => request,
            Err(error) => return error_json(StatusCode(422), &error),
        };
        match KnowledgeSearchService::search(search_request) {
            Ok(response) => ok_json(&response),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_knowledge_preview(
        &self,
        raw_url: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        let requested_path = match parse_query_param(raw_url, "path")
            .and_then(percent_decode_optional_query_value)
            .and_then(|value| non_empty_string(&value))
        {
            Some(path) => path,
            None => return error_json(StatusCode(400), "knowledge preview requires path"),
        };
        let settings = match self.admin_store.knowledge_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let preview_path = match resolve_knowledge_preview_path(&requested_path, &settings) {
            Ok(path) => path,
            Err(error) => return error_json(error.status, &error.message),
        };
        static_file_response(&preview_path)
    }

    fn handle_dvr_recording_settings(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.dvr_recording_settings() {
            Ok(settings) => ok_json(&settings),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_save_dvr_recording_settings(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let settings: DvrRecordingSettings = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self.admin_store.save_dvr_recording_settings(settings) {
            Ok(state) => ok_json(&state.dvr),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_dvr_recordings_status(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        let settings = match self.admin_store.dvr_recording_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let devices = match self.hub().load_registered_cameras() {
            Ok(devices) => devices,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let statuses =
            match self
                .dvr_runtime
                .statuses(&devices, &settings, Some(&self.public_origin))
            {
                Ok(statuses) => statuses,
                Err(error) => return error_json(StatusCode(500), &error),
            };
        ok_json(&build_status_response(settings, statuses, devices.len()))
    }

    fn handle_dvr_recordings_timeline(
        &self,
        raw_url: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        let settings = match self.admin_store.dvr_recording_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        if let Err(error) = apply_retention_policy(&settings) {
            return error_json(StatusCode(422), &error);
        }
        let devices = match self.hub().load_registered_cameras() {
            Ok(devices) => devices,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let device_id = parse_query_param(raw_url, "device_id")
            .and_then(percent_decode_optional_query_value)
            .and_then(|value| non_empty_string(&value));
        let from_secs =
            parse_query_param(raw_url, "from").and_then(|value| value.parse::<u64>().ok());
        let to_secs = parse_query_param(raw_url, "to").and_then(|value| value.parse::<u64>().ok());
        match scan_timeline(
            &settings,
            &devices,
            device_id.as_deref(),
            from_secs,
            to_secs,
            Some(&self.public_origin),
        ) {
            Ok(response) => ok_json(&response),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_run_knowledge_index(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let settings = match self.admin_store.knowledge_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        if settings.enabled_source_root_paths().is_empty() {
            return error_json(
                StatusCode(422),
                "请先在 HarborDesk 配置并启用至少一个知识源目录。",
            );
        }
        if let Ok(jobs) = self.admin_store.list_knowledge_index_jobs() {
            if jobs
                .iter()
                .any(|job| matches!(job.status.as_str(), "queued" | "running"))
            {
                return error_json(
                    StatusCode(409),
                    "已有 knowledge index job 正在 queued/running；请等待完成或取消后再启动新的刷新。",
                );
            }
        }
        if let Err(error) = KnowledgeIndexConfig::new(PathBuf::from(settings.index_root.clone()))
            .and_then(KnowledgeIndexService::from_config)
        {
            return error_json(StatusCode(422), &error);
        }
        let generated_at = now_unix_string();
        let enabled_roots = settings
            .source_roots
            .iter()
            .filter(|root| root.enabled)
            .cloned()
            .collect::<Vec<_>>();
        let mut job_ids = Vec::new();
        let mut indexed_roots = Vec::new();
        let mut jobs = Vec::new();
        for root in &enabled_roots {
            let job =
                build_knowledge_index_job(root, &generated_at, settings.default_resource_profile);
            job_ids.push(job.job_id.clone());
            if let Err(error) = self.admin_store.save_knowledge_index_job(job.clone()) {
                return error_json(StatusCode(500), &error);
            }
            indexed_roots.push(queued_knowledge_root_status(root));
            jobs.push(job);
        }
        let worker_store = AdminConsoleStore::new(
            self.admin_store.path().to_path_buf(),
            self.admin_store.registry_store().clone(),
        );
        if let Err(error) =
            spawn_knowledge_index_worker(worker_store, settings.clone(), jobs.clone())
        {
            for mut job in jobs {
                job.status = "failed".to_string();
                job.progress_percent = Some(100);
                job.completed_at = Some(now_unix_string());
                job.error_message = Some(error.clone());
                job.checkpoint = json!({"phase": "worker_spawn_failed"});
                let _ = self.admin_store.save_knowledge_index_job(job);
            }
            return error_json(StatusCode(500), &error);
        }
        ok_json(&KnowledgeIndexRunResponse {
            generated_at,
            job_ids,
            status: "queued".to_string(),
            index_root: settings.index_root,
            root_count: indexed_roots.len(),
            indexed_roots,
            errors: Vec::new(),
        })
    }

    fn handle_knowledge_index_status(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.knowledge_settings() {
            Ok(settings) => ok_json(&build_knowledge_index_status_response(settings)),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_knowledge_index_jobs(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.list_knowledge_index_jobs() {
            Ok(jobs) => ok_json(&json!({
                "generated_at": now_unix_string(),
                "jobs": jobs,
            })),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_cancel_knowledge_index_job(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let Some(job_id) = parse_knowledge_index_job_cancel_path(path) else {
            return error_json(StatusCode(404), "knowledge index job route not found");
        };
        match self
            .admin_store
            .cancel_knowledge_index_job(&job_id, now_unix_string())
        {
            Ok(Some(job)) => ok_json(&job),
            Ok(None) => error_json(StatusCode(404), "knowledge index job not found"),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_files_browse(
        &self,
        raw_url: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        let settings = match self.admin_store.knowledge_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let requested_path = parse_query_param(raw_url, "path")
            .and_then(percent_decode_optional_query_value)
            .and_then(|value| non_empty_string(&value));
        match build_files_browse_response(requested_path.as_deref(), &settings) {
            Ok(response) => ok_json(&response),
            Err(error) if error.contains("not inside an allowed") => {
                error_json(StatusCode(403), &error)
            }
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_harboros_status(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        ok_json(&build_harboros_status_response(&self.public_origin))
    }

    fn handle_harboros_im_capability_map(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        ok_json(&build_harboros_im_capability_map())
    }

    fn handle_model_endpoints(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.load_or_create_state() {
            Ok(state) => {
                let runtime_projection = probe_local_model_runtime(&state.models.endpoints);
                let endpoints = overlay_model_endpoints_with_runtime_truth(
                    &state.models.endpoints,
                    &runtime_projection,
                );
                ok_json(&ModelEndpointsResponse {
                    endpoints: endpoints.iter().map(redact_model_endpoint).collect(),
                })
            }
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_local_model_catalog(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.list_model_download_jobs() {
            Ok(download_jobs) => ok_json(&build_local_model_catalog(download_jobs)),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_model_download_jobs(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        match self.admin_store.list_model_download_jobs() {
            Ok(jobs) => ok_json(&ModelDownloadJobsResponse {
                generated_at: now_unix_string(),
                checked_at: now_unix_string(),
                status: model_download_jobs_status(&jobs).to_string(),
                jobs: jobs.clone(),
                downloads: jobs,
                blockers: Vec::new(),
                warnings: Vec::new(),
            }),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_model_download_job(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }
        let job_id = match parse_model_download_job_path(path) {
            Some(job_id) => job_id,
            None => return error_json(StatusCode(400), "invalid model download job path"),
        };
        match self.admin_store.model_download_job(&job_id) {
            Ok(Some(job)) => ok_json(&ModelDownloadJobResponse::new(job)),
            Ok(None) => error_json(StatusCode(404), "model download job not found"),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_feature_availability(
        &self,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminReadState) {
            return error_json(StatusCode(403), &error);
        }

        self.refresh_gateway_projection_best_effort();
        let live_gateway_status = fetch_remote_gateway_status().ok();
        let live_bridge_provider = live_gateway_status
            .as_ref()
            .and_then(live_bridge_provider_from_setup_status);

        match self.admin_store.load_or_create_state() {
            Ok(state) => {
                let runtime_projection = probe_local_model_runtime(&state.models.endpoints);
                let endpoints = overlay_model_endpoints_with_runtime_truth(
                    &state.models.endpoints,
                    &runtime_projection,
                );
                let mut account_management =
                    account_management_snapshot(&state, Some(&self.public_origin));
                if let Some(provider) = live_bridge_provider.as_ref() {
                    apply_bridge_provider_projection_to_gateway_summary(
                        &mut account_management.gateway,
                        provider,
                    );
                }
                let response = build_feature_availability_response(
                    &endpoints,
                    &state.models.route_policies,
                    &account_management,
                    live_gateway_status.as_ref(),
                    &runtime_projection,
                );
                ok_json(&response)
            }
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

    fn handle_create_model_download(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let body: ModelDownloadRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        let catalog = build_local_model_catalog(Vec::new());
        let catalog_item = catalog
            .models
            .iter()
            .find(|item| item.model_id == body.model_id);
        let display_name = body
            .display_name
            .as_deref()
            .and_then(non_empty_string)
            .or_else(|| catalog_item.map(|item| item.display_name.clone()))
            .unwrap_or_else(|| body.model_id.clone());
        let provider_key = body
            .provider_key
            .as_deref()
            .and_then(non_empty_string)
            .or_else(|| catalog_item.map(|item| item.provider_key.clone()))
            .unwrap_or_else(|| "local".to_string());
        let target_path = body
            .target_path
            .as_deref()
            .and_then(non_empty_string)
            .or_else(|| catalog_item.and_then(|item| item.local_path.clone()))
            .or_else(|| Some(default_model_download_target_path(&body.model_id)));
        let mut metadata = if body.metadata.is_null() {
            json!({})
        } else {
            body.metadata
        };
        if let Some(catalog_item) = catalog_item {
            enrich_model_download_metadata(&mut metadata, catalog_item);
        }
        if let Some(hf_endpoint) = body.hf_endpoint.as_deref().and_then(non_empty_string) {
            if !metadata.is_object() {
                metadata = json!({});
            }
            if let Some(object) = metadata.as_object_mut() {
                object.insert("hf_endpoint".to_string(), json!(hf_endpoint));
            }
        }
        match self.admin_store.create_model_download_job(
            &body.model_id,
            &display_name,
            &provider_key,
            target_path,
            redact_secret_json_value(metadata),
        ) {
            Ok(job) => {
                let worker_store = AdminConsoleStore::new(
                    self.admin_store.path().to_path_buf(),
                    self.admin_store.registry_store().clone(),
                );
                match spawn_model_download_worker(worker_store, job.clone()) {
                    Ok(()) => ok_json(&ModelDownloadJobResponse::new(job)),
                    Err(error) => {
                        let _ =
                            mark_model_download_spawn_failed(&self.admin_store, job, error.clone());
                        error_json(StatusCode(500), &error)
                    }
                }
            }
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_cancel_model_download(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let job_id = match parse_model_download_cancel_path(path) {
            Some(job_id) => job_id,
            None => return error_json(StatusCode(400), "invalid model download cancel path"),
        };
        match self.admin_store.cancel_model_download_job(&job_id) {
            Ok(Some(job)) => ok_json(&ModelDownloadJobResponse::new(job)),
            Ok(None) => error_json(StatusCode(404), "model download job not found"),
            Err(error) => error_json(StatusCode(500), &error),
        }
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
            }) {
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
            }) {
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

    fn handle_set_default_camera(
        &self,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let body: DefaultCameraRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        let requested_device_id = body.device_id.and_then(|value| non_empty_string(&value));
        if let Some(device_id) = requested_device_id.as_deref() {
            if let Err(error) = self.load_camera_device(device_id) {
                return if error.contains("device not found") {
                    error_json(StatusCode(404), &error)
                } else {
                    error_json(StatusCode(422), &error)
                };
            }
        }

        let mut state = match self.admin_store.load_or_create_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        state.defaults.selected_camera_device_id = requested_device_id;
        match self
            .hub()
            .save_defaults(state.defaults, Some(&self.public_origin))
        {
            Ok(payload) => ok_json(&redact_state_snapshot(payload)),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_patch_device_metadata(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_device_metadata_patch_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid device metadata patch path"),
        };
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let body: DeviceMetadataPatchRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self.patch_device_metadata(&device_id, body) {
            Ok(payload) => ok_json(&redact_state_snapshot(payload)),
            Err(error) if error.contains("device not found") => error_json(StatusCode(404), &error),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_device_credential_status(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_device_credential_status_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid device credential-status path"),
        };
        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraView)
        {
            return error_json(StatusCode(403), &error);
        }
        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };
        match self.admin_store.load_or_create_state() {
            Ok(state) => ok_json(&build_device_credential_status(&state, &device)),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_device_evidence(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_device_evidence_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid device evidence path"),
        };
        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraView)
        {
            return error_json(StatusCode(403), &error);
        }
        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };
        match self.build_device_evidence_response(&device) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_save_device_credentials(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_device_credentials_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid device credentials path"),
        };
        if let Err(error) = self.authorize_admin_action(hints, AccessAction::AdminManage) {
            return error_json(StatusCode(403), &error);
        }
        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };
        let body: DeviceCredentialsRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        let state = match self.admin_store.load_or_create_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let existing = state
            .device_credentials
            .iter()
            .find(|credential| credential.device_id == device_id);
        let username = body
            .username
            .as_deref()
            .and_then(non_empty_string)
            .or_else(|| existing.map(|credential| credential.username.clone()))
            .or_else(|| non_empty_string(&state.defaults.rtsp_username))
            .unwrap_or_default();
        let password = body
            .password
            .as_deref()
            .and_then(non_empty_string)
            .or_else(|| existing.map(|credential| credential.password.clone()))
            .or_else(|| non_empty_string(&state.defaults.rtsp_password))
            .unwrap_or_default();
        let rtsp_port = body
            .rtsp_port
            .filter(|port| *port > 0)
            .or_else(|| existing.and_then(|credential| credential.rtsp_port))
            .or_else(|| rtsp_port_from_url(&device.primary_stream.url))
            .or(Some(state.defaults.rtsp_port));
        let rtsp_paths = if body.rtsp_paths.is_empty() {
            existing
                .map(|credential| credential.rtsp_paths.clone())
                .filter(|paths| !paths.is_empty())
                .or_else(|| rtsp_path_from_url(&device.primary_stream.url).map(|path| vec![path]))
                .unwrap_or_else(|| state.defaults.rtsp_paths.clone())
        } else {
            body.rtsp_paths
        };

        let credential = DeviceCredentialSecret {
            device_id: device_id.clone(),
            username,
            password,
            rtsp_port,
            rtsp_paths: dedupe_rtsp_paths(rtsp_paths),
            updated_at: Some(now_unix_string()),
            last_verified_at: existing.and_then(|credential| credential.last_verified_at.clone()),
        };
        match self.admin_store.save_device_credential(credential) {
            Ok(state) => ok_json(&build_device_credential_status(&state, &device)),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_device_validation_run(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_device_validation_run_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid device validation run path"),
        };
        let principal =
            match self.authorize_camera_action(hints, &device_id, AccessAction::CameraOperate) {
                Ok(principal) => principal,
                Err(error) => return error_json(StatusCode(403), &error),
            };
        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };

        match self.run_device_validation(&principal, &device) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_rtsp_check(
        &self,
        path: &str,
        request: &mut Request,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_device_rtsp_check_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid device rtsp-check path"),
        };
        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraOperate)
        {
            return error_json(StatusCode(403), &error);
        }
        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };
        let body: RtspCheckRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        match self.check_device_rtsp(&device, body) {
            Ok(payload) => {
                let evidence = build_rtsp_check_evidence(&device, &payload, None);
                if let Err(error) = self.admin_store.record_device_evidence(evidence) {
                    return error_json(StatusCode(500), &error);
                }
                ok_json(&payload)
            }
            Err(error) => {
                let evidence =
                    build_rtsp_check_error_evidence(&device, &error, &now_unix_string(), None);
                let _ = self.admin_store.record_device_evidence(evidence);
                error_json(StatusCode(422), &redact_stream_url_credentials(&error))
            }
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
            task_response: redact_camera_task_response(self.analyze_camera(&principal, &device_id)),
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
            task_response: redact_camera_task_response(
                self.snapshot_camera(&principal, &device_id),
            ),
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

        let task_response = self.share_camera_link(&principal, &device_id);
        self.record_share_link_response_evidence(&device_id, &task_response);
        ok_json(&CameraTaskResponse {
            task_response: redact_camera_task_response(task_response),
        })
    }

    fn handle_dvr_recording_start(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_camera_recording_start_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid camera recording start path"),
        };
        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraOperate)
        {
            return error_json(StatusCode(403), &error);
        }
        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };
        let mut settings = match self.admin_store.dvr_recording_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        if !settings
            .enabled_device_ids
            .iter()
            .any(|enabled| enabled == &device_id)
        {
            settings.enabled_device_ids.push(device_id.clone());
        }
        let settings = match self.admin_store.save_dvr_recording_settings(settings) {
            Ok(state) => state.dvr,
            Err(error) => return error_json(StatusCode(422), &error),
        };
        match self
            .dvr_runtime
            .start_recording(&device, &settings, Some(&self.public_origin))
        {
            Ok(status) => ok_json(&status),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_dvr_recording_stop(
        &self,
        path: &str,
        hints: &AccessIdentityHints,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_camera_recording_stop_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid camera recording stop path"),
        };
        if let Err(error) =
            self.authorize_camera_action(hints, &device_id, AccessAction::CameraOperate)
        {
            return error_json(StatusCode(403), &error);
        }
        if let Err(error) = self.load_camera_device(&device_id) {
            return if error.contains("device not found") {
                error_json(StatusCode(404), &error)
            } else {
                error_json(StatusCode(422), &error)
            };
        }
        let mut settings = match self.admin_store.dvr_recording_settings() {
            Ok(settings) => settings,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        settings
            .enabled_device_ids
            .retain(|enabled| enabled != &device_id);
        if let Err(error) = self.admin_store.save_dvr_recording_settings(settings) {
            return error_json(StatusCode(422), &error);
        }
        match self
            .dvr_runtime
            .stop_recording(&device_id, Some(&self.public_origin))
        {
            Ok(status) => ok_json(&status),
            Err(error) => error_json(StatusCode(500), &error),
        }
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
                Ok(Some(media_session)) => media_session,
                Ok(None) => return error_json(StatusCode(404), "media session not found"),
                Err(error) => return error_json(StatusCode(500), &error),
            };
        self.record_share_link_revoke_evidence(
            &media_session.device_id,
            &revoked.share_link_id,
            &media_session.media_session_id,
            media_session
                .ended_at
                .as_deref()
                .unwrap_or_else(|| revoked.revoked_at.as_deref().unwrap_or("")),
        );

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

    fn patch_device_metadata(
        &self,
        device_id: &str,
        request: DeviceMetadataPatchRequest,
    ) -> Result<StateResponse, String> {
        let registry_store = self.admin_store.registry_store();
        let mut devices = registry_store.load_devices()?;
        let Some(device) = devices
            .iter_mut()
            .find(|device| device.device_id == device_id)
        else {
            return Err(format!("device not found: {device_id}"));
        };

        if let Some(name) = request.name.as_deref().and_then(non_empty_string) {
            device.name = name;
        }
        if let Some(room) = request.room {
            device.room = non_empty_string(&room);
        }
        if let Some(vendor) = request.vendor {
            device.vendor = non_empty_string(&vendor);
        }
        if let Some(model) = request.model {
            device.model = non_empty_string(&model);
        }
        if let Some(ip_address) = request.ip_address {
            device.ip_address = non_empty_string(&ip_address);
        }
        if let Some(snapshot_url) = request.snapshot_url {
            device.snapshot_url = non_empty_string(&snapshot_url);
            if device.snapshot_url.is_some() {
                device.capabilities.snapshot = true;
            }
        }
        if let Some(requires_auth) = request.requires_auth {
            device.primary_stream.requires_auth = requires_auth;
        }
        if let Some(primary_stream_url) = request
            .primary_stream_url
            .as_deref()
            .and_then(non_empty_string)
        {
            device.primary_stream.url = primary_stream_url;
            device.capabilities.stream = true;
        } else if request.rtsp_path.is_some() || request.rtsp_port.is_some() {
            device.primary_stream.url =
                build_rtsp_url_from_patch(device, request.rtsp_path.as_deref(), request.rtsp_port)?;
            device.capabilities.stream = true;
        }

        registry_store.save_devices(&devices)?;
        self.current_state()
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

    fn check_device_rtsp(
        &self,
        device: &CameraDevice,
        request: RtspCheckRequest,
    ) -> Result<RtspCheckResponse, String> {
        let state = self.admin_store.load_or_create_state()?;
        let credential = state
            .device_credentials
            .iter()
            .find(|credential| credential.device_id == device.device_id);
        let ip_address = device
            .ip_address
            .clone()
            .or_else(|| rtsp_host_from_url(&device.primary_stream.url))
            .ok_or_else(|| format!("device {} does not expose an RTSP host", device.device_id))?;
        let rtsp_port = request
            .rtsp_port
            .filter(|port| *port > 0)
            .or_else(|| credential.and_then(|credential| credential.rtsp_port))
            .or_else(|| rtsp_port_from_url(&device.primary_stream.url))
            .unwrap_or(state.defaults.rtsp_port);
        let username = request
            .username
            .as_deref()
            .and_then(non_empty_string)
            .or_else(|| credential.and_then(|credential| non_empty_string(&credential.username)))
            .or_else(|| non_empty_string(&state.defaults.rtsp_username));
        let password = request
            .password
            .as_deref()
            .and_then(non_empty_string)
            .or_else(|| credential.and_then(|credential| non_empty_string(&credential.password)))
            .or_else(|| non_empty_string(&state.defaults.rtsp_password));
        let path_candidates = if request.rtsp_paths.is_empty() {
            credential
                .map(|credential| credential.rtsp_paths.clone())
                .filter(|paths| !paths.is_empty())
                .or_else(|| rtsp_path_from_url(&device.primary_stream.url).map(|path| vec![path]))
                .unwrap_or_else(|| state.defaults.rtsp_paths.clone())
        } else {
            request.rtsp_paths
        };

        let adapter = CommandRtspAdapter::default();
        let checked_at = now_unix_string();
        let result = adapter.probe(&RtspProbeRequest {
            candidate_id: format!("rtsp-check-{}", device.device_id),
            ip_address,
            port: rtsp_port,
            username,
            password,
            path_candidates: dedupe_rtsp_paths(path_candidates),
        })?;
        if result.reachable {
            let _ = self
                .admin_store
                .mark_device_credential_verified(&device.device_id, checked_at.clone());
        }

        Ok(RtspCheckResponse {
            device_id: device.device_id.clone(),
            reachable: result.reachable,
            stream_url: result
                .stream_url
                .as_deref()
                .map(redact_stream_url_credentials),
            transport: format!("{:?}", result.transport).to_lowercase(),
            requires_auth: result.requires_auth,
            capabilities: result.capabilities,
            error_message: result.error_message,
            checked_at,
        })
    }

    fn run_device_validation(
        &self,
        principal: &AccessPrincipal,
        device: &CameraDevice,
    ) -> Result<DeviceValidationRunResponse, String> {
        let validation_id = format!(
            "device-validation-{}-{}",
            sanitize_id_fragment(&device.device_id),
            Uuid::new_v4().simple()
        );
        let rtsp_check = match self.check_device_rtsp(device, RtspCheckRequest::default()) {
            Ok(payload) => build_rtsp_check_evidence(device, &payload, Some(&validation_id)),
            Err(error) => build_rtsp_check_error_evidence(
                device,
                &error,
                &now_unix_string(),
                Some(&validation_id),
            ),
        };
        self.admin_store
            .record_device_evidence(rtsp_check.clone())?;

        let snapshot_check = if device_has_snapshot_path(device) {
            let response = self.snapshot_camera(principal, &device.device_id);
            build_snapshot_check_evidence(device, &response, Some(&validation_id))
        } else {
            build_snapshot_skipped_evidence(
                device,
                "device has no stream or snapshot endpoint to validate",
                Some(&validation_id),
            )
        };
        self.admin_store
            .record_device_evidence(snapshot_check.clone())?;

        let evidence = self.build_device_evidence_response(device)?;
        let status = validation_status(&rtsp_check, &snapshot_check);

        Ok(DeviceValidationRunResponse {
            validation_id,
            device_id: device.device_id.clone(),
            status,
            rtsp_check: evidence
                .recent_rtsp_check
                .clone()
                .unwrap_or_else(|| rtsp_check.clone()),
            snapshot_check: evidence
                .recent_snapshot_check
                .clone()
                .unwrap_or_else(|| snapshot_check.clone()),
            evidence,
        })
    }

    fn build_device_evidence_response(
        &self,
        device: &CameraDevice,
    ) -> Result<DeviceEvidenceResponse, String> {
        let state = self.admin_store.load_or_create_state()?;
        let credential_status = build_device_credential_status(&state, device);
        let share_links = self.list_share_links(Some(&device.device_id))?;
        let mut evidence = self.admin_store.list_device_evidence(&device.device_id)?;
        if let Some(snapshot_evidence) = self.latest_snapshot_asset_evidence(device)? {
            evidence.push(snapshot_evidence);
        }
        evidence.extend(
            share_links
                .iter()
                .map(build_share_link_evidence)
                .collect::<Vec<_>>(),
        );
        evidence = redact_device_evidence_records(evidence);
        evidence.sort_by(|left, right| {
            right
                .observed_at
                .cmp(&left.observed_at)
                .then(right.evidence_id.cmp(&left.evidence_id))
        });
        let recent_rtsp_check = evidence
            .iter()
            .find(|record| record.evidence_kind == "rtsp_check")
            .cloned();
        let recent_snapshot_check = evidence
            .iter()
            .find(|record| record.evidence_kind == "snapshot_check")
            .cloned();
        evidence.truncate(50);

        Ok(DeviceEvidenceResponse {
            device_id: device.device_id.clone(),
            generated_at: now_unix_string(),
            credential_status,
            recent_rtsp_check,
            recent_snapshot_check,
            share_links,
            evidence,
        })
    }

    fn latest_snapshot_asset_evidence(
        &self,
        device: &CameraDevice,
    ) -> Result<Option<DeviceEvidenceRecord>, String> {
        let media_assets = self.task_service.conversation_store().list_media_assets()?;
        Ok(media_assets
            .into_iter()
            .filter(|asset| {
                asset.device_id.as_deref() == Some(device.device_id.as_str())
                    && matches!(asset.asset_kind, MediaAssetKind::Snapshot)
            })
            .max_by(|left, right| {
                left.captured_at
                    .cmp(&right.captured_at)
                    .then(left.asset_id.cmp(&right.asset_id))
            })
            .map(|asset| build_snapshot_asset_evidence(device, &asset)))
    }

    fn record_share_link_response_evidence(&self, device_id: &str, response: &TaskResponse) {
        let observed_at = now_unix_string();
        let status = if matches!(response.status, TaskStatus::Completed) {
            "ready"
        } else {
            "blocked"
        };
        let share_link_id = response
            .result
            .data
            .pointer("/share_link/share_link_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let evidence = DeviceEvidenceRecord {
            evidence_id: format!(
                "share-link-create-{}-{}",
                sanitize_id_fragment(device_id),
                observed_at
            ),
            device_id: device_id.to_string(),
            evidence_kind: "share_link_create".to_string(),
            status: status.to_string(),
            observed_at,
            summary: format!("Share link create status={status} share_link_id={share_link_id}"),
            details: json!({
                "task_id": response.task_id,
                "status": format!("{:?}", response.status).to_lowercase(),
                "share_link_id": share_link_id,
                "artifact_count": response.result.artifacts.len(),
            }),
        };
        let _ = self.admin_store.record_device_evidence(evidence);
    }

    fn record_share_link_revoke_evidence(
        &self,
        device_id: &str,
        share_link_id: &str,
        media_session_id: &str,
        observed_at: &str,
    ) {
        let evidence = DeviceEvidenceRecord {
            evidence_id: format!(
                "share-link-revoke-{}-{}",
                sanitize_id_fragment(share_link_id),
                observed_at
            ),
            device_id: device_id.to_string(),
            evidence_kind: "share_link_revoke".to_string(),
            status: "ready".to_string(),
            observed_at: observed_at.to_string(),
            summary: format!("Share link revoked: {share_link_id}"),
            details: json!({
                "share_link_id": share_link_id,
                "media_session_id": media_session_id,
                "revoked": true,
            }),
        };
        let _ = self.admin_store.record_device_evidence(evidence);
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
    std::env::set_var(ADMIN_STATE_PATH_ENV, &admin_state_path);
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

fn normalize_unified_admin_url(raw_url: &str) -> String {
    let (path, query) = raw_url
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((raw_url, None));
    let normalized_path = normalize_unified_admin_path(path);
    match query {
        Some(query) => format!("{normalized_path}?{query}"),
        None => normalized_path,
    }
}

fn normalize_unified_admin_path(path: &str) -> String {
    if let Some(tail) = path.strip_prefix("/api/harbordesk") {
        let tail = tail.trim_start_matches('/');
        if tail.is_empty() {
            return "/api/state".to_string();
        }
        return format!("/api/{tail}");
    }
    let Some(tail) = path.strip_prefix("/api/admin") else {
        return path.to_string();
    };
    let tail = tail.trim_start_matches('/');
    if tail.is_empty() {
        return "/api/state".to_string();
    }
    if tail == "notification-targets" || tail.starts_with("notification-targets/") {
        return path.to_string();
    }
    format!("/api/{tail}")
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
    let trimmed = path
        .strip_prefix("/api/admin/notification-targets/")?
        .trim();
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

fn parse_model_download_job_path(path: &str) -> Option<String> {
    let job_id = path.strip_prefix("/api/models/local-downloads/")?.trim();
    if job_id.is_empty() || job_id.contains('/') {
        None
    } else {
        percent_decode_path_segment(job_id).ok()
    }
}

fn parse_model_download_cancel_path(path: &str) -> Option<String> {
    let job_id = path
        .strip_prefix("/api/models/local-downloads/")?
        .strip_suffix("/cancel")?
        .trim();
    if job_id.is_empty() || job_id.contains('/') {
        None
    } else {
        percent_decode_path_segment(job_id).ok()
    }
}

fn parse_knowledge_index_job_cancel_path(path: &str) -> Option<String> {
    let job_id = path
        .strip_prefix("/api/knowledge/index/jobs/")?
        .strip_suffix("/cancel")?
        .trim();
    if job_id.is_empty() || job_id.contains('/') {
        None
    } else {
        percent_decode_path_segment(job_id).ok()
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

fn deprecated_im_binding_response_json(manage_url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(
        StatusCode(410),
        &json!({
            "error": deprecated_im_binding_message(),
            "manage_url": manage_url,
        }),
    )
}

fn deprecated_im_binding_response_html(manage_url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
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
    let expected =
        env_var_with_legacy_alias("HARBORGATE_BEARER_TOKEN", "HARBOR_IM_GATEWAY_BEARER_TOKEN")
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
        || path == "/api/release/readiness"
        || path == "/api/release/readiness/history"
        || path == "/api/release/readiness/run"
        || path == "/api/hardware/readiness"
        || path == "/api/rag/readiness"
        || path == "/api/knowledge/settings"
        || path == "/api/knowledge/search"
        || path == "/api/knowledge/preview"
        || path == "/api/knowledge/index/run"
        || path == "/api/knowledge/index/status"
        || path == "/api/knowledge/index/jobs"
        || (path.starts_with("/api/knowledge/index/jobs/") && path.ends_with("/cancel"))
        || path == "/api/files/browse"
        || path == "/api/cameras/recording-settings"
        || path == "/api/cameras/recordings/status"
        || path == "/api/cameras/recordings/timeline"
        || path == "/api/harboros/status"
        || path == "/api/harboros/im-capability-map"
        || path == "/api/models/endpoints"
        || path == "/api/models/local-catalog"
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
        || path == "/api/devices/default-camera"
        || (path.starts_with("/api/devices/") && !path.contains("/../"))
        || (path.starts_with("/api/devices/") && path.ends_with("/credentials"))
        || (path.starts_with("/api/devices/") && path.ends_with("/credential-status"))
        || (path.starts_with("/api/devices/") && path.ends_with("/rtsp-check"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/share-link"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/recordings/start"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/recordings/stop"))
        || (path.starts_with("/api/share-links/") && path.ends_with("/revoke"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/snapshot"))
        || (path.starts_with("/api/cameras/") && path.ends_with("/analyze"))
        || path == "/api/defaults"
        || path.starts_with("/api/models/endpoints/")
        || path == "/api/models/local-downloads"
        || path.starts_with("/api/models/local-downloads/")
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
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("avi") => "video/x-msvideo",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        Some("md") | Some("markdown") => "text/markdown; charset=utf-8",
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

fn build_release_readiness_response(
    public_origin: &str,
    state: Option<&StateResponse>,
    account: &AccountManagementSnapshot,
    feature_availability: &FeatureAvailabilityResponse,
    hardware: &HardwareReadinessResponse,
    harboros: &HarborOsStatusResponse,
    rag: &RagReadinessResponse,
    runtime: &LocalModelRuntimeProjection,
) -> ReleaseReadinessResponse {
    let mut groups = Vec::new();

    let interactive = find_feature_item(feature_availability, "interactive_reply");
    let proactive = find_feature_item(feature_availability, "proactive_delivery");
    let binding = find_feature_item(feature_availability, "binding_availability");
    groups.push(release_group(
        "im",
        "IM Gateway",
        "harbor-im-gateway",
        vec![
            release_item_from_feature(
                "weixin-setup",
                "Weixin setup",
                "harbor-im-gateway",
                proactive,
                "/im-gateway",
                vec![format!(
                    "manage_url={}",
                    to_non_empty_option(&account.gateway.manage_url)
                )],
            ),
            release_item_from_feature(
                "feishu-setup",
                "Feishu API key setup",
                "harbor-im-gateway",
                interactive,
                "/im-gateway",
                vec![format!(
                    "gateway_configured={}",
                    yes_no(account.gateway.bridge_provider.configured)
                )],
            ),
            release_item_from_feature(
                "binding-availability",
                "Binding availability",
                "harbor-im-gateway",
                binding,
                "/account-management",
                Vec::new(),
            ),
        ],
    ));

    let answer = find_feature_item(feature_availability, "retrieval.answer");
    let embed = find_feature_item(feature_availability, "retrieval.embed");
    let vision = find_feature_item(feature_availability, "retrieval.vision_summary");
    groups.push(release_group(
        "models",
        "Models & Policies",
        "harbor-framework",
        vec![
            release_item_from_feature(
                "model-answer",
                "LLM answer endpoint",
                "harbor-framework",
                answer,
                "/models-policies",
                vec![format!("runtime_ready={}", yes_no(runtime.ready))],
            ),
            release_item_from_feature(
                "model-embedding",
                "Embedding endpoint",
                "harbor-framework",
                embed,
                "/models-policies",
                vec![format!("backend_ready={}", yes_no(runtime.backend_ready))],
            ),
            release_item_from_feature(
                "model-vision",
                "Vision summary endpoint",
                "harbor-framework",
                vision,
                "/models-policies",
                Vec::new(),
            ),
        ],
    ));

    groups.push(release_group(
        "rag",
        "HarborOS Multimodal RAG",
        "harbor-framework",
        vec![release_item(
            "rag-readiness",
            "Multimodal RAG readiness",
            "harbor-framework",
            release_status_from_probe_status(&rag.status),
            &format!(
                "Index: {}; embedding: {}",
                rag.index_directory.status, rag.embedding_model.status
            ),
            "RAG readiness checks index directory, embedding model, media parser, and writable storage.",
            "GET /api/rag/readiness",
            "/models-policies",
            rag.evidence.clone(),
        )],
    ));

    groups.push(release_group(
        "hardware",
        "Hardware Readiness",
        "harbor-framework",
        vec![release_item(
            "hardware-profile",
            "CPU / GPU / NPU readiness",
            "harbor-framework",
            release_status_from_probe_status(&hardware.status),
            &hardware.recommended_model_profile,
            "Hardware probe for local inference placement.",
            "GET /api/hardware/readiness",
            "/models-policies",
            hardware.evidence.clone(),
        )],
    ));

    groups.push(release_group(
        "harboros",
        "HarborOS System Domain",
        "harbor-hos-control",
        vec![release_item(
            "harboros-status",
            "System Domain status",
            "harbor-hos-control",
            release_status_from_probe_status(&harboros.status),
            &format!("HarborOS WebUI: {}", harboros.webui_url),
            "HarborOS stays System Domain only; AIoT is not managed here.",
            "GET /api/harboros/status",
            "/harboros",
            harboros.evidence.clone(),
        )],
    ));

    let devices = state.map(|state| state.devices.as_slice()).unwrap_or(&[]);
    let selected_camera = state
        .and_then(|state| state.defaults.selected_camera_device_id.clone())
        .unwrap_or_default();
    let selected_device = devices
        .iter()
        .find(|device| device.device_id == selected_camera);
    let rtsp_ready = selected_device
        .map(|device| !device.primary_stream.url.trim().is_empty())
        .unwrap_or(false);
    let snapshot_ready = selected_device
        .map(|device| {
            device
                .snapshot_url
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        })
        .unwrap_or(false);
    let default_camera_summary = if selected_camera.is_empty() {
        "No default camera selected.".to_string()
    } else {
        format!("Default camera: {selected_camera}")
    };
    groups.push(release_group(
        "aiot",
        "Devices & AIoT",
        "harbor-aiot",
        vec![
            release_item(
                "aiot-default-camera",
                "Default camera",
                "harbor-aiot",
                if selected_device.is_some() {
                    "ready"
                } else if devices.is_empty() {
                    "blocked"
                } else {
                    "needs-config"
                },
                &default_camera_summary,
                "Default camera is configured in HarborDesk Devices & AIoT.",
                "GET /api/state",
                "/devices-aiot",
                vec![format!("registered_devices={}", devices.len())],
            ),
            release_item(
                "aiot-rtsp-snapshot",
                "RTSP / snapshot readiness",
                "harbor-aiot",
                if rtsp_ready && snapshot_ready {
                    "ready"
                } else if rtsp_ready || snapshot_ready {
                    "needs-config"
                } else {
                    "blocked"
                },
                "Camera media capabilities are projected from the Home Device Domain registry.",
                "RTSP/snapshot checks are explicit actions and should only run after target confirmation.",
                "POST /api/devices/{device_id}/rtsp-check",
                "/devices-aiot",
                vec![
                    format!("rtsp_ready={}", yes_no(rtsp_ready)),
                    format!("snapshot_ready={}", yes_no(snapshot_ready)),
                ],
            ),
        ],
    ));

    let overall_status = rollup_group_status(groups.iter().map(|group| group.status.as_str()));
    let generated_at = now_unix_string();
    let checklist = groups
        .iter()
        .flat_map(|group| group.items.clone())
        .collect::<Vec<_>>();
    let status_cards = groups
        .iter()
        .map(|group| ReleaseReadinessStatusCard {
            id: format!("{}-status", group.group_id),
            label: group.label.clone(),
            value: group.status.clone(),
            status: group.status.clone(),
            detail: format!("{} readiness owned by {}.", group.label, group.owner_lane),
            endpoint: group
                .items
                .first()
                .map(|item| item.endpoint.clone())
                .unwrap_or_else(|| "GET /api/release/readiness".to_string()),
            deep_link: group
                .items
                .first()
                .map(|item| item.deep_link.clone())
                .unwrap_or_else(|| "/overview".to_string()),
        })
        .collect::<Vec<_>>();
    let deep_links = release_readiness_deep_links(public_origin, harboros);
    let blockers = checklist
        .iter()
        .filter(|item| item.status == "blocked")
        .filter_map(|item| {
            non_empty_string(&item.blocking_reason).or_else(|| Some(item.label.clone()))
        })
        .collect::<Vec<_>>();
    let warnings = checklist
        .iter()
        .filter(|item| item.status == "needs-config")
        .filter_map(|item| {
            non_empty_string(&item.blocking_reason).or_else(|| Some(item.label.clone()))
        })
        .collect::<Vec<_>>();
    ReleaseReadinessResponse {
        generated_at: generated_at.clone(),
        checked_at: generated_at,
        status: overall_status.clone(),
        summary: format!(
            "Release readiness is {overall_status}; HarborDesk stays on :4174 and HarborOS WebUI stays on /ui/ or 80/443."
        ),
        overall_status,
        harbor_desk: ReadinessSurfaceSummary {
            admin_origin: public_origin.to_string(),
            admin_port: public_origin_port(public_origin).unwrap_or(4174),
            harboros_webui: harboros_webui_url(public_origin),
            note: "HarborDesk uses port 4174; HarborOS WebUI stays on /ui/ or 80/443.".to_string(),
        },
        groups,
        checklist,
        status_cards,
        deep_links,
        blockers,
        warnings,
    }
}

fn release_readiness_deep_links(
    public_origin: &str,
    harboros: &HarborOsStatusResponse,
) -> Vec<ReleaseReadinessDeepLink> {
    let origin = public_origin.trim_end_matches('/');
    vec![
        ReleaseReadinessDeepLink {
            label: "HarborDesk Overview (:4174)".to_string(),
            href: format!("{origin}/overview"),
            detail: "Release readiness entry in HarborDesk.".to_string(),
            endpoint: "GET /api/release/readiness".to_string(),
        },
        ReleaseReadinessDeepLink {
            label: "Devices & AIoT".to_string(),
            href: format!("{origin}/devices-aiot"),
            detail: "Camera, RTSP, snapshot, share-link, and credential management.".to_string(),
            endpoint: "GET /api/devices/{device_id}/evidence".to_string(),
        },
        ReleaseReadinessDeepLink {
            label: "Models & Policies".to_string(),
            href: format!("{origin}/models-policies"),
            detail: "Model endpoints, local catalog, downloads, and RAG readiness.".to_string(),
            endpoint: "GET /api/models/local-catalog + GET /api/rag/readiness".to_string(),
        },
        ReleaseReadinessDeepLink {
            label: "HarborOS System Domain".to_string(),
            href: format!("{origin}/harboros"),
            detail: "System Domain status inside HarborDesk, not AIoT device control.".to_string(),
            endpoint: "GET /api/harboros/status".to_string(),
        },
        ReleaseReadinessDeepLink {
            label: "HarborOS WebUI (/ui/)".to_string(),
            href: harboros.webui_url.clone(),
            detail: "HarborOS WebUI stays on /ui/ or ports 80/443, separate from HarborDesk :4174."
                .to_string(),
            endpoint: "HarborOS WebUI /ui/ or 80/443".to_string(),
        },
    ]
}

fn release_group(
    group_id: &str,
    label: &str,
    owner_lane: &str,
    items: Vec<ReleaseReadinessItem>,
) -> ReleaseReadinessGroup {
    let status = rollup_group_status(items.iter().map(|item| item.status.as_str()));
    ReleaseReadinessGroup {
        group_id: group_id.to_string(),
        label: label.to_string(),
        owner_lane: owner_lane.to_string(),
        status,
        items,
    }
}

fn release_item_from_feature(
    item_id: &str,
    label: &str,
    owner_lane: &str,
    feature: Option<&FeatureAvailabilityItem>,
    action_path: &str,
    mut extra_evidence: Vec<String>,
) -> ReleaseReadinessItem {
    if let Some(feature) = feature {
        extra_evidence.extend(feature.evidence.clone());
        return release_item(
            item_id,
            label,
            owner_lane,
            release_status_from_feature_status(&feature.status),
            &feature.current_option,
            &feature.blocker,
            &feature.source_of_truth,
            action_path,
            extra_evidence,
        );
    }
    release_item(
        item_id,
        label,
        owner_lane,
        "blocked",
        "Feature projection missing.",
        "Readiness aggregator could not find the expected feature row.",
        "GET /api/feature-availability",
        action_path,
        extra_evidence,
    )
}

fn release_item(
    item_id: &str,
    label: &str,
    owner_lane: &str,
    status: &str,
    summary: &str,
    detail: &str,
    source_of_truth: &str,
    action_path: &str,
    evidence: Vec<String>,
) -> ReleaseReadinessItem {
    let summary = redact_admin_string(summary);
    let detail = redact_admin_string(detail);
    let source_of_truth = redact_admin_string(source_of_truth);
    let redacted_evidence = evidence
        .into_iter()
        .map(|item| redact_admin_string(&item))
        .collect::<Vec<_>>();
    let blocking_reason = if status == "ready" {
        String::new()
    } else {
        detail.clone()
    };
    ReleaseReadinessItem {
        id: item_id.to_string(),
        item_id: item_id.to_string(),
        label: label.to_string(),
        lane: owner_lane.to_string(),
        owner_lane: owner_lane.to_string(),
        status: status.to_string(),
        summary,
        detail,
        endpoint: source_of_truth.clone(),
        source_of_truth,
        deep_link: action_path.to_string(),
        next_action: action_path.to_string(),
        action_path: action_path.to_string(),
        last_verified_at: Some(now_unix_string()),
        blocking_reason: blocking_reason.clone(),
        blockers: if blocking_reason.is_empty() {
            Vec::new()
        } else {
            vec![blocking_reason.clone()]
        },
        evidence: redacted_evidence.clone(),
        evidence_records: vec![ReadinessEvidenceRecord {
            generated_at: now_unix_string(),
            lane: owner_lane.to_string(),
            status: status.to_string(),
            action_path: action_path.to_string(),
            blocking_reason,
            evidence: redacted_evidence,
        }],
    }
}

fn rollup_group_status<'a>(statuses: impl Iterator<Item = &'a str>) -> String {
    let mut best = "ready";
    for status in statuses {
        if readiness_status_rank(status) > readiness_status_rank(best) {
            best = status;
        }
    }
    best.to_string()
}

fn readiness_status_rank(status: &str) -> u8 {
    match status {
        "blocked" => 3,
        "needs-config" => 2,
        "ready" => 1,
        _ => 2,
    }
}

fn release_status_from_feature_status(status: &str) -> &'static str {
    match status {
        "available" => "ready",
        "degraded" | "not_configured" => "needs-config",
        "blocked" => "blocked",
        _ => "needs-config",
    }
}

fn release_status_from_probe_status(status: &str) -> &'static str {
    match status {
        "ready" | "available" => "ready",
        "blocked" => "blocked",
        _ => "needs-config",
    }
}

fn find_feature_item<'a>(
    response: &'a FeatureAvailabilityResponse,
    feature_id: &str,
) -> Option<&'a FeatureAvailabilityItem> {
    response
        .groups
        .iter()
        .flat_map(|group| group.items.iter())
        .find(|item| item.feature_id == feature_id)
}

fn build_hardware_readiness_response() -> HardwareReadinessResponse {
    let generated_at = now_unix_string();
    let cpu_count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let memory_mb = proc_mem_total_mb();
    let gpu_evidence = gpu_probe_evidence();
    let npu_evidence = npu_probe_evidence();

    let cpu = HardwareComponentReadiness {
        status: if cpu_count >= 2 {
            "ready"
        } else {
            "needs-config"
        }
        .to_string(),
        summary: format!("{cpu_count} logical CPU threads"),
        detail: format!("{} / {}", env::consts::OS, env::consts::ARCH),
        evidence: vec![format!("available_parallelism={cpu_count}")],
    };
    let memory = HardwareComponentReadiness {
        status: memory_mb
            .map(|mb| if mb >= 8192 { "ready" } else { "needs-config" })
            .unwrap_or("needs-config")
            .to_string(),
        summary: memory_mb
            .map(|mb| format!("{mb} MiB memory detected"))
            .unwrap_or_else(|| "memory total not detected".to_string()),
        detail: "Memory is read from /proc/meminfo when available.".to_string(),
        evidence: memory_mb
            .map(|mb| vec![format!("mem_total_mb={mb}")])
            .unwrap_or_else(|| vec!["mem_total_mb=unknown".to_string()]),
    };
    let gpu_ready = gpu_evidence
        .iter()
        .any(|item| item.contains("present=true"));
    let gpu = HardwareComponentReadiness {
        status: if gpu_ready { "ready" } else { "needs-config" }.to_string(),
        summary: if gpu_ready {
            "GPU runtime detected".to_string()
        } else {
            "No GPU runtime detected".to_string()
        },
        detail: "Checks nvidia-smi, /dev/nvidia0, /dev/dri, and CUDA visibility.".to_string(),
        evidence: gpu_evidence,
    };
    let npu_ready = npu_evidence
        .iter()
        .any(|item| item.contains("present=true"));
    let npu = HardwareComponentReadiness {
        status: if npu_ready { "ready" } else { "needs-config" }.to_string(),
        summary: if npu_ready {
            "NPU/accelerator device detected".to_string()
        } else {
            "No NPU runtime detected".to_string()
        },
        detail: "Checks common Linux accelerator device nodes.".to_string(),
        evidence: npu_evidence,
    };
    let recommended_model_profile = if gpu_ready {
        "local-vlm-plus-llm".to_string()
    } else if memory_mb.unwrap_or_default() >= 16384 && cpu_count >= 4 {
        "cpu-small-llm-and-embedding".to_string()
    } else {
        "cloud-or-tiny-local-models".to_string()
    };
    let mut blockers = Vec::new();
    if cpu_count < 2 {
        blockers.push("CPU parallelism is below the release recommendation.".to_string());
    }
    if memory_mb.unwrap_or_default() > 0 && memory_mb.unwrap_or_default() < 4096 {
        blockers.push("Memory is below the minimal local model recommendation.".to_string());
    }
    let status = if blockers.is_empty() {
        "ready"
    } else {
        "needs-config"
    }
    .to_string();
    let mut evidence = Vec::new();
    evidence.extend(cpu.evidence.clone());
    evidence.extend(memory.evidence.clone());
    evidence.extend(gpu.evidence.clone());
    evidence.extend(npu.evidence.clone());
    HardwareReadinessResponse {
        generated_at,
        status,
        cpu,
        memory,
        gpu,
        npu,
        recommended_model_profile,
        blockers,
        evidence,
    }
}

fn build_knowledge_index_job(
    root: &KnowledgeSourceRoot,
    requested_at: &str,
    resource_profile: RagResourceProfile,
) -> KnowledgeIndexJobRecord {
    KnowledgeIndexJobRecord {
        job_id: format!("knowledge-index-{}", Uuid::new_v4().simple()),
        source_root_id: root.root_id.clone(),
        source_root_label: root.label.clone(),
        source_root_path: root.path.clone(),
        modalities: vec![
            "document".to_string(),
            "image".to_string(),
            "audio".to_string(),
            "video".to_string(),
        ],
        status: "queued".to_string(),
        progress_percent: Some(0),
        requested_at: Some(requested_at.to_string()),
        started_at: None,
        completed_at: None,
        error_message: None,
        retry_count: 0,
        checkpoint: json!({
            "phase": "queued",
            "source_root_id": root.root_id.clone(),
        }),
        resource_profile,
        cancel_requested: false,
    }
}

fn queued_knowledge_root_status(root: &KnowledgeSourceRoot) -> KnowledgeIndexRootStatus {
    let mut status = knowledge_root_status(root, None);
    status.status = "queued".to_string();
    status.detail = "Index refresh has been queued as a background job.".to_string();
    status
}

fn spawn_knowledge_index_worker(
    store: AdminConsoleStore,
    settings: KnowledgeSettings,
    jobs: Vec<KnowledgeIndexJobRecord>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("harborbeacon-knowledge-index".to_string())
        .spawn(move || run_knowledge_index_jobs(store, settings, jobs))
        .map(|_| ())
        .map_err(|error| format!("failed to spawn knowledge index worker: {error}"))
}

fn run_knowledge_index_jobs(
    store: AdminConsoleStore,
    settings: KnowledgeSettings,
    jobs: Vec<KnowledgeIndexJobRecord>,
) {
    let service = match KnowledgeIndexConfig::new(PathBuf::from(settings.index_root.clone()))
        .and_then(KnowledgeIndexService::from_config)
    {
        Ok(service) => service,
        Err(error) => {
            for job in jobs {
                fail_knowledge_index_job(&store, job, "index_root_unavailable", error.clone());
            }
            return;
        }
    };

    for job in jobs {
        run_knowledge_index_job(&store, &service, job);
    }
}

fn run_knowledge_index_job(
    store: &AdminConsoleStore,
    service: &KnowledgeIndexService,
    mut job: KnowledgeIndexJobRecord,
) {
    if knowledge_index_job_cancel_requested(store, &job.job_id) {
        cancel_knowledge_index_job(store, job, "canceled_before_start");
        return;
    }

    job.status = "running".to_string();
    job.started_at = job.started_at.or_else(|| Some(now_unix_string()));
    job.progress_percent = Some(10);
    job.checkpoint = json!({
        "phase": "load_or_refresh",
        "source_root_id": job.source_root_id.clone(),
    });
    if let Err(error) = store.save_knowledge_index_job(job.clone()) {
        fail_knowledge_index_job(store, job, "job_state_write_failed", error);
        return;
    }

    let root_path = PathBuf::from(job.source_root_path.trim());
    if !root_path.exists() {
        fail_knowledge_index_job(
            store,
            job,
            "source_root_missing",
            format!("knowledge source root not found: {}", root_path.display()),
        );
        return;
    }
    if knowledge_index_job_cancel_requested(store, &job.job_id) {
        cancel_knowledge_index_job(store, job, "canceled_before_refresh");
        return;
    }

    match service.load_or_refresh(&root_path) {
        Ok(snapshot) => {
            if knowledge_index_job_cancel_requested(store, &job.job_id) {
                cancel_knowledge_index_job(store, job, "canceled_after_refresh");
                return;
            }
            let indexed_at = snapshot.manifest.generated_at.clone();
            let _ = mark_knowledge_source_root_indexed(
                store,
                &job.source_root_id,
                &job.source_root_path,
                indexed_at,
            );
            job.status = "completed".to_string();
            job.progress_percent = Some(100);
            job.completed_at = Some(now_unix_string());
            job.error_message = None;
            job.checkpoint = json!({
                "phase": "completed",
                "entry_count": snapshot.manifest.entries.len(),
                "manifest_path": snapshot.manifest_path.to_string_lossy(),
            });
            let _ = store.save_knowledge_index_job(job);
        }
        Err(error) => fail_knowledge_index_job(store, job, "load_or_refresh_failed", error),
    }
}

fn fail_knowledge_index_job(
    store: &AdminConsoleStore,
    mut job: KnowledgeIndexJobRecord,
    phase: &str,
    error: String,
) {
    job.status = "failed".to_string();
    job.progress_percent = Some(100);
    job.completed_at = Some(now_unix_string());
    job.error_message = Some(error);
    job.checkpoint = json!({"phase": phase});
    let _ = store.save_knowledge_index_job(job);
}

fn cancel_knowledge_index_job(
    store: &AdminConsoleStore,
    mut job: KnowledgeIndexJobRecord,
    phase: &str,
) {
    job.status = "canceled".to_string();
    job.cancel_requested = true;
    job.progress_percent = job.progress_percent.or(Some(0));
    job.completed_at = Some(now_unix_string());
    job.checkpoint = json!({"phase": phase});
    let _ = store.save_knowledge_index_job(job);
}

fn knowledge_index_job_cancel_requested(store: &AdminConsoleStore, job_id: &str) -> bool {
    store
        .list_knowledge_index_jobs()
        .map(|jobs| {
            jobs.into_iter().any(|job| {
                job.job_id == job_id && (job.cancel_requested || job.status.as_str() == "canceled")
            })
        })
        .unwrap_or(false)
}

fn mark_knowledge_source_root_indexed(
    store: &AdminConsoleStore,
    source_root_id: &str,
    source_root_path: &str,
    indexed_at: String,
) -> Result<(), String> {
    let mut settings = store.knowledge_settings()?;
    let Some(root) = settings
        .source_roots
        .iter_mut()
        .find(|root| root.root_id == source_root_id && root.path.trim() == source_root_path.trim())
    else {
        return Ok(());
    };
    root.last_indexed_at = Some(indexed_at);
    store.save_knowledge_settings(settings).map(|_| ())
}

fn build_knowledge_index_status_response(
    settings: KnowledgeSettings,
) -> KnowledgeIndexStatusResponse {
    let index_path = Path::new(&settings.index_root);
    let index_root_exists = index_path.exists();
    let index_root_writable = path_can_accept_write(index_path);
    let storage_summary = knowledge_index_storage_summary(index_path);
    let source_roots = settings
        .source_roots
        .iter()
        .map(|root| knowledge_root_status(root, None))
        .collect::<Vec<_>>();
    let mut blockers = Vec::new();
    if settings.enabled_source_root_paths().is_empty() {
        blockers.push("No enabled knowledge source roots are configured.".to_string());
    }
    if !index_root_writable {
        blockers.push("Knowledge index root is not writable or its parent is missing.".to_string());
    }
    for root in &source_roots {
        if root.enabled && !root.exists {
            blockers.push(format!("Knowledge source root not found: {}", root.path));
        }
    }
    if let Err(error) = validate_knowledge_settings(settings.clone()) {
        blockers.push(error);
    }
    let status = if blockers.is_empty() {
        "ready"
    } else if index_root_writable || source_roots.iter().any(|root| root.enabled && root.exists) {
        "needs-config"
    } else {
        "blocked"
    }
    .to_string();
    KnowledgeIndexStatusResponse {
        generated_at: now_unix_string(),
        status,
        settings,
        index_root_exists,
        index_root_writable,
        manifest_count: storage_summary.manifest_count,
        manifest_entry_count: storage_summary.manifest_entry_count,
        document_count: storage_summary.document_count,
        image_count: storage_summary.image_count,
        audio_count: storage_summary.audio_count,
        video_count: storage_summary.video_count,
        content_indexed_image_count: storage_summary.content_indexed_image_count,
        vlm_indexed_image_count: storage_summary.vlm_indexed_image_count,
        ocr_indexed_image_count: storage_summary.ocr_indexed_image_count,
        image_content_missing_count: storage_summary.image_content_missing_count,
        image_text_source_counts: storage_summary.image_text_source_counts,
        embedding_cache_count: storage_summary.embedding_cache_count,
        embedding_entry_count: storage_summary.embedding_entry_count,
        storage_usage_bytes: storage_summary.storage_usage_bytes,
        last_indexed_at: storage_summary.last_indexed_at,
        source_roots,
        blockers,
    }
}

fn knowledge_index_storage_summary(index_path: &Path) -> KnowledgeIndexStorageSummary {
    let mut summary = KnowledgeIndexStorageSummary {
        storage_usage_bytes: directory_storage_bytes(index_path),
        ..KnowledgeIndexStorageSummary::default()
    };
    let Ok(entries) = fs::read_dir(index_path) else {
        return summary;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.ends_with(".embeddings.json") {
            summary.embedding_cache_count += 1;
            if let Ok(store) = load_embedding_store(&path) {
                summary.embedding_entry_count += store.entries.len();
            }
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<KnowledgeIndexManifest>(&text) else {
            continue;
        };
        summary.manifest_count += 1;
        summary.manifest_entry_count += manifest.entries.len();
        for indexed_entry in &manifest.entries {
            match indexed_entry.modality {
                KnowledgeModality::Document => summary.document_count += 1,
                KnowledgeModality::Image => {
                    summary.image_count += 1;
                    let mut has_vlm = false;
                    let mut has_ocr = false;
                    for source in &indexed_entry.text_sources {
                        let source_kind = source.source_kind.trim().to_ascii_lowercase();
                        if source_kind.is_empty() {
                            continue;
                        }
                        *summary
                            .image_text_source_counts
                            .entry(source_kind.clone())
                            .or_insert(0) += 1;
                        has_vlm |= source_kind == "vlm";
                        has_ocr |= source_kind == "ocr";
                    }
                    if has_vlm {
                        summary.vlm_indexed_image_count += 1;
                    }
                    if has_ocr {
                        summary.ocr_indexed_image_count += 1;
                    }
                    if has_vlm || has_ocr {
                        summary.content_indexed_image_count += 1;
                    } else {
                        summary.image_content_missing_count += 1;
                    }
                }
                KnowledgeModality::Audio => summary.audio_count += 1,
                KnowledgeModality::Video => summary.video_count += 1,
            }
        }
        summary.last_indexed_at =
            max_unix_timestamp_string(summary.last_indexed_at.take(), Some(manifest.generated_at));
    }
    summary
}

fn build_admin_knowledge_search_request(
    payload: KnowledgeSearchApiRequest,
    settings: &KnowledgeSettings,
) -> Result<KnowledgeSearchRequest, String> {
    let query = payload.query.trim();
    if query.is_empty() {
        return Err("HarborBot search requires a non-empty query.".to_string());
    }
    let configured_roots = settings.enabled_source_root_paths();
    if configured_roots.is_empty() {
        return Err("No enabled knowledge source roots are configured.".to_string());
    }
    let mut request = KnowledgeSearchRequest::new(query.to_string());
    request.configured_roots = configured_roots.clone();
    request.roots = configured_roots;
    request.index_root = non_empty_string(&settings.index_root);
    request.include_documents = payload.include_documents.unwrap_or(true);
    request.include_images = payload.include_images.unwrap_or(true);
    request.include_videos = payload.include_videos.unwrap_or(true);
    request.limit = payload.limit.unwrap_or(24).clamp(1, 50);
    request.privacy_level = settings.privacy_level;
    request.resource_profile = settings.default_resource_profile;
    request.require_embeddings = false;
    request.latency_budget_ms = None;
    Ok(request)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KnowledgePreviewError {
    status: StatusCode,
    message: String,
}

impl KnowledgePreviewError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

fn resolve_knowledge_preview_path(
    requested_path: &str,
    settings: &KnowledgeSettings,
) -> Result<PathBuf, KnowledgePreviewError> {
    let requested = PathBuf::from(requested_path.trim());
    if !requested.is_absolute() {
        return Err(KnowledgePreviewError::new(
            StatusCode(400),
            "knowledge preview path must be absolute",
        ));
    }
    let requested = requested.canonicalize().map_err(|_| {
        KnowledgePreviewError::new(
            StatusCode(404),
            format!("knowledge preview file not found: {}", requested.display()),
        )
    })?;
    if requested.is_dir() {
        return Err(KnowledgePreviewError::new(
            StatusCode(400),
            "knowledge preview does not support directory paths",
        ));
    }
    if !requested.is_file() {
        return Err(KnowledgePreviewError::new(
            StatusCode(404),
            format!("knowledge preview file not found: {}", requested.display()),
        ));
    }
    if !knowledge_preview_mime_supported(&requested) {
        return Err(KnowledgePreviewError::new(
            StatusCode(415),
            format!(
                "knowledge preview supports images, videos, text, and Markdown only: {}",
                requested.display()
            ),
        ));
    }
    let allowed_roots = enabled_existing_knowledge_roots(settings);
    if allowed_roots.is_empty() {
        return Err(KnowledgePreviewError::new(
            StatusCode(422),
            "No enabled knowledge source roots are configured.",
        ));
    }
    if !allowed_roots
        .iter()
        .any(|root| path_is_same_or_inside(&requested.to_string_lossy(), &root.to_string_lossy()))
    {
        return Err(KnowledgePreviewError::new(
            StatusCode(403),
            format!(
                "knowledge preview path is outside configured source roots: {}",
                requested.display()
            ),
        ));
    }
    if !knowledge_preview_path_is_indexed(&requested, settings, &allowed_roots) {
        return Err(KnowledgePreviewError::new(
            StatusCode(404),
            format!(
                "knowledge preview path is not present in the current index: {}",
                requested.display()
            ),
        ));
    }
    Ok(requested)
}

fn enabled_existing_knowledge_roots(settings: &KnowledgeSettings) -> Vec<PathBuf> {
    settings
        .source_roots
        .iter()
        .filter(|root| root.enabled)
        .filter_map(|root| non_empty_string(&root.path))
        .filter_map(|path| PathBuf::from(path).canonicalize().ok())
        .filter(|path| path.is_dir())
        .collect()
}

fn knowledge_preview_path_is_indexed(
    requested: &Path,
    settings: &KnowledgeSettings,
    allowed_roots: &[PathBuf],
) -> bool {
    let service = match KnowledgeIndexConfig::new(PathBuf::from(settings.index_root.clone()))
        .and_then(KnowledgeIndexService::from_config)
    {
        Ok(service) => service,
        Err(_) => return false,
    };
    let requested_string = requested.to_string_lossy();
    allowed_roots
        .iter()
        .filter(|root| path_is_same_or_inside(&requested_string, &root.to_string_lossy()))
        .filter_map(|root| service.load_existing(root).ok())
        .flat_map(|snapshot| snapshot.manifest.entries.into_iter())
        .any(|entry| indexed_entry_matches_path(&entry.path, requested))
}

fn indexed_entry_matches_path(indexed_path: &str, requested: &Path) -> bool {
    let indexed = PathBuf::from(indexed_path);
    let indexed = indexed.canonicalize().unwrap_or(indexed);
    path_is_same_or_inside(&indexed.to_string_lossy(), &requested.to_string_lossy())
        && path_is_same_or_inside(&requested.to_string_lossy(), &indexed.to_string_lossy())
}

fn knowledge_preview_mime_supported(path: &Path) -> bool {
    let mime = mime_type_for_path(path);
    mime.starts_with("image/")
        || mime.starts_with("video/")
        || mime.starts_with("text/plain")
        || mime.starts_with("text/markdown")
}

fn directory_storage_bytes(path: &Path) -> u64 {
    let Ok(metadata) = fs::metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| directory_storage_bytes(&entry.path()))
        .sum()
}

fn max_unix_timestamp_string(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => {
            let left_value = left.parse::<u64>().unwrap_or_default();
            let right_value = right.parse::<u64>().unwrap_or_default();
            if right_value > left_value {
                Some(right)
            } else {
                Some(left)
            }
        }
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn knowledge_root_status(
    root: &KnowledgeSourceRoot,
    error: Option<String>,
) -> KnowledgeIndexRootStatus {
    let exists = Path::new(root.path.trim()).exists();
    let status = if !root.enabled {
        "disabled"
    } else if error.is_some() {
        "blocked"
    } else if exists && root.last_indexed_at.is_some() {
        "ready"
    } else if exists {
        "needs-index"
    } else {
        "missing"
    }
    .to_string();
    let detail = error.unwrap_or_else(|| {
        if !root.enabled {
            "Source root is disabled.".to_string()
        } else if exists {
            "Source root exists and can be indexed through HarborBeacon.".to_string()
        } else {
            "Source root path does not exist on this host.".to_string()
        }
    });
    KnowledgeIndexRootStatus {
        root_id: root.root_id.clone(),
        label: root.label.clone(),
        path: root.path.clone(),
        enabled: root.enabled,
        exists,
        last_indexed_at: root.last_indexed_at.clone(),
        status,
        detail,
    }
}

fn build_files_browse_response(
    requested_path: Option<&str>,
    settings: &KnowledgeSettings,
) -> Result<FilesBrowseResponse, String> {
    let allowed_roots = knowledge_browse_allowed_roots(settings);
    let requested = requested_path
        .map(PathBuf::from)
        .or_else(|| allowed_roots.first().map(PathBuf::from))
        .ok_or_else(|| "No HarborOS file browse roots are available.".to_string())?;
    if !allowed_roots
        .iter()
        .any(|root| path_is_same_or_inside(&requested.to_string_lossy(), root))
    {
        return Err(format!(
            "requested path is not inside an allowed HarborOS file browse root: {}",
            requested.display()
        ));
    }
    let path = requested.canonicalize().unwrap_or(requested);
    if !path.is_dir() {
        return Err(format!(
            "requested path is not a directory: {}",
            path.display()
        ));
    }
    let mut entries = fs::read_dir(&path)
        .map_err(|error| format!("failed to list directory {}: {error}", path.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let is_dir = metadata.is_dir();
            let name = entry.file_name().to_string_lossy().into_owned();
            Some(FileBrowseEntry {
                name,
                path: entry.path().to_string_lossy().into_owned(),
                is_dir,
                size_bytes: (!is_dir).then_some(metadata.len()),
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(FilesBrowseResponse {
        parent: path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned()),
        path: path.to_string_lossy().into_owned(),
        readonly: true,
        allowed_roots,
        entries,
    })
}

fn knowledge_browse_allowed_roots(settings: &KnowledgeSettings) -> Vec<String> {
    let mut roots = Vec::new();
    roots.push(harboros_writable_root());
    if Path::new("/mnt").exists() {
        roots.push("/mnt".to_string());
    }
    roots.extend(settings.source_roots.iter().map(|root| root.path.clone()));
    if let Some(parent) = Path::new(&settings.index_root).parent() {
        roots.push(parent.to_string_lossy().into_owned());
    }
    let mut seen = Vec::<String>::new();
    roots
        .into_iter()
        .filter_map(|root| non_empty_string(&root))
        .filter(|root| {
            let exists = seen.iter().any(|seen_root| {
                path_is_same_or_inside(root, seen_root) && path_is_same_or_inside(seen_root, root)
            });
            if exists {
                false
            } else {
                seen.push(root.clone());
                true
            }
        })
        .collect()
}

fn build_rag_model_readiness(model_endpoints: &[ModelEndpoint]) -> Vec<RagModelReadinessCard> {
    [
        (
            ModelKind::Ocr,
            "OCR",
            "Image and scanned document text extraction",
        ),
        (
            ModelKind::Vlm,
            "VLM",
            "Image/keyframe caption and visual summary",
        ),
        (
            ModelKind::Embedder,
            "Embedder",
            "Dense retrieval embeddings",
        ),
        (ModelKind::Llm, "LLM", "Answer synthesis over cited context"),
        (ModelKind::Asr, "ASR", "Audio/video transcript extraction"),
    ]
    .into_iter()
    .map(|(kind, label, detail)| rag_model_readiness_card(model_endpoints, kind, label, detail))
    .collect()
}

fn rag_model_readiness_card(
    model_endpoints: &[ModelEndpoint],
    kind: ModelKind,
    label: &str,
    detail: &str,
) -> RagModelReadinessCard {
    let endpoint = model_endpoints
        .iter()
        .find(|endpoint| {
            endpoint.model_kind == kind && endpoint.status == ModelEndpointStatus::Active
        })
        .or_else(|| {
            model_endpoints
                .iter()
                .find(|endpoint| endpoint.model_kind == kind)
        });
    let status = endpoint
        .map(|endpoint| {
            if endpoint.status == ModelEndpointStatus::Active {
                "ready"
            } else {
                "needs-config"
            }
        })
        .unwrap_or("needs-config")
        .to_string();
    let blocker = match endpoint {
        Some(endpoint) if endpoint.status == ModelEndpointStatus::Active => None,
        Some(endpoint) => Some(format!(
            "{} endpoint {} is {}.",
            label,
            endpoint.model_endpoint_id,
            endpoint.status.as_str()
        )),
        None => Some(format!("{label} endpoint is not configured.")),
    };
    RagModelReadinessCard {
        model_kind: kind.as_str().to_string(),
        label: label.to_string(),
        status,
        endpoint_id: endpoint.map(|endpoint| endpoint.model_endpoint_id.clone()),
        endpoint_kind: endpoint.map(|endpoint| endpoint.endpoint_kind.as_str().to_string()),
        provider_key: endpoint.map(|endpoint| endpoint.provider_key.clone()),
        model_name: endpoint.map(|endpoint| endpoint.model_name.clone()),
        detail: detail.to_string(),
        blocker,
    }
}

fn build_rag_privacy_policy_component(
    knowledge: &KnowledgeSettings,
    model_endpoints: &[ModelEndpoint],
) -> RagReadinessComponent {
    let cloud_endpoint_ready = has_active_endpoint_kind(model_endpoints, ModelEndpointKind::Cloud);
    let privacy = privacy_level_as_str(knowledge.privacy_level);
    let status = match knowledge.privacy_level {
        PrivacyLevel::StrictLocal => "ready",
        PrivacyLevel::AllowRedactedCloud => {
            if cloud_endpoint_ready {
                "needs-config"
            } else {
                "blocked"
            }
        }
        PrivacyLevel::AllowCloud => {
            if cloud_endpoint_ready {
                "needs-config"
            } else {
                "blocked"
            }
        }
    }
    .to_string();
    RagReadinessComponent {
        status,
        summary: format!("Privacy policy: {privacy}"),
        detail: match knowledge.privacy_level {
            PrivacyLevel::StrictLocal => {
                "Cloud execution is blocked by default; RAG must stay local or return degraded status.".to_string()
            }
            PrivacyLevel::AllowRedactedCloud => {
                "Cloud execution is allowed only after redaction and audit records are present.".to_string()
            }
            PrivacyLevel::AllowCloud => {
                "Cloud execution is permitted by workspace policy and still requires audit evidence.".to_string()
            }
        },
        evidence: vec![
            format!("privacy_level={privacy}"),
            format!("cloud_endpoint_ready={cloud_endpoint_ready}"),
        ],
    }
}

fn build_rag_resource_profiles(
    knowledge: &KnowledgeSettings,
    model_endpoints: &[ModelEndpoint],
    storage_writable: bool,
    embedding_ready: bool,
) -> Vec<RagResourceProfileStatus> {
    [
        RagResourceProfile::CpuOnly,
        RagResourceProfile::LocalGpu,
        RagResourceProfile::SidecarGpu,
        RagResourceProfile::CloudAllowed,
    ]
    .into_iter()
    .map(|profile| {
        rag_resource_profile_status(
            profile,
            knowledge,
            model_endpoints,
            storage_writable,
            embedding_ready,
        )
    })
    .collect()
}

fn rag_resource_profile_status(
    profile: RagResourceProfile,
    knowledge: &KnowledgeSettings,
    model_endpoints: &[ModelEndpoint],
    storage_writable: bool,
    embedding_ready: bool,
) -> RagResourceProfileStatus {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if !storage_writable {
        blockers.push("RAG index storage is not writable.".to_string());
    }
    if !embedding_ready {
        blockers.push("Embedding runtime is not ready.".to_string());
    }
    match profile {
        RagResourceProfile::CpuOnly => {
            warnings.push("Audio/video ingestion may be slow on CPU-only hosts.".to_string());
        }
        RagResourceProfile::LocalGpu => {
            if !local_gpu_detected() {
                blockers.push("No local GPU was detected by the readiness probe.".to_string());
            }
        }
        RagResourceProfile::SidecarGpu => {
            if !has_active_endpoint_kind(model_endpoints, ModelEndpointKind::Sidecar) {
                blockers.push("No active sidecar model endpoint is configured.".to_string());
            }
        }
        RagResourceProfile::CloudAllowed => {
            if knowledge.privacy_level == PrivacyLevel::StrictLocal {
                blockers.push("privacy_level strict_local blocks cloud execution.".to_string());
            }
            if !has_active_endpoint_kind(model_endpoints, ModelEndpointKind::Cloud) {
                blockers.push("No active cloud model endpoint is configured.".to_string());
            }
            warnings.push(
                "Cloud use requires redaction and audit evidence before execution.".to_string(),
            );
        }
    }
    let status = if blockers.is_empty() {
        "ready"
    } else if storage_writable || embedding_ready {
        "needs-config"
    } else {
        "blocked"
    };
    RagResourceProfileStatus {
        profile: profile.as_str().to_string(),
        label: match profile {
            RagResourceProfile::CpuOnly => "CPU only",
            RagResourceProfile::LocalGpu => "Local GPU",
            RagResourceProfile::SidecarGpu => "Sidecar GPU",
            RagResourceProfile::CloudAllowed => "Cloud allowed",
        }
        .to_string(),
        status: status.to_string(),
        detail: if profile == knowledge.default_resource_profile {
            "Default RAG resource profile.".to_string()
        } else {
            "Optional RAG resource profile.".to_string()
        },
        blockers,
        warnings,
    }
}

fn build_rag_capability_profiles(
    storage_summary: &KnowledgeIndexStorageSummary,
    model_readiness: &[RagModelReadinessCard],
    storage_writable: bool,
    embedding_ready: bool,
    media_parser_ready: bool,
    default_profile: RagResourceProfile,
) -> Vec<RagCapabilityReadinessCard> {
    let mut text_blockers = Vec::new();
    if !storage_writable {
        text_blockers.push("RAG index storage is not writable.".to_string());
    }
    if !embedding_ready {
        text_blockers.push("Embedding runtime is not ready.".to_string());
    }
    let text_status = if text_blockers.is_empty() {
        "ready"
    } else if storage_summary.document_count > 0 {
        "degraded"
    } else {
        "blocked"
    };

    let vlm_ready = rag_model_ready(model_readiness, ModelKind::Vlm);
    let ocr_ready = rag_model_ready(model_readiness, ModelKind::Ocr);
    let mut image_blockers = Vec::new();
    let mut image_warnings = Vec::new();
    if !vlm_ready {
        image_blockers
            .push("VLM endpoint is required for content-level natural photo indexing.".to_string());
    }
    if !storage_writable {
        image_blockers.push("RAG index storage is not writable.".to_string());
    }
    if !embedding_ready {
        image_warnings.push(
            "Embedding runtime is not ready; image retrieval can only use content text lexical matching.".to_string(),
        );
    }
    if !ocr_ready {
        image_warnings
            .push("OCR endpoint is not ready; OCR can only be used as a supplement.".to_string());
    }
    if !media_parser_ready {
        image_warnings.push("No local media parser binary was detected.".to_string());
    }
    if storage_summary.image_count == 0 {
        image_warnings.push("No indexed image entries were found yet.".to_string());
    } else if storage_summary.vlm_indexed_image_count == 0 {
        image_warnings.push(
            "Indexed images do not yet include VLM-derived content; do not treat photo RAG as ready.".to_string(),
        );
    }
    if storage_summary.image_content_missing_count > 0 {
        image_warnings.push(format!(
            "{} indexed image(s) are missing VLM/OCR content text.",
            storage_summary.image_content_missing_count
        ));
    }
    if default_profile == RagResourceProfile::CpuOnly {
        image_warnings.push(
            "CPU-only VLM indexing may be slow, but it must still produce image-derived content."
                .to_string(),
        );
    }
    let image_status = if !image_blockers.is_empty() {
        "blocked"
    } else if embedding_ready && storage_summary.vlm_indexed_image_count > 0 {
        "ready"
    } else {
        "degraded"
    };

    vec![
        RagCapabilityReadinessCard {
            capability_id: "text_rag".to_string(),
            label: "Text RAG".to_string(),
            status: text_status.to_string(),
            summary: format!(
                "{} document(s), {} embedding cache entrie(s)",
                storage_summary.document_count, storage_summary.embedding_entry_count
            ),
            blockers: text_blockers,
            warnings: Vec::new(),
            evidence: vec![
                format!("document_count={}", storage_summary.document_count),
                format!("embedding_ready={embedding_ready}"),
                format!("storage_writable={storage_writable}"),
            ],
        },
        RagCapabilityReadinessCard {
            capability_id: "image_rag".to_string(),
            label: "Image RAG".to_string(),
            status: image_status.to_string(),
            summary: format!(
                "{} image(s), {} content-indexed, {} with VLM, {} with OCR",
                storage_summary.image_count,
                storage_summary.content_indexed_image_count,
                storage_summary.vlm_indexed_image_count,
                storage_summary.ocr_indexed_image_count
            ),
            blockers: image_blockers,
            warnings: image_warnings,
            evidence: vec![
                format!("image_count={}", storage_summary.image_count),
                format!(
                    "content_indexed_image_count={}",
                    storage_summary.content_indexed_image_count
                ),
                format!(
                    "vlm_indexed_image_count={}",
                    storage_summary.vlm_indexed_image_count
                ),
                format!(
                    "ocr_indexed_image_count={}",
                    storage_summary.ocr_indexed_image_count
                ),
                format!(
                    "image_content_missing_count={}",
                    storage_summary.image_content_missing_count
                ),
                format!("vlm_endpoint_ready={vlm_ready}"),
                format!("ocr_endpoint_ready={ocr_ready}"),
                format!("embedding_ready={embedding_ready}"),
                format!("storage_writable={storage_writable}"),
            ],
        },
    ]
}

fn rag_model_ready(model_readiness: &[RagModelReadinessCard], kind: ModelKind) -> bool {
    model_readiness
        .iter()
        .any(|card| card.model_kind == kind.as_str() && card.status == "ready")
}

fn has_active_endpoint_kind(
    model_endpoints: &[ModelEndpoint],
    endpoint_kind: ModelEndpointKind,
) -> bool {
    model_endpoints.iter().any(|endpoint| {
        endpoint.endpoint_kind == endpoint_kind && endpoint.status == ModelEndpointStatus::Active
    })
}

fn local_gpu_detected() -> bool {
    env::var("CUDA_VISIBLE_DEVICES")
        .ok()
        .map(|value| !value.trim().is_empty() && value.trim() != "-1")
        .unwrap_or(false)
        || command_available("nvidia-smi")
        || Path::new("/dev/nvidia0").exists()
        || Path::new("/dev/dri").exists()
}

fn privacy_level_as_str(level: PrivacyLevel) -> &'static str {
    match level {
        PrivacyLevel::StrictLocal => "strict_local",
        PrivacyLevel::AllowRedactedCloud => "allow_redacted_cloud",
        PrivacyLevel::AllowCloud => "allow_cloud",
    }
}

fn recent_knowledge_index_jobs(
    index_jobs: &[KnowledgeIndexJobRecord],
) -> Vec<KnowledgeIndexJobRecord> {
    let mut jobs = index_jobs.to_vec();
    jobs.sort_by(|left, right| {
        right
            .requested_at
            .cmp(&left.requested_at)
            .then_with(|| right.job_id.cmp(&left.job_id))
    });
    jobs.truncate(8);
    jobs
}

fn build_rag_readiness_response(
    runtime: &LocalModelRuntimeProjection,
    knowledge: &KnowledgeSettings,
    model_endpoints: &[ModelEndpoint],
    index_jobs: &[KnowledgeIndexJobRecord],
) -> RagReadinessResponse {
    let generated_at = now_unix_string();
    let index_dir = knowledge.index_root.clone();
    let index_path = Path::new(&index_dir);
    let index_exists = index_path.exists();
    let index_parent_exists = index_path.parent().map(Path::exists).unwrap_or(false);
    let storage_writable = path_can_accept_write(index_path);
    let storage_summary = knowledge_index_storage_summary(index_path);
    let embedding_ready = runtime.ready
        && runtime.backend_ready
        && runtime
            .embedding_model
            .as_ref()
            .is_some_and(|model| !model.trim().is_empty());
    let ffmpeg_ready = command_available("ffmpeg");
    let tesseract_ready = command_available("tesseract");
    let media_parser_ready = ffmpeg_ready || tesseract_ready;
    let enabled_source_roots = knowledge
        .source_roots
        .iter()
        .filter(|root| root.enabled)
        .collect::<Vec<_>>();
    let existing_enabled_source_roots = enabled_source_roots
        .iter()
        .filter(|root| Path::new(root.path.trim()).exists())
        .count();
    let model_readiness = build_rag_model_readiness(model_endpoints);
    let privacy_policy = build_rag_privacy_policy_component(knowledge, model_endpoints);

    let source_roots = RagReadinessComponent {
        status: if existing_enabled_source_roots > 0 {
            "ready"
        } else if enabled_source_roots.is_empty() {
            "needs-config"
        } else {
            "blocked"
        }
        .to_string(),
        summary: format!(
            "{} enabled source root(s), {} existing",
            enabled_source_roots.len(),
            existing_enabled_source_roots
        ),
        detail: "Knowledge source roots are configured in HarborDesk and are the only roots eligible for search or benchmark.".to_string(),
        evidence: knowledge
            .source_roots
            .iter()
            .map(|root| {
                format!(
                    "root_id={} enabled={} exists={} path={}",
                    root.root_id,
                    root.enabled,
                    Path::new(root.path.trim()).exists(),
                    root.path
                )
            })
            .collect(),
    };

    let index_directory = RagReadinessComponent {
        status: if index_exists || index_parent_exists {
            "ready"
        } else {
            "needs-config"
        }
        .to_string(),
        summary: if index_exists {
            format!("Index directory exists: {index_dir}")
        } else {
            format!("Index directory not created yet: {index_dir}")
        },
        detail:
            "RAG index path is persisted in knowledge.index_root and is managed from HarborDesk."
                .to_string(),
        evidence: vec![
            format!("index_dir={index_dir}"),
            format!("index_exists={index_exists}"),
            format!("index_parent_exists={index_parent_exists}"),
        ],
    };
    let embedding_model = RagReadinessComponent {
        status: if embedding_ready {
            "ready"
        } else {
            "needs-config"
        }
        .to_string(),
        summary: runtime
            .embedding_model
            .clone()
            .unwrap_or_else(|| "embedding model not detected".to_string()),
        detail: "Embedding readiness is read from harbor-model-api /healthz runtime projection."
            .to_string(),
        evidence: vec![
            format!("runtime_ready={}", runtime.ready),
            format!("backend_ready={}", runtime.backend_ready),
            format!(
                "embedding_model={}",
                runtime
                    .embedding_model
                    .clone()
                    .unwrap_or_else(|| "none".to_string())
            ),
        ],
    };
    let media_parser = RagReadinessComponent {
        status: if media_parser_ready {
            "ready"
        } else {
            "needs-config"
        }
        .to_string(),
        summary: if media_parser_ready {
            "At least one media parser is available".to_string()
        } else {
            "No media parser binary detected".to_string()
        },
        detail: "Multimodal RAG can use ffmpeg for audio/video and tesseract for OCR when present."
            .to_string(),
        evidence: vec![
            format!("ffmpeg_present={ffmpeg_ready}"),
            format!("tesseract_present={tesseract_ready}"),
        ],
    };
    let storage = RagReadinessComponent {
        status: if storage_writable { "ready" } else { "needs-config" }.to_string(),
        summary: if storage_writable {
            "RAG storage path appears writable".to_string()
        } else {
            "RAG storage path is not writable or parent is missing".to_string()
        },
        detail: "Readiness uses non-mutating filesystem metadata checks; it does not create the index automatically.".to_string(),
        evidence: vec![format!("storage_writable={storage_writable}")],
    };
    let resource_profiles = build_rag_resource_profiles(
        knowledge,
        model_endpoints,
        storage_writable,
        embedding_ready,
    );
    let capability_profiles = build_rag_capability_profiles(
        &storage_summary,
        &model_readiness,
        storage_writable,
        embedding_ready,
        media_parser_ready,
        knowledge.default_resource_profile,
    );

    let mut blockers = Vec::new();
    if !embedding_ready {
        blockers.push("Embedding model is not ready.".to_string());
    }
    if existing_enabled_source_roots == 0 {
        blockers.push("No enabled knowledge source root exists on this host.".to_string());
    }
    if !storage_writable {
        blockers.push("RAG index storage is not writable.".to_string());
    }
    if !media_parser_ready {
        blockers.push("No multimodal media parser was detected.".to_string());
    }
    for model in &model_readiness {
        if model.status != "ready" {
            blockers.push(model.blocker.clone().unwrap_or_else(|| {
                format!("{} model readiness is {}.", model.label, model.status)
            }));
        }
    }
    let mut warnings = Vec::new();
    if knowledge.privacy_level != PrivacyLevel::StrictLocal {
        warnings.push(
            "Cloud-capable privacy policy is configured; redaction and audit are required before cloud execution.".to_string(),
        );
    }
    for capability in &capability_profiles {
        if capability.capability_id == "image_rag" && capability.status == "blocked" {
            blockers.push(format!(
                "Image RAG is blocked: {}",
                capability.blockers.join("; ")
            ));
        } else if capability.capability_id == "image_rag" && capability.status == "degraded" {
            warnings.push(format!("Image RAG is degraded: {}", capability.summary));
        }
    }
    if knowledge.default_resource_profile == RagResourceProfile::CloudAllowed
        && knowledge.privacy_level == PrivacyLevel::StrictLocal
    {
        blockers.push(
            "Default resource profile is cloud_allowed but privacy policy is strict_local."
                .to_string(),
        );
    }
    for profile in &resource_profiles {
        if profile.profile == knowledge.default_resource_profile.as_str()
            && profile.status == "blocked"
        {
            blockers.push(format!(
                "Default resource profile {} is blocked: {}",
                profile.profile,
                profile.blockers.join("; ")
            ));
        }
    }
    let status = if blockers.is_empty() {
        "ready"
    } else if embedding_ready
        || storage_writable
        || media_parser_ready
        || existing_enabled_source_roots > 0
    {
        "needs-config"
    } else {
        "blocked"
    }
    .to_string();
    let mut evidence = Vec::new();
    evidence.extend(source_roots.evidence.clone());
    evidence.extend(index_directory.evidence.clone());
    evidence.extend(embedding_model.evidence.clone());
    evidence.extend(privacy_policy.evidence.clone());
    evidence.extend(media_parser.evidence.clone());
    evidence.extend(storage.evidence.clone());
    evidence.push(format!(
        "default_resource_profile={}",
        knowledge.default_resource_profile.as_str()
    ));

    RagReadinessResponse {
        generated_at,
        status,
        summary: if blockers.is_empty() {
            "Multimodal RAG admin skeleton is configured.".to_string()
        } else {
            format!(
                "{} blocker(s) require admin action before RAG is ready.",
                blockers.len()
            )
        },
        source_roots,
        index_directory,
        embedding_model,
        model_readiness,
        resource_profiles,
        capability_profiles,
        privacy_policy,
        media_parser,
        storage_writable: storage,
        index_jobs: recent_knowledge_index_jobs(index_jobs),
        blockers,
        warnings,
        evidence,
    }
}

fn path_can_accept_write(path: &Path) -> bool {
    let candidate = if path.exists() {
        path
    } else {
        match path.parent() {
            Some(parent) => parent,
            None => path,
        }
    };
    fs::metadata(candidate)
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

fn proc_mem_total_mb() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb = line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())?;
    Some(kb / 1024)
}

fn gpu_probe_evidence() -> Vec<String> {
    let mut evidence = Vec::new();
    let cuda_visible = env::var("CUDA_VISIBLE_DEVICES")
        .ok()
        .map(|value| !value.trim().is_empty() && value.trim() != "-1")
        .unwrap_or(false);
    evidence.push(format!("cuda_visible_devices_present={cuda_visible}"));
    evidence.push(format!(
        "nvidia_smi_present={}",
        command_available("nvidia-smi")
    ));
    evidence.push(format!(
        "dev_nvidia0_present={}",
        Path::new("/dev/nvidia0").exists()
    ));
    evidence.push(format!(
        "dev_dri_present={}",
        Path::new("/dev/dri").exists()
    ));
    let present = cuda_visible
        || command_available("nvidia-smi")
        || Path::new("/dev/nvidia0").exists()
        || Path::new("/dev/dri").exists();
    evidence.push(format!("gpu_present={present}"));
    evidence
}

fn npu_probe_evidence() -> Vec<String> {
    let candidates = [
        "/dev/accel/accel0",
        "/dev/apex_0",
        "/dev/davinci0",
        "/dev/hisi_hdc",
        "/dev/vpu_service",
    ];
    let mut evidence = candidates
        .iter()
        .map(|path| format!("{path}_present={}", Path::new(path).exists()))
        .collect::<Vec<_>>();
    let present = candidates.iter().any(|path| Path::new(path).exists());
    evidence.push(format!("npu_present={present}"));
    evidence
}

fn command_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn build_harboros_status_response(public_origin: &str) -> HarborOsStatusResponse {
    let webui_url = harboros_webui_url(public_origin);
    let writable_root = env::var("HARBOR_HARBOROS_WRITABLE_ROOT")
        .unwrap_or_else(|_| "/mnt/software/harborbeacon-agent-ci".to_string());
    let writable_root_exists = Path::new(&writable_root).exists();
    let version = env::var("HARBOROS_VERSION")
        .ok()
        .and_then(|value| non_empty_string(&value))
        .or_else(os_release_pretty_name)
        .unwrap_or_else(|| "unknown".to_string());
    let services = vec![
        HarborOsServiceStatus {
            service_id: "harbordesk-admin-api".to_string(),
            label: "HarborDesk Admin API".to_string(),
            status: "ready".to_string(),
            detail: "Current process serves HarborDesk on port 4174.".to_string(),
        },
        HarborOsServiceStatus {
            service_id: "harboros-webui".to_string(),
            label: "HarborOS WebUI".to_string(),
            status: "external".to_string(),
            detail: format!("Expected at {webui_url}; HarborDesk does not own this port."),
        },
        HarborOsServiceStatus {
            service_id: "writable-root".to_string(),
            label: "HarborOS writable root".to_string(),
            status: if writable_root_exists {
                "ready"
            } else {
                "needs-config"
            }
            .to_string(),
            detail: writable_root.clone(),
        },
    ];
    let jobs_alerts = HarborOsServiceStatus {
        service_id: "jobs-alerts-readiness".to_string(),
        label: "Jobs / Alerts entry readiness".to_string(),
        status: "ready".to_string(),
        detail:
            "Safe query capability only; approval is required before any state-changing job action."
                .to_string(),
    };
    let storage_files_entry = HarborOsServiceStatus {
        service_id: "storage-files-entry-readiness".to_string(),
        label: "Storage / Files entry readiness".to_string(),
        status: if writable_root_exists {
            "ready"
        } else {
            "needs-config"
        }
        .to_string(),
        detail: format!(
            "Storage entry is checked through HarborOS System Domain path: {writable_root}"
        ),
    };
    let blockers = if writable_root_exists {
        Vec::new()
    } else {
        vec![format!("writable root not found: {writable_root}")]
    };
    HarborOsStatusResponse {
        generated_at: now_unix_string(),
        status: if blockers.is_empty() {
            "ready"
        } else {
            "needs-config"
        }
        .to_string(),
        version,
        webui_url,
        system_domain_only: true,
        services,
        jobs_alerts,
        storage_files_entry,
        evidence: vec![
            "domain=HarborOS System Domain".to_string(),
            "aiot_management=excluded".to_string(),
            "jobs_alerts.safe_query=true".to_string(),
            "storage_files.entry_ready_checked=true".to_string(),
            format!("writable_root_exists={writable_root_exists}"),
        ],
        blockers,
    }
}

fn os_release_pretty_name() -> Option<String> {
    let text = fs::read_to_string("/etc/os-release").ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_string())
        .and_then(|value| non_empty_string(&value))
}

fn build_harboros_im_capability_map() -> HarborOsImCapabilityMapResponse {
    HarborOsImCapabilityMapResponse {
        generated_at: now_unix_string(),
        source: "exports/harboros_webui_manual/harboros_webui_user_guide.md".to_string(),
        items: vec![
            harboros_im_capability(
                "dashboard.status",
                "Dashboard system status",
                true,
                "low",
                false,
                "Dashboard",
                "Query version, hostname, uptime, CPU, memory, and temperature.",
            ),
            harboros_im_capability(
                "jobs.alerts",
                "Jobs and Alerts",
                true,
                "low",
                false,
                "Jobs / Alerts",
                "Read-only recent task and alert summaries are IM-safe.",
            ),
            harboros_im_capability(
                "services.status",
                "Service status",
                true,
                "low",
                false,
                "System / Services",
                "Read service state without changing autostart or configuration.",
            ),
            harboros_im_capability(
                "storage.summary",
                "Storage summary",
                true,
                "medium",
                false,
                "Storage / Datasets",
                "Read pool and dataset summary; mutation requires approval.",
            ),
            harboros_im_capability(
                "file.entrypoints",
                "File Manager entrypoints",
                true,
                "medium",
                false,
                "File Manager",
                "Expose locations and status, not arbitrary file mutation.",
            ),
            harboros_im_capability(
                "services.restart",
                "Service restart",
                false,
                "high",
                true,
                "System / Services",
                "High-risk operation; show capability and require approval before execution.",
            ),
            harboros_im_capability(
                "backup.restore",
                "Backup and restore",
                false,
                "high",
                true,
                "Data Protection",
                "High-risk state-changing workflow; not auto-executed from readiness.",
            ),
        ],
    }
}

fn harboros_im_capability(
    capability_id: &str,
    label: &str,
    im_ready: bool,
    risk_level: &str,
    approval_required: bool,
    harboros_surface: &str,
    notes: &str,
) -> HarborOsImCapabilityItem {
    HarborOsImCapabilityItem {
        capability_id: capability_id.to_string(),
        label: label.to_string(),
        capability_class: if approval_required {
            "approval_required_action"
        } else if im_ready {
            "safe_query"
        } else {
            "unsupported_high_risk"
        }
        .to_string(),
        im_ready,
        risk_level: risk_level.to_string(),
        approval_required,
        harboros_surface: harboros_surface.to_string(),
        notes: notes.to_string(),
    }
}

fn build_local_model_catalog(
    download_jobs: Vec<ModelDownloadJobRecord>,
) -> LocalModelCatalogResponse {
    let cache_roots = local_model_cache_roots();
    let models = local_model_catalog_specs()
        .into_iter()
        .map(|spec| local_model_catalog_item(&cache_roots, &download_jobs, spec))
        .collect::<Vec<_>>();
    let generated_at = now_unix_string();
    LocalModelCatalogResponse {
        generated_at: generated_at.clone(),
        checked_at: generated_at,
        status: if models.is_empty() {
            "needs-config"
        } else {
            "ready"
        }
        .to_string(),
        cache_roots,
        models,
        download_jobs: download_jobs.clone(),
        downloads: download_jobs,
        blockers: Vec::new(),
        warnings: Vec::new(),
    }
}

#[derive(Debug, Clone)]
struct LocalModelCatalogSpec {
    model_id: &'static str,
    display_name: &'static str,
    provider_key: &'static str,
    model_kind: &'static str,
    recommended_hardware: &'static str,
    download_size_hint: &'static str,
    source_kind: &'static str,
    repo_id: Option<&'static str>,
    revision: &'static str,
    file_policy: &'static str,
    runtime_profiles: &'static [&'static str],
    expected_capabilities: &'static [&'static str],
    acceptance_note: Option<&'static str>,
}

fn local_model_catalog_specs() -> Vec<LocalModelCatalogSpec> {
    vec![
        LocalModelCatalogSpec {
            model_id: "Qwen/Qwen3.5-4B",
            display_name: "Qwen3.5 4B",
            provider_key: "qwen",
            model_kind: "llm_vlm",
            recommended_hardware: "GPU 16GB+; primary 182 live-test target",
            download_size_hint: "10-15 GB",
            source_kind: "huggingface",
            repo_id: Some("Qwen/Qwen3.5-4B"),
            revision: "main",
            file_policy: "runtime_snapshot",
            runtime_profiles: &["vllm-openai-compatible", "sglang-openai-compatible"],
            expected_capabilities: &["llm", "vlm", "image_text_to_text"],
            acceptance_note: Some("primary-live-test"),
        },
        LocalModelCatalogSpec {
            model_id: "Qwen/Qwen3.5-9B",
            display_name: "Qwen3.5 9B",
            provider_key: "qwen",
            model_kind: "llm_vlm",
            recommended_hardware: "GPU 16GB+ only after 4B path is green",
            download_size_hint: "20-30 GB",
            source_kind: "huggingface",
            repo_id: Some("Qwen/Qwen3.5-9B"),
            revision: "main",
            file_policy: "runtime_snapshot",
            runtime_profiles: &["vllm-openai-compatible", "sglang-openai-compatible"],
            expected_capabilities: &["llm", "vlm", "image_text_to_text"],
            acceptance_note: Some("stretch-after-4b"),
        },
        LocalModelCatalogSpec {
            model_id: "Qwen/Qwen3.6-35B-A3B",
            display_name: "Qwen3.6 35B A3B",
            provider_key: "qwen",
            model_kind: "llm_vlm",
            recommended_hardware: "multi-GPU recommended; not a 182 16GB acceptance target",
            download_size_hint: "60+ GB",
            source_kind: "huggingface",
            repo_id: Some("Qwen/Qwen3.6-35B-A3B"),
            revision: "main",
            file_policy: "runtime_snapshot",
            runtime_profiles: &["vllm-openai-compatible", "sglang-openai-compatible"],
            expected_capabilities: &["llm", "vlm", "image_text_to_text"],
            acceptance_note: Some("not-today-acceptance"),
        },
        LocalModelCatalogSpec {
            model_id: "Qwen/Qwen3-Embedding-0.6B",
            display_name: "Qwen3 Embedding 0.6B",
            provider_key: "qwen",
            model_kind: "embedder",
            recommended_hardware: "CPU or GPU",
            download_size_hint: "1-2 GB",
            source_kind: "huggingface",
            repo_id: Some("Qwen/Qwen3-Embedding-0.6B"),
            revision: "main",
            file_policy: "runtime_snapshot",
            runtime_profiles: &["openai-compatible-embedding"],
            expected_capabilities: &["embedding"],
            acceptance_note: Some("retrieval-quality-upgrade"),
        },
        LocalModelCatalogSpec {
            model_id: "qwen2.5-1.5b-instruct",
            display_name: "Qwen2.5 1.5B Instruct",
            provider_key: "qwen",
            model_kind: "llm",
            recommended_hardware: "CPU 8GB+",
            download_size_hint: "1-2 GB",
            source_kind: "manual_or_url",
            repo_id: None,
            revision: "main",
            file_policy: "single_file_or_existing_cache",
            runtime_profiles: &["harbor-model-api-candle"],
            expected_capabilities: &["llm"],
            acceptance_note: Some("legacy-catalog"),
        },
        LocalModelCatalogSpec {
            model_id: "jina-embeddings-v2-base-zh",
            display_name: "Jina Embeddings v2 zh",
            provider_key: "jina",
            model_kind: "embedder",
            recommended_hardware: "CPU 4GB+",
            download_size_hint: "300-700 MB",
            source_kind: "manual_or_url",
            repo_id: None,
            revision: "main",
            file_policy: "single_file_or_existing_cache",
            runtime_profiles: &["harbor-model-api-candle"],
            expected_capabilities: &["embedding"],
            acceptance_note: Some("legacy-catalog"),
        },
        LocalModelCatalogSpec {
            model_id: "bge-m3",
            display_name: "BGE M3 Embedding",
            provider_key: "bge",
            model_kind: "embedder",
            recommended_hardware: "CPU 4GB+",
            download_size_hint: "1-2 GB",
            source_kind: "manual_or_url",
            repo_id: None,
            revision: "main",
            file_policy: "single_file_or_existing_cache",
            runtime_profiles: &["openai-compatible-embedding"],
            expected_capabilities: &["embedding"],
            acceptance_note: Some("legacy-catalog"),
        },
        LocalModelCatalogSpec {
            model_id: "HuggingFaceTB/SmolVLM-256M-Instruct",
            display_name: "SmolVLM 256M Instruct",
            provider_key: "huggingfacetb",
            model_kind: "vlm",
            recommended_hardware:
                "CPU smoke VLM; slow but suitable for VM content-index validation",
            download_size_hint: "1-2 GB",
            source_kind: "huggingface",
            repo_id: Some("HuggingFaceTB/SmolVLM-256M-Instruct"),
            revision: "main",
            file_policy: "runtime_snapshot",
            runtime_profiles: &["openai-compatible-vlm", "cpu-vlm-sidecar"],
            expected_capabilities: &["vlm", "image_text_to_text"],
            acceptance_note: Some("vm-cpu-photo-rag"),
        },
        LocalModelCatalogSpec {
            model_id: "minicpm-v-2.6",
            display_name: "MiniCPM-V 2.6",
            provider_key: "minicpm",
            model_kind: "vlm",
            recommended_hardware: "GPU recommended",
            download_size_hint: "4-8 GB",
            source_kind: "manual_or_url",
            repo_id: None,
            revision: "main",
            file_policy: "single_file_or_existing_cache",
            runtime_profiles: &["openai-compatible-vlm"],
            expected_capabilities: &["vlm"],
            acceptance_note: Some("legacy-catalog"),
        },
    ]
}

fn local_model_catalog_item(
    cache_roots: &[String],
    download_jobs: &[ModelDownloadJobRecord],
    spec: LocalModelCatalogSpec,
) -> LocalModelCatalogItem {
    let latest_job = latest_model_download_job(download_jobs, spec.model_id);
    let candidate_path = find_cached_model_path(cache_roots, spec.model_id).or_else(|| {
        latest_job
            .and_then(|job| job.target_path.as_ref())
            .filter(|path| Path::new(path).exists())
            .cloned()
    });
    let candidate_size = candidate_path
        .as_ref()
        .and_then(|path| model_path_size(Path::new(path)).ok());
    let local_path = candidate_path
        .as_ref()
        .zip(candidate_size)
        .and_then(|(path, size)| (size > 0).then(|| path.clone()));
    let installed = local_path.is_some();
    let size_bytes = installed.then_some(candidate_size).flatten();
    let status = if installed {
        "ready"
    } else if let Some(job) = latest_job {
        match job.status.as_str() {
            "queued" | "running" | "downloading" => "running",
            "completed" | "failed" => "blocked",
            "canceled" | "cancelled" => "needs-config",
            _ => "needs-config",
        }
    } else {
        "needs-config"
    }
    .to_string();
    let mut evidence = vec![
        format!("model_id={}", spec.model_id),
        format!("source_kind={}", spec.source_kind),
        format!("file_policy={}", spec.file_policy),
    ];
    if let Some(repo_id) = spec.repo_id {
        evidence.push(format!("repo_id={repo_id}"));
        evidence.push(format!("revision={}", spec.revision));
    }
    if let Some(path) = local_path.as_ref() {
        evidence.push(format!("local_path={path}"));
    } else if let Some(path) = candidate_path.as_ref() {
        evidence.push(format!("ignored_incomplete_local_path={path}"));
        if let Some(size) = candidate_size {
            evidence.push(format!("ignored_incomplete_size_bytes={size}"));
        }
    }
    if let Some(job) = latest_job {
        evidence.push(format!("latest_download_job={}", job.job_id));
        evidence.push(format!("latest_download_status={}", job.status));
    }
    let detail = if installed {
        format!(
            "{} is installed at {}.",
            spec.display_name,
            local_path.clone().unwrap_or_default()
        )
    } else if let Some(job) = latest_job {
        format!(
            "{} latest download job is {} with status {}.",
            spec.display_name, job.job_id, job.status
        )
    } else if spec.source_kind == "huggingface" {
        format!(
            "Direct Hugging Face snapshot download is available for {}.",
            spec.repo_id.unwrap_or(spec.model_id)
        )
    } else {
        "Configure a source_url or install into the model cache root.".to_string()
    };
    LocalModelCatalogItem {
        model_id: spec.model_id.to_string(),
        label: spec.display_name.to_string(),
        display_name: spec.display_name.to_string(),
        provider: spec.provider_key.to_string(),
        provider_key: spec.provider_key.to_string(),
        model_kind: spec.model_kind.to_string(),
        recommended_hardware: spec.recommended_hardware.to_string(),
        status,
        installed,
        local_path,
        size_bytes,
        download_job_id: latest_job.map(|job| job.job_id.clone()),
        download_size_hint: spec.download_size_hint.to_string(),
        detail,
        source_kind: spec.source_kind.to_string(),
        repo_id: spec.repo_id.map(str::to_string),
        revision: Some(spec.revision.to_string()),
        file_policy: spec.file_policy.to_string(),
        default_hf_endpoint: (spec.source_kind == "huggingface")
            .then(|| DEFAULT_HF_ENDPOINT.to_string()),
        runtime_profiles: spec
            .runtime_profiles
            .iter()
            .map(|item| item.to_string())
            .collect(),
        expected_capabilities: spec
            .expected_capabilities
            .iter()
            .map(|item| item.to_string())
            .collect(),
        acceptance_note: spec.acceptance_note.map(str::to_string),
        evidence,
    }
}

fn latest_model_download_job<'a>(
    jobs: &'a [ModelDownloadJobRecord],
    model_id: &str,
) -> Option<&'a ModelDownloadJobRecord> {
    jobs.iter()
        .filter(|job| job.model_id == model_id)
        .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
}

fn enrich_model_download_metadata(metadata: &mut Value, item: &LocalModelCatalogItem) {
    if !metadata.is_object() {
        *metadata = json!({});
    }
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    object
        .entry("source_kind".to_string())
        .or_insert_with(|| json!(item.source_kind));
    if let Some(repo_id) = item.repo_id.as_ref() {
        object
            .entry("repo_id".to_string())
            .or_insert_with(|| json!(repo_id));
    }
    if let Some(revision) = item.revision.as_ref() {
        object
            .entry("revision".to_string())
            .or_insert_with(|| json!(revision));
    }
    object
        .entry("file_policy".to_string())
        .or_insert_with(|| json!(item.file_policy));
}

fn model_download_jobs_status(jobs: &[ModelDownloadJobRecord]) -> &'static str {
    let latest_jobs = latest_model_download_jobs_by_model(jobs);
    if latest_jobs
        .values()
        .any(|job| matches!(job.status.as_str(), "queued" | "running" | "downloading"))
    {
        return "running";
    }
    if latest_jobs.values().any(|job| job.status == "failed") {
        return "blocked";
    }
    "ready"
}

fn latest_model_download_jobs_by_model<'a>(
    jobs: &'a [ModelDownloadJobRecord],
) -> HashMap<&'a str, &'a ModelDownloadJobRecord> {
    let mut latest_jobs: HashMap<&str, &ModelDownloadJobRecord> = HashMap::new();
    for job in jobs {
        let key = if job.model_id.trim().is_empty() {
            job.job_id.as_str()
        } else {
            job.model_id.as_str()
        };
        match latest_jobs.get(key) {
            Some(current) if !model_download_job_is_newer(job, current) => {}
            _ => {
                latest_jobs.insert(key, job);
            }
        }
    }
    latest_jobs
}

fn model_download_job_is_newer(
    candidate: &ModelDownloadJobRecord,
    current: &ModelDownloadJobRecord,
) -> bool {
    let candidate_key = (
        model_download_job_timestamp(&candidate.updated_at),
        model_download_job_timestamp(&candidate.requested_at),
        candidate.job_id.as_str(),
    );
    let current_key = (
        model_download_job_timestamp(&current.updated_at),
        model_download_job_timestamp(&current.requested_at),
        current.job_id.as_str(),
    );
    candidate_key > current_key
}

fn model_download_job_timestamp(value: &str) -> u64 {
    value.trim().parse::<u64>().unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelDownloadTransferStats {
    bytes_written: u64,
    total_bytes: Option<u64>,
    file_count: Option<usize>,
    manifest_path: Option<String>,
    message: String,
}

const MODEL_DOWNLOAD_CANCELED: &str = "model download canceled";

fn spawn_model_download_worker(
    store: AdminConsoleStore,
    job: ModelDownloadJobRecord,
) -> Result<(), String> {
    thread::Builder::new()
        .name("harborbeacon-model-download".to_string())
        .spawn(move || run_model_download_job(store, job))
        .map(|_| ())
        .map_err(|error| format!("failed to spawn model download worker: {error}"))
}

fn mark_model_download_spawn_failed(
    store: &AdminConsoleStore,
    mut job: ModelDownloadJobRecord,
    error: String,
) -> ModelDownloadJobRecord {
    let now = now_unix_string();
    job.status = "failed".to_string();
    job.progress_percent = Some(100);
    job.completed_at = Some(now.clone());
    job.updated_at = now;
    job.error_message = Some(error.clone());
    job.message = format!("download worker failed to start: {error}");
    store.save_model_download_job(job.clone()).unwrap_or(job)
}

fn run_model_download_job(store: AdminConsoleStore, mut job: ModelDownloadJobRecord) {
    if model_download_job_cancel_requested(&store, &job.job_id) {
        mark_model_download_canceled(&store, job, "canceled_before_start");
        return;
    }

    let started_at = now_unix_string();
    job.status = "downloading".to_string();
    job.started_at = job.started_at.or_else(|| Some(started_at.clone()));
    job.updated_at = started_at;
    job.progress_percent = Some(0);
    job.error_message = None;
    job.message = "download job started by explicit admin action".to_string();
    if let Err(error) = store.save_model_download_job(job.clone()) {
        mark_model_download_failed(&store, job, "job_state_write_failed", error);
        return;
    }

    let result = run_model_download_transfer_with_progress(&mut job, Some(&store));
    if model_download_job_cancel_requested(&store, &job.job_id) {
        mark_model_download_canceled(&store, job, "canceled_after_transfer");
        return;
    }

    let finished_at = now_unix_string();
    match result {
        Ok(stats) => {
            job.status = "completed".to_string();
            job.progress_percent = Some(100);
            job.bytes_downloaded = Some(stats.bytes_written);
            job.total_bytes = stats.total_bytes.or(Some(stats.bytes_written));
            job.completed_at = Some(finished_at.clone());
            job.updated_at = finished_at;
            job.error_message = None;
            job.message = stats.message;
            merge_model_download_metadata(
                &mut job.metadata,
                json!({
                    "file_count": stats.file_count,
                    "snapshot_manifest_path": stats.manifest_path,
                }),
            );
            let _ = store.save_model_download_job(job);
        }
        Err(error) if error == MODEL_DOWNLOAD_CANCELED => {
            mark_model_download_canceled(&store, job, "canceled_during_transfer");
        }
        Err(error) => mark_model_download_failed(&store, job, "transfer_failed", error),
    }
}

fn mark_model_download_failed(
    store: &AdminConsoleStore,
    mut job: ModelDownloadJobRecord,
    phase: &str,
    error: String,
) {
    let now = now_unix_string();
    job.status = "failed".to_string();
    job.progress_percent = Some(100);
    job.completed_at = Some(now.clone());
    job.updated_at = now;
    job.error_message = Some(error.clone());
    job.message = format!("download job failed at {phase}: {error}");
    let _ = store.save_model_download_job(job);
}

fn mark_model_download_canceled(
    store: &AdminConsoleStore,
    mut job: ModelDownloadJobRecord,
    phase: &str,
) {
    let now = now_unix_string();
    job.status = "canceled".to_string();
    job.progress_percent = job.progress_percent.or(Some(0));
    job.completed_at = Some(now.clone());
    job.updated_at = now;
    job.error_message = None;
    job.message = format!("download job canceled: {phase}");
    let _ = store.save_model_download_job(job);
}

fn model_download_job_cancel_requested(store: &AdminConsoleStore, job_id: &str) -> bool {
    matches!(
        store
            .model_download_job(job_id)
            .ok()
            .flatten()
            .map(|job| job.status),
        Some(status) if status == "canceled" || status == "cancelled"
    )
}

fn save_model_download_checkpoint(
    store: Option<&AdminConsoleStore>,
    job_id: &str,
    progress_percent: Option<u8>,
    bytes_downloaded: Option<u64>,
    total_bytes: Option<u64>,
    message: impl Into<String>,
) {
    let Some(store) = store else {
        return;
    };
    let Ok(Some(mut job)) = store.model_download_job(job_id) else {
        return;
    };
    if matches!(
        job.status.as_str(),
        "canceled" | "cancelled" | "completed" | "failed"
    ) {
        return;
    }
    job.status = "downloading".to_string();
    job.progress_percent = progress_percent;
    job.bytes_downloaded = bytes_downloaded;
    if total_bytes.is_some() {
        job.total_bytes = total_bytes;
    }
    job.updated_at = now_unix_string();
    job.message = message.into();
    let _ = store.save_model_download_job(job);
}

fn run_model_download_transfer(
    job: &ModelDownloadJobRecord,
) -> Result<ModelDownloadTransferStats, String> {
    let mut job = job.clone();
    run_model_download_transfer_with_progress(&mut job, None)
}

fn run_model_download_transfer_with_progress(
    job: &mut ModelDownloadJobRecord,
    store: Option<&AdminConsoleStore>,
) -> Result<ModelDownloadTransferStats, String> {
    if model_download_huggingface_repo_id(job).is_some() {
        return run_huggingface_snapshot_download(job, store);
    }

    let target_path = job
        .target_path
        .as_deref()
        .and_then(non_empty_string)
        .unwrap_or_else(|| default_model_download_target_path(&job.model_id));
    let target = PathBuf::from(&target_path);
    if target.exists() {
        let bytes = model_path_size(&target)?;
        return Ok(ModelDownloadTransferStats {
            bytes_written: bytes,
            total_bytes: Some(bytes),
            file_count: None,
            manifest_path: None,
            message: format!("model already present at {}", target.display()),
        });
    }

    let source = model_download_source_url(&job.metadata).ok_or_else(|| {
        "download source_url is required for executable download; configure source_url or install the model out-of-band".to_string()
    })?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create target directory {}: {error}",
                parent.display()
            )
        })?;
    }

    if let Some(source_path) = model_download_source_file_path(&source) {
        fs::copy(&source_path, &target).map_err(|error| {
            format!(
                "failed to copy model from {} to {}: {error}",
                source_path.display(),
                target.display()
            )
        })?;
        let bytes = target
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        return Ok(ModelDownloadTransferStats {
            bytes_written: bytes,
            total_bytes: Some(bytes),
            file_count: Some(1),
            manifest_path: None,
            message: format!("model copied to {}", target.display()),
        });
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("failed to build model download client: {error}"))?;
    let mut response = client
        .get(&source)
        .send()
        .map_err(|error| format!("model download request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "model download failed with HTTP {}",
            status.as_u16()
        ));
    }
    let total_bytes = response.content_length();
    let mut file = fs::File::create(&target).map_err(|error| {
        format!(
            "failed to create model target {}: {error}",
            target.display()
        )
    })?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_written = 0_u64;
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("model download stream failed: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| {
            format!("failed to write model target {}: {error}", target.display())
        })?;
        bytes_written += read as u64;
        if let Some(total) = total_bytes {
            let progress = ((bytes_written as f64 / total.max(1) as f64) * 100.0)
                .floor()
                .clamp(0.0, 99.0) as u8;
            save_model_download_checkpoint(
                store,
                &job.job_id,
                Some(progress),
                Some(bytes_written),
                Some(total),
                format!(
                    "downloading {} bytes to {}",
                    bytes_written,
                    target.display()
                ),
            );
        }
    }

    Ok(ModelDownloadTransferStats {
        bytes_written,
        total_bytes,
        file_count: Some(1),
        manifest_path: None,
        message: format!("model downloaded to {}", target.display()),
    })
}

fn run_huggingface_snapshot_download(
    job: &mut ModelDownloadJobRecord,
    store: Option<&AdminConsoleStore>,
) -> Result<ModelDownloadTransferStats, String> {
    if store
        .map(|store| model_download_job_cancel_requested(store, &job.job_id))
        .unwrap_or(false)
    {
        return Err(MODEL_DOWNLOAD_CANCELED.to_string());
    }

    let repo_id = model_download_huggingface_repo_id(job)
        .ok_or_else(|| "huggingface repo_id is required for snapshot download".to_string())?;
    let revision = model_download_metadata_string(&job.metadata, "revision")
        .unwrap_or_else(|| "main".to_string());
    let hf_endpoint = model_download_huggingface_endpoint(&job.metadata);
    let file_policy = model_download_metadata_string(&job.metadata, "file_policy")
        .unwrap_or_else(|| "runtime_snapshot".to_string());
    let allow_patterns = model_download_allow_patterns(&job.metadata);
    let target_path = job
        .target_path
        .as_deref()
        .and_then(non_empty_string)
        .unwrap_or_else(|| default_model_download_target_path(&job.model_id));
    let target = PathBuf::from(&target_path);
    let manifest_path = target.join("snapshot_manifest.json");
    if manifest_path.exists() {
        let bytes = model_path_size(&target)?;
        return Ok(ModelDownloadTransferStats {
            bytes_written: bytes,
            total_bytes: Some(bytes),
            file_count: None,
            manifest_path: Some(manifest_path.display().to_string()),
            message: format!(
                "huggingface snapshot already present at {}",
                target.display()
            ),
        });
    }

    fs::create_dir_all(&target).map_err(|error| {
        format!(
            "failed to create model snapshot directory {}: {error}",
            target.display()
        )
    })?;
    let hf_cache_dir = target.join(".hf-cache").join("hub");
    fs::create_dir_all(&hf_cache_dir).map_err(|error| {
        format!(
            "failed to create huggingface cache directory {}: {error}",
            hf_cache_dir.display()
        )
    })?;

    save_model_download_checkpoint(
        store,
        &job.job_id,
        Some(1),
        Some(0),
        None,
        format!("resolving Hugging Face snapshot {repo_id}@{revision} via {hf_endpoint}"),
    );

    let mut builder = HfApiBuilder::from_cache(HfCache::new(hf_cache_dir))
        .with_progress(false)
        .with_retries(3);
    if let Some(token) = huggingface_token_from_env() {
        builder = builder.with_token(Some(token));
    }
    builder = builder.with_endpoint(hf_endpoint.clone());
    let api = builder
        .build()
        .map_err(|error| format!("failed to initialize Hugging Face client: {error}"))?;
    let repo = api.repo(Repo::with_revision(
        repo_id.clone(),
        RepoType::Model,
        revision.clone(),
    ));
    let info = repo
        .info()
        .map_err(|error| format!("failed to read Hugging Face repo info for {repo_id}: {error}"))?;
    let resolved_sha = info.sha.clone();
    let mut files = info
        .siblings
        .into_iter()
        .map(|sibling| sibling.rfilename)
        .filter(|filename| model_snapshot_file_allowed(filename, &allow_patterns))
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        return Err(format!(
            "no downloadable files matched allow_patterns for Hugging Face repo {repo_id}"
        ));
    }

    let mut downloaded_files = Vec::new();
    let mut bytes_written = 0_u64;
    for (index, filename) in files.iter().enumerate() {
        if store
            .map(|store| model_download_job_cancel_requested(store, &job.job_id))
            .unwrap_or(false)
        {
            return Err(MODEL_DOWNLOAD_CANCELED.to_string());
        }
        let relative = safe_snapshot_relative_path(filename)?;
        let progress = HfModelDownloadProgress::new(
            store.cloned(),
            job.job_id.clone(),
            index,
            files.len(),
            bytes_written,
            filename.clone(),
        );
        let destination = target.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create snapshot file directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let cached = match repo.download_with_progress(filename, progress) {
            Ok(cached) => Some(cached),
            Err(error) => {
                let error_message = error.to_string();
                if !huggingface_download_should_fallback_to_plain_http(&error_message) {
                    return Err(format!(
                        "failed to download Hugging Face file {filename} from {repo_id}: {error_message}"
                    ));
                }
                let fallback_progress = HfModelDownloadProgress::new(
                    store.cloned(),
                    job.job_id.clone(),
                    index,
                    files.len(),
                    bytes_written,
                    filename.clone(),
                );
                download_huggingface_file_via_plain_http(
                    &hf_endpoint,
                    &repo_id,
                    &revision,
                    filename,
                    &destination,
                    fallback_progress,
                )
                .map_err(|fallback_error| {
                    format!(
                        "failed to download Hugging Face file {filename} from {repo_id}: {error_message}; plain HTTP fallback failed: {fallback_error}"
                    )
                })?;
                None
            }
        };
        if store
            .map(|store| model_download_job_cancel_requested(store, &job.job_id))
            .unwrap_or(false)
        {
            return Err(MODEL_DOWNLOAD_CANCELED.to_string());
        }
        if let Some(cached) = cached {
            fs::copy(&cached, &destination).map_err(|error| {
                format!(
                    "failed to copy Hugging Face cache file {} to {}: {error}",
                    cached.display(),
                    destination.display()
                )
            })?;
        }
        let bytes = destination
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        bytes_written += bytes;
        downloaded_files.push(json!({
            "path": filename,
            "bytes": bytes,
        }));
        let progress_percent = (((index + 1) as f64 / files.len() as f64) * 100.0)
            .floor()
            .clamp(1.0, 99.0) as u8;
        save_model_download_checkpoint(
            store,
            &job.job_id,
            Some(progress_percent),
            Some(bytes_written),
            None,
            format!(
                "downloaded {}/{} Hugging Face snapshot files",
                index + 1,
                files.len()
            ),
        );
    }

    let manifest = json!({
        "source_kind": "huggingface",
        "hf_endpoint": hf_endpoint,
        "repo_id": repo_id,
        "revision": revision,
        "resolved_sha": resolved_sha,
        "file_policy": file_policy,
        "allow_patterns": allow_patterns,
        "downloaded_at": now_unix_string(),
        "file_count": downloaded_files.len(),
        "bytes_written": bytes_written,
        "files": downloaded_files,
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("failed to serialize snapshot manifest: {error}"))?;
    fs::write(&manifest_path, manifest_bytes).map_err(|error| {
        format!(
            "failed to write snapshot manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let total_bytes = model_path_size(&target).ok();

    Ok(ModelDownloadTransferStats {
        bytes_written,
        total_bytes,
        file_count: Some(files.len()),
        manifest_path: Some(manifest_path.display().to_string()),
        message: format!(
            "huggingface snapshot downloaded to {} with manifest {}",
            target.display(),
            manifest_path.display()
        ),
    })
}

fn huggingface_download_should_fallback_to_plain_http(error_message: &str) -> bool {
    error_message.contains("Header Content-Range is missing")
}

fn download_huggingface_file_via_plain_http(
    endpoint: &str,
    repo_id: &str,
    revision: &str,
    filename: &str,
    destination: &Path,
    mut progress: HfModelDownloadProgress,
) -> Result<(), String> {
    let url = huggingface_resolve_url(endpoint, repo_id, revision, filename)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("failed to build Hugging Face fallback client: {error}"))?;
    let mut request = client
        .get(url)
        .header("User-Agent", "HarborBeacon model downloader");
    if let Some(token) = huggingface_token_from_env() {
        request = request.bearer_auth(token);
    }
    let mut response = request
        .send()
        .map_err(|error| format!("plain HTTP request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "plain HTTP request failed with HTTP {}",
            status.as_u16()
        ));
    }

    let total = response.content_length().unwrap_or(0);
    progress.init(total.try_into().unwrap_or(usize::MAX), filename);
    let partial = partial_snapshot_download_path(destination);
    let _ = fs::remove_file(&partial);
    let mut file = fs::File::create(&partial).map_err(|error| {
        format!(
            "failed to create temporary Hugging Face file {}: {error}",
            partial.display()
        )
    })?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("plain HTTP stream failed: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| {
            format!(
                "failed to write temporary Hugging Face file {}: {error}",
                partial.display()
            )
        })?;
        progress.update(read);
    }
    drop(file);
    fs::rename(&partial, destination).map_err(|error| {
        let _ = fs::remove_file(&partial);
        format!(
            "failed to move temporary Hugging Face file {} to {}: {error}",
            partial.display(),
            destination.display()
        )
    })?;
    progress.finish();
    Ok(())
}

fn huggingface_resolve_url(
    endpoint: &str,
    repo_id: &str,
    revision: &str,
    filename: &str,
) -> Result<Url, String> {
    let base = format!("{}/", endpoint.trim_end_matches('/'));
    let mut url = Url::parse(&base)
        .map_err(|error| format!("invalid Hugging Face endpoint {endpoint}: {error}"))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| format!("invalid Hugging Face endpoint {endpoint}"))?;
        for segment in repo_id.split('/') {
            segments.push(segment);
        }
        segments.push("resolve");
        segments.push(revision);
        for segment in filename.split('/') {
            segments.push(segment);
        }
    }
    Ok(url)
}

fn partial_snapshot_download_path(destination: &Path) -> PathBuf {
    let mut partial = destination.to_path_buf();
    let file_name = destination
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "download".into());
    partial.set_file_name(format!(".{file_name}.download"));
    partial
}

#[derive(Debug, Clone)]
struct HfModelDownloadProgress {
    store: Option<AdminConsoleStore>,
    job_id: String,
    file_index: usize,
    file_count: usize,
    completed_bytes: u64,
    filename: String,
    current_file_size: u64,
    current_file_bytes: u64,
    last_saved_bytes: u64,
    last_saved_percent: u8,
}

impl HfModelDownloadProgress {
    fn new(
        store: Option<AdminConsoleStore>,
        job_id: String,
        file_index: usize,
        file_count: usize,
        completed_bytes: u64,
        filename: String,
    ) -> Self {
        Self {
            store,
            job_id,
            file_index,
            file_count: file_count.max(1),
            completed_bytes,
            filename,
            current_file_size: 0,
            current_file_bytes: 0,
            last_saved_bytes: 0,
            last_saved_percent: 0,
        }
    }

    fn percent(&self) -> u8 {
        let file_fraction = if self.current_file_size > 0 {
            (self.current_file_bytes as f64 / self.current_file_size as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (((self.file_index as f64 + file_fraction) / self.file_count as f64) * 100.0)
            .floor()
            .clamp(1.0, 99.0) as u8
    }

    fn checkpoint(&mut self, force: bool) {
        let percent = self.percent();
        let bytes = self.completed_bytes + self.current_file_bytes;
        if !force
            && bytes.saturating_sub(self.last_saved_bytes) < 16 * 1024 * 1024
            && percent == self.last_saved_percent
        {
            return;
        }
        save_model_download_checkpoint(
            self.store.as_ref(),
            &self.job_id,
            Some(percent),
            Some(bytes),
            None,
            format!(
                "downloading {} ({}/{})",
                self.filename,
                self.file_index + 1,
                self.file_count
            ),
        );
        self.last_saved_bytes = bytes;
        self.last_saved_percent = percent;
    }
}

impl HfProgress for HfModelDownloadProgress {
    fn init(&mut self, size: usize, filename: &str) {
        self.current_file_size = size as u64;
        self.current_file_bytes = 0;
        self.filename = filename.to_string();
        self.checkpoint(true);
    }

    fn update(&mut self, size: usize) {
        self.current_file_bytes = self.current_file_bytes.saturating_add(size as u64);
        self.checkpoint(false);
    }

    fn finish(&mut self) {
        if self.current_file_size > 0 {
            self.current_file_bytes = self.current_file_size;
        }
        self.checkpoint(true);
    }
}

fn model_download_huggingface_repo_id(job: &ModelDownloadJobRecord) -> Option<String> {
    model_download_metadata_string(&job.metadata, "repo_id").or_else(|| {
        let source_kind = model_download_metadata_string(&job.metadata, "source_kind")
            .unwrap_or_default()
            .to_ascii_lowercase();
        (source_kind == "huggingface" && job.model_id.contains('/')).then(|| job.model_id.clone())
    })
}

fn model_download_huggingface_endpoint(metadata: &Value) -> String {
    model_download_metadata_string(metadata, "hf_endpoint")
        .or_else(|| {
            env::var("HF_ENDPOINT")
                .ok()
                .and_then(|value| non_empty_string(&value))
        })
        .unwrap_or_else(|| DEFAULT_HF_ENDPOINT.to_string())
}

fn model_download_metadata_string(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .and_then(non_empty_string)
}

fn model_download_allow_patterns(metadata: &Value) -> Vec<String> {
    let configured = metadata
        .get("allow_patterns")
        .and_then(|value| {
            if let Some(items) = value.as_array() {
                Some(
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(non_empty_string)
                        .collect::<Vec<_>>(),
                )
            } else {
                value.as_str().map(|text| {
                    text.split(',')
                        .filter_map(non_empty_string)
                        .collect::<Vec<_>>()
                })
            }
        })
        .unwrap_or_default();
    if !configured.is_empty() {
        return configured;
    }
    [
        ".gitattributes",
        "*.json",
        "*.safetensors",
        "*.model",
        "*.txt",
        "*.tiktoken",
        "*.jinja",
        "*.md",
        "tokenizer*",
        "vocab.*",
        "merges.txt",
        "*.py",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn model_snapshot_file_allowed(filename: &str, allow_patterns: &[String]) -> bool {
    if safe_snapshot_relative_path(filename).is_err() {
        return false;
    }
    let normalized = filename.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if lower.ends_with(".bin")
        || lower.ends_with(".pt")
        || lower.ends_with(".pth")
        || lower.ends_with(".ckpt")
        || lower.ends_with(".h5")
        || lower.ends_with(".onnx")
        || lower.ends_with(".pkl")
        || lower.ends_with(".pickle")
        || lower.ends_with(".msgpack")
    {
        return false;
    }
    allow_patterns
        .iter()
        .any(|pattern| wildcard_match(pattern, &normalized))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut pattern_index, mut value_index) = (0_usize, 0_usize);
    let mut star_index = None;
    let mut retry_value_index = 0_usize;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            retry_value_index = value_index;
            pattern_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            retry_value_index += 1;
            value_index = retry_value_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn safe_snapshot_relative_path(filename: &str) -> Result<PathBuf, String> {
    let trimmed = filename.trim();
    if trimmed.is_empty() {
        return Err("snapshot filename is empty".to_string());
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(format!("snapshot filename must be relative: {filename}"));
    }
    for component in path.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(format!("unsafe snapshot filename component in {filename}"));
        }
    }
    Ok(path.to_path_buf())
}

fn huggingface_token_from_env() -> Option<String> {
    ["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN"]
        .into_iter()
        .find_map(|key| {
            env::var(key)
                .ok()
                .and_then(|value| non_empty_string(&value))
        })
}

fn merge_model_download_metadata(metadata: &mut Value, patch: Value) {
    if !metadata.is_object() {
        *metadata = json!({});
    }
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    if let Some(patch_object) = patch.as_object() {
        for (key, value) in patch_object {
            if !value.is_null() {
                object.insert(key.clone(), value.clone());
            }
        }
    }
}

fn model_path_size(path: &Path) -> Result<u64, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("failed to inspect model path {}: {error}", path.display()))?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }
    let mut total = 0_u64;
    for entry in fs::read_dir(path)
        .map_err(|error| format!("failed to read model directory {}: {error}", path.display()))?
    {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read model directory entry under {}: {error}",
                path.display()
            )
        })?;
        if entry.file_name().to_string_lossy() == ".hf-cache" {
            continue;
        }
        total = total.saturating_add(model_path_size(&entry.path())?);
    }
    Ok(total)
}

fn model_download_source_url(metadata: &Value) -> Option<String> {
    metadata
        .get("source_url")
        .and_then(Value::as_str)
        .and_then(non_empty_string)
        .or_else(|| {
            metadata
                .get("url")
                .and_then(Value::as_str)
                .and_then(non_empty_string)
        })
}

fn model_download_source_file_path(source: &str) -> Option<PathBuf> {
    let path = PathBuf::from(source);
    if path.exists() {
        return Some(path);
    }
    if let Ok(url) = Url::parse(source) {
        if url.scheme() == "file" {
            return url.to_file_path().ok();
        }
        return None;
    }
    None
}

fn default_model_download_target_path(model_id: &str) -> String {
    let root = local_model_cache_roots()
        .into_iter()
        .next()
        .unwrap_or_else(|| ".harborbeacon/models".to_string());
    let slug = model_id
        .trim()
        .chars()
        .map(|ch| {
            if matches!(ch, '/' | '\\' | ':') {
                '-'
            } else {
                ch
            }
        })
        .collect::<String>()
        .to_ascii_lowercase();
    Path::new(&root)
        .join(if slug.is_empty() {
            "model"
        } else {
            slug.as_str()
        })
        .display()
        .to_string()
}

fn local_model_cache_roots() -> Vec<String> {
    let mut roots = Vec::new();
    for key in [
        "HARBOR_MODEL_CACHE_DIR",
        "HARBOR_MODEL_DIR",
        "HARBOR_MODEL_STORE_DIR",
    ] {
        if let Ok(value) = env::var(key) {
            if let Some(value) = non_empty_string(&value) {
                push_unique_root(&mut roots, value);
            }
        }
    }
    for root in [
        "/mnt/software/harborbeacon-models".to_string(),
        "/mnt/software/harborbeacon/models".to_string(),
        "/models".to_string(),
        ".harborbeacon/models".to_string(),
    ] {
        push_unique_root(&mut roots, root);
    }
    roots
}

fn push_unique_root(roots: &mut Vec<String>, root: String) {
    if !roots.iter().any(|existing| existing == &root) {
        roots.push(root);
    }
}

fn find_cached_model_path(cache_roots: &[String], model_id: &str) -> Option<String> {
    let slug = model_id.replace('/', "-").to_ascii_lowercase();
    for root in cache_roots {
        let direct = Path::new(root).join(model_id);
        if direct.exists() {
            return Some(direct.display().to_string());
        }
        let slugged = Path::new(root).join(&slug);
        if slugged.exists() {
            return Some(slugged.display().to_string());
        }
    }
    None
}

fn public_origin_port(public_origin: &str) -> Option<u16> {
    Url::parse(public_origin).ok()?.port_or_known_default()
}

fn harboros_webui_url(public_origin: &str) -> String {
    if let Ok(url) = Url::parse(public_origin) {
        if let Some(host) = url.host_str() {
            return format!("{}://{host}/ui/", url.scheme());
        }
    }
    "http://192.168.3.182/ui/".to_string()
}

fn build_rtsp_url_from_patch(
    device: &CameraDevice,
    rtsp_path: Option<&str>,
    rtsp_port: Option<u16>,
) -> Result<String, String> {
    let host = device
        .ip_address
        .clone()
        .or_else(|| rtsp_host_from_url(&device.primary_stream.url))
        .ok_or_else(|| format!("device {} does not expose an RTSP host", device.device_id))?;
    let port = rtsp_port
        .filter(|port| *port > 0)
        .or_else(|| rtsp_port_from_url(&device.primary_stream.url))
        .unwrap_or(554);
    let path = rtsp_path
        .and_then(non_empty_string)
        .or_else(|| rtsp_path_from_url(&device.primary_stream.url))
        .unwrap_or_else(|| "/stream1".to_string());
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    Ok(format!("rtsp://{host}:{port}{path}"))
}

fn redact_secret_json_value(mut value: Value) -> Value {
    redact_secret_json_value_in_place(&mut value);
    value
}

fn redact_secret_json_value_in_place(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_secret_key(key) {
                    let configured = value
                        .as_str()
                        .map(|text| !text.trim().is_empty())
                        .unwrap_or(!value.is_null());
                    *value = Value::String(String::new());
                    if configured {
                        // Keep callers able to show configured state without exposing material.
                    }
                } else {
                    redact_secret_json_value_in_place(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_secret_json_value_in_place(item);
            }
        }
        Value::String(text) => {
            *text = redact_admin_string(text);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("password")
        || normalized.contains("secret")
        || normalized.contains("token")
        || normalized.contains("api_key")
        || normalized.contains("apikey")
}

fn redact_state_snapshot(mut state: StateResponse) -> StateResponse {
    state.defaults.rtsp_password.clear();
    state.binding.session_code.clear();
    state.binding.qr_token.clear();
    state.binding.setup_url.clear();
    state.binding.static_setup_url.clear();
    state.bridge_provider = redact_bridge_provider_config(state.bridge_provider);
    for device in &mut state.devices {
        redact_camera_device_projection(device);
    }
    let Ok(mut value) = serde_json::to_value(&state) else {
        return state;
    };
    redact_value_stream_credentials(&mut value);
    serde_json::from_value::<StateResponse>(value).unwrap_or(state)
}

fn redact_camera_device_projection(device: &mut CameraDevice) {
    device.primary_stream.url = redact_stream_url_credentials(&device.primary_stream.url);
    if let Some(snapshot_url) = device.snapshot_url.as_mut() {
        *snapshot_url = redact_stream_url_credentials(snapshot_url);
    }
    if let Some(onvif_url) = device.onvif_device_service_url.as_mut() {
        *onvif_url = redact_stream_url_credentials(onvif_url);
    }
}

fn redact_camera_task_response(mut response: TaskResponse) -> TaskResponse {
    redact_value_stream_credentials(&mut response.result.data);
    for event in &mut response.result.events {
        redact_value_stream_credentials(event);
    }
    for artifact in &mut response.result.artifacts {
        if let Some(url) = artifact.url.as_mut() {
            *url = redact_stream_url_credentials(url);
        }
        redact_value_stream_credentials(&mut artifact.metadata);
    }
    response
}

fn redact_value_stream_credentials(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = redact_admin_string(text);
        }
        Value::Array(items) => {
            for item in items {
                redact_value_stream_credentials(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                redact_value_stream_credentials(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
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

    let mut request = client.get(endpoint).header("X-Contract-Version", "2.0");
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

fn probe_local_model_runtime(endpoints: &[ModelEndpoint]) -> LocalModelRuntimeProjection {
    let builtin_defaults = default_model_endpoints();
    let preferred = endpoints
        .iter()
        .find(|endpoint| is_builtin_local_openai_endpoint(endpoint))
        .cloned()
        .or_else(|| {
            builtin_defaults
                .iter()
                .find(|endpoint| is_builtin_local_openai_endpoint(endpoint))
                .cloned()
        });
    let Some(template) = preferred else {
        return LocalModelRuntimeProjection {
            error: Some("local OpenAI-compatible runtime is not configured".to_string()),
            ..Default::default()
        };
    };
    let fallback = builtin_defaults
        .iter()
        .find(|endpoint| endpoint.model_endpoint_id == template.model_endpoint_id)
        .or_else(|| {
            builtin_defaults
                .iter()
                .find(|endpoint| is_builtin_local_openai_endpoint(endpoint))
        });

    let base_url = metadata_string_value(&template.metadata, "base_url")
        .or_else(|| {
            fallback.and_then(|endpoint| metadata_string_value(&endpoint.metadata, "base_url"))
        })
        .unwrap_or_default();
    let healthz_url = metadata_string_value(&template.metadata, "healthz_url")
        .or_else(|| {
            fallback.and_then(|endpoint| metadata_string_value(&endpoint.metadata, "healthz_url"))
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| infer_healthz_url(&base_url));
    let api_key_configured = metadata_bool_value(&template.metadata, "api_key_configured")
        || metadata_string_value(&template.metadata, "api_key")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
    let api_key_configured = api_key_configured
        || fallback
            .map(|endpoint| {
                metadata_bool_value(&endpoint.metadata, "api_key_configured")
                    || metadata_string_value(&endpoint.metadata, "api_key")
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false)
            })
            .unwrap_or(false);

    if healthz_url.trim().is_empty() {
        return LocalModelRuntimeProjection {
            base_url,
            healthz_url,
            api_key_configured,
            error: Some("local model healthz URL is not configured".to_string()),
            ..Default::default()
        };
    }

    let client = match Client::builder().timeout(Duration::from_secs(3)).build() {
        Ok(client) => client,
        Err(error) => {
            return LocalModelRuntimeProjection {
                base_url,
                healthz_url,
                api_key_configured,
                error: Some(format!(
                    "failed to build local runtime probe client: {error}"
                )),
                ..Default::default()
            }
        }
    };

    let response = match client.get(&healthz_url).send() {
        Ok(response) => response,
        Err(error) => {
            return LocalModelRuntimeProjection {
                base_url,
                healthz_url,
                api_key_configured,
                error: Some(format!("local model healthz request failed: {error}")),
                ..Default::default()
            }
        }
    };
    let body = match response.text() {
        Ok(body) => body,
        Err(error) => {
            return LocalModelRuntimeProjection {
                base_url,
                healthz_url,
                api_key_configured,
                error: Some(format!(
                    "failed to read local model healthz response: {error}"
                )),
                ..Default::default()
            }
        }
    };
    let payload = match serde_json::from_str::<Value>(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return LocalModelRuntimeProjection {
                base_url,
                healthz_url,
                api_key_configured,
                error: Some(format!(
                    "local model healthz returned invalid JSON: {error}"
                )),
                ..Default::default()
            }
        }
    };

    LocalModelRuntimeProjection {
        base_url,
        healthz_url,
        api_key_configured,
        ready: payload
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        backend_ready: payload
            .get("backend")
            .and_then(|value| value.get("ready"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        backend_kind: payload
            .get("backend")
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str)
            .map(str::to_string),
        chat_model: payload
            .get("chat_model")
            .and_then(Value::as_str)
            .map(str::to_string),
        embedding_model: payload
            .get("embedding_model")
            .and_then(Value::as_str)
            .map(str::to_string),
        note: payload
            .get("note")
            .and_then(Value::as_str)
            .map(str::to_string),
        error: None,
    }
}

fn overlay_model_endpoints_with_runtime_truth(
    endpoints: &[ModelEndpoint],
    runtime: &LocalModelRuntimeProjection,
) -> Vec<ModelEndpoint> {
    let builtin_defaults = default_model_endpoints()
        .into_iter()
        .map(|endpoint| (endpoint.model_endpoint_id.clone(), endpoint))
        .collect::<HashMap<_, _>>();

    endpoints
        .iter()
        .map(|endpoint| {
            let mut overlayed = endpoint.clone();
            let original_status = overlayed.status;
            let mut projection_mismatch = false;

            if let Some(default_endpoint) = builtin_defaults.get(&overlayed.model_endpoint_id) {
                if is_builtin_local_openai_endpoint(default_endpoint) {
                    if metadata_missing_or_empty(&overlayed.metadata, "base_url") {
                        if let Some(base_url) =
                            metadata_string_value(&default_endpoint.metadata, "base_url")
                        {
                            set_metadata_string(&mut overlayed.metadata, "base_url", base_url);
                            projection_mismatch = true;
                        }
                    }
                    if metadata_missing_or_empty(&overlayed.metadata, "healthz_url") {
                        if let Some(healthz_url) =
                            metadata_string_value(&default_endpoint.metadata, "healthz_url")
                        {
                            set_metadata_string(
                                &mut overlayed.metadata,
                                "healthz_url",
                                healthz_url,
                            );
                            projection_mismatch = true;
                        }
                    }
                    if metadata_missing_or_empty(&overlayed.metadata, "api_key") {
                        if let Some(api_key) =
                            metadata_string_value(&default_endpoint.metadata, "api_key")
                        {
                            set_metadata_string(&mut overlayed.metadata, "api_key", api_key);
                            projection_mismatch = true;
                        }
                    }
                    if !metadata_bool_value(&overlayed.metadata, "api_key_configured")
                        && metadata_bool_value(&default_endpoint.metadata, "api_key_configured")
                    {
                        set_metadata_bool(&mut overlayed.metadata, "api_key_configured", true);
                        projection_mismatch = true;
                    }

                    set_metadata_string(
                        &mut overlayed.metadata,
                        "projection_source",
                        "local_runtime_overlay".to_string(),
                    );
                    set_metadata_bool(
                        &mut overlayed.metadata,
                        "runtime_ready",
                        runtime.ready && runtime.backend_ready,
                    );
                    if let Some(kind) = runtime.backend_kind.as_ref() {
                        set_metadata_string(
                            &mut overlayed.metadata,
                            "runtime_backend_kind",
                            kind.clone(),
                        );
                    }
                    if let Some(chat_model) = runtime.chat_model.as_ref() {
                        set_metadata_string(
                            &mut overlayed.metadata,
                            "runtime_chat_model",
                            chat_model.clone(),
                        );
                    }
                    if let Some(embedding_model) = runtime.embedding_model.as_ref() {
                        set_metadata_string(
                            &mut overlayed.metadata,
                            "runtime_embedding_model",
                            embedding_model.clone(),
                        );
                    }
                    if let Some(note) = runtime.note.as_ref() {
                        set_metadata_string(&mut overlayed.metadata, "runtime_note", note.clone());
                    }
                    if let Some(error) = runtime.error.as_ref() {
                        set_metadata_string(
                            &mut overlayed.metadata,
                            "runtime_error",
                            error.clone(),
                        );
                    }

                    if matches!(overlayed.model_kind, ModelKind::Llm | ModelKind::Embedder)
                        && runtime.ready
                        && runtime.backend_ready
                    {
                        overlayed.status = ModelEndpointStatus::Active;
                    }
                }
            }

            if overlayed.status != original_status {
                projection_mismatch = true;
            }
            if projection_mismatch {
                set_metadata_bool(&mut overlayed.metadata, "projection_mismatch", true);
                set_metadata_string(
                    &mut overlayed.metadata,
                    "projection_mismatch_reason",
                    "runtime truth overrode stale admin endpoint state".to_string(),
                );
            }

            overlayed
        })
        .collect()
}

fn build_feature_availability_response(
    endpoints: &[ModelEndpoint],
    route_policies: &[ModelRoutePolicy],
    account_management: &AccountManagementSnapshot,
    gateway_status: Option<&Value>,
    runtime: &LocalModelRuntimeProjection,
) -> FeatureAvailabilityResponse {
    let retrieval_group = FeatureAvailabilityGroup {
        group_id: "retrieval".to_string(),
        label: "Retrieval & Models".to_string(),
        items: vec![
            build_ocr_feature(endpoints, route_policies),
            build_embed_feature(endpoints, route_policies, runtime),
            build_answer_feature(endpoints, route_policies, runtime),
            build_vision_summary_feature(endpoints, route_policies),
        ],
    };
    let delivery_group = FeatureAvailabilityGroup {
        group_id: "delivery".to_string(),
        label: "Interaction & Delivery".to_string(),
        items: vec![
            build_interactive_reply_feature(account_management),
            build_proactive_delivery_feature(account_management, gateway_status),
        ],
    };
    let binding_group = FeatureAvailabilityGroup {
        group_id: "binding".to_string(),
        label: "Binding & Access".to_string(),
        items: vec![build_binding_availability_feature(
            account_management,
            gateway_status,
        )],
    };

    FeatureAvailabilityResponse {
        groups: vec![retrieval_group, delivery_group, binding_group],
    }
}

fn build_ocr_feature(
    endpoints: &[ModelEndpoint],
    route_policies: &[ModelRoutePolicy],
) -> FeatureAvailabilityItem {
    let policy = find_route_policy(route_policies, "retrieval.ocr");
    let endpoint = select_model_endpoint(endpoints, "ocr-local-tesseract", ModelKind::Ocr);
    let fallback_order = policy_fallback_order(policy);
    let endpoint_status = endpoint
        .map(|value| value.status.as_str().to_string())
        .unwrap_or_else(|| "missing".to_string());
    let status = match endpoint {
        Some(value) if value.status == ModelEndpointStatus::Active => "available",
        Some(_) => "degraded",
        None => "not_configured",
    };
    let blocker = if status == "available" {
        String::new()
    } else if endpoint.is_none() {
        "No OCR endpoint is configured.".to_string()
    } else {
        "OCR route is present, but the local tesseract path still needs verification.".to_string()
    };

    FeatureAvailabilityItem {
        feature_id: "retrieval.ocr".to_string(),
        label: "OCR extraction".to_string(),
        owner_lane: "harbor-framework".to_string(),
        status: status.to_string(),
        source_of_truth: "route_policy + model_endpoint".to_string(),
        current_option: endpoint
            .map(|value| format!("{} / {}", value.model_endpoint_id, value.provider_key))
            .unwrap_or_else(|| "unconfigured".to_string()),
        fallback_order,
        blocker,
        evidence: vec![
            format!("route_policy_status={}", policy_status_value(policy)),
            format!("endpoint_status={endpoint_status}"),
            format!(
                "provider={}",
                endpoint
                    .map(|value| value.provider_key.clone())
                    .unwrap_or_else(|| "none".to_string())
            ),
        ],
    }
}

fn build_embed_feature(
    endpoints: &[ModelEndpoint],
    route_policies: &[ModelRoutePolicy],
    runtime: &LocalModelRuntimeProjection,
) -> FeatureAvailabilityItem {
    let policy = find_route_policy(route_policies, "retrieval.embed");
    let endpoint = select_model_endpoint(
        endpoints,
        "embed-local-openai-compatible",
        ModelKind::Embedder,
    );
    let runtime_ready = runtime.ready && runtime.backend_ready;
    let projection_mismatch = endpoint.is_some_and(has_projection_mismatch);
    let status = if runtime_ready {
        "available"
    } else if endpoint.is_some() {
        "degraded"
    } else {
        "not_configured"
    };
    let blocker = if runtime_ready {
        String::new()
    } else if let Some(error) = runtime.error.as_ref() {
        error.clone()
    } else {
        "Local embeddings runtime is not ready.".to_string()
    };

    let mut evidence = vec![
        format!("route_policy_status={}", policy_status_value(policy)),
        format!("runtime_ready={runtime_ready}"),
    ];
    if let Some(kind) = runtime.backend_kind.as_ref() {
        evidence.push(format!("4176.backend.kind={kind}"));
    }
    if let Some(model) = runtime.embedding_model.as_ref() {
        evidence.push(format!("embedding_model={model}"));
    }
    if let Some(endpoint) = endpoint {
        evidence.push(format!(
            "endpoint={} status={}",
            endpoint.model_endpoint_id,
            endpoint.status.as_str()
        ));
        if projection_mismatch {
            evidence.push("projection_mismatch=runtime_overrode_stale_admin_state".to_string());
        }
    }

    FeatureAvailabilityItem {
        feature_id: "retrieval.embed".to_string(),
        label: "Embedding retrieval".to_string(),
        owner_lane: "harbor-framework".to_string(),
        status: status.to_string(),
        source_of_truth: "4176 /healthz + route_policy".to_string(),
        current_option: endpoint
            .map(|value| {
                format!(
                    "{} / {}",
                    value.model_endpoint_id,
                    runtime
                        .backend_kind
                        .clone()
                        .unwrap_or_else(|| value.provider_key.clone())
                )
            })
            .unwrap_or_else(|| {
                runtime
                    .backend_kind
                    .as_deref()
                    .map(|kind| format!("local runtime / {kind}"))
                    .unwrap_or_else(|| "unconfigured".to_string())
            }),
        fallback_order: policy_fallback_order(policy),
        blocker,
        evidence,
    }
}

fn build_answer_feature(
    endpoints: &[ModelEndpoint],
    route_policies: &[ModelRoutePolicy],
    runtime: &LocalModelRuntimeProjection,
) -> FeatureAvailabilityItem {
    let policy = find_route_policy(route_policies, "retrieval.answer");
    let endpoint = select_model_endpoint(endpoints, "llm-local-openai-compatible", ModelKind::Llm);
    let runtime_ready = runtime.ready && runtime.backend_ready;
    let projection_mismatch = endpoint.is_some_and(has_projection_mismatch);
    let status = if runtime_ready {
        "available"
    } else if endpoint.is_some() {
        "degraded"
    } else {
        "not_configured"
    };
    let blocker = if runtime_ready {
        String::new()
    } else if let Some(error) = runtime.error.as_ref() {
        error.clone()
    } else {
        "Local answer runtime is not ready.".to_string()
    };

    let mut evidence = vec![
        format!("route_policy_status={}", policy_status_value(policy)),
        format!("runtime_ready={runtime_ready}"),
    ];
    if let Some(kind) = runtime.backend_kind.as_ref() {
        evidence.push(format!("4176.backend.kind={kind}"));
    }
    if let Some(model) = runtime.chat_model.as_ref() {
        evidence.push(format!("chat_model={model}"));
    }
    if let Some(endpoint) = endpoint {
        evidence.push(format!(
            "endpoint={} status={}",
            endpoint.model_endpoint_id,
            endpoint.status.as_str()
        ));
        if projection_mismatch {
            evidence.push("projection_mismatch=runtime_overrode_stale_admin_state".to_string());
        }
    }

    FeatureAvailabilityItem {
        feature_id: "retrieval.answer".to_string(),
        label: "Retrieval answer synthesis".to_string(),
        owner_lane: "harbor-framework".to_string(),
        status: status.to_string(),
        source_of_truth: "4176 /healthz + route_policy".to_string(),
        current_option: endpoint
            .map(|value| {
                format!(
                    "{} / {}",
                    value.model_endpoint_id,
                    runtime
                        .backend_kind
                        .clone()
                        .unwrap_or_else(|| value.provider_key.clone())
                )
            })
            .unwrap_or_else(|| {
                runtime
                    .backend_kind
                    .as_deref()
                    .map(|kind| format!("local runtime / {kind}"))
                    .unwrap_or_else(|| "unconfigured".to_string())
            }),
        fallback_order: policy_fallback_order(policy),
        blocker,
        evidence,
    }
}

fn build_vision_summary_feature(
    endpoints: &[ModelEndpoint],
    route_policies: &[ModelRoutePolicy],
) -> FeatureAvailabilityItem {
    let policy = find_route_policy(route_policies, "retrieval.vision_summary");
    let endpoint = select_model_endpoint(endpoints, "vlm-local-openai-compatible", ModelKind::Vlm);
    let status = match endpoint {
        Some(value) if value.status == ModelEndpointStatus::Active => "available",
        Some(value) if value.status == ModelEndpointStatus::Degraded => "degraded",
        Some(_) if policy.is_some_and(|value| value.status.eq_ignore_ascii_case("degraded")) => {
            "degraded"
        }
        _ if policy.is_some_and(|value| value.status.eq_ignore_ascii_case("degraded")) => {
            "degraded"
        }
        _ => "not_configured",
    };
    let blocker = if status == "available" {
        String::new()
    } else {
        "No live VLM endpoint is enabled for still-image summary.".to_string()
    };

    FeatureAvailabilityItem {
        feature_id: "retrieval.vision_summary".to_string(),
        label: "Still-image vision summary".to_string(),
        owner_lane: "harbor-framework".to_string(),
        status: status.to_string(),
        source_of_truth: "route_policy + vlm endpoint".to_string(),
        current_option: endpoint
            .map(|value| format!("{} / {}", value.model_endpoint_id, value.provider_key))
            .unwrap_or_else(|| "unconfigured".to_string()),
        fallback_order: policy_fallback_order(policy),
        blocker,
        evidence: vec![
            format!("route_policy_status={}", policy_status_value(policy)),
            format!(
                "endpoint_status={}",
                endpoint
                    .map(|value| value.status.as_str().to_string())
                    .unwrap_or_else(|| "missing".to_string())
            ),
        ],
    }
}

fn build_interactive_reply_feature(
    account_management: &AccountManagementSnapshot,
) -> FeatureAvailabilityItem {
    let delivery_policy = &account_management.delivery_policy.interactive_reply;
    let gateway = &account_management.gateway;
    let configured = gateway.bridge_provider.configured;
    let connected = gateway.bridge_provider.connected;
    let status = if delivery_policy.eq_ignore_ascii_case("source_bound") && connected {
        "available"
    } else if delivery_policy.eq_ignore_ascii_case("source_bound") && configured {
        "degraded"
    } else if delivery_policy.trim().is_empty() {
        "not_configured"
    } else {
        "blocked"
    };
    let blocker = match status {
        "available" => String::new(),
        "degraded" => "Gateway is configured but not fully connected.".to_string(),
        "not_configured" => "Interactive reply policy is not configured.".to_string(),
        _ => format!(
            "Interactive reply must stay source_bound, but current option is {}.",
            to_non_empty_option(delivery_policy)
        ),
    };

    FeatureAvailabilityItem {
        feature_id: "interactive_reply".to_string(),
        label: "Interaction-linked reply".to_string(),
        owner_lane: "harbor-im-gateway".to_string(),
        status: status.to_string(),
        source_of_truth: "delivery_policy + gateway_status".to_string(),
        current_option: to_non_empty_option(delivery_policy),
        fallback_order: Vec::new(),
        blocker,
        evidence: vec![
            format!(
                "binding_channel={}",
                to_non_empty_option(&gateway.binding_channel)
            ),
            format!("gateway_configured={}", yes_no(configured)),
            format!("gateway_connected={}", yes_no(connected)),
        ],
    }
}

fn build_proactive_delivery_feature(
    account_management: &AccountManagementSnapshot,
    gateway_status: Option<&Value>,
) -> FeatureAvailabilityItem {
    let delivery_policy = &account_management.delivery_policy.proactive_delivery;
    let default_target = account_management
        .notification_targets
        .iter()
        .find(|target| target.is_default)
        .or_else(|| account_management.notification_targets.first());
    let gateway_blocker = gateway_platform_blocker(gateway_status, "weixin");
    let status = if gateway_blocker.is_some() {
        "blocked"
    } else if default_target.is_none() {
        "not_configured"
    } else if account_management.gateway.bridge_provider.connected {
        "available"
    } else if account_management.gateway.bridge_provider.configured {
        "degraded"
    } else {
        "not_configured"
    };
    let blocker = if let Some(blocker) = gateway_blocker.clone() {
        blocker
    } else if default_target.is_none() {
        "No default notification target is configured.".to_string()
    } else if status == "degraded" {
        "Bridge provider is configured but not yet connected for proactive delivery.".to_string()
    } else {
        String::new()
    };

    let mut evidence = vec![
        format!("delivery_policy={}", to_non_empty_option(delivery_policy)),
        format!(
            "default_target={}",
            default_target
                .map(|target| format!("{} / {}", target.label, target.route_key))
                .unwrap_or_else(|| "none".to_string())
        ),
    ];
    if let Some(record_count) = gateway_delivery_record_count(gateway_status) {
        evidence.push(format!(
            "delivery_observability.record_count={record_count}"
        ));
    }

    FeatureAvailabilityItem {
        feature_id: "proactive_delivery".to_string(),
        label: "Proactive delivery".to_string(),
        owner_lane: "harbor-im-gateway".to_string(),
        status: status.to_string(),
        source_of_truth: "delivery_policy + notification_targets + gateway_status".to_string(),
        current_option: to_non_empty_option(delivery_policy),
        fallback_order: Vec::new(),
        blocker,
        evidence,
    }
}

fn build_binding_availability_feature(
    account_management: &AccountManagementSnapshot,
    gateway_status: Option<&Value>,
) -> FeatureAvailabilityItem {
    let bindings = &account_management.identity_bindings;
    let available_count = bindings
        .iter()
        .filter(|binding| binding.binding_available)
        .count();
    let status = if bindings.is_empty() {
        "not_configured"
    } else if available_count == bindings.len() {
        "available"
    } else if available_count > 0 {
        "degraded"
    } else {
        "blocked"
    };
    let blocker = if bindings.is_empty() {
        "No HarborGate-owned identity bindings are projected yet.".to_string()
    } else {
        bindings
            .iter()
            .find(|binding| !binding.binding_available)
            .map(|binding| binding.binding_availability_note.clone())
            .or_else(|| gateway_platform_blocker(gateway_status, "weixin"))
            .unwrap_or_default()
    };

    FeatureAvailabilityItem {
        feature_id: "binding_availability".to_string(),
        label: "Binding availability".to_string(),
        owner_lane: "harbor-im-gateway".to_string(),
        status: status.to_string(),
        source_of_truth: "account_management.identity_bindings + gateway_status".to_string(),
        current_option: format!("identity_bindings={}", bindings.len()),
        fallback_order: Vec::new(),
        blocker,
        evidence: vec![
            format!("available_bindings={available_count}"),
            format!(
                "binding_surfaces={}",
                if bindings.is_empty() {
                    "none".to_string()
                } else {
                    bindings
                        .iter()
                        .map(|binding| binding.proactive_delivery_surface.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ),
        ],
    }
}

fn find_route_policy<'a>(
    route_policies: &'a [ModelRoutePolicy],
    route_policy_id: &str,
) -> Option<&'a ModelRoutePolicy> {
    route_policies
        .iter()
        .find(|policy| policy.route_policy_id == route_policy_id)
}

fn select_model_endpoint<'a>(
    endpoints: &'a [ModelEndpoint],
    preferred_id: &str,
    model_kind: ModelKind,
) -> Option<&'a ModelEndpoint> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.model_endpoint_id == preferred_id)
        .or_else(|| {
            endpoints
                .iter()
                .filter(|endpoint| endpoint.model_kind == model_kind)
                .min_by_key(|endpoint| {
                    (
                        model_endpoint_status_rank(endpoint.status),
                        endpoint.model_endpoint_id.clone(),
                    )
                })
        })
}

fn model_endpoint_status_rank(status: ModelEndpointStatus) -> usize {
    match status {
        ModelEndpointStatus::Active => 0,
        ModelEndpointStatus::Degraded => 1,
        ModelEndpointStatus::Disabled => 2,
    }
}

fn is_builtin_local_openai_endpoint(endpoint: &ModelEndpoint) -> bool {
    endpoint.endpoint_kind == ModelEndpointKind::Local
        && endpoint
            .provider_key
            .eq_ignore_ascii_case("openai_compatible")
        && matches!(
            endpoint.model_kind,
            ModelKind::Llm | ModelKind::Embedder | ModelKind::Vlm
        )
}

fn infer_healthz_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/v1") {
        format!("{prefix}/healthz")
    } else if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/healthz")
    }
}

fn metadata_missing_or_empty(metadata: &Value, key: &str) -> bool {
    metadata_string_value(metadata, key)
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
}

fn metadata_string_value(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn metadata_bool_value(metadata: &Value, key: &str) -> bool {
    metadata.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn ensure_metadata_object(metadata: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !metadata.is_object() {
        *metadata = json!({});
    }
    metadata.as_object_mut().expect("metadata object")
}

fn set_metadata_string(metadata: &mut Value, key: &str, value: String) {
    ensure_metadata_object(metadata).insert(key.to_string(), Value::String(value));
}

fn set_metadata_bool(metadata: &mut Value, key: &str, value: bool) {
    ensure_metadata_object(metadata).insert(key.to_string(), Value::Bool(value));
}

fn has_projection_mismatch(endpoint: &ModelEndpoint) -> bool {
    metadata_bool_value(&endpoint.metadata, "projection_mismatch")
}

fn policy_fallback_order(policy: Option<&ModelRoutePolicy>) -> Vec<String> {
    policy
        .map(|value| value.fallback_order.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            vec![
                "local".to_string(),
                "sidecar".to_string(),
                "cloud".to_string(),
            ]
        })
}

fn policy_status_value(policy: Option<&ModelRoutePolicy>) -> String {
    policy
        .map(|value| value.status.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "missing".to_string())
}

fn gateway_platform_blocker(payload: Option<&Value>, platform: &str) -> Option<String> {
    let from_platform_summary = payload
        .and_then(|value| value.get(platform))
        .and_then(|platform_value| {
            platform_value
                .get("blocker_category")
                .and_then(Value::as_str)
                .or_else(|| platform_value.get("blocker").and_then(Value::as_str))
                .or_else(|| platform_value.get("error").and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if from_platform_summary.is_some() {
        return from_platform_summary;
    }

    let release_v1_key = format!("{platform}_blocker_category");
    payload
        .and_then(|value| value.get("release_v1"))
        .and_then(|value| value.get(release_v1_key.as_str()))
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .and_then(|value| value.get(release_v1_key.as_str()))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn gateway_delivery_record_count(payload: Option<&Value>) -> Option<u64> {
    payload
        .and_then(|value| value.get("delivery_observability"))
        .and_then(|value| value.get("record_count"))
        .and_then(Value::as_u64)
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn to_non_empty_option(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unconfigured".to_string()
    } else {
        trimmed.to_string()
    }
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

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn now_unix_string() -> String {
    remote_view::now_unix_secs().to_string()
}

fn build_device_credential_statuses(
    state: &AdminConsoleState,
    devices: &[CameraDevice],
) -> Vec<DeviceCredentialStatusResponse> {
    devices
        .iter()
        .map(|device| build_device_credential_status(state, device))
        .collect()
}

fn build_device_credential_status(
    state: &AdminConsoleState,
    device: &CameraDevice,
) -> DeviceCredentialStatusResponse {
    let credential = state
        .device_credentials
        .iter()
        .find(|credential| credential.device_id == device.device_id);
    let fallback_configured = state.defaults.selected_camera_device_id.as_deref()
        == Some(device.device_id.as_str())
        && !state.defaults.rtsp_password.trim().is_empty();
    let platform_credential_configured = state.platform.credentials.iter().any(|credential| {
        credential.credential_id == device_rtsp_credential_id(&device.device_id)
            || credential
                .scope
                .get("device_id")
                .and_then(Value::as_str)
                .is_some_and(|value| value == device.device_id)
    });
    let configured = credential.is_some_and(|credential| !credential.password.trim().is_empty())
        || platform_credential_configured
        || fallback_configured;
    let username = credential
        .and_then(|credential| non_empty_string(&credential.username))
        .or_else(|| fallback_configured.then(|| state.defaults.rtsp_username.clone()))
        .and_then(|value| non_empty_string(&value));
    let rtsp_port = credential
        .and_then(|credential| credential.rtsp_port)
        .or_else(|| fallback_configured.then_some(state.defaults.rtsp_port))
        .or_else(|| rtsp_port_from_url(&device.primary_stream.url));
    let path_count = credential
        .map(|credential| credential.rtsp_paths.len())
        .filter(|count| *count > 0)
        .or_else(|| rtsp_path_from_url(&device.primary_stream.url).map(|_| 1))
        .unwrap_or_else(|| state.defaults.rtsp_paths.len());

    DeviceCredentialStatusResponse {
        device_id: device.device_id.clone(),
        configured,
        redacted: configured,
        username,
        rtsp_port,
        path_count,
        source: if credential.is_some() {
            "device_rtsp".to_string()
        } else if fallback_configured {
            "default_rtsp".to_string()
        } else {
            "none".to_string()
        },
        updated_at: credential.and_then(|credential| credential.updated_at.clone()),
        last_verified_at: credential.and_then(|credential| credential.last_verified_at.clone()),
    }
}

fn build_rtsp_check_evidence(
    device: &CameraDevice,
    check: &RtspCheckResponse,
    validation_id: Option<&str>,
) -> DeviceEvidenceRecord {
    let summary = if check.reachable {
        "RTSP probe reached a video stream"
    } else {
        "RTSP probe did not reach a video stream"
    };
    device_evidence_record(
        &device.device_id,
        "rtsp_check",
        if check.reachable { "passed" } else { "failed" },
        &check.checked_at,
        summary,
        json!({
            "validation_id": validation_id,
            "reachable": check.reachable,
            "stream_url": check.stream_url.clone(),
            "transport": check.transport.clone(),
            "requires_auth": check.requires_auth,
            "capabilities": check.capabilities.clone(),
            "error_message": check.error_message.clone(),
        }),
    )
}

fn build_rtsp_check_error_evidence(
    device: &CameraDevice,
    error: &str,
    observed_at: &str,
    validation_id: Option<&str>,
) -> DeviceEvidenceRecord {
    device_evidence_record(
        &device.device_id,
        "rtsp_check",
        "failed",
        observed_at,
        "RTSP probe could not run for this device",
        json!({
            "validation_id": validation_id,
            "reachable": false,
            "error_message": redact_stream_url_credentials(error),
        }),
    )
}

fn build_snapshot_check_evidence(
    device: &CameraDevice,
    response: &TaskResponse,
    validation_id: Option<&str>,
) -> DeviceEvidenceRecord {
    let redacted = redact_camera_task_response(response.clone());
    let (status, summary) = match redacted.status {
        TaskStatus::Completed => ("passed", "Snapshot capture produced metadata"),
        TaskStatus::NeedsInput => ("skipped", "Snapshot capture needs more input"),
        TaskStatus::Failed => ("failed", "Snapshot capture failed"),
    };
    let observed_at = redacted
        .result
        .data
        .pointer("/snapshot/captured_at_epoch_ms")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(now_unix_string);
    let snapshot = redacted
        .result
        .data
        .pointer("/snapshot")
        .cloned()
        .unwrap_or(Value::Null);
    let artifact_count = redacted.result.artifacts.len();
    let artifacts = serde_json::to_value(&redacted.result.artifacts).unwrap_or(Value::Null);
    let task_id = redacted.task_id.clone();
    let trace_id = redacted.trace_id.clone();
    let executor_used = redacted.executor_used.clone();
    let message = redacted.result.message.clone();
    device_evidence_record(
        &device.device_id,
        "snapshot_check",
        status,
        &observed_at,
        summary,
        json!({
            "validation_id": validation_id,
            "task_id": task_id,
            "trace_id": trace_id,
            "executor_used": executor_used,
            "message": message,
            "snapshot": snapshot,
            "artifact_count": artifact_count,
            "artifacts": artifacts,
        }),
    )
}

fn build_snapshot_skipped_evidence(
    device: &CameraDevice,
    reason: &str,
    validation_id: Option<&str>,
) -> DeviceEvidenceRecord {
    device_evidence_record(
        &device.device_id,
        "snapshot_check",
        "skipped",
        &now_unix_string(),
        "Snapshot capture skipped",
        json!({
            "validation_id": validation_id,
            "reason": reason,
        }),
    )
}

fn build_snapshot_asset_evidence(
    device: &CameraDevice,
    asset: &MediaAsset,
) -> DeviceEvidenceRecord {
    let observed_at = asset.captured_at.clone().unwrap_or_else(now_unix_string);
    DeviceEvidenceRecord {
        evidence_id: format!("media-asset-{}", asset.asset_id),
        device_id: device.device_id.clone(),
        evidence_kind: "snapshot_check".to_string(),
        status: "passed".to_string(),
        observed_at,
        summary: "Recent persisted snapshot media asset".to_string(),
        details: redact_secret_json_value(json!({
            "media_asset_id": asset.asset_id.clone(),
            "storage_uri": asset.storage_uri.clone(),
            "mime_type": asset.mime_type.clone(),
            "byte_size": asset.byte_size,
            "captured_at": asset.captured_at.clone(),
            "tags": asset.tags.clone(),
            "metadata": asset.metadata.clone(),
        })),
    }
}

fn build_share_link_evidence(summary: &ShareLinkSummary) -> DeviceEvidenceRecord {
    let evidence_kind = if summary.status == "revoked" {
        "share_link_revoke"
    } else {
        "share_link_create"
    };
    let observed_at = if summary.status == "revoked" {
        summary
            .revoked_at
            .clone()
            .or_else(|| summary.ended_at.clone())
            .or_else(|| summary.started_at.clone())
    } else {
        summary
            .started_at
            .clone()
            .or_else(|| summary.expires_at.clone())
    }
    .unwrap_or_else(now_unix_string);
    DeviceEvidenceRecord {
        evidence_id: format!("{}-{}", evidence_kind, summary.share_link_id),
        device_id: summary.device_id.clone(),
        evidence_kind: evidence_kind.to_string(),
        status: summary.status.clone(),
        observed_at,
        summary: format!("Share link is {}", summary.status),
        details: redact_secret_json_value(json!({
            "share_link_id": summary.share_link_id.clone(),
            "media_session_id": summary.media_session_id.clone(),
            "access_scope": summary.access_scope.clone(),
            "session_status": summary.session_status.clone(),
            "expires_at": summary.expires_at.clone(),
            "revoked_at": summary.revoked_at.clone(),
            "started_at": summary.started_at.clone(),
            "ended_at": summary.ended_at.clone(),
            "can_revoke": summary.can_revoke,
        })),
    }
}

fn device_evidence_record(
    device_id: &str,
    evidence_kind: &str,
    status: &str,
    observed_at: &str,
    summary: &str,
    details: Value,
) -> DeviceEvidenceRecord {
    DeviceEvidenceRecord {
        evidence_id: format!(
            "device-evidence-{}-{}",
            sanitize_id_fragment(evidence_kind),
            Uuid::new_v4().simple()
        ),
        device_id: device_id.to_string(),
        evidence_kind: evidence_kind.to_string(),
        status: status.to_string(),
        observed_at: observed_at.to_string(),
        summary: redact_stream_url_credentials(summary),
        details: redact_secret_json_value(details),
    }
}

fn redact_device_evidence_records(records: Vec<DeviceEvidenceRecord>) -> Vec<DeviceEvidenceRecord> {
    records
        .into_iter()
        .map(redact_device_evidence_record)
        .collect()
}

fn redact_device_evidence_record(mut record: DeviceEvidenceRecord) -> DeviceEvidenceRecord {
    record.summary = redact_stream_url_credentials(&record.summary);
    record.details = redact_secret_json_value(record.details);
    record
}

fn validation_status(
    rtsp_check: &DeviceEvidenceRecord,
    snapshot_check: &DeviceEvidenceRecord,
) -> String {
    match (rtsp_check.status.as_str(), snapshot_check.status.as_str()) {
        ("passed", "passed") => "passed",
        ("failed", "failed") | ("failed", "skipped") => "failed",
        _ => "degraded",
    }
    .to_string()
}

fn device_has_snapshot_path(device: &CameraDevice) -> bool {
    !device.primary_stream.url.trim().is_empty()
        || device
            .snapshot_url
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

fn sanitize_id_fragment(value: &str) -> String {
    let mut output = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        "item".to_string()
    } else {
        output
    }
}

fn rtsp_host_from_url(value: &str) -> Option<String> {
    Url::parse(value)
        .ok()
        .filter(|url| url.scheme().eq_ignore_ascii_case("rtsp"))
        .and_then(|url| url.host_str().map(str::to_string))
}

fn rtsp_port_from_url(value: &str) -> Option<u16> {
    Url::parse(value)
        .ok()
        .filter(|url| url.scheme().eq_ignore_ascii_case("rtsp"))
        .and_then(|url| url.port_or_known_default())
}

fn rtsp_path_from_url(value: &str) -> Option<String> {
    Url::parse(value)
        .ok()
        .filter(|url| url.scheme().eq_ignore_ascii_case("rtsp"))
        .map(|url| url.path().to_string())
        .filter(|path| !path.trim().is_empty() && path != "/")
}

fn redact_stream_url_credentials(value: &str) -> String {
    let parsed = Url::parse(value).ok().map(|mut url| {
        if !url.username().is_empty() || url.password().is_some() {
            let _ = url.set_username("redacted");
            let _ = url.set_password(Some("redacted"));
        }
        url.to_string()
    });
    redact_query_like_secrets(&redact_url_userinfo_occurrences(
        parsed.as_deref().unwrap_or(value),
    ))
}

fn redact_admin_string(value: &str) -> String {
    redact_url_query_secrets(&redact_stream_url_credentials(value))
}

fn redact_url_query_secrets(value: &str) -> String {
    let Ok(mut url) = Url::parse(value) else {
        return value.to_string();
    };
    let pairs = url
        .query_pairs()
        .map(|(key, value)| {
            if is_secret_key(key.as_ref()) {
                (key.to_string(), "redacted".to_string())
            } else {
                (key.to_string(), value.to_string())
            }
        })
        .collect::<Vec<_>>();
    if pairs.iter().all(|(key, value)| {
        url.query_pairs().any(|(original_key, original_value)| {
            original_key == key.as_str() && original_value == value.as_str()
        })
    }) {
        return value.to_string();
    }
    url.query_pairs_mut().clear().extend_pairs(pairs.iter());
    url.to_string()
}

fn redact_url_userinfo_occurrences(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative_scheme_end) = value[cursor..].find("://") {
        let scheme_end = cursor + relative_scheme_end;
        let scheme_start = value[..scheme_end]
            .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.'))
            .map(|index| index + 1)
            .unwrap_or(0);
        let scheme = &value[scheme_start..scheme_end];
        if !matches!(scheme, "rtsp" | "rtsps" | "http" | "https") {
            output.push_str(&value[cursor..scheme_end + 3]);
            cursor = scheme_end + 3;
            continue;
        }

        let authority_start = scheme_end + 3;
        let authority_end = value[authority_start..]
            .find(|ch: char| {
                matches!(
                    ch,
                    '/' | '?' | '#' | '"' | '\'' | '<' | '>' | ' ' | '\n' | '\r' | '\t'
                )
            })
            .map(|relative| authority_start + relative)
            .unwrap_or(value.len());
        let authority = &value[authority_start..authority_end];
        let userinfo_end = authority.rfind('@');
        let has_password = userinfo_end
            .and_then(|end| authority[..end].find(':'))
            .is_some();
        if let Some(end) = userinfo_end.filter(|_| has_password) {
            output.push_str(&value[cursor..authority_start]);
            output.push_str("redacted:redacted@");
            output.push_str(&authority[end + 1..]);
        } else {
            output.push_str(&value[cursor..authority_end]);
        }
        cursor = authority_end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn redact_query_like_secrets(value: &str) -> String {
    let mut redacted = value.to_string();
    for key in ["password", "token", "api_key", "apikey", "secret"] {
        redacted = redact_query_like_secret(&redacted, key);
    }
    redacted
}

fn redact_query_like_secret(value: &str, key: &str) -> String {
    let needle = format!("{key}=");
    let lower = value.to_ascii_lowercase();
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative_start) = lower[cursor..].find(&needle) {
        let start = cursor + relative_start;
        let value_start = start + needle.len();
        let value_end = value[value_start..]
            .find(|ch: char| matches!(ch, '&' | ' ' | '\n' | '\r' | '\t' | '"' | '\'' | '<' | '>'))
            .map(|relative| value_start + relative)
            .unwrap_or(value.len());
        output.push_str(&value[cursor..value_start]);
        output.push_str("redacted");
        cursor = value_end;
    }
    output.push_str(&value[cursor..]);
    output
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

fn parse_camera_recording_start_path(path: &str) -> Option<String> {
    parse_camera_recording_action_path(path, "/recordings/start")
}

fn parse_camera_recording_stop_path(path: &str) -> Option<String> {
    parse_camera_recording_action_path(path, "/recordings/stop")
}

fn parse_camera_recording_action_path(path: &str, suffix: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix(suffix)?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_device_credentials_path(path: &str) -> Option<String> {
    parse_device_scoped_path(path, "/credentials")
}

fn parse_device_evidence_path(path: &str) -> Option<String> {
    parse_device_scoped_path(path, "/evidence")
}

fn parse_device_validation_run_path(path: &str) -> Option<String> {
    parse_device_scoped_path(path, "/validation/run")
}

fn parse_device_rtsp_check_path(path: &str) -> Option<String> {
    parse_device_scoped_path(path, "/rtsp-check")
}

fn parse_device_credential_status_path(path: &str) -> Option<String> {
    parse_device_scoped_path(path, "/credential-status")
}

fn parse_device_metadata_patch_path(path: &str) -> Option<String> {
    let device_id = path.strip_prefix("/api/devices/")?.trim();
    if device_id.is_empty() || device_id.contains('/') {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_device_scoped_path(path: &str, suffix: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/devices/")?;
    let device_id = trimmed.strip_suffix(suffix)?;
    if device_id.is_empty() || device_id.contains('/') {
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
        build_admin_knowledge_search_request, build_device_credential_status,
        build_feature_availability_response, build_files_browse_response,
        build_harboros_im_capability_map, build_harboros_status_response,
        build_hardware_readiness_response, build_knowledge_index_job,
        build_knowledge_index_status_response, build_local_model_catalog,
        build_rag_readiness_response, build_release_readiness_response, build_rtsp_url_from_patch,
        default_model_download_target_path, default_model_endpoints, ensure_local_admin_access,
        ensure_local_camera_access, harbordesk_build_missing_response, has_forwarding_headers,
        huggingface_download_should_fallback_to_plain_http, huggingface_resolve_url,
        identity_query_suffix, is_admin_surface_path, is_harbordesk_client_route,
        is_harbordesk_surface_path, knowledge_preview_mime_supported,
        live_bridge_provider_from_setup_status, local_model_catalog_item,
        local_model_catalog_specs, mime_type_for_path, model_download_huggingface_endpoint,
        model_download_jobs_status, model_snapshot_file_allowed, normalize_unified_admin_path,
        overlay_model_endpoints_with_runtime_truth, parse_approval_decision_path,
        parse_camera_analyze_path, parse_camera_live_page_path, parse_camera_live_stream_path,
        parse_camera_recording_start_path, parse_camera_recording_stop_path,
        parse_camera_share_link_path, parse_camera_snapshot_path, parse_camera_task_snapshot_path,
        parse_device_credential_status_path, parse_device_credentials_path,
        parse_device_evidence_path, parse_device_metadata_patch_path, parse_device_rtsp_check_path,
        parse_device_validation_run_path, parse_knowledge_index_job_cancel_path,
        parse_member_default_delivery_surface_update_path, parse_member_role_update_path,
        parse_model_download_cancel_path, parse_model_download_job_path, parse_model_endpoint_path,
        parse_model_endpoint_test_path, parse_notification_target_delete_path,
        parse_share_link_revoke_path, parse_shared_camera_live_page_path,
        parse_shared_camera_live_stream_path, percent_decode_path_segment,
        probe_local_model_runtime, redact_account_management_snapshot,
        redact_bridge_provider_config, redact_camera_device_projection,
        redact_model_endpoint_response, redact_state_snapshot, redact_stream_url_credentials,
        redact_value_stream_credentials, release_item, request_identity_hints,
        resolve_harbordesk_asset_path, resolve_knowledge_preview_path, run_knowledge_index_jobs,
        run_model_download_transfer, url_encode_path_segment, AdminApi, KnowledgeSearchApiRequest,
        LocalModelRuntimeProjection, ManualAddRequest, DEFAULT_HF_ENDPOINT,
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
        AdminConsoleState, AdminConsoleStore, BridgeProviderConfig, DeviceCredentialSecret,
        DeviceEvidenceRecord, KnowledgeSettings, KnowledgeSourceRoot, ModelDownloadJobRecord,
        RemoteViewConfig,
    };
    use harborbeacon_local_agent::runtime::hub::CameraHubService;
    use harborbeacon_local_agent::runtime::knowledge_index::{
        KnowledgeIndexConfig, KnowledgeIndexService,
    };
    use harborbeacon_local_agent::runtime::registry::{CameraDevice, DeviceRegistryStore};
    use harborbeacon_local_agent::runtime::remote_view;
    use harborbeacon_local_agent::runtime::task_api::TaskApiService;
    use harborbeacon_local_agent::runtime::task_session::TaskConversationStore;
    use serde_json::json;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn hf_endpoint_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
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

        fn remove(key: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
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
            assert!(conversation_store
                .pending_approvals()
                .expect("load pending approvals")
                .is_empty());

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
        state.platform.users.push(
            harborbeacon_local_agent::control_plane::users::UserAccount {
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
            },
        );
        state.platform.memberships.push(
            harborbeacon_local_agent::control_plane::users::Membership {
                membership_id: "membership-operator-1".to_string(),
                workspace_id: "home-1".to_string(),
                user_id: "operator-1".to_string(),
                role_kind: RoleKind::Operator,
                status: MembershipStatus::Active,
                granted_by_user_id: Some("local-owner".to_string()),
                granted_at: None,
            },
        );
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
            parse_camera_recording_start_path(&format!("/api/cameras/{encoded}/recordings/start")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_recording_stop_path(&format!("/api/cameras/{encoded}/recordings/stop")),
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
    fn device_admin_paths_decode_percent_encoded_device_ids() {
        let encoded = "camera%201%2Fleft";
        assert_eq!(
            parse_device_credentials_path(&format!("/api/devices/{encoded}/credentials")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_device_rtsp_check_path(&format!("/api/devices/{encoded}/rtsp-check")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_device_credential_status_path(&format!(
                "/api/devices/{encoded}/credential-status"
            )),
            Some("camera 1/left".to_string())
        );
    }

    #[test]
    fn device_credential_status_redacts_secret_projection() {
        let mut state = AdminConsoleState::default();
        state.device_credentials.push(DeviceCredentialSecret {
            device_id: "cam-1".to_string(),
            username: "admin".to_string(),
            password: "secret".to_string(),
            rtsp_port: Some(8554),
            rtsp_paths: vec!["/stream1".to_string(), "/stream2".to_string()],
            updated_at: Some("123".to_string()),
            last_verified_at: Some("456".to_string()),
        });
        let device = CameraDevice::new("cam-1", "Living Room", "rtsp://192.168.3.73/stream1");

        let status = build_device_credential_status(&state, &device);

        assert!(status.configured);
        assert!(status.redacted);
        assert_eq!(status.username.as_deref(), Some("admin"));
        assert_eq!(status.rtsp_port, Some(8554));
        assert_eq!(status.path_count, 2);
        assert_eq!(status.source, "device_rtsp");
    }

    #[test]
    fn stream_url_redaction_removes_rtsp_credentials() {
        assert_eq!(
            redact_stream_url_credentials("rtsp://admin:secret@192.168.3.73:8554/stream1"),
            "rtsp://redacted:redacted@192.168.3.73:8554/stream1"
        );
        assert_eq!(
            redact_stream_url_credentials("rtsp://192.168.3.73/stream1"),
            "rtsp://192.168.3.73/stream1"
        );
    }

    #[test]
    fn stream_url_redaction_handles_embedded_urls() {
        assert_eq!(
            redact_stream_url_credentials(
                "primary=rtsp://admin:secret@192.168.3.73:8554/stream1 secondary=ok"
            ),
            "primary=rtsp://redacted:redacted@192.168.3.73:8554/stream1 secondary=ok"
        );
    }

    #[test]
    fn recursive_camera_task_redaction_removes_stream_credentials() {
        let mut value = json!({
            "camera_target": {
                "primary_stream": {
                    "url": "rtsp://admin:secret@192.168.3.73:8554/stream1"
                }
            },
            "share_link": {
                "url": "/shared/cameras/token"
            },
            "nested": [
                "rtsp://operator:password@camera.local/live",
                "plain text"
            ]
        });

        redact_value_stream_credentials(&mut value);

        assert_eq!(
            value["camera_target"]["primary_stream"]["url"],
            json!("rtsp://redacted:redacted@192.168.3.73:8554/stream1")
        );
        assert_eq!(value["share_link"]["url"], json!("/shared/cameras/token"));
        assert_eq!(
            value["nested"][0],
            json!("rtsp://redacted:redacted@camera.local/live")
        );
        assert_eq!(value["nested"][1], json!("plain text"));
    }

    #[test]
    fn camera_device_projection_redacts_stream_and_snapshot_urls() {
        let mut device = CameraDevice::new(
            "cam-secret",
            "Secret Camera",
            "rtsp://admin:secret@192.168.3.73:8554/stream1",
        );
        device.snapshot_url = Some("http://admin:secret@192.168.3.73/snapshot.jpg".to_string());

        redact_camera_device_projection(&mut device);

        assert_eq!(
            device.primary_stream.url,
            "rtsp://redacted:redacted@192.168.3.73:8554/stream1"
        );
        assert_eq!(
            device.snapshot_url.as_deref(),
            Some("http://redacted:redacted@192.168.3.73/snapshot.jpg")
        );
    }

    #[test]
    fn state_snapshot_redacts_device_stream_urls() {
        let registry_path = unique_store_path("harborbeacon-state-redaction-registry");
        let admin_path = unique_store_path("harborbeacon-state-redaction-state");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store.clone());
        let mut device = CameraDevice::new(
            "cam-secret",
            "Secret Camera",
            "rtsp://admin:secret@192.168.3.73:8554/stream1",
        );
        device.snapshot_url = Some("http://admin:secret@192.168.3.73/snapshot.jpg".to_string());
        registry_store
            .save_devices(&[device])
            .expect("save device registry");
        let state = CameraHubService::new(admin_store)
            .state_snapshot(Some("http://harborbeacon.local:4174"))
            .expect("state snapshot");

        let redacted = redact_state_snapshot(state);
        let payload = serde_json::to_string(&redacted).expect("serialize redacted state");

        assert!(!payload.contains("admin:secret"));
        assert!(payload.contains("rtsp://redacted:redacted@192.168.3.73:8554/stream1"));
        assert!(payload.contains("http://redacted:redacted@192.168.3.73/snapshot.jpg"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
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
        assert!(is_admin_surface_path("/api/knowledge/search"));
        assert!(is_admin_surface_path("/api/knowledge/preview"));
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
        assert!(is_admin_surface_path("/api/cameras/recording-settings"));
        assert!(is_admin_surface_path("/api/cameras/recordings/status"));
        assert!(is_admin_surface_path("/api/cameras/recordings/timeline"));
        assert!(is_admin_surface_path(
            "/api/cameras/camera-1/recordings/start"
        ));
        assert!(is_admin_surface_path(
            "/api/cameras/camera-1/recordings/stop"
        ));
        assert!(is_admin_surface_path(
            "/api/share-links/share-link-1/revoke"
        ));
        assert!(is_admin_surface_path("/api/cameras/camera-1/snapshot"));
        assert!(is_admin_surface_path("/api/cameras/camera-1/analyze"));
    }

    #[test]
    fn harbordesk_api_prefix_normalizes_to_admin_api_routes() {
        assert_eq!(
            normalize_unified_admin_path("/api/harbordesk/cameras/recording-settings"),
            "/api/cameras/recording-settings"
        );
        assert_eq!(
            normalize_unified_admin_path("/api/harbordesk"),
            "/api/state"
        );
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
        assert_eq!(
            mime_type_for_path(Path::new("C:/tmp/photo.jpeg")),
            "image/jpeg"
        );
        assert_eq!(
            mime_type_for_path(Path::new("C:/tmp/note.md")),
            "text/markdown; charset=utf-8"
        );
        assert_eq!(
            mime_type_for_path(Path::new("C:/tmp/clip.mp4")),
            "video/mp4"
        );
        assert_eq!(
            mime_type_for_path(Path::new("C:/tmp/clip.webm")),
            "video/webm"
        );
        assert!(knowledge_preview_mime_supported(Path::new(
            "C:/tmp/photo.webp"
        )));
        assert!(knowledge_preview_mime_supported(Path::new(
            "C:/tmp/clip.mp4"
        )));
        assert!(knowledge_preview_mime_supported(Path::new(
            "C:/tmp/note.txt"
        )));
        assert!(!knowledge_preview_mime_supported(Path::new(
            "C:/tmp/data.json"
        )));
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
    fn phase2_admin_paths_decode_ids() {
        let encoded_device = "camera%201%2Fleft";
        assert_eq!(
            parse_device_metadata_patch_path("/api/devices/camera%201"),
            Some("camera 1".to_string())
        );
        assert_eq!(
            parse_device_metadata_patch_path(&format!("/api/devices/{encoded_device}")),
            Some("camera 1/left".to_string())
        );

        let encoded_job = "model-download-1";
        assert_eq!(
            parse_model_download_job_path(&format!("/api/models/local-downloads/{encoded_job}")),
            Some("model-download-1".to_string())
        );
        assert_eq!(
            parse_model_download_cancel_path(&format!(
                "/api/models/local-downloads/{encoded_job}/cancel"
            )),
            Some("model-download-1".to_string())
        );
        assert_eq!(
            parse_device_evidence_path("/api/devices/camera%201%2Fleft/evidence"),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_device_validation_run_path("/api/devices/camera%201%2Fleft/validation/run"),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_device_evidence_path("/api/devices/camera/left/evidence"),
            None
        );
    }

    #[test]
    fn release_readiness_schema_groups_core_lanes() {
        let registry_path = unique_store_path("harborbeacon-readiness-registry");
        let admin_path = unique_store_path("harborbeacon-readiness-state");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let state = admin_store.load_or_create_state().expect("state");
        let account_management =
            harborbeacon_local_agent::runtime::admin_console::account_management_snapshot(
                &state,
                Some("http://192.168.3.182:4174"),
            );
        let runtime = LocalModelRuntimeProjection {
            ready: true,
            backend_ready: true,
            backend_kind: Some("candle".to_string()),
            chat_model: Some("/models/qwen".to_string()),
            embedding_model: Some("/models/jina".to_string()),
            ..Default::default()
        };
        let features = build_feature_availability_response(
            &state.models.endpoints,
            &state.models.route_policies,
            &account_management,
            None,
            &runtime,
        );
        let hardware = build_hardware_readiness_response();
        let harboros = build_harboros_status_response("http://192.168.3.182:4174");
        let rag = build_rag_readiness_response(
            &runtime,
            &state.knowledge,
            &state.models.endpoints,
            &state.knowledge_index_jobs,
        );

        let response = build_release_readiness_response(
            "http://192.168.3.182:4174",
            None,
            &account_management,
            &features,
            &hardware,
            &harboros,
            &rag,
            &runtime,
        );

        assert_eq!(response.harbor_desk.admin_port, 4174);
        assert_eq!(
            response.harbor_desk.harboros_webui,
            "http://192.168.3.182/ui/"
        );
        assert_eq!(response.status, response.overall_status);
        assert!(!response.checklist.is_empty());
        assert!(!response.status_cards.is_empty());
        for group_id in ["im", "models", "rag", "hardware", "harboros", "aiot"] {
            assert!(response
                .groups
                .iter()
                .any(|group| group.group_id == group_id));
        }
        assert!(response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .all(|item| !serde_json::to_string(item).unwrap().contains("secret")));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn harboros_im_capability_map_keeps_risky_actions_approval_gated() {
        let map = build_harboros_im_capability_map();
        let service_restart = map
            .items
            .iter()
            .find(|item| item.capability_id == "services.restart")
            .expect("service restart capability");
        assert!(!service_restart.im_ready);
        assert!(service_restart.approval_required);
        assert_eq!(service_restart.risk_level, "high");
        assert!(map
            .items
            .iter()
            .any(|item| item.capability_id == "dashboard.status" && item.im_ready));
    }

    #[test]
    fn local_model_catalog_surfaces_download_jobs_without_auto_download() {
        let job = ModelDownloadJobRecord {
            job_id: "model-download-1".to_string(),
            model_id: "qwen2.5-1.5b-instruct".to_string(),
            display_name: "Qwen2.5 1.5B Instruct".to_string(),
            provider_key: "qwen".to_string(),
            status: "queued".to_string(),
            requested_at: "1".to_string(),
            updated_at: "1".to_string(),
            target_path: None,
            progress_percent: Some(0),
            bytes_downloaded: Some(0),
            total_bytes: None,
            started_at: None,
            completed_at: None,
            error_message: None,
            message: "download request registered".to_string(),
            metadata: json!({}),
        };
        let catalog = build_local_model_catalog(vec![job.clone()]);

        assert!(catalog
            .models
            .iter()
            .any(|item| item.model_id == "qwen2.5-1.5b-instruct"));
        assert_eq!(catalog.download_jobs, vec![job]);
    }

    #[test]
    fn model_download_status_uses_latest_job_per_model() {
        let job =
            |job_id: &str, model_id: &str, status: &str, requested_at: &str, updated_at: &str| {
                ModelDownloadJobRecord {
                    job_id: job_id.to_string(),
                    model_id: model_id.to_string(),
                    display_name: model_id.to_string(),
                    provider_key: "qwen".to_string(),
                    status: status.to_string(),
                    requested_at: requested_at.to_string(),
                    updated_at: updated_at.to_string(),
                    target_path: None,
                    progress_percent: None,
                    bytes_downloaded: None,
                    total_bytes: None,
                    started_at: None,
                    completed_at: None,
                    error_message: None,
                    message: String::new(),
                    metadata: json!({}),
                }
            };

        let failed_old = job("job-1", "Qwen/Qwen3.5-4B", "failed", "1", "1");
        let completed_new = job("job-2", "Qwen/Qwen3.5-4B", "completed", "2", "2");
        assert_eq!(
            model_download_jobs_status(&[failed_old.clone(), completed_new.clone()]),
            "ready"
        );

        let running_other = job("job-3", "Qwen/Qwen3.5-9B", "running", "3", "3");
        assert_eq!(
            model_download_jobs_status(
                &[failed_old.clone(), completed_new.clone(), running_other,]
            ),
            "running"
        );

        let failed_latest = job("job-4", "Qwen/Qwen3.5-9B", "failed", "4", "4");
        assert_eq!(
            model_download_jobs_status(&[failed_old, completed_new, failed_latest]),
            "blocked"
        );
    }

    #[test]
    fn local_model_catalog_ignores_failed_huggingface_cache_only_directory() {
        let root = unique_store_path("model-cache-root").with_extension("");
        let target = root.join("qwen-qwen3.5-4b");
        fs::create_dir_all(target.join(".hf-cache")).expect("create hf cache only target");
        let job = ModelDownloadJobRecord {
            job_id: "model-download-failed".to_string(),
            model_id: "Qwen/Qwen3.5-4B".to_string(),
            display_name: "Qwen3.5 4B".to_string(),
            provider_key: "qwen".to_string(),
            status: "failed".to_string(),
            requested_at: "1".to_string(),
            updated_at: "2".to_string(),
            target_path: Some(target.display().to_string()),
            progress_percent: Some(100),
            bytes_downloaded: Some(0),
            total_bytes: None,
            started_at: Some("1".to_string()),
            completed_at: Some("2".to_string()),
            error_message: Some("dns failed".to_string()),
            message: "download failed".to_string(),
            metadata: json!({}),
        };
        let spec = local_model_catalog_specs()
            .into_iter()
            .find(|spec| spec.model_id == "Qwen/Qwen3.5-4B")
            .expect("qwen live-test catalog spec");

        let item = local_model_catalog_item(&[root.display().to_string()], &[job], spec);

        assert!(!item.installed);
        assert_eq!(item.status, "blocked");
        assert!(item.local_path.is_none());
        assert_eq!(item.size_bytes, None);
        assert!(item
            .evidence
            .iter()
            .any(|entry| entry.starts_with("ignored_incomplete_local_path=")));
        assert!(item
            .evidence
            .iter()
            .any(|entry| entry == "ignored_incomplete_size_bytes=0"));
        assert!(item
            .evidence
            .iter()
            .any(|entry| entry == "latest_download_status=failed"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_model_catalog_marks_nonempty_cache_directory_installed() {
        let root = unique_store_path("model-cache-root").with_extension("");
        let target = root.join("qwen-qwen3.5-4b");
        fs::create_dir_all(&target).expect("create model target");
        fs::write(target.join("config.json"), b"{}").expect("write model payload");
        let spec = local_model_catalog_specs()
            .into_iter()
            .find(|spec| spec.model_id == "Qwen/Qwen3.5-4B")
            .expect("qwen live-test catalog spec");

        let item = local_model_catalog_item(&[root.display().to_string()], &[], spec);

        assert!(item.installed);
        assert_eq!(item.status, "ready");
        let expected_path = target.display().to_string();
        assert_eq!(item.local_path.as_deref(), Some(expected_path.as_str()));
        assert_eq!(item.size_bytes, Some(2));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_model_catalog_surfaces_huggingface_qwen_live_test_models() {
        let catalog = build_local_model_catalog(Vec::new());

        let primary = catalog
            .models
            .iter()
            .find(|item| item.model_id == "Qwen/Qwen3.5-4B")
            .expect("primary qwen model");
        assert_eq!(primary.source_kind, "huggingface");
        assert_eq!(primary.repo_id.as_deref(), Some("Qwen/Qwen3.5-4B"));
        assert_eq!(primary.file_policy, "runtime_snapshot");
        assert!(primary
            .runtime_profiles
            .iter()
            .any(|profile| profile == "vllm-openai-compatible"));
        assert_eq!(
            primary.acceptance_note.as_deref(),
            Some("primary-live-test")
        );

        let stretch = catalog
            .models
            .iter()
            .find(|item| item.model_id == "Qwen/Qwen3.5-9B")
            .expect("stretch qwen model");
        assert_eq!(stretch.acceptance_note.as_deref(), Some("stretch-after-4b"));

        let not_today = catalog
            .models
            .iter()
            .find(|item| item.model_id == "Qwen/Qwen3.6-35B-A3B")
            .expect("not today qwen model");
        assert_eq!(
            not_today.acceptance_note.as_deref(),
            Some("not-today-acceptance")
        );

        let smolvlm = catalog
            .models
            .iter()
            .find(|item| item.model_id == "HuggingFaceTB/SmolVLM-256M-Instruct")
            .expect("vm cpu smolvlm catalog model");
        assert_eq!(smolvlm.source_kind, "huggingface");
        assert_eq!(
            smolvlm.repo_id.as_deref(),
            Some("HuggingFaceTB/SmolVLM-256M-Instruct")
        );
        assert_eq!(smolvlm.model_kind, "vlm");
        assert_eq!(smolvlm.file_policy, "runtime_snapshot");
        assert!(smolvlm
            .runtime_profiles
            .iter()
            .any(|profile| profile == "cpu-vlm-sidecar"));
        assert_eq!(smolvlm.acceptance_note.as_deref(), Some("vm-cpu-photo-rag"));
    }

    #[test]
    fn huggingface_snapshot_allow_patterns_keep_runtime_files_only() {
        let patterns = vec![
            "*.json".to_string(),
            "*.safetensors".to_string(),
            "tokenizer*".to_string(),
        ];

        assert!(model_snapshot_file_allowed("config.json", &patterns));
        assert!(model_snapshot_file_allowed(
            "model-00001-of-00002.safetensors",
            &patterns
        ));
        assert!(model_snapshot_file_allowed("tokenizer.model", &patterns));
        assert!(!model_snapshot_file_allowed("pytorch_model.bin", &patterns));
        assert!(!model_snapshot_file_allowed("../escape.json", &patterns));
    }

    #[test]
    fn huggingface_plain_http_fallback_handles_mirror_content_range_gap() {
        assert!(huggingface_download_should_fallback_to_plain_http(
            "Header Content-Range is missing"
        ));
        assert!(!huggingface_download_should_fallback_to_plain_http(
            "HTTP status client error"
        ));
    }

    #[test]
    fn huggingface_resolve_url_uses_endpoint_repo_revision_and_file_path() {
        let url = huggingface_resolve_url(
            "https://hf-mirror.com/",
            "Qwen/Qwen3.5-4B",
            "main",
            "nested/config.json",
        )
        .expect("resolve url");

        assert_eq!(
            url.as_str(),
            "https://hf-mirror.com/Qwen/Qwen3.5-4B/resolve/main/nested/config.json"
        );
    }

    #[test]
    fn huggingface_endpoint_prefers_job_metadata_then_env_then_default_mirror() {
        let _guard = hf_endpoint_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let env_guard = EnvGuard::set("HF_ENDPOINT", "https://env-mirror.example");

        assert_eq!(
            model_download_huggingface_endpoint(&json!({
                "hf_endpoint": "https://user-mirror.example"
            })),
            "https://user-mirror.example"
        );
        assert_eq!(
            model_download_huggingface_endpoint(&json!({})),
            "https://env-mirror.example"
        );

        drop(env_guard);
        let remove_guard = EnvGuard::remove("HF_ENDPOINT");
        assert_eq!(
            model_download_huggingface_endpoint(&json!({})),
            DEFAULT_HF_ENDPOINT
        );
        drop(remove_guard);
    }

    #[test]
    fn default_model_download_target_path_is_snapshot_directory() {
        let target = default_model_download_target_path("Qwen/Qwen3.5-4B");

        assert!(target.ends_with("qwen-qwen3.5-4b"));
        assert!(!target.ends_with("model.bin"));
    }

    #[test]
    fn model_download_transfer_copies_explicit_source_to_target() {
        let source_path = unique_store_path("harborbeacon-model-source");
        let target_path = unique_store_path("harborbeacon-model-target");
        fs::write(&source_path, b"model-bytes").expect("write source");
        let _ = fs::remove_file(&target_path);

        let job = harborbeacon_local_agent::runtime::admin_console::ModelDownloadJobRecord {
            job_id: "model-download-copy".to_string(),
            model_id: "demo-model".to_string(),
            display_name: "Demo model".to_string(),
            provider_key: "local".to_string(),
            status: "running".to_string(),
            requested_at: "1".to_string(),
            updated_at: "1".to_string(),
            target_path: Some(target_path.display().to_string()),
            progress_percent: Some(0),
            bytes_downloaded: Some(0),
            total_bytes: None,
            started_at: Some("1".to_string()),
            completed_at: None,
            error_message: None,
            message: String::new(),
            metadata: json!({
                "source_url": source_path.display().to_string(),
                "api_key": "should-not-be-used"
            }),
        };

        let stats = run_model_download_transfer(&job).expect("download transfer");

        assert_eq!(stats.bytes_written, 11);
        assert_eq!(fs::read(&target_path).expect("read target"), b"model-bytes");
        let _ = fs::remove_file(source_path);
        let _ = fs::remove_file(target_path);
    }

    #[test]
    fn rag_readiness_schema_reports_required_release_fields() {
        let index_root = std::env::temp_dir().join("harborbeacon-rag-index-test");
        let source_root = std::env::temp_dir().join("harborbeacon-rag-source-test");
        fs::create_dir_all(&source_root).expect("create rag source root");
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "test-root".to_string(),
                label: "Test root".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: None,
            }],
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let response = build_rag_readiness_response(
            &LocalModelRuntimeProjection {
                ready: true,
                backend_ready: true,
                embedding_model: Some("jina".to_string()),
                ..Default::default()
            },
            &settings,
            &default_model_endpoints(),
            &[],
        );

        assert!(!response.generated_at.is_empty());
        assert!(!response.status.is_empty());
        assert_eq!(response.source_roots.status, "ready");
        assert!(response
            .evidence
            .iter()
            .any(|entry| entry.contains("embedding_model=jina")));
        let _ = fs::remove_dir_all(source_root);
    }

    #[test]
    fn rag_readiness_top_level_status_includes_degraded_model_cards() {
        let source_root = unique_store_path("harborbeacon-rag-source-model-blocker");
        let index_root = unique_store_path("harborbeacon-rag-index-model-blocker");
        fs::create_dir_all(&source_root).expect("create rag source root");
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "test-root".to_string(),
                label: "Test root".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: None,
            }],
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        };

        let response = build_rag_readiness_response(
            &LocalModelRuntimeProjection {
                ready: true,
                backend_ready: true,
                embedding_model: Some("jina".to_string()),
                ..Default::default()
            },
            &settings,
            &[],
            &[],
        );

        assert_ne!(response.status, "ready");
        assert!(response
            .blockers
            .iter()
            .any(|item| item.contains("OCR endpoint is not configured")));
        assert!(response
            .model_readiness
            .iter()
            .any(|card| card.label == "OCR" && card.status == "needs-config"));
        let _ = fs::remove_dir_all(source_root);
    }

    #[test]
    fn rag_readiness_image_capability_is_not_ready_without_vlm_content_index() {
        let source_root = unique_store_path("harborbeacon-rag-source-image-content");
        let index_root = unique_store_path("harborbeacon-rag-index-image-content");
        fs::create_dir_all(&source_root).expect("create rag source root");
        fs::create_dir_all(&index_root).expect("create rag index root");
        fs::write(
            index_root.join("root-a.json"),
            serde_json::to_string(&json!({
                "schema_version": 1,
                "root": source_root.to_string_lossy(),
                "root_signature": {
                    "modified_unix_millis": 0,
                    "size_bytes": 0
                },
                "generated_at": "200",
                "directories": [],
                "entries": [{
                    "modality": "image",
                    "path": source_root.join("spring.jpg").to_string_lossy(),
                    "title": "spring.jpg",
                    "searchable_text": "user sidecar only",
                    "text_sources": [{
                        "source_kind": "sidecar",
                        "text": "user sidecar only"
                    }],
                    "file_signature": {
                        "modified_unix_millis": 0,
                        "size_bytes": 9
                    }
                }]
            }))
            .expect("serialize manifest"),
        )
        .expect("write manifest");
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "test-root".to_string(),
                label: "Test root".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: None,
            }],
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let endpoint =
            |model_kind: ModelKind, model_endpoint_id: &str, model_name: &str| ModelEndpoint {
                model_endpoint_id: model_endpoint_id.to_string(),
                workspace_id: Some("home-1".to_string()),
                provider_account_id: None,
                model_kind,
                endpoint_kind: ModelEndpointKind::Local,
                provider_key: "openai_compatible".to_string(),
                model_name: model_name.to_string(),
                capability_tags: Vec::new(),
                cost_policy: json!({}),
                status: ModelEndpointStatus::Active,
                metadata: json!({}),
            };
        let response = build_rag_readiness_response(
            &LocalModelRuntimeProjection {
                ready: true,
                backend_ready: true,
                embedding_model: Some("jina".to_string()),
                ..Default::default()
            },
            &settings,
            &[
                endpoint(ModelKind::Embedder, "embedder-local", "jina"),
                endpoint(ModelKind::Vlm, "vlm-local", "cpu-vlm"),
                endpoint(ModelKind::Ocr, "ocr-local", "tesseract"),
            ],
            &[],
        );

        let image_rag = response
            .capability_profiles
            .iter()
            .find(|profile| profile.capability_id == "image_rag")
            .expect("image rag profile");
        assert_eq!(image_rag.status, "degraded");
        assert!(image_rag
            .evidence
            .iter()
            .any(|entry| entry == "vlm_indexed_image_count=0"));
        assert!(response
            .warnings
            .iter()
            .any(|warning| warning.contains("Image RAG is degraded")));

        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(index_root);
    }

    #[test]
    fn knowledge_index_status_counts_manifest_cache_and_storage() {
        let index_root = unique_store_path("harborbeacon-knowledge-index-status");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(
            index_root.join("root-a.json"),
            serde_json::to_string(&json!({
                "schema_version": 1,
                "root": "/tmp/root-a",
                "root_signature": {
                    "modified_unix_millis": 0,
                    "size_bytes": 0
                },
                "generated_at": "200",
                "directories": [],
                "entries": [{
                    "modality": "document",
                    "path": "/tmp/root-a/doc.md",
                    "title": "doc.md",
                    "searchable_text": "hello",
                    "file_signature": {
                        "modified_unix_millis": 0,
                        "size_bytes": 5
                    }
                }, {
                    "modality": "image",
                    "path": "/tmp/root-a/spring.jpg",
                    "title": "spring.jpg",
                    "searchable_text": "春天的公园",
                    "text_sources": [{
                        "source_kind": "vlm",
                        "provider_key": "mock-vlm",
                        "text": "春天的公园"
                    }],
                    "file_signature": {
                        "modified_unix_millis": 0,
                        "size_bytes": 9
                    }
                }, {
                    "modality": "image",
                    "path": "/tmp/root-a/no-content.jpg",
                    "title": "no-content.jpg",
                    "searchable_text": "",
                    "file_signature": {
                        "modified_unix_millis": 0,
                        "size_bytes": 9
                    }
                }]
            }))
            .expect("serialize manifest"),
        )
        .expect("write manifest");
        fs::write(
            index_root.join("root-a.embeddings.json"),
            serde_json::to_string(&json!({
                "schema_version": 1,
                "root": "/tmp/root-a",
                "entries": [{
                    "key": "chunk-1",
                    "path": "/tmp/root-a/doc.md",
                    "text_hash": "abc",
                    "vector": [0.1, 0.2]
                }]
            }))
            .expect("serialize embedding store"),
        )
        .expect("write embedding store");

        let response = build_knowledge_index_status_response(KnowledgeSettings {
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        });

        assert_eq!(response.manifest_count, 1);
        assert_eq!(response.manifest_entry_count, 3);
        assert_eq!(response.document_count, 1);
        assert_eq!(response.image_count, 2);
        assert_eq!(response.content_indexed_image_count, 1);
        assert_eq!(response.vlm_indexed_image_count, 1);
        assert_eq!(response.ocr_indexed_image_count, 0);
        assert_eq!(response.image_content_missing_count, 1);
        assert_eq!(response.image_text_source_counts.get("vlm"), Some(&1));
        assert_eq!(response.embedding_cache_count, 1);
        assert_eq!(response.embedding_entry_count, 1);
        assert!(response.storage_usage_bytes > 0);
        assert_eq!(response.last_indexed_at.as_deref(), Some("200"));
        let _ = fs::remove_dir_all(index_root);
    }

    #[test]
    fn admin_knowledge_search_request_uses_enabled_roots_and_clamps_limit() {
        let settings = KnowledgeSettings {
            source_roots: vec![
                KnowledgeSourceRoot {
                    root_id: "enabled".to_string(),
                    label: "Enabled".to_string(),
                    path: "/mnt/source-a".to_string(),
                    enabled: true,
                    include: Vec::new(),
                    exclude: Vec::new(),
                    last_indexed_at: None,
                },
                KnowledgeSourceRoot {
                    root_id: "disabled".to_string(),
                    label: "Disabled".to_string(),
                    path: "/mnt/source-b".to_string(),
                    enabled: false,
                    include: Vec::new(),
                    exclude: Vec::new(),
                    last_indexed_at: None,
                },
            ],
            index_root: "/mnt/index".to_string(),
            ..Default::default()
        };

        let request = build_admin_knowledge_search_request(
            KnowledgeSearchApiRequest {
                query: " 找到春天照片 ".to_string(),
                limit: Some(500),
                include_documents: Some(false),
                include_images: None,
                include_videos: Some(true),
            },
            &settings,
        )
        .expect("search request");

        assert_eq!(request.query, "找到春天照片");
        assert_eq!(request.limit, 50);
        assert_eq!(request.configured_roots, vec!["/mnt/source-a".to_string()]);
        assert_eq!(request.roots, vec!["/mnt/source-a".to_string()]);
        assert!(!request.include_documents);
        assert!(request.include_images);
        assert!(request.include_videos);
        assert_eq!(request.index_root.as_deref(), Some("/mnt/index"));
    }

    #[test]
    fn knowledge_preview_allows_indexed_files_under_enabled_source_root() {
        let source_root = unique_store_path("harborbot-preview-source");
        let index_root = unique_store_path("harborbot-preview-index");
        fs::create_dir_all(&source_root).expect("create source root");
        fs::create_dir_all(&index_root).expect("create index root");
        let indexed_path = source_root.join("indexed.md");
        fs::write(&indexed_path, "HarborBot indexed preview").expect("write indexed file");
        let service = KnowledgeIndexService::from_config(
            KnowledgeIndexConfig::new(index_root.clone()).unwrap(),
        )
        .expect("index service");
        service
            .load_or_refresh(&source_root)
            .expect("write index manifest");
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "source".to_string(),
                label: "Source".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: Some("1".to_string()),
            }],
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        };

        let resolved = resolve_knowledge_preview_path(&indexed_path.to_string_lossy(), &settings)
            .expect("preview path");

        assert_eq!(
            resolved,
            indexed_path.canonicalize().expect("canonical path")
        );
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(index_root);
    }

    #[test]
    fn knowledge_preview_rejects_outside_unindexed_directory_and_unsupported_files() {
        let source_root = unique_store_path("harborbot-preview-guard-source");
        let outside_root = unique_store_path("harborbot-preview-guard-outside");
        let index_root = unique_store_path("harborbot-preview-guard-index");
        fs::create_dir_all(&source_root).expect("create source root");
        fs::create_dir_all(&outside_root).expect("create outside root");
        fs::create_dir_all(&index_root).expect("create index root");
        let indexed_path = source_root.join("indexed.md");
        fs::write(&indexed_path, "indexed").expect("write indexed file");
        let service = KnowledgeIndexService::from_config(
            KnowledgeIndexConfig::new(index_root.clone()).unwrap(),
        )
        .expect("index service");
        service
            .load_or_refresh(&source_root)
            .expect("write index manifest");
        let unindexed_path = source_root.join("unindexed.md");
        fs::write(&unindexed_path, "not indexed").expect("write unindexed file");
        let unsupported_path = source_root.join("payload.bin");
        fs::write(&unsupported_path, "not previewable").expect("write unsupported file");
        let outside_path = outside_root.join("outside.md");
        fs::write(&outside_path, "outside").expect("write outside file");
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "source".to_string(),
                label: "Source".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: Some("1".to_string()),
            }],
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        };

        let outside_error =
            resolve_knowledge_preview_path(&outside_path.to_string_lossy(), &settings)
                .expect_err("outside root rejected");
        assert_eq!(outside_error.status, StatusCode(403));

        let unindexed_error =
            resolve_knowledge_preview_path(&unindexed_path.to_string_lossy(), &settings)
                .expect_err("unindexed path rejected");
        assert_eq!(unindexed_error.status, StatusCode(404));

        let directory_error =
            resolve_knowledge_preview_path(&source_root.to_string_lossy(), &settings)
                .expect_err("directory rejected");
        assert_eq!(directory_error.status, StatusCode(400));

        let unsupported_error =
            resolve_knowledge_preview_path(&unsupported_path.to_string_lossy(), &settings)
                .expect_err("unsupported mime rejected");
        assert_eq!(unsupported_error.status, StatusCode(415));

        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(outside_root);
        let _ = fs::remove_dir_all(index_root);
    }

    #[test]
    fn parse_knowledge_index_job_cancel_route_extracts_job_id() {
        assert_eq!(
            parse_knowledge_index_job_cancel_path("/api/knowledge/index/jobs/job-123/cancel"),
            Some("job-123".to_string())
        );
        assert_eq!(
            parse_knowledge_index_job_cancel_path("/api/knowledge/index/jobs//cancel"),
            None
        );
        assert_eq!(
            parse_knowledge_index_job_cancel_path("/api/knowledge/index/jobs/job-123"),
            None
        );
    }

    #[test]
    fn knowledge_index_worker_completes_job_and_updates_source_root() {
        let admin_path = unique_store_path("harborbeacon-knowledge-index-worker-admin");
        let registry_path = unique_store_path("harborbeacon-knowledge-index-worker-registry");
        let source_root = unique_store_path("harborbeacon-knowledge-index-worker-root");
        let index_root = unique_store_path("harborbeacon-knowledge-index-worker-index");
        fs::create_dir_all(&source_root).expect("create source root");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(source_root.join("note.md"), "worker indexed note").expect("write source doc");

        let store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "worker-root".to_string(),
                label: "Worker Root".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: None,
            }],
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        };
        store
            .save_knowledge_settings(settings.clone())
            .expect("save settings");
        let job = build_knowledge_index_job(
            &settings.source_roots[0],
            "100",
            settings.default_resource_profile,
        );
        store
            .save_knowledge_index_job(job.clone())
            .expect("save job");

        let worker_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        run_knowledge_index_jobs(worker_store, settings, vec![job]);

        let jobs = store
            .list_knowledge_index_jobs()
            .expect("list knowledge index jobs");
        assert_eq!(jobs[0].status, "completed");
        assert_eq!(jobs[0].progress_percent, Some(100));
        assert_eq!(jobs[0].checkpoint["phase"], "completed");
        let updated_settings = store.knowledge_settings().expect("load settings");
        assert!(updated_settings.source_roots[0].last_indexed_at.is_some());
        assert!(index_root
            .read_dir()
            .expect("list index root")
            .flatten()
            .any(
                |entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            ));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(index_root);
    }

    #[test]
    fn knowledge_index_worker_honors_queued_cancel() {
        let admin_path = unique_store_path("harborbeacon-knowledge-index-cancel-admin");
        let registry_path = unique_store_path("harborbeacon-knowledge-index-cancel-registry");
        let source_root = unique_store_path("harborbeacon-knowledge-index-cancel-root");
        let index_root = unique_store_path("harborbeacon-knowledge-index-cancel-index");
        fs::create_dir_all(&source_root).expect("create source root");
        fs::create_dir_all(&index_root).expect("create index root");
        fs::write(source_root.join("note.md"), "canceled note").expect("write source doc");

        let store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "cancel-root".to_string(),
                label: "Cancel Root".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: None,
            }],
            index_root: index_root.to_string_lossy().into_owned(),
            ..Default::default()
        };
        store
            .save_knowledge_settings(settings.clone())
            .expect("save settings");
        let job = build_knowledge_index_job(
            &settings.source_roots[0],
            "100",
            settings.default_resource_profile,
        );
        store
            .save_knowledge_index_job(job.clone())
            .expect("save job");
        store
            .cancel_knowledge_index_job(&job.job_id, "101".to_string())
            .expect("cancel job");

        let worker_store = AdminConsoleStore::new(
            admin_path.clone(),
            DeviceRegistryStore::new(registry_path.clone()),
        );
        run_knowledge_index_jobs(worker_store, settings, vec![job]);

        let jobs = store
            .list_knowledge_index_jobs()
            .expect("list knowledge index jobs");
        assert_eq!(jobs[0].status, "canceled");
        assert_eq!(jobs[0].checkpoint["phase"], "canceled_before_start");
        let updated_settings = store.knowledge_settings().expect("load settings");
        assert!(updated_settings.source_roots[0].last_indexed_at.is_none());

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(index_root);
    }

    #[test]
    fn files_browse_lists_configured_root_without_writes() {
        let source_root = std::env::temp_dir().join(format!(
            "harborbeacon-files-browse-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let nested = source_root.join("MM-test");
        fs::create_dir_all(&nested).expect("create nested directory");
        fs::write(source_root.join("note.txt"), "sample").expect("write sample file");
        let settings = KnowledgeSettings {
            source_roots: vec![KnowledgeSourceRoot {
                root_id: "sample".to_string(),
                label: "Sample".to_string(),
                path: source_root.to_string_lossy().into_owned(),
                enabled: true,
                include: Vec::new(),
                exclude: Vec::new(),
                last_indexed_at: None,
            }],
            index_root: source_root
                .join("index-root")
                .to_string_lossy()
                .into_owned(),
            ..Default::default()
        };

        let response = build_files_browse_response(Some(&source_root.to_string_lossy()), &settings)
            .expect("browse configured root");

        assert!(response.readonly);
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "MM-test" && entry.is_dir));
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "note.txt" && !entry.is_dir));

        let outside = std::env::temp_dir().join("harborbeacon-outside-browse");
        let denied = build_files_browse_response(Some(&outside.to_string_lossy()), &settings);
        assert!(denied.is_err());

        let _ = fs::remove_dir_all(source_root);
    }

    #[test]
    fn readiness_evidence_records_redact_stream_credentials() {
        let item = release_item(
            "camera-check",
            "Camera check",
            "harbor-aiot",
            "blocked",
            "blocked",
            "rtsp://admin:secret@192.168.1.20:554/stream1 failed",
            "POST /api/devices/cam/rtsp-check",
            "/devices-aiot",
            vec!["stream=rtsp://admin:secret@192.168.1.20:554/stream1".to_string()],
        );
        let text = serde_json::to_string(&item).expect("json");

        assert!(!text.contains("admin:secret"));
        assert!(!text.contains("rtsp://admin"));
        assert!(text.contains("redacted"));
    }

    #[test]
    fn rtsp_metadata_patch_url_does_not_inject_credentials() {
        let mut device = CameraDevice::new(
            "cam-1",
            "Front Door",
            "rtsp://admin:secret@192.168.1.10:554/old",
        );
        device.ip_address = Some("192.168.1.10".to_string());

        let url =
            build_rtsp_url_from_patch(&device, Some("stream1"), Some(8554)).expect("rtsp url");

        assert_eq!(url, "rtsp://192.168.1.10:8554/stream1");
        assert!(!url.contains("secret"));
        assert!(!url.contains("admin:"));
    }

    #[test]
    fn device_evidence_response_exposes_recent_checks_without_secrets() {
        let registry_path = unique_store_path("harborbeacon-device-evidence-registry");
        let admin_path = unique_store_path("harborbeacon-device-evidence-state");
        let conversation_path = unique_store_path("harborbeacon-device-evidence-conversations");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store.clone());
        let conversation_store = TaskConversationStore::new(conversation_path.clone());

        let mut device = CameraDevice::new(
            "cam-secret",
            "Secret Camera",
            "rtsp://admin:secret@192.168.1.10:554/stream1",
        );
        device.snapshot_url =
            Some("http://admin:secret@192.168.1.10/snapshot.jpg?token=abc".to_string());
        registry_store
            .save_devices(&[device.clone()])
            .expect("save device");
        admin_store
            .save_device_credential(DeviceCredentialSecret {
                device_id: "cam-secret".to_string(),
                username: "admin".to_string(),
                password: "secret".to_string(),
                rtsp_port: Some(554),
                rtsp_paths: vec!["/stream1".to_string()],
                updated_at: Some("100".to_string()),
                last_verified_at: Some("101".to_string()),
            })
            .expect("save credential");
        admin_store
            .record_device_evidence(DeviceEvidenceRecord {
                evidence_id: "rtsp-evidence".to_string(),
                device_id: "cam-secret".to_string(),
                evidence_kind: "rtsp_check".to_string(),
                status: "passed".to_string(),
                observed_at: "200".to_string(),
                summary: "rtsp://admin:secret@192.168.1.10:554/stream1 ok".to_string(),
                details: json!({
                    "stream_url": "rtsp://admin:secret@192.168.1.10:554/stream1",
                    "api_token": "raw-token"
                }),
            })
            .expect("record rtsp evidence");
        admin_store
            .record_device_evidence(DeviceEvidenceRecord {
                evidence_id: "snapshot-evidence".to_string(),
                device_id: "cam-secret".to_string(),
                evidence_kind: "snapshot_check".to_string(),
                status: "passed".to_string(),
                observed_at: "201".to_string(),
                summary: "snapshot ok".to_string(),
                details: json!({
                    "snapshot_url": "http://admin:secret@192.168.1.10/snapshot.jpg?token=abc"
                }),
            })
            .expect("record snapshot evidence");

        conversation_store
            .save_share_link_bundle(
                &MediaSession {
                    media_session_id: "media-session-active".to_string(),
                    device_id: "cam-secret".to_string(),
                    stream_profile_id: "stream-cam-secret-primary".to_string(),
                    session_kind: MediaSessionKind::Share,
                    delivery_mode: MediaDeliveryMode::Hls,
                    opened_by_user_id: Some("local-owner".to_string()),
                    status: MediaSessionStatus::Active,
                    share_link_id: Some("share-link-active".to_string()),
                    started_at: Some("202".to_string()),
                    ended_at: None,
                    metadata: json!({
                        "stream_url": "rtsp://admin:secret@192.168.1.10:554/stream1"
                    }),
                },
                &ShareLink {
                    share_link_id: "share-link-active".to_string(),
                    media_session_id: "media-session-active".to_string(),
                    token_hash: "token-hash".to_string(),
                    access_scope: ShareAccessScope::PublicLink,
                    expires_at: Some("9999999999".to_string()),
                    revoked_at: None,
                },
            )
            .expect("save share link");

        let task_service = TaskApiService::new(admin_store.clone(), conversation_store);
        let api = AdminApi::new(
            admin_store,
            task_service,
            PathBuf::from("frontend/harbordesk/dist/harbordesk"),
            "http://harborbeacon.local:4174".to_string(),
        );
        let response = api
            .build_device_evidence_response(&device)
            .expect("evidence response");
        let payload = serde_json::to_string(&response).expect("serialize response");

        assert!(response.credential_status.configured);
        assert!(response.credential_status.redacted);
        assert_eq!(
            response
                .recent_rtsp_check
                .as_ref()
                .map(|record| record.status.as_str()),
            Some("passed")
        );
        assert_eq!(
            response
                .recent_snapshot_check
                .as_ref()
                .map(|record| record.status.as_str()),
            Some("passed")
        );
        assert_eq!(response.share_links.len(), 1);
        assert_eq!(response.share_links[0].status, "active");
        assert!(!payload.contains("admin:secret"));
        assert!(!payload.contains("raw-token"));
        assert!(!payload.contains("token=abc"));
        assert!(payload.contains("redacted:redacted@192.168.1.10"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_file(conversation_path);
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
    fn runtime_overlay_promotes_live_local_llm_and_embedder_rows() {
        let mut endpoints =
            harborbeacon_local_agent::runtime::admin_console::default_model_endpoints();
        for endpoint in &mut endpoints {
            if matches!(
                endpoint.model_kind,
                ModelKind::Llm | ModelKind::Embedder | ModelKind::Vlm
            ) {
                endpoint.status = ModelEndpointStatus::Disabled;
                endpoint.metadata = json!({
                    "builtin": true,
                    "base_url": "",
                    "healthz_url": "",
                    "api_key": "",
                    "api_key_configured": false,
                });
            }
        }

        let overlayed = overlay_model_endpoints_with_runtime_truth(
            &endpoints,
            &LocalModelRuntimeProjection {
                ready: true,
                backend_ready: true,
                backend_kind: Some("candle".to_string()),
                chat_model: Some("/models/qwen".to_string()),
                embedding_model: Some("/models/jina".to_string()),
                ..Default::default()
            },
        );

        let llm = overlayed
            .iter()
            .find(|endpoint| endpoint.model_endpoint_id == "llm-local-openai-compatible")
            .expect("llm endpoint");
        assert_eq!(llm.status, ModelEndpointStatus::Active);
        assert_eq!(llm.metadata["projection_mismatch"], json!(true));
        assert_ne!(llm.metadata["base_url"], json!(""));
        assert_eq!(llm.metadata["runtime_backend_kind"], json!("candle"));

        let embedder = overlayed
            .iter()
            .find(|endpoint| endpoint.model_endpoint_id == "embed-local-openai-compatible")
            .expect("embedder endpoint");
        assert_eq!(embedder.status, ModelEndpointStatus::Active);
        assert_eq!(embedder.metadata["api_key_configured"], json!(true));

        let vlm = overlayed
            .iter()
            .find(|endpoint| endpoint.model_endpoint_id == "vlm-local-openai-compatible")
            .expect("vlm endpoint");
        assert_eq!(vlm.status, ModelEndpointStatus::Disabled);
    }

    #[test]
    fn runtime_probe_falls_back_to_builtin_local_endpoint_urls() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let addr = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = json!({
                "service": "harbor-model-api",
                "status": "ok",
                "backend": {
                    "kind": "candle",
                    "ready": true
                },
                "chat_model": "/models/qwen",
                "embedding_model": "/models/jina",
                "ready": true
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let _base_url = EnvGuard::set("HARBOR_MODEL_API_BASE_URL", &format!("http://{addr}/v1"));
        let mut endpoints =
            harborbeacon_local_agent::runtime::admin_console::default_model_endpoints();
        for endpoint in &mut endpoints {
            if matches!(endpoint.model_kind, ModelKind::Llm | ModelKind::Embedder) {
                endpoint.metadata = json!({
                    "builtin": true,
                    "base_url": "",
                    "healthz_url": "",
                    "api_key": "",
                    "api_key_configured": false
                });
            }
        }

        let projection = probe_local_model_runtime(&endpoints);

        assert!(projection.ready);
        assert!(projection.backend_ready);
        assert_eq!(projection.backend_kind.as_deref(), Some("candle"));
        assert_eq!(projection.chat_model.as_deref(), Some("/models/qwen"));
        assert_eq!(projection.embedding_model.as_deref(), Some("/models/jina"));
        assert!(projection.api_key_configured);
        assert_eq!(projection.healthz_url, format!("http://{addr}/healthz"));

        server.join().expect("server join");
    }

    #[test]
    fn feature_availability_prefers_runtime_truth_for_embed_and_answer() {
        let registry_path = unique_store_path("harborbeacon-feature-runtime-registry");
        let admin_path = unique_store_path("harborbeacon-feature-runtime-state");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let mut state = admin_store.load_or_create_state().expect("state");
        for endpoint in &mut state.models.endpoints {
            if matches!(endpoint.model_kind, ModelKind::Llm | ModelKind::Embedder) {
                endpoint.status = ModelEndpointStatus::Disabled;
                endpoint.metadata = json!({
                    "builtin": true,
                    "base_url": "",
                    "healthz_url": "",
                    "api_key": "",
                    "api_key_configured": false,
                });
            }
        }
        let account_management =
            harborbeacon_local_agent::runtime::admin_console::account_management_snapshot(
                &state,
                Some("http://harborbeacon.local:4174"),
            );
        let overlayed = overlay_model_endpoints_with_runtime_truth(
            &state.models.endpoints,
            &LocalModelRuntimeProjection {
                ready: true,
                backend_ready: true,
                backend_kind: Some("candle".to_string()),
                chat_model: Some("/models/qwen".to_string()),
                embedding_model: Some("/models/jina".to_string()),
                ..Default::default()
            },
        );

        let response = build_feature_availability_response(
            &overlayed,
            &state.models.route_policies,
            &account_management,
            None,
            &LocalModelRuntimeProjection {
                ready: true,
                backend_ready: true,
                backend_kind: Some("candle".to_string()),
                chat_model: Some("/models/qwen".to_string()),
                embedding_model: Some("/models/jina".to_string()),
                ..Default::default()
            },
        );

        let embed = response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.feature_id == "retrieval.embed")
            .expect("embed feature");
        assert_eq!(embed.status, "available");
        assert!(embed
            .evidence
            .iter()
            .any(|entry| entry.contains("projection_mismatch")));

        let answer = response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.feature_id == "retrieval.answer")
            .expect("answer feature");
        assert_eq!(answer.status, "available");
        assert!(answer
            .evidence
            .iter()
            .any(|entry| entry.contains("4176.backend.kind=candle")));

        let vision = response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.feature_id == "retrieval.vision_summary")
            .expect("vision feature");
        assert_ne!(vision.status, "available");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn feature_availability_surfaces_weixin_blocker_without_secret_material() {
        let registry_path = unique_store_path("harborbeacon-feature-weixin-registry");
        let admin_path = unique_store_path("harborbeacon-feature-weixin-state");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let state = admin_store.load_or_create_state().expect("state");
        let account_management =
            harborbeacon_local_agent::runtime::admin_console::account_management_snapshot(
                &state,
                Some("http://harborbeacon.local:4174"),
            );
        let gateway_payload = json!({
            "ok": true,
            "channels": [
                {
                    "platform": "weixin",
                    "connected": false,
                    "transport": {
                        "status": "error"
                    }
                }
            ],
            "weixin": {
                "blocker_category": "weixin_dns_resolution",
                "ingress_blocker_category": "getupdates",
                "status": "error",
                "poll": {
                    "status": "error",
                    "error": "<urlopen error [Errno 11001] getaddrinfo failed>"
                },
                "delivery_observability": {
                    "last_send_status": ""
                },
                "app_secret": "should-not-leak"
            },
            "release_v1": {
                "weixin_blocker_category": "getupdates"
            },
            "delivery_observability": {
                "record_count": 0
            }
        });

        let response = build_feature_availability_response(
            &state.models.endpoints,
            &state.models.route_policies,
            &account_management,
            Some(&gateway_payload),
            &LocalModelRuntimeProjection::default(),
        );

        let proactive = response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.feature_id == "proactive_delivery")
            .expect("proactive delivery");
        assert_eq!(proactive.status, "blocked");
        assert_eq!(proactive.blocker, "weixin_dns_resolution");
        assert!(proactive
            .evidence
            .iter()
            .any(|entry| entry.contains("delivery_observability.record_count=0")));
        assert!(!proactive.blocker.contains("should-not-leak"));

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn feature_availability_keeps_runtime_features_available_while_weixin_blocker_isolated() {
        let registry_path = unique_store_path("harborbeacon-feature-isolated-runtime-registry");
        let admin_path = unique_store_path("harborbeacon-feature-isolated-runtime-state");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let mut state = admin_store.load_or_create_state().expect("state");
        for endpoint in &mut state.models.endpoints {
            if matches!(endpoint.model_kind, ModelKind::Llm | ModelKind::Embedder) {
                endpoint.status = ModelEndpointStatus::Disabled;
                endpoint.metadata = json!({
                    "builtin": true,
                    "base_url": "",
                    "healthz_url": "",
                    "api_key": "",
                    "api_key_configured": false,
                });
            }
        }
        let account_management =
            harborbeacon_local_agent::runtime::admin_console::account_management_snapshot(
                &state,
                Some("http://harborbeacon.local:4174"),
            );
        let runtime = LocalModelRuntimeProjection {
            ready: true,
            backend_ready: true,
            backend_kind: Some("candle".to_string()),
            chat_model: Some("/models/qwen".to_string()),
            embedding_model: Some("/models/jina".to_string()),
            ..Default::default()
        };
        let overlayed =
            overlay_model_endpoints_with_runtime_truth(&state.models.endpoints, &runtime);
        let gateway_payload = json!({
            "ok": true,
            "channels": [
                {
                    "platform": "weixin",
                    "connected": false,
                    "transport": {
                        "status": "error"
                    }
                }
            ],
            "weixin": {
                "blocker_category": "weixin_dns_resolution",
                "ingress_blocker_category": "getupdates",
                "status": "error",
                "poll": {
                    "status": "error",
                    "error": "<urlopen error [Errno 11001] getaddrinfo failed>"
                }
            },
            "release_v1": {
                "weixin_blocker_category": "getupdates"
            },
            "delivery_observability": {
                "record_count": 0
            }
        });

        let response = build_feature_availability_response(
            &overlayed,
            &state.models.route_policies,
            &account_management,
            Some(&gateway_payload),
            &runtime,
        );

        let answer = response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.feature_id == "retrieval.answer")
            .expect("answer feature");
        assert_eq!(answer.status, "available");
        assert!(answer.blocker.is_empty());
        assert!(answer
            .evidence
            .iter()
            .any(|entry| entry.contains("4176.backend.kind=candle")));

        let proactive = response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.feature_id == "proactive_delivery")
            .expect("proactive delivery");
        assert_eq!(proactive.status, "blocked");
        assert_eq!(proactive.blocker, "weixin_dns_resolution");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
    }

    #[test]
    fn feature_availability_falls_back_to_release_v1_weixin_blocker_category() {
        let registry_path = unique_store_path("harborbeacon-feature-release-v1-weixin-registry");
        let admin_path = unique_store_path("harborbeacon-feature-release-v1-weixin-state");
        let registry_store = DeviceRegistryStore::new(registry_path.clone());
        let admin_store = AdminConsoleStore::new(admin_path.clone(), registry_store);
        let state = admin_store.load_or_create_state().expect("state");
        let account_management =
            harborbeacon_local_agent::runtime::admin_console::account_management_snapshot(
                &state,
                Some("http://harborbeacon.local:4174"),
            );
        let gateway_payload = json!({
            "ok": true,
            "channels": [
                {
                    "platform": "weixin",
                    "connected": false,
                    "transport": {
                        "status": "error"
                    }
                }
            ],
            "release_v1": {
                "weixin_blocker_category": "getupdates"
            },
            "delivery_observability": {
                "record_count": 0
            }
        });

        let response = build_feature_availability_response(
            &state.models.endpoints,
            &state.models.route_policies,
            &account_management,
            Some(&gateway_payload),
            &LocalModelRuntimeProjection::default(),
        );

        let proactive = response
            .groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.feature_id == "proactive_delivery")
            .expect("proactive delivery");
        assert_eq!(proactive.status, "blocked");
        assert_eq!(proactive.blocker, "getupdates");

        let _ = fs::remove_file(admin_path);
        let _ = fs::remove_file(registry_path);
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
        let page_response =
            api.handle_mobile_setup_page("/setup/mobile", &AccessIdentityHints::default());

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
