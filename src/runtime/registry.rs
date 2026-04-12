//! Device registry and runtime metadata cache.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
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
            capabilities: CameraCapabilities::default(),
            last_seen_at: None,
        }
    }
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

    pub fn load_devices(&self) -> Result<Vec<CameraDevice>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let text = fs::read_to_string(&self.path).map_err(|e| {
            format!(
                "failed to read device registry {}: {e}",
                self.path.display()
            )
        })?;
        serde_json::from_str(&text).map_err(|e| {
            format!(
                "failed to parse device registry {}: {e}",
                self.path.display()
            )
        })
    }

    pub fn save_devices(&self, devices: &[CameraDevice]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "failed to create device registry directory {}: {e}",
                    parent.display()
                )
            })?;
        }

        let payload = serde_json::to_string_pretty(devices).map_err(|e| {
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
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{CameraDevice, DeviceKind, DeviceRegistryStore, StreamTransport};

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
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.10/live");

        store
            .save_devices(std::slice::from_ref(&device))
            .expect("save registry");
        let loaded = store.load_devices().expect("load registry");

        assert_eq!(loaded, vec![device]);
        let _ = fs::remove_file(store.path());
    }
}
