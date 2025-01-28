use super::Strategy;
use crate::types::{ArbitrageOpportunity, PriceData, Route};
use anyhow::Result;
use crate::strategies::two_hop::calculate_profit;
use crate::strategies::create_route_from_price;

pub struct TriangleStrategy {
    min_profit_percentage: f64,
}

impl TriangleStrategy {
    pub fn new(min_profit_percentage: f64) -> Self {
        Self { min_profit_percentage }
    }
}

impl Strategy for TriangleStrategy {
    fn name(&self) -> &'static str {
        "Triangle Arbitrage"
    }

    fn find_opportunities(&self, prices: &[PriceData]) -> Result<Vec<ArbitrageOpportunity>> {
        let mut opportunities = Vec::new();
        
        for price in prices {
            let route = create_route_from_price(price, true);
            let profit = calculate_profit(&route);
            if profit >= self.min_profit_percentage {
                opportunities.push(ArbitrageOpportunity {
                    buy_dex: price.dex.clone(),
                    sell_dex: "SomeDEX".to_string(),
                    token_pair: price.token_pair.clone(),
                    profit_percentage: profit,
                });
            }
        }

        Ok(opportunities)
    }

    fn validate_route(&self, route: &Route) -> Result<bool> {
        Ok(route.steps.len() == 3)
    }

    fn estimate_profit(&self, _route: &Route) -> Result<f64> {
        Ok(0.0) // Placeholder
    }
} 