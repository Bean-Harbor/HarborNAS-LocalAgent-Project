//! Device registry and runtime metadata cache.

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
    pub fn new(device_id: impl Into<String>, name: impl Into<String>, url: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::{CameraDevice, DeviceKind, StreamTransport};

    #[test]
    fn camera_device_defaults_to_camera_rtsp() {
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.10/live");
        assert_eq!(device.kind, DeviceKind::Camera);
        assert_eq!(device.primary_stream.transport, StreamTransport::Rtsp);
        assert_eq!(device.primary_stream.url, "rtsp://192.168.1.10/live");
    }
}
