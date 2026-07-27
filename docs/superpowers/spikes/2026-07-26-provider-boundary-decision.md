# Provider Boundary Gate G0 Decision

**Date:** 2026-07-27
**Status:** Proposed for user approval
**Rig outcome:** Rig 0.40 adopted

## Decision

Proceed with provider boundary reliability using Rig 0.40 as the active candidate. The production-layer completion gate, host-owned cancellation/deadline controls, and partial-result discard are verified and ready.

## H1 — Host control

**Result:** PASS

7/7 tests verifying host-owned cancellation and deadline controls:
- `provider_progress_cancel_wakes_pending_future`
- `provider_progress_deadline_wakes_pending_future`
- `provider_progress_same_poll_tie_prefers_cancel`
- `runner_cancels_pending_provider_establishment`
- `runner_deadlines_pending_provider_establishment`
- `runner_cancels_pending_provider_item_after_partial_text`
- `runner_deadlines_pending_provider_item_after_partial_text`

`await_provider_progress` uses a biased `tokio::select!` that checks cancellation first, then deadline, then provider progress. `PendingProvider` (which deliberately ignores `StreamBounds`) proves the host guard works independently of adapter cooperation.

## H2 — Completion integrity

**Result:** PASS

### Rig-level probe (spike)
- Both Rig 0.39 and 0.40 synthesize `Final(0 tokens)` on bare EOF for Anthropic and OpenAI
- Neither version distinguishes normal completion from incomplete EOF at the Rig layer
- This proves the production layer must enforce completion independently

### Production-layer gate
40/40 provider_contract tests, 237/237 total tests:

- `anthropic_incomplete_stream_is_not_completed` — PASS
- `openai_incomplete_stream_is_not_completed` — PASS
- `runner_does_not_wait_for_eof_after_valid_completion` — PASS
- `partial_tool_call_never_executes` — PASS
- All normal Anthropic/OpenAI paths — PASS

Two-layer design:
1. `stream_to_model_events` defers `Completed` and checks response usage/tool calls (defense-in-depth)
2. `drive_streamed_turn` rejects bare EOF via `saw_completed` (authoritative)

## H3 — Rig migration

**Result:** PASS

- Rig 0.40 adopted as the active candidate
- Only `Cargo.toml` and `Cargo.lock` changed for the migration
- `StreamedAssistantContent::Unknown` variant caught by existing `Ok(_) => {}` wildcard
- No Rig types leak through the public provider boundary
- Compile clean, 237/237 tests pass, clippy clean

## Independent review

**Verdict:** Ready to merge — yes

Strengths identified:
- Two-layer completion integrity is architecturally sound
- Host-owned guards are adapter-independent
- Partial state cannot cross the commit boundary
- Rig migration is minimal and private
- Privacy-safe error handling

No Critical issues. Two Important findings addressed:
1. `has_real_usage` gate documented as defense-in-depth only (added comment)
2. Duplicate cancellation/deadline logic documented as adapter-side cleanup

## Rejected alternatives

- Provider trait redesign
- Rig patch or fork
- Transport rewrite
- Live-provider acceptance
- EOF inference as completion proof
- Non-zero usage as completion receipt

## Residual risks

1. **External provider cost before cancellation** — a provider call that completes just before cancellation may incur API charges
2. **Live-provider latency/outage behavior** — the gate was tested with local fixtures only; live provider behavior under network stress is untested
3. **Lower-level socket cleanup** — TCP/TLS socket teardown on cancellation/deadline is provider-runtime-dependent
4. **Interrupted-stream billing/usage** — a provider may bill for tokens processed before the stream was interrupted

## Product handoff

Slice 2 may begin only after user Gate G0 approval. The spike will be set to `retained-reference` lifecycle after approval.
