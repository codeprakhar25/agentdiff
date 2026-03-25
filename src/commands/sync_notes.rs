use crate::store::Store;
use colored::Colorize;

pub fn run(store: &Store) -> anyhow::Result<()> {
    println!("{}", "agentdiff sync-notes".cyan().bold());
    println!(
        "{} sync-notes is legacy; committed source-of-truth is .agentdiff/ledger.jsonl",
        "!".yellow()
    );

    let origin_url = std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(&store.repo_root)
        .output()?;
    let has_origin = origin_url.status.success()
        && !String::from_utf8_lossy(&origin_url.stdout)
            .trim()
            .is_empty();

    if !has_origin {
        anyhow::bail!("No git remote named 'origin' found");
    }

    let has_remote_notes = std::process::Command::new("git")
        .args([
            "-c",
            "credential.helper=",
            "ls-remote",
            "--exit-code",
            "origin",
            "refs/notes/agentdiff",
        ])
        .current_dir(&store.repo_root)
        .output()?
        .status
        .success();

    if !has_remote_notes {
        println!(
            "{} no remote refs/notes/agentdiff yet (nothing to fetch)",
            "--".dimmed()
        );
        return Ok(());
    }

    let status = std::process::Command::new("git")
        .args(["-c", "credential.helper="])
        .args([
            "fetch",
            "origin",
            "refs/notes/agentdiff:refs/notes/agentdiff",
        ])
        .current_dir(&store.repo_root)
        .status()?;

    if !status.success() {
        anyhow::bail!("Failed to fetch refs/notes/agentdiff from origin");
    }

    println!("{} fetched refs/notes/agentdiff", "ok".green());
    Ok(())
}
