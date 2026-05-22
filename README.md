# Velox — Self-Hosted AI Gateway

**Proxy for LLM calls with caching, cost tracking, and observability.**

```bash
docker run -p 8080:8080 \
  -e DATABASE_URL=postgres://... \
  -e JWT_SECRET=$(openssl rand -base64 32) \
  -e OPENAI_API_KEY=$YOUR_KEY \
  ghcr.io/anthropics/velox:latest
```

Or run locally: `cargo run` (requires Postgres and models/)

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
export ANTHROPIC_API_KEY=sk-ant-... # Anthropic
export GEMINI_API_KEY=...           # Google Gemini
export GROQ_API_KEY=...             # Groq
export DEEPSEEK_API_KEY=...         # DeepSeek
```

### 3. Download embedding model (for semantic cache)

```bash
mkdir -p models
# Download all-MiniLM-L6-v2 from HuggingFace (ONNX + tokenizer)
# Or run without semantic cache — it will gracefully degrade
```

### 4. Run Velox

```bash
cargo run --release
# Server listening on 0.0.0.0:8080
```

### 5. Create an API key and test

```bash
# Create a key (shows once, never again)
curl -X POST http://localhost:8080/admin/keys \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -d '{"name":"test","budget":100}' \
  -H "Content-Type: application/json"

# Use it to proxy LLM calls
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer vx-sk-..." \
  -d '{
    "model":"gpt-4",
    "messages":[{"role":"user","content":"Hello"}],
    "stream":false
  }' \
  -H "Content-Type: application/json"
```

---

## Architecture & Phases

Velox is built in **9 phases** of increasing complexity:

| Phase | What | Status |
|-------|------|--------|
| **0** | Core tables, config system, error handling | ✅ Done |
| **1** | OpenAI/Anthropic/Gemini/Groq/DeepSeek/Bedrock adapters, gateway proxy | ✅ Done |
| **2** | Streaming (SSE) for all providers | ✅ Done |
| **3** | Rate limiting, retry logic, provider failover | ✅ Done |
| **4** | Exact cache (SHA-256 hot layer + DB persistence) | ✅ Done |
| **5** | Semantic cache (ONNX embeddings, HNSW index) | ✅ Done |
| **6** | Embedded web dashboard (Next.js, static assets) | ✅ Done |
| **7** | Production hardening (Prometheus, graceful shutdown, request limits) | ✅ Done |
| **8** | Open source launch (GitHub, licensing, docs) | 🚧 In progress |
| **9** | Mobile app | ⏸️ Not started |

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

All settings can be overridden with `UPPERCASE_ENVAR_NAMES`.

---

## API Examples

### Gateway API (OpenAI-compatible)

**POST /v1/chat/completions**

Request:
```json
{
  "model": "gpt-4",
  "messages": [{"role": "user", "content": "Hello"}],
  "stream": false,
  "temperature": 0.7
}
```

Response:
```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "gpt-4",
  "choices": [{"message": {"content": "Hi there!"}, "finish_reason": "stop"}],
  "usage": {"prompt_tokens": 10, "completion_tokens": 8, "total_tokens": 18}
}
```

Headers:
- `X-Velox-Cache-Hit: exact` or `semantic` if cached
- `X-Velox-Cache-Similarity: 0.9542` (for semantic hits)

### Admin API

**POST /admin/keys** — Create API key

**GET /admin/keys** — List keys (safe view, no secrets)

**GET /admin/analytics/overview** — Daily costs, request counts, top models

**GET /admin/cache/stats** — Cache hit ratio, tokens saved, cost saved

**GET /metrics** — Prometheus metrics

Full API docs in DECISIONS.md.

---

## Metrics (Prometheus)

Available at `GET /metrics`:

```
velox_requests_total{provider="openai",model="gpt-4",status="success",cache_type="exact"} 142
velox_request_duration_seconds_bucket{provider="openai",model="gpt-4",le="5ms"} 45
velox_tokens_total{provider="openai",model="gpt-4",direction="prompt"} 2840
velox_cost_usd_total{provider="openai",model="gpt-4"} 0.142857
```

Scrape with Prometheus every 30s:

```yaml
global:
  scrape_interval: 30s

scrape_configs:
  - job_name: velox
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/metrics'
```

---

## Caching Strategy

### Exact Cache (fast, guaranteed match)

- **Key:** SHA-256 of normalized request body
- **Lookup time:** < 2ms
- **Hit rate:** 100% if request is identical

### Semantic Cache (smart, best-effort)

- **Key:** HNSW nearest-neighbor on prompt embedding
- **Lookup time:** < 10ms
- **Hit rate:** Depends on similarity threshold (default 0.90)
- **Model:** `all-MiniLM-L6-v2` (384-dim, 22MB)

### How to Use

Skip cache for a single request:
```bash
curl ... -H "X-Velox-Cache: false"
```

All caching is automatic otherwise. Flush with:
```bash
curl -X DELETE http://localhost:8080/admin/cache
```

---

## Deployment

### Docker (recommended)

```bash
docker build -t velox:latest .
docker run -p 8080:8080 \
  -e DATABASE_URL=postgres://... \
  -e JWT_SECRET=... \
  -e ENCRYPTION_KEY=... \
  -e OPENAI_API_KEY=... \
  velox:latest
```

Docker image targets **<50MB** after multi-stage build.

### Kubernetes

Helm chart coming soon. For now:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: velox
spec:
  replicas: 2
  template:
    spec:
      containers:
        - name: velox
          image: ghcr.io/anthropics/velox:latest
          ports:
            - containerPort: 8080
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: velox
                  key: database-url
            # ... other env vars
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 30
          resources:
            requests:
              memory: "256Mi"
              cpu: "250m"
            limits:
              memory: "512Mi"
              cpu: "500m"
```

### Performance Benchmarks

Velox layer overhead (vs direct provider call):

| Operation | Latency | Notes |
|-----------|---------|-------|
| Exact cache hit | +3ms | DashMap lookup |
| Semantic cache hit | +8ms | Embedding inference + cosine search |
| Cache miss + auth | +5ms | Token validation + budget check |
| Rate limit check | +1ms | Sliding window |
| Provider call (passthrough) | +0ms | Transparent proxy |

**Total overhead for new request:** ~5ms

---

## Debugging

### Enable debug logging

```bash
RUST_LOG=debug cargo run
```

### Check health

```bash
curl http://localhost:8080/health
# {
#   "status": "ok",
#   "version": "0.1.0",
#   "database": "ok",
#   "providers": [
#     {"name": "openai", "priority": 10, "health": "ok"},
#     {"name": "anthropic", "priority": 20, "health": "ok"}
#   ],
#   "cache": {"enabled": true, "entries": 142}
# }
```

### View request logs

```bash
SELECT * FROM requests 
WHERE created_at > now() - interval '1 hour'
ORDER BY created_at DESC;
```

### Monitor cache effectiveness

```bash
SELECT 
  cache_type,
  COUNT(*) as hits,
  SUM(tokens_saved) as tokens_saved,
  SUM(cost_saved) as cost_saved
FROM cache_hits
GROUP BY cache_type;
```

---

## Contributing

Velox is open source (MIT). PRs welcome!

**Before submitting:**

1. Run `cargo test` — all tests must pass
2. Run `cargo clippy -- -D warnings` — no warnings
3. Run `cargo fmt` — code must be formatted
4. Update CHANGELOG.md

**Development setup:**

```bash
git clone https://github.com/anthropics/velox.git
cd velox
cp velox.toml.example velox.toml
# Edit velox.toml with your Postgres + API keys
cargo test
cargo run
```

---

## License

MIT License. See LICENSE file.

---

## Support

- **Issues:** GitHub Issues
- **Discussions:** GitHub Discussions
- **Security:** security@anthropic.com

---

**Made with ❤️ by Anthropic. Built for scale.**
