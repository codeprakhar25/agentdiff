use crate::cli::BlameArgs;
use crate::store::Store;
use crate::util::agent_color_str;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

pub fn run(store: &Store, args: &BlameArgs) -> Result<()> {
    let entries = store.load_entries()?;
    let rel_path = args.file.clone();
    let abs_path = store.repo_root.join(&rel_path);

    if !abs_path.exists() {
        anyhow::bail!("File not found: {}", abs_path.display());
    }

    let file_content = std::fs::read_to_string(&abs_path)?;
    let file_lines: Vec<&str> = file_content.lines().collect();

    // Build line_number → Entry map (last write wins)
    let mut line_map: HashMap<u32, &crate::data::Entry> = HashMap::new();
    for e in &entries {
        let matches = e.file == rel_path || e.abs_file == abs_path.to_string_lossy();
        if !matches {
            continue;
        }
        for &ln in &e.lines {
            line_map.insert(ln, e);
        }
    }

    println!(
        "\n  {} — {}\n",
        "agentdiff blame".cyan().bold(),
        rel_path.display()
    );
    println!("{}", format!("  {}", "─".repeat(100)).dimmed());

    for (i, line) in file_lines.iter().enumerate() {
        let line_num = (i + 1) as u32;
        let attr = line_map.get(&line_num);

        if let Some(entry) = attr {
            if let Some(ref filter) = args.agent {
                if !entry.agent.contains(filter) {
                    println!(
                        "  {:>4} {} {}",
                        line_num,
                        agent_color_str(&entry.agent),
                        line
                    );
                    continue;
                }
            }
            println!(
                "  {:>4} {} ({}) {}",
                line_num,
                agent_color_str(&entry.agent),
                entry.tool,
                line
            );
        } else {
            println!(
                "  {:>4} {} {}",
                line_num,
                "human".color(colored::Color::BrightGreen).bold(),
                line
            );
        }
    }
    println!();
    Ok(())
}
