/// Legacy sync-notes command — replaced by per-branch refs in v2.
use colored::Colorize;

use crate::store::Store;

pub fn run(_store: &Store) -> anyhow::Result<()> {
    println!(
        "{} sync-notes is no longer needed in v2.",
        "!".yellow()
    );
    println!("  agentdiff now uses per-branch refs (refs/agentdiff/traces/*).");
    println!("  Run 'agentdiff init' to configure fetch refspecs.");
    Ok(())
}
