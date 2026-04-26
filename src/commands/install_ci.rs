use crate::cli::InstallCiArgs;
use crate::util::{ok, print_command_header, warn};
use anyhow::Result;
use colored::Colorize;
use std::path::Path;

const CONSOLIDATE_WORKFLOW: &str = r#"name: Consolidate Agent Traces

on:
  pull_request:
    types: [closed]

permissions:
  contents: write
  pull-requests: write

jobs:
  consolidate:
    if: github.event.pull_request.merged == true
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Fetch agentdiff refs
        run: |
          git fetch origin '+refs/agentdiff/*:refs/agentdiff/*' || true

      - name: Install agentdiff
        run: |
          curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/main/install.sh | bash
          echo "$HOME/.local/bin" >> $GITHUB_PATH

      - name: Consolidate traces
        run: |
          BRANCH="${{ github.head_ref }}"
          agentdiff consolidate --branch "$BRANCH" --push

      - name: Post attribution comment
        if: success()
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          PR="${{ github.event.pull_request.number }}"
          agentdiff report --format markdown --post-pr-comment "$PR" || true
"#;

const POLICY_WORKFLOW: &str = r#"name: AgentDiff Policy Check

on:
  pull_request:

permissions:
  contents: read
  checks: write
  pull-requests: write

jobs:
  policy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Fetch agentdiff refs
        run: |
          git fetch origin '+refs/agentdiff/*:refs/agentdiff/*' || true

      - name: Check out PR head branch
        run: |
          git checkout -B "${{ github.head_ref }}" "${{ github.event.pull_request.head.sha }}"

      - name: Install agentdiff
        run: |
          curl -fsSL https://raw.githubusercontent.com/codeprakhar25/agentdiff/main/install.sh | bash
          echo "$HOME/.local/bin" >> $GITHUB_PATH

      - name: Check policy
        run: |
          agentdiff policy check --format github-annotations

      - name: Post attribution comment
        if: always()
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          PR="${{ github.event.pull_request.number }}"
          agentdiff report --format markdown --post-pr-comment "$PR" || true
"#;

pub fn run(repo_root: &Path, args: &InstallCiArgs) -> Result<()> {
    print_command_header("install-ci");

    let workflows_dir = repo_root.join(".github").join("workflows");
    std::fs::create_dir_all(&workflows_dir)?;

    let consolidate_path = workflows_dir.join("agentdiff-consolidate.yml");
    let policy_path = workflows_dir.join("agentdiff-policy.yml");

    write_workflow(
        &consolidate_path,
        CONSOLIDATE_WORKFLOW,
        args.force,
        "agentdiff-consolidate.yml",
    )?;
    write_workflow(
        &policy_path,
        POLICY_WORKFLOW,
        args.force,
        "agentdiff-policy.yml",
    )?;

    println!();
    println!("  {}", "install-ci complete".bold().green());
    println!();
    println!("  Next steps:");
    println!("    1. Commit the workflow files:");
    println!(
        "       git add .github/workflows/agentdiff-consolidate.yml .github/workflows/agentdiff-policy.yml"
    );
    println!("       git commit -m 'ci: add agentdiff consolidation and policy workflows'");
    println!(
        "    2. Ensure each developer runs: {}",
        "agentdiff init".cyan()
    );
    println!(
        "    3. On merge, traces auto-consolidate and an attribution comment is posted to the PR."
    );

    Ok(())
}

fn write_workflow(path: &Path, content: &str, force: bool, label: &str) -> Result<()> {
    if path.exists() && !force {
        println!(
            "  {} {} already exists — skipping (use --force to overwrite)",
            warn(),
            label
        );
        return Ok(());
    }
    std::fs::write(path, content)?;
    println!("  {} wrote {}", ok(), path.display());
    Ok(())
}
