//! End-to-end tests that need no network access and no funded wallet.
//!
//! Live prices are used in every real run of this bot, but tests deliberately
//! use `MockPriceSource` so the suite stays offline, fast and deterministic.

use anyhow::Result;
use solana_arbitrage_bot::{
    config::{sol_to_lamports, ExecutionMode, Network},
    execution::{
        ledger::TradeLedger, transaction_builder::TransactionBuilder, ExecutionEngine, Refusal,
        SafetyLimits,
    },
    prices::{MockPriceSource, PriceSource, RoundTrip, SOL, USDC},
    strategies::{self, Strategy, TradeCosts, TwoHopStrategy},
    types::CapSnapshot,
};
use solana_sdk::signature::Keypair;

fn engine(spend: Option<u64>, loss: Option<u64>) -> ExecutionEngine {
    ExecutionEngine::new(
        TransactionBuilder::new(Keypair::new(), 1_000),
        SafetyLimits {
            max_spend_lamports: spend,
            max_cumulative_loss_lamports: loss,
        },
    )
}

/// Build a round trip through the mock source, exactly as the bot would.
async fn round_trip(forward_rate: f64, back_rate: f64, amount: u64) -> Result<RoundTrip> {
    let mut src = MockPriceSource::new(50);
    src.set_rate(SOL.mint, USDC.mint, forward_rate);
    src.set_rate(USDC.mint, SOL.mint, back_rate);

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

#[tokio::test]
async fn detects_profitable_round_trip_end_to_end() -> Result<()> {
    // 2% round-trip gain: out and back at rates whose product exceeds 1.
    let rt = round_trip(100.0, 0.0102, sol_to_lamports(1.0)).await?;
    let strategy = TwoHopStrategy::new(1.0, 1_000);

    let routes = strategy.find_opportunities(&[rt]).await?;
    assert_eq!(routes.len(), 1, "expected one profitable route");
    assert!(routes[0].net_profit > 0);
    assert_eq!(routes[0].steps.len(), 2, "a round trip has two legs");
    Ok(())
}

#[tokio::test]
async fn ignores_break_even_round_trip() -> Result<()> {
    // Product of rates is exactly 1: gross break-even, so a net loss once
    // fees are counted.
    let rt = round_trip(100.0, 0.01, sol_to_lamports(1.0)).await?;
    let strategy = TwoHopStrategy::new(0.0, 1_000);

    let routes = strategy.find_opportunities(&[rt]).await?;
    assert!(
        routes.is_empty(),
        "break-even before fees must not be treated as an opportunity"
    );
    Ok(())
}

#[tokio::test]
async fn detect_and_simulate_never_submit() {
    let e = engine(Some(sol_to_lamports(1.0)), Some(sol_to_lamports(1.0)));

    for mode in [ExecutionMode::Detect, ExecutionMode::Simulate] {
        match e.check_limits(mode, sol_to_lamports(0.01), 0) {
            Some(Refusal::ModeDoesNotSubmit(_)) => {}
            other => panic!("{:?} should never submit, got {:?}", mode, other),
        }
    }
}

#[tokio::test]
async fn live_is_refused_without_caps() {
    let e = engine(None, None);
    assert_eq!(
        e.check_limits(ExecutionMode::Live, sol_to_lamports(0.01), 0),
        Some(Refusal::NoSpendCapConfigured),
        "live trading must be impossible without a spend cap"
    );

    let e = engine(Some(sol_to_lamports(1.0)), None);
    assert_eq!(
        e.check_limits(ExecutionMode::Live, sol_to_lamports(0.01), 0),
        Some(Refusal::NoLossCapConfigured),
        "live trading must be impossible without a loss cap"
    );
}

#[tokio::test]
async fn live_respects_spend_and_loss_caps() {
    let cap = sol_to_lamports(0.05);
    let loss_cap = sol_to_lamports(0.1);
    let e = engine(Some(cap), Some(loss_cap));

    // Over the per-trade cap.
    assert!(matches!(
        e.check_limits(ExecutionMode::Live, cap + 1, 0),
        Some(Refusal::ExceedsSpendCap { .. })
    ));

    // Within caps.
    assert_eq!(e.check_limits(ExecutionMode::Live, cap, 0), None);

    // Loss cap reached halts trading entirely.
    assert!(matches!(
        e.check_limits(ExecutionMode::Live, 1, loss_cap),
        Some(Refusal::LossCapReached { .. })
    ));
}

#[tokio::test]
async fn rehearse_needs_no_caps_since_it_spends_no_real_funds() {
    let e = engine(None, None);
    assert_eq!(e.check_limits(ExecutionMode::Rehearse, u64::MAX, 0), None);
}

#[tokio::test]
async fn strategy_is_resolved_by_name_and_tagged_on_the_route() -> Result<()> {
    // The bot only ever talks to the trait, so a strategy can be swapped in
    // by name without touching detection, execution or logging.
    let strategy = strategies::build("two-hop", 0.0, 1_000)?;
    assert_eq!(strategy.name(), "two-hop");

    let rt = round_trip(0.15, 6.8, sol_to_lamports(1.0)).await?;
    let routes = strategy.find_opportunities(&[rt]).await?;
    assert_eq!(routes.len(), 1);

    // Every route carries its origin, so the trade log can attribute it.
    assert_eq!(routes[0].strategy, "two-hop");
    Ok(())
}

#[test]
fn unknown_strategy_is_rejected_with_a_useful_message() {
    let err = match strategies::build("multi-exchange", 1.0, 1_000) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("an unimplemented strategy must not silently build"),
    };
    assert!(err.contains("unknown strategy"));
    assert!(err.contains("two-hop"), "should list what is available");
}

#[tokio::test]
async fn route_records_full_cost_breakdown() -> Result<()> {
    let strategy = strategies::build("two-hop", 0.0, 1_000)?;
    let rt = round_trip(0.15, 6.8, sol_to_lamports(1.0)).await?;
    let routes = strategy.find_opportunities(&[rt]).await?;
    let route = &routes[0];

    // Costs must be itemised, not just totalled, so a rejection is auditable.
    assert_eq!(route.costs.base_fee_lamports, 5_000);
    assert_eq!(route.costs.priority_fee_lamports, 400); // 1000 micro * 400k CU / 1e6
    assert_eq!(route.costs.total_lamports(), 5_400);
    assert_eq!(route.costs.compute_units, 400_000);

    // The expected-case arithmetic must hold exactly.
    assert_eq!(
        route.net_profit_expected,
        route.gross_profit - route.costs.total_lamports() as i64
    );

    // And the decision figure is the worst case, which is derived from the
    // guaranteed-minimum output rather than the expected one.
    assert_eq!(
        route.net_profit,
        route.amount_out_worst_case as i64
            - route.amount_in as i64
            - route.costs.total_lamports() as i64
    );
    assert!(
        route.net_profit <= route.net_profit_expected,
        "the decision must never be more optimistic than expectation"
    );
    Ok(())
}

#[tokio::test]
async fn leg_prices_are_decimal_adjusted_and_recorded() -> Result<()> {
    // 1 SOL (9dp) -> 150 USDC (6dp). In raw base units that ratio is 0.15,
    // which is meaningless to a human; decimal-adjusted it must read as 150.
    let rt = round_trip(0.15, 6.8, sol_to_lamports(1.0)).await?;

    assert!(
        (rt.forward_rate() - 150.0).abs() < 0.01,
        "forward rate should be ~150 USDC/SOL, got {}",
        rt.forward_rate()
    );
    // Selling back: 6.8 raw ratio -> 0.0068 SOL per USDC, i.e. ~147 USDC/SOL.
    assert!(
        (rt.back_rate() - 0.0068).abs() < 0.0001,
        "back rate should be ~0.0068 SOL/USDC, got {}",
        rt.back_rate()
    );
    // Inverted, the return leg is directly comparable to the outbound one.
    assert!(
        (rt.back_rate_inverted() - 147.06).abs() < 0.1,
        "inverted back rate should be ~147 USDC/SOL, got {}",
        rt.back_rate_inverted()
    );

    // And both legs must reach the trade log with those prices attached.
    let strategy = strategies::build("two-hop", 0.0, 1_000)?;
    let routes = strategy.find_opportunities(&[rt]).await?;
    let legs = &routes[0].legs;

    assert_eq!(legs.len(), 2, "a round trip records both legs");
    assert_eq!(legs[0].from, "SOL");
    assert_eq!(legs[0].to, "USDC");
    assert!((legs[0].ui_amount_in - 1.0).abs() < 1e-9, "1 SOL in");
    assert!((legs[0].rate - 150.0).abs() < 0.01);
    assert_eq!(legs[1].from, "USDC");
    assert_eq!(legs[1].to, "SOL");
    Ok(())
}

#[test]
fn cap_snapshot_captures_limits_in_force() {
    let limits = SafetyLimits {
        max_spend_lamports: Some(sol_to_lamports(0.05)),
        max_cumulative_loss_lamports: Some(sol_to_lamports(0.1)),
    };
    let snap = limits.snapshot(sol_to_lamports(0.02));

    assert_eq!(snap.max_spend_lamports, Some(sol_to_lamports(0.05)));
    assert_eq!(snap.max_cumulative_loss_lamports, Some(sol_to_lamports(0.1)));
    assert_eq!(snap.cumulative_loss_at_evaluation, sol_to_lamports(0.02));
}

#[test]
fn modes_imply_sensible_networks() {
    // Rehearsal is devnet by definition; real trading needs mainnet liquidity.
    assert_eq!(ExecutionMode::Rehearse.implied_network(), Network::Devnet);
    assert_eq!(ExecutionMode::Simulate.implied_network(), Network::Mainnet);
    assert_eq!(ExecutionMode::Live.implied_network(), Network::Mainnet);
}

#[test]
fn ledger_loss_survives_reload() -> Result<()> {
    use solana_arbitrage_bot::types::TradeRecord;

    let dir = std::env::temp_dir().join(format!("arb-int-{}", std::process::id()));
    let path = dir.join("trades.json");
    std::fs::remove_dir_all(&dir).ok();

    let mut ledger = TradeLedger::load(&path)?;
    ledger.append(TradeRecord {
        timestamp: "2026-01-01T00:00:00Z".into(),
        mode: "live".into(),
        strategy: "two-hop".into(),
        label: "SOL/USDC".into(),
        legs: Vec::new(),
        amount_in: sol_to_lamports(0.01),
        expected_out: sol_to_lamports(0.011),
        worst_case_out: sol_to_lamports(0.0105),
        gross_profit: sol_to_lamports(0.001) as i64,
        net_profit: sol_to_lamports(0.0004) as i64,
        net_profit_expected: sol_to_lamports(0.0009) as i64,
        expected_profit_pct: 1.0,
        costs: TradeCosts::estimate(1, 1_000, 400_000),
        caps: CapSnapshot {
            max_spend_lamports: Some(sol_to_lamports(0.05)),
            max_cumulative_loss_lamports: Some(sol_to_lamports(0.1)),
            cumulative_loss_at_evaluation: 0,
        },
        realised_profit: Some(-(sol_to_lamports(0.002) as i64)),
        signature: None,
        outcome: "submitted".into(),
    })?;

    // A cap that forgets on restart is not a cap.
    let reloaded = TradeLedger::load(&path)?;
    assert_eq!(reloaded.cumulative_loss_lamports(), sol_to_lamports(0.002));

    std::fs::remove_dir_all(&dir).ok();
    Ok(())
}

/// Verifies the Discord notifier produces a well-formed webhook request.
///
/// Discord itself is unreachable from CI, so this points the notifier at a
/// local socket and inspects exactly what goes on the wire.
#[tokio::test]
async fn discord_notifier_sends_wellformed_webhook() -> Result<()> {
    use solana_arbitrage_bot::notify::{DiscordNotifier, Level, Notification, Notifier};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = sock.read(&mut buf).await.unwrap();
        sock.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8_lossy(&buf[..n]).to_string()
    });

    // https:// is enforced, so exercise the payload via the same code path
    // with a plain-HTTP override only a test can construct.
    let notifier = DiscordNotifier::new_unchecked(format!("http://{}/webhook", addr))?;
    notifier
        .send(
            &Notification::new(Level::Alert, "HALTED: loss cap reached", "stopping")
                .field("Mode", "live")
                .field("Net", "-1234 lamports"),
        )
        .await?;

    let request = server.await?;
    assert!(request.starts_with("POST /webhook"), "got: {}", request);
    assert!(request.contains("content-type: application/json"));

    let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(body)?;
    let embed = &json["embeds"][0];

    assert_eq!(embed["title"], "HALTED: loss cap reached");
    assert_eq!(embed["description"], "stopping");
    assert_eq!(embed["color"], 0xED4245); // alert red
    assert_eq!(embed["fields"][0]["name"], "Mode");
    assert_eq!(embed["fields"][1]["value"], "-1234 lamports");
    Ok(())
}
