# Two-Point Tools Design

**Date:** 2026-07-13  
**Status:** Approved design  
**Program:** Annotation Editor  
**Slice:** 2 — Two-Point Tools

## 1. Purpose And Authority

This slice adds complete Line and Arrow annotation lifecycles to the Result
Workspace. It builds on the landed Editor And Style Foundation and extends its
document, defaults, properties, toolbar, gesture, history, and output systems.
It does not introduce competing systems for any of those responsibilities.

This design is subordinate to
[`2026-07-12-annotation-editor-umbrella-design.md`](2026-07-12-annotation-editor-umbrella-design.md)
and builds on
[`2026-07-12-editor-and-style-foundation-design.md`](2026-07-12-editor-and-style-foundation-design.md).
The umbrella's program invariants remain authoritative. If implementation
discovers a conflict with an umbrella invariant, this slice stops until the
umbrella is revised with user approval.

## 2. Goals

- Add Line and Arrow annotations backed by one shared two-point model.
- Support creation, live preview, selection, endpoint editing, body movement,
  deletion, undo/redo, Navigator, Copy, Save, and full-resolution flattening.
- Add independent persisted Line and Arrow color and width defaults.
- Add selected-object color and width editing through Slice 1's transactional
  property flow.
- Snap creation and endpoint edits to 45-degree increments while Shift is held.
- Use one reviewed filled-triangle Arrow head that remains legible across
  supported widths and screenshot backgrounds.
- Keep live Canvas and flattened output geometrically consistent by consuming
  the same framework-neutral render commands.

## 3. Non-Goals

- Rectangle, Ellipse, Shapes, Pen, Highlighter, or Pixelate.
- Multiple arrowheads, start arrowheads, arrowhead selectors, dash styles,
  line caps, gradients, shadows, or opacity controls.
- Rotation, multi-selection, Alt-from-center gestures, guides, rulers, or
  arbitrary-angle numeric input.
- A generic path annotation or speculative render API for later slices.
- Line or Arrow creation in Action Guide, Timeline Workspace, automation
  proposals, or workbench review.
- Changes to Capture Overlay, capture backends, or stitching core.

## 4. Approved Product Decisions

- `Line` and `Arrow` are directly visible at Wide and Compact widths.
- Narrow keeps `Arrow` visible and moves `Line` into More.
- More shows active styling and the current tool name while Narrow Line is
  active.
- Arrow uses a single filled-triangle head at the drag-release endpoint.
- Arrow uses shortcut `A`; Line uses shortcut `L`.
- Line and Arrow remain active after successful creation.
- A completed creation does not select the new annotation or switch to Select.
- Line and Arrow each retain independent future-object defaults even though
  they use the same style type.
- Both tools initially use accent red `#E5484D`, width `4` full-resolution
  image pixels, and opacity `1.0`.
- The committed property UI exposes color and width only.

## 5. Ownership And Architecture

```text
rollshot-app
  active tool / persisted defaults / transient draft / modifiers / handles
  toolbar and properties / screen-space gesture threshold
                              |
                              | completed EditOp
                              v
rollshot-image-document
  TwoPoint annotation / validation / history / shared geometry
  bounds / hit testing / Navigator / render commands
                    |                         |
                    v                         v
        iced live Canvas             full-resolution raster flatten
```

`rollshot-app` owns transient interaction and display-scale concerns.
`rollshot-image-document` owns committed state, validation, history, image-space
geometry, hit testing, Navigator semantics, and framework-neutral rendering.
Pointer movement never mutates the document.

The iced implementation extends the existing `canvas::Program`. Standard
toolbar and property controls continue to use iced 0.14 built-in widgets. This
slice adds no Shader, custom `iced::advanced::Widget`, or custom Overlay.

## 6. Document Model

### 6.1 Annotation representation

Line and Arrow use one annotation variant:

```rust
pub enum TwoPointKind {
    Line,
    Arrow,
}

pub struct StrokeStyle {
    pub color: Rgb8,
    pub width: f32,
    pub opacity: f32,
}

pub enum Annotation {
    // Existing variants remain unchanged.
    TwoPoint {
        id: AnnotationId,
        kind: TwoPointKind,
        start: ImagePoint,
        end: ImagePoint,
        style: StrokeStyle,
    },
}
```

`TwoPointKind` is a semantic distinction, not a duplicate geometry family.
`StrokeStyle` is shared with later ordinary stroke-based annotations only when
those slices arrive; this slice does not add future annotation variants.

Canonical constructors accept a kind and apply `StrokeStyle::default()`.
Explicit-style construction is available for document edits and compatibility
consumers. All committed annotations keep stable identity through replace,
undo, and redo.

### 6.2 Style semantics

The canonical `StrokeStyle` is accent red `#E5484D`, width `4.0`, and opacity
`1.0`. Color is stored as `Rgb8`; opacity remains a separate validated value.
Line and Arrow UI does not expose opacity, and ordinary creation always copies
the fully opaque tool default.

Width is expressed in full-resolution image pixels. The UI permits integer
values from `1` through `16`, inclusive. The document accepts any finite,
strictly positive width so non-UI consumers are not coupled to the UI range.

### 6.3 Edit operations

The typed edit boundary gains operations for:

- Adding a two-point annotation with kind, endpoints, and style.
- Replacing the endpoints of an existing two-point annotation.
- Replacing the style of an existing two-point annotation.

Kind is immutable for this slice. Changing Line to Arrow or Arrow to Line is
not a property operation. Existing generic delete, batch atomicity, history,
stable-ID, and redo-invalidation behavior applies unchanged.

### 6.4 Validation and clamping

The document independently validates every add or update:

- Both endpoints must be finite.
- Width must be finite and strictly positive.
- Opacity must be finite and within `0.0..=1.0`.
- Endpoints are clamped to the immutable source-image bounds, matching existing
  annotation coordinate behavior.
- The clamped endpoints must not coincide.

Validation happens within the atomic edit transaction. Rejection restores the
annotation graph, history, next ID, and all other document state.

## 7. Shared Geometry And Rendering

### 7.1 Framework-neutral commands

The existing render boundary gains only the primitive required by this slice:

- A line segment with endpoints, color, width, and opacity.

A Line emits one line command. An Arrow emits the shaft line followed by one
existing filled `RenderShape::Triangle`. Paint order keeps the triangle above
the shaft and hides their join. Later path and box slices may extend the command
set, but this slice does not generalize it preemptively.

Both Result Workspace Canvas and the full-resolution raster flattener consume
these commands. Timeline Canvas also handles the new commands so the shared
render enum remains exhaustive, but Timeline gains no creation UI.

### 7.2 Filled-triangle arrowhead

For non-coincident endpoints, the normalized direction points from `start` to
`end`. The triangle tip is exactly `end`. Its base center lies backward along
the direction by:

```text
head_length = clamp(width × 6, 16, 32) image px
```

The base extends on each side of the center along the perpendicular by:

```text
head_half_width = clamp(width × 3, 8, 16) image px
```

The shaft extends to `end`; the filled triangle covers the join. Shaft and
triangle use identical color and opacity. Reversing endpoints reverses the
arrow without changing its style.

### 7.3 Bounds and anchors

Line bounds include both endpoints expanded by half the stroke width. Arrow
bounds union the expanded shaft with all three triangle points. These bounds
drive viewport culling, Navigator jump targets, and conservative invalidation.

The reading-order anchor is the top-left of the endpoint extent
`(min(start.x, end.x), min(start.y, end.y))`. Navigator labels are exactly
`Line` and `Arrow`; stable ID remains the final ordering tie-breaker.

### 7.4 Hit testing

Endpoint handles have priority over body hits. The editor converts the existing
fixed screen-space tolerance to image space before calling document hit tests.

- A point within endpoint tolerance returns `StartEndpoint` or `EndEndpoint`.
- Distance to the finite shaft segment determines the Line body hit.
- Arrow body hit is the union of shaft proximity and the filled triangle.
- Points beyond the finite segment do not hit merely because they lie near the
  infinite supporting line.

The same endpoint positions drive handle drawing and hit testing.

## 8. Creation And Editing

### 8.1 Creation

Pointer press begins one transient two-point draft with the active kind and a
copy of that tool's current defaults. Pointer movement updates only the draft.
Pointer release commits one add operation using the same constrained endpoints
shown by the preview.

The Arrow points from press to release. A gesture shorter than `4` screen pixels
is cancelled by the app without submitting an edit. The document separately
rejects coincident clamped endpoints. A rejected or cancelled gesture creates
no history entry and leaves dirty state and selection unchanged.

After a successful creation, the creation tool remains active and the new
annotation remains unselected. Its stable ID is allocated by the document.

### 8.2 Shift snapping

While Shift is held, the moving endpoint snaps to the nearest multiple of 45
degrees around the fixed endpoint. Snapping preserves the unsnapped radial
distance and resolves exact half-angle ties deterministically.

- Creation fixes `start` and snaps `end`.
- Start-handle editing fixes `end` and snaps `start`.
- End-handle editing fixes `start` and snaps `end`.
- Body movement is not angle-constrained.

Changing Shift state during a drag recomputes the preview immediately. Release
uses the current modifier state and the same pure constraint helper as preview,
so the committed annotation cannot diverge from the last preview.

### 8.3 Selection handles and movement

Select mode exposes two zoom-independent circular endpoint handles. Line uses
the same white-fill/accent-ring treatment for both endpoints. Arrow uses that
treatment for its start and accent-fill/white-ring for its arrow-tip endpoint.

Dragging either handle changes only that endpoint. Dragging the shaft or Arrow
triangle translates both endpoints by the same delta, preserving direction,
length, kind, and style. Movement and endpoint edits remain transient until
release, which submits exactly one endpoint-update history entry.

Creation tools do not select or manipulate annotations under the pointer.
Users switch to Select before editing existing objects.

### 8.4 Cancellation and keyboard routing

Tool changes cancel uncommitted two-point drafts and property previews through
the existing Slice 1 paths. `Esc` continues to resolve the most local state:

1. Cancel a draft or property interaction.
2. Clear selection.
3. Switch Line or Arrow back to Select.
4. Continue existing workspace close and dirty-state behavior.

`A` activates Arrow and `L` activates Line unless an inline text editor or
property input owns keyboard input. Delete and Backspace delete a selected
two-point annotation through the existing selection path.

## 9. Toolbar, Defaults, And Properties

### 9.1 Toolbar routing

Arrow joins the directly visible tools at every supported density. Line is
directly visible at Wide and Compact densities and moves into More at Narrow.
This follows the mature screenshot-editor convention of adjacent Line and
Arrow tools when space permits while preserving the umbrella's approved
narrow-width priority without adding a new split selector.

When Narrow Line is active, More uses active styling and displays `Line`.
Closing More does not change the active tool. Line's tooltip is `Line (L) —
Shift: Snap to 45°`; Arrow's tooltip is `Arrow (A) — Shift: Snap to 45°`.
Output actions remain pinned and never enter overflow.

### 9.2 Persisted defaults

Slice 1's `annotation_defaults` configuration gains independent `line` and
`arrow` sections, each storing a `StrokeStyle`. Missing sections or fields use
canonical defaults. Invalid fields fall back only for the affected tool and
produce one warning. Unknown unrelated configuration data remains preserved.

Changing active-tool properties updates the relevant in-memory defaults and
uses the existing non-blocking persistence path. Persistence failure retains
the in-memory value for the current session and reports one warning.

### 9.3 Contextual properties

Active Line and Arrow tools show controls for next-object color and width.
Select with a selected two-point annotation shows controls for that object's
color and width. Editing a selected object never changes either tool default.

Color uses Slice 1's palette and custom picker transaction. Width uses a compact
`1..=16` integer-pixel control. Continuous preview remains app-only. Apply or a
completed width interaction submits one style update and creates at most one
undo entry; Cancel restores the committed style without history.

No opacity, arrowhead, cap, or dash control appears.

## 10. Output, Dirty State, And Failure Semantics

Primary Copy and Save flatten Line and Arrow at full source resolution. Copy
Original continues to return the immutable source. Drafts, hover, selection,
and endpoint handles never enter flattened output.

A successful committed create, endpoint edit, body move, style edit, delete,
undo, or redo follows existing dirty-state and Navigator-refresh behavior.
Property preview and pointer movement do not change durable dirty state.

Document rejection leaves graph, history, dirty state, selection, active tool,
and defaults unchanged and produces the existing non-blocking inline error.
Flatten, Copy, or Save failure preserves existing output paths and dirty state
and never reports success.

## 11. Compatibility

Every exhaustive `Annotation` match is updated for `TwoPoint`, including
document operations, shapes, bounds, hit testing, Navigator, Result Workspace,
secure-sharing inspection, and tests. Existing Number, Text, and Opaque
Redaction constructors and visual behavior remain unchanged.

Every exhaustive render-command consumer handles the new line command and
continues to handle the existing filled triangle. Timeline Canvas may display
them if supplied a shared document but does not expose Line or Arrow tools.
Automation proposal and workbench behavior remain unchanged; this slice adds no
proposal operation for TwoPoint.

Annotation graphs are session-only, so no editable-project migration is
required. Config migration is missing-field-safe through canonical defaults.

## 12. Automated Verification

### 12.1 Document and style

- Canonical and explicit-style construction for Line and Arrow.
- Kind, stable ID, endpoints, style equality, and serde/config round trips.
- Non-finite endpoint, width, and opacity rejection.
- Zero and negative width rejection; opacity below zero and above one rejection.
- Coincident endpoints before and after source-bound clamping.
- Atomic add and update failure with graph, history, and next-ID preservation.
- Create, endpoint update, style update, delete, undo, redo, and redo clearing.
- Navigator `Line`/`Arrow` labels, anchors, centers, ordering, and stable-ID ties.

### 12.2 Geometry, hit testing, and rendering

- Horizontal, vertical, diagonal, and reversed Line and Arrow geometry.
- Exact filled-triangle points at minimum, default, and maximum UI widths.
- Shaft bounds include half-width; Arrow bounds include the complete head.
- Finite-segment body hits, endpoint precedence, triangle hits, and near misses.
- All eight snapped directions, unsnapped distance preservation, and
  deterministic half-angle ties.
- Line emits one line command; Arrow emits shaft then the existing triangle.
- Canvas and raster consumers accept both commands with matching color, width,
  opacity, endpoints, and paint order.
- Flattened-pixel assertions cover representative horizontal and diagonal
  shapes, alpha semantics, edge clipping, and immutable source preservation.

### 12.3 Result Workspace

- Draft and release commit call the same constraint/geometry path.
- Sub-four-screen-pixel cancellation at multiple zoom levels.
- Shift changes during creation and endpoint editing update preview and commit.
- Start edit, end edit, body translation, release commit, and cancellation.
- Zoom-independent handle rendering and image-space hit tolerance.
- Tool persistence after creation and no implicit selection.
- Wide/Compact adjacent Line and Arrow visibility, Narrow Line More routing,
  active More state, and all supported density classes.
- `A`/`L` shortcuts and focused inline-editor/property-input precedence.
- Independent Line/Arrow defaults, missing/invalid config fallback, unrelated
  config preservation, and non-blocking persistence failure.
- Active-tool versus selected-object property targets, preview, Apply, Cancel,
  and one-entry undo semantics.
- Copy, Save, Copy Original, dirty state, Navigator refresh, and exclusion of
  editor overlays.

### 12.4 Regression coverage

- Existing Number, Text, and Opaque Redaction document and UI suites.
- Opaque Redaction remains fully opaque and style-isolated.
- Existing automation proposal, workbench, Action Guide, and Timeline tests.
- Long-image coordinates under zoom and downscaled display.
- The existing long-image 100-annotation scale test includes representative
  Line and Arrow annotations without changing its history-limit intent.

Workspace verification runs:

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

This slice does not touch `rollshot-core` stitching paths and does not run the
stitching benchmark workflow.

## 13. Linux And macOS Runtime Verification

Both platform Result Workspace paths verify:

- Wide/Compact adjacent Line and Arrow access, Narrow Line access through More,
  active-state visibility, Shift hints, and `A`/`L` shortcuts.
- Line and Arrow creation, sub-threshold cancellation, persistent active tool,
  and press-to-release Arrow direction.
- Shift snapping during creation and endpoint editing.
- Start/end handles, Arrow tip distinction, body movement, and deletion.
- Color and width defaults plus selected-object transactional editing.
- Filled-triangle legibility over light, dark, and visually busy screenshots.
- Undo/redo, Navigator jumps, Copy, Save As, Copy Original, and dirty state.
- Zoom, pan, long-image coordinate accuracy, and viewport culling.

Capture Overlay is unchanged. Platform risk is confined to the shared Result
Workspace and its existing native clipboard and file-dialog integrations.

## 14. Completion Criteria

Slice 2 is complete only when:

1. Line and Arrow satisfy their complete create-through-output lifecycle.
2. Shared two-point geometry drives preview, commit, hit testing, bounds, live
   rendering, and flattening without parallel calculations.
3. Endpoint editing, body movement, and Shift snapping pass automated and
   Linux/macOS runtime verification.
4. Independent defaults and selected-object properties extend Slice 1's
   systems without changing existing annotation behavior.
5. Arrow uses the approved filled-triangle head and remains legible across the
   supported width range and representative screenshot backgrounds.
6. Existing Number, Text, Opaque Redaction, automation, workbench, Timeline,
   Copy, Save, and dirty-state behavior remains compatible.
7. All required automated checks pass and no required Slice 2 work remains.

Only then may the umbrella registry mark Slice 2 `Complete` and allow Slice 3
implementation to begin.
