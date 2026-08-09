use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use std::env;
use std::fmt;
use std::str::FromStr;

/// Which Solana cluster to talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Devnet,
    Mainnet,
}

impl Network {
    pub fn default_rpc_url(&self) -> &'static str {
        match self {
            Network::Devnet => "https://api.devnet.solana.com",
            Network::Mainnet => "https://api.mainnet-beta.solana.com",
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Network::Devnet => write!(f, "devnet"),
            Network::Mainnet => write!(f, "mainnet"),
        }
    }
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "devnet" | "dev" => Ok(Network::Devnet),
            "mainnet" | "mainnet-beta" | "main" => Ok(Network::Mainnet),
            other => Err(anyhow!("unknown network '{}' (expected devnet or mainnet)", other)),
        }
    }
}

/// How far along the execution ladder to go.
///
/// Each rung exercises something the others cannot, and is intended to be
/// promoted through in order: `Detect` -> `Rehearse` -> `Simulate` -> `Live`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Real live prices, evaluate opportunities. Builds and submits nothing.
    Detect,
    /// Submits a real but trivial (non-swap) transaction to devnet, proving the
    /// sign -> submit -> confirm -> log path works. Costs only test SOL.
    Rehearse,
    /// Builds the real mainnet swap transaction and simulates it. Never submits.
    Simulate,
    /// Builds and submits the real swap transaction. Spends real funds.
    Live,
}

impl ExecutionMode {
    /// Whether this mode ever sends a transaction to a cluster.
    pub fn submits(&self) -> bool {
        matches!(self, ExecutionMode::Rehearse | ExecutionMode::Live)
    }

    /// Whether this mode risks real funds.
    pub fn risks_real_funds(&self) -> bool {
        matches!(self, ExecutionMode::Live)
    }

    /// The cluster a mode implies when the user has not overridden it.
    ///
    /// `Rehearse` is devnet by definition; everything else needs mainnet
    /// liquidity to be meaningful.
    pub fn implied_network(&self) -> Network {
        match self {
            ExecutionMode::Rehearse => Network::Devnet,
            _ => Network::Mainnet,
        }
    }
}

impl fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionMode::Detect => write!(f, "detect"),
            ExecutionMode::Rehearse => write!(f, "rehearse"),
            ExecutionMode::Simulate => write!(f, "simulate"),
            ExecutionMode::Live => write!(f, "live"),
        }
    }
}

impl FromStr for ExecutionMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "detect" => Ok(ExecutionMode::Detect),
            "rehearse" => Ok(ExecutionMode::Rehearse),
            "simulate" => Ok(ExecutionMode::Simulate),
            "live" => Ok(ExecutionMode::Live),
            other => Err(anyhow!(
                "unknown mode '{}' (expected detect, rehearse, simulate or live)",
                other
            )),
        }
    }
}

pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Jupiter's free tier. Requires no API key.
pub const JUPITER_LITE_URL: &str = "https://lite-api.jup.ag/swap/v1";
/// Jupiter's paid tier. Requires an API key.
pub const JUPITER_PRO_URL: &str = "https://api.jup.ag/swap/v1";

pub fn sol_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SOL as f64).round() as u64
}

pub fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL as f64
}

pub struct Config {
    /// Explicit RPC override from the environment, if any. When unset the URL
    /// is derived from the active network so switching clusters is one change.
    pub rpc_url_override: Option<String>,
    pub network: Network,
    pub min_profit_percentage: f64,
    /// Per-trade ceiling. `None` means unset, which blocks live trading.
    pub max_spend_lamports: Option<u64>,
    /// Cumulative realised-loss ceiling. `None` means unset, which blocks live
    /// trading. Tracked across restarts via the trade log.
    pub max_cumulative_loss_lamports: Option<u64>,
    pub slippage_bps: u16,
    pub priority_fee_microlamports: u64,
    pub jupiter_base_url: String,
    pub jupiter_api_key: Option<String>,
    pub poll_interval_secs: u64,
    pub wallet_path: Option<String>,
    /// Discord incoming-webhook URL. Unset disables notifications entirely.
    pub discord_webhook_url: Option<String>,
    /// Submit via Jito bundles rather than a naked transaction.
    ///
    /// Defaults on: a bundle is atomic across both legs, and an unselected
    /// bundle commits nothing and therefore costs nothing, where a reverted
    /// naked transaction still burns the fee.
    pub use_jito_bundles: bool,
    pub jito_block_engine_url: String,
    /// Which landed-tip percentile to pay.
    pub jito_tip_percentile: String,
}

impl Config {
    /// RPC endpoint for a given network, honouring an explicit override.
    pub fn rpc_url_for(&self, network: Network) -> String {
        self.rpc_url_override
            .clone()
            .unwrap_or_else(|| network.default_rpc_url().to_string())
    }

    /// The profit floor slippage alone imposes, as a percentage.
    ///
    /// Each leg may execute up to `slippage_bps` worse than quoted, and a round
    /// trip has two legs, so the round trip can lose twice that before
    /// anything has gone wrong.
    pub fn slippage_floor_pct(&self) -> f64 {
        2.0 * (self.slippage_bps as f64 / 100.0)
    }

    /// Reject configurations that can act on trades guaranteed to be able to
    /// lose money.
    ///
    /// If the profit threshold sits below what slippage can take, a trade can
    /// pass the filter, execute entirely within tolerance, and still settle at
    /// a loss. That is a configuration error, not a trading risk, so it fails
    /// at startup rather than silently at trade time.
    pub fn validate(&self) -> Result<()> {
        let floor = self.slippage_floor_pct();
        if self.min_profit_percentage <= floor {
            return Err(anyhow!(
                "MIN_PROFIT_PERCENTAGE ({:.3}%) must exceed {:.3}% — with SLIPPAGE_BPS={} \
                 each leg may fill {:.3}% worse than quoted, and a round trip has two legs, \
                 so a trade could pass this filter and still settle at a loss. \
                 Either raise MIN_PROFIT_PERCENTAGE above {:.3}% or lower SLIPPAGE_BPS.",
                self.min_profit_percentage,
                floor,
                self.slippage_bps,
                self.slippage_bps as f64 / 100.0,
                floor,
            ));
        }
        Ok(())
    }
}

fn env_opt(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn env_parse<T: FromStr>(key: &str, default: T) -> T {
    env_opt(key)
        .and_then(|v| v.parse::<T>().ok())
        .unwrap_or(default)
}

lazy_static! {
    pub static ref CONFIG: Config = {
        dotenv::dotenv().ok();

        let network = env_opt("SOLANA_NETWORK")
            .and_then(|v| v.parse::<Network>().ok())
            .unwrap_or(Network::Mainnet);

        let jupiter_api_key = env_opt("JUPITER_API_KEY");

        // Default to whichever Jupiter host matches the credentials we have:
        // the free tier needs no key, the pro tier requires one.
        let jupiter_base_url = env_opt("JUPITER_BASE_URL").unwrap_or_else(|| {
            if jupiter_api_key.is_some() {
                JUPITER_PRO_URL.to_string()
            } else {
                JUPITER_LITE_URL.to_string()
            }
        });

        Config {
            rpc_url_override: env_opt("SOLANA_RPC_URL"),
            network,
            min_profit_percentage: env_parse("MIN_PROFIT_PERCENTAGE", 1.5),
            max_spend_lamports: env_opt("MAX_TRADE_AMOUNT_SOL")
                .and_then(|v| v.parse::<f64>().ok())
                .map(sol_to_lamports),
            max_cumulative_loss_lamports: env_opt("MAX_CUMULATIVE_LOSS_SOL")
                .and_then(|v| v.parse::<f64>().ok())
                .map(sol_to_lamports),
            slippage_bps: env_parse("SLIPPAGE_BPS", 50),
            priority_fee_microlamports: env_parse("PRIORITY_FEE_MICROLAMPORTS", 1_000),
            jupiter_base_url,
            jupiter_api_key,
            poll_interval_secs: env_parse("POLL_INTERVAL_SECS", 10),
            wallet_path: env_opt("WALLET_PATH"),
            discord_webhook_url: env_opt("DISCORD_WEBHOOK_URL"),
            use_jito_bundles: env_opt("USE_JITO_BUNDLES")
                .map(|v| !matches!(v.trim().to_lowercase().as_str(), "0" | "false" | "no"))
                .unwrap_or(true),
            jito_block_engine_url: env_opt("JITO_BLOCK_ENGINE_URL")
                .unwrap_or_else(|| crate::execution::bundle::DEFAULT_BLOCK_ENGINE.to_string()),
            jito_tip_percentile: env_opt("JITO_TIP_PERCENTILE")
                .unwrap_or_else(|| "50".to_string()),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamports_round_trip() {
        assert_eq!(sol_to_lamports(1.0), LAMPORTS_PER_SOL);
        assert_eq!(sol_to_lamports(0.1), 100_000_000);
        assert!((lamports_to_sol(LAMPORTS_PER_SOL) - 1.0).abs() < f64::EPSILON);
        assert!((lamports_to_sol(100_000_000) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn mode_parsing_and_properties() {
        assert_eq!("detect".parse::<ExecutionMode>().unwrap(), ExecutionMode::Detect);
        assert_eq!("LIVE".parse::<ExecutionMode>().unwrap(), ExecutionMode::Live);
        assert!("nonsense".parse::<ExecutionMode>().is_err());

        assert!(!ExecutionMode::Detect.submits());
        assert!(!ExecutionMode::Simulate.submits());
        assert!(ExecutionMode::Rehearse.submits());
        assert!(ExecutionMode::Live.submits());

        // Only live risks real money.
        assert!(ExecutionMode::Live.risks_real_funds());
        assert!(!ExecutionMode::Rehearse.risks_real_funds());

        // Rehearse is devnet by definition.
        assert_eq!(ExecutionMode::Rehearse.implied_network(), Network::Devnet);
        assert_eq!(ExecutionMode::Live.implied_network(), Network::Mainnet);
    }

    fn cfg(min_profit: f64, slippage_bps: u16) -> Config {
        Config {
            rpc_url_override: None,
            network: Network::Mainnet,
            min_profit_percentage: min_profit,
            max_spend_lamports: None,
            max_cumulative_loss_lamports: None,
            slippage_bps,
            priority_fee_microlamports: 1_000,
            jupiter_base_url: JUPITER_LITE_URL.to_string(),
            jupiter_api_key: None,
            poll_interval_secs: 10,
            wallet_path: None,
            discord_webhook_url: None,
            use_jito_bundles: true,
            jito_block_engine_url: crate::execution::bundle::DEFAULT_BLOCK_ENGINE.to_string(),
            jito_tip_percentile: "50".to_string(),
        }
    }

    #[test]
    fn slippage_floor_counts_both_legs() {
        // 50 bps per leg, two legs, so 1% of round-trip value can vanish.
        assert!((cfg(1.5, 50).slippage_floor_pct() - 1.0).abs() < 1e-9);
        assert!((cfg(1.5, 10).slippage_floor_pct() - 0.2).abs() < 1e-9);
    }

    #[test]
    fn rejects_profit_threshold_below_slippage_floor() {
        // 0.3% profit target with 1% of slippage headroom: a trade could pass
        // the filter and still settle at a loss.
        let err = cfg(0.3, 50).validate().unwrap_err().to_string();
        assert!(err.contains("MIN_PROFIT_PERCENTAGE"), "got: {}", err);
        assert!(err.contains("SLIPPAGE_BPS"), "should name the other knob too");

        // Exactly at the floor is still rejected — it leaves zero margin.
        assert!(cfg(1.0, 50).validate().is_err());
    }

    #[test]
    fn accepts_threshold_above_slippage_floor() {
        assert!(cfg(1.5, 50).validate().is_ok());
        // Tightening slippage lowers the floor, so a smaller target is fine.
        assert!(cfg(0.5, 10).validate().is_ok());
    }

    #[test]
    fn network_parsing() {
        assert_eq!("devnet".parse::<Network>().unwrap(), Network::Devnet);
        assert_eq!("mainnet-beta".parse::<Network>().unwrap(), Network::Mainnet);
        assert!("testnet".parse::<Network>().is_err());
    }
}
