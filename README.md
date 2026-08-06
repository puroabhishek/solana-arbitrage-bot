# Solana Arbitrage Bot

A Rust CLI bot that scans live Solana DEX prices via the Jupiter aggregator for
circular arbitrage opportunities, and can execute them under strict safety caps.

> **Read this before running anything with real funds.** Mainnet arbitrage is a
> latency race against well-capitalised bots on co-located infrastructure. This
> bot is correct and careful, but correctness is the entry fee, not an edge —
> most opportunities it detects will be gone before a transaction lands. Treat
> any capital you point at it as capital you can afford to lose entirely.

## Execution modes

The bot works as a ladder. Each rung tests something the ones below it cannot,
and you are meant to promote through them in order.

| Mode | Prices | Transaction | Submitted | Risk |
| --- | --- | --- | --- | --- |
| `detect` (default) | Real, live | none | no | none |
| `rehearse` | Real, live | real, non-swap, devnet | **yes, to devnet** | none (test SOL) |
| `simulate` | Real, live | real mainnet swap | no | none |
| `live` | Real, live | real mainnet swap | **yes** | **real funds** |

**Prices are always real, in every mode.** Quoting is a read-only HTTP call that
spends nothing, and detection logic tested against invented prices proves
nothing. What changes between modes is only whether a transaction is submitted.

Two details worth understanding:

- **`rehearse` cannot submit a real swap.** A swap transaction encodes mainnet
  pool accounts that do not exist on devnet. Instead it submits a real but
  trivial transaction (a zero-value self-transfer) carrying the same
  compute-budget and priority-fee instructions, which exercises the entire
  sign → submit → confirm → log path for free. It proves the *submission
  machinery*; `simulate` proves the *swap itself*.
- **`simulate` always runs before `live` submits.** Even in live mode, the
  transaction is simulated against the cluster first, and submission is aborted
  if simulation fails.

## Safety

`live` mode refuses to submit anything unless **both** caps are configured:

- `MAX_TRADE_AMOUNT_SOL` — per-trade ceiling
- `MAX_CUMULATIVE_LOSS_SOL` — total realised loss before the bot halts

The loss cap is tracked in `data/trades.json` and reloaded at startup, so it
survives restarts and crashes — a cap that resets on restart is not a cap.
Profits do **not** top the budget back up; it limits how much this bot may lose
while proving itself, not running P&L. Once tripped, the bot halts until you
clear that file deliberately.

## Requirements

- Rust (stable) and Cargo
- A Solana RPC endpoint. The public devnet endpoint is fine for testing; real
  arbitrage scanning needs a low-latency provider (Helius, QuickNode, Triton),
  which generally means signing up for an API key.
- A Solana wallet keypair (see below)

## Wallet setup

This bot is a local process that reads a private key from a file — it does not
use a browser wallet adapter. That shapes which wallets are usable:

- **Phantom** and **Solflare** both work. Either can export a base58 private
  key that this bot accepts.
- **MetaMask is not usable.** It is an EVM wallet; Solana uses ed25519 keys,
  which MetaMask does not natively hold.

**Use a dedicated wallet.** Do not point this at a wallet holding funds you
care about — generate a fresh keypair and fund it with only what you are
willing to lose.

### Option A — generate a dedicated keypair (recommended)

The key never passes through a browser export dialog:

```bash
solana-keygen new --outfile ~/.config/solana/bot-wallet.json
chmod 600 ~/.config/solana/bot-wallet.json
```

Import it into Solflare or Phantom afterwards if you want a UI to manage it.

### Option B — export from Solflare or Phantom

In the wallet: **Settings → Export Private Key**, then save the base58 string
to a plain text file and restrict it:

```bash
chmod 600 ~/.config/solana/bot-wallet.txt
```

Both formats are accepted — the loader detects a JSON byte array (starting
with `[`) and otherwise treats the file as a base58 secret key.

### Funding

Switch your wallet to **Devnet** for testing, then airdrop test SOL:

```bash
solana airdrop 1 <bot-wallet-address> --url devnet
```

For mainnet, send a small, deliberate amount from your main wallet to the bot
wallet's public address.

## Setup

Fastest path — checks prerequisites, creates a dedicated wallet, writes a
working `.env`, and builds. Safe to re-run; it never overwrites an existing
wallet or `.env`:

```bash
./scripts/setup.sh
```

<details>
<summary>Or do it manually</summary>

```bash
cp .env.example .env
solana-keygen new --outfile ~/.config/solana/bot-wallet.json
chmod 600 ~/.config/solana/bot-wallet.json
# Set WALLET_PATH in .env to that ABSOLUTE path ("~" is not expanded)
cargo build --release
```
</details>

Then work up the ladder, one rung at a time:

```bash
cargo run --example offline_demo               # 1. logic, no network or funds
cargo run --release -- start                   # 2. live prices, spends nothing
cargo run --release -- start --mode rehearse --yes   # 3. real devnet submit
cargo run --release -- start --mode simulate --yes   # 4. real tx, not sent
cargo run --release -- start --mode live --yes       # 5. real money
```

Step 3 needs devnet SOL:
`solana airdrop 1 $(solana-keygen pubkey ~/.config/solana/bot-wallet.json) --url devnet`

Do not skip rungs — each catches a different class of failure, and only step 5
can cost you anything.

## Configuration

All runtime configuration comes from environment variables in `.env` (see
`.env.example` for the annotated list):

| Variable | Purpose | Default |
| --- | --- | --- |
| `WALLET_PATH` | Path to the wallet keypair file | *(required)* |
| `SOLANA_RPC_URL` | RPC override | cluster default |
| `SOLANA_NETWORK` | `devnet` / `mainnet` | implied by mode |
| `JUPITER_API_KEY` | Jupiter paid tier; unset uses the free tier | *(unset)* |
| `MIN_PROFIT_PERCENTAGE` | Minimum **net** profit to act on | `1.5` |
| `SLIPPAGE_BPS` | Slippage tolerance | `50` |
| `PRIORITY_FEE_MICROLAMPORTS` | Priority fee per compute unit | `1000` |
| `POLL_INTERVAL_SECS` | Seconds between scans | `10` |
| `MAX_TRADE_AMOUNT_SOL` | Per-trade cap — **required for `live`** | *(unset)* |
| `MAX_CUMULATIVE_LOSS_SOL` | Total loss cap — **required for `live`** | *(unset)* |

Notes:

- `WALLET_PATH` does **not** expand `~` — use an absolute path.
- `config/config.json` is **not read by the bot**. Only `.env` matters.
- Leaving `JUPITER_API_KEY` unset uses Jupiter's free tier
  (`lite-api.jup.ag`), which needs no signup but is rate limited. Setting a key
  switches to the paid host automatically.

## Usage

```bash
cargo run -- start                          # detect (default): watch only
cargo run -- start --mode rehearse --yes    # real devnet submit, free test SOL
cargo run -- start --mode simulate --yes    # real mainnet tx, not submitted
cargo run -- start --mode live --yes        # real trade (needs both caps set)

cargo run -- start --once                   # single scan instead of looping
cargo run -- start -a 0.05 -p 2.0           # 0.05 SOL trades, 2% min net profit
cargo run -- status                         # status as JSON
cargo run -- history                        # recorded trades + realised loss
```

Pass `--yes` to skip the confirmation prompt. The bot runs unattended: when no
terminal is attached it proceeds automatically in the safe modes, and refuses to
start in `live` without `--yes` rather than hanging on a prompt nothing can
answer.

## Testing

```bash
cargo build
cargo test
```

Tests need no network access, no funded wallet and no API key — they run against
a deterministic mock price source and cover the profit math (including that a
gross gain smaller than fees is *not* an opportunity), the lamports/SOL
boundary, and every safety refusal.

## Security

- Never commit wallet keys. `.gitignore` covers `wallet.json` and `.env`;
  make sure whatever filename you choose is also covered.
- A private key and an SSH keypair were previously committed to this
  repository. They have been removed from the working tree, but **remain in
  git history** — treat them as compromised and rotate them.
- Prefer a dedicated, minimally funded wallet over your primary one.
- Never paste a paid RPC URL containing an embedded API key into a shared
  channel — it belongs in `.env` only.

## Discord alerts

The bot can post to a Discord channel so you do not have to watch a terminal.
It uses an **incoming webhook** — no bot token, no OAuth, no persistent gateway
connection, just an HTTPS POST.

### Setup

**You must own or admin the server.** Webhooks are created from a server's own
settings, so if your account only *joins* other people's servers there will be
no Integrations menu to find. Making your own server is free and instant.

Also note: **webhook creation is desktop/browser only.** The Discord mobile app
does not expose it.

1. **Have a server you own.** In the left sidebar click **`+`** →
   **Create My Own** → skip the questions → name it anything. You are now its
   owner, which grants Manage Webhooks automatically.
2. **Pick or create a text channel** in that server (e.g. `#bot-alerts`).
3. **Hover the channel → ⚙ Edit Channel → Integrations → Webhooks →
   New Webhook.**
4. Click the webhook, then **Copy Webhook URL**. It looks like:
   ```
   https://discord.com/api/webhooks/1234567890123456789/AbCdEf-long_token_here
   ```
   Both the numeric ID *and* the long token after it are required — a
   truncated copy fails with 401 or 404.
5. **Put it in `.env`:**
   ```
   DISCORD_WEBHOOK_URL=https://discord.com/api/webhooks/.../...
   ```
6. **Verify it:**
   ```bash
   cargo run -- test-alert
   ```
   This touches no wallet, RPC or price source, so anything that goes wrong is
   unambiguously a notification problem. It prints the specific cause on
   failure.

Treat that URL as a secret — anyone holding it can post to your channel. Leave
it unset to disable alerts.

<details>
<summary>Testing the webhook without the bot</summary>

To rule the bot out entirely, post to the webhook directly:

```bash
curl -X POST -H "Content-Type: application/json" \
  -d '{"content":"hello from curl"}' \
  "https://discord.com/api/webhooks/.../..."
```

A `204 No Content` means the webhook is good and the problem is configuration.
A `401`/`404` means the URL is wrong, truncated, or the webhook was deleted.
</details>

You get pinged for:

| Event | Severity |
| --- | --- |
| Bot started / stopped | info |
| Opportunity detected | info |
| Simulated OK | notable |
| Trade submitted (with a Solscan link) | notable |
| Trade refused (cap exceeded, etc.) | notable |
| **Halted — loss cap reached** | alert |
| **Simulation failed** | alert |
| 5 consecutive scan failures | alert |

Two deliberate behaviours: notification failures are logged and swallowed, so a
Discord outage can never stop or crash trading; and repeated scan failures
alert **once** at a threshold rather than every cycle, so an outage does not
become a flood.

## Strategies

Pick one with `--strategy`; list them with `cargo run -- strategies`.

| Strategy | Description |
| --- | --- |
| `two-hop` (default) | `A -> B -> A` round trip through the aggregator |

Only two-hop is implemented. Triangular and cross-venue strategies fit the same
trait and are deliberately absent rather than present as non-working stubs.

### Adding one

Everything else — detection loop, execution gate, safety caps, logging, CLI —
works against the `Strategy` trait, not any concrete strategy. Adding one is
two steps:

1. Implement `Strategy` (see `src/strategies/two_hop.rs`):
   ```rust
   #[async_trait]
   impl Strategy for TriangularStrategy {
       fn name(&self) -> &'static str { "triangular" }
       async fn find_opportunities(&self, rts: &[RoundTrip]) -> Result<Vec<Route>> { ... }
       fn estimate_profit(&self, route: &Route) -> Result<f64> { ... }
   }
   ```
2. Register it in `strategies::build` and add it to `strategies::AVAILABLE`
   (`src/strategies/mod.rs`).

Set `Route::strategy` to your `name()` and every trade is automatically
attributed to it in the log. Use `TradeCosts::estimate` so costs are itemised
consistently — the profit check must be **net**, or the strategy will report
wins it did not earn.

## Seeing what the bot is doing

Logs default to `info`, so a running bot reports every scan without extra
configuration. Each cycle logs both legs and their prices, whether or not the
round trip was profitable:

```
INFO SOL/USDC: SOL -> USDC @ 150.230000 | USDC -> SOL @ 0.006797 (= 147.13 USDC/SOL)
     | in 0.010000 SOL -> 1.502300 USDC -> 0.010210 SOL | gross +210000 lamports
```

Reading it: the bot bought at **150.23 USDC per SOL** and could sell back at an
effective **147.13 USDC per SOL**. Buying high and selling back low sounds
wrong, but it is what profit looks like here — the cheaper SOL is on the return
leg, the more of it your USDC buys back.

Verbosity:

```bash
RUST_LOG=warn cargo run -- start     # quieter: problems only
RUST_LOG=debug cargo run -- start    # louder
cargo run -- start > bot.log 2>&1    # to a file
```

Prices also land in the trade log whenever an opportunity is found, and
`cargo run -- history` shows them per leg under **Path (buy @ / sell @)**.

## Choosing your thresholds

Run this before setting anything:

```bash
cargo run -- breakeven
```

It prints the minimum viable edge and trade size for your actual settings,
using the same cost model the strategy does.

Two relationships decide everything:

```
minimum edge:  p_min = 2σ + F / (w × S)
minimum size:  S_min = F / (w × (p − 2σ))

σ = slippage per leg   F = fixed cost   w = win rate   S = trade size
```

**Slippage sets an irreducible floor.** A round trip has two legs and each may
fill up to the tolerance worse than quoted, so `2 × SLIPPAGE_BPS` is lost before
any fee. At the default 50 bps that floor is **1%** — and an edge below it is
impossible at *any* trade size. The bot refuses to start if
`MIN_PROFIT_PERCENTAGE` does not clear it.

**Fixed costs decide minimum size**, because they do not scale with the trade.
They dominate small trades and vanish on large ones.

**Jito bundles change the arithmetic entirely.** Solana charges fees whether a
transaction succeeds or fails, so on the naked path every lost race is a direct
bleed. A bundle that loses its auction commits nothing and costs nothing — which
means `w = 1` for cost purposes, and slippage can be set tight because a revert
is free.

| Setting | Suggested | Why |
| --- | --- | --- |
| `USE_JITO_BUNDLES` | `true` (default) | Atomic, and lost races are free |
| `SLIPPAGE_BPS` | 10–20 | Tight is affordable once reverts cost nothing |
| `MIN_PROFIT_PERCENTAGE` | 0.5 | Clears the 0.2–0.4% floor with margin |
| Trade size | ≥ 0.05 SOL | Keeps fixed costs immaterial |

Tightening slippage from 50 to 10 bps turns a 0.25% edge from *impossible at any
size* into viable at ~0.011 SOL. That single setting matters more than any
other.

## Understanding the numbers

Amounts are in **lamports**, the smallest unit of SOL:
**1 SOL = 1,000,000,000 lamports.**

Every trade costs a fixed amount to land regardless of size:

| Component | Typical | What it is |
| --- | --- | --- |
| Base fee | 5,000 lamports | Solana's per-signature charge |
| Priority fee | 400 lamports | `PRIORITY_FEE_MICROLAMPORTS` × compute units ÷ 1e6 |
| **Total** | **~5,400 lamports** | Subtracted before a trade is judged profitable |

That is ~0.0000054 SOL — trivial in absolute terms, decisive on a thin margin.
Trading 0.01 SOL for a 1% edge earns ~100,000 lamports gross, so fees are ~5%
of the profit; on a thinner edge they turn a "win" into a loss. This is why the
bot rejects a +5,000 lamport gross gain: landing it costs more than it makes.

`cargo run -- history` shows gross, fees (itemised), net, and the caps in force
for every trade, so any decision can be audited after the fact:

```
| Mode   | Strategy | Pair     | In (SOL) | Gross   | Fees            | Net     | Net %   | Cap    | Outcome
| detect | two-hop  | SOL/USDC | 0.010000 | +210000 | 5400 (5000+400) | +204600 | +2.046% | 0.0500 | detected
| live   | two-hop  | SOL/USDC | 0.060000 | +1300000| 5400 (5000+400) | +1294600| +2.158% | 0.0500 | refused: above the 0.05 SOL cap
```

## Known limitations

- **MEV/Jito bundle submission** (`src/execution/mev_builder.rs`) is unchanged
  scaffolding, not wired into the execution path.
- **Only two-hop (`A -> B -> A`) routes** are implemented. Triangular routes
  across three tokens are not — see [Adding one](#adding-one).
- **Token accounts are not pre-created.** The first trade into a token pays
  ~0.00204 SOL of ATA rent. It is a refundable deposit rather than a fee, and
  Jupiter creates the account automatically, but it is working capital tied up
  until you close the account.
- **Bundle landing is not confirmed.** `sendBundle` returns a bundle ID; the bot
  does not yet poll `getBundleStatuses` to confirm the bundle landed, so a
  submitted-but-unselected bundle is recorded from the submission response
  rather than from observed on-chain state.
- **Compute budget is a fixed 600,000 units** for a composed two-leg swap rather
  than derived per route. Priority fee is charged on the requested limit, so an
  over-request costs real money.
- Only `SOL/USDC` and `SOL/USDT` are scanned; the pair list is a compile-time
  constant in `src/prices/mod.rs`.

## Backup

To create a local backup:

1. Run `./backup.sh`
2. Backups are stored in `~/solana_bot_backup/`
3. Each backup is timestamped
