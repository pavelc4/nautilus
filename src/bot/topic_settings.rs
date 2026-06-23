use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
extern crate serde_json;

#[derive(Clone, Default)]
pub struct TopicSettings {
    // Map of group chat_id to a list of whitelisted topic IDs
    pub whitelisted_topics: Arc<RwLock<HashMap<i64, Vec<i32>>>>,
}

impl TopicSettings {
    pub async fn load() -> Self {
        let path = Self::path();
        if let Ok(data) = tokio::fs::read_to_string(&path).await {
            if let Ok(map) = serde_json::from_str::<HashMap<i64, Vec<i32>>>(&data) {
                return Self {
                    whitelisted_topics: Arc::new(RwLock::new(map)),
                };
            }
        }
        Self::default()
    }

    pub async fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let map = self.whitelisted_topics.read().await;
        let serialized = serde_json::to_string_pretty(&*map)?;
        tokio::fs::write(&path, serialized).await?;
        Ok(())
    }

    fn path() -> String {
        std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into()) + "/topics.json"
    }

    pub async fn is_topic_allowed(&self, chat_id: i64, topic_id: Option<i32>) -> bool {
        let map = self.whitelisted_topics.read().await;
        if let Some(allowed) = map.get(&chat_id) {
            if allowed.is_empty() {
                // Empty vector means any topic is allowed
                true
            } else {
                // Must be in one of the allowed topics
                match topic_id {
                    Some(tid) => allowed.contains(&tid),
                    None => false, // Topic is restricted, but message is not in any topic (main thread)
                }
            }
        } else {
            // No restriction set for this group
            true
        }
    }

    pub async fn set_allowed_topics(&self, chat_id: i64, topics: Vec<i32>) -> anyhow::Result<()> {
        {
            let mut map = self.whitelisted_topics.write().await;
            if topics.is_empty() {
                map.remove(&chat_id);
            } else {
                map.insert(chat_id, topics);
            }
        }
        self.save().await
    }
}

pub fn get_message_topic_id(msg: &grammers_client::update::Message) -> Option<i32> {
    match &std::ops::Deref::deref(msg).raw {
        grammers_client::tl::enums::Message::Message(raw_msg) => match &raw_msg.reply_to {
            Some(grammers_client::tl::enums::MessageReplyHeader::Header(header)) => {
                if header.forum_topic {
                    header.reply_to_top_id.or(header.reply_to_msg_id)
                } else {
                    None
                }
            }
            _ => None,
        },
        _ => None,
    }
}
