use crate::cli::{ReportArgs, ReportFormat};
use crate::data::{AgentTrace, Entry};
use crate::store::Store;
use crate::util::{agent_color_str, print_command_header, print_not_initialized};
use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

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
    let mut traces = store.load_all_traces()?;

    if let Some(ref since) = args.since {
        if let Ok(since_ts) =
            chrono::DateTime::parse_from_rfc3339(since).map(|dt| dt.with_timezone(&chrono::Utc))
        {
            entries.retain(|e| e.timestamp >= since_ts);
            traces.retain(|t| t.timestamp >= since_ts);
        }
    }
    if let Some(ref agent) = args.agent {
        entries.retain(|e| e.agent.contains(agent.as_str()));
        traces.retain(|t| t.agent_name().contains(agent.as_str()));
    }
    if let Some(ref model) = args.model {
        entries.retain(|e| e.model.contains(model.as_str()));
        traces.retain(|t| trace_models(t).iter().any(|m| m.contains(model.as_str())));
    }

    // --post-pr-comment: generate markdown and post via gh CLI regardless of --format.
    if let Some(ref pr_arg) = args.post_pr_comment {
        if let Some(pr_shas) = current_branch_commits(&store.repo_root) {
            traces = filter_traces_by_commit_set(&traces, &pr_shas);
        }
        let md = markdown_trace_report(&traces, true)?;
        return post_pr_comment(&store.repo_root, &md, *pr_arg);
    }

    match args.format {
        ReportFormat::Text => run_text(store, &entries, args),
        ReportFormat::Markdown => {
            let md = if args.context {
                markdown_trace_report(&traces, true)?
            } else {
                markdown_report(&entries)?
            };
            write_or_stdout(args.out.as_deref(), &md)
        }
        ReportFormat::Json => {
            let json = serde_json::to_string_pretty(&context_json_report(&traces)?)?;
            write_or_stdout(args.out.as_deref(), &format!("{json}\n"))
        }
        ReportFormat::Annotations => {
            let json = format_annotations(&entries, args.out.is_none())?;
            write_or_stdout(args.out.as_deref(), &json)
        }
        ReportFormat::Jsonl => unreachable!(),
    }
}

fn post_pr_comment(repo_root: &Path, body: &str, pr_number: Option<u64>) -> Result<()> {
    let edit = run_gh_pr_comment(repo_root, body, pr_number, true)
        .context("running gh pr comment --edit-last")?;
    if edit.status.success() {
        return Ok(());
    }

    let create =
        run_gh_pr_comment(repo_root, body, pr_number, false).context("running gh pr comment")?;
    if create.status.success() {
        return Ok(());
    }

    anyhow::bail!(
        "gh pr comment failed; edit-last stderr: {}; create stderr: {}",
        String::from_utf8_lossy(&edit.stderr).trim(),
        String::from_utf8_lossy(&create.stderr).trim()
    );
}

fn run_gh_pr_comment(
    repo_root: &Path,
    body: &str,
    pr_number: Option<u64>,
    edit_last: bool,
) -> Result<std::process::Output> {
    let mut cmd = Command::new("gh");
    cmd.arg("pr").arg("comment");

    if let Some(n) = pr_number {
        cmd.arg(n.to_string());
    }

    cmd.args(["--body", body]);
    if edit_last {
        cmd.arg("--edit-last");
    }
    cmd.current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("running gh pr comment")
}

fn current_branch_commits(repo_root: &Path) -> Option<HashSet<String>> {
    let base = find_merge_base(repo_root)?;
    let out = std::process::Command::new("git")
        .args(["rev-list", &format!("{base}..HEAD")])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let shas = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    Some(shas)
}

fn find_merge_base(repo_root: &Path) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(remote_head) = remote_default_branch(repo_root) {
        candidates.push(remote_head);
    }
    for branch in ["origin/main", "origin/master", "main", "master"] {
        if !candidates.iter().any(|b| b == branch) {
            candidates.push(branch.to_string());
        }
    }

    for branch in candidates {
        let Ok(out) = std::process::Command::new("git")
            .args(["merge-base", "HEAD", &branch])
            .current_dir(repo_root)
            .output()
        else {
            continue;
        };
        if out.status.success() {
            let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
            if !sha.is_empty() {
                return Some(sha);
            }
        }
    }
    None
}

fn remote_default_branch(repo_root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let branch = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch.trim_start_matches("refs/remotes/").to_string())
    }
}

fn filter_traces_by_commit_set(
    traces: &[AgentTrace],
    commit_shas: &HashSet<String>,
) -> Vec<AgentTrace> {
    traces
        .iter()
        .filter(|trace| commit_shas.contains(trace.sha()))
        .cloned()
        .collect()
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

#[derive(Default)]
struct ContextGroup {
    intent: String,
    lines: usize,
    files: HashSet<String>,
    agents: HashSet<String>,
    models: HashSet<String>,
    files_read: HashSet<String>,
    prompts: HashSet<String>,
    flags: HashSet<String>,
    trace_ids: Vec<String>,
    max_trust: Option<u8>,
}

#[derive(Default)]
struct FileContextRow {
    lines: usize,
    agents: HashMap<String, usize>,
    intents: HashSet<String>,
    trace_ids: Vec<String>,
}

fn markdown_trace_report(traces: &[AgentTrace], include_context: bool) -> Result<String> {
    let mut out = String::from("# AgentDiff Report\n\n");
    if traces.is_empty() {
        out.push_str("No AgentDiff traces found.\n");
        return Ok(out);
    }

    let mut agent_counts: HashMap<String, usize> = HashMap::new();
    let mut file_rows: HashMap<String, FileContextRow> = HashMap::new();
    let mut context_groups: HashMap<String, ContextGroup> = HashMap::new();
    let mut total_lines = 0usize;

    for trace in traces {
        let agent = trace.agent_name().to_string();
        let meta = trace.agentdiff_metadata();
        let intent = meta
            .as_ref()
            .and_then(|m| m.intent.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "unspecified".to_string());
        let line_count = trace_line_count(trace);
        total_lines += line_count;
        *agent_counts.entry(agent.clone()).or_default() += line_count;

        let group = context_groups
            .entry(intent.clone())
            .or_insert_with(|| ContextGroup {
                intent: intent.clone(),
                ..Default::default()
            });
        group.lines += line_count;
        group.agents.insert(agent.clone());
        group.trace_ids.push(short_id(&trace.id).to_string());

        for model in trace_models(trace) {
            if !model.is_empty() {
                group.models.insert(model);
            }
        }

        if let Some(meta) = meta.as_ref() {
            for file in &meta.files_read {
                group.files_read.insert(file.clone());
            }
            for flag in &meta.flags {
                group.flags.insert(flag.clone());
            }
            if let Some(prompt) = meta
                .prompt_excerpt
                .as_ref()
                .filter(|p| !p.trim().is_empty())
            {
                group.prompts.insert(prompt.clone());
            }
            if let Some(trust) = meta.trust {
                group.max_trust = Some(group.max_trust.map_or(trust, |v| v.max(trust)));
            }
        }

        for file in &trace.files {
            group.files.insert(file.path.clone());
            let file_line_count = trace_file_line_count(file);
            let row = file_rows.entry(file.path.clone()).or_default();
            row.lines += file_line_count;
            *row.agents.entry(agent.clone()).or_default() += file_line_count;
            row.intents.insert(intent.clone());
            row.trace_ids.push(short_id(&trace.id).to_string());
        }
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
        out.push_str(&format!("| {} | {} | {}% |\n", md_cell(agent), lines, pct));
    }

    if include_context {
        out.push_str("\n## Review Context\n\n");
        let mut groups: Vec<_> = context_groups.values().collect();
        groups.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.intent.cmp(&b.intent)));
        for group in groups.iter().take(5) {
            out.push_str(&format!(
                "- Intent: {} ({} lines, {} file{})\n",
                group.intent,
                group.lines,
                group.files.len(),
                if group.files.len() == 1 { "" } else { "s" }
            ));
            out.push_str(&format!(
                "  - Agent/model: {} / {}\n",
                limited_join(&group.agents, 4),
                limited_join(&group.models, 4)
            ));
            if !group.files_read.is_empty() {
                out.push_str(&format!(
                    "  - Files read: {}\n",
                    limited_join(&group.files_read, 6)
                ));
            }
            if !group.prompts.is_empty() {
                out.push_str(&format!(
                    "  - Prompt: {}\n",
                    limited_join(&group.prompts, 2)
                ));
            }
            if !group.flags.is_empty() {
                out.push_str(&format!("  - Flags: {}\n", limited_join(&group.flags, 6)));
            }
            if let Some(trust) = group.max_trust {
                out.push_str(&format!("  - Trust: {trust}\n"));
            }
            // Warn when low-confidence Copilot heuristic captures were present.
            // These fire on any large VS Code document change — not only real
            // Copilot completions — so attribution may be unreliable.
            let has_cpl_warning = traces.iter().any(|trace| {
                trace
                    .agentdiff_metadata()
                    .as_ref()
                    .and_then(|m| m.copilot_context.as_ref())
                    .and_then(|ctx| ctx.get("low_confidence"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            });
            if has_cpl_warning {
                out.push_str(
                    "  - Copilot context: low-confidence heuristic capture detected \
                     (inline change events may include edits from other agents or humans)\n",
                );
            }
        }
    }

    out.push_str("\n## Files To Review First\n\n");
    out.push_str("| File | Lines | Dominant Agent | Intent | Context |\n");
    out.push_str("|------|-------|----------------|--------|---------|\n");
    let mut rows: Vec<_> = file_rows.into_iter().collect();
    rows.sort_by(|a, b| b.1.lines.cmp(&a.1.lines).then_with(|| a.0.cmp(&b.0)));
    for (file, row) in rows.iter().take(20) {
        let dominant = row
            .agents
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(agent, _)| agent.as_str())
            .unwrap_or("unknown");
        let context = if row.trace_ids.is_empty() {
            String::new()
        } else {
            format!("trace {}", row.trace_ids[0])
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            md_cell(file),
            row.lines,
            md_cell(dominant),
            md_cell(&limited_join(&row.intents, 3)),
            md_cell(&context)
        ));
    }

    if include_context {
        out.push_str("\n<details>\n<summary>Trace details</summary>\n\n");
        out.push_str("| Trace | Agent | Intent | Files | Lines |\n");
        out.push_str("|-------|-------|--------|-------|-------|\n");
        for trace in traces.iter().take(30) {
            let meta = trace.agentdiff_metadata();
            let intent = meta
                .as_ref()
                .and_then(|m| m.intent.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "unspecified".to_string());
            let files: HashSet<String> = trace.files.iter().map(|f| f.path.clone()).collect();
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                short_id(&trace.id),
                md_cell(trace.agent_name()),
                md_cell(&intent),
                md_cell(&limited_join(&files, 5)),
                trace_line_count(trace)
            ));
        }
        out.push_str("\n</details>\n");
    }

    Ok(out)
}

fn context_json_report(traces: &[AgentTrace]) -> Result<serde_json::Value> {
    let total_lines: usize = traces.iter().map(trace_line_count).sum();
    let trace_values: Vec<_> = traces
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
                "files": trace.files.iter().map(|file| serde_json::json!({
                    "path": file.path,
                    "lines": trace_file_line_count(file),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "total_traces": traces.len(),
        "total_lines": total_lines,
        "traces": trace_values,
    }))
}

fn trace_models(trace: &AgentTrace) -> Vec<String> {
    let mut models = HashSet::new();
    for file in &trace.files {
        for conv in &file.conversations {
            if let Some(model) = &conv.contributor.model_id {
                models.insert(model.clone());
            }
        }
    }
    let mut out: Vec<_> = models.into_iter().collect();
    out.sort();
    out
}

fn trace_line_count(trace: &AgentTrace) -> usize {
    trace.files.iter().map(trace_file_line_count).sum()
}

fn trace_file_line_count(file: &crate::data::TraceFile) -> usize {
    file.conversations
        .iter()
        .flat_map(|conv| conv.ranges.iter())
        .map(|range| {
            let lo = range.start_line.min(range.end_line);
            let hi = range.start_line.max(range.end_line);
            hi.saturating_sub(lo) as usize + 1
        })
        .sum()
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn md_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn limited_join(values: &HashSet<String>, limit: usize) -> String {
    let mut sorted: Vec<_> = values.iter().filter(|v| !v.is_empty()).cloned().collect();
    sorted.sort();
    if sorted.is_empty() {
        return "unknown".to_string();
    }
    let extra = sorted.len().saturating_sub(limit);
    let mut shown: Vec<_> = sorted.into_iter().take(limit).collect();
    if extra > 0 {
        shown.push(format!("+{extra} more"));
    }
    shown.join(", ")
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
    use crate::data::{
        AgentTrace, Contributor, Conversation, ToolInfo, TraceFile, TraceRange, VcsInfo,
    };
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

    fn sample_trace_with_context() -> AgentTrace {
        AgentTrace {
            version: "0.1.0".to_string(),
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: Utc::now(),
            vcs: Some(VcsInfo {
                vcs_type: "git".to_string(),
                revision: "abc123".to_string(),
            }),
            tool: Some(ToolInfo {
                name: "cursor".to_string(),
                version: None,
            }),
            files: vec![TraceFile {
                path: "src/api.rs".to_string(),
                conversations: vec![Conversation {
                    url: None,
                    contributor: Contributor {
                        contributor_type: "ai".to_string(),
                        model_id: Some("cursor-test".to_string()),
                    },
                    ranges: vec![TraceRange {
                        start_line: 1,
                        end_line: 3,
                        content_hash: None,
                        contributor: None,
                    }],
                    related: None,
                }],
            }],
            metadata: Some(serde_json::json!({
                "agentdiff": {
                    "intent": "security hardening",
                    "prompt_excerpt": "add route guard",
                    "files_read": ["src/auth.rs"],
                    "flags": ["security"],
                    "trust": 91,
                    "session_id": "sess-1"
                }
            })),
            sig: None,
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

    #[test]
    fn markdown_trace_report_includes_review_context() {
        let md = markdown_trace_report(&[sample_trace_with_context()], true).unwrap();
        assert!(md.contains("## Review Context"));
        assert!(md.contains("Intent: security hardening"));
        assert!(md.contains("Files read: src/auth.rs"));
        assert!(md.contains("| src/api.rs | 3 | cursor | security hardening | trace 550e8400 |"));
    }

    #[test]
    fn context_json_report_includes_trace_metadata() {
        let json = context_json_report(&[sample_trace_with_context()]).unwrap();
        assert_eq!(json["total_traces"], 1);
        assert_eq!(json["traces"][0]["intent"], "security hardening");
        assert_eq!(json["traces"][0]["files_read"][0], "src/auth.rs");
    }

    #[test]
    fn filter_traces_by_commit_set_keeps_only_branch_commits() {
        let mut keep = sample_trace_with_context();
        keep.vcs.as_mut().unwrap().revision = "keep-sha".to_string();
        let mut drop = sample_trace_with_context();
        drop.vcs.as_mut().unwrap().revision = "drop-sha".to_string();

        let shas = HashSet::from(["keep-sha".to_string()]);
        let filtered = filter_traces_by_commit_set(&[keep, drop], &shas);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].sha(), "keep-sha");
    }
}
