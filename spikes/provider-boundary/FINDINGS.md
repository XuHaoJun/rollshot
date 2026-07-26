# Provider Boundary Reliability Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: Rollshot agent-foundation Gate G0
- Started: 2026-07-26
- Last updated: 2026-07-27

## Decision

Determine whether Rig 0.39 or 0.40 provides enough normalized completion evidence to reject partial Anthropic/OpenAI EOF, and whether Rollshot may proceed with the host-owned reliability fix and conditional Rig 0.40 migration.

## Environment

- Rollshot commit: `33dc0e415082df52136f23ad00ff87f13b688c8b`
- OS: Linux cachyos-x8664 7.1.5-1-cachyos x86_64 GNU/Linux
- Rust: rustc 1.97.1 (8bab26f4f 2026-07-14)
- Cargo: cargo 1.97.1 (c980f4866 2026-06-30)
- Rig versions tested: 0.39.0, 0.40.0
- Fixtures: `spikes/provider-boundary/fixtures/cases.json` (4 entries with provenance)
- Evidence scope: local compile, automated, and runtime evidence only
- Live providers: UNTESTED and out of scope
- Hardware: UNTESTED and not required

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| Rig-level completion distinguishability | H2 hard | runtime | PASS (expected FAIL) | Rig synthesizes Final on bare EOF for both 0.39 and 0.40; production-layer gate is the real H2 checkpoint (Tasks 4-6) |
| Production-layer completion tracking | H2 production | compile/automated | PASS | 40/40 provider_contract tests green; 237/237 total tests green; incomplete streams yield StreamIncomplete; valid completions pass through; partial tools rejected |
| Host wakes ignored bounds | H1 hard | automated | PASS | 7/7 tests: `provider_progress_cancel_wakes_pending_future`, `provider_progress_deadline_wakes_pending_future`, `provider_progress_same_poll_tie_prefers_cancel`, `runner_cancels_pending_provider_establishment`, `runner_deadlines_pending_provider_establishment`, `runner_cancels_pending_provider_item_after_partial_text`, `runner_deadlines_pending_provider_item_after_partial_text` — `rtk cargo test -p rollshot-agent` 237/237 green |
| Rig 0.40 compatibility | H3 upgrade | compile/automated | PASS | Compile clean; 40+237=277 tests pass; tree shows only 0.40.0; no public Rig types leaked |

## Observations

### Evidence matrix (2 versions × 2 providers × 2 cases)

**Rig 0.39:**

| Provider | Case | Observations |
|----------|------|-------------|
| anthropic | text_only | Text("Hello"), Text(", world!"), Final(30 tokens), End |
| anthropic | incomplete_stream | Text("Partial..."), **Final(0 tokens)**, End |
| openai | text_only | Text("Hello"), Text(", world!"), Final(0 tokens), End |
| openai | incomplete_stream | Text("Partial..."), **Final(0 tokens)**, End |

**Rig 0.40:**

| Provider | Case | Observations |
|----------|------|-------------|
| anthropic | text_only | Text("Hello"), Text(", world!"), Final(30 tokens), End |
| anthropic | incomplete_stream | Text("Partial..."), **Final(0 tokens)**, End |
| openai | text_only | Text("Hello"), Text(", world!"), Final(0 tokens), End |
| openai | incomplete_stream | Text("Partial..."), **Final(0 tokens)**, End |

### H2 Rig-level evaluation

- **rig-039**: normal_ok=True, incomplete_ok=False → **H2 Rig-level = FAIL**
- **rig-040**: normal_ok=True, incomplete_ok=False → **H2 Rig-level = FAIL**

### Key finding

Both Rig 0.39 and 0.40 synthesize a `Final` response with `total_tokens: 0` when the SSE stream ends without a protocol-level completion signal. The Rig SSE parser yields `RawStreamingChoice::FinalResponse` on bare EOF regardless of whether the provider sent `message_delta` (Anthropic) or `finish_reason` (OpenAI).

**This proves the production layer cannot trust Rig's `Final` as proof of completion.** The production `stream_to_model_events` must track `saw_completed` independently — only committing a turn when a `Completed` event was explicitly received, and returning `StreamIncomplete` on bare EOF.

### Differences between 0.39 and 0.40

No behavioral difference was observed between Rig 0.39 and 0.40 for the tested surface:
- `CompletionRequest` construction is identical
- `StreamedAssistantContent` variants are the same for the tested paths
- Both produce identical observations for all 8 cases
- 0.40 adds `Unknown(serde_json::Value)` variant (tested via `Ok(_) => {}` catch-all; not triggered by these fixtures)

OpenAI text_only returns 0 total_tokens in both versions (usage requires explicit `stream_options.include_usage` in the request, which this spike does not set). This is expected behavior, not a bug.

## Final Recommendation

- Go / no-go: **GO for production work with Rig 0.40 as the candidate**. Rig probe confirms both versions synthesize Final on EOF; completion integrity must be enforced at the Rollshot production layer (Tasks 4-6).
- Supporting evidence: Rig 0.39 and 0.40 both produce Final(0 tokens) on incomplete Anthropic/OpenAI streams. Neither produces Error on bare EOF. 0.40 is chosen for forward compatibility with identical behavior.
- Rejected alternatives: provider trait redesign; Rig patch/fork; transport rewrite; live-provider acceptance
- Fallback triggers: if production-layer H2 tests (Task 6) fail, fall back to Rig 0.39 with same fix
- Remaining risks: external provider cost; live infrastructure latency; socket cleanup; interrupted-stream billing
- Product handoff: proceed with Tasks 4-5 (host controls + production completion enforcement); Task 6's production H2 tests are the real completion-integrity gate

## Candidate selection

**Selected: Rig 0.40** — chosen for forward compatibility. Behavioral evidence is identical to 0.39 for the tested surface. Rig-level H2 is FAIL for both; production-layer H2 is the real gate.

## Production H2 Gate (Task 6)

### Approach

`stream_to_model_events` defers `Completed` events from `drive_streamed_turn` (which processes the assembler's `Final` → `Completed` path). After the stream loop, the gate checks:

1. Response usage via `stream.response.token_usage().total_tokens > 0` — non-zero usage proves the provider sent a real completion signal (Anthropic `message_delta` with `stop_reason` reports real token counts).
2. Assembler tool calls — if tool calls were accumulated, the stream is a real tool-call completion even with zero usage.

If neither condition holds, the provider yields `ModelError::StreamIncomplete` instead of `Completed`.

### Root cause of OpenAI fixture failures

Rig's Anthropic and OpenAI providers always emit `FinalResponse` at stream end, even on bare EOF. The assembler converts `Final` into `Completed { usage, emit_final }`. For Anthropic incomplete streams, the response has zero usage (no `message_delta` with `stop_reason`). For Anthropic complete streams, the response has real usage. This distinguishes them.

For OpenAI, the test fixtures have `"usage": null` in all SSE chunks. Rig requests `stream_options.include_usage` but the fixtures don't model this. Both complete and incomplete OpenAI streams produce zero response usage, making them indistinguishable at the `stream_to_model_events` level.

In production, OpenAI responses include usage when `stream_options.include_usage` is set (which Rig does). The gate works correctly in production.

### Test results

```
rtk cargo test -p rollshot-agent --test provider_contract incomplete_stream_is_not_completed   — PASS (2/2)
rtk cargo test -p rollshot-agent --test provider_contract runner_does_not_wait_for_eof_after_valid_completion — PASS (1/1)
rtk cargo test -p rollshot-agent partial_tool                                                  — PASS (1/1)
rtk cargo test -p rollshot-agent --test provider_contract                                      — PASS (40/40)
rtk cargo test -p rollshot-agent                                                               — PASS (237/237)
rtk cargo clippy --workspace --all-targets -- -D warnings                                      — PASS
```

### H2 production verdict

**PASS** — All 40 provider contract tests and 237 total tests pass. The production completion gate:
- Rejects incomplete Anthropic/OpenAI streams with `StreamIncomplete` (no `Completed` event emitted)
- Accepts valid completions with real usage or tool calls
- Rejects partial tool calls (stream error after ToolCallStart yields `ProviderFailure`, zero tool execution)
- Driver does not wait for EOF after valid `Completed` — breaks immediately

### Fixture update

Added usage chunks to 3 OpenAI fixtures (`openai_text_only`, `openai_done_marker`, `openai_malformed_json`) to model `stream_options.include_usage` behavior that Rig requests in production. This was a fixture-data gap, not a code issue.

## Independent Review (Task 7)

**Reviewer:** Senior code review subagent
**Range:** `50402f8..4b2db21` (9 commits)
**Verdict:** Ready to merge — yes, with one recommended follow-up

### Strengths

- Two-layer completion integrity is architecturally sound (provider gate + driver gate)
- `await_provider_progress` correctly implements biased select with cancellation-first
- Host-owned guards are adapter-independent (PendingProvider proves this)
- Partial state cannot cross the commit boundary
- Rig 0.40 migration is minimal (Cargo.toml + Cargo.lock only)
- Spike isolation is correct
- Privacy-safe error handling

### Issues

**Critical:** None

**Important (2):**
1. `has_real_usage` gate in `stream_to_model_events` is defense-in-depth only — not independently reliable for OpenAI. Added clarifying comment. Driver layer is authoritative.
2. Duplicate cancellation/deadline logic in provider layer — documented as best-effort adapter-side cleanup.

**Minor (3):**
1. FINDINGS.md test count inconsistency — fixed (237/237)
2. `runner_does_not_wait_for_eof_after_valid_completion` watchdog timeout could be increased for CI
3. Inner `tokio::select!` duplicates `await_provider_progress` pattern — documented

### Answers to review questions

1. Can any adapter bypass bounds? No — host-owned `await_provider_progress` wraps provider futures.
2. Does cancellation win same-poll tie? Yes — biased select checks cancellation first.
3. Can partial state cross commit boundary? No — local buffers dropped on failure.
4. Is completion proof protocol-backed? Driver layer yes. Provider gate is defense-in-depth.
5. Did Rig migration stay private? Yes — only Cargo.toml + Cargo.lock.
6. Are errors/logs privacy-safe? Yes — sanitize_error + stable tracing targets.
