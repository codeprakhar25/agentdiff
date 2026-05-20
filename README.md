# agentdiff — Know What Every AI Agent Wrote

<p align="center">
  <strong>Line-level attribution for AI-assisted code. Audit every agent, model, and prompt across your entire git history.</strong>
</p>

<p align="center">
  <a href="https://github.com/codeprakhar25/agentdiff/releases"><img src="https://img.shields.io/github/v/release/codeprakhar25/agentdiff?style=flat-square" alt="Latest release"></a>
  <a href="https://github.com/codeprakhar25/agentdiff/stargazers"><img src="https://img.shields.io/github/stars/codeprakhar25/agentdiff?style=flat-square" alt="GitHub stars"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue?style=flat-square" alt="License"></a>
  <a href="https://github.com/codeprakhar25/agentdiff/actions"><img src="https://img.shields.io/github/actions/workflow/status/codeprakhar25/agentdiff/ci.yml?style=flat-square&label=CI" alt="CI"></a>
  <img src="https://img.shields.io/badge/agents-7%2B-blueviolet?style=flat-square" alt="Agents supported">
  <img src="https://img.shields.io/badge/built_with-Rust-orange?style=flat-square" alt="Built with Rust">
</p>

---

agentdiff hooks into every major AI coding agent — Claude Code, Cursor, Codex, Copilot, Windsurf, OpenCode, Gemini — and writes a permanent, commit-scoped attribution record to your repository. Each record captures the agent name, model, prompt excerpt, and exact line ranges. All of it queryable from the CLI, no server required.

[Watch the launch demo](https://x.com/PrakharKhatri3/status/2049703391488888903) to see the attribution workflow end-to-end.

```
agentdiff list

  agentdiff list — 5 entries

  #    COMMIT     TIME          AGENT          MODEL                  FILE(S)                          LINES              TRUST    PROMPT
  ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  1    a1b2c3d4   Apr 14 09:12  claude-code    claude-sonnet-4-6      src/commands/push.rs             1-47               92       "fix ordering: write local ref before…"
  2    b2c3d4e5   Apr 14 09:44  codex          o4-mini                src/store.rs +2                  112-198, 201-230   —        "add fetch_ref_content helper"
  3    c3d4e5f6   Apr 13 18:01  cursor         cursor-fast            src/cli.rs                       305-381            —        "add status --remote args struct"
  4    d4e5f6a7   Apr 13 17:30  opencode       claude-sonnet-4-6      src/main.rs                      80-94              88       "wire remote_status dispatch"
  5    e5f6a7b8   Apr 13 14:22  human          —                      README.md                        —                 —        —
```

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/main/install.sh | bash
```

**Requirements:** Python 3.7+ on PATH, Git 2.20+

<details>
<summary>Other install methods</summary>

```bash
# Specific version
curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/main/install.sh | bash -s -- --version v0.1.0

# From source (requires Rust 1.85+)
git clone https://github.com/codeprakhar25/agentdiff.git
cd agentdiff
cargo build --release
mkdir -p ~/.local/bin
install -m 0755 target/release/agentdiff ~/.local/bin/agentdiff
install -m 0755 target/release/agentdiff-mcp ~/.local/bin/agentdiff-mcp
```

</details>

---

## Quick Start

```bash
# 1. Configure global agent hooks — run once per machine
agentdiff configure

# 2. Initialize a repository
cd ~/your-project
agentdiff init

# 3. Verify hooks are active
agentdiff status

# 4. Work normally — make AI-assisted edits, then commit
git add . && git commit -m "feat: add feature"

# 5. Inspect attribution
agentdiff list
agentdiff blame src/main.rs
agentdiff report --by-file --by-model

# 6. Give local agents context before editing traced files
agentdiff context src/main.rs --json
```

That's it. From here every commit is attributed to whichever agent (or human) wrote it.

> **Note:** `agentdiff configure` installs capture scripts globally, but capture only fires in repos where `agentdiff init` has been run (the `.git/agentdiff/` directory must exist). Running `configure` on its own does not track any repo — you must also run `agentdiff init` inside each repo you want to track.

> **AGENTS.md:** `agentdiff configure` also writes (or updates) an `## AgentDiff` section in `AGENTS.md` in the current directory. This file is the emerging standard for multi-agent repo context — Codex, Cursor, Copilot, and other tools read it to understand repo conventions. The section is idempotent: re-running configure updates it without duplicating or touching the rest of your `AGENTS.md`. Use `--no-agents-md` to skip.

---

## Commands

| Command | Description |
|---------|-------------|
| `agentdiff configure` | Install global agent capture hooks and write `AGENTS.md` context — run once per machine |
| `agentdiff init` | Initialize tracking in current repository (required per repo) |
| `agentdiff install-ci` | Write CI workflow YAMLs to `.github/workflows/` — run once per repo |
| `agentdiff list` | List attribution entries |
| `agentdiff blame <file>` | Line-level attribution, like `git blame` |
| `agentdiff context <file>` | File-scoped trace context: intent, prompt excerpt, files read, flags, trust |
| `agentdiff diff [<sha>]` | Attribution diff for a commit or range |
| `agentdiff show <sha>` | Full details for one trace entry |
| `agentdiff report` | Aggregate report (text, markdown, annotations, JSONL) |
| `agentdiff install-skill` | Install the AgentDiff context skill into a project or globally |
| `agentdiff status` | Health check — hooks, keys, traces |
| `agentdiff status --remote` | Show remote trace ref state (`refs/agentdiff/*` on origin) |
| `agentdiff push` | Push local traces to per-branch ref on origin |
| `agentdiff consolidate` | Merge per-branch traces into permanent store (CI) |
| `agentdiff verify` | Verify ed25519 signatures on trace entries |
| `agentdiff keys init` | Generate a local signing keypair |
| `agentdiff keys register` | Register your public key in the git key registry |
| `agentdiff keys rotate` | Rotate your keypair and register the new key |
| `agentdiff policy check` | Enforce AI attribution policy rules |
| `agentdiff config` | Manage global configuration |

<details>
<summary>Command flags and examples</summary>

```bash
# Filter list by agent or file
agentdiff list --agent cursor --file src/auth
agentdiff list --limit 50

# Blame for a specific agent only
agentdiff blame src/api.rs --agent claude-code

# Show why a file was changed and what context the agent used
agentdiff context src/api.rs
agentdiff context src/api.rs --json

# Report broken down by file and model
agentdiff report --by-file --by-model

# Report from a specific date
agentdiff report --since 2026-01-01T00:00:00Z

# Report to file
agentdiff report --format markdown --out report.md
agentdiff report --format annotations --out annotations.json

# Include intent, files read, flags, trust, and trace IDs in reports
agentdiff report --format markdown --context
agentdiff report --format json --context

# Post report as a PR comment (auto-detects PR from current branch)
agentdiff report --format markdown --post-pr-comment
agentdiff report --format markdown --post-pr-comment 42   # explicit PR number

# Install the local agent guidance skill into this repo
agentdiff install-skill --scope project
agentdiff install-skill --scope global   # optional personal default

# Attribution diff for last 3 commits
agentdiff diff HEAD~3

# Verify signatures since merge-base with main
agentdiff verify
agentdiff verify --since abc1234 --strict

# Policy check (reads .agentdiff/policy.toml)
agentdiff policy check
agentdiff policy check --format github-annotations

# Push traces from current branch to GitHub
agentdiff push

# Consolidate a branch's traces into permanent store (CI step)
agentdiff consolidate --branch feature/my-branch --push

# Write CI workflows to .github/workflows/ (run once per repo)
agentdiff install-ci

# Configure all supported agents directly, including Gemini/Antigravity
agentdiff configure --all

# Configure only selected agents without the interactive picker
agentdiff configure --agents cursor,codex,opencode

# Skip specific agents during configure
agentdiff configure --no-copilot --no-antigravity

# Skip git hook install during init
agentdiff init --no-git-hook

# Check remote trace ref state after pushing
agentdiff status --remote
agentdiff status --remote --no-fetch   # fast: show refs + SHAs only, skip trace counts
```

</details>

---

## Supported Agents

| Agent | Hook mechanism | Captures |
|-------|---------------|----------|
| **Claude Code** | `PostToolUse` hook (`~/.claude/settings.json`) | Edit, Write, MultiEdit |
| **Cursor** | `afterFileEdit`, `afterTabFileEdit` hooks | Agent edits + Tab completions |
| **GitHub Copilot** | VS Code extension (`~/.vscode/extensions/`) | Large inline insertions, saved AI edits, manual captures |
| **Windsurf** | `post_write_code` hook (`~/.codeium/windsurf/hooks.json`) | Cascade agent writes |
| **OpenCode** | `tool.execute.after` plugin (`~/.config/opencode/plugins/`) | All tool writes |
| **Codex CLI** | `notify` hook (`~/.codex/config.toml`) | Task-level file changes |
| **Gemini / Antigravity** | `BeforeTool`/`AfterTool` hooks (`~/.gemini/settings.json`) | `write_file`, `replace` |

Agent hooks are installed **globally once** via `agentdiff configure`. In an interactive terminal, AgentDiff detects available agent configs and lets you choose integrations with Space + Enter. By default it selects the main coding agents and leaves Gemini/Antigravity optional; use `agentdiff configure --all` to install every supported integration directly. Claude MCP setup is part of the Claude Code integration, so it only runs when Claude is selected; use `--no-mcp` to skip MCP while still configuring Claude. Capture only fires in repos where `agentdiff init` has been run — the `.git/agentdiff/` directory must exist for any data to be written.

---

## Example Output

```
agentdiff list

  agentdiff list — 6 entries

  #    COMMIT     TIME          AGENT          MODEL                  FILE(S)                          LINES              TRUST    PROMPT
  ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  1    a1b2c3d4   Apr 14 09:12  claude-code    claude-sonnet-4-6      src/commands/push.rs             1-47               92       "fix ordering: write local ref befor…"
  2    b2c3d4e5   Apr 14 09:44  codex          o4-mini                src/store.rs +2                  112-198, 201-230   —        "add fetch_ref_content helper"
  3    c3d4e5f6   Apr 13 18:01  cursor         cursor-fast            src/cli.rs                       305-381            —        "add status --remote args struct"
  4    d4e5f6a7   Apr 13 17:30  opencode       claude-sonnet-4-6      src/main.rs                      80-94              88       "wire remote_status dispatch"
  5    e5f6a7b8   Apr 12 11:04  windsurf       claude-sonnet-4-6      src/init.rs                      44-68              —        "remove legacy .agentdiff dir creat…"
  6    f6a7b8c9   Apr 11 16:22  human          —                      README.md                        —                  —        —
```

<details>
<summary>agentdiff list flags</summary>

```bash
# Filter to a specific agent
agentdiff list --agent claude-code

# Filter to files matching a path substring
agentdiff list --file src/commands

# Show the 10 most recent entries
agentdiff list -n 10

# Show only uncommitted (in-progress session) entries
agentdiff list --uncommitted
```

</details>

<details>
<summary>agentdiff report --by-file --by-model</summary>

```
  agentdiff report

  Total lines tracked: 4,231

  By Agent:
    claude-code   2,741 (65%) ████████████████████
    codex           892 (21%) ███████
    opencode        282  (7%) ██
    cursor          148  (3%) █
    human           168  (4%) █

  By Model:
    claude-sonnet-4-6   3,023 (72%)
    o4-mini               892 (21%)
    cursor-fast           148  (3%)
    —                     168  (4%)
```

</details>

<details>
<summary>agentdiff context src/api.rs --json</summary>

```json
{
  "file": "src/api.rs",
  "traces": [
    {
      "short_id": "60eb15b8",
      "agent": "cursor",
      "intent": "security hardening",
      "prompt_excerpt": "add rate limiting to the API",
      "files_read": ["src/api.rs", "src/config.rs"],
      "flags": ["security"],
      "trust": 92,
      "ranges": [{ "start_line": 17, "end_line": 24 }]
    }
  ]
}
```

</details>

<details>
<summary>agentdiff status --remote</summary>

```
  agentdiff status --remote — github.com/org/repo

  REF                                           TRACES     LOCAL
  ────────────────────────────────────────────────────────────────────────────
  refs/agentdiff/meta                           18         synced
  refs/agentdiff/traces/main                    6          synced
  refs/agentdiff/traces/feature%2Fauth-rewrite  3          synced
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
<summary>agentdiff verify</summary>

```
  ok 550e8400 — valid
  ok b2c3d4e5 — valid
  warn d4e5f6a7 — no signature
  Verified 4 entries: 3 valid, 1 missing sig, 0 invalid
```

</details>

<details>
<summary>agentdiff report --format markdown --context</summary>

```markdown
# AgentDiff Report

## Summary

| Agent | Lines | % |
|-------|-------|---|
| claude-code | 2,741 | 65% |
| cursor | 973 | 23% |
| human | 164 | 4% |

## Review Context

- Intent: security hardening (17 lines, 1 file)
  - Agent/model: claude-code / claude-sonnet-4-6
  - Files read: src/api.rs, src/config.rs
  - Prompt: add rate limiting to the API
  - Flags: security

## Files To Review First

| File | Lines | Dominant Agent | Intent | Context |
|------|-------|----------------|--------|---------|
| src/api.rs | 17 | claude-code | security hardening | trace 550e8400 |
```

</details>

---

## How It Works

<details>
<summary>Architecture overview</summary>

**1. `agentdiff configure` — one-time global setup**

Installs Python capture scripts to `~/.agentdiff/scripts/` and registers hooks with selected agents. In an interactive terminal, AgentDiff shows a Space/Enter multi-select picker; in scripts, use `--all` or `--agents cursor,codex` to avoid prompting. The Claude MCP server is registered only when Claude Code is selected.

- Claude Code → `~/.claude/settings.json` (PostToolUse)
- Cursor → `~/.cursor/hooks.json` (afterFileEdit, afterTabFileEdit)
- Codex → `~/.codex/config.toml` (notify)
- Gemini → `~/.gemini/settings.json` (BeforeTool, AfterTool)
- Windsurf → `~/.codeium/windsurf/hooks.json` (post_write_code)
- OpenCode → `~/.config/opencode/plugins/agentdiff.ts` (tool.execute.after)
- Copilot → VS Code extension in `~/.vscode/extensions/agentdiff-copilot-0.1.0/` or the matching VS Code Server extensions directory for WSL/remote workspaces

**2. `agentdiff init` — per-repo setup**

Installs git `pre-commit`, `post-commit`, and `pre-push` hooks. Configures a `refs/agentdiff/*` fetch refspec so teammates' traces are visible after `git fetch`.

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

The VS Code Copilot extension also writes structured JSON diagnostics to the `AgentDiff` output channel. Run `AgentDiff: Open Logs` from the VS Code command palette when capture appears inactive or a repository has not been initialized.

**4. Commit → sign → push**

On `git commit`:
- Pre-commit hook: matches session entries against staged diff → writes `pending-ledger.json`
- Post-commit hook: finalizes one trace entry (UUID-keyed, Agent Trace v0.1 format) into the local buffer at `.git/agentdiff/traces/{branch}.jsonl`; attaches structured context such as `intent`, `files_read`, `flags`, and `trust`; signs it with ed25519 if keys are configured

On `git push`:
- Pre-push hook: uploads the local trace buffer to `refs/agentdiff/traces/{branch}` on origin via the GitHub Git Database API; auto-consolidates on direct pushes to main/master

**5. Three-tier storage**

```
Tier 1 — local buffer (ephemeral)
  .git/agentdiff/traces/{branch}.jsonl

Tier 2 — per-branch ref (on GitHub, per developer)
  refs/agentdiff/traces/{branch}:traces.jsonl

Tier 3 — permanent meta store (consolidated by CI)
  refs/agentdiff/meta:traces.jsonl
```

The `refs/agentdiff/*` namespace sits outside `refs/heads/*`, so branch protection rules never block it. UUIDs survive squash/rebase/cherry-pick — a trace is never lost due to SHA rewriting.

**6. Key registry**

Signing keys are registered per-developer in `refs/agentdiff/keys/{key_id}:pub.key`. `agentdiff verify` looks up each signature's `key_id` in the registry, so you can verify traces signed by any team member's key without manually exchanging public keys.

**7. Directory layout**

```
~/.agentdiff/
├── config.toml           ← global config
├── keys/
│   ├── private.key       ← ed25519 signing key (chmod 600)
│   └── public.key        ← ed25519 verifying key
└── scripts/              ← capture scripts (Python)

<repo>/.agentdiff/
└── policy.toml           ← optional policy rules

<repo>/.git/agentdiff/
├── session.jsonl         ← live capture buffer (not committed)
├── pending.json          ← MCP context handoff (ephemeral)
├── pending-ledger.json   ← pre-commit snapshot (ephemeral)
└── traces/
    └── {branch}.jsonl    ← local trace buffer (pushed by pre-push hook)
```

</details>

---

## Agent Context Workflow

agentdiff can preserve lightweight intent and files-read context so reviewers and local agents can understand why a change was made, not just which lines were attributed.

```bash
# Before editing a traced file, inspect its local context
agentdiff context src/api.rs --json

# Before PR review or summaries, generate a context-aware report
agentdiff report --format markdown --context
agentdiff report --format json --context

# Install project-local guidance so Cursor agents learn this workflow
agentdiff install-skill --scope project
```

`agentdiff install-skill --scope project` writes `.cursor/skills/agentdiff-context/SKILL.md` in the current repo. Use `--scope global` for a personal default, and `--force` to overwrite an existing skill file.

When used with `--post-pr-comment`, context reports are filtered to commits on the current PR branch and update the existing AgentDiff comment when possible.

---

## Signing & Verification

agentdiff can sign each trace entry with an ed25519 key so tampering is detectable:

```bash
# One-time setup per developer
agentdiff keys init

# Register your public key so teammates can verify your signatures
agentdiff keys register

# Rotate keys (backs up old keys, generates new ones, registers them)
agentdiff keys rotate

# Verify the current branch's trace history
agentdiff verify

# Strict mode — exit immediately on any missing or invalid signature
agentdiff verify --strict

# Verify a specific range
agentdiff verify --since abc1234
```

Each trace record stores `sig.key_id` (first 16 hex chars of SHA-256 of the public key). `agentdiff verify` looks up the matching key from the git key registry (`refs/agentdiff/keys/{key_id}`) — no manual key exchange required.

---

## Policy Enforcement

Define AI attribution rules in `.agentdiff/policy.toml`:

```toml
# Fail CI if AI wrote more than 80% of lines in this PR
max_ai_percent = 80.0

# Every trace must have at least one attributed file
require_attribution = true

# Every trace must carry an ed25519 signature
require_signed = true

# Override the default branch for merge-base calculation
# base_branch = "develop"
```

Run in CI:

```bash
agentdiff policy check
agentdiff policy check --format github-annotations  # inline PR annotations
```

Exits 0 on pass, 1 on violation. Use `--since <sha>` to scope to a specific range.

---

## CI Integration

Run once to write both workflow files into your repo:

```bash
agentdiff install-ci
git add .github/workflows/agentdiff-*.yml
git commit -m "ci: add agentdiff consolidation and policy workflows"
```

This writes two workflows:

- **`agentdiff-consolidate.yml`** — triggers on PR merge: consolidates per-branch traces into the permanent store and posts an attribution comment to the PR.
- **`agentdiff-policy.yml`** — triggers on every PR: runs `agentdiff policy check` and posts GitHub check annotations if rules are violated.

For repos that need a custom pipeline, the manual equivalent:

```yaml
# .github/workflows/agentdiff-policy.yml
on: [pull_request]
permissions:
  contents: read
  checks: write

jobs:
  agentdiff:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install agentdiff
        run: |
          curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/main/install.sh | bash
          echo "$HOME/.local/bin" >> $GITHUB_PATH

      - name: Fetch agentdiff refs
        run: git fetch origin '+refs/agentdiff/*:refs/agentdiff/*' || true

      - name: Verify signatures
        run: agentdiff verify

      - name: Policy check
        run: agentdiff policy check --format github-annotations
```

---

## Configuration

Config lives at `~/.agentdiff/config.toml`:

```toml
schema_version = "1.0"
scripts_dir = "~/.agentdiff/scripts"
capture_prompts = true   # set false to omit prompt excerpts from traces

[[repos]]
path = "/home/user/my-project"
slug = "-home-user-my-project"
```

```bash
# Disable prompt capture
agentdiff config set capture_prompts false

# View current config
agentdiff config show
```

---

## Data & Privacy

**What agentdiff captures:**

Each AI-assisted edit generates a trace entry containing:
- Agent name and model (e.g., `claude-code`, `claude-sonnet-4-6`)
- A short prompt excerpt (the first ~100 characters of your request to the AI)
- Optional structured context from MCP or `record-context.py`: intent, files read, flags, and trust score
- File paths and line ranges affected
- Timestamp and session ID

**Where it's stored:**

- **Locally:** `.git/agentdiff/session.jsonl` (not committed, stays in your `.git/` directory)
- **On GitHub:** `refs/agentdiff/traces/{branch}` — pushed by `agentdiff push` or the pre-push hook

**Prompt content visibility:** Once pushed, prompt excerpts are accessible to anyone with read access to the repository. If your prompts contain sensitive business context, IP, or credentials, disable prompt capture:

```bash
agentdiff config set capture_prompts false
```

When `capture_prompts = false`, the `prompt` field is omitted from all trace entries.

**No external telemetry.** agentdiff does not send data to any server outside your own GitHub repository.

---

## Editor Event Capture Limits

VS Code does not expose a perfect "this exact range came from Copilot" event to extensions. AgentDiff therefore treats Copilot capture as a conservative heuristic:

**What the VS Code extension can capture:**

- File-backed `onDidChangeTextDocument` events while the GitHub Copilot extension is installed and active.
- Multi-line insertions, or single-line insertions of at least 50 characters, which filters out most normal typing.
- The next save of a buffered file, plus document version and timestamps so stale captures are easier to diagnose.
- Manual full-file captures through `AgentDiff: Capture Copilot edits for current file`, useful after a Copilot Chat edit.
- The correct repository in multi-root workspaces by resolving the edited document's owning workspace folder and git root.
- WSL/remote extension hosts installed under VS Code Server paths; Windows-hosted extensions can bridge to WSL capture scripts when configured from WSL.

**What it cannot reliably capture:**

- Rejected Copilot suggestions, hover/chat text that never becomes a file edit, or edits in virtual documents such as git diff views.
- The exact Copilot prompt, chat transcript, or acceptance source; VS Code does not provide those details to third-party extensions.
- A guaranteed distinction between Copilot, paste, and another extension when they all produce a large text-document change event.
- Rewrites in repositories where `agentdiff init` has not created `.git/agentdiff/`; these are logged as skipped captures in the `AgentDiff` output channel.

For local extension development, run:

```bash
node --test scripts/tests/test_extension.js
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

## Debugging

For Copilot/VS Code capture, open the command palette and run `AgentDiff: Open Logs`. The extension writes structured JSON events there, including activation state, skipped captures, repo initialization failures, git lookup failures, and capture spawn errors.

```bash
# Enable verbose logging for all capture scripts
export AGENTDIFF_DEBUG=1

# Then make an AI edit and commit, then check file logs
tail -f ~/.agentdiff/logs/capture-claude.log
tail -f ~/.agentdiff/logs/capture-cursor.log
tail -f ~/.agentdiff/logs/capture-codex.log
tail -f ~/.agentdiff/logs/capture-copilot-ext.log

# Check what was captured before committing
cat .git/agentdiff/session.jsonl

# Check agentdiff health (hooks, keys, pending traces)
agentdiff status
```

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).
