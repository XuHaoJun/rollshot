# Action Guide Project Publishing and Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish the current saved Action Guide revision into a safe, regenerable viewer bundle and optional storyboard/GIF/MP4 derivatives, expose trustworthy per-output freshness and cancellation, and provide separate safe-copy, editable-project, and Issue Pack sharing flows.

**Architecture:** Publishing is a derived, revision-bound background operation. It first renders a complete core viewer into a sibling temporary directory and atomically swaps that directory into `publish/`; optional derivatives are then produced one at a time through lazy frame access and recorded independently in `publish-state.json`. The iced workspace owns operation/revision guards and UI state, while `rollshot-action` owns strict publish-state persistence, lazy reviewed-image resolution, and cancellable bounded-memory renderers.

**Tech Stack:** Rust 2021, iced 0.14, image 0.25 (including its GIF encoder), ffmpeg-sidecar, serde/serde_json, rustix, existing Action Guide export/storyboard/Issue Pack code.

## Global Constraints

- Execute only after `2026-07-17-action-guide-project-app-integration.md` is complete and its gate passes.
- Authoritative spec: `docs/superpowers/specs/2026-07-17-action-guide-project-editing-design.md`.
- Before implementing iced view/subscription work, invoke the `iced-rs` skill and use iced 0.14 signatures.
- A successful Save commits the project first and schedules publishing for that exact committed revision. Save success never depends on publish success.
- The core viewer is always enabled. Storyboard, GIF, and MP4 are explicit project-level toggles in Publish Details; changing a toggle marks the project dirty and takes effect on the next Save.
- Publish never mutates `project.json` or `assets/`. `publish/` and `publish-state.json` remain derived and replaceable.
- Core publication is all-or-old: a failed or cancelled core render leaves the previous `publish/` untouched. Once a new core bundle replaces it, old optional derivatives are intentionally absent until their current-revision jobs succeed; stale files must never be shared as current.
- Optional output failures are independent. Each renderer creates a unique temporary sibling without overwriting one, then atomically renames it over the stable derivative filename and persists the successful revision.
- Share-triggered regeneration is a staged exception: it renders core plus every enabled derivative into one temporary publish directory and commits only after the complete required set succeeds. Cancellation or failure before that commit leaves both prior publish content and publish state unchanged.
- Missing or corrupt `publish-state.json` means every output is stale; it never makes the project unopenable.
- Every worker result carries both `operation_id` and `revision`; the UI discards late results that do not match the active operation and current saved revision. Filesystem commits enforce the same guard before rename, so ignoring a late UI event is not the only race defense.
- Cancellation is cooperative between steps/frames and terminates an active ffmpeg child. Partial temporary files/directories are removed by RAII guards.
- Export code resolves and releases at most one full-resolution frame at a time. The final storyboard canvas and encoder-owned buffers are allowed; a second collection of all decoded frames is not.
- Safe Copy and Issue Pack use only successful outputs for the current saved revision. Editable Project sharing requires a saved, clean project and excludes `.lock`, temporary files, and abandoned transaction artifacts.
- Sharing never copies assets from outside the saved project revision and never follows symlinks.
- Every runtime diagnostic uses `tracing` with a stable explicit `rollshot::*` target and privacy-safe structural fields.
- Commands run from `/home/noah/rollshot` and are prefixed with `rtk`.

---

## File Structure

**Create:**

- `crates/rollshot-action/src/project/publish.rs` — strict publish-state DTO, freshness rules, atomic state persistence, and cancellation token.
- `crates/rollshot-action/tests/project_publish_state.rs` — public publish-state corruption, freshness, and atomicity tests.
- `crates/rollshot-app/src/timeline_workspace/project_publish.rs` — background publish orchestration, core swap, derivative commits, and guarded results.
- `crates/rollshot-app/src/timeline_workspace/share.rs` — safe-copy, editable-project, and Issue Pack share workers.

**Modify:**

- `crates/rollshot-action/src/project/mod.rs` — export publishing contracts.
- `crates/rollshot-action/src/project/store.rs` — expose the existing atomic JSON/fsync primitive within the project module so publish state does not duplicate it.
- `crates/rollshot-action/src/lib.rs` — re-export the reviewed cancellable renderer entry points and existing error/result types.
- `crates/rollshot-action/src/export/model.rs` — add lazy project-backed reviewed images.
- `crates/rollshot-action/src/export/mod.rs` — resolve one reviewed image at a time and add cancellation checks.
- `crates/rollshot-action/src/storyboard.rs` — bounded-memory cancellable storyboard rendering.
- `crates/rollshot-action/src/gif.rs` — stream reviewed frames directly to the GIF encoder.
- `crates/rollshot-action/src/video.rs` — stream reviewed frames to ffmpeg and kill it on cancellation.
- `crates/rollshot-app/src/issue_pack.rs` — accept a lazy reviewed export job and current publish outputs.
- `crates/rollshot-app/src/timeline_workspace/guide_export.rs` — build lazy reviewed jobs from either frame source.
- `crates/rollshot-app/src/timeline_workspace/mod.rs` — publish settings/status, operation ownership, and share state.
- `crates/rollshot-app/src/timeline_workspace/update.rs` — schedule after Save, cancel, ignore late results, and dispatch shares.
- `crates/rollshot-app/src/timeline_workspace/view.rs` — aggregate status, Publish Details toggles/errors, and three explicit share choices.
- `crates/rollshot-app/src/timeline_workspace/project.rs` — include publish toggles in snapshots and surface saved-revision transitions.
- `README.md` — explain publish regeneration and safe versus editable sharing.

## Engineering Review Lock-In

### Step 0: Scope Challenge

- **Goal alignment:** all eight tasks directly support revision-bound publishing, bounded derivative rendering, trustworthy status/cancellation, or one of the three requested sharing boundaries. No task is merely nice-to-have.
- **Complexity check:** 4 net-new files, 0 new crates, and 8 tasks; the automatic reduction threshold is not triggered.
- **Minimum viable slice:** retain all tasks. Deferring any renderer, publish transaction, UI lifecycle, or share boundary would leave a promised output unsafe or unavailable.
- **Framework/API check:** iced 0.14 provides `Task::run` for a stream of progress messages; `Task::perform` maps one future result and is insufficient for `CoreCommitted` plus multiple derivative events. Existing `image::codecs::gif::GifEncoder`, Plan 1 atomic JSON/fsync code, and rustix `RenameFlags::NOREPLACE` already cover the required primitives, so no new GIF or transaction framework is introduced.
- **Footgun check:** named temporary paths need same-filesystem placement and RAII cleanup; no-replace persistence is platform/filesystem dependent; directory plus state replacement is not one filesystem atomic operation and therefore needs explicit rollback; dropping a blocking task does not stop ffmpeg or a blocked pipe write.
- **Completeness check:** the reviewed plan keeps complete failure, privacy, cancellation, and no-overwrite coverage rather than relying on happy-path manual checks.
- **Distribution check:** no new binary, crate, or installable artifact is introduced. The existing `action-guide` feature and Linux/macOS CI lanes remain the distribution boundary.

### Auto decisions applied

- **D1 — Stream publish progress with iced `Task::run`.** Use a bounded `iced::stream::channel`/`Task::run` bridge around `spawn_blocking`; `Task::perform` can deliver only the final future result. This is explicit and uses iced's built-in stream primitive.
- **D2 — Make ShareGate rollback cover both durable writes.** Keep backups of the old publish directory and old state until the new directory and `publish-state.json` are both durable; restore both if either replacement or the final sync fails. This is the minimum design that satisfies the stated all-or-old contract for ordinary errors.
- **D3 — Copy from no-follow handles in cancellable chunks.** Validate/open each allowlisted regular file without following symlinks, then stream from that handle while checking cancellation. A path-check followed by `std::fs::copy` has a TOCTOU privacy hole and cannot promptly cancel a large asset.
- **D4 — Reuse existing renderer contracts.** Keep `GifError`, `VideoError`, `StoryboardError`, and `image::codecs::gif::GifEncoder`; do not invent parallel error types or add a direct `gif` dependency for one codepath.
- **D5 — Reuse Plan 1 atomic JSON persistence.** Generalize the project module's existing tmp + file fsync + rename + directory fsync helper for both manifests and publish state. This prevents two subtly different durability implementations.
- **D6 — Add publish privacy and strict-state negative tests.** Verify redactions are flattened, public output contains no private asset/root references, unknown schema is unavailable/stale, and failed durable commits preserve prior bytes.
- **D7 — Remove Issue Pack overwrite behavior.** Change its existing delete-then-swap destination commit to no-replace and test a collision. All three share choices must honor the plan's no-silent-overwrite promise.
- **D8 — Test diagnostic privacy.** Capture publish/share tracing and assert titles, annotation text, image bytes, and title-bearing paths are absent. A prose logging rule is insufficient at this privacy boundary.
- **D9 — Bound one-frame allocations.** Add a checked derivative-frame pixel ceiling before GIF/video buffer allocation. One-at-a-time resolution alone does not bound a maliciously tall or oversized frame.
- **D10 — Make ffmpeg cancellation interrupt blocked writes.** Add a cancellation watchdog that can kill the child independently of stdin progress, and test it with a fake child that stops reading. Polling only before `write_all` cannot cancel a blocked pipe.
- **D11 — Reconcile durable success with artifact presence.** A matching revision is current only when the expected regular output still exists and the core viewer shape is complete; deletion or symlink replacement makes it stale. Otherwise the UI can claim a file is current after it has disappeared.
- **D12 — Match the approved aggregate indicator.** Use exactly `Publishing`, `NeedsAttention`, and `Published` at the header; keep never-published/current/stale/failed distinctions in per-output details. This avoids contradicting the authoritative spec with extra aggregate states.
- **D13 — Keep the cross-process writer lease alive in workers.** Each mutating publish request owns an `Arc` lease over Plan 2's project writer guard until its terminal event. Cancellation plus an in-process arbiter cannot prevent another process from acquiring the lock if closing the workspace drops the only guard while a blocking worker is still unwinding.

### What already exists

- `ReviewedGuideExportJob`, `ReviewedStepImage`, and `render_guide_folder` already define the reviewed core viewer boundary; this plan extends them for lazy project images instead of adding a second export model.
- `render_reviewed_storyboard`, `export_gif_images`, and `export_video` already provide output layout/encoding behavior and atomic single-file precedents; this plan makes the reviewed paths streaming and cancellable.
- `TimelineWorkspace`, `GuideExportState`, and operation IDs already demonstrate stale-result rejection; publish adds the saved revision and disk-commit guard.
- `guide_export.rs` already uses rustix `RenameFlags::NOREPLACE` plus an RAII temporary-directory guard; sharing follows the same no-overwrite pattern.
- `issue_pack.rs` already owns an outer temporary folder transaction and current reviewed job; Task 7 replaces eager frame collection while preserving its stable format.
- Plan 1 owns strict project DTOs, validated same-handle/no-follow asset access, typed `ProjectError`, and durable atomic JSON persistence; this plan reuses those primitives.
- Plan 2 owns `StepFrameSource`, project session/revision state, writer locking, Save completion, and the shared Linux/macOS Timeline hosts; this plan adds publishing to that lifecycle.
- Existing `StoryboardOptions::max_canvas_pixels` bounds the final storyboard canvas; the reviewed renderer keeps that limit.

### NOT in scope

- Cloud hosting, sync, collaboration, comments, or remotely updating previously shared copies — outputs remain local snapshots.
- Windows sharing/file integration and writable network-filesystem guarantees — v1 remains Linux/macOS on supported local filesystems.
- Importing legacy exports as editable projects — flattened artifacts lack private edit state.
- Background completion after Rollshot exits, resumable jobs, or a persistent worker service — reopen shows stale state and requires Retry.
- New output formats, codec settings UI, arbitrary publish themes, or configurable memory limits — the slice implements only Core, Storyboard, GIF, and MP4 with fixed safe bounds.
- Garbage collection of Plan 1 orphaned immutable assets or abandoned crash leftovers — loaders ignore them and sharing uses explicit allowlists.
- Release-feature enablement or a new build/publish pipeline — the existing non-default feature CI remains authoritative.

### Publish and share flows

```text
Save commits revision R
        │
        ▼
PublishArbiter::begin(op, R) ──▶ Task::run(progress stream) ──▶ spawn_blocking
        │                                                        │
        │                 render outside lock                    ├─ Core temp dir
        │                                                        ├─ Storyboard temp
        │                                                        ├─ GIF temp
        │                                                        └─ MP4 temp + ffmpeg
        │
        └─ commit guard: op current + project.json revision R
                              │
                              ├─ commit output
                              ├─ persist successful revision
                              └─ emit guarded progress / Finished
```

```text
Safe Copy / Issue Pack needs current outputs
        │ stale
        ▼
ShareGate renders complete temp publish tree
        │
        ▼ one guarded logical commit
old publish/state -> backups -> new publish -> new state -> fsync
        │                                      │
        │ success                              └─ failure: restore both backups
        ▼
handle-based, no-follow, chunked no-replace share copy

Editable Project skips regeneration but requires saved + clean + warning,
then copies only manifest-referenced assets and current safe publish files.
```

### Test coverage map

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / strict state, exact revision freshness, cancellation, durable rewrite | ✓ | ✓ | — | no |
| Task 2 / lazy project resolution, annotation equivalence, title fallback | ✓ | ✓ | — | no |
| Task 3 / bounded storyboard/GIF/video streaming, cancellation, ffmpeg kill | ✓ | ✓ | — | no |
| Task 4 / core/derivative/ShareGate transactions, arbitration, privacy | ✓ | ✓ | — | no |
| Task 5 / Save scheduling, late-result guards, aggregate/details UI | ✓ | ✓ | — | no |
| Task 6 / safe/editable allowlists, no-follow copy, gating, cancellation | ✓ | ✓ | — | no |
| Task 7 / lazy/current Issue Pack, no-overwrite, privacy boundary | ✓ | ✓ | — | no |
| Task 8 / crate suites, lint, shared Linux/macOS lifecycle | — | ✓ | ✓ | cross-platform runtime |

### Failure modes

| New codepath | Realistic production failure | Test / handling / user result |
|---|---|---|
| Publish-state load/write | malformed/newer JSON, missing output artifact, or fsync/rename failure | Task 1 Steps 1–4; unavailable/missing means stale, write returns `ProjectError`, prior bytes survive |
| Lazy project image | asset deleted/corrupt after job construction | Task 2 Steps 1–5; stable export category, affected output fails visibly |
| Storyboard/GIF buffers | computed output exceeds the fixed pixel ceiling | Task 3 Steps 1–6; typed too-large error, no destination committed |
| Video process | ffmpeg unavailable, exits, or stops reading stdin | Task 3 Steps 1–6; typed error/watchdog kill, temp removed, output failed visibly |
| Background core commit | render, swap, root sync, writer lease, or revision guard fails | Task 4 Steps 1–6; lease/backup restore/superseded outcome, old core remains or failure is shown |
| ShareGate logical commit | state rename fails after directory swap | Task 4 Steps 1–6; transaction restores old directory/state, share aborts visibly |
| Publish event delivery | old operation emits after a newer Save | Tasks 4–5; operation+revision checks reject disk and UI mutation |
| Safe/editable copy | symlink substitution, destination collision, or cancellation | Task 6 Steps 1–6; no-follow handle/no-replace/temp cleanup, clear failed/cancelled state |
| Issue Pack | stale derivative supplied or destination collides | Task 7 Steps 1–5; reject/rerender and no-replace error, existing pack preserved |
| Diagnostics | private title/path/annotation leaks into tracing | Tasks 4 and 6 tracing-capture tests; structural fields only |
| Platform lifecycle | close or Save supersedes active worker | Tasks 5–8; token cancellation plus guards; no silent late success |

No reviewed codepath has an untested, unhandled, silent failure.

### Task dependencies and execution strategy

| Task | Modules touched | Depends on |
|---|---|---|
| 1: publish state/cancellation | `crates/rollshot-action/src/project/` | Plans 1–2 |
| 2: lazy reviewed images | `crates/rollshot-action/src/export/`, `crates/rollshot-app/src/timeline_workspace/` | 1 |
| 3: derivative renderers | `crates/rollshot-action/` | 1, 2 |
| 4: publish transactions | `crates/rollshot-app/src/timeline_workspace/` | 1–3 |
| 5: Timeline publish lifecycle/UI | `crates/rollshot-app/src/timeline_workspace/` | 4 |
| 6: safe/editable sharing | `crates/rollshot-app/src/timeline_workspace/` | 4, 5 |
| 7: Issue Pack sharing | `crates/rollshot-app/src/issue_pack.rs`, `crates/rollshot-app/src/timeline_workspace/` | 2, 3, 6 |
| 8: docs/verification | `README.md`, all changed modules | 1–7 |

Sequential execution, no parallelization opportunity: every task consumes the preceding contract or shares the same Timeline/action modules. Repository policy also forbids worktrees unless explicitly requested. No task modifies root `Cargo.toml`.

---

### Task 1: Persist strict per-output publish freshness and cancellation

**Files:**

- Create: `crates/rollshot-action/src/project/publish.rs`
- Create: `crates/rollshot-action/tests/project_publish_state.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`
- Modify: `crates/rollshot-action/src/project/store.rs`

**Interfaces:**

- Consumes: Plan 1 project revision (`u64`), project-root validation, atomic JSON helpers, and privacy-safe `ProjectError`.
- Produces:
  - `PublishOutputKind::{Core, Storyboard, Gif, Mp4}`
  - `PublishedOutputV1 { last_successful_revision: u64 }`
  - `PublishStateV1 { schema_version, outputs }` containing durable successes only
  - `PublishFreshness::{Current, Stale}`; app runtime state adds Updating and Failed
  - `load_publish_state(root) -> PublishStateLoad`
  - `write_publish_state(root, &PublishStateV1) -> Result<(), ProjectError>`
  - `PublishCancellation::{new, cancel, is_cancelled, check}` and zero-sized `PublishCancelled`.

- [ ] **Step 1: Write failing state and cancellation tests**

Cover missing state, malformed JSON, unknown fields, unsupported schema versions, mixed current/stale successful revisions, a matching revision whose derivative is missing or symlinked, an incomplete core viewer tree, atomic rewrite, injected rename/sync failure preserving the prior complete bytes, and idempotent cancellation:

```rust
#[test]
fn corrupt_publish_state_is_non_fatal_and_all_stale() {
    let project = committed_project();
    std::fs::write(project.path().join("publish-state.json"), b"{").unwrap();

    let loaded = load_publish_state(project.path());
    assert!(matches!(loaded, PublishStateLoad::Unavailable));
    for kind in PublishOutputKind::ALL {
        assert_eq!(loaded.freshness(kind, 4), PublishFreshness::Stale);
    }
}

#[test]
fn freshness_requires_the_exact_saved_revision() {
    let project = committed_project();
    write_complete_core_bundle(project.path());
    write_state(project.path(), state_with_success(PublishOutputKind::Core, 3));

    let loaded = load_publish_state(project.path());
    assert_eq!(loaded.freshness(PublishOutputKind::Core, 3), PublishFreshness::Current);
    assert_eq!(loaded.freshness(PublishOutputKind::Core, 4), PublishFreshness::Stale);
}
```

- [ ] **Step 2: Run the focused tests and verify failure**

```bash
rtk cargo test -p rollshot-action --test project_publish_state
```

Expected: compile failure because publish-state APIs do not exist.

- [ ] **Step 3: Implement strict DTOs and derived freshness**

Use `#[serde(deny_unknown_fields)]`, an exact `schema_version: 1`, and a `BTreeMap<PublishOutputKind, PublishedOutputV1>`. This file records only the last successful revision for each output; Updating and Failed are runtime states owned by the app. `PublishStateLoad::Unavailable` carries no untrusted parse detail into UI.

`load_publish_state(root)` reconciles the durable revision markers with disk shape. Optional output markers count only when the stable path is a regular non-symlink file. Core counts only when `index.html`, `steps.md`, `session.json`, and every keyframe referenced by the parsed session manifest are regular non-symlink files under `publish/`; reject absolute, parent-traversing, non-`keyframes/*.png`, duplicate, or escaping references before checking them. Store the reconciled availability in `PublishStateLoad`, so `freshness(kind, saved_revision)` returns `Current` only when the marker exactly equals `saved_revision` and its artifact is present; it returns `Stale` otherwise. Sharing still reopens source handles with no-follow semantics at copy time because freshness checks are not a security lock.

The app overlays a current-operation failure to derive `Failed`. Generalize Plan 1's project-module-private JSON transaction helper in `store.rs` and use it for both manifest and publish-state writes: create a unique sibling, serialize/write, sync the open file, rename over the destination, and sync the project directory. Do not add a second durability implementation in `publish.rs`.

Implement `PublishCancellation` as a cloneable `Arc<AtomicBool>` whose `check()` returns `Result<(), PublishCancelled>`. Renderer and app boundaries map that zero-sized signal into their existing typed cancellation/error categories.

- [ ] **Step 4: Run tests**

```bash
rtk cargo test -p rollshot-action --test project_publish_state
```

Expected: state, corruption, revision, atomic rewrite, and cancellation tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/project/mod.rs crates/rollshot-action/src/project/publish.rs crates/rollshot-action/src/project/store.rs crates/rollshot-action/tests/project_publish_state.rs
rtk git commit -m "feat(action): track project publish freshness"
```

---

### Task 2: Resolve reviewed project images lazily

**Files:**

- Modify: `crates/rollshot-action/src/export/model.rs`
- Modify: `crates/rollshot-action/src/export/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/guide_export.rs`
- Test: `crates/rollshot-action/src/export/model.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/guide_export.rs`

**Interfaces:**

- Consumes: Plan 2 `StepFrameSource`, project asset descriptors, persisted annotations, and existing `ReviewedGuideExportJob`.
- Produces:
  - `ProjectReviewedImage { project_root, frame, annotations }`
  - `ReviewedStepImage::Project(ProjectReviewedImage)`
  - `ReviewedStepImage::dimensions() -> (u32, u32)` without decode
  - `ReviewedStepImage::with_flattened_image(cancel, callback)` with one-image lifetime
  - `build_reviewed_export_job(&TimelineWorkspace) -> Result<ReviewedGuideExportJob, ExportError>` without eager project decode.

- [ ] **Step 1: Write failing lazy-resolution tests**

Create a two-step project job, delete the second asset after job construction, and assert construction succeeds without decoding while resolving step 1 succeeds and resolving step 2 reports `asset_missing`. Add an annotated step and assert its resolved pixels equal the existing in-memory flattened path. Add an empty editable Guide title and assert the job uses the existing public `rollshot_action::DEFAULT_GUIDE_TITLE` while the project snapshot remains empty.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-action export::model::tests::project_reviewed_image
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::guide_export
```

Expected: compile failure because the project reviewed-image variant does not exist.

- [ ] **Step 3: Add the lazy project image variant**

Store only the project root, validated `ProjectFrame`, and current annotation vector. `dimensions()` returns manifest dimensions. `with_flattened_image` checks cancellation and calls Plan 1's same-handle, no-follow `decode_png_asset`, then rehydrates annotations through `ImageDocument::from_persisted_annotations`, flattens only when annotations exist, invokes the callback, and drops the image before returning.

Keep existing retained and already-annotated in-memory variants unchanged. Map errors to stable export categories and never include annotation text or full paths in tracing fields.

- [ ] **Step 4: Build lazy jobs in the workspace**

For `StepFrameSource::InMemory`, preserve the current shared-`Arc` path. For `StepFrameSource::Project`, create `ProjectReviewedImage` descriptors from the current step/keyframe mapping and current loaded-or-pending annotations. When the editable title trims to empty, set only the reviewed job title to `DEFAULT_GUIDE_TITLE`; never write that fallback back into the Guide or project snapshot. Do not consult the 256 MiB UI cache: export workers own their one-frame decode lifecycle and must not evict interactive frames.

- [ ] **Step 5: Run focused tests**

```bash
rtk cargo test -p rollshot-action export::model::tests::project_reviewed_image
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::guide_export
```

Expected: construction is lazy, resolution validates assets, annotations flatten identically, and one failing step does not decode unrelated steps.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/export/model.rs crates/rollshot-action/src/export/mod.rs crates/rollshot-app/src/timeline_workspace/guide_export.rs
rtk git commit -m "feat(action): resolve reviewed project images lazily"
```

---

### Task 3: Make derivative renderers bounded and cancellable

**Files:**

- Modify: `crates/rollshot-action/src/storyboard.rs`
- Modify: `crates/rollshot-action/src/gif.rs`
- Modify: `crates/rollshot-action/src/video.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: same files

**Interfaces:**

- Consumes: Task 1 cancellation and Task 2 lazy reviewed images.
- Produces:
  - `render_reviewed_storyboard_cancellable(job, options, cancel) -> Result<StoryboardRenderResult, StoryboardError>`
  - `export_reviewed_gif(job, options, cancel, destination) -> Result<(), GifError>`
  - `export_reviewed_video(job, options, ffmpeg, cancel, destination) -> Result<(), VideoError>`.

- [ ] **Step 1: Write failing cancellation and bounded-resolution tests**

Add a test-only reviewed image resolver that records concurrent live images and cancels after a chosen step. Assert each renderer observes at most one source frame alive, stops before later steps, and does not leave a destination file. Add oversized/tall descriptor cases that fail before allocating beyond the fixed derivative-frame pixel ceiling. For video, use the existing fake ffmpeg harness for both normal cancellation and a child that stops reading stdin; assert the cancellation watchdog terminates the child and removes its temporary output.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-action storyboard::tests::cancellable
rtk cargo test -p rollshot-action gif::tests::reviewed_streaming
rtk cargo test -p rollshot-action video::tests::reviewed_streaming
```

Expected: compile failure because cancellable streaming entry points do not exist.

- [ ] **Step 3: Render storyboard without retaining step images**

First compute layout from descriptor dimensions and text metrics. Allocate the final canvas once. Resolve, scale, draw, and release each step sequentially, checking cancellation before and after each resolution. Keep the current non-cancellable public wrapper by delegating to a never-cancelled token so existing standalone callers retain behavior.

Re-export the three reviewed cancellable entry points and their existing result/error types from `lib.rs`; the app must not reach into private renderer modules.

- [ ] **Step 4: Stream GIF frames directly**

Open a temporary sibling output and initialize the existing `image::codecs::gif::GifEncoder` on a buffered file. Resolve one reviewed image, enforce the checked output-pixel ceiling, scale/letterbox it into one frame buffer, encode and release it, and continue. Do not collect `Vec<RgbaImage>` or encode the entire GIF into a `Vec<u8>`, and do not add a direct `gif` dependency. Flush/sync and atomically rename only after every frame succeeds; cancellation drops the temporary guard.

- [ ] **Step 5: Stream video frames and terminate on cancellation**

Use descriptor dimensions to select checked output geometry before starting ffmpeg and reject geometry above the fixed derivative-frame pixel ceiling. Resolve one image, scale/letterbox into one reusable raw-frame buffer, write its repeats, and release it before the next image. Start a cancellation watchdog that can kill the child without waiting for stdin writes to return; disarm and join it on every terminal path. On cancel or write error, close stdin, kill, wait, join stderr/watchdog workers, and remove the temporary output. Rename only after a successful ffmpeg exit.

- [ ] **Step 6: Run focused and compatibility tests**

```bash
rtk cargo test -p rollshot-action storyboard
rtk cargo test -p rollshot-action gif
rtk cargo test -p rollshot-action video
```

Expected: new cancellation/streaming tests and existing export-output tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/lib.rs crates/rollshot-action/src/storyboard.rs crates/rollshot-action/src/gif.rs crates/rollshot-action/src/video.rs
rtk git commit -m "perf(action): stream project publish derivatives"
```

---

### Task 4: Publish a saved revision transactionally in the background

**Files:**

- Create: `crates/rollshot-app/src/timeline_workspace/project_publish.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/project_publish.rs`

**Interfaces:**

- Consumes: Plan 2 committed snapshots, existing reviewed core renderer, Tasks 1–3 publish contracts.
- Produces:
  - `PublishSettings { storyboard, gif, mp4 }`
  - `PublishSelection::{AllEnabled, Only(BTreeSet<PublishOutputKind>)}`
  - `PublishPurpose::{Background, ShareGate}`
  - `PublishRequest { operation_id, revision, project_root, writer_lease, job, settings, selection, purpose, ffmpeg }`, where every mutating request owns an `Arc` lease over Plan 2's writer guard
  - `PublishEvent::{CoreCommitted, OutputCommitted, OutputFailed, Finished}` carrying operation/revision
  - `PublishArbiter` shared by the workspace and workers, with `begin`, `clear_if_current`, and guarded commit closure
  - `run_publish(PublishRequest, PublishCancellation, sender)` blocking worker, where `sender` is the bounded progress side of an iced stream task
  - `commit_publish_file(temp, destination)` and `swap_publish_directory(temp, publish)` transaction helpers.

- [ ] **Step 1: Write failing transaction tests**

Use temporary projects and an injected renderer seam to assert:

- a core render failure leaves the previous `publish/` byte-for-byte unchanged;
- rendered redactions are flattened and no core HTML/JSON/keyframe contains the project root or an `assets/frames` reference;
- successful core swap removes old optional derivatives before current ones exist;
- one optional failure does not prevent later enabled outputs;
- every event carries the requested operation and revision;
- superseding operation B before A's commit prevents A from swapping any core/file/state output;
- changing `project.json` to a newer revision before commit prevents the older worker from committing;
- closing/dropping the workspace's guard while a worker unwinds does not release the cross-process writer lock until that worker reaches a terminal result;
- cancellation cleans sibling temporary entries;
- ShareGate cancellation or one derivative failure leaves prior `publish/` and `publish-state.json` byte-for-byte unchanged;
- ShareGate publish-state rename/sync failure after the directory swap restores both prior `publish/` and prior `publish-state.json` byte-for-byte;
- the resulting state advances only successfully committed outputs to the current revision and preserves older last-successful revisions as stale.

Capture tracing for representative render/commit failures and assert it contains only operation, revision, output kind, and error category — never Guide/annotation text, image bytes, or title-bearing full paths.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::project_publish
```

Expected: compile failure because the orchestrator does not exist.

- [ ] **Step 3: Implement commit-time arbitration**

`PublishArbiter` is an `Arc<Mutex<Option<(operation_id, revision)>>>`. Starting or superseding an operation updates that tuple before the worker is dispatched. Immediately before any core directory, derivative file, or publish-state rename, the worker locks the arbiter, requires the exact tuple, re-reads `project.json`, requires its committed revision to equal the request, performs the rename while still holding the short-lived arbiter lock, and then releases it. Rendering and encoding never hold the lock. A mismatch returns a superseded outcome and removes the temporary artifact.

The advisory project writer guard remains the cross-process ownership boundary: wrap Plan 2's guard in an `Arc`, clone that writer lease into every background or ShareGate request, and release it only when the worker terminates. The arbiter closes in-process worker races. Tests pause A between rendering and commit, begin B, then prove A cannot alter disk; a separate lock attempt also remains blocked after the workspace closes until A exits.

- [ ] **Step 4: Implement the core directory transaction**

Create a sibling directory named from a random transaction token, render `index.html`, current guide JSON, static assets, and keyframes into it, sync files/directories, then:

1. rename existing `publish/` to a sibling backup when present;
2. rename the fully synced temporary directory to `publish/`;
3. sync the project root;
4. remove the backup only after the new directory is committed;
5. if step 2 or 3 fails, restore the backup before returning failure.

Do not copy old derivatives into the new core directory. This makes stale artifacts impossible to mistake for current output.

- [ ] **Step 5: Commit optional files independently**

For background `AllEnabled`, render core and then enabled outputs in fixed order: storyboard, GIF, MP4. For background `Only`, require Core to be current before retrying selected derivatives; if Core is selected or stale, render it first and remove now-stale derivatives through the directory swap. Each renderer targets a unique sibling temporary path inside `publish/`; sync and atomically rename it to its stable filename only on success. Persist `publish-state.json` after core and after each optional success so a process exit cannot falsely report an unfinished output as current. A failure is emitted to runtime UI but does not overwrite the file's older last-successful revision. Continue after an optional background failure unless cancellation is set.

For `ShareGate`, ignore incremental commit: render core and every enabled derivative inside the new temporary publish directory and prepare a fully synced temporary state file, then check cancellation and enter commit arbitration exactly once. Hold that short arbiter guard across the logical directory+state transaction. Rename any old publish directory and old state to unique backups, install the new directory and state, and sync the project root. If either install or the final sync returns an ordinary error, move the new artifacts aside/remove them, restore both backups, and sync the restored root before returning failure. Remove both backups only after the new pair is durable. Do not observe cancellation after entering the commit section; a concurrent late Cancel loses to a completed commit and is reported as success, while every observed cancellation leaves old disk state untouched. An uncatchable process/power loss between the two installs may expose mismatched content/state; startup treats any missing/corrupt state as stale, and Task 1 validation plus revision checks must never treat a mismatched revision as current.

- [ ] **Step 6: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::project_publish
```

Expected: all transaction, cleanup, ordering, and independent-failure tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/project.rs crates/rollshot-app/src/timeline_workspace/project_publish.rs
rtk git commit -m "feat(app): publish saved guide revisions"
```

---

### Task 5: Wire publish status, settings, and revision guards into Timeline

**Files:**

- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**

- Consumes: Plan 2 Save completion and Task 4 events.
- Produces:
  - aggregate header states `Publishing`, `NeedsAttention`, and `Published`, derived exactly as the authoritative spec defines
  - `Message::{TogglePublishOutput, OpenPublishDetails, RetryPublishOutput, RetryAllPublishOutputs, CancelPublish, PublishEvent}`
  - exactly one active `PublishOperation { id, revision, cancel, per_output }` plus one shared `PublishArbiter`.

- [ ] **Step 1: Write failing state-machine tests**

Assert that Save revision 7 schedules operation A for revision 7; another edit makes the affected per-output display stale without cancelling A and yields aggregate `NeedsAttention`; Save revision 8 cancels A and schedules B; late A events do not change state; closing cancels B; reopening stale output does not auto-publish; Retry targets one failed output; Retry All targets every stale/failed enabled output; any updating output yields `Publishing`; mixed current/failed or current/stale outputs yield `NeedsAttention`; and all enabled outputs current yields `Published`.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update::tests::publish
```

Expected: compile failure because publish state/messages are absent.

- [ ] **Step 3: Schedule only after successful Save**

On `SaveCompleted`, first install the returned root/revision and mark the project clean. Build the reviewed job from that committed state, cancel any prior operation, allocate a monotonically increasing operation ID, call `PublishArbiter::begin` before dispatch, and dispatch `AllEnabled` through `iced::Task::run(iced::stream::channel(...))`; the stream future owns one `spawn_blocking` worker and maps each bounded-channel progress item to `Message::PublishEvent`. Size the channel above the maximum event count for four outputs plus `Finished`, treat an unexpected send failure as worker shutdown, and guarantee one terminal `Finished` when the UI still owns the task. A Save failure schedules nothing and leaves the previous publish status unchanged. Opening a project only loads freshness; it never auto-starts expensive publishing. Retry and Retry All call the same dispatcher against the already saved, clean revision with an `Only` selection.

- [ ] **Step 4: Guard every result**

Accept a `PublishEvent` only when both IDs equal the active operation and its revision still equals the workspace saved revision. Apply per-output statuses from disk-compatible categories, recompute the aggregate, and clear the active operation plus matching arbiter entry only on matching `Finished`. Cancellation is not shown as a failure when caused by a superseding Save or close.

- [ ] **Step 5: Add Publish Details UI**

Keep the header compact: one aggregate label and a details affordance. The details panel lists Core, Storyboard, GIF, and MP4 with current/stale/updating/failed state; Core has no toggle. Changing an optional toggle marks dirty and explains that Save applies the change. Show per-output Retry and Retry All only for a saved, clean, writable project, and Cancel only while an operation is active. Remove project-mode standalone export actions that could imply an untracked second truth; keep Preview and Copy Storyboard as non-authoritative convenience actions.

- [ ] **Step 6: Run state and view tests**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update::tests::publish
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::view::tests::publish
```

Expected: scheduling, supersession, aggregate state, toggle-dirty, and action-availability tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): show project publish lifecycle"
```

---

### Task 6: Add safe and editable project sharing

**Files:**

- Create: `crates/rollshot-app/src/timeline_workspace/share.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/share.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**

- Consumes: current saved revision, publish freshness, existing directory picker/clipboard helpers, and Task 1 cancellation.
- Produces:
  - `ShareKind::{SafeCopy, EditableProject}`
  - `ShareRequest { operation_id, revision, source_root, destination, kind }`
  - `copy_safe_publish(...)` and `copy_editable_project(...)`
  - visible `ShareProgress::{WaitingForPublish, Copying, Complete, Failed, Cancelled}`.

- [ ] **Step 1: Write failing sharing-boundary tests**

Build a project containing assets, publish files, `.lock`, `*.tmp`, a transaction backup, and an external symlink. Assert Safe Copy contains only the current `publish/` tree, Editable Project contains `project.json`, referenced assets, `publish/`, and `publish-state.json`, and neither mode copies lock/temp/backup/symlink targets. Add a deterministic substitution hook proving that replacing an allowlisted path with a symlink cannot redirect a copy after validation. Assert a pre-existing destination is never overwritten, cancellation during a large file removes the partial sibling promptly, and diagnostics contain no private text/full paths.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::share
```

Expected: compile failure because share workers do not exist.

- [ ] **Step 3: Implement no-overwrite directory copies**

Create a random sibling temporary directory beside the chosen destination. Walk only an explicit allowlist, open each source as a regular file with no-follow semantics using the validated directory/asset-handle pattern from Plan 1, and copy from that bound handle in fixed-size chunks with cancellation checks; never check a path and later reopen it through `std::fs::copy`. Sync copied files/directories and commit the destination with rustix no-replace semantics. Safe Copy allowlists the core viewer plus current successful enabled derivatives. Editable Project allowlists `project.json`, every asset referenced by that manifest, `publish-state.json` when present, and the safe current `publish/` contents; it never recursively copies arbitrary project-root entries.

- [ ] **Step 4: Gate sharing in the state machine**

Safe Copy on a stale/missing/failed required output visibly enters `WaitingForPublish`, dispatches Task 4 `ShareGate` regeneration for the current saved revision, and continues only after every enabled output is current. A cancelled or failed gate aborts the share and preserves the old publish directory/state. In read-only mode it may copy already-current output but must not regenerate; stale Safe Copy is disabled with an explanation. Editable Project is disabled while unsaved or dirty, shows the approved editable-content warning before the picker, and never auto-saves. A new Save or close cancels an outstanding share whose revision is no longer current.

- [ ] **Step 5: Add three unambiguous sharing actions**

In Publish Details, label the project actions `Share safe viewer copy` and `Share editable project`, with short explanatory text. Preserve Issue Pack as a third separate action implemented in Task 7. Picker cancellation returns to the details panel without an error toast.

- [ ] **Step 6: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::share
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update::tests::share
```

Expected: allowlists, no-overwrite behavior, regeneration gating, warnings, cancellation, and picker-cancel behavior pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/share.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): share safe and editable guide projects"
```

---

### Task 7: Regenerate and share Issue Packs from the current revision

**Files:**

- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/guide_export.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/share.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/share.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**

- Consumes: Task 2 lazy `ReviewedGuideExportJob`, Tasks 3–6 publish/share cancellation and current-revision gates, existing Issue Pack manifest/output contract.
- Produces: `prepare_issue_pack_from_reviewed_job(job, current_publish, options, cancel)` without an eager frame vector.

- [ ] **Step 1: Write failing lazy/current-revision Issue Pack tests**

Assert an Issue Pack with storyboard/GIF enabled resolves one source frame at a time, includes only successful derivative files for the requested revision, rejects stale supplied publish files, cancels cleanly, never copies `.rollshot-guide/assets/` or `project.json` into the pack, and never deletes or overwrites a pre-existing destination on collision.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack::tests::reviewed_project
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::share::tests::issue_pack
```

Expected: compile failure because Issue Pack preparation still expects eager retained images.

- [ ] **Step 3: Reuse lazy reviewed renderers**

Replace eager GIF/storyboard frame collection with the Task 3 cancellable reviewed renderers. Keep the existing outer Issue Pack temporary transaction and stable output names, but replace its current delete-then-swap commit with the same no-replace destination primitive used by other shares. When a current published derivative already satisfies the requested option, copy that verified allowlisted file; otherwise render from the same revision-bound reviewed job. Check cancellation between every output stage.

- [ ] **Step 4: Gate and expose Issue Pack sharing**

Require a saved, clean project. If required current-revision publish material is absent, enter the same visible regeneration path as Safe Copy. Present `Create Issue Pack` separately from viewer/project sharing, keep its existing options, and bind worker results to operation/revision before showing success.

- [ ] **Step 5: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::share
```

Expected: existing Issue Pack compatibility tests plus lazy, freshness, privacy-boundary, and cancellation tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs crates/rollshot-app/src/timeline_workspace/guide_export.rs crates/rollshot-app/src/timeline_workspace/share.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(action-guide): publish current revision issue packs"
```

---

### Task 8: Document and verify the full publishing slice

**Files:**

- Modify: `README.md`
- Verify: all files changed by this plan

- [ ] **Step 1: Update user-facing documentation**

Document that Save commits editable content before derived publishing, the meaning of aggregate/per-output states, optional output toggles, cancellation/retry behavior, and the content/privacy distinction among Safe Viewer Copy, Editable Project, and Issue Pack. State that writable projects are supported only on local filesystems in v1 and MP4 requires the existing ffmpeg availability path.

- [ ] **Step 2: Run formatting**

```bash
rtk cargo fmt --all --check
```

Expected: no formatting diff.

- [ ] **Step 3: Run focused crate suites**

```bash
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide
```

Expected: all Action Guide project, publishing, renderer, sharing, and legacy standalone-export tests pass.

- [ ] **Step 4: Run lint gates**

```bash
rtk cargo clippy -p rollshot-action --all-targets -- -D warnings
rtk cargo clippy -p rollshot-app --all-targets --features action-guide -- -D warnings
```

Expected: no warnings.

- [ ] **Step 5: Run the workspace regression gate**

```bash
rtk cargo test
```

Expected: the workspace default-member suite passes. OCR remains in its existing dedicated lane and is not enabled by this feature.

- [ ] **Step 6: Perform Linux and macOS lifecycle inspection**

Verify from code and platform-specific tests that Linux Home/Timeline and macOS Home/Record/Timeline dispatch the same Save-to-publish/share messages, close cancels active workers, and neither platform creates a second iced daemon. On each available platform, exercise Safe Copy, its cancellable regeneration path, and the mandatory Editable Project warning. Record any unavailable platform runtime check explicitly in the implementation handoff rather than implying it ran.

- [ ] **Step 7: Commit**

```bash
rtk git add README.md
rtk git commit -m "docs(action-guide): explain project publishing and sharing"
```

---

## Completion Gate

This plan is complete only when:

- Save success remains independent from publish results and schedules exactly the committed revision.
- Core publication is all-or-old; optional outputs have independent current/stale/failed status.
- late, superseded, and cancelled worker results cannot mutate visible state.
- project-backed exports decode no more than one source frame at a time.
- Safe Copy, Editable Project, and Issue Pack have distinct UI, allowlists, and privacy behavior.
- missing/corrupt publish state is stale rather than fatal.
- temporary artifacts are cleaned after failure/cancellation and destinations are never silently overwritten.
- focused tests, `cargo fmt --check`, both clippy commands, and the workspace default-member suite pass.

After this gate, invoke `superpowers:requesting-code-review` before branch integration.
