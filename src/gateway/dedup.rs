use crate::providers::ChatCompletionResponse;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Seconds a waiter will block before giving up and returning an error.
/// Keeps the pipeline moving even if the primary provider call hangs.
const WAITER_TIMEOUT_SECS: u64 = 30;

/// Tracks in-flight non-streaming requests by their SHA-256 cache hash.
///
/// When N identical requests arrive concurrently, exactly one proceeds to the
/// provider (the "primary"). All others subscribe to a broadcast channel and
/// receive the primary's result when it completes.
///
/// Dedup is skipped for streaming requests — SSE cannot be broadcast across
/// multiple HTTP connections.
pub struct InFlightDeduplicator {
    in_flight: DashMap<String, broadcast::Sender<Arc<DeduplicatedResult>>>,
}

pub enum DeduplicatedResult {
    Response(ChatCompletionResponse),
    Error(String),
}

/// Return value of `register_or_subscribe`.
pub enum DedupRole {
    /// Caller is the primary — proceed to call the provider.
    Primary,
    /// Caller is a duplicate — wait on this receiver for the primary's result.
    Waiter(broadcast::Receiver<Arc<DeduplicatedResult>>),
}

impl InFlightDeduplicator {
    pub fn new() -> Self {
        Self {
            in_flight: DashMap::new(),
        }
    }

    /// Atomically register as primary or subscribe as a waiter.
    ///
    /// Returns `DedupRole::Primary` if no request for this hash is in-flight.
    /// Returns `DedupRole::Waiter(rx)` if one is already in-flight.
    pub fn register_or_subscribe(&self, hash: &str) -> DedupRole {
        use dashmap::mapref::entry::Entry;
        match self.in_flight.entry(hash.to_string()) {
            Entry::Occupied(entry) => DedupRole::Waiter(entry.get().subscribe()),
            Entry::Vacant(entry) => {
                // Buffer size 2: we broadcast exactly one message, small headroom for races.
                let (tx, _) = broadcast::channel(2);
                entry.insert(tx);
                DedupRole::Primary
            }
        }
    }

    /// Called by the primary to broadcast its result to all current waiters.
    /// Must be called before `release` so that the in-memory cache is already
    /// warm when the slot is freed (new arrivals after release get a cache hit).
    pub fn broadcast_result(&self, hash: &str, result: Arc<DeduplicatedResult>) {
        if let Some(tx) = self.in_flight.get(hash) {
            let _ = tx.send(result);
        }
    }

    /// Remove the in-flight entry. After this, new requests for the same hash
    /// register as primaries (they will get exact cache hits anyway).
    pub fn release(&self, hash: &str) {
        self.in_flight.remove(hash);
    }

    /// Number of currently tracked in-flight requests. Intended for tests.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// How long (in seconds) a waiter blocks before timing out.
    pub fn waiter_timeout_secs() -> u64 {
        WAITER_TIMEOUT_SECS
    }
}

impl Default for InFlightDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}
