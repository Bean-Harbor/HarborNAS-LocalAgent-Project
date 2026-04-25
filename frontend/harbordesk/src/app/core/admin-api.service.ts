import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable, concat, forkJoin, of, throwError } from 'rxjs';
import { catchError, map } from 'rxjs/operators';

import {
  AccessMemberSummary,
  AccountManagementSnapshot,
  AdminStateResponse,
  DeliverySurface,
  DeskPageModel,
  DeskRow,
  FeatureAvailabilityGroup,
  FeatureAvailabilityItem,
  FeatureAvailabilityResponse,
  FeatureAvailabilityStatus,
  GatewayPlatformStatus,
  GatewayStatusResponse,
  MetricCard,
  ModelEndpointRecord,
  ModelEndpointTestResult,
  ModelEndpointsResponse,
  ModelPoliciesResponse,
  PageState,
  SetupFlowSection,
  SetupFlowStep,
  TaskApprovalSummary
} from './admin-api.types';
import { HarborDeskPageId } from './page-registry';

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

  private pageRequest(pageId: HarborDeskPageId): Observable<PageState<DeskPageModel>> {
    switch (pageId) {
      case 'overview':
        return forkJoin({
          state: this.getState(),
          gateway: this.getGatewayStatus(),
          models: this.getModelEndpoints(),
          policies: this.getModelPolicies()
        }).pipe(map(({ state, gateway, models, policies }) => this.buildOverviewState(state, gateway, models, policies)));
      case 'im-gateway':
        return this.getGatewayStatus().pipe(map((gateway) => this.buildImGatewayState(gateway)));
      case 'account-management':
        return this.getAccountManagement().pipe(map((account) => this.buildAccountManagementState(account)));
      case 'tasks-approvals':
        return this.getPendingApprovals().pipe(map((approvals) => this.buildTasksState(approvals)));
      case 'devices-aiot':
        return this.getState().pipe(map((state) => this.buildDevicesState(state)));
      case 'harboros':
        return this.getState().pipe(map((state) => this.buildHarborOsState(state)));
      case 'models-policies':
        return forkJoin({
          endpoints: this.getModelEndpoints(),
          policies: this.getModelPolicies(),
          availability: this.getFeatureAvailability()
        }).pipe(map(({ endpoints, policies, availability }) => this.buildModelsState(endpoints, policies, availability)));
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

  private loadingState(pageId: HarborDeskPageId): PageState<DeskPageModel> {
    return {
      kind: 'loading',
      detail: 'Hydrating same-origin admin API projection.',
      data: this.baseModel(pageId)
    };
  }

  private blockerState(pageId: HarborDeskPageId, detail: string): PageState<DeskPageModel> {
    return {
      kind: 'blocker',
      detail,
      data: {
        ...this.baseModel(pageId),
        summary: 'The page could not finish loading from the same-origin admin API.',
        metrics: [
          {
            label: 'Admin API',
            value: 'Blocked',
            detail,
            tone: 'danger'
          }
        ],
        blockers: [detail],
        nextStep: 'Restore the admin-plane response for this page before using HarborDesk for operations.'
      }
    };
  }

  private baseModel(pageId: HarborDeskPageId): DeskPageModel {
    const titleMap: Record<HarborDeskPageId, string> = {
      overview: 'Overview',
      'im-gateway': 'IM Gateway',
      'account-management': 'Account Management',
      'tasks-approvals': 'Tasks & Approvals',
      'devices-aiot': 'Devices & AIoT',
      harboros: 'HarborOS',
      'models-policies': 'Models & Policies',
      'system-settings': 'System Settings'
    };
    return {
      pageId,
      title: titleMap[pageId],
      eyebrow: 'HarborDesk',
      summary: 'Loading same-origin HarborBeacon admin-plane data.',
      endpoint: '/api/state',
      outputDirectory: this.outputDirectory,
      metrics: [],
      highlights: [],
      blockers: [],
      emptyNote: 'No data reported yet.',
      nextStep: 'Wait for the admin API projection to finish loading.'
    };
  }

  private buildOverviewState(
    state: AdminStateResponse,
    gateway: GatewayStatusResponse,
    models: ModelEndpointsResponse,
    policies: ModelPoliciesResponse
  ): PageState<DeskPageModel> {
    const members = state.account_management?.workspace?.member_count ?? 0;
    const devices = state.devices?.length ?? 0;
    const endpoints = models.endpoints?.length ?? 0;
    const routePolicies = policies.route_policies?.length ?? 0;
    const feishuReady = this.platformReady(gateway, 'feishu');
    const weixinReady = this.platformReady(gateway, 'weixin');
    const blockers = this.imBlockers(gateway);
    return {
      kind: 'success',
      detail: 'Overview is reading live state, delivery policy, gateway readiness, and model inventory.',
      data: {
        ...this.baseModel('overview'),
        eyebrow: 'Unified control plane',
        summary: 'Interactive replies stay source-bound, while proactive delivery follows the default named HarborGate target.',
        endpoint: 'GET /api/state + /api/gateway/status + /api/models/endpoints',
        setupFlow: this.setupFlow(
          'Release-v1 setup flow',
          'Use the live admin-plane projection to wire Weixin, HarborOS identity, camera registration, storage guidance, and policy selection without inventing new framework objects.',
          [
            this.setupStep(
              'HarborOS principal reuse',
              state.account_management?.workspace?.current_principal_user_id
                ? 'ready'
                : 'blocked',
              state.account_management?.workspace?.current_principal_user_id
                ? 'HarborDesk inherits the HarborOS login principal as the admin principal.'
                : 'HarborOS identity is not projected yet.',
              state.account_management?.workspace?.current_principal_user_id
                ? `Current HarborOS principal: ${state.account_management.workspace.current_principal_display_name || state.account_management.workspace.current_principal_user_id}. Workspace owner baseline: ${state.account_management.workspace.owner_user_id}.`
                : 'The current OS-session principal must be surfaced before release-v1 can be treated as HarborOS-native.',
              state.account_management?.workspace?.current_principal_user_id
                ? ['Use the HarborOS login user as the admin owner baseline.', 'No separate HarborDesk login is introduced.']
                : ['Blocker: current HarborOS principal is not yet surfaced by the backend.']
            ),
            this.setupStep(
              'Weixin transport readiness',
              weixinReady ? 'ready' : 'needs-config',
              weixinReady ? 'Weixin parity is available for the真人测试主链。' : 'Weixin still needs transport/provider-side cleanup before it can carry the release-v1 flow.',
              weixinReady
                ? `Gateway status is ${gateway.weixin?.rehearsal_ready ? 'rehearsal_ready' : 'connected'} and source-bound delivery remains separated from proactive delivery.`
                : `Current blocker: ${this.weixinBlocker(gateway) ?? 'transport not yet ready'}.`,
              weixinReady
                ? ['Use Weixin as the live validation surface.', 'Keep replies source-bound to the original message channel.']
                : ['Do not promote Weixin into the flow until the gateway is healthy enough to carry private-text ingress.']
            ),
            this.setupStep(
              'Camera registry selection',
              devices > 0 ? 'read-only' : 'blocked',
              devices > 0 ? 'The registry has camera candidates, but the backend still does not expose a selected-camera flag.' : 'No camera candidates are registered yet.',
              devices > 0
                ? 'Pick the acceptance camera from Devices & AIoT. The UI can guide you through the registry, but the backend does not yet project a selected-camera field.'
                : 'Register at least one camera before the release-v1 setup flow can continue.',
              devices > 0
                ? ['Use the registry row as the source of truth for camera capabilities.', 'Selection remains an operator choice, not a backend-managed scene object.']
                : ['Blocker: device registry is empty.']
            ),
            this.setupStep(
              'Capture storage and subdirectory',
              state.writable_root && state.defaults.capture_subdirectory ? 'read-only' : 'blocked',
              state.writable_root && state.defaults.capture_subdirectory
                ? 'Writable root and capture subdirectory are both projected from the backend.'
                : 'The backend does not yet project a writable root and capture_subdir together.',
              state.writable_root && state.defaults.capture_subdirectory
                ? `Capture target: ${state.writable_root}/${state.defaults.capture_subdirectory}.`
                : `Current capture label: ${state.defaults.capture}. Writable-root and concrete subdirectory wiring must be surfaced by the backend before this step becomes editable.`,
              state.writable_root && state.defaults.capture_subdirectory
                ? ['This remains a setup flow over existing defaults and recording policy metadata.', 'No release-v1 scene object is introduced.']
                : ['Treat this as read-only guidance for now.', 'Do not fake a capture path in the UI.']
            ),
            this.setupStep(
              'Clip length and recording policy',
              state.defaults.clip_length_seconds ? 'read-only' : 'blocked',
              state.defaults.clip_length_seconds
                ? 'Clip duration and keyframe hints are projected from the existing recording policy.'
                : 'Clip duration is not projected as a numeric, writable policy yet.',
              state.defaults.clip_length_seconds
                ? `Clip length: ${state.defaults.clip_length_seconds}s. Keyframes: ${state.defaults.keyframe_count ?? 'n/a'} at ${state.defaults.keyframe_interval_seconds ?? 'n/a'}s interval.`
                : `Current recording label: ${state.defaults.recording}. The real clip length must be surfaced through the existing recording policy projection before release-v1 can make it editable.`,
              ['Short-video support stays release-v1 scoped as short clip + keyframe retrieval.', 'No new scene object is introduced to solve this.']
            ),
            this.setupStep(
              'OCR / VLM / reply policy selection',
              endpoints > 0 && routePolicies > 0 ? 'ready' : 'needs-config',
              endpoints > 0 && routePolicies > 0 ? 'Model endpoints and route policies are visible and can be inspected in the models page.' : 'Model selection still needs the backend-projected inventory before it can be treated as release-ready.',
              endpoints > 0 && routePolicies > 0
                ? `Endpoints: ${endpoints}. Route policies: ${routePolicies}. Use Models & Policies to verify the OCR/VLM/reply choices and their operator status.`
                : 'Register model endpoints and route policies before using this part of the setup flow.',
              endpoints > 0 && routePolicies > 0
                ? ['VLM-first stays the multimodal priority.', 'Audio and full video understanding remain pending by design.']
                : ['Blocker: model endpoints or route policies are still missing.']
            ),
            this.setupStep(
              'Default proactive notification target',
              state.delivery_policy.proactive_delivery ? 'ready' : 'blocked',
              'Independent notifications follow the default named HarborGate target.',
              `Current workspace policy: ${state.delivery_policy.proactive_delivery}. Account Management shows which named target is currently default.`,
              ['HarborGate owns target capture and IM identity.', 'Interactive replies remain source-bound.']
            )
          ]
        ),
        metrics: [
          this.metric('Workspace members', `${members}`, 'Live count from account management.', members > 0 ? 'good' : 'warn'),
          this.metric('Registered devices', `${devices}`, 'Current Home Device Domain registry size.', devices > 0 ? 'good' : 'neutral'),
          this.metric('Model endpoints', `${endpoints}`, 'Visible from Model Center admin-plane.', endpoints > 0 ? 'good' : 'warn'),
          this.metric('Route policies', `${routePolicies}`, 'Model route policies surfaced by the admin-plane.', routePolicies > 0 ? 'good' : 'warn'),
          this.metric('Delivery policy', `${state.delivery_policy.interactive_reply} / ${state.delivery_policy.proactive_delivery}`, 'Interactive reply and proactive default policy are frozen.', 'good'),
          this.metric('Feishu readiness', feishuReady ? 'Ready' : 'Pending', 'Baseline channel readiness from HarborGate.', feishuReady ? 'good' : 'warn'),
          this.metric('Weixin readiness', weixinReady ? 'Ready' : 'Pending', 'Parity track readiness from HarborGate.', weixinReady ? 'good' : 'warn')
        ],
        highlights: [
          `Workspace: ${state.account_management.workspace.display_name}`,
          `Current principal: ${state.account_management.workspace.current_principal_display_name || state.current_principal_display_name || state.account_management.workspace.owner_user_id}.`,
          `Default proactive routing is ${state.delivery_policy.proactive_delivery}.`,
          `Bridge provider status: ${state.bridge_provider.status || 'unknown'}.`
        ],
        blockers,
        detailRows: [
          {
            title: 'Gateway base URL',
            subtitle: state.bridge_provider.gateway_base_url || 'not configured',
            meta: [`Binding channel: ${state.binding.channel}`, `Binding status: ${state.binding.status}`],
            tone: state.bridge_provider.connected ? 'good' : 'warn'
          }
        ],
        emptyNote: 'Overview has no live metrics yet.',
        nextStep: blockers.length === 0 ? 'Proceed to a domain page for action-level operations.' : 'Clear the surfaced blockers before declaring dual-surface readiness.'
      }
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

  private buildDevicesState(state: AdminStateResponse): PageState<DeskPageModel> {
    const devices = state.devices ?? [];
    const kind = devices.length === 0 ? 'empty' : 'success';
    return {
      kind,
      detail: 'Devices and AIoT inventory are projected from the Home Device Domain registry.',
      data: {
        ...this.baseModel('devices-aiot'),
        eyebrow: 'Home Device Domain',
        summary: 'Device discovery, preview, share-link, inspect, and control remain device-domain owned.',
        endpoint: 'GET /api/state',
        setupFlow: this.setupFlow(
          'Release-v1 camera setup flow',
          'Use the registry data to choose the acceptance camera, confirm snapshot/clip capability, and keep capture storage read-only until the backend exposes it.',
          [
            this.setupStep(
              'Selected camera',
              devices.length > 0 ? 'read-only' : 'blocked',
              devices.length > 0 ? 'Camera candidates are available in the registry.' : 'No cameras are registered yet.',
              devices.length > 0
                ? `Choose the acceptance camera from the list below. The backend still does not expose an explicit selected-camera field; ${devices.length} candidate(s) are available.`
                : 'Register at least one camera before the release-v1 camera flow can be configured.',
              devices.length > 0
                ? ['Use the registry row to inspect capability and metadata.', 'Selection itself remains an operator choice.']
                : ['Blocker: device registry is empty.']
            ),
            this.setupStep(
              'Snapshot and clip capability',
              devices.some((device) => Boolean(device.profile?.snapshot_url) || Boolean(device.profile?.rtsp_url)) ? 'ready' : 'needs-config',
              'The page highlights whether the device has a native snapshot URL or falls back to RTSP/ffmpeg.',
              devices.some((device) => Boolean(device.profile?.snapshot_url) || Boolean(device.profile?.rtsp_url))
                ? 'At least one registered device can be used for release-v1 capture.'
                : 'No registered device currently projects both a usable stream and snapshot capability.',
              ['TP-Link/Tapo keeps the local RTSP + snapshot-first path.', 'The UI does not invent device-native control.']
            ),
            this.setupStep(
              'Capture subdirectory',
              'blocked',
              'A concrete capture_subdir is not yet projected by the backend.',
              'The capture target must remain read-only until the admin-plane exposes a writable capture path. The page only shows the existing capture label and the operator guidance.',
              ['Do not fake a writable capture path.', 'Keep the configured storage target inside the backend-projected root once it becomes available.']
            )
          ]
        ),
        metrics: [
          this.metric('Registered devices', `${devices.length}`, 'Current devices in the Home Device Domain registry.', devices.length > 0 ? 'good' : 'warn'),
          this.metric(
            'Native snapshot ready',
            `${devices.filter((device) => Boolean(device.profile?.snapshot_url)).length}`,
            'Devices with a native snapshot URL persisted in profile metadata.',
            'neutral'
          ),
          this.metric(
            'RTSP capable',
            `${devices.filter((device) => Boolean(device.profile?.rtsp_url)).length}`,
            'Devices with a stored RTSP stream URL.',
            devices.some((device) => Boolean(device.profile?.rtsp_url)) ? 'good' : 'warn'
          )
        ],
        highlights: [
          'TP-Link/Tapo remains local RTSP + snapshot first.',
          'HarborOS does not take over device-native control.'
        ],
        blockers: [],
        detailRows: devices.map((device) => ({
          title: device.name,
          subtitle: `${device.room || 'unassigned room'} / ${device.device_id}`,
          meta: [
            `status: ${device.status ?? 'unknown'}`,
            `rtsp: ${device.profile?.rtsp_url ? 'configured' : 'pending'}`,
            `snapshot: ${device.profile?.snapshot_url ? 'native' : 'ffmpeg fallback'}`,
            `selected: ${state.defaults.selected_camera_device_id === device.device_id ? 'yes' : 'no'}`
          ],
          tone: state.defaults.selected_camera_device_id === device.device_id ? 'good' : device.profile?.snapshot_url ? 'good' : 'neutral'
        })),
        emptyNote: 'No devices are registered yet.',
        nextStep: 'Use manual add or discovery to populate the device registry, then revisit snapshot/share-link operations.'
      }
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
    availability: FeatureAvailabilityResponse
  ): PageState<DeskPageModel> {
    const endpointRows = endpoints.endpoints ?? [];
    const featureGroups = availability.groups ?? [];
    const retrievalFeatures = this.findFeatureGroup(featureGroups, 'retrieval')?.items ?? [];
    const availableRetrievalCount = retrievalFeatures.filter((item) => item.status === 'available').length;
    const runtimeAlignment = this.buildRuntimeAlignmentSummary(endpointRows);
    const kind = endpointRows.length === 0 ? 'empty' : 'success';
    return {
      kind,
      detail: 'Model Center now keeps runtime truth, endpoint projection, and route-policy inventory on the same page.',
      data: {
        ...this.baseModel('models-policies'),
        eyebrow: 'Model center operations',
        summary: 'Runtime alignment, feature availability, endpoint state, and route-policy control now share the same HarborDesk page.',
        endpoint: 'GET /api/models/endpoints + /api/models/policies + /api/feature-availability',
        setupFlow: this.setupFlow(
          'Release-v1 model setup flow',
          'The setup flow exposes OCR, retrieval, and reply choices using the live runtime overlay plus the existing endpoint and route-policy inventory.',
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
          this.metric('Policies', `${policies.route_policies.length}`, 'Route policies exposed by the admin-plane.', policies.route_policies.length > 0 ? 'good' : 'neutral')
        ],
        highlights: [
          'Projection mismatch stays visible instead of being silently flattened into the stored admin state.',
          'Endpoint tests are operator actions, not hidden background probes.'
        ],
        blockers: this.featureBlockers(featureGroups),
        modelEndpoints: endpointRows,
        modelPolicies: policies.route_policies,
        featureGroups,
        runtimeAlignment,
        detailRows: policies.route_policies.map((policy) => ({
          title: policy.route_policy_id,
          subtitle: `${policy.domain_scope} / ${policy.modality}`,
          meta: [
            `privacy: ${policy.privacy_level}`,
            `status: ${policy.status}`,
            `fallback: ${policy.fallback_order.join(' -> ') || 'none'}`
          ],
          tone: policy.local_preferred ? 'good' : 'neutral'
        })),
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
