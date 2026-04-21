//! Shared media binary discovery helpers.

use std::env;
use std::path::{Path, PathBuf};

const FFMPEG_ENV_VAR: &str = "HARBOR_FFMPEG_BIN";
const FFPROBE_ENV_VAR: &str = "HARBOR_FFPROBE_BIN";

pub fn resolve_ffmpeg_bin() -> Option<String> {
    resolve_media_binary(FFMPEG_ENV_VAR, "ffmpeg")
}

pub fn resolve_ffprobe_bin() -> Option<String> {
    resolve_media_binary(FFPROBE_ENV_VAR, "ffprobe")
}

pub fn ffmpeg_resolution_hint() -> String {
    format!(
        "set {} or place ffmpeg under tools/ffmpeg/bin, .harborbeacon/bin, or PATH",
        FFMPEG_ENV_VAR
    )
}

pub fn command_exists(spec: &str) -> bool {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return false;
    }

    let path = Path::new(trimmed);
    path.is_file() || which::which(trimmed).is_ok()
}

fn resolve_media_binary(env_var: &str, binary_name: &str) -> Option<String> {
    if let Some(configured) = env::var_os(env_var) {
        let configured = configured.to_string_lossy().trim().to_string();
        if command_exists(&configured) {
            return Some(configured);
        }
    }

    for candidate in bundled_binary_candidates(binary_name) {
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }

    which::which(binary_name)
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

fn bundled_binary_candidates(binary_name: &str) -> Vec<PathBuf> {
    let file_name = binary_file_name(binary_name);
    let Ok(repo_root) = env::current_dir() else {
        return Vec::new();
    };

    vec![
        repo_root
            .join("tools")
            .join("ffmpeg")
            .join("bin")
            .join(&file_name),
        repo_root.join(".harborbeacon").join("bin").join(file_name),
    ]
}

fn binary_file_name(binary_name: &str) -> String {
    if cfg!(windows) {
        format!("{binary_name}.exe")
    } else {
        binary_name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{binary_file_name, command_exists};

    #[test]
    fn binary_file_name_matches_platform_suffix() {
        if cfg!(windows) {
            assert_eq!(binary_file_name("ffmpeg"), "ffmpeg.exe");
        } else {
            assert_eq!(binary_file_name("ffmpeg"), "ffmpeg");
        }
    }

    #[test]
    fn command_exists_accepts_direct_file_paths() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("harborbeacon-media-bin-{unique}.cmd"));
        fs::write(&path, "@echo off\r\n").expect("write temp bin");

        assert!(command_exists(path.to_string_lossy().as_ref()));

        let _ = fs::remove_file(path);
    }
}
