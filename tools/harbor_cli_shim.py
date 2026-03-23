import argparse
import csv
import io
import json
import re
import shlex
import ssl
import sys

import websocket


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--url", required=True)
    parser.add_argument("--user", required=True)
    parser.add_argument("--password", required=True)
    parser.add_argument("-m", "--mode", default="csv")
    parser.add_argument("--print-template", action="store_true")
    parser.add_argument("-c", "--command", required=True)
    return parser.parse_args()


def connect(url: str, user: str, password: str):
    sslopt = {"cert_reqs": ssl.CERT_NONE} if url.startswith("wss://") else {}
    ws = websocket.create_connection(url, timeout=15, sslopt=sslopt)
    ws.send(json.dumps({"msg": "connect", "version": "1", "support": ["1"]}))
    recv = json.loads(ws.recv())
    if recv.get("msg") != "connected":
        raise RuntimeError(f"websocket connect failed: {recv}")
    ws.send(json.dumps({"id": 1, "msg": "method", "method": "auth.login", "params": [user, password]}))
    recv = json.loads(ws.recv())
    if recv.get("msg") != "result" or recv.get("result") is not True:
        raise RuntimeError(f"auth.login failed: {recv}")
    return ws


def call(ws, method: str, params: list):
    ws.send(json.dumps({"id": 2, "msg": "method", "method": method, "params": params}))
    recv = json.loads(ws.recv())
    if recv.get("msg") != "result":
        raise RuntimeError(f"method call failed: {recv}")
    if "error" in recv:
        raise RuntimeError(json.dumps(recv["error"], ensure_ascii=False))
    return recv.get("result")


def parse_service_query(command: str):
    match = re.fullmatch(r"service\s+query\s+([a-z_,]+)\s+WHERE\s+service\s*==\s*'([^']+)'", command)
    if not match:
        raise ValueError(f"unsupported service query command: {command}")
    fields = [field.strip() for field in match.group(1).split(",") if field.strip()]
    service_name = match.group(2)
    return fields, service_name


def parse_filesystem_listdir(command: str):
    match = re.fullmatch(r"filesystem\s+listdir\s+path=(.+)", command)
    if not match:
        raise ValueError(f"unsupported filesystem listdir command: {command}")
    return match.group(1).strip().strip('"')


def parse_service_action(command: str):
    match = re.fullmatch(r"service\s+(restart|start|stop)\s+service=([a-z0-9_-]{1,64})", command)
    if not match:
        raise ValueError(f"unsupported service action command: {command}")
    return match.group(1), match.group(2)


def parse_filesystem_mutation(command: str):
    parts = shlex.split(command)
    if len(parts) < 4 or parts[0] != "filesystem" or parts[1] not in {"copy", "move"}:
        raise ValueError(f"unsupported filesystem mutation command: {command}")

    operation = parts[1]
    kv = {}
    for token in parts[2:]:
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        kv[key] = value

    if "src" not in kv or "dst" not in kv:
        raise ValueError(f"missing src/dst in command: {command}")

    src = kv["src"]
    dst = kv["dst"]
    recursive = kv.get("recursive", "false").lower() == "true"
    return operation, src, dst, recursive


def rows_to_csv(rows: list[dict], fields: list[str]) -> str:
    out = io.StringIO()
    writer = csv.DictWriter(out, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    for row in rows:
        writer.writerow({field: row.get(field, "") for field in fields})
    return out.getvalue()


def main() -> int:
    args = parse_args()
    ws = connect(args.url, args.user, args.password)
    try:
        command = args.command.strip()
        if command.startswith("service query "):
            fields, service_name = parse_service_query(command)
            result = call(ws, "service.query", [[ ["service", "=", service_name] ], {"select": fields, "order_by": ["service"]}])
            if args.mode == "csv":
                sys.stdout.write(rows_to_csv(result if isinstance(result, list) else [result], fields))
            else:
                sys.stdout.write(json.dumps(result, ensure_ascii=False))
            return 0

        if command.startswith("filesystem listdir "):
            path = parse_filesystem_listdir(command)
            result = call(ws, "filesystem.listdir", [path, [], {"limit": 5, "select": ["path", "type"]}])
            if args.mode == "csv":
                if result:
                    sys.stdout.write(rows_to_csv(result, ["path", "type"]))
                else:
                    sys.stdout.write(f"{path}\n")
            else:
                sys.stdout.write(json.dumps(result, ensure_ascii=False))
            return 0

        if command.startswith("service "):
            action, service_name = parse_service_action(command)
            result = call(ws, "service.control", [action.upper(), service_name, {}])
            if args.mode == "csv":
                sys.stdout.write("result\n")
                sys.stdout.write(f"{json.dumps(result, ensure_ascii=False)}\n")
            else:
                sys.stdout.write(json.dumps(result, ensure_ascii=False))
            return 0

        if command.startswith("filesystem copy ") or command.startswith("filesystem move "):
            operation, src, dst, recursive = parse_filesystem_mutation(command)
            if operation == "copy":
                result = call(ws, "filesystem.copy", [src, dst, {"recursive": recursive, "preserve_attrs": False}])
            else:
                result = call(ws, "filesystem.move", [[src], dst, {"recursive": recursive}])
            if args.mode == "csv":
                sys.stdout.write("result\n")
                sys.stdout.write(f"{json.dumps(result, ensure_ascii=False)}\n")
            else:
                sys.stdout.write(json.dumps(result, ensure_ascii=False))
            return 0

        raise ValueError(f"unsupported command: {command}")
    finally:
        ws.close()


if __name__ == "__main__":
    raise SystemExit(main())