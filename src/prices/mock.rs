use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;

use super::{PriceSource, Quote};

/// Deterministic price source for tests only.
///
/// This is deliberately not reachable from the CLI: every real run uses live
/// prices. It exists so the test suite stays offline, fast and reproducible.
pub struct MockPriceSource {
    /// (input_mint, output_mint) -> output per 1.0 input unit, scaled.
    rates: HashMap<(String, String), f64>,
    slippage_bps: u16,
}

impl MockPriceSource {
    pub fn new(slippage_bps: u16) -> Self {
        Self {
            rates: HashMap::new(),
            slippage_bps,
        }
    }

    /// Set the conversion rate applied to raw base-unit amounts.
    pub fn set_rate(&mut self, input_mint: &str, output_mint: &str, rate: f64) -> &mut Self {
        self.rates
            .insert((input_mint.to_string(), output_mint.to_string()), rate);
        self
    }
}

#[async_trait]
impl PriceSource for MockPriceSource {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn quote(&self, input_mint: &str, output_mint: &str, amount: u64) -> Result<Quote> {
        let rate = self
            .rates
            .get(&(input_mint.to_string(), output_mint.to_string()))
            .copied()
            .ok_or_else(|| anyhow!("no mock rate configured for {} -> {}", input_mint, output_mint))?;

        let out_amount = (amount as f64 * rate).round() as u64;
        let threshold =
            (out_amount as f64 * (1.0 - self.slippage_bps as f64 / 10_000.0)).round() as u64;

        Ok(Quote {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            in_amount: amount,
            out_amount,
            other_amount_threshold: threshold,
            slippage_bps: self.slippage_bps,
            price_impact_pct: 0.0,
            raw: Some(serde_json::json!({ "mock": true })),
        })
    }

    async fn swap_transaction(&self, _quote: &Quote, _user_pubkey: &str) -> Result<String> {
        Err(anyhow!(
            "MockPriceSource cannot produce a real swap transaction; it is for tests only"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn applies_rate_and_slippage() {
        let mut src = MockPriceSource::new(50); // 0.5%
        src.set_rate("A", "B", 2.0);

        let q = src.quote("A", "B", 1_000).await.unwrap();
        assert_eq!(q.in_amount, 1_000);
        assert_eq!(q.out_amount, 2_000);
        // 0.5% below 2000
        assert_eq!(q.other_amount_threshold, 1_990);
    }

    #[tokio::test]
    async fn unknown_pair_errors() {
        let src = MockPriceSource::new(50);
        assert!(src.quote("A", "B", 1).await.is_err());
    }
}
