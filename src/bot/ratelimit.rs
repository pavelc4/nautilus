use std::time::Instant;

use dashmap::DashMap;

struct TokenBucket {
    tokens: u64,
    max_tokens: u64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: u64, refill_secs: u64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_per_sec: max_tokens as f64 / refill_secs as f64,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            let new_tokens = (elapsed * self.refill_per_sec) as u64;
            if new_tokens > 0 {
                self.tokens = (self.tokens + new_tokens).min(self.max_tokens);
                self.last_refill = Instant::now();
            }
        }
    }
}

pub struct RateLimiter {
    buckets: DashMap<i64, TokenBucket>,
    max_tokens: u64,
    refill_secs: u64,
}

impl RateLimiter {
    pub fn new(max_tokens: u64, refill_secs: u64) -> Self {
        Self {
            buckets: DashMap::new(),
            max_tokens,
            refill_secs,
        }
    }

    pub fn check(&self, user_id: i64) -> bool {
        let mut bucket = self
            .buckets
            .entry(user_id)
            .or_insert_with(|| TokenBucket::new(self.max_tokens, self.refill_secs));
        bucket.try_consume()
    }
}
