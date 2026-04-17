use crate::config::Config;
use crate::util::{dim, ok, warn};
use anyhow::{Context, Result};

use std::fs;

pub fn step_configure_codex(config: &Config) -> Result<()> {
    let codex_dir = dirs::home_dir().unwrap().join(".codex");
    let config_path = codex_dir.join("config.toml");
    if !codex_dir.exists() && !config_path.exists() {
        println!("{} ~/.codex not found — skipping Codex setup", warn());
        return Ok(());
    }

    step_configure_codex_toml(config, &codex_dir, &config_path)?;
    step_configure_codex_hooks(config, &codex_dir)?;
    Ok(())
}

/// Write notify + codex_hooks=true into ~/.codex/config.toml.
/// The notify array is kept for backwards compat with older Codex builds that
/// predate the hooks.json event system.
fn step_configure_codex_toml(
    config: &Config,
    codex_dir: &std::path::Path,
    config_path: &std::path::Path,
) -> Result<()> {
    let capture_script = config.scripts_root().join("capture-codex.py");
    let raw = fs::read_to_string(config_path).unwrap_or_default();
    let mut cfg_val: toml::Value = if raw.trim().is_empty() {
        toml::Value::Table(Default::default())
    } else {
        toml::from_str(&raw).context("parsing ~/.codex/config.toml")?
    };

    let table = cfg_val
        .as_table_mut()
        .context("Codex config root must be a table")?;
    let mut changed = false;

    let current_notify = table.get("notify").and_then(toml_array_to_strings);
    let wanted_base = vec![
        "python3".to_string(),
        capture_script.to_string_lossy().to_string(),
    ];

    let next_notify = match current_notify {
        None => wanted_base.clone(),
        Some(existing) => {
            if existing.iter().any(|part| part.contains("capture-codex.py")) {
                if let Some(forward_idx) = existing.iter().position(|p| p == "--forward") {
                    let forward = existing.get(forward_idx + 1).cloned().unwrap_or_default();
                    if forward.is_empty() {
                        wanted_base.clone()
                    } else {
                        let mut with_forward = wanted_base.clone();
                        with_forward.push("--forward".to_string());
                        with_forward.push(forward);
                        with_forward
                    }
                } else {
                    wanted_base.clone()
                }
            } else if existing.is_empty() {
                wanted_base.clone()
            } else {
                let forward = serde_json::to_string(&existing)?;
                let mut chained = wanted_base.clone();
                chained.push("--forward".to_string());
                chained.push(forward);
                chained
            }
        }
    };

    if table
        .get("notify")
        .and_then(toml_array_to_strings)
        .unwrap_or_default()
        != next_notify
    {
        table.insert("notify".to_string(), string_array_to_toml(&next_notify));
        changed = true;
    }

    // Ensure codex_hooks = true so hooks.json events are emitted.
    let features = table
        .entry("features".to_string())
        .or_insert(toml::Value::Table(Default::default()));
    let features_table = features
        .as_table_mut()
        .context("~/.codex/config.toml [features] must be a table")?;
    if features_table.get("codex_hooks").and_then(|v| v.as_bool()) != Some(true) {
        features_table.insert("codex_hooks".to_string(), toml::Value::Boolean(true));
        changed = true;
    }

    if changed {
        fs::create_dir_all(codex_dir)?;
        fs::write(config_path, toml::to_string_pretty(&cfg_val)?)?;
        println!(
            "{} Codex config.toml updated (notify + codex_hooks=true) in {}",
            ok(),
            config_path.display()
        );
    } else {
        println!("{} Codex config.toml already up-to-date", dim());
    }
    Ok(())
}

/// Write UserPromptSubmit + Stop hooks into ~/.codex/hooks.json.
///
/// - UserPromptSubmit: fires before each turn — capture-codex.py uses it to
///   snapshot the current dirty-file list so task attribution stays clean.
/// - Stop: fires when the session ends — capture-codex.py reads git diff at
///   this point to record which files were changed and by which agent.
///
/// Migrates the old flat-array format to the current nested-object format if
/// an existing hooks.json uses the old shape.
fn step_configure_codex_hooks(config: &Config, codex_dir: &std::path::Path) -> Result<()> {
    let hooks_path = codex_dir.join("hooks.json");
    let capture_script = config.scripts_root().join("capture-codex.py");
    let capture_cmd = format!("python3 {}", capture_script.display());

    // Load existing file or start fresh with the correct shape.
    let raw = fs::read_to_string(&hooks_path).unwrap_or_default();
    let mut root: serde_json::Value = if raw.trim().is_empty() {
        serde_json::json!({ "hooks": {} })
    } else {
        serde_json::from_str(&raw).unwrap_or(serde_json::json!({ "hooks": {} }))
    };

    // Migrate old flat-array format ({ "hooks": [...] }) to the current
    // nested-object format ({ "hooks": { "EventName": [...] } }).
    let hooks_val = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(serde_json::json!({}));
    if hooks_val.is_array() {
        println!(
            "{} Codex hooks.json: migrating old flat-array format to event-keyed format",
            warn()
        );
        *hooks_val = serde_json::json!({});
    }
    let hooks_map = hooks_val.as_object_mut().unwrap();

    // (event, timeout_secs)
    let events: &[(&str, u64)] = &[
        // Fires before the model processes each user turn.
        // capture-codex.py treats this as "task_started": saves a pre-task
        // snapshot of dirty files so Stop-time attribution excludes pre-existing changes.
        ("UserPromptSubmit", 10),
        // Fires when the session ends.
        // capture-codex.py reads git diff at this point and writes session.jsonl entries.
        ("Stop", 30),
    ];

    let mut changed = false;
    for (event, timeout_secs) in events {
        let event_arr = hooks_map
            .entry(*event)
            .or_insert(serde_json::json!([]))
            .as_array_mut()
            .unwrap();

        // Check if our hook already exists anywhere in this event's groups.
        let found = event_arr.iter().any(|group| {
            group
                .get("hooks")
                .and_then(|h| h.as_array())
                .map(|hs| {
                    hs.iter().any(|h| {
                        h.get("command")
                            .and_then(|c| c.as_str())
                            .map(|c| c.contains("capture-codex.py"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        });

        if !found {
            event_arr.push(serde_json::json!({
                "hooks": [{
                    "type": "command",
                    "command": capture_cmd,
                    "timeout": timeout_secs
                }]
            }));
            changed = true;
        } else {
            // Update command path if scripts_dir moved.
            for group in event_arr.iter_mut() {
                if let Some(hs) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                    for h in hs.iter_mut() {
                        if let Some(cmd_val) = h.get_mut("command") {
                            if cmd_val
                                .as_str()
                                .map(|c| c.contains("capture-codex.py"))
                                .unwrap_or(false)
                                && cmd_val.as_str() != Some(&capture_cmd)
                            {
                                *cmd_val = serde_json::Value::String(capture_cmd.clone());
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    if changed {
        if let Some(parent) = hooks_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&hooks_path, serde_json::to_string_pretty(&root)?)?;
        println!(
            "{} Codex hooks.json configured (UserPromptSubmit + Stop) in {}",
            ok(),
            hooks_path.display()
        );
    } else {
        println!(
            "{} Codex hooks.json already up-to-date in {}",
            dim(),
            hooks_path.display()
        );
    }
    Ok(())
}

fn toml_array_to_strings(v: &toml::Value) -> Option<Vec<String>> {
    let arr = v.as_array()?;
    arr.iter()
        .map(|x| x.as_str().map(|s| s.to_string()))
        .collect()
}

fn string_array_to_toml(parts: &[String]) -> toml::Value {
    toml::Value::Array(
        parts
            .iter()
            .map(|p| toml::Value::String(p.clone()))
            .collect(),
    )
}
