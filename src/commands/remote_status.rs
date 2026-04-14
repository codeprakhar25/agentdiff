use anyhow::Result;
use colored::Colorize;
use std::process::Command;

use crate::cli::RemoteStatusArgs;
use crate::store::{self, Store};

pub fn run(store: &Store, args: &RemoteStatusArgs) -> Result<()> {
    // 1. List all agentdiff refs on the remote via git ls-remote.
    //    This works with any git remote (no GitHub API required).
    let ls_out = Command::new("git")
        .args(["ls-remote", "origin", "refs/agentdiff/*"])
        .current_dir(&store.repo_root)
        .output();

    let remote_refs: Vec<(String, String)> = match ls_out {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|line| {
                    let mut parts = line.splitn(2, '\t');
                    let sha = parts.next()?.trim().to_string();
                    let refname = parts.next()?.trim().to_string();
                    Some((sha, refname))
                })
                .collect()
        }
        Ok(_) => {
            // ls-remote failed — likely no remote or no access
            Vec::new()
        }
        Err(e) => {
            anyhow::bail!("git ls-remote failed: {e}");
        }
    };

    // 2. Try to get the remote URL for display.
    let remote_label = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(&store.repo_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "origin".to_string());

    println!();
    println!(
        "  {} — {}",
        "agentdiff remote-status".cyan().bold(),
        remote_label.dimmed()
    );
    println!();

    if remote_refs.is_empty() {
        println!("  {} no agentdiff refs found on remote", "--".dimmed());
        println!();
        println!(
            "  {}",
            "Push local traces with: agentdiff push && git push".dimmed()
        );
        println!();
        return Ok(());
    }

    // 3. For each remote ref, optionally fetch trace count and compare with local.
    let hdr = format!("  {:<45} {:<10} {}", "REF", "TRACES", "LOCAL");
    println!("{}", hdr.dimmed());
    println!("  {}", "─".repeat(72).dimmed());

    for (sha, refname) in &remote_refs {
        let short_sha = if sha.len() >= 8 { &sha[..8] } else { sha };

        // Optionally fetch content to count traces.
        let trace_count = if !args.no_fetch {
            fetch_trace_count(&store.repo_root, refname)
        } else {
            None
        };

        let count_str = match trace_count {
            Some(n) => format!("{n}"),
            None => short_sha.to_string(),
        };

        // Compare with local ref.
        let local_str = local_ref_status(&store, refname);

        println!(
            "  {:<45} {:<10} {}",
            refname.cyan(),
            count_str,
            local_str.dimmed()
        );
    }

    // 4. Show unpushed local traces (in file buffer but not yet on remote).
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
                    "!".yellow(),
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

/// Fetch JSONL content from a ref and count trace lines.
fn fetch_trace_count(repo_root: &std::path::Path, ref_name: &str) -> Option<usize> {
    // Try local ref first (fast, no network).
    let local_count = count_local_ref(repo_root, ref_name);
    if local_count.is_some() {
        return local_count;
    }

    // Fall back to GitHub API.
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

/// Describe the local state of a ref relative to remote.
fn local_ref_status(store: &Store, ref_name: &str) -> String {
    // Check if local ref exists and matches remote.
    let local_sha = Command::new("git")
        .args(["rev-parse", ref_name])
        .current_dir(&store.repo_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        });

    match local_sha {
        Some(_) => "synced".to_string(),
        None => {
            // Check if there's a local file buffer for this ref.
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
