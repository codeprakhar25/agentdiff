import json
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
RECORD_CONTEXT = REPO_ROOT / "scripts" / "record-context.py"


class RecordContextTests(unittest.TestCase):
    def test_cli_flags_do_not_block_without_stdin_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init", "-b", "main"], cwd=repo, check=True, capture_output=True)

            result = subprocess.run(
                [
                    "python3",
                    str(RECORD_CONTEXT),
                    "--cwd",
                    str(repo),
                    "--agent",
                    "cursor",
                    "--model-id",
                    "validation-model",
                    "--files-read",
                    '["src/app.py"]',
                    "--intent",
                    "context validation",
                ],
                text=True,
                capture_output=True,
                timeout=2,
            )

            self.assertEqual(result.returncode, 0)
            pending = json.loads((repo / ".git" / "agentdiff" / "pending.json").read_text())
            self.assertEqual(pending["agent"], "cursor")
            self.assertEqual(pending["model_id"], "validation-model")
            self.assertEqual(pending["files_read"], ["src/app.py"])
            self.assertEqual(pending["intent"], "context validation")


if __name__ == "__main__":
    unittest.main()
