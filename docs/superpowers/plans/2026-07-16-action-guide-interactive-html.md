# Action Guide Interactive HTML Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a deterministic, offline `index.html` reader to standalone and Issue Pack Action Guide folders while preserving reviewed pixels, bounded export memory, and the editable Timeline Workspace.

**Architecture:** `rollshot-app` freezes Timeline `Guide`, retained frames, and committed presentation into one owned `ReviewedGuideExportJob`. `rollshot-action` validates and renders that job sequentially into PNG, Markdown, JSON, and an inline HTML/CSS/JavaScript reader; standalone naming and Issue Pack transactions remain caller-owned. Shared `Arc<RgbaImage>` sources plus history-free annotation snapshots allow background rendering without cloning every full-resolution step.

**Tech Stack:** Rust, `image`, `serde`/`serde_json`, `rollshot-image-document`, iced 0.14 `Task`, Tokio `spawn_blocking`, `rustix` no-replace rename, inline HTML/CSS/JavaScript, Playwright Test.

## Global Constraints

- The approved source of truth is `docs/superpowers/specs/2026-07-15-action-guide-interactive-html-design.md`.
- Preserve `steps.md`, `session.json`, and `keyframes/*.png`; `index.html` is required for standalone and Issue Pack Action Guides.
- Export is deterministic and invokes no LLM, OCR, network, clipboard, or browser API.
- The exported reader uses no `fetch()`, server, remote asset, service worker, analytics, telemetry, or local storage.
- Only `TextNote` text and `NumberCallout` entries with non-empty Guide-specific explanations create hotspots.
- Every exported keyframe is flattened; no exported file contains pixels hidden by an opaque redaction.
- Search includes Guide title, step title, caption, and interactive explanation text; OCR search remains out of scope.
- Standalone exports never overwrite an existing directory and leave Timeline Workspace open after success.
- Export pixel and filesystem work runs off the iced update thread; added full-resolution bitmap memory must not grow with step count.
- Linux and macOS are both active product paths; Windows shell integration is out of scope.
- Runtime diagnostics use stable `rollshot::*` targets and structural fields only; never log Guide text, clipboard text, pixels, raw events, or paths containing the Guide title.
- Do not change GIF/MP4 semantics, capture backends, recording, semantic ingestion, or step detection.
- All local shell commands in this plan are prefixed with `rtk`; GitHub Actions commands are not, because `rtk` is not installed on hosted runners.

---

## File Structure

### New files

- `crates/rollshot-action/src/export/model.rs` — owned export job, reviewed step image, hotspot geometry, schema validation.
- `crates/rollshot-action/src/export/html.rs` — safe viewer-data serialization and template assembly.
- `crates/rollshot-action/src/export/viewer.html` — complete offline reader markup, styles, and behavior.
- `crates/rollshot-action/examples/export_html_fixture.rs` — deterministic browser-test fixture generator.
- `crates/rollshot-app/src/timeline_workspace/guide_export.rs` — Timeline presentation adapter, standalone naming/transaction, and background worker.
- `crates/rollshot-app/src/platform_actions.rs` — shared Linux/macOS open and reveal helpers.
- `scripts/html-guide-e2e/package.json` — pinned Playwright scripts and dependency.
- `scripts/html-guide-e2e/package-lock.json` — reproducible browser-test dependency lock.
- `scripts/html-guide-e2e/playwright.config.mjs` — `file://` Chromium/Firefox/WebKit configuration.
- `scripts/html-guide-e2e/guide.spec.mjs` — offline viewer behavior tests.

### Existing files changed

- `crates/rollshot-image-document/src/{document.rs,flatten.rs,lib.rs}` — shared-source construction and immutable flatten snapshot.
- `crates/rollshot-action/src/{frame_store.rs,guide.rs,export.rs,error.rs,lib.rs,gif.rs,storyboard.rs}` — shared retained frames, Guide title, renderer, schema v1, reviewed-job Storyboard adapter, and owned GIF frame adapter.
- `crates/rollshot-app/src/timeline_workspace/{mod.rs,annotation.rs,update.rs,view.rs}` — title/explanation editing and export lifecycle.
- `crates/rollshot-app/src/{main.rs,issue_pack.rs}` — shared platform module and common Action Guide job in Issue Packs.
- `crates/rollshot-app/src/result_workspace/{actions.rs,update.rs}` — move reveal behavior to shared platform actions.
- `.github/workflows/ci.yml` — browser-test job.
- `Cargo.toml`, `Cargo.lock`, and `crates/rollshot-app/Cargo.toml` — direct `rustix` dependency for atomic no-replace directory commits.
- `README.md` — offline Action Guide folder and opening instructions.

---

## Engineering Review — Auto Mode Double Check

### Step 0: Scope and complexity

The plan creates 10 files, changes two existing top-level crates without adding
a crate, and contains 8 implementation tasks. It does not cross the mandatory
decomposition threshold (>12 new files, >2 new top-level modules/crates, or >10
tasks). The scope remains appropriate for one feature branch. Playwright is the
only new test toolchain and is justified because the core deliverable is a
browser application opened through `file://`.

### Best-practice verification

- Rust's standard `rename` API explicitly replaces an existing destination,
  so it cannot implement this feature's no-clobber commit by itself:
  <https://doc.rust-lang.org/std/fs/fn.rename.html>.
- `rustix::fs::renameat_with` exposes `RenameFlags::NOREPLACE` and maps the
  operation to supported Linux and Apple primitives:
  <https://docs.rs/rustix/latest/rustix/fs/fn.renameat_with.html>.
- Tokio documents that started `spawn_blocking` work cannot be aborted, which
  is why v1 has stale-result suppression but no misleading Cancel action:
  <https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html>.

### Architecture and data flow

```text
Timeline editable state
  ├─ Guide metadata
  ├─ Arc<retained pixels>
  └─ committed annotation documents + explanation map
                 │ freeze once (operation_id)
                 ▼
        ReviewedGuideExportJob
          ├─ immutable text/geometry
          └─ Retained(Arc) | Annotated(FlattenSnapshot)
                 │ move to spawn_blocking
                 ▼
   ┌─────────────────────────────┐
   │ rollshot-action renderers   │
   │ one full-resolution step    │
   │ materialized at a time      │
   └──────────────┬──────────────┘
                  ├─ standalone temp sibling
                  │    └─ atomic NOREPLACE commit
                  └─ Issue Pack outer temp tree
                       ├─ required Action Guide folder
                       ├─ optional bounded Storyboard
                       └─ optional raw-keyframe GIF
```

The iced surface uses built-in inputs, buttons, containers, and `Task`
messages. No custom widget, Canvas, Shader, or custom overlay is required.

### Auto decision D1 — Make destination commit atomically no-clobber

**Context:** the draft checked `destination.exists()` and then called
`std::fs::rename`. Rust documents that `rename` replaces an existing
destination, so an external writer can win between those two calls. **ELI10:**
checking that a parking space is empty does not reserve it. **Stakes:** a rare
race could replace an empty directory despite the product promise.

**Recommendation:** remove the parent-directory lock from this flow and add a
direct `rustix` dependency. Commit the completed temp directory with
`renameat_with(CWD, temp, CWD, destination, RenameFlags::NOREPLACE)`.
On `EXIST`, choose the next numeric suffix and retry the same completed temp
directory; on `NOSYS`/`INVAL`, fail recoverably and retain no final output.
This maps to `renameat2(RENAME_NOREPLACE)` on Linux and
`renameatx_np(RENAME_EXCL)` on macOS without adding an unsafe boundary.

- Keep `fs4` plus precheck: low effort, but only cooperating Rollshot writers
  honor the lock and the final rename can still replace an external path.
- Reserve the final directory before rendering: portable no-clobber, but
  exposes a half-built final folder and weakens rollback.
- Atomic no-replace rename: small direct dependency, exact semantics on active
  platforms, with an honest unsupported-filesystem error.

**Effort (human / AI):** 0.5 day / 1–2 hours. **Completeness:** 95%; unusual
filesystems that reject the flag fail safely instead of falling back to an
unsafe rename. **Maintenance / net:** low / selected.

### Auto decision D2 — Freeze before the picker and correlate async results

**Context:** the draft entered `Exporting` only after the folder picker
returned and carried no operation identity. Duplicate picker requests and late
worker results could race newer UI state. **ELI10:** every export needs a claim
ticket, including while the folder window is open. **Stakes:** the wrong result
could overwrite the visible status or export edits made after the original
click.

**Recommendation:** build the immutable job when Export is clicked and store it
in `PickingDestination { operation_id, pending }`. Disable export controls in
both picking and exporting states. Picker and worker messages carry the same
monotonic ID; stale results are ignored. Picker cancellation returns to
`Idle` without files. Do not expose a worker Cancel action in v1:
`spawn_blocking` work cannot be aborted once running.

- Build after the picker: simpler state, but the snapshot is not tied to the
  user's click and duplicate pickers remain possible.
- Add only a busy boolean: blocks duplicates, but cannot reject stale results.
- Explicit state plus operation ID: modest reducer work, deterministic
  ownership and testable late-result behavior.

**Effort (human / AI):** 0.5–1 day / 2–3 hours. **Completeness:** 100% for the
v1 UI, which intentionally has no mid-export cancellation. **Maintenance /
net:** low / selected.

### Auto decision D3 — Move every Issue Pack derivative off the iced thread

**Context:** the draft moved folder/ZIP writing to `spawn_blocking` but still
called `render_timeline_storyboard` while preparing the request. That function
snapshots and renders all cards synchronously. **ELI10:** moving the delivery
truck does not help if the heavy boxes are still packed on the cashier's desk.
**Stakes:** long Guides can still freeze the Timeline UI and temporarily retain
one full-size flattened image per step.

**Recommendation:** remove `storyboard_image` from prepared app state. In the
Issue Pack worker, borrow the same `ReviewedGuideExportJob` sequentially:
render the required Guide first, then call a new
`render_reviewed_storyboard(&job, opts)` adapter that flattens and downsizes
one step at a time before retaining bounded cards. Preserve the existing
`max_canvas_pixels` cap and optional-warning semantics. GIF continues to use
raw shared retained frames so its semantics do not change.

- Keep pre-rendered Storyboard: least refactor, violates responsiveness and
  memory gates.
- Drop Storyboard from Issue Packs: smallest runtime path, user-visible
  regression outside this feature.
- Render from the reviewed job in the worker: medium change, preserves behavior
  and makes all exported visual artifacts observe one snapshot.

**Effort (human / AI):** 1 day / 3–5 hours. **Completeness:** 95%; downscaled
card memory is bounded by Storyboard limits, while full-resolution added memory
remains constant. **Maintenance / net:** medium-low / selected.

### Auto decision D4 — Preserve focus and keep popovers anchored

**Context:** `openHotspot` rerendered every hotspot, replacing the focused
button, and the keyboard guard treated all buttons as text-owning controls.
Popover coordinates were also calculated only when opened. **ELI10:** the
button vanished under a keyboard user's finger, while the note stopped
following the picture after zoom. **Stakes:** signature keyboard, zoom, and
annotation behavior would be visibly unreliable.

**Recommendation:** render hotspot buttons only when the step changes; opening
a hotspot updates existing `aria-expanded` attributes and popover content.
Reposition the popover on open, zoom, viewport scroll, and window resize.
Suppress global shortcuts only for text-entry/select/contenteditable controls,
not ordinary buttons. Keep the mobile below-image presentation.

- Full hotspot rerender: simple code, loses focus and stale anchoring.
- Move focus into the dialog: valid modal pattern, but this is a non-modal
  explanatory popover and adds unnecessary focus trapping.
- Preserve trigger nodes and update state in place: small change, expected
  keyboard behavior.

**Effort (human / AI):** 0.5 day / 1–2 hours. **Completeness:** 100% for the
specified viewer interactions. **Maintenance / net:** low / selected.

### Auto decision D5 — Make shell actions and export reducers injectable

**Context:** the coverage map promised command-spawn errors and async lifecycle
tests, but the draft directly spawned `open`/`xdg-open` and had no way to
simulate picker cancellation or stale completion. **ELI10:** a test needs a
fake door that can refuse to open. **Stakes:** recoverable errors could regress
while only happy paths pass.

**Recommendation:** separate pure platform command construction from a small
injectable spawn function; production uses `Command::spawn`, tests inject
success/failure. Keep export state transitions in pure reducers keyed by
operation ID. Add tests for copy success as well as denial, snapshot isolation,
picker cancellation, atomic collision retry, and stale results.

- Test only command strings: misses spawn failure behavior.
- Launch real desktop programs in tests: flaky and unsuitable for CI.
- Pure specs plus injected side-effect boundary: small seam, deterministic
  error coverage.

**Effort (human / AI):** 0.5 day / 1–2 hours. **Completeness:** 100% for planned
failure behavior. **Maintenance / net:** low / selected.

### Auto decision D6 — Keep one browser harness but close accessibility gaps

**Context:** Rust tests cannot validate `file://` browser behavior, but the
draft browser suite omitted real clipboard success, light theme, highlighted
matches, and focus preservation. **ELI10:** checking that a lamp turns off does
not prove it turns on. **Stakes:** CI could report green while core visible
feedback is wrong.

**Recommendation:** keep the single pinned Playwright package and three-engine
matrix, add assertions for both clipboard branches, light/dark modes,
`<mark>` output, hotspot trigger focus after open/Escape, and popover
realignment after zoom. Keep tests network-denying and fixture-generated.

- Rust/string tests only: no browser confidence.
- More browser packages per crate: duplicated tooling.
- One focused harness with complete signature cases: moderate CI cost, strong
  behavioral coverage.

**Effort (human / AI):** 0.5 day / 1–2 hours. **Completeness:** 95%; real Safari
and OS shell integration remain manual gates. **Maintenance / net:** medium /
selected.

### Auto decision D7 — Accept the shared-frame API change with a workspace gate

**Context:** making `RetainedFrame.image` an `Arc<RgbaImage>` is required to
freeze an owned background job without cloning every full-resolution frame.
The knowledge graph reports a high two-hop blast radius (81 affected files),
even though many consumers will continue to work through `Arc` deref.
**ELI10:** changing the handle on a shared toolbox can affect every room that
borrows it. **Stakes:** testing only Action Guide crates could miss a compile or
ownership regression in capture, app, or developer-tool consumers.

**Recommendation:** keep the `Arc` transition because it is the simplest
ownership model that satisfies immutable background export and bounded bitmap
memory. Before implementation, use the graph to enumerate direct
`RetainedFrame.image` consumers. Make only deref/`Arc::clone` adaptations,
never clone the underlying `RgbaImage`, and add a workspace-wide all-targets
check to Task 1 before later renderer work.

- Clone pixels into the export job: tiny API diff, memory grows with Guide
  length and violates the feasibility gate.
- Keep borrowed frames in the worker: avoids Arc changes, but cannot safely
  outlive mutable iced state.
- Shared frames plus workspace compile gate: broad mechanical compatibility
  check, correct ownership and bounded memory.

**Effort (human / AI):** 0.5–1 day / 2–4 hours. **Completeness:** 95%; platform
runtime checks remain in Task 8. **Maintenance / net:** low / selected.

## What already exists

- Deterministic Guide/step metadata and retained keyframes in
  `rollshot-action`.
- Timeline-owned committed `ImageDocument` annotation state and navigator
  ordering.
- Existing Action Guide Markdown/JSON/PNG export and Issue Pack outer
  folder/ZIP transactions.
- Annotation-aware Storyboard rendering and bounded Storyboard canvas options.
- Linux/macOS reveal behavior in Result Workspace.
- iced `Task` usage and an existing `spawn_blocking` Storyboard-copy
  precedent.

## NOT in scope

- Single-file HTML, OCR search, manual theme selection, hosted publishing, or
  editing an exported Guide.
- Changes to recording, input privacy, capture backends, step detection,
  GIF/MP4 semantics, or result-workspace annotations.
- Windows shell integration, a new unsafe isolation crate, or fallback to a
  clobbering rename on filesystems that reject atomic no-replace.
- A cancellable mid-render worker. The v1 worker is bounded and must finish;
  stale UI results are ignored.

## Failure modes

| Failure | Handling | User-visible result | Verification |
| --- | --- | --- | --- |
| Invalid/empty job or missing keyframe | validate before picker/write | recoverable export error | model + adapter tests |
| Picker cancelled | discard pending job and return `Idle` | no banner/no files | reducer test |
| Duplicate click or stale completion | disable controls; compare operation ID | current operation remains authoritative | reducer test |
| PNG/HTML/manifest write failure | temp guard removes only temp | export failed; prior output preserved | injected/tempdir tests |
| Final path appears during commit | atomic `NOREPLACE`; retry suffix | unique success or safe OS error | collision hook + concurrency test |
| Unsupported no-replace filesystem | do not fall back to `rename` | recoverable commit error | error mapping test |
| Worker panic | join error; guard unwinds and cleans temp | export failed; editor remains | async wrapper test |
| Issue Pack required Guide failure | outer transaction rolls back | Issue Pack export error | folder + ZIP tests |
| Optional Storyboard failure | retain existing warning semantics | pack succeeds with warning | Issue Pack test |
| Missing exported PNG | step-local error state | other text/steps remain usable | Playwright |
| Clipboard denied | selected manual-copy field | explicit Ctrl/Cmd+C instruction | Playwright |
| Shell spawn failure | retain successful export paths | action-specific error only | injected command test |

## Test coverage

| Layer | Unit/integration coverage | Browser/runtime coverage |
| --- | --- | --- |
| image document/frame store | Arc identity, immutable flatten snapshot, redaction pixels | — |
| export job adapter | hotspot eligibility/order, trimmed metadata, snapshot isolation | — |
| renderer/transaction | parity, escaping, rollback, atomic collision retry, deterministic artifacts | moved-folder `file://` |
| Timeline reducer | picking/exporting/success/failure, duplicate/stale IDs, no exit | Linux + macOS active path |
| Issue Pack | same job, worker Storyboard, folder/ZIP rollback and manifest | open moved/extracted pack |
| viewer | serialized contract | navigation, search/mark, hotspots/focus, zoom/anchor, keyboard, copy, themes, responsive, lazy images, no network |
| platform actions | pure command specs + injected spawn errors | real open/reveal on Linux/macOS |
| performance | one-step materialization review and bounded Storyboard options | 40-step large-image RSS sample in final verification |

## Task dependency and parallel lanes

```text
Task 1 pixel ownership
   └─ Task 2 export contract
        └─ Task 3 Timeline snapshot adapter
             ├─ Task 4 renderer ── Task 5 viewer/E2E
             └───────────────┬── Task 6 standalone lifecycle
                             └── Task 7 Issue Pack worker integration
Tasks 5 and 6 may proceed in parallel after Task 4's schema/renderer API lands.
Task 7 depends on Tasks 3–4 and its Storyboard adapter may proceed beside Task 5.
Task 8 depends on all implementation tasks.
```

---

### Task 1: Share retained pixels and freeze annotation rendering

**Files:**
- Modify: `crates/rollshot-image-document/src/document.rs`
- Modify: `crates/rollshot-image-document/src/flatten.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`
- Modify: `crates/rollshot-action/src/frame_store.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`

**Interfaces:**
- Consumes: existing `ImageDocument::{source, shared_source, annotations, flatten}` and `FrameStore::retained`.
- Produces: `ImageDocument::from_shared_source(Arc<RgbaImage>)`, `ImageDocument::flatten_snapshot() -> FlattenSnapshot`, `FlattenSnapshot::{dimensions, annotations, flatten}`, and `RetainedFrame.image: Arc<RgbaImage>`.

- [ ] **Step 1: Write failing shared-source and flatten-snapshot tests**

Add to `document.rs` tests:

```rust
#[test]
fn from_shared_source_and_flatten_snapshot_reuse_pixels_without_history() {
    let source = Arc::new(RgbaImage::from_pixel(8, 8, Rgba([9, 8, 7, 255])));
    let mut document = ImageDocument::from_shared_source(Arc::clone(&source));
    document
        .add_redaction(ImageRect::new(0.0, 0.0, 4.0, 4.0))
        .unwrap();

    let snapshot = document.flatten_snapshot();

    assert!(Arc::ptr_eq(&snapshot.shared_source(), &source));
    assert_eq!(snapshot.dimensions(), (8, 8));
    assert_eq!(snapshot.annotations(), document.annotations());
    assert_eq!(snapshot.flatten(), document.flatten());
    assert!(document.undo());
    assert_eq!(snapshot.annotations().len(), 1);
}
```

Add to `frame_store.rs` tests:

```rust
#[test]
fn retained_window_shares_ring_pixels() {
    let mut store = small_store();
    let id = store.ingest(RgbaImage::new(4, 4), 0);
    store.retain_window(id);

    let ring = &store.ring.back().unwrap().image;
    let retained = &store.retained(id).unwrap().image;
    assert!(Arc::ptr_eq(ring, retained));
}
```

- [ ] **Step 2: Run the focused tests and confirm the missing APIs/types fail**

Run:

```bash
rtk cargo test -p rollshot-image-document from_shared_source_and_flatten_snapshot_reuse_pixels_without_history
rtk cargo test -p rollshot-action retained_window_shares_ring_pixels
```

Expected: compilation fails because `from_shared_source`, `FlattenSnapshot`, and `Arc` frame storage do not exist.

- [ ] **Step 3: Implement the immutable flatten snapshot**

In `flatten.rs`, add:

```rust
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FlattenSnapshot {
    source: Arc<RgbaImage>,
    annotations: Vec<Annotation>,
}

impl FlattenSnapshot {
    pub(crate) fn new(source: Arc<RgbaImage>, annotations: Vec<Annotation>) -> Self {
        Self { source, annotations }
    }

    pub fn shared_source(&self) -> Arc<RgbaImage> {
        Arc::clone(&self.source)
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.source.dimensions()
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub fn flatten(&self) -> RgbaImage {
        flatten_onto(&self.source, &self.annotations)
    }
}
```

In `document.rs`, make `new` delegate to the shared constructor and add the snapshot method:

```rust
pub fn new(source: RgbaImage) -> Self {
    Self::from_shared_source(Arc::new(source))
}

pub fn from_shared_source(source: Arc<RgbaImage>) -> Self {
    Self {
        source,
        annotations: Vec::new(),
        next_number: 1,
        next_id: 1,
        state_id: 0,
        next_state_id: 0,
        undo_stack: VecDeque::new(),
        redo_stack: Vec::new(),
    }
}

pub fn flatten_snapshot(&self) -> crate::FlattenSnapshot {
    crate::FlattenSnapshot::new(self.shared_source(), self.annotations.clone())
}
```

Re-export `FlattenSnapshot` from `lib.rs`.

- [ ] **Step 4: Convert ring and retained frames to shared pixels**

In `frame_store.rs`, import `Arc`, change both image fields, and share the ingest allocation:

```rust
use std::sync::Arc;

struct RingFrame {
    id: FrameId,
    at_ms: Millis,
    image: Arc<RgbaImage>,
}

pub struct RetainedFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub image: Arc<RgbaImage>,
}

pub fn ingest(&mut self, image: RgbaImage, at_ms: Millis) -> FrameId {
    let id = self.next_id;
    self.next_id += 1;
    let luma = downsample_luma(&image, self.config.analysis_width);
    let image = Arc::new(image);
    self.ring.push_back(RingFrame { id, at_ms, image });
    if self.ring.len() > self.config.ring_capacity {
        self.ring.pop_front();
    }
    self.analysis.push_back(AnalysisFrame { id, at_ms, luma });
    if self.analysis.len() > self.config.analysis_capacity {
        self.analysis.pop_front();
        self.dropped += 1;
    }
    id
}
```

Inside `retain_window`, replace the image clone with `Arc::clone(&f.image)`. Update `ActionGuidePresentation::document_for_step` to call:

```rust
document: ImageDocument::from_shared_source(Arc::clone(&frame.image)),
```

Add `use std::sync::Arc;` there. Adjust compile errors at callers only with deref-safe borrowing (`frame.image.as_ref()`) or `Arc::clone`; do not clone underlying pixel buffers.

Before editing callers, use code-review-graph `get_impact_radius` and
`query_graph(importers_of)` for `frame_store.rs` to enumerate direct
consumers. Every caller change must be an ownership adaptation caused by the
`Arc` transition; do not refactor adjacent code.

- [ ] **Step 5: Run crate and feature tests**

Run:

```bash
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo check --workspace --exclude rollshot-ocr --all-targets --features rollshot-app/action-guide
rtk cargo fmt --all -- --check
```

Expected: all pass; no caller converts an `Arc<RgbaImage>` back into a full image clone except where an existing output API intentionally owns pixels.

- [ ] **Step 6: Commit the ownership foundation**

```bash
rtk git add crates/rollshot-image-document/src crates/rollshot-action/src/frame_store.rs crates/rollshot-app/src/timeline_workspace/annotation.rs
rtk git commit -m "refactor(action): share reviewed frame pixels"
```

---

### Task 2: Add Guide title and the owned export contract

**Files:**
- Create: `crates/rollshot-action/src/export/model.rs`
- Modify: `crates/rollshot-action/src/guide.rs`
- Modify: `crates/rollshot-action/src/export.rs`
- Modify: `crates/rollshot-action/src/error.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

**Interfaces:**
- Consumes: `FlattenSnapshot`, shared retained frames, existing semantic metadata.
- Produces: `Guide::{title,set_title,effective_title}`, `ReviewedGuideExportJob`, `ReviewedGuideStep`, `ReviewedStepImage`, `GuideHotspot`, `NormalizedRect`, `GUIDE_SCHEMA_VERSION`, and `ReviewedGuideExportJob::validate()`.

- [ ] **Step 1: Write failing Guide title tests**

Add to `guide.rs` tests:

```rust
#[test]
fn guide_title_is_editable_with_export_fallback() {
    let mut guide = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);
    assert_eq!(guide.title(), "Action Guide");
    guide.set_title("  Checkout failure  ".to_string());
    assert_eq!(guide.title(), "  Checkout failure  ");
    assert_eq!(guide.effective_title(), "Checkout failure");
    guide.set_title("   ".to_string());
    assert_eq!(guide.effective_title(), "Action Guide");
}
```

- [ ] **Step 2: Write failing export-job validation tests**

Create `export/model.rs` with a test module that constructs a one-step job and asserts:

```rust
fn one_step_job() -> ReviewedGuideExportJob {
    ReviewedGuideExportJob {
        title: "Checkout failure".into(),
        region: CaptureRegion { x: 0, y: 0, width: 8, height: 8 },
        input_source: InputSourceKind::VisualOnly,
        input_capability: InputCapability::VisualOnly {
            reason: DegradedReason::SourceStartFailed,
        },
        steps: vec![ReviewedGuideStep {
            index: 1,
            title: "Submit order".into(),
            caption: Some("Confirm the request".into()),
            kind: CandidateKind::Click,
            reason: DetectReason::VisualChange,
            at_ms: 100,
            image: ReviewedStepImage::Retained(Arc::new(RgbaImage::new(8, 8))),
            hotspots: vec![GuideHotspot {
                annotation_id: 1,
                bounds: NormalizedRect { x: 0.1, y: 0.1, width: 0.2, height: 0.2 },
                explanation: "Open settings".into(),
            }],
        }],
    }
}

#[test]
fn validation_rejects_non_finite_or_outside_hotspots() {
    let mut job = one_step_job();
    job.steps[0].hotspots.push(GuideHotspot {
        annotation_id: 7,
        bounds: NormalizedRect { x: f32::NAN, y: 0.0, width: 0.2, height: 0.2 },
        explanation: "Open settings".into(),
    });
    assert!(matches!(
        job.validate(),
        Err(ExportError::InvalidHotspot { step: 1, .. })
    ));

    job.steps[0].hotspots.pop();
    job.steps[0].hotspots[0].bounds =
        NormalizedRect { x: 1.1, y: 0.0, width: 0.2, height: 0.2 };
    assert!(job.validate().is_err());

    job.steps[0].hotspots[0].bounds =
        NormalizedRect { x: 0.9, y: 0.0, width: 0.2, height: 0.2 };
    assert!(job.validate().is_err());
}

#[test]
fn validation_rejects_empty_explanation_and_accepts_valid_job() {
    let mut job = one_step_job();
    job.steps[0].hotspots[0].explanation = "  ".into();
    assert!(matches!(job.validate(), Err(ExportError::InvalidHotspot { step: 1, .. })));
    job.steps[0].hotspots[0].explanation = "Open settings".into();
    assert!(job.validate().is_ok());
}
```

- [ ] **Step 3: Run tests and confirm failures**

```bash
rtk cargo test -p rollshot-action guide_title_is_editable_with_export_fallback
rtk cargo test -p rollshot-action validation_rejects_non_finite_or_outside_hotspots
```

Expected: compilation fails on the new title and export model APIs.

- [ ] **Step 4: Implement Guide title**

In `guide.rs`:

```rust
pub const DEFAULT_GUIDE_TITLE: &str = "Action Guide";

#[derive(Clone)]
pub struct Guide {
    title: String,
    steps: Vec<GuideStep>,
}

pub fn title(&self) -> &str { &self.title }

pub fn set_title(&mut self, title: String) { self.title = title; }

pub fn effective_title(&self) -> &str {
    let trimmed = self.title.trim();
    if trimmed.is_empty() { DEFAULT_GUIDE_TITLE } else { trimmed }
}
```

Initialize `title` to `DEFAULT_GUIDE_TITLE.to_string()` in `from_candidates` and preserve all existing step construction.

- [ ] **Step 5: Implement the owned export model and typed validation errors**

Use these exact public shapes in `export/model.rs`:

```rust
use std::sync::Arc;
use image::RgbaImage;
use rollshot_image_document::FlattenSnapshot;

pub const GUIDE_SCHEMA_VERSION: u32 = 1;

pub struct ReviewedGuideExportJob {
    pub title: String,
    pub region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub steps: Vec<ReviewedGuideStep>,
}

pub struct ReviewedGuideStep {
    pub index: usize,
    pub title: String,
    pub caption: Option<String>,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub image: ReviewedStepImage,
    pub hotspots: Vec<GuideHotspot>,
}

pub enum ReviewedStepImage {
    Retained(Arc<RgbaImage>),
    Annotated(FlattenSnapshot),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideHotspot {
    pub annotation_id: u64,
    pub bounds: NormalizedRect,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
```

`ReviewedStepImage` must expose `dimensions()` and a crate-private `with_flattened_image` callback that borrows retained pixels directly and owns only one annotated flatten result at a time:

```rust
pub fn dimensions(&self) -> (u32, u32) {
    match self {
        Self::Retained(image) => image.dimensions(),
        Self::Annotated(snapshot) => snapshot.dimensions(),
    }
}

pub(crate) fn with_flattened_image<T>(
    &self,
    use_image: impl FnOnce(&RgbaImage) -> Result<T, ExportError>,
) -> Result<T, ExportError> {
    match self {
        Self::Retained(image) => use_image(image),
        Self::Annotated(snapshot) => {
            let flattened = snapshot.flatten();
            use_image(&flattened)
        }
    }
}
```

Add `ExportError::{MissingKeyframe { index }, InvalidHotspot { step, category }, DestinationExists { path }}`. Validation requires an already-normalized non-empty title, at least one step, 1-based contiguous indexes, and non-empty trimmed hotspot explanations; it checks finite positive hotspot rectangles are fully contained in `[0,1] × [0,1]`, including their right/bottom edges. Error categories are static strings such as `non_finite`, `empty_text`, and `outside_image`.

- [ ] **Step 6: Export the model and run tests**

Declare `mod model;` from `export.rs` and re-export all Task 2 public types from `lib.rs` together with `DEFAULT_GUIDE_TITLE`.

```bash
rtk cargo test -p rollshot-action
rtk cargo fmt --all -- --check
rtk cargo clippy -p rollshot-action --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 7: Commit the export contract**

```bash
rtk git add crates/rollshot-action/src
rtk git commit -m "feat(action): define reviewed guide export job"
```

---

### Task 3: Freeze Timeline presentation into the export job

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/guide_export.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`

**Interfaces:**
- Consumes: Task 2 export model, `Guide`, `FrameStore`, and `ActionGuidePresentation`.
- Produces: `ActionGuidePresentation::{set_explanation,explanation}`, `build_reviewed_export_job(&TimelineWorkspace) -> Result<ReviewedGuideExportJob, ExportError>`, and normalized hotspot order matching `ImageDocument::navigator_items()`.

- [ ] **Step 1: Write failing explanation lifecycle tests**

In `annotation.rs` tests:

```rust
#[test]
fn callout_explanation_is_keyed_by_annotation_and_survives_temporary_absence() {
    let mut presentation = ActionGuidePresentation::new();
    let store = frame_store_with_two_frames();
    let guide = guide();
    let step = guide.steps()[0].clone();
    let doc = presentation.document_for_step(&step, &store).unwrap();
    let id = doc.document.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(4.0, 4.0));

    assert!(presentation.set_explanation(step.source, id, "Click Settings".into()));
    assert_eq!(presentation.explanation(step.source, id), Some("Click Settings"));
    presentation.doc_mut(step.source).unwrap().document.delete_annotation(id).unwrap();
    assert_eq!(presentation.explanation(step.source, id), Some("Click Settings"));
}
```

Add `explanations: BTreeMap<AnnotationId, String>` to `StepAnnotationDocument`, initialize it empty, and expose `doc_mut` only at `pub(crate)` visibility for Timeline update/tests.

- [ ] **Step 2: Write failing reviewed-job adapter tests**

In new `guide_export.rs` tests, cover these exact cases:

```rust
fn real_workspace() -> TimelineWorkspace {
    TimelineWorkspace::new(
        super::super::tests::recording_from_frames(),
        CaptureRegion { x: 0, y: 0, width: 32, height: 32 },
        InputCapability::SemanticEvents,
        InputSourceKind::LinuxEvdev,
    )
}

#[test]
fn job_contains_text_notes_and_only_explained_callouts_in_navigator_order() {
    let mut state = real_workspace();
    let step = state.guide.steps()[0].clone();
    let doc = state.presentation.document_for_step(&step, &state.store).unwrap();
    let late = doc.document.add_number_callout(ImagePoint::new(20.0, 20.0), ImagePoint::new(24.0, 24.0));
    doc.document.add_text_note(ImagePoint::new(2.0, 2.0), "First note".into()).unwrap();
    let silent = doc.document.add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(14.0, 14.0));
    state.presentation.set_explanation(step.source, late, "Second explanation".into());
    state.presentation.set_explanation(step.source, silent, "   ".into());

    let job = build_reviewed_export_job(&state).unwrap();

    assert_eq!(job.steps[0].hotspots.len(), 2);
    assert_eq!(job.steps[0].hotspots[0].explanation, "First note");
    assert_eq!(job.steps[0].hotspots[1].explanation, "Second explanation");
    assert!(matches!(job.steps[0].image, ReviewedStepImage::Annotated(_)));
}

#[test]
fn job_without_matching_annotations_shares_retained_keyframe() {
    let state = real_workspace();
    let frame = Arc::clone(&state.store.retained(state.guide.steps()[0].keyframe).unwrap().image);
    let job = build_reviewed_export_job(&state).unwrap();
    let ReviewedStepImage::Retained(exported) = &job.steps[0].image else { panic!("retained") };
    assert!(Arc::ptr_eq(exported, &frame));
}

#[test]
fn job_is_isolated_from_edits_after_export_click() {
    let mut state = real_workspace();
    let job = build_reviewed_export_job(&state).unwrap();
    let exported_title = job.title.clone();
    let exported_step_title = job.steps[0].title.clone();

    state.guide.set_title("Edited after click".into());
    assert!(state.guide.rename(1, "Changed later".into()));

    assert_eq!(job.title, exported_title);
    assert_eq!(job.steps[0].title, exported_step_title);
    job.validate().unwrap();
}
```

- [ ] **Step 3: Run focused tests and confirm failure**

```bash
rtk cargo test -p rollshot-app --features action-guide callout_explanation_is_keyed_by_annotation_and_survives_temporary_absence
rtk cargo test -p rollshot-app --features action-guide job_contains_text_notes_and_only_explained_callouts_in_navigator_order
```

Expected: compilation fails on explanation and adapter APIs.

- [ ] **Step 4: Implement explanation storage and job construction**

Register `mod guide_export;` in `timeline_workspace/mod.rs`. In `guide_export.rs`, iterate Guide steps, require each retained frame, and select the image:

```rust
let image = match state.presentation.doc(step.source) {
    Some(doc) if doc.keyframe == step.keyframe && !doc.document.annotations().is_empty() =>
        ReviewedStepImage::Annotated(doc.document.flatten_snapshot()),
    _ => ReviewedStepImage::Retained(Arc::clone(&frame.image)),
};
```

For matching documents, iterate `doc.document.navigator_items()`, resolve each live annotation, and accept only:

```rust
let explanation = match annotation {
    Annotation::TextNote { text, .. } => text.trim(),
    Annotation::NumberCallout { id, .. } => doc.explanations
        .get(id)
        .map(String::as_str)
        .unwrap_or("")
        .trim(),
    _ => "",
};
```

Skip empty explanations. Normalize `annotation_bounds(annotation)` by source width/height, clamp rectangle edges to the image, and preserve navigator order. Finish with `job.validate()?`.

Set `job.title` from `state.guide.effective_title()`. Trim each caption and
store `None` when empty; trim every exported explanation once in this adapter.
Do not mutate the editable Guide strings during export.

- [ ] **Step 5: Run adapter and feature tests**

```bash
rtk cargo test -p rollshot-app --features action-guide guide_export
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo fmt --all -- --check
```

Expected: all pass, including keyframe-replacement clearing the entire step document and its explanation map.

- [ ] **Step 6: Commit the presentation adapter**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace
rtk git commit -m "feat(action): snapshot reviewed guide presentation"
```

---

### Task 4: Render the required Guide folder deterministically

**Files:**
- Modify: `crates/rollshot-action/src/export.rs`
- Modify: `crates/rollshot-action/src/error.rs`
- Create: `crates/rollshot-action/src/export/html.rs`
- Create: `crates/rollshot-action/src/export/viewer.html`
- Modify: `crates/rollshot-action/src/lib.rs`

**Interfaces:**
- Consumes: `ReviewedGuideExportJob`.
- Produces: `render_guide_folder(&ReviewedGuideExportJob, &Path) -> Result<PathBuf, ExportError>`, schema-v1 `SessionManifest`, and `index.html` containing safely encoded viewer data.

- [ ] **Step 1: Replace exporter tests with owned-job output tests**

Keep existing fixture helpers, construct a `ReviewedGuideExportJob`, and add:

```rust
fn annotated_job() -> ReviewedGuideExportJob {
    let mut document = ImageDocument::new(RgbaImage::from_pixel(8, 8, Rgba([20, 30, 40, 255])));
    document.add_redaction(ImageRect::new(0.0, 0.0, 4.0, 4.0)).unwrap();
    ReviewedGuideExportJob {
        title: "Checkout failure".into(),
        region: CaptureRegion { x: 0, y: 0, width: 8, height: 8 },
        input_source: InputSourceKind::LinuxEvdev,
        input_capability: InputCapability::SemanticEvents,
        steps: vec![ReviewedGuideStep {
            index: 1,
            title: "Submit order".into(),
            caption: Some("Confirm the request".into()),
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 100,
            image: ReviewedStepImage::Annotated(document.flatten_snapshot()),
            hotspots: vec![GuideHotspot {
                annotation_id: 1,
                bounds: NormalizedRect { x: 0.0, y: 0.0, width: 0.5, height: 0.5 },
                explanation: "Open Settings".into(),
            }],
        }],
    }
}

#[test]
fn renderer_writes_all_required_artifacts_from_one_job() {
    let parent = temp_dir("required");
    let destination = parent.join("guide");
    let job = annotated_job();

    let result = render_guide_folder(&job, &destination).unwrap();

    assert_eq!(result, destination);
    for relative in ["index.html", "steps.md", "session.json", "keyframes/001.png"] {
        assert!(result.join(relative).is_file(), "missing {relative}");
    }
    let manifest: SessionManifest = serde_json::from_slice(&std::fs::read(result.join("session.json")).unwrap()).unwrap();
    assert_eq!(manifest.schema_version, GUIDE_SCHEMA_VERSION);
    assert_eq!(manifest.title, "Checkout failure");
    assert_eq!(manifest.steps[0].title, "Submit order");
}

#[test]
fn renderer_never_replaces_destination_and_cleans_failed_build() {
    let parent = temp_dir("noclobber");
    let destination = parent.join("guide");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("keep.txt"), "old").unwrap();
    let error = render_guide_folder(&annotated_job(), &destination).unwrap_err();
    assert!(matches!(error, ExportError::DestinationExists { .. }));
    assert_eq!(std::fs::read_to_string(destination.join("keep.txt")).unwrap(), "old");
}

#[test]
fn embedded_json_cannot_close_its_script_element() {
    let mut job = annotated_job();
    job.title = "</script><script>globalThis.pwned=true</script>".into();
    let html = html::render(&job).unwrap();
    assert!(!html.contains("</script><script>globalThis.pwned"));
    assert!(html.contains("\\u003c/script\\u003e"));
}
```

- [ ] **Step 2: Run focused renderer tests and confirm failure**

```bash
rtk cargo test -p rollshot-action renderer_writes_all_required_artifacts_from_one_job
rtk cargo test -p rollshot-action embedded_json_cannot_close_its_script_element
```

Expected: compilation fails because the renderer and HTML module do not exist.

- [ ] **Step 3: Implement schema-v1 normalized serialization**

Update `SessionManifest`:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionManifest {
    #[serde(default = "legacy_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_manifest_title")]
    pub title: String,
    pub region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub steps: Vec<ManifestStep>,
}

fn legacy_schema_version() -> u32 { 0 }
fn default_manifest_title() -> String { DEFAULT_GUIDE_TITLE.to_string() }
```

Add a compatibility test deserializing the pre-v1 JSON shape and asserting schema `0` plus title `Action Guide`.

Add `hotspots: Vec<GuideHotspot>` to `ManifestStep` with
`#[serde(default, skip_serializing_if = "Vec::is_empty")]`, so old manifests
still deserialize and annotation explanation text matches the HTML snapshot.

- [ ] **Step 4: Implement sequential folder rendering**

`render_guide_folder` must:

1. Call `job.validate()` before creating the destination.
2. Return `DestinationExists` if the destination already exists.
3. Create `destination/keyframes`.
4. For each step, call `step.image.with_flattened_image(...)`, save `NNN.png`, and drop the callback-owned flattened image before advancing.
5. Build Markdown, manifest steps, and viewer data from the same step loop/order.
6. Write `steps.md`, `session.json`, and required `index.html`.
7. On any error after directory creation, remove only the newly created destination.
8. Log target `rollshot::action::export` with step count and result category only.

Use this transaction boundary and build loop:

```rust
pub fn render_guide_folder(
    job: &ReviewedGuideExportJob,
    destination: &Path,
) -> Result<PathBuf, ExportError> {
    job.validate()?;
    if destination.exists() {
        return Err(ExportError::DestinationExists {
            path: destination.display().to_string(),
        });
    }
    std::fs::create_dir(destination).map_err(|source| ExportError::Io {
        path: destination.display().to_string(),
        source,
    })?;
    let result = (|| {
        std::fs::create_dir(destination.join("keyframes")).map_err(|source| ExportError::Io {
            path: destination.display().to_string(),
            source,
        })?;
        build_folder(job, destination)
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(destination);
        tracing::debug!(target: TARGET_EXPORT, category = error.category(), "guide export rolled back");
        return Err(error);
    }
    tracing::info!(target: TARGET_EXPORT, steps = job.steps.len(), "guide export complete");
    Ok(destination.to_path_buf())
}

fn build_folder(job: &ReviewedGuideExportJob, destination: &Path) -> Result<(), ExportError> {
    let mut markdown = format!("# {}\n\n", job.title);
    let mut manifest_steps = Vec::with_capacity(job.steps.len());
    for (offset, step) in job.steps.iter().enumerate() {
        let file_name = format!("{:03}.png", offset + 1);
        let relative = format!("keyframes/{file_name}");
        let path = destination.join(&relative);
        step.image.with_flattened_image(|image| {
            image.save_with_format(&path, image::ImageFormat::Png)
                .map_err(|source| ExportError::Encode {
                    path: path.display().to_string(), source,
                })
        })?;
        markdown.push_str(&format!("{}. {}\n\n", step.index, step.title));
        if let Some(caption) = &step.caption {
            markdown.push_str(&format!("   {caption}\n\n"));
        }
        markdown.push_str(&format!("   ![]({relative})\n\n"));
        manifest_steps.push(ManifestStep {
            index: step.index,
            title: step.title.clone(),
            caption: step.caption.clone(),
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            keyframe_file: relative,
            hotspots: step.hotspots.clone(),
        });
    }
    write_text(destination.join("steps.md"), &markdown)?;
    let manifest = SessionManifest {
        schema_version: GUIDE_SCHEMA_VERSION,
        title: job.title.clone(),
        region: job.region,
        input_source: job.input_source,
        input_capability: job.input_capability,
        steps: manifest_steps,
    };
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|_| ExportError::Serialize { category: "session_manifest" })?;
    write_text(destination.join("session.json"), &json)?;
    write_text(destination.join("index.html"), &html::render(job)?)?;
    Ok(())
}

fn write_text(path: PathBuf, contents: &str) -> Result<(), ExportError> {
    std::fs::write(&path, contents).map_err(|source| ExportError::Io {
        path: path.display().to_string(), source,
    })
}
```

Add `ExportError::category() -> &'static str` mapping every variant to a stable
structural category. Do not log `Display` for errors that carry destination
paths.

Keep a temporary compatibility `export_guide` wrapper for existing call sites; implement it by building an annotation-free owned job and forwarding to `render_guide_folder`. Mark it `#[deprecated(note = "build a ReviewedGuideExportJob and call render_guide_folder")]`; Task 7 removes it after all callers migrate.

- [ ] **Step 5: Implement safe viewer-data assembly**

In `export/html.rs`, use these serializable private shapes; `From<&ReviewedGuideExportJob>` copies only small text/geometry metadata and reads dimensions without flattening pixels:

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerGuide {
    schema_version: u32,
    title: String,
    steps: Vec<ViewerStep>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerStep {
    index: usize,
    title: String,
    caption: Option<String>,
    keyframe_file: String,
    image_width: u32,
    image_height: u32,
    hotspots: Vec<GuideHotspot>,
}

impl From<&ReviewedGuideExportJob> for ViewerGuide {
    fn from(job: &ReviewedGuideExportJob) -> Self {
        Self {
            schema_version: GUIDE_SCHEMA_VERSION,
            title: job.title.clone(),
            steps: job.steps.iter().enumerate().map(|(offset, step)| {
                let (image_width, image_height) = step.image.dimensions();
                ViewerStep {
                    index: step.index,
                    title: step.title.clone(),
                    caption: step.caption.clone(),
                    keyframe_file: format!("keyframes/{:03}.png", offset + 1),
                    image_width,
                    image_height,
                    hotspots: step.hotspots.clone(),
                }
            }).collect(),
        }
    }
}
```

Serialize with `serde_json::to_string`, then escape in this order:

```rust
fn escape_script_data(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub(crate) fn render(job: &ReviewedGuideExportJob) -> Result<String, ExportError> {
    let data = escape_script_data(&serde_json::to_string(&ViewerGuide::from(job))
        .map_err(|_| ExportError::Serialize { category: "viewer_data" })?);
    let template = include_str!("viewer.html");
    let marker = "__ROLLSHOT_GUIDE_DATA__";
    if template.matches(marker).count() != 1 {
        return Err(ExportError::Template { category: "data_marker" });
    }
    Ok(template.replace(marker, &data))
}
```

Add `ExportError::Serialize { category: &'static str }` and
`ExportError::Template { category: &'static str }`; their displays and tracing
contain the category only, never serialized Guide content.

Create `viewer.html` with a working first-step baseline; Task 5 extends this exact DOM without renaming its IDs or accessibility labels:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Action Guide</title>
  <style>
    :root { color-scheme: light dark; font-family: system-ui, sans-serif; }
    body { margin: 0; }
    #reader { display: grid; grid-template-columns: 18rem 1fr; min-height: 100vh; }
    nav, main { padding: 1rem; }
    #image-shell { position: relative; display: inline-block; transform-origin: top left; }
    #step-image { display: block; max-width: 100%; height: auto; }
    #hotspots { position: absolute; inset: 0; }
    .hotspot { position: absolute; }
    [hidden] { display: none !important; }
  </style>
</head>
<body>
  <a href="#step-content">Skip to current step</a>
  <div id="reader">
    <nav aria-label="Guide steps">
      <h1 id="guide-title"></h1>
      <label for="search">Search guide</label>
      <input id="search" type="search">
      <div id="step-list"></div>
    </nav>
    <main id="step-content" tabindex="-1">
      <p id="step-progress" data-testid="step-progress"></p>
      <div id="image-viewport">
        <div id="image-shell">
          <img id="step-image" alt="">
          <div id="hotspots"></div>
        </div>
        <p id="image-loading" role="status">Loading image…</p>
        <p id="image-error" hidden>Image unavailable</p>
      </div>
      <h2 id="step-title"></h2>
      <p id="step-caption"></p>
      <button type="button" data-action="previous" aria-label="Previous step">Previous</button>
      <button type="button" data-action="next" aria-label="Next step">Next</button>
      <button type="button" data-action="copy">Copy step text</button>
      <button type="button" data-action="zoom-out" aria-label="Zoom out">−</button>
      <span id="zoom-value" data-testid="zoom-value">100%</span>
      <button type="button" data-action="zoom-in" aria-label="Zoom in">+</button>
      <button type="button" data-action="zoom-reset" aria-label="Reset zoom">Reset</button>
      <div id="popover" role="dialog" aria-label="Annotation explanation" hidden></div>
      <div id="copy-panel" hidden>
        <label for="copy-fallback">Step text for manual copy</label>
        <textarea id="copy-fallback" readonly></textarea>
      </div>
      <p id="status" role="status" aria-live="polite"></p>
    </main>
  </div>
  <noscript><a href="steps.md">Open the Markdown guide</a></noscript>
  <script id="guide-data" type="application/json">__ROLLSHOT_GUIDE_DATA__</script>
  <script>
    const guide = JSON.parse(document.getElementById('guide-data').textContent);
    const first = guide.steps[0];
    document.title = guide.title;
    document.getElementById('guide-title').textContent = guide.title;
    document.getElementById('step-progress').textContent = `Step 1 of ${guide.steps.length}`;
    document.getElementById('step-title').textContent = first.title;
    document.getElementById('step-caption').textContent = first.caption || '';
    const image = document.getElementById('step-image');
    image.onload = () => { document.getElementById('image-loading').hidden = true; };
    image.onerror = () => {
      document.getElementById('image-loading').hidden = true;
      document.getElementById('image-error').hidden = false;
    };
    image.src = first.keyframeFile;
    image.alt = `Step 1: ${first.title}`;
  </script>
</body>
</html>
```

- [ ] **Step 6: Verify redaction and rollback**

Add a test that creates a source with a unique pixel, applies `OpaqueRedaction`, renders the folder, decodes `keyframes/001.png`, and asserts the redacted output differs from the source while the PNG contains no ancillary source payload. Run:

```bash
rtk cargo test -p rollshot-action export
rtk cargo fmt --all -- --check
rtk cargo clippy -p rollshot-action --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 7: Commit the deterministic renderer**

```bash
rtk git add crates/rollshot-action/src
rtk git commit -m "feat(action): render offline guide folders"
```

---

### Task 5: Build and browser-test the offline reader

**Files:**
- Modify: `crates/rollshot-action/src/export/viewer.html`
- Create: `crates/rollshot-action/examples/export_html_fixture.rs`
- Create: `scripts/html-guide-e2e/package.json`
- Create: `scripts/html-guide-e2e/package-lock.json`
- Create: `scripts/html-guide-e2e/playwright.config.mjs`
- Create: `scripts/html-guide-e2e/guide.spec.mjs`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: Task 4 viewer schema and renderer.
- Produces: complete `file://` reader behavior and a pinned browser-test lane.

- [ ] **Step 1: Create the deterministic browser fixture generator**

`export_html_fixture.rs` accepts exactly one destination argument, removes that test-only destination if present, builds four 320×180 colored steps with captions and normalized hotspots, then calls `render_guide_folder`. Use adversarial text in step 2 and omit no images; the missing-image test temporarily renames one file.

The entry point must be:

```rust
use std::{path::PathBuf, sync::Arc};
use image::{Rgba, RgbaImage};
use rollshot_action::{
    CandidateKind, CaptureRegion, DetectReason, GuideHotspot, InputCapability,
    InputSourceKind, NormalizedRect, ReviewedGuideExportJob, ReviewedGuideStep,
    ReviewedStepImage,
};

fn fixture_job() -> ReviewedGuideExportJob {
    let titles = ["Open Settings", "Submit </script><script>globalThis.pwned=true</script>", "Verify result", "Finish"];
    let colors = [[40, 90, 180, 255], [50, 150, 90, 255], [180, 100, 40, 255], [120, 70, 170, 255]];
    let steps = titles.into_iter().zip(colors).enumerate().map(|(offset, (title, color))| {
        let hotspots = if offset == 0 {
            vec![
                GuideHotspot {
                    annotation_id: 10,
                    bounds: NormalizedRect { x: 0.1, y: 0.1, width: 0.2, height: 0.2 },
                    explanation: "Open Settings".into(),
                },
                GuideHotspot {
                    annotation_id: 11,
                    bounds: NormalizedRect { x: 0.6, y: 0.5, width: 0.2, height: 0.2 },
                    explanation: "Choose Privacy".into(),
                },
            ]
        } else {
            Vec::new()
        };
        ReviewedGuideStep {
            index: offset + 1,
            title: title.into(),
            caption: Some(format!("Caption for step {}", offset + 1)),
            kind: CandidateKind::Click,
            reason: DetectReason::VisualChange,
            at_ms: (offset as u64 + 1) * 100,
            image: ReviewedStepImage::Retained(Arc::new(RgbaImage::from_pixel(320, 180, Rgba(color)))),
            hotspots,
        }
    }).collect();
    ReviewedGuideExportJob {
        title: "Checkout failure".into(),
        region: CaptureRegion { x: 0, y: 0, width: 320, height: 180 },
        input_source: InputSourceKind::LinuxEvdev,
        input_capability: InputCapability::SemanticEvents,
        steps,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args_os().nth(1).ok_or("destination argument required")?;
    let destination = PathBuf::from(destination);
    if destination.exists() { std::fs::remove_dir_all(&destination)?; }
    let job = fixture_job();
    rollshot_action::render_guide_folder(&job, &destination)?;
    Ok(())
}
```

- [ ] **Step 2: Add pinned Playwright configuration**

Create `package.json`:

```json
{
  "name": "rollshot-html-guide-e2e",
  "private": true,
  "type": "module",
  "scripts": {
    "fixture": "cd ../.. && cargo run -p rollshot-action --example export_html_fixture -- scripts/html-guide-e2e/.tmp/guide",
    "test": "npm run fixture && playwright test"
  },
  "devDependencies": {
    "@playwright/test": "1.55.0"
  }
}
```

Create `playwright.config.mjs`:

```javascript
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: '.',
  testMatch: 'guide.spec.mjs',
  workers: 1,
  fullyParallel: false,
  use: { trace: 'retain-on-failure' },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } }
  ]
});
```

Run `rtk npm install --package-lock-only` from `scripts/html-guide-e2e` and commit the resulting `package-lock.json`.

- [ ] **Step 3: Write failing browser behavior tests**

Create `guide.spec.mjs` with helpers and tests for all signature behaviors:

```javascript
import { test, expect } from '@playwright/test';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { rename } from 'node:fs/promises';

const guideDir = resolve('.tmp/guide');
const guideUrl = pathToFileURL(resolve(guideDir, 'index.html')).href;

test.beforeEach(async ({ page }) => {
  const nonFileRequests = [];
  page.on('request', request => {
    const protocol = new URL(request.url()).protocol;
    if (protocol !== 'file:' && protocol !== 'data:') nonFileRequests.push(request.url());
  });
  await page.goto(guideUrl);
  await expect(page.getByTestId('step-progress')).toHaveText('Step 1 of 4');
  expect(nonFileRequests).toEqual([]);
});

test('navigation, keyboard, and zoom stay synchronized', async ({ page }) => {
  await page.getByRole('button', { name: 'Next step' }).click();
  await expect(page.getByTestId('step-progress')).toHaveText('Step 2 of 4');
  await page.keyboard.press('ArrowRight');
  await expect(page.getByTestId('step-progress')).toHaveText('Step 3 of 4');
  await page.keyboard.press('+');
  await expect(page.getByTestId('zoom-value')).toHaveText('125%');
  await page.keyboard.press('0');
  await expect(page.getByTestId('zoom-value')).toHaveText('100%');
});

test('search opens annotation matches and does not execute guide markup', async ({ page }) => {
  await page.getByRole('searchbox', { name: 'Search guide' }).fill('settings');
  await page.getByRole('button', { name: /settings/i }).click();
  await expect(page.getByRole('dialog')).toContainText('Open Settings');
  expect(await page.evaluate(() => globalThis.pwned)).toBeUndefined();
});

test('clipboard rejection exposes honest manual copy', async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: () => Promise.reject(new DOMException('denied', 'NotAllowedError')) }
  }));
  await page.reload();
  await page.getByRole('button', { name: 'Copy step text' }).click();
  const fallback = page.getByRole('textbox', { name: 'Step text for manual copy' });
  await expect(fallback).toBeFocused();
  await expect(page.getByRole('status')).toContainText('Press Ctrl/Cmd+C');
});

test('clipboard success copies exact step text and reports success', async ({ page }) => {
  let copied = null;
  await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: text => { globalThis.__copied = text; return Promise.resolve(); } }
  }));
  await page.reload();
  await page.getByRole('button', { name: 'Copy step text' }).click();
  copied = await page.evaluate(() => globalThis.__copied);
  expect(copied).toContain('Step 1: Open Settings');
  await expect(page.getByRole('status')).toHaveText('Copied');
  await expect(page.getByRole('textbox', { name: 'Step text for manual copy' })).toBeHidden();
});

test('missing image is local to one step', async ({ page }) => {
  const image = resolve(guideDir, 'keyframes/003.png');
  const hidden = `${image}.missing`;
  await rename(image, hidden);
  try {
    await page.goto(guideUrl);
    await page.getByRole('button', { name: /Step 3/ }).click();
    await expect(page.getByText('Image unavailable')).toBeVisible();
    await page.getByRole('button', { name: /Step 2/ }).click();
    await expect(page.getByRole('img')).toBeVisible();
  } finally {
    await rename(hidden, image);
  }
});
```

Append these cases to the same file:

```javascript
test('guide-title search appears once and slash focuses search', async ({ page }) => {
  await page.keyboard.press('/');
  await expect(page.getByRole('searchbox', { name: 'Search guide' })).toBeFocused();
  await page.keyboard.type('Checkout failure');
  await expect(page.locator('[data-result-kind="guide"]')).toHaveCount(1);
  await expect(page.locator('#guide-title mark')).toHaveText('Checkout failure');
});

test('popover replaces, closes on Escape, and shortcuts ignore text entry', async ({ page }) => {
  const hotspots = page.locator('.hotspot');
  await hotspots.nth(0).click();
  await expect(hotspots.nth(0)).toBeFocused();
  const firstText = await page.getByRole('dialog').textContent();
  await hotspots.nth(1).click();
  await expect(hotspots.nth(1)).toBeFocused();
  await expect(page.getByRole('dialog')).not.toHaveText(firstText);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toBeHidden();
  await expect(hotspots.nth(1)).toBeFocused();
  await page.getByRole('searchbox', { name: 'Search guide' }).fill('x');
  await page.keyboard.press('ArrowRight');
  await expect(page.getByTestId('step-progress')).toHaveText('Step 1 of 4');
});

test('narrow layout uses drawer and below-image explanations', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 800 });
  await expect(page.getByRole('button', { name: 'Toggle steps' })).toBeVisible();
  await page.locator('.hotspot').first().click();
  await expect(page.getByRole('dialog')).toHaveCSS('position', 'static');
});

test('theme, reduced motion, skip link, and focus visibility are honored', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Skip to current step' })).toBeFocused();
  await page.emulateMedia({ colorScheme: 'light', reducedMotion: 'no-preference' });
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'full');
});

test('hotspot percentages stay aligned while shell zooms', async ({ page }) => {
  await page.locator('.hotspot').first().click();
  const popoverBefore = await page.getByRole('dialog').boundingBox();
  const hotspot = page.locator('.hotspot').first();
  const shell = page.locator('#image-shell');
  const before = await hotspot.boundingBox();
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await page.getByRole('button', { name: 'Zoom in' }).click();
  const after = await hotspot.boundingBox();
  const shellAfter = await shell.boundingBox();
  expect(after.width / before.width).toBeCloseTo(1.5, 1);
  expect(after.x).toBeGreaterThanOrEqual(shellAfter.x);
  expect(after.x + after.width).toBeLessThanOrEqual(shellAfter.x + shellAfter.width + 1);
  const popoverAfter = await page.getByRole('dialog').boundingBox();
  expect(popoverAfter.x).not.toBe(popoverBefore.x);
});

test('initial load requests only current and adjacent keyframes', async ({ page }) => {
  const images = [];
  page.on('request', request => {
    if (request.url().endsWith('.png')) images.push(request.url());
  });
  await page.goto(guideUrl);
  await page.waitForLoadState('load');
  expect(images.some(url => url.endsWith('/004.png'))).toBe(false);
});
```

- [ ] **Step 4: Run the browser tests and confirm interaction failures**

From `scripts/html-guide-e2e`:

```bash
rtk npm ci
rtk npx playwright install chromium firefox webkit
rtk npm test
```

Expected: fixture generation passes, while tests fail on missing reader behavior.

- [ ] **Step 5: Implement the complete reader state machine**

Replace the Task 4 baseline inline script with the complete script in this
step. It parses `#guide-data` once and keeps exactly this state:

```javascript
const guide = JSON.parse(document.getElementById('guide-data').textContent);
const state = { step: 0, zoom: 1, query: '', openHotspot: null, drawerOpen: false };
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 3;
const ZOOM_STEP = 0.25;
```

Implement these named functions so browser failures map to one responsibility:

```javascript
const elements = {
  reader: document.getElementById('reader'),
  title: document.getElementById('guide-title'),
  search: document.getElementById('search'),
  list: document.getElementById('step-list'),
  progress: document.getElementById('step-progress'),
  stepTitle: document.getElementById('step-title'),
  caption: document.getElementById('step-caption'),
  image: document.getElementById('step-image'),
  imageLoading: document.getElementById('image-loading'),
  imageError: document.getElementById('image-error'),
  shell: document.getElementById('image-shell'),
  hotspots: document.getElementById('hotspots'),
  popover: document.getElementById('popover'),
  zoom: document.getElementById('zoom-value'),
  status: document.getElementById('status'),
  copyPanel: document.getElementById('copy-panel'),
  copyFallback: document.getElementById('copy-fallback')
};

const normalized = guide.steps.map(step => ({
  title: step.title.toLocaleLowerCase(),
  caption: (step.caption || '').toLocaleLowerCase(),
  hotspots: step.hotspots.map(hotspot => hotspot.explanation.toLocaleLowerCase())
}));

function ownsKeyboard(event) {
  const target = event.target;
  return target instanceof Element &&
    (target.matches('input,textarea,select') || target.isContentEditable);
}

function setHighlightedText(element, text, query) {
  element.replaceChildren();
  const q = query.trim().toLocaleLowerCase();
  const at = q ? text.toLocaleLowerCase().indexOf(q) : -1;
  if (at < 0) { element.textContent = text; return; }
  element.append(document.createTextNode(text.slice(0, at)));
  const mark = document.createElement('mark');
  mark.textContent = text.slice(at, at + query.trim().length);
  element.append(mark, document.createTextNode(text.slice(at + query.trim().length)));
}

function searchableResults(query) {
  const q = query.trim().toLocaleLowerCase();
  if (!q) return guide.steps.map((step, index) => ({ kind: 'step', index, step }));
  const results = [];
  if (guide.title.toLocaleLowerCase().includes(q)) results.push({ kind: 'guide', index: 0 });
  normalized.forEach((value, index) => {
    const hotspot = value.hotspots.findIndex(text => text.includes(q));
    if (value.title.includes(q) || value.caption.includes(q) || hotspot >= 0) {
      results.push({ kind: 'step', index, hotspot: hotspot >= 0 ? hotspot : null });
    }
  });
  return results;
}

function renderSteps() {
  setHighlightedText(elements.title, guide.title, state.query);
  elements.list.replaceChildren();
  for (const result of searchableResults(state.query)) {
    const button = document.createElement('button');
    button.type = 'button';
    button.dataset.action = 'select-step';
    button.dataset.step = String(result.index);
    if (result.hotspot !== undefined && result.hotspot !== null) {
      button.dataset.hotspot = String(guide.steps[result.index].hotspots[result.hotspot].annotationId);
    }
    if (result.kind === 'guide') {
      button.dataset.resultKind = 'guide';
      button.textContent = `Guide: ${guide.title}`;
    } else {
      button.textContent = `Step ${result.index + 1}: ${guide.steps[result.index].title}`;
      if (result.index === state.step) button.setAttribute('aria-current', 'step');
    }
    elements.list.append(button);
  }
}

function renderHotspots() {
  elements.hotspots.replaceChildren();
  const step = guide.steps[state.step];
  for (const hotspot of step.hotspots) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'hotspot';
    button.dataset.action = 'hotspot';
    button.dataset.hotspot = String(hotspot.annotationId);
    button.setAttribute('aria-label', hotspot.explanation);
    button.style.left = `${hotspot.bounds.x * 100}%`;
    button.style.top = `${hotspot.bounds.y * 100}%`;
    button.style.width = `${hotspot.bounds.width * 100}%`;
    button.style.height = `${hotspot.bounds.height * 100}%`;
    button.setAttribute('aria-expanded', String(state.openHotspot === hotspot.annotationId));
    elements.hotspots.append(button);
  }
}

function openHotspot(id) {
  const hotspot = guide.steps[state.step].hotspots.find(value => value.annotationId === id);
  state.openHotspot = hotspot ? id : null;
  elements.popover.hidden = !hotspot;
  setHighlightedText(elements.popover, hotspot ? hotspot.explanation : '', state.query);
  for (const button of elements.hotspots.querySelectorAll('.hotspot')) {
    button.setAttribute('aria-expanded', String(Number(button.dataset.hotspot) === state.openHotspot));
  }
  positionPopover();
}

function positionPopover() {
  if (state.openHotspot !== null && innerWidth > 760) {
    const anchor = elements.hotspots
      .querySelector(`[data-hotspot="${state.openHotspot}"]`)
      ?.getBoundingClientRect();
    if (!anchor) return;
    elements.popover.style.left = `${Math.min(innerWidth - 400, anchor.right + 8)}px`;
    elements.popover.style.top = `${Math.max(8, Math.min(innerHeight - 160, anchor.top))}px`;
  } else {
    elements.popover.style.removeProperty('left');
    elements.popover.style.removeProperty('top');
  }
}

function preloadAdjacent() {
  for (const index of [state.step - 1, state.step + 1]) {
    if (guide.steps[index]) {
      const preload = new Image();
      preload.src = guide.steps[index].keyframeFile;
    }
  }
}

function renderStep() {
  const step = guide.steps[state.step];
  elements.progress.textContent = `Step ${state.step + 1} of ${guide.steps.length}`;
  setHighlightedText(elements.stepTitle, step.title, state.query);
  setHighlightedText(elements.caption, step.caption || '', state.query);
  elements.imageLoading.hidden = false;
  elements.imageError.hidden = true;
  elements.image.hidden = false;
  elements.hotspots.hidden = false;
  elements.image.alt = `Step ${state.step + 1}: ${step.title}`;
  elements.image.onload = () => {
    elements.imageLoading.hidden = true;
    elements.imageError.hidden = true;
    elements.hotspots.hidden = false;
    positionPopover();
  };
  elements.image.onerror = () => {
    elements.imageLoading.hidden = true;
    elements.image.hidden = true;
    elements.hotspots.hidden = true;
    elements.imageError.hidden = false;
  };
  elements.image.src = step.keyframeFile;
  renderHotspots();
  renderSteps();
  preloadAdjacent();
}

function selectStep(index, hotspotId = null) {
  state.step = Math.max(0, Math.min(guide.steps.length - 1, index));
  state.openHotspot = null;
  elements.popover.hidden = true;
  renderStep();
  if (hotspotId !== null) openHotspot(hotspotId);
}

function setZoom(value) {
  state.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, value));
  elements.shell.style.transform = `scale(${state.zoom})`;
  elements.zoom.textContent = `${Math.round(state.zoom * 100)}%`;
  positionPopover();
}

function stepCopyText(step) {
  return [
    `Step ${step.index}: ${step.title}`,
    step.caption || '',
    ...step.hotspots.map(hotspot => hotspot.explanation)
  ].filter(Boolean).join('\n\n');
}

async function copyStepText() {
  const text = stepCopyText(guide.steps[state.step]);
  try {
    if (!navigator.clipboard?.writeText) throw new Error('clipboard unavailable');
    await navigator.clipboard.writeText(text);
    elements.copyPanel.hidden = true;
    elements.status.textContent = 'Copied';
    setTimeout(() => {
      if (elements.status.textContent === 'Copied') elements.status.textContent = '';
    }, 1800);
  } catch (_) {
    elements.copyPanel.hidden = false;
    elements.copyFallback.value = text;
    elements.copyFallback.focus();
    elements.copyFallback.select();
    elements.status.textContent = 'Copy was blocked. Press Ctrl/Cmd+C to copy the selected text.';
  }
}
```

Wire the state machine once:

```javascript
elements.reader.addEventListener('click', event => {
  const control = event.target.closest('[data-action]');
  if (!control) {
    if (event.target.closest('#image-viewport') && state.openHotspot !== null) openHotspot(null);
    return;
  }
  const action = control.dataset.action;
  if (action === 'previous') selectStep(state.step - 1);
  if (action === 'next') selectStep(state.step + 1);
  if (action === 'select-step') selectStep(Number(control.dataset.step), control.dataset.hotspot ? Number(control.dataset.hotspot) : null);
  if (action === 'hotspot') openHotspot(Number(control.dataset.hotspot));
  if (action === 'copy') void copyStepText();
  if (action === 'zoom-in') setZoom(state.zoom + ZOOM_STEP);
  if (action === 'zoom-out') setZoom(state.zoom - ZOOM_STEP);
  if (action === 'zoom-reset') setZoom(1);
  if (action === 'toggle-drawer') {
    state.drawerOpen = !state.drawerOpen;
    document.querySelector('nav').classList.toggle('open', state.drawerOpen);
    control.setAttribute('aria-expanded', String(state.drawerOpen));
  }
});

elements.search.addEventListener('input', event => {
  state.query = event.target.value;
  renderSteps();
  const step = guide.steps[state.step];
  setHighlightedText(elements.stepTitle, step.title, state.query);
  setHighlightedText(elements.caption, step.caption || '', state.query);
  if (state.openHotspot !== null) openHotspot(state.openHotspot);
});

document.getElementById('image-viewport').addEventListener('scroll', positionPopover, { passive: true });
window.addEventListener('resize', positionPopover, { passive: true });

document.addEventListener('keydown', event => {
  if (event.key === '/' && !ownsKeyboard(event)) { event.preventDefault(); elements.search.focus(); return; }
  if (event.key === 'Escape') {
    if (state.openHotspot !== null) openHotspot(null);
    else {
      state.query = '';
      elements.search.value = '';
      renderSteps();
      setHighlightedText(elements.stepTitle, guide.steps[state.step].title, '');
      setHighlightedText(elements.caption, guide.steps[state.step].caption || '', '');
    }
    return;
  }
  if (ownsKeyboard(event)) return;
  if (event.key === 'ArrowLeft') selectStep(state.step - 1);
  if (event.key === 'ArrowRight') selectStep(state.step + 1);
  if (event.key === '+' || event.key === '=') setZoom(state.zoom + ZOOM_STEP);
  if (event.key === '-') setZoom(state.zoom - ZOOM_STEP);
  if (event.key === '0') setZoom(1);
});

const dark = matchMedia('(prefers-color-scheme: dark)');
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
document.documentElement.dataset.theme = dark.matches ? 'dark' : 'light';
document.documentElement.dataset.motion = reduced.matches ? 'reduce' : 'full';
elements.title.textContent = guide.title;
document.title = guide.title;
renderStep();
```

Use one delegated `click` handler on the reader root and one `keydown` handler on `document`; rerendered nodes must not register listeners. Add this button before `<nav>`:

```html
<button id="drawer-toggle" type="button" data-action="toggle-drawer"
        aria-label="Toggle steps" aria-expanded="false">Steps</button>
```

Replace the baseline style with this complete layout; image and hotspot overlay remain in the same transformed shell, so percentage geometry scales together:

```css
:root {
  color-scheme: light dark;
  --bg: #f7f7f8; --panel: #fff; --text: #202124; --muted: #62666d;
  --line: #d9dce1; --accent: #2563eb; --focus: #f59e0b;
  font-family: Inter, ui-sans-serif, system-ui, sans-serif;
}
html[data-theme="dark"] {
  --bg: #15171a; --panel: #202328; --text: #f4f5f6; --muted: #b7bbc2;
  --line: #3a3f47; --accent: #78a7ff; --focus: #fbbf24;
}
* { box-sizing: border-box; }
body { margin: 0; color: var(--text); background: var(--bg); }
body > a { position: fixed; left: 1rem; top: -4rem; z-index: 20; }
body > a:focus { top: 1rem; padding: .5rem; background: var(--panel); }
button, input, textarea { font: inherit; }
button:focus-visible, input:focus-visible, textarea:focus-visible, a:focus-visible {
  outline: 3px solid var(--focus); outline-offset: 2px;
}
#reader { display: grid; grid-template-columns: 19rem minmax(0, 1fr); min-height: 100vh; }
nav { padding: 1rem; border-right: 1px solid var(--line); background: var(--panel); }
#search { width: 100%; margin: .5rem 0 1rem; }
#step-list { display: grid; gap: .4rem; }
#step-list button { text-align: left; padding: .65rem; }
#step-list [aria-current="step"] { border-color: var(--accent); color: var(--accent); }
main { min-width: 0; padding: 1rem 1.5rem 2rem; }
#image-viewport { overflow: auto; min-height: 12rem; padding: 1rem; text-align: center; }
#image-shell { position: relative; display: inline-block; transform-origin: top left; }
#step-image { display: block; max-width: min(100%, 1200px); height: auto; }
#hotspots { position: absolute; inset: 0; }
.hotspot { position: absolute; border: 2px solid var(--accent); background: color-mix(in srgb, var(--accent) 20%, transparent); }
#popover { position: fixed; max-width: 24rem; padding: 1rem; border: 1px solid var(--line); border-radius: .6rem; background: var(--panel); box-shadow: 0 .5rem 2rem #0004; }
#copy-panel textarea { width: min(100%, 44rem); min-height: 9rem; }
#drawer-toggle { display: none; }
[hidden] { display: none !important; }
@media (max-width: 760px) {
  #reader { display: block; }
  #drawer-toggle { display: block; position: fixed; right: 1rem; top: 1rem; z-index: 12; }
  nav { display: none; position: fixed; inset: 0 25% 0 0; z-index: 11; overflow: auto; box-shadow: .5rem 0 2rem #0005; }
  nav.open { display: block; }
  main { padding-top: 4rem; }
  #popover { position: static; max-width: none; margin-top: 1rem; box-shadow: none; }
}
@media (prefers-reduced-motion: no-preference) {
  #image-shell, #popover { transition: transform 120ms ease, opacity 120ms ease; }
}
```

Keep semantic landmarks, the skip link, labels, `aria-current`, `aria-expanded`, `role="status"`, `role="dialog"`, and the `data-testid` hooks used by the tests.

- [ ] **Step 6: Add the browser CI lane**

Append a separate Ubuntu job to `.github/workflows/ci.yml`:

```yaml
  html-guide-e2e:
    name: Offline HTML Guide
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: scripts/html-guide-e2e/package-lock.json
      - name: Install browser test dependencies
        working-directory: scripts/html-guide-e2e
        run: npm ci
      - name: Install Playwright browsers
        working-directory: scripts/html-guide-e2e
        run: npx playwright install --with-deps chromium firefox webkit
      - name: Test offline reader
        working-directory: scripts/html-guide-e2e
        run: npm test
```

- [ ] **Step 7: Run renderer and browser verification**

```bash
rtk cargo test -p rollshot-action export
rtk npm test
rtk cargo fmt --all -- --check
```

Run `rtk npm test` from `scripts/html-guide-e2e`. Expected: Chromium, Firefox, and WebKit projects pass with no HTTP(S) request.

- [ ] **Step 8: Commit the offline reader**

```bash
rtk git add crates/rollshot-action/src/export crates/rollshot-action/examples scripts/html-guide-e2e .github/workflows/ci.yml
rtk git commit -m "feat(action): add offline interactive guide reader"
```

---

### Task 6: Add standalone export lifecycle and Timeline editing

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/timeline_workspace/guide_export.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/{mod.rs,annotation.rs,update.rs,view.rs}`
- Create: `crates/rollshot-app/src/platform_actions.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/result_workspace/{actions.rs,update.rs}`

**Interfaces:**
- Consumes: `build_reviewed_export_job`, `render_guide_folder`, iced `Task`, and `rustix::fs::renameat_with`.
- Produces: `PendingStandaloneExport`, `StandaloneExportRequest`, `StandaloneExportResult`, `run_standalone_export`, atomic unique folder commits, `platform_actions::{open_path,reveal}`, and operation-ID-correlated Timeline export states/actions.

- [ ] **Step 1: Write failing standalone naming and no-clobber tests**

In `guide_export.rs` tests:

```rust
#[test]
fn folder_name_uses_title_time_and_numeric_suffix_without_replacing() {
    let parent = tempfile::tempdir().unwrap();
    let at = chrono::Local.with_ymd_and_hms(2026, 7, 16, 9, 8, 7).unwrap();
    let first = choose_destination(parent.path(), "Checkout / Failure", at, 1);
    assert_eq!(first.file_name().unwrap(), "checkout-failure-2026-07-16-090807");
    std::fs::create_dir(&first).unwrap();
    let second = choose_destination(parent.path(), "Checkout / Failure", at, 2);
    assert_eq!(second.file_name().unwrap(), "checkout-failure-2026-07-16-090807-2");
}

#[test]
fn failed_standalone_export_removes_temp_and_keeps_existing_output() {
    let parent = tempfile::tempdir().unwrap();
    let existing = parent.path().join("action-guide-2026-07-16-090807");
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("keep"), "safe").unwrap();
    let mut job = build_reviewed_export_job(&real_workspace()).unwrap();
    job.steps.clear();
    let result = export_standalone(StandaloneExportRequest {
        parent: parent.path().to_path_buf(),
        created_at: Local.with_ymd_and_hms(2026, 7, 16, 9, 8, 7).unwrap(),
        job,
    }).unwrap_err();
    assert!(result.contains("steps"));
    assert_eq!(std::fs::read_to_string(existing.join("keep")).unwrap(), "safe");
    assert!(std::fs::read_dir(parent.path()).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().contains(".tmp-")));
}

#[test]
fn commit_collision_retries_without_replacing_external_directory() {
    let parent = tempfile::tempdir().unwrap();
    let request = standalone_request(parent.path());
    let first = choose_destination(parent.path(), &request.job.title, request.created_at, 1);
    let result = export_standalone_with_commit_hook(request, |attempt, destination| {
        if attempt == 1 {
            std::fs::create_dir(destination).unwrap();
            std::fs::write(destination.join("external"), "safe").unwrap();
        }
    }).unwrap();
    assert!(result.directory.file_name().unwrap().to_string_lossy().ends_with("-2"));
    assert_eq!(std::fs::read_to_string(first.join("external")).unwrap(), "safe");
}
```

- [ ] **Step 2: Write failing Timeline state tests**

Add tests in `update.rs` for:

```rust
#[test]
fn export_completion_keeps_workspace_and_exposes_open_actions() {
    let mut state = ws(synthetic_recording(1));
    state.export_state = GuideExportState::Exporting { operation_id: 7 };
    let directory = PathBuf::from("/tmp/guide");
    let index_html = directory.join("index.html");
    apply_export_finished(&mut state, 7, Ok(StandaloneExportResult {
        operation_id: 7,
        directory: directory.clone(),
        index_html: index_html.clone(),
    }));
    assert!(matches!(state.export_state, GuideExportState::Succeeded));
    assert_eq!(state.last_export.as_ref().unwrap().index_html, index_html);
}

#[test]
fn callout_explanation_message_updates_only_matching_annotation() {
    let mut state = ws(recording_from_frames());
    update(&mut state, Message::AnnotateStepRequested);
    let source = state.annotation_session.as_ref().unwrap().source;
    let id = state.presentation.doc_mut(source).unwrap().document.add_number_callout(
        ImagePoint::new(2.0, 2.0),
        ImagePoint::new(8.0, 8.0),
    );
    update(&mut state, Message::AnnotationExplanationChanged(id, "Open Settings".into()));
    assert_eq!(state.presentation.explanation(source, id), Some("Open Settings"));
}

#[test]
fn picker_cancel_and_stale_results_do_not_mutate_current_operation() {
    let mut state = ws(synthetic_recording(1));
    begin_export(&mut state, 41).unwrap();
    apply_export_dir_chosen(&mut state, 41, None);
    assert!(matches!(state.export_state, GuideExportState::Idle));
    assert!(state.last_export.is_none());

    begin_export(&mut state, 42).unwrap();
    apply_export_finished(&mut state, 41, Ok(fake_result("/tmp/stale")));
    assert!(matches!(
        state.export_state,
        GuideExportState::PickingDestination { operation_id: 42, .. }
    ));
}
```

Implement and test the pure reducer with this exact boundary; the `Message::ExportFinished` arm calls it and returns `Task::none()`:

```rust
fn apply_export_finished(
    state: &mut TimelineWorkspace,
    operation_id: u64,
    result: Result<StandaloneExportResult, String>,
) {
    let GuideExportState::Exporting { operation_id: current } = &state.export_state else {
        return;
    };
    if operation_id != *current { return; }
    match result {
        Ok(exported) => {
            state.export_state = GuideExportState::Succeeded;
            state.last_export = Some(exported);
            state.message = Some("Action Guide exported.".into());
        }
        Err(error) => {
            state.export_state = GuideExportState::Idle;
            state.message = Some(format!("Action Guide export failed: {error}"));
        }
    }
}
```

- [ ] **Step 3: Run the standalone and reducer tests in RED state**

```bash
rtk cargo test -p rollshot-app --features action-guide commit_collision_retries_without_replacing_external_directory
rtk cargo test -p rollshot-app --features action-guide picker_cancel_and_stale_results_do_not_mutate_current_operation
```

Expected: compilation fails because atomic commit helpers, pending state, and
operation-correlated messages do not exist.

- [ ] **Step 4: Implement safe destination allocation and worker**

Add `rustix = { version = "1.1", features = ["fs"] }` to workspace
dependencies and consume it directly from `rollshot-app`. Do not use a
check-then-`std::fs::rename` fallback.

In `guide_export.rs`, define:

```rust
pub(crate) struct PendingStandaloneExport {
    pub operation_id: u64,
    pub created_at: DateTime<Local>,
    pub job: ReviewedGuideExportJob,
}

pub(crate) struct StandaloneExportRequest {
    pub operation_id: u64,
    pub parent: PathBuf,
    pub created_at: DateTime<Local>,
    pub job: ReviewedGuideExportJob,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StandaloneExportResult {
    pub operation_id: u64,
    pub directory: PathBuf,
    pub index_html: PathBuf,
}

pub(crate) async fn run_standalone_export(
    request: StandaloneExportRequest,
) -> Result<StandaloneExportResult, String> {
    tokio::task::spawn_blocking(move || export_standalone(request))
        .await
        .map_err(|_| "Action Guide export worker failed".to_string())?
}
```

`safe_slug` keeps the stated Unicode rules but tracks the scalar count in a
counter rather than repeatedly scanning the output. Split destination
construction from existence checks:

```rust
fn choose_destination(
    parent: &Path,
    title: &str,
    created_at: DateTime<Local>,
    suffix: u32,
) -> PathBuf {
    let base = format!("{}-{}", safe_slug(title), created_at.format("%Y-%m-%d-%H%M%S"));
    let name = if suffix == 1 { base } else { format!("{base}-{suffix}") };
    parent.join(name)
}

fn commit_noreplace(temp: &Path, destination: &Path) -> Result<(), rustix::io::Errno> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, temp, CWD, destination, RenameFlags::NOREPLACE)
}
```

Render exactly once into a randomized sibling temp guarded by
`TempGuideGuard`. Then try suffixes from 1 upward:

1. Call `commit_noreplace`.
2. On `Errno::EXIST`, choose the next suffix and retry the same temp.
3. On `Errno::NOSYS`, `Errno::INVAL`, or `Errno::NOTSUP`, return an explicit
   `atomic no-replace commit is unsupported on this filesystem` error.
4. Map other errors structurally and fail.
5. On success, mark the guard committed and return paths plus operation ID.

Never call `remove_dir_all` on a final path and never fall back to
`std::fs::rename`. Add a test-only commit hook immediately before each
`commit_noreplace` call so the collision test can deterministically insert an
external directory. Add a second two-thread test to prove concurrent Rollshot
exports retain both outputs.

Slug rules: Unicode alphanumeric characters lowercased, non-alphanumeric runs
become one `-`, trim `-`, cap at 80 Unicode scalar values, fallback
`action-guide`.

- [ ] **Step 5: Move reveal behavior and add open behavior**

Register `mod platform_actions;` in `main.rs`. Move `reveal`, its Linux D-Bus/`xdg-open` fallback, and tests from `result_workspace/actions.rs` into `platform_actions.rs`. Update result-workspace callers to `crate::platform_actions::reveal(path)`.

Add a pure `PlatformCommand { program, args }` builder and a small injected
execution boundary:

```rust
fn open_command(path: &Path) -> Result<PlatformCommand, String> {
    #[cfg(target_os = "macos")]
    return Ok(PlatformCommand::new("open").arg(path));
    #[cfg(target_os = "linux")]
    return Ok(PlatformCommand::new("xdg-open").arg(path));
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err("open is not supported on this platform".to_string());
}

fn run_command_with(
    command: &PlatformCommand,
    spawn: impl FnOnce(&PlatformCommand) -> std::io::Result<()>,
) -> Result<(), String> {
    spawn(command).map_err(|error| format!("open failed: {error}"))
}

pub(crate) fn open_path(path: &Path) -> Result<(), String> {
    let command = open_command(path)?;
    run_command_with(&command, PlatformCommand::spawn)
}
```

Use the same command-spec boundary for the macOS reveal command and Linux
`xdg-open` fallback without changing the existing FileManager1 behavior.
Unit tests assert program/argument construction and inject a failing spawn;
never launch desktop programs in tests. Keep command/path values out of tracing
events.

- [ ] **Step 6: Implement Timeline title, explanation, and export state**

Add a monotonic `next_export_operation_id` and this state to
`TimelineWorkspace`:

```rust
pub(crate) enum GuideExportState {
    Idle,
    PickingDestination {
        operation_id: u64,
        pending: PendingStandaloneExport,
    },
    Exporting { operation_id: u64 },
    Succeeded,
}
pub(crate) export_state: GuideExportState,
pub(crate) last_export: Option<StandaloneExportResult>,
```

Add messages:

```rust
GuideTitleChanged(String),
AnnotationExplanationChanged(AnnotationId, String),
ExportDirChosen { operation_id: u64, parent: Option<PathBuf> },
ExportFinished { operation_id: u64, result: Result<StandaloneExportResult, String> },
OpenExportedGuide,
ShowExportedGuideInFolder,
PlatformActionFinished(Result<(), String>),
```

`ExportRequested` acts only from `Idle`/`Succeeded`: build and validate the
owned job immediately, allocate the operation ID, set
`PickingDestination`, and start the picker. A matching
`ExportDirChosen { parent: None, .. }` drops the pending job and returns to
`Idle` without changing the banner. A matching chosen parent moves that exact
pending job into `StandaloneExportRequest`, sets `Exporting`, and returns
`Task::perform`. Matching completion retains workspace and paths; failure
resets `Idle` with a recoverable banner. Ignore every mismatched picker or
worker result. Remove `iced::exit()` from successful Guide export only.

In `view.rs`, add a Guide title text input in `header`. Disable export
controls while picking or exporting. Do not show a worker Cancel action because
started `spawn_blocking` tasks cannot be aborted. After success, render
`Open Guide` and `Show in Folder` buttons. In the annotation modal, render
one labeled text input for every live `NumberCallout`, ordered by navigator
items:

```rust
text_input(
    "Optional explanation",
    doc.explanations.get(&id).map(String::as_str).unwrap_or(""),
)
.on_input(move |text| Message::AnnotationExplanationChanged(id, text))
```

Text notes need no second explanation field.

- [ ] **Step 7: Run Timeline and platform tests**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo test -p rollshot-app platform_actions
rtk cargo fmt --all -- --check
rtk cargo clippy -p rollshot-app --all-targets --features action-guide -- -D warnings
```

Expected: all pass; standalone export does not exit and no worker error loses editable state.

- [ ] **Step 8: Commit the standalone flow**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-app/src
rtk git commit -m "feat(action): export guides without closing timeline"
```

---

### Task 7: Put the same reviewed job inside Issue Packs

**Files:**
- Modify: `crates/rollshot-action/src/gif.rs`
- Modify: `crates/rollshot-action/src/storyboard.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/{guide_export.rs,update.rs}`
- Modify: `crates/rollshot-action/src/export.rs`

**Interfaces:**
- Consumes: owned reviewed job and Task 6 Timeline adapter.
- Produces: owned `ActionGuideExportSource`, `ActionGuideIssueAssets::from_job`, `export_gif_images`, `render_reviewed_storyboard`, and Issue Pack folder/ZIP outputs containing required `action-guide/index.html`.

- [ ] **Step 1: Write failing Issue Pack parity and rollback tests**

Update existing Issue Pack Action Guide fixtures to own the job. Add:

```rust
fn issue_pack_test_job() -> ReviewedGuideExportJob {
    ReviewedGuideExportJob {
        title: "Checkout failure".into(),
        region: rollshot_action::CaptureRegion { x: 0, y: 0, width: 8, height: 8 },
        input_source: rollshot_action::InputSourceKind::LinuxEvdev,
        input_capability: rollshot_action::InputCapability::SemanticEvents,
        steps: vec![rollshot_action::ReviewedGuideStep {
            index: 1,
            title: "Submit order".into(),
            caption: Some("Confirm the request".into()),
            kind: rollshot_action::CandidateKind::Click,
            reason: rollshot_action::DetectReason::ClickConfirmed,
            at_ms: 100,
            image: rollshot_action::ReviewedStepImage::Retained(Arc::new(RgbaImage::new(8, 8))),
            hotspots: vec![rollshot_action::GuideHotspot {
                annotation_id: 1,
                bounds: rollshot_action::NormalizedRect { x: 0.1, y: 0.1, width: 0.2, height: 0.2 },
                explanation: "Open Settings".into(),
            }],
        }],
    }
}

fn owned_action_source() -> ActionGuideExportSource {
    ActionGuideExportSource {
        job: issue_pack_test_job(),
        include_gif: false,
        gif_frames: vec![Arc::new(RgbaImage::new(8, 8))],
    }
}

fn reviewed_issue_pack_input() -> IssuePackInput {
    let mut input = base_input();
    input.final_image = None;
    input.action_guide = Some(ActionGuideIssueAssets::from_job(&issue_pack_test_job(), false));
    input.evidence_review.required = true;
    input.evidence_review.completed = true;
    input.evidence_review.action_guide_keyframes_reviewed = true;
    input
}

#[test]
fn issue_pack_lists_and_writes_interactive_guide() {
    let parent = tempfile::tempdir().unwrap();
    let input = reviewed_issue_pack_input();
    let result = export_folder_with_action_guide(&input, Some(owned_action_source()), parent.path()).unwrap();
    assert!(result.directory.join("action-guide/index.html").is_file());
    let issue = std::fs::read_to_string(result.directory.join("issue.md")).unwrap();
    assert!(issue.contains("`action-guide/index.html`"));
    let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"action_html\""));
}

#[test]
fn issue_pack_html_failure_rolls_back_outer_transaction() {
    let parent = tempfile::tempdir().unwrap();
    let mut source = owned_action_source();
    source.job.steps[0].hotspots[0].explanation.clear();
    assert!(export_folder_with_action_guide(&reviewed_issue_pack_input(), Some(source), parent.path()).is_err());
    assert!(std::fs::read_dir(parent.path()).unwrap().next().is_none());
}
```

Add a ZIP assertion that `action-guide/index.html` exists in the archive.

- [ ] **Step 2: Run the Issue Pack tests in RED state**

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack_lists_and_writes_interactive_guide
rtk cargo test -p rollshot-app --features action-guide issue_pack_html_failure_rolls_back_outer_transaction
```

Expected: compilation fails because Issue Pack still accepts borrowed
`Guide`/`FrameStore` inputs and the required HTML asset does not exist.

- [ ] **Step 3: Refactor GIF encoding to accept owned shared frames**

Extract the encoder body in `gif.rs`:

```rust
pub fn export_gif_images<'a>(
    images: impl IntoIterator<Item = &'a RgbaImage>,
    opts: GifOptions,
    out_path: &Path,
) -> Result<(), GifError> {
    let images = images.into_iter().map(|image| downscale(image, opts.max_width)).collect::<Vec<_>>();
    if images.is_empty() { return Err(GifError::Empty); }
    encode_images(images, opts.frame_dwell_ms, out_path)
}

fn encode_images(
    images: Vec<RgbaImage>,
    frame_dwell_ms: u32,
    out_path: &Path,
) -> Result<(), GifError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut bytes);
        encoder.set_repeat(Repeat::Infinite).map_err(|source| GifError::Encode { source })?;
        for image in images {
            let delay = Delay::from_numer_denom_ms(frame_dwell_ms, 1);
            encoder.encode_frame(Frame::from_parts(image, 0, 0, delay))
                .map_err(|source| GifError::Encode { source })?;
        }
    }
    write_atomic(out_path, &bytes)
}
```

Keep `export_gif(guide, store, ...)` as a compatibility adapter that validates retained keyframes and calls `export_gif_images`. This preserves raw reviewed-keyframe GIF semantics.

- [ ] **Step 4: Make Issue Pack Action Guide input owned**

Replace the lifetime-bearing type with:

```rust
pub(crate) struct ActionGuideExportSource {
    pub job: rollshot_action::ReviewedGuideExportJob,
    pub include_gif: bool,
    pub gif_frames: Vec<Arc<RgbaImage>>,
}
```

Implement Issue Pack metadata directly from the job:

```rust
impl ActionGuideIssueAssets {
    pub(crate) fn from_job(job: &ReviewedGuideExportJob, include_gif: bool) -> Self {
        let steps = job.steps.iter().enumerate().map(|(offset, step)| IssuePackStep {
            index: step.index,
            title: step.title.clone(),
            caption: step.caption.clone(),
            keyframe_path: format!("action-guide/keyframes/{:03}.png", offset + 1),
        }).collect();
        Self { steps, include_gif }
    }
}
```

`build_folder` calls
`render_guide_folder(&action.job, &tmp_dir.join("action-guide"))`. GIF uses
`action.gif_frames.iter().map(Arc::as_ref)`. Storyboard calls
`render_reviewed_storyboard(&action.job, StoryboardOptions::default())`
inside this same worker-owned build. Keep the existing optional Storyboard
warning result and the existing outer folder/ZIP transaction.

In `storyboard.rs`, add
`render_reviewed_storyboard(&ReviewedGuideExportJob, StoryboardOptions)`.
Factor the current layout/composition body so the new adapter calls
`ReviewedStepImage::with_flattened_image` and downsizes each reviewed step
inside that callback before retaining the card. It must never collect
full-resolution flattened images. Reuse the existing
`max_canvas_pixels` validation and return the existing warning-compatible
`StoryboardError`. Add a test with several annotated 4K inputs that asserts
the retained cards obey the configured width/canvas bounds and the reviewed
redaction pixels appear in the Storyboard.

Add `action-guide/index.html` to issue Markdown attachments and `AssetEntry { kind: "action_html", path: "action-guide/index.html" }` to the manifest.

- [ ] **Step 5: Build the owned source before starting Issue Pack work**

Replace the two independently derived helpers with one preparation boundary so
Issue Pack metadata and files cannot observe different Guide states:

```rust
fn prepare_issue_pack_export(
    state: &TimelineWorkspace,
) -> Result<(IssuePackInput, ActionGuideExportSource), String> {
    let include_gif = state.issue_pack.as_ref().is_some_and(|dialog| dialog.include_gif);
    let job = build_reviewed_export_job(state).map_err(|error| error.to_string())?;
    let assets = ActionGuideIssueAssets::from_job(&job, include_gif);
    let input = timeline_issue_pack_input(state, assets);
    let gif_frames = state.guide.steps().iter().enumerate().map(|(offset, step)| {
        state.store.retained(step.keyframe)
            .map(|frame| Arc::clone(&frame.image))
            .ok_or_else(|| format!("step {} keyframe is unavailable", offset + 1))
    }).collect::<Result<Vec<_>, _>>()?;
    Ok((input, ActionGuideExportSource {
        job,
        include_gif,
        gif_frames,
    }))
}
```

Change `timeline_issue_pack_input` to accept the already-derived
`ActionGuideIssueAssets` instead of calling `from_guide`.
`begin_issue_pack_export` must call `prepare_issue_pack_export` before
opening the picker, store that owned pair with the new operation ID, and fail
before the picker/filesystem if preparation returns an error. A matching
`IssuePackFolderChosen` moves the stored pair into the worker; it must not
re-read Timeline state.

Move folder/ZIP filesystem work into:

```rust
async fn run_issue_pack_export(
    input: IssuePackInput,
    action: ActionGuideExportSource,
    parent: PathBuf,
    kind: IssuePackKind,
) -> Result<IssuePackExportResult, String> {
    tokio::task::spawn_blocking(move || match kind {
        IssuePackKind::Folder => export_folder_with_action_guide(&input, Some(action), &parent),
        IssuePackKind::Zip => export_zip_with_action_guide(&input, Some(action), &parent),
    })
    .await
    .map_err(|_| "Issue Pack export worker failed".to_string())?
    .map_err(|error| error.to_string())
}
```

Return it through the existing `Message::IssuePackFinished` path.

Allocate an Issue Pack operation ID before opening its picker and carry it
through `IssuePackFolderChosen` and `IssuePackFinished`. Freeze the common
job before the picker, disable duplicate Export/modal Cancel controls while
the picker or worker is active, and ignore stale completions. Picker
cancellation drops the pending owned source. Add reducer tests for picker
cancellation and a mismatched completion ID. This worker has the same
no-mid-render-cancellation policy as standalone export.

- [ ] **Step 6: Remove the old borrowed Guide exporter**

After all call sites use `render_guide_folder`, remove deprecated `export_guide`, its fixed `action-guide` naming, and `swap_into_place`. Remove the `export_guide` re-export from `lib.rs`. Preserve renderer rollback tests under the new API.

- [ ] **Step 7: Run Action Guide and Issue Pack tests**

```bash
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide issue_pack
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets --features rollshot-app/action-guide -- -D warnings
```

Expected: all pass; standalone and Issue Pack keyframes/HTML/Storyboard derive
from the same job, all pixel and filesystem work is off the iced update thread,
and GIF remains an optional raw-reviewed-keyframe derivative.

- [ ] **Step 8: Commit Issue Pack integration**

```bash
rtk git add crates/rollshot-action/src crates/rollshot-app/src/issue_pack.rs crates/rollshot-app/src/timeline_workspace
rtk git commit -m "feat(action): include interactive guides in issue packs"
```

---

### Task 8: Document and verify the complete feature

**Files:**
- Modify: `README.md`
- Test: all files changed in Tasks 1–7

**Interfaces:**
- Consumes: completed standalone and Issue Pack flows.
- Produces: user-facing opening instructions and final verification evidence.

- [ ] **Step 1: Add README instructions**

In the Action Guide export section, document the exact folder:

```text
<guide-title>-<YYYY-MM-DD-HHMMSS>/
  index.html
  steps.md
  session.json
  keyframes/
```

State that recipients double-click `index.html`, the reader works offline without a server, the whole folder must be moved together, `steps.md` is the no-JavaScript fallback, OCR search and single-file HTML are not included, and `Open Guide`/`Show in Folder` appear after standalone export.

- [ ] **Step 2: Run complete automated verification**

```bash
rtk cargo test --workspace --exclude rollshot-ocr --features rollshot-app/action-guide
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets --features rollshot-app/action-guide -- -D warnings
rtk npm --prefix scripts/html-guide-e2e test
```

Expected: all Rust, Chromium, Firefox, and WebKit tests pass.

- [ ] **Step 3: Inspect privacy and affected flows**

Run code-review-graph `detect_changes`, `get_affected_flows`, and `tests_for` for `render_guide_folder`, Timeline `update`, and Issue Pack `build_folder`. Confirm no untested path writes raw annotated source frames to `keyframes/`, and run:

```bash
rtk rg -n "println!|eprintln!|dbg!" crates/rollshot-action/src/export.rs crates/rollshot-action/src/export crates/rollshot-app/src/timeline_workspace/guide_export.rs
rtk rg -n "fetch\(|https?://|localStorage|serviceWorker" crates/rollshot-action/src/export/viewer.html
```

Expected: no runtime print diagnostics in product paths and no viewer network/storage API.

- [ ] **Step 4: Perform Linux runtime verification**

Build/run the `action-guide` product path on Linux, export a Guide with a text note, explained callout, and opaque redaction, then verify:

1. Timeline remains open.
2. `Open Guide` launches `index.html` and `Show in Folder` reveals it.
3. Chrome and Firefox open the moved folder through `file://`.
4. Search, keyboard, zoom, hotspot, copy success/fallback, theme, and missing-image recovery work.
5. Issue Pack folder and ZIP contain the same required reader.

- [ ] **Step 5: Perform macOS runtime verification**

On macOS, repeat the active Action Guide product flow and verify `open`, Finder reveal, Chrome, Firefox, and real Safari. Move/extract standalone and Issue Pack folders before opening them. Record any browser-specific clipboard fallback as expected behavior, not export failure.

- [ ] **Step 6: Commit documentation**

```bash
rtk git add README.md
rtk git commit -m "docs(action): document offline guide reader"
```

- [ ] **Step 7: Request final code review**

Invoke `superpowers:requesting-code-review` against the approved spec and this plan. Resolve findings with `superpowers:receiving-code-review`, rerun Step 2 verification, then use `superpowers:verification-before-completion` before claiming the feature complete.
