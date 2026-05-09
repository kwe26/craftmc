use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Signature {
    pub name: String,
    pub signed_at: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Rejection {
    pub name: String,
    pub rejected_at: DateTime<Utc>,
    pub reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Deal {
    pub id: String,
    pub title: String,
    pub body: String,
    pub parties: Vec<String>,
    #[serde(default)]
    pub signatures: Vec<Signature>,
    #[serde(default)]
    pub rejections: Vec<Rejection>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DealView {
    #[serde(flatten)]
    pub deal: Deal,
    pub status: String,
    pub remaining: Vec<String>,
}

impl Deal {
    pub fn status(&self) -> &'static str {
        if !self.rejections.is_empty() { return "rejected"; }
        let all_signed = !self.parties.is_empty()
            && self.parties.iter().all(|p| self.signatures.iter().any(|s| eq_ci(&s.name, p)));
        if all_signed { return "signed"; }
        if !self.signatures.is_empty() { "partial" } else { "pending" }
    }

    pub fn remaining(&self) -> Vec<String> {
        self.parties.iter()
            .filter(|p| !self.signatures.iter().any(|s| eq_ci(&s.name, p)))
            .filter(|p| !self.rejections.iter().any(|r| eq_ci(&r.name, p)))
            .cloned().collect()
    }

    pub fn view(&self) -> DealView {
        DealView { remaining: self.remaining(), status: self.status().into(), deal: self.clone() }
    }
}

fn eq_ci(a: &str, b: &str) -> bool { a.eq_ignore_ascii_case(b) }

#[derive(Clone)]
pub struct DealStore {
    inner: Arc<RwLock<HashMap<String, Deal>>>,
    path: PathBuf,
}

impl DealStore {
    pub async fn load() -> Result<Self> {
        let path = PathBuf::from("data/deals.json");
        let map: HashMap<String, Deal> = if path.exists() {
            let s = tokio::fs::read_to_string(&path).await?;
            serde_json::from_str(&s).unwrap_or_default()
        } else { HashMap::new() };
        Ok(Self { inner: Arc::new(RwLock::new(map)), path })
    }

    async fn save(&self) -> Result<()> {
        let s = { serde_json::to_string_pretty(&*self.inner.read())? };
        if let Some(parent) = self.path.parent() { tokio::fs::create_dir_all(parent).await.ok(); }
        tokio::fs::write(&self.path, s).await?;
        Ok(())
    }

    pub fn list(&self) -> Vec<DealView> {
        let mut v: Vec<DealView> = self.inner.read().values().map(|d| d.view()).collect();
        v.sort_by(|a, b| b.deal.created_at.cmp(&a.deal.created_at));
        v
    }

    pub fn get(&self, id: &str) -> Option<DealView> {
        self.inner.read().get(id).map(|d| d.view())
    }

    pub async fn create(&self, title: String, body: String, parties: Vec<String>, created_by: Option<String>) -> Result<DealView> {
        if title.trim().is_empty() { return Err(anyhow!("title required")); }
        if title.len() > 200 { return Err(anyhow!("title too long")); }
        if body.len() > 16_000 { return Err(anyhow!("body too long (16 KB max)")); }
        let parties: Vec<String> = parties.into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parties.is_empty() { return Err(anyhow!("at least one party required")); }
        if parties.len() > 32 { return Err(anyhow!("too many parties (32 max)")); }
        for p in &parties {
            if p.len() > 16 || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(anyhow!("invalid party name: {p}"));
            }
        }
        let id = short_id();
        let deal = Deal {
            id: id.clone(),
            title: title.trim().into(),
            body,
            parties,
            signatures: Vec::new(),
            rejections: Vec::new(),
            created_at: Utc::now(),
            created_by: created_by.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        };
        self.inner.write().insert(id.clone(), deal.clone());
        self.save().await?;
        Ok(deal.view())
    }

    pub async fn approve(&self, id: &str, name: &str) -> Result<DealView> {
        if name.trim().is_empty() { return Err(anyhow!("name required")); }
        let view = {
            let mut g = self.inner.write();
            let deal = g.get_mut(id).ok_or_else(|| anyhow!("deal not found"))?;
            if !deal.parties.iter().any(|p| eq_ci(p, name)) {
                return Err(anyhow!("'{}' is not a party to this deal", name));
            }
            if deal.signatures.iter().any(|s| eq_ci(&s.name, name)) {
                return Err(anyhow!("'{}' has already signed", name));
            }
            if deal.rejections.iter().any(|r| eq_ci(&r.name, name)) {
                return Err(anyhow!("'{}' has already rejected", name));
            }
            deal.signatures.push(Signature { name: name.to_string(), signed_at: Utc::now() });
            deal.view()
        };
        self.save().await?;
        Ok(view)
    }

    pub async fn reject(&self, id: &str, name: &str, reason: Option<String>) -> Result<DealView> {
        if name.trim().is_empty() { return Err(anyhow!("name required")); }
        let view = {
            let mut g = self.inner.write();
            let deal = g.get_mut(id).ok_or_else(|| anyhow!("deal not found"))?;
            if !deal.parties.iter().any(|p| eq_ci(p, name)) {
                return Err(anyhow!("'{}' is not a party to this deal", name));
            }
            if deal.rejections.iter().any(|r| eq_ci(&r.name, name)) {
                return Err(anyhow!("'{}' has already rejected", name));
            }
            deal.rejections.push(Rejection { name: name.to_string(), rejected_at: Utc::now(), reason });
            deal.view()
        };
        self.save().await?;
        Ok(view)
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let removed = self.inner.write().remove(id).is_some();
        if removed { self.save().await?; }
        Ok(())
    }
}

fn short_id() -> String {
    // 8-char base36-ish from a UUID — short enough for /sign in chat.
    let u = uuid::Uuid::new_v4().simple().to_string();
    u[..8].to_string()
}
