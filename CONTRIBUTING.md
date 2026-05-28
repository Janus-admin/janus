# Contributing to Janus

Thank you for your interest in contributing to Janus!

## Getting Started

### Prerequisites

- Rust 1.75+
- Docker & Docker Compose (for PostgreSQL)
- `cargo`, `clippy`, `rustfmt`

### Setup

```bash
git clone https://github.com/Janus-admin/janus.git
cd janus
cp .env.example .env
docker compose up -d postgres
cargo build
```

### Running Tests

```bash
cargo test
```

All tests must pass before submitting a PR.

## How to Contribute

### Reporting Bugs

Open an issue with:
- A clear description of the problem
- Steps to reproduce
- Expected vs actual behavior
- Janus version and OS

### Suggesting Features

Open an issue tagged `enhancement` before writing code. This saves time if the feature doesn't fit the project direction.

### Submitting a Pull Request

1. Fork the repo and create a branch from `master`
2. Make your changes
3. Ensure all checks pass:

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

4. Open a PR with a clear description of what and why

## Code Style

- Follow standard Rust conventions
- No `unwrap()` in production paths — use `?` or explicit error handling
- No `println!` in library code — use `tracing`
- New database changes go in a new migration file — never modify existing ones

## License

By contributing, you agree that your contributions will be licensed under the [BUSL-1.1 License](LICENSE).
