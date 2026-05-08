use anyhow::{anyhow, Result};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    let mut p = root.to_path_buf();
    let rel_path = Path::new(rel.trim_start_matches('/'));
    for c in rel_path.components() {
        match c {
            Component::Normal(s) => p.push(s),
            Component::CurDir => {}
            _ => return Err(anyhow!("invalid path component")),
        }
    }
    Ok(p)
}

pub async fn list_dir(root: &Path, rel: &str) -> Result<Vec<DirEntry>> {
    let dir = safe_join(root, rel)?;
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let meta = e.metadata().await?;
        let name = e.file_name().to_string_lossy().to_string();
        let rel_path = if rel.is_empty() || rel == "/" { format!("/{name}") } else { format!("{}/{name}", rel.trim_end_matches('/')) };
        let modified = meta.modified().ok().and_then(|t| {
            let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
            chrono::DateTime::from_timestamp(dur.as_secs() as i64, 0)
        });
        out.push(DirEntry {
            name,
            path: rel_path,
            is_dir: meta.is_dir(),
            size: meta.len(),
            modified,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}
