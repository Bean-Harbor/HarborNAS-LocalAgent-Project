import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable, concat, forkJoin, of, throwError } from 'rxjs';
import { catchError, map, switchMap } from 'rxjs/operators';

import {
  AccessMemberSummary,
  AccountManagementSnapshot,
  AdminStateResponse,
  CameraDevice,
  DeviceCredentialStatus,
  DeviceCredentialsPayload,
  DeviceEvidenceField,
  DeviceEvidencePanel,
  DeviceEvidenceResponse,
  DeviceEvidenceResult,
  DeviceValidationRunRequest,
  DeviceValidationRunResponse,
  DeliverySurface,
  DeskPageModel,
  DeskRow,
  DiscoveryScanPayload,
  FeatureAvailabilityGroup,
  FeatureAvailabilityItem,
  FeatureAvailabilityResponse,
  FeatureAvailabilityStatus,
  GatewayPlatformStatus,
  GatewayStatusResponse,
  HardwareReadinessResponse,
  HarborOsImCapabilityMapResponse,
  HarborOsStatusResponse,
  FilesBrowseResponse,
  KnowledgeIndexJobsResponse,
  KnowledgeIndexJobRecord,
  KnowledgeIndexRunResponse,
  KnowledgeIndexStatusResponse,
  KnowledgeSettings,
  LocalModelCatalogResponse,
  LocalModelDownloadJobResponse,
  LocalModelDownloadsResponse,
  LocalModelDownloadStatusResponse,
  ManualDevicePayload,
  MetricCard,
  MetricTone,
  ModelEndpointRecord,
  ModelEndpointTestResult,
  ModelEndpointsResponse,
  ModelPoliciesResponse,
  PageState,
  RagReadinessResponse,
  ReleaseDomainReadiness,
  ReleaseDomainReadinessCard,
  ReleaseReadinessChecklistItem,
  ReleaseReadinessDeepLink,
  ReleaseReadinessEndpointState,
  ReleaseReadinessEndpointStatus,
  ReleaseReadinessHistoryEntry,
  ReleaseReadinessHistoryResponse,
  ReleaseReadinessPanel,
  ReleaseReadinessResponse,
  ReleaseReadinessRunRequest,
  ReleaseReadinessRunResponse,
  ReleaseReadinessStatus,
  ReleaseReadinessStatusCard,
  RtspCheckPayload,
  RtspCheckResult,
  SetupFlowSection,
  SetupFlowStep,
  ShareLinkSummary,
  StartLocalModelDownloadRequest,
  TaskApprovalSummary
} from './admin-api.types';
import { HarborDeskPageId } from './page-registry';
import { uiText } from './ui-locale';

interface EndpointProjection<T> {
  endpoint: string;
  state: ReleaseReadinessEndpointState;
  data?: T;
  error?: string;
}

@Injectable({
  providedIn: 'root'
})
export class HarborDeskAdminApiService {
  private readonly http = inject(HttpClient);
  private readonly outputDirectory = 'frontend/harbordesk/dist/harbordesk';

  observePage(pageId: HarborDeskPageId): Observable<PageState<DeskPageModel>> {
    return concat(
      of<PageState<DeskPageModel>>(this.loadingState(pageId)),
      this.pageRequest(pageId).pipe(catchError((error) => of(this.blockerState(pageId, this.errorMessage(error)))))
    );
  }

  updateDefaultDeliverySurface(userId: string, surface: DeliverySurface): Observable<AccessMemberSummary[]> {
    return this.http.post<AccessMemberSummary[]>(
      `/api/access/members/${encodeURIComponent(userId)}/default-delivery-surface`,
      { surface }
    );
  }

  setDefaultNotificationTarget(targetId: string): Observable<void> {
    return this.http.post<void>('/api/admin/notification-targets/default', { target_id: targetId });
  }

  deleteNotificationTarget(targetId: string): Observable<void> {
    return this.http.delete<void>(`/api/admin/notification-targets/${encodeURIComponent(targetId)}`);
  }

  testModelEndpoint(modelEndpointId: string): Observable<ModelEndpointTestResult> {
    return this.http.post<ModelEndpointTestResult>(
      `/api/models/endpoints/${encodeURIComponent(modelEndpointId)}/test`,
      {}
    );
  }

  scanDevices(payload: DiscoveryScanPayload): Observable<unknown> {
    return this.http.post<unknown>('/api/discovery/scan', payload);
  }

  addManualDevice(payload: ManualDevicePayload): Observable<unknown> {
    return this.http.post<unknown>('/api/devices/manual', payload);
  }

  setDefaultCamera(deviceId: string | null): Observable<AdminStateResponse> {
    return this.http.post<AdminStateResponse>('/api/devices/default-camera', { device_id: deviceId });
  }

  saveDeviceCredentials(deviceId: string, payload: DeviceCredentialsPayload): Observable<DeviceCredentialStatus> {
    return this.http.post<DeviceCredentialStatus>(
      `/api/devices/${encodeURIComponent(deviceId)}/credentials`,
      payload
    );
  }

  checkDeviceRtsp(deviceId: string, payload: RtspCheckPayload): Observable<RtspCheckResult> {
    return this.http.post<RtspCheckResult>(
      `/api/devices/${encodeURIComponent(deviceId)}/rtsp-check`,
      payload
    );
  }

  getDeviceEvidence(deviceId: string): Observable<DeviceEvidenceResponse> {
    return this.http.get<DeviceEvidenceResponse>(`/api/devices/${encodeURIComponent(deviceId)}/evidence`);
  }

  runDeviceValidation(
    deviceId: string,
    payload: DeviceValidationRunRequest = { scope: 'all' }
  ): Observable<DeviceValidationRunResponse> {
    return this.http.post<DeviceValidationRunResponse>(
      `/api/devices/${encodeURIComponent(deviceId)}/validation/run`,
      payload
    );
  }

  createCameraShareLink(deviceId: string): Observable<unknown> {
    return this.http.post<unknown>(`/api/cameras/${encodeURIComponent(deviceId)}/share-link`, {});
  }

  revokeShareLink(shareLinkId: string): Observable<unknown> {
    return this.http.post<unknown>(`/api/share-links/${encodeURIComponent(shareLinkId)}/revoke`, {});
  }

  createCameraSnapshotTask(deviceId: string): Observable<unknown> {
    return this.http.post<unknown>(`/api/cameras/${encodeURIComponent(deviceId)}/snapshot`, {});
  }

  getReleaseReadiness(): Observable<ReleaseReadinessResponse> {
    return this.http.get<ReleaseReadinessResponse>('/api/release/readiness');
  }

  getReleaseReadinessHistory(): Observable<ReleaseReadinessHistoryResponse> {
    return this.http.get<ReleaseReadinessHistoryResponse>('/api/release/readiness/history');
  }

  runReleaseReadiness(payload: ReleaseReadinessRunRequest = { scope: 'all' }): Observable<ReleaseReadinessRunResponse> {
    return this.http.post<ReleaseReadinessRunResponse>('/api/release/readiness/run', payload);
  }

  getHardwareReadiness(): Observable<HardwareReadinessResponse> {
    return this.http.get<HardwareReadinessResponse>('/api/hardware/readiness');
  }

  getHarborOsStatus(): Observable<HarborOsStatusResponse> {
    return this.http.get<HarborOsStatusResponse>('/api/harboros/status');
  }

  getHarborOsImCapabilityMap(): Observable<HarborOsImCapabilityMapResponse> {
    return this.http.get<HarborOsImCapabilityMapResponse>('/api/harboros/im-capability-map');
  }

  getLocalModelCatalog(): Observable<LocalModelCatalogResponse> {
    return this.http.get<LocalModelCatalogResponse>('/api/models/local-catalog');
  }

  getRagReadiness(): Observable<RagReadinessResponse> {
    return this.http.get<RagReadinessResponse>('/api/rag/readiness');
  }

  getKnowledgeSettings(): Observable<KnowledgeSettings> {
    return this.http.get<KnowledgeSettings>('/api/knowledge/settings');
  }

  saveKnowledgeSettings(payload: KnowledgeSettings): Observable<KnowledgeSettings> {
    return this.http.put<KnowledgeSettings>('/api/knowledge/settings', payload);
  }

  runKnowledgeIndex(): Observable<KnowledgeIndexRunResponse> {
    return this.http.post<KnowledgeIndexRunResponse>('/api/knowledge/index/run', {});
  }

  getKnowledgeIndexStatus(): Observable<KnowledgeIndexStatusResponse> {
    return this.http.get<KnowledgeIndexStatusResponse>('/api/knowledge/index/status');
  }

  getKnowledgeIndexJobs(): Observable<KnowledgeIndexJobsResponse> {
    return this.http.get<KnowledgeIndexJobsResponse>('/api/knowledge/index/jobs');
  }

  cancelKnowledgeIndexJob(jobId: string): Observable<KnowledgeIndexJobRecord> {
    return this.http.post<KnowledgeIndexJobRecord>(
      `/api/knowledge/index/jobs/${encodeURIComponent(jobId)}/cancel`,
      {}
    );
  }

  browseFiles(path?: string | null): Observable<FilesBrowseResponse> {
    const query = path ? `?path=${encodeURIComponent(path)}` : '';
    return this.http.get<FilesBrowseResponse>(`/api/files/browse${query}`);
  }

  getLocalModelDownloads(): Observable<LocalModelDownloadsResponse> {
    return this.http.get<LocalModelDownloadsResponse>('/api/models/local-downloads');
  }

  getLocalModelDownload(jobId: string): Observable<LocalModelDownloadStatusResponse> {
    return this.http.get<LocalModelDownloadStatusResponse>(`/api/models/local-downloads/${encodeURIComponent(jobId)}`);
  }

  startLocalModelDownload(payload: StartLocalModelDownloadRequest): Observable<LocalModelDownloadJobResponse> {
    return this.http.post<LocalModelDownloadJobResponse>('/api/models/local-downloads', payload);
  }

  cancelLocalModelDownload(jobId: string): Observable<LocalModelDownloadJobResponse> {
    return this.http.post<LocalModelDownloadJobResponse>(
      `/api/models/local-downloads/${encodeURIComponent(jobId)}/cancel`,
      {}
    );
  }

  private pageRequest(pageId: HarborDeskPageId): Observable<PageState<DeskPageModel>> {
    switch (pageId) {
      case 'overview':
        return forkJoin({
          state: this.getState(),
          gateway: this.getGatewayStatus(),
          models: this.getModelEndpoints(),
          policies: this.getModelPolicies(),
          release: this.readProjection('GET /api/release/readiness', this.getReleaseReadiness()),
          releaseHistory: this.readProjection('GET /api/release/readiness/history', this.getReleaseReadinessHistory()),
          hardware: this.readProjection('GET /api/hardware/readiness', this.getHardwareReadiness()),
          harborOsStatus: this.readProjection('GET /api/harboros/status', this.getHarborOsStatus()),
          imCapabilityMap: this.readProjection('GET /api/harboros/im-capability-map', this.getHarborOsImCapabilityMap()),
          localCatalog: this.readProjection('GET /api/models/local-catalog', this.getLocalModelCatalog()),
          ragReadiness: this.readProjection('GET /api/rag/readiness', this.getRagReadiness()),
          localDownloads: this.readProjection('GET /api/models/local-downloads', this.getLocalModelDownloads())
        }).pipe(map(({
          state,
          gateway,
          models,
          policies,
          release,
          releaseHistory,
          hardware,
          harborOsStatus,
          imCapabilityMap,
          localCatalog,
          ragReadiness,
          localDownloads
        }) => this.buildOverviewState(
          state,
          gateway,
          models,
          policies,
          release,
          releaseHistory,
          hardware,
          harborOsStatus,
          imCapabilityMap,
          localCatalog,
          ragReadiness,
          localDownloads
        )));
      case 'im-gateway':
        return this.getGatewayStatus().pipe(map((gateway) => this.buildImGatewayState(gateway)));
      case 'account-management':
        return this.getAccountManagement().pipe(map((account) => this.buildAccountManagementState(account)));
      case 'tasks-approvals':
        return this.getPendingApprovals().pipe(map((approvals) => this.buildTasksState(approvals)));
      case 'devices-aiot':
        return this.getState().pipe(
          switchMap((state) => forkJoin({
            shareLinks: this.getShareLinks(),
            evidence: this.getDeviceEvidenceProjections(state.devices ?? [])
          }).pipe(map(({ shareLinks, evidence }) => this.buildDevicesState(state, shareLinks ?? [], evidence))))
        );
      case 'harboros':
        return this.getState().pipe(map((state) => this.buildHarborOsState(state)));
      case 'models-policies':
        return forkJoin({
          endpoints: this.getModelEndpoints(),
          policies: this.getModelPolicies(),
          availability: this.getFeatureAvailability(),
          ragReadiness: this.readProjection('GET /api/rag/readiness', this.getRagReadiness()),
          knowledgeSettings: this.getKnowledgeSettings(),
          knowledgeIndexStatus: this.getKnowledgeIndexStatus(),
          knowledgeIndexJobs: this.readProjection('GET /api/knowledge/index/jobs', this.getKnowledgeIndexJobs()),
          localCatalog: this.readProjection('GET /api/models/local-catalog', this.getLocalModelCatalog()),
          localDownloads: this.readProjection('GET /api/models/local-downloads', this.getLocalModelDownloads())
        }).pipe(map(({ endpoints, policies, availability, ragReadiness, knowledgeSettings, knowledgeIndexStatus, knowledgeIndexJobs, localCatalog, localDownloads }) => this.buildModelsState(
          endpoints,
          policies,
          availability,
          ragReadiness,
          knowledgeSettings,
          knowledgeIndexStatus,
          knowledgeIndexJobs,
          localCatalog,
          localDownloads
        )));
      case 'system-settings':
        return forkJoin({
          state: this.getState(),
          gateway: this.getGatewayStatus(),
          availability: this.getFeatureAvailability()
        }).pipe(map(({ state, gateway, availability }) => this.buildSystemSettingsState(state, gateway, availability)));
      default:
        return throwError(() => new Error(`Unknown HarborDesk page: ${pageId}`));
    }
  }

  private getState(): Observable<AdminStateResponse> {
    return this.http.get<AdminStateResponse>('/api/state');
  }

  private getAccountManagement(): Observable<AccountManagementSnapshot> {
    return this.http.get<AccountManagementSnapshot>('/api/account-management');
  }

  private getAccessMembers(): Observable<AccessMemberSummary[]> {
    return this.http.get<AccessMemberSummary[]>('/api/access/members');
  }

  private getPendingApprovals(): Observable<TaskApprovalSummary[]> {
    return this.http.get<TaskApprovalSummary[]>('/api/tasks/approvals');
  }

  private getGatewayStatus(): Observable<GatewayStatusResponse> {
    return this.http.get<GatewayStatusResponse>('/api/gateway/status');
  }

  private getModelEndpoints(): Observable<ModelEndpointsResponse> {
    return this.http.get<ModelEndpointsResponse>('/api/models/endpoints');
  }

  private getModelPolicies(): Observable<ModelPoliciesResponse> {
    return this.http.get<ModelPoliciesResponse>('/api/models/policies');
  }

  private getFeatureAvailability(): Observable<FeatureAvailabilityResponse> {
    return this.http.get<FeatureAvailabilityResponse>('/api/feature-availability');
  }

  private getShareLinks(): Observable<ShareLinkSummary[]> {
    return this.http.get<ShareLinkSummary[]>('/api/share-links');
  }

  private getDeviceEvidenceProjections(
    devices: CameraDevice[]
  ): Observable<Record<string, EndpointProjection<DeviceEvidenceResponse>>> {
    if (devices.length === 0) {
      return of({});
    }
    return forkJoin(
      devices.map((device) =>
        this.readProjection(
          `GET /api/devices/${device.device_id}/evidence`,
          this.getDeviceEvidence(device.device_id)
        ).pipe(map((projection) => [device.device_id, projection] as const))
      )
    ).pipe(map((entries) => Object.fromEntries(entries) as Record<string, EndpointProjection<DeviceEvidenceResponse>>));
  }

  private readProjection<T>(endpoint: string, request: Observable<T>): Observable<EndpointProjection<T>> {
    return request.pipe(
      map((data) => ({
        endpoint,
        state: this.isEmptyPayload(data) ? 'empty' as const : 'available' as const,
        data
      })),
      catchError((error) => of({
        endpoint,
        state: 'error' as const,
        error: this.errorMessage(error)
      }))
    );
  }

  private loadingState(pageId: HarborDeskPageId): PageState<DeskPageModel> {
    return {
      kind: 'loading',
      detail: this.text('Hydrating same-origin admin API projection.', '正在加载同源后台 API 投影。'),
      data: this.baseModel(pageId)
    };
  }

  private blockerState(pageId: HarborDeskPageId, detail: string): PageState<DeskPageModel> {
    return {
      kind: 'blocker',
      detail,
      data: {
        ...this.baseModel(pageId),
        summary: this.text('The page could not finish loading from the same-origin admin API.', '页面未能从同源后台 API 完成加载。'),
        metrics: [
          {
            label: 'Admin API',
            value: this.text('Blocked', '受阻'),
            detail,
            tone: 'danger'
          }
        ],
        blockers: [detail],
        nextStep: this.text(
          'Restore the admin-plane response for this page before using HarborDesk for operations.',
          '先恢复这个页面需要的后台 API 响应，再使用 HarborDesk 做配置。'
        )
      }
    };
  }

  private baseModel(pageId: HarborDeskPageId): DeskPageModel {
    const titleMap: Record<HarborDeskPageId, string> = {
      overview: this.text('Overview', '总览'),
      'im-gateway': this.text('IM Gateway', 'IM 网关'),
      'account-management': this.text('Account Management', '账号与通知'),
      'tasks-approvals': this.text('Tasks & Approvals', '任务与审批'),
      'devices-aiot': this.text('Devices & AIoT', '设备与 AIoT'),
      harboros: 'HarborOS',
      'models-policies': this.text('Models & Policies', '模型与策略'),
      'system-settings': this.text('System Settings', '系统设置')
    };
    return {
      pageId,
      title: titleMap[pageId],
      eyebrow: 'HarborDesk',
      summary: this.text('Loading same-origin HarborBeacon admin-plane data.', '正在加载 HarborBeacon 同源后台数据。'),
      endpoint: '/api/state',
      outputDirectory: this.outputDirectory,
      metrics: [],
      highlights: [],
      blockers: [],
      emptyNote: this.text('No data reported yet.', '暂无数据。'),
      nextStep: this.text('Wait for the admin API projection to finish loading.', '等待后台 API 投影加载完成。')
    };
  }

  private buildOverviewState(
    state: AdminStateResponse,
    gateway: GatewayStatusResponse,
    models: ModelEndpointsResponse,
    policies: ModelPoliciesResponse,
    release: EndpointProjection<ReleaseReadinessResponse>,
    releaseHistory: EndpointProjection<ReleaseReadinessHistoryResponse>,
    hardware: EndpointProjection<HardwareReadinessResponse>,
    harborOsStatus: EndpointProjection<HarborOsStatusResponse>,
    imCapabilityMap: EndpointProjection<HarborOsImCapabilityMapResponse>,
    localCatalog: EndpointProjection<LocalModelCatalogResponse>,
    ragReadiness: EndpointProjection<RagReadinessResponse>,
    localDownloads: EndpointProjection<LocalModelDownloadsResponse>
  ): PageState<DeskPageModel> {
    const members = state.account_management?.workspace?.member_count ?? 0;
    const devices = state.devices?.length ?? 0;
    const selectedCamera = state.defaults.selected_camera_device_id ?? null;
    const rtspReady = (state.devices ?? []).some((device) => this.deviceRtspConfigured(device));
    const snapshotReady = (state.devices ?? []).some((device) => this.deviceSnapshotConfigured(device));
    const credentialReady = (state.device_credential_statuses ?? []).some((status) => status.configured);
    const endpoints = models.endpoints?.length ?? 0;
    const activeEndpoints = models.endpoints?.filter((endpoint) => endpoint.status === 'active').length ?? 0;
    const routePolicies = policies.route_policies?.length ?? 0;
    const feishuReady = this.platformReady(gateway, 'feishu');
    const weixinReady = this.platformReady(gateway, 'weixin');
    const releaseReadiness = this.buildReleaseReadinessPanel(
      state,
      release,
      releaseHistory,
      hardware,
      harborOsStatus,
      imCapabilityMap,
      localCatalog,
      ragReadiness,
      localDownloads
    );
    const blockers = this.uniqueLines([...this.imBlockers(gateway), ...releaseReadiness.blockers]);
    return {
      kind: 'success',
      detail: this.text(
        'Overview is reading live state, delivery policy, gateway readiness, and model inventory.',
        '总览正在读取实时状态、投递策略、IM 网关 readiness 和模型清单。'
      ),
      data: {
        ...this.baseModel('overview'),
        eyebrow: this.text('Unified control plane', '统一配置入口'),
        summary: this.text(
          'Interactive replies stay source-bound, while proactive delivery follows the default named HarborGate target.',
          '首屏用于发布前配置 IM、模型、硬件、HarborOS 和 AIoT readiness；实际 IM 凭据仍归 HarborGate。'
        ),
        endpoint: 'GET /api/release/readiness + /api/state + /api/gateway/status',
        releaseReadiness,
        setupFlow: this.setupFlow(
          this.text('Release readiness setup flow', '发布 readiness 配置流程'),
          this.text(
            'Use the live admin-plane projection to wire Weixin, HarborOS identity, camera registration, storage guidance, local models, and policy selection without inventing new framework objects.',
            '使用实时后台投影完成微信、飞书、模型、硬件、默认摄像头、HarborOS 和存储 readiness 配置。'
          ),
          [
            this.setupStep(
              this.text('HarborOS principal reuse', 'HarborOS 身份复用'),
              state.account_management?.workspace?.current_principal_user_id
                ? 'ready'
                : 'blocked',
              state.account_management?.workspace?.current_principal_user_id
                ? this.text('HarborDesk inherits the HarborOS login principal as the admin principal.', 'HarborDesk 复用 HarborOS 登录用户作为后台管理主体。')
                : this.text('HarborOS identity is not projected yet.', 'HarborOS 登录身份尚未投影到后台。'),
              state.account_management?.workspace?.current_principal_user_id
                ? this.text(
                  `Current HarborOS principal: ${state.account_management.workspace.current_principal_display_name || state.account_management.workspace.current_principal_user_id}. Workspace owner baseline: ${state.account_management.workspace.owner_user_id}.`,
                  `当前 HarborOS 用户：${state.account_management.workspace.current_principal_display_name || state.account_management.workspace.current_principal_user_id}。工作区 owner 基线：${state.account_management.workspace.owner_user_id}。`
                )
                : this.text('The current OS-session principal must be surfaced before release-v1 can be treated as HarborOS-native.', '发布前需要先把当前 OS 会话用户投影出来。'),
              state.account_management?.workspace?.current_principal_user_id
                ? [
                  this.text('Use the HarborOS login user as the admin owner baseline.', '使用 HarborOS 登录用户作为后台 owner 基线。'),
                  this.text('No separate HarborDesk login is introduced.', '不引入第二套 HarborDesk 登录。')
                ]
                : [this.text('Blocker: current HarborOS principal is not yet surfaced by the backend.', '阻塞：后端尚未投影当前 HarborOS 用户。')]
            ),
            this.setupStep(
              this.text('Weixin transport readiness', '微信接入 readiness'),
              weixinReady ? 'ready' : 'needs-config',
              weixinReady
                ? this.text('Weixin parity is available for live validation.', '微信已可用于真人测试主链。')
                : this.text('Weixin still needs transport/provider-side cleanup before it can carry the release-v1 flow.', '微信传输侧仍需清理后才能承载发布流程。'),
              weixinReady
                ? this.text(
                  `Gateway status is ${gateway.weixin?.rehearsal_ready ? 'rehearsal_ready' : 'connected'} and source-bound delivery remains separated from proactive delivery.`,
                  `网关状态：${gateway.weixin?.rehearsal_ready ? 'rehearsal_ready' : 'connected'}；交互回复仍保持 source-bound。`
                )
                : this.text(
                  `Current blocker: ${this.weixinBlocker(gateway) ?? 'transport not yet ready'}.`,
                  `当前阻塞：${this.weixinBlocker(gateway) ?? '传输尚未 ready'}。`
                ),
              weixinReady
                ? [
                  this.text('Use Weixin as the live validation surface.', '使用微信作为 live 验收入口。'),
                  this.text('Keep replies source-bound to the original message channel.', '回复保持绑定原消息通道。')
                ]
                : [this.text('Do not promote Weixin into the flow until the gateway is healthy enough to carry private-text ingress.', '微信私聊入口健康前不要进入 release 主链。')]
            ),
            this.setupStep(
              this.text('AIoT default camera', 'AIoT 默认摄像头'),
              selectedCamera && rtspReady ? 'ready' : devices > 0 ? 'needs-config' : 'blocked',
              selectedCamera
                ? this.text('A default camera is projected from HarborBeacon defaults.', '已从 HarborBeacon defaults 投影默认摄像头。')
                : this.text('No default camera is selected yet.', '尚未选择默认摄像头。'),
              selectedCamera
                ? this.text(
                  `Default camera: ${selectedCamera}. RTSP ready=${String(rtspReady)}, snapshot ready=${String(snapshotReady)}, credentials configured=${String(credentialReady)}.`,
                  `默认摄像头：${selectedCamera}。RTSP=${String(rtspReady)}，快照=${String(snapshotReady)}，凭据已配置=${String(credentialReady)}。`
                )
                : devices > 0
                  ? this.text('Open Devices & AIoT to select the default camera and run RTSP/snapshot checks.', '进入“设备与 AIoT”选择默认摄像头并执行 RTSP/快照检查。')
                  : this.text('Register at least one camera before the release setup flow can continue.', '至少注册一台摄像头后才能继续发布前配置。'),
              [
                this.text('AIoT management lives in Devices & AIoT.', 'AIoT 管理入口在“设备与 AIoT”。'),
                this.text('HarborOS stays System Domain only.', 'HarborOS 页面只保留 System Domain 能力。')
              ]
            ),
            this.setupStep(
              this.text('Capture storage and subdirectory', '采集存储与子目录'),
              state.writable_root && state.defaults.capture_subdirectory ? 'read-only' : 'blocked',
              state.writable_root && state.defaults.capture_subdirectory
                ? this.text('Writable root and capture subdirectory are both projected from the backend.', '后端已投影 writable root 和 capture 子目录。')
                : this.text('The backend does not yet project a writable root and capture_subdir together.', '后端尚未同时投影 writable root 与 capture_subdir。'),
              state.writable_root && state.defaults.capture_subdirectory
                ? this.text(
                  `Capture target: ${state.writable_root}/${state.defaults.capture_subdirectory}.`,
                  `采集目标：${state.writable_root}/${state.defaults.capture_subdirectory}。`
                )
                : this.text(
                  `Current capture label: ${state.defaults.capture}. Writable-root and concrete subdirectory wiring must be surfaced by the backend before this step becomes editable.`,
                  `当前 capture label：${state.defaults.capture}。后端投影 writable-root 和具体子目录后，这一步才能进入可编辑状态。`
                ),
              state.writable_root && state.defaults.capture_subdirectory
                ? [
                  this.text('This remains a setup flow over existing defaults and recording policy metadata.', '这里仍然只基于现有 defaults 与记录策略 metadata。'),
                  this.text('No release scene object is introduced.', '不引入新的 release scene 对象。')
                ]
                : [
                  this.text('Treat this as read-only guidance for now.', '当前只作为只读指引。'),
                  this.text('Do not fake a capture path in the UI.', '界面不伪造 capture path。')
                ]
            ),
            this.setupStep(
              this.text('Clip length and recording policy', '短视频长度与录制策略'),
              state.defaults.clip_length_seconds ? 'read-only' : 'blocked',
              state.defaults.clip_length_seconds
                ? this.text('Clip duration and keyframe hints are projected from the existing recording policy.', '短视频长度与 keyframe hint 已由现有记录策略投影。')
                : this.text('Clip duration is not projected as a numeric, writable policy yet.', '短视频长度尚未作为可写数值策略投影。'),
              state.defaults.clip_length_seconds
                ? this.text(
                  `Clip length: ${state.defaults.clip_length_seconds}s. Keyframes: ${state.defaults.keyframe_count ?? 'n/a'} at ${state.defaults.keyframe_interval_seconds ?? 'n/a'}s interval.`,
                  `短视频长度：${state.defaults.clip_length_seconds}s。Keyframes：${state.defaults.keyframe_count ?? 'n/a'}，间隔 ${state.defaults.keyframe_interval_seconds ?? 'n/a'}s。`
                )
                : this.text(
                  `Current recording label: ${state.defaults.recording}. The real clip length must be surfaced through the existing recording policy projection before release readiness can make it editable.`,
                  `当前 recording label：${state.defaults.recording}。真实 clip length 需要先通过现有记录策略投影出来。`
                ),
              [
                this.text('Short-video support stays scoped as short clip + keyframe retrieval.', '短视频支持范围仍是短片段 + keyframe 检索。'),
                this.text('No new scene object is introduced to solve this.', '不通过新增 scene 对象绕过这一步。')
              ]
            ),
            this.setupStep(
              this.text('OCR / VLM / reply policy selection', 'OCR / VLM / 回复策略选择'),
              endpoints > 0 && routePolicies > 0 ? 'ready' : 'needs-config',
              endpoints > 0 && routePolicies > 0
                ? this.text('Model endpoints and route policies are visible and can be inspected in the models page.', '模型端点和路由策略已可在模型页面检查。')
                : this.text('Model selection still needs the backend-projected inventory before it can be treated as release-ready.', '模型选择还需要后端投影清单后才能算作 release-ready。'),
              endpoints > 0 && routePolicies > 0
                ? this.text(
                  `Endpoints: ${endpoints}. Route policies: ${routePolicies}. Use Models & Policies to verify the OCR/VLM/reply choices and their operator status.`,
                  `模型端点：${endpoints}。路由策略：${routePolicies}。进入“模型与策略”确认 OCR/VLM/回复选择和 operator 状态。`
                )
                : this.text('Register model endpoints and route policies before using this part of the setup flow.', '使用这一段配置流程前，需要先注册模型端点和路由策略。'),
              endpoints > 0 && routePolicies > 0
                ? [
                  this.text('VLM-first stays the multimodal priority.', '多模态优先保持 VLM-first。'),
                  this.text('Audio and full video understanding remain pending by design.', '音频与完整视频理解仍按设计暂不放开。')
                ]
                : [this.text('Blocker: model endpoints or route policies are still missing.', '阻塞：模型端点或路由策略仍缺失。')]
            ),
            this.setupStep(
              this.text('Default proactive notification target', '默认主动通知目标'),
              state.delivery_policy.proactive_delivery ? 'ready' : 'blocked',
              this.text('Independent notifications follow the default named HarborGate target.', '独立通知走默认命名 HarborGate target。'),
              this.text(
                `Current workspace policy: ${state.delivery_policy.proactive_delivery}. Account Management shows which named target is currently default.`,
                `当前工作区策略：${state.delivery_policy.proactive_delivery}。账号与通知页面显示当前默认命名 target。`
              ),
              [
                this.text('HarborGate owns target capture and IM identity.', 'HarborGate 拥有 target capture 与 IM identity。'),
                this.text('Interactive replies remain source-bound.', '交互回复保持 source-bound。')
              ]
            )
          ]
        ),
        metrics: [
          this.metric(this.text('Release readiness', '发布 readiness'), releaseReadiness.status, releaseReadiness.summary, this.readinessTone(releaseReadiness.status)),
          this.metric(this.text('Workspace members', '工作区成员'), `${members}`, this.text('Live count from account management.', '账号管理实时人数。'), members > 0 ? 'good' : 'warn'),
          this.metric(this.text('Registered devices', '已注册设备'), `${devices}`, this.text('Current Home Device Domain registry size.', '当前 Home Device Domain registry 数量。'), devices > 0 ? 'good' : 'neutral'),
          this.metric(this.text('Default camera', '默认摄像头'), selectedCamera ? this.text('Selected', '已选择') : this.text('Pending', '待配置'), selectedCamera || this.text('Configure in Devices & AIoT.', '在“设备与 AIoT”配置。'), selectedCamera ? 'good' : 'warn'),
          this.metric(this.text('RTSP / snapshot', 'RTSP / 快照'), `${rtspReady ? 'RTSP' : this.text('RTSP pending', 'RTSP 待配置')} / ${snapshotReady ? this.text('snapshot', '快照') : this.text('snapshot pending', '快照待配置')}`, this.text('AIoT readiness from camera registry and credential projection.', '来自摄像头 registry 和凭据投影的 AIoT readiness。'), rtspReady || snapshotReady ? 'good' : 'warn'),
          this.metric(this.text('Model endpoints', '模型端点'), `${activeEndpoints}/${endpoints}`, this.text('Active/total endpoints from Model Center admin-plane.', '模型中心 active/total 端点数。'), activeEndpoints > 0 ? 'good' : 'warn'),
          this.metric(this.text('Route policies', '路由策略'), `${routePolicies}`, this.text('Model route policies surfaced by the admin-plane.', '后台投影出的模型路由策略。'), routePolicies > 0 ? 'good' : 'warn'),
          this.metric(this.text('Delivery policy', '投递策略'), `${state.delivery_policy.interactive_reply} / ${state.delivery_policy.proactive_delivery}`, this.text('Interactive reply and proactive default policy are frozen.', '交互回复与主动通知默认策略保持冻结。'), 'good'),
          this.metric(this.text('Feishu readiness', '飞书 readiness'), feishuReady ? this.text('Ready', '已就绪') : this.text('Pending', '待配置'), this.text('Baseline channel readiness from HarborGate.', 'HarborGate 投影的基线通道状态。'), feishuReady ? 'good' : 'warn'),
          this.metric(this.text('Weixin readiness', '微信 readiness'), weixinReady ? this.text('Ready', '已就绪') : this.text('Pending', '待配置'), this.text('Parity track readiness from HarborGate.', 'HarborGate 投影的微信通道状态。'), weixinReady ? 'good' : 'warn')
        ],
        highlights: [
          this.text('HarborDesk is the Angular admin shell, normally opened at :4174; HarborOS WebUI remains /ui/ on ports 80/443.', 'HarborDesk 是 Angular 后台 shell，通常打开在 :4174；HarborOS WebUI 仍在 80/443 的 /ui/。'),
          this.text(`Workspace: ${state.account_management.workspace.display_name}`, `工作区：${state.account_management.workspace.display_name}`),
          this.text(`Current principal: ${state.account_management.workspace.current_principal_display_name || state.current_principal_display_name || state.account_management.workspace.owner_user_id}.`, `当前用户：${state.account_management.workspace.current_principal_display_name || state.current_principal_display_name || state.account_management.workspace.owner_user_id}。`),
          this.text(`Weixin setup entry: ${gateway.weixin?.configured ? 'configured' : gateway.manage_url || state.account_management?.gateway?.manage_url || 'HarborGate manage URL pending'}.`, `微信配置入口：${gateway.weixin?.configured ? '已配置' : gateway.manage_url || state.account_management?.gateway?.manage_url || 'HarborGate manage URL 待投影'}。`),
          this.text(`Feishu API key entry: ${gateway.feishu?.rehearsal_ready ? 'ready' : gateway.manage_url || state.account_management?.gateway?.manage_url || 'HarborGate manage URL pending'}.`, `飞书 API key 入口：${gateway.feishu?.rehearsal_ready ? '已就绪' : gateway.manage_url || state.account_management?.gateway?.manage_url || 'HarborGate manage URL 待投影'}。`),
          this.text(`Default proactive routing is ${state.delivery_policy.proactive_delivery}.`, `默认主动通知路由：${state.delivery_policy.proactive_delivery}。`),
          this.text(`Bridge provider status: ${state.bridge_provider.status || 'unknown'}.`, `Bridge provider 状态：${state.bridge_provider.status || 'unknown'}。`)
        ],
        blockers,
        detailRows: [
          {
            title: this.text('Gateway base URL', '网关 base URL'),
            subtitle: state.bridge_provider.gateway_base_url || this.text('not configured', '未配置'),
            meta: [
              this.text(`Binding channel: ${state.binding.channel}`, `绑定通道：${state.binding.channel}`),
              this.text(`Binding status: ${state.binding.status}`, `绑定状态：${state.binding.status}`)
            ],
            tone: state.bridge_provider.connected ? 'good' : 'warn'
          }
        ],
        emptyNote: this.text('Overview has no live metrics yet.', '总览暂无实时指标。'),
        nextStep: blockers.length === 0
          ? this.text('Proceed to a domain page for action-level operations.', '进入对应页面完成具体配置操作。')
          : this.text('Clear the surfaced blockers before declaring dual-surface readiness.', '先清理页面暴露的阻塞项，再宣布 release readiness。')
      }
    };
  }

  private buildReleaseReadinessPanel(
    state: AdminStateResponse,
    release: EndpointProjection<ReleaseReadinessResponse>,
    releaseHistory: EndpointProjection<ReleaseReadinessHistoryResponse>,
    hardware: EndpointProjection<HardwareReadinessResponse>,
    harborOsStatus: EndpointProjection<HarborOsStatusResponse>,
    imCapabilityMap: EndpointProjection<HarborOsImCapabilityMapResponse>,
    localCatalog: EndpointProjection<LocalModelCatalogResponse>,
    ragReadiness: EndpointProjection<RagReadinessResponse>,
    localDownloads: EndpointProjection<LocalModelDownloadsResponse>
  ): ReleaseReadinessPanel {
    const coreEndpointStates = [
      this.endpointStatus(release),
      this.endpointStatus(hardware),
      this.endpointStatus(harborOsStatus),
      this.endpointStatus(imCapabilityMap),
      this.endpointStatus(localCatalog)
    ];
    const optionalEndpointStates = [
      this.endpointStatus(releaseHistory),
      this.endpointStatus(ragReadiness),
      this.endpointStatus(localDownloads)
    ];
    const endpointStates = [...coreEndpointStates, ...optionalEndpointStates];
    const endpointErrors = coreEndpointStates
      .filter((state) => state.state === 'error')
      .map((state) => `${state.endpoint}: ${state.detail}`);
    const optionalEndpointErrors = optionalEndpointStates
      .filter((state) => state.state === 'error')
      .map((state) => `${state.endpoint}: ${state.detail}`);
    const releaseData = release.data;
    const checklist = releaseData?.checklist?.length
      ? releaseData.checklist.map((item) => this.normalizedChecklistItem(item))
      : this.defaultReleaseChecklist(release, hardware, harborOsStatus, imCapabilityMap, localCatalog);
    const backendCards = (releaseData?.status_cards ?? []).map((card) => this.normalizedStatusCard(card));
    const statusCards = [
      ...this.defaultReleaseStatusCards(release, hardware, harborOsStatus, imCapabilityMap, localCatalog),
      ...backendCards
    ];
    const domainCards = this.releaseDomainCards(
      releaseData?.domains ?? releaseData?.domain_cards ?? [],
      state,
      hardware,
      harborOsStatus,
      imCapabilityMap,
      localCatalog,
      ragReadiness,
      localDownloads
    );
    const deepLinks = this.releaseDeepLinks(releaseData?.deep_links ?? []);
    const history = (releaseHistory.data?.history ?? []).slice(0, 5).map((entry) => ({
      ...entry,
      status: this.normalizeReadinessStatus(entry.status)
    }));
    const blockers = this.uniqueLines([
      ...(releaseData?.blockers ?? []),
      ...(hardware.data?.blockers ?? []),
      ...(harborOsStatus.data?.blockers ?? []),
      ...(imCapabilityMap.data?.blockers ?? []),
      ...(localCatalog.data?.blockers ?? []),
      ...endpointErrors
    ]);
    const warnings = this.uniqueLines([
      ...(releaseData?.warnings ?? []),
      ...(hardware.data?.warnings ?? []),
      ...(harborOsStatus.data?.warnings ?? []),
      ...(imCapabilityMap.data?.warnings ?? []),
      ...(localCatalog.data?.warnings ?? []),
      ...(ragReadiness.data?.warnings ?? []),
      ...(localDownloads.data?.warnings ?? []),
      ...optionalEndpointErrors
    ]);
    const empty = release.state === 'empty' || (
      release.state === 'available' &&
      (releaseData?.checklist?.length ?? 0) === 0 &&
      (releaseData?.status_cards?.length ?? 0) === 0 &&
      (releaseData?.deep_links?.length ?? 0) === 0
    );
    const status = this.releasePanelStatus(releaseData?.status, checklist, blockers);

    return {
      status,
      summary: releaseData?.summary ?? this.releasePanelSummary(release, endpointErrors.length, empty),
      checked_at: releaseData?.checked_at ?? releaseData?.generated_at ?? hardware.data?.checked_at ?? harborOsStatus.data?.checked_at ?? null,
      checklist,
      statusCards,
      domainCards,
      deepLinks,
      history,
      blockers,
      warnings,
      endpointStates,
      empty
    };
  }

  private defaultReleaseStatusCards(
    release: EndpointProjection<ReleaseReadinessResponse>,
    hardware: EndpointProjection<HardwareReadinessResponse>,
    harborOsStatus: EndpointProjection<HarborOsStatusResponse>,
    imCapabilityMap: EndpointProjection<HarborOsImCapabilityMapResponse>,
    localCatalog: EndpointProjection<LocalModelCatalogResponse>
  ): ReleaseReadinessStatusCard[] {
    const localModels = localCatalog.data?.models ?? [];
    const installedModels = localModels.filter((model) => model.installed || model.status === 'ready').length;
    return [
      this.releaseStatusCard(
        'release-api',
        this.text('Release control pack', '发布控制包'),
        this.projectionValue(release),
        this.projectionReadiness(release),
        this.projectionDetail(release, this.text('Aggregated checklist from Release Readiness API.', '来自 Release Readiness API 的聚合 checklist。')),
        release.endpoint,
        '/overview'
      ),
      this.releaseStatusCard(
        'hardware-readiness',
        this.text('Hardware readiness', '硬件 readiness'),
        this.projectionValue(hardware),
        this.projectionReadiness(hardware),
        this.projectionDetail(hardware, this.text('Hardware and camera readiness stay in the Home Device Domain.', '硬件与摄像头 readiness 仍归 Home Device Domain。')),
        hardware.endpoint,
        '/devices-aiot'
      ),
      this.releaseStatusCard(
        'harboros-status',
        'HarborOS',
        this.projectionValue(harborOsStatus),
        this.projectionReadiness(harborOsStatus),
        this.projectionDetail(harborOsStatus, this.text('HarborOS status is read from the HarborOS status API, not inferred from HarborDesk.', 'HarborOS 状态来自 HarborOS status API，不由 HarborDesk 推断。')),
        harborOsStatus.endpoint,
        '/harboros'
      ),
      this.releaseStatusCard(
        'im-capability-map',
        this.text('IM capability map', 'IM capability map'),
        this.projectionValue(imCapabilityMap),
        this.projectionReadiness(imCapabilityMap),
        this.projectionDetail(imCapabilityMap, this.text('IM capability map keeps HarborGate transport and HarborBeacon business state separated.', 'IM capability map 用来保持 HarborGate transport 与 HarborBeacon business state 分离。')),
        imCapabilityMap.endpoint,
        '/system-settings'
      ),
      this.releaseStatusCard(
        'local-model-catalog',
        this.text('Local model catalog', '本地模型清单'),
        localModels.length > 0 ? `${installedModels}/${localModels.length}` : this.projectionValue(localCatalog),
        this.projectionReadiness(localCatalog),
        localModels.length > 0
          ? this.text('Installed/total local model entries projected by the catalog API.', '本地模型清单 API 投影的 installed/total 数量。')
          : this.projectionDetail(localCatalog, this.text('Local model catalog is not populated yet.', '本地模型清单尚未填充。')),
        localCatalog.endpoint,
        '/models-policies'
      ),
      this.releaseStatusCard(
        'ui-boundary',
        this.text('UI boundary', 'UI 边界'),
        ':4174 != /ui/',
        'ready',
        this.text(
          'HarborDesk is the Angular admin shell commonly opened at :4174. HarborOS WebUI remains /ui/ on ports 80/443.',
          'HarborDesk 是通常打开在 :4174 的 Angular 后台 shell。HarborOS WebUI 仍在 80/443 的 /ui/。'
        ),
        'HarborDesk :4174; HarborOS WebUI /ui/ or 80/443',
        '/ui/'
      )
    ];
  }

  private releaseDomainCards(
    extras: ReleaseDomainReadiness[],
    state: AdminStateResponse,
    hardware: EndpointProjection<HardwareReadinessResponse>,
    harborOsStatus: EndpointProjection<HarborOsStatusResponse>,
    imCapabilityMap: EndpointProjection<HarborOsImCapabilityMapResponse>,
    localCatalog: EndpointProjection<LocalModelCatalogResponse>,
    ragReadiness: EndpointProjection<RagReadinessResponse>,
    localDownloads: EndpointProjection<LocalModelDownloadsResponse>
  ): ReleaseDomainReadinessCard[] {
    const orderedIds = ['im', 'models', 'hardware', 'harboros', 'aiot'];
    const defaults = this.defaultReleaseDomainCards(
      state,
      hardware,
      harborOsStatus,
      imCapabilityMap,
      localCatalog,
      ragReadiness,
      localDownloads
    );
    const byId = new Map(defaults.map((card) => [card.id, card]));
    for (const extra of extras) {
      byId.set(extra.id, this.normalizedReleaseDomainCard(extra, byId.get(extra.id)));
    }
    return [
      ...orderedIds.filter((id) => byId.has(id)).map((id) => byId.get(id) as ReleaseDomainReadinessCard),
      ...Array.from(byId.values()).filter((card) => !orderedIds.includes(card.id))
    ];
  }

  private defaultReleaseDomainCards(
    state: AdminStateResponse,
    hardware: EndpointProjection<HardwareReadinessResponse>,
    harborOsStatus: EndpointProjection<HarborOsStatusResponse>,
    imCapabilityMap: EndpointProjection<HarborOsImCapabilityMapResponse>,
    localCatalog: EndpointProjection<LocalModelCatalogResponse>,
    ragReadiness: EndpointProjection<RagReadinessResponse>,
    localDownloads: EndpointProjection<LocalModelDownloadsResponse>
  ): ReleaseDomainReadinessCard[] {
    const localModels = localCatalog.data?.models ?? [];
    const installedModels = localModels.filter((model) => model.installed || model.status === 'ready').length;
    const downloads = localDownloads.data?.downloads ?? [];
    const activeDownloads = downloads.filter((download) => ['queued', 'downloading', 'running'].includes(String(download.status))).length;
    const devices = state.devices ?? [];
    const credentialStatuses = state.device_credential_statuses ?? [];
    const configuredCredentials = credentialStatuses.filter((status) => status.configured).length;
    const selectedDeviceId = state.defaults.selected_camera_device_id ?? null;
    const aiotStatus: ReleaseReadinessStatus = devices.length === 0
      ? 'needs-config'
      : selectedDeviceId && configuredCredentials > 0
        ? 'ready'
        : 'needs-config';

    return [
      this.releaseDomainCard(
        'im',
        this.text('IM', 'IM'),
        this.projectionReadiness(imCapabilityMap),
        this.projectionDetail(
          imCapabilityMap,
          this.text('HarborGate owns IM transport and credential readiness.', 'HarborGate 负责 IM transport 与凭据 readiness。')
        ),
        this.projectionReadiness(imCapabilityMap) === 'ready'
          ? this.text('Keep credentials in HarborGate and continue release validation.', '凭据继续留在 HarborGate，然后继续发布验收。')
          : this.text('Open IM Gateway and HarborGate management to clear transport or credential blockers.', '进入 IM Gateway 与 HarborGate 管理侧清理传输或凭据阻塞。'),
        '/im-gateway',
        imCapabilityMap.data?.checked_at,
        imCapabilityMap.endpoint,
        this.text('Credential ownership stays outside HarborBeacon.', 'IM 凭据归属保持在 HarborBeacon 之外。'),
        [
          this.text('HarborBeacon consumes redacted readiness only.', 'HarborBeacon 只消费 redacted readiness。'),
          this.text('route_key remains opaque.', 'route_key 保持 opaque。')
        ]
      ),
      this.releaseDomainCard(
        'models',
        this.text('Models', '模型'),
        this.combinedReadiness([this.projectionReadiness(localCatalog), this.optionalProjectionReadiness(ragReadiness)]),
        this.text(
          `Local models installed: ${installedModels}/${localModels.length}. RAG: ${ragReadiness.data?.status ?? ragReadiness.state}. Active downloads: ${activeDownloads}.`,
          `本地模型 installed：${installedModels}/${localModels.length}。RAG：${ragReadiness.data?.status ?? ragReadiness.state}。进行中下载：${activeDownloads}。`
        ),
        installedModels > 0
          ? this.text('Verify route policy, RAG readiness, and fallback order in Models & Policies.', '进入“模型与策略”确认路由策略、RAG readiness 与 fallback 顺序。')
          : this.text('Populate local model inventory or start the required local downloads.', '补齐本地模型清单或启动必要的本地模型下载。'),
        '/models-policies',
        this.latestTimestamp([
          localCatalog.data?.checked_at,
          ragReadiness.data?.checked_at,
          ragReadiness.data?.generated_at,
          localDownloads.data?.checked_at,
          localDownloads.data?.generated_at,
          ...downloads.map((download) => download.updated_at)
        ]),
        'GET /api/models/local-catalog + GET /api/rag/readiness + GET /api/models/local-downloads',
        this.text('Models include endpoint inventory, RAG readiness, and local download state.', '模型域包括端点清单、RAG readiness 和本地下载状态。'),
        [
          this.text(`Catalog endpoint: ${localCatalog.state}.`, `模型清单接口：${localCatalog.state}。`),
          this.text(`RAG endpoint: ${ragReadiness.state}.`, `RAG 接口：${ragReadiness.state}。`),
          this.text(`Downloads endpoint: ${localDownloads.state}.`, `下载状态接口：${localDownloads.state}。`)
        ]
      ),
      this.releaseDomainCard(
        'hardware',
        this.text('Hardware', '硬件'),
        this.projectionReadiness(hardware),
        this.projectionDetail(hardware, this.text('Hardware readiness is projected by the readiness API.', '硬件 readiness 由 readiness API 投影。')),
        this.projectionReadiness(hardware) === 'ready'
          ? this.text('Keep the latest hardware validation evidence attached.', '保留最新硬件验收证据。')
          : this.text('Run hardware validation and clear blockers before release.', '运行硬件验收并清理阻塞项后再发布。'),
        '/devices-aiot',
        hardware.data?.checked_at,
        hardware.endpoint,
        this.text('Hardware status is distinct from HarborDesk shell availability.', '硬件状态不等同于 HarborDesk shell 可用性。'),
        hardware.data?.devices?.map((device) => `${device.label ?? device.device_id}: ${device.status}`) ?? []
      ),
      this.releaseDomainCard(
        'harboros',
        'HarborOS',
        this.projectionReadiness(harborOsStatus),
        this.text(
          `HarborOS status: ${harborOsStatus.data?.status ?? harborOsStatus.state}. WebUI: ${harborOsStatus.data?.webui_url ?? '/ui/ or 80/443'}. HarborDesk: :4174.`,
          `HarborOS 状态：${harborOsStatus.data?.status ?? harborOsStatus.state}。WebUI：${harborOsStatus.data?.webui_url ?? '/ui/ 或 80/443'}。HarborDesk：:4174。`
        ),
        this.projectionReadiness(harborOsStatus) === 'ready'
          ? this.text('Use HarborDesk for config/admin; use HarborOS WebUI only for system UI.', '配置管理用 HarborDesk；系统 UI 仍走 HarborOS WebUI。')
          : this.text('Open the HarborOS page and confirm System Domain projection.', '进入 HarborOS 页面确认 System Domain 投影。'),
        '/harboros',
        harborOsStatus.data?.checked_at,
        harborOsStatus.endpoint,
        this.text('HarborOS remains System Domain only.', 'HarborOS 只保留 System Domain。'),
        [
          this.text('HarborDesk route: :4174.', 'HarborDesk 路由：:4174。'),
          this.text('HarborOS WebUI: /ui/ or 80/443.', 'HarborOS WebUI：/ui/ 或 80/443。')
        ]
      ),
      this.releaseDomainCard(
        'aiot',
        'AIoT',
        aiotStatus,
        this.text(
          `Devices: ${devices.length}. Default camera: ${selectedDeviceId ?? 'pending'}. Credential records: ${configuredCredentials}/${devices.length}.`,
          `设备数：${devices.length}。默认摄像头：${selectedDeviceId ?? '待配置'}。凭据记录：${configuredCredentials}/${devices.length}。`
        ),
        aiotStatus === 'ready'
          ? this.text('Run per-device validation to refresh RTSP, snapshot, share-link, and credential evidence.', '运行单设备验收以刷新 RTSP、快照、分享链接和凭据证据。')
          : this.text('Register devices, choose a default camera, and validate RTSP/snapshot evidence.', '注册设备、选择默认摄像头，并验证 RTSP/快照证据。'),
        '/devices-aiot',
        this.latestTimestamp([
          ...credentialStatuses.map((status) => status.last_verified_at ?? status.updated_at),
          hardware.data?.checked_at
        ]),
        'GET /api/devices/{device_id}/evidence + POST /api/devices/{device_id}/validation/run',
        this.text('AIoT management lives in HarborDesk plus the AIoT lane, not HarborOS or HarborGate.', 'AIoT 管理在 HarborDesk 与 AIoT lane，不进入 HarborOS 或 HarborGate。'),
        [
          this.text('Evidence renders redacted credential status only.', '证据只显示 redacted 凭据状态。'),
          this.text('RTSP URLs and passwords are not rendered.', '不渲染 RTSP URL 与密码。')
        ]
      )
    ];
  }

  private normalizedReleaseDomainCard(
    domain: ReleaseDomainReadiness,
    fallback?: ReleaseDomainReadinessCard
  ): ReleaseDomainReadinessCard {
    const status = this.normalizeReadinessStatus(domain.status ?? fallback?.status);
    return {
      id: domain.id,
      label: domain.label ?? fallback?.label ?? domain.id,
      status,
      current_status: domain.current_status ?? domain.detail ?? fallback?.current_status ?? status,
      next_action: domain.next_action ?? fallback?.next_action ?? this.text('Review this domain before release.', '发布前检查该域。'),
      action_path: domain.action_path ?? fallback?.action_path ?? '/overview',
      recent_at: domain.recent_validation_at ?? domain.checked_at ?? domain.generated_at ?? fallback?.recent_at ?? null,
      endpoint: domain.endpoint ?? fallback?.endpoint,
      detail: domain.detail ?? fallback?.detail,
      evidence: this.uniqueLines([...(fallback?.evidence ?? []), ...(domain.evidence ?? [])]),
      blockers: this.uniqueLines([...(fallback?.blockers ?? []), ...(domain.blockers ?? [])]),
      warnings: this.uniqueLines([...(fallback?.warnings ?? []), ...(domain.warnings ?? [])]),
      tone: this.readinessTone(status)
    };
  }

  private releaseDomainCard(
    id: string,
    label: string,
    status: ReleaseReadinessStatus,
    currentStatus: string,
    nextAction: string,
    actionPath: string,
    recentAt: string | null | undefined,
    endpoint: string,
    detail: string,
    evidence: string[] = []
  ): ReleaseDomainReadinessCard {
    return {
      id,
      label,
      status,
      current_status: currentStatus,
      next_action: nextAction,
      action_path: actionPath,
      recent_at: recentAt ?? null,
      endpoint,
      detail,
      evidence,
      blockers: [],
      warnings: [],
      tone: this.readinessTone(status)
    };
  }

  private defaultReleaseChecklist(
    release: EndpointProjection<ReleaseReadinessResponse>,
    hardware: EndpointProjection<HardwareReadinessResponse>,
    harborOsStatus: EndpointProjection<HarborOsStatusResponse>,
    imCapabilityMap: EndpointProjection<HarborOsImCapabilityMapResponse>,
    localCatalog: EndpointProjection<LocalModelCatalogResponse>
  ): ReleaseReadinessChecklistItem[] {
    return [
      this.releaseChecklistItem(
        'release-readiness-api',
        this.text('Load release readiness API', '加载 release readiness API'),
        this.projectionReadiness(release),
        this.projectionDetail(release, this.text('Read the aggregated checklist and release status cards.', '读取聚合 checklist 与发布状态卡。')),
        release.endpoint,
        '/overview'
      ),
      this.releaseChecklistItem(
        'hardware-readiness-api',
        this.text('Confirm hardware readiness', '确认硬件 readiness'),
        this.projectionReadiness(hardware),
        this.projectionDetail(hardware, this.text('Read hardware readiness before declaring device workflows ready.', '声明设备工作流 ready 前先读取 hardware readiness。')),
        hardware.endpoint,
        '/devices-aiot'
      ),
      this.releaseChecklistItem(
        'harboros-status-api',
        this.text('Confirm HarborOS status', '确认 HarborOS 状态'),
        this.projectionReadiness(harborOsStatus),
        this.projectionDetail(harborOsStatus, this.text('Read HarborOS status without mixing it with HarborDesk shell availability.', '读取 HarborOS 状态，不与 HarborDesk shell 可用性混淆。')),
        harborOsStatus.endpoint,
        '/harboros'
      ),
      this.releaseChecklistItem(
        'im-capability-map-api',
        this.text('Confirm IM capability map', '确认 IM capability map'),
        this.projectionReadiness(imCapabilityMap),
        this.projectionDetail(imCapabilityMap, this.text('Check IM capability mapping while keeping route_key semantics opaque.', '检查 IM capability map，同时保持 route_key 语义 opaque。')),
        imCapabilityMap.endpoint,
        '/system-settings'
      ),
      this.releaseChecklistItem(
        'local-model-catalog-api',
        this.text('Confirm local model catalog', '确认本地模型清单'),
        this.projectionReadiness(localCatalog),
        this.projectionDetail(localCatalog, this.text('Read local catalog before model-dependent release validation.', '模型相关发布验收前先读取本地模型清单。')),
        localCatalog.endpoint,
        '/models-policies'
      ),
      this.releaseChecklistItem(
        'harbordesk-harboros-ui-boundary',
        this.text('Keep HarborDesk and HarborOS WebUI distinct', '区分 HarborDesk 与 HarborOS WebUI'),
        'ready',
        this.text('HarborDesk :4174 is not HarborOS WebUI /ui/ on 80/443.', 'HarborDesk :4174 不是 80/443 上的 HarborOS WebUI /ui/。'),
        'HarborDesk :4174; HarborOS WebUI /ui/ or 80/443',
        '/ui/'
      )
    ];
  }

  private releaseDeepLinks(extras: ReleaseReadinessDeepLink[]): ReleaseReadinessDeepLink[] {
    const links: ReleaseReadinessDeepLink[] = [
      {
        label: this.text('HarborDesk Overview (:4174)', 'HarborDesk 总览 (:4174)'),
        href: '/overview',
        detail: this.text('Angular admin shell route for release readiness.', '发布 readiness 所在的 Angular 后台 shell 路由。'),
        endpoint: 'HarborDesk :4174'
      },
      {
        label: this.text('Devices & AIoT', '设备与 AIoT'),
        href: '/devices-aiot',
        detail: this.text('Configure cameras, credentials, RTSP, snapshots, and share links.', '配置摄像头、凭据、RTSP、快照与分享链接。'),
        endpoint: 'GET /api/hardware/readiness'
      },
      {
        label: this.text('Models & Policies', '模型与策略'),
        href: '/models-policies',
        detail: this.text('Inspect local model catalog, endpoint status, route policies, and fallback order.', '检查本地模型清单、端点状态、路由策略与 fallback 顺序。'),
        endpoint: 'GET /api/models/local-catalog'
      },
      {
        label: 'HarborOS',
        href: '/harboros',
        detail: this.text('Read HarborOS status inside HarborDesk without treating it as the HarborOS WebUI.', '在 HarborDesk 内读取 HarborOS 状态，但不把它当作 HarborOS WebUI。'),
        endpoint: 'GET /api/harboros/status'
      },
      {
        label: 'HarborOS WebUI (/ui/)',
        href: '/ui/',
        detail: this.text('HarborOS WebUI is served by HarborOS on /ui/ or ports 80/443, separate from HarborDesk :4174.', 'HarborOS WebUI 由 HarborOS 在 /ui/ 或 80/443 提供，和 HarborDesk :4174 分开。'),
        endpoint: 'HarborOS WebUI /ui/ or 80/443'
      }
    ];
    return this.uniqueDeepLinks([...links, ...extras]);
  }

  private normalizedChecklistItem(item: ReleaseReadinessChecklistItem): ReleaseReadinessChecklistItem {
    return {
      ...item,
      status: this.normalizeReadinessStatus(item.status)
    };
  }

  private normalizedStatusCard(card: ReleaseReadinessStatusCard): ReleaseReadinessStatusCard {
    const status = this.normalizeReadinessStatus(card.status);
    return {
      ...card,
      status,
      tone: card.tone ?? this.readinessTone(status)
    };
  }

  private releaseStatusCard(
    id: string,
    label: string,
    value: string,
    status: ReleaseReadinessStatus,
    detail: string,
    endpoint: string,
    deepLink: string
  ): ReleaseReadinessStatusCard {
    return {
      id,
      label,
      value,
      status,
      detail,
      endpoint,
      deep_link: deepLink,
      tone: this.readinessTone(status)
    };
  }

  private releaseChecklistItem(
    id: string,
    label: string,
    status: ReleaseReadinessStatus,
    detail: string,
    endpoint: string,
    deepLink: string
  ): ReleaseReadinessChecklistItem {
    return {
      id,
      label,
      status,
      detail,
      endpoint,
      deep_link: deepLink
    };
  }

  private buildImGatewayState(gateway: GatewayStatusResponse): PageState<DeskPageModel> {
    const platformRows = this.platformRows(gateway);
    const blockers = this.imBlockers(gateway);
    const parityReady = gateway.parity_ready === true;
    const kind = platformRows.length === 0 ? 'blocker' : 'success';
    return {
      kind,
      detail: 'Feishu and Weixin are rendered as parallel surfaces. Source-bound and proactive delivery signals stay split.',
      data: {
        ...this.baseModel('im-gateway'),
        eyebrow: 'Transport and route surfaces',
        summary: 'HarborGate owns Feishu and Weixin transport readiness while HarborBeacon only consumes the redacted gateway status.',
        endpoint: 'GET /api/gateway/status',
        setupFlow: this.setupFlow(
          'Release-v1 IM setup flow',
          'Use this page to confirm that Weixin is healthy enough for the真人测试主链 and that replies remain source-bound.',
          [
            this.setupStep(
              'Weixin transport health',
              this.platformReady(gateway, 'weixin') ? 'ready' : 'needs-config',
              this.platformReady(gateway, 'weixin') ? 'Weixin can carry private-text ingress.' : 'Weixin still needs provider-side cleanup.',
              this.platformReady(gateway, 'weixin')
                ? `Gateway blocker taxonomy: ${this.weixinBlocker(gateway) ?? 'none'}.`
                : `Current blocker: ${this.weixinBlocker(gateway) ?? 'transport not yet ready'}.`,
              ['Keep Feishu as the baseline fallback.', 'Do not expand group-chat scope.']
            ),
            this.setupStep(
              'Source-bound vs proactive split',
              'ready',
              'Interactive replies stay on the source surface, while proactive delivery follows the default named target.',
              'The route policy is already frozen in the backend projection; the UI only explains it and surfaces any queue/failure separation.',
              ['Source-bound replies never auto-cross channels.', 'Proactive notifications stay route-key driven.']
            )
          ]
        ),
        metrics: [
          this.metric('Feishu baseline', this.platformReady(gateway, 'feishu') ? 'Ready' : 'Pending', 'Live-gate baseline readiness.', this.platformReady(gateway, 'feishu') ? 'good' : 'warn'),
          this.metric('Weixin parity track', this.platformReady(gateway, 'weixin') ? 'Ready' : 'Pending', 'Provider-side parity progression.', this.platformReady(gateway, 'weixin') ? 'good' : 'warn'),
          this.metric('Parity ready', parityReady ? 'Yes' : 'No', 'Only true when both surfaces satisfy the same rehearsal matrix.', parityReady ? 'good' : 'warn'),
          this.metric('Bridge transport', gateway.status || gateway.platform || 'unknown', 'Current transport health surfaced by HarborGate.', gateway.configured ? 'good' : 'warn')
        ],
        highlights: [
          'Interactive replies remain source-bound.',
          'Proactive delivery uses the default named target route_key.',
          `Manage IM in HarborGate: ${gateway.manage_url || 'not surfaced'}.`
        ],
        blockers,
        detailRows: platformRows,
        emptyNote: 'No platform rows were returned from HarborGate.',
        nextStep: blockers.length === 0 ? 'Use HarborGate live rehearsal to verify provider-side ingress.' : 'Focus on the listed Weixin blockers before re-running parity.'
      }
    };
  }

  private buildAccountManagementState(account: AccountManagementSnapshot): PageState<DeskPageModel> {
    const targets = account.notification_targets ?? [];
    const kind = targets.length === 0 ? 'empty' : 'success';
    const ownerUserId = account.workspace.owner_user_id;
    return {
      kind,
      detail: 'Workspace governance and named notification targets are loaded from the same-origin admin-plane.',
      data: {
        ...this.baseModel('account-management'),
        eyebrow: 'People and notification targets',
        summary: 'HarborBeacon keeps workspace governance local, while proactive IM routing points at HarborGate-owned opaque route keys.',
        endpoint: 'GET /api/account-management',
        setupFlow: this.setupFlow(
          'Notification target governance',
          'HarborDesk reuses the HarborOS login principal, but proactive routing now depends on named HarborGate targets instead of IM identity bindings.',
          [
            this.setupStep(
              'HarborOS principal reuse',
              ownerUserId ? 'ready' : 'blocked',
              ownerUserId ? 'The HarborOS owner baseline is available to HarborDesk.' : 'Owner principal is not projected yet.',
              ownerUserId
                ? `Workspace owner_user_id: ${ownerUserId}. The same-origin admin principal is expected to align with this identity.`
                : 'The backend must surface the current HarborOS user before the UI can treat this as release-v1 ready.',
              ['HarborDesk does not introduce a second local login.', 'This lane intentionally stays inside the OS account model.']
            ),
            this.setupStep(
              'Default notification target',
              targets.some((target) => target.is_default) ? 'ready' : 'needs-config',
              targets.some((target) => target.is_default)
                ? 'A named HarborGate target is selected as the proactive default.'
                : 'No default notification target is registered yet.',
              `Workspace default policy: ${account.delivery_policy.proactive_delivery}. ${targets.filter((target) => target.is_default).length} target(s) currently carry the default flag.`,
              ['Target labels are business-owned names.', 'The stored route_key stays opaque to HarborBeacon.']
            ),
            this.setupStep(
              'HarborGate IM ownership',
              account.gateway.manage_url ? 'ready' : 'needs-config',
              account.gateway.manage_url
                ? 'HarborGate is the only place that should manage IM login, QR flows, and target capture.'
                : 'HarborGate manage URL is not surfaced yet.',
              account.gateway.manage_url
                ? `Manage IM targets in HarborGate: ${account.gateway.manage_url}.`
                : 'Expose HarborGate manage_url before operators rely on this page for IM governance.',
              ['HarborBeacon only stores label + route_key + platform_hint.', 'Legacy identity bindings remain read-only context.']
            )
          ]
        ),
        metrics: [
          this.metric('Members', `${account.workspace.member_count}`, 'Workspace roster size.', account.workspace.member_count > 0 ? 'good' : 'warn'),
          this.metric('Active members', `${account.workspace.active_member_count}`, 'Members currently active in governance scope.', account.workspace.active_member_count > 0 ? 'good' : 'neutral'),
          this.metric('Notification targets', `${targets.length}`, 'Named HarborGate-owned route-key targets registered in HarborBeacon.', targets.length > 0 ? 'good' : 'warn'),
          this.metric('Permission rules', `${account.workspace.permission_rule_count}`, 'Approval and admin governance rules in force.', account.workspace.permission_rule_count > 0 ? 'good' : 'neutral')
        ],
        highlights: [
          `Workspace owner: ${account.workspace.owner_user_id}`,
          `HarborGate manage URL: ${account.gateway.manage_url || 'not surfaced'}`,
          `Interactive reply policy stays ${account.delivery_policy.interactive_reply}.`
        ],
        blockers: targets.length === 0 ? ['No notification target is registered yet. Capture one from HarborGate before relying on proactive delivery.'] : [],
        notificationTargets: targets,
        detailRows: targets.slice(0, 8).map((target) => ({
          title: target.label,
          subtitle: target.platform_hint || 'platform pending',
          meta: [
            `route_key: ${target.route_key}`,
            `default: ${target.is_default ? 'yes' : 'no'}`,
            'HarborBeacon stores this as an opaque HarborGate target.'
          ],
          tone: target.is_default ? 'good' : 'neutral'
        })),
        emptyNote: 'No notification targets are currently registered from HarborGate.',
        nextStep: 'Register a named target in HarborGate, then choose the default target from HarborBeacon.'
      }
    };
  }

  private buildTasksState(approvals: TaskApprovalSummary[]): PageState<DeskPageModel> {
    const kind = approvals.length === 0 ? 'empty' : 'success';
    return {
      kind,
      detail: 'Approval tickets are loaded directly from HarborBeacon task state and remain distinct from proactive delivery failures.',
      data: {
        ...this.baseModel('tasks-approvals'),
        eyebrow: 'Risk review and audit',
        summary: 'Interaction-linked replies and proactive notifications stay separate from approval state.',
        endpoint: 'GET /api/tasks/approvals',
        metrics: [
          this.metric('Pending approvals', `${approvals.length}`, 'Current number of approval tickets waiting for review.', approvals.length > 0 ? 'warn' : 'good'),
          this.metric(
            'High risk tickets',
            `${approvals.filter((item) => String(item.risk_level).toLowerCase() === 'high').length}`,
            'High-risk actions still require explicit approval.',
            approvals.some((item) => String(item.risk_level).toLowerCase() === 'high') ? 'warn' : 'good'
          )
        ],
        highlights: [
          'Approval tickets remain source-bound to their interaction chain.',
          'Queued or failed proactive delivery does not rewrite approval state.'
        ],
        blockers: [],
        detailRows: approvals.map((approval) => ({
          title: approval.intent_text || `${approval.domain}:${approval.action}`,
          subtitle: `${approval.domain} / ${approval.action}`,
          meta: [
            `risk: ${approval.risk_level}`,
            `surface: ${approval.surface}`,
            `channel: ${approval.source_channel}`,
            `conversation: ${approval.conversation_id}`
          ],
          tone: String(approval.risk_level).toLowerCase() === 'high' ? 'warn' : 'neutral'
        })),
        emptyNote: 'No approval tickets are waiting at the moment.',
        nextStep: approvals.length === 0 ? 'No review action is needed right now.' : 'Review the surfaced approval tickets before advancing the related workflow.'
      }
    };
  }

  private buildDevicesState(
    state: AdminStateResponse,
    shareLinks: ShareLinkSummary[] = [],
    evidenceProjections: Record<string, EndpointProjection<DeviceEvidenceResponse>> = {}
  ): PageState<DeskPageModel> {
    const devices = state.devices ?? [];
    const credentialStatuses = state.device_credential_statuses ?? [];
    const selectedDeviceId = state.defaults.selected_camera_device_id ?? null;
    const rtspReady = devices.filter((device) => this.deviceRtspConfigured(device)).length;
    const snapshotReady = devices.filter((device) => this.deviceSnapshotConfigured(device)).length;
    const credentialsConfigured = credentialStatuses.filter((status) => status.configured).length;
    const deviceEvidence = this.buildDeviceEvidencePanels(devices, evidenceProjections, credentialStatuses, shareLinks);
    const evidenceAvailable = Object.values(deviceEvidence).filter((panel) => panel.state === 'available').length;
    const kind = devices.length === 0 ? 'empty' : 'success';
    return {
      kind,
      detail: this.text(
        'Devices and AIoT are managed from the HarborBeacon Home Device Domain admin API.',
        '设备与 AIoT 通过 HarborBeacon Home Device Domain 后台 API 管理。'
      ),
      data: {
        ...this.baseModel('devices-aiot'),
        eyebrow: this.text('Home Device Domain', 'Home Device Domain'),
        summary: this.text(
          'Discover, add, select, test, share, and credential-check cameras here without moving AIoT ownership into HarborOS or HarborGate.',
          '在这里完成摄像头发现、手动添加、默认选择、RTSP/快照测试、分享链接和凭据状态管理；AIoT 不进入 HarborOS 或 HarborGate。'
        ),
        endpoint: 'GET /api/state + POST /api/devices/*',
        setupFlow: this.setupFlow(
          this.text('Release-v1 AIoT setup flow', 'AIoT 发布前配置流程'),
          this.text(
            'Use this page as the release configuration path for cameras: scan or add, save redacted credentials, pick the default camera, and verify RTSP/snapshot/share readiness.',
            '这里就是摄像头发布前配置路径：扫描或手动添加、保存 redacted 凭据、选择默认摄像头，并验证 RTSP/快照/分享 readiness。'
          ),
          [
            this.setupStep(
              this.text('Discovery or manual add', '发现扫描或手动添加'),
              devices.length > 0 ? 'ready' : 'needs-config',
              devices.length > 0
                ? this.text('At least one camera is registered.', '已注册至少一台摄像头。')
                : this.text('No cameras are registered yet.', '尚未注册摄像头。'),
              devices.length > 0
                ? this.text(
                  `${devices.length} device(s) are available. The controls below can still scan for more or add a known RTSP camera.`,
                  `当前已有 ${devices.length} 台设备；下方仍可继续扫描或添加已知 RTSP 摄像头。`
                )
                : this.text(
                  'Run discovery scan or add a known RTSP camera from this page before using IM camera workflows.',
                  '使用 IM 摄像头工作流前，先在本页发现或手动添加摄像头。'
                ),
              [
                this.text('Discovery and manual add stay in HarborBeacon AIoT/Admin API.', '发现和手动添加走 HarborBeacon AIoT/Admin API。'),
                this.text('No command-line or JSON editing is required.', '不需要编辑 JSON 或使用命令行。')
              ]
            ),
            this.setupStep(
              this.text('Default camera', '默认摄像头'),
              selectedDeviceId ? 'ready' : devices.length > 0 ? 'needs-config' : 'blocked',
              selectedDeviceId
                ? this.text('A default camera is selected for HarborBeacon workflows.', 'HarborBeacon 工作流已有默认摄像头。')
                : this.text('Default camera has not been selected yet.', '尚未选择默认摄像头。'),
              selectedDeviceId
                ? this.text(`Selected camera: ${selectedDeviceId}.`, `已选择摄像头：${selectedDeviceId}。`)
                : devices.length > 0
                  ? this.text('Choose one registered camera as the default below.', '在下方选择一台已注册摄像头作为默认摄像头。')
                  : this.text('Register a camera before selecting the default.', '先注册摄像头，再选择默认摄像头。'),
              [this.text('The selection is stored in HarborBeacon defaults and recording policy projection.', '默认选择保存到 HarborBeacon defaults 和记录策略投影。')]
            ),
            this.setupStep(
              this.text('RTSP and snapshot readiness', 'RTSP 与快照 readiness'),
              rtspReady > 0 || snapshotReady > 0 ? 'ready' : devices.length > 0 ? 'needs-config' : 'blocked',
              rtspReady > 0
                ? this.text('At least one registered camera exposes RTSP.', '至少一台已注册摄像头具备 RTSP。')
                : this.text('RTSP has not been verified for any device yet.', '尚未验证任何设备的 RTSP。'),
              this.text(
                `RTSP capable: ${rtspReady}. Snapshot capable/native: ${snapshotReady}. Use per-device actions to run RTSP checks and snapshot tests.`,
                `RTSP 可用：${rtspReady}；快照可用/原生：${snapshotReady}。使用每台设备的操作按钮执行 RTSP 检查和快照测试。`
              ),
              [
                this.text('RTSP checks use HarborBeacon AIoT adapters.', 'RTSP 检查使用 HarborBeacon AIoT adapter。'),
                this.text('Snapshot tasks stay device-domain operations.', '快照任务仍属于设备域操作。')
              ]
            ),
            this.setupStep(
              this.text('Credential status', '设备凭据状态'),
              credentialsConfigured > 0 ? 'ready' : devices.length > 0 ? 'needs-config' : 'blocked',
              credentialsConfigured > 0
                ? this.text('At least one device has a configured redacted credential record.', '至少一台设备已有 redacted 凭据记录。')
                : this.text('No per-device credential record is configured yet.', '尚未配置任何设备凭据记录。'),
              this.text(
                `Configured credential records: ${credentialsConfigured}/${devices.length}. Passwords are never rendered back to the UI.`,
                `已配置凭据记录：${credentialsConfigured}/${devices.length}。密码不会回显到界面。`
              ),
              [
                this.text('Credentials are stored through HarborBeacon AIoT/Admin API.', '设备凭据通过 HarborBeacon AIoT/Admin API 保存。'),
                this.text('HarborGate credentials remain IM-only.', 'HarborGate 凭据只用于 IM。')
              ]
            )
          ]
        ),
        metrics: [
          this.metric(this.text('Registered devices', '已注册设备'), `${devices.length}`, this.text('Current devices in the Home Device Domain registry.', 'Home Device Domain registry 中的当前设备。'), devices.length > 0 ? 'good' : 'warn'),
          this.metric(
            this.text('Default camera', '默认摄像头'),
            selectedDeviceId ? this.text('Selected', '已选择') : this.text('Pending', '待配置'),
            selectedDeviceId ?? this.text('Choose a default camera before release validation.', '发布验收前需要选择默认摄像头。'),
            selectedDeviceId ? 'good' : 'warn'
          ),
          this.metric(
            this.text('RTSP capable', 'RTSP 可用'),
            `${rtspReady}`,
            this.text('Devices with a stored RTSP stream URL or positive capability.', '已有 RTSP stream URL 或 capability 的设备数。'),
            rtspReady > 0 ? 'good' : 'warn'
          ),
          this.metric(
            this.text('Credential records', '凭据记录'),
            `${credentialsConfigured}/${devices.length}`,
            this.text('Redacted device credential status from HarborBeacon.', 'HarborBeacon 投影的 redacted 设备凭据状态。'),
            credentialsConfigured > 0 ? 'good' : devices.length > 0 ? 'warn' : 'neutral'
          ),
          this.metric(
            this.text('Share links', '分享链接'),
            `${shareLinks.filter((link) => link.status === 'active').length}/${shareLinks.length}`,
            this.text('Active/total camera share links known to the task conversation store.', '任务会话存储中的 active/total 摄像头分享链接。'),
            shareLinks.some((link) => link.status === 'active') ? 'good' : 'neutral'
          ),
          this.metric(
            this.text('Evidence endpoints', '证据接口'),
            `${evidenceAvailable}/${devices.length}`,
            this.text('Future per-device evidence projections loaded without rendering secrets.', '未来单设备证据投影加载状态；不渲染密钥。'),
            evidenceAvailable > 0 ? 'good' : devices.length > 0 ? 'warn' : 'neutral'
          )
        ],
        highlights: [
          this.text('AIoT device management is in HarborDesk Devices & AIoT.', 'AIoT 设备管理入口在 HarborDesk “设备与 AIoT”。'),
          this.text('HarborOS remains System Domain only.', 'HarborOS 页面只保留 System Domain。'),
          this.text('Device credentials render only as redacted configured status.', '设备凭据只显示 redacted/configured 状态。')
        ],
        blockers: [],
        detailRows: devices.map((device) => ({
          title: device.name,
          subtitle: `${device.room || this.text('unassigned room', '未分配房间')} / ${device.device_id}`,
          meta: [
            this.text(`status: ${device.status ?? 'unknown'}`, `状态：${device.status ?? 'unknown'}`),
            this.text(`ip: ${device.ip_address ?? 'pending'}`, `IP：${device.ip_address ?? '待配置'}`),
            this.text(`rtsp: ${this.deviceRtspConfigured(device) ? 'configured' : 'pending'}`, `RTSP：${this.deviceRtspConfigured(device) ? '已配置' : '待配置'}`),
            this.text(`snapshot: ${this.deviceSnapshotConfigured(device) ? 'ready' : 'ffmpeg fallback/pending'}`, `快照：${this.deviceSnapshotConfigured(device) ? 'ready' : 'ffmpeg fallback/待配置'}`),
            this.text(`credentials: ${credentialStatuses.find((status) => status.device_id === device.device_id)?.configured ? 'configured' : 'pending'}`, `凭据：${credentialStatuses.find((status) => status.device_id === device.device_id)?.configured ? '已配置' : '待配置'}`),
            this.text(`evidence endpoint: ${deviceEvidence[device.device_id]?.state ?? 'pending'}`, `证据接口：${deviceEvidence[device.device_id]?.state ?? '待接入'}`),
            this.text(`selected: ${selectedDeviceId === device.device_id ? 'yes' : 'no'}`, `默认：${selectedDeviceId === device.device_id ? '是' : '否'}`)
          ],
          tone: selectedDeviceId === device.device_id ? 'good' : this.deviceSnapshotConfigured(device) ? 'good' : 'neutral'
        })),
        devices,
        defaults: state.defaults,
        deviceCredentialStatuses: credentialStatuses,
        deviceEvidence,
        shareLinks,
        emptyNote: this.text('No devices are registered yet.', '尚未注册设备。'),
        nextStep: this.text(
          'Run discovery or manually add a camera, then set the default and run RTSP/snapshot checks.',
          '先执行发现扫描或手动添加摄像头，然后设置默认摄像头并运行 RTSP/快照检查。'
        )
      }
    };
  }

  private buildDeviceEvidencePanels(
    devices: CameraDevice[],
    evidenceProjections: Record<string, EndpointProjection<DeviceEvidenceResponse>>,
    credentialStatuses: DeviceCredentialStatus[],
    shareLinks: ShareLinkSummary[]
  ): Record<string, DeviceEvidencePanel> {
    return Object.fromEntries(
      devices.map((device) => {
        const deviceLinks = shareLinks.filter((link) => link.device_id === device.device_id);
        const activeLinks = deviceLinks.filter((link) => link.status === 'active');
        const credentialStatus = credentialStatuses.find((status) => status.device_id === device.device_id);
        const projection = evidenceProjections[device.device_id];
        const response = projection?.data;
        const endpoint = projection?.endpoint ?? `GET /api/devices/${device.device_id}/evidence`;
        const state = projection?.state ?? 'empty';
        const rtspResult = this.deviceEvidenceResult(response, 'rtsp_check');
        const snapshotResult = this.deviceEvidenceResult(response, 'snapshot');
        const shareLinkResult = this.deviceEvidenceResult(response, 'share_link');
        const credentialResult = this.deviceEvidenceResult(response, 'credential_status');
        const panel: DeviceEvidencePanel = {
          device_id: device.device_id,
          endpoint,
          state,
          summary: response?.summary ?? this.deviceEvidenceSummary(state),
          generated_at: response?.generated_at ?? response?.checked_at ?? null,
          fields: [
            this.deviceEvidenceField(
              'rtsp_check',
              this.text('RTSP check', 'RTSP 检查'),
              rtspResult,
              this.deviceRtspConfigured(device) ? 'configured' : 'pending',
              this.deviceRtspConfigured(device)
                ? this.text('RTSP capability is configured; stream URLs are intentionally hidden.', 'RTSP 能力已配置；stream URL 不在界面回显。')
                : this.text('RTSP has no recent positive evidence yet.', 'RTSP 暂无最近成功证据。'),
              response?.checked_at,
              response?.action_path ?? '/devices-aiot',
              endpoint
            ),
            this.deviceEvidenceField(
              'snapshot',
              this.text('Snapshot', '快照'),
              snapshotResult,
              this.deviceSnapshotConfigured(device) ? 'ready' : 'pending',
              this.deviceSnapshotConfigured(device)
                ? this.text('Snapshot capability or URL is projected; raw URL is hidden.', '已投影快照能力或 URL；原始 URL 不回显。')
                : this.text('Snapshot validation has not produced recent evidence yet.', '快照验收暂无最近证据。'),
              response?.checked_at,
              response?.action_path ?? '/devices-aiot',
              endpoint
            ),
            this.deviceEvidenceField(
              'share_link',
              this.text('Share-link', '分享链接'),
              shareLinkResult,
              activeLinks.length > 0 ? 'ready' : 'pending',
              this.text(
                `Active/total share links: ${activeLinks.length}/${deviceLinks.length}. Access URLs are not rendered.`,
                `分享链接 active/total：${activeLinks.length}/${deviceLinks.length}。访问 URL 不回显。`
              ),
              this.latestTimestamp(deviceLinks.map((link) => link.started_at ?? link.ended_at ?? link.expires_at)),
              response?.action_path ?? '/devices-aiot',
              endpoint
            ),
            this.deviceEvidenceField(
              'credential_status',
              this.text('Credential recent result', '凭据最近结果'),
              credentialResult,
              credentialStatus?.configured ? 'configured' : 'pending',
              credentialStatus?.configured
                ? this.text(
                  `Redacted credential record is configured from ${credentialStatus.source}; password and token values are never rendered.`,
                  `已配置来自 ${credentialStatus.source} 的 redacted 凭据记录；密码和 token 不回显。`
                )
                : this.text('No redacted credential record is configured yet.', '尚未配置 redacted 凭据记录。'),
              credentialStatus?.last_verified_at ?? credentialStatus?.updated_at ?? response?.checked_at,
              response?.action_path ?? '/devices-aiot',
              endpoint
            )
          ],
          blockers: response?.blockers ?? [],
          warnings: this.uniqueLines([
            ...(response?.warnings ?? []),
            ...(projection?.state === 'error' ? [projection.error ?? this.text('Device evidence endpoint failed.', '设备证据接口请求失败。')] : [])
          ])
        };
        return [device.device_id, panel] as const;
      })
    );
  }

  private deviceEvidenceSummary(state: ReleaseReadinessEndpointState): string {
    if (state === 'available') {
      return this.text('Per-device evidence is available.', '单设备证据已可用。');
    }
    if (state === 'error') {
      return this.text('Future evidence endpoint is not available yet; fallback readiness stays visible.', '未来 evidence 接口尚不可用；保留 fallback readiness 展示。');
    }
    return this.text('Waiting for future per-device evidence projection.', '等待未来单设备 evidence 投影。');
  }

  private deviceEvidenceResult(
    response: DeviceEvidenceResponse | undefined,
    key: DeviceEvidenceField['key']
  ): DeviceEvidenceResult | undefined {
    if (!response) {
      return undefined;
    }
    switch (key) {
      case 'rtsp_check':
        return response.rtsp_check ?? response.results?.find((result) => this.normalizedEvidenceKind(result.kind) === key);
      case 'snapshot':
        return response.snapshot ?? response.results?.find((result) => this.normalizedEvidenceKind(result.kind) === key);
      case 'share_link':
        return response.share_link ?? response.results?.find((result) => this.normalizedEvidenceKind(result.kind) === key);
      case 'credential_status':
        return response.credential_status ?? response.results?.find((result) => this.normalizedEvidenceKind(result.kind) === key);
      default:
        return undefined;
    }
  }

  private normalizedEvidenceKind(kind: string): string {
    return kind.trim().toLowerCase().replace(/-/g, '_');
  }

  private deviceEvidenceField(
    key: DeviceEvidenceField['key'],
    label: string,
    result: DeviceEvidenceResult | undefined,
    fallbackStatus: string,
    fallbackDetail: string,
    fallbackCheckedAt: string | null | undefined,
    fallbackActionPath: string,
    fallbackEndpoint: string
  ): DeviceEvidenceField {
    const status = String(result?.status ?? fallbackStatus);
    const normalizedStatus = this.normalizeReadinessStatus(status);
    return {
      key,
      label,
      status,
      detail: result?.summary ?? result?.detail ?? result?.error_message ?? fallbackDetail,
      tone: this.readinessTone(normalizedStatus),
      checked_at: result?.checked_at ?? result?.generated_at ?? fallbackCheckedAt ?? null,
      action_path: result?.action_path ?? fallbackActionPath,
      endpoint: result?.endpoint ?? fallbackEndpoint
    };
  }

  private buildHarborOsState(state: AdminStateResponse): PageState<DeskPageModel> {
    return {
      kind: 'blocker',
      detail: 'HarborOS remains deliberately separate: the Angular app now has same-origin admin delivery, but a live HarborOS summary projection has not been published yet.',
      data: {
        ...this.baseModel('harboros'),
        eyebrow: 'System-domain summary',
        summary: 'This page distinguishes live status from proof summaries and refuses to invent HarborOS telemetry.',
        endpoint: 'Blocked pending HarborOS summary projection',
        setupFlow: this.setupFlow(
          'Release-v1 HarborOS setup flow',
          'HarborOS is the install target, so the page only exposes what the backend already projects and clearly flags what is still missing.',
          [
            this.setupStep(
              'Writable root projection',
              state.writable_root ? 'read-only' : 'blocked',
              state.writable_root ? 'The writable root is projected from the same-origin admin-plane.' : 'The UI does not yet receive a concrete writable-root field from the admin-plane.',
              state.writable_root
                ? `Writable root: ${state.writable_root}. Capture subdirectory: ${state.defaults.capture_subdirectory || 'pending'}.`
                : 'Keep the capture target inside the currently verified HarborOS install root once the backend projects it; until then the page stays read-only.',
              state.writable_root
                ? ['No invented /mnt path is shown here.', 'The release-v1 capture target remains bounded inside the approved HarborOS root.']
                : ['No invented /mnt path is shown here.', 'This is intentionally a blocker until the backend exposes the root.']
            ),
            this.setupStep(
              'Same-origin admin principal',
              state.account_management?.workspace?.current_principal_user_id ? 'ready' : 'blocked',
              state.account_management?.workspace?.current_principal_user_id ? 'HarborDesk follows the HarborOS account model.' : 'The owner principal is not projected yet.',
              state.account_management?.workspace?.current_principal_user_id
                ? `Current principal: ${state.account_management.workspace.current_principal_display_name || state.account_management.workspace.current_principal_user_id}. Owner baseline: ${state.account_management.workspace.owner_user_id}.`
                : 'Without the owner projection, HarborDesk cannot claim to be fully OS-native yet.',
              ['The admin UI should feel like part of HarborOS, not a separate SaaS.', 'No second login surface is introduced.']
            )
          ]
        ),
        metrics: [
          this.metric('Route order', 'middleware -> midcli -> browser/mcp', 'Frozen HarborOS fallback order.', 'good'),
          this.metric('Writable root', state.writable_root || 'blocked', 'Current HarborOS writable root projection.', state.writable_root ? 'good' : 'warn'),
          this.metric('Recording label', state.defaults.recording, 'Current recording projection visible from HarborBeacon state.', 'neutral'),
          this.metric('Capture label', state.defaults.capture, 'Current capture projection visible from HarborBeacon state.', 'neutral')
        ],
        highlights: [
          'HarborOS does not own IM routing.',
          'HarborOS does not take over Home Device Domain ownership.',
          state.writable_root ? `Writable root: ${state.writable_root}` : 'Writable root projection pending.'
        ],
        blockers: state.writable_root ? [] : ['A same-origin HarborOS summary block is still pending on the HarborBeacon admin-plane.'],
        emptyNote: 'HarborOS live summary is not yet projected.',
        nextStep: 'Publish the HarborOS summary block through the admin-plane before exposing control actions here.'
      }
    };
  }

  private buildModelsState(
    endpoints: ModelEndpointsResponse,
    policies: ModelPoliciesResponse,
    availability: FeatureAvailabilityResponse,
    ragReadiness: EndpointProjection<RagReadinessResponse>,
    knowledgeSettings: KnowledgeSettings,
    knowledgeIndexStatus: KnowledgeIndexStatusResponse,
    knowledgeIndexJobs: EndpointProjection<KnowledgeIndexJobsResponse>,
    localCatalog: EndpointProjection<LocalModelCatalogResponse>,
    localDownloads: EndpointProjection<LocalModelDownloadsResponse>
  ): PageState<DeskPageModel> {
    const endpointRows = endpoints.endpoints ?? [];
    const featureGroups = availability.groups ?? [];
    const retrievalFeatures = this.findFeatureGroup(featureGroups, 'retrieval')?.items ?? [];
    const availableRetrievalCount = retrievalFeatures.filter((item) => item.status === 'available').length;
    const runtimeAlignment = this.buildRuntimeAlignmentSummary(endpointRows);
    const enabledRoots = (knowledgeSettings.source_roots ?? []).filter((root) => root.enabled);
    const existingRoots = (knowledgeIndexStatus.source_roots ?? []).filter((root) => root.enabled && root.exists).length;
    const rag = ragReadiness.data;
    const jobs = knowledgeIndexJobs.data?.jobs ?? rag?.index_jobs ?? [];
    const modelDownloads = localDownloads.data?.downloads ?? localDownloads.data?.jobs ?? localCatalog.data?.downloads ?? [];
    const activeDownloads = modelDownloads.filter((job) => ['queued', 'running', 'downloading'].includes(String(job.status))).length;
    const activeJobs = jobs.filter((job) => ['queued', 'running'].includes(String(job.status))).length;
    const kind = endpointRows.length === 0 ? 'empty' : 'success';
    return {
      kind,
      detail: 'Model Center now keeps runtime truth, endpoint projection, route-policy inventory, and knowledge-source readiness on the same page.',
      data: {
        ...this.baseModel('models-policies'),
        eyebrow: 'Model center and retrieval operations',
        summary: 'Runtime alignment, multimodal readiness, knowledge roots, endpoint state, and route-policy control now share the same HarborDesk page.',
        endpoint: 'GET /api/models/endpoints + /api/models/policies + /api/models/local-catalog + /api/models/local-downloads + /api/knowledge/settings + /api/rag/readiness',
        setupFlow: this.setupFlow(
          'Knowledge & multimodal setup flow',
          'The setup flow exposes OCR, VLM, embedding, source roots, and local index storage using persisted HarborDesk settings.',
          [
            this.setupStep(
              'Runtime alignment',
              runtimeAlignment?.status === 'aligned' ? 'ready' : endpointRows.length > 0 ? 'read-only' : 'blocked',
              runtimeAlignment?.status === 'aligned'
                ? 'The persisted endpoint projection matches the current local runtime.'
                : runtimeAlignment
                  ? 'The page is surfacing a projection mismatch instead of hiding it.'
                  : 'Local endpoint inventory is not projected yet.',
              runtimeAlignment?.detail ?? 'Register endpoints before HarborDesk can compare runtime truth against the stored endpoint projection.',
              runtimeAlignment
                ? ['Use Runtime alignment as the first stop before editing endpoint metadata.', 'Projection mismatch means runtime truth is overruling stale admin state.']
                : ['No local endpoint projection is available yet.']
            ),
            this.setupStep(
              'Feature availability',
              retrievalFeatures.some((item) => item.status === 'available') ? 'ready' : retrievalFeatures.length > 0 ? 'needs-config' : 'blocked',
              retrievalFeatures.some((item) => item.status === 'available')
                ? 'At least one retrieval feature is confirmed available from live runtime and policy state.'
                : retrievalFeatures.length > 0
                  ? 'Feature rows are projected, but none are green yet.'
                  : 'Feature availability has not been projected yet.',
              retrievalFeatures.length > 0
                ? `Retrieval features available: ${availableRetrievalCount}/${retrievalFeatures.length}. Use the grouped cards below to inspect OCR, embed, answer, and vision availability.`
                : 'Expose feature availability before trying to make model-center decisions from this page.',
              ['VLM-first stays the multimodal priority when a real VLM endpoint exists.', 'Audio and full video understanding remain pending.']
            ),
            this.setupStep(
              'Knowledge source roots',
              enabledRoots.length > 0 && existingRoots > 0 ? 'ready' : enabledRoots.length > 0 ? 'needs-config' : 'blocked',
              enabledRoots.length > 0
                ? `${enabledRoots.length} source root(s) are enabled; ${existingRoots} exist on this host.`
                : 'No source root is enabled yet.',
              enabledRoots.length > 0
                ? 'Search and benchmark requests can only narrow within these configured roots.'
                : 'Use the Knowledge & Multimodal panel below to add a HarborOS file-manager folder before searching.',
              ['The /mnt/MM-test folder is not loaded until it is added here.', 'Request roots cannot expand beyond enabled source roots.']
            ),
            this.setupStep(
              'Index storage',
              knowledgeIndexStatus.index_root_writable ? 'ready' : 'needs-config',
              knowledgeSettings.index_root || 'Index root is not configured.',
              knowledgeIndexStatus.index_root_writable
                ? 'Manifest, chunks, embeddings, vector cache, and reports stay under this index root.'
                : 'Choose an index root that is outside every source root and writable on this host.',
              [rag?.embedding_model?.summary ?? 'Embedding model readiness is pending.', rag?.media_parser?.summary ?? 'OCR/VLM media parser readiness is pending.']
            )
          ]
        ),
        metrics: [
          this.metric('Endpoints', `${endpointRows.length}`, 'Visible model endpoints in the current workspace.', endpointRows.length > 0 ? 'good' : 'warn'),
          this.metric(
            'Runtime alignment',
            runtimeAlignment?.status ?? 'unavailable',
            runtimeAlignment?.detail ?? 'No local runtime projection is available yet.',
            runtimeAlignment?.tone ?? 'warn'
          ),
          this.metric(
            'Retrieval features',
            `${availableRetrievalCount}/${retrievalFeatures.length || 0}`,
            'Grouped feature availability keeps runtime truth and route-policy state visible together.',
            availableRetrievalCount > 0 ? 'good' : 'warn'
          ),
          this.metric(
            'Knowledge roots',
            `${existingRoots}/${enabledRoots.length}`,
            'Only enabled configured roots are eligible for knowledge.search.',
            enabledRoots.length > 0 && existingRoots > 0 ? 'good' : 'warn'
          ),
          this.metric(
            'Index root',
            knowledgeIndexStatus.index_root_writable ? 'writable' : 'needs config',
            `${knowledgeSettings.index_root || 'No index root configured.'} Manifests: ${knowledgeIndexStatus.manifest_count ?? 0}; embeddings: ${knowledgeIndexStatus.embedding_cache_count ?? 0}.`,
            knowledgeIndexStatus.index_root_writable ? 'good' : 'warn'
          ),
          this.metric(
            'Model downloads',
            `${activeDownloads}/${modelDownloads.length}`,
            localCatalog.data
              ? `Catalog entries: ${localCatalog.data.models?.length ?? 0}; cache roots: ${(localCatalog.data.cache_roots ?? []).join(', ') || 'none'}.`
              : `Catalog endpoint: ${localCatalog.state}.`,
            activeDownloads > 0 ? 'warn' : modelDownloads.length > 0 ? 'neutral' : 'warn'
          ),
          this.metric(
            'Index jobs',
            `${activeJobs}/${jobs.length}`,
            jobs.length > 0 ? `Recent index jobs visible from HarborBeacon admin API. Last indexed: ${knowledgeIndexStatus.last_indexed_at || 'never'}.` : 'No index jobs have been recorded yet.',
            activeJobs > 0 ? 'warn' : jobs.length > 0 ? 'neutral' : 'warn'
          ),
          this.metric('Policies', `${policies.route_policies.length}`, 'Route policies exposed by the admin-plane.', policies.route_policies.length > 0 ? 'good' : 'neutral')
        ],
        highlights: [
          'Projection mismatch stays visible instead of being silently flattened into the stored admin state.',
          'Endpoint tests are operator actions, not hidden background probes.',
          'Knowledge retrieval uses configured roots and a separate local index root.'
        ],
        blockers: [
          ...this.featureBlockers(featureGroups),
          ...(knowledgeIndexStatus.blockers ?? []),
          ...(rag?.blockers ?? []),
          ...(localCatalog.data?.blockers ?? []),
          ...(localDownloads.data?.blockers ?? [])
        ],
        modelEndpoints: endpointRows,
        modelPolicies: policies.route_policies,
        knowledgeSettings,
        knowledgeIndexStatus,
        knowledgeIndexJobs: jobs,
        localModelCatalog: localCatalog.data,
        localModelDownloads: modelDownloads,
        ragReadiness: rag,
        featureGroups,
        runtimeAlignment,
        detailRows: [
          ...(knowledgeIndexStatus.source_roots ?? []).map((root) => ({
            title: root.label || root.root_id,
            subtitle: root.path,
            meta: [
              `enabled: ${root.enabled}`,
              `exists: ${root.exists}`,
              `status: ${root.status}`,
              `last indexed: ${root.last_indexed_at || 'never'}`
            ],
            tone: root.enabled && root.exists ? 'good' as MetricTone : 'warn' as MetricTone
          })),
          ...policies.route_policies.map((policy) => ({
            title: policy.route_policy_id,
            subtitle: `${policy.domain_scope} / ${policy.modality}`,
            meta: [
              `privacy: ${policy.privacy_level}`,
              `status: ${policy.status}`,
              `fallback: ${policy.fallback_order.join(' -> ') || 'none'}`
            ],
            tone: (policy.local_preferred ? 'good' : 'neutral') as MetricTone
          }))
        ],
        emptyNote: 'No model endpoints are projected yet.',
        nextStep: endpointRows.length === 0
          ? 'Register model endpoints before operating the model center.'
          : 'Use Runtime alignment first, then inspect feature availability and fallback ordering before changing endpoint metadata.'
      }
    };
  }

  private buildSystemSettingsState(
    state: AdminStateResponse,
    gateway: GatewayStatusResponse,
    availability: FeatureAvailabilityResponse
  ): PageState<DeskPageModel> {
    const gatewayBaseUrl = state.bridge_provider.gateway_base_url || gateway.gateway_base_url || 'not configured';
    const manageUrl = gateway.manage_url || state.account_management?.gateway?.manage_url || 'not surfaced';
    const featureGroups = availability.groups ?? [];
    const interactiveReply = this.findFeatureItem(featureGroups, 'interactive_reply');
    const proactiveDelivery = this.findFeatureItem(featureGroups, 'proactive_delivery');
    const bindingAvailability = this.findFeatureItem(featureGroups, 'binding_availability');
    return {
      kind: 'success',
      detail: 'System settings now combines backend-backed routing metadata with the grouped feature-availability read model.',
      data: {
        ...this.baseModel('system-settings'),
        eyebrow: 'Routing and gateway policy',
        summary: 'This page exposes the frozen reply/delivery policy and the grouped read-model that says which options are really usable right now.',
        endpoint: 'GET /api/state + /api/gateway/status + /api/feature-availability',
        setupFlow: this.setupFlow(
          'Release-v1 system setup flow',
          'This page keeps the OS-install contract honest: only backend-backed settings are surfaced, and feature status is derived from real routing, binding, and gateway signals.',
          [
            this.setupStep(
              'Gateway linkage',
              gatewayBaseUrl === 'not configured' ? 'needs-config' : 'ready',
              gatewayBaseUrl === 'not configured' ? 'HarborGate base URL still needs to be configured.' : 'HarborGate is reachable from the same-origin admin UI.',
              `Gateway base URL: ${gatewayBaseUrl}.`,
              ['Use this as the single source of truth for same-origin gateway status.']
            ),
            this.setupStep(
              'Reply / delivery option readiness',
              interactiveReply?.status === 'available' && proactiveDelivery?.status !== 'blocked' ? 'ready' : 'needs-config',
              interactiveReply?.status === 'available'
                ? 'Interactive reply is live, and proactive delivery readiness is derived from the same frozen delivery policy.'
                : 'At least one delivery option still needs configuration or gateway cleanup.',
              `Interactive reply=${interactiveReply?.status ?? 'unknown'}, proactive delivery=${proactiveDelivery?.status ?? 'unknown'}, binding availability=${bindingAvailability?.status ?? 'unknown'}.`,
              ['Use Feature availability below to see the exact blocker and source of truth for each option.']
            ),
            this.setupStep(
              'Writable root and capture target',
              state.writable_root && state.defaults.capture_subdirectory ? 'read-only' : 'blocked',
              state.writable_root && state.defaults.capture_subdirectory
                ? 'The current admin-plane projection exposes both writable root and capture subdirectory.'
                : 'The current admin-plane projection does not expose a writable-root or capture_subdir field together.',
              state.writable_root && state.defaults.capture_subdirectory
                ? `Capture target: ${state.writable_root}/${state.defaults.capture_subdirectory}.`
                : 'Keep the capture target read-only until HarborOS exposes the actual storage root and capture subdirectory separately.',
              ['This is the exact spot where release-v1 should not fake data.', 'Use the blocker as a reminder to backfill the projection later.']
            )
          ]
        ),
        metrics: [
          this.metric(
            'Interactive reply',
            interactiveReply?.status ?? state.delivery_policy.interactive_reply,
            interactiveReply?.current_option || 'Replies follow the original source surface.',
            interactiveReply ? this.featureTone(interactiveReply.status) : 'good'
          ),
          this.metric(
            'Proactive delivery',
            proactiveDelivery?.status ?? state.delivery_policy.proactive_delivery,
            proactiveDelivery?.blocker || proactiveDelivery?.current_option || 'Independent notifications follow the default named HarborGate target.',
            proactiveDelivery ? this.featureTone(proactiveDelivery.status) : 'good'
          ),
          this.metric(
            'Binding availability',
            bindingAvailability?.status ?? `${state.account_management?.workspace?.identity_binding_count ?? 0}`,
            bindingAvailability?.current_option || 'Current HarborGate-owned identity binding projection.',
            bindingAvailability ? this.featureTone(bindingAvailability.status) : 'neutral'
          ),
          this.metric('Gateway base URL', gatewayBaseUrl, 'Current HarborGate admin/status origin.', gatewayBaseUrl === 'not configured' ? 'warn' : 'neutral')
        ],
        highlights: [
          `HarborGate manage URL: ${manageUrl}`,
          `Gateway status: ${state.bridge_provider.status}`
        ],
        blockers: this.uniqueLines([...this.imBlockers(gateway), ...this.featureBlockers(featureGroups)]),
        featureGroups,
        detailRows: [
          {
            title: 'Default scan CIDR',
            subtitle: state.defaults.cidr,
            meta: [
              `discovery: ${state.defaults.discovery}`,
              `recording: ${state.defaults.recording}`,
              `capture: ${state.defaults.capture}`,
              `ai: ${state.defaults.ai}`,
              `selected camera: ${state.defaults.selected_camera_device_id || 'pending'}`,
              `capture dir: ${state.defaults.capture_subdirectory || 'pending'}`,
              `clip length: ${state.defaults.clip_length_seconds ?? 'pending'}`
            ],
            tone: 'neutral'
          }
        ],
        emptyNote: 'No settings metadata available.',
        nextStep: 'Use Feature availability to decide which options are actually usable before touching gateway or delivery settings.'
      }
    };
  }

  private endpointStatus<T>(projection: EndpointProjection<T>): ReleaseReadinessEndpointStatus {
    if (projection.state === 'error') {
      return {
        endpoint: projection.endpoint,
        state: projection.state,
        detail: projection.error ?? this.text('Endpoint request failed.', '接口请求失败。')
      };
    }
    if (projection.state === 'empty') {
      return {
        endpoint: projection.endpoint,
        state: projection.state,
        detail: this.text('Endpoint returned an empty readiness projection.', '接口返回了空 readiness 投影。')
      };
    }
    return {
      endpoint: projection.endpoint,
      state: projection.state,
      detail: this.text('Live response available.', '实时响应可用。')
    };
  }

  private releasePanelStatus(
    status: ReleaseReadinessStatus | undefined,
    checklist: ReleaseReadinessChecklistItem[],
    blockers: string[]
  ): ReleaseReadinessStatus {
    if (status) {
      return this.normalizeReadinessStatus(status);
    }
    if (blockers.length > 0 || checklist.some((item) => item.status === 'blocked')) {
      return 'blocked';
    }
    if (checklist.some((item) => item.status === 'running')) {
      return 'running';
    }
    if (checklist.some((item) => item.status === 'needs-config' || item.status === 'unknown')) {
      return 'needs-config';
    }
    return checklist.length > 0 ? 'ready' : 'unknown';
  }

  private releasePanelSummary(
    release: EndpointProjection<ReleaseReadinessResponse>,
    errorCount: number,
    empty: boolean
  ): string {
    if (release.state === 'available' && !empty) {
      return this.text(
        'Release readiness is loaded from the Phase 2 readiness API.',
        'Release readiness 已从 Phase 2 readiness API 加载。'
      );
    }
    if (empty) {
      return this.text(
        'The readiness API is reachable but has not populated checklist or status-card data yet.',
        'readiness API 可达，但尚未填充 checklist 或状态卡数据。'
      );
    }
    if (errorCount > 0) {
      return this.text(
        'Some Phase 2 readiness endpoints are not available yet; the shell keeps their errors visible without blocking the rest of Overview.',
        '部分 Phase 2 readiness 接口尚不可用；shell 会显示这些错误，但不阻塞总览其他内容。'
      );
    }
    return this.text(
      'Release readiness shell is waiting for backend readiness projections.',
      '发布 readiness shell 正在等待后端 readiness 投影。'
    );
  }

  private projectionValue<T extends { status?: ReleaseReadinessStatus }>(projection: EndpointProjection<T>): string {
    if (projection.state === 'error') {
      return this.text('Error', '错误');
    }
    if (projection.state === 'empty') {
      return this.text('Empty', '空');
    }
    return projection.data?.status ? this.normalizeReadinessStatus(projection.data.status) : this.text('Available', '可用');
  }

  private projectionReadiness<T extends { status?: ReleaseReadinessStatus }>(projection: EndpointProjection<T>): ReleaseReadinessStatus {
    if (projection.state === 'error') {
      return 'blocked';
    }
    if (projection.state === 'empty') {
      return 'unknown';
    }
    return this.normalizeReadinessStatus(projection.data?.status);
  }

  private optionalProjectionReadiness<T extends { status?: ReleaseReadinessStatus }>(projection: EndpointProjection<T>): ReleaseReadinessStatus {
    if (projection.state === 'error' || projection.state === 'empty') {
      return 'unknown';
    }
    return this.normalizeReadinessStatus(projection.data?.status);
  }

  private combinedReadiness(statuses: ReleaseReadinessStatus[]): ReleaseReadinessStatus {
    if (statuses.includes('blocked')) {
      return 'blocked';
    }
    if (statuses.includes('running')) {
      return 'running';
    }
    if (statuses.includes('needs-config')) {
      return 'needs-config';
    }
    if (statuses.includes('unknown')) {
      return 'unknown';
    }
    return statuses.length > 0 ? 'ready' : 'unknown';
  }

  private projectionDetail<T>(projection: EndpointProjection<T>, fallback: string): string {
    if (projection.state === 'error') {
      return projection.error ?? this.text('Endpoint request failed.', '接口请求失败。');
    }
    if (projection.state === 'empty') {
      return this.text('Endpoint is reachable but returned no actionable readiness data yet.', '接口可达，但尚未返回可操作的 readiness 数据。');
    }
    const maybeSummary = (projection.data as { summary?: unknown } | undefined)?.summary;
    return typeof maybeSummary === 'string' && maybeSummary.trim() ? maybeSummary : fallback;
  }

  private normalizeReadinessStatus(status?: string | null): ReleaseReadinessStatus {
    const normalized = String(status ?? 'unknown').trim().toLowerCase().replace(/_/g, '-');
    switch (normalized) {
      case 'ready':
      case 'ok':
      case 'healthy':
      case 'available':
      case 'completed':
      case 'configured':
      case 'passed':
      case 'success':
      case 'reachable':
        return 'ready';
      case 'needs-config':
      case 'needs-configuration':
      case 'not-configured':
      case 'degraded':
      case 'pending':
      case 'empty':
      case 'skipped':
        return 'needs-config';
      case 'blocked':
      case 'failed':
      case 'error':
      case 'unavailable':
        return 'blocked';
      case 'running':
      case 'queued':
      case 'accepted':
      case 'downloading':
        return 'running';
      default:
        return 'unknown';
    }
  }

  private latestTimestamp(values: Array<string | null | undefined>): string | null {
    const timestamps = values.filter((value): value is string => typeof value === 'string' && value.trim().length > 0);
    if (timestamps.length === 0) {
      return null;
    }
    return timestamps.sort()[timestamps.length - 1];
  }

  private readinessTone(status: ReleaseReadinessStatus): MetricCard['tone'] {
    switch (status) {
      case 'ready':
        return 'good';
      case 'blocked':
        return 'danger';
      case 'running':
      case 'needs-config':
        return 'warn';
      case 'unknown':
      default:
        return 'neutral';
    }
  }

  private isEmptyPayload(data: unknown): boolean {
    if (data === null || data === undefined) {
      return true;
    }
    if (Array.isArray(data)) {
      return data.length === 0;
    }
    if (typeof data !== 'object') {
      return false;
    }
    const entries = Object.entries(data as Record<string, unknown>);
    if (entries.length === 0) {
      return true;
    }
    return entries.every(([, value]) => {
      if (value === null || value === undefined) {
        return true;
      }
      if (Array.isArray(value)) {
        return value.length === 0;
      }
      return false;
    });
  }

  private uniqueDeepLinks(links: ReleaseReadinessDeepLink[]): ReleaseReadinessDeepLink[] {
    const seen = new Set<string>();
    return links.filter((link) => {
      const key = `${link.label}::${link.href}`;
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    });
  }

  private platformRows(gateway: GatewayStatusResponse): DeskRow[] {
    if (Array.isArray(gateway.platforms) && gateway.platforms.length > 0) {
      return gateway.platforms.map((platform) => this.platformRow(platform, gateway));
    }
    if (gateway.platform || gateway.status) {
      return [
        {
          title: gateway.platform || 'gateway',
          subtitle: gateway.status || 'status unavailable',
          meta: [
            `configured: ${String(gateway.configured ?? false)}`,
            `connected: ${String(gateway.connected ?? false)}`
          ],
          tone: gateway.connected ? 'good' : 'warn'
        }
      ];
    }
    return [];
  }

  private deviceRtspConfigured(device: { primary_stream?: { url?: string; transport?: string }; profile?: { rtsp_url?: string }; capabilities?: { stream?: boolean } }): boolean {
    return Boolean(
      device.profile?.rtsp_url ||
      device.primary_stream?.url ||
      device.primary_stream?.transport === 'rtsp' ||
      device.capabilities?.stream
    );
  }

  private deviceSnapshotConfigured(device: { snapshot_url?: string | null; profile?: { snapshot_url?: string | null }; capabilities?: { snapshot?: boolean } }): boolean {
    return Boolean(device.snapshot_url || device.profile?.snapshot_url || device.capabilities?.snapshot);
  }

  private platformRow(platform: GatewayPlatformStatus, gateway: GatewayStatusResponse): DeskRow {
    const readiness = platform.platform === 'feishu'
      ? gateway.feishu?.rehearsal_ready
      : platform.platform === 'weixin'
        ? gateway.weixin?.rehearsal_ready
        : undefined;
    const meta = [
      `enabled: ${String(platform.enabled ?? false)}`,
      `connected: ${String(platform.connected ?? false)}`
    ];
    if (platform.platform === 'weixin') {
      meta.push(`blocker: ${this.weixinBlocker(gateway) ?? 'none'}`);
    }
    return {
      title: platform.display_name || platform.platform,
      subtitle: readiness === true ? 'rehearsal_ready=true' : 'rehearsal_ready=false',
      meta,
      tone: readiness === true ? 'good' : platform.connected ? 'neutral' : 'warn'
    };
  }

  private platformReady(gateway: GatewayStatusResponse, platform: 'feishu' | 'weixin'): boolean {
    if (platform === 'feishu') {
      return gateway.feishu?.rehearsal_ready === true;
    }
    return gateway.weixin?.rehearsal_ready === true;
  }

  private weixinBlocker(gateway: GatewayStatusResponse): string | undefined {
    return gateway.weixin?.blocker_category || gateway.weixin_blocker_category;
  }

  private imBlockers(gateway: GatewayStatusResponse): string[] {
    const blockers: string[] = [];
    const weixinBlocker = this.weixinBlocker(gateway);
    if (weixinBlocker) {
      blockers.push(`Weixin blocker: ${weixinBlocker}`);
    }
    return blockers;
  }

  private featureTone(status: FeatureAvailabilityStatus): MetricCard['tone'] {
    switch (status) {
      case 'available':
        return 'good';
      case 'blocked':
        return 'danger';
      case 'degraded':
      case 'not_configured':
      default:
        return 'warn';
    }
  }

  private findFeatureGroup(groups: FeatureAvailabilityGroup[], groupId: string): FeatureAvailabilityGroup | undefined {
    return groups.find((group) => group.group_id === groupId);
  }

  private findFeatureItem(groups: FeatureAvailabilityGroup[], featureId: string): FeatureAvailabilityItem | undefined {
    return groups.flatMap((group) => group.items).find((item) => item.feature_id === featureId);
  }

  private featureBlockers(groups: FeatureAvailabilityGroup[]): string[] {
    return this.uniqueLines(
      groups
        .flatMap((group) => group.items)
        .filter((item) => item.status === 'blocked' && item.blocker)
        .map((item) => `${item.label}: ${item.blocker}`)
    );
  }

  private buildRuntimeAlignmentSummary(endpointRows: ModelEndpointRecord[]): DeskPageModel['runtimeAlignment'] {
    const runtimeRows = endpointRows.filter((endpoint) =>
      ['llm-local-openai-compatible', 'embed-local-openai-compatible', 'vlm-local-openai-compatible'].includes(endpoint.model_endpoint_id)
    );
    if (runtimeRows.length === 0) {
      return undefined;
    }

    const mismatchedRows = runtimeRows.filter((endpoint) => this.metadataBoolean(endpoint, 'projection_mismatch'));
    return {
      status: mismatchedRows.length > 0 ? 'projection_mismatch' : 'aligned',
      detail:
        mismatchedRows.length > 0
          ? `Live runtime is overriding stale admin endpoint state for ${mismatchedRows.length} built-in endpoint${mismatchedRows.length === 1 ? '' : 's'}.`
          : 'The stored endpoint projection matches the current local runtime signals.',
      tone: mismatchedRows.length > 0 ? 'warn' : 'good',
      rows: runtimeRows.map((endpoint) => {
        const runtimeKind = this.metadataString(endpoint, 'runtime_backend_kind') || endpoint.provider_key || 'runtime';
        const meta = [
          `status: ${endpoint.status}`,
          `provider: ${endpoint.provider_key}`,
          `runtime backend: ${runtimeKind}`,
          `base_url: ${this.metadataString(endpoint, 'base_url') || 'n/a'}`,
          `healthz_url: ${this.metadataString(endpoint, 'healthz_url') || 'n/a'}`,
          `api_key_configured: ${String(this.metadataBoolean(endpoint, 'api_key_configured'))}`
        ];
        const mismatchReason = this.metadataString(endpoint, 'projection_mismatch_reason');
        if (mismatchReason) {
          meta.push(`projection mismatch: ${mismatchReason}`);
        }
        return {
          title: endpoint.model_endpoint_id,
          subtitle: `${endpoint.model_kind} / ${runtimeKind}`,
          meta,
          tone: this.metadataBoolean(endpoint, 'projection_mismatch') ? 'warn' : endpoint.status === 'active' ? 'good' : 'neutral'
        };
      })
    };
  }

  private metadataString(endpoint: ModelEndpointRecord, key: string): string | null {
    const value = endpoint.metadata?.[key];
    return typeof value === 'string' && value.trim() ? value : null;
  }

  private metadataBoolean(endpoint: ModelEndpointRecord, key: string): boolean {
    return endpoint.metadata?.[key] === true;
  }

  private uniqueLines(entries: string[]): string[] {
    return Array.from(new Set(entries.filter((entry) => entry.trim().length > 0)));
  }

  private metric(label: string, value: string, detail: string, tone: MetricCard['tone']): MetricCard {
    return { label, value, detail, tone };
  }

  private setupFlow(title: string, summary: string, steps: SetupFlowStep[]): SetupFlowSection {
    return { title, summary, steps };
  }

  private setupStep(
    title: string,
    state: SetupFlowStep['state'],
    summary: string,
    detail: string,
    bullets: string[] = []
  ): SetupFlowStep {
    return {
      title,
      state,
      summary,
      detail,
      bullets
    };
  }

  private text(english: string, chinese: string): string {
    return uiText(english, chinese);
  }

  private errorMessage(error: unknown): string {
    if (typeof error === 'object' && error !== null && 'error' in error) {
      const payload = (error as { error?: { message?: string } | string }).error;
      if (typeof payload === 'string' && payload.trim()) {
        return payload;
      }
      if (payload && typeof payload === 'object' && 'message' in payload && typeof payload.message === 'string') {
        return payload.message;
      }
    }
    if (typeof error === 'object' && error !== null && 'message' in error && typeof error.message === 'string') {
      return error.message;
    }
    return 'The request failed before HarborDesk could render a live projection.';
  }
}
