use std::fmt::Write as _;
use std::path::PathBuf;

use clap::Parser;
use qrcodegen::{QrCode, QrCodeEcc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use harbornas_local_agent::runtime::admin_console::{
    AdminConsoleStore, AdminDefaults, FeishuUserBinding,
};
use harbornas_local_agent::runtime::hub::{
    CameraConnectRequest, CameraHubService, HubManualAddSummary, HubScanRequest, HubScanSummary,
    HubStateSnapshot,
};
use harbornas_local_agent::runtime::registry::DeviceRegistryStore;

#[derive(Debug, Parser)]
#[command(name = "agent-hub-admin-api")]
#[command(about = "Local admin API for HarborNAS Agent Hub demo console")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4174", help = "Bind address")]
    bind: String,

    #[arg(
        long,
        default_value = ".harbornas/admin-console.json",
        help = "Admin console state file"
    )]
    admin_state: PathBuf,

    #[arg(
        long,
        default_value = ".harbornas/device-registry.json",
        help = "Device registry file"
    )]
    device_registry: PathBuf,

    #[arg(
        long,
        default_value = "http://harbornas.local:4174",
        help = "Public origin used in QR setup URL"
    )]
    public_origin: String,
}

#[derive(Debug, Clone)]
struct AdminApi {
    admin_store: AdminConsoleStore,
    public_origin: String,
}

#[derive(Debug, Deserialize)]
struct BindingTestRequest {
    #[serde(default)]
    binding_code: Option<String>,
    display_name: String,
    #[serde(default)]
    open_id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    union_id: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManualAddRequest {
    name: String,
    room: Option<String>,
    ip: String,
    path: Option<String>,
    username: Option<String>,
    password: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct DefaultsRequest {
    cidr: String,
    discovery: String,
    recording: String,
    capture: String,
    ai: String,
    feishu_group: String,
    #[serde(default)]
    rtsp_username: String,
    #[serde(default)]
    rtsp_password: String,
    #[serde(default)]
    rtsp_port: Option<u16>,
    #[serde(default)]
    rtsp_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FeishuConfigRequest {
    app_id: String,
    app_secret: String,
}

type StateResponse = HubStateSnapshot;
type ScanRequest = HubScanRequest;
type ScanResponse = HubScanSummary;
type ManualAddResponse = HubManualAddSummary;

impl AdminApi {
    fn new(admin_store: AdminConsoleStore, public_origin: String) -> Self {
        Self {
            admin_store,
            public_origin,
        }
    }

    fn hub(&self) -> CameraHubService {
        CameraHubService::new(self.admin_store.clone())
    }

    fn handle(&self, mut request: Request) {
        let method = request.method().clone();
        let path = request.url().split('?').next().unwrap_or("/");

        let response = match method {
            Method::Get if path == "/healthz" => ok_json(&json!({"status":"ok"})),
            Method::Get if path == "/api/state" => self.handle_state(),
            Method::Get if path == "/api/binding/qr.svg" => self.handle_binding_qr_svg(),
            Method::Get if path == "/api/binding/static-qr.svg" => {
                self.handle_static_binding_qr_svg()
            }
            Method::Get if path == "/setup/mobile" => self.handle_mobile_setup_page(request.url()),
            Method::Get if path.starts_with("/api/cameras/") && path.ends_with("/snapshot.jpg") => {
                self.handle_camera_snapshot(path)
            }
            Method::Post if path == "/api/binding/refresh" => self.handle_refresh_binding(),
            Method::Post if path == "/api/binding/demo-bind" => self.handle_demo_bind(),
            Method::Post if path == "/api/binding/test-bind" => self.handle_test_bind(&mut request),
            Method::Post if path == "/api/feishu/configure" => {
                self.handle_configure_feishu(&mut request)
            }
            Method::Post if path == "/api/discovery/scan" => self.handle_scan(&mut request),
            Method::Post if path == "/api/devices/manual" => self.handle_manual_add(&mut request),
            Method::Post if path == "/api/defaults" => self.handle_save_defaults(&mut request),
            Method::Options => no_content(),
            _ => error_json(StatusCode(404), "route not found"),
        };

        let _ = request.respond(response);
    }

    fn handle_state(&self) -> Response<std::io::Cursor<Vec<u8>>> {
        match self.current_state() {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_binding_qr_svg(&self) -> Response<std::io::Cursor<Vec<u8>>> {
        let state = match self.current_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let setup_url = state.binding.setup_url.clone();
        let qr = match QrCode::encode_text(&setup_url, QrCodeEcc::Medium) {
            Ok(qr) => qr,
            Err(error) => {
                return error_json(StatusCode(500), &format!("qr encode failed: {error}"))
            }
        };
        let svg = qr_to_svg(&qr, 4);
        let mut response = Response::from_string(svg).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"image/svg+xml; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_static_binding_qr_svg(&self) -> Response<std::io::Cursor<Vec<u8>>> {
        let state = match self.current_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let setup_url = state.binding.static_setup_url.clone();
        let qr = match QrCode::encode_text(&setup_url, QrCodeEcc::Medium) {
            Ok(qr) => qr,
            Err(error) => {
                return error_json(StatusCode(500), &format!("qr encode failed: {error}"))
            }
        };
        let svg = qr_to_svg(&qr, 4);
        let mut response = Response::from_string(svg).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"image/svg+xml; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_mobile_setup_page(&self, url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
        let state = match self.current_state() {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(500), &error),
        };
        let session_code =
            parse_query_param(url, "session").unwrap_or_else(|| state.binding.session_code.clone());
        let body = render_mobile_setup_page(&state, &session_code);
        let mut response = Response::from_string(body).with_status_code(StatusCode(200));
        add_common_headers(&mut response);
        response.add_header(
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"text/html; charset=utf-8".as_slice(),
            )
            .expect("header"),
        );
        response
    }

    fn handle_refresh_binding(&self) -> Response<std::io::Cursor<Vec<u8>>> {
        match self
            .admin_store
            .refresh_binding_qr()
            .and_then(|_| self.current_state())
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_demo_bind(&self) -> Response<std::io::Cursor<Vec<u8>>> {
        match self
            .admin_store
            .mark_demo_bound("Bean / 飞书管理员")
            .and_then(|_| self.current_state())
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_test_bind(&self, request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
        let body: BindingTestRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        let display_name = body.display_name.trim();
        if display_name.is_empty() {
            return error_json(StatusCode(400), "display_name 不能为空");
        }

        let state = match self.admin_store.load_or_create_state() {
            Ok(state) => state,
            Err(error) => return error_json(StatusCode(500), &error),
        };

        let binding_code = body
            .binding_code
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(state.binding.session_code.as_str())
            .to_string();
        let open_id = body
            .open_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .unwrap_or_else(|| slugify_identity(display_name));

        let user = FeishuUserBinding {
            open_id,
            user_id: body
                .user_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            union_id: body
                .union_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            display_name: display_name.to_string(),
            chat_id: body
                .chat_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        };

        match self
            .admin_store
            .bind_feishu_user(&binding_code, user)
            .and_then(|_| self.current_state())
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_configure_feishu(&self, request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
        let body: FeishuConfigRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        match self.hub().configure_feishu_bot(
            &body.app_id,
            &body.app_secret,
            Some(&self.public_origin),
        ) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_scan(&self, request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
        let body: ScanRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        match self.scan(body) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_manual_add(&self, request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
        let body: ManualAddRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        match self.manual_add(body) {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn handle_save_defaults(&self, request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
        let body: DefaultsRequest = match read_json_body(request) {
            Ok(payload) => payload,
            Err(error) => return error_json(StatusCode(400), &error),
        };

        let defaults = AdminDefaults {
            cidr: body.cidr,
            discovery: body.discovery,
            recording: body.recording,
            capture: body.capture,
            ai: body.ai,
            feishu_group: body.feishu_group,
            rtsp_username: body.rtsp_username,
            rtsp_password: body.rtsp_password,
            rtsp_port: body.rtsp_port.unwrap_or(554),
            rtsp_paths: body.rtsp_paths,
        };

        match self
            .hub()
            .save_defaults(defaults, Some(&self.public_origin))
        {
            Ok(payload) => ok_json(&payload),
            Err(error) => error_json(StatusCode(500), &error),
        }
    }

    fn handle_camera_snapshot(&self, path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
        let device_id = match parse_camera_snapshot_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid camera snapshot path"),
        };

        match self.capture_camera_snapshot(&device_id) {
            Ok(bytes) => image_response(StatusCode(200), bytes, "image/jpeg"),
            Err(error) if error.contains("device not found") => error_json(StatusCode(404), &error),
            Err(error) => error_json(StatusCode(422), &error),
        }
    }

    fn current_state(&self) -> Result<StateResponse, String> {
        self.hub().state_snapshot(Some(&self.public_origin))
    }

    fn scan(&self, request: ScanRequest) -> Result<ScanResponse, String> {
        self.hub().scan(request, Some(&self.public_origin))
    }

    fn manual_add(&self, request: ManualAddRequest) -> Result<ManualAddResponse, String> {
        let path_candidates = request
            .path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                if value.starts_with('/') {
                    value.to_string()
                } else {
                    format!("/{value}")
                }
            })
            .map(|value| vec![value])
            .unwrap_or_default();

        self.hub().manual_add(
            CameraConnectRequest {
                name: request.name,
                room: request.room,
                ip: request.ip,
                path_candidates,
                username: request.username,
                password: request.password,
                port: request.port,
                discovery_source: "manual_entry".to_string(),
                vendor: None,
                model: None,
            },
            Some(&self.public_origin),
        )
    }

    fn capture_camera_snapshot(&self, device_id: &str) -> Result<Vec<u8>, String> {
        self.hub().capture_camera_snapshot(device_id)
    }
}

fn main() {
    let cli = Cli::parse();
    let registry_store = DeviceRegistryStore::new(cli.device_registry);
    let admin_store = AdminConsoleStore::new(cli.admin_state, registry_store);
    let api = AdminApi::new(admin_store, cli.public_origin);

    let server = Server::http(&cli.bind).unwrap_or_else(|error| {
        eprintln!("failed to start admin api on {}: {}", cli.bind, error);
        std::process::exit(1);
    });

    println!(
        "HarborNAS Agent Hub admin API listening on http://{}",
        cli.bind
    );
    for request in server.incoming_requests() {
        api.handle(request);
    }
}

fn parse_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=')?;
        if name == key {
            return Some(value.to_string());
        }
    }
    None
}

fn render_mobile_setup_page(state: &StateResponse, session_code: &str) -> String {
    let app_id = html_escape(&state.feishu_bot.app_id);
    let app_secret = html_escape(&state.feishu_bot.app_secret);
    let bot_name = if state.feishu_bot.app_name.trim().is_empty() {
        "尚未配置".to_string()
    } else {
        html_escape(&state.feishu_bot.app_name)
    };
    let status = html_escape(&state.binding.metric);
    let session_code = html_escape(session_code);
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>HarborNAS 飞书 Bot 配置</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, sans-serif; background: #f4efe7; color: #1e1b18; margin: 0; }}
    .wrap {{ max-width: 560px; margin: 0 auto; padding: 24px 18px 40px; }}
    .card {{ background: rgba(255,255,255,0.9); border-radius: 20px; padding: 20px; box-shadow: 0 18px 48px rgba(51,36,18,0.12); }}
    h1 {{ margin: 0 0 8px; font-size: 28px; }}
    p {{ line-height: 1.5; }}
    .meta {{ color: #6b5a49; font-size: 14px; margin-bottom: 18px; }}
    label {{ display: block; margin: 14px 0 8px; font-weight: 600; }}
    input {{ width: 100%; box-sizing: border-box; padding: 14px 12px; border-radius: 12px; border: 1px solid #d9c6ae; font-size: 16px; }}
    button {{ width: 100%; margin-top: 18px; padding: 14px 16px; border: 0; border-radius: 999px; background: #1f7a6f; color: white; font-size: 16px; font-weight: 700; }}
    .status {{ margin: 16px 0; padding: 12px 14px; border-radius: 14px; background: #f6f2ec; }}
    .hint {{ font-size: 13px; color: #766757; }}
    .ok {{ color: #1f7a6f; }}
    .err {{ color: #b94739; }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="card">
      <div class="meta">HarborNAS Agent Hub · 手机配置页</div>
      <h1>绑定飞书 Bot</h1>
      <p>在这台手机上填入你刚创建的飞书机器人的 <code>app_id</code> 和 <code>app_secret</code>。保存成功后，这台 Agent Hub 就能正式接通飞书消息。</p>
      <div class="status">
        <div><strong>当前状态：</strong><span id="status-text">{status}</span></div>
        <div><strong>当前会话：</strong>{session_code}</div>
        <div><strong>已连接 Bot：</strong><span id="bot-name">{bot_name}</span></div>
      </div>
      <label for="app-id">App ID</label>
      <input id="app-id" value="{app_id}" autocomplete="off" />
      <label for="app-secret">App Secret</label>
      <input id="app-secret" type="password" value="{app_secret}" autocomplete="off" />
      <button id="submit-btn">保存并验证飞书连接</button>
      <p class="hint">保存时会立即调用飞书 Open API 校验凭证，并读取 Bot 信息。成功后桌面端后台会同步显示“Bot 已连接”。你也可以把这台机器上贴的静态二维码固定为 <code>{}</code>。</p>
      <p id="result" class="hint"></p>
    </div>
  </div>
  <script>
    document.getElementById('submit-btn').addEventListener('click', async () => {{
      const result = document.getElementById('result');
      result.className = 'hint';
      result.textContent = '正在验证飞书凭证...';
      try {{
        const response = await fetch('/api/feishu/configure', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: JSON.stringify({{
            app_id: document.getElementById('app-id').value.trim(),
            app_secret: document.getElementById('app-secret').value.trim()
          }})
        }});
        const payload = await response.json();
        if (!response.ok) {{
          throw new Error(payload.error || '保存失败');
        }}
        document.getElementById('status-text').textContent = payload.binding.metric || 'Bot 已连接';
        document.getElementById('bot-name').textContent = payload.feishu_bot?.app_name || '已连接';
        result.className = 'hint ok';
        result.textContent = '飞书 Bot 已验证成功，现在可以回到飞书里直接和 Bot 对话了。';
      }} catch (error) {{
        result.className = 'hint err';
        result.textContent = error.message;
      }}
    }});
  </script>
</body>
</html>"#,
        html_escape(&state.binding.static_setup_url)
    )
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => {
                let _ = escaped.write_char(ch);
            }
        }
    }
    escaped
}

fn qr_to_svg(qr: &QrCode, border: i32) -> String {
    let size = qr.size();
    let dimension = size + border * 2;
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                let _ = write!(&mut path, "M{},{}h1v1h-1z ", x + border, y + border);
            }
        }
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {d} {d}" shape-rendering="crispEdges"><rect width="100%" height="100%" fill="#f8f3ec"/><path d="{path}" fill="#1b1814"/></svg>"##,
        d = dimension,
        path = path.trim()
    )
}

fn slugify_identity(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        format!("ou_local_{}", value.chars().count())
    } else {
        format!("ou_local_{slug}")
    }
}

fn read_json_body<T: for<'de> Deserialize<'de>>(request: &mut Request) -> Result<T, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|e| format!("failed to read request body: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("invalid JSON body: {e}"))
}

fn ok_json(payload: &impl Serialize) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(StatusCode(200), payload)
}

fn image_response(
    status: StatusCode,
    bytes: Vec<u8>,
    mime_type: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_data(bytes).with_status_code(status);
    add_common_headers(&mut response);
    response.add_header(
        Header::from_bytes(b"Content-Type".as_slice(), mime_type.as_bytes()).expect("header"),
    );
    response
}

fn no_content() -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(StatusCode(204));
    add_common_headers(&mut response);
    response
}

fn error_json(status: StatusCode, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(status, &json!({ "error": message }))
}

fn json_response(
    status: StatusCode,
    payload: &impl Serialize,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec_pretty(payload)
        .unwrap_or_else(|_| b"{\"error\":\"serialize failed\"}".to_vec());
    let mut response = Response::from_data(body).with_status_code(status);
    add_common_headers(&mut response);
    response.add_header(
        Header::from_bytes(
            b"Content-Type".as_slice(),
            b"application/json; charset=utf-8".as_slice(),
        )
        .expect("header"),
    );
    response
}

fn add_common_headers(response: &mut Response<std::io::Cursor<Vec<u8>>>) {
    for header in [
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Headers", "Content-Type"),
        ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        ("Cache-Control", "no-store"),
    ] {
        response.add_header(
            Header::from_bytes(header.0.as_bytes(), header.1.as_bytes()).expect("header"),
        );
    }
}

fn parse_camera_snapshot_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/snapshot.jpg")?;
    if device_id.is_empty() {
        None
    } else {
        Some(device_id.to_string())
    }
}
