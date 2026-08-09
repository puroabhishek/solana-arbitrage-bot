use anyhow::Result;
use async_trait::async_trait;

use super::{net_profit_lamports, net_profit_percentage, Strategy, TradeCosts};
use crate::prices::{to_ui_amount, RoundTrip};
use crate::types::{LegRecord, Route, SwapStep, DEX};

pub const STRATEGY_NAME: &str = "two-hop";

/// Compute units a two-leg swap is assumed to consume when estimating the
/// priority-fee component of cost. Jupiter sets the real limit dynamically;
/// this is only used to decide whether a trade is worth attempting.
const ASSUMED_COMPUTE_UNITS: u64 = 400_000;

/// Signatures on a swap transaction (fee payer only).
const SIGNATURE_COUNT: u64 = 1;

pub struct TwoHopStrategy {
    min_profit_percentage: f64,
    priority_fee_microlamports: u64,
    /// Reject routes whose quoted price impact exceeds this percentage.
    max_price_impact_pct: f64,
}

/// Default ceiling on price impact. Beyond this the trade is moving the pool
/// against itself enough that the quoted edge is unlikely to survive.
pub const DEFAULT_MAX_PRICE_IMPACT_PCT: f64 = 1.0;

impl TwoHopStrategy {
    pub fn new(min_profit_percentage: f64, priority_fee_microlamports: u64) -> Self {
        Self {
            min_profit_percentage,
            priority_fee_microlamports,
            max_price_impact_pct: DEFAULT_MAX_PRICE_IMPACT_PCT,
        }
    }

    pub fn with_max_price_impact(mut self, pct: f64) -> Self {
        self.max_price_impact_pct = pct;
        self
    }

    /// Worst price impact across the legs, as a percentage.
    fn worst_price_impact(rt: &RoundTrip) -> f64 {
        rt.forward
            .price_impact_pct
            .abs()
            .max(rt.back.price_impact_pct.abs())
            * 100.0
    }

    /// Itemised cost for one attempt, independent of trade size.
    fn costs(&self) -> TradeCosts {
        TradeCosts::estimate(
            SIGNATURE_COUNT,
            self.priority_fee_microlamports,
            ASSUMED_COMPUTE_UNITS,
        )
    }

    /// Turn a round trip into a Route, whether or not it is profitable.
    fn to_route(&self, rt: &RoundTrip) -> Route {
        let costs = self.costs();
        let cost = costs.total_lamports();
        let amount_in = rt.amount_in();
        let amount_out = rt.amount_out();
        // Judge on what the swap guarantees, not what it hopes for. A trade
        // can legally execute anywhere down to this figure.
        let amount_out_worst = rt.amount_out_worst_case();

        let steps = vec![
            SwapStep {
                dex: DEX::Aggregator,
                token_in: rt.forward.input_mint.clone(),
                token_out: rt.forward.output_mint.clone(),
                amount_in: rt.forward.in_amount,
                amount_out: rt.forward.out_amount,
                minimum_amount_out: rt.forward.other_amount_threshold,
            },
            SwapStep {
                dex: DEX::Aggregator,
                token_in: rt.back.input_mint.clone(),
                token_out: rt.back.output_mint.clone(),
                amount_in: rt.back.in_amount,
                amount_out: rt.back.out_amount,
                minimum_amount_out: rt.back.other_amount_threshold,
            },
        ];

        // Both legs with their quoted prices: what it buys at, and what it can
        // sell back at.
        let legs = vec![
            LegRecord {
                from: rt.base_symbol.clone(),
                to: rt.quote_symbol.clone(),
                amount_in: rt.forward.in_amount,
                amount_out: rt.forward.out_amount,
                ui_amount_in: to_ui_amount(rt.forward.in_amount, rt.base_decimals),
                ui_amount_out: to_ui_amount(rt.forward.out_amount, rt.quote_decimals),
                rate: rt.forward_rate(),
            },
            LegRecord {
                from: rt.quote_symbol.clone(),
                to: rt.base_symbol.clone(),
                amount_in: rt.back.in_amount,
                amount_out: rt.back.out_amount,
                ui_amount_in: to_ui_amount(rt.back.in_amount, rt.quote_decimals),
                ui_amount_out: to_ui_amount(rt.back.out_amount, rt.base_decimals),
                rate: rt.back_rate(),
            },
        ];

        Route {
            steps,
            expected_profit: net_profit_percentage(amount_in, amount_out_worst, cost),
            label: rt.label.clone(),
            strategy: STRATEGY_NAME,
            amount_in,
            amount_out,
            amount_out_worst_case: amount_out_worst,
            gross_profit: amount_out as i64 - amount_in as i64,
            net_profit: net_profit_lamports(amount_in, amount_out_worst, cost),
            net_profit_expected: net_profit_lamports(amount_in, amount_out, cost),
            costs,
            legs,
            quotes: vec![rt.forward.clone(), rt.back.clone()],
        }
    }
}

#[async_trait]
impl Strategy for TwoHopStrategy {
    fn name(&self) -> &'static str {
        STRATEGY_NAME
    }

    async fn find_opportunities(&self, round_trips: &[RoundTrip]) -> Result<Vec<Route>> {
        let mut routes: Vec<Route> = round_trips
            .iter()
            .filter(|rt| {
                // A high-impact route is moving the pool against itself; the
                // quoted edge is unlikely to survive execution.
                let impact = Self::worst_price_impact(rt);
                if impact > self.max_price_impact_pct {
                    log::info!(
                        "{}: skipping, price impact {:.3}% exceeds {:.3}% ceiling",
                        rt.label,
                        impact,
                        self.max_price_impact_pct
                    );
                    return false;
                }
                true
            })
            .map(|rt| self.to_route(rt))
            .filter(|r| {
                // Both conditions matter: net_profit > 0 rejects trades that
                // lose money outright, and the percentage threshold rejects
                // margins too thin to survive a price move between quote and
                // landing.
                r.net_profit > 0 && r.expected_profit >= self.min_profit_percentage
            })
            .collect();

        // Best first, so the caller can simply take the head.
        routes.sort_by(|a, b| b.net_profit.cmp(&a.net_profit));
        Ok(routes)
    }

    fn estimate_profit(&self, route: &Route) -> Result<f64> {
        Ok(route.expected_profit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prices::Quote;

    fn quote(input: &str, output: &str, in_amt: u64, out_amt: u64) -> Quote {
        Quote {
            input_mint: input.to_string(),
            output_mint: output.to_string(),
            in_amount: in_amt,
            out_amount: out_amt,
            other_amount_threshold: (out_amt as f64 * 0.995) as u64,
            slippage_bps: 50,
            price_impact_pct: 0.0,
            raw: None,
        }
    }

    fn round_trip(in_amt: u64, mid: u64, out_amt: u64) -> RoundTrip {
        RoundTrip {
            label: "SOL/USDC".to_string(),
            base_symbol: "SOL".to_string(),
            quote_symbol: "USDC".to_string(),
            base_decimals: 9,
            quote_decimals: 6,
            forward: quote("SOL", "USDC", in_amt, mid),
            back: quote("USDC", "SOL", mid, out_amt),
        }
    }

    #[tokio::test]
    async fn rejects_round_trip_that_loses_money() {
        let s = TwoHopStrategy::new(0.0, 1_000);
        // Comes back with less than went in.
        let found = s.find_opportunities(&[round_trip(1_000_000, 500, 990_000)]).await.unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn rejects_gross_gain_that_does_not_cover_fees() {
        let s = TwoHopStrategy::new(0.0, 1_000);
        // +1000 gross, but the attempt costs ~5400 lamports to land.
        let found = s.find_opportunities(&[round_trip(1_000_000, 500, 1_001_000)]).await.unwrap();
        assert!(
            found.is_empty(),
            "a gross gain smaller than fees must not be treated as an opportunity"
        );
    }

    #[tokio::test]
    async fn accepts_genuinely_profitable_round_trip() {
        // Must clear the bar on the GUARANTEED minimum, not just on
        // expectation. At +2% expected the guarantee is only +0.95% once the
        // fixture's 0.5% threshold is applied, which is exactly the kind of
        // trade that should not qualify against a 1% floor.
        let s = TwoHopStrategy::new(1.0, 1_000);
        let found = s.find_opportunities(&[round_trip(1_000_000, 500, 1_030_000)]).await.unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].net_profit > 0);
        assert_eq!(found[0].steps.len(), 2);
    }

    #[tokio::test]
    async fn honours_minimum_profit_threshold() {
        // Profitable, but under a 5% floor.
        let s = TwoHopStrategy::new(5.0, 1_000);
        let found = s.find_opportunities(&[round_trip(1_000_000, 500, 1_020_000)]).await.unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn judges_on_worst_case_not_expected() {
        // Expected output clears a 1% bar, but the guaranteed minimum (0.5%
        // below, per the test fixture's threshold) does not. Judging on
        // expectation here would take a trade that can legally settle at a
        // loss.
        let s = TwoHopStrategy::new(1.0, 1_000);
        let rt = round_trip(1_000_000, 500, 1_012_000);

        let expected_pct = (1_012_000i64 - 1_000_000 - 5_400) as f64 / 1_000_000.0 * 100.0;
        assert!(expected_pct > 0.6, "fixture should look good on expectation");

        let found = s.find_opportunities(&[rt]).await.unwrap();
        assert!(
            found.is_empty(),
            "worst-case output must gate the decision, not the expected output"
        );
    }

    #[tokio::test]
    async fn worst_case_never_exceeds_expected() {
        let s = TwoHopStrategy::new(0.0, 1_000);
        let found = s.find_opportunities(&[round_trip(1_000_000, 500, 1_100_000)]).await.unwrap();
        let r = &found[0];
        assert!(
            r.net_profit <= r.net_profit_expected,
            "worst case {} must not exceed expected {}",
            r.net_profit,
            r.net_profit_expected
        );
        assert!(r.amount_out_worst_case <= r.amount_out);
    }

    #[tokio::test]
    async fn rejects_routes_with_excessive_price_impact() {
        let s = TwoHopStrategy::new(0.0, 1_000).with_max_price_impact(0.5);
        let mut rt = round_trip(1_000_000, 500, 1_100_000);
        // 2% impact, over the 0.5% ceiling.
        rt.forward.price_impact_pct = 0.02;

        let found = s.find_opportunities(&[rt]).await.unwrap();
        assert!(found.is_empty(), "high price impact must disqualify a route");
    }

    #[tokio::test]
    async fn sorts_best_first() {
        let s = TwoHopStrategy::new(0.0, 1_000);
        let found = s
            .find_opportunities(&[
                round_trip(1_000_000, 500, 1_020_000),
                round_trip(1_000_000, 500, 1_050_000),
            ])
            .await
            .unwrap();
        assert_eq!(found.len(), 2);
        assert!(found[0].net_profit > found[1].net_profit);
    }
}
