use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::json;

use crate::adapters::mdns::MdnsDiscoveryAdapter;
use crate::adapters::onvif::{
    default_onvif_device_service_url, OnvifDiscoveryAdapter, OnvifPtzAdapter, OnvifPtzRequest,
    PtzDirection, SoapOnvifPtzAdapter,
};
use crate::adapters::rtsp::RtspProbeAdapter;
use crate::adapters::ssdp::SsdpDiscoveryAdapter;
use crate::connectors::ezviz::{EzvizCloudConfig, EzvizCloudPtzConnector, EzvizPtzRequest};
use crate::domains::device::{
    DeviceDiscoverArgs, DeviceDiscoverPayload, DeviceGetArgs, DeviceGetPayload, DeviceListArgs,
    DeviceListPayload, DeviceOpenStreamArgs, DeviceOpenStreamPayload, DevicePtzArgs,
    DevicePtzDirection, DevicePtzPayload, DevicePtzProvider, DeviceSnapshotArgs,
    DeviceSnapshotPayload, DeviceUpdateArgs, DeviceUpdatePayload,
};
use crate::orchestrator::contracts::{Action, ExecutionResult, Route, StepStatus};
use crate::orchestrator::router::Executor;
use crate::runtime::discovery::DiscoveryService;
use crate::runtime::hub::normalize_camera_metadata;
use crate::runtime::registry::{CameraDevice, DeviceRegistrySnapshot, DeviceRegistryStore};

pub struct DeviceDiscoveryExecutor {
    service: DiscoveryService,
    ptz: Box<dyn OnvifPtzAdapter>,
    devices: Arc<Mutex<DeviceRegistrySnapshot>>,
    registry_store: Option<DeviceRegistryStore>,
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
            ptz: Box::new(SoapOnvifPtzAdapter::default()),
            devices: Arc::new(Mutex::new(DeviceRegistrySnapshot::default())),
            registry_store: None,
        }
    }

    pub fn with_devices(mut self, devices: Vec<CameraDevice>) -> Self {
        let devices: Vec<_> = devices.into_iter().map(normalize_camera_metadata).collect();
        self.devices = Arc::new(Mutex::new(DeviceRegistrySnapshot::from_camera_devices(
            &devices,
        )));
        self
    }

    pub fn with_registry_store(mut self, store: DeviceRegistryStore) -> Result<Self, String> {
        self.devices = Arc::new(Mutex::new(store.load_snapshot()?));
        self.registry_store = Some(store);
        Ok(self)
    }

    fn devices_snapshot(&self) -> Result<Vec<CameraDevice>, String> {
        self.devices
            .lock()
            .map(|snapshot| {
                snapshot
                    .to_camera_devices()
                    .into_iter()
                    .map(normalize_camera_metadata)
                    .collect()
            })
            .map_err(|_| "device registry lock poisoned".to_string())
    }

    fn find_device(&self, device_id: &str) -> Result<CameraDevice, String> {
        let snapshot = self
            .devices
            .lock()
            .map_err(|_| "device registry lock poisoned".to_string())?;
        snapshot
            .to_camera_devices()
            .into_iter()
            .find(|d| d.device_id == device_id)
            .map(normalize_camera_metadata)
            .ok_or_else(|| format!("device not found: {device_id}"))
    }

    fn upsert_discovered_devices(
        &self,
        discovered: &[CameraDevice],
    ) -> Result<Vec<CameraDevice>, String> {
        let mut snapshot = self
            .devices
            .lock()
            .map_err(|_| "device registry lock poisoned".to_string())?;
        snapshot.upsert_camera_devices_preserving_platform_records(discovered);

        if let Some(store) = &self.registry_store {
            store.save_snapshot(&snapshot)?;
        }

        Ok(snapshot
            .to_camera_devices()
            .into_iter()
            .map(normalize_camera_metadata)
            .collect())
    }

    fn update_device(&self, args: &DeviceUpdateArgs) -> Result<CameraDevice, String> {
        let mut snapshot = self
            .devices
            .lock()
            .map_err(|_| "device registry lock poisoned".to_string())?;
        let mut devices = snapshot.to_camera_devices();
        let device = devices
            .iter_mut()
            .find(|d| d.device_id == args.device_id)
            .ok_or_else(|| format!("device not found: {}", args.device_id))?;

        if let Some(name) = &args.name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                device.name = trimmed.to_string();
            }
        }
        if let Some(room) = &args.room {
            let trimmed = room.trim();
            device.room = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }

        let updated = normalize_camera_metadata(device.clone());
        snapshot.upsert_camera_devices_preserving_platform_records(&[updated.clone()]);
        if let Some(store) = &self.registry_store {
            store.save_snapshot(&snapshot)?;
        }
        Ok(updated)
    }

    fn ptz_device(&self, args: &DevicePtzArgs) -> Result<DevicePtzPayload, String> {
        let device = self.find_device(&args.device_id)?;
        if args.provider == DevicePtzProvider::EzvizCloud {
            let config = EzvizCloudConfig::from_env().ok_or_else(|| {
                "EZVIZ cloud PTZ requires HARBOR_EZVIZ_APP_KEY and HARBOR_EZVIZ_APP_SECRET"
                    .to_string()
            })?;
            let connector = EzvizCloudPtzConnector::new(config)?;
            let device_serial = args
                .ezviz_device_serial
                .clone()
                .or(device.ezviz_device_serial.clone())
                .ok_or_else(|| {
                    "EZVIZ cloud PTZ requires ezviz_device_serial in args or registry".to_string()
                })?;
            let camera_no = args
                .ezviz_camera_no
                .or(device.ezviz_camera_no)
                .ok_or_else(|| {
                    "EZVIZ cloud PTZ requires ezviz_camera_no in args or registry".to_string()
                })?;
            let result = connector.control_ptz(&EzvizPtzRequest {
                device_serial: device_serial.clone(),
                camera_no,
                direction: args.direction.clone().into(),
                speed: args.ezviz_speed,
            })?;
            return Ok(DevicePtzPayload {
                device_id: device.device_id,
                profile_token: String::new(),
                ptz_service_url: result.provider,
                action: result.action,
            });
        }

        let device_service_url = default_onvif_device_service_url(&device).ok_or_else(|| {
            format!(
                "device {} is missing ONVIF device_service url and IP address",
                device.device_id
            )
        })?;

        let result = self.ptz.ptz(&OnvifPtzRequest {
            device_service_url,
            username: args.username.clone(),
            password: args.password.clone(),
            direction: map_ptz_direction(args.direction.clone()),
            pan_speed: args.pan_speed,
            tilt_speed: args.tilt_speed,
        })?;

        Ok(DevicePtzPayload {
            device_id: device.device_id,
            profile_token: result.profile_token,
            ptz_service_url: result.ptz_service_url,
            action: result.action,
        })
    }
}

impl Executor for DeviceDiscoveryExecutor {
    fn route(&self) -> Route {
        Route::Mcp
    }

    fn supports(&self, action: &Action) -> bool {
        action.domain == "device"
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
                let args: DeviceDiscoverArgs = serde_json::from_value(merged)
                    .map_err(|e| format!("invalid discover args: {e}"))?;
                let mut result = self.service.discover(&args.into_request())?;
                result.connected_devices =
                    self.upsert_discovered_devices(&result.connected_devices)?;
                serde_json::to_value(DeviceDiscoverPayload { discovery: result })
                    .map_err(|e| format!("discover payload serialize failed: {e}"))?
            }
            "list" => {
                let _args: DeviceListArgs =
                    serde_json::from_value(merge_resource_and_args(action)).unwrap_or_default();
                serde_json::to_value(DeviceListPayload {
                    devices: self.devices_snapshot()?,
                })
                .map_err(|e| format!("list payload serialize failed: {e}"))?
            }
            "get" => {
                let args: DeviceGetArgs = serde_json::from_value(merge_resource_and_args(action))
                    .map_err(|e| format!("invalid get args: {e}"))?;
                let device = self.find_device(&args.device_id)?;
                serde_json::to_value(DeviceGetPayload { device })
                    .map_err(|e| format!("get payload serialize failed: {e}"))?
            }
            "update" => {
                let args: DeviceUpdateArgs =
                    serde_json::from_value(merge_resource_and_args(action))
                        .map_err(|e| format!("invalid update args: {e}"))?;
                let device = self.update_device(&args)?;
                serde_json::to_value(DeviceUpdatePayload { device })
                    .map_err(|e| format!("update payload serialize failed: {e}"))?
            }
            "snapshot" => {
                let args: DeviceSnapshotArgs =
                    serde_json::from_value(merge_resource_and_args(action))
                        .map_err(|e| format!("invalid snapshot args: {e}"))?;
                let device = self.find_device(&args.device_id)?;
                let snapshot = self
                    .service
                    .capture_snapshot(&args.into_request(&device))?
                    .with_device_context(
                        Some(device.name.clone()),
                        device.room.clone(),
                        device.vendor.clone(),
                        device.model.clone(),
                        Some(device.discovery_source.clone()),
                        Some(format!("{:?}", device.primary_stream.transport).to_lowercase()),
                        Some(device.primary_stream.requires_auth),
                    );
                serde_json::to_value(DeviceSnapshotPayload { snapshot })
                    .map_err(|e| format!("snapshot payload serialize failed: {e}"))?
            }
            "open_stream" => {
                let args: DeviceOpenStreamArgs =
                    serde_json::from_value(merge_resource_and_args(action))
                        .map_err(|e| format!("invalid open_stream args: {e}"))?;
                let device = self.find_device(&args.device_id)?;
                let stream = self
                    .service
                    .open_stream(&args.into_request(&device))?
                    .with_device_context(
                        Some(device.name.clone()),
                        device.room.clone(),
                        device.vendor.clone(),
                        device.model.clone(),
                        Some(device.discovery_source.clone()),
                        Some(format!("{:?}", device.primary_stream.transport).to_lowercase()),
                        Some(device.primary_stream.requires_auth),
                    );
                serde_json::to_value(DeviceOpenStreamPayload { stream })
                    .map_err(|e| format!("open_stream payload serialize failed: {e}"))?
            }
            "ptz" => {
                let args: DevicePtzArgs = serde_json::from_value(merge_resource_and_args(action))
                    .map_err(|e| format!("invalid ptz args: {e}"))?;
                let ptz = self.ptz_device(&args)?;
                serde_json::to_value(ptz)
                    .map_err(|e| format!("ptz payload serialize failed: {e}"))?
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

fn map_ptz_direction(direction: DevicePtzDirection) -> PtzDirection {
    match direction {
        DevicePtzDirection::Left => PtzDirection::Left,
        DevicePtzDirection::Right => PtzDirection::Right,
        DevicePtzDirection::Up => PtzDirection::Up,
        DevicePtzDirection::Down => PtzDirection::Down,
        DevicePtzDirection::Stop => PtzDirection::Stop,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::adapters::mdns::MdnsDiscoveryAdapter;
    use crate::adapters::onvif::OnvifDiscoveryAdapter;
    use crate::adapters::rtsp::RtspProbeAdapter;
    use crate::adapters::ssdp::SsdpDiscoveryAdapter;
    use crate::connectors::storage::StorageTarget;
    use crate::control_plane::devices::{
        ConnectivityState, DeviceKind as ControlDeviceKind, DeviceLifecycleState,
        DeviceRecord as ControlDeviceRecord, DeviceTwin,
    };
    use crate::orchestrator::contracts::Action;
    use crate::orchestrator::router::Executor;
    use crate::runtime::discovery::{
        DiscoveryCandidate, DiscoveryCandidateStatus, DiscoveryProtocol, DiscoveryRequest,
        RtspProbeRequest, RtspProbeResult,
    };
    use crate::runtime::media::{
        SnapshotCaptureRequest, SnapshotCaptureResult, StreamOpenRequest, StreamOpenResult,
    };
    use crate::runtime::registry::{
        CameraDevice, DeviceRegistrySnapshot, DeviceRegistryStore, StreamTransport,
    };

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

        fn capture_snapshot(
            &self,
            request: &SnapshotCaptureRequest,
        ) -> Result<SnapshotCaptureResult, String> {
            Ok(SnapshotCaptureResult::new(
                request.device_id.clone(),
                request.format,
                "ZmFrZS1qcGVn",
                9,
                request.storage_target,
            ))
        }

        fn open_stream(&self, request: &StreamOpenRequest) -> Result<StreamOpenResult, String> {
            Ok(StreamOpenResult::new(
                request.device_id.clone(),
                request.stream_url.clone(),
                request
                    .preferred_player
                    .clone()
                    .unwrap_or_else(|| "ffplay".to_string()),
                "/usr/bin/ffplay".into(),
                5151,
            ))
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

        let result = executor
            .execute(&action, "t1", "s1")
            .expect("discover result");
        assert_eq!(result.executor_used, "mcp");
        assert_eq!(
            result.status,
            crate::orchestrator::contracts::StepStatus::Success
        );
        assert_eq!(
            result.result_payload["discovery"]["connected_devices"][0]["primary_stream"]["url"],
            "rtsp://192.168.1.50/live"
        );
    }

    #[test]
    fn discover_auto_registers_devices_for_later_list_and_get() {
        let executor = DeviceDiscoveryExecutor::new(
            Box::new(StaticRtspAdapter),
            Some(Box::new(StaticOnvifAdapter)),
            Some(Box::new(EmptySsdpAdapter)),
            Some(Box::new(EmptyMdnsAdapter)),
        );

        let discover_action = Action {
            domain: "device".to_string(),
            operation: "discover".to_string(),
            resource: json!({"scan_id":"scan-1","network_cidr":"192.168.1.0/24"}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        executor
            .execute(&discover_action, "t1", "s1")
            .expect("discover result");

        let list_action = Action {
            domain: "device".to_string(),
            operation: "list".to_string(),
            resource: json!({}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let list_result = executor
            .execute(&list_action, "t1", "s2")
            .expect("list result");
        assert_eq!(
            list_result.result_payload["devices"][0]["ip_address"],
            "192.168.1.50"
        );

        let device_id = list_result.result_payload["devices"][0]["device_id"]
            .as_str()
            .expect("device id")
            .to_string();
        let get_action = Action {
            domain: "device".to_string(),
            operation: "get".to_string(),
            resource: json!({"device_id": device_id}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let get_result = executor
            .execute(&get_action, "t1", "s3")
            .expect("get result");
        assert_eq!(get_result.result_payload["device"]["name"], "Front Door");
    }

    #[test]
    fn update_changes_name_and_room_for_registered_device() {
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.50/live");
        let executor = DeviceDiscoveryExecutor::new(Box::new(StaticRtspAdapter), None, None, None)
            .with_devices(vec![device]);

        let update_action = Action {
            domain: "device".to_string(),
            operation: "update".to_string(),
            resource: json!({"device_id":"cam-1"}),
            args: json!({"name":"Living Room Cam","room":"Living Room"}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let update_result = executor
            .execute(&update_action, "t1", "s1")
            .expect("update result");

        assert_eq!(
            update_result.result_payload["device"]["name"],
            "Living Room Cam"
        );
        assert_eq!(
            update_result.result_payload["device"]["room"],
            "Living Room"
        );

        let list_action = Action {
            domain: "device".to_string(),
            operation: "list".to_string(),
            resource: json!({}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let list_result = executor
            .execute(&list_action, "t1", "s2")
            .expect("list result");
        assert_eq!(
            list_result.result_payload["devices"][0]["name"],
            "Living Room Cam"
        );
        assert_eq!(
            list_result.result_payload["devices"][0]["room"],
            "Living Room"
        );
    }

    #[test]
    fn discover_upserts_existing_device_without_duplication() {
        let mut device = CameraDevice::new("cam-existing", "Old Name", "rtsp://192.168.1.50/live");
        device.ip_address = Some("192.168.1.50".to_string());
        device.room = Some("Garage".to_string());

        let executor = DeviceDiscoveryExecutor::new(
            Box::new(StaticRtspAdapter),
            Some(Box::new(StaticOnvifAdapter)),
            Some(Box::new(EmptySsdpAdapter)),
            Some(Box::new(EmptyMdnsAdapter)),
        )
        .with_devices(vec![device]);

        let discover_action = Action {
            domain: "device".to_string(),
            operation: "discover".to_string(),
            resource: json!({"scan_id":"scan-1","network_cidr":"192.168.1.0/24"}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        executor
            .execute(&discover_action, "t1", "s1")
            .expect("discover result");

        let list_action = Action {
            domain: "device".to_string(),
            operation: "list".to_string(),
            resource: json!({}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let list_result = executor
            .execute(&list_action, "t1", "s2")
            .expect("list result");
        let devices = list_result.result_payload["devices"]
            .as_array()
            .expect("devices");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0]["room"], "Garage");
        assert_eq!(devices[0]["name"], "Front Door");
    }

    #[test]
    fn list_and_get_return_registered_devices() {
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.50/live");
        let executor = DeviceDiscoveryExecutor::new(Box::new(StaticRtspAdapter), None, None, None)
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
        let list_result = executor
            .execute(&list_action, "t1", "s1")
            .expect("list result");
        assert_eq!(
            list_result.result_payload["devices"][0]["device_id"],
            "cam-1"
        );

        let get_action = Action {
            domain: "device".to_string(),
            operation: "get".to_string(),
            resource: json!({"device_id":"cam-1"}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let get_result = executor
            .execute(&get_action, "t1", "s2")
            .expect("get result");
        assert_eq!(get_result.result_payload["device"]["name"], "Front Door");
    }

    #[test]
    fn snapshot_returns_capture_payload_for_registered_device() {
        let mut device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.50/live");
        device.capabilities.snapshot = true;
        let executor = DeviceDiscoveryExecutor::new(Box::new(StaticRtspAdapter), None, None, None)
            .with_devices(vec![device]);

        let snapshot_action = Action {
            domain: "device".to_string(),
            operation: "snapshot".to_string(),
            resource: json!({"device_id":"cam-1"}),
            args: json!({"storage_target":"local_disk"}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let snapshot_result = executor
            .execute(&snapshot_action, "t1", "s3")
            .expect("snapshot result");

        assert_eq!(
            snapshot_result.result_payload["snapshot"]["device_id"],
            "cam-1"
        );
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["mime_type"],
            "image/jpeg"
        );
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["storage"]["target"],
            json!(StorageTarget::LocalDisk)
        );
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["ingest_metadata"]["provenance"],
            "media"
        );
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["ingest_metadata"]["ingest_disposition"],
            "knowledge_index_candidate"
        );
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["ingest_metadata"]["stream_transport"],
            "rtsp"
        );
    }

    #[test]
    fn open_stream_returns_launch_payload_for_registered_device() {
        let device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.50/live");
        let executor = DeviceDiscoveryExecutor::new(Box::new(StaticRtspAdapter), None, None, None)
            .with_devices(vec![device]);

        let open_action = Action {
            domain: "device".to_string(),
            operation: "open_stream".to_string(),
            resource: json!({"device_id":"cam-1"}),
            args: json!({"preferred_player":"mpv"}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let open_result = executor
            .execute(&open_action, "t1", "s4")
            .expect("open stream result");

        assert_eq!(open_result.result_payload["stream"]["device_id"], "cam-1");
        assert_eq!(open_result.result_payload["stream"]["player"], "mpv");
        assert_eq!(open_result.result_payload["stream"]["process_id"], 5151);
        assert_eq!(
            open_result.result_payload["stream"]["ingest_metadata"]["provenance"],
            "control"
        );
        assert_eq!(
            open_result.result_payload["stream"]["ingest_metadata"]["ingest_disposition"],
            "runtime_only"
        );
        assert_eq!(
            open_result.result_payload["stream"]["ingest_metadata"]["stream_transport"],
            "rtsp"
        );
    }

    #[test]
    fn snapshot_and_open_stream_keep_media_and_control_paths_separate() {
        let mut device = CameraDevice::new("cam-1", "Front Door", "rtsp://192.168.1.50/live");
        device.capabilities.snapshot = true;
        device.capabilities.stream = true;
        device.capabilities.ptz = true;
        let executor = DeviceDiscoveryExecutor::new(Box::new(StaticRtspAdapter), None, None, None)
            .with_devices(vec![device]);

        let snapshot_action = Action {
            domain: "device".to_string(),
            operation: "snapshot".to_string(),
            resource: json!({"device_id":"cam-1"}),
            args: json!({"storage_target":"local_disk"}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let snapshot_result = executor
            .execute(&snapshot_action, "t1", "s4")
            .expect("snapshot result");
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["storage"]["target"],
            json!(StorageTarget::LocalDisk)
        );
        assert!(
            snapshot_result.result_payload["snapshot"]["storage"]["relative_path"]
                .as_str()
                .expect("snapshot path")
                .starts_with("snapshots/cam-1/")
        );
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["ingest_metadata"]["device_name"],
            "Front Door"
        );
        assert_eq!(
            snapshot_result.result_payload["snapshot"]["ingest_metadata"]["room"],
            json!(null)
        );

        let open_action = Action {
            domain: "device".to_string(),
            operation: "open_stream".to_string(),
            resource: json!({"device_id":"cam-1"}),
            args: json!({"preferred_player":"mpv"}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let open_result = executor
            .execute(&open_action, "t1", "s5")
            .expect("open stream result");
        assert_eq!(open_result.result_payload["stream"]["player"], "mpv");
        assert_eq!(open_result.result_payload["stream"]["device_id"], "cam-1");
        assert_eq!(
            open_result.result_payload["stream"]["ingest_metadata"]["provenance"],
            "control"
        );
        assert_eq!(
            open_result.result_payload["stream"]["ingest_metadata"]["device_name"],
            "Front Door"
        );
        assert_eq!(
            open_result.result_payload["stream"]["ingest_metadata"]["stream_transport"],
            "rtsp"
        );

        let list_action = Action {
            domain: "device".to_string(),
            operation: "list".to_string(),
            resource: json!({}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let list_result = executor
            .execute(&list_action, "t1", "s6")
            .expect("list result");
        assert_eq!(
            list_result.result_payload["devices"][0]["capabilities"]["snapshot"],
            true
        );
        assert_eq!(
            list_result.result_payload["devices"][0]["capabilities"]["ptz"],
            true
        );
    }

    #[test]
    fn discover_persists_registry_to_disk_when_store_is_configured() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("harborbeacon-device-executor-{unique}.json"));
        let store = DeviceRegistryStore::new(&path);
        let executor = DeviceDiscoveryExecutor::new(
            Box::new(StaticRtspAdapter),
            Some(Box::new(StaticOnvifAdapter)),
            Some(Box::new(EmptySsdpAdapter)),
            Some(Box::new(EmptyMdnsAdapter)),
        )
        .with_registry_store(store.clone())
        .expect("attach registry store");

        let discover_action = Action {
            domain: "device".to_string(),
            operation: "discover".to_string(),
            resource: json!({"scan_id":"scan-1","network_cidr":"192.168.1.0/24"}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        executor
            .execute(&discover_action, "t1", "s1")
            .expect("discover result");

        let persisted = store.load_devices().expect("load persisted devices");
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].ip_address.as_deref(), Some("192.168.1.50"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn discover_preserves_non_camera_platform_records_when_store_is_configured() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "harborbeacon-device-executor-platform-{unique}.json"
        ));
        let store = DeviceRegistryStore::new(&path);
        store
            .save_snapshot(&DeviceRegistrySnapshot {
                devices: vec![ControlDeviceRecord {
                    device_id: "light-1".to_string(),
                    workspace_id: "home".to_string(),
                    kind: ControlDeviceKind::Light,
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
                }],
                device_twins: vec![DeviceTwin {
                    device_id: "light-1".to_string(),
                    connectivity_state: ConnectivityState::Online,
                    reported_state: json!({"power":"on"}),
                    desired_state: json!({}),
                    health_state: json!({}),
                    last_event_id: None,
                    last_seen_at: None,
                }],
                ..DeviceRegistrySnapshot::default()
            })
            .expect("persist initial snapshot");

        let executor = DeviceDiscoveryExecutor::new(
            Box::new(StaticRtspAdapter),
            Some(Box::new(StaticOnvifAdapter)),
            Some(Box::new(EmptySsdpAdapter)),
            Some(Box::new(EmptyMdnsAdapter)),
        )
        .with_registry_store(store.clone())
        .expect("attach registry store");

        let discover_action = Action {
            domain: "device".to_string(),
            operation: "discover".to_string(),
            resource: json!({"scan_id":"scan-1","network_cidr":"192.168.1.0/24"}),
            args: json!({}),
            risk_level: crate::orchestrator::contracts::RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        executor
            .execute(&discover_action, "t1", "s1")
            .expect("discover result");

        let snapshot = store.load_snapshot().expect("load persisted snapshot");
        assert!(
            snapshot
                .devices
                .iter()
                .any(|device| device.device_id == "light-1"
                    && device.kind == ControlDeviceKind::Light)
        );
        assert!(snapshot
            .devices
            .iter()
            .any(|device| device.device_id == "cam-cand-1"
                && device.kind == ControlDeviceKind::Camera));

        let _ = std::fs::remove_file(&path);
    }
}
