#!/usr/bin/env python3
"""
Finalize buffered agentdiff session events into a git note on HEAD.

Usage:
  write-note.py <repo_root> <session_log>
"""
import hashlib
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from typing import Dict, List, Tuple


def run(cmd: List[str], cwd: str, input_text: str = "") -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        input=input_text,
        capture_output=True,
    )


def parse_changed_lines(repo_root: str, commit_hash: str) -> Dict[str, List[int]]:
    result = run(["git", "show", "--unified=0", "--no-color", "--pretty=format:", commit_hash], cwd=repo_root)
    if result.returncode != 0:
        return {}

    changed: Dict[str, set] = {}
    current_file = ""

    for raw in result.stdout.splitlines():
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

        if not line.startswith("@@"):
            continue

        # @@ -old_start,old_count +new_start,new_count @@
        m = re.search(r"\+(\d+)(?:,(\d+))?", line)
        if not m or not current_file:
            continue

        start = int(m.group(1))
        count = int(m.group(2) or "1")
        if count <= 0:
            continue

        for ln in range(start, start + count):
            changed[current_file].add(ln)

    return {k: sorted(v) for k, v in changed.items()}


def normalize_lines(raw_lines) -> List[int]:
    if not isinstance(raw_lines, list):
        return []
    out = []
    for v in raw_lines:
        try:
            n = int(v)
            if n > 0:
                out.append(n)
        except Exception:
            continue
    return sorted(set(out))


def compress_ranges(lines: List[int]) -> List[Tuple[int, int]]:
    if not lines:
        return []
    ranges: List[Tuple[int, int]] = []
    start = lines[0]
    prev = lines[0]
    for ln in lines[1:]:
        if ln == prev + 1:
            prev = ln
            continue
        ranges.append((start, prev))
        start = ln
        prev = ln
    ranges.append((start, prev))
    return ranges


def prompt_excerpt(prompt: str, limit: int = 160) -> str:
    if not prompt:
        return ""
    text = " ".join(prompt.split())
    if len(text) <= limit:
        return text
    return text[: max(0, limit - 3)] + "..."


def prompt_hash(prompt: str) -> str:
    return hashlib.sha256((prompt or "").encode("utf-8")).hexdigest()


def session_ref(session_id: str) -> str:
    return hashlib.sha256((session_id or "unknown").encode("utf-8")).hexdigest()[:16]


def derive_intent(raw_intent: str, excerpt: str) -> str:
    text = (raw_intent or "").strip()
    if text:
        return text[:120]
    if not excerpt:
        return ""
    sentence = excerpt.split(".")[0].strip()
    return sentence[:120]


def read_events(path: str) -> List[dict]:
    if not os.path.exists(path):
        return []
    events = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
                if isinstance(obj, dict):
                    events.append(obj)
            except Exception:
                continue
    return events


def write_events(path: str, events: List[dict]) -> None:
    parent = os.path.dirname(path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        for e in events:
            f.write(json.dumps(e, separators=(",", ":")) + "\n")


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: write-note.py <repo_root> <session_log>", file=sys.stderr)
        return 2

    repo_root = os.path.abspath(sys.argv[1])
    session_log = os.path.abspath(sys.argv[2])

    if not os.path.exists(os.path.join(repo_root, ".git")):
        return 0

    events = read_events(session_log)
    if not events:
        return 0

    commit_res = run(["git", "rev-parse", "HEAD"], cwd=repo_root)
    if commit_res.returncode != 0:
        print(commit_res.stderr.strip(), file=sys.stderr)
        return 1

    commit_hash = commit_res.stdout.strip()
    changed_by_file = parse_changed_lines(repo_root, commit_hash)

    contributors: Dict[str, dict] = {}
    files = []
    trace_ids = set()
    remaining = []

    for e in events:
        file_path = str(e.get("file", "")).strip()
        if not file_path or file_path not in changed_by_file:
            remaining.append(e)
            continue

        changed_lines = changed_by_file.get(file_path, [])
        if not changed_lines:
            remaining.append(e)
            continue

        event_lines = normalize_lines(e.get("lines", []))
        if event_lines:
            changed_set = set(changed_lines)
            selected = [ln for ln in event_lines if ln in changed_set]
            if not selected:
                selected = changed_lines
        else:
            selected = changed_lines

        selected = sorted(set(selected))
        if not selected:
            remaining.append(e)
            continue

        agent = str(e.get("agent", "unknown"))
        model = str(e.get("model", "unknown"))
        sess = str(e.get("session_id", "unknown"))
        prompt = str(e.get("prompt", "") or "")
        excerpt = prompt_excerpt(prompt)
        intent = derive_intent(str(e.get("intent", "") or ""), excerpt)

        sid_ref = session_ref(sess)
        phash = prompt_hash(prompt)
        ckey = f"{agent}|{model}|{sid_ref}|{intent}|{excerpt}|{phash}"
        cid = "c" + hashlib.sha1(ckey.encode("utf-8")).hexdigest()[:12]

        contributors[cid] = {
            "id": cid,
            "agent": agent,
            "model": model,
            "session_ref": sid_ref,
            "intent": intent,
            "prompt_excerpt": excerpt,
            "prompt_hash": phash,
        }

        tool = str(e.get("tool", "unknown"))
        files.append(
            {
                "path": file_path,
                "tool": tool,
                "contributor_id": cid,
                "ranges": compress_ranges(selected),
            }
        )

        trace_ids.add(hashlib.sha1(f"{sess}:{file_path}:{tool}".encode("utf-8")).hexdigest()[:16])

    if not files:
        # Nothing to commit for this revision; keep events for later.
        write_events(session_log, remaining)
        return 0

    note = {
        "version": "1.0.0",
        "commit": commit_hash,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "contributors": sorted(contributors.values(), key=lambda c: c["id"]),
        "files": sorted(files, key=lambda f: (f["path"], f["tool"], f["contributor_id"])),
        "trace_ids": sorted(trace_ids),
    }

    payload = json.dumps(note, separators=(",", ":"))
    write_res = run(
        ["git", "notes", "--ref=agentdiff", "add", "-f", "-F", "-", commit_hash],
        cwd=repo_root,
        input_text=payload,
    )
    if write_res.returncode != 0:
        print(write_res.stderr.strip(), file=sys.stderr)
        return 1

    write_events(session_log, remaining)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
