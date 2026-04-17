use anyhow::Result;

use crate::cli::ConsolidateArgs;
use crate::store::{self, Store};
use crate::util::{ok, warn};

pub fn run(store: &Store, args: &ConsolidateArgs) -> Result<()> {
    let branch = args
        .branch
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--branch is required"))?;

    // Read traces from per-branch ref
    let branch_traces = store.load_branch_traces(branch)?;

    if branch_traces.is_empty() {
        println!("No traces found for branch '{branch}'");
        return Ok(());
    }

    // Read existing meta traces
    let mut meta_traces = store.load_meta_traces()?;
    let existing_ids: std::collections::HashSet<String> =
        meta_traces.iter().map(|t| t.id.clone()).collect();

    // Append only new traces
    let new_traces: Vec<_> = branch_traces
        .into_iter()
        .filter(|t| !existing_ids.contains(&t.id))
        .collect();

    let new_count = new_traces.len();
    meta_traces.extend(new_traces);

    // Write to meta ref (refs/agentdiff/meta — custom namespace, not refs/heads/).
    let jsonl = store::traces_to_jsonl(&meta_traces)?;
    store::write_to_ref(
        &store.repo_root,
        "refs/agentdiff/meta",
        "traces.jsonl",
        &jsonl,
        &format!("agentdiff: consolidate {branch} ({new_count} traces)"),
    )?;

    // Delete the per-branch ref (local)
    let ref_name = store::branch_ref_name(branch);
    if let Err(e) = store::delete_ref(&store.repo_root, &ref_name) {
        eprintln!("  {} could not delete local ref: {e}", warn());
    }

    // Best-effort push to GitHub via the Git Database API + delete remote per-branch ref.
    // Non-fatal: local repos (no GitHub remote) or unauthenticated machines will fail
    // here; the local refs/agentdiff/meta is already written and remains correct.
    if args.push {
        if let Err(e) = store::push_content_to_ref(
            &store.repo_root,
            "refs/agentdiff/meta",
            "traces.jsonl",
            &jsonl,
            &format!("agentdiff: consolidate {branch} ({new_count} traces)"),
        ) {
            let msg = e.to_string();
            if !msg.contains("not a GitHub URL") {
                eprintln!("  {} could not push meta ref to GitHub: {e}", warn());
            }
        } else if let Err(e) = store::delete_remote_ref(&store.repo_root, &ref_name) {
            eprintln!("  {} could not delete remote ref: {e}", warn());
        }
    }

    println!(
        "  {} consolidated {} trace(s) from '{}' into refs/agentdiff/meta ({} total)",
        ok(),
        new_count,
        branch,
        meta_traces.len()
    );

    Ok(())
}
