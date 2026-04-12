use std::fmt::Write as _;
use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};

use clap::Parser;
use qrcodegen::{QrCode, QrCodeEcc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};

use harbornas_local_agent::runtime::admin_console::{
    AdminConsoleStore, AdminDefaults, FeishuUserBinding,
};
use harbornas_local_agent::runtime::hub::{
    CameraConnectRequest, CameraHubService, HubManualAddSummary, HubScanRequest, HubScanSummary,
    HubStateSnapshot,
};
use harbornas_local_agent::runtime::remote_view;
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
        let remote_addr = request.remote_addr().copied();

        let response = match method {
            Method::Get if path == "/healthz" => ok_json(&json!({"status":"ok"})).boxed(),
            Method::Get if path == "/api/state" => self.handle_state().boxed(),
            Method::Get if path == "/api/binding/qr.svg" => self.handle_binding_qr_svg().boxed(),
            Method::Get if path == "/api/binding/static-qr.svg" => {
                self.handle_static_binding_qr_svg().boxed()
            }
            Method::Get if path == "/setup/mobile" => {
                self.handle_mobile_setup_page(request.url()).boxed()
            }
            Method::Get if path.starts_with("/shared/cameras/") && path.ends_with("/live.mjpeg") => {
                self.handle_shared_camera_live_mjpeg(path)
            }
            Method::Get if path.starts_with("/shared/cameras/") => {
                self.handle_shared_live_view_page(path).boxed()
            }
            Method::Get if path.starts_with("/live/cameras/") => {
                self.handle_live_view_page(path, remote_addr).boxed()
            }
            Method::Get if path.starts_with("/api/cameras/") && path.ends_with("/live.mjpeg") => {
                self.handle_camera_live_mjpeg(path, remote_addr)
            }
            Method::Get if path.starts_with("/api/cameras/") && path.ends_with("/snapshot.jpg") => {
                self.handle_camera_snapshot(path, remote_addr).boxed()
            }
            Method::Post if path == "/api/binding/refresh" => self.handle_refresh_binding().boxed(),
            Method::Post if path == "/api/binding/demo-bind" => self.handle_demo_bind().boxed(),
            Method::Post if path == "/api/binding/test-bind" => {
                self.handle_test_bind(&mut request).boxed()
            }
            Method::Post if path == "/api/feishu/configure" => {
                self.handle_configure_feishu(&mut request).boxed()
            }
            Method::Post if path == "/api/discovery/scan" => self.handle_scan(&mut request).boxed(),
            Method::Post if path == "/api/devices/manual" => {
                self.handle_manual_add(&mut request).boxed()
            }
            Method::Post if path == "/api/defaults" => {
                self.handle_save_defaults(&mut request).boxed()
            }
            Method::Options => no_content().boxed(),
            _ => error_json(StatusCode(404), "route not found").boxed(),
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

    fn handle_live_view_page(
        &self,
        path: &str,
        remote_addr: Option<SocketAddr>,
    ) -> Response<Cursor<Vec<u8>>> {
        if let Err(error) = ensure_local_camera_access(remote_addr) {
            return error_json(StatusCode(403), &error);
        }

        let device_id = match parse_camera_live_page_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid live view page path"),
        };

        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };

        let body = render_live_view_page(&self.public_origin, &device);
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

    fn handle_shared_live_view_page(&self, path: &str) -> Response<Cursor<Vec<u8>>> {
        let token = match parse_shared_camera_live_page_path(path) {
            Some(token) => token,
            None => return error_json(StatusCode(400), "invalid shared live view path"),
        };
        let claims = match self.verify_shared_camera_token(&token) {
            Ok(claims) => claims,
            Err(error) => return error_json(StatusCode(403), &error),
        };
        let device = match self.load_camera_device(&claims.device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error)
            }
            Err(error) => return error_json(StatusCode(422), &error),
        };

        let body = render_shared_live_view_page(&token, &device);
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

    fn handle_camera_snapshot(
        &self,
        path: &str,
        remote_addr: Option<SocketAddr>,
    ) -> Response<std::io::Cursor<Vec<u8>>> {
        if let Err(error) = ensure_local_camera_access(remote_addr) {
            return error_json(StatusCode(403), &error);
        }

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

    fn handle_camera_live_mjpeg(&self, path: &str, remote_addr: Option<SocketAddr>) -> ResponseBox {
        if let Err(error) = ensure_local_camera_access(remote_addr) {
            return error_json(StatusCode(403), &error).boxed();
        }

        let device_id = match parse_camera_live_stream_path(path) {
            Some(device_id) => device_id,
            None => return error_json(StatusCode(400), "invalid live stream path").boxed(),
        };

        let device = match self.load_camera_device(&device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error).boxed()
            }
            Err(error) => return error_json(StatusCode(422), &error).boxed(),
        };

        let stream = match FfmpegMjpegStream::spawn(&device.primary_stream.url) {
            Ok(stream) => stream,
            Err(error) => {
                return error_json(StatusCode(422), &format!("打开实时画面失败: {error}")).boxed()
            }
        };

        let headers = vec![
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"multipart/x-mixed-replace;boundary=ffmpeg".as_slice(),
            )
            .expect("header"),
            Header::from_bytes(b"X-Accel-Buffering".as_slice(), b"no".as_slice())
                .expect("header"),
        ];
        let mut response = Response::new(StatusCode(200), headers, stream, None, None).boxed();
        add_common_headers(&mut response);
        response
    }

    fn handle_shared_camera_live_mjpeg(&self, path: &str) -> ResponseBox {
        let token = match parse_shared_camera_live_stream_path(path) {
            Some(token) => token,
            None => return error_json(StatusCode(400), "invalid shared live stream path").boxed(),
        };
        let claims = match self.verify_shared_camera_token(&token) {
            Ok(claims) => claims,
            Err(error) => return error_json(StatusCode(403), &error).boxed(),
        };
        let device = match self.load_camera_device(&claims.device_id) {
            Ok(device) => device,
            Err(error) if error.contains("device not found") => {
                return error_json(StatusCode(404), &error).boxed()
            }
            Err(error) => return error_json(StatusCode(422), &error).boxed(),
        };

        let stream = match FfmpegMjpegStream::spawn(&device.primary_stream.url) {
            Ok(stream) => stream,
            Err(error) => {
                return error_json(StatusCode(422), &format!("打开共享实时画面失败: {error}"))
                    .boxed()
            }
        };

        let headers = vec![
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"multipart/x-mixed-replace;boundary=ffmpeg".as_slice(),
            )
            .expect("header"),
            Header::from_bytes(b"X-Accel-Buffering".as_slice(), b"no".as_slice())
                .expect("header"),
        ];
        let mut response = Response::new(StatusCode(200), headers, stream, None, None).boxed();
        add_common_headers(&mut response);
        response
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

    fn load_camera_device(
        &self,
        device_id: &str,
    ) -> Result<harbornas_local_agent::runtime::registry::CameraDevice, String> {
        self.hub()
            .load_registered_cameras()?
            .into_iter()
            .find(|device| device.device_id == device_id)
            .ok_or_else(|| format!("device not found: {device_id}"))
    }

    fn verify_shared_camera_token(
        &self,
        token: &str,
    ) -> Result<remote_view::CameraShareClaims, String> {
        let state = self.admin_store.load_or_create_state()?;
        remote_view::verify_camera_share_token(&state.remote_view.share_secret, token)
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

fn render_live_view_page(
    public_origin: &str,
    device: &harbornas_local_agent::runtime::registry::CameraDevice,
) -> String {
    let device_label = device.room.as_deref().unwrap_or(device.name.as_str());
    let device_label = html_escape(device_label);
    let device_name = html_escape(&device.name);
    let ip_address = html_escape(device.ip_address.as_deref().unwrap_or("未知 IP"));
    let device_id = url_encode_path_segment(&device.device_id);
    let origin = public_origin.trim_end_matches('/');
    let live_stream_url = format!("{origin}/api/cameras/{device_id}/live.mjpeg");
    let snapshot_url = format!("{origin}/api/cameras/{device_id}/snapshot.jpg");

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{device_label} 实时观看</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #0f1720;
      --card: rgba(10, 18, 28, 0.82);
      --line: rgba(255,255,255,0.12);
      --text: #f3f7fb;
      --muted: #98a8bb;
      --accent: #4fd1c5;
      --danger: #ff8f70;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif;
      background:
        radial-gradient(circle at top, rgba(79,209,197,0.22), transparent 35%),
        linear-gradient(180deg, #0c1218 0%, #101927 100%);
      color: var(--text);
      min-height: 100vh;
    }}
    .wrap {{ max-width: 880px; margin: 0 auto; padding: 20px 16px 28px; }}
    .topbar {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      margin-bottom: 14px;
    }}
    .title {{ margin: 0; font-size: 28px; line-height: 1.1; }}
    .meta {{ color: var(--muted); font-size: 14px; margin-top: 6px; }}
    .status {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      border-radius: 999px;
      background: rgba(255,255,255,0.08);
      border: 1px solid rgba(255,255,255,0.1);
      font-size: 13px;
      white-space: nowrap;
    }}
    .status-dot {{
      width: 10px;
      height: 10px;
      border-radius: 999px;
      background: var(--danger);
      box-shadow: 0 0 0 4px rgba(255,143,112,0.18);
    }}
    .status.live .status-dot {{
      background: var(--accent);
      box-shadow: 0 0 0 4px rgba(79,209,197,0.18);
    }}
    .panel {{
      background: var(--card);
      border: 1px solid var(--line);
      border-radius: 24px;
      padding: 14px;
      box-shadow: 0 24px 60px rgba(0,0,0,0.28);
      backdrop-filter: blur(12px);
    }}
    .viewer {{
      position: relative;
      overflow: hidden;
      border-radius: 18px;
      background: #060b11;
      aspect-ratio: 16 / 9;
    }}
    .viewer img {{
      width: 100%;
      height: 100%;
      display: block;
      object-fit: contain;
      background: #060b11;
    }}
    .overlay {{
      position: absolute;
      left: 12px;
      right: 12px;
      bottom: 12px;
      display: flex;
      justify-content: space-between;
      gap: 12px;
      flex-wrap: wrap;
    }}
    .chip {{
      padding: 8px 10px;
      border-radius: 12px;
      background: rgba(3, 10, 17, 0.72);
      border: 1px solid rgba(255,255,255,0.1);
      font-size: 13px;
      color: var(--muted);
    }}
    .actions {{
      display: flex;
      gap: 10px;
      flex-wrap: wrap;
      margin-top: 14px;
    }}
    .actions button, .actions a {{
      appearance: none;
      border: 0;
      text-decoration: none;
      color: var(--text);
      background: rgba(255,255,255,0.08);
      border: 1px solid rgba(255,255,255,0.1);
      border-radius: 999px;
      padding: 11px 16px;
      font-size: 14px;
      font-weight: 600;
    }}
    .actions .primary {{
      background: linear-gradient(135deg, #27c4b3, #1f8f85);
      border-color: transparent;
    }}
    .hint {{
      margin-top: 14px;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.6;
    }}
    @media (max-width: 640px) {{
      .title {{ font-size: 22px; }}
      .topbar {{ align-items: flex-start; flex-direction: column; }}
      .actions button, .actions a {{ flex: 1 1 calc(50% - 10px); text-align: center; }}
    }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="topbar">
      <div>
        <h1 class="title">{device_label} 实时观看</h1>
        <div class="meta">{device_name} · {ip_address} · 浏览器内低延迟 MJPEG 预览</div>
      </div>
      <div id="status" class="status">
        <span class="status-dot"></span>
        <span id="status-text">正在连接画面…</span>
      </div>
    </div>

    <div class="panel">
      <div class="viewer">
        <img id="stream" src="{live_stream_url}" alt="{device_label} 实时画面" />
        <div class="overlay">
          <div class="chip">链路：RTSP → 本地 ffmpeg → MJPEG</div>
          <div class="chip" id="last-frame">等待首帧…</div>
        </div>
      </div>

      <div class="actions">
        <button id="reload-btn" class="primary" type="button">重连画面</button>
        <a href="{snapshot_url}" target="_blank" rel="noreferrer">打开当前截图</a>
      </div>

      <div class="hint">
        如果画面没有出来，先确认手机和 HarborNAS Agent Hub 在同一个局域网，再点击“重连画面”。
        这个页面只负责看实时视频；拍照、录像、云台控制仍然建议继续在飞书里完成。
      </div>
    </div>
  </div>

  <script>
    const streamUrl = {live_stream_url:?};
    const streamEl = document.getElementById('stream');
    const statusEl = document.getElementById('status');
    const statusTextEl = document.getElementById('status-text');
    const lastFrameEl = document.getElementById('last-frame');

    function setStatus(isLive, text) {{
      statusEl.classList.toggle('live', isLive);
      statusTextEl.textContent = text;
    }}

    function reloadStream() {{
      setStatus(false, '正在重连画面…');
      streamEl.src = `${{streamUrl}}?ts=${{Date.now()}}`;
    }}

    streamEl.addEventListener('load', () => {{
      setStatus(true, '实时画面连接中');
      lastFrameEl.textContent = `最后更新：${{new Date().toLocaleTimeString()}}`;
    }});

    streamEl.addEventListener('error', () => {{
      setStatus(false, '画面连接失败，请重试');
    }});

    document.getElementById('reload-btn').addEventListener('click', reloadStream);
  </script>
</body>
</html>"#
    )
}

fn render_shared_live_view_page(
    share_token: &str,
    device: &harbornas_local_agent::runtime::registry::CameraDevice,
) -> String {
    let device_label = html_escape(device.room.as_deref().unwrap_or(device.name.as_str()));
    let device_name = html_escape(&device.name);
    let ip_address = html_escape(device.ip_address.as_deref().unwrap_or("未知 IP"));
    let live_stream_url = format!(
        "/shared/cameras/{}/live.mjpeg",
        url_encode_path_segment(share_token)
    );

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{device_label} 远程观看</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #0d1119;
      --card: rgba(7, 14, 24, 0.86);
      --line: rgba(255,255,255,0.12);
      --text: #eff6ff;
      --muted: #98a7ba;
      --accent: #58d6c2;
      --danger: #ff9f79;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      min-height: 100vh;
      color: var(--text);
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif;
      background:
        radial-gradient(circle at top, rgba(88,214,194,0.16), transparent 34%),
        linear-gradient(180deg, #0b1118 0%, #0e1723 100%);
    }}
    .wrap {{ max-width: 880px; margin: 0 auto; padding: 18px 14px 28px; }}
    .header {{ display: flex; justify-content: space-between; gap: 12px; align-items: flex-start; margin-bottom: 14px; }}
    .title {{ margin: 0; font-size: 28px; line-height: 1.1; }}
    .meta {{ margin-top: 8px; color: var(--muted); font-size: 14px; line-height: 1.5; }}
    .status {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      border-radius: 999px;
      background: rgba(255,255,255,0.08);
      border: 1px solid rgba(255,255,255,0.1);
      font-size: 13px;
      white-space: nowrap;
    }}
    .status-dot {{
      width: 10px;
      height: 10px;
      border-radius: 999px;
      background: var(--danger);
      box-shadow: 0 0 0 4px rgba(255,159,121,0.16);
    }}
    .status.live .status-dot {{
      background: var(--accent);
      box-shadow: 0 0 0 4px rgba(88,214,194,0.18);
    }}
    .panel {{
      padding: 14px;
      border-radius: 24px;
      background: var(--card);
      border: 1px solid var(--line);
      box-shadow: 0 24px 64px rgba(0,0,0,0.28);
      backdrop-filter: blur(12px);
    }}
    .viewer {{
      position: relative;
      overflow: hidden;
      border-radius: 18px;
      background: #05080d;
      aspect-ratio: 16 / 9;
    }}
    .viewer img {{
      width: 100%;
      height: 100%;
      object-fit: contain;
      display: block;
      background: #05080d;
    }}
    .overlay {{
      position: absolute;
      left: 12px;
      right: 12px;
      bottom: 12px;
      display: flex;
      justify-content: space-between;
      gap: 12px;
      flex-wrap: wrap;
    }}
    .chip {{
      padding: 8px 10px;
      border-radius: 12px;
      background: rgba(3, 10, 17, 0.74);
      border: 1px solid rgba(255,255,255,0.08);
      color: var(--muted);
      font-size: 13px;
    }}
    .actions {{ display: flex; gap: 10px; flex-wrap: wrap; margin-top: 14px; }}
    .actions button {{
      appearance: none;
      border: 0;
      border-radius: 999px;
      padding: 11px 16px;
      font-size: 14px;
      font-weight: 600;
      color: var(--text);
      background: linear-gradient(135deg, #28c6b5, #1d8d82);
    }}
    .hint {{ margin-top: 14px; color: var(--muted); font-size: 13px; line-height: 1.6; }}
    @media (max-width: 640px) {{
      .title {{ font-size: 22px; }}
      .header {{ flex-direction: column; }}
    }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="header">
      <div>
        <h1 class="title">{device_label} 远程观看</h1>
        <div class="meta">{device_name} · {ip_address} · 这是一个带签名的临时分享链接，仅用于看实时画面。</div>
      </div>
      <div id="status" class="status">
        <span class="status-dot"></span>
        <span id="status-text">正在连接画面…</span>
      </div>
    </div>

    <div class="panel">
      <div class="viewer">
        <img id="stream" src="{live_stream_url}" alt="{device_label} 远程实时画面" />
        <div class="overlay">
          <div class="chip">链路：公网入口 → 本地 ffmpeg → MJPEG</div>
          <div class="chip" id="last-frame">等待首帧…</div>
        </div>
      </div>

      <div class="actions">
        <button id="reload-btn" type="button">重连画面</button>
      </div>

      <div class="hint">
        这个链接默认会在一段时间后自动过期。分享出去时请只发给需要查看的人，不要长期公开传播。
      </div>
    </div>
  </div>

  <script>
    const streamUrl = {live_stream_url:?};
    const streamEl = document.getElementById('stream');
    const statusEl = document.getElementById('status');
    const statusTextEl = document.getElementById('status-text');
    const lastFrameEl = document.getElementById('last-frame');

    function setStatus(isLive, text) {{
      statusEl.classList.toggle('live', isLive);
      statusTextEl.textContent = text;
    }}

    function reloadStream() {{
      setStatus(false, '正在重连画面…');
      streamEl.src = `${{streamUrl}}?ts=${{Date.now()}}`;
    }}

    streamEl.addEventListener('load', () => {{
      setStatus(true, '远程画面连接中');
      lastFrameEl.textContent = `最后更新：${{new Date().toLocaleTimeString()}}`;
    }});

    streamEl.addEventListener('error', () => {{
      setStatus(false, '画面连接失败，请重试');
    }});

    document.getElementById('reload-btn').addEventListener('click', reloadStream);
  </script>
</body>
</html>"#
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

fn add_common_headers<R: Read>(response: &mut Response<R>) {
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

fn ensure_local_camera_access(remote_addr: Option<SocketAddr>) -> Result<(), String> {
    if remote_addr.is_none() || remote_addr.is_some_and(is_local_socket_addr) {
        return Ok(());
    }

    Err("当前摄像头直连预览只允许本机或局域网访问；如果要给外网用户观看，请使用带签名的共享链接。".to_string())
}

fn is_local_socket_addr(addr: SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(ip) => ip.is_loopback() || is_private_ipv4(ip),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
    }
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

fn parse_camera_snapshot_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/snapshot.jpg")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_camera_live_stream_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/api/cameras/")?;
    let device_id = trimmed.strip_suffix("/live.mjpeg")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_camera_live_page_path(path: &str) -> Option<String> {
    let device_id = path.strip_prefix("/live/cameras/")?;
    if device_id.is_empty() {
        None
    } else {
        percent_decode_path_segment(device_id).ok()
    }
}

fn parse_shared_camera_live_page_path(path: &str) -> Option<String> {
    let token = path.strip_prefix("/shared/cameras/")?;
    if token.is_empty() || token.contains('/') {
        None
    } else {
        percent_decode_path_segment(token).ok()
    }
}

fn parse_shared_camera_live_stream_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix("/shared/cameras/")?;
    let token = trimmed.strip_suffix("/live.mjpeg")?;
    if token.is_empty() || token.contains('/') {
        None
    } else {
        percent_decode_path_segment(token).ok()
    }
}

fn url_encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn percent_decode_path_segment(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("incomplete percent escape".to_string());
            }
            let hi = decode_hex(bytes[index + 1]).ok_or_else(|| "invalid hex digit".to_string())?;
            let lo = decode_hex(bytes[index + 2]).ok_or_else(|| "invalid hex digit".to_string())?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).map_err(|error| format!("invalid utf-8 path segment: {error}"))
}

fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

struct FfmpegMjpegStream {
    child: Child,
    stdout: ChildStdout,
}

impl FfmpegMjpegStream {
    fn spawn(stream_url: &str) -> Result<Self, String> {
        if which::which("ffmpeg").is_err() {
            return Err("当前机器缺少 ffmpeg".to_string());
        }

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-rtsp_transport",
                "tcp",
                "-fflags",
                "nobuffer",
                "-flags",
                "low_delay",
                "-i",
                stream_url,
                "-an",
                "-vf",
                "fps=5,scale=960:-2:flags=fast_bilinear",
                "-q:v",
                "6",
                "-f",
                "mpjpeg",
                "-boundary_tag",
                "ffmpeg",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("启动实时转码 ffmpeg 失败: {error}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "无法获取 ffmpeg 输出管道".to_string())?;

        Ok(Self { child, stdout })
    }
}

impl Read for FfmpegMjpegStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stdout.read(buf)
    }
}

impl Drop for FfmpegMjpegStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_local_camera_access, parse_camera_live_page_path, parse_camera_live_stream_path,
        parse_camera_snapshot_path, parse_shared_camera_live_page_path,
        parse_shared_camera_live_stream_path, percent_decode_path_segment,
        url_encode_path_segment,
    };
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    #[test]
    fn camera_paths_decode_percent_encoded_device_ids() {
        let encoded = "camera%201%2Fleft";
        assert_eq!(
            parse_camera_snapshot_path(&format!("/api/cameras/{encoded}/snapshot.jpg")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_live_stream_path(&format!("/api/cameras/{encoded}/live.mjpeg")),
            Some("camera 1/left".to_string())
        );
        assert_eq!(
            parse_camera_live_page_path(&format!("/live/cameras/{encoded}")),
            Some("camera 1/left".to_string())
        );
    }

    #[test]
    fn shared_camera_paths_decode_tokens() {
        let encoded = "abc.def%2D123";
        assert_eq!(
            parse_shared_camera_live_page_path(&format!("/shared/cameras/{encoded}")),
            Some("abc.def-123".to_string())
        );
        assert_eq!(
            parse_shared_camera_live_stream_path(&format!("/shared/cameras/{encoded}/live.mjpeg")),
            Some("abc.def-123".to_string())
        );
    }

    #[test]
    fn path_segment_round_trips_utf8_content() {
        let raw = "客厅 camera #1";
        let encoded = url_encode_path_segment(raw);
        assert_eq!(percent_decode_path_segment(&encoded).as_deref(), Ok(raw));
    }

    #[test]
    fn direct_camera_access_is_restricted_to_local_clients() {
        let local = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 3, 12), 4567));
        let remote = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(8, 8, 8, 8), 4567));
        assert!(ensure_local_camera_access(Some(local)).is_ok());
        assert!(ensure_local_camera_access(Some(remote)).is_err());
    }
}
