#!/usr/bin/env bash
# ec2-setup.sh — one-shot setup for a fresh Ubuntu 22.04 / 24.04 / 26.04 cloud VM.
#
# Run as root (or with sudo) after SSH-ing in:
#   bash ec2-setup.sh
#
# Tested on:
#   • Hetzner CPX32 with Ubuntu 22.04 LTS
#   • Hetzner CPX32 with Ubuntu 26.04 LTS
#   • EC2 t3.xlarge with Ubuntu 22.04 LTS
#
# What it does:
#   1. Installs system packages (build toolchain, Docker, jq, Node.js, PostgreSQL libs)
#   2. Installs Rust (if missing) and oha (load tester)
#   3. Clones or uses an existing repo at REPO_DIR
#   4. Starts PostgreSQL via docker compose (`db` service)
#   5. Generates JWT_SECRET / ENCRYPTION_KEY / DATABASE_URL if not set
#   6. Builds Janus (release) — includes embedded Next.js dashboard
#   7. Builds mock-llm
#
# Next step after this finishes: ./benchmarks/run-all-cloud.sh

set -euo pipefail

REPO_DIR="${REPO_DIR:-$HOME/janus}"
JANUS_REPO="${JANUS_REPO:-}"   # set to your git clone URL, or leave empty if repo already present

log() { printf '\n[setup] %s\n' "$*"; }

# ── 0. sudo wrapper (works as root or via sudo) ────────────────────────────────
if [ "$(id -u)" -eq 0 ]; then
    SUDO=""
else
    SUDO="sudo"
fi

# ── 1. System packages ─────────────────────────────────────────────────────────
log "Installing system packages..."
$SUDO apt-get update -qq
$SUDO apt-get install -y -qq \
    curl git jq ca-certificates gnupg \
    build-essential pkg-config \
    libssl-dev libpq-dev \
    docker.io docker-compose-v2

# Node.js 20 is required because Janus's build.rs builds the embedded
# Next.js dashboard at compile time. Without it `cargo build` panics on `npm not found`.
if ! command -v node &>/dev/null; then
    log "Installing Node.js 20..."
    curl -fsSL https://deb.nodesource.com/setup_20.x | $SUDO bash -
    $SUDO apt-get install -y -qq nodejs
fi
log "node: $(node --version)"

$SUDO systemctl enable --now docker
# Add the invoking user to the docker group (no-op when running as root).
if [ -n "${SUDO}" ]; then
    $SUDO usermod -aG docker "$USER"
fi

# ── 2. Rust ────────────────────────────────────────────────────────────────────
if ! command -v rustc &>/dev/null; then
    log "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
# Always source so the rest of the script sees cargo/rustc on PATH.
# shellcheck disable=SC1091
source "$HOME/.cargo/env"
log "Rust: $(rustc --version)"

# ── 3. oha (load tester) ───────────────────────────────────────────────────────
if ! command -v oha &>/dev/null; then
    log "Installing oha (this takes 1–2 minutes)..."
    cargo install oha --quiet
fi
log "oha: $(oha --version 2>/dev/null)"

# ── 4. Clone or use existing repo ──────────────────────────────────────────────
if [ -n "$JANUS_REPO" ] && [ ! -d "$REPO_DIR" ]; then
    log "Cloning repo from $JANUS_REPO..."
    git clone "$JANUS_REPO" "$REPO_DIR"
elif [ ! -d "$REPO_DIR" ]; then
    log "ERROR: REPO_DIR=$REPO_DIR does not exist and JANUS_REPO is not set."
    log "Either: export JANUS_REPO=<git-url> before running this script,"
    log "     or: scp / git clone the project to $REPO_DIR first."
    exit 1
fi

cd "$REPO_DIR"
log "Working directory: $(pwd) — commit: $(git rev-parse --short HEAD 2>/dev/null || echo dirty)"

# ── 5. .env (with auto-generated secrets) ──────────────────────────────────────
if [ ! -f .env ]; then
    cp .env.example .env
    log "Copied .env.example → .env"
fi

# Append any required secrets/URLs that are missing. We never overwrite an
# existing value the operator may have set deliberately.
ensure_env() {
    local key="$1"; local value="$2"
    if ! grep -q "^${key}=." .env 2>/dev/null; then
        # Strip empty `KEY=` lines so the appended one wins.
        sed -i.bak "/^${key}=$/d" .env 2>/dev/null || true
        printf '%s=%s\n' "$key" "$value" >> .env
        log "  → added $key to .env"
    fi
}
ensure_env JWT_SECRET       "$(openssl rand -base64 32)"
ensure_env ENCRYPTION_KEY   "$(openssl rand -base64 32)"
ensure_env DATABASE_URL     "postgres://janus:janus_dev@localhost:5432/janus"
rm -f .env.bak

# ── 6. PostgreSQL via Docker ───────────────────────────────────────────────────
# docker-compose.yml exposes the service as `db` (not `postgres`).
log "Starting PostgreSQL container ('db' service)..."
$SUDO docker compose up -d db

log "Waiting for PostgreSQL to be ready..."
for _ in $(seq 1 30); do
    $SUDO docker compose exec -T db pg_isready -U janus &>/dev/null && break || sleep 2
done

# ── 7. Build Janus ─────────────────────────────────────────────────────────────
# SQLX_OFFLINE=true tells sqlx's compile-time query macros to use the prepared
# `.sqlx/` fingerprints instead of opening a fresh connection to validate every
# query. Without this, the build connects to whatever DATABASE_URL points at —
# which on a fresh VM is the PG container we just started but with no schema
# yet (migrations run at startup, not at compile time), so the build fails on
# `relation "model_pricing" does not exist`.
log "Building Janus (release) — first build takes ~5 minutes..."
SQLX_OFFLINE=true cargo build --release 2>&1 | tail -5
log "Janus built."

# ── 8. Build mock-llm ──────────────────────────────────────────────────────────
log "Building mock-llm..."
( cd benchmarks/mock-llm && SQLX_OFFLINE=true cargo build --release -q )
log "mock-llm built."

log "Setup complete. Run benchmarks with:"
log "  bash benchmarks/run-all-cloud.sh"
