use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "agentdiff",
    about = "Audit and trace AI contributions in git repositories",
    version,
    propagate_version = true
)]
pub struct Cli {
    /// Path to repository (default: current directory)
    #[arg(short = 'C', long, global = true)]
    pub repo: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Configure global agent hooks (Claude, Cursor, Codex, Windsurf, OpenCode, Copilot) — run once per machine
    Configure(ConfigureArgs),

    /// Initialize agentdiff in this repository (install git hooks, create ledger)
    Init(InitArgs),

    /// List all captured attribution entries
    List(ListArgs),

    /// Show line-level attribution for a file (like git-blame)
    Blame(BlameArgs),

    /// Show aggregate statistics across agents and files
    Stats(StatsArgs),

    /// Generate CI report (markdown or GitHub annotations)
    Report(ReportArgs),

    /// Show attribution changes introduced by a commit range
    Diff(DiffArgs),

    /// Show chronological history of AI contributions
    Log(LogArgs),

    /// Show one ledger entry by commit SHA
    Show(ShowArgs),

    /// Ledger maintenance commands
    Ledger(LedgerArgs),

    /// Fetch refs/notes/agentdiff from origin
    SyncNotes,

    /// Manage global and repo-level configuration
    Config(ConfigArgs),
}

#[derive(Args, Debug)]
pub struct ConfigureArgs {
    /// Skip Claude Code hook setup
    #[arg(long)]
    pub no_claude: bool,

    /// Skip Cursor hook setup
    #[arg(long)]
    pub no_cursor: bool,

    /// Skip Codex hook setup
    #[arg(long)]
    pub no_codex: bool,

    /// Skip Gemini/Antigravity hook setup
    #[arg(long)]
    pub no_antigravity: bool,

    /// Skip Windsurf hook setup
    #[arg(long)]
    pub no_windsurf: bool,

    /// Skip OpenCode hook setup
    #[arg(long)]
    pub no_opencode: bool,

    /// Skip VS Code Copilot extension setup
    #[arg(long)]
    pub no_copilot: bool,
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Skip git pre-commit and post-commit hook setup
    #[arg(long)]
    pub no_git_hook: bool,

    /// Legacy migration flag (disabled in impl-1)
    #[arg(long)]
    pub migrate: bool,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Show only uncommitted (session) entries
    #[arg(long)]
    pub uncommitted: bool,

    /// Filter by agent name
    #[arg(long)]
    pub agent: Option<String>,

    /// Filter by file path (substring match)
    #[arg(long)]
    pub file: Option<String>,

    /// Limit output to N entries
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,
}

#[derive(Args, Debug)]
pub struct BlameArgs {
    /// File to blame (relative to repo root)
    pub file: std::path::PathBuf,

    /// Only show lines attributed to a specific agent
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args, Debug)]
pub struct StatsArgs {
    /// Show per-file breakdown
    #[arg(long)]
    pub by_file: bool,

    /// Show per-model breakdown
    #[arg(long)]
    pub by_model: bool,

    /// Only include entries since this ISO timestamp
    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Output format: markdown | annotations | both
    #[arg(long, default_value = "markdown")]
    pub format: ReportFormat,

    /// Write markdown to this file instead of stdout
    #[arg(long)]
    pub out_md: Option<std::path::PathBuf>,

    /// Write annotations JSON to this file instead of stdout
    #[arg(long)]
    pub out_annotations: Option<std::path::PathBuf>,

    /// Only include entries after this ISO timestamp
    #[arg(long)]
    pub since: Option<String>,

    /// Only include entries whose agent name contains this substring
    #[arg(long)]
    pub agent: Option<String>,

    /// Only include entries whose model name contains this substring
    #[arg(long)]
    pub model: Option<String>,
}

#[derive(Args, Debug)]
pub struct DiffArgs {
    /// Commit or range (e.g., HEAD~3, abc123..HEAD). Default: HEAD
    pub commit: Option<String>,

    /// Show only AI-attributed lines
    #[arg(long)]
    pub ai_only: bool,
}

#[derive(Args, Debug)]
pub struct LogArgs {
    /// Filter by agent
    #[arg(long)]
    pub agent: Option<String>,

    /// Limit to N entries
    #[arg(short = 'n', long, default_value = "20")]
    pub limit: usize,

    /// Show full prompt text
    #[arg(long)]
    pub full_prompt: bool,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Full SHA or SHA prefix from .agentdiff/ledger.jsonl
    pub sha: String,
}

#[derive(Args, Debug)]
pub struct LedgerArgs {
    #[command(subcommand)]
    pub action: LedgerAction,
}

#[derive(Subcommand, Debug)]
pub enum LedgerAction {
    /// Normalize, sort, and deduplicate ledger.jsonl
    Repair,
    /// Import legacy refs/notes/agentdiff records into ledger.jsonl
    ImportNotes,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show current config
    Show,
    /// Set a config value: spillover_dir | scripts_dir | auto_amend_ledger
    Set { key: String, value: String },
    /// Get a config value
    Get { key: String },
    /// Add a repo to the config
    AddRepo { path: std::path::PathBuf },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ReportFormat {
    Markdown,
    Annotations,
    Both,
}
