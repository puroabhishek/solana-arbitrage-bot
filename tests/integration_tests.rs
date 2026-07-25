use solana_arbitrage_bot::bot::ArbitrageBot;
use anyhow::Result;
use solana_sdk::signature::Keypair;
use std::io::Write;

/// Writes a throwaway keypair to a temp file and points WALLET_PATH at it, so
/// tests don't depend on a real funded wallet or network access.
fn setup_test_wallet() -> Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!("solana-arbitrage-bot-test-wallet-{}.json", std::process::id()));
    let keypair = Keypair::new();
    let mut file = std::fs::File::create(&path)?;
    file.write_all(serde_json::to_string(&keypair.to_bytes().to_vec())?.as_bytes())?;

    std::env::set_var("WALLET_PATH", &path);
    std::env::set_var("SOLANA_RPC_URL", "https://api.devnet.solana.com");
    Ok(path)
}

#[tokio::test]
async fn test_bot_lifecycle() -> Result<()> {
    let wallet_path = setup_test_wallet()?;

    let bot = ArbitrageBot::new()?;
    assert!(bot.get_status().get("status").is_some());

    // fetch_prices/find_opportunities are currently stubs (see README/known
    // limitations) that return no data - this just verifies the call path
    // works end to end without panicking or requiring network access.
    let prices = bot.fetch_prices().await?;
    assert!(prices.is_empty());

    std::fs::remove_file(&wallet_path).ok();
    Ok(())
}
