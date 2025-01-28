use super::Strategy;
use anyhow::Result;
use async_trait::async_trait;
use crate::types::{PriceData, Route};

pub struct TwoHopStrategy {
    name: &'static str,
    min_profit_percentage: f64,
}

impl TwoHopStrategy {
    pub fn new(min_profit_percentage: f64) -> Self {
        Self {
            name: "Two-Hop Strategy",
            min_profit_percentage,
        }
    }
}

#[async_trait]
impl Strategy for TwoHopStrategy {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn find_opportunities(&self, _prices: &[PriceData]) -> Result<Vec<Route>> {
        Ok(vec![])
    }

    fn estimate_profit(&self, route: &Route) -> Result<f64> {
        Ok(route.expected_profit)
    }
}