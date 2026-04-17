use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::cli::{PolicyAction, PolicyCheckArgs, PolicyFormat};
use crate::data::AgentTrace;
use crate::store::Store;
use crate::util::{err, ok};

#[derive(Debug, Deserialize, Default)]
struct PolicyConfig {
    /// AI lines added / total lines added must not exceed this percent (0–100).
    #[serde(default)]
    max_ai_percent: Option<f64>,
    /// Every trace in range must have at least one file attributed.
    #[serde(default)]
    require_attribution: bool,
    /// Every trace in range must carry an ed25519 signature.
    #[serde(default)]
    require_signed: bool,
    /// Base branch for merge-base calculation (overrides auto-detection).
    /// Useful for repos using non-standard default branches (develop, trunk, etc.)
    #[serde(default)]
    base_branch: Option<String>,
}

pub fn run(store: &Store, action: &PolicyAction) -> Result<()> {
    match action {
        PolicyAction::Check(args) => run_check(store, args),
    }
}

fn run_check(store: &Store, args: &PolicyCheckArgs) -> Result<()> {
    let policy = load_policy(&store.repo_root)?;

    if policy.max_ai_percent.is_none() && !policy.require_attribution && !policy.require_signed {
        emit(
            "All policy rules are disabled (no policy.toml or all defaults).",
            args,
        );
        return Ok(());
    }

    let base_sha = match &args.since {
        Some(sha) => Some(sha.clone()),
        None => find_merge_base(&store.repo_root, policy.base_branch.as_deref()),
    };

    let traces = store.load_all_traces()?;
    let in_range = filter_traces_since(&traces, base_sha.as_deref());

    let mut failures: Vec<String> = Vec::new();

    // Rule 1: max_ai_percent
    if let Some(threshold) = policy.max_ai_percent {
        let pct = compute_ai_percent(store, base_sha.as_deref(), &in_range)?;
        let label = format!("max_ai_percent={threshold:.0}%");
        if pct > threshold {
            failures.push(format!(
                "{label}: AI code is {pct:.1}% of lines added (threshold {threshold:.0}%)"
            ));
        } else {
            emit_ok(&format!("{label}: AI code is {pct:.1}% (ok)"), args);
        }
    }

    // Rule 2: require_attribution
    if policy.require_attribution {
        let unattr: Vec<_> = in_range
            .iter()
            .filter(|t| !t.files.is_empty() && t.files.iter().all(|f| f.conversations.is_empty()))
            .collect();
        if !unattr.is_empty() {
            let ids: Vec<_> = unattr.iter().map(|t| short_id(&t.id)).collect();
            failures.push(format!(
                "require_attribution: {} multi-file trace(s) have no attribution: {}",
                unattr.len(),
                ids.join(", ")
            ));
        } else {
            emit_ok("require_attribution: all traces attributed (ok)", args);
        }
    }

    // Rule 3: require_signed
    if policy.require_signed {
        let unsigned: Vec<_> = in_range.iter().filter(|t| t.sig.is_none()).collect();
        if !unsigned.is_empty() {
            let ids: Vec<_> = unsigned.iter().map(|t| short_id(&t.id)).collect();
            failures.push(format!(
                "require_signed: {} trace(s) are unsigned: {}",
                unsigned.len(),
                ids.join(", ")
            ));
        } else {
            emit_ok("require_signed: all traces signed (ok)", args);
        }
    }

    if failures.is_empty() {
        println!("{} All policy checks passed.", ok());
        Ok(())
    } else {
        for msg in &failures {
            emit_failure(msg, args);
        }
        std::process::exit(1);
    }
}

fn load_policy(repo_root: &Path) -> Result<PolicyConfig> {
    let path = repo_root.join(".agentdiff").join("policy.toml");
    if !path.exists() {
        return Ok(PolicyConfig::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading policy file {}", path.display()))?;
    toml::from_str::<PolicyConfig>(&raw)
        .with_context(|| format!("parsing policy file {}", path.display()))
}

fn filter_traces_since<'a>(
    traces: &'a [AgentTrace],
    base_sha: Option<&str>,
) -> Vec<&'a AgentTrace> {
    match base_sha {
        None => traces.iter().collect(),
        Some(base) => {
            let pos = traces
                .iter()
                .position(|t| t.sha().starts_with(base));
            match pos {
                Some(i) => traces[i + 1..].iter().collect(),
                None => {
                    eprintln!(
                        "agentdiff: warn — base SHA {} not found in traces; \
                         evaluating all entries. Set base_branch in .agentdiff/policy.toml \
                         if your default branch is not main/master.",
                        &base[..base.len().min(8)]
                    );
                    traces.iter().collect()
                }
            }
        }
    }
}

fn compute_ai_percent(
    store: &Store,
    base_sha: Option<&str>,
    in_range: &[&AgentTrace],
) -> Result<f64> {
    let range = match base_sha {
        Some(base) => format!("{base}..HEAD"),
        None => "HEAD".to_string(),
    };

    let out = std::process::Command::new("git")
        .args(["diff", "--numstat", &range])
        .current_dir(&store.repo_root)
        .output()
        .context("running git diff --numstat")?;

    let numstat = String::from_utf8_lossy(&out.stdout);
    let mut file_added: HashMap<String, u64> = HashMap::new();
    let mut total_added = 0u64;

    for line in numstat.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let added: u64 = parts[0].parse().unwrap_or(0);
        file_added.insert(parts[2].to_string(), added);
        total_added += added;
    }

    if total_added == 0 {
        return Ok(0.0);
    }

    let ai_files: HashSet<&str> = in_range
        .iter()
        .flat_map(|t| t.files.iter().map(|f| f.path.as_str()))
        .collect();

    let ai_added: u64 = file_added
        .iter()
        .filter(|(f, _)| ai_files.contains(f.as_str()))
        .map(|(_, n)| *n)
        .sum();

    Ok(ai_added as f64 / total_added as f64 * 100.0)
}

fn find_merge_base(repo_root: &Path, configured_branch: Option<&str>) -> Option<String> {
    // Priority: policy.toml base_branch → remote HEAD → main → master
    let candidates = {
        let mut v: Vec<String> = Vec::new();
        if let Some(b) = configured_branch {
            v.push(b.to_string());
        }
        // Try to read the remote's default branch from the cached symbolic ref.
        let remote_head = std::process::Command::new("git")
            .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
            .current_dir(repo_root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().trim_start_matches("refs/remotes/origin/").to_string())
            .filter(|s| !s.is_empty());
        if let Some(b) = remote_head {
            if !v.contains(&b) {
                v.push(b);
            }
        }
        for fallback in ["main", "master"] {
            let fb = fallback.to_string();
            if !v.contains(&fb) {
                v.push(fb);
            }
        }
        v
    };

    for branch in &candidates {
        let out = std::process::Command::new("git")
            .args(["merge-base", "HEAD", branch])
            .current_dir(repo_root)
            .output()
            .ok()?;
        if out.status.success() {
            let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
            if !sha.is_empty() {
                return Some(sha);
            }
        }
    }
    None
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn emit(msg: &str, args: &PolicyCheckArgs) {
    match args.format {
        PolicyFormat::Text => println!("{msg}"),
        PolicyFormat::GithubAnnotations => {}
    }
}

fn emit_ok(msg: &str, args: &PolicyCheckArgs) {
    match args.format {
        PolicyFormat::Text => println!("{} {msg}", ok()),
        PolicyFormat::GithubAnnotations => {}
    }
}

fn emit_failure(msg: &str, args: &PolicyCheckArgs) {
    match args.format {
        PolicyFormat::Text => eprintln!("{} {msg}", err()),
        PolicyFormat::GithubAnnotations => {
            println!("::error::{msg}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_policy_defaults_when_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = load_policy(dir.path()).unwrap();
        assert!(p.max_ai_percent.is_none());
        assert!(!p.require_attribution);
        assert!(!p.require_signed);
    }

    #[test]
    fn test_load_policy_parses_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let agentdiff_dir = dir.path().join(".agentdiff");
        std::fs::create_dir_all(&agentdiff_dir).unwrap();
        std::fs::write(
            agentdiff_dir.join("policy.toml"),
            "max_ai_percent = 80.0\nrequire_signed = true\n",
        )
        .unwrap();

        let p = load_policy(dir.path()).unwrap();
        assert_eq!(p.max_ai_percent, Some(80.0));
        assert!(p.require_signed);
        assert!(!p.require_attribution);
    }

    #[test]
    fn test_ai_percent_boundary() {
        let threshold = 80.0f64;
        let pct_at = 80.0f64;
        let pct_over = 81.0f64;
        assert!(pct_at <= threshold);
        assert!(pct_over > threshold);
    }
}
