//! Best-effort PII redaction for log and audit-cache bodies.
//!
//! ## Scope and intent
//!
//! This scrubber is a **safety net**, not a compliance tool.  It catches the
//! most common operator-visible secrets (credit cards, SSNs, emails, bearer
//! tokens, API keys, phone numbers, IPv4 addresses) before request bodies are
//! written to `cache_entries` or emitted via `tracing::debug!`.  It will miss:
//!
//! - International ID formats (passports, national IDs outside US SSN)
//! - Free-form addresses, full names
//! - Custom secret formats specific to your provider
//! - Anything obfuscated, base64-encoded, or split across tokens
//!
//! For stronger guarantees use the `PiiRedactionPlugin` (request-time pipeline
//! plugin) or run a dedicated DLP service in front of the gateway.
//!
//! ## Regex limitations
//!
//! Patterns are intentionally conservative on false positives:
//! - Credit-card matches are validated against the **Luhn checksum** before
//!   being redacted, so 16-digit order numbers / invoice IDs are left alone.
//! - IPv4 patterns will (correctly) redact valid IP addresses, but may
//!   over-redact dotted version strings like `1.2.3.4` — accepted tradeoff
//!   on the safe side.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::{Captures, Regex};

struct Patterns {
    credit_card: Regex,
    amex: Regex,
    ssn: Regex,
    email: Regex,
    bearer: Regex,
    api_key: Regex,
    phone_intl: Regex,
    phone_na: Regex,
    ipv4: Regex,
}

static PATTERNS: OnceLock<Patterns> = OnceLock::new();

fn patterns() -> &'static Patterns {
    PATTERNS.get_or_init(|| Patterns {
        // 16-digit card numbers with optional spaces or dashes between groups.
        // Match is only redacted if it passes the Luhn checksum (see `luhn`).
        credit_card: Regex::new(r"\b(?:\d{4}[- ]?){3}\d{4}\b").unwrap(),
        // 15-digit American Express (starts with 34 or 37), optionally
        // separator-formatted as 4-6-5.
        amex: Regex::new(r"\b3[47]\d{2}[- ]?\d{6}[- ]?\d{5}\b").unwrap(),
        // US Social Security Numbers.
        ssn: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(),
        // Email addresses.
        email: Regex::new(r"\b[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}\b").unwrap(),
        // Bearer tokens in Authorization headers.
        bearer: Regex::new(r"(?i)Bearer\s+[A-Za-z0-9+/=._\-]{20,}").unwrap(),
        // Generic API key patterns (sk-..., pk-..., key=...).
        api_key: Regex::new(r"\b(?:sk|pk|key|token|secret)-[A-Za-z0-9_\-]{20,}\b").unwrap(),
        // International phone in roughly-E.164 form: leading + and 8–15 digits,
        // optional space/dash/dot separators between any pair of digits.
        // Accepts both "+1 555 123 4567" and "+33-1-23-45-67-89".
        phone_intl: Regex::new(r"\+[1-9]\d{0,3}(?:[- .]?\d){7,14}").unwrap(),
        // North-American phone: (NNN) NNN-NNNN, NNN-NNN-NNNN, NNN.NNN.NNNN.
        // The leading `\b` is intentionally only on the bare-digit branch —
        // `\b` doesn't anchor against `(`, since both are non-word characters.
        phone_na: Regex::new(r"(?:\(\d{3}\)\s?|\b\d{3}[- .])\d{3}[- .]\d{4}\b").unwrap(),
        // IPv4 address (each octet validated 0–255).
        ipv4: Regex::new(
            r"\b(?:25[0-5]|2[0-4]\d|1?\d?\d)\.(?:25[0-5]|2[0-4]\d|1?\d?\d)\.(?:25[0-5]|2[0-4]\d|1?\d?\d)\.(?:25[0-5]|2[0-4]\d|1?\d?\d)\b",
        )
        .unwrap(),
    })
}

/// Luhn checksum validator. Returns true for valid credit-card numbers.
///
/// Strips non-digits before computing the checksum so the caller can pass the
/// raw regex match (which may contain spaces or dashes) directly.
fn luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                d
            }
        })
        .sum();
    sum % 10 == 0
}

/// Redact only matches that pass `predicate`; leave others as-is.
fn replace_if<'a, F>(re: &Regex, input: &'a str, redaction: &str, predicate: F) -> Cow<'a, str>
where
    F: Fn(&str) -> bool,
{
    re.replace_all(input, |caps: &Captures<'_>| {
        let m = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        if predicate(m) {
            redaction.to_string()
        } else {
            m.to_string()
        }
    })
}

/// Scrub common PII patterns from a string and return the sanitized version.
///
/// See module-level docs for the full scope and limitations.
pub fn scrub(input: &str) -> Cow<'_, str> {
    let p = patterns();

    // Order matters: more-specific patterns first so they consume their text
    // before looser patterns get a chance to match overlapping spans.
    let s = replace_if(&p.credit_card, input, "[CC-REDACTED]", luhn);
    let s = replace_if(&p.amex, &s, "[CC-REDACTED]", luhn);
    let s = p.ssn.replace_all(&s, "[SSN-REDACTED]");
    let s = p.email.replace_all(&s, "[EMAIL-REDACTED]");
    let s = p.bearer.replace_all(&s, "Bearer [TOKEN-REDACTED]");
    let s = p.api_key.replace_all(&s, "[KEY-REDACTED]");
    let s = p.phone_intl.replace_all(&s, "[PHONE-REDACTED]");
    let s = p.phone_na.replace_all(&s, "[PHONE-REDACTED]");
    let s = p.ipv4.replace_all(&s, "[IP-REDACTED]");

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
        // 4111 1111 1111 1111 is a well-known Visa test number that passes Luhn.
        let input = "Card number: 4111 1111 1111 1111 thanks";
        assert!(scrub(input).contains("[CC-REDACTED]"));
        assert!(!scrub(input).contains("4111"));
    }

    #[test]
    fn luhn_rejects_fake_credit_card() {
        // 16-digit invoice / order numbers that happen to look like cards must
        // NOT be redacted.  1234 5678 9012 3456 fails Luhn.
        let input = "Order #1234 5678 9012 3456 confirmed";
        let out = scrub(input);
        assert!(!out.contains("[CC-REDACTED]"), "expected order number to pass through unchanged, got: {out}");
        assert!(out.contains("1234"));
    }

    #[test]
    fn redacts_amex() {
        // 378282246310005 is a documented Amex test number that passes Luhn.
        let input = "Amex: 3782 822463 10005";
        let out = scrub(input);
        assert!(out.contains("[CC-REDACTED]"), "expected redaction, got: {out}");
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
    fn redacts_phone_na() {
        for phone in ["(555) 123-4567", "555-123-4567", "555.123.4567"] {
            let input = format!("Call me at {phone} tomorrow");
            let out = scrub(&input);
            assert!(out.contains("[PHONE-REDACTED]"), "expected redaction for {phone}, got: {out}");
            assert!(!out.contains(phone), "raw phone leaked for {phone}: {out}");
        }
    }

    #[test]
    fn redacts_phone_intl() {
        for phone in ["+1 555 123 4567", "+44 20 7946 0958", "+33-1-23-45-67-89"] {
            let input = format!("Reach me on {phone} please");
            let out = scrub(&input);
            assert!(out.contains("[PHONE-REDACTED]"), "expected redaction for {phone}, got: {out}");
        }
    }

    #[test]
    fn redacts_ipv4() {
        let input = "Connect from 192.168.1.42 or 10.0.0.255";
        let out = scrub(input);
        assert!(out.contains("[IP-REDACTED]"));
        assert!(!out.contains("192.168.1.42"));
        assert!(!out.contains("10.0.0.255"));
    }

    #[test]
    fn ipv4_rejects_out_of_range_octets() {
        // 999.999.999.999 is not a valid IP; the octet alternation caps at 255.
        let input = "Build version 999.999.999.999";
        let out = scrub(input);
        assert!(!out.contains("[IP-REDACTED]"), "out-of-range octets should not match, got: {out}");
    }

    #[test]
    fn clean_text_unchanged() {
        let input = "Hello world, the answer is 42.";
        assert_eq!(scrub(input).as_ref(), input);
    }
}
