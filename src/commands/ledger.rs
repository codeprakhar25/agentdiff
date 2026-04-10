use crate::cli::{LedgerAction, LedgerArgs};
use crate::store::{self, Store};
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &LedgerArgs) -> Result<()> {
    match args.action {
        LedgerAction::Repair => cmd_repair(store),
        LedgerAction::ImportNotes => cmd_import_notes(),
    }
}

/// Normalize, deduplicate, and re-sort traces on the agentdiff-meta branch.
fn cmd_repair(store: &Store) -> Result<()> {
    let traces = store.load_meta_traces()?;
    if traces.is_empty() {
        println!("{} no traces to repair", "--".dimmed());
        return Ok(());
    }

    let jsonl = store::traces_to_jsonl(&traces)?;
    store::write_to_ref(
        &store.repo_root,
        "refs/heads/agentdiff-meta",
        "traces.jsonl",
        &jsonl,
        "agentdiff: repair",
    )?;

    println!(
        "{} repaired {} traces on agentdiff-meta",
        "ok".green(),
        traces.len()
    );
    Ok(())
}

fn cmd_import_notes() -> Result<()> {
    println!(
        "{} import-notes is no longer supported in v2 (Agent Trace format)",
        "!".yellow()
    );
    println!("  Notes have been replaced by per-branch refs.");
    Ok(())
}
