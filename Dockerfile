# Multi-stage build for minimal image size
# Builds on the MSRV: Cargo.toml `rust-version` is authoritative, and cargo
# refuses to build if this toolchain falls below it.
FROM rust:1.88-slim-trixie AS builder

WORKDIR /build

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy main.rs to cache dependencies
RUN mkdir -p src && echo "fn main() {}" > src/main.rs

# Build dependencies (cached layer)
RUN cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src

# Build the actual binary
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM debian:trixie-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder (the dashboard assets are embedded in it)
COPY --from=builder /build/target/release/nanomon /app/nanomon

# Run unprivileged: the files NanoMon reads under /proc and /sys are
# world-readable. The Docker socket is not, so the deployment grants the
# socket's group with `group_add` (see deploy/compose.yml).
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin nanomon
USER 10001

# Settings and their defaults live in src/config.rs. EXPOSE and the
# healthcheck assume the default NANOMON_PORT.
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
    CMD curl -sf http://localhost:3000/api/health || exit 1

# Run the binary
CMD ["/app/nanomon"]
