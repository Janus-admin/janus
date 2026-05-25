# Velox — Self-Hosted AI Gateway

**Proxy for LLM calls with caching, cost tracking, and observability.**

```bash
docker run -p 8080:8080 \
  -e DATABASE_URL=postgres://... \
  -e JWT_SECRET=$(openssl rand -base64 32) \
  -e OPENAI_API_KEY=$YOUR_KEY \
  ghcr.io/alizadehafpn/velox:latest
```

Or run locally: `cargo run` (requires Postgres and `models/`)

---

## What Is Velox?

Velox is a **self-hosted proxy** that sits between your applications and LLM providers (OpenAI, Anthropic, Google Gemini, Groq, DeepSeek, AWS Bedrock). It:

- ✅ **Caches responses** — exact + semantic similarity (ONNX embeddings)
- 💰 **Tracks costs** — per-token pricing, per-API-key budgets
- 🔄 **Handles failover** — retries + provider switching
- 📊 **Exports metrics** — Prometheus `/metrics` endpoint
- 🔐 **Manages auth** — API keys, rate limiting, encrypted at rest
- 🎛️ **Web dashboard** — view costs, cache stats, live streaming

### What It's NOT

Velox is **not** a database, a BaaS, a Firebase clone, or a generic ML platform. It's specifically designed for LLM gateway use cases.

---

## 5-Minute Quickstart

### 1. Start Postgres (Docker)

```bash
docker run -d \
  --name velox-postgres \
  -e POSTGRES_PASSWORD=velox_dev \
  -e POSTGRES_DB=velox \
  -p 5432:5432 \
  postgres:16
```

### 2. Set environment variables

```bash
export DATABASE_URL=postgres://postgres:velox_dev@localhost:5432/velox
export JWT_SECRET=$(openssl rand -base64 32)
export ENCRYPTION_KEY=$(openssl rand -base64 32)

# Provider API keys (set at least one)
export OPENAI_API_KEY=sk-...        # OpenAI
export ANTHROPIC_API_KEY=sk-ant-... # Anthropic Claude
export GEMINI_API_KEY=...           # Google Gemini
export GROQ_API_KEY=...             # Groq
export DEEPSEEK_API_KEY=...         # DeepSeek
```

### 3. Download embedding model (for semantic cache)

```bash
mkdir -p models
# Download all-MiniLM-L6-v2 from HuggingFace (ONNX + tokenizer)
# Or run without — Velox degrades gracefully to exact-only caching
```

### 4. Run Velox

```bash
cargo run --release
# Server listening on 0.0.0.0:8080
```

### 5. Create an API key and test

```bash
# Create a key (shown once, never again)
curl -X POST http://localhost:8080/admin/keys \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"test","budget":100}'

# Use it to proxy LLM calls (OpenAI-compatible)
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer vx-sk-..." \
  -H "Content-Type: application/json" \
  -d '{
    "model":"gpt-4o",
    "messages":[{"role":"user","content":"Hello"}]
  }'
```

Full API docs: `http://localhost:8080/admin/docs` (Swagger UI, no auth required)

---

## Features

| Capability | Details |
|---|---|
| **Providers** | OpenAI, Anthropic, Bedrock, Gemini, Groq, DeepSeek, any OpenAI-compatible endpoint |
| **Endpoints** | `/v1/chat/completions`, `/v1/embeddings`, `/v1/models`, `/v1/images/generations`, `/v1/audio/*` |
| **Caching** | Exact (SHA-256) + semantic (ONNX cosine similarity, configurable threshold) |
| **Cost tracking** | Per-token, per-image, per-audio-second; per-key budgets |
| **Rate limiting** | Sliding window per API key |
| **Failover** | Automatic retry + provider switching on error |
| **RBAC** | ReadOnly / BillingViewer / ApiManager / Admin roles per workspace |
| **Observability** | Prometheus `/metrics`, structured request log, web dashboard |
| **Deployment** | Docker, Helm chart (`charts/velox/`), Railway, Fly.io, Render one-click configs |

---

## Configuration

Copy `velox.toml.example` to `velox.toml` and customize:

```toml
host = "0.0.0.0"
port = 8080
database_url = "postgres://..."
jwt_secret = "your-secret"
encryption_key = "your-encryption-key"
openai_api_key = "sk-..."
anthropic_api_key = "sk-ant-..."
cache_enabled = true
semantic_cache_threshold = 0.90
rate_limit_window_secs = 60
max_retries = 1
prometheus_enabled = true
```

All settings can be overridden with `UPPERCASE_ENV_VARS`.
Full reference: [`docs/configuration.md`](docs/configuration.md)

---

## API Examples

### Gateway API (OpenAI-compatible)

**POST /v1/chat/completions**

```json
{
  "model": "gpt-4o",
  "messages": [{"role": "user", "content": "Hello"}],
  "stream": false
}
```

Response headers on cache hits:
- `X-Velox-Cache-Hit: exact` or `semantic`
- `X-Velox-Cache-Similarity: 0.9542` (semantic only)

Skip cache for a single request:
```bash
curl ... -H "X-Velox-Cache: false"
```

### Admin API

- `POST /admin/keys` — Create API key
- `GET  /admin/keys` — List keys (safe view, no secrets)
- `GET  /admin/analytics/overview` — Daily costs, request counts, top models
- `GET  /admin/cache/stats` — Cache hit ratio, tokens saved, cost saved
- `GET  /metrics` — Prometheus metrics
- `GET  /admin/docs` — Interactive Swagger UI (OpenAPI 3.1)
- `GET  /admin/openapi.json` — Raw OpenAPI spec

### `velox` CLI

```bash
# Key management
velox keys list
velox keys create --name production --budget 500

# Migrations
velox migrate up
velox migrate status

# Import from competitors
velox import litellm --file litellm_config.yaml
velox import portkey --file portkey.json

# Backup / restore
velox backup create --out backup.tar.gz
velox backup restore --file backup.tar.gz
```

---

## Metrics (Prometheus)

Available at `GET /metrics`:

```
velox_requests_total{provider="openai",model="gpt-4o",status="success",cache_type="exact"} 142
velox_request_duration_seconds_bucket{provider="openai",model="gpt-4o",le="5ms"} 45
velox_tokens_total{provider="openai",model="gpt-4o",direction="prompt"} 2840
velox_cost_usd_total{provider="openai",model="gpt-4o"} 0.142857
```

---

## Caching Strategy

### Exact Cache (fast, guaranteed match)

- **Key:** SHA-256 of normalized request body
- **Lookup:** < 2ms (in-memory DashMap)

### Semantic Cache (smart, best-effort)

- **Key:** Cosine similarity over prompt embeddings
- **Lookup:** < 10ms
- **Model:** `all-MiniLM-L6-v2` (384-dim, 22MB)
- **Threshold:** 0.90 (configurable)

---

## Deployment

### Docker

```bash
docker build -t velox:latest .
docker run -p 8080:8080 \
  -e DATABASE_URL=postgres://... \
  -e JWT_SECRET=... \
  -e ENCRYPTION_KEY=... \
  -e OPENAI_API_KEY=... \
  velox:latest
```

Full Docker Compose setup: [`docs/deployment/docker.md`](docs/deployment/docker.md)

### Kubernetes (Helm)

```bash
helm repo add velox https://github.com/AlizadehAFPN/Velox
helm install velox charts/velox \
  --set secrets.jwtSecret=$(openssl rand -base64 32) \
  --set secrets.encryptionKey=$(openssl rand -base64 32) \
  --set secrets.openaiApiKey=$OPENAI_API_KEY \
  --set database.url=postgres://...
```

Full Helm reference: [`docs/deployment/helm.md`](docs/deployment/helm.md)

### One-click cloud deploys

| Platform | Config |
|---|---|
| Railway | [`deploy/railway/`](deploy/railway/) |
| Fly.io | [`deploy/fly/`](deploy/fly/) |
| Render | [`deploy/render/`](deploy/render/) |

HA setup with multiple nodes: [`docs/deployment/ha.md`](docs/deployment/ha.md)

---

## Debugging

```bash
# Enable debug logging
RUST_LOG=debug cargo run

# Health check
curl http://localhost:8080/health

# View recent requests (SQL)
SELECT * FROM requests
WHERE created_at > now() - interval '1 hour'
ORDER BY created_at DESC;
```

---

## Contributing

Velox is open source (MIT). PRs welcome!

**Before submitting:**

1. `cargo test` — all tests must pass
2. `cargo clippy -- -D warnings` — no warnings
3. `cargo fmt` — code must be formatted
4. Update CHANGELOG.md

**Development setup:**

```bash
git clone https://github.com/AlizadehAFPN/Velox.git
cd Velox
cp velox.toml.example velox.toml
# Edit velox.toml with your Postgres + provider API keys
cargo test
cargo run
```

---

## License

MIT License. See [LICENSE](LICENSE).

---

## Support

- **Issues:** [GitHub Issues](https://github.com/AlizadehAFPN/Velox/issues)
- **Discussions:** [GitHub Discussions](https://github.com/AlizadehAFPN/Velox/discussions)
- **Security:** Open a GitHub issue with the `security` label

---

**Built by [Farzad Alizadeh](https://github.com/AlizadehAFPN). Self-host your AI gateway.**
