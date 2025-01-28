use clap::{Parser, Subcommand};
use dialoguer::{Input, Select, Confirm};
use console::Term;
use prettytable::{Table, row};
use anyhow::Result;
use crate::bot::ArbitrageBot;
use std::fs;
use serde_json::Value;

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
        /// Minimum profit percentage
        #[arg(short = 'p', long)]
        min_profit: Option<f64>,
        
        /// Maximum trade amount in SOL
        #[arg(short = 'a', long)]
        max_amount: Option<f64>,

        /// Dry run without executing trades
        #[arg(short, long)]
        dry_run: bool,

        /// Mode of operation (dev or mainnet)
        #[arg(short, long)]
        mode: String,
    },
    /// View transaction history
    History,
    /// Configure bot settings
    Configure,
    /// View current status
    Status,
}

pub struct BotInterface {
    bot: ArbitrageBot,
    term: Term,
}

impl BotInterface {
    pub fn new() -> Result<Self> {
        Ok(Self {
            bot: ArbitrageBot::new()?,
            term: Term::stdout(),
        })
    }

    pub async fn run() -> Result<()> {
        let cli = Cli::parse();
        let interface = Self::new()?;

        match cli.command {
            Commands::Start { min_profit, max_amount, dry_run, mode } => {
                interface.start_bot(min_profit, max_amount, dry_run, mode).await?;
            }
            Commands::History => {
                interface.show_history()?;
            }
            Commands::Configure => {
                interface.configure()?;
            }
            Commands::Status => {
                interface.show_status()?;
            }
        }

        Ok(())
    }

    async fn start_bot(&self, min_profit: Option<f64>, max_amount: Option<f64>, dry_run: bool, mode: String) -> Result<()> {
        let min_profit = if let Some(profit) = min_profit {
            profit
        } else {
            Input::new()
                .with_prompt("Enter minimum profit percentage")
                .default(1.5)
                .interact()?
        };

        let max_amount = if let Some(amount) = max_amount {
            amount
        } else {
            Input::new()
                .with_prompt("Enter maximum trade amount in SOL")
                .default(0.1)
                .interact()?
        };

        println!("Starting bot with:");
        println!("Minimum profit: {}%", min_profit);
        println!("Maximum trade amount: {} SOL", max_amount);
        println!("Mode: {}", mode);

        if !Confirm::new()
            .with_prompt("Continue with these settings?")
            .interact()? {
            return Ok(());
        }

        // Load DEX program IDs from a JSON file
        let dexes = self.load_dex_program_ids()?;
        println!("Loaded DEX program IDs: {:?}", dexes);

        // Start the bot with dry run option
        self.bot.monitor_markets(dry_run).await?;
        Ok(())
    }

    fn load_dex_program_ids(&self) -> Result<Value> {
        let dex_file = "config/dexes.json";
        let data = fs::read_to_string(dex_file)
            .map_err(|_| anyhow::anyhow!("dexes.json not found in config directory. Please create it first."))?;
        let dexes: Value = serde_json::from_str(&data)?;
        Ok(dexes)
    }

    fn show_history(&self) -> Result<()> {
        let mut table = Table::new();
        table.add_row(row!["Time", "Type", "Amount", "Profit", "Status"]);

        // Add sample data (replace with actual transaction history)
        table.add_row(row![
            "2024-03-14 10:30:00",
            "Two-Hop",
            "0.1 SOL",
            "+1.8%",
            "Success"
        ]);

        table.printstd();
        Ok(())
    }

    fn configure(&self) -> Result<()> {
        let options = vec!["RPC Endpoint", "Wallet", "Strategy Settings", "Network"];
        let selection = Select::new()
            .with_prompt("Select setting to configure")
            .items(&options)
            .interact()?;

        match selection {
            0 => self.configure_rpc()?,
            1 => self.configure_wallet()?,
            2 => self.configure_strategy()?,
            3 => self.configure_network()?,
            _ => unreachable!(),
        }

        Ok(())
    }

    fn configure_rpc(&self) -> Result<()> {
        let rpc_url: String = Input::new()
            .with_prompt("Enter RPC URL")
            .default(String::from("https://api.devnet.solana.com"))
            .interact()?;

        println!("RPC URL updated to: {}", rpc_url);
        // TODO: Save to config
        Ok(())
    }

    fn configure_wallet(&self) -> Result<()> {
        let wallet_path: String = Input::new()
            .with_prompt("Enter wallet path")
            .interact()?;

        println!("Wallet path updated to: {}", wallet_path);
        // TODO: Save to config
        Ok(())
    }

    fn configure_strategy(&self) -> Result<()> {
        let strategies = vec!["Two-Hop", "Triangle", "Multi-DEX"];
        let selection = Select::new()
            .with_prompt("Select strategy to configure")
            .items(&strategies)
            .interact()?;

        let min_profit: f64 = Input::new()
            .with_prompt("Enter minimum profit percentage")
            .default(1.5)
            .interact()?;

        println!("Strategy {} configured with min profit: {}%", strategies[selection], min_profit);
        Ok(())
    }

    fn configure_network(&self) -> Result<()> {
        let networks = vec!["Devnet", "Mainnet"];
        let selection = Select::new()
            .with_prompt("Select network")
            .items(&networks)
            .interact()?;

        println!("Network switched to: {}", networks[selection]);
        Ok(())
    }

    fn show_status(&self) -> Result<()> {
        let status = self.bot.get_status();
        self.term.write_line(&format!("Bot Status: {}", status))?;
        Ok(())
    }
}