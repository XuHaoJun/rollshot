# Two-Point Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Slice 2 of the annotation-editor program: complete Line and Arrow creation, editing, styling, history, Navigator, live rendering, and full-resolution output with shared two-point geometry and Shift snapping.

**Architecture:** `rollshot-image-document` owns the committed `TwoPoint` annotation, validation, history, image-space geometry, hit testing, Navigator semantics, render commands, and raster flattening. `rollshot-app` owns independent persisted Line/Arrow defaults, transient width/color previews, screen-space gesture thresholds, modifier-aware drafts, endpoint handles, responsive toolbar routing, and keyboard shortcuts. Both Result Workspace and Timeline Canvas consume the same framework-neutral render commands; only Result Workspace gains creation and editing UX.

**Tech Stack:** Rust, `image`, serde/TOML, iced 0.14 built-in widgets and `Canvas`, existing snapshot history, Result Workspace Elm architecture.

**Source specifications:**

- Umbrella: `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md`
- Approved slice spec: `docs/superpowers/specs/2026-07-13-two-point-tools-design.md`
- Product reference survey: `docs/researchs/annotation-tools-reference-survey.md`

## Global Constraints

- Line and Arrow use one `Annotation::TwoPoint` model with `TwoPointKind::{Line, Arrow}`.
- Canonical `StrokeStyle` is `#E5484D`, width `4.0` full-resolution image pixels, and opacity `1.0`.
- The committed UI exposes color and integer width `1..=16`; it exposes no opacity or arrowhead selector.
- Arrow uses one filled-triangle head at the drag-release endpoint: length `clamp(width × 6, 16, 32)` and half-width `clamp(width × 3, 8, 16)` image pixels.
- Shift snaps creation and endpoint edits to the nearest 45-degree increment while preserving radial distance; body movement is unconstrained.
- Gestures shorter than `4` screen pixels are cancelled without a document edit.
- One completed create, move, endpoint edit, or property interaction creates at most one history entry; previews and cancelled edits create none.
- Line and Arrow defaults persist independently. Editing a selected annotation never changes either tool default.
- Result Workspace Line/Arrow defaults always load and save opacity `1.0`; a
  non-opaque config value falls back to `1.0` with one warning.
- Wide and Compact show adjacent Line and Arrow tools. Narrow keeps Arrow visible and moves Line into More with active-tool visibility.
- Tooltips are exactly `Line (L) — Shift: Snap to 45°` and `Arrow (A) — Shift: Snap to 45°`.
- Copy and Save flatten full-resolution committed state only. Drafts, hover, selection, and handles never enter output.
- Existing Number, Text, Opaque Redaction, automation, workbench, Action Guide, OCR, and Timeline behavior remains compatible.
- Result Workspace behavior is shared on Linux and macOS; runtime verification is required on both before Slice 2 is marked Complete.
- Use iced 0.14 built-ins and the existing Canvas. Do not add Shader, a custom `Widget`, or a custom `Overlay`.
- Do not add later-slice tools, a generic path annotation, arrow variants, opacity UI, or any `rollshot-core` stitching change.
- Every shell command in this repository is prefixed with `rtk`.

## File Structure

### New files

- `crates/rollshot-image-document/src/two_point.rs` — pure image-space segment distance, arrowhead, bounds, and triangle-containment geometry.
- `crates/rollshot-app/src/result_workspace/two_point.rs` — pure Shift constraint and screen-space gesture-threshold helpers.

### Existing files

- `crates/rollshot-image-document/src/style.rs` — `StrokeStyle` and canonical defaults.
- `crates/rollshot-image-document/src/annotation.rs` — `TwoPointKind`, `Annotation::TwoPoint`, constructors, identity, and anchor.
- `crates/rollshot-image-document/src/edit_op.rs` — typed add/point/style operations.
- `crates/rollshot-image-document/src/document.rs` — atomic validation, public edit API, history, and clamping.
- `crates/rollshot-image-document/src/shapes.rs` — `RenderShape::Line` and Line/Arrow command lowering.
- `crates/rollshot-image-document/src/hit.rs` — endpoint/body hit parts and TwoPoint hit testing.
- `crates/rollshot-image-document/src/raster.rs` — anti-aliased finite-segment stroke rasterization.
- `crates/rollshot-image-document/src/flatten.rs` — line-command flattening and pixel tests.
- `crates/rollshot-image-document/src/navigator.rs` — Line/Arrow labels and ordering tests.
- `crates/rollshot-image-document/src/lib.rs` — public exports.
- `crates/rollshot-app/src/result_workspace/annotation_defaults.rs` — independent Line/Arrow persistence and fallback.
- `crates/rollshot-app/src/result_workspace/properties.rs` — TwoPoint targets, stroke color/width transactions, controls, and app-only preview.
- `crates/rollshot-app/src/result_workspace/canvas.rs` — tools, drafts, render command, endpoint handles, and modifier-aware edit preview.
- `crates/rollshot-app/src/result_workspace/view.rs` — pass current workspace modifiers into the Canvas program explicitly.
- `crates/rollshot-app/src/result_workspace/update.rs` — gesture lifecycle, typed commits, defaults/property updates, Esc, and shortcuts.
- `crates/rollshot-app/src/result_workspace/toolbar.rs` — density routing, More active state, and exact tooltips.
- `crates/rollshot-app/src/result_workspace/mod.rs` — register the focused app geometry module.
- `crates/rollshot-app/src/timeline_workspace/annotation.rs` — exhaustively draw the shared line command without adding creation UX.
- `crates/rollshot-app/src/timeline_workspace/update.rs`, `crates/rollshot-app/src/result_workspace/secure_sharing.rs`, and `crates/rollshot-app/src/result_workspace/ocr_text.rs` — audited compatibility consumers; change only if an explicit test demonstrates a required behavior change.
- `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md` — lifecycle evidence at Handoff or Complete.

## Task Dependencies

1. Task 1 records the lifecycle transition and establishes additive shared value types.
2. Task 2 atomically adds the committed model, typed edit API, framework-neutral geometry, hit testing, live commands, and flattening.
3. Task 3 extends Slice 1 defaults and transactional properties.
4. Task 4 wires modifier-aware creation and editing gestures.
5. Task 5 exposes the tools through responsive toolbar and keyboard routing.
6. Task 6 runs integrated regression and platform verification and records the lifecycle outcome.

Each task ends in a green commit. Do not land an enum variant in one commit while leaving exhaustive workspace consumers uncompilable.

## Engineering Review Record (auto mode, 2026-07-13)

### Step 0 — Scope Challenge

- **Goal alignment:** all six tasks are required for the approved Line/Arrow vertical slice. Task 6 is acceptance and lifecycle evidence, not a product expansion.
- **Minimum viable plan:** Tasks 1–6 are the minimum complete slice because omitting persistence/properties, gestures, discoverability, or full-resolution/platform verification would leave an approved user-facing requirement incomplete. No task is deferred.
- **Complexity check:** 2 net-new files, 2 focused modules, and 6 tasks. None of the thresholds (>12 net-new files, >2 top-level modules/crates, >10 tasks) trigger.
- **Search check:** no new concurrency or infrastructure is introduced. The plan reuses iced 0.14's built-in `Slider::on_release`, built-in `Canvas`, and existing snapshot/history and atomic config-writing flows, verified against the repository's pinned iced 0.14 reference and current code. No web search is needed for an unpinned or novel pattern.
- **Completeness check:** complete negative-path, rollback, preview-isolation, platform, and long-image coverage is retained; no AI-time shortcut is accepted.
- **Distribution check:** no new binary, crate, container, or installable artifact is introduced; existing Rollshot application distribution remains unchanged.

### What already exists

- `ImageDocument` already owns snapshots, undo/redo, batch preflight, clamping, stable IDs, and dirty `state_id`; Task 2 extends these paths instead of creating a second history system.
- `RenderShape::Triangle`, `blend_px`, Canvas shape dispatch, and `flatten()` already provide the shared live/output pipeline; Task 2 adds only a finite line primitive and TwoPoint lowering.
- Result Workspace already owns transient color transactions, persisted per-tool defaults, selection replacement previews, modifier tracking, and Canvas event routing; Tasks 3–4 extend those mechanisms.
- Toolbar density/More routing and captured-input shortcut precedence already exist; Task 5 inserts Line/Arrow into the existing model.
- The existing 100-annotation long-image test and Linux/macOS Result Workspace paths already provide scale and platform acceptance harnesses; Task 6 extends them.

### NOT in scope

- Generic paths, freehand, shapes, blur, crop, and later annotation slices — not required for the approved two-point slice.
- Arrowhead variants, opacity controls, dashed strokes, and per-end decorations — explicitly excluded by the approved spec.
- Timeline creation/editing UX — Timeline only consumes the shared render command in this slice.
- Automation proposal operations, OCR semantics, Action Guide creation, and secure-sharing classification changes — TwoPoint remains an ordinary visible annotation.
- New Shader/custom Widget/custom Overlay work — iced built-ins and the current Canvas are sufficient.
- `rollshot-core` stitching changes or benchmarks — annotation rendering does not touch stitching hot paths.
- Packaging or CI distribution changes — this slice creates no new artifact.

### Reviewed data and test flow

```text
persisted tool default ──press──> raw image-space draft ──Shift/zoom──> Canvas preview
                                  │                         │
                                  │ release point           └── never enters ImageDocument
                                  v
                         validated EditOp batch
                                  │
                     ┌────────────┴────────────┐
                     v                         v
             commit one Snapshot       reject + restore full Snapshot
                     │                  (annotations/counters/next_id)
                     v
          Annotation::TwoPoint ──annotation_shapes──> Line + optional Triangle
                     │                         ├── Result Workspace Canvas
                     │                         ├── Timeline Canvas
                     │                         └── full-resolution flatten
                     └── hit test / Navigator / undo-redo

TDD layers:
value types → document transactions → pure geometry → render/hit/flatten
            → defaults/properties → gestures/modifiers → toolbar/shortcuts
            → integrated output + Linux/macOS acceptance
```

### Auto decisions

#### Auto decision D1 — Should failed batches restore allocated annotation IDs?
Context: `ImageDocument::Snapshot` currently omits `next_id`, so a batch can fail after allocation and still consume an ID.
ELI10: A failed edit should behave as if it never happened. If its hidden ID counter moves forward, later successful annotations expose evidence of an edit the user never got.
Stakes if we pick wrong: rejected automation or UI batches silently create gaps and violate atomic rollback.
Recommendation: **1A** because a full snapshot is the explicit, boring atomicity boundary.
Completeness: A=10/10, B=5/10.
Pros / cons:
A) **1A — Add `next_id` to snapshot/restore plus regression test (recommended)** (human: ~1 hour / AI: ~10 min; low risk; negligible maintenance) ✅ restores true atomicity and undo/redo identity; ❌ slightly broadens an existing private snapshot.
B) **1B — Accept monotonic ID gaps** (human: 0 / AI: 0; medium semantic risk; hidden maintenance debt) ✅ no code change; ❌ contradicts the batch contract and masks partial mutation.
Net: pay a tiny private-state cost to make rollback truthful.

#### Auto decision D2 — Where should Canvas read current Shift state?
Context: the plan referenced nonexistent `editor.modifiers`; modifiers actually live on `ResultWorkspace`.
ELI10: The preview needs the same Shift key state as the release handler. Passing it explicitly prevents the picture from disagreeing with what gets committed.
Stakes if we pick wrong: the plan does not compile or the snapped preview differs from the saved annotation.
Recommendation: **2A** because explicit data flow is smaller than relocating shared editor state.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) **2A — Add `modifiers` to `AnnotationCanvas` and pass it from `view.rs` (recommended)** (human: ~1 hour / AI: ~10 min; low risk; low maintenance) ✅ mirrors current ownership and is directly testable; ❌ adds one Canvas input.
B) **2B — Move modifiers into `EditorState`** (human: ~0.5 day / AI: ~30 min; medium regression risk; medium maintenance) ✅ Canvas could keep reading only editor state; ❌ needlessly changes wheel/keyboard ownership across the workspace.
Net: pass one value instead of redesigning state ownership.

#### Auto decision D3 — How should no-op TwoPoint updates avoid history entries?
Context: `apply_batch` commits every non-empty successful batch even when update values are unchanged.
ELI10: Moving an endpoint to where it already is should not make Undo look like it did something. One mutation check at the batch boundary also keeps direct and batch APIs consistent.
Stakes if we pick wrong: no-op property or gesture releases pollute history and dirty state.
Recommendation: **3A** because one transactional path is DRY and complete.
Completeness: A=10/10, B=8/10.
Pros / cons:
A) **3A — Detect snapshot changes before batch commit; delegate new public methods through one-op batches (recommended)** (human: ~0.5 day / AI: ~25 min; low-to-medium risk; low maintenance) ✅ fixes direct and batch semantics together; ❌ requires regression tests for existing batch behavior.
B) **3B — Precheck only the new direct methods** (human: ~2 hours / AI: ~15 min; low immediate risk; medium maintenance) ✅ minimizes the touched batch code; ❌ leaves equivalent `EditOp` no-ops with different history semantics.
Net: make the existing transaction boundary authoritative rather than duplicating policy.

#### Auto decision D4 — When should the umbrella lifecycle enter `In progress`?
Context: the registry is `Planned`, but execution steps did not record the required start transition.
ELI10: The umbrella is the handoff ledger. Updating it before code tells the next person whether work has actually started and where to resume.
Stakes if we pick wrong: branch state and lifecycle evidence drift, making interruption recovery ambiguous.
Recommendation: **4A** because lifecycle truth should precede implementation.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) **4A — Add a committed Task 1 Step 0 transition to `In progress` (recommended)** (human: ~10 min / AI: ~5 min; very low risk; low maintenance) ✅ preserves an auditable transition; ❌ adds one docs-only commit.
B) **4B — Wait until Task 1's feature commit** (human: 0 / AI: 0; low code risk; medium process risk) ✅ fewer commits; ❌ the registry remains stale during active work.
Net: one small commit buys reliable resumability.

#### Auto decision D5 — Should task file lists and commits name every compatibility consumer exactly?
Context: Task 2 staged files not declared in its file list, while Task 6 used conditional modification wording.
ELI10: An executor should know before starting which files a task owns. Wildcards and “if needed” make review boundaries and accidental edits harder to spot.
Stakes if we pick wrong: commits can include unrelated changes or miss an exhaustive match needed for a green workspace.
Recommendation: **5A** because explicit file ownership is safer for both humans and agents.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) **5A — Declare known exhaustive files, narrow `git add`, and mark other consumers verify-only (recommended)** (human: ~1 hour / AI: ~15 min; low risk; low maintenance) ✅ aligns steps, file lists, and commits; ❌ compiler discoveries still require updating the live plan before touching a new file.
B) **5B — Keep broad directory staging and conditional files** (human: 0 / AI: 0; medium staging risk; medium maintenance) ✅ tolerates surprises without plan edits; ❌ weakens surgical-change and review guarantees.
Net: prefer an amendable precise plan over speculative staging.

#### Auto decision D6 — How should color-only and width-only previews compose?
Context: current `preview_annotation` returns early without a color transaction, and `Annotation::with_stroke_style` was left as an ambiguous helper choice.
ELI10: Width dragging must show a preview even when the color picker is closed. An exact local reconstruction keeps that preview out of the document and avoids inventing a mutating model API.
Stakes if we pick wrong: the width slider appears unresponsive or preview code bypasses document history.
Recommendation: **6A** because explicit local reconstruction is the smallest safe contract.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) **6A — Rewrite `preview_annotation` to accept either transaction and reconstruct `TwoPoint` explicitly (recommended)** (human: ~2 hours / AI: ~20 min; low risk; low maintenance) ✅ width-only/color-only previews work without public mutation helpers; ❌ the variant fields are written once in app code.
B) **6B — Add public `Annotation::with_stroke_style`** (human: ~2 hours / AI: ~15 min; medium boundary risk; medium maintenance) ✅ shortens app matching; ❌ expands the framework-neutral API for one UI-only use.
Net: keep preview mutation local and document mutation transactional.

#### Auto decision D7 — Which point is authoritative when pointer release arrives without a final move event?
Context: the release pseudocode used stored `raw_current`, which may lag the actual release coordinate.
ELI10: Operating systems do not promise a move event immediately before release. The release event itself is the final user intent and must drive both threshold and commit.
Stakes if we pick wrong: quick drags end at the previous cursor position or are wrongly cancelled.
Recommendation: **7A** because the terminal event is the canonical input.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) **7A — Clamp/store the release point, then constrain and commit from it; add a no-final-move test (recommended)** (human: ~2 hours / AI: ~20 min; low risk; low maintenance) ✅ eliminates stale-coordinate behavior and preserves preview/commit equality; ❌ release handling gains one explicit update.
B) **7B — Keep the last move point** (human: 0 / AI: 0; medium UX risk; hidden maintenance) ✅ simpler pseudocode; ❌ depends on an event-order guarantee iced/OS input does not provide.
Net: trust the event that ends the gesture.

#### Auto decision D8 — Are Task 6 integration tests RED tests or acceptance tests?
Context: Tasks 1–5 already implement the behavior, so Task 6 cannot honestly predict that newly added integration tests fail.
ELI10: A RED step is useful only before behavior exists. At the end of a vertical slice, these tests should pass and expose an integration defect if they do not.
Stakes if we pick wrong: executors may treat a real regression as an expected failure.
Recommendation: **8A** because exact Run/Expected pairs matter more than ritual wording.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) **8A — Label them acceptance tests with Expected: PASS (recommended)** (human: ~15 min / AI: ~5 min; very low risk; low maintenance) ✅ makes failures actionable; ❌ they are not a fresh RED cycle.
B) **8B — Move the tests into earlier implementation tasks** (human: ~0.5 day / AI: ~30 min; medium plan churn; low maintenance) ✅ preserves strict RED timing; ❌ splits cross-layer acceptance setup across unrelated tasks.
Net: keep end-to-end acceptance together and tell the truth about expected results.

#### Auto decision D9 — Which negative paths need explicit automated coverage before runtime checks?
Context: rollback IDs, no-op history, width-only preview cancellation, target changes, and release-without-move were not all asserted.
ELI10: These are the cases users hit when they cancel, click quickly, or retry after invalid input. They are cheap deterministic tests and expensive manual regressions.
Stakes if we pick wrong: silent dirty-state/history errors reach both platforms despite happy-path tests passing.
Recommendation: **9A** because completeness is inexpensive here and manual-only coverage is fragile.
Completeness: A=10/10, B=7/10.
Pros / cons:
A) **9A — Add all five focused negative/regression cases in Tasks 2–4 (recommended)** (human: ~0.5 day / AI: ~35 min; low risk; low maintenance) ✅ locks each failure at its owning layer; ❌ adds several focused tests.
B) **9B — Rely on final runtime checklist for cancellation and event ordering** (human: ~1 hour / AI: ~10 min; medium regression risk; high recurring manual cost) ✅ fewer tests now; ❌ failures are slower and less deterministic to diagnose.
Net: automate deterministic edge cases and reserve manual checks for rendering feel and native integration.

#### Auto decision D10 — Should the long-image acceptance allocate a wider second fixture?
Context: the repository already tests a 1000×20,000 image; the proposed replacement widened it to 1200 pixels and duplicated scale work.
ELI10: Full-resolution RGBA images are large, and flattening holds another full image. Reusing the current dimensions tests the same behavior without adding roughly 40 MB of peak pixel storage.
Stakes if we pick wrong: local/CI tests become slower or memory-sensitive without increasing coverage.
Recommendation: **10A** because performance tests should control resource cost while preserving the scale invariant.
Completeness: A=10/10, B=10/10.
Pros / cons:
A) **10A — Extend the existing 1000×20,000 test to 100 mixed annotations (recommended)** (human: ~1 hour / AI: ~15 min; low risk; low maintenance) ✅ preserves long-image and mixed-tool coverage at existing peak dimensions; ❌ rewrites the current fixture mix.
B) **10B — Add/keep a 1200×20,000 fixture** (human: ~30 min / AI: ~10 min; low correctness risk; higher resource maintenance) ✅ leaves the old test untouched; ❌ duplicates coverage and raises peak memory/time.
Net: same correctness signal, lower CI resource exposure.

### Test coverage

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / value defaults and distinct kinds | ✓ | — | — | no |
| Task 2 / constructor, validation, atomic rollback including `next_id`, no-op history | ✓ | ✓ | — | no |
| Task 2 / arrow geometry, bounds, endpoint/shaft/head hit priority | ✓ | ✓ | — | no |
| Task 2 / shared lowering, Canvas consumers, alpha/edge raster, immutable flatten | ✓ | ✓ | — | no |
| Task 2 / Navigator labels and ordering | ✓ | ✓ | — | no |
| Task 3 / independent config round-trip, fallback, warning, write failure | ✓ | ✓ | — | no |
| Task 3 / color/width preview, Apply/Cancel/target switch, one-entry undo | ✓ | ✓ | — | no |
| Task 4 / 45° snap, zoom threshold, live modifier toggle, release-without-move | ✓ | ✓ | — | no |
| Task 4 / endpoint/body edits, Esc/delete/undo/redo | ✓ | ✓ | — | no |
| Task 5 / responsive routing, More active state, exact hints, shortcut precedence | ✓ | ✓ | — | no |
| Task 6 / committed-only Copy/Save, Copy Original, 100-annotation long image | ✓ | ✓ | ✓ | no |
| Task 6 / Linux and macOS native dialogs, clipboard, visual legibility | — | — | ✓ | yes |

### Failure modes

| Codepath | Realistic failure | Automated coverage / handling | User-visible result |
|---|---|---|---|
| Document add/update batch | non-finite/coincident point or invalid style after an earlier allocation | Task 2 Steps 1–3; typed `EditError`, full snapshot restore including `next_id` | inline error for UI commits; no mutation/history |
| Document no-op update | identical endpoint/style creates dirty state | Task 2 Steps 1–3; snapshot-change guard returns success without commit | no message; correctly no visible change/history |
| Geometry/raster | short or edge-clipped arrow produces non-finite/out-of-bounds sampling | Task 2 Steps 4–10; validated non-coincident points, bounded raster loop | valid clipped annotation, no panic |
| Defaults load/save | malformed field, non-opaque value, or atomic write failure | Task 3 Steps 1–7; per-field fallback and existing `Result` writer | one warning; in-memory value retained on save failure |
| Property preview | width-only transaction, Cancel/Esc, or target switch leaks into document | Task 3 Steps 4–7; app-only clone and transaction clearing | preview reverts; no dirty/history entry |
| Gesture input | modifier changes mid-drag or release arrives without final move | Task 4 Steps 4–8; explicit Canvas modifiers and canonical release point | preview and committed endpoint agree |
| Gesture validation | sub-threshold or clamped-coincident gesture | Task 4 Steps 4–8 plus Task 2 `EditError`; cancel or inline error | no annotation for short gesture; error for invalid commit |
| Toolbar/keyboard | narrow active tool disappears or focused input consumes L/A | Task 5 Steps 1–6; More active model and captured-input early return | active Line remains visible in More; typing unaffected |
| Output/privacy | draft/handles enter output or TwoPoint becomes a redaction mask | Task 6 Steps 1–4; flatten reads committed graph, explicit redaction-only audit | output contains only committed visible strokes |
| Platform handoff | Linux/macOS clipboard or Save As behavior differs/unavailable | Task 6 Steps 5–7; failed/unavailable check forces `Handoff` | lifecycle records exact remaining risk; never false Complete |

**Critical silent gaps:** none after applying D1–D10.

### Performance and resources

- Rasterization visits only the line's expanded bounding box; there is no full-image scan per stroke beyond the existing final flatten traversal.
- Live Canvas continues existing visible-rectangle culling and emits one line plus at most one triangle per TwoPoint annotation.
- The long-image acceptance test reuses the existing 1000×20,000 RGBA dimensions and replaces its annotation mix; it does not add a second oversized fixture.
- No capture/stitching loop, pixel-format conversion, I/O queue, or system resource lifecycle changes; stitching benchmarks are not applicable.

### Parallelization strategy

Sequential execution, no parallelization opportunity. Tasks 1–2 establish shared public enums and exhaustive matches; Tasks 3–5 all modify `crates/rollshot-app/src/result_workspace/`; Task 6 consumes every earlier task and records lifecycle evidence. Parallel worktrees would create avoidable conflicts and are forbidden by repository instructions unless explicitly requested.

---

### Task 1: Add the shared two-point value types without changing annotations

**Files:**
- Modify: `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md`
- Modify: `crates/rollshot-image-document/src/style.rs`
- Modify: `crates/rollshot-image-document/src/annotation.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`
- Test: inline `#[cfg(test)]` modules in the same files

**Interfaces:**
- Produces: `TwoPointKind::{Line, Arrow}`.
- Produces: `StrokeStyle { color: Rgb8, width: f32, opacity: f32 }` and `StrokeStyle::default()`.
- Does not change `Annotation`, `EditOp`, document behavior, or any exhaustive
  match; the commit remains additive and workspace-green.

- [ ] **Step 0: Record Slice 2 as In progress before implementation**

Update the Slice 2 umbrella row from `Planned` to `In progress`, preserving the registered spec/plan links and hashes. Record this branch and Task 1 as the current entry point.

```bash
rtk git add docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md
rtk git commit -m "docs(annotation): start two-point tool slice"
```

Expected: the lifecycle transition is committed on `feat/annotation-two-point-tools` before product code changes.

- [ ] **Step 1: Write failing value-type tests**

Add these tests before defining the types:

```rust
#[test]
fn canonical_stroke_style_is_reviewed_opaque_accent() {
    assert_eq!(
        StrokeStyle::default(),
        StrokeStyle {
            color: Rgb8::new(0xE5, 0x48, 0x4D),
            width: 4.0,
            opacity: 1.0,
        }
    );
}

#[test]
fn two_point_kinds_are_distinct_and_copyable() {
    let line = TwoPointKind::Line;
    let arrow = TwoPointKind::Arrow;
    assert_ne!(line, arrow);
    assert_eq!(line, TwoPointKind::Line);
}
```

- [ ] **Step 2: Run the focused tests and confirm they fail for missing types**

Run:

```bash
rtk cargo test -p rollshot-image-document canonical_stroke_style_is_reviewed_opaque_accent
rtk cargo test -p rollshot-image-document two_point_kinds_are_distinct_and_copyable
```

Expected: both commands fail to compile because `StrokeStyle` and `TwoPointKind` do not exist.

- [ ] **Step 3: Add only the value types and exports**

Implement the following public shape, preserving every existing variant and accessor:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StrokeStyle {
    pub color: Rgb8,
    pub width: f32,
    pub opacity: f32,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            color: Rgb8::new(0xE5, 0x48, 0x4D),
            width: 4.0,
            opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TwoPointKind {
    Line,
    Arrow,
}
```

Define `TwoPointKind` next to `Annotation` but do not add an annotation variant yet. Export `StrokeStyle` and `TwoPointKind` from `lib.rs`.

- [ ] **Step 4: Run the value-type tests and full workspace check**

Run:

```bash
rtk cargo test -p rollshot-image-document
rtk cargo check --workspace --all-targets
```

Expected: all document tests pass and the additive types leave every workspace target compiling.

- [ ] **Step 5: Commit the additive types**

```bash
rtk git add crates/rollshot-image-document/src/style.rs crates/rollshot-image-document/src/annotation.rs crates/rollshot-image-document/src/lib.rs
rtk git commit -m "feat(annotation): add two-point value types"
```

---

### Task 2: Atomically add the committed model, geometry, rendering, and output

**Files:**
- Create: `crates/rollshot-image-document/src/two_point.rs`
- Modify: `crates/rollshot-image-document/src/annotation.rs`
- Modify: `crates/rollshot-image-document/src/edit_op.rs`
- Modify: `crates/rollshot-image-document/src/document.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`
- Modify: `crates/rollshot-image-document/src/shapes.rs`
- Modify: `crates/rollshot-image-document/src/hit.rs`
- Modify: `crates/rollshot-image-document/src/raster.rs`
- Modify: `crates/rollshot-image-document/src/flatten.rs`
- Modify: `crates/rollshot-image-document/src/navigator.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Test: inline tests in the owning modules

**Interfaces:**
- Consumes: Task 1 `TwoPointKind` and `StrokeStyle`.
- Produces: `Annotation::TwoPoint { id, kind, start, end, style }`.
- Produces: `EditOp::{AddTwoPoint, UpdateTwoPointPoints, UpdateStrokeStyle}`.
- Produces: `ImageDocument::{add_two_point, add_two_point_with_style, set_two_point_points, set_stroke_style}`.
- Produces: `EditError::{CoincidentPoints, InvalidStrokeWidth, InvalidOpacity}`.
- Produces: `arrowhead_points(start, end, width) -> [ImagePoint; 3]`.
- Produces: `segment_distance(point, start, end) -> f32`.
- Produces: `point_in_triangle(point, triangle) -> bool`.
- Produces: `two_point_bounds(kind, start, end, width) -> ImageRect`.
- Produces: `RenderShape::Line { start, end, width, color: Rgba8 }`.
- Produces: `HitPart::{StartEndpoint, EndEndpoint}`.

- [ ] **Step 1: Write failing constructor, atomic-edit, and validation tests**

Add these tests before changing the enum:

```rust
#[test]
fn canonical_two_point_constructor_preserves_kind_and_points() {
    let annotation = Annotation::two_point(
        AnnotationId(7),
        TwoPointKind::Arrow,
        ImagePoint::new(10.0, 20.0),
        ImagePoint::new(80.0, 40.0),
    );
    assert!(matches!(
        annotation,
        Annotation::TwoPoint {
            id: AnnotationId(7),
            kind: TwoPointKind::Arrow,
            start,
            end,
            style,
        } if start == ImagePoint::new(10.0, 20.0)
            && end == ImagePoint::new(80.0, 40.0)
            && style == StrokeStyle::default()
    ));
}

#[test]
fn two_point_add_update_style_delete_undo_redo_is_one_entry_per_edit() {
    let mut doc = ImageDocument::new(image());
    let id = doc
        .add_two_point(
            TwoPointKind::Arrow,
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(80.0, 40.0),
        )
        .unwrap();
    doc.set_two_point_points(
        id,
        ImagePoint::new(20.0, 20.0),
        ImagePoint::new(90.0, 60.0),
    )
    .unwrap();
    let style = StrokeStyle {
        color: Rgb8::new(1, 2, 3),
        width: 8.0,
        opacity: 1.0,
    };
    doc.set_stroke_style(id, style).unwrap();
    doc.delete_annotation(id).unwrap();
    assert!(doc.undo());
    assert_eq!(doc.annotation(id).and_then(Annotation::stroke_style), Some(style));
    assert!(doc.redo());
    assert!(doc.annotation(id).is_none());
}

#[test]
fn rejected_two_point_edits_are_atomic() {
    let mut doc = ImageDocument::new(image());
    let before_state = doc.state_id();
    assert_eq!(
        doc.add_two_point(
            TwoPointKind::Line,
            ImagePoint::new(5.0, 5.0),
            ImagePoint::new(5.0, 5.0),
        ),
        Err(EditError::CoincidentPoints)
    );
    assert_eq!(doc.state_id(), before_state);
    assert!(doc.annotations().is_empty());
}

#[test]
fn invalid_stroke_values_are_rejected_without_mutation() {
    let mut doc = ImageDocument::new(image());
    for style in [
        StrokeStyle { width: 0.0, ..StrokeStyle::default() },
        StrokeStyle { width: f32::NAN, ..StrokeStyle::default() },
        StrokeStyle { opacity: -0.1, ..StrokeStyle::default() },
        StrokeStyle { opacity: 1.1, ..StrokeStyle::default() },
    ] {
        assert!(doc
            .add_two_point_with_style(
                TwoPointKind::Line,
                ImagePoint::new(1.0, 1.0),
                ImagePoint::new(20.0, 20.0),
                style,
            )
            .is_err());
    }
    assert!(doc.annotations().is_empty());
}

#[test]
fn failed_batch_restores_next_id_and_noop_updates_create_no_history() {
    let mut doc = ImageDocument::new(image());
    let result = doc.apply_batch(vec![
        EditOp::AddTwoPoint {
            kind: TwoPointKind::Line,
            start: ImagePoint::new(1.0, 1.0),
            end: ImagePoint::new(20.0, 20.0),
            style: StrokeStyle::default(),
        },
        EditOp::AddTwoPoint {
            kind: TwoPointKind::Arrow,
            start: ImagePoint::new(5.0, 5.0),
            end: ImagePoint::new(5.0, 5.0),
            style: StrokeStyle::default(),
        },
    ]);
    assert_eq!(result, Err(EditError::CoincidentPoints));
    let id = doc
        .add_two_point(TwoPointKind::Line, ImagePoint::new(1.0, 1.0), ImagePoint::new(20.0, 20.0))
        .unwrap();
    assert_eq!(id, AnnotationId(1));
    while doc.undo() {}
    let id = doc
        .add_two_point(TwoPointKind::Line, ImagePoint::new(1.0, 1.0), ImagePoint::new(20.0, 20.0))
        .unwrap();
    let before = doc.state_id();
    doc.set_stroke_style(id, StrokeStyle::default()).unwrap();
    assert_eq!(doc.state_id(), before);
}
```

- [ ] **Step 2: Run the model tests and confirm the missing-API failure**

```bash
rtk cargo test -p rollshot-image-document canonical_two_point_constructor_preserves_kind_and_points
rtk cargo test -p rollshot-image-document rejected_two_point_edits_are_atomic
```

Expected: compile failures because `Annotation::TwoPoint` and the public edit methods do not exist.

- [ ] **Step 3: Add the annotation variant, typed operations, validation, and public methods**

Add the exact variant from the approved spec, include it in `id()`, and return the endpoint extent's top-left from `anchor()`:

```rust
Annotation::TwoPoint { start, end, .. } => {
    ImagePoint::new(start.x.min(end.x), start.y.min(end.y))
}
```

Add `two_point`, `two_point_with_style`, and `stroke_style() -> Option<StrokeStyle>`. Add the edit operations:

```rust
AddTwoPoint {
    kind: TwoPointKind,
    start: ImagePoint,
    end: ImagePoint,
    style: StrokeStyle,
},
UpdateTwoPointPoints {
    id: AnnotationId,
    start: ImagePoint,
    end: ImagePoint,
},
UpdateStrokeStyle {
    id: AnnotationId,
    style: StrokeStyle,
},
```

Centralize validation:

```rust
fn validate_stroke_style(style: StrokeStyle) -> Result<(), EditError> {
    if !style.width.is_finite() || style.width <= 0.0 {
        return Err(EditError::InvalidStrokeWidth);
    }
    if !style.opacity.is_finite() || !(0.0..=1.0).contains(&style.opacity) {
        return Err(EditError::InvalidOpacity);
    }
    Ok(())
}

fn clamp_two_point(
    start: ImagePoint,
    end: ImagePoint,
    width: u32,
    height: u32,
) -> Result<(ImagePoint, ImagePoint), EditError> {
    ensure_point_finite(&start)?;
    ensure_point_finite(&end)?;
    let start = start.clamp_to(width, height);
    let end = end.clamp_to(width, height);
    if start == end {
        return Err(EditError::CoincidentPoints);
    }
    Ok((start, end))
}
```

Add `next_id` to the private `Snapshot`, `snapshot()`, and `restore()` so failed batches and undo/redo restore the complete identity state. Direct TwoPoint methods delegate through one-op `apply_batch` calls and extract the returned ID for adds. Update referenced-ID preflight and `apply_one` exhaustively.

After all operations succeed, compare `annotations`, `next_number`, and `next_id` with the pre-batch snapshot. If none changed, return `Ok(BatchOutcome { added_ids })` without `commit`; otherwise commit exactly once. This makes identical point/style updates no-ops for both public methods and raw batches without duplicating policy.

- [ ] **Step 4: Write failing pure-geometry tests**

Create `two_point.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_horizontal_arrowhead_matches_reviewed_geometry() {
        let points = arrowhead_points(
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(100.0, 50.0),
            4.0,
        );
        assert_eq!(points[0], ImagePoint::new(100.0, 50.0));
        assert_eq!(points[1], ImagePoint::new(76.0, 62.0));
        assert_eq!(points[2], ImagePoint::new(76.0, 38.0));
    }

    #[test]
    fn arrowhead_clamps_at_minimum_and_maximum() {
        let thin = arrowhead_points(
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(100.0, 0.0),
            1.0,
        );
        assert_eq!(thin[1], ImagePoint::new(84.0, 8.0));
        let thick = arrowhead_points(
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(100.0, 0.0),
            16.0,
        );
        assert_eq!(thick[1], ImagePoint::new(68.0, 16.0));
    }

    #[test]
    fn segment_distance_clamps_to_finite_endpoints() {
        let a = ImagePoint::new(10.0, 10.0);
        let b = ImagePoint::new(20.0, 10.0);
        assert_eq!(segment_distance(ImagePoint::new(15.0, 14.0), a, b), 4.0);
        assert_eq!(segment_distance(ImagePoint::new(25.0, 10.0), a, b), 5.0);
    }
}
```

- [ ] **Step 5: Run the geometry tests and confirm the missing-function failure**

```bash
rtk cargo test -p rollshot-image-document two_point::tests -- --nocapture
```

Expected: compile failure because the pure functions do not exist.

- [ ] **Step 6: Implement the pure geometry module**

Use normalized direction/perpendicular math and the approved clamps:

```rust
pub fn arrowhead_points(start: ImagePoint, end: ImagePoint, width: f32) -> [ImagePoint; 3] {
    let length = start.distance(end);
    debug_assert!(length > 0.0);
    let direction = ((end.x - start.x) / length, (end.y - start.y) / length);
    let perpendicular = (-direction.1, direction.0);
    let head_length = (width * 6.0).clamp(16.0, 32.0);
    let half_width = (width * 3.0).clamp(8.0, 16.0);
    let base = ImagePoint::new(
        end.x - direction.0 * head_length,
        end.y - direction.1 * head_length,
    );
    [
        end,
        ImagePoint::new(
            base.x + perpendicular.0 * half_width,
            base.y + perpendicular.1 * half_width,
        ),
        ImagePoint::new(
            base.x - perpendicular.0 * half_width,
            base.y - perpendicular.1 * half_width,
        ),
    ]
}
```

Implement `segment_distance` by projecting onto the segment with `t.clamp(0.0, 1.0)`. Implement `point_in_triangle` with consistent edge-sign tests. Implement `two_point_bounds` as the union of the shaft expanded by `width / 2.0` and, for Arrow, all three triangle points.

- [ ] **Step 7: Write failing lowering, bounds, hit, Navigator, and flatten tests**

Add focused assertions:

```rust
fn line() -> Annotation {
    Annotation::two_point(
        AnnotationId(1),
        TwoPointKind::Line,
        ImagePoint::new(10.0, 50.0),
        ImagePoint::new(100.0, 50.0),
    )
}

fn arrow() -> Annotation {
    Annotation::two_point(
        AnnotationId(2),
        TwoPointKind::Arrow,
        ImagePoint::new(10.0, 50.0),
        ImagePoint::new(100.0, 50.0),
    )
}

#[test]
fn arrow_lowers_to_shaft_then_existing_triangle() {
    let annotation = arrow();
    let shapes = annotation_shapes(&annotation);
    assert!(matches!(shapes[0], RenderShape::Line { .. }));
    assert!(matches!(shapes[1], RenderShape::Triangle { .. }));
}

#[test]
fn arrow_hit_tests_endpoints_shaft_and_triangle_in_priority_order() {
    let annotation = arrow();
    assert_eq!(
        hit_test_annotation(&annotation, ImagePoint::new(10.0, 50.0), 8.0),
        Some(HitPart::StartEndpoint)
    );
    assert_eq!(
        hit_test_annotation(&annotation, ImagePoint::new(100.0, 50.0), 8.0),
        Some(HitPart::EndEndpoint)
    );
    assert_eq!(
        hit_test_annotation(&annotation, ImagePoint::new(50.0, 53.0), 8.0),
        Some(HitPart::Body)
    );
    assert_eq!(
        hit_test_annotation(&annotation, ImagePoint::new(80.0, 58.0), 2.0),
        Some(HitPart::Body)
    );
}

#[test]
fn navigator_labels_two_point_kinds_and_uses_visual_center() {
    let items = navigator_items(&[line(), arrow()]);
    assert_eq!(items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["Line", "Arrow"]);
    assert!(items.iter().all(|item| item.center.x.is_finite() && item.center.y.is_finite()));
}

#[test]
fn flatten_paints_shaft_and_arrowhead_without_mutating_source() {
    let mut doc = ImageDocument::new(base(140, 100));
    doc.add_two_point(
        TwoPointKind::Arrow,
        ImagePoint::new(10.0, 50.0),
        ImagePoint::new(110.0, 50.0),
    )
    .unwrap();
    let out = doc.flatten();
    assert_ne!(out.get_pixel(50, 50), doc.source().get_pixel(50, 50));
    assert_ne!(out.get_pixel(100, 50), doc.source().get_pixel(100, 50));
    assert_eq!(doc.source().get_pixel(50, 50).0, [10, 20, 30, 255]);
}
```

- [ ] **Step 8: Add the render command, hit parts, and annotation lowering**

Add:

```rust
RenderShape::Line {
    start: ImagePoint,
    end: ImagePoint,
    width: f32,
    color: Rgba8,
},
```

Convert opacity once during lowering:

```rust
let alpha = (style.opacity * 255.0).round() as u8;
let color = style.color.with_alpha(alpha);
let mut shapes = vec![RenderShape::Line {
    start: *start,
    end: *end,
    width: style.width,
    color,
}];
if *kind == TwoPointKind::Arrow {
    shapes.push(RenderShape::Triangle {
        points: arrowhead_points(*start, *end, style.width),
        color,
    });
}
```

Add endpoint hit parts and check them before shaft/triangle body tests. Route `annotation_bounds` through `two_point_bounds`. Add `Line`/`Arrow` Navigator labels.

- [ ] **Step 9: Implement anti-aliased raster and both Canvas consumers**

Add `stroke_line` to `raster.rs` using the existing `blend_px` and per-pixel distance coverage:

```rust
pub(crate) fn stroke_line(
    img: &mut RgbaImage,
    start: ImagePoint,
    end: ImagePoint,
    width: f32,
    color: Rgba8,
) {
    let radius = width / 2.0;
    let bounds = ImageRect::from_corners(start, end).expanded(radius + 1.0);
    let x0 = bounds.x.floor() as i32;
    let y0 = bounds.y.floor() as i32;
    let x1 = (bounds.x + bounds.width).ceil() as i32;
    let y1 = (bounds.y + bounds.height).ceil() as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let sample = ImagePoint::new(x as f32 + 0.5, y as f32 + 0.5);
            let coverage = (radius + 0.5 - segment_distance(sample, start, end)).clamp(0.0, 1.0);
            blend_px(img, x, y, color, coverage);
        }
    }
}
```

Route `RenderShape::Line` to `stroke_line` in `flatten.rs`. In both Canvas consumers draw the same finite path:

```rust
let path = canvas::Path::line(
    Point::new(start.x * scale, start.y * scale),
    Point::new(end.x * scale, end.y * scale),
);
frame.stroke(
    &path,
    canvas::Stroke::default()
        .with_color(token_color(*color))
        .with_width(width * scale),
);
```

Do not add Timeline tools or state; only make its shared render match exhaustive.

- [ ] **Step 10: Run model, geometry, render, and full workspace tests**

```bash
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-app result_workspace::canvas
rtk cargo test -p rollshot-app timeline_workspace::annotation
rtk cargo check --workspace --all-targets
```

Expected: all commands pass, including exact arrowhead, finite-segment miss, opacity blending, edge clipping, Navigator, and immutable-source tests.

- [ ] **Step 11: Commit the atomic model and render lifecycle**

```bash
rtk git add crates/rollshot-image-document/src/annotation.rs crates/rollshot-image-document/src/edit_op.rs crates/rollshot-image-document/src/document.rs crates/rollshot-image-document/src/lib.rs crates/rollshot-image-document/src/two_point.rs crates/rollshot-image-document/src/shapes.rs crates/rollshot-image-document/src/hit.rs crates/rollshot-image-document/src/raster.rs crates/rollshot-image-document/src/flatten.rs crates/rollshot-image-document/src/navigator.rs crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/properties.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/timeline_workspace/annotation.rs
rtk git commit -m "feat(annotation): add two-point document lifecycle"
```

---

### Task 3: Extend persisted defaults and transactional properties

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/annotation_defaults.rs`
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Test: inline tests in those modules

**Interfaces:**
- Consumes: Tasks 1–2 `TwoPointKind`, `StrokeStyle`, and render lowering.
- Produces: `AnnotationDefaults::{line, arrow}: StrokeStyle`.
- Produces: `PropertyTarget::TwoPointTool(TwoPointKind)` and `ColorProperty::StrokeColor`.
- Produces: `StrokeWidthTransaction { target, original, preview }` in `PropertyState`.
- Produces: `Message::{PreviewStrokeWidth(f32), ApplyStrokeWidth, CancelStrokeWidth}`.
- Produces: app-only color/width preview clones that never enter document output.

- [ ] **Step 1: Write failing defaults round-trip and fallback tests**

```rust
#[test]
fn missing_two_point_sections_use_independent_canonical_defaults() {
    let ctx = TestContext::new();
    ctx.write_config("[annotation_defaults.number]\nsize = \"Medium\"\n");
    let loaded = load_from(&ctx.path());
    assert_eq!(loaded.values.line, StrokeStyle::default());
    assert_eq!(loaded.values.arrow, StrokeStyle::default());
}

#[test]
fn invalid_line_width_does_not_reset_arrow_defaults() {
    let ctx = TestContext::new();
    ctx.write_config(
        "[annotation_defaults.line]\nwidth = -2.0\nopacity = 0.5\n\
         [annotation_defaults.arrow]\nwidth = 9.0\nopacity = 1.0\n\
         [annotation_defaults.arrow.color]\nr = 1\ng = 2\nb = 3\n",
    );
    let loaded = load_from(&ctx.path());
    assert_eq!(loaded.values.line, StrokeStyle::default());
    assert_eq!(loaded.values.arrow.width, 9.0);
    assert_eq!(loaded.values.arrow.color, Rgb8::new(1, 2, 3));
    assert_eq!(loaded.warnings.len(), 1);
}

#[test]
fn save_preserves_unrelated_config_and_round_trips_two_point_defaults() {
    let ctx = TestContext::new();
    ctx.write_config("[daemon]\ncapture_region_hotkey = \"Alt+Shift+6\"\n");
    let mut values = AnnotationDefaults::default();
    values.line.width = 7.0;
    values.arrow.color = Rgb8::new(10, 20, 30);
    save_to(&ctx.path(), &values).unwrap();
    let text = std::fs::read_to_string(ctx.path()).unwrap();
    assert!(text.contains("capture_region_hotkey"));
    let loaded = load_from(&ctx.path());
    assert_eq!(loaded.values, values);
}
```

- [ ] **Step 2: Run the defaults tests and confirm they fail**

```bash
rtk cargo test -p rollshot-app annotation_defaults::tests -- --nocapture
```

Expected: compile failures for missing `line` and `arrow` fields.

- [ ] **Step 3: Extend defaults without duplicating validation policy**

Add fields:

```rust
pub struct AnnotationDefaults {
    pub number: NumberStyle,
    pub text: TextStyle,
    pub line: StrokeStyle,
    pub arrow: StrokeStyle,
}
```

Add `load_stroke_style(parent, key, warnings)` that parses `color` and `width`, validates width using the document rule, and falls back only the affected field. The app accepts persisted opacity only when it is exactly `1.0`; missing opacity resolves to `1.0`, and any other value resolves to `1.0` with one warning. Preserve the existing table-merge and atomic temp/sync/rename writer. `save_to` always writes `1.0` for Line and Arrow defaults.

- [ ] **Step 4: Write failing property-target, preview, and one-entry width tests**

```rust
fn workspace_with_arrow() -> ResultWorkspace {
    let mut state = workspace();
    state
        .document
        .image
        .add_two_point(
            TwoPointKind::Arrow,
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(90.0, 40.0),
        )
        .unwrap();
    state
}

#[test]
fn creation_tools_target_independent_two_point_defaults() {
    let mut state = workspace();
    state.editor.tool = Tool::Line;
    assert_eq!(
        property_target(&state),
        Some(PropertyTarget::TwoPointTool(TwoPointKind::Line))
    );
    state.editor.tool = Tool::Arrow;
    assert_eq!(
        property_target(&state),
        Some(PropertyTarget::TwoPointTool(TwoPointKind::Arrow))
    );
}

#[test]
fn width_preview_does_not_mutate_document_and_release_commits_once() {
    let mut state = workspace_with_arrow();
    let id = state.document.image.annotations()[0].id();
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    let before = state.document.image.state_id();

    update(&mut state, Message::PreviewStrokeWidth(9.0));
    assert_eq!(state.document.image.state_id(), before);
    assert_eq!(preview_annotation(&state).unwrap().stroke_style().unwrap().width, 9.0);

    update(&mut state, Message::ApplyStrokeWidth);
    assert_ne!(state.document.image.state_id(), before);
    assert_eq!(state.document.image.annotation(id).unwrap().stroke_style().unwrap().width, 9.0);
    assert!(state.document.image.undo());
    assert_eq!(state.document.image.annotation(id).unwrap().stroke_style().unwrap().width, 4.0);
}

#[test]
fn selected_arrow_style_does_not_change_arrow_or_line_defaults() {
    let mut state = workspace_with_arrow();
    let defaults = state.annotation_defaults.values.clone();
    let id = state.document.image.annotations()[0].id();
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    update(&mut state, Message::PreviewStrokeWidth(12.0));
    update(&mut state, Message::ApplyStrokeWidth);
    assert_eq!(state.annotation_defaults.values, defaults);
}

#[test]
fn width_only_preview_cancel_and_target_change_never_mutate_document() {
    let mut state = workspace_with_arrow();
    let id = state.document.image.annotations()[0].id();
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    let before = state.document.image.state_id();
    update(&mut state, Message::PreviewStrokeWidth(11.0));
    assert_eq!(preview_annotation(&state).unwrap().stroke_style().unwrap().width, 11.0);
    update(&mut state, Message::CancelStrokeWidth);
    assert!(preview_annotation(&state).is_none());
    update(&mut state, Message::PreviewStrokeWidth(12.0));
    update(&mut state, Message::SelectTool(Tool::Arrow));
    assert!(preview_annotation(&state).is_none());
    assert_eq!(state.document.image.state_id(), before);
}
```

- [ ] **Step 5: Implement TwoPoint targets and mutually exclusive property transactions**

Add:

```rust
pub enum PropertyTarget {
    NumberTool,
    TextTool,
    TwoPointTool(TwoPointKind),
    Annotation(AnnotationId),
}

pub enum ColorProperty {
    NumberAccent,
    TextColor,
    TextBackground,
    StrokeColor,
}

pub struct StrokeWidthTransaction {
    pub target: PropertyTarget,
    pub original: f32,
    pub preview: f32,
}
```

`PropertyState` stores `width: Option<StrokeWidthTransaction>`. Opening a color picker cancels width preview; beginning width preview cancels color preview. `CancelColor`, `CancelStrokeWidth`, tool changes, Undo, Redo, and Esc clear the applicable transient without document history.

Build the compact control with the pinned iced 0.14 API:

```rust
slider(1.0..=16.0, displayed_width, Message::PreviewStrokeWidth)
    .step(1.0)
    .on_release(Message::ApplyStrokeWidth)
    .width(96)
```

Use `Message::CancelStrokeWidth` when Esc or a target change invalidates the transaction. Active-tool apply updates only `values.line` or `values.arrow` and persists through `persist_annotation_defaults`. Selected-object apply calls `set_stroke_style` once.

- [ ] **Step 6: Extend app-only preview cloning for both stroke properties**

Rewrite `preview_annotation` so it does not require a color transaction up front. Resolve the selected annotation ID, then independently look up matching color and width transactions. For a selected `Annotation::TwoPoint`, apply either or both active transactions only to a reconstructed clone:

```rust
Annotation::TwoPoint { id, kind, start, end, mut style } => {
    let mut changed = false;
    if let Some(tx) = state.editor.properties.color.as_ref() {
        if tx.target == PropertyTarget::Annotation(id)
            && tx.property == ColorProperty::StrokeColor
        {
            style.color = tx.preview;
            changed = true;
        }
    }
    if let Some(tx) = state.editor.properties.width.as_ref() {
        if tx.target == PropertyTarget::Annotation(id) {
            style.width = tx.preview;
            changed = true;
        }
    }
    changed.then_some(Annotation::TwoPoint { id, kind, start, end, style })
}
```

Keep the existing Number/Text reconstruction arms. Do not add `Annotation::with_stroke_style` or any mutating document bypass. `AnnotationCanvas.property_preview` remains the only consumer, so Copy/Save cannot observe it.

- [ ] **Step 7: Run property/default tests and workspace tests**

```bash
rtk cargo test -p rollshot-app annotation_defaults::tests -- --nocapture
rtk cargo test -p rollshot-app properties::tests -- --nocapture
rtk cargo test -p rollshot-app result_workspace::update::tests -- --nocapture
rtk cargo check --workspace --all-targets
```

Expected: all tests pass, including invalid-field isolation, unrelated config preservation, in-memory persistence failure, target separation, preview exclusion, Apply/Cancel, and one-entry undo.

- [ ] **Step 8: Commit defaults and properties**

```bash
rtk git add crates/rollshot-app/src/result_workspace/annotation_defaults.rs crates/rollshot-app/src/result_workspace/properties.rs crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(annotation): add two-point defaults and properties"
```

---

### Task 4: Wire creation, Shift snapping, endpoint editing, and body movement

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/two_point.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Test: inline tests in `two_point.rs`, `canvas.rs`, and `update.rs`

**Interfaces:**
- Consumes: Tasks 1–3 document, render, defaults, and property APIs.
- Produces: `snap_endpoint(fixed, moving) -> ImagePoint`.
- Produces: `constrained_endpoint(fixed, moving, shift) -> ImagePoint`.
- Produces: `gesture_meets_threshold(start, end, scale) -> bool`.
- Produces: `Tool::{Line, Arrow}` and `DragState::CreateTwoPoint`.
- Extends: `DragState::EditAnnotation` with the raw pointer required to recompute preview when modifiers change.

- [ ] **Step 1: Write failing pure constraint tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_all_octants_and_preserves_distance() {
        let fixed = ImagePoint::new(100.0, 100.0);
        for moving in [
            ImagePoint::new(140.0, 103.0),
            ImagePoint::new(138.0, 139.0),
            ImagePoint::new(97.0, 140.0),
            ImagePoint::new(61.0, 138.0),
            ImagePoint::new(60.0, 97.0),
            ImagePoint::new(62.0, 61.0),
            ImagePoint::new(103.0, 60.0),
            ImagePoint::new(139.0, 62.0),
        ] {
            let snapped = snap_endpoint(fixed, moving);
            assert!((fixed.distance(snapped) - fixed.distance(moving)).abs() < 0.001);
            let angle = (snapped.y - fixed.y).atan2(snapped.x - fixed.x);
            let eighth_turn = std::f32::consts::FRAC_PI_4;
            assert!((angle / eighth_turn - (angle / eighth_turn).round()).abs() < 0.001);
        }
    }

    #[test]
    fn four_screen_pixel_threshold_is_zoom_independent() {
        let start = ImagePoint::new(10.0, 10.0);
        assert!(!gesture_meets_threshold(start, ImagePoint::new(13.9, 10.0), 1.0));
        assert!(gesture_meets_threshold(start, ImagePoint::new(14.0, 10.0), 1.0));
        assert!(!gesture_meets_threshold(start, ImagePoint::new(17.9, 10.0), 0.5));
        assert!(gesture_meets_threshold(start, ImagePoint::new(18.0, 10.0), 0.5));
    }
}
```

- [ ] **Step 2: Run the pure tests and confirm missing helpers**

```bash
rtk cargo test -p rollshot-app result_workspace::two_point::tests -- --nocapture
```

Expected: compile failure because the module and helpers do not exist.

- [ ] **Step 3: Implement constraint helpers without document dependencies beyond primitives**

```rust
pub const MIN_GESTURE_SCREEN: f32 = 4.0;

pub fn snap_endpoint(fixed: ImagePoint, moving: ImagePoint) -> ImagePoint {
    let dx = moving.x - fixed.x;
    let dy = moving.y - fixed.y;
    let distance = fixed.distance(moving);
    if distance == 0.0 {
        return moving;
    }
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    ImagePoint::new(
        fixed.x + angle.cos() * distance,
        fixed.y + angle.sin() * distance,
    )
}

pub fn constrained_endpoint(
    fixed: ImagePoint,
    moving: ImagePoint,
    shift: bool,
) -> ImagePoint {
    if shift { snap_endpoint(fixed, moving) } else { moving }
}

pub fn gesture_meets_threshold(start: ImagePoint, end: ImagePoint, scale: f32) -> bool {
    start.distance(end) * scale >= MIN_GESTURE_SCREEN
}
```

Rounding supplies deterministic half-angle ties. Keep this helper in app space because Shift and screen scale are editor concerns.

- [ ] **Step 4: Write failing gesture-lifecycle tests**

```rust
fn endpoints(annotation: &Annotation) -> (ImagePoint, ImagePoint) {
    match annotation {
        Annotation::TwoPoint { start, end, .. } => (*start, *end),
        _ => panic!("expected TwoPoint annotation"),
    }
}

fn arrow_annotation() -> Annotation {
    Annotation::two_point(
        AnnotationId(1),
        TwoPointKind::Arrow,
        ImagePoint::new(10.0, 20.0),
        ImagePoint::new(80.0, 40.0),
    )
}

fn workspace_with_selected_arrow() -> ResultWorkspace {
    let mut state = workspace();
    let id = state
        .document
        .image
        .add_two_point(
            TwoPointKind::Arrow,
            ImagePoint::new(10.0, 20.0),
            ImagePoint::new(80.0, 40.0),
        )
        .unwrap();
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    state
}

fn current_drag_annotation(state: &ResultWorkspace) -> Option<Annotation> {
    match &state.editor.drag {
        Some(DragState::CreateTwoPoint {
            kind,
            start,
            raw_current,
            style,
        }) => Some(Annotation::two_point_with_style(
            AnnotationId(u64::MAX),
            *kind,
            *start,
            constrained_endpoint(*start, *raw_current, state.modifiers.shift()),
            *style,
        )),
        Some(DragState::EditAnnotation { current, .. }) => Some(current.clone()),
        _ => None,
    }
}

#[test]
fn arrow_creation_previews_and_commits_same_snapped_endpoint_without_selection() {
    let mut state = workspace();
    state.editor.tool = Tool::Arrow;
    update(&mut state, Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT));
    handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
    handle_canvas_moved(&mut state, ImagePoint::new(90.0, 36.0));
    let preview = current_drag_annotation(&state).unwrap();
    handle_canvas_released(&mut state, ImagePoint::new(90.0, 36.0));
    let committed = &state.document.image.annotations()[0];
    assert_eq!(endpoints(committed), endpoints(&preview));
    assert_eq!(committed.stroke_style(), preview.stroke_style());
    assert_eq!(state.editor.tool, Tool::Arrow);
    assert_eq!(state.editor.selection, None);
}

#[test]
fn sub_threshold_two_point_gesture_creates_no_history() {
    let mut state = workspace();
    state.editor.tool = Tool::Line;
    handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
    handle_canvas_released(&mut state, ImagePoint::new(11.0, 10.0));
    assert!(state.document.image.annotations().is_empty());
    assert!(!state.document.image.can_undo());
}

#[test]
fn shift_toggle_recomputes_endpoint_preview_before_pointer_moves_again() {
    let mut state = workspace_with_selected_arrow();
    let id = state.editor.selection.unwrap();
    let (_, end) = endpoints(state.document.image.annotation(id).unwrap());
    handle_canvas_pressed(&mut state, end, Instant::now());
    handle_canvas_moved(&mut state, ImagePoint::new(90.0, 36.0));
    let unsnapped = current_drag_annotation(&state).unwrap();
    update(&mut state, Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT));
    let snapped = current_drag_annotation(&state).unwrap();
    assert_ne!(snapped, unsnapped);
}

#[test]
fn body_drag_translates_both_endpoints_and_preserves_vector() {
    let original = arrow_annotation();
    let moved = dragged_annotation(
        &original,
        HitPart::Body,
        ImagePoint::new(60.0, 70.0),
        (10.0, 20.0),
        false,
    );
    let (before_start, before_end) = endpoints(&original);
    let (after_start, after_end) = endpoints(&moved);
    assert_eq!(after_end.x - after_start.x, before_end.x - before_start.x);
    assert_eq!(after_end.y - after_start.y, before_end.y - before_start.y);
}

#[test]
fn release_point_is_used_even_without_a_final_move_event() {
    let mut state = workspace();
    state.editor.tool = Tool::Line;
    handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
    handle_canvas_moved(&mut state, ImagePoint::new(30.0, 10.0));
    handle_canvas_released(&mut state, ImagePoint::new(70.0, 10.0));
    assert_eq!(
        endpoints(&state.document.image.annotations()[0]),
        (ImagePoint::new(10.0, 10.0), ImagePoint::new(70.0, 10.0))
    );
}
```

- [ ] **Step 5: Add tools and transient draft representation**

Extend the enums:

```rust
pub enum Tool {
    Select,
    Number,
    Text,
    Line,
    Arrow,
    Redact,
    #[cfg(feature = "ocr")]
    OcrText,
}

CreateTwoPoint {
    kind: TwoPointKind,
    start: ImagePoint,
    raw_current: ImagePoint,
    style: StrokeStyle,
},
```

Add `raw_point: ImagePoint` to `EditAnnotation`. Add `modifiers: iced::keyboard::Modifiers` to `AnnotationCanvas` and pass `state.modifiers` explicitly from `view.rs`. The current preview remains derived state. `draft_annotation()` applies `constrained_endpoint(start, raw_current, self.modifiers.shift())`; it does not read current defaults after press because style was captured at draft creation.

- [ ] **Step 6: Wire pressed, moved, modifier-changed, and released paths**

Use one mapping from tool to kind/default:

```rust
fn active_two_point(state: &ResultWorkspace) -> Option<(TwoPointKind, StrokeStyle)> {
    match state.editor.tool {
        Tool::Line => Some((TwoPointKind::Line, state.annotation_defaults.values.line)),
        Tool::Arrow => Some((TwoPointKind::Arrow, state.annotation_defaults.values.arrow)),
        _ => None,
    }
}
```

On press, create `CreateTwoPoint`. On move, update `raw_current`. On `ModifiersChanged`, recompute any `EditAnnotation.current` from `original`, `part`, stored `raw_point`, `grab_offset`, and the new Shift state. On release, treat the clamped release event point as authoritative, store it as the final raw point, and then constrain it:

```rust
let (image_width, image_height) = state.document.image.source().dimensions();
let raw_current = released_point.clamp_to(image_width, image_height);
let end = constrained_endpoint(start, raw_current, state.modifiers.shift());
if gesture_meets_threshold(start, end, current_scale(state)) {
    if let Err(error) = state.document.image.add_two_point_with_style(kind, start, end, style) {
        state.message = Some(InlineMessage::Error(error.to_string()));
    }
}
```

Do not select the new ID. Endpoint edit fixes the opposite endpoint and applies Shift; body movement ignores Shift and translates both endpoints. Release calls `set_two_point_points` once and surfaces errors inline.

- [ ] **Step 7: Draw endpoint handles and include triangle body movement**

For Line draw white-fill/accent-ring handles at both endpoints. For Arrow draw start the same way and end as accent-fill/white-ring. Because Task 2 hit testing returns `Body` for the triangle, the existing select drag path moves it without a separate Arrow-only state.

- [ ] **Step 8: Run the gesture, Canvas, and update suites**

```bash
rtk cargo test -p rollshot-app result_workspace::two_point::tests -- --nocapture
rtk cargo test -p rollshot-app result_workspace::canvas -- --nocapture
rtk cargo test -p rollshot-app result_workspace::update::tests -- --nocapture
rtk cargo check --workspace --all-targets
```

Expected: all eight directions, mid-drag Shift toggles, threshold at multiple zooms, preview/commit equality, endpoint priority, body translation, cancellation, Esc, deletion, and one-entry undo tests pass.

- [ ] **Step 9: Commit the gesture lifecycle**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/two_point.rs crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/view.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(annotation): add two-point editing gestures"
```

---

### Task 5: Expose responsive toolbar routing, exact hints, and shortcuts

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/toolbar.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs`
- Test: inline tests in those modules

**Interfaces:**
- Consumes: Task 4 `Tool::{Line, Arrow}`.
- Produces: Wide/Compact visible `[Select, Number, Text, Line, Arrow, Redact]`.
- Produces: Narrow visible `[Select, Number, Text, Arrow]` with Line and Redact in More.
- Produces: exact Line/Arrow tooltip text and `L`/`A` shortcut routing.

- [ ] **Step 1: Write failing toolbar-density and active-More tests**

```rust
#[test]
fn wide_and_compact_show_adjacent_line_and_arrow() {
    for width in [1000.0, 800.0] {
        let state = workspace();
        let model = toolbar_model(&state, width);
        let pair = model
            .visible_tools
            .windows(2)
            .any(|tools| tools == [Tool::Line, Tool::Arrow]);
        assert!(pair, "Line and Arrow must be adjacent at width {width}");
    }
}

#[test]
fn narrow_keeps_arrow_visible_and_routes_active_line_through_more() {
    let mut state = workspace();
    state.editor.tool = Tool::Line;
    let model = toolbar_model(&state, 600.0);
    assert!(model.visible_tools.contains(&Tool::Arrow));
    assert!(!model.visible_tools.contains(&Tool::Line));
    assert!(model.more.iter().any(|item| item.kind == ToolbarItemKind::Tool(Tool::Line)));
    assert_eq!(model.more_active_tool, Some((Tool::Line, "Line")));
}

#[test]
fn two_point_tooltips_include_shortcut_and_shift_hint() {
    assert_eq!(tool_tooltip(Tool::Line), "Line (L) — Shift: Snap to 45°");
    assert_eq!(tool_tooltip(Tool::Arrow), "Arrow (A) — Shift: Snap to 45°");
}
```

- [ ] **Step 2: Write failing shortcut-precedence tests**

```rust
#[test]
fn line_and_arrow_shortcuts_route_when_input_is_not_captured() {
    let modifiers = keyboard::Modifiers::empty();
    assert_eq!(
        map_key_press(&keyboard::Key::Character("l".into()), modifiers, false),
        Some(Message::SelectTool(Tool::Line))
    );
    assert_eq!(
        map_key_press(&keyboard::Key::Character("a".into()), modifiers, false),
        Some(Message::SelectTool(Tool::Arrow))
    );
}

#[test]
fn captured_input_blocks_two_point_shortcuts() {
    let modifiers = keyboard::Modifiers::empty();
    assert_eq!(map_key_press(&keyboard::Key::Character("l".into()), modifiers, true), None);
    assert_eq!(map_key_press(&keyboard::Key::Character("a".into()), modifiers, true), None);
}
```

- [ ] **Step 3: Run the focused tests and confirm current routing fails**

```bash
rtk cargo test -p rollshot-app toolbar::tests -- --nocapture
rtk cargo test -p rollshot-app map_key_press -- --nocapture
```

Expected: failures because Line/Arrow are absent from the toolbar model and key map.

- [ ] **Step 4: Implement density routing and exact tooltips**

Use:

```rust
let primary_tools = match density {
    ToolbarDensity::Wide | ToolbarDensity::Compact => vec![
        Tool::Select,
        Tool::Number,
        Tool::Text,
        Tool::Line,
        Tool::Arrow,
        Tool::Redact,
    ],
    ToolbarDensity::Narrow => vec![
        Tool::Select,
        Tool::Number,
        Tool::Text,
        Tool::Arrow,
    ],
};
```

For Narrow insert Line before Redact in overflow. Keep output actions pinned. Add:

```rust
fn tool_tooltip(tool: Tool) -> String {
    match tool {
        Tool::Line => "Line (L) — Shift: Snap to 45°".into(),
        Tool::Arrow => "Arrow (A) — Shift: Snap to 45°".into(),
        _ => {
            let item = tool_item(tool);
            shortcut_label(item.label, item.shortcut)
        }
    }
}
```

Use this function in the existing tooltip wrapper. Preserve active styling for directly visible tools and `more_active_tool` for Narrow Line.

- [ ] **Step 5: Add keyboard mappings and preserve focused-input precedence**

Extend only the non-command, non-Alt character branch:

```rust
"l" => Some(Message::SelectTool(Tool::Line)),
"a" => Some(Message::SelectTool(Tool::Arrow)),
```

Do not change the early `captured` return. Preserve command+A for OCR Select All under its feature gate; command handling runs before unmodified tool shortcuts.

- [ ] **Step 6: Run toolbar, properties, keyboard, and workspace suites**

```bash
rtk cargo test -p rollshot-app toolbar::tests -- --nocapture
rtk cargo test -p rollshot-app properties::tests -- --nocapture
rtk cargo test -p rollshot-app map_key_press -- --nocapture
rtk cargo check --workspace --all-targets
```

Expected: all density, More, tooltip, property visibility, active state, shortcut, and input-precedence tests pass.

- [ ] **Step 7: Commit the exposed tools**

```bash
rtk git add crates/rollshot-app/src/result_workspace/toolbar.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/properties.rs
rtk git commit -m "feat(annotation): expose line and arrow tools"
```

---

### Task 6: Lock integrated output, long-image compatibility, and lifecycle evidence

**Files:**
- Modify: `crates/rollshot-image-document/src/flatten.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Verify without modification: `crates/rollshot-app/src/result_workspace/secure_sharing.rs`
- Verify without modification: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md`
- Test: existing inline suites plus new integration tests in `flatten.rs` and `update.rs`

**Interfaces:**
- Consumes: complete Tasks 1–5 vertical lifecycle.
- Produces: mixed long-image regression evidence and platform runtime evidence.
- Produces: umbrella status `Complete` only after all required automated and Linux/macOS runtime checks pass; otherwise `Handoff` with exact remaining entry point.

- [ ] **Step 1: Add mixed-output and long-image acceptance tests**

Extend the existing long-image scale test instead of creating a second synthetic benchmark:

```rust
#[test]
fn hundred_mixed_annotations_on_long_image_include_line_and_arrow() {
    let mut doc = ImageDocument::new(base(1000, 20_000));
    for i in 0..20u32 {
        let y = 100.0 + i as f32 * 950.0;
        doc.add_number_callout(ImagePoint::new(100.0, y), ImagePoint::new(160.0, y));
        doc.add_text_note(ImagePoint::new(300.0, y), format!("step {i}")).unwrap();
        doc.add_redaction(ImageRect { x: 500.0, y, width: 80.0, height: 40.0 }).unwrap();
        doc.add_two_point(
            TwoPointKind::Line,
            ImagePoint::new(20.0, y + 100.0),
            ImagePoint::new(300.0, y + 180.0),
        ).unwrap();
        doc.add_two_point(
            TwoPointKind::Arrow,
            ImagePoint::new(500.0, y + 100.0),
            ImagePoint::new(900.0, y + 180.0),
        ).unwrap();
    }
    let flattened = doc.flatten();
    assert_eq!(flattened.dimensions(), doc.source().dimensions());
    assert_ne!(flattened.get_pixel(160, 240), doc.source().get_pixel(160, 240));
    assert_eq!(doc.navigator_items().len(), 100);
    assert!(doc.hit_test(ImagePoint::new(160.0, 240.0), 8.0).is_some());
}
```

Add a Result Workspace output test that starts an uncommitted draft and selection, then proves `copy_payload` contains only committed Line/Arrow pixels and `copy_original_payload` remains source-identical.

- [ ] **Step 2: Run the acceptance tests**

```bash
rtk cargo test -p rollshot-image-document hundred_mixed_annotations_on_long_image_include_line_and_arrow
rtk cargo test -p rollshot-app two_point_output_excludes_draft_and_handles
```

Expected: both commands pass. Any failure is an integration defect to fix in Step 3, not an expected RED state, because Tasks 1–5 already implement the behavior under acceptance.

- [ ] **Step 3: Complete compatibility arms and integration assertions**

Use explicit handling:

- Verify that secure-sharing redaction checks continue to classify only `OpaqueRedaction` as secure redaction; TwoPoint is an ordinary visible annotation. Do not edit the module unless its focused test first demonstrates a behavior gap and the live plan is updated to declare that file.
- Verify that OCR redaction masks continue to extract only `OpaqueRedaction` bounds; TwoPoint does not enter privacy masks. Do not edit the module unless its focused test first demonstrates a behavior gap and the live plan is updated to declare that file.
- Timeline accepts the line render command from Task 2 but exposes no creation action.
- Copy/Save use `ImageDocument::flatten()` unchanged; tests prove drafts, property previews, and handles never reach it.
- Copy Original uses the immutable source unchanged.

Do not add a proposal operation, Timeline tool, OCR behavior, or Action Guide creation path.

- [ ] **Step 4: Run fresh automated verification**

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all commands exit 0 with zero test failures, formatting differences, or Clippy warnings. This slice does not run stitching benchmarks.

- [ ] **Step 5: Run Linux Result Workspace verification**

On Linux, record pass/fail evidence for:

```text
Wide/Compact: Line and Arrow are adjacent and directly visible.
Narrow: Arrow remains visible; Line is in More; active More reads Line.
Tooltips show L/A and Shift: Snap to 45°.
Line and Arrow create repeatedly without switching tools or selecting new objects.
Sub-4-screen-pixel drags create nothing at multiple zoom levels.
Shift toggles live during creation and both endpoint edits.
Start/end handles, Arrow tip handle, triangle body drag, delete, undo, and redo work.
Line and Arrow defaults remain independent across workspace restart.
Selected color/width preview, Apply, Cancel, and one-step undo work.
Navigator, Copy, Save As, Copy Original, dirty state, zoom, pan, and long-image culling remain correct.
Filled triangle is legible on light, dark, and visually busy screenshots.
```

Expected: every line passes with no output/dirty-state ambiguity.

- [ ] **Step 6: Run macOS Result Workspace verification**

Run the same checklist on macOS, including native clipboard and Save As dialog handoff. Expected: behavior matches Linux except for existing native integration presentation.

If macOS access is unavailable or any required check fails, do not mark Complete. Update the umbrella row to `Handoff` with completed tasks, exact fresh automated/Linux evidence, remaining macOS or failure details, known risk, branch/commit range, and the exact next command/manual entry point.

- [ ] **Step 7: Record the lifecycle outcome**

Only after Tasks 1–6, all three automated commands, and both platform runtime checklists pass, collect the exact evidence:

```bash
rtk git log -1 --format='%h'
rtk date +%F
```

Preserve the already-registered slice-spec and implementation-plan links and commits. Change status to `Complete`; record the implementation commit or PR from the first command, the three automated commands, both platform runtime passes, the statement `no required work remains`, and the completion date from the second command. If the outcome is Handoff, use the registry's required Handoff evidence instead of Complete.

- [ ] **Step 8: Commit integration evidence**

```bash
rtk git add crates/rollshot-image-document/src/flatten.rs crates/rollshot-app/src/result_workspace/update.rs docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md
rtk git commit -m "test(annotation): verify two-point tool lifecycle"
```

## Plan Self-Review Record

- [x] Every approved Slice 2 requirement maps to a task above.
- [x] `TwoPointKind`, `StrokeStyle`, edit operation, helper, message, and property names are identical across tasks.
- [x] Every code-changing step includes the concrete API or code block to implement.
- [x] Every TDD cycle has an exact command and expected red/green result.
- [x] No later-slice tool, generic path system, opacity UI, or arrowhead selector entered the plan.
- [x] Line/Arrow visibility matches the product review: adjacent at Wide/Compact, Line in More only at Narrow.
- [x] Completion cannot be recorded without fresh automated and Linux/macOS runtime evidence.
