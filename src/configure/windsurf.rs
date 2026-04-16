use crate::config::Config;
use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;

pub fn step_configure_windsurf(config: &Config) -> Result<()> {
    // Use the Windsurf user-level global config (~/.codeium/windsurf/hooks.json).
    // This applies to all repos without needing per-repo setup.
    let hooks_path = dirs::home_dir()
        .unwrap()
        .join(".codeium")
        .join("windsurf")
        .join("hooks.json");
    let capture_script = config.scripts_root().join("capture-windsurf.py");
    let capture_cmd = format!("python3 {}", capture_script.display());

    let raw = fs::read_to_string(&hooks_path).unwrap_or_else(|_| "{}".to_string());
    let mut hooks_cfg: serde_json::Value =
        serde_json::from_str(&raw).context("parsing .windsurf/hooks.json")?;
    let hooks = hooks_cfg
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();

    let events = ["post_write_code", "post_cascade_response_with_transcript"];
    let mut changed = false;

    for event in events {
        let arr = hooks
            .entry(event)
            .or_insert(serde_json::json!([]))
            .as_array_mut()
            .unwrap();

        let mut found = false;
        for hook in arr.iter_mut() {
            let Some(cmd_val) = hook.get_mut("command") else {
                continue;
            };
            let Some(cmd) = cmd_val.as_str() else {
                continue;
            };
            if cmd.contains("capture-windsurf.py") {
                found = true;
                if cmd != capture_cmd {
                    *cmd_val = serde_json::Value::String(capture_cmd.clone());
                    changed = true;
                }
            }
        }

        if !found {
            arr.push(serde_json::json!({
                "type": "command",
                "command": capture_cmd
            }));
            changed = true;
        }

        let mut seen = std::collections::HashSet::new();
        arr.retain(|hook| {
            let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) else {
                return true;
            };
            if seen.contains(cmd) {
                changed = true;
                false
            } else {
                seen.insert(cmd.to_string());
                true
            }
        });
    }

    if changed {
        if let Some(parent) = hooks_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&hooks_path, serde_json::to_string_pretty(&hooks_cfg)?)?;
        println!(
            "{} Windsurf hooks configured in {}",
            "ok".green(),
            hooks_path.display()
        );
    } else {
        println!(
            "{} Windsurf hooks already present in {}",
            "--".dimmed(),
            hooks_path.display()
        );
    }
    Ok(())
}
