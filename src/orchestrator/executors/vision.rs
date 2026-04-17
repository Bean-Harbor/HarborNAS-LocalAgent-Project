use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;

use crate::adapters::rtsp::{CommandRtspAdapter, RtspProbeAdapter};
use crate::connectors::ai_provider::{
    OpenAiCompatibleConfig, OpenAiCompatibleVisionClient, VisionDetectionRequest,
    VisionSidecarClient, VisionSidecarConfig, VisionSummaryRequest,
};
use crate::connectors::storage::StorageTarget;
use crate::domains::vision::{
    VisionAnalyzeCameraArgs, VisionAnalyzeCameraPayload, VisionDetection, VisionImageArtifact,
};
use crate::orchestrator::contracts::{Action, ExecutionResult, Route, StepStatus};
use crate::orchestrator::router::Executor;
use crate::runtime::media::{SnapshotCaptureRequest, SnapshotCaptureResult, SnapshotFormat};
use crate::runtime::registry::{DeviceRegistryStore, ResolvedCameraTarget};

pub struct VisionExecutor {
    registry_store: DeviceRegistryStore,
    rtsp: Box<dyn RtspProbeAdapter>,
    python_bin: String,
    detector_script: PathBuf,
    bridge_script: PathBuf,
    artifact_root: PathBuf,
}

impl VisionExecutor {
    pub fn new(registry_store: DeviceRegistryStore) -> Self {
        let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let detector_script = repo_root.join("tools/detect_person_yolo.py");
        let bridge_script = repo_root.join("tools/vision_detect_bridge.sh");
        let default_venv_python = repo_root.join(".harbornas/.venv-vision/bin/python");
        let python_bin = std::env::var("HARBOR_VISION_PYTHON").unwrap_or_else(|_| {
            if default_venv_python.exists() {
                return default_venv_python.to_string_lossy().to_string();
            }

            which::which("python3")
                .or_else(|_| which::which("python"))
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|_| "python3".to_string())
        });
        Self {
            registry_store,
            rtsp: Box::new(CommandRtspAdapter::default()),
            python_bin,
            detector_script,
            bridge_script,
            artifact_root: PathBuf::from(".harbornas/vision"),
        }
    }

    fn analyze_camera(
        &self,
        args: &VisionAnalyzeCameraArgs,
    ) -> Result<VisionAnalyzeCameraPayload, String> {
        let device = self.find_device(&args.device_id)?;
        let snapshot = self.capture_snapshot(&device)?;
        let stored_snapshot = self.persist_snapshot(&device, &snapshot)?;
        let detections = self.run_detection(&stored_snapshot.image_path, args)?;
        let detection_summary = describe_detections(&detections.detections, &args.detect_label);
        let (summary, summary_source) =
            self.describe_with_model_or_fallback(&snapshot, &detection_summary, args)?;
        let notification_card =
            build_notification_card(&device, &summary, &summary_source, &detection_summary);

        Ok(VisionAnalyzeCameraPayload {
            camera_target: device.clone(),
            summary,
            summary_source,
            detections: detections.detections,
            snapshot: VisionImageArtifact {
                image_path: stored_snapshot.image_path,
                annotated_image_path: detections
                    .annotated_image_path
                    .map(|path| path.to_string_lossy().to_string()),
                mime_type: snapshot.mime_type,
                source_storage: Some(snapshot.storage.clone()),
                byte_size: Some(snapshot.byte_size as u64),
                captured_at_epoch_ms: Some(snapshot.captured_at_epoch_ms),
            },
            detection_summary,
            notification_channel: "im_bridge".to_string(),
            notification_format: "lark_card".to_string(),
            notification_card,
        })
    }

    fn find_device(&self, device_id: &str) -> Result<ResolvedCameraTarget, String> {
        self.registry_store.resolve_camera_target(device_id)
    }

    fn capture_snapshot(
        &self,
        device: &ResolvedCameraTarget,
    ) -> Result<SnapshotCaptureResult, String> {
        self.rtsp.capture_snapshot(&SnapshotCaptureRequest::new(
            device.device_id.clone(),
            device.primary_stream.url.clone(),
            SnapshotFormat::Jpeg,
            StorageTarget::LocalDisk,
        ))
    }

    fn persist_snapshot(
        &self,
        device: &ResolvedCameraTarget,
        snapshot: &SnapshotCaptureResult,
    ) -> Result<VisionImageArtifact, String> {
        let image_bytes = base64::engine::general_purpose::STANDARD
            .decode(&snapshot.bytes_base64)
            .map_err(|e| format!("failed to decode snapshot bytes: {e}"))?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let device_slug = sanitize_path_segment(&device.device_id);

        let snapshot_dir = self.artifact_root.join("snapshots");
        fs::create_dir_all(&snapshot_dir).map_err(|e| {
            format!(
                "failed to create vision snapshot directory {}: {e}",
                snapshot_dir.display()
            )
        })?;
        let image_path = snapshot_dir.join(format!("{device_slug}-{ts}.jpg"));
        fs::write(&image_path, image_bytes)
            .map_err(|e| format!("failed to write snapshot {}: {e}", image_path.display()))?;

        Ok(VisionImageArtifact {
            image_path: image_path.to_string_lossy().to_string(),
            annotated_image_path: None,
            mime_type: snapshot.mime_type.clone(),
            source_storage: Some(snapshot.storage.clone()),
            byte_size: Some(snapshot.byte_size as u64),
            captured_at_epoch_ms: Some(snapshot.captured_at_epoch_ms),
        })
    }

    fn run_detection(
        &self,
        image_path: &str,
        args: &VisionAnalyzeCameraArgs,
    ) -> Result<DetectionOutput, String> {
        let annotated_dir = self.artifact_root.join("annotated");
        fs::create_dir_all(&annotated_dir).map_err(|e| {
            format!(
                "failed to create vision annotated directory {}: {e}",
                annotated_dir.display()
            )
        })?;
        let annotated_path =
            annotated_dir.join(Path::new(image_path).file_name().unwrap_or_default());

        let image_path = fs::canonicalize(image_path)
            .map_err(|e| format!("failed to canonicalize snapshot path: {e}"))?;
        let annotated_path = annotated_path.canonicalize().unwrap_or(annotated_path);

        if let Some(config) = VisionSidecarConfig::from_env() {
            let client = VisionSidecarClient::new(config)?;
            if client.healthz().is_ok() {
                return self.run_detection_via_sidecar(client, &image_path, &annotated_path, args);
            }
        }

        self.run_detection_via_bridge(&image_path, &annotated_path, args)
    }

    fn run_detection_via_sidecar(
        &self,
        client: VisionSidecarClient,
        image_path: &Path,
        annotated_path: &Path,
        args: &VisionAnalyzeCameraArgs,
    ) -> Result<DetectionOutput, String> {
        let response = client.detect(&VisionDetectionRequest {
            image_path: image_path.to_string_lossy().to_string(),
            label: args.detect_label.clone(),
            min_confidence: args.min_confidence,
            annotated_output: annotated_path.to_string_lossy().to_string(),
        })?;

        let detections = response
            .detections
            .into_iter()
            .map(|value| {
                serde_json::from_value(value)
                    .map_err(|e| format!("vision sidecar returned invalid detection payload: {e}"))
            })
            .collect::<Result<Vec<VisionDetection>, String>>()?;

        Ok(DetectionOutput {
            detections,
            annotated_image_path: response.annotated_image_path.map(PathBuf::from),
        })
    }

    fn run_detection_via_bridge(
        &self,
        image_path: &Path,
        annotated_path: &Path,
        args: &VisionAnalyzeCameraArgs,
    ) -> Result<DetectionOutput, String> {
        let output = Command::new(&self.bridge_script)
            .arg("--image")
            .arg(image_path)
            .arg("--label")
            .arg(&args.detect_label)
            .arg("--min-confidence")
            .arg(args.min_confidence.to_string())
            .arg("--annotated-output")
            .arg(annotated_path)
            .env("HARBOR_VISION_PYTHON", &self.python_bin)
            .output()
            .map_err(|e| format!("failed to launch YOLO bridge: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "detector exited without stderr output".to_string()
            } else {
                stderr
            };
            return Err(format!("YOLO detector failed: {detail}"));
        }

        let mut detected: DetectionOutput = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("failed to parse detector output: {e}"))?;
        if detected.annotated_image_path.is_none() && annotated_path.exists() {
            detected.annotated_image_path = Some(annotated_path.to_path_buf());
        }
        Ok(detected)
    }

    #[allow(dead_code)]
    fn run_detection_direct_python(
        &self,
        image_path: &Path,
        annotated_path: &Path,
        args: &VisionAnalyzeCameraArgs,
    ) -> Result<DetectionOutput, String> {
        let shell_command = format!(
            "env -i HOME={home} PATH={path} LANG={lang} {python} {script} --image {image} --label {label} --min-confidence {conf} --annotated-output {annotated}",
            home = shell_escape(&std::env::var("HOME").unwrap_or_default()),
            path = shell_escape(&std::env::var("PATH").unwrap_or_default()),
            lang = shell_escape(&std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string())),
            python = shell_escape(&self.python_bin),
            script = shell_escape(&self.detector_script.to_string_lossy()),
            image = shell_escape(&image_path.to_string_lossy()),
            label = shell_escape(&args.detect_label),
            conf = shell_escape(&args.min_confidence.to_string()),
            annotated = shell_escape(&annotated_path.to_string_lossy()),
        );

        let output = Command::new("/bin/zsh")
            .arg("-lc")
            .arg(shell_command)
            .output()
            .map_err(|e| format!("failed to launch YOLO detector: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "detector exited without stderr output".to_string()
            } else {
                stderr
            };
            return Err(format!("YOLO detector failed: {detail}"));
        }

        let mut detected: DetectionOutput = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("failed to parse detector output: {e}"))?;
        if detected.annotated_image_path.is_none() && annotated_path.exists() {
            detected.annotated_image_path = Some(annotated_path.to_path_buf());
        }
        Ok(detected)
    }

    fn describe_with_model_or_fallback(
        &self,
        snapshot: &SnapshotCaptureResult,
        detection_summary: &str,
        args: &VisionAnalyzeCameraArgs,
    ) -> Result<(String, String), String> {
        if let Some(config) = OpenAiCompatibleConfig::from_env() {
            let image_data_url = format!(
                "data:{};base64,{}",
                snapshot.mime_type, snapshot.bytes_base64
            );
            let client = OpenAiCompatibleVisionClient::new(config)?;
            let response = client.describe_frame(&VisionSummaryRequest {
                image_data_url,
                detection_summary: detection_summary.to_string(),
                user_prompt: args.prompt.clone(),
            })?;
            return Ok((response.summary, "openai_compatible".to_string()));
        }

        Ok((
            heuristic_summary(detection_summary),
            "heuristic_fallback".to_string(),
        ))
    }
}

impl Executor for VisionExecutor {
    fn route(&self) -> Route {
        Route::Mcp
    }

    fn supports(&self, action: &Action) -> bool {
        action.domain == "vision"
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
            "analyze_camera" => {
                let args: VisionAnalyzeCameraArgs =
                    serde_json::from_value(merge_resource_and_args(action))
                        .map_err(|e| format!("invalid analyze_camera args: {e}"))?;
                let result = self.analyze_camera(&args)?;
                serde_json::to_value(result)
                    .map_err(|e| format!("vision payload serialize failed: {e}"))?
            }
            other => return Err(format!("unsupported vision operation: {other}")),
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

fn heuristic_summary(detection_summary: &str) -> String {
    if detection_summary.contains("未检测到") {
        "当前画面未检测到明显人员活动。".to_string()
    } else {
        format!("{detection_summary}，建议查看抓拍图确认现场情况。")
    }
}

fn describe_detections(detections: &[VisionDetection], label: &str) -> String {
    if detections.is_empty() {
        return format!("未检测到 {label}。");
    }

    let max_confidence = detections
        .iter()
        .map(|detection| detection.confidence)
        .fold(0.0f32, f32::max);
    format!(
        "检测到 {} 个 {}，最高置信度 {:.0}%。",
        detections.len(),
        label,
        max_confidence * 100.0
    )
}

fn build_notification_card(
    device: &ResolvedCameraTarget,
    summary: &str,
    summary_source: &str,
    detection_summary: &str,
) -> serde_json::Value {
    json!({
        "config": {"wide_screen_mode": true},
        "header": {
            "title": {"tag": "plain_text", "content": format!("{} AI 分析", device.display_name)},
            "template": "green"
        },
        "elements": [
            {
                "tag": "div",
                "text": {"tag": "lark_md", "content": format!("**摘要**\n{}", summary)}
            },
            {
                "tag": "div",
                "text": {"tag": "lark_md", "content": format!("**检测结果**\n{}", detection_summary)}
            },
            {
                "tag": "note",
                "elements": [
                    {"tag": "plain_text", "content": format!("device={} | source={}", device.device_id, summary_source)}
                ]
            }
        ]
    })
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[derive(Debug, Deserialize)]
struct DetectionOutput {
    #[serde(default)]
    detections: Vec<VisionDetection>,
    #[serde(default)]
    annotated_image_path: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::{build_notification_card, describe_detections, heuristic_summary};
    use crate::domains::vision::VisionDetection;
    use crate::runtime::registry::{
        CameraCapabilities, CameraStreamRef, DeviceStatus, ResolvedCameraTarget, StreamTransport,
    };

    #[test]
    fn detection_summary_reports_empty_result() {
        assert_eq!(describe_detections(&[], "person"), "未检测到 person。");
    }

    #[test]
    fn heuristic_summary_mentions_no_activity() {
        assert_eq!(
            heuristic_summary("未检测到 person。"),
            "当前画面未检测到明显人员活动。"
        );
    }

    #[test]
    fn notification_card_includes_summary_text() {
        let device = ResolvedCameraTarget {
            device_id: "cam-1".to_string(),
            display_name: "Front Door".to_string(),
            status: DeviceStatus::Online,
            room_name: None,
            vendor: None,
            model: None,
            ip_address: Some("192.168.1.10".to_string()),
            mac_address: None,
            discovery_source: "onvif".to_string(),
            primary_stream: CameraStreamRef {
                transport: StreamTransport::Rtsp,
                url: "rtsp://192.168.1.10/live".to_string(),
                requires_auth: false,
            },
            onvif_device_service_url: None,
            ezviz_device_serial: None,
            ezviz_camera_no: None,
            capabilities: CameraCapabilities::default(),
            last_seen_at: None,
        };
        let card = build_notification_card(
            &device,
            "画面中有 1 人",
            "openai_compatible",
            "检测到 1 个 person，最高置信度 88%。",
        );
        assert_eq!(card["header"]["title"]["content"], "Front Door AI 分析");
        assert!(card["elements"][0]["text"]["content"]
            .as_str()
            .expect("summary text")
            .contains("画面中有 1 人"));
    }

    #[test]
    fn detection_summary_reports_highest_confidence() {
        let detections = vec![
            VisionDetection {
                label: "person".to_string(),
                confidence: 0.71,
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            },
            VisionDetection {
                label: "person".to_string(),
                confidence: 0.88,
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            },
        ];

        assert_eq!(
            describe_detections(&detections, "person"),
            "检测到 2 个 person，最高置信度 88%。"
        );
    }
}
