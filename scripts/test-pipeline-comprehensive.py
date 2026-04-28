#!/usr/bin/env python3
"""
scripts/test-pipeline-comprehensive.py

Comprehensive agentdiff pipeline integration test.

Creates a fresh test repo, invokes real agents (claude-code, codex, opencode)
with timeouts to write files inside ml-research/, then validates the full
capture → prepare → finalize pipeline, reporting every gap with a fix suggestion.

Usage:
    python3 scripts/test-pipeline-comprehensive.py [options]

Options:
    --simulate-only   Skip real agent invocation; inject synthetic hook payloads
    --debug           Set AGENTDIFF_DEBUG=1 for all capture scripts
    --keep-dir        Keep test dir after exit (for manual debugging)
    --repo PATH       Use an existing agentdiff-init'd repo instead of creating one
    --timeout N       Per-agent timeout in seconds (default: 90)
    --no-commit       Skip commit phase (only check session.jsonl capture)
    --agents A,B,C    Comma-separated agents to test (default: claude-code,codex,opencode)
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# ─── ANSI colours ─────────────────────────────────────────────────────────────

IS_TTY = sys.stdout.isatty()

def _c(code: str, text: str) -> str:
    if not IS_TTY:
        return text
    return f"\033[{code}m{text}\033[0m"

RED    = lambda t: _c("0;31", t)
GREEN  = lambda t: _c("0;32", t)
YELLOW = lambda t: _c("0;33", t)
CYAN   = lambda t: _c("0;36", t)
BOLD   = lambda t: _c("1", t)
DIM    = lambda t: _c("2", t)


def header(msg: str) -> None:
    print(f"\n{BOLD(CYAN(f'═══ {msg} ═══'))}")

def info(msg: str) -> None:
    print(f"  {CYAN('·')} {msg}")

def ok(msg: str) -> None:
    print(f"  {GREEN('✓')} {msg}")

def warn(msg: str) -> None:
    print(f"  {YELLOW('!')} {msg}")

def err(msg: str) -> None:
    print(f"  {RED('✗')} {msg}")

def gap(msg: str, fix: str) -> None:
    print(f"  {RED('GAP')}  {msg}")
    print(f"         {DIM('FIX:')} {fix}")


# ─── Data structures ──────────────────────────────────────────────────────────

@dataclass
class SessionEntry:
    raw: dict
    agent: str = ""
    model: str = ""
    prompt: str = ""
    session_id: str = ""
    tool: str = ""
    file: str = ""
    lines: List[int] = field(default_factory=list)
    timestamp: str = ""

    def __post_init__(self):
        self.agent      = self.raw.get("agent", "")
        self.model      = self.raw.get("model", "")
        self.prompt     = self.raw.get("prompt", "")
        self.session_id = self.raw.get("session_id", "")
        self.tool       = self.raw.get("tool", "")
        self.file       = self.raw.get("file", "")
        self.lines      = self.raw.get("lines", [])
        self.timestamp  = self.raw.get("timestamp", "")

    # Data-quality predicates
    @property
    def model_ok(self) -> bool:
        return bool(self.model) and self.model not in ("unknown", "", agent_basename(self.agent))

    @property
    def prompt_ok(self) -> bool:
        return bool(self.prompt) and self.prompt not in ("unknown", "", "null", None)

    @property
    def lines_ok(self) -> bool:
        return isinstance(self.lines, list) and len(self.lines) > 0

    @property
    def file_ok(self) -> bool:
        return bool(self.file) and not os.path.isabs(self.file)


def agent_basename(agent: str) -> str:
    """Return the fallback model string each agent uses when it can't read the model."""
    return {"claude-code": "unknown", "codex": "codex", "opencode": "opencode"}.get(agent, "unknown")


@dataclass
class AgentResult:
    agent: str
    ran: bool = False           # did we attempt invocation?
    real: bool = False          # was it a real (not simulated) run?
    exit_code: Optional[int] = None
    timed_out: bool = False
    stdout: str = ""
    stderr: str = ""
    files_created: List[str] = field(default_factory=list)
    entries: List[SessionEntry] = field(default_factory=list)
    gaps: List[Tuple[str, str]] = field(default_factory=list)  # (description, fix)

    @property
    def captured(self) -> bool:
        return len(self.entries) > 0

    @property
    def quality_score(self) -> int:
        if not self.entries:
            return 0
        e = self.entries[-1]  # take the last/richest entry
        score = 0
        if e.agent:      score += 1
        if e.model_ok:   score += 2
        if e.prompt_ok:  score += 2
        if e.lines_ok:   score += 1
        if e.file_ok:    score += 1
        return score   # max 7


# ─── Helpers ──────────────────────────────────────────────────────────────────

SCRIPTS_DIR = Path(os.path.expanduser("~/.agentdiff/scripts"))


def run(cmd: List[str], cwd: str = ".", env: Optional[dict] = None,
        timeout: Optional[int] = None, input_text: Optional[str] = None) -> subprocess.CompletedProcess:
    merged_env = {**os.environ, **(env or {})}
    return subprocess.run(
        cmd, cwd=cwd, env=merged_env, text=True, capture_output=True,
        timeout=timeout, input=input_text,
    )


def inject_capture(script: str, payload: dict, cwd: str, debug: bool = False,
                    extra_env: Optional[dict] = None) -> bool:
    """Inject a hook payload into a capture script, return True on success."""
    env: dict = {}
    if debug:
        env["AGENTDIFF_DEBUG"] = "1"
    if extra_env:
        env.update(extra_env)
    script_path = SCRIPTS_DIR / script
    if not script_path.exists():
        warn(f"Capture script not found: {script_path}")
        return False
    try:
        result = run(
            [sys.executable, str(script_path)],
            cwd=cwd, env=env,
            input_text=json.dumps(payload),
            timeout=10,
        )
        return result.returncode == 0
    except Exception as e:
        warn(f"inject_capture {script} failed: {e}")
        return False


def read_session_entries(session_log: Path) -> List[SessionEntry]:
    if not session_log.exists():
        return []
    entries = []
    for line in session_log.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            entries.append(SessionEntry(json.loads(line)))
        except json.JSONDecodeError:
            pass
    return entries


def read_traces(traces_dir: Path, branch: str) -> List[dict]:
    safe_branch = branch.replace("/", "%2F")
    path = traces_dir / f"{safe_branch}.jsonl"
    if not path.exists():
        return []
    traces = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            traces.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return traces


def current_branch(repo_root: str) -> str:
    r = run(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=repo_root)
    return r.stdout.strip() if r.returncode == 0 else "main"


def agent_available(agent: str) -> bool:
    return shutil.which(agent_cmd(agent)) is not None


def agent_cmd(agent: str) -> str:
    return {"claude-code": "claude", "codex": "codex", "opencode": "opencode"}.get(agent, agent)


# ─── Setup ────────────────────────────────────────────────────────────────────

def setup_test_repo(base_dir: Optional[str] = None) -> Path:
    if base_dir:
        repo = Path(base_dir)
        info(f"Using existing repo: {repo}")
        return repo

    tmp = Path(tempfile.mkdtemp(prefix="agentdiff-pipeline-"))
    info(f"Created test repo: {tmp}")

    run(["git", "init", "-q"], cwd=str(tmp))
    run(["git", "config", "user.email", "pipeline-test@agentdiff.test"], cwd=str(tmp))
    run(["git", "config", "user.name", "Pipeline Test"], cwd=str(tmp))

    # Seed file + initial commit
    seed = tmp / "README.md"
    seed.write_text("# ML Research Test Repo\n\ngenerated by agentdiff pipeline test\n")
    run(["git", "add", "README.md"], cwd=str(tmp))
    run(["git", "commit", "-q", "-m", "chore: initial seed"], cwd=str(tmp))

    # ml-research directory with starter files so agents have context
    ml = tmp / "ml-research"
    ml.mkdir()

    (ml / "context.md").write_text(textwrap.dedent("""\
        # ML Research Context

        This is a test ML research project.
        Current focus: fine-tuning a language model for classification.

        ## Stack
        - PyTorch / HuggingFace Transformers
        - Python 3.11
        - Dataset: custom CSV with text + label columns

        ## Goal
        Predict sentiment label (positive / negative / neutral) from text.
    """))

    (ml / "config.py").write_text(textwrap.dedent("""\
        # Training configuration
        MODEL_NAME = "distilbert-base-uncased"
        NUM_LABELS = 3
        BATCH_SIZE = 16
        MAX_EPOCHS = 5
        LEARNING_RATE = 2e-5
    """))

    run(["git", "add", "-A"], cwd=str(tmp))
    run(["git", "commit", "-q", "-m", "chore: add ml-research starter files"], cwd=str(tmp))

    # agentdiff init
    r = run(["agentdiff", "init"], cwd=str(tmp))
    if r.returncode != 0:
        warn(f"agentdiff init failed: {r.stderr.strip()}")
    else:
        ok("agentdiff init succeeded")

    return tmp


# ─── Per-agent tasks ──────────────────────────────────────────────────────────

AGENT_TASKS: Dict[str, dict] = {
    "claude-code": {
        "prompt": (
            "In the ml-research/ directory, create a new file called neural_net.py. "
            "It should contain a minimal PyTorch transformer encoder class called MiniTransformer "
            "with __init__ and forward methods. Keep it under 60 lines. "
            "Do NOT modify any existing files."
        ),
        "target_file": "ml-research/neural_net.py",
    },
    "codex": {
        "prompt": (
            "In the ml-research/ directory, create a new file called data_pipeline.py. "
            "It should contain a simple PyTorch Dataset class called TextDataset "
            "with __init__, __len__, and __getitem__ methods reading from a CSV file. "
            "Keep it under 60 lines."
        ),
        "target_file": "ml-research/data_pipeline.py",
    },
    "opencode": {
        "prompt": (
            "In the ml-research/ directory, create a new file called trainer.py. "
            "It should contain a training loop function called train_epoch that takes "
            "model, dataloader, optimizer, device and returns average loss. "
            "Keep it under 60 lines."
        ),
        "target_file": "ml-research/trainer.py",
    },
}

# Simulated payloads used in --simulate-only mode
def simulated_payload(agent: str, repo_root: str, target_file: str, content: str) -> dict:
    abs_file = os.path.join(repo_root, target_file)
    task_prompt = AGENT_TASKS[agent]["prompt"]

    if agent == "claude-code":
        # PostToolUse Write hook — session_id is fake so history lookup will miss;
        # AGENTDIFF_PROMPT env var injected by run_simulated_agent compensates.
        return {
            "tool": "Write",
            "tool_input": {
                "file_path": abs_file,
                "content": content,
            },
            "session_id": "sim-claude-sess-001",
            "cwd": repo_root,
        }

    elif agent == "codex":
        # task_complete notify event — prompt comes from last_agent_message here
        # (history lookup uses fake session_id; event prompt is good enough).
        return {
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "last_agent_message": task_prompt[:300],
                "turn_id": "sim-codex-turn-001",
            },
            "session_meta": {"id": "sim-codex-sess-001", "cwd": repo_root},
            "cwd": repo_root,
            "model": "o4-mini",
        }

    elif agent == "opencode":
        # Include prompt directly in payload — capture-opencode uses it when not "unknown".
        return {
            "hook_event_name": "PostToolUse",
            "tool_name": "write",
            "tool_input": {
                "filePath": abs_file,
                "content": content,
            },
            "session_id": "sim-opencode-sess-001",
            "model": "claude-sonnet-4-5",
            "prompt": task_prompt[:300],
            "cwd": repo_root,
        }

    return {}


SIMULATED_CONTENT = {
    "claude-code": textwrap.dedent("""\
        import torch
        import torch.nn as nn

        class MiniTransformer(nn.Module):
            def __init__(self, d_model: int = 64, nhead: int = 4, num_layers: int = 2):
                super().__init__()
                encoder_layer = nn.TransformerEncoderLayer(d_model, nhead, batch_first=True)
                self.encoder = nn.TransformerEncoder(encoder_layer, num_layers)
                self.pool = nn.AdaptiveAvgPool1d(1)

            def forward(self, x):
                out = self.encoder(x)
                return self.pool(out.transpose(1, 2)).squeeze(-1)
        """),
    "codex": textwrap.dedent("""\
        import pandas as pd
        import torch
        from torch.utils.data import Dataset

        class TextDataset(Dataset):
            def __init__(self, csv_path, tokenizer, max_length=128):
                self.df = pd.read_csv(csv_path)
                self.tokenizer = tokenizer
                self.max_length = max_length

            def __len__(self):
                return len(self.df)

            def __getitem__(self, idx):
                row = self.df.iloc[idx]
                enc = self.tokenizer(row["text"], max_length=self.max_length,
                                     padding="max_length", truncation=True, return_tensors="pt")
                return {k: v.squeeze(0) for k, v in enc.items()}, torch.tensor(row["label"])
        """),
    "opencode": textwrap.dedent("""\
        import torch

        def train_epoch(model, dataloader, optimizer, device):
            model.train()
            total_loss = 0.0
            criterion = torch.nn.CrossEntropyLoss()
            for batch, labels in dataloader:
                batch = {k: v.to(device) for k, v in batch.items()}
                labels = labels.to(device)
                optimizer.zero_grad()
                logits = model(**batch)
                loss = criterion(logits, labels)
                loss.backward()
                optimizer.step()
                total_loss += loss.item()
            return total_loss / len(dataloader)
        """),
}


# ─── Real agent invocation ────────────────────────────────────────────────────

def run_real_agent(agent: str, repo_root: str, timeout: int, debug: bool) -> AgentResult:
    result = AgentResult(agent=agent, ran=True, real=True)
    task = AGENT_TASKS[agent]
    cmd_name = agent_cmd(agent)
    env = {}
    if debug:
        env["AGENTDIFF_DEBUG"] = "1"

    info(f"Invoking {cmd_name} (timeout={timeout}s) …")
    info(f"  Prompt: {task['prompt'][:80]}…")

    cmd: List[str]
    if agent == "claude-code":
        cmd = [
            cmd_name,
            "--dangerously-skip-permissions",
            "-p", task["prompt"],
        ]
    elif agent == "codex":
        cmd = [cmd_name, task["prompt"]]
    elif agent == "opencode":
        cmd = [cmd_name, "run", task["prompt"]]
    else:
        cmd = [cmd_name, task["prompt"]]

    try:
        proc = run(cmd, cwd=repo_root, env=env, timeout=timeout)
        result.exit_code = proc.returncode
        result.stdout = proc.stdout[:2000]
        result.stderr = proc.stderr[:2000]
        if proc.returncode != 0:
            warn(f"{agent} exited {proc.returncode}")
            if proc.stderr:
                warn(f"  stderr: {proc.stderr[:300]}")
        else:
            ok(f"{agent} finished (rc=0)")
    except subprocess.TimeoutExpired:
        result.timed_out = True
        warn(f"{agent} timed out after {timeout}s — checking what was written anyway")

    # Detect which target file (if any) got created
    target = os.path.join(repo_root, task["target_file"])
    if os.path.exists(target):
        result.files_created.append(task["target_file"])
        ok(f"  Created {task['target_file']}")
    else:
        warn(f"  Target file not found: {task['target_file']}")

    return result


# ─── Simulated agent invocation ───────────────────────────────────────────────

def run_simulated_agent(agent: str, repo_root: str, debug: bool) -> AgentResult:
    result = AgentResult(agent=agent, ran=True, real=False)
    task = AGENT_TASKS[agent]
    content = SIMULATED_CONTENT[agent]
    abs_file = os.path.join(repo_root, task["target_file"])

    info(f"[SIMULATE] {agent}: writing {task['target_file']}")

    # Write the file so git diff / prepare-ledger can see it
    os.makedirs(os.path.dirname(abs_file), exist_ok=True)
    with open(abs_file, "w") as f:
        f.write(content)
    result.files_created.append(task["target_file"])
    ok(f"  Wrote {task['target_file']} ({len(content.splitlines())} lines)")

    payload = simulated_payload(agent, repo_root, task["target_file"], content)
    script_name = f"capture-{agent if agent != 'claude-code' else 'claude'}.py"
    # For claude-code in simulation: history.jsonl lookup will miss the fake session_id.
    # Inject AGENTDIFF_PROMPT so the env-var fallback path is exercised instead.
    extra_env = {}
    if agent == "claude-code":
        extra_env["AGENTDIFF_PROMPT"] = task["prompt"][:300]
    success = inject_capture(script_name, payload, repo_root, debug=debug, extra_env=extra_env)
    if success:
        ok(f"  Injected hook payload for {agent}")
    else:
        warn(f"  Hook injection failed for {agent}")

    return result


# ─── Gap analysis ─────────────────────────────────────────────────────────────

def analyze_entries(agent: str, entries: List[SessionEntry]) -> Tuple[List[SessionEntry], List[Tuple[str, str]]]:
    """Return (agent_entries, gap_list).  gaps are (description, fix)."""
    agent_entries = [e for e in entries if e.agent == agent]
    gaps: List[Tuple[str, str]] = []

    if not agent_entries:
        gaps.append((
            f"No session.jsonl entries for agent={agent!r}",
            f"Check that the {agent} hook is configured (agentdiff configure) "
            f"and the capture script at ~/.agentdiff/scripts/capture-{agent.replace('claude-code','claude')}.py fires.",
        ))
        return agent_entries, gaps

    # Take the entry for the target file (or last entry)
    target = AGENT_TASKS.get(agent, {}).get("target_file", "")
    relevant = [e for e in agent_entries if target in e.file] or agent_entries

    e = relevant[-1]

    if not e.model_ok:
        fallback = agent_basename(agent)
        gaps.append((
            f"model={e.model!r} (fallback/unknown) for {agent}",
            {
                "claude-code": (
                    "capture-claude.py reads model from ~/.claude/projects/{slug}/{session_id}.jsonl. "
                    "The hook fires immediately after tool execution — the session JSONL may not have "
                    "flushed the 'assistant' entry yet. Fix: retry the model lookup in a short loop "
                    "(e.g. 3×, 100ms apart) before giving up, or read the model from CLAUDE_MODEL env var."
                ),
                "codex": (
                    "capture-codex.py reads model from the rollout JSONL. "
                    "If the session file hasn't been written, it falls back to 'codex'. "
                    "Fix: also check CODEX_MODEL env var, or read from ~/.codex/sessions/ more aggressively."
                ),
                "opencode": (
                    "capture-opencode.py reads model from payload['model']. "
                    "OpenCode should pass the actual model string in the hook payload. "
                    "Fix: verify the OpenCode hook plugin injects model correctly — check "
                    "~/.config/opencode/plugins/agentdiff.ts and ensure 'modelID' is included."
                ),
            }.get(agent, f"Investigate how {agent} reports its model to the hook."),
        ))

    if not e.prompt_ok:
        gaps.append((
            f"prompt={e.prompt!r} (missing/unknown) for {agent}",
            {
                "claude-code": (
                    "capture-claude.py reads 'last-prompt' from the session JSONL. "
                    "This entry may not exist if the session hasn't written it yet, or if the session "
                    "was not found (slug mismatch). "
                    "Fix: (1) read AGENTDIFF_PROMPT env var as a higher-priority source; "
                    "(2) search all project dirs more broadly; "
                    "(3) retry with backoff on the file read."
                ),
                "codex": (
                    "capture-codex.py extracts prompt from 'last_agent_message' in the task_complete event. "
                    "If missing, it means the event payload didn't include it. "
                    "Fix: also try reading the first user message from the rollout JSONL."
                ),
                "opencode": (
                    "capture-opencode.py reads prompt from payload['prompt']. "
                    "OpenCode's hook plugin may not be forwarding the user prompt. "
                    "Fix: update agentdiff.ts to pass the session's initial user message in the hook payload."
                ),
            }.get(agent, f"Investigate how {agent} forwards user prompts to the hook."),
        ))

    if not e.lines_ok:
        gaps.append((
            f"lines=[] (empty) for {agent}",
            f"capture-{agent.replace('claude-code','claude')}.py failed to compute changed lines. "
            "Check that the file existed on disk when the hook fired.",
        ))

    if not e.file_ok:
        gaps.append((
            f"file={e.file!r} is absolute (should be repo-relative) for {agent}",
            "capture script is writing abs_file to the 'file' field. "
            "Fix: strip repo_root prefix and lstrip('/') before writing the entry.",
        ))

    return agent_entries, gaps


def analyze_traces(traces: List[dict], agent: str) -> List[Tuple[str, str]]:
    """Return gaps found in the trace records for this agent."""
    agent_traces = [
        t for t in traces
        if isinstance(t.get("tool"), dict) and t["tool"].get("name") == agent
    ]
    gaps: List[Tuple[str, str]] = []

    if not agent_traces:
        gaps.append((
            f"No trace entry for {agent} in .git/agentdiff/traces/",
            "prepare-ledger.py may have failed to match session entries to staged files, "
            "or finalize-ledger.py didn't run (check post-commit hook). "
            "Run: AGENTDIFF_DEBUG=1 git commit to see prepare/finalize output.",
        ))
        return gaps

    t = agent_traces[-1]
    files = t.get("files", [])
    if not files:
        gaps.append((
            f"Trace for {agent} has no 'files' entries",
            "prepare-ledger.py produced a pending_ledger with empty lines_map. "
            "Check that git diff --cached showed changes when pre-commit hook ran.",
        ))

    for f in files:
        convs = f.get("conversations", [])
        for conv in convs:
            contrib = conv.get("contributor", {})
            if not contrib.get("model_id"):
                gaps.append((
                    f"Trace contributor for {agent}/{f.get('path','?')} has no model_id",
                    "finalize-ledger.py writes model_id only when the model string is non-empty. "
                    "This is downstream of the session.jsonl model gap — fix that first.",
                ))

    return gaps


# ─── Report ───────────────────────────────────────────────────────────────────

def print_session_entry_detail(e: SessionEntry) -> None:
    print(f"    agent      : {e.agent}")
    print(f"    model      : {BOLD(e.model) if e.model_ok else RED(e.model + ' ⚠')}")
    print(f"    prompt     : {(e.prompt[:100] + '…') if len(e.prompt) > 100 else e.prompt!r}" +
          ("" if e.prompt_ok else f"  {RED('⚠ missing')}"))
    print(f"    file       : {e.file}" + ("" if e.file_ok else f"  {RED('⚠ absolute')}"))
    print(f"    lines      : {len(e.lines)} lines captured" + ("" if e.lines_ok else f"  {RED('⚠ empty')}"))
    print(f"    tool       : {e.tool}")
    print(f"    session_id : {e.session_id}")
    print(f"    timestamp  : {e.timestamp}")


def print_full_report(results: List[AgentResult], trace_gaps: Dict[str, List[Tuple[str, str]]]) -> int:
    total_gaps = 0
    header("COMPREHENSIVE REPORT")

    for r in results:
        print(f"\n  {BOLD(r.agent.upper())}  " +
              (GREEN("[REAL]") if r.real else YELLOW("[SIMULATED]")) +
              (f"  exit={r.exit_code}" if r.exit_code is not None else "") +
              (f"  {RED('TIMEOUT')}" if r.timed_out else ""))

        if r.files_created:
            print(f"    files written : {', '.join(r.files_created)}")

        if not r.entries:
            err("  No session.jsonl entries captured")
        else:
            e = r.entries[-1]
            print(f"    entries in session.jsonl : {len(r.entries)}")
            print_session_entry_detail(e)
            score = r.quality_score
            bar = "█" * score + "░" * (7 - score)
            colour = GREEN if score >= 6 else (YELLOW if score >= 4 else RED)
            print(f"    quality score  : {colour(bar)} {score}/7")

        if r.gaps or trace_gaps.get(r.agent):
            all_gaps = r.gaps + trace_gaps.get(r.agent, [])
            total_gaps += len(all_gaps)
            print(f"\n    {RED(f'{len(all_gaps)} gap(s) found:')}")
            for desc, fix in all_gaps:
                print(f"      {RED('▸')} {desc}")
                for line in textwrap.wrap(fix, width=72):
                    print(f"        {DIM(line)}")
        else:
            ok("  No gaps found")

    return total_gaps


# ─── Main ─────────────────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--simulate-only", action="store_true", help="Use injected payloads, skip real agents")
    parser.add_argument("--debug", action="store_true", help="Enable AGENTDIFF_DEBUG=1")
    parser.add_argument("--keep-dir", action="store_true", help="Don't delete test repo on exit")
    parser.add_argument("--repo", metavar="PATH", help="Use existing repo (must have agentdiff init)")
    parser.add_argument("--timeout", type=int, default=90, metavar="N", help="Per-agent timeout seconds")
    parser.add_argument("--no-commit", action="store_true", help="Skip commit phase")
    parser.add_argument("--agents", default="claude-code,codex,opencode", help="Agents to test (comma-separated)")
    args = parser.parse_args()

    agents_to_test = [a.strip() for a in args.agents.split(",") if a.strip()]

    # ── Setup ─────────────────────────────────────────────────────────────────
    header("SETUP")
    repo_root = setup_test_repo(args.repo)
    repo_str = str(repo_root)
    session_log = repo_root / ".git" / "agentdiff" / "session.jsonl"
    traces_dir = repo_root / ".git" / "agentdiff" / "traces"

    info(f"Repo root  : {repo_root}")
    info(f"Session log: {session_log}")
    info(f"Traces dir : {traces_dir}")
    info(f"Branch     : {current_branch(repo_str)}")
    info(f"Agents     : {', '.join(agents_to_test)}")

    # Snapshot session.jsonl size at test start so we can isolate new entries
    pre_count = len(read_session_entries(session_log))
    info(f"Pre-test session.jsonl entries: {pre_count}")

    # ── Agent invocations ─────────────────────────────────────────────────────
    header("AGENT INVOCATIONS")
    results: List[AgentResult] = []

    for agent in agents_to_test:
        print(f"\n  {BOLD(agent)}")
        task = AGENT_TASKS.get(agent)
        if not task:
            warn(f"No task defined for {agent}, skipping")
            continue

        if args.simulate_only:
            r = run_simulated_agent(agent, repo_str, args.debug)
        elif agent_available(agent):
            r = run_real_agent(agent, repo_str, args.timeout, args.debug)
            # If real agent didn't write the target, fall back to simulation
            if not r.files_created:
                warn(f"{agent} didn't create target file — falling back to simulation for capture")
                sim = run_simulated_agent(agent, repo_str, args.debug)
                r.files_created = sim.files_created
        else:
            warn(f"{agent_cmd(agent)} not found in PATH — using simulation mode")
            r = run_simulated_agent(agent, repo_str, args.debug)

        results.append(r)

    # ── Pre-commit session.jsonl inspection ───────────────────────────────────
    header("PRE-COMMIT SESSION.JSONL ANALYSIS")
    all_entries = read_session_entries(session_log)
    new_entries = all_entries[pre_count:]
    info(f"New entries since test start: {len(new_entries)}")

    if new_entries:
        agents_seen = sorted({e.agent for e in new_entries})
        info(f"Agents in new entries: {', '.join(agents_seen)}")
        print()
        for e in new_entries:
            print(f"  [{e.agent}] file={e.file!r} model={e.model!r} "
                  f"lines={len(e.lines)} prompt={'OK' if e.prompt_ok else 'MISSING'}")
    else:
        warn("No new entries written to session.jsonl — capture hooks may not be firing")
        print()
        info("Debugging hints:")
        info("  1. Run: agentdiff configure --no-copilot  (re-install global hooks)")
        info("  2. Check: cat ~/.agentdiff/logs/capture-claude.log")
        info("  3. Set AGENTDIFF_DEBUG=1 and re-run")

    # Attach entries to results for gap analysis
    for r in results:
        agent_entries, session_gaps = analyze_entries(r.agent, new_entries)
        r.entries = agent_entries
        r.gaps = session_gaps

    # ── Commit phase ──────────────────────────────────────────────────────────
    trace_gaps: Dict[str, List[Tuple[str, str]]] = {}

    if not args.no_commit:
        header("COMMIT PHASE")
        # Stage all new files
        new_files = [r.files_created for r in results]
        staged: List[str] = []
        for r in results:
            for f in r.files_created:
                abs_f = os.path.join(repo_str, f)
                if os.path.exists(abs_f):
                    run(["git", "add", f], cwd=repo_str)
                    staged.append(f)

        if staged:
            info(f"Staged {len(staged)} file(s): {', '.join(staged)}")
            r_commit = run(
                ["git", "commit", "-m",
                 f"test: pipeline test commit [{datetime.now(timezone.utc).isoformat()[:19]}]"],
                cwd=repo_str,
            )
            if r_commit.returncode == 0:
                ok("Committed successfully — prepare-ledger + finalize-ledger hooks should have run")
                sha = run(["git", "rev-parse", "HEAD"], cwd=repo_str).stdout.strip()
                info(f"Commit SHA: {sha[:12]}")
            else:
                warn(f"Commit failed (rc={r_commit.returncode}): {r_commit.stderr.strip()[:200]}")
        else:
            warn("Nothing staged — skipping commit")

        # ── Post-commit trace analysis ─────────────────────────────────────
        header("POST-COMMIT TRACE ANALYSIS")
        branch = current_branch(repo_str)
        traces = read_traces(traces_dir, branch)
        info(f"Traces in .git/agentdiff/traces/{branch.replace('/', '%2F')}.jsonl: {len(traces)}")

        if not traces:
            warn("No traces written. Possible causes:")
            warn("  - prepare-ledger.py hook not installed (run: agentdiff init)")
            warn("  - finalize-ledger.py hook not installed (run: agentdiff init)")
            warn("  - Hooks installed but scripts missing from ~/.agentdiff/scripts/")
            warn(f"  - Check: cat {repo_root}/.git/hooks/pre-commit")
        else:
            for t in traces[-3:]:  # show last 3
                tool_name = t.get("tool", {}).get("name", "?")
                n_files = len(t.get("files", []))
                sha = t.get("vcs", {}).get("revision", "?")[:8]
                print(f"  trace: sha={sha} tool={tool_name!r} files={n_files}")

        for agent in agents_to_test:
            trace_gaps[agent] = analyze_traces(traces, agent)

    # ── Detailed report ───────────────────────────────────────────────────────
    total_gaps = print_full_report(results, trace_gaps)

    # ── Raw session dump ──────────────────────────────────────────────────────
    if args.debug and new_entries:
        header("RAW SESSION ENTRIES (debug)")
        for e in new_entries:
            print(json.dumps(e.raw, indent=2))
            print()

    # ── Summary ───────────────────────────────────────────────────────────────
    header("SUMMARY")
    info(f"Agents tested: {', '.join(agents_to_test)}")
    info(f"New session entries: {len(new_entries)}")
    agents_captured = [r.agent for r in results if r.captured]
    agents_missing = [r.agent for r in results if not r.captured]
    if agents_captured:
        ok(f"Captured: {', '.join(agents_captured)}")
    if agents_missing:
        err(f"Not captured: {', '.join(agents_missing)}")

    if total_gaps == 0:
        ok("ALL CHECKS PASSED — no gaps found")
        print()
        info("Next step: push traces to origin and run `agentdiff report` to see attribution.")
    else:
        print()
        err(f"{total_gaps} gap(s) found across all agents")
        print()
        print(BOLD("ITERATION INSTRUCTIONS FOR NEXT CLAUDE INSTANCE:"))
        print()
        print("  Re-run this test after applying fixes to verify they work:")
        print(f"    python3 scripts/test-pipeline-comprehensive.py \\")
        print(f"      --repo {repo_root} \\")
        print(f"      --simulate-only --debug")
        print()
        print("  After fixing and rebuilding the binary:")
        print("    cargo build --release && cp target/release/agentdiff ~/.local/bin/agentdiff")
        print("    cp scripts/*.py ~/.agentdiff/scripts/")
        print("    # Then re-run the test to verify all gaps are resolved")

    # Cleanup
    if not args.keep_dir and not args.repo:
        shutil.rmtree(str(repo_root), ignore_errors=True)
        info(f"Cleaned up: {repo_root}")
    else:
        info(f"Test repo preserved at: {repo_root}")

    return 0 if total_gaps == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
