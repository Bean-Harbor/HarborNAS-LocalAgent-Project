from conftest import ROOT, read_doc


def read_text(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def test_release_packaging_scripts_and_templates_exist() -> None:
    required = [
        "tools/build_release_bundle.sh",
        "tools/bootstrap_release_builder.sh",
        "tools/install_harboros_release.sh",
        "tools/rollback_harboros_release.sh",
        "tools/release_templates/harborbeacon-agent-hub.env.template",
        "tools/release_templates/bin/run-harbor-model-api",
        "tools/release_templates/bin/run-assistant-task-api",
        "tools/release_templates/bin/run-agent-hub-admin-api",
        "tools/release_templates/bin/harborgate",
        "tools/release_templates/bin/harborgate-weixin-runner",
        "tools/release_templates/systemd/harbor-model-api.service.template",
        "tools/release_templates/systemd/assistant-task-api.service.template",
        "tools/release_templates/systemd/agent-hub-admin-api.service.template",
        "tools/release_templates/systemd/harborgate.service.template",
        "tools/release_templates/systemd/harborgate-weixin-runner.service.template",
        "docs/harboros-release-packaging-runbook.md",
    ]
    missing = [path for path in required if not (ROOT / path).exists()]
    assert not missing


def test_release_bundle_builder_covers_expected_artifacts() -> None:
    content = read_text("tools/build_release_bundle.sh")
    required_phrases = [
        "RUST_TARGET",
        "x86_64-unknown-linux-musl",
        "BOOTSTRAP_BUILDER_IF_NEEDED",
        "bootstrap_release_builder.sh",
        "RUSTUP_TOOLCHAIN",
        "ZIG_VERSION",
        "cargo zigbuild",
        "cargo-zigbuild",
        "zig",
        "harbor-model-api",
        "file",
        "assistant-task-api",
        "agent-hub-admin-api",
        "validate-contract-schemas",
        "run-e2e-suite",
        "frontend/harbordesk",
        "harborgate/site-packages",
        "manifest.json",
        '"rust_target"',
        '"linkage"',
        "writable_root_default",
        "checksums.sha256",
        "harbor-release-",
        "tar -C",
    ]
    assert all(phrase in content for phrase in required_phrases)


def test_harboros_installer_manages_release_layout_and_services() -> None:
    content = read_text("tools/install_harboros_release.sh")
    required_phrases = [
        "/var/lib/harborbeacon-agent-ci",
        "/mnt/software/harborbeacon-agent-ci",
        "--writable-root",
        "default_writable_root",
        "releases",
        "current",
        "runtime",
        "captures",
        "logs",
        "HARBOR_HARBOROS_WRITABLE_ROOT",
        "HARBOR_KNOWLEDGE_INDEX_ROOT",
        "HARBOR_RELEASE_INSTALL_ROOT",
        "HARBOR_MODEL_API_BIND=127.0.0.1:4176",
        "HARBOR_MODEL_API_BASE_URL=http://127.0.0.1:4176/v1",
        "HARBOR_MODEL_API_TOKEN",
        "HARBOR_MODEL_API_BACKEND",
        "HARBOR_MODEL_API_UPSTREAM_BASE_URL",
        "HARBOR_MODEL_API_CANDLE_CHAT_MODEL_ID",
        "HARBOR_MODEL_API_CANDLE_EMBEDDING_MODEL_ID",
        "HARBOR_MODEL_API_CANDLE_CACHE_DIR",
        "assistant-task-api.service",
        "agent-hub-admin-api.service",
        "harbor-model-api.service",
        "harborgate.service",
        "harborgate-weixin-runner.service",
        "systemctl daemon-reload",
        "systemctl enable",
        "systemctl disable",
        "systemctl stop",
        "ln -sfn",
        "HARBOR_HARBOROS_USER",
        "WEIXIN_ACCOUNT_ID",
        "EXISTING_WRITABLE_ROOT",
        "HARBORBEACON_ADMIN_API_URL=http://127.0.0.1:4174",
        "HARBORBEACON_ADMIN_API_TOKEN",
        "not configured, skipped",
        "append_optional_env",
    ]
    assert all(phrase in content for phrase in required_phrases)


def test_harboros_rollback_script_switches_current_release() -> None:
    content = read_text("tools/rollback_harboros_release.sh")
    required_phrases = [
        "/var/lib/harborbeacon-agent-ci",
        "releases",
        "current",
        "--env-file",
        "/etc/default/harborbeacon-agent-hub",
        "HARBOR_RELEASE_VERSION",
        "ln -sfn",
        "CORE_SERVICES",
        "harbor-model-api.service",
        "harborgate-weixin-runner.service",
        "systemctl restart \"${CORE_SERVICES[@]}\"",
        "systemctl is-enabled",
    ]
    assert all(phrase in content for phrase in required_phrases)


def test_release_packaging_runbook_records_builder_target_and_install_shape() -> None:
    content = read_doc("docs/harboros-release-packaging-runbook.md")
    required_phrases = [
        "192.168.3.223",
        "192.168.3.182",
        "HarborDesk Angular `dist`",
        "HarborGate Python 运行包",
        "不在机上执行 `cargo`、`rustc`、`node`、`npm` 或 `pip`",
        "bootstrap_release_builder.sh",
        "BOOTSTRAP_BUILDER_IF_NEEDED",
        "x86_64-unknown-linux-musl",
        "cargo-zigbuild",
        "zig",
        "harbor-model-api.service",
        "assistant-task-api.service",
        "agent-hub-admin-api.service",
        "harborgate.service",
        "harborgate-weixin-runner.service",
        "/var/lib/harborbeacon-agent-ci",
        "/mnt/software/harborbeacon-agent-ci",
        "HARBOR_HARBOROS_WRITABLE_ROOT",
        "HARBOR_KNOWLEDGE_INDEX_ROOT",
        "HARBOR_RELEASE_VERSION",
        "HARBORBEACON_ADMIN_API_URL=http://127.0.0.1:4174",
        "HarborGate admin sync 依赖 `:4174`",
        "HARBOR_MODEL_API_BASE_URL=http://127.0.0.1:4176/v1",
        "本地 OpenAI-compatible 模型服务",
        "HARBOR_MODEL_API_CANDLE_CHAT_MODEL_ID",
        "HARBOR_MODEL_API_CANDLE_EMBEDDING_MODEL_ID",
        "Qwen/Qwen3-1.7B",
        "jinaai/jina-embeddings-v2-base-zh",
        "not configured, skipped",
    ]
    assert all(phrase in content for phrase in required_phrases)
