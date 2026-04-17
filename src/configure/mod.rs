mod antigravity;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod opencode;
mod windsurf;

use crate::config::Config;
use crate::util::{dim, ok, warn};
use anyhow::{Context, Result};
use colored::Colorize;
use std::{fs, process::Command};

// Script sources embedded at compile time.
const CLAUDE_CAPTURE_SCRIPT: &str = include_str!("../../scripts/capture-claude.py");
const CURSOR_CAPTURE_SCRIPT: &str = include_str!("../../scripts/capture-cursor.py");
const CODEX_CAPTURE_SCRIPT: &str = include_str!("../../scripts/capture-codex.py");
const WINDSURF_CAPTURE_SCRIPT: &str = include_str!("../../scripts/capture-windsurf.py");
const OPENCODE_CAPTURE_SCRIPT: &str = include_str!("../../scripts/capture-opencode.py");
const ANTIGRAVITY_CAPTURE_SCRIPT: &str = include_str!("../../scripts/capture-antigravity.py");
const COPILOT_CAPTURE_SCRIPT: &str = include_str!("../../scripts/capture-copilot.py");
const PREPARE_LEDGER_SCRIPT: &str = include_str!("../../scripts/prepare-ledger.py");
const FINALIZE_LEDGER_SCRIPT: &str = include_str!("../../scripts/finalize-ledger.py");
const RECORD_CONTEXT_SCRIPT: &str = include_str!("../../scripts/record-context.py");
const WRITE_NOTE_SCRIPT: &str = include_str!("../../scripts/write-note.py");

/// Configure global agent hooks — run once per machine, no git repo required.
pub fn run_configure(
    config: &mut Config,
    no_claude: bool,
    no_cursor: bool,
    no_codex: bool,
    no_antigravity: bool,
    no_windsurf: bool,
    no_opencode: bool,
    no_copilot: bool,
    no_mcp: bool,
) -> Result<()> {
    println!("{}", "agentdiff configure".bold().cyan());
    println!();
    println!(
        "     To disable prompt capture: {}",
        "agentdiff config set capture_prompts false".dimmed()
    );
    println!();

    // Check Python 3 availability — capture scripts require it.
    check_python3()?;

    // Step 1 — create global dirs
    step_create_dirs(config)?;

    // Step 2 — install Python scripts into ~/.agentdiff/scripts/
    step_install_scripts(config)?;

    // Step 3 — configure Claude Code ~/.claude/settings.json (hooks + MCP server)
    if !no_claude {
        claude::step_configure_claude(config)?;
    }
    if !no_mcp {
        claude::step_configure_mcp_claude()?;
    }

    // Step 4 — configure Cursor ~/.cursor/hooks.json
    if !no_cursor {
        cursor::step_configure_cursor(config)?;
    }

    // Step 5 — configure Codex ~/.codex/config.toml + ~/.codex/hooks.json
    if !no_codex {
        codex::step_configure_codex(config)?;
    }

    // Step 6 — configure Gemini / Antigravity hooks
    if !no_antigravity {
        antigravity::step_configure_antigravity(config)?;
    }

    // Step 7 — configure Windsurf globally (~/.codeium/windsurf/hooks.json)
    if !no_windsurf {
        windsurf::step_configure_windsurf(config)?;
    }

    // Step 8 — configure OpenCode globally (~/.config/opencode/plugins/)
    if !no_opencode {
        opencode::step_configure_opencode(config)?;
    }

    // Step 9 — install VS Code Copilot extension
    if !no_copilot {
        copilot::step_configure_copilot(config)?;
    }

    // Save updated config
    config.save()?;
    println!(
        "{} Config written to {}",
        ok(),
        Config::config_path().display()
    );

    println!();
    print_configure_summary(
        config,
        no_claude,
        no_cursor,
        no_codex,
        no_antigravity,
        no_windsurf,
        no_opencode,
        no_copilot,
    );
    println!();
    println!("{}", "agentdiff configure complete.".bold().green());
    println!(
        "{}",
        "Run 'agentdiff init' inside each repo you want to track.".dimmed()
    );
    Ok(())
}

fn print_configure_summary(
    _config: &Config,
    no_claude: bool,
    no_cursor: bool,
    no_codex: bool,
    no_antigravity: bool,
    no_windsurf: bool,
    no_opencode: bool,
    no_copilot: bool,
) {
    println!("{}", "Hook summary:".bold());
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };

    // Each tuple: (display name, skipped flag, config path joined from home, marker string)
    let home_based: &[(&str, bool, &[&str], &str)] = &[
        (
            "claude-code",
            no_claude,
            &[".claude", "settings.json"],
            "capture-claude",
        ),
        (
            "cursor",
            no_cursor,
            &[".cursor", "hooks.json"],
            "capture-cursor",
        ),
        (
            "windsurf",
            no_windsurf,
            &[".codeium", "windsurf", "hooks.json"],
            "capture-windsurf",
        ),
    ];

    for (name, skipped, path_parts, marker) in home_based {
        if *skipped {
            println!(
                "  {} {}  skipped (--no-{})",
                dim(),
                name,
                name.replace('/', "-")
            );
            continue;
        }
        let config_path = path_parts.iter().fold(home.clone(), |p, part| p.join(part));
        if !config_path.exists() {
            println!("  {} {}  not installed on this machine", dim(), name);
            continue;
        }
        let registered = std::fs::read_to_string(&config_path)
            .map(|s| s.contains(marker))
            .unwrap_or(false);
        if registered {
            println!("  {} {}  registered", ok(), name);
        } else {
            println!(
                "  {} {}  hook missing — re-run 'agentdiff configure'",
                warn(),
                name
            );
        }
    }

    // Gemini CLI + Antigravity: two separate products, two separate files.
    if !no_antigravity {
        let gemini_dir = home.join(".gemini");
        if !gemini_dir.exists() {
            println!("  {} gemini/antigravity  not installed on this machine", dim());
        } else {
            // Gemini CLI: settings.json hooks
            let cli_ok = std::fs::read_to_string(gemini_dir.join("settings.json"))
                .map(|s| s.contains("capture-antigravity"))
                .unwrap_or(false);
            // Antigravity editor: GEMINI.md managed block
            let rule_ok = std::fs::read_to_string(gemini_dir.join("GEMINI.md"))
                .map(|s| s.contains("agentdiff: managed block"))
                .unwrap_or(false);
            match (cli_ok, rule_ok) {
                (true, true) => println!("  {} gemini-cli  hooks registered; antigravity  GEMINI.md rule set", ok()),
                (true, false) => println!("  {} gemini-cli  hooks ok; {} antigravity  GEMINI.md rule missing — re-run 'agentdiff configure'", ok(), warn()),
                (false, true) => println!("  {} gemini-cli  hooks missing; {} antigravity  GEMINI.md rule ok — re-run 'agentdiff configure'", warn(), ok()),
                (false, false) => println!("  {} gemini/antigravity  hooks missing — re-run 'agentdiff configure'", warn()),
            }
        }
    } else {
        println!("  {} gemini/antigravity  skipped (--no-antigravity)", dim());
    }

    // Codex: check both config.toml and hooks.json.
    if !no_codex {
        let codex_dir = home.join(".codex");
        if !codex_dir.exists() {
            println!("  {} codex  not installed on this machine", dim());
        } else {
            let toml_ok = std::fs::read_to_string(codex_dir.join("config.toml"))
                .map(|s| s.contains("capture-codex"))
                .unwrap_or(false);
            let hooks_ok = std::fs::read_to_string(codex_dir.join("hooks.json"))
                .map(|s| s.contains("capture-codex"))
                .unwrap_or(false);
            match (toml_ok, hooks_ok) {
                (true, true) => println!("  {} codex  registered (config.toml + hooks.json)", ok()),
                (true, false) => println!("  {} codex  config.toml ok, hooks.json missing — re-run 'agentdiff configure'", warn()),
                (false, true) => println!("  {} codex  hooks.json ok, config.toml missing — re-run 'agentdiff configure'", warn()),
                (false, false) => println!("  {} codex  hook missing — re-run 'agentdiff configure'", warn()),
            }
        }
    } else {
        println!("  {} codex  skipped (--no-codex)", dim());
    }

    // OpenCode: platform-aware path (macOS: ~/Library/Application Support, Linux: ~/.config)
    if !no_opencode {
        let opencode_path = dirs::config_dir()
            .map(|d| d.join("opencode").join("plugins").join("agentdiff.ts"));
        match opencode_path {
            Some(ref p) if p.exists() => {
                let registered = std::fs::read_to_string(p)
                    .map(|s| s.contains("agentdiff"))
                    .unwrap_or(false);
                if registered {
                    println!("  {} opencode  registered", ok());
                } else {
                    println!(
                        "  {} opencode  hook missing — re-run 'agentdiff configure'",
                        warn()
                    );
                }
            }
            _ => println!(
                "  {} opencode  not installed on this machine",
                dim()
            ),
        }
    } else {
        println!("  {} opencode  skipped (--no-opencode)", dim());
    }

    // Copilot: directory-based check across all VS Code install locations.
    if !no_copilot {
        let vscode_dirs = [
            ".vscode/extensions",
            ".vscode-server/extensions",
            ".vscode-insiders/extensions",
        ];
        let found = vscode_dirs
            .iter()
            .filter_map(|d| {
                let p = home.join(d);
                if p.exists() { Some(p) } else { None }
            })
            .any(|d| {
                std::fs::read_dir(&d)
                    .map(|mut e| {
                        e.any(|e| {
                            e.map(|e| {
                                e.file_name()
                                    .to_string_lossy()
                                    .starts_with("agentdiff-copilot")
                            })
                            .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            });
        let any_vscode = vscode_dirs.iter().any(|d| home.join(d).exists());
        if !any_vscode {
            println!(
                "  {} copilot  not installed on this machine",
                dim()
            );
        } else if found {
            println!("  {} copilot  registered", ok());
        } else {
            println!(
                "  {} copilot  extension not found — re-run 'agentdiff configure'",
                warn()
            );
        }
    } else {
        println!("  {} copilot  skipped (--no-copilot)", dim());
    }

    println!(
        "\n  {} Make an AI-assisted edit, commit, then run {} to see attribution.",
        "→".cyan(),
        "agentdiff list".cyan()
    );
}

fn check_python3() -> Result<()> {
    // On Windows, `python3` may not exist; `python` is the common name.
    let python_cmd = if cfg!(windows) { "python" } else { "python3" };
    let output = Command::new(python_cmd).arg("--version").output();
    match output {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            println!("{} {python_cmd} found: {ver}", ok());
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "{python_cmd} not found on PATH.\n\
             agentdiff capture scripts require Python 3.\n\
             Install Python 3 and ensure it is on your PATH, then re-run 'agentdiff configure'."
        )),
    }
}

fn step_create_dirs(config: &Config) -> Result<()> {
    let dirs_to_create = [
        config.scripts_root(),
        Config::config_path().parent().unwrap().to_path_buf(),
    ];
    for dir in &dirs_to_create {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        println!("{} mkdir {}", dim(), dir.display());
    }
    Ok(())
}

fn step_install_scripts(config: &Config) -> Result<()> {
    let scripts = [
        ("capture-claude.py", CLAUDE_CAPTURE_SCRIPT),
        ("capture-cursor.py", CURSOR_CAPTURE_SCRIPT),
        ("capture-codex.py", CODEX_CAPTURE_SCRIPT),
        ("capture-windsurf.py", WINDSURF_CAPTURE_SCRIPT),
        ("capture-opencode.py", OPENCODE_CAPTURE_SCRIPT),
        ("capture-antigravity.py", ANTIGRAVITY_CAPTURE_SCRIPT),
        ("capture-copilot.py", COPILOT_CAPTURE_SCRIPT),
        ("prepare-ledger.py", PREPARE_LEDGER_SCRIPT),
        ("finalize-ledger.py", FINALIZE_LEDGER_SCRIPT),
        ("record-context.py", RECORD_CONTEXT_SCRIPT),
        ("write-note.py", WRITE_NOTE_SCRIPT),
    ];
    for (name, content) in &scripts {
        let dest = config.scripts_root().join(name);
        let existing = fs::read_to_string(&dest).unwrap_or_default();
        if existing != *content {
            fs::write(&dest, content).with_context(|| format!("writing {}", dest.display()))?;
            println!("{} installed {}", ok(), dest.display());
        } else {
            println!("{} up-to-date {}", dim(), dest.display());
        }
    }
    Ok(())
}
