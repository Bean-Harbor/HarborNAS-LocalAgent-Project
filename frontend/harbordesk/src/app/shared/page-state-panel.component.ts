import { NgClass } from '@angular/common';
import { Component, EventEmitter, Input, Output } from '@angular/core';
import { FormsModule } from '@angular/forms';

import {
  DeviceCredentialsPayload,
  DeviceEvidencePanel,
  DeliverySurface,
  DeskPageModel,
  DiscoveryScanPayload,
  FilesBrowseResponse,
  FeatureAvailabilityStatus,
  KnowledgeSettings,
  KnowledgeSourceRoot,
  ManualDevicePayload,
  MetricTone,
  ModelEndpointTestResult,
  PageState,
  RagPrivacyLevel,
  RagResourceProfile,
  ReleaseReadinessStatus,
  RtspCheckPayload,
  RtspCheckResult,
  SetupStepState
} from '../core/admin-api.types';
import { uiText } from '../core/ui-locale';

@Component({
  selector: 'hd-page-state-panel',
  standalone: true,
  imports: [NgClass, FormsModule],
  templateUrl: './page-state-panel.component.html',
  styleUrl: './page-state-panel.component.css'
})
export class PageStatePanelComponent {
  @Input() state: PageState<DeskPageModel> | null = null;
  @Input() savingMemberId: string | null = null;
  @Input() saveError: string | null = null;
  @Input() saveSuccess: string | null = null;
  @Input() testingEndpointId: string | null = null;
  @Input() endpointTestResults: Record<string, ModelEndpointTestResult> = {};
  @Input() savingTargetId: string | null = null;
  @Input() deletingTargetId: string | null = null;
  @Input() deviceActionBusyKey: string | null = null;
  @Input() deviceActionResults: Record<string, string> = {};
  @Input() rtspCheckResults: Record<string, RtspCheckResult> = {};
  @Input() releaseReadinessBusy = false;
  @Input() knowledgeIndexBusy = false;
  @Input() knowledgeIndexJobBusyId: string | null = null;
  @Input() filesBrowse: FilesBrowseResponse | null = null;

  @Output() readonly defaultDeliverySurfaceChange = new EventEmitter<{
    userId: string;
    surface: DeliverySurface;
  }>();
  @Output() readonly notificationTargetDefaultChange = new EventEmitter<string>();
  @Output() readonly notificationTargetDelete = new EventEmitter<string>();
  @Output() readonly endpointTestRequested = new EventEmitter<string>();
  @Output() readonly deviceScanRequested = new EventEmitter<DiscoveryScanPayload>();
  @Output() readonly manualDeviceAddRequested = new EventEmitter<ManualDevicePayload>();
  @Output() readonly defaultCameraChange = new EventEmitter<string>();
  @Output() readonly deviceCredentialsSave = new EventEmitter<{
    deviceId: string;
    payload: DeviceCredentialsPayload;
  }>();
  @Output() readonly deviceRtspCheck = new EventEmitter<{
    deviceId: string;
    payload: RtspCheckPayload;
  }>();
  @Output() readonly cameraSnapshotRequested = new EventEmitter<string>();
  @Output() readonly cameraShareLinkCreate = new EventEmitter<string>();
  @Output() readonly deviceValidationRun = new EventEmitter<string>();
  @Output() readonly shareLinkRevoke = new EventEmitter<string>();
  @Output() readonly releaseReadinessRunRequested = new EventEmitter<void>();
  @Output() readonly knowledgeSettingsSave = new EventEmitter<KnowledgeSettings>();
  @Output() readonly knowledgeIndexRunRequested = new EventEmitter<void>();
  @Output() readonly knowledgeIndexJobCancelRequested = new EventEmitter<string>();
  @Output() readonly filesBrowseRequested = new EventEmitter<string | null>();

  protected scanForm: Required<DiscoveryScanPayload> = {
    cidr: '',
    protocol: ''
  };
  protected manualForm: ManualDevicePayload = {
    name: '',
    room: '',
    ip: '',
    path: '',
    snapshot_url: '',
    username: '',
    password: '',
    port: null
  };
  protected deviceCredentialForms: Record<string, DeviceCredentialsPayload & { rtsp_paths_text?: string }> = {};
  protected knowledgeIndexRoot = '';
  protected knowledgeSourceRootsText = '';
  protected knowledgeSourceRootEnabled: Record<string, boolean> = {};
  protected knowledgePrivacyLevel: RagPrivacyLevel = 'strict_local';
  protected knowledgeResourceProfile: RagResourceProfile = 'cpu_only';
  protected readonly knowledgePrivacyOptions: Array<{ value: RagPrivacyLevel; label: string; detail: string }> = [
    {
      value: 'strict_local',
      label: 'strict_local',
      detail: 'Local-only retrieval and inference.'
    },
    {
      value: 'allow_redacted_cloud',
      label: 'allow_redacted_cloud',
      detail: 'Cloud calls require redaction and audit evidence.'
    },
    {
      value: 'allow_cloud',
      label: 'allow_cloud',
      detail: 'Cloud calls are policy-allowed and still audited.'
    }
  ];
  protected readonly knowledgeResourceOptions: Array<{ value: RagResourceProfile; label: string; detail: string }> = [
    {
      value: 'cpu_only',
      label: 'cpu_only',
      detail: 'Safe default; heavy media jobs may be slow.'
    },
    {
      value: 'local_gpu',
      label: 'local_gpu',
      detail: 'Requires local GPU readiness.'
    },
    {
      value: 'sidecar_gpu',
      label: 'sidecar_gpu',
      detail: 'Requires active sidecar model endpoints.'
    },
    {
      value: 'cloud_allowed',
      label: 'cloud_allowed',
      detail: 'Requires cloud policy and endpoint readiness.'
    }
  ];
  private knowledgeFormKey = '';

  protected toneClass(tone: MetricTone): string {
    return `tone-${tone}`;
  }

  protected text(english: string, chinese: string): string {
    return uiText(english, chinese);
  }

  protected setupToneClass(state: SetupStepState): string {
    switch (state) {
      case 'ready':
        return 'tone-good';
      case 'blocked':
        return 'tone-danger';
      case 'needs-config':
      case 'read-only':
      default:
        return 'tone-warn';
    }
  }

  protected readinessToneClass(status: ReleaseReadinessStatus | string | undefined | null): string {
    switch (status) {
      case 'ready':
        return 'tone-good';
      case 'blocked':
        return 'tone-danger';
      case 'running':
      case 'needs-config':
        return 'tone-warn';
      case 'unknown':
      default:
        return 'tone-neutral';
    }
  }

  protected requestDefaultSurfaceChange(userId: string, surface: string): void {
    if (surface !== 'feishu' && surface !== 'weixin') {
      return;
    }
    this.defaultDeliverySurfaceChange.emit({
      userId,
      surface
    });
  }

  protected requestEndpointTest(modelEndpointId: string): void {
    this.endpointTestRequested.emit(modelEndpointId);
  }

  protected requestNotificationTargetDefaultChange(targetId: string): void {
    this.notificationTargetDefaultChange.emit(targetId);
  }

  protected requestNotificationTargetDelete(targetId: string): void {
    this.notificationTargetDelete.emit(targetId);
  }

  protected requestDeviceScan(): void {
    this.deviceScanRequested.emit({
      cidr: this.scanForm.cidr || null,
      protocol: this.scanForm.protocol || null
    });
  }

  protected requestManualDeviceAdd(): void {
    this.manualDeviceAddRequested.emit({ ...this.manualForm });
  }

  protected requestDefaultCameraChange(deviceId: string): void {
    this.defaultCameraChange.emit(deviceId);
  }

  protected credentialForm(deviceId: string): DeviceCredentialsPayload & { rtsp_paths_text?: string } {
    if (!this.deviceCredentialForms[deviceId]) {
      this.deviceCredentialForms[deviceId] = {
        username: '',
        password: '',
        rtsp_port: null,
        rtsp_paths_text: ''
      };
    }
    return this.deviceCredentialForms[deviceId];
  }

  protected requestDeviceCredentialsSave(deviceId: string): void {
    const form = this.credentialForm(deviceId);
    this.deviceCredentialsSave.emit({
      deviceId,
      payload: {
        username: form.username || null,
        password: form.password || null,
        rtsp_port: form.rtsp_port || null,
        rtsp_paths: this.pathList(form.rtsp_paths_text)
      }
    });
  }

  protected requestDeviceRtspCheck(deviceId: string): void {
    const form = this.credentialForm(deviceId);
    this.deviceRtspCheck.emit({
      deviceId,
      payload: {
        username: form.username || null,
        password: form.password || null,
        rtsp_port: form.rtsp_port || null,
        rtsp_paths: this.pathList(form.rtsp_paths_text)
      }
    });
  }

  protected requestCameraSnapshot(deviceId: string): void {
    this.cameraSnapshotRequested.emit(deviceId);
  }

  protected requestCameraShareLink(deviceId: string): void {
    this.cameraShareLinkCreate.emit(deviceId);
  }

  protected requestDeviceValidation(deviceId: string): void {
    this.deviceValidationRun.emit(deviceId);
  }

  protected requestShareLinkRevoke(shareLinkId: string): void {
    this.shareLinkRevoke.emit(shareLinkId);
  }

  protected requestReleaseReadinessRun(): void {
    this.releaseReadinessRunRequested.emit();
  }

  protected hydrateKnowledgeForm(settings: KnowledgeSettings | undefined): boolean {
    if (!settings) {
      return false;
    }
    const key = JSON.stringify(settings);
    if (key !== this.knowledgeFormKey) {
      this.knowledgeFormKey = key;
      this.knowledgeIndexRoot = settings.index_root ?? '';
      this.knowledgePrivacyLevel = settings.privacy_level ?? 'strict_local';
      this.knowledgeResourceProfile = settings.default_resource_profile ?? 'cpu_only';
      this.knowledgeSourceRootEnabled = Object.fromEntries(
        (settings.source_roots ?? []).map((root) => [root.path, root.enabled])
      );
      this.knowledgeSourceRootsText = (settings.source_roots ?? [])
        .map((root) => root.path)
        .filter(Boolean)
        .join('\n');
    }
    return true;
  }

  protected requestKnowledgeSettingsSave(settings: KnowledgeSettings | undefined): void {
    if (!settings) {
      return;
    }
    const existingByPath = new Map((settings.source_roots ?? []).map((root) => [root.path, root]));
    const sourceRoots = this.pathListByLine(this.knowledgeSourceRootsText).map((path, index) => {
      const existing = existingByPath.get(path);
      const enabled = this.knowledgeSourceRootEnabled[path] ?? existing?.enabled ?? true;
      return {
        root_id: existing?.root_id || `knowledge-root-${index + 1}`,
        label: existing?.label || this.pathLabel(path),
        path,
        enabled,
        include: existing?.include ?? [],
        exclude: existing?.exclude ?? [],
        last_indexed_at: existing?.last_indexed_at ?? null
      } satisfies KnowledgeSourceRoot;
    });
    this.knowledgeSettingsSave.emit({
      source_roots: sourceRoots,
      index_root: this.knowledgeIndexRoot.trim(),
      privacy_level: this.knowledgePrivacyLevel,
      default_resource_profile: this.knowledgeResourceProfile
    });
  }

  protected requestKnowledgeIndexRun(): void {
    this.knowledgeIndexRunRequested.emit();
  }

  protected setKnowledgeSourceEnabled(path: string, enabled: boolean): void {
    this.knowledgeSourceRootEnabled = {
      ...this.knowledgeSourceRootEnabled,
      [path]: enabled
    };
  }

  protected knowledgeSourceEnabled(path: string, fallback: boolean): boolean {
    return this.knowledgeSourceRootEnabled[path] ?? fallback;
  }

  protected requestKnowledgeIndexJobCancel(jobId: string): void {
    this.knowledgeIndexJobCancelRequested.emit(jobId);
  }

  protected requestFilesBrowse(path?: string | null): void {
    this.filesBrowseRequested.emit(path?.trim() || null);
  }

  protected addKnowledgeSourcePath(path: string): void {
    const paths = this.pathListByLine(this.knowledgeSourceRootsText);
    if (!paths.includes(path)) {
      paths.push(path);
    }
    this.knowledgeSourceRootsText = paths.join('\n');
  }

  protected useKnowledgeIndexPath(path: string): void {
    this.knowledgeIndexRoot = path;
  }

  protected deviceStatus(deviceId: string): string {
    return this.deviceActionResults[deviceId] ?? '';
  }

  protected credentialStatus(deviceId: string): string {
    const status = this.state?.data.deviceCredentialStatuses?.find((item) => item.device_id === deviceId);
    if (!status) {
      return this.text('pending', '待配置');
    }
    return status.configured
      ? this.text(`configured / ${status.source}`, `已配置 / ${status.source}`)
      : this.text('pending', '待配置');
  }

  protected isDeviceBusy(deviceId: string, action: string): boolean {
    return this.deviceActionBusyKey === `${deviceId}:${action}`;
  }

  protected activeShareLinks(deviceId: string): number {
    return this.state?.data.shareLinks?.filter((link) => link.device_id === deviceId && link.status === 'active').length ?? 0;
  }

  protected deviceEvidence(deviceId: string): DeviceEvidencePanel | undefined {
    return this.state?.data.deviceEvidence?.[deviceId];
  }

  protected featureStatusToneClass(status: FeatureAvailabilityStatus): string {
    switch (status) {
      case 'available':
        return 'tone-good';
      case 'blocked':
        return 'tone-danger';
      case 'degraded':
      case 'not_configured':
      default:
        return 'tone-warn';
    }
  }

  protected featureFallbackLabel(values: string[]): string {
    return values.length > 0 ? values.join(' -> ') : this.text('none', '无');
  }

  protected featureWhereToEdit(featureId: string): string {
    switch (featureId) {
      case 'retrieval.ocr':
      case 'retrieval.embed':
      case 'retrieval.answer':
      case 'retrieval.vision_summary':
        return this.text('Models & Policies', '模型与策略');
      case 'interactive_reply':
      case 'proactive_delivery':
        return this.text('System Settings', '系统设置');
      case 'binding_availability':
        return this.text('Account Management', '账号与通知');
      default:
        return this.text('Admin API projection', '后台 API 投影');
    }
  }

  protected canCancelKnowledgeIndexJob(status: string): boolean {
    return ['queued', 'running'].includes(String(status).trim().toLowerCase());
  }

  protected knowledgeJobToneClass(status: string): string {
    switch (String(status).trim().toLowerCase()) {
      case 'completed':
      case 'ready':
        return 'tone-good';
      case 'failed':
      case 'blocked':
      case 'canceled':
        return 'tone-danger';
      case 'queued':
      case 'running':
      case 'partial':
        return 'tone-warn';
      default:
        return 'tone-neutral';
    }
  }

  protected bytesLabel(value: number | undefined | null): string {
    const bytes = Number(value ?? 0);
    if (!Number.isFinite(bytes) || bytes <= 0) {
      return '0 B';
    }
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let current = bytes;
    let unit = 0;
    while (current >= 1024 && unit < units.length - 1) {
      current /= 1024;
      unit += 1;
    }
    return `${current >= 10 || unit === 0 ? current.toFixed(0) : current.toFixed(1)} ${units[unit]}`;
  }

  private pathList(value: string[] | string | undefined | null): string[] {
    if (Array.isArray(value)) {
      return value.map((item) => String(item).trim()).filter(Boolean);
    }
    if (typeof value === 'string') {
      return value
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean);
    }
    return [];
  }

  private pathListByLine(value: string | undefined | null): string[] {
    if (!value) {
      return [];
    }
    const seen = new Set<string>();
    return value
      .split(/\r?\n/)
      .map((item) => item.trim())
      .filter((item) => item.length > 0)
      .filter((item) => {
        if (seen.has(item)) {
          return false;
        }
        seen.add(item);
        return true;
      });
  }

  private pathLabel(path: string): string {
    return path.split(/[\\/]/).filter(Boolean).pop() || path;
  }
}
