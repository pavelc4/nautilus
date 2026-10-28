use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::RwLock;
use std::time::{Duration, Instant};

struct CacheEntry<V> {
    value: V,
    expires_at: Instant,
    last_accessed: Instant,
}

struct CacheInner<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
}

/// A thread-safe in-memory cache bounded by maximum item capacity and time-to-live (TTL).
///
/// When capacity is reached, expired items are automatically pruned. If the cache is still
/// saturated, the least-recently accessed item (LRU) is evicted to maintain the bound.
pub struct BoundedCache<K, V> {
    capacity: usize,
    ttl: Duration,
    inner: RwLock<CacheInner<K, V>>,
}

impl<K, V> BoundedCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        assert!(capacity > 0, "cache capacity must be greater than zero");
        Self {
            capacity,
            ttl,
            inner: RwLock::new(CacheInner {
                entries: HashMap::with_capacity(capacity.min(1024)),
            }),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.entries.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retrieve a cloned value if the key exists and has not expired.
    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let now = Instant::now();

        // 1. Fast read path
        {
            let guard = self.inner.read().ok()?;
            let entry = guard.entries.get(key)?;
            if entry.expires_at <= now {
                return None;
            }
        }

        // 2. Touch last_accessed under write lock and return value
        let mut guard = self.inner.write().ok()?;
        let entry = guard.entries.get_mut(key)?;
        if entry.expires_at <= now {
            // Expired between read and write locks
            return None;
        }
        entry.last_accessed = now;
        Some(entry.value.clone())
    }

    /// Check if an unexpired key exists without updating access time.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let now = Instant::now();
        self.inner
            .read()
            .ok()
            .and_then(|g| g.entries.get(key).map(|e| e.expires_at > now))
            .unwrap_or(false)
    }

    /// Insert or update an entry, enforcing capacity and TTL.
    pub fn insert(&self, key: K, value: V) {
        let Ok(mut guard) = self.inner.write() else {
            return;
        };

        let now = Instant::now();
        let expires_at = now + self.ttl;

        if let Some(entry) = guard.entries.get_mut(&key) {
            entry.value = value;
            entry.expires_at = expires_at;
            entry.last_accessed = now;
            return;
        }

        // Evict expired items if capacity is full
        if guard.entries.len() >= self.capacity {
            guard.entries.retain(|_, v| v.expires_at > now);
        }

        // Evict least-recently-accessed entry if still full
        if guard.entries.len() >= self.capacity {
            let oldest_key = guard
                .entries
                .iter()
                .min_by_key(|(_, v)| v.last_accessed)
                .map(|(k, _)| k.clone());

            if let Some(k) = oldest_key {
                guard.entries.remove(&k);
            }
        }

        guard.entries.insert(
            key,
            CacheEntry {
                value,
                expires_at,
                last_accessed: now,
            },
        );
    }

    /// Explicitly remove an entry by key.
    pub fn remove<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut guard = self.inner.write().ok()?;
        guard.entries.remove(key).map(|e| e.value)
    }

    /// Prune all expired entries from memory, returning the number of pruned items.
    pub fn prune_expired(&self) -> usize {
        let Ok(mut guard) = self.inner.write() else {
            return 0;
        };
        let now = Instant::now();
        let before = guard.entries.len();
        guard.entries.retain(|_, v| v.expires_at > now);
        before.saturating_sub(guard.entries.len())
    }

    /// Clear all items from the cache.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.entries.clear();
        }
    }
}
