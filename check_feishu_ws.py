import json
import urllib.request
import urllib.error

url = 'https://open.feishu.cn/callback/ws/endpoint'
payload = json.dumps({
    'AppID': 'cli_a94bb44b7aba5bcc',
    'AppSecret': 'd9owtlQMNrhI3OxDNbjX6cYBWBN6881H',
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
