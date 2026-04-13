"""Attachment resolution for IM-originated media.

Resolves platform-specific attachment references into local temporary files
that the rest of the HarborBeacon -> HarborOS pipeline can treat as normal
file inputs.
"""
from __future__ import annotations

import json
import os
import re
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from harborbeacon.channels import (
    Attachment,
    AttachmentType,
    Channel,
    ChannelConfig,
    InboundMessage,
)

_DEFAULT_FEISHU_DOMAIN = "https://open.feishu.cn"
_SAFE_NAME_RE = re.compile(r"[^A-Za-z0-9._-]+")


class AttachmentResolutionError(RuntimeError):
    """Raised when a remote IM attachment cannot be materialized locally."""


@dataclass
class ResolvedAttachment:
    local_path: str
    file_name: str
    size_bytes: int = 0
    mime_type: str = "application/octet-stream"
    metadata: dict[str, Any] = field(default_factory=dict)


class AttachmentResolver:
    """Resolve IM attachments into local temporary files."""

    def __init__(self, download_root: Path | None = None) -> None:
        env_root = os.environ.get("HARBORBEACON_ATTACHMENT_DIR", "")
        if download_root is not None:
            self._download_root = download_root
        elif env_root:
            self._download_root = Path(env_root)
        else:
            self._download_root = Path(tempfile.gettempdir()) / "harborbeacon" / "attachments"
        self._token_cache: dict[tuple[str, str], tuple[str, int]] = {}

    def resolve_message_attachment(
        self,
        inbound: InboundMessage,
        config: ChannelConfig,
        attachment: Attachment,
    ) -> ResolvedAttachment | None:
        if inbound.channel != Channel.FEISHU:
            return None
        if not inbound.message_id:
            raise AttachmentResolutionError("Feishu attachment download requires message_id")
        if not attachment.content:
            raise AttachmentResolutionError("Feishu attachment download requires attachment key")
        if not config.app_id or not config.app_secret:
            raise AttachmentResolutionError("Feishu attachment download requires app_id and app_secret")

        domain = str(config.extra.get("domain", _DEFAULT_FEISHU_DOMAIN)).rstrip("/")
        token = self._get_tenant_token(config.app_id, config.app_secret, domain)
        payload, headers = self._download_feishu_resource(
            domain=domain,
            token=token,
            message_id=inbound.message_id,
            resource_key=attachment.content,
            attachment_type=attachment.type,
        )

        file_name = attachment.file_name or self._infer_filename(headers, attachment)
        target_path = self._build_target_path(inbound, file_name)
        target_path.parent.mkdir(parents=True, exist_ok=True)
        target_path.write_bytes(payload)

        return ResolvedAttachment(
            local_path=str(target_path),
            file_name=target_path.name,
            size_bytes=len(payload),
            mime_type=headers.get("Content-Type", attachment.mime_type),
            metadata={
                "source_channel": inbound.channel.value,
                "source_message_id": inbound.message_id,
                "attachment_key": attachment.content,
                "attachment_type": attachment.type.value,
            },
        )

    def _get_tenant_token(self, app_id: str, app_secret: str, domain: str) -> str:
        cache_key = (domain, app_id)
        cached = self._token_cache.get(cache_key)
        now = int(__import__("time").time())
        if cached and cached[1] > now:
            return cached[0]

        url = f"{domain}/open-apis/auth/v3/tenant_access_token/internal"
        body = json.dumps({"app_id": app_id, "app_secret": app_secret}).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=body,
            headers={"Content-Type": "application/json; charset=utf-8"},
            method="POST",
        )

        try:
            with urllib.request.urlopen(request, timeout=15) as response:
                data = json.loads(response.read().decode("utf-8"))
        except urllib.error.URLError as exc:
            raise AttachmentResolutionError(f"Failed to obtain Feishu tenant token: {exc}") from exc

        if data.get("code") != 0 or not data.get("tenant_access_token"):
            raise AttachmentResolutionError(
                f"Failed to obtain Feishu tenant token: {data.get('msg', 'unknown error')}"
            )

        expires_in = int(data.get("expire", data.get("expires_in", 7200)))
        token = str(data["tenant_access_token"])
        self._token_cache[cache_key] = (token, now + max(expires_in - 60, 60))
        return token

    def _download_feishu_resource(
        self,
        *,
        domain: str,
        token: str,
        message_id: str,
        resource_key: str,
        attachment_type: AttachmentType,
    ) -> tuple[bytes, dict[str, str]]:
        resource_type = self._feishu_resource_type(attachment_type)
        quoted_message_id = urllib.parse.quote(message_id, safe="")
        quoted_resource_key = urllib.parse.quote(resource_key, safe="")
        url = (
            f"{domain}/open-apis/im/v1/messages/{quoted_message_id}/resources/"
            f"{quoted_resource_key}?type={resource_type}"
        )
        request = urllib.request.Request(
            url,
            headers={"Authorization": f"Bearer {token}"},
            method="GET",
        )

        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                payload = response.read()
                headers = {str(k): str(v) for k, v in response.info().items()}
                return payload, headers
        except urllib.error.URLError as exc:
            raise AttachmentResolutionError(f"Failed to download Feishu attachment: {exc}") from exc

    @staticmethod
    def _feishu_resource_type(attachment_type: AttachmentType) -> str:
        mapping = {
            AttachmentType.IMAGE: "image",
            AttachmentType.FILE: "file",
            AttachmentType.AUDIO: "audio",
            AttachmentType.VIDEO: "media",
        }
        return mapping.get(attachment_type, "file")

    def _build_target_path(self, inbound: InboundMessage, file_name: str) -> Path:
        safe_name = _SAFE_NAME_RE.sub("_", file_name).strip("._") or "attachment.bin"
        message_folder = _SAFE_NAME_RE.sub("_", inbound.message_id).strip("._") or "message"
        return self._download_root / inbound.channel.value / message_folder / safe_name

    def _infer_filename(self, headers: dict[str, str], attachment: Attachment) -> str:
        disposition = headers.get("Content-Disposition", "")
        match = re.search(r'filename\*=UTF-8\'\'([^;]+)|filename="?([^";]+)"?', disposition)
        if match:
            encoded = match.group(1) or match.group(2) or ""
            decoded = urllib.parse.unquote(encoded)
            if decoded:
                return decoded

        extension = {
            AttachmentType.IMAGE: ".png",
            AttachmentType.FILE: ".bin",
            AttachmentType.AUDIO: ".opus",
            AttachmentType.VIDEO: ".mp4",
        }.get(attachment.type, ".bin")
        stem = attachment.content or "attachment"
        safe_stem = _SAFE_NAME_RE.sub("_", stem).strip("._") or "attachment"
        return f"{safe_stem}{extension}"
