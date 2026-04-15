//! Signed share links for camera live view pages.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CameraShareClaims {
    pub v: u8,
    pub kind: String,
    pub device_id: String,
    pub exp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCameraShareToken {
    pub token: String,
    pub expires_at_unix_secs: u64,
    pub ttl_minutes: u32,
}

pub fn issue_camera_share_token(
    secret: &str,
    device_id: &str,
    ttl_minutes: u32,
) -> Result<IssuedCameraShareToken, String> {
    if secret.trim().is_empty() {
        return Err("share secret is empty".to_string());
    }
    if device_id.trim().is_empty() {
        return Err("device_id is empty".to_string());
    }

    let ttl_minutes = ttl_minutes.clamp(1, 24 * 60);
    let expires_at_unix_secs = now_unix_secs()
        .checked_add(u64::from(ttl_minutes) * 60)
        .ok_or_else(|| "share token expiration overflowed".to_string())?;
    let claims = CameraShareClaims {
        v: 1,
        kind: "camera_live".to_string(),
        device_id: device_id.to_string(),
        exp: expires_at_unix_secs,
    };
    let payload = serde_json::to_vec(&claims)
        .map_err(|error| format!("failed to serialize camera share claims: {error}"))?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload);
    let signature_b64 = sign_payload(secret, &payload_b64)?;

    Ok(IssuedCameraShareToken {
        token: format!("{payload_b64}.{signature_b64}"),
        expires_at_unix_secs,
        ttl_minutes,
    })
}

pub fn verify_camera_share_token(secret: &str, token: &str) -> Result<CameraShareClaims, String> {
    if secret.trim().is_empty() {
        return Err("share secret is empty".to_string());
    }

    let (payload_b64, signature_b64) = token
        .split_once('.')
        .ok_or_else(|| "share token format is invalid".to_string())?;
    if payload_b64.is_empty() || signature_b64.is_empty() {
        return Err("share token format is invalid".to_string());
    }

    let expected_signature = sign_payload(secret, payload_b64)?;
    let provided_signature = URL_SAFE_NO_PAD
        .decode(signature_b64.as_bytes())
        .map_err(|error| format!("share token signature is invalid: {error}"))?;

    let mut mac = new_hmac(secret)?;
    mac.update(payload_b64.as_bytes());
    let expected_signature = URL_SAFE_NO_PAD
        .decode(expected_signature.as_bytes())
        .map_err(|error| format!("expected share signature is invalid: {error}"))?;
    if expected_signature.len() != provided_signature.len() {
        return Err("share token signature mismatch".to_string());
    }
    mac.verify_slice(&provided_signature)
        .map_err(|_| "share token signature mismatch".to_string())?;

    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64.as_bytes())
        .map_err(|error| format!("share token payload is invalid: {error}"))?;
    let claims: CameraShareClaims = serde_json::from_slice(&payload)
        .map_err(|error| format!("share token payload parse failed: {error}"))?;
    if claims.v != 1 || claims.kind != "camera_live" {
        return Err("share token scope is invalid".to_string());
    }
    if claims.device_id.trim().is_empty() {
        return Err("share token missing device_id".to_string());
    }
    if now_unix_secs() > claims.exp {
        return Err("share token expired".to_string());
    }

    Ok(claims)
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

fn sign_payload(secret: &str, payload_b64: &str) -> Result<String, String> {
    let mut mac = new_hmac(secret)?;
    mac.update(payload_b64.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn new_hmac(secret: &str) -> Result<HmacSha256, String> {
    HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|error| format!("failed to initialize HMAC: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{issue_camera_share_token, verify_camera_share_token};

    #[test]
    fn issued_camera_share_tokens_round_trip() {
        let issued =
            issue_camera_share_token("test-secret", "cam-1", 15).expect("share token issued");
        let claims =
            verify_camera_share_token("test-secret", &issued.token).expect("share token valid");
        assert_eq!(claims.device_id, "cam-1");
        assert_eq!(claims.kind, "camera_live");
    }

    #[test]
    fn tampered_camera_share_tokens_are_rejected() {
        let issued =
            issue_camera_share_token("test-secret", "cam-1", 15).expect("share token issued");
        let tampered = format!("{}x", issued.token);
        assert!(verify_camera_share_token("test-secret", &tampered).is_err());
    }
}
