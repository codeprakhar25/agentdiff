#!/usr/bin/env python3
"""
AgentDiff capture script for OpenCode plugin hooks.
"""
import json
import os
import subprocess
import sys
from datetime import datetime, timezone


def debug_enabled() -> bool:
    return os.environ.get("AGENTDIFF_DEBUG", "").lower() in {"1", "true", "yes", "on"}


def debug_log(message: str) -> None:
    if not debug_enabled():
        return
    log_dir = os.path.expanduser("~/.agentdiff/logs")
    os.makedirs(log_dir, exist_ok=True)
    path = os.path.join(log_dir, "capture-opencode.log")
    ts = datetime.now(timezone.utc).isoformat()
    with open(path, "a", encoding="utf-8") as f:
        f.write(f"{ts} {message}\n")


def first(payload: dict, *keys, default=None):
    for key in keys:
        if key in payload and payload.get(key) is not None:
            return payload.get(key)
    return default


def find_repo_root(cwd: str) -> str:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            cwd=cwd,
        )
        return result.stdout.strip() if result.returncode == 0 else cwd
    except Exception:
        return cwd


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


def compute_line_range(abs_file: str, old_content: str, new_content: str):
    try:
        with open(abs_file, "r", encoding="utf-8") as f:
            current = f.read()

        old_lines = set()
        new_lines = set()

        for i, line in enumerate(current.split("\n"), 1):
            if old_content and old_content in line:
                old_lines.add(i)
            if new_content and new_content in line:
                new_lines.add(i)

        lines = sorted(old_lines | new_lines)
        if lines:
            return lines
    except Exception:
        pass

    if new_content:
        return list(range(1, new_content.count("\n") + 2))
    return [1]


def main() -> int:
    input_data = sys.stdin.read()
    if not input_data.strip():
        return 0
    debug_log(f"raw={input_data[:2000]}")

    try:
        payload = json.loads(input_data)
    except json.JSONDecodeError:
        return 0
    if not isinstance(payload, dict):
        return 0

    event_name = first(payload, "hook_event_name", "hookEventName", "event_name", "event", default="")
    if event_name not in {"PostToolUse", "post_tool_use"}:
        return 0

    cwd = first(payload, "cwd", default=os.getcwd())
    repo_root = find_repo_root(cwd)

    tool_name = str(first(payload, "tool_name", "toolName", "tool", default="unknown"))
    tool_input = first(payload, "tool_input", "toolInput", default={})
    if not isinstance(tool_input, dict):
        tool_input = {}

    abs_file = first(tool_input, "filePath", "file_path", "path", default="")
    if not abs_file:
        return 0
    if not os.path.isabs(abs_file):
        abs_file = os.path.abspath(os.path.join(cwd, abs_file))
    if os.path.exists(os.path.join(repo_root, ".git")) and not abs_file.startswith(repo_root):
        return 0

    rel_file = abs_file[len(repo_root):].lstrip("/") if abs_file.startswith(repo_root) else abs_file
    session_id = str(first(payload, "session_id", "sessionId", default="unknown"))
    model = str(first(payload, "model", "modelID", "model_id", default="opencode"))
    prompt = first(payload, "prompt", "user_prompt", "userPrompt", default="unknown")

    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "agent": "opencode",
        "mode": "agent",
        "model": model,
        "session_id": session_id,
        "tool": tool_name,
        "file": rel_file,
        "abs_file": abs_file,
        "prompt": prompt if isinstance(prompt, str) else "unknown",
        "acceptance": "verbatim",
    }

    tool_lower = tool_name.lower()
    if tool_lower in {"edit", "patch", "replace"}:
        old_str = str(first(tool_input, "old_string", "oldString", "old", default=""))
        new_str = str(first(tool_input, "new_string", "newString", "new", default=""))
        entry["old"] = old_str[:200]
        entry["new"] = new_str[:200]
        entry["lines"] = compute_line_range(abs_file, old_str, new_str)
    elif tool_lower == "write":
        content = str(first(tool_input, "content", default=""))
        entry["content_preview"] = content[:200]
        entry["total_lines"] = content.count("\n") + 1
        entry["lines"] = list(range(1, content.count("\n") + 2))
    elif tool_lower == "multiedit":
        edits = first(tool_input, "edits", default=[])
        if not isinstance(edits, list):
            edits = []
        entry["edit_count"] = len(edits)
        out_edits = []
        all_lines = []
        for edit in edits:
            if not isinstance(edit, dict):
                continue
            old_str = str(first(edit, "old_string", "oldString", "old", default=""))
            new_str = str(first(edit, "new_string", "newString", "new", default=""))
            out_edits.append({"old": old_str[:100], "new": new_str[:100]})
            all_lines.extend(compute_line_range(abs_file, old_str, new_str))
        entry["edits"] = out_edits
        entry["lines"] = sorted(set(all_lines)) if all_lines else [1]
    else:
        line_num = first(tool_input, "line", "lineNumber", "line_number", default=1)
        entry["lines"] = [int(line_num) if isinstance(line_num, int) and line_num > 0 else 1]

    session_log = get_session_log(cwd)
    if session_log is None:
        debug_log("skip: agentdiff init not run in this repo")
        return 0
    with open(session_log, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(f"wrote entry tool={tool_name} file={rel_file}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
