//! Camera, stream, recording, and media asset schemas.

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
