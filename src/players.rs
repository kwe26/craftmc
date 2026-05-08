use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone, Serialize, Debug)]
pub struct Player {
    pub name: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Players {
    inner: Arc<RwLock<BTreeMap<String, Player>>>,
    tx: broadcast::Sender<Vec<Player>>,
}

impl Players {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { inner: Arc::new(RwLock::new(BTreeMap::new())), tx }
    }

    pub fn list(&self) -> Vec<Player> {
        self.inner.read().values().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Vec<Player>> { self.tx.subscribe() }

    fn emit(&self) {
        let snap = self.list();
        let _ = self.tx.send(snap);
    }

    pub fn add(&self, name: &str) {
        let mut g = self.inner.write();
        if g.contains_key(name) { return; }
        g.insert(name.to_string(), Player { name: name.to_string(), joined_at: Utc::now() });
        drop(g);
        self.emit();
    }

    pub fn remove(&self, name: &str) {
        let mut g = self.inner.write();
        if g.remove(name).is_some() {
            drop(g);
            self.emit();
        }
    }

    pub fn clear(&self) {
        let mut g = self.inner.write();
        if g.is_empty() { return; }
        g.clear();
        drop(g);
        self.emit();
    }
}

/// Spawn a console-listener that maintains the player set.
/// Patterns recognised on Paper / Spigot / vanilla:
///   - "<Name> joined the game"
///   - "<Name> left the game"
///   - "Stopping the server" / system "Server exited" → clear
pub fn spawn_console_parser(state: crate::state::AppState) {
    let mut rx = state.inner.console.subscribe();
    let players = state.inner.players.clone();
    tokio::spawn(async move {
        while let Ok(line) = rx.recv().await {
            // Strip the standard "[hh:mm:ss INFO]:" prefix if present so we match the message text.
            let msg = strip_log_prefix(&line.line);
            if line.stream == "system" {
                let l = msg.to_lowercase();
                if l.contains("server exited") || l.contains("stopping the server") {
                    players.clear();
                }
                continue;
            }
            // join: "Name joined the game" — also "Name[/ip:port] logged in" appears earlier
            if let Some(name) = msg.strip_suffix(" joined the game") {
                let name = name.trim();
                if is_valid_name(name) { players.add(name); }
            } else if let Some(name) = msg.strip_suffix(" left the game") {
                let name = name.trim();
                if is_valid_name(name) { players.remove(name); }
            } else if msg.contains("lost connection:") {
                // "Name lost connection: ..." — fallback removal
                if let Some(name) = msg.split_whitespace().next() {
                    if is_valid_name(name) { players.remove(name); }
                }
            }
        }
    });
}

fn strip_log_prefix(line: &str) -> &str {
    // Format: "[12:34:56 INFO]: ..." or "[12:34:56] [Server thread/INFO]: ..."
    if let Some(idx) = line.find("]: ") { return &line[idx + 3..]; }
    line
}

fn is_valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 16
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn is_valid_name_pub(s: &str) -> bool { is_valid_name(s) }
