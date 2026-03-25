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
const WRITE_NOTE_SCRIPT: &str = include_str!("../scripts/write-note.py");

pub fn run_init(
    repo_root: &Path,
    config: &mut Config,
    no_claude: bool,
    no_cursor: bool,
    no_codex: bool,
    no_windsurf: bool,
    no_opencode: bool,
    no_git_hook: bool,
    migrate: bool,
) -> Result<()> {
    println!("{}", "agentdiff init".bold().cyan());
    println!("Repo: {}", repo_root.display());
    println!();

    // Step 1 — create global dirs
    step_create_dirs(config)?;

    // Step 2 — install Python scripts into ~/.agentdiff/scripts/
    step_install_scripts(config)?;

    // Step 3 — install git hooks and notes config
    if !no_git_hook {
        step_install_git_hook(repo_root, config)?;
        step_configure_git_notes(repo_root)?;
    }

    // Step 4 — configure Claude Code ~/.claude/settings.json
    if !no_claude {
        step_configure_claude(config)?;
    }

    // Step 5 — configure Cursor ~/.cursor/hooks.json
    if !no_cursor {
        step_configure_cursor(config)?;
    }

    // Step 6 — configure Codex ~/.codex/config.toml notify hook
    if !no_codex {
        step_configure_codex(config)?;
    }

    // Step 7 — configure Windsurf repo-level .windsurf/hooks.json
    if !no_windsurf {
        step_configure_windsurf(repo_root, config)?;
    }

    // Step 8 — configure OpenCode repo-level plugin
    if !no_opencode {
        step_configure_opencode(repo_root, config)?;
    }

    // Step 9 — register repo in global config
    step_register_repo(repo_root, config)?;

    // Step 10 — save updated config
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

    let hook_path = hooks_dir.join("post-commit");
    let scripts_dir = config.scripts_root();
    let session_log = Config::repo_session_log(repo_root);
    let lockfile = Config::repo_lockfile(repo_root);

    let hook_content = format!(
        r#"#!/usr/bin/env bash
# agentdiff post-commit hook — managed by agentdiff init
# DO NOT EDIT — regenerate with: agentdiff init

set -euo pipefail

REPO_ROOT="{repo_root}"
SESSION_LOG="{session_log}"
LOCKFILE="{lockfile}"
SCRIPTS_DIR="{scripts_dir}"

[ -f "$SESSION_LOG" ] && [ -s "$SESSION_LOG" ] || exit 0
[ -f "$LOCKFILE" ] && exit 0

mkdir -p "$(dirname "$LOCKFILE")"
touch "$LOCKFILE"
trap 'rm -f "$LOCKFILE"' EXIT

python3 "$SCRIPTS_DIR/write-note.py" "$REPO_ROOT" "$SESSION_LOG"
exit 0
"#,
        repo_root = repo_root.display(),
        session_log = session_log.display(),
        lockfile = lockfile.display(),
        scripts_dir = scripts_dir.display(),
    );

    // If a hook already exists that is NOT ours, append rather than overwrite
    if hook_path.exists() {
        let existing = fs::read_to_string(&hook_path)?;
        if !existing.contains("agentdiff post-commit hook") {
            let combined = format!("{}\n\n{}", existing.trim_end(), hook_content);
            fs::write(&hook_path, combined)?;
            println!("{} appended to existing post-commit hook", "ok".green());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
            }
            return Ok(());
        }
    }

    fs::write(&hook_path, &hook_content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
    }
    println!(
        "{} installed post-commit hook at {}",
        "ok".green(),
        hook_path.display()
    );
    Ok(())
}

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

fn step_configure_windsurf(repo_root: &Path, config: &Config) -> Result<()> {
    let hooks_path = repo_root.join(".windsurf").join("hooks.json");
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

fn step_configure_opencode(repo_root: &Path, config: &Config) -> Result<()> {
    let plugin_path = repo_root
        .join(".opencode")
        .join("plugins")
        .join("agentdiff.ts");
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
        if let Some(parent) = plugin_path.parent() {
            fs::create_dir_all(parent)?;
        }
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
    Ok(())
}
