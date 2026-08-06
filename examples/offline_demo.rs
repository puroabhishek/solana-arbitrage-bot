//! Runs the real detection and safety pipeline against a deterministic price
//! source, with no network access and no funded wallet.
//!
//! Useful for seeing what the bot actually does — and for verifying the profit
//! math and safety gate on a machine that cannot reach Jupiter or an RPC node.
//!
//!     cargo run --example offline_demo

use anyhow::Result;
use solana_arbitrage_bot::{
    config::{lamports_to_sol, sol_to_lamports, ExecutionMode},
    execution::{transaction_builder::TransactionBuilder, ExecutionEngine, SafetyLimits},
    prices::{MockPriceSource, PriceSource, RoundTrip, SOL, USDC},
    strategies::{Strategy, TwoHopStrategy},
};
use solana_sdk::signature::Keypair;

/// Build a round trip the way the bot does: quote out, then quote the exact
/// proceeds back.
async fn round_trip(sol_to_usdc: f64, usdc_to_sol: f64, amount: u64) -> Result<RoundTrip> {
    let mut src = MockPriceSource::new(50);
    src.set_rate(SOL.mint, USDC.mint, sol_to_usdc);
    src.set_rate(USDC.mint, SOL.mint, usdc_to_sol);

    let forward = src.quote(SOL.mint, USDC.mint, amount).await?;
    let back = src.quote(USDC.mint, SOL.mint, forward.out_amount).await?;

    Ok(RoundTrip {
        label: format!("{}/{}", SOL.symbol, USDC.symbol),
        base_symbol: SOL.symbol.to_string(),
        quote_symbol: USDC.symbol.to_string(),
        base_decimals: SOL.decimals,
        quote_decimals: USDC.decimals,
        forward,
        back,
    })
}

async fn evaluate(name: &str, rt: RoundTrip, strategy: &TwoHopStrategy) -> Result<()> {
    println!("\n--- {} ---", name);
    println!("  {}", rt.price_summary());
    println!(
        "  in  {:.6} SOL -> {} USDC -> out {:.6} SOL",
        lamports_to_sol(rt.amount_in()),
        rt.forward.out_amount,
        lamports_to_sol(rt.amount_out())
    );

    let gross = rt.amount_out() as i64 - rt.amount_in() as i64;
    println!("  gross: {:+} lamports", gross);

    match strategy.find_opportunities(&[rt]).await?.first() {
        Some(route) => println!(
            "  ACTED ON [{}]: gross {:+} - fees {} (base {} + priority {}) = net {:+} ({:+.3}%)",
            route.strategy,
            route.gross_profit,
            route.costs.total_lamports(),
            route.costs.base_fee_lamports,
            route.costs.priority_fee_lamports,
            route.net_profit,
            route.expected_profit
        ),
        None => println!("  rejected: not net-profitable above the threshold"),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let size = sol_to_lamports(1.0);
    // ~150 USDC per SOL, expressed over base units (SOL 9dp, USDC 6dp).
    let strategy = TwoHopStrategy::new(0.0, 1_000);

    // One attempt costs 5000 base + 400 priority = 5400 lamports to land, so
    // any gross gain below that is really a loss.
    println!("Detection pipeline (mock prices, 1.0 SOL trade, 0% min profit)");
    println!("Cost to land one attempt: 5400 lamports");

    evaluate("Loses money outright", round_trip(0.15, 6.6, size).await?, &strategy).await?;
    evaluate("Gross gain BELOW the 5400 fee", round_trip(0.15, 6.6667, size).await?, &strategy).await?;
    evaluate("Gross gain just ABOVE the fee", round_trip(0.15, 6.66675, size).await?, &strategy).await?;
    evaluate("Genuinely profitable (~2%)", round_trip(0.15, 6.8, size).await?, &strategy).await?;

    // The safety gate, evaluated without touching the network.
    println!("\n\nSafety gate");
    let trade = sol_to_lamports(0.01);

    let uncapped = ExecutionEngine::new(
        TransactionBuilder::new(Keypair::new(), 1_000),
        SafetyLimits {
            max_spend_lamports: None,
            max_cumulative_loss_lamports: None,
        },
    );
    for mode in [
        ExecutionMode::Detect,
        ExecutionMode::Rehearse,
        ExecutionMode::Simulate,
        ExecutionMode::Live,
    ] {
        match uncapped.check_limits(mode, trade, 0) {
            Some(r) => println!("  {:9} -> refused: {}", mode.to_string(), r),
            None => println!("  {:9} -> would submit", mode.to_string()),
        }
    }

    println!("\n  with caps set (0.05 SOL per trade, 0.1 SOL total loss):");
    let capped = ExecutionEngine::new(
        TransactionBuilder::new(Keypair::new(), 1_000),
        SafetyLimits {
            max_spend_lamports: Some(sol_to_lamports(0.05)),
            max_cumulative_loss_lamports: Some(sol_to_lamports(0.1)),
        },
    );
    for (label, amount, loss) in [
        ("within caps", trade, 0),
        ("over per-trade cap", sol_to_lamports(0.06), 0),
        ("loss cap reached", trade, sol_to_lamports(0.1)),
    ] {
        match capped.check_limits(ExecutionMode::Live, amount, loss) {
            Some(r) => println!("  live/{:19} -> refused: {}", label, r),
            None => println!("  live/{:19} -> would submit", label),
        }
    }

    Ok(())
}
