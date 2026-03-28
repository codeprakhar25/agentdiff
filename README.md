# agentdiff — Know What Every AI Agent Wrote

<p align="center">
  <strong>Line-level attribution for AI-assisted code. Audit every agent, model, and prompt across your entire git history.</strong>
</p>

<p align="center">
  <a href="https://github.com/codeprakhar25/agentdiff/releases"><img src="https://img.shields.io/github/v/release/codeprakhar25/agentdiff?style=flat-square" alt="Latest release"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue?style=flat-square" alt="License"></a>
  <a href="https://github.com/codeprakhar25/agentdiff/actions"><img src="https://img.shields.io/github/actions/workflow/status/codeprakhar25/agentdiff/ci.yml?style=flat-square&label=CI" alt="CI"></a>
  <img src="https://img.shields.io/badge/agents-7%2B-blueviolet?style=flat-square" alt="Agents supported">
  <img src="https://img.shields.io/badge/built_with-Rust-orange?style=flat-square" alt="Built with Rust">
</p>

---

agentdiff hooks into every major AI coding agent — Claude Code, Cursor, Codex, Copilot, Windsurf, OpenCode, Gemini — and writes a permanent, commit-scoped attribution record to your repository. Each record captures the agent name, model, prompt excerpt, and exact line ranges. All of it queryable from the CLI, no server required.

```
agentdiff stats

  Total lines tracked: 4,231

  By Agent:
    claude-code   2,741 (65%) ████████████████████
    cursor          973 (23%) ███████
    copilot         353  (8%) ███
    human           164  (4%) █
```

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/master/install.sh | bash
```

<details>
<summary>Other install methods</summary>

```bash
# Specific version
curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/master/install.sh | bash -s -- --version v0.1.0

# From source (requires Rust 1.85+)
cargo install --git https://github.com/codeprakhar25/agentdiff agentdiff
```

**Requirements:** Python 3.7+ on PATH, Git 2.20+

</details>

---

## Quick Start

```bash
# 1. Configure global agent hooks — run once per machine
agentdiff configure

# 2. Initialize a repository
cd ~/your-project
agentdiff init

# 3. Work normally — make AI-assisted edits, then commit
git add . && git commit -m "feat: add feature"

# 4. Inspect attribution
agentdiff list
agentdiff blame src/main.rs
agentdiff stats
```

That's it. From here every commit is attributed to whichever agent (or human) wrote it.

---

## Commands

| Command | Description |
|---------|-------------|
| `agentdiff configure` | Install global agent hooks — run once per machine |
| `agentdiff init` | Initialize tracking in current repository |
| `agentdiff list` | List attribution entries |
| `agentdiff blame <file>` | Line-level attribution, like `git blame` |
| `agentdiff stats` | Aggregate stats by agent, model, file |
| `agentdiff log` | Chronological AI contribution history |
| `agentdiff diff [<sha>]` | Attribution diff for a commit or range |
| `agentdiff show <sha>` | Full details for one commit |
| `agentdiff report` | CI report in Markdown or GitHub annotations |
| `agentdiff config` | Manage global configuration |

<details>
<summary>Command flags and examples</summary>

```bash
# Filter list by agent or file
agentdiff list --agent cursor --file src/auth
agentdiff list --limit 50

# Blame for a specific agent only
agentdiff blame src/api.rs --agent claude-code

# Stats broken down by file and model
agentdiff stats --by-file --by-model

# Stats from a specific date
agentdiff stats --since 2026-01-01T00:00:00Z

# CI report to file
agentdiff report --format markdown --out-md report.md
agentdiff report --format annotations --out-annotations annotations.json

# Attribution diff for last 3 commits
agentdiff diff HEAD~3

# Skip specific agents during configure
agentdiff configure --no-copilot --no-antigravity

# Skip git hook install during init
agentdiff init --no-git-hook
```

</details>

---

## Supported Agents

| Agent | Hook mechanism | Captures |
|-------|---------------|----------|
| **Claude Code** | `PostToolUse` hook (`~/.claude/settings.json`) | Edit, Write, MultiEdit |
| **Cursor** | `afterFileEdit`, `afterTabFileEdit` hooks | Agent edits + Tab completions |
| **GitHub Copilot** | VS Code extension (`~/.vscode/extensions/`) | Inline completions, chat edits |
| **Windsurf** | `post_write_code` hook (`~/.codeium/windsurf/hooks.json`) | Cascade agent writes |
| **OpenCode** | `tool.execute.after` plugin (`~/.config/opencode/plugins/`) | All tool writes |
| **Codex CLI** | `notify` hook (`~/.codex/config.toml`) | Task-level file changes |
| **Gemini / Antigravity** | `BeforeTool`/`AfterTool` hooks (`~/.gemini/settings.json`) | `write_file`, `replace` |

Agent hooks for Claude, Cursor, Codex, Windsurf, OpenCode, and Gemini are all installed **globally once** via `agentdiff configure` — no per-repo setup needed for those.

---

## Example Output

<details>
<summary>agentdiff list</summary>

```
  agentdiff list — 5 entries

  #   COMMIT     TIME          AGENT         MODEL           FILES  LINES   PROMPT
  ──────────────────────────────────────────────────────────────────────────────────────────────────
  1   a1b2c3d4   Mar 20 17:52  claude-code   sonnet-4-6      1      17-24   "add auth middleware"
  2   b2c3d4e5   Mar 20 18:10  cursor        cursor-fast     2      1, 44   "refactor utils module"
  3   c3d4e5f6   Mar 20 18:45  copilot       gpt-4o          1      10-12   —
  4   d4e5f6a7   Mar 20 19:01  codex         o4-mini         3      1-89    "migrate to new API"
  *   (pending)  Mar 20 19:14  claude-code   sonnet-4-6      1      5-31    "add tests"  (uncommitted)
```

</details>

<details>
<summary>agentdiff blame src/main.rs</summary>

```
  agentdiff blame — src/main.rs

     1  human         fn main() {
     2  human             let cli = Cli::parse();
     3  claude-code       let config = Config::load()?;  (Edit)
     4  claude-code       let store = Store::new(repo_root, config);  (Edit)
     5  human
     6  cursor            match cli.command {  (afterFileEdit)
     7  cursor                Command::Init(args) => init::run_init(&repo_root, &mut cfg),  (afterFileEdit)
     8  human             }
     9  human         }
```

</details>

<details>
<summary>agentdiff report (Markdown)</summary>

```markdown
## AI Attribution Report

**Total lines tracked:** 4,231 across 47 commits

| Agent | Lines | Share |
|-------|-------|-------|
| claude-code | 2,741 | 65% |
| cursor | 973 | 23% |
| copilot | 353 | 8% |
| human | 164 | 4% |

### Recent AI commits
- `a1b2c3d` claude-code — "add auth middleware" → src/auth.rs (17-24)
- `b2c3d4e` cursor — "refactor utils" → src/utils.rs (1-89)
```

</details>

---

## How It Works

<details>
<summary>Architecture overview</summary>

**1. `agentdiff configure` — one-time global setup**

Installs Python capture scripts to `~/.agentdiff/scripts/` and registers hooks with each agent:

- Claude Code → `~/.claude/settings.json` (PostToolUse)
- Cursor → `~/.cursor/hooks.json` (afterFileEdit, afterTabFileEdit)
- Codex → `~/.codex/config.toml` (notify)
- Gemini → `~/.gemini/settings.json` (BeforeTool, AfterTool)
- Windsurf → `~/.codeium/windsurf/hooks.json` (post_write_code)
- OpenCode → `~/.config/opencode/plugins/agentdiff.ts` (tool.execute.after)
- Copilot → VS Code extension in `~/.vscode/extensions/agentdiff-copilot-0.1.0/`

**2. `agentdiff init` — per-repo setup**

Installs git `pre-commit` and `post-commit` hooks in `<repo>/.git/hooks/`, creates `.agentdiff/ledger.jsonl`, and registers the repo in `~/.agentdiff/config.toml`.

**3. Capture flow**

When an AI agent makes an edit, its hook fires and writes a JSON entry to `<repo>/.git/agentdiff/session.jsonl`:

```json
{
  "timestamp": "2026-03-28T10:54:00Z",
  "agent": "claude-code",
  "model": "sonnet-4-6",
  "session_id": "sess_abc123",
  "tool": "Edit",
  "file": "src/auth.rs",
  "lines": [17, 18, 19, 20],
  "prompt": "add auth middleware"
}
```

**4. Commit**

On `git commit`:
- Pre-commit hook: matches session entries against staged diff → writes `pending-ledger.json`
- Post-commit hook: finalizes one ledger line with the commit SHA → appends to `.agentdiff/ledger.jsonl`
- By default, auto-amends the commit to include the updated ledger

**5. Query**

```
~/.agentdiff/
├── config.toml           ← global config
└── scripts/              ← capture scripts (Python)

<repo>/.agentdiff/
└── ledger.jsonl          ← committed, append-only attribution log

<repo>/.git/agentdiff/
├── session.jsonl         ← live capture buffer (not committed)
├── pending.json          ← MCP context handoff (ephemeral)
└── pending-ledger.json   ← pre-commit snapshot (ephemeral)
```

</details>

---

## Configuration

Config lives at `~/.agentdiff/config.toml`:

```toml
schema_version = "1.0"
scripts_dir = "~/.agentdiff/scripts"
auto_amend_ledger = true        # include ledger in same commit automatically
data_dir = "~/.agentdiff/spillover"

[[repos]]
path = "/home/user/my-project"
slug = "-home-user-my-project"
```

```bash
# Disable auto-amend
agentdiff config set auto_amend_ledger false

# View current config
agentdiff config show
```

---

## MCP Server

`agentdiff-mcp` is a stdio MCP server for richer context capture. It exposes a `record_context` tool that writes structured metadata before a commit:

```json
{
  "mcpServers": {
    "agentdiff": {
      "command": "agentdiff-mcp",
      "args": []
    }
  }
}
```

When an agent calls `record_context`, the prompt, model, session ID, files read, intent, and trust score are stored and attached to the next ledger entry:

```json
{
  "cwd": "/path/to/repo",
  "model_id": "claude-sonnet-4-6",
  "prompt": "add rate limiting to the API",
  "files_read": ["src/api.rs", "src/config.rs"],
  "intent": "security hardening",
  "trust": 92,
  "flags": ["security"]
}
```

---

## CI Integration

Add AI attribution to your pull requests with one workflow step:

```yaml
# .github/workflows/agentdiff-report.yml
on: [pull_request]

jobs:
  report:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Install agentdiff
        run: |
          curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/master/install.sh | bash
          echo "$HOME/.local/bin" >> $GITHUB_PATH
      - name: Init repo (no agent hooks needed in CI)
        run: agentdiff init --no-git-hook
      - name: Generate report
        run: agentdiff report --format markdown --out-md ai-report.md
      - name: Post as PR comment
        uses: marocchino/sticky-pull-request-comment@v2
        with:
          path: ai-report.md
```

---

## Debugging

```bash
# Enable verbose logging for all capture scripts
export AGENTDIFF_DEBUG=1

# Then make an AI edit and commit, then check logs
tail -f ~/.agentdiff/logs/capture-claude.log
tail -f ~/.agentdiff/logs/capture-cursor.log
tail -f ~/.agentdiff/logs/capture-codex.log

# Check what was captured before committing
cat .git/agentdiff/session.jsonl
```

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).
