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
    env_logger::init();

    // Initialize directories
    initialize_directories()
        .context("Failed to initialize required directories")?;

    // Run the bot interface
    BotInterface::run().await?;

    Ok(())
}