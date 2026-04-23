#!/usr/bin/env python3
"""
AgentDiff capture script for Claude Code PostToolUse hook.
Writes to <repo>/.git/agentdiff/session.jsonl only when agentdiff init has been
run in that repo (i.e. .git/agentdiff/ exists). Exits silently otherwise.
"""
import os
import sys
import json
import subprocess
from datetime import datetime, timezone


def debug_enabled() -> bool:
    return os.environ.get("AGENTDIFF_DEBUG", "").lower() in {"1", "true", "yes", "on"}


def capture_prompts_enabled() -> bool:
    """Read capture_prompts setting from ~/.agentdiff/config.toml. Defaults to True."""
    config_path = os.path.expanduser("~/.agentdiff/config.toml")
    try:
        with open(config_path, encoding="utf-8") as f:
            content = f.read()
        # Look for capture_prompts = false (simple line-level check, no full TOML parse needed)
        for line in content.splitlines():
            stripped = line.strip().replace(" ", "").lower()
            if stripped.startswith("capture_prompts="):
                # Strip inline TOML comments before comparing the value.
                val = stripped.split("=", 1)[1].split("#")[0].strip()
                return val not in ("false", "0", "no", "off")
    except (OSError, IOError):
        pass
    return True


def debug_log(message: str) -> None:
    if not debug_enabled():
        return
    log_dir = os.path.expanduser("~/.agentdiff/logs")
    os.makedirs(log_dir, exist_ok=True)
    path = os.path.join(log_dir, "capture-claude.log")
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


def get_session_log(cwd: str):
    """Return session log path, or None if agentdiff init has not been run here.

    Capture is opt-in per-repo: agentdiff init creates .git/agentdiff/ which is
    the signal that this repo should be tracked. Without that directory the hook
    exits silently — no data is written anywhere.
    """
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

    # agentdiff init not run in this repo — skip silently.
    return None


def _tail_read_jsonl(path: str, chunk_size: int = 32768) -> list:
    """Read JSONL lines from the end of a potentially large file.

    Returns parsed dicts, most-recent first.  Reads at most chunk_size bytes
    from the end on the first pass — enough for thousands of short entries.
    """
    results = []
    try:
        size = os.path.getsize(path)
        with open(path, "rb") as fh:
            offset = max(0, size - chunk_size)
            fh.seek(offset)
            raw = fh.read()
        if offset > 0:
            # Skip the (possibly partial) first line we cut into.
            nl = raw.find(b"\n")
            raw = raw[nl + 1:] if nl >= 0 else raw
        for line in reversed(raw.decode("utf-8", errors="replace").splitlines()):
            line = line.strip()
            if not line:
                continue
            try:
                results.append(json.loads(line))
            except Exception:
                continue
    except Exception:
        pass
    return results


def get_prompt_from_history(session_id: str) -> str:
    """Read the most-recent user prompt for session_id from ~/.claude/history.jsonl.

    history.jsonl format (one JSON object per line):
      {"display":"...", "pastedContents":{...}, "sessionId":"...", "project":"...", "timestamp":...}

    We take the most-recent entry whose sessionId matches and whose display
    is not a slash command.  We also append any inline pasted text content.
    """
    path = os.path.expanduser("~/.claude/history.jsonl")
    entries = _tail_read_jsonl(path)
    for entry in entries:
        if entry.get("sessionId") != session_id:
            continue
        display = entry.get("display", "").strip()
        if not display or display.startswith("/"):
            continue
        # Append pasted content that has actual text (not just a hash).
        extra_parts = []
        for pasted in (entry.get("pastedContents") or {}).values():
            if isinstance(pasted, dict) and pasted.get("type") == "text":
                content = pasted.get("content", "")
                if content:
                    extra_parts.append(content[:200])
        if extra_parts:
            display = display + " [pasted: " + " | ".join(extra_parts) + "]"
        return display[:500]
    return "unknown"


def get_model_and_prompt(cwd: str, session_id: str) -> tuple:
    """Read model from Claude Code session JSONL, prompt from history.jsonl.

    Model: ~/.claude/projects/{repo-slug}/{session_id}.jsonl — assistant entries.
      Skips <synthetic> model values (injected during context compression).
    Prompt: ~/.claude/history.jsonl — most-recent display for this sessionId.
    """
    import glob as _glob
    model = "unknown"
    try:
        home = os.path.expanduser("~")
        pattern = os.path.join(home, ".claude", "projects", "**", f"{session_id}.jsonl")
        debug_log(f"glob pattern: {pattern}")
        matches = _glob.glob(pattern, recursive=True)
        debug_log(f"glob matches: {matches}")
        if matches:
            session_path = matches[0]
            debug_log(f"session_path: {session_path}")
            for entry in _tail_read_jsonl(session_path):
                if entry.get("type") == "assistant":
                    m = entry.get("message", {}).get("model", "")
                    if m and m != "<synthetic>":
                        model = m
                        debug_log(f"model found: {model}")
                        break
    except Exception as exc:
        debug_log(f"model lookup error: {exc}")

    prompt = get_prompt_from_history(session_id)
    # Allow test/CI injection via env var when history lookup can't find the session.
    if prompt == "unknown":
        env_prompt = os.environ.get("AGENTDIFF_PROMPT", "")
        if env_prompt:
            prompt = env_prompt
            debug_log(f"prompt from AGENTDIFF_PROMPT env var")
    debug_log(f"prompt: {prompt[:80]!r}")
    return model, prompt


def is_in_repo(abs_file: str, repo_root: str) -> bool:
    """Check if file is in the repo """
    if not abs_file.startswith(repo_root):
        return False
    rel = abs_file[len(repo_root):]
    if rel.startswith("/.git"):
        return False
    return True


def compute_line_range(abs_file: str, old_content: str, new_content: str) -> list:
    """Compute which lines were changed."""
    try:
        # Read current file
        with open(abs_file) as f:
            current = f.read()

        old_lines = set()
        new_lines = set()

        for i, line in enumerate(current.split("\n"), 1):
            if old_content in line:
                old_lines.add(i)
            if new_content in line:
                new_lines.add(i)

        # Return union of old and new line numbers
        return sorted(list(old_lines | new_lines))
    except Exception:
        # Fall back to line count
        return list(range(1, new_content.count("\n") + 2))


def main():
    input_data = sys.stdin.read()
    if not input_data.strip():
        sys.exit(0)
    debug_log(f"raw={input_data[:2000]}")

    try:
        payload = json.loads(input_data)
    except json.JSONDecodeError:
        sys.exit(0)

    # Support both legacy top-level payload and nested hook payloads.
    tool = first(payload, "tool", "tool_name", "toolName", default="")
    tool_input = payload.get("tool_input") if isinstance(payload.get("tool_input"), dict) else payload

    # Only handle Edit, Write, MultiEdit tools
    if tool not in ["Edit", "Write", "MultiEdit"]:
        debug_log(f"skip: unknown tool={tool!r}")
        sys.exit(0)

    cwd = first(payload, "cwd", default=os.getcwd())
    repo_root = find_repo_root(cwd)

    abs_file = first(tool_input, "file_path", "filePath", "path", default="")
    if not abs_file:
        debug_log("skip: missing abs_file")
        sys.exit(0)
    in_repo = os.path.exists(os.path.join(repo_root, ".git"))
    if in_repo and not is_in_repo(abs_file, repo_root):
        sys.exit(0)

    session_id = first(payload, "session_id", "sessionId", default="unknown")
    debug_log(f"before get_model_and_prompt session_id={session_id}")
    model, prompt = get_model_and_prompt(cwd, session_id)

    timestamp = datetime.now(timezone.utc).isoformat()

    entry = {
        "timestamp": timestamp,
        "agent": "claude-code",
        "model": model,
        "session_id": session_id,
        "tool": tool,
        "file": abs_file[len(repo_root):].lstrip("/") if in_repo else abs_file,
        "abs_file": abs_file,
        "acceptance": "verbatim",
    }
    if capture_prompts_enabled():
        entry["prompt"] = prompt

    if tool == "Edit":
        entry["old"] = first(tool_input, "old_string", "oldString", default="")[:200]
        entry["new"] = first(tool_input, "new_string", "newString", default="")[:200]
        entry["lines"] = compute_line_range(abs_file, entry["old"], entry["new"])
    elif tool == "Write":
        content = first(tool_input, "content", default="")
        entry["content_preview"] = content[:200]
        entry["total_lines"] = content.count("\n") + 1
        entry["lines"] = list(range(1, content.count("\n") + 2))
    elif tool == "MultiEdit":
        edits = first(tool_input, "edits", default=[])
        entry["edit_count"] = len(edits)
        entry["edits"] = [{"old": e.get("old_string", "")[:100], "new": e.get("new_string", "")[:100]} for e in edits]
        # Approximate lines
        total_lines = sum(e.get("new_string", "").count("\n") for e in edits)
        entry["lines"] = list(range(1, total_lines + 2))

    session_log = get_session_log(cwd)
    if session_log is None:
        debug_log("skip: agentdiff init not run in this repo")
        sys.exit(0)
    with open(session_log, "a") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(f"wrote entry tool={tool} file={entry['file']} lines={entry.get('lines')}")


if __name__ == "__main__":
    main()
