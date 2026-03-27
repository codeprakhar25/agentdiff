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
// Short single-char or very short insertions are treated as manual typing.
const MIN_COPILOT_CHANGE_LEN = 10;

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

function getCopilotModel() {
    // Copilot doesn't publicly expose the active model name.
    // Try to read it from known config keys; fall back to a sensible default.
    try {
        const cfg = vscode.workspace.getConfiguration('github.copilot');
        const advanced = cfg.get('advanced');
        if (advanced && typeof advanced === 'object' && advanced['engine']) {
            return String(advanced['engine']);
        }
        // Copilot Chat >= 1.0 uses GPT-4o by default
        const chatExt = vscode.extensions.getExtension('GitHub.copilot-chat');
        if (chatExt) return 'gpt-4o';
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
    debugLog(`fired capture: file=${payload.file_path} lines=${JSON.stringify(payload.lines)}`);
}

async function captureFile(filePath, pending) {
    const repoRoot = await findRepoRoot(filePath);
    const cwd = repoRoot || path.dirname(filePath);
    fireCapture({
        event: pending.tool,
        cwd,
        file_path: filePath,
        model: getCopilotModel(),
        session_id: `vscode-${Date.now()}`,
        prompt: null,
        lines: Array.from(pending.lines).sort((a, b) => a - b),
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

    debugLog('agentdiff Copilot extension activated');

    // pendingChanges: filePath -> { lines: Set<number>, tool: string }
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
    const changeDisposable = vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document.uri.scheme !== 'file') return;
        if (!copilotExt.isActive) return;

        const filePath = event.document.uri.fsPath;
        const pending = pendingChanges.get(filePath) || { lines: new Set(), tool: 'inline' };
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
    const saveDisposable = vscode.workspace.onDidSaveTextDocument(async (doc) => {
        if (doc.uri.scheme !== 'file') return;
        const filePath = doc.uri.fsPath;
        const pending = pendingChanges.get(filePath);
        if (!pending || pending.lines.size === 0) return;
        await captureFile(filePath, { lines: pending.lines, tool: 'save' });
        pendingChanges.delete(filePath);
    });

    // Command: manually record all lines of the current file as Copilot-authored.
    // Useful after a Copilot Chat session that generated a whole file.
    const captureCmd = vscode.commands.registerCommand('agentdiff.captureNow', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showInformationMessage('agentdiff: No active editor');
            return;
        }
        const filePath = editor.document.uri.fsPath;
        const lines = new Set();
        for (let i = 1; i <= editor.document.lineCount; i++) lines.add(i);
        await captureFile(filePath, { lines, tool: 'manual' });
        vscode.window.showInformationMessage('agentdiff: Copilot capture recorded');
    });

    context.subscriptions.push(changeDisposable, saveDisposable, captureCmd);
}

function deactivate() {}

module.exports = { activate, deactivate };
