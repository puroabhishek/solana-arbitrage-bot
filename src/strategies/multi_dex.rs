use super::Strategy;
use crate::types::{ArbitrageOpportunity, PriceData, Route};
use anyhow::Result;

pub struct MultiDexStrategy {
    min_profit_percentage: f64,
}

impl MultiDexStrategy {
    pub fn new(min_profit_percentage: f64) -> Self {
        Self { min_profit_percentage }
    }
}

impl Strategy for MultiDexStrategy {
    fn name(&self) -> &'static str {
        "Multi-DEX Arbitrage"
    }

    fn find_opportunities(&self, _prices: &[PriceData]) -> Result<Vec<ArbitrageOpportunity>> {
        Ok(vec![])
    }

    fn validate_route(&self, route: &Route) -> Result<bool> {
        Ok(!route.steps.is_empty())
    }

    fn estimate_profit(&self, _route: &Route) -> Result<f64> {
        Ok(0.0)
    }
} 