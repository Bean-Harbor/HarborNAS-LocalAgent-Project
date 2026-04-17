//! Device registry and runtime metadata cache.

use std::fs;
use std::path::{Path, PathBuf};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::control_plane::credentials::ProviderKind;
use crate::control_plane::devices::{
    CapabilityAccessMode, CapabilityAvailability, CapabilityCategory, CapabilityRecord,
    ConnectivityState, DeviceEndpoint, DeviceEndpointKind, DeviceLifecycleState,
    DeviceRecord as ControlDeviceRecord, DeviceSupportMode, DeviceTwin, ProviderBinding,
    ProviderBindingStatus, ReachabilityStatus,
};
use crate::control_plane::media::{
    CameraProfile, StreamProfile, StreamTransport as ControlStreamTransport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Camera,
    Light,
    Sensor,
    Lock,
    Gateway,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    #[default]
    Online,
    Offline,
    Degraded,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamTransport {
    Rtsp,
    Hls,
    Webrtc,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CameraStreamRef {
    pub transport: StreamTransport,
    pub url: String,
    #[serde(default)]
    pub requires_auth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CameraCapabilities {
    #[serde(default)]
    pub snapshot: bool,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub ptz: bool,
    #[serde(default)]
    pub audio: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CameraDevice {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub status: DeviceStatus,
    pub room: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub ip_address: Option<String>,
    pub mac_address: Option<String>,
    pub discovery_source: String,
    pub primary_stream: CameraStreamRef,
    #[serde(default)]
    pub onvif_device_service_url: Option<String>,
    #[serde(default)]
    pub ezviz_device_serial: Option<String>,
    #[serde(default)]
    pub ezviz_camera_no: Option<u32>,
    #[serde(default)]
    pub capabilities: CameraCapabilities,
    #[serde(default)]
    pub last_seen_at: Option<String>,
}

impl CameraDevice {
    pub fn new(
        device_id: impl Into<String>,
        name: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            name: name.into(),
            kind: DeviceKind::Camera,
            status: DeviceStatus::Unknown,
            room: None,
            vendor: None,
            model: None,
            ip_address: None,
            mac_address: None,
            discovery_source: "unknown".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: url.into(),
                requires_auth: false,
            },
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities::default(),
            last_seen_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedCameraTarget {
    pub device_id: String,
    #[serde(alias = "name")]
    pub display_name: String,
    #[serde(default)]
    pub status: DeviceStatus,
    #[serde(default)]
    #[serde(alias = "room")]
    pub room_name: Option<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub mac_address: Option<String>,
    #[serde(default)]
    pub discovery_source: String,
    pub primary_stream: CameraStreamRef,
    #[serde(default)]
    pub onvif_device_service_url: Option<String>,
    #[serde(default)]
    pub ezviz_device_serial: Option<String>,
    #[serde(default)]
    pub ezviz_camera_no: Option<u32>,
    #[serde(default)]
    pub capabilities: CameraCapabilities,
    #[serde(default)]
    pub last_seen_at: Option<String>,
}

impl From<ResolvedCameraTarget> for CameraDevice {
    fn from(target: ResolvedCameraTarget) -> Self {
        Self {
            device_id: target.device_id,
            name: target.display_name,
            kind: DeviceKind::Camera,
            status: target.status,
            room: target.room_name,
            vendor: target.vendor,
            model: target.model,
            ip_address: target.ip_address,
            mac_address: target.mac_address,
            discovery_source: target.discovery_source,
            primary_stream: target.primary_stream,
            onvif_device_service_url: target.onvif_device_service_url,
            ezviz_device_serial: target.ezviz_device_serial,
            ezviz_camera_no: target.ezviz_camera_no,
            capabilities: target.capabilities,
            last_seen_at: target.last_seen_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceRegistrySnapshot {
    #[serde(default = "default_registry_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub devices: Vec<ControlDeviceRecord>,
    #[serde(default)]
    pub device_endpoints: Vec<DeviceEndpoint>,
    #[serde(default)]
    pub provider_bindings: Vec<ProviderBinding>,
    #[serde(default)]
    pub capabilities: Vec<CapabilityRecord>,
    #[serde(default)]
    pub device_twins: Vec<DeviceTwin>,
    #[serde(default)]
    pub camera_profiles: Vec<CameraProfile>,
    #[serde(default)]
    pub stream_profiles: Vec<StreamProfile>,
}

impl DeviceRegistrySnapshot {
    pub fn from_camera_devices(devices: &[CameraDevice]) -> Self {
        let mut snapshot = Self::default();

        for camera in devices {
            snapshot.devices.push(ControlDeviceRecord {
                device_id: camera.device_id.clone(),
                workspace_id: String::new(),
                kind: legacy_device_kind_to_control(camera.kind),
                subtype: camera.capabilities.ptz.then(|| "ptz_camera".to_string()),
                display_name: camera.name.clone(),
                aliases: Vec::new(),
                vendor: camera.vendor.clone(),
                model: camera.model.clone(),
                serial_number: None,
                mac_address: camera.mac_address.clone(),
                primary_room_id: camera.room.clone(),
                lifecycle_state: DeviceLifecycleState::Registered,
                source: camera.discovery_source.clone(),
                metadata: json!({}),
            });

            snapshot.device_twins.push(DeviceTwin {
                device_id: camera.device_id.clone(),
                connectivity_state: legacy_status_to_connectivity(camera.status),
                reported_state: json!({}),
                desired_state: json!({}),
                health_state: json!({}),
                last_event_id: None,
                last_seen_at: camera.last_seen_at.clone(),
            });

            if let Some(ip_address) = &camera.ip_address {
                snapshot.device_endpoints.push(DeviceEndpoint {
                    endpoint_id: format!("{}::endpoint::ipv4", camera.device_id),
                    device_id: camera.device_id.clone(),
                    endpoint_kind: DeviceEndpointKind::Ipv4,
                    scheme: "ipv4".to_string(),
                    host: ip_address.clone(),
                    port: None,
                    path: None,
                    requires_auth: false,
                    reachability_status: legacy_status_to_reachability(camera.status),
                    last_seen_at: camera.last_seen_at.clone(),
                    metadata: json!({}),
                });
            }

            let primary_endpoint_id = format!("{}::endpoint::primary_stream", camera.device_id);
            snapshot.device_endpoints.push(stream_endpoint_from_camera(
                camera,
                primary_endpoint_id.clone(),
            ));

            snapshot.stream_profiles.push(StreamProfile {
                stream_profile_id: format!("{}::stream::primary", camera.device_id),
                device_id: camera.device_id.clone(),
                profile_name: "primary".to_string(),
                transport: legacy_stream_transport_to_control(camera.primary_stream.transport),
                endpoint_id: primary_endpoint_id.clone(),
                video_codec: None,
                audio_codec: camera.capabilities.audio.then(|| "unknown".to_string()),
                width: None,
                height: None,
                fps: None,
                bitrate_kbps: None,
                is_default: true,
            });

            snapshot.camera_profiles.push(CameraProfile {
                device_id: camera.device_id.clone(),
                default_stream_profile_id: Some(format!("{}::stream::primary", camera.device_id)),
                audio_supported: camera.capabilities.audio,
                ptz_supported: camera.capabilities.ptz,
                privacy_supported: false,
                playback_supported: false,
                recording_policy_id: None,
                vendor_features: json!({}),
            });

            snapshot
                .capabilities
                .extend(capability_records_from_camera(camera));
            snapshot
                .provider_bindings
                .extend(provider_bindings_from_camera(camera));
        }

        snapshot
    }

    pub fn into_camera_devices(self) -> Vec<CameraDevice> {
        self.to_camera_devices()
    }

    pub fn camera_target(&self, device_id: &str) -> Option<ResolvedCameraTarget> {
        self.devices
            .iter()
            .find(|device| {
                device.device_id == device_id
                    && matches!(
                        device.kind,
                        crate::control_plane::devices::DeviceKind::Camera
                    )
            })
            .and_then(|device| self.resolved_camera_target(device))
    }

    pub fn camera_targets(&self) -> Vec<ResolvedCameraTarget> {
        self.devices
            .iter()
            .filter_map(|device| self.resolved_camera_target(device))
            .collect()
    }

    pub fn upsert_camera_devices_preserving_platform_records(&mut self, devices: &[CameraDevice]) {
        let normalized_devices: Vec<CameraDevice> = devices
            .iter()
            .cloned()
            .map(normalize_camera_metadata)
            .collect();

        for incoming in normalized_devices {
            let resolved_device_id = self
                .find_matching_camera_device_id(&incoming)
                .unwrap_or_else(|| incoming.device_id.clone());
            let mut canonical = incoming;
            canonical.device_id = resolved_device_id;
            self.upsert_camera_device(&canonical);
        }
    }

    pub fn replace_camera_devices_preserving_platform_records(&mut self, devices: &[CameraDevice]) {
        let mut keep_device_ids = Vec::with_capacity(devices.len());
        let normalized_devices: Vec<CameraDevice> = devices
            .iter()
            .cloned()
            .map(normalize_camera_metadata)
            .collect();
        for incoming in &normalized_devices {
            let resolved_device_id = self
                .find_matching_camera_device_id(incoming)
                .unwrap_or_else(|| incoming.device_id.clone());
            keep_device_ids.push(resolved_device_id);
        }

        self.upsert_camera_devices_preserving_platform_records(&normalized_devices);

        self.prune_missing_camera_devices(&keep_device_ids);
    }

    pub fn to_camera_devices(&self) -> Vec<CameraDevice> {
        self.camera_targets().into_iter().map(Into::into).collect()
    }

    fn resolved_camera_target(&self, device: &ControlDeviceRecord) -> Option<ResolvedCameraTarget> {
        if !matches!(
            device.kind,
            crate::control_plane::devices::DeviceKind::Camera
        ) {
            return None;
        }

        let camera_profile = self
            .camera_profiles
            .iter()
            .find(|profile| profile.device_id == device.device_id);
        let stream_profile = preferred_stream_profile(
            &self.stream_profiles,
            &device.device_id,
            camera_profile.and_then(|profile| profile.default_stream_profile_id.as_deref()),
        )?;
        let endpoint = self
            .device_endpoints
            .iter()
            .find(|item| item.endpoint_id == stream_profile.endpoint_id)?;
        let twin = self
            .device_twins
            .iter()
            .find(|item| item.device_id == device.device_id);
        let binding_set: Vec<&ProviderBinding> = self
            .provider_bindings
            .iter()
            .filter(|binding| binding.device_id == device.device_id)
            .collect();
        let capability_set: Vec<&CapabilityRecord> = self
            .capabilities
            .iter()
            .filter(|capability| capability.device_id == device.device_id)
            .collect();
        let discovery_source = if device.source.trim().is_empty() {
            infer_discovery_source(&binding_set)
        } else {
            device.source.clone()
        };

        Some(ResolvedCameraTarget {
            device_id: device.device_id.clone(),
            display_name: device.display_name.clone(),
            status: twin
                .map(|item| connectivity_to_legacy_status(item.connectivity_state))
                .unwrap_or(DeviceStatus::Unknown),
            room_name: device.primary_room_id.clone(),
            vendor: device.vendor.clone(),
            model: device.model.clone(),
            ip_address: self
                .device_endpoints
                .iter()
                .find(|item| {
                    item.device_id == device.device_id
                        && matches!(item.endpoint_kind, DeviceEndpointKind::Ipv4)
                })
                .map(|item| item.host.clone()),
            mac_address: device.mac_address.clone(),
            discovery_source,
            primary_stream: CameraStreamRef {
                transport: control_stream_transport_to_legacy(stream_profile.transport),
                url: stream_url_from_endpoint(endpoint),
                requires_auth: endpoint.requires_auth,
            },
            onvif_device_service_url: binding_set
                .iter()
                .find(|binding| binding.provider_key == "onvif")
                .and_then(|binding| {
                    binding
                        .metadata
                        .get("device_service_url")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                }),
            ezviz_device_serial: binding_set
                .iter()
                .find(|binding| binding.provider_key == "ezviz")
                .and_then(|binding| binding.remote_device_id.clone()),
            ezviz_camera_no: binding_set
                .iter()
                .find(|binding| binding.provider_key == "ezviz")
                .and_then(|binding| {
                    binding
                        .metadata
                        .get("camera_no")
                        .and_then(Value::as_u64)
                        .map(|value| value as u32)
                }),
            capabilities: camera_capabilities_from_records(&capability_set),
            last_seen_at: twin.and_then(|item| item.last_seen_at.clone()),
        })
    }

    fn find_matching_camera_device_id(&self, incoming: &CameraDevice) -> Option<String> {
        self.to_camera_devices()
            .into_iter()
            .find(|existing| same_camera_view(existing, incoming))
            .map(|device| device.device_id)
    }

    fn upsert_camera_device(&mut self, camera: &CameraDevice) {
        self.upsert_camera_record(camera);
        self.upsert_camera_twin(camera);
        self.replace_managed_camera_endpoints(camera);
        self.replace_managed_camera_stream_profiles(camera);
        self.upsert_camera_profile(camera);
        self.replace_managed_camera_capabilities(camera);
        self.replace_managed_provider_bindings(camera);
    }

    fn upsert_camera_record(&mut self, camera: &CameraDevice) {
        if let Some(existing) = self
            .devices
            .iter_mut()
            .find(|device| device.device_id == camera.device_id)
        {
            existing.kind = legacy_device_kind_to_control(camera.kind);
            if !camera.name.trim().is_empty() {
                existing.display_name = camera.name.clone();
            }
            if camera.capabilities.ptz && existing.subtype.is_none() {
                existing.subtype = Some("ptz_camera".to_string());
            }
            if camera.vendor.is_some() {
                existing.vendor = camera.vendor.clone();
            }
            if camera.model.is_some() {
                existing.model = camera.model.clone();
            }
            if camera.mac_address.is_some() {
                existing.mac_address = camera.mac_address.clone();
            }
            if camera.room.is_some() {
                existing.primary_room_id = camera.room.clone();
            }
            existing.lifecycle_state = DeviceLifecycleState::Registered;
            if !camera.discovery_source.trim().is_empty() {
                existing.source = normalize_discovery_source(&camera.discovery_source).to_string();
            }
        } else {
            self.devices.push(ControlDeviceRecord {
                device_id: camera.device_id.clone(),
                workspace_id: String::new(),
                kind: legacy_device_kind_to_control(camera.kind),
                subtype: camera.capabilities.ptz.then(|| "ptz_camera".to_string()),
                display_name: camera.name.clone(),
                aliases: Vec::new(),
                vendor: camera.vendor.clone(),
                model: camera.model.clone(),
                serial_number: None,
                mac_address: camera.mac_address.clone(),
                primary_room_id: camera.room.clone(),
                lifecycle_state: DeviceLifecycleState::Registered,
                source: normalize_discovery_source(&camera.discovery_source).to_string(),
                metadata: json!({}),
            });
        }
    }

    fn upsert_camera_twin(&mut self, camera: &CameraDevice) {
        if let Some(existing) = self
            .device_twins
            .iter_mut()
            .find(|twin| twin.device_id == camera.device_id)
        {
            existing.connectivity_state = legacy_status_to_connectivity(camera.status);
            if camera.last_seen_at.is_some() {
                existing.last_seen_at = camera.last_seen_at.clone();
            }
        } else {
            self.device_twins.push(DeviceTwin {
                device_id: camera.device_id.clone(),
                connectivity_state: legacy_status_to_connectivity(camera.status),
                reported_state: json!({}),
                desired_state: json!({}),
                health_state: json!({}),
                last_event_id: None,
                last_seen_at: camera.last_seen_at.clone(),
            });
        }
    }

    fn replace_managed_camera_endpoints(&mut self, camera: &CameraDevice) {
        let managed_ids = managed_camera_endpoint_ids(&camera.device_id);
        self.device_endpoints
            .retain(|endpoint| !managed_ids.iter().any(|id| id == &endpoint.endpoint_id));

        if let Some(ip_address) = &camera.ip_address {
            self.device_endpoints.push(DeviceEndpoint {
                endpoint_id: managed_camera_ipv4_endpoint_id(&camera.device_id),
                device_id: camera.device_id.clone(),
                endpoint_kind: DeviceEndpointKind::Ipv4,
                scheme: "ipv4".to_string(),
                host: ip_address.clone(),
                port: None,
                path: None,
                requires_auth: false,
                reachability_status: legacy_status_to_reachability(camera.status),
                last_seen_at: camera.last_seen_at.clone(),
                metadata: json!({}),
            });
        }

        self.device_endpoints.push(stream_endpoint_from_camera(
            camera,
            managed_camera_primary_stream_endpoint_id(&camera.device_id),
        ));
    }

    fn replace_managed_camera_stream_profiles(&mut self, camera: &CameraDevice) {
        let primary_stream_profile_id = managed_camera_primary_stream_profile_id(&camera.device_id);
        self.stream_profiles.retain(|profile| {
            !(profile.device_id == camera.device_id
                && profile.stream_profile_id == primary_stream_profile_id)
        });

        self.stream_profiles.push(StreamProfile {
            stream_profile_id: primary_stream_profile_id,
            device_id: camera.device_id.clone(),
            profile_name: "primary".to_string(),
            transport: legacy_stream_transport_to_control(camera.primary_stream.transport),
            endpoint_id: managed_camera_primary_stream_endpoint_id(&camera.device_id),
            video_codec: None,
            audio_codec: camera.capabilities.audio.then(|| "unknown".to_string()),
            width: None,
            height: None,
            fps: None,
            bitrate_kbps: None,
            is_default: true,
        });
    }

    fn upsert_camera_profile(&mut self, camera: &CameraDevice) {
        let primary_stream_profile_id = managed_camera_primary_stream_profile_id(&camera.device_id);
        if let Some(existing) = self
            .camera_profiles
            .iter_mut()
            .find(|profile| profile.device_id == camera.device_id)
        {
            existing.default_stream_profile_id = Some(primary_stream_profile_id);
            existing.audio_supported = existing.audio_supported || camera.capabilities.audio;
            existing.ptz_supported = existing.ptz_supported || camera.capabilities.ptz;
        } else {
            self.camera_profiles.push(CameraProfile {
                device_id: camera.device_id.clone(),
                default_stream_profile_id: Some(primary_stream_profile_id),
                audio_supported: camera.capabilities.audio,
                ptz_supported: camera.capabilities.ptz,
                privacy_supported: false,
                playback_supported: false,
                recording_policy_id: None,
                vendor_features: json!({}),
            });
        }
    }

    fn replace_managed_camera_capabilities(&mut self, camera: &CameraDevice) {
        self.capabilities.retain(|capability| {
            !(capability.device_id == camera.device_id
                && matches!(
                    capability.capability_code.as_str(),
                    "snapshot" | "stream_live" | "ptz" | "audio"
                ))
        });
        self.capabilities
            .extend(capability_records_from_camera(camera));
    }

    fn replace_managed_provider_bindings(&mut self, camera: &CameraDevice) {
        let managed_ids = managed_provider_binding_ids(&camera.device_id);
        self.provider_bindings
            .retain(|binding| !managed_ids.iter().any(|id| id == &binding.binding_id));
        self.provider_bindings
            .extend(provider_bindings_from_camera(camera));
    }

    fn prune_missing_camera_devices(&mut self, keep_device_ids: &[String]) {
        let remove_device_ids: Vec<String> = self
            .devices
            .iter()
            .filter(|device| {
                matches!(
                    device.kind,
                    crate::control_plane::devices::DeviceKind::Camera
                ) && !keep_device_ids.iter().any(|id| id == &device.device_id)
            })
            .map(|device| device.device_id.clone())
            .collect();

        if remove_device_ids.is_empty() {
            return;
        }

        self.devices
            .retain(|device| !remove_device_ids.iter().any(|id| id == &device.device_id));
        self.device_endpoints
            .retain(|endpoint| !remove_device_ids.iter().any(|id| id == &endpoint.device_id));
        self.provider_bindings
            .retain(|binding| !remove_device_ids.iter().any(|id| id == &binding.device_id));
        self.capabilities.retain(|capability| {
            !remove_device_ids
                .iter()
                .any(|id| id == &capability.device_id)
        });
        self.device_twins
            .retain(|twin| !remove_device_ids.iter().any(|id| id == &twin.device_id));
        self.camera_profiles
            .retain(|profile| !remove_device_ids.iter().any(|id| id == &profile.device_id));
        self.stream_profiles
            .retain(|profile| !remove_device_ids.iter().any(|id| id == &profile.device_id));
    }
}

impl Default for DeviceRegistrySnapshot {
    fn default() -> Self {
        Self {
            schema_version: default_registry_schema_version(),
            devices: Vec::new(),
            device_endpoints: Vec::new(),
            provider_bindings: Vec::new(),
            capabilities: Vec::new(),
            device_twins: Vec::new(),
            camera_profiles: Vec::new(),
            stream_profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum DeviceRegistryPayload {
    Legacy(Vec<CameraDevice>),
    Snapshot(DeviceRegistrySnapshot),
}

#[derive(Debug, Clone)]
pub struct DeviceRegistryStore {
    path: PathBuf,
}

impl DeviceRegistryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_snapshot(&self) -> Result<DeviceRegistrySnapshot, String> {
        if !self.path.exists() {
            return Ok(empty_registry_snapshot());
        }

        let text = self.read_registry_text()?;
        let payload: DeviceRegistryPayload = serde_json::from_str(&text).map_err(|e| {
            format!(
                "failed to parse device registry {}: {e}",
                self.path.display()
            )
        })?;
        Ok(match payload {
            DeviceRegistryPayload::Legacy(devices) => {
                DeviceRegistrySnapshot::from_camera_devices(&devices)
            }
            DeviceRegistryPayload::Snapshot(snapshot) => snapshot,
        })
    }

    pub fn save_snapshot(&self, snapshot: &DeviceRegistrySnapshot) -> Result<(), String> {
        self.ensure_registry_parent()?;
        let payload = serde_json::to_string_pretty(snapshot).map_err(|e| {
            format!(
                "failed to serialize device registry {}: {e}",
                self.path.display()
            )
        })?;
        fs::write(&self.path, payload).map_err(|e| {
            format!(
                "failed to write device registry {}: {e}",
                self.path.display()
            )
        })
    }

    pub fn load_devices(&self) -> Result<Vec<CameraDevice>, String> {
        Ok(self.load_snapshot()?.into_camera_devices())
    }

    pub fn load_camera_targets(&self) -> Result<Vec<ResolvedCameraTarget>, String> {
        Ok(self.load_snapshot()?.camera_targets())
    }

    pub fn resolve_camera_target(&self, device_id: &str) -> Result<ResolvedCameraTarget, String> {
        self.load_snapshot()?
            .camera_target(device_id)
            .ok_or_else(|| format!("device not found in registry: {device_id}"))
    }

    pub fn save_devices(&self, devices: &[CameraDevice]) -> Result<(), String> {
        let mut snapshot = self.load_snapshot()?;
        if snapshot.schema_version == 0 {
            snapshot.schema_version = default_registry_schema_version();
        }
        snapshot.replace_camera_devices_preserving_platform_records(devices);
        self.save_snapshot(&snapshot)
    }

    pub fn upsert_devices(
        &self,
        devices: &[CameraDevice],
    ) -> Result<DeviceRegistrySnapshot, String> {
        let mut snapshot = self.load_snapshot()?;
        if snapshot.schema_version == 0 {
            snapshot.schema_version = default_registry_schema_version();
        }
        snapshot.upsert_camera_devices_preserving_platform_records(devices);
        self.save_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn ensure_registry_parent(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "failed to create device registry directory {}: {e}",
                    parent.display()
                )
            })?;
        }
        Ok(())
    }

    fn read_registry_text(&self) -> Result<String, String> {
        fs::read_to_string(&self.path).map_err(|e| {
            format!(
                "failed to read device registry {}: {e}",
                self.path.display()
            )
        })
    }
}

fn default_registry_schema_version() -> u32 {
    1
}

fn empty_registry_snapshot() -> DeviceRegistrySnapshot {
    DeviceRegistrySnapshot::default()
}

fn managed_camera_ipv4_endpoint_id(device_id: &str) -> String {
    format!("{device_id}::endpoint::ipv4")
}

fn managed_camera_primary_stream_endpoint_id(device_id: &str) -> String {
    format!("{device_id}::endpoint::primary_stream")
}

fn managed_camera_primary_stream_profile_id(device_id: &str) -> String {
    format!("{device_id}::stream::primary")
}

fn managed_camera_endpoint_ids(device_id: &str) -> [String; 2] {
    [
        managed_camera_ipv4_endpoint_id(device_id),
        managed_camera_primary_stream_endpoint_id(device_id),
    ]
}

fn managed_provider_binding_ids(device_id: &str) -> [String; 3] {
    [
        format!("{device_id}::binding::stream"),
        format!("{device_id}::binding::onvif"),
        format!("{device_id}::binding::ezviz"),
    ]
}

fn legacy_device_kind_to_control(kind: DeviceKind) -> crate::control_plane::devices::DeviceKind {
    match kind {
        DeviceKind::Camera => crate::control_plane::devices::DeviceKind::Camera,
        DeviceKind::Light => crate::control_plane::devices::DeviceKind::Light,
        DeviceKind::Sensor => crate::control_plane::devices::DeviceKind::Sensor,
        DeviceKind::Lock => crate::control_plane::devices::DeviceKind::Lock,
        DeviceKind::Gateway => crate::control_plane::devices::DeviceKind::Gateway,
        DeviceKind::Unknown => crate::control_plane::devices::DeviceKind::Unknown,
    }
}

fn legacy_status_to_connectivity(status: DeviceStatus) -> ConnectivityState {
    match status {
        DeviceStatus::Online => ConnectivityState::Online,
        DeviceStatus::Offline => ConnectivityState::Offline,
        DeviceStatus::Degraded => ConnectivityState::Degraded,
        DeviceStatus::Unknown => ConnectivityState::Unknown,
    }
}

fn connectivity_to_legacy_status(state: ConnectivityState) -> DeviceStatus {
    match state {
        ConnectivityState::Online => DeviceStatus::Online,
        ConnectivityState::Offline => DeviceStatus::Offline,
        ConnectivityState::Degraded => DeviceStatus::Degraded,
        ConnectivityState::Unknown => DeviceStatus::Unknown,
    }
}

fn legacy_status_to_reachability(status: DeviceStatus) -> ReachabilityStatus {
    match status {
        DeviceStatus::Online => ReachabilityStatus::Online,
        DeviceStatus::Offline => ReachabilityStatus::Offline,
        DeviceStatus::Degraded => ReachabilityStatus::Degraded,
        DeviceStatus::Unknown => ReachabilityStatus::Unknown,
    }
}

fn legacy_stream_transport_to_control(transport: StreamTransport) -> ControlStreamTransport {
    match transport {
        StreamTransport::Rtsp => ControlStreamTransport::Rtsp,
        StreamTransport::Hls => ControlStreamTransport::Hls,
        StreamTransport::Webrtc => ControlStreamTransport::Webrtc,
        StreamTransport::Unknown => ControlStreamTransport::Unknown,
    }
}

fn control_stream_transport_to_legacy(transport: ControlStreamTransport) -> StreamTransport {
    match transport {
        ControlStreamTransport::Rtsp => StreamTransport::Rtsp,
        ControlStreamTransport::Hls => StreamTransport::Hls,
        ControlStreamTransport::Webrtc => StreamTransport::Webrtc,
        ControlStreamTransport::File | ControlStreamTransport::Unknown => StreamTransport::Unknown,
    }
}

fn stream_endpoint_from_camera(camera: &CameraDevice, endpoint_id: String) -> DeviceEndpoint {
    let parsed = Url::parse(&camera.primary_stream.url).ok();
    let scheme = parsed
        .as_ref()
        .map(|url| url.scheme().to_string())
        .unwrap_or_else(|| {
            legacy_stream_transport_scheme(camera.primary_stream.transport).to_string()
        });
    let host = parsed
        .as_ref()
        .and_then(Url::host_str)
        .map(str::to_string)
        .unwrap_or_else(|| camera.primary_stream.url.clone());
    let port = parsed.as_ref().and_then(Url::port_or_known_default);
    let path = parsed.as_ref().map(|url| {
        let mut value = url.path().to_string();
        if let Some(query) = url.query() {
            value.push('?');
            value.push_str(query);
        }
        value
    });

    DeviceEndpoint {
        endpoint_id,
        device_id: camera.device_id.clone(),
        endpoint_kind: match camera.primary_stream.transport {
            StreamTransport::Rtsp => DeviceEndpointKind::Rtsp,
            StreamTransport::Hls => DeviceEndpointKind::Http,
            StreamTransport::Webrtc => DeviceEndpointKind::Websocket,
            StreamTransport::Unknown => DeviceEndpointKind::Http,
        },
        scheme,
        host,
        port,
        path,
        requires_auth: camera.primary_stream.requires_auth,
        reachability_status: legacy_status_to_reachability(camera.status),
        last_seen_at: camera.last_seen_at.clone(),
        metadata: json!({
            "url": camera.primary_stream.url,
        }),
    }
}

fn legacy_stream_transport_scheme(transport: StreamTransport) -> &'static str {
    match transport {
        StreamTransport::Rtsp => "rtsp",
        StreamTransport::Hls => "hls",
        StreamTransport::Webrtc => "webrtc",
        StreamTransport::Unknown => "unknown",
    }
}

fn provider_bindings_from_camera(camera: &CameraDevice) -> Vec<ProviderBinding> {
    let mut bindings = vec![ProviderBinding {
        binding_id: format!("{}::binding::stream", camera.device_id),
        device_id: camera.device_id.clone(),
        provider_account_id: None,
        provider_key: match camera.primary_stream.transport {
            StreamTransport::Rtsp => "rtsp".to_string(),
            StreamTransport::Hls => "hls".to_string(),
            StreamTransport::Webrtc => "webrtc".to_string(),
            StreamTransport::Unknown => "stream".to_string(),
        },
        provider_kind: ProviderKind::Standard,
        remote_device_id: None,
        credential_ref: None,
        binding_status: ProviderBindingStatus::Active,
        support_mode: DeviceSupportMode::Native,
        metadata: json!({
            "discovery_source": camera.discovery_source,
            "transport": legacy_stream_transport_scheme(camera.primary_stream.transport),
        }),
        last_sync_at: camera.last_seen_at.clone(),
    }];

    if let Some(device_service_url) = &camera.onvif_device_service_url {
        bindings.push(ProviderBinding {
            binding_id: format!("{}::binding::onvif", camera.device_id),
            device_id: camera.device_id.clone(),
            provider_account_id: None,
            provider_key: "onvif".to_string(),
            provider_kind: ProviderKind::Standard,
            remote_device_id: None,
            credential_ref: None,
            binding_status: ProviderBindingStatus::Active,
            support_mode: DeviceSupportMode::Native,
            metadata: json!({
                "device_service_url": device_service_url,
                "discovery_source": camera.discovery_source,
            }),
            last_sync_at: camera.last_seen_at.clone(),
        });
    }

    if let Some(device_serial) = &camera.ezviz_device_serial {
        bindings.push(ProviderBinding {
            binding_id: format!("{}::binding::ezviz", camera.device_id),
            device_id: camera.device_id.clone(),
            provider_account_id: None,
            provider_key: "ezviz".to_string(),
            provider_kind: ProviderKind::VendorCloud,
            remote_device_id: Some(device_serial.clone()),
            credential_ref: None,
            binding_status: ProviderBindingStatus::Active,
            support_mode: DeviceSupportMode::Cloud,
            metadata: json!({
                "camera_no": camera.ezviz_camera_no,
            }),
            last_sync_at: camera.last_seen_at.clone(),
        });
    }

    bindings
}

fn capability_records_from_camera(camera: &CameraDevice) -> Vec<CapabilityRecord> {
    let mut records = Vec::new();
    let mut push_capability = |code: &str, category: CapabilityCategory| {
        records.push(CapabilityRecord {
            capability_id: format!("{}::capability::{code}", camera.device_id),
            device_id: camera.device_id.clone(),
            capability_code: code.to_string(),
            category,
            access_mode: match category {
                CapabilityCategory::State => CapabilityAccessMode::Read,
                CapabilityCategory::Control => CapabilityAccessMode::Invoke,
                CapabilityCategory::Stream | CapabilityCategory::Media => {
                    CapabilityAccessMode::Read
                }
                CapabilityCategory::Event => CapabilityAccessMode::Subscribe,
            },
            support_mode: DeviceSupportMode::Native,
            availability: CapabilityAvailability::Available,
            source_binding_id: Some(format!("{}::binding::stream", camera.device_id)),
            metadata: json!({}),
        });
    };

    if camera.capabilities.snapshot {
        push_capability("snapshot", CapabilityCategory::Media);
    }
    if camera.capabilities.stream {
        push_capability("stream_live", CapabilityCategory::Stream);
    }
    if camera.capabilities.ptz {
        push_capability("ptz", CapabilityCategory::Control);
    }
    if camera.capabilities.audio {
        push_capability("audio", CapabilityCategory::Media);
    }

    records
}

fn preferred_stream_profile<'a>(
    stream_profiles: &'a [StreamProfile],
    device_id: &str,
    preferred_stream_profile_id: Option<&str>,
) -> Option<&'a StreamProfile> {
    if let Some(stream_profile_id) = preferred_stream_profile_id {
        if let Some(profile) = stream_profiles
            .iter()
            .find(|profile| profile.stream_profile_id == stream_profile_id)
        {
            return Some(profile);
        }
    }

    stream_profiles
        .iter()
        .find(|profile| profile.device_id == device_id && profile.is_default)
        .or_else(|| {
            stream_profiles
                .iter()
                .find(|profile| profile.device_id == device_id)
        })
}

fn stream_url_from_endpoint(endpoint: &DeviceEndpoint) -> String {
    endpoint
        .metadata
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| rebuild_url_from_endpoint(endpoint))
}

fn rebuild_url_from_endpoint(endpoint: &DeviceEndpoint) -> String {
    let mut url = format!("{}://{}", endpoint.scheme, endpoint.host);
    if let Some(port) = endpoint.port {
        url.push(':');
        url.push_str(&port.to_string());
    }
    if let Some(path) = &endpoint.path {
        if !path.is_empty() {
            if !path.starts_with('/') {
                url.push('/');
            }
            url.push_str(path);
        }
    }
    url
}

fn camera_capabilities_from_records(records: &[&CapabilityRecord]) -> CameraCapabilities {
    let mut capabilities = CameraCapabilities::default();
    for record in records {
        match record.capability_code.as_str() {
            "snapshot" => capabilities.snapshot = true,
            "stream_live" => capabilities.stream = true,
            "ptz" => capabilities.ptz = true,
            "audio" => capabilities.audio = true,
            _ => {}
        }
    }
    capabilities
}

fn infer_discovery_source(bindings: &[&ProviderBinding]) -> String {
    if bindings
        .iter()
        .any(|binding| binding.provider_key == "onvif")
    {
        "onvif".to_string()
    } else if let Some(binding) = bindings.first() {
        binding.provider_key.clone()
    } else {
        "unknown".to_string()
    }
}

fn same_camera_view(existing: &CameraDevice, incoming: &CameraDevice) -> bool {
    existing.device_id == incoming.device_id
        || (existing.ip_address.is_some()
            && existing.ip_address == incoming.ip_address
            && existing.primary_stream.url == incoming.primary_stream.url)
        || existing.primary_stream.url == incoming.primary_stream.url
}

fn normalize_camera_metadata(mut device: CameraDevice) -> CameraDevice {
    device.discovery_source = normalize_discovery_source(&device.discovery_source).to_string();
    if matches!(device.primary_stream.transport, StreamTransport::Unknown) {
        device.primary_stream.transport = StreamTransport::Rtsp;
    }
    device
}

fn normalize_discovery_source(value: &str) -> &str {
    match value {
        "rtspprobe" => "rtsp_probe",
        "mdns" => "mdns",
        "ssdp" => "ssdp",
        "onvif" => "onvif",
        "matter" => "matter",
        "rtsp_probe" => "rtsp_probe",
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use crate::control_plane::credentials::ProviderKind;
    use crate::control_plane::devices::{
        CapabilityAccessMode, CapabilityAvailability, CapabilityCategory, CapabilityRecord,
        ConnectivityState, DeviceEndpoint, DeviceEndpointKind, DeviceLifecycleState,
        DeviceRecord as ControlDeviceRecord, DeviceSupportMode, DeviceTwin, ProviderBinding,
        ProviderBindingStatus, ReachabilityStatus,
    };
    use crate::control_plane::media::{
        CameraProfile, StreamProfile, StreamTransport as ControlStreamTransport,
    };

    use super::{
        CameraDevice, DeviceKind, DeviceRegistrySnapshot, DeviceRegistryStore,
        ResolvedCameraTarget, StreamTransport,
    };

    #[test]
    fn camera_device_defaults_to_camera_rtsp() {
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.10/live");
        assert_eq!(device.kind, DeviceKind::Camera);
        assert_eq!(device.primary_stream.transport, StreamTransport::Rtsp);
        assert_eq!(device.primary_stream.url, "rtsp://192.168.1.10/live");
    }

    #[test]
    fn device_registry_store_round_trips_devices() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("harbornas-device-registry-{unique}.json"));
        let store = DeviceRegistryStore::new(&path);
        let mut device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.10/live");
        device.capabilities.stream = true;

        store
            .save_devices(std::slice::from_ref(&device))
            .expect("save registry");
        let loaded = store.load_devices().expect("load registry");

        assert_eq!(loaded, vec![device]);
        let text = fs::read_to_string(store.path()).expect("read file");
        assert!(text.contains("\"schema_version\": 1"));
        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn snapshot_conversion_preserves_camera_view() {
        let mut device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.10/live");
        device.status = super::DeviceStatus::Online;
        device.vendor = Some("Acme".to_string());
        device.model = Some("X1".to_string());
        device.ip_address = Some("192.168.1.10".to_string());
        device.discovery_source = "onvif".to_string();
        device.onvif_device_service_url =
            Some("http://192.168.1.10/onvif/device_service".to_string());
        device.capabilities.stream = true;
        device.capabilities.snapshot = true;

        let snapshot = DeviceRegistrySnapshot::from_camera_devices(std::slice::from_ref(&device));
        let round_tripped = snapshot.to_camera_devices();

        assert_eq!(round_tripped, vec![device]);
    }

    #[test]
    fn snapshot_resolves_camera_target_from_platform_records() {
        let mut device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.10/live");
        device.status = super::DeviceStatus::Online;
        device.room = Some("entrance".to_string());
        device.ip_address = Some("192.168.1.10".to_string());
        device.discovery_source = "onvif".to_string();
        device.capabilities.stream = true;
        device.capabilities.snapshot = true;

        let snapshot = DeviceRegistrySnapshot::from_camera_devices(std::slice::from_ref(&device));
        let target = snapshot.camera_target("cam-1").expect("camera target");

        assert_eq!(
            target,
            ResolvedCameraTarget {
                device_id: "cam-1".to_string(),
                display_name: "Front Door".to_string(),
                status: super::DeviceStatus::Online,
                room_name: Some("entrance".to_string()),
                vendor: None,
                model: None,
                ip_address: Some("192.168.1.10".to_string()),
                mac_address: None,
                discovery_source: "onvif".to_string(),
                primary_stream: device.primary_stream.clone(),
                onvif_device_service_url: None,
                ezviz_device_serial: None,
                ezviz_camera_no: None,
                capabilities: device.capabilities.clone(),
                last_seen_at: None,
            }
        );
    }

    #[test]
    fn load_snapshot_accepts_legacy_camera_array_payload() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("harbornas-device-registry-legacy-{unique}.json"));
        let store = DeviceRegistryStore::new(&path);
        let legacy_payload = json!([
            {
                "device_id": "cam-1",
                "name": "Front Door",
                "kind": "camera",
                "status": "online",
                "room": "entrance",
                "vendor": "Acme",
                "model": "X1",
                "ip_address": "192.168.1.10",
                "mac_address": "AA:BB:CC:DD:EE:FF",
                "discovery_source": "rtsp_probe",
                "primary_stream": {
                    "transport": "rtsp",
                    "url": "rtsp://192.168.1.10/live",
                    "requires_auth": false
                },
                "capabilities": {
                    "snapshot": true,
                    "stream": true,
                    "ptz": false,
                    "audio": false
                }
            }
        ]);
        fs::write(
            &path,
            serde_json::to_string_pretty(&legacy_payload).expect("legacy json"),
        )
        .expect("write legacy");

        let snapshot = store.load_snapshot().expect("load snapshot");
        let devices = store.load_devices().expect("load devices");

        assert_eq!(snapshot.schema_version, 1);
        assert_eq!(snapshot.devices.len(), 1);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Front Door");
        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn save_devices_preserves_non_legacy_snapshot_records() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("harbornas-device-registry-preserve-{unique}.json"));
        let store = DeviceRegistryStore::new(&path);

        let snapshot = DeviceRegistrySnapshot {
            schema_version: 1,
            devices: vec![
                ControlDeviceRecord {
                    device_id: "cam-1".to_string(),
                    workspace_id: "home-1".to_string(),
                    kind: crate::control_plane::devices::DeviceKind::Camera,
                    subtype: Some("ptz_camera".to_string()),
                    display_name: "Front Door".to_string(),
                    aliases: vec!["门口摄像头".to_string()],
                    vendor: Some("Acme".to_string()),
                    model: Some("X1".to_string()),
                    serial_number: Some("SN-1".to_string()),
                    mac_address: Some("AA:BB:CC:DD:EE:FF".to_string()),
                    primary_room_id: Some("entrance".to_string()),
                    lifecycle_state: DeviceLifecycleState::Active,
                    source: "onvif".to_string(),
                    metadata: json!({"custom": "keep-me"}),
                },
                ControlDeviceRecord {
                    device_id: "light-1".to_string(),
                    workspace_id: "home-1".to_string(),
                    kind: crate::control_plane::devices::DeviceKind::Light,
                    subtype: None,
                    display_name: "Porch Light".to_string(),
                    aliases: vec![],
                    vendor: Some("Harbor".to_string()),
                    model: Some("L1".to_string()),
                    serial_number: None,
                    mac_address: None,
                    primary_room_id: Some("entrance".to_string()),
                    lifecycle_state: DeviceLifecycleState::Active,
                    source: "matter".to_string(),
                    metadata: json!({}),
                },
            ],
            device_endpoints: vec![
                DeviceEndpoint {
                    endpoint_id: "cam-1::endpoint::ipv4".to_string(),
                    device_id: "cam-1".to_string(),
                    endpoint_kind: DeviceEndpointKind::Ipv4,
                    scheme: "ipv4".to_string(),
                    host: "192.168.1.10".to_string(),
                    port: None,
                    path: None,
                    requires_auth: false,
                    reachability_status: ReachabilityStatus::Online,
                    last_seen_at: Some("2026-04-15T08:00:00Z".to_string()),
                    metadata: json!({}),
                },
                DeviceEndpoint {
                    endpoint_id: "cam-1::endpoint::primary_stream".to_string(),
                    device_id: "cam-1".to_string(),
                    endpoint_kind: DeviceEndpointKind::Rtsp,
                    scheme: "rtsp".to_string(),
                    host: "192.168.1.10".to_string(),
                    port: Some(554),
                    path: Some("/live".to_string()),
                    requires_auth: true,
                    reachability_status: ReachabilityStatus::Online,
                    last_seen_at: Some("2026-04-15T08:00:00Z".to_string()),
                    metadata: json!({"url": "rtsp://192.168.1.10/live"}),
                },
            ],
            provider_bindings: vec![
                ProviderBinding {
                    binding_id: "cam-1::binding::stream".to_string(),
                    device_id: "cam-1".to_string(),
                    provider_account_id: None,
                    provider_key: "rtsp".to_string(),
                    provider_kind: ProviderKind::Standard,
                    remote_device_id: None,
                    credential_ref: None,
                    binding_status: ProviderBindingStatus::Active,
                    support_mode: DeviceSupportMode::Native,
                    metadata: json!({"transport": "rtsp"}),
                    last_sync_at: Some("2026-04-15T08:00:00Z".to_string()),
                },
                ProviderBinding {
                    binding_id: "cam-1::binding::xiaomi".to_string(),
                    device_id: "cam-1".to_string(),
                    provider_account_id: Some("provider-1".to_string()),
                    provider_key: "xiaomi".to_string(),
                    provider_kind: ProviderKind::VendorLocal,
                    remote_device_id: Some("did-1".to_string()),
                    credential_ref: Some("cred-1".to_string()),
                    binding_status: ProviderBindingStatus::Active,
                    support_mode: DeviceSupportMode::Bridge,
                    metadata: json!({"token_ref": "vault://token"}),
                    last_sync_at: Some("2026-04-15T08:00:00Z".to_string()),
                },
            ],
            capabilities: vec![
                CapabilityRecord {
                    capability_id: "cam-1::capability::stream_live".to_string(),
                    device_id: "cam-1".to_string(),
                    capability_code: "stream_live".to_string(),
                    category: CapabilityCategory::Stream,
                    access_mode: CapabilityAccessMode::Read,
                    support_mode: DeviceSupportMode::Native,
                    availability: CapabilityAvailability::Available,
                    source_binding_id: Some("cam-1::binding::stream".to_string()),
                    metadata: json!({}),
                },
                CapabilityRecord {
                    capability_id: "cam-1::capability::privacy_mode".to_string(),
                    device_id: "cam-1".to_string(),
                    capability_code: "privacy_mode".to_string(),
                    category: CapabilityCategory::Control,
                    access_mode: CapabilityAccessMode::Invoke,
                    support_mode: DeviceSupportMode::Bridge,
                    availability: CapabilityAvailability::Available,
                    source_binding_id: Some("cam-1::binding::xiaomi".to_string()),
                    metadata: json!({}),
                },
            ],
            device_twins: vec![
                DeviceTwin {
                    device_id: "cam-1".to_string(),
                    connectivity_state: ConnectivityState::Online,
                    reported_state: json!({"privacy_mode": false}),
                    desired_state: json!({}),
                    health_state: json!({"temperature": "normal"}),
                    last_event_id: Some("evt-1".to_string()),
                    last_seen_at: Some("2026-04-15T08:00:00Z".to_string()),
                },
                DeviceTwin {
                    device_id: "light-1".to_string(),
                    connectivity_state: ConnectivityState::Online,
                    reported_state: json!({"power": "off"}),
                    desired_state: json!({}),
                    health_state: json!({}),
                    last_event_id: None,
                    last_seen_at: Some("2026-04-15T08:00:00Z".to_string()),
                },
            ],
            camera_profiles: vec![CameraProfile {
                device_id: "cam-1".to_string(),
                default_stream_profile_id: Some("cam-1::stream::primary".to_string()),
                audio_supported: false,
                ptz_supported: true,
                privacy_supported: true,
                playback_supported: true,
                recording_policy_id: Some("policy-1".to_string()),
                vendor_features: json!({"night_vision": true}),
            }],
            stream_profiles: vec![StreamProfile {
                stream_profile_id: "cam-1::stream::primary".to_string(),
                device_id: "cam-1".to_string(),
                profile_name: "primary".to_string(),
                transport: ControlStreamTransport::Rtsp,
                endpoint_id: "cam-1::endpoint::primary_stream".to_string(),
                video_codec: Some("h264".to_string()),
                audio_codec: None,
                width: Some(1920),
                height: Some(1080),
                fps: Some(25.0),
                bitrate_kbps: Some(2048),
                is_default: true,
            }],
        };
        store.save_snapshot(&snapshot).expect("save snapshot");

        let mut camera =
            CameraDevice::new("cam-1", "Front Door Updated", "rtsp://192.168.1.10/live");
        camera.status = super::DeviceStatus::Online;
        camera.vendor = Some("Acme".to_string());
        camera.model = Some("X1".to_string());
        camera.ip_address = Some("192.168.1.10".to_string());
        camera.discovery_source = "rtsp_probe".to_string();
        camera.capabilities.stream = true;
        camera.capabilities.snapshot = true;
        store.save_devices(&[camera]).expect("save devices");

        let saved = store.load_snapshot().expect("load snapshot");
        assert_eq!(saved.devices.len(), 2);
        assert!(saved
            .devices
            .iter()
            .any(|device| device.device_id == "light-1"));
        let saved_camera = saved
            .devices
            .iter()
            .find(|device| device.device_id == "cam-1")
            .expect("camera device");
        assert_eq!(saved_camera.display_name, "Front Door Updated");
        assert_eq!(saved_camera.metadata["custom"], "keep-me");

        let saved_camera_profile = saved
            .camera_profiles
            .iter()
            .find(|profile| profile.device_id == "cam-1")
            .expect("camera profile");
        assert!(saved_camera_profile.privacy_supported);
        assert_eq!(
            saved_camera_profile.recording_policy_id.as_deref(),
            Some("policy-1")
        );
        assert_eq!(saved_camera_profile.vendor_features["night_vision"], true);

        assert!(saved
            .provider_bindings
            .iter()
            .any(|binding| binding.binding_id == "cam-1::binding::xiaomi"));
        assert!(saved
            .capabilities
            .iter()
            .any(|capability| capability.capability_code == "privacy_mode"));
        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn upsert_devices_merges_camera_views_without_pruning_existing_snapshot_records() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("harbornas-device-registry-upsert-{unique}.json"));
        let store = DeviceRegistryStore::new(&path);

        let mut existing_a = CameraDevice::new("cam-a", "Front Door", "rtsp://192.168.1.10/live");
        existing_a.ip_address = Some("192.168.1.10".to_string());
        existing_a.capabilities.stream = true;
        let mut existing_b = CameraDevice::new("cam-b", "Back Door", "rtsp://192.168.1.11/live");
        existing_b.ip_address = Some("192.168.1.11".to_string());
        existing_b.capabilities.stream = true;

        let mut initial_snapshot =
            DeviceRegistrySnapshot::from_camera_devices(&[existing_a.clone(), existing_b]);
        initial_snapshot.devices.push(ControlDeviceRecord {
            device_id: "light-1".to_string(),
            workspace_id: "home".to_string(),
            kind: crate::control_plane::devices::DeviceKind::Light,
            subtype: None,
            display_name: "Hall Light".to_string(),
            aliases: vec![],
            vendor: None,
            model: None,
            serial_number: None,
            mac_address: None,
            primary_room_id: Some("hall".to_string()),
            lifecycle_state: DeviceLifecycleState::Registered,
            source: "matter".to_string(),
            metadata: json!({}),
        });
        store
            .save_snapshot(&initial_snapshot)
            .expect("save initial snapshot");

        let mut incoming =
            CameraDevice::new("cam-new", "Front Door Updated", "rtsp://192.168.1.10/live");
        incoming.ip_address = Some("192.168.1.10".to_string());
        incoming.capabilities.stream = true;

        let saved = store.upsert_devices(&[incoming]).expect("upsert devices");
        let devices = saved.to_camera_devices();
        assert_eq!(devices.len(), 2);
        assert!(devices.iter().any(|device| device.device_id == "cam-a"));
        assert!(devices.iter().any(|device| device.device_id == "cam-b"));
        assert!(saved
            .devices
            .iter()
            .any(|device| device.device_id == "light-1"));

        let saved_camera = devices
            .iter()
            .find(|device| device.device_id == "cam-a")
            .expect("existing camera");
        assert_eq!(saved_camera.name, "Front Door Updated");
        let _ = fs::remove_file(store.path());
    }
}
