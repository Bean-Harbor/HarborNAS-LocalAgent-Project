import { HarborDeskPageId } from './page-registry';

export type PageKind = 'loading' | 'empty' | 'blocker' | 'success';
export type MetricTone = 'neutral' | 'good' | 'warn' | 'danger';
export type DeliverySurface = 'feishu' | 'weixin';
export type SetupStepState = 'ready' | 'needs-config' | 'read-only' | 'blocked';

export interface MetricCard {
  label: string;
  value: string;
  detail: string;
  tone: MetricTone;
}

export interface DeskRow {
  title: string;
  subtitle?: string;
  meta?: string[];
  tone?: MetricTone;
}

export interface SetupFlowStep {
  title: string;
  state: SetupStepState;
  summary: string;
  detail: string;
  bullets?: string[];
}

export interface SetupFlowSection {
  title: string;
  summary: string;
  steps: SetupFlowStep[];
}

export interface AccessMemberSummary {
  user_id: string;
  display_name: string;
  role_kind: string;
  membership_status: string;
  source: string;
  open_id?: string | null;
  chat_id?: string | null;
  can_edit: boolean;
  is_owner: boolean;
  proactive_delivery_surface: string;
  proactive_delivery_default: boolean;
  binding_availability: string;
  binding_available: boolean;
  binding_availability_note: string;
  recent_interactive_surface?: string | null;
}

export interface WorkspaceSummary {
  workspace_id: string;
  display_name: string;
  workspace_type: string;
  status: string;
  timezone: string;
  locale: string;
  owner_user_id: string;
  member_count: number;
  active_member_count: number;
  identity_binding_count: number;
  permission_rule_count: number;
  provider_account_count: number;
  credential_count: number;
  current_principal_user_id?: string | null;
  current_principal_display_name?: string | null;
  current_principal_auth_source?: string | null;
}

export interface MemberRoleSummary {
  role_kind: string;
  member_count: number;
  active_member_count: number;
}

export interface IdentityBindingSummary {
  identity_id: string;
  user_id: string;
  display_name: string;
  provider_key: string;
  open_id: string;
  union_id?: string | null;
  chat_id?: string | null;
  role_kind: string;
  membership_status: string;
  can_edit: boolean;
  is_owner: boolean;
  proactive_delivery_surface: string;
  binding_availability: string;
  binding_available: boolean;
  binding_availability_note: string;
  recent_interactive_surface?: string | null;
}

export interface AccessGovernanceSummary {
  permission_rule_count: number;
  owner_count: number;
  member_count: number;
  active_member_count: number;
  role_policies: Array<{ role_kind: string; permission_count: number; can_manage: boolean }>;
}

export interface BridgeProviderCapabilities {
  reply: boolean;
  update: boolean;
  attachments: boolean;
}

export interface BridgeProviderConfig {
  configured: boolean;
  connected: boolean;
  platform: string;
  gateway_base_url: string;
  app_id?: string;
  app_secret?: string;
  app_name?: string;
  bot_open_id?: string;
  status: string;
  last_checked_at: string;
  capabilities: BridgeProviderCapabilities;
}

export interface GatewayStatusSummary {
  binding_channel: string;
  binding_status: string;
  binding_metric: string;
  binding_bound_user?: string | null;
  manage_url: string;
  setup_url: string;
  static_setup_url: string;
  bridge_provider: BridgeProviderConfig;
}

export interface DeliveryPolicySummary {
  interactive_reply: string;
  proactive_delivery: string;
}

export interface NotificationTargetRecord {
  target_id: string;
  label: string;
  route_key: string;
  platform_hint: string;
  is_default: boolean;
}

export interface AccountManagementSnapshot {
  workspace: WorkspaceSummary;
  member_role_counts: MemberRoleSummary[];
  identity_bindings: IdentityBindingSummary[];
  access_governance: AccessGovernanceSummary;
  gateway: GatewayStatusSummary;
  notification_targets: NotificationTargetRecord[];
  delivery_policy: DeliveryPolicySummary;
}

export interface CameraProfile {
  transport?: string;
  rtsp_url?: string;
  snapshot_url?: string | null;
  path_candidates?: string[];
}

export interface CameraDevice {
  device_id: string;
  name: string;
  room: string;
  status?: string;
  provider?: string;
  profile?: CameraProfile;
  metadata?: Record<string, unknown>;
}

export interface AdminBindingState {
  channel: string;
  status: string;
  session_code: string;
  setup_url: string;
  static_setup_url: string;
  metric: string;
  bound_user?: string | null;
}

export interface AdminDefaults {
  cidr: string;
  discovery: string;
  recording: string;
  capture: string;
  ai: string;
  notification_channel: string;
  rtsp_username: string;
  rtsp_password?: string;
  rtsp_port?: number | null;
  rtsp_paths: string[];
  selected_camera_device_id?: string | null;
  capture_subdirectory?: string | null;
  clip_length_seconds?: number | null;
  keyframe_count?: number | null;
  keyframe_interval_seconds?: number | null;
}

export interface AdminStateResponse {
  binding: AdminBindingState;
  defaults: AdminDefaults;
  bridge_provider: BridgeProviderConfig;
  delivery_policy: DeliveryPolicySummary;
  writable_root?: string;
  current_principal_user_id?: string;
  current_principal_display_name?: string;
  devices: CameraDevice[];
  account_management: AccountManagementSnapshot;
}

export interface GatewayPlatformStatus {
  platform: string;
  enabled?: boolean;
  connected?: boolean;
  display_name?: string;
  capabilities?: BridgeProviderCapabilities;
}

export interface GatewayStatusResponse {
  platforms?: GatewayPlatformStatus[];
  configured?: boolean;
  connected?: boolean;
  platform?: string;
  status?: string;
  manage_url?: string;
  gateway_base_url?: string;
  last_checked_at?: string;
  parity_ready?: boolean;
  feishu?: { rehearsal_ready?: boolean };
  weixin?: {
    rehearsal_ready?: boolean;
    blocker_category?: string;
    ingress_observability?: Record<string, unknown>;
    delivery_observability?: Record<string, unknown>;
  };
  weixin_blocker_category?: string;
  ingress_observability?: Record<string, unknown>;
  delivery_observability?: Record<string, unknown>;
}

export interface ApprovalTicket {
  approval_id?: string;
  status?: string;
  created_at?: string;
}

export interface TaskApprovalSummary {
  approval_ticket: ApprovalTicket;
  source_channel: string;
  surface: string;
  conversation_id: string;
  user_id: string;
  session_id: string;
  domain: string;
  action: string;
  intent_text: string;
  autonomy_level: string;
  risk_level: string;
}

export interface ModelEndpointRecord {
  model_endpoint_id: string;
  workspace_id?: string | null;
  provider_account_id?: string | null;
  model_kind: string;
  endpoint_kind: string;
  provider_key: string;
  model_name: string;
  capability_tags: string[];
  cost_policy: Record<string, unknown>;
  status: string;
  metadata: Record<string, unknown>;
}

export interface ModelRoutePolicyRecord {
  route_policy_id: string;
  workspace_id: string;
  domain_scope: string;
  modality: string;
  privacy_level: string;
  local_preferred: boolean;
  max_cost_per_run?: number | null;
  fallback_order: string[];
  status: string;
  metadata: Record<string, unknown>;
}

export interface ModelEndpointsResponse {
  endpoints: ModelEndpointRecord[];
}

export interface ModelPoliciesResponse {
  route_policies: ModelRoutePolicyRecord[];
}

export interface ModelEndpointTestResult {
  ok: boolean;
  status: string;
  summary: string;
  endpoint: ModelEndpointRecord;
  details?: Record<string, unknown>;
}

export interface DeskPageModel {
  pageId: HarborDeskPageId;
  title: string;
  eyebrow: string;
  summary: string;
  endpoint: string;
  outputDirectory: string;
  metrics: MetricCard[];
  setupFlow?: SetupFlowSection;
  highlights: string[];
  blockers: string[];
  emptyNote: string;
  nextStep: string;
  detailRows?: DeskRow[];
  members?: AccessMemberSummary[];
  notificationTargets?: NotificationTargetRecord[];
  modelEndpoints?: ModelEndpointRecord[];
  modelPolicies?: ModelRoutePolicyRecord[];
}

export interface PageState<T> {
  kind: PageKind;
  detail: string;
  data: T;
}
