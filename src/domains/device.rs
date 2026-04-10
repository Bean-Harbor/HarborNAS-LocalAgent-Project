//! Device domain actions such as discover, snapshot, and ptz.

use serde::{Deserialize, Serialize};

use crate::runtime::discovery::{
    DiscoveryBatchResult, DiscoveryProtocol, DiscoveryRequest, RtspProbeResult,
};
use crate::runtime::registry::CameraDevice;

pub const DOMAIN: &str = "device";
pub const OP_DISCOVER: &str = "discover";
pub const OP_LIST: &str = "list";
pub const OP_GET: &str = "get";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceDiscoverArgs {
    pub scan_id: String,
    pub network_cidr: String,
    #[serde(default = "default_discovery_protocols")]
    pub protocols: Vec<DiscoveryProtocol>,
    #[serde(default = "default_true")]
    pub include_rtsp_probe: bool,
}

impl DeviceDiscoverArgs {
    pub fn into_request(self) -> DiscoveryRequest {
        DiscoveryRequest {
            scan_id: self.scan_id,
            network_cidr: self.network_cidr,
            protocols: self.protocols,
            include_rtsp_probe: self.include_rtsp_probe,
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
pub struct DeviceProbePayload {
    pub result: RtspProbeResult,
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

#[cfg(test)]
mod tests {
    use super::{DeviceDiscoverArgs, OP_DISCOVER};
    use crate::domains::device::DOMAIN;
    use crate::runtime::discovery::DiscoveryProtocol;

    #[test]
    fn discover_args_default_to_auto_discovery_stack() {
        let args = DeviceDiscoverArgs {
            scan_id: "scan-1".to_string(),
            network_cidr: "192.168.1.0/24".to_string(),
            protocols: vec![],
            include_rtsp_probe: true,
        };
        let request = args.into_request();
        assert_eq!(DOMAIN, "device");
        assert_eq!(OP_DISCOVER, "discover");
        assert!(request.include_rtsp_probe);
        assert!(request.protocols.is_empty());
    }

    #[test]
    fn default_protocols_include_rtsp_probe() {
        let args: DeviceDiscoverArgs = serde_json::from_str(
            r#"{"scan_id":"scan-1","network_cidr":"192.168.1.0/24"}"#,
        )
        .expect("parse args");
        assert!(args.protocols.contains(&DiscoveryProtocol::RtspProbe));
    }
}
