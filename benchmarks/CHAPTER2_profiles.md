# Chapter 2 — Workload Profiles

> A "profile" is a frozen description of *what* you are sending. Without one, "Janus does X RPS" is meaningless.

---

## 2.1 Why a profile is a file, not a flag

Profiles are checked-in JSON files (`benchmarks/profiles/*.json`). They are immutable. Every benchmark run names the profile it used. That way, two months from now, when you re-run `chat-short`, you are sending the exact same body Janus saw the first time.

If profiles were flags or scripts, they could drift over time and you'd lose the ability to compare runs. Files in git are forever.

---

## 2.2 The four profiles we ship

| File | Stream | Cache-friendly | Out tokens | Approximates |
|---|:---:|:---:|---:|---|
| [`chat-short.json`](profiles/chat-short.json) | yes | no | ~150 | typical chatbot turn |
| [`chat-long.json`](profiles/chat-long.json) | yes | no | ~500–800 | RAG-style long answer |
| [`cache-warm.json`](profiles/cache-warm.json) | no | yes (identical bytes) | ~100 | cache-hit ceiling |
| [`tools.json`](profiles/tools.json) | no | no | ~80 | function-calling overhead |

Each profile answers a different question:

- **chat-short** — what does Janus look like for the boring 80 % of real traffic?
- **chat-long** — does Janus stay healthy as response size grows?
- **cache-warm** — when the cache is doing its job, how fast is the path?
- **tools** — does function-calling (extra schema in request, structured response) add measurable overhead?

You should run all four whenever you publish numbers. Reporting only `cache-warm` and claiming "Janus is fast" would be misleading — it's the easiest profile.

---

## 2.3 Anatomy of a profile file

Each profile is a single OpenAI `/v1/chat/completions` request body. Janus accepts it unchanged. The mock receives it unchanged. You can `curl` any of these against either system to verify.

Required fields (Janus's openapi.rs and the OpenAI spec):

- `model` — must match a row in the seeded `model_pricing` table (so cost calc works)
- `messages` — at minimum one user message
- `stream` — `true` to exercise SSE path, `false` for one-shot JSON

Optional but recommended:

- `max_tokens` — caps the upstream response. With the mock, this is decorative because the mock returns the number you set on its CLI. But Janus reads it for accounting.
- `temperature` — purely informational for the mock; affects cache key on Janus.
- `tools` — for function-calling profiles.

What you must NOT put in a profile:

- ❌ Real API keys.
- ❌ User PII or customer data.
- ❌ Random nonces or timestamps (would break exact-cache profiling).

---

## 2.4 Why `cache-warm` looks weird

It contains an absurd-looking sentence:

```json
"Janus benchmark canonical cache-warm prompt v1. This prompt is designed to be
byte-identical across every request in the cache-warm profile, so the exact-match
cache layer should hit on every request after the first."
```

That sentence is **on purpose verbose and unique**. It does two things:

1. **Byte-identical content** — every request through this profile produces the same SHA-256 hash, so Janus's exact-cache hits on the second request and onward.
2. **Distinctive marker** — if you ever see this string in a server log, you'll know exactly where it came from.

We also pin `"temperature": 0` and `"stream": false` to keep the cache key stable. (Streaming is cached too, but the non-streaming path has fewer moving parts to measure.)

---

## 2.5 Why `chat-long` includes that long paragraph

The paragraph in `chat-long.json` is about 1 600 tokens of plain text. That's deliberate: real RAG applications stuff retrieved context into the system or user message, producing inputs in the 2k–8k token range. We want a profile that exercises Janus's request serialisation, audit logging, and PII scrubber against a body of realistic size — not just a 5-token "hi there".

Output is also longer (`max_tokens: 800`) so that streaming runs spend a meaningful amount of time *actually streaming*, instead of just measuring TTFT over and over.

---

## 2.6 Why `tools.json` has TWO tools

A request with one tool is trivial — the model has nothing to choose between. A request with two means the response *has* to contain a tool-routing decision. Janus has dedicated code for extracting `tool_calls` from the response and writing them to the `requests.tool_calls` JSONB column (see [V5-0 work in CLAUDE.md](../CLAUDE.md)). The tool profile is the one that exercises that code path.

This is the kind of thing you wouldn't notice if you only had one tool, then six months from now the tool-extraction code regressed silently. The `tools` profile catches it.

---

## 2.7 Writing your own profile

Three rules:

1. **Name it something that describes the traffic shape**, not the test that motivated it. Good: `chat-japanese-long.json`. Bad: `regression-issue-1247.json`.
2. **Pin everything.** No randomness in the body. If you want to vary inputs, write a profile *generator* that emits a JSON file you commit.
3. **Keep it under 32 KiB.** Larger bodies stress your load tool more than Janus.

A good template:

```json
{
  "model": "gpt-4o-mini",
  "stream": false,
  "messages": [
    { "role": "system", "content": "<consistent persona>" },
    { "role": "user", "content": "<the test prompt — fixed text>" }
  ],
  "max_tokens": 150,
  "temperature": 0
}
```

Then add it to `run.sh`'s allowed-profile list. The script will refuse to run profiles it doesn't know about — to prevent fat-fingered "ran a profile that doesn't exist" results.

---

## 2.8 What the mock does with a profile

The mock **ignores the profile content**. It:

- Sleeps `--ttft-ms`.
- Returns a hard-coded `--output-tokens` count of `"tok tok tok ..."` strings.
- Reports `--prompt-tokens-reported` regardless of the actual prompt length.

This sounds wrong but it's actually critical: if the mock tokenized the real input, the mock's CPU usage would creep into the measurement. By making it dumb, we ensure **the time the mock takes is exactly what we configured, no matter what we send it**.

That means: a 5 KB chat-long body and a 200 B chat-short body produce the *same response timing from the mock*. Any difference in measured TTFT between the two profiles is **entirely Janus's overhead**. That's the whole game.

---

## 2.9 Verifying a profile against the mock

```bash
# With the mock running on :9999
curl -s -X POST http://localhost:9999/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d @benchmarks/profiles/chat-short.json | jq '.usage'
```

Expected output:

```json
{ "prompt_tokens": 200, "completion_tokens": 50, "total_tokens": 250 }
```

If you see the response, the profile is well-formed JSON and the mock accepted it. (The `usage` block reflects the mock's static numbers, not the actual content of your profile.)

---

*Next: [CHAPTER3_seed.md](CHAPTER3_seed.md). It explains the database preparation step.*
