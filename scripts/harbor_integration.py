from __future__ import annotations

import ast
import csv
import io
import json
import os
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
        )


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


def default_midcli_service_query(config: IntegrationConfig) -> str:
    if config.midcli_service_query_command:
        return config.midcli_service_query_command
    return f"service query service,state,enable WHERE service == '{config.probe_service}'"


def default_midcli_filesystem_command(config: IntegrationConfig) -> str:
    if config.midcli_filesystem_command:
        return config.midcli_filesystem_command
    return f"filesystem listdir path={config.filesystem_path}"