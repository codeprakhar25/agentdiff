use crate::cli::ListArgs;
use crate::data::AgentTrace;
use crate::store::Store;
use crate::util::{fmt_lines, fmt_prompt, fmt_time};
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &ListArgs) -> Result<()> {
    if args.uncommitted {
        return run_uncommitted(store, args);
    }

    let mut traces = store.load_all_traces()?;
    if traces.is_empty() {
        println!("\n  {} — no traces found\n", "agentdiff list".cyan().bold());
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

    println!();
    println!(
        "  {} — {} entries",
        "agentdiff list".cyan().bold(),
        traces.len()
    );
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

    println!();
    println!(
        "  {} — {} uncommitted entries",
        "agentdiff list".cyan().bold(),
        entries.len()
    );
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
