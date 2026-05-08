use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus { Running, Done, Failed }

#[derive(Clone, Serialize, Debug)]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub status: TaskStatus,
    pub progress: u64,
    pub total: Option<u64>,
    pub message: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct TaskRegistry {
    inner: Arc<RwLock<HashMap<String, Task>>>,
    tx: broadcast::Sender<Task>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { inner: Arc::new(RwLock::new(HashMap::new())), tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Task> { self.tx.subscribe() }

    pub fn create(&self, kind: &str, label: &str) -> Task {
        let id = Uuid::new_v4().to_string();
        let t = Task {
            id: id.clone(),
            kind: kind.into(),
            label: label.into(),
            status: TaskStatus::Running,
            progress: 0,
            total: None,
            message: String::new(),
            started_at: Utc::now(),
            finished_at: None,
        };
        self.inner.write().insert(id, t.clone());
        let _ = self.tx.send(t.clone());
        t
    }

    pub fn update<F: FnOnce(&mut Task)>(&self, id: &str, f: F) {
        let mut g = self.inner.write();
        if let Some(t) = g.get_mut(id) {
            f(t);
            let snapshot = t.clone();
            drop(g);
            let _ = self.tx.send(snapshot);
        }
    }

    pub fn finish_done(&self, id: &str, msg: impl Into<String>) {
        self.update(id, |t| {
            t.status = TaskStatus::Done;
            t.message = msg.into();
            t.finished_at = Some(Utc::now());
            if let Some(total) = t.total { t.progress = total; }
        });
    }

    pub fn finish_failed(&self, id: &str, msg: impl Into<String>) {
        self.update(id, |t| {
            t.status = TaskStatus::Failed;
            t.message = msg.into();
            t.finished_at = Some(Utc::now());
        });
    }

    pub fn list(&self) -> Vec<Task> {
        let mut v: Vec<Task> = self.inner.read().values().cloned().collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v
    }

    pub fn prune_old(&self, keep: usize) {
        let mut g = self.inner.write();
        if g.len() <= keep { return; }
        let mut v: Vec<_> = g.values().cloned().collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        for t in v.into_iter().skip(keep) { g.remove(&t.id); }
    }
}

/// Stream a reqwest response body to a file, updating task progress.
pub async fn download_with_progress(
    tasks: &TaskRegistry,
    task_id: &str,
    url: &str,
    target: &std::path::Path,
) -> anyhow::Result<()> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;
    let resp = reqwest::get(url).await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length();
    tasks.update(task_id, |t| { t.total = total; t.message = "downloading".into(); });

    if let Some(parent) = target.parent() { tokio::fs::create_dir_all(parent).await.ok(); }
    let mut f = tokio::fs::File::create(target).await?;
    let mut stream = resp.bytes_stream();
    let mut received: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        f.write_all(&bytes).await?;
        received += bytes.len() as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(120) {
            tasks.update(task_id, |t| { t.progress = received; });
            last_emit = std::time::Instant::now();
        }
    }
    f.flush().await?;
    tasks.update(task_id, |t| { t.progress = received; });
    Ok(())
}
