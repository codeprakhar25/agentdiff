use crate::cli::ReportArgs;
use crate::data::Entry;
use crate::store::Store;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub fn run(store: &Store, args: &ReportArgs) -> Result<()> {
    let mut entries = store.load_entries()?;

    if let Some(ref since) = args.since {
        if let Some(since_ts) = chrono::DateTime::parse_from_rfc3339(since)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok()
        {
            entries.retain(|e| e.timestamp >= since_ts);
        }
    }

    if let Some(ref agent) = args.agent {
        entries.retain(|e| e.agent.contains(agent.as_str()));
    }
    if let Some(ref model) = args.model {
        entries.retain(|e| e.model.contains(model.as_str()));
    }

    match args.format {
        crate::cli::ReportFormat::Markdown => {
            let md = markdown_report(&entries)?;
            write_or_stdout(args.out_md.as_deref(), &md)?;
        }
        crate::cli::ReportFormat::Annotations => {
            let text = format_annotations(&entries, args.out_annotations.is_none())?;
            write_or_stdout(args.out_annotations.as_deref(), &text)?;
        }
        crate::cli::ReportFormat::Both => {
            let md = markdown_report(&entries)?;
            let ann_stdout = args.out_annotations.is_none();
            let json = format_annotations(&entries, ann_stdout)?;
            let md_to_file = args.out_md.is_some();
            let ann_to_file = args.out_annotations.is_some();
            write_or_stdout(args.out_md.as_deref(), &md)?;
            if !md_to_file && !ann_to_file {
                writeln!(io::stdout())?;
            }
            write_or_stdout(args.out_annotations.as_deref(), &json)?;
        }
    }

    Ok(())
}

fn write_or_stdout(path: Option<&Path>, content: &str) -> Result<()> {
    match path {
        Some(p) => {
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::write(p, content.as_bytes()).with_context(|| format!("writing {}", p.display()))?;
        }
        None => {
            print!("{content}");
            io::stdout().flush()?;
        }
    }
    Ok(())
}

fn markdown_report(entries: &[Entry]) -> Result<String> {
    let mut out = String::from("# AgentDiff Report\n\n");

    if entries.is_empty() {
        out.push_str("No AI-authored changes detected.\n");
        return Ok(out);
    }

    let mut agent_counts: HashMap<String, usize> = HashMap::new();
    let mut total_lines = 0usize;

    for e in entries {
        let n = e.lines.len();
        *agent_counts.entry(e.agent.clone()).or_insert(0) += n;
        total_lines += n;
    }

    out.push_str("## Summary\n\n");
    out.push_str("| Agent | Lines | % |\n");
    out.push_str("|-------|-------|---|\n");
    let mut agents: Vec<_> = agent_counts.iter().collect();
    agents.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (agent, lines) in agents {
        let pct = if total_lines > 0 {
            (*lines as f64 / total_lines as f64 * 100.0).round() as u32
        } else {
            0
        };
        out.push_str(&format!("| {agent} | {lines} | {pct}% |\n"));
    }

    out.push('\n');
    out.push_str("## Files Modified\n\n");
    out.push_str("| File | Lines | Dominant Agent |\n");
    out.push_str("|------|-------|----------------|\n");

    let file_rows = aggregate_file_dominant(entries);
    for (file, count, agent) in file_rows.iter().take(20) {
        out.push_str(&format!("| {file} | {count} | {agent} |\n"));
    }

    Ok(out)
}

/// Per file: total attributed lines and agent with the largest share (ties → lexicographically smallest agent name).
fn aggregate_file_dominant(entries: &[Entry]) -> Vec<(String, usize, String)> {
    let mut totals: HashMap<String, usize> = HashMap::new();
    let mut by_agent: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for e in entries {
        let n = e.lines.len();
        *totals.entry(e.file.clone()).or_insert(0) += n;
        *by_agent
            .entry(e.file.clone())
            .or_default()
            .entry(e.agent.clone())
            .or_insert(0) += n;
    }

    let mut rows: Vec<(String, usize, String)> = totals
        .into_iter()
        .map(|(file, total)| {
            let dominant = by_agent
                .get(&file)
                .and_then(|agents| {
                    let max_lines = agents.values().copied().max()?;
                    agents
                        .iter()
                        .filter(|(_, c)| **c == max_lines)
                        .map(|(a, _)| a.as_str())
                        .min()
                })
                .map(str::to_string)
                .unwrap_or_else(|| "—".to_string());
            (file, total, dominant)
        })
        .collect();

    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows
}

/// When `for_stdout` is true, prefix with a short heading (human-friendly). Files get a pure JSON array for tooling.
fn format_annotations(entries: &[Entry], for_stdout: bool) -> Result<String> {
    let annotations: Vec<_> = entries
        .iter()
        .flat_map(|e| {
            e.lines.iter().map(|&ln| {
                serde_json::json!({
                    "path": e.file,
                    "start_line": ln,
                    "end_line": ln,
                    "annotation_level": "notice",
                    "message": format!(
                        "AI-authored ({} {}) - Prompt: {}",
                        e.agent,
                        e.model,
                        e.prompt.as_deref().unwrap_or("unknown")
                    )
                })
            })
        })
        .collect();

    let body = serde_json::to_string_pretty(&annotations)?;
    if for_stdout {
        Ok(format!("# GitHub Check Annotations (JSON)\n\n{body}\n"))
    } else {
        Ok(format!("{body}\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_entry(agent: &str, file: &str, lines: Vec<u32>) -> Entry {
        Entry {
            timestamp: Utc::now(),
            agent: agent.to_string(),
            mode: None,
            model: "test-model".to_string(),
            session_id: "s".to_string(),
            tool: "t".to_string(),
            file: file.to_string(),
            abs_file: String::new(),
            prompt: None,
            acceptance: "verbatim".to_string(),
            lines,
            old: None,
            new: None,
            content_preview: None,
            total_lines: None,
            edit_count: None,
            edits: None,
            committed: true,
            commit_hash: "abc".to_string(),
            batch_author: String::new(),
        }
    }

    #[test]
    fn dominant_agent_picks_highest_line_share() {
        let entries = vec![
            sample_entry("cursor", "src/a.rs", vec![1, 2, 3]),
            sample_entry("claude-code", "src/a.rs", vec![4, 5, 6, 7]),
        ];
        let rows = aggregate_file_dominant(&entries);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, 7);
        assert_eq!(rows[0].2, "claude-code");
    }

    #[test]
    fn dominant_agent_tie_breaks_lexicographically() {
        let entries = vec![
            sample_entry("zebra", "f.rs", vec![1, 2]),
            sample_entry("alpha", "f.rs", vec![3, 4]),
        ];
        let rows = aggregate_file_dominant(&entries);
        assert_eq!(rows[0].2, "alpha");
    }
}
