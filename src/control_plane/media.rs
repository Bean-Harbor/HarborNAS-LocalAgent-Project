//! Camera, stream, recording, and media asset schemas.

use std::convert::TryFrom;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StreamTransport {
    #[default]
    Rtsp,
    Hls,
    Webrtc,
    File,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CameraProfile {
    pub device_id: String,
    #[serde(default)]
    pub default_stream_profile_id: Option<String>,
    #[serde(default)]
    pub audio_supported: bool,
    #[serde(default)]
    pub ptz_supported: bool,
    #[serde(default)]
    pub privacy_supported: bool,
    #[serde(default)]
    pub playback_supported: bool,
    #[serde(default)]
    pub recording_policy_id: Option<String>,
    #[serde(default)]
    pub vendor_features: Value,
}

impl CameraProfile {
    pub fn vendor_feature_str(&self, key: &str) -> Option<&str> {
        self.vendor_features.get(key).and_then(Value::as_str)
    }

    pub fn native_snapshot_url(&self) -> Option<&str> {
        self.vendor_feature_str("native_snapshot_url").or_else(|| {
            self.vendor_features
                .pointer("/native_snapshot/url")
                .and_then(Value::as_str)
        })
    }

    pub fn rtsp_path_candidates(&self) -> Vec<String> {
        let mut candidates = Vec::new();

        if let Some(entries) = self
            .vendor_features
            .get("rtsp_path_candidates")
            .and_then(Value::as_array)
        {
            for candidate in entries
                .iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_rtsp_candidate)
            {
                if !candidates.iter().any(|existing| existing == &candidate) {
                    candidates.push(candidate);
                }
            }
        }

        candidates
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StreamProfile {
    pub stream_profile_id: String,
    pub device_id: String,
    pub profile_name: String,
    pub transport: StreamTransport,
    pub endpoint_id: String,
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f32>,
    #[serde(default)]
    pub bitrate_kbps: Option<u32>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordingTriggerMode {
    #[default]
    Continuous,
    Event,
    Manual,
    Schedule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StorageTargetKind {
    #[default]
    Nas,
    LocalDisk,
    HarborOsPool,
    ObjectStorage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RecordingPolicy {
    pub recording_policy_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub device_id: Option<String>,
    pub trigger_mode: RecordingTriggerMode,
    #[serde(default)]
    pub pre_event_seconds: u32,
    #[serde(default)]
    pub post_event_seconds: u32,
    #[serde(default)]
    pub clip_length_seconds: u32,
    #[serde(default)]
    pub retention_days: u32,
    pub storage_target: StorageTargetKind,
    #[serde(default)]
    pub metadata: Value,
}

impl RecordingPolicy {
    pub fn metadata_str(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).and_then(Value::as_str)
    }

    pub fn metadata_u32(&self, key: &str) -> Option<u32> {
        self.metadata
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    }

    pub fn capture_subdirectory(&self) -> Option<&str> {
        self.metadata_str("capture_subdirectory")
            .or_else(|| self.metadata_str("capture_folder_path"))
    }

    pub fn keyframe_count_hint(&self) -> Option<u32> {
        self.metadata_u32("keyframe_count")
            .or_else(|| self.metadata_u32("keyframe_sample_count"))
    }

    pub fn keyframe_interval_seconds_hint(&self) -> Option<u32> {
        self.metadata_u32("keyframe_interval_seconds")
            .or_else(|| self.metadata_u32("keyframe_interval"))
    }

    pub fn clip_length_seconds_hint(&self) -> Option<u32> {
        if self.clip_length_seconds > 0 {
            Some(self.clip_length_seconds)
        } else {
            self.metadata_u32("clip_length_seconds")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaAssetKind {
    #[default]
    Snapshot,
    Clip,
    Recording,
    Replay,
    Derived,
    Report,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MediaAsset {
    pub asset_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub device_id: Option<String>,
    pub asset_kind: MediaAssetKind,
    pub storage_target: StorageTargetKind,
    pub storage_uri: String,
    pub mime_type: String,
    #[serde(default)]
    pub byte_size: u64,
    #[serde(default)]
    pub checksum: Option<String>,
    #[serde(default)]
    pub captured_at: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub derived_from_asset_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaSessionKind {
    #[default]
    LiveView,
    Replay,
    Share,
    Proxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaDeliveryMode {
    #[default]
    LocalPlayer,
    Webrtc,
    Hls,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaSessionStatus {
    #[default]
    Opening,
    Active,
    Closed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MediaSession {
    pub media_session_id: String,
    pub device_id: String,
    pub stream_profile_id: String,
    pub session_kind: MediaSessionKind,
    pub delivery_mode: MediaDeliveryMode,
    #[serde(default)]
    pub opened_by_user_id: Option<String>,
    pub status: MediaSessionStatus,
    #[serde(default)]
    pub share_link_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShareAccessScope {
    #[default]
    PublicLink,
    Workspace,
    InviteOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ShareLink {
    pub share_link_id: String,
    pub media_session_id: String,
    pub token_hash: String,
    pub access_scope: ShareAccessScope,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

fn normalize_rtsp_candidate(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CameraProfile, RecordingPolicy, RecordingTriggerMode, StorageTargetKind};

    #[test]
    fn camera_profile_helpers_read_native_snapshot_and_vendor_paths() {
        let profile = CameraProfile {
            device_id: "cam-1".to_string(),
            default_stream_profile_id: Some("stream-1".to_string()),
            audio_supported: false,
            ptz_supported: false,
            privacy_supported: false,
            playback_supported: false,
            recording_policy_id: None,
            vendor_features: json!({
                "profile": "tp-link/tapo",
                "native_snapshot_url": "http://192.168.1.10/snapshot.jpg",
                "rtsp_path_candidates": ["stream1", "/stream2", "/stream2"]
            }),
        };

        assert_eq!(
            profile.native_snapshot_url(),
            Some("http://192.168.1.10/snapshot.jpg")
        );
        assert_eq!(
            profile.rtsp_path_candidates(),
            vec!["/stream1".to_string(), "/stream2".to_string()]
        );
    }

    #[test]
    fn recording_policy_helpers_read_clip_metadata_hints() {
        let policy = RecordingPolicy {
            recording_policy_id: "policy-1".to_string(),
            workspace_id: "workspace-1".to_string(),
            device_id: Some("cam-1".to_string()),
            trigger_mode: RecordingTriggerMode::Manual,
            pre_event_seconds: 3,
            post_event_seconds: 2,
            clip_length_seconds: 12,
            retention_days: 7,
            storage_target: StorageTargetKind::HarborOsPool,
            metadata: json!({
                "capture_subdirectory": "camera-archive/front-door",
                "keyframe_count": 4,
                "keyframe_interval_seconds": 2,
            }),
        };

        assert_eq!(policy.capture_subdirectory(), Some("camera-archive/front-door"));
        assert_eq!(policy.keyframe_count_hint(), Some(4));
        assert_eq!(policy.keyframe_interval_seconds_hint(), Some(2));
        assert_eq!(policy.clip_length_seconds_hint(), Some(12));
    }
}
