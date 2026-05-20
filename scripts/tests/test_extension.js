'use strict';
/**
 * Unit tests for scripts/vscode-extension/extension.js
 *
 * Run with: node --test scripts/tests/test_extension.js
 * (Node 18+ required for built-in test runner)
 *
 * The vscode module is not available outside VS Code, so we inject a mock
 * into require.cache before loading the extension. All tests work offline
 * with zero npm dependencies.
 */
const { test, describe, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const path = require('path');
const Module = require('module');
const EventEmitter = require('events');
const childProcess = require('child_process');
const fs = require('fs');

const EXT_PATH = path.resolve(__dirname, '../../scripts/vscode-extension/extension.js');

let origExecFile;
let origSpawn;
let origExistsSync;
let origAppendFileSync;
let origMkdirSync;

beforeEach(() => {
    origExecFile = childProcess.execFile;
    origSpawn = childProcess.spawn;
    origExistsSync = fs.existsSync;
    origAppendFileSync = fs.appendFileSync;
    origMkdirSync = fs.mkdirSync;
    fs.appendFileSync = () => {};
    fs.mkdirSync = () => {};
});

afterEach(() => {
    childProcess.execFile = origExecFile;
    childProcess.spawn = origSpawn;
    fs.existsSync = origExistsSync;
    fs.appendFileSync = origAppendFileSync;
    fs.mkdirSync = origMkdirSync;
    delete require.cache[EXT_PATH];
    delete require.cache.__vscode_mock__;
});

function makeUri(fsPath, scheme = 'file') {
    return { scheme, fsPath };
}

function makeDocument(filePath, { scheme = 'file', version = 1, lineCount = 1 } = {}) {
    return { uri: makeUri(filePath, scheme), version, lineCount };
}

function makeVscodeMock({ copilotInstalled = true, copilotActive = true, workspaceFolders } = {}) {
    const subscriptions = [];
    const registeredCommands = {};
    const outputLines = [];
    const folders = (workspaceFolders || ['/tmp/repo-a', '/tmp/repo-b']).map((folderPath) => ({
        uri: makeUri(folderPath),
        name: path.basename(folderPath),
    }));

    function makeEvent() {
        let _handler = null;
        const event = (handler) => {
            _handler = handler;
            return { dispose: () => { _handler = null; } };
        };
        event.fire = (...args) => _handler && _handler(...args);
        return event;
    }

    function folderFor(uri) {
        return folders
            .filter((folder) => {
                const rel = path.relative(folder.uri.fsPath, uri.fsPath);
                return rel === '' || (!!rel && !rel.startsWith('..') && !path.isAbsolute(rel));
            })
            .sort((a, b) => b.uri.fsPath.length - a.uri.fsPath.length)[0];
    }

    const onDidChangeTextDocument = makeEvent();
    const onDidSaveTextDocument = makeEvent();
    const copilotExt = copilotInstalled ? { isActive: copilotActive } : undefined;

    return {
        workspace: {
            workspaceFolders: folders,
            onDidChangeTextDocument,
            onDidSaveTextDocument,
            getWorkspaceFolder: folderFor,
            asRelativePath: (uriOrPath) => {
                const uri = typeof uriOrPath === 'string' ? makeUri(uriOrPath) : uriOrPath;
                const folder = folderFor(uri);
                return folder ? path.relative(folder.uri.fsPath, uri.fsPath) : uri.fsPath;
            },
            getConfiguration: () => ({ get: () => null }),
        },
        extensions: {
            getExtension: (id) => {
                if (id === 'GitHub.copilot' || id === 'GitHub.copilot-chat') {
                    return copilotExt;
                }
                return undefined;
            },
        },
        window: {
            activeTextEditor: null,
            showInformationMessage: () => {},
            createOutputChannel: () => ({
                appendLine: (line) => outputLines.push(line),
                show: () => { outputLines.push('__shown__'); },
                dispose: () => {},
            }),
        },
        commands: {
            registerCommand: (id, fn) => {
                registeredCommands[id] = fn;
                return { dispose: () => {} };
            },
        },
        lm: {
            selectChatModels: async () => [{ id: 'copilot-test-model' }],
        },
        Uri: { parse: (s) => makeUri(s) },
        _fire: { change: onDidChangeTextDocument.fire, save: onDidSaveTextDocument.fire },
        _commands: registeredCommands,
        _subscriptions: subscriptions,
        _outputLines: outputLines,
    };
}

function loadExtension(vscodeMock) {
    const origResolve = Module._resolveFilename;
    Module._resolveFilename = function (request, parent, isMain, options) {
        if (request === 'vscode') return '__vscode_mock__';
        return origResolve.call(this, request, parent, isMain, options);
    };

    require.cache.__vscode_mock__ = {
        id: '__vscode_mock__',
        filename: '__vscode_mock__',
        loaded: true,
        exports: vscodeMock,
        children: [],
        paths: [],
    };

    delete require.cache[EXT_PATH];

    try {
        return require(EXT_PATH);
    } finally {
        Module._resolveFilename = origResolve;
    }
}

function activateExt(vscodeMock) {
    const ext = loadExtension(vscodeMock);
    const ctx = { subscriptions: vscodeMock._subscriptions };
    ext.activate(ctx);
    return { ext, vscode: vscodeMock };
}

function makeChangeEvent(filePath, changes, { scheme = 'file', version = 1 } = {}) {
    return {
        document: makeDocument(filePath, { scheme, version }),
        contentChanges: changes.map(({ text, startLine = 0 }) => ({
            text,
            range: { start: { line: startLine } },
        })),
    };
}

function installRepoStubs({ initialized = true } = {}) {
    const spawned = [];

    childProcess.execFile = (_cmd, args, opts, cb) => {
        const cwd = opts.cwd;
        const repo = cwd.startsWith('/tmp/repo-a') ? '/tmp/repo-a'
            : cwd.startsWith('/tmp/repo-b') ? '/tmp/repo-b'
                : null;
        if (!repo) {
            cb(new Error('not a repo'), '', 'fatal');
            return;
        }
        if (args.join(' ') === 'rev-parse --show-toplevel') {
            cb(null, `${repo}\n`, '');
            return;
        }
        if (args.join(' ') === 'rev-parse --git-common-dir') {
            cb(null, `${repo}/.git\n`, '');
            return;
        }
        cb(new Error('unexpected git args'), '', '');
    };

    fs.existsSync = (p) => {
        if (p === '__AGENTDIFF_CAPTURE_COPILOT__') return true;
        if (String(p).endsWith('/.git/agentdiff')) return initialized;
        return origExistsSync(p);
    };

    childProcess.spawn = (command, args) => {
        const proc = new EventEmitter();
        proc.stderr = new EventEmitter();
        proc.stdin = {
            chunks: [],
            write(chunk) { this.chunks.push(String(chunk)); },
            end() {
                spawned.push({
                    command,
                    args,
                    payload: JSON.parse(this.chunks.join('')),
                });
            },
        };
        return proc;
    };

    return spawned;
}

describe('Extension: edit capture flow', () => {
    test('captures a large insertion on save with repo, workspace, version, and lines', async () => {
        const spawned = installRepoStubs();
        const vscode = makeVscodeMock();
        activateExt(vscode);

        const filePath = '/tmp/repo-a/src/main.rs';
        vscode._fire.change(makeChangeEvent(filePath, [{ text: 'x'.repeat(80), startLine: 2 }], { version: 7 }));
        await vscode._fire.save(makeDocument(filePath, { version: 7 }));

        assert.equal(spawned.length, 1);
        assert.equal(spawned[0].command, 'python3');
        assert.deepEqual(spawned[0].payload.lines, [3]);
        assert.equal(spawned[0].payload.event, 'save');
        assert.equal(spawned[0].payload.cwd, '/tmp/repo-a');
        assert.equal(spawned[0].payload.repo_root, '/tmp/repo-a');
        assert.equal(spawned[0].payload.workspace_folder, '/tmp/repo-a');
        assert.equal(spawned[0].payload.document_version, 7);
        assert.equal(spawned[0].payload.model, 'copilot-test-model');
    });

    test('captures multi-line insertions below the single-line threshold', async () => {
        const spawned = installRepoStubs();
        const vscode = makeVscodeMock();
        activateExt(vscode);

        const filePath = '/tmp/repo-a/src/main.rs';
        vscode._fire.change(makeChangeEvent(filePath, [{ text: 'a\nb', startLine: 4 }], { version: 2 }));
        await vscode._fire.save(makeDocument(filePath, { version: 2 }));

        assert.equal(spawned.length, 1);
        assert.deepEqual(spawned[0].payload.lines, [5, 6]);
    });

    test('does not capture a short single-line insertion', async () => {
        const spawned = installRepoStubs();
        const vscode = makeVscodeMock();
        activateExt(vscode);

        const filePath = '/tmp/repo-a/src/main.rs';
        vscode._fire.change(makeChangeEvent(filePath, [{ text: 'x'.repeat(49) }]));
        await vscode._fire.save(makeDocument(filePath));

        assert.equal(spawned.length, 0);
    });

    test('skips non-file documents before buffering capture state', async () => {
        const spawned = installRepoStubs();
        const vscode = makeVscodeMock();
        activateExt(vscode);

        vscode._fire.change(makeChangeEvent('/tmp/repo-a/src/main.rs', [{ text: 'x'.repeat(80) }], { scheme: 'git' }));

        assert.equal(spawned.length, 0);
    });
});

describe('Extension: multi-root workspace and path filtering', () => {
    test('uses the owning workspace folder in a multi-root workspace', async () => {
        const spawned = installRepoStubs();
        const vscode = makeVscodeMock();
        activateExt(vscode);

        const filePath = '/tmp/repo-b/pkg/lib.js';
        vscode._fire.change(makeChangeEvent(filePath, [{ text: 'y'.repeat(80), startLine: 0 }]));
        await vscode._fire.save(makeDocument(filePath));

        assert.equal(spawned.length, 1);
        assert.equal(spawned[0].payload.cwd, '/tmp/repo-b');
        assert.equal(spawned[0].payload.repo_root, '/tmp/repo-b');
        assert.equal(spawned[0].payload.workspace_folder, '/tmp/repo-b');
    });

    test('ignores .agentdiff and .git paths relative to the owning root', async () => {
        const spawned = installRepoStubs();
        const vscode = makeVscodeMock();
        activateExt(vscode);

        vscode._fire.change(makeChangeEvent('/tmp/repo-b/.agentdiff/ledger.jsonl', [{ text: 'x'.repeat(80) }]));
        vscode._fire.change(makeChangeEvent('/tmp/repo-b/.git/COMMIT_EDITMSG', [{ text: 'x'.repeat(80) }]));

        assert.equal(spawned.length, 0);
    });
});

describe('Extension: diagnostics and commands', () => {
    test('logs a structured warning when the repo has not been initialized', async () => {
        const spawned = installRepoStubs({ initialized: false });
        const vscode = makeVscodeMock();
        activateExt(vscode);

        const filePath = '/tmp/repo-a/src/main.rs';
        vscode._fire.change(makeChangeEvent(filePath, [{ text: 'x'.repeat(80) }]));
        await vscode._fire.save(makeDocument(filePath));

        assert.equal(spawned.length, 0);
        assert.ok(vscode._outputLines.some((line) => line.includes('"event":"capture.skipped_repo_not_initialized"')));
    });

    test('registers capture and log commands', () => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        assert.ok('agentdiff.captureNow' in vscode._commands);
        assert.ok('agentdiff.openLogs' in vscode._commands);
    });

    test('open logs command shows the output channel', () => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        vscode._commands['agentdiff.openLogs']();

        assert.ok(vscode._outputLines.includes('__shown__'));
    });

    test('captureNow shows a message when no active editor exists', async () => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        let shown = null;
        vscode.window.showInformationMessage = (msg) => { shown = msg; };
        vscode.window.activeTextEditor = null;

        await vscode._commands['agentdiff.captureNow']();

        assert.ok(shown && shown.includes('agentdiff'), `Expected info message, got: ${shown}`);
    });

    test('activate returns early without listeners when Copilot is absent', () => {
        const vscode = makeVscodeMock({ copilotInstalled: false });
        activateExt(vscode);

        assert.equal(vscode._subscriptions.length, 0);
        assert.ok(vscode._outputLines.some((line) => line.includes('"event":"extension.inactive_copilot_missing"')));
    });
});

describe('Extension: WSL path helpers', () => {
    test('translates common Windows and WSL UNC paths for WSL capture scripts', () => {
        const vscode = makeVscodeMock();
        const { ext } = activateExt(vscode);

        assert.equal(ext._test.windowsPathToWsl('C:\\Users\\me\\repo\\file.js'), '/mnt/c/Users/me/repo/file.js');
        assert.equal(ext._test.windowsPathToWsl('\\\\wsl.localhost\\Ubuntu\\home\\me\\repo\\file.js'), '/home/me/repo/file.js');
        assert.equal(ext._test.windowsPathToWsl('/home/me/repo/file.js'), '/home/me/repo/file.js');
    });
});

describe('Extension: deactivate', () => {
    test('deactivate does not throw', () => {
        const vscode = makeVscodeMock();
        const { ext } = activateExt(vscode);
        assert.doesNotThrow(() => ext.deactivate());
    });
});
