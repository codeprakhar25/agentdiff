use crate::config::Config;
use crate::data::{AgentTrace, Entry};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Store {
    pub repo_root: PathBuf,
}

impl Store {
    pub fn new(repo_root: PathBuf, _config: Config) -> Self {
        Self { repo_root }
    }

    /// Returns true if `agentdiff init` has been run in this repo.
    /// The presence of .git/agentdiff/ is the canonical signal.
    pub fn is_initialized(&self) -> bool {
        self.repo_root.join(".git").join("agentdiff").is_dir()
    }

    // ── Trace loading (new Agent Trace storage) ─────────────────────────

    /// Load traces from refs/agentdiff/meta:traces.jsonl (permanent store).
    pub fn load_meta_traces(&self) -> Result<Vec<AgentTrace>> {
        match read_ref_file(&self.repo_root, "refs/agentdiff/meta", "traces.jsonl")? {
            Some(raw) => parse_traces_jsonl(&raw),
            None => Ok(Vec::new()),
        }
    }

    /// Load traces from refs/agentdiff/traces/{branch} (per-branch ref).
    pub fn load_branch_traces(&self, branch: &str) -> Result<Vec<AgentTrace>> {
        let ref_name = branch_ref_name(branch);
        let content = read_ref_blob(&self.repo_root, &ref_name)?;
        match content {
            Some(raw) => parse_traces_jsonl(&raw),
            None => Ok(Vec::new()),
        }
    }

    /// Load local unpushed traces from .git/agentdiff/traces/{branch}.jsonl.
    pub fn load_local_traces(&self, branch: &str) -> Result<Vec<AgentTrace>> {
        let path = self.local_traces_path(branch);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        parse_traces_jsonl(&raw)
    }

    /// Load ALL traces (meta + branch ref + local), deduped by UUID.
    pub fn load_all_traces(&self) -> Result<Vec<AgentTrace>> {
        let mut traces = self.load_meta_traces()?;

        if let Ok(branch) = self.current_branch() {
            traces.extend(self.load_branch_traces(&branch)?);
            traces.extend(self.load_local_traces(&branch)?);
        }

        dedup_traces(&mut traces);
        Ok(traces)
    }

    /// Load all traces as raw JSON values (for signature verification).
    pub fn load_all_traces_raw(&self) -> Result<Vec<serde_json::Value>> {
        let mut values = Vec::new();

        // Meta ref
        if let Some(raw) = read_ref_file(&self.repo_root, "refs/agentdiff/meta", "traces.jsonl")? {
            parse_raw_jsonl(&raw, &mut values);
        }

        // Branch ref
        if let Ok(branch) = self.current_branch() {
            let ref_name = branch_ref_name(&branch);
            if let Some(raw) = read_ref_blob(&self.repo_root, &ref_name)? {
                parse_raw_jsonl(&raw, &mut values);
            }

            // Local buffer
            let local_path = self.local_traces_path(&branch);
            if local_path.exists() {
                if let Ok(raw) = std::fs::read_to_string(&local_path) {
                    parse_raw_jsonl(&raw, &mut values);
                }
            }
        }

        // Dedup by id field
        let mut seen = std::collections::HashSet::new();
        values.retain(|v| {
            let id = v.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if id.is_empty() {
                true
            } else {
                seen.insert(id)
            }
        });

        // Sort by timestamp
        values.sort_by(|a, b| {
            let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            ta.cmp(tb)
        });

        Ok(values)
    }

    /// Load uncommitted session entries (not yet finalized into AgentTrace records).
    pub fn load_uncommitted_entries(&self) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        let session_path = Config::repo_session_log(&self.repo_root);
        load_session_from(&session_path, &mut entries, false)?;
        Ok(entries)
    }

    /// Load all traces and convert to Entry for display commands.
    pub fn load_entries(&self) -> Result<Vec<Entry>> {
        let traces = self.load_all_traces()?;
        let mut entries: Vec<Entry> = traces
            .iter()
            .flat_map(|t| t.to_entries(&self.repo_root))
            .collect();

        entries.sort_by(|a, b| {
            a.timestamp
                .cmp(&b.timestamp)
                .then_with(|| a.commit_hash.cmp(&b.commit_hash))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.tool.cmp(&b.tool))
        });

        entries.dedup_by(|a, b| {
            a.timestamp == b.timestamp
                && a.agent == b.agent
                && a.model == b.model
                && a.file == b.file
                && a.tool == b.tool
                && a.commit_hash == b.commit_hash
                && a.lines == b.lines
        });

        Ok(entries)
    }

    /// Find a trace by UUID prefix.
    pub fn find_trace(&self, id_prefix: &str) -> Result<Option<AgentTrace>> {
        let traces = self.load_all_traces()?;
        Ok(traces
            .into_iter()
            .find(|t| t.id == id_prefix || t.id.starts_with(id_prefix)))
    }

    /// Find traces for a specific commit SHA (searches vcs.revision).
    pub fn find_traces_by_sha(&self, sha_prefix: &str) -> Result<Vec<AgentTrace>> {
        let traces = self.load_all_traces()?;
        Ok(traces
            .into_iter()
            .filter(|t| {
                t.vcs
                    .as_ref()
                    .map(|v| v.revision == sha_prefix || v.revision.starts_with(sha_prefix))
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Get current git branch name.
    pub fn current_branch(&self) -> Result<String> {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&self.repo_root)
            .output()
            .context("running git rev-parse")?;
        if !out.status.success() {
            anyhow::bail!("failed to determine current branch");
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Path to local traces buffer for a branch.
    pub fn local_traces_path(&self, branch: &str) -> PathBuf {
        Config::repo_session_dir(&self.repo_root)
            .join("traces")
            .join(format!("{}.jsonl", sanitize_branch_name(branch)))
    }
}

// ── Git plumbing helpers ────────────────────────────────────────────────────

/// Read a file from a branch via `git show {branch}:{file}`.
fn read_ref_file(repo_root: &Path, branch: &str, file: &str) -> Result<Option<String>> {
    let spec = format!("{branch}:{file}");
    let out = std::process::Command::new("git")
        .args(["show", &spec])
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("git show {spec}"))?;
    if out.status.success() {
        Ok(Some(String::from_utf8_lossy(&out.stdout).to_string()))
    } else {
        Ok(None)
    }
}

/// Read a blob directly from a ref (the ref itself points to a blob, not a tree).
fn read_ref_blob(repo_root: &Path, ref_name: &str) -> Result<Option<String>> {
    // First try: ref points to a commit with traces.jsonl in its tree
    let spec = format!("{ref_name}:traces.jsonl");
    let out = std::process::Command::new("git")
        .args(["show", &spec])
        .current_dir(repo_root)
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            return Ok(Some(String::from_utf8_lossy(&out.stdout).to_string()));
        }
    }

    // Second try: ref points directly to a blob
    let out = std::process::Command::new("git")
        .args(["cat-file", "-p", ref_name])
        .current_dir(repo_root)
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let content = String::from_utf8_lossy(&out.stdout).to_string();
            // Sanity check: should look like JSONL
            if content.trim_start().starts_with('{') {
                return Ok(Some(content));
            }
        }
    }

    Ok(None)
}

/// Write content as a file on a git ref using plumbing only.
pub fn write_to_ref(
    repo_root: &Path,
    ref_name: &str,
    filename: &str,
    content: &str,
    message: &str,
) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Write content to a temp file for hash-object.
    let mut tmp = tempfile::NamedTempFile::new().context("creating temp file")?;
    tmp.write_all(content.as_bytes())
        .context("writing content to temp file")?;
    tmp.flush()?;
    let tmp_path = tmp.path().to_string_lossy().to_string();

    // git hash-object -w → blob SHA
    let blob = Command::new("git")
        .args(["hash-object", "-w", &tmp_path])
        .current_dir(repo_root)
        .output()
        .context("git hash-object")?;
    anyhow::ensure!(
        blob.status.success(),
        "git hash-object failed: {}",
        String::from_utf8_lossy(&blob.stderr)
    );
    let blob_sha = String::from_utf8_lossy(&blob.stdout).trim().to_string();

    // git mktree → tree SHA
    let tree_input = format!("100644 blob {}\t{}\n", blob_sha, filename);
    let mut mktree = Command::new("git")
        .arg("mktree")
        .current_dir(repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("git mktree spawn")?;
    mktree
        .stdin
        .as_mut()
        .unwrap()
        .write_all(tree_input.as_bytes())?;
    let tree_out = mktree.wait_with_output().context("git mktree wait")?;
    anyhow::ensure!(
        tree_out.status.success(),
        "git mktree failed: {}",
        String::from_utf8_lossy(&tree_out.stderr)
    );
    let tree_sha = String::from_utf8_lossy(&tree_out.stdout)
        .trim()
        .to_string();

    // Find parent commit if ref already exists.
    let parent = Command::new("git")
        .args(["rev-parse", ref_name])
        .current_dir(repo_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let mut commit_args = vec![
        "commit-tree".to_string(),
        tree_sha,
        "-m".to_string(),
        message.to_string(),
    ];
    if let Some(p) = parent {
        commit_args.push("-p".to_string());
        commit_args.push(p);
    }

    let commit = Command::new("git")
        .args(&commit_args)
        .current_dir(repo_root)
        .output()
        .context("git commit-tree")?;
    anyhow::ensure!(
        commit.status.success(),
        "git commit-tree failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let commit_sha = String::from_utf8_lossy(&commit.stdout)
        .trim()
        .to_string();

    // Update the ref.
    let update = Command::new("git")
        .args(["update-ref", ref_name, &commit_sha])
        .current_dir(repo_root)
        .status()
        .context("git update-ref")?;
    anyhow::ensure!(update.success(), "git update-ref failed");

    Ok(())
}

/// Delete a local git ref.
pub fn delete_ref(repo_root: &Path, ref_name: &str) -> Result<()> {
    let status = std::process::Command::new("git")
        .args(["update-ref", "-d", ref_name])
        .current_dir(repo_root)
        .status()
        .context("git update-ref -d")?;
    if !status.success() {
        anyhow::bail!("failed to delete ref {ref_name}");
    }
    Ok(())
}

/// Returns the GitHub host to use for API calls.
/// Reads GH_HOST env var (same var the gh CLI respects) with github.com as default.
fn github_host() -> String {
    std::env::var("GH_HOST").unwrap_or_else(|_| "github.com".to_string())
}

/// Parse "owner/repo" from a GitHub (or GitHub Enterprise) remote URL.
/// Handles https://{host}/owner/repo.git and git@{host}:owner/repo.git
fn parse_github_nwo(remote_url: &str, host: &str) -> Option<(String, String)> {
    let url = remote_url.trim().trim_end_matches(".git");
    // HTTPS: https://{host}/owner/repo
    let https_prefix = format!("https://{host}/");
    if let Some(rest) = url.strip_prefix(https_prefix.as_str()) {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    // SSH: git@{host}:owner/repo
    let ssh_prefix = format!("git@{host}:");
    if let Some(rest) = url.strip_prefix(ssh_prefix.as_str()) {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}

fn get_github_nwo(repo_root: &Path) -> Result<(String, String)> {
    let out = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
        .context("git remote get-url origin")?;
    let remote_url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let host = github_host();
    parse_github_nwo(&remote_url, &host)
        .ok_or_else(|| anyhow::anyhow!("origin remote is not a GitHub URL (host={host}): {remote_url}"))
}

/// Push JSONL content to a GitHub ref using the Git Database API.
///
/// Creates all git objects server-side (blob → tree → commit → ref),
/// so no local git objects need to exist on the remote. This avoids the
/// HTTPS hang that occurs when pushing non-standard ref namespaces via
/// `git push`.
///
/// Uses compare-and-swap semantics (no force) with exponential-backoff retry
/// to handle concurrent pushes safely. Concurrent writers each create their
/// own commit; only one succeeds per round; losers retry with the new parent.
pub fn push_content_to_ref(
    repo_root: &Path,
    ref_name: &str,
    filename: &str,
    content: &str,
    message: &str,
) -> Result<()> {
    use base64::Engine;
    use std::io::Write;
    use std::process::{Command, Stdio};

    let (owner, repo) = get_github_nwo(repo_root)?;
    // API ref format strips "refs/" prefix
    let api_ref = ref_name.strip_prefix("refs/").unwrap_or(ref_name);

    fn gh_api_json(
        owner: &str,
        repo: &str,
        method: &str,
        path: &str,
        body: &serde_json::Value,
        cwd: &Path,
    ) -> Result<serde_json::Value> {
        use std::process::{Command, Stdio};
        let body_str = serde_json::to_string(body)?;
        let full_path = format!("/repos/{owner}/{repo}/{path}");
        let mut child = Command::new("gh")
            .args(["api", "--method", method, &full_path, "--input", "-"])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("gh api spawn")?;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(body_str.as_bytes())?;
        let out = child.wait_with_output().context("gh api wait")?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("gh api {method} {full_path} failed: {err}");
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)
            .context("parsing gh api response")?;
        Ok(v)
    }

    /// Fetch current ref tip SHA (None if ref doesn't exist yet).
    fn fetch_ref_sha(owner: &str, repo: &str, api_ref: &str, cwd: &Path) -> Option<String> {
        let out = Command::new("gh")
            .args(["api", &format!("/repos/{owner}/{repo}/git/ref/{api_ref}")])
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        v["object"]["sha"].as_str().map(String::from)
    }

    // Create the blob once — it's content-addressed, so this is idempotent.
    let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let blob_resp = gh_api_json(
        &owner, &repo, "POST", "git/blobs",
        &serde_json::json!({"content": encoded, "encoding": "base64"}),
        repo_root,
    )?;
    let blob_sha = blob_resp["sha"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no sha in blob response"))?
        .to_string();

    // CAS retry loop: fetch parent → build tree+commit → update ref (no force).
    // On non-fast-forward (422), re-fetch parent and retry with exponential backoff.
    // 10 retries covers ~100 concurrent pushes at sprint-end merge bursts.
    // Backoff: 200ms, 400ms, 800ms, 1.6s, 3.2s … capped at 5s per attempt.
    const MAX_RETRIES: u32 = 10;
    for attempt in 0..MAX_RETRIES {
        let parent_sha = fetch_ref_sha(&owner, &repo, api_ref, repo_root);

        // Build tree
        let tree_body = serde_json::json!({
            "tree": [{"path": filename, "mode": "100644", "type": "blob", "sha": blob_sha}]
        });
        let tree_resp = gh_api_json(&owner, &repo, "POST", "git/trees", &tree_body, repo_root)?;
        let tree_sha = tree_resp["sha"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no sha in tree response"))?
            .to_string();

        // Build commit
        let mut commit_body = serde_json::json!({"message": message, "tree": tree_sha});
        if let Some(ref p) = parent_sha {
            commit_body["parents"] = serde_json::json!([p]);
        }
        let commit_resp =
            gh_api_json(&owner, &repo, "POST", "git/commits", &commit_body, repo_root)?;
        let commit_sha = commit_resp["sha"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no sha in commit response"))?
            .to_string();

        // Update or create the ref (no force — CAS semantics).
        let ref_result = if parent_sha.is_some() {
            gh_api_json(
                &owner, &repo, "PATCH",
                &format!("git/refs/{api_ref}"),
                &serde_json::json!({"sha": commit_sha}),
                repo_root,
            )
        } else {
            gh_api_json(
                &owner, &repo, "POST",
                "git/refs",
                &serde_json::json!({"ref": ref_name, "sha": commit_sha}),
                repo_root,
            )
        };

        match ref_result {
            Ok(_) => return Ok(()),
            Err(e) => {
                let msg = e.to_string();
                let is_conflict = msg.contains("422") || msg.contains("not a fast forward")
                    || msg.contains("non-fast-forward");
                if is_conflict && attempt < MAX_RETRIES - 1 {
                    // Exponential backoff capped at 5s: 200ms, 400ms, 800ms … 5000ms
                    let delay_ms = (200u64 * (1 << attempt)).min(5_000);
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    continue;
                }
                return Err(e);
            }
        }
    }

    anyhow::bail!("push to {ref_name} failed after {MAX_RETRIES} retries (concurrent writers)");
}

/// Fetch JSONL content from a GitHub ref via the Git Database API.
/// Used by consolidate/status to read remote traces without needing
/// a prior `git fetch`.
pub fn fetch_ref_content_via_api(
    repo_root: &Path,
    ref_name: &str,
    filename: &str,
) -> Result<Option<String>> {
    use std::process::{Command, Stdio};

    let (owner, repo) = get_github_nwo(repo_root)?;
    let api_ref = ref_name.strip_prefix("refs/").unwrap_or(ref_name);

    // Get ref → commit SHA
    let ref_out = Command::new("gh")
        .args(["api", &format!("/repos/{owner}/{repo}/git/ref/{api_ref}")])
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;
    if !ref_out.status.success() {
        return Ok(None); // ref doesn't exist
    }
    let ref_json: serde_json::Value = serde_json::from_slice(&ref_out.stdout)
        .unwrap_or(serde_json::Value::Null);
    let commit_sha = match ref_json["object"]["sha"].as_str() {
        Some(s) => s.to_string(),
        None => return Ok(None),
    };

    // Get commit → tree SHA
    let commit_out = Command::new("gh")
        .args(["api", &format!("/repos/{owner}/{repo}/git/commits/{commit_sha}")])
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;
    if !commit_out.status.success() {
        return Ok(None);
    }
    let commit_json: serde_json::Value = serde_json::from_slice(&commit_out.stdout)
        .unwrap_or(serde_json::Value::Null);
    let tree_sha = match commit_json["tree"]["sha"].as_str() {
        Some(s) => s.to_string(),
        None => return Ok(None),
    };

    // Get tree → blob SHA for the file
    let tree_out = Command::new("gh")
        .args(["api", &format!("/repos/{owner}/{repo}/git/trees/{tree_sha}")])
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;
    if !tree_out.status.success() {
        return Ok(None);
    }
    let tree_json: serde_json::Value = serde_json::from_slice(&tree_out.stdout)
        .unwrap_or(serde_json::Value::Null);
    let blob_sha = tree_json["tree"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|e| e["path"].as_str() == Some(filename))
                .and_then(|e| e["sha"].as_str())
                .map(String::from)
        });
    let blob_sha = match blob_sha {
        Some(s) => s,
        None => return Ok(None),
    };

    // Get blob content (base64 decoded)
    let blob_out = Command::new("gh")
        .args(["api", &format!("/repos/{owner}/{repo}/git/blobs/{blob_sha}")])
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;
    if !blob_out.status.success() {
        return Ok(None);
    }
    let blob_json: serde_json::Value = serde_json::from_slice(&blob_out.stdout)
        .unwrap_or(serde_json::Value::Null);
    let encoded = match blob_json["content"].as_str() {
        Some(s) => s.replace('\n', ""), // GitHub wraps at 60 chars
        None => return Ok(None),
    };
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .context("base64 decode blob")?;
    Ok(Some(String::from_utf8_lossy(&decoded).to_string()))
}

/// Delete a remote ref via the GitHub REST API.
pub fn delete_remote_ref(repo_root: &Path, ref_name: &str) -> Result<()> {
    use std::process::{Command, Stdio};

    let (owner, repo) = get_github_nwo(repo_root)?;
    let api_ref = ref_name.strip_prefix("refs/").unwrap_or(ref_name);
    let api_path = format!("/repos/{owner}/{repo}/git/refs/{api_ref}");

    let out = Command::new("gh")
        .args(["api", "--method", "DELETE", &api_path])
        .current_dir(repo_root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("gh api DELETE")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("failed to delete remote ref {ref_name}: {stderr}");
    }
    Ok(())
}

// ── Ref naming ──────────────────────────────────────────────────────────────

/// Convert a branch name to a per-branch ref path.
/// e.g. "feature/auth" → "refs/agentdiff/traces/feature--auth"
pub fn branch_ref_name(branch: &str) -> String {
    format!(
        "refs/agentdiff/traces/{}",
        sanitize_branch_name(branch)
    )
}

/// Sanitize branch name for use in ref paths and local file names.
/// Uses percent-encoding for `/` so `feature/foo` and `feature--foo` never collide.
pub fn sanitize_branch_name(branch: &str) -> String {
    branch.replace('/', "%2F")
}

// ── JSONL parsing ───────────────────────────────────────────────────────────

/// Parse JSONL into AgentTrace records, skipping malformed lines with a warning.
/// Public so commands can parse raw content fetched outside the Store.
pub fn parse_traces_from_jsonl(raw: &str) -> Vec<AgentTrace> {
    let mut traces = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<AgentTrace>(line) {
            Ok(t) => traces.push(t),
            Err(err) => {
                eprintln!(
                    "agentdiff: skipping malformed trace line {}: {}",
                    idx + 1,
                    err
                );
            }
        }
    }
    traces
}

/// Private wrapper returning Result<Vec> for use inside Store methods.
fn parse_traces_jsonl(raw: &str) -> Result<Vec<AgentTrace>> {
    Ok(parse_traces_from_jsonl(raw))
}

fn parse_raw_jsonl(raw: &str, out: &mut Vec<serde_json::Value>) {
    for (idx, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => out.push(v),
            Err(err) => {
                eprintln!(
                    "agentdiff: skipping malformed trace line {}: {}",
                    idx + 1,
                    err
                );
            }
        }
    }
}

/// Dedup traces by UUID (keep newest by timestamp).
fn dedup_traces(traces: &mut Vec<AgentTrace>) {
    let mut by_id: HashMap<String, AgentTrace> = HashMap::new();
    for trace in traces.drain(..) {
        match by_id.get(&trace.id) {
            Some(existing) if existing.timestamp > trace.timestamp => {}
            _ => {
                by_id.insert(trace.id.clone(), trace);
            }
        }
    }
    *traces = by_id.into_values().collect();
    traces.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
}

/// Serialize traces to JSONL string.
pub fn traces_to_jsonl(traces: &[AgentTrace]) -> Result<String> {
    let mut out = String::new();
    for trace in traces {
        out.push_str(&serde_json::to_string(trace)?);
        out.push('\n');
    }
    Ok(out)
}

fn load_session_from(path: &Path, out: &mut Vec<Entry>, committed: bool) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(path)?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Entry>(line) {
            Ok(mut e) => {
                e.committed = committed;
                e.commit_hash = if committed {
                    String::new()
                } else {
                    "(uncommitted)".into()
                };
                out.push(e);
            }
            Err(err) => {
                eprintln!("agentdiff: skipping malformed entry: {err}");
            }
        }
    }
    Ok(())
}
