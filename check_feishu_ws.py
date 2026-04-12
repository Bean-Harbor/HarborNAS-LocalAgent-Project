import json
import os
import urllib.request
import urllib.error

url = 'https://open.feishu.cn/callback/ws/endpoint'
app_id = os.environ.get('FEISHU_APP_ID', '').strip()
app_secret = os.environ.get('FEISHU_APP_SECRET', '').strip()
if not app_id or not app_secret:
    raise SystemExit('Set FEISHU_APP_ID and FEISHU_APP_SECRET before running this script.')
payload = json.dumps({
    'AppID': app_id,
    'AppSecret': app_secret,
}).encode('utf-8')
req = urllib.request.Request(url, data=payload, headers={'Content-Type': 'application/json', 'locale': 'zh'})
try:
    with urllib.request.urlopen(req, timeout=20) as resp:
        body = resp.read().decode('utf-8', errors='replace')
except urllib.error.HTTPError as exc:
    body = exc.read().decode('utf-8', errors='replace')
    print(body)
    raise
print(body)
