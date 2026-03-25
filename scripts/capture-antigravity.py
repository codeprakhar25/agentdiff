#!/usr/bin/env python3
"""
AgentDiff capture script for Antigravity / batch agents.
Uses git diff as source of truth.
"""
import os
import sys
import json
import subprocess
from datetime import datetime, timezone


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


def parse_diff(repo_root: str, diff_output: str) -> dict:
    """Parse unified diff to extract added line numbers per file."""
    result = {}
    current_file = None

    for line in diff_output.split("\n"):
        if line.startswith("diff --git"):
            # Extract file path
            parts = line.split()
            if len(parts) >= 4:
                current_file = parts[-1].replace("b/", "")
                result[current_file] = []
        elif line.startswith("@@"):
            # Parse hunk header to get starting line
            # @@ -start,count +start,count @@
            parts = line.split()
            if len(parts) >= 2:
                after_plus = parts[2][1:]
                start_line = int(after_plus.split(",")[0])
                result[current_file].append(("start", start_line))
        elif line.startswith("+") and not line.startswith("+++"):
            # Added line
            if current_file and result.get(current_file):
                # Get the last start line and count additions
                if result[current_file] and result[current_file][-1][0] == "start":
                    start = result[current_file][-1][1]
                    # Count how many + lines since the start
                    result[current_file].append(("add", start))
                    result[current_file][-2] = ("line", start)

    return result


def main():
    # Parse arguments
    args = sys.argv[1:]
    prompt = "unknown"
    model = "antigravity"

    i = 0
    while i < len(args):
        if args[i] == "--prompt" and i + 1 < len(args):
            prompt = args[i + 1]
            i += 2
        elif args[i] == "--model" and i + 1 < len(args):
            model = args[i + 1]
            i += 2
        else:
            i += 1

    cwd = os.getcwd()
    repo_root = find_repo_root(cwd)

    if not os.path.exists(os.path.join(repo_root, ".git")):
        print("Not in a git repository", file=sys.stderr)
        sys.exit(1)

    # Get git diff as ground truth
    try:
        result = subprocess.run(
            ["git", "diff", "--no-color"],
            capture_output=True, text=True, cwd=repo_root
        )
        diff_output = result.stdout

        if not diff_output.strip():
            # Try staged changes
            result = subprocess.run(
                ["git", "diff", "--cached", "--no-color"],
                capture_output=True, text=True, cwd=repo_root
            )
            diff_output = result.stdout
    except Exception as e:
        print(f"Error running git diff: {e}", file=sys.stderr)
        sys.exit(1)

    # Parse diff to find added lines
    added_by_file = {}

    current_file = None
    line_num = 0

    for line in diff_output.split("\n"):
        if line.startswith("diff --git"):
            parts = line.split()
            if len(parts) >= 4:
                current_file = parts[-1].replace("b/", "")
                added_by_file[current_file] = []
        elif line.startswith("@@"):
            # Parse to get starting line
            # @@ -old_start,old_count +new_start,new_count @@
            import re
            m = re.search(r"\+(\d+)", line)
            if m:
                line_num = int(m.group(1))
                if current_file:
                    added_by_file[current_file] = []
        elif line.startswith("+") and not line.startswith("+++"):
            if current_file:
                added_by_file[current_file].append(line_num)
            line_num += 1
        elif line.startswith(" ") or line.startswith("-"):
            line_num += 1

    timestamp = datetime.now(timezone.utc).isoformat()
    session_id = f"antigravity-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"

    session_log = get_session_log(cwd)

    for file_path, lines in added_by_file.items():
        if not lines:
            continue

        abs_file = os.path.join(repo_root, file_path)

        entry = {
            "timestamp": timestamp,
            "agent": "antigravity",
            "mode": "agent",
            "model": model,
            "session_id": session_id,
            "tool": "batch-edit",
            "file": file_path,
            "abs_file": abs_file,
            "prompt": prompt,
            "acceptance": "verbatim",
            "lines": sorted(set(lines)),
        }

        with open(session_log, "a") as f:
            f.write(json.dumps(entry) + "\n")

    print(f"Captured {len(added_by_file)} files with {sum(len(v) for v in added_by_file.values())} total lines")


if __name__ == "__main__":
    main()
