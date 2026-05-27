# Janus — Self-Hosted AI Gateway

**One base_url. Every model. Every provider. Your VPC.**

[![License: BUSL-1.1](https://img.shields.io/badge/license-BUSL--1.1-orange.svg)](LICENSE)
[![Docker Pulls](https://img.shields.io/docker/pulls/janusadmin/janus?logo=docker&logoColor=white)](https://hub.docker.com/r/janusadmin/janus)
[![Prometheus](https://img.shields.io/badge/prometheus-native-E6522C?logo=prometheus&logoColor=white)](https://prometheus.io)
[![GitHub Stars](https://img.shields.io/github/stars/Janus-admin/janus?style=flat&logo=github)](https://github.com/Janus-admin/janus/stargazers)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org)
[![CI](https://github.com/Janus-admin/janus/actions/workflows/ci.yml/badge.svg)](https://github.com/Janus-admin/janus/actions)

Janus is a **self-hosted AI gateway** written in Rust. It sits between your applications and every major LLM provider — OpenAI, Anthropic, AWS Bedrock, Gemini, Groq, DeepSeek — and adds two-layer caching, smart routing, cost control, and observability with ~0.5 ms proxy overhead.

---

## One-Line Install

```bash
git clone https://github.com/Janus-admin/janus && cd janus && cp .env.example .env && docker compose up -d
```

Edit `.env` to add your API keys, then open `http://localhost:8080`.

Interactive API explorer (no auth required): `http://localhost:8080/admin/docs`

---

## How It Works

```
Your app
  │
  └─► POST /v1/chat/completions          (OpenAI-compatible — zero client changes)
            │
            ▼
      ┌─────────────────────────────────────────────┐
      │              Janus Gateway                  │
      │                                             │
      │  1. API key auth + budget check             │
      │  2. Exact cache?  ──hit──► response <2ms   │
      │  3. Semantic cache? ─hit──► response <10ms  │
      │  4. Smart router   (pick model + provider)  │
      │  5. Provider call  (with retry + failover)  │
      │  6. Cost calc + audit log + alert check     │
      │  7. Cache write (exact + semantic)          │
      └─────────────────────────────────────────────┘
            │
            ▼
      OpenAI / Anthropic / Bedrock / Gemini / Groq / DeepSeek
```

---

## Features

| Capability | Details |
|---|---|
| **Providers** | OpenAI, Anthropic, AWS Bedrock, Gemini, Groq, DeepSeek, any OpenAI-compatible endpoint |
| **Gateway endpoints** | `/v1/chat/completions`, `/v1/embeddings`, `/v1/models`, `/v1/images/generations`, `/v1/audio/speech`, `/v1/audio/transcriptions` |
| **Smart routing** | 4-layer pipeline: capability filter → tag/rule match → complexity scoring → config default |
| **Exact cache** | SHA-256, DashMap hot layer + Postgres persistent — **< 2 ms**, zero API cost on hit |
| **Semantic cache** | ONNX cosine similarity, all-MiniLM-L6-v2 — **< 10 ms**, configurable threshold; optional Qdrant backend |
| **Cost tracking** | Per-token, per-image, per-audio pricing; per-key budgets; cost tags per request |
| **Alerts** | Slack block-kit and SMTP email on spend, error rate, and latency thresholds |
| **Failover** | Per-provider circuit breakers, automatic retry, priority-based provider switching |
| **Auth** | Gateway API keys (`jn-sk-…`), admin JWT, OIDC/SSO with PKCE and group→role mapping |
| **RBAC** | ReadOnly / BillingViewer / ApiManager / Admin — scoped per workspace |
| **Workspaces** | Multi-tenant: separate keys, budgets, routing rules, and members per workspace |
| **Prompts** | Versioned prompt library with per-version activation |
| **MCP** | Model Context Protocol server — stdio + SSE for tool-calling agents |
| **Observability** | Prometheus `/metrics`, structured request audit log, web dashboard, live WebSocket feed |
| **Deployment** | Docker, Helm chart, Railway, Fly.io, Render one-click configs |
| **CLI** | `janus` binary — keys, migrate, import (LiteLLM / Portkey), backup / restore |
| **OpenAPI** | Full OpenAPI 3.1 spec + Swagger UI embedded in the binary |

---

## Quickstart

### Option A — Docker Compose (recommended)

```bash
git clone https://github.com/Janus-admin/janus
cd janus
cp .env.example .env            # add your API keys here
docker compose up -d
```

This starts Janus + Postgres. Dashboard at `http://localhost:8080`.

### Option B — Single Docker container (existing Postgres)

```bash
docker run -d --name janus -p 8080:8080 \
  -e DATABASE_URL="postgres://user:pass@host:5432/janus" \
  -e JWT_SECRET="$(openssl rand -base64 32)" \
  -e ENCRYPTION_KEY="$(openssl rand -base64 32)" \
  -e OPENAI_API_KEY="sk-..." \
  ghcr.io/Janus-admin/janus:latest
```

### Option C — From source

```bash
git clone https://github.com/Janus-admin/janus && cd janus
cp janus.toml.example janus.toml   # edit with your Postgres URL + provider keys
cargo run --release
```

---

## Integrations

Janus is fully OpenAI-compatible. **Change one URL — zero other code changes.**

### LangChain (Python)

```python
from langchain_openai import ChatOpenAI

llm = ChatOpenAI(
    base_url="http://localhost:8080/v1",
    api_key="jn-sk-...",      # your Janus key
    model="gpt-4o",           # or omit — Janus picks the model automatically
)

response = llm.invoke("Summarize this in 3 bullet points: ...")
print(response.content)
```

Tag requests for cost attribution:

```python
llm = ChatOpenAI(
    base_url="http://localhost:8080/v1",
    api_key="jn-sk-...",
    model="gpt-4o",
    default_headers={
        "X-Janus-Tags": "team=growth,feature=summariser",
    },
)
```

Enable smart routing (Janus picks the model):

```python
# Omit model — Janus scores request complexity and picks the cheapest capable model
llm = ChatOpenAI(
    base_url="http://localhost:8080/v1",
    api_key="jn-sk-...",
    model="",                  # empty string triggers smart routing
)
```

### Vercel AI SDK (TypeScript)

```typescript
import { createOpenAI } from "@ai-sdk/openai";
import { generateText } from "ai";

const janus = createOpenAI({
  baseURL: "http://localhost:8080/v1",
  apiKey: "jn-sk-...",
});

const { text } = await generateText({
  model: janus("gpt-4o"),
  prompt: "Hello from Janus!",
});
```

Streaming in a Next.js route handler:

```typescript
import { createOpenAI } from "@ai-sdk/openai";
import { streamText } from "ai";

const janus = createOpenAI({
  baseURL: process.env.JANUS_URL + "/v1",
  apiKey: process.env.JANUS_API_KEY,
});

export async function POST(req: Request) {
  const { messages } = await req.json();
  const result = streamText({
    model: janus("gpt-4o"),
    messages,
  });
  return result.toDataStreamResponse();
}
```

### OpenAI SDK (Python / Node)

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/v1",
    api_key="jn-sk-...",
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello"}],
)
```

```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:8080/v1",
  apiKey: "jn-sk-...",
});
```

---

## Honest Comparison: Janus vs LiteLLM

Both projects proxy LLM requests, enforce budgets, and expose an OpenAI-compatible API. Here is where they differ:

| | **Janus** | **LiteLLM** |
|---|---|---|
| **Language** | Rust | Python |
| **Proxy overhead (est.)** | ~0.5 ms | ~15–30 ms |
| **Idle memory (est.)** | ~60 MB | ~400–600 MB |
| **With semantic cache** | ~220 MB | ~700 MB+ |
| **Exact caching** | ✅ built-in (SHA-256, <2 ms) | ✅ via Redis |
| **Semantic caching** | ✅ built-in (ONNX, <10 ms) | ✅ via Redis + embedding call |
| **Smart routing** | ✅ 4-layer (complexity score + tags + rules + fallback) | ✅ routing via YAML config |
| **Model support** | 50+ models seeded | 100+ provider aliases |
| **RBAC / workspaces** | ✅ built-in (4 roles, per-workspace) | Enterprise plan |
| **OIDC / SSO** | ✅ built-in (PKCE, JIT provisioning, group mapping) | Enterprise plan |
| **Dashboard** | ✅ embedded Next.js (single binary) | ✅ separate UI |
| **Backup / restore** | ✅ `janus backup create/restore` | ❌ |
| **Import from LiteLLM** | ✅ `janus import litellm` | — |
| **Kubernetes Helm chart** | ✅ | ✅ |
| **Python ecosystem** | ❌ | ✅ large |
| **License** | BUSL-1.1 | MIT |

> **Overhead figures** are estimates based on architecture — Rust/axum single-threaded dispatch vs Python/asyncio with GIL. Run your own benchmark; results vary by workload.

**Choose Janus** when you want minimal memory footprint, fast cache hits, built-in SSO and RBAC, and a single self-contained binary.

**Choose LiteLLM** when you need maximum model/provider coverage or are already deep in the Python ecosystem.

---

## Smart Routing

When you omit `model`, Janus selects automatically using a 4-layer pipeline:

```
Layer 1 — Capability filter     removes models that can't handle the request
                                 (vision, JSON mode, context window)
Layer 2 — Tag / rule match      applies X-Janus-Tags header and workspace
                                 admin routing rules (first-match wins)
Layer 3 — Complexity scoring    scores request 0–10 → micro / standard / premium tier
                                 (token estimate, depth, tools, complex verbs)
Layer 4 — Config default        fallback to workspace-configured default model
```

```bash
# Let Janus pick the model — omit "model" field
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer jn-sk-..." \
  -H "Content-Type: application/json" \
  -d '{"messages": [{"role": "user", "content": "What is 2+2?"}]}'

# Response headers tell you exactly what was chosen and why:
# X-Janus-Model-Selected: groq/llama-3.1-8b-instant
# X-Janus-Routing-Reason: complexity:micro(score=1)
```

Force premium quality with a tag:

```bash
curl ... -H "X-Janus-Tags: quality=premium"
```

Manage workspace routing rules:

```
GET/PUT   /admin/workspaces/:id/smart-routing/config
GET/POST  /admin/workspaces/:id/smart-routing/rules
PATCH/DELETE /admin/workspaces/:id/smart-routing/rules/:rule_id
```

---

## Two-Layer Cache

| Layer | Mechanism | Speed | Notes |
|---|---|---|---|
| **Exact** | SHA-256 + DashMap (hot) + Postgres (persistent) | < 2 ms | Zero API cost on hit |
| **Semantic** | ONNX cosine similarity, all-MiniLM-L6-v2 (384-dim) | < 10 ms | Threshold: 0.90, configurable |
| **Vector store** | Qdrant (optional backend) | < 5 ms | Better for large entry counts (>10 K) |

Download models for semantic cache (optional — degrades gracefully without):

```bash
mkdir -p models
curl -L https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx \
  -o models/all-MiniLM-L6-v2.onnx
curl -L https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json \
  -o models/tokenizer.json
```

Then mount `./models:/app/models` in Docker or set `embedding_model_path` in `janus.toml`.

Cache response headers:

| Header | Value |
|---|---|
| `X-Janus-Cache-Hit` | `exact` or `semantic` |
| `X-Janus-Cache-Similarity` | `0.9542` (semantic hits only) |

Skip cache for one request: `-H "X-Janus-Cache: false"`

---

## Observability

### Prometheus

Native metrics at `GET /metrics`:

```
janus_requests_total{provider,model,status,cache_type}
janus_request_duration_seconds{provider,model,le}
janus_tokens_total{provider,model,direction}
janus_cost_usd_total{provider,model}
janus_cache_exact_size
janus_cache_semantic_size
janus_cache_hit_ratio
```

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: janus
    static_configs:
      - targets: ["localhost:8080"]
    metrics_path: /metrics
```

### Dashboard

The web dashboard is embedded in the binary — no separate deployment. Open `http://localhost:8080` after login.

Pages: Overview · Requests · Analytics · Cost tags · Cache stats · Alerts · Providers · Workspaces · Prompt library · SSO settings · Onboarding tour

### Live stream

WebSocket feed of all proxied requests: `ws://localhost:8080/admin/stream`

---

## Configuration

```toml
# janus.toml — all values overridable by UPPERCASE_ENV_VARS

host             = "0.0.0.0"
port             = 8080
database_url     = "postgres://janus:pass@localhost:5432/janus"
jwt_secret       = "$(openssl rand -base64 32)"
encryption_key   = "$(openssl rand -base64 32)"

openai_api_key   = "sk-..."
anthropic_api_key = "sk-ant-..."
gemini_api_key   = ""
groq_api_key     = ""
deepseek_api_key = ""

cache_enabled               = true
semantic_cache_threshold    = 0.90
semantic_cache_backend      = "linear"   # or "qdrant"

rate_limit_window_secs      = 60
max_retries                 = 1
prometheus_enabled          = true

[smart_routing]
enabled       = false     # set true to allow omitting "model"
default_model = ""        # fallback when no routing rule matches
```

Full reference: [`CONFIGURATION.md`](CONFIGURATION.md)

---

## API Reference

### Gateway (OpenAI-compatible)

```bash
# Standard call
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer jn-sk-..." \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o", "messages": [{"role": "user", "content": "Hello"}]}'

# Smart-routed (omit model)
curl ... -d '{"messages": [{"role": "user", "content": "Hello"}]}'

# Tag for cost attribution
curl ... -H "X-Janus-Tags: team=growth,feature=chat"

# Skip cache
curl ... -H "X-Janus-Cache: false"

# Stream
curl ... -d '{"model": "gpt-4o", "stream": true, "messages": [...]}'
```

### Admin API (selected endpoints)

```
POST   /admin/keys                       Create API key (shown once, never again)
GET    /admin/keys                       List keys (safe view — prefixes only)
GET    /admin/analytics/overview         Daily costs, request counts, top models
GET    /admin/analytics/cost-by-tag      Cost breakdown by tag key
GET    /admin/cache/stats                Hit ratio, tokens saved, cost saved
DELETE /admin/cache                      Flush cache
GET    /admin/alerts                     List alert rules
POST   /admin/alerts                     Create alert rule
GET    /admin/providers                  List providers + health status
PATCH  /admin/providers/:id              Update provider config
GET    /admin/workspaces                 List workspaces and members
GET    /admin/prompts                    Versioned prompt library
GET    /metrics                          Prometheus metrics
GET    /admin/docs                       Swagger UI (OpenAPI 3.1, no auth)
GET    /admin/openapi.json               Raw OpenAPI spec
```

### CLI

```bash
# Key management
janus keys list
janus keys create --name production --budget 500
janus keys rotate jn-sk-...

# Database migrations
janus migrate up
janus migrate status

# Import from competitors
janus import litellm --file litellm_config.yaml --apply
janus import portkey  --file portkey.json        --apply

# Backup and restore
janus backup create  --out backup-$(date +%Y%m%d).tar.gz
janus backup restore --file backup-20260526.tar.gz

# Health check
janus doctor
```

---

## Deployment

### Docker Compose

```bash
git clone https://github.com/Janus-admin/janus && cd janus && cp .env.example .env && docker compose up -d
```

### Kubernetes (Helm)

```bash
helm install janus charts/janus \
  --set secrets.jwtSecret="$(openssl rand -base64 32)" \
  --set secrets.encryptionKey="$(openssl rand -base64 32)" \
  --set secrets.openaiApiKey="$OPENAI_API_KEY" \
  --set database.url="postgres://..."
```

### One-click cloud

| Platform | Config |
|---|---|
| Railway | [`deploy/railway/`](deploy/railway/) |
| Fly.io | [`deploy/fly/`](deploy/fly/) |
| Render | [`deploy/render/`](deploy/render/) |

---

## OIDC / SSO

Configure identity providers from the admin dashboard or API:

```bash
curl -X POST http://localhost:8080/admin/idp \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "oidc",
    "name": "Okta",
    "issuer": "https://your-org.okta.com",
    "client_id": "...",
    "client_secret": "...",
    "group_role_map": {"ai-platform": "Admin", "viewers": "ReadOnly"}
  }'
```

Login flow: `GET /auth/oidc/:idp_id/start` → IdP → `GET /auth/oidc/:idp_id/callback` → JWT.

Users are provisioned JIT on first login. Group claims map to Janus RBAC roles.

---

## Contributing

Janus is source-available under the Business Source License 1.1. PRs are welcome.

**Before submitting:**

1. `cargo test` — all tests must pass
2. `cargo clippy -- -D warnings` — zero warnings
3. `cargo fmt` — code must be formatted

**Development setup:**

```bash
git clone https://github.com/Janus-admin/janus
cd janus
cp janus.toml.example janus.toml   # edit with your Postgres URL + provider keys
cargo test
cargo run
```

For bug reports use [GitHub Issues](https://github.com/Janus-admin/janus/issues).
For questions and ideas use [GitHub Discussions](https://github.com/Janus-admin/janus/discussions).

---

## License

[Business Source License 1.1](LICENSE) — free to self-host, modify, and contribute. You may not offer Janus as a hosted managed service to third parties without a commercial license.

---

**Self-host your AI gateway.**
