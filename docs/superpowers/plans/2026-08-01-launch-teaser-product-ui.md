# Launch Teaser Product UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the complete product-facing Create → Review → optional Agent → Preview → Render → Complete launch-teaser flow to the shared Action Guide Timeline Workspace.

**Architecture:** A focused `launch_teaser` workspace module owns eligibility, review state, bounded edits, proposal decisions, and operation identities. A sibling view module renders the storyboard editor, while a sibling agent module follows the existing durable caption/visual proposal patterns. Preview and final render jobs call the headless `rollshot-action` APIs; final success writes the derived sidecar and exposes native Open/Show in Folder actions.

**Tech Stack:** Rust, iced 0.14, existing Timeline Workspace Elm update pattern, `rollshot-action` launch-teaser APIs, `rollshot-agent` bounded skill/repository APIs, managed FFmpeg, rfd pickers, existing task store/job cancellation/audit infrastructure, repo-local Iced Simulator/Emulator evidence.

## Global Constraints

- Invoke `skill://iced-rs` and `skill://testing-iced-ui` before the first implementation edit.
- The product-changing agent must not approve golden baselines; raw evidence goes to an independent clean-context reviewer as required by the repo-local testing skill.
- The shared Timeline Workspace implementation must be inspected through both Linux and macOS product entry paths.
- Create teaser requires a writable saved project, available native motion, and at least three reviewed steps.
- Provider-free deterministic creation, edit, preview, and final render is a complete path.
- Repository context is optional and requires a visible per-run root/allowlist confirmation.
- Agent output is a second field-level proposal; it never overwrites edits or renders.
- Preview is an external 960×540 temporary MP4 opened with `platform_actions::open_path`; no inline video player is added.
- Final output is a user-selected 1920×1080 silent MP4 and is never duplicated into the project.
- Render requires explicit captured-content/repository-copy confirmation.
- Project/motion staleness disables preview and render and requires regeneration.
- Only one teaser operation can run per workspace; late cancelled/superseded results are ignored by operation ID.
- Prefix every shell command with `rtk`.

---

### Task 1: Teaser workspace state, eligibility, and deterministic creation

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/launch_teaser.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: `TimelineWorkspace::project_session`, `save_state`, `motion`, guide/project loading, and domain/renderer plan Task 1–2 APIs.
- Produces:
  - `LaunchTeaserEligibility`
  - `LaunchTeaserState::{Closed, Reviewing, AgentRunning, PreviewRendering, FinalRendering, Completed}`
  - `TimelineWorkspace::launch_teaser_eligibility(&self) -> LaunchTeaserEligibility`
  - messages `CreateTeaser`, `TeaserSeeded`, `CloseTeaser`, and `RegenerateTeaser`.

- [ ] **Step 1: Load the required Iced skills**

Read:

```text
skill://iced-rs
skill://testing-iced-ui
```

Record the selected auto-mode scenario workflow before editing UI code.

- [ ] **Step 2: Write failing eligibility and lifecycle tests**

Cover unsaved/read-only/dirty project, no motion, unavailable motion, fewer than three steps, eligible clean project, seed success, seed failure, close, regenerate, and late seed result.

```rust
#[test]
fn eligible_project_enters_review_with_deterministic_plan() {
    let mut state = writable_project_workspace_with_motion(3);
    let update = update(&mut state, Message::CreateTeaser);
    assert_eq!(state.launch_teaser_eligibility(), LaunchTeaserEligibility::Eligible);
    assert!(update.task.units() > 0);
    let plan = seed_result_for(&state);
    let _ = update(&mut state, Message::TeaserSeeded { operation_id: 1, result: Ok(plan) });
    assert!(matches!(state.launch_teaser, LaunchTeaserState::Reviewing(_)));
}
```

- [ ] **Step 3: Run focused tests and observe failure**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::tests -- --nocapture`
Expected: FAIL because teaser workspace state does not exist.

- [ ] **Step 4: Implement focused state types**

Define:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchTeaserEligibility {
    Eligible,
    UnsavedProject,
    ReadOnlyProject,
    DirtyProject,
    MissingMotion,
    UnavailableMotion,
    TooFewReviewedSteps,
}

#[derive(Debug)]
pub(crate) enum LaunchTeaserState {
    Closed,
    Reviewing(LaunchTeaserReviewState),
    AgentRunning { operation_id: u64, review: LaunchTeaserReviewState, cancellation: RunCancellation },
    PreviewRendering { operation_id: u64, review: LaunchTeaserReviewState, cancellation: PublishCancellation },
    FinalRendering { operation_id: u64, review: LaunchTeaserReviewState, destination: PathBuf, cancellation: PublishCancellation },
    Completed(LaunchTeaserCompletedState),
}
```

`LaunchTeaserReviewState` owns current plan, last valid external preview metadata/path guard, validation issues, optional pending agent proposal, repository scope draft, content-review checkbox, and the next operation ID.

- [ ] **Step 5: Implement eligibility and asynchronous seed creation**

Eligibility uses the existing writer guard/access state, requires `ProjectSaveState::Clean`, `WorkspaceMotion::Ready`, and at least three guide steps. `CreateTeaser` snapshots operation ID and project root, loads the project on a blocking task, calls `seed_launch_teaser`, and returns `TeaserSeeded`. The reducer ignores mismatched operation IDs.

Any guide-changing message marks an open teaser review stale before applying the guide change. Do this through one `mark_launch_teaser_stale(&mut state)` helper called by rename, delete, reorder, keyframe replacement, caption acceptance, visual-annotation acceptance, undo/redo, and project reload paths.

- [ ] **Step 6: Run lifecycle tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::tests -- --nocapture`
Expected: all lifecycle tests PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/launch_teaser.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(app): add launch teaser workspace state"
```

---

### Task 2: Bounded review edits and field-level proposal decisions

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/launch_teaser.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: `LaunchTeaserPlanV1`, `LaunchTeaserPatchV1`, plan validation.
- Produces:
  - bounded edit messages for hook/outro, order, range, focus, speed, caption, and transition;
  - `LaunchTeaserAgentProposalReview` with per-field decisions;
  - `map_agent_patch(base: &LaunchTeaserPlanV1, patch: LaunchTeaserPatchV1) -> Result<LaunchTeaserAgentProposalReview, String>`.

- [ ] **Step 1: Write failing reducer and proposal tests**

Test every edit, invalid edit rejection, preview invalidation, content-confirmation reset after content edits, reorder preserving 3–5 shots, atomic accept-all, per-field accept/reject, duplicate patch fields, stale base plan, invalid agent duration, and no mutation before acceptance.

```rust
#[test]
fn agent_patch_does_not_change_plan_before_acceptance() {
    let base = valid_plan();
    let proposal = map_agent_patch(&base, patch_changing_hook()).unwrap();
    assert_eq!(proposal.current_plan(), &base);
    assert_ne!(proposal.proposed_plan().hook, base.hook);
}
```

- [ ] **Step 2: Run review tests and observe failure**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::review_tests -- --nocapture`
Expected: FAIL because review reducers do not exist.

- [ ] **Step 3: Implement typed editor messages**

Add messages carrying only typed values:

```rust
TeaserSetHook(String)
TeaserSetOutro(String)
TeaserMoveShot { from: usize, to: usize }
TeaserSetRange { shot: usize, start_ms: u64, end_ms: u64 }
TeaserSetFocus { shot: usize, path: FocusPathV1 }
TeaserSetSpeed { shot: usize, speed: SpeedV1 }
TeaserSetCaption { shot: usize, caption: String }
TeaserSetTransition { shot: usize, transition: TransitionV1 }
TeaserSetContentReviewed(bool)
```

Every edit clones the plan, applies one typed change, validates it, and replaces the current plan only on success. Successful content edits clear the review checkbox and delete/mark stale any preview guard.

- [ ] **Step 4: Implement proposal diff and acceptance**

Represent field paths as an internal enum, not arbitrary strings. Build the complete proposed candidate, map every changed field to `Pending`, and require the candidate to pass domain validation before showing it. `AcceptAll` atomically replaces the plan. Per-field acceptance builds and validates a candidate from current accepted decisions; an invalid combination remains pending and exposes the domain error.

Record accepted agent/user fields into `LaunchTeaserProvenanceV1.accepted_user_edits` with the correct source.

- [ ] **Step 5: Run review tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::review_tests -- --nocapture`
Expected: all review tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/launch_teaser.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(app): review launch teaser plans"
```

---

### Task 3: Iced review surface

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/launch_teaser_view.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**
- Consumes: Tasks 1–2 state/messages and existing selected-frame image handles.
- Produces: Create teaser entry control, modal/full-workspace review UI, proposal diff UI, and completion UI.

- [ ] **Step 1: Define automated UI scenarios before layout code**

Add scenario IDs to the repo-local Iced harness for:

```text
action-guide-launch-teaser-eligible
action-guide-launch-teaser-disabled-no-motion
action-guide-launch-teaser-review-wide
action-guide-launch-teaser-review-narrow
action-guide-launch-teaser-agent-diff
action-guide-launch-teaser-rendering
action-guide-launch-teaser-complete
```

Each scenario uses synthetic nonprivate frames and deterministic text.

- [ ] **Step 2: Add failing view-model tests**

Test exact disabled reasons, button presence, selected shot, card order, validation banner, repository read count, agent diff actions, review checkbox gating, running cancel action, and completion actions. Keep assertions on semantic model/accessible labels rather than widget debug text.

- [ ] **Step 3: Run view tests and observe failure**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser_view::tests -- --nocapture`
Expected: FAIL because the view does not exist.

- [ ] **Step 4: Implement the Create teaser entry and review layout**

Use existing Timeline Workspace visual tokens and iced 0.14 APIs from the loaded skill. Wide layout:

- left column: ordered 3–5 shot cards and reorder controls;
- center: selected keyframe, fixed 16:9 crop/focus overlay, start/end focus handles;
- right column: hook/outro, bounded controls, validation, provenance, repository context, agent action, content confirmation, Preview, and Render.

Narrow layout stacks shot cards, selected preview, and controls without horizontal clipping. Disabled Create teaser copy is exactly:

- `Save this Action Guide before creating a teaser.`
- `This Action Guide is read-only.`
- `Record motion to create a teaser.`
- `The motion recording is unavailable.`
- `Review at least 3 steps to create a teaser.`
- `Save current guide edits before creating a teaser.`

- [ ] **Step 5: Implement agent diff and running/completion surfaces**

Diff rows show current/proposed value plus Accept/Reject. Show **Files read (N)** with expandable relative-path receipts. Running states show operation-specific copy and Cancel. Completion shows duration, dimensions, path label, Open, Show in Folder, and Close.

- [ ] **Step 6: Run view tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser_view::tests -- --nocapture`
Expected: all view tests PASS.

- [ ] **Step 7: Capture raw Iced evidence**

Run the repo-local `testing-iced-ui` auto workflow for all seven scenario IDs. Save raw simulator/emulator evidence in the location prescribed by the skill. Do not update or approve goldens in this task.

- [ ] **Step 8: Send evidence for independent review**

Start a clean-context independent reviewer with only the allowed scenario/baseline paths. Apply baseline updates only if the skill's semantic image-capability and reviewer rules permit them.

- [ ] **Step 9: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/launch_teaser_view.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): add launch teaser review UI"
```

---

### Task 4: Preview and final render jobs

**Files:**
- Modify: `crates/rollshot-app/src/managed_ffmpeg.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/launch_teaser.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/platform_actions.rs`

**Interfaces:**
- Consumes: domain render APIs, `resolve_video_import_toolchain`, `PublishCancellation`, `open_path`, async save picker.
- Produces: preview/final messages, FFmpeg availability gating, cancellation, verified completion metadata.

- [ ] **Step 1: Write failing render-operation tests**

Test FFmpeg unavailable, preview start/success/failure/cancel, stale preview result, edit invalidating preview, external open failure, save-picker cancel, content checkbox gating, final start/success/failure/cancel, stale final result, no destination on failure, and operation exclusivity.

```rust
#[test]
fn final_render_requires_content_confirmation() {
    let mut state = reviewing_workspace();
    let update = update(&mut state, Message::TeaserRenderRequested);
    assert_eq!(update.task.units(), 0);
    assert!(state.launch_teaser_review().unwrap().validation_message().contains("Review captured content"));
}
```

- [ ] **Step 2: Run operation tests and observe failure**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::render_tests -- --nocapture`
Expected: FAIL because render messages do not exist.

- [ ] **Step 3: Add launch-teaser FFmpeg preflight**

Extend managed FFmpeg resolution with a launch-teaser preflight that resolves FFmpeg + ffprobe and checks the fixed required filter set. Return `Available(VideoToolchain)`, `SetupRequired`, or `Unsupported { message }`; never discover filters after the user chooses a destination.

- [ ] **Step 4: Implement preview job**

`TeaserPreviewRequested` reopens the project, validates binding, and calls the preview render profile in `spawn_blocking`. On success, retain the preview guard in review state and call `platform_actions::open_path`. Any edit drops the guard. Close/cancel drops it and removes the scratch directory.

- [ ] **Step 5: Implement final render job**

After checkbox confirmation, show the async MP4 save picker. Admit one operation ID and cancellation token. Reopen project and call final render in `spawn_blocking`. Handle cancellation separately from failures. Ignore late IDs. A verified success transitions to `Completed`; failure returns to `Reviewing` with the plan intact.

- [ ] **Step 6: Run render-operation and platform helper tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::render_tests -- --nocapture
rtk cargo test -p rollshot-app platform_actions::tests -- --nocapture
```

Expected: all tests PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/managed_ffmpeg.rs crates/rollshot-app/src/platform_actions.rs crates/rollshot-app/src/timeline_workspace/launch_teaser.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(app): preview and render launch teasers"
```

---

### Task 5: Optional repository scope and durable agent proposal

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/launch_teaser_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/launch_teaser.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/launch_teaser_view.rs`
- Modify: `crates/rollshot-app/src/agent_store/task_store.rs`

**Interfaces:**
- Consumes: agent/repository plan APIs, current teaser plan, project projection, keyframe attachments, task store, provider configuration.
- Produces: repository scope draft/confirmation, `suggest_launch_teaser_task`, durable ReadyForReview artifact, restore/review receipt, and field-level proposal state.

- [ ] **Step 1: Write failing scope and agent orchestration tests**

Cover root selection, file/directory entries, entry outside root, no implicit reuse, confirmation, provider unavailable, no task store, Action Guide-only run, repository-enriched run, exact files-read UI data, cancellation, timeout, invalid proposal, stale source binding, durable restore, accept/reject receipt, and absolute-path privacy.

- [ ] **Step 2: Run agent UI tests and observe failure**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser_agent::tests -- --nocapture`
Expected: FAIL because orchestration does not exist.

- [ ] **Step 3: Implement repository scope selection and confirmation**

The user selects one workspace root, then adds files/directories through rfd pickers. Convert every choice to a normalized relative path and reject anything outside the root. Show sorted/deduplicated entries and fixed limits before enabling **Authorize for this run**. Closing or completing the run discards the root path and grant; only privacy-safe receipts remain.

- [ ] **Step 4: Prepare bounded model input**

Reopen the project at the expected revision, rebuild `ActionGuideContextProjectionV1`, revalidate motion and current plan, and load only the 3–5 referenced keyframes. Construct an `AuthorizedModelInput` with bounded metadata plus attachments. Use `DisclosureCeiling::FullScreenshot`, `SubmitReviewCandidate`, `DiscloseScreenshotAttachment`, and optional `ReadAuthorizedWorkspaceFile` grants.

Create the source binding `ActionGuideLaunchTeaserProject` with project-root digest, revision, projection digest, and motion digest.

- [ ] **Step 5: Run the bundled profile and map the patch**

Resolve `bundled_action_guide_launch_teaser_use`, create optional `RepositoryReadTool`, compose `launch_teaser_profile`, and run the bounded single-submit driver. Strictly decode `LaunchTeaserPatchV1`, map it through Task 2, and keep the current plan unchanged until review acceptance.

- [ ] **Step 6: Promote and restore durable artifacts**

Follow `caption_agent.rs` durable flow:

- create `TaskKind::ActionGuideLaunchTeaser`;
- start one attempt;
- bind `RunContractReceiptV1` with its optional repository grant receipt;
- promote strict patch payload + metadata to ReadyForReview;
- persist terminal failures/cancellation;
- restore only when source identity and freshness match;
- write `ReviewReceipt` after accept/reject completion.

Store only the strict patch, privacy-safe receipts, and digests. Do not store prompts, provider payloads, attachments, repository root paths, or repository file content.

- [ ] **Step 7: Run orchestration and privacy tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser_agent::tests -- --nocapture
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser_agent::privacy_tests -- --nocapture
```

Expected: all tests PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/agent_store/task_store.rs crates/rollshot-app/src/timeline_workspace
rtk git commit -m "feat(app): add agent launch teaser proposals"
```

---

### Task 6: Verified sidecar persistence and completion actions

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/launch_teaser.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/launch_teaser_view.rs`

**Interfaces:**
- Consumes: final render result, accepted plan/provenance, sidecar API, platform Open/Show in Folder.
- Produces: atomic success persistence, stale sidecar presentation, and completion actions.

- [ ] **Step 1: Write failing completion tests**

Test sidecar written only after verified render, sidecar failure reported without deleting valid external MP4, no project revision increment, output digest recorded, later guide change marks sidecar stale, Open uses MP4 path, Show in Folder uses parent, and project contains no MP4 duplicate.

- [ ] **Step 2: Run completion tests and observe failure**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::completion_tests -- --nocapture`
Expected: FAIL because final success does not persist provenance.

- [ ] **Step 3: Persist the accepted artifact**

Build `LaunchTeaserArtifactV1` from the accepted plan, renderer/FFmpeg versions, final output SHA-256, and current time. Call `write_launch_teaser_sidecar` only after render verification. Treat sidecar write failure as a visible provenance-persistence error while preserving and presenting the already verified external MP4.

- [ ] **Step 4: Implement completion actions and stale history**

Use `platform_actions::open_path` for Open and existing `reveal` for Show in Folder. On project open, load the sidecar; show current history only when fresh and label it stale after guide/motion changes. Never auto-render or reuse a stale plan.

- [ ] **Step 5: Run completion tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::launch_teaser::completion_tests -- --nocapture`
Expected: all completion tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace
rtk git commit -m "feat(app): persist launch teaser results"
```

---

### Task 7: Product acceptance, visual review, and platform verification

**Files:**
- Create: `crates/rollshot-app/tests/launch_teaser_product_contract.rs`
- Modify only if evidence finds a real defect: files from Tasks 1–6.

**Interfaces:**
- Consumes: all prior domain, agent, and product tasks.
- Produces: end-to-end evidence for the approved MVP acceptance criteria.

- [ ] **Step 1: Add provider-free product contract test**

Drive a writable project with persistent synthetic motion and three reviewed steps through create, bounded edit, preview, content confirmation, final render, ffprobe verification, sidecar persistence, Open/Show in Folder command construction, and stale detection after a project revision change.

- [ ] **Step 2: Add scripted-provider product contract test**

Authorize one README file, run the scripted read-then-submit provider, inspect the exact read receipt, accept selected fields, reject another field, render, and assert sidecar provenance. Assert the repository root and file content are absent from durable task/audit JSON except for permitted relative path and digest metadata.

- [ ] **Step 3: Run product contract tests**

Run: `rtk cargo test -p rollshot-app --features action-guide --test launch_teaser_product_contract -- --nocapture`
Expected: PASS with explicit FFmpeg preflight behavior.

- [ ] **Step 4: Run the raw Iced scenarios again**

Use the repo-local `testing-iced-ui` auto workflow for wide, narrow, diff, rendering, disabled, and completion states. Send changed evidence to a new clean-context reviewer; the product-changing agent does not approve baselines.

- [ ] **Step 5: Inspect both platform paths**

Confirm the shared Timeline Workspace is reachable from:

- Linux Action Guide product path in `action_guide_linux_product.rs`;
- macOS product path in `macos_product.rs`.

Exercise Linux Open/Show in Folder on this workstation. Compile and test macOS-gated helper code where the configured target is available. If native macOS runtime execution is unavailable, record exactly that remaining runtime risk in the final implementation report; do not claim runtime verification.

- [ ] **Step 6: Run workspace verification**

Run:

```bash
rtk cargo fmt --check
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app --features action-guide
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all commands PASS.

- [ ] **Step 7: Run the real-motion launch-quality gate**

Record one real Rollshot feature with native motion, create a reviewed Action Guide, produce a 15–25 second teaser through the product path, and evaluate it against the fixed-operation Phase 0 story-gap criteria. Store private case inputs outside git and commit only privacy-reviewed aggregate evidence. This gate may block launch-ready status but does not change implementation-test results.

- [ ] **Step 8: Commit acceptance tests and permitted evidence**

```bash
rtk git add crates/rollshot-app/tests/launch_teaser_product_contract.rs
rtk git commit -m "test(app): cover launch teaser product flow"
```
