//! Local camera DVR settings, segment metadata, and ffmpeg process control.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::runtime::media_tools::{ffmpeg_resolution_hint, resolve_ffmpeg_bin};
use crate::runtime::registry::CameraDevice;

const DEFAULT_HARBOROS_WRITABLE_ROOT: &str = "/mnt/software/harborbeacon-agent-ci";
const HARBOROS_WRITABLE_ROOT_ENV: &str = "HARBOR_HARBOROS_WRITABLE_ROOT";
const DVR_KNOWLEDGE_ROOT_ID: &str = "camera-dvr-recordings";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DvrRecordingSettings {
    #[serde(default = "default_recording_root")]
    pub recording_root: String,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_segment_seconds")]
    pub segment_seconds: u32,
    #[serde(default = "default_true")]
    pub continuous_recording_enabled: bool,
    #[serde(default = "default_true")]
    pub low_bitrate_stream_preferred: bool,
    #[serde(default = "default_continuous_bitrate_mbps")]
    pub continuous_bitrate_mbps: u32,
    #[serde(default = "default_true")]
    pub high_res_event_clips_enabled: bool,
    #[serde(default = "default_high_res_event_clip_seconds")]
    pub high_res_event_clip_seconds: u32,
    #[serde(default = "default_continuous_stream_path_hint")]
    pub continuous_stream_path_hint: String,
    #[serde(default = "default_high_res_stream_path_hint")]
    pub high_res_stream_path_hint: String,
    #[serde(default)]
    pub disk_budget_gb: Option<u64>,
    #[serde(default = "default_keyframe_count")]
    pub keyframe_count: u32,
    #[serde(default = "default_keyframe_interval_seconds")]
    pub keyframe_interval_seconds: u32,
    #[serde(default)]
    pub enabled_device_ids: Vec<String>,
}

impl Default for DvrRecordingSettings {
    fn default() -> Self {
        Self {
            recording_root: default_recording_root(),
            retention_days: default_retention_days(),
            segment_seconds: default_segment_seconds(),
            continuous_recording_enabled: true,
            low_bitrate_stream_preferred: true,
            continuous_bitrate_mbps: default_continuous_bitrate_mbps(),
            high_res_event_clips_enabled: true,
            high_res_event_clip_seconds: default_high_res_event_clip_seconds(),
            continuous_stream_path_hint: default_continuous_stream_path_hint(),
            high_res_stream_path_hint: default_high_res_stream_path_hint(),
            disk_budget_gb: None,
            keyframe_count: default_keyframe_count(),
            keyframe_interval_seconds: default_keyframe_interval_seconds(),
            enabled_device_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DvrCapacityEstimate {
    pub camera_count: usize,
    pub enabled_camera_count: usize,
    pub retention_days: u32,
    pub bitrate_mbps: u32,
    pub estimated_bytes_per_camera: u64,
    pub estimated_bytes_enabled_total: u64,
    pub disk_budget_bytes: Option<u64>,
    pub disk_budget_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DvrRecordingStatus {
    pub device_id: String,
    pub status: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub stream_kind: String,
    #[serde(default)]
    pub last_segment_path: Option<String>,
    #[serde(default)]
    pub live_mjpeg_url: Option<String>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DvrTimelineSegment {
    pub device_id: String,
    pub file_path: String,
    #[serde(default)]
    pub sidecar_path: Option<String>,
    pub stream_kind: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_seconds: u32,
    pub retention_expires_at: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub replay_url: Option<String>,
    #[serde(default)]
    pub indexed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DvrTimelineResponse {
    pub generated_at: String,
    pub recording_root: String,
    pub segments: Vec<DvrTimelineSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DvrRecordingStatusResponse {
    pub generated_at: String,
    pub settings: DvrRecordingSettings,
    pub capacity: DvrCapacityEstimate,
    pub statuses: Vec<DvrRecordingStatus>,
    pub root_exists: bool,
    pub root_writable: bool,
}

#[derive(Debug)]
struct DvrProcess {
    child: Child,
    started_at: String,
    stream_kind: String,
    pattern: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct DvrRuntime {
    processes: Arc<Mutex<HashMap<String, DvrProcess>>>,
}

impl DvrRuntime {
    pub fn start_recording(
        &self,
        device: &CameraDevice,
        settings: &DvrRecordingSettings,
        public_origin: Option<&str>,
    ) -> Result<DvrRecordingStatus, String> {
        let settings = sanitize_dvr_recording_settings(settings.clone());
        if !settings.continuous_recording_enabled {
            return Err("continuous DVR recording is disabled in HarborDesk settings".to_string());
        }
        let ffmpeg_bin = resolve_ffmpeg_bin().ok_or_else(|| {
            format!(
                "ffmpeg is required for local DVR recording but is unavailable; {}",
                ffmpeg_resolution_hint()
            )
        })?;
        let stream_url = recording_stream_url(device, &settings);
        if stream_url.trim().is_empty() {
            return Err(format!(
                "camera {} does not expose a usable RTSP stream URL",
                device.device_id
            ));
        }

        let root = recording_root_path(&settings);
        fs::create_dir_all(
            root.join("recordings")
                .join(device_path_component(&device.device_id)),
        )
        .map_err(|error| format!("failed to create DVR recording directory: {error}"))?;
        prepare_segment_calendar_dirs(&root, &device.device_id, now_unix_secs(), &settings)
            .map_err(|error| {
                format!("failed to create DVR segment calendar directories: {error}")
            })?;
        let pattern = segment_output_pattern(&settings, &device.device_id);
        let mut command = Command::new(&ffmpeg_bin);
        command
            .env("TZ", "UTC")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("warning")
            .arg("-nostdin")
            .arg("-rtsp_transport")
            .arg("tcp")
            .arg("-i")
            .arg(&stream_url)
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("0:a?")
            .arg("-c")
            .arg("copy")
            .arg("-f")
            .arg("segment")
            .arg("-segment_time")
            .arg(settings.segment_seconds.to_string())
            .arg("-reset_timestamps")
            .arg("1")
            .arg("-strftime")
            .arg("1")
            .arg("-strftime_mkdir")
            .arg("1")
            .arg(pattern.to_string_lossy().into_owned())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|error| format!("failed to start DVR ffmpeg process: {error}"))?;
        let started_at = now_unix_secs().to_string();
        let stream_kind = if settings.low_bitrate_stream_preferred {
            "substream"
        } else {
            "mainstream"
        }
        .to_string();

        let mut processes = self
            .processes
            .lock()
            .map_err(|_| "DVR runtime process lock is poisoned".to_string())?;
        if let Some(mut existing) = processes.remove(&device.device_id) {
            let _ = existing.child.kill();
            let _ = existing.child.wait();
        }
        processes.insert(
            device.device_id.clone(),
            DvrProcess {
                child,
                started_at: started_at.clone(),
                stream_kind: stream_kind.clone(),
                pattern,
            },
        );

        Ok(DvrRecordingStatus {
            device_id: device.device_id.clone(),
            status: "recording".to_string(),
            started_at: Some(started_at.clone()),
            updated_at: Some(started_at),
            stream_kind,
            last_segment_path: latest_segment_path(&settings, &device.device_id),
            live_mjpeg_url: public_origin.map(|origin| {
                format!(
                    "{}/api/cameras/{}/live.mjpeg",
                    origin.trim_end_matches('/'),
                    url_encode_path_segment(&device.device_id)
                )
            }),
            message: "continuous low-bitrate DVR recording is running".to_string(),
        })
    }

    pub fn stop_recording(
        &self,
        device_id: &str,
        public_origin: Option<&str>,
    ) -> Result<DvrRecordingStatus, String> {
        let mut processes = self
            .processes
            .lock()
            .map_err(|_| "DVR runtime process lock is poisoned".to_string())?;
        let Some(mut process) = processes.remove(device_id) else {
            return Ok(DvrRecordingStatus {
                device_id: device_id.to_string(),
                status: "stopped".to_string(),
                started_at: None,
                updated_at: Some(now_unix_secs().to_string()),
                stream_kind: "substream".to_string(),
                last_segment_path: None,
                live_mjpeg_url: public_origin.map(|origin| {
                    format!(
                        "{}/api/cameras/{}/live.mjpeg",
                        origin.trim_end_matches('/'),
                        url_encode_path_segment(device_id)
                    )
                }),
                message: "DVR recording was not running".to_string(),
            });
        };
        let _ = process.child.kill();
        let _ = process.child.wait();
        Ok(DvrRecordingStatus {
            device_id: device_id.to_string(),
            status: "stopped".to_string(),
            started_at: Some(process.started_at),
            updated_at: Some(now_unix_secs().to_string()),
            stream_kind: process.stream_kind,
            last_segment_path: None,
            live_mjpeg_url: public_origin.map(|origin| {
                format!(
                    "{}/api/cameras/{}/live.mjpeg",
                    origin.trim_end_matches('/'),
                    url_encode_path_segment(device_id)
                )
            }),
            message: "DVR recording stopped".to_string(),
        })
    }

    pub fn statuses(
        &self,
        devices: &[CameraDevice],
        settings: &DvrRecordingSettings,
        public_origin: Option<&str>,
    ) -> Result<Vec<DvrRecordingStatus>, String> {
        let settings = sanitize_dvr_recording_settings(settings.clone());
        let mut processes = self
            .processes
            .lock()
            .map_err(|_| "DVR runtime process lock is poisoned".to_string())?;
        let mut statuses = Vec::new();
        let mut exited = Vec::new();
        for device in devices {
            let live_mjpeg_url = public_origin.map(|origin| {
                format!(
                    "{}/api/cameras/{}/live.mjpeg",
                    origin.trim_end_matches('/'),
                    url_encode_path_segment(&device.device_id)
                )
            });
            if let Some(process) = processes.get_mut(&device.device_id) {
                match process.child.try_wait() {
                    Ok(Some(status)) => {
                        let code = status
                            .code()
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "signal".to_string());
                        statuses.push(DvrRecordingStatus {
                            device_id: device.device_id.clone(),
                            status: "failed".to_string(),
                            started_at: Some(process.started_at.clone()),
                            updated_at: Some(now_unix_secs().to_string()),
                            stream_kind: process.stream_kind.clone(),
                            last_segment_path: latest_segment_path(&settings, &device.device_id),
                            live_mjpeg_url,
                            message: format!("DVR ffmpeg process exited with status {code}"),
                        });
                        exited.push(device.device_id.clone());
                    }
                    Ok(None) => statuses.push(DvrRecordingStatus {
                        device_id: device.device_id.clone(),
                        status: "recording".to_string(),
                        started_at: Some(process.started_at.clone()),
                        updated_at: Some(now_unix_secs().to_string()),
                        stream_kind: process.stream_kind.clone(),
                        last_segment_path: latest_segment_path(&settings, &device.device_id),
                        live_mjpeg_url,
                        message: format!(
                            "DVR segmenter writing {}",
                            process.pattern.to_string_lossy()
                        ),
                    }),
                    Err(error) => {
                        statuses.push(DvrRecordingStatus {
                            device_id: device.device_id.clone(),
                            status: "failed".to_string(),
                            started_at: Some(process.started_at.clone()),
                            updated_at: Some(now_unix_secs().to_string()),
                            stream_kind: process.stream_kind.clone(),
                            last_segment_path: latest_segment_path(&settings, &device.device_id),
                            live_mjpeg_url,
                            message: format!("failed to inspect DVR process: {error}"),
                        });
                        exited.push(device.device_id.clone());
                    }
                }
            } else {
                statuses.push(DvrRecordingStatus {
                    device_id: device.device_id.clone(),
                    status: "stopped".to_string(),
                    started_at: None,
                    updated_at: Some(now_unix_secs().to_string()),
                    stream_kind: if settings.low_bitrate_stream_preferred {
                        "substream".to_string()
                    } else {
                        "mainstream".to_string()
                    },
                    last_segment_path: latest_segment_path(&settings, &device.device_id),
                    live_mjpeg_url,
                    message: "DVR recording is not running".to_string(),
                });
            }
        }
        for device_id in exited {
            processes.remove(&device_id);
        }
        Ok(statuses)
    }
}

pub fn sanitize_dvr_recording_settings(mut settings: DvrRecordingSettings) -> DvrRecordingSettings {
    settings.recording_root = settings.recording_root.trim().to_string();
    if settings.recording_root.is_empty() {
        settings.recording_root = default_recording_root();
    }
    settings.retention_days = settings.retention_days.clamp(1, 365);
    settings.segment_seconds = settings.segment_seconds.clamp(30, 3600);
    settings.continuous_bitrate_mbps = settings.continuous_bitrate_mbps.clamp(1, 20);
    settings.high_res_event_clip_seconds = settings.high_res_event_clip_seconds.clamp(3, 600);
    settings.continuous_stream_path_hint =
        normalize_rtsp_path_hint(&settings.continuous_stream_path_hint, "/stream2");
    settings.high_res_stream_path_hint =
        normalize_rtsp_path_hint(&settings.high_res_stream_path_hint, "/stream1");
    settings.disk_budget_gb = settings.disk_budget_gb.filter(|value| *value > 0);
    settings.keyframe_count = settings.keyframe_count.clamp(1, 12);
    settings.keyframe_interval_seconds = settings.keyframe_interval_seconds.clamp(1, 3600);
    settings.enabled_device_ids = dedupe_non_empty(settings.enabled_device_ids);
    settings
}

pub fn default_recording_root() -> String {
    Path::new(
        &std::env::var(HARBOROS_WRITABLE_ROOT_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_HARBOROS_WRITABLE_ROOT.to_string()),
    )
    .join("camera-dvr")
    .to_string_lossy()
    .into_owned()
}

pub fn dvr_knowledge_root_id() -> &'static str {
    DVR_KNOWLEDGE_ROOT_ID
}

pub fn dvr_capacity_estimate(
    settings: &DvrRecordingSettings,
    camera_count: usize,
) -> DvrCapacityEstimate {
    let settings = sanitize_dvr_recording_settings(settings.clone());
    let enabled_camera_count = settings.enabled_device_ids.len();
    let seconds = settings.retention_days as u64 * 24 * 60 * 60;
    let estimated_bytes_per_camera =
        settings.continuous_bitrate_mbps as u64 * 1_000_000 * seconds / 8;
    let estimated_bytes_enabled_total =
        estimated_bytes_per_camera.saturating_mul(enabled_camera_count as u64);
    let disk_budget_bytes = settings
        .disk_budget_gb
        .map(|gb| gb.saturating_mul(1_000_000_000));
    let disk_budget_warning = disk_budget_bytes.and_then(|budget| {
        (enabled_camera_count > 0 && estimated_bytes_enabled_total > budget).then(|| {
            format!(
                "Estimated DVR usage {} GB exceeds configured disk budget {} GB.",
                bytes_to_decimal_gb(estimated_bytes_enabled_total),
                bytes_to_decimal_gb(budget)
            )
        })
    });
    DvrCapacityEstimate {
        camera_count,
        enabled_camera_count,
        retention_days: settings.retention_days,
        bitrate_mbps: settings.continuous_bitrate_mbps,
        estimated_bytes_per_camera,
        estimated_bytes_enabled_total,
        disk_budget_bytes,
        disk_budget_warning,
    }
}

pub fn recording_segment_path(recording_root: &Path, device_id: &str, unix_secs: u64) -> PathBuf {
    let (year, month, day, hour, minute, second) = unix_to_utc_parts(unix_secs);
    recording_root
        .join("recordings")
        .join(device_path_component(device_id))
        .join(format!("{year:04}"))
        .join(format!("{month:02}"))
        .join(format!("{day:02}"))
        .join(format!("{hour:02}{minute:02}{second:02}.mp4"))
}

pub fn scan_timeline(
    settings: &DvrRecordingSettings,
    devices: &[CameraDevice],
    device_filter: Option<&str>,
    from_secs: Option<u64>,
    to_secs: Option<u64>,
    public_origin: Option<&str>,
) -> Result<DvrTimelineResponse, String> {
    let settings = sanitize_dvr_recording_settings(settings.clone());
    let root = recording_root_path(&settings);
    let mut segments = Vec::new();
    let device_lookup = devices
        .iter()
        .map(|device| (device.device_id.as_str(), device))
        .collect::<HashMap<_, _>>();
    let recording_root = root.join("recordings");
    if recording_root.exists() {
        collect_segments(
            &recording_root,
            &settings,
            &device_lookup,
            device_filter,
            from_secs,
            to_secs,
            public_origin,
            &mut segments,
        )?;
    }
    segments.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.file_path.cmp(&left.file_path))
    });
    Ok(DvrTimelineResponse {
        generated_at: now_unix_secs().to_string(),
        recording_root: settings.recording_root,
        segments,
    })
}

pub fn apply_retention_policy(settings: &DvrRecordingSettings) -> Result<usize, String> {
    let settings = sanitize_dvr_recording_settings(settings.clone());
    let cutoff = now_unix_secs().saturating_sub(settings.retention_days as u64 * 24 * 60 * 60);
    let recording_root = recording_root_path(&settings).join("recordings");
    if !recording_root.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    let mut files = Vec::new();
    collect_mp4_paths(&recording_root, &mut files)?;
    for file in files {
        let timestamp = segment_timestamp_from_path(&file).or_else(|| file_modified_secs(&file));
        if timestamp.is_some_and(|value| value < cutoff) {
            remove_file_if_exists(&file, &mut removed)?;
            for sidecar in sidecar_candidates(&file) {
                remove_file_if_exists(&sidecar, &mut removed)?;
            }
            if let Some(stem) = file.file_stem().and_then(|value| value.to_str()) {
                remove_dir_if_exists(&file.with_file_name(format!("{stem}.frames")), &mut removed)?;
            }
        }
    }
    Ok(removed)
}

pub fn build_status_response(
    settings: DvrRecordingSettings,
    statuses: Vec<DvrRecordingStatus>,
    camera_count: usize,
) -> DvrRecordingStatusResponse {
    let settings = sanitize_dvr_recording_settings(settings);
    let root = recording_root_path(&settings);
    DvrRecordingStatusResponse {
        generated_at: now_unix_secs().to_string(),
        capacity: dvr_capacity_estimate(&settings, camera_count),
        root_exists: root.exists(),
        root_writable: path_can_accept_write(&root),
        settings,
        statuses,
    }
}

pub fn recording_stream_url(device: &CameraDevice, settings: &DvrRecordingSettings) -> String {
    if !settings.low_bitrate_stream_preferred {
        return device.primary_stream.url.clone();
    }
    replace_rtsp_path(
        &device.primary_stream.url,
        &settings.continuous_stream_path_hint,
    )
    .unwrap_or_else(|| device.primary_stream.url.clone())
}

fn collect_segments(
    directory: &Path,
    settings: &DvrRecordingSettings,
    device_lookup: &HashMap<&str, &CameraDevice>,
    device_filter: Option<&str>,
    from_secs: Option<u64>,
    to_secs: Option<u64>,
    public_origin: Option<&str>,
    segments: &mut Vec<DvrTimelineSegment>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "failed to read DVR directory {}: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to read DVR directory entry: {error}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_segments(
                &path,
                settings,
                device_lookup,
                device_filter,
                from_secs,
                to_secs,
                public_origin,
                segments,
            )?;
            continue;
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("mp4"))
        {
            continue;
        }
        let device_id = device_id_from_segment_path(settings, &path).unwrap_or_default();
        if device_id.is_empty() {
            continue;
        }
        if device_filter.is_some_and(|filter| filter != device_id) {
            continue;
        }
        let started = segment_timestamp_from_path(&path).or_else(|| file_modified_secs(&path));
        let Some(started) = started else {
            continue;
        };
        let ended = started.saturating_add(settings.segment_seconds as u64);
        if from_secs.is_some_and(|from| ended < from) || to_secs.is_some_and(|to| started > to) {
            continue;
        }
        let size_bytes = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let retention_expires_at =
            started.saturating_add(settings.retention_days as u64 * 24 * 60 * 60);
        let sidecar_path = path.with_extension("json");
        let sidecar_exists = sidecar_path.exists();
        let sidecar_path_text = sidecar_path.to_string_lossy().into_owned();
        let mut segment = DvrTimelineSegment {
            device_id: device_id.clone(),
            file_path: path.to_string_lossy().into_owned(),
            sidecar_path: Some(sidecar_path_text),
            stream_kind: if settings.low_bitrate_stream_preferred {
                "substream".to_string()
            } else {
                "mainstream".to_string()
            },
            started_at: started.to_string(),
            ended_at: ended.to_string(),
            duration_seconds: settings.segment_seconds,
            retention_expires_at: retention_expires_at.to_string(),
            size_bytes,
            replay_url: public_origin.map(|origin| {
                format!(
                    "{}/api/knowledge/preview?path={}",
                    origin.trim_end_matches('/'),
                    url_encode_query_component(&path.to_string_lossy())
                )
            }),
            indexed: sidecar_exists,
        };
        if let Some(device) = device_lookup.get(device_id.as_str()) {
            write_segment_sidecar(&segment, device, settings)?;
            segment.indexed = true;
        }
        segments.push(segment);
    }
    Ok(())
}

fn write_segment_sidecar(
    segment: &DvrTimelineSegment,
    device: &CameraDevice,
    settings: &DvrRecordingSettings,
) -> Result<(), String> {
    let sidecar_path = Path::new(&segment.file_path).with_extension("json");
    let payload = json!({
        "media_asset": {
            "kind": "recording",
            "source": "camera_dvr",
            "device_id": segment.device_id,
            "device_name": device.name,
            "room": device.room,
            "vendor": device.vendor,
            "model": device.model,
            "stream_kind": segment.stream_kind,
            "started_at": segment.started_at,
            "ended_at": segment.ended_at,
            "duration_seconds": segment.duration_seconds,
            "retention_expires_at": segment.retention_expires_at,
            "source_video_path": segment.file_path,
            "labels": ["video", "recording", "dvr", "analysis_pending"],
            "analysis_pipeline": "multimodal_rag_vlm",
            "model_boundary": "reuse_model_center_vlm_and_existing_knowledge_index",
            "keyframe_count": settings.keyframe_count,
            "keyframe_interval_seconds": settings.keyframe_interval_seconds
        },
        "caption": format!(
            "Camera DVR recording from {} between {} and {}. Video sidecar is ready for existing multimodal RAG/VLM indexing.",
            segment.device_id, segment.started_at, segment.ended_at
        ),
        "derived_text": "local continuous DVR segment; analyze via existing HarborBeacon knowledge index video keyframe VLM path",
        "source_video_path": segment.file_path,
        "camera": {
            "device_id": segment.device_id,
            "name": device.name,
            "room": device.room,
            "ip_address": device.ip_address
        },
        "labels": ["video", "recording", "dvr", "analysis_pending"]
    });
    fs::write(
        &sidecar_path,
        serde_json::to_vec_pretty(&payload)
            .map_err(|error| format!("failed to serialize DVR sidecar: {error}"))?,
    )
    .map_err(|error| {
        format!(
            "failed to write DVR sidecar {}: {error}",
            sidecar_path.display()
        )
    })
}

fn collect_mp4_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| {
        format!(
            "failed to read DVR directory {}: {error}",
            directory.display()
        )
    })? {
        let entry =
            entry.map_err(|error| format!("failed to read DVR directory entry: {error}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_mp4_paths(&path, paths)?;
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn latest_segment_path(settings: &DvrRecordingSettings, device_id: &str) -> Option<String> {
    let root = recording_root_path(settings)
        .join("recordings")
        .join(device_path_component(device_id));
    if !root.exists() {
        return None;
    }
    let mut paths = Vec::new();
    collect_mp4_paths(&root, &mut paths).ok()?;
    paths
        .sort_by_key(|path| segment_timestamp_from_path(path).or_else(|| file_modified_secs(path)));
    paths.last().map(|path| path.to_string_lossy().into_owned())
}

fn segment_output_pattern(settings: &DvrRecordingSettings, device_id: &str) -> PathBuf {
    recording_root_path(settings)
        .join("recordings")
        .join(device_path_component(device_id))
        .join("%Y")
        .join("%m")
        .join("%d")
        .join("%H%M%S.mp4")
}

fn recording_root_path(settings: &DvrRecordingSettings) -> PathBuf {
    PathBuf::from(settings.recording_root.trim())
}

fn prepare_segment_calendar_dirs(
    recording_root: &Path,
    device_id: &str,
    start_secs: u64,
    settings: &DvrRecordingSettings,
) -> std::io::Result<()> {
    let days_to_prepare = settings.retention_days.min(365).saturating_add(1);
    for day_offset in 0..=days_to_prepare {
        let segment_path = recording_segment_path(
            recording_root,
            device_id,
            start_secs.saturating_add(day_offset as u64 * 86_400),
        );
        if let Some(parent) = segment_path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn device_id_from_segment_path(settings: &DvrRecordingSettings, path: &Path) -> Option<String> {
    let root = recording_root_path(settings).join("recordings");
    let relative = path.strip_prefix(root).ok()?;
    relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .map(|value| value.to_string())
}

fn segment_timestamp_from_path(path: &Path) -> Option<u64> {
    let file_stem = path.file_stem()?.to_str()?;
    if file_stem.len() != 6 || !file_stem.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let second = file_stem[4..6].parse::<u32>().ok()?;
    let minute = file_stem[2..4].parse::<u32>().ok()?;
    let hour = file_stem[0..2].parse::<u32>().ok()?;
    let day = path.parent()?.file_name()?.to_str()?.parse::<u32>().ok()?;
    let month = path
        .parent()?
        .parent()?
        .file_name()?
        .to_str()?
        .parse::<u32>()
        .ok()?;
    let year = path
        .parent()?
        .parent()?
        .parent()?
        .file_name()?
        .to_str()?
        .parse::<i32>()
        .ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + hour as u64 * 3_600 + minute as u64 * 60 + second as u64)
}

fn file_modified_secs(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn sidecar_candidates(video_path: &Path) -> Vec<PathBuf> {
    ["json", "txt", "md", "csv", "yaml", "yml"]
        .iter()
        .map(|extension| video_path.with_extension(extension))
        .collect()
}

fn remove_file_if_exists(path: &Path, removed: &mut usize) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("failed to remove DVR file {}: {error}", path.display()))?;
        *removed += 1;
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path, removed: &mut usize) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|error| {
            format!(
                "failed to remove DVR derived directory {}: {error}",
                path.display()
            )
        })?;
        *removed += 1;
    }
    Ok(())
}

fn path_can_accept_write(path: &Path) -> bool {
    if fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(".harborbeacon-dvr-write-probe");
    match fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

fn replace_rtsp_path(stream_url: &str, path_hint: &str) -> Option<String> {
    let mut url = Url::parse(stream_url).ok()?;
    if !matches!(url.scheme(), "rtsp" | "rtsps") {
        return None;
    }
    url.set_path(path_hint);
    Some(url.to_string())
}

fn normalize_rtsp_path_hint(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn dedupe_non_empty(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() && seen.insert(trimmed.clone()) {
            output.push(trimmed);
        }
    }
    output
}

fn device_path_component(device_id: &str) -> String {
    let mut output = String::new();
    for ch in device_id.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "camera".to_string()
    } else {
        output
    }
}

fn bytes_to_decimal_gb(bytes: u64) -> u64 {
    (bytes + 999_999_999) / 1_000_000_000
}

fn url_encode_path_segment(value: &str) -> String {
    url_encode_query_component(value)
}

fn url_encode_query_component(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(*byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_to_utc_parts(unix_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (unix_secs / 86_400) as i64;
    let day_secs = unix_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = (day_secs / 3_600) as u32;
    let minute = ((day_secs % 3_600) / 60) as u32;
    let second = (day_secs % 60) as u32;
    (year, month, day, hour, minute, second)
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let mut y = year as i64;
    let m = month as i64;
    y -= i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn default_true() -> bool {
    true
}

fn default_retention_days() -> u32 {
    7
}

fn default_segment_seconds() -> u32 {
    300
}

fn default_continuous_bitrate_mbps() -> u32 {
    2
}

fn default_high_res_event_clip_seconds() -> u32 {
    30
}

fn default_continuous_stream_path_hint() -> String {
    "/stream2".to_string()
}

fn default_high_res_stream_path_hint() -> String {
    "/stream1".to_string()
}

fn default_keyframe_count() -> u32 {
    5
}

fn default_keyframe_interval_seconds() -> u32 {
    60
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{label}-{}-{}",
            std::process::id(),
            now_unix_secs()
        ))
    }

    #[test]
    fn recording_path_uses_device_and_utc_calendar_segments() {
        let root = PathBuf::from("/tmp/harbor-dvr");
        let path = recording_segment_path(&root, "camera/main", 1_704_067_205);
        assert!(path.ends_with(
            Path::new("recordings")
                .join("camera_main")
                .join("2024")
                .join("01")
                .join("01")
                .join("000005.mp4")
        ));
        assert_eq!(segment_timestamp_from_path(&path), Some(1_704_067_205));
    }

    #[test]
    fn capacity_estimate_matches_low_bitrate_weekly_budget() {
        let settings = DvrRecordingSettings {
            continuous_bitrate_mbps: 2,
            enabled_device_ids: vec!["camera-main".to_string()],
            ..Default::default()
        };
        let estimate = dvr_capacity_estimate(&settings, 1);
        assert_eq!(estimate.retention_days, 7);
        assert_eq!(
            bytes_to_decimal_gb(estimate.estimated_bytes_per_camera),
            152
        );
    }

    #[test]
    fn prepare_segment_calendar_dirs_creates_retention_window_parents() {
        let root = unique_dir("harborbeacon-dvr-calendar-dirs");
        let settings = DvrRecordingSettings {
            recording_root: root.to_string_lossy().into_owned(),
            retention_days: 2,
            ..Default::default()
        };
        let start = 1_704_067_205;

        prepare_segment_calendar_dirs(&root, "camera-main", start, &settings)
            .expect("prepare dirs");

        for offset in 0..=3 {
            let path = recording_segment_path(&root, "camera-main", start + offset * 86_400);
            assert!(path.parent().expect("parent").is_dir());
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sanitize_settings_clamps_retention_segments_and_dedupes_devices() {
        let settings = sanitize_dvr_recording_settings(DvrRecordingSettings {
            recording_root: " ".to_string(),
            retention_days: 0,
            segment_seconds: 12,
            continuous_bitrate_mbps: 0,
            continuous_stream_path_hint: "stream-low".to_string(),
            enabled_device_ids: vec![
                " camera-main ".to_string(),
                "camera-main".to_string(),
                " ".to_string(),
            ],
            ..Default::default()
        });
        assert!(!settings.recording_root.trim().is_empty());
        assert_eq!(settings.retention_days, 1);
        assert_eq!(settings.segment_seconds, 30);
        assert_eq!(settings.continuous_bitrate_mbps, 1);
        assert_eq!(settings.continuous_stream_path_hint, "/stream-low");
        assert_eq!(settings.enabled_device_ids, vec!["camera-main"]);
    }

    #[test]
    fn timeline_scan_writes_video_sidecar_for_rag() {
        let root = unique_dir("harborbeacon-dvr-timeline");
        let settings = DvrRecordingSettings {
            recording_root: root.to_string_lossy().into_owned(),
            segment_seconds: 300,
            ..Default::default()
        };
        let video_path = recording_segment_path(&root, "camera-main", 1_704_067_205);
        fs::create_dir_all(video_path.parent().expect("parent")).expect("create dir");
        fs::write(&video_path, b"fake-mp4").expect("write video");
        let mut device = CameraDevice::new("camera-main", "Front Door", "rtsp://host/stream1");
        device.room = Some("Door".to_string());

        let response = scan_timeline(&settings, &[device], None, None, None, None).expect("scan");

        assert_eq!(response.segments.len(), 1);
        assert_eq!(response.segments[0].device_id, "camera-main");
        let sidecar = video_path.with_extension("json");
        let text = fs::read_to_string(sidecar).expect("sidecar");
        assert!(text.contains("multimodal_rag_vlm"));
        assert!(text.contains("analysis_pending"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recording_stream_url_prefers_substream_path_without_losing_auth() {
        let device = CameraDevice::new(
            "camera-main",
            "Main",
            "rtsp://user:pass@192.168.3.231:554/stream1",
        );
        let settings = DvrRecordingSettings::default();
        assert_eq!(
            recording_stream_url(&device, &settings),
            "rtsp://user:pass@192.168.3.231:554/stream2"
        );
    }
}
