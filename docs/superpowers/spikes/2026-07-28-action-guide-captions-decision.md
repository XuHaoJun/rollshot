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
  `agent_store::task_store` — the active product shell owns one process-wide
  instance and injects it into every workspace it creates or opens.
- **Caption run profile** is a single-submit bounded run extracted from the
  visual annotation shape, extended with authority snapshot, skill use, and audit
  sink threading.
- **Caption proposal restore** repopulates the review surface and its durable
  `ProductTaskSnapshot` from a persisted `CaptionProposal` without a provider
  call.
- **Reconciliation** serializes through the store lock and consults per-task
  process-liveness ownership before interrupting running work or staling an
  ephemeral review. A second live process therefore leaves active tasks alone;
  abandoned tasks still reconcile after their owner exits.
- **Durable audit** writes per-task JSONL journals with hash-chain integrity and
  write-ahead transition records, covering all material caption lifecycle events.

## 2. Gate A1 evidence table

| Gate A1 item | Evidence |
|---|---|
| 1. Durable task, new kind, run contract bound | `caption_task_file_holds_no_image_bytes_and_no_skill_body` (run-contract assertions: no image bytes, no skill body in task file), `caption_task_lifecycle_appends_every_material_event` (audit journal covers every caption lifecycle transition) |
| 2. Typed artifact bound to origin | `promotion_binds_the_kind_the_origin_and_the_payload_digest` (caption artifact carries `ActionGuideCaptions`, the durable-project origin, and `canonical_payload_sha256`), `real_worker_promotes_both_caption_payloads_before_success` (the production worker durably promotes artifact and serialized proposal payloads before returning success) |
| 3. Review receipt bound to artifact revision | `review_receipt_partitions_decisions_and_binds_the_artifact_revision` (receipt partitions Applied/Rejected and binds artifact revision), `ordered_caption_review_stays_applying_until_every_candidate_is_decided`, `ordered_caption_review_accept_all_finishes_in_one_batch`, and `ordered_caption_review_persists_rejected_batch` exercise the legal ordered transitions and both terminal receipt partitions |
| 4. Deterministic stale rejection | `identity_ignores_freshness_and_rejects_other_domains` (source-binding identity rejects mismatched domains), `restore_declines_and_marks_stale_when_the_revision_moved` (revision mismatch → Stale), `restore_is_deterministic_across_repeated_calls` (same inputs → same outcome) |
| 5. Reconciliation after restart | `second_live_store_does_not_interrupt_running_task` drives both open-time and source-scoped reconciliation while another owner is live; `second_live_store_does_not_stale_ephemeral_review` covers live review ownership; `open_marks_running_tasks_interrupted_for_both_domains` proves abandoned Smart Redaction and caption tasks still reconcile |
| 6. Restore without a provider call | `opening_project_with_process_store_restores_task_snapshot_and_proposal` verifies the product project-open adapter restores both UI payload and durable snapshot; the provider-free restore tests use a panicking adapter to prove no model call occurs |
| 7. Budget and cancellation honored | `single_submit_enforces_the_caption_argument_budget`, `single_submit_enforces_the_caption_result_budget`, `single_submit_reports_wall_time_exhaustion`, and the cancellation tests cover transport bounds; `real_worker_persists_cancellation_terminal`, `real_worker_persists_provider_and_decode_failures`, `real_worker_persists_wall_time_and_authority_failures`, and `real_worker_terminalizes_attempt_audit_commit_failure` prove production worker terminal persistence, including a post-CAS audit failure |
| 8. Audit coverage, privacy-safe | `single_submit_preserves_authority_audit_failure_category` verifies typed audit failure propagation; `real_worker_persists_wall_time_and_authority_failures` exercises production authority denial through the real audit sink; lifecycle tests verify `AuthorityDenied` / `TaskTerminated` events and no artifact promotion |
| 9. Smart Redaction unregressed, V1 fixtures load | `loads_pre_migration_schema_fixtures` (V1/V2 schema fixtures deserialise), `legacy_flat_dry_run_counters_become_a_smart_redaction_summary` (V1 counters migrate to smart redaction summary), plus the full workbench suite |
| 10. Restore and review UI evidence | `scripted_caption_review_survives_close_and_reopen_without_review_card` drives the real scripted provider, durable promotion, writable product-open path, accepted review persistence, close, and reopen; `caption_review_controls_do_not_emit_decisions_while_persisting`, `caption_review_persistence_blocks_a_new_caption_run`, and `stale_caption_review_persistence_cannot_mutate_a_new_proposal` cover transient interaction suppression and task-correlated completion; the unchanged restore visual baseline received an independent scoped ACCEPT with no baseline update |

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

`open_marks_running_tasks_interrupted_for_both_domains` verifies abandoned Smart
Redaction and caption tasks transition to `Interrupted`. The paired
`second_live_store_*` tests hold a live owner while a second process-store
instance opens the same tree and verify that neither a running task nor an
ephemeral review is disturbed.

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

1. **Project identity is a canonicalized path digest.** Accepted in spec §10;
   moving a project orphans pending tasks to `Stale`.

2. **`ProductArtifactMetadata` has a hand-written `Deserialize`.** Adding a
   field to that struct requires editing both the type and compatibility shim.
   A future slice that drops V1/V2 on-disk support should delete the shim.

3. **The open-time sweep and `reconcile_for_source` share the same
   non-terminal reconciliation policy.** A later policy change must continue
   to update the shared merge decision used by both entry points.

4. **Process liveness is local-host ownership, not distributed coordination.**
   Store locking serializes filesystem mutation and task-liveness records keep
   a second live Rollshot process from interrupting the first process's work.
   This does not make the task store safe to share over a filesystem whose
   locking or process identity semantics differ from the local host.

## 7. Verification command results

### Full regression

| Suite | Passed | Failed | Ignored |
|---|---|---|---|
| `rollshot-agent` | 552 | 0 | 0 |
| `rollshot-action` | 414 | 0 | 0 |
| `rollshot-app` (no features) | 895 | 0 | 6 |
| `rollshot-app --features action-guide` | 1401 | 0 | 7 |

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
| Caption prompt improvement, caption eval harness | Spec §9. The original text instruction remains byte-stable, and the pre-migration text-JSON fallback is preserved |
| New UI surface, affordance, or copy | Umbrella constraint. Restore reuses the existing review surface |
| Unifying `ActionGuideContextProjectionV1` with `rollshot-agent::continuity` | Spec §9 |
| Project manifest schema change (stable project UUID) | Spec §3.1 rejected it; path digest accepted with `Stale` as the consequence of a move |
| Dropping the V1/V2 on-disk shims | Both compatibility deserializers exist only for pre-migration files; deletion is a separate decision |

## 10. Decision

**Gate A1: VERIFIED.**

All ten gate items are evidenced by named tests. Full regression passes across
all four suite configurations. fmt and clippy are clean. Task 7's
authority-digest audit came back clean, and the separator-extended formula was
adopted. Slice A extras (schema fixtures, liveness-aware two-domain
reconciliation, prompt text assertion, authority-digest audit) are recorded
above. Four residual risks are identified and accepted. Eight migrations are
recorded with rollback paths.
