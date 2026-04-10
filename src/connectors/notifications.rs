//! Outbound notification connectors for IM, webhook, and local alerts.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationChannel {
    Feishu,
    Wecom,
    Telegram,
    Webhook,
    LocalUi,
}
