use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::Term;
use dialoguer::Confirm;
use prettytable::{row, Table};

use crate::bot::ArbitrageBot;
use crate::config::{lamports_to_sol, sol_to_lamports, ExecutionMode, Network, CONFIG};
use crate::execution::ledger::TradeLedger;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the arbitrage bot
    Start {
        /// Minimum net profit percentage required to act
        #[arg(short = 'p', long)]
        min_profit: Option<f64>,

        /// Trade size in SOL
        #[arg(short = 'a', long)]
        amount: Option<f64>,

        /// How far to go: detect | rehearse | simulate | live
        #[arg(short = 'm', long, default_value = "detect")]
        mode: String,

        /// Cluster override: devnet | mainnet. Defaults to the mode's cluster.
        #[arg(short = 'n', long)]
        network: Option<String>,

        /// Seconds between scans
        #[arg(long)]
        interval: Option<u64>,

        /// Skip the confirmation prompt (required when not on a terminal)
        #[arg(short = 'y', long)]
        yes: bool,

        /// Run a single scan and exit instead of looping
        #[arg(long)]
        once: bool,
    },
    /// View recorded trade history
    History,
    /// View current status
    Status {
        /// Mode to report status for
        #[arg(short = 'm', long, default_value = "detect")]
        mode: String,
    },
}

pub struct BotInterface;

impl BotInterface {
    pub async fn run() -> Result<()> {
        let cli = Cli::parse();

        match cli.command {
            Commands::Start {
                min_profit,
                amount,
                mode,
                network,
                interval,
                yes,
                once,
            } => {
                Self::start(min_profit, amount, &mode, network.as_deref(), interval, yes, once)
                    .await
            }
            Commands::History => Self::show_history(),
            Commands::Status { mode } => Self::show_status(&mode).await,
        }
    }

    async fn start(
        min_profit: Option<f64>,
        amount: Option<f64>,
        mode: &str,
        network: Option<&str>,
        interval: Option<u64>,
        yes: bool,
        once: bool,
    ) -> Result<()> {
        let mode: ExecutionMode = mode.parse()?;

        // The mode implies its cluster, so the two cannot silently disagree —
        // but an explicit override still wins.
        let network: Network = match network {
            Some(n) => n.parse()?,
            None => mode.implied_network(),
        };

        let trade_size_sol = amount.unwrap_or(0.01);
        let trade_size_lamports = sol_to_lamports(trade_size_sol);
        let min_profit = min_profit.unwrap_or(CONFIG.min_profit_percentage);
        let interval = interval.unwrap_or(CONFIG.poll_interval_secs);

        println!("Mode:        {}", mode);
        println!("Network:     {}", network);
        println!("Trade size:  {} SOL", trade_size_sol);
        println!("Min profit:  {}%", min_profit);

        if mode.risks_real_funds() {
            Self::print_live_warning(trade_size_lamports);
        }

        if !Self::confirm(yes, mode)? {
            println!("Aborted.");
            return Ok(());
        }

        let mut bot = ArbitrageBot::new(mode, network, trade_size_lamports)?;
        println!("Wallet:      {}", bot.wallet_pubkey());

        match bot.check_balance().await {
            Ok(balance) => println!("Balance:     {:.6} SOL", balance),
            // A balance read failing is not fatal in non-spending modes.
            Err(e) => println!("Balance:     unavailable ({})", e),
        }

        if once {
            bot.scan_once().await
        } else {
            bot.monitor_markets(interval).await
        }
    }

    fn print_live_warning(trade_size_lamports: u64) {
        println!();
        println!("  ***  LIVE MODE — REAL FUNDS WILL BE SPENT  ***");
        match CONFIG.max_spend_lamports {
            Some(cap) => println!(
                "  Per-trade cap:   {:.6} SOL (this trade: {:.6} SOL)",
                lamports_to_sol(cap),
                lamports_to_sol(trade_size_lamports)
            ),
            None => println!("  Per-trade cap:   NOT SET — trades will be refused"),
        }
        match CONFIG.max_cumulative_loss_lamports {
            Some(cap) => println!("  Total loss cap:  {:.6} SOL", lamports_to_sol(cap)),
            None => println!("  Total loss cap:  NOT SET — trades will be refused"),
        }
        println!();
    }

    /// Ask for confirmation, tolerating a non-interactive environment.
    ///
    /// The previous implementation called `Confirm` unconditionally, which
    /// hard-fails with "not a terminal" when piped — making unattended
    /// operation impossible for what is meant to be a long-running bot.
    fn confirm(yes: bool, mode: ExecutionMode) -> Result<bool> {
        if yes {
            return Ok(true);
        }
        if !console::user_attended() {
            // Refuse only where real money is at stake; safe modes proceed.
            if mode.risks_real_funds() {
                anyhow::bail!(
                    "live mode needs confirmation but no terminal is attached; pass --yes to proceed"
                );
            }
            return Ok(true);
        }
        Confirm::new()
            .with_prompt("Continue with these settings?")
            .default(false)
            .interact()
            .context("reading confirmation")
    }

    fn show_history() -> Result<()> {
        let ledger = TradeLedger::load(crate::bot::TRADE_LOG_PATH)?;
        let records = ledger.records();

        if records.is_empty() {
            println!("No trades recorded yet.");
            return Ok(());
        }

        let mut table = Table::new();
        table.add_row(row!["Time", "Mode", "Pair", "In (SOL)", "Expected", "Outcome"]);
        for r in records {
            table.add_row(row![
                r.timestamp,
                r.mode,
                r.label,
                format!("{:.6}", lamports_to_sol(r.amount_in)),
                format!("{:+.3}%", r.expected_profit_pct),
                r.outcome,
            ]);
        }
        table.printstd();

        println!(
            "\nCumulative realised loss: {:.6} SOL",
            lamports_to_sol(ledger.cumulative_loss_lamports())
        );
        Ok(())
    }

    async fn show_status(mode: &str) -> Result<()> {
        let mode: ExecutionMode = mode.parse()?;
        let bot = ArbitrageBot::new(mode, mode.implied_network(), sol_to_lamports(0.01))?;
        let term = Term::stdout();
        term.write_line(&format!(
            "{}",
            serde_json::to_string_pretty(&bot.get_status())?
        ))?;
        Ok(())
    }
}
