use anyhow::Result;
use colored::Colorize;

use crate::keys;
use crate::store::Store;

pub fn run(store: &Store) -> Result<()> {
    println!("{}", "agentdiff status".bold().cyan());
    println!();

    print_init_status(store);
    print_keys_status();
    print_traces_status(store)?;
    print_hook_status(store);
    print_agent_hook_status();
    print_unpushed_status(store);

    Ok(())
}

fn print_init_status(store: &Store) {
    if store.is_initialized() {
        println!("  {} repo             initialized (.git/agentdiff/ exists)", "ok".green());
    } else {
        println!(
            "  {} repo             not initialized — run {} to start tracking",
            "!".yellow(),
            "agentdiff init".cyan()
        );
    }
}

fn print_keys_status() {
    let priv_path = match keys::private_key_path() {
        Ok(p) => p,
        Err(_) => {
            println!("  {} signing keys  cannot resolve home dir", "?".yellow());
            return;
        }
    };
    let pub_path = match keys::public_key_path() {
        Ok(p) => p,
        Err(_) => return,
    };

    let priv_ok = priv_path.exists();
    let pub_ok = pub_path.exists();

    #[cfg(unix)]
    let perms_ok = {
        use std::os::unix::fs::MetadataExt;
        priv_path
            .metadata()
            .map(|m| m.mode() & 0o077 == 0)
            .unwrap_or(false)
    };
    #[cfg(not(unix))]
    let perms_ok = true;

    if priv_ok && pub_ok {
        let key_id = keys::load_verifying_key()
            .map(|vk| keys::compute_key_id(&vk))
            .unwrap_or_else(|_| "error reading".to_string());
        let perm_label = if perms_ok {
            "chmod 600 ok"
        } else {
            "chmod 600 MISSING"
        };
        let perm_color = if perms_ok {
            perm_label.green().to_string()
        } else {
            perm_label.red().to_string()
        };
        println!(
            "  {} signing keys  key_id={} ({})",
            "ok".green(),
            key_id,
            perm_color
        );
    } else if !priv_ok && !pub_ok {
        println!(
            "  {} signing keys  not initialized — run 'agentdiff keys init'",
            "!".yellow()
        );
    } else {
        println!(
            "  {} signing keys  partial — private={} public={}",
            "!".yellow(),
            if priv_ok { "ok" } else { "missing" },
            if pub_ok { "ok" } else { "missing" }
        );
    }
}

fn print_traces_status(store: &Store) -> Result<()> {
    let traces = store.load_meta_traces()?;

    if traces.is_empty() {
        println!(
            "  {} traces         none on agentdiff-meta",
            "--".dimmed()
        );
        return Ok(());
    }

    let total = traces.len();
    let signed = traces.iter().filter(|t| t.sig.is_some()).count();
    let last = traces.last().unwrap();
    let last_ts = last.timestamp.format("%Y-%m-%d %H:%M:%SZ");
    let last_id = &last.id[..last.id.len().min(8)];

    println!(
        "  {} traces         {} entries ({}/{} signed), last: {} ({})",
        "ok".green(),
        total,
        signed,
        total,
        last_id,
        last_ts
    );

    Ok(())
}

fn print_hook_status(store: &Store) {
    let hooks_dir = store.repo_root.join(".git").join("hooks");
    let pre = hooks_dir.join("pre-commit");
    let post = hooks_dir.join("post-commit");
    let pre_push = hooks_dir.join("pre-push");

    let pre_ok = pre.exists()
        && std::fs::read_to_string(&pre)
            .map(|s| s.contains("agentdiff"))
            .unwrap_or(false);
    let post_ok = post.exists()
        && std::fs::read_to_string(&post)
            .map(|s| s.contains("agentdiff"))
            .unwrap_or(false);
    let pre_push_ok = pre_push.exists()
        && std::fs::read_to_string(&pre_push)
            .map(|s| s.contains("agentdiff"))
            .unwrap_or(false);

    if pre_ok && post_ok && pre_push_ok {
        println!(
            "  {} git hooks      pre-commit + post-commit + pre-push installed",
            "ok".green()
        );
    } else {
        println!(
            "  {} git hooks      pre-commit={} post-commit={} pre-push={} — run 'agentdiff init'",
            "!".yellow(),
            if pre_ok { "ok" } else { "missing" },
            if post_ok { "ok" } else { "missing" },
            if pre_push_ok { "ok" } else { "missing" }
        );
    }
}

fn print_agent_hook_status() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };

    struct AgentCheck {
        name: &'static str,
        config_path_parts: &'static [&'static str],
        marker: &'static str,
    }

    let checks = [
        AgentCheck {
            name: "claude-code",
            config_path_parts: &[".claude", "settings.json"],
            marker: "capture-claude",
        },
        AgentCheck {
            name: "cursor",
            config_path_parts: &[".cursor", "hooks.json"],
            marker: "capture-cursor",
        },
        AgentCheck {
            name: "codex",
            config_path_parts: &[".codex", "config.toml"],
            marker: "capture-codex",
        },
        AgentCheck {
            name: "windsurf",
            config_path_parts: &[".codeium", "windsurf", "hooks.json"],
            marker: "capture-windsurf",
        },
    ];

    // OpenCode uses dirs::config_dir() (platform-aware: ~/Library/Application Support on macOS).
    let opencode_path = dirs::config_dir().map(|d| d.join("opencode").join("plugins").join("agentdiff.ts"));

    let mut any_checked = false;
    let mut any_missing = false;

    // Check the struct-based agents.
    for check in &checks {
        let path = check
            .config_path_parts
            .iter()
            .fold(home.clone(), |p, part| p.join(part));
        if !path.exists() {
            continue;
        }
        any_checked = true;
        let registered = std::fs::read_to_string(&path)
            .map(|s| s.contains(check.marker))
            .unwrap_or(false);
        if registered {
            println!("  {} agent hook     {} registered", "ok".green(), check.name);
        } else {
            println!(
                "  {} agent hook     {} config found but agentdiff hook missing — re-run 'agentdiff configure'",
                "!".yellow(),
                check.name
            );
            any_missing = true;
        }
    }

    // Gemini CLI + Antigravity: two separate products sharing ~/.gemini/
    {
        let gemini_dir = home.join(".gemini");
        if gemini_dir.exists() {
            any_checked = true;
            let cli_ok = std::fs::read_to_string(gemini_dir.join("settings.json"))
                .map(|s| s.contains("capture-antigravity"))
                .unwrap_or(false);
            let rule_ok = std::fs::read_to_string(gemini_dir.join("GEMINI.md"))
                .map(|s| s.contains("agentdiff: managed block"))
                .unwrap_or(false);
            match (cli_ok, rule_ok) {
                (true, true) => {
                    println!("  {} agent hook     gemini-cli registered; antigravity rule set", "ok".green());
                }
                (true, false) => {
                    println!("  {} agent hook     gemini-cli registered", "ok".green());
                    println!(
                        "  {} agent hook     antigravity GEMINI.md rule missing — re-run 'agentdiff configure'",
                        "!".yellow()
                    );
                    any_missing = true;
                }
                (false, true) => {
                    println!(
                        "  {} agent hook     gemini-cli hooks missing — re-run 'agentdiff configure'",
                        "!".yellow()
                    );
                    println!("  {} agent hook     antigravity rule set", "ok".green());
                    any_missing = true;
                }
                (false, false) => {
                    println!(
                        "  {} agent hook     gemini/antigravity config found but hooks missing — re-run 'agentdiff configure'",
                        "!".yellow()
                    );
                    any_missing = true;
                }
            }
        }
    }

    // OpenCode: platform-aware path via dirs::config_dir()
    if let Some(ref ocp) = opencode_path {
        if ocp.exists() {
            any_checked = true;
            let registered = std::fs::read_to_string(ocp)
                .map(|s| s.contains("agentdiff"))
                .unwrap_or(false);
            if registered {
                println!("  {} agent hook     opencode registered", "ok".green());
            } else {
                println!(
                    "  {} agent hook     opencode plugin found but agentdiff missing — re-run 'agentdiff configure'",
                    "!".yellow()
                );
                any_missing = true;
            }
        }
    }

    // Copilot: check for agentdiff extension directory in VS Code extensions paths.
    // Check all dirs — the extension may be installed in only one (e.g., vscode-server on remote).
    let mut copilot_found = false;
    let mut copilot_checked = false;
    for vscode_dir in &[".vscode/extensions", ".vscode-server/extensions", ".vscode-insiders/extensions"] {
        let ext_dir = home.join(vscode_dir);
        if !ext_dir.exists() {
            continue;
        }
        copilot_checked = true;
        any_checked = true;
        if std::fs::read_dir(&ext_dir)
            .map(|d| d.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("agentdiff-copilot")))
            .unwrap_or(false)
        {
            copilot_found = true;
        }
    }
    if copilot_checked {
        if copilot_found {
            println!("  {} agent hook     copilot registered", "ok".green());
        } else {
            println!(
                "  {} agent hook     copilot extension not found — re-run 'agentdiff configure'",
                "!".yellow()
            );
            any_missing = true;
        }
    }

    if !any_checked {
        println!(
            "  {} agent hooks    no AI agent configs found — run 'agentdiff configure'",
            "--".dimmed()
        );
    } else if any_missing {
        println!(
            "  {}",
            "Re-run 'agentdiff configure' to restore missing hooks.".yellow()
        );
    }
}

fn print_unpushed_status(store: &Store) {
    let branch = match store.current_branch() {
        Ok(b) => b,
        Err(_) => return,
    };

    let local_path = store.local_traces_path(&branch);
    if !local_path.exists() {
        return;
    }

    if let Ok(raw) = std::fs::read_to_string(&local_path) {
        let count = raw.lines().filter(|l| !l.trim().is_empty()).count();
        if count > 0 {
            println!(
                "  {} unpushed       {} trace(s) for branch '{}' — run 'agentdiff push'",
                "!".yellow(),
                count,
                branch
            );
        }
    }
}
