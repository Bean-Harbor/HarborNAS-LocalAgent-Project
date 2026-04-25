#!/usr/bin/env python3
"""Minimal local HTTP sidecar for HarborBeacon vision detection."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


ROOT_DIR = Path(__file__).resolve().parent.parent
BRIDGE_SCRIPT = ROOT_DIR / "tools" / "vision_detect_bridge.sh"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="HarborBeacon vision sidecar")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18080)
    return parser.parse_args()


def run_detector(payload: dict) -> tuple[int, dict]:
    image_path = str(payload.get("image_path", "")).strip()
    label = str(payload.get("label", "person")).strip() or "person"
    min_confidence = str(payload.get("min_confidence", 0.25))
    annotated_output = str(payload.get("annotated_output", "")).strip()

    if not image_path:
        return 400, {"error": "image_path is required"}

    cmd = [
        str(BRIDGE_SCRIPT),
        "--image",
        image_path,
        "--label",
        label,
        "--min-confidence",
        min_confidence,
    ]
    if annotated_output:
        cmd.extend(["--annotated-output", annotated_output])

    proc = subprocess.run(cmd, capture_output=True, text=True, cwd=str(ROOT_DIR))
    if proc.returncode != 0:
        detail = proc.stderr.strip() or "detector failed"
        return 500, {"error": "detector_failed", "detail": detail}

    try:
        return 200, json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        return 500, {
            "error": "invalid_detector_output",
            "detail": str(exc),
            "stdout": proc.stdout[:1000],
        }


class Handler(BaseHTTPRequestHandler):
    server_version = "HarborVisionSidecar/0.1"

    def do_GET(self) -> None:
        if self.path == "/healthz":
            self._json(200, {"ok": True})
            return
        self._json(404, {"error": "not_found"})

    def do_POST(self) -> None:
        if self.path != "/analyze":
            self._json(404, {"error": "not_found"})
            return

        try:
            content_length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self._json(400, {"error": "invalid_content_length"})
            return

        body = self.rfile.read(content_length)
        try:
            payload = json.loads(body or b"{}")
        except json.JSONDecodeError as exc:
            self._json(400, {"error": "invalid_json", "detail": str(exc)})
            return

        status, response = run_detector(payload)
        self._json(status, response)

    def log_message(self, fmt: str, *args: object) -> None:
        sys.stdout.write(f"[vision-sidecar] {fmt % args}\n")
        sys.stdout.flush()

    def _json(self, status: int, payload: dict) -> None:
        encoded = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def main() -> int:
    args = parse_args()
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"vision sidecar listening on http://{args.host}:{args.port}", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
