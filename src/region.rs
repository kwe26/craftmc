// Minimal Anvil (.mca) region file inspector.
// Anvil format: 32x32 chunks per region. Header is 8 KiB:
//   - 4 KiB locations (4 bytes per chunk: 3-byte offset in 4KiB sectors, 1-byte length in sectors)
//   - 4 KiB timestamps (4 bytes each, last-modified per chunk)
// We expose: list chunks (which exist + size + timestamp + compression), and a
// destructive "clear chunk" action (zero its location entry — Minecraft will regenerate it).

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct RegionFileInfo {
    pub path: String,
    pub size: u64,
    pub chunk_count: u32,
}

#[derive(Serialize)]
pub struct ChunkInfo {
    pub x: u32,
    pub z: u32,
    pub offset_sectors: u32,
    pub length_sectors: u32,
    pub data_length: u32,
    pub compression: u8,
    pub timestamp: u32,
}

/// New 1.21+ layout: world/dimensions/minecraft/{overworld,the_nether,the_end}/region/*.mca
/// Old layout still supported: world/region, world_nether/DIM-1/region, world_the_end/DIM1/region.
pub fn dimension_region_dirs(server_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let new_overworld = server_dir.join("world/dimensions/minecraft/overworld/region");
    let new_nether = server_dir.join("world/dimensions/minecraft/the_nether/region");
    let new_end = server_dir.join("world/dimensions/minecraft/the_end/region");
    if new_overworld.exists() { out.push(("overworld".into(), new_overworld)); }
    else if server_dir.join("world/region").exists() { out.push(("overworld".into(), server_dir.join("world/region"))); }
    if new_nether.exists() { out.push(("the_nether".into(), new_nether)); }
    else if server_dir.join("world_nether/DIM-1/region").exists() { out.push(("the_nether".into(), server_dir.join("world_nether/DIM-1/region"))); }
    if new_end.exists() { out.push(("the_end".into(), new_end)); }
    else if server_dir.join("world_the_end/DIM1/region").exists() { out.push(("the_end".into(), server_dir.join("world_the_end/DIM1/region"))); }
    out
}

pub async fn list_region_files(server_dir: &Path, dimension: &str) -> Result<Vec<RegionFileInfo>> {
    let dirs = dimension_region_dirs(server_dir);
    let dir = dirs.into_iter().find(|(d, _)| d == dimension).ok_or_else(|| anyhow!("unknown dimension"))?.1;
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let m = e.metadata().await?;
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".mca") || !m.is_file() { continue; }
        let p = e.path();
        let chunk_count = tokio::task::spawn_blocking({
            let p = p.clone();
            move || count_chunks(&p).unwrap_or(0)
        }).await.unwrap_or(0);
        out.push(RegionFileInfo {
            path: format!("{dimension}/{name}"),
            size: m.len(),
            chunk_count,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

pub fn region_file_path(server_dir: &Path, dimension: &str, file: &str) -> Result<PathBuf> {
    let dirs = dimension_region_dirs(server_dir);
    let dir = dirs.into_iter().find(|(d, _)| d == dimension).ok_or_else(|| anyhow!("unknown dimension"))?.1;
    let safe = Path::new(file).file_name().ok_or_else(|| anyhow!("bad name"))?.to_string_lossy().to_string();
    if !safe.ends_with(".mca") { return Err(anyhow!("not an mca")); }
    Ok(dir.join(safe))
}

fn count_chunks(path: &Path) -> Result<u32> {
    let mut f = std::fs::File::open(path)?;
    let mut hdr = [0u8; 4096];
    if f.read_exact(&mut hdr).is_err() { return Ok(0); }
    let mut count = 0u32;
    for i in 0..1024 {
        let off = i * 4;
        let entry = u32::from_be_bytes([0, hdr[off], hdr[off + 1], hdr[off + 2]]);
        let len = hdr[off + 3];
        if entry != 0 && len != 0 { count += 1; }
    }
    Ok(count)
}

pub fn read_chunks(path: &Path) -> Result<Vec<ChunkInfo>> {
    let mut f = std::fs::File::open(path)?;
    let mut loc = [0u8; 4096];
    let mut ts = [0u8; 4096];
    f.read_exact(&mut loc)?;
    f.read_exact(&mut ts)?;
    let mut out = Vec::new();
    for i in 0..1024 {
        let off = i * 4;
        let offset_sectors = u32::from_be_bytes([0, loc[off], loc[off + 1], loc[off + 2]]);
        let length_sectors = loc[off + 3] as u32;
        if offset_sectors == 0 || length_sectors == 0 { continue; }
        let timestamp = u32::from_be_bytes([ts[off], ts[off + 1], ts[off + 2], ts[off + 3]]);
        let chunk_byte_off = (offset_sectors as u64) * 4096;
        let mut header = [0u8; 5];
        let (data_length, compression) = match (|| -> std::io::Result<_> {
            f.seek(SeekFrom::Start(chunk_byte_off))?;
            f.read_exact(&mut header)?;
            Ok((u32::from_be_bytes([header[0], header[1], header[2], header[3]]), header[4]))
        })() {
            Ok(v) => v,
            Err(_) => (0, 0),
        };
        let x = (i % 32) as u32;
        let z = (i / 32) as u32;
        out.push(ChunkInfo { x, z, offset_sectors, length_sectors, data_length, compression, timestamp });
    }
    Ok(out)
}

/// Zero out the chunk's location entry so the server regenerates it.
pub fn clear_chunk(path: &Path, x: u32, z: u32) -> Result<()> {
    if x >= 32 || z >= 32 { return Err(anyhow!("chunk x/z must be 0..32")); }
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let idx = (z * 32 + x) as u64;
    f.seek(SeekFrom::Start(idx * 4))?;
    f.write_all(&[0u8; 4])?;
    f.flush()?;
    Ok(())
}
