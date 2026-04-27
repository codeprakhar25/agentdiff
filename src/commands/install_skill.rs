use crate::cli::{InstallSkillArgs, SkillScope};
use crate::util::{ok, print_command_header, warn};
use anyhow::{Context, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};

const SKILL_CONTENT: &str = include_str!("../../.cursor/skills/agentdiff-context/SKILL.md");
const SKILL_REL_PATH: &[&str] = &["agentdiff-context", "SKILL.md"];

pub fn run(repo_root: &Path, args: &InstallSkillArgs) -> Result<()> {
    print_command_header("install-skill");

    let path = skill_path(repo_root, &args.scope)?;
    if path.exists() && !args.force {
        println!(
            "  {} {} already exists — skipping (use --force to overwrite)",
            warn(),
            path.display()
        );
        println!();
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, SKILL_CONTENT).with_context(|| format!("writing {}", path.display()))?;

    println!("  {} wrote {}", ok(), path.display());
    println!();
    println!("  Next steps:");
    match args.scope {
        SkillScope::Project => {
            println!(
                "    1. Commit {}",
                ".cursor/skills/agentdiff-context/SKILL.md".cyan()
            );
            println!("    2. Ask agents to use AgentDiff context before editing traced files");
        }
        SkillScope::Global => {
            println!("    1. Restart or refresh Cursor agents so the global skill is discovered");
            println!(
                "    2. Prefer project scope when repository-specific guidance should be versioned"
            );
        }
    }
    println!();
    Ok(())
}

fn skill_path(repo_root: &Path, scope: &SkillScope) -> Result<PathBuf> {
    match scope {
        SkillScope::Project => Ok(repo_root
            .join(".cursor")
            .join("skills")
            .join(SKILL_REL_PATH[0])
            .join(SKILL_REL_PATH[1])),
        SkillScope::Global => {
            let home = dirs::home_dir().context("could not determine home directory")?;
            Ok(home
                .join(".agents")
                .join("skills")
                .join(SKILL_REL_PATH[0])
                .join(SKILL_REL_PATH[1]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_scope_writes_under_repo_cursor_skills() {
        let path = skill_path(Path::new("/repo"), &SkillScope::Project).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/repo/.cursor/skills/agentdiff-context/SKILL.md")
        );
    }

    #[test]
    fn embedded_skill_has_expected_frontmatter() {
        assert!(SKILL_CONTENT.contains("name: agentdiff-context"));
        assert!(SKILL_CONTENT.contains("agentdiff context path/to/file --json"));
    }
}
