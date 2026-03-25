use anyhow::Context;
use colored::*;

pub fn agent_color(agent: &str) -> Color {
    match agent {
        "claude-code" => Color::BrightBlue,
        "cursor" => Color::BrightYellow,
        "codex" | "windsurf" | "aider" | "antigravity" => Color::Yellow,
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

#[derive(Debug, thiserror::Error)]
pub enum AgentDiffError {
    #[error("Not a git repository: {path}")]
    NotAGitRepo { path: String },

    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("Config not found at {path}; run `agentdiff init` first")]
    ConfigMissing { path: String },

    #[error("Hook already installed by a different tool at {path}")]
    HookConflict { path: String },

    #[error("No attribution data found; make some edits and commit")]
    NoEntries,
}
