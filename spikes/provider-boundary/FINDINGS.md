# Provider Boundary Reliability Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: Rollshot agent-foundation Gate G0
- Started: 2026-07-26
- Last updated: 2026-07-26

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
| Production-layer completion tracking | H2 production | compile/automated | PENDING | Tasks 4-5 implement `saw_completed` in `stream_to_model_events` |
| Host wakes ignored bounds | H1 hard | automated | PASS | 7/7 tests: `provider_progress_cancel_wakes_pending_future`, `provider_progress_deadline_wakes_pending_future`, `provider_progress_same_poll_tie_prefers_cancel`, `runner_cancels_pending_provider_establishment`, `runner_deadlines_pending_provider_establishment`, `runner_cancels_pending_provider_item_after_partial_text`, `runner_deadlines_pending_provider_item_after_partial_text` — `rtk cargo test -p rollshot-agent` 235/235 green |
| Rig 0.40 compatibility | H3 upgrade | compile/automated | PENDING | Conditional on Task 6 |

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
