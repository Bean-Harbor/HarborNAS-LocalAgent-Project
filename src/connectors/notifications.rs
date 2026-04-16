//! Outbound notification contract for IM, webhook, and local alert delivery.

use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    #[serde(alias = "feishu")]
    ImBridge,
    Wecom,
    Telegram,
    Webhook,
    LocalUi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPayloadFormat {
    PlainText,
    Markdown,
    LarkCard,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAttachmentKind {
    Image,
    Video,
    Link,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationAttachment {
    pub kind: NotificationAttachmentKind,
    pub label: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationRequest {
    pub channel: NotificationChannel,
    pub destination: String,
    pub title: String,
    pub body: String,
    #[serde(default = "default_payload_format")]
    pub payload_format: NotificationPayloadFormat,
    #[serde(default)]
    pub structured_payload: Value,
    #[serde(default)]
    pub attachments: Vec<NotificationAttachment>,
    #[serde(default)]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationRecipientIdType {
    ChatId,
    OpenId,
}

impl NotificationRecipientIdType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatId => "chat_id",
            Self::OpenId => "open_id",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationRecipient {
    pub receive_id_type: NotificationRecipientIdType,
    pub receive_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NotificationBridgeConfig {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub app_secret: String,
    #[serde(default)]
    pub bot_open_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryStatus {
    Sent,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationDeliveryRecord {
    pub delivery_id: String,
    pub channel: NotificationChannel,
    pub destination: String,
    pub status: NotificationDeliveryStatus,
    #[serde(default)]
    pub provider_message_id: Option<String>,
    #[serde(default)]
    pub recipient: Option<NotificationRecipient>,
    #[serde(default)]
    pub provider_payload: Value,
    #[serde(default)]
    pub delivered_at: Option<String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
}

pub struct NotificationDeliveryService {
    client: Client,
}

impl NotificationDeliveryService {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| format!("failed to build notification delivery client: {error}"))?;
        Ok(Self { client })
    }

    pub fn deliver(
        &self,
        request: &NotificationRequest,
        bridge_provider: Option<&NotificationBridgeConfig>,
        recipient: Option<&NotificationRecipient>,
    ) -> Result<NotificationDeliveryRecord, String> {
        match request.channel {
            NotificationChannel::ImBridge => {
                self.deliver_im_bridge(request, bridge_provider, recipient)
            }
            NotificationChannel::LocalUi => Ok(NotificationDeliveryRecord {
                delivery_id: new_delivery_id(),
                channel: request.channel,
                destination: request.destination.clone(),
                status: NotificationDeliveryStatus::Sent,
                provider_message_id: None,
                recipient: recipient.cloned(),
                provider_payload: json!({
                    "delivery_mode": "local_ui",
                    "request": request,
                }),
                delivered_at: Some(current_timestamp()),
                correlation_id: request.correlation_id.clone(),
            }),
            NotificationChannel::Webhook
            | NotificationChannel::Telegram
            | NotificationChannel::Wecom => Err(format!(
                "notification delivery for channel {:?} 尚未实现",
                request.channel
            )),
        }
    }

    fn deliver_im_bridge(
        &self,
        request: &NotificationRequest,
        bridge_provider: Option<&NotificationBridgeConfig>,
        recipient: Option<&NotificationRecipient>,
    ) -> Result<NotificationDeliveryRecord, String> {
        let bridge_provider = bridge_provider
            .ok_or_else(|| "bridge provider 未配置，无法发送 IM 通知".to_string())?;
        if bridge_provider.app_id.trim().is_empty() || bridge_provider.app_secret.trim().is_empty()
        {
            return Err("bridge provider 凭证不完整，无法发送 IM 通知".to_string());
        }
        let recipient =
            recipient.ok_or_else(|| "通知目的地未映射到可投递的 IM 收件人".to_string())?;
        if recipient.receive_id.trim().is_empty() {
            return Err("通知收件人为空，无法发送 IM 通知".to_string());
        }

        let token = self.fetch_bridge_provider_token(bridge_provider)?;
        let (msg_type, content) = build_im_bridge_message(request)?;
        let response: Value = self
            .client
            .post(format!(
                "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={}",
                recipient.receive_id_type.as_str()
            ))
            .header("Authorization", format!("Bearer {token}"))
            .json(&json!({
                "receive_id": recipient.receive_id,
                "msg_type": msg_type,
                "content": content,
            }))
            .send()
            .map_err(|error| format!("bridge provider send request failed: {error}"))?
            .json()
            .map_err(|error| format!("bridge provider send response parse failed: {error}"))?;
        if response.get("code").and_then(Value::as_i64) != Some(0) {
            let msg = response
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!("bridge provider 发送通知失败: {msg}"));
        }

        let provider_message_id = response
            .pointer("/data/message_id")
            .and_then(Value::as_str)
            .map(|value| value.to_string());
        Ok(NotificationDeliveryRecord {
            delivery_id: new_delivery_id(),
            channel: request.channel,
            destination: request.destination.clone(),
            status: NotificationDeliveryStatus::Sent,
            provider_message_id,
            recipient: Some(recipient.clone()),
            provider_payload: response,
            delivered_at: Some(current_timestamp()),
            correlation_id: request.correlation_id.clone(),
        })
    }

    fn fetch_bridge_provider_token(
        &self,
        bridge_provider: &NotificationBridgeConfig,
    ) -> Result<String, String> {
        let token_resp: Value = self
            .client
            .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .json(&json!({
                "app_id": bridge_provider.app_id,
                "app_secret": bridge_provider.app_secret,
            }))
            .send()
            .map_err(|error| format!("bridge provider token request failed: {error}"))?
            .json()
            .map_err(|error| format!("bridge provider token response parse failed: {error}"))?;
        if token_resp.get("code").and_then(Value::as_i64) != Some(0) {
            let msg = token_resp
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!("bridge provider token request rejected: {msg}"));
        }
        token_resp
            .get("tenant_access_token")
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .ok_or_else(|| {
                "bridge provider token request succeeded but tenant_access_token 缺失".to_string()
            })
    }
}

fn default_payload_format() -> NotificationPayloadFormat {
    NotificationPayloadFormat::PlainText
}

fn build_im_bridge_message(request: &NotificationRequest) -> Result<(String, String), String> {
    match request.payload_format {
        NotificationPayloadFormat::LarkCard => {
            if request.structured_payload.is_null() {
                return Err("interactive notification 缺少 structured_payload".to_string());
            }
            Ok((
                "interactive".to_string(),
                serde_json::to_string(&request.structured_payload).map_err(|error| {
                    format!("failed to encode interactive card payload: {error}")
                })?,
            ))
        }
        NotificationPayloadFormat::Json => Ok((
            "interactive".to_string(),
            serde_json::to_string(&request.structured_payload)
                .map_err(|error| format!("failed to encode JSON payload: {error}"))?,
        )),
        NotificationPayloadFormat::PlainText | NotificationPayloadFormat::Markdown => Ok((
            "text".to_string(),
            serde_json::to_string(&json!({ "text": request.body }))
                .map_err(|error| format!("failed to encode text payload: {error}"))?,
        )),
    }
}

fn new_delivery_id() -> String {
    Uuid::new_v4().as_simple().to_string()
}

fn current_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{
        build_im_bridge_message, NotificationAttachment, NotificationAttachmentKind,
        NotificationChannel, NotificationDeliveryService, NotificationPayloadFormat,
        NotificationRecipient, NotificationRecipientIdType, NotificationRequest,
    };

    #[test]
    fn legacy_feishu_channel_deserializes_as_im_bridge() {
        let request: NotificationRequest = serde_json::from_value(json!({
            "channel": "feishu",
            "destination": "chat-1",
            "title": "AI 分析",
            "body": "检测到人员活动",
            "structured_payload": {"header": {"title": {"content": "Front Door AI 分析"}}}
        }))
        .expect("request");

        assert_eq!(request.channel, NotificationChannel::ImBridge);
        assert_eq!(request.payload_format, NotificationPayloadFormat::PlainText);
    }

    #[test]
    fn request_round_trips_card_payload_and_attachments() {
        let request = NotificationRequest {
            channel: NotificationChannel::ImBridge,
            destination: "home-default".to_string(),
            title: "AI 分析".to_string(),
            body: "检测到人员活动".to_string(),
            payload_format: NotificationPayloadFormat::LarkCard,
            structured_payload: json!({
                "header": {"title": {"content": "Front Door AI 分析"}}
            }),
            attachments: vec![NotificationAttachment {
                kind: NotificationAttachmentKind::Image,
                label: "抓拍图".to_string(),
                mime_type: "image/jpeg".to_string(),
                path: Some(".harbornas/vision/snapshots/front-door.jpg".to_string()),
                url: None,
                metadata: Value::Null,
            }],
            correlation_id: Some("audit-1".to_string()),
        };

        let encoded = serde_json::to_value(&request).expect("encode");
        assert_eq!(encoded["channel"], "im_bridge");
        assert_eq!(encoded["payload_format"], "lark_card");
        assert_eq!(encoded["attachments"][0]["kind"], "image");
    }

    #[test]
    fn card_payload_encodes_as_interactive_message() {
        let request = NotificationRequest {
            channel: NotificationChannel::ImBridge,
            destination: "chat-1".to_string(),
            title: "AI 分析".to_string(),
            body: "检测到人员活动".to_string(),
            payload_format: NotificationPayloadFormat::LarkCard,
            structured_payload: json!({
                "header": {"title": {"content": "Front Door AI 分析"}}
            }),
            attachments: Vec::new(),
            correlation_id: Some("trace-1".to_string()),
        };

        let (msg_type, content) = build_im_bridge_message(&request).expect("message");

        assert_eq!(msg_type, "interactive");
        assert!(content.contains("Front Door AI 分析"));
    }

    #[test]
    fn local_ui_delivery_returns_sent_record_without_network() {
        let service = NotificationDeliveryService::new().expect("service");
        let request = NotificationRequest {
            channel: NotificationChannel::LocalUi,
            destination: "dashboard".to_string(),
            title: "AI 分析".to_string(),
            body: "检测到人员活动".to_string(),
            payload_format: NotificationPayloadFormat::PlainText,
            structured_payload: Value::Null,
            attachments: Vec::new(),
            correlation_id: Some("trace-1".to_string()),
        };

        let record = service
            .deliver(
                &request,
                None,
                Some(&NotificationRecipient {
                    receive_id_type: NotificationRecipientIdType::ChatId,
                    receive_id: "chat-1".to_string(),
                    label: "家庭通知频道".to_string(),
                }),
            )
            .expect("delivery");

        assert_eq!(record.channel, NotificationChannel::LocalUi);
        assert!(record.delivered_at.is_some());
        assert_eq!(record.provider_payload["delivery_mode"], "local_ui");
    }
}
