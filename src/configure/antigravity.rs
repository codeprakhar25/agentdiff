/// Configures two separate products that share the ~/.gemini/ directory:
///
/// 1. **Gemini CLI** (`~/.gemini/settings.json`) — event-driven hooks.
///    BeforeTool fires before a tool call; AfterTool fires after. These reliably
///    invoke capture-antigravity.py with structured JSON on stdin.
///
/// 2. **Antigravity editor** (`~/.gemini/GEMINI.md`) — rule-based capture.
///    Antigravity has no event hook system. Instead, a managed block in the
///    global rules file instructs the agent to run the capture script after each
///    file edit. This is best-effort (LLM-followed) rather than guaranteed.
use crate::config::Config;
use crate::util::{dim, ok, warn};
use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;

/// Managed-block markers used in ~/.gemini/GEMINI.md.
const GEMINI_MD_START: &str = "<!-- agentdiff: managed block — do not edit -->";
const GEMINI_MD_END: &str = "<!-- end agentdiff -->";

pub fn step_configure_antigravity(config: &Config) -> Result<()> {
    let gemini_dir = dirs::home_dir().unwrap().join(".gemini");
    let settings_path = gemini_dir.join("settings.json");
    if !gemini_dir.exists() && !settings_path.exists() {
        println!(
            "{} ~/.gemini not found — skipping Gemini/Antigravity setup",
            warn()
        );
        return Ok(());
    }

    // Part 1: Gemini CLI event hooks (~/.gemini/settings.json).
    step_configure_gemini_hooks(config, &gemini_dir, &settings_path)?;

    // Part 2: Antigravity global rule (~/.gemini/GEMINI.md).
    step_configure_antigravity_rule(&gemini_dir)?;

    Ok(())
}

/// Write BeforeTool + AfterTool hooks into ~/.gemini/settings.json for Gemini CLI.
fn step_configure_gemini_hooks(
    config: &Config,
    gemini_dir: &std::path::Path,
    settings_path: &std::path::Path,
) -> Result<()> {
    let capture_script = config.scripts_root().join("capture-antigravity.py");
    let capture_cmd = format!("python3 {}", capture_script.display());

    let raw = fs::read_to_string(settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut cfg: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.gemini/settings.json")?;
    let root = cfg
        .as_object_mut()
        .context("~/.gemini/settings.json root must be an object")?;
    let hooks = root
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .context("~/.gemini/settings.json hooks must be an object")?;

    let mut changed = false;
    let events = ["BeforeTool", "AfterTool"];

    for event in events {
        let arr = hooks
            .entry(event)
            .or_insert(serde_json::json!([]))
            .as_array_mut()
            .context("Gemini hook event must be an array")?;

        let mut found_matcher_idx: Option<usize> = None;
        for (idx, item) in arr.iter().enumerate() {
            let matcher = item.get("matcher").and_then(|m| m.as_str()).unwrap_or("");
            if matcher == "write_file|replace" {
                found_matcher_idx = Some(idx);
                break;
            }
        }

        if found_matcher_idx.is_none() {
            arr.push(serde_json::json!({
                "matcher": "write_file|replace",
                "hooks": [{
                    "type": "command",
                    "command": capture_cmd
                }]
            }));
            changed = true;
            continue;
        }

        if let Some(idx) = found_matcher_idx {
            let Some(obj) = arr[idx].as_object_mut() else {
                continue;
            };
            let inner = obj
                .entry("hooks")
                .or_insert(serde_json::json!([]))
                .as_array_mut()
                .context("Gemini hooks entry must contain hooks array")?;

            let mut found_cmd = false;
            for hook in inner.iter_mut() {
                let Some(cmd_val) = hook.get_mut("command") else {
                    continue;
                };
                let Some(cmd) = cmd_val.as_str() else {
                    continue;
                };
                if cmd.contains("capture-antigravity.py") {
                    found_cmd = true;
                    if cmd != capture_cmd {
                        *cmd_val = serde_json::Value::String(capture_cmd.clone());
                        changed = true;
                    }
                }
            }

            if !found_cmd {
                inner.push(serde_json::json!({
                    "type": "command",
                    "command": capture_cmd
                }));
                changed = true;
            }

            let mut seen = std::collections::HashSet::new();
            inner.retain(|hook| {
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
    }

    let tools_obj = root
        .entry("tools")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .context("~/.gemini/settings.json tools must be an object")?;
    if tools_obj
        .get("enableHooks")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        != true
    {
        tools_obj.insert("enableHooks".to_string(), serde_json::Value::Bool(true));
        changed = true;
    }

    if changed {
        fs::create_dir_all(gemini_dir)?;
        fs::write(settings_path, serde_json::to_string_pretty(&cfg)?)?;
        println!(
            "{} Gemini CLI hooks configured in {}",
            ok(),
            settings_path.display()
        );
    } else {
        println!("{} Gemini CLI hooks already present", dim());
    }
    Ok(())
}

/// Append or update a managed rule block in ~/.gemini/GEMINI.md for Antigravity.
///
/// Antigravity has no event hooks; the rule instructs the agent to run the
/// capture script after each file edit. Reliability depends on the model
/// following the rule (best-effort), not a guaranteed callback.
fn step_configure_antigravity_rule(gemini_dir: &std::path::Path) -> Result<()> {
    let gemini_md_path = gemini_dir.join("GEMINI.md");

    let rule_block = format!(
        "{start}\n\
         ## agentdiff: record your file edits for attribution\n\
         \n\
         After every response where you edit or create files, append one JSON line **per \
         file** directly to `.git/agentdiff/session.jsonl` in the repository root. Write \
         the JSON yourself — do NOT run the capture-antigravity.py script (it is for \
         Gemini CLI hooks only and will hang if called without piped stdin).\n\
         \n\
         Use this exact format (no pretty-printing, one object per line):\n\
         \n\
         ```\n\
         {{\"timestamp\":\"<ISO-8601 UTC>\",\"agent\":\"antigravity\",\"mode\":\"agent\",\
\"model\":\"<your model name>\",\"session_id\":\"<conversation or session id>\",\
\"tool\":\"write_file\",\"file\":\"<repo-relative path>\",\"abs_file\":\"<absolute path>\",\
\"prompt\":\"<the user message that caused these edits>\",\"acceptance\":\"verbatim\",\
\"lines\":[<line numbers you added or changed>]}}\n\
         ```\n\
         \n\
         Rules:\n\
         - One JSON line per file edited (not one line per turn)\n\
         - `file` must be relative to the git repository root\n\
         - `lines` must be the actual line numbers you added or modified in this turn\n\
         - `prompt` must be the user message from **this chat** that triggered the edits\n\
         - Only write if `.git/agentdiff/session.jsonl` exists — if it is absent, the \
           repo has not opted into tracking; skip silently\n\
         {end}",
        start = GEMINI_MD_START,
        end = GEMINI_MD_END,
    );

    let existing = fs::read_to_string(&gemini_md_path).unwrap_or_default();

    // Check if our block already exists.
    if let Some(start_pos) = existing.find(GEMINI_MD_START) {
        if let Some(end_pos) = existing[start_pos..].find(GEMINI_MD_END) {
            let current_block = &existing[start_pos..start_pos + end_pos + GEMINI_MD_END.len()];
            if current_block == rule_block {
                println!(
                    "{} Antigravity GEMINI.md rule already up-to-date",
                    dim()
                );
                return Ok(());
            }
            // Update existing block (script path may have changed).
            let updated = format!(
                "{}{}{}",
                &existing[..start_pos],
                rule_block,
                &existing[start_pos + end_pos + GEMINI_MD_END.len()..]
            );
            fs::create_dir_all(gemini_dir)?;
            fs::write(&gemini_md_path, updated)?;
            println!(
                "{} Antigravity GEMINI.md rule updated in {}",
                ok(),
                gemini_md_path.display()
            );
            println!(
                "{}",
                "    Note: rule-based capture is best-effort (agent must follow the rule).".dimmed()
            );
            return Ok(());
        }
    }

    // Append block to end of file.
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let updated = format!("{}{}{}\n", existing, separator, rule_block);
    fs::create_dir_all(gemini_dir)?;
    fs::write(&gemini_md_path, updated)?;
    println!(
        "{} Antigravity GEMINI.md rule added to {}",
        ok(),
        gemini_md_path.display()
    );
    println!(
        "{}",
        "    Note: rule-based capture is best-effort (agent must follow the rule).".dimmed()
    );
    Ok(())
}
