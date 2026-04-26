use crate::cli::DiffArgs;
use crate::store::Store;
use crate::util::{agent_color_str, print_command_header, print_not_initialized};
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &DiffArgs) -> Result<()> {
    if !store.is_initialized() {
        print_not_initialized();
        return Ok(());
    }

    let commit = args.commit.as_deref().unwrap_or("HEAD");
    let entries = store.load_entries()?;

    // Run a zero-context diff so changed-line parsing is not polluted by
    // surrounding context lines.
    let diff_output = run_git_diff(&store.repo_root, commit)?;
    let changed = parse_diff_hunks(&diff_output);

    print_command_header("diff");
    println!("  {}", commit);
    println!();

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
            total_ai as f64 / total_changed as f64 * 100.0
        } else {
            0.0
        }
    );

    Ok(())
}

fn run_git_diff(repo_root: &std::path::Path, commit: &str) -> Result<String> {
    let spec = diff_spec(commit);
    let output = std::process::Command::new("git")
        .args(["diff", "--unified=0", "--no-color", "--no-ext-diff", &spec])
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git diff failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn diff_spec(commit: &str) -> String {
    if commit.contains("..") {
        commit.to_string()
    } else {
        format!("{commit}^!")
    }
}

fn parse_diff_hunks(diff: &str) -> std::collections::HashMap<String, Vec<u32>> {
    let mut result: std::collections::HashMap<String, Vec<u32>> = std::collections::HashMap::new();
    let mut current_file = String::new();
    let mut new_line: u32 = 0;

    for line in diff.lines() {
        if line.starts_with("diff --git") {
            current_file.clear();
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ ") {
            if path == "/dev/null" {
                current_file.clear();
            } else {
                current_file = path.trim_start_matches("b/").to_string();
            }
            continue;
        }

        if line.starts_with("@@") {
            // Parse hunk header: @@ -start,count +start,count @@
            if let Some(end) = line.find(" @@") {
                let header = &line[4..end];
                if let Some(pos) = header.find("+") {
                    let after_plus = &header[pos + 1..];
                    if let Some(comma) = after_plus.find(',') {
                        new_line = after_plus[..comma].parse().unwrap_or(1);
                    } else {
                        new_line = after_plus.parse().unwrap_or(1);
                    }
                }
            }
            continue;
        }

        if line.starts_with('+') && !line.starts_with("+++") {
            if !current_file.is_empty() {
                result
                    .entry(current_file.clone())
                    .or_default()
                    .push(new_line);
            }
            new_line += 1;
        } else if line.starts_with(' ') {
            new_line += 1;
        }
    }

    for lines in result.values_mut() {
        lines.sort_unstable();
        lines.dedup();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_commit_diff_uses_commit_changes_not_worktree_delta() {
        assert_eq!(diff_spec("HEAD"), "HEAD^!");
        assert_eq!(diff_spec("abc123"), "abc123^!");
    }

    #[test]
    fn explicit_range_is_preserved() {
        assert_eq!(diff_spec("main..HEAD"), "main..HEAD");
        assert_eq!(diff_spec("main...HEAD"), "main...HEAD");
    }

    #[test]
    fn parses_only_new_side_changed_lines() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,3 @@
 context
-old
+new
 context
@@ -20,0 +21,2 @@
+added one
+added two
"#;

        let changed = parse_diff_hunks(diff);
        assert_eq!(changed.get("src/lib.rs"), Some(&vec![11, 21, 22]));
    }
}
