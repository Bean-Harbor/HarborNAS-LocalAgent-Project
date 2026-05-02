use std::env;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use serde::Serialize;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[path = "agent_hub_admin_api.rs"]
mod agent_hub_admin_api;
#[path = "assistant_task_api.rs"]
mod assistant_task_api;
#[path = "harbor_model_api_support.rs"]
mod harbor_model_api_support;

use harbor_model_api_support::{ModelApiConfig, ModelApiService};
use harborbeacon_local_agent::runtime::admin_console::AdminConsoleStore;
use harborbeacon_local_agent::runtime::model_center::ADMIN_STATE_PATH_ENV;
use harborbeacon_local_agent::runtime::registry::DeviceRegistryStore;
use harborbeacon_local_agent::runtime::task_api::TaskApiService;
use harborbeacon_local_agent::runtime::task_session::TaskConversationStore;

#[derive(Debug, Clone)]
struct Cli {
    bind: String,
    admin_state: PathBuf,
    device_registry: PathBuf,
    conversations: PathBuf,
    harbordesk_dist: PathBuf,
    public_origin: String,
    service_token: Option<String>,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:4174".to_string(),
            admin_state: PathBuf::from(".harborbeacon/admin-console.json"),
            device_registry: PathBuf::from(".harborbeacon/device-registry.json"),
            conversations: PathBuf::from(".harborbeacon/task-api-conversations.json"),
            harbordesk_dist: PathBuf::from("frontend/harbordesk/dist/harbordesk"),
            public_origin: "http://harborbeacon.local:4174".to_string(),
            service_token: None,
        }
    }
}

impl Cli {
    fn parse() -> Self {
        let args = env::args().skip(1).collect::<Vec<_>>();
        if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
            print_usage();
            std::process::exit(0);
        }

        let mut cli = Self::default();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            match arg.as_str() {
                "--bind" => cli.bind = take_value(&args, &mut index, "--bind"),
                value if value.starts_with("--bind=") => {
                    cli.bind = value["--bind=".len()..].to_string();
                }
                "--admin-state" => {
                    cli.admin_state = PathBuf::from(take_value(&args, &mut index, "--admin-state"))
                }
                value if value.starts_with("--admin-state=") => {
                    cli.admin_state = PathBuf::from(value["--admin-state=".len()..].to_string())
                }
                "--device-registry" => {
                    cli.device_registry =
                        PathBuf::from(take_value(&args, &mut index, "--device-registry"))
                }
                value if value.starts_with("--device-registry=") => {
                    cli.device_registry =
                        PathBuf::from(value["--device-registry=".len()..].to_string())
                }
                "--conversations" => {
                    cli.conversations =
                        PathBuf::from(take_value(&args, &mut index, "--conversations"))
                }
                value if value.starts_with("--conversations=") => {
                    cli.conversations = PathBuf::from(value["--conversations=".len()..].to_string())
                }
                "--harbordesk-dist" => {
                    cli.harbordesk_dist =
                        PathBuf::from(take_value(&args, &mut index, "--harbordesk-dist"))
                }
                value if value.starts_with("--harbordesk-dist=") => {
                    cli.harbordesk_dist =
                        PathBuf::from(value["--harbordesk-dist=".len()..].to_string())
                }
                "--public-origin" => {
                    cli.public_origin = take_value(&args, &mut index, "--public-origin")
                }
                value if value.starts_with("--public-origin=") => {
                    cli.public_origin = value["--public-origin=".len()..].to_string();
                }
                "--service-token" => {
                    cli.service_token = Some(take_value(&args, &mut index, "--service-token"))
                }
                value if value.starts_with("--service-token=") => {
                    cli.service_token = Some(value["--service-token=".len()..].to_string())
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                value if value.starts_with('-') => fail(&format!("unknown flag: {value}")),
                value => fail(&format!("unexpected positional argument: {value}")),
            }
            index += 1;
        }
        cli
    }
}

#[derive(Clone)]
struct HarborBeaconService {
    admin_api: agent_hub_admin_api::AdminApi,
    task_api: assistant_task_api::TaskApiHttpServer,
    model_api: Arc<ModelApiService>,
}

impl HarborBeaconService {
    fn new(cli: &Cli, service_token: String) -> Self {
        env::set_var(ADMIN_STATE_PATH_ENV, &cli.admin_state);
        let registry_store = DeviceRegistryStore::new(cli.device_registry.clone());
        let admin_store = AdminConsoleStore::new(cli.admin_state.clone(), registry_store);
        let conversation_store = TaskConversationStore::new(cli.conversations.clone());
        let task_service = TaskApiService::new(admin_store.clone(), conversation_store);
        let mut model_config = ModelApiConfig::from_env();
        model_config.bind = cli.bind.clone();
        Self {
            admin_api: agent_hub_admin_api::AdminApi::new(
                admin_store,
                task_service.clone(),
                cli.harbordesk_dist.clone(),
                cli.public_origin.clone(),
            ),
            task_api: assistant_task_api::TaskApiHttpServer::new(task_service, service_token),
            model_api: Arc::new(ModelApiService::new(model_config)),
        }
    }

    fn handle(&self, request: Request) {
        let path = request.url().split('?').next().unwrap_or("/").to_string();
        if path == "/healthz" {
            let _ = request.respond(ok_json(&json!({
                "status": "ok",
                "service": "harborbeacon",
                "topology": "single-port",
                "admin": "/api/admin/*",
                "web": "/api/web/*",
                "inference": "/api/inference/*"
            })));
            return;
        }
        if path == "/api/web/turns" || path == "/api/turns" {
            self.task_api.handle(request);
            return;
        }
        if path == "/api/inference" || path.starts_with("/api/inference/") {
            self.handle_inference(request);
            return;
        }
        self.admin_api.handle(request);
    }

    fn handle_inference(&self, mut request: Request) {
        let method = request.method().clone();
        let path = request.url().split('?').next().unwrap_or("/");
        let model_path = inference_model_path(path);
        let headers = request.headers().to_vec();
        let body = if method == Method::Post {
            match read_request_body(&mut request) {
                Ok(body) => body,
                Err(error) => {
                    let _ = request.respond(error_json(
                        StatusCode(500),
                        "INFRASTRUCTURE_ERROR",
                        &error,
                    ));
                    return;
                }
            }
        } else {
            Vec::new()
        };
        let response = self.model_api.route(method, &model_path, &headers, &body);
        let _ = request.respond(response);
    }
}

fn main() {
    let cli = Cli::parse();
    let service_token = resolve_service_token(cli.service_token.clone());
    let service = HarborBeaconService::new(&cli, service_token);
    let server = Server::http(&cli.bind).unwrap_or_else(|error| {
        panic!("failed to bind harborbeacon service on {}: {error}", cli.bind);
    });
    println!(
        "harborbeacon-service listening on http://{} (admin/web/inference single-port)",
        cli.bind
    );

    for request in server.incoming_requests() {
        let service = service.clone();
        thread::spawn(move || service.handle(request));
    }
}

fn inference_model_path(path: &str) -> String {
    if path == "/api/inference" || path == "/api/inference/" {
        return "/healthz".to_string();
    }
    let tail = path
        .strip_prefix("/api/inference")
        .unwrap_or(path)
        .trim_start_matches('/');
    format!("/{tail}")
}

fn resolve_service_token(cli_token: Option<String>) -> String {
    cli_token
        .or_else(|| env::var("HARBOR_TASK_API_BEARER_TOKEN").ok())
        .or_else(|| env::var("HARBORBEACON_SERVICE_TOKEN").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            eprintln!(
                "harborbeacon-service requires a bearer token via --service-token, HARBOR_TASK_API_BEARER_TOKEN, or HARBORBEACON_SERVICE_TOKEN"
            );
            std::process::exit(2);
        })
}

fn read_request_body(request: &mut Request) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    request
        .as_reader()
        .read_to_end(&mut body)
        .map_err(|error| format!("failed to read request body: {error}"))?;
    Ok(body)
}

fn ok_json(payload: &impl Serialize) -> Response<Cursor<Vec<u8>>> {
    json_response(StatusCode(200), payload)
}

fn error_json(status: StatusCode, code: &'static str, message: &str) -> Response<Cursor<Vec<u8>>> {
    json_response(
        status,
        &json!({
            "ok": false,
            "error": {
                "code": code,
                "message": message
            }
        }),
    )
}

fn json_response(status: StatusCode, payload: &impl Serialize) -> Response<Cursor<Vec<u8>>> {
    let body = serde_json::to_vec_pretty(payload)
        .unwrap_or_else(|_| b"{\"ok\":false,\"error\":{\"code\":\"SERIALIZE_FAILED\"}}".to_vec());
    let mut response = Response::from_data(body).with_status_code(status);
    response.add_header(
        Header::from_bytes(
            b"Content-Type".as_slice(),
            b"application/json; charset=utf-8".as_slice(),
        )
        .expect("header"),
    );
    response.add_header(
        Header::from_bytes(b"Cache-Control".as_slice(), b"no-store".as_slice()).expect("header"),
    );
    response
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> String {
    *index += 1;
    if *index >= args.len() {
        fail(&format!("missing value for {flag}"));
    }
    args[*index].clone()
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn print_usage() {
    eprintln!(
        "Usage: harborbeacon-service [--bind ADDR] [--admin-state PATH] [--device-registry PATH] [--conversations PATH] [--harbordesk-dist PATH] [--public-origin URL] [--service-token TOKEN]"
    );
}
