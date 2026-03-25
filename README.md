# agentdiff

Audit and trace autonomous AI code contributions in git repositories.

## What is agentdiff?

agentdiff tracks **who** (which AI agent) wrote **what** code in your repository. It captures the agent name, model, prompt, and line-level attribution for every AI-assisted edit — storing this metadata for audit trails, compliance, and accountability.

## Installation

```bash
# Build from source
cargo install --path ~/agentdiff

# Or build release and copy manually
cargo build --release
cp target/release/agentdiff ~/.cargo/bin/
```

## Quick Start

```bash
# Initialize tracking in a repository
cd ~/your-project
agentdiff init

# Make some AI-assisted edits, then commit

# List all captured entries
agentdiff list

# See line-by-line blame for a file
agentdiff blame src/main.rs

# View statistics
agentdiff stats

# Generate CI report
agentdiff report --format markdown
```

## Commands

| Command | Description |
|---------|-------------|
| `agentdiff init` | Initialize tracking in current repository |
| `agentdiff list` | List all captured attribution entries |
| `agentdiff blame <file>` | Show line-level attribution (like git-blame) |
| `agentdiff stats` | Show aggregate statistics by agent/file/model |
| `agentdiff report` | Generate CI report (markdown or GitHub annotations) |
| `agentdiff diff [<commit>]` | Show attribution changes in a commit |
| `agentdiff log` | Show chronological history |
| `agentdiff sync-notes` | Fetch `refs/notes/agentdiff` from origin |
| `agentdiff config` | Manage configuration |

## How It Works

1. **Hook Installation** — `agentdiff init` installs:
   - Git post-commit hook to write compact attribution notes
   - Claude Code PostToolUse hook
   - Cursor afterFileEdit hook
   - Codex notify hook
   - Windsurf repo-level hooks
   - OpenCode repo-level plugin

2. **Capture** — When you use AI tools:
   - Claude Code → PostToolUse hook fires on Edit/Write/MultiEdit
   - Cursor → afterFileEdit/afterTabFileEdit hooks fire
   - Codex → notify hook fires on turn completion
   - Windsurf → post_write_code hook fires on code write
   - OpenCode → plugin fires on tool.execute.after
   - Each capture writes to `<repo>/.git/agentdiff/session.jsonl`

3. **Commit** — On `git commit`:
   - Post-commit hook compacts buffered events
   - Writes one JSON note to `refs/notes/agentdiff` attached to `HEAD`
   - Leaves unmatched events buffered for the next commit

4. **View** — Use CLI commands to inspect captured data

## Configuration

Config stored at `~/.agentdiff/config.toml`

```toml
schema_version = "1.0"
data_dir = "~/.agentdiff/spillover" # optional spillover for no-repo captures
scripts_dir = "~/.agentdiff/scripts"

[[repos]]
path = "/home/user/project"
slug = "-home-user-project"
```

## Supported Agents

- **Claude Code** — via PostToolUse hook
- **Cursor** — via afterFileEdit/afterTabFileEdit hooks
- **Codex CLI** — via `notify` hook
- **Windsurf** — via `post_write_code` hook
- **OpenCode** — via `.opencode/plugins/agentdiff.ts`
- **Antigravity/batch agents** — via CLI (`capture-antigravity.py --prompt "..." --model "..."`)

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
│   ├── capture-antigravity.py
│   └── write-note.py
└── spillover/            ← Optional no-repo event spillover

<repo>/.git/agentdiff/
└── session.jsonl         ← Uncommitted per-repo buffer

<repo>/.windsurf/
└── hooks.json            ← Windsurf repo-level hook config

<repo>/.opencode/plugins/
└── agentdiff.ts          ← OpenCode repo-level plugin

git refs:
└── refs/notes/agentdiff  ← Canonical committed attribution notes
```

## Requirements

- Rust 1.70+
- Python 3.7+ (for capture scripts)
- Git repository

## License

MIT
