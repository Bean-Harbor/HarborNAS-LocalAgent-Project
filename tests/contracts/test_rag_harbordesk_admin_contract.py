import re

from conftest import read_doc


def test_rag_admin_endpoints_are_exposed_by_backend_and_harbordesk() -> None:
    backend = read_doc("src/bin/agent_hub_admin_api.rs")
    service = read_doc("frontend/harbordesk/src/app/core/admin-api.service.ts")

    required_routes = [
        ("GET", "/api/rag/readiness"),
        ("GET", "/api/knowledge/settings"),
        ("PUT", "/api/knowledge/settings"),
        ("POST", "/api/knowledge/index/run"),
        ("GET", "/api/knowledge/index/status"),
        ("GET", "/api/knowledge/index/jobs"),
        ("POST", "/api/knowledge/index/jobs/"),
    ]

    for method, route in required_routes:
        assert method in backend
        assert route in backend
        assert route in service

    assert "path.ends_with(\"/cancel\")" in backend
    assert "/cancel`" in service


def test_harbordesk_admin_service_uses_same_origin_beacon_api_only() -> None:
    service = read_doc("frontend/harbordesk/src/app/core/admin-api.service.ts")

    direct_calls = re.findall(r"this\.http\.(?:get|post|put|delete)<", service)
    literal_calls = re.findall(
        r"this\.http\.(?:get|post|put|delete)<[^>]+>\(\s*([`'])([^`']+)",
        service,
    )

    assert direct_calls
    assert len(literal_calls) == len(direct_calls)
    for _quote, url in literal_calls:
        assert url.startswith("/api/"), url

    assert "this.http.get<" in service
    assert "http://" not in service
    assert "https://" not in service


def test_harbordesk_index_run_copy_preserves_async_job_boundary() -> None:
    component = read_doc("frontend/harbordesk/src/app/pages/desk-page.component.ts")
    panel = read_doc("frontend/harbordesk/src/app/shared/page-state-panel.component.html")

    message_body = component.split("private knowledgeIndexRunMessage", 1)[1].split(
        "private runDeviceAction", 1
    )[0]

    assert "queued" in message_body
    assert "accepted" in message_body
    assert "Track progress in Index jobs" in message_body
    assert "job_ids" in component
    assert "Knowledge index finished" not in message_body
    assert "Knowledge index completed" not in message_body
    assert "知识库索引已完成" not in message_body

    assert "Queueing knowledge index refresh" in panel
    assert "Queueing..." in panel
    assert "Running knowledge index" not in panel
    assert "Indexing..." not in panel
