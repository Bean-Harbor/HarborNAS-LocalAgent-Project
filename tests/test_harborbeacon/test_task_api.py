"""Tests for harborbeacon.task_api."""
from orchestrator.contracts import Action

from harborbeacon.task_api import TaskApiClient


def test_execute_action_builds_task_payload():
    captured = {}

    def fake_request(url, payload, timeout_s):
        captured["url"] = url
        captured["payload"] = payload
        captured["timeout_s"] = timeout_s
        return 200, {"status": "completed", "result": {"message": "ok"}}

    client = TaskApiClient(base_url="http://127.0.0.1:4175", request_fn=fake_request)
    response = client.execute_action(
        Action(
            domain="camera",
            operation="analyze",
            resource={"device_hint": "客厅"},
            args={
                "detect_label": "person",
                "_source": {
                    "channel": "feishu",
                    "conversation_id": "chat-1",
                    "user_id": "user-1",
                    "session_id": "sess-1",
                    "raw_text": "分析客厅摄像头",
                    "approval_token": "tok-1",
                    "approver_id": "user-1",
                },
            },
        ),
        "task-1",
        "step-1",
    )

    assert response["status"] == "completed"
    assert captured["url"] == "http://127.0.0.1:4175/api/tasks"
    assert captured["payload"]["step_id"] == "step-1"
    assert captured["payload"]["intent"]["action"] == "analyze"
    assert captured["payload"]["source"]["conversation_id"] == "chat-1"
    assert captured["payload"]["entity_refs"]["device_hint"] == "客厅"
    assert captured["payload"]["args"]["detect_label"] == "person"
    assert captured["payload"]["autonomy"]["level"] == "supervised"
    assert captured["payload"]["args"]["approval"]["token"] == "tok-1"
    assert captured["payload"]["args"]["approval"]["approver_id"] == "user-1"
    assert "_source" not in captured["payload"]["args"]


def test_execute_action_raises_on_http_error():
    client = TaskApiClient(
        base_url="http://127.0.0.1:4175",
        request_fn=lambda url, payload, timeout_s: (422, {"error": "camera connect failed"}),
    )

    action = Action(domain="camera", operation="connect", resource={}, args={})

    try:
        client.execute_action(action, "task-1", "step-1")
    except RuntimeError as exc:
        assert "camera connect failed" in str(exc)
    else:
        raise AssertionError("expected RuntimeError")
