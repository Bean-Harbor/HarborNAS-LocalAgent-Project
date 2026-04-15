//! Device domain actions such as discover, snapshot, and ptz.

use serde::{Deserialize, Serialize};

use crate::connectors::ezviz::EzvizPtzDirection;
use crate::connectors::storage::StorageTarget;
use crate::runtime::discovery::{
    DiscoveryBatchResult, DiscoveryProtocol, DiscoveryRequest, RtspProbeResult,
};
use crate::runtime::media::{
    SnapshotCaptureRequest, SnapshotCaptureResult, SnapshotFormat, StreamOpenRequest,
    StreamOpenResult,
};
use crate::runtime::registry::CameraDevice;

pub const DOMAIN: &str = "device";
pub const OP_DISCOVER: &str = "discover";
pub const OP_LIST: &str = "list";
pub const OP_GET: &str = "get";
pub const OP_UPDATE: &str = "update";
pub const OP_SNAPSHOT: &str = "snapshot";
pub const OP_OPEN_STREAM: &str = "open_stream";
pub const OP_PTZ: &str = "ptz";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceDiscoverArgs {
    pub scan_id: String,
    pub network_cidr: String,
    #[serde(default = "default_discovery_protocols")]
    pub protocols: Vec<DiscoveryProtocol>,
    #[serde(default = "default_true")]
    pub include_rtsp_probe: bool,
    #[serde(default)]
    pub rtsp_port: Option<u16>,
    #[serde(default)]
    pub rtsp_username: Option<String>,
    #[serde(default)]
    pub rtsp_password: Option<String>,
    #[serde(default)]
    pub rtsp_paths: Vec<String>,
}

impl DeviceDiscoverArgs {
    pub fn into_request(self) -> DiscoveryRequest {
        DiscoveryRequest {
            scan_id: self.scan_id,
            network_cidr: self.network_cidr,
            protocols: self.protocols,
            include_rtsp_probe: self.include_rtsp_probe,
            rtsp_port: self.rtsp_port,
            rtsp_username: self.rtsp_username,
            rtsp_password: self.rtsp_password,
            rtsp_paths: self.rtsp_paths,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DeviceListArgs {
    pub room: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceGetArgs {
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceUpdateArgs {
    pub device_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub room: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceSnapshotArgs {
    pub device_id: String,
    #[serde(default)]
    pub format: SnapshotFormat,
    #[serde(default)]
    pub storage_target: StorageTarget,
}

impl DeviceSnapshotArgs {
    pub fn into_request(self, device: &CameraDevice) -> SnapshotCaptureRequest {
        SnapshotCaptureRequest::new(
            self.device_id,
            device.primary_stream.url.clone(),
            self.format,
            self.storage_target,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceOpenStreamArgs {
    pub device_id: String,
    #[serde(default)]
    pub preferred_player: Option<String>,
}

impl DeviceOpenStreamArgs {
    pub fn into_request(self, device: &CameraDevice) -> StreamOpenRequest {
        StreamOpenRequest::new(
            self.device_id,
            device.primary_stream.url.clone(),
            self.preferred_player,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePtzDirection {
    Left,
    Right,
    Up,
    Down,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePtzProvider {
    Onvif,
    EzvizCloud,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DevicePtzArgs {
    pub device_id: String,
    pub direction: DevicePtzDirection,
    #[serde(default = "default_ptz_provider")]
    pub provider: DevicePtzProvider,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub ezviz_device_serial: Option<String>,
    #[serde(default)]
    pub ezviz_camera_no: Option<u32>,
    #[serde(default = "default_pan_speed")]
    pub pan_speed: f32,
    #[serde(default = "default_tilt_speed")]
    pub tilt_speed: f32,
    #[serde(default = "default_ezviz_speed")]
    pub ezviz_speed: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceDiscoverPayload {
    pub discovery: DiscoveryBatchResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceListPayload {
    pub devices: Vec<CameraDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceGetPayload {
    pub device: CameraDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceUpdatePayload {
    pub device: CameraDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceProbePayload {
    pub result: RtspProbeResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceSnapshotPayload {
    pub snapshot: SnapshotCaptureResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceOpenStreamPayload {
    pub stream: StreamOpenResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevicePtzPayload {
    pub device_id: String,
    pub profile_token: String,
    pub ptz_service_url: String,
    pub action: String,
}

fn default_discovery_protocols() -> Vec<DiscoveryProtocol> {
    vec![
        DiscoveryProtocol::Onvif,
        DiscoveryProtocol::Ssdp,
        DiscoveryProtocol::Mdns,
        DiscoveryProtocol::RtspProbe,
    ]
}

fn default_true() -> bool {
    true
}

fn default_pan_speed() -> f32 {
    0.4
}

fn default_tilt_speed() -> f32 {
    0.4
}

fn default_ezviz_speed() -> u8 {
    2
}

fn default_ptz_provider() -> DevicePtzProvider {
    DevicePtzProvider::Onvif
}

impl From<DevicePtzDirection> for EzvizPtzDirection {
    fn from(value: DevicePtzDirection) -> Self {
        match value {
            DevicePtzDirection::Left => EzvizPtzDirection::Left,
            DevicePtzDirection::Right => EzvizPtzDirection::Right,
            DevicePtzDirection::Up => EzvizPtzDirection::Up,
            DevicePtzDirection::Down => EzvizPtzDirection::Down,
            DevicePtzDirection::Stop => EzvizPtzDirection::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DeviceDiscoverArgs, DeviceOpenStreamArgs, DevicePtzArgs, DevicePtzDirection,
        DeviceSnapshotArgs, DeviceUpdateArgs, OP_DISCOVER, OP_OPEN_STREAM, OP_PTZ, OP_SNAPSHOT,
        OP_UPDATE,
    };
    use crate::connectors::storage::StorageTarget;
    use crate::domains::device::DOMAIN;
    use crate::runtime::discovery::DiscoveryProtocol;
    use crate::runtime::media::SnapshotFormat;
    use crate::runtime::registry::CameraDevice;

    #[test]
    fn discover_args_default_to_auto_discovery_stack() {
        let args = DeviceDiscoverArgs {
            scan_id: "scan-1".to_string(),
            network_cidr: "192.168.1.0/24".to_string(),
            protocols: vec![],
            include_rtsp_probe: true,
            rtsp_port: None,
            rtsp_username: None,
            rtsp_password: None,
            rtsp_paths: vec![],
        };
        let request = args.into_request();
        assert_eq!(DOMAIN, "device");
        assert_eq!(OP_DISCOVER, "discover");
        assert!(request.include_rtsp_probe);
        assert!(request.protocols.is_empty());
    }

    #[test]
    fn default_protocols_include_rtsp_probe() {
        let args: DeviceDiscoverArgs =
            serde_json::from_str(r#"{"scan_id":"scan-1","network_cidr":"192.168.1.0/24"}"#)
                .expect("parse args");
        assert!(args.protocols.contains(&DiscoveryProtocol::RtspProbe));
    }

    #[test]
    fn snapshot_args_default_to_local_jpeg_capture() {
        let args: DeviceSnapshotArgs =
            serde_json::from_str(r#"{"device_id":"cam-1"}"#).expect("parse snapshot args");
        assert_eq!(OP_SNAPSHOT, "snapshot");
        assert_eq!(args.format, SnapshotFormat::Jpeg);
        assert_eq!(args.storage_target, StorageTarget::LocalDisk);
    }

    #[test]
    fn open_stream_args_can_override_player() {
        let args: DeviceOpenStreamArgs =
            serde_json::from_str(r#"{"device_id":"cam-1","preferred_player":"mpv"}"#)
                .expect("parse stream args");
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.50/live");
        let request = args.into_request(&device);

        assert_eq!(OP_OPEN_STREAM, "open_stream");
        assert_eq!(request.preferred_player.as_deref(), Some("mpv"));
        assert_eq!(request.stream_url, "rtsp://192.168.1.50/live");
    }

    #[test]
    fn discover_args_preserve_rtsp_probe_configuration() {
        let args: DeviceDiscoverArgs = serde_json::from_str(
            r#"{
                "scan_id":"scan-1",
                "network_cidr":"192.168.3.0/24",
                "rtsp_port":554,
                "rtsp_username":"admin",
                "rtsp_password":"MZBEHH",
                "rtsp_paths":["/ch1/main","/h264/ch1/main/av_stream"]
            }"#,
        )
        .expect("parse args");

        let request = args.into_request();
        assert_eq!(request.rtsp_port, Some(554));
        assert_eq!(request.rtsp_username.as_deref(), Some("admin"));
        assert_eq!(request.rtsp_password.as_deref(), Some("MZBEHH"));
        assert_eq!(request.rtsp_paths.len(), 2);
    }

    #[test]
    fn update_args_allow_setting_name_and_room() {
        let args: DeviceUpdateArgs =
            serde_json::from_str(r#"{"device_id":"cam-1","name":"Living Room","room":"Home"}"#)
                .expect("parse args");

        assert_eq!(OP_UPDATE, "update");
        assert_eq!(args.name.as_deref(), Some("Living Room"));
        assert_eq!(args.room.as_deref(), Some("Home"));
    }

    #[test]
    fn ptz_args_default_to_medium_speed() {
        let args: DevicePtzArgs =
            serde_json::from_str(r#"{"device_id":"cam-1","direction":"left"}"#)
                .expect("parse ptz args");
        assert_eq!(OP_PTZ, "ptz");
        assert_eq!(args.direction, DevicePtzDirection::Left);
        assert_eq!(args.pan_speed, 0.4);
        assert_eq!(args.tilt_speed, 0.4);
        assert_eq!(args.provider, super::DevicePtzProvider::Onvif);
        assert_eq!(args.ezviz_speed, 2);
    }
}
