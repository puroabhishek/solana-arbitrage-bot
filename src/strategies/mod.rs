use anyhow::Result;
use async_trait::async_trait;

use crate::prices::RoundTrip;
use crate::types::Route;

pub mod two_hop;
pub use two_hop::TwoHopStrategy;

/// A way of turning candidate round trips into executable routes.
///
/// Takes `RoundTrip` rather than scalar prices: a route can only be built from
/// real quotes, since the quote carries the routing data the swap needs.
#[async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    async fn find_opportunities(&self, round_trips: &[RoundTrip]) -> Result<Vec<Route>>;
    fn estimate_profit(&self, route: &Route) -> Result<f64>;
}

/// Solana's per-signature base fee.
pub const BASE_SIGNATURE_FEE_LAMPORTS: u64 = 5_000;

/// Total lamports a trade costs to land, independent of its size.
///
/// Priority fee is quoted in micro-lamports per compute unit, so it must be
/// scaled by the compute budget and divided down by 1e6.
pub fn transaction_cost_lamports(
    signatures: u64,
    priority_fee_microlamports: u64,
    compute_units: u64,
) -> u64 {
    let base = BASE_SIGNATURE_FEE_LAMPORTS.saturating_mul(signatures);
    let priority = priority_fee_microlamports
        .saturating_mul(compute_units)
        .saturating_div(1_000_000);
    base.saturating_add(priority)
}

/// Net profit of a round trip, in lamports, after transaction costs.
///
/// Returns a signed value: negative means the round trip loses money. Taking
/// gross profit here instead is the classic way a toy bot convinces itself it
/// is winning while its wallet drains.
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
        let cost = transaction_cost_lamports(1, 1_000, 200_000);
        assert_eq!(cost, BASE_SIGNATURE_FEE_LAMPORTS + 200);
    }

    #[test]
    fn gross_gain_can_still_be_a_net_loss() {
        // Out exceeds in by 1000, but the trade costs 5200 to land.
        let cost = transaction_cost_lamports(1, 1_000, 200_000);
        let net = net_profit_lamports(1_000_000, 1_001_000, cost);
        assert!(net < 0, "expected a net loss, got {}", net);
    }

    #[test]
    fn genuinely_profitable_round_trip_is_positive() {
        let cost = transaction_cost_lamports(1, 1_000, 200_000);
        let net = net_profit_lamports(1_000_000, 1_020_000, cost);
        assert!(net > 0);
        let pct = net_profit_percentage(1_000_000, 1_020_000, cost);
        assert!(pct > 1.4 && pct < 1.5, "unexpected pct {}", pct);
    }

    #[test]
    fn zero_input_does_not_divide_by_zero() {
        assert_eq!(net_profit_percentage(0, 100, 0), 0.0);
    }
}
