//! Thin persistence layer for the local Agent Hub admin console.

use std::collections::HashSet;
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use if_addrs::{get_if_addrs, IfAddr};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::control_plane::access::{PermissionBinding, PermissionEffect, ScopeKind};
use crate::control_plane::auth::{AuthSource, IdentityBinding};
use crate::control_plane::credentials::{
    CredentialKind, CredentialRecord, CredentialRotationState, ProviderAccount,
    ProviderAccountStatus, ProviderKind, ProviderOwnerScope,
};
use crate::control_plane::media::{RecordingPolicy, RecordingTriggerMode, StorageTargetKind};
use crate::control_plane::users::{
    Membership, MembershipStatus, RoleKind, UserAccount, UserStatus, Workspace, WorkspaceStatus,
    WorkspaceType,
};
use crate::runtime::registry::{CameraDevice, DeviceRegistryStore};

const DEFAULT_BINDING_CHANNEL_LABEL: &str = "Harbor IM Gateway";
const DEFAULT_PROVIDER_ACCOUNT_DISPLAY_NAME: &str = "Harbor IM Gateway";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdminBindingState {
    pub status: String,
    pub metric: String,
    pub bound_user: Option<String>,
    pub channel: String,
    #[serde(default = "generate_binding_code")]
    pub session_code: String,
    #[serde(default)]
    pub qr_token: String,
    #[serde(default)]
    pub setup_url: String,
    #[serde(default)]
    pub static_setup_url: String,
}

impl Default for AdminBindingState {
    fn default() -> Self {
        let session_code = generate_binding_code();
        Self {
            status: "等待扫码".to_string(),
            metric: "等待绑定".to_string(),
            bound_user: None,
            channel: DEFAULT_BINDING_CHANNEL_LABEL.to_string(),
            session_code: session_code.clone(),
            qr_token: generate_qr_token(&session_code),
            setup_url: String::new(),
            static_setup_url: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BridgeProviderCapabilities {
    #[serde(default)]
    pub reply: bool,
    #[serde(default)]
    pub update: bool,
    #[serde(default)]
    pub attachments: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BridgeProviderConfig {
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub gateway_base_url: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub app_secret: String,
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub bot_open_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_checked_at: String,
    #[serde(default)]
    pub capabilities: BridgeProviderCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteViewConfig {
    #[serde(default = "default_share_secret")]
    pub share_secret: String,
    #[serde(default = "default_share_link_ttl_minutes")]
    pub share_link_ttl_minutes: u32,
}

impl Default for RemoteViewConfig {
    fn default() -> Self {
        Self {
            share_secret: default_share_secret(),
            share_link_ttl_minutes: default_share_link_ttl_minutes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityBindingRecord {
    pub open_id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub union_id: Option<String>,
    pub display_name: String,
    #[serde(default)]
    pub chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdminDefaults {
    pub cidr: String,
    pub discovery: String,
    pub recording: String,
    pub capture: String,
    pub ai: String,
    #[serde(alias = "feishu_group")]
    pub notification_channel: String,
    #[serde(default = "default_rtsp_username")]
    pub rtsp_username: String,
    #[serde(default)]
    pub rtsp_password: String,
    #[serde(default = "default_rtsp_port")]
    pub rtsp_port: u16,
    #[serde(default = "default_rtsp_paths")]
    pub rtsp_paths: Vec<String>,
}

impl Default for AdminDefaults {
    fn default() -> Self {
        Self {
            cidr: default_scan_cidr(),
            // Prefer ONVIF WS-Discovery when available, fall back to RTSP probe for legacy cameras.
            discovery: "ONVIF + RTSP".to_string(),
            recording: "按事件录制".to_string(),
            capture: "图片 + 摘要".to_string(),
            ai: "人体检测 + 中文摘要".to_string(),
            notification_channel: "家庭通知频道".to_string(),
            rtsp_username: default_rtsp_username(),
            rtsp_password: String::new(),
            rtsp_port: default_rtsp_port(),
            rtsp_paths: default_rtsp_paths(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AdminPlatformState {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub users: Vec<UserAccount>,
    #[serde(default)]
    pub memberships: Vec<Membership>,
    #[serde(default)]
    pub identity_bindings: Vec<IdentityBinding>,
    #[serde(default)]
    pub permission_bindings: Vec<PermissionBinding>,
    #[serde(default)]
    pub provider_accounts: Vec<ProviderAccount>,
    #[serde(default)]
    pub credentials: Vec<CredentialRecord>,
    #[serde(default)]
    pub recording_policies: Vec<RecordingPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AdminConsoleState {
    #[serde(default)]
    pub binding: AdminBindingState,
    #[serde(default)]
    pub defaults: AdminDefaults,
    #[serde(default, alias = "feishu_bot")]
    pub bridge_provider: BridgeProviderConfig,
    #[serde(default)]
    pub remote_view: RemoteViewConfig,
    #[serde(default, alias = "feishu_users")]
    pub identity_bindings: Vec<IdentityBindingRecord>,
    #[serde(default)]
    pub platform: AdminPlatformState,
}

#[derive(Debug, Clone)]
pub struct AdminConsoleStore {
    path: PathBuf,
    registry_store: DeviceRegistryStore,
}

const DEFAULT_WORKSPACE_ID: &str = "home-1";
const DEFAULT_WORKSPACE_OWNER_ID: &str = "local-owner";
const LOCAL_RTSP_PROVIDER_ACCOUNT_ID: &str = "provider-local-rtsp";
const LOCAL_RTSP_CREDENTIAL_ID: &str = "credential-local-rtsp-password";
const BRIDGE_PROVIDER_ACCOUNT_ID: &str = "provider-im-bridge";
const DEFAULT_RECORDING_POLICY_ID: &str = "recording-policy-default";

impl AdminConsoleStore {
    pub fn new(path: impl Into<PathBuf>, registry_store: DeviceRegistryStore) -> Self {
        Self {
            path: path.into(),
            registry_store,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_state(&self) -> Result<AdminConsoleState, String> {
        if !self.path.exists() {
            return self.bootstrap_state();
        }

        let text = fs::read_to_string(&self.path).map_err(|e| {
            format!(
                "failed to read admin console state {}: {e}",
                self.path.display()
            )
        })?;
        let mut state: AdminConsoleState = serde_json::from_str(&text).map_err(|e| {
            format!(
                "failed to parse admin console state {}: {e}",
                self.path.display()
            )
        })?;
        self.apply_registry_hints(&mut state)?;
        Ok(state)
    }

    fn save_state(&self, state: &AdminConsoleState) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "failed to create admin console directory {}: {e}",
                    parent.display()
                )
            })?;
        }

        let mut sanitized = state.clone();
        sanitize_admin_state(&mut sanitized);
        let payload = serde_json::to_string_pretty(&sanitized).map_err(|e| {
            format!(
                "failed to serialize admin console state {}: {e}",
                self.path.display()
            )
        })?;
        fs::write(&self.path, payload).map_err(|e| {
            format!(
                "failed to write admin console state {}: {e}",
                self.path.display()
            )
        })
    }

    fn save_platform_primary_state(
        &self,
        mut platform_state: AdminConsoleState,
    ) -> Result<AdminConsoleState, String> {
        hydrate_legacy_views_from_platform(&mut platform_state);
        sanitize_legacy_admin_fields(&mut platform_state);
        platform_state.platform = sync_platform_from_legacy(&platform_state);
        self.save_state(&platform_state)?;
        Ok(platform_state)
    }

    fn save_projected_state(
        &self,
        mut projected_state: AdminConsoleState,
    ) -> Result<AdminConsoleState, String> {
        sanitize_legacy_admin_fields(&mut projected_state);
        projected_state.platform = sync_platform_from_legacy(&projected_state);
        hydrate_legacy_views_from_platform(&mut projected_state);
        self.save_state(&projected_state)?;
        Ok(projected_state)
    }

    pub fn load_or_create_state(&self) -> Result<AdminConsoleState, String> {
        let state = self.load_state()?;
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn refresh_binding_qr(&self) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.binding = AdminBindingState::default();
        self.save_projected_state(state)
    }

    pub fn mark_demo_bound(&self, user_name: &str) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.binding.status = "已绑定".to_string();
        state.binding.metric = "已绑定".to_string();
        state.binding.bound_user = Some(user_name.to_string());
        self.save_projected_state(state)
    }

    pub fn bind_identity_user(
        &self,
        token_or_code: &str,
        user: IdentityBindingRecord,
    ) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        let provided_code =
            normalize_binding_code(token_or_code).ok_or_else(|| "绑定码格式不正确".to_string())?;
        if provided_code != state.binding.session_code {
            return Err(format!(
                "绑定码不匹配，当前有效绑定码是 {}",
                state.binding.session_code
            ));
        }

        let user = sanitize_identity_binding_record(user)?;
        let workspace = state
            .platform
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == DEFAULT_WORKSPACE_ID)
            .or_else(|| state.platform.workspaces.first())
            .cloned()
            .ok_or_else(|| "当前没有可编辑的 workspace".to_string())?;
        let projected_user_id = projected_user_id_for_binding(&user);

        state.binding.status = "已绑定".to_string();
        state.binding.metric = "已绑定".to_string();
        state.binding.bound_user = Some(user.display_name.clone());

        upsert_user(
            &mut state.platform.users,
            build_user_account_projection(&user, &workspace.workspace_id),
        );
        upsert_identity_binding(
            &mut state.platform.identity_bindings,
            build_identity_binding_projection(&user),
        );

        if projected_user_id != workspace.owner_user_id {
            if let Some(existing) = state.platform.memberships.iter_mut().find(|membership| {
                membership.workspace_id == workspace.workspace_id
                    && membership.user_id == projected_user_id
            }) {
                existing.status = MembershipStatus::Active;
            } else {
                state
                    .platform
                    .memberships
                    .push(build_membership_projection(&workspace, &user));
            }
        }

        if let Some(workspace) = preferred_workspace_mut(&mut state.platform) {
            set_workspace_binding_projection(workspace, &state.binding);
        }

        self.save_platform_primary_state(state)
    }

    pub fn set_member_role(
        &self,
        user_id: &str,
        role_kind: RoleKind,
    ) -> Result<AdminConsoleState, String> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Err("user_id 不能为空".to_string());
        }
        if role_kind == RoleKind::Owner {
            return Err("当前入口不支持把成员直接提升为 owner".to_string());
        }

        let mut state = self.load_or_create_state()?;
        let workspace = state
            .platform
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == DEFAULT_WORKSPACE_ID)
            .or_else(|| state.platform.workspaces.first())
            .cloned()
            .ok_or_else(|| "当前没有可编辑的 workspace".to_string())?;

        if user_id == workspace.owner_user_id {
            return Err("本地 owner 角色不能在这里修改".to_string());
        }

        ensure_platform_user_exists(&mut state, &workspace.workspace_id, user_id)?;

        if let Some(membership) = state.platform.memberships.iter_mut().find(|membership| {
            membership.workspace_id == workspace.workspace_id && membership.user_id == user_id
        }) {
            membership.role_kind = role_kind;
            membership.status = MembershipStatus::Active;
        } else {
            state.platform.memberships.push(Membership {
                membership_id: format!("membership-{user_id}"),
                workspace_id: workspace.workspace_id,
                user_id: user_id.to_string(),
                role_kind,
                status: MembershipStatus::Active,
                granted_by_user_id: Some(workspace.owner_user_id),
                granted_at: None,
            });
        }

        self.save_platform_primary_state(state)
    }

    pub fn save_defaults(&self, defaults: AdminDefaults) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.defaults = sanitize_defaults(defaults);
        self.save_projected_state(state)
    }

    pub fn save_remote_view_config(
        &self,
        config: RemoteViewConfig,
    ) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.remote_view = sanitize_remote_view_config(config);
        if let Some(workspace) = preferred_workspace_mut(&mut state.platform) {
            set_workspace_remote_view_projection(workspace, &state.remote_view);
        }
        self.save_platform_primary_state(state)
    }

    pub fn load_remote_view_config(&self) -> Result<RemoteViewConfig, String> {
        let state = self.load_or_create_state()?;
        Ok(resolved_remote_view_config(&state))
    }

    pub fn save_bridge_provider_status(
        &self,
        config: BridgeProviderConfig,
    ) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.bridge_provider = sanitize_bridge_provider_config(config);
        if state.bridge_provider.connected {
            state.binding.status = "Gateway 已连接".to_string();
            state.binding.metric = "Gateway 在线".to_string();
        } else if state.bridge_provider.configured {
            state.binding.status = "Gateway 已启用".to_string();
            state.binding.metric = "Gateway 未连通".to_string();
        } else {
            state.binding.status = "等待 Gateway".to_string();
            state.binding.metric = "Gateway 未配置".to_string();
        }
        if !state.bridge_provider.app_name.trim().is_empty() {
            state.binding.bound_user = Some(state.bridge_provider.app_name.clone());
        } else if !state.bridge_provider.platform.trim().is_empty() {
            state.binding.bound_user = Some(format!("{} gateway", state.bridge_provider.platform));
        } else {
            state.binding.bound_user = None;
        }

        state
            .platform
            .provider_accounts
            .retain(|provider| provider.provider_account_id != BRIDGE_PROVIDER_ACCOUNT_ID);
        if let Some(provider) = build_bridge_provider_account(
            &state.bridge_provider,
            &state.defaults.notification_channel,
            state.platform.identity_bindings.len(),
        ) {
            upsert_provider_account(&mut state.platform.provider_accounts, provider);
        }

        if let Some(workspace) = preferred_workspace_mut(&mut state.platform) {
            set_workspace_binding_projection(workspace, &state.binding);
        }

        self.save_platform_primary_state(state)
    }

    pub fn registry_store(&self) -> &DeviceRegistryStore {
        &self.registry_store
    }

    fn bootstrap_state(&self) -> Result<AdminConsoleState, String> {
        let mut state = AdminConsoleState::default();
        self.apply_registry_hints(&mut state)?;
        Ok(state)
    }

    fn apply_registry_hints(&self, state: &mut AdminConsoleState) -> Result<(), String> {
        let devices = self.registry_store.load_devices()?;
        if let Some(hints) = derive_rtsp_hints(&devices) {
            if state.defaults.rtsp_username.trim().is_empty() {
                state.defaults.rtsp_username = hints.username.clone();
            }
            if state.defaults.rtsp_password.trim().is_empty() {
                state.defaults.rtsp_password = hints.password;
            }
            if state.defaults.rtsp_paths.is_empty() {
                state.defaults.rtsp_paths = hints.paths;
            }
        }

        normalize_loaded_admin_state(state);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspHints {
    pub username: String,
    pub password: String,
    pub paths: Vec<String>,
}

pub fn sanitize_defaults(mut defaults: AdminDefaults) -> AdminDefaults {
    if defaults.cidr.trim().is_empty() || defaults.cidr.trim().eq_ignore_ascii_case("auto") {
        defaults.cidr = default_scan_cidr();
    }
    if defaults.discovery.trim().is_empty() {
        defaults.discovery = "RTSP Probe".to_string();
    }
    if defaults.recording.trim().is_empty() {
        defaults.recording = "按事件录制".to_string();
    }
    if defaults.capture.trim().is_empty() {
        defaults.capture = "图片 + 摘要".to_string();
    }
    if defaults.ai.trim().is_empty() {
        defaults.ai = "人体检测 + 中文摘要".to_string();
    }
    if defaults.notification_channel.trim().is_empty() {
        defaults.notification_channel = "家庭通知频道".to_string();
    }
    if defaults.rtsp_username.trim().is_empty() {
        defaults.rtsp_username = default_rtsp_username();
    }
    if defaults.rtsp_port == 0 {
        defaults.rtsp_port = default_rtsp_port();
    }
    defaults.rtsp_paths = dedupe_rtsp_paths(defaults.rtsp_paths);
    if defaults.rtsp_paths.is_empty() {
        defaults.rtsp_paths = default_rtsp_paths();
    }
    defaults
}

pub fn sanitize_binding(mut binding: AdminBindingState) -> AdminBindingState {
    if binding.status.trim().is_empty() {
        binding.status = "等待扫码".to_string();
    }
    if binding.metric.trim().is_empty() {
        binding.metric = "等待绑定".to_string();
    }
    if binding.channel.trim().is_empty() {
        binding.channel = DEFAULT_BINDING_CHANNEL_LABEL.to_string();
    }
    if let Some(token_code) = normalize_binding_code(&binding.qr_token) {
        if binding.session_code.trim().is_empty() || binding.session_code != token_code {
            binding.session_code = token_code;
        }
    } else if binding.session_code.trim().is_empty() {
        binding.session_code = generate_binding_code();
    }
    if binding.qr_token.trim().is_empty()
        || normalize_binding_code(&binding.qr_token).as_deref()
            != Some(binding.session_code.as_str())
    {
        binding.qr_token = generate_qr_token(&binding.session_code);
    }
    binding
}

pub fn sanitize_bridge_provider_config(mut config: BridgeProviderConfig) -> BridgeProviderConfig {
    config.app_id.clear();
    config.app_secret.clear();
    config.bot_open_id.clear();
    config.platform = config.platform.trim().to_string();
    config.gateway_base_url = config.gateway_base_url.trim().to_string();
    config.app_name = config.app_name.trim().to_string();
    config.last_checked_at = config.last_checked_at.trim().to_string();
    if config.status.trim().is_empty() {
        config.status = if config.connected {
            "已连接".to_string()
        } else if config.configured {
            "已启用，待连接".to_string()
        } else {
            "未配置".to_string()
        };
    }
    config
}

pub fn sanitize_remote_view_config(mut config: RemoteViewConfig) -> RemoteViewConfig {
    if config.share_secret.trim().is_empty() {
        config.share_secret = default_share_secret();
    }
    config.share_link_ttl_minutes = config.share_link_ttl_minutes.clamp(5, 24 * 60);
    config
}

fn normalize_loaded_admin_state(state: &mut AdminConsoleState) {
    sanitize_legacy_admin_fields(state);
    hydrate_legacy_views_from_platform(state);
    sanitize_legacy_admin_fields(state);
    state.platform = sync_platform_from_legacy(state);
}

pub fn sanitize_admin_state(state: &mut AdminConsoleState) {
    sanitize_legacy_admin_fields(state);
    state.platform = sync_platform_from_legacy(state);
}

fn sanitize_legacy_admin_fields(state: &mut AdminConsoleState) {
    state.binding = sanitize_binding(state.binding.clone());
    state.defaults = sanitize_defaults(state.defaults.clone());
    state.bridge_provider = sanitize_bridge_provider_config(state.bridge_provider.clone());
    state.remote_view = sanitize_remote_view_config(state.remote_view.clone());
}

fn hydrate_legacy_views_from_platform(state: &mut AdminConsoleState) {
    apply_provider_projections_to_legacy(state);
    apply_workspace_projection_to_legacy(state);
    apply_recording_policy_to_legacy(state);
    if !state.platform.identity_bindings.is_empty() {
        state.identity_bindings = legacy_identity_bindings_from_platform(&state.platform);
    }
}

fn apply_workspace_projection_to_legacy(state: &mut AdminConsoleState) {
    let workspace = state
        .platform
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == DEFAULT_WORKSPACE_ID)
        .or_else(|| state.platform.workspaces.first())
        .cloned();
    let Some(workspace) = workspace else {
        return;
    };

    if let Some(binding) = workspace.settings.get("binding") {
        assign_string(&mut state.binding.channel, binding.get("channel"));
        assign_string(&mut state.binding.status, binding.get("status"));
        assign_string(&mut state.binding.metric, binding.get("metric"));
        state.binding.bound_user = optional_string(binding.get("bound_user"));
    }

    if let Some(defaults) = workspace.settings.get("defaults") {
        assign_string(&mut state.defaults.cidr, defaults.get("cidr"));
        assign_string(&mut state.defaults.discovery, defaults.get("discovery"));
        assign_string(&mut state.defaults.capture, defaults.get("capture"));
        assign_string(&mut state.defaults.ai, defaults.get("ai"));
        assign_string(
            &mut state.defaults.notification_channel,
            defaults.get("notification_channel"),
        );
        assign_string(
            &mut state.defaults.rtsp_username,
            defaults.get("rtsp_username"),
        );
        if let Some(port) = defaults.get("rtsp_port").and_then(Value::as_u64) {
            state.defaults.rtsp_port = port as u16;
        }
        if let Some(paths) = string_vec(defaults.get("rtsp_paths")) {
            state.defaults.rtsp_paths = paths;
        }
    }

    state.remote_view = resolved_remote_view_config(state);
}

fn apply_provider_projections_to_legacy(state: &mut AdminConsoleState) {
    if let Some(local_rtsp) = state
        .platform
        .provider_accounts
        .iter()
        .find(|provider| provider.provider_account_id == LOCAL_RTSP_PROVIDER_ACCOUNT_ID)
    {
        assign_string(
            &mut state.defaults.cidr,
            local_rtsp.capabilities.get("cidr"),
        );
        assign_string(
            &mut state.defaults.discovery,
            local_rtsp.capabilities.get("discovery"),
        );
        assign_string(
            &mut state.defaults.rtsp_username,
            local_rtsp.capabilities.get("rtsp_username"),
        );
        if let Some(port) = local_rtsp
            .capabilities
            .get("rtsp_port")
            .and_then(Value::as_u64)
        {
            state.defaults.rtsp_port = port as u16;
        }
        if let Some(paths) = string_vec(local_rtsp.capabilities.get("rtsp_paths")) {
            state.defaults.rtsp_paths = paths;
        }
        assign_string(
            &mut state.defaults.capture,
            local_rtsp.metadata.get("capture_mode"),
        );
        assign_string(&mut state.defaults.ai, local_rtsp.metadata.get("ai_mode"));
    }

    if let Some(bridge_provider) = state
        .platform
        .provider_accounts
        .iter()
        .find(|provider| provider.provider_account_id == BRIDGE_PROVIDER_ACCOUNT_ID)
    {
        state.bridge_provider.connected = bridge_provider.status == ProviderAccountStatus::Active;
        state.bridge_provider.configured =
            !matches!(bridge_provider.status, ProviderAccountStatus::Disabled);
        assign_string(
            &mut state.defaults.notification_channel,
            bridge_provider.capabilities.get("channel"),
        );
        assign_string(
            &mut state.bridge_provider.platform,
            bridge_provider.metadata.get("platform"),
        );
        assign_string(
            &mut state.bridge_provider.app_name,
            bridge_provider.metadata.get("display_name"),
        );
        assign_string(
            &mut state.bridge_provider.status,
            bridge_provider.metadata.get("status"),
        );
        assign_string(
            &mut state.bridge_provider.gateway_base_url,
            bridge_provider.metadata.get("gateway_base_url"),
        );
        assign_string(
            &mut state.bridge_provider.last_checked_at,
            bridge_provider.metadata.get("last_checked_at"),
        );
        state.bridge_provider.capabilities.reply = bridge_provider
            .capabilities
            .get("reply")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        state.bridge_provider.capabilities.update = bridge_provider
            .capabilities
            .get("update")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        state.bridge_provider.capabilities.attachments = bridge_provider
            .capabilities
            .get("attachments")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }

    if let Some(local_rtsp_credential) = state
        .platform
        .credentials
        .iter()
        .find(|credential| credential.credential_id == LOCAL_RTSP_CREDENTIAL_ID)
    {
        assign_string(
            &mut state.defaults.rtsp_username,
            local_rtsp_credential.scope.get("username"),
        );
        if let Some(port) = local_rtsp_credential
            .scope
            .get("port")
            .and_then(Value::as_u64)
        {
            state.defaults.rtsp_port = port as u16;
        }
    }
}

fn apply_recording_policy_to_legacy(state: &mut AdminConsoleState) {
    let policy = state
        .platform
        .recording_policies
        .iter()
        .find(|policy| policy.recording_policy_id == DEFAULT_RECORDING_POLICY_ID)
        .or_else(|| state.platform.recording_policies.first());
    let Some(policy) = policy else {
        return;
    };

    if let Some(label) = policy
        .metadata
        .get("recording_label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        state.defaults.recording = label.to_string();
    } else {
        state.defaults.recording = recording_label_from_policy(policy.trigger_mode);
    }
    assign_string(
        &mut state.defaults.capture,
        policy.metadata.get("capture_mode"),
    );
    assign_string(&mut state.defaults.ai, policy.metadata.get("ai_mode"));
    assign_string(
        &mut state.defaults.notification_channel,
        policy.metadata.get("notification_channel"),
    );
}

fn legacy_identity_bindings_from_platform(
    platform: &AdminPlatformState,
) -> Vec<IdentityBindingRecord> {
    let mut bindings = Vec::new();
    for binding in &platform.identity_bindings {
        if bindings
            .iter()
            .any(|existing: &IdentityBindingRecord| existing.open_id == binding.external_user_id)
        {
            continue;
        }
        let display_name = platform
            .users
            .iter()
            .find(|user| user.user_id == binding.user_id)
            .map(|user| user.display_name.clone())
            .or_else(|| {
                binding
                    .profile_snapshot
                    .get("display_name")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| binding.external_user_id.clone());
        bindings.push(IdentityBindingRecord {
            open_id: binding.external_user_id.clone(),
            user_id: Some(binding.user_id.clone()),
            union_id: binding.external_union_id.clone(),
            display_name,
            chat_id: binding.external_chat_id.clone(),
        });
    }
    bindings
}

pub fn resolved_identity_binding_records(state: &AdminConsoleState) -> Vec<IdentityBindingRecord> {
    if !state.platform.identity_bindings.is_empty() {
        return legacy_identity_bindings_from_platform(&state.platform);
    }

    state
        .identity_bindings
        .iter()
        .cloned()
        .filter_map(|binding| sanitize_identity_binding_record(binding).ok())
        .collect()
}

fn sync_platform_from_legacy(state: &AdminConsoleState) -> AdminPlatformState {
    let mut platform = state.platform.clone();
    let workspace = build_workspace_projection(state);
    upsert_workspace(&mut platform.workspaces, workspace.clone());

    for user in build_user_accounts(state, &workspace) {
        upsert_user(&mut platform.users, user);
    }
    for membership in build_memberships(state, &workspace) {
        let membership = preserve_custom_membership(&platform.memberships, membership);
        upsert_membership(&mut platform.memberships, membership);
    }
    for binding in build_identity_binding_projections(state) {
        upsert_identity_binding(&mut platform.identity_bindings, binding);
    }
    for permission in build_permission_bindings(&workspace) {
        upsert_permission_binding(&mut platform.permission_bindings, permission);
    }

    platform.provider_accounts.retain(|provider| {
        provider.provider_account_id != LOCAL_RTSP_PROVIDER_ACCOUNT_ID
            && provider.provider_account_id != BRIDGE_PROVIDER_ACCOUNT_ID
    });
    platform
        .provider_accounts
        .extend(build_provider_accounts(state));

    platform
        .credentials
        .retain(|credential| credential.credential_id != LOCAL_RTSP_CREDENTIAL_ID);
    platform.credentials.extend(build_credentials(state));

    platform
        .recording_policies
        .retain(|policy| policy.recording_policy_id != DEFAULT_RECORDING_POLICY_ID);
    platform
        .recording_policies
        .push(build_recording_policy(state));

    platform
}

fn upsert_workspace(workspaces: &mut Vec<Workspace>, workspace: Workspace) {
    if let Some(existing) = workspaces
        .iter_mut()
        .find(|existing| existing.workspace_id == workspace.workspace_id)
    {
        *existing = workspace;
    } else {
        workspaces.push(workspace);
    }
}

fn upsert_user(users: &mut Vec<UserAccount>, user: UserAccount) {
    if let Some(existing) = users
        .iter_mut()
        .find(|existing| existing.user_id == user.user_id)
    {
        *existing = user;
    } else {
        users.push(user);
    }
}

fn upsert_provider_account(providers: &mut Vec<ProviderAccount>, provider: ProviderAccount) {
    if let Some(existing) = providers
        .iter_mut()
        .find(|existing| existing.provider_account_id == provider.provider_account_id)
    {
        *existing = provider;
    } else {
        providers.push(provider);
    }
}

fn preferred_workspace_mut(platform: &mut AdminPlatformState) -> Option<&mut Workspace> {
    let index = platform
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id == DEFAULT_WORKSPACE_ID)
        .or_else(|| (!platform.workspaces.is_empty()).then_some(0))?;
    platform.workspaces.get_mut(index)
}

fn set_workspace_binding_projection(workspace: &mut Workspace, binding: &AdminBindingState) {
    if !workspace.settings.is_object() {
        workspace.settings = json!({});
    }
    let Some(settings) = workspace.settings.as_object_mut() else {
        return;
    };
    settings.insert(
        "binding".to_string(),
        json!({
            "channel": binding.channel.clone(),
            "status": binding.status.clone(),
            "metric": binding.metric.clone(),
            "bound_user": binding.bound_user.clone(),
        }),
    );
}

fn set_workspace_remote_view_projection(workspace: &mut Workspace, remote_view: &RemoteViewConfig) {
    if !workspace.settings.is_object() {
        workspace.settings = json!({});
    }
    let Some(settings) = workspace.settings.as_object_mut() else {
        return;
    };
    settings.insert(
        "remote_view".to_string(),
        json!({
            "share_link_ttl_minutes": remote_view.share_link_ttl_minutes,
            "share_secret": remote_view.share_secret.clone(),
            "share_secret_configured": !remote_view.share_secret.trim().is_empty(),
        }),
    );
}

pub fn resolved_remote_view_config(state: &AdminConsoleState) -> RemoteViewConfig {
    let mut config = sanitize_remote_view_config(state.remote_view.clone());
    let workspace = state
        .platform
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == DEFAULT_WORKSPACE_ID)
        .or_else(|| state.platform.workspaces.first());

    if let Some(remote_view) = workspace.and_then(|workspace| workspace.settings.get("remote_view"))
    {
        assign_string(&mut config.share_secret, remote_view.get("share_secret"));
        if let Some(ttl) = remote_view
            .get("share_link_ttl_minutes")
            .and_then(Value::as_u64)
        {
            config.share_link_ttl_minutes = ttl as u32;
        }
    }

    sanitize_remote_view_config(config)
}

fn ensure_platform_user_exists(
    state: &mut AdminConsoleState,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), String> {
    if state
        .platform
        .users
        .iter()
        .any(|user| user.user_id == user_id)
    {
        return Ok(());
    }

    let bindings = resolved_identity_binding_records(state);
    if let Some(binding) = bindings
        .iter()
        .find(|binding| binding.user_id.as_deref() == Some(user_id))
    {
        state.platform.users.push(UserAccount {
            user_id: user_id.to_string(),
            display_name: binding.display_name.clone(),
            email: None,
            phone: None,
            status: UserStatus::Active,
            default_workspace_id: Some(workspace_id.to_string()),
            preferences: json!({
                "auth_source": "im_bridge",
                "open_id": binding.open_id.clone(),
            }),
        });
        return Ok(());
    }

    Err(format!("未找到 user_id={user_id} 对应的平台用户"))
}

fn upsert_membership(memberships: &mut Vec<Membership>, membership: Membership) {
    if let Some(existing) = memberships
        .iter_mut()
        .find(|existing| existing.membership_id == membership.membership_id)
    {
        *existing = membership;
    } else {
        memberships.push(membership);
    }
}

fn preserve_custom_membership(
    existing_memberships: &[Membership],
    mut membership: Membership,
) -> Membership {
    if membership.role_kind == RoleKind::Owner {
        return membership;
    }

    if let Some(existing) = existing_memberships
        .iter()
        .find(|existing| existing.membership_id == membership.membership_id)
    {
        membership.role_kind = existing.role_kind;
        membership.status = existing.status;
        membership.granted_by_user_id = existing.granted_by_user_id.clone();
        membership.granted_at = existing.granted_at.clone();
    }

    membership
}

fn upsert_identity_binding(bindings: &mut Vec<IdentityBinding>, binding: IdentityBinding) {
    if let Some(existing) = bindings
        .iter_mut()
        .find(|existing| existing.identity_id == binding.identity_id)
    {
        *existing = binding;
    } else {
        bindings.push(binding);
    }
}

fn upsert_permission_binding(
    permissions: &mut Vec<PermissionBinding>,
    permission: PermissionBinding,
) {
    if let Some(existing) = permissions
        .iter_mut()
        .find(|existing| existing.permission_binding_id == permission.permission_binding_id)
    {
        *existing = permission;
    } else {
        permissions.push(permission);
    }
}

fn sanitize_identity_binding_record(
    mut binding: IdentityBindingRecord,
) -> Result<IdentityBindingRecord, String> {
    binding.open_id = binding.open_id.trim().to_string();
    if binding.open_id.is_empty() {
        return Err("open_id 不能为空".to_string());
    }
    binding.display_name = binding.display_name.trim().to_string();
    if binding.display_name.is_empty() {
        binding.display_name = binding.open_id.clone();
    }
    binding.user_id = binding
        .user_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    binding.union_id = binding
        .union_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    binding.chat_id = binding
        .chat_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Ok(binding)
}

fn assign_string(target: &mut String, value: Option<&Value>) {
    if let Some(value) = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        *target = value.to_string();
    }
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn string_vec(value: Option<&Value>) -> Option<Vec<String>> {
    let values = value?.as_array()?;
    let items: Vec<String> = values
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn recording_label_from_policy(trigger_mode: RecordingTriggerMode) -> String {
    match trigger_mode {
        RecordingTriggerMode::Continuous => "持续录制".to_string(),
        RecordingTriggerMode::Event => "按事件录制".to_string(),
        RecordingTriggerMode::Manual => "手动录制".to_string(),
        RecordingTriggerMode::Schedule => "定时录制".to_string(),
    }
}

pub fn build_platform_state(state: &AdminConsoleState) -> AdminPlatformState {
    let workspace = build_workspace_projection(state);
    AdminPlatformState {
        workspaces: vec![workspace.clone()],
        users: build_user_accounts(state, &workspace),
        memberships: build_memberships(state, &workspace),
        identity_bindings: build_identity_binding_projections(state),
        permission_bindings: build_permission_bindings(&workspace),
        provider_accounts: build_provider_accounts(state),
        credentials: build_credentials(state),
        recording_policies: vec![build_recording_policy(state)],
    }
}

fn build_workspace_projection(state: &AdminConsoleState) -> Workspace {
    let mut workspace = Workspace {
        workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
        workspace_type: WorkspaceType::Home,
        display_name: "Harbor Home".to_string(),
        timezone: "Asia/Shanghai".to_string(),
        locale: "zh-CN".to_string(),
        owner_user_id: DEFAULT_WORKSPACE_OWNER_ID.to_string(),
        status: WorkspaceStatus::Active,
        settings: json!({
            "binding": {
                "channel": state.binding.channel.clone(),
                "status": state.binding.status.clone(),
                "metric": state.binding.metric.clone(),
                "bound_user": state.binding.bound_user.clone(),
            },
            "defaults": {
                "cidr": state.defaults.cidr.clone(),
                "discovery": state.defaults.discovery.clone(),
                "capture": state.defaults.capture.clone(),
                "ai": state.defaults.ai.clone(),
                "notification_channel": state.defaults.notification_channel.clone(),
                "rtsp_port": state.defaults.rtsp_port,
                "rtsp_paths": state.defaults.rtsp_paths.clone(),
                "rtsp_username": state.defaults.rtsp_username.clone(),
            },
        }),
    };
    set_workspace_remote_view_projection(&mut workspace, &state.remote_view);
    workspace
}

fn build_provider_accounts(state: &AdminConsoleState) -> Vec<ProviderAccount> {
    let bindings = resolved_identity_binding_records(state);
    let mut providers = vec![ProviderAccount {
        provider_account_id: LOCAL_RTSP_PROVIDER_ACCOUNT_ID.to_string(),
        workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
        provider_key: "local_rtsp".to_string(),
        provider_kind: ProviderKind::VendorLocal,
        display_name: "本地 RTSP 默认接入".to_string(),
        owner_scope: ProviderOwnerScope::Workspace,
        owner_user_id: None,
        status: ProviderAccountStatus::Active,
        capabilities: json!({
            "cidr": state.defaults.cidr.clone(),
            "discovery": state.defaults.discovery.clone(),
            "rtsp_port": state.defaults.rtsp_port,
            "rtsp_paths": state.defaults.rtsp_paths.clone(),
            "rtsp_username": state.defaults.rtsp_username.clone(),
        }),
        metadata: json!({
            "capture_mode": state.defaults.capture.clone(),
            "ai_mode": state.defaults.ai.clone(),
        }),
    }];

    if let Some(provider) = build_bridge_provider_account(
        &state.bridge_provider,
        &state.defaults.notification_channel,
        bindings.len(),
    ) {
        providers.push(provider);
    }

    providers
}

fn build_bridge_provider_account(
    config: &BridgeProviderConfig,
    notification_channel: &str,
    bound_users: usize,
) -> Option<ProviderAccount> {
    if !config.configured
        && !config.connected
        && config.app_name.trim().is_empty()
        && config.gateway_base_url.trim().is_empty()
    {
        return None;
    }

    Some(ProviderAccount {
        provider_account_id: BRIDGE_PROVIDER_ACCOUNT_ID.to_string(),
        workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
        provider_key: "im_bridge".to_string(),
        provider_kind: ProviderKind::Bridge,
        display_name: DEFAULT_PROVIDER_ACCOUNT_DISPLAY_NAME.to_string(),
        owner_scope: ProviderOwnerScope::Workspace,
        owner_user_id: None,
        status: if config.connected {
            ProviderAccountStatus::Active
        } else if config.configured {
            ProviderAccountStatus::NeedsReauth
        } else {
            ProviderAccountStatus::Disabled
        },
        capabilities: json!({
            "channel": notification_channel,
            "bound_users": bound_users,
            "reply": config.capabilities.reply,
            "update": config.capabilities.update,
            "attachments": config.capabilities.attachments,
        }),
        metadata: json!({
            "platform": config.platform.clone(),
            "display_name": config.app_name.clone(),
            "status": config.status.clone(),
            "gateway_base_url": config.gateway_base_url.clone(),
            "last_checked_at": config.last_checked_at.clone(),
        }),
    })
}

fn build_user_accounts(state: &AdminConsoleState, workspace: &Workspace) -> Vec<UserAccount> {
    let bindings = resolved_identity_binding_records(state);
    let mut users = vec![UserAccount {
        user_id: workspace.owner_user_id.clone(),
        display_name: "本地管理员".to_string(),
        email: None,
        phone: None,
        status: UserStatus::Active,
        default_workspace_id: Some(workspace.workspace_id.clone()),
        preferences: json!({
            "bootstrap": true,
            "channel": "local_console",
        }),
    }];

    for binding in &bindings {
        let user_id = projected_user_id_for_binding(binding);
        if users.iter().any(|user| user.user_id == user_id) {
            continue;
        }
        users.push(build_user_account_projection(
            binding,
            &workspace.workspace_id,
        ));
    }

    users
}

fn build_user_account_projection(
    binding: &IdentityBindingRecord,
    workspace_id: &str,
) -> UserAccount {
    UserAccount {
        user_id: projected_user_id_for_binding(binding),
        display_name: binding.display_name.clone(),
        email: None,
        phone: None,
        status: UserStatus::Active,
        default_workspace_id: Some(workspace_id.to_string()),
        preferences: json!({
            "auth_source": "im_bridge",
            "open_id": binding.open_id.clone(),
        }),
    }
}

fn build_memberships(state: &AdminConsoleState, workspace: &Workspace) -> Vec<Membership> {
    let bindings = resolved_identity_binding_records(state);
    let mut memberships = vec![Membership {
        membership_id: format!("membership-{}", workspace.owner_user_id),
        workspace_id: workspace.workspace_id.clone(),
        user_id: workspace.owner_user_id.clone(),
        role_kind: RoleKind::Owner,
        status: MembershipStatus::Active,
        granted_by_user_id: None,
        granted_at: None,
    }];

    for binding in &bindings {
        let user_id = projected_user_id_for_binding(binding);
        if user_id == workspace.owner_user_id
            || memberships
                .iter()
                .any(|membership| membership.user_id == user_id)
        {
            continue;
        }
        memberships.push(build_membership_projection(workspace, binding));
    }

    memberships
}

fn build_membership_projection(
    workspace: &Workspace,
    binding: &IdentityBindingRecord,
) -> Membership {
    let user_id = projected_user_id_for_binding(binding);
    Membership {
        membership_id: format!("membership-{user_id}"),
        workspace_id: workspace.workspace_id.clone(),
        user_id,
        role_kind: RoleKind::Viewer,
        status: MembershipStatus::Active,
        granted_by_user_id: Some(workspace.owner_user_id.clone()),
        granted_at: None,
    }
}

fn build_identity_binding_projections(state: &AdminConsoleState) -> Vec<IdentityBinding> {
    resolved_identity_binding_records(state)
        .iter()
        .map(build_identity_binding_projection)
        .collect()
}

fn build_identity_binding_projection(binding: &IdentityBindingRecord) -> IdentityBinding {
    IdentityBinding {
        identity_id: format!("identity-{}", binding.open_id),
        user_id: projected_user_id_for_binding(binding),
        auth_source: AuthSource::ImChannel,
        provider_key: "im_bridge".to_string(),
        external_user_id: binding.open_id.clone(),
        external_union_id: binding.union_id.clone(),
        external_chat_id: binding.chat_id.clone(),
        profile_snapshot: json!({
            "display_name": binding.display_name.clone(),
        }),
        last_seen_at: None,
    }
}

fn build_permission_bindings(workspace: &Workspace) -> Vec<PermissionBinding> {
    let workspace_id = workspace.workspace_id.clone();
    vec![
        allow_workspace_permission(&workspace_id, RoleKind::Owner, "*", "*"),
        allow_workspace_permission(&workspace_id, RoleKind::Admin, "*", "admin.*"),
        allow_workspace_permission(&workspace_id, RoleKind::Admin, "*", "camera.*"),
        allow_workspace_permission(&workspace_id, RoleKind::Admin, "*", "approval.*"),
        allow_workspace_permission(&workspace_id, RoleKind::Operator, "*", "admin.read_state"),
        allow_workspace_permission(&workspace_id, RoleKind::Operator, "*", "camera.view"),
        allow_workspace_permission(&workspace_id, RoleKind::Operator, "*", "camera.operate"),
        allow_workspace_permission(&workspace_id, RoleKind::Member, "*", "camera.view"),
        allow_workspace_permission(&workspace_id, RoleKind::Viewer, "*", "camera.view"),
    ]
}

fn allow_workspace_permission(
    workspace_id: &str,
    role_kind: RoleKind,
    resource_pattern: &str,
    action_pattern: &str,
) -> PermissionBinding {
    PermissionBinding {
        permission_binding_id: format!(
            "perm-{workspace_id}-{}-{}",
            role_kind_key(role_kind),
            action_pattern.replace('.', "_").replace('*', "all")
        ),
        workspace_id: workspace_id.to_string(),
        role_kind: role_kind_key(role_kind).to_string(),
        scope_kind: ScopeKind::Workspace,
        resource_pattern: resource_pattern.to_string(),
        action_pattern: action_pattern.to_string(),
        effect: PermissionEffect::Allow,
        constraints: json!({}),
    }
}

fn role_kind_key(role_kind: RoleKind) -> &'static str {
    match role_kind {
        RoleKind::Owner => "owner",
        RoleKind::Admin => "admin",
        RoleKind::Operator => "operator",
        RoleKind::Member => "member",
        RoleKind::Viewer => "viewer",
        RoleKind::Guest => "guest",
    }
}

fn projected_user_id_for_binding(binding: &IdentityBindingRecord) -> String {
    binding
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("im-user-{}", slugify_identity_component(&binding.open_id)))
}

fn slugify_identity_component(value: &str) -> String {
    let mut compact = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            compact.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | '.') {
            compact.push('-');
        }
    }
    while compact.contains("--") {
        compact = compact.replace("--", "-");
    }
    compact.trim_matches('-').to_string()
}

fn build_credentials(state: &AdminConsoleState) -> Vec<CredentialRecord> {
    let mut credentials = Vec::new();
    if !state.defaults.rtsp_password.trim().is_empty() {
        credentials.push(CredentialRecord {
            credential_id: LOCAL_RTSP_CREDENTIAL_ID.to_string(),
            provider_account_id: LOCAL_RTSP_PROVIDER_ACCOUNT_ID.to_string(),
            credential_kind: CredentialKind::SessionSecret,
            vault_key: "admin_console.defaults.rtsp_password".to_string(),
            scope: json!({
                "username": state.defaults.rtsp_username.clone(),
                "port": state.defaults.rtsp_port,
            }),
            expires_at: None,
            rotation_state: CredentialRotationState::Valid,
            last_verified_at: None,
            metadata: json!({
                "present": true,
                "path_count": state.defaults.rtsp_paths.len(),
            }),
        });
    }
    credentials
}

fn build_recording_policy(state: &AdminConsoleState) -> RecordingPolicy {
    let trigger_mode = recording_trigger_mode_from_label(&state.defaults.recording);
    RecordingPolicy {
        recording_policy_id: DEFAULT_RECORDING_POLICY_ID.to_string(),
        workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
        device_id: None,
        trigger_mode,
        pre_event_seconds: if trigger_mode == RecordingTriggerMode::Event {
            15
        } else {
            0
        },
        post_event_seconds: if trigger_mode == RecordingTriggerMode::Event {
            30
        } else {
            0
        },
        clip_length_seconds: match trigger_mode {
            RecordingTriggerMode::Continuous | RecordingTriggerMode::Schedule => 300,
            RecordingTriggerMode::Manual => 180,
            RecordingTriggerMode::Event => 60,
        },
        retention_days: 30,
        storage_target: StorageTargetKind::Nas,
        metadata: json!({
            "recording_label": state.defaults.recording.clone(),
            "capture_mode": state.defaults.capture.clone(),
            "ai_mode": state.defaults.ai.clone(),
            "notification_channel": state.defaults.notification_channel.clone(),
        }),
    }
}

fn recording_trigger_mode_from_label(label: &str) -> RecordingTriggerMode {
    let normalized = label.trim().to_lowercase();
    if normalized.contains("continuous") || normalized.contains("持续") {
        RecordingTriggerMode::Continuous
    } else if normalized.contains("manual") || normalized.contains("手动") {
        RecordingTriggerMode::Manual
    } else if normalized.contains("schedule")
        || normalized.contains("计划")
        || normalized.contains("定时")
    {
        RecordingTriggerMode::Schedule
    } else {
        RecordingTriggerMode::Event
    }
}

pub fn default_rtsp_username() -> String {
    "admin".to_string()
}

pub fn default_share_secret() -> String {
    let primary = Uuid::new_v4().simple().to_string();
    let secondary = Uuid::new_v4().simple().to_string();
    format!("{primary}{secondary}")
}

pub fn default_share_link_ttl_minutes() -> u32 {
    120
}

pub fn default_scan_cidr() -> String {
    detect_primary_private_ipv4_cidr().unwrap_or_else(|| "192.168.3.0/24".to_string())
}

pub fn default_rtsp_port() -> u16 {
    554
}

pub fn default_rtsp_paths() -> Vec<String> {
    vec![
        "/ch1/main".to_string(),
        "/h264/ch1/main/av_stream".to_string(),
        "/Streaming/Channels/101".to_string(),
    ]
}

fn detect_primary_private_ipv4_cidr() -> Option<String> {
    let interfaces = get_if_addrs().ok()?;
    for iface in interfaces {
        let IfAddr::V4(v4) = iface.addr else {
            continue;
        };
        if v4.ip.is_loopback() || !is_private_ipv4(v4.ip) {
            continue;
        }
        let prefix = netmask_prefix(v4.netmask)?;
        let network = Ipv4Addr::from(u32::from(v4.ip) & u32::from(v4.netmask));
        return Some(format!("{network}/{prefix}"));
    }
    None
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

fn netmask_prefix(netmask: Ipv4Addr) -> Option<u8> {
    let ones = u32::from(netmask).count_ones();
    (ones <= 32).then_some(ones as u8)
}

pub fn dedupe_rtsp_paths(paths: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let formatted = if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            format!("/{trimmed}")
        };
        if seen.insert(formatted.clone()) {
            normalized.push(formatted);
        }
    }
    normalized
}

pub fn generate_binding_code() -> String {
    let token = Uuid::new_v4().simple().to_string().to_uppercase();
    format!("{}-{}", &token[0..4], &token[4..8])
}

pub fn generate_qr_token(session_code: &str) -> String {
    format!("hub://bind/im_bridge/{session_code}")
}

pub fn normalize_binding_code(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_prefix = trimmed
        .strip_prefix("hub://bind/")
        .map(|value| value.rsplit('/').next().unwrap_or(value))
        .unwrap_or(trimmed)
        .trim();
    let compact: String = without_prefix
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    if compact.len() != 8 {
        return None;
    }

    Some(format!("{}-{}", &compact[0..4], &compact[4..8]))
}

pub fn derive_rtsp_hints(devices: &[CameraDevice]) -> Option<RtspHints> {
    let mut paths = Vec::new();
    for device in devices {
        let url = device.primary_stream.url.trim();
        if let Some((username, password, path)) = parse_rtsp_auth(url) {
            paths.push(path);
            return Some(RtspHints {
                username,
                password,
                paths: dedupe_rtsp_paths(paths),
            });
        }

        if let Some(path) = parse_rtsp_path(url) {
            paths.push(path);
        }
    }

    None
}

pub fn parse_rtsp_auth(url: &str) -> Option<(String, String, String)> {
    let without_scheme = url.strip_prefix("rtsp://")?;
    let at_index = without_scheme.find('@')?;
    let auth = &without_scheme[..at_index];
    let path = parse_rtsp_path(url)?;
    let mut parts = auth.splitn(2, ':');
    let username = parts.next()?.trim();
    let password = parts.next()?.trim();
    if username.is_empty() || password.is_empty() {
        return None;
    }
    Some((username.to_string(), password.to_string(), path))
}

pub fn parse_rtsp_path(url: &str) -> Option<String> {
    let without_scheme = url.strip_prefix("rtsp://")?;
    let slash_index = without_scheme.find('/')?;
    let path = &without_scheme[slash_index..];
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::control_plane::auth::{AuthSource, IdentityBinding};
    use crate::control_plane::media::RecordingTriggerMode;
    use crate::control_plane::users::{
        Membership, MembershipStatus, RoleKind, UserAccount, UserStatus,
    };
    use crate::runtime::registry::CameraDevice;
    use serde_json::json;

    use super::{
        build_platform_state, dedupe_rtsp_paths, derive_rtsp_hints, normalize_binding_code,
        normalize_loaded_admin_state, parse_rtsp_auth, parse_rtsp_path,
        resolved_identity_binding_records, resolved_remote_view_config,
        sanitize_bridge_provider_config, AdminConsoleStore, AdminDefaults,
        BridgeProviderCapabilities, BridgeProviderConfig, IdentityBindingRecord, RemoteViewConfig,
        BRIDGE_PROVIDER_ACCOUNT_ID, LOCAL_RTSP_CREDENTIAL_ID, LOCAL_RTSP_PROVIDER_ACCOUNT_ID,
    };

    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("harbornas-{name}-{unique}.json"))
    }

    #[test]
    fn parse_rtsp_auth_extracts_user_pass_and_path() {
        let parsed = parse_rtsp_auth("rtsp://admin:secret@192.168.3.73:554/ch1/main")
            .expect("auth should parse");
        assert_eq!(parsed.0, "admin");
        assert_eq!(parsed.1, "secret");
        assert_eq!(parsed.2, "/ch1/main");
    }

    #[test]
    fn parse_rtsp_path_handles_urls_without_auth() {
        let path = parse_rtsp_path("rtsp://192.168.3.73:554/Streaming/Channels/101").expect("path");
        assert_eq!(path, "/Streaming/Channels/101");
    }

    #[test]
    fn dedupe_rtsp_paths_normalizes_leading_slashes() {
        let paths = dedupe_rtsp_paths(vec![
            "ch1/main".to_string(),
            "/ch1/main".to_string(),
            " /Streaming/Channels/101 ".to_string(),
        ]);
        assert_eq!(paths, vec!["/ch1/main", "/Streaming/Channels/101"]);
    }

    #[test]
    fn derive_rtsp_hints_uses_first_authenticated_stream() {
        let device = CameraDevice::new(
            "cam-1",
            "Living Room",
            "rtsp://admin:MZBEHH@192.168.3.73:554/ch1/main",
        );
        let hints = derive_rtsp_hints(&[device]).expect("hints");
        assert_eq!(hints.username, "admin");
        assert_eq!(hints.password, "MZBEHH");
        assert_eq!(hints.paths, vec!["/ch1/main"]);
    }

    #[test]
    fn load_or_create_state_bootstraps_from_registry() {
        let registry_path = temp_path("registry");
        let admin_path = temp_path("admin");
        let registry = crate::runtime::registry::DeviceRegistryStore::new(registry_path.clone());
        let device = CameraDevice::new(
            "cam-1",
            "Living Room",
            "rtsp://admin:MZBEHH@192.168.3.73:554/ch1/main",
        );
        registry
            .save_devices(&[device])
            .expect("save device registry");

        let store = AdminConsoleStore::new(admin_path.clone(), registry);
        let state = store.load_or_create_state().expect("state");

        assert_eq!(state.defaults.rtsp_username, "admin");
        assert_eq!(state.defaults.rtsp_password, "MZBEHH");
        assert_eq!(state.platform.workspaces.len(), 1);
        assert_eq!(state.platform.users.len(), 1);
        assert_eq!(state.platform.memberships.len(), 1);
        assert!(!state.platform.permission_bindings.is_empty());
        assert_eq!(state.platform.provider_accounts.len(), 1);
        assert_eq!(state.platform.credentials.len(), 1);
        assert_eq!(state.platform.recording_policies.len(), 1);
        assert!(store.path().exists());

        let _ = std::fs::remove_file(admin_path);
        let _ = std::fs::remove_file(registry_path);
    }

    #[test]
    fn normalize_binding_code_accepts_token_or_compact_code() {
        assert_eq!(
            normalize_binding_code("hub://bind/feishu/5b86-a98f"),
            Some("5B86-A98F".to_string())
        );
        assert_eq!(
            normalize_binding_code("hub://bind/im_bridge/5b86-a98f"),
            Some("5B86-A98F".to_string())
        );
        assert_eq!(
            normalize_binding_code("5b86a98f"),
            Some("5B86-A98F".to_string())
        );
    }

    #[test]
    fn bind_identity_user_persists_mapping() {
        let registry_path = temp_path("registry-bind");
        let admin_path = temp_path("admin-bind");
        let registry = crate::runtime::registry::DeviceRegistryStore::new(registry_path.clone());
        let store = AdminConsoleStore::new(admin_path.clone(), registry);
        let state = store.load_or_create_state().expect("state");

        let updated = store
            .bind_identity_user(
                &state.binding.qr_token,
                IdentityBindingRecord {
                    open_id: "ou_demo".to_string(),
                    user_id: Some("u_demo".to_string()),
                    union_id: Some("on_demo".to_string()),
                    display_name: "Bean".to_string(),
                    chat_id: Some("oc_demo".to_string()),
                },
            )
            .expect("bind");

        assert_eq!(updated.binding.bound_user.as_deref(), Some("Bean"));
        assert_eq!(updated.identity_bindings.len(), 1);
        assert_eq!(updated.identity_bindings[0].open_id, "ou_demo");
        assert_eq!(updated.platform.users.len(), 2);
        assert_eq!(updated.platform.memberships.len(), 2);
        assert_eq!(updated.platform.identity_bindings.len(), 1);
        assert_eq!(
            updated.platform.identity_bindings[0].external_user_id,
            "ou_demo"
        );

        let _ = std::fs::remove_file(admin_path);
        let _ = std::fs::remove_file(registry_path);
    }

    #[test]
    fn save_defaults_returns_updated_platform_projection() {
        let registry_path = temp_path("registry-defaults");
        let admin_path = temp_path("admin-defaults");
        let registry = crate::runtime::registry::DeviceRegistryStore::new(registry_path.clone());
        let store = AdminConsoleStore::new(admin_path.clone(), registry);

        let updated = store
            .save_defaults(AdminDefaults {
                cidr: "10.42.0.0/24".to_string(),
                discovery: "ONVIF".to_string(),
                recording: "持续录制".to_string(),
                capture: "仅图片".to_string(),
                ai: "快速摘要".to_string(),
                notification_channel: "平台频道".to_string(),
                rtsp_username: "platform-user".to_string(),
                rtsp_password: "secret-rtsp".to_string(),
                rtsp_port: 8554,
                rtsp_paths: vec!["/alt/main".to_string()],
            })
            .expect("save defaults");

        assert_eq!(updated.platform.workspaces.len(), 1);
        assert_eq!(
            updated.platform.workspaces[0].settings["defaults"]["cidr"],
            json!("10.42.0.0/24")
        );
        assert!(updated.platform.provider_accounts.iter().any(|provider| {
            provider.provider_account_id == LOCAL_RTSP_PROVIDER_ACCOUNT_ID
                && provider.capabilities["rtsp_port"] == json!(8554)
        }));
        assert!(updated.platform.credentials.iter().any(|credential| {
            credential.credential_id == LOCAL_RTSP_CREDENTIAL_ID
                && credential.vault_key == "admin_console.defaults.rtsp_password"
        }));
        assert_eq!(
            updated.platform.recording_policies[0].trigger_mode,
            RecordingTriggerMode::Continuous
        );

        let reloaded = store.load_or_create_state().expect("reload");
        assert_eq!(reloaded.defaults.cidr, "10.42.0.0/24");
        assert_eq!(reloaded.defaults.rtsp_port, 8554);

        let _ = std::fs::remove_file(admin_path);
        let _ = std::fs::remove_file(registry_path);
    }

    #[test]
    fn save_remote_view_config_returns_updated_platform_projection() {
        let registry_path = temp_path("registry-remote-view");
        let admin_path = temp_path("admin-remote-view");
        let registry = crate::runtime::registry::DeviceRegistryStore::new(registry_path.clone());
        let store = AdminConsoleStore::new(admin_path.clone(), registry);

        let updated = store
            .save_remote_view_config(RemoteViewConfig {
                share_secret: "platform-share-secret".to_string(),
                share_link_ttl_minutes: 45,
            })
            .expect("save remote view");

        assert_eq!(updated.remote_view.share_secret, "platform-share-secret");
        assert_eq!(updated.remote_view.share_link_ttl_minutes, 45);
        assert_eq!(
            updated.platform.workspaces[0].settings["remote_view"]["share_secret"],
            json!("platform-share-secret")
        );
        assert_eq!(
            updated.platform.workspaces[0].settings["remote_view"]["share_link_ttl_minutes"],
            json!(45)
        );

        let reloaded = store.load_or_create_state().expect("reload");
        assert_eq!(reloaded.remote_view.share_secret, "platform-share-secret");
        assert_eq!(reloaded.remote_view.share_link_ttl_minutes, 45);
        assert_eq!(
            store
                .load_remote_view_config()
                .expect("resolved remote view"),
            RemoteViewConfig {
                share_secret: "platform-share-secret".to_string(),
                share_link_ttl_minutes: 45,
            }
        );

        let _ = std::fs::remove_file(admin_path);
        let _ = std::fs::remove_file(registry_path);
    }

    #[test]
    fn set_member_role_persists_custom_role() {
        let registry_path = temp_path("registry-role");
        let admin_path = temp_path("admin-role");
        let registry = crate::runtime::registry::DeviceRegistryStore::new(registry_path.clone());
        let store = AdminConsoleStore::new(admin_path.clone(), registry);
        let state = store.load_or_create_state().expect("state");

        store
            .bind_identity_user(
                &state.binding.qr_token,
                IdentityBindingRecord {
                    open_id: "ou_operator".to_string(),
                    user_id: Some("user-operator".to_string()),
                    union_id: None,
                    display_name: "Operator".to_string(),
                    chat_id: None,
                },
            )
            .expect("bind");

        let updated = store
            .set_member_role("user-operator", RoleKind::Operator)
            .expect("set role");

        assert!(updated.platform.memberships.iter().any(|membership| {
            membership.user_id == "user-operator" && membership.role_kind == RoleKind::Operator
        }));

        let reloaded = store.load_or_create_state().expect("reload");
        assert!(reloaded.platform.memberships.iter().any(|membership| {
            membership.user_id == "user-operator" && membership.role_kind == RoleKind::Operator
        }));

        let _ = std::fs::remove_file(admin_path);
        let _ = std::fs::remove_file(registry_path);
    }

    #[test]
    fn save_bridge_provider_status_returns_updated_platform_projection() {
        let registry_path = temp_path("registry-bridge");
        let admin_path = temp_path("admin-bridge");
        let registry = crate::runtime::registry::DeviceRegistryStore::new(registry_path.clone());
        let store = AdminConsoleStore::new(admin_path.clone(), registry);

        let updated = store
            .save_bridge_provider_status(BridgeProviderConfig {
                configured: true,
                connected: true,
                platform: "feishu".to_string(),
                gateway_base_url: "http://gateway.local:4180".to_string(),
                app_name: "HarborNAS Bot".to_string(),
                status: "已连接".to_string(),
                last_checked_at: "2026-04-18T10:00:00Z".to_string(),
                capabilities: BridgeProviderCapabilities {
                    reply: true,
                    update: true,
                    attachments: true,
                },
                ..Default::default()
            })
            .expect("save bridge provider status");

        assert_eq!(updated.binding.metric, "Gateway 在线");
        assert_eq!(updated.binding.bound_user.as_deref(), Some("HarborNAS Bot"));
        assert!(updated.platform.provider_accounts.iter().any(|provider| {
            provider.provider_account_id == BRIDGE_PROVIDER_ACCOUNT_ID
                && provider.metadata["platform"] == json!("feishu")
                && provider.metadata["display_name"] == json!("HarborNAS Bot")
        }));
        assert!(updated.platform.credentials.is_empty());

        let reloaded = store.load_or_create_state().expect("reload");
        assert_eq!(reloaded.bridge_provider.app_name, "HarborNAS Bot");
        assert_eq!(
            reloaded.bridge_provider.gateway_base_url,
            "http://gateway.local:4180"
        );
        assert_eq!(reloaded.bridge_provider.app_secret, "");
        assert_eq!(reloaded.bridge_provider.bot_open_id, "");

        let _ = std::fs::remove_file(admin_path);
        let _ = std::fs::remove_file(registry_path);
    }

    #[test]
    fn sanitize_bridge_provider_status_keeps_platform_empty_without_feishu_fallback() {
        let sanitized = sanitize_bridge_provider_config(BridgeProviderConfig {
            configured: false,
            connected: false,
            platform: "   ".to_string(),
            gateway_base_url: "  http://gateway.local:4180  ".to_string(),
            app_name: "  HarborNAS Bot  ".to_string(),
            ..Default::default()
        });

        assert_eq!(sanitized.platform, "");
        assert_eq!(sanitized.gateway_base_url, "http://gateway.local:4180");
        assert_eq!(sanitized.app_name, "HarborNAS Bot");
        assert_eq!(sanitized.status, "未配置");
    }

    #[test]
    fn platform_projection_adds_bridge_provider_without_secret_metadata() {
        let mut state = super::AdminConsoleState::default();
        state.bridge_provider = BridgeProviderConfig {
            configured: true,
            connected: true,
            platform: "feishu".to_string(),
            gateway_base_url: "http://gateway.local:4180".to_string(),
            app_name: "HarborNAS Bot".to_string(),
            status: "已连接".to_string(),
            last_checked_at: "2026-04-18T10:00:00Z".to_string(),
            capabilities: BridgeProviderCapabilities {
                reply: true,
                update: false,
                attachments: true,
            },
            ..Default::default()
        };
        state.identity_bindings.push(IdentityBindingRecord {
            open_id: "ou_demo".to_string(),
            user_id: Some("u_demo".to_string()),
            union_id: None,
            display_name: "Bean".to_string(),
            chat_id: None,
        });

        let platform = build_platform_state(&state);

        assert_eq!(platform.users.len(), 2);
        assert_eq!(platform.memberships.len(), 2);
        assert_eq!(platform.identity_bindings.len(), 1);
        assert!(!platform.permission_bindings.is_empty());
        assert_eq!(platform.provider_accounts.len(), 2);
        assert!(platform.credentials.is_empty());
        assert_eq!(platform.provider_accounts[1].provider_key, "im_bridge");
        assert_eq!(
            platform.provider_accounts[1].metadata["display_name"],
            json!("HarborNAS Bot")
        );
        assert_eq!(
            platform.provider_accounts[1].capabilities["reply"],
            json!(true)
        );
    }

    #[test]
    fn resolved_identity_binding_records_prefers_platform_projection() {
        let mut state = super::AdminConsoleState::default();
        state.identity_bindings.push(IdentityBindingRecord {
            open_id: "ou_legacy".to_string(),
            user_id: Some("legacy-user".to_string()),
            union_id: None,
            display_name: "Legacy".to_string(),
            chat_id: Some("oc_legacy".to_string()),
        });
        state.platform.users.push(UserAccount {
            user_id: "viewer-1".to_string(),
            display_name: "Viewer".to_string(),
            email: None,
            phone: None,
            status: UserStatus::Active,
            default_workspace_id: Some("home-1".to_string()),
            preferences: json!({}),
        });
        state.platform.identity_bindings.push(IdentityBinding {
            identity_id: "identity-ou_viewer".to_string(),
            user_id: "viewer-1".to_string(),
            auth_source: AuthSource::ImChannel,
            provider_key: "im_bridge".to_string(),
            external_user_id: "ou_viewer".to_string(),
            external_union_id: None,
            external_chat_id: Some("oc_viewer".to_string()),
            profile_snapshot: json!({
                "display_name": "Viewer",
            }),
            last_seen_at: None,
        });

        let bindings = resolved_identity_binding_records(&state);

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].open_id, "ou_viewer");
        assert_eq!(bindings[0].display_name, "Viewer");
        assert_eq!(bindings[0].chat_id.as_deref(), Some("oc_viewer"));
    }

    #[test]
    fn loaded_state_prefers_platform_projection_and_preserves_custom_memberships() {
        let mut state = super::AdminConsoleState::default();
        state.platform = build_platform_state(&state);
        state.remote_view.share_secret = "legacy-share-secret".to_string();
        state.platform.workspaces[0].settings = json!({
            "binding": {
                "channel": "Platform Bridge",
                "status": "平台已绑定",
                "metric": "平台在线",
                "bound_user": "Platform Admin",
            },
            "defaults": {
                "cidr": "10.42.0.0/24",
                "discovery": "ONVIF",
                "capture": "仅图片",
                "ai": "快速摘要",
                "notification_channel": "平台频道",
                "rtsp_port": 8554,
                "rtsp_paths": ["/alt/main"],
                "rtsp_username": "platform-user",
            },
            "remote_view": {
                "share_link_ttl_minutes": 45,
                "share_secret": "platform-share-secret",
                "share_secret_configured": true,
            }
        });
        state.platform.users.push(UserAccount {
            user_id: "viewer-1".to_string(),
            display_name: "Viewer".to_string(),
            email: None,
            phone: None,
            status: UserStatus::Active,
            default_workspace_id: Some("home-1".to_string()),
            preferences: json!({}),
        });
        state.platform.memberships.push(Membership {
            membership_id: "membership-viewer-1".to_string(),
            workspace_id: "home-1".to_string(),
            user_id: "viewer-1".to_string(),
            role_kind: RoleKind::Admin,
            status: MembershipStatus::Active,
            granted_by_user_id: Some("local-owner".to_string()),
            granted_at: None,
        });
        state.platform.identity_bindings.push(IdentityBinding {
            identity_id: "identity-ou_viewer".to_string(),
            user_id: "viewer-1".to_string(),
            auth_source: AuthSource::ImChannel,
            provider_key: "im_bridge".to_string(),
            external_user_id: "ou_viewer".to_string(),
            external_union_id: Some("on_viewer".to_string()),
            external_chat_id: Some("oc_viewer".to_string()),
            profile_snapshot: json!({
                "display_name": "Viewer",
            }),
            last_seen_at: None,
        });
        state.platform.recording_policies[0].metadata = json!({
            "recording_label": "持续录制",
            "capture_mode": "仅图片",
            "ai_mode": "快速摘要",
            "notification_channel": "平台频道",
        });

        normalize_loaded_admin_state(&mut state);

        assert_eq!(state.binding.channel, "Platform Bridge");
        assert_eq!(state.defaults.cidr, "10.42.0.0/24");
        assert_eq!(state.defaults.rtsp_port, 8554);
        assert_eq!(state.defaults.recording, "持续录制");
        assert_eq!(state.remote_view.share_link_ttl_minutes, 45);
        assert_eq!(state.remote_view.share_secret, "platform-share-secret");
        assert_eq!(state.identity_bindings.len(), 1);
        assert_eq!(state.identity_bindings[0].open_id, "ou_viewer");
        assert_eq!(state.identity_bindings[0].display_name, "Viewer");
        assert!(state.platform.memberships.iter().any(|membership| {
            membership.user_id == "viewer-1" && membership.role_kind == RoleKind::Admin
        }));
    }

    #[test]
    fn resolved_remote_view_config_prefers_workspace_projection() {
        let mut state = super::AdminConsoleState::default();
        state.remote_view = RemoteViewConfig {
            share_secret: "legacy-share-secret".to_string(),
            share_link_ttl_minutes: 120,
        };
        state.platform = build_platform_state(&state);
        state.platform.workspaces[0].settings["remote_view"] = json!({
            "share_secret": "platform-share-secret",
            "share_link_ttl_minutes": 30,
            "share_secret_configured": true,
        });

        let resolved = resolved_remote_view_config(&state);

        assert_eq!(
            resolved,
            RemoteViewConfig {
                share_secret: "platform-share-secret".to_string(),
                share_link_ttl_minutes: 30,
            }
        );
    }
}
