use crate::cli::LogArgs;
use crate::store::Store;
use crate::util::{fmt_lines, fmt_prompt, fmt_time};
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &LogArgs) -> Result<()> {
    let mut entries = store.load_entries()?;

    if let Some(ref agent) = args.agent {
        entries.retain(|e| e.agent.contains(agent.as_str()));
    }

    entries.sort_by_key(|e| e.timestamp);
    entries.reverse(); // newest first

    let display_entries = entries.iter().take(args.limit);

    println!();
    println!(
        "  {} — last {} entries\n",
        "agentdiff log".cyan().bold(),
        args.limit
    );

    for e in display_entries {
        let agent_color = crate::util::agent_color_str(&e.agent);
        let commit = if e.commit_hash.len() > 8 {
            &e.commit_hash[..8]
        } else {
            &e.commit_hash
        };
        let uncommitted = if !e.committed {
            " (uncommitted)".yellow()
        } else {
            "".normal()
        };

        println!(
            "  {} {} {:<12} {} → {}{}",
            commit.dimmed(),
            fmt_time(&e.timestamp.to_rfc3339()),
            agent_color,
            e.tool.dimmed(),
            e.file,
            uncommitted
        );

        if args.full_prompt {
            if let Some(ref prompt) = e.prompt {
                if !prompt.is_empty() && prompt != "unknown" {
                    println!("    Prompt: {}", fmt_prompt(prompt, 80));
                }
            }
        }

        println!(
            "    Lines: {} | Model: {}",
            fmt_lines(&e.lines),
            e.model.dimmed()
        );
    }

    println!();
    Ok(())
}
