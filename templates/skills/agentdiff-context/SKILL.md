---
name: agentdiff-context
description: Use AgentDiff trace context during development and review. Use when editing traced files, preparing PR summaries, reviewing agent-authored code, or when the user asks about why a file changed, attribution, intent, files read, or AgentDiff context.
---

# AgentDiff Context

## When To Use

Use this skill when working in a repository that has AgentDiff initialized and the task involves:

- editing an existing file that may have AI attribution
- reviewing a PR or writing a PR summary
- explaining why a file or line range changed
- recording agent intent before a commit
- deciding which files deserve review attention first

## Workflow

1. Before editing a known file, check its trace context:

```bash
agentdiff context path/to/file --json
```

2. For PR review or summary work, inspect structured report context:

```bash
agentdiff report --format json --context
agentdiff report --format markdown --context
```

3. Use the context to answer:

- What intent produced this change?
- Which agent/model touched it?
- Which files were read to make the change?
- Which trace IDs explain the relevant file/ranges?
- Are there flags like `security`, `refactor`, or `risky`?

4. Before committing substantial agent work, record context through AgentDiff MCP when available. Include concise values:

```json
{
  "prompt": "short user request or task summary",
  "model_id": "model name",
  "agent": "cursor",
  "files_read": ["src/api.rs", "src/config.rs"],
  "intent": "security hardening",
  "trust": 80,
  "flags": ["security"]
}
```

## Rules

- Do not paste full prompts into commit messages or PR summaries. Use AgentDiff's stored prompt excerpt/hash.
- Keep `intent` short and review-useful, for example `security hardening`, `bug fix`, `test coverage`, or `refactor`.
- If `agentdiff context` returns no traces, say so and continue with normal code reading.
- Do not treat AgentDiff attribution as proof of correctness. It is context for review, not validation.
- If context conflicts with code reality, trust the code and mention the mismatch.

## Output Guidance

When summarizing AgentDiff context to a user, prefer this shape:

```markdown
AgentDiff context for `path/to/file`:
- Last traced intent: ...
- Agent/model: ...
- Relevant ranges: ...
- Files read: ...
- Review note: ...
```
