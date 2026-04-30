"""HarborBeacon runtime bootstrap helpers.

This module provides the first concrete startup assembly layer for HarborBeacon.
It wires the registry, router, runtime, dispatcher, webhook receiver, and
long-connection gateway into a single object so callers do not have to
hand-assemble the stack in every entry point or test.
"""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable

from orchestrator.audit import AuditLog
from orchestrator.runtime import Runtime
from orchestrator.router import Router
from skills.executor import executors_from_manifest
from skills.registry import Registry

from .attachments import AttachmentResolver
from .autonomy import Autonomy
from .camera_domain import register_camera_domain
from .channels import Channel, ChannelConfig, ChannelRegistry, OutboundMessage, load_channel_configs
from .dispatcher import Dispatcher
from .intent import IntentParser
from .long_connection import FeishuWsConfig, Gateway, TransportMode, recommended_transport
from .mcp_adapter import McpServerAdapter
from .task_api import TaskApiClient
from .webhook import WebhookReceiver


ChannelSender = Callable[[OutboundMessage], None]
TaskApiRequestFn = Callable[[str, dict[str, Any], float], tuple[int, dict[str, Any]]]

DEFAULT_BUILTINS_DIR = Path(__file__).resolve().parents[1] / "skills" / "builtins"


@dataclass
class HarborBeaconApp:
    """Fully assembled HarborBeacon runtime surface."""

    registry: Registry
    router: Router
    runtime: Runtime
    mcp_adapter: McpServerAdapter
    intent_parser: IntentParser
    channel_registry: ChannelRegistry
    dispatcher: Dispatcher
    webhook_receiver: WebhookReceiver
    gateway: Gateway
    task_api_client: TaskApiClient
    channel_configs: list[ChannelConfig] = field(default_factory=list)


def build_harborbeacon_app(
    channel_configs: list[ChannelConfig] | dict[str, Any] | None = None,
    *,
    channel_senders: dict[Channel, ChannelSender] | None = None,
    task_api_client: TaskApiClient | None = None,
    task_api_base_url: str | None = None,
    task_api_request_fn: TaskApiRequestFn | None = None,
    skills_dirs: Iterable[Path | str] | None = None,
    api_call_fn: Callable[..., tuple[Any, int]] | None = None,
    cli_run_fn: Callable[[str], tuple[str, int]] | None = None,
    attachment_resolver: AttachmentResolver | None = None,
    default_autonomy: Autonomy = Autonomy.SUPERVISED,
    approval_token: str | None = None,
    session_timeout: int = 600,
    thinking_threshold_ms: int = 2500,
) -> HarborBeaconApp:
    """Build the default HarborBeacon stack.

    The bootstrap loads built-in skills, registers route executors derived from
    manifests, explicitly wires the Home Agent Hub camera domain through the
    local Task API bridge, and assembles both webhook and long-connection
    channel entry points.
    """

    normalized_configs = _normalize_channel_configs(channel_configs)
    resolved_task_api_client = task_api_client or TaskApiClient(
        base_url=task_api_base_url,
        request_fn=task_api_request_fn,
    )

    registry = Registry()
    for skills_dir in _resolve_skills_dirs(skills_dirs):
        registry.load_dir(skills_dir)

    router = Router()
    task_api_call_fn = (
        resolved_task_api_client.execute_action
        if resolved_task_api_client.is_available()
        else None
    )
    for manifest in registry.skills:
        for executor in executors_from_manifest(
            manifest,
            api_call_fn=api_call_fn,
            cli_run_fn=cli_run_fn,
            task_api_call_fn=task_api_call_fn,
        ):
            router.register(executor)

    # Ensure the camera domain is present on the real startup path even when
    # callers forget to hand-wire it after loading built-in manifests.
    register_camera_domain(
        registry,
        router,
        task_api_client=resolved_task_api_client,
    )

    runtime = Runtime(router=router, audit=AuditLog())
    mcp_adapter = McpServerAdapter(
        registry,
        runtime,
        default_autonomy=default_autonomy,
        approval_token=approval_token,
    )
    intent_parser = IntentParser(tools=mcp_adapter.list_tools())

    channel_registry = ChannelRegistry()
    webhook_receiver = WebhookReceiver()
    dispatcher = Dispatcher(
        intent_parser=intent_parser,
        mcp_adapter=mcp_adapter,
        channel_registry=channel_registry,
        attachment_resolver=attachment_resolver,
        session_timeout=session_timeout,
        default_autonomy=default_autonomy,
        thinking_threshold_ms=thinking_threshold_ms,
    )
    webhook_receiver.set_default_handler(dispatcher.handle)
    gateway = Gateway(on_message=dispatcher.handle)

    for config in normalized_configs:
        sender = channel_senders.get(config.channel) if channel_senders else None
        channel_registry.register(config, sender=sender)
        if config.is_configured():
            webhook_receiver.register_channel(
                config.channel,
                config,
                on_message=dispatcher.handle,
            )
            _register_gateway_transport(gateway, config)

    return HarborBeaconApp(
        registry=registry,
        router=router,
        runtime=runtime,
        mcp_adapter=mcp_adapter,
        intent_parser=intent_parser,
        channel_registry=channel_registry,
        dispatcher=dispatcher,
        webhook_receiver=webhook_receiver,
        gateway=gateway,
        task_api_client=resolved_task_api_client,
        channel_configs=normalized_configs,
    )


def _normalize_channel_configs(
    channel_configs: list[ChannelConfig] | dict[str, Any] | None,
) -> list[ChannelConfig]:
    if channel_configs is None:
        return []
    if isinstance(channel_configs, dict):
        return load_channel_configs(channel_configs)
    return list(channel_configs)


def _resolve_skills_dirs(skills_dirs: Iterable[Path | str] | None) -> list[Path]:
    resolved = [DEFAULT_BUILTINS_DIR]
    if skills_dirs:
        for path in skills_dirs:
            resolved.append(Path(path))
    return resolved


def _register_gateway_transport(gateway: Gateway, config: ChannelConfig) -> None:
    mode = _resolve_transport_mode(config)

    if config.channel == Channel.FEISHU and mode == TransportMode.WEBSOCKET:
        if not (config.app_id and config.app_secret):
            return
        domain = (
            str(config.extra.get("domain") or "")
            or os.getenv("FEISHU_DOMAIN", "https://open.feishu.cn")
        )
        gateway.register_feishu(
            FeishuWsConfig(
                app_id=config.app_id,
                app_secret=config.app_secret,
                domain=domain,
            )
        )
        return

    if config.channel == Channel.TELEGRAM and mode == TransportMode.LONG_POLL:
        if not config.bot_token:
            return
        poll_timeout = int(config.extra.get("poll_timeout", 30))
        gateway.register_telegram(config.bot_token, poll_timeout=poll_timeout)


def _resolve_transport_mode(config: ChannelConfig) -> TransportMode:
    if config.transport_mode:
        return TransportMode(config.transport_mode)
    return recommended_transport(config.channel)
