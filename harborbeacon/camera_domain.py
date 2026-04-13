"""Home Agent Hub camera-domain registration for HarborBeacon."""
from __future__ import annotations

from skills.executor import executors_from_manifest
from skills.manifest import ExecutorConfig, RiskConfig, SkillManifest
from skills.registry import Registry
from orchestrator.router import Router

from .task_api import TaskApiClient


CAMERA_CAPABILITIES = [
    "camera.scan",
    "camera.connect",
    "camera.snapshot",
    "camera.live_view",
    "camera.analyze",
    "camera.ptz",
]


def build_camera_domain_manifest() -> SkillManifest:
    return SkillManifest(
        id="home.camera_hub",
        name="Home Agent Hub Camera Domain",
        version="0.1.0",
        summary="Scan, connect, and analyze home cameras",
        owner="harbor-team",
        capabilities=list(CAMERA_CAPABILITIES),
        executors={"mcp": ExecutorConfig(enabled=True)},
        risk=RiskConfig(default_level="LOW"),
        input_schema={
            "type": "object",
            "properties": {
                "resource": {"type": "object"},
                "args": {"type": "object"},
            },
        },
    )


def register_camera_domain(
    registry: Registry,
    router: Router,
    *,
    task_api_client: TaskApiClient | None = None,
) -> SkillManifest:
    manifest = build_camera_domain_manifest()
    if manifest.id not in registry.skill_ids:
        registry.register(manifest)

    client = task_api_client or TaskApiClient()
    for executor in executors_from_manifest(
        manifest,
        task_api_call_fn=client.execute_action if client.is_available() else None,
    ):
        router.register(executor)

    return manifest
