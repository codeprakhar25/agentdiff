#!/usr/bin/env python3
"""
AgentDiff capture script for Antigravity / Gemini hooks and batch agents.
"""
import argparse
from datetime import datetime, timezone
from typing import Dict, List

import os
import re
import sys
import json
import subprocess


def find_repo_root(cwd: str) -> str:
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


def first(payload: dict, *keys, default=None):
    for key in keys:
        if key in payload and payload.get(key) is not None:
            return payload.get(key)
    return default


def parse_json_or_jsonl(text: str):
    raw = (text or "").strip()
    if not raw:
        return None

    try:
        obj = json.loads(raw)
        if isinstance(obj, dict):
            return obj
        if isinstance(obj, list):
            for item in reversed(obj):
                if isinstance(item, dict):
                    return item
            return None
    except Exception:
        pass

    for line in reversed(raw.splitlines()):
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
            if isinstance(obj, dict):
                return obj
        except Exception:
            continue
    return None


def is_git_repo(path: str) -> bool:
    return bool(path) and os.path.exists(os.path.join(path, ".git"))


def parse_diff_added_lines(diff_text: str) -> Dict[str, List[int]]:
    changed: Dict[str, List[int]] = {}
    current_file = ""
    current_line = 0
    in_hunk = False

    for raw in diff_text.splitlines():
        if raw.startswith("diff --git "):
            parts = raw.split()
            if len(parts) >= 4:
                path = parts[3]
                if path.startswith("b/"):
                    path = path[2:]
                current_file = path
                changed.setdefault(current_file, [])
                in_hunk = False
            continue

        if raw.startswith("@@"):
            m = re.search(r"\+(\d+)(?:,\d+)?", raw)
            if m:
                current_line = int(m.group(1))
                in_hunk = True
            continue

        if not in_hunk or not current_file:
            continue

        if raw.startswith("+") and not raw.startswith("+++"):
            changed[current_file].append(current_line)
            current_line += 1
            continue

        if raw.startswith("-") and not raw.startswith("---"):
            continue

        current_line += 1

    return {k: sorted(set(v)) for k, v in changed.items() if v}


def collect_changed_lines(repo_root: str) -> Dict[str, List[int]]:
    result: Dict[str, List[int]] = {}
    commands = [
        ["git", "diff", "--no-color", "--unified=0"],
        ["git", "diff", "--cached", "--no-color", "--unified=0"],
        ["git", "diff", "HEAD", "--no-color", "--unified=0"],
    ]
    for cmd in commands:
        try:
            out = subprocess.run(cmd, capture_output=True, text=True, cwd=repo_root)
        except Exception:
            continue
        if out.returncode != 0 or not out.stdout.strip():
            continue
        parsed = parse_diff_added_lines(out.stdout)
        for path, lines in parsed.items():
            result.setdefault(path, [])
            result[path].extend(lines)

    return {k: sorted(set(v)) for k, v in result.items() if v}


def cache_root() -> str:
    root = os.path.expanduser("~/.agentdiff/antigravity/prompts")
    os.makedirs(root, exist_ok=True)
    return root


def prompt_cache_path(session_id: str) -> str:
    sid = session_id or "unknown"
    return os.path.join(cache_root(), f"{sid}.txt")


def cache_prompt(session_id: str, prompt: str) -> None:
    if not prompt:
        return
    with open(prompt_cache_path(session_id), "w", encoding="utf-8") as f:
        f.write(prompt)


def get_cached_prompt(session_id: str) -> str:
    path = prompt_cache_path(session_id)
    if not os.path.exists(path):
        return ""
    try:
        with open(path, "r", encoding="utf-8") as f:
            return f.read().strip()
    except Exception:
        return ""


def normalize_abs_path(path: str, cwd: str) -> str:
    if not path:
        return ""
    p = os.path.expanduser(str(path))
    if os.path.isabs(p):
        return os.path.abspath(p)
    return os.path.abspath(os.path.join(cwd, p))


def compute_line_range(abs_file: str, old_content: str, new_content: str) -> List[int]:
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


def resolve_payload_context(payload: dict) -> dict:
    event_name = str(
        first(payload, "hook_event_name", "hookEventName", "event_name", "event", "type", default="")
    )
    tool_name = str(first(payload, "tool_name", "toolName", "tool", default=""))
    tool_info = payload.get("tool_input")
    if not isinstance(tool_info, dict):
        tool_info = payload.get("toolInput")
    if not isinstance(tool_info, dict):
        tool_info = payload.get("tool_info")
    if not isinstance(tool_info, dict):
        tool_info = {}

    file_path = str(
        first(
            tool_info,
            "file_path",
            "filePath",
            "path",
            "filepath",
            "target",
            default=first(payload, "file_path", "filePath", "path", "filepath", default=""),
        )
    )
    old_str = str(first(tool_info, "old_string", "oldString", "old", default=first(payload, "old_string", "oldString", "old", default="")))
    new_str = str(first(tool_info, "new_string", "newString", "new", default=first(payload, "new_string", "newString", "new", default="")))

    session_id = str(
        first(
            payload,
            "session_id",
            "sessionId",
            "conversation_id",
            "conversationId",
            "trajectory_id",
            "trajectoryId",
            default="",
        )
    )
    prompt = str(first(payload, "prompt", "user_prompt", "userPrompt", "input", "message", default=""))
    model = str(first(payload, "model", "model_name", "modelName", "model_id", "modelId", default=""))
    cwd = str(first(payload, "cwd", "workspace", "workspace_path", "workspacePath", default=os.getcwd()))

    return {
        "event_name": event_name,
        "tool_name": tool_name,
        "file_path": file_path,
        "old_str": old_str,
        "new_str": new_str,
        "session_id": session_id,
        "prompt": prompt,
        "model": model,
        "cwd": cwd,
    }


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--prompt", default="")
    parser.add_argument("--model", default="")
    args = parser.parse_args()

    raw_stdin = sys.stdin.read()

    # Always write a fire-marker (helps diagnose silent failures).
    try:
        marker_dir = os.path.expanduser("~/.agentdiff/logs")
        os.makedirs(marker_dir, exist_ok=True)
        with open(os.path.join(marker_dir, "antigravity-hook-fired.log"), "a") as mf:
            ts = datetime.now(timezone.utc).isoformat()
            mf.write(f"{ts} stdin_len={len(raw_stdin)} first200={raw_stdin[:200]}\n")
    except Exception:
        pass

    stdin_payload = parse_json_or_jsonl(raw_stdin)

    if isinstance(stdin_payload, dict):
        ctx = resolve_payload_context(stdin_payload)
        event_name = (ctx.get("event_name") or "").strip()
        tool_name = (ctx.get("tool_name") or "").strip()
        session_id = (ctx.get("session_id") or "").strip()
        prompt = (ctx.get("prompt") or "").strip()

        event_lower = event_name.lower()
        tool_lower = tool_name.lower()

        # Use BeforeTool hooks to cache prompt for subsequent AfterTool writes.
        if event_lower in {"beforetool", "before_tool", "before-tool"}:
            if session_id and prompt:
                cache_prompt(session_id, prompt)
            return 0

        # Ignore non-write events when event names are explicit.
        if event_lower in {"aftertool", "after_tool", "after-tool"}:
            if tool_lower and tool_lower not in {"write_file", "replace", "edit", "write", "multiedit"}:
                return 0

        cwd = ctx.get("cwd") or os.getcwd()
        repo_root = find_repo_root(cwd)

        abs_file = normalize_abs_path(ctx.get("file_path") or "", cwd)
        if not is_git_repo(repo_root) and abs_file:
            repo_root = find_repo_root(os.path.dirname(abs_file))
        if not is_git_repo(repo_root):
            return 0

        changed: Dict[str, List[int]] = {}
        if abs_file and abs_file.startswith(repo_root):
            rel_file = abs_file[len(repo_root):].lstrip("/")
            if rel_file:
                changed[rel_file] = compute_line_range(
                    abs_file,
                    ctx.get("old_str") or "",
                    ctx.get("new_str") or "",
                )

        if not changed:
            changed = collect_changed_lines(repo_root)
        if not changed:
            return 0

        if not session_id:
            session_id = f"antigravity-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
        model = (ctx.get("model") or "").strip() or args.model or "gemini"
        prompt = prompt or get_cached_prompt(session_id) or args.prompt or "unknown"
        tool = tool_name or event_name or "batch-edit"
        timestamp = datetime.now(timezone.utc).isoformat()

        session_log = get_session_log(cwd)
        with open(session_log, "a", encoding="utf-8") as f:
            for file_path, lines in changed.items():
                entry = {
                    "timestamp": timestamp,
                    "agent": "antigravity",
                    "mode": "agent",
                    "model": model,
                    "session_id": session_id,
                    "tool": tool,
                    "file": file_path,
                    "abs_file": os.path.join(repo_root, file_path),
                    "prompt": prompt,
                    "acceptance": "verbatim",
                    "lines": sorted(set(lines)),
                }
                f.write(json.dumps(entry) + "\n")
        return 0

    # Manual fallback mode: allow direct CLI use in any git repo.
    cwd = os.getcwd()
    repo_root = find_repo_root(cwd)
    if not is_git_repo(repo_root):
        print("Not in a git repository", file=sys.stderr)
        return 1

    changed = collect_changed_lines(repo_root)
    if not changed:
        return 0

    timestamp = datetime.now(timezone.utc).isoformat()
    session_id = f"antigravity-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
    model = args.model or "antigravity"
    prompt = args.prompt or "unknown"
    session_log = get_session_log(cwd)

    with open(session_log, "a", encoding="utf-8") as f:
        for file_path, lines in changed.items():
            entry = {
                "timestamp": timestamp,
                "agent": "antigravity",
                "mode": "agent",
                "model": model,
                "session_id": session_id,
                "tool": "batch-edit",
                "file": file_path,
                "abs_file": os.path.join(repo_root, file_path),
                "prompt": prompt,
                "acceptance": "verbatim",
                "lines": sorted(set(lines)),
            }
            f.write(json.dumps(entry) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
