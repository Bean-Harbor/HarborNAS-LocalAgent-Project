//! Snapshot, clip, stream, and timeline processing primitives.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::connectors::storage::{StorageObjectRef, StorageTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactProvenance {
    Media,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactIngestDisposition {
    KnowledgeIndexCandidate,
    RuntimeOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceArtifactMetadata {
    pub device_id: String,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub room: Option<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub discovery_source: Option<String>,
    #[serde(default)]
    pub captured_at_epoch_ms: Option<u128>,
    #[serde(default)]
    pub stream_transport: Option<String>,
    #[serde(default)]
    pub source_requires_auth: Option<bool>,
    pub provenance: ArtifactProvenance,
    pub ingest_disposition: ArtifactIngestDisposition,
}

impl DeviceArtifactMetadata {
    pub fn knowledge_index_candidate(
        device_id: impl Into<String>,
        captured_at_epoch_ms: u128,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            device_name: None,
            room: None,
            vendor: None,
            model: None,
            discovery_source: None,
            captured_at_epoch_ms: Some(captured_at_epoch_ms),
            stream_transport: None,
            source_requires_auth: None,
            provenance: ArtifactProvenance::Media,
            ingest_disposition: ArtifactIngestDisposition::KnowledgeIndexCandidate,
        }
    }

    pub fn runtime_only(device_id: impl Into<String>, captured_at_epoch_ms: u128) -> Self {
        Self {
            device_id: device_id.into(),
            device_name: None,
            room: None,
            vendor: None,
            model: None,
            discovery_source: None,
            captured_at_epoch_ms: Some(captured_at_epoch_ms),
            stream_transport: None,
            source_requires_auth: None,
            provenance: ArtifactProvenance::Control,
            ingest_disposition: ArtifactIngestDisposition::RuntimeOnly,
        }
    }

    pub fn with_device_context(
        mut self,
        device_name: Option<String>,
        room: Option<String>,
        vendor: Option<String>,
        model: Option<String>,
        discovery_source: Option<String>,
        stream_transport: Option<String>,
        source_requires_auth: Option<bool>,
    ) -> Self {
        self.device_name = device_name;
        self.room = room;
        self.vendor = vendor;
        self.model = model;
        self.discovery_source = discovery_source;
        self.stream_transport = stream_transport;
        self.source_requires_auth = source_requires_auth;
        self
    }
}

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
    #[serde(default)]
    pub index_sidecar_relative_path: String,
    #[serde(default)]
    pub ingest_metadata: Option<DeviceArtifactMetadata>,
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
        let ingest_metadata = Some(DeviceArtifactMetadata::knowledge_index_candidate(
            device_id.clone(),
            captured_at_epoch_ms,
        ));
        let relative_path = format!(
            "snapshots/{}/{}.{}",
            sanitize_path_segment(&device_id),
            captured_at_epoch_ms,
            format.file_extension()
        );
        let index_sidecar_relative_path = derive_index_sidecar_relative_path(&relative_path);

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
            index_sidecar_relative_path,
            ingest_metadata,
        }
    }

    pub fn with_device_context(
        mut self,
        device_name: Option<String>,
        room: Option<String>,
        vendor: Option<String>,
        model: Option<String>,
        discovery_source: Option<String>,
        stream_transport: Option<String>,
        source_requires_auth: Option<bool>,
    ) -> Self {
        self.ingest_metadata = self.ingest_metadata.take().map(|metadata| {
            metadata.with_device_context(
                device_name,
                room,
                vendor,
                model,
                discovery_source,
                stream_transport,
                source_requires_auth,
            )
        });
        self
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
    #[serde(default)]
    pub opened_at_epoch_ms: Option<u128>,
    #[serde(default)]
    pub ingest_metadata: Option<DeviceArtifactMetadata>,
}

impl StreamOpenResult {
    pub fn new(
        device_id: impl Into<String>,
        stream_url: impl Into<String>,
        player: impl Into<String>,
        player_path: PathBuf,
        process_id: u32,
    ) -> Self {
        let device_id = device_id.into();
        let opened_at_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        Self {
            device_id: device_id.clone(),
            asset_kind: MediaAssetKind::Stream,
            stream_url: stream_url.into(),
            player: player.into(),
            player_path,
            process_id,
            opened_at_epoch_ms: Some(opened_at_epoch_ms),
            ingest_metadata: Some(DeviceArtifactMetadata::runtime_only(
                device_id,
                opened_at_epoch_ms,
            )),
        }
    }

    pub fn with_device_context(
        mut self,
        device_name: Option<String>,
        room: Option<String>,
        vendor: Option<String>,
        model: Option<String>,
        discovery_source: Option<String>,
        stream_transport: Option<String>,
        source_requires_auth: Option<bool>,
    ) -> Self {
        self.ingest_metadata = self.ingest_metadata.take().map(|metadata| {
            metadata.with_device_context(
                device_name,
                room,
                vendor,
                model,
                discovery_source,
                stream_transport,
                source_requires_auth,
            )
        });
        self
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

fn derive_index_sidecar_relative_path(relative_path: &str) -> String {
    let path = PathBuf::from(relative_path);
    let mut sidecar = path;
    sidecar.set_extension("json");
    sidecar.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        ArtifactIngestDisposition, ArtifactProvenance, SnapshotCaptureResult, SnapshotFormat,
        StreamOpenRequest, StreamOpenResult,
    };
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
        assert!(result.index_sidecar_relative_path.ends_with(".json"));
        assert!(result
            .index_sidecar_relative_path
            .starts_with("snapshots/front_door_cam/"));
        assert_eq!(
            result
                .ingest_metadata
                .as_ref()
                .expect("metadata")
                .provenance,
            ArtifactProvenance::Media
        );
        assert_eq!(
            result
                .ingest_metadata
                .as_ref()
                .expect("metadata")
                .ingest_disposition,
            ArtifactIngestDisposition::KnowledgeIndexCandidate
        );
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
        assert_eq!(
            result
                .ingest_metadata
                .as_ref()
                .expect("metadata")
                .provenance,
            ArtifactProvenance::Control
        );
        assert_eq!(
            result
                .ingest_metadata
                .as_ref()
                .expect("metadata")
                .ingest_disposition,
            ArtifactIngestDisposition::RuntimeOnly
        );
    }

    #[test]
    fn snapshot_metadata_round_trips_with_device_context() {
        let result = SnapshotCaptureResult::new(
            "cam-1",
            SnapshotFormat::Png,
            "ZmFrZQ==",
            4,
            StorageTarget::LocalDisk,
        )
        .with_device_context(
            Some("Front Door".to_string()),
            Some("Entry".to_string()),
            Some("DemoCam".to_string()),
            Some("C1".to_string()),
            Some("manual_entry".to_string()),
            Some("rtsp".to_string()),
            Some(false),
        );

        let encoded = serde_json::to_value(&result).expect("serialize");
        assert_eq!(encoded["ingest_metadata"]["device_id"], json!("cam-1"));
        assert_eq!(
            encoded["ingest_metadata"]["device_name"],
            json!("Front Door")
        );
        assert_eq!(encoded["ingest_metadata"]["provenance"], json!("media"));
        assert_eq!(
            encoded["ingest_metadata"]["ingest_disposition"],
            json!("knowledge_index_candidate")
        );

        let decoded: SnapshotCaptureResult = serde_json::from_value(encoded).expect("decode");
        assert_eq!(
            decoded.ingest_metadata.expect("metadata").room.as_deref(),
            Some("Entry")
        );
    }
}
