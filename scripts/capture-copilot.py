#!/usr/bin/env python3
"""
AgentDiff capture script for VS Code GitHub Copilot.
Receives events from the agentdiff-copilot VS Code extension via stdin.
Writes to <repo>/.git/agentdiff/session.jsonl.
"""
import os
import sys
import json
import subprocess
from datetime import datetime, timezone


def debug_enabled() -> bool:
    return os.environ.get("AGENTDIFF_DEBUG", "").lower() in {"1", "true", "yes", "on"}


def debug_log(message: str) -> None:
    if not debug_enabled():
        return
    log_dir = os.path.expanduser("~/.agentdiff/logs")
    os.makedirs(log_dir, exist_ok=True)
    path = os.path.join(log_dir, "capture-copilot.log")
    ts = datetime.now(timezone.utc).isoformat()
    with open(path, "a", encoding="utf-8") as f:
        f.write(f"{ts} {message}\n")


def find_repo_root(cwd: str) -> str:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, cwd=cwd
        )
        return result.stdout.strip() if result.returncode == 0 else cwd
    except Exception:
        return cwd


def is_git_repo(path: str) -> bool:
    return bool(path) and os.path.exists(os.path.join(path, ".git"))


def get_session_log(cwd: str):
    """Return session log path, or None if agentdiff init has not been run here."""
    override = os.environ.get("AGENTDIFF_SESSION_LOG")
    if override:
        parent = os.path.dirname(override)
        if parent:
            os.makedirs(parent, exist_ok=True)
        return override

    repo_root = find_repo_root(cwd)
    base = os.path.join(repo_root, ".git", "agentdiff")
    if os.path.isdir(base):
        return os.path.join(base, "session.jsonl")

    return None


def main():
    input_data = sys.stdin.read()
    if not input_data.strip():
        sys.exit(0)
    debug_log(f"raw={input_data[:2000]}")

    try:
        payload = json.loads(input_data)
    except json.JSONDecodeError:
        sys.exit(0)

    abs_file = payload.get("file_path") or ""
    if not abs_file:
        debug_log("skip: missing file_path")
        sys.exit(0)

    cwd = payload.get("cwd") or os.path.dirname(abs_file) or os.getcwd()
    repo_root = find_repo_root(cwd)
    in_repo = is_git_repo(repo_root)

    # Resolve relative file path within repo
    if in_repo:
        try:
            rel_file = os.path.relpath(abs_file, repo_root)
        except ValueError:
            rel_file = abs_file
    else:
        rel_file = abs_file

    event = payload.get("event", "inline")
    tool_map = {
        "inline":      "copilot-inline",
        "save":        "copilot-save",
        "chat_edit":   "copilot-chat",
        "manual":      "copilot-manual",
    }
    tool = tool_map.get(event, f"copilot-{event}")

    lines = payload.get("lines") or []
    if not isinstance(lines, list):
        lines = []

    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "agent": "copilot",
        "model": payload.get("model") or "copilot-unknown",
        "session_id": payload.get("session_id") or "unknown",
        "tool": tool,
        "file": rel_file,
        "abs_file": abs_file,
        "prompt": payload.get("prompt"),
        "acceptance": "verbatim",
        "lines": lines,
    }

    session_log = get_session_log(cwd)
    if session_log is None:
        debug_log(f"skip: agentdiff init not run in {repo_root!r}")
        return
    with open(session_log, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(
        f"wrote entry tool={tool} file={entry['file']} lines={entry.get('lines')} "
        f"repo_root={repo_root!r} session_log={session_log!r}"
    )


if __name__ == "__main__":
    main()
