//! Snapshot, clip, stream, and timeline processing primitives.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::connectors::storage::{StorageObjectRef, StorageTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAssetKind {
    Snapshot,
    Clip,
    Stream,
    TimelineEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotFormat {
    Jpeg,
    Png,
}

impl SnapshotFormat {
    pub fn file_extension(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

impl Default for SnapshotFormat {
    fn default() -> Self {
        Self::Jpeg
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCaptureRequest {
    pub device_id: String,
    pub stream_url: String,
    #[serde(default)]
    pub format: SnapshotFormat,
    #[serde(default)]
    pub storage_target: StorageTarget,
}

impl SnapshotCaptureRequest {
    pub fn new(
        device_id: impl Into<String>,
        stream_url: impl Into<String>,
        format: SnapshotFormat,
        storage_target: StorageTarget,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            stream_url: stream_url.into(),
            format,
            storage_target,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCaptureResult {
    pub device_id: String,
    pub asset_kind: MediaAssetKind,
    pub format: SnapshotFormat,
    pub mime_type: String,
    pub byte_size: usize,
    pub bytes_base64: String,
    pub storage: StorageObjectRef,
    #[serde(default)]
    pub captured_at_epoch_ms: u128,
}

impl SnapshotCaptureResult {
    pub fn new(
        device_id: impl Into<String>,
        format: SnapshotFormat,
        bytes_base64: impl Into<String>,
        byte_size: usize,
        storage_target: StorageTarget,
    ) -> Self {
        let device_id = device_id.into();
        let captured_at_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let relative_path = format!(
            "snapshots/{}/{}.{}",
            sanitize_path_segment(&device_id),
            captured_at_epoch_ms,
            format.file_extension()
        );

        Self {
            device_id,
            asset_kind: MediaAssetKind::Snapshot,
            format,
            mime_type: format.mime_type().to_string(),
            byte_size,
            bytes_base64: bytes_base64.into(),
            storage: StorageObjectRef {
                target: storage_target,
                relative_path,
            },
            captured_at_epoch_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamOpenRequest {
    pub device_id: String,
    pub stream_url: String,
    #[serde(default)]
    pub preferred_player: Option<String>,
}

impl StreamOpenRequest {
    pub fn new(
        device_id: impl Into<String>,
        stream_url: impl Into<String>,
        preferred_player: Option<String>,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            stream_url: stream_url.into(),
            preferred_player,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamOpenResult {
    pub device_id: String,
    pub asset_kind: MediaAssetKind,
    pub stream_url: String,
    pub player: String,
    pub player_path: PathBuf,
    pub process_id: u32,
}

impl StreamOpenResult {
    pub fn new(
        device_id: impl Into<String>,
        stream_url: impl Into<String>,
        player: impl Into<String>,
        player_path: PathBuf,
        process_id: u32,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            asset_kind: MediaAssetKind::Stream,
            stream_url: stream_url.into(),
            player: player.into(),
            player_path,
            process_id,
        }
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{SnapshotCaptureResult, SnapshotFormat, StreamOpenRequest, StreamOpenResult};
    use crate::connectors::storage::StorageTarget;

    #[test]
    fn snapshot_result_uses_snapshot_path_convention() {
        let result = SnapshotCaptureResult::new(
            "front door/cam",
            SnapshotFormat::Jpeg,
            "ZmFrZS1qcGVn",
            9,
            StorageTarget::LocalDisk,
        );

        assert_eq!(result.mime_type, "image/jpeg");
        assert!(result
            .storage
            .relative_path
            .starts_with("snapshots/front_door_cam/"));
        assert!(result.storage.relative_path.ends_with(".jpg"));
    }

    #[test]
    fn stream_open_request_can_capture_preferred_player() {
        let request = StreamOpenRequest::new(
            "cam-1",
            "rtsp://192.168.1.10/live",
            Some("ffplay".to_string()),
        );

        assert_eq!(request.device_id, "cam-1");
        assert_eq!(request.preferred_player.as_deref(), Some("ffplay"));
    }

    #[test]
    fn stream_open_result_uses_stream_asset_kind() {
        let result = StreamOpenResult::new(
            "cam-1",
            "rtsp://192.168.1.10/live",
            "ffplay",
            PathBuf::from("/usr/bin/ffplay"),
            1234,
        );

        assert_eq!(result.asset_kind, super::MediaAssetKind::Stream);
        assert_eq!(result.player, "ffplay");
        assert_eq!(result.process_id, 1234);
    }
}
