use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

#[derive(Serialize)]
pub struct BackupEntry {
    pub name: String,
    pub size: u64,
    pub created: chrono::DateTime<chrono::Utc>,
}

pub fn backups_dir() -> PathBuf { PathBuf::from("data/backups") }

pub async fn list() -> Result<Vec<BackupEntry>> {
    let dir = backups_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let m = e.metadata().await?;
        if !m.is_file() { continue; }
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".zip") { continue; }
        let created = m.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
            .unwrap_or_else(Utc::now);
        out.push(BackupEntry { name, size: m.len(), created });
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(out)
}

pub async fn delete(name: &str) -> Result<()> {
    let safe = Path::new(name).file_name().ok_or_else(|| anyhow!("bad name"))?.to_string_lossy().to_string();
    if !safe.ends_with(".zip") { return Err(anyhow!("only zip backups")); }
    let p = backups_dir().join(safe);
    if p.exists() { tokio::fs::remove_file(p).await?; }
    Ok(())
}

pub fn backup_path(name: &str) -> Result<PathBuf> {
    let safe = Path::new(name).file_name().ok_or_else(|| anyhow!("bad name"))?.to_string_lossy().to_string();
    if !safe.ends_with(".zip") { return Err(anyhow!("only zip backups")); }
    Ok(backups_dir().join(safe))
}

/// Backup all world-related dirs in server_dir: world, world_nether, world_the_end,
/// plus the new layout world/dimensions/minecraft/{overworld,the_nether,the_end}.
pub async fn create_world_backup(server_dir: PathBuf) -> Result<String> {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let name = format!("world-backup-{stamp}.zip");
    let out = backups_dir().join(&name);
    tokio::fs::create_dir_all(backups_dir()).await?;
    let server_dir_c = server_dir.clone();
    let out_c = out.clone();
    tokio::task::spawn_blocking(move || zip_worlds(&server_dir_c, &out_c)).await??;
    Ok(name)
}

fn zip_worlds(server_dir: &Path, out: &Path) -> Result<()> {
    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let candidates = ["world", "world_nether", "world_the_end"];
    for c in candidates {
        let src = server_dir.join(c);
        if src.exists() && src.is_dir() {
            add_dir_to_zip(&mut zip, server_dir, &src, &opts)?;
        }
    }
    zip.finish()?;
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    base: &Path,
    src: &Path,
    opts: &SimpleFileOptions,
) -> Result<()> {
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(base)?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            if !rel_str.is_empty() {
                zip.add_directory(format!("{rel_str}/"), *opts)?;
            }
        } else if entry.file_type().is_file() {
            zip.start_file(rel_str, *opts)?;
            let data = std::fs::read(path)?;
            zip.write_all(&data)?;
        }
    }
    Ok(())
}

pub async fn prune(keep: usize) -> Result<()> {
    let mut all = list().await?;
    if all.len() <= keep { return Ok(()); }
    let to_delete: Vec<_> = all.split_off(keep);
    for b in to_delete { delete(&b.name).await.ok(); }
    Ok(())
}

pub fn spawn_auto_backup_task(state: crate::state::AppState) {
    tokio::spawn(async move {
        loop {
            let cfg = state.config();
            let interval = cfg.auto_backup_interval_minutes.max(15);
            tokio::time::sleep(std::time::Duration::from_secs(interval * 60)).await;
            let cfg = state.config();
            if !cfg.auto_backup_enabled { continue; }
            state.inner.console.push("system", "Auto-backup starting...".into());
            match create_world_backup(cfg.server_dir.clone()).await {
                Ok(name) => {
                    state.inner.console.push("system", format!("Auto-backup created: {name}"));
                    let _ = prune(cfg.backup_keep).await;
                }
                Err(e) => state.inner.console.push("system", format!("Auto-backup failed: {e}")),
            }
        }
    });
}
