use solana_arbitrage_bot::cli::BotInterface;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    BotInterface::run().await
} 