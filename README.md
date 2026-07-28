# Solana Arbitrage Bot

A bot for executing arbitrage opportunities across multiple Solana DEXes.

## Features

- Multi-DEX arbitrage monitoring
- Support for Raydium, Orca, and Meteora
- Configurable profit thresholds
- Dry run mode for testing
- MEV bundle submission support

## Setup

1. Clone the repository
2. Copy `.env.example` to `.env` and fill in real values (see Configuration below):
   ```bash
   cp .env.example .env
   ```
3. Provide a wallet keypair file at the path you set for `WALLET_PATH` in `.env`. This accepts either:
   - a standard Solana CLI keypair, e.g. `solana-keygen new --outfile ~/.config/solana/devnet-wallet.json`, or
   - a raw base58 secret key exported from a wallet such as Phantom or Solflare, saved as plain text.

   Never commit this file. `config/config.json` is not currently read by the bot - only `.env` matters.
4. Install dependencies: `cargo build`
5. Run: `cargo run start --mode devnet`

## Configuration

All runtime configuration comes from environment variables in `.env` (see `.env.example`):
- `SOLANA_RPC_URL` - Solana RPC endpoint
- `WALLET_PATH` - path to your wallet keypair file (JSON array or base58 string)
- `MIN_PROFIT_PERCENTAGE` - minimum profit threshold

`config/dexes.json` holds DEX program IDs used for display in the CLI.

## Known limitations

This bot is a work in progress. Price fetching, opportunity detection, swap-instruction building, and live transaction submission are not yet implemented - `cargo run start --mode devnet` will run end-to-end (wallet load, balance check, monitoring loop) but will not currently find or execute real trades.

## Backup

To create a local backup:
1. Run `./backup.sh`
2. Backups are stored in `~/solana_bot_backup/`
3. Each backup is timestamped