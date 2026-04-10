use crate::config::{Config, RepoConfig};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::{fs, path::Path, process::Command};

const CLAUDE_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-claude.py");
const CURSOR_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-cursor.py");
const CODEX_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-codex.py");
const WINDSURF_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-windsurf.py");
const OPENCODE_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-opencode.py");
const OPENCODE_PLUGIN_TEMPLATE: &str = include_str!("../scripts/opencode-agentdiff.ts");
const ANTIGRAVITY_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-antigravity.py");
const COPILOT_CAPTURE_SCRIPT: &str = include_str!("../scripts/capture-copilot.py");
const COPILOT_EXT_PACKAGE_JSON: &str =
    include_str!("../scripts/vscode-extension/package.json");
const COPILOT_EXT_JS_TEMPLATE: &str =
    include_str!("../scripts/vscode-extension/extension.js");
const PREPARE_LEDGER_SCRIPT: &str = include_str!("../scripts/prepare-ledger.py");
const FINALIZE_LEDGER_SCRIPT: &str = include_str!("../scripts/finalize-ledger.py");
const RECORD_CONTEXT_SCRIPT: &str = include_str!("../scripts/record-context.py");
const WRITE_NOTE_SCRIPT: &str = include_str!("../scripts/write-note.py");

/// Configure global agent hooks — run once per machine, no git repo required.
pub fn run_configure(
    config: &mut Config,
    no_claude: bool,
    no_cursor: bool,
    no_codex: bool,
    no_antigravity: bool,
    no_windsurf: bool,
    no_opencode: bool,
    no_copilot: bool,
    no_mcp: bool,
) -> Result<()> {
    println!("{}", "agentdiff configure".bold().cyan());
    println!();

    // Check Python 3 availability — capture scripts require it.
    check_python3()?;

    // Step 1 — create global dirs
    step_create_dirs(config)?;

    // Step 2 — install Python scripts into ~/.agentdiff/scripts/
    step_install_scripts(config)?;

    // Step 3 — configure Claude Code ~/.claude/settings.json (hooks + MCP server)
    if !no_claude {
        step_configure_claude(config)?;
    }
    if !no_mcp {
        step_configure_mcp_claude()?;
    }

    // Step 4 — configure Cursor ~/.cursor/hooks.json
    if !no_cursor {
        step_configure_cursor(config)?;
    }

    // Step 5 — configure Codex ~/.codex/config.toml notify hook
    if !no_codex {
        step_configure_codex(config)?;
    }

    // Step 6 — configure Gemini / Antigravity hooks
    if !no_antigravity {
        step_configure_antigravity(config)?;
    }

    // Step 7 — configure Windsurf globally (~/.codeium/windsurf/hooks.json)
    if !no_windsurf {
        step_configure_windsurf(config)?;
    }

    // Step 8 — configure OpenCode globally (~/.config/opencode/plugins/)
    if !no_opencode {
        step_configure_opencode(config)?;
    }

    // Step 9 — install VS Code Copilot extension
    if !no_copilot {
        step_configure_copilot(config)?;
    }

    // Save updated config
    config.save()?;
    println!(
        "{} Config written to {}",
        "ok".green(),
        Config::config_path().display()
    );

    println!();
    println!("{}", "agentdiff configure complete.".bold().green());
    println!(
        "{}",
        "Run 'agentdiff init' inside each repo you want to track.".dimmed()
    );
    Ok(())
}

/// Initialize agentdiff in this repository — installs git hooks and creates the ledger.
/// Run `agentdiff configure` first to set up global agent hooks.
pub fn run_init(
    repo_root: &Path,
    config: &mut Config,
    no_git_hook: bool,
    migrate: bool,
) -> Result<()> {
    println!("{}", "agentdiff init".bold().cyan());
    println!("Repo: {}", repo_root.display());
    println!();

    // Warn if configure hasn't been run yet (scripts dir empty or missing).
    let scripts_dir = config.scripts_root();
    let capture_claude = scripts_dir.join("capture-claude.py");
    if !capture_claude.exists() {
        println!(
            "{} Agent hooks not configured yet. Run 'agentdiff configure' first to set up global hooks.",
            "!".yellow()
        );
        println!();
    }

    // Step 1 — install git hooks
    if !no_git_hook {
        step_install_git_hook(repo_root, config)?;
    }

    // Step 1b — configure fetch refspec for per-branch refs
    step_configure_refspec(repo_root)?;

    // Step 2 — register repo in global config and create ledger/session dirs
    step_register_repo(repo_root, config)?;

    // Step 3 — save updated config
    config.save()?;
    println!(
        "{} Config written to {}",
        "ok".green(),
        Config::config_path().display()
    );

    // In impl-1, migration is intentionally disabled by default.
    if migrate {
        println!(
            "{} Legacy migration is not part of impl-1; skipping.",
            "!".yellow()
        );
    }

    println!();
    println!("{}", "agentdiff init complete.".bold().green());
    Ok(())
}

fn check_python3() -> Result<()> {
    // On Windows, `python3` may not exist; `python` is the common name.
    let python_cmd = if cfg!(windows) { "python" } else { "python3" };
    let output = Command::new(python_cmd).arg("--version").output();
    match output {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            println!("{} {python_cmd} found: {ver}", "ok".green());
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "{python_cmd} not found on PATH.\n\
             agentdiff capture scripts require Python 3.\n\
             Install Python 3 and ensure it is on your PATH, then re-run 'agentdiff configure'."
        )),
    }
}

fn step_create_dirs(config: &Config) -> Result<()> {
    let dirs = [
        config.spillover_root(),
        config.scripts_root(),
        Config::config_path().parent().unwrap().to_path_buf(),
    ];
    for dir in &dirs {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        println!("{} mkdir {}", "ok".green().dimmed(), dir.display());
    }
    Ok(())
}

fn step_install_scripts(config: &Config) -> Result<()> {
    let scripts = [
        ("capture-claude.py", CLAUDE_CAPTURE_SCRIPT),
        ("capture-cursor.py", CURSOR_CAPTURE_SCRIPT),
        ("capture-codex.py", CODEX_CAPTURE_SCRIPT),
        ("capture-windsurf.py", WINDSURF_CAPTURE_SCRIPT),
        ("capture-opencode.py", OPENCODE_CAPTURE_SCRIPT),
        ("capture-antigravity.py", ANTIGRAVITY_CAPTURE_SCRIPT),
        ("capture-copilot.py", COPILOT_CAPTURE_SCRIPT),
        ("prepare-ledger.py", PREPARE_LEDGER_SCRIPT),
        ("finalize-ledger.py", FINALIZE_LEDGER_SCRIPT),
        ("record-context.py", RECORD_CONTEXT_SCRIPT),
        ("write-note.py", WRITE_NOTE_SCRIPT),
    ];
    for (name, content) in &scripts {
        let dest = config.scripts_root().join(name);
        let existing = fs::read_to_string(&dest).unwrap_or_default();
        if existing != *content {
            fs::write(&dest, content).with_context(|| format!("writing {}", dest.display()))?;
            println!("{} installed {}", "ok".green(), dest.display());
        } else {
            println!("{} up-to-date {}", "--".dimmed(), dest.display());
        }
    }
    Ok(())
}

fn step_install_git_hook(repo_root: &Path, config: &Config) -> Result<()> {
    let hooks_dir = repo_root.join(".git").join("hooks");
    if !hooks_dir.exists() {
        bail!("Not a git repository (no .git/hooks directory)");
    }

    let pre_commit_path = hooks_dir.join("pre-commit");
    let post_commit_path = hooks_dir.join("post-commit");
    let pre_push_path = hooks_dir.join("pre-push");
    let scripts_dir = config.scripts_root();
    let session_log = Config::repo_session_log(repo_root);
    let pending_context = Config::repo_pending_context(repo_root);
    let pending_ledger = Config::repo_pending_ledger(repo_root);
    let lockfile = Config::repo_lockfile(repo_root);

    let pre_commit_content = format!(
        r#"#!/usr/bin/env bash
# agentdiff pre-commit hook — managed by agentdiff init
# DO NOT EDIT — regenerate with: agentdiff init

set -euo pipefail

REPO_ROOT="{repo_root}"
SESSION_LOG="{session_log}"
PENDING_CONTEXT="{pending_context}"
PENDING_LEDGER="{pending_ledger}"
SCRIPTS_DIR="{scripts_dir}"

mkdir -p "$(dirname "$PENDING_CONTEXT")"
python3 "$SCRIPTS_DIR/prepare-ledger.py" "$REPO_ROOT" "$SESSION_LOG" "$PENDING_CONTEXT" "$PENDING_LEDGER"
exit 0
"#,
        repo_root = repo_root.display(),
        session_log = session_log.display(),
        pending_context = pending_context.display(),
        pending_ledger = pending_ledger.display(),
        scripts_dir = scripts_dir.display(),
    );

    let post_commit_content = format!(
        r#"#!/usr/bin/env bash
# agentdiff post-commit hook — managed by agentdiff init
# DO NOT EDIT — regenerate with: agentdiff init

set -euo pipefail

REPO_ROOT="{repo_root}"
PENDING_CONTEXT="{pending_context}"
PENDING_LEDGER="{pending_ledger}"
LOCKFILE="{lockfile}"
SCRIPTS_DIR="{scripts_dir}"

[ -f "$LOCKFILE" ] && exit 0

mkdir -p "$(dirname "$LOCKFILE")"
touch "$LOCKFILE"
trap 'rm -f "$LOCKFILE"' EXIT

# Finalize trace entry in Agent Trace format (UUID-keyed).
python3 "$SCRIPTS_DIR/finalize-ledger.py" "$REPO_ROOT" "$PENDING_LEDGER" "$PENDING_CONTEXT"

# Sign the last trace entry (no-op if keys not initialized).
agentdiff sign-entry 2>/dev/null || true

# Print a post-commit attribution summary.
echo ""
agentdiff -C "$REPO_ROOT" status 2>/dev/null || true
echo ""
exit 0
"#,
        repo_root = repo_root.display(),
        pending_context = pending_context.display(),
        pending_ledger = pending_ledger.display(),
        lockfile = lockfile.display(),
        scripts_dir = scripts_dir.display(),
    );

    let pre_push_content = format!(
        r#"#!/usr/bin/env bash
# agentdiff pre-push hook — managed by agentdiff init
# DO NOT EDIT — regenerate with: agentdiff init
# Pushes local traces to per-branch ref on origin.

set -euo pipefail

REPO_ROOT="{repo_root}"

# Get current branch
branch=$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || true)
if [ -z "$branch" ] || [ "$branch" = "HEAD" ]; then
    exit 0  # detached HEAD, skip
fi

# Check for local traces.
# Use %2F encoding to match store.rs branch name sanitization (not --).
sanitized=$(echo "$branch" | sed 's|/|%2F|g')
local_traces="$REPO_ROOT/.git/agentdiff/traces/$sanitized.jsonl"
if [ ! -f "$local_traces" ]; then
    exit 0  # no traces to push
fi

# Push traces to per-branch ref (quiet, non-blocking, 30s timeout).
# push also mirrors to the local ref so consolidate can run immediately.
timeout 30 agentdiff -C "$REPO_ROOT" push --quiet 2>/dev/null || true

# On the default branch (main/master), direct pushes bypass the PR merge event
# that normally triggers CI consolidation. Auto-consolidate here instead so
# traces are never stranded in refs/agentdiff/traces/main.
default_branch=$(git -C "$REPO_ROOT" symbolic-ref refs/remotes/origin/HEAD 2>/dev/null \
    | sed 's|refs/remotes/origin/||' || echo "main")
if [ "$branch" = "$default_branch" ] || [ "$branch" = "main" ] || [ "$branch" = "master" ]; then
    timeout 60 agentdiff -C "$REPO_ROOT" consolidate --branch "$branch" --push 2>/dev/null || true
fi

exit 0
"#,
        repo_root = repo_root.display(),
    );

    install_managed_hook(
        &pre_commit_path,
        "agentdiff pre-commit hook",
        &pre_commit_content,
    )?;
    install_managed_hook(
        &post_commit_path,
        "agentdiff post-commit hook",
        &post_commit_content,
    )?;
    install_managed_hook(
        &pre_push_path,
        "agentdiff pre-push hook",
        &pre_push_content,
    )?;

    println!(
        "{} installed git hooks (pre-commit, post-commit, pre-push)",
        "ok".green()
    );
    Ok(())
}

/// Configure fetch refspec for per-branch refs: +refs/agentdiff/*:refs/agentdiff/*
fn step_configure_refspec(repo_root: &Path) -> Result<()> {
    let fetch_spec = "+refs/agentdiff/*:refs/agentdiff/*";

    // Check if already present.
    let fetch_output = Command::new("git")
        .args(["config", "--get-all", "remote.origin.fetch"])
        .current_dir(repo_root)
        .output();

    let already_present = fetch_output
        .as_ref()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .map(|s| s.lines().any(|line| line.trim() == fetch_spec))
        .unwrap_or(false);

    if !already_present {
        let status = Command::new("git")
            .args(["config", "--add", "remote.origin.fetch", fetch_spec])
            .current_dir(repo_root)
            .status()
            .context("adding remote.origin.fetch agentdiff refspec")?;
        if status.success() {
            println!("{} added fetch refspec for refs/agentdiff/*", "ok".green());
        } else {
            println!(
                "{} could not add fetch refspec (no remote origin?)",
                "!".yellow()
            );
        }
    } else {
        println!(
            "{} fetch refspec for refs/agentdiff/* already present",
            "--".dimmed()
        );
    }

    Ok(())
}

fn install_managed_hook(path: &Path, marker: &str, content: &str) -> Result<()> {
    if path.exists() {
        let existing = fs::read_to_string(path)?;
        if existing.contains(marker) {
            fs::write(path, content)?;
        } else {
            let combined = format!("{}\n\n{}", existing.trim_end(), content);
            fs::write(path, combined)?;
            println!("{} appended to existing {}", "ok".green(), path.display());
        }
    } else {
        fs::write(path, content)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[allow(dead_code)]
fn step_configure_git_notes(repo_root: &Path) -> Result<()> {
    let set_pairs = [
        ("notes.rewrite.amend", "true"),
        ("notes.rewrite.rebase", "true"),
        ("notes.rewriteRef", "refs/notes/agentdiff"),
        ("notes.rewriteMode", "overwrite"),
    ];

    for (key, value) in set_pairs {
        let status = Command::new("git")
            .args(["config", key, value])
            .current_dir(repo_root)
            .status()
            .with_context(|| format!("setting git config {key}"))?;
        if status.success() {
            println!("{} git config {}={}", "ok".green().dimmed(), key, value);
        }
    }

    // Add notes fetch refspec once.
    let fetch_spec = "+refs/notes/agentdiff:refs/notes/agentdiff";
    let fetch_output = Command::new("git")
        .args(["config", "--get-all", "remote.origin.fetch"])
        .current_dir(repo_root)
        .output()?;
    let existing = String::from_utf8_lossy(&fetch_output.stdout);
    if !existing.lines().any(|line| line.trim() == fetch_spec) {
        let status = Command::new("git")
            .args(["config", "--add", "remote.origin.fetch", fetch_spec])
            .current_dir(repo_root)
            .status()
            .context("adding remote.origin.fetch notes refspec")?;
        if status.success() {
            println!("{} added notes fetch refspec", "ok".green().dimmed());
        }
    } else {
        println!("{} notes fetch refspec already present", "--".dimmed());
    }

    // One-time fetch of notes from origin, only when URL is configured.
    let origin_url = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(repo_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            } else {
                None
            }
        });

    if origin_url.is_some() {
        let has_remote_notes_output = Command::new("git")
            .args([
                "-c",
                "credential.helper=",
                "ls-remote",
                "--exit-code",
                "origin",
                "refs/notes/agentdiff",
            ])
            .current_dir(repo_root)
            .output();
        let has_remote_notes = has_remote_notes_output
            .as_ref()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if has_remote_notes {
            let fetch_status = Command::new("git")
                .args([
                    "-c",
                    "credential.helper=",
                    "fetch",
                    "origin",
                    "refs/notes/agentdiff:refs/notes/agentdiff",
                ])
                .current_dir(repo_root)
                .status();
            match fetch_status {
                Ok(status) if status.success() => {
                    println!("{} fetched refs/notes/agentdiff", "ok".green().dimmed())
                }
                _ => println!(
                    "{} unable to fetch refs/notes/agentdiff from origin (continuing)",
                    "!".yellow()
                ),
            }
        } else {
            println!(
                "{} no remote refs/notes/agentdiff yet (nothing to fetch)",
                "--".dimmed()
            );
        }
    }

    Ok(())
}

fn step_configure_claude(config: &Config) -> Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");
    let scripts_dir = config.scripts_root();
    let capture_script = scripts_dir.join("capture-claude.py");

    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut settings: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.claude/settings.json")?;

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap()
        .entry("PostToolUse")
        .or_insert(serde_json::json!([]))
        .as_array_mut()
        .unwrap();

    let new_hook = serde_json::json!({
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [{
            "type": "command",
            "command": format!("python3 {}", capture_script.display())
        }]
    });

    let mut changed = false;
    let mut found = false;
    for hook in hooks.iter_mut() {
        let Some(hs) = hook.get_mut("hooks").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for inner in hs.iter_mut() {
            let Some(cmd_val) = inner.get_mut("command") else {
                continue;
            };
            let Some(cmd) = cmd_val.as_str() else {
                continue;
            };
            if cmd.contains("capture-claude.py") {
                found = true;
                let wanted = format!("python3 {}", capture_script.display());
                if cmd != wanted {
                    *cmd_val = serde_json::Value::String(wanted);
                    changed = true;
                }
            }
        }
    }

    if !found {
        hooks.push(new_hook);
        changed = true;
    }

    if changed {
        let updated = serde_json::to_string_pretty(&settings)?;
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&settings_path, updated)?;
        println!(
            "{} Claude Code hook configured in {}",
            "ok".green(),
            settings_path.display()
        );
    } else {
        println!("{} Claude Code hook already present", "--".dimmed());
    }
    Ok(())
}

/// Register agentdiff-mcp as an MCP server in ~/.claude/settings.json.
/// This lets Claude automatically call `record_context` during sessions,
/// enriching ledger entries with intent, trust, files_read, and flags.
fn step_configure_mcp_claude() -> Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");

    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut settings: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.claude/settings.json")?;

    let mcp_servers = settings
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();

    let server_entry = serde_json::json!({
        "command": "agentdiff-mcp",
        "args": [],
        "env": {}
    });

    let already_correct = mcp_servers
        .get("agentdiff")
        .and_then(|e| e.get("command"))
        .and_then(|c| c.as_str())
        == Some("agentdiff-mcp");

    if !already_correct {
        mcp_servers.insert("agentdiff".to_string(), server_entry);
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
        println!(
            "{} agentdiff-mcp registered in {}",
            "ok".green(),
            settings_path.display()
        );
        println!(
            "{}",
            "    Restart Claude Code to activate the MCP server.".dimmed()
        );
    } else {
        println!("{} agentdiff-mcp already registered in Claude Code", "--".dimmed());
    }
    Ok(())
}

fn step_configure_cursor(config: &Config) -> Result<()> {
    let hooks_path = dirs::home_dir().unwrap().join(".cursor").join("hooks.json");
    if !hooks_path.exists() {
        println!(
            "{} ~/.cursor/hooks.json not found — skipping Cursor setup",
            "!".yellow()
        );
        return Ok(());
    }

    let capture_script = config.scripts_root().join("capture-cursor.py");
    let raw = fs::read_to_string(&hooks_path)?;
    let mut hooks_cfg: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.cursor/hooks.json")?;

    let hooks = hooks_cfg
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();

    let capture_cmd = format!("python3 {}", capture_script.display());
    let events = ["afterFileEdit", "afterTabFileEdit", "beforeSubmitPrompt"];

    let mut changed = false;
    for event in events {
        let arr = hooks
            .entry(event)
            .or_insert(serde_json::json!([]))
            .as_array_mut()
            .unwrap();

        let mut found = false;
        for hook in arr.iter_mut() {
            let Some(cmd_val) = hook.get_mut("command") else {
                continue;
            };
            let Some(cmd) = cmd_val.as_str() else {
                continue;
            };
            if cmd.contains("capture-cursor.py") {
                found = true;
                if cmd != capture_cmd {
                    *cmd_val = serde_json::Value::String(capture_cmd.clone());
                    changed = true;
                }
            }
        }

        if !found {
            arr.push(serde_json::json!({ "command": capture_cmd }));
            changed = true;
        }

        // De-duplicate exact command duplicates while preserving order.
        let mut seen = std::collections::HashSet::new();
        arr.retain(|hook| {
            let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) else {
                return true;
            };
            if seen.contains(cmd) {
                changed = true;
                false
            } else {
                seen.insert(cmd.to_string());
                true
            }
        });
    }

    if changed {
        fs::write(&hooks_path, serde_json::to_string_pretty(&hooks_cfg)?)?;
        println!(
            "{} Cursor hooks registered in {}",
            "ok".green(),
            hooks_path.display()
        );
    } else {
        println!("{} Cursor hooks already present", "--".dimmed());
    }
    Ok(())
}

fn step_configure_codex(config: &Config) -> Result<()> {
    let codex_dir = dirs::home_dir().unwrap().join(".codex");
    let config_path = codex_dir.join("config.toml");
    if !codex_dir.exists() && !config_path.exists() {
        println!("{} ~/.codex not found — skipping Codex setup", "!".yellow());
        return Ok(());
    }

    let capture_script = config.scripts_root().join("capture-codex.py");
    let raw = fs::read_to_string(&config_path).unwrap_or_default();
    let mut cfg_val: toml::Value = if raw.trim().is_empty() {
        toml::Value::Table(Default::default())
    } else {
        toml::from_str(&raw).context("parsing ~/.codex/config.toml")?
    };

    let table = cfg_val
        .as_table_mut()
        .context("Codex config root must be a table")?;
    let mut changed = false;

    let current_notify = table.get("notify").and_then(toml_array_to_strings);
    let wanted_base = vec![
        "python3".to_string(),
        capture_script.to_string_lossy().to_string(),
    ];

    let next_notify = match current_notify {
        None => wanted_base.clone(),
        Some(existing) => {
            if existing
                .iter()
                .any(|part| part.contains("capture-codex.py"))
            {
                if let Some(forward_idx) = existing.iter().position(|p| p == "--forward") {
                    let forward = existing.get(forward_idx + 1).cloned().unwrap_or_default();
                    if forward.is_empty() {
                        wanted_base.clone()
                    } else {
                        let mut with_forward = wanted_base.clone();
                        with_forward.push("--forward".to_string());
                        with_forward.push(forward);
                        with_forward
                    }
                } else {
                    wanted_base.clone()
                }
            } else if existing.is_empty() {
                wanted_base.clone()
            } else {
                let forward = serde_json::to_string(&existing)?;
                let mut chained = wanted_base.clone();
                chained.push("--forward".to_string());
                chained.push(forward);
                chained
            }
        }
    };

    if table
        .get("notify")
        .and_then(toml_array_to_strings)
        .unwrap_or_default()
        != next_notify
    {
        table.insert("notify".to_string(), string_array_to_toml(&next_notify));
        changed = true;
    }

    // Ensure Codex hooks are enabled so notify is actually emitted.
    let features = table
        .entry("features".to_string())
        .or_insert(toml::Value::Table(Default::default()));
    let features_table = features
        .as_table_mut()
        .context("~/.codex/config.toml [features] must be a table")?;
    if features_table.get("codex_hooks").and_then(|v| v.as_bool()) != Some(true) {
        features_table.insert("codex_hooks".to_string(), toml::Value::Boolean(true));
        changed = true;
    }

    if changed {
        fs::create_dir_all(&codex_dir)?;
        fs::write(&config_path, toml::to_string_pretty(&cfg_val)?)?;
        println!(
            "{} Codex notify hook configured in {}",
            "ok".green(),
            config_path.display()
        );
    } else {
        println!("{} Codex notify hook already present", "--".dimmed());
    }
    Ok(())
}

fn step_configure_antigravity(config: &Config) -> Result<()> {
    let gemini_dir = dirs::home_dir().unwrap().join(".gemini");
    let settings_path = gemini_dir.join("settings.json");
    if !gemini_dir.exists() && !settings_path.exists() {
        println!(
            "{} ~/.gemini not found — skipping Gemini/Antigravity setup",
            "!".yellow()
        );
        return Ok(());
    }

    let capture_script = config.scripts_root().join("capture-antigravity.py");
    let capture_cmd = format!("python3 {}", capture_script.display());

    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut cfg: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.gemini/settings.json")?;
    let root = cfg
        .as_object_mut()
        .context("~/.gemini/settings.json root must be an object")?;
    let hooks = root
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .context("~/.gemini/settings.json hooks must be an object")?;

    let mut changed = false;
    let events = ["BeforeTool", "AfterTool"];

    for event in events {
        let arr = hooks
            .entry(event)
            .or_insert(serde_json::json!([]))
            .as_array_mut()
            .context("Gemini hook event must be an array")?;

        let mut found_matcher_idx: Option<usize> = None;
        for (idx, item) in arr.iter().enumerate() {
            let matcher = item.get("matcher").and_then(|m| m.as_str()).unwrap_or("");
            if matcher == "write_file|replace" {
                found_matcher_idx = Some(idx);
                break;
            }
        }

        if found_matcher_idx.is_none() {
            arr.push(serde_json::json!({
                "matcher": "write_file|replace",
                "hooks": [{
                    "type": "command",
                    "command": capture_cmd
                }]
            }));
            changed = true;
            continue;
        }

        if let Some(idx) = found_matcher_idx {
            let Some(obj) = arr[idx].as_object_mut() else {
                continue;
            };
            let inner = obj
                .entry("hooks")
                .or_insert(serde_json::json!([]))
                .as_array_mut()
                .context("Gemini hooks entry must contain hooks array")?;

            let mut found_cmd = false;
            for hook in inner.iter_mut() {
                let Some(cmd_val) = hook.get_mut("command") else {
                    continue;
                };
                let Some(cmd) = cmd_val.as_str() else {
                    continue;
                };
                if cmd.contains("capture-antigravity.py") {
                    found_cmd = true;
                    if cmd != capture_cmd {
                        *cmd_val = serde_json::Value::String(capture_cmd.clone());
                        changed = true;
                    }
                }
            }

            if !found_cmd {
                inner.push(serde_json::json!({
                    "type": "command",
                    "command": capture_cmd
                }));
                changed = true;
            }

            let mut seen = std::collections::HashSet::new();
            inner.retain(|hook| {
                let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) else {
                    return true;
                };
                if seen.contains(cmd) {
                    changed = true;
                    false
                } else {
                    seen.insert(cmd.to_string());
                    true
                }
            });
        }
    }

    let tools_obj = root
        .entry("tools")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .context("~/.gemini/settings.json tools must be an object")?;
    if tools_obj
        .get("enableHooks")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        != true
    {
        tools_obj.insert("enableHooks".to_string(), serde_json::Value::Bool(true));
        changed = true;
    }

    if changed {
        fs::create_dir_all(&gemini_dir)?;
        fs::write(&settings_path, serde_json::to_string_pretty(&cfg)?)?;
        println!(
            "{} Gemini/Antigravity hooks configured in {}",
            "ok".green(),
            settings_path.display()
        );
    } else {
        println!(
            "{} Gemini/Antigravity hooks already present",
            "--".dimmed()
        );
    }
    Ok(())
}

fn step_configure_windsurf(config: &Config) -> Result<()> {
    // Use the Windsurf user-level global config (~/.codeium/windsurf/hooks.json).
    // This applies to all repos without needing per-repo setup.
    let hooks_path = dirs::home_dir()
        .unwrap()
        .join(".codeium")
        .join("windsurf")
        .join("hooks.json");
    let capture_script = config.scripts_root().join("capture-windsurf.py");
    let capture_cmd = format!("python3 {}", capture_script.display());

    let raw = fs::read_to_string(&hooks_path).unwrap_or_else(|_| "{}".to_string());
    let mut hooks_cfg: serde_json::Value =
        serde_json::from_str(&raw).context("parsing .windsurf/hooks.json")?;
    let hooks = hooks_cfg
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();

    let events = ["post_write_code", "post_cascade_response_with_transcript"];
    let mut changed = false;

    for event in events {
        let arr = hooks
            .entry(event)
            .or_insert(serde_json::json!([]))
            .as_array_mut()
            .unwrap();

        let mut found = false;
        for hook in arr.iter_mut() {
            let Some(cmd_val) = hook.get_mut("command") else {
                continue;
            };
            let Some(cmd) = cmd_val.as_str() else {
                continue;
            };
            if cmd.contains("capture-windsurf.py") {
                found = true;
                if cmd != capture_cmd {
                    *cmd_val = serde_json::Value::String(capture_cmd.clone());
                    changed = true;
                }
            }
        }

        if !found {
            arr.push(serde_json::json!({
                "type": "command",
                "command": capture_cmd
            }));
            changed = true;
        }

        let mut seen = std::collections::HashSet::new();
        arr.retain(|hook| {
            let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) else {
                return true;
            };
            if seen.contains(cmd) {
                changed = true;
                false
            } else {
                seen.insert(cmd.to_string());
                true
            }
        });
    }

    if changed {
        if let Some(parent) = hooks_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&hooks_path, serde_json::to_string_pretty(&hooks_cfg)?)?;
        println!(
            "{} Windsurf hooks configured in {}",
            "ok".green(),
            hooks_path.display()
        );
    } else {
        println!(
            "{} Windsurf hooks already present in {}",
            "--".dimmed(),
            hooks_path.display()
        );
    }
    Ok(())
}

fn step_configure_opencode(config: &Config) -> Result<()> {
    // Use the OpenCode global plugins directory (~/.config/opencode/plugins/).
    // This applies to all repos without needing per-repo setup.
    // dirs::config_dir() returns ~/.config on Linux, ~/Library/Application Support on macOS.
    let plugins_dir = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("opencode")
        .join("plugins");
    let plugin_path = plugins_dir.join("agentdiff.ts");

    let capture_script = config.scripts_root().join("capture-opencode.py");
    let plugin_content = OPENCODE_PLUGIN_TEMPLATE.replace(
        "__AGENTDIFF_CAPTURE_OPENCODE__",
        &capture_script.to_string_lossy(),
    );

    let existing = fs::read_to_string(&plugin_path).unwrap_or_default();
    if !existing.is_empty()
        && !existing.contains("agentdiff plugin for OpenCode")
        && existing != plugin_content
    {
        println!(
            "{} {} exists and is not managed by agentdiff — skipping",
            "!".yellow(),
            plugin_path.display()
        );
        return Ok(());
    }

    if existing != plugin_content {
        fs::create_dir_all(&plugins_dir)?;
        fs::write(&plugin_path, plugin_content)?;
        println!(
            "{} OpenCode plugin configured in {}",
            "ok".green(),
            plugin_path.display()
        );
    } else {
        println!(
            "{} OpenCode plugin already present in {}",
            "--".dimmed(),
            plugin_path.display()
        );
    }

    Ok(())
}

fn step_configure_copilot(config: &Config) -> Result<()> {
    let capture_script = config.scripts_root().join("capture-copilot.py");
    let ext_js = COPILOT_EXT_JS_TEMPLATE.replace(
        "__AGENTDIFF_CAPTURE_COPILOT__",
        &capture_script.to_string_lossy(),
    );

    // VS Code loads extensions placed directly in ~/.vscode/extensions/<name>-<version>/
    // This works on Linux, macOS, and Windows without any build step or vsce.
    let mut installed_any = false;
    for vscode_dir in vscode_extension_dirs() {
        let ext_dir = vscode_dir.join("agentdiff-copilot-0.1.0");
        if let Err(e) = fs::create_dir_all(&ext_dir) {
            println!(
                "{} cannot create {}: {}",
                "!".yellow(),
                ext_dir.display(),
                e
            );
            continue;
        }

        let pkg_path = ext_dir.join("package.json");
        let js_path = ext_dir.join("extension.js");

        let existing_pkg = fs::read_to_string(&pkg_path).unwrap_or_default();
        let existing_js = fs::read_to_string(&js_path).unwrap_or_default();

        let mut changed = false;
        if existing_pkg != COPILOT_EXT_PACKAGE_JSON {
            fs::write(&pkg_path, COPILOT_EXT_PACKAGE_JSON)
                .with_context(|| format!("writing {}", pkg_path.display()))?;
            changed = true;
        }
        if existing_js != ext_js {
            fs::write(&js_path, &ext_js)
                .with_context(|| format!("writing {}", js_path.display()))?;
            changed = true;
        }

        if changed {
            println!(
                "{} VS Code Copilot extension installed in {}",
                "ok".green(),
                ext_dir.display()
            );
            installed_any = true;
        } else {
            println!(
                "{} VS Code Copilot extension already up-to-date in {}",
                "--".dimmed(),
                ext_dir.display()
            );
            installed_any = true;
        }
    }

    if !installed_any {
        println!(
            "{} VS Code extensions directory not found — skipping Copilot setup",
            "!".yellow()
        );
        println!("    Checked: ~/.vscode-server/extensions, ~/.vscode/extensions, ~/.vscode-insiders/extensions");
        println!(
            "    To install manually: mkdir -p ~/.vscode-server/extensions/agentdiff-copilot-0.1.0 && cp {script_dir}/vscode-extension/* ~/.vscode-server/extensions/agentdiff-copilot-0.1.0/",
            script_dir = capture_script.parent().unwrap_or(capture_script.as_path()).display()
        );
    } else {
        println!("{} Restart VS Code to activate the agentdiff Copilot extension", "!".yellow());
    }

    Ok(())
}

/// Returns VS Code extension directories that exist on this machine.
///
/// Priority order:
/// 1. `~/.vscode-server/extensions`          — WSL2 / Remote-SSH extension host (Linux side).
///    Extensions here run inside WSL, so `python3` and file paths resolve correctly.
/// 2. `~/.vscode-server-insiders/extensions` — VS Code Insiders server variant.
/// 3. `~/.vscode/extensions`                 — VS Code stable on Linux / macOS / Windows.
/// 4. `~/.vscode-insiders/extensions`        — VS Code Insiders on Linux / macOS.
fn vscode_extension_dirs() -> Vec<std::path::PathBuf> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let candidates = vec![
        // WSL2 / Remote-SSH: server-side extension host (highest priority on Linux)
        home.join(".vscode-server").join("extensions"),
        home.join(".vscode-server-insiders").join("extensions"),
        // Regular VS Code on Linux / macOS / Windows
        home.join(".vscode").join("extensions"),
        home.join(".vscode-insiders").join("extensions"),
    ];

    candidates
        .into_iter()
        .filter(|p| p.exists())
        .collect()
}

fn toml_array_to_strings(v: &toml::Value) -> Option<Vec<String>> {
    let arr = v.as_array()?;
    arr.iter()
        .map(|x| x.as_str().map(|s| s.to_string()))
        .collect()
}

fn string_array_to_toml(parts: &[String]) -> toml::Value {
    toml::Value::Array(
        parts
            .iter()
            .map(|p| toml::Value::String(p.clone()))
            .collect(),
    )
}

fn step_register_repo(repo_root: &Path, config: &mut Config) -> Result<()> {
    let slug = Config::slug_for(repo_root);
    let already = config.repos.iter().any(|r| r.slug == slug);
    if !already {
        config.repos.push(RepoConfig {
            path: repo_root.to_path_buf(),
            slug,
        });
        println!("{} Repo registered in config", "ok".green());
    }

    fs::create_dir_all(Config::repo_session_dir(repo_root))?;
    fs::create_dir_all(Config::repo_ledger_dir(repo_root))?;
    let ledger = Config::repo_ledger_path(repo_root);
    if !ledger.exists() {
        fs::write(&ledger, "")?;
    }
    Ok(())
}
