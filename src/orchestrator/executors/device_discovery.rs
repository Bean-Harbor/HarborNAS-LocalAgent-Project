use std::time::Instant;

use serde_json::json;

use crate::adapters::mdns::MdnsDiscoveryAdapter;
use crate::adapters::onvif::OnvifDiscoveryAdapter;
use crate::adapters::rtsp::RtspProbeAdapter;
use crate::adapters::ssdp::SsdpDiscoveryAdapter;
use crate::domains::device::{
    DeviceDiscoverArgs, DeviceDiscoverPayload, DeviceGetArgs, DeviceGetPayload, DeviceListArgs,
    DeviceListPayload,
};
use crate::orchestrator::contracts::{Action, ExecutionResult, Route, StepStatus};
use crate::orchestrator::router::Executor;
use crate::runtime::discovery::DiscoveryService;
use crate::runtime::registry::CameraDevice;

pub struct DeviceDiscoveryExecutor {
    service: DiscoveryService,
    devices: Vec<CameraDevice>,
}

impl DeviceDiscoveryExecutor {
    pub fn new(
        rtsp: Box<dyn RtspProbeAdapter>,
        onvif: Option<Box<dyn OnvifDiscoveryAdapter>>,
        ssdp: Option<Box<dyn SsdpDiscoveryAdapter>>,
        mdns: Option<Box<dyn MdnsDiscoveryAdapter>>,
    ) -> Self {
        Self {
            service: DiscoveryService::new(rtsp, onvif, ssdp, mdns),
            devices: Vec::new(),
        }
    }

    pub fn with_devices(mut self, devices: Vec<CameraDevice>) -> Self {
        self.devices = devices;
        self
    }
}

impl Executor for DeviceDiscoveryExecutor {
    fn route(&self) -> Route {
        Route::Mcp
    }

    fn is_available(&self) -> bool {
        true
    }

    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String> {
        let started = Instant::now();

        let payload = match action.operation.as_str() {
            "discover" => {
                let merged = merge_resource_and_args(action);
                let args: DeviceDiscoverArgs =
                    serde_json::from_value(merged).map_err(|e| format!("invalid discover args: {e}"))?;
                let result = self.service.discover(&args.into_request())?;
                serde_json::to_value(DeviceDiscoverPayload { discovery: result })
                    .map_err(|e| format!("discover payload serialize failed: {e}"))?
            }
            "list" => {
                let _args: DeviceListArgs = serde_json::from_value(merge_resource_and_args(action))
                    .unwrap_or_default();
                serde_json::to_value(DeviceListPayload {
                    devices: self.devices.clone(),
                })
                .map_err(|e| format!("list payload serialize failed: {e}"))?
            }
            "get" => {
                let args: DeviceGetArgs = serde_json::from_value(merge_resource_and_args(action))
                    .map_err(|e| format!("invalid get args: {e}"))?;
                let device = self
                    .devices
                    .iter()
                    .find(|d| d.device_id == args.device_id)
                    .cloned()
                    .ok_or_else(|| format!("device not found: {}", args.device_id))?;
                serde_json::to_value(DeviceGetPayload { device })
                    .map_err(|e| format!("get payload serialize failed: {e}"))?
            }
            other => return Err(format!("unsupported device operation: {other}")),
        };

        Ok(ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: Route::Mcp.as_str().to_string(),
            fallback_used: false,
            status: StepStatus::Success,
            duration_ms: started.elapsed().as_millis() as u64,
            error_code: None,
            error_message: None,
            audit_ref: String::new(),
            result_payload: payload,
        })
    }
}

fn merge_resource_and_args(action: &Action) -> serde_json::Value {
    let mut merged = serde_json::Map::new();
    if let Some(resource) = action.resource.as_object() {
        merged.extend(resource.clone());
    }
    if let Some(args) = action.args.as_object() {
        merged.extend(args.clone());
    }
    json!(merged)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::adapters::mdns::MdnsDiscoveryAdapter;
    use crate::adapters::onvif::OnvifDiscoveryAdapter;
    use crate::adapters::rtsp::RtspProbeAdapter;
    use crate::adapters::ssdp::SsdpDiscoveryAdapter;
    use crate::orchestrator::contracts::Action;
    use crate::orchestrator::router::Executor;
    use crate::runtime::discovery::{
        DiscoveryCandidate, DiscoveryCandidateStatus, DiscoveryProtocol, DiscoveryRequest,
        RtspProbeRequest, RtspProbeResult,
    };
    use crate::runtime::registry::{CameraDevice, StreamTransport};

    use super::DeviceDiscoveryExecutor;

    struct StaticOnvifAdapter;
    struct EmptySsdpAdapter;
    struct EmptyMdnsAdapter;
    struct StaticRtspAdapter;

    impl OnvifDiscoveryAdapter for StaticOnvifAdapter {
        fn discover(&self, _request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String> {
            Ok(vec![DiscoveryCandidate {
                candidate_id: "cand-1".to_string(),
                protocol: DiscoveryProtocol::Onvif,
                name: Some("Front Door".to_string()),
                ip_address: "192.168.1.50".to_string(),
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
                stream_url: Some(format!("rtsp://{}/live", request.ip_address)),
                transport: StreamTransport::Rtsp,
                requires_auth: false,
                capabilities: Default::default(),
                error_message: None,
            })
        }
    }

    #[test]
    fn discover_returns_execution_result_payload() {
        let executor = DeviceDiscoveryExecutor::new(
            Box::new(StaticRtspAdapter),
            Some(Box::new(StaticOnvifAdapter)),
            Some(Box::new(EmptySsdpAdapter)),
            Some(Box::new(EmptyMdnsAdapter)),
        );
        let action = Action {
            domain: "device".to_string(),
            operation: "discover".to_string(),
            resource: json!({"scan_id":"scan-1","network_cidr":"192.168.1.0/24"}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };

        let result = executor.execute(&action, "t1", "s1").expect("discover result");
        assert_eq!(result.executor_used, "mcp");
        assert_eq!(result.status, crate::orchestrator::contracts::StepStatus::Success);
        assert_eq!(
            result.result_payload["discovery"]["connected_devices"][0]["primary_stream"]["url"],
            "rtsp://192.168.1.50/live"
        );
    }

    #[test]
    fn list_and_get_return_registered_devices() {
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.50/live");
        let executor = DeviceDiscoveryExecutor::new(
            Box::new(StaticRtspAdapter),
            None,
            None,
            None,
        )
        .with_devices(vec![device]);

        let list_action = Action {
            domain: "device".to_string(),
            operation: "list".to_string(),
            resource: json!({}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let list_result = executor.execute(&list_action, "t1", "s1").expect("list result");
        assert_eq!(list_result.result_payload["devices"][0]["device_id"], "cam-1");

        let get_action = Action {
            domain: "device".to_string(),
            operation: "get".to_string(),
            resource: json!({"device_id":"cam-1"}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let get_result = executor.execute(&get_action, "t1", "s2").expect("get result");
        assert_eq!(get_result.result_payload["device"]["name"], "Front Door");
    }
}
