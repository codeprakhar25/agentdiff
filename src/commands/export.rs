use anyhow::Result;

use crate::cli::{ExportArgs, ExportFormat};
use crate::store::Store;

/// Agent Trace JSONL output — near-passthrough since storage IS Agent Trace format.
pub fn run(store: &Store, args: &ExportArgs) -> Result<()> {
    match args.format {
        ExportFormat::AgentTrace => run_agent_trace(store),
    }
}

fn run_agent_trace(store: &Store) -> Result<()> {
    let traces = store.load_all_traces()?;
    for trace in &traces {
        println!("{}", serde_json::to_string(trace)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::data::*;
    use chrono::Utc;

    fn make_trace(id: &str, files: &[&str]) -> AgentTrace {
        AgentTrace {
            version: "0.1.0".to_string(),
            id: id.to_string(),
            timestamp: Utc::now(),
            vcs: Some(VcsInfo {
                vcs_type: "git".to_string(),
                revision: "abc123".to_string(),
            }),
            tool: Some(ToolInfo {
                name: "claude-code".to_string(),
                version: None,
            }),
            files: files
                .iter()
                .map(|f| TraceFile {
                    path: f.to_string(),
                    conversations: vec![Conversation {
                        url: None,
                        contributor: Contributor {
                            contributor_type: "ai".to_string(),
                            model_id: Some("anthropic/claude-sonnet-4-6".to_string()),
                        },
                        ranges: vec![TraceRange {
                            start_line: 1,
                            end_line: 10,
                            content_hash: None,
                            contributor: None,
                        }],
                        related: None,
                    }],
                })
                .collect(),
            metadata: None,
            sig: None,
        }
    }

    #[test]
    fn test_trace_serializes_to_valid_json() {
        let trace = make_trace("uuid-1", &["src/main.rs"]);
        let json = serde_json::to_string(&trace).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["version"], "0.1.0");
        assert_eq!(parsed["id"], "uuid-1");
        assert_eq!(parsed["files"][0]["path"], "src/main.rs");
    }
}
