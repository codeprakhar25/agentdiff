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

    /// Show agent context for a file
    Context(ContextArgs),

    /// Aggregate report in text, markdown, annotations, or JSONL format
    Report(ReportArgs),

    /// Show attribution changes introduced by a commit range
    Diff(DiffArgs),

    /// Show one trace entry by UUID or commit SHA
    Show(ShowArgs),

    /// Manage global and repo-level configuration
    Config(ConfigArgs),

    /// Manage local signing keypair
    Keys(KeysArgs),

    /// Verify ledger entry signatures
    Verify(VerifyArgs),

    /// Evaluate agentdiff policy rules
    Policy(PolicyArgs),

    /// Show current agentdiff health (hooks, keys, traces)
    Status(StatusArgs),

    /// Push local traces to per-branch ref on origin
    Push(PushArgs),

    /// Consolidate per-branch ref traces into agentdiff-meta (used by CI)
    Consolidate(ConsolidateArgs),

    /// Write agentdiff CI workflow files to .github/workflows/
    InstallCi(InstallCiArgs),

    /// Install the AgentDiff context skill for Cursor agents
    InstallSkill(InstallSkillArgs),

    /// [internal] Sign the last trace entry — called by the post-commit hook
    #[command(hide = true)]
    SignEntry,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Show remote agentdiff ref state (refs/agentdiff/* on origin)
    #[arg(long)]
    pub remote: bool,

    /// With --remote: skip fetching trace counts for each ref (faster)
    #[arg(long)]
    pub no_fetch: bool,
}

#[derive(Args, Debug)]
pub struct ConfigureArgs {
    /// Configure every supported agent without prompting
    #[arg(long)]
    pub all: bool,

    /// Configure only these agents (comma-separated: claude-code,cursor,codex,windsurf,opencode,copilot,antigravity)
    #[arg(long, value_delimiter = ',', value_name = "AGENTS")]
    pub agents: Vec<String>,

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

    /// Skip MCP server registration with Claude Code
    #[arg(long)]
    pub no_mcp: bool,
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Skip git pre-commit and post-commit hook setup
    #[arg(long)]
    pub no_git_hook: bool,
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

    /// Limit output to N entries (or N commits with --by-commit)
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,

    /// Group entries by commit and display chronologically (replaces `log`)
    #[arg(long)]
    pub by_commit: bool,

    /// With --by-commit: show full prompt text
    #[arg(long)]
    pub full_prompt: bool,
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
pub struct ContextArgs {
    /// File to explain (relative to repo root)
    pub file: std::path::PathBuf,

    /// Output machine-readable JSON
    #[arg(long)]
    pub json: bool,

    /// Only include traces whose agent name contains this substring
    #[arg(long)]
    pub agent: Option<String>,

    /// Limit number of trace records shown
    #[arg(short = 'n', long, default_value_t = 10)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Output format: text (default, terminal-friendly) | markdown | annotations | jsonl | json
    #[arg(long, default_value = "text")]
    pub format: ReportFormat,

    /// Write output to a file instead of stdout
    #[arg(long)]
    pub out: Option<std::path::PathBuf>,

    /// Post the markdown report as a PR comment (requires gh CLI and GH_TOKEN).
    /// Provide PR number, or omit to auto-detect from the current branch.
    #[arg(long, value_name = "PR_NUMBER")]
    pub post_pr_comment: Option<Option<u64>>,

    /// Only include entries after this ISO timestamp
    #[arg(long)]
    pub since: Option<String>,

    /// Only include entries whose agent name contains this substring
    #[arg(long)]
    pub agent: Option<String>,

    /// Only include entries whose model name contains this substring
    #[arg(long)]
    pub model: Option<String>,

    /// Include structured intent/files-read context in markdown reports (JSON is always structured)
    #[arg(long)]
    pub context: bool,

    /// With --format=text: also show per-file breakdown
    #[arg(long)]
    pub by_file: bool,

    /// With --format=text: also show per-model breakdown
    #[arg(long)]
    pub by_model: bool,
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
pub struct ShowArgs {
    /// UUID or commit SHA prefix to look up
    pub sha: String,
}

#[derive(Args, Debug)]
pub struct PushArgs {
    /// Branch to push traces for (default: current branch)
    #[arg(long)]
    pub branch: Option<String>,

    /// Suppress output
    #[arg(long)]
    pub quiet: bool,
}

#[derive(Args, Debug)]
pub struct ConsolidateArgs {
    /// Branch whose traces to consolidate into agentdiff-meta
    #[arg(long)]
    pub branch: Option<String>,

    /// Also push agentdiff-meta and delete the remote per-branch ref
    #[arg(long)]
    pub push: bool,
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
    /// Set a config value: scripts_dir | capture_prompts
    Set { key: String, value: String },
    /// Get a config value
    Get { key: String },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ReportFormat {
    /// Terminal display (replaces `stats`)
    Text,
    /// Markdown report
    Markdown,
    /// GitHub check annotations JSON
    Annotations,
    /// Agent Trace JSONL (replaces `export`)
    Jsonl,
    /// Structured JSON summary
    Json,
}

// ── Keys ─────────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct KeysArgs {
    #[command(subcommand)]
    pub action: KeysAction,
}

#[derive(Subcommand, Debug)]
pub enum KeysAction {
    /// Generate a new local ed25519 signing keypair
    Init,
    /// Register the local public key in the git key registry (refs/agentdiff/keys/)
    Register,
    /// Rotate the local keypair: back up the old keys, generate new ones, and register them
    Rotate,
}

// ── Verify ────────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// Verify only entries after this commit SHA (default: git merge-base with main)
    #[arg(long)]
    pub since: Option<String>,

    /// Treat missing signatures as hard failures (exit 1 immediately)
    #[arg(long)]
    pub strict: bool,
}

// ── Policy ────────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct PolicyArgs {
    #[command(subcommand)]
    pub action: PolicyAction,
}

#[derive(Subcommand, Debug)]
pub enum PolicyAction {
    /// Evaluate policy rules against current ledger
    Check(PolicyCheckArgs),
}

#[derive(Args, Debug)]
pub struct PolicyCheckArgs {
    /// Only evaluate commits after this SHA (default: git merge-base with main)
    #[arg(long)]
    pub since: Option<String>,

    /// Output format: text | github-annotations
    #[arg(long, default_value = "text")]
    pub format: PolicyFormat,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum PolicyFormat {
    Text,
    GithubAnnotations,
}

// ── InstallCi ────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct InstallCiArgs {
    /// Overwrite existing workflow files without prompting
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct InstallSkillArgs {
    /// Where to install the skill: project writes .cursor/skills, global writes ~/.agents/skills
    #[arg(long, default_value = "project")]
    pub scope: SkillScope,

    /// Overwrite an existing skill file
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, clap::ValueEnum, PartialEq, Eq)]
pub enum SkillScope {
    Project,
    Global,
}
