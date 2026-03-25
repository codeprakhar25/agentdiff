use crate::cli::ShowArgs;
use crate::store::Store;
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &ShowArgs) -> Result<()> {
    let Some(record) = store.find_ledger_record(&args.sha)? else {
        anyhow::bail!("No ledger entry found for SHA/prefix: {}", args.sha);
    };

    println!();
    println!(
        "  {} — {}",
        "agentdiff show".cyan().bold(),
        record.sha.dimmed()
    );
    println!();
    println!("  Timestamp: {}", record.ts.to_rfc3339());
    println!(
        "  Author: {}",
        record.author.unwrap_or_else(|| "unknown".into())
    );
    println!("  Agent: {}", crate::util::agent_color_str(&record.agent));
    println!("  Model: {}", record.model);
    println!("  Session: {}", record.session_id);
    println!("  Prompt: {}", record.prompt_excerpt);
    println!("  Prompt Hash: {}", record.prompt_hash.dimmed());
    if let Some(intent) = record.intent {
        if !intent.is_empty() {
            println!("  Intent: {}", intent);
        }
    }
    if let Some(trust) = record.trust {
        println!("  Trust: {}", trust);
    }
    if !record.flags.is_empty() {
        println!("  Flags: {}", record.flags.join(", "));
    }
    if !record.files_read.is_empty() {
        println!("  Files Read: {}", record.files_read.join(", "));
    }
    println!();
    println!("  Files:");
    for path in &record.files_touched {
        let ranges = record.lines.get(path).cloned().unwrap_or_default();
        let range_text = if ranges.is_empty() {
            "—".to_string()
        } else {
            ranges
                .iter()
                .map(|(a, b)| {
                    if a == b {
                        a.to_string()
                    } else {
                        format!("{a}-{b}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("    - {}  [{}]", path, range_text.dimmed());
    }
    println!();
    Ok(())
}
