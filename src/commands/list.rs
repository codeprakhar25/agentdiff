use crate::cli::ListArgs;
use crate::store::Store;
use crate::util::{fmt_lines, fmt_prompt, fmt_time};
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &ListArgs) -> Result<()> {
    if args.uncommitted {
        return run_uncommitted(store, args);
    }

    let mut records = store.load_ledger_records()?;
    if records.is_empty() {
        return run_committed_fallback(store, args);
    }

    if let Some(ref agent) = args.agent {
        records.retain(|r| r.agent.contains(agent.as_str()));
    }
    if let Some(ref file) = args.file {
        records.retain(|r| r.files_touched.iter().any(|f| f.contains(file.as_str())));
    }
    if let Some(limit) = args.limit {
        records.truncate(limit);
    }

    println!();
    println!(
        "  {} — {} entries",
        "agentdiff list".cyan().bold(),
        records.len()
    );
    println!();

    let hdr = format!(
        "  {:<4} {:<10} {:<13} {:<13} {:<20} {:<6} {:<8} {:<8} {:<12} PROMPT",
        "#", "COMMIT", "TIME", "AGENT", "MODEL", "FILES", "LINES", "TRUST", "FLAGS"
    );
    println!("{}", hdr.dimmed());
    println!("  {}", "─".repeat(130).dimmed());

    for (i, r) in records.iter().enumerate() {
        let commit = if r.sha.len() > 8 { &r.sha[..8] } else { &r.sha };
        let flags = if r.flags.is_empty() {
            "—".to_string()
        } else {
            r.flags.join(",")
        };
        let trust = r
            .trust
            .map(|t| t.to_string())
            .unwrap_or_else(|| "—".to_string());
        let prompt = fmt_prompt(&r.prompt_excerpt, 45);
        println!(
            "  {:<4} {:<10} {:<13} {:<13} {:<20} {:<6} {:<8} {:<8} {:<12} {}",
            i + 1,
            commit.dimmed(),
            fmt_time(&r.ts.to_rfc3339()),
            crate::util::agent_color_str(&r.agent),
            r.model,
            r.files_touched.len(),
            count_lines(r),
            trust,
            trim_text(&flags, 12),
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

fn run_committed_fallback(store: &Store, args: &ListArgs) -> Result<()> {
    let mut entries = store.load_entries()?;
    entries.retain(|e| e.committed);
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
        "  {} — {} committed entries (legacy view)",
        "agentdiff list".cyan().bold(),
        entries.len()
    );
    println!();

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
        let line_str = fmt_lines(&e.lines);
        let prompt_str = fmt_prompt(e.prompt.as_deref().unwrap_or("unknown"), 45);

        println!(
            "  {:<4} {:<10} {:<13} {:<13} {:<22} {:<10} {:<32} {:<8} {}",
            i + 1,
            commit.dimmed(),
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

fn count_lines(record: &crate::data::LedgerRecord) -> usize {
    let mut total = 0usize;
    for ranges in record.lines.values() {
        for (a, b) in ranges {
            if *a == 0 || *b == 0 {
                continue;
            }
            let lo = (*a).min(*b);
            let hi = (*a).max(*b);
            total += (hi - lo + 1) as usize;
        }
    }
    total
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
