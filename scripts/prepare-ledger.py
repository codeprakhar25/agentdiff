#!/usr/bin/env python3
"""
Prepare commit-scoped ledger snapshot before commit.

Usage:
  prepare-ledger.py <repo_root> <session_log> <pending_context> <pending_ledger>
"""
import hashlib
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from typing import Dict, List, Tuple


def run(cmd: List[str], cwd: str) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)


def parse_staged_changes(repo_root: str) -> Dict[str, List[Tuple[int, int]]]:
    diff = run(["git", "diff", "--cached", "--unified=0", "--no-color"], cwd=repo_root)
    if diff.returncode != 0 or not diff.stdout.strip():
        return {}

    changed: Dict[str, set] = {}
    current_file = ""

    for raw in diff.stdout.splitlines():
        line = raw.rstrip("\n")
        if line.startswith("diff --git "):
            parts = line.split()
            if len(parts) >= 4:
                path = parts[3]
                if path.startswith("b/"):
                    path = path[2:]
                current_file = path
                changed.setdefault(current_file, set())
            continue

        if not line.startswith("@@") or not current_file:
            continue

        # @@ -old_start,old_count +new_start,new_count @@
        m = re.search(r"\+(\d+)(?:,(\d+))?", line)
        if not m:
            continue

        start = int(m.group(1))
        count = int(m.group(2) or "1")
        if count <= 0:
            continue

        for ln in range(start, start + count):
            changed[current_file].add(ln)

    out: Dict[str, List[Tuple[int, int]]] = {}
    for path, lines in changed.items():
        sorted_lines = sorted(lines)
        if not sorted_lines:
            out[path] = []
            continue
        ranges: List[Tuple[int, int]] = []
        start = sorted_lines[0]
        prev = sorted_lines[0]
        for ln in sorted_lines[1:]:
            if ln == prev + 1:
                prev = ln
                continue
            ranges.append((start, prev))
            start = ln
            prev = ln
        ranges.append((start, prev))
        out[path] = ranges
    return out


def read_json_file(path: str):
    if not os.path.exists(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return None


def parse_event_ts(event_ts: str) -> int:
    try:
        normalized = event_ts.replace("Z", "+00:00")
        dt = datetime.fromisoformat(normalized)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return int(dt.timestamp())
    except Exception:
        return 0


def head_commit_ts(repo_root: str) -> int:
    out = run(["git", "show", "-s", "--format=%ct", "HEAD"], cwd=repo_root)
    if out.returncode != 0:
        return 0
    try:
        return int(out.stdout.strip())
    except Exception:
        return 0


def event_overlaps_staged(event: dict, lines_by_file: Dict[str, List[Tuple[int, int]]]) -> bool:
    file_path = str(event.get("file", "")).strip()
    if not file_path or file_path not in lines_by_file:
        return False

    event_lines = event.get("lines")
    if not isinstance(event_lines, list) or not event_lines:
        return True

    line_values = set()
    for v in event_lines:
        try:
            n = int(v)
            if n > 0:
                line_values.add(n)
        except Exception:
            continue
    if not line_values:
        return True

    for (start, end) in lines_by_file.get(file_path, []):
        lo = min(start, end)
        hi = max(start, end)
        for ln in line_values:
            if lo <= ln <= hi:
                return True
    return False


# Agents completely excluded from winning per-file attribution.
# Copilot fires on every VS Code document change — including edits by other
# AI agents running in the terminal and normal human typing. Letting it win
# attribution would mask real sources. We keep capturing its events in
# session.jsonl (for usage stats) but never let it claim a file.
_EXCLUDED_AGENTS = {"copilot"}


def read_events_per_file(
    path: str,
    touched_files: List[str],
    min_ts: int,
    lines_by_file: Dict[str, List[Tuple[int, int]]],
) -> Dict[str, dict]:
    """Return the most-recent overlapping session event for each staged file.

    Returns a dict keyed by repo-relative file path. When multiple agents
    touched different files in the same session window each file gets its
    own correct attribution instead of a single winner-takes-all event.

    Excluded agents (copilot) are skipped entirely — the most recent
    non-excluded agent wins per file. If only excluded agents have events
    for a file, no event is returned and the file gets human attribution.
    """
    if not os.path.exists(path):
        return {}
    best: Dict[str, Tuple[int, dict]] = {}  # file -> (ts, event)
    try:
        with open(path, "r", encoding="utf-8") as f:
            for raw in f:
                line = raw.strip()
                if not line:
                    continue
                try:
                    event = json.loads(line)
                except Exception:
                    continue
                if not isinstance(event, dict):
                    continue
                event_ts = parse_event_ts(str(event.get("timestamp") or ""))
                if event_ts < min_ts:
                    continue
                file_path = str(event.get("file", "")).strip()
                if not file_path or file_path not in touched_files:
                    continue
                if not event_overlaps_staged(event, lines_by_file):
                    continue

                agent = str(event.get("agent") or "human")
                # Never attribute to excluded agents, even as a last resort.
                if agent in _EXCLUDED_AGENTS:
                    continue

                prev_ts, _ = best.get(file_path, (0, None))
                if event_ts >= prev_ts:
                    # Most recent non-excluded agent wins per file.
                    best[file_path] = (event_ts, event)
    except Exception:
        return {}
    return {fp: ev for fp, (_, ev) in best.items()}


def dominant_event(events_by_file: Dict[str, dict], lines_by_file: Dict[str, List[Tuple[int, int]]]) -> dict:
    """Pick the agent/model to use as the top-level record field.

    Chooses the agent that wrote the most lines across all staged files.
    Falls back to most-recently-touched file if line counts are equal.
    """
    if not events_by_file:
        return {}
    line_count: Dict[str, int] = {}
    for fp, event in events_by_file.items():
        agent = str(event.get("agent") or "human")
        ranges = lines_by_file.get(fp, [])
        count = sum(max(0, b - a + 1) for a, b in ranges)
        line_count[agent] = line_count.get(agent, 0) + count
    dominant_agent = max(line_count, key=lambda a: line_count[a])
    # Return the most recent event for the dominant agent
    for fp, event in reversed(list(events_by_file.items())):
        if str(event.get("agent") or "human") == dominant_agent:
            return event
    return next(iter(events_by_file.values()))


def get_git_username(repo_root: str) -> str:
    """Return git user.name; falls back to 'human' if unset."""
    try:
        out = run(["git", "config", "user.name"], cwd=repo_root)
        name = out.stdout.strip()
        return name if name else "human"
    except Exception:
        return "human"


def prompt_excerpt(prompt: str, limit: int = 160) -> str:
    text = " ".join((prompt or "").split())
    if not text:
        return ""
    if len(text) <= limit:
        return text
    return text[: max(0, limit - 3)] + "..."


def sha256_text(text: str) -> str:
    return hashlib.sha256((text or "").encode("utf-8")).hexdigest()


def main() -> int:
    if len(sys.argv) < 5:
        print(
            "usage: prepare-ledger.py <repo_root> <session_log> <pending_context> <pending_ledger>",
            file=sys.stderr,
        )
        return 2

    repo_root = os.path.abspath(sys.argv[1])
    session_log = os.path.abspath(sys.argv[2])
    pending_context_path = os.path.abspath(sys.argv[3])
    pending_ledger_path = os.path.abspath(sys.argv[4])

    if not os.path.exists(os.path.join(repo_root, ".git")):
        return 0

    lines_by_file = parse_staged_changes(repo_root)
    if not lines_by_file:
        return 0

    # Exclude agentdiff metadata from attribution — it's auto-generated, not AI code.
    lines_by_file = {
        f: r for f, r in lines_by_file.items()
        if not f.startswith(".agentdiff/")
    }
    if not lines_by_file:
        return 0

    files_touched = sorted(lines_by_file.keys())
    pending = read_json_file(pending_context_path)
    if not isinstance(pending, dict):
        pending = {}

    events_by_file = read_events_per_file(
        session_log,
        files_touched,
        head_commit_ts(repo_root),
        lines_by_file,
    )

    # Top-level agent/model/session come from the dominant event (most lines written)
    event = dominant_event(events_by_file, lines_by_file) or {}

    prompt = str(pending.get("prompt") or event.get("prompt") or "")
    session_id = str(pending.get("session_id") or event.get("session_id") or "unknown")
    agent = str(pending.get("agent") or event.get("agent") or "human")
    if agent == "human":
        agent = get_git_username(repo_root)
    model = str(pending.get("model_id") or pending.get("model") or event.get("model") or "human")
    files_read = pending.get("files_read")
    if not isinstance(files_read, list):
        files_read = []
    files_read = [str(p) for p in files_read]

    trust = pending.get("trust")
    if isinstance(trust, bool) or not isinstance(trust, int):
        trust = None
    if isinstance(trust, int):
        trust = max(0, min(100, trust))

    flags = pending.get("flags")
    if not isinstance(flags, list):
        flags = []
    flags = [str(f) for f in flags]

    intent = pending.get("intent")
    if intent is not None:
        intent = str(intent)

    # Per-file attribution — each file maps to the agent/model that wrote it.
    # Only populated when multiple agents are detected; omitted for single-agent commits.
    attribution: Dict[str, dict] = {}
    for fp, ev in events_by_file.items():
        file_agent = str(ev.get("agent") or "human")
        file_model = str(ev.get("model") or "human")
        if file_agent != agent or file_model != model:
            # Only store when it differs from the dominant agent (saves space for single-agent commits)
            attribution[fp] = {
                "agent": file_agent,
                "model": file_model,
                "session_id": str(ev.get("session_id") or "unknown"),
                "tool": str(ev.get("tool") or "commit"),
            }

    payload = {
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "agent": agent,
        "model": model,
        "session_id": session_id,
        "files_touched": files_touched,
        "lines": {
            file_path: [[int(a), int(b)] for (a, b) in ranges]
            for file_path, ranges in lines_by_file.items()
        },
        "prompt_excerpt": prompt_excerpt(prompt),
        "prompt_hash": sha256_text(prompt),
        "files_read": files_read,
        "flags": flags,
        "tool": str(event.get("tool") or "commit"),
        "mode": event.get("mode"),
    }
    if attribution:
        payload["attribution"] = attribution
    if intent:
        payload["intent"] = intent
    if trust is not None:
        payload["trust"] = trust

    parent = os.path.dirname(pending_ledger_path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(pending_ledger_path, "w", encoding="utf-8") as f:
        json.dump(payload, f, separators=(",", ":"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
