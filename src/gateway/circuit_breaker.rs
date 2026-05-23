use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq)]
enum State {
    Closed,
    Open { since: Instant },
    HalfOpen,
}

/// Three-state circuit breaker per provider.
///
/// Closed  → normal operation; failures increment the counter.
/// Open    → all calls skipped until `recovery_timeout` elapses.
/// HalfOpen → one probe allowed; success closes, failure re-opens.
pub struct CircuitBreaker {
    state: Mutex<State>,
    consecutive_failures: Mutex<u32>,
    failure_threshold: u32,
    recovery_timeout: Duration,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_timeout_secs: u64) -> Self {
        Self {
            state: Mutex::new(State::Closed),
            consecutive_failures: Mutex::new(0),
            failure_threshold,
            recovery_timeout: Duration::from_secs(recovery_timeout_secs),
        }
    }

    /// Returns `true` if this provider should be skipped right now.
    pub fn is_open(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if let State::Open { since } = *state {
            if since.elapsed() >= self.recovery_timeout {
                *state = State::HalfOpen;
                return false;
            }
            return true;
        }
        false
    }

    pub fn record_success(&self) {
        let mut state = self.state.lock().unwrap();
        *state = State::Closed;
        *self.consecutive_failures.lock().unwrap() = 0;
    }

    pub fn record_failure(&self) {
        let mut failures = self.consecutive_failures.lock().unwrap();
        *failures += 1;
        if *failures >= self.failure_threshold {
            *self.state.lock().unwrap() = State::Open { since: Instant::now() };
            *failures = 0;
        }
    }
}
