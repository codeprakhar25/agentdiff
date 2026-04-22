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
from typing import Dict, List, Tuple


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


def always_log(message: str) -> None:
    """Write to codex.log unconditionally — key events, no secrets."""
    _write_log("capture-codex.log", message)


def debug_log(message: str) -> None:
    if not debug_enabled():
        return
    _write_log("capture-codex-debug.log", message)


def first(payload: dict, *keys, default=None):
    for key in keys:
        if key in payload and payload.get(key) is not None:
            return payload.get(key)
    return default


def codex_sessions_root() -> str:
    return os.environ.get("CODEX_SESSIONS_ROOT", os.path.expanduser("~/.codex/sessions"))


def _tail_read_jsonl(path: str, chunk_size: int = 32768) -> List[dict]:
    """Read JSONL lines from the end of a potentially large file, most-recent first."""
    results: List[dict] = []
    try:
        size = os.path.getsize(path)
        with open(path, "rb") as fh:
            offset = max(0, size - chunk_size)
            fh.seek(offset)
            raw = fh.read()
        if offset > 0:
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
    """Read the most-recent user prompt for session_id from ~/.codex/history.jsonl.

    history.jsonl format:
      {"session_id":"...","ts":1234567890,"text":"..."}

    Returns the text of the most-recent entry whose session_id matches.
    """
    path = os.path.expanduser("~/.codex/history.jsonl")
    for entry in _tail_read_jsonl(path):
        if entry.get("session_id") == session_id and entry.get("text"):
            return str(entry["text"])[:500]
    return ""


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
    # Check unstaged, staged, and HEAD-relative diffs — Codex may write files
    # at various staging states depending on user workflow.
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

    # git diff does not show brand-new untracked files. Detect them separately
    # so Codex attribution works when it creates a file from scratch.
    try:
        untracked = subprocess.run(
            ["git", "ls-files", "--others", "--exclude-standard"],
            capture_output=True, text=True, cwd=repo_root,
        )
        if untracked.returncode == 0:
            for rel_path in untracked.stdout.splitlines():
                rel_path = rel_path.strip()
                if not rel_path or rel_path in result:
                    continue
                abs_path = os.path.join(repo_root, rel_path)
                try:
                    with open(abs_path, "r", encoding="utf-8", errors="replace") as fh:
                        line_count = sum(1 for _ in fh)
                    if line_count > 0:
                        result[rel_path] = list(range(1, line_count + 1))
                    else:
                        result[rel_path] = [1]
                except (OSError, IOError):
                    result[rel_path] = [1]
    except Exception:
        pass

    return {k: sorted(set(v)) for k, v in result.items() if v}


def get_dirty_file_names(repo_root: str) -> List[str]:
    """Return repo-relative paths of all files currently differing from HEAD, including untracked."""
    files: List[str] = []
    try:
        out = subprocess.run(
            ["git", "diff", "HEAD", "--name-only"],
            capture_output=True, text=True, cwd=repo_root,
        )
        if out.returncode == 0:
            files.extend(line.strip() for line in out.stdout.splitlines() if line.strip())
    except Exception:
        pass
    try:
        untracked = subprocess.run(
            ["git", "ls-files", "--others", "--exclude-standard"],
            capture_output=True, text=True, cwd=repo_root,
        )
        if untracked.returncode == 0:
            files.extend(line.strip() for line in untracked.stdout.splitlines() if line.strip())
    except Exception:
        pass
    return list(dict.fromkeys(files))  # deduplicate, preserve order


def pre_task_state_path(repo_root: str) -> str:
    return os.path.join(repo_root, ".git", "agentdiff", "codex-pre-task.json")


def save_pre_task_state(repo_root: str) -> None:
    """Snapshot current dirty-file list so task_complete can isolate codex's changes.

    Only runs if agentdiff init has been run (i.e. .git/agentdiff/ exists).
    """
    base = os.path.join(repo_root, ".git", "agentdiff")
    if not os.path.isdir(base):
        debug_log("pre-task snapshot skipped: agentdiff init not run in this repo")
        return
    dirty = get_dirty_file_names(repo_root)
    state_path = pre_task_state_path(repo_root)
    try:
        with open(state_path, "w", encoding="utf-8") as f:
            json.dump({"files": sorted(dirty)}, f)
        debug_log(f"pre-task snapshot: {len(dirty)} dirty files → {state_path}")
    except Exception as e:
        debug_log(f"pre-task snapshot failed: {e}")


def load_and_consume_pre_task_state(repo_root: str) -> set:
    """Load and delete the pre-task snapshot; returns set of pre-existing dirty files."""
    state_path = pre_task_state_path(repo_root)
    pre_dirty: set = set()
    if not os.path.exists(state_path):
        return pre_dirty
    try:
        with open(state_path, "r", encoding="utf-8") as f:
            data = json.load(f)
        pre_dirty = set(data.get("files", []))
        debug_log(f"pre-task loaded: {len(pre_dirty)} pre-existing dirty files")
    except Exception as e:
        debug_log(f"pre-task load failed: {e}")
    try:
        os.unlink(state_path)
    except Exception:
        pass
    return pre_dirty


def is_git_repo(path: str) -> bool:
    return bool(path) and os.path.exists(os.path.join(path, ".git"))


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
    root = codex_sessions_root()
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


def find_cwd_and_meta_from_recent_session(turn_id: str = "") -> tuple:
    """Search recent Codex rollout files for cwd, model, session_id.

    Codex's task_complete event doesn't include cwd. We recover it by scanning
    the most recently modified rollout files for a matching turn_id (fast path)
    or simply the newest session (fallback).
    Returns (cwd, model, session_id) — all may be empty strings.
    """
    root = codex_sessions_root()
    if not os.path.exists(root):
        return "", "", ""

    pattern = os.path.join(root, "**", "rollout-*.jsonl")
    matches = sorted(glob.glob(pattern, recursive=True),
                     key=lambda p: os.path.getmtime(p), reverse=True)

    for path in matches[:5]:  # check 5 most recent files
        found_cwd = ""
        found_model = ""
        found_session_id = ""
        found_turn = False
        try:
            with open(path, "r", encoding="utf-8") as f:
                for line in f:
                    try:
                        obj = json.loads(line.strip())
                    except Exception:
                        continue
                    obj_type = obj.get("type", "")
                    obj_payload = obj.get("payload") or {}
                    if not isinstance(obj_payload, dict):
                        obj_payload = {}

                    if obj_type == "session_meta":
                        found_session_id = obj_payload.get("id", "")
                        # session_meta may also have cwd
                        found_cwd = found_cwd or obj_payload.get("cwd", "")

                    elif obj_type == "turn_context":
                        found_cwd = obj_payload.get("cwd", "") or found_cwd
                        found_model = obj_payload.get("model", "") or found_model
                        if turn_id and obj_payload.get("turn_id") == turn_id:
                            found_turn = True

                    elif obj_type == "event_msg":
                        inner_type = obj_payload.get("type", "")
                        if turn_id and obj_payload.get("turn_id") == turn_id:
                            if inner_type in ("task_complete", "task_started"):
                                found_turn = True

            # If we found the specific turn in this file, return immediately.
            if turn_id and found_turn and found_cwd:
                return found_cwd, found_model, found_session_id
            # Fallback: return from newest file that has any cwd.
            if not turn_id and found_cwd:
                return found_cwd, found_model, found_session_id

        except Exception:
            continue

    return "", "", ""


def parse_notify_stdin(input_data: str) -> List[dict]:
    """Parse Codex notify stdin.

    Codex may send a single JSON object OR a JSONL stream (one event per line).
    Returns a list of parsed dicts.
    """
    text = input_data.strip()
    if not text:
        return []

    # Try as a single JSON object first.
    try:
        obj = json.loads(text)
        if isinstance(obj, dict):
            return [obj]
        if isinstance(obj, list):
            return [x for x in obj if isinstance(x, dict)]
    except json.JSONDecodeError:
        pass

    # Try as JSONL.
    results = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
            if isinstance(obj, dict):
                results.append(obj)
        except json.JSONDecodeError:
            continue
    return results


def append_unique(values: List[str], value: str) -> None:
    if not value:
        return
    if value not in values:
        values.append(value)


def extract_prompt_from_legacy_messages(messages) -> str:
    if not isinstance(messages, list):
        return ""
    for msg in reversed(messages):
        if not isinstance(msg, dict):
            continue
        role = msg.get("role")
        if role not in {"user", "system", None}:
            continue
        text = extract_text(msg.get("content"))
        if text.strip():
            return text.strip()
    return ""


def extract_codex_context(events: List[dict]) -> tuple:
    """Extract cwd, model, session_id, turn_id, prompt, event_name from Codex events.

    The Codex session JSONL format has outer fields {type, timestamp, payload}.
    - turn_context: payload has {cwd, model, turn_id, ...}
    - event_msg/task_complete: payload has {type, turn_id, last_agent_message}
    - session_meta: payload has {id, cwd, model_provider, ...}
    """
    cwd = ""
    model = ""
    session_id = ""
    turn_id = ""
    prompt = ""
    event_name = ""

    for event in events:
        outer_type = event.get("type", "")
        inner = event.get("payload") or {}
        if not isinstance(inner, dict):
            inner = {}

        if outer_type == "turn_context":
            cwd = cwd or inner.get("cwd", "")
            model = model or inner.get("model", "")
            turn_id = turn_id or inner.get("turn_id", "")

        elif outer_type == "event_msg":
            inner_type = inner.get("type", "")
            event_name = event_name or inner_type
            turn_id = turn_id or inner.get("turn_id", "")
            if inner_type == "task_complete":
                prompt = prompt or inner.get("last_agent_message", "")

        elif outer_type == "session_meta":
            session_id = session_id or inner.get("id", "")
            cwd = cwd or inner.get("cwd", "")

        # Legacy flat payload support.
        event_name = event_name or str(
            first(event, "event", "event_name", "hook_event_name", "hookEventName", default="")
        )
        turn_id = turn_id or str(first(event, "turn-id", "turn_id", "turnId", default=""))
        session_id = session_id or str(
            first(event, "session_id", "sessionId", "thread-id", "thread_id", "threadId", default="")
        )
        cwd = cwd or str(
            first(
                event,
                "cwd",
                "workspace",
                "workspace_path",
                "workspacePath",
                "working_directory",
                "workingDirectory",
                default="",
            )
        )
        model = model or str(first(event, "model", "model_name", "modelName", default=""))
        if not prompt:
            prompt = str(
                first(
                    event,
                    "last-assistant-message",
                    "last_assistant_message",
                    "lastAgentMessage",
                    default="",
                )
            )
        if not prompt:
            prompt = extract_prompt_from_legacy_messages(
                first(event, "input-messages", "input_messages", default=[])
            )

        # Also check flat fields for older/alternate payload shapes.
        cwd = cwd or first(event, "cwd", "workspace", "workspace_path", default="")
        model = model or first(event, "model", "model_name", default="")
        session_id = session_id or first(event, "session_id", "sessionId", default="")
        prompt = prompt or extract_prompt(event)

    return cwd, model, session_id, turn_id, prompt, event_name


def resolve_repo_and_changes(candidates: List[str]) -> Tuple[str, str, Dict[str, List[int]]]:
    for candidate in candidates:
        repo_root = find_repo_root(candidate)
        if not is_git_repo(repo_root):
            debug_log(f"candidate skip (not git): cwd={candidate!r} repo_root={repo_root!r}")
            continue
        changed = collect_changed_lines(repo_root)
        if changed:
            debug_log(
                f"candidate hit: cwd={candidate!r} repo_root={repo_root!r} files={list(changed.keys())}"
            )
            return repo_root, candidate, changed
        debug_log(f"candidate no changes: cwd={candidate!r} repo_root={repo_root!r}")
    return "", "", {}


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--forward", default="")
    # Codex may pass JSON as a positional arg (sys.argv[1]) or on stdin.
    # Accept both — newer versions use argv, older versions use stdin.
    parser.add_argument("payload", nargs="?", default="")
    args, unknown = parser.parse_known_args()

    # Try positional arg first (Codex v0.118+), then unknown args, then stdin.
    input_data = args.payload.strip()
    if not input_data and unknown:
        input_data = " ".join(unknown).strip()
    if not input_data:
        input_data = sys.stdin.read()

    # Always write a fire-marker so we can tell if notify ever runs,
    # regardless of AGENTDIFF_DEBUG (helps diagnose silent failures).
    try:
        marker_dir = os.path.expanduser("~/.agentdiff/logs")
        os.makedirs(marker_dir, exist_ok=True)
        marker = os.path.join(marker_dir, "codex-notify-fired.log")
        with open(marker, "a", encoding="utf-8") as mf:
            ts = datetime.now(timezone.utc).isoformat()
            source = "argv" if args.payload.strip() or unknown else "stdin"
            mf.write(f"{ts} {source} len={len(input_data)}\n")
    except Exception:
        pass

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

    events = parse_notify_stdin(input_data)
    if not events:
        run_forward(forward_cmd, input_data)
        return 0

    try:
        cwd, model, session_id, turn_id, prompt, event_name = extract_codex_context(events)
        always_log(f"event={event_name!r} turn={turn_id!r} cwd={cwd!r} model={model!r} session={session_id!r}")
        debug_log(f"event_name={event_name!r} turn_id={turn_id!r} cwd_from_events={cwd!r}")

        # task_started / UserPromptSubmit: snapshot dirty files so task_complete can
        # isolate what codex changed. UserPromptSubmit fires from hooks.json before
        # the model processes each turn — same semantics as the older task_started event.
        task_started_events = {"task_started", "TaskStarted", "UserPromptSubmit"}
        if event_name and event_name in task_started_events:
            debug_log(f"task_started: saving pre-task state")
            # Resolve repo root for snapshot (best-effort — use event cwd or process cwd).
            snap_candidates: List[str] = []
            append_unique(snap_candidates, cwd if isinstance(cwd, str) else "")
            append_unique(snap_candidates, os.getcwd())
            for candidate in snap_candidates:
                snap_root = find_repo_root(candidate)
                if is_git_repo(snap_root):
                    save_pre_task_state(snap_root)
                    break
            run_forward(forward_cmd, input_data)
            return 0

        # Skip other non-edit events.
        known_skip_events = {
            "turn_aborted",
            "TurnAborted",
            "agent-turn-start",
            "agent-turn-stop",
            "agent_turn_start",
            "agent_turn_stop",
        }
        if event_name and event_name in known_skip_events:
            always_log(f"SKIP non_edit_event={event_name!r}")
            debug_log(f"skip: non-edit event {event_name!r}")
            run_forward(forward_cmd, input_data)
            return 0

        # Candidate order matters: prefer event cwd and current process cwd.
        # Session scan is used only as fallback to avoid cross-repo misses.
        candidate_cwds: List[str] = []
        append_unique(candidate_cwds, cwd if isinstance(cwd, str) else "")
        append_unique(candidate_cwds, os.getcwd())
        repo_root, chosen_cwd, changed = resolve_repo_and_changes(candidate_cwds)

        recovered_model = ""
        recovered_sid = ""
        if not changed:
            recovered_cwd, recovered_model, recovered_sid = find_cwd_and_meta_from_recent_session(turn_id)
            debug_log(
                f"recovery candidate: cwd={recovered_cwd!r} model={recovered_model!r} sid={recovered_sid!r}"
            )
            repo_root, chosen_cwd, changed = resolve_repo_and_changes([recovered_cwd] if recovered_cwd else [])

        if not changed:
            always_log(f"SKIP no_changed_lines candidates={candidate_cwds}")
            debug_log("skip: no changed lines found in any candidate repo")
            run_forward(forward_cmd, input_data)
            return 0

        # Consume (and discard) the pre-task snapshot — kept for hook compatibility
        # but no longer used to filter. Attribution conflicts across agents are
        # resolved by prepare-ledger at commit time, not here.
        load_and_consume_pre_task_state(repo_root) if repo_root else None

        if not chosen_cwd:
            chosen_cwd = cwd or os.getcwd()
        if not model:
            model = recovered_model

        if not model:
            model = read_model_from_rollout(str(session_id))
        if not session_id:
            session_id = recovered_sid
        if not session_id:
            session_id = "unknown"

        timestamp = datetime.now(timezone.utc).isoformat()
        session_log = get_session_log(chosen_cwd)
        if session_log is None:
            always_log(f"SKIP no_agentdiff_init cwd={chosen_cwd!r}")
            debug_log(f"skip: agentdiff init not run in {chosen_cwd!r}")
            return 0

        with open(session_log, "a", encoding="utf-8") as f:
            for file_path, lines in changed.items():
                abs_file = os.path.join(repo_root, file_path)
                entry = {
                    "timestamp": timestamp,
                    "agent": "codex",
                    "mode": "agent",
                    "model": model or "codex",
                    "session_id": str(session_id),
                    "tool": event_name or "task_complete",
                    "file": file_path,
                    "abs_file": abs_file,
                    "prompt": prompt or get_prompt_from_history(str(session_id)) or "unknown",
                    "acceptance": "verbatim",
                    "lines": lines,
                }
                f.write(json.dumps(entry) + "\n")

        always_log(f"WROTE {len(changed)} entries files={list(changed.keys())} model={model!r} session={session_log!r}")
        debug_log(f"wrote {len(changed)} codex entries to {session_log}")
    finally:
        run_forward(forward_cmd, input_data)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
