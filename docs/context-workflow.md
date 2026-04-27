# AgentDiff Context Workflow

This note records the context work added to AgentDiff so the implementation and validation criteria do not live only in chat history.

## What Changed

AgentDiff now preserves more of the structured context agents already provide before a commit. `scripts/finalize-ledger.py` writes these fields into `metadata.agentdiff` on each `AgentTrace`:

- `intent`
- `files_read`
- `author`
- `capture_tool`
- existing prompt excerpt/hash, session id, trust, and flags

Prompt text still follows the `capture_prompts` privacy gate. Intent and files-read are stored because they are explicit structured context, not raw transcript capture.

## Report Context

`agentdiff report --format markdown --context` adds a compact review section for humans and PR bots:

- a per-agent line summary
- grouped review context by intent
- files read, flags, trust, and prompt excerpt when available
- a `Files To Review First` table with trace ids
- collapsed trace details for deeper inspection

`agentdiff report --format json --context` exposes the same trace metadata for bots or custom tooling.

The goal is not to make the PR comment long. The goal is to help a reviewer quickly decide which files deserve attention first and why the agent changed them.

When posting a PR comment with `--post-pr-comment`, AgentDiff filters the report to commits on the current branch since the merge-base with the default branch. This keeps old consolidated traces from `main` out of the PR review comment. Comment posting first tries to edit the last AgentDiff comment and falls back to creating one, so it works with older `gh` versions that do not support `--create-if-none`.

## Local File Context

`agentdiff context <file>` shows trace context for one file:

```bash
agentdiff context src/api.rs
agentdiff context src/api.rs --json
```

This is the local agent-facing surface. Before editing a traced file, an agent can inspect:

- who or what last changed the file
- the recorded intent
- relevant line ranges
- files read when the change was made
- flags and trust metadata

This should help agents avoid losing project context in large codebases, but it is not a replacement for reading the code.

## Cursor Skill

The project skill at `.cursor/skills/agentdiff-context/SKILL.md` tells agents to:

- run `agentdiff context <file> --json` before modifying traced files
- run `agentdiff report --format json --context` before PR review or summaries
- record concise intent and files-read context before substantial commits

The skill helps local Cursor agents. Generic GitHub review bots only benefit if they read the PR comment, consume the JSON report, or run AgentDiff themselves.

Install it into a repository with:

```bash
agentdiff install-skill --scope project
```

Global install is available for personal defaults, but project install is preferred because repository-specific AgentDiff guidance should be versioned with the code:

```bash
agentdiff install-skill --scope global
```

## Validation In `~/roam/monorepo`

The realistic validation target is the local `~/roam/monorepo` checkout for GitHub repo `roam-agentdiff`.

Validation should check:

- `agentdiff push` is fast with no new traces and with one new trace
- `agentdiff report --format markdown --context` stays readable
- `agentdiff report --format json --context` is parseable
- `agentdiff context <changed-file> --json` returns useful intent/files-read metadata
- the CI PR comment is updated rather than duplicated

The release validation PR was opened at `https://github.com/codeprakhar25/roam-agentdiff/pull/6`. It installed the AgentDiff context skill and added a small validation note. The PR comment was posted with a local AgentDiff build and correctly filtered out old `main` traces.

Measured timings in `~/roam/monorepo`:

- `agentdiff push` with 2 new traces: `5.09s`
- `agentdiff push` with no local traces: `0.00s`
- `agentdiff report --format markdown --context`: `0.02s`
- `agentdiff report --format json --context`: `0.02s`
- `agentdiff context agentdiff-context-validation.md --json`: `0.01s`

The validation trace included:

- intent: `agentdiff context validation`
- files read: `agentdiff-context-smoke.txt`, `README.md`
- flags: `validation`
- trust: `90`

The final PR validation used intent `agentdiff release validation` with files read `agentdiff-context-validation.md` and `README.md`. The generated PR comment surfaced that intent in `Review Context`, and file-scoped JSON returned the same metadata for the changed file.

## Known Limitations

- AgentDiff context is review context, not correctness evidence.
- Large PR comments still need size discipline; detailed trace data should stay collapsed or in JSON.
- Older traces may not include intent/files-read, so reports must handle `unspecified`.
- Bots that do not read AgentDiff output will not benefit from the skill alone.
