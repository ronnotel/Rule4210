# ── Stage 1: Build ───────────────────────────────────────────────────────────
FROM rust:1.82-slim AS builder

WORKDIR /app

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

# Now copy real source and rebuild only what changed
COPY crates/ crates/
RUN cargo build --release --bin server

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/server ./server

EXPOSE 8080
CMD ["./server"]
