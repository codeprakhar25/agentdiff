use anyhow::Result;
use colored::Colorize;
use std::process::Command;

use crate::cli::StatusArgs;
use crate::keys;
use crate::store::{self, Store};
use crate::util::{dim, ok, print_command_header, warn};

pub fn run(store: &Store, args: &StatusArgs) -> Result<()> {
    if args.remote {
        return run_remote(store, args);
    }

    print_command_header("status");

    print_init_status(store);
    print_keys_status();
    print_traces_status(store)?;
    print_hook_status(store);
    print_agent_hook_status();
    print_unpushed_status(store);

    Ok(())
}

/// Renders a status prefix padded to a fixed width so columns line up.
/// Widths: ok=2, warn=4, error=5 → pad everything to 5 chars.
fn prefix(label: colored::ColoredString) -> String {
    let visible_len = strip_ansi(&label.to_string()).chars().count();
    let pad = 5usize.saturating_sub(visible_len);
    format!("{}{}", label, " ".repeat(pad))
}

fn strip_ansi(s: &str) -> String {
    // cheap ANSI escape stripper for length calculation
    let mut out = String::with_capacity(s.len());
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c == 'm' {
                in_esc = false;
            }
        } else if c == '\u{1b}' {
            in_esc = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn print_init_status(store: &Store) {
    if store.is_initialized() {
        println!(
            "  {} repo             initialized (.git/agentdiff/ exists)",
            prefix(ok())
        );
    } else {
        println!(
            "  {} repo             not initialized — run {} to start tracking",
            prefix(warn()),
            "agentdiff init".cyan()
        );
    }
}

fn print_keys_status() {
    let priv_path = match keys::private_key_path() {
        Ok(p) => p,
        Err(_) => {
            println!("  {} signing keys  cannot resolve home dir", prefix(warn()));
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
            prefix(ok()),
            key_id,
            perm_color
        );
    } else if !priv_ok && !pub_ok {
        println!(
            "  {} signing keys  not initialized — run '{}'",
            prefix(warn()),
            "agentdiff keys init".cyan()
        );
    } else {
        println!(
            "  {} signing keys  partial — private={} public={}",
            prefix(warn()),
            if priv_ok { "ok" } else { "missing" },
            if pub_ok { "ok" } else { "missing" }
        );
    }
}

fn print_traces_status(store: &Store) -> Result<()> {
    let traces = store.load_meta_traces()?;

    if traces.is_empty() {
        println!(
            "  {} traces         none in refs/agentdiff/meta",
            prefix(dim())
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
        prefix(ok()),
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
            prefix(ok())
        );
    } else {
        println!(
            "  {} git hooks      pre-commit={} post-commit={} pre-push={} — run '{}'",
            prefix(warn()),
            if pre_ok { "ok" } else { "missing" },
            if post_ok { "ok" } else { "missing" },
            if pre_push_ok { "ok" } else { "missing" },
            "agentdiff init".cyan()
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
            println!("  {} agent hook     {} registered", prefix(ok()), check.name);
        } else {
            println!(
                "  {} agent hook     {} config found but agentdiff hook missing — re-run 'agentdiff configure'",
                prefix(warn()),
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
                    println!("  {} agent hook     gemini-cli registered; antigravity rule set", prefix(ok()));
                }
                (true, false) => {
                    println!("  {} agent hook     gemini-cli registered", prefix(ok()));
                    println!(
                        "  {} agent hook     antigravity GEMINI.md rule missing — re-run 'agentdiff configure'",
                        prefix(warn())
                    );
                    any_missing = true;
                }
                (false, true) => {
                    println!(
                        "  {} agent hook     gemini-cli hooks missing — re-run 'agentdiff configure'",
                        prefix(warn())
                    );
                    println!("  {} agent hook     antigravity rule set", prefix(ok()));
                    any_missing = true;
                }
                (false, false) => {
                    println!(
                        "  {} agent hook     gemini/antigravity config found but hooks missing — re-run 'agentdiff configure'",
                        prefix(warn())
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
                println!("  {} agent hook     opencode registered", prefix(ok()));
            } else {
                println!(
                    "  {} agent hook     opencode plugin found but agentdiff missing — re-run 'agentdiff configure'",
                    prefix(warn())
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
            println!("  {} agent hook     copilot registered", prefix(ok()));
        } else {
            println!(
                "  {} agent hook     copilot extension not found — re-run 'agentdiff configure'",
                prefix(warn())
            );
            any_missing = true;
        }
    }

    if !any_checked {
        println!(
            "  {} agent hooks    no AI agent configs found — run 'agentdiff configure'",
            prefix(dim())
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
                "  {} unpushed       {} trace(s) for branch '{}' — run '{}'",
                prefix(warn()),
                count,
                branch,
                "agentdiff push".cyan()
            );
        }
    }
}

// ── Remote status (--remote) ────────────────────────────────────────────────

fn run_remote(store: &Store, args: &StatusArgs) -> Result<()> {
    let ls_out = Command::new("git")
        .args(["ls-remote", "origin", "refs/agentdiff/*"])
        .current_dir(&store.repo_root)
        .output();

    let remote_refs: Vec<(String, String)> = match ls_out {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, '\t');
                let sha = parts.next()?.trim().to_string();
                let refname = parts.next()?.trim().to_string();
                Some((sha, refname))
            })
            .collect(),
        Ok(_) => Vec::new(),
        Err(e) => anyhow::bail!("git ls-remote failed: {e}"),
    };

    let remote_label = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(&store.repo_root)
        .output()
        .ok()
        .and_then(|o| {
            o.status
                .success()
                .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_else(|| "origin".to_string());

    print_command_header("status --remote");
    println!("  {}", remote_label.dimmed());
    println!();

    if remote_refs.is_empty() {
        println!("  {} no agentdiff refs found on remote", dim());
        println!();
        println!("  Push local traces with: {}", "agentdiff push".cyan());
        println!();
        return Ok(());
    }

    let hdr = format!("  {:<45} {:<10} {}", "REF", "TRACES", "LOCAL");
    println!("{}", hdr.dimmed());
    println!("  {}", "─".repeat(72).dimmed());

    for (sha, refname) in &remote_refs {
        let short_sha = if sha.len() >= 8 { &sha[..8] } else { sha };

        let trace_count = if !args.no_fetch {
            fetch_trace_count(&store.repo_root, refname)
        } else {
            None
        };

        let count_str = match trace_count {
            Some(n) => format!("{n}"),
            None => short_sha.to_string(),
        };

        let local_str = local_ref_status(store, refname);

        println!(
            "  {:<45} {:<10} {}",
            refname.cyan(),
            count_str,
            local_str.dimmed()
        );
    }

    if let Ok(branch) = store.current_branch() {
        let local_path = store.local_traces_path(&branch);
        if local_path.exists() {
            let local_traces = store.load_local_traces(&branch).unwrap_or_default();
            let branch_ref = store::branch_ref_name(&branch);
            let on_remote = remote_refs.iter().any(|(_, r)| r == &branch_ref);
            if !on_remote && !local_traces.is_empty() {
                println!();
                println!(
                    "  {} {} local trace(s) for '{}' not yet pushed — run: {}",
                    warn(),
                    local_traces.len(),
                    branch,
                    "agentdiff push".cyan()
                );
            }
        }
    }

    println!();
    Ok(())
}

fn fetch_trace_count(repo_root: &std::path::Path, ref_name: &str) -> Option<usize> {
    if let Some(n) = count_local_ref(repo_root, ref_name) {
        return Some(n);
    }
    store::fetch_ref_content_via_api(repo_root, ref_name, "traces.jsonl")
        .ok()
        .flatten()
        .map(|content| content.lines().filter(|l| !l.trim().is_empty()).count())
}

fn count_local_ref(repo_root: &std::path::Path, ref_name: &str) -> Option<usize> {
    let spec = format!("{ref_name}:traces.jsonl");
    let out = Command::new("git")
        .args(["show", &spec])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let content = String::from_utf8_lossy(&out.stdout);
    Some(content.lines().filter(|l| !l.trim().is_empty()).count())
}

fn local_ref_status(store: &Store, ref_name: &str) -> String {
    let local_sha = Command::new("git")
        .args(["rev-parse", ref_name])
        .current_dir(&store.repo_root)
        .output()
        .ok()
        .and_then(|o| {
            o.status
                .success()
                .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });

    match local_sha {
        Some(_) => "synced".to_string(),
        None => {
            if ref_name.starts_with("refs/agentdiff/traces/") {
                let branch_part = ref_name.trim_start_matches("refs/agentdiff/traces/");
                let local_path = store.local_traces_path(branch_part);
                if local_path.exists() {
                    return "local buffer only (run: agentdiff push)".to_string();
                }
            }
            "not fetched locally".to_string()
        }
    }
}
