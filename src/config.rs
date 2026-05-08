use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub setup_complete: bool,
    pub password_hash: Option<String>,
    pub server_dir: PathBuf,
    pub paper_url: Option<String>,
    pub min_ram_mb: u32,
    pub max_ram_mb: u32,
    pub java_path: String,
    pub jvm_args: Vec<String>,
    pub crash_action: CrashAction,
    pub auto_restart_max: u32,
    pub host: String,
    pub motd: String,
    pub auto_backup_enabled: bool,
    pub auto_backup_interval_minutes: u64,
    pub backup_keep: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrashAction {
    Stop,
    Restart,
    RestartWithBackoff,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            setup_complete: false,
            password_hash: None,
            server_dir: PathBuf::from("server"),
            paper_url: None,
            min_ram_mb: 1024,
            max_ram_mb: 4096,
            java_path: "java".into(),
            jvm_args: vec![
                "-XX:+UseG1GC".into(),
                "-XX:+ParallelRefProcEnabled".into(),
                "-XX:MaxGCPauseMillis=200".into(),
            ],
            crash_action: CrashAction::RestartWithBackoff,
            auto_restart_max: 5,
            host: "0.0.0.0".into(),
            motd: "A CraftMC Server".into(),
            auto_backup_enabled: true,
            auto_backup_interval_minutes: 360,
            backup_keep: 10,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        PathBuf::from("data/config.toml")
    }

    pub async fn load_or_default() -> Result<Self> {
        let p = Self::config_path();
        if p.exists() {
            let s = tokio::fs::read_to_string(&p).await?;
            Ok(toml::from_str(&s)?)
        } else {
            Ok(Self::default())
        }
    }

    pub async fn save(&self) -> Result<()> {
        let p = Self::config_path();
        if let Some(parent) = p.parent() { tokio::fs::create_dir_all(parent).await?; }
        let s = toml::to_string_pretty(self)?;
        tokio::fs::write(&p, s).await?;
        Ok(())
    }
}
