# Freehand Tools Design

**Date:** 2026-07-14  
**Status:** Approved design  
**Program:** Annotation Editor  
**Slice:** 4 — Freehand Tools

## 1. Purpose And Authority

This slice adds complete Pen and Highlighter freehand annotation lifecycles to
the Result Workspace. It builds on the landed Editor And Style Foundation,
Two-Point Tools, and Box Tools slices and extends their document, defaults,
properties, toolbar, gesture, history, rendering, and output systems instead
of introducing competing systems.

This design is subordinate to
[`2026-07-12-annotation-editor-umbrella-design.md`](2026-07-12-annotation-editor-umbrella-design.md)
and builds on
[`2026-07-14-box-tools-design.md`](2026-07-14-box-tools-design.md).
The umbrella's program invariants remain authoritative. If implementation
discovers a conflict with an umbrella invariant, this slice stops until the
umbrella is revised with user approval.

Research input comes from
[`annotation-tools-reference-survey.md`](../../researchs/annotation-tools-reference-survey.md)
and the checked-out Snow Shot, Flameshot, mark-shot, and KDE Spectacle
sources. The freehand reference findings that shaped this design:

- Flameshot's Pencil and mark-shot's Pen both append every pointer-move point
  with no filtering or simplification; Rollshot deliberately improves on this
  with distance filtering and commit-time simplification so long-image strokes
  stay bounded.
- mark-shot's Highlighter draws one whole-stroke path so a self-crossing
  stroke stays uniform; Rollshot adopts that uniform-per-stroke semantics.
- Flameshot (0.35) and mark-shot (~0.78) both use Multiply compositing;
  Rollshot instead uses source-over uniform alpha because the iced live
  renderer has no per-path blend mode and live/flatten consistency outranks
  physical-highlighter fidelity.
- Flameshot and mark-shot both discard single-click strokes; Rollshot does the
  same through its existing minimum-gesture rule.
- Both reference tools hit-test freehand by inflated bounding box; Rollshot
  improves on this with per-segment distance so crossing strokes do not
  capture each other's empty regions.
- Spectacle's Shift straight-line snap for freehand is explicitly deferred.

## 2. Goals

- Add Pen and Highlighter annotations backed by one shared Freehand family.
- Support creation, live preview, selection, whole-stroke body movement,
  delete, undo/redo, Navigator, Copy, Save, and full-resolution flattening.
- Bound stored geometry through pointer distance filtering and commit-time
  simplification within a documented tolerance.
- Define uniform per-stroke alpha compositing for the Highlighter that live
  rendering and flattening share.
- Add independent persisted Pen and Highlighter next-object defaults,
  including the first persisted sub-1.0 stroke opacity.
- Add the editor's first opacity property control, exposed only for the
  Highlighter, through the existing transactional property flow.
- Record simplification input/output point counts and long-stroke render time
  as retained tracing diagnostics.

## 3. Non-Goals

- Pixelate, Blur, or any later-slice tool.
- Shift straight-line or 45-degree constraints for freehand strokes
  (deferred; Line/Arrow already cover straight annotation).
- Freehand resize, per-point editing, rotation, smoothing curves
  (render-time bezier), pressure/velocity width, or eraser behavior.
- Opacity controls for Pen, Line, Arrow, Rectangle, Ellipse, or any
  non-Highlighter tool.
- Multiply or any non-source-over blend mode.
- Multi-selection, group transforms, or layer reorder.
- Freehand creation in Capture Overlay, Action Guide, Timeline Workspace,
  automation proposals, or workbench review.
- Changes to capture backends, platform overlay runners, or stitching core.

## 4. Approved Product Decisions

- Pen and Highlighter use one shared `Freehand` annotation family
  distinguished by `FreehandKind`.
- The Highlighter composites as one uniform-alpha stroke: a self-crossing
  stroke never darkens at its own overlaps; separate strokes over the same
  pixels darken normally under source-over.
- Pointer sampling uses a minimum-distance filter of two screen pixels;
  commit applies Ramer–Douglas–Peucker simplification with an epsilon of one
  screen pixel. Both are zoom-independent screen-space values converted to
  image space through the current scale. These are the documented tolerances
  required by the umbrella.
- Hit testing is per-segment distance along the simplified polyline, not
  bounding box.
- Selected freehand strokes support whole-stroke movement, deletion, and
  style editing only. No resize handles and no per-point editing.
- Pen defaults: accent red `#E5484D`, width `4`, opacity `1.0`, no opacity
  control.
- Highlighter defaults: highlighter yellow `#FFD400`, width `12`, opacity
  `0.4`, with an opacity control from 10% to 100%. Exact default values may
  be fine-tuned during runtime verification without revising this spec;
  the color/width/opacity direction is fixed.
- Pen uses shortcut `P`; Highlighter uses shortcut `H`. They are separate
  toolbar buttons. Highlighter routes into More under width pressure per the
  umbrella's narrow-width priority.
- Both tools remain active after successful creation and do not select the
  new annotation.
- Single clicks and sub-threshold gestures cancel without history, matching
  the existing four-screen-pixel minimum-gesture rule and reference-tool
  behavior.

## 5. Ownership And Architecture

```text
rollshot-app
  active Pen/Highlighter tool / persisted defaults / point accumulation
  distance filter / RDP simplification / minimum gesture / transient draft
  selection / transactional properties incl. opacity slider
                              |
                              | completed edits (simplified points)
                              v
rollshot-image-document
  Freehand annotation / validation / history / path bounds
  per-segment hit testing / Navigator / Polyline render command
                    |                         |
                    v                         v
        iced live Canvas             full-resolution raster flatten
```

`rollshot-app` owns transient interaction: point accumulation, screen-space
filtering and simplification thresholds, active tools, defaults persistence,
and property preview state. `rollshot-image-document` owns committed point
lists, validation, history, image-space geometry, bounds, hit testing,
Navigator semantics, and framework-neutral rendering. Pointer movement never
mutates the document. The document stores the simplified polyline; the raw
pointer stream never leaves the app draft.

The iced implementation extends the existing `canvas::Program`. This slice
adds no Shader, custom `iced::advanced::Widget`, or custom Overlay.

## 6. Document Model

### 6.1 Annotation representation

```rust
pub enum FreehandKind {
    Pen,
    Highlighter,
}

pub enum Annotation {
    // Existing variants remain unchanged.
    Freehand {
        id: AnnotationId,
        kind: FreehandKind,
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    },
}
```

`points` is the simplified polyline in full-resolution image coordinates,
ordered from stroke start to stroke end. `FreehandKind` is a semantic
distinction over shared path geometry: it selects Navigator labels, property
capability (opacity exposure), and default styles; geometry, hit testing,
rendering, and history are shared. Stable identity is retained through
movement edits, style edits, undo, and redo.

`StrokeStyle` is reused unchanged. The Highlighter is the first committed
annotation whose stored opacity is routinely below `1.0`; the existing
validation, `annotation_shapes` alpha lowering, and raster source-over
blending already support this.

Canonical constructors apply the reviewed Pen and Highlighter defaults.
Explicit-style construction is available to document edits and compatibility
consumers without exposing app configuration types.

### 6.2 Validation

The document rejects:

- fewer than two points, or a point list without at least two distinct
  points;
- any non-finite point coordinate;
- non-finite or non-positive stroke width;
- stroke opacity outside the existing valid range.

Validation failure leaves annotations, history, state ID, Navigator data,
and source pixels unchanged. App-level filtering, simplification, and the
minimum-gesture rule do not replace document validation.

### 6.3 Edit operations

The document exposes validated operations for:

- adding a Pen or Highlighter stroke with canonical or explicit style;
- replacing one freehand annotation's full point list while preserving ID,
  kind, and style (whole-stroke movement submits translated points through
  this operation);
- replacing stroke style through the existing stroke-style operation;
- deleting, undoing, and redoing through the established document history.

Kind changes are not a selected-object property. One completed creation,
movement, or applied property transaction creates at most one history entry.
Rejected, cancelled, or unchanged operations create none.

### 6.4 Bounds, anchor, and Navigator

Path bounds are the axis-aligned bounding box of the points expanded by half
the stroke width; culling and visibility use these bounds. The Navigator
anchor is the point-list minimum-x/minimum-y corner, matching the TwoPoint
convention. Navigator labels are `Pen` and `Highlighter` with existing
reading order and stable-ID tie breaking.

### 6.5 Hit testing

Freehand hit testing walks the simplified polyline and accepts a pointer
whose distance to any segment is at most half the stroke width plus the
existing zoom-adjusted screen tolerance, reusing the shared
segment-distance helper. Bounding-box containment alone never hits. The
whole stroke exposes only `Body`; there are no freehand resize handles or
endpoint parts. Topmost-wins scanning and selected-annotation priority
follow existing rules.

## 7. Sampling, Simplification, And Creation

### 7.1 Creation draft

Pointer press with Pen or Highlighter active captures that tool's defaults
into an app-only draft holding `kind`, an accumulating point list, and the
captured style. This is the editor's first accumulating draft; existing
drafts track a single moving point or box.

Pointer movement appends the clamped image-space point only when it is at
least two screen pixels (converted through the current scale) from the last
accepted point. The draft renders through the same polyline geometry the
committed annotation uses. Changing defaults during a draft does not restyle
that draft. Pointer movement creates no document state or history.

### 7.2 Commit and simplification

On release, the app:

1. applies Ramer–Douglas–Peucker simplification to the accepted points with
   an epsilon of one screen pixel converted to image space;
2. cancels without any document operation when the gesture does not meet the
   existing four-screen-pixel minimum (evaluated on the path bounding box),
   which also discards single clicks;
3. otherwise submits one add operation with the simplified points.

Simplification runs once at commit; the draft preview shows filtered raw
points. The visible deviation between preview and committed stroke is
bounded by the one-screen-pixel epsilon at the creation zoom; this is the
documented simplification tolerance.

A `tracing` event on the stable `rollshot::annotation` target records input
and output point counts for every simplification so long-stroke growth is
observable.

### 7.3 Modifiers and cancellation

Shift and other modifiers have no effect on freehand creation in this slice.
`Esc` cancels an active freehand draft first, then follows the umbrella's
selection, active-tool, and workspace-close order. A cancelled draft submits
no document operation. Points are clamped to immutable source bounds during
accumulation; full-resolution coordinates remain the source of truth at
every zoom level.

### 7.4 Selection and movement

Creation tools always create; existing-object editing occurs under Select.
Body movement translates every point by the drag delta, clamping so the
path bounding box stays within source bounds without deforming the stroke.
Release submits one point-list replacement only when geometry changed.
Delete, undo, and redo follow existing behavior.

## 8. Rendering And Compositing

### 8.1 Polyline render command

The framework-neutral render-command model gains one primitive:

```rust
RenderShape::Polyline {
    points: Vec<ImagePoint>,
    width: f32,
    color: /* same color-with-alpha type the existing Line command uses,
              with stroke opacity pre-applied as alpha */
}
```

Exact Rust names and the color representation follow the existing `Line`
command in the implementation plan.

Polyline strokes use round caps and round joins. Both Pen and Highlighter
lower to this command; Pen simply carries full alpha. Existing annotations
keep their current commands. Commands contain no viewport scale, selection,
hover, handles, drafts, or app defaults.

### 8.2 Uniform per-stroke alpha

The committed compositing semantics, shared by live rendering and
flattening, are:

- One freehand stroke composites over the destination exactly once with its
  uniform alpha. Self-overlaps within the stroke (crossings, tight
  switchbacks, round-join overlap regions) never multiply alpha.
- Separate annotations composite in document order with normal source-over
  blending, so two overlapping Highlighter strokes darken where they cross.

The raster flattener implements this by computing a whole-stroke coverage
mask — per pixel, the maximum (not sum) of coverage over all segments,
caps, and joins — and blending the stroke color through that mask in one
source-over pass. Antialiasing follows the existing coverage-based edge
rules: only samples within one full-resolution pixel of the ideal stroke
boundary may vary between renderers.

### 8.3 iced live rendering and known risk

The iced Canvas draws one multi-point `Path` with round caps and joins per
freehand annotation. Known risk: lyon stroke tessellation of a
self-intersecting path may produce overlapping triangles that double-blend
translucent fragments, deviating from the uniform-alpha semantics at
self-overlap pixels.

Flattened output is authoritative and always uniform. If implementation
finds the live deviation visually unacceptable for the Highlighter, resolve
it through a `rollshot-run-spike` investigation (for example stroke-outline
fill) before diverging from this spec; a minor, draft/live-only deviation
confined to self-overlap pixels is acceptable and must be documented in the
implementation notes. This deviation can never affect Copy or Save output.

A `tracing` event on `rollshot::annotation` records long-stroke render time
so freehand cost stays observable. Live rendering continues to cull
committed annotations outside the visible viewport by their path bounds.

## 9. Toolbar, Defaults, And Properties

### 9.1 Toolbar and shortcuts

`Tool::Pen` and `Tool::Highlighter` are separate buttons placed per the
umbrella's approved second-row order (Pen before Highlighter). Pen stays
visible at the umbrella's narrow-width priority; Highlighter routes into
More under width pressure with existing active-state-in-More behavior.
Unmodified `P` activates Pen and `H` activates Highlighter; both are ignored
while an input captures keyboard events and yield to command-modified and
native shortcuts. Tooltips include the shortcut hints.

### 9.2 Persisted defaults

`AnnotationDefaults` gains independent `pen` and `highlighter` stroke
defaults:

- Pen canonical default: accent red `#E5484D`, width `4.0`, opacity `1.0`.
- Highlighter canonical default: `#FFD400`, width `12.0`, opacity `0.4`.

The configuration layer's opacity handling is relaxed for the highlighter
key only: it persists and loads any opacity in `(0.0, 1.0]`, while every
other stroke consumer keeps the existing force-to-`1.0` behavior. Missing,
malformed, or newly introduced fields resolve independently to canonical
defaults. Editing a selected stroke never changes either tool's defaults.
Persistence failure retains in-memory values for the session with the
existing one-time non-blocking warning.

### 9.3 Contextual properties

- Active Pen shows next-object stroke color and width.
- Active Highlighter shows next-object stroke color, width, and the
  editor's first opacity control: a slider from 10% to 100%.
- A selected Freehand annotation shows the committed values for its kind;
  only Highlighter selections expose the opacity control.

Width controls reuse the existing `1`–`16` range. Property changes use the
existing transient preview transactions: Apply submits one validated style
replacement and one history entry; Cancel discards the preview; switching
selection or tools follows existing transaction resolution. The opacity
transaction follows the stroke-width transaction pattern. No opacity
control appears for any non-Highlighter target, preserving the umbrella's
Opaque Redaction safety isolation and tool-capability rules.

## 10. Output, Navigator, And Failure Semantics

Copy and Save flatten every committed freehand stroke at full source
resolution with the uniform-alpha semantics of §8.2. Copy Original remains
source-identical. Drafts, property previews, selection feedback, and hover
never enter output. Successful freehand edits update the document state ID,
durable dirty state, and Navigator cache through existing paths; defaults
changes are configuration changes and do not mark the image dirty.

A rejected create, move, or style edit leaves document, history, selection,
dirty state, Navigator, defaults, and source unchanged and reports the
existing inline error. Copy, Save, flatten, clipboard, and dialog failures
retain their existing rollback behavior.

Opaque Redaction remains the only secure-redaction annotation. Freehand
strokes never enter secure-sharing classification or secure-redaction
wording regardless of color or opacity.

## 11. Compatibility And Scope Boundaries

All exhaustive `Annotation` and `RenderShape` consumers must be inspected
and extended: Navigator labels, `anchor()`, `stroke_style()`, hit testing,
bounds, `annotation_shapes`, the raster `draw_shape` match, the iced
`draw_shape` match, `property_target`, toolbar `tool_item`, and any
automation/workbench/Timeline/Action Guide render-compatibility paths.
Non-Result-Workspace consumers gain only display compatibility, not freehand
creation operations.

Existing Number, Text, Opaque Redaction, Line, Arrow, Rectangle, and
Ellipse behavior, constructors, history, rendering, and output remain
compatible. The Result Workspace behavior is shared by Linux and macOS;
Capture Overlay is unchanged on both platforms. Platform runtime
verification covers the shared Result Workspace and existing native
clipboard and Save As handoffs.

## 12. Automated Verification

### 12.1 Document and history

- Freehand kinds, canonical and explicit-style construction, stable IDs,
  and style equality.
- Rejection of every invalid point-list and style class, including
  single-point and all-coincident lists.
- Create, point-list replacement, style edit, delete, undo, redo, redo
  clearing, and no-op history behavior.
- Navigator labels, anchors, reading order, and stable-ID ties.

### 12.2 Sampling, simplification, and interaction

- Distance-filter acceptance and rejection at multiple zoom levels.
- RDP simplification: output within epsilon of input, collinear collapse,
  short strokes preserved, large point counts bounded.
- Minimum-gesture cancellation including single clicks.
- Draft accumulation, Esc cancellation, and source-bound clamping.
- Per-segment hit testing: on-stroke hits, near-miss beyond tolerance,
  empty-region misses inside the bounding box, crossing-stroke topmost
  selection, and width sensitivity.
- Whole-stroke movement, clamping without deformation, and one-entry
  release commits.

### 12.3 Defaults, properties, toolbar, and keyboard

- Independent Pen/Highlighter defaults and missing-field-safe persistence,
  including highlighter opacity round-trip and continued opacity forcing
  for all other tools.
- Opacity slider range, transaction preview, Apply, Cancel, target
  switching, one-step undo, and invalid rollback.
- Opacity control exposed only for Highlighter targets.
- Toolbar density routing, More behavior, tooltips, and `P`/`H` shortcuts
  with captured-input and command-modifier precedence.

### 12.4 Rendering and output

- Polyline command geometry, round caps/joins, culling bounds, and
  clipping.
- Uniform-alpha flattening: a self-crossing Highlighter stroke blends its
  alpha exactly once at overlap pixels; two separate strokes darken where
  they cross; Pen renders fully opaque.
- Live and flattened geometry agreement within documented edge tolerance.
- Draft, preview, selection, and hover excluded from flattening.
- Copy, Save, Copy Original, dirty state, and failure rollback.
- Opaque Redaction security and existing-annotation output regressions.

### 12.5 Compatibility and scale

- Existing Number, Text, Redaction, Line, Arrow, Rectangle, and Ellipse
  suites.
- Automation, workbench, Timeline, and Action Guide consumers.
- The long-image 100-annotation test extended with representative Pen and
  Highlighter strokes without changing its history-limit intent.
- Full-resolution coordinates under zoom/downscaled display and viewport
  culling with mixed annotations.

Workspace verification runs:

```bash
rtk cargo test
rtk cargo fmt --all --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

This slice does not touch `rollshot-core` stitching paths and does not run
the stitching benchmark workflow.

## 13. Linux And macOS Runtime Verification

Both platform Result Workspace paths verify:

1. Pen and Highlighter buttons, active state, tooltips, `P`/`H` shortcuts,
   and More routing at Wide, Compact, and Narrow widths.
2. Repeated stroke creation, persistent active tool, single-click and
   sub-threshold cancellation, and Esc cancellation.
3. Smooth drawing feel at 100% and zoomed views; simplified strokes remain
   faithful to the drawn path; simplification point counts appear in
   tracing diagnostics.
4. Highlighter translucency over light, dark, and text-heavy content;
   self-crossing strokes stay uniform in flattened output; overlapping
   separate strokes darken; live preview matches within the documented
   self-overlap deviation.
5. Selection by clicking the stroke body, empty-region misses, whole-stroke
   movement, deletion, undo, and redo.
6. Defaults round-trip across restart including highlighter opacity;
   selected-object Preview, Apply, Cancel, and one-step undo for color,
   width, and opacity.
7. Navigator, Copy, Save As, Copy Original, dirty state, zoom, pan,
   long-image coordinates, viewport culling with many-point strokes,
   native clipboard, and file-dialog handoff.

Capture Overlay is unchanged. Platform risk is confined to the shared
Result Workspace and existing native clipboard and file-dialog
integrations.

## 14. Completion Criteria

Slice 4 is complete only when:

1. Pen and Highlighter satisfy their complete create-through-output
   lifecycles.
2. One shared Freehand family drives validation, history, geometry, hit
   testing, Navigator, live rendering, and flattening without duplicated
   Pen/Highlighter lifecycle systems.
3. Distance filtering, RDP simplification with the documented tolerance,
   minimum-gesture cancellation, and per-segment hit testing pass automated
   verification and both platform runtime checklists.
4. Uniform per-stroke alpha holds in flattened output, and any live
   self-overlap deviation is bounded, documented, and absent from output.
5. Highlighter opacity persists across sessions while every other tool
   retains forced full opacity, and the opacity control appears only for
   Highlighter targets.
6. Existing annotation, automation, workbench, Timeline, Action Guide,
   Copy, Save, Navigator, and dirty-state behavior remains compatible, and
   Opaque Redaction remains the only secure-redaction annotation.
7. All required automated checks and both platform runtime checklists pass,
   the umbrella registry records the transition, and no required Slice 4
   work remains.

Only then may Slice 5 implementation begin.
