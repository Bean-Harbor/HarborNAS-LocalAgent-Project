//! Device registry, provider binding, capability, and digital twin schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::control_plane::credentials::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    #[default]
    Camera,
    Light,
    Sensor,
    Lock,
    Gateway,
    Nas,
    Service,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceLifecycleState {
    #[default]
    Discovered,
    Registered,
    Active,
    Disabled,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DeviceRecord {
    pub device_id: String,
    pub workspace_id: String,
    pub kind: DeviceKind,
    #[serde(default)]
    pub subtype: Option<String>,
    pub display_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub serial_number: Option<String>,
    #[serde(default)]
    pub mac_address: Option<String>,
    #[serde(default)]
    pub primary_room_id: Option<String>,
    pub lifecycle_state: DeviceLifecycleState,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceEndpointKind {
    #[default]
    Ipv4,
    Ipv6,
    Mac,
    Rtsp,
    Onvif,
    Matter,
    Http,
    Websocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReachabilityStatus {
    #[default]
    Online,
    Offline,
    Degraded,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DeviceEndpoint {
    pub endpoint_id: String,
    pub device_id: String,
    pub endpoint_kind: DeviceEndpointKind,
    #[serde(default)]
    pub scheme: String,
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub requires_auth: bool,
    pub reachability_status: ReachabilityStatus,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceSupportMode {
    #[default]
    Native,
    Cloud,
    Bridge,
    Lab,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBindingStatus {
    #[default]
    Active,
    Invalid,
    Pending,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProviderBinding {
    pub binding_id: String,
    pub device_id: String,
    #[serde(default)]
    pub provider_account_id: Option<String>,
    pub provider_key: String,
    pub provider_kind: ProviderKind,
    #[serde(default)]
    pub remote_device_id: Option<String>,
    #[serde(default)]
    pub credential_ref: Option<String>,
    pub binding_status: ProviderBindingStatus,
    pub support_mode: DeviceSupportMode,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub last_sync_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCategory {
    #[default]
    State,
    Control,
    Stream,
    Media,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAccessMode {
    #[default]
    Read,
    Write,
    Invoke,
    Subscribe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAvailability {
    #[default]
    Available,
    Unavailable,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CapabilityRecord {
    pub capability_id: String,
    pub device_id: String,
    pub capability_code: String,
    pub category: CapabilityCategory,
    pub access_mode: CapabilityAccessMode,
    pub support_mode: DeviceSupportMode,
    pub availability: CapabilityAvailability,
    #[serde(default)]
    pub source_binding_id: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectivityState {
    #[default]
    Online,
    Offline,
    Degraded,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DeviceTwin {
    pub device_id: String,
    pub connectivity_state: ConnectivityState,
    #[serde(default)]
    pub reported_state: Value,
    #[serde(default)]
    pub desired_state: Value,
    #[serde(default)]
    pub health_state: Value,
    #[serde(default)]
    pub last_event_id: Option<String>,
    #[serde(default)]
    pub last_seen_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CapabilityAccessMode, CapabilityAvailability, CapabilityCategory, CapabilityRecord,
        ConnectivityState, DeviceKind, DeviceLifecycleState, DeviceRecord, DeviceSupportMode,
        DeviceTwin,
    };

    #[test]
    fn device_record_preserves_aliases_and_metadata() {
        let device = DeviceRecord {
            device_id: "cam-1".to_string(),
            workspace_id: "home-1".to_string(),
            kind: DeviceKind::Camera,
            subtype: Some("ptz_camera".to_string()),
            display_name: "客厅摄像头".to_string(),
            aliases: vec!["客厅相机".to_string(), "客厅监控".to_string()],
            vendor: Some("Xiaomi".to_string()),
            model: Some("C22".to_string()),
            serial_number: None,
            mac_address: Some("B8:88:80:AF:C7:2C".to_string()),
            primary_room_id: Some("room-1".to_string()),
            lifecycle_state: DeviceLifecycleState::Registered,
            source: "discovered".to_string(),
            metadata: json!({"discovery_source": "miio"}),
        };

        assert_eq!(device.aliases.len(), 2);
        assert_eq!(device.metadata["discovery_source"], "miio");
    }

    #[test]
    fn device_twin_tracks_reported_and_desired_state() {
        let twin = DeviceTwin {
            device_id: "light-1".to_string(),
            connectivity_state: ConnectivityState::Online,
            reported_state: json!({"power": "off"}),
            desired_state: json!({"power": "on"}),
            health_state: json!({"battery": "good"}),
            last_event_id: Some("evt-1".to_string()),
            last_seen_at: Some("2026-04-15T08:00:00Z".to_string()),
        };

        let capability = CapabilityRecord {
            capability_id: "cap-1".to_string(),
            device_id: "light-1".to_string(),
            capability_code: "switch".to_string(),
            category: CapabilityCategory::Control,
            access_mode: CapabilityAccessMode::Write,
            support_mode: DeviceSupportMode::Native,
            availability: CapabilityAvailability::Available,
            source_binding_id: Some("binding-1".to_string()),
            metadata: json!({}),
        };

        assert_eq!(twin.desired_state["power"], "on");
        assert_eq!(capability.capability_code, "switch");
    }
}
