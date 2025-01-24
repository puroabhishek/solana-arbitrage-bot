use solana_client::rpc_client::RpcClient;
use solana_sdk::{signature::Keypair, pubkey::Pubkey};
use anyhow::Result;
use crate::{
    config::CONFIG,
    types::{ArbitrageOpportunity, PriceData, Route},
    strategies::{Strategy, two_hop::TwoHopStrategy},
    execution::{
        ExecutionEngine,
        transaction_builder::TransactionBuilder,
        mev_builder::MEVBuilder,
    },
};
use std::time::SystemTime;
use std::fs::File;
use std::io::Read;

pub struct ArbitrageBot {
    connection: RpcClient,
    wallet: Keypair,
    start_time: SystemTime,
    strategies: Vec<Box<dyn Strategy>>,
    execution_engine: ExecutionEngine,
}

impl ArbitrageBot {
    pub fn new() -> Result<Self> {
        // Load wallet from file path in env
        let wallet_path = std::env::var("WALLET_PATH")?;
        let mut key_file = File::open(wallet_path)?;
        let mut key_data = String::new();
        key_file.read_to_string(&mut key_data)?;
        
        // Parse keypair from JSON file
        let wallet = Keypair::from_base58_string(&key_data);

        let connection = RpcClient::new(&CONFIG.rpc_url);
        let start_time = SystemTime::now();
        
        let program_id = Pubkey::new_unique(); // For testing
        let keypair = Keypair::new();

        println!("ArbitrageBot initialized");
        println!("Connection established to: {}", &CONFIG.rpc_url);

        let strategies: Vec<Box<dyn Strategy>> = vec![
            Box::new(TwoHopStrategy::new(1.5)),
            // Add other strategies
        ];

        let execution_engine = ExecutionEngine::new(
            TransactionBuilder::new(program_id, keypair),
            Some(MEVBuilder::new())
        );

        Ok(Self {
            connection,
            wallet,
            start_time,
            strategies,
            execution_engine,
        })
    }

    pub fn get_status(&self) -> serde_json::Value {
        let uptime = self.start_time
            .elapsed()
            .unwrap_or_default()
            .as_secs();

        serde_json::json!({
            "status": "running",
            "uptime_seconds": uptime,
            "connection": {
                "endpoint": &CONFIG.rpc_url,
                "network": if CONFIG.rpc_url.contains("devnet") { "devnet" } 
                          else if CONFIG.rpc_url.contains("mainnet") { "mainnet" }
                          else { "unknown" }
            },
            "config": {
                "min_profit_percentage": &CONFIG.min_profit_percentage
            }
        })
    }

    pub async fn monitor_markets(&self) -> Result<()> {
        println!("Monitoring markets...");
        
        // Fix the prices type
        let prices: Vec<PriceData> = self.fetch_prices().await?;

        // Find opportunities across all strategies
        for strategy in &self.strategies {
            if let Ok(opportunities) = strategy.find_opportunities(&prices) {
                for opportunity in opportunities {
                    if let Ok(route) = self.build_route(&opportunity) {
                        // Execute if profitable
                        if strategy.estimate_profit(&route)? > CONFIG.min_profit_percentage {
                            self.execution_engine.execute_route(route).await?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn find_arbitrage(&self) -> Result<Vec<ArbitrageOpportunity>> {
        // Placeholder for arbitrage detection logic
        Ok(vec![])
    }

    async fn fetch_prices(&self) -> Result<Vec<PriceData>> {
        // Implement price fetching logic
        Ok(vec![])
    }

    fn build_route(&self, opportunity: &ArbitrageOpportunity) -> Result<Route> {
        // Implement route building logic
        Ok(Route {
            steps: vec![],
            expected_profit: opportunity.profit_percentage,
        })
    }
} 