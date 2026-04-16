use crate::cli::ConfigArgs;
use crate::config::Config;
use anyhow::Result;
use colored::Colorize;

pub fn run(config: &Config, args: &ConfigArgs) -> Result<()> {
    match &args.action {
        crate::cli::ConfigAction::Show => cmd_show(config),
        crate::cli::ConfigAction::Set { key, value } => cmd_set(config, key, value),
        crate::cli::ConfigAction::Get { key } => cmd_get(config, key),
        crate::cli::ConfigAction::AddRepo { path } => cmd_add_repo(config, path),
    }
}

fn cmd_show(config: &Config) -> Result<()> {
    println!("{}", "agentdiff config".cyan().bold());
    println!();
    println!("  Scripts directory: {}", config.scripts_root().display());
    println!(
        "  Auto-amend ledger: {}",
        if config.auto_amend_ledger_enabled() {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Capture prompts:   {} (disable with: agentdiff config set capture_prompts false)",
        if config.capture_prompts { "enabled" } else { "disabled" }
    );
    println!("  Config file: {}", Config::config_path().display());
    println!();

    if config.repos.is_empty() {
        println!("  No repos registered.");
    } else {
        println!("  Registered repos:");
        for repo in &config.repos {
            println!("    - {} ({})", repo.path.display(), repo.slug);
        }
    }

    if !config.agent_aliases.is_empty() {
        println!();
        println!("  Agent aliases:");
        for (alias, target) in &config.agent_aliases {
            println!("    {} → {}", alias, target);
        }
    }

    Ok(())
}

fn cmd_set(config: &Config, key: &str, value: &str) -> Result<()> {
    let mut cfg = config.clone();
    match key {
        "scripts_dir" => {
            cfg.scripts_dir = Some(std::path::PathBuf::from(value));
        }
        "auto_amend_ledger" => {
            cfg.auto_amend_ledger = parse_bool(value)?;
        }
        "capture_prompts" => {
            cfg.capture_prompts = parse_bool(value)?;
        }
        _ => {
            anyhow::bail!(
                "Unknown config key: {}. Valid keys: scripts_dir, auto_amend_ledger, capture_prompts",
                key
            );
        }
    }
    cfg.save()?;
    println!("{} Set {} = {}", "ok".green(), key, value);
    Ok(())
}

fn cmd_get(config: &Config, key: &str) -> Result<()> {
    match key {
        "scripts_dir" => {
            println!("{}", config.scripts_root().display());
        }
        "auto_amend_ledger" => {
            println!("{}", config.auto_amend_ledger_enabled());
        }
        "capture_prompts" => {
            println!("{}", config.capture_prompts);
        }
        _ => {
            anyhow::bail!("Unknown config key: {}", key);
        }
    }
    Ok(())
}

fn parse_bool(input: &str) -> Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => anyhow::bail!("Invalid boolean value: {} (expected true/false)", input),
    }
}

fn cmd_add_repo(config: &Config, path: &std::path::Path) -> Result<()> {
    let mut cfg = config.clone();
    let slug = Config::slug_for(path);

    if cfg.repos.iter().any(|r| r.slug == slug) {
        println!("{} Repo already registered", "--".dimmed());
    } else {
        cfg.repos.push(crate::config::RepoConfig {
            path: path.to_path_buf(),
            slug,
        });
        cfg.save()?;
        println!("{} Registered repo: {}", "ok".green(), path.display());
    }

    Ok(())
}
