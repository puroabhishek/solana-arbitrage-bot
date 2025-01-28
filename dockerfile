# Build stage
FROM rust:1.75 as builder

WORKDIR /usr/src/app
COPY . .

RUN cargo build --release

# Runtime stage
FROM debian:bullseye-slim

WORKDIR /usr/local/bin

COPY --from=builder /usr/src/app/target/release/solana-arbitrage-bot .
COPY --from=builder /usr/src/app/scripts ./scripts
COPY --from=builder /usr/src/app/.env.example .

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

CMD ["./solana-arbitrage-bot", "start"]