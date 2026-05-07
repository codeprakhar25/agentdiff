mod agents_md;
mod antigravity;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod opencode;
mod windsurf;

use crate::cli::ConfigureArgs;
use crate::config::Config;
use crate::util::{dim, ok, warn};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, MultiSelect};
use std::{fs, io::IsTerminal, path::Path, path::PathBuf, process::Command};

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
pub fn run_configure(config: &mut Config, args: &ConfigureArgs, repo_root: &PathBuf) -> Result<()> {
    println!("{}", "agentdiff configure".bold().cyan());
    println!();
    println!(
        "     To disable prompt capture: {}",
        "agentdiff config set capture_prompts false".dimmed()
    );
    println!();

    let selection = resolve_agent_selection(args)?;

    // Check Python 3 availability — capture scripts require it.
    check_python3()?;

    // Step 1 — create global dirs
    step_create_dirs(config)?;

    // Step 2 — install Python scripts into ~/.agentdiff/scripts/
    step_install_scripts(config)?;

    // Step 3 — configure Claude Code ~/.claude/settings.json (hooks + MCP server)
    if selection.claude {
        claude::step_configure_claude(config)?;
    }
    if selection.claude && !args.no_mcp {
        claude::step_configure_mcp_claude()?;
    }

    // Step 4 — configure Cursor ~/.cursor/hooks.json
    if selection.cursor {
        cursor::step_configure_cursor(config)?;
    }

    // Step 5 — configure Codex ~/.codex/config.toml + ~/.codex/hooks.json
    if selection.codex {
        codex::step_configure_codex(config)?;
    }

    // Step 6 — configure Gemini / Antigravity hooks
    if selection.antigravity {
        antigravity::step_configure_antigravity(config)?;
    }

    // Step 7 — configure Windsurf globally (~/.codeium/windsurf/hooks.json)
    if selection.windsurf {
        windsurf::step_configure_windsurf(config)?;
    }

    // Step 8 — configure OpenCode globally (~/.config/opencode/plugins/)
    if selection.opencode {
        opencode::step_configure_opencode(config)?;
    }

    // Step 9 — install VS Code Copilot extension
    if selection.copilot {
        copilot::step_configure_copilot(config)?;
    }

    // Step 10 — write/update AgentDiff section in AGENTS.md (repo root, if available)
    if !args.no_agents_md {
        agents_md::step_configure_agents_md(repo_root)?;
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
        !selection.claude,
        !selection.cursor,
        !selection.codex,
        !selection.antigravity,
        !selection.windsurf,
        !selection.opencode,
        !selection.copilot,
    );
    println!();
    println!("{}", "agentdiff configure complete.".bold().green());
    println!(
        "{}",
        "Run 'agentdiff init' inside each repo you want to track.".dimmed()
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentTarget {
    Claude,
    Cursor,
    Codex,
    Windsurf,
    OpenCode,
    Copilot,
    Antigravity,
}

impl AgentTarget {
    fn all() -> &'static [AgentTarget] {
        &[
            AgentTarget::Claude,
            AgentTarget::Cursor,
            AgentTarget::Codex,
            AgentTarget::Windsurf,
            AgentTarget::OpenCode,
            AgentTarget::Copilot,
            AgentTarget::Antigravity,
        ]
    }

    fn display(self) -> &'static str {
        match self {
            AgentTarget::Claude => "Claude Code",
            AgentTarget::Cursor => "Cursor",
            AgentTarget::Codex => "Codex CLI",
            AgentTarget::Windsurf => "Windsurf",
            AgentTarget::OpenCode => "OpenCode",
            AgentTarget::Copilot => "VS Code Copilot",
            AgentTarget::Antigravity => "Gemini/Antigravity",
        }
    }

    fn default_selected(self) -> bool {
        !matches!(self, AgentTarget::Antigravity)
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            AgentTarget::Claude => &["claude", "claude-code", "claudecode"],
            AgentTarget::Cursor => &["cursor"],
            AgentTarget::Codex => &["codex", "codex-cli"],
            AgentTarget::Windsurf => &["windsurf"],
            AgentTarget::OpenCode => &["opencode", "open-code"],
            AgentTarget::Copilot => &["copilot", "github-copilot", "vscode-copilot"],
            AgentTarget::Antigravity => &["antigravity", "gemini", "gemini-cli"],
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        let normalized = name.trim().to_ascii_lowercase();
        AgentTarget::all()
            .iter()
            .copied()
            .find(|agent| agent.aliases().contains(&normalized.as_str()))
    }
}

#[derive(Clone, Copy, Debug)]
struct AgentSelection {
    claude: bool,
    cursor: bool,
    codex: bool,
    windsurf: bool,
    opencode: bool,
    copilot: bool,
    antigravity: bool,
}

impl AgentSelection {
    fn all() -> Self {
        Self {
            claude: true,
            cursor: true,
            codex: true,
            windsurf: true,
            opencode: true,
            copilot: true,
            antigravity: true,
        }
    }

    fn recommended() -> Self {
        Self {
            antigravity: false,
            ..Self::all()
        }
    }

    fn empty() -> Self {
        Self {
            claude: false,
            cursor: false,
            codex: false,
            windsurf: false,
            opencode: false,
            copilot: false,
            antigravity: false,
        }
    }

    fn set(&mut self, agent: AgentTarget, enabled: bool) {
        match agent {
            AgentTarget::Claude => self.claude = enabled,
            AgentTarget::Cursor => self.cursor = enabled,
            AgentTarget::Codex => self.codex = enabled,
            AgentTarget::Windsurf => self.windsurf = enabled,
            AgentTarget::OpenCode => self.opencode = enabled,
            AgentTarget::Copilot => self.copilot = enabled,
            AgentTarget::Antigravity => self.antigravity = enabled,
        }
    }

    fn apply_skip_flags(&mut self, args: &ConfigureArgs) {
        if args.no_claude {
            self.claude = false;
        }
        if args.no_cursor {
            self.cursor = false;
        }
        if args.no_codex {
            self.codex = false;
        }
        if args.no_windsurf {
            self.windsurf = false;
        }
        if args.no_opencode {
            self.opencode = false;
        }
        if args.no_copilot {
            self.copilot = false;
        }
        if args.no_antigravity {
            self.antigravity = false;
        }
    }
}

fn resolve_agent_selection(args: &ConfigureArgs) -> Result<AgentSelection> {
    if args.all && !args.agents.is_empty() {
        anyhow::bail!("use either --all or --agents, not both");
    }

    let mut selection = if args.all {
        AgentSelection::all()
    } else if !args.agents.is_empty() {
        let mut explicit = AgentSelection::empty();
        for raw in &args.agents {
            let agent = AgentTarget::from_name(raw)
                .ok_or_else(|| anyhow::anyhow!("unknown agent '{raw}' in --agents"))?;
            explicit.set(agent, true);
        }
        explicit
    } else if std::io::stdin().is_terminal() {
        prompt_agent_selection()?
    } else {
        println!(
            "{} non-interactive configure: using recommended agents (use --all for Gemini/Antigravity too)",
            dim()
        );
        AgentSelection::recommended()
    };

    selection.apply_skip_flags(args);
    Ok(selection)
}

fn prompt_agent_selection() -> Result<AgentSelection> {
    let detected = detect_agents();
    let items: Vec<String> = AgentTarget::all()
        .iter()
        .map(|agent| {
            let status = if detected.contains(agent) {
                "detected"
            } else {
                "not detected"
            };
            let default_note = if agent.default_selected() {
                "default"
            } else {
                "optional"
            };
            format!("{} ({status}, {default_note})", agent.display())
        })
        .collect();
    let defaults: Vec<bool> = AgentTarget::all()
        .iter()
        .map(|agent| agent.default_selected() && detected.contains(agent))
        .collect();

    println!("{}", "Select agents to configure:".bold());
    println!(
        "{}",
        "Use Space to toggle, Enter to continue. Gemini/Antigravity is optional by default."
            .dimmed()
    );
    let selected = MultiSelect::with_theme(&ColorfulTheme::default())
        .items(&items)
        .defaults(&defaults)
        .interact()
        .context("reading configure agent selection")?;

    let mut selection = AgentSelection::empty();
    for index in selected {
        if let Some(agent) = AgentTarget::all().get(index).copied() {
            selection.set(agent, true);
        }
    }
    Ok(selection)
}

fn detect_agents() -> Vec<AgentTarget> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut detected = Vec::new();

    if home.join(".claude").exists() {
        detected.push(AgentTarget::Claude);
    }
    if home.join(".cursor").exists() || windows_cursor_dir_exists() {
        detected.push(AgentTarget::Cursor);
    }
    if home.join(".codex").exists() {
        detected.push(AgentTarget::Codex);
    }
    if home.join(".codeium").join("windsurf").exists() {
        detected.push(AgentTarget::Windsurf);
    }
    if dirs::config_dir()
        .map(|dir| dir.join("opencode").exists())
        .unwrap_or(false)
    {
        detected.push(AgentTarget::OpenCode);
    }
    if copilot_extension_exists(&home) || windows_copilot_extension_exists() {
        detected.push(AgentTarget::Copilot);
    }
    if home.join(".gemini").exists() {
        detected.push(AgentTarget::Antigravity);
    }

    detected
}

fn copilot_extension_exists(home: &Path) -> bool {
    [
        ".vscode/extensions",
        ".vscode-server/extensions",
        ".vscode-insiders/extensions",
    ]
    .iter()
    .any(|path| extension_dir_has_copilot(&home.join(path)))
}

fn extension_dir_has_copilot(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("github.copilot")
            })
        })
        .unwrap_or(false)
}

fn windows_copilot_extension_exists() -> bool {
    let users = Path::new("/mnt/c/Users");
    fs::read_dir(users)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                let home = entry.path();
                [
                    ".vscode/extensions",
                    ".vscode-server/extensions",
                    ".vscode-insiders/extensions",
                ]
                .iter()
                .any(|path| extension_dir_has_copilot(&home.join(path)))
            })
        })
        .unwrap_or(false)
}

fn windows_cursor_dir_exists() -> bool {
    let users = Path::new("/mnt/c/Users");
    fs::read_dir(users)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                let path = entry.path().join(".cursor");
                path.exists()
            })
        })
        .unwrap_or(false)
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

    // Each tuple: (display name, skipped flag, presence-check path parts, config file path parts, marker string)
    // presence_parts: path that must exist for the tool to be considered installed (dir or file)
    // config_parts: path that contains the hooks (checked for marker string)
    let home_based: &[(&str, bool, &[&str], &[&str], &str)] = &[
        (
            "claude-code",
            no_claude,
            &[".claude"],
            &[".claude", "settings.json"],
            "capture-claude",
        ),
        (
            "cursor",
            no_cursor,
            &[".cursor"],
            &[".cursor", "hooks.json"],
            "capture-cursor",
        ),
        (
            "windsurf",
            no_windsurf,
            &[".codeium", "windsurf"],
            &[".codeium", "windsurf", "hooks.json"],
            "capture-windsurf",
        ),
    ];

    for (name, skipped, presence_parts, config_parts, marker) in home_based {
        if *skipped {
            println!(
                "  {} {}  skipped (--no-{})",
                dim(),
                name,
                name.replace('/', "-")
            );
            continue;
        }
        let presence_path = presence_parts
            .iter()
            .fold(home.clone(), |p, part| p.join(part));
        if !presence_path.exists() {
            println!("  {} {}  not installed on this machine", dim(), name);
            continue;
        }
        let config_path = config_parts
            .iter()
            .fold(home.clone(), |p, part| p.join(part));
        if !config_path.exists() {
            println!(
                "  {} {}  hook missing — re-run 'agentdiff configure'",
                warn(),
                name
            );
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
            println!(
                "  {} gemini/antigravity  not installed on this machine",
                dim()
            );
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
                (true, false) => println!(
                    "  {} codex  config.toml ok, hooks.json missing — re-run 'agentdiff configure'",
                    warn()
                ),
                (false, true) => println!(
                    "  {} codex  hooks.json ok, config.toml missing — re-run 'agentdiff configure'",
                    warn()
                ),
                (false, false) => println!(
                    "  {} codex  hook missing — re-run 'agentdiff configure'",
                    warn()
                ),
            }
        }
    } else {
        println!("  {} codex  skipped (--no-codex)", dim());
    }

    // OpenCode: platform-aware path (macOS: ~/Library/Application Support, Linux: ~/.config)
    if !no_opencode {
        let opencode_path =
            dirs::config_dir().map(|d| d.join("opencode").join("plugins").join("agentdiff.ts"));
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
            _ => println!("  {} opencode  not installed on this machine", dim()),
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
                if p.exists() {
                    Some(p)
                } else {
                    None
                }
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
            println!("  {} copilot  not installed on this machine", dim());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> ConfigureArgs {
        ConfigureArgs {
            all: false,
            agents: Vec::new(),
            no_claude: false,
            no_cursor: false,
            no_codex: false,
            no_antigravity: false,
            no_windsurf: false,
            no_opencode: false,
            no_copilot: false,
            no_mcp: false,
            no_agents_md: false,
        }
    }

    #[test]
    fn parses_agent_aliases() {
        assert_eq!(AgentTarget::from_name("claude"), Some(AgentTarget::Claude));
        assert_eq!(
            AgentTarget::from_name("codex-cli"),
            Some(AgentTarget::Codex)
        );
        assert_eq!(
            AgentTarget::from_name("github-copilot"),
            Some(AgentTarget::Copilot)
        );
        assert_eq!(
            AgentTarget::from_name("gemini"),
            Some(AgentTarget::Antigravity)
        );
        assert_eq!(AgentTarget::from_name("unknown"), None);
    }

    #[test]
    fn skip_flags_override_explicit_agents() {
        let mut args = args();
        args.agents = vec!["cursor".to_string(), "codex".to_string()];
        args.no_cursor = true;

        let selection = resolve_agent_selection(&args).unwrap();

        assert!(!selection.cursor);
        assert!(selection.codex);
        assert!(!selection.claude);
    }

    #[test]
    fn all_and_agents_are_mutually_exclusive() {
        let mut args = args();
        args.all = true;
        args.agents = vec!["cursor".to_string()];

        assert!(resolve_agent_selection(&args).is_err());
    }

    #[test]
    fn copilot_detection_requires_copilot_extension() {
        let root =
            std::env::temp_dir().join(format!("agentdiff-copilot-detect-{}", std::process::id()));
        let extensions = root.join(".vscode").join("extensions");
        fs::create_dir_all(extensions.join("rust-lang.rust-analyzer-1.0.0")).unwrap();

        assert!(!copilot_extension_exists(&root));

        fs::create_dir_all(extensions.join("github.copilot-1.2.3")).unwrap();
        assert!(copilot_extension_exists(&root));

        let _ = fs::remove_dir_all(root);
    }
}
