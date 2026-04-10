/// `agentdiff migrate` — legacy command, now a no-op with guidance.
use anyhow::Result;
use colored::Colorize;

use crate::store::Store;

pub fn run(_store: &Store) -> Result<()> {
    println!(
        "{} The migrate command is no longer needed in v2.",
        "!".yellow()
    );
    println!("  agentdiff now uses Agent Trace format with per-branch refs.");
    println!("  Traces are stored on the agentdiff-meta branch (traces.jsonl).");
    println!();
    println!("  To push local traces:       agentdiff push");
    println!("  To consolidate after merge:  agentdiff consolidate --branch <name>");
    Ok(())
}
