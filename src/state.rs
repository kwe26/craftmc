use crate::config::AppConfig;
use crate::console::ConsoleBuffer;
use crate::deals::DealStore;
use crate::messages::MessageStore;
use crate::metrics::Metrics;
use crate::players::Players;
use crate::server::ServerProc;
use crate::tasks::TaskRegistry;
use anyhow::Result;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<Inner>,
}

pub struct Inner {
    pub config: RwLock<AppConfig>,
    pub server: ServerProc,
    pub console: ConsoleBuffer,
    pub sessions: RwLock<HashSet<String>>,
    pub metrics: Metrics,
    pub tasks: TaskRegistry,
    pub players: Players,
    pub deals: DealStore,
    pub messages: MessageStore,
}

impl AppState {
    pub async fn load() -> Result<Self> {
        tokio::fs::create_dir_all("data").await.ok();
        tokio::fs::create_dir_all("data/backups").await.ok();
        tokio::fs::create_dir_all("data/logs").await.ok();
        let config = AppConfig::load_or_default().await?;
        let console = ConsoleBuffer::new(5000);
        let server = ServerProc::new(console.clone());
        let metrics = Metrics::new(180);
        let tasks = TaskRegistry::new();
        let players = Players::new();
        let deals = DealStore::load().await?;
        let messages = MessageStore::load().await?;
        let state = Self {
            inner: Arc::new(Inner {
                config: RwLock::new(config),
                server,
                console,
                sessions: RwLock::new(HashSet::new()),
                metrics,
                tasks,
                players,
                deals,
                messages,
            }),
        };
        crate::metrics::spawn_collector(state.clone());
        crate::players::spawn_console_parser(state.clone());
        Ok(state)
    }

    pub fn config(&self) -> AppConfig { self.inner.config.read().clone() }
    pub fn set_config(&self, c: AppConfig) { *self.inner.config.write() = c; }
}
