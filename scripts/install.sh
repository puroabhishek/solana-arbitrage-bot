#!/bin/bash
set -e

# Install Solana CLI
sh -c "$(curl -sSfL https://release.solana.com/v1.17.0/install)"

# Install Rust if not installed
if ! command -v rustc &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
fi

# Build the project
cargo build --release

# Create config directory
mkdir -p ~/.config/solana-arbitrage-bot

echo "Installation complete!" 