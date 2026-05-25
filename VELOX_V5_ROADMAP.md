# VELOX V5 — Launch Readiness Roadmap (Revised 2026-05-25)
> Built on V4 (all 10 phases complete, 2026-05-24).
> **If you are Claude: read CLAUDE.md first, then VELOX_V4_ROADMAP.md §16, then this file.**

---

## V5 Philosophy

V1–V4 built a technically complete product. V5 makes it a **shippable product for the first 10 customers.**

**The right question for V5 is:**
> *What would stop a technical buyer at a 20–100 person company from adopting Velox today?*

Not: *What does Okta enterprise procurement require?*
Not: *What benchmarks does a public HN launch need?*

Enterprise features (SAML, SCIM, SOC2 audit, Vault integration, policy engine, APM exporters)
are real requirements — but they belong to customers 20–50, not customers 1–10.
Building them before you have paying customers is:
1. Expensive — weeks of engineering time on unvalidated requirements
2. Risky — the first customers will tell you what they actually need
3. Premature — no one does enterprise procurement with a company that has 0 customers

**Rules for V5 Launch phases:**
1. Every phase must answer "which first-10-customer blocker does this remove?"
2. If a feature is first needed by a $50k+ ACV enterprise deal, it goes to Future Plans.
3. Ship independently. No phase blocks the next.

---

## What Is Already Complete

The following were completed before V5 began or in V5-0 through V5-2.
Do not rebuild these.

### V5-0 ✅ API Surface Expansion (2026-05-25)
- `POST /v1/embeddings`, `GET /v1/models`, `POST /v1/images/generations`
- `POST /v1/audio/transcriptions`, `POST /v1/audio/speech`
- Tool calls extracted and stored in `requests.tool_calls` (JSONB)
- Per-modality cost tracking (image, audio, character pricing)

### V5-1 ✅ OpenAPI + Swagger UI + `velox` CLI (2026-05-25)
- OpenAPI 3.1 spec at `GET /admin/openapi.json`
- Swagger UI at `GET /admin/docs`
- `velox` binary with full subcommand structure: `serve`, `keys`, `migrate`, `config`, `import`, `backup`, `doctor`, `demo`

### V5-2 ✅ Deployment & Migration Tooling (2026-05-25)
- Helm chart at `charts/velox/` (HPA, ServiceMonitor, Ingress, external Postgres)
- One-click deploy configs: Railway, Fly.io, Render
- Migration importers: `velox import litellm`, `velox import portkey`, `velox import openrouter`
- `velox backup` / `velox restore` with version-safe tar.gz archives
- HA deployment guide at `docs/deployment/ha.md`
- Terraform provider is a separate repo (`terraform-provider-velox`) — not in scope here

---

## V5 Launch Phases (What Remains)

These four phases are the only remaining work before reaching out to the first 10 customers.
Total estimated time: **3–4 weeks.**

---

## Phase V5-L1: Brand & First Impression

**Blocker removed:** "Is this an Anthropic product? Can I trust this solo founder's product?"

Brand confusion is a credibility killer. The README previously contained placeholder image paths
and GitHub org references that made it look like an Anthropic project. A first-time visitor
immediately wonders whether this is an official product, an unofficial fork, or something else.
This must be resolved before any customer conversation.

### What to do

1. **README rewrite** — Replace placeholder Docker image paths with `ghcr.io/alizadehafpn/velox`.
   Rewrite the intro to be a clear product pitch. Remove any leftover references to Anthropic
   as a company or employer.

2. **Code and doc sweep** — `grep -ri "anthropic" --include="*.md" --include="*.rs" --include="*.toml" .`
   should return only:
   - Config keys like `anthropic_api_key = "..."` — these are fine (they mean the LLM provider)
   - The Anthropic provider adapter (`src/providers/anthropic.rs`) — keep as-is, it's a feature

3. **Cargo.toml metadata** — Set `authors`, `repository`, `homepage`, `description` to your own
   identity and repo URL. These show up in `cargo info` and in the Helm chart.

4. **License decision** — The roadmap originally planned switching MIT → ELv2 to protect against
   hosted-resale. For the first 10 customers (all self-hosting), MIT is cleaner.
   **Recommendation: stay on MIT now.** Switch to ELv2 before a public launch or V5-L5 (docs).
   Reason: ELv2 creates legal review friction. No 20-person company will adopt a product with a
   non-OSI license without legal sign-off they don't have bandwidth for.

5. **Docker image** — Confirm you have or can build a real public Docker image at the new path.
   The README should show a working `docker pull` command, not a placeholder.

### Files to modify
- `README.md`
- `Cargo.toml` (`[package]` metadata)
- `docs/quickstart.md`, `docs/configuration.md`
- Any `docs/deployment/*.md` that references the old image

### Definition of done
```bash
# No old-org image or repo paths remain
grep -rn "ghcr\.io/anthropi\|github\.com/anthropi" \
  --include="*.md" --include="*.toml" --include="*.rs" .
# → zero matches
```

---

## Phase V5-L2: OIDC Login

**Blocker removed:** "Can we use our Google / GitHub / company SSO instead of creating new passwords?"

About half of B2B demos for developer tools end with this question.
OIDC covers Google Workspace, GitHub, GitLab, Okta, and any standards-compliant IdP.
It typically unlocks the deal for companies with 20+ employees.

SAML and SCIM are NOT in this phase. SAML is required by legacy enterprise IT departments
(customers 15–30). SCIM is required by IT-managed provisioning (customers 30+). Neither is
asked by the first 10 customers. Both move to Future Plans (§ below).

### Schema

```sql
-- migrations/0028_oidc.sql
CREATE TABLE identity_providers (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind            VARCHAR(20) NOT NULL CHECK (kind = 'oidc'),
    name            VARCHAR(100) NOT NULL,
    config          JSONB NOT NULL,   -- discovery_url, client_id, client_secret (encrypted)
    group_role_map  JSONB NOT NULL DEFAULT '{}',
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
```

### Routes
- `GET  /auth/oidc/:idp_id/start` — redirect to IdP authorization endpoint with PKCE
- `GET  /auth/oidc/:idp_id/callback` — verify ID token, JIT-create user if new, mint Velox JWT
- `GET  /admin/idp` — list configured identity providers
- `POST /admin/idp` — configure a new OIDC IdP
- `DELETE /admin/idp/:id` — remove an IdP

### Dashboard: `/settings/sso`
- Form: name, discovery URL, client ID, client secret
- "Test connection" button — verifies the discovery URL resolves and returns valid metadata
- Table of configured IdPs with enable/disable toggle
- Group claim → Velox role mapping (optional; default: authenticated = ReadOnly)

### Key behaviors
- JIT user creation: first OIDC login creates a `users` row automatically
- Group mapping: if the IdP returns a `groups` claim, map it to a Velox role via `group_role_map`
- Existing password accounts: unaffected. OIDC is additive.
- `openidconnect` crate (Rust) handles the token validation and discovery

### Files to create
- `src/auth/oidc.rs`
- `src/handlers/auth/sso.rs`
- `src/db/identities.rs`
- `migrations/0028_oidc.sql` (+ sqlite mirror)
- `dashboard/src/app/(dashboard)/settings/sso/page.tsx`

### Files to modify
- `src/routes/mod.rs` — add OIDC routes
- `Cargo.toml` — add `openidconnect = "3"`

### Test contract

```rust
// tests/v5_l2_oidc.rs
async fn v5_l2_oidc_callback_creates_user_jit()
async fn v5_l2_oidc_second_login_reuses_existing_user()
async fn v5_l2_oidc_group_claim_maps_to_role()
async fn v5_l2_oidc_invalid_id_token_rejected()
async fn v5_l2_oidc_csrf_state_param_enforced()
async fn v5_l2_oidc_disabled_idp_returns_404()
async fn v5_l2_idp_crud_endpoints_require_admin()
```

### Definition of done
```bash
cargo test v5_l2
cargo test
cargo clippy -- -D warnings
# Manual: test login against a Google OAuth app or Auth0 free tenant
```

---

## Phase V5-L3: Cost Tags

**Blocker removed:** "How much did the backend team spend vs the ML team last month?"

Every team evaluating an AI gateway asks this in the first demo.
The existing dashboard shows costs per API key and per model, but not by business dimension
(team, project, environment). That gap makes the cost dashboard feel incomplete to a buyer.

This phase adds a tag system. It does NOT add forecasting, PDF invoicing, or chargeback.
Those belong in Future Plans — they are finance team features (customers 15+), not dev team features.

### Schema

```sql
-- migrations/0029_request_tags.sql
ALTER TABLE requests ADD COLUMN tags JSONB DEFAULT '{}';
CREATE INDEX idx_requests_tags_gin ON requests USING gin(tags);
```

### How tags are sent (client-side, zero SDK changes)
```bash
# Option A: OpenAI metadata field (already in OpenAI spec)
curl -H "Authorization: Bearer vx-sk-..." \
     -d '{"model":"gpt-4o","messages":[...],"metadata":{"team":"backend","project":"rag"}}' \
     http://velox/v1/chat/completions

# Option B: Velox header
curl -H "Authorization: Bearer vx-sk-..." \
     -H "X-Velox-Tags: team=backend,project=rag" \
     http://velox/v1/chat/completions
```

### New analytics endpoint
```
GET /admin/analytics/cost?group_by=tag.team&period=30d
GET /admin/analytics/cost?group_by=tag.project&period=7d
GET /admin/analytics/cost?group_by=tag.team,model&period=30d
```

Response shape:
```json
{
  "data": {
    "period": "30d",
    "total_cost_usd": 1420.50,
    "groups": [
      {"key": {"tag.team": "backend"}, "cost_usd": 820.40, "request_count": 12400},
      {"key": {"tag.team": "ml"},      "cost_usd": 600.10, "request_count": 8200}
    ]
  }
}
```

### Dashboard addition
Add a "Cost by Tag" card to the existing `/analytics` page.
Dimension selector (dropdown: team, project, env, or any custom tag key).
Bar chart. Date range filter.
No new page needed — extend the existing analytics page.

### Files to modify
- `src/gateway/pipeline.rs` — extract tags from `metadata` body field and `X-Velox-Tags` header
- `src/db/requests.rs` — include `tags` in `insert_request`
- `src/handlers/admin/analytics.rs` — add cost breakdown by tag query
- `dashboard/src/app/(dashboard)/analytics/page.tsx` — add cost-by-tag card

### Test contract
```rust
// tests/v5_l3_cost_tags.rs
async fn v5_l3_metadata_field_tags_stored_in_requests()
async fn v5_l3_header_tags_stored_in_requests()
async fn v5_l3_header_overrides_body_when_both_present()
async fn v5_l3_cost_breakdown_sums_match_raw_total()
async fn v5_l3_multi_dimension_breakdown_groups_correctly()
async fn v5_l3_missing_tag_key_returns_null_group()
```

### Definition of done
```bash
cargo test v5_l3
cargo test
cargo clippy -- -D warnings
# Manual: tag a batch of requests, verify the cost breakdown matches sum(cost_usd)
```

---

## Phase V5-L4: Polished Alerts (Slack + Email)

**Blocker removed:** "We need to know when a team hits its budget limit, not check the dashboard."

You already have webhook-based alerts from V2-2. This phase promotes two channels to first-class:
Slack and email. These are the two channels every early customer already uses.

PagerDuty, Microsoft Teams, Discord, Datadog — these are Future Plans.
They add integration surface without adding customers in the 1–10 range.

### What to add

**Slack** — native Slack block format via incoming webhook URL:
```json
{
  "blocks": [
    {"type":"header","text":{"type":"plain_text","text":"⚠️ Velox Alert: Budget Limit Reached"}},
    {"type":"section","fields":[
      {"type":"mrkdwn","text":"*Key:* Production API"},
      {"type":"mrkdwn","text":"*Spend:* $95.20 / $100.00"},
      {"type":"mrkdwn","text":"*Workspace:* Acme Corp"},
      {"type":"mrkdwn","text":"*Time:* 2026-06-01 14:32 UTC"}
    ]}
  ]
}
```

**Email** — SMTP via `lettre` crate. Plain text + HTML fallback. One SMTP config per Velox instance
(set in `velox.toml`). Alert includes: alert name, condition triggered, current value, link to dashboard.

### Schema addition
```sql
-- migrations/0030_alert_channels.sql
ALTER TABLE alerts
    ADD COLUMN slack_webhook_url TEXT,
    ADD COLUMN email_to          TEXT[];
-- existing webhook_url stays for generic HTTP webhooks
```

### Dashboard
- Alert create/edit form: add "Slack webhook URL" field + "Email recipients" field
- "Test" button sends a sample payload to verify the channel works before saving

### Files to modify
- `src/alerts/dispatch.rs` (or wherever V2-2 sends alerts) — add Slack + email dispatch paths
- `migrations/0030_alert_channels.sql`
- `dashboard/src/app/(dashboard)/alerts/` — update form with Slack/email fields
- `Cargo.toml` — add `lettre = { version = "0.11", features = ["smtp-transport", "tokio1-native-tls"] }`

### Test contract
```rust
// tests/v5_l4_alerts.rs
async fn v5_l4_slack_webhook_sends_block_payload()
async fn v5_l4_email_sends_via_smtp()
async fn v5_l4_alert_dispatches_to_both_channels_when_configured()
async fn v5_l4_missing_channel_config_falls_back_gracefully()
async fn v5_l4_test_endpoint_sends_sample_payload()
```

### Definition of done
```bash
cargo test v5_l4
cargo test
cargo clippy -- -D warnings
# Manual: create an alert with a Slack webhook URL, trigger it, confirm message appears in Slack
```

---

## Phase V5-L5: Onboarding & Docs Polish

**Blocker removed:** "I couldn't figure out how to get started. The README was confusing."

This phase completes the first-impression loop. A developer who finds Velox should be able to
go from "what is this?" to "first successful request" in under 10 minutes without asking anyone.

### What to do

1. **In-dashboard onboarding tour** (4 steps, dismissable, shown on first login)
   - Step 1: Create your first API key
   - Step 2: Send a test request from the Playground
   - Step 3: View the request in the Requests list
   - Step 4: Set your first budget alert
   - State stored in `users.tour_completed_at TIMESTAMPTZ` (add in migration 0031)

2. **Docs review pass** — Read `docs/quickstart.md` and `docs/configuration.md` as if you've
   never seen the project. Fix anything that's stale, unclear, or references old behavior.
   Focus on: getting started, first API key, first request, reading the dashboard.

3. **README final state** — After V5-L1 + this phase, the README should be:
   - 5-minute quickstart that works
   - Links to the dashboard, OpenAPI docs, Helm chart, and migration guide
   - No broken references, no Anthropic mentions, no placeholder paths

4. **License** — Switch to ELv2 now (deferred from V5-L1 as recommended).
   Replace `LICENSE`, update `Cargo.toml license`, update `README.md` badge.

### Files to modify
- `dashboard/src/components/OnboardingTour.tsx` (create)
- `migrations/0031_user_tour.sql`
- `docs/quickstart.md`, `docs/configuration.md`
- `README.md` (final pass)
- `LICENSE`, `Cargo.toml`

### Definition of done
```bash
cargo test
cargo clippy -- -D warnings
# Manual: follow the README from scratch on a clean machine, confirm it works
grep -rn "ghcr\.io/anthropi\|github\.com/anthropi" --include="*.md" --include="*.toml" .
# → zero matches
```

---

## Launch Checklist (After All Five Phases)

Run this before reaching out to the first customer:

```bash
# 1. Full test suite green
cargo test

# 2. No warnings
cargo clippy -- -D warnings

# 3. Formatted
cargo fmt -- --check

# 4. Release build succeeds
cargo build --release

# 5. No brand confusion (org-slug references removed in V5-L1)
grep -rn "ghcr\.io/anthropi\|github\.com/anthropi" --include="*.md" --include="*.toml" .

# 6. Docker image builds and runs
docker build -t velox:latest .
docker run --rm -e DATABASE_URL=... velox:latest velox doctor

# 7. Helm lint
helm lint charts/velox
```

---

## V5 Phase Status Tracker

| Phase | Description | Status | Notes |
|---|---|---|---|
| V5-0 | API Surface Expansion | ✅ Complete (2026-05-25) | Embeddings, images, audio, models, tool calls |
| V5-1 | OpenAPI + Swagger UI + `velox` CLI | ✅ Complete (2026-05-25) | Server side; SDKs live in separate repos |
| V5-2 | Deployment & Migration Tooling | ✅ Complete (2026-05-25) | Helm, one-click deploy, importers, backup; Terraform = separate repo |
| V5-L1 | Brand & First Impression | ✅ Complete (2026-05-25) | README rewritten, Docker image path fixed, Cargo.toml metadata, openapi.rs |
| V5-L2 | OIDC Login | ⏳ Not started | Migration 0028 |
| V5-L3 | Cost Tags | ⏳ Not started | Migration 0029 |
| V5-L4 | Polished Alerts (Slack + Email) | ⏳ Not started | Migration 0030 |
| V5-L5 | Onboarding & Docs Polish | ⏳ Not started | Migration 0031 |

---

## Future Plans

These items were in the original V5 roadmap. They are real and important — but they are
requirements for customers 10–50, not customers 1–10. Build them when a paying customer
asks for them, or when you have 5+ customers and know the pattern.

### Authentication (after first 10 customers)
- **SAML 2.0** — required by traditional enterprise IT, not by developer-led evaluation
- **SCIM provisioning** — required by IT-managed onboarding workflows (Okta, Azure AD)
- **MFA enforcement** — nice for compliance, not asked in first 10 deals

### Compliance (after SOC2 process begins)
- **Tamper-evident audit chain** — SHA-256 chain on `requests` table (SOC2 CC7.2)
- **GDPR data export** — DSAR / Article 15 workspace export
- **Right to be forgotten** — Article 17 user deletion with audit preservation
- **Secrets manager integration** — Vault, AWS Secrets Manager, GCP SM, K8s Secrets
- **Data residency controls** — per-workspace region enforcement on provider selection
- **Compliance docs** — SOC2 control matrix, GDPR checklist, HIPAA BAA checklist

### FinOps (after finance teams enter the picture)
- **Budget forecasting** — linear regression over daily spend, "hit cap on May 18" alert
- **Invoice generation** — PDF / HTML / CSV per workspace per month
- **Chargeback configuration** — markup %, default tag dimension, secondary splits

### Notifications (after Slack + Email prove insufficient)
- **PagerDuty** — trigger / acknowledge / resolve lifecycle
- **Microsoft Teams** — incoming webhook + AdaptiveCard
- **Discord** — webhook + embed
- **APM exporters** — Datadog (dogstatsd), New Relic (Insights API)
- **Monthly SLA reports** — per-workspace uptime, p95 latency, error rate

### Governance & Safety (after security team enters the deal)
- **Policy engine** — admin-configurable rules: reject, downgrade, route, redact
- **Prompt injection detection** — pattern library + ONNX classifier
- **Content moderation hook** — OpenAI Moderation API or local ONNX model
- **Egress allowlist** — per-workspace allowed provider list
- **Visual policy builder** — dashboard UI for building rules without writing JSON

### Performance & Reliability Proofs (for public launch / bake-offs)
- **k6 load test suite** — 100/500/1000 req/s benchmarks committed to CI
- **Chaos test suite** — toxiproxy-based provider cut, DB disconnect, cache panic
- **Playwright E2E suite** — one spec per dashboard page
- **Public benchmark page** — auto-updated from CI, shared at docs site

### GTM & Community (after first 10 customers give you testimonials)
- **Marketing landing page** — hero, comparison table, install CTA
- **Comparison pages** — `/vs/litellm`, `/vs/portkey`, `/vs/helicone`, `/vs/cloudflare`
- **Sample apps repo** — RAG chatbot, tool-using agent, embeddings search
- **Docs site migration** — Mintlify or Docusaurus at docs.velox.dev
- **Discord server** — community, showcase, feature requests
- **Status page** — status.velox.dev
- **Blog launch sequence** — 5 posts over 4 weeks
- **Python SDK** (`velox-python`, separate repo)
- **Node.js SDK** (`velox-node`, separate repo)

---

## Locked Decisions (carried from original V5)

| # | Decision | Rationale |
|---|---|---|
| L1 | License: MIT now → ELv2 at V5-L5 | MIT removes legal friction for first customers; ELv2 protects before public launch |
| L2 | Brand: Velox by Farzad Alizadeh | Independent product; remove all Anthropic org references |
| L3 | Managed cloud: deferred to V6 | Cannot build self-host + managed cloud in parallel as a solo founder |
| L4 | OIDC first, SAML later | OIDC covers ~80% of buyers; SAML is legacy IT, not developer-led evaluation |
| L5 | CLI: single `velox` binary, clap subcommands | Done in V5-1 |
| L6 | OpenAPI: utoipa + utoipa-swagger-ui | Done in V5-1 |
| L7 | Postgres in Helm: external only | Done in V5-2 |

---

## Session Start Ritual for V5 Work

```bash
# 1. Confirm all prior phases still green
cargo test 2>&1 | tail -20

# 2. Check current phase (V5 Status Tracker above)

# 3. Run phase-specific tests if in progress
cargo test v5_l2   # (or l3, l4, l5)

# 4. Tell the user: "V5 is on Phase V5-LX. Ready to continue."
```

---

*Original V5 created: 2026-05-24 — full market-readiness roadmap (10 phases)*
*Revised: 2026-05-25 — refocused on first 10 customers; 6 phases remain (5 new L-series)*
*Future Plans section contains all deferred enterprise/GTM features*
*Update the Phase Status Tracker at the end of every session.*
