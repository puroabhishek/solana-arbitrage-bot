pub mod two_hop;
pub mod triangle;
pub mod multi_dex;

use crate::types::{ArbitrageOpportunity, PriceData, Route};
use anyhow::Result;

#[derive(Debug, Clone)]
pub enum StrategyType {
    TwoHop,
    Triangle,
    MultiDex,
}

pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn find_opportunities(&self, prices: &[PriceData]) -> Result<Vec<ArbitrageOpportunity>>;
    fn validate_route(&self, route: &Route) -> Result<bool>;
    fn estimate_profit(&self, route: &Route) -> Result<f64>;
} 