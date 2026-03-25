import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "capture-cursor.py"


def load_module():
    spec = importlib.util.spec_from_file_location("capture_cursor", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class CaptureCursorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mod = load_module()

    def test_normalize_windows_style_path(self):
        normalized = self.mod.normalize_path(r"\home\prakh\repo\src\main.rs", "/tmp")
        self.assertEqual(normalized, "/home/prakh/repo/src/main.rs")

    def test_session_log_uses_repo_hint(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)

            session_log = self.mod.get_session_log(str(repo), str(repo))
            expected = repo / ".git" / "agentdiff" / "session.jsonl"
            self.assertEqual(Path(session_log), expected)
            self.assertTrue(expected.parent.exists())

    def test_hook_event_writes_repo_session_when_cwd_not_repo(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
            src = repo / "src"
            src.mkdir()
            edited = src / "main.rs"
            edited.write_text("fn main() {}\n", encoding="utf-8")

            payload = {
                "hook_event_name": "afterTabFileEdit",
                "cwd": str(Path.home() / ".cursor"),
                "file_path": str(edited).replace("/", "\\"),
                "lineNumber": 7,
                "model": "cursor-test-model",
                "conversationId": "conv-test-1",
            }

            spillover_root = Path(tmp) / "spillover"
            env = os.environ.copy()
            env["AGENTDIFF_SPILLOVER"] = str(spillover_root)

            proc = subprocess.run(
                ["python3", str(SCRIPT_PATH)],
                input=json.dumps(payload),
                text=True,
                capture_output=True,
                env=env,
            )
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            self.assertTrue(session_log.exists(), "repo-local session log should exist")

            lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
            self.assertEqual(len(lines), 1)
            entry = json.loads(lines[0])
            self.assertEqual(entry.get("agent"), "cursor")
            self.assertEqual(entry.get("file"), "src/main.rs")

            spill_files = list(spillover_root.glob("*.jsonl"))
            self.assertEqual(spill_files, [], "event should not fall back to spillover for repo file edits")


if __name__ == "__main__":
    unittest.main()
