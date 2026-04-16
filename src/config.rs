use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Schema version for future migrations
    #[serde(default = "default_schema")]
    pub schema_version: String,

    /// Global scripts dir — defaults to ~/.agentdiff/scripts
    #[serde(default)]
    pub scripts_dir: Option<PathBuf>,

    /// Repos this instance manages (repo-root → slug mapping)
    #[serde(default)]
    pub repos: Vec<RepoConfig>,

    /// Agents to include/exclude from stats
    #[serde(default)]
    pub agent_aliases: std::collections::HashMap<String, String>,

    /// Include .agentdiff/ledger.jsonl in the same commit via post-commit amend.
    #[serde(default = "default_auto_amend_ledger")]
    pub auto_amend_ledger: bool,

    /// When false, prompt excerpts are omitted from trace entries.
    /// Set to false in environments with sensitive prompt content.
    #[serde(default = "default_capture_prompts")]
    pub capture_prompts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub path: PathBuf,
    pub slug: String,
}

fn default_schema() -> String {
    "1.0".to_string()
}

fn default_auto_amend_ledger() -> bool {
    false
}

fn default_capture_prompts() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: "1.0".to_string(),
            scripts_dir: None,
            repos: Vec::new(),
            agent_aliases: std::collections::HashMap::new(),
            auto_amend_ledger: false,
            capture_prompts: true,
        }
    }
}

impl Config {
    /// ~/.agentdiff/config.toml
    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .expect("home dir must exist")
            .join(".agentdiff")
            .join("config.toml")
    }

    /// Legacy config path from earlier versions.
    pub fn legacy_config_path() -> PathBuf {
        dirs::home_dir()
            .expect("home dir must exist")
            .join("config.toml")
    }

    pub fn scripts_root(&self) -> PathBuf {
        self.scripts_dir
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap().join(".agentdiff").join("scripts"))
    }

    pub fn auto_amend_ledger_enabled(&self) -> bool {
        self.auto_amend_ledger
    }

    /// Derive slug from repo root path
    pub fn slug_for(repo_root: &std::path::Path) -> String {
        repo_root.to_string_lossy().replace('/', "-")
    }

    /// Session directory for the current repo.
    pub fn repo_session_dir(repo_root: &std::path::Path) -> PathBuf {
        repo_root.join(".git").join("agentdiff")
    }

    pub fn repo_session_log(repo_root: &std::path::Path) -> PathBuf {
        Self::repo_session_dir(repo_root).join("session.jsonl")
    }

    pub fn repo_lockfile(repo_root: &std::path::Path) -> PathBuf {
        Self::repo_session_dir(repo_root).join("hook.lock")
    }

    pub fn repo_pending_context(repo_root: &std::path::Path) -> PathBuf {
        Self::repo_session_dir(repo_root).join("pending.json")
    }

    pub fn repo_pending_ledger(repo_root: &std::path::Path) -> PathBuf {
        Self::repo_session_dir(repo_root).join("pending-ledger.json")
    }

    pub fn load() -> anyhow::Result<Self> {
        let primary = Self::config_path();
        if primary.exists() {
            let raw = std::fs::read_to_string(&primary)?;
            return Ok(toml::from_str(&raw)?);
        }
        let legacy = Self::legacy_config_path();
        if legacy.exists() {
            let raw = std::fs::read_to_string(&legacy)?;
            return Ok(toml::from_str(&raw)?);
        }
        Ok(Config::default())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(self)?;
        std::fs::write(&path, toml_str)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_prompts_defaults_to_true() {
        // When the key is absent from TOML, capture_prompts must default to true.
        // This ensures existing users who haven't set the key see no behavior change.
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.capture_prompts, "capture_prompts must default to true");
    }

    #[test]
    fn capture_prompts_can_be_disabled() {
        let cfg: Config = toml::from_str("capture_prompts = false").unwrap();
        assert!(!cfg.capture_prompts);
    }

    #[test]
    fn capture_prompts_default_struct_matches_serde_default() {
        // Default::default() and serde default must agree.
        let from_default = Config::default();
        let from_toml: Config = toml::from_str("").unwrap();
        assert_eq!(from_default.capture_prompts, from_toml.capture_prompts);
    }
}
