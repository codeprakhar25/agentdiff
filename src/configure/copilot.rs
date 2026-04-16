use crate::config::Config;
use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;

const COPILOT_EXT_PACKAGE_JSON: &str =
    include_str!("../../scripts/vscode-extension/package.json");
const COPILOT_EXT_JS_TEMPLATE: &str =
    include_str!("../../scripts/vscode-extension/extension.js");

pub fn step_configure_copilot(config: &Config) -> Result<()> {
    let capture_script = config.scripts_root().join("capture-copilot.py");
    let ext_js = COPILOT_EXT_JS_TEMPLATE.replace(
        "__AGENTDIFF_CAPTURE_COPILOT__",
        &capture_script.to_string_lossy(),
    );

    // VS Code loads extensions placed directly in ~/.vscode/extensions/<name>-<version>/
    // This works on Linux, macOS, and Windows without any build step or vsce.
    let mut installed_any = false;
    for vscode_dir in vscode_extension_dirs() {
        let ext_dir = vscode_dir.join("agentdiff-copilot-0.1.0");
        if let Err(e) = fs::create_dir_all(&ext_dir) {
            println!(
                "{} cannot create {}: {}",
                "!".yellow(),
                ext_dir.display(),
                e
            );
            continue;
        }

        let pkg_path = ext_dir.join("package.json");
        let js_path = ext_dir.join("extension.js");

        let existing_pkg = fs::read_to_string(&pkg_path).unwrap_or_default();
        let existing_js = fs::read_to_string(&js_path).unwrap_or_default();

        let mut changed = false;
        if existing_pkg != COPILOT_EXT_PACKAGE_JSON {
            fs::write(&pkg_path, COPILOT_EXT_PACKAGE_JSON)
                .with_context(|| format!("writing {}", pkg_path.display()))?;
            changed = true;
        }
        if existing_js != ext_js {
            fs::write(&js_path, &ext_js)
                .with_context(|| format!("writing {}", js_path.display()))?;
            changed = true;
        }

        if changed {
            println!(
                "{} VS Code Copilot extension installed in {}",
                "ok".green(),
                ext_dir.display()
            );
            installed_any = true;
        } else {
            println!(
                "{} VS Code Copilot extension already up-to-date in {}",
                "--".dimmed(),
                ext_dir.display()
            );
            installed_any = true;
        }
    }

    if !installed_any {
        println!(
            "{} VS Code extensions directory not found — skipping Copilot setup",
            "!".yellow()
        );
        println!("    Checked: ~/.vscode-server/extensions, ~/.vscode/extensions, ~/.vscode-insiders/extensions");
        println!(
            "    To install manually: mkdir -p ~/.vscode-server/extensions/agentdiff-copilot-0.1.0 && cp {script_dir}/vscode-extension/* ~/.vscode-server/extensions/agentdiff-copilot-0.1.0/",
            script_dir = capture_script.parent().unwrap_or(capture_script.as_path()).display()
        );
    } else {
        println!("{} Restart VS Code to activate the agentdiff Copilot extension", "!".yellow());
    }

    Ok(())
}

/// Returns VS Code extension directories that exist on this machine.
///
/// Priority order:
/// 1. `~/.vscode-server/extensions`          — WSL2 / Remote-SSH extension host (Linux side).
///    Extensions here run inside WSL, so `python3` and file paths resolve correctly.
/// 2. `~/.vscode-server-insiders/extensions` — VS Code Insiders server variant.
/// 3. `~/.vscode/extensions`                 — VS Code stable on Linux / macOS / Windows.
/// 4. `~/.vscode-insiders/extensions`        — VS Code Insiders on Linux / macOS.
pub fn vscode_extension_dirs() -> Vec<std::path::PathBuf> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let candidates = vec![
        home.join(".vscode-server").join("extensions"),
        home.join(".vscode-server-insiders").join("extensions"),
        home.join(".vscode").join("extensions"),
        home.join(".vscode-insiders").join("extensions"),
    ];

    candidates.into_iter().filter(|p| p.exists()).collect()
}
