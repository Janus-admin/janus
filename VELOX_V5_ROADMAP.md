# VELOX V5 — Market-Readiness Roadmap
> Built on V4 (all 10 phases complete, 2026-05-24).
> **If you are Claude: read CLAUDE.md first, then VELOX_V4_ROADMAP.md §16, then this file.**

---

## V5 Philosophy

V1–V4 built a technically complete product. V5 makes it a **sellable** product.

A phase belongs in V5 if and only if a real prospect would say *"we can't adopt without this."* Everything else is parked in VELOX_FUTURE.md.

V5 is organised by **buyer-adoption blockers**, not by engineering aesthetics:

1. **Tier 1 — Self-serve adoption blockers** (V5-0, V5-1, V5-2): without these a developer cannot evaluate Velox at all.
2. **Tier 2 — Enterprise deal blockers** (V5-3, V5-4, V5-5, V5-6): without these every deal above ~$50k ACV stalls at procurement, security, or finance review.
3. **Tier 3 — Competitive parity & launch surface** (V5-7, V5-8, V5-9): polish that wins bake-offs and converts the funnel.

**Rules for every V5 phase:**
1. Every phase must answer "which adoption blocker does this remove?" in one sentence.
2. Each phase ships independently — no phase blocks the next on merge.
3. Backend, SDK, and frontend phases follow the V4 separation: do not mix them.
4. Every phase must list at least one external signal that proves it removed the blocker (download count, SSO test login, customer migration, etc.).

**Locked decisions (made before V5-0 starts — see §17):**
- License: Elastic License v2 (replaces MIT)
- Brand: independent (no "Anthropic" anywhere)
- CLI shape: single `velox` binary with subcommands
- OpenAPI tool: `utoipa` + `utoipa-swagger-ui`
- SSO order: OIDC first, then SAML — both in V5-3
- Managed cloud: deferred to V6 (not V5 scope)

---

## Table of Contents

1. [Complete Gap Audit](#1-complete-gap-audit)
2. [V5 Testing Philosophy](#2-v5-testing-philosophy)
3. [Phase V5-0: API Surface Expansion](#3-phase-v5-0-api-surface-expansion)
4. [Phase V5-1: SDKs, CLI, and OpenAPI](#4-phase-v5-1-sdks-cli-and-openapi)
5. [Phase V5-2: Deployment & Migration Tooling](#5-phase-v5-2-deployment--migration-tooling)
6. [Phase V5-3: Enterprise Auth (OIDC, SAML, SCIM)](#6-phase-v5-3-enterprise-auth-oidc-saml-scim)
7. [Phase V5-4: Compliance & Audit Hardening](#7-phase-v5-4-compliance--audit-hardening)
8. [Phase V5-5: Cost Attribution & FinOps](#8-phase-v5-5-cost-attribution--finops)
9. [Phase V5-6: Notifications & APM Integrations](#9-phase-v5-6-notifications--apm-integrations)
10. [Phase V5-7: Governance & Safety](#10-phase-v5-7-governance--safety)
11. [Phase V5-8: Performance & Reliability Proofs](#11-phase-v5-8-performance--reliability-proofs)
12. [Phase V5-9: Docs, Onboarding, and GTM Assets](#12-phase-v5-9-docs-onboarding-and-gtm-assets)
13. [What V5 Explicitly Does NOT Include](#13-what-v5-explicitly-does-not-include)
14. [Migration Plan](#14-migration-plan)
15. [Dependency Plan](#15-dependency-plan)
16. [V5 Phase Status Tracker](#16-v5-phase-status-tracker)
17. [Locked Decisions Record](#17-locked-decisions-record)
18. [Session Start Ritual for V5 Work](#18-session-start-ritual-for-v5-work)

---

## 1. Complete Gap Audit

Three lenses: **API surface**, **enterprise readiness**, and **funnel/GTM**.

### API surface gaps (blocks self-serve adoption)

| Gap | Why it blocks adoption | Phase |
|---|---|---|
| No `/v1/embeddings` endpoint | RAG apps (the largest LLM workload) cannot use Velox at all | V5-0 |
| No `/v1/images/generations`, `/v1/audio/*` | Multimodal apps cannot unify on Velox | V5-0 |
| No `/v1/models` | OpenAI SDK calls `client.models.list()` on init; many libraries break | V5-0 |
| No `/v1/completions` (legacy) | Older codebases (LangChain ≤0.1, llama-index ≤0.10) cannot point at Velox | V5-0 |
| No `tools` / `tool_calls` extracted in audit log | Function-calling apps have no observability | V5-0 |
| No official Python / Node SDKs | Developer evaluation gives up in <15 minutes | V5-1 |
| No OpenAPI spec | No autocomplete, no codegen, no Postman | V5-1 |
| No CLI for ops | Operators expect `velox keys create`, `velox migrate`, `velox doctor` | V5-1 |
| No Helm chart | Enterprises will not paste raw YAML from a README | V5-2 |
| No Terraform provider | IaC users cannot manage Velox declaratively | V5-2 |
| No migration importers (LiteLLM, Portkey, OpenRouter) | Switching cost kills every replacement deal | V5-2 |
| No one-click deploy (Railway/Fly/Render) | Demo-day prospects need a 60-second deploy path | V5-2 |
| No backup/restore CLI | Ops will not run `pg_dump` by hand against a vendor product | V5-2 |

### Enterprise gaps (blocks deals over ~$50k ACV)

| Gap | Why it blocks adoption | Phase |
|---|---|---|
| No SSO / OIDC | Hard blocker for any company with central IAM | V5-3a |
| No SAML 2.0 | Hard blocker for traditional enterprise IT | V5-3b |
| No SCIM provisioning | Hard blocker for IT-led rollouts | V5-3 |
| Audit log not tamper-evident | Fails SOC 2 CC7.2 controls | V5-4 |
| No data export (DSAR / GDPR Article 15) | EU prospects walk during legal review | V5-4 |
| No right-to-be-forgotten | GDPR Article 17 blocker | V5-4 |
| No secrets manager integration (Vault, AWS SM, GCP SM) | Production teams do not use env vars for secrets | V5-4 |
| No data residency controls | Cannot answer "stays in EU?" | V5-4 |
| No cost attribution by team/project/user | Finance teams need chargeback splits | V5-5 |
| No budget forecasting | Cannot answer "when will we hit cap?" | V5-5 |
| No invoice generation / chargeback CSV | Finance builds the spreadsheets externally | V5-5 |
| No Slack / PagerDuty / Email / Teams channels | Webhook-only feels half-finished | V5-6 |
| No Datadog / New Relic exporters | "We can't see Velox in our APM" disqualifies | V5-6 |
| No SLA reports per workspace | Mid-market customers ask for monthly proof | V5-6 |
| No admin-configurable policy engine | Security teams ask "what stops a dev sending PII to OpenAI?" | V5-7 |
| No prompt injection detection | AI-security teams ask | V5-7 |
| No content moderation hook | Required by regulated industries | V5-7 |

### Funnel / GTM gaps (blocks ever being considered)

| Gap | Why it blocks adoption | Phase |
|---|---|---|
| No public load test numbers | "How many req/s?" — no answer means lost bake-off | V5-8 |
| No chaos test report | Reliability claims read as marketing without proof | V5-8 |
| No E2E Playwright tests | Quality signal; V5 regressions otherwise certain | V5-8 |
| No marketing landing page | Visitors leave without conversion | V5-9 |
| No comparison pages (vs LiteLLM, Portkey, Helicone, Cloudflare) | You do not appear in consideration sets | V5-9 |
| No interactive product tour | Conversion drop between sign-up and first request | V5-9 |
| No sample apps repo (RAG, agent, embeddings search) | Devs cannot picture using Velox in their stack | V5-9 |
| No public Discord / community | No defensible adoption surface | V5-9 |
| No status page | Operators check before adoption — empty = no | V5-9 |
| MIT license open to AWS-style commodification | Future managed offering unprotected | V5-9 (license switch ships with launch) |
| README still attributes to Anthropic | Brand confusion blocks credibility for solo founder | V5-9 |

---

## 2. V5 Testing Philosophy

Inherits V2/V3/V4 regression contract. Every phase gate-in and gate-out:

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

V5 backend tests live in `tests/v5/`. Frontend phases have no Rust tests
(they are tested via Playwright in V5-8 and visually during the phase).

```
tests/
├── v2/   ← must stay green, never touch
├── v3/   ← must stay green, never touch
├── v4/   ← must stay green, never touch
└── v5/
    ├── common.rs
    ├── v5_0_api_expansion.rs
    ├── v5_1_openapi.rs
    ├── v5_2_migration_imports.rs
    ├── v5_3_oidc.rs
    ├── v5_3_saml.rs
    ├── v5_3_scim.rs
    ├── v5_4_audit_chain.rs
    ├── v5_4_data_export.rs
    ├── v5_4_secrets_manager.rs
    ├── v5_5_cost_attribution.rs
    ├── v5_5_budget_forecast.rs
    ├── v5_6_notifications.rs
    ├── v5_6_apm_exporters.rs
    └── v5_7_policy_engine.rs
```

**Test naming**: `v5_{phase}_{feature}_{expected_outcome}` — underscore, never the letter `p` (CLAUDE.md §11).

**External signals per phase** (proof the blocker was removed):

| Phase | External signal |
|---|---|
| V5-0 | RAG sample app produces `/v1/embeddings` → semantic search loop end-to-end |
| V5-1 | `pip install velox` + 3-line example posts to `/v1/chat/completions` |
| V5-2 | `velox import litellm config.yaml` converts a real LiteLLM config to working Velox state |
| V5-3 | Test login via Auth0 + Google Workspace; SCIM provisioning from Okta sandbox |
| V5-4 | `velox audit verify` passes on 100k-row chain; export → re-import roundtrip lossless |
| V5-5 | Cost report split by 3 tag dimensions matches `sum(requests.cost_usd)` exactly |
| V5-6 | Test alert fires into a real Slack workspace + PagerDuty trigger |
| V5-7 | Policy rule blocks a PII-containing request in dry-run mode with audit trail |
| V5-8 | k6 + criterion CI runs produce a public benchmark page; chaos report committed |
| V5-9 | Landing site live at velox.dev; license switched to ELv2; "Anthropic" removed |

---

## 3. Phase V5-0: API Surface Expansion

**Goal**: Make Velox usable for every real LLM workload, not just chat completions.

### 3.1 Problem

The OpenAI-compatible surface today is only `POST /v1/chat/completions`. Any application that calls `client.embeddings.create()`, `client.models.list()`, `client.images.generate()`, or `client.audio.transcriptions.create()` cannot use Velox without changing application code. The OpenAI SDK calls `models.list()` on construction for many integrations — so even chat-only apps fail to initialise.

### 3.2 Endpoints to add

| Endpoint | Cache | Cost tracked | Stream | Notes |
|---|---|---|---|---|
| `POST /v1/embeddings` | Exact only (semantic doesn't apply to itself) | Yes | No | Largest gap — RAG depends on this |
| `GET /v1/models` | No (5s in-memory TTL) | No | No | Aggregated from enabled providers |
| `POST /v1/images/generations` | No (high cost, low repetition) | Yes | No | Pricing per image, not per token |
| `POST /v1/audio/transcriptions` | No (binary input) | Yes (per second) | No | `multipart/form-data` passthrough |
| `POST /v1/audio/speech` | Exact (same text → same audio) | Yes (per char) | Yes (audio chunks) | Binary streaming response |
| `POST /v1/completions` (legacy) | Yes (both layers) | Yes | Yes | Wraps as chat internally — minimal extra code |

### 3.3 Function-calling audit

Extract `tools` from request body and `tool_calls` from response body. Store as JSONB on `requests` table for analytics queries:

```sql
-- migrations/0027_api_expansion.sql
ALTER TABLE requests
    ADD COLUMN tool_calls JSONB,
    ADD COLUMN endpoint   VARCHAR(50) NOT NULL DEFAULT 'chat/completions';
CREATE INDEX idx_requests_endpoint ON requests(endpoint);
CREATE INDEX idx_requests_tool_calls_gin ON requests USING gin(tool_calls);

-- Extend model_pricing for non-text modalities
ALTER TABLE model_pricing
    ADD COLUMN price_per_image       DECIMAL(12,8),
    ADD COLUMN price_per_audio_second DECIMAL(12,8),
    ADD COLUMN price_per_character   DECIMAL(12,8);
```

### 3.4 Provider trait extensions

Add to `src/providers/mod.rs`:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    // existing
    async fn chat_completion(...);
    async fn chat_completion_stream(...);
    // new in V5-0
    async fn embeddings(&self, req: EmbeddingsRequest) -> Result<EmbeddingsResponse>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;
    async fn images_generate(&self, req: ImagesRequest) -> Result<ImagesResponse> {
        Err(ProviderError::Unsupported)  // default — opt-in per provider
    }
    async fn audio_transcribe(&self, req: TranscribeRequest) -> Result<TranscribeResponse> {
        Err(ProviderError::Unsupported)
    }
    async fn audio_speech(&self, req: SpeechRequest) -> Result<SpeechStream> {
        Err(ProviderError::Unsupported)
    }
}
```

Default impls return `Unsupported` so existing adapters (Bedrock, DeepSeek) don't have to implement every modality before V5-0 ships.

### 3.5 Files to create

- `src/handlers/gateway/embeddings.rs`
- `src/handlers/gateway/models.rs`
- `src/handlers/gateway/images.rs`
- `src/handlers/gateway/audio.rs`
- `src/handlers/gateway/completions_legacy.rs`
- `src/providers/embeddings.rs` — shared request/response shapes
- `src/gateway/tool_extract.rs` — extracts `tools`/`tool_calls` for audit log

### 3.6 Files to modify

- `src/providers/openai.rs` — implement all 5 new trait methods
- `src/providers/anthropic.rs` — embeddings, models (no images/audio)
- `src/providers/gemini.rs`, `groq.rs`, `deepseek.rs` — embeddings + models where supported
- `src/providers/bedrock.rs` — embeddings (Titan/Cohere) + models
- `src/gateway/pipeline.rs` — extract `tool_calls` into request record
- `src/db/requests.rs` — `insert_request` accepts `tool_calls` + `endpoint`
- `src/pricing/mod.rs` — handle image / audio / character pricing
- `src/routes/mod.rs` — wire new routes

### 3.7 Test contract

**File**: `tests/v5/v5_0_api_expansion.rs`

```rust
async fn v5_0_embeddings_endpoint_returns_openai_shape()
async fn v5_0_embeddings_exact_cache_hit_returns_cached()
async fn v5_0_embeddings_cost_tracked_in_requests()
async fn v5_0_embeddings_routes_via_priority()
async fn v5_0_list_models_aggregates_across_providers()
async fn v5_0_list_models_cached_for_5_seconds()
async fn v5_0_images_endpoint_passes_through()
async fn v5_0_images_cost_uses_price_per_image()
async fn v5_0_audio_transcription_multipart_upload_works()
async fn v5_0_audio_speech_streams_chunks()
async fn v5_0_completions_legacy_proxies_to_chat()
async fn v5_0_tool_calls_extracted_into_requests_row()
async fn v5_0_endpoint_field_set_per_route()
async fn v5_0_unsupported_modality_returns_404_with_provider_hint()
async fn v5_0_regression_chat_completions_unaffected()
```

### 3.8 Definition of done

```bash
cargo test v5_0
cargo test
cargo clippy -- -D warnings

# External signal: sample RAG app at examples/rag-chatbot/ uses
# /v1/embeddings → pgvector → /v1/chat/completions in one process
cd examples/rag-chatbot && python main.py "what is velox?"
```

---

## 4. Phase V5-1: SDKs, CLI, and OpenAPI

**Goal**: Make integration a single-line code change for any developer, and let operators administer Velox without `curl`.

### 4.1 OpenAPI spec generation

Adopt `utoipa` + `utoipa-swagger-ui`. Every existing admin handler gets a `#[utoipa::path(...)]` annotation. Spec is generated at compile time, served at:

- `GET /admin/openapi.json` — machine-readable OpenAPI 3.1
- `GET /admin/docs` — Swagger UI (no JS bundling — `utoipa-swagger-ui` ships static assets)

```toml
# Cargo.toml — V5-1
utoipa = { version = "5", features = ["axum_extras", "uuid", "chrono", "decimal"] }
utoipa-swagger-ui = { version = "8", features = ["axum"] }
```

### 4.2 `velox` CLI

Single binary, `clap`-based, subcommand structure:

```
velox serve                      # runs the server (default if no subcommand)
velox doctor                     # readiness checks (existing --doctor logic)
velox demo                       # demo mode (existing --demo logic)

velox keys list
velox keys create --name X --budget 100 --rpm 60
velox keys rotate <id>
velox keys revoke <id>

velox migrate up
velox migrate down
velox migrate status

velox config get
velox config set log_request_bodies=true

velox import litellm <config.yaml>
velox import portkey <export.json>
velox import openrouter

velox replay <request-id>
velox audit verify              # (lands fully in V5-4)
velox backup <dest>             # (lands fully in V5-2)
velox restore <src>             # (lands fully in V5-2)
```

The CLI talks to a running Velox via the admin API using a `velox.cli.toml` file (`url`, `admin_token`). When run on the same host as the server it can also speak directly to the DB for `migrate`/`backup`/`restore`.

### 4.3 Python SDK (`velox-python`, separate repo)

Thin wrapper around the official `openai` Python SDK so users get full LLM SDK ergonomics + Velox-specific extras:

```python
from velox import Velox

client = Velox(url="https://velox.acme.com", api_key="vx-sk-...")

# OpenAI-compatible (just like openai.OpenAI)
client.chat.completions.create(model="gpt-4o", messages=[...])

# Velox-specific helpers
client.keys.create(name="prod", budget=100)
client.analytics.overview(period="30d")
client.replay("req-uuid")
client.cache.flush()

# Response headers exposed as typed attributes
resp = client.chat.completions.create(...)
resp.velox.cache_hit         # "exact" | "semantic" | None
resp.velox.cache_similarity  # 0.94 | None
resp.velox.downgraded        # "cost_optimized" | None
resp.velox.audit_hash        # str
```

Auto-generated from the OpenAPI spec for `client.keys`/`analytics`/`replay`/`cache` methods. Hand-coded thin layer over `openai` SDK for chat/embeddings/etc.

### 4.4 Node.js / TypeScript SDK (`velox-node`, separate repo)

Same surface in TypeScript, wraps `openai` npm package. Published to npm as `velox`.

### 4.5 Postman collection

Generated from OpenAPI via `openapi-to-postmanv2`. Shipped at `docs/postman/velox.postman_collection.json`. CI regenerates on every release.

### 4.6 Files to create (server side)

- `src/cli/mod.rs` — clap parser
- `src/cli/keys.rs`, `migrate.rs`, `config.rs`, `import.rs`, `backup.rs`
- `src/openapi.rs` — `OpenApiDoc` derive root combining all handler annotations
- `src/handlers/admin/docs.rs` — serves Swagger UI + JSON

### 4.7 Files to modify

- `src/main.rs` — replace ad-hoc `--doctor` / `--demo` flag parsing with `velox::cli::run()`
- Every `src/handlers/admin/*.rs` — add `#[utoipa::path(...)]` annotations
- `Cargo.toml` — add `utoipa`, `utoipa-swagger-ui`, `clap` features (`derive`, `env`)

### 4.8 Test contract

**File**: `tests/v5/v5_1_openapi.rs`

```rust
async fn v5_1_openapi_json_endpoint_returns_valid_spec()
async fn v5_1_openapi_includes_all_admin_endpoints()
async fn v5_1_openapi_spec_validates_against_3_1_schema()
async fn v5_1_swagger_ui_endpoint_returns_200()
fn v5_1_cli_keys_create_invokes_admin_api()
fn v5_1_cli_migrate_status_reads_migrations_table()
fn v5_1_cli_config_get_reads_admin_config()
fn v5_1_cli_help_lists_all_subcommands()
async fn v5_1_regression_existing_handler_responses_unchanged()
```

SDK tests live in their own repos (`velox-python/tests/`, `velox-node/test/`) and are gated by their own CI.

### 4.9 Definition of done

```bash
cargo test v5_1
cargo test
cargo clippy -- -D warnings

# External signal: 3-line Python script works
pip install velox
python -c "from velox import Velox; v=Velox(url='http://localhost:8080', api_key='vx-sk-...'); print(v.chat.completions.create(model='gpt-4o-mini', messages=[{'role':'user','content':'hi'}]).choices[0].message.content)"
```

---

## 5. Phase V5-2: Deployment & Migration Tooling

**Goal**: Eliminate the deploy and switching-cost friction that keeps prospects on competitors.

### 5.1 Helm chart

`charts/velox/` containing:
- `values.yaml` with sensible defaults
- HPA (CPU + custom metric: requests/sec)
- ServiceMonitor for Prometheus Operator
- Ingress (nginx/traefik examples)
- Secrets references for `JWT_SECRET`, `ENCRYPTION_KEY`, provider keys
- External Postgres reference (does not bundle Postgres — see VELOX_FUTURE §3)
- Optional Qdrant subchart toggle

Published to a GitHub Pages helm repo (`https://farzad.github.io/velox-charts`).

### 5.2 Terraform provider (`terraform-provider-velox`, separate repo)

Resources:
- `velox_provider`
- `velox_api_key`
- `velox_workspace`
- `velox_alert`
- `velox_prompt`
- `velox_workspace_member`

Built against the OpenAPI spec from V5-1 via `terraform-plugin-framework`. Published to the Terraform Registry.

### 5.3 One-click deploy

| Platform | Artifact | Path |
|---|---|---|
| Railway | `railway.json` + Postgres template | `deploy/railway/` |
| Fly.io | `fly.toml` + `fly launch` config | `deploy/fly/` |
| Render | `render.yaml` Blueprint | `deploy/render/` |
| AWS Marketplace AMI | Packer config + launch script | `deploy/aws-ami/` (defer to V6 if time-constrained) |

### 5.4 Migration importers (CLI commands from V5-1)

Each importer reads the competitor's config format and creates equivalent Velox state via the admin API:

```bash
velox import litellm   path/to/proxy_config.yaml
velox import portkey   path/to/portkey_export.json
velox import openrouter   # uses OpenRouter's public model API
```

**LiteLLM mapping** (reads `model_list:` and `general_settings:`):
- LiteLLM model entry → Velox provider + key
- LiteLLM `router_settings.routing_strategy` → Velox `routing_strategy` on api_keys
- LiteLLM `litellm_settings.cache` → Velox cache config

**Portkey mapping**:
- Virtual key → Velox api_key
- Provider config → Velox provider
- Routing rules → Velox routing_strategy (best-effort; complex rules become V5-7 policies)

### 5.5 Backup / Restore CLI

```bash
velox backup ./velox-backup-2026-06-01.tar.gz
# Bundles:
#   - pg_dump of velox DB
#   - models/ directory (ONNX + tokenizer)
#   - velox.toml
#   - schema version stamp

velox restore ./velox-backup-2026-06-01.tar.gz
# Restores all of the above, with version compatibility check
```

Wraps `pg_dump`/`pg_restore` under the hood, but provides a single artifact and version safety. Cron example documented.

### 5.6 HA deployment guide

New doc: `docs/deployment/ha.md`:
- Postgres primary + replica setup
- ≥2 Velox nodes behind LB (V2-6 clustering already supports this)
- External Qdrant (V4-9)
- Encryption key rotation procedure
- DR runbook

### 5.7 Files to create

- `charts/velox/` — full Helm chart
- `deploy/railway/railway.json`, `deploy/fly/fly.toml`, `deploy/render/render.yaml`
- `src/cli/import/litellm.rs`, `portkey.rs`, `openrouter.rs`
- `src/cli/backup.rs`
- `docs/deployment/ha.md`
- `docs/deployment/helm.md`
- `docs/deployment/terraform.md`

### 5.8 Test contract

**File**: `tests/v5/v5_2_migration_imports.rs`

```rust
fn v5_2_litellm_yaml_parses_to_provider_list()
fn v5_2_litellm_routing_strategy_maps_correctly()
fn v5_2_portkey_export_parses_to_provider_list()
fn v5_2_openrouter_import_creates_model_aliases()
fn v5_2_backup_produces_complete_archive()
fn v5_2_restore_roundtrip_preserves_all_tables()
fn v5_2_restore_rejects_incompatible_version()
fn v5_2_helm_chart_lint_passes()    # invokes `helm lint charts/velox`
fn v5_2_helm_template_produces_valid_k8s_yaml()
```

### 5.9 Definition of done

```bash
cargo test v5_2
helm lint charts/velox
helm template charts/velox | kubeval

# External signal: real LiteLLM proxy_config.yaml converts cleanly
velox import litellm test-fixtures/litellm-sample.yaml
# → reports N providers created, K keys created, 0 errors
```

---

## 6. Phase V5-3: Enterprise Auth (OIDC, SAML, SCIM)

**Goal**: Unblock every deal where IT controls login.

This is a large phase. It ships in three sub-phases, each independently shippable.

### 6.1 Schema

```sql
-- migrations/0028_sso_identity.sql
CREATE TABLE identity_providers (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind            VARCHAR(20) NOT NULL,    -- 'oidc' | 'saml'
    name            VARCHAR(100) NOT NULL,
    config          JSONB NOT NULL,          -- discovery URL / IdP metadata / client_id etc.
    group_role_map  JSONB NOT NULL DEFAULT '{}',  -- {"velox-admin":"admin",...}
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE identities (
    id           UUID PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    idp_id       UUID NOT NULL REFERENCES identity_providers(id) ON DELETE CASCADE,
    external_id  TEXT NOT NULL,
    last_login   TIMESTAMPTZ,
    UNIQUE (idp_id, external_id)
);

CREATE TABLE scim_tokens (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         VARCHAR(100) NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at   TIMESTAMPTZ
);
```

### 6.2 Sub-phase V5-3a — OIDC + SCIM

**OIDC**:
- `openidconnect` crate handles Authorization Code + PKCE flow
- New routes: `GET /auth/oidc/:idp_id/start`, `GET /auth/oidc/:idp_id/callback`
- On callback: verify ID token, look up `identities` row, JIT-create user if absent, mint Velox JWT
- Group claims → Velox role via `group_role_map`

**SCIM 2.0**:
- New routes under `/scim/v2/`:
  - `GET/POST /Users`
  - `GET/PUT/PATCH/DELETE /Users/:id`
  - `GET/POST /Groups`
- Auth via `Authorization: Bearer <scim_token>` (separate from admin JWT)
- Idempotent operations; mapped to existing `users` + `workspace_members` tables

**Dashboard**:
- `/settings/sso` page — connect IdP, paste discovery URL, configure group mapping, "test connection"
- `/settings/scim` page — generate SCIM token, copy once, list active integrations

### 6.3 Sub-phase V5-3b — SAML 2.0

- `samael` crate for SAML assertion validation
- New routes: `POST /auth/saml/:idp_id/acs`, `GET /auth/saml/:idp_id/metadata`
- IdP-initiated and SP-initiated flows
- Sign-out via SAML SLO (best-effort)
- Same `identities` table; `idp_kind = 'saml'`

### 6.4 Files to create

- `src/auth/oidc.rs`
- `src/auth/saml.rs`
- `src/auth/scim.rs`
- `src/handlers/auth/sso.rs`
- `src/handlers/scim/users.rs`, `groups.rs`
- `src/db/identities.rs`
- `dashboard/src/app/(dashboard)/settings/sso/page.tsx`
- `dashboard/src/app/(dashboard)/settings/scim/page.tsx`

### 6.5 Test contract

**File**: `tests/v5/v5_3_oidc.rs`

```rust
async fn v5_3_oidc_callback_creates_user_jit()
async fn v5_3_oidc_callback_returns_existing_user_on_subsequent_login()
async fn v5_3_oidc_group_claim_maps_to_role()
async fn v5_3_oidc_invalid_id_token_rejected()
async fn v5_3_oidc_state_param_csrf_protected()
async fn v5_3_oidc_disabled_idp_returns_404()
```

**File**: `tests/v5/v5_3_saml.rs`

```rust
async fn v5_3_saml_acs_valid_assertion_creates_session()
async fn v5_3_saml_invalid_signature_rejected()
async fn v5_3_saml_expired_assertion_rejected()
async fn v5_3_saml_metadata_endpoint_returns_xml()
async fn v5_3_saml_group_attribute_maps_to_role()
```

**File**: `tests/v5/v5_3_scim.rs`

```rust
async fn v5_3_scim_create_user_succeeds_with_token()
async fn v5_3_scim_create_user_rejects_without_token()
async fn v5_3_scim_update_user_deactivation_revokes_access()
async fn v5_3_scim_get_users_paginated()
async fn v5_3_scim_delete_user_cascades_to_memberships()
async fn v5_3_scim_token_revocation_blocks_further_calls()
```

### 6.6 Definition of done

```bash
cargo test v5_3
cargo test
cargo clippy -- -D warnings

# External signals:
# - Successful OIDC login from Google Workspace test tenant
# - Successful SAML login from Auth0 test tenant
# - SCIM provisioning from Okta sandbox creates 5 users + revokes 1
```

---

## 7. Phase V5-4: Compliance & Audit Hardening

**Goal**: Pass a SOC 2 Type II readiness review and a GDPR/HIPAA RFP without ad-hoc changes.

### 7.1 Tamper-evident audit chain

Each row in `requests` carries the SHA-256 of the previous row's audit hash, forming a chain.

```sql
-- migrations/0029_audit_chain.sql
ALTER TABLE requests
    ADD COLUMN audit_hash     TEXT,
    ADD COLUMN previous_hash  TEXT,
    ADD COLUMN chain_sequence BIGSERIAL;
CREATE INDEX idx_requests_chain_sequence ON requests(chain_sequence);
```

Hash inputs: `request_id || api_key_id || created_at || status || cost_usd || tokens_total || previous_hash`.

`velox audit verify [--from <id>] [--to <id>]` walks the chain and reports first mismatch (if any). Existing rows pre-V5-4 are stamped with `chain_sequence = 0` and excluded from verification.

### 7.2 Data export (DSAR / Article 15)

Streaming JSONL or CSV export of all data tied to a workspace or a single user:

- `GET /admin/export/workspace/:id?format=jsonl` — all requests, keys, alerts for a workspace
- `GET /admin/export/user/:id?format=jsonl` — all DSAR-relevant fields for a single user (their requests across workspaces, account metadata)

Both are admin-only. Streamed via `axum::body::Body` to avoid loading everything into memory. Includes a manifest with row counts and the audit chain range covered.

### 7.3 Right to be forgotten (Article 17)

```bash
DELETE /admin/users/:id?cascade=true&redact_audit=true
```

- Removes user, identities, workspace_members rows
- Optionally redacts `request_body.messages[].content` and `response_body.choices[].message.content` to `[redacted]` while preserving the audit row (so the chain stays intact)
- Logs the deletion event itself to `audit_events` table (new) for compliance proof

### 7.4 Secrets manager integration

Provider keys + encryption keys can be sourced from external secret backends:

```toml
# velox.toml
encryption_key = "vault://secret/data/velox#encryption_key"
[providers.openai]
api_key = "aws-sm://prod/velox/openai_key"
```

Supported backends:
- `env://VAR_NAME` (default — current behaviour)
- `file:///etc/velox/secrets/openai.key`
- `vault://path/to/secret#field` (HashiCorp Vault, KV v2)
- `aws-sm://secret-name` (AWS Secrets Manager)
- `gcp-sm://projects/p/secrets/s/versions/v` (GCP Secret Manager)
- `k8s://namespace/secret-name#key` (Kubernetes Secret)

Resolved at startup; refresh on SIGHUP for rotation without restart.

### 7.5 Data residency

```sql
-- migrations/0030_workspace_region.sql
ALTER TABLE workspaces
    ADD COLUMN region        VARCHAR(20) DEFAULT 'global',
    ADD COLUMN allowed_regions TEXT[];
ALTER TABLE providers
    ADD COLUMN region VARCHAR(20);
```

Router enforces: a workspace with `region = 'eu'` can only select providers with `region IN ('eu', NULL)` (NULL = unrestricted). Violation → 403 with explanation header. Configurable per-workspace via `/settings/region`.

### 7.6 Compliance docs

New section `docs/compliance/`:
- `soc2.md` — control matrix mapping SOC 2 Trust Service Criteria → Velox features
- `gdpr.md` — Articles 15, 17, 20, 32 → Velox features
- `hipaa.md` — BAA-readiness checklist (encryption, audit, access controls)
- `cloud-acts.md` — what data leaves the user's infrastructure (none; clarify "self-hosted means self-hosted")

### 7.7 Files to create

- `src/audit/chain.rs`
- `src/handlers/admin/export.rs`
- `src/handlers/admin/users_delete.rs`
- `src/secrets/mod.rs` — Backend trait
- `src/secrets/env.rs`, `file.rs`, `vault.rs`, `aws_sm.rs`, `gcp_sm.rs`, `k8s.rs`
- `src/cli/audit.rs` — `velox audit verify`
- `docs/compliance/soc2.md`, `gdpr.md`, `hipaa.md`, `cloud-acts.md`

### 7.8 Test contract

**File**: `tests/v5/v5_4_audit_chain.rs`

```rust
async fn v5_4_first_row_after_migration_has_genesis_previous_hash()
async fn v5_4_each_row_chains_correctly()
async fn v5_4_verify_passes_on_intact_chain()
async fn v5_4_verify_detects_tampered_row()
async fn v5_4_verify_detects_deleted_row()
async fn v5_4_chain_sequence_monotonic_under_concurrency()
```

**File**: `tests/v5/v5_4_data_export.rs`

```rust
async fn v5_4_workspace_export_streams_all_requests()
async fn v5_4_user_export_includes_cross_workspace_data()
async fn v5_4_export_manifest_row_counts_match_actual()
async fn v5_4_user_delete_cascades_correctly()
async fn v5_4_user_delete_with_redact_preserves_chain()
async fn v5_4_user_delete_logged_to_audit_events()
```

**File**: `tests/v5/v5_4_secrets_manager.rs`

```rust
fn v5_4_env_backend_resolves_var()
fn v5_4_file_backend_resolves_path()
async fn v5_4_vault_backend_resolves_kv2()
async fn v5_4_aws_sm_backend_resolves_secret()
fn v5_4_unknown_scheme_returns_error()
async fn v5_4_sighup_refreshes_secrets()
async fn v5_4_workspace_region_blocks_cross_region_provider()
```

### 7.9 Definition of done

```bash
cargo test v5_4
velox audit verify --from beginning   # passes on production-shaped data
# External signal: workspace export → re-import roundtrip on a fresh DB → row counts match
```

---

## 8. Phase V5-5: Cost Attribution & FinOps

**Goal**: Let finance teams answer "who spent what?" without building a side spreadsheet.

### 8.1 Request tags

Clients send tags via either:
- Header: `X-Velox-Tags: team=acme,project=rag,user=u_123`
- OpenAI-compatible `metadata` field in request body (preferred — already in OpenAI spec)

Stored as JSONB on `requests`:

```sql
-- migrations/0031_request_tags.sql
ALTER TABLE requests ADD COLUMN tags JSONB DEFAULT '{}';
CREATE INDEX idx_requests_tags_gin ON requests USING gin(tags);
```

### 8.2 Cost breakdown endpoints

```
GET /admin/analytics/cost?group_by=tag.team&period=30d
GET /admin/analytics/cost?group_by=tag.project&period=30d
GET /admin/analytics/cost?group_by=tag.team,tag.project&period=30d
GET /admin/analytics/cost?group_by=workspace,model&period=30d
```

Returns:
```json
{
  "data": {
    "period": "30d",
    "total_cost_usd": 1420.50,
    "groups": [
      {"key": {"tag.team": "acme"}, "cost_usd": 820.40, "request_count": 12400},
      {"key": {"tag.team": "beta"}, "cost_usd": 600.10, "request_count": 8200}
    ]
  }
}
```

### 8.3 Budget forecasting

Background task every hour computes linear regression on the last 30 days of daily spend per key/workspace. Result stored in `daily_costs` extension:

```sql
ALTER TABLE daily_costs
    ADD COLUMN forecast_eom_usd  DECIMAL(12,8),  -- end-of-month projection
    ADD COLUMN forecast_eob_days INTEGER;        -- days until budget exhausted
```

Dashboard widget on `/overview`: spend trajectory line with budget line overlay; "at current rate you will hit budget on May 18".

Alerts integration: new alert type `forecast_exceeds_budget` triggers when projection crosses the line.

### 8.4 Invoice generation

```
GET /admin/billing/invoice?workspace=:id&month=2026-04&format=pdf|html|csv
```

Returns:
- PDF (server-side via `printpdf` crate, no headless browser)
- HTML (renderable in dashboard for preview)
- CSV (Stripe-compatible columns for finance imports)

Invoice includes: line items by tag dimension chosen at config time, subtotal, optional markup, total in USD with daily exchange-rate snapshot if needed.

### 8.5 Chargeback configuration

Workspace-level config in dashboard:
- Primary tag dimension (e.g. `team`)
- Secondary dimension (optional, e.g. `project`)
- Markup percentage (for internal cost recovery)
- Default tag value if missing

### 8.6 Files to create

- `src/handlers/admin/cost_breakdown.rs`
- `src/handlers/admin/invoice.rs`
- `src/analytics/forecast.rs` — hourly task
- `src/billing/pdf.rs`, `csv.rs`
- `dashboard/src/app/(dashboard)/billing/page.tsx`
- `dashboard/src/app/(dashboard)/analytics/cost/page.tsx`

### 8.7 Files to modify

- `src/gateway/pipeline.rs` — extract tags from header or `metadata` body field
- `src/db/requests.rs` — store `tags`
- `src/main.rs` — start forecast background task

### 8.8 Test contract

**File**: `tests/v5/v5_5_cost_attribution.rs`

```rust
async fn v5_5_header_tags_extracted_into_requests()
async fn v5_5_metadata_body_tags_extracted_into_requests()
async fn v5_5_header_overrides_body_when_both_present()
async fn v5_5_cost_breakdown_sums_match_raw_cost()
async fn v5_5_cost_breakdown_multi_dimension_groups_correctly()
async fn v5_5_invoice_pdf_renders_without_error()
async fn v5_5_invoice_csv_matches_stripe_format()
async fn v5_5_default_tag_value_applied_when_missing()
```

**File**: `tests/v5/v5_5_budget_forecast.rs`

```rust
async fn v5_5_forecast_linear_regression_correct_on_known_data()
async fn v5_5_forecast_handles_sparse_data_without_crash()
async fn v5_5_forecast_alert_triggers_on_threshold_cross()
async fn v5_5_forecast_updated_hourly_by_background_task()
```

### 8.9 Definition of done

```bash
cargo test v5_5
# External signal: 3-dimension cost breakdown sums match SUM(cost_usd) to within 1 cent
```

---

## 9. Phase V5-6: Notifications & APM Integrations

**Goal**: Plug into the tools customer oncall and observability teams already use.

### 9.1 Notification channels (extend V2-2)

New schema:

```sql
-- migrations/0032_notification_channels.sql
CREATE TABLE notification_channels (
    id           UUID PRIMARY KEY,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
    kind         VARCHAR(30) NOT NULL,   -- 'slack' | 'teams' | 'pagerduty' | 'email' | 'discord'
    name         VARCHAR(100) NOT NULL,
    config       JSONB NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
ALTER TABLE alerts ADD COLUMN channel_ids UUID[];
```

Each channel implements a `NotificationChannel` trait:

```rust
#[async_trait]
pub trait NotificationChannel: Send + Sync {
    async fn send(&self, event: &AlertEvent) -> Result<()>;
    fn kind(&self) -> &str;
}
```

Implementations:
- **Slack** — Bot API (`chat.postMessage` with blocks); workspace + channel selector via OAuth in dashboard
- **Microsoft Teams** — incoming webhook + AdaptiveCard payload
- **PagerDuty** — Events API v2 (`trigger`, `acknowledge`, `resolve` lifecycle)
- **Email** — `lettre` crate, SMTP credentials per workspace
- **Discord** — webhook + embed

Alerts can target multiple channels via `alerts.channel_ids[]`.

### 9.2 APM exporters

Generic `MetricsExporter` trait alongside the existing Prometheus path:

- **Datadog** — `dogstatsd` UDP + Logs API
- **New Relic** — Insights Events API (`POST /v1/accounts/:id/events`)
- **Honeycomb** — already supported via OTel (V3-2); document the recipe, no new code

Config:

```toml
[exporters.datadog]
enabled = true
api_key = "..."
site    = "datadoghq.com"

[exporters.newrelic]
enabled = true
license_key = "..."
account_id  = "..."
```

Exporters consume the existing metrics atomic gauges + `tracing` events — no changes to the event sources.

### 9.3 SLA reports

Background task, last day of each month, generates per-workspace SLA report:

- Uptime % (from `requests` table — successful proxied requests / total)
- p50, p95, p99 latency
- Error rate by provider
- Top 5 costs
- Cache savings
- Output: HTML email to all `admin` role members of the workspace, plus PDF artifact retrievable via `GET /admin/sla/:workspace/:month`

### 9.4 Rate-limit analytics page (`/analytics/rate-limits`)

Reads existing rate-limit hit events from `requests` (status_code = 429). Shows:
- Keys with most 429s in last 7d
- Peak usage windows (heatmap by hour of week)
- Recommendation: "key X would benefit from raising rpm from N to M"

Pure frontend — no new backend code.

### 9.5 Function-calling analytics page (`/analytics/tools`)

Uses `requests.tool_calls` from V5-0:
- Most-called tools per key/workspace
- Avg latency / cost per tool
- Tool error rate (tool returned error in subsequent request)

Pure frontend.

### 9.6 Files to create

- `src/notifications/mod.rs` — `NotificationChannel` trait + dispatch
- `src/notifications/slack.rs`, `teams.rs`, `pagerduty.rs`, `email.rs`, `discord.rs`
- `src/exporters/mod.rs` — `MetricsExporter` trait
- `src/exporters/datadog.rs`, `newrelic.rs`
- `src/handlers/admin/channels.rs` — CRUD for notification channels
- `src/handlers/admin/sla.rs`
- `src/analytics/sla.rs` — monthly background task
- `dashboard/src/app/(dashboard)/notifications/page.tsx`
- `dashboard/src/app/(dashboard)/analytics/rate-limits/page.tsx`
- `dashboard/src/app/(dashboard)/analytics/tools/page.tsx`

### 9.7 Test contract

**File**: `tests/v5/v5_6_notifications.rs`

```rust
async fn v5_6_slack_channel_sends_block_payload()
async fn v5_6_teams_channel_sends_adaptive_card()
async fn v5_6_pagerduty_trigger_acknowledge_resolve_lifecycle()
async fn v5_6_email_channel_sends_via_smtp()
async fn v5_6_discord_channel_sends_embed()
async fn v5_6_alert_dispatches_to_multiple_channels()
async fn v5_6_channel_disable_skips_send()
async fn v5_6_channel_failure_does_not_block_others()
```

**File**: `tests/v5/v5_6_apm_exporters.rs`

```rust
fn v5_6_datadog_exporter_emits_dogstatsd_format()
fn v5_6_newrelic_exporter_emits_insights_event_format()
async fn v5_6_exporter_disabled_when_config_missing()
async fn v5_6_exporter_failure_isolated_from_request_path()
```

### 9.8 Definition of done

```bash
cargo test v5_6
# External signal: test alert reaches a real Slack workspace + real PagerDuty incident triggered + Datadog dashboard shows velox_* metrics
```

---

## 10. Phase V5-7: Governance & Safety

**Goal**: Give security teams the answers they need before greenlighting Velox.

### 10.1 Policy engine

Admin-configurable rules evaluated in the request pipeline, between RBAC and rate-limit gates.

```sql
-- migrations/0033_policies.sql
CREATE TABLE policies (
    id           UUID PRIMARY KEY,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
    name         VARCHAR(100) NOT NULL,
    condition    JSONB NOT NULL,        -- {"all":[{"model":"gpt-4o"},{"tag.team":"free-tier"}]}
    action       JSONB NOT NULL,        -- {"reject":{"message":"..."}}
    priority     INTEGER NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    dry_run      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE TABLE policy_events (
    id           UUID PRIMARY KEY,
    policy_id    UUID NOT NULL REFERENCES policies(id),
    request_id   UUID,
    matched      BOOLEAN NOT NULL,
    action_taken VARCHAR(20) NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

**Condition DSL** (JSON, evaluated server-side — no eval, no scripting):

```json
{"all": [
  {"model": "gpt-4o"},
  {"tag.team": "free-tier"}
]}
{"any": [
  {"prompt_matches_pattern": "credit_card"},
  {"prompt_matches_pattern": "ssn"}
]}
```

**Actions**:
- `reject` (return 403 with message)
- `downgrade` (override `model` or routing strategy)
- `route_to` (force provider id)
- `redact` (strip matched pattern from prompt before forwarding)
- `tag` (add a tag — useful for audit cohorts)
- `log_only` (matches but does nothing else; combine with `dry_run` for soft-launch)

Dry-run mode: matched events written to `policy_events` but action skipped, so admins can validate before enforcement.

### 10.2 Prompt injection detection

Optional V3-4 plugin shipped as part of V5-7. Uses:
- Pattern library (curated list of known jailbreaks)
- Optional small ONNX classifier (already linked via embedding model — no new heavy dep)

Sets `X-Velox-Injection-Score: 0.0-1.0` header. Combine with policy engine to act on it: `{"injection_score_gt": 0.7}` → reject.

### 10.3 Content moderation hook

Optional pre-flight call to:
- OpenAI Moderation API (free), or
- Local ONNX moderation model (configurable)

Behavior configurable: `block` | `flag` | `log`. Adds latency only when enabled.

### 10.4 Egress allowlist

Per workspace: `allowed_providers UUID[]` on `workspaces`. Router refuses any provider not in the list. Combined with V5-4 data residency = strong egress controls.

### 10.5 Dashboard

New `/governance` page:
- Policy list with priority ordering (drag-to-reorder)
- Visual rule builder (no JSON editing required) — condition tree + action selector
- Dry-run toggle per policy
- Audit drawer: last N events for any policy, with matched request snippet
- Templates: "Block PII to public providers", "Cost cap for non-prod", "Compliance mode for HIPAA workspace"

### 10.6 Files to create

- `src/policy/mod.rs` — `Policy` + `Condition` + `Action` types
- `src/policy/eval.rs` — condition evaluator
- `src/policy/builtin_patterns.rs` — curated regex set (pii, secrets, jailbreaks)
- `src/handlers/admin/policies.rs`
- `src/plugins/injection_detector.rs`
- `src/plugins/moderation.rs`
- `dashboard/src/app/(dashboard)/governance/page.tsx`
- `dashboard/src/components/PolicyBuilder.tsx`

### 10.7 Files to modify

- `src/gateway/pipeline.rs` — evaluate policies after RBAC, before rate-limit
- `src/state.rs` — add `policy_engine: Arc<PolicyEngine>` cache

### 10.8 Test contract

**File**: `tests/v5/v5_7_policy_engine.rs`

```rust
fn v5_7_condition_all_matches_when_all_true()
fn v5_7_condition_any_matches_when_one_true()
fn v5_7_pattern_match_detects_credit_card()
fn v5_7_pattern_match_detects_ssn()
async fn v5_7_reject_action_returns_403()
async fn v5_7_downgrade_action_overrides_model()
async fn v5_7_route_to_action_forces_provider()
async fn v5_7_redact_action_modifies_prompt_before_forward()
async fn v5_7_dry_run_logs_match_but_does_not_act()
async fn v5_7_priority_order_evaluated_correctly()
async fn v5_7_disabled_policy_skipped()
async fn v5_7_egress_allowlist_blocks_unlisted_provider()
async fn v5_7_injection_score_attached_when_plugin_enabled()
async fn v5_7_moderation_block_action_rejects_disallowed_content()
async fn v5_7_regression_no_policies_means_no_overhead()
```

### 10.9 Definition of done

```bash
cargo test v5_7
# External signal: ship 3 template policies; one of them catches a planted PII test request in dry-run and emits an audit event
```

---

## 11. Phase V5-8: Performance & Reliability Proofs

**Goal**: When a buyer asks "how fast? how reliable?" you have numbers, they're public, and they were produced by CI.

**This phase is mostly tests + infra. Zero new product features.**

### 11.1 Load test suite

`benches/load/` — k6 scripts driven by GitHub Actions:

- `01-baseline.js` — 100 req/s for 5 min, chat completions, mixed models
- `02-spike.js` — 500 req/s burst from 50
- `03-sustained.js` — 1000 req/s for 5 min
- `04-dedup-amplification.js` — N identical concurrent requests; verify only one provider call
- `05-cache-cold-to-warm.js` — measure cache fill rate + hit ratio trajectory

CI workflow `.github/workflows/load.yml` runs these on a known instance type (e.g. AWS c6i.xlarge) nightly + on tagged releases. Results posted to S3 + rendered in `docs/benchmarks/`.

### 11.2 Chaos test suite

`tests/v5/v5_8_chaos.rs` — uses `toxiproxy-rs` for failure injection:

- Provider connection cut mid-stream → SSE downstream gets clean error
- DB connection drops → in-flight requests fail fast, new requests get 503 until pool recovers
- Provider returns 5xx → failover triggers
- Cache layer panics → falls through to provider
- OTel exporter dead → zero impact on request path
- Disk fills (90%) → graceful degradation, warning logged

### 11.3 Playwright E2E suite

`dashboard/e2e/` — one suite per dashboard page (Alerts, Keys, Providers, Prompts, Playground, Cost Simulator, Workspaces, Notifications, Governance). Each suite: create / edit / delete the canonical entity, verify API calls succeeded, verify error states render.

CI workflow `.github/workflows/e2e.yml` runs against `velox demo` instance.

### 11.4 Performance regression CI

Criterion benches (existing `benches/cache_bench.rs` + new ones) gate PRs:

- Cache lookup p95
- Cosine scan over 10k entries
- Request pipeline overhead (auth → cache → forward)
- Embeddings inference p95

Any >10% regression vs `main` baseline fails CI.

### 11.5 Public benchmark page

`docs/benchmarks/index.md` — auto-updated from CI:
- Latest run timestamp + Velox version
- Chart: req/s vs p95 latency at 100/500/1000 RPS
- Chart: cache hit ratio over time
- Side-by-side comparison: Velox vs LiteLLM at matched config (same providers, same cache settings)

### 11.6 Files to create

- `benches/load/*.js`
- `tests/v5/v5_8_chaos.rs`
- `dashboard/e2e/` — Playwright config + per-page specs
- `.github/workflows/load.yml`, `e2e.yml`, `bench-regression.yml`
- `docs/benchmarks/index.md` + auto-generated subpages

### 11.7 Test contract

**File**: `tests/v5/v5_8_chaos.rs`

```rust
async fn v5_8_chaos_provider_disconnect_mid_stream_emits_clean_error()
async fn v5_8_chaos_db_disconnect_returns_503_until_pool_recovers()
async fn v5_8_chaos_provider_5xx_triggers_failover()
async fn v5_8_chaos_cache_panic_falls_through_to_provider()
async fn v5_8_chaos_otel_exporter_dead_zero_impact()
```

### 11.8 Definition of done

```bash
cargo test v5_8
npx playwright test
# CI gate: load tests green, regression CI green
# External signal: docs/benchmarks/index.md committed with real numbers
```

---

## 12. Phase V5-9: Docs, Onboarding, and GTM Assets

**Goal**: Convert visitor → first successful request in under 5 minutes, and remove brand confusion.

**This is mostly content + a small Next.js site. Zero new backend code.**

### 12.1 License switch (ELv2)

- Replace `LICENSE` file (MIT → ELv2)
- Update `Cargo.toml` `license = "Elastic-2.0"`
- Update README + `docs/` license headers
- Update package metadata in `velox-python/` and `velox-node/`

### 12.2 Brand cleanup

- Replace every "Anthropic" mention in README, CHANGELOG, docs, dashboard footer
- Replace `ghcr.io/anthropics/velox` image path with new namespace
- Replace `support@anthropic.com` with new support email
- Update repo description on GitHub

### 12.3 Marketing landing (`velox.dev`)

Separate Next.js site at `marketing/` in repo (or separate repo `velox-www`):
- Hero: positioning statement, demo gif, "Run in 60 seconds" CTA
- Social proof: install command, GitHub star count, customer logos when present
- Comparison table: Velox vs LiteLLM/Portkey/Helicone/Cloudflare
- Features grid (cached, observable, governed, multi-tenant, self-hosted)
- Pricing: "Self-hosted: free under ELv2. Managed cloud: waitlist."
- Footer: docs, GitHub, Discord, status page, blog

Hosted on Vercel/Cloudflare Pages. Custom domain.

### 12.4 Comparison pages

Each is a long-form honest comparison:
- `/vs/litellm` — Velox is a product, LiteLLM is a library. When to pick which.
- `/vs/portkey` — Both products. Self-hosted vs SaaS tradeoffs.
- `/vs/helicone` — Velox includes routing + caching; Helicone is observability-only.
- `/vs/cloudflare-ai-gateway` — Edge cost vs feature depth.

Each page ends with a `velox import <competitor>` CTA.

### 12.5 Quickstart 2.0

- 60-second demo video embedded on landing + in dashboard onboarding
- Interactive in-dashboard tour via `react-joyride`:
  1. Create your first key
  2. Send a test request from the Playground
  3. View it in Requests
  4. Configure your first alert
  5. (Optional) Connect SSO
- Tour state persisted in `users.tour_completed_steps[]`

### 12.6 Sample apps repo (`velox-examples`)

Each in `examples/<app>/` with a README:
- `rag-chatbot` — Python: pgvector + `/v1/embeddings` + `/v1/chat/completions`, uses `velox-python`
- `tool-using-agent` — Node: function calling + V5-0 tool analytics
- `multi-provider-failover` — Demonstrates V3 failover behavior under provider outage
- `embeddings-search` — Semantic search over a corpus, shows cache hit ratio over time
- `compliance-mode-deployment` — Velox configured for HIPAA-ready posture (V5-4 + V5-7 settings)

### 12.7 Docs site

`docs/` migrates to Mintlify or Docusaurus, served at `docs.velox.dev` (or embedded at `/docs` in Velox itself for self-hosted users):
- API reference: auto-generated from V5-1 OpenAPI
- Guides: deployment, RBAC, SSO, compliance, governance, cost attribution
- Tutorials: every sample app
- Migration guides: from LiteLLM/Portkey/Helicone

### 12.8 Community

- Discord server with channels: `#announcements`, `#general`, `#self-hosting`, `#feature-requests`, `#showcase`, `#contributors`
- GitHub Discussions enabled and triaged weekly
- CONTRIBUTING.md (was Anthropic-branded — needs rewrite)
- Code of Conduct
- Issue templates: bug, feature, question, security

### 12.9 Status page

`status.velox.dev` — Statuspage.io (free tier) or self-hosted Cachet. Subscribe-by-email, RSS, Slack integration. Used for managed-cloud beta (V6) but launched now to establish trust.

### 12.10 Blog launch sequence (5 posts over 4 weeks)

1. *Why I built Velox in Rust* — founder story
2. *Semantic cache deep dive: how a 384-dim vector saved $X* — technical
3. *From LiteLLM to Velox in 10 minutes* — migration
4. *RBAC, audit logs, and SSO: the boring parts that matter* — enterprise readiness
5. *Cost attribution by team — without the spreadsheet* — finance

Each post syndicated to dev.to, Hacker News, /r/MachineLearning, /r/selfhosted, AI Engineer Slack/Discord communities.

### 12.11 Files to create

- `LICENSE` (replace MIT with ELv2)
- `marketing/` — Next.js landing site
- `examples/rag-chatbot/`, `tool-using-agent/`, etc.
- `docs/vs/litellm.md`, `portkey.md`, `helicone.md`, `cloudflare-ai-gateway.md`
- `dashboard/src/components/OnboardingTour.tsx`
- `.github/ISSUE_TEMPLATE/*.yml`
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`
- `blog/2026-06-01-why-rust.md` ... 5 posts

### 12.12 Files to modify

- `README.md` — full rewrite: new license, new brand, new positioning, link to landing
- `CHANGELOG.md` — V5 entries
- Every `docs/*.md` — strip "Anthropic" mentions
- `dashboard/src/app/layout.tsx` — footer brand
- `Cargo.toml` — package metadata, license field, authors

### 12.13 Definition of done

```bash
# Self-checks
grep -ri "anthropic" --include="*.md" --include="*.rs" --include="*.toml" \
     --include="*.tsx" --include="*.ts" .   # → no matches
grep -i "MIT" LICENSE   # → no match (now ELv2)
cargo test                                  # still green
```

External signals (all must be live before declaring V5 launched):
- velox.dev landing page live
- docs.velox.dev / `/docs` content complete
- Discord server live with launch announcement
- status.velox.dev live with `Velox Self-Hosted Build Pipeline` component
- ELv2 license switch committed
- 5 sample apps run end-to-end
- HN launch post drafted

---

## 13. What V5 Explicitly Does NOT Include

| Item | Why deferred |
|---|---|
| Managed cloud (control plane, signup, billing) | V6 scope — too much surface for parallel work; opens with waitlist at V5-9 launch |
| Async batch API (`/v1/batches`) | Architectural; needs job queue. VELOX_FUTURE §3 |
| Fine-tuning proxy | Async + large files, thin demand signal |
| Prompt compression as default behavior | Violates transparent-proxy contract; opt-in only via plugin |
| LLM-as-judge / evaluation harness | Different product category |
| Native trace viewer in dashboard | Jaeger / Grafana Tempo already excellent — OTel from V3-2 |
| Horizontal semantic cache sync | V4-9 Qdrant already covers it |
| Mobile apps | No demand signal |
| Visual policy editor with NL→DSL conversion | V6 candidate after policy engine matures |
| Customer-facing usage portal | Managed cloud (V6) feature |
| AWS Marketplace AMI | V6 — deploy options shipped in V5-2 are sufficient |

---

## 14. Migration Plan

| Migration | Phase | Description |
|---|---|---|
| 0001–0024 | V1/V2/V3/V4 | Existing schema |
| 0025–0026 | (pricing fixes, current) | Already on disk |
| 0027 | V5-0 | `requests.tool_calls`, `requests.endpoint`; `model_pricing` modality columns |
| 0028 | V5-3 | `identity_providers`, `identities`, `scim_tokens` |
| 0029 | V5-4 | `requests.audit_hash`, `previous_hash`, `chain_sequence`; `audit_events` |
| 0030 | V5-4 | `workspaces.region`, `allowed_regions`; `providers.region` |
| 0031 | V5-5 | `requests.tags`; `daily_costs.forecast_*` |
| 0032 | V5-6 | `notification_channels`; `alerts.channel_ids` |
| 0033 | V5-7 | `policies`, `policy_events`; `workspaces.allowed_providers` |

SQLite migrations: maintained in `migrations/sqlite/` per V2-1 convention. Every PostgreSQL migration must have a matching SQLite migration before phase completion.

> **Rule (CLAUDE.md): never modify existing migrations. Each change is a new file.**

---

## 15. Dependency Plan

### V5-0
```toml
# No new heavy deps; existing reqwest + multer (for multipart) cover audio uploads
multer = "3"                      # if not already present transitively
```

### V5-1
```toml
utoipa            = { version = "5", features = ["axum_extras", "uuid", "chrono", "decimal"] }
utoipa-swagger-ui = { version = "8", features = ["axum"] }
clap              = { version = "4", features = ["derive", "env"] }   # confirm not already pinned
```

### V5-2
```toml
flate2 = "1"      # backup tar.gz
tar    = "0.4"    # backup archive
```

### V5-3
```toml
openidconnect = "3"
samael        = "0.0.18"     # SAML — confirm latest at implementation time
```

### V5-4
```toml
hashicorp_vault   = "2"                    # or custom thin client
aws-sdk-secretsmanager = "1"
google-cloud-secretmanager = "0.5"
```

### V5-5
```toml
printpdf = "0.7"     # invoice PDF
```

### V5-6
```toml
lettre = { version = "0.11", features = ["smtp-transport", "tokio1-native-tls"] }
# Slack, Teams, Discord, PagerDuty use existing reqwest — no new deps
# Datadog uses cadence (dogstatsd) — light
cadence = "1"
```

### V5-7
```toml
# No new heavy deps — policy engine uses serde_json + regex (both present)
```

### V5-8
```toml
[dev-dependencies]
toxiproxy_rust = "0.2"   # chaos testing
```

### V5-9
```toml
# No Rust deps — content + Next.js site only
```

---

## 16. V5 Phase Status Tracker

| Phase | Type | Description | Status | Migration |
|---|---|---|---|---|
| V5-0 | Backend | API Surface Expansion (embeddings, images, audio, models, tools) | ✅ Complete (2026-05-25) | 0027 |
| V5-1 | Backend + SDKs | SDKs, CLI, OpenAPI | ✅ Server side complete (2026-05-25); SDKs live in separate repos (V5-1b) | — |
| V5-2 | Backend + Infra | Deployment & Migration Tooling | ⏳ Not started | — |
| V5-3a | Backend + Frontend | OIDC + SCIM | ⏳ Not started | 0028 |
| V5-3b | Backend + Frontend | SAML 2.0 | ⏳ Not started | 0028 (shared) |
| V5-4 | Backend | Compliance & Audit Hardening | ⏳ Not started | 0029, 0030 |
| V5-5 | Backend + Frontend | Cost Attribution & FinOps | ⏳ Not started | 0031 |
| V5-6 | Backend + Frontend | Notifications & APM Integrations | ⏳ Not started | 0032 |
| V5-7 | Backend + Frontend | Governance & Safety | ⏳ Not started | 0033 |
| V5-8 | Tests + CI | Performance & Reliability Proofs | ⏳ Not started | — |
| V5-9 | Content + Frontend | Docs, Onboarding, GTM Assets | ⏳ Not started | — |

---

## 17. Locked Decisions Record

Decisions made before V5-0 begins. Update DECISIONS.md with these.

| # | Topic | Decision | Reasoning |
|---|---|---|---|
| L1 | License | Switch MIT → **Elastic License v2** | Self-host stays free; blocks AWS-style hosted resale; same path Elastic/MongoDB/Redis took |
| L2 | Brand | **Independent — Velox by Farzad Alizadeh**, remove all "Anthropic" references | Solo developer product, not Anthropic-owned |
| L3 | Managed cloud | **Deferred to V6** | Solo dev cannot ship self-host V5 + managed cloud in parallel; managed needs ≥20 self-host customers first |
| L4 | SSO order | **OIDC first (V5-3a), SAML second (V5-3b)** | OIDC covers ~80% of buyers per week of work |
| L5 | CLI shape | **Single `velox` binary with subcommands via `clap`** | Standard ergonomics (kubectl/docker/gh); one PATH entry |
| L6 | OpenAPI tool | **`utoipa` + `utoipa-swagger-ui`** | Compile-time generation prevents spec drift |
| L7 | Sample-app stack | **Python primary, Node secondary** | Python is dominant in AI/RAG today; covers more buyers |
| L8 | Docs platform | **Mintlify (or Docusaurus fallback)** | Mintlify gives fastest setup; Docusaurus if self-host is required |
| L9 | Postgres in Helm | **External Postgres only** | Production should never run Postgres in the same Pod as the app |

---

## 18. Session Start Ritual for V5 Work

```bash
# 1. Confirm V4 (and all earlier) is still green
cargo test 2>&1 | tail -20

# 2. Check V5 phase status (this file, §16)

# 3. Run the specific phase tests if work is in progress
cargo test v5_0   # (or v5_1, v5_3, etc.)

# 4. Confirm decisions in §17 are reflected in the codebase
grep -ri "anthropic" --include="*.md" --include="*.rs" .   # should taper to 0 by V5-9

# 5. Tell the user: "We are on Phase V5-X. Sub-phase: Y. Ready to continue."
```

**Do NOT write any code until you have done all 5 steps.**

---

## Glossary

| Term | Meaning in V5 |
|---|---|
| Adoption blocker | A gap that causes a real prospect to say "we cannot adopt Velox until this is fixed" |
| External signal | A verifiable artifact (download, working demo, real Slack message) that proves a phase removed its blocker |
| Self-serve | A developer evaluating Velox alone, without sales contact |
| Enterprise deal | An organization where IT, security, finance, and procurement each review separately |
| ELv2 | Elastic License v2 — source-available, free for end-users and self-hosters, restricted for hosted-resale |
| DSAR | Data Subject Access Request (GDPR Article 15) |
| RTBF | Right To Be Forgotten (GDPR Article 17) |

---

*Created: 2026-05-24 — based on V4 complete (all 10 phases) and market-readiness gap analysis*
*Locked decisions: §17 — do not change without explicit conversation + DECISIONS.md update*
*Update the Phase Status Tracker (§16) at the end of every session.*
