# Chapter 3 — Seeding the database

> Janus is a multi-tenant system with auth, RBAC, and provider configuration. Before you can `curl` it under load, the database needs the right rows. `seed.sh` puts them there.

---

## 3.1 What `seed.sh` does — in five sentences

1. Checks that Janus is running and the mock is running. Fails fast if not.
2. Creates a benchmark admin user (or notices one already exists).
3. Logs in to that user and grabs a JWT.
4. PATCHes the `openai` provider so its `base_url` points at the mock.
5. Creates an API key with no budget cap and no rate limit, then prints it.

---

## 3.2 Why each step is necessary

| Step | Why |
|---|---|
| 1. Preflight | If Janus isn't up, nothing further can succeed. The script must fail with a clear message *now*, not with a 500-line stack trace 20 seconds in. |
| 2. Register admin | Janus has no built-in admin user. The first user that registers becomes admin via the RBAC bootstrap rule (V4-8). We need this to call admin endpoints. |
| 3. Login → JWT | All admin endpoints require a JWT. JWTs are short-lived; we get a fresh one per seed run. |
| 4. PATCH provider base_url | Janus's openai adapter calls whatever URL is stored in the `providers` table. We need to redirect it from `api.openai.com` to `localhost:9999`. |
| 5. Create API key | Janus's gateway API (`POST /v1/chat/completions`) is what the load tool will hit. It requires a valid `jn-sk-...` key. We make one with no caps so rate limits don't artificially throttle the benchmark. |

---

## 3.3 Why restart Janus after seeding

This is the most surprising thing about Janus and the only counter-intuitive step in the harness.

Look at [src/handlers/admin/providers.rs:38-42](../src/handlers/admin/providers.rs):

> "Persists to DB immediately. Changes to API keys take effect on next restart.
> Changes to is_enabled / priority affect routing immediately via the DB record
> (the running ProviderRegistry is seeded at startup, restart required for those too)."

In plain English: when Janus boots, it reads all `providers` rows into an in-memory `ProviderRegistry`. After boot it never re-reads them. PATCHing the row in the DB changes what *next* boot will see, but the running process is unchanged.

So:

1. Start Janus once. Its provider list points at real OpenAI.
2. Run `seed.sh`. The DB row now points at the mock.
3. **Restart Janus.** It reads the DB again, this time seeing the mock URL.
4. Now Janus actually proxies to the mock.

If you skip step 3, your benchmark calls real OpenAI. You will hit rate limits and burn money. Don't.

---

## 3.4 Why API keys *don't* need a restart

The note above also says: API keys take effect *without* restart. That's because of [src/handlers/admin/keys.rs:100-102](../src/handlers/admin/keys.rs):

```rust
// Immediately insert into the dashmap so subsequent requests work without restart
state.key_cache.insert(key_bytes, key.clone());
```

The key cache is updated live. So step 5 of seed (creating the key) takes effect immediately. Only step 4 (the provider PATCH) needs a restart.

This asymmetry is annoying. It's also reality. Documenting it here so you don't waste an hour debugging.

---

## 3.5 Idempotency

You can run `seed.sh` more than once. On the second run:

- Register returns HTTP 409 (user exists). The script handles this gracefully.
- Login succeeds with the same credentials.
- PATCH overwrites the existing base_url (no-op if you didn't change anything).
- Key creation creates a **new key**. Previous keys still work — Janus doesn't revoke them. If you want to clean those up, do it via the dashboard or the `janus keys revoke` CLI.

This means: re-running `seed.sh` is the right answer to "I lost my API key". Don't try to dig it out of the database — bcrypt hashes are not reversible.

---

## 3.6 What goes wrong, and why

### Janus isn't reachable

```
[seed] ERROR: Janus is not reachable at http://localhost:8080.
```

Check:
- Is `cargo run --release -- serve` actually running in another terminal?
- Did you set `OPENAI_API_KEY` before starting it? Without that, the openai provider isn't registered at all and PATCHing it returns 404.
- Is the port really 8080? If you set `JANUS_PORT=...`, also `export JANUS_ADMIN_URL=http://localhost:<port>` before running seed.

### Mock isn't reachable

```
[seed] ERROR: mock-llm is not reachable at http://localhost:9999.
```

Start it: `./benchmarks/mock-llm/target/release/mock-llm --port 9999`. (You built it once with `cargo build --release --manifest-path benchmarks/mock-llm/Cargo.toml`.)

### `PATCH /admin/providers/openai` returns 404

The openai provider isn't seeded. This happens when Janus starts without `OPENAI_API_KEY` set. Janus skips registering a provider whose API key is empty. Set the env var to literally anything (the mock doesn't validate it):

```bash
OPENAI_API_KEY=mock-key cargo run --release -- serve
```

Then re-run `seed.sh`.

### `PATCH /admin/providers/openai` returns 403

The user that logged in doesn't have the Admin role. This only happens if you've run seed against a database that already had other users, and your benchmark user wasn't the first one registered.

Two fixes:
- Drop the DB and re-migrate (`docker compose down -v && docker compose up -d postgres && cargo run -- migrate up`), then re-seed.
- Or: log in as your existing admin and grant the benchmark user Admin via the workspaces page.

### "key creation failed"

If the response is `{"error":{"code":"FORBIDDEN", ...}}`, see the 403 case above.

If the response says something about `workspace_id` or `budget_limit`, the model fields shipped in the request don't match the current schema. Check `src/models/api_key.rs` — the script may need an update for a newer Janus.

---

## 3.7 The `.janus_api_key` file

After a successful run, `benchmarks/.janus_api_key` contains the latest key, file-mode `600`. `run.sh` reads it as a fallback if `$JANUS_API_KEY` isn't set. The file is in `.gitignore`d via the inherited rules (it's in `benchmarks/` but not committed — verify with `git status`).

You can also export it manually after seeding:

```bash
export JANUS_API_KEY=$(./benchmarks/seed.sh | tail -1)
```

The script writes its progress to stderr and the key alone to stdout, so `$(...)` capture works cleanly.

---

## 3.8 The lifecycle

```
   first time                  every benchmark
   ───────────                  ───────────────
   docker compose up -d ─┐
   migrate up            │
   start mock-llm        │      ┌── start mock-llm (if not running)
   start Janus           │      │
   seed.sh ──────────────┤      ├── ./run.sh chat-short 60s 50
   restart Janus ────────┘      │
                                └── inspect benchmarks/results/<ts>/
```

The left column happens once per machine. The right column repeats forever.

---

*Next: [CHAPTER4_running.md](CHAPTER4_running.md). It explains the actual measurement orchestration.*
