use crate::cli::LogArgs;
use crate::store::Store;
use crate::util::{fmt_prompt, fmt_time};
use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

pub fn run(store: &Store, args: &LogArgs) -> Result<()> {
    let mut entries = store.load_entries()?;

    if let Some(ref agent) = args.agent {
        entries.retain(|e| e.agent.contains(agent.as_str()));
    }

    entries.sort_by_key(|e| e.timestamp);
    entries.reverse(); // newest first

    // Group by commit hash, preserving newest-first order.
    let mut seen_commits: Vec<String> = Vec::new();
    let mut by_commit: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        let key = if e.commit_hash.is_empty() {
            "uncommitted".to_string()
        } else {
            e.commit_hash.clone()
        };
        if !by_commit.contains_key(&key) {
            seen_commits.push(key.clone());
        }
        by_commit.entry(key).or_default().push(i);
    }

    let display_commits = seen_commits.iter().take(args.limit);

    println!();
    println!(
        "  {} — last {} commits\n",
        "agentdiff log".cyan().bold(),
        args.limit
    );

    for commit_key in display_commits {
        let idxs = &by_commit[commit_key];
        let first = &entries[idxs[0]];

        let commit_short = if commit_key == "uncommitted" {
            "untracked".to_string()
        } else if commit_key.len() > 8 {
            commit_key[..8].to_string()
        } else {
            commit_key.clone()
        };

        let uncommitted = if !first.committed {
            " (uncommitted)".yellow()
        } else {
            "".normal()
        };

        println!(
            "  {} {}{}",
            commit_short.dimmed(),
            fmt_time(&first.timestamp.to_rfc3339()),
            uncommitted
        );

        // Group files by agent within this commit.
        let mut agent_files: HashMap<&str, Vec<&str>> = HashMap::new();
        for &i in idxs {
            let e = &entries[i];
            agent_files.entry(e.agent.as_str()).or_default().push(e.file.as_str());
        }

        if agent_files.len() > 1 {
            // Multi-agent commit — show each agent with their files.
            let mut agents: Vec<&str> = agent_files.keys().copied().collect();
            agents.sort();
            for agent in agents {
                let files = &agent_files[agent];
                let agent_col = crate::util::agent_color_str(agent);
                let file_list = if files.len() <= 3 {
                    files.join(", ")
                } else {
                    format!("{} +{} more", files[..3].join(", "), files.len() - 3)
                };
                println!("    {} {}", agent_col, file_list.dimmed());
            }
        } else {
            // Single agent — compact file list.
            let (agent, files) = agent_files.iter().next().unwrap();
            let agent_col = crate::util::agent_color_str(agent);
            let file_list = if files.len() <= 4 {
                files.join(", ")
            } else {
                format!("{} +{} more", files[..4].join(", "), files.len() - 4)
            };
            println!("    {} {}", agent_col, file_list.dimmed());
        }

        if args.full_prompt {
            if let Some(ref prompt) = first.prompt {
                if !prompt.is_empty() && prompt != "unknown" {
                    println!("    Prompt: {}", fmt_prompt(prompt, 80));
                }
            }
        }

        println!();
    }

    Ok(())
}
