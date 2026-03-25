use crate::cli::ListArgs;
use crate::store::Store;
use crate::util::{fmt_lines, fmt_prompt, fmt_time};
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &ListArgs) -> Result<()> {
    let mut entries = store.load_entries()?;

    if args.uncommitted {
        entries.retain(|e| !e.committed);
    }
    if let Some(ref agent) = args.agent {
        entries.retain(|e| e.agent.contains(agent.as_str()));
    }
    if let Some(ref file) = args.file {
        entries.retain(|e| e.file.contains(file.as_str()));
    }
    if let Some(limit) = args.limit {
        entries.truncate(limit);
    }

    println!();
    println!(
        "  {} — {} entries",
        "agentdiff list".cyan().bold(),
        entries.len()
    );
    println!();

    // Column headers
    let hdr = format!(
        "  {:<4} {:<10} {:<13} {:<13} {:<22} {:<10} {:<32} {:<8} PROMPT",
        "#", "COMMIT", "TIME", "AGENT", "MODEL", "TOOL", "FILE", "LINES"
    );
    println!("{}", hdr.dimmed());
    println!("  {}", "─".repeat(135).dimmed());

    for (i, e) in entries.iter().enumerate() {
        let commit = if e.commit_hash.len() > 8 {
            &e.commit_hash[..8]
        } else {
            &e.commit_hash
        };
        let agent_color = crate::util::agent_color_str(&e.agent);
        let line_str = fmt_lines(&e.lines);
        let prompt_str = fmt_prompt(e.prompt.as_deref().unwrap_or("unknown"), 45);
        let uncommitted = if !e.committed {
            " *".yellow().to_string()
        } else {
            "  ".to_string()
        };

        println!(
            "  {:<4} {:<10} {:<13} {:<13} {:<22} {:<10} {:<32} {:<8} {}{}",
            i + 1,
            commit.dimmed(),
            fmt_time(&e.timestamp.to_rfc3339()),
            agent_color,
            e.model,
            e.tool,
            e.file,
            line_str,
            prompt_str,
            uncommitted
        );
    }
    println!();
    Ok(())
}
