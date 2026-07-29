# Gate Decision: Slice A — Action Guide Caption Provenance

**Status:** Verified
**Date:** 2026-07-29
**Branch:** feat/action-guide-agent-foundation-captions
**Commit:** 5029649

## 1. Selected architecture

The shared agent foundation contracts in `rollshot-agent` become domain-tagged
where they currently hardcode Smart Redaction shapes. The caption suggestion
flow uses a bounded single-submit profile with a bundled skill, typed artifact
promotion, review receipts, restore, and durable audit.

- **`SourceBinding`** becomes an enum with `SmartRedaction`, `ActionGuideProject`,
  and `ActionGuideEphemeralGuide` variants (serde-tagged, round-trip safe).
- **`AuthoritySubject`** adds `ActionGuideProject` and `ActionGuideEphemeralGuide`
  alongside the existing `Document`.
- **`DocumentContentBinding`** becomes optional on `AuthorityBinding`, so a caption
  run constructs authority from a guide revision and projection digest instead of
  a base image and annotation state.
- **`AuthoritySubject::ActionGuideProject`** uses a `b"rollshot-authority-subject-action-guide-project-v1\0"`
  domain separator in its digest formula (adopted because Task 7's audit came back
  clean — see Section 3).
- **`ProductTaskSnapshot`** gains `run_contract`, `audit_handle`, and
  `caption_kind` fields; the `ReadyForReview` payload surface becomes
  kind-agnostic (caller-serialized bytes, `canonical_payload_sha256` for
  integrity).
- **`TaskStore`** moves from `result_workspace::workbench::task_store` to
  `agent_store::task_store` — one process-wide instance, shared by both
  workspaces.
- **Caption run profile** is a single-submit bounded run extracted from the
  visual annotation shape, extended with authority snapshot, skill use, and audit
  sink threading.
- **Caption proposal restore** repopulates the review surface from a persisted
  `CaptionProposal` without a provider call.
- **Reconciliation** sweeps ephemeral `ReadyForReview` tasks to `Stale` and
  running tasks to `Interrupted` on open, using a shared helper for both
  domains.
- **Durable audit** writes per-task JSONL journals with hash-chain integrity and
  write-ahead transition records, covering all material caption lifecycle events.

## 2. Gate A1 evidence table

| Gate A1 item | Evidence |
|---|---|
| 1. Durable task, new kind, run contract bound | `caption_task_file_holds_no_image_bytes_and_no_skill_body` (run-contract assertions: no image bytes, no skill body in task file), `caption_task_lifecycle_appends_every_material_event` (audit journal covers every caption lifecycle transition) |
| 2. Typed artifact bound to origin | `promotion_binds_the_kind_the_origin_and_the_payload_digest` (caption artifact carries `Caption` kind, `DurableProject` origin, and `canonical_payload_sha256`) |
| 3. Review receipt bound to artifact revision | `review_receipt_partitions_decisions_and_binds_the_artifact_revision` (receipt partitions Applied/Rejected and binds artifact revision), `accepted_suggestions_land_in_applied_not_rejected` (accept path exercises the Applied arm) |
| 4. Deterministic stale rejection | `identity_ignores_freshness_and_rejects_other_domains` (source-binding identity rejects mismatched domains), `restore_declines_and_marks_stale_when_the_revision_moved` (revision mismatch → Stale), `restore_is_deterministic_across_repeated_calls` (same inputs → same outcome) |
| 5. Reconciliation after restart | `open_marks_ephemeral_ready_for_review_stale` (ephemeral → Stale on open), `open_marks_running_tasks_interrupted_for_both_domains` (running → Interrupted for both Smart Redaction and caption tasks), `open_is_idempotent_across_two_restarts` (second restart is a no-op) |
| 6. Restore without a provider call | `restore_repopulates_the_review_surface_without_a_provider` (panicking adapter proves no provider call is made) |
| 7. Budget and cancellation honored | `single_submit_reports_wall_time_exhaustion` (wall-time budget exhaustion → terminal), `single_submit_reports_cancellation_before_the_first_turn` (pre-turn cancel → terminal), `single_submit_reports_cancellation_mid_stream` (mid-stream cancel → terminal), `wall_time_exhaustion_reports_the_frozen_timeout_copy` (timeout maps to exact user-visible copy) |
| 8. Audit coverage, privacy-safe | `caption_task_lifecycle_appends_every_material_event` (all material caption events audited), `a_failed_caption_run_appends_task_terminated` (failure → `TaskTerminated` event), `an_authority_denied_submit_appends_authority_denied_and_does_not_promote` (denial → `AuthorityDenied` event, no promotion) |
| 9. Smart Redaction unregressed, V1 fixtures load | `loads_pre_migration_schema_fixtures` (V1/V2 schema fixtures deserialise), `legacy_flat_dry_run_counters_become_a_smart_redaction_summary` (V1 counters migrate to smart redaction summary), plus the full workbench suite (77 passed) |
| 10. Restore path UI evidence | Task 21 `restored_caption_proposal_renders_review_surface` (structural iced test: "Suggested captions" header, "Accept all" / "Dismiss" / per-suggestion "Accept" buttons visible), `render_restore_caption_proposal_visual_scenario` (visual baseline captured at `target/ui-artifacts/timeline-workspace/restore-caption-proposal-wgpu.png`, reviewed by independent reviewer — verdict: ACCEPT) |

## 3. Task 7 authority-digest audit

**Verdict: Clean.** No site recomputes an authority digest from a loaded
snapshot and compares it against a persisted string. The `Document` arm's
existing separator-less formula was therefore safe to extend: the new
`ActionGuideProject` arm uses a `b"rollshot-authority-subject-action-guide-project-v1\0"`
domain separator, and the `ActionGuideEphemeralGuide` arm uses
`b"rollshot-authority-subject-action-guide-ephemeral-guide-v1\0"`.

The full site-by-site classification (23 production sites, 15 test sites,
4 notes) is recorded in Task 7 of the implementation plan
(`docs/superpowers/plans/2026-07-28-action-guide-agent-foundation-captions.md`,
§Task 7 Step 1). Key structural guarantees confirmed:

- `ContinuityProjectionV1` is never persisted (no `Deserialize` impl, rebuilt
  from `ProductTaskSnapshot` on every use).
- `AuthoritySnapshot` cannot be reconstructed from a persisted receipt (no
  `Deserialize` impl).
- The continuity-recovery check compares cached in-memory values against a
  projection rebuilt from the same process's snapshot — same code version,
  no cross-process cross-version exposure.
- The pinning test `persisted_authority_digest_is_never_recomputed_for_comparison`
  (Task 7 Step 2) is green.

## 4. Slice A extras

### Schema fixtures

`loads_pre_migration_schema_fixtures` verifies that all three on-disk schema
versions (`task-schema-v1.json`, `task-schema-v2.json`,
`task-schema-v2-ready.json`) deserialise correctly through the legacy-tolerant
`Deserialize` path.

### Two-domain concurrency

`open_marks_running_tasks_interrupted_for_both_domains` seeds one Smart
Redaction task and one caption task, opens the store, and verifies both
transition to `Interrupted`. This is a regression test that the lock and the
audited-write path behave for two task kinds sharing one tree.

### Prompt text assertion

`bundled_caption_skill_body_matches_the_recorded_instruction_text` verifies the
bundled `SKILL.md` body equals `CAPTION_INSTRUCTION_BASELINE` byte for byte.
`bundled_caption_skill_golden_digest_stable` pins the golden digest
(`3aaa0566bd4cb28eecc87f964c093a4d3bceeb7c0e4d0de0860a43988cb865db`).

### Authority-digest audit result

See Section 3. Clean. The separator formula was adopted.

## 5. Migrations performed

| Migration | Scope | Rollback |
|---|---|---|
| `SourceBinding` struct → enum | All call sites in `rollshot-agent`, `rollshot-app` | Revert to struct with `smart_redaction` constructor; `ActionGuide*` fixtures become dead code |
| `AuthoritySubject` new variants | `rollshot-agent` authority, `rollshot-app` caption run | Remove variants; caption run reverts to `ProtocolFailure` |
| `DocumentContentBinding` optional | `rollshot-agent` authority binding | Restore required field; caption authority construction breaks |
| `ProductTaskSnapshot` new fields | `rollshot-agent` product_task, `rollshot-app` task store | Remove fields; audit handle and run contract become unavailable |
| `TaskStore` module move | `rollshot-app` `result_workspace::workbench::task_store` → `agent_store::task_store` | Move back; Action Guide workspace loses store access |
| Artifact payload surface | `record_ready_for_review` parameter, `PromotionContext` | Restore `SmartRedactionReviewPayload` parameter type |
| `ReviewReceipt` field rename | `document_state_id` → `resulting_document_state_id`, `document_digest` → `resulting_document_digest` | Revert field names; `AuthoritySnapshotReceiptV1` sites break |
| V1/V2 on-disk compatibility | Legacy-tolerant `Deserialize` on `SourceBinding` and `ProductArtifactMetadata` | Remove shims; pre-migration files fail to load |

## 6. Residual risks

1. **The text-JSON caption fallback is gone.** `SKILL.md` still instructs the
   model to return bare JSON when tool calling is unavailable (preserved verbatim
   by design), but `run_single_submit_with_provider` treats a completion with no
   terminal tool call as `ProtocolFailure`. Providers without tool calling now
   fail where they previously succeeded. Task 16 Step 5a records the behavior;
   correcting the instruction text is deliberately out of scope (spec §9: "no
   caption prompt improvement").

2. **Project identity is a canonicalized path digest.** Accepted in spec §10;
   moving a project orphans pending tasks to `Stale`.

3. **`ProductArtifactMetadata` gained a hand-written `Deserialize`.** Adding a
   field to that struct now requires editing two places. Task 5 Step 3a explains
   why; a future slice that drops V1/V2 on-disk support should delete the shim.

4. **The open-time sweep and `reconcile_for_source` both own the
   non-terminal-to-`Interrupted` rule.** Task 19 factors the decision into one
   helper; if a later slice changes one, it must change the helper.

5. **The two workspaces are separate processes.** The umbrella and child spec
   describe "one store shared into both workspaces"; in code, `main.rs` dispatches
   into mutually exclusive iced applications, so the invariant is enforced per
   workspace root. Slice B should not plan against a shared owner that does not
   exist.

## 7. Verification command results

### Full regression

| Suite | Passed | Failed | Ignored |
|---|---|---|---|
| `rollshot-agent` | 546 | 0 | 0 |
| `rollshot-action` | 414 | 0 | 0 |
| `rollshot-app` (no features) | 893 | 0 | 6 |
| `rollshot-app --features action-guide` | 1384 | 0 | 7 |

### Formatting and lint

| Check | Result |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |

## 8. Spec section coverage

| Spec section | Task |
|---|---|
| §3.1 `SourceBinding` | 2, 3 |
| §3.2 `ArtifactSummary` | 5 |
| §3.3 payload surface | 6 |
| §3.4 new variants | 9, 10 |
| §3.5 `TextMetadataOnly` | 9 |
| §3.6 `AuthoritySubject` + digest uncertainty | 7, 8 |
| §3.7 store schema | 4 |
| §3.8 store module move | 11 |
| §4.1 single-submit profile | 15 |
| §4.2 caption skill | 13, 14 |
| §4.3 authority construction | 16 |
| §4.4 budget and preserved behavior | 1, 16 |
| §5.1 lifecycle mapping | 16, 17 |
| §5.2 batch review | 17 |
| §5.3 restore | 18 |
| §5.4 ephemeral reconciliation | 19 |
| §6 failure semantics | 15, 16 |
| §7 privacy | 20 |
| §8 Gate A1 mapping | 21, 22 |

## 9. Deferred scope

Per spec §9 and the umbrella:

| Deferred | Why |
|---|---|
| Any change to `run_visual_annotation_with_provider` | Slice B owns the migration; touching it here removes Gate B1's falsification value |
| Caption prompt improvement, caption eval harness | Spec §9. Task 13 moves the text verbatim for a clean baseline |
| Fixing `SKILL.md`'s "return only JSON" sentence now that the text fallback is gone | Would be a prompt change, which §9 forbids. Recorded as residual risk 1 |
| New UI surface, affordance, or copy | Umbrella constraint. Restore reuses the existing review surface |
| Unifying `ActionGuideContextProjectionV1` with `rollshot-agent::continuity` | Spec §9 |
| Project manifest schema change (stable project UUID) | Spec §3.1 rejected it; path digest accepted with `Stale` as the consequence of a move |
| Dropping the V1/V2 on-disk shims | Both compatibility deserializers exist only for pre-migration files; deletion is a separate decision |

## 10. Decision

**Gate A1: VERIFIED.**

All ten gate items are evidenced by named tests. Full regression passes across
all four suite configurations. fmt and clippy are clean. Task 7's
authority-digest audit came back clean, and the separator-extended formula was
adopted. Slice A extras (schema fixtures, two-domain concurrency, prompt text
assertion, authority-digest audit) are recorded above. Five residual risks are
identified and accepted. Eight migrations are recorded with rollback paths.
