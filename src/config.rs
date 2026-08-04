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
}

impl Config {
    /// RPC endpoint for a given network, honouring an explicit override.
    pub fn rpc_url_for(&self, network: Network) -> String {
        self.rpc_url_override
            .clone()
            .unwrap_or_else(|| network.default_rpc_url().to_string())
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

    #[test]
    fn network_parsing() {
        assert_eq!("devnet".parse::<Network>().unwrap(), Network::Devnet);
        assert_eq!("mainnet-beta".parse::<Network>().unwrap(), Network::Mainnet);
        assert!("testnet".parse::<Network>().is_err());
    }
}
