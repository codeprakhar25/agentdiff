#!/usr/bin/env python3
"""
AgentDiff capture script for Codex notify hooks.
"""
import argparse
import glob
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from typing import Dict, List


def debug_enabled() -> bool:
    return os.environ.get("AGENTDIFF_DEBUG", "").lower() in {"1", "true", "yes", "on"}


def debug_log(message: str) -> None:
    if not debug_enabled():
        return
    log_dir = os.path.expanduser("~/.agentdiff/logs")
    os.makedirs(log_dir, exist_ok=True)
    path = os.path.join(log_dir, "capture-codex.log")
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


def extract_text(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        out = []
        for item in content:
            if isinstance(item, str):
                out.append(item)
            elif isinstance(item, dict):
                if isinstance(item.get("text"), str):
                    out.append(item["text"])
                elif item.get("type") in {"input_text", "output_text"} and isinstance(item.get("text"), str):
                    out.append(item["text"])
        return "\n".join([x for x in out if x])
    if isinstance(content, dict):
        txt = content.get("text")
        return txt if isinstance(txt, str) else ""
    return ""


def extract_prompt(payload: dict) -> str:
    for key in ("prompt", "user_prompt", "input", "message"):
        val = payload.get(key)
        if isinstance(val, str) and val.strip():
            return val.strip()

    messages = first(payload, "input-messages", "input_messages", "messages", default=[])
    if not isinstance(messages, list):
        return "unknown"

    for msg in reversed(messages):
        if not isinstance(msg, dict):
            continue
        role = msg.get("role")
        if role not in {"user", "system", None}:
            continue
        text = extract_text(msg.get("content"))
        if text.strip():
            return text.strip()
    return "unknown"


def find_codex_rollout(session_id: str) -> str:
    if not session_id:
        return ""
    root = os.path.expanduser("~/.codex/sessions")
    if not os.path.exists(root):
        return ""
    pattern = os.path.join(root, "**", f"*{session_id}.jsonl")
    matches = glob.glob(pattern, recursive=True)
    if not matches:
        return ""
    matches.sort(key=lambda p: os.path.getmtime(p), reverse=True)
    return matches[0]


def read_model_from_rollout(session_id: str) -> str:
    path = find_codex_rollout(session_id)
    if not path:
        return "codex"
    model = ""
    try:
        with open(path, "r", encoding="utf-8") as f:
            for line in f:
                try:
                    obj = json.loads(line)
                except Exception:
                    continue
                if obj.get("type") == "turn_context":
                    payload = obj.get("payload", {})
                    if isinstance(payload, dict) and isinstance(payload.get("model"), str):
                        model = payload["model"]
                elif obj.get("type") == "session_meta":
                    payload = obj.get("payload", {})
                    if isinstance(payload, dict) and isinstance(payload.get("model"), str):
                        model = payload["model"]
        return model or "codex"
    except Exception:
        return "codex"


def run_forward(forward_cmd, input_data: str) -> None:
    if not forward_cmd:
        return
    try:
        subprocess.run(forward_cmd, input=input_data, text=True)
    except Exception as e:
        debug_log(f"forward failed: {e}")


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--forward", default="")
    args = parser.parse_args()

    input_data = sys.stdin.read()
    if not input_data.strip():
        return 0
    debug_log(f"raw={input_data[:2000]}")

    forward_cmd = None
    if args.forward:
        try:
            parsed = json.loads(args.forward)
            if isinstance(parsed, list) and all(isinstance(p, str) for p in parsed):
                forward_cmd = parsed
        except Exception as e:
            debug_log(f"invalid --forward payload: {e}")

    try:
        payload = json.loads(input_data)
    except json.JSONDecodeError:
        run_forward(forward_cmd, input_data)
        return 0

    if not isinstance(payload, dict):
        run_forward(forward_cmd, input_data)
        return 0

    try:
        event_name = first(payload, "hook_event", "hookEvent", "event_name", "event", default="")
        if event_name and event_name not in {"agent-turn-complete", "agent_turn_complete"}:
            run_forward(forward_cmd, input_data)
            return 0

        cwd = first(payload, "cwd", "workspace", "workspace_path", "workspacePath", default=os.getcwd())
        repo_root = find_repo_root(cwd)
        in_repo = os.path.exists(os.path.join(repo_root, ".git"))
        if not in_repo:
            run_forward(forward_cmd, input_data)
            return 0

        changed = collect_changed_lines(repo_root)
        if not changed:
            run_forward(forward_cmd, input_data)
            return 0

        session_id = first(payload, "thread-id", "thread_id", "session_id", "sessionId", default="unknown")
        model = first(payload, "model", "model_name", "modelName", default="")
        if not model:
            model = read_model_from_rollout(str(session_id))
        prompt = extract_prompt(payload)
        timestamp = datetime.now(timezone.utc).isoformat()
        session_log = get_session_log(cwd)

        with open(session_log, "a", encoding="utf-8") as f:
            for file_path, lines in changed.items():
                abs_file = os.path.join(repo_root, file_path)
                entry = {
                    "timestamp": timestamp,
                    "agent": "codex",
                    "mode": "agent",
                    "model": model or "codex",
                    "session_id": str(session_id),
                    "tool": event_name or "agent-turn-complete",
                    "file": file_path,
                    "abs_file": abs_file,
                    "prompt": prompt,
                    "acceptance": "verbatim",
                    "lines": lines,
                }
                f.write(json.dumps(entry) + "\n")

        debug_log(f"wrote {len(changed)} codex entries")
    finally:
        run_forward(forward_cmd, input_data)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
