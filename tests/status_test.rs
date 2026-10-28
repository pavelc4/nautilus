use nautilus::bot::status::BotStats;

#[test]
fn test_worker_counters_should_increment_and_decrement_safely() {
    let stats = BotStats::new("test_bot".to_string());
    assert_eq!(stats.total_workers(), 0);
    assert_eq!(stats.active_workers(), 0);

    stats.set_total_workers(8);
    assert_eq!(stats.total_workers(), 8);

    stats.incr_active_workers();
    stats.incr_active_workers();
    assert_eq!(stats.active_workers(), 2);

    stats.decr_active_workers();
    assert_eq!(stats.active_workers(), 1);

    stats.decr_active_workers();
    assert_eq!(stats.active_workers(), 0);

    // Underflow protection
    stats.decr_active_workers();
    assert_eq!(stats.active_workers(), 0);
}
