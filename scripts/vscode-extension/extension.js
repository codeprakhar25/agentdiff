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
// onDidChangeTextDocument events. 50 chars catches multi-line Copilot
// completions while filtering out most false positives.
const MIN_COPILOT_CHANGE_LEN = 50;
const DEFAULT_FLUSH_DELAY_MS = 2000;
const FLUSH_DELAY_MS = Number(process.env.AGENTDIFF_EXT_FLUSH_DELAY_MS || DEFAULT_FLUSH_DELAY_MS);

// Paths that should never be attributed to Copilot (auto-generated metadata).
const EXCLUDED_PATHS = ['.agentdiff/', '.git/'];

let outputChannel = null;

function isDebug() {
    const v = process.env.AGENTDIFF_DEBUG || '';
    return ['1', 'true', 'yes', 'on'].includes(v.toLowerCase());
}

function getOutputChannel() {
    if (!outputChannel && vscode.window && typeof vscode.window.createOutputChannel === 'function') {
        outputChannel = vscode.window.createOutputChannel('AgentDiff');
    }
    return outputChannel;
}

function writeFileLog(line, force = false) {
    if (!force && !isDebug()) return;
    try {
        const logDir = path.join(os.homedir(), '.agentdiff', 'logs');
        fs.mkdirSync(logDir, { recursive: true });
        fs.appendFileSync(path.join(logDir, 'capture-copilot-ext.log'), `${line}\n`);
    } catch (_) {}
}

function logEvent(level, event, fields = {}) {
    const entry = {
        ts: new Date().toISOString(),
        level,
        component: 'vscode-copilot-extension',
        event,
        ...fields,
    };
    const line = JSON.stringify(entry);
    const channel = getOutputChannel();
    if (channel && (level !== 'debug' || isDebug())) {
        channel.appendLine(line);
    }
    writeFileLog(line, level !== 'debug');
}

function normalizeSlashes(value) {
    return String(value || '').replace(/\\/g, '/');
}

function isExcludedRelativePath(relPath) {
    const normalized = normalizeSlashes(relPath).replace(/^\/+/, '');
    return EXCLUDED_PATHS.some((excluded) => normalized === excluded.slice(0, -1) || normalized.startsWith(excluded));
}

function isPathInside(child, parent) {
    if (!child || !parent) return false;
    const relative = path.relative(parent, child);
    return relative === '' || (!!relative && !relative.startsWith('..') && !path.isAbsolute(relative));
}

function getWorkspaceFolder(uri) {
    if (vscode.workspace && typeof vscode.workspace.getWorkspaceFolder === 'function') {
        const folder = vscode.workspace.getWorkspaceFolder(uri);
        if (folder) return folder;
    }

    const folders = (vscode.workspace && vscode.workspace.workspaceFolders) || [];
    return folders.find((folder) => isPathInside(uri.fsPath, folder.uri.fsPath)) || null;
}

function getRelativePath(uri) {
    const folder = getWorkspaceFolder(uri);
    if (folder) {
        return normalizeSlashes(path.relative(folder.uri.fsPath, uri.fsPath));
    }
    if (vscode.workspace && typeof vscode.workspace.asRelativePath === 'function') {
        return normalizeSlashes(vscode.workspace.asRelativePath(uri, false));
    }
    return normalizeSlashes(uri.fsPath);
}

function execGit(args, cwd) {
    return new Promise((resolve) => {
        const useWsl = looksLikeWslScript(CAPTURE_SCRIPT);
        const command = useWsl ? 'wsl.exe' : 'git';
        const commandArgs = useWsl ? ['--cd', windowsPathToWsl(cwd), '-e', 'git', ...args] : args;
        const options = useWsl ? {} : { cwd };

        cp.execFile(command, commandArgs, options, (err, stdout, stderr) => {
            if (err) {
                logEvent('debug', 'git.rev_parse.failed', {
                    cwd,
                    args,
                    error: err.message,
                    stderr: String(stderr || '').trim(),
                });
                resolve(null);
                return;
            }
            resolve(String(stdout || '').trim() || null);
        });
    });
}

function pathExistsDir(dirPath) {
    if (!dirPath) return Promise.resolve(false);
    if (!looksLikeWslScript(CAPTURE_SCRIPT)) {
        return Promise.resolve(fs.existsSync(dirPath));
    }
    return new Promise((resolve) => {
        cp.execFile('wsl.exe', ['-e', 'test', '-d', dirPath], {}, (err) => {
            resolve(!err);
        });
    });
}

function resolveGitDir(gitDir, cwd) {
    if (!gitDir) return null;
    if (looksLikeWslScript(CAPTURE_SCRIPT)) {
        return gitDir.startsWith('/') ? gitDir : path.posix.resolve(windowsPathToWsl(cwd), gitDir);
    }
    return path.isAbsolute(gitDir) ? gitDir : path.resolve(cwd, gitDir);
}

async function findRepoInfo(uri) {
    const filePath = uri.fsPath;
    const workspaceFolder = getWorkspaceFolder(uri);
    const startDir = path.dirname(filePath);
    const repoRoot = await execGit(['rev-parse', '--show-toplevel'], startDir);
    const gitDirRaw = await execGit(['rev-parse', '--git-common-dir'], startDir);
    const gitDir = resolveGitDir(gitDirRaw, startDir);
    const agentdiffDir = gitDir
        ? (looksLikeWslScript(CAPTURE_SCRIPT) ? path.posix.join(gitDir, 'agentdiff') : path.join(gitDir, 'agentdiff'))
        : null;
    const initialized = await pathExistsDir(agentdiffDir);

    return {
        cwd: repoRoot || (workspaceFolder && workspaceFolder.uri.fsPath) || startDir,
        repoRoot,
        gitDir,
        agentdiffDir,
        initialized,
        workspaceFolder: workspaceFolder ? workspaceFolder.uri.fsPath : null,
    };
}

async function getCopilotModel() {
    try {
        if (!vscode.lm || typeof vscode.lm.selectChatModels !== 'function') {
            return 'copilot';
        }
        const models = await vscode.lm.selectChatModels({ vendor: 'copilot' });
        if (models && models.length > 0) {
            return models[0].id;
        }
    } catch (err) {
        logEvent('debug', 'copilot.model.lookup_failed', { error: err.message });
    }
    return 'copilot';
}

function looksLikeWslScript(scriptPath) {
    return process.platform === 'win32' && /^\/(home|mnt|opt|usr|var|tmp)\//.test(scriptPath);
}

function windowsPathToWsl(value) {
    if (!value) return value;
    const normalized = normalizeSlashes(value);
    const drive = normalized.match(/^([A-Za-z]):\/(.*)$/);
    if (drive) {
        return `/mnt/${drive[1].toLowerCase()}/${drive[2]}`;
    }
    const unc = normalized.match(/^\/\/wsl(?:\.localhost|\$)\/[^/]+(\/.*)$/i);
    if (unc) {
        return unc[1];
    }
    return value;
}

function buildCaptureProcess(payload) {
    if (looksLikeWslScript(CAPTURE_SCRIPT)) {
        const translatedPayload = { ...payload };
        for (const key of ['cwd', 'file_path', 'repo_root', 'workspace_folder']) {
            translatedPayload[key] = windowsPathToWsl(translatedPayload[key]);
        }
        return {
            command: 'wsl.exe',
            args: ['-e', 'python3', CAPTURE_SCRIPT],
            payload: translatedPayload,
            allowMissingScript: true,
        };
    }

    const python = process.platform === 'win32' ? 'python' : 'python3';
    return {
        command: python,
        args: [CAPTURE_SCRIPT],
        payload,
        allowMissingScript: false,
    };
}

function fireCapture(payload) {
    const procSpec = buildCaptureProcess(payload);
    if (!procSpec.allowMissingScript && !fs.existsSync(CAPTURE_SCRIPT)) {
        logEvent('error', 'capture.script_missing', { captureScript: CAPTURE_SCRIPT });
        return false;
    }

    const proc = cp.spawn(procSpec.command, procSpec.args, { stdio: ['pipe', 'ignore', 'pipe'] });
    proc.stdin.write(JSON.stringify(procSpec.payload));
    proc.stdin.end();
    proc.stderr.on('data', (chunk) => {
        logEvent('error', 'capture.stderr', { message: String(chunk).trim() });
    });
    proc.on('error', (err) => logEvent('error', 'capture.spawn_failed', { error: err.message }));
    logEvent('debug', 'capture.spawned', {
        command: procSpec.command,
        args: procSpec.args,
        filePath: procSpec.payload.file_path,
        lines: procSpec.payload.lines,
        repoRoot: procSpec.payload.repo_root,
    });
    return true;
}

async function captureDocument(document, pending) {
    const uri = document.uri;
    const filePath = uri.fsPath;
    const repoInfo = await findRepoInfo(uri);

    if (!repoInfo.initialized) {
        logEvent('warn', 'capture.skipped_repo_not_initialized', {
            filePath,
            repoRoot: repoInfo.repoRoot,
            gitDir: repoInfo.gitDir,
            workspaceFolder: repoInfo.workspaceFolder,
        });
        return false;
    }

    const payload = {
        event: pending.tool,
        cwd: repoInfo.cwd,
        file_path: filePath,
        repo_root: repoInfo.repoRoot,
        workspace_folder: repoInfo.workspaceFolder,
        document_version: pending.documentVersion || document.version || null,
        captured_at: new Date().toISOString(),
        changed_at: pending.changedAt || null,
        model: await getCopilotModel(),
        session_id: `vscode-${Date.now()}`,
        prompt: null,
        lines: Array.from(pending.lines).sort((a, b) => a - b),
    };

    return fireCapture(payload);
}

function activate(context) {
    const channel = getOutputChannel();
    const copilotExt =
        vscode.extensions.getExtension('GitHub.copilot') ||
        vscode.extensions.getExtension('GitHub.copilot-chat');

    if (!copilotExt) {
        logEvent('warn', 'extension.inactive_copilot_missing');
        return;
    }

    logEvent('info', 'extension.activated', {
        copilotActive: !!copilotExt.isActive,
        workspaceFolders: ((vscode.workspace && vscode.workspace.workspaceFolders) || []).map((f) => f.uri.fsPath),
    });

    // pendingChanges: filePath -> { document, lines, tool, documentVersion, changedAt }
    const pendingChanges = new Map();
    let flushTimer;

    async function flushAll() {
        flushTimer = null;
        const entries = Array.from(pendingChanges.entries());
        pendingChanges.clear();
        for (const [, pending] of entries) {
            if (pending.lines.size > 0) {
                await captureDocument(pending.document, pending);
            }
        }
    }

    // Track document changes and attribute "large" insertions to Copilot.
    const changeDisposable = vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document.uri.scheme !== 'file') {
            logEvent('debug', 'change.ignored_non_file', { scheme: event.document.uri.scheme });
            return;
        }
        if (!copilotExt.isActive) {
            logEvent('debug', 'change.ignored_copilot_inactive');
            return;
        }

        const filePath = event.document.uri.fsPath;
        const relPath = getRelativePath(event.document.uri);
        if (isExcludedRelativePath(relPath)) {
            logEvent('debug', 'change.ignored_excluded_path', { filePath, relPath });
            return;
        }

        const pending = pendingChanges.get(filePath) || {
            document: event.document,
            lines: new Set(),
            tool: 'inline',
        };
        pending.document = event.document;
        pending.documentVersion = event.document.version || null;
        pending.changedAt = new Date().toISOString();
        let changed = false;

        for (const change of event.contentChanges) {
            const insertedLen = change.text.length;
            const insertedLineCount = change.text.split('\n').length;
            // Treat as Copilot if multi-line insertion or single-line >= threshold.
            if (insertedLen >= MIN_COPILOT_CHANGE_LEN || insertedLineCount > 1) {
                const startLine = change.range.start.line + 1; // 1-based
                for (let l = 0; l < insertedLineCount; l++) {
                    pending.lines.add(startLine + l);
                }
                changed = true;
            }
        }

        if (!changed) {
            logEvent('debug', 'change.ignored_below_threshold', { filePath, relPath });
            return;
        }

        pendingChanges.set(filePath, pending);
        logEvent('debug', 'change.buffered', {
            filePath,
            relPath,
            documentVersion: pending.documentVersion,
            lines: Array.from(pending.lines).sort((a, b) => a - b),
        });
        if (flushTimer) clearTimeout(flushTimer);
        flushTimer = setTimeout(flushAll, FLUSH_DELAY_MS);
    });

    // On save, flush pending changes for that file immediately.
    const saveDisposable = vscode.workspace.onDidSaveTextDocument(async (doc) => {
        if (doc.uri.scheme !== 'file') return;
        const filePath = doc.uri.fsPath;
        const pending = pendingChanges.get(filePath);
        if (!pending || pending.lines.size === 0) return;
        pending.tool = 'save';
        pending.document = doc;
        pending.documentVersion = doc.version || pending.documentVersion || null;
        await captureDocument(doc, pending);
        pendingChanges.delete(filePath);
        if (pendingChanges.size === 0 && flushTimer) {
            clearTimeout(flushTimer);
            flushTimer = null;
        }
    });

    // Command: manually record all lines of the current file as Copilot-authored.
    // Useful after a Copilot Chat session that generated a whole file.
    const captureCmd = vscode.commands.registerCommand('agentdiff.captureNow', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showInformationMessage('agentdiff: No active editor');
            return;
        }
        const lines = new Set();
        for (let i = 1; i <= editor.document.lineCount; i++) lines.add(i);
        const ok = await captureDocument(editor.document, {
            lines,
            tool: 'manual',
            documentVersion: editor.document.version || null,
            changedAt: null,
        });
        vscode.window.showInformationMessage(ok
            ? 'agentdiff: Copilot capture recorded'
            : 'agentdiff: Copilot capture skipped; see AgentDiff output');
    });

    const openLogsCmd = vscode.commands.registerCommand('agentdiff.openLogs', () => {
        if (channel && typeof channel.show === 'function') {
            channel.show();
        }
    });

    context.subscriptions.push(changeDisposable, saveDisposable, captureCmd, openLogsCmd);
}

function deactivate() {
    logEvent('info', 'extension.deactivated');
    if (outputChannel && typeof outputChannel.dispose === 'function') {
        outputChannel.dispose();
        outputChannel = null;
    }
}

module.exports = {
    activate,
    deactivate,
    _test: {
        buildCaptureProcess,
        getRelativePath,
        isExcludedRelativePath,
        windowsPathToWsl,
    },
};
