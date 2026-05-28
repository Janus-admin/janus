use crate::providers::ChatCompletionRequest;
use regex::RegexSet;

/// Detects prompts that are inherently time-bound (e.g. "current price", "today").
///
/// When a request matches, it is excluded from both cache lookup and cache write.
/// The set of patterns is configured via `time_sensitive_patterns` in janus.toml.
pub struct TimeGuard {
    patterns: RegexSet,
}

impl TimeGuard {
    /// Compile the configured pattern list into a `TimeGuard`.
    /// Invalid regex patterns are logged and skipped — the rest still apply.
    pub fn new(raw_patterns: &[String]) -> Self {
        let mut valid_patterns = Vec::new();
        for p in raw_patterns {
            match regex::Regex::new(p) {
                Ok(_) => {
                    valid_patterns.push(p.clone());
                }
                Err(e) => {
                    tracing::warn!(pattern = %p, error = %e, "Invalid time-sensitive pattern — skipping");
                }
            }
        }
        let patterns =
            RegexSet::new(valid_patterns).unwrap_or_else(|_| RegexSet::new(["a^"]).unwrap());
        Self { patterns }
    }

    /// Returns `true` if any message content in the request matches a time-sensitive pattern.
    pub fn is_time_sensitive(&self, request: &ChatCompletionRequest) -> bool {
        if self.patterns.is_empty() {
            return false;
        }
        for msg in &request.messages {
            let text = extract_text_content(&msg.content);
            if self.patterns.is_match(&text) {
                return true;
            }
        }
        false
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

/// Extract plain text from a message content value (string or content-block array).
fn extract_text_content(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter_map(|item| {
                if item["type"].as_str() == Some("text") {
                    item["text"].as_str().map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req_with_content(content: &str) -> ChatCompletionRequest {
        serde_json::from_value(json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": content }]
        }))
        .unwrap()
    }

    #[test]
    fn english_today_pattern_matches() {
        let guard = TimeGuard::new(&[r"\btoday\b".into()]);
        assert!(guard.is_time_sensitive(&req_with_content("What happened today?")));
    }

    #[test]
    fn non_time_sensitive_prompt_not_matched() {
        let guard = TimeGuard::new(&[r"\btoday\b".into()]);
        assert!(!guard.is_time_sensitive(&req_with_content("What is 2 + 2?")));
    }

    #[test]
    fn empty_pattern_list_never_matches() {
        let guard = TimeGuard::new(&[]);
        assert!(!guard.is_time_sensitive(&req_with_content("today right now currently")));
    }

    #[test]
    fn persian_pattern_matches() {
        let guard = TimeGuard::new(&["امروز".into()]);
        assert!(guard.is_time_sensitive(&req_with_content("امروز چه خبر است؟")));
    }
}
