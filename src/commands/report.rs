use crate::cli::{ReportArgs, ReportFormat};
use crate::data::Entry;
use crate::store::Store;
use crate::util::{agent_color_str, print_command_header, print_not_initialized};
use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub fn run(store: &Store, args: &ReportArgs) -> Result<()> {
    // Text format reads like the old `stats` — needs initialization check + header.
    if matches!(args.format, ReportFormat::Text) && !store.is_initialized() {
        print_not_initialized();
        return Ok(());
    }

    // JSONL format wants pure machine output — skip even the filter step noise.
    if matches!(args.format, ReportFormat::Jsonl) {
        return run_jsonl(store, args);
    }

    let mut entries = store.load_entries()?;

    if let Some(ref since) = args.since {
        if let Ok(since_ts) =
            chrono::DateTime::parse_from_rfc3339(since).map(|dt| dt.with_timezone(&chrono::Utc))
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

    // --post-pr-comment: generate markdown and post via gh CLI regardless of --format.
    if let Some(ref pr_arg) = args.post_pr_comment {
        let md = markdown_report(&entries)?;
        return post_pr_comment(&store.repo_root, &md, *pr_arg);
    }

    match args.format {
        ReportFormat::Text => run_text(store, &entries, args),
        ReportFormat::Markdown => {
            let md = markdown_report(&entries)?;
            write_or_stdout(args.out.as_deref(), &md)
        }
        ReportFormat::Annotations => {
            let json = format_annotations(&entries, args.out.is_none())?;
            write_or_stdout(args.out.as_deref(), &json)
        }
        ReportFormat::Jsonl => unreachable!(),
    }
}

fn post_pr_comment(repo_root: &Path, body: &str, pr_number: Option<u64>) -> Result<()> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("gh");
    cmd.arg("pr").arg("comment");

    if let Some(n) = pr_number {
        cmd.arg(n.to_string());
    }

    cmd.args(["--body", body, "--edit-last", "--create-if-none"])
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status().context("running gh pr comment")?;
    if !status.success() {
        anyhow::bail!("gh pr comment failed (exit {})", status);
    }
    Ok(())
}

// ── Text (replaces `stats`) ──────────────────────────────────────────────────

fn run_text(store: &Store, entries: &[Entry], args: &ReportArgs) -> Result<()> {
    let mut agent_lines: HashMap<String, u32> = HashMap::new();
    let mut file_agent_lines: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut ai_lines_by_file: HashMap<String, HashSet<u32>> = HashMap::new();

    for e in entries {
        let agent = &e.agent;
        for &ln in &e.lines {
            *agent_lines.entry(agent.clone()).or_default() += 1;
            ai_lines_by_file
                .entry(e.file.clone())
                .or_default()
                .insert(ln);
            *file_agent_lines
                .entry(e.file.clone())
                .or_default()
                .entry(agent.clone())
                .or_default() += 1;
        }
    }

    for (file, ai_set) in &ai_lines_by_file {
        let abs = store.repo_root.join(file);
        if let Ok(content) = fs::read_to_string(&abs) {
            let total = content.lines().count() as u32;
            let human = total.saturating_sub(ai_set.len() as u32);
            if human > 0 {
                *agent_lines.entry("human".into()).or_default() += human;
                *file_agent_lines
                    .entry(file.clone())
                    .or_default()
                    .entry("human".into())
                    .or_default() += human;
            }
        }
    }

    let total_lines: u32 = agent_lines.values().sum();

    print_command_header("report");
    println!("  Total lines tracked: {}", total_lines);
    println!();

    println!("  By Agent:");
    let mut sorted: Vec<_> = agent_lines.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (agent, count) in sorted {
        let pct = pct(*count, total_lines);
        let bar = "█".repeat((pct / 5) as usize);
        println!(
            "    {} {:>6} ({:>3}%) {}",
            agent_color_str(agent),
            count,
            pct,
            bar.dimmed()
        );
    }

    if args.by_file {
        println!();
        println!("  By File:");
        let mut file_sorted: Vec<_> = file_agent_lines.iter().collect();
        file_sorted.sort_by(|a, b| {
            let a_total: u32 = a.1.values().sum();
            let b_total: u32 = b.1.values().sum();
            b_total.cmp(&a_total)
        });

        for (file, agents) in file_sorted.iter().take(10) {
            let file_total: u32 = agents.values().sum();
            let ai_total: u32 = agents
                .iter()
                .filter(|(a, _)| *a != "human")
                .map(|(_, c)| c)
                .sum();
            let ai_pct = pct(ai_total, file_total);

            let dominant = agents
                .iter()
                .max_by_key(|(_, c)| *c)
                .map(|(a, _)| a.clone())
                .unwrap_or_else(|| "human".to_string());

            println!(
                "    {:<40} {:>5} lines ({:>3}% AI) — {}",
                file,
                file_total,
                ai_pct,
                agent_color_str(&dominant)
            );
        }
    }

    if args.by_model {
        println!();
        println!("  By Model:");
        let mut model_counts: HashMap<String, u32> = HashMap::new();
        for e in entries {
            *model_counts.entry(e.model.clone()).or_default() += e.lines.len() as u32;
        }
        let mut model_sorted: Vec<_> = model_counts.iter().collect();
        model_sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (model, count) in model_sorted.iter().take(10) {
            println!(
                "    {:<30} {:>6} ({:>3}%)",
                model,
                count,
                pct(**count, total_lines)
            );
        }
    }

    println!();
    Ok(())
}

fn pct(part: u32, whole: u32) -> u32 {
    if whole == 0 {
        0
    } else {
        (part as f64 / whole as f64 * 100.0) as u32
    }
}

// ── JSONL (replaces `export`) ────────────────────────────────────────────────

fn run_jsonl(store: &Store, args: &ReportArgs) -> Result<()> {
    let traces = store.load_all_traces()?;
    let mut buf = String::new();
    for trace in &traces {
        buf.push_str(&serde_json::to_string(trace)?);
        buf.push('\n');
    }
    match &args.out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::write(path, buf.as_bytes())
                .with_context(|| format!("writing {}", path.display()))?;
        }
        None => {
            print!("{buf}");
            io::stdout().flush()?;
        }
    }
    Ok(())
}

// ── Markdown + GitHub annotations (original `report` formats) ────────────────

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
