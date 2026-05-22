// tests/phase3_reliability.rs
// Phase 3 acceptance tests — Rate Limiting & Reliability.

mod common;

/// Exceeding rate limit must return 429 with Retry-After header.
#[tokio::test]
#[ignore = "Phase 3 not yet implemented"]
async fn phase3_rate_limit_returns_429_with_retry_after() {
    // Create a key with rate_limit_rpm = 2
    // Send 3 requests in quick succession
    // Third request must return 429
    // Response must include Retry-After header
}

/// After rate limit window resets, requests must succeed again.
#[tokio::test]
#[ignore = "Phase 3 not yet implemented"]
async fn phase3_rate_limit_resets_after_window() {
    // Wait for rate limit window to expire, then verify requests succeed
}

/// When primary provider returns 500, request must be retried.
#[tokio::test]
#[ignore = "Phase 3 not yet implemented"]
async fn phase3_provider_500_triggers_retry() {
    // Mock provider returning 500 on first call, 200 on second
    // Verify the response is successful (transparent retry)
}

/// When primary provider is down, secondary provider must be used automatically.
#[tokio::test]
#[ignore = "Phase 3 not yet implemented"]
async fn phase3_provider_failover_uses_next_priority() {
    // Mock primary provider as unavailable, secondary as healthy
    // Verify request succeeds and was served by secondary
}

/// When all providers are down, must return 503.
#[tokio::test]
#[ignore = "Phase 3 not yet implemented"]
async fn phase3_all_providers_down_returns_503() {
    // Mock all providers as unavailable
    // Verify 503 Service Unavailable is returned
}
