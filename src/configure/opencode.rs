use crate::config::Config;
use crate::util::{dim, ok, warn};
use anyhow::Result;

use std::fs;

const OPENCODE_PLUGIN_TEMPLATE: &str = include_str!("../../scripts/opencode-agentdiff.ts");

pub fn step_configure_opencode(config: &Config) -> Result<()> {
    // Use the OpenCode global plugins directory (~/.config/opencode/plugins/).
    // This applies to all repos without needing per-repo setup.
    // dirs::config_dir() returns ~/.config on Linux, ~/Library/Application Support on macOS.
    let plugins_dir = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("opencode")
        .join("plugins");
    let plugin_path = plugins_dir.join("agentdiff.ts");

    let capture_script = config.scripts_root().join("capture-opencode.py");
    let plugin_content = OPENCODE_PLUGIN_TEMPLATE.replace(
        "__AGENTDIFF_CAPTURE_OPENCODE__",
        &capture_script.to_string_lossy(),
    );

    let existing = fs::read_to_string(&plugin_path).unwrap_or_default();
    if !existing.is_empty()
        && !existing.contains("agentdiff plugin for OpenCode")
        && existing != plugin_content
    {
        println!(
            "{} {} exists and is not managed by agentdiff — skipping",
            warn(),
            plugin_path.display()
        );
        return Ok(());
    }

    if existing != plugin_content {
        fs::create_dir_all(&plugins_dir)?;
        fs::write(&plugin_path, plugin_content)?;
        println!(
            "{} OpenCode plugin configured in {}",
            ok(),
            plugin_path.display()
        );
    } else {
        println!(
            "{} OpenCode plugin already present in {}",
            dim(),
            plugin_path.display()
        );
    }

    Ok(())
}
