use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct BotStats {
    boot_time: Instant,
    bot_username: String,
    total_workers: AtomicU64,
    active_workers: AtomicU64,
    processed_total: AtomicU64,
    processed_ok: AtomicU64,
    processed_fail: AtomicU64,
    active_jobs: AtomicU64,
    cache_hits: AtomicU64,
}

impl BotStats {
    pub fn new(bot_username: String) -> Self {
        Self {
            boot_time: Instant::now(),
            bot_username,
            total_workers: AtomicU64::new(0),
            active_workers: AtomicU64::new(0),
            processed_total: AtomicU64::new(0),
            processed_ok: AtomicU64::new(0),
            processed_fail: AtomicU64::new(0),
            active_jobs: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
        }
    }

    pub fn uptime(&self) -> Duration {
        self.boot_time.elapsed()
    }

    pub fn total_workers(&self) -> u64 {
        self.total_workers.load(Ordering::Relaxed)
    }

    pub fn set_total_workers(&self, count: u64) {
        self.total_workers.store(count, Ordering::Relaxed);
    }

    pub fn active_workers(&self) -> u64 {
        self.active_workers.load(Ordering::Relaxed)
    }

    pub fn incr_active_workers(&self) {
        self.active_workers.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decr_active_workers(&self) {
        let _ = self
            .active_workers
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |val| {
                Some(val.saturating_sub(1))
            });
    }

    pub fn record_success(&self) {
        self.processed_total.fetch_add(1, Ordering::Relaxed);
        self.processed_ok.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.processed_total.fetch_add(1, Ordering::Relaxed);
        self.processed_fail.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn incr_active_jobs(&self) {
        self.active_jobs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decr_active_jobs(&self) {
        let _ = self
            .active_jobs
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |val| {
                Some(val.saturating_sub(1))
            });
    }

    pub fn bot_username(&self) -> &str {
        &self.bot_username
    }
    pub fn processed_total(&self) -> u64 {
        self.processed_total.load(Ordering::Relaxed)
    }
    pub fn processed_ok(&self) -> u64 {
        self.processed_ok.load(Ordering::Relaxed)
    }
    pub fn processed_fail(&self) -> u64 {
        self.processed_fail.load(Ordering::Relaxed)
    }
    pub fn active_jobs(&self) -> u64 {
        self.active_jobs.load(Ordering::Relaxed)
    }
    pub fn cache_hits(&self) -> u64 {
        self.cache_hits.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_counters_should_increment_and_decrement_safely() {
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
}
