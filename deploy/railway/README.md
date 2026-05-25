# Velox on Railway

One-click deploy. Railway provisions an external Postgres add-on and points
`DATABASE_URL` at it automatically.

## Prerequisites
- Railway account (free tier works for evaluation)
- A `JWT_SECRET` and `ENCRYPTION_KEY` (any 32-byte hex string). Generate with:

  ```bash
  openssl rand -hex 32  # JWT_SECRET
  openssl rand -hex 32  # ENCRYPTION_KEY
  ```

## Deploy
1. Create a new Railway project from this repo: `New Project → Deploy from GitHub repo`.
2. Add the Postgres plugin: `+ New → Database → PostgreSQL`.
3. Add the shared variables `JWT_SECRET` and `ENCRYPTION_KEY` under the project's
   *Variables* tab.
4. Add the provider keys you plan to use:
   `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GROQ_API_KEY`, ...
5. Railway uses `railway.json` from this directory — no further config needed.

## Notes
- Replicas: Railway runs a single instance by default. For >1 instance, enable
  cluster mode by setting `CLUSTER_ENABLED=true` and shar(e the DB).
- Semantic cache: defaults to `linear` (in-process). For Qdrant, set
  `SEMANTIC_CACHE_BACKEND=qdrant` and `QDRANT_URL` to a managed Qdrant instance.
