# Box Tools Design

**Date:** 2026-07-14  
**Status:** Approved design  
**Program:** Annotation Editor  
**Slice:** 3 — Box Tools

## 1. Purpose And Authority

This slice adds complete Rectangle and Ellipse annotation lifecycles to the
Result Workspace, together with the approved Shapes selector. It builds on the
landed Editor And Style Foundation and Two-Point Tools slices. It extends their
document, defaults, properties, toolbar, gesture, history, rendering, and
output systems instead of introducing competing systems.

This design is subordinate to
[`2026-07-12-annotation-editor-umbrella-design.md`](2026-07-12-annotation-editor-umbrella-design.md)
and builds on
[`2026-07-13-two-point-tools-design.md`](2026-07-13-two-point-tools-design.md).
The umbrella's program invariants remain authoritative. If implementation
discovers a conflict with an umbrella invariant, this slice stops until the
umbrella is revised with user approval.

Research input comes from
[`annotation-tools-reference-survey.md`](../../researchs/annotation-tools-reference-survey.md)
and the checked-out Snow Shot, Flameshot, mark-shot, and KDE Spectacle sources.
Rollshot borrows their established box-tool interaction vocabulary where it
fits the existing Result Workspace, without copying unrelated capture-window,
rotation, multi-selection, or advanced-style behavior.

## 2. Goals

- Add Rectangle and Ellipse annotations backed by one shared shape family.
- Add the remembered Shapes primary action and explicit Rectangle/Ellipse
  selector approved by the umbrella.
- Support creation, preview, selection, body movement, eight-direction resize,
  delete, undo/redo, Navigator, Copy, Save, and full-resolution flattening.
- Support Shift-constrained square/circle creation and aspect-preserving corner
  resize.
- Add independent persisted Rectangle and Ellipse stroke/fill defaults.
- Add selected-object stroke/fill editing through the existing transactional
  property flow.
- Keep Rectangle and Ellipse live rendering and flattened output consistent by
  consuming shared framework-neutral render commands.
- Preserve secure Opaque Redaction as a separate annotation and safety
  contract.

## 3. Non-Goals

- Pen, Highlighter, Pixelate, Blur, or any later-slice tool.
- Stroke enable/disable, stroke opacity controls, fill opacity controls,
  gradients, shadows, dash styles, alternate joins, or corner radius.
- Rotation, multi-selection, group transforms, layer reorder, or
  Alt/Control-from-center drawing.
- Numeric width/height editing, guides, rulers, alignment, or snapping to
  image content or other annotations.
- A generic transform framework for TwoPoint, Freehand, Pixelate, or future
  annotation families.
- Rectangle or Ellipse creation in Capture Overlay, Action Guide, Timeline
  Workspace, automation proposals, or workbench review.
- Changes to capture backends, platform overlay runners, or stitching core.

## 4. Approved Product Decisions

- Rectangle and Ellipse use one shared `Shape` annotation family.
- Shapes is a primary button plus an explicit chevron selector. The primary
  action uses the remembered Rectangle or Ellipse.
- The selector opens on click, not hover. Opening it does not change tools.
- Rectangle is the canonical initial remembered shape.
- Choosing a selector item immediately activates and persists that shape.
- Unmodified `S` activates the remembered shape. Repeated `S` does not cycle.
- Shapes remains directly visible at Wide, Compact, and Narrow widths.
- Rectangle and Ellipse remain active after successful creation.
- Successful creation does not select the new annotation or switch to Select.
- Reverse-direction creation is supported and normalized before commit.
- Creation requires at least four screen pixels on both axes.
- Shift creates a square or circle from the press anchor.
- During selected-object resize, Shift on a corner preserves the annotation's
  existing aspect ratio. Shift has no effect on edge handles.
- Handles may cross the opposite edge and continue the resize after
  normalization.
- Outline-only Rectangle and Ellipse interiors remain selectable. Ellipse
  bounding-box corners outside the ellipse do not hit.
- Rectangle and Ellipse have independent persisted next-object defaults.
- Each shape always has a stroke and may have an optional solid fill.
- Stroke and fill opacity controls are not exposed.
- Both tools initially use accent red `#E5484D`, width `4` full-resolution
  image pixels, and fill disabled. The remembered disabled fill color is also
  accent red.

## 5. Ownership And Architecture

```text
rollshot-app
  active Rectangle/Ellipse tool / remembered Shapes action
  independent persisted defaults / transient draft / modifiers
  selection / handles / toolbar selector / transactional properties
                              |
                              | completed edits
                              v
rollshot-image-document
  Shape annotation / validation / history / normalized bounds
  shape hit testing / shared resize handles / Navigator / render commands
                    |                         |
                    v                         v
        iced live Canvas             full-resolution raster flatten
```

`rollshot-app` owns transient interaction, screen-space gesture thresholds,
active tools, remembered shape state, defaults persistence, and property
preview state. `rollshot-image-document` owns committed shape data, validation,
history, image-space geometry, bounds, hit testing, Navigator semantics, and
framework-neutral rendering. Pointer movement never mutates the document.

The iced implementation extends the existing `canvas::Program`. Standard
toolbar, selector, and property controls continue to use built-in iced 0.14
composition and the existing anchored-menu pattern. This slice adds no Shader,
custom `iced::advanced::Widget`, or custom Overlay.

## 6. Document Model

### 6.1 Annotation representation

Rectangle and Ellipse use one annotation variant:

```rust
pub enum ShapeKind {
    Rectangle,
    Ellipse,
}

pub enum Annotation {
    // Existing variants remain unchanged.
    Shape {
        id: AnnotationId,
        kind: ShapeKind,
        bounds: ImageRect,
        stroke: StrokeStyle,
        fill: Option<Rgb8>,
    },
}
```

`ShapeKind` is a semantic rendering and hit-testing distinction over shared box
geometry. `bounds` is a finite, normalized, positive-size rectangle in
full-resolution image coordinates. Stable identity is retained through bounds
edits, style edits, undo, and redo.

`fill: None` means outline-only. A committed annotation does not retain an
inactive fill color because that value does not affect the annotation. The app
defaults retain their fill color while fill is disabled.

Canonical constructors apply the reviewed accent-red, four-pixel,
outline-only style. Explicit-style construction is available to document edits
and compatibility consumers without exposing app configuration types.

### 6.2 Style semantics

Shapes reuse `StrokeStyle`. Stroke color is `Rgb8`, width is a finite positive
full-resolution image-space value, and stored opacity remains validated. The
Result Workspace creates fully opaque shape strokes and exposes no opacity
control in this slice.

The UI uses the existing stroke-width range of `1` through `16`, inclusive.
The document accepts any finite, strictly positive width so its validation is
not coupled to one UI range.

Fill is an optional solid `Rgb8` color. It is fully opaque and has no separate
style wrapper or opacity field. This is the complete approved Slice 3 fill
contract and reserves no advanced-styling fields.

### 6.3 Edit operations

The document exposes validated operations for:

- adding a Rectangle or Ellipse with canonical or explicit style;
- replacing one shape's normalized bounds while preserving ID, kind, and
  style;
- replacing one shape's stroke and fill while preserving ID, kind, and
  bounds;
- deleting, undoing, and redoing through the established document history.

Kind changes are not a selected-object property. Choosing a different Shapes
subtool changes the next creation tool, not an existing annotation.

One completed creation, move, resize, or applied property transaction creates
at most one history entry. Rejected, cancelled, or unchanged operations create
none. A new successful edit after undo clears redo through existing semantics.

### 6.4 Validation

The document rejects:

- non-finite bounds coordinates or dimensions;
- zero or negative normalized width or height;
- non-finite or non-positive stroke width;
- stroke opacity outside the existing valid range.

Validation failure leaves annotations, history, state ID, Navigator data, and
source pixels unchanged. App-level clamping and minimum gesture sizes do not
replace document validation.

## 7. Shared Geometry And Rendering

### 7.1 Logical geometry

The stored `bounds` defines the logical shape path and handle box. Rectangle
edges follow the bounds edges. Ellipse center is the bounds center; its radii
are half the bounds width and height.

Fill covers the logical interior. Stroke is centered on the logical path and
drawn after fill. Rectangle has straight edges and square corners. No corner
radius, dash pattern, or alternate join is introduced.

Committed annotation bounds used for culling and visibility expand logical
bounds by half the stroke width. Navigator ordering uses the normalized logical
bounds top-left anchor.

### 7.2 Framework-neutral commands

The shared render-command model gains explicit Rectangle and Ellipse commands
that carry logical bounds, stroke, and optional fill. Commands fully describe
paint order and centered-stroke semantics. They do not contain viewport scale,
selection, hover, handles, drafts, or app defaults.

The iced Canvas and raster flattener consume the same commands. Fill paints
first, stroke paints second, and annotations composite in document order. A
solid fill intentionally covers source pixels and earlier annotations beneath
its interior.

Raster output uses deterministic coverage at curved and stroked edges. Only
samples whose centers lie within one full-resolution output pixel of the ideal
stroke or fill boundary may vary in edge coverage between renderers; samples
farther from the boundary must agree on inside/outside classification.
Geometry, stroke placement, fill extent, paint order, and clipping must agree.

### 7.3 Shared handles

Rectangle, Ellipse, and Opaque Redaction use the same pure helper for the eight
handle anchor positions: four corners and four edge midpoints of logical
bounds. This sharing does not merge their annotation variants, style APIs,
render commands, or safety semantics.

Handles remain app-only visuals with a zoom-independent screen radius. Handle
hit testing precedes body hit testing.

### 7.4 Shape hit testing

Rectangle body hit testing accepts its logical interior plus the existing
zoom-adjusted screen tolerance.

Ellipse body hit testing uses its normalized ellipse equation. The interior
and tolerated boundary hit; bounding-box corners outside the ellipse do not.
This rule applies whether fill is enabled or disabled.

The stroke neighborhood is included in the same screen-space tolerance so a
thin or small outline remains usable. The document continues to scan
annotations from topmost to bottommost. For a selected annotation, handles take
priority over its body.

## 8. Creation And Editing

### 8.1 Creation draft

Pointer press captures the active Rectangle or Ellipse defaults into an
app-only draft. The draft stores the press anchor and latest raw clamped
pointer. Pointer movement derives preview bounds through the same pure helper
used on release.

Creation supports all drag directions. The helper applies the active Shift
constraint, image-bound clamping, normalization, and minimum-size decision in
a defined order. The final preview before release is the geometry offered to
the document.

Changing defaults during a draft does not restyle that draft. Pointer movement
does not create document state or history.

### 8.2 Minimum size and source bounds

A creation commits only when normalized width and height are each at least
four screen pixels at the current display scale. Otherwise release cancels the
draft without history, dirty-state, selection, or Navigator changes.

All creation points are constrained to immutable source bounds. Shift-created
squares and circles use the largest constrained extent permitted by the raw
drag direction and source edges. Full-resolution coordinates remain the source
of truth at every zoom level.

### 8.3 Shift creation

Without Shift, the normalized box follows the press anchor and current pointer.
With Shift, both dimensions use the smaller absolute drag-axis distance, so a
Rectangle becomes a square and an Ellipse becomes a circle. The signs of the
raw drag axes preserve the active quadrant.

Modifier changes recompute the draft immediately from the retained raw
pointer, even without additional pointer movement.

### 8.4 Selection and movement

Creation tools always create; they do not implicitly edit annotations under
the pointer. Existing-object editing occurs under Select.

The selection hit order is selected handles, selected body, then the topmost
annotation under the pointer. Body movement preserves size and clamps the
entire logical bounds to source bounds. Release submits one bounds replacement
only when geometry changed.

### 8.5 Eight-direction resize

Each handle keeps its opposite edge or opposite corner anchored. Edge handles
change one axis. Corner handles change both axes.

Corner resize with Shift preserves the original annotation aspect ratio.
Edge-handle resize ignores Shift. The helper retains raw pointer input so Shift
can toggle live during a corner drag.

A handle may cross the anchored edge or corner. Geometry normalizes and the
resize continues naturally on the opposite side. Interactive geometry retains
at least four screen pixels on each axis so handles do not collapse together.
The raw pointer determines which side of the anchor is active; at a crossing,
the constrained moving edge switches to the new side at the minimum distance
instead of producing a zero-size preview. The completed normalized shape
remains within source bounds.

### 8.6 Cancellation and cursors

`Esc` cancels a shape draft or active property interaction first, then follows
the umbrella's selection, active-tool, and workspace-close order. A cancelled
draft or resize submits no document operation.

Body, horizontal edge, vertical edge, and the two diagonal handle families use
the corresponding move/resize cursor feedback. Cursor state is app-only.

## 9. Shapes Selector, Defaults, And Properties

### 9.1 Shapes selector

Shapes is a split control with a remembered primary Rectangle/Ellipse action
and an explicit chevron. The primary icon and accessible label identify the
remembered shape. The chevron menu lists Rectangle and Ellipse and indicates
the active choice.

Clicking the primary action activates the remembered shape. Opening or closing
the menu changes no tool. Selecting a menu item activates it, updates the
primary action, and persists `last_shape`.

Rectangle is the missing-field-safe canonical remembered value. The selector
uses the existing anchored menu composition and adds no hover-only behavior or
custom overlay implementation.

### 9.2 Density routing and shortcut

Shapes is directly visible at Wide, Compact, and Narrow widths. Wide and
Compact place it with the core creation tools. Narrow preserves the umbrella's
priority by keeping Shapes visible while Line and Redact use More.

Unmodified `S` activates the remembered shape. It does not cycle kinds and is
ignored when an input captures keyboard events. Existing command-modified Save
and native shortcuts retain precedence. Tooltip text includes the active shape
name and `S` shortcut.

### 9.3 Persisted defaults

Rectangle and Ellipse each persist:

- stroke color;
- stroke width;
- fill enabled;
- remembered fill color.

Both canonical defaults are accent red `#E5484D`, width `4.0`, opacity `1.0`,
fill disabled, and remembered fill color accent red. Disabling fill preserves
the remembered fill color. Creating an annotation copies the active tool's
stroke and either `Some(fill_color)` or `None` into the document.

Missing, malformed, or newly introduced fields resolve independently to
canonical defaults. Rectangle and Ellipse defaults never overwrite one
another. Editing a selected shape never changes either tool's defaults.

Persistence failure retains in-memory values for the current session and
reports the existing one-time non-blocking warning.

### 9.4 Contextual properties

An active Rectangle or Ellipse with no selected-object target shows that
tool's next-object stroke color, width, fill toggle, and fill color. The fill
color control remains visible but disabled while fill is off, which keeps the
property layout stable and makes the available control discoverable.

A selected Shape shows the committed annotation values. Changes use the
existing transient property-preview system. Apply submits one validated style
replacement and one history entry; Cancel discards the preview. Switching
selection or tools follows existing transaction resolution and must not leak a
preview to another target.

Because an outline-only committed annotation intentionally stores no inactive
fill color, enabling fill on a selected outline seeds the transaction from the
current remembered fill color for that annotation's `ShapeKind`. Further
preview changes remain local to the transaction and never update tool defaults.
Disabling and re-enabling fill within the same transaction retains its preview
color.

Properties expose no shape-kind conversion, stroke toggle, opacity, corner
radius, dash, rotation, or other deferred control.

## 10. Output, Navigator, And Failure Semantics

Navigator includes `Rectangle` and `Ellipse` labels with normalized logical
bounds top-left anchors and existing stable-ID tie breaking. It does not expose
style details.

Copy and Save flatten every committed Shape at full source resolution. Copy
Original remains source-identical. Drafts, property previews, selection
outlines, hover feedback, and handles never enter output.

Successful shape edits update the document state ID, durable dirty state, and
Navigator cache through existing paths. Defaults and `last_shape` changes are
configuration changes, not document edits, and do not mark the image dirty.

A rejected create, resize, move, or style edit leaves document, history,
selection, dirty state, Navigator, defaults, and source unchanged and reports
the existing inline error. Copy, Save, flatten, clipboard, and dialog failures
retain their existing rollback behavior.

Opaque Redaction remains the only secure-redaction annotation. Shapes never
enter secure-sharing classification, OCR privacy masks, or secure-redaction
wording, regardless of fill color or opacity.

## 11. Compatibility And Scope Boundaries

All exhaustive `Annotation` and render-command consumers must be inspected.
Result Workspace gains the complete create/edit lifecycle. Timeline and Action
Guide gain only the render compatibility required to display shared document
commands. Automation proposal lowering, workbench review, and agent tool
contracts do not gain shape creation operations.

Existing Number, Text, Opaque Redaction, Line, and Arrow constructors, edits,
history, rendering, and output remain compatible. Shared resize-handle geometry
must not change Opaque Redaction behavior or loosen its fixed opaque-black
rendering.

The Result Workspace behavior is shared by Linux and macOS. Capture Overlay is
unchanged on both platforms, so this slice does not edit or runtime-test the
Linux layer-shell or macOS ScreenCaptureKit overlay paths as changed surfaces.
Platform runtime verification still covers the shared Result Workspace and its
native clipboard and Save As handoffs.

## 12. Automated Verification

### 12.1 Document and history

- Shape kinds, canonical and explicit-style construction, stable IDs, and
  style equality.
- Rejection of every invalid bounds and style class.
- Create, bounds edit, style edit, delete, undo, redo, redo clearing, and no-op
  history behavior.
- Navigator labels, anchors, reading order, and stable-ID ties.

### 12.2 Geometry and interaction

- Four reverse-drag quadrants and normalization.
- Four-screen-pixel two-axis threshold at multiple zoom levels.
- Source-bound clamping near every edge and corner.
- Square/circle creation and live Shift toggling.
- Eight handle locations, priorities, crossing, and minimum interactive size.
- Corner aspect preservation and edge-handle Shift immunity.
- Body movement and one-entry release commits.
- Rectangle interior, Ellipse interior, ellipse-corner misses, stroke
  tolerance, handle/body priority, and topmost selection.

### 12.3 Defaults, properties, toolbar, and keyboard

- Independent Rectangle/Ellipse defaults and missing-field-safe persistence.
- Fill-color retention while disabled and no cross-tool contamination.
- Remembered shape persistence and canonical Rectangle fallback.
- Tool-default and selected-object property targets.
- Preview, Apply, Cancel, target switching, one-step undo, and invalid rollback.
- Wide, Compact, and Narrow Shapes visibility and More routing.
- Primary icon/label, selector choices, menu state, active state, tooltip, and
  `S` shortcut with captured-input and command-modifier precedence.

### 12.4 Rendering and output

- Rectangle and Ellipse command geometry, fill-before-stroke order, centered
  stroke, culling bounds, and clipping.
- Live and flattened geometry agreement with documented edge tolerance.
- Deterministic solid fill and annotation compositing order.
- Draft, preview, selection, hover, and handles excluded from flattening.
- Copy, Save, Copy Original, dirty state, and failure rollback.
- Opaque Redaction security and OCR privacy-mask regressions.

### 12.5 Compatibility and scale

- Existing Number, Text, Opaque Redaction, Line, and Arrow suites.
- Automation, workbench, Timeline, and Action Guide consumers.
- The existing long-image 100-annotation test extended with representative
  Rectangle and Ellipse annotations without changing its history-limit intent.
- Full-resolution coordinates under zoom/downscaled display and viewport
  culling with mixed annotations.

Workspace verification runs:

```bash
rtk cargo test
rtk cargo fmt --all --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

This slice does not touch `rollshot-core` stitching paths and does not run the
stitching benchmark workflow.

## 13. Linux And macOS Runtime Verification

Both platform Result Workspace paths verify:

- Shapes primary action, chevron selector, remembered shape, active state,
  tooltip, `S` shortcut, and Wide/Compact/Narrow routing.
- Repeated Rectangle and Ellipse creation, reverse drag, minimum threshold,
  persistent active tool, and cancellation.
- Live Shift square/circle creation.
- Eight-direction resize, handle crossing, corner aspect lock, edge resize,
  body movement, cursors, deletion, undo, and redo.
- Independent defaults, fill toggle/color retention across restart, and
  selected-object Preview, Apply, Cancel, and one-step undo.
- Rectangle and Ellipse selection over light, dark, and visually busy images,
  including ellipse bounding-box corner misses.
- Stroke/fill legibility and live-versus-flattened visual agreement.
- Navigator, Copy, Save As, Copy Original, dirty state, zoom, pan, long-image
  coordinates, viewport culling, native clipboard, and file-dialog handoff.

Capture Overlay is unchanged. Platform risk is confined to the shared Result
Workspace and existing native clipboard and file-dialog integrations.

## 14. Completion Criteria

Slice 3 is complete only when:

1. Rectangle and Ellipse satisfy their complete create-through-output
   lifecycles.
2. One shared Shape family drives validation, history, geometry, hit testing,
   handles, Navigator, live rendering, and flattening without duplicated
   Rectangle/Ellipse lifecycle systems.
3. Reverse drag, source bounds, minimum size, eight-direction resize, handle
   crossing, and Shift semantics pass automated and Linux/macOS runtime
   verification.
4. Independent defaults, remembered Shapes behavior, toolbar routing,
   shortcut, and selected-object properties extend the existing systems
   without changing prior annotation behavior.
5. Live Canvas and full-resolution flattening agree on Rectangle/Ellipse
   geometry, fill, centered stroke, compositing, and clipping.
6. Opaque Redaction remains fixed, fully opaque, and the only secure-redaction
   annotation.
7. Existing Number, Text, Line, Arrow, automation, workbench, Timeline, Action
   Guide, Copy, Save, Navigator, and dirty-state behavior remains compatible.
8. All required automated checks and both platform runtime checklists pass,
   the umbrella registry records `Complete`, and no required Slice 3 work
   remains.

Only then may Slice 4 implementation begin.
