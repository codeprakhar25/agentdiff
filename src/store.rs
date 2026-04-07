use crate::config::Config;
use crate::data::{CommittedBatch, Entry, LedgerRecord, NotesRecord};
use anyhow::{Context, Result};
use glob::glob;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Store {
    pub repo_root: PathBuf,
}

impl Store {
    pub fn new(repo_root: PathBuf, _config: Config) -> Self {
        Self { repo_root }
    }

    pub fn ledger_path(&self) -> PathBuf {
        Config::repo_ledger_path(&self.repo_root)
    }

    /// Read raw JSONL content for ledger entries, trying agentdiff-meta branch
    /// first (enterprise storage) then falling back to the working-tree file.
    pub fn ledger_jsonl_content(&self) -> Result<Option<String>> {
        // Try meta branch first.
        let meta = std::process::Command::new("git")
            .args(["show", "agentdiff-meta:ledger.jsonl"])
            .current_dir(&self.repo_root)
            .output();
        if let Ok(out) = meta {
            if out.status.success() {
                return Ok(Some(String::from_utf8_lossy(&out.stdout).to_string()));
            }
        }
        // Fall back to working-tree file.
        let path = self.ledger_path();
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            return Ok(Some(raw));
        }
        Ok(None)
    }

    /// Load ALL entries from:
    ///   1) agentdiff-meta:ledger.jsonl       [meta branch — enterprise storage]
    ///   2) .agentdiff/ledger.jsonl           [working-tree fallback]
    ///   3) refs/notes/agentdiff              [legacy committed fallback]
    ///   4) .git/agentdiff/session.jsonl      [uncommitted]
    ///   5) legacy paths (read-only compat)
    pub fn load_entries(&self) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();

        let ledger_records = self.load_ledger_records()?;
        if ledger_records.is_empty() {
            self.load_notes_entries(&mut entries)?;
        } else {
            for record in &ledger_records {
                self.ledger_record_to_entries(record, &mut entries);
            }
        }

        let session_path = Config::repo_session_log(&self.repo_root);
        self.load_session_from(&session_path, &mut entries, false)?;

        // Legacy global path (read-only backward compat)
        let legacy_global = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(".agentblame")
            .join("data")
            .join(Config::slug_for(&self.repo_root));
        self.load_committed_from(&legacy_global.join("entries"), &mut entries)?;
        self.load_session_from(&legacy_global.join("session.jsonl"), &mut entries, false)?;

        // Legacy in-repo path (read-only backward compat)
        let legacy_repo = self.repo_root.join(".agentblame");
        if legacy_repo.exists() {
            self.load_committed_from(&legacy_repo.join("entries"), &mut entries)?;
            self.load_session_from(&legacy_repo.join("session.jsonl"), &mut entries, false)?;
        }

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

    pub fn load_ledger_records(&self) -> Result<Vec<LedgerRecord>> {
        let raw = match self.ledger_jsonl_content()? {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let mut by_sha: HashMap<String, LedgerRecord> = HashMap::new();
        for (idx, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let record: LedgerRecord = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!(
                        "agentdiff: skipping malformed ledger line {}: {}",
                        idx + 1,
                        err
                    );
                    continue;
                }
            };

            match by_sha.get(&record.sha) {
                Some(existing) if existing.ts > record.ts => {}
                _ => {
                    by_sha.insert(record.sha.clone(), record);
                }
            }
        }

        let mut out: Vec<LedgerRecord> = by_sha.into_values().collect();
        out.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.sha.cmp(&b.sha)));
        Ok(out)
    }

    /// Load ledger as raw JSON values in chronological order.
    /// Used by `verify` so signatures are checked against the exact bytes on disk,
    /// not a serde-roundtripped representation that may change field values (e.g. timestamps).
    pub fn load_ledger_raw(&self) -> Result<Vec<serde_json::Value>> {
        let raw = match self.ledger_jsonl_content()? {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let mut by_sha: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();
        for (idx, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!(
                        "agentdiff: skipping malformed ledger line {}: {}",
                        idx + 1,
                        err
                    );
                    continue;
                }
            };
            let sha = value
                .get("sha")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !sha.is_empty() {
                by_sha.entry(sha).or_insert(value);
            }
        }

        // Sort by "ts" field string (ISO-8601 sorts lexicographically).
        let mut out: Vec<serde_json::Value> = by_sha.into_values().collect();
        out.sort_by(|a, b| {
            let ta = a.get("ts").and_then(|v| v.as_str()).unwrap_or("");
            let tb = b.get("ts").and_then(|v| v.as_str()).unwrap_or("");
            ta.cmp(tb)
        });
        Ok(out)
    }

    pub fn find_ledger_record(&self, sha_prefix: &str) -> Result<Option<LedgerRecord>> {
        let records = self.load_ledger_records()?;
        let mut matched: Vec<LedgerRecord> = records
            .into_iter()
            .filter(|r| r.sha == sha_prefix || r.sha.starts_with(sha_prefix))
            .collect();

        if matched.is_empty() {
            return Ok(None);
        }
        matched.sort_by(|a, b| b.ts.cmp(&a.ts));
        Ok(matched.into_iter().next())
    }

    pub fn load_notes_records(&self) -> Result<Vec<NotesRecord>> {
        let list_output = std::process::Command::new("git")
            .args(["notes", "--ref=agentdiff", "list"])
            .current_dir(&self.repo_root)
            .output()
            .context("listing agentdiff notes")?;

        if !list_output.status.success() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        let list_raw = String::from_utf8_lossy(&list_output.stdout);
        for line in list_raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let _note_obj = parts.next();
            let commit = match parts.next() {
                Some(v) => v,
                None => continue,
            };

            let show_output = std::process::Command::new("git")
                .args(["notes", "--ref=agentdiff", "show", commit])
                .current_dir(&self.repo_root)
                .output()?;

            if !show_output.status.success() {
                continue;
            }

            let raw = String::from_utf8_lossy(&show_output.stdout).to_string();
            let record: NotesRecord = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("agentdiff: skipping malformed note on {commit}: {err}");
                    continue;
                }
            };
            records.push(record);
        }

        Ok(records)
    }

    fn load_notes_entries(&self, out: &mut Vec<Entry>) -> Result<()> {
        for record in self.load_notes_records()? {
            self.notes_record_to_entries(&record, out);
        }
        Ok(())
    }

    fn ledger_record_to_entries(&self, record: &LedgerRecord, out: &mut Vec<Entry>) {
        let tool = record.tool.clone().unwrap_or_else(|| "commit".to_string());
        for file in &record.files_touched {
            let abs = self.repo_root.join(file).to_string_lossy().to_string();
            let lines = expand_ranges(record.lines.get(file).cloned().unwrap_or_default());

            // Use per-file attribution if available, otherwise fall back to record-level.
            let file_attr = record.attribution.get(file);
            let agent = file_attr.map(|a| a.agent.clone()).unwrap_or_else(|| record.agent.clone());
            let model = file_attr.map(|a| a.model.clone()).unwrap_or_else(|| record.model.clone());
            let session_id = file_attr.map(|a| a.session_id.clone()).unwrap_or_else(|| record.session_id.clone());
            let file_tool = file_attr.map(|a| a.tool.clone()).unwrap_or_else(|| tool.clone());

            out.push(Entry {
                timestamp: record.ts,
                agent,
                mode: record.mode.clone(),
                model,
                session_id,
                tool: file_tool,
                file: file.clone(),
                abs_file: abs,
                prompt: if record.prompt_excerpt.is_empty() {
                    None
                } else {
                    Some(record.prompt_excerpt.clone())
                },
                acceptance: "verbatim".to_string(),
                lines,
                old: None,
                new: None,
                content_preview: None,
                total_lines: None,
                edit_count: None,
                edits: None,
                committed: true,
                commit_hash: record.sha.clone(),
                batch_author: record.author.clone().unwrap_or_default(),
            });
        }
    }

    fn notes_record_to_entries(&self, record: &NotesRecord, out: &mut Vec<Entry>) {
        let mut contributors: HashMap<&str, &crate::data::NotesContributor> = HashMap::new();
        for c in &record.contributors {
            contributors.insert(c.id.as_str(), c);
        }

        for f in &record.files {
            let Some(c) = contributors.get(f.contributor_id.as_str()) else {
                continue;
            };

            let mut lines = Vec::new();
            for (start, end) in &f.ranges {
                let s = *start;
                let e = *end;
                if s == 0 || e == 0 {
                    continue;
                }
                let lo = s.min(e);
                let hi = s.max(e);
                for ln in lo..=hi {
                    lines.push(ln);
                }
            }

            let abs = self.repo_root.join(&f.path).to_string_lossy().to_string();

            out.push(Entry {
                timestamp: record.generated_at,
                agent: c.agent.clone(),
                mode: None,
                model: c.model.clone(),
                session_id: c.session_ref.clone(),
                tool: f.tool.clone(),
                file: f.path.clone(),
                abs_file: abs,
                prompt: if c.prompt_excerpt.is_empty() {
                    None
                } else {
                    Some(c.prompt_excerpt.clone())
                },
                acceptance: "verbatim".to_string(),
                lines,
                old: None,
                new: None,
                content_preview: None,
                total_lines: None,
                edit_count: None,
                edits: None,
                committed: true,
                commit_hash: record.commit.clone(),
                batch_author: String::new(),
            });
        }
    }

    fn load_committed_from(&self, entries_dir: &Path, out: &mut Vec<Entry>) -> Result<()> {
        if !entries_dir.exists() {
            return Ok(());
        }
        let pattern = entries_dir.join("*.json").to_string_lossy().to_string();
        for path in glob(&pattern)?.flatten() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let batch: CommittedBatch = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", path.display()))?;
            for mut e in batch.entries {
                e.committed = true;
                e.commit_hash = batch.commit.clone();
                e.batch_author = batch.author.clone();
                out.push(e);
            }
        }
        Ok(())
    }

    fn load_session_from(&self, path: &Path, out: &mut Vec<Entry>, committed: bool) -> Result<()> {
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

    /// Get only uncommitted entries
    pub fn load_uncommitted(&self) -> Result<Vec<Entry>> {
        let all = self.load_entries()?;
        Ok(all.into_iter().filter(|e| !e.committed).collect())
    }
}

fn expand_ranges(ranges: Vec<(u32, u32)>) -> Vec<u32> {
    let mut out = Vec::new();
    for (start, end) in ranges {
        if start == 0 || end == 0 {
            continue;
        }
        let lo = start.min(end);
        let hi = start.max(end);
        out.extend(lo..=hi);
    }
    out.sort_unstable();
    out.dedup();
    out
}
