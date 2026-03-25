#!/usr/bin/env python3
"""
AgentDiff capture script for Cursor hooks.
"""
import os
import sys
import json
import re
import subprocess
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
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, cwd=cwd
        )
        return result.stdout.strip() if result.returncode == 0 else cwd
    except Exception:
        return cwd


def normalize_path(path: str, cwd: str = "") -> str:
    if not isinstance(path, str):
        return ""
    p = path.strip()
    if not p:
        return ""

    p = p.replace("\\", "/")
    p = re.sub(r"/{2,}", "/", p)
    p = os.path.expanduser(p)

    if os.path.isabs(p):
        return os.path.abspath(p)
    if cwd:
        return os.path.abspath(os.path.join(cwd, p))
    return os.path.abspath(p)


def is_git_repo(path: str) -> bool:
    return bool(path) and os.path.exists(os.path.join(path, ".git"))


def find_repo_root_from_path(path: str) -> str:
    if not path:
        return ""
    start = path if os.path.isdir(path) else os.path.dirname(path)
    if not start:
        return ""
    root = find_repo_root(start)
    if is_git_repo(root):
        return root
    return ""


def is_within_repo(path: str, repo_root: str) -> bool:
    if not path or not repo_root:
        return False
    try:
        return os.path.commonpath([os.path.abspath(path), os.path.abspath(repo_root)]) == os.path.abspath(repo_root)
    except Exception:
        return False


def get_session_log(cwd: str, repo_root_hint: str = "") -> str:
    override = os.environ.get("AGENTDIFF_SESSION_LOG")
    if override:
        parent = os.path.dirname(override)
        if parent:
            os.makedirs(parent, exist_ok=True)
        return override

    repo_root = repo_root_hint or find_repo_root(cwd)
    if is_git_repo(repo_root):
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

    cwd_raw = first(payload, "cwd", "workspace", "workspace_path", "workspacePath", default=os.getcwd())
    cwd = normalize_path(str(cwd_raw), os.getcwd())
    repo_root = find_repo_root(cwd)

    abs_file = first(payload, "file_path", "filePath", "path", default="")
    if not abs_file and isinstance(payload.get("file"), dict):
        abs_file = first(payload.get("file", {}), "path", "file_path", "filePath", default="")
    if not abs_file:
        debug_log("skip: missing abs_file")
        sys.exit(0)

    abs_file = normalize_path(str(abs_file), cwd)
    if not abs_file:
        debug_log("skip: invalid abs_file after normalize")
        sys.exit(0)

    file_repo_root = find_repo_root_from_path(abs_file)
    if file_repo_root:
        repo_root = file_repo_root

    in_repo = is_git_repo(repo_root)
    if in_repo and not is_within_repo(abs_file, repo_root):
        debug_log(f"skip: file outside repo abs_file={abs_file!r} repo_root={repo_root!r}")
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
        "file": os.path.relpath(abs_file, repo_root) if in_repo else abs_file,
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

    session_log = get_session_log(cwd, repo_root if in_repo else "")
    with open(session_log, "a") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(
        "wrote entry "
        f"event={event_name} file={entry['file']} lines={entry.get('lines')} "
        f"cwd={cwd!r} repo_root={repo_root!r} session_log={session_log!r}"
    )


if __name__ == "__main__":
    main()
