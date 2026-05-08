use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
pub struct MarketItem {
    pub source: &'static str,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub url: String,
}

const UA: &str = "CraftMC-Manager/0.1 (+https://github.com/local)";

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent(UA).build()?)
}

// ---------- Modrinth ----------

#[derive(Deserialize)]
struct ModrinthSearch { hits: Vec<ModrinthHit> }
#[derive(Deserialize)]
struct ModrinthHit {
    slug: String,
    title: String,
    description: String,
    author: String,
    icon_url: Option<String>,
    downloads: u64,
}
#[derive(Deserialize)]
struct ModrinthVersion {
    files: Vec<ModrinthFile>,
    date_published: String,
}
#[derive(Deserialize)]
struct ModrinthFile {
    url: String,
    filename: String,
    primary: bool,
}

pub async fn search_modrinth(q: &str) -> Result<Vec<MarketItem>> {
    // Plugin-loaders that target Paper/Spigot/Bukkit servers.
    let facets = r#"[["project_type:plugin"],["categories:paper","categories:spigot","categories:bukkit","categories:purpur","categories:folia"]]"#;
    let url = format!(
        "https://api.modrinth.com/v2/search?query={}&facets={}&limit=20",
        urlencoding(q), urlencoding(facets));
    let r: ModrinthSearch = client()?.get(url).send().await?.error_for_status()?.json().await?;
    Ok(r.hits.into_iter().map(|h| MarketItem {
        source: "modrinth",
        url: format!("https://modrinth.com/plugin/{}", h.slug),
        slug: h.slug,
        title: h.title,
        description: h.description,
        author: h.author,
        icon_url: h.icon_url,
        downloads: h.downloads,
    }).collect())
}

pub async fn install_modrinth(slug: &str) -> Result<(String, String)> {
    // List versions filtered to plugin loaders.
    let loaders = r#"["paper","spigot","bukkit","purpur","folia"]"#;
    let url = format!(
        "https://api.modrinth.com/v2/project/{}/version?loaders={}",
        urlencoding(slug), urlencoding(loaders));
    let mut versions: Vec<ModrinthVersion> = client()?.get(&url).send().await?.error_for_status()?.json().await?;
    if versions.is_empty() {
        // Fallback: any version
        let url = format!("https://api.modrinth.com/v2/project/{}/version", urlencoding(slug));
        versions = client()?.get(&url).send().await?.error_for_status()?.json().await?;
    }
    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    let v = versions.into_iter().next().ok_or_else(|| anyhow!("no versions for {slug}"))?;
    let file = v.files.iter().find(|f| f.primary && f.filename.to_lowercase().ends_with(".jar"))
        .or_else(|| v.files.iter().find(|f| f.filename.to_lowercase().ends_with(".jar")))
        .ok_or_else(|| anyhow!("no .jar file in latest version"))?;
    Ok((file.filename.clone(), file.url.clone()))
}

// ---------- Hangar (PaperMC) ----------

#[derive(Deserialize)]
struct HangarSearch { result: Vec<HangarProject> }
#[derive(Deserialize)]
struct HangarProject {
    name: String,
    description: Option<String>,
    namespace: HangarNamespace,
    avatar_url: Option<String>,
    stats: Option<HangarStats>,
}
#[derive(Deserialize)]
struct HangarNamespace { owner: String, slug: String }
#[derive(Deserialize)]
struct HangarStats { #[serde(default)] downloads: u64 }

#[derive(Deserialize)]
struct HangarVersion {
    name: String,
    #[serde(default)]
    downloads: std::collections::HashMap<String, HangarVersionDownload>,
}
#[derive(Deserialize)]
struct HangarVersionDownload {
    #[serde(default)] file_info: Option<HangarFileInfo>,
    #[serde(default)] download_url: Option<String>,
    #[serde(default)] external_url: Option<String>,
}
#[derive(Deserialize)]
struct HangarFileInfo { name: String }

#[derive(Deserialize)]
struct HangarVersionList { result: Vec<HangarVersion> }

pub async fn search_hangar(q: &str) -> Result<Vec<MarketItem>> {
    let url = format!(
        "https://hangar.papermc.io/api/v1/projects?query={}&limit=20&platform=PAPER",
        urlencoding(q));
    let r: HangarSearch = client()?.get(url).send().await?.error_for_status()?.json().await?;
    Ok(r.result.into_iter().map(|p| {
        let full = format!("{}/{}", p.namespace.owner, p.namespace.slug);
        MarketItem {
            source: "hangar",
            url: format!("https://hangar.papermc.io/{full}"),
            slug: full,
            title: p.name,
            description: p.description.unwrap_or_default(),
            author: p.namespace.owner.clone(),
            icon_url: p.avatar_url,
            downloads: p.stats.map(|s| s.downloads).unwrap_or(0),
        }
    }).collect())
}

pub async fn install_hangar(slug: &str) -> Result<(String, String)> {
    // slug is "owner/project"; Hangar endpoints use the project slug only after a lookup.
    let project_slug = slug.split('/').last().unwrap_or(slug);
    let url = format!(
        "https://hangar.papermc.io/api/v1/projects/{}/versions?limit=1&platform=PAPER",
        urlencoding(project_slug));
    let v: HangarVersionList = client()?.get(url).send().await?.error_for_status()?.json().await?;
    let ver = v.result.into_iter().next().ok_or_else(|| anyhow!("no versions"))?;
    let dl = ver.downloads.get("PAPER")
        .or_else(|| ver.downloads.values().next())
        .ok_or_else(|| anyhow!("no Paper download"))?;
    let url = dl.download_url.clone()
        .or_else(|| dl.external_url.clone())
        .unwrap_or_else(|| format!("https://hangar.papermc.io/api/v1/projects/{}/versions/{}/PAPER/download",
            urlencoding(project_slug), urlencoding(&ver.name)));
    let filename = dl.file_info.as_ref().map(|f| f.name.clone())
        .unwrap_or_else(|| format!("{}-{}.jar", project_slug, ver.name));
    Ok((filename, url))
}

fn urlencoding(s: &str) -> String {
    // Minimal URL-encoder: escape non-unreserved chars per RFC 3986.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
