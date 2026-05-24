pub mod cost;
pub mod latency;
pub mod round_robin;

/// How a request should be routed to a provider.
/// `Priority` is the original behavior and remains the default for all existing keys.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RoutingStrategy {
    /// Pick the highest-priority enabled provider (original behavior).
    #[default]
    Priority,
    /// Pick the provider with the lowest total token cost for the requested model.
    CostOptimized,
    /// Pick the provider with the lowest 15-minute p95 latency.
    LatencyOptimized,
    /// Distribute requests evenly across all enabled providers via atomic counter.
    RoundRobin,
}

impl RoutingStrategy {
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "cost" => Self::CostOptimized,
            "latency" => Self::LatencyOptimized,
            "round_robin" => Self::RoundRobin,
            _ => Self::Priority,
        }
    }

    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Priority => "priority",
            Self::CostOptimized => "cost",
            Self::LatencyOptimized => "latency",
            Self::RoundRobin => "round_robin",
        }
    }
}
