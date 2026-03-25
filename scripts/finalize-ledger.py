#!/usr/bin/env python3
"""
Finalize pending ledger snapshot after commit and append to ledger.jsonl.

Usage:
  finalize-ledger.py <repo_root> <pending_ledger> <pending_context> <ledger_path>
"""
import json
import os
import subprocess
import sys
from typing import List


def run(cmd: List[str], cwd: str) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)


def read_json_file(path: str):
    if not os.path.exists(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return None


def remove_if_exists(path: str) -> None:
    try:
        if os.path.exists(path):
            os.remove(path)
    except Exception:
        pass


def sha_exists(ledger_path: str, sha: str) -> bool:
    if not os.path.exists(ledger_path):
        return False
    try:
        with open(ledger_path, "r", encoding="utf-8") as f:
            for raw in f:
                line = raw.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except Exception:
                    continue
                if isinstance(obj, dict) and obj.get("sha") == sha:
                    return True
    except Exception:
        return False
    return False


def main() -> int:
    if len(sys.argv) < 5:
        print(
            "usage: finalize-ledger.py <repo_root> <pending_ledger> <pending_context> <ledger_path>",
            file=sys.stderr,
        )
        return 2

    repo_root = os.path.abspath(sys.argv[1])
    pending_ledger_path = os.path.abspath(sys.argv[2])
    pending_context_path = os.path.abspath(sys.argv[3])
    ledger_path = os.path.abspath(sys.argv[4])

    if not os.path.exists(os.path.join(repo_root, ".git")):
        return 0

    pending = read_json_file(pending_ledger_path)
    if not isinstance(pending, dict):
        return 0

    sha_res = run(["git", "rev-parse", "HEAD"], cwd=repo_root)
    if sha_res.returncode != 0:
        return 1
    sha = sha_res.stdout.strip()
    if not sha:
        return 1

    if sha_exists(ledger_path, sha):
        remove_if_exists(pending_ledger_path)
        remove_if_exists(pending_context_path)
        return 0

    ts_res = run(["git", "show", "-s", "--format=%cI", "HEAD"], cwd=repo_root)
    if ts_res.returncode != 0:
        return 1
    ts = ts_res.stdout.strip()

    author_res = run(["git", "show", "-s", "--format=%an", "HEAD"], cwd=repo_root)
    author = author_res.stdout.strip() if author_res.returncode == 0 else ""

    entry = {
        "sha": sha,
        "ts": ts,
        "agent": str(pending.get("agent") or "human"),
        "model": str(pending.get("model") or "human"),
        "session_id": str(pending.get("session_id") or "unknown"),
        "author": author or None,
        "files_touched": pending.get("files_touched") if isinstance(pending.get("files_touched"), list) else [],
        "lines": pending.get("lines") if isinstance(pending.get("lines"), dict) else {},
        "prompt_excerpt": str(pending.get("prompt_excerpt") or ""),
        "prompt_hash": str(pending.get("prompt_hash") or ""),
        "files_read": pending.get("files_read") if isinstance(pending.get("files_read"), list) else [],
        "flags": pending.get("flags") if isinstance(pending.get("flags"), list) else [],
        "tool": str(pending.get("tool") or "commit"),
        "mode": pending.get("mode"),
    }

    if pending.get("intent"):
        entry["intent"] = str(pending.get("intent"))
    if isinstance(pending.get("trust"), int):
        entry["trust"] = max(0, min(100, int(pending["trust"])))

    parent = os.path.dirname(ledger_path)
    if parent:
        os.makedirs(parent, exist_ok=True)

    entry = {k: v for (k, v) in entry.items() if v is not None}

    with open(ledger_path, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry, separators=(",", ":")) + "\n")

    remove_if_exists(pending_ledger_path)
    remove_if_exists(pending_context_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
