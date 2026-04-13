"""Client for the local Assistant Task API bridge."""
from __future__ import annotations

import json
import os
import urllib.error
import urllib.request
from typing import Any, Callable

from orchestrator.contracts import Action


TaskApiRequestFn = Callable[[str, dict[str, Any], float], tuple[int, dict[str, Any]]]

DEFAULT_TASK_API_URL = "http://127.0.0.1:4175"


def _default_request(url: str, payload: dict[str, Any], timeout_s: float) -> tuple[int, dict[str, Any]]:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout_s) as response:
            body = response.read().decode("utf-8")
            return response.getcode(), json.loads(body) if body else {}
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8")
        try:
            payload = json.loads(body) if body else {}
        except json.JSONDecodeError:
            payload = {"error": body or str(exc)}
        return exc.code, payload


class TaskApiClient:
    """Thin HTTP client used by HarborBeacon's camera-domain executor."""

    def __init__(
        self,
        base_url: str | None = None,
        *,
        timeout_s: float = 15.0,
        request_fn: TaskApiRequestFn | None = None,
    ) -> None:
        self._base_url = (base_url or os.getenv("HARBOR_TASK_API_URL") or DEFAULT_TASK_API_URL).rstrip("/")
        self._timeout_s = timeout_s
        self._request_fn = request_fn or _default_request

    @property
    def base_url(self) -> str:
        return self._base_url

    def is_available(self) -> bool:
        return bool(self._base_url)

    def execute_action(self, action: Action, task_id: str, step_id: str) -> dict[str, Any]:
        payload = self._build_payload(action, task_id=task_id, step_id=step_id)
        status, response = self._request_fn(f"{self._base_url}/api/tasks", payload, self._timeout_s)
        if status >= 400:
            error = response.get("error") or response.get("message") or f"Task API request failed ({status})"
            raise RuntimeError(str(error))
        return response

    def _build_payload(self, action: Action, *, task_id: str, step_id: str) -> dict[str, Any]:
        args = dict(action.args or {})
        source = args.pop("_source", {})
        if not isinstance(source, dict):
            source = {}
        trace_id = str(source.get("trace_id") or f"{task_id}:{step_id}")
        raw_text = str(source.get("raw_text") or "")
        autonomy_level = str(source.get("autonomy_level") or "supervised")

        return {
            "task_id": task_id,
            "trace_id": trace_id,
            "source": {
                "channel": source.get("channel", ""),
                "surface": source.get("surface", "harborbeacon"),
                "conversation_id": source.get("conversation_id", ""),
                "user_id": source.get("user_id", ""),
                "session_id": source.get("session_id", ""),
            },
            "intent": {
                "domain": action.domain,
                "action": action.operation,
                "raw_text": raw_text,
            },
            "entity_refs": dict(action.resource or {}),
            "args": args,
            "autonomy": {"level": autonomy_level},
        }
