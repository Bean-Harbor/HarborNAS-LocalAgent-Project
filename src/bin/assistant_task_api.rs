use std::env;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use harbornas_local_agent::runtime::admin_console::AdminConsoleStore;
use harbornas_local_agent::runtime::registry::DeviceRegistryStore;
use harbornas_local_agent::runtime::task_api::{
    TaskApiService, TaskRequest, TaskRequestAcceptance,
};
use harbornas_local_agent::runtime::task_session::TaskConversationStore;

const CONTRACT_VERSION: &str = "1.5";
const SERVICE_TOKEN_ENV: &str = "HARBOR_TASK_API_BEARER_TOKEN";
const HEADER_AUTHORIZATION: &str = "Authorization";
const HEADER_CONTRACT_VERSION: &str = "X-Contract-Version";

#[derive(Debug, Parser)]
#[command(name = "assistant-task-api")]
#[command(about = "Local Assistant Task API for HarborNAS <-> IM Gateway bridging")]
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

    #[arg(
        long,
        help = "Bearer token required from IM Gateway callers; falls back to HARBOR_TASK_API_BEARER_TOKEN"
    )]
    service_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct SharedHttpErrorDetail {
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct SharedHttpErrorEnvelope {
    ok: bool,
    error: SharedHttpErrorDetail,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
}

#[derive(Debug, Clone)]
struct TaskApiHttpServer {
    service: TaskApiService,
    service_token: String,
}

impl TaskApiHttpServer {
    fn new(service: TaskApiService, service_token: String) -> Self {
        Self {
            service,
            service_token,
        }
    }

    fn handle(&self, mut request: Request) {
        let method = request.method().clone();
        let path = request.url().split('?').next().unwrap_or("/").to_string();

        let response = match method {
            Method::Get if path == "/healthz" => ok_json(&json!({"status":"ok"})).boxed(),
            Method::Post if path == "/api/tasks" => self.handle_task(&mut request).boxed(),
            Method::Options => no_content().boxed(),
            _ => shared_error_json(
                StatusCode(404),
                "ROUTE_NOT_FOUND",
                &format!("route not found: {path}"),
                None,
            )
            .boxed(),
        };

        let _ = request.respond(response);
    }

    fn handle_task(&self, request: &mut Request) -> Response<Cursor<Vec<u8>>> {
        let headers = request.headers().to_vec();
        let body = match read_request_body(request) {
            Ok(body) => body,
            Err(error) => {
                return shared_error_json(StatusCode(500), "INFRASTRUCTURE_ERROR", &error, None)
            }
        };
        self.handle_task_payload(&headers, &body)
    }

    fn handle_task_payload(&self, headers: &[Header], body: &[u8]) -> Response<Cursor<Vec<u8>>> {
        let trace_id = trace_id_from_body(body);

        if !self.is_service_authorized(headers) {
            return service_auth_failed(trace_id);
        }

        let Some(contract_version) = header_value(headers, HEADER_CONTRACT_VERSION) else {
            return shared_error_json(
                StatusCode(400),
                "CONTRACT_VERSION_MISMATCH",
                &format!(
                    "missing {HEADER_CONTRACT_VERSION}; expected {HEADER_CONTRACT_VERSION}: {CONTRACT_VERSION}"
                ),
                trace_id,
            );
        };
        if contract_version != CONTRACT_VERSION {
            return shared_error_json(
                StatusCode(400),
                "CONTRACT_VERSION_MISMATCH",
                &format!(
                    "unsupported {HEADER_CONTRACT_VERSION}: {contract_version}; expected {CONTRACT_VERSION}"
                ),
                trace_id,
            );
        }

        let task_request: TaskRequest = match parse_json_body(body) {
            Ok(body) => body,
            Err(error) => {
                return shared_error_json(StatusCode(422), "VALIDATION_ERROR", &error, trace_id)
            }
        };
        if let Err(error) = validate_task_request_contract(&task_request) {
            return shared_error_json(StatusCode(422), "VALIDATION_ERROR", &error, trace_id);
        }

        match self.service.accept_or_replay_task(&task_request) {
            Ok(TaskRequestAcceptance::Accept) => {
                let response = self.service.handle_task(task_request);
                ok_json(&response)
            }
            Ok(TaskRequestAcceptance::Replay(response)) => ok_json(&response),
            Ok(TaskRequestAcceptance::Conflict(message)) => shared_error_json(
                StatusCode(409),
                "IDEMPOTENCY_CONFLICT",
                &message,
                trace_id_from_body(body),
            ),
            Err(error) => shared_error_json(
                StatusCode(500),
                "INFRASTRUCTURE_ERROR",
                &error,
                trace_id_from_body(body),
            ),
        }
    }

    fn is_service_authorized(&self, headers: &[Header]) -> bool {
        header_value(headers, HEADER_AUTHORIZATION)
            .and_then(|value| parse_bearer_token(&value))
            .is_some_and(|value| value == self.service_token)
    }
}

fn main() {
    let cli = Cli::parse();
    let service_token = resolve_service_token(cli.service_token);

    let registry_store = DeviceRegistryStore::new(cli.device_registry.clone());
    let admin_store = AdminConsoleStore::new(cli.admin_state.clone(), registry_store);
    let conversation_store = TaskConversationStore::new(cli.conversations.clone());
    let service = TaskApiService::new(admin_store, conversation_store);
    let api = TaskApiHttpServer::new(service, service_token);

    let server = Server::http(&cli.bind).unwrap_or_else(|error| {
        panic!("failed to bind assistant task api on {}: {error}", cli.bind);
    });
    println!(
        "assistant-task-api listening on http://{} (contract {}, bearer token required)",
        cli.bind, CONTRACT_VERSION
    );

    for request in server.incoming_requests() {
        api.handle(request);
    }
}

fn resolve_service_token(cli_token: Option<String>) -> String {
    cli_token
        .or_else(|| env::var(SERVICE_TOKEN_ENV).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            eprintln!(
                "assistant-task-api requires a bearer token via --service-token or {SERVICE_TOKEN_ENV}"
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

fn parse_json_body<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|error| format!("invalid JSON body: {error}"))
}

fn trace_id_from_body(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/trace_id")
                .and_then(Value::as_str)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn header_value(headers: &[Header], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.field.as_str().to_string().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_bearer_token(value: &str) -> Option<String> {
    let prefix = "bearer ";
    value
        .trim()
        .to_ascii_lowercase()
        .starts_with(prefix)
        .then(|| value.trim()[prefix.len()..].trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_task_request_contract(request: &TaskRequest) -> Result<(), String> {
    if !request
        .source
        .surface
        .trim()
        .eq_ignore_ascii_case("im_gateway")
    {
        return Ok(());
    }

    for (field, value) in [
        ("task_id", request.task_id.trim()),
        ("trace_id", request.trace_id.trim()),
        ("source.channel", request.source.channel.trim()),
        ("source.surface", request.source.surface.trim()),
        (
            "source.conversation_id",
            request.source.conversation_id.trim(),
        ),
        ("source.user_id", request.source.user_id.trim()),
        ("source.route_key", request.source.route_key.trim()),
        ("intent.domain", request.intent.domain.trim()),
        ("intent.action", request.intent.action.trim()),
    ] {
        if value.is_empty() {
            return Err(format!(
                "missing required field for IM Gateway caller: {field}"
            ));
        }
    }

    let Some(message) = request.message.as_ref() else {
        return Err("missing required field for IM Gateway caller: message".to_string());
    };

    match message.chat_type.trim() {
        "p2p" | "group" | "channel" | "unknown" => Ok(()),
        "" => Err("missing required field for IM Gateway caller: message.chat_type".to_string()),
        other => Err(format!(
            "invalid message.chat_type for IM Gateway caller: {other}"
        )),
    }
}

fn ok_json(payload: &impl Serialize) -> Response<Cursor<Vec<u8>>> {
    json_response(StatusCode(200), payload)
}

fn no_content() -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(StatusCode(204));
    add_common_headers(&mut response);
    response
}

fn service_auth_failed(trace_id: Option<String>) -> Response<Cursor<Vec<u8>>> {
    let mut response = shared_error_json(
        StatusCode(401),
        "SERVICE_AUTH_FAILED",
        "missing or invalid bearer token",
        trace_id,
    );
    response.add_header(
        Header::from_bytes(b"WWW-Authenticate".as_slice(), b"Bearer".as_slice()).expect("header"),
    );
    response
}

fn shared_error_json(
    status: StatusCode,
    code: &'static str,
    message: &str,
    trace_id: Option<String>,
) -> Response<Cursor<Vec<u8>>> {
    json_response(
        status,
        &SharedHttpErrorEnvelope {
            ok: false,
            error: SharedHttpErrorDetail {
                code,
                message: message.to_string(),
            },
            trace_id,
        },
    )
}

fn json_response(status: StatusCode, payload: &impl Serialize) -> Response<Cursor<Vec<u8>>> {
    let body = serde_json::to_vec_pretty(payload).unwrap_or_else(|_| {
        serde_json::to_vec(&json!({
            "ok": false,
            "error": {
                "code": "INFRASTRUCTURE_ERROR",
                "message": "serialize failed"
            }
        }))
        .unwrap_or_else(|_| b"{\"ok\":false}".to_vec())
    });
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
        (
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, X-Contract-Version",
        ),
        ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        ("Access-Control-Expose-Headers", "X-Contract-Version"),
        ("Cache-Control", "no-store"),
        ("X-Contract-Version", CONTRACT_VERSION),
    ] {
        response.add_header(
            Header::from_bytes(header.0.as_bytes(), header.1.as_bytes()).expect("header"),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, Read};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{json, Value};
    use tiny_http::{Header, StatusCode};

    use super::{
        header_value, parse_bearer_token, TaskApiHttpServer, HEADER_AUTHORIZATION,
        HEADER_CONTRACT_VERSION,
    };
    use harbornas_local_agent::runtime::admin_console::AdminConsoleStore;
    use harbornas_local_agent::runtime::registry::DeviceRegistryStore;
    use harbornas_local_agent::runtime::task_api::TaskApiService;
    use harbornas_local_agent::runtime::task_session::TaskConversationStore;

    fn unique_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}.json"))
    }

    fn build_server(token: &str) -> (TaskApiHttpServer, Vec<std::path::PathBuf>) {
        let admin_path = unique_path("assistant-task-api-admin");
        let registry_path = unique_path("assistant-task-api-registry");
        let conversation_path = unique_path("assistant-task-api-conversations");
        let service = TaskApiService::new(
            AdminConsoleStore::new(
                admin_path.clone(),
                DeviceRegistryStore::new(registry_path.clone()),
            ),
            TaskConversationStore::new(conversation_path.clone()),
        );
        (
            TaskApiHttpServer::new(service, token.to_string()),
            vec![admin_path, registry_path, conversation_path],
        )
    }

    fn header(name: &str, value: &str) -> Header {
        Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("header")
    }

    fn response_json(
        response: tiny_http::Response<Cursor<Vec<u8>>>,
    ) -> (StatusCode, Value, Vec<Header>) {
        let status = response.status_code();
        let headers = response.headers().to_vec();
        let mut reader = response.into_reader();
        let mut body = String::new();
        reader
            .read_to_string(&mut body)
            .expect("read response body");
        let payload = serde_json::from_str(&body).expect("parse response body json");
        (status, payload, headers)
    }

    fn cleanup(paths: Vec<std::path::PathBuf>) {
        for path in paths {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn bearer_parser_requires_bearer_prefix() {
        assert_eq!(
            parse_bearer_token("Bearer token-1"),
            Some("token-1".to_string())
        );
        assert_eq!(
            parse_bearer_token("bearer token-2"),
            Some("token-2".to_string())
        );
        assert_eq!(parse_bearer_token("token-3"), None);
    }

    #[test]
    fn task_endpoint_rejects_missing_auth() {
        let (server, paths) = build_server("shared-token");
        let (status, payload, headers) = response_json(server.handle_task_payload(
            &[header(HEADER_CONTRACT_VERSION, "1.5")],
            br#"{"trace_id":"trace-auth"}"#,
        ));

        assert_eq!(status.0, 401);
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["error"]["code"], "SERVICE_AUTH_FAILED");
        assert_eq!(payload["trace_id"], "trace-auth");
        assert_eq!(
            header_value(&headers, "WWW-Authenticate"),
            Some("Bearer".to_string())
        );
        cleanup(paths);
    }

    #[test]
    fn task_endpoint_rejects_contract_version_mismatch() {
        let (server, paths) = build_server("shared-token");
        let headers = [
            header(HEADER_AUTHORIZATION, "Bearer shared-token"),
            header(HEADER_CONTRACT_VERSION, "1.4"),
        ];
        let (status, payload, response_headers) =
            response_json(server.handle_task_payload(&headers, br#"{"trace_id":"trace-version"}"#));

        assert_eq!(status.0, 400);
        assert_eq!(payload["error"]["code"], "CONTRACT_VERSION_MISMATCH");
        assert_eq!(payload["trace_id"], "trace-version");
        assert_eq!(
            header_value(&response_headers, HEADER_CONTRACT_VERSION),
            Some("1.5".to_string())
        );
        cleanup(paths);
    }

    #[test]
    fn task_endpoint_rejects_invalid_json_with_validation_error() {
        let (server, paths) = build_server("shared-token");
        let headers = [
            header(HEADER_AUTHORIZATION, "Bearer shared-token"),
            header(HEADER_CONTRACT_VERSION, "1.5"),
        ];
        let (status, payload, _) = response_json(server.handle_task_payload(&headers, br#"{"#));

        assert_eq!(status.0, 422);
        assert_eq!(payload["error"]["code"], "VALIDATION_ERROR");
        cleanup(paths);
    }

    #[test]
    fn task_endpoint_rejects_im_gateway_request_without_message_block() {
        let (server, paths) = build_server("shared-token");
        let headers = [
            header(HEADER_AUTHORIZATION, "Bearer shared-token"),
            header(HEADER_CONTRACT_VERSION, "1.5"),
        ];
        let body = serde_json::to_vec(&json!({
            "task_id": "task-http-no-message",
            "trace_id": "trace-http-no-message",
            "step_id": "step-http-no-message",
            "source": {
                "channel": "im_bridge",
                "surface": "im_gateway",
                "conversation_id": "chat-http-no-message",
                "user_id": "user-1",
                "session_id": "sess-http-no-message",
                "route_key": "gw_route_http_no_message"
            },
            "intent": {
                "domain": "system",
                "action": "ping",
                "raw_text": "ping"
            },
            "entity_refs": {},
            "args": {},
            "autonomy": {
                "level": "supervised"
            }
        }))
        .expect("encode request");
        let (status, payload, _) = response_json(server.handle_task_payload(&headers, &body));

        assert_eq!(status.0, 422);
        assert_eq!(payload["error"]["code"], "VALIDATION_ERROR");
        assert!(payload["error"]["message"]
            .as_str()
            .is_some_and(|value| value.contains("message")));
        cleanup(paths);
    }

    #[test]
    fn task_endpoint_returns_business_response_when_headers_are_valid() {
        let (server, paths) = build_server("shared-token");
        let headers = [
            header(HEADER_AUTHORIZATION, "Bearer shared-token"),
            header(HEADER_CONTRACT_VERSION, "1.5"),
        ];
        let body = json!({
            "task_id": "task-http-ok",
            "trace_id": "trace-http-ok",
            "step_id": "step-http-ok",
            "source": {
                "channel": "im_bridge",
                "surface": "im_gateway",
                "conversation_id": "chat-http-ok",
                "user_id": "user-1",
                "session_id": "sess-http-ok",
                "route_key": "gw_route_http_ok"
            },
            "intent": {
                "domain": "system",
                "action": "ping",
                "raw_text": "ping"
            },
            "entity_refs": {},
            "args": {},
            "autonomy": {
                "level": "supervised"
            },
            "message": {
                "message_id": "om_http_ok",
                "chat_type": "group",
                "mentions": [],
                "attachments": []
            }
        });
        let encoded = serde_json::to_vec(&body).expect("encode request");
        let (status, payload, response_headers) =
            response_json(server.handle_task_payload(&headers, &encoded));

        assert_eq!(status.0, 200);
        assert_eq!(payload["task_id"], "task-http-ok");
        assert_eq!(payload["trace_id"], "trace-http-ok");
        assert_eq!(payload["status"], "failed");
        assert_eq!(
            header_value(&response_headers, HEADER_CONTRACT_VERSION),
            Some("1.5".to_string())
        );
        cleanup(paths);
    }

    #[test]
    fn task_endpoint_rejects_conflicting_reuse_of_task_id() {
        let (server, paths) = build_server("shared-token");
        let headers = [
            header(HEADER_AUTHORIZATION, "Bearer shared-token"),
            header(HEADER_CONTRACT_VERSION, "1.5"),
        ];
        let initial = serde_json::to_vec(&json!({
            "task_id": "task-http-conflict",
            "trace_id": "trace-http-conflict",
            "step_id": "step-http-conflict",
            "source": {
                "channel": "im_bridge",
                "surface": "im_gateway",
                "conversation_id": "chat-http-conflict",
                "user_id": "user-1",
                "session_id": "sess-http-conflict",
                "route_key": "gw_route_http_conflict"
            },
            "intent": {
                "domain": "system",
                "action": "ping",
                "raw_text": "ping"
            },
            "entity_refs": {},
            "args": {},
            "autonomy": {
                "level": "supervised"
            },
            "message": {
                "message_id": "om_http_conflict",
                "chat_type": "group",
                "mentions": [],
                "attachments": []
            }
        }))
        .expect("encode initial request");
        let conflicting = serde_json::to_vec(&json!({
            "task_id": "task-http-conflict",
            "trace_id": "trace-http-conflict",
            "step_id": "step-http-conflict",
            "source": {
                "channel": "im_bridge",
                "surface": "im_gateway",
                "conversation_id": "chat-http-conflict",
                "user_id": "user-1",
                "session_id": "sess-http-conflict",
                "route_key": "gw_route_http_conflict"
            },
            "intent": {
                "domain": "system",
                "action": "ping",
                "raw_text": "ping again"
            },
            "entity_refs": {},
            "args": {},
            "autonomy": {
                "level": "supervised"
            },
            "message": {
                "message_id": "om_http_conflict",
                "chat_type": "group",
                "mentions": [],
                "attachments": []
            }
        }))
        .expect("encode conflicting request");

        let first = response_json(server.handle_task_payload(&headers, &initial));
        assert_eq!(first.0 .0, 200);

        let (status, payload, _) =
            response_json(server.handle_task_payload(&headers, &conflicting));
        assert_eq!(status.0, 409);
        assert_eq!(payload["error"]["code"], "IDEMPOTENCY_CONFLICT");

        cleanup(paths);
    }
}
