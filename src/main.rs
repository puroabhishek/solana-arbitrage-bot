use anyhow::{Context, Result};
use solana_arbitrage_bot::cli::BotInterface;
use std::fs;

/// Create the directories the bot actually writes to.
///
/// Only `data/` is used, for the trade ledger. Logging goes to stderr via
/// `RUST_LOG`, and configuration comes from `.env` — neither needs a
/// directory, so creating `logs/` and `config/` only implied storage that was
/// never written.
fn initialize_directories() -> Result<()> {
    fs::create_dir_all("data").context("Failed to create directory: data")?;
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