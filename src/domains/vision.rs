//! Vision domain actions for detection and image understanding.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::registry::ResolvedCameraTarget;

pub const DOMAIN: &str = "vision";
pub const OP_ANALYZE_CAMERA: &str = "analyze_camera";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisionAnalyzeCameraArgs {
    pub device_id: String,
    #[serde(default = "default_detect_label")]
    pub detect_label: String,
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisionDetection {
    pub label: String,
    pub confidence: f32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisionImageArtifact {
    pub image_path: String,
    pub annotated_image_path: Option<String>,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisionAnalyzeCameraPayload {
    #[serde(rename = "camera_target", alias = "device")]
    pub camera_target: ResolvedCameraTarget,
    pub summary: String,
    pub summary_source: String,
    #[serde(default)]
    pub detections: Vec<VisionDetection>,
    pub snapshot: VisionImageArtifact,
    pub detection_summary: String,
    #[serde(default = "default_notification_channel")]
    pub notification_channel: String,
    #[serde(default = "default_notification_format")]
    pub notification_format: String,
    #[serde(default, alias = "feishu_card")]
    pub notification_card: Value,
}

fn default_detect_label() -> String {
    "person".to_string()
}

fn default_min_confidence() -> f32 {
    0.25
}

fn default_notification_channel() -> String {
    "im_bridge".to_string()
}

fn default_notification_format() -> String {
    "lark_card".to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{VisionAnalyzeCameraArgs, VisionAnalyzeCameraPayload, OP_ANALYZE_CAMERA};

    #[test]
    fn analyze_camera_args_use_person_defaults() {
        let args: VisionAnalyzeCameraArgs =
            serde_json::from_str(r#"{"device_id":"cam-1"}"#).expect("parse args");

        assert_eq!(OP_ANALYZE_CAMERA, "analyze_camera");
        assert_eq!(args.detect_label, "person");
        assert!((args.min_confidence - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn payload_accepts_legacy_feishu_card_alias() {
        let payload: VisionAnalyzeCameraPayload = serde_json::from_value(json!({
            "device": {
                "device_id": "cam-1",
                "name": "Front Door",
                "kind": "camera",
                "status": "online",
                "room": null,
                "vendor": null,
                "model": null,
                "ip_address": "192.168.1.10",
                "mac_address": null,
                "discovery_source": "onvif",
                "primary_stream": {
                    "transport": "rtsp",
                    "url": "rtsp://192.168.1.10/live",
                    "requires_auth": false
                },
                "onvif_device_service_url": null,
                "ezviz_device_serial": null,
                "ezviz_camera_no": null,
                "capabilities": {
                    "snapshot": true,
                    "stream": true,
                    "ptz": false,
                    "audio": false
                },
                "last_seen_at": null
            },
            "summary": "画面中有 1 人",
            "summary_source": "openai_compatible",
            "detections": [],
            "snapshot": {
                "image_path": "snap.jpg",
                "annotated_image_path": null,
                "mime_type": "image/jpeg"
            },
            "detection_summary": "检测到 1 个 person，最高置信度 88%。",
            "feishu_card": {"header": {"title": {"content": "Front Door AI 分析"}}}
        }))
        .expect("payload");

        assert_eq!(payload.notification_channel, "im_bridge");
        assert_eq!(payload.notification_format, "lark_card");
        assert_eq!(payload.camera_target.device_id, "cam-1");
        assert_eq!(
            payload.notification_card["header"]["title"]["content"],
            "Front Door AI 分析"
        );
    }
}
