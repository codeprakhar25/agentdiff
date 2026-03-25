#!/usr/bin/env python3
"""
Record MCP context into .git/agentdiff/pending.json

Usage:
  record-context.py [--cwd <path>] [--agent <name>] [--model-id <id>] [--session-id <id>]
                    [--prompt <text>] [--files-read <json-array>] [--intent <text>]
                    [--trust <0-100>] [--flags <json-array>]

If stdin contains JSON object, it is used as payload and CLI flags override fields.
"""
import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone


def find_repo_root(cwd: str) -> str:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            cwd=cwd,
        )
        return result.stdout.strip() if result.returncode == 0 else ""
    except Exception:
        return ""


def parse_json_array(raw: str):
    if not raw:
        return []
    try:
        data = json.loads(raw)
        if isinstance(data, list):
            return data
    except Exception:
        pass
    return []


def main() -> int:
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument("--cwd", default=os.getcwd())
    parser.add_argument("--agent", default="")
    parser.add_argument("--model-id", default="")
    parser.add_argument("--session-id", default="")
    parser.add_argument("--prompt", default="")
    parser.add_argument("--files-read", default="")
    parser.add_argument("--intent", default="")
    parser.add_argument("--trust", type=int, default=None)
    parser.add_argument("--flags", default="")
    args = parser.parse_args()

    payload = {}
    stdin = sys.stdin.read()
    if stdin.strip():
        try:
            obj = json.loads(stdin)
            if isinstance(obj, dict):
                payload = obj
        except Exception:
            payload = {}

    repo_root = find_repo_root(args.cwd or os.getcwd())
    if not repo_root:
        return 0

    pending = {
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "agent": args.agent or str(payload.get("agent") or ""),
        "model_id": args.model_id or str(payload.get("model_id") or payload.get("model") or ""),
        "session_id": args.session_id or str(payload.get("session_id") or ""),
        "prompt": args.prompt or str(payload.get("prompt") or ""),
        "files_read": parse_json_array(args.files_read) or payload.get("files_read") or [],
        "intent": args.intent or str(payload.get("intent") or ""),
        "flags": parse_json_array(args.flags) or payload.get("flags") or [],
    }

    trust = args.trust if args.trust is not None else payload.get("trust")
    if isinstance(trust, int):
        pending["trust"] = max(0, min(100, trust))

    session_dir = os.path.join(repo_root, ".git", "agentdiff")
    os.makedirs(session_dir, exist_ok=True)
    out_path = os.path.join(session_dir, "pending.json")
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(pending, f, separators=(",", ":"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
