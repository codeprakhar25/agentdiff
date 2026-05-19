#!/usr/bin/env python3
"""
AgentDiff capture script for VS Code GitHub Copilot.
Receives events from the agentdiff-copilot VS Code extension via stdin.
Writes to <repo>/.git/agentdiff/session.jsonl.

Supported capture modes
-----------------------
``manual`` (confidence="high")
    Triggered by the ``agentdiff.captureNow`` command.  The user explicitly
    declares the current file as Copilot-authored after a Chat session.  All
    lines in the file are recorded.  This is the only mode that produces
    deterministic, reproducible attribution.

``inline_heuristic`` (confidence="low")
    Triggered by VS Code's ``onDidChangeTextDocument`` event whenever an
    insertion exceeds the extension's length threshold.  This fires on edits
    from *any* source — other AI agents running in the terminal, human
    copy-paste, IDE refactors — not only Copilot.  Use only as a hint;
    never treat it as a reliable attribution signal.

``save_flush`` (confidence="low")
    Same heuristic as ``inline_heuristic``, flushed on file save rather than
    a debounce timer.  Same caveats apply.

``chat_edit`` (confidence="low")
    Reserved for a future VS Code API that identifies Copilot Chat edits
    directly.  Not currently emitted by the extension.

Because Copilot is in ``_EXCLUDED_AGENTS`` in prepare-ledger.py, captured
events never win per-file attribution.  They are recorded in session.jsonl
for usage statistics and surfaced via the ``copilot_context`` field in traces.
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

    # Pass through confidence and capture_mode from the extension payload.
    # These fields distinguish reliable captures (manual command) from heuristic
    # ones (inline change detection) so downstream consumers can weight them
    # appropriately.  Defaults to "low"/"inline_heuristic" for backwards
    # compatibility with extension versions that predate this field.
    confidence = payload.get("confidence") or "low"
    capture_mode = payload.get("capture_mode") or "inline_heuristic"

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
        "confidence": confidence,
        "capture_mode": capture_mode,
    }

    session_log = get_session_log(cwd)
    if session_log is None:
        debug_log(f"skip: agentdiff init not run in {repo_root!r}")
        return
    with open(session_log, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(
        f"wrote entry tool={tool} file={entry['file']} lines={entry.get('lines')} "
        f"confidence={confidence} capture_mode={capture_mode} "
        f"repo_root={repo_root!r} session_log={session_log!r}"
    )


if __name__ == "__main__":
    main()
