mod cli;
mod commands;
mod config;
mod configure;
mod data;
mod init;
mod keys;
mod store;
mod util;

use anyhow::Context;
use clap::Parser;
use cli::{Cli, Command};
use config::Config;

fn main() -> anyhow::Result<()> {
    // Suppress broken pipe errors (e.g. `agentdiff export | head`).
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();

    let config = Config::load().context("loading config")?;

    // Commands that don't require a git repository.
    let no_repo_needed = matches!(
        &cli.command,
        Command::Config(_) | Command::Configure(_) | Command::Keys(_) | Command::InstallCi(_)
    );

    // Resolve repo root
    let repo_root = if let Some(ref explicit) = cli.repo {
        explicit.clone()
    } else {
        match util::find_repo_root() {
            Ok(root) => root,
            Err(_) => {
                if no_repo_needed {
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
        Command::Configure(args) => {
            let mut cfg = config;
            configure::run_configure(
                &mut cfg,
                args.no_claude,
                args.no_cursor,
                args.no_codex,
                args.no_antigravity,
                args.no_windsurf,
                args.no_opencode,
                args.no_copilot,
                args.no_mcp,
            )
        }
        Command::Init(args) => {
            let mut cfg = config;
            init::run_init(&repo_root, &mut cfg, args.no_git_hook)
        }
        Command::List(args) => commands::list::run(&store, &args),
        Command::Blame(args) => commands::blame::run(&store, &args),
        Command::Report(args) => commands::report::run(&store, &args),
        Command::Diff(args) => commands::diff::run(&store, &args),
        Command::Show(args) => commands::show::run(&store, &args),
        Command::Config(args) => commands::config_cmd::run(&config, &args),
        Command::Keys(args) => match args.action {
            cli::KeysAction::Init => commands::keys::run_init(),
            cli::KeysAction::Register => commands::keys::run_register(&store),
            cli::KeysAction::Rotate => commands::keys::run_rotate(&store),
        },
        Command::Verify(args) => commands::verify::run(&store, &args),
        Command::Policy(args) => commands::policy::run(&store, &args.action),
        Command::Status(args) => commands::status::run(&store, &args),
        Command::Push(args) => commands::push::run(&store, &args),
        Command::Consolidate(args) => commands::consolidate::run(&store, &args),
        Command::InstallCi(args) => commands::install_ci::run(&repo_root, &args),
        Command::SignEntry => commands::sign_entry::run(&store),
    }
}
