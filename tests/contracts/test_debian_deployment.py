from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_debian_installer_enables_harborbeacon_service() -> None:
    content = (ROOT / "tools" / "install_debian13_services.sh").read_text(encoding="utf-8")
    assert "harborbeacon.service" in content
    assert "run_harborbeacon_service.sh" in content
    assert "HARBOR_TASK_API_URL=http://127.0.0.1:4174" in content
    assert "HARBORBEACON_WEB_API_URL=http://127.0.0.1:4174" in content
    assert "HARBOR_MODEL_API_BASE_URL=http://127.0.0.1:4174/api/inference/v1" in content
    assert "cat > /etc/systemd/system/feishu-harbor-bot.service <<EOF" not in content
    assert "run_feishu_harbor_bot.sh" not in content
    assert "enable --now assistant-task-api.service agent-hub-admin-api.service feishu-harbor-bot.service" not in content
    assert "systemctl disable --now \"${legacy_service}\"" in content
    assert 'rm -f "/etc/systemd/system/${legacy_service}"' in content
    assert "systemctl enable --now harborbeacon.service" in content


def test_debian_real_usage_plan_mentions_unified_harborbeacon_service() -> None:
    content = (ROOT / "docs" / "debian13-real-usage-plan.md").read_text(encoding="utf-8")
    assert "`harborbeacon.service`" in content
    assert "127.0.0.1:4174" in content
    assert "task-api-conversations.json" in content
    assert "feishu-conversations.json" not in content
