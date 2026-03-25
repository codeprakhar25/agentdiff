use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";
const TOOL_NAME: &str = "record_context";

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct RecordContextArgs {
    prompt: Option<String>,
    files_read: Option<Vec<String>>,
    model_id: Option<String>,
    session_id: Option<String>,
    agent: Option<String>,
    cwd: Option<String>,
    intent: Option<String>,
    trust: Option<i32>,
    flags: Option<Vec<String>>,
}

fn main() -> Result<()> {
    let default_cwd = std::env::current_dir().context("resolving current directory")?;

    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    while let Some(message) = read_message(&mut reader)? {
        let request: Value = match serde_json::from_str(&message) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(response) = handle_request(&request, &default_cwd) {
            write_message(&mut writer, &response)?;
        }
    }

    Ok(())
}

fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            let parsed = value
                .trim()
                .parse::<usize>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length"))?;
            content_length = Some(parsed);
        }
    }

    let len = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let message = String::from_utf8(body)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid utf-8 body"))?;
    Ok(Some(message))
}

fn write_message<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let payload = value.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", payload.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(payload.as_bytes())?;
    writer.flush()
}

fn handle_request(request: &Value, default_cwd: &Path) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str)?;

    match method {
        "initialize" => {
            let protocol = request
                .get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL_VERSION);

            id.map(|rid| {
                response_ok(
                    rid,
                    json!({
                        "protocolVersion": protocol,
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "agentdiff-mcp",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }),
                )
            })
        }
        "notifications/initialized" => None,
        "ping" => id.map(|rid| response_ok(rid, json!({}))),
        "tools/list" => id.map(|rid| response_ok(rid, json!({"tools":[tool_definition()]}))),
        "tools/call" => id.map(|rid| {
            let name = request
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");

            if name != TOOL_NAME {
                return response_error(rid, -32601, format!("unknown tool: {name}"));
            }

            let raw_args = request
                .get("params")
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or_else(|| json!({}));

            let args: RecordContextArgs = match serde_json::from_value(raw_args) {
                Ok(v) => v,
                Err(err) => {
                    return response_error(rid, -32602, format!("invalid arguments: {err}"));
                }
            };

            match record_context(&args, default_cwd) {
                Ok(out_path) => response_ok(
                    rid,
                    json!({
                        "content": [{
                            "type":"text",
                            "text": format!("recorded context in {}", out_path.display())
                        }],
                        "structuredContent": {
                            "status":"recorded",
                            "path": out_path.display().to_string(),
                            "will_attach_on_next_commit": true
                        }
                    }),
                ),
                Err(err) => response_error(rid, -32000, format!("{err:#}")),
            }
        }),
        _ => id.map(|rid| response_error(rid, -32601, format!("method not found: {method}"))),
    }
}

fn tool_definition() -> Value {
    json!({
      "name": TOOL_NAME,
      "description": "Record agent session context into .git/agentdiff/pending.json for the next commit.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "prompt": {"type":"string"},
          "files_read": {"type":"array","items":{"type":"string"}},
          "model_id": {"type":"string"},
          "session_id": {"type":"string"},
          "agent": {"type":"string"},
          "cwd": {"type":"string"},
          "intent": {"type":"string"},
          "trust": {"type":"integer","minimum":0,"maximum":100},
          "flags": {"type":"array","items":{"type":"string"}}
        },
        "required": ["prompt","model_id"],
        "additionalProperties": false
      }
    })
}

fn response_ok(id: Value, result: Value) -> Value {
    json!({
      "jsonrpc":"2.0",
      "id": id,
      "result": result
    })
}

fn response_error(id: Value, code: i32, message: String) -> Value {
    json!({
      "jsonrpc":"2.0",
      "id": id,
      "error": {
        "code": code,
        "message": message
      }
    })
}

fn record_context(args: &RecordContextArgs, default_cwd: &Path) -> Result<PathBuf> {
    let cwd = args
        .cwd
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_cwd.to_path_buf());
    let repo_root = find_repo_root(&cwd)?;

    let pending_path = repo_root.join(".git").join("agentdiff").join("pending.json");
    if let Some(parent) = pending_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let trust = args.trust.map(|v| v.clamp(0, 100));
    let payload = json!({
      "recorded_at": Utc::now().to_rfc3339(),
      "agent": args.agent.clone().unwrap_or_else(|| "unknown".to_string()),
      "model_id": args.model_id.clone().unwrap_or_else(|| "unknown".to_string()),
      "session_id": args.session_id.clone().unwrap_or_else(|| "unknown".to_string()),
      "prompt": args.prompt.clone().unwrap_or_default(),
      "files_read": args.files_read.clone().unwrap_or_default(),
      "intent": args.intent.clone().unwrap_or_default(),
      "flags": args.flags.clone().unwrap_or_default(),
      "trust": trust
    });

    let tmp_path = pending_path.with_extension("json.tmp");
    fs::write(&tmp_path, payload.to_string())?;
    fs::rename(&tmp_path, &pending_path)?;
    Ok(pending_path)
}

fn find_repo_root(cwd: &Path) -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["-C", &cwd.display().to_string(), "rev-parse", "--show-toplevel"])
        .output()
        .with_context(|| format!("running git rev-parse in {}", cwd.display()))?;

    if !out.status.success() {
        bail!("not a git repository: {}", cwd.display());
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if root.is_empty() {
        bail!("unable to determine git repo root for {}", cwd.display());
    }
    Ok(PathBuf::from(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn init_repo() -> PathBuf {
        let dir = tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();
        // Keep tempdir alive by leaking for test scope.
        std::mem::forget(dir);
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .expect("git init");
        assert!(status.success());
        repo
    }

    #[test]
    fn initialize_response_contains_tools_capability() {
        let req = json!({
          "jsonrpc":"2.0",
          "id":1,
          "method":"initialize",
          "params":{"protocolVersion":"2024-11-05"}
        });
        let resp = handle_request(&req, Path::new(".")).expect("response");
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_includes_record_context() {
        let req = json!({
          "jsonrpc":"2.0",
          "id":"abc",
          "method":"tools/list",
          "params":{}
        });
        let resp = handle_request(&req, Path::new(".")).expect("response");
        let tools = resp["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], TOOL_NAME);
    }

    #[test]
    fn tools_call_writes_pending_context() {
        let repo = init_repo();
        let req = json!({
          "jsonrpc":"2.0",
          "id":99,
          "method":"tools/call",
          "params":{
            "name":"record_context",
            "arguments":{
              "cwd": repo.display().to_string(),
              "prompt":"add auth middleware",
              "files_read":["src/auth.rs"],
              "model_id":"gpt-5.4",
              "session_id":"sess_test",
              "agent":"codex",
              "intent":"auth hardening",
              "trust":88,
              "flags":["security"]
            }
          }
        });

        let resp = handle_request(&req, Path::new(".")).expect("response");
        assert!(resp.get("result").is_some());
        let pending_path = repo.join(".git").join("agentdiff").join("pending.json");
        assert!(pending_path.exists());

        let raw = fs::read_to_string(&pending_path).expect("read pending");
        let obj: Value = serde_json::from_str(&raw).expect("json");
        assert_eq!(obj["agent"], "codex");
        assert_eq!(obj["model_id"], "gpt-5.4");
        assert_eq!(obj["session_id"], "sess_test");
        assert_eq!(obj["trust"], 88);
    }

    #[test]
    fn unknown_tool_returns_error() {
        let req = json!({
          "jsonrpc":"2.0",
          "id":7,
          "method":"tools/call",
          "params":{"name":"unknown","arguments":{}}
        });
        let resp = handle_request(&req, Path::new(".")).expect("response");
        assert!(resp.get("error").is_some());
        assert_eq!(resp["error"]["code"], -32601);
    }
}
