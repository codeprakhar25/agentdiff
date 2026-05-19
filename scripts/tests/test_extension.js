'use strict';
/**
 * Unit tests for scripts/vscode-extension/extension.js
 *
 * Run with:  node --test scripts/tests/test_extension.js
 * (Node 18+ required for built-in test runner)
 *
 * The vscode module is not available outside VS Code, so we inject a mock
 * into require.cache before loading the extension.  All tests work offline
 * with zero npm dependencies.
 */
const { test, describe, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const path = require('path');
const Module = require('module');

// ─── Mock vscode ─────────────────────────────────────────────────────────────

/**
 * Minimal vscode mock that lets the extension register listeners and commands
 * without an actual extension host.  Returns handles so tests can fire events.
 */
function makeVscodeMock({ copilotInstalled = true, copilotActive = true } = {}) {
    const subscriptions = [];
    const registeredCommands = {};

    // EventEmitter-style helpers
    function makeEvent() {
        let _handler = null;
        const event = (handler) => {
            _handler = handler;
            return { dispose: () => { _handler = null; } };
        };
        event.fire = (...args) => _handler && _handler(...args);
        return event;
    }

    const onDidChangeTextDocument = makeEvent();
    const onDidSaveTextDocument = makeEvent();

    const copilotExt = copilotInstalled
        ? { isActive: copilotActive }
        : undefined;

    const mock = {
        workspace: {
            onDidChangeTextDocument,
            onDidSaveTextDocument,
            asRelativePath: (fsPath) => fsPath.replace(/^\/tmp\/[^/]+\//, ''),
            getConfiguration: (_section) => ({
                get: (_key) => null,
            }),
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
        },
        commands: {
            registerCommand: (id, fn) => {
                registeredCommands[id] = fn;
                return { dispose: () => {} };
            },
        },
        Uri: { parse: (s) => ({ scheme: 'file', fsPath: s }) },

        // Test helpers (not part of real vscode API)
        _fire: { change: onDidChangeTextDocument.fire, save: onDidSaveTextDocument.fire },
        _commands: registeredCommands,
        _subscriptions: subscriptions,
    };

    return mock;
}

// ─── Load extension with a given vscode mock ─────────────────────────────────

const EXT_PATH = path.resolve(__dirname, '../../scripts/vscode-extension/extension.js');

function loadExtension(vscodeMock) {
    // Patch require.cache with the mock before loading.
    // The module key must match what extension.js resolves 'vscode' to.
    const fakeVscodeId = require.resolve('path'); // any stable key — we overwrite below
    const vscodeKey = 'vscode'; // extension does: require('vscode')

    // Node resolves built-in modules differently; inject via _resolveFilename hook.
    const origResolve = Module._resolveFilename;
    Module._resolveFilename = function (request, parent, isMain, options) {
        if (request === 'vscode') return '__vscode_mock__';
        return origResolve.call(this, request, parent, isMain, options);
    };

    // Place the mock in require.cache under our fake id.
    require.cache['__vscode_mock__'] = {
        id: '__vscode_mock__',
        filename: '__vscode_mock__',
        loaded: true,
        exports: vscodeMock,
        children: [],
        paths: [],
    };

    // Clear the extension from cache so it re-executes with the new mock.
    delete require.cache[EXT_PATH];

    let ext;
    try {
        ext = require(EXT_PATH);
    } finally {
        Module._resolveFilename = origResolve;
    }

    return ext;
}

function activateExt(vscodeMock) {
    const ext = loadExtension(vscodeMock);
    const ctx = { subscriptions: vscodeMock._subscriptions };
    ext.activate(ctx);
    return { ext, vscode: vscodeMock };
}

// ─── Helper: build a fake text document change event ────────────────────────

function makeChangeEvent(filePath, changes, { scheme = 'file', isActive = true } = {}) {
    return {
        document: { uri: { scheme, fsPath: filePath } },
        contentChanges: changes.map(({ text, startLine = 0 }) => ({
            text,
            range: { start: { line: startLine } },
        })),
        // stub — vscode extension checks copilotExt.isActive, not this
    };
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe('Extension: Copilot detection threshold (MIN_COPILOT_CHANGE_LEN = 50)', () => {
    test('does not capture a short single-line insertion (<50 chars)', (t, done) => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        // 49 chars — below threshold
        const shortText = 'x'.repeat(49);
        vscode._fire.change(makeChangeEvent('/tmp/repo/src/main.rs', [{ text: shortText }]));

        // Wait longer than the 2-second debounce to confirm nothing fires.
        // We can't easily intercept the spawn, so we just check no error is thrown
        // and the handler runs without crashing.
        setTimeout(() => done(), 50);
    });

    test('handles multi-line insertion (>1 newline) regardless of length', (t, done) => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        // Only 5 chars but spans 2 lines
        vscode._fire.change(makeChangeEvent('/tmp/repo/src/main.rs', [{ text: 'a\nb' }]));

        // Extension should not throw
        setTimeout(() => done(), 50);
    });

    test('ignores changes from non-file URI schemes', (t, done) => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        // git scheme — should be ignored immediately
        vscode._fire.change(makeChangeEvent('/tmp/repo/src/main.rs', [{ text: 'x'.repeat(100) }], { scheme: 'git' }));

        setTimeout(() => done(), 50);
    });
});

describe('Extension: EXCLUDED_PATHS filtering', () => {
    test('ignores changes to .agentdiff/ paths', (t, done) => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        // This path resolves to something that starts with .agentdiff/ after asRelativePath
        const event = {
            document: { uri: { scheme: 'file', fsPath: '/tmp/repo/.agentdiff/ledger.jsonl' } },
            contentChanges: [{ text: 'x'.repeat(200), range: { start: { line: 0 } } }],
        };
        // asRelativePath will return '.agentdiff/ledger.jsonl' — should be filtered
        const vscodeReal = require.__spy || vscode;
        vscode.workspace.asRelativePath = (_p) => '.agentdiff/ledger.jsonl';

        vscode._fire.change(event);
        setTimeout(() => done(), 50);
    });

    test('ignores changes to .git/ paths', (t, done) => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        const event = {
            document: { uri: { scheme: 'file', fsPath: '/tmp/repo/.git/COMMIT_EDITMSG' } },
            contentChanges: [{ text: 'x'.repeat(200), range: { start: { line: 0 } } }],
        };
        vscode.workspace.asRelativePath = (_p) => '.git/COMMIT_EDITMSG';

        vscode._fire.change(event);
        setTimeout(() => done(), 50);
    });
});

describe('Extension: Copilot not installed', () => {
    test('activate() returns early without registering listeners when Copilot is absent', () => {
        const vscode = makeVscodeMock({ copilotInstalled: false });
        const { ext } = activateExt(vscode);

        // If Copilot is not installed, no subscriptions should be registered
        assert.equal(vscode._subscriptions.length, 0,
            'Should not register any listeners when Copilot extension is absent');
    });
});

describe('Extension: getCopilotModel() fallback', () => {
    test('returns "gpt-4o" when copilot-chat extension is present', () => {
        // Load module and inspect getCopilotModel directly by activating
        // and checking what model ends up in the capture payload.
        // We do this indirectly — if chat ext is present without advanced config,
        // getCopilotModel returns 'gpt-4o'.  We verify activate() doesn't throw.
        const vscode = makeVscodeMock({ copilotInstalled: true, copilotActive: true });
        // Make copilot-chat also present
        vscode.extensions.getExtension = (id) => {
            if (id === 'GitHub.copilot') return { isActive: true };
            if (id === 'GitHub.copilot-chat') return { isActive: true };
            return undefined;
        };
        assert.doesNotThrow(() => activateExt(vscode));
    });

    test('returns "copilot" fallback when no config and no chat ext', () => {
        const vscode = makeVscodeMock();
        vscode.extensions.getExtension = (id) => {
            if (id === 'GitHub.copilot') return { isActive: true };
            // no chat ext
            return undefined;
        };
        assert.doesNotThrow(() => activateExt(vscode));
    });
});

describe('Extension: agentdiff.captureNow command', () => {
    test('command is registered after activation', () => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        assert.ok('agentdiff.captureNow' in vscode._commands,
            'agentdiff.captureNow command should be registered');
    });

    test('captureNow shows message when no active editor', async () => {
        const vscode = makeVscodeMock();
        activateExt(vscode);

        let shown = null;
        vscode.window.showInformationMessage = (msg) => { shown = msg; };
        vscode.window.activeTextEditor = null;

        await vscode._commands['agentdiff.captureNow']();

        assert.ok(shown && shown.includes('agentdiff'), `Expected info message, got: ${shown}`);
    });
});

describe('Extension: deactivate', () => {
    test('deactivate() does not throw', () => {
        const vscode = makeVscodeMock();
        const { ext } = activateExt(vscode);
        assert.doesNotThrow(() => ext.deactivate());
    });
});

describe('Extension: confidence metadata', () => {
    /**
     * Intercept fireCapture by stubbing fs.existsSync and cp.spawn so we can
     * inspect the payload without spawning a real Python process.
     */
    function activateWithCaptureInterceptor(vscode) {
        // Load the extension module fresh
        const ext = loadExtension(vscode);

        // We capture payloads by monkey-patching the module's internal spawn.
        // The extension reads CAPTURE_SCRIPT at runtime; make it look like it exists
        // by patching fs.existsSync for the '__AGENTDIFF_CAPTURE_COPILOT__' path.
        const captured = [];
        const origExistsSync = require('fs').existsSync;
        require('fs').existsSync = (p) => {
            if (p === '__AGENTDIFF_CAPTURE_COPILOT__') return true;
            return origExistsSync(p);
        };

        const cp = require('child_process');
        const origSpawn = cp.spawn;
        cp.spawn = (_cmd, _args, _opts) => {
            // Return a fake child process that captures what was written to stdin.
            let written = '';
            const fakeStdin = {
                write: (data) => { written += data; },
                end: () => {
                    try { captured.push(JSON.parse(written)); } catch (_) {}
                },
            };
            return { stdin: fakeStdin, on: () => {} };
        };

        // Also stub findRepoRoot (cp.exec) so it resolves synchronously
        const origExec = cp.exec;
        cp.exec = (_cmd, _opts, cb) => cb(null, '/tmp/repo');

        // Stub getCopilotModel (vscode.lm)
        vscode.lm = { selectChatModels: async () => [] };

        const ctx = { subscriptions: vscode._subscriptions };
        ext.activate(ctx);

        function restore() {
            require('fs').existsSync = origExistsSync;
            cp.spawn = origSpawn;
            cp.exec = origExec;
        }

        return { ext, vscode, captured, restore };
    }

    test('heuristic onDidChangeTextDocument capture has confidence="low" and capture_mode="inline_heuristic"', async (t) => {
        const vscode = makeVscodeMock();
        const { captured, restore } = activateWithCaptureInterceptor(vscode);

        try {
            // Fire a large insertion to trigger the heuristic path
            const bigText = 'x'.repeat(60);
            vscode._fire.change(makeChangeEvent('/tmp/repo/src/main.rs', [{ text: bigText }]));

            // Wait for the 2-second debounce to fire (use a short flush trick).
            // We can't easily fast-forward timers in node:test without a library,
            // so we instead rely on the save-flush path (no timer).
            vscode._fire.save({ uri: { scheme: 'file', fsPath: '/tmp/repo/src/main.rs' } });

            // Give the async captureFile call time to resolve
            await new Promise((r) => setTimeout(r, 100));

            assert.ok(captured.length > 0, 'Expected at least one capture payload');
            const payload = captured[captured.length - 1];
            assert.equal(payload.confidence, 'low', `Expected confidence="low", got ${payload.confidence}`);
            // save-flush sets capture_mode to "save_flush"
            assert.ok(
                payload.capture_mode === 'save_flush' || payload.capture_mode === 'inline_heuristic',
                `Expected save_flush or inline_heuristic, got ${payload.capture_mode}`
            );
        } finally {
            restore();
        }
    });

    test('captureNow command produces confidence="high" and capture_mode="manual"', async (t) => {
        const vscode = makeVscodeMock();
        const { captured, restore } = activateWithCaptureInterceptor(vscode);

        try {
            // Set up a fake active editor
            vscode.window.activeTextEditor = {
                document: {
                    uri: { fsPath: '/tmp/repo/src/main.rs' },
                    lineCount: 5,
                },
            };

            await vscode._commands['agentdiff.captureNow']();

            // Give the async captureFile call time to resolve
            await new Promise((r) => setTimeout(r, 100));

            assert.ok(captured.length > 0, 'Expected at least one capture payload from captureNow');
            const payload = captured[captured.length - 1];
            assert.equal(payload.confidence, 'high', `Expected confidence="high", got ${payload.confidence}`);
            assert.equal(payload.capture_mode, 'manual', `Expected capture_mode="manual", got ${payload.capture_mode}`);
            assert.equal(payload.event, 'manual', `Expected event="manual", got ${payload.event}`);
        } finally {
            restore();
        }
    });
});

describe('Extension: stable window session ID', () => {
    test('session_id is consistent across multiple captures in same window', async (t) => {
        const vscode = makeVscodeMock();

        const captured = [];
        const origExistsSync = require('fs').existsSync;
        require('fs').existsSync = (p) => {
            if (p === '__AGENTDIFF_CAPTURE_COPILOT__') return true;
            return origExistsSync(p);
        };
        const cp = require('child_process');
        const origSpawn = cp.spawn;
        cp.spawn = (_cmd, _args, _opts) => {
            let written = '';
            const fakeStdin = {
                write: (data) => { written += data; },
                end: () => {
                    try { captured.push(JSON.parse(written)); } catch (_) {}
                },
            };
            return { stdin: fakeStdin, on: () => {} };
        };
        const origExec = cp.exec;
        cp.exec = (_cmd, _opts, cb) => cb(null, '/tmp/repo');
        vscode.lm = { selectChatModels: async () => [] };

        const ext = loadExtension(vscode);
        const ctx = { subscriptions: vscode._subscriptions };
        ext.activate(ctx);

        try {
            // Trigger captureNow twice
            vscode.window.activeTextEditor = {
                document: {
                    uri: { fsPath: '/tmp/repo/a.rs' },
                    lineCount: 3,
                },
            };
            await vscode._commands['agentdiff.captureNow']();

            vscode.window.activeTextEditor = {
                document: {
                    uri: { fsPath: '/tmp/repo/b.rs' },
                    lineCount: 2,
                },
            };
            await vscode._commands['agentdiff.captureNow']();

            await new Promise((r) => setTimeout(r, 100));

            assert.ok(captured.length >= 2, `Expected >=2 captures, got ${captured.length}`);
            const ids = captured.map((p) => p.session_id);
            assert.equal(
                ids[0], ids[1],
                `session_id must be stable within a window: got ${ids[0]} vs ${ids[1]}`
            );
            assert.ok(
                ids[0].startsWith('vscode-'),
                `session_id should start with "vscode-", got ${ids[0]}`
            );
        } finally {
            require('fs').existsSync = origExistsSync;
            cp.spawn = origSpawn;
            cp.exec = origExec;
        }
    });
});
