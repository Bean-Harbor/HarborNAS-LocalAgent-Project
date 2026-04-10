//! Device discovery over LAN protocols such as ONVIF, mDNS, and SSDP.

use serde::{Deserialize, Serialize};

use crate::adapters::mdns::MdnsDiscoveryAdapter;
use crate::adapters::onvif::OnvifDiscoveryAdapter;
use crate::adapters::rtsp::RtspProbeAdapter;
use crate::adapters::ssdp::SsdpDiscoveryAdapter;
use crate::runtime::registry::{CameraCapabilities, CameraDevice, DeviceStatus, StreamTransport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProtocol {
    Onvif,
    Mdns,
    Ssdp,
    Matter,
    RtspProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryCandidateStatus {
    Discovered,
    Validated,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryRequest {
    pub scan_id: String,
    pub network_cidr: String,
    #[serde(default)]
    pub protocols: Vec<DiscoveryProtocol>,
    #[serde(default)]
    pub include_rtsp_probe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryCandidate {
    pub candidate_id: String,
    pub protocol: DiscoveryProtocol,
    pub name: Option<String>,
    pub ip_address: String,
    pub port: Option<u16>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub rtsp_paths: Vec<String>,
    pub status: DiscoveryCandidateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RtspProbeRequest {
    pub candidate_id: String,
    pub ip_address: String,
    pub port: u16,
    #[serde(default)]
    pub path_candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RtspProbeResult {
    pub candidate_id: String,
    pub reachable: bool,
    pub stream_url: Option<String>,
    pub transport: StreamTransport,
    #[serde(default)]
    pub requires_auth: bool,
    #[serde(default)]
    pub capabilities: CameraCapabilities,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryBatchResult {
    pub scan_id: String,
    #[serde(default)]
    pub candidates: Vec<DiscoveryCandidate>,
    #[serde(default)]
    pub connected_devices: Vec<CameraDevice>,
}

pub struct DiscoveryService {
    onvif: Option<Box<dyn OnvifDiscoveryAdapter>>,
    ssdp: Option<Box<dyn SsdpDiscoveryAdapter>>,
    mdns: Option<Box<dyn MdnsDiscoveryAdapter>>,
    rtsp: Box<dyn RtspProbeAdapter>,
}

impl DiscoveryService {
    pub fn new(
        rtsp: Box<dyn RtspProbeAdapter>,
        onvif: Option<Box<dyn OnvifDiscoveryAdapter>>,
        ssdp: Option<Box<dyn SsdpDiscoveryAdapter>>,
        mdns: Option<Box<dyn MdnsDiscoveryAdapter>>,
    ) -> Self {
        Self {
            onvif,
            ssdp,
            mdns,
            rtsp,
        }
    }

    pub fn discover(&self, request: &DiscoveryRequest) -> Result<DiscoveryBatchResult, String> {
        let mut candidates = Vec::new();

        for protocol in &request.protocols {
            match protocol {
                DiscoveryProtocol::Onvif => {
                    if let Some(adapter) = &self.onvif {
                        candidates.extend(adapter.discover(request)?);
                    }
                }
                DiscoveryProtocol::Ssdp => {
                    if let Some(adapter) = &self.ssdp {
                        candidates.extend(adapter.discover(request)?);
                    }
                }
                DiscoveryProtocol::Mdns => {
                    if let Some(adapter) = &self.mdns {
                        candidates.extend(adapter.discover(request)?);
                    }
                }
                DiscoveryProtocol::Matter | DiscoveryProtocol::RtspProbe => {}
            }
        }

        let mut connected_devices = Vec::new();
        if request.include_rtsp_probe {
            for candidate in &candidates {
                let probe_request = RtspProbeRequest {
                    candidate_id: candidate.candidate_id.clone(),
                    ip_address: candidate.ip_address.clone(),
                    port: candidate.port.unwrap_or(554),
                    path_candidates: candidate.rtsp_paths.clone(),
                };
                let result = self.rtsp.probe(&probe_request)?;
                if let Some(device) = result.into_camera_device(candidate, format!("cam-{}", candidate.candidate_id)) {
                    connected_devices.push(device);
                }
            }
        }

        Ok(DiscoveryBatchResult {
            scan_id: request.scan_id.clone(),
            candidates,
            connected_devices,
        })
    }
}

impl RtspProbeResult {
    pub fn into_camera_device(
        self,
        candidate: &DiscoveryCandidate,
        device_id: impl Into<String>,
    ) -> Option<CameraDevice> {
        let stream_url = self.stream_url?;
        let mut device = CameraDevice::new(
            device_id.into(),
            candidate
                .name
                .clone()
                .unwrap_or_else(|| format!("camera-{}", candidate.ip_address)),
            stream_url,
        );
        device.status = if self.reachable {
            DeviceStatus::Online
        } else {
            DeviceStatus::Unknown
        };
        device.vendor = candidate.vendor.clone();
        device.model = candidate.model.clone();
        device.ip_address = Some(candidate.ip_address.clone());
        device.discovery_source = format!("{:?}", candidate.protocol).to_lowercase();
        device.primary_stream.transport = self.transport;
        device.primary_stream.requires_auth = self.requires_auth;
        device.capabilities = self.capabilities;
        Some(device)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveryBatchResult, DiscoveryCandidate, DiscoveryCandidateStatus, DiscoveryProtocol,
        DiscoveryRequest, DiscoveryService, RtspProbeRequest, RtspProbeResult,
    };
    use crate::adapters::mdns::MdnsDiscoveryAdapter;
    use crate::adapters::onvif::OnvifDiscoveryAdapter;
    use crate::adapters::rtsp::RtspProbeAdapter;
    use crate::adapters::ssdp::SsdpDiscoveryAdapter;
    use crate::runtime::registry::StreamTransport;

    struct StaticOnvifAdapter;
    struct EmptySsdpAdapter;
    struct EmptyMdnsAdapter;
    struct StaticRtspAdapter;

    impl OnvifDiscoveryAdapter for StaticOnvifAdapter {
        fn discover(&self, _request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String> {
            Ok(vec![DiscoveryCandidate {
                candidate_id: "cand-1".to_string(),
                protocol: DiscoveryProtocol::Onvif,
                name: Some("Gate Cam".to_string()),
                ip_address: "192.168.1.20".to_string(),
                port: Some(554),
                vendor: Some("Demo".to_string()),
                model: Some("X1".to_string()),
                rtsp_paths: vec!["/live".to_string()],
                status: DiscoveryCandidateStatus::Discovered,
            }])
        }
    }

    impl SsdpDiscoveryAdapter for EmptySsdpAdapter {
        fn discover(&self, _request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String> {
            Ok(vec![])
        }
    }

    impl MdnsDiscoveryAdapter for EmptyMdnsAdapter {
        fn discover(&self, _request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String> {
            Ok(vec![])
        }
    }

    impl RtspProbeAdapter for StaticRtspAdapter {
        fn probe(&self, request: &RtspProbeRequest) -> Result<RtspProbeResult, String> {
            Ok(RtspProbeResult {
                candidate_id: request.candidate_id.clone(),
                reachable: true,
                stream_url: Some(format!("rtsp://{}{}", request.ip_address, "/live")),
                transport: StreamTransport::Rtsp,
                requires_auth: false,
                capabilities: Default::default(),
                error_message: None,
            })
        }
    }

    #[test]
    fn probe_result_can_become_camera_device() {
        let candidate = DiscoveryCandidate {
            candidate_id: "cand-1".to_string(),
            protocol: DiscoveryProtocol::Onvif,
            name: Some("Gate Cam".to_string()),
            ip_address: "192.168.1.20".to_string(),
            port: Some(554),
            vendor: Some("Demo".to_string()),
            model: Some("X1".to_string()),
            rtsp_paths: vec!["/live".to_string()],
            status: DiscoveryCandidateStatus::Validated,
        };
        let probe = RtspProbeResult {
            candidate_id: "cand-1".to_string(),
            reachable: true,
            stream_url: Some("rtsp://192.168.1.20/live".to_string()),
            transport: StreamTransport::Rtsp,
            requires_auth: false,
            capabilities: Default::default(),
            error_message: None,
        };

        let device = probe.into_camera_device(&candidate, "cam-1").expect("device");
        assert_eq!(device.device_id, "cam-1");
        assert_eq!(device.ip_address.as_deref(), Some("192.168.1.20"));
        assert_eq!(device.discovery_source, "onvif");
    }

    #[test]
    fn discovery_service_promotes_candidates_to_devices() {
        let service = DiscoveryService::new(
            Box::new(StaticRtspAdapter),
            Some(Box::new(StaticOnvifAdapter)),
            Some(Box::new(EmptySsdpAdapter)),
            Some(Box::new(EmptyMdnsAdapter)),
        );
        let request = DiscoveryRequest {
            scan_id: "scan-1".to_string(),
            network_cidr: "192.168.1.0/24".to_string(),
            protocols: vec![
                DiscoveryProtocol::Onvif,
                DiscoveryProtocol::Ssdp,
                DiscoveryProtocol::Mdns,
            ],
            include_rtsp_probe: true,
        };

        let result: DiscoveryBatchResult = service.discover(&request).expect("discovery result");
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.connected_devices.len(), 1);
        assert_eq!(result.connected_devices[0].primary_stream.url, "rtsp://192.168.1.20/live");
    }
}
