use std::sync::Arc;
use std::time::Duration;
use nautilus::cache::BoundedCache;

#[test]
fn test_bounded_cache_basic_operations() {
    let cache = BoundedCache::new(5, Duration::from_secs(60));
    assert_eq!(cache.capacity(), 5);
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());

    cache.insert("key1".to_string(), 100);
    cache.insert("key2".to_string(), 200);

    assert_eq!(cache.len(), 2);
    assert!(!cache.is_empty());
    assert_eq!(cache.get("key1"), Some(100));
    assert_eq!(cache.get("key2"), Some(200));
    assert_eq!(cache.get("key3"), None);

    assert!(cache.contains_key("key1"));
    assert!(!cache.contains_key("key3"));

    assert_eq!(cache.remove("key1"), Some(100));
    assert_eq!(cache.get("key1"), None);
    assert_eq!(cache.len(), 1);

    cache.clear();
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

#[test]
fn test_bounded_cache_capacity_lru_eviction() {
    let cache = BoundedCache::new(3, Duration::from_secs(60));

    cache.insert("a".to_string(), 1);
    cache.insert("b".to_string(), 2);
    cache.insert("c".to_string(), 3);
    assert_eq!(cache.len(), 3);

    // Access "a" so it is more recently used than "b"
    assert_eq!(cache.get("a"), Some(1));

    // Insert "d", which should evict "b" (since "a" was accessed, and "c" was inserted after "b")
    cache.insert("d".to_string(), 4);
    assert_eq!(cache.len(), 3);

    assert_eq!(cache.get("a"), Some(1));
    assert_eq!(cache.get("b"), None, "least-recently used key 'b' should be evicted");
    assert_eq!(cache.get("c"), Some(3));
    assert_eq!(cache.get("d"), Some(4));
}

#[test]
fn test_bounded_cache_ttl_expiration() {
    // 50ms TTL
    let cache = BoundedCache::new(10, Duration::from_millis(50));

    cache.insert("temp".to_string(), "val");
    assert_eq!(cache.get("temp"), Some("val"));

    std::thread::sleep(Duration::from_millis(70));

    // Entry should have expired
    assert_eq!(cache.get("temp"), None);
    assert!(!cache.contains_key("temp"));

    // Prune expired
    let pruned = cache.prune_expired();
    assert_eq!(pruned, 1);
    assert_eq!(cache.len(), 0);
}

#[tokio::test]
async fn test_bounded_cache_concurrent_stress() {
    let cache = Arc::new(BoundedCache::new(20, Duration::from_secs(5)));
    let mut handles = Vec::new();

    // 20 concurrent tasks hammering cache
    for task_id in 0..20 {
        let c = Arc::clone(&cache);
        handles.push(tokio::spawn(async move {
            for i in 0..50 {
                let key = format!("k_{}", (task_id + i) % 15);
                c.insert(key.clone(), task_id * 100 + i);
                let _ = c.get(&key);
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert!(cache.len() <= cache.capacity(), "cache size must never exceed capacity");
}
