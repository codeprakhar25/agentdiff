'use strict';
// agentdiff Copilot VS Code extension
// Managed by agentdiff — do not edit by hand. Regenerate with: agentdiff init
const vscode = require('vscode');
const cp = require('child_process');
const path = require('path');
const os = require('os');
const fs = require('fs');

const CAPTURE_SCRIPT = '__AGENTDIFF_CAPTURE_COPILOT__';

// Minimum insertion length to be considered a Copilot-originated change.
// Must be high enough to avoid capturing human typing, copy-paste, and edits
// from other agents (Claude, Cursor, Codex) that also trigger VS Code's
// onDidChangeTextDocument events.  50 chars catches multi-line Copilot
// completions while filtering out most false positives.
const MIN_COPILOT_CHANGE_LEN = 50;

// Paths that should never be attributed to Copilot (auto-generated metadata).
const EXCLUDED_PATHS = ['.agentdiff/', '.git/'];

// ── Capture modes ────────────────────────────────────────────────────────────
//
// RELIABLE (confidence = "high"):
//   "manual"       — agentdiff.captureNow command: user explicitly marks the
//                    current file as Copilot-authored after a Chat session.
//                    All lines in the file are captured.  This is the only
//                    mode that produces deterministic, reproducible results.
//
// HEURISTIC / UNSUPPORTED (confidence = "low"):
//   "inline_heuristic" — onDidChangeTextDocument fires on EVERY text change in
//                    VS Code, including edits from Claude Code running in the
//                    terminal, Cursor, human typing, and copy-paste.  A length
//                    threshold (MIN_COPILOT_CHANGE_LEN) reduces false positives
//                    but cannot eliminate them.  Do NOT use this mode as a
//                    reliable source of attribution; treat it as a hint only.
//   "save_flush"   — Same heuristic events, flushed on file save rather than
//                    on the debounce timer.  Same caveats apply.
//   "chat_edit"    — Reserved for future use when a VS Code API for detecting
//                    Copilot Chat edits becomes available.  Not currently
//                    triggered by this extension.
//
// These limitations exist because VS Code does not expose a stable public API
// that identifies the source of a document edit as Copilot vs. human vs. other
// agent.  The VS Code team is tracking this at:
//   https://github.com/microsoft/vscode/issues/XXXXX  (placeholder)

// A single stable session ID generated once per window activation.
// Using Date.now() + random suffix gives a unique-enough ID that is consistent
// across all capture events in the same VS Code window, making it possible to
// group them into one session rather than treating each event as isolated.
const WINDOW_SESSION_ID = `vscode-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

function isDebug() {
    const v = process.env.AGENTDIFF_DEBUG || '';
    return v === '1' || v.toLowerCase() === 'true' || v.toLowerCase() === 'yes';
}

function debugLog(msg) {
    if (!isDebug()) return;
    try {
        const logDir = path.join(os.homedir(), '.agentdiff', 'logs');
        fs.mkdirSync(logDir, { recursive: true });
        const ts = new Date().toISOString();
        fs.appendFileSync(path.join(logDir, 'capture-copilot-ext.log'), `${ts} ${msg}\n`);
    } catch (_) {}
}

function findRepoRoot(filePath) {
    return new Promise((resolve) => {
        const dir = path.dirname(filePath);
        cp.exec('git rev-parse --show-toplevel', { cwd: dir }, (err, stdout) => {
            resolve(err ? null : stdout.trim());
        });
    });
}

async function getCopilotModel() {
    try {
        const models = await vscode.lm.selectChatModels({ vendor: 'copilot' });
        if (models && models.length > 0) {
            return models[0].id;
        }
    } catch (_) {}
    return 'copilot';
}

function fireCapture(payload) {
    if (!fs.existsSync(CAPTURE_SCRIPT)) {
        debugLog(`capture script not found: ${CAPTURE_SCRIPT}`);
        return;
    }
    const python = process.platform === 'win32' ? 'python' : 'python3';
    const proc = cp.spawn(python, [CAPTURE_SCRIPT], { stdio: ['pipe', 'ignore', 'ignore'] });
    proc.stdin.write(JSON.stringify(payload));
    proc.stdin.end();
    proc.on('error', (err) => debugLog(`spawn error: ${err.message}`));
    debugLog(
        `fired capture: file=${payload.file_path} lines=${JSON.stringify(payload.lines)} ` +
        `confidence=${payload.confidence} capture_mode=${payload.capture_mode}`
    );
}

async function captureFile(filePath, pending) {
    const repoRoot = await findRepoRoot(filePath);
    const cwd = repoRoot || path.dirname(filePath);
    fireCapture({
        event: pending.tool,
        cwd,
        file_path: filePath,
        model: await getCopilotModel(),
        session_id: WINDOW_SESSION_ID,
        prompt: null,
        lines: Array.from(pending.lines).sort((a, b) => a - b),
        confidence: pending.confidence,
        capture_mode: pending.capture_mode,
    });
}

function activate(context) {
    const copilotExt =
        vscode.extensions.getExtension('GitHub.copilot') ||
        vscode.extensions.getExtension('GitHub.copilot-chat');

    if (!copilotExt) {
        debugLog('GitHub Copilot extension not found — agentdiff Copilot capture inactive');
        return;
    }

    debugLog(`agentdiff Copilot extension activated (session=${WINDOW_SESSION_ID})`);

    // pendingChanges: filePath -> { lines: Set<number>, tool: string, confidence: string, capture_mode: string }
    const pendingChanges = new Map();
    let flushTimer;

    async function flushAll() {
        for (const [filePath, pending] of pendingChanges) {
            if (pending.lines.size > 0) {
                await captureFile(filePath, pending);
            }
        }
        pendingChanges.clear();
    }

    // Track document changes and attribute "large" insertions to Copilot.
    // NOTE: This is a HEURISTIC — any sufficiently large insertion triggers
    // capture regardless of actual source.  confidence="low", capture_mode="inline_heuristic".
    const changeDisposable = vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document.uri.scheme !== 'file') return;
        if (!copilotExt.isActive) return;

        const filePath = event.document.uri.fsPath;

        // Skip metadata paths that are auto-generated.
        const relPath = vscode.workspace.asRelativePath(filePath, false);
        if (EXCLUDED_PATHS.some((p) => relPath.startsWith(p))) return;
        const pending = pendingChanges.get(filePath) || {
            lines: new Set(),
            tool: 'inline',
            confidence: 'low',
            capture_mode: 'inline_heuristic',
        };
        let changed = false;

        for (const change of event.contentChanges) {
            const insertedLen = change.text.length;
            const insertedLineCount = change.text.split('\n').length;
            // Treat as Copilot if multi-line insertion or single-line >= threshold
            if (insertedLen >= MIN_COPILOT_CHANGE_LEN || insertedLineCount > 1) {
                const startLine = change.range.start.line + 1; // 1-based
                for (let l = 0; l < insertedLineCount; l++) {
                    pending.lines.add(startLine + l);
                }
                changed = true;
            }
        }

        if (!changed) return;

        pendingChanges.set(filePath, pending);
        if (flushTimer) clearTimeout(flushTimer);
        flushTimer = setTimeout(flushAll, 2000);
    });

    // On save, flush pending changes for that file immediately.
    // confidence="low", capture_mode="save_flush" — same heuristic as inline,
    // just triggered earlier (on save rather than debounce timer).
    const saveDisposable = vscode.workspace.onDidSaveTextDocument(async (doc) => {
        if (doc.uri.scheme !== 'file') return;
        const filePath = doc.uri.fsPath;
        const pending = pendingChanges.get(filePath);
        if (!pending || pending.lines.size === 0) return;
        await captureFile(filePath, {
            lines: pending.lines,
            tool: 'save',
            confidence: 'low',
            capture_mode: 'save_flush',
        });
        pendingChanges.delete(filePath);
    });

    // Command: manually record all lines of the current file as Copilot-authored.
    // Useful after a Copilot Chat session that generated a whole file.
    // This is the ONLY reliable (confidence="high") capture mode.
    const captureCmd = vscode.commands.registerCommand('agentdiff.captureNow', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showInformationMessage('agentdiff: No active editor');
            return;
        }
        const filePath = editor.document.uri.fsPath;
        const lines = new Set();
        for (let i = 1; i <= editor.document.lineCount; i++) lines.add(i);
        await captureFile(filePath, {
            lines,
            tool: 'manual',
            confidence: 'high',
            capture_mode: 'manual',
        });
        vscode.window.showInformationMessage('agentdiff: Copilot capture recorded');
    });

    context.subscriptions.push(changeDisposable, saveDisposable, captureCmd);
}

function deactivate() {}

module.exports = { activate, deactivate, _WINDOW_SESSION_ID: WINDOW_SESSION_ID };
