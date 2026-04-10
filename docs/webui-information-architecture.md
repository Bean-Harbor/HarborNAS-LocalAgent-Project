# WebUI Information Architecture

更新时间：2026-04-10

## 1. 设计原则

- 与 TrueNAS 风格和 Angular 工程方式保持兼容
- 前台和后台共用一套设计系统与 API SDK
- 普通用户优先简单、直接、少配置
- 管理员后台优先治理、监控、配置深度

## 2. 应用结构

```text
webui/
  apps/
    portal/
    admin/
  shared/
    api/
    auth/
    models/
    ui/
    workflow/
```

## 3. Portal 页面

- `/`：家庭首页 Dashboard
- `/devices`：设备列表与房间视图
- `/cameras`：摄像头实时与历史
- `/scenes`：常用场景
- `/alerts`：告警列表
- `/timeline`：事件时间线
- `/agent`：Home Agent 对话页
- `/me`：个人中心

## 4. Admin 页面

- `/admin`：总览
- `/admin/devices`：设备中心
- `/admin/models`：模型中心
- `/admin/plugins`：插件中心
- `/admin/workflows`：工作流列表
- `/admin/workflows/:id`：工作流编辑器
- `/admin/runtime`：运行监控
- `/admin/users`：用户管理
- `/admin/roles`：角色与权限
- `/admin/settings`：系统设置

## 5. 共享能力

- 统一登录和会话
- 统一通知中心
- 统一资源选择器：Home / Room / Device / Workflow
- 统一实时订阅：告警、设备状态、工作流执行状态
