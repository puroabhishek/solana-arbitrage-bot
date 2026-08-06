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
    /// Symbol of the token the trip starts and ends in, e.g. "SOL".
    pub base_symbol: String,
    /// Symbol of the intermediate token, e.g. "USDC".
    pub quote_symbol: String,
    /// Decimals of the base token, needed to render human-readable prices.
    pub base_decimals: u8,
    /// Decimals of the intermediate token.
    pub quote_decimals: u8,
    /// First leg, e.g. SOL -> USDC.
    pub forward: Quote,
    /// Return leg, e.g. USDC -> SOL.
    pub back: Quote,
}

/// Convert a base-unit amount to its human-scale value.
pub fn to_ui_amount(raw: u64, decimals: u8) -> f64 {
    raw as f64 / 10f64.powi(decimals as i32)
}

impl RoundTrip {
    /// Base units of the starting token committed.
    pub fn amount_in(&self) -> u64 {
        self.forward.in_amount
    }

    /// Base units of the starting token expected back, at the mid quote.
    pub fn amount_out(&self) -> u64 {
        self.back.out_amount
    }

    /// Base units guaranteed back in the worst case the quote allows.
    ///
    /// Jupiter's `otherAmountThreshold` is the minimum the swap will accept
    /// before reverting. Judging profitability on the *expected* amount instead
    /// means a trade can execute anywhere within slippage tolerance and still
    /// settle at a loss, while the bot records it as a win.
    pub fn amount_out_worst_case(&self) -> u64 {
        self.back.other_amount_threshold
    }

    /// Price of the outbound leg: quote tokens per one base token.
    ///
    /// Decimal-adjusted, so this is the price a human would recognise (e.g.
    /// ~150 USDC per SOL) rather than a raw base-unit ratio.
    pub fn forward_rate(&self) -> f64 {
        let input = to_ui_amount(self.forward.in_amount, self.base_decimals);
        if input == 0.0 {
            return 0.0;
        }
        to_ui_amount(self.forward.out_amount, self.quote_decimals) / input
    }

    /// Price of the return leg: base tokens per one quote token.
    pub fn back_rate(&self) -> f64 {
        let input = to_ui_amount(self.back.in_amount, self.quote_decimals);
        if input == 0.0 {
            return 0.0;
        }
        to_ui_amount(self.back.out_amount, self.base_decimals) / input
    }

    /// The return leg expressed the same way round as the outbound leg, so the
    /// two are directly comparable: quote tokens per one base token.
    ///
    /// Arbitrage exists when you can sell back at a better price than you
    /// bought at, so seeing both in the same units is what makes a gap visible.
    pub fn back_rate_inverted(&self) -> f64 {
        let r = self.back_rate();
        if r == 0.0 {
            return 0.0;
        }
        1.0 / r
    }

    /// One-line summary of both legs, for logs.
    pub fn price_summary(&self) -> String {
        format!(
            "{} -> {} @ {:.6} | {} -> {} @ {:.6} (= {:.6} {}/{})",
            self.base_symbol,
            self.quote_symbol,
            self.forward_rate(),
            self.quote_symbol,
            self.base_symbol,
            self.back_rate(),
            self.back_rate_inverted(),
            self.quote_symbol,
            self.base_symbol,
        )
    }
}

/// A swap expressed as instructions rather than a finished transaction.
///
/// This is what makes atomic arbitrage possible: a pre-built transaction can
/// only ever hold one leg, whereas instructions from two legs can be composed
/// into a single transaction that either completes the whole round trip or
/// reverts entirely.
#[derive(Debug, Clone)]
pub struct SwapInstructions {
    /// Compute budget instructions. Discarded when composing legs — the
    /// combined transaction needs one budget covering both, not two.
    pub compute_budget: Vec<solana_sdk::instruction::Instruction>,
    /// Account creation / wSOL wrapping that must run before the swap.
    pub setup: Vec<solana_sdk::instruction::Instruction>,
    /// The swap itself.
    pub swap: solana_sdk::instruction::Instruction,
    /// Account closing / unwrapping. Deferred to the end of the composed
    /// transaction so leg 2 can still use accounts leg 1 set up.
    pub cleanup: Vec<solana_sdk::instruction::Instruction>,
    /// Address lookup tables this leg's accounts are drawn from.
    pub address_lookup_tables: Vec<String>,
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
    ///
    /// Single-leg only. Prefer [`swap_instructions`](Self::swap_instructions)
    /// for arbitrage, which needs both legs in one transaction.
    async fn swap_transaction(&self, quote: &Quote, user_pubkey: &str) -> Result<String>;

    /// Fetch a swap as composable instructions, so multiple legs can share one
    /// atomic transaction.
    async fn swap_instructions(
        &self,
        quote: &Quote,
        user_pubkey: &str,
    ) -> Result<SwapInstructions>;
}
