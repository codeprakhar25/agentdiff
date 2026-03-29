use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One edit event captured by a capture script.
/// Matches the JSON schema written by capture.py / agentblame-capture.py.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub timestamp: DateTime<Utc>,
    pub agent: String,
    #[serde(default)]
    pub mode: Option<String>, // "agent" | "tab" — Cursor-specific
    pub model: String,
    pub session_id: String,
    pub tool: String,
    pub file: String, // repo-relative path
    pub abs_file: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default = "default_acceptance")]
    pub acceptance: String,
    #[serde(default)]
    pub lines: Vec<u32>,

    // Edit-specific
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,

    // Write-specific
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<u32>,

    // MultiEdit-specific
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edits: Option<Vec<EditPair>>,

    // Injected at load time — not in the raw JSON
    #[serde(skip)]
    pub committed: bool,
    #[serde(skip)]
    pub commit_hash: String,
    #[serde(skip)]
    pub batch_author: String,
}

fn default_acceptance() -> String {
    "verbatim".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPair {
    pub old: String,
    pub new: String,
}

/// A committed batch — the JSON written by the post-commit hook.
/// schema_version = "1.0"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommittedBatch {
    pub schema_version: String,
    pub commit: String,
    pub author: String,
    pub committed_at: DateTime<Utc>,
    pub entries: Vec<Entry>,
}

/// Compact commit-scoped note payload written to refs/notes/agentdiff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotesRecord {
    pub version: String,
    pub commit: String,
    pub generated_at: DateTime<Utc>,
    #[serde(default)]
    pub contributors: Vec<NotesContributor>,
    #[serde(default)]
    pub files: Vec<NotesFileAttribution>,
    #[serde(default)]
    pub trace_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotesContributor {
    pub id: String,
    pub agent: String,
    pub model: String,
    pub session_ref: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub prompt_excerpt: String,
    #[serde(default)]
    pub prompt_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotesFileAttribution {
    pub path: String,
    pub tool: String,
    pub contributor_id: String,
    /// [start, end] (inclusive) line ranges at this commit revision.
    #[serde(default)]
    pub ranges: Vec<(u32, u32)>,
}

/// Per-file attribution when multiple agents contributed to a single commit.
/// Stored in LedgerRecord.attribution for files whose agent differs from the
/// dominant agent recorded at the top level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttribution {
    pub agent: String,
    pub model: String,
    pub session_id: String,
    pub tool: String,
}

/// Canonical append-only ledger entry (one line per commit) stored at
/// <repo>/.agentdiff/ledger.jsonl
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRecord {
    pub sha: String,
    pub ts: DateTime<Utc>,
    pub agent: String,
    pub model: String,
    pub session_id: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub files_touched: Vec<String>,
    /// Per-file line ranges [start, end] (inclusive)
    #[serde(default)]
    pub lines: HashMap<String, Vec<(u32, u32)>>,
    /// Per-file attribution when multiple agents contributed.
    /// Only present when a file's agent differs from the top-level agent.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attribution: HashMap<String, FileAttribution>,
    #[serde(default)]
    pub prompt_excerpt: String,
    #[serde(default)]
    pub prompt_hash: String,
    #[serde(default)]
    pub files_read: Vec<String>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub trust: Option<u8>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}
