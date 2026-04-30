# Contributing to agentdiff

Thanks for your interest in contributing. This guide covers bug reports, development setup, adding agent support, and the PR process.

## Quick links

- [Report a bug](#reporting-bugs)
- [Development setup](#development-setup)
- [Add a new agent](#adding-a-new-agent)
- [PR process](#pull-request-process)

---

## Reporting Bugs

Open an issue at [github.com/codeprakhar25/agentdiff/issues](https://github.com/codeprakhar25/agentdiff/issues).

Include:
- `agentdiff --version`
- OS, Rust version (`rustc --version`), Python version (`python3 --version`)
- Which agent you were using
- Steps to reproduce
- What you expected vs. what happened
- Debug logs (see below)

**Capture debug logs:**
```bash
export AGENTDIFF_DEBUG=1
# reproduce the issue (make an AI edit, commit)
cat ~/.agentdiff/logs/capture-<agent>.log
cat .git/agentdiff/session.jsonl
```

---

## Development Setup

**Prerequisites:** Rust 1.85+, Python 3.7+, Git 2.20+

```bash
git clone https://github.com/codeprakhar25/agentdiff.git
cd agentdiff
cargo build
```

### Running tests

```bash
# Rust tests
cargo test

# Python unit tests (capture script logic)
python3 -m unittest discover -s scripts/tests -v

# MCP server smoke test
python3 scripts/mcp-smoke-test.py

# Full end-to-end test (all 7 agents, simulated payloads)
bash scripts/e2e-test.sh
```

### Installing locally for manual testing

```bash
cargo build --release
mkdir -p ~/.local/bin
install -m 0755 target/release/agentdiff ~/.local/bin/agentdiff
install -m 0755 target/release/agentdiff-mcp ~/.local/bin/agentdiff-mcp

# Run configure + init in a throwaway repo
agentdiff configure
mkdir /tmp/test-repo && cd /tmp/test-repo
git init && git config user.email "test@test.com" && git config user.name "Test"
agentdiff init
```

---

## Project Structure

```
src/
├── main.rs              ← CLI entry point and command routing
├── cli.rs               ← Argument definitions (clap)
├── init.rs              ← agentdiff configure + agentdiff init logic
├── config.rs            ← Global config (load, save, paths)
├── store.rs             ← Ledger + session reading
├── data.rs              ← Data structures (Entry, LedgerRecord)
├── util.rs              ← Shared helpers
├── bin/
│   └── agentdiff-mcp.rs ← MCP stdio server
└── commands/            ← One file per CLI command

scripts/
├── capture-<agent>.py   ← Agent-specific capture scripts (stdin JSON → session.jsonl)
├── prepare-ledger.py    ← Pre-commit: match staged diff to captured events
├── finalize-ledger.py   ← Post-commit: write ledger entry
├── record-context.py    ← MCP record_context tool handler
├── opencode-agentdiff.ts ← OpenCode TypeScript plugin template
├── vscode-extension/    ← Copilot VS Code extension (plain JS)
└── tests/               ← Python unit tests
```

---

## Adding a New Agent

Supporting a new AI agent means two things: a capture script and a hook installation step.

### 1. Write the capture script

Create `scripts/capture-<agentname>.py`. The script:
- Reads a JSON payload from **stdin**
- Resolves the repo root with `git rev-parse --show-toplevel`
- Writes one JSON entry to `<repo>/.git/agentdiff/session.jsonl`

Minimum entry schema:
```json
{
  "timestamp": "2026-03-28T10:00:00Z",
  "agent": "my-agent",
  "model": "model-name-or-unknown",
  "session_id": "session-id-or-unknown",
  "tool": "Edit",
  "file": "src/main.rs",
  "abs_file": "/home/user/project/src/main.rs",
  "lines": [10, 11, 12],
  "prompt": "prompt text or null"
}
```

Look at `scripts/capture-claude.py` as the reference implementation — it handles path normalization, repo resolution, and debug logging.

**Debug logging** — use the shared pattern:
```python
import os, sys

def debug_log(msg):
    if os.environ.get("AGENTDIFF_DEBUG"):
        log_dir = Path.home() / ".agentdiff" / "logs"
        log_dir.mkdir(parents=True, exist_ok=True)
        with open(log_dir / "capture-myagent.log", "a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {msg}\n")
```

### 2. Embed the script in the binary

In `src/init.rs`, add:
```rust
const MYAGENT_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-myagent.py");
```

Add it to `step_install_scripts()`:
```rust
("capture-myagent.py", MYAGENT_CAPTURE_SCRIPT),
```

### 3. Add the hook installation step

Add a `step_configure_myagent()` function that writes to the agent's global config file (home directory preferred over per-repo). Call it from `run_configure()`.

### 4. Add CLI flags

In `src/cli.rs`, add to `ConfigureArgs`:
```rust
/// Skip MyAgent hook setup
#[arg(long)]
pub no_myagent: bool,
```

Wire it through `main.rs` and `run_configure()`.

### 5. Add tests

Add a Python unit test in `scripts/tests/test_capture_myagent.py` that:
- Pipes a minimal valid JSON payload to the script
- Asserts the resulting `session.jsonl` entry has the correct `agent`, `tool`, and `file` fields

Add an entry to the `e2e-test.sh` script simulating a real payload.

---

## Code Conventions

### Rust

- Follow existing patterns — match the style of adjacent code
- Use `anyhow::Result` for error propagation; add `.context("...")` on file I/O
- Prefer `dirs::home_dir()` over hardcoding `~`
- All user-visible output uses `colored` crate: `"ok".green()`, `"!".yellow()`, `"--".dimmed()`
- No `unwrap()` in non-test code except after `dirs::home_dir()` (it won't fail on supported platforms)

### Python (capture scripts)

- Target Python 3.7+ — no f-string `=` syntax, no `match` statements
- Read from `sys.stdin`; resolve paths with `pathlib.Path`
- Write to `session.jsonl` with a file lock (`fcntl.flock` / `msvcrt.locking`)
- Fail silently on errors that shouldn't break git workflows — log to debug file, `sys.exit(0)`
- Always resolve repo root with `git rev-parse --show-toplevel` from the file's directory

### Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add MyAgent capture support
fix: handle missing cwd in windsurf payload
docs: add CI integration example
chore: bump serde to 1.0.200
```

---

## Pull Request Process

1. **Fork** the repo and branch from `main`:
   ```bash
   git checkout -b feat/my-agent-support
   ```

2. **Make focused changes** — one feature or fix per PR.

3. **Build and test:**
   ```bash
   cargo build
   cargo test
   python3 -m unittest discover -s scripts/tests
   python3 scripts/mcp-smoke-test.py
   bash scripts/e2e-test.sh
   ```

4. **Open a PR** with:
   - What changed and why
   - How to test it
   - `Closes #<issue>` if applicable

5. **Address review feedback** — push updates and reply to comments.

### CI checks

All PRs must pass:
- `cargo build --locked` — no compile errors
- `cargo test --locked` — all Rust tests green
- `python3 -m unittest discover -s scripts/tests` — all Python tests green
- `python3 scripts/mcp-smoke-test.py` — MCP server smoke test

---

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE-MIT) and [Apache-2.0 License](LICENSE-APACHE).
