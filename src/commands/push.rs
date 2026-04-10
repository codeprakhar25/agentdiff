use anyhow::Result;
use colored::Colorize;

use crate::cli::PushArgs;
use crate::store::{self, Store};

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
    let remote_traces_raw = store::fetch_ref_content_via_api(
        &store.repo_root,
        &ref_name,
        "traces.jsonl",
    )
    .unwrap_or(None);
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

    // Push all traces to GitHub via Git Database API (blob → tree → commit → ref)
    let jsonl = store::traces_to_jsonl(&remote_traces)?;
    store::push_content_to_ref(
        &store.repo_root,
        &ref_name,
        "traces.jsonl",
        &jsonl,
        &format!("agentdiff: traces for {branch}"),
    )?;

    // Mirror the merged content to the local ref so that `agentdiff consolidate`
    // (which reads via git-show) can run immediately without a separate git fetch.
    let _ = store::write_to_ref(
        &store.repo_root,
        &ref_name,
        "traces.jsonl",
        &jsonl,
        &format!("agentdiff: traces for {branch}"),
    );

    if !args.quiet {
        println!(
            "{} Pushed {} trace(s) for branch '{}' ({} total on ref)",
            "ok".green(),
            new_count,
            branch,
            remote_traces.len()
        );
    }

    Ok(())
}
