# Camera Domain Task Contract

更新时间：2026-04-13

## 1. 目标

本契约定义 Home Agent Hub 作为平台首个垂直域时，需要对平台主干暴露的最小能力集合。

平台侧只关心：

- task request / task result
- action name
- 参数
- 风险等级
- artifact / event 输出

平台侧不应依赖任何摄像头产品面的内部状态实现细节。

---

## 2. Assistant Task API（最小版）

## 2.1 Request

```json
{
  "task_id": "uuid-or-stable-id",
  "trace_id": "trace-id",
  "source": {
    "channel": "feishu",
    "surface": "harborbeacon",
    "conversation_id": "optional",
    "user_id": "optional"
  },
  "intent": {
    "domain": "camera",
    "action": "analyze",
    "raw_text": "分析客厅摄像头"
  },
  "entity_refs": {
    "device_id": "optional",
    "room": "客厅"
  },
  "args": {
    "detect_label": "person",
    "min_confidence": 0.25
  },
  "autonomy": {
    "level": "supervised"
  }
}
```

## 2.2 Result

```json
{
  "task_id": "uuid-or-stable-id",
  "trace_id": "trace-id",
  "status": "completed",
  "executor_used": "mcp",
  "risk_level": "medium",
  "result": {
    "message": "客厅摄像头分析完成",
    "artifacts": [],
    "events": [],
    "next_actions": []
  },
  "audit_ref": "audit-uuid"
}
```

## 2.3 平台字段要求

- `task_id`：全链路稳定 ID
- `trace_id`：跨组件追踪 ID
- `source.channel`：`feishu | wecom | telegram | web | mobile | api`
- `intent.domain`：首批固定为 `camera`
- `intent.action`：映射到标准 domain action
- `entity_refs`：平台解析到的实体引用
- `args`：动作参数
- `executor_used`：保留审计值
- `audit_ref`：唯一审计引用

---

## 3. Camera Domain Actions

首批冻结六个动作。

## 3.1 `camera.scan`

用途：

- 扫描局域网中的摄像头候选设备

输入：

```json
{
  "cidr": "192.168.3.0/24",
  "protocols": ["onvif", "ssdp", "mdns", "rtsp_probe"],
  "rtsp_username": "optional",
  "rtsp_password": "optional"
}
```

输出：

```json
{
  "summary": "发现 3 台候选设备，其中 1 台已完成 RTSP 验证",
  "candidates": [
    {
      "candidate_id": "rtsp-192-168-3-73",
      "name": "Living Room Cam",
      "ip": "192.168.3.73",
      "protocol": "ONVIF + RTSP / 已验证",
      "reachable": true,
      "requires_auth": false
    }
  ]
}
```

风险：

- `LOW`

补参：

- 若缺 CIDR，允许平台使用默认策略

## 3.2 `camera.connect`

用途：

- 将候选摄像头正式接入设备库

输入：

```json
{
  "candidate_id": "rtsp-192-168-3-73",
  "name": "客厅摄像头",
  "room": "客厅",
  "ip": "192.168.3.73",
  "port": 554,
  "path_candidates": ["/stream1", "/stream2", "/Streaming/Channels/101"],
  "snapshot_url": "http://192.168.3.73/snapshot.jpg",
  "username": "optional",
  "password": "optional"
}
```

输出：

```json
{
  "summary": "设备已通过 RTSP 验证并写入设备库",
  "device": {
    "device_id": "cam-rtsp-192-168-3-73",
    "name": "客厅摄像头",
    "room": "客厅"
  }
}
```

风险：

- `MEDIUM`

补参：

- 如探测到鉴权失败，应返回 `missing_fields=["password"]`

## 3.3 `camera.snapshot`

用途：

- 抓取一张最新截图

输入：

```json
{
  "device_id": "cam-rtsp-192-168-3-73"
}
```

输出：

```json
{
  "summary": "已抓拍 1 张图片",
  "artifacts": [
    {
      "kind": "image",
      "mime_type": "image/jpeg",
      "path": ".harborbeacon/tmp/feishu-snapshots/cam.jpg"
    }
  ]
}
```

风险：

- `LOW`

## 3.4 `camera.share_link`

用途：

- 生成平台托管的共享观看入口
- 当前正式动作名为 `camera.share_link`
- `camera.live_view` 只保留为兼容别名，不再作为冻结主口径

输入：

```json
{
  "device_id": "cam-rtsp-192-168-3-73"
}
```

输出：

```json
{
  "summary": "已生成观看入口",
  "artifacts": [
    {
      "kind": "link",
      "label": "共享观看链接",
      "url": "/shared/cameras/camera-share-token",
      "scope": "public_link"
    }
  ]
}
```

风险：

- `MEDIUM`

说明：

- 输出应为平台签发的临时共享入口，而不是原始设备直连地址
- 当前 canary 主链路使用 `share_link_id` / `media_session_id` 追踪共享记录

## 3.5 `camera.analyze`

用途：

- 抓拍并执行视觉分析，返回摘要与图片

输入：

```json
{
  "device_id": "cam-rtsp-192-168-3-73",
  "detect_label": "person",
  "min_confidence": 0.25,
  "prompt": "optional"
}
```

输出：

```json
{
  "summary": "客厅摄像头分析完成",
  "analysis": {
    "text": "画面中发现 1 人，位于中部偏左，建议关注",
    "source": "openai_compatible_or_fallback"
  },
  "artifacts": [
    {
      "kind": "image",
      "path": ".harborbeacon/vision/annotated/example.jpg"
    }
  ]
}
```

风险：

- `LOW`

## 3.6 `camera.ptz`

用途：

- 控制云台方向

输入：

```json
{
  "device_id": "cam-rtsp-192-168-3-73",
  "direction": "left",
  "mode": "fine"
}
```

输出：

```json
{
  "summary": "客厅摄像头已精调左转"
}
```

风险：

- `MEDIUM`

说明：

- 后续如扩展到预置位、连续移动、巡航，也继续挂在 `camera.ptz`

---

## 4. Artifact Contract

平台侧至少要接受以下 artifact：

- `image`
- `video`
- `link`
- `card`
- `text`

推荐最小结构：

```json
{
  "kind": "image",
  "label": "抓拍结果",
  "mime_type": "image/jpeg",
  "path": "optional-local-path",
  "url": "optional-public-url",
  "metadata": {}
}
```

---

## 5. 补参 Contract

当动作无法继续执行但适合通过对话补参时，不直接返回终态失败，而返回：

```json
{
  "status": "needs_input",
  "missing_fields": ["password"],
  "prompt": "这台摄像头需要密码，请回复：密码 xxxxxx",
  "resume_token": "opaque-token"
}
```

适用动作：

- `camera.connect`
- 后续可能扩展到 `camera.ptz`、`camera.share_link`

---

## 6. 近期实现映射

当前仓库里的已有实现可大致映射为：

- `camera.scan` -> `CameraHubService::scan`
- `camera.connect` -> `CameraHubService::manual_add`
- `camera.snapshot` -> `CameraHubService::capture_camera_snapshot`
- `camera.analyze` -> `vision.analyze_camera`
- `camera.share_link` -> `remote_view + admin api`
- `camera.ptz` -> `feishu_harbor_bot` 现有 PTZ 逻辑，后续应下沉成正式 domain action

---

## 7. 当前限制

当前契约先解决 Home Agent Hub 首个垂直域入轨问题，不覆盖：

- 多摄像头批量操作
- 工作流编排 DSL
- 多租户与家庭站点隔离
- 复杂多模态联合检索
- 长期历史时间线查询

这些能力后续再在平台主干或域模型中扩展。

