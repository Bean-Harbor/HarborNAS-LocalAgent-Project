//! Vision domain actions for detection and image understanding.

use serde::{Deserialize, Serialize};

use crate::runtime::registry::CameraDevice;

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
    pub device: CameraDevice,
    pub summary: String,
    pub summary_source: String,
    #[serde(default)]
    pub detections: Vec<VisionDetection>,
    pub snapshot: VisionImageArtifact,
    pub detection_summary: String,
    pub feishu_card: serde_json::Value,
}

fn default_detect_label() -> String {
    "person".to_string()
}

fn default_min_confidence() -> f32 {
    0.25
}

#[cfg(test)]
mod tests {
    use super::{VisionAnalyzeCameraArgs, OP_ANALYZE_CAMERA};

    #[test]
    fn analyze_camera_args_use_person_defaults() {
        let args: VisionAnalyzeCameraArgs =
            serde_json::from_str(r#"{"device_id":"cam-1"}"#).expect("parse args");

        assert_eq!(OP_ANALYZE_CAMERA, "analyze_camera");
        assert_eq!(args.detect_label, "person");
        assert!((args.min_confidence - 0.25).abs() < f32::EPSILON);
    }
}
