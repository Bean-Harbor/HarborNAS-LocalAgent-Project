import { HarborDeskPageId } from './page-registry';

export type PageKind = 'loading' | 'empty' | 'blocker' | 'success';
export type MetricTone = 'neutral' | 'good' | 'warn' | 'danger';
export type DeliverySurface = 'feishu' | 'weixin';
export type SetupStepState = 'ready' | 'needs-config' | 'read-only' | 'blocked';
export type ReleaseReadinessStatus = 'ready' | 'needs-config' | 'blocked' | 'running' | 'unknown';
export type ReleaseReadinessEndpointState = 'available' | 'empty' | 'error';
export type RagPrivacyLevel = 'strict_local' | 'allow_redacted_cloud' | 'allow_cloud';
export type RagResourceProfile = 'cpu_only' | 'local_gpu' | 'sidecar_gpu' | 'cloud_allowed';

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

export interface ReleaseReadinessChecklistItem {
  id: string;
  label: string;
  status: ReleaseReadinessStatus;
  summary?: string;
  detail?: string;
  owner_lane?: string;
  endpoint?: string;
  deep_link?: string;
  blockers?: string[];
  evidence?: string[];
}

export interface ReleaseReadinessStatusCard {
  id: string;
  label: string;
  value: string;
  status: ReleaseReadinessStatus;
  detail: string;
  endpoint?: string;
  deep_link?: string;
  tone?: MetricTone;
}

export interface ReleaseReadinessDeepLink {
  label: string;
  href: string;
  detail: string;
  endpoint?: string;
}

export interface ReleaseDomainReadiness {
  id: string;
  label?: string;
  status?: ReleaseReadinessStatus;
  current_status?: string;
  next_action?: string;
  action_path?: string;
  recent_validation_at?: string | null;
  generated_at?: string | null;
  checked_at?: string | null;
  endpoint?: string;
  detail?: string;
  evidence?: string[];
  blockers?: string[];
  warnings?: string[];
}

export interface ReleaseDomainReadinessCard {
  id: string;
  label: string;
  status: ReleaseReadinessStatus;
  current_status: string;
  next_action: string;
  action_path: string;
  recent_at?: string | null;
  endpoint?: string;
  detail?: string;
  evidence: string[];
  blockers: string[];
  warnings: string[];
  tone: MetricTone;
}

export interface ReleaseReadinessRunRequest {
  scope?: 'all' | 'release' | 'hardware' | 'harboros' | 'models' | 'im';
  reason?: string;
}

export interface ReleaseReadinessRunResponse {
  run_id?: string;
  job_id?: string;
  status: ReleaseReadinessStatus | 'accepted' | 'queued';
  summary?: string;
  started_at?: string;
  completed_at?: string | null;
  checklist?: ReleaseReadinessChecklistItem[];
  blockers?: string[];
  warnings?: string[];
}

export interface ReleaseReadinessResponse {
  status: ReleaseReadinessStatus;
  summary?: string;
  generated_at?: string;
  checked_at?: string;
  domains?: ReleaseDomainReadiness[];
  domain_cards?: ReleaseDomainReadiness[];
  checklist?: ReleaseReadinessChecklistItem[];
  status_cards?: ReleaseReadinessStatusCard[];
  deep_links?: ReleaseReadinessDeepLink[];
  blockers?: string[];
  warnings?: string[];
  last_run?: ReleaseReadinessRunResponse | null;
}

export interface ReleaseReadinessHistoryEntry {
  run_id?: string;
  status: ReleaseReadinessStatus;
  summary?: string;
  generated_at?: string | null;
  checked_at?: string | null;
  started_at?: string | null;
  completed_at?: string | null;
  action_path?: string;
  actor?: string;
  domains?: ReleaseDomainReadiness[];
  blockers?: string[];
  warnings?: string[];
}

export interface ReleaseReadinessHistoryResponse {
  generated_at?: string | null;
  history: ReleaseReadinessHistoryEntry[];
}

export interface RagReadinessResponse {
  status: ReleaseReadinessStatus;
  summary?: string;
  generated_at?: string | null;
  checked_at?: string | null;
  source_roots?: RagReadinessComponent;
  index_directory?: RagReadinessComponent;
  embedding_model?: RagReadinessComponent;
  model_readiness?: RagModelReadinessCard[];
  resource_profiles?: RagResourceProfileStatus[];
  capability_profiles?: RagCapabilityReadinessCard[];
  privacy_policy?: RagReadinessComponent;
  media_parser?: RagReadinessComponent;
  storage_writable?: RagReadinessComponent;
  index_jobs?: KnowledgeIndexJobRecord[];
  evidence?: string[];
  next_action?: string;
  action_path?: string;
  checklist?: ReleaseReadinessChecklistItem[];
  blockers?: string[];
  warnings?: string[];
}

export interface RagReadinessComponent {
  status: ReleaseReadinessStatus;
  summary: string;
  detail: string;
  evidence: string[];
}

export interface RagModelReadinessCard {
  model_kind: string;
  label: string;
  status: ReleaseReadinessStatus;
  endpoint_id?: string | null;
  endpoint_kind?: string | null;
  provider_key?: string | null;
  model_name?: string | null;
  detail: string;
  blocker?: string | null;
}

export interface RagResourceProfileStatus {
  profile: RagResourceProfile;
  label: string;
  status: ReleaseReadinessStatus;
  detail: string;
  blockers: string[];
  warnings: string[];
}

export interface RagCapabilityReadinessCard {
  capability_id: string;
  label: string;
  status: ReleaseReadinessStatus | string;
  summary: string;
  blockers: string[];
  warnings: string[];
  evidence: string[];
}

export interface HardwareReadinessDevice {
  device_id: string;
  label?: string;
  kind?: string;
  status: ReleaseReadinessStatus;
  detail?: string;
  evidence?: string[];
  blockers?: string[];
}

export interface HardwareReadinessResponse {
  status: ReleaseReadinessStatus;
  summary?: string;
  checked_at?: string;
  devices?: HardwareReadinessDevice[];
  checklist?: ReleaseReadinessChecklistItem[];
  status_cards?: ReleaseReadinessStatusCard[];
  blockers?: string[];
  warnings?: string[];
}

export interface HarborOsServiceStatus {
  service_id: string;
  label?: string;
  status: ReleaseReadinessStatus;
  detail?: string;
}

export interface HarborOsStatusResponse {
  status: ReleaseReadinessStatus;
  summary?: string;
  checked_at?: string;
  version?: string;
  webui_url?: string;
  admin_origin?: string;
  services?: HarborOsServiceStatus[];
  blockers?: string[];
  warnings?: string[];
}

export interface HarborOsImCapability {
  capability_id: string;
  label?: string;
  status: ReleaseReadinessStatus;
  surface?: string;
  detail?: string;
  endpoint?: string;
}

export interface HarborOsImCapabilityMapResponse {
  status: ReleaseReadinessStatus;
  summary?: string;
  checked_at?: string;
  capabilities?: HarborOsImCapability[];
  blockers?: string[];
  warnings?: string[];
}

export interface LocalModelCatalogItem {
  model_id: string;
  label?: string;
  display_name?: string;
  provider?: string;
  provider_key?: string;
  model_kind?: string;
  recommended_hardware?: string;
  status: ReleaseReadinessStatus | string;
  size_bytes?: number | null;
  installed?: boolean;
  local_path?: string | null;
  download_job_id?: string | null;
  download_size_hint?: string;
  detail?: string;
  source_kind?: string;
  repo_id?: string | null;
  revision?: string | null;
  file_policy?: string;
  runtime_profiles?: string[];
  expected_capabilities?: string[];
  acceptance_note?: string | null;
  evidence?: string[];
}

export interface LocalModelCatalogResponse {
  status?: ReleaseReadinessStatus;
  generated_at?: string | null;
  checked_at?: string;
  cache_roots?: string[];
  models: LocalModelCatalogItem[];
  download_jobs?: LocalModelDownloadStatusResponse[];
  downloads?: LocalModelDownloadStatusResponse[];
  blockers?: string[];
  warnings?: string[];
}

export interface LocalModelDownloadStatusResponse {
  job_id: string;
  model_id?: string;
  display_name?: string;
  provider_key?: string;
  status: ReleaseReadinessStatus | 'queued' | 'running' | 'downloading' | 'completed' | 'failed' | 'canceled' | 'cancelled' | string;
  requested_at?: string;
  target_path?: string | null;
  progress_percent?: number | null;
  bytes_downloaded?: number | null;
  total_bytes?: number | null;
  error_message?: string | null;
  started_at?: string;
  updated_at?: string;
  completed_at?: string | null;
  message?: string;
  metadata?: Record<string, unknown>;
}

export interface LocalModelDownloadsResponse {
  status?: ReleaseReadinessStatus;
  generated_at?: string | null;
  checked_at?: string | null;
  jobs?: LocalModelDownloadStatusResponse[];
  downloads: LocalModelDownloadStatusResponse[];
  blockers?: string[];
  warnings?: string[];
}

export interface StartLocalModelDownloadRequest {
  model_id: string;
  display_name?: string;
  provider_key?: string;
  target_path?: string | null;
  metadata?: Record<string, unknown>;
}

export interface LocalModelDownloadJobResponse extends LocalModelDownloadStatusResponse {
  job?: LocalModelDownloadStatusResponse;
}

export interface ReleaseReadinessEndpointStatus {
  endpoint: string;
  state: ReleaseReadinessEndpointState;
  detail: string;
}

export interface ReleaseReadinessPanel {
  status: ReleaseReadinessStatus;
  summary: string;
  checked_at?: string | null;
  checklist: ReleaseReadinessChecklistItem[];
  statusCards: ReleaseReadinessStatusCard[];
  domainCards: ReleaseDomainReadinessCard[];
  deepLinks: ReleaseReadinessDeepLink[];
  history: ReleaseReadinessHistoryEntry[];
  blockers: string[];
  warnings: string[];
  endpointStates: ReleaseReadinessEndpointStatus[];
  empty: boolean;
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

export interface CameraStreamRef {
  transport?: string;
  url?: string;
  requires_auth?: boolean;
}

export interface CameraCapabilities {
  snapshot?: boolean;
  stream?: boolean;
  ptz?: boolean;
  audio?: boolean;
}

export interface CameraDevice {
  device_id: string;
  name: string;
  room?: string | null;
  status?: string;
  kind?: string;
  vendor?: string | null;
  model?: string | null;
  ip_address?: string | null;
  discovery_source?: string;
  primary_stream?: CameraStreamRef;
  snapshot_url?: string | null;
  capabilities?: CameraCapabilities;
  provider?: string;
  profile?: CameraProfile;
  metadata?: Record<string, unknown>;
}

export interface DeviceCredentialStatus {
  device_id: string;
  configured: boolean;
  redacted: boolean;
  username?: string | null;
  rtsp_port?: number | null;
  path_count: number;
  source: string;
  updated_at?: string | null;
  last_verified_at?: string | null;
}

export interface DeviceEvidenceResult {
  id?: string;
  kind: 'rtsp_check' | 'snapshot' | 'share_link' | 'credential_status' | string;
  status?: ReleaseReadinessStatus | 'passed' | 'failed' | 'pending' | 'skipped' | string;
  summary?: string;
  detail?: string;
  checked_at?: string | null;
  generated_at?: string | null;
  action_path?: string;
  endpoint?: string;
  artifact_path?: string | null;
  share_link_id?: string | null;
  redacted?: boolean;
  expires_at?: string | null;
  error_message?: string | null;
}

export interface DeviceEvidenceResponse {
  device_id: string;
  status?: ReleaseReadinessStatus;
  summary?: string;
  generated_at?: string | null;
  checked_at?: string | null;
  next_action?: string;
  action_path?: string;
  results?: DeviceEvidenceResult[];
  rtsp_check?: DeviceEvidenceResult;
  snapshot?: DeviceEvidenceResult;
  share_link?: DeviceEvidenceResult;
  credential_status?: DeviceEvidenceResult;
  blockers?: string[];
  warnings?: string[];
}

export interface DeviceEvidenceField {
  key: 'rtsp_check' | 'snapshot' | 'share_link' | 'credential_status';
  label: string;
  status: string;
  detail: string;
  tone: MetricTone;
  checked_at?: string | null;
  action_path?: string;
  endpoint?: string;
}

export interface DeviceEvidencePanel {
  device_id: string;
  endpoint: string;
  state: ReleaseReadinessEndpointState;
  summary: string;
  generated_at?: string | null;
  fields: DeviceEvidenceField[];
  blockers: string[];
  warnings: string[];
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
  device_credential_statuses?: DeviceCredentialStatus[];
}

export interface KnowledgeSourceRoot {
  root_id: string;
  label: string;
  path: string;
  enabled: boolean;
  include: string[];
  exclude: string[];
  last_indexed_at?: string | null;
}

export interface KnowledgeSettings {
  source_roots: KnowledgeSourceRoot[];
  index_root: string;
  privacy_level: RagPrivacyLevel;
  default_resource_profile: RagResourceProfile;
}

export interface KnowledgeIndexRootStatus {
  root_id: string;
  label: string;
  path: string;
  enabled: boolean;
  exists: boolean;
  last_indexed_at?: string | null;
  status: string;
  detail: string;
}

export interface KnowledgeIndexStatusResponse {
  generated_at: string;
  status: ReleaseReadinessStatus | string;
  settings: KnowledgeSettings;
  index_root_exists: boolean;
  index_root_writable: boolean;
  manifest_count: number;
  manifest_entry_count: number;
  document_count?: number;
  image_count?: number;
  audio_count?: number;
  video_count?: number;
  content_indexed_image_count?: number;
  vlm_indexed_image_count?: number;
  ocr_indexed_image_count?: number;
  image_content_missing_count?: number;
  image_text_source_counts?: Record<string, number>;
  embedding_cache_count: number;
  embedding_entry_count: number;
  storage_usage_bytes: number;
  last_indexed_at?: string | null;
  source_roots: KnowledgeIndexRootStatus[];
  blockers: string[];
}

export interface KnowledgeIndexRunResponse {
  generated_at: string;
  job_ids?: string[];
  status: string;
  index_root: string;
  root_count: number;
  indexed_roots: KnowledgeIndexRootStatus[];
  errors: string[];
}

export interface KnowledgeIndexJobRecord {
  job_id: string;
  source_root_id: string;
  source_root_label: string;
  source_root_path: string;
  modalities: string[];
  status: string;
  progress_percent?: number | null;
  requested_at?: string | null;
  started_at?: string | null;
  completed_at?: string | null;
  error_message?: string | null;
  retry_count: number;
  checkpoint: Record<string, unknown>;
  resource_profile: RagResourceProfile;
  cancel_requested: boolean;
}

export interface KnowledgeIndexJobsResponse {
  generated_at: string;
  jobs: KnowledgeIndexJobRecord[];
}

export interface KnowledgeSearchRequestPayload {
  query: string;
  limit?: number;
  include_documents?: boolean;
  include_images?: boolean;
  include_videos?: boolean;
}

export interface KnowledgeSearchCitation {
  title: string;
  path: string;
  modality: string;
  chunk_id?: string | null;
  line_start?: number | null;
  line_end?: number | null;
  matched_terms: string[];
  preview?: string | null;
  score: number;
  lexical_score?: number | null;
  embedding_score?: number | null;
  hybrid_score?: number | null;
  provenance?: string | null;
  source_path?: string | null;
}

export interface KnowledgeSearchReplyPack {
  summary: string;
  citations: KnowledgeSearchCitation[];
}

export interface KnowledgeSearchHit {
  modality: string;
  path: string;
  title: string;
  score: number;
  lexical_score?: number | null;
  embedding_score?: number | null;
  hybrid_score?: number | null;
  chunk_id?: string | null;
  line_start?: number | null;
  line_end?: number | null;
  snippet?: string | null;
  matched_terms: string[];
  provenance?: string | null;
  source_path?: string | null;
  content_source_kinds: string[];
  content_indexed: boolean;
  filename_match_used: boolean;
  content_match_used: boolean;
}

export interface KnowledgeSearchResponse {
  query: string;
  roots: string[];
  total_matches: number;
  documents: KnowledgeSearchHit[];
  images: KnowledgeSearchHit[];
  videos: KnowledgeSearchHit[];
  reply_pack: KnowledgeSearchReplyPack;
  supported_modalities: string[];
  pending_modalities: string[];
  status: string;
  degraded: boolean;
  degraded_reason?: string | null;
  blockers: string[];
  warnings: string[];
  source_scope: string[];
  privacy_level: RagPrivacyLevel | string;
  resource_profile: RagResourceProfile | string;
  empty_reason?: string | null;
  empty_guidance?: string | null;
}

export interface FileBrowseEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size_bytes?: number | null;
}

export interface FilesBrowseResponse {
  path: string;
  parent?: string | null;
  readonly: boolean;
  allowed_roots: string[];
  entries: FileBrowseEntry[];
}

export interface ShareLinkSummary {
  share_link_id: string;
  media_session_id: string;
  device_id: string;
  device_name: string;
  opened_by_user_id?: string | null;
  access_scope: string;
  session_status: string;
  status: string;
  expires_at?: string | null;
  revoked_at?: string | null;
  started_at?: string | null;
  ended_at?: string | null;
  can_revoke: boolean;
}

export interface DeviceCredentialsPayload {
  username?: string | null;
  password?: string | null;
  rtsp_port?: number | null;
  rtsp_paths?: string[];
}

export interface RtspCheckPayload extends DeviceCredentialsPayload {}

export interface RtspCheckResult {
  device_id: string;
  reachable: boolean;
  stream_url?: string | null;
  transport: string;
  requires_auth: boolean;
  capabilities: CameraCapabilities;
  error_message?: string | null;
  checked_at: string;
}

export interface DeviceValidationRunRequest {
  scope?: 'all' | 'rtsp' | 'snapshot' | 'share-link' | 'credentials';
  reason?: string;
}

export interface DeviceValidationRunResponse {
  run_id?: string;
  device_id: string;
  status: ReleaseReadinessStatus | 'accepted' | 'queued';
  summary?: string;
  started_at?: string;
  completed_at?: string | null;
  evidence?: DeviceEvidenceResponse;
  blockers?: string[];
  warnings?: string[];
}

export interface ManualDevicePayload {
  name: string;
  room?: string | null;
  ip: string;
  path?: string | null;
  snapshot_url?: string | null;
  username?: string | null;
  password?: string | null;
  port?: number | null;
}

export interface DiscoveryScanPayload {
  cidr?: string | null;
  protocol?: string | null;
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
    configured?: boolean;
    connected?: boolean;
    status?: string;
    rehearsal_ready?: boolean;
    blocker_category?: string;
    ingress_blocker_category?: string;
    poll?: Record<string, unknown>;
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

export type FeatureAvailabilityStatus = 'available' | 'degraded' | 'blocked' | 'not_configured';

export interface FeatureAvailabilityItem {
  feature_id: string;
  label: string;
  owner_lane: string;
  status: FeatureAvailabilityStatus;
  source_of_truth: string;
  current_option: string;
  fallback_order: string[];
  blocker: string;
  evidence: string[];
}

export interface FeatureAvailabilityGroup {
  group_id: string;
  label: string;
  items: FeatureAvailabilityItem[];
}

export interface FeatureAvailabilityResponse {
  groups: FeatureAvailabilityGroup[];
}

export interface ModelEndpointTestResult {
  ok: boolean;
  status: string;
  summary: string;
  endpoint: ModelEndpointRecord;
  details?: Record<string, unknown>;
}

export interface RuntimeAlignmentSummary {
  status: string;
  detail: string;
  tone: MetricTone;
  rows: DeskRow[];
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
  devices?: CameraDevice[];
  defaults?: AdminDefaults;
  deviceCredentialStatuses?: DeviceCredentialStatus[];
  deviceEvidence?: Record<string, DeviceEvidencePanel>;
  shareLinks?: ShareLinkSummary[];
  modelEndpoints?: ModelEndpointRecord[];
  modelPolicies?: ModelRoutePolicyRecord[];
  knowledgeSettings?: KnowledgeSettings;
  knowledgeIndexStatus?: KnowledgeIndexStatusResponse;
  knowledgeIndexJobs?: KnowledgeIndexJobRecord[];
  localModelCatalog?: LocalModelCatalogResponse;
  localModelDownloads?: LocalModelDownloadStatusResponse[];
  ragReadiness?: RagReadinessResponse;
  featureGroups?: FeatureAvailabilityGroup[];
  runtimeAlignment?: RuntimeAlignmentSummary;
  releaseReadiness?: ReleaseReadinessPanel;
}

export interface PageState<T> {
  kind: PageKind;
  detail: string;
  data: T;
}
