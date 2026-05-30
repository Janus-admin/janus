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

## Getting Started

> **Prerequisites:** [Docker](https://docs.docker.com/get-docker/) and [Docker Compose](https://docs.docker.com/compose/install/) installed.

### Step 1 — Clone the repository

```bash
git clone https://github.com/Janus-admin/janus
cd janus
```

### Step 2 — Configure your environment

```bash
cp .env.example .env
```

Open `.env` and fill in the required values:

```bash
# Required — generate with: openssl rand -base64 32
JWT_SECRET=your-secret-here
ENCRYPTION_KEY=your-encryption-key-here

# Admin account — created automatically on first startup
ADMIN_EMAIL=admin@yourcompany.com
ADMIN_PASSWORD=your-strong-password

# Add at least one provider key
OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...
# GEMINI_API_KEY=...
```

> **Security:** change `ADMIN_PASSWORD` before exposing port 8080 to the internet.

### Step 3 — Start Janus

```bash
docker compose up -d
```

This starts Janus and a Postgres database. On first startup, Janus automatically creates your admin account using the credentials from step 2.

### Step 4 — Log in

Open **http://localhost:8080** in your browser and log in with the email and password you set in step 2.

Interactive API explorer (no auth required): **http://localhost:8080/admin/docs**

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
| **Providers** | OpenAI (GPT-5.x/4.x/o-series), Anthropic (Claude 4.x), Google Gemini (3.x/2.5), Groq (Llama 4/3), DeepSeek (V4), AWS Bedrock, any OpenAI-compatible endpoint |
| **Gateway endpoints** | `/v1/chat/completions`, `/v1/chat/completions/multi`, `/v1/embeddings`, `/v1/models`, `/v1/images/generations`, `/v1/audio/speech`, `/v1/audio/transcriptions` |
| **Multi-model compare** | `POST /v1/chat/completions/multi` — send one prompt to N models in parallel, get all responses in one JSON object; per-model error isolation; admin playground Compare tab |
| **Model-aware routing** | Router looks up `model_pricing` to find each model's owning provider — `claude-*` → Anthropic, `gemini-*` → Gemini, etc. No more wrong-provider fallthrough |
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

## Other Install Options

### Docker Compose — see [Getting Started](#getting-started) above (recommended)

### Single Docker container (existing Postgres)

```bash
docker run -d --name janus -p 8080:8080 \
  -e DATABASE_URL="postgres://user:pass@host:5432/janus" \
  -e JWT_SECRET="$(openssl rand -base64 32)" \
  -e ENCRYPTION_KEY="$(openssl rand -base64 32)" \
  -e ADMIN_EMAIL="admin@yourcompany.com" \
  -e ADMIN_PASSWORD="$(openssl rand -base64 16)" \
  -e OPENAI_API_KEY="sk-..." \
  ghcr.io/Janus-admin/janus:latest
```

### From source

```bash
git clone https://github.com/Janus-admin/janus && cd janus
cp janus.toml.example janus.toml   # edit with your Postgres URL, provider keys, and admin credentials
cargo run --release
```

On first startup with no users in the database, Janus creates the admin account from `admin_email` / `admin_password` in `janus.toml` (or the `ADMIN_EMAIL` / `ADMIN_PASSWORD` env vars). If those are not set, you can self-register at `POST /api/v1/auth/register` — registration closes automatically after the first account is created.

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

## Performance

Measured on Hetzner CCX23 (4 vCPU **dedicated** AMD EPYC, 16 GB RAM, Ubuntu
26.04), mock upstream simulating real-LLM latency (250 ms TTFT + 20 ms /
token), 60 s under sustained load at 50 concurrent connections:

| Workload | Throughput | p50 | p95 | **p99** | Errors |
|----------|-----------:|----:|----:|--------:|-------:|
| `chat-short`    — single-turn, ~50-token reply | 67,920 RPS | 0.63 ms | 1.56 ms | **2.12 ms** | 0 |
| `chat-long`     — 5-turn conversation          | 61,385 RPS | 0.71 ms | 1.74 ms | **2.40 ms** | 0 |
| `tools`         — function-calling, 3 tools    | 64,469 RPS | 0.64 ms | 1.83 ms | **2.97 ms** | 0 |
| `cache-warm`    — 100 % cache hit              | 73,790 RPS | 0.58 ms | 1.50 ms | **2.12 ms** | 0 |
| `smart-routing` — V5-L6 router on every req    | 95,596 RPS | 0.43 ms | 1.08 ms | **1.52 ms** | 0 |

All numbers are measured *with the full feature set on*: exact + semantic
cache layers warmed, PII redaction plugin active, time-guard regex set
loaded, audit log writing to PostgreSQL, plugin chain dispatch, and (for
the smart-routing row) the 4-layer V5-L6 routing engine evaluating every
request.

### Isolated gateway overhead: **0.84 ms p99**

An isolated overhead probe (cache disabled, mock at 1 ms TTFT) subtracts
the mock-llm baseline from the full-stack run on the same host:

| | p99 |
|---|---:|
| Mock-llm alone, no Janus in path | 44.15 ms |
| Janus + mock-llm, cache disabled | 44.99 ms |
| **Inferred Janus-only overhead** | **0.84 ms** |

(The 44 ms floor is a `tokio::time::sleep(1ms)` artefact inside mock-llm
under 50 concurrent SSE streams — present whether Janus is in the path
or not. The probe script is at [`benchmarks/bench-overhead-probe.sh`](benchmarks/bench-overhead-probe.sh).)

### Shared- vs dedicated-vCPU comparison (same workload, two Hetzner classes)

| Profile | CPX32 shared / 4 vCPU | CCX23 dedicated / 4 vCPU | Δ p99 | Δ RPS |
|---------|----------------------:|-------------------------:|------:|------:|
| chat-short | 40,694 / 3.01 ms | **67,920 / 2.12 ms** | −30 % | +67 % |
| chat-long  | 37,405 / 3.47 ms | **61,385 / 2.40 ms** | −31 % | +64 % |
| tools      | 40,262 / 2.99 ms | **64,469 / 2.97 ms** | ≈ 0 % | +60 % |
| cache-warm | 46,747 / 2.67 ms | **73,790 / 2.12 ms** | −21 % | +58 % |

Full reports:
- [CCX23 dedicated](benchmarks/history/cloud-hetzner-ccx23-2026-05-28/REPORT.md)
- [CPX32 shared](benchmarks/history/cloud-hetzner-cpx32-2026-05-28/REPORT.md)

---

## Honest Comparison: Janus vs LiteLLM

Both projects proxy LLM requests, enforce budgets, and expose an OpenAI-compatible API. Here is where they differ:

| | **Janus** | **LiteLLM** |
|---|---|---|
| **Language** | Rust | Python |
| **Proxy overhead (measured)** | **0.84 ms p99** (Hetzner CCX23 dedicated, see Performance section) | ~15–30 ms (community reports) |
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

> **Overhead figures** for Janus are measured (see Performance section above);
> the LiteLLM number is from community reports. Run your own benchmark on
> your hardware before quoting either; results vary by workload.

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

# First-run admin account — created on startup when the users table is empty.
# Remove after first run, or leave set (safe — idempotent).
admin_email      = "admin@yourcompany.com"
admin_password   = "changeme"

# Set to true to re-enable open self-registration (not recommended).
# allow_registration = false

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

# Multi-model parallel completion — same prompt to N models simultaneously
curl -X POST http://localhost:8080/v1/chat/completions/multi \
  -H "Authorization: Bearer jn-sk-..." \
  -H "Content-Type: application/json" \
  -d '{
    "models": ["gpt-4.1", "claude-opus-4-7", "gemini-3.5-flash", "deepseek-v4-pro"],
    "messages": [{"role": "user", "content": "Explain recursion in one sentence."}]
  }'
# Response:
# {
#   "results": [
#     { "model": "gpt-4.1",         "response": {...}, "latency_ms": 980  },
#     { "model": "claude-opus-4-7", "response": {...}, "latency_ms": 1340 },
#     { "model": "gemini-3.5-flash","response": {...}, "latency_ms": 720  },
#     { "model": "deepseek-v4-pro", "error": "...",   "latency_ms": 0    }
#   ]
# }
```

### Multi-Model Parallel Completions

`POST /v1/chat/completions/multi` sends the same prompt to every model in the `models` array **simultaneously** and returns all responses in one JSON object.

| Feature | Behaviour |
|---|---|
| **Partial failure** | One failed model returns `"error": "..."` — others are unaffected |
| **Concurrency cap** | Max 5 tasks run at once to avoid provider rate-limit storms |
| **Streaming** | Not supported — use `/v1/chat/completions` with `stream: true` per model |
| **Cost & audit** | Each model call logged and billed independently |
| **Caching** | Per-model cache — identical prompts on the same model hit cache |
| **Standard fields** | `temperature`, `max_tokens`, `tools`, `top_p`, etc. forwarded to every model |

**Admin playground** has a built-in **Compare** tab: select a gateway key → models auto-load from `allowed_models` → send prompt → see all results side-by-side with latency and cost.

### Model-Aware Routing

Janus automatically routes each model to its owning provider using the `model_pricing` catalogue:

| Model prefix | Provider |
|---|---|
| `gpt-*`, `o1`, `o3`, `o4-*` | OpenAI |
| `claude-*` | Anthropic |
| `gemini-*` | Google Gemini |
| `llama-*`, `groq/*`, `qwen/*` | Groq |
| `deepseek-*` | DeepSeek |
| Unknown model | Priority-ordered fallback across all providers |

### Admin API (selected endpoints)

```
POST   /admin/keys                       Create API key (shown once, never again)
GET    /admin/keys                       List keys (safe view — prefixes only)
POST   /admin/playground                 Test a single model (admin JWT, no limits)
POST   /admin/playground/multi           Test N models in parallel (admin JWT)
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
cp .env.example .env               # set ADMIN_EMAIL / ADMIN_PASSWORD + provider keys
docker compose up -d db            # start Postgres
cargo test
cargo run                          # creates admin account on first startup
```

For bug reports use [GitHub Issues](https://github.com/Janus-admin/janus/issues).
For questions and ideas use [GitHub Discussions](https://github.com/Janus-admin/janus/discussions).

---

## License

[Business Source License 1.1](LICENSE) — free to self-host, modify, and contribute. You may not offer Janus as a hosted managed service to third parties without a commercial license.

---

**Self-host your AI gateway.**
