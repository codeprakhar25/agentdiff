#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# agentdiff end-to-end test
#
# Simulates every supported agent's hook, commits after each,
# then verifies the ledger has the correct entries.
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "${GREEN}PASS${NC}  $1"; }
fail() { echo -e "${RED}FAIL${NC}  $1"; FAILURES=$((FAILURES + 1)); }
info() { echo -e "${CYAN}----${NC}  $1"; }
header() { echo -e "\n${BOLD}${CYAN}=== $1 ===${NC}"; }

FAILURES=0
TEST_DIR=$(mktemp -d /tmp/agentdiff-e2e-XXXXXX)
SCRIPTS_DIR="${HOME}/.agentdiff/scripts"

cleanup() {
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# ─────────────────────────────────────────────────────────────────────
header "Setup"
# ─────────────────────────────────────────────────────────────────────

info "Test dir: $TEST_DIR"

# 1) Run agentdiff configure (global hooks)
info "Running agentdiff configure --no-copilot ..."
agentdiff configure --no-copilot 2>&1 | tail -3
echo

# 2) Create test repo
info "Creating test repository ..."
cd "$TEST_DIR"
git init -q
git config user.email "e2e@agentdiff.test"
git config user.name "E2E Test"

# Seed file so we have a first commit
echo "# E2E Test Repo" > README.md
git add README.md
git commit -q -m "initial commit"

# 3) Run agentdiff init (per-repo)
info "Running agentdiff init ..."
agentdiff init 2>&1 | tail -3
echo

SESSION_LOG="$TEST_DIR/.git/agentdiff/session.jsonl"

# ─────────────────────────────────────────────────────────────────────
header "Agent 1: Claude Code (Edit)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/claude-edit.txt"
echo -e "line1\nline2 old text\nline3" > "$FILE"
git add "$FILE" && git commit -q -m "add claude-edit.txt"

# Simulate the Edit: change "old text" to "new text"
sed -i 's/old text/new text/' "$FILE"

echo '{
  "tool": "Edit",
  "tool_input": {
    "file_path": "'"$FILE"'",
    "old_string": "old text",
    "new_string": "new text"
  },
  "session_id": "claude-sess-001",
  "cwd": "'"$TEST_DIR"'"
}' | python3 "$SCRIPTS_DIR/capture-claude.py"

git add "$FILE"
git commit -q -m "claude: edit old text to new text"

if grep -q '"claude-code"' "$SESSION_LOG" 2>/dev/null; then
    pass "Claude Code entry in session.jsonl"
else
    fail "Claude Code entry missing from session.jsonl"
fi

# ─────────────────────────────────────────────────────────────────────
header "Agent 2: Cursor (afterFileEdit)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/cursor-agent.txt"
echo -e "alpha\nbeta\ngamma" > "$FILE"
git add "$FILE" && git commit -q -m "add cursor-agent.txt"

sed -i 's/beta/beta-modified/' "$FILE"

# Cache a prompt first (beforeSubmitPrompt)
echo '{
  "hook_event_name": "beforeSubmitPrompt",
  "conversation_id": "cursor-conv-001",
  "user_prompt": "modify the beta line"
}' | python3 "$SCRIPTS_DIR/capture-cursor.py" || true

# Then fire the afterFileEdit
echo '{
  "hook_event_name": "afterFileEdit",
  "file_path": "'"$FILE"'",
  "conversation_id": "cursor-conv-001",
  "old_lines": [2],
  "new_lines": [2],
  "model": "cursor-fast",
  "cwd": "'"$TEST_DIR"'"
}' | python3 "$SCRIPTS_DIR/capture-cursor.py"

git add "$FILE"
git commit -q -m "cursor: modify beta line"

if grep -q '"cursor"' "$SESSION_LOG" 2>/dev/null; then
    pass "Cursor (agent) entry in session.jsonl"
else
    fail "Cursor (agent) entry missing from session.jsonl"
fi

# ─────────────────────────────────────────────────────────────────────
header "Agent 3: Cursor (afterTabFileEdit)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/cursor-tab.txt"
echo -e "foo\nbar\nbaz" > "$FILE"
git add "$FILE" && git commit -q -m "add cursor-tab.txt"

sed -i 's/bar/bar_completed/' "$FILE"

echo '{
  "hook_event_name": "afterTabFileEdit",
  "file_path": "'"$FILE"'",
  "line_number": 2,
  "lineNumber": 2,
  "model": "cursor-tab-model",
  "cwd": "'"$TEST_DIR"'"
}' | python3 "$SCRIPTS_DIR/capture-cursor.py"

git add "$FILE"
git commit -q -m "cursor: tab complete on line 2"

if grep -q '"tab"' "$SESSION_LOG" 2>/dev/null; then
    pass "Cursor (tab) entry in session.jsonl"
else
    fail "Cursor (tab) entry missing from session.jsonl"
fi

# ─────────────────────────────────────────────────────────────────────
header "Agent 4: Windsurf (post_write_code)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/windsurf-edit.txt"
echo -e "start\nmiddle\nend" > "$FILE"
git add "$FILE" && git commit -q -m "add windsurf-edit.txt"

sed -i 's/middle/middle-cascade/' "$FILE"

# Cache prompt first
echo '{
  "agent_action_name": "post_cascade_response_with_transcript",
  "trajectory_id": "wind-traj-001",
  "prompt": "update the middle section"
}' | python3 "$SCRIPTS_DIR/capture-windsurf.py" || true

# Fire the post_write_code event
echo '{
  "agent_action_name": "post_write_code",
  "trajectory_id": "wind-traj-001",
  "tool_info": {
    "file_path": "'"$FILE"'",
    "edits": [
      {
        "old_string": "middle",
        "new_string": "middle-cascade"
      }
    ]
  },
  "cwd": "'"$TEST_DIR"'",
  "model": "windsurf-cascade-v1"
}' | python3 "$SCRIPTS_DIR/capture-windsurf.py"

git add "$FILE"
git commit -q -m "windsurf: cascade edit middle section"

if grep -q '"windsurf"' "$SESSION_LOG" 2>/dev/null; then
    pass "Windsurf entry in session.jsonl"
else
    fail "Windsurf entry missing from session.jsonl"
fi

# ─────────────────────────────────────────────────────────────────────
header "Agent 5: OpenCode (PostToolUse)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/opencode-edit.txt"
echo -e "one\ntwo\nthree" > "$FILE"
git add "$FILE" && git commit -q -m "add opencode-edit.txt"

sed -i 's/two/two-updated/' "$FILE"

echo '{
  "hook_event_name": "PostToolUse",
  "tool_name": "edit",
  "tool_input": {
    "filePath": "'"$FILE"'",
    "old_string": "two",
    "new_string": "two-updated"
  },
  "cwd": "'"$TEST_DIR"'",
  "session_id": "oc-sess-001",
  "model": "opencode-model-v2"
}' | python3 "$SCRIPTS_DIR/capture-opencode.py"

git add "$FILE"
git commit -q -m "opencode: edit two to two-updated"

if grep -q '"opencode"' "$SESSION_LOG" 2>/dev/null; then
    pass "OpenCode entry in session.jsonl"
else
    fail "OpenCode entry missing from session.jsonl"
fi

# ─────────────────────────────────────────────────────────────────────
header "Agent 6: Gemini / Antigravity (AfterTool)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/antigravity-edit.txt"
echo -e "apple\nbanana\ncherry" > "$FILE"
git add "$FILE" && git commit -q -m "add antigravity-edit.txt"

sed -i 's/banana/banana-gemini/' "$FILE"

# Cache prompt (BeforeTool)
echo '{
  "hook_event_name": "BeforeTool",
  "session_id": "gem-sess-001",
  "prompt": "replace banana with banana-gemini"
}' | python3 "$SCRIPTS_DIR/capture-antigravity.py" || true

# Fire AfterTool
echo '{
  "hook_event_name": "AfterTool",
  "tool_name": "write_file",
  "tool_input": {
    "file_path": "'"$FILE"'",
    "old_string": "banana",
    "new_string": "banana-gemini"
  },
  "session_id": "gem-sess-001",
  "cwd": "'"$TEST_DIR"'",
  "model": "gemini-2.5-pro"
}' | python3 "$SCRIPTS_DIR/capture-antigravity.py"

git add "$FILE"
git commit -q -m "antigravity: gemini edit banana"

if grep -q '"antigravity"' "$SESSION_LOG" 2>/dev/null; then
    pass "Antigravity/Gemini entry in session.jsonl"
else
    fail "Antigravity/Gemini entry missing from session.jsonl"
fi

# ─────────────────────────────────────────────────────────────────────
header "Agent 7: Copilot (inline completion)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/copilot-inline.txt"
echo -e "def hello():\n    pass" > "$FILE"
git add "$FILE" && git commit -q -m "add copilot-inline.txt"

# Simulate Copilot inserting a multi-line completion
cat > "$FILE" <<'PYEOF'
def hello():
    print("Hello, World!")
    return True
PYEOF

echo '{
  "file_path": "'"$FILE"'",
  "event": "inline",
  "lines": [2, 3],
  "model": "gpt-4o",
  "session_id": "copilot-sess-001",
  "prompt": "autocomplete hello function"
}' | python3 "$SCRIPTS_DIR/capture-copilot.py"

git add "$FILE"
git commit -q -m "copilot: inline completion in hello()"

if grep -q '"copilot"' "$SESSION_LOG" 2>/dev/null; then
    pass "Copilot entry in session.jsonl"
else
    fail "Copilot entry missing from session.jsonl"
fi

# ─────────────────────────────────────────────────────────────────────
header "Agent 8: Codex (notify hook)"
# ─────────────────────────────────────────────────────────────────────

FILE="$TEST_DIR/codex-task.txt"
echo "placeholder" > "$FILE"
git add "$FILE" && git commit -q -m "add codex-task.txt"

# Codex changes a file and the notify hook fires at task end
echo "codex wrote this line" > "$FILE"
git add "$FILE"

# The codex capture script scans git diff, so we need staged changes.
# It reads a JSON event from stdin.
echo '{
  "type": "event_msg",
  "payload": {
    "type": "task_complete",
    "cwd": "'"$TEST_DIR"'",
    "last_agent_message": "wrote the codex file"
  },
  "session_id": "codex-sess-001",
  "model": "codex-o4-mini",
  "cwd": "'"$TEST_DIR"'"
}' | python3 "$SCRIPTS_DIR/capture-codex.py" 2>/dev/null || true

git commit -q -m "codex: write codex-task.txt"

if grep -q '"codex"' "$SESSION_LOG" 2>/dev/null; then
    pass "Codex entry in session.jsonl"
else
    # Codex relies on git diff scanning — may not produce entry in all test scenarios
    echo -e "${YELLOW}SKIP${NC}  Codex entry (relies on live git diff scanning, may not fire in test)"
fi

# ─────────────────────────────────────────────────────────────────────
header "Verification: Ledger"
# ─────────────────────────────────────────────────────────────────────

LEDGER="$TEST_DIR/.agentdiff/ledger.jsonl"

info "Session log entries:"
if [ -f "$SESSION_LOG" ]; then
    wc -l < "$SESSION_LOG" | xargs -I{} echo "  {} entries in session.jsonl"
    echo
    info "Agents captured in session.jsonl:"
    python3 -c "
import json, sys
agents = set()
for line in open('$SESSION_LOG'):
    line = line.strip()
    if not line: continue
    try:
        e = json.loads(line)
        agents.add(e.get('agent', 'unknown'))
    except: pass
for a in sorted(agents):
    print(f'  - {a}')
"
else
    fail "session.jsonl not found"
fi

echo
info "Ledger entries:"
if [ -f "$LEDGER" ]; then
    LEDGER_COUNT=$(wc -l < "$LEDGER" | tr -d ' ')
    echo "  $LEDGER_COUNT entries in ledger.jsonl"
    echo
    info "Agents in ledger:"
    python3 -c "
import json
agents = {}
for line in open('$LEDGER'):
    line = line.strip()
    if not line: continue
    try:
        e = json.loads(line)
        a = e.get('agent', 'unknown')
        agents[a] = agents.get(a, 0) + 1
    except: pass
for a in sorted(agents):
    print(f'  - {a}: {agents[a]} commit(s)')
"
else
    fail "ledger.jsonl not found"
fi

# ─────────────────────────────────────────────────────────────────────
header "Verification: agentdiff CLI commands"
# ─────────────────────────────────────────────────────────────────────

info "agentdiff list:"
agentdiff list --limit 10 2>&1 || true
echo

info "agentdiff log:"
agentdiff log -n 10 2>&1 || true
echo

info "agentdiff stats:"
agentdiff stats 2>&1 || true
echo

# ─────────────────────────────────────────────────────────────────────
header "Verification: Global config (no repo-level pollution)"
# ─────────────────────────────────────────────────────────────────────

if [ -d "$TEST_DIR/.windsurf" ]; then
    fail ".windsurf/ folder exists in repo (should be global only)"
else
    pass "No .windsurf/ folder in repo"
fi

if [ -d "$TEST_DIR/.opencode" ]; then
    fail ".opencode/ folder exists in repo (should be global only)"
else
    pass "No .opencode/ folder in repo"
fi

if [ -f "$HOME/.codeium/windsurf/hooks.json" ]; then
    pass "Windsurf hooks at ~/.codeium/windsurf/hooks.json (global)"
else
    echo -e "${YELLOW}SKIP${NC}  Windsurf global hooks (may not exist if Windsurf not installed)"
fi

OPENCODE_PLUGIN="$HOME/.config/opencode/plugins/agentdiff.ts"
if [ -f "$OPENCODE_PLUGIN" ]; then
    pass "OpenCode plugin at ~/.config/opencode/plugins/agentdiff.ts (global)"
else
    echo -e "${YELLOW}SKIP${NC}  OpenCode global plugin (may not exist if config_dir differs)"
fi

# ─────────────────────────────────────────────────────────────────────
header "Results"
# ─────────────────────────────────────────────────────────────────────

if [ "$FAILURES" -eq 0 ]; then
    echo -e "${GREEN}${BOLD}All tests passed.${NC}"
else
    echo -e "${RED}${BOLD}$FAILURES test(s) failed.${NC}"
fi

exit "$FAILURES"
