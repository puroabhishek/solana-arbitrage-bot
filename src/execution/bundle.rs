use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::Client;
use serde::Deserialize;
use solana_sdk::{pubkey::Pubkey, transaction::VersionedTransaction};
use std::str::FromStr;
use std::time::Duration;

/// Jito's mainnet block engine. Regional endpoints exist; the generic one is
/// the safe default when latency has not been measured.
pub const DEFAULT_BLOCK_ENGINE: &str = "https://mainnet.block-engine.jito.wtf/api/v1/bundles";

/// Live landed-tip percentiles.
pub const TIP_FLOOR_URL: &str = "https://bundles.jito.wtf/api/v1/bundles/tip_floor";

/// Jito's published tip accounts. A tip must go to one of these.
///
/// Picking at random spreads load and avoids every bot contending on the same
/// account, which would serialise bundles that could otherwise run in parallel.
pub const TIP_ACCOUNTS: &[&str] = &[
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

/// Jito's documented floor. A tip below this is rejected outright.
pub const MIN_TIP_LAMPORTS: u64 = 1_000;

/// Which published percentile of landed tips to pay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipPercentile {
    P25,
    P50,
    P75,
    P95,
    P99,
}

impl std::str::FromStr for TipPercentile {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "25" | "p25" => Ok(TipPercentile::P25),
            "50" | "p50" => Ok(TipPercentile::P50),
            "75" | "p75" => Ok(TipPercentile::P75),
            "95" | "p95" => Ok(TipPercentile::P95),
            "99" | "p99" => Ok(TipPercentile::P99),
            other => Err(anyhow!(
                "unknown tip percentile '{}' (expected 25, 50, 75, 95 or 99)",
                other
            )),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct TipFloor {
    #[serde(default)]
    pub landed_tips_25th_percentile: f64,
    #[serde(default)]
    pub landed_tips_50th_percentile: f64,
    #[serde(default)]
    pub landed_tips_75th_percentile: f64,
    #[serde(default)]
    pub landed_tips_95th_percentile: f64,
    #[serde(default)]
    pub landed_tips_99th_percentile: f64,
}

impl TipFloor {
    /// Tip for a percentile, in lamports.
    ///
    /// Jito publishes these in SOL, so they are converted here; treating them
    /// as lamports would under-tip by a factor of 1e9 and never land.
    pub fn lamports_for(&self, p: TipPercentile) -> u64 {
        let sol = match p {
            TipPercentile::P25 => self.landed_tips_25th_percentile,
            TipPercentile::P50 => self.landed_tips_50th_percentile,
            TipPercentile::P75 => self.landed_tips_75th_percentile,
            TipPercentile::P95 => self.landed_tips_95th_percentile,
            TipPercentile::P99 => self.landed_tips_99th_percentile,
        };
        let lamports = (sol * crate::config::LAMPORTS_PER_SOL as f64).round();
        // Guard the floor and any absent/garbage field.
        if !lamports.is_finite() || lamports < MIN_TIP_LAMPORTS as f64 {
            return MIN_TIP_LAMPORTS;
        }
        lamports as u64
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<String>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Submits atomic bundles to Jito's block engine.
///
/// Bundles matter for two reasons beyond atomicity: an unselected bundle
/// commits nothing and therefore **costs nothing**, unlike a naked transaction
/// which burns fees when it reverts; and selection is by tip alone, so the
/// priority fee does not help win the auction.
pub struct BundleClient {
    block_engine_url: String,
    client: Client,
}

impl BundleClient {
    pub fn new(block_engine_url: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("building HTTP client for Jito")?;
        Ok(Self {
            block_engine_url: block_engine_url.into().trim_end_matches('/').to_string(),
            client,
        })
    }

    /// Fetch current landed-tip percentiles.
    ///
    /// Tips are never hardcoded: the competitive level moves constantly, and a
    /// stale figure either wastes money or never lands.
    pub async fn tip_floor(&self) -> Result<TipFloor> {
        let resp = self
            .client
            .get(TIP_FLOOR_URL)
            .send()
            .await
            .context("fetching Jito tip floor")?;

        let status = resp.status();
        let text = resp.text().await.context("reading tip floor body")?;
        if !status.is_success() {
            return Err(anyhow!(
                "tip floor request failed ({}): {}",
                status,
                text.chars().take(200).collect::<String>()
            ));
        }

        // The endpoint returns a single-element array.
        let floors: Vec<TipFloor> =
            serde_json::from_str(&text).context("decoding tip floor response")?;
        floors
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("tip floor response was empty"))
    }

    /// Pick a tip account at random, to avoid contending on one account.
    pub fn random_tip_account() -> Result<Pubkey> {
        // Cheap, dependency-free jitter; the choice only needs to be spread,
        // not cryptographically random.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        let pick = TIP_ACCOUNTS[nanos % TIP_ACCOUNTS.len()];
        Pubkey::from_str(pick).with_context(|| format!("parsing tip account '{}'", pick))
    }

    /// Submit an atomic bundle. Returns the bundle ID.
    ///
    /// A bundle holds at most 5 transactions and executes sequentially,
    /// all-or-none.
    pub async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<String> {
        if txs.is_empty() {
            return Err(anyhow!("cannot submit an empty bundle"));
        }
        if txs.len() > 5 {
            return Err(anyhow!(
                "bundle holds at most 5 transactions, got {}",
                txs.len()
            ));
        }

        let encoded: Vec<String> = txs
            .iter()
            .map(|tx| {
                bincode::serialize(tx)
                    .context("serializing transaction for bundle")
                    .map(|bytes| BASE64.encode(bytes))
            })
            .collect::<Result<Vec<_>>>()?;

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendBundle",
            "params": [encoded, { "encoding": "base64" }],
        });

        let resp = self
            .client
            .post(&self.block_engine_url)
            .json(&body)
            .send()
            .await
            .context("submitting bundle to Jito")?;

        let status = resp.status();
        let text = resp.text().await.context("reading bundle response")?;
        if !status.is_success() {
            return Err(anyhow!(
                "sendBundle failed ({}): {}",
                status,
                text.chars().take(300).collect::<String>()
            ));
        }

        let parsed: JsonRpcResponse =
            serde_json::from_str(&text).context("decoding sendBundle response")?;

        if let Some(e) = parsed.error {
            return Err(anyhow!("sendBundle error {}: {}", e.code, e.message));
        }
        parsed
            .result
            .ok_or_else(|| anyhow!("sendBundle returned neither a result nor an error"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor() -> TipFloor {
        TipFloor {
            landed_tips_25th_percentile: 0.000001,
            landed_tips_50th_percentile: 0.00001,
            landed_tips_75th_percentile: 0.0001,
            landed_tips_95th_percentile: 0.001,
            landed_tips_99th_percentile: 0.01,
        }
    }

    #[test]
    fn percentiles_convert_from_sol_to_lamports() {
        // Jito publishes SOL. Treating the value as lamports would under-tip
        // by 1e9 and never land.
        assert_eq!(floor().lamports_for(TipPercentile::P50), 10_000);
        assert_eq!(floor().lamports_for(TipPercentile::P95), 1_000_000);
        assert_eq!(floor().lamports_for(TipPercentile::P99), 10_000_000);
    }

    #[test]
    fn tip_never_falls_below_jito_minimum() {
        // 0.000001 SOL = 1000 lamports, exactly the floor.
        assert_eq!(floor().lamports_for(TipPercentile::P25), MIN_TIP_LAMPORTS);

        let empty = TipFloor {
            landed_tips_25th_percentile: 0.0,
            landed_tips_50th_percentile: 0.0,
            landed_tips_75th_percentile: 0.0,
            landed_tips_95th_percentile: 0.0,
            landed_tips_99th_percentile: 0.0,
        };
        assert_eq!(empty.lamports_for(TipPercentile::P50), MIN_TIP_LAMPORTS);
    }

    #[test]
    fn nonfinite_percentile_falls_back_to_minimum() {
        let bad = TipFloor {
            landed_tips_25th_percentile: f64::NAN,
            landed_tips_50th_percentile: f64::INFINITY,
            landed_tips_75th_percentile: 0.0,
            landed_tips_95th_percentile: 0.0,
            landed_tips_99th_percentile: 0.0,
        };
        assert_eq!(bad.lamports_for(TipPercentile::P25), MIN_TIP_LAMPORTS);
        assert_eq!(bad.lamports_for(TipPercentile::P50), MIN_TIP_LAMPORTS);
    }

    #[test]
    fn tip_accounts_are_valid_pubkeys() {
        for acct in TIP_ACCOUNTS {
            assert!(
                Pubkey::from_str(acct).is_ok(),
                "tip account '{}' is not a valid pubkey",
                acct
            );
        }
        assert!(BundleClient::random_tip_account().is_ok());
    }

    #[test]
    fn percentile_parsing() {
        assert_eq!("50".parse::<TipPercentile>().unwrap(), TipPercentile::P50);
        assert_eq!("p95".parse::<TipPercentile>().unwrap(), TipPercentile::P95);
        assert!("42".parse::<TipPercentile>().is_err());
    }

    #[tokio::test]
    async fn rejects_oversized_and_empty_bundles() {
        let c = BundleClient::new(DEFAULT_BLOCK_ENGINE).unwrap();
        assert!(c.send_bundle(&[]).await.is_err(), "empty bundle");

        let too_many: Vec<VersionedTransaction> = (0..6)
            .map(|_| VersionedTransaction::default())
            .collect();
        let err = c.send_bundle(&too_many).await.unwrap_err().to_string();
        assert!(err.contains("at most 5"), "got: {}", err);
    }
}
