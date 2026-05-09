use anyhow::Result;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Message {
    pub id: String,
    pub player_name: String,
    pub message_type: MessageType,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Chat,
    Join,
    Leave,
}

#[derive(Serialize, Deserialize)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub page: u32,
    pub total_pages: u32,
    pub total_count: u32,
    pub per_page: u32,
}

const MAX_MESSAGES: usize = 10000;
const PER_PAGE: u32 = 50;

#[derive(Clone)]
pub struct MessageStore {
    inner: Arc<RwLock<VecDeque<Message>>>,
    path: PathBuf,
}

impl MessageStore {
    pub async fn load() -> Result<Self> {
        let path = PathBuf::from("data/messages.json");
        let messages: Vec<Message> = if path.exists() {
            let s = tokio::fs::read_to_string(&path).await?;
            serde_json::from_str(&s).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut deque = VecDeque::new();
        for msg in messages {
            deque.push_back(msg);
        }
        Ok(Self {
            inner: Arc::new(RwLock::new(deque)),
            path,
        })
    }

    async fn save(&self) -> Result<()> {
        let messages: Vec<Message> = self.inner.read().iter().cloned().collect();
        let s = serde_json::to_string_pretty(&messages)?;
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(&self.path, s).await?;
        Ok(())
    }

    pub async fn add(&self, player_name: String, message_type: MessageType, content: String) -> Result<Message> {
        let message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            player_name,
            message_type,
            content,
            timestamp: Utc::now(),
        };

        {
            let mut inner = self.inner.write();
            inner.push_back(message.clone());
            
            // Keep only the last MAX_MESSAGES
            if inner.len() > MAX_MESSAGES {
                inner.pop_front();
            }
        } // Drop the write guard here before await
        
        self.save().await?;
        Ok(message)
    }

    pub fn get_page(&self, page: u32) -> MessagePage {
        let inner = self.inner.read();
        let total_count = inner.len() as u32;
        let total_pages = (total_count + PER_PAGE - 1) / PER_PAGE;
        let page = page.max(1).min(total_pages.max(1));

        let skip = ((page - 1) * PER_PAGE) as usize;
        let messages: Vec<Message> = inner
            .iter()
            .rev() // Most recent first
            .skip(skip)
            .take(PER_PAGE as usize)
            .cloned()
            .collect();

        MessagePage {
            messages,
            page,
            total_pages,
            total_count,
            per_page: PER_PAGE,
        }
    }

    pub fn list_all(&self) -> Vec<Message> {
        self.inner.read().iter().rev().cloned().collect()
    }
}
