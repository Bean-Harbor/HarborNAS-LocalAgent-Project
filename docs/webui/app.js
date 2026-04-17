const apiHost = window.location.hostname || "127.0.0.1";
const apiProtocol = window.location.protocol === "file:" ? "http:" : window.location.protocol;
const APP_BASE = `${apiProtocol}//${apiHost}:4174`;
const API_BASE = `${apiProtocol}//${apiHost}:4174/api`;

const state = {
  binding: {
    status: "等待扫码",
    metric: "等待绑定",
    boundUser: "未配置",
    channel: "Harbor IM Bridge",
    qrToken: "http://127.0.0.1:4174/setup/mobile?session=PENDING",
    staticQrToken: "http://harbornas.local:4174/setup/mobile",
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
    rtspPaths: ["/ch1/main", "/h264/ch1/main/av_stream", "/Streaming/Channels/101"],
  },
  lastCommand: "等待后台动作",
  recordingEnabled: false,
  cameras: [],
  activeCameraId: null,
  approvals: [],
  approvalsLoaded: false,
  approvalsError: "",
  accessMembers: [],
  accessMembersLoaded: false,
  accessMembersError: "",
  shareLinks: [],
  shareLinksLoaded: false,
  shareLinksError: "",
  latestTaskOutcome: null,
  scanResults: [],
  events: [
    {
      type: "info",
      title: "正在连接本地管理 API",
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
  taskOutcomeStatus: document.querySelector("#task-outcome-status"),
  taskOutcomeMessage: document.querySelector("#task-outcome-message"),
  taskOutcomeAction: document.querySelector("#task-outcome-action"),
  taskOutcomeAudit: document.querySelector("#task-outcome-audit"),
  taskOutcomeNotification: document.querySelector("#task-outcome-notification"),
  taskOutcomeArtifacts: document.querySelector("#task-outcome-artifacts"),
  taskOutcomeEvents: document.querySelector("#task-outcome-events"),
};

const previewState = {
  timer: null,
  deviceId: null,
};

function getActiveCamera() {
  return state.cameras.find((camera) => camera.id === state.activeCameraId) || state.cameras[0] || null;
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
    staticQrToken: binding?.static_setup_url || binding?.setup_url || "http://harbornas.local:4174/setup/mobile",
  };
}

function mapBridgeProvider(config) {
  return {
    configured: Boolean(config?.configured),
    appId: config?.app_id || "",
    appSecret: config?.app_secret || "",
    appName: config?.app_name || "",
    botOpenId: config?.bot_open_id || "",
    status: config?.status || "未配置",
  };
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
      : ["/ch1/main", "/h264/ch1/main/av_stream", "/Streaming/Channels/101"],
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
  if (payload.bridge_provider) {
    state.bridgeProvider = mapBridgeProvider(payload.bridge_provider);
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
      "批准或拒绝之后，这里会显示任务执行状态、审计引用和通知投递结果。";
    els.taskOutcomeAction.textContent = "等待审批动作";
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

    copy.appendChild(pillRow);

    const meta = document.createElement("span");
    meta.className = "scan-result-meta";
    meta.textContent = [
      member.userId,
      member.openId ? `open_id ${member.openId}` : "",
      member.chatId ? `chat_id ${member.chatId}` : "",
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

    item.appendChild(copy);
    item.appendChild(actions);
    els.accessMemberList.appendChild(item);
  });
}

function renderAll() {
  renderMetrics();
  renderBinding();
  renderScanResults();
  renderAccessMembers();
  renderApprovals();
  renderTaskOutcome();
  renderCameraTabs();
  renderDevicePanel();
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

async function boot() {
  renderAll();
  try {
    const payload = await api("/state");
    applyServerState(payload);
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
    state.lastCommand = state.cameras.length ? "已载入设备库" : "等待首次接入";
    renderAll();
    pushEvent({
      type: "normal",
      title: "本地管理 API 已连接",
      body:
        `已经读取到 ${state.cameras.length} 台真实设备，当前成员 ${state.accessMembers.length} 项，已登记共享链路 ${state.shareLinks.length} 条，默认策略来自 .harbornas 下的本地状态文件。` +
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
      body: `请先启动 agent-hub-admin-api。当前尝试连接：${API_BASE}。错误：${error.message}`,
      time: "刚刚",
    });
    renderEvents();
    showToast("未连接到本地管理 API。");
  }
}

boot();
