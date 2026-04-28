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


def _write_log(path: str, message: str) -> None:
    try:
        log_dir = os.path.expanduser("~/.agentdiff/logs")
        os.makedirs(log_dir, exist_ok=True)
        ts = datetime.now(timezone.utc).isoformat()
        with open(os.path.join(log_dir, path), "a", encoding="utf-8") as f:
            f.write(f"{ts} {message}\n")
    except Exception:
        pass


def debug_log(message: str) -> None:
    if not debug_enabled():
        return
    _write_log("capture-cursor-debug.log", message)


def first(payload: dict, *keys, default=None):
    for key in keys:
        if key in payload and payload.get(key) is not None:
            return payload.get(key)
    return default


def coerce_line(value):
    try:
        n = int(value)
        return n if n > 0 else None
    except Exception:
        return None


def _add_line_range(out: set, start, end=None):
    start_n = coerce_line(start)
    end_n = coerce_line(end if end is not None else start)
    if start_n is None or end_n is None:
        return
    for ln in range(min(start_n, end_n), max(start_n, end_n) + 1):
        out.add(ln)


def _extract_line_from_position(value):
    if isinstance(value, dict):
        return first(value, "line", "lineNumber", "line_number")
    return value


def _add_lines_from_range_object(out: set, value):
    if not isinstance(value, dict):
        return

    start = first(value, "startLine", "start_line", "line_start", "line", "lineNumber", "line_number")
    end = first(value, "endLine", "end_line", "line_end", default=start)
    if start is not None:
        _add_line_range(out, start, end)
        return

    start_pos = _extract_line_from_position(value.get("start"))
    end_pos = _extract_line_from_position(value.get("end"))
    if start_pos is not None:
        _add_line_range(out, start_pos, end_pos if end_pos is not None else start_pos)


def extract_lines(value):
    """Extract 1-indexed line numbers from common Cursor hook payload shapes."""
    out = set()
    if isinstance(value, list):
        for item in value:
            if isinstance(item, dict):
                _add_lines_from_range_object(out, item)
            else:
                line = coerce_line(item)
                if line is not None:
                    out.add(line)
    elif isinstance(value, dict):
        _add_lines_from_range_object(out, value)
    else:
        line = coerce_line(value)
        if line is not None:
            out.add(line)
    return sorted(out)


def extract_lines_from_changes(changes):
    out = set()
    if not isinstance(changes, list):
        return []
    for change in changes:
        if not isinstance(change, dict):
            continue
        before = len(out)
        _add_lines_from_range_object(out, change)
        if len(out) == before:
            for key in ("range", "newRange", "new_range", "selection", "targetSelection"):
                _add_lines_from_range_object(out, change.get(key))
    return sorted(out)


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

    # Strip Windows WSL UNC prefix: /wsl.localhost/<distro>/... or /wsl$/<distro>/...
    # These arrive after \\ → / conversion above.
    wsl_match = re.match(r"^/wsl(?:\.localhost|[$])/[^/]+(/.+)", p, re.IGNORECASE)
    if wsl_match:
        p = wsl_match.group(1)

    # Convert Windows drive letter paths: C:/... → /mnt/c/...
    drive_match = re.match(r"^([A-Za-z]):/(.*)", p)
    if drive_match:
        p = f"/mnt/{drive_match.group(1).lower()}/{drive_match.group(2)}"

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


def get_session_log(cwd: str, repo_root_hint: str = ""):
    """Return session log path, or None if agentdiff init has not been run here."""
    override = os.environ.get("AGENTDIFF_SESSION_LOG")
    if override:
        parent = os.path.dirname(override)
        if parent:
            os.makedirs(parent, exist_ok=True)
        return override

    repo_root = repo_root_hint or find_repo_root(cwd)
    base = os.path.join(repo_root, ".git", "agentdiff")
    if os.path.isdir(base):
        return os.path.join(base, "session.jsonl")

    return None


def get_cached_prompt(conversation_id: str) -> str:
    """Read cached prompt from beforeSubmitPrompt."""
    prompt_path = os.path.expanduser(f"~/.cursor/hooks/prompts/{conversation_id}.txt")
    if os.path.exists(prompt_path):
        try:
            with open(prompt_path) as f:
                return f.read().strip()
        except Exception:
            pass
    return ""


def _cursor_project_slug(repo_root: str) -> str:
    """Derive the ~/.cursor/projects/ slug from a repo root path.

    /home/prakh/ml-resarch  →  home-prakh-ml-resarch
    """
    return repo_root.lstrip("/").replace("/", "-")


def _wsl_distro_name() -> str:
    """Return the WSL distro name (e.g. 'Ubuntu'), or empty string if not in WSL."""
    name = os.environ.get("WSL_DISTRO_NAME", "")
    if name:
        return name
    try:
        with open("/proc/version") as f:
            if "microsoft" in f.read().lower():
                pass  # fall through to os-release
    except Exception:
        return ""
    try:
        with open("/etc/os-release") as f:
            for line in f:
                if line.startswith("NAME="):
                    return line.split("=", 1)[1].strip().strip('"').replace(" ", "-")
    except Exception:
        pass
    return "Ubuntu"


def _cursor_transcript_candidates(conversation_id: str, repo_root: str) -> list:
    """Return candidate transcript paths to try, most-specific first."""
    linux_slug = _cursor_project_slug(repo_root)
    path_suffix = linux_slug  # e.g. home-prakh-agentdiff

    candidates = []

    # Windows-side cursor projects dir (WSL2 host)
    win_projects = None
    try:
        win_users = "/mnt/c/Users"
        for entry in sorted(os.scandir(win_users), key=lambda e: e.name):
            p = os.path.join(entry.path, ".cursor", "projects")
            if os.path.isdir(p):
                win_projects = p
                break
    except Exception:
        pass

    # Linux-side cursor projects dir
    linux_projects = os.path.expanduser("~/.cursor/projects")

    for projects_dir in filter(None, [win_projects, linux_projects if os.path.isdir(linux_projects) else None]):
        # Search for a slug ending in the right path suffix (handles wsl-localhost-Ubuntu-... prefix)
        try:
            for slug in os.listdir(projects_dir):
                if slug == linux_slug or slug.lower().endswith("-" + path_suffix.lower()):
                    t = os.path.join(projects_dir, slug, "agent-transcripts",
                                     conversation_id, f"{conversation_id}.jsonl")
                    if os.path.exists(t):
                        candidates.append(t)
        except Exception:
            pass

    # Fallback: original linux-side path
    distro = _wsl_distro_name()
    for slug in ([f"wsl-localhost-{distro}-{path_suffix}", linux_slug] if distro else [linux_slug]):
        t = os.path.expanduser(
            f"~/.cursor/projects/{slug}/agent-transcripts/{conversation_id}/{conversation_id}.jsonl"
        )
        if t not in candidates:
            candidates.append(t)

    return candidates


def get_prompt_from_transcript(conversation_id: str, repo_root: str) -> str:
    """Read the user's prompt from Cursor's agent-transcript JSONL.

    Searches both the Linux-side and Windows-side cursor project dirs, and
    tries multiple slug patterns (bare Linux slug + WSL UNC slug) so it works
    regardless of how the workspace was opened.
    """
    transcript_paths = _cursor_transcript_candidates(conversation_id, repo_root)
    transcript_path = next((p for p in transcript_paths if os.path.exists(p)), None)
    if not transcript_path:
        debug_log(f"transcript not found for conv={conversation_id} candidates={transcript_paths[:3]}")
        return ""
    try:
        with open(transcript_path, encoding="utf-8", errors="replace") as f:
            for raw in f:
                raw = raw.strip()
                if not raw:
                    continue
                try:
                    entry = json.loads(raw)
                except Exception:
                    continue
                if entry.get("role") != "user":
                    continue
                content = entry.get("message", {}).get("content", [])
                if isinstance(content, list):
                    for part in content:
                        if isinstance(part, dict) and part.get("type") == "text":
                            text = part.get("text", "")
                            # Strip the <user_query>…</user_query> wrapper Cursor adds.
                            text = re.sub(r"<user_query>\s*", "", text)
                            text = re.sub(r"\s*</user_query>", "", text)
                            return text.strip()[:500]
                elif isinstance(content, str):
                    return content.strip()[:500]
    except Exception as exc:
        debug_log(f"transcript read error: {exc}")
    return ""




def main():
    input_data = sys.stdin.read()
    if not input_data.strip():
        sys.exit(0)
    debug_log(f"raw={input_data[:2000]}")

    try:
        payload = json.loads(input_data)
    except json.JSONDecodeError:
        debug_log(f"SKIP parse_error input={input_data[:120]!r}")
        sys.exit(0)

    event_name = first(payload, "hook_event_name", "hookEventName", "event_name", "event", default="")
    if event_name not in ["afterFileEdit", "afterTabFileEdit", "beforeSubmitPrompt"]:
        debug_log(f"SKIP unknown_event={event_name!r}")
        sys.exit(0)

    # Handle beforeSubmitPrompt - cache the prompt
    if event_name == "beforeSubmitPrompt":
        conversation_id = first(payload, "conversation_id", "conversationId", default="unknown")
        prompt = first(payload, "user_prompt", "userPrompt", "prompt", default="")
        prompt_dir = os.path.expanduser("~/.cursor/hooks/prompts")
        os.makedirs(prompt_dir, exist_ok=True)
        with open(os.path.join(prompt_dir, f"{conversation_id}.txt"), "w") as f:
            f.write(prompt)
        debug_log(f"cached_prompt conv={conversation_id}")
        sys.exit(0)

    cwd_raw = first(payload, "cwd", "workspace", "workspace_path", "workspacePath", default=os.getcwd())
    cwd = normalize_path(str(cwd_raw), os.getcwd())
    repo_root = find_repo_root(cwd)

    abs_file = first(payload, "file_path", "filePath", "path", default="")
    if not abs_file and isinstance(payload.get("file"), dict):
        abs_file = first(payload.get("file", {}), "path", "file_path", "filePath", default="")
    if not abs_file:
        debug_log("SKIP missing_abs_file")
        sys.exit(0)

    abs_file = normalize_path(str(abs_file), cwd)
    if not abs_file:
        debug_log("SKIP invalid_abs_file_after_normalize")
        sys.exit(0)

    file_repo_root = find_repo_root_from_path(abs_file)
    if file_repo_root:
        repo_root = file_repo_root

    in_repo = is_git_repo(repo_root)
    if in_repo and not is_within_repo(abs_file, repo_root):
        debug_log(f"SKIP file_outside_repo file={abs_file!r} repo={repo_root!r}")
        sys.exit(0)

    # Get prompt for agent mode
    if event_name == "afterFileEdit":
        conversation_id = first(payload, "conversation_id", "conversationId", default="")
        prompt = ""
        if conversation_id:
            prompt = get_cached_prompt(conversation_id)
            if not prompt and repo_root:
                prompt = get_prompt_from_transcript(conversation_id, repo_root)
        if not prompt:
            prompt = "unknown"
        mode = "agent"
    else:  # afterTabFileEdit
        prompt = None
        mode = "tab"

    timestamp = datetime.now(timezone.utc).isoformat()
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
        old_lines = extract_lines(first(payload, "old_lines", "oldLines", default=[]))
        new_lines = extract_lines(first(payload, "new_lines", "newLines", "changed_lines", "changedLines", "lines", default=[]))
        if not new_lines:
            new_lines = extract_lines_from_changes(payload.get("changes"))
        if not new_lines:
            new_lines = extract_lines_from_changes(payload.get("edits"))

        entry["lines"] = new_lines if new_lines else old_lines
        debug_log(
            f"event={event_name} file={entry['file']!r} model={model!r} "
            f"n_lines={len(entry['lines'])} "
            f"payload_old={old_lines} payload_new={new_lines} "
            f"repo={repo_root!r}"
        )
    else:  # tab completion
        line_num = first(payload, "line_number", "lineNumber", default=1)
        entry["lines"] = [line_num if isinstance(line_num, int) else 1]
        debug_log(f"event={event_name} file={entry['file']!r} line={line_num}")

    session_log = get_session_log(cwd, repo_root if in_repo else "")
    if session_log is None:
        debug_log(f"SKIP no_agentdiff_init repo={repo_root!r}")
        sys.exit(0)
    with open(session_log, "a") as f:
        f.write(json.dumps(entry) + "\n")
    debug_log(f"WROTE file={entry['file']!r} lines={entry.get('lines')} session_log={session_log!r}")


if __name__ == "__main__":
    main()
