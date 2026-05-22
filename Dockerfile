# Multi-stage Dockerfile for Velox
# Stage 1: Builder
FROM rust:1.80-slim as builder

# Install Node.js 20 for dashboard build
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && \
    apt-get install -y nodejs && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    postgresql-client \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./

# Copy source
COPY src ./src
COPY migrations ./migrations
COPY build.rs .
COPY dashboard ./dashboard
COPY models ./models

# Build the application (includes dashboard build via build.rs)
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    postgresql-client \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/velox .

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD pg_isready -h ${DATABASE_URL:-localhost} -U ${DB_USER:-postgres} || exit 1

# Run the application
CMD ["./velox"]
