use std::sync::Arc;
use grammers_client::Client;
use grammers_session::types::PeerRef;
use libsql::params;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DumpRecord {
    pub cache_key: String,
    pub channel_id: i64,
    pub message_ids: Vec<i32>,
    pub kind: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub total_bytes: u64,
    pub dims: Option<(i32, i32)>,
    pub duration_secs: Option<u64>,
    pub created_at: i64,
}

pub struct DumpCdnCache {
    conn: Arc<Mutex<libsql::Connection>>,
}

impl DumpCdnCache {
    pub async fn open(path: &str) -> anyhow::Result<Self> {
        let parent = std::path::Path::new(path)
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid db path"))?;
        tokio::fs::create_dir_all(parent).await?;

        let db = libsql::Builder::new_local(path).build().await?;
        let conn = db.connect()?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS dump_cdn (
                cache_key TEXT PRIMARY KEY,
                channel_id INTEGER NOT NULL,
                message_ids TEXT NOT NULL,
                kind TEXT NOT NULL,
                title TEXT,
                description TEXT,
                total_bytes INTEGER NOT NULL,
                dims TEXT,
                duration_secs INTEGER,
                created_at INTEGER NOT NULL
            );",
            params![],
        )
        .await?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_dump_cdn_created ON dump_cdn(created_at);",
            params![],
        )
        .await?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn get(&self, cache_key: &str) -> anyhow::Result<Option<DumpRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT cache_key, channel_id, message_ids, kind, title, description, \
                 total_bytes, dims, duration_secs, created_at \
                 FROM dump_cdn WHERE cache_key = ?1 LIMIT 1",
            )
            .await?;

        let row_opt = match stmt.query_row(params![cache_key]).await {
            Ok(row) => Some(row),
            Err(libsql::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e.into()),
        };

        let row = match row_opt {
            Some(r) => r,
            None => return Ok(None),
        };

        let key: String = row.get(0)?;
        let channel_id: i64 = row.get(1)?;
        let msg_ids_raw: String = row.get(2)?;
        let kind: String = row.get(3)?;
        let title: Option<String> = row.get(4)?;
        let description: Option<String> = row.get(5)?;
        let total_bytes: i64 = row.get(6)?;
        let dims_raw: Option<String> = row.get(7)?;
        let duration_secs: Option<i64> = row.get(8)?;
        let created_at: i64 = row.get(9)?;

        let message_ids: Vec<i32> = serde_json::from_str(&msg_ids_raw).unwrap_or_default();
        let dims = dims_raw.and_then(|d| {
            let mut parts = d.split('x');
            let w = parts.next()?.parse().ok()?;
            let h = parts.next()?.parse().ok()?;
            Some((w, h))
        });

        Ok(Some(DumpRecord {
            cache_key: key,
            channel_id,
            message_ids,
            kind,
            title,
            description,
            total_bytes: total_bytes as u64,
            dims,
            duration_secs: duration_secs.map(|s| s as u64),
            created_at,
        }))
    }

    pub async fn insert(&self, record: &DumpRecord) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let msg_ids_raw = serde_json::to_string(&record.message_ids)?;
        let dims_raw = record.dims.map(|(w, h)| format!("{w}x{h}"));

        conn.execute(
            "INSERT OR REPLACE INTO dump_cdn (
                cache_key, channel_id, message_ids, kind, title, description,
                total_bytes, dims, duration_secs, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.cache_key.clone(),
                record.channel_id,
                msg_ids_raw,
                record.kind.clone(),
                record.title.clone(),
                record.description.clone(),
                record.total_bytes as i64,
                dims_raw,
                record.duration_secs.map(|s| s as i64),
                record.created_at,
            ],
        )
        .await?;

        Ok(())
    }

    pub async fn delete(&self, cache_key: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM dump_cdn WHERE cache_key = ?1",
            params![cache_key],
        )
        .await?;
        Ok(())
    }
}

pub struct DumpChannelManager {
    target: Option<String>,
    peer_cache: RwLock<Option<PeerRef>>,
}

impl DumpChannelManager {
    pub fn new(target: Option<String>) -> Self {
        Self {
            target,
            peer_cache: RwLock::new(None),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.target.is_some()
    }

    pub async fn get_peer(&self, client: &Client) -> anyhow::Result<Option<PeerRef>> {
        let target = match self.target.as_deref() {
            Some(t) if !t.is_empty() => t,
            _ => return Ok(None),
        };

        if let Some(pref) = *self.peer_cache.read().await {
            return Ok(Some(pref));
        }

        let mut lock = self.peer_cache.write().await;
        if let Some(pref) = *lock {
            return Ok(Some(pref));
        }

        if target.starts_with('@')
            || (!target.starts_with('-') && target.chars().any(|c| c.is_alphabetic()))
        {
            let username = target.trim_start_matches('@');
            if let Some(peer) = client.resolve_username(username).await? {
                if let Ok(Some(pref)) = peer.to_ref().await {
                    tracing::info!(username, ?pref, "resolved dump channel via username");
                    *lock = Some(pref);
                    return Ok(Some(pref));
                }
            }
        }

        if let Ok(id) = target.parse::<i64>() {
            if let Some(peer_id) = grammers_session::types::PeerId::from_bot_api_dialog_id(id) {
                let ambient = peer_id.to_ambient_ref();
                match client.resolve_peer(ambient).await {
                    Ok(peer) => {
                        if let Ok(Some(pref)) = peer.to_ref().await {
                            tracing::info!(id, ?pref, "resolved dump channel via numeric peer id");
                            *lock = Some(pref);
                            return Ok(Some(pref));
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            id,
                            error = %e,
                            "could not resolve dump channel ID yet (send a message or add bot to channel)"
                        );
                    }
                }
            }
        }

        Ok(None)
    }

    pub async fn observe_peer(&self, peer_ref: PeerRef) {
        if let Some(ref target) = self.target
            && let Ok(id) = target.parse::<i64>()
            && peer_ref.id.bot_api_dialog_id() == Some(id)
        {
            let mut lock = self.peer_cache.write().await;
            *lock = Some(peer_ref);
            tracing::info!(id, "learned dump channel PeerRef from live update");
        }
    }
}
