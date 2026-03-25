use crate::config::Config;
use crate::data::{CommittedBatch, Entry, NotesRecord};
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

    /// Load ALL entries from:
    ///   1) git notes (refs/notes/agentdiff) [canonical committed]
    ///   2) .git/agentdiff/session.jsonl      [uncommitted]
    ///   3) legacy paths (read-only compat)
    pub fn load_entries(&self) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();

        self.load_notes_entries(&mut entries)?;

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

    fn load_notes_entries(&self, out: &mut Vec<Entry>) -> Result<()> {
        let list_output = std::process::Command::new("git")
            .args(["notes", "--ref=agentdiff", "list"])
            .current_dir(&self.repo_root)
            .output()
            .context("listing agentdiff notes")?;

        if !list_output.status.success() {
            return Ok(());
        }

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

            self.notes_record_to_entries(&record, out);
        }

        Ok(())
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
