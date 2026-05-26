---
name: Bug report
about: Something is broken or behaving unexpectedly
title: "[bug] "
labels: bug
assignees: ''
---

## What happened?

<!-- A clear description of the bug. -->

## Expected behavior

<!-- What you expected to happen. -->

## Steps to reproduce

1. 
2. 
3. 

## Environment

| Field | Value |
|---|---|
| Janus version | <!-- `janus --version` or image tag --> |
| Deployment | <!-- Docker / Helm / cargo run / Railway / Fly.io --> |
| Database | <!-- Postgres version --> |
| OS | |

## Relevant logs

```
# docker compose logs janus  OR  RUST_LOG=debug cargo run
```

## Config (redact secrets)

```toml
# paste relevant janus.toml / environment variables here — remove real keys
```
