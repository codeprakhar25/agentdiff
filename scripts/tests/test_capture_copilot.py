import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "capture-copilot.py"


def load_module():
    spec = importlib.util.spec_from_file_location("capture_copilot", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class CaptureCopilotTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mod = load_module()

    # ── get_session_log ─────────────────────────────────────────────────────

    def test_get_session_log_returns_none_when_not_initialized(self):
        """No .git/agentdiff/ directory → return None (agentdiff init not run)."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
            # .git exists but .git/agentdiff/ does NOT → not initialized
            env = {**os.environ, "AGENTDIFF_SESSION_LOG": ""}
            result = self.mod.get_session_log(str(repo))
            self.assertIsNone(result, "get_session_log must return None when init not run")

    def test_get_session_log_returns_path_when_initialized(self):
        """.git/agentdiff/ exists → return the session.jsonl path."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)

            agentdiff_dir = repo / ".git" / "agentdiff"
            agentdiff_dir.mkdir(parents=True, exist_ok=True)

            # Unset override env var so module uses the directory-based check
            original = os.environ.pop("AGENTDIFF_SESSION_LOG", None)
            try:
                result = self.mod.get_session_log(str(repo))
            finally:
                if original is not None:
                    os.environ["AGENTDIFF_SESSION_LOG"] = original

            expected = agentdiff_dir / "session.jsonl"
            self.assertEqual(Path(result), expected)

    # ── end-to-end capture via subprocess ───────────────────────────────────

    def _run_capture(self, repo: Path, payload: dict, env: dict | None = None) -> subprocess.CompletedProcess:
        run_env = os.environ.copy()
        if env:
            run_env.update(env)
        return subprocess.run(
            ["python3", str(SCRIPT_PATH)],
            input=json.dumps(payload),
            text=True,
            capture_output=True,
            env=run_env,
        )

    def _make_repo_with_init(self, tmp: str) -> Path:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
        edited = repo / "main.py"
        edited.write_text("print('hello')\n", encoding="utf-8")
        agentdiff_dir = repo / ".git" / "agentdiff"
        agentdiff_dir.mkdir(parents=True, exist_ok=True)
        return repo

    def test_capture_writes_entry_with_correct_fields(self):
        """Run script with inline_heuristic payload; entry must have expected fields."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._make_repo_with_init(tmp)
            edited = repo / "main.py"

            payload = {
                "event": "inline",
                "cwd": str(repo),
                "file_path": str(edited),
                "model": "copilot-gpt-4o",
                "session_id": "vscode-111-abc",
                "prompt": None,
                "lines": [1, 2, 3],
                "confidence": "low",
                "capture_mode": "inline_heuristic",
            }

            proc = self._run_capture(repo, payload)
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            self.assertTrue(session_log.exists(), "session.jsonl should be created")

            lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
            self.assertEqual(len(lines), 1)
            entry = json.loads(lines[0])

            self.assertEqual(entry["agent"], "copilot")
            self.assertEqual(entry["tool"], "copilot-inline")
            self.assertEqual(entry["confidence"], "low")
            self.assertEqual(entry["capture_mode"], "inline_heuristic")
            self.assertEqual(entry["file"], "main.py")
            self.assertEqual(entry["lines"], [1, 2, 3])
            self.assertEqual(entry["session_id"], "vscode-111-abc")
            self.assertIn("timestamp", entry)

    def test_capture_with_high_confidence_manual(self):
        """manual event → tool=copilot-manual, confidence=high."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._make_repo_with_init(tmp)
            edited = repo / "main.py"

            payload = {
                "event": "manual",
                "cwd": str(repo),
                "file_path": str(edited),
                "model": "copilot-gpt-4o",
                "session_id": "vscode-222-xyz",
                "prompt": None,
                "lines": list(range(1, 11)),
                "confidence": "high",
                "capture_mode": "manual",
            }

            proc = self._run_capture(repo, payload)
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
            self.assertEqual(len(lines), 1)
            entry = json.loads(lines[0])

            self.assertEqual(entry["agent"], "copilot")
            self.assertEqual(entry["tool"], "copilot-manual")
            self.assertEqual(entry["confidence"], "high")
            self.assertEqual(entry["capture_mode"], "manual")

    def test_capture_skips_when_not_initialized(self):
        """No .git/agentdiff/ → no session.jsonl is created (exit 0, silent)."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
            edited = repo / "main.py"
            edited.write_text("x = 1\n", encoding="utf-8")
            # Do NOT create .git/agentdiff/

            payload = {
                "event": "inline",
                "cwd": str(repo),
                "file_path": str(edited),
                "model": "copilot-gpt-4o",
                "session_id": "vscode-333",
                "lines": [1],
                "confidence": "low",
                "capture_mode": "inline_heuristic",
            }

            # Remove env override so the script uses directory-based check
            env = {k: v for k, v in os.environ.items() if k != "AGENTDIFF_SESSION_LOG"}
            proc = subprocess.run(
                ["python3", str(SCRIPT_PATH)],
                input=json.dumps(payload),
                text=True,
                capture_output=True,
                env=env,
            )
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            self.assertFalse(
                session_log.exists(),
                "session.jsonl must not be created when agentdiff init has not been run",
            )

    def test_capture_missing_file_path_exits_silently(self):
        """Empty payload → exit 0, no file created."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._make_repo_with_init(tmp)

            proc = self._run_capture(repo, {})
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            self.assertFalse(session_log.exists())

    def test_tool_mapping(self):
        """Verify all event → tool name mappings."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._make_repo_with_init(tmp)
            edited = repo / "main.py"

            mappings = [
                ("inline",    "copilot-inline"),
                ("save",      "copilot-save"),
                ("chat_edit", "copilot-chat"),
                ("manual",    "copilot-manual"),
            ]

            for event_name, expected_tool in mappings:
                session_log = repo / ".git" / "agentdiff" / "session.jsonl"
                # Clear between runs
                if session_log.exists():
                    session_log.unlink()

                payload = {
                    "event": event_name,
                    "cwd": str(repo),
                    "file_path": str(edited),
                    "model": "copilot",
                    "session_id": "s",
                    "lines": [1],
                    "confidence": "high" if event_name == "manual" else "low",
                    "capture_mode": event_name if event_name == "manual" else "inline_heuristic",
                }

                proc = self._run_capture(repo, payload)
                self.assertEqual(proc.returncode, 0, msg=f"event={event_name}: {proc.stderr}")

                self.assertTrue(session_log.exists(), f"event={event_name}: session.jsonl not created")
                lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
                entry = json.loads(lines[-1])
                self.assertEqual(
                    entry["tool"], expected_tool,
                    f"event={event_name!r}: expected tool={expected_tool!r}, got {entry['tool']!r}",
                )

    def test_relative_path_within_repo(self):
        """abs_file inside repo → file field is repo-relative."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._make_repo_with_init(tmp)
            src = repo / "src"
            src.mkdir()
            edited = src / "app.py"
            edited.write_text("# app\n", encoding="utf-8")

            payload = {
                "event": "manual",
                "cwd": str(repo),
                "file_path": str(edited),
                "model": "copilot",
                "session_id": "s",
                "lines": [1],
                "confidence": "high",
                "capture_mode": "manual",
            }

            proc = self._run_capture(repo, payload)
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
            entry = json.loads(lines[0])

            # Must be repo-relative, not absolute
            self.assertEqual(entry["file"], "src/app.py")
            self.assertFalse(entry["file"].startswith("/"), "file field must be repo-relative")

    def test_save_flush_capture_mode(self):
        """save event → tool=copilot-save, confidence=low, capture_mode=save_flush."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._make_repo_with_init(tmp)
            edited = repo / "main.py"

            payload = {
                "event": "save",
                "cwd": str(repo),
                "file_path": str(edited),
                "model": "copilot",
                "session_id": "s",
                "lines": [5, 6, 7],
                "confidence": "low",
                "capture_mode": "save_flush",
            }

            proc = self._run_capture(repo, payload)
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
            entry = json.loads(lines[0])

            self.assertEqual(entry["tool"], "copilot-save")
            self.assertEqual(entry["confidence"], "low")
            self.assertEqual(entry["capture_mode"], "save_flush")

    def test_confidence_defaults_to_low_when_absent(self):
        """Payload without confidence/capture_mode fields → defaults to low/inline_heuristic."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = self._make_repo_with_init(tmp)
            edited = repo / "main.py"

            # Omit confidence and capture_mode to simulate older extension versions
            payload = {
                "event": "inline",
                "cwd": str(repo),
                "file_path": str(edited),
                "model": "copilot",
                "session_id": "old-ext-session",
                "lines": [1],
            }

            proc = self._run_capture(repo, payload)
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            session_log = repo / ".git" / "agentdiff" / "session.jsonl"
            lines = [ln for ln in session_log.read_text(encoding="utf-8").splitlines() if ln.strip()]
            entry = json.loads(lines[0])

            self.assertEqual(entry["confidence"], "low")
            self.assertEqual(entry["capture_mode"], "inline_heuristic")


if __name__ == "__main__":
    unittest.main()
