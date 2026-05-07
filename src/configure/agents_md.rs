use crate::util::{dim, ok};
use anyhow::Result;
use std::path::Path;

const AGENTS_MD_START: &str = "<!-- agentdiff: managed block — do not edit -->";
const AGENTS_MD_END: &str = "<!-- end agentdiff -->";

fn managed_block() -> String {
    format!(
        "{start}\n\
         ## AgentDiff\n\
         \n\
         [AgentDiff](https://github.com/codeprakhar25/agentdiff) tracks which AI agent \
         wrote which lines of code in this repository. Every file edit made through a \
         configured agent is captured and stored as a signed `AgentTrace` record in \
         `.git/agentdiff/traces/`. Attribution is computed per-commit and covers all \
         configured agents: Claude Code, Cursor, Codex, Copilot, Windsurf, OpenCode, \
         and Gemini.\n\
         \n\
         ### Before editing traced files\n\
         \n\
         Check attribution context to understand prior AI contributions:\n\
         \n\
         ```bash\n\
         agentdiff context path/to/file\n\
         agentdiff context path/to/file --json\n\
         ```\n\
         \n\
         ### Before committing\n\
         \n\
         Let the git hooks run — do **not** bypass them with `--no-verify`. The \
         `pre-commit` hook computes per-file attribution, and the `post-commit` hook \
         signs and stores the trace. Skipping either breaks the attribution ledger.\n\
         \n\
         ### When reviewing PRs\n\
         \n\
         ```bash\n\
         agentdiff report --format markdown   # Aggregate attribution summary\n\
         agentdiff blame src/main.rs           # Line-level attribution\n\
         agentdiff diff HEAD                   # Attribution changes in a commit\n\
         ```\n\
         \n\
         ### Attribution conventions\n\
         \n\
         - Files you edit without an AI agent are attributed to `human`\n\
         - Files changed by an agent use its name: `claude-code`, `cursor`, `codex`, etc.\n\
         - Copilot inline completions are tracked for stats but excluded from file attribution\n\
         - When multiple agents touch a file in one session, the majority-lines agent wins\n\
         {end}",
        start = AGENTS_MD_START,
        end = AGENTS_MD_END,
    )
}

pub fn step_configure_agents_md(repo_root: &Path) -> Result<()> {
    let agents_md_path = repo_root.join("AGENTS.md");
    let block = managed_block();

    let existing = std::fs::read_to_string(&agents_md_path).unwrap_or_default();

    if let Some(start_pos) = existing.find(AGENTS_MD_START) {
        if let Some(rel_end) = existing[start_pos..].find(AGENTS_MD_END) {
            let end_pos = start_pos + rel_end + AGENTS_MD_END.len();
            let current_block = &existing[start_pos..end_pos];
            if current_block == block {
                println!("{} AGENTS.md AgentDiff section already up-to-date", dim());
                return Ok(());
            }
            // Update existing block in place.
            let updated = format!("{}{}{}", &existing[..start_pos], block, &existing[end_pos..]);
            std::fs::write(&agents_md_path, updated)?;
            println!(
                "{} AGENTS.md AgentDiff section updated in {}",
                ok(),
                agents_md_path.display()
            );
            return Ok(());
        }
    }

    // Append block to file (create if absent).
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let updated = format!("{}{}{}\n", existing, separator, block);
    std::fs::write(&agents_md_path, updated)?;

    if existing.is_empty() {
        println!(
            "{} AGENTS.md created with AgentDiff section at {}",
            ok(),
            agents_md_path.display()
        );
    } else {
        println!(
            "{} AGENTS.md AgentDiff section added to {}",
            ok(),
            agents_md_path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("agentdiff-agents-md-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_agents_md_from_scratch() {
        let dir = tmp_dir().join("scratch");
        fs::create_dir_all(&dir).unwrap();

        step_configure_agents_md(&dir).unwrap();

        let content = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert!(content.contains(AGENTS_MD_START));
        assert!(content.contains(AGENTS_MD_END));
        assert!(content.contains("## AgentDiff"));
        assert!(content.contains("agentdiff context path/to/file"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn idempotent_when_block_unchanged() {
        let dir = tmp_dir().join("idempotent");
        fs::create_dir_all(&dir).unwrap();

        step_configure_agents_md(&dir).unwrap();
        let first = fs::read_to_string(dir.join("AGENTS.md")).unwrap();

        step_configure_agents_md(&dir).unwrap();
        let second = fs::read_to_string(dir.join("AGENTS.md")).unwrap();

        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn updates_existing_block_without_duplicating() {
        let dir = tmp_dir().join("update");
        fs::create_dir_all(&dir).unwrap();

        // Write a stale block.
        let stale = format!(
            "# My Project\n\n{start}\n## AgentDiff\n\nOld content here.\n{end}\n\n## Other section\n",
            start = AGENTS_MD_START,
            end = AGENTS_MD_END,
        );
        fs::write(dir.join("AGENTS.md"), &stale).unwrap();

        step_configure_agents_md(&dir).unwrap();

        let content = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        // Should have exactly one managed block.
        assert_eq!(content.matches(AGENTS_MD_START).count(), 1);
        assert_eq!(content.matches(AGENTS_MD_END).count(), 1);
        // Old content replaced.
        assert!(!content.contains("Old content here."));
        // Surrounding content preserved.
        assert!(content.contains("# My Project"));
        assert!(content.contains("## Other section"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn appends_to_existing_agents_md_without_managed_block() {
        let dir = tmp_dir().join("append");
        fs::create_dir_all(&dir).unwrap();

        let pre_existing = "# My Project\n\nSome existing content.\n";
        fs::write(dir.join("AGENTS.md"), pre_existing).unwrap();

        step_configure_agents_md(&dir).unwrap();

        let content = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert!(content.starts_with("# My Project"));
        assert!(content.contains("Some existing content."));
        assert!(content.contains(AGENTS_MD_START));
        assert!(content.contains("## AgentDiff"));

        let _ = fs::remove_dir_all(&dir);
    }
}
