use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use glob::glob;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("capability unavailable: {0}")]
    CapabilityUnavailable(String),
    #[error("approval required: {0}")]
    ApprovalRequired(String),
    #[error("path policy violation: {0}")]
    PathPolicy(String),
    #[error("command failed: {message}")]
    CommandExecution {
        message: String,
        argv: Vec<String>,
        stdout: String,
        stderr: String,
        returncode: i32,
    },
    #[error("integration error: {0}")]
    Generic(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub argv: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub returncode: i32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct IntegrationConfig {
    pub middleware_bin: String,
    pub middleware_timeout: u64,
    pub midcli_bin: String,
    pub midcli_mode: String,
    pub midcli_timeout: u64,
    pub midcli_url: Option<String>,
    pub midcli_user: Option<String>,
    pub midcli_password: Option<String>,
    pub probe_service: String,
    pub filesystem_path: String,
    pub midcli_service_query_command: Option<String>,
    pub midcli_filesystem_command: Option<String>,
    pub harbor_repo_path: Option<String>,
    pub upstream_repo_path: Option<String>,
    pub allow_mutations: bool,
    pub approval_token: Option<String>,
    pub required_approval_token: Option<String>,
    pub approver_id: Option<String>,
    pub mutation_root: String,
    pub rollback_on_failure: bool,
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            middleware_bin: "midclt".to_string(),
            middleware_timeout: 1200,
            midcli_bin: "cli".to_string(),
            midcli_mode: "csv".to_string(),
            midcli_timeout: 1200,
            midcli_url: None,
            midcli_user: None,
            midcli_password: None,
            probe_service: "ssh".to_string(),
            filesystem_path: "/mnt".to_string(),
            midcli_service_query_command: None,
            midcli_filesystem_command: None,
            harbor_repo_path: None,
            upstream_repo_path: None,
            allow_mutations: false,
            approval_token: None,
            required_approval_token: None,
            approver_id: None,
            mutation_root: "/mnt/agent-ci".to_string(),
            rollback_on_failure: true,
        }
    }
}

impl IntegrationConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        config.middleware_bin = env_or("HARBOR_MIDDLEWARE_BIN", &config.middleware_bin);
        config.middleware_timeout = env_or("HARBOR_MIDDLEWARE_TIMEOUT", "1200")
            .parse::<u64>()
            .unwrap_or(1200);
        config.midcli_bin = env_or("HARBOR_MIDCLI_BIN", &config.midcli_bin);
        config.midcli_mode = env_or("HARBOR_MIDCLI_MODE", &config.midcli_mode);
        config.midcli_timeout = env_or("HARBOR_MIDCLI_TIMEOUT", "1200")
            .parse::<u64>()
            .unwrap_or(1200);
        config.midcli_url = std::env::var("HARBOR_MIDCLI_URL").ok();
        config.midcli_user = std::env::var("HARBOR_MIDCLI_USER").ok();
        config.midcli_password = std::env::var("HARBOR_MIDCLI_PASSWORD").ok();
        config.probe_service = env_or("HARBOR_PROBE_SERVICE", &config.probe_service);
        config.filesystem_path = env_or("HARBOR_FILESYSTEM_PATH", &config.filesystem_path);
        config.midcli_service_query_command = std::env::var("HARBOR_MIDCLI_SERVICE_QUERY_COMMAND").ok();
        config.midcli_filesystem_command = std::env::var("HARBOR_MIDCLI_FILESYSTEM_COMMAND").ok();
        config.harbor_repo_path = std::env::var("HARBOR_SOURCE_REPO_PATH").ok();
        config.upstream_repo_path = std::env::var("UPSTREAM_SOURCE_REPO_PATH").ok();
        config.allow_mutations = env_to_bool(std::env::var("HARBOR_ALLOW_MUTATIONS").ok());
        config.approval_token = std::env::var("HARBOR_APPROVAL_TOKEN").ok();
        config.required_approval_token = std::env::var("HARBOR_REQUIRED_APPROVAL_TOKEN").ok();
        config.approver_id = std::env::var("HARBOR_APPROVER_ID").ok();
        config.mutation_root = env_or("HARBOR_MUTATION_ROOT", &config.mutation_root);
        config.rollback_on_failure = env_to_bool(std::env::var("HARBOR_ROLLBACK_ON_FAILURE").ok());
        config
    }
}

fn env_or(name: &str, default_value: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default_value.to_string())
}

pub fn env_to_bool(value: Option<String>) -> bool {
    let Some(v) = value else {
        return false;
    };
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

pub fn parse_loose_value(text: &str) -> Value {
    let stripped = text.trim();
    if stripped.is_empty() {
        return Value::Null;
    }
    serde_json::from_str::<Value>(stripped).unwrap_or_else(|_| Value::String(stripped.to_string()))
}

pub fn parse_csv_rows(text: &str) -> Vec<HashMap<String, String>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut lines = trimmed.lines();
    let Some(header_line) = lines.next() else {
        return Vec::new();
    };

    let headers: Vec<String> = header_line.split(',').map(|s| s.trim().to_string()).collect();
    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut map = HashMap::new();
            for (idx, value) in line.split(',').enumerate() {
                if let Some(key) = headers.get(idx) {
                    map.insert(key.clone(), value.trim().to_string());
                }
            }
            map
        })
        .collect()
}

pub fn command_exists(name: &str) -> bool {
    which::which(name).is_ok()
}

pub fn run_command(argv: &[String], timeout_ms: u64) -> Result<CommandResult, IntegrationError> {
    let started = Instant::now();
    let mut cmd = Command::new(
        argv.first()
            .ok_or_else(|| IntegrationError::Generic("empty argv".to_string()))?,
    );
    cmd.args(&argv[1..]);

    let output = cmd.output().map_err(|e| IntegrationError::Generic(e.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;

    let result = CommandResult {
        argv: argv.to_vec(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        returncode: output.status.code().unwrap_or(1),
        duration_ms,
    };

    if duration_ms > timeout_ms {
        return Err(IntegrationError::Generic("command timeout exceeded".to_string()));
    }

    Ok(result)
}

pub fn normalize_path(path: &str) -> String {
    let p = Path::new(path);
    p.canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .to_string()
}

fn ensure_service_name(service_name: &str) -> Result<(), IntegrationError> {
    let valid = !service_name.is_empty()
        && service_name.len() <= 64
        && service_name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-');

    if !valid {
        return Err(IntegrationError::Generic(format!("invalid service name: {service_name:?}")));
    }
    Ok(())
}

pub fn ensure_approved(
    risk_level: &str,
    config: &IntegrationConfig,
    approval_token: Option<&str>,
    action_name: &str,
) -> Result<(), IntegrationError> {
    if !matches!(risk_level, "HIGH" | "CRITICAL") {
        return Ok(());
    }

    let Some(token) = approval_token else {
        return Err(IntegrationError::ApprovalRequired(format!(
            "{action_name} requires an approval token"
        )));
    };

    if let Some(required) = &config.required_approval_token {
        if token != required {
            return Err(IntegrationError::ApprovalRequired(format!(
                "{action_name} approval token did not match the required token"
            )));
        }
    }

    Ok(())
}

const ALLOWED_READ_ROOTS: [&str; 2] = ["/mnt", "/data"];
const ALLOWED_WRITE_ROOTS: [&str; 3] = ["/mnt", "/data", "/tmp/agent"];
const DENIED_ROOTS: [&str; 5] = ["/", "/etc", "/boot", "/root", "/var/lib"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathPolicy {
    pub read_paths: Vec<String>,
    pub write_paths: Vec<String>,
}

pub fn validate_path_policy(
    read_paths: &[String],
    write_paths: &[String],
) -> Result<PathPolicy, IntegrationError> {
    let normalized_reads: Vec<String> = read_paths.iter().map(|p| normalize_path(p)).collect();
    let normalized_writes: Vec<String> = write_paths.iter().map(|p| normalize_path(p)).collect();

    for path in normalized_reads.iter().chain(normalized_writes.iter()) {
        if is_under_any(path, &DENIED_ROOTS) {
            return Err(IntegrationError::PathPolicy(format!("denied path: {path}")));
        }
    }

    for path in &normalized_reads {
        if !is_under_any(path, &ALLOWED_READ_ROOTS) {
            return Err(IntegrationError::PathPolicy(format!("read path outside allowlist: {path}")));
        }
    }

    for path in &normalized_writes {
        if !is_under_any(path, &ALLOWED_WRITE_ROOTS) {
            return Err(IntegrationError::PathPolicy(format!("write path outside allowlist: {path}")));
        }
    }

    Ok(PathPolicy {
        read_paths: normalized_reads,
        write_paths: normalized_writes,
    })
}

fn is_under_any(path: &str, roots: &[&str]) -> bool {
    roots.iter().any(|root| {
        if *root == "/" {
            path == "/"
        } else {
            path == *root || path.starts_with(&format!("{root}/"))
        }
    })
}

pub fn service_operation_risk(operation: &str) -> Result<&'static str, IntegrationError> {
    match operation {
        "status" => Ok("LOW"),
        "start" | "enable" => Ok("MEDIUM"),
        "stop" | "restart" => Ok("HIGH"),
        _ => Err(IntegrationError::Generic(format!("unsupported service operation: {operation}"))),
    }
}

pub fn file_operation_risk(operation: &str, overwrite: bool) -> Result<&'static str, IntegrationError> {
    match operation {
        "search" => Ok("LOW"),
        "copy" => {
            if overwrite {
                Ok("HIGH")
            } else {
                Ok("MEDIUM")
            }
        }
        "move" => Ok("HIGH"),
        "archive" => Ok("MEDIUM"),
        _ => Err(IntegrationError::Generic(format!("unsupported file operation: {operation}"))),
    }
}

pub fn build_service_preview(operation: &str, service_name: &str, executor: &str, risk_level: &str) -> Value {
    json!({
        "preview": true,
        "domain": "service",
        "operation": operation,
        "service_name": service_name,
        "executor": executor,
        "risk_level": risk_level,
    })
}

pub fn build_file_preview(
    operation: &str,
    src: &str,
    dst: &str,
    executor: &str,
    risk_level: &str,
    overwrite: bool,
) -> Value {
    json!({
        "preview": true,
        "domain": "files",
        "operation": operation,
        "src": src,
        "dst": dst,
        "executor": executor,
        "risk_level": risk_level,
        "overwrite": overwrite,
    })
}

pub fn ensure_mutation_fixture(root: &str, filename: &str, content: &str) -> Result<String, IntegrationError> {
    let root_path = PathBuf::from(normalize_path(root));
    fs::create_dir_all(&root_path).map_err(|e| IntegrationError::Generic(e.to_string()))?;
    let file_path = root_path.join(filename);
    fs::write(&file_path, content).map_err(|e| IntegrationError::Generic(e.to_string()))?;
    Ok(file_path.to_string_lossy().to_string())
}

pub fn ensure_directory(path: &str) -> Result<String, IntegrationError> {
    let normalized = normalize_path(path);
    fs::create_dir_all(&normalized).map_err(|e| IntegrationError::Generic(e.to_string()))?;
    Ok(normalized)
}

pub fn discover_source_capabilities(repo_path: Option<&str>) -> HashMap<String, bool> {
    let Some(repo_path) = repo_path else {
        return HashMap::new();
    };

    let root = PathBuf::from(repo_path);
    if !root.exists() {
        return HashMap::new();
    }

    let service_text = read_first_match(&root, "**/api/v*/service.py");
    let filesystem_text = read_first_match(&root, "**/api/v*/filesystem.py");
    let plugin_service_text = read_first_match(&root, "**/plugins/service.py");
    let plugin_filesystem_text = read_first_match(&root, "**/plugins/filesystem.py");

    let mut caps = HashMap::new();
    caps.insert(
        "service.query".to_string(),
        plugin_service_text.contains("query") && plugin_service_text.contains("class ServiceService"),
    );
    caps.insert(
        "service.control".to_string(),
        service_text.contains("ServiceControlArgs") && plugin_service_text.contains("def control"),
    );
    caps.insert(
        "filesystem.listdir".to_string(),
        filesystem_text.contains("FilesystemListdirArgs") && plugin_filesystem_text.contains("def listdir"),
    );
    caps.insert(
        "filesystem.copy".to_string(),
        filesystem_text.contains("FilesystemCopyArgs") && plugin_filesystem_text.contains("def copy"),
    );
    caps.insert(
        "filesystem.move".to_string(),
        filesystem_text.contains("FilesystemMoveArgs") && plugin_filesystem_text.contains("def move"),
    );
    caps
}

fn read_first_match(root: &Path, pattern: &str) -> String {
    let glob_pattern = format!("{}/{}", root.display(), pattern);
    let Ok(entries) = glob(&glob_pattern) else {
        return String::new();
    };

    for entry in entries.flatten() {
        if entry.is_file() {
            if let Ok(text) = fs::read_to_string(&entry) {
                return text;
            }
        }
    }
    String::new()
}

#[derive(Debug, Clone)]
pub struct MiddlewareClient {
    pub config: IntegrationConfig,
}

impl MiddlewareClient {
    pub fn new(config: IntegrationConfig) -> Self {
        Self { config }
    }

    pub fn is_available(&self) -> bool {
        command_exists(&self.config.middleware_bin)
    }

    pub fn build_call_argv(&self, method: &str, args: &[Value]) -> Vec<String> {
        let mut argv = vec![self.config.middleware_bin.clone(), "call".to_string(), method.to_string()];
        argv.extend(args.iter().map(|arg| serde_json::to_string(arg).unwrap_or_else(|_| "null".to_string())));
        argv
    }

    pub fn call(&self, method: &str, args: &[Value]) -> Result<(Value, CommandResult), IntegrationError> {
        if !self.is_available() {
            return Err(IntegrationError::CapabilityUnavailable(format!(
                "middleware command not found: {}",
                self.config.middleware_bin
            )));
        }

        let argv = self.build_call_argv(method, args);
        let result = run_command(&argv, self.config.middleware_timeout)?;
        if result.returncode != 0 {
            return Err(IntegrationError::CommandExecution {
                message: format!("middleware call failed for {method}"),
                argv: result.argv,
                stdout: result.stdout,
                stderr: result.stderr,
                returncode: result.returncode,
            });
        }

        Ok((parse_loose_value(&result.stdout), result))
    }

    pub fn get_methods(&self, target: &str) -> Result<(HashMap<String, Value>, CommandResult), IntegrationError> {
        let (payload, result) = self.call("core.get_methods", &[Value::Null, Value::String(target.to_string())])?;
        let map = payload
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<String, Value>>();
        Ok((map, result))
    }

    pub fn service_control(&self, operation: &str, service_name: &str) -> Result<(Value, CommandResult), IntegrationError> {
        self.call(
            "service.control",
            &[
                Value::String(operation.to_uppercase()),
                Value::String(service_name.to_string()),
                json!({}),
            ],
        )
    }

    pub fn filesystem_copy(
        &self,
        src: &str,
        dst: &str,
        recursive: bool,
        preserve_attrs: bool,
    ) -> Result<(Value, CommandResult), IntegrationError> {
        self.call(
            "filesystem.copy",
            &[
                Value::String(src.to_string()),
                Value::String(dst.to_string()),
                json!({"recursive": recursive, "preserve_attrs": preserve_attrs}),
            ],
        )
    }

    pub fn filesystem_move(&self, src: &str, dst_dir: &str, recursive: bool) -> Result<(Value, CommandResult), IntegrationError> {
        self.call(
            "filesystem.move",
            &[
                json!([src]),
                Value::String(dst_dir.to_string()),
                json!({"recursive": recursive}),
            ],
        )
    }
}

#[derive(Debug, Clone)]
pub struct MidcliClient {
    pub config: IntegrationConfig,
}

impl MidcliClient {
    pub fn new(config: IntegrationConfig) -> Self {
        Self { config }
    }

    pub fn is_available(&self) -> bool {
        command_exists(&self.config.midcli_bin)
    }

    pub fn build_run_argv(&self, command: &str, mode: Option<&str>, print_template: bool) -> Vec<String> {
        let mut argv = vec![self.config.midcli_bin.clone()];
        if let Some(url) = &self.config.midcli_url {
            argv.extend(["--url".to_string(), url.clone()]);
        }
        if let Some(user) = &self.config.midcli_user {
            argv.extend(["--user".to_string(), user.clone()]);
        }
        if let Some(password) = &self.config.midcli_password {
            argv.extend(["--password".to_string(), password.clone()]);
        }
        argv.extend([
            "-m".to_string(),
            mode.unwrap_or(&self.config.midcli_mode).to_string(),
        ]);
        if print_template {
            argv.push("--print-template".to_string());
        }
        argv.extend(["-c".to_string(), command.to_string()]);
        argv
    }

    pub fn run(&self, command: &str, mode: Option<&str>, print_template: bool) -> Result<CommandResult, IntegrationError> {
        if !self.is_available() {
            return Err(IntegrationError::CapabilityUnavailable(format!(
                "midcli command not found: {}",
                self.config.midcli_bin
            )));
        }

        let argv = self.build_run_argv(command, mode, print_template);
        let result = run_command(&argv, self.config.midcli_timeout)?;
        if result.returncode != 0 {
            return Err(IntegrationError::CommandExecution {
                message: format!("midcli command failed: {command}"),
                argv: result.argv,
                stdout: result.stdout,
                stderr: result.stderr,
                returncode: result.returncode,
            });
        }
        Ok(result)
    }

    pub fn run_csv_query(&self, command: &str) -> Result<(Vec<HashMap<String, String>>, CommandResult), IntegrationError> {
        let result = self.run(command, Some("csv"), false)?;
        Ok((parse_csv_rows(&result.stdout), result))
    }

    pub fn service_control(&self, operation: &str, service_name: &str) -> Result<CommandResult, IntegrationError> {
        self.run(&format!("service {operation} service={service_name}"), None, false)
    }

    pub fn filesystem_copy(&self, src: &str, dst: &str, recursive: bool) -> Result<CommandResult, IntegrationError> {
        let mut command = format!("filesystem copy src={} dst={}", json!(src), json!(dst));
        if recursive {
            command.push_str(" recursive=true");
        }
        self.run(&command, None, false)
    }

    pub fn filesystem_move(&self, src: &str, dst: &str, recursive: bool) -> Result<CommandResult, IntegrationError> {
        let mut command = format!("filesystem move src={} dst={}", json!(src), json!(dst));
        if recursive {
            command.push_str(" recursive=true");
        }
        self.run(&command, None, false)
    }
}

pub fn default_midcli_service_query(config: &IntegrationConfig) -> String {
    config
        .midcli_service_query_command
        .clone()
        .unwrap_or_else(|| format!("service query service,state,enable WHERE service == '{}'", config.probe_service))
}

pub fn default_midcli_filesystem_command(config: &IntegrationConfig) -> String {
    config
        .midcli_filesystem_command
        .clone()
        .unwrap_or_else(|| format!("filesystem listdir path={}", config.filesystem_path))
}

pub fn execute_service_action(
    middleware: &MiddlewareClient,
    midcli: &MidcliClient,
    config: &IntegrationConfig,
    operation: &str,
    service_name: &str,
    prefer_midcli: bool,
    dry_run: bool,
    approval_token: Option<&str>,
) -> Result<Value, IntegrationError> {
    ensure_service_name(service_name)?;
    let risk_level = service_operation_risk(operation)?;
    let executor = if prefer_midcli { "midcli" } else { "middleware_api" };

    if dry_run {
        return Ok(build_service_preview(operation, service_name, executor, risk_level));
    }

    ensure_approved(risk_level, config, approval_token, &format!("service.{operation}"))?;

    if !prefer_midcli && middleware.is_available() {
        let (payload, result) = middleware.service_control(operation, service_name)?;
        return Ok(json!({
            "preview": false,
            "executor": "middleware_api",
            "operation": operation,
            "service_name": service_name,
            "risk_level": risk_level,
            "duration_ms": result.duration_ms,
            "result": payload,
            "approver_id": config.approver_id,
        }));
    }

    if midcli.is_available() {
        let result = midcli.service_control(operation, service_name)?;
        return Ok(json!({
            "preview": false,
            "executor": "midcli",
            "operation": operation,
            "service_name": service_name,
            "risk_level": risk_level,
            "duration_ms": result.duration_ms,
            "result": parse_loose_value(&result.stdout),
            "approver_id": config.approver_id,
        }));
    }

    Err(IntegrationError::CapabilityUnavailable(
        "neither middleware nor midcli is available for service control".to_string(),
    ))
}

pub fn execute_file_action(
    middleware: &MiddlewareClient,
    midcli: &MidcliClient,
    config: &IntegrationConfig,
    operation: &str,
    src: &str,
    dst: &str,
    recursive: bool,
    overwrite: bool,
    prefer_midcli: bool,
    dry_run: bool,
    approval_token: Option<&str>,
) -> Result<Value, IntegrationError> {
    let policy = validate_path_policy(&[src.to_string()], &[dst.to_string()])?;
    let src_normalized = policy.read_paths[0].clone();
    let dst_normalized = policy.write_paths[0].clone();
    let risk_level = file_operation_risk(operation, overwrite)?;
    let executor = if prefer_midcli { "midcli" } else { "middleware_api" };

    if dry_run {
        return Ok(build_file_preview(
            operation,
            &src_normalized,
            &dst_normalized,
            executor,
            risk_level,
            overwrite,
        ));
    }

    ensure_approved(risk_level, config, approval_token, &format!("files.{operation}"))?;

    match operation {
        "copy" => {
            if !prefer_midcli && middleware.is_available() {
                let (payload, result) = middleware.filesystem_copy(&src_normalized, &dst_normalized, recursive, false)?;
                return Ok(json!({
                    "preview": false,
                    "executor": "middleware_api",
                    "operation": operation,
                    "src": src_normalized,
                    "dst": dst_normalized,
                    "risk_level": risk_level,
                    "duration_ms": result.duration_ms,
                    "result": payload,
                    "approver_id": config.approver_id,
                }));
            }
            if midcli.is_available() {
                let result = midcli.filesystem_copy(&src_normalized, &dst_normalized, recursive)?;
                return Ok(json!({
                    "preview": false,
                    "executor": "midcli",
                    "operation": operation,
                    "src": src_normalized,
                    "dst": dst_normalized,
                    "risk_level": risk_level,
                    "duration_ms": result.duration_ms,
                    "result": parse_loose_value(&result.stdout),
                    "approver_id": config.approver_id,
                }));
            }
        }
        "move" => {
            if !prefer_midcli && middleware.is_available() {
                let (payload, result) = middleware.filesystem_move(&src_normalized, &dst_normalized, recursive)?;
                return Ok(json!({
                    "preview": false,
                    "executor": "middleware_api",
                    "operation": operation,
                    "src": src_normalized,
                    "dst": dst_normalized,
                    "risk_level": risk_level,
                    "duration_ms": result.duration_ms,
                    "result": payload,
                    "approver_id": config.approver_id,
                }));
            }
            if midcli.is_available() {
                let result = midcli.filesystem_move(&src_normalized, &dst_normalized, recursive)?;
                return Ok(json!({
                    "preview": false,
                    "executor": "midcli",
                    "operation": operation,
                    "src": src_normalized,
                    "dst": dst_normalized,
                    "risk_level": risk_level,
                    "duration_ms": result.duration_ms,
                    "result": parse_loose_value(&result.stdout),
                    "approver_id": config.approver_id,
                }));
            }
        }
        _ => {
            return Err(IntegrationError::Generic(format!(
                "unsupported file operation: {operation}"
            )));
        }
    }

    Err(IntegrationError::CapabilityUnavailable(format!(
        "no available executor for files.{operation}"
    )))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use serde_json::Value;

    use super::{
        discover_source_capabilities, execute_file_action, execute_service_action, parse_csv_rows,
        IntegrationConfig, MiddlewareClient, MidcliClient,
    };

    #[test]
    fn parse_csv_rows_returns_structured_rows() {
        let rows = parse_csv_rows("service,state\nssh,RUNNING\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["service"], "ssh");
        assert_eq!(rows[0]["state"], "RUNNING");
    }

    #[test]
    fn middleware_client_builds_midclt_call() {
        let config = IntegrationConfig::default();
        let client = MiddlewareClient::new(config);
        let argv = client.build_call_argv("core.get_methods", &[Value::Null, Value::String("REST".to_string())]);
        assert_eq!(argv, vec!["midclt", "call", "core.get_methods", "null", "\"REST\""]);
    }

    #[test]
    fn midcli_client_builds_noninteractive_command() {
        let mut config = IntegrationConfig::default();
        config.midcli_url = Some("ws://nas/websocket".to_string());
        config.midcli_user = Some("root".to_string());
        config.midcli_password = Some("secret".to_string());

        let client = MidcliClient::new(config);
        let argv = client.build_run_argv("service query service,state WHERE service == 'ssh'", Some("csv"), false);
        assert_eq!(
            argv,
            vec![
                "cli",
                "--url",
                "ws://nas/websocket",
                "--user",
                "root",
                "--password",
                "secret",
                "-m",
                "csv",
                "-c",
                "service query service,state WHERE service == 'ssh'"
            ]
        );
    }

    #[test]
    fn discover_source_capabilities_reads_repo_files() {
        let temp_root = std::env::temp_dir().join(format!("harbor-caps-{}", uuid::Uuid::new_v4()));
        let service_api = temp_root.join("src/middlewared/middlewared/api/v27_0_0/service.py");
        let filesystem_api = temp_root.join("src/middlewared/middlewared/api/v27_0_0/filesystem.py");
        let service_plugin = temp_root.join("src/middlewared/middlewared/plugins/service.py");
        let filesystem_plugin = temp_root.join("src/middlewared/middlewared/plugins/filesystem.py");

        fs::create_dir_all(service_api.parent().unwrap()).unwrap();
        fs::create_dir_all(filesystem_api.parent().unwrap()).unwrap();
        fs::create_dir_all(service_plugin.parent().unwrap()).unwrap();
        fs::create_dir_all(filesystem_plugin.parent().unwrap()).unwrap();

        fs::write(&service_api, "class ServiceControlArgs: pass\n").unwrap();
        fs::write(&filesystem_api, "FilesystemListdirArgs\nFilesystemCopyArgs\nFilesystemMoveArgs\n").unwrap();
        fs::write(&service_plugin, "class ServiceService:\n    def control(self):\n        pass\n    def query(self):\n        pass\n").unwrap();
        fs::write(&filesystem_plugin, "def listdir(self):\n    pass\ndef copy(self):\n    pass\ndef move(self):\n    pass\n").unwrap();

        let caps = discover_source_capabilities(Some(temp_root.to_string_lossy().as_ref()));
        assert_eq!(caps.get("service.query"), Some(&true));
        assert_eq!(caps.get("service.control"), Some(&true));
        assert_eq!(caps.get("filesystem.listdir"), Some(&true));
        assert_eq!(caps.get("filesystem.copy"), Some(&true));
        assert_eq!(caps.get("filesystem.move"), Some(&true));

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn execute_service_action_requires_approval_for_restart() {
        let mut config = IntegrationConfig::default();
        config.required_approval_token = Some("approved".to_string());
        let middleware = MiddlewareClient::new(config.clone());
        let midcli = MidcliClient::new(config.clone());

        let result = execute_service_action(
            &middleware,
            &midcli,
            &config,
            "restart",
            "ssh",
            false,
            false,
            None,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("approval"));
    }

    #[test]
    fn execute_file_action_blocks_denied_path() {
        let config = IntegrationConfig::default();
        let middleware = MiddlewareClient::new(config.clone());
        let midcli = MidcliClient::new(config.clone());

        let result = execute_file_action(
            &middleware,
            &midcli,
            &config,
            "copy",
            "/etc/passwd",
            "/mnt/agent-ci/out.txt",
            false,
            false,
            false,
            true,
            None,
        );

        assert!(result.is_err());
        let err_text = result.unwrap_err().to_string();
        assert!(err_text.contains("denied path") || err_text.contains("outside allowlist"));
    }

    #[test]
    fn execute_file_action_dry_run_returns_preview() {
        let config = IntegrationConfig::default();
        let middleware = MiddlewareClient::new(config.clone());
        let midcli = MidcliClient::new(config.clone());

        let result = execute_file_action(
            &middleware,
            &midcli,
            &config,
            "move",
            "/mnt/agent-ci/source.txt",
            "/mnt/agent-ci/dst",
            false,
            false,
            false,
            true,
            None,
        )
        .unwrap();

        assert_eq!(result["preview"], true);
        assert_eq!(result["risk_level"], "HIGH");
    }
}
