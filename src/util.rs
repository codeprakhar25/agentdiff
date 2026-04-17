use anyhow::Context;
use colored::*;

// ── Output prefixes ─────────────────────────────────────────────────────────
//
// All CLI output uses these helpers so `ok`, `warn`, and `error` lines look
// identical across commands. Keep prefixes to a fixed width so columns align.

pub fn ok() -> ColoredString {
    "ok".green()
}

pub fn warn() -> ColoredString {
    "warn".yellow()
}

pub fn err() -> ColoredString {
    "error".red()
}

pub fn dim() -> ColoredString {
    "--".dimmed()
}

/// Standard command header: leading blank line, then `  agentdiff <name>`.
pub fn print_command_header(name: &str) {
    println!();
    println!("  {}", format!("agentdiff {name}").bold().cyan());
    println!();
}

/// Single source of truth for the "init not run" hint used by every query
/// command. Keeps the message consistent if it ever changes.
pub fn print_not_initialized() {
    println!();
    println!(
        "  {} agentdiff init not run in this repo — no captures recorded.",
        warn()
    );
    println!(
        "  Run {} to start tracking AI contributions.",
        "agentdiff init".cyan()
    );
    println!();
}

pub fn agent_color(agent: &str) -> Color {
    match agent {
        "claude-code" => Color::BrightBlue,
        "cursor" => Color::BrightYellow,
        "codex" | "windsurf" | "aider" | "antigravity" | "opencode" => Color::Yellow,
        "human" => Color::BrightGreen,
        _ => Color::White,
    }
}

pub fn agent_color_str(agent: &str) -> ColoredString {
    let color = agent_color(agent);
    agent.color(color).bold()
}

pub fn fmt_lines(lines: &[u32]) -> String {
    match lines.len() {
        0 => "—".into(),
        1 => lines[0].to_string(),
        _ => format!("{}-{}", lines.first().unwrap(), lines.last().unwrap()),
    }
}

pub fn fmt_prompt(prompt: &str, width: usize) -> String {
    if prompt.is_empty() || prompt == "unknown" {
        return "—".dimmed().to_string();
    }
    let p = prompt.replace('\n', " ");
    let truncated = if p.chars().count() > width {
        format!("{}…", &p[..width.saturating_sub(1)])
    } else {
        p
    };
    format!("\"{truncated}\"")
}

pub fn fmt_time(ts: &str) -> String {
    // Parse ISO 8601 and display as "Mar 20 17:52"
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.format("%b %d %H:%M").to_string())
        .unwrap_or_else(|_| ts[..ts.len().min(16)].to_string())
}

pub fn find_repo_root() -> anyhow::Result<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("running git rev-parse")?;

    if output.status.success() {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(std::path::PathBuf::from(root))
    } else {
        anyhow::bail!("Not in a git repository (run git init or cd to a repo)")
    }
}
