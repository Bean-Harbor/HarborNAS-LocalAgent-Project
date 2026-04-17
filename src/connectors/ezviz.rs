use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EzvizPtzDirection {
    Up,
    Down,
    Left,
    Right,
    Stop,
}

impl EzvizPtzDirection {
    pub fn command_code(&self) -> Option<u8> {
        match self {
            EzvizPtzDirection::Up => Some(0),
            EzvizPtzDirection::Down => Some(1),
            EzvizPtzDirection::Left => Some(2),
            EzvizPtzDirection::Right => Some(3),
            EzvizPtzDirection::Stop => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EzvizCloudConfig {
    pub base_url: String,
    pub app_key: String,
    pub app_secret: String,
    pub access_token: Option<String>,
}

impl EzvizCloudConfig {
    pub fn from_env() -> Option<Self> {
        let app_key = std::env::var("HARBOR_EZVIZ_APP_KEY").ok()?;
        let app_secret = std::env::var("HARBOR_EZVIZ_APP_SECRET").ok()?;
        let access_token = std::env::var("HARBOR_EZVIZ_ACCESS_TOKEN").ok();
        let base_url = std::env::var("HARBOR_EZVIZ_BASE_URL")
            .unwrap_or_else(|_| "https://open.ys7.com".to_string());
        Some(Self {
            base_url,
            app_key,
            app_secret,
            access_token,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EzvizPtzRequest {
    pub device_serial: String,
    pub camera_no: u32,
    pub direction: EzvizPtzDirection,
    pub speed: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EzvizPtzResult {
    pub provider: String,
    pub action: String,
    pub device_serial: String,
    pub camera_no: u32,
}

pub struct EzvizCloudPtzConnector {
    client: Client,
    config: EzvizCloudConfig,
}

#[derive(Debug, Clone)]
struct CachedAccessToken {
    app_key: String,
    token: String,
    expires_at_ms: i64,
}

fn access_token_cache() -> &'static Mutex<Option<CachedAccessToken>> {
    static CACHE: OnceLock<Mutex<Option<CachedAccessToken>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

impl EzvizCloudPtzConnector {
    pub fn new(config: EzvizCloudConfig) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("failed to build EZVIZ client: {e}"))?;
        Ok(Self { client, config })
    }

    pub fn control_ptz(&self, request: &EzvizPtzRequest) -> Result<EzvizPtzResult, String> {
        let access_token = self.resolve_access_token()?;
        if request.direction == EzvizPtzDirection::Stop {
            let camera_no = request.camera_no.to_string();
            self.post_form(
                "/api/lapp/device/ptz/stop",
                &[
                    ("accessToken", access_token.as_str()),
                    ("deviceSerial", request.device_serial.as_str()),
                    ("cameraNo", camera_no.as_str()),
                    ("channelNo", camera_no.as_str()),
                ],
            )?;
        } else {
            let command = request
                .direction
                .command_code()
                .ok_or_else(|| "EZVIZ PTZ direction missing command code".to_string())?
                .to_string();
            let speed = request.speed.clamp(1, 7).to_string();
            let camera_no = request.camera_no.to_string();
            self.post_form(
                "/api/lapp/device/ptz/start",
                &[
                    ("accessToken", access_token.as_str()),
                    ("deviceSerial", request.device_serial.as_str()),
                    ("cameraNo", camera_no.as_str()),
                    ("channelNo", camera_no.as_str()),
                    ("direction", command.as_str()),
                    ("speed", speed.as_str()),
                ],
            )?;
        }

        Ok(EzvizPtzResult {
            provider: "ezviz_cloud".to_string(),
            action: match request.direction {
                EzvizPtzDirection::Stop => "stop".to_string(),
                _ => format!("{:?}", request.direction).to_lowercase(),
            },
            device_serial: request.device_serial.clone(),
            camera_no: request.camera_no,
        })
    }

    pub fn warm_up_access_token(&self) -> Result<(), String> {
        self.resolve_access_token().map(|_| ())
    }

    fn resolve_access_token(&self) -> Result<String, String> {
        if let Some(token) = &self.config.access_token {
            if !token.trim().is_empty() {
                return Ok(token.clone());
            }
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_millis() as i64)
            .unwrap_or_default();
        if let Ok(cache) = access_token_cache().lock() {
            if let Some(entry) = cache.as_ref() {
                if entry.app_key == self.config.app_key && entry.expires_at_ms > now_ms + 60_000 {
                    println!("[EZVIZ] access token cache hit");
                    return Ok(entry.token.clone());
                }
            }
        }

        println!("[EZVIZ] access token cache miss");

        let payload = self.post_form(
            "/api/lapp/token/get",
            &[
                ("appKey", self.config.app_key.as_str()),
                ("appSecret", self.config.app_secret.as_str()),
            ],
        )?;

        let access_token = payload
            .pointer("/data/accessToken")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .ok_or_else(|| "EZVIZ token response missing data.accessToken".to_string())?;
        let expires_at_ms = payload
            .pointer("/data/expireTime")
            .and_then(|value| value.as_i64())
            .unwrap_or(now_ms + 6 * 24 * 60 * 60 * 1000);
        if let Ok(mut cache) = access_token_cache().lock() {
            *cache = Some(CachedAccessToken {
                app_key: self.config.app_key.clone(),
                token: access_token.clone(),
                expires_at_ms,
            });
        }
        Ok(access_token)
    }

    fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<Value, String> {
        let url = format!("{}{}", self.config.base_url.trim_end_matches('/'), path);
        let started_at = Instant::now();
        let response = self
            .client
            .post(&url)
            .form(form)
            .send()
            .map_err(|e| format!("EZVIZ request failed for {}: {e}", url))?;
        let payload: Value = response
            .json()
            .map_err(|e| format!("EZVIZ response parse failed for {}: {e}", url))?;
        println!(
            "[EZVIZ] {} completed in {} ms",
            path,
            started_at.elapsed().as_millis()
        );

        let code = payload
            .get("code")
            .and_then(|value| value.as_str())
            .unwrap_or("0");
        if code != "200" && code != "0" {
            let message = payload
                .get("msg")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown EZVIZ error");
            return Err(format!("EZVIZ API error {}: {}", code, message));
        }

        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::EzvizPtzDirection;

    #[test]
    fn ezviz_direction_command_mapping_matches_sdk_convention() {
        assert_eq!(EzvizPtzDirection::Up.command_code(), Some(0));
        assert_eq!(EzvizPtzDirection::Down.command_code(), Some(1));
        assert_eq!(EzvizPtzDirection::Left.command_code(), Some(2));
        assert_eq!(EzvizPtzDirection::Right.command_code(), Some(3));
        assert_eq!(EzvizPtzDirection::Stop.command_code(), None);
    }
}
