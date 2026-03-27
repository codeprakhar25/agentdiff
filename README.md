# agentdiff

Audit and trace autonomous AI code contributions in git repositories.

## What is agentdiff?

agentdiff tracks **who** (which AI agent) wrote **what** code in your repository. It captures the agent name, model, prompt, and line-level attribution for every AI-assisted edit — storing this metadata for audit trails, compliance, and accountability.

## Installation

```bash
# Install latest release (recommended)
curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/main/install.sh | bash

# Install a specific version
curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/main/install.sh | bash -s -- --version v0.1.0

# Install from crate source
cargo install --path ~/agentdiff

# Build release and copy manually
cargo build --release
cp target/release/agentdiff ~/.cargo/bin/
cp target/release/agentdiff-mcp ~/.cargo/bin/
```

## Quick Start

```bash
# Initialize tracking in a repository
cd ~/your-project
agentdiff init

# Start MCP server (stdio) for agent integrations
agentdiff-mcp

# Make some AI-assisted edits, then commit

# List all captured entries
agentdiff list

# Show one commit's full ledger details
agentdiff show <sha>

# See line-by-line blame for a file
agentdiff blame src/main.rs

# View statistics
agentdiff stats

# Generate CI report (stdout, or --out-md / --out-annotations for files)
agentdiff report --format markdown
agentdiff report --format annotations --out-annotations annotations.json
# optional filters: --since RFC3339 --agent substring --model substring
```

## Commands

| Command | Description |
|---------|-------------|
| `agentdiff init` | Initialize tracking in current repository |
| `agentdiff list` | List all captured attribution entries |
| `agentdiff blame <file>` | Show line-level attribution (like git-blame) |
| `agentdiff stats` | Show aggregate statistics by agent/file/model |
| `agentdiff report` | CI report: markdown and/or GitHub-style annotations (`--out-md`, `--out-annotations`, `--agent`, `--model`, `--since`) |
| `agentdiff diff [<commit>]` | Show attribution changes in a commit |
| `agentdiff log` | Show chronological history |
| `agentdiff show <sha>` | Show one ledger entry by commit SHA |
| `agentdiff ledger repair` | Normalize and de-duplicate `.agentdiff/ledger.jsonl` |
| `agentdiff ledger import-notes` | Import legacy `refs/notes/agentdiff` into ledger |
| `agentdiff sync-notes` | Legacy: fetch `refs/notes/agentdiff` for migration |
| `agentdiff config` | Manage configuration |

## How It Works

1. **Hook Installation** — `agentdiff init` installs:
   - Git `pre-commit` hook to snapshot staged AI attribution context
   - Git `post-commit` hook to append one commit-scoped ledger line
   - Claude Code PostToolUse hook
   - Cursor afterFileEdit hook
   - Codex notify hook
   - Gemini/Antigravity BeforeTool/AfterTool hooks
   - Windsurf repo-level hooks
   - OpenCode repo-level plugin
   - VS Code Copilot extension (`~/.vscode/extensions/agentdiff-copilot-0.1.0/`)

2. **Capture** — When you use AI tools:
   - Claude Code → PostToolUse hook fires on Edit/Write/MultiEdit
   - Cursor → afterFileEdit/afterTabFileEdit hooks fire
   - Codex → notify hook fires on turn completion
   - Gemini/Antigravity → BeforeTool/AfterTool hooks fire on write_file/replace
   - Windsurf → post_write_code hook fires on code write
   - OpenCode → plugin fires on tool.execute.after
   - GitHub Copilot → VS Code extension captures inline completions and chat edits on save
   - Each capture writes to `<repo>/.git/agentdiff/session.jsonl`
   - Optional MCP writes context to `<repo>/.git/agentdiff/pending.json`

3. **Commit** — On `git commit`:
   - Pre-commit hook writes `<repo>/.git/agentdiff/pending-ledger.json`
   - Post-commit hook finalizes one line into `<repo>/.agentdiff/ledger.jsonl`
   - By default, post-commit auto-amends the commit to include ledger changes
   - Pending files are cleared after finalize

4. **View** — Use CLI commands to inspect captured data

## Configuration

Config stored at `~/.agentdiff/config.toml`

```toml
schema_version = "1.0"
data_dir = "~/.agentdiff/spillover" # optional spillover for no-repo captures
scripts_dir = "~/.agentdiff/scripts"
auto_amend_ledger = true

[[repos]]
path = "/home/user/project"
slug = "-home-user-project"
```

Disable same-commit amend behavior:
```bash
agentdiff config set auto_amend_ledger false
agentdiff init
```

## Supported Agents

- **Claude Code** — via PostToolUse hook
- **Cursor** — via afterFileEdit/afterTabFileEdit hooks
- **Codex CLI** — via `notify` hook
- **Gemini/Antigravity** — via `~/.gemini/settings.json` hooks (`write_file|replace`)
- **Windsurf** — via `post_write_code` hook
- **OpenCode** — via `.opencode/plugins/agentdiff.ts`
- **GitHub Copilot (VS Code)** — via VS Code extension installed to `~/.vscode/extensions/`
- **MCP context writer** — via `record-context.py` (writes `.git/agentdiff/pending.json`)
- **Manual batch fallback** — `capture-antigravity.py --prompt "..." --model "..."`

## MCP Server

`agentdiff-mcp` is a real MCP stdio server.  
Supported methods:
- `initialize`
- `tools/list`
- `tools/call` for tool `record_context`

`record_context` writes context to `<repo>/.git/agentdiff/pending.json`, which is consumed by `agentdiff` pre/post commit hooks and attached to the next ledger line.

Example `record_context` arguments:
```json
{
  "cwd": "/path/to/repo",
  "agent": "codex",
  "model_id": "gpt-5.4",
  "session_id": "sess_abc123",
  "prompt": "add auth middleware",
  "files_read": ["src/auth.rs", "src/config.rs"],
  "intent": "auth hardening",
  "trust": 92,
  "flags": ["security"]
}
```

### MCP Client Config

Minimal MCP stdio config is provided at:
- `examples/mcp/agentdiff-mcp.json`

Generic server block:
```json
{
  "mcpServers": {
    "agentdiff": {
      "command": "agentdiff-mcp",
      "args": [],
      "env": {}
    }
  }
}
```

Use this same command in Claude/Cursor/Codex/OpenCode MCP settings.

## End-to-End Testing

### 1) Reinstall latest binaries and scripts
```bash
cargo install --path /home/prakh/agentdiff --force
cd /path/to/your/repo
agentdiff init
```

### 2) MCP flow test (commit should include prompt/model/session)
```bash
# In repo root
agentdiff-mcp
# In another terminal, send tools/call record_context via your MCP client
# then make a small edit + git add + git commit

agentdiff list
agentdiff show HEAD
```

### 3) Hook-only fallback test (no MCP)
```bash
# Make an AI edit from Cursor/Codex/Windsurf/Claude, then commit
agentdiff list
agentdiff show HEAD
```

### 4) Cursor diagnostics (if entry is missing)
```bash
export AGENTDIFF_DEBUG=1
# make one Cursor edit + commit
tail -n 100 ~/.agentdiff/logs/capture-cursor.log
tail -n 50 .git/agentdiff/session.jsonl
tail -n 20 .agentdiff/ledger.jsonl
```

### 4b) Codex / Gemini diagnostics (if entry is missing)
```bash
export AGENTDIFF_DEBUG=1
# make one edit in Codex or Gemini, then commit
tail -n 100 ~/.agentdiff/logs/codex-notify-fired.log
tail -n 100 ~/.agentdiff/logs/capture-codex.log
tail -n 50 .git/agentdiff/session.jsonl
tail -n 20 .agentdiff/ledger.jsonl
```

### 5) Edge cases to validate
- Empty or malformed hook payloads should not crash capture scripts.
- `afterTabFileEdit` events should still attribute file/line/model without prompt.
- Events with Windows-style backslash paths should resolve to repo-local paths.
- If hook `cwd` is not repo (for example `~/.cursor`), file-path-based repo resolution should still write to `<repo>/.git/agentdiff/session.jsonl`.
- Commits with no staged overlap against captured lines should fall back to `human`.
- Duplicate finalize on same SHA should not append duplicate ledger rows.

## Release Process

Tagging `v*` triggers release workflow:
- Runs Rust + Python tests and MCP smoke test
- Builds `agentdiff` and `agentdiff-mcp` artifacts for Linux/macOS/Windows
- Publishes `SHA256SUMS`
- Creates GitHub Release with artifacts
- Optionally publishes crate to crates.io when `CARGO_REGISTRY_TOKEN` is set

## Output Formats

### List Output
```
  # — COMMIT     TIME         AGENT         MODEL                 TOOL      FILE                          LINES    PROMPT
  ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  1  a1b2c3d4   Mar 20 17:52 claude-code   sonnet-4-6           Edit      src/auth/tokens.py             17-24   "add auth middleware"
  2  a1b2c3d4   Mar 20 17:55 cursor        gpt-4o               afterFileEdit src/main.rs             1       "add main entry"
  * a1b2c3e5   Mar 20 18:02 claude-code   sonnet-4-6           Write     src/utils.rs                  1-94    "refactor utils"  (uncommitted)
```

### Blame Output
```
  agentdiff blame — src/main.rs
  ───────────────────────────────────────────────────────────────────────────────────────────────────
     1 human         fn main() {
     2 human         use anyhow::Result;
     3 claude-code   use std::process::Command;  (Edit)
     4 human
     5 cursor        fn run() -> Result<()> {   (afterFileEdit)
```

### Stats Output
```
  agentdiff — Statistics

  Total lines tracked: 2847

  By Agent:
    claude-code   1847 (65%) ████████████████████
    cursor         647 (23%) ████████
    human         353 (12%) ████

  By File:
    src/main.rs                    340 lines (78% AI) — claude-code
    src/auth/mod.rs                220 lines (65% AI) — cursor
```

## Use Cases

- **Security audits** — Know which AI generated risky code
- **Compliance** — SOC2, ISO 27001 require knowing what generated production code
- **Code review** — Identify AI-authored code requiring extra scrutiny
- **Attribution** — Track prompts driving major changes

## VS Code Copilot Extension

`agentdiff init` installs a plain-JavaScript VS Code extension directly into
`~/.vscode/extensions/agentdiff-copilot-0.1.0/`. No `npm install` or build step
is needed. **Restart VS Code** after running `agentdiff init` to activate it.

The extension:
- Detects GitHub Copilot inline completions (multi-character/multi-line insertions)
- Flushes captured lines to `session.jsonl` on file save
- Exposes command **"agentdiff: Capture Copilot edits for current file"** for manual capture

To skip Copilot setup: `agentdiff init --no-copilot`
To skip Gemini/Antigravity setup: `agentdiff init --no-antigravity`

> **Note:** The extension uses a heuristic — insertions of ≥10 characters or multiple
> lines while Copilot is active are attributed to Copilot. Short single-character
> insertions are treated as manual typing.

## Architecture

```
~/.agentdiff/
├── config.toml           ← Global configuration
├── scripts/              ← Python capture scripts
│   ├── capture-claude.py
│   ├── capture-cursor.py
│   ├── capture-codex.py
│   ├── capture-windsurf.py
│   ├── capture-opencode.py
│   ├── capture-copilot.py
│   ├── prepare-ledger.py
│   ├── finalize-ledger.py
│   ├── record-context.py
│   ├── capture-antigravity.py
│   └── write-note.py      ← legacy notes writer (migration only)
└── spillover/            ← Optional no-repo event spillover

~/.vscode/extensions/
└── agentdiff-copilot-0.1.0/
    ├── package.json      ← VS Code extension manifest
    └── extension.js      ← Copilot capture logic (plain JS, no build needed)

<repo>/.agentdiff/
└── ledger.jsonl          ← Canonical committed append-only attribution log

<repo>/.git/agentdiff/
├── session.jsonl         ← Uncommitted per-repo buffer from hooks/agents
├── pending.json          ← Ephemeral MCP context handoff
└── pending-ledger.json   ← Pre-commit snapshot finalized in post-commit

<repo>/.windsurf/
└── hooks.json            ← Windsurf repo-level hook config

<repo>/.opencode/plugins/
└── agentdiff.ts          ← OpenCode repo-level plugin

git refs (legacy migration source):
└── refs/notes/agentdiff
```

## Requirements

- Rust 1.85+
- Python 3.7+ (for capture scripts)
- Git repository

## License

MIT
