//! RTSP media stream adapter boundary.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use reqwest::blocking::Client;
use reqwest::Url;
use serde::Deserialize;

use crate::runtime::discovery::{RtspProbeRequest, RtspProbeResult};
use crate::runtime::media::{
    ClipCaptureRequest, ClipCaptureResult, SnapshotCaptureRequest, SnapshotCaptureResult,
    SnapshotFormat, StreamOpenRequest, StreamOpenResult,
};
use crate::runtime::media_tools::{
    command_exists, ffmpeg_resolution_hint, resolve_ffmpeg_bin, resolve_ffprobe_bin,
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
    const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(100);
    const FFPROBE_TIMEOUT: Duration = Duration::from_secs(7);
    const FFMPEG_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(12);
    const HTTP_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(8);
    const RTSP_DESCRIBE_TIMEOUT: Duration = Duration::from_secs(4);

    pub fn new(ffmpeg_bin: impl Into<String>) -> Self {
        Self::with_bins(
            ffmpeg_bin,
            resolve_ffprobe_bin().unwrap_or_else(|| "ffprobe".to_string()),
        )
    }

    fn with_bins(ffmpeg_bin: impl Into<String>, ffprobe_bin: impl Into<String>) -> Self {
        Self {
            ffmpeg_bin: ffmpeg_bin.into(),
            ffprobe_bin: ffprobe_bin.into(),
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
        command_exists(&self.ffmpeg_bin)
    }

    fn ffprobe_available(&self) -> bool {
        command_exists(&self.ffprobe_bin)
    }

    fn ffmpeg_missing_error(&self) -> String {
        format!(
            "ffmpeg is required for RTSP snapshot capture but '{}' is unavailable; {}",
            self.ffmpeg_bin,
            ffmpeg_resolution_hint()
        )
    }

    fn spawn_pipe_reader<R>(
        mut pipe: R,
        stream_name: &'static str,
    ) -> thread::JoinHandle<Result<Vec<u8>, String>>
    where
        R: Read + Send + 'static,
    {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes)
                .map_err(|error| format!("failed to read child {stream_name}: {error}"))?;
            Ok(bytes)
        })
    }

    fn collect_pipe_output(
        handle: Option<thread::JoinHandle<Result<Vec<u8>, String>>>,
        stream_name: &'static str,
    ) -> Result<Vec<u8>, String> {
        let Some(handle) = handle else {
            return Ok(Vec::new());
        };

        handle
            .join()
            .map_err(|_| format!("child {stream_name} reader thread panicked"))?
    }

    fn collect_child_output(
        status: ExitStatus,
        stdout_reader: Option<thread::JoinHandle<Result<Vec<u8>, String>>>,
        stderr_reader: Option<thread::JoinHandle<Result<Vec<u8>, String>>>,
    ) -> Result<Output, String> {
        Ok(Output {
            status,
            stdout: Self::collect_pipe_output(stdout_reader, "stdout")?,
            stderr: Self::collect_pipe_output(stderr_reader, "stderr")?,
        })
    }

    fn run_command_with_timeout(
        &self,
        command: &mut Command,
        timeout: Duration,
        operation: &str,
    ) -> Result<Output, String> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to launch {operation}: {error}"))?;
        let stdout_reader = child
            .stdout
            .take()
            .map(|pipe| Self::spawn_pipe_reader(pipe, "stdout"));
        let stderr_reader = child
            .stderr
            .take()
            .map(|pipe| Self::spawn_pipe_reader(pipe, "stderr"));
        let started_at = Instant::now();

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Self::collect_child_output(status, stdout_reader, stderr_reader);
                }
                Ok(None) if started_at.elapsed() < timeout => {
                    thread::sleep(Self::COMMAND_POLL_INTERVAL);
                }
                Ok(None) => {
                    let _ = child.kill();
                    let status = child.wait().map_err(|error| {
                        format!("failed to stop {operation} after timeout: {error}")
                    })?;
                    let output = Self::collect_child_output(status, stdout_reader, stderr_reader)?;
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let timeout_secs = timeout.as_secs();
                    if stderr.is_empty() {
                        return Err(format!("{operation} timed out after {timeout_secs}s"));
                    }
                    return Err(format!(
                        "{operation} timed out after {timeout_secs}s: {stderr}"
                    ));
                }
                Err(error) => return Err(format!("failed while waiting for {operation}: {error}")),
            }
        }
    }

    fn run_media_command_with_optional_rw_timeout<F>(
        &self,
        program: &str,
        timeout: Duration,
        operation: &str,
        mut configure: F,
    ) -> Result<Output, String>
    where
        F: FnMut(&mut Command, bool),
    {
        let mut command = Command::new(program);
        configure(&mut command, true);
        let output = self.run_command_with_timeout(&mut command, timeout, operation)?;
        if output.status.success() || !Self::missing_rw_timeout_option(&output.stderr) {
            return Ok(output);
        }

        let mut retry = Command::new(program);
        configure(&mut retry, false);
        self.run_command_with_timeout(&mut retry, timeout, operation)
    }

    fn missing_rw_timeout_option(stderr: &[u8]) -> bool {
        String::from_utf8_lossy(stderr)
            .to_ascii_lowercase()
            .contains("option rw_timeout not found")
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
        if player == "open" {
            return Ok(vec![stream_url.to_string()]);
        }

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

    fn probe_stream_url_with_ffprobe(&self, stream_url: &str) -> Result<ProbeOutcome, String> {
        let output = self.run_media_command_with_optional_rw_timeout(
            &self.ffprobe_bin,
            Self::FFPROBE_TIMEOUT,
            "ffprobe for RTSP probe",
            |command, include_rw_timeout| {
                command.args(["-v", "error", "-rtsp_transport", "tcp"]);
                if include_rw_timeout {
                    command.args(["-rw_timeout", "5000000"]);
                }
                command.args([
                    "-show_entries",
                    "stream=codec_name,codec_type",
                    "-of",
                    "json",
                    stream_url,
                ]);
            },
        )?;

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

    fn basic_authorization_header(
        username: Option<&str>,
        password: Option<&str>,
    ) -> Option<String> {
        let username = username?.trim();
        if username.is_empty() {
            return None;
        }

        let credentials = format!("{username}:{}", password.unwrap_or_default());
        Some(format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes())
        ))
    }

    fn content_length_from_headers(headers: &str) -> Option<usize> {
        headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("Content-Length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
    }

    fn read_rtsp_response(stream: &mut TcpStream) -> Result<String, String> {
        let mut response = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut header_end = None;
        let mut content_length = None;

        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    response.extend_from_slice(&buffer[..bytes_read]);

                    if header_end.is_none() {
                        if let Some(position) =
                            response.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let end = position + 4;
                            header_end = Some(end);
                            let headers = String::from_utf8_lossy(&response[..end]);
                            content_length = Self::content_length_from_headers(headers.as_ref());
                        }
                    }

                    if let Some(end) = header_end {
                        if let Some(body_length) = content_length {
                            if response.len() >= end + body_length {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => return Err(format!("failed to read RTSP response: {error}")),
            }
        }

        if response.is_empty() {
            return Err("RTSP DESCRIBE returned no response".to_string());
        }

        Ok(String::from_utf8_lossy(&response).into_owned())
    }

    fn probe_outcome_from_rtsp_describe_response(response: &str) -> Result<ProbeOutcome, String> {
        let (headers, body) = response
            .split_once("\r\n\r\n")
            .or_else(|| response.split_once("\n\n"))
            .unwrap_or((response, ""));
        let status_line = headers
            .lines()
            .next()
            .ok_or_else(|| "RTSP DESCRIBE returned an empty response".to_string())?;
        let mut parts = status_line.splitn(3, ' ');
        let protocol = parts.next().unwrap_or_default();
        if !protocol.starts_with("RTSP/") {
            return Err(format!("invalid RTSP status line: {status_line}"));
        }

        let status_code: u16 = parts
            .next()
            .ok_or_else(|| format!("missing RTSP status code in response: {status_line}"))?
            .parse()
            .map_err(|error| format!("invalid RTSP status code in response: {error}"))?;
        let reason = parts.next().unwrap_or_default().trim();

        match status_code {
            200 => {
                let has_video = body
                    .lines()
                    .any(|line| line.trim_start().starts_with("m=video "));
                let has_audio = body
                    .lines()
                    .any(|line| line.trim_start().starts_with("m=audio "));
                if !has_video {
                    return Err(
                        "RTSP DESCRIBE succeeded but SDP did not advertise a video stream"
                            .to_string(),
                    );
                }
                Ok(ProbeOutcome {
                    has_video,
                    has_audio,
                })
            }
            401 => Err("RTSP authentication failed".to_string()),
            404 => Err("RTSP stream path not found".to_string()),
            status if reason.is_empty() => {
                Err(format!("RTSP DESCRIBE failed with status {status}"))
            }
            status => Err(format!("RTSP DESCRIBE failed with {status} {reason}")),
        }
    }

    fn probe_stream_url_via_rtsp_describe(
        &self,
        ip_address: &str,
        port: u16,
        path: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<ProbeOutcome, String> {
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let socket_addr = format!("{ip_address}:{port}")
            .to_socket_addrs()
            .map_err(|error| {
                format!("failed to resolve RTSP address {ip_address}:{port}: {error}")
            })?
            .next()
            .ok_or_else(|| format!("failed to resolve RTSP address {ip_address}:{port}"))?;
        let mut stream = TcpStream::connect_timeout(&socket_addr, Self::RTSP_DESCRIBE_TIMEOUT)
            .map_err(|error| format!("failed to connect RTSP socket: {error}"))?;
        stream
            .set_read_timeout(Some(Self::RTSP_DESCRIBE_TIMEOUT))
            .map_err(|error| format!("failed to set RTSP read timeout: {error}"))?;
        stream
            .set_write_timeout(Some(Self::RTSP_DESCRIBE_TIMEOUT))
            .map_err(|error| format!("failed to set RTSP write timeout: {error}"))?;

        let stream_url = format!("rtsp://{ip_address}:{port}{normalized_path}");
        let auth_header = Self::basic_authorization_header(username, password)
            .map(|value| format!("Authorization: {value}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "DESCRIBE {stream_url} RTSP/1.0\r\nCSeq: 1\r\nAccept: application/sdp\r\nUser-Agent: HarborBeacon/rtsp-probe\r\n{auth_header}\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| format!("failed to send RTSP DESCRIBE: {error}"))?;
        stream
            .flush()
            .map_err(|error| format!("failed to flush RTSP DESCRIBE: {error}"))?;

        let response = Self::read_rtsp_response(&mut stream)?;
        Self::probe_outcome_from_rtsp_describe_response(&response)
    }

    fn probe_stream_url(
        &self,
        stream_url: &str,
        ip_address: &str,
        port: u16,
        path: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<ProbeOutcome, String> {
        if self.ffprobe_available() {
            return self.probe_stream_url_with_ffprobe(stream_url);
        }

        self.probe_stream_url_via_rtsp_describe(ip_address, port, path, username, password)
    }

    fn capture_snapshot_with_ffmpeg(
        &self,
        request: &SnapshotCaptureRequest,
    ) -> Result<SnapshotCaptureResult, String> {
        let codec = match request.format {
            SnapshotFormat::Jpeg => "mjpeg",
            SnapshotFormat::Png => "png",
        };

        let output = self.run_media_command_with_optional_rw_timeout(
            &self.ffmpeg_bin,
            Self::FFMPEG_SNAPSHOT_TIMEOUT,
            "ffmpeg for snapshot capture",
            |command, include_rw_timeout| {
                command.args(["-rtsp_transport", "tcp"]);
                if include_rw_timeout {
                    command.args(["-rw_timeout", "10000000"]);
                }
                command.args([
                    "-i",
                    &request.stream_url,
                    "-frames:v",
                    "1",
                    "-f",
                    "image2pipe",
                    "-vcodec",
                    codec,
                    "-",
                ]);
            },
        )?;

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

    pub fn capture_clip_to_path(
        &self,
        request: &ClipCaptureRequest,
        output_path: &Path,
    ) -> Result<ClipCaptureResult, String> {
        if !self.ffmpeg_available() {
            return Err(self.ffmpeg_missing_error());
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create clip output directory {}: {error}",
                    parent.display()
                )
            })?;
        }

        let timeout = Duration::from_secs(u64::from(request.clip_length_seconds.max(1)) + 20);
        let clip_length_seconds = request.clip_length_seconds.max(1).to_string();
        let output_path = output_path.to_path_buf();
        let output = self.run_media_command_with_optional_rw_timeout(
            &self.ffmpeg_bin,
            timeout,
            "ffmpeg for clip capture",
            |command, include_rw_timeout| {
                command.args(["-y", "-rtsp_transport", "tcp"]);
                if include_rw_timeout {
                    command.args(["-rw_timeout", "10000000"]);
                }
                command.args([
                    "-i",
                    &request.stream_url,
                    "-map",
                    "0:v:0",
                    "-map",
                    "0:a?",
                    "-t",
                    &clip_length_seconds,
                    "-c",
                    "copy",
                    "-movflags",
                    "+faststart",
                ]);
                command.arg(&output_path);
            },
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "ffmpeg exited without stderr output".to_string()
            } else {
                stderr
            };
            return Err(format!("ffmpeg clip capture failed: {detail}"));
        }

        let byte_size = fs::metadata(&output_path)
            .map_err(|error| {
                format!(
                    "clip capture succeeded, but {} could not be inspected: {error}",
                    output_path.display()
                )
            })?
            .len() as usize;
        let mut result = ClipCaptureResult::new(
            request.device_id.clone(),
            request.clip_length_seconds,
            byte_size,
            request.storage_target,
        )
        .with_keyframe_hints(
            request.keyframe_count,
            request.keyframe_interval_seconds,
        );
        result.storage.relative_path = output_path.to_string_lossy().to_string();
        result.index_sidecar_relative_path = output_path
            .with_extension("json")
            .to_string_lossy()
            .to_string();
        Ok(result)
    }

    pub fn extract_keyframes(
        &self,
        video_path: &Path,
        output_dir: &Path,
        keyframe_count: Option<u32>,
        keyframe_interval_seconds: Option<u32>,
    ) -> Result<Vec<PathBuf>, String> {
        if !self.ffmpeg_available() {
            return Err(self.ffmpeg_missing_error());
        }

        fs::create_dir_all(output_dir).map_err(|error| {
            format!(
                "failed to create keyframe directory {}: {error}",
                output_dir.display()
            )
        })?;

        let count = keyframe_count.unwrap_or(4).clamp(1, 12);
        let interval = keyframe_interval_seconds.unwrap_or(3).clamp(1, 60);
        let output_pattern = output_dir.join("frame-%03d.jpg");

        let mut command = Command::new(&self.ffmpeg_bin);
        command.args([
            "-y",
            "-i",
            video_path.to_string_lossy().as_ref(),
            "-vf",
            &format!("fps=1/{interval}"),
            "-frames:v",
            &count.to_string(),
            output_pattern.to_string_lossy().as_ref(),
        ]);
        let output = self.run_command_with_timeout(
            &mut command,
            Duration::from_secs(20),
            "ffmpeg for keyframe extraction",
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "ffmpeg exited without stderr output".to_string()
            } else {
                stderr
            };
            return Err(format!("ffmpeg keyframe extraction failed: {detail}"));
        }

        let mut paths = fs::read_dir(output_dir)
            .map_err(|error| {
                format!(
                    "failed to enumerate keyframe directory {}: {error}",
                    output_dir.display()
                )
            })?
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jpg"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        Ok(paths)
    }

    fn capture_snapshot_via_http(
        &self,
        request: &SnapshotCaptureRequest,
        snapshot_url: &str,
    ) -> Result<SnapshotCaptureResult, String> {
        let url = Self::resolve_snapshot_http_url(snapshot_url, &request.stream_url)?;
        let auth = Self::snapshot_request_auth(snapshot_url, &request.stream_url);
        let mut sanitized_url = url.clone();
        let _ = sanitized_url.set_username("");
        let _ = sanitized_url.set_password(None);

        let client = Client::builder()
            .timeout(Self::HTTP_SNAPSHOT_TIMEOUT)
            .build()
            .map_err(|error| format!("failed to construct native snapshot client: {error}"))?;

        let mut http_request = client.get(sanitized_url.clone());
        if let Some((username, password)) = auth {
            http_request = http_request.basic_auth(username, password);
        }

        let response = http_request.send().map_err(|error| {
            format!(
                "native snapshot request to {} failed: {error}",
                sanitized_url
            )
        })?;
        if !response.status().is_success() {
            return Err(format!(
                "native snapshot request to {} failed with HTTP {}",
                sanitized_url,
                response.status()
            ));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let bytes = response
            .bytes()
            .map_err(|error| format!("failed to read native snapshot response: {error}"))?;
        if bytes.is_empty() {
            return Err("native snapshot response was empty".to_string());
        }

        let format = Self::snapshot_format_from_response(content_type.as_deref(), bytes.as_ref())
            .ok_or_else(|| {
            let content_type = content_type.unwrap_or_else(|| "unknown".to_string());
            format!(
                "native snapshot returned unsupported image payload (content-type: {content_type})"
            )
        })?;

        Ok(SnapshotCaptureResult::new(
            request.device_id.clone(),
            format,
            base64::engine::general_purpose::STANDARD.encode(bytes.as_ref()),
            bytes.len(),
            request.storage_target,
        ))
    }

    fn resolve_snapshot_http_url(snapshot_url: &str, stream_url: &str) -> Result<Url, String> {
        let trimmed = snapshot_url.trim();
        if trimmed.is_empty() {
            return Err("snapshot_url is empty".to_string());
        }

        if let Ok(url) = Url::parse(trimmed) {
            return Ok(url);
        }

        let stream_url = Url::parse(stream_url)
            .map_err(|error| format!("failed to resolve snapshot host from stream_url: {error}"))?;
        let host = stream_url.host_str().ok_or_else(|| {
            "stream_url did not contain a host for snapshot resolution".to_string()
        })?;
        let mut url = Url::parse("http://localhost")
            .map_err(|error| format!("failed to construct snapshot base URL: {error}"))?;
        url.set_host(Some(host))
            .map_err(|_| format!("invalid snapshot host derived from stream_url: {host}"))?;

        if trimmed.starts_with(':') {
            return Url::parse(&format!("http://{host}{trimmed}"))
                .map_err(|error| format!("invalid snapshot_url '{trimmed}': {error}"));
        }

        if let Some(query) = trimmed.strip_prefix('?') {
            url.set_path("/");
            url.set_query(Some(query));
            return Ok(url);
        }

        let normalized = if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            format!("/{trimmed}")
        };
        if let Some((path, query)) = normalized.split_once('?') {
            url.set_path(if path.is_empty() { "/" } else { path });
            url.set_query(Some(query));
        } else {
            url.set_path(&normalized);
        }
        Ok(url)
    }

    fn snapshot_request_auth(
        snapshot_url: &str,
        stream_url: &str,
    ) -> Option<(String, Option<String>)> {
        Url::parse(snapshot_url)
            .ok()
            .and_then(|url| Self::userinfo_from_url(&url))
            .or_else(|| Self::stream_url_auth(stream_url))
    }

    fn userinfo_from_url(url: &Url) -> Option<(String, Option<String>)> {
        let username = url.username().trim();
        (!username.is_empty()).then(|| (username.to_string(), url.password().map(str::to_string)))
    }

    fn stream_url_auth(stream_url: &str) -> Option<(String, Option<String>)> {
        Url::parse(stream_url)
            .ok()
            .and_then(|url| Self::userinfo_from_url(&url))
            .or_else(|| Self::userinfo_from_authority(stream_url))
    }

    fn userinfo_from_authority(value: &str) -> Option<(String, Option<String>)> {
        let authority = value.split_once("://")?.1.split_once('@')?.0;
        if authority.trim().is_empty() {
            return None;
        }
        let (username, password) = authority
            .split_once(':')
            .map(|(username, password)| (username.trim(), Some(password.trim().to_string())))
            .unwrap_or((authority.trim(), None));
        (!username.is_empty()).then(|| (username.to_string(), password))
    }

    fn snapshot_format_from_response(
        content_type: Option<&str>,
        bytes: &[u8],
    ) -> Option<SnapshotFormat> {
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(SnapshotFormat::Jpeg);
        }
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some(SnapshotFormat::Png);
        }

        let content_type = content_type?.to_ascii_lowercase();
        if content_type.contains("jpeg") || content_type.contains("jpg") {
            Some(SnapshotFormat::Jpeg)
        } else if content_type.contains("png") {
            Some(SnapshotFormat::Png)
        } else {
            None
        }
    }
}

impl Default for CommandRtspAdapter {
    fn default() -> Self {
        Self::with_bins(
            resolve_ffmpeg_bin().unwrap_or_else(|| "ffmpeg".to_string()),
            resolve_ffprobe_bin().unwrap_or_else(|| "ffprobe".to_string()),
        )
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

            match self.probe_stream_url(
                &stream_url,
                &request.ip_address,
                request.port,
                &path,
                request.username.as_deref(),
                request.password.as_deref(),
            ) {
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
        let native_snapshot_error = if let Some(snapshot_url) = request.snapshot_url.as_deref() {
            match self.capture_snapshot_via_http(request, snapshot_url) {
                Ok(result) => return Ok(result),
                Err(error) => Some(error),
            }
        } else {
            None
        };

        if !self.ffmpeg_available() {
            return native_snapshot_error.map_or_else(
                || Err(self.ffmpeg_missing_error()),
                |error| {
                    Err(format!(
                        "native snapshot capture failed: {error}; {}",
                        self.ffmpeg_missing_error()
                    ))
                },
            );
        }

        match self.capture_snapshot_with_ffmpeg(request) {
            Ok(result) => Ok(result),
            Err(ffmpeg_error) => {
                if let Some(native_error) = native_snapshot_error {
                    Err(format!(
                        "native snapshot capture failed: {native_error}; ffmpeg fallback also failed: {ffmpeg_error}"
                    ))
                } else {
                    Err(ffmpeg_error)
                }
            }
        }
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use base64::Engine as _;

    use super::{CommandRtspAdapter, RtspProbeAdapter};
    use crate::connectors::storage::StorageTarget;
    use crate::runtime::discovery::RtspProbeRequest;
    use crate::runtime::media::{SnapshotCaptureRequest, SnapshotFormat};

    fn spawn_snapshot_server(
        expected_auth_header: Option<String>,
        content_type: &str,
        body: &[u8],
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind snapshot server");
        let address = listener.local_addr().expect("snapshot server address");
        let content_type = content_type.to_string();
        let body = body.to_vec();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept snapshot request");
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let bytes_read = stream.read(&mut buffer).expect("read snapshot request");
                if bytes_read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..bytes_read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let request_text = String::from_utf8_lossy(&request);
            let authorized = expected_auth_header
                .as_deref()
                .map(|header| {
                    request_text
                        .to_ascii_lowercase()
                        .contains(&header.to_ascii_lowercase())
                })
                .unwrap_or(true);
            if authorized {
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write snapshot response headers");
                stream
                    .write_all(&body)
                    .expect("write snapshot response body");
            } else {
                stream
                    .write_all(
                        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .expect("write unauthorized snapshot response");
            }
        });

        format!("http://{address}/snapshot.jpg")
    }

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
    fn rw_timeout_compatibility_error_is_detected() {
        assert!(CommandRtspAdapter::missing_rw_timeout_option(
            b"Option rw_timeout not found.\n"
        ));
        assert!(!CommandRtspAdapter::missing_rw_timeout_option(
            b"Connection refused\n"
        ));
    }

    #[test]
    fn snapshot_uses_native_snapshot_url_without_ffmpeg_when_available() {
        let auth_header = format!(
            "Authorization: Basic {}",
            base64::engine::general_purpose::STANDARD.encode(b"admin:secret")
        );
        let snapshot_url =
            spawn_snapshot_server(Some(auth_header), "image/jpeg", &[0xFF, 0xD8, 0xFF, 0xD9]);
        let adapter = CommandRtspAdapter::new("ffmpeg-does-not-exist");

        let result = adapter
            .capture_snapshot(
                &SnapshotCaptureRequest::new(
                    "cam-1",
                    "rtsp://admin:secret@192.168.1.30/live",
                    SnapshotFormat::Jpeg,
                    StorageTarget::LocalDisk,
                )
                .with_snapshot_url(Some(snapshot_url)),
            )
            .expect("native snapshot capture should succeed");

        assert_eq!(result.format, SnapshotFormat::Jpeg);
        assert_eq!(result.mime_type, "image/jpeg");
        assert_eq!(result.byte_size, 4);
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
    fn rtsp_describe_response_detects_video_and_audio_tracks() {
        let response = "RTSP/1.0 200 OK\r\nContent-Type: application/sdp\r\nContent-Length: 96\r\n\r\nv=0\r\nm=video 0 RTP/AVP 96\r\na=rtpmap:96 H264/90000\r\nm=audio 0 RTP/AVP 104\r\n";
        let outcome = CommandRtspAdapter::probe_outcome_from_rtsp_describe_response(response)
            .expect("describe response should parse");

        assert!(outcome.has_video);
        assert!(outcome.has_audio);
    }

    #[test]
    fn rtsp_describe_response_surfaces_auth_failure() {
        let response =
            "RTSP/1.0 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Camera\"\r\n\r\n";
        let error = CommandRtspAdapter::probe_outcome_from_rtsp_describe_response(response)
            .expect_err("auth failure should be reported");

        assert!(error.contains("authentication failed"));
    }

    #[test]
    fn escape_rtsp_userinfo_encodes_reserved_characters() {
        assert_eq!(super::escape_rtsp_userinfo("pa:ss@word"), "pa%3Ass%40word");
    }
}
