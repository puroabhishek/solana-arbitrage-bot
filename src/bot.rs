use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::{Keypair, Signer};
use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use crate::config::{lamports_to_sol, ExecutionMode, Network, CONFIG};
use crate::execution::{
    ledger::TradeLedger, transaction_builder::TransactionBuilder, ExecutionEngine,
    ExecutionOutcome, Refusal, SafetyLimits,
};
use crate::notify::{DiscordNotifier, Level, Notification, Notifier, Notifiers};
use crate::prices::{default_pairs, JupiterPriceSource, PriceSource, RoundTrip};
use crate::strategies::{self, Strategy};
use crate::types::{PriceData, Route, TradeRecord};

pub const TRADE_LOG_PATH: &str = "data/trades.json";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct BotStatus {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub current_profit: f64,
    #[serde(default)]
    pub total_trades: u64,
    #[serde(default)]
    pub uptime: String,
    #[serde(default)]
    pub wallet_balance: f64,
}

/// Load a wallet keypair from disk.
///
/// Accepts both formats users actually have: the Solana CLI's JSON byte array
/// (`solana-keygen`) and a raw base58 secret key (Phantom/Solflare export).
pub fn load_keypair(path: &str) -> Result<Keypair> {
    let mut file = File::open(path)
        .with_context(|| format!("opening wallet file '{}'", path))?;
    let mut data = String::new();
    file.read_to_string(&mut data)
        .with_context(|| format!("reading wallet file '{}'", path))?;
    let data = data.trim();

    let bytes = if data.starts_with('[') {
        serde_json::from_str::<Vec<u8>>(data)
            .with_context(|| format!("parsing '{}' as a JSON keypair array", path))?
    } else {
        bs58::decode(data)
            .into_vec()
            .with_context(|| format!("decoding '{}' as a base58 secret key", path))?
    };

    Keypair::from_bytes(&bytes)
        .with_context(|| format!("'{}' did not contain a valid 64-byte keypair", path))
}

pub struct ArbitrageBot {
    connection: RpcClient,
    wallet_pubkey: solana_sdk::pubkey::Pubkey,
    start_time: DateTime<Utc>,
    strategies: Vec<Box<dyn Strategy>>,
    execution_engine: ExecutionEngine,
    price_source: Arc<dyn PriceSource>,
    ledger: TradeLedger,
    notifiers: Notifiers,
    mode: ExecutionMode,
    network: Network,
    trade_size_lamports: u64,
    status: BotStatus,
}

impl ArbitrageBot {
    /// Build a bot running the named strategy.
    ///
    /// Strategies are resolved through [`strategies::build`], so adding one
    /// requires no change here.
    pub fn new(
        mode: ExecutionMode,
        network: Network,
        trade_size_lamports: u64,
        strategy_name: &str,
    ) -> Result<Self> {
        let wallet_path = CONFIG
            .wallet_path
            .clone()
            .context("WALLET_PATH is not set; point it at your keypair file (see README)")?;
        let wallet = load_keypair(&wallet_path)?;
        let wallet_pubkey = wallet.pubkey();

        let connection = RpcClient::new(CONFIG.rpc_url_for(network));

        let price_source: Arc<dyn PriceSource> = Arc::new(JupiterPriceSource::new(
            CONFIG.jupiter_base_url.clone(),
            CONFIG.jupiter_api_key.clone(),
            CONFIG.slippage_bps,
            CONFIG.priority_fee_microlamports,
        )?);

        let execution_engine = ExecutionEngine::new(
            TransactionBuilder::new(wallet, CONFIG.priority_fee_microlamports),
            SafetyLimits {
                max_spend_lamports: CONFIG.max_spend_lamports,
                max_cumulative_loss_lamports: CONFIG.max_cumulative_loss_lamports,
            },
        );

        let strategy = strategies::build(
            strategy_name,
            CONFIG.min_profit_percentage,
            CONFIG.priority_fee_microlamports,
        )?;

        // A bad webhook URL fails here, at startup, rather than silently
        // never delivering an alert you were relying on.
        let mut targets: Vec<Box<dyn Notifier>> = Vec::new();
        if let Some(url) = &CONFIG.discord_webhook_url {
            targets.push(Box::new(DiscordNotifier::new(url.clone())?));
        }
        let notifiers = Notifiers::new(targets);

        Ok(Self {
            connection,
            wallet_pubkey,
            start_time: Utc::now(),
            strategies: vec![strategy],
            execution_engine,
            price_source,
            ledger: TradeLedger::load(TRADE_LOG_PATH)?,
            notifiers,
            mode,
            network,
            trade_size_lamports,
            status: BotStatus::default(),
        })
    }

    pub fn mode(&self) -> ExecutionMode {
        self.mode
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn wallet_pubkey(&self) -> solana_sdk::pubkey::Pubkey {
        self.wallet_pubkey
    }

    /// Fetch live round-trip quotes for every configured pair.
    ///
    /// Prices are always real, in every mode — quoting is read-only and risks
    /// nothing, and detection logic tested against invented prices proves
    /// nothing.
    pub async fn fetch_round_trips(&self) -> Result<Vec<RoundTrip>> {
        let mut out = Vec::new();

        for (base, quote_token) in default_pairs() {
            let label = format!("{}/{}", base.symbol, quote_token.symbol);

            let forward = match self
                .price_source
                .quote(base.mint, quote_token.mint, self.trade_size_lamports)
                .await
            {
                Ok(q) => q,
                Err(e) => {
                    log::warn!("quote {} leg 1 failed: {}", label, e);
                    continue;
                }
            };

            // Second leg must start from exactly what the first leg returns,
            // or the round trip is not a real one.
            let back = match self
                .price_source
                .quote(quote_token.mint, base.mint, forward.out_amount)
                .await
            {
                Ok(q) => q,
                Err(e) => {
                    log::warn!("quote {} leg 2 failed: {}", label, e);
                    continue;
                }
            };

            out.push(RoundTrip {
                label,
                forward,
                back,
            });
        }

        Ok(out)
    }

    /// Round trips rendered as price observations, for display and logging.
    pub fn as_price_data(round_trips: &[RoundTrip]) -> Vec<PriceData> {
        round_trips
            .iter()
            .map(|rt| PriceData {
                dex: "jupiter".to_string(),
                token_pair: rt.label.clone(),
                price: if rt.forward.in_amount == 0 {
                    0.0
                } else {
                    rt.forward.out_amount as f64 / rt.forward.in_amount as f64
                },
                timestamp: Utc::now().timestamp(),
            })
            .collect()
    }

    /// One scan: quote, detect, and act as far as the mode allows.
    pub async fn scan_once(&mut self) -> Result<()> {
        let round_trips = self.fetch_round_trips().await?;
        if round_trips.is_empty() {
            log::info!("no quotes available this cycle");
            return Ok(());
        }

        let mut best: Option<Route> = None;
        for strategy in &self.strategies {
            for route in strategy.find_opportunities(&round_trips).await? {
                if best.as_ref().map_or(true, |b| route.net_profit > b.net_profit) {
                    best = Some(route);
                }
            }
        }

        let route = match best {
            Some(r) => r,
            None => {
                log::info!(
                    "scanned {} pair(s), no net-profitable opportunity",
                    round_trips.len()
                );
                return Ok(());
            }
        };

        println!(
            "Opportunity [{}]: {} | in {:.6} SOL -> out {:.6} SOL",
            route.strategy,
            route.label,
            lamports_to_sol(route.amount_in),
            lamports_to_sol(route.amount_out),
        );
        println!(
            "  gross {:+} lamports - costs {} (base {} + priority {}) = net {:+} lamports ({:+.3}%)",
            route.gross_profit,
            route.costs.total_lamports(),
            route.costs.base_fee_lamports,
            route.costs.priority_fee_lamports,
            route.net_profit,
            route.expected_profit
        );

        let outcome = self
            .execution_engine
            .execute_route(
                &route,
                self.mode,
                &self.connection,
                self.price_source.as_ref(),
                &self.ledger,
            )
            .await?;

        self.record(&route, &outcome).await?;
        Ok(())
    }

    async fn record(&mut self, route: &Route, outcome: &ExecutionOutcome) -> Result<()> {
        let (outcome_label, signature) = match outcome {
            ExecutionOutcome::Detected => ("detected".to_string(), None),
            ExecutionOutcome::Simulated => {
                println!("  simulated OK (not submitted)");
                ("simulated".to_string(), None)
            }
            ExecutionOutcome::Submitted { signature } => {
                println!("  submitted: {}", signature);
                ("submitted".to_string(), Some(signature.clone()))
            }
            ExecutionOutcome::Refused(r) => {
                println!("  refused: {}", r);
                (format!("refused: {}", r), None)
            }
        };

        // Only a confirmed submission has a realised result; everything else
        // must stay None so it cannot skew the loss cap.
        let realised = match outcome {
            ExecutionOutcome::Submitted { .. } => Some(route.net_profit),
            _ => None,
        };

        // Snapshot the caps as they were when this trade was judged; config
        // can change, and a log entry must stay interpretable afterwards.
        let caps = self
            .execution_engine
            .limits()
            .snapshot(self.ledger.cumulative_loss_lamports());

        self.status.total_trades += 1;
        self.ledger.append(TradeRecord {
            timestamp: Utc::now().to_rfc3339(),
            mode: self.mode.to_string(),
            strategy: route.strategy.to_string(),
            label: route.label.clone(),
            amount_in: route.amount_in,
            expected_out: route.amount_out,
            gross_profit: route.gross_profit,
            net_profit: route.net_profit,
            expected_profit_pct: route.expected_profit,
            costs: route.costs,
            caps,
            realised_profit: realised,
            signature: signature.clone(),
            outcome: outcome_label.clone(),
        })?;

        self.alert(route, outcome, &outcome_label, signature).await;
        Ok(())
    }

    /// Send an alert for a decision, if any notifier is configured.
    async fn alert(
        &self,
        route: &Route,
        outcome: &ExecutionOutcome,
        outcome_label: &str,
        signature: Option<String>,
    ) {
        if self.notifiers.is_empty() {
            return;
        }

        // Severity follows what actually happened: a halt or failed
        // submission needs attention, a refusal is worth seeing, a detection
        // is routine.
        let (level, title) = match outcome {
            ExecutionOutcome::Detected => (Level::Info, "Opportunity detected"),
            ExecutionOutcome::Simulated => (Level::Notable, "Simulated OK"),
            ExecutionOutcome::Submitted { .. } => (Level::Notable, "Trade submitted"),
            ExecutionOutcome::Refused(Refusal::LossCapReached { .. }) => {
                (Level::Alert, "HALTED: loss cap reached")
            }
            ExecutionOutcome::Refused(Refusal::SimulationFailed(_)) => {
                (Level::Alert, "Simulation failed")
            }
            ExecutionOutcome::Refused(_) => (Level::Notable, "Trade refused"),
        };

        let mut n = Notification::new(
            level,
            format!("{} — {}", title, route.label),
            outcome_label.to_string(),
        )
        .field("Mode", self.mode.to_string())
        .field("Strategy", route.strategy.to_string())
        .field("In", format!("{:.6} SOL", lamports_to_sol(route.amount_in)))
        .field(
            "Net",
            format!("{:+} lamports ({:+.3}%)", route.net_profit, route.expected_profit),
        )
        .field(
            "Fees",
            format!(
                "{} ({}+{})",
                route.costs.total_lamports(),
                route.costs.base_fee_lamports,
                route.costs.priority_fee_lamports
            ),
        );

        if let Some(sig) = signature {
            n = n.field("Explorer", format!("https://solscan.io/tx/{}", sig));
        }

        self.notifiers.notify(n).await;
    }

    /// Continuous monitoring loop, until Ctrl-C.
    pub async fn monitor_markets(&mut self, poll_interval_secs: u64) -> Result<()> {
        self.status.running = true;
        println!(
            "Monitoring {} on {} (mode: {}), polling every {}s. Ctrl-C to stop.",
            self.price_source.name(),
            self.network,
            self.mode,
            poll_interval_secs
        );

        if !self.notifiers.is_empty() {
            println!("Alerts:      {}", self.notifiers.names().join(", "));
            self.notifiers
                .notify(
                    Notification::new(
                        Level::Info,
                        "Bot started",
                        format!("Watching for opportunities every {}s.", poll_interval_secs),
                    )
                    .field("Mode", self.mode.to_string())
                    .field("Network", self.network.to_string())
                    .field("Wallet", self.wallet_pubkey.to_string()),
                )
                .await;
        }

        let mut ticker =
            tokio::time::interval(std::time::Duration::from_secs(poll_interval_secs.max(1)));

        // Repeated failures are worth one alert, but alerting on every cycle
        // would turn an outage into a flood.
        let mut consecutive_failures: u32 = 0;
        const FAILURE_ALERT_THRESHOLD: u32 = 5;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match self.scan_once().await {
                        Ok(()) => consecutive_failures = 0,
                        Err(e) => {
                            // A single bad cycle must not kill a long-running bot.
                            log::error!("scan failed: {}", e);
                            consecutive_failures += 1;
                            if consecutive_failures == FAILURE_ALERT_THRESHOLD {
                                self.notifiers.notify(Notification::new(
                                    Level::Alert,
                                    "Repeated scan failures",
                                    format!("{} cycles in a row failed. Latest: {}", consecutive_failures, e),
                                )).await;
                            }
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nShutting down.");
                    self.status.running = false;
                    self.notifiers.notify(Notification::new(
                        Level::Info,
                        "Bot stopped",
                        "Shut down cleanly.",
                    )).await;
                    return Ok(());
                }
            }
        }
    }

    pub fn get_status(&self) -> serde_json::Value {
        let uptime = Utc::now()
            .signed_duration_since(self.start_time)
            .num_seconds();

        serde_json::json!({
            "status": if self.status.running { "running" } else { "stopped" },
            "uptime_seconds": uptime,
            "mode": self.mode.to_string(),
            "network": self.network.to_string(),
            "wallet": self.wallet_pubkey.to_string(),
            "total_trades": self.status.total_trades,
            "cumulative_loss_sol": lamports_to_sol(self.ledger.cumulative_loss_lamports()),
        })
    }

    pub fn ledger(&self) -> &TradeLedger {
        &self.ledger
    }

    pub async fn check_balance(&self) -> Result<f64> {
        let balance = self.connection.get_balance(&self.wallet_pubkey)?;
        Ok(lamports_to_sol(balance))
    }
}
