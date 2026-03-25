#!/usr/bin/env python3
"""
AgentDiff capture script for Cursor hooks.
"""
import os
import sys
import json
from datetime import datetime, timezone


def debug_enabled() -> bool:
    return os.environ.get("AGENTDIFF_DEBUG", "").lower() in {"1", "true", "yes", "on"}


def debug_log(message: str) -> None:
    if not debug_enabled():
        return
    log_dir = os.path.expanduser("~/.agentdiff/logs")
    os.makedirs(log_dir, exist_ok=True)
    path = os.path.join(log_dir, "capture-cursor.log")
    ts = datetime.now(timezone.utc).isoformat()
    with open(path, "a", encoding="utf-8") as f:
        f.write(f"{ts} {message}\n")


def first(payload: dict, *keys, default=None):
    for key in keys:
        if key in payload and payload.get(key) is not None:
            return payload.get(key)
    return default


def find_repo_root(cwd: str) -> str:
    import subprocess
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, cwd=cwd
        )
        return result.stdout.strip() if result.returncode == 0 else cwd
    except Exception:
        return cwd


def get_session_log(cwd: str) -> str:
    override = os.environ.get("AGENTDIFF_SESSION_LOG")
    if override:
        parent = os.path.dirname(override)
        if parent:
            os.makedirs(parent, exist_ok=True)
        return override

    repo_root = find_repo_root(cwd)
    if os.path.exists(os.path.join(repo_root, ".git")):
        base = os.path.join(repo_root, ".git", "agentdiff")
        os.makedirs(base, exist_ok=True)
        return os.path.join(base, "session.jsonl")

    spill_root = os.environ.get("AGENTDIFF_SPILLOVER", os.path.expanduser("~/.agentdiff/spillover"))
    os.makedirs(spill_root, exist_ok=True)
    slug = cwd.replace("/", "-") or "unknown"
    return os.path.join(spill_root, f"{slug}.jsonl")


def get_cached_prompt(conversation_id: str) -> str:
    """Read cached prompt from beforeSubmitPrompt."""
    prompt_path = os.path.expanduser(f"~/.cursor/hooks/prompts/{conversation_id}.txt")
    if os.path.exists(prompt_path):
        with open(prompt_path) as f:
            return f.read().strip()
    return "unknown"


def main():
    input_data = sys.stdin.read()
    if not input_data.strip():
        sys.exit(0)
    debug_log(f"raw={input_data[:2000]}")

    try:
        payload = json.loads(input_data)
    except json.JSONDecodeError:
        sys.exit(0)

    event_name = first(payload, "hook_event_name", "hookEventName", "event_name", "event", default="")
    if event_name not in ["afterFileEdit", "afterTabFileEdit", "beforeSubmitPrompt"]:
        debug_log(f"skip: unknown event_name={event_name!r}")
        sys.exit(0)

    # Handle beforeSubmitPrompt - cache the prompt
    if event_name == "beforeSubmitPrompt":
        conversation_id = first(payload, "conversation_id", "conversationId", default="unknown")
        prompt = first(payload, "user_prompt", "userPrompt", "prompt", default="")
        prompt_dir = os.path.expanduser("~/.cursor/hooks/prompts")
        os.makedirs(prompt_dir, exist_ok=True)
        with open(os.path.join(prompt_dir, f"{conversation_id}.txt"), "w") as f:
            f.write(prompt)
        debug_log(f"cached prompt for conversation_id={conversation_id}")
        sys.exit(0)

    cwd = first(payload, "cwd", "workspace", "workspace_path", "workspacePath", default=os.getcwd())
    repo_root = find_repo_root(cwd)

    abs_file = first(payload, "file_path", "filePath", "path", default="")
    if not abs_file and isinstance(payload.get("file"), dict):
        abs_file = first(payload.get("file", {}), "path", "file_path", "filePath", default="")
    if not abs_file:
        debug_log("skip: missing abs_file")
        sys.exit(0)
    in_repo = os.path.exists(os.path.join(repo_root, ".git"))
    if in_repo and not abs_file.startswith(repo_root):
        sys.exit(0)

    # Get prompt for agent mode
    if event_name == "afterFileEdit":
        conversation_id = first(payload, "conversation_id", "conversationId", default="")
        prompt = get_cached_prompt(conversation_id) if conversation_id else "unknown"
        mode = "agent"
    else:  # afterTabFileEdit
        prompt = None
        mode = "tab"

    timestamp = datetime.now(timezone.utc).isoformat()

    # Model comes from payload in Cursor
    model = first(payload, "model", "model_name", "modelName", default="cursor-unknown")

    entry = {
        "timestamp": timestamp,
        "agent": "cursor",
        "mode": mode,
        "model": model,
        "session_id": first(payload, "conversation_id", "conversationId", default="unknown"),
        "tool": event_name,
        "file": abs_file[len(repo_root):].lstrip("/") if in_repo else abs_file,
        "abs_file": abs_file,
        "prompt": prompt,
        "acceptance": "verbatim",
    }

    # Line numbers from payload
    if event_name == "afterFileEdit":
        old_lines = first(payload, "old_lines", "oldLines", default=[])
        new_lines = first(payload, "new_lines", "newLines", default=[])
        if not isinstance(old_lines, list):
            old_lines = []
        if not isinstance(new_lines, list):
            new_lines = []
        if not new_lines and isinstance(payload.get("changes"), list):
            for ch in payload["changes"]:
                if not isinstance(ch, dict):
                    continue
                start = ch.get("startLine") or ch.get("line_start")
                end = ch.get("endLine") or ch.get("line_end") or start
                if isinstance(start, int) and isinstance(end, int):
                    new_lines.extend(list(range(min(start, end), max(start, end) + 1)))
        entry["lines"] = new_lines if new_lines else old_lines
    else:  # tab completion
        line_num = first(payload, "line_number", "lineNumber", default=1)
        entry["lines"] = [line_num if isinstance(line_num, int) else 1]

    session_log = get_session_log(cwd)
    with open(session_log, "a") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(f"wrote entry event={event_name} file={entry['file']} lines={entry.get('lines')}")


if __name__ == "__main__":
    main()
