use nautilus::pool::{FlightStatus, SingleFlight};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_singleflight_concurrent_deduplication() {
    let sf = Arc::new(SingleFlight::<String>::new());
    let leaders_count = Arc::new(AtomicUsize::new(0));
    let followers_count = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for _ in 0..20 {
        let sf = sf.clone();
        let leaders = leaders_count.clone();
        let followers = followers_count.clone();

        handles.push(tokio::spawn(async move {
            match sf.wait_or_lead("https://x.com/test/status/1").await {
                Ok(FlightStatus::Leader(leader)) => {
                    leaders.fetch_add(1, Ordering::SeqCst);
                    // Simulate doing heavy work
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    leader.complete("media_file_id_123".to_string());
                    "media_file_id_123".to_string()
                }
                Ok(FlightStatus::Follower(res)) => {
                    followers.fetch_add(1, Ordering::SeqCst);
                    res
                }
                Err(e) => panic!("unexpected error: {e}"),
            }
        }));
    }

    for h in handles {
        let res = h.await.expect("task completed cleanly");
        assert_eq!(res, "media_file_id_123");
    }

    assert_eq!(leaders_count.load(Ordering::SeqCst), 1, "exactly one task must lead");
    assert_eq!(followers_count.load(Ordering::SeqCst), 19, "all other 19 must follow");
    assert_eq!(sf.active_flights(), 0, "no flights should remain active");
}

#[tokio::test]
async fn test_singleflight_cancellation_safety() {
    let sf = Arc::new(SingleFlight::<String>::new());

    // Spawn a leader that drops prematurely (e.g. simulated network abort)
    let sf_clone = sf.clone();
    let leader_handle = tokio::spawn(async move {
        let status = sf_clone.wait_or_lead("cancel_key").await.unwrap();
        if let FlightStatus::Leader(_leader) = status {
            // Drop without calling .complete()
        }
    });

    leader_handle.await.unwrap();

    // Verify key was removed upon drop
    assert_eq!(sf.active_flights(), 0);

    // New request should smoothly become leader
    let status2 = sf.wait_or_lead("cancel_key").await.unwrap();
    match status2 {
        FlightStatus::Leader(leader) => {
            leader.complete("success".to_string());
        }
        FlightStatus::Follower(_) => panic!("should be leader after previous dropped"),
    }
}

#[tokio::test]
async fn test_singleflight_independent_keys() {
    let sf = SingleFlight::<String>::new();

    let s1 = sf.wait_or_lead("key1").await.unwrap();
    let s2 = sf.wait_or_lead("key2").await.unwrap();

    assert!(matches!(s1, FlightStatus::Leader(_)));
    assert!(matches!(s2, FlightStatus::Leader(_)));
    assert_eq!(sf.active_flights(), 2);
}
