#!/usr/bin/env bash
# ec2-setup.sh — one-shot setup for a fresh Ubuntu 22.04 EC2 instance.
#
# Run this as the default user (ubuntu) after SSH-ing in:
#   bash ec2-setup.sh
#
# What it does:
#   1. Installs system deps (Rust, Docker, oha, jq, curl)
#   2. Starts PostgreSQL via Docker
#   3. Builds Janus (release)
#   4. Builds mock-llm (release)
#   5. Seeds the database
#   6. Runs all benchmark profiles and saves results

set -euo pipefail

REPO_DIR="${REPO_DIR:-$HOME/janus}"
JANUS_REPO="${JANUS_REPO:-}"   # set to your git clone URL, or leave empty to use scp

log() { printf '\n[setup] %s\n' "$*"; }

# ── 1. System packages ─────────────────────────────────────────────────────────
log "Installing system packages..."
sudo apt-get update -qq
sudo apt-get install -y -qq \
    curl git jq build-essential pkg-config \
    libssl-dev libpq-dev ca-certificates \
    docker.io docker-compose-v2

sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"

# ── 2. Rust ────────────────────────────────────────────────────────────────────
if ! command -v rustc &>/dev/null; then
    log "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    source "$HOME/.cargo/env"
fi
source "$HOME/.cargo/env"
log "Rust: $(rustc --version)"

# ── 3. oha (load tool) ────────────────────────────────────────────────────────
if ! command -v oha &>/dev/null; then
    log "Installing oha..."
    cargo install oha --quiet
fi
log "oha: $(oha --version 2>/dev/null)"

# ── 4. Clone or use existing repo ─────────────────────────────────────────────
if [ -n "$JANUS_REPO" ]; then
    log "Cloning repo from $JANUS_REPO..."
    git clone "$JANUS_REPO" "$REPO_DIR"
elif [ ! -d "$REPO_DIR" ]; then
    log "ERROR: REPO_DIR=$REPO_DIR does not exist and JANUS_REPO is not set."
    log "Either: export JANUS_REPO=<git-url> before running this script,"
    log "     or: scp the project to $REPO_DIR manually."
    exit 1
fi

cd "$REPO_DIR"
log "Working directory: $(pwd) — commit: $(git rev-parse --short HEAD 2>/dev/null || echo dirty)"

# ── 5. PostgreSQL via Docker ───────────────────────────────────────────────────
log "Starting PostgreSQL..."
# Use newgrp to pick up the docker group without re-login
sg docker -c "docker compose up -d postgres" 2>/dev/null || \
    sudo docker compose up -d postgres

log "Waiting for PostgreSQL to be ready..."
for i in $(seq 1 30); do
    sg docker -c "docker compose exec -T postgres pg_isready -U postgres" \
        &>/dev/null && break || sleep 2
done

# ── 6. Build Janus ─────────────────────────────────────────────────────────────
log "Building Janus (release) — this takes ~5 minutes on first run..."
cargo build --release 2>&1 | tail -5
log "Janus built: $(./target/release/janus --version 2>/dev/null || echo ok)"

# ── 7. Build mock-llm ─────────────────────────────────────────────────────────
log "Building mock-llm..."
cd benchmarks/mock-llm
cargo build --release -q
cd "$REPO_DIR"
log "mock-llm built."

# ── 8. Copy .env if not present ───────────────────────────────────────────────
if [ ! -f .env ]; then
    cp .env.example .env
    log "Copied .env.example → .env (edit DATABASE_URL if needed)"
fi

log "Setup complete. Run benchmarks with:"
log "  ./benchmarks/run-all-cloud.sh"
