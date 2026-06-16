# Action Guide P0c — Spec Delta (App Integration)

Status: Approved delta
Date: 2026-06-16
Supersedes (where it conflicts): the increment boundaries and Architecture
assumptions of `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

## Why this delta exists

The 2026-06-15 design spec was written **before** the capture-workflow-scope
refactor (commit `7a48d27`, PR #45) landed. It assumed:

- The `rollshot-app` toolbar entry, recording controls, Timeline Workspace, and
  export would all ship inside **P0a**.
- `Workflow::ActionGuide` could be added as a "natural third peer" with no
  friction.
- The frame stream feeds `rollshot-action` directly (Architecture §, line 270).

What actually shipped (PRs #46 P0a, #47 P0b): the platform-neutral
`rollshot-action` engine, the `rollshot-linux-input` / `rollshot-macos-input`
crates, the `action_input.rs` seam, and a **3-second input-capability probe**
behind `rollshot action-guide`. None of the in-app record → review → export
flow exists yet. This delta records the four design decisions that the
post-refactor codebase forces, so the original spec stays a frozen snapshot
(AGENTS.md §11) and the P0c plans have an authoritative reference.

The original spec's **product behavior** (toolbar entry UX, recording controls,
Recording State And Warning, Timeline Workspace layout and operations, export
folder structure, session lifecycle state machine) is unchanged and remains the
source of truth for *what the feature does*. This delta only resolves *how it
wires into current code*.

## Verified-current integration seams (as of commit `8867135`)

- `Workflow` (`rollshot-capture/src/types.rs:43`) = `Screenshot | Scrolling`,
  `#[derive(Copy, Default, Serialize, Deserialize)]`, serialized inside
  `CaptureRequest`. `is_supported()` (`types.rs:90`) currently only excludes
  `Scrolling × Fullscreen`.
- Toolbar is a workflow-switcher: `ToolbarAction::{RegionMode, ScrollingMode}`
  send `activate_workflow(Workflow) -> WorkspaceEffect::ActivateWorkflow`
  (`workspace.rs:81`). `WorkspacePhase` = `Selecting | Selected |
  ScrollingCapture` (`workspace.rs`). `OverlayEffect` (`app.rs:40`) =
  `None | BeginStitch | FinishScrolling | FinishRegion | Cancel |
  EnablePassthrough | DisablePassthrough | ActivateWorkflow`.
- Frames arrive as `rollshot_capture::CapturedFrame { image: RgbaImage,
  timestamp: SystemTime, metadata }`. The single per-frame consumption seam is
  the stitch thread in `driver.rs` (~313–382), which today crops each frame and
  calls `process_frame(...)` into the stitcher.
- `rollshot-action` engine API is ready and unchanged:
  `ActionRecorder::new(region, StoreConfig, DetectorConfig)` →
  `ingest_frame(RgbaImage, Millis)` / `ingest_event(TimedSemanticAction)` →
  `finish() -> Recording { candidates: Vec<CandidateStep>, store: FrameStore }`;
  `Guide::from_candidates(candidates)` → `steps()/rename/delete/replace_keyframe`;
  `export_guide(&guide, &store, region, capability, source, out_dir) ->
  Result<PathBuf, ExportError>`.
- `action_input.rs` seam: `create_input_source() -> Box<dyn SemanticInputSource>`,
  `ActionInputSession::{new, start(region) -> InputCapability, poll_into(&mut
  ActionRecorder), stop}`, `degraded_advisory(DegradedReason) -> &'static str`.
- Handoff today: **Linux** `run_overlay(config) -> Result<Option<CaptureResult>,
  OverlayError>` blocks, returns via `RESULT_SLOT` + `iced::exit()`; `main.rs`
  then boots a *separate* `result_workspace::run()` iced app. **macOS** runs one
  `iced::daemon` (`macos_product.rs`) with `Phase::{Capture(Component),
  Thumbnail, Workspace}` and a `HostEffect::{Completed(CaptureResult),
  Cancelled, Fatal, Task, None}` protocol; the image stays in memory.

## Decision 1 — Driver decoupling (stream without stitching)

The `Driver` couples the capture stream with the stitcher. Action Guide needs
the stream but must route frames to `ActionRecorder`, not the stitcher.

**Resolution.** Reuse the Driver's existing reader thread (it already fills
`shared.latest`). Add a parallel consumer path:

- `Driver::begin_action_recording(region: CaptureRegion)` spawns an **action
  consumer thread** (mirroring the stitch thread structure) that owns an
  `ActionRecorder` and an `ActionInputSession`. Each loop iteration: pull the
  newest frame from `shared.latest`, convert `CapturedFrame.timestamp`
  (`SystemTime`) to session-relative `Millis`, call
  `recorder.ingest_frame(frame.image, at_ms)`, then `session.poll_into(&mut
  recorder)`. The stitcher is never constructed for this path.
- `Driver::finalize_action(self) -> Recording` stops the input session and
  returns `recorder.finish()`.

Timestamp conversion uses a session-start reference captured when recording
begins (first frame's `SystemTime`), so `Millis` is monotonic from 0 — matching
`rollshot-action`'s contract. The detector/recorder remain on one thread; no
cross-thread `ActionRecorder` sharing.

## Decision 2 — `Workflow::ActionGuide` variant ripple

**Resolution.** Add `Workflow::ActionGuide` (per the original spec's third-peer
model) and accept the exhaustive-match ripple. Concretely:

- `rollshot-capture/src/types.rs`: add the variant; `is_supported()` allows only
  `ActionGuide × Region` (reject `ActionGuide × Fullscreen`); add
  `CaptureRequest::action_guide_region()`. `needs_overlay()` is already correct
  (Region ⇒ true).
- Update every exhaustive `match self.workflow` / `match active_workflow`:
  `linux_runner.rs`, `macos_capture.rs`, `fullscreen.rs`, `toolbar.rs`,
  `workspace.rs`, `app.rs`. `fullscreen.rs` rejects `ActionGuide` (region-only).
- The serde rename is `action-guide` (kebab-case, matching existing derive).

## Decision 3 — Recorder ownership and cross-platform `Recording` handoff

Because the **Linux** overlay runs as a *blocking* `run_overlay` call, the app
thread is blocked for the whole recording; frames cannot stream to the app
during recording. Therefore the `ActionRecorder` must live **inside the overlay
layer**, next to the frame stream (Decision 1's consumer thread).

**Resolution.**

- `rollshot-iced-overlay` gains a **feature-gated** (`action-guide`) dependency
  on `rollshot-action` (and, transitively through the app, the platform input
  crates via the existing `action_input.rs` seam — which moves no further; the
  overlay calls into the same `SemanticInputSource` trait objects). The overlay
  produces the finished `Recording`.
- **Linux:** add `run_action_guide(config) -> Result<Option<Recording>,
  OverlayError>` alongside `run_overlay`, returning the `Recording` via a typed
  result slot (mirroring `RESULT_SLOT`). `main.rs` receives it.
- **macOS:** extend the capture `Component` with an action-recording mode and a
  new `HostEffect::ActionRecorded(Recording)` (or carry it through the existing
  completion channel as a typed variant). The daemon keeps the `Recording` in
  memory for the Timeline phase.
- `Recording` (which owns a `FrameStore` full of `RgbaImage`s) is moved, never
  cloned, across the handoff.

This keeps the overlay "capture-only" boundary intact in spirit: it captures and
returns a finished artifact (image, or `Recording`), and the app owns review and
export.

## Decision 4 — P0c split and per-plan Definition of Done

P0c lands as two independently shippable plans:

- **P0c-1 — Recording lifecycle.** `Workflow::ActionGuide`, the 🎬 toolbar
  entry, region → `Start Recording` → `WorkspacePhase::Recording` controls
  (elapsed time, capability label, amber advisory, `Finish`/`Cancel`) → detection
  → a `Recording`, on **both** platforms. To be shippable without the review UI,
  `Finish` hands the `Recording` to a thin handler that builds a `Guide` via
  `Guide::from_candidates` and calls `export_guide(...)` to a default
  timestamped output directory, logging the path. **DoD:** record a short
  workflow from the overlay and get an `action-guide/` folder with `steps.md` +
  keyframes; headless tests assert the consumer thread produces candidates from
  fixture frames; both platform build/clippy/test pass.
- **P0c-2 — Timeline Workspace + export.** A sibling `timeline_workspace/`
  module mirroring `result_workspace/`'s Elm structure, inserted between
  detection and export: select/rename/delete a step, replace a keyframe from the
  nearby strip, discard, and choose the output directory at export. Replaces
  P0c-1's direct-export handler. **DoD:** the original spec's Timeline Workspace
  operations and export behavior, on both platforms.

## Out of scope (unchanged from original spec's Deferred Work)

Merge/split editing, full-session scrubber, manual Add Step, free-form Markdown
editing, GIF/HTML/MP4/WebM, OCR/a11y/LLM, global hotkey, cross-platform absolute
pointer position. P0c-1 also defers the output-directory **picker** (uses a
default dir) and all review editing to P0c-2.
