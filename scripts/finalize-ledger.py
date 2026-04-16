#!/ usr/bin/env python3
"""
Finalize pending ledger snapshot after commit.

Writes to the agentdiff-meta branch (enterprise storage, no working-tree
pollution) via git plumbing. Falls back to appending to ledger.jsonl on disk
if the git plumbing fails (e.g. first-time setup, no git binary on PATH).

Also writes an AgentTrace record to the local per-branch trace buffer at
.git/agentdiff/traces/{branch}.jsonl so that `agentdiff list` works without
needing a push/consolidate first.

Usage:
  finalize-ledger.py <repo_root> <pending_ledger> <pending_context> [<ledger_path>]
"""
import json
import os
import subprocess
import sys
import tempfile
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
                # Strip inline TOML comments before comparing the value.
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


def sha_in_content(content: str, sha: str) -> bool:
    """Check if a SHA already exists in JSONL content."""
    for raw in content.splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except Exception:
            continue
        if isinstance(obj, dict) and obj.get("sha") == sha:
            return True
    return False


def get_meta_branch_content(repo_root: str) -> Optional[str]:
    """Read current ledger.jsonl from agentdiff-meta branch. Returns None if not found."""
    res = run(["git", "show", "agentdiff-meta:ledger.jsonl"], cwd=repo_root)
    if res.returncode == 0:
        return res.stdout
    return None


def write_to_meta_branch(repo_root: str, new_content: str) -> bool:
    """
    Write new_content as ledger.jsonl on the agentdiff-meta branch using
    git plumbing — no checkout, no working-tree changes.
    Returns True on success.
    """
    tmp_path = None
    try:
        # Write content to a temp file so git can hash it.
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".jsonl", delete=False, encoding="utf-8"
        ) as f:
            f.write(new_content)
            tmp_path = f.name

        # Hash the file to get a blob SHA.
        blob_res = run(["git", "hash-object", "-w", tmp_path], cwd=repo_root)
        if blob_res.returncode != 0:
            return False
        blob_sha = blob_res.stdout.strip()

        # Create a tree containing just ledger.jsonl.
        tree_input = f"100644 blob {blob_sha}\tledger.jsonl\n"
        tree_res = subprocess.run(
            ["git", "mktree"],
            input=tree_input,
            text=True,
            capture_output=True,
            cwd=repo_root,
        )
        if tree_res.returncode != 0:
            return False
        tree_sha = tree_res.stdout.strip()

        # Find parent commit if the branch already exists.
        parent_res = run(
            ["git", "rev-parse", "refs/heads/agentdiff-meta"], cwd=repo_root
        )
        parent_args: List[str] = []
        if parent_res.returncode == 0:
            parent_sha = parent_res.stdout.strip()
            if parent_sha:
                parent_args = ["-p", parent_sha]

        # Get short SHA of the current HEAD for the commit message.
        short_res = run(["git", "rev-parse", "--short", "HEAD"], cwd=repo_root)
        short_sha = short_res.stdout.strip() if short_res.returncode == 0 else "?"

        # Create the commit object.
        commit_res = subprocess.run(
            ["git", "commit-tree", tree_sha, "-m", f"agentdiff: {short_sha}"]
            + parent_args,
            text=True,
            capture_output=True,
            cwd=repo_root,
        )
        if commit_res.returncode != 0:
            return False
        commit_sha = commit_res.stdout.strip()

        # Update the branch ref.
        ref_res = run(
            ["git", "update-ref", "refs/heads/agentdiff-meta", commit_sha],
            cwd=repo_root,
        )
        return ref_res.returncode == 0

    except Exception:
        return False
    finally:
        if tmp_path and os.path.exists(tmp_path):
            try:
                os.unlink(tmp_path)
            except Exception:
                pass


def write_agent_trace(repo_root: str, pending: dict, sha: str, ts: str) -> bool:
    """Append an AgentTrace record to .git/agentdiff/traces/{branch}.jsonl.

    This populates the local trace buffer so `agentdiff list` works immediately
    after a commit, without needing a push/consolidate first.
    """
    # Get current branch name.
    branch_res = run(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=repo_root)
    if branch_res.returncode != 0:
        return False
    branch = branch_res.stdout.strip()
    if not branch or branch == "HEAD":
        return False

    # Sanitize branch name to match store.rs: replace / with %2F.
    sanitized = branch.replace("/", "%2F")

    # Ensure traces directory exists.
    traces_dir = os.path.join(repo_root, ".git", "agentdiff", "traces")
    os.makedirs(traces_dir, exist_ok=True)
    traces_path = os.path.join(traces_dir, f"{sanitized}.jsonl")

    # Build per-file trace entries from pending payload.
    agent = str(pending.get("agent") or "human")
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
        return True  # Nothing to record, not an error.

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

    trace: dict = {
        "version": "0.1.0",
        "id": str(uuid_mod.uuid4()),
        "timestamp": ts,
        "vcs": {"type": "git", "revision": sha},
        "tool": {"name": agent},
        "files": files,
    }
    if metadata:
        trace["metadata"] = metadata

    line = json.dumps(trace, separators=(",", ":")) + "\n"
    try:
        with open(traces_path, "a", encoding="utf-8") as f:
            f.write(line)
        return True
    except Exception:
        return False


def sha_exists_on_disk(ledger_path: str, sha: str) -> bool:
    if not os.path.exists(ledger_path):
        return False
    try:
        with open(ledger_path, "r", encoding="utf-8") as f:
            return sha_in_content(f.read(), sha)
    except Exception:
        return False


def main() -> int:
    if len(sys.argv) < 4:
        print(
            "usage: finalize-ledger.py <repo_root> <pending_ledger> <pending_context> [<ledger_path>]",
            file=sys.stderr,
        )
        return 2

    repo_root = os.path.abspath(sys.argv[1])
    pending_ledger_path = os.path.abspath(sys.argv[2])
    pending_context_path = os.path.abspath(sys.argv[3])
    # ledger_path is optional — only used as fallback when git ref write fails.
    ledger_path = os.path.abspath(sys.argv[4]) if len(sys.argv) >= 5 else None

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

    # Check if SHA already recorded — try meta branch first, then disk file.
    meta_content = get_meta_branch_content(repo_root)
    if meta_content is not None:
        if sha_in_content(meta_content, sha):
            remove_if_exists(pending_ledger_path)
            remove_if_exists(pending_context_path)
            return 0
    elif ledger_path and sha_exists_on_disk(ledger_path, sha):
        remove_if_exists(pending_ledger_path)
        remove_if_exists(pending_context_path)
        return 0

    ts_res = run(["git", "show", "-s", "--format=%cI", "HEAD"], cwd=repo_root)
    if ts_res.returncode != 0:
        return 1
    ts = ts_res.stdout.strip()

    author_res = run(["git", "show", "-s", "--format=%an", "HEAD"], cwd=repo_root)
    author = author_res.stdout.strip() if author_res.returncode == 0 else ""

    prompts_on = _capture_prompts_enabled()
    entry: dict = {
        "sha": sha,
        "ts": ts,
        "agent": str(pending.get("agent") or "human"),
        "model": str(pending.get("model") or "human"),
        "session_id": str(pending.get("session_id") or "unknown"),
        "author": author or None,
        "files_touched": pending.get("files_touched") if isinstance(pending.get("files_touched"), list) else [],
        "lines": pending.get("lines") if isinstance(pending.get("lines"), dict) else {},
        "prompt_excerpt": str(pending.get("prompt_excerpt") or "") if prompts_on else "",
        "prompt_hash": str(pending.get("prompt_hash") or "") if prompts_on else "",
        "files_read": pending.get("files_read") if isinstance(pending.get("files_read"), list) else [],
        "flags": pending.get("flags") if isinstance(pending.get("flags"), list) else [],
        "tool": str(pending.get("tool") or "commit"),
        "mode": pending.get("mode"),
    }

    if pending.get("intent"):
        entry["intent"] = str(pending.get("intent"))
    if isinstance(pending.get("trust"), int):
        entry["trust"] = max(0, min(100, int(pending["trust"])))
    if isinstance(pending.get("attribution"), dict):
        entry["attribution"] = pending["attribution"]

    entry = {k: v for (k, v) in entry.items() if v is not None}
    entry_line = json.dumps(entry, separators=(",", ":")) + "\n"

    # Write AgentTrace format to the local per-branch buffer so that
    # `agentdiff list` works immediately (no push/consolidate required).
    write_agent_trace(repo_root, pending, sha, ts)

    # Try writing to agentdiff-meta branch (enterprise storage).
    existing = meta_content if meta_content is not None else ""
    if write_to_meta_branch(repo_root, existing + entry_line):
        remove_if_exists(pending_ledger_path)
        remove_if_exists(pending_context_path)
        return 0

    # Fallback: append to working-tree ledger.jsonl (only when path is provided).
    if not ledger_path:
        remove_if_exists(pending_ledger_path)
        remove_if_exists(pending_context_path)
        return 1  # git ref write failed and no disk fallback configured

    parent = os.path.dirname(ledger_path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(ledger_path, "a", encoding="utf-8") as f:
        f.write(entry_line)

    remove_if_exists(pending_ledger_path)
    remove_if_exists(pending_context_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
