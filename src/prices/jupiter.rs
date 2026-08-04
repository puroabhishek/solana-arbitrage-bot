use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::{PriceSource, Quote};

/// Jupiter's aggregator API.
///
/// Field names below mirror Jupiter's OpenAPI schema exactly; they are
/// camelCase on the wire.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupQuoteResponse {
    input_mint: String,
    /// Amounts arrive as decimal strings, not numbers, to avoid JS precision
    /// loss on u64 values.
    in_amount: String,
    output_mint: String,
    out_amount: String,
    other_amount_threshold: String,
    #[serde(default)]
    slippage_bps: u16,
    #[serde(default)]
    price_impact_pct: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupSwapResponse {
    swap_transaction: String,
}

pub struct JupiterPriceSource {
    base_url: String,
    api_key: Option<String>,
    slippage_bps: u16,
    priority_fee_microlamports: u64,
    client: Client,
}

impl JupiterPriceSource {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        slippage_bps: u16,
        priority_fee_microlamports: u64,
    ) -> Result<Self> {
        let client = Client::builder()
            // An arbitrage quote that takes longer than this is already stale.
            .timeout(Duration::from_secs(10))
            .build()
            .context("building HTTP client for Jupiter")?;

        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            slippage_bps,
            priority_fee_microlamports,
            client,
        })
    }

    fn with_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => req.header("x-api-key", key),
            None => req,
        }
    }
}

fn parse_amount(field: &str, raw: &str) -> Result<u64> {
    raw.parse::<u64>()
        .with_context(|| format!("parsing Jupiter '{}' value '{}' as u64", field, raw))
}

#[async_trait]
impl PriceSource for JupiterPriceSource {
    fn name(&self) -> &'static str {
        "jupiter"
    }

    async fn quote(&self, input_mint: &str, output_mint: &str, amount: u64) -> Result<Quote> {
        let url = format!("{}/quote", self.base_url);
        let slippage = self.slippage_bps.to_string();
        let amount_str = amount.to_string();

        let req = self.client.get(&url).query(&[
            ("inputMint", input_mint),
            ("outputMint", output_mint),
            ("amount", amount_str.as_str()),
            ("slippageBps", slippage.as_str()),
        ]);

        let resp = self
            .with_auth(req)
            .send()
            .await
            .with_context(|| format!("requesting Jupiter quote {} -> {}", input_mint, output_mint))?;

        let status = resp.status();
        let body = resp.text().await.context("reading Jupiter quote body")?;

        if !status.is_success() {
            return Err(anyhow!(
                "Jupiter quote failed ({}): {}",
                status,
                body.chars().take(400).collect::<String>()
            ));
        }

        // Keep the raw payload: /swap requires the quote echoed back verbatim.
        let raw: serde_json::Value =
            serde_json::from_str(&body).context("parsing Jupiter quote as JSON")?;
        let parsed: JupQuoteResponse =
            serde_json::from_value(raw.clone()).context("decoding Jupiter quote schema")?;

        Ok(Quote {
            input_mint: parsed.input_mint,
            output_mint: parsed.output_mint,
            in_amount: parse_amount("inAmount", &parsed.in_amount)?,
            out_amount: parse_amount("outAmount", &parsed.out_amount)?,
            other_amount_threshold: parse_amount(
                "otherAmountThreshold",
                &parsed.other_amount_threshold,
            )?,
            slippage_bps: parsed.slippage_bps,
            price_impact_pct: parsed.price_impact_pct.parse::<f64>().unwrap_or(0.0),
            raw: Some(raw),
        })
    }

    async fn swap_transaction(&self, quote: &Quote, user_pubkey: &str) -> Result<String> {
        let quote_response = quote
            .raw
            .as_ref()
            .ok_or_else(|| anyhow!("quote is missing its raw payload; cannot request a swap"))?;

        let url = format!("{}/swap", self.base_url);
        let body = serde_json::json!({
            "userPublicKey": user_pubkey,
            "quoteResponse": quote_response,
            "wrapAndUnwrapSol": true,
            "dynamicComputeUnitLimit": true,
            "computeUnitPriceMicroLamports": self.priority_fee_microlamports,
        });

        let resp = self
            .with_auth(self.client.post(&url).json(&body))
            .send()
            .await
            .context("requesting Jupiter swap transaction")?;

        let status = resp.status();
        let text = resp.text().await.context("reading Jupiter swap body")?;

        if !status.is_success() {
            return Err(anyhow!(
                "Jupiter swap failed ({}): {}",
                status,
                text.chars().take(400).collect::<String>()
            ));
        }

        let parsed: JupSwapResponse =
            serde_json::from_str(&text).context("decoding Jupiter swap schema")?;
        Ok(parsed.swap_transaction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_string_amounts() {
        // Jupiter sends u64 amounts as strings; make sure we don't lose them.
        let json = r#"{
            "inputMint": "So11111111111111111111111111111111111111112",
            "inAmount": "1000000000",
            "outputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "outAmount": "18446744073",
            "otherAmountThreshold": "18354010352",
            "slippageBps": 50,
            "priceImpactPct": "0.0001"
        }"#;
        let parsed: JupQuoteResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parse_amount("inAmount", &parsed.in_amount).unwrap(), 1_000_000_000);
        assert_eq!(parse_amount("outAmount", &parsed.out_amount).unwrap(), 18_446_744_073);
        assert_eq!(parsed.slippage_bps, 50);
    }

    #[test]
    fn rejects_non_numeric_amount() {
        assert!(parse_amount("inAmount", "not-a-number").is_err());
    }

    #[test]
    fn trims_trailing_slash_from_base_url() {
        let src = JupiterPriceSource::new("https://example.test/swap/v1/", None, 50, 1000).unwrap();
        assert_eq!(src.base_url, "https://example.test/swap/v1");
    }
}
