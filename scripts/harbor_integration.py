from __future__ import annotations

import ast
import csv
import io
import json
import os
import re
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


class IntegrationError(RuntimeError):
    pass


class CapabilityUnavailableError(IntegrationError):
    pass


class ApprovalRequiredError(IntegrationError):
    pass


class PathPolicyError(IntegrationError):
    pass


class CommandExecutionError(IntegrationError):
    def __init__(self, message: str, *, argv: list[str], stdout: str, stderr: str, returncode: int):
        super().__init__(message)
        self.argv = argv
        self.stdout = stdout
        self.stderr = stderr
        self.returncode = returncode


@dataclass
class CommandResult:
    argv: list[str]
    stdout: str
    stderr: str
    returncode: int
    duration_ms: int


@dataclass
class IntegrationConfig:
    middleware_bin: str = "midclt"
    middleware_timeout: int = 1200
    midcli_bin: str = "cli"
    midcli_mode: str = "csv"
    midcli_timeout: int = 1200
    midcli_url: str | None = None
    midcli_user: str | None = None
    midcli_password: str | None = None
    probe_service: str = "ssh"
    filesystem_path: str = "/mnt"
    midcli_service_query_command: str | None = None
    midcli_filesystem_command: str | None = None
    harbor_repo_path: str | None = None
    upstream_repo_path: str | None = None
    allow_mutations: bool = False
    approval_token: str | None = None
    required_approval_token: str | None = None
    approver_id: str | None = None
    mutation_root: str = "/mnt/agent-ci"
    rollback_on_failure: bool = True

    @classmethod
    def from_env(cls) -> "IntegrationConfig":
        return cls(
            middleware_bin=os.getenv("HARBOR_MIDDLEWARE_BIN", "midclt"),
            middleware_timeout=int(os.getenv("HARBOR_MIDDLEWARE_TIMEOUT", "1200")),
            midcli_bin=os.getenv("HARBOR_MIDCLI_BIN", "cli"),
            midcli_mode=os.getenv("HARBOR_MIDCLI_MODE", "csv"),
            midcli_timeout=int(os.getenv("HARBOR_MIDCLI_TIMEOUT", "1200")),
            midcli_url=os.getenv("HARBOR_MIDCLI_URL"),
            midcli_user=os.getenv("HARBOR_MIDCLI_USER"),
            midcli_password=os.getenv("HARBOR_MIDCLI_PASSWORD"),
            probe_service=os.getenv("HARBOR_PROBE_SERVICE", "ssh"),
            filesystem_path=os.getenv("HARBOR_FILESYSTEM_PATH", "/mnt"),
            midcli_service_query_command=os.getenv("HARBOR_MIDCLI_SERVICE_QUERY_COMMAND"),
            midcli_filesystem_command=os.getenv("HARBOR_MIDCLI_FILESYSTEM_COMMAND"),
            harbor_repo_path=os.getenv("HARBOR_SOURCE_REPO_PATH"),
            upstream_repo_path=os.getenv("UPSTREAM_SOURCE_REPO_PATH"),
            allow_mutations=env_to_bool(os.getenv("HARBOR_ALLOW_MUTATIONS", "0")),
            approval_token=os.getenv("HARBOR_APPROVAL_TOKEN"),
            required_approval_token=os.getenv("HARBOR_REQUIRED_APPROVAL_TOKEN"),
            approver_id=os.getenv("HARBOR_APPROVER_ID"),
            mutation_root=os.getenv("HARBOR_MUTATION_ROOT", "/mnt/agent-ci"),
            rollback_on_failure=env_to_bool(os.getenv("HARBOR_ROLLBACK_ON_FAILURE", "1")),
        )


def env_to_bool(value: str | None) -> bool:
    if value is None:
        return False
    return value.strip().lower() in {"1", "true", "yes", "on"}


def command_exists(name: str) -> bool:
    return shutil.which(name) is not None


def run_command(argv: list[str], *, timeout: int) -> CommandResult:
    started_at = time.monotonic()
    completed = subprocess.run(argv, capture_output=True, text=True, timeout=timeout, check=False)
    duration_ms = int((time.monotonic() - started_at) * 1000)
    return CommandResult(
        argv=argv,
        stdout=completed.stdout,
        stderr=completed.stderr,
        returncode=completed.returncode,
        duration_ms=duration_ms,
    )


def parse_loose_value(text: str) -> Any:
    stripped = text.strip()
    if not stripped:
        return None

    for parser in (json.loads, ast.literal_eval):
        try:
            return parser(stripped)
        except Exception:
            continue

    return stripped


def parse_csv_rows(text: str) -> list[dict[str, str]]:
    stripped = text.strip()
    if not stripped:
        return []

    reader = csv.DictReader(io.StringIO(stripped))
    return [dict(row) for row in reader]


SERVICE_NAME_RE = re.compile(r"^[a-z0-9_-]{1,64}$")
ALLOWED_READ_ROOTS = ("/mnt", "/data")
ALLOWED_WRITE_ROOTS = ("/mnt", "/data", "/tmp/agent")
DENIED_ROOTS = ("/", "/etc", "/boot", "/root", "/var/lib")


def normalize_path(path: str) -> str:
    return str(Path(path).resolve(strict=False))


def ensure_service_name(service_name: str) -> None:
    if not SERVICE_NAME_RE.match(service_name):
        raise IntegrationError(f"invalid service name: {service_name!r}")


def ensure_approved(risk_level: str, config: IntegrationConfig, *, approval_token: str | None, action_name: str) -> None:
    if risk_level not in {"HIGH", "CRITICAL"}:
        return
    if not approval_token:
        raise ApprovalRequiredError(f"{action_name} requires an approval token")
    if config.required_approval_token and approval_token != config.required_approval_token:
        raise ApprovalRequiredError(f"{action_name} approval token did not match the required token")


def validate_path_policy(*, read_paths: list[str] | None = None, write_paths: list[str] | None = None) -> dict[str, list[str]]:
    normalized_reads = [normalize_path(path) for path in (read_paths or [])]
    normalized_writes = [normalize_path(path) for path in (write_paths or [])]

    for path in normalized_reads + normalized_writes:
        if path in DENIED_ROOTS or any(path == denied or path.startswith(f"{denied}/") for denied in DENIED_ROOTS if denied != "/"):
            raise PathPolicyError(f"denied path: {path}")

    for path in normalized_reads:
        if not any(path == root or path.startswith(f"{root}/") for root in ALLOWED_READ_ROOTS):
            raise PathPolicyError(f"read path outside allowlist: {path}")

    for path in normalized_writes:
        if not any(path == root or path.startswith(f"{root}/") for root in ALLOWED_WRITE_ROOTS):
            raise PathPolicyError(f"write path outside allowlist: {path}")

    return {"read_paths": normalized_reads, "write_paths": normalized_writes}


def service_operation_risk(operation: str) -> str:
    return {
        "status": "LOW",
        "start": "MEDIUM",
        "enable": "MEDIUM",
        "stop": "HIGH",
        "restart": "HIGH",
    }[operation]


def file_operation_risk(operation: str, *, overwrite: bool = False) -> str:
    if operation == "search":
        return "LOW"
    if operation == "copy":
        return "HIGH" if overwrite else "MEDIUM"
    if operation == "move":
        return "HIGH"
    if operation == "archive":
        return "MEDIUM"
    raise IntegrationError(f"unsupported file operation: {operation}")


def build_service_preview(operation: str, service_name: str, executor: str, risk_level: str) -> dict[str, Any]:
    return {
        "preview": True,
        "domain": "service",
        "operation": operation,
        "service_name": service_name,
        "executor": executor,
        "risk_level": risk_level,
    }


def build_file_preview(operation: str, src: str, dst: str, executor: str, risk_level: str, *, overwrite: bool = False) -> dict[str, Any]:
    return {
        "preview": True,
        "domain": "files",
        "operation": operation,
        "src": src,
        "dst": dst,
        "executor": executor,
        "risk_level": risk_level,
        "overwrite": overwrite,
    }


def ensure_mutation_fixture(root: str, *, filename: str, content: str) -> str:
    root_path = Path(normalize_path(root))
    root_path.mkdir(parents=True, exist_ok=True)
    file_path = root_path / filename
    file_path.write_text(content, encoding="utf-8")
    return str(file_path)


def ensure_directory(path: str) -> str:
    normalized = normalize_path(path)
    Path(normalized).mkdir(parents=True, exist_ok=True)
    return normalized


def discover_source_capabilities(repo_path: str | None) -> dict[str, bool]:
    if not repo_path:
        return {}

    root = Path(repo_path)
    if not root.exists():
        return {}

    service_text = _read_first_match(root, "**/api/v*/service.py")
    filesystem_text = _read_first_match(root, "**/api/v*/filesystem.py")
    plugin_service_text = _read_first_match(root, "**/plugins/service.py")
    plugin_filesystem_text = _read_first_match(root, "**/plugins/filesystem.py")

    return {
        "service.query": "query" in plugin_service_text and "class ServiceService" in plugin_service_text,
        "service.control": "ServiceControlArgs" in service_text and "def control" in plugin_service_text,
        "filesystem.listdir": "FilesystemListdirArgs" in filesystem_text and "def listdir" in plugin_filesystem_text,
        "filesystem.copy": "FilesystemCopyArgs" in filesystem_text and "def copy" in plugin_filesystem_text,
        "filesystem.move": "FilesystemMoveArgs" in filesystem_text and "def move" in plugin_filesystem_text,
    }


def _read_first_match(root: Path, pattern: str) -> str:
    for path in sorted(root.glob(pattern)):
        if path.is_file():
            return path.read_text(encoding="utf-8")
    return ""


class MiddlewareClient:
    def __init__(self, config: IntegrationConfig):
        self.config = config

    def is_available(self) -> bool:
        return command_exists(self.config.middleware_bin)

    def call(self, method: str, *args: Any) -> tuple[Any, CommandResult]:
        if not self.is_available():
            raise CapabilityUnavailableError(f"middleware command not found: {self.config.middleware_bin}")

        argv = [self.config.middleware_bin, "call", method, *[json.dumps(arg) for arg in args]]
        result = run_command(argv, timeout=self.config.middleware_timeout)
        if result.returncode != 0:
            raise CommandExecutionError(
                f"middleware call failed for {method}",
                argv=result.argv,
                stdout=result.stdout,
                stderr=result.stderr,
                returncode=result.returncode,
            )

        return parse_loose_value(result.stdout), result

    def get_methods(self, target: str = "REST") -> tuple[dict[str, Any], CommandResult]:
        payload, result = self.call("core.get_methods", None, target)
        if not isinstance(payload, dict):
            raise IntegrationError("core.get_methods did not return a dictionary")
        return payload, result

    def get_services(self, target: str = "CLI") -> tuple[dict[str, Any], CommandResult]:
        payload, result = self.call("core.get_services", target)
        if not isinstance(payload, dict):
            raise IntegrationError("core.get_services did not return a dictionary")
        return payload, result

    def service_control(self, operation: str, service_name: str) -> tuple[Any, CommandResult]:
        if operation not in {"start", "stop", "restart"}:
            raise IntegrationError(f"unsupported service control operation: {operation}")
        return self.call("service.control", operation.upper(), service_name, {})

    def filesystem_copy(self, src: str, dst: str, *, recursive: bool = False, preserve_attrs: bool = False) -> tuple[Any, CommandResult]:
        return self.call("filesystem.copy", src, dst, {"recursive": recursive, "preserve_attrs": preserve_attrs})

    def filesystem_move(self, src: str, dst_dir: str, *, recursive: bool = False) -> tuple[Any, CommandResult]:
        return self.call("filesystem.move", [src], dst_dir, {"recursive": recursive})


class MidcliClient:
    def __init__(self, config: IntegrationConfig):
        self.config = config

    def is_available(self) -> bool:
        return command_exists(self.config.midcli_bin)

    def run(self, command: str, *, mode: str | None = None, print_template: bool = False) -> CommandResult:
        if not self.is_available():
            raise CapabilityUnavailableError(f"midcli command not found: {self.config.midcli_bin}")

        argv = [self.config.midcli_bin]
        if self.config.midcli_url:
            argv.extend(["--url", self.config.midcli_url])
        if self.config.midcli_user:
            argv.extend(["--user", self.config.midcli_user])
        if self.config.midcli_password:
            argv.extend(["--password", self.config.midcli_password])
        argv.extend(["-m", mode or self.config.midcli_mode])
        if print_template:
            argv.append("--print-template")
        argv.extend(["-c", command])

        result = run_command(argv, timeout=self.config.midcli_timeout)
        if result.returncode != 0:
            raise CommandExecutionError(
                f"midcli command failed: {command}",
                argv=result.argv,
                stdout=result.stdout,
                stderr=result.stderr,
                returncode=result.returncode,
            )

        return result

    def run_csv_query(self, command: str) -> tuple[list[dict[str, str]], CommandResult]:
        result = self.run(command, mode="csv")
        return parse_csv_rows(result.stdout), result

    def service_control(self, operation: str, service_name: str) -> CommandResult:
        if operation not in {"start", "stop", "restart"}:
            raise IntegrationError(f"unsupported service control operation: {operation}")
        return self.run(f"service {operation} service={service_name}")

    def filesystem_copy(self, src: str, dst: str, *, recursive: bool = False) -> CommandResult:
        command = f"filesystem copy src={json.dumps(src)} dst={json.dumps(dst)}"
        if recursive:
            command += " recursive=true"
        return self.run(command)

    def filesystem_move(self, src: str, dst_dir: str, *, recursive: bool = False) -> CommandResult:
        command = f"filesystem move src={json.dumps(src)} dst={json.dumps(dst_dir)}"
        if recursive:
            command += " recursive=true"
        return self.run(command)


def default_midcli_service_query(config: IntegrationConfig) -> str:
    if config.midcli_service_query_command:
        return config.midcli_service_query_command
    return f"service query service,state,enable WHERE service == '{config.probe_service}'"


def default_midcli_filesystem_command(config: IntegrationConfig) -> str:
    if config.midcli_filesystem_command:
        return config.midcli_filesystem_command
    return f"filesystem listdir path={config.filesystem_path}"


def execute_service_action(
    *,
    middleware: MiddlewareClient,
    midcli: MidcliClient,
    config: IntegrationConfig,
    operation: str,
    service_name: str,
    prefer_midcli: bool = False,
    dry_run: bool = True,
    approval_token: str | None = None,
) -> dict[str, Any]:
    ensure_service_name(service_name)
    risk_level = service_operation_risk(operation)
    executor = "midcli" if prefer_midcli else "middleware_api"

    if dry_run:
        return build_service_preview(operation, service_name, executor, risk_level)

    ensure_approved(risk_level, config, approval_token=approval_token, action_name=f"service.{operation}")

    if not prefer_midcli and middleware.is_available():
        payload, result = middleware.service_control(operation, service_name)
        return {
            "preview": False,
            "executor": "middleware_api",
            "operation": operation,
            "service_name": service_name,
            "risk_level": risk_level,
            "duration_ms": result.duration_ms,
            "result": payload,
            "approver_id": config.approver_id,
        }

    if midcli.is_available():
        result = midcli.service_control(operation, service_name)
        return {
            "preview": False,
            "executor": "midcli",
            "operation": operation,
            "service_name": service_name,
            "risk_level": risk_level,
            "duration_ms": result.duration_ms,
            "result": parse_loose_value(result.stdout) or result.stdout.strip(),
            "approver_id": config.approver_id,
        }

    raise CapabilityUnavailableError("neither middleware nor midcli is available for service control")


def execute_file_action(
    *,
    middleware: MiddlewareClient,
    midcli: MidcliClient,
    config: IntegrationConfig,
    operation: str,
    src: str,
    dst: str,
    recursive: bool = False,
    overwrite: bool = False,
    prefer_midcli: bool = False,
    dry_run: bool = True,
    approval_token: str | None = None,
) -> dict[str, Any]:
    policy = validate_path_policy(read_paths=[src], write_paths=[dst])
    src_normalized = policy["read_paths"][0]
    dst_normalized = policy["write_paths"][0]
    risk_level = file_operation_risk(operation, overwrite=overwrite)
    executor = "midcli" if prefer_midcli else "middleware_api"

    if dry_run:
        return build_file_preview(operation, src_normalized, dst_normalized, executor, risk_level, overwrite=overwrite)

    ensure_approved(risk_level, config, approval_token=approval_token, action_name=f"files.{operation}")

    if operation == "copy":
        if not prefer_midcli and middleware.is_available():
            payload, result = middleware.filesystem_copy(src_normalized, dst_normalized, recursive=recursive)
            return {
                "preview": False,
                "executor": "middleware_api",
                "operation": operation,
                "src": src_normalized,
                "dst": dst_normalized,
                "risk_level": risk_level,
                "duration_ms": result.duration_ms,
                "result": payload,
                "approver_id": config.approver_id,
            }
        if midcli.is_available():
            result = midcli.filesystem_copy(src_normalized, dst_normalized, recursive=recursive)
            return {
                "preview": False,
                "executor": "midcli",
                "operation": operation,
                "src": src_normalized,
                "dst": dst_normalized,
                "risk_level": risk_level,
                "duration_ms": result.duration_ms,
                "result": parse_loose_value(result.stdout) or result.stdout.strip(),
                "approver_id": config.approver_id,
            }

    if operation == "move":
        dst_dir = dst_normalized
        if not prefer_midcli and middleware.is_available():
            payload, result = middleware.filesystem_move(src_normalized, dst_dir, recursive=recursive)
            return {
                "preview": False,
                "executor": "middleware_api",
                "operation": operation,
                "src": src_normalized,
                "dst": dst_dir,
                "risk_level": risk_level,
                "duration_ms": result.duration_ms,
                "result": payload,
                "approver_id": config.approver_id,
            }
        if midcli.is_available():
            result = midcli.filesystem_move(src_normalized, dst_dir, recursive=recursive)
            return {
                "preview": False,
                "executor": "midcli",
                "operation": operation,
                "src": src_normalized,
                "dst": dst_dir,
                "risk_level": risk_level,
                "duration_ms": result.duration_ms,
                "result": parse_loose_value(result.stdout) or result.stdout.strip(),
                "approver_id": config.approver_id,
            }

    raise CapabilityUnavailableError(f"no available executor for files.{operation}")