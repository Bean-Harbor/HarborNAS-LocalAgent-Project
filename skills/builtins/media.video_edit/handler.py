"""media.video_edit handler stub.

In production, this would shell out to ffmpeg / ffprobe or similar CLI tools.
"""
from __future__ import annotations

from typing import Any


def handle(operation: str, input_path: str, output_path: str | None = None,
           options: dict[str, Any] | None = None) -> dict[str, Any]:
    handlers = {
        "trim": _trim,
        "concat": _concat,
        "transcode": _transcode,
        "thumbnail": _thumbnail,
    }
    fn = handlers.get(operation)
    if fn is None:
        return {"error": f"Unknown operation: {operation!r}"}
    return fn(input_path=input_path, output_path=output_path, options=options or {})


def _trim(*, input_path: str, output_path: str | None, options: dict) -> dict[str, Any]:
    return {"status": "ok", "operation": "trim", "input": input_path, "output": output_path}


def _concat(*, input_path: str, output_path: str | None, options: dict) -> dict[str, Any]:
    return {"status": "ok", "operation": "concat", "input": input_path, "output": output_path}


def _transcode(*, input_path: str, output_path: str | None, options: dict) -> dict[str, Any]:
    return {"status": "ok", "operation": "transcode", "input": input_path, "output": output_path}


def _thumbnail(*, input_path: str, output_path: str | None, options: dict) -> dict[str, Any]:
    return {"status": "ok", "operation": "thumbnail", "input": input_path, "output": output_path}
