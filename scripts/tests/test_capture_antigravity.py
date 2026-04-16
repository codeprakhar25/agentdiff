import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "capture-antigravity.py"


def init_repo(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "init"], cwd=path, check=True, capture_output=True)
    subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=path, check=True, capture_output=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=path, check=True, capture_output=True)


class CaptureAntigravityTests(unittest.TestCase):
    def test_before_after_hook_flow_uses_cached_prompt(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp) / "home"
            repo = Path(tmp) / "repo"
            home.mkdir(parents=True, exist_ok=True)
            init_repo(repo)

            # Simulate agentdiff init: create .git/agentdiff/ so capture fires.
            (repo / ".git" / "agentdiff").mkdir(parents=True, exist_ok=True)

            file_path = repo / "src" / "main.ts"
            file_path.parent.mkdir(parents=True, exist_ok=True)
            file_path.write_text("const greeting = 'hello';\n", encoding="utf-8")
            subprocess.run(["git", "add", "src/main.ts"], cwd=repo, check=True, capture_output=True)
            subprocess.run(["git", "commit", "-m", "init"], cwd=repo, check=True, capture_output=True)

            before_payload = {
                "hook_event_name": "BeforeTool",
                "tool_name": "replace",
                "session_id": "sess-123",
                "cwd": str(repo),
                "user_prompt": "update greeting",
            }

            env = os.environ.copy()
            env["HOME"] = str(home)

            before_proc = subprocess.run(
                ["python3", str(SCRIPT_PATH)],
                input=json.dumps(before_payload),
                text=True,
                cwd=repo,
                env=env,
                capture_output=True,
            )
            print("Before payload:", before_proc)
            self.assertEqual(before_proc.returncode, 0, msg=before_proc.stderr)

            res = file_path.write_text("const greeting = 'hello world';\n", encoding="utf-8")
            print("File write result:", res)
            after_payload = {
                "hook_event_name": "AfterTool",
                "tool_name": "replace",
                "session_id": "sess-123",
                "cwd": str(repo),
                "model": "gemini-2.5-pro",
                "tool_input": {
                    "file_path": str(file_path),
                    "old_string": "hello",
                    "new_string": "hello world",
                },
            }
            print("After payload:", after_payload)
            after_proc = subprocess.run(
                ["python3", str(SCRIPT_PATH)],
                input=json.dumps(after_payload),
                text=True,
                cwd=repo,
                env=env,
                capture_output=True,
            )
            print("After stdout:", after_proc.stdout)
            print("After stderr:", after_proc.stderr)
            self.assertEqual(after_proc.returncode, 0, msg=after_proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            self.assertTrue(session_log.exists())
            lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
            self.assertEqual(len(lines), 1)
            entry = json.loads(lines[0])

            self.assertEqual(entry["agent"], "antigravity")
            self.assertEqual(entry["model"], "gemini-2.5-pro")
            self.assertEqual(entry["file"], "src/main.ts")
            self.assertEqual(entry["prompt"], "update greeting")
            self.assertTrue(entry.get("lines"))


if __name__ == "__main__":
    unittest.main()
