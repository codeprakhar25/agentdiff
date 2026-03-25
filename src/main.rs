mod cli;
mod commands;
mod config;
mod data;
mod init;
mod store;
mod util;

use anyhow::Context;
use clap::Parser;
use cli::{Cli, Command};
use config::Config;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = Config::load().context("loading config")?;

    // Resolve repo root
    let repo_root = if let Some(ref explicit) = cli.repo {
        explicit.clone()
    } else {
        // Try to find git repo, fall back to cwd for non-repo commands
        match util::find_repo_root() {
            Ok(root) => root,
            Err(_) => {
                // Config commands can run outside a git repo.
                if matches!(&cli.command, Command::Config(_)) {
                    std::env::current_dir()?
                } else {
                    return Err(anyhow::anyhow!(
                        "Not in a git repository. Run agentdiff init in a repo first."
                    ));
                }
            }
        }
    };

    let store = store::Store::new(repo_root.clone(), config.clone());

    match cli.command {
        Command::Init(args) => {
            let mut cfg = config;
            init::run_init(
                &repo_root,
                &mut cfg,
                args.no_claude,
                args.no_cursor,
                args.no_codex,
                args.no_windsurf,
                args.no_opencode,
                args.no_git_hook,
                args.migrate,
            )
        }
        Command::List(args) => commands::list::run(&store, &args),
        Command::Blame(args) => commands::blame::run(&store, &args),
        Command::Stats(args) => commands::stats::run(&store, &args),
        Command::Report(args) => commands::report::run(&store, &args),
        Command::Diff(args) => commands::diff::run(&store, &args),
        Command::Log(args) => commands::log::run(&store, &args),
        Command::SyncNotes => commands::sync_notes::run(&store),
        Command::Config(args) => commands::config_cmd::run(&config, &args),
    }
}
