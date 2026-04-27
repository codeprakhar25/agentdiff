use crate::cli::ContextArgs;
use crate::data::{AgentTrace, TraceFile};
use crate::store::Store;
use crate::util::{agent_color_str, print_command_header, print_not_initialized};
use anyhow::Result;
use colored::Colorize;
use std::path::Path;

pub fn run(store: &Store, args: &ContextArgs) -> Result<()> {
    if !store.is_initialized() {
        print_not_initialized();
        return Ok(());
    }

    let rel_path = normalize_path(&args.file);
    let mut traces: Vec<AgentTrace> = store
        .load_all_traces()?
        .into_iter()
        .filter(|trace| trace.files.iter().any(|file| file.path == rel_path))
        .collect();

    if let Some(agent) = &args.agent {
        traces.retain(|trace| trace.agent_name().contains(agent));
    }

    traces.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    traces.truncate(args.limit);

    if args.json {
        let body = context_json(&rel_path, &traces)?;
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    print_command_header("context");
    println!("  {}", rel_path.bold());
    println!();

    if traces.is_empty() {
        println!("  No AgentDiff context found for this file.");
        println!();
        return Ok(());
    }

    for trace in &traces {
        let meta = trace.agentdiff_metadata();
        let intent = meta
            .as_ref()
            .and_then(|m| m.intent.as_deref())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("unspecified");
        println!(
            "  {} {} {}",
            short_id(&trace.id).dimmed(),
            agent_color_str(trace.agent_name()),
            trace.timestamp.to_rfc3339().dimmed()
        );
        println!("    Intent: {}", intent);
        if let Some(prompt) = meta
            .as_ref()
            .and_then(|m| m.prompt_excerpt.as_deref())
            .filter(|p| !p.trim().is_empty())
        {
            println!("    Prompt: {}", prompt);
        }
        if let Some(files_read) = meta
            .as_ref()
            .map(|m| m.files_read.as_slice())
            .filter(|files| !files.is_empty())
        {
            println!("    Files read: {}", files_read.join(", "));
        }
        if let Some(flags) = meta
            .as_ref()
            .map(|m| m.flags.as_slice())
            .filter(|flags| !flags.is_empty())
        {
            println!("    Flags: {}", flags.join(", "));
        }
        for file in trace.files.iter().filter(|file| file.path == rel_path) {
            println!("    Ranges: {}", format_ranges(file));
        }
        println!();
    }

    Ok(())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn context_json(file: &str, traces: &[AgentTrace]) -> Result<serde_json::Value> {
    let values: Vec<_> = traces
        .iter()
        .map(|trace| {
            let meta = trace.agentdiff_metadata();
            serde_json::json!({
                "id": trace.id,
                "short_id": short_id(&trace.id),
                "timestamp": trace.timestamp,
                "sha": trace.sha(),
                "agent": trace.agent_name(),
                "intent": meta.as_ref().and_then(|m| m.intent.clone()),
                "prompt_excerpt": meta.as_ref().and_then(|m| m.prompt_excerpt.clone()),
                "files_read": meta.as_ref().map(|m| m.files_read.clone()).unwrap_or_default(),
                "flags": meta.as_ref().map(|m| m.flags.clone()).unwrap_or_default(),
                "trust": meta.as_ref().and_then(|m| m.trust),
                "ranges": trace.files.iter()
                    .filter(|f| f.path == file)
                    .flat_map(|f| f.conversations.iter())
                    .flat_map(|c| c.ranges.iter())
                    .map(|r| serde_json::json!({
                        "start_line": r.start_line,
                        "end_line": r.end_line,
                    }))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    Ok(serde_json::json!({
        "file": file,
        "traces": values,
    }))
}

fn format_ranges(file: &TraceFile) -> String {
    let ranges: Vec<String> = file
        .conversations
        .iter()
        .flat_map(|conv| conv.ranges.iter())
        .map(|range| {
            if range.start_line == range.end_line {
                range.start_line.to_string()
            } else {
                format!("{}-{}", range.start_line, range.end_line)
            }
        })
        .collect();
    if ranges.is_empty() {
        "unknown".to_string()
    } else {
        ranges.join(", ")
    }
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}
