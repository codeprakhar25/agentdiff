use crate::cli::ListArgs;
use crate::data::AgentTrace;
use crate::store::Store;
use crate::util::{fmt_lines, fmt_prompt, fmt_time, print_command_header, print_not_initialized};
use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

pub fn run(store: &Store, args: &ListArgs) -> Result<()> {
    if !store.is_initialized() {
        print_not_initialized();
        return Ok(());
    }

    if args.uncommitted {
        return run_uncommitted(store, args);
    }

    if args.by_commit {
        return run_by_commit(store, args);
    }

    let mut traces = store.load_all_traces()?;
    if traces.is_empty() {
        print_command_header("list");
        println!("  no traces found");
        println!("  Use an AI agent to edit files, then commit.");
        println!(
            "  Run {} to verify hooks and agent capture are active.",
            "agentdiff status".cyan()
        );
        println!();
        return Ok(());
    }

    if let Some(ref agent) = args.agent {
        traces.retain(|t| t.agent_name().contains(agent.as_str()));
    }
    if let Some(ref file) = args.file {
        traces.retain(|t| t.files.iter().any(|f| f.path.contains(file.as_str())));
    }
    if let Some(limit) = args.limit {
        traces.truncate(limit);
    }

    print_command_header("list");
    println!("  {} entries", traces.len());
    println!();

    let hdr = format!(
        "  {:<4} {:<10} {:<13} {:<14} {:<22} {:<32} {:<18} {:<8} PROMPT",
        "#", "COMMIT", "TIME", "AGENT", "MODEL", "FILE(S)", "LINES", "TRUST"
    );
    println!("{}", hdr.dimmed());
    println!("  {}", "─".repeat(140).dimmed());

    for (i, t) in traces.iter().enumerate() {
        let commit = {
            let sha = t.sha();
            if sha.len() > 8 { &sha[..8] } else { sha }
        };
        let meta = t.agentdiff_metadata();
        let trust = meta
            .as_ref()
            .and_then(|m| m.trust)
            .map(|t| t.to_string())
            .unwrap_or_else(|| "—".to_string());
        let prompt_text = meta
            .as_ref()
            .and_then(|m| m.prompt_excerpt.clone())
            .unwrap_or_default();
        let prompt = fmt_prompt(&prompt_text, 45);
        let model = trim_text(&first_model(t), 22);
        let files_col = fmt_files_col(t);
        let lines_col = fmt_lines_col(t);

        println!(
            "  {:<4} {:<10} {:<13} {:<14} {:<22} {:<32} {:<18} {:<8} {}",
            i + 1,
            commit.dimmed(),
            fmt_time(&t.timestamp.to_rfc3339()),
            crate::util::agent_color_str(t.agent_name()),
            model,
            files_col,
            lines_col,
            trust,
            prompt
        );
    }
    println!();
    Ok(())
}

fn run_uncommitted(store: &Store, args: &ListArgs) -> Result<()> {
    let mut entries = store.load_entries()?;
    entries.retain(|e| !e.committed);

    if let Some(ref agent) = args.agent {
        entries.retain(|e| e.agent.contains(agent.as_str()));
    }
    if let Some(ref file) = args.file {
        entries.retain(|e| e.file.contains(file.as_str()));
    }
    if let Some(limit) = args.limit {
        entries.truncate(limit);
    }

    print_command_header("list");
    println!("  {} uncommitted entries", entries.len());
    println!();

    let hdr = format!(
        "  {:<4} {:<10} {:<13} {:<13} {:<22} {:<10} {:<36} {:<8} PROMPT",
        "#", "COMMIT", "TIME", "AGENT", "MODEL", "TOOL", "FILE", "LINES"
    );
    println!("{}", hdr.dimmed());
    println!("  {}", "─".repeat(140).dimmed());

    for (i, e) in entries.iter().enumerate() {
        let line_str = fmt_lines(&e.lines);
        let prompt_str = fmt_prompt(e.prompt.as_deref().unwrap_or("unknown"), 45);
        println!(
            "  {:<4} {:<10} {:<13} {:<13} {:<22} {:<10} {:<36} {:<8} {}",
            i + 1,
            "(pending)".yellow(),
            fmt_time(&e.timestamp.to_rfc3339()),
            crate::util::agent_color_str(&e.agent),
            e.model,
            e.tool,
            e.file,
            line_str,
            prompt_str
        );
    }
    println!();
    Ok(())
}

fn run_by_commit(store: &Store, args: &ListArgs) -> Result<()> {
    let mut entries = store.load_entries()?;

    if let Some(ref agent) = args.agent {
        entries.retain(|e| e.agent.contains(agent.as_str()));
    }
    if let Some(ref file) = args.file {
        entries.retain(|e| e.file.contains(file.as_str()));
    }

    entries.sort_by_key(|e| e.timestamp);
    entries.reverse(); // newest first

    // Group by commit, preserving newest-first order.
    let mut seen: Vec<String> = Vec::new();
    let mut by_commit: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        let key = if e.commit_hash.is_empty() {
            "uncommitted".to_string()
        } else {
            e.commit_hash.clone()
        };
        if !by_commit.contains_key(&key) {
            seen.push(key.clone());
        }
        by_commit.entry(key).or_default().push(i);
    }

    let limit = args.limit.unwrap_or(20);

    print_command_header("list");
    println!("  last {} commits", limit);
    println!();

    for commit_key in seen.iter().take(limit) {
        let idxs = &by_commit[commit_key];
        let first = &entries[idxs[0]];

        let commit_short = if commit_key == "uncommitted" {
            "untracked".to_string()
        } else if commit_key.len() > 8 {
            commit_key[..8].to_string()
        } else {
            commit_key.clone()
        };

        let uncommitted_tag = if !first.committed {
            " (uncommitted)".yellow()
        } else {
            "".normal()
        };

        println!(
            "  {} {}{}",
            commit_short.dimmed(),
            fmt_time(&first.timestamp.to_rfc3339()),
            uncommitted_tag
        );

        // Group files by agent within this commit.
        let mut agent_files: HashMap<&str, Vec<&str>> = HashMap::new();
        for &i in idxs {
            let e = &entries[i];
            agent_files
                .entry(e.agent.as_str())
                .or_default()
                .push(e.file.as_str());
        }

        let multi_agent = agent_files.len() > 1;
        let mut agents: Vec<&str> = agent_files.keys().copied().collect();
        agents.sort();
        for agent in agents {
            let files = &agent_files[agent];
            let agent_col = crate::util::agent_color_str(agent);
            let max_files = if multi_agent { 3 } else { 4 };
            let file_list = if files.len() <= max_files {
                files.join(", ")
            } else {
                format!(
                    "{} +{} more",
                    files[..max_files].join(", "),
                    files.len() - max_files
                )
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

fn first_model(trace: &AgentTrace) -> String {
    trace
        .files
        .iter()
        .flat_map(|f| &f.conversations)
        .find_map(|c| c.contributor.model_id.clone())
        .unwrap_or_default()
}

/// Show first filename with "+N" suffix if there are more files.
fn fmt_files_col(trace: &AgentTrace) -> String {
    let files = &trace.files;
    if files.is_empty() {
        return "—".to_string();
    }
    let first = short_path(&files[0].path, 24);
    if files.len() == 1 {
        first
    } else {
        format!("{} +{}", first, files.len() - 1)
    }
}

/// Show line ranges across all files in a compact form.
fn fmt_lines_col(trace: &AgentTrace) -> String {
    let mut parts: Vec<String> = Vec::new();
    for file in &trace.files {
        for conv in &file.conversations {
            for range in &conv.ranges {
                let lo = range.start_line.min(range.end_line);
                let hi = range.start_line.max(range.end_line);
                if lo == hi {
                    parts.push(lo.to_string());
                } else {
                    parts.push(format!("{lo}-{hi}"));
                }
            }
        }
    }
    if parts.is_empty() {
        return "—".to_string();
    }
    let combined = parts.join(", ");
    trim_text(&combined, 18)
}

/// Shorten a path to max_len chars, truncating from the left with '…' if needed.
fn short_path(path: &str, max_len: usize) -> String {
    if path.chars().count() <= max_len {
        return path.to_string();
    }
    let chars: Vec<char> = path.chars().collect();
    let keep = max_len.saturating_sub(1);
    let start = chars.len().saturating_sub(keep);
    format!("…{}", chars[start..].iter().collect::<String>())
}

fn trim_text(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= width.saturating_sub(1) {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}
