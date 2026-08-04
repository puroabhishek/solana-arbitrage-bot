use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod jupiter;
pub mod mock;

pub use jupiter::JupiterPriceSource;
pub use mock::MockPriceSource;

/// A token this bot knows how to trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub symbol: &'static str,
    pub mint: &'static str,
    pub decimals: u8,
}

pub const SOL: Token = Token {
    symbol: "SOL",
    mint: "So11111111111111111111111111111111111111112",
    decimals: 9,
};

pub const USDC: Token = Token {
    symbol: "USDC",
    mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    decimals: 6,
};

pub const USDT: Token = Token {
    symbol: "USDT",
    mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
    decimals: 6,
};

/// Pairs the bot scans for round-trip opportunities.
pub fn default_pairs() -> Vec<(Token, Token)> {
    vec![(SOL, USDC), (SOL, USDT)]
}

/// A quote for swapping `in_amount` of `input_mint` into `output_mint`.
///
/// Amounts are in each token's smallest unit (base units), never floats —
/// float amounts are how rounding errors turn into lost funds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub input_mint: String,
    pub output_mint: String,
    pub in_amount: u64,
    pub out_amount: u64,
    /// Worst-case output after slippage tolerance is applied.
    pub other_amount_threshold: u64,
    pub slippage_bps: u16,
    pub price_impact_pct: f64,
    /// Opaque provider payload, needed to request the matching swap
    /// transaction. Carried verbatim so we never reconstruct a quote by hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

/// A complete circular trade: swap out and back to the starting token.
///
/// Arbitrage is only meaningful as a round trip — ending up with more of the
/// token you started with. Holding both legs together keeps the two quotes
/// that must agree from drifting apart.
#[derive(Debug, Clone)]
pub struct RoundTrip {
    /// Display label, e.g. "SOL/USDC".
    pub label: String,
    /// First leg, e.g. SOL -> USDC.
    pub forward: Quote,
    /// Return leg, e.g. USDC -> SOL.
    pub back: Quote,
}

impl RoundTrip {
    /// Base units of the starting token committed.
    pub fn amount_in(&self) -> u64 {
        self.forward.in_amount
    }

    /// Base units of the starting token received back.
    pub fn amount_out(&self) -> u64 {
        self.back.out_amount
    }
}

/// Where quotes and swap transactions come from.
///
/// Prices are always real and live in every execution mode — the mock
/// implementation exists purely so tests stay offline and deterministic.
#[async_trait]
pub trait PriceSource: Send + Sync {
    fn name(&self) -> &'static str;

    /// Quote `amount` (base units of `input_mint`) into `output_mint`.
    async fn quote(&self, input_mint: &str, output_mint: &str, amount: u64) -> Result<Quote>;

    /// Fetch a ready-to-sign swap transaction for a quote, as returned by the
    /// provider. Returned base64-encoded exactly as the provider encodes it.
    async fn swap_transaction(&self, quote: &Quote, user_pubkey: &str) -> Result<String>;
}
