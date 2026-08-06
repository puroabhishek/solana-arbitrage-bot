use anyhow::{Context, Result};
use solana_arbitrage_bot::cli::BotInterface;
use std::fs;

// Initialize required directories
fn initialize_directories() -> Result<()> {
    let required_dirs = vec!["config", "logs", "data"];
    
    for dir in required_dirs {
        fs::create_dir_all(dir)
            .context(format!("Failed to create directory: {}", dir))?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    // Default to `info` so the bot is observable out of the box. env_logger's
    // own default is error-only, which silently hides every info/warn line and
    // makes a running bot look like it is doing nothing. RUST_LOG still wins.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    // Initialize directories
    initialize_directories()
        .context("Failed to initialize required directories")?;

    // Run the bot interface
    BotInterface::run().await?;

    Ok(())
}