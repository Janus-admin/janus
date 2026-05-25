# Janus on Render

## Prerequisites
- Render account
- A fork of this repo on GitHub

## Deploy
1. Render dashboard → **New → Blueprint** → connect this repo.
2. Render reads `deploy/render/render.yaml` and provisions:
   - A web service running the Janus Docker image
   - A managed Postgres instance (`janus-pg`)
3. On first deploy, Render prompts for the keys marked `sync: false`
   (provider API keys + AWS creds). Supply at least one provider.
4. The first request to `/health` confirms the stack is up.

## After deploy
- Generate an admin user via the dashboard or `janus keys create` against
  `https://<your-service>.onrender.com`.
- Bump `plan: starter` → `plan: standard` (or higher) once you have real traffic.
