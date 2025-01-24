use super::Strategy;
use crate::types::{ArbitrageOpportunity, PriceData, Route};
use anyhow::Result;

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

    fn find_opportunities(&self, _prices: &[PriceData]) -> Result<Vec<ArbitrageOpportunity>> {
        Ok(vec![])
    }

    fn validate_route(&self, route: &Route) -> Result<bool> {
        Ok(route.steps.len() == 3)
    }

    fn estimate_profit(&self, _route: &Route) -> Result<f64> {
        Ok(0.0)
    }
} 