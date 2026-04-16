"""
Tests for the capture_prompts_enabled() / _capture_prompts_enabled() config gate.

Covers both capture-claude.py (capture side) and finalize-ledger.py (trace side).
"""
import importlib.util
import os
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CAPTURE_CLAUDE = REPO_ROOT / "scripts" / "capture-claude.py"
FINALIZE_LEDGER = REPO_ROOT / "scripts" / "finalize-ledger.py"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(mod)
    return mod


class CaptureCapturePromptsEnabledTests(unittest.TestCase):
    """Tests for capture_prompts_enabled() in capture-claude.py."""

    @classmethod
    def setUpClass(cls):
        cls.mod = load_module(CAPTURE_CLAUDE, "capture_claude")

    def _write_config(self, tmp_dir: str, content: str) -> str:
        path = os.path.join(tmp_dir, "config.toml")
        with open(path, "w") as f:
            f.write(content)
        return path

    def test_returns_true_when_config_missing(self):
        original = os.environ.get("HOME")
        try:
            with tempfile.TemporaryDirectory() as tmp:
                # Point HOME to an empty directory with no config
                os.environ["HOME"] = tmp
                result = self.mod.capture_prompts_enabled()
                self.assertTrue(result, "Should default to True when config is absent")
        finally:
            if original is not None:
                os.environ["HOME"] = original

    def test_returns_false_when_explicitly_disabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            agentdiff_dir = os.path.join(tmp, ".agentdiff")
            os.makedirs(agentdiff_dir)
            config_path = os.path.join(agentdiff_dir, "config.toml")
            with open(config_path, "w") as f:
                f.write('schema_version = "1.0"\ncapture_prompts = false\n')
            original = os.environ.get("HOME")
            try:
                os.environ["HOME"] = tmp
                result = self.mod.capture_prompts_enabled()
                self.assertFalse(result, "Should return False when capture_prompts = false")
            finally:
                if original is not None:
                    os.environ["HOME"] = original

    def test_returns_true_when_explicitly_enabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            agentdiff_dir = os.path.join(tmp, ".agentdiff")
            os.makedirs(agentdiff_dir)
            config_path = os.path.join(agentdiff_dir, "config.toml")
            with open(config_path, "w") as f:
                f.write('schema_version = "1.0"\ncapture_prompts = true\n')
            original = os.environ.get("HOME")
            try:
                os.environ["HOME"] = tmp
                result = self.mod.capture_prompts_enabled()
                self.assertTrue(result, "Should return True when capture_prompts = true")
            finally:
                if original is not None:
                    os.environ["HOME"] = original

    def test_accepts_off_as_false(self):
        with tempfile.TemporaryDirectory() as tmp:
            agentdiff_dir = os.path.join(tmp, ".agentdiff")
            os.makedirs(agentdiff_dir)
            config_path = os.path.join(agentdiff_dir, "config.toml")
            with open(config_path, "w") as f:
                f.write("capture_prompts = off\n")
            original = os.environ.get("HOME")
            try:
                os.environ["HOME"] = tmp
                result = self.mod.capture_prompts_enabled()
                self.assertFalse(result, "Should treat 'off' as disabled")
            finally:
                if original is not None:
                    os.environ["HOME"] = original

    def test_inline_comment_does_not_break_false_detection(self):
        """Regression: capture_prompts = false # comment was previously misread as enabled."""
        with tempfile.TemporaryDirectory() as tmp:
            agentdiff_dir = os.path.join(tmp, ".agentdiff")
            os.makedirs(agentdiff_dir)
            config_path = os.path.join(agentdiff_dir, "config.toml")
            with open(config_path, "w") as f:
                f.write("capture_prompts = false  # prod default\n")
            original = os.environ.get("HOME")
            try:
                os.environ["HOME"] = tmp
                result = self.mod.capture_prompts_enabled()
                self.assertFalse(result, "Inline comment must not prevent false detection")
            finally:
                if original is not None:
                    os.environ["HOME"] = original


class FinalizeCapturePropmptsEnabledTests(unittest.TestCase):
    """Tests for _capture_prompts_enabled() in finalize-ledger.py."""

    @classmethod
    def setUpClass(cls):
        cls.mod = load_module(FINALIZE_LEDGER, "finalize_ledger")

    def test_returns_true_when_config_missing(self):
        original = os.environ.get("HOME")
        try:
            with tempfile.TemporaryDirectory() as tmp:
                os.environ["HOME"] = tmp
                result = self.mod._capture_prompts_enabled()
                self.assertTrue(result)
        finally:
            if original is not None:
                os.environ["HOME"] = original

    def test_returns_false_when_disabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            agentdiff_dir = os.path.join(tmp, ".agentdiff")
            os.makedirs(agentdiff_dir)
            with open(os.path.join(agentdiff_dir, "config.toml"), "w") as f:
                f.write("capture_prompts = false\n")
            original = os.environ.get("HOME")
            try:
                os.environ["HOME"] = tmp
                result = self.mod._capture_prompts_enabled()
                self.assertFalse(result)
            finally:
                if original is not None:
                    os.environ["HOME"] = original

    def test_returns_true_when_enabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            agentdiff_dir = os.path.join(tmp, ".agentdiff")
            os.makedirs(agentdiff_dir)
            with open(os.path.join(agentdiff_dir, "config.toml"), "w") as f:
                f.write("capture_prompts = true\n")
            original = os.environ.get("HOME")
            try:
                os.environ["HOME"] = tmp
                result = self.mod._capture_prompts_enabled()
                self.assertTrue(result)
            finally:
                if original is not None:
                    os.environ["HOME"] = original


if __name__ == "__main__":
    unittest.main()
