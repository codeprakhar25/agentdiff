# agentdiff — Project Context for Claude

## Project summary
`agentdiff` is a Rust + Python CLI that tracks which AI agent (claude-code, cursor, opencode, copilot, etc.) wrote which lines of code in a git repo. It hooks into agent tool callbacks, captures to `session.jsonl`, then on commit runs `prepare-ledger.py` → `finalize-ledger.py` to produce signed `AgentTrace` records in `.git/agentdiff/traces/{branch}.jsonl`.

## Architecture
- **Capture**: per-agent Python scripts (`capture-claude.py`, etc.) write raw events to `.git/agentdiff/session.jsonl`
- **Prepare** (`scripts/prepare-ledger.py`): runs pre-commit, reads session.jsonl, computes per-file attribution using line overlap, writes `pending_ledger.json`
- **Finalize** (`scripts/finalize-ledger.py`): runs post-commit, converts pending payload to signed `AgentTrace`, appends to `traces/{branch}.jsonl`
- **Store** (`src/store.rs`): reads traces into `Entry` structs for `list` / `report` commands
- **Binary install path**: `~/.local/bin/agentdiff` (NOT `~/.cargo/bin/`) — always `cp target/release/agentdiff ~/.local/bin/agentdiff` after build

## Attribution invariants
- `copilot` is in `_EXCLUDED_AGENTS` — captured in session.jsonl for stats, never wins file attribution
- Files with no session evidence → `agent = "human"`, must be explicit in attribution dict
- `agent = "human"` in payload is the semantic token; `git_author` holds the display name
- `contributor.type = "human"` iff `file_agent == "human"` — never infer from tool name
