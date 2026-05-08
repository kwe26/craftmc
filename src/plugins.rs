use anyhow::{anyhow, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct PluginEntry {
    pub name: String,
    pub size: u64,
    pub enabled: bool,
}

pub fn plugins_dir(server_dir: &Path) -> PathBuf { server_dir.join("plugins") }

pub async fn list(server_dir: &Path) -> Result<Vec<PluginEntry>> {
    let dir = plugins_dir(server_dir);
    tokio::fs::create_dir_all(&dir).await.ok();
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let meta = e.metadata().await?;
        if !meta.is_file() { continue; }
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        let (real_name, enabled) = if lower.ends_with(".jar.disabled") {
            (name.trim_end_matches(".disabled").to_string(), false)
        } else if lower.ends_with(".jar") { (name.clone(), true) } else { continue };
        out.push(PluginEntry { name: real_name, size: meta.len(), enabled });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

pub async fn delete(server_dir: &Path, name: &str) -> Result<()> {
    let dir = plugins_dir(server_dir);
    let safe = sanitize_jar(name)?;
    let candidates = [dir.join(&safe), dir.join(format!("{safe}.disabled"))];
    for p in candidates.iter() {
        if p.exists() { tokio::fs::remove_file(p).await.ok(); }
    }
    Ok(())
}

pub async fn toggle(server_dir: &Path, name: &str, enable: bool) -> Result<()> {
    let dir = plugins_dir(server_dir);
    let safe = sanitize_jar(name)?;
    let on = dir.join(&safe);
    let off = dir.join(format!("{safe}.disabled"));
    if enable && off.exists() {
        tokio::fs::rename(&off, &on).await?;
    } else if !enable && on.exists() {
        tokio::fs::rename(&on, &off).await?;
    }
    Ok(())
}

pub async fn save_uploaded(server_dir: &Path, filename: &str, bytes: &[u8], replace: bool) -> Result<String> {
    let dir = plugins_dir(server_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let safe = sanitize_jar(filename)?;
    let target = dir.join(&safe);
    if target.exists() && !replace {
        return Err(anyhow!("plugin already exists"));
    }
    tokio::fs::write(&target, bytes).await?;
    Ok(safe)
}

pub async fn install_from_url(server_dir: &Path, url: &str, replace: bool) -> Result<String> {
    let parsed = url::Url::parse(url)?;
    let last = parsed.path_segments().and_then(|s| s.last()).unwrap_or("plugin.jar").to_string();
    let name = if last.to_lowercase().ends_with(".jar") { last } else { format!("{last}.jar") };
    let resp = reqwest::get(url).await?;
    if !resp.status().is_success() {
        return Err(anyhow!("download failed: {}", resp.status()));
    }
    let bytes = resp.bytes().await?;
    save_uploaded(server_dir, &name, &bytes, replace).await
}

fn sanitize_jar(name: &str) -> Result<String> {
    let n = Path::new(name).file_name().ok_or_else(|| anyhow!("bad filename"))?.to_string_lossy().to_string();
    if n.contains('/') || n.contains('\\') || n.contains("..") {
        return Err(anyhow!("invalid filename"));
    }
    let lower = n.to_lowercase();
    if !lower.ends_with(".jar") { return Err(anyhow!("must be .jar")); }
    Ok(n)
}
