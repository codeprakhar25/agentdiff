use crate::cli::ConfigArgs;
use crate::config::Config;
use crate::util::{ok, print_command_header};
use anyhow::Result;

pub fn run(config: &Config, args: &ConfigArgs) -> Result<()> {
    match &args.action {
        crate::cli::ConfigAction::Show => cmd_show(config),
        crate::cli::ConfigAction::Set { key, value } => cmd_set(config, key, value),
        crate::cli::ConfigAction::Get { key } => cmd_get(config, key),
    }
}

fn cmd_show(config: &Config) -> Result<()> {
    print_command_header("config");
    println!("  Scripts directory: {}", config.scripts_root().display());
    println!(
        "  Capture prompts:   {} (disable with: agentdiff config set capture_prompts false)",
        if config.capture_prompts { "enabled" } else { "disabled" }
    );
    println!("  Config file:       {}", Config::config_path().display());
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
        "capture_prompts" => {
            cfg.capture_prompts = parse_bool(value)?;
        }
        _ => {
            anyhow::bail!(
                "Unknown config key: {}. Valid keys: scripts_dir, capture_prompts",
                key
            );
        }
    }
    cfg.save()?;
    println!("  {} set {} = {}", ok(), key, value);
    Ok(())
}

fn cmd_get(config: &Config, key: &str) -> Result<()> {
    match key {
        "scripts_dir" => {
            println!("{}", config.scripts_root().display());
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
