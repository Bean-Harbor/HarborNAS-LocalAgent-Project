import requests
import json

DOMAIN = "https://open.feishu.cn"
APP_ID = "cli_a94bb44b7aba5bcc"
APP_SECRET = "d9owtlQMNrhI3OxDNbjX6cYBWBN6881H"
CHAT_ID = "oc_9c38993cab239a9b1a05e425e590800e"

# Get token
resp = requests.post(
    f"{DOMAIN}/open-apis/auth/v3/tenant_access_token/internal",
    json={"app_id": APP_ID, "app_secret": APP_SECRET},
)
token = resp.json()["tenant_access_token"]

# Send message
msg = (
    "准备工作已完成！\n\n"
    "✅ SKILL.md 已写入 Rust-only 约束\n"
    "✅ 今日工作日志已更新 (docs/daily/2026-03-23.md)\n"
    "✅ feishu-harbor-bot 已编译并运行中\n"
    "✅ HarborOS 连通性已验证 (SSH 当前: RUNNING)\n\n"
    "你回来后直接在这里发「关闭ssh」即可测试。"
)

r = requests.post(
    f"{DOMAIN}/open-apis/im/v1/messages?receive_id_type=chat_id",
    headers={"Authorization": f"Bearer {token}"},
    json={
        "receive_id": CHAT_ID,
        "msg_type": "text",
        "content": json.dumps({"text": msg}),
    },
)
print(r.json())
