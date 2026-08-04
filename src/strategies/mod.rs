use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::prices::RoundTrip;
use crate::types::Route;

pub mod two_hop;
pub use two_hop::TwoHopStrategy;

/// A way of turning candidate round trips into executable routes.
///
/// Adding a new strategy means implementing this trait and registering it in
/// [`build`] — nothing else in the bot needs to change. The execution gate,
/// logging and CLI all work against the trait, not any concrete strategy.
#[async_trait]
pub trait Strategy: Send + Sync {
    /// Stable identifier, recorded in the trade log so it is always clear
    /// which strategy produced a given trade.
    fn name(&self) -> &'static str;

    async fn find_opportunities(&self, round_trips: &[RoundTrip]) -> Result<Vec<Route>>;

    fn estimate_profit(&self, route: &Route) -> Result<f64>;
}

/// Strategies available to `--strategy`.
///
/// Only two-hop is implemented. Triangular and cross-venue strategies fit the
/// same trait; they are deliberately absent rather than present as stubs.
pub const AVAILABLE: &[&str] = &["two-hop"];

/// Construct a strategy by name.
pub fn build(
    name: &str,
    min_profit_percentage: f64,
    priority_fee_microlamports: u64,
) -> Result<Box<dyn Strategy>> {
    match name.trim().to_lowercase().as_str() {
        "two-hop" | "twohop" | "two_hop" => Ok(Box::new(TwoHopStrategy::new(
            min_profit_percentage,
            priority_fee_microlamports,
        ))),
        other => Err(anyhow!(
            "unknown strategy '{}' (available: {})",
            other,
            AVAILABLE.join(", ")
        )),
    }
}

/// Solana's per-signature base fee, in lamports.
pub const BASE_SIGNATURE_FEE_LAMPORTS: u64 = 5_000;

/// What it costs to land one attempt, itemised.
///
/// Kept as a breakdown rather than a single number so the trade log can show
/// exactly why a trade was or was not worth taking — a bare total makes a
/// rejected opportunity impossible to audit after the fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeCosts {
    /// Signature fees: `BASE_SIGNATURE_FEE_LAMPORTS` per signature.
    pub base_fee_lamports: u64,
    /// Priority fee actually payable, in lamports.
    pub priority_fee_lamports: u64,
    /// Number of signatures assumed.
    pub signatures: u64,
    /// The configured price per compute unit, in micro-lamports.
    pub priority_fee_microlamports_per_cu: u64,
    /// Compute units assumed when estimating the priority fee.
    pub compute_units: u64,
}

impl TradeCosts {
    /// Priority fee is priced in micro-lamports per compute unit, so it scales
    /// with the compute budget and needs dividing down by 1e6.
    pub fn estimate(
        signatures: u64,
        priority_fee_microlamports_per_cu: u64,
        compute_units: u64,
    ) -> Self {
        Self {
            base_fee_lamports: BASE_SIGNATURE_FEE_LAMPORTS.saturating_mul(signatures),
            priority_fee_lamports: priority_fee_microlamports_per_cu
                .saturating_mul(compute_units)
                .saturating_div(1_000_000),
            signatures,
            priority_fee_microlamports_per_cu,
            compute_units,
        }
    }

    pub fn total_lamports(&self) -> u64 {
        self.base_fee_lamports
            .saturating_add(self.priority_fee_lamports)
    }
}

/// Net profit of a round trip in lamports, after costs.
///
/// Signed: negative means the round trip loses money. Using gross profit here
/// is the classic way a toy bot convinces itself it is winning while its
/// wallet drains.
pub fn net_profit_lamports(amount_in: u64, amount_out: u64, tx_cost: u64) -> i64 {
    amount_out as i64 - amount_in as i64 - tx_cost as i64
}

/// Net profit as a percentage of the amount committed.
pub fn net_profit_percentage(amount_in: u64, amount_out: u64, tx_cost: u64) -> f64 {
    if amount_in == 0 {
        return 0.0;
    }
    net_profit_lamports(amount_in, amount_out, tx_cost) as f64 / amount_in as f64 * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_fee_is_scaled_by_compute_units() {
        // 1000 micro-lamports/CU over 200k CU = 200_000_000 micro = 200 lamports
        let c = TradeCosts::estimate(1, 1_000, 200_000);
        assert_eq!(c.base_fee_lamports, BASE_SIGNATURE_FEE_LAMPORTS);
        assert_eq!(c.priority_fee_lamports, 200);
        assert_eq!(c.total_lamports(), 5_200);
    }

    #[test]
    fn breakdown_retains_inputs_for_auditing() {
        let c = TradeCosts::estimate(2, 1_500, 400_000);
        assert_eq!(c.signatures, 2);
        assert_eq!(c.priority_fee_microlamports_per_cu, 1_500);
        assert_eq!(c.compute_units, 400_000);
        assert_eq!(c.base_fee_lamports, 10_000);
        assert_eq!(c.priority_fee_lamports, 600);
    }

    #[test]
    fn gross_gain_can_still_be_a_net_loss() {
        let cost = TradeCosts::estimate(1, 1_000, 200_000).total_lamports();
        assert!(net_profit_lamports(1_000_000, 1_001_000, cost) < 0);
    }

    #[test]
    fn genuinely_profitable_round_trip_is_positive() {
        let cost = TradeCosts::estimate(1, 1_000, 200_000).total_lamports();
        assert!(net_profit_lamports(1_000_000, 1_020_000, cost) > 0);
    }

    #[test]
    fn zero_input_does_not_divide_by_zero() {
        assert_eq!(net_profit_percentage(0, 100, 0), 0.0);
    }

    #[test]
    fn registry_builds_known_and_rejects_unknown() {
        assert!(build("two-hop", 1.0, 1_000).is_ok());
        assert!(build("TwoHop", 1.0, 1_000).is_ok());
        let err = match build("triangular", 1.0, 1_000) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("unknown strategy should not build"),
        };
        assert!(err.contains("unknown strategy"), "got: {}", err);
        assert!(err.contains("two-hop"), "error should list what is available");
    }
}
