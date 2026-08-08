# Spike — intercepting the wire to the model API

**Date:** 2026-08-08 · **Timebox:** one session · **Status:** finding, nothing shipped
**Harness:** [`scripts/wire-capture.ts`](../../scripts/wire-capture.ts)

The question was whether a local intercepting proxy would show us enough that
the JSONL transcript doesn't, to be worth building. It does, by a wide margin,
and the TLS problem that made it look expensive turns out not to exist.

**Verdict: worth building, in the narrow form described at the end.** The one
number that decides it: on a real first turn, **93.6% of the context window was
tool-definition JSON that appears nowhere on disk**, and Peektrace today renders
that as a single grey bar labelled *"not in transcript"*.

---

## 1. How the agent gets pointed at the proxy

All four claims below were tested by running the real binaries, not read off
docs.

| Mechanism | Result | Sees request bodies? |
| --- | --- | --- |
| `ANTHROPIC_BASE_URL=http://127.0.0.1:PORT` (Claude Code 2.1.220) | **Works.** Session completed normally; every `/v1/messages` call arrived at the proxy. | Yes — plaintext |
| `HTTPS_PROXY=http://127.0.0.1:PORT` (Claude Code) | **Honoured.** `CONNECT api.anthropic.com:443`, plus `mcp-proxy.anthropic.com:443` and a Datadog logs endpoint. | No — host names only, without MITM |
| `model_providers.<id>.base_url` (Codex CLI 0.146.0) | **Works.** A 46 KB `POST /v1/responses` body landed at a local sink. | Yes — plaintext |
| Pi | **Not tested** — no Pi install in the spike environment. | Unknown |

Two details that matter for a product decision:

- **Claude Code forwards its OAuth bearer to whatever base URL you set.** The
  Max-plan session authenticated fine through a plain-HTTP loopback hop. That is
  what makes the capture trivial; it is also why the proxy must never be
  reachable off-box, and why the harness drops credential headers before
  anything is written.
- `HTTPS_PROXY` catches *more* traffic (MCP proxy, telemetry) but tells you
  nothing about any of it. It is the right tool for "what does this thing talk
  to", not for "what is in the context window".

## 2. TLS: the install step we thought we needed, we don't

Interception via `HTTPS_PROXY` would need a local CA and a trusted cert — a real
install step, and a genuinely awkward thing for a tool that sells itself on
local-first inspection.

**The base-URL override sidesteps it completely.** The agent speaks plain HTTP
to `127.0.0.1`; the proxy speaks TLS to the real API on the far side, verifying
the upstream cert normally. No CA is generated, no trust store is touched,
nothing on the machine is weakened after the process exits. The blast radius is
one env var.

The cost of that route is coverage: it only works for agents that expose a
base-URL knob, and only for the endpoint that knob controls. Claude Code and
Codex both do. That is the trade to take — a capture that covers two agents
without touching the trust store beats a capture that covers everything and
requires a CA.

## 3. What the wire buys over the JSONL

Two real Claude Code sessions were captured and diffed against the transcripts
the same runs wrote. Token counts are **exact** — the captured bodies were
re-submitted to `/v1/messages/count_tokens` with and without each component, and
the components differenced.

### Run A — trivial prompt, 28 tools, MCP tools deferred

Ground truth from the transcript's own `usage`: **37,081 context tokens.**

| Component | Exact tokens | Share | In the JSONL? |
| --- | ---: | ---: | --- |
| Tool definitions (28) | 28,807 | 77.7% | **No** — names only, via `deferred_tools_delta` |
| System prompt (3 blocks) | 6,298 | 17.0% | **No** |
| Messages (3 `<system-reminder>` blocks + the prompt) | 1,830 | 4.9% | Partly — as `attachment` lines |
| Per-request framing | 147 | 0.4% | No |

What Peektrace produces from the transcript for the same turn:

```
systemOverheadTokens (inferred floor): 35457
budget: system_tools=35457  listings=1618  prompts=6
```

So **95.6% of the window is one opaque bar.** The wire splits that same 35,457
into 6,298 system + 28,807 tools + ~206 of reminder text the parser under-counts
+ 147 framing = **35,458**. The inferred floor is not wrong — it is exact to one
token — it is just undivided. The wire supplies the division.

The tool block itself decomposes cleanly: a fixed 496-token framing constant
plus the sum of the individual definitions (496 + 28,310 = 28,806 vs 28,807
measured). So per-tool attribution is sound, not a heuristic:

| Tool | Exact tokens |
| --- | ---: |
| `Workflow` | 6,023 |
| `Bash` | 3,033 |
| `DesignSync` | 2,628 |
| `Monitor` | 2,175 |
| `Agent` | 2,050 |
| … 23 more | 12,401 |

### Run B — same machine, MCP connectors loaded, 176 tools

Ground truth: **152,017 context tokens on turn 1**, before the user's second
sentence.

| Component | Exact tokens | Share |
| --- | ---: | ---: |
| Tool definitions (176) | 142,546 | **93.8%** |
| System prompt | 6,300 | 4.1% |
| Messages | 3,027 | 2.0% |
| Framing | 145 | 0.1% |

The 142,546 is six claude.ai connectors nobody asked for on this turn:

| Source | Tools | ~tokens |
| --- | ---: | ---: |
| `mcp__claude_ai_Coda` | 33 | 33,100 |
| built-in | 31 | 26,000 |
| `mcp__claude_ai_Canva` | 32 | 23,200 |
| `mcp__claude_ai_Notion` | 28 | 21,500 |
| `mcp__claude_ai_Trello` | 15 | 9,000 |
| `mcp__claude_ai_Supabase` | 29 | 4,000 |
| `mcp__claude_ai_Google_Drive` | 8 | 2,700 |

Peektrace's view of the same turn: `system_tools=149243` opaque, everything
else — listings, prompts, tool results, assistant text, thinking — summing to
under 3,100. Again the floor is exact (wire decomposition reproduces 149,244 vs
149,243) and again it is undivided.

**This is the finding.** "You are paying 116k tokens per session for Coda,
Canva, Notion, Trello, Supabase and Drive schemas, and here is the per-tool
bill" is a sentence Peektrace cannot say today and could say from one captured
request. It is also directly actionable in a way the current chart is not: the
fix is to disconnect a connector.

## 4. Other things only the wire has

- **Thinking text.** The transcript stores retained thinking blocks as `""` —
  `analyze.ts` reconstructs the token count from `output_tokens` minus visible
  text, and the UI shows *"content not stored in transcript"*. The wire carried
  the full text (796 chars on Run B turn 1). Every resend carries it until
  `context_management` clears it.
- **Cache breakpoints, and where they move.** `system[1]` and `system[2]` carry
  `cache_control: {ttl: "1h"}`; the tools array carries none; a fourth
  breakpoint sits on the last content block and moves forward each turn (last
  user block → last `tool_result`). The transcript reports the *consequences*
  (`cache_creation_input_tokens` / `cache_read_input_tokens`) but never the
  placement, so a cache miss is currently unexplainable from disk.
- **Request configuration.** `thinking: {budget_tokens: 31999, display:
  "omitted"}`, `context_management: {edits: [{type: "clear_thinking_20251015",
  keep: "all"}]}`, `temperature`, `max_tokens`, and the full `anthropic-beta`
  list (8 betas incl. `context-management-2025-06-27`,
  `prompt-caching-scope-2026-01-05`). None of it is on disk.
- **A billing/telemetry system block.** `system[0]` is
  `x-anthropic-billing-header: cc_version=…; cc_entrypoint=sdk-cli;` — a header
  smuggled in as a system block, and part of the prompt the model sees.
- **Framing overhead is measurable.** `count_tokens` came in 145–147 tokens
  under the real `usage` on all three turns measured — consistent enough to be
  a constant, not noise. Today that is invisible.

**What the wire does *not* add:** headline totals. `usage` in the transcript is
already ground truth for context, cache and output tokens, and per-turn growth
curves are fine as they are. Capture is not a replacement source — it is a
decomposition of the one bar the transcript can't break down.

## 5. Storage and privacy

Raw request bodies are the most sensitive artifact this tool could hold: full
file contents, whatever the user typed, the whole system prompt. Concretely, one
Run B body is **898 KB on disk for a single turn**, ~90% of it identical tool
schemas resent every turn.

What the spike established:

- **`redactText` works unmodified on wire bodies.** Run against a capture with
  planted credentials it caught `sk_live_…`, an AWS access key id and a GitHub
  token, and it redacts the `Authorization: Bearer sk-ant-oat01-…` header to
  `[REDACTED:anthropic-key]`. The existing transcript redaction is the right
  default here, not a new one.
- **It does not catch account identifiers.** `metadata.user_id` carries
  `device_id`, `account_uuid` and `session_id`; Codex carries `client_metadata`
  and `prompt_cache_key`. None are secret-shaped, so no regex fires. These need
  an explicit field-level rule, not an entropy rule. (The artifacts from this
  spike were scrubbed by hand for exactly this reason.)
- **Credential headers should never be written at all.** The harness drops
  `authorization` / `x-api-key` / `cookie` before serialization, independent of
  redaction. Redaction is a net; not writing the token is a wall.

Retention: nothing should keep raw bodies. The value shown above is entirely in
the *decomposition* — per-tool token costs, system-prompt size, breakpoint
positions — which is a few KB per turn and dedupes almost perfectly across
turns. Store the derived table, drop the body. That also makes the storage
question answerable without a retention policy: there is nothing to retain.

## 6. Live versus post-hoc

The capture is inherently live — it exists only while the proxy is running and
the agent is pointed at it — while the inspector is built around finished
sessions on disk. Those don't have to be the same surface.

The cheap version keeps the existing shape: the proxy writes a derived sidecar
next to the session it belongs to (`x-claude-code-session-id` is on every
request, so the join key is free and exact), and the inspector reads it
post-hoc, the way it reads everything else. A live pane is a separate, larger
piece of work and this spike says nothing about whether it is worth it.

## 7. Recommendation

Build the narrow thing:

1. **Capture proxy as an opt-in CLI command**, loopback-bound, base-URL only, no
   CA, printing the exact env var to export. Off unless invoked.
2. **Derive and discard.** Per-request: per-tool token costs, system-block sizes
   and cache-breakpoint positions, betas, request config. Write that; never
   write the body. Field-level scrub for `metadata.user_id` and the Codex
   equivalents on top of `redactText`.
3. **Join by session id** and let the existing analyzer replace the estimated
   `system_tools` slice with the measured split when a sidecar exists —
   `CAT_META.system_tools.estimated` becomes `false` for those sessions, and
   `unattributed` mostly disappears.

Explicitly not now: MITM/CA support, a live-tailing pane, response capture, any
attempt to make capture the primary source.

**What would have killed this idea:** if the tool definitions had turned out to
be reconstructible from disk, or if the opaque floor had been small. Neither
holds — it is 77–94% of the window and the only place it exists is the wire.

## 8. Caveats

- Two Claude Code sessions on one machine, one Claude Code version (2.1.220),
  one model (`claude-sonnet-4-5`). The shape is stable across both, but no claim
  is made about older/newer clients.
- Codex was verified for *plumbing only* — a real request body reached a local
  sink — not end-to-end against OpenAI, because the spike environment had no
  Codex credentials. Its decomposition (46 KB of `input[]` items with
  `developer` role, no separate `tools` array) is read off the captured body.
- Pi untested.
- The `count_tokens` differencing assumes components are independently
  tokenized. The two independent closures (35,458 vs 35,457 and 149,244 vs
  149,243) are the evidence that it holds.

## 9. Reproducing

```sh
# terminal 1
bun run scripts/wire-capture.ts --out .wire-capture

# terminal 2 — Claude Code
ANTHROPIC_BASE_URL=http://127.0.0.1:8899 claude -p "hello"

# terminal 2 — Codex
codex exec --config model_provider=local \
  --config 'model_providers.local={name="local",base_url="http://127.0.0.1:8899/v1",env_key="KEY",wire_api="responses"}' \
  "hello"

# decomposition
bun run scripts/wire-capture.ts --summarize .wire-capture
```

Captures are redacted and stripped of credential headers before they hit disk.
`--no-redact` exists for debugging the redactor and should not be used on a real
session.
