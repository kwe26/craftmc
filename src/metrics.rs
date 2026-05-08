use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Clone, Serialize)]
pub struct MetricsSnapshot {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub cpu_percent: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub mem_percent: f32,
    pub server_cpu_percent: Option<f32>,
    pub server_mem_mb: Option<u64>,
    pub disk_total_gb: f64,
    pub disk_used_gb: f64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub uptime_secs: u64,
    pub load_avg: Option<f64>,
}

#[derive(Clone)]
pub struct Metrics {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    history: VecDeque<MetricsSnapshot>,
    cap: usize,
    last_disk_read: u64,
    last_disk_write: u64,
    last_tick: std::time::Instant,
}

impl Metrics {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                history: VecDeque::with_capacity(cap),
                cap,
                last_disk_read: 0,
                last_disk_write: 0,
                last_tick: std::time::Instant::now(),
            })),
        }
    }

    pub fn history(&self) -> Vec<MetricsSnapshot> {
        let g = self.inner.lock();
        g.history.iter().cloned().collect()
    }

    pub fn latest(&self) -> Option<MetricsSnapshot> {
        self.inner.lock().history.back().cloned()
    }
}

pub fn spawn_collector(state: crate::state::AppState) {
    let metrics = state.inner.metrics.clone();
    tokio::task::spawn_blocking(move || {
        let mut sys = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(sysinfo::CpuRefreshKind::everything())
                .with_memory(sysinfo::MemoryRefreshKind::everything())
                .with_processes(ProcessRefreshKind::new().with_cpu().with_memory().with_disk_usage()),
        );
        let mut disks = Disks::new_with_refreshed_list();
        // Prime CPU readings.
        sys.refresh_cpu_all();
        std::thread::sleep(Duration::from_millis(250));
        loop {
            sys.refresh_cpu_all();
            sys.refresh_memory();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            disks.refresh();

            let global_cpu: f32 = {
                let cpus = sys.cpus();
                if cpus.is_empty() { 0.0 } else { cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32 }
            };
            let mem_used = sys.used_memory() / (1024 * 1024);
            let mem_total = sys.total_memory() / (1024 * 1024);
            let mem_pct = if mem_total > 0 { (mem_used as f32 / mem_total as f32) * 100.0 } else { 0.0 };

            let server_pid = state.inner.server.info().pid;
            let (server_cpu, server_mem, server_read, server_write) = if let Some(pid) = server_pid {
                if let Some(p) = sys.process(Pid::from_u32(pid)) {
                    let du = p.disk_usage();
                    (Some(p.cpu_usage()), Some(p.memory() / (1024 * 1024)), du.read_bytes, du.written_bytes)
                } else { (None, None, 0, 0) }
            } else { (None, None, 0, 0) };

            // Disk totals across all mounted disks.
            let mut total = 0u64;
            let mut used = 0u64;
            for d in disks.list() {
                total += d.total_space();
                used += d.total_space().saturating_sub(d.available_space());
            }
            let disk_total_gb = total as f64 / 1024f64.powi(3);
            let disk_used_gb = used as f64 / 1024f64.powi(3);

            // Disk RW: use server-process delta if available.
            let now = std::time::Instant::now();
            let snap = {
                let mut g = metrics.inner.lock();
                let dt = now.duration_since(g.last_tick).as_secs_f64().max(0.001);
                let read_bps = if g.last_disk_read == 0 { 0 } else { ((server_read.saturating_sub(g.last_disk_read)) as f64 / dt) as u64 };
                let write_bps = if g.last_disk_write == 0 { 0 } else { ((server_write.saturating_sub(g.last_disk_write)) as f64 / dt) as u64 };
                g.last_disk_read = server_read;
                g.last_disk_write = server_write;
                g.last_tick = now;
                MetricsSnapshot {
                    ts: chrono::Utc::now(),
                    cpu_percent: global_cpu,
                    mem_used_mb: mem_used,
                    mem_total_mb: mem_total,
                    mem_percent: mem_pct,
                    server_cpu_percent: server_cpu,
                    server_mem_mb: server_mem,
                    disk_total_gb,
                    disk_used_gb,
                    disk_read_bps: read_bps,
                    disk_write_bps: write_bps,
                    uptime_secs: System::uptime(),
                    load_avg: {
                        let l = System::load_average();
                        if l.one >= 0.0 { Some(l.one) } else { None }
                    },
                }
            };
            {
                let mut g = metrics.inner.lock();
                if g.history.len() == g.cap { g.history.pop_front(); }
                g.history.push_back(snap);
            }
            std::thread::sleep(Duration::from_millis(2000));
        }
    });
}
