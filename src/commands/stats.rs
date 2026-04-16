use crate::cli::StatsArgs;
use crate::store::Store;
use crate::util::agent_color_str;
use anyhow::Result;
use colored::Colorize;
use std::collections::{HashMap, HashSet};

pub fn run(store: &Store, args: &StatsArgs) -> Result<()> {
    if !store.is_initialized() {
        println!(
            "\n  {} agentdiff init not run in this repo — no captures recorded.",
            "!".yellow()
        );
        println!(
            "  Run {} to start tracking AI contributions.\n",
            "agentdiff init".cyan()
        );
        return Ok(());
    }

    let entries = store.load_entries()?;

    let mut agent_lines: HashMap<String, u32> = HashMap::new();
    let mut agent_models: HashMap<String, HashSet<String>> = HashMap::new();
    let mut file_agent_lines: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut ai_lines_by_file: HashMap<String, HashSet<u32>> = HashMap::new();

    // Count AI-attributed lines
    for e in &entries {
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
        agent_models
            .entry(agent.clone())
            .or_default()
            .insert(e.model.clone());
    }

    // Add human lines (total - AI)
    for (file, ai_set) in &ai_lines_by_file {
        let abs = store.repo_root.join(file);
        if let Ok(content) = std::fs::read_to_string(&abs) {
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

    println!();
    println!("  {} — Statistics\n", "agentdiff stats".cyan().bold());
    println!("  Total lines tracked: {}", total_lines);
    println!();

    // By agent
    println!("  By Agent:");
    let mut sorted: Vec<_> = agent_lines.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (agent, count) in sorted {
        let pct = (*count as f64 / total_lines as f64 * 100.0) as u32;
        let bar = "█".repeat((pct / 5) as usize);
        println!(
            "    {} {:>6} ({:>3}%) {}",
            agent_color_str(agent),
            count,
            pct,
            bar.dimmed()
        );
    }

    // By file if requested
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
            let ai_pct = if file_total > 0 {
                (ai_total as f64 / file_total as f64 * 100.0) as u32
            } else {
                0
            };

            let dominant = agents
                .iter()
                .max_by_key(|(_, c)| *c)
                .map(|(a, _)| a.clone())
                .unwrap_or("human".to_string());

            println!(
                "    {:<40} {:>5} lines ({:>3}% AI) — {}",
                file,
                file_total,
                ai_pct,
                agent_color_str(&dominant)
            );
        }
    }

    // By model if requested
    if args.by_model {
        println!();
        println!("  By Model:");
        let mut model_counts: HashMap<String, u32> = HashMap::new();
        for e in &entries {
            *model_counts.entry(e.model.clone()).or_default() += e.lines.len() as u32;
        }
        let mut model_sorted: Vec<_> = model_counts.iter().collect();
        model_sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (model, count) in model_sorted.iter().take(10) {
            let pct = (*count / total_lines) * 100.0 as u32;
            println!("    {:<30} {:>6} ({:>3}%)", model, count, pct);
        }
    }

    println!();
    Ok(())
}
