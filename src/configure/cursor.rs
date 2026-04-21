use crate::config::Config;
use crate::util::{dim, ok, warn};
use anyhow::{Context, Result};

use std::fs;

pub fn step_configure_cursor(config: &Config) -> Result<()> {
    let capture_script = config.scripts_root().join("capture-cursor.py");
    let capture_cmd = format!("python3 {}", capture_script.display());

    // Cursor on WSL2 is a Windows app — it reads hooks from the Windows-side ~/.cursor/.
    // We write to both locations so native Linux installs and WSL2 are both covered.
    let candidate_dirs: Vec<std::path::PathBuf> = {
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".cursor"));
        }
        // Windows-side path when running under WSL2.
        let win_cursor = std::path::Path::new("/mnt/c/Users")
            .read_dir()
            .ok()
            .and_then(|mut rd| rd.next())
            .and_then(|e| e.ok())
            .map(|e| e.path().join(".cursor"));
        // More reliable: derive from $USERPROFILE or the actual Windows username.
        // Fall back to scanning /mnt/c/Users for the first user directory that has .cursor.
        let win_cursor_reliable = std::path::Path::new("/mnt/c/Users")
            .read_dir()
            .ok()
            .and_then(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path().join(".cursor"))
                    .find(|p| p.exists())
            });
        if let Some(p) = win_cursor_reliable {
            dirs.push(p);
        } else if let Some(p) = win_cursor {
            if p.exists() {
                dirs.push(p);
            }
        }
        dirs
    };

    let mut any_found = false;
    for cursor_dir in &candidate_dirs {
        if !cursor_dir.exists() {
            continue;
        }
        any_found = true;
        let hooks_path = cursor_dir.join("hooks.json");
        configure_cursor_hooks_file(&hooks_path, &capture_cmd)
            .with_context(|| format!("configuring {}", hooks_path.display()))?;
    }

    if !any_found {
        println!(
            "{} ~/.cursor not found — skipping Cursor setup",
            warn()
        );
    }
    Ok(())
}

fn configure_cursor_hooks_file(
    hooks_path: &std::path::Path,
    capture_cmd: &str,
) -> Result<()> {
    let raw = fs::read_to_string(hooks_path).unwrap_or_else(|_| "{}".to_string());
    let mut hooks_cfg: serde_json::Value =
        serde_json::from_str(&raw).context("parsing hooks.json")?;

    let hooks = hooks_cfg
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();

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
                    *cmd_val = serde_json::Value::String(capture_cmd.to_string());
                    changed = true;
                }
            }
        }

        if !found {
            arr.push(serde_json::json!({ "command": capture_cmd }));
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
        fs::write(hooks_path, serde_json::to_string_pretty(&hooks_cfg)?)?;
        println!(
            "{} Cursor hooks registered in {}",
            ok(),
            hooks_path.display()
        );
    } else {
        println!("{} Cursor hooks already present in {}", dim(), hooks_path.display());
    }
    Ok(())
}
