const apiHost = window.location.hostname || "127.0.0.1";
const apiProtocol = window.location.protocol === "file:" ? "http:" : window.location.protocol;
const API_BASE = `${apiProtocol}//${apiHost}:4174/api`;

const state = {
  binding: {
    status: "等待扫码",
    metric: "等待绑定",
    boundUser: "未配置",
    channel: "飞书 HarborNAS Bot",
    qrToken: "http://127.0.0.1:4174/setup/mobile?session=PENDING",
    staticQrToken: "http://harbornas.local:4174/setup/mobile",
  },
  feishuBot: {
    configured: false,
    appId: "",
    appSecret: "",
    appName: "",
    botOpenId: "",
    status: "未配置",
  },
  defaults: {
    cidr: "192.168.3.0/24",
    discovery: "RTSP Probe",
    recording: "按事件录制",
    capture: "图片 + 摘要",
    ai: "人体检测 + 中文摘要",
    feishuGroup: "客厅安全群",
    rtspUsername: "admin",
    rtspPassword: "",
    rtspPaths: ["/ch1/main", "/h264/ch1/main/av_stream", "/Streaming/Channels/101"],
  },
  lastCommand: "等待后台动作",
  recordingEnabled: false,
  cameras: [],
  activeCameraId: null,
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
  detailFeishu: document.querySelector("#detail-feishu"),
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
  policyFeishuGroup: document.querySelector("#policy-feishu-group"),
  policyRtspUsername: document.querySelector("#policy-rtsp-username"),
  policyRtspPassword: document.querySelector("#policy-rtsp-password"),
  policyRtspPaths: document.querySelector("#policy-rtsp-paths"),
  manualForm: document.querySelector("#manual-form"),
  bindTestForm: document.querySelector("#bind-test-form"),
  bindTestDisplayName: document.querySelector("#bind-test-display-name"),
  bindTestOpenId: document.querySelector("#bind-test-open-id"),
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
    channel: binding?.channel || "飞书 HarborNAS Bot",
    qrToken: binding?.setup_url || binding?.qr_token || "http://127.0.0.1:4174/setup/mobile?session=PENDING",
    staticQrToken: binding?.static_setup_url || binding?.setup_url || "http://harbornas.local:4174/setup/mobile",
  };
}

function mapFeishuBot(config) {
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
    feishuGroup: defaults?.feishu_group || "HarborNAS Bot",
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
    feishu: `${state.defaults.feishuGroup} / ${state.binding.channel}`,
    signal: toSignalLabel(device),
    hint: toHint(device),
    markers: [],
  };
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
  if (payload.feishu_bot) {
    state.feishuBot = mapFeishuBot(payload.feishu_bot);
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
  els.policyFeishuGroup.value = state.defaults.feishuGroup;
  els.policyRtspUsername.value = state.defaults.rtspUsername;
  els.policyRtspPassword.value = state.defaults.rtspPassword;
  els.policyRtspPaths.value = state.defaults.rtspPaths.join(", ");
  els.bindTestDisplayName.value = state.feishuBot.appId;
  els.bindTestOpenId.value = state.feishuBot.appSecret;
}

function renderMetrics() {
  els.bindingMetric.textContent = state.binding.metric;
  els.scanMetric.textContent = state.defaults.cidr;
  els.camerasMetric.textContent = String(state.cameras.length);
  els.commandMetric.textContent = state.lastCommand;
}

function renderBinding() {
  els.qrToken.textContent = state.binding.staticQrToken;
  els.qrInstruction.textContent = "这张静态二维码应该贴在 Bot 硬件上。手机扫码后会在浏览器里打开后台配置页，然后填写飞书 Bot 的 app_id 和 app_secret。";
  if (els.qrImage) {
    els.qrImage.src = `${API_BASE}/binding/static-qr.svg?ts=${Date.now()}`;
  }
  els.bindStatus.textContent = state.binding.status;
  els.boundUser.textContent = state.feishuBot.appName || state.binding.boundUser;
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
    els.detailFeishu.textContent = `${state.defaults.feishuGroup} / ${state.binding.channel}`;
    els.deviceBadge.textContent = "待接入";
    els.deviceBadge.style.color = "#cc7420";
    els.deviceBadge.style.background = "rgba(204, 116, 32, 0.14)";
    renderOverlay(null);
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
  els.detailFeishu.textContent = camera.feishu;
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
  syncPreviewLoop();
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

function renderAll() {
  renderMetrics();
  renderBinding();
  renderScanResults();
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

document.querySelector("#refresh-qr").addEventListener("click", (event) => {
  withBusy(event.currentTarget, "刷新中...", async () => {
    const payload = await api("/binding/refresh", { method: "POST" });
    applyServerState(payload);
    state.lastCommand = "刷新绑定二维码";
    renderAll();
    pushEvent({
      type: "info",
      title: "绑定二维码已刷新",
      body: "这个动作已经通过本地管理 API 落到真实状态文件，后续接飞书扫码流程时可以沿用同一个绑定对象。",
      time: "刚刚",
    });
    showToast("已刷新绑定二维码。");
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
      body: "这个动作会把手机浏览器带到本地后台设置页，真实接入动作是填写飞书 Bot 的 app_id 和 app_secret，而不是发送绑定码。",
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
    const payload = await api("/feishu/configure", {
      method: "POST",
      body: JSON.stringify({
        app_id: String(form.get("app_id") || "").trim(),
        app_secret: String(form.get("app_secret") || "").trim(),
      }),
    });
    applyServerState(payload);
    state.lastCommand = "保存飞书 Bot 配置";
    renderAll();
    pushEvent({
      type: "normal",
      title: "飞书 Bot 已验证成功",
      body: `后台已经保存并验证 ${payload.feishu_bot?.app_name || "这个飞书应用"} 的凭证，现在可以启动真实飞书消息链路。`,
      time: "刚刚",
    });
    showToast("飞书 Bot 已保存。");
  }).catch((error) => {
    pushEvent({
      type: "warning",
      title: "飞书 Bot 配置失败",
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
  state.lastCommand = "同步飞书引导";
  renderMetrics();
  pushEvent({
    type: "info",
    title: "飞书引导菜单待接入",
    body: "这一步暂时还是后台演示动作。下一步接二维码绑定时，会把默认策略和欢迎语串起来。",
    time: "刚刚",
  });
  showToast("已记录这次飞书引导同步动作。");
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
        feishu_group: els.policyFeishuGroup.value.trim() || state.defaults.feishuGroup,
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
      body: "扫描网段、RTSP 凭证、录像策略和默认飞书去向都已经落到本地配置文件里。",
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
    title: "模拟飞书命令：看看客厅摄像头",
    body: "当前页面已经接了真实设备库；下一步需要把这条 IM 命令正式路由到绑定关系和默认策略。",
    time: "刚刚",
  });
  showToast("已模拟一条飞书命令流。");
});

document.querySelector("#snapshot-button").addEventListener("click", () => {
  const camera = getActiveCamera();
  if (!camera) {
    showToast("还没有可验证的摄像头。");
    return;
  }
  state.lastCommand = `拍一张${camera.room}`;
  renderMetrics();
  pushEvent({
    type: "normal",
    title: `后台抓拍验证：${camera.name}`,
    body: "抓拍按钮还保留在后台作为运维动作，但主链路仍然建议通过飞书触发。",
    time: "刚刚",
  });
  showToast(`已记录 ${camera.name} 的抓拍验证动作。`);
});

document.querySelector("#analyze-button").addEventListener("click", () => {
  const camera = getActiveCamera();
  if (!camera) {
    showToast("还没有可分析的摄像头。");
    return;
  }
  state.lastCommand = `分析${camera.room}摄像头`;
  renderMetrics();
  pushEvent({
    type: "warning",
    title: `后台触发分析：${camera.name}`,
    body: "这部分能力已经在飞书主链上可用，这里继续保留成后台验证入口。",
    time: "刚刚",
  });
  showToast(`已记录 ${camera.name} 的分析动作。`);
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
      ? "已切到临时手动录像。后续建议也暴露成飞书命令。"
      : "已恢复后台默认录像策略。",
    time: "刚刚",
  });
  showToast(state.recordingEnabled ? `已开始录制 ${camera.name}` : "已恢复默认录像策略");
});

document.querySelector("#send-to-feishu").addEventListener("click", () => {
  const camera = getActiveCamera();
  if (!camera) {
    showToast("还没有可发送的结果。");
    return;
  }
  state.lastCommand = `把${camera.room}结果发到飞书`;
  renderMetrics();
  pushEvent({
    type: "normal",
    title: `已准备发送测试结果到飞书：${camera.name}`,
    body: "这一步仍然是后台演示按钮，之后会接到真实的绑定用户或默认飞书群组。",
    time: "刚刚",
  });
  showToast(`已记录 ${camera.name} 的飞书发送动作。`);
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
    state.lastCommand = state.cameras.length ? "已载入设备库" : "等待首次接入";
    renderAll();
    pushEvent({
      type: "normal",
      title: "本地管理 API 已连接",
      body: `已经读取到 ${state.cameras.length} 台真实设备，默认策略来自 .harbornas 下的本地状态文件。`,
      time: "刚刚",
    });
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
