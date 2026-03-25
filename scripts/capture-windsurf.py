#!/usr/bin/env python3
"""
AgentDiff capture script for Windsurf hooks.
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
    path = os.path.join(log_dir, "capture-windsurf.log")
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


def prompt_cache_path(trajectory_id: str) -> str:
    cache_root = os.path.expanduser("~/.agentdiff/windsurf/prompts")
    os.makedirs(cache_root, exist_ok=True)
    return os.path.join(cache_root, f"{trajectory_id}.txt")


def cache_prompt(trajectory_id: str, prompt: str) -> None:
    if not trajectory_id:
        return
    path = prompt_cache_path(trajectory_id)
    with open(path, "w", encoding="utf-8") as f:
        f.write(prompt or "")


def get_cached_prompt(trajectory_id: str) -> str:
    if not trajectory_id:
        return "unknown"
    path = prompt_cache_path(trajectory_id)
    if not os.path.exists(path):
        return "unknown"
    try:
        with open(path, "r", encoding="utf-8") as f:
            text = f.read().strip()
            return text or "unknown"
    except Exception:
        return "unknown"


def read_model_from_transcript(trajectory_id: str) -> str:
    if not trajectory_id:
        return "windsurf"
    path = os.path.expanduser(f"~/.windsurf/transcripts/{trajectory_id}.jsonl")
    if not os.path.exists(path):
        return "windsurf"

    model = ""
    try:
        with open(path, "r", encoding="utf-8") as f:
            for line in f:
                try:
                    obj = json.loads(line)
                except Exception:
                    continue
                for key in ("model", "model_name", "modelName", "selectedModel", "identifier"):
                    value = obj.get(key)
                    if isinstance(value, str) and value:
                        model = value
        return model or "windsurf"
    except Exception:
        return "windsurf"


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
    trajectory_id = first(payload, "trajectory_id", "trajectoryId", "session_id", "sessionId", default="unknown")
    prompt = first(payload, "prompt", "user_prompt", "userPrompt", default="")

    # Cache prompt from transcript event so post_write_code has context.
    if event_name == "post_cascade_response_with_transcript":
        if isinstance(prompt, str) and prompt.strip():
            cache_prompt(str(trajectory_id), prompt.strip())
        return 0

    if event_name != "post_write_code":
        return 0

    cwd = first(payload, "cwd", "workspace", "workspace_path", "workspacePath", default=os.getcwd())
    repo_root = find_repo_root(cwd)
    if not os.path.exists(os.path.join(repo_root, ".git")):
        return 0

    abs_file = first(payload, "filepath", "file_path", "filePath", "path", default="")
    if not abs_file:
        return 0
    if not os.path.isabs(abs_file):
        abs_file = os.path.abspath(os.path.join(cwd, abs_file))
    if not abs_file.startswith(repo_root):
        return 0

    old_str = first(payload, "old_str", "old_string", "oldString", default="")
    new_str = first(payload, "new_str", "new_string", "newString", default="")
    lines = compute_line_range(abs_file, str(old_str), str(new_str))

    model = first(payload, "model", "model_name", "modelName", default="")
    if not model:
        model = read_model_from_transcript(str(trajectory_id))
    if not prompt:
        prompt = get_cached_prompt(str(trajectory_id))

    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "agent": "windsurf",
        "mode": "agent",
        "model": model or "windsurf",
        "session_id": str(trajectory_id),
        "tool": event_name,
        "file": abs_file[len(repo_root):].lstrip("/"),
        "abs_file": abs_file,
        "prompt": prompt if isinstance(prompt, str) else "unknown",
        "acceptance": "verbatim",
        "lines": lines,
        "old": str(old_str)[:200] if old_str else None,
        "new": str(new_str)[:200] if new_str else None,
    }
    if entry["old"] is None:
        entry.pop("old")
    if entry["new"] is None:
        entry.pop("new")

    session_log = get_session_log(cwd)
    with open(session_log, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(f"wrote entry file={entry['file']} lines={len(lines)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
