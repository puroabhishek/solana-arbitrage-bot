#!/usr/bin/env bash
#
# Run the bot with settings from .env.
#
# Defaults to `detect` mode, which spends nothing. Pass any bot flags through:
#
#   ./scripts/launch.sh                          # detect
#   ./scripts/launch.sh --mode simulate --yes
#   ./scripts/launch.sh --mode live --yes

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if [ ! -f .env ]; then
    echo "No .env found. Run ./scripts/setup.sh first." >&2
    exit 1
fi

# The bot loads .env itself via dotenv; this is only to validate the wallet
# path up front, so a missing key fails with a clear message rather than deep
# inside a run. Parsed rather than sourced so comments and quoting in .env
# cannot execute as shell.
WALLET_PATH="$(grep -E '^WALLET_PATH=' .env | tail -1 | cut -d= -f2- | tr -d '"'"'"'' | xargs || true)"

if [ -z "${WALLET_PATH}" ]; then
    echo "WALLET_PATH is not set in .env" >&2
    exit 1
fi

if [ ! -f "${WALLET_PATH}" ]; then
    echo "Wallet file not found: ${WALLET_PATH}" >&2
    echo "Run ./scripts/setup.sh, or fix WALLET_PATH in .env." >&2
    echo "Note: '~' is NOT expanded — use an absolute path." >&2
    exit 1
fi

exec cargo run --release -- start "$@"
