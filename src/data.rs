use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Agent Trace spec v0.1.0 ─────────────────────────────────────────────────

/// Top-level Agent Trace record — the native storage format.
/// Follows the open Agent Trace specification (Cognition/Cursor/Vercel/Cloudflare).
/// UUID is the PRIMARY KEY, not the git SHA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTrace {
    /// Spec version — always "0.1.0"
    pub version: String,
    /// UUID — primary key, survives squash/rebase/cherry-pick
    pub id: String,
    /// RFC 3339 timestamp
    pub timestamp: DateTime<Utc>,
    /// Version control context (git SHA is informational, not identity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<VcsInfo>,
    /// Agent/tool that produced the change
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolInfo>,
    /// Per-file attribution with conversations and line ranges
    pub files: Vec<TraceFile>,
    /// Extension point for agentdiff-specific fields (prompt, trust, flags, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// ed25519 signature (agentdiff extension)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<LedgerSig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsInfo {
    /// VCS type — "git", "jj", "hg", "svn"
    #[serde(rename = "type")]
    pub vcs_type: String,
    /// VCS-specific revision identifier (git SHA, jj change ID, etc.)
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Agent name — "claude-code", "cursor", "codex", "copilot", etc.
    pub name: String,
    /// Agent/tool version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceFile {
    /// Repo-relative file path
    pub path: String,
    /// Conversations that contributed to this file
    pub conversations: Vec<Conversation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// URL/URI to the full conversation/session context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Who made this contribution
    pub contributor: Contributor,
    /// Line ranges attributed to this conversation
    pub ranges: Vec<TraceRange>,
    /// Related resources (docs, issues, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related: Option<Vec<RelatedResource>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    /// "ai" | "human" | "mixed" | "unknown"
    #[serde(rename = "type")]
    pub contributor_type: String,
    /// Model identifier in models.dev format (e.g. "anthropic/claude-sonnet-4-6")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceRange {
    /// 1-indexed, inclusive start line
    pub start_line: u32,
    /// 1-indexed, inclusive end line
    pub end_line: u32,
    /// Position-independent content hash for tracking across rebases
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Override conversation-level contributor for this range
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contributor: Option<Contributor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedResource {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub url: String,
}

/// agentdiff-specific metadata stored in AgentTrace.metadata under "agentdiff" key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentdiffMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_read: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Capture tool (Edit, Write, MultiEdit, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_tool: Option<String>,
}

/// ed25519 signature attached to a trace entry after `agentdiff keys init`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerSig {
    /// Signing algorithm — always "ed25519"
    pub alg: String,
    /// First 16 hex chars of SHA-256(public key bytes)
    pub key_id: String,
    /// Base64-encoded 64-byte signature over JCS-canonical JSON (sig field excluded)
    pub value: String,
}

// ── Internal Entry type (display layer) ─────────────────────────────────────

/// One edit event — internal representation used by query/display commands.
/// All storage formats are converted to Entry before display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub timestamp: DateTime<Utc>,
    pub agent: String,
    #[serde(default)]
    pub mode: Option<String>,
    pub model: String,
    pub session_id: String,
    pub tool: String,
    pub file: String,
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
}

fn default_acceptance() -> String {
    "verbatim".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPair {
    pub old: String,
    pub new: String,
}

// ── AgentTrace → Entry conversion ───────────────────────────────────────────

impl AgentTrace {
    /// Convert this trace into Entry structs for display commands.
    pub fn to_entries(&self, repo_root: &Path) -> Vec<Entry> {
        let commit_hash = self
            .vcs
            .as_ref()
            .map(|v| v.revision.clone())
            .unwrap_or_default();

        let agent_name = self
            .tool
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_else(|| "unknown".to_string());

        // Extract agentdiff metadata if present
        let ad_meta: Option<AgentdiffMetadata> = self
            .metadata
            .as_ref()
            .and_then(|m| m.get("agentdiff"))
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let session_id = ad_meta
            .as_ref()
            .and_then(|m| m.session_id.clone())
            .unwrap_or_default();
        let prompt = ad_meta.as_ref().and_then(|m| m.prompt_excerpt.clone());
        let capture_tool = ad_meta
            .as_ref()
            .and_then(|m| m.capture_tool.clone())
            .unwrap_or_else(|| "commit".to_string());

        let mut entries = Vec::new();

        for file in &self.files {
            let abs_file = repo_root.join(&file.path).to_string_lossy().to_string();

            for conversation in &file.conversations {
                let model = conversation
                    .contributor
                    .model_id
                    .clone()
                    .unwrap_or_default();

                let mut lines = Vec::new();
                for range in &conversation.ranges {
                    let lo = range.start_line.min(range.end_line);
                    let hi = range.start_line.max(range.end_line);
                    for ln in lo..=hi {
                        lines.push(ln);
                    }
                }
                lines.sort_unstable();
                lines.dedup();

                entries.push(Entry {
                    timestamp: self.timestamp,
                    agent: agent_name.clone(),
                    mode: None,
                    model,
                    session_id: session_id.clone(),
                    tool: capture_tool.clone(),
                    file: file.path.clone(),
                    abs_file: abs_file.clone(),
                    prompt: prompt.clone(),
                    acceptance: "verbatim".to_string(),
                    lines,
                    old: None,
                    new: None,
                    content_preview: None,
                    total_lines: None,
                    edit_count: None,
                    edits: None,
                    committed: true,
                    commit_hash: commit_hash.clone(),
                });
            }
        }

        entries
    }

    /// Get the agentdiff-specific metadata, if present.
    pub fn agentdiff_metadata(&self) -> Option<AgentdiffMetadata> {
        self.metadata
            .as_ref()
            .and_then(|m| m.get("agentdiff"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Get the commit SHA from VCS info (convenience).
    pub fn sha(&self) -> &str {
        self.vcs
            .as_ref()
            .map(|v| v.revision.as_str())
            .unwrap_or("")
    }

    /// Get the agent name from tool info (convenience).
    pub fn agent_name(&self) -> &str {
        self.tool.as_ref().map(|t| t.name.as_str()).unwrap_or("unknown")
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Expand inclusive [start, end] ranges into individual line numbers.
#[cfg(test)]
fn expand_ranges(ranges: &[(u32, u32)]) -> Vec<u32> {
    let mut out = Vec::new();
    for &(start, end) in ranges {
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_trace() -> AgentTrace {
        AgentTrace {
            version: "0.1.0".to_string(),
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: Utc::now(),
            vcs: Some(VcsInfo {
                vcs_type: "git".to_string(),
                revision: "abc123def456".to_string(),
            }),
            tool: Some(ToolInfo {
                name: "claude-code".to_string(),
                version: None,
            }),
            files: vec![TraceFile {
                path: "src/main.rs".to_string(),
                conversations: vec![Conversation {
                    url: None,
                    contributor: Contributor {
                        contributor_type: "ai".to_string(),
                        model_id: Some("anthropic/claude-sonnet-4-6".to_string()),
                    },
                    ranges: vec![
                        TraceRange {
                            start_line: 10,
                            end_line: 20,
                            content_hash: None,
                            contributor: None,
                        },
                        TraceRange {
                            start_line: 30,
                            end_line: 35,
                            content_hash: None,
                            contributor: None,
                        },
                    ],
                    related: None,
                }],
            }],
            metadata: Some(serde_json::json!({
                "agentdiff": {
                    "prompt_excerpt": "add auth middleware",
                    "session_id": "sess-123",
                    "trust": 92,
                    "flags": ["security"],
                    "intent": "security hardening",
                    "author": "Prakhar Khatri"
                }
            })),
            sig: None,
        }
    }

    #[test]
    fn test_serialize_roundtrip() {
        let trace = sample_trace();
        let json = serde_json::to_string(&trace).unwrap();
        let parsed: AgentTrace = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "0.1.0");
        assert_eq!(parsed.id, trace.id);
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].conversations[0].ranges.len(), 2);
    }

    #[test]
    fn test_to_entries() {
        let trace = sample_trace();
        let entries = trace.to_entries(&PathBuf::from("/repo"));
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.agent, "claude-code");
        assert_eq!(e.model, "anthropic/claude-sonnet-4-6");
        assert_eq!(e.commit_hash, "abc123def456");
        assert_eq!(e.file, "src/main.rs");
        // 10..=20 (11 lines) + 30..=35 (6 lines) = 17 lines
        assert_eq!(e.lines.len(), 17);
        assert_eq!(e.lines[0], 10);
        assert_eq!(*e.lines.last().unwrap(), 35);
        assert_eq!(e.prompt.as_deref(), Some("add auth middleware"));
        assert_eq!(e.session_id, "sess-123");
    }

    #[test]
    fn test_agentdiff_metadata() {
        let trace = sample_trace();
        let meta = trace.agentdiff_metadata().unwrap();
        assert_eq!(meta.trust, Some(92));
        assert_eq!(meta.intent.as_deref(), Some("security hardening"));
        assert_eq!(meta.flags, vec!["security"]);
    }

    #[test]
    fn test_convenience_methods() {
        let trace = sample_trace();
        assert_eq!(trace.sha(), "abc123def456");
        assert_eq!(trace.agent_name(), "claude-code");
    }

    #[test]
    fn test_minimal_trace() {
        let json = r#"{
            "version": "0.1.0",
            "id": "test-uuid",
            "timestamp": "2026-01-25T10:00:00Z",
            "files": [{
                "path": "src/app.ts",
                "conversations": [{
                    "contributor": { "type": "ai" },
                    "ranges": [{ "start_line": 1, "end_line": 50 }]
                }]
            }]
        }"#;
        let trace: AgentTrace = serde_json::from_str(json).unwrap();
        assert_eq!(trace.id, "test-uuid");
        assert!(trace.vcs.is_none());
        assert!(trace.tool.is_none());
        assert!(trace.metadata.is_none());
        assert_eq!(trace.files[0].conversations[0].ranges[0].end_line, 50);
    }

    #[test]
    fn test_expand_ranges() {
        assert_eq!(expand_ranges(&[(1, 3), (5, 5)]), vec![1, 2, 3, 5]);
        assert_eq!(expand_ranges(&[(3, 1)]), vec![1, 2, 3]); // reversed
        assert_eq!(expand_ranges(&[(0, 5)]), Vec::<u32>::new()); // zero start
    }
}
