use serde::{Deserialize, Serialize};

use crate::prices::Quote;
use crate::strategies::TradeCosts;

/// A price observation, kept for display and logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    pub dex: String,
    pub token_pair: String,
    pub price: f64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub buy_dex: String,
    pub sell_dex: String,
    pub token_pair: String,
    pub profit_percentage: f64,
}

/// An executable circular trade.
///
/// Amounts are base units (lamports for SOL) throughout — never SOL floats.
/// Mixing the two is how a size check silently becomes a no-op.
#[derive(Debug, Clone)]
pub struct Route {
    pub steps: Vec<SwapStep>,
    /// Net profit as a percentage of the amount committed, after fees.
    pub expected_profit: f64,
    /// Display label, e.g. "SOL/USDC".
    pub label: String,
    /// Which strategy produced this route.
    pub strategy: &'static str,
    /// Base units of the starting token committed.
    pub amount_in: u64,
    /// Base units of the starting token expected back.
    pub amount_out: u64,
    /// Gross difference before costs. Negative is a loss.
    pub gross_profit: i64,
    /// Net profit in base units after transaction costs. Negative is a loss.
    pub net_profit: i64,
    /// Itemised cost of landing this trade.
    pub costs: TradeCosts,
    /// The quotes backing each leg, needed to request swap transactions.
    pub quotes: Vec<Quote>,
}

#[derive(Debug, Clone)]
pub struct SwapStep {
    pub dex: DEX,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: u64,
    pub amount_out: u64,
    pub minimum_amount_out: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DEX {
    Raydium,
    Orca,
    Meteora,
    /// Routed by an aggregator, which picks the venue itself.
    Aggregator,
}

impl std::fmt::Display for DEX {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DEX::Raydium => write!(f, "Raydium"),
            DEX::Orca => write!(f, "Orca"),
            DEX::Meteora => write!(f, "Meteora"),
            DEX::Aggregator => write!(f, "Aggregator"),
        }
    }
}

/// The safety limits in force at the moment a trade was evaluated.
///
/// Snapshotted per trade rather than read from config at review time, because
/// config changes: without this, a log entry cannot answer "what were the caps
/// when this was allowed through?"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapSnapshot {
    /// Per-trade ceiling in lamports, if configured.
    pub max_spend_lamports: Option<u64>,
    /// Cumulative realised-loss ceiling in lamports, if configured.
    pub max_cumulative_loss_lamports: Option<u64>,
    /// Realised losses accumulated before this trade was evaluated.
    pub cumulative_loss_at_evaluation: u64,
}

/// A recorded trade attempt, persisted so history and the loss cap survive
/// restarts.
///
/// Deliberately verbose: every number that fed the decision is recorded, so a
/// trade — taken or refused — can be fully audited later without rerunning it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub timestamp: String,
    pub mode: String,
    /// Which strategy produced the route.
    pub strategy: String,
    pub label: String,
    pub amount_in: u64,
    pub expected_out: u64,
    /// Difference before costs, in lamports.
    pub gross_profit: i64,
    /// Difference after costs, in lamports. This drives the decision.
    pub net_profit: i64,
    pub expected_profit_pct: f64,
    /// Itemised cost of landing the trade.
    pub costs: TradeCosts,
    /// Limits in force when this trade was evaluated.
    pub caps: CapSnapshot,
    /// Realised profit in base units once known. Negative is a loss.
    pub realised_profit: Option<i64>,
    pub signature: Option<String>,
    pub outcome: String,
}

pub type Price = f64;
pub type Amount = f64;
