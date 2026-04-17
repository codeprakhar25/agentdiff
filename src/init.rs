use crate::config::{Config, RepoConfig};
use crate::util::{ok, print_command_header, warn};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::{fs, path::Path, process::Command};

/// Initialize agentdiff in this repository — installs git hooks and creates the ledger.
/// Run `agentdiff configure` first to set up global agent hooks.
pub fn run_init(repo_root: &Path, config: &mut Config, no_git_hook: bool) -> Result<()> {
    print_command_header("init");
    println!("  Repo: {}", repo_root.display());
    println!();

    // Warn if configure hasn't been run yet (scripts dir empty or missing).
    let scripts_dir = config.scripts_root();
    let capture_claude = scripts_dir.join("capture-claude.py");
    if !capture_claude.exists() {
        println!(
            "  {} agent hooks not configured — run '{}' first",
            warn(),
            "agentdiff configure".cyan()
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
        "  {} config written to {}",
        ok(),
        Config::config_path().display()
    );

    println!();
    println!("  {}", "init complete".bold().green());
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
        "  {} installed git hooks (pre-commit, post-commit, pre-push)",
        ok()
    );
    Ok(())
}

/// Configure fetch refspec for per-branch refs: +refs/agentdiff/*:refs/agentdiff/*
fn step_configure_refspec(repo_root: &Path) -> Result<()> {
    let fetch_spec = "+refs/agentdiff/*:refs/agentdiff/*";

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
            println!("  {} added fetch refspec for refs/agentdiff/*", ok());
        } else {
            println!(
                "  {} could not add fetch refspec (no remote origin?)",
                warn()
            );
        }
    } else {
        println!(
            "  {} fetch refspec for refs/agentdiff/* already present",
            crate::util::dim()
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
            println!("  {} appended to existing {}", ok(), path.display());
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

fn step_register_repo(repo_root: &Path, config: &mut Config) -> Result<()> {
    let slug = Config::slug_for(repo_root);
    let already = config.repos.iter().any(|r| r.slug == slug);
    if !already {
        config.repos.push(RepoConfig {
            path: repo_root.to_path_buf(),
            slug,
        });
        println!("  {} repo registered in config", ok());
    }

    fs::create_dir_all(Config::repo_session_dir(repo_root))?;
    Ok(())
}
