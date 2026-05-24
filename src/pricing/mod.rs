use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

/// Compute cost in USD for a single request.
///
/// Formula: (prompt_tokens / 1_000_000) * input_price
///        + (completion_tokens / 1_000_000) * output_price
///
/// `input_price` and `output_price` are per-1M-token rates from `model_pricing`.
pub fn calculate_cost(
    prompt_tokens: u32,
    completion_tokens: u32,
    input_per_1m: Decimal,
    output_per_1m: Decimal,
) -> Decimal {
    let million = Decimal::from(1_000_000u32);

    let prompt_cost = Decimal::from(prompt_tokens) / million * input_per_1m;
    let completion_cost = Decimal::from(completion_tokens) / million * output_per_1m;

    prompt_cost + completion_cost
}

/// Like `calculate_cost` but accepts raw f64 token counts (e.g. from JSON).
/// Returns None if conversion fails.
pub fn calculate_cost_f64(
    prompt_tokens: f64,
    completion_tokens: f64,
    input_per_1m: Decimal,
    output_per_1m: Decimal,
) -> Option<Decimal> {
    let p = Decimal::from_f64(prompt_tokens)?;
    let c = Decimal::from_f64(completion_tokens)?;
    let million = Decimal::from(1_000_000u32);
    Some((p / million * input_per_1m) + (c / million * output_per_1m))
}

// ── V5-0 modality cost helpers ────────────────────────────────────────────────

/// Image generation cost: flat per-image fee × image count.
pub fn calculate_image_cost(image_count: u32, price_per_image: Decimal) -> Decimal {
    Decimal::from(image_count) * price_per_image
}

/// Audio transcription cost: per-second rate × duration in seconds.
/// `duration_seconds` is a float because providers return fractional durations
/// (e.g. 12.34s); we round-trip through Decimal::from_f64 for precision.
pub fn calculate_audio_cost(duration_seconds: f64, price_per_second: Decimal) -> Option<Decimal> {
    let d = Decimal::from_f64(duration_seconds)?;
    Some(d * price_per_second)
}

/// Speech synthesis cost: per-character rate × character count.
pub fn calculate_character_cost(char_count: u32, price_per_character: Decimal) -> Decimal {
    Decimal::from(char_count) * price_per_character
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn test_calculate_cost_zero_tokens() {
        let cost = calculate_cost(0, 0, d("15.00"), d("60.00"));
        assert_eq!(cost, Decimal::ZERO);
    }

    #[test]
    fn test_calculate_cost_one_million_tokens() {
        let cost = calculate_cost(1_000_000, 0, d("15.00"), d("60.00"));
        assert_eq!(cost, d("15.00"));
    }

    #[test]
    fn test_calculate_cost_mixed() {
        // 1000 input @ $15/1M + 500 output @ $60/1M
        // = 0.015 + 0.030 = 0.045
        let cost = calculate_cost(1_000, 500, d("15.00"), d("60.00"));
        assert_eq!(cost, d("0.045"));
    }
}
