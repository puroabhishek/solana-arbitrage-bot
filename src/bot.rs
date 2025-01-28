use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{Keypair, Signer},
    pubkey::Pubkey,
};
use chrono::{DateTime, Utc};
use std::fs::File;
use std::io::Read;
use serde::{Serialize, Deserialize};
use crate::{
    config::CONFIG,
    types::{PriceData, Route},  // Add Route back
    strategies::{Strategy, two_hop::TwoHopStrategy},
    execution::{ExecutionEngine, transaction_builder::TransactionBuilder, mev_builder::MEVBuilder},
};

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

pub struct ArbitrageBot {
    connection: RpcClient,
    wallet: Keypair,
    start_time: DateTime<Utc>,
    strategies: Vec<Box<dyn Strategy>>,
    execution_engine: ExecutionEngine,
    mode: String,
    min_profit: f64,
    min_investment: f64,
    status: BotStatus,
}

impl ArbitrageBot {
    pub fn new() -> Result<Self> {
        let wallet_path = std::env::var("WALLET_PATH")?;
        let mut key_file = File::open(wallet_path)?;
        let mut key_data = String::new();
        key_file.read_to_string(&mut key_data)?;
        
        let wallet = Keypair::from_bytes(&bs58::decode(&key_data.trim()).into_vec()?)?;
        let connection = RpcClient::new(&CONFIG.rpc_url);
        
        let transaction_builder = TransactionBuilder::new(
            Pubkey::new_unique(),
            Keypair::new()
        );
        
        let mev_builder = Some(MEVBuilder::new("https://api.eden.network"));
        let execution_engine = ExecutionEngine::new(transaction_builder, mev_builder);

        Ok(Self {
            connection,
            wallet,
            start_time: Utc::now(),
            strategies: vec![Box::new(TwoHopStrategy::new(1.5))],
            execution_engine,
            mode: "devnet".to_string(),
            min_profit: 1.5,
            min_investment: 0.1,
            status: BotStatus::default(),
        })
    }

    pub async fn monitor_markets(&self, dry_run: bool) -> Result<()> {
        println!("Starting market monitoring on {} mode", self.mode);
        
        let prices = self.fetch_prices().await?;
        let mut best_route = None;
        let mut best_profit = self.min_profit;
        let mut selected_strategy = None;
        
        for strategy in &self.strategies {
            let opportunities = strategy.find_opportunities(&prices).await?;
            for route in opportunities {
                let profit = strategy.estimate_profit(&route)?;
                if profit > best_profit && route.steps[0].amount_in as f64 <= self.min_investment {
                    best_profit = profit;
                    best_route = Some(route);
                    selected_strategy = Some(strategy);
                }
            }
        }

        if let (Some(route), Some(strategy)) = (best_route, selected_strategy) {
            println!("Selected strategy: {}", strategy.name());
            println!("Expected profit: {}%", best_profit);
            
            if !dry_run {
                self.execution_engine.execute_route(&route, dry_run).await?;
            } else {
                println!("Dry run: Would execute trade with {}% profit using {}", 
                    best_profit, strategy.name());
            }
        }

        Ok(())
    }

    pub fn get_status(&self) -> serde_json::Value {
        let uptime = Utc::now()
            .signed_duration_since(self.start_time)
            .num_seconds();

        serde_json::json!({
            "status": if self.status.running { "running" } else { "stopped" },
            "uptime_seconds": uptime,
            "current_profit": self.status.current_profit,
            "total_trades": self.status.total_trades,
            "wallet_balance": self.status.wallet_balance,
            "mode": self.mode
        })
    }

    pub async fn fetch_prices(&self) -> Result<Vec<PriceData>> {
        Ok(vec![])
    }

    pub async fn check_balance(&self) -> Result<f64> {
        let balance = self.connection.get_balance(&self.wallet.pubkey())?;
        Ok(balance as f64 / 1e9)
    }
}