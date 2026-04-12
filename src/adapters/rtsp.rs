//! RTSP media stream adapter boundary.

use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use base64::Engine as _;
use serde::Deserialize;

use crate::runtime::discovery::{RtspProbeRequest, RtspProbeResult};
use crate::runtime::media::{
    SnapshotCaptureRequest, SnapshotCaptureResult, SnapshotFormat, StreamOpenRequest,
    StreamOpenResult,
};
use crate::runtime::registry::{CameraCapabilities, StreamTransport};

pub const ADAPTER_NAME: &str = "rtsp";

pub trait RtspProbeAdapter: Send + Sync {
    fn probe(&self, request: &RtspProbeRequest) -> Result<RtspProbeResult, String>;
    fn capture_snapshot(
        &self,
        request: &SnapshotCaptureRequest,
    ) -> Result<SnapshotCaptureResult, String>;
    fn open_stream(&self, request: &StreamOpenRequest) -> Result<StreamOpenResult, String>;
}

pub struct CommandRtspAdapter {
    ffmpeg_bin: String,
    ffprobe_bin: String,
}

impl CommandRtspAdapter {
    pub fn new(ffmpeg_bin: impl Into<String>) -> Self {
        Self {
            ffmpeg_bin: ffmpeg_bin.into(),
            ffprobe_bin: "ffprobe".to_string(),
        }
    }

    fn build_stream_url(
        ip_address: &str,
        port: u16,
        path: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> String {
        let auth = match (username, password) {
            (Some(user), Some(pass)) => {
                format!(
                    "{}:{}@",
                    escape_rtsp_userinfo(user),
                    escape_rtsp_userinfo(pass)
                )
            }
            (Some(user), None) => format!("{}@", escape_rtsp_userinfo(user)),
            _ => String::new(),
        };
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("rtsp://{auth}{ip_address}:{port}{normalized_path}")
    }

    fn ffmpeg_available(&self) -> bool {
        which::which(&self.ffmpeg_bin).is_ok()
    }

    fn ffprobe_available(&self) -> bool {
        which::which(&self.ffprobe_bin).is_ok()
    }

    fn ffmpeg_missing_error(&self) -> String {
        format!(
            "ffmpeg is required for RTSP snapshot capture but was not found in PATH as '{}'",
            self.ffmpeg_bin
        )
    }

    fn ffprobe_missing_error(&self) -> String {
        format!(
            "ffprobe is required for RTSP probing but was not found in PATH as '{}'",
            self.ffprobe_bin
        )
    }

    fn linux_player_candidates() -> &'static [&'static str] {
        &["ffplay", "mpv", "vlc", "gst-launch-1.0", "xdg-open"]
    }

    fn macos_player_candidates() -> &'static [&'static str] {
        &["vlc", "iina", "open"]
    }

    fn resolve_player(preferred_player: Option<&str>) -> Result<(String, PathBuf), String> {
        if let Some(player) = preferred_player {
            return Self::resolve_platform_player(player);
        }

        if cfg!(target_os = "linux") {
            for candidate in Self::linux_player_candidates() {
                if let Ok(resolved) = Self::resolve_platform_player(candidate) {
                    return Ok(resolved);
                }
            }

            return Err(format!(
                "no supported RTSP player found in PATH; tried: {}",
                Self::linux_player_candidates().join(", ")
            ));
        }

        if cfg!(target_os = "macos") {
            for candidate in Self::macos_player_candidates() {
                if let Ok(resolved) = Self::resolve_platform_player(candidate) {
                    return Ok(resolved);
                }
            }

            return Err(format!(
                "no supported RTSP player found on macOS; tried: {}",
                Self::macos_player_candidates().join(", ")
            ));
        }

        Err("RTSP stream open is currently implemented for Linux and macOS only".to_string())
    }

    fn resolve_platform_player(player: &str) -> Result<(String, PathBuf), String> {
        if cfg!(target_os = "linux") {
            let path = which::which(player)
                .map_err(|_| format!("preferred RTSP player '{player}' was not found in PATH"))?;
            return Ok((player.to_string(), path));
        }

        if cfg!(target_os = "macos") {
            return match player {
                "vlc" => {
                    if PathBuf::from("/Applications/VLC.app").exists() {
                        Ok(("vlc".to_string(), PathBuf::from("/usr/bin/open")))
                    } else {
                        Err(
                            "preferred RTSP player 'vlc' was not found at /Applications/VLC.app"
                                .to_string(),
                        )
                    }
                }
                "iina" => {
                    if PathBuf::from("/Applications/IINA.app").exists() {
                        Ok(("iina".to_string(), PathBuf::from("/usr/bin/open")))
                    } else {
                        Err(
                            "preferred RTSP player 'iina' was not found at /Applications/IINA.app"
                                .to_string(),
                        )
                    }
                }
                "open" => Ok(("open".to_string(), PathBuf::from("/usr/bin/open"))),
                other => Err(format!("unsupported RTSP player on macOS: {other}")),
            };
        }

        Err(format!(
            "unsupported RTSP player on this platform: {player}"
        ))
    }

    fn player_args(player: &str, stream_url: &str) -> Result<Vec<String>, String> {
        if cfg!(target_os = "macos") {
            match player {
                "vlc" => {
                    return Ok(vec![
                        "-a".to_string(),
                        "VLC".to_string(),
                        stream_url.to_string(),
                    ]);
                }
                "iina" => {
                    return Ok(vec![
                        "-a".to_string(),
                        "IINA".to_string(),
                        stream_url.to_string(),
                    ]);
                }
                "open" => return Ok(vec![stream_url.to_string()]),
                _ => {}
            }
        }

        match player {
            "ffplay" => Ok(vec![
                "-rtsp_transport".to_string(),
                "tcp".to_string(),
                stream_url.to_string(),
            ]),
            "mpv" => Ok(vec![
                "--profile=low-latency".to_string(),
                stream_url.to_string(),
            ]),
            "vlc" => Ok(vec![
                "--network-caching=150".to_string(),
                stream_url.to_string(),
            ]),
            "gst-launch-1.0" => Ok(vec![
                "rtspsrc".to_string(),
                format!("location={stream_url}"),
                "latency=200".to_string(),
                "protocols=tcp".to_string(),
                "!".to_string(),
                "decodebin".to_string(),
                "!".to_string(),
                "autovideosink".to_string(),
            ]),
            "xdg-open" => Ok(vec![stream_url.to_string()]),
            other => Err(format!("unsupported RTSP player: {other}")),
        }
    }

    fn probe_stream_url(&self, stream_url: &str) -> Result<ProbeOutcome, String> {
        if !self.ffprobe_available() {
            return Err(self.ffprobe_missing_error());
        }

        let output = Command::new(&self.ffprobe_bin)
            .args([
                "-v",
                "error",
                "-rtsp_transport",
                "tcp",
                "-rw_timeout",
                "5000000",
                "-show_entries",
                "stream=codec_name,codec_type",
                "-of",
                "json",
                stream_url,
            ])
            .output()
            .map_err(|e| format!("failed to launch ffprobe for RTSP probe: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "ffprobe exited without stderr output".to_string()
            } else {
                stderr
            };
            return Err(detail);
        }

        let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("invalid ffprobe output: {e}"))?;
        let has_video = parsed
            .streams
            .iter()
            .any(|stream| stream.codec_type.as_deref() == Some("video"));
        let has_audio = parsed
            .streams
            .iter()
            .any(|stream| stream.codec_type.as_deref() == Some("audio"));

        Ok(ProbeOutcome {
            has_video,
            has_audio,
        })
    }
}

impl Default for CommandRtspAdapter {
    fn default() -> Self {
        Self::new("ffmpeg")
    }
}

impl RtspProbeAdapter for CommandRtspAdapter {
    fn probe(&self, request: &RtspProbeRequest) -> Result<RtspProbeResult, String> {
        let path_candidates = if request.path_candidates.is_empty() {
            vec!["/".to_string()]
        } else {
            request.path_candidates.clone()
        };
        let requires_auth = request.username.is_some() || request.password.is_some();
        let mut errors = Vec::new();

        for path in path_candidates {
            let stream_url = Self::build_stream_url(
                &request.ip_address,
                request.port,
                &path,
                request.username.as_deref(),
                request.password.as_deref(),
            );

            match self.probe_stream_url(&stream_url) {
                Ok(outcome) if outcome.has_video => {
                    return Ok(RtspProbeResult {
                        candidate_id: request.candidate_id.clone(),
                        reachable: true,
                        stream_url: Some(stream_url),
                        transport: StreamTransport::Rtsp,
                        requires_auth,
                        capabilities: CameraCapabilities {
                            snapshot: self.ffmpeg_available(),
                            stream: true,
                            ptz: false,
                            audio: outcome.has_audio,
                        },
                        error_message: None,
                    });
                }
                Ok(_) => {
                    errors.push(format!("{path}: no video stream returned"));
                }
                Err(error) => {
                    errors.push(format!("{path}: {error}"));
                }
            }
        }

        Ok(RtspProbeResult {
            candidate_id: request.candidate_id.clone(),
            reachable: false,
            stream_url: None,
            transport: StreamTransport::Rtsp,
            requires_auth,
            capabilities: CameraCapabilities {
                snapshot: self.ffmpeg_available(),
                stream: false,
                ptz: false,
                audio: false,
            },
            error_message: Some(errors.join(" | ")),
        })
    }

    fn capture_snapshot(
        &self,
        request: &SnapshotCaptureRequest,
    ) -> Result<SnapshotCaptureResult, String> {
        if !self.ffmpeg_available() {
            return Err(self.ffmpeg_missing_error());
        }

        let codec = match request.format {
            SnapshotFormat::Jpeg => "mjpeg",
            SnapshotFormat::Png => "png",
        };

        let output = Command::new(&self.ffmpeg_bin)
            .args([
                "-rtsp_transport",
                "tcp",
                "-i",
                &request.stream_url,
                "-frames:v",
                "1",
                "-f",
                "image2pipe",
                "-vcodec",
                codec,
                "-",
            ])
            .output()
            .map_err(|e| format!("failed to launch ffmpeg for snapshot capture: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "ffmpeg exited without stderr output".to_string()
            } else {
                stderr
            };
            return Err(format!("ffmpeg snapshot capture failed: {detail}"));
        }

        let bytes = output.stdout;
        if bytes.is_empty() {
            return Err("ffmpeg snapshot capture returned empty output".to_string());
        }

        Ok(SnapshotCaptureResult::new(
            request.device_id.clone(),
            request.format,
            base64::engine::general_purpose::STANDARD.encode(&bytes),
            bytes.len(),
            request.storage_target,
        ))
    }

    fn open_stream(&self, request: &StreamOpenRequest) -> Result<StreamOpenResult, String> {
        let (player, player_path) = Self::resolve_player(request.preferred_player.as_deref())?;
        let args = Self::player_args(&player, &request.stream_url)?;

        let child = Command::new(&player_path)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to launch RTSP player '{player}': {e}"))?;

        Ok(StreamOpenResult::new(
            request.device_id.clone(),
            request.stream_url.clone(),
            player,
            player_path,
            child.id(),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    #[serde(default)]
    codec_type: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ProbeOutcome {
    has_video: bool,
    has_audio: bool,
}

fn escape_rtsp_userinfo(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            escaped.push(ch);
        } else {
            escaped.push_str(&format!("%{:02X}", ch as u32));
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{CommandRtspAdapter, RtspProbeAdapter};
    use crate::connectors::storage::StorageTarget;
    use crate::runtime::discovery::RtspProbeRequest;
    use crate::runtime::media::{SnapshotCaptureRequest, SnapshotFormat};

    #[test]
    fn probe_builds_rtsp_url_from_candidate_path() {
        assert_eq!(
            CommandRtspAdapter::build_stream_url(
                "192.168.1.30",
                8554,
                "live/main",
                Some("admin"),
                Some("pass")
            ),
            "rtsp://admin:pass@192.168.1.30:8554/live/main"
        );
    }

    #[test]
    fn snapshot_returns_clear_error_when_ffmpeg_missing() {
        let adapter = CommandRtspAdapter::new("ffmpeg-does-not-exist");
        let error = adapter
            .capture_snapshot(&SnapshotCaptureRequest::new(
                "cam-1",
                "rtsp://192.168.1.30/live",
                SnapshotFormat::Jpeg,
                StorageTarget::LocalDisk,
            ))
            .expect_err("missing ffmpeg should fail");

        assert!(error.contains("ffmpeg is required"));
    }

    #[test]
    fn ffplay_arguments_use_tcp_transport() {
        let args = CommandRtspAdapter::player_args("ffplay", "rtsp://192.168.1.30/live")
            .expect("ffplay args");
        assert_eq!(
            args,
            vec![
                "-rtsp_transport".to_string(),
                "tcp".to_string(),
                "rtsp://192.168.1.30/live".to_string()
            ]
        );
    }

    #[test]
    fn unsupported_player_returns_error() {
        let error = CommandRtspAdapter::player_args("made-up-player", "rtsp://cam/live")
            .expect_err("unsupported player should fail");
        assert!(error.contains("unsupported RTSP player"));
    }

    #[test]
    fn macos_open_arguments_are_url_only() {
        let args = CommandRtspAdapter::player_args("open", "rtsp://192.168.1.30/live")
            .expect("macOS open args");
        assert_eq!(args, vec!["rtsp://192.168.1.30/live".to_string()]);
    }

    #[test]
    fn probe_returns_unreachable_when_ffprobe_cannot_validate_stream() {
        let adapter = CommandRtspAdapter::new("ffmpeg");
        let result = adapter
            .probe(&RtspProbeRequest {
                candidate_id: "cand-1".to_string(),
                ip_address: "192.0.2.10".to_string(),
                port: 554,
                username: Some("admin".to_string()),
                password: Some("secret".to_string()),
                path_candidates: vec!["/missing".to_string()],
            })
            .expect("probe should return result");

        assert!(!result.reachable);
        assert!(result.stream_url.is_none());
        assert!(result.requires_auth);
        assert!(result.error_message.is_some());
    }

    #[test]
    fn escape_rtsp_userinfo_encodes_reserved_characters() {
        assert_eq!(super::escape_rtsp_userinfo("pa:ss@word"), "pa%3Ass%40word");
    }
}
