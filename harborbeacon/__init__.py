"""HarborBeacon — HarborOS built-in AI assistant (ZeroClaw fork).

Pre-installed in HarborOS. Connects to IM channels (Feishu, WeCom,
Telegram, Discord, etc.) and controls HarborOS via CLI, MCP, and API.
"""

from .bootstrap import HarborBeaconApp, build_harborbeacon_app

__all__ = ["HarborBeaconApp", "build_harborbeacon_app"]
