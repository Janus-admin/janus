use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use uuid::Uuid;

/// In-memory sliding window rate limiter, keyed by API key ID.
///
/// Two windows per key: one for request count (RPM) and one for token volume (TPM).
/// Stale entries outside the window are evicted on every check.
pub struct RateLimiter {
    windows: DashMap<Uuid, VecDeque<i64>>,
    /// Stores (timestamp_ms, token_count) pairs for TPM tracking.
    tpm_windows: DashMap<Uuid, VecDeque<(i64, i64)>>,
    window_ms: i64,
}

impl RateLimiter {
    pub fn new(window_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            windows: DashMap::new(),
            tpm_windows: DashMap::new(),
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

    /// Check and record an estimated token count against the TPM limit.
    ///
    /// Returns `Ok(())` if within the limit, `Err(retry_after_secs)` if exceeded.
    pub fn check_and_record_tokens(
        &self,
        key_id: Uuid,
        tokens: i64,
        limit: i32,
    ) -> Result<(), u64> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let cutoff_ms = now_ms - self.window_ms;

        let mut entry = self.tpm_windows.entry(key_id).or_default();
        let deque = entry.value_mut();

        while let Some(&(ts, _)) = deque.front() {
            if ts < cutoff_ms {
                deque.pop_front();
            } else {
                break;
            }
        }

        let used: i64 = deque.iter().map(|(_, t)| t).sum();
        if used + tokens > limit as i64 {
            let oldest_ms = deque.front().map(|(ts, _)| *ts).unwrap_or(now_ms);
            let wait_ms = (oldest_ms + self.window_ms - now_ms).max(0);
            return Err((wait_ms / 1_000 + 1) as u64);
        }

        deque.push_back((now_ms, tokens));
        Ok(())
    }
}
