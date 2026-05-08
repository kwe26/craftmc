use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
pub struct ConsoleLine {
    pub seq: u64,
    pub ts: DateTime<Utc>,
    pub stream: &'static str, // "stdout" | "stderr" | "system"
    pub line: String,
}

#[derive(Clone)]
pub struct ConsoleBuffer {
    inner: Arc<Mutex<Inner>>,
    tx: broadcast::Sender<ConsoleLine>,
}

struct Inner {
    buf: VecDeque<ConsoleLine>,
    cap: usize,
    next_seq: u64,
    log_file: Option<PathBuf>,
}

impl ConsoleBuffer {
    pub fn new(cap: usize) -> Self {
        let (tx, _) = broadcast::channel(1024);
        let log_file = Some(PathBuf::from(format!(
            "data/logs/console-{}.log",
            Utc::now().format("%Y%m%d-%H%M%S")
        )));
        Self {
            inner: Arc::new(Mutex::new(Inner { buf: VecDeque::with_capacity(cap), cap, next_seq: 1, log_file })),
            tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConsoleLine> { self.tx.subscribe() }

    pub fn snapshot(&self, last_n: usize) -> Vec<ConsoleLine> {
        let g = self.inner.lock();
        let n = last_n.min(g.buf.len());
        g.buf.iter().rev().take(n).rev().cloned().collect()
    }

    pub fn push(&self, stream: &'static str, line: String) {
        let entry = {
            let mut g = self.inner.lock();
            let seq = g.next_seq;
            g.next_seq += 1;
            let entry = ConsoleLine { seq, ts: Utc::now(), stream, line };
            if g.buf.len() == g.cap { g.buf.pop_front(); }
            g.buf.push_back(entry.clone());
            // Best-effort log write
            if let Some(p) = &g.log_file {
                if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                    use std::io::Write;
                    let _ = writeln!(f, "[{}] [{}] {}", entry.ts.format("%Y-%m-%d %H:%M:%S"), stream, entry.line);
                }
            }
            entry
        };
        let _ = self.tx.send(entry);
    }

    pub fn current_log_path(&self) -> Option<PathBuf> { self.inner.lock().log_file.clone() }
}
