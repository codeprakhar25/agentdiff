import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "capture-codex.py"


def init_repo(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "init"], cwd=path, check=True, capture_output=True)
    subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=path, check=True, capture_output=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=path, check=True, capture_output=True)


def commit_file(repo: Path, rel: str, content: str) -> Path:
    p = repo / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    subprocess.run(["git", "add", rel], cwd=repo, check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", f"add {rel}"], cwd=repo, check=True, capture_output=True)
    return p


class CaptureCodexTests(unittest.TestCase):
    def test_prefers_process_cwd_over_unrelated_recent_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            repo_a = root / "repo_a"
            repo_b = root / "repo_b"
            init_repo(repo_a)
            init_repo(repo_b)

            file_a = commit_file(repo_a, "src/a.txt", "one\n")
            file_b = commit_file(repo_b, "src/b.txt", "alpha\n")

            # Simulate agentdiff init in both repos.
            (repo_a / ".git" / "agentdiff").mkdir(parents=True, exist_ok=True)
            (repo_b / ".git" / "agentdiff").mkdir(parents=True, exist_ok=True)

            # Create pending changes in both repos.
            file_a.write_text("one\ntwo\n", encoding="utf-8")
            file_b.write_text("alpha\nbeta\n", encoding="utf-8")

            # Fake Codex sessions where the newest rollout points at repo_b.
            sessions_root = root / "sessions"
            sessions_root.mkdir(parents=True, exist_ok=True)
            rollout = sessions_root / "rollout-newest.jsonl"
            rollout.write_text(
                json.dumps(
                    {
                        "type": "session_meta",
                        "payload": {"id": "sid-wrong", "cwd": str(repo_b)},
                    }
                )
                + "\n",
                encoding="utf-8",
            )

            env = os.environ.copy()
            env["CODEX_SESSIONS_ROOT"] = str(sessions_root)

            payload = {"event": "agent-turn-complete"}
            proc = subprocess.run(
                ["python3", str(SCRIPT_PATH)],
                input=json.dumps(payload),
                text=True,
                cwd=repo_a,
                env=env,
                capture_output=True,
            )
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_a = repo_a / ".git" / "agentdiff" / "session.jsonl"
            self.assertTrue(session_a.exists())
            lines = [ln for ln in session_a.read_text(encoding="utf-8").splitlines() if ln.strip()]
            self.assertEqual(len(lines), 1)

            entry = json.loads(lines[0])
            self.assertEqual(entry["agent"], "codex")
            self.assertEqual(entry["file"], "src/a.txt")


if __name__ == "__main__":
    unittest.main()
