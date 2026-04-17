use crate::config::Config;
use crate::util::{dim, ok};
use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;

pub fn step_configure_claude(config: &Config) -> Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");
    let scripts_dir = config.scripts_root();
    let capture_script = scripts_dir.join("capture-claude.py");

    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut settings: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.claude/settings.json")?;

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap()
        .entry("PostToolUse")
        .or_insert(serde_json::json!([]))
        .as_array_mut()
        .unwrap();

    let new_hook = serde_json::json!({
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [{
            "type": "command",
            "command": format!("python3 {}", capture_script.display())
        }]
    });

    let mut changed = false;
    let mut found = false;
    for hook in hooks.iter_mut() {
        let Some(hs) = hook.get_mut("hooks").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for inner in hs.iter_mut() {
            let Some(cmd_val) = inner.get_mut("command") else {
                continue;
            };
            let Some(cmd) = cmd_val.as_str() else {
                continue;
            };
            if cmd.contains("capture-claude.py") {
                found = true;
                let wanted = format!("python3 {}", capture_script.display());
                if cmd != wanted {
                    *cmd_val = serde_json::Value::String(wanted);
                    changed = true;
                }
            }
        }
    }

    if !found {
        hooks.push(new_hook);
        changed = true;
    }

    if changed {
        let updated = serde_json::to_string_pretty(&settings)?;
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&settings_path, updated)?;
        println!(
            "{} Claude Code hook configured in {}",
            ok(),
            settings_path.display()
        );
    } else {
        println!("{} Claude Code hook already present", dim());
    }
    Ok(())
}

/// Register agentdiff-mcp as an MCP server in ~/.claude/settings.json.
/// This lets Claude automatically call `record_context` during sessions,
/// enriching ledger entries with intent, trust, files_read, and flags.
pub fn step_configure_mcp_claude() -> Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");

    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut settings: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.claude/settings.json")?;

    let mcp_servers = settings
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();

    let server_entry = serde_json::json!({
        "command": "agentdiff-mcp",
        "args": [],
        "env": {}
    });

    let already_correct = mcp_servers
        .get("agentdiff")
        .and_then(|e| e.get("command"))
        .and_then(|c| c.as_str())
        == Some("agentdiff-mcp");

    if !already_correct {
        mcp_servers.insert("agentdiff".to_string(), server_entry);
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
        println!(
            "{} agentdiff-mcp registered in {}",
            ok(),
            settings_path.display()
        );
        println!(
            "{}",
            "    Restart Claude Code to activate the MCP server.".dimmed()
        );
    } else {
        println!("{} agentdiff-mcp already registered in Claude Code", dim());
    }
    Ok(())
}
