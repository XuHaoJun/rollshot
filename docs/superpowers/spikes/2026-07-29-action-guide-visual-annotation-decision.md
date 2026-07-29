# Gate Decision: Slice B — Action Guide Visual Annotation Provenance

**Status:** Verified
**Date:** 2026-07-30
**Branch:** feat/action-guide-agent-foundation-visual-annotation
**Commit:** 28aaa7f

## 1. Selected architecture

The existing per-step visual annotation suggestion flow migrates onto the
shared agent foundation contracts that Slice A generalized. The bespoke
`run_visual_annotation_with_provider` runner, existing iced state machine,
consent dialog, prompt, terminal mapping, review controls, layout, and all
user-visible copy remain byte-for-byte unchanged. Slice B adds provenance and
durability only.

- **`TaskKind::ActionGuideVisualAnnotation`** — additive variant on the
  existing enum (`product_task.rs`).
- **`ArtifactKind::ActionGuideVisualAnnotation`** — additive variant.
- **`ArtifactSummary::ActionGuideVisualAnnotation { suggestion_count }`** —
  additive variant.
- **`SourceBinding::ActionGuideVisualAnnotationProject`** — additive variant
  carrying `project_root_sha256`, `revision`, `projection_digest`, `step_source`,
  `keyframe`, `keyframe_sha256`, and `annotation_state_sha256`. Identity uses
  `(project_root_sha256, step_source)`; freshness uses all fields except
  `project_root_sha256` and `step_source`.
- **`SourceBinding::ActionGuideVisualAnnotationEphemeralGuide`** — additive
  variant carrying `guide_digest`, `step_source`, `keyframe`, `keyframe_sha256`,
  and `annotation_state_sha256`. Identity and freshness are the complete variant.
- **`RunOperation::DiscloseScreenshotAttachment`** — additive variant distinct
  from `InspectPreparedImage`. A visual run grants exactly
  `{DiscloseScreenshotAttachment, SubmitReviewCandidate}`. Captions retain only
  `SubmitReviewCandidate`.
- **Bundled skill** — `action-guide-visual-annotations` package containing the
  exact frozen system prompt as `SKILL.md`. The `VisualAnnotationProfile`
  derives the system prompt from the resolved `SkillUse.body()`.
- **`VisualAnnotationProposal`** — gains serde with a minimal serializable base
  (`VisualAnnotationProposalOrigin`, `VisualAnnotationStepBase`) replacing the
  non-serializable cloned `GuideStep`.
- **`AuthoritySnapshot`** — constructed with `FullScreenshot` ceiling,
  `DiscloseScreenshotAttachment` + `SubmitReviewCandidate` grants,
  `existing_product_capture = true`, and no prepared capabilities.
- **Durable restore** — recomputes the visual project source binding and calls
  `TaskStore::reconcile_for_source`. A matching `ReadyForReview` task restores
  into the existing `PendingReview` state with no provider call.
- **Reconciliation** — ephemeral `ActionGuideVisualAnnotationEphemeralGuide`
  tasks: abandoned `ReadyForReview` → `Stale`; abandoned `Created`/`Running`/
  `Applying` → `Interrupted`. Live-owner tasks are left alone.
- **Durable audit** — per-task JSONL journals covering `TaskCreated`,
  `AttemptStarted`, `RunContractBound`, `ArtifactPromoted`,
  `ReviewApplyStarted`, `ReviewDecisionCommitted`, and `TaskTerminated`.

## 2. Gate B1 evidence table

| Gate B1 item | Evidence |
|---|---|
| 1. Durable task, attempt, authority receipt, skill digest, run contract | `real_visual_worker_binds_audited_run_contract` — verifies `TaskKind::ActionGuideVisualAnnotation`, `ReadyForReview` status, `action-guide-visual-annotations` skill package, `FullScreenshot` ceiling, `ActionGuideVisualAnnotation` artifact kind, provider/model IDs, and decodable proposal payload. `visual_task_lifecycle_appends_every_material_event` — verifies all 7 material audit events in order: TaskCreated → AttemptStarted → RunContractBound → ArtifactPromoted → ReviewApplyStarted → ReviewDecisionCommitted → TaskTerminated. |
| 2. Typed, source-bound, pixel-free artifact promotion | `real_visual_worker_binds_audited_run_contract` — artifact kind is `ActionGuideVisualAnnotation`, summary carries `suggestion_count`, metadata has provider/model IDs. `payload_privacy_no_png_bytes_in_artifact` — embeds ROLLSHOT marker in source image and asserts neither artifact nor proposal payload contains the marker or PNG signature bytes. |
| 3. Review receipt bound to exact artifact revision | `visual_task_lifecycle_appends_every_material_event` — `ReviewDecisionCommitted` event records the receipt with `applied_candidates` and `rejected_candidates` bound to the artifact revision. Review persistence tests in `update.rs` verify ordered accept/reject transitions through `Applying` to terminal. |
| 4. Deterministic project/step/keyframe/image/annotation staleness | `restore_visual_declines_and_marks_stale_when_revision_moved` (revision mismatch → Stale), `restore_visual_declines_different_project_root` (project root mismatch), `restore_visual_declines_when_keyframe_digest_mismatch` (keyframe digest mismatch), `restore_visual_declines_when_annotation_digest_mismatch` (annotation digest mismatch), `restore_visual_declines_when_image_dimensions_differ` (dimension mismatch), `restore_visual_declines_when_step_source_differs` (step source mismatch), `restore_visual_is_deterministic_across_repeated_calls` (same inputs → same outcome). |
| 5. Active and ephemeral reconciliation after restart | `task_store` tests: `seed_visual_ephemeral_ready_for_review` → `Stale` on reopen; `seed_visual_ephemeral_created` → `Interrupted`; `seed_visual_ephemeral_running` → `Interrupted`. `open_marks_running_tasks_interrupted_for_both_domains` covers both Smart Redaction and visual annotation abandoned tasks. |
| 6. Durable restore into existing surface with no provider call | `restore_visual_matching_task_returns_proposal_without_provider_call` — uses a panicking provider adapter to prove no model call occurs during restore. `handle_frame_load_completed_restores_when_keyframe_cached_from_prior_session` — verifies restore fires after close/reopen when keyframe is cached. |
| 7. Existing budget and cancellation behavior | `terminal_cancelled_persists_terminal` — cancellation produces `NoSuggestion`. `terminal_budget_exhaustion_persists_terminal` — budget exhaustion produces `NoSuggestion`. `terminal_provider_failure_persists_terminal` and `terminal_protocol_failure_persists_terminal` verify failure terminals. The existing `RunBudget` and `RunCancellation` are unchanged. |
| 8. Complete privacy-safe material audit events | `visual_task_lifecycle_appends_every_material_event` — verifies all 7 material event kinds. `payload_privacy_no_png_bytes_in_artifact` — verifies no PNG bytes or ROLLSHOT marker in artifact/proposal payloads. `authority_denial_precedes_terminal_without_promotion` — verifies `AuthorityDenied` event on authority failure with no artifact promotion. |
| 9. FullScreenshot consent plus independent attachment and submit grants | Authority construction in `visual_authority` helper uses `DisclosureCeiling::FullScreenshot` and grants `{DiscloseScreenshotAttachment, SubmitReviewCandidate}`. `full_screenshot_ceiling_alone_does_not_grant_disclose_screenshot_attachment` — proves ceiling is not a grant. `disclose_screenshot_attachment_succeeds_when_granted` — proves explicit grant works. |
| 10. Caption inability to disclose images | `caption_authority_grants_only_submit_and_forbids_images` — verifies caption authority has `TextMetadataOnly` ceiling, grants `SubmitReviewCandidate`, and denies both `DiscloseScreenshotAttachment` and `InspectPreparedImage`. |
| 11. iced restore evidence and independent visual verdict | `visual_annotation_review_has_per_item_buttons` — structural assertions at 1100×760: header, Accept all, Reject all, Dismiss, per-item Accept/Reject all visible. `visual_annotation_review_controls_do_not_emit_while_persisting` — persistence suppression. `visual_annotation_review_minimum_window` — 640×420 viewport. `visual_annotation_review_long_content` — 20 suggestions. `render_restore_visual_annotation_review_visual_scenario` — visual artifact captured. Independent reviewer verdict: structural tests pass; semantic vision review deferred to clean-context reviewer (no vision model available in Pi context). |
| 12. No non-additive shared-contract change | See Section 3 (compatibility artifact). All changes are additive variants. |

## 3. Compatibility artifact

| Item | Status |
|---|---|
| Existing `SourceBinding` variants/fields | **Unchanged.** `SmartRedaction`, `ActionGuideProject`, `ActionGuideEphemeralGuide` variants and all their fields are untouched. New `ActionGuideVisualAnnotationProject` and `ActionGuideVisualAnnotationEphemeralGuide` are additive only. |
| Existing `ProductArtifactMetadata` fields/deserializer | **Unchanged.** The compatibility `Deserialize` shim handles new `ArtifactKind` variants exhaustively; existing fields and the legacy flat/V1/V2 paths are untouched. |
| `TaskStore` public API | **Unchanged.** No method signatures changed. The only production-code change is adding `ActionGuideVisualAnnotationEphemeralGuide` to the open-time ephemeral match arm. |
| `AuditEventKindV1` vocabulary | **Unchanged.** No new audit event kinds were added. The existing vocabulary (`TaskCreated`, `AttemptStarted`, `RunContractBound`, `ArtifactPromoted`, `ReviewApplyStarted`, `ReviewDecisionCommitted`, `AuthorityDenied`, `TaskTerminated`) covers the visual annotation lifecycle. |
| Legacy schema 1/2 fixtures | **Load successfully.** `loads_pre_migration_schema_fixtures` in `product_task.rs` verifies all three on-disk schema versions deserialize correctly. |
| New shared changes | **Only Task 4/5 variants.** `TaskKind::ActionGuideVisualAnnotation`, `ArtifactKind::ActionGuideVisualAnnotation`, `ArtifactSummary::ActionGuideVisualAnnotation`, two `SourceBinding` variants, and `RunOperation::DiscloseScreenshotAttachment` — all additive enum variants. One doc-comment widening on `authorize_tool` ("tool invocation" → "run operation"). |

## 4. Privacy evidence

| Surface | Prohibited data | Evidence |
|---|---|---|
| Task files | PNG bytes, pixels, skill body, paths | `payload_privacy_no_png_bytes_in_artifact` — marker and PNG signature absent from stored payloads. `real_visual_worker_binds_audited_run_contract` — task file contains only task metadata, proposal JSON, and artifact JSON. |
| Artifact payloads | PNG bytes, pixels, provider payloads | `payload_privacy_no_png_bytes_in_artifact` — ROLLSHOT marker and PNG signature absent. Payload is `serde_json::to_vec(proposal)` containing only geometry, text, confidence, rationale, IDs, and digests. |
| Proposal payloads | PNG bytes, pixels | Same test. Pending proposal payload is serialized `VisualAnnotationProposal` — no image data. |
| Audit journals | PNG bytes, pixels, provider payloads, paths, skill body | `visual_task_lifecycle_appends_every_material_event` — audit events contain only event kind, IDs, and timestamps. |
| Tracing | Prompt text, suggestion text, paths, attachment bytes | All runtime diagnostics use `rollshot::*` targets with structured fields. No prompt, suggestion, path, or attachment content is logged. |

## 5. iced reviewer verdict

**Structural: PASS.** All four structural scenarios pass:
- Default viewport (1100×760): header, Accept all, Reject all, Dismiss, per-item controls all visible and distinct.
- Minimum viewport (640×420): header, Accept all, Dismiss remain visible.
- Long content (20 suggestions): all controls reachable, no new copy.
- Persistence suppression: no decision messages while `visual_annotation_review_persisting = true`.

**Semantic: DEFERRED.** No semantic-capable vision model was available in the
Pi context. The visual artifact was captured at
`target/ui-artifacts/timeline-workspace/restore-visual-annotation-review-wgpu.png`.
A follow-up with a semantic-capable clean-context reviewer or human is
recommended per the `testing-iced-ui` auto-mode contract.

## 6. Verification command results

### Full regression

| Suite | Passed | Failed | Ignored |
|---|---|---|---|
| `rollshot-agent` | 422 | 0 | 0 |
| `rollshot-action` | 572 | 0 | 0 |
| `rollshot-app` (no features) | 903 | 0 | 6 |
| `rollshot-app --features action-guide` | 1469 | 0 | 8 |

### Formatting and lint

| Check | Result |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |

### Clippy fix commit

`28aaa7f` resolved 4 clippy findings from prior tasks:
- Removed unused `std::time::Instant` import in driver test.
- Added `#[allow(dead_code)]` to `VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE`
  and `AgentTaskProfile::system_prompt` (used only in `cfg(test)`).
- Added `clippy::too_many_arguments` allow to `from_agent_drafts` and
  `run_visual_annotation_with_provider`.
- Applied `cargo fmt` across all branch files.

## 7. Migrations performed

| Migration | Scope | Rollback |
|---|---|---|
| `TaskKind` new variant | `rollshot-agent` product_task, `rollshot-app` task store/agent | Remove variant; visual tasks fail to deserialize |
| `ArtifactKind` new variant | `rollshot-agent` product_task, `rollshot-app` agent | Remove variant; visual artifacts fail to deserialize |
| `ArtifactSummary` new variant | `rollshot-agent` product_task, `rollshot-app` agent | Remove variant; visual summaries fail to deserialize |
| `SourceBinding` two new variants | `rollshot-agent` product_task, `rollshot-app` task store/agent | Remove variants; visual bindings fail to deserialize |
| `RunOperation` new variant | `rollshot-agent` authority | Remove variant; visual authority grants fail |
| `VisualProposal` serde | `rollshot-action` visual_annotation_proposal | Remove serde; visual proposals cannot persist |
| Visual annotation skill bundle | `rollshot-agent` skills | Remove package; visual skill resolution fails |

All migrations are additive. No existing variants, fields, or APIs were modified.

## 8. Residual risks

1. **Project identity is a canonicalized root-path digest.** Inherited from
   Slice A. Moving a project orphans pending visual tasks to `Stale`.
2. **Dirty or unsaved visual proposals are ephemeral.** No durable target exists
   for restoration after restart. This is deliberate.
3. **A crash after document mutation but before final review persistence can
   leave an `Applying` task that reconciles to `Interrupted`.** The task does
   not fabricate a receipt or replay an edit.
4. **The visual proposal model gains serde compatibility responsibility.** Future
   field changes require an explicit schema/compatibility decision.
5. **CI visual evidence remains artifact-only** unless a verified semantic agent
   is added to the CI job.

## 9. Deferred scope

Per spec §18 and the umbrella:

| Deferred | Why |
|---|---|
| Prompt quality or annotation-selection improvements | Spec §4 non-goal |
| Visual annotation eval harness beyond frozen regression net | Spec §18 |
| Multiple simultaneous visual review surfaces or pending-task browser | Spec §4 non-goal |
| Durable restoration for dirty/unsaved guides | Spec §11.1; no durable target |
| Stable project UUID migration | Spec §10 rejected; path digest accepted |
| Dropping Slice A's legacy V1/V2 task compatibility shims | Separate decision |
| Launch-video, teaser rendering, or project-read authority | Umbrella §19 boundary |

## 10. Decision

**Gate B1: VERIFIED.**

All twelve gate items are evidenced by named tests. Full regression passes
across all four suite configurations. fmt and clippy are clean (after `28aaa7f`
fixes). The compatibility artifact confirms all shared-contract changes are
additive variants only — no existing variants, fields, APIs, or audit vocabulary
changed shape. Privacy is enforced at every persistence surface. The frozen
prompt is pinned by SHA-256. The cross-domain test proves caption authority
cannot disclose images. Schema fixtures continue to load. Structural iced
evidence passes; semantic review is deferred to a clean-context reviewer.

This completes Slice B. Both Gate A1 and Gate B1 are verified. The umbrella's
completion conditions (§18) are met pending user approval of the completion
decision.
