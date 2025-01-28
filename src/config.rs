use lazy_static::lazy_static;
use std::env;

pub struct Config {
    pub rpc_url: String,
    pub min_profit_percentage: f64,
}

lazy_static! {
    pub static ref CONFIG: Config = {
        dotenv::dotenv().ok();
        
        Config {
            rpc_url: env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.devnet.solana.com".to_string()),
            min_profit_percentage: env::var("MIN_PROFIT_PERCENTAGE")
                .unwrap_or_else(|_| "1.5".to_string())
                .parse()
                .unwrap_or(1.5),
        }
    };
}