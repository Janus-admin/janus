use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use uuid::Uuid;

/// In-memory sliding window rate limiter, keyed by API key ID.
///
/// Each entry stores the millisecond timestamps of recent requests for that key.
/// On every `check_and_record` call, stale entries (outside the window) are
/// evicted, the count is compared to the limit, and — if within limit — the
/// current timestamp is appended.
pub struct RateLimiter {
    windows: DashMap<Uuid, VecDeque<i64>>,
    window_ms: i64,
}

impl RateLimiter {
    pub fn new(window_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            windows: DashMap::new(),
            window_ms: (window_secs * 1_000) as i64,
        })
    }

    /// Check the key against its RPM limit and record the request if allowed.
    ///
    /// Returns `Ok(())` if the request is within limits.
    /// Returns `Err(retry_after_secs)` — the number of seconds until the
    /// oldest in-window request expires — if the limit is exceeded.
    pub fn check_and_record(&self, key_id: Uuid, limit: i32) -> Result<(), u64> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let cutoff_ms = now_ms - self.window_ms;

        let mut entry = self.windows.entry(key_id).or_default();
        let deque = entry.value_mut();

        // Evict timestamps that have fallen outside the window.
        while let Some(&front) = deque.front() {
            if front < cutoff_ms {
                deque.pop_front();
            } else {
                break;
            }
        }

        if deque.len() >= limit as usize {
            // Return the number of seconds until the oldest entry expires.
            let oldest_ms = deque.front().copied().unwrap_or(now_ms);
            let wait_ms = (oldest_ms + self.window_ms - now_ms).max(0);
            let retry_after_secs = (wait_ms / 1_000 + 1) as u64;
            return Err(retry_after_secs);
        }

        deque.push_back(now_ms);
        Ok(())
    }
}
