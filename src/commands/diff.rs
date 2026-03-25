use crate::cli::DiffArgs;
use crate::store::Store;
use crate::util::agent_color_str;
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &DiffArgs) -> Result<()> {
    let commit = args.commit.as_deref().unwrap_or("HEAD");
    let entries = store.load_entries()?;

    // Run git diff to get changed lines
    let diff_output = run_git_diff(&store.repo_root, commit)?;
    let changed = parse_diff_hunks(&diff_output);

    println!("\n  {} — {}\n", "agentdiff diff".cyan().bold(), commit);

    let mut total_changed = 0;
    let mut total_ai = 0;

    for (file, lines) in &changed {
        let file_entries: Vec<_> = entries.iter().filter(|e| &e.file == file).collect();

        println!("  {}", file.bold());
        total_changed += lines.len();

        for &ln in lines {
            let attribution = file_entries.iter().find(|e| e.lines.contains(&(ln)));

            if let Some(entry) = attribution {
                total_ai += 1;
                if args.ai_only {
                    println!(
                        "    {:>4} {} ({})",
                        ln,
                        agent_color_str(&entry.agent),
                        entry.tool
                    );
                } else {
                    println!(
                        "    {:>4} {} ({}) {}",
                        ln,
                        agent_color_str(&entry.agent),
                        entry.tool,
                        entry.model.dimmed()
                    );
                }
            } else {
                if !args.ai_only {
                    println!(
                        "    {:>4} {}",
                        ln,
                        "human".color(colored::Color::BrightGreen).bold()
                    );
                }
            }
        }
        println!();
    }

    println!(
        "  {} lines changed, {} AI-attributed ({:.1}%)\n",
        total_changed,
        total_ai,
        if total_changed > 0 {
            (total_ai as f64 / total_changed as f64 * 100.0)
        } else {
            0.0
        }
    );

    Ok(())
}

fn run_git_diff(repo_root: &std::path::Path, commit: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["diff", commit])
        .current_dir(repo_root)
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_diff_hunks(diff: &str) -> std::collections::HashMap<String, Vec<u32>> {
    let mut result: std::collections::HashMap<String, Vec<u32>> = std::collections::HashMap::new();
    let mut current_file = String::new();
    let mut base_line: u32 = 0;

    for line in diff.lines() {
        if line.starts_with("diff --git") {
            // Extract file from "diff --git a/path b/path"
            if let Some(path) = line.split_whitespace().last() {
                // Remove "b/" prefix
                current_file = path.trim_start_matches("b/").to_string();
            }
        } else if line.starts_with("@@") {
            // Parse hunk header: @@ -start,count +start,count @@
            if let Some(end) = line.find(" @@") {
                let header = &line[4..end];
                if let Some(pos) = header.find("+") {
                    let after_plus = &header[pos + 1..];
                    if let Some(comma) = after_plus.find(',') {
                        base_line = after_plus[..comma].parse().unwrap_or(1);
                    } else {
                        base_line = after_plus.parse().unwrap_or(1);
                    }
                }
            }
        } else if (line.starts_with('+') || line.starts_with(' '))
            && !line.starts_with("+++")
            && !line.starts_with("index")
        {
            // Added or context line
            if !current_file.is_empty() {
                result
                    .entry(current_file.clone())
                    .or_default()
                    .push(base_line);
            }
            base_line += 1;
        }
    }

    result
}
