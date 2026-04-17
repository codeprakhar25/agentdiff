use crate::config::Config;
use crate::util::{dim, ok, warn};
use anyhow::{Context, Result};

use std::fs;

pub fn step_configure_cursor(config: &Config) -> Result<()> {
    let hooks_path = dirs::home_dir().unwrap().join(".cursor").join("hooks.json");
    if !hooks_path.exists() {
        println!(
            "{} ~/.cursor/hooks.json not found — skipping Cursor setup",
            warn()
        );
        return Ok(());
    }

    let capture_script = config.scripts_root().join("capture-cursor.py");
    let raw = fs::read_to_string(&hooks_path)?;
    let mut hooks_cfg: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.cursor/hooks.json")?;

    let hooks = hooks_cfg
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();

    let capture_cmd = format!("python3 {}", capture_script.display());
    let events = ["afterFileEdit", "afterTabFileEdit", "beforeSubmitPrompt"];

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
            if cmd.contains("capture-cursor.py") {
                found = true;
                if cmd != capture_cmd {
                    *cmd_val = serde_json::Value::String(capture_cmd.clone());
                    changed = true;
                }
            }
        }

        if !found {
            arr.push(serde_json::json!({ "command": capture_cmd }));
            changed = true;
        }

        // De-duplicate exact command duplicates while preserving order.
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
        fs::write(&hooks_path, serde_json::to_string_pretty(&hooks_cfg)?)?;
        println!(
            "{} Cursor hooks registered in {}",
            ok(),
            hooks_path.display()
        );
    } else {
        println!("{} Cursor hooks already present", dim());
    }
    Ok(())
}
