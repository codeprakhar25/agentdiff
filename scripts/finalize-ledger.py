#!/usr/bin/env python3
"""
Finalize pending ledger snapshot after commit.

Writes an AgentTrace record to the local per-branch trace buffer at
.git/agentdiff/traces/{branch}.jsonl so that `agentdiff list` works without
needing a push/consolidate first.

Usage:
  finalize-ledger.py <repo_root> <pending_ledger> <pending_context>
"""
import json
import os
import subprocess
import sys
import uuid as uuid_mod
from typing import List, Optional


def _capture_prompts_enabled() -> bool:
    """Read capture_prompts setting from ~/.agentdiff/config.toml. Defaults to True.

    Note: this gates what ends up in final trace entries (git refs, pushed to GitHub).
    The ephemeral session.jsonl in .git/agentdiff/ may still contain prompts from
    non-Claude agents even when capture_prompts=false, since individual capture scripts
    each control their own session.jsonl writes. Only capture-claude.py currently
    respects this setting at capture time. The critical protection is here: nothing
    with capture_prompts=false reaches a git ref or remote.
    """
    config_path = os.path.expanduser("~/.agentdiff/config.toml")
    try:
        with open(config_path, encoding="utf-8") as f:
            content = f.read()
        for line in content.splitlines():
            stripped = line.strip().replace(" ", "").lower()
            if stripped.startswith("capture_prompts="):
                val = stripped.split("=", 1)[1].split("#")[0].strip()
                return val not in ("false", "0", "no", "off")
    except (OSError, IOError):
        pass
    return True


def run(cmd: List[str], cwd: str) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)


def read_json_file(path: str):
    if not os.path.exists(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return None


def remove_if_exists(path: str) -> None:
    try:
        if os.path.exists(path):
            os.remove(path)
    except Exception:
        pass


def sha_already_recorded(traces_path: str, sha: str) -> bool:
    """Skip finalize if this commit already has a trace recorded locally."""
    if not os.path.exists(traces_path):
        return False
    try:
        with open(traces_path, "r", encoding="utf-8") as f:
            for raw in f:
                line = raw.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except Exception:
                    continue
                vcs = obj.get("vcs") if isinstance(obj, dict) else None
                if isinstance(vcs, dict) and vcs.get("revision") == sha:
                    return True
    except (OSError, IOError):
        pass
    return False


def write_agent_trace(repo_root: str, pending: dict, sha: str, ts: str) -> Optional[str]:
    """Append an AgentTrace record to .git/agentdiff/traces/{branch}.jsonl.

    Returns the traces file path on success, None on failure.
    """
    branch_res = run(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=repo_root)
    if branch_res.returncode != 0:
        return None
    branch = branch_res.stdout.strip()
    if not branch or branch == "HEAD":
        return None

    # Sanitize branch name to match store.rs: replace / with %2F.
    sanitized = branch.replace("/", "%2F")

    traces_dir = os.path.join(repo_root, ".git", "agentdiff", "traces")
    os.makedirs(traces_dir, exist_ok=True)
    traces_path = os.path.join(traces_dir, f"{sanitized}.jsonl")

    # Build per-file trace entries from pending payload.
    agent = str(pending.get("agent") or "human")
    git_author = str(pending.get("git_author") or agent)
    model = str(pending.get("model") or "human")
    attribution = pending.get("attribution") or {}
    lines_map = pending.get("lines") or {}

    files = []
    for file_path, ranges in lines_map.items():
        file_attr = attribution.get(file_path, {})
        file_agent = str(file_attr.get("agent") or agent)
        file_model = str(file_attr.get("model") or model)

        contributor_type = "human" if file_agent == "human" else "ai"
        contributor: dict = {"type": contributor_type}
        if file_model and file_model != "human":
            contributor["model_id"] = file_model

        trace_ranges = []
        for r in ranges:
            if isinstance(r, (list, tuple)) and len(r) == 2:
                trace_ranges.append({
                    "start_line": int(min(r[0], r[1])),
                    "end_line": int(max(r[0], r[1])),
                })

        if trace_ranges:
            files.append({
                "path": file_path,
                "conversations": [{
                    "contributor": contributor,
                    "ranges": trace_ranges,
                }],
            })

    if not files:
        return traces_path  # Nothing to record, not an error.

    # Build metadata extension block.
    prompts_on = _capture_prompts_enabled()
    metadata: dict = {}
    if prompts_on:
        if pending.get("prompt_excerpt"):
            metadata["prompt_excerpt"] = str(pending["prompt_excerpt"])
        if pending.get("prompt_hash"):
            metadata["prompt_hash"] = str(pending["prompt_hash"])
    if isinstance(pending.get("trust"), int):
        metadata["trust"] = pending["trust"]
    if isinstance(pending.get("flags"), list) and pending["flags"]:
        metadata["flags"] = pending["flags"]
    if pending.get("session_id"):
        metadata["session_id"] = str(pending["session_id"])
    if pending.get("intent"):
        metadata["intent"] = str(pending["intent"])
    if isinstance(pending.get("files_read"), list) and pending["files_read"]:
        metadata["files_read"] = [str(p) for p in pending["files_read"]]
    if git_author:
        metadata["author"] = git_author
    if pending.get("tool"):
        metadata["capture_tool"] = str(pending["tool"])

    trace: dict = {
        "version": "0.1.0",
        "id": str(uuid_mod.uuid4()),
        "timestamp": ts,
        "vcs": {"type": "git", "revision": sha},
        "tool": {"name": git_author if agent == "human" else agent},
        "files": files,
    }
    _ = model  # captured above into per-file contributor.model_id
    if metadata:
        trace["metadata"] = {"agentdiff": metadata}

    line = json.dumps(trace, separators=(",", ":")) + "\n"
    try:
        with open(traces_path, "a", encoding="utf-8") as f:
            f.write(line)
        return traces_path
    except Exception:
        return None


def main() -> int:
    if len(sys.argv) < 4:
        print(
            "usage: finalize-ledger.py <repo_root> <pending_ledger> <pending_context>",
            file=sys.stderr,
        )
        return 2

    repo_root = os.path.abspath(sys.argv[1])
    pending_ledger_path = os.path.abspath(sys.argv[2])
    pending_context_path = os.path.abspath(sys.argv[3])

    if not os.path.exists(os.path.join(repo_root, ".git")):
        return 0

    pending = read_json_file(pending_ledger_path)
    if not isinstance(pending, dict):
        return 0

    sha_res = run(["git", "rev-parse", "HEAD"], cwd=repo_root)
    if sha_res.returncode != 0:
        return 1
    sha = sha_res.stdout.strip()
    if not sha:
        return 1

    # Skip if we've already recorded a trace for this SHA on this branch.
    branch_res = run(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=repo_root)
    branch = branch_res.stdout.strip() if branch_res.returncode == 0 else ""
    if branch and branch != "HEAD":
        sanitized = branch.replace("/", "%2F")
        existing = os.path.join(repo_root, ".git", "agentdiff", "traces", f"{sanitized}.jsonl")
        if sha_already_recorded(existing, sha):
            remove_if_exists(pending_ledger_path)
            remove_if_exists(pending_context_path)
            return 0

    ts_res = run(["git", "show", "-s", "--format=%cI", "HEAD"], cwd=repo_root)
    if ts_res.returncode != 0:
        return 1
    ts = ts_res.stdout.strip()

    result = write_agent_trace(repo_root, pending, sha, ts)
    remove_if_exists(pending_ledger_path)
    remove_if_exists(pending_context_path)
    return 0 if result is not None else 1


if __name__ == "__main__":
    raise SystemExit(main())
