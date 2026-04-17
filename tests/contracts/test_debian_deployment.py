from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_debian_installer_enables_assistant_task_api_service() -> None:
    content = (ROOT / "tools" / "install_debian13_services.sh").read_text(encoding="utf-8")
    assert "assistant-task-api.service" in content
    assert "run_assistant_task_api.sh" in content
    assert "HARBOR_TASK_API_URL=http://127.0.0.1:4175" in content


def test_debian_real_usage_plan_mentions_task_api_service() -> None:
    content = (ROOT / "docs" / "debian13-real-usage-plan.md").read_text(encoding="utf-8")
    assert "`assistant-task-api`" in content
    assert "127.0.0.1:4175" in content
