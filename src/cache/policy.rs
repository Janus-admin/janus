/// Controls which requests are eligible for semantic cache lookup.
///
/// Checked in the gateway handler before calling `cache.semantic_lookup()`.
/// An empty `models` list means all models are allowed.
#[derive(Debug, Clone, Default)]
pub struct SemanticCachePolicy {
    /// If non-empty, only requests for these model IDs use semantic cache.
    pub models: Vec<String>,
    /// Route prefixes excluded from semantic cache (e.g. "/v1/embeddings").
    pub exclude_routes: Vec<String>,
    /// API key names excluded from semantic cache.
    pub exclude_keys: Vec<String>,
}

impl SemanticCachePolicy {
    pub fn new(
        models: Vec<String>,
        exclude_routes: Vec<String>,
        exclude_keys: Vec<String>,
    ) -> Self {
        Self {
            models,
            exclude_routes,
            exclude_keys,
        }
    }

    /// Returns `true` when the given model/route/key combination may use semantic cache.
    pub fn allows(&self, model: &str, route: &str, api_key_name: &str) -> bool {
        // Non-empty allowlist: model must be listed.
        if !self.models.is_empty() && !self.models.iter().any(|m| m == model) {
            return false;
        }
        // Any matching exclude-route prefix blocks semantic cache.
        if self
            .exclude_routes
            .iter()
            .any(|prefix| route.starts_with(prefix.as_str()))
        {
            return false;
        }
        // Explicitly excluded API key names.
        if self.exclude_keys.iter().any(|k| k == api_key_name) {
            return false;
        }
        true
    }
}
