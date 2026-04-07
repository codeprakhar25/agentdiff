use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Schema version for future migrations
    #[serde(default = "default_schema")]
    pub schema_version: String,

    /// Global spillover root — defaults to ~/.agentdiff/spillover
    #[serde(default)]
    pub data_dir: Option<PathBuf>,

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

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: "1.0".to_string(),
            data_dir: None,
            scripts_dir: None,
            repos: Vec::new(),
            agent_aliases: std::collections::HashMap::new(),
            auto_amend_ledger: false,
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

    /// Legacy config path from earlier agentblame versions.
    pub fn legacy_config_path() -> PathBuf {
        dirs::home_dir()
            .expect("home dir must exist")
            .join(".agentblame")
            .join("config.toml")
    }

    /// Global spillover dir for events captured outside a git repo.
    pub fn spillover_root(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap()
                .join(".agentdiff")
                .join("spillover")
        })
    }

    pub fn scripts_root(&self) -> PathBuf {
        self.scripts_dir
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap().join(".agentdiff").join("scripts"))
    }

    pub fn auto_amend_ledger_enabled(&self) -> bool {
        self.auto_amend_ledger
    }

    /// Derive slug from repo root path: /home/prakh/dev/rust → -home-prakh-dev-rust
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

    pub fn repo_ledger_dir(repo_root: &std::path::Path) -> PathBuf {
        repo_root.join(".agentdiff")
    }

    pub fn repo_ledger_path(repo_root: &std::path::Path) -> PathBuf {
        Self::repo_ledger_dir(repo_root).join("ledger.jsonl")
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
