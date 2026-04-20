#!/usr/bin/env python3
"""
Smoke test for agentdiff-mcp framed stdio + ledger finalize path.
"""
import json
import os
import select
import stat
import subprocess
import tempfile
import textwrap
import time
from pathlib import Path


def run(cmd, cwd=None, input_text=None, check=True):
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        input=input_text,
        text=True,
        capture_output=True,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(
            f"command failed: {cmd}\nrc={proc.returncode}\nstdout={proc.stdout}\nstderr={proc.stderr}"
        )
    return proc


def framed_send(proc: subprocess.Popen, msg: dict) -> None:
    body = json.dumps(msg)
    payload = f"Content-Length: {len(body)}\r\n\r\n{body}".encode("utf-8")
    proc.stdin.write(payload)
    proc.stdin.flush()


def framed_recv(proc: subprocess.Popen, timeout_s: float = 5.0) -> dict:
    fd = proc.stdout.fileno()
    header = b""
    deadline = time.time() + timeout_s

    while b"\r\n\r\n" not in header:
        left = deadline - time.time()
        if left <= 0:
            raise TimeoutError("timed out waiting for MCP headers")
        ready, _, _ = select.select([fd], [], [], left)
        if not ready:
            raise TimeoutError("timed out waiting for MCP headers")
        chunk = os.read(fd, 1)
        if not chunk:
            raise EOFError("MCP stdout closed before header was complete")
        header += chunk

    head_raw, _ = header.split(b"\r\n\r\n", 1)
    content_length = None
    for line in head_raw.decode("utf-8", errors="replace").split("\r\n"):
        if ":" not in line:
            continue
        k, v = line.split(":", 1)
        if k.strip().lower() == "content-length":
            content_length = int(v.strip())
            break
    if content_length is None:
        raise RuntimeError("missing content-length in MCP response")

    body = b""
    while len(body) < content_length:
        left = deadline - time.time()
        if left <= 0:
            raise TimeoutError("timed out waiting for MCP body")
        ready, _, _ = select.select([fd], [], [], left)
        if not ready:
            raise TimeoutError("timed out waiting for MCP body")
        chunk = os.read(fd, content_length - len(body))
        if not chunk:
            raise EOFError("MCP stdout closed before body was complete")
        body += chunk

    return json.loads(body.decode("utf-8"))


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    prep = repo_root / "scripts" / "prepare-ledger.py"
    final = repo_root / "scripts" / "finalize-ledger.py"
    mcp_bin = os.environ.get(
        "AGENTDIFF_MCP_BIN",
        str(repo_root / "target" / "debug" / "agentdiff-mcp"),
    )

    with tempfile.TemporaryDirectory(prefix="agentdiff-mcp-smoke-", dir="/tmp") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir(parents=True)

        run(["git", "init"], cwd=repo)
        run(["git", "config", "user.name", "MCP Smoke"], cwd=repo)
        run(["git", "config", "user.email", "mcp-smoke@example.com"], cwd=repo)
        (repo / ".agentdiff").mkdir(exist_ok=True)
        (repo / ".git" / "agentdiff").mkdir(parents=True, exist_ok=True)

        pre_hook = textwrap.dedent(
            f"""#!/usr/bin/env bash
            set -euo pipefail
            python3 "{prep}" "{repo}" "{repo / '.git/agentdiff/session.jsonl'}" "{repo / '.git/agentdiff/pending.json'}" "{repo / '.git/agentdiff/pending-ledger.json'}"
            """
        )
        post_hook = textwrap.dedent(
            f"""#!/usr/bin/env bash
            set -euo pipefail
            python3 "{final}" "{repo}" "{repo / '.git/agentdiff/pending-ledger.json'}" "{repo / '.git/agentdiff/pending.json'}" "{repo / '.agentdiff/ledger.jsonl'}"
            """
        )

        pre_path = repo / ".git" / "hooks" / "pre-commit"
        post_path = repo / ".git" / "hooks" / "post-commit"
        pre_path.write_text(pre_hook, encoding="utf-8")
        post_path.write_text(post_hook, encoding="utf-8")
        os.chmod(pre_path, os.stat(pre_path).st_mode | stat.S_IXUSR)
        os.chmod(post_path, os.stat(post_path).st_mode | stat.S_IXUSR)

        test_file = repo / "mcp-smoke.txt"
        test_file.write_text("mcp smoke\n", encoding="utf-8")

        proc = subprocess.Popen(
            [mcp_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        framed_send(
            proc,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "smoke", "version": "1"},
                },
            },
        )
        framed_send(
            proc,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "record_context",
                    "arguments": {
                        "cwd": str(repo),
                        "agent": "codex",
                        "model_id": "mcp-smoke-model",
                        "session_id": "mcp-smoke-1",
                        "prompt": "mcp smoke prompt",
                        "files_read": ["mcp-smoke.txt"],
                        "intent": "mcp smoke",
                        "trust": 87,
                        "flags": ["smoke"],
                    },
                },
            },
        )
        r1 = framed_recv(proc)
        r2 = framed_recv(proc)
        proc.stdin.close()
        proc.wait(timeout=3)

        if r1.get("id") != 1 or "result" not in r1:
            raise RuntimeError(f"unexpected initialize response: {r1}")
        if r2.get("id") != 2 or "error" in r2:
            raise RuntimeError(f"unexpected tools/call response: {r2}")

        run(["git", "add", "mcp-smoke.txt"], cwd=repo)
        run(["git", "commit", "-m", "test: mcp smoke"], cwd=repo)

        # finalize-ledger.py writes AgentTrace records to .git/agentdiff/traces/{branch}.jsonl.
        branch_res = run(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=repo)
        branch = branch_res.stdout.strip().replace("/", "%2F")
        traces_path = repo / ".git" / "agentdiff" / "traces" / f"{branch}.jsonl"
        if not traces_path.exists():
            raise RuntimeError(f"traces file not found: {traces_path}")
        rows = [json.loads(x) for x in traces_path.read_text(encoding="utf-8").splitlines() if x.strip()]
        if not rows:
            raise RuntimeError("traces file is empty after commit")
        entry = rows[-1]
        tool_name = (entry.get("tool") or {}).get("name")
        if tool_name != "codex":
            raise RuntimeError(f"expected tool.name=codex in trace entry, got {entry}")
        files = entry.get("files") or []
        if not files:
            raise RuntimeError(f"expected files in trace entry, got {entry}")
        model_id = ((files[0].get("conversations") or [{}])[0].get("contributor") or {}).get("model_id")
        if model_id != "mcp-smoke-model":
            raise RuntimeError(f"expected model_id=mcp-smoke-model in trace entry, got {entry}")
        session_id = ((entry.get("metadata") or {}).get("agentdiff") or {}).get("session_id")
        if session_id != "mcp-smoke-1":
            raise RuntimeError(f"expected session_id=mcp-smoke-1 in trace entry, got {entry}")

    print("mcp smoke test: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
