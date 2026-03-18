# ── Stage 1: Build ───────────────────────────────────────────────────────────
FROM rust:1.85-slim AS builder

WORKDIR /app

# Increase stack size for rustc running under QEMU emulation
ENV RUST_MIN_STACK=16777216

# Cache dependencies separately from source
COPY Cargo.toml Cargo.lock ./
COPY crates/rule4210-core/Cargo.toml      crates/rule4210-core/Cargo.toml
COPY crates/rule4210-pricer/Cargo.toml    crates/rule4210-pricer/Cargo.toml
COPY crates/rule4210-scenarios/Cargo.toml crates/rule4210-scenarios/Cargo.toml
COPY crates/rule4210-margin/Cargo.toml    crates/rule4210-margin/Cargo.toml
COPY crates/rule4210-demo/Cargo.toml      crates/rule4210-demo/Cargo.toml
COPY crates/rule4210-server/Cargo.toml    crates/rule4210-server/Cargo.toml

# Create stub lib/main files so cargo can resolve the dependency graph
RUN mkdir -p crates/rule4210-core/src      && echo "pub fn _stub() {}" > crates/rule4210-core/src/lib.rs \
 && mkdir -p crates/rule4210-pricer/src    && echo "pub fn _stub() {}" > crates/rule4210-pricer/src/lib.rs \
 && mkdir -p crates/rule4210-scenarios/src && echo "pub fn _stub() {}" > crates/rule4210-scenarios/src/lib.rs \
 && mkdir -p crates/rule4210-margin/src    && echo "pub fn _stub() {}" > crates/rule4210-margin/src/lib.rs \
 && mkdir -p crates/rule4210-demo/src      && echo "fn main() {}"      > crates/rule4210-demo/src/main.rs \
 && mkdir -p crates/rule4210-server/src    && echo "fn main() {}"      > crates/rule4210-server/src/main.rs \
 && mkdir -p crates/rule4210-server/static && touch crates/rule4210-server/static/index.html

RUN cargo build --release --bin server 2>/dev/null; true

# Now copy real source — touch all .rs files so cargo sees them as newer than stubs
COPY crates/ crates/
RUN find /app/crates -name "*.rs" -exec touch {} \;
RUN cargo build --release --bin server

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl \
    && curl -fsSL https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 \
       -o /usr/local/bin/cloudflared \
    && chmod +x /usr/local/bin/cloudflared \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/server ./server
COPY entrypoint.sh ./entrypoint.sh
RUN chmod +x entrypoint.sh

EXPOSE 8080
ENTRYPOINT ["./entrypoint.sh"]
