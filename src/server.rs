use crate::config::{AppConfig, CrashAction};
use crate::console::ConsoleBuffer;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

#[derive(Clone, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}

#[derive(Clone)]
pub struct ServerProc {
    inner: Arc<Mutex<Inner>>,
    console: ConsoleBuffer,
}

struct Inner {
    status: ServerStatus,
    child_kill: Option<mpsc::Sender<KillSig>>,
    stdin_tx: Option<mpsc::Sender<String>>,
    pid: Option<u32>,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    restart_count: u32,
}

enum KillSig { Stop, Kill }

#[derive(Serialize)]
pub struct ServerInfo {
    pub status: ServerStatus,
    pub pid: Option<u32>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub restart_count: u32,
}

impl ServerProc {
    pub fn new(console: ConsoleBuffer) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                status: ServerStatus::Stopped,
                child_kill: None,
                stdin_tx: None,
                pid: None,
                started_at: None,
                restart_count: 0,
            })),
            console,
        }
    }

    pub fn info(&self) -> ServerInfo {
        let g = self.inner.lock();
        ServerInfo {
            status: g.status.clone(),
            pid: g.pid,
            started_at: g.started_at,
            restart_count: g.restart_count,
        }
    }

    pub fn status(&self) -> ServerStatus { self.inner.lock().status.clone() }

    pub async fn send_command(&self, cmd: &str) -> Result<()> {
        let tx = { self.inner.lock().stdin_tx.clone() };
        let tx = tx.ok_or_else(|| anyhow!("server not running"))?;
        tx.send(format!("{cmd}\n")).await.map_err(|_| anyhow!("stdin closed"))?;
        Ok(())
    }

    pub async fn stop(&self, force: bool) -> Result<()> {
        let tx = { self.inner.lock().child_kill.clone() };
        if let Some(tx) = tx {
            let sig = if force { KillSig::Kill } else { KillSig::Stop };
            tx.send(sig).await.ok();
            self.inner.lock().status = ServerStatus::Stopping;
            Ok(())
        } else {
            Err(anyhow!("server not running"))
        }
    }

    pub async fn restart(&self, cfg: AppConfig) -> Result<()> {
        if self.status() != ServerStatus::Stopped {
            self.stop(false).await.ok();
            // wait briefly
            for _ in 0..60 {
                if self.status() == ServerStatus::Stopped { break; }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        self.start(cfg).await
    }

    pub fn start_boxed(&self, cfg: AppConfig) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> {
        let s = self.clone();
        Box::pin(async move { s.start(cfg).await })
    }

    pub async fn start(&self, cfg: AppConfig) -> Result<()> {
        {
            let g = self.inner.lock();
            if !matches!(g.status, ServerStatus::Stopped | ServerStatus::Crashed) {
                return Err(anyhow!("server already running"));
            }
        }
        let server_dir = cfg.server_dir.clone();
        if !server_dir.join("server.jar").exists() {
            return Err(anyhow!("server.jar not found in {}", server_dir.display()));
        }
        // Ensure eula
        let eula_path = server_dir.join("eula.txt");
        if !eula_path.exists() {
            tokio::fs::write(&eula_path, "eula=true\n").await.ok();
        }

        let jvm_args = cfg.jvm_args.clone();
        let xms = format!("-Xms{}M", cfg.min_ram_mb);
        let xmx = format!("-Xmx{}M", cfg.max_ram_mb);
        let java = cfg.java_path.clone();

        let mut cmd = Command::new(&java);
        cmd.current_dir(&server_dir)
            .arg(&xms)
            .arg(&xmx)
            .args(&jvm_args)
            .arg("-jar")
            .arg("server.jar")
            .arg("nogui")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.console.push("system", format!("Starting: {} {} {} {} -jar server.jar nogui", java, xms, xmx, jvm_args.join(" ")));

        let mut child: Child = cmd.spawn().map_err(|e| anyhow!("failed to spawn java: {e}"))?;
        let pid = child.id();

        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;

        let (kill_tx, mut kill_rx) = mpsc::channel::<KillSig>(4);
        let (stdin_tx, stdin_rx) = mpsc::channel::<String>(64);

        {
            let mut g = self.inner.lock();
            g.status = ServerStatus::Starting;
            g.child_kill = Some(kill_tx);
            g.stdin_tx = Some(stdin_tx);
            g.pid = pid;
            g.started_at = Some(chrono::Utc::now());
        }

        // stdout reader
        let console = self.console.clone();
        tokio::spawn(async move {
            let mut r = BufReader::new(stdout).lines();
            while let Ok(Some(l)) = r.next_line().await {
                console.push("stdout", l);
            }
        });
        let console_e = self.console.clone();
        tokio::spawn(async move {
            let mut r = BufReader::new(stderr).lines();
            while let Ok(Some(l)) = r.next_line().await {
                console_e.push("stderr", l);
            }
        });

        // stdin writer
        tokio::spawn(write_stdin(stdin, stdin_rx));

        // status: starting -> running once we detect "Done ("
        let console_s = self.console.clone();
        let inner_s = self.inner.clone();
        tokio::spawn(async move {
            let mut rx = console_s.subscribe();
            while let Ok(line) = rx.recv().await {
                if line.line.contains("Done (") && line.line.contains("For help, type") {
                    inner_s.lock().status = ServerStatus::Running;
                    break;
                }
                if matches!(inner_s.lock().status, ServerStatus::Stopping | ServerStatus::Stopped | ServerStatus::Crashed) { break; }
            }
        });

        // process supervisor
        let inner_p = self.inner.clone();
        let console_p = self.console.clone();
        let cfg_p = cfg.clone();
        let self_clone = self.clone();
        tokio::spawn(async move {
            let exit = tokio::select! {
                ex = child.wait() => ex,
                sig = kill_rx.recv() => {
                    match sig {
                        Some(KillSig::Stop) => {
                            // try graceful "stop" via stdin
                            let stdin_tx = { inner_p.lock().stdin_tx.clone() };
                            if let Some(tx) = stdin_tx { tx.send("stop\n".into()).await.ok(); }
                            // wait up to 30s
                            let waited = tokio::time::timeout(std::time::Duration::from_secs(30), child.wait()).await;
                            match waited {
                                Ok(r) => r,
                                Err(_) => { let _ = child.kill().await; child.wait().await }
                            }
                        }
                        _ => { let _ = child.kill().await; child.wait().await }
                    }
                }
            };
            let exit_ok = matches!(&exit, Ok(s) if s.success());
            console_p.push("system", format!("Server exited: {:?}", exit));

            let was_stopping = { matches!(inner_p.lock().status, ServerStatus::Stopping) };
            {
                let mut g = inner_p.lock();
                g.stdin_tx = None;
                g.child_kill = None;
                g.pid = None;
                g.status = if exit_ok || was_stopping { ServerStatus::Stopped } else { ServerStatus::Crashed };
            }

            if !exit_ok && !was_stopping {
                let action = cfg_p.crash_action.clone();
                if matches!(action, CrashAction::Restart | CrashAction::RestartWithBackoff) {
                    let count = { let mut g = inner_p.lock(); g.restart_count += 1; g.restart_count };
                    if count <= cfg_p.auto_restart_max {
                        let backoff = if matches!(action, CrashAction::RestartWithBackoff) {
                            std::time::Duration::from_secs((5 * count as u64).min(60))
                        } else { std::time::Duration::from_secs(2) };
                        console_p.push("system", format!("Auto-restart in {}s (attempt {}/{})", backoff.as_secs(), count, cfg_p.auto_restart_max));
                        tokio::time::sleep(backoff).await;
                        if let Err(e) = self_clone.start_boxed(cfg_p).await {
                            console_p.push("system", format!("Auto-restart failed: {e}"));
                        }
                    } else {
                        console_p.push("system", format!("Auto-restart limit reached ({})", cfg_p.auto_restart_max));
                    }
                }
            } else {
                inner_p.lock().restart_count = 0;
            }
        });

        Ok(())
    }
}

async fn write_stdin(mut stdin: ChildStdin, mut rx: mpsc::Receiver<String>) {
    while let Some(s) = rx.recv().await {
        if stdin.write_all(s.as_bytes()).await.is_err() { break; }
        if stdin.flush().await.is_err() { break; }
    }
}

// Helper: detect server.jar in dir
pub fn has_server_jar(dir: &Path) -> bool {
    dir.join("server.jar").exists()
}
