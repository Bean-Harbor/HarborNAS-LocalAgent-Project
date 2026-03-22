/**
 * HarborBeacon Settings — TypeScript interfaces.
 *
 * Mirror the Python dataclasses in harborbeacon/channels.py and
 * harborbeacon/autonomy.py so the Angular UI speaks the same language
 * as the middleware API.
 */

// ---------------------------------------------------------------------------
// Channel
// ---------------------------------------------------------------------------

export enum Channel {
  Feishu = 'feishu',
  Wecom = 'wecom',
  Telegram = 'telegram',
  Discord = 'discord',
  Dingtalk = 'dingtalk',
  Slack = 'slack',
  Mqtt = 'mqtt',
}

export interface ChannelConfig {
  channel: Channel;
  enabled: boolean;
  webhook_url?: string;
  app_id?: string;
  app_secret?: string;
  bot_token?: string;
  extra: Record<string, unknown>;
}

/** Human-readable metadata per channel (for the UI). */
export interface ChannelMeta {
  channel: Channel;
  label: string;
  icon: string;                // Material icon name
  credentialFields: string[];  // which ChannelConfig fields to show
}

export const CHANNEL_META: ChannelMeta[] = [
  {
    channel: Channel.Feishu,
    label: '飞书 / Lark',
    icon: 'chat',
    credentialFields: ['app_id', 'app_secret', 'webhook_url'],
  },
  {
    channel: Channel.Wecom,
    label: '企业微信 / WeCom',
    icon: 'business',
    credentialFields: ['app_id', 'app_secret', 'webhook_url'],
  },
  {
    channel: Channel.Telegram,
    label: 'Telegram',
    icon: 'send',
    credentialFields: ['bot_token'],
  },
  {
    channel: Channel.Discord,
    label: 'Discord',
    icon: 'headset_mic',
    credentialFields: ['bot_token', 'webhook_url'],
  },
  {
    channel: Channel.Dingtalk,
    label: '钉钉 / DingTalk',
    icon: 'notifications',
    credentialFields: ['app_id', 'app_secret', 'webhook_url'],
  },
  {
    channel: Channel.Slack,
    label: 'Slack',
    icon: 'tag',
    credentialFields: ['bot_token', 'webhook_url'],
  },
  {
    channel: Channel.Mqtt,
    label: 'MQTT (IoT)',
    icon: 'developer_board',
    credentialFields: ['extra.broker', 'extra.port', 'extra.topic'],
  },
];

// ---------------------------------------------------------------------------
// Autonomy
// ---------------------------------------------------------------------------

export enum Autonomy {
  ReadOnly = 'ReadOnly',
  Supervised = 'Supervised',
  Full = 'Full',
}

export interface AutonomyConfig {
  default_level: Autonomy;
  approval_timeout_seconds: number;
  allow_full_for_channels: Channel[];
}

// ---------------------------------------------------------------------------
// Route strategy
// ---------------------------------------------------------------------------

export enum Route {
  MiddlewareApi = 'middleware_api',
  Midcli = 'midcli',
  Browser = 'browser',
  Mcp = 'mcp',
}

export interface RouteStatus {
  route: Route;
  label: string;
  available: boolean;
  priority: number;  // 1 = highest
}

export const DEFAULT_ROUTE_PRIORITY: Route[] = [
  Route.MiddlewareApi,
  Route.Midcli,
  Route.Browser,
  Route.Mcp,
];

// ---------------------------------------------------------------------------
// Connectivity
// ---------------------------------------------------------------------------

export interface ConnectivityResult {
  channel: Channel;
  reachable: boolean;
  latency_ms: number | null;
  error?: string;
  tested_at: string;  // ISO 8601
}

// ---------------------------------------------------------------------------
// Aggregate settings payload
// ---------------------------------------------------------------------------

export interface HarborBeaconSettings {
  channels: ChannelConfig[];
  autonomy: AutonomyConfig;
  route_priority: Route[];
}
