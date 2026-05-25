# Multi-stage Dockerfile for Janus
# Stage 1: Builder
FROM rust:1.88-slim-trixie AS builder

# Install build prerequisites first (slim image lacks these)
RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    gnupg \
    build-essential \
    pkg-config \
    libssl-dev \
    postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 20 for dashboard build
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && \
    apt-get install -y nodejs && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy Cargo files
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./

# Copy source
COPY src ./src
COPY migrations ./migrations
COPY benches ./benches
COPY build.rs .
COPY dashboard ./dashboard
COPY models ./models
COPY .sqlx ./.sqlx

# Build the application (includes dashboard build via build.rs)
ENV SQLX_OFFLINE=true
RUN cargo build --release

# Stage 2: Runtime
FROM debian:trixie-slim

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/janus .

# Bundle semantic-cache ONNX model + tokenizer (read at runtime)
COPY --from=builder /build/models ./models

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the application
CMD ["./janus"]
