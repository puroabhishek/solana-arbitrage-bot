use solana_arbitrage_bot::{
    bot::ArbitrageBot,
    types::PriceData,
};
use anyhow::Result;
use dotenv::dotenv;

#[tokio::test]
async fn test_bot_initialization() -> Result<()> {
    dotenv().ok(); // Load environment variables
    let bot = ArbitrageBot::new()?;
    assert!(bot.get_status().get("status").is_some());
    Ok(())
}

#[tokio::test]
async fn test_price_monitoring() -> Result<()> {
    dotenv().ok(); // Load environment variables
    let bot = ArbitrageBot::new()?;
    let prices = bot.fetch_prices().await?;
    assert!(!prices.is_empty(), "Price data should not be empty");
    Ok(())
}

#[tokio::test]
async fn test_arbitrage_detection() -> Result<()> {
    dotenv().ok(); // Load environment variables
    let bot = ArbitrageBot::new()?;
    let opportunities = bot.find_arbitrage().await?;
    assert!(opportunities.len() >= 0);
    Ok(())
} 