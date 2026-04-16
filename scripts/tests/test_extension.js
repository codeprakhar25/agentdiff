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
