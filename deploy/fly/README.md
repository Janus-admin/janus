# Janus on Fly.io

Single-region launch. For HA, scale to 2+ machines and use a Fly Postgres HA
cluster.

## Prerequisites
- `flyctl` installed and authenticated
- `openssl` (for generating secrets)

## First-time deploy

```bash
# 1. Create the app shell and copy this config.
fly launch --copy-config --no-deploy --name janus

# 2. Provision Postgres (HA two-node by default).
fly postgres create --name janus-pg --region iad --vm-size shared-cpu-1x \
  --initial-cluster-size 2

# 3. Attach Postgres to the app (sets DATABASE_URL secret).
fly postgres attach --app janus janus-pg

# 4. Set the rest of the secrets.
fly secrets set --app janus \
  JWT_SECRET=$(openssl rand -hex 32) \
  ENCRYPTION_KEY=$(openssl rand -hex 32) \
  OPENAI_API_KEY=sk-...

# 5. Create the models volume (one per region you deploy to).
fly volumes create janus_models --region iad --size 5 --app janus

# 6. Deploy.
fly deploy --app janus
```

## Scaling

```bash
# Add a second machine in the same region for HA.
fly scale count 2 --region iad --app janus

# Multi-region (read-only failover; Postgres needs its own multi-region story).
fly scale count 2 --region iad --app janus
fly scale count 1 --region fra --app janus
fly volumes create janus_models --region fra --size 5 --app janus
```
