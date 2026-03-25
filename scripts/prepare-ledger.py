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


def read_latest_session_event(
    path: str,
    touched_files: List[str],
    min_ts: int,
    lines_by_file: Dict[str, List[Tuple[int, int]]],
):
    if not os.path.exists(path):
        return None
    latest_for_touched = None
    latest_for_touched_ts = 0
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
                if file_path and file_path in touched_files:
                    if not event_overlaps_staged(event, lines_by_file):
                        continue
                    if event_ts >= latest_for_touched_ts:
                        latest_for_touched = event
                        latest_for_touched_ts = event_ts
    except Exception:
        return None
    return latest_for_touched


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

    files_touched = sorted(lines_by_file.keys())
    pending = read_json_file(pending_context_path)
    if not isinstance(pending, dict):
        pending = {}

    event = (
        read_latest_session_event(
            session_log,
            files_touched,
            head_commit_ts(repo_root),
            lines_by_file,
        )
        or {}
    )

    prompt = str(pending.get("prompt") or event.get("prompt") or "")
    session_id = str(pending.get("session_id") or event.get("session_id") or "unknown")
    agent = str(pending.get("agent") or event.get("agent") or "human")
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
