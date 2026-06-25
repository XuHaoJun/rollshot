# Preset Workbench Handoff (SP6)

**Date:** 2026-06-25
**Branch:** `feat/preset-workbench`
**Base:** `bfb5693`
**Head:** `0363196`

## What Landed

Tasks 1–9 of the Preset Workbench implementation plan, plus a `cargo fmt` pass.

### Task 1: Dependencies + Workbench module scaffolding
- Added `rollshot-{agent,preset,vision,edit-proposal,automation,automation-rquickjs}` deps to `rollshot-app`
- `tokio` moved from linux-only to general deps; added `async_stream`, `toml`, `chrono`
- Created `workbench/` module with `WorkbenchState`, `WorkspaceMode`, `WorkbenchMessage` (25 variants including `CandidateUnrejected`), `WorkbenchError`, `RunState`, `CandidateReview`, `ActivityEntry`, `PayloadMode`, `VisionContext`, `PendingDraft`, `PendingRunParams`, `RunKind`
- Added `mode: WorkspaceMode` field to `ResultWorkspace`
- Added `Message::SmartRedaction` + `Message::Workbench(WorkbenchMessage)` with routing stubs
- Toolbar "Smart Redaction" button

### Task 2: Provider configuration
- `ProviderConfig` with `ProviderKind`, `KeySource` (env var name only — key never persisted)
- `load_provider_config` / `save_provider_config` via toml at `rollshot_config_dir()/provider.toml`
- `resolve_key` from env, `has_key`, `provider_model_label`
- `build_adapter` constructs Anthropic/OpenAI adapter from same config (§10.7 single-source rule)
- 6 unit tests

### Task 3: Candidate review model
- `CandidateReview::from_candidates`, `mark_rejected`/`mark_modified`/`mark_pending`/`mark_accepted`
- `decision_sets`, `is_empty`, `pending_count`, `rejected_count`, `modified_count`, `warning_count`
- `RunState::is_idle`/`is_running`
- `event_to_activity_entry` (TurnComplete → None per §10.8)
- `terminal_state_label` (exhaustive over all 8 variants)
- 7 unit tests

### Task 4: Review → apply orchestration
- `restamp_proposal` (fixes DryRunTool's hardcoded `base_document_state_id: 0`)
- `build_review_decision` from proposal + review + doc state id
- `apply_candidates` (restamp → lower → apply_batch, one undoable transaction)
- 6 unit tests

### Task 5: Run existing preset (headless)
- `run_existing_preset` builds VisualIndex + RealAutomationHost + QuickJsExecutor
- Runs revision's `ValidatedAutomation` through `execute_to_proposal`
- No LLM, no upload. VisionPrepare on bad image; RuntimeFailure on exec error
- 2 unit tests

### Task 6: Canvas candidate overlay + review bar + composer
- `AnnotationCanvas` gains `pending_proposal`/`review`/`selected_candidate` fields
- Candidate draw pass: dashed borders, confidence badges, selected handles, culled to visible rect
- `draw_dashed_rect` + `point_on_rect_perimeter` helpers
- `hit_test_proposal_candidate` (skips rejected)
- E9 geometry test (4 corners + 4 midpoints + wraparound)
- `workbench_view` layout: review bar, candidate list, composer, disclosure modal
- Disclosure radios update `payload_mode` only — explicit Send button confirms (§7.2)
- `CandidateUnrejected` used for Undo button (D7)

### Task 7: Agent run + full handler
- `prepare_vision_context` (VisualIndex + RealAutomationHost)
- `start_agent_run` with ALL addendum fixes:
  - A1: uses `provider_config` parameter (not `wb`)
  - B4: `tokio::spawn` inside `async_stream::stream!` block
  - B5: vision-prep + PNG-encode inside spawned task
  - C6: `payload_mode` gates bytes (OcrLayoutOnly → empty, FullScreenshot → PNG)
- `ChannelEventSink` with `try_send` (non-blocking)
- Session moved by value into spawned task (Send-safe, no Mutex)
- Full `Message::Workbench` handler: all 15+ arms
- 9 reducer tests (exceeds 5 minimum)

### Task 8: Copy/Save gating + result banners
- `has_pending_candidates` + `apply_skip_summary` (3 tests)
- Copy/Save blocked with inline error while unapplied candidates exist
- Result-state banners: no-match, low-confidence-only, candidates-found
- Error/message banner rendering `wb.error` and `state.message` (addendum F)

### Task 9: Save revision + Improve Preset
- `save_revision` validates source, writes immutable revision via PresetStore
- `CorrectionEvidence` + `assemble_correction_evidence` (added_count: 0 — SP6.1)
- `SavePresetOrRevision` handler (creates preset if needed, saves revision)
- `ImStart` context-gated to review/correction states
- Improve modal with explicit include-checkbox
- 4 new tests (2 save round-trip + 2 evidence)

## Platform Verification

### Linux `iced::application`
- `cargo fmt --all -- --check` — PASS (after fmt fix)
- `cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings` — PASS
- `cargo test --workspace --exclude rollshot-ocr` — 1284 passed, 5 ignored

### macOS `iced::daemon` Phase::Workspace
- Not manually verified (no macOS machine available). The existing `Message::Workspace(msg)` forwarding in `macos_product.rs:344-348` covers nested `Workbench` variants — verified by code inspection. No macOS-specific code was added.

## Known Limitations / Deferred

- **In-memory sessions only (D7):** No cross-run resume. Session-restore-on-terminal is documented for the deferred persistence subproject.
- **Budget tuning UI deferred:** Finite budget built from documented defaults (`smart_redaction_budget()`); per-run budget configuration is a later product decision.
- **Full provider-management settings UI deferred:** Minimal key-presence surface only; provider switching/keychain UX is a later product decision.
- **`payload_mode` (OcrLayoutOnly) honored at modal copy layer:** Bytes-gating is implemented (C6 fix), but the modal's "This run will send:" copy always shows "Screenshot image" — it should omit that line under OcrLayoutOnly. Follow-up polish.
- **Improve run-kind distinction vs Author in the modal is a stub:** `SendRequested` hardcodes `RunKind::Author`. The improve modal is reachable but the run-kind distinction is not wired (SP6.1).
- **Run-existing UI entry not wired:** `run_existing_preset` is built + tested but no UI calls it. Either add a minimal preset-pick + "Run" button, or note that run-existing is reachable only programmatically in SP6.
- **`Next warning` / `Jump to candidate` viewport scroll not yet wired:** Handlers are no-ops.
- **Activity drawer `ToolCard.summary` is empty:** Tool-specific bounded summaries deferred.
- **Automation review drawer deferred:** Human-readable default tab + advanced source/IR/cost tab not built.
- **Before/after toggle deferred.**
- **Fixture regression UI deferred (per spec §8.3).**
- **`assemble_correction_evidence` added_count always 0:** Real counting is SP6.1.
- **macOS manual verification not performed.**

## Commit History

```
0363196 chore: cargo fmt across workbench module
70e2bf3 feat(workbench): save revision + Improve Preset correction evidence
72ecce6 feat(workbench): Copy/Save gating + product result banners
7d53dda feat(workbench): agent run via Task::run channel bridge + full handler
d6ddd4d feat(workbench): canvas candidate overlay + review bar + composer
8846cec feat(workbench): headless run-existing via execute_to_proposal
fc22a43 feat(workbench): review → apply orchestration
32370e0 feat(workbench): candidate review model + event→activity mapping
430cc01 feat(workbench): provider config domain + load/save + build_adapter
19d01ab fix(workbench): add missing CandidateUnrejected variant (addendum D7)
45d886c feat(workbench): scaffold workbench module + WorkspaceMode
```
