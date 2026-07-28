# Solana Arbitrage Bot

A Rust CLI scaffold for monitoring and (eventually) executing arbitrage
opportunities across Solana DEXes.

> **Project status: work in progress — it does not trade yet.**
> The CLI, configuration, wallet loading, RPC connection and balance checks
> work. Price fetching, opportunity detection, swap-instruction building and
> transaction submission are still stubs. See
> [Current limitations](#current-limitations) before using this.

## What works today

- CLI with `start`, `status`, `history` and `configure` subcommands
- Loads a wallet keypair from disk (Solana CLI JSON or raw base58)
- Connects to a Solana RPC endpoint and reads the wallet balance
- Configurable minimum profit threshold and dry-run flag
- Strategy trait with a `TwoHopStrategy` implementation wired in

## Current limitations

These are scaffolding only, and are the work remaining before the bot can
place a real trade:

| Area | Status |
| --- | --- |
| Price fetching (`ArbitrageBot::fetch_prices`) | Returns an empty list |
| Opportunity detection (`TwoHopStrategy::find_opportunities`) | Returns an empty list |
| Swap instruction building (`TransactionBuilder::build_swap_instruction`) | Returns an empty instruction |
| Recent blockhash | Uses `Hash::default()`, not fetched from RPC |
| Transaction submission | Never calls `send_transaction` / `send_and_confirm_transaction` |
| Transaction signer | Uses a throwaway keypair, not the loaded wallet |
| MEV bundle submission | Posts to a hardcoded endpoint, no auth |
| Trade history (`history`) | Prints a hardcoded sample row |
| `configure` subcommand | Prints values but does not persist them |

`cargo run -- start --mode devnet` runs end to end (wallet load, config, DEX ID
load, monitoring loop) but will not find or execute trades.

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
5. Run against devnet:
   ```bash
   cargo run -- start --mode devnet --dry-run
   ```

## Configuration

All runtime configuration comes from environment variables in `.env` (see
`.env.example`):

| Variable | Purpose | Default |
| --- | --- | --- |
| `SOLANA_RPC_URL` | Solana RPC endpoint | `https://api.devnet.solana.com` |
| `WALLET_PATH` | Path to the wallet keypair file | *(required)* |
| `MIN_PROFIT_PERCENTAGE` | Minimum profit threshold | `1.5` |

Notes:

- `WALLET_PATH` does **not** expand `~` — use an absolute path.
- `config/config.json` is **not read by the bot**. Editing it has no effect;
  only `.env` matters.
- `config/dexes.json` holds DEX program IDs, currently only printed at startup.
- `MAX_TRADE_AMOUNT_SOL`, `RAYDIUM_PROGRAM_ID` and `ORCA_PROGRAM_ID` appear in
  `.env.example` but are not yet read by any code.

## Usage

```bash
cargo run -- start --mode devnet --dry-run   # monitor without executing
cargo run -- start --mode devnet \
  --min-profit 2.0 --max-amount 0.1          # non-interactive thresholds
cargo run -- status                          # bot status as JSON
cargo run -- history                         # trade history (sample data)
cargo run -- configure                       # interactive config (not persisted)
```

Omitted `--min-profit` / `--max-amount` are prompted for interactively.

## Testing

```bash
cargo build
cargo test
```

The integration test generates a throwaway keypair in a temp directory and sets
`WALLET_PATH` itself, so it needs no real wallet or network access.

## Security

- Never commit wallet keys. `.gitignore` covers `wallet.json` and `.env`;
  make sure whatever filename you choose is also covered.
- A private key and an SSH keypair were previously committed to this
  repository. They have been removed from the working tree, but **remain in
  git history** — treat them as compromised and rotate them.
- Prefer a dedicated, minimally funded wallet over your primary one.

## Backup

To create a local backup:

1. Run `./backup.sh`
2. Backups are stored in `~/solana_bot_backup/`
3. Each backup is timestamped
