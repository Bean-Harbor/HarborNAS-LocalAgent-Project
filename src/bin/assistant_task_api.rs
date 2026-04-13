use std::io::Read;
use std::path::PathBuf;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use harbornas_local_agent::runtime::admin_console::AdminConsoleStore;
use harbornas_local_agent::runtime::registry::DeviceRegistryStore;
use harbornas_local_agent::runtime::task_api::{TaskApiService, TaskRequest};
use harbornas_local_agent::runtime::task_session::TaskConversationStore;

#[derive(Debug, Parser)]
#[command(name = "assistant-task-api")]
#[command(about = "Local Assistant Task API for HarborBeacon <-> Home Agent Hub bridging")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4175", help = "Bind address")]
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
        default_value = ".harbornas/task-api-conversations.json",
        help = "Task conversation state file"
    )]
    conversations: PathBuf,
}

#[derive(Debug, Clone)]
struct TaskApiHttpServer {
    service: TaskApiService,
}

impl TaskApiHttpServer {
    fn new(service: TaskApiService) -> Self {
        Self { service }
    }

    fn handle(&self, mut request: Request) {
        let method = request.method().clone();
        let path = request.url().split('?').next().unwrap_or("/");

        let response = match method {
            Method::Get if path == "/healthz" => ok_json(&json!({"status":"ok"})).boxed(),
            Method::Post if path == "/api/tasks" => self.handle_task(&mut request).boxed(),
            Method::Options => no_content().boxed(),
            _ => error_json(StatusCode(404), "route not found").boxed(),
        };

        let _ = request.respond(response);
    }

    fn handle_task(&self, request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
        let body: TaskRequest = match read_json_body(request) {
            Ok(body) => body,
            Err(error) => return error_json(StatusCode(400), &error),
        };
        let response = self.service.handle_task(body);
        ok_json(&response)
    }
}

fn main() {
    let cli = Cli::parse();

    let registry_store = DeviceRegistryStore::new(cli.device_registry.clone());
    let admin_store = AdminConsoleStore::new(cli.admin_state.clone(), registry_store);
    let conversation_store = TaskConversationStore::new(cli.conversations.clone());
    let service = TaskApiService::new(admin_store, conversation_store);
    let api = TaskApiHttpServer::new(service);

    let server = Server::http(&cli.bind).unwrap_or_else(|error| {
        panic!("failed to bind assistant task api on {}: {error}", cli.bind);
    });
    println!("assistant-task-api listening on http://{}", cli.bind);

    for request in server.incoming_requests() {
        api.handle(request);
    }
}

fn read_json_body<T: for<'de> Deserialize<'de>>(request: &mut Request) -> Result<T, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|error| format!("failed to read request body: {error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("invalid JSON body: {error}"))
}

fn ok_json(payload: &impl Serialize) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(StatusCode(200), payload)
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
