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
2. Create configuration files:
   ```bash
   mkdir -p config
   touch config/config.json
   touch .env
```
3. Install dependencies: `cargo build`
4. Run: `cargo run start --mode devnet`

## Configuration

Create necessary configuration files with appropriate values:
- RPC endpoint
- Wallet credentials
- DEX program IDs

## Backup

To create a local backup:
1. Run `./backup.sh`
2. Backups are stored in `~/solana_bot_backup/`
3. Each backup is timestamped