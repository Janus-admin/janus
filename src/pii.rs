use std::borrow::Cow;
use std::sync::OnceLock;

use regex::Regex;

struct Patterns {
    credit_card: Regex,
    ssn: Regex,
    email: Regex,
    bearer: Regex,
    api_key: Regex,
}

static PATTERNS: OnceLock<Patterns> = OnceLock::new();

fn patterns() -> &'static Patterns {
    PATTERNS.get_or_init(|| Patterns {
        // 16-digit card numbers with optional spaces or dashes between groups
        credit_card: Regex::new(r"\b(?:\d{4}[- ]?){3}\d{4}\b").unwrap(),
        // US Social Security Numbers
        ssn: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(),
        // Email addresses
        email: Regex::new(r"\b[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}\b").unwrap(),
        // Bearer tokens in Authorization headers
        bearer: Regex::new(r"(?i)Bearer\s+[A-Za-z0-9+/=._\-]{20,}").unwrap(),
        // Generic API key patterns (sk-..., pk-..., key=...)
        api_key: Regex::new(r"\b(?:sk|pk|key|token|secret)-[A-Za-z0-9_\-]{20,}\b").unwrap(),
    })
}

/// Scrub common PII patterns from a string and return the sanitized version.
///
/// Redacts: credit card numbers, SSNs, email addresses, bearer tokens, API keys.
/// Safe to call on any string including JSON bodies.
pub fn scrub(input: &str) -> Cow<'_, str> {
    let p = patterns();

    // Apply each substitution in sequence. We use Cow to avoid allocating when
    // none of the patterns match (the common path).
    let s = p.credit_card.replace_all(input, "[CC-REDACTED]");
    let s = p.ssn.replace_all(&s, "[SSN-REDACTED]");
    let s = p.email.replace_all(&s, "[EMAIL-REDACTED]");
    let s = p.bearer.replace_all(&s, "Bearer [TOKEN-REDACTED]");
    let s = p.api_key.replace_all(&s, "[KEY-REDACTED]");

    // Cow chain: if no replacements happened we get back &str borrows all the
    // way, avoiding any allocation. If any replacement happened we get a String.
    match s {
        Cow::Borrowed(b) if std::ptr::eq(b, input) => Cow::Borrowed(input),
        other => Cow::Owned(other.into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credit_card() {
        let input = "Card number: 4111 1111 1111 1111 thanks";
        assert!(scrub(input).contains("[CC-REDACTED]"));
        assert!(!scrub(input).contains("4111"));
    }

    #[test]
    fn redacts_ssn() {
        let input = "My SSN is 123-45-6789.";
        assert!(scrub(input).contains("[SSN-REDACTED]"));
        assert!(!scrub(input).contains("123-45-6789"));
    }

    #[test]
    fn redacts_email() {
        let input = "Contact me at alice@example.com for details.";
        assert!(scrub(input).contains("[EMAIL-REDACTED]"));
        assert!(!scrub(input).contains("alice@example.com"));
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload";
        assert!(scrub(input).contains("[TOKEN-REDACTED]"));
    }

    #[test]
    fn clean_text_unchanged() {
        let input = "Hello world, the answer is 42.";
        assert_eq!(scrub(input).as_ref(), input);
    }
}
