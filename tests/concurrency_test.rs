use nautilus::pool::WorkerPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_worker_pool_immediate_acquire_and_release() {
    let pool = WorkerPool::new(3);
    assert_eq!(pool.total_slots(), 3);
    assert_eq!(pool.active_workers(), 0);
    assert_eq!(pool.available_permits(), 3);

    let p1 = pool.try_acquire().expect("slot 1 should be available");
    assert_eq!(pool.active_workers(), 1);
    assert_eq!(pool.available_permits(), 2);

    let p2 = pool.try_acquire().expect("slot 2 should be available");
    assert_eq!(pool.active_workers(), 2);
    assert_eq!(pool.available_permits(), 1);

    drop(p1);
    assert_eq!(pool.active_workers(), 1);
    assert_eq!(pool.available_permits(), 2);

    drop(p2);
    assert_eq!(pool.active_workers(), 0);
    assert_eq!(pool.available_permits(), 3);
}

#[tokio::test]
async fn test_worker_pool_saturation_and_queue() {
    let pool = WorkerPool::new(2);

    let p1 = pool.try_acquire().expect("slot 1");
    let p2 = pool.try_acquire().expect("slot 2");

    assert!(pool.try_acquire().is_none(), "pool should be saturated");
    assert_eq!(pool.active_workers(), 2);

    let queued_called = Arc::new(AtomicUsize::new(0));
    let q_clone = queued_called.clone();
    let pool_clone = pool.clone();

    let task = tokio::spawn(async move {
        let _permit = pool_clone
            .acquire(|pos| {
                q_clone.store(pos, Ordering::SeqCst);
            })
            .await
            .expect("should acquire once released");
        // Hold briefly
        tokio::time::sleep(Duration::from_millis(10)).await;
    });

    // Wait a brief tick for queue callback to trigger
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(queued_called.load(Ordering::SeqCst), 1, "task must be queued at position 1");
    assert_eq!(pool.queued_tasks(), 1);

    // Drop one permit, task should acquire immediately
    drop(p1);

    task.await.expect("task completed");
    assert_eq!(pool.queued_tasks(), 0);
    assert_eq!(pool.active_workers(), 1); // p2 still alive

    drop(p2);
    assert_eq!(pool.active_workers(), 0);
    assert_eq!(pool.available_permits(), 2);
}

#[tokio::test]
async fn test_worker_pool_stress_concurrent_tasks() {
    let pool = Arc::new(WorkerPool::new(4));
    let counter = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for _ in 0..50 {
        let pool = pool.clone();
        let counter = counter.clone();
        handles.push(tokio::spawn(async move {
            let _permit = pool.acquire(|_| {}).await.expect("acquire permit");
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }));
    }

    for h in handles {
        h.await.expect("task finished cleanly");
    }

    assert_eq!(counter.load(Ordering::SeqCst), 50);
    assert_eq!(pool.active_workers(), 0);
    assert_eq!(pool.queued_tasks(), 0);
    assert_eq!(pool.available_permits(), 4);
}

#[tokio::test]
async fn test_worker_pool_panic_safety() {
    let pool = Arc::new(WorkerPool::new(1));
    let pool_clone = pool.clone();

    let result = tokio::spawn(async move {
        let _permit = pool_clone.acquire(|_| {}).await.expect("acquired");
        panic!("simulated worker panic inside job");
    })
    .await;

    assert!(result.is_err(), "task should have panicked");
    // Verify slot was not leaked
    assert_eq!(pool.active_workers(), 0);
    assert_eq!(pool.available_permits(), 1);

    // Next task must be able to acquire smoothly
    let p = pool.try_acquire();
    assert!(p.is_some(), "slot must be reusable after panic");
}
