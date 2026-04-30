#!/usr/bin/env python3
"""
Local-first rehearsal smoke for HarborOS host .82.

This script is intentionally operator-facing: it prints a compact checklist for
tomorrow's demo rather than emitting machine-oriented JSON only.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

import paramiko


TURN_TOKEN = "0DjSGWlVWJEkWpzHHYyyXLty4jSgRLSI5aHLarRK2l8"


def http_get(url: str, timeout: int = 30) -> tuple[int, str]:
    req = urllib.request.Request(url)
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return response.status, response.read().decode("utf-8", errors="replace")


def http_post(url: str, payload: dict, headers: dict[str, str] | None = None, timeout: int = 90) -> tuple[int, str]:
    merged_headers = {"Content-Type": "application/json"}
    if headers:
        merged_headers.update(headers)
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers=merged_headers,
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return response.status, response.read().decode("utf-8", errors="replace")


def ssh_python(host: str, username: str, password: str, script: str, timeout: int = 180) -> str:
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(host, username=username, password=password, timeout=20)
    try:
        stdin, stdout, stderr = client.exec_command("python3 -", timeout=timeout)
        stdin.write(script)
        stdin.channel.shutdown_write()
        output = stdout.read().decode("utf-8", errors="replace")
        error = stderr.read().decode("utf-8", errors="replace")
        if error.strip():
            output += "\n[stderr]\n" + error
        return output
    finally:
        client.close()


def print_section(title: str) -> None:
    print(f"\n== {title} ==")


def main() -> int:
    parser = argparse.ArgumentParser(description="Harbor 82 local-first rehearsal smoke")
    parser.add_argument("--host", default="192.168.3.82")
    parser.add_argument("--username", default="harboros_admin")
    parser.add_argument("--password", default="123456")
    parser.add_argument(
        "--demo-image",
        default=str(Path(".codex_tmp_harbor82_demo.png").resolve()),
        help="Local PNG used for fallback VLM validation",
    )
    args = parser.parse_args()

    host = args.host
    admin_base = f"http://{host}:4174"

    print_section("Target")
    print(f"host={host}")
    print(f"username={args.username}")

    print_section("RAG readiness")
    status, body = http_get(f"{admin_base}/api/rag/readiness")
    readiness = json.loads(body)
    print(f"http={status} status={readiness.get('status')} summary={readiness.get('summary')}")
    privacy = readiness.get("privacy_policy") or {}
    print(f"privacy_policy={privacy.get('summary', 'n/a')}")
    profiles = readiness.get("resource_profiles") or []
    for item in profiles:
        print(
            f"profile={item.get('profile')} status={item.get('status')} detail={item.get('detail')}"
        )

    print_section("Model policies")
    status, body = http_get(f"{admin_base}/api/models/policies")
    policies = json.loads(body).get("route_policies", [])
    interesting = [
        policy
        for policy in policies
        if policy.get("route_policy_id") in {"retrieval.embed", "retrieval.answer", "retrieval.vision_summary"}
    ]
    if not interesting:
        print("No route policies found for retrieval.*")
    for policy in interesting:
        print(
            "policy={id} privacy={privacy} local_preferred={local} fallback_order={fallback}".format(
                id=policy.get("route_policy_id"),
                privacy=policy.get("privacy_level"),
                local=policy.get("local_preferred"),
                fallback=policy.get("fallback_order"),
            )
        )

    print_section("Knowledge index")
    status, body = http_get(f"{admin_base}/api/knowledge/index/status")
    index_status = json.loads(body)
    print(
        "http={http} status={status} docs={docs} images={images} embeddings={embeddings}".format(
            http=status,
            status=index_status.get("status"),
            docs=index_status.get("document_count"),
            images=index_status.get("image_count"),
            embeddings=index_status.get("embedding_entry_count"),
        )
    )

    print_section("Loopback health and fallback")
    loopback_script = """
import base64
import json
import urllib.request
from pathlib import Path

def get(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.status, response.read().decode("utf-8", errors="replace")

def post(url, payload, headers=None, timeout=90):
    merged_headers = {"Content-Type": "application/json"}
    if headers:
        merged_headers.update(headers)
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers=merged_headers,
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return response.status, response.read().decode("utf-8", errors="replace")

for url in [
    "http://127.0.0.1:4188/v1/healthz",
    "http://127.0.0.1:4176/healthz",
]:
    status, body = get(url)
    print(f"GET {url} -> {status}")
    print(body[:600])

status, body = post(
    "http://127.0.0.1:4176/v1/chat/completions",
    {"model": "deepseek-ai/DeepSeek-V4-Flash", "messages": [{"role": "user", "content": "只回复OK"}]},
    {"Authorization": "Bearer local-proxy"},
)
print(f"CHAT -> {status}")
print(body[:400])

status, body = post(
    "http://127.0.0.1:4176/v1/embeddings",
    {"model": "Qwen/Qwen3-Embedding-0.6B", "input": "local first fallback rehearsal"},
    {"Authorization": "Bearer local-proxy"},
)
print(f"EMBED -> {status}")
print(body[:400])

img = Path("/var/lib/harborbeacon-agent-ci/writable/knowledge-demo/harbor-82-demo.png").read_bytes()
data_url = "data:image/png;base64," + base64.b64encode(img).decode("ascii")
status, body = post(
    "http://127.0.0.1:4188/v1/chat/completions",
    {
        "model": "Qwen/Qwen3-VL-8B-Instruct",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image_url", "image_url": {"url": data_url, "detail": "low"}},
                {"type": "text", "text": "用不超过8个字描述图片主题"}
            ]
        }],
        "stream": False,
        "max_tokens": 48
    },
)
print(f"VLM -> {status}")
print(body[:400])
"""
    print(ssh_python(host, args.username, args.password, loopback_script).strip())

    print_section("Turn smoke")
    turn_script = f"""
import json
import urllib.request

def ask(turn_id, trace_id, thread_id, message_id, route_key, text):
    payload = {{
      "turn": {{"turn_id": turn_id, "trace_id": trace_id}},
      "actor": {{"user_id": "local-owner", "workspace_id": "home-1"}},
      "conversation": {{
        "handle": None,
        "channel": "api",
        "surface": "harborgate",
        "thread_id": thread_id,
        "chat_type": "p2p"
      }},
      "transport": {{
        "route_key": route_key,
        "message_id": message_id,
        "capabilities": ["text"]
      }},
      "input": {{"text": text}},
      "autonomy": {{"level": "supervised"}}
    }}
    req = urllib.request.Request(
        "http://127.0.0.1:4175/api/turns",
        data=json.dumps(payload).encode("utf-8"),
        headers={{
            "Content-Type": "application/json",
            "Authorization": "Bearer {TURN_TOKEN}",
            "X-Contract-Version": "2.0",
        }},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=90) as response:
        return json.loads(response.read().decode("utf-8"))

for item in [
    ask(
        "turn-rehearsal-content-01",
        "trace-rehearsal-content-01",
        "thread-rehearsal-content",
        "msg-rehearsal-content",
        "route-rehearsal-content",
        "搜索已有内容：根据当前知识库，总结 Harbor 82 的演示环境。"
    ),
    ask(
        "turn-rehearsal-arch-01",
        "trace-rehearsal-arch-01",
        "thread-rehearsal-arch",
        "msg-rehearsal-arch",
        "route-rehearsal-arch",
        "搜索已有内容：HarborBeacon 为什么说是 local first，而不是 cloud first？"
    )
]:
    print(json.dumps(item, ensure_ascii=False)[:1800])
"""
    print(ssh_python(host, args.username, args.password, turn_script).strip())

    print_section("Talking points")
    print("1. Default posture is local first.")
    print("2. privacy_level and resource_profile gate cloud execution.")
    print("3. SiliconFlow is the current fallback proof, not the final default architecture.")
    print("4. GPU is present on .82, but local runtime promotion is still the next step after tomorrow's rehearsal.")
    print("5. Use the explicit prefix '搜索已有内容：' to avoid the generic clarify branch.")

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except urllib.error.HTTPError as error:
        print(f"HTTP error {error.code}: {error.read().decode('utf-8', errors='replace')}", file=sys.stderr)
        raise
