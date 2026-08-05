#!/usr/bin/env bash
#
# One-time setup: prerequisites, a dedicated wallet, and a working .env.
#
# Safe to re-run — it never overwrites an existing wallet or .env.
#
#   ./scripts/setup.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

WALLET_DIR="${HOME}/.config/solana"
WALLET_PATH="${WALLET_DIR}/bot-wallet.json"

say() { printf '\n\033[1m==> %s\033[0m\n' "$1"; }
warn() { printf '\033[33m    %s\033[0m\n' "$1"; }

say "Checking prerequisites"

if ! command -v cargo >/dev/null 2>&1; then
    warn "Rust not found. Install it, then re-run this script:"
    warn "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi
echo "    rust:   $(rustc --version)"

if ! command -v solana-keygen >/dev/null 2>&1; then
    warn "Solana CLI not found. Install it, then re-run this script:"
    warn "  sh -c \"\$(curl -sSfL https://release.anza.xyz/stable/install)\""
    warn ""
    warn "You can continue without it only if you already have a keypair file."
    exit 1
fi
echo "    solana: $(solana-keygen --version)"

say "Wallet"

if [ -f "$WALLET_PATH" ]; then
    echo "    Using existing wallet: $WALLET_PATH"
else
    mkdir -p "$WALLET_DIR"
    warn "Creating a NEW dedicated wallet for this bot."
    warn "Do not reuse a wallet holding funds you care about."
    echo
    solana-keygen new --outfile "$WALLET_PATH"
fi
chmod 600 "$WALLET_PATH"

PUBKEY="$(solana-keygen pubkey "$WALLET_PATH")"
echo "    Address: $PUBKEY"

say "Configuration"

if [ -f .env ]; then
    echo "    .env already exists — leaving it untouched."
    if ! grep -qE '^WALLET_PATH=' .env; then
        warn "But it has no WALLET_PATH set. Add this line:"
        warn "  WALLET_PATH=$WALLET_PATH"
    fi
else
    cp .env.example .env
    # Absolute path: the bot does not expand "~", which is a common trip-up.
    if sed --version >/dev/null 2>&1; then
        sed -i "s|^WALLET_PATH=.*|WALLET_PATH=${WALLET_PATH}|" .env      # GNU
    else
        sed -i '' "s|^WALLET_PATH=.*|WALLET_PATH=${WALLET_PATH}|" .env   # BSD/macOS
    fi
    echo "    Created .env with WALLET_PATH=$WALLET_PATH"
fi

say "Building (first build takes a few minutes)"
cargo build --release

say "Ready"
cat <<EOF

Wallet address: $PUBKEY

Next steps, in order — do not skip ahead:

  1. See the logic work offline (no network, no funds):
       cargo run --example offline_demo

  2. Watch real live prices. Spends nothing:
       cargo run --release -- start

  3. Prove submission works, using free devnet SOL:
       solana airdrop 1 $PUBKEY --url devnet
       cargo run --release -- start --mode rehearse --yes

  4. Build a real mainnet swap and validate it without sending:
       cargo run --release -- start --mode simulate --yes

  5. Only after 1-4 are clean, and only with real SOL in the wallet,
     both caps set in .env, and money you can afford to lose:
       cargo run --release -- start --mode live --yes

  Anytime:
       cargo run --release -- history      # what happened and why
       cargo run --release -- status       # current state

EOF
