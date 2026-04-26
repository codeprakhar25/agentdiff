use anyhow::Result;

use crate::cli::PushArgs;
use crate::store::{self, Store};
use crate::util::ok;

pub fn run(store: &Store, args: &PushArgs) -> Result<()> {
    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => store.current_branch()?,
    };

    let local_path = store.local_traces_path(&branch);
    if !local_path.exists() {
        if !args.quiet {
            println!("No local traces for branch '{branch}'");
        }
        return Ok(());
    }

    // Read local traces
    let local_traces = store.load_local_traces(&branch)?;
    if local_traces.is_empty() {
        if !args.quiet {
            println!("No local traces for branch '{branch}'");
        }
        return Ok(());
    }

    // Read existing remote traces via GitHub API (avoids needing a prior git fetch)
    let ref_name = store::branch_ref_name(&branch);
    let mut remote_read_failed = false;
    let remote_traces_raw =
        match store::fetch_ref_content_via_api(&store.repo_root, &ref_name, "traces.jsonl") {
            Ok(raw) => raw,
            Err(e) => {
                let msg = e.to_string();
                remote_read_failed = true;
                if !msg.contains("not a GitHub URL") && !args.quiet {
                    eprintln!("agentdiff: warn — could not read remote traces from GitHub: {e}");
                }
                None
            }
        };
    let mut remote_traces = if let Some(raw) = remote_traces_raw {
        store::parse_traces_from_jsonl(&raw)
    } else {
        Vec::new()
    };

    // Merge: add local traces not already in remote (dedup by UUID)
    let remote_ids: std::collections::HashSet<String> =
        remote_traces.iter().map(|t| t.id.clone()).collect();
    let new_traces: Vec<_> = local_traces
        .into_iter()
        .filter(|t| !remote_ids.contains(&t.id))
        .collect();

    let new_count = new_traces.len();
    remote_traces.extend(new_traces);

    let jsonl = store::traces_to_jsonl(&remote_traces)?;

    // Always write the local ref first — consolidate reads from this ref and
    // it must be present even when there is no GitHub remote.
    if let Err(e) = store::write_to_ref(
        &store.repo_root,
        &ref_name,
        "traces.jsonl",
        &jsonl,
        &format!("agentdiff: traces for {branch}"),
    ) {
        if !args.quiet {
            eprintln!("agentdiff: warn — could not write local trace ref: {e}");
        }
    }

    if new_count == 0 {
        prune_local_traces(&local_path, args.quiet)?;
        if !args.quiet {
            println!(
                "  {} traces for branch '{}' already up to date ({} total on ref)",
                ok(),
                branch,
                remote_traces.len()
            );
        }
        return Ok(());
    }

    // Best-effort push to GitHub via the Git Database API.
    // Non-fatal: local repos (no GitHub remote) or unauthenticated machines
    // will fail here but the local ref is already updated above.
    let remote_pushed = if remote_read_failed {
        false
    } else {
        match store::push_content_to_ref(
            &store.repo_root,
            &ref_name,
            "traces.jsonl",
            &jsonl,
            &format!("agentdiff: traces for {branch}"),
        ) {
            Ok(_) => true,
            Err(e) => {
                // Suppress "not a GitHub URL" noise for local repos; warn on real errors.
                let msg = e.to_string();
                if !msg.contains("not a GitHub URL") && !args.quiet {
                    eprintln!("agentdiff: warn — could not push traces to GitHub: {e}");
                }
                false
            }
        }
    };

    if remote_pushed {
        prune_local_traces(&local_path, args.quiet)?;
    }

    if !args.quiet {
        println!(
            "  {} pushed {} trace(s) for branch '{}' ({} total on ref)",
            ok(),
            new_count,
            branch,
            remote_traces.len()
        );
    }

    Ok(())
}

fn prune_local_traces(path: &std::path::Path, quiet: bool) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    std::fs::remove_file(path)?;
    if !quiet {
        println!("  {} cleared local trace buffer", ok());
    }
    Ok(())
}
