use serde::{Serialize, Deserialize};
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Serialize, Deserialize)]
pub struct PriceData {
    pub dex: String,
    pub token_pair: String,
    pub price: f64,
    pub timestamp: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub buy_dex: String,
    pub sell_dex: String,
    pub token_pair: String,
    pub profit_percentage: f64,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub steps: Vec<SwapStep>,
    pub expected_profit: f64,
}

#[derive(Debug, Clone)]
pub struct SwapStep {
    pub dex: DEX,
    pub token_in: Pubkey,
    pub token_out: Pubkey,
    pub amount_in: u64,
    pub amount_out: u64,
    pub minimum_amount_out: u64,
}

#[derive(Debug, Clone)]
pub enum DEX {
    Raydium,
    Orca,
    Meteora,
}
// Add common types here
pub type Price = f64;
pub type Amount = f64;