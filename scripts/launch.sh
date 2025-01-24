#!/bin/bash

# Load environment variables
source .env

# Check if wallet exists
if [ ! -f "$WALLET_PATH" ]; then
    echo "Wallet not found. Creating new wallet..."
    solana-keygen new --outfile "$WALLET_PATH"
fi

# Start the bot
cargo run --release -- start 