"""Tests for harborbeacon.attachments."""
from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from harborbeacon.attachments import AttachmentResolutionError, AttachmentResolver
from harborbeacon.channels import Attachment, AttachmentType, Channel, ChannelConfig, InboundMessage


def _make_inbound() -> InboundMessage:
    return InboundMessage(
        channel=Channel.FEISHU,
        sender_id="ou_user",
        text="[图片]",
        message_id="om_dc13264520392913933b6a78b5",
    )


def _make_config() -> ChannelConfig:
    return ChannelConfig(channel=Channel.FEISHU, enabled=True, app_id="cli_x", app_secret="secret")


class TestAttachmentResolver:
    def test_non_feishu_returns_none(self, tmp_path):
        resolver = AttachmentResolver(download_root=tmp_path)
        inbound = InboundMessage(channel=Channel.TELEGRAM, sender_id="u1", text="hi", message_id="m1")
        attachment = Attachment(type=AttachmentType.IMAGE, content="img")
        assert resolver.resolve_message_attachment(inbound, _make_config(), attachment) is None

    def test_requires_message_id(self, tmp_path):
        resolver = AttachmentResolver(download_root=tmp_path)
        inbound = InboundMessage(channel=Channel.FEISHU, sender_id="u1", text="hi")
        attachment = Attachment(type=AttachmentType.IMAGE, content="img")
        with pytest.raises(AttachmentResolutionError):
            resolver.resolve_message_attachment(inbound, _make_config(), attachment)

    def test_downloads_attachment_and_writes_file(self, tmp_path):
        resolver = AttachmentResolver(download_root=tmp_path)
        inbound = _make_inbound()
        config = _make_config()
        attachment = Attachment(type=AttachmentType.IMAGE, content="img_key", file_name="camera.png")

        token_response = MagicMock()
        token_response.read.return_value = json.dumps({
            "code": 0,
            "tenant_access_token": "tenant-token",
            "expire": 7200,
        }).encode("utf-8")
        token_response.__enter__ = MagicMock(return_value=token_response)
        token_response.__exit__ = MagicMock(return_value=False)

        file_response = MagicMock()
        file_response.read.return_value = b"png-bytes"
        file_response.info.return_value = {"Content-Type": "image/png"}
        file_response.__enter__ = MagicMock(return_value=file_response)
        file_response.__exit__ = MagicMock(return_value=False)

        with patch("urllib.request.urlopen", side_effect=[token_response, file_response]):
            resolved = resolver.resolve_message_attachment(inbound, config, attachment)

        assert resolved is not None
        assert resolved.file_name == "camera.png"
        assert resolved.size_bytes == 9
        assert Path(resolved.local_path).read_bytes() == b"png-bytes"

    def test_uses_cached_token(self, tmp_path):
        resolver = AttachmentResolver(download_root=tmp_path)
        resolver._token_cache[("https://open.feishu.cn", "cli_x")] = ("cached-token", 9999999999)

        response = MagicMock()
        response.read.return_value = b"file"
        response.info.return_value = {"Content-Type": "image/png"}
        response.__enter__ = MagicMock(return_value=response)
        response.__exit__ = MagicMock(return_value=False)

        with patch("urllib.request.urlopen", return_value=response) as mock_urlopen:
            resolved = resolver.resolve_message_attachment(
                _make_inbound(),
                _make_config(),
                Attachment(type=AttachmentType.IMAGE, content="img_key", file_name="a.png"),
            )

        assert resolved is not None
        assert mock_urlopen.call_count == 1
