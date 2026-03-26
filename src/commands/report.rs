use crate::cli::ReportArgs;
use crate::store::Store;
use crate::util::agent_color_str;
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &ReportArgs) -> Result<()> {
    let entries = store.load_entries()?;

    // Filter by timestamp if requested
    let filtered: Vec<_> = if let Some(ref since) = args.since {
        let since_ts = chrono::DateTime::parse_from_rfc3339(since)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok();
        if let Some(since) = since_ts {
            entries
                .into_iter()
                .filter(|e| e.timestamp >= since)
                .collect()
        } else {
            entries
        }
    } else {
        entries
    };

    // TODO: filter by agent/model if requested
    match args.format {
        crate::cli::ReportFormat::Markdown => {
            render_markdown(&filtered, &store.repo_root)?;
        }
        crate::cli::ReportFormat::Annotations => {
            render_annotations(&filtered)?;
        }
        crate::cli::ReportFormat::Both => {
            render_markdown(&filtered, &store.repo_root)?;
            println!();
            render_annotations(&filtered)?;
        }
    }

    Ok(())
}

fn render_markdown(entries: &[crate::data::Entry], _repo_root: &std::path::Path) -> Result<()> {
    println!("# AgentDiff Report\n");

    if entries.is_empty() {
        println!("No AI-authored changes detected.");
        return Ok(());
    }

    // Summary stats
    let mut agent_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut model_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut total_lines = 0;

    for e in entries {
        *agent_counts.entry(e.agent.clone()).or_insert(0) += e.lines.len();
        *model_counts.entry(e.model.clone()).or_insert(0) += e.lines.len();
        total_lines += e.lines.len();
    }

    println!("## Summary\n");
    println!("| Agent | Lines | % |");
    println!("|-------|-------|---|");
    for (agent, lines) in agent_counts.iter() {
        let pct = (*lines as f64 / total_lines as f64 * 100.0) as u32;
        println!("| {} | {} | {}% |", agent, lines, pct);
    }

    println!();
    println!("## Files Modified\n");
    let mut file_counts: std::collections::HashMap<String, (usize, String)> =
        std::collections::HashMap::new();
    for e in entries {
        let dominant = e.agent.clone();
        let entry = file_counts
            .entry(e.file.clone())
            .or_insert((0, dominant.clone()));
        entry.0 += e.lines.len();
    }

    println!("| File | Lines | Dominant Agent |");
    println!("|------|-------|----------------|");
    let mut sorted_files: Vec<_> = file_counts.iter().collect();
    sorted_files.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    for (file, (count, agent)) in sorted_files.iter().take(20) {
        println!("| {} | {} | {} |", file, count, agent);
    }

    Ok(())
}

fn render_annotations(entries: &[crate::data::Entry]) -> Result<()> {
    println!("# GitHub Check Annotations (JSON)\n");

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
                        e.agent, e.model, e.prompt.as_deref().unwrap_or("unknown")
                    )
                })
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&annotations)?);
    Ok(())
}
