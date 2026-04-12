//! Thin persistence layer for the local Agent Hub admin console.

use std::collections::HashSet;
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use if_addrs::{get_if_addrs, IfAddr};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::registry::{CameraDevice, DeviceRegistryStore};

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
            channel: "飞书 HarborNAS Bot".to_string(),
            session_code: session_code.clone(),
            qr_token: generate_qr_token(&session_code),
            setup_url: String::new(),
            static_setup_url: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FeishuBotConfig {
    #[serde(default)]
    pub configured: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeishuUserBinding {
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
    pub feishu_group: String,
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
            feishu_group: "客厅安全群".to_string(),
            rtsp_username: default_rtsp_username(),
            rtsp_password: String::new(),
            rtsp_port: default_rtsp_port(),
            rtsp_paths: default_rtsp_paths(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AdminConsoleState {
    #[serde(default)]
    pub binding: AdminBindingState,
    #[serde(default)]
    pub defaults: AdminDefaults,
    #[serde(default)]
    pub feishu_bot: FeishuBotConfig,
    #[serde(default)]
    pub feishu_users: Vec<FeishuUserBinding>,
}

#[derive(Debug, Clone)]
pub struct AdminConsoleStore {
    path: PathBuf,
    registry_store: DeviceRegistryStore,
}

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

    pub fn save_state(&self, state: &AdminConsoleState) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "failed to create admin console directory {}: {e}",
                    parent.display()
                )
            })?;
        }

        let payload = serde_json::to_string_pretty(state).map_err(|e| {
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

    pub fn load_or_create_state(&self) -> Result<AdminConsoleState, String> {
        let state = self.load_state()?;
        if !self.path.exists() {
            self.save_state(&state)?;
        }
        Ok(state)
    }

    pub fn refresh_binding_qr(&self) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.binding = AdminBindingState::default();
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn mark_demo_bound(&self, user_name: &str) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.binding.status = "已绑定".to_string();
        state.binding.metric = "已绑定".to_string();
        state.binding.bound_user = Some(user_name.to_string());
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn bind_feishu_user(
        &self,
        token_or_code: &str,
        user: FeishuUserBinding,
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

        state.binding.status = "已绑定".to_string();
        state.binding.metric = "已绑定".to_string();
        state.binding.bound_user = Some(user.display_name.clone());

        if let Some(existing) = state
            .feishu_users
            .iter_mut()
            .find(|existing| existing.open_id == user.open_id)
        {
            *existing = user;
        } else {
            state.feishu_users.push(user);
        }

        self.save_state(&state)?;
        Ok(state)
    }

    pub fn save_defaults(&self, defaults: AdminDefaults) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.defaults = sanitize_defaults(defaults);
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn save_feishu_bot_config(
        &self,
        config: FeishuBotConfig,
    ) -> Result<AdminConsoleState, String> {
        let mut state = self.load_or_create_state()?;
        state.feishu_bot = sanitize_feishu_bot_config(config);
        if state.feishu_bot.configured {
            state.binding.status = "Bot 已配置".to_string();
            state.binding.metric = "Bot 已连接".to_string();
            if !state.feishu_bot.app_name.trim().is_empty() {
                state.binding.bound_user = Some(state.feishu_bot.app_name.clone());
            }
        }
        self.save_state(&state)?;
        Ok(state)
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

        state.binding = sanitize_binding(state.binding.clone());
        state.defaults = sanitize_defaults(state.defaults.clone());
        state.feishu_bot = sanitize_feishu_bot_config(state.feishu_bot.clone());
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
    if defaults.feishu_group.trim().is_empty() {
        defaults.feishu_group = "HarborNAS Bot".to_string();
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
        binding.channel = "飞书 HarborNAS Bot".to_string();
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

pub fn sanitize_feishu_bot_config(mut config: FeishuBotConfig) -> FeishuBotConfig {
    if config.status.trim().is_empty() {
        config.status = if config.configured {
            "已连接".to_string()
        } else {
            "未配置".to_string()
        };
    }
    config
}

pub fn default_rtsp_username() -> String {
    "admin".to_string()
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
    format!("hub://bind/feishu/{session_code}")
}

pub fn normalize_binding_code(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_prefix = trimmed
        .strip_prefix("hub://bind/feishu/")
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

    use crate::runtime::registry::CameraDevice;

    use super::{
        dedupe_rtsp_paths, derive_rtsp_hints, normalize_binding_code, parse_rtsp_auth,
        parse_rtsp_path, AdminConsoleStore, FeishuUserBinding,
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
            normalize_binding_code("5b86a98f"),
            Some("5B86-A98F".to_string())
        );
    }

    #[test]
    fn bind_feishu_user_persists_mapping() {
        let registry_path = temp_path("registry-bind");
        let admin_path = temp_path("admin-bind");
        let registry = crate::runtime::registry::DeviceRegistryStore::new(registry_path.clone());
        let store = AdminConsoleStore::new(admin_path.clone(), registry);
        let state = store.load_or_create_state().expect("state");

        let updated = store
            .bind_feishu_user(
                &state.binding.qr_token,
                FeishuUserBinding {
                    open_id: "ou_demo".to_string(),
                    user_id: Some("u_demo".to_string()),
                    union_id: Some("on_demo".to_string()),
                    display_name: "Bean".to_string(),
                    chat_id: Some("oc_demo".to_string()),
                },
            )
            .expect("bind");

        assert_eq!(updated.binding.bound_user.as_deref(), Some("Bean"));
        assert_eq!(updated.feishu_users.len(), 1);
        assert_eq!(updated.feishu_users[0].open_id, "ou_demo");

        let _ = std::fs::remove_file(admin_path);
        let _ = std::fs::remove_file(registry_path);
    }
}
