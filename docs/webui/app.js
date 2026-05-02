const apiHost = window.location.hostname || "127.0.0.1";
const apiProtocol = window.location.protocol === "file:" ? "http:" : window.location.protocol;
const runtimeOrigin =
  window.location.origin && window.location.origin !== "null"
    ? window.location.origin
    : `${apiProtocol}//${apiHost}:4174`;
const APP_BASE = runtimeOrigin;
const API_BASE = `${runtimeOrigin}/api`;
const HARBOROS_ROUTE_ORDER = ["Middleware API", "MidCLI", "Browser/MCP fallback"];
const HARBOROS_VERIFIER_LINE_LABELS = {
  middleware_first: "Windows verifier line",
  midcli_fallback: "Debian shim line",
};

const state = {
  binding: {
    status: "等待扫码",
    metric: "等待绑定",
    boundUser: "未配置",
    channel: "Harbor IM Bridge",
    qrToken: "http://127.0.0.1:4174/setup/mobile?session=PENDING",
    staticQrToken: "http://harbordesk.local:4174/setup/mobile",
  },
  bridgeProvider: {
    configured: false,
    appId: "",
    appSecret: "",
    appName: "",
    botOpenId: "",
    status: "未配置",
  },
  defaults: {
    cidr: "192.168.3.0/24",
    discovery: "ONVIF + RTSP",
    recording: "按事件录制",
    capture: "图片 + 摘要",
    ai: "人体检测 + 中文摘要",
    notificationChannel: "家庭通知频道",
    rtspUsername: "admin",
    rtspPassword: "",
    rtspPaths: [
      "/h264/ch1/main/av_stream",
      "/ch1/main",
      "/Streaming/Channels/101",
      "/live",
      "/stream1",
      "/stream2",
      "/h264/ch1/sub/av_stream",
      "/ch1/sub",
      "/Streaming/Channels/102",
    ],
  },
  lastCommand: "等待后台动作",
  recordingEnabled: false,
  cameras: [],
  activeCameraId: null,
  approvals: [],
  approvalsLoaded: false,
  approvalsError: "",
  accountManagement: null,
  accountManagementLoaded: false,
  accountManagementError: "",
  gatewayStatus: null,
  gatewayStatusLoaded: false,
  gatewayStatusError: "",
  accessMembers: [],
  accessMembersLoaded: false,
  accessMembersError: "",
  modelEndpoints: [],
  modelEndpointsLoaded: false,
  modelEndpointsError: "",
  modelPolicies: [],
  modelPoliciesLoaded: false,
  modelPoliciesError: "",
  featureAvailabilityGroups: [],
  featureAvailabilityLoaded: false,
  featureAvailabilityError: "",
  modelEndpointTestResults: {},
  shareLinks: [],
  shareLinksLoaded: false,
  shareLinksError: "",
  deliveryPolicy: null,
  latestTaskOutcome: null,
  scanResults: [],
  events: [
    {
      type: "info",
      title: "正在连接 HarborDesk 管理 API",
      body: "页面启动后会读取真实的绑定状态、默认策略和设备库，而不是继续使用演示假数据。",
      time: "刚刚",
    },
  ],
};

const els = {
  bindingMetric: document.querySelector("#metric-binding"),
  scanMetric: document.querySelector("#metric-scan"),
  camerasMetric: document.querySelector("#metric-cameras"),
  commandMetric: document.querySelector("#metric-command"),
  qrToken: document.querySelector("#qr-token"),
  qrImage: document.querySelector("#qr-image"),
  qrInstruction: document.querySelector("#qr-instruction"),
  bindStatus: document.querySelector("#bind-status"),
  boundUser: document.querySelector("#bound-user"),
  boundChannel: document.querySelector("#bound-channel"),
  scanResults: document.querySelector("#scan-results"),
  approvalList: document.querySelector("#approval-list"),
  accessMemberList: document.querySelector("#access-member-list"),
  shareLinkList: document.querySelector("#share-link-list"),
  cameraTabs: document.querySelector("#camera-tabs"),
  activeName: document.querySelector("#active-camera-name"),
  activeMeta: document.querySelector("#active-camera-meta"),
  activeHint: document.querySelector("#active-camera-hint"),
  activeLiveStatus: document.querySelector("#active-live-status"),
  livePreviewFrame: document.querySelector("#live-preview-frame"),
  signalChip: document.querySelector("#signal-chip"),
  streamMode: document.querySelector("#stream-mode"),
  overlayBoxes: document.querySelector("#overlay-boxes"),
  detailName: document.querySelector("#detail-name"),
  detailRoom: document.querySelector("#detail-room"),
  detailSource: document.querySelector("#detail-source"),
  detailStream: document.querySelector("#detail-stream"),
  detailNotification: document.querySelector("#detail-notification"),
  deviceBadge: document.querySelector("#device-badge"),
  eventList: document.querySelector("#event-list"),
  toast: document.querySelector("#toast"),
  scanCidr: document.querySelector("#scan-cidr"),
  scanProtocol: document.querySelector("#scan-protocol"),
  policyCidr: document.querySelector("#policy-cidr"),
  policyDiscovery: document.querySelector("#policy-discovery"),
  policyRecording: document.querySelector("#policy-recording"),
  policyCapture: document.querySelector("#policy-capture"),
  policyAi: document.querySelector("#policy-ai"),
  policyNotificationChannel: document.querySelector("#policy-notification-channel"),
  policyRtspUsername: document.querySelector("#policy-rtsp-username"),
  policyRtspPassword: document.querySelector("#policy-rtsp-password"),
  policyRtspPaths: document.querySelector("#policy-rtsp-paths"),
  manualForm: document.querySelector("#manual-form"),
  bindTestForm: document.querySelector("#bind-test-form"),
  bindTestDisplayName: document.querySelector("#bind-test-display-name"),
  bindTestOpenId: document.querySelector("#bind-test-open-id"),
  refreshApprovals: document.querySelector("#refresh-approvals"),
  refreshAccessMembers: document.querySelector("#refresh-access-members"),
  refreshShareLinks: document.querySelector("#refresh-share-links"),
  workspaceSummaryList: document.querySelector("#workspace-summary-list"),
  roleCountList: document.querySelector("#role-count-list"),
  memberDeliveryDefaultList: document.querySelector("#member-delivery-default-list"),
  memberBindingAvailabilityList: document.querySelector("#member-binding-availability-list"),
  identityBindingList: document.querySelector("#identity-binding-list"),
  governanceList: document.querySelector("#governance-list"),
  overviewReplyList: document.querySelector("#overview-reply-list"),
  overviewDeliveryList: document.querySelector("#overview-delivery-list"),
  gatewayFeishuList: document.querySelector("#gateway-feishu-list"),
  gatewayWeixinList: document.querySelector("#gateway-weixin-list"),
  gatewayChannelList: document.querySelector("#gateway-channel-list"),
  gatewayStatusNote: document.querySelector("#gateway-status-note"),
  overviewPageState: document.querySelector("#overview-page-state"),
  imGatewayPageState: document.querySelector("#im-gateway-page-state"),
  accountManagementPageState: document.querySelector("#account-management-page-state"),
  tasksApprovalsPageState: document.querySelector("#tasks-approvals-page-state"),
  devicesAiotPageState: document.querySelector("#devices-aiot-page-state"),
  harborosPageState: document.querySelector("#harboros-page-state"),
  modelsPoliciesPageState: document.querySelector("#models-policies-page-state"),
  systemSettingsPageState: document.querySelector("#system-settings-page-state"),
  modelEndpointSummary: document.querySelector("#model-endpoint-summary"),
  modelEndpointList: document.querySelector("#model-endpoint-list"),
  modelPolicySummary: document.querySelector("#model-policy-summary"),
  modelPolicyList: document.querySelector("#model-policy-list"),
  modelRuntimeAlignmentList: document.querySelector("#model-runtime-alignment-list"),
  modelCenterNotes: document.querySelector("#model-center-notes"),
  refreshModelCenter: document.querySelector("#refresh-model-center"),
  saveRoutePolicies: document.querySelector("#save-route-policies"),
  taskReplyList: document.querySelector("#task-reply-list"),
  taskDeliveryFailureList: document.querySelector("#task-delivery-failure-list"),
  aiotSummaryList: document.querySelector("#aiot-summary-list"),
  harborosLiveStatusList: document.querySelector("#harboros-live-status-list"),
  harborosProofSummaryList: document.querySelector("#harboros-proof-summary-list"),
  harborosProofNote: document.querySelector("#harboros-proof-note"),
  systemRoutingList: document.querySelector("#system-routing-list"),
  systemBlockerList: document.querySelector("#system-blocker-list"),
  featureAvailabilitySummary: document.querySelector("#feature-availability-summary"),
  featureAvailabilityGroups: document.querySelector("#feature-availability-groups"),
  taskOutcomeStatus: document.querySelector("#task-outcome-status"),
  taskOutcomeMessage: document.querySelector("#task-outcome-message"),
  taskOutcomeAction: document.querySelector("#task-outcome-action"),
  taskOutcomeReply: document.querySelector("#task-outcome-reply"),
  taskOutcomeDeliveryFailure: document.querySelector("#task-outcome-delivery-failure"),
  taskOutcomeAudit: document.querySelector("#task-outcome-audit"),
  taskOutcomeNotification: document.querySelector("#task-outcome-notification"),
  taskOutcomeArtifacts: document.querySelector("#task-outcome-artifacts"),
  taskOutcomeEvents: document.querySelector("#task-outcome-events"),
};

const navButtons = Array.from(document.querySelectorAll(".nav-item[data-view]"));
const viewSections = Array.from(document.querySelectorAll("[data-view-section]"));
const viewNames = new Set(navButtons.map((button) => button.dataset.view));

const previewState = {
  timer: null,
  deviceId: null,
};

const viewState = {
  activeView: "overview",
};

function getActiveCamera() {
  return state.cameras.find((camera) => camera.id === state.activeCameraId) || state.cameras[0] || null;
}

function normalizeView(view) {
  const nextView = String(view || "").trim();
  return viewNames.has(nextView) ? nextView : "overview";
}

function setActiveView(view, options = {}) {
  const { updateHash = true } = options;
  const nextView = normalizeView(view);
  viewState.activeView = nextView;

  navButtons.forEach((button) => {
    const isActive = button.dataset.view === nextView;
    button.classList.toggle("active", isActive);
    if (isActive) {
      button.setAttribute("aria-current", "page");
    } else {
      button.removeAttribute("aria-current");
    }
  });

  viewSections.forEach((section) => {
    const isActive = section.dataset.viewSection === nextView;
    section.hidden = !isActive;
    section.classList.toggle("active", isActive);
  });

  if (updateHash) {
    const nextHash = `#${nextView}`;
    if (window.location.hash !== nextHash) {
      window.history.replaceState(null, "", nextHash);
    }
  }
}

function showToast(message) {
  els.toast.textContent = message;
  els.toast.classList.add("show");
  window.clearTimeout(showToast.timer);
  showToast.timer = window.setTimeout(() => {
    els.toast.classList.remove("show");
  }, 2800);
}

function pushEvent(event) {
  state.events.unshift(event);
  renderEvents();
}

async function api(path, options = {}) {
  const response = await fetch(`${API_BASE}${path}`, {
    headers: {
      "Content-Type": "application/json",
      ...(options.headers || {}),
    },
    ...options,
  });

  let payload = {};
  try {
    payload = await response.json();
  } catch (_error) {
    payload = {};
  }

  if (!response.ok) {
    throw new Error(payload.error || `Request failed: ${response.status}`);
  }

  return payload;
}

function absoluteAppUrl(path) {
  if (!path) {
    return "";
  }
  if (/^https?:\/\//i.test(path)) {
    return path;
  }
  return `${APP_BASE}${path.startsWith("/") ? "" : "/"}${path}`;
}

function cameraSnapshotUrl(deviceId) {
  return `${API_BASE}/cameras/${encodeURIComponent(deviceId)}/snapshot.jpg?ts=${Date.now()}`;
}

function maskRtspUrl(url) {
  return String(url || "").replace(/(rtsp:\/\/[^:\/]+:)([^@]+)@/i, "$1***@");
}

function toRoomLabel(room) {
  return room || "未分配房间";
}

function toStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "online":
      return "在线";
    case "offline":
      return "离线";
    case "degraded":
      return "待排查";
    default:
      return "待验证";
  }
}

function toStatusTone(status) {
  switch (String(status || "").toLowerCase()) {
    case "online":
      return "online";
    case "offline":
      return "offline";
    default:
      return "warning";
  }
}

function toTransportLabel(device) {
  const transport = String(device.primary_stream?.transport || "rtsp").toUpperCase();
  const streamKind = device.capabilities?.audio ? "主码流 + 音频" : "主码流";
  return `${transport} / ${streamKind}`;
}

function toLiveStatus(device) {
  switch (String(device.status || "").toLowerCase()) {
    case "online":
      return "近实时预览正常";
    case "offline":
      return "等待重新连接";
    default:
      return "等待后台验证";
  }
}

function toSignalLabel(device) {
  switch (String(device.status || "").toLowerCase()) {
    case "online":
      return "链路稳定";
    case "offline":
      return "掉线";
    default:
      return "待验证";
  }
}

function toRiskLabel(level) {
  switch (String(level || "").toUpperCase()) {
    case "MEDIUM":
      return "中风险";
    case "HIGH":
      return "高风险";
    case "CRITICAL":
      return "极高风险";
    default:
      return "低风险";
  }
}

function toRiskClass(level) {
  switch (String(level || "").toUpperCase()) {
    case "MEDIUM":
      return "approval-risk-medium";
    case "HIGH":
      return "approval-risk-high";
    case "CRITICAL":
      return "approval-risk-critical";
    default:
      return "";
  }
}

function toApprovalStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "approved":
      return "已批准";
    case "rejected":
      return "已拒绝";
    case "expired":
      return "已过期";
    case "cancelled":
      return "已取消";
    default:
      return "待审批";
  }
}

function toApprovalStatusClass(status) {
  switch (String(status || "").toLowerCase()) {
    case "approved":
      return "approval-status-approved";
    case "rejected":
    case "expired":
    case "cancelled":
      return "approval-status-rejected";
    default:
      return "approval-status-pending";
  }
}

function toAutonomyLabel(level) {
  switch (String(level || "").toLowerCase()) {
    case "read_only":
    case "readonly":
      return "ReadOnly";
    case "full":
      return "Full";
    default:
      return "Supervised";
  }
}

function toMemberRoleLabel(roleKind) {
  switch (String(roleKind || "").toLowerCase()) {
    case "owner":
      return "Owner";
    case "admin":
      return "Admin";
    case "operator":
      return "Operator";
    case "member":
      return "Member";
    case "guest":
      return "Guest";
    default:
      return "Viewer";
  }
}

function toMemberStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "pending":
      return "待加入";
    case "revoked":
      return "已停用";
    default:
      return "生效中";
  }
}

function toMemberSourceLabel(source) {
  switch (String(source || "").toLowerCase()) {
    case "im_bridge":
      return "IM Bridge";
    case "local_console":
      return "本地控制台";
    default:
      return source || "未知来源";
  }
}

function mapAccessMember(member) {
  return {
    userId: member?.user_id || "",
    displayName: member?.display_name || member?.user_id || "未命名成员",
    roleKind: member?.role_kind || "viewer",
    membershipStatus: member?.membership_status || "active",
    source: member?.source || "unknown",
    openId: member?.open_id || "",
    chatId: member?.chat_id || "",
    canEdit: Boolean(member?.can_edit),
    isOwner: Boolean(member?.is_owner),
    proactiveDeliverySurface:
      member?.proactive_delivery_surface ||
      member?.default_proactive_delivery_surface ||
      member?.delivery_surface ||
      member?.notification_surface ||
      "",
    proactiveDeliveryDefault:
      member?.proactive_delivery_default ??
      member?.default_proactive_delivery ??
      member?.is_default_proactive_delivery ??
      null,
    bindingAvailability:
      member?.binding_availability ||
      member?.binding_status ||
      member?.binding_state ||
      "",
    bindingAvailable:
      member?.binding_available ??
      member?.can_bind ??
      member?.is_binding_available ??
      null,
    bindingAvailabilityNote: member?.binding_availability_note || member?.binding_reason || "",
    recentInteractiveSurface:
      member?.recent_interactive_surface || member?.last_interactive_surface || "",
  };
}

function mapDeliveryPolicy(policy) {
  if (!policy || typeof policy !== "object") {
    return null;
  }
  return {
    interactiveReply: policy.interactive_reply || policy.interactiveReply || "",
    proactiveDelivery: policy.proactive_delivery || policy.proactiveDelivery || "",
  };
}

function toShareLinkStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "revoked":
      return "已撤销";
    case "expired":
      return "已过期";
    case "closed":
      return "已关闭";
    case "failed":
      return "会话失败";
    default:
      return "生效中";
  }
}

function toShareLinkStatusClass(status) {
  switch (String(status || "").toLowerCase()) {
    case "revoked":
    case "failed":
      return "approval-status-rejected";
    case "expired":
    case "closed":
      return "approval-status-pending";
    default:
      return "approval-status-approved";
  }
}

function toShareAccessScopeLabel(scope) {
  switch (String(scope || "").toLowerCase()) {
    case "workspace":
      return "工作区内";
    case "invite_only":
      return "仅邀请";
    default:
      return "公开链接";
  }
}

function toShareSessionStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "opening":
      return "会话建立中";
    case "closed":
      return "会话已关闭";
    case "failed":
      return "会话失败";
    default:
      return "会话活跃";
  }
}

function mapShareLink(item) {
  return {
    shareLinkId: item?.share_link_id || "",
    mediaSessionId: item?.media_session_id || "",
    deviceId: item?.device_id || "",
    deviceName: item?.device_name || item?.device_id || "未命名设备",
    openedByUserId: item?.opened_by_user_id || "",
    accessScope: item?.access_scope || "public_link",
    sessionStatus: item?.session_status || "active",
    status: item?.status || "active",
    expiresAt: item?.expires_at || "",
    revokedAt: item?.revoked_at || "",
    startedAt: item?.started_at || "",
    endedAt: item?.ended_at || "",
    canRevoke: Boolean(item?.can_revoke),
  };
}

function formatTimestamp(value) {
  if (!value) {
    return "刚刚";
  }
  if (typeof value === "number") {
    const milliseconds = value < 1_000_000_000_000 ? value * 1000 : value;
    return new Date(milliseconds).toLocaleString("zh-CN", { hour12: false });
  }

  const normalized = String(value).trim();
  if (/^\d+$/.test(normalized)) {
    const numeric = Number(normalized);
    const milliseconds = normalized.length <= 10 ? numeric * 1000 : numeric;
    return new Date(milliseconds).toLocaleString("zh-CN", { hour12: false });
  }

  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) {
    return String(value);
  }
  return date.toLocaleString("zh-CN", { hour12: false });
}

function toApprovalActionLabel(approval) {
  if (approval.intentText) {
    return approval.intentText;
  }
  const actionKey = `${approval.domain}.${approval.action}`;
  switch (actionKey) {
    case "camera.connect":
      return "接入摄像头";
    case "camera.scan":
      return "扫描摄像头";
    case "camera.analyze":
      return "分析摄像头";
    default:
      return actionKey === "." ? "待审批任务" : actionKey;
  }
}

function toApprovalReason(approval) {
  return approval.reason || `${toApprovalActionLabel(approval)} 需要管理员确认后才会继续执行。`;
}

function toTaskStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "completed":
      return "已完成";
    case "failed":
      return "执行失败";
    case "needs_input":
    case "needsinput":
      return "等待输入";
    case "rejected":
      return "已拒绝";
    default:
      return "处理中";
  }
}

function toTaskStatusClass(status) {
  switch (String(status || "").toLowerCase()) {
    case "completed":
      return "approval-status-approved";
    case "failed":
    case "rejected":
      return "approval-status-rejected";
    case "needs_input":
    case "needsinput":
      return "approval-status-pending";
    default:
      return "";
  }
}

function toChannelLabel(channel) {
  switch (String(channel || "").toLowerCase()) {
    case "im_bridge":
    case "feishu":
      return "IM Bridge";
    case "local_ui":
      return "Local UI";
    case "telegram":
      return "Telegram";
    case "wecom":
      return "WeCom";
    case "webhook":
      return "Webhook";
    default:
      return String(channel || "未知通道");
  }
}

function toEventTone(severity) {
  switch (String(severity || "").toLowerCase()) {
    case "warning":
      return "warning";
    case "error":
    case "critical":
      return "error";
    case "info":
      return "info";
    default:
      return "normal";
  }
}

function summarizeArtifacts(artifacts) {
  if (!Array.isArray(artifacts) || !artifacts.length) {
    return "尚无产物";
  }
  const labels = artifacts
    .map((artifact) => artifact?.label || artifact?.kind || "未命名产物")
    .filter(Boolean);
  if (!labels.length) {
    return `${artifacts.length} 个产物`;
  }
  const preview = labels.slice(0, 3).join(" / ");
  return artifacts.length > 3 ? `${preview} 等 ${artifacts.length} 个产物` : preview;
}

function summarizeNotificationFeedback(delivery, request) {
  if (delivery) {
    const channel = toChannelLabel(delivery.channel);
    const destination = delivery.destination || request?.destination || "未指定去向";
    const recipient =
      delivery.recipient?.label || delivery.recipient?.receive_id || "未映射收件人";
    const status = String(delivery.status || "").toLowerCase();
    const statusLabel = status === "sent" ? "已投递" : status === "failed" ? "投递失败" : "已跳过";
    return `${statusLabel} · ${channel} · ${destination} · ${recipient}`;
  }

  if (request) {
    return `已生成通知请求 · ${toChannelLabel(request.channel)} · ${request.destination || "未指定去向"}`;
  }

  return "本次任务未触发通知";
}

function eventTitleFromRecord(record) {
  switch (record?.event_type) {
    case "task.completed":
      return "任务执行完成";
    case "task.failed":
      return "任务执行失败";
    case "task.needs_input":
      return "任务等待输入";
    case "task.notification_requested":
      return "已生成通知请求";
    case "task.notification_delivered":
      return "通知已投递";
    case "task.notification_failed":
      return "通知投递失败";
    case "task.interaction_replied":
    case "task.reply_delivered":
      return "交互回复已返回";
    case "task.proactive_delivery_failed":
      return "主动投递失败";
    case "task.share_link_issued":
      return "共享链接已生成";
    case "task.approval_required":
      return "任务需要审批";
    case "task.approval_approved":
      return "审批已通过";
    case "task.approval_rejected":
      return "审批已拒绝";
    case "task.autonomy_blocked":
      return "Autonomy 已阻止任务";
    default:
      return record?.event_type || "任务事件";
  }
}

function eventBodyFromRecord(record) {
  const payload = record?.payload || {};
  switch (record?.event_type) {
    case "task.completed":
    case "task.failed":
    case "task.needs_input":
      return payload.message || "任务状态已更新。";
    case "task.notification_requested":
      return payload.notification?.title
        ? `已为“${payload.notification.title}”生成通知请求。`
        : "分析结果已经准备进入通知链路。";
    case "task.notification_delivered": {
      const delivery = payload.delivery || payload;
      const channel = toChannelLabel(delivery.channel);
      const destination = delivery.destination || "未指定去向";
      const recipient =
        delivery.recipient?.label || delivery.recipient?.receive_id || "未映射收件人";
      return `通知已通过 ${channel} 投递到 ${destination}，收件人 ${recipient}。`;
    }
    case "task.notification_failed": {
      const delivery = payload.delivery || payload;
      return delivery.error || payload.error || "通知投递失败。";
    }
    case "task.interaction_replied":
    case "task.reply_delivered":
      return payload.reply?.message || payload.message || "交互回复已经回到来源会话。";
    case "task.proactive_delivery_failed": {
      const failure = payload.failure || payload.delivery || payload;
      return failure.error || failure.reason || payload.error || "主动投递失败。";
    }
    case "task.share_link_issued":
      return payload.url
        ? `已生成共享观看页：${absoluteAppUrl(payload.url)}`
        : "已生成共享观看链接。";
    case "task.approval_required":
      return payload.policy_violation?.message || "任务需要管理员审批后继续执行。";
    case "task.approval_approved":
      return "管理员已批准，任务恢复执行。";
    case "task.approval_rejected":
      return "管理员已拒绝，任务已结束。";
    case "task.autonomy_blocked":
      return payload.policy_ref
        ? `${payload.policy_ref} 超出了当前 autonomy 级别允许的范围。`
        : "当前 autonomy 级别阻止了任务继续执行。";
    default:
      return payload.message || "任务事件已记录。";
  }
}

function normalizeTaskEvents(events) {
  if (!Array.isArray(events)) {
    return [];
  }
  return events
    .filter((event) => event && typeof event === "object")
    .map((event) => ({
      eventType: event.event_type || "task.event",
      severity: event.severity || "info",
      occurredAt: event.occurred_at || event.ingested_at || "",
      title: eventTitleFromRecord(event),
      body: eventBodyFromRecord(event),
    }));
}

function buildOutcomeFromTaskResponse(taskResponse, actionLabel) {
  const result = taskResponse?.result || {};
  const data = result.data || {};
  const artifacts = Array.isArray(result.artifacts) ? result.artifacts : [];
  const events = normalizeTaskEvents(result.events);
  return {
    actionLabel,
    status: taskResponse?.status || "completed",
    auditRef: taskResponse?.audit_ref || "",
    taskId: taskResponse?.task_id || "",
    message: result.message || "任务已处理。",
    interactionReplySummary: summarizeInteractionReply(
      data.interaction_reply || data.reply || data.reply_delivery
    ),
    proactiveDeliveryFailureSummary: summarizeProactiveDeliveryFailure(
      data.proactive_delivery_failure || data.delivery_failure || data.notification_failure
    ),
    notificationSummary: summarizeNotificationFeedback(
      data.notification_delivery,
      data.notification_request
    ),
    artifactsSummary: summarizeArtifacts(artifacts),
    events,
  };
}

function extractVisionMarkers(taskResponse) {
  const detections = taskResponse?.result?.data?.detections;
  if (!Array.isArray(detections)) {
    return [];
  }

  return detections
    .map((detection) => {
      const x1 = Number(detection?.x1);
      const y1 = Number(detection?.y1);
      const x2 = Number(detection?.x2);
      const y2 = Number(detection?.y2);
      if (![x1, y1, x2, y2].every(Number.isFinite)) {
        return null;
      }

      const left = Math.max(0, Math.min(Math.min(x1, x2), 1));
      const top = Math.max(0, Math.min(Math.min(y1, y2), 1));
      const right = Math.max(left, Math.min(Math.max(x1, x2), 1));
      const bottom = Math.max(top, Math.min(Math.max(y1, y2), 1));
      const confidence = Number(detection?.confidence);
      const confidenceLabel = Number.isFinite(confidence)
        ? ` ${(confidence * 100).toFixed(0)}%`
        : "";

      return {
        label: `${detection?.label || "object"}${confidenceLabel}`,
        x: left * 100,
        y: top * 100,
        w: Math.max((right - left) * 100, 2),
        h: Math.max((bottom - top) * 100, 2),
      };
    })
    .filter(Boolean);
}

function applyAnalyzeOutcomeToCamera(camera, taskResponse) {
  if (!camera) {
    return;
  }
  camera.markers = extractVisionMarkers(taskResponse);
}

function extractShareLinkUrl(taskResponse) {
  const artifacts = Array.isArray(taskResponse?.result?.artifacts)
    ? taskResponse.result.artifacts
    : [];
  const artifactLink = artifacts.find((artifact) => artifact?.kind === "link" && artifact?.url);
  if (artifactLink?.url) {
    return absoluteAppUrl(artifactLink.url);
  }

  const payloadLink = taskResponse?.result?.data?.share_link?.url;
  return payloadLink ? absoluteAppUrl(payloadLink) : "";
}

function buildOutcomeFromRejectedApproval(actionLabel, approval) {
  return {
    actionLabel,
    status: "rejected",
    auditRef: "",
    taskId: approval.taskId || "",
    message: "审批已拒绝，任务不会继续落到后续执行步骤。",
    notificationSummary: "任务已在审批阶段结束，未继续执行通知链路",
    artifactsSummary: "尚无产物",
    events: [
      {
        eventType: "task.approval_rejected",
        severity: "warning",
        occurredAt: approval.decidedAt || "",
        title: "审批已拒绝",
        body: "管理员已拒绝该高风险动作，任务已经结束。",
      },
    ],
  };
}

function appendTaskEventsToFeed(events) {
  events.forEach((event) => {
    pushEvent({
      type: toEventTone(event.severity),
      title: event.title,
      body: event.body,
      time: formatTimestamp(event.occurredAt),
    });
  });
}

function mapApproval(summary) {
  const ticket = summary?.approval_ticket || {};
  return {
    approvalId: ticket.approval_id || "",
    taskId: ticket.task_id || "",
    policyRef: ticket.policy_ref || "",
    requesterUserId: ticket.requester_user_id || summary?.user_id || "unknown",
    approverUserId: ticket.approver_user_id || "",
    status: ticket.status || "pending",
    reason: ticket.reason || "",
    requestedAt: ticket.requested_at || "",
    decidedAt: ticket.decided_at || "",
    sourceChannel: summary?.source_channel || "unknown",
    surface: summary?.surface || "unknown",
    conversationId: summary?.conversation_id || "",
    sessionId: summary?.session_id || "",
    domain: summary?.domain || "",
    action: summary?.action || "",
    intentText: summary?.intent_text || "",
    autonomyLevel: summary?.autonomy_level || "supervised",
    riskLevel: summary?.risk_level || "LOW",
  };
}

function renderApprovalPlaceholder(title, note, chipLabel) {
  els.approvalList.innerHTML = "";
  const item = document.createElement("li");
  item.className = "scan-result-item approval-item";
  item.innerHTML = `
    <div class="scan-result-main approval-copy">
      <span class="scan-result-title">${title}</span>
      <span class="scan-result-note">${note}</span>
    </div>
    <div class="scan-result-actions approval-actions">
      <span class="status-chip">${chipLabel}</span>
    </div>
  `;
  els.approvalList.appendChild(item);
}

function renderAccessMemberPlaceholder(title, note, chipLabel) {
  els.accessMemberList.innerHTML = "";
  const item = document.createElement("li");
  item.className = "scan-result-item member-item";
  item.innerHTML = `
    <div class="scan-result-main member-copy">
      <span class="scan-result-title">${title}</span>
      <span class="scan-result-note">${note}</span>
    </div>
    <div class="scan-result-actions member-actions">
      <span class="status-chip">${chipLabel}</span>
    </div>
  `;
  els.accessMemberList.appendChild(item);
}

function toHint(device) {
  const source = String(device.discovery_source || "manual_entry");
  if (source === "manual_entry") {
    return "这台设备是手动录入并已做 RTSP 验证，适合继续在后台确认默认推送策略。";
  }
  if (String(device.status || "").toLowerCase() === "offline") {
    return "掉线排查、凭证修复和推送目标调整都应该在后台完成，这也是这个 WebUI 的核心价值。";
  }
  return "这里的意义是管理员后台验证设备可用，而不是让最终用户每天打开网页来操作。";
}

function mapBinding(binding) {
  return {
    status: binding?.status || "等待扫码",
    metric: binding?.metric || "等待绑定",
    boundUser: binding?.bound_user || "未配置",
    channel: binding?.channel || "Harbor IM Bridge",
    qrToken: binding?.setup_url || binding?.qr_token || "http://127.0.0.1:4174/setup/mobile?session=PENDING",
    staticQrToken: binding?.static_setup_url || binding?.setup_url || "http://harbordesk.local:4174/setup/mobile",
  };
}

function mapBridgeProvider(config) {
  return {
    configured: Boolean(config?.configured),
    connected: Boolean(config?.connected),
    platform: config?.platform || "",
    gatewayBaseUrl: config?.gateway_base_url || "",
    appId: config?.app_id || "",
    appSecret: config?.app_secret || "",
    appName: config?.app_name || "",
    botOpenId: config?.bot_open_id || "",
    status: config?.status || "未配置",
    lastCheckedAt: config?.last_checked_at || "",
    capabilities: {
      reply: Boolean(config?.capabilities?.reply),
      update: Boolean(config?.capabilities?.update),
      attachments: Boolean(config?.capabilities?.attachments),
    },
  };
}

function mapAccountManagement(payload) {
  if (!payload || typeof payload !== "object") {
    return null;
  }
  return {
    workspace: payload.workspace || null,
    memberRoleCounts: Array.isArray(payload.member_role_counts) ? payload.member_role_counts : [],
    identityBindings: Array.isArray(payload.identity_bindings) ? payload.identity_bindings : [],
    accessGovernance: payload.access_governance || null,
    gateway: payload.gateway || null,
  };
}

function mapGatewayStatus(payload) {
  if (!payload || typeof payload !== "object") {
    return null;
  }

  const gatewayStatus =
    payload.gateway_status && typeof payload.gateway_status === "object"
      ? payload.gateway_status
      : payload;
  const channels = Array.isArray(payload.channels)
    ? payload.channels
    : Array.isArray(gatewayStatus.channels)
      ? gatewayStatus.channels
      : [];

  return {
    source:
      payload.feishu ||
      payload.weixin ||
      channels.length ||
      payload.source_bound_queue ||
      gatewayStatus.source_bound_queue ||
      payload.queue ||
      gatewayStatus.queue ||
      payload.source_bound_failures ||
      gatewayStatus.source_bound_failures ||
      payload.failures ||
      gatewayStatus.failures
        ? "harborgate_status"
        : "local_gateway_summary",
    bindingChannel:
      payload.binding_channel || gatewayStatus.binding_channel || state.binding.channel || "",
    bindingStatus:
      payload.binding_status || gatewayStatus.binding_status || state.binding.status || "",
    bindingMetric:
      payload.binding_metric || gatewayStatus.binding_metric || state.binding.metric || "",
    bindingBoundUser:
      payload.binding_bound_user || gatewayStatus.binding_bound_user || state.binding.boundUser || "",
    setupUrl: payload.setup_url || gatewayStatus.setup_url || state.binding.qrToken || "",
    staticSetupUrl:
      payload.static_setup_url || gatewayStatus.static_setup_url || state.binding.staticQrToken || "",
    bridgeProvider: mapBridgeProvider(payload.bridge_provider || gatewayStatus.bridge_provider || {}),
    feishu: payload.feishu || null,
    weixin: payload.weixin || null,
    channels,
    sourceBoundQueue:
      payload.source_bound_queue ||
      gatewayStatus.source_bound_queue ||
      payload.queue ||
      gatewayStatus.queue ||
      [],
    sourceBoundFailures:
      payload.source_bound_failures ||
      gatewayStatus.source_bound_failures ||
      payload.failures ||
      gatewayStatus.failures ||
      [],
  };
}

function mapModelEndpoint(endpoint) {
  if (!endpoint || typeof endpoint !== "object") {
    return null;
  }
  return {
    modelEndpointId: endpoint.model_endpoint_id || "",
    workspaceId: endpoint.workspace_id || "",
    providerAccountId: endpoint.provider_account_id || "",
    modelKind: endpoint.model_kind || "",
    endpointKind: endpoint.endpoint_kind || "",
    providerKey: endpoint.provider_key || "",
    modelName: endpoint.model_name || "",
    capabilityTags: Array.isArray(endpoint.capability_tags) ? endpoint.capability_tags : [],
    costPolicy: endpoint.cost_policy || {},
    status: endpoint.status || "",
    metadata: endpoint.metadata || {},
  };
}

function mapModelPolicy(policy) {
  if (!policy || typeof policy !== "object") {
    return null;
  }
  return {
    routePolicyId: policy.route_policy_id || "",
    workspaceId: policy.workspace_id || "",
    domainScope: policy.domain_scope || "",
    modality: policy.modality || "",
    privacyLevel: policy.privacy_level || "",
    localPreferred: Boolean(policy.local_preferred),
    maxCostPerRun:
      policy.max_cost_per_run === null || policy.max_cost_per_run === undefined
        ? null
        : policy.max_cost_per_run,
    fallbackOrder: Array.isArray(policy.fallback_order) ? policy.fallback_order : [],
    status: policy.status || "",
    metadata: policy.metadata || {},
  };
}

function mapFeatureAvailabilityItem(item) {
  if (!item || typeof item !== "object") {
    return null;
  }
  return {
    featureId: item.feature_id || "",
    label: item.label || item.feature_id || "Unnamed feature",
    ownerLane: item.owner_lane || "",
    status: item.status || "unknown",
    sourceOfTruth: item.source_of_truth || "",
    currentOption: item.current_option || "",
    fallbackOrder: Array.isArray(item.fallback_order) ? item.fallback_order : [],
    blocker: item.blocker || "",
    evidence: Array.isArray(item.evidence) ? item.evidence : [],
  };
}

function mapFeatureAvailabilityGroup(group) {
  if (!group || typeof group !== "object") {
    return null;
  }
  return {
    groupId: group.group_id || "",
    label: group.label || group.group_id || "Unnamed group",
    items: Array.isArray(group.items) ? group.items.map(mapFeatureAvailabilityItem).filter(Boolean) : [],
  };
}

function yesNo(value) {
  return value ? "yes" : "no";
}

function toMaybeLabel(value, fallback = "未提供") {
  if (value === null || value === undefined || value === "") {
    return fallback;
  }
  return String(value);
}

function toAvailabilityLabel(value) {
  if (typeof value === "boolean") {
    return value ? "可用" : "不可用";
  }
  switch (String(value || "").toLowerCase()) {
    case "available":
    case "ready":
    case "enabled":
    case "bound":
      return "可用";
    case "blocked":
    case "unavailable":
    case "disabled":
      return "不可用";
    case "pending":
    case "unknown":
      return "待验证";
    default:
      return value ? String(value) : "待验证";
  }
}

function toFeatureStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "available":
      return "available";
    case "degraded":
      return "degraded";
    case "blocked":
      return "blocked";
    case "not_configured":
      return "not configured";
    default:
      return status ? String(status) : "unknown";
  }
}

function toFeatureStatusClass(status) {
  switch (String(status || "").toLowerCase()) {
    case "available":
      return "approval-status-approved";
    case "degraded":
      return "approval-status-pending";
    case "blocked":
      return "approval-status-rejected";
    case "not_configured":
      return "";
    default:
      return "";
  }
}

function toFeatureEditHint(featureId) {
  switch (String(featureId || "").toLowerCase()) {
    case "retrieval.ocr":
      return "Models & Policies -> model endpoints / route policies";
    case "retrieval.embed":
    case "retrieval.answer":
    case "retrieval.vision_summary":
      return "Models & Policies -> runtime projection + route policies";
    case "interactive_reply":
    case "proactive_delivery":
      return "IM Gateway + Account Management";
    case "binding_availability":
      return "Account Management + IM Gateway";
    default:
      return "System Settings";
  }
}

function hasFeatureProjectionMismatch(item) {
  return Array.isArray(item?.evidence)
    && item.evidence.some((entry) => String(entry || "").includes("projection_mismatch"));
}

function toDefaultLabel(value) {
  if (typeof value === "boolean") {
    return value ? "member default" : "not default";
  }
  if (value === null || value === undefined || value === "") {
    return "backend not exposed";
  }
  return String(value);
}

function toModelKindLabel(kind) {
  switch (String(kind || "").toLowerCase()) {
    case "llm":
      return "LLM";
    case "vlm":
      return "VLM";
    case "ocr":
      return "OCR";
    case "asr":
      return "ASR";
    case "detector":
      return "Detector";
    case "embedder":
      return "Embedder";
    default:
      return kind ? String(kind) : "unknown";
  }
}

function toEndpointKindLabel(kind) {
  switch (String(kind || "").toLowerCase()) {
    case "local":
      return "Local";
    case "sidecar":
      return "Sidecar";
    case "cloud":
      return "Cloud";
    default:
      return kind ? String(kind) : "unknown";
  }
}

function toModelStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "active":
      return "active";
    case "degraded":
      return "degraded";
    case "disabled":
      return "disabled";
    default:
      return status ? String(status) : "unknown";
  }
}

function toPrivacyLevelLabel(level) {
  switch (String(level || "").toLowerCase()) {
    case "strictlocal":
    case "strict_local":
      return "strict local";
    case "allowredactedcloud":
    case "allow_redacted_cloud":
      return "allow redacted cloud";
    case "allowcloud":
    case "allow_cloud":
      return "allow cloud";
    default:
      return level ? String(level) : "unknown";
  }
}

function toBooleanLabel(value) {
  if (typeof value === "boolean") {
    return value ? "yes" : "no";
  }
  return value ? String(value) : "no";
}

function parseFallbackOrder(value) {
  return String(value || "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function toFallbackOrderText(order) {
  if (!Array.isArray(order) || !order.length) {
    return "not set";
  }
  return order.join(" -> ");
}

function matchRoutePolicyForEndpoint(endpoint, policies) {
  if (!endpoint || !Array.isArray(policies) || !policies.length) {
    return null;
  }
  const modelKind = String(endpoint.model_kind || "").toLowerCase();
  const modelName = String(endpoint.model_name || "").toLowerCase();
  const providerKey = String(endpoint.provider_key || "").toLowerCase();
  const endpointKind = String(endpoint.endpoint_kind || "").toLowerCase();
  const preferredTokens = [];
  if (modelKind) {
    preferredTokens.push(modelKind);
  }
  if (modelName) {
    preferredTokens.push(modelName);
  }
  if (providerKey) {
    preferredTokens.push(providerKey);
  }
  if (endpointKind) {
    preferredTokens.push(endpointKind);
  }
  return (
    policies.find((policy) => {
      const haystack = [
        policy.route_policy_id,
        policy.domain_scope,
        policy.modality,
        policy.status,
        ...(Array.isArray(policy.fallback_order) ? policy.fallback_order : []),
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      return preferredTokens.some((token) => haystack.includes(token));
    }) || policies[0] || null
  );
}

function renderPageState(target, status, title, note, details = []) {
  if (!target) {
    return;
  }

  const states = ["loading", "empty", "blocker", "success"];
  const labelMap = {
    loading: "loading",
    empty: "empty",
    blocker: "blocker",
    success: "success",
  };
  const chips = states
    .map((item) => `<span class="state-chip ${item === status ? `active ${item}` : ""}">${labelMap[item]}</span>`)
    .join("");

  target.innerHTML = `
    <div class="page-state-head">
      <span class="page-state-title">${title}</span>
      <span class="status-chip ${status === "blocker" ? "approval-status-rejected" : status === "success" ? "approval-status-approved" : status === "empty" ? "approval-status-pending" : ""}">${status}</span>
    </div>
    <div class="page-state-chips">${chips}</div>
    <p class="page-state-note">${note}</p>
    ${details.length ? `<ul class="feature-list compact-feature-list">${details.map((detail) => `<li>${detail}</li>`).join("")}</ul>` : ""}
  `;
}

function normalizeSignalEntry(entry) {
  if (entry === null || entry === undefined || entry === "") {
    return "";
  }
  if (typeof entry === "string" || typeof entry === "number" || typeof entry === "boolean") {
    return String(entry);
  }
  if (Array.isArray(entry)) {
    return entry.map(normalizeSignalEntry).filter(Boolean).join(" · ");
  }
  if (typeof entry !== "object") {
    return "";
  }

  const parts = [];
  const source = entry.source || entry.surface || entry.platform || entry.channel || entry.target;
  if (source) {
    parts.push(`source=${source}`);
  }
  const status = entry.status || entry.state || entry.result;
  if (status) {
    parts.push(`status=${status}`);
  }
  const queueDepth =
    entry.queue_depth ?? entry.queueDepth ?? entry.pending_count ?? entry.pendingCount ?? entry.pending;
  if (queueDepth !== undefined && queueDepth !== null && queueDepth !== "") {
    parts.push(`queue=${queueDepth}`);
  }
  const failureCount =
    entry.failure_count ?? entry.failureCount ?? entry.failed_count ?? entry.failedCount ?? entry.failures;
  if (failureCount !== undefined && failureCount !== null && failureCount !== "") {
    parts.push(`failures=${failureCount}`);
  }
  const message = entry.message || entry.reason || entry.error || entry.note || entry.summary;
  if (message) {
    parts.push(String(message));
  }

  return parts.length ? parts.join(" · ") : JSON.stringify(entry);
}

function normalizeSignalList(values) {
  if (!Array.isArray(values)) {
    return values ? [normalizeSignalEntry(values)] : [];
  }
  return values.map(normalizeSignalEntry).filter(Boolean);
}

function summarizeInteractionReply(reply) {
  if (!reply) {
    return "No interaction-linked reply reported yet.";
  }
  if (typeof reply !== "object") {
    return String(reply);
  }

  const channel = toChannelLabel(reply.channel || reply.source_channel || reply.source);
  const destination =
    reply.destination || reply.surface || reply.reply_to || reply.thread || reply.conversation || "未指定目标";
  const status = reply.status || reply.state || "";
  const message = reply.message || reply.summary || reply.body || reply.content || "";
  return [channel, destination, status ? `status=${status}` : "", message].filter(Boolean).join(" · ");
}

function summarizeProactiveDeliveryFailure(failure) {
  if (!failure) {
    return "No proactive delivery failure reported yet.";
  }
  if (typeof failure !== "object") {
    return String(failure);
  }

  const surface =
    failure.destination ||
    failure.surface ||
    failure.member ||
    failure.recipient?.label ||
    failure.recipient?.receive_id ||
    "未指定成员";
  const channel = toChannelLabel(failure.channel || failure.delivery_channel || failure.source_channel);
  const reason = failure.error || failure.reason || failure.message || failure.summary || "delivery failed";
  return [channel, surface, reason].filter(Boolean).join(" · ");
}

function mapDefaults(defaults) {
  return {
    cidr: defaults?.cidr || "192.168.3.0/24",
    discovery: defaults?.discovery || "RTSP Probe",
    recording: defaults?.recording || "按事件录制",
    capture: defaults?.capture || "图片 + 摘要",
    ai: defaults?.ai || "人体检测 + 中文摘要",
    notificationChannel: defaults?.notification_channel || defaults?.feishu_group || "家庭通知频道",
    rtspUsername: defaults?.rtsp_username || "admin",
    rtspPassword: defaults?.rtsp_password || "",
    rtspPaths: Array.isArray(defaults?.rtsp_paths) && defaults.rtsp_paths.length
      ? defaults.rtsp_paths
      : [
          "/h264/ch1/main/av_stream",
          "/ch1/main",
          "/Streaming/Channels/101",
          "/live",
          "/stream1",
          "/stream2",
          "/h264/ch1/sub/av_stream",
          "/ch1/sub",
          "/Streaming/Channels/102",
        ],
  };
}

function mapCamera(device) {
  const ip = device.ip_address || "未知 IP";
  const statusLabel = toStatusLabel(device.status);
  return {
    id: device.device_id,
    name: device.name || `Camera ${ip}`,
    room: toRoomLabel(device.room),
    ip,
    status: statusLabel,
    statusTone: toStatusTone(device.status),
    stream: maskRtspUrl(device.primary_stream?.url),
    transport: toTransportLabel(device),
    source: device.discovery_source || "manual_entry",
    liveStatus: toLiveStatus(device),
    recordingMode: state.recordingEnabled ? "手动录像中" : state.defaults.recording,
    notification: `${state.defaults.notificationChannel} / ${state.binding.channel}`,
    signal: toSignalLabel(device),
    hint: toHint(device),
    markers: [],
  };
}

async function loadPendingApprovals(options = {}) {
  const { silent = false } = options;
  try {
    const payload = await api("/tasks/approvals");
    state.approvals = Array.isArray(payload) ? payload.map(mapApproval) : [];
    state.approvalsLoaded = true;
    state.approvalsError = "";
    renderApprovals();
    return state.approvals;
  } catch (error) {
    state.approvals = [];
    state.approvalsLoaded = true;
    state.approvalsError = error.message;
    renderApprovals();
    if (!silent) {
      pushEvent({
        type: "warning",
        title: "审批队列读取失败",
        body: error.message,
        time: "刚刚",
      });
    }
    throw error;
  }
}

async function loadAccountManagement(options = {}) {
  const { silent = false } = options;
  try {
    const payload = await api("/account-management");
    state.accountManagement = mapAccountManagement(payload);
    state.accountManagementLoaded = true;
    state.accountManagementError = "";
    renderAccountManagement();
    return state.accountManagement;
  } catch (error) {
    state.accountManagement = null;
    state.accountManagementLoaded = true;
    state.accountManagementError = error.message;
    renderAccountManagement();
    if (!silent) {
      pushEvent({
        type: "warning",
        title: "账号治理视图读取失败",
        body: error.message,
        time: "刚刚",
      });
    }
    throw error;
  }
}

async function loadAccessMembers(options = {}) {
  const { silent = false } = options;
  try {
    const payload = await api("/access/members");
    state.accessMembers = Array.isArray(payload) ? payload.map(mapAccessMember) : [];
    state.accessMembersLoaded = true;
    state.accessMembersError = "";
    renderAccessMembers();
    return state.accessMembers;
  } catch (error) {
    state.accessMembers = [];
    state.accessMembersLoaded = true;
    state.accessMembersError = error.message;
    renderAccessMembers();
    if (!silent) {
      pushEvent({
        type: "warning",
        title: "成员角色列表读取失败",
        body: error.message,
        time: "刚刚",
      });
    }
    throw error;
  }
}

async function loadGatewayStatus(options = {}) {
  const { silent = false } = options;
  try {
    const payload = await api("/gateway/status");
    state.gatewayStatus = mapGatewayStatus(payload);
    state.gatewayStatusLoaded = true;
    state.gatewayStatusError = "";
    renderGatewayStatus();
    return state.gatewayStatus;
  } catch (error) {
    state.gatewayStatus = null;
    state.gatewayStatusLoaded = true;
    state.gatewayStatusError = error.message;
    renderGatewayStatus();
    if (!silent) {
      pushEvent({
        type: "warning",
        title: "IM Gateway 状态读取失败",
        body: error.message,
        time: "刚刚",
      });
    }
    throw error;
  }
}

async function loadModelCenter(options = {}) {
  const { silent = false } = options;
  const [endpointsResult, policiesResult, featureAvailabilityResult] = await Promise.allSettled([
    api("/models/endpoints"),
    api("/models/policies"),
    api("/feature-availability"),
  ]);

  const nextErrors = [];

  if (endpointsResult.status === "fulfilled") {
    const payload = endpointsResult.value;
    state.modelEndpoints = Array.isArray(payload?.endpoints)
      ? payload.endpoints.map(mapModelEndpoint).filter(Boolean)
      : [];
    state.modelEndpointsLoaded = true;
    state.modelEndpointsError = "";
  } else {
    state.modelEndpoints = [];
    state.modelEndpointsLoaded = true;
    state.modelEndpointsError = endpointsResult.reason?.message || "unknown error";
    nextErrors.push(state.modelEndpointsError);
  }

  if (policiesResult.status === "fulfilled") {
    const payload = policiesResult.value;
    state.modelPolicies = Array.isArray(payload?.route_policies)
      ? payload.route_policies.map(mapModelPolicy).filter(Boolean)
      : [];
    state.modelPoliciesLoaded = true;
    state.modelPoliciesError = "";
  } else {
    state.modelPolicies = [];
    state.modelPoliciesLoaded = true;
    state.modelPoliciesError = policiesResult.reason?.message || "unknown error";
    nextErrors.push(state.modelPoliciesError);
  }

  if (featureAvailabilityResult.status === "fulfilled") {
    const payload = featureAvailabilityResult.value;
    state.featureAvailabilityGroups = Array.isArray(payload?.groups)
      ? payload.groups.map(mapFeatureAvailabilityGroup).filter(Boolean)
      : [];
    state.featureAvailabilityLoaded = true;
    state.featureAvailabilityError = "";
  } else {
    state.featureAvailabilityGroups = [];
    state.featureAvailabilityLoaded = true;
    state.featureAvailabilityError =
      featureAvailabilityResult.reason?.message || "unknown error";
    nextErrors.push(state.featureAvailabilityError);
  }

  const endpointIds = new Set(state.modelEndpoints.map((endpoint) => endpoint.modelEndpointId));
  state.modelEndpointTestResults = Object.fromEntries(
    Object.entries(state.modelEndpointTestResults || {}).filter(([endpointId]) =>
      endpointIds.has(endpointId)
    )
  );

  renderModelsPolicies();
  renderSystemSettings();

  if (nextErrors.length && !silent) {
    pushEvent({
      type: "warning",
      title: "Model Center 读取失败",
      body: nextErrors.join(" · "),
      time: "刚刚",
    });
  }

  if (nextErrors.length && !silent) {
    throw new Error(nextErrors.join(" · "));
  }

  return {
    endpoints: state.modelEndpoints,
    routePolicies: state.modelPolicies,
  };
}

async function loadShareLinks(options = {}) {
  const { silent = false } = options;
  try {
    const payload = await api("/share-links");
    state.shareLinks = Array.isArray(payload) ? payload.map(mapShareLink) : [];
    state.shareLinksLoaded = true;
    state.shareLinksError = "";
    renderShareLinks();
    return state.shareLinks;
  } catch (error) {
    state.shareLinks = [];
    state.shareLinksLoaded = true;
    state.shareLinksError = error.message;
    renderShareLinks();
    if (!silent) {
      pushEvent({
        type: "warning",
        title: "共享链接列表读取失败",
        body: error.message,
        time: "刚刚",
      });
    }
    throw error;
  }
}

function stopPreviewLoop() {
  if (previewState.timer) {
    window.clearInterval(previewState.timer);
    previewState.timer = null;
  }
  previewState.deviceId = null;
}

function refreshPreviewFrame() {
  const camera = getActiveCamera();
  if (!camera) {
    els.livePreviewFrame.removeAttribute("src");
    return;
  }
  els.livePreviewFrame.src = cameraSnapshotUrl(camera.id);
}

function syncPreviewLoop() {
  const camera = getActiveCamera();
  if (!camera) {
    stopPreviewLoop();
    els.livePreviewFrame.removeAttribute("src");
    return;
  }

  if (previewState.deviceId !== camera.id) {
    stopPreviewLoop();
    previewState.deviceId = camera.id;
    refreshPreviewFrame();
    previewState.timer = window.setInterval(refreshPreviewFrame, 1500);
    return;
  }

  if (!previewState.timer) {
    previewState.timer = window.setInterval(refreshPreviewFrame, 1500);
  }
}

function applyServerState(payload) {
  if (payload.binding) {
    state.binding = mapBinding(payload.binding);
  }
  if (payload.delivery_policy) {
    state.deliveryPolicy = mapDeliveryPolicy(payload.delivery_policy);
  }
  if (payload.bridge_provider) {
    state.bridgeProvider = mapBridgeProvider(payload.bridge_provider);
  }
  if (payload.account_management) {
    state.accountManagement = mapAccountManagement(payload.account_management);
    state.accountManagementLoaded = true;
    state.accountManagementError = "";
    if (state.accountManagement?.gateway) {
      state.gatewayStatus = mapGatewayStatus(state.accountManagement.gateway);
      state.gatewayStatusLoaded = true;
      state.gatewayStatusError = "";
    }
  }
  if (payload.defaults) {
    state.defaults = mapDefaults(payload.defaults);
  }
  if (Array.isArray(payload.devices)) {
    const nextCameras = payload.devices.map(mapCamera);
    const activeStillExists = nextCameras.some((camera) => camera.id === state.activeCameraId);
    state.cameras = nextCameras;
    if (!activeStillExists) {
      state.activeCameraId = nextCameras[0]?.id || null;
    }
  }

  els.scanCidr.value = state.defaults.cidr;
  els.scanProtocol.value = state.defaults.discovery;
  els.policyCidr.value = state.defaults.cidr;
  els.policyDiscovery.value = state.defaults.discovery;
  els.policyRecording.value = state.defaults.recording;
  els.policyCapture.value = state.defaults.capture;
  els.policyAi.value = state.defaults.ai;
  els.policyNotificationChannel.value = state.defaults.notificationChannel;
  els.policyRtspUsername.value = state.defaults.rtspUsername;
  els.policyRtspPassword.value = state.defaults.rtspPassword;
  els.policyRtspPaths.value = state.defaults.rtspPaths.join(", ");
  els.bindTestDisplayName.value = state.bridgeProvider.appId;
  els.bindTestOpenId.value = state.bridgeProvider.appSecret;
}

function renderMetrics() {
  els.bindingMetric.textContent = state.binding.metric;
  els.scanMetric.textContent = state.defaults.cidr;
  els.camerasMetric.textContent = String(state.cameras.length);
  els.commandMetric.textContent = state.lastCommand;
}

function renderBinding() {
  els.qrToken.textContent = state.binding.staticQrToken;
  els.qrInstruction.textContent = "这张静态二维码应该贴在 bridge 硬件或本地配网页入口上。手机扫码后会在浏览器里打开后台配置页，然后填写消息桥接 provider 的 app_id 和 app_secret。";
  if (els.qrImage) {
    els.qrImage.src = `${API_BASE}/binding/static-qr.svg?ts=${Date.now()}`;
  }
  els.bindStatus.textContent = state.binding.status;
  els.boundUser.textContent = state.bridgeProvider.appName || state.binding.boundUser;
  els.boundChannel.textContent = state.binding.channel;
}

function renderScanResults() {
  els.scanResults.innerHTML = "";

  if (!state.scanResults.length) {
    const item = document.createElement("li");
    item.className = "scan-result-item";
    item.innerHTML = `
      <div class="scan-result-main">
        <span class="scan-result-title">等待一次真实扫描</span>
        <span class="scan-result-meta">${state.defaults.cidr} · ${state.defaults.discovery}</span>
        <span class="scan-result-note">这里会显示后台扫描到的可验证主机，不再展示写死的演示设备。</span>
      </div>
      <div class="scan-result-actions">
        <span class="status-chip">尚未扫描</span>
      </div>
    `;
    els.scanResults.appendChild(item);
    return;
  }

  state.scanResults.forEach((result) => {
    const item = document.createElement("li");
    item.className = "scan-result-item";
    const badge = result.reachable ? "可接入" : "需排查";
    item.innerHTML = `
      <div class="scan-result-main">
        <span class="scan-result-title">${result.name}</span>
        <span class="scan-result-meta">${result.room} · ${result.ip} · ${result.protocol}</span>
        <span class="scan-result-note">${result.note}</span>
      </div>
      <div class="scan-result-actions">
        <span class="status-chip">${badge}</span>
        ${result.reachable && result.device_id ? '<button class="button button-secondary">查看设备</button>' : ""}
      </div>
    `;

    const button = item.querySelector("button");
    if (button) {
      button.addEventListener("click", () => {
        state.activeCameraId = result.device_id;
        renderCameraTabs();
        renderDevicePanel();
        showToast(`已切到 ${result.name}。`);
      });
    }

    els.scanResults.appendChild(item);
  });
}

function renderCameraTabs() {
  els.cameraTabs.innerHTML = "";

  if (!state.cameras.length) {
    const placeholder = document.createElement("div");
    placeholder.className = "status-chip";
    placeholder.textContent = "还没有摄像头，请先扫描或手动添加";
    els.cameraTabs.appendChild(placeholder);
    return;
  }

  state.cameras.forEach((camera) => {
    const button = document.createElement("button");
    button.className = `camera-tab ${camera.id === state.activeCameraId ? "active" : ""}`;
    button.innerHTML = `
      <span class="camera-tab-name">${camera.name}</span>
      <span class="camera-tab-meta">${camera.room} · ${camera.ip}</span>
    `;
    button.addEventListener("click", () => {
      state.activeCameraId = camera.id;
      renderCameraTabs();
      renderDevicePanel();
    });
    els.cameraTabs.appendChild(button);
  });
}

function renderOverlay(camera) {
  els.overlayBoxes.innerHTML = "";
  if (!camera) {
    return;
  }

  camera.markers.forEach((marker) => {
    const box = document.createElement("div");
    box.className = "person-box";
    box.dataset.label = marker.label;
    box.style.left = `${marker.x}%`;
    box.style.top = `${marker.y}%`;
    box.style.width = `${marker.w}%`;
    box.style.height = `${marker.h}%`;
    els.overlayBoxes.appendChild(box);
  });
}

function renderDevicePanel() {
  const camera = getActiveCamera();
  if (!camera) {
    els.activeName.textContent = "还没有摄像头";
    els.activeMeta.textContent = "等待扫描或手动添加";
    els.activeHint.textContent = "这个后台页会显示真实设备库中的摄像头。";
    els.activeLiveStatus.textContent = "尚未连接";
    els.signalChip.textContent = "等待设备";
    els.streamMode.textContent = state.defaults.recording;
    els.detailName.textContent = "未选择设备";
    els.detailRoom.textContent = "-";
    els.detailSource.textContent = "-";
    els.detailStream.textContent = "-";
    els.detailNotification.textContent = `${state.defaults.notificationChannel} / ${state.binding.channel}`;
    els.deviceBadge.textContent = "待接入";
    els.deviceBadge.style.color = "#cc7420";
    els.deviceBadge.style.background = "rgba(204, 116, 32, 0.14)";
    renderOverlay(null);
    renderShareLinks();
    syncPreviewLoop();
    return;
  }

  els.activeName.textContent = camera.name;
  els.activeMeta.textContent = `${camera.room} · ${camera.ip} · ${camera.transport}`;
  els.activeHint.textContent = camera.hint;
  els.activeLiveStatus.textContent = camera.liveStatus;
  els.signalChip.textContent = camera.signal;
  els.streamMode.textContent = camera.recordingMode;
  els.detailName.textContent = camera.name;
  els.detailRoom.textContent = camera.room;
  els.detailSource.textContent = camera.source;
  els.detailStream.textContent = camera.stream;
  els.detailNotification.textContent = camera.notification;
  els.deviceBadge.textContent = camera.status;

  if (camera.statusTone === "warning") {
    els.deviceBadge.style.color = "#cc7420";
    els.deviceBadge.style.background = "rgba(204, 116, 32, 0.14)";
  } else if (camera.statusTone === "offline") {
    els.deviceBadge.style.color = "#b94739";
    els.deviceBadge.style.background = "rgba(185, 71, 57, 0.14)";
  } else {
    els.deviceBadge.style.color = "#0f7d72";
    els.deviceBadge.style.background = "rgba(15, 125, 114, 0.12)";
  }

  renderOverlay(camera);
  renderShareLinks();
  syncPreviewLoop();
}

function renderShareLinks() {
  if (!els.shareLinkList) {
    return;
  }

  const camera = getActiveCamera();
  els.shareLinkList.innerHTML = "";

  if (!camera) {
    const item = document.createElement("li");
    item.className = "scan-result-item share-link-item";
    item.innerHTML = `
      <div class="scan-result-main share-link-copy">
        <span class="scan-result-title">等待选择设备</span>
        <span class="scan-result-note">选中一台摄像头后，这里会展示它已经登记过的共享链接记录。</span>
      </div>
      <div class="scan-result-actions share-link-actions">
        <span class="status-chip">尚无设备</span>
      </div>
    `;
    els.shareLinkList.appendChild(item);
    return;
  }

  if (!state.shareLinksLoaded) {
    const item = document.createElement("li");
    item.className = "scan-result-item share-link-item";
    item.innerHTML = `
      <div class="scan-result-main share-link-copy">
        <span class="scan-result-title">正在读取共享链接</span>
        <span class="scan-result-note">后台会返回已经登记的 ShareLink / MediaSession 记录，方便直接撤销。</span>
      </div>
      <div class="scan-result-actions share-link-actions">
        <span class="status-chip">加载中</span>
      </div>
    `;
    els.shareLinkList.appendChild(item);
    return;
  }

  if (state.shareLinksError) {
    const item = document.createElement("li");
    item.className = "scan-result-item share-link-item";
    item.innerHTML = `
      <div class="scan-result-main share-link-copy">
        <span class="scan-result-title">共享链接列表暂不可用</span>
        <span class="scan-result-note">${state.shareLinksError}</span>
      </div>
      <div class="scan-result-actions share-link-actions">
        <span class="status-chip approval-status-rejected">读取失败</span>
      </div>
    `;
    els.shareLinkList.appendChild(item);
    return;
  }

  const visibleLinks = state.shareLinks.filter((link) => link.deviceId === camera.id);
  if (!visibleLinks.length) {
    const item = document.createElement("li");
    item.className = "scan-result-item share-link-item";
    item.innerHTML = `
      <div class="scan-result-main share-link-copy">
        <span class="scan-result-title">这个设备还没有共享链接记录</span>
        <span class="scan-result-note">原始 token 不会在后台再次明文回显；如果要重新外发，请点上面的“生成共享链接”。</span>
      </div>
      <div class="scan-result-actions share-link-actions">
        <span class="status-chip">尚无记录</span>
      </div>
    `;
    els.shareLinkList.appendChild(item);
    return;
  }

  visibleLinks.forEach((link) => {
    const item = document.createElement("li");
    item.className = "scan-result-item share-link-item";

    const copy = document.createElement("div");
    copy.className = "scan-result-main share-link-copy";
    copy.innerHTML = `
      <span class="scan-result-title">${link.deviceName} · ${link.shareLinkId}</span>
      <div class="approval-pill-row share-link-pill-row">
        <span class="status-chip ${toShareLinkStatusClass(link.status)}">${toShareLinkStatusLabel(link.status)}</span>
        <span class="status-chip">${toShareAccessScopeLabel(link.accessScope)}</span>
        <span class="status-chip">${toShareSessionStatusLabel(link.sessionStatus)}</span>
      </div>
      <span class="share-link-submeta">
        Media Session: ${link.mediaSessionId || "未知"} · 发起时间 ${formatTimestamp(link.startedAt)}
        ${link.openedByUserId ? ` · 发起人 ${link.openedByUserId}` : ""}
      </span>
      <span class="scan-result-note">
        ${
          link.revokedAt
            ? `这条共享链路已在 ${formatTimestamp(link.revokedAt)} 被撤销。`
            : link.expiresAt
              ? `这条共享链路会在 ${formatTimestamp(link.expiresAt)} 过期；原始 token 不会在后台再次回显。`
              : "这条共享链路没有显式过期时间；原始 token 不会在后台再次回显。"
        }
      </span>
    `;

    const actions = document.createElement("div");
    actions.className = "scan-result-actions share-link-actions";
    if (link.canRevoke) {
      const revokeButton = document.createElement("button");
      revokeButton.className = "button button-danger";
      revokeButton.type = "button";
      revokeButton.textContent = "撤销链接";
      revokeButton.addEventListener("click", () => {
        handleShareLinkRevoke(link, revokeButton);
      });
      actions.appendChild(revokeButton);
    } else {
      const chip = document.createElement("span");
      chip.className = `status-chip ${toShareLinkStatusClass(link.status)}`.trim();
      chip.textContent = toShareLinkStatusLabel(link.status);
      actions.appendChild(chip);
    }

    item.appendChild(copy);
    item.appendChild(actions);
    els.shareLinkList.appendChild(item);
  });
}

function renderEvents() {
  els.eventList.innerHTML = "";
  state.events.forEach((event) => {
    const item = document.createElement("article");
    item.className = `event-item ${event.type}`;
    item.innerHTML = `
      <div class="event-stripe"></div>
      <div class="event-copy">
        <div class="event-title">${event.title}</div>
        <p>${event.body}</p>
      </div>
      <div class="event-time">${event.time}</div>
    `;
    els.eventList.appendChild(item);
  });
}

function renderTaskOutcome() {
  if (!els.taskOutcomeStatus) {
    return;
  }

  const outcome = state.latestTaskOutcome;
  if (!outcome) {
    els.taskOutcomeStatus.className = "status-chip";
    els.taskOutcomeStatus.textContent = "尚无结果";
    els.taskOutcomeMessage.textContent =
      "批准或拒绝之后，这里会显示任务执行状态、审计引用，以及交互回复和主动投递失败的分流结果。";
    els.taskOutcomeAction.textContent = "等待审批动作";
    els.taskOutcomeReply.textContent = "No interaction-linked reply reported yet.";
    els.taskOutcomeDeliveryFailure.textContent = "No proactive delivery failure reported yet.";
    els.taskOutcomeAudit.textContent = "尚未生成";
    els.taskOutcomeNotification.textContent = "尚未触发";
    els.taskOutcomeArtifacts.textContent = "尚无产物";
    els.taskOutcomeEvents.innerHTML =
      '<li>当前还没有一条完整的审批执行结果。下一次批准或拒绝后，这里会显示真实任务事件。</li>';
    return;
  }

  els.taskOutcomeStatus.className = `status-chip ${toTaskStatusClass(outcome.status)}`.trim();
  els.taskOutcomeStatus.textContent = toTaskStatusLabel(outcome.status);
  els.taskOutcomeMessage.textContent = outcome.message;
  els.taskOutcomeAction.textContent = outcome.actionLabel;
  els.taskOutcomeReply.textContent = outcome.interactionReplySummary;
  els.taskOutcomeDeliveryFailure.textContent = outcome.proactiveDeliveryFailureSummary;
  els.taskOutcomeAudit.textContent = outcome.auditRef
    ? `${outcome.taskId || "unknown"} / ${outcome.auditRef}`
    : outcome.taskId || "未生成审计引用";
  els.taskOutcomeNotification.textContent = outcome.notificationSummary;
  els.taskOutcomeArtifacts.textContent = outcome.artifactsSummary;

  els.taskOutcomeEvents.innerHTML = "";
  const events = Array.isArray(outcome.events) && outcome.events.length
    ? outcome.events
    : [
        {
          title: "任务结果已记录",
          body: "这次审批动作已经返回结果，但没有额外事件可显示。",
          occurredAt: "",
        },
      ];

  events.slice(0, 4).forEach((event) => {
    const item = document.createElement("li");
    item.textContent = `${event.title} · ${event.body}${
      event.occurredAt ? ` · ${formatTimestamp(event.occurredAt)}` : ""
    }`;
    els.taskOutcomeEvents.appendChild(item);
  });
}

function renderApprovals() {
  if (!els.approvalList) {
    return;
  }

  if (!state.approvalsLoaded) {
    renderApprovalPlaceholder(
      "正在同步审批队列",
      "页面启动后会读取当前待审批的高风险任务，这样管理员可以直接在后台做批准或拒绝。",
      "同步中"
    );
    return;
  }

  if (state.approvalsError) {
    renderApprovalPlaceholder(
      "审批队列暂时不可用",
      `当前无法读取待审批任务：${state.approvalsError}`,
      "读取失败"
    );
    return;
  }

  if (!state.approvals.length) {
    renderApprovalPlaceholder(
      "当前没有待审批动作",
      "像 camera.connect 这种默认需要审批的动作，触发后就会出现在这里。",
      "队列为空"
    );
    return;
  }

  els.approvalList.innerHTML = "";
  state.approvals.forEach((approval) => {
    const item = document.createElement("li");
    item.className = "scan-result-item approval-item";
    item.innerHTML = `
      <div class="scan-result-main approval-copy">
        <span class="scan-result-title">${toApprovalActionLabel(approval)}</span>
        <div class="approval-pill-row">
          <span class="status-chip ${toApprovalStatusClass(approval.status)}">${toApprovalStatusLabel(approval.status)}</span>
          <span class="status-chip ${toRiskClass(approval.riskLevel)}">${toRiskLabel(approval.riskLevel)}</span>
          <span class="pill pill-plan">${toAutonomyLabel(approval.autonomyLevel)}</span>
          <span class="pill">${approval.sourceChannel}</span>
        </div>
        <span class="scan-result-meta">${approval.requesterUserId} · ${approval.surface} · ${approval.domain}.${approval.action}</span>
        <span class="scan-result-note">${toApprovalReason(approval)}</span>
        <span class="approval-submeta">请求时间 ${formatTimestamp(approval.requestedAt)} · 会话 ${approval.conversationId || approval.sessionId || "未绑定会话"}</span>
      </div>
      <div class="scan-result-actions approval-actions">
        <button class="button button-primary" type="button" data-action="approve">批准并继续</button>
        <button class="button button-danger" type="button" data-action="reject">拒绝任务</button>
      </div>
    `;

    const approveButton = item.querySelector('[data-action="approve"]');
    const rejectButton = item.querySelector('[data-action="reject"]');

    approveButton.addEventListener("click", () => {
      handleApprovalDecision(approval, "approve", approveButton);
    });
    rejectButton.addEventListener("click", () => {
      handleApprovalDecision(approval, "reject", rejectButton);
    });

    els.approvalList.appendChild(item);
  });
}

function renderAccessMembers() {
  if (!els.accessMemberList) {
    return;
  }

  if (!state.accessMembersLoaded) {
    renderAccessMemberPlaceholder(
      "正在同步成员角色",
      "页面启动后会读取 workspace / membership / identity binding 投影，这里展示的是当前真正参与权限判断的成员视图。",
      "同步中"
    );
    return;
  }

  if (state.accessMembersError) {
    renderAccessMemberPlaceholder(
      "成员角色暂时不可用",
      `当前无法读取成员与角色：${state.accessMembersError}`,
      "读取失败"
    );
    return;
  }

  if (!state.accessMembers.length) {
    renderAccessMemberPlaceholder(
      "当前还没有成员记录",
      "后续完成绑定或手动配置后，这里会显示 workspace 内的成员、来源和角色。",
      "列表为空"
    );
    return;
  }

  els.accessMemberList.innerHTML = "";
  state.accessMembers.forEach((member) => {
    const item = document.createElement("li");
    item.className = "scan-result-item member-item";

    const copy = document.createElement("div");
    copy.className = "scan-result-main member-copy";

    const title = document.createElement("span");
    title.className = "scan-result-title";
    title.textContent = member.displayName;
    copy.appendChild(title);

    const pillRow = document.createElement("div");
    pillRow.className = "member-pill-row";

    const rolePill = document.createElement("span");
    rolePill.className = "status-chip";
    rolePill.textContent = toMemberRoleLabel(member.roleKind);
    pillRow.appendChild(rolePill);

    const statusPill = document.createElement("span");
    statusPill.className = "pill";
    statusPill.textContent = toMemberStatusLabel(member.membershipStatus);
    pillRow.appendChild(statusPill);

    const sourcePill = document.createElement("span");
    sourcePill.className = "pill";
    sourcePill.textContent = toMemberSourceLabel(member.source);
    pillRow.appendChild(sourcePill);

    const deliveryPill = document.createElement("span");
    deliveryPill.className = "pill pill-plan";
    deliveryPill.textContent = `默认投递 ${toMaybeLabel(member.proactiveDeliverySurface, "backend not exposed")}`;
    pillRow.appendChild(deliveryPill);

    const bindingAvailabilityLabel = toAvailabilityLabel(member.bindingAvailable ?? member.bindingAvailability);
    const bindingPill = document.createElement("span");
    bindingPill.className = `pill ${bindingAvailabilityLabel === "不可用" ? "" : "pill-live"}`.trim();
    bindingPill.textContent = `绑定 ${bindingAvailabilityLabel}`;
    pillRow.appendChild(bindingPill);

    copy.appendChild(pillRow);

    const meta = document.createElement("span");
    meta.className = "scan-result-meta";
    meta.textContent = [
      member.userId,
      member.openId ? `open_id ${member.openId}` : "",
      member.chatId ? `chat_id ${member.chatId}` : "",
      member.recentInteractiveSurface ? `recent ${member.recentInteractiveSurface}` : "",
    ]
      .filter(Boolean)
      .join(" · ");
    copy.appendChild(meta);

    const note = document.createElement("span");
    note.className = "scan-result-note";
    note.textContent = member.isOwner
      ? "这是当前 workspace 的 owner。这个入口只允许调整普通成员角色，不会改写 owner。"
      : member.canEdit
        ? "这里改的是 platform.memberships 里的真实角色，不再只是兼容层投影。"
        : "当前成员角色暂不可在这里修改。";
    copy.appendChild(note);

    const actions = document.createElement("div");
    actions.className = "scan-result-actions member-actions";

    const select = document.createElement("select");
    select.className = "member-role-select";
    select.disabled = !member.canEdit;

    const roleOptions = member.isOwner
      ? ["owner", "admin", "operator", "member", "viewer", "guest"]
      : ["admin", "operator", "member", "viewer", "guest"];
    roleOptions.forEach((roleKind) => {
      const option = document.createElement("option");
      option.value = roleKind;
      option.textContent = toMemberRoleLabel(roleKind);
      option.selected = roleKind === member.roleKind;
      select.appendChild(option);
    });
    actions.appendChild(select);

    const saveButton = document.createElement("button");
    saveButton.className = member.canEdit ? "button button-secondary" : "button button-ghost";
    saveButton.type = "button";
    saveButton.disabled = !member.canEdit;
    saveButton.textContent = member.canEdit ? "保存角色" : "角色固定";
    if (member.canEdit) {
      saveButton.addEventListener("click", () => {
        handleMemberRoleSave(member, select, saveButton);
      });
    }
    actions.appendChild(saveButton);

    const deliverySelect = document.createElement("select");
    deliverySelect.className = "member-role-select";
    deliverySelect.disabled = !member.canEdit;
    ["feishu", "weixin"].forEach((surface) => {
      const option = document.createElement("option");
      option.value = surface;
      option.textContent = surface === "feishu" ? "Feishu" : "Weixin";
      option.selected = surface === member.proactiveDeliverySurface;
      deliverySelect.appendChild(option);
    });
    actions.appendChild(deliverySelect);

    const deliveryButton = document.createElement("button");
    deliveryButton.className = member.canEdit ? "button button-secondary" : "button button-ghost";
    deliveryButton.type = "button";
    deliveryButton.disabled = !member.canEdit;
    deliveryButton.textContent = member.canEdit ? "保存默认投递面" : "默认投递固定";
    if (member.canEdit) {
      deliveryButton.addEventListener("click", () => {
        handleMemberDefaultDeliverySave(member, deliverySelect, deliveryButton);
      });
    }
    actions.appendChild(deliveryButton);

    item.appendChild(copy);
    item.appendChild(actions);
    els.accessMemberList.appendChild(item);
  });
}

function renderFeatureList(target, items, fallback) {
  if (!target) {
    return;
  }
  target.innerHTML = "";
  const lines = Array.isArray(items) && items.length ? items : [fallback];
  lines.forEach((line) => {
    const item = document.createElement("li");
    item.textContent = line;
    target.appendChild(item);
  });
}

function renderOverviewSignals() {
  if (!els.overviewReplyList || !els.overviewDeliveryList) {
    return;
  }

  const loading =
    !state.gatewayStatusLoaded ||
    !state.accountManagementLoaded ||
    !state.accessMembersLoaded ||
    !state.approvalsLoaded ||
    !state.modelEndpointsLoaded ||
    !state.modelPoliciesLoaded;
  const blocker =
    Boolean(state.gatewayStatusError) ||
    Boolean(state.accountManagementError) ||
    Boolean(state.accessMembersError) ||
    Boolean(state.approvalsError) ||
    Boolean(state.modelEndpointsError) ||
    Boolean(state.modelPoliciesError);
  const empty =
    !loading &&
    !blocker &&
    !state.gatewayStatus &&
    !state.accountManagement &&
    !state.accessMembers.length &&
    !state.approvals.length &&
    !state.modelEndpoints.length &&
    !state.modelPolicies.length;

  renderPageState(
    els.overviewPageState,
    loading ? "loading" : blocker ? "blocker" : empty ? "empty" : "success",
    "Overview page state",
    loading
      ? "Overview is waiting for the core admin-plane views to finish loading."
      : blocker
        ? "Overview has at least one backend blocker, so the shell shows the underlying page-level signals instead of pretending everything is healthy."
        : empty
          ? "Overview is connected, but the current workspace still has no members, approvals, model endpoints, or gateway surface to summarize."
          : "Overview is showing real gateway, account, task, model, and policy state from HarborBeacon.",
    [
      `interactive reply = ${(state.deliveryPolicy?.interactiveReply || "source_bound").replaceAll("_", "-")}`,
      `proactive delivery = ${(state.deliveryPolicy?.proactiveDelivery || "member_default").replaceAll("_", " ")}`,
      `Feishu baseline = ${state.gatewayStatus?.feishu ? "present" : "pending"}`,
      `Weixin parity = ${state.gatewayStatus?.weixin ? "present" : "pending"}`,
    ]
  );

  const replyLines = [
    `interactive reply = ${(state.deliveryPolicy?.interactiveReply || "source_bound").replaceAll("_", "-")}`,
    `current source = ${state.gatewayStatus?.bindingChannel || state.binding.channel || "originating source"}`,
    "reply routing stays attached to the source conversation rather than a free-floating inbox.",
  ];
  const deliveryLines = [
    `proactive delivery = ${(state.deliveryPolicy?.proactiveDelivery || "member_default").replaceAll("_", " ")}`,
    `member default surface = ${toMaybeLabel(
      state.accessMembers.find((member) => member.proactiveDeliverySurface)?.proactiveDeliverySurface,
      "backend not exposed"
    )}`,
    "delivery routing follows the member-scoped default when backend state is available.",
  ];

  renderFeatureList(els.overviewReplyList, replyLines, "interactive reply = source-bound");
  renderFeatureList(els.overviewDeliveryList, deliveryLines, "proactive delivery = member default");
}

function renderTaskSignals() {
  if (!els.taskReplyList || !els.taskDeliveryFailureList) {
    return;
  }

  const loading = !state.approvalsLoaded;
  const blocker = Boolean(state.approvalsError);
  const empty = !loading && !blocker && !state.approvals.length && !state.latestTaskOutcome;
  renderPageState(
    els.tasksApprovalsPageState,
    loading ? "loading" : blocker ? "blocker" : empty ? "empty" : "success",
    "Tasks & Approvals page state",
    loading
      ? "Tasks & Approvals is waiting for the approval queue and the latest task outcome."
      : blocker
        ? "Approval queue failed to load; the page still keeps interaction-linked replies and proactive failures split apart."
        : empty
          ? "There are currently no pending approvals and no recorded task outcome."
          : "Tasks & Approvals is showing live approval queue rows and interaction vs proactive outcome splits.",
    [
      `pending approvals = ${state.approvals.length}`,
      `latest outcome = ${state.latestTaskOutcome ? "present" : "none"}`,
    ]
  );

  const outcome = state.latestTaskOutcome;
  const replyLines = outcome
    ? [outcome.interactionReplySummary]
    : ["No interaction-linked reply reported yet."];
  const failureLines = outcome
    ? [outcome.proactiveDeliveryFailureSummary]
    : ["No proactive delivery failure reported yet."];

  renderFeatureList(els.taskReplyList, replyLines, "No interaction-linked reply reported yet.");
  renderFeatureList(
    els.taskDeliveryFailureList,
    failureLines,
    "No proactive delivery failure reported yet."
  );
}

function renderAccessMemberSignals() {
  if (!els.memberDeliveryDefaultList || !els.memberBindingAvailabilityList) {
    return;
  }

  const loading = !state.accountManagementLoaded || !state.accessMembersLoaded;
  const blocker = Boolean(state.accountManagementError || state.accessMembersError);
  const empty =
    !loading &&
    !blocker &&
    !state.accountManagement &&
    !state.accessMembers.length;
  renderPageState(
    els.accountManagementPageState,
    loading ? "loading" : blocker ? "blocker" : empty ? "empty" : "success",
    "Account Management page state",
    loading
      ? "Account Management is waiting for the workspace, member, and identity binding projections."
      : blocker
        ? "Account Management hit a backend blocker, so HarborDesk shows the current governance evidence instead of a mock member list."
        : empty
          ? "The workspace is reachable, but no members or identity bindings have been projected yet."
          : "Account Management is ready for member roles, default proactive delivery, and binding governance.",
    [
      `members = ${state.accessMembers.length}`,
      `default delivery defaults exposed = ${state.accessMembers.some((member) => member.proactiveDeliverySurface) ? "yes" : "no"}`,
    ]
  );

  if (!state.accessMembersLoaded) {
    renderFeatureList(els.memberDeliveryDefaultList, [], "正在加载 member default proactive delivery...");
    renderFeatureList(els.memberBindingAvailabilityList, [], "正在加载 binding availability...");
    return;
  }

  if (state.accessMembersError) {
    renderFeatureList(
      els.memberDeliveryDefaultList,
      [],
      `member default proactive delivery 暂不可用：${state.accessMembersError}`
    );
    renderFeatureList(
      els.memberBindingAvailabilityList,
      [],
      `binding availability 暂不可用：${state.accessMembersError}`
    );
    return;
  }

  const deliveryLines = state.accessMembers.map((member) => {
    const surface = toMaybeLabel(member.proactiveDeliverySurface, "backend not exposed");
    return `${member.displayName} · surface=${surface} · default=${toDefaultLabel(member.proactiveDeliveryDefault)}`;
  });
  const bindingLines = state.accessMembers.map((member) => {
    const note = member.bindingAvailabilityNote ? ` · ${member.bindingAvailabilityNote}` : "";
    return `${member.displayName} · availability=${toAvailabilityLabel(
      member.bindingAvailable ?? member.bindingAvailability
    )}${note}`;
  });

  renderFeatureList(
    els.memberDeliveryDefaultList,
    deliveryLines,
    "当前还没有 member-scoped proactive delivery surface。"
  );
  renderFeatureList(
    els.memberBindingAvailabilityList,
    bindingLines,
    "当前还没有 binding availability 信号。"
  );
}

function renderAccountManagement() {
  if (!state.accountManagementLoaded) {
    renderFeatureList(els.workspaceSummaryList, [], "正在加载 workspace summary...");
    renderFeatureList(els.roleCountList, [], "正在加载 role counts...");
    renderFeatureList(els.identityBindingList, [], "正在加载 identity bindings...");
    renderFeatureList(els.governanceList, [], "正在加载 access governance...");
    return;
  }

  if (state.accountManagementError || !state.accountManagement) {
    renderPageState(
      els.accountManagementPageState,
      "blocker",
      "Account Management page state",
      `账号治理读面暂不可用：${state.accountManagementError || "unknown error"}`,
      ["Please refresh the admin-plane state after the backend endpoint recovers."]
    );
    const fallback = `账号治理读面暂不可用：${state.accountManagementError || "unknown error"}`;
    renderFeatureList(els.workspaceSummaryList, [], fallback);
    renderFeatureList(els.roleCountList, [], "等待 /api/account-management 恢复。");
    renderFeatureList(els.identityBindingList, [], "等待 identity binding 读面恢复。");
    renderFeatureList(els.governanceList, [], "等待 governance summary 恢复。");
    return;
  }

  const { workspace, memberRoleCounts, identityBindings, accessGovernance } = state.accountManagement;
  const workspaceLines = workspace
    ? [
        `${workspace.display_name || workspace.workspace_id} · ${workspace.workspace_type || "workspace"} · ${workspace.status || "active"}`,
        `owner=${workspace.owner_user_id || "unknown"} · timezone=${workspace.timezone || "unknown"} · locale=${workspace.locale || "unknown"}`,
        `members=${workspace.member_count || 0} · active=${workspace.active_member_count || 0} · bindings=${workspace.identity_binding_count || 0}`,
        `permission_rules=${workspace.permission_rule_count || 0} · provider_accounts=${workspace.provider_account_count || 0}`,
      ]
    : [];
  const roleLines = memberRoleCounts.map((entry) => {
    return `${entry.role_kind || "unknown"}: ${entry.member_count || 0} total / ${entry.active_member_count || 0} active`;
  });
  const bindingLines = identityBindings.map((binding) => {
    return `${binding.display_name || binding.user_id || "未命名成员"} · ${binding.role_kind || "viewer"} · ${binding.provider_key || "provider"} · open_id=${binding.open_id || "-"}`;
  });
  const governanceLines = accessGovernance
    ? [
        `permission_rules=${accessGovernance.permission_rule_count || 0} · owners=${accessGovernance.owner_count || 0} · members=${accessGovernance.member_count || 0}`,
        ...((Array.isArray(accessGovernance.role_policies) ? accessGovernance.role_policies : []).map(
          (policy) =>
            `${policy.role_kind || "unknown"}: rules=${policy.permission_rule_count || 0}, members=${policy.member_count || 0}, active=${policy.active_member_count || 0}`
        )),
      ]
    : [];

  renderFeatureList(els.workspaceSummaryList, workspaceLines, "当前还没有 workspace summary。");
  renderFeatureList(els.roleCountList, roleLines, "当前还没有 role count 数据。");
  renderFeatureList(els.identityBindingList, bindingLines, "当前还没有 identity binding。");
  renderFeatureList(els.governanceList, governanceLines, "当前还没有 access governance summary。");
}

function renderGatewayStatus() {
  if (!state.gatewayStatusLoaded) {
    renderPageState(
      els.imGatewayPageState,
      "loading",
      "IM Gateway page state",
      "IM Gateway is waiting for HarborGate setup/status and the paired Feishu/Weixin readiness signals.",
      ["Feishu baseline = loading", "Weixin parity = loading", "source-bound queue/failure = loading"]
    );
    renderFeatureList(els.gatewayFeishuList, [], "正在加载 Feishu baseline...");
    renderFeatureList(els.gatewayWeixinList, [], "正在加载 Weixin parity...");
    renderFeatureList(
      els.gatewayChannelList,
      [],
      "正在加载 source-bound queue / failure signals..."
    );
    if (els.gatewayStatusNote) {
      els.gatewayStatusNote.textContent = "正在读取 HarborGate 状态。";
    }
    return;
  }

  if (state.gatewayStatusError || !state.gatewayStatus) {
    renderPageState(
      els.imGatewayPageState,
      "blocker",
      "IM Gateway page state",
      `IM Gateway cannot yet surface same-origin readiness: ${state.gatewayStatusError || "unknown error"}.`,
      ["Feishu baseline blocked", "Weixin parity blocked", "source-bound queue/failure unavailable"]
    );
    renderFeatureList(
      els.gatewayFeishuList,
      [],
      `Feishu baseline 暂不可用：${state.gatewayStatusError || "unknown error"}`
    );
    renderFeatureList(
      els.gatewayWeixinList,
      [],
      "Weixin parity 暂不可用，因为同源状态未能返回。"
    );
    renderFeatureList(
      els.gatewayChannelList,
      [],
      "HarborDesk 还拿不到 source-bound queue/failure signals；当前轮次只能显示明确 blocker。"
    );
    if (els.gatewayStatusNote) {
      els.gatewayStatusNote.textContent =
        "Feishu baseline / Weixin parity 的实时信号尚未到达 HarborDesk；请确认 HarborGate 状态面可读。";
    }
    return;
  }

  const gateway = state.gatewayStatus;
  const feishu = gateway.feishu;
  const weixin = gateway.weixin;
  const feishuLines = [];
  if (feishu) {
    feishuLines.push(
      `configured=${yesNo(feishu.configured)} · connected=${yesNo(feishu.connected)} · status=${
        feishu.transport_status || feishu.status || "unknown"
      }`
    );
    feishuLines.push(
      `reply=${yesNo(feishu.capabilities?.reply ?? feishu.reply)} · delivery=${yesNo(
        feishu.capabilities?.delivery ?? feishu.delivery
      )} · attachments=${yesNo(feishu.capabilities?.attachments ?? feishu.attachments)}`
    );
    feishuLines.push(
      `binding=${toMaybeLabel(gateway.bindingStatus, "unknown")} · channel=${toMaybeLabel(
        gateway.bindingChannel,
        "unknown"
      )}`
    );
  } else {
    feishuLines.push(
      `Feishu baseline · waiting for HarborGate setup/status read surface (current provider=${toMaybeLabel(
        gateway.bridgeProvider.platform,
        "unknown"
      )})`
    );
  }

  const weixinLines = [];
  if (weixin) {
    weixinLines.push(
      `configured=${yesNo(weixin.configured)} · connected=${yesNo(weixin.connected)} · status=${
        weixin.transport_status || weixin.status || "unknown"
      }`
    );
    weixinLines.push(
      `poll=${toMaybeLabel(weixin.poll?.last_getupdates_at, "n/a")} · private_dm_count=${
        weixin.poll?.last_private_text_message_count ?? 0
      }`
    );
    weixinLines.push(
      `parity=${toMaybeLabel(weixin.parity_state || weixin.parity || weixin.mode, "unknown")} · blocker=${toMaybeLabel(
        weixin.blocker_category || weixin.blocker,
        "none"
      )}`
    );
  } else {
    weixinLines.push(
      "Weixin parity track · waiting for HarborGate setup/status read surface before HarborDesk can show ingress and context-token signals."
    );
  }

  const signalLines = [
    ...normalizeSignalList(gateway.sourceBoundQueue).map((line) => `queue · ${line}`),
    ...normalizeSignalList(gateway.sourceBoundFailures).map((line) => `failure · ${line}`),
  ];

  renderFeatureList(els.gatewayFeishuList, feishuLines, "当前没有 Feishu baseline 状态。");
  renderFeatureList(els.gatewayWeixinList, weixinLines, "当前没有 Weixin parity 状态。");
  renderFeatureList(
    els.gatewayChannelList,
    signalLines,
    "当前没有 source-bound queue/failure signals。"
  );
  if (els.gatewayStatusNote) {
    els.gatewayStatusNote.textContent =
      gateway.source === "harborgate_status"
        ? "HarborDesk 正在显示 HarborGate 的真实 setup/status、Feishu baseline、Weixin parity，以及 source-bound queue/failure signals。"
        : "当前只拿到了 HarborBeacon 本地 bridge summary；如果要显示 Feishu/Weixin 的细粒度 parity 信号，还需要 HarborGate setup/status 同源可读。";
  }

  renderPageState(
    els.imGatewayPageState,
    gateway.source === "harborgate_status" ? "success" : "empty",
    "IM Gateway page state",
    gateway.source === "harborgate_status"
      ? "HarborDesk is showing same-origin HarborGate setup/status with Feishu baseline and Weixin parity side by side."
      : "HarborDesk only has a local bridge summary right now; full parity data needs HarborGate same-origin state.",
    [
      `feishu = ${feishu ? "present" : "pending"}`,
      `weixin = ${weixin ? "present" : "pending"}`,
      `queue/failure signals = ${signalLines.length}`,
    ]
  );
}

function renderModelsPolicies() {
  if (!els.modelEndpointList || !els.modelPolicyList) {
    return;
  }

  const loading =
    !state.modelEndpointsLoaded ||
    !state.modelPoliciesLoaded ||
    !state.featureAvailabilityLoaded;
  const blocker = Boolean(
    state.modelEndpointsError || state.modelPoliciesError || state.featureAvailabilityError
  );
  const empty =
    !loading &&
    !blocker &&
    !state.modelEndpoints.length &&
    !state.modelPolicies.length &&
    !state.featureAvailabilityGroups.length;
  const mismatchCount = state.featureAvailabilityGroups
    .flatMap((group) => group.items || [])
    .filter(hasFeatureProjectionMismatch).length;

  renderPageState(
    els.modelsPoliciesPageState,
    loading ? "loading" : blocker ? "blocker" : empty ? "empty" : "success",
    "Models & Policies page state",
    loading
      ? "Model Center is waiting for endpoint, route policy, and feature-availability projections."
      : blocker
        ? "At least one model-center read path has a backend blocker; HarborDesk will show the exact endpoint/policy gap instead of pretending policy state exists."
        : empty
          ? "Model Center is reachable, but there are no endpoints, route policies, or feature-availability rows projected yet."
          : "Model Center is showing endpoint status, runtime-overlay alignment, and route policy/fallback order side by side.",
    [
      `endpoints = ${state.modelEndpoints.length}`,
      `route policies = ${state.modelPolicies.length}`,
      `projection mismatches = ${mismatchCount}`,
      `VLM endpoints = ${state.modelEndpoints.filter((endpoint) => String(endpoint.modelKind || "").toLowerCase() === "vlm").length}`,
    ]
  );

  if (els.modelEndpointSummary) {
    const active = state.modelEndpoints.filter((endpoint) => String(endpoint.status || "").toLowerCase() === "active").length;
    const degraded = state.modelEndpoints.filter((endpoint) => String(endpoint.status || "").toLowerCase() === "degraded").length;
    const disabled = state.modelEndpoints.filter((endpoint) => String(endpoint.status || "").toLowerCase() === "disabled").length;
    els.modelEndpointSummary.textContent = loading
      ? "Waiting for model endpoint projection..."
      : blocker
        ? `Model endpoint read blocked: ${state.modelEndpointsError || state.modelPoliciesError || state.featureAvailabilityError}`
        : state.modelEndpoints.length
          ? `${state.modelEndpoints.length} endpoints · active ${active} · degraded ${degraded} · disabled ${disabled}`
          : "No endpoints projected yet.";
  }

  if (els.modelPolicySummary) {
    const policyCount = state.modelPolicies.length;
    els.modelPolicySummary.textContent = loading
      ? "Waiting for route policy projection..."
      : blocker
        ? `Route policy read blocked: ${state.modelPoliciesError || state.modelEndpointsError || state.featureAvailabilityError}`
        : policyCount
          ? `${policyCount} route policies · fallback order and privacy level are editable below`
          : "No route policies projected yet.";
  }

  if (els.modelRuntimeAlignmentList) {
    if (!state.featureAvailabilityLoaded) {
      renderFeatureList(
        els.modelRuntimeAlignmentList,
        [],
        "正在加载 runtime projection 和配置投影对齐结果..."
      );
    } else if (state.featureAvailabilityError) {
      renderFeatureList(
        els.modelRuntimeAlignmentList,
        [],
        `runtime alignment 暂不可用：${state.featureAvailabilityError}`
      );
    } else {
      const mismatchItems = state.featureAvailabilityGroups
        .flatMap((group) => group.items || [])
        .filter(hasFeatureProjectionMismatch);
      const alignmentLines = mismatchItems.length
        ? mismatchItems.map((item) => {
            return `${item.label} · runtime truth is overriding stale config projection · source=${item.sourceOfTruth}`;
          })
        : [
            "Runtime truth currently matches the projected endpoint/policy state for the first-wave feature matrix.",
          ];
      renderFeatureList(
        els.modelRuntimeAlignmentList,
        alignmentLines,
        "当前没有 runtime/config alignment 信号。"
      );
    }
  }

  els.modelEndpointList.innerHTML = "";
  if (loading) {
    renderFeatureList(
      els.modelEndpointList,
      [],
      "正在加载 model endpoints、test results、kind/provider、route policy 以及 fallback order。"
    );
  } else if (blocker) {
    renderFeatureList(
      els.modelEndpointList,
      [],
      `model endpoints 暂不可用：${state.modelEndpointsError || "unknown error"}`
    );
  } else if (!state.modelEndpoints.length) {
    renderFeatureList(
      els.modelEndpointList,
      [],
      "当前 workspace 还没有 model endpoint，endpoint status 和 test result 的操作位会在这里出现。"
    );
  } else {
    state.modelEndpoints.forEach((endpoint) => {
      const item = document.createElement("li");
      item.className = "scan-result-item approval-item";
      item.dataset.endpointId = endpoint.modelEndpointId;

      const copy = document.createElement("div");
      copy.className = "scan-result-main approval-copy";

      const title = document.createElement("span");
      title.className = "scan-result-title";
      title.textContent = endpoint.modelName || endpoint.modelEndpointId || "未命名端点";
      copy.appendChild(title);

      const pills = document.createElement("div");
      pills.className = "approval-pill-row";

      const statusPill = document.createElement("span");
      statusPill.className = `status-chip ${toModelStatusLabel(endpoint.status) === "active" ? "approval-status-approved" : toModelStatusLabel(endpoint.status) === "degraded" ? "approval-status-pending" : "approval-status-rejected"}`;
      statusPill.textContent = `status=${toModelStatusLabel(endpoint.status)}`;
      pills.appendChild(statusPill);

      const kindPill = document.createElement("span");
      kindPill.className = "pill pill-plan";
      kindPill.textContent = `kind=${toModelKindLabel(endpoint.modelKind)}`;
      pills.appendChild(kindPill);

      const endpointKindPill = document.createElement("span");
      endpointKindPill.className = "pill";
      endpointKindPill.textContent = `endpoint=${toEndpointKindLabel(endpoint.endpointKind)}`;
      pills.appendChild(endpointKindPill);

      const providerPill = document.createElement("span");
      providerPill.className = "pill";
      providerPill.textContent = `provider=${endpoint.providerKey || "unknown"}`;
      pills.appendChild(providerPill);

      const testResult = state.modelEndpointTestResults[endpoint.modelEndpointId];
      const testPill = document.createElement("span");
      testPill.className = `model-result-chip ${testResult ? (testResult.ok ? "ok" : "failed") : "unknown"}`;
      testPill.textContent = testResult
        ? `test=${testResult.ok ? "ok" : "failed"} · ${toMaybeLabel(testResult.status, "unknown")}`
        : "test=not run";
      pills.appendChild(testPill);

      const policy = matchRoutePolicyForEndpoint(endpoint, state.modelPolicies);
      const policyPill = document.createElement("span");
      policyPill.className = "pill";
      policyPill.textContent = `route policy=${policy ? policy.routePolicyId : "unmapped"}`;
      pills.appendChild(policyPill);

      copy.appendChild(pills);

      const meta = document.createElement("span");
      meta.className = "scan-result-meta";
      meta.textContent = [
        `endpoint_id=${endpoint.modelEndpointId}`,
        `provider_account=${endpoint.providerAccountId || "-"}`,
        `workspace=${endpoint.workspaceId || "-"}`,
      ].join(" · ");
      copy.appendChild(meta);

      const note = document.createElement("span");
      note.className = "scan-result-note";
      note.textContent = [
        `capability_tags=${endpoint.capabilityTags.length ? endpoint.capabilityTags.join(", ") : "none"}`,
        `cost_policy=${typeof endpoint.costPolicy === "object" ? JSON.stringify(endpoint.costPolicy) : String(endpoint.costPolicy || "{}")}`,
      ].join(" · ");
      copy.appendChild(note);

      const fields = document.createElement("div");
      fields.className = "model-row-fields";
      fields.innerHTML = `
        <label>
          Model kind
          <select data-field="model_kind">
            ${["llm", "vlm", "ocr", "asr", "detector", "embedder"]
              .map(
                (kind) =>
                  `<option value="${kind}" ${kind === String(endpoint.modelKind || "").toLowerCase() ? "selected" : ""}>${toModelKindLabel(kind)}</option>`
              )
              .join("")}
          </select>
        </label>
        <label>
          Endpoint kind
          <select data-field="endpoint_kind">
            ${["local", "sidecar", "cloud"]
              .map(
                (kind) =>
                  `<option value="${kind}" ${kind === String(endpoint.endpointKind || "").toLowerCase() ? "selected" : ""}>${toEndpointKindLabel(kind)}</option>`
              )
              .join("")}
          </select>
        </label>
        <label>
          Provider key
          <input data-field="provider_key" type="text" value="${endpoint.providerKey || ""}" />
        </label>
        <label>
          Model name
          <input data-field="model_name" type="text" value="${endpoint.modelName || ""}" />
        </label>
        <label>
          Provider account id
          <input data-field="provider_account_id" type="text" value="${endpoint.providerAccountId || ""}" placeholder="Optional" />
        </label>
        <label>
          Status
          <select data-field="status">
            ${["active", "degraded", "disabled"]
              .map(
                (status) =>
                  `<option value="${status}" ${status === String(endpoint.status || "").toLowerCase() ? "selected" : ""}>${toModelStatusLabel(status)}</option>`
              )
              .join("")}
          </select>
        </label>
        <label>
          Capability tags
          <input data-field="capability_tags" type="text" value="${endpoint.capabilityTags.join(", ")}" placeholder="ocr, vision, answer" />
        </label>
        <label>
          Test result
          <input data-field="test_result" type="text" value="${testResult ? `${testResult.ok ? "ok" : "failed"} · ${testResult.summary || testResult.status || "unknown"}` : "not run yet"}" readonly />
        </label>
      `;
      copy.appendChild(fields);

      const actions = document.createElement("div");
      actions.className = "scan-result-actions model-row-actions";

      const saveButton = document.createElement("button");
      saveButton.className = "button button-secondary";
      saveButton.type = "button";
      saveButton.textContent = "Save endpoint";
      saveButton.addEventListener("click", () => {
        handleModelEndpointSave(endpoint, item, saveButton);
      });
      actions.appendChild(saveButton);

      const testButton = document.createElement("button");
      testButton.className = "button button-primary";
      testButton.type = "button";
      testButton.textContent = "Test endpoint";
      testButton.addEventListener("click", () => {
        handleModelEndpointTest(endpoint, item, testButton);
      });
      actions.appendChild(testButton);

      item.appendChild(copy);
      item.appendChild(actions);
      els.modelEndpointList.appendChild(item);
    });
  }

  els.modelPolicyList.innerHTML = "";
  if (loading) {
    renderFeatureList(
      els.modelPolicyList,
      [],
      "正在加载 route policies、privacy level、fallback order 和默认路由。"
    );
  } else if (blocker) {
    renderFeatureList(
      els.modelPolicyList,
      [],
      `route policies 暂不可用：${state.modelPoliciesError || "unknown error"}`
    );
  } else if (!state.modelPolicies.length) {
    renderFeatureList(
      els.modelPolicyList,
      [],
      "当前 workspace 还没有 route policy，fallback order 和默认路由会在这里配置。"
    );
  } else {
    state.modelPolicies.forEach((policy, index) => {
      const item = document.createElement("li");
      item.className = "scan-result-item approval-item";
      item.dataset.routePolicyId = policy.routePolicyId;

      const copy = document.createElement("div");
      copy.className = "scan-result-main approval-copy";

      const title = document.createElement("span");
      title.className = "scan-result-title";
      title.textContent = policy.routePolicyId;
      copy.appendChild(title);

      const pills = document.createElement("div");
      pills.className = "approval-pill-row";

      const statusPill = document.createElement("span");
      statusPill.className = `status-chip ${policy.status === "active" ? "approval-status-approved" : policy.status === "degraded" ? "approval-status-pending" : "approval-status-rejected"}`;
      statusPill.textContent = `status=${policy.status || "unknown"}`;
      pills.appendChild(statusPill);

      const domainPill = document.createElement("span");
      domainPill.className = "pill pill-plan";
      domainPill.textContent = `domain=${policy.domainScope || "-"}`;
      pills.appendChild(domainPill);

      const modalityPill = document.createElement("span");
      modalityPill.className = "pill";
      modalityPill.textContent = `modality=${policy.modality || "-"}`;
      pills.appendChild(modalityPill);

      const privacyPill = document.createElement("span");
      privacyPill.className = "pill";
      privacyPill.textContent = `privacy=${toPrivacyLevelLabel(policy.privacyLevel)}`;
      pills.appendChild(privacyPill);

      copy.appendChild(pills);

      const meta = document.createElement("span");
      meta.className = "scan-result-meta";
      meta.textContent = `workspace=${policy.workspaceId || "-"} · local_preferred=${toBooleanLabel(
        policy.localPreferred
      )} · max_cost_per_run=${policy.maxCostPerRun ?? "unbounded"}`;
      copy.appendChild(meta);

      const note = document.createElement("span");
      note.className = "scan-result-note";
      note.textContent = `fallback_order=${toFallbackOrderText(policy.fallbackOrder)} · metadata=${JSON.stringify(policy.metadata || {})}`;
      copy.appendChild(note);

      const fields = document.createElement("div");
      fields.className = "model-route-fields";
      fields.innerHTML = `
        <label>
          Privacy level
          <select data-field="privacy_level">
            ${["strict_local", "allow_redacted_cloud", "allow_cloud"]
              .map(
                (level) =>
                  `<option value="${level}" ${level === String(policy.privacyLevel || "").toLowerCase() ? "selected" : ""}>${toPrivacyLevelLabel(level)}</option>`
              )
              .join("")}
          </select>
        </label>
        <label>
          Status
          <select data-field="status">
            ${["active", "degraded", "disabled"]
              .map(
                (status) =>
                  `<option value="${status}" ${status === String(policy.status || "").toLowerCase() ? "selected" : ""}>${status}</option>`
              )
              .join("")}
          </select>
        </label>
        <label>
          Local preferred
          <select data-field="local_preferred">
            <option value="true" ${policy.localPreferred ? "selected" : ""}>yes</option>
            <option value="false" ${!policy.localPreferred ? "selected" : ""}>no</option>
          </select>
        </label>
        <label>
          Max cost per run
          <input data-field="max_cost_per_run" type="number" step="0.01" min="0" value="${policy.maxCostPerRun ?? ""}" placeholder="Optional" />
        </label>
        <label class="model-route-inline" style="grid-column: 1 / -1;">
          Fallback order
          <input data-field="fallback_order" type="text" value="${policy.fallbackOrder.join(", ")}" placeholder="local -> sidecar -> cloud" />
        </label>
      `;
      copy.appendChild(fields);

      const actions = document.createElement("div");
      actions.className = "scan-result-actions model-route-actions";

      const saveButton = document.createElement("button");
      saveButton.className = "button button-secondary";
      saveButton.type = "button";
      saveButton.textContent = "Save route policies";
      saveButton.addEventListener("click", () => {
        handleModelPoliciesSave(saveButton);
      });
      actions.appendChild(saveButton);

      item.appendChild(copy);
      item.appendChild(actions);
      els.modelPolicyList.appendChild(item);
    });
  }

  if (els.modelCenterNotes) {
    const notes = [
      `endpoint status/test result/kind/provider now have a real admin-plane home.`,
      `route policy and fallback order are editable in place and saved back to /api/models/policies.`,
      `runtime truth from 4176 can override stale llm/embedder projection without rewriting the stored admin state.`,
      `VLM first now covers still images, snapshots, and DVR keyframe sidecars; audio transcript extraction remains pending.`,
    ];
    renderFeatureList(els.modelCenterNotes, notes, "Model Center notes pending.");
  }
}

function renderAiotSummary() {
  renderPageState(
    els.devicesAiotPageState,
    "success",
    "Devices & AIoT page state",
    "Devices & AIoT stays on the Home Device Domain path and keeps HarborOS out of device-native control.",
    [
      `device count = ${state.cameras.length}`,
      "discover / snapshot / share_link / inspect / control remain owned by AIoT",
    ]
  );
  const lines = [
    `owned_actions=discover / snapshot / share_link / inspect / control`,
    `device_count=${state.cameras.length} · runtime control remains in Home Device Domain`,
    "retrieval/control separation stays explicit: HarborOS does not own device-native control.",
    "non_regression=route_key stays opaque routing metadata · resume_token stays business-flow continuation",
  ];
  renderFeatureList(els.aiotSummaryList, lines, "等待 AIoT boundary summary。");
}

function renderHarborOsSummary() {
  renderPageState(
    els.harborosPageState,
    "success",
    "HarborOS page state",
    "HarborOS is presented as live status plus proof summary, with the fallback order and writable root kept explicit.",
    [
      `route order = ${HARBOROS_ROUTE_ORDER.join(" -> ")}`,
      `verifier lines = ${Object.values(HARBOROS_VERIFIER_LINE_LABELS).join(" / ")}`,
    ]
  );
  const liveStatusLines = [
    "live_status=HarborDesk keeps HarborOS live status separate from proof summary",
    `route_order=${HARBOROS_ROUTE_ORDER.join(" -> ")}`,
    "writable_root=/mnt/software/harborbeacon-agent-ci",
  ];
  const proofSummaryLines = [
    "proof_summary=service.query / files.list / service.restart / files.copy / files.move",
    (
      "verifier_line_labels="
      + `middleware_first:${HARBOROS_VERIFIER_LINE_LABELS.middleware_first} · `
      + `midcli_fallback:${HARBOROS_VERIFIER_LINE_LABELS.midcli_fallback}`
    ),
    "pause_conditions=browser/MCP drift, midcli_fallback spikes, executor loss, or writable-root escape",
  ];
  renderFeatureList(els.harborosLiveStatusList, liveStatusLines, "等待 HarborOS live status block。");
  renderFeatureList(els.harborosProofSummaryList, proofSummaryLines, "等待 HarborOS proof summary block。");
  if (els.harborosProofNote) {
    els.harborosProofNote.textContent =
      "HarborDesk renders HarborOS live status and proof summary separately. The live block describes current system-domain state; the proof block carries the reviewer-facing evidence rows and verifier lines.";
  }
}

function renderFeatureAvailabilityGroups() {
  if (!els.featureAvailabilityGroups || !els.featureAvailabilitySummary) {
    return;
  }

  els.featureAvailabilityGroups.innerHTML = "";

  if (!state.featureAvailabilityLoaded) {
    els.featureAvailabilitySummary.textContent =
      "Waiting for grouped feature-availability projection from runtime truth and gateway state.";
    return;
  }

  if (state.featureAvailabilityError) {
    els.featureAvailabilitySummary.textContent =
      `Feature availability read blocked: ${state.featureAvailabilityError}`;
    return;
  }

  const groups = Array.isArray(state.featureAvailabilityGroups)
    ? state.featureAvailabilityGroups
    : [];
  const items = groups.flatMap((group) => group.items || []);
  const availableCount = items.filter(
    (item) => String(item.status || "").toLowerCase() === "available"
  ).length;
  const blockerCount = items.filter(
    (item) => String(item.status || "").toLowerCase() === "blocked"
  ).length;
  const mismatchCount = items.filter(hasFeatureProjectionMismatch).length;

  els.featureAvailabilitySummary.textContent = groups.length
    ? `${items.length} feature rows · available ${availableCount} · blocked ${blockerCount} · projection mismatches ${mismatchCount}`
    : "No grouped feature availability has been projected yet.";

  if (!groups.length) {
    return;
  }

  groups.forEach((group) => {
    const card = document.createElement("article");
    card.className = "stack-card muted-card";

    const title = document.createElement("h4");
    title.textContent = group.label || group.groupId || "Unnamed group";
    card.appendChild(title);

    const list = document.createElement("ul");
    list.className = "scan-result-list";

    if (!group.items.length) {
      const empty = document.createElement("li");
      empty.className = "scan-result-item";
      empty.textContent = "No feature rows projected yet for this group.";
      list.appendChild(empty);
      card.appendChild(list);
      els.featureAvailabilityGroups.appendChild(card);
      return;
    }

    group.items.forEach((item) => {
      const row = document.createElement("li");
      row.className = "scan-result-item approval-item";
      row.dataset.featureId = item.featureId;

      const copy = document.createElement("div");
      copy.className = "scan-result-main approval-copy";

      const heading = document.createElement("span");
      heading.className = "scan-result-title";
      heading.textContent = item.label || item.featureId || "Unnamed feature";
      copy.appendChild(heading);

      const pills = document.createElement("div");
      pills.className = "approval-pill-row";

      const statusPill = document.createElement("span");
      statusPill.className = `status-chip ${toFeatureStatusClass(item.status)}`.trim();
      statusPill.textContent = `status=${toFeatureStatusLabel(item.status)}`;
      pills.appendChild(statusPill);

      const ownerPill = document.createElement("span");
      ownerPill.className = "pill pill-plan";
      ownerPill.textContent = `owner=${item.ownerLane || "unknown"}`;
      pills.appendChild(ownerPill);

      const sourcePill = document.createElement("span");
      sourcePill.className = "pill";
      sourcePill.textContent = `source=${item.sourceOfTruth || "unknown"}`;
      pills.appendChild(sourcePill);

      copy.appendChild(pills);

      const meta = document.createElement("span");
      meta.className = "scan-result-meta";
      meta.textContent = [
        `current=${toMaybeLabel(item.currentOption, "unconfigured")}`,
        `fallback=${item.fallbackOrder.length ? item.fallbackOrder.join(" -> ") : "n/a"}`,
      ].join(" · ");
      copy.appendChild(meta);

      const note = document.createElement("span");
      note.className = "scan-result-note";
      note.textContent = [
        `blocker=${toMaybeLabel(item.blocker, "none")}`,
        `edit=${toFeatureEditHint(item.featureId)}`,
      ].join(" · ");
      copy.appendChild(note);

      if (Array.isArray(item.evidence) && item.evidence.length) {
        const evidence = document.createElement("span");
        evidence.className = "scan-result-note";
        evidence.textContent = `evidence=${item.evidence.join(" | ")}`;
        copy.appendChild(evidence);
      }

      row.appendChild(copy);
      list.appendChild(row);
    });

    card.appendChild(list);
    els.featureAvailabilityGroups.appendChild(card);
  });
}

function renderSystemSettings() {
  const gateway = state.gatewayStatus;
  const loading = !state.gatewayStatusLoaded || !state.featureAvailabilityLoaded;
  const blocker = Boolean(state.gatewayStatusError || state.featureAvailabilityError);
  const empty =
    !loading &&
    !blocker &&
    !gateway &&
    !state.featureAvailabilityGroups.length;
  renderPageState(
    els.systemSettingsPageState,
    loading ? "loading" : blocker ? "blocker" : empty ? "empty" : "success",
    "System Settings page state",
    loading
      ? "System Settings is waiting for real routing, gateway status, and feature availability."
      : blocker
        ? "Routing/gateway status has a backend blocker, so the page shows only explicit blockers instead of inferred settings."
        : empty
          ? "Routing policy is present, but no gateway summary or feature availability is available yet."
          : "System Settings is showing real routing/gateway state plus grouped feature availability.",
    [
      `interactive reply = ${(state.deliveryPolicy?.interactiveReply || "source_bound").replaceAll("_", "-")}`,
      `proactive delivery = ${(state.deliveryPolicy?.proactiveDelivery || "member_default").replaceAll("_", " ")}`,
      `feature rows = ${state.featureAvailabilityGroups.flatMap((group) => group.items || []).length}`,
    ]
  );

  const routingLines = [];
  routingLines.push(
    `interactive_reply=${(state.deliveryPolicy?.interactiveReply || "source_bound").replaceAll("_", "-")} · proactive_delivery=${(state.deliveryPolicy?.proactiveDelivery || "member_default").replaceAll("_", " ")}`
  );
  if (gateway) {
    routingLines.push(
      `binding=${toMaybeLabel(gateway.bindingChannel, "unknown")} · status=${toMaybeLabel(
        gateway.bindingStatus,
        "unknown"
      )} · metric=${toMaybeLabel(gateway.bindingMetric, "unknown")} · bound_user=${toMaybeLabel(
        gateway.bindingBoundUser,
        "unknown"
      )}`
    );
    routingLines.push(
      `bridge_provider=${toMaybeLabel(gateway.bridgeProvider.platform, "unknown")} · configured=${yesNo(
        gateway.bridgeProvider.configured
      )} · connected=${yesNo(gateway.bridgeProvider.connected)} · gateway_base_url=${toMaybeLabel(
        gateway.bridgeProvider.gatewayBaseUrl,
        "not exposed"
      )}`
    );
    if (gateway.feishu) {
      routingLines.push(
        `feishu · reply=${yesNo(gateway.feishu.capabilities?.reply ?? gateway.feishu.reply)} · delivery=${yesNo(
          gateway.feishu.capabilities?.delivery ?? gateway.feishu.delivery
        )} · status=${toMaybeLabel(gateway.feishu.transport_status || gateway.feishu.status, "unknown")}`
      );
    }
    if (gateway.weixin) {
      routingLines.push(
        `weixin · parity=${toMaybeLabel(
          gateway.weixin.parity_state || gateway.weixin.parity || gateway.weixin.mode,
          "unknown"
        )} · blocker=${toMaybeLabel(gateway.weixin.blocker_category || gateway.weixin.blocker, "none")}`
      );
    }
    const queueLines = normalizeSignalList(gateway.sourceBoundQueue);
    const failureLines = normalizeSignalList(gateway.sourceBoundFailures);
    if (queueLines.length) {
      routingLines.push(...queueLines.map((line) => `queue · ${line}`));
    }
    if (failureLines.length) {
      routingLines.push(...failureLines.map((line) => `failure · ${line}`));
    }
  }

  const blockerLines = [];
  if (!gateway || gateway.source !== "harborgate_status") {
    blockerLines.push("IM Gateway 仍未暴露同源状态，Feishu / Weixin routing 只能显示明确 blocker。");
  }
  if (!state.accountManagement) {
    blockerLines.push("Account Management 还没有返回 gateway / provisioning 投影，binding availability 不能确认。");
  }
  if (state.featureAvailabilityError) {
    blockerLines.push(`Feature availability 暂不可用：${state.featureAvailabilityError}`);
  }
  if (!routingLines.length) {
    blockerLines.push("当前没有可用的 routing/gateway status。");
  }
  renderFeatureList(els.systemRoutingList, routingLines, "当前没有 real routing/gateway status。");
  renderFeatureList(els.systemBlockerList, blockerLines, "当前没有 explicit blockers。");
  renderFeatureAvailabilityGroups();
}

function renderAll() {
  renderMetrics();
  renderBinding();
  renderOverviewSignals();
  renderGatewayStatus();
  renderAccountManagement();
  renderScanResults();
  renderAccessMembers();
  renderAccessMemberSignals();
  renderApprovals();
  renderTaskSignals();
  renderTaskOutcome();
  renderModelsPolicies();
  renderCameraTabs();
  renderDevicePanel();
  renderAiotSummary();
  renderHarborOsSummary();
  renderSystemSettings();
  renderEvents();
}

async function withBusy(button, pendingLabel, work) {
  const original = button.textContent;
  button.disabled = true;
  button.textContent = pendingLabel;
  try {
    await work();
  } finally {
    button.disabled = false;
    button.textContent = original;
  }
}

async function handleApprovalDecision(approval, action, button) {
  const pendingLabel = action === "approve" ? "批准中..." : "拒绝中...";
  const endpoint = `/tasks/approvals/${encodeURIComponent(approval.approvalId)}/${action}`;
  try {
    await withBusy(button, pendingLabel, async () => {
      const payload = await api(endpoint, {
        method: "POST",
        body: JSON.stringify({}),
      });

      try {
        const nextState = await api("/state");
        applyServerState(nextState);
      } catch (_error) {
        // Keep the decision flow usable even if a follow-up state refresh fails.
      }

      try {
        await loadPendingApprovals({ silent: true });
      } catch (_error) {
        // Approval decision has already succeeded; keep the UI usable and show the latest known queue state.
      }

      const actionLabel = toApprovalActionLabel(approval);
      if (action === "approve") {
        const taskResponse = payload.task_response || null;
        state.latestTaskOutcome = buildOutcomeFromTaskResponse(taskResponse, actionLabel);
        state.lastCommand = `批准 ${actionLabel}`;
        renderAll();
        pushEvent({
          type:
            String(taskResponse?.status || "").toLowerCase() === "failed"
              ? "warning"
              : "normal",
          title: `审批已通过：${actionLabel}`,
          body:
            taskResponse?.result?.message ||
            taskResponse?.prompt ||
            "任务已经越过审批闸口，并继续沿当前 Task API 主链执行。",
          time: "刚刚",
        });
        appendTaskEventsToFeed(state.latestTaskOutcome.events);
        showToast(`已批准 ${actionLabel}。`);
        return;
      }

      state.latestTaskOutcome = buildOutcomeFromRejectedApproval(actionLabel, approval);
      state.lastCommand = `拒绝 ${actionLabel}`;
      renderAll();
      pushEvent({
        type: "warning",
        title: `审批已拒绝：${actionLabel}`,
        body: "任务已经结束，不会继续落到后续执行步骤。",
        time: "刚刚",
      });
      appendTaskEventsToFeed(state.latestTaskOutcome.events);
      showToast(`已拒绝 ${actionLabel}。`);
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: action === "approve" ? "批准审批失败" : "拒绝审批失败",
      body: error.message,
      time: "刚刚",
    });
    renderAll();
    showToast(error.message);
  }
}

async function handleMemberRoleSave(member, select, button) {
  const nextRoleKind = String(select.value || "").trim().toLowerCase();
  if (!nextRoleKind || nextRoleKind === member.roleKind) {
    showToast("角色没有变化。");
    return;
  }

  try {
    await withBusy(button, "保存中...", async () => {
      const payload = await api(`/access/members/${encodeURIComponent(member.userId)}/role`, {
        method: "POST",
        body: JSON.stringify({ role_kind: nextRoleKind }),
      });
      state.accessMembers = Array.isArray(payload) ? payload.map(mapAccessMember) : [];
      state.accessMembersLoaded = true;
      state.accessMembersError = "";
      try {
        await loadAccountManagement({ silent: true });
      } catch (_error) {
        // Keep HarborDesk usable even if the summary read model lags behind the write.
      }
      state.lastCommand = `调整 ${member.displayName} 的访问角色`;
      renderAll();
      pushEvent({
        type: "normal",
        title: `成员角色已更新：${member.displayName}`,
        body: `${member.displayName} 现在是 ${toMemberRoleLabel(nextRoleKind)}。这次变更已经直接写入平台 membership 记录。`,
        time: "刚刚",
      });
      showToast(`已更新 ${member.displayName} 的角色。`);
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: `成员角色更新失败：${member.displayName}`,
      body: error.message,
      time: "刚刚",
    });
    renderAll();
    showToast(error.message);
  }
}

async function handleMemberDefaultDeliverySave(member, select, button) {
  const nextSurface = String(select.value || "").trim().toLowerCase();
  if (!nextSurface || nextSurface === member.proactiveDeliverySurface) {
    showToast("默认主动投递面没有变化。");
    return;
  }

  try {
    await withBusy(button, "保存中...", async () => {
      const payload = await api(
        `/access/members/${encodeURIComponent(member.userId)}/default-delivery-surface`,
        {
          method: "POST",
          body: JSON.stringify({ surface: nextSurface }),
        }
      );
      state.accessMembers = Array.isArray(payload) ? payload.map(mapAccessMember) : [];
      state.accessMembersLoaded = true;
      state.accessMembersError = "";
      try {
        await loadAccountManagement({ silent: true });
      } catch (_error) {
        // The write already succeeded; keep the dashboard responsive.
      }
      state.lastCommand = `调整 ${member.displayName} 的默认主动投递面`;
      renderAll();
      pushEvent({
        type: "normal",
        title: `默认主动投递面已更新：${member.displayName}`,
        body: `${member.displayName} 的系统主动推送默认面已切到 ${nextSurface}；互动链回复仍然保持 source-bound。`,
        time: "刚刚",
      });
      showToast(`已更新 ${member.displayName} 的默认主动投递面。`);
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: `默认主动投递面更新失败：${member.displayName}`,
      body: error.message,
      time: "刚刚",
    });
    renderAll();
    showToast(error.message);
  }
}

function collectModelEndpointDraft(item, endpoint) {
  const read = (field) => item.querySelector(`[data-field="${field}"]`);
  const capabilityTags = parseFallbackOrder(read("capability_tags")?.value || "");
  return {
    model_endpoint_id: endpoint.modelEndpointId,
    model_kind: String(read("model_kind")?.value || endpoint.modelKind || "").trim().toLowerCase(),
    endpoint_kind: String(read("endpoint_kind")?.value || endpoint.endpointKind || "").trim().toLowerCase(),
    provider_key: String(read("provider_key")?.value || endpoint.providerKey || "").trim(),
    model_name: String(read("model_name")?.value || endpoint.modelName || "").trim(),
    provider_account_id: String(read("provider_account_id")?.value || "").trim(),
    capability_tags: capabilityTags,
    status: String(read("status")?.value || endpoint.status || "").trim().toLowerCase(),
  };
}

async function handleModelEndpointSave(endpoint, item, button) {
  try {
    await withBusy(button, "保存中...", async () => {
      const payload = collectModelEndpointDraft(item, endpoint);
      const response = await api(`/models/endpoints/${encodeURIComponent(endpoint.modelEndpointId)}`, {
        method: "PATCH",
        body: JSON.stringify(payload),
      });
      state.modelEndpoints = Array.isArray(response?.endpoints)
        ? response.endpoints.map(mapModelEndpoint).filter(Boolean)
        : state.modelEndpoints;
      state.modelEndpointsLoaded = true;
      state.modelEndpointsError = "";
      try {
        await loadModelCenter({ silent: true });
      } catch (_error) {
        // Keep the release shell usable even if the follow-up readback lags the write.
      }
      state.lastCommand = `保存模型端点 ${endpoint.modelEndpointId}`;
      renderAll();
      pushEvent({
        type: "normal",
        title: `模型端点已保存：${endpoint.modelEndpointId}`,
        body: `Endpoint 的 status/kind/provider/route posture 已保存，并刷新了 HarborDesk 的模型中心视图。`,
        time: "刚刚",
      });
      showToast(`已保存 ${endpoint.modelEndpointId}。`);
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: `模型端点保存失败：${endpoint.modelEndpointId}`,
      body: error.message,
      time: "刚刚",
    });
    renderAll();
    showToast(error.message);
  }
}

async function handleModelEndpointTest(endpoint, item, button) {
  try {
    await withBusy(button, "测试中...", async () => {
      const result = await api(`/models/endpoints/${encodeURIComponent(endpoint.modelEndpointId)}/test`, {
        method: "POST",
        body: JSON.stringify({}),
      });
      state.modelEndpointTestResults = {
        ...state.modelEndpointTestResults,
        [endpoint.modelEndpointId]: result,
      };
      state.lastCommand = `测试模型端点 ${endpoint.modelEndpointId}`;
      renderAll();
      pushEvent({
        type: result?.ok ? "normal" : "warning",
        title: `模型端点测试${result?.ok ? "通过" : "失败"}：${endpoint.modelEndpointId}`,
        body: result?.summary || result?.status || "Endpoint test completed.",
        time: "刚刚",
      });
      showToast(
        result?.ok
          ? `${endpoint.modelEndpointId} 测试通过。`
          : `${endpoint.modelEndpointId} 测试未通过。`
      );
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: `模型端点测试失败：${endpoint.modelEndpointId}`,
      body: error.message,
      time: "刚刚",
    });
    renderAll();
    showToast(error.message);
  }
}

function collectModelRoutePolicies() {
  const items = Array.from(document.querySelectorAll('[data-route-policy-id]'));
  return items.map((item) => {
    const read = (field) => item.querySelector(`[data-field="${field}"]`);
    const routePolicyId = item.dataset.routePolicyId || "";
    return {
      route_policy_id: routePolicyId,
      workspace_id: state.modelPolicies.find((policy) => policy.routePolicyId === routePolicyId)?.workspaceId || "",
      domain_scope: state.modelPolicies.find((policy) => policy.routePolicyId === routePolicyId)?.domainScope || "",
      modality: state.modelPolicies.find((policy) => policy.routePolicyId === routePolicyId)?.modality || "",
      privacy_level: String(read("privacy_level")?.value || "").trim().toLowerCase(),
      local_preferred: String(read("local_preferred")?.value || "false").trim().toLowerCase() === "true",
      max_cost_per_run: (() => {
        const value = String(read("max_cost_per_run")?.value || "").trim();
        if (!value) {
          return null;
        }
        const parsed = Number(value);
        return Number.isFinite(parsed) ? parsed : null;
      })(),
      fallback_order: parseFallbackOrder(read("fallback_order")?.value || ""),
      status: String(read("status")?.value || "").trim().toLowerCase(),
      metadata: state.modelPolicies.find((policy) => policy.routePolicyId === routePolicyId)?.metadata || {},
    };
  });
}

async function handleModelPoliciesSave(button) {
  try {
    await withBusy(button, "保存中...", async () => {
      const route_policies = collectModelRoutePolicies();
      const response = await api("/models/policies", {
        method: "PUT",
        body: JSON.stringify({ route_policies }),
      });
      state.modelPolicies = Array.isArray(response?.route_policies)
        ? response.route_policies.map(mapModelPolicy).filter(Boolean)
        : state.modelPolicies;
      state.modelPoliciesLoaded = true;
      state.modelPoliciesError = "";
      try {
        await loadModelCenter({ silent: true });
      } catch (_error) {
        // The route-policy save already landed; keep the editor responsive.
      }
      state.lastCommand = "保存模型路由策略";
      renderAll();
      pushEvent({
        type: "normal",
        title: "Route policies 已保存",
        body: "默认路由与 fallback order 已写回后端，HarborDesk 会刷新模型中心读面。",
        time: "刚刚",
      });
      showToast("已保存 route policies。");
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: "Route policies 保存失败",
      body: error.message,
      time: "刚刚",
    });
    renderAll();
    showToast(error.message);
  }
}

async function handleShareLinkRevoke(link, button) {
  try {
    await withBusy(button, "撤销中...", async () => {
      await api(`/share-links/${encodeURIComponent(link.shareLinkId)}/revoke`, {
        method: "POST",
        body: JSON.stringify({}),
      });
      try {
        await loadShareLinks({ silent: true });
      } catch (_error) {
        // Revocation already succeeded; keep the dashboard usable with the last known list.
      }
      state.lastCommand = `撤销共享链接 ${link.shareLinkId}`;
      renderAll();
      pushEvent({
        type: "warning",
        title: `共享链接已撤销：${link.deviceName}`,
        body: `后台已经关闭 ${link.shareLinkId} 对应的共享会话，旧的 shared 页面会立即失效。`,
        time: "刚刚",
      });
      showToast(`已撤销 ${link.shareLinkId}。`);
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: `撤销共享链接失败：${link.deviceName}`,
      body: error.message,
      time: "刚刚",
    });
    renderAll();
    showToast(error.message);
  }
}

document.querySelector("#refresh-qr").addEventListener("click", (event) => {
  withBusy(event.currentTarget, "刷新中...", async () => {
    const payload = await api("/binding/refresh", { method: "POST" });
    applyServerState(payload);
    state.lastCommand = "刷新绑定二维码";
    renderAll();
    pushEvent({
      type: "info",
      title: "绑定二维码已刷新",
      body: "这个动作已经通过本地管理 API 落到真实状态文件，后续接外部 IM 扫码流程时可以沿用同一个绑定对象。",
      time: "刚刚",
    });
    showToast("已刷新绑定二维码。");
  }).catch((error) => {
    showToast(error.message);
  });
});

els.refreshAccessMembers.addEventListener("click", (event) => {
  withBusy(event.currentTarget, "刷新中...", async () => {
    await loadAccessMembers();
    state.lastCommand = "刷新成员角色列表";
    renderAll();
    pushEvent({
      type: "info",
      title: "成员角色列表已刷新",
      body: `当前 workspace 内有 ${state.accessMembers.length} 条成员记录。`,
      time: "刚刚",
    });
    showToast("已刷新成员角色列表。");
  }).catch((error) => {
    showToast(error.message);
  });
});

els.refreshShareLinks.addEventListener("click", (event) => {
  withBusy(event.currentTarget, "刷新中...", async () => {
    await loadShareLinks();
    state.lastCommand = "刷新共享链接列表";
    renderAll();
    pushEvent({
      type: "info",
      title: "共享链接列表已刷新",
      body: `当前平台里登记了 ${state.shareLinks.length} 条共享链路记录。`,
      time: "刚刚",
    });
    showToast("已刷新共享链接列表。");
  }).catch((error) => {
    showToast(error.message);
  });
});

if (els.refreshModelCenter) {
  els.refreshModelCenter.addEventListener("click", (event) => {
    withBusy(event.currentTarget, "刷新中...", async () => {
      await loadModelCenter();
      state.lastCommand = "刷新模型中心";
      renderAll();
      pushEvent({
        type: "normal",
        title: "Model Center 已刷新",
        body: `当前有 ${state.modelEndpoints.length} 个模型端点和 ${state.modelPolicies.length} 条路由策略。`,
        time: "刚刚",
      });
      showToast("已刷新模型中心。");
    }).catch((error) => {
      showToast(error.message);
    });
  });
}

if (els.saveRoutePolicies) {
  els.saveRoutePolicies.addEventListener("click", (event) => {
    handleModelPoliciesSave(event.currentTarget).catch((error) => {
      showToast(error.message);
    });
  });
}

els.refreshApprovals.addEventListener("click", (event) => {
  withBusy(event.currentTarget, "刷新中...", async () => {
    await loadPendingApprovals();
    state.lastCommand = "刷新审批队列";
    renderAll();
    pushEvent({
      type: "info",
      title: "审批队列已刷新",
      body: `当前还有 ${state.approvals.length} 个待审批动作。`,
      time: "刚刚",
    });
    showToast("已刷新审批队列。");
  }).catch((error) => {
    showToast(error.message);
  });
});

document.querySelector("#simulate-bind").addEventListener("click", (event) => {
  withBusy(event.currentTarget, "打开中...", async () => {
    window.open(state.binding.staticQrToken, "_blank", "noopener,noreferrer");
    state.lastCommand = "打开手机配置页";
    renderMetrics();
    pushEvent({
      type: "info",
      title: "手机配置页已打开",
      body: "这个动作会把手机浏览器带到本地后台设置页，真实接入动作是填写 bridge provider 的 app_id 和 app_secret，而不是发送绑定码。",
      time: "刚刚",
    });
    showToast("已打开手机配置页。");
  }).catch((error) => {
    showToast(error.message);
  });
});

els.bindTestForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const submitButton = event.currentTarget.querySelector('button[type="submit"]');
  withBusy(submitButton, "验证中...", async () => {
    const form = new FormData(event.currentTarget);
    const payload = await api("/bridge/configure", {
      method: "POST",
      body: JSON.stringify({
        app_id: String(form.get("app_id") || "").trim(),
        app_secret: String(form.get("app_secret") || "").trim(),
      }),
    });
    applyServerState(payload);
    state.lastCommand = "保存 Bridge Provider 配置";
    renderAll();
    pushEvent({
      type: "normal",
      title: "Bridge Provider 已验证成功",
      body: `后台已经保存并验证 ${payload.bridge_provider?.app_name || "这个桥接应用"} 的凭证，现在可以启动真实消息桥接链路。`,
      time: "刚刚",
    });
    showToast("Bridge Provider 已保存。");
  }).catch((error) => {
    pushEvent({
      type: "warning",
      title: "Bridge Provider 配置失败",
      body: error.message,
      time: "刚刚",
    });
    renderEvents();
    showToast(error.message);
  });
});

document.querySelector("#scan-button").addEventListener("click", (event) => {
  withBusy(event.currentTarget, "扫描中...", async () => {
    const cidr = els.scanCidr.value.trim() || state.defaults.cidr;
    const protocol = els.scanProtocol.value;
    const payload = await api("/discovery/scan", {
      method: "POST",
      body: JSON.stringify({ cidr, protocol }),
    });
    applyServerState(payload);
    state.scanResults = Array.isArray(payload.results) ? payload.results : [];
    state.lastCommand = "扫描摄像头";
    renderAll();
    pushEvent({
      type: "normal",
      title: "局域网扫描已执行",
      body: `后台这次真实探测了 ${payload.scanned_hosts || 0} 个候选主机，并把可用摄像头回写到了设备库。`,
      time: "刚刚",
    });
    showToast("已执行真实扫描。");
  }).catch((error) => {
    pushEvent({
      type: "warning",
      title: "扫描未完成",
      body: error.message,
      time: "刚刚",
    });
    renderEvents();
    showToast(error.message);
  });
});

document.querySelector("#sync-im-guide").addEventListener("click", () => {
  state.lastCommand = "同步 IM 引导";
  renderMetrics();
  pushEvent({
    type: "info",
    title: "IM 引导菜单待接入",
    body: "这一步暂时还是后台演示动作。下一步接二维码绑定时，会把默认策略和欢迎语串起来。",
    time: "刚刚",
  });
  showToast("已记录这次 IM 引导同步动作。");
});

els.manualForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const submitButton = event.currentTarget.querySelector('button[type="submit"]');
  withBusy(submitButton, "验证中...", async () => {
    const form = new FormData(event.currentTarget);
    const payload = await api("/devices/manual", {
      method: "POST",
      body: JSON.stringify({
        name: String(form.get("name") || "").trim(),
        room: String(form.get("room") || "").trim(),
        ip: String(form.get("ip") || "").trim(),
        path: String(form.get("path") || "").trim(),
        snapshot_url: String(form.get("snapshot_url") || "").trim(),
        username: String(form.get("username") || "").trim(),
        password: String(form.get("password") || "").trim(),
      }),
    });
    applyServerState(payload);
    state.activeCameraId = payload.device?.device_id || state.activeCameraId;
    state.lastCommand = `手动添加 ${payload.device?.name || "摄像头"}`;
    renderAll();
    pushEvent({
      type: "normal",
      title: `手动添加成功：${payload.device?.name || "摄像头"}`,
      body: payload.note || "设备已通过 RTSP 验证并写入设备库。",
      time: "刚刚",
    });
    showToast(`已写入 ${payload.device?.name || "摄像头"}。`);
    event.currentTarget.reset();
  }).catch((error) => {
    pushEvent({
      type: "warning",
      title: "手动添加失败",
      body: error.message,
      time: "刚刚",
    });
    renderEvents();
    showToast(error.message);
  });
});

document.querySelector("#save-policies").addEventListener("click", (event) => {
  withBusy(event.currentTarget, "保存中...", async () => {
    const payload = await api("/defaults", {
      method: "POST",
      body: JSON.stringify({
        cidr: els.policyCidr.value.trim() || state.defaults.cidr,
        discovery: els.policyDiscovery.value,
        recording: els.policyRecording.value,
        capture: els.policyCapture.value,
        ai: els.policyAi.value,
        notification_channel:
          els.policyNotificationChannel.value.trim() || state.defaults.notificationChannel,
        rtsp_username: els.policyRtspUsername.value.trim() || "admin",
        rtsp_password: els.policyRtspPassword.value,
        rtsp_paths: els.policyRtspPaths.value
          .split(",")
          .map((item) => item.trim())
          .filter(Boolean),
      }),
    });
    applyServerState(payload);
    state.lastCommand = "应用默认策略";
    renderAll();
    pushEvent({
      type: "normal",
      title: "默认策略已保存",
      body: "扫描网段、RTSP 凭证、录像策略和默认通知通道都已经落到本地配置文件里。",
      time: "刚刚",
    });
    showToast("已保存默认策略。");
  }).catch((error) => {
    showToast(error.message);
  });
});

document.querySelector("#test-im-command").addEventListener("click", () => {
  state.lastCommand = "看看客厅摄像头";
  renderMetrics();
  pushEvent({
    type: "info",
      title: "模拟 IM 命令：看看客厅摄像头",
      body: "当前页面已经接了真实设备库；下一步需要把这条 IM 命令正式路由到绑定关系和默认策略。",
    time: "刚刚",
  });
  showToast("已模拟一条 IM 命令流。");
});

document.querySelector("#snapshot-button").addEventListener("click", async (event) => {
  const camera = getActiveCamera();
  if (!camera) {
    showToast("还没有可验证的摄像头。");
    return;
  }

  try {
    await withBusy(event.currentTarget, "抓拍中...", async () => {
      const payload = await api(`/cameras/${encodeURIComponent(camera.id)}/snapshot`, {
        method: "POST",
      });
      const taskResponse = payload.task_response || null;
      state.latestTaskOutcome = buildOutcomeFromTaskResponse(
        taskResponse,
        `后台抓拍 ${camera.name}`
      );
      state.lastCommand = `拍一张${camera.room}`;
      renderAll();
      refreshPreviewFrame();
      pushEvent({
        type:
          String(taskResponse?.status || "").toLowerCase() === "failed"
            ? "warning"
            : "normal",
        title: `后台抓拍已执行：${camera.name}`,
        body:
          taskResponse?.result?.message ||
          "抓拍请求已经通过统一 Task API 执行，当前产物会落到任务结果与 artifact 记录里。",
        time: "刚刚",
      });
      appendTaskEventsToFeed(state.latestTaskOutcome.events);
      showToast(
        String(taskResponse?.status || "").toLowerCase() === "failed"
          ? `${camera.name} 抓拍失败。`
          : `已完成 ${camera.name} 的后台抓拍。`
      );
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: `后台抓拍失败：${camera.name}`,
      body: error.message,
      time: "刚刚",
    });
    showToast(error.message);
  }
});

document.querySelector("#analyze-button").addEventListener("click", async (event) => {
  const camera = getActiveCamera();
  if (!camera) {
    showToast("还没有可分析的摄像头。");
    return;
  }

  try {
    await withBusy(event.currentTarget, "分析中...", async () => {
      const payload = await api(`/cameras/${encodeURIComponent(camera.id)}/analyze`, {
        method: "POST",
      });
      const taskResponse = payload.task_response || null;
      applyAnalyzeOutcomeToCamera(camera, taskResponse);
      state.latestTaskOutcome = buildOutcomeFromTaskResponse(
        taskResponse,
        `后台分析 ${camera.name}`
      );
      state.lastCommand = `分析${camera.room}摄像头`;
      renderAll();
      pushEvent({
        type:
          String(taskResponse?.status || "").toLowerCase() === "failed"
            ? "warning"
            : "normal",
        title: `后台分析已执行：${camera.name}`,
        body:
          taskResponse?.result?.message ||
          "分析请求已经通过统一 Task API 执行，结果、产物和通知状态会在当前页面持续展示。",
        time: "刚刚",
      });
      appendTaskEventsToFeed(state.latestTaskOutcome.events);
      showToast(
        String(taskResponse?.status || "").toLowerCase() === "failed"
          ? `${camera.name} 分析失败。`
          : `已完成 ${camera.name} 的后台分析。`
      );
    });
  } catch (error) {
    pushEvent({
      type: "warning",
      title: `后台分析失败：${camera.name}`,
      body: error.message,
      time: "刚刚",
    });
    showToast(error.message);
  }
});

document.querySelector("#record-button").addEventListener("click", () => {
  const camera = getActiveCamera();
  if (!camera) {
    showToast("还没有可录制的摄像头。");
    return;
  }
  state.recordingEnabled = !state.recordingEnabled;
  camera.recordingMode = state.recordingEnabled ? "手动录像中" : state.defaults.recording;
  renderDevicePanel();
  pushEvent({
    type: "info",
    title: `录像策略切换：${camera.name}`,
    body: state.recordingEnabled
      ? "已切到临时手动录像。后续建议也暴露成统一 IM 命令。"
      : "已恢复后台默认录像策略。",
    time: "刚刚",
  });
  showToast(state.recordingEnabled ? `已开始录制 ${camera.name}` : "已恢复默认录像策略");
});

document.querySelector("#share-link-button").addEventListener("click", async (event) => {
  const camera = getActiveCamera();
  if (!camera) {
    showToast("还没有可共享的摄像头。");
    return;
  }

  let previewWindow = null;
  try {
    previewWindow = window.open("about:blank", "_blank", "noopener,noreferrer");
  } catch (_error) {
    previewWindow = null;
  }

  try {
    await withBusy(event.currentTarget, "生成中...", async () => {
      const payload = await api(`/cameras/${encodeURIComponent(camera.id)}/share-link`, {
        method: "POST",
      });
      const taskResponse = payload.task_response || null;
      const shareUrl = extractShareLinkUrl(taskResponse);
      try {
        await loadShareLinks({ silent: true });
      } catch (_error) {
        // Keep the immediate share flow usable even if the follow-up list refresh fails.
      }
      state.latestTaskOutcome = buildOutcomeFromTaskResponse(
        taskResponse,
        `生成共享链接 ${camera.name}`
      );
      state.lastCommand = `共享${camera.room}摄像头`;
      renderAll();

      if (shareUrl) {
        if (previewWindow && !previewWindow.closed) {
          previewWindow.location = shareUrl;
        }
        pushEvent({
          type: "normal",
          title: `共享链接已生成：${camera.name}`,
          body: `可直接打开共享观看页：${shareUrl}`,
          time: "刚刚",
        });
        showToast(`已为 ${camera.name} 生成共享链接。`);
      } else {
        if (previewWindow && !previewWindow.closed) {
          previewWindow.close();
        }
        pushEvent({
          type: "warning",
          title: `共享链接已生成：${camera.name}`,
          body: "任务已完成，但这次返回里没有可直接打开的共享 URL。",
          time: "刚刚",
        });
        showToast(`已为 ${camera.name} 生成共享链接。`);
      }

      appendTaskEventsToFeed(state.latestTaskOutcome.events);
    });
  } catch (error) {
    if (previewWindow && !previewWindow.closed) {
      previewWindow.close();
    }
    pushEvent({
      type: "warning",
      title: `生成共享链接失败：${camera.name}`,
      body: error.message,
      time: "刚刚",
    });
    showToast(error.message);
  }
});

document.querySelector("#clear-events").addEventListener("click", () => {
  state.events = [
    {
      type: "info",
      title: "事件流已清空",
      body: "后续这里建议保留绑定、扫描、设备接入、命令调用和 AI 结果等关键审计事件。",
      time: "刚刚",
    },
  ];
  renderEvents();
  showToast("已清空演示事件。");
});

els.livePreviewFrame.addEventListener("error", () => {
  els.activeLiveStatus.textContent = "预览刷新失败，等待下一次抓拍";
});

window.addEventListener("beforeunload", () => {
  stopPreviewLoop();
});

navButtons.forEach((button) => {
  button.addEventListener("click", () => {
    setActiveView(button.dataset.view);
  });
});

window.addEventListener("hashchange", () => {
  setActiveView(window.location.hash.slice(1), { updateHash: false });
});

async function boot() {
  setActiveView(window.location.hash.slice(1), { updateHash: false });
  renderAll();
  try {
    const payload = await api("/state");
    applyServerState(payload);
    try {
      await loadAccountManagement({ silent: true });
    } catch (_error) {
      // Keep the shell usable even if account-management detail view is temporarily unavailable.
    }
    try {
      await loadGatewayStatus({ silent: true });
    } catch (_error) {
      // Keep the shell usable even if same-origin HarborGate status is temporarily unavailable.
    }
    try {
      await loadPendingApprovals({ silent: true });
    } catch (_error) {
      // Keep the dashboard usable even if the approval queue is temporarily unavailable.
    }
    try {
      await loadAccessMembers({ silent: true });
    } catch (_error) {
      // Keep the dashboard usable even if the member list is temporarily unavailable.
    }
    try {
      await loadShareLinks({ silent: true });
    } catch (_error) {
      // Keep the dashboard usable even if the share-link list is temporarily unavailable.
    }
    try {
      await loadModelCenter({ silent: true });
    } catch (_error) {
      // Keep HarborDesk usable even if the model center is temporarily unavailable.
    }
    state.lastCommand = state.cameras.length ? "已载入设备库" : "等待首次接入";
    renderAll();
    pushEvent({
      type: "normal",
      title: "本地管理 API 已连接",
      body:
        `已经读取到 ${state.cameras.length} 台真实设备，当前成员 ${state.accessMembers.length} 项，已登记共享链路 ${state.shareLinks.length} 条，默认策略来自本地管理状态。` +
        (state.accountManagement ? `Workspace ${state.accountManagement.workspace?.display_name || "unknown"} 已接入。` : "账号治理详细视图暂不可用。") +
        (state.gatewayStatus ? "IM Gateway 状态已同步。" : "IM Gateway 状态详细读面暂不可用。") +
        (state.approvalsError ? "审批队列当前暂不可用。" : `当前待审批 ${state.approvals.length} 项。`) +
        (state.shareLinksError ? "共享链接列表当前暂不可用。" : "") +
        (state.accessMembersError ? "成员角色列表当前暂不可用。" : ""),
      time: "刚刚",
    });
    if (state.approvals.length) {
      pushEvent({
        type: "warning",
        title: `有 ${state.approvals.length} 个高风险动作等待审批`,
        body: "你可以直接在这个后台页批准或拒绝，不需要再手工查 approval token。",
        time: "刚刚",
      });
    }
  } catch (error) {
    pushEvent({
      type: "warning",
      title: "本地管理 API 未连接",
      body: `请先启动 HarborDesk 管理 API。当前尝试连接：${API_BASE}。错误：${error.message}`,
      time: "刚刚",
    });
    renderEvents();
    showToast("未连接到本地管理 API。");
  }
}

boot();
