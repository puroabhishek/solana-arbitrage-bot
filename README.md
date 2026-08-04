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

1. Clone the repository.
2. Copy the environment template and fill in real values:
   ```bash
   cp .env.example .env
   ```
3. Set `WALLET_PATH` in `.env` to the keypair file from
   [Wallet setup](#wallet-setup). **Never commit this file.**
4. Build:
   ```bash
   cargo build
   ```
5. Watch for opportunities, risking nothing:
   ```bash
   cargo run -- start
   ```

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

## Known limitations

- **MEV/Jito bundle submission** (`src/execution/mev_builder.rs`) is unchanged
  scaffolding, not wired into the execution path.
- **Only two-hop (`A -> B -> A`) routes** are implemented. Triangular routes
  across three tokens are not.
- **Realised profit is recorded as the expected value**, not measured from
  post-trade balances, so the loss cap is an estimate rather than a settled
  accounting of what actually happened on-chain.
- Only `SOL/USDC` and `SOL/USDT` are scanned; the pair list is a compile-time
  constant in `src/prices/mod.rs`.

## Backup

To create a local backup:

1. Run `./backup.sh`
2. Backups are stored in `~/solana_bot_backup/`
3. Each backup is timestamped
