# Annotation Editor Umbrella Design

**Date:** 2026-07-12

**Status:** Approved umbrella design

**Scope:** Result Workspace annotation editing and the shared
`rollshot-image-document` contracts that support it

**Research:** `docs/researchs/annotation-tools-reference-survey.md`

## 1. Purpose

Rollshot will evolve the Result Workspace from a viewer with three fixed-style
callout types into a lightweight, non-destructive screenshot annotation editor.

This is an umbrella specification. It fixes the product boundary, shared data
and rendering contracts, committed capabilities, UX direction, safety
semantics, implementation slices, and cross-slice acceptance criteria. It is
not one implementation plan. Each slice defined in this document requires its
own live sub-project specification and implementation plan before code changes
begin.

This umbrella remains a live program-control document until all slices are
complete. Each slice transition, handoff, and completion must update the Slice
Status Registry in this document. Once the complete program lands, this file
becomes a historical snapshot under the normal `docs/superpowers/` rules.

The design builds on the existing Result Workspace and
`rollshot-image-document`; it does not replace their established ownership
boundaries.

## 2. Product Position

The product is:

> A lightweight, non-destructive screenshot annotation editor inside the
> Result Workspace, optimized for both ordinary screenshots and very long
> captures.

It is not a general drawing application. It is not an editable project-file
system. It does not move annotation into capture or stitching. The shared
document crate remains reusable, but this program designs and delivers only the
Result Workspace product experience.

## 3. Goals

- Let users explain, emphasize, and safely redact screenshots without leaving
  Rollshot.
- Make the existing Number, Text, and Opaque Redaction annotations visually
  configurable where that does not weaken safety.
- Add the drawing tools common to mature screenshot editors: Line, Arrow,
  Rectangle, Ellipse, Pen, Highlighter, and Pixelate.
- Give every committed annotation complete create, preview, select, edit,
  delete, undo/redo, and flattened-export behavior.
- Preserve the immutable full-resolution source throughout editing.
- Keep live rendering and flattened output visually consistent.
- Keep controls usable while navigating very tall images.
- Deliver the program through independently reviewable vertical slices.

## 4. Non-Goals

- Annotation UX in Capture Overlay, Action Guide, or agent proposal surfaces.
- Editable project files, sidecars, or reloading an annotation graph after the
  Result Workspace closes.
- Multi-selection, group transforms, rotation, or a layers panel.
- Magnifier, Watermark, Invert, Laser, or export-frame effects.
- Blur in the committed program.
- Font-family selection, dash styles, arrowhead variants, rectangle corner
  radius, or alternate number formats.
- Generic image filters or a plugin API for arbitrary annotation tools.
- Annotation during capture or stitching.
- Platform-specific Result Workspace annotation behavior.

Deferred features must not leak speculative fields or UI affordances into the
committed implementation. A future feature receives its own specification.

## 5. Committed Capability Set

### 5.1 Existing annotations to improve

- Number Callout
- Text Note
- Opaque Redaction

### 5.2 New annotation tools

- Line
- Arrow
- Rectangle
- Ellipse
- Pen
- Highlighter
- Pixelate

### 5.3 Shared editor capabilities

- Contextual color, size, fill, opacity, and effect controls as applicable.
- Editing the properties of a selected annotation.
- Per-tool next-object defaults persisted across sessions.
- Undo and redo for completed property changes.
- A responsive two-row toolbar with low-frequency overflow.
- Long-image coordinate correctness and performance.
- Deterministic full-resolution flattened Copy and Save output.

## 6. Program Invariants

Every downstream slice must preserve these invariants:

1. The source image is immutable through every document edit.
2. Annotation geometry and style live in full-resolution image coordinates or
   image-space values, never viewport-scaled values.
3. Live preview and flattened output share framework-neutral rendering
   semantics.
4. One completed gesture or property edit creates at most one history entry.
5. Draft, hover, selection, and handles never enter flattened output.
6. Copy and Save produce a flattened raster image; no editable project format
   is introduced.
7. Opaque Redaction is the only annotation with a secure-redaction promise. It
   remains completely opaque and cannot acquire opacity or visual-effect
   controls.
8. Pixelate, and any future Blur tool, is described as visual obfuscation, not
   secure removal of information.
9. Result Workspace is the only product surface in scope. Shared code may be
   reused, but no speculative UI API is designed for other surfaces.
10. Linux and macOS use the same Result Workspace behavior.

Completion means every committed tool satisfies the whole vertical contract,
not merely that its toolbar button or document variant exists.

## 7. Ownership And Architecture

The existing boundary remains:

```text
rollshot-app
  active tool / defaults / drafts / selection / toolbar / gestures
                              |
                              | completed edits
                              v
rollshot-image-document
  immutable source / annotations / geometry / hit testing / history
  framework-neutral render commands / full-resolution flatten
```

### 7.1 `rollshot-app` owns

- Active tool and last-used shape subtool.
- Per-tool next-object defaults and their persistence.
- Selection, hover, pointer state, modifiers, and transient drafts.
- Gesture interpretation and geometry constraints.
- Property-control interaction and preview state.
- The two-row toolbar, More menu, tooltips, and shortcuts.
- iced live rendering and preview caches.
- Clipboard, Save As, inline messages, and native integration.

### 7.2 `rollshot-image-document` owns

- Immutable source pixels.
- The annotation graph and stable annotation IDs.
- Annotation geometry and committed style values.
- Validation of completed edits.
- Bounds, shape-specific hit testing, handles, and Navigator anchors.
- Number sequence semantics.
- Undo and redo history.
- Framework-neutral render commands.
- Deterministic full-resolution flattening.

The document crate does not own active tools, pointer movement, viewport state,
property widgets, configuration persistence, clipboard, or file dialogs.

## 8. Annotation Model

The conceptual model groups annotations that share geometry while keeping
styles explicit and type-safe:

```rust
enum Annotation {
    NumberCallout {
        id: AnnotationId,
        number: u32,
        tip: ImagePoint,
        bubble: ImagePoint,
        style: NumberStyle,
    },
    TextNote {
        id: AnnotationId,
        position: ImagePoint,
        text: String,
        style: TextStyle,
    },
    OpaqueRedaction {
        id: AnnotationId,
        bounds: ImageRect,
    },
    TwoPoint {
        id: AnnotationId,
        kind: TwoPointKind, // Line | Arrow
        start: ImagePoint,
        end: ImagePoint,
        style: StrokeStyle,
    },
    Shape {
        id: AnnotationId,
        kind: ShapeKind, // Rectangle | Ellipse
        bounds: ImageRect,
        stroke: StrokeStyle,
        fill: Option<FillStyle>,
    },
    Freehand {
        id: AnnotationId,
        kind: FreehandKind, // Pen | Highlighter
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    },
    Pixelate {
        id: AnnotationId,
        bounds: ImageRect,
        block_size: f32,
    },
}
```

Exact Rust names and representation belong to the slice plans. The required
properties are:

- Geometry families share bounds, hit-testing, and editing behavior where the
  semantics genuinely match.
- There is no universal property bag containing irrelevant optional fields.
- `StrokeStyle` contains color, positive width, and opacity.
- Ordinary drawing tools use fully opaque defaults. Only Highlighter exposes
  stroke opacity in the committed UI.
- Text and Number use dedicated style types.
- Opaque Redaction exposes no style capable of weakening opacity.
- Pixelate stores effect parameters, not derived or already-pixelated pixels.
- All committed annotations retain stable identity through applicable edits
  and undo/redo.

## 9. Style Controls

### 9.1 Common controls

Use a compact common palette plus a custom color picker. Controls shown depend
on the active creation tool or selected annotation.

- Line, Arrow, Pen: stroke color and stroke width.
- Rectangle, Ellipse: stroke color, stroke width, fill on/off, fill color.
- Text: font size, text color, background on/off, background color.
- Number: accent color and size.
- Highlighter: color, width, opacity.
- Pixelate: block size.
- Opaque Redaction: no color or opacity controls.

Font family, advanced typography, dash style, arrowhead variants, corner
radius, and number formats are deferred.

### 9.2 Tool defaults

Per-tool defaults are app configuration, not document state:

- Creating an annotation copies current tool defaults into that annotation.
- Editing a selected annotation does not silently change any tool default.
- Applying a selected style as a future default would require an explicit
  future action.
- Defaults persist across Result Workspace sessions.
- Missing or newly introduced configuration fields resolve to canonical
  defaults.
- The annotation graph itself remains session-only.

## 10. Result Workspace Layout

Use the approved two-row layout.

```text
Close | title                    Undo Redo | Copy dropdown | Save As
Select Number Text Arrow Shapes Pen Highlight Redact Pixelate | properties
```

### 10.1 First row

- Approximately 40 pixels high.
- Keeps Close and title on the left.
- Keeps Undo and Redo near the editing controls.
- Separates Undo/Redo visually from output actions.
- Pins Copy and Save As to the right at every supported width.
- Copy and Save As never enter overflow.

### 10.2 Second row

- Approximately 36–40 pixels high.
- Contains annotation tools followed by contextual properties.
- Select with no selected annotation hides the property cluster.
- Selecting an annotation shows only properties supported by that annotation.
- A creation tool shows its next-object defaults.
- Active tools use a visible selected treatment while retaining a tooltip and
  shortcut hint.
- Width pressure moves low-frequency tools into More instead of compressing
  the entire row into unlabeled icons.

The default narrow-width priority keeps Select, Number, Text, Arrow, Shapes,
and Pen visible. Highlighter, Redact, and Pixelate enter More as space requires.
If an active tool is inside More, the More control displays active state and
the current tool name so mode remains visible.

Shapes is a small Rectangle/Ellipse selector that remembers the last-used
shape. Its primary action reuses that shape.

## 11. Interaction Model

### 11.1 Common rules

- Select is the default tool.
- A creation tool remains active after one successful creation.
- Selected annotations expose only their supported handles and properties.
- A creation tool does not implicitly select an annotation under the pointer;
  users switch to Select for existing-object editing.
- `Delete` and `Backspace` delete the selected annotation.
- Shift constrains relevant creation and handle gestures.
- Alt-from-center drawing, rotation, and multi-selection are deferred.

`Esc` resolves the most local state first:

1. Cancel an active draft or property interaction.
2. Clear selection.
3. Switch the active creation tool back to Select.
4. Apply the existing workspace close and dirty-state behavior.

### 11.2 Number Callout

- Preserve click-to-stamp and drag-to-separate-leader behavior.
- Expose accent color and size.
- Permit setting the next number.
- Preserve compact renumbering after deletion and exact undo restoration.

### 11.3 Text Note

- Click opens the existing inline editor.
- Existing text can be reopened and edited.
- Expose font size, text color, and optional background color.
- A complete text edit is one history entry, not one entry per keystroke.

### 11.4 Line And Arrow

- Drag creates a two-point annotation.
- Select permits body movement and independent endpoint movement.
- Shift snaps creation and endpoint movement to 45-degree increments.
- The first Arrow design has one reviewed, legible arrowhead.

### 11.5 Rectangle And Ellipse

- Drag creates an axis-aligned box.
- Select permits body movement and eight-direction resize handles.
- Shift constrains creation to a square or circle.
- Fill is optional and independent of the stroke color.
- Ellipse hit testing follows the ellipse, not only its bounding box.

### 11.6 Pen And Highlighter

- Pointer drag creates one path annotation.
- Sampling and simplification preserve the visible path within a documented
  tolerance.
- Pointer release commits one history entry.
- Highlighter shares path geometry but has independent width and opacity
  defaults and explicitly defined alpha compositing.

### 11.7 Opaque Redaction

- Preserve drag creation, movement, and resize behavior.
- Preserve completely opaque source-pixel replacement in flattened output.
- Show no style control that could weaken the secure-redaction promise.

### 11.8 Pixelate

- Drag creates an axis-aligned effect region.
- Select permits movement and resize.
- Expose only block size in the committed property UI.
- Label the tool and its output as visual obfuscation, not secure redaction.

## 12. Edit And History Semantics

The app owns transient interaction. The document receives completed create,
replace, or delete operations.

```text
tool defaults or selected style
              |
              v
transient app draft
              |
              | pointer release or property commit
              v
validated document edit
              |
              v
annotation graph plus one history entry
```

- Pointer movement never enters document history.
- Property controls may preview continuously, but a completed slider or color
  interaction creates one undo entry.
- Invalid or cancelled edits create no history entry.
- A new successful edit after undo clears redo history.
- Selection and active tool remain editor state and do not enter history.
- Number sequence remains part of document history.
- Dirty state and Navigator refresh only after a successful commit.

## 13. Rendering

The current framework-neutral rendering boundary expands beyond fixed
`RenderShape` output into render commands that can describe all committed
annotations.

### 13.1 Vector commands

The shared semantics must cover paths, lines, polygons, ellipses, filled
rectangles, and text with explicit color, width, fill, and alpha behavior.

The iced live renderer and the full-resolution raster flattener consume the
same geometry and style rules. They may use different rendering libraries, but
must agree on bounds, path shape, stroke placement, fill, text layout, and
compositing semantics.

### 13.2 Pixelate commands

Pixelate is a raster-effect command containing a source-space region and block
size.

- The live renderer may use a preview cache keyed by source identity, region,
  and block size.
- Moving, resizing, or changing block size invalidates the relevant cache.
- Flatten always computes from the immutable full-resolution source.
- Pixelate never samples already-pixelated preview or prior effect output.
- A stale or missing preview cache cannot affect Copy or Save output.

Selection, hover, handles, and drafts remain app-only overlays.

## 14. Navigator And Output

Navigator supports every committed annotation kind with a stable label and
reading-order anchor. It does not display complete style details.

- Number retains its number label.
- Text retains a short text summary.
- Other annotations use stable kind labels such as Arrow, Rectangle, Pen,
  Redaction, and Pixelate.
- Reading order continues to use image-space position with stable-ID tie
  breaking.

Copy and Save preserve existing output semantics:

- Primary Copy and Save use the flattened full-resolution document.
- Copy Original continues to use the immutable source.
- Output excludes drafts, selection, hover, and handles.
- A successful Copy does not clear durable dirty state.
- A successful Save As updates the export path without changing the source
  path and clears dirty state according to existing behavior.
- Output failure leaves paths, dirty state, document, and source unchanged.

## 15. Validation And Error Handling

- The document rejects non-finite coordinates, non-positive stroke widths,
  opacity outside its valid range, invalid block sizes, and zero-area box
  annotations.
- UI controls clamp or reject invalid intermediate input, but the document
  independently validates every completed edit.
- A rejected edit leaves annotation graph, history, dirty state, and selection
  unchanged and produces a non-blocking inline error.
- Tool-default persistence failure does not block editing. The current session
  retains in-memory defaults and reports one warning.
- Pixelate preview-cache failure triggers recomputation or a temporary region
  outline. It never substitutes stale preview pixels into output.
- Copy, Save, or flatten failure uses existing inline failure behavior and
  never reports success or clears dirty state.
- Opaque Redaction has no style API capable of lowering its opacity.

## 16. Compatibility

`rollshot-image-document` is shared infrastructure. Model changes must inspect
and preserve all existing consumers, including Result Workspace, automation
proposal lowering, workbench review, and their tests.

Existing consumers create Number, Text, and Opaque Redaction through canonical
constructors that apply reviewed defaults. They must not duplicate style
constants across crates.

No editable-project migration is required because annotation graphs are not
persisted. Tool-default configuration remains backward compatible through
missing-field-safe canonical defaults.

## 17. Implementation Slices

Each slice is a separate sub-project with its own live specification,
implementation plan, review, and verification.

### 17.1 Required superpowers lifecycle

Every slice follows the full superpowers workflow independently:

1. Invoke `superpowers:brainstorming` for that slice. Explore current code and
   the landed output of the preceding slice, resolve slice-specific product and
   engineering decisions, write the slice spec, obtain user approval, and
   commit the approved spec.
2. After the user reviews the written slice spec, invoke
   `superpowers:writing-plans` to create its implementation plan. The plan must
   cite both this umbrella and the approved slice spec.
3. Execute only the approved slice plan, using
   `superpowers:executing-plans` or
   `superpowers:subagent-driven-development` as appropriate to the session.
   Implementation follows `superpowers:test-driven-development` for feature
   and bug-fix work.
4. Before any completion claim, invoke
   `superpowers:verification-before-completion` and run the slice's full
   automated and runtime verification. Use
   `superpowers:requesting-code-review` before integration when applicable.
5. Use `superpowers:finishing-a-development-branch` for the integration or
   handoff decision when the slice implementation is complete and verified.
6. Update and commit the Slice Status Registry below as part of every handoff,
   blocked transition, or completion. The registry update is required work,
   not optional release notes.

The next slice may perform read-only research or an explicitly approved spike,
but its implementation does not begin until the preceding slice is marked
`Complete` in this umbrella. A downstream discovery that conflicts with an
umbrella invariant stops the slice: revise this umbrella with user approval
before changing the slice spec or implementation to diverge from it.

### 17.2 Slice status registry

Allowed statuses are `Not started`, `Brainstorming`, `Spec approved`,
`Planned`, `In progress`, `Handoff`, `Blocked`, and `Complete`.

| Slice | Status | Slice spec | Implementation plan | Implementation / verification | Last update |
|---|---|---|---|---|---|
| 1 — Editor And Style Foundation | Complete | [`2026-07-12-editor-and-style-foundation-design.md`](2026-07-12-editor-and-style-foundation-design.md) (`ec909a0`) | [`2026-07-12-editor-and-style-foundation.md`](../plans/2026-07-12-editor-and-style-foundation.md) (`b722d2d`) | Landed in PR #90 (`745d424`); automated verification and Linux/macOS runtime verification complete; no required work remains | 2026-07-13 |
| 2 — Two-Point Tools | Complete | [`2026-07-13-two-point-tools-design.md`](2026-07-13-two-point-tools-design.md) (`af4690c`) | [`2026-07-13-two-point-tools.md`](../plans/2026-07-13-two-point-tools.md) (`afdd260`) | Landed in PR #91 (`79d080d`); automated verification and Linux/macOS CI pass. The complete native Linux/macOS Result Workspace runtime checklists were not executed in the headless implementation environment; on 2026-07-14 the user explicitly accepted that documented runtime risk, approved Slice 2 as complete, and confirmed no further Slice 2 work is required. | 2026-07-14 |
| 3 — Box Tools | Brainstorming | — | — | Codex brainstorming session started; reviewing the landed editor architecture and Rectangle/Ellipse UX in Snow Shot, Flameshot, mark-shot, and KDE Spectacle before resolving slice-specific decisions. | 2026-07-14 |
| 4 — Freehand Tools | Not started | — | — | — | 2026-07-12 |
| 5 — Pixelate Effect | Not started | — | — | — | 2026-07-12 |
| 6 — Integrated Hardening | Not started | — | — | — | 2026-07-12 |

Registry update requirements:

- `Brainstorming`: record the start date and current owner/session in the
  implementation/verification column.
- `Spec approved`: link the committed slice spec and record its commit or PR.
- `Planned`: link the implementation plan and record its commit or PR.
- `In progress`: record the implementation branch, commit range, or PR.
- `Handoff`: record completed tasks, fresh verification evidence, remaining
  tasks, known risks, and the exact next entry point. A handoff is not
  completion and does not unlock the next slice.
- `Blocked`: record the blocking condition, evidence, and the decision or
  external change required to resume.
- `Complete`: link the landed implementation, record automated and platform
  runtime verification, record the completion date, and confirm that no
  required work remains. Only `Complete` unlocks implementation of the next
  slice.
- Every registry transition is committed so a new session can recover program
  state from the repository without relying on chat history.

### 17.3 Slice 1 — Editor And Style Foundation

- Annotation style value types and edit/history contracts.
- Canonical constructors and compatibility updates.
- Approved two-row toolbar, responsive More, and contextual properties.
- Per-tool defaults and persistence.
- Number and Text style controls.
- Opaque Redaction safety isolation.
- Selected-object property editing with one-entry undo semantics.

This slice lands first. Later slices may not create competing property,
default, or style systems.

### 17.4 Slice 2 — Two-Point Tools

- Line and Arrow.
- Shared two-point geometry, bounds, hit testing, and render commands.
- Endpoint handles, body movement, and Shift snapping.
- Live/flatten consistency.

### 17.5 Slice 3 — Box Tools

- Rectangle, Ellipse, and Shapes selector.
- Stroke and optional fill.
- Eight-direction resize and body movement.
- Shift aspect constraint and ellipse-specific hit testing.

### 17.6 Slice 4 — Freehand Tools

- Pen and Highlighter.
- Pointer sampling and path simplification.
- Path bounds, hit testing, and movement.
- Highlighter compositing.
- Long-stroke preview and flatten performance.

### 17.7 Slice 5 — Pixelate Effect

- Pixelate annotation and block-size property.
- Move and resize behavior.
- Live preview cache.
- Immutable-source full-resolution flattening.
- Product wording and type separation from Opaque Redaction.

### 17.8 Slice 6 — Integrated Hardening

- Cross-tool shortcuts and tooltip consistency.
- More-menu active state and narrow-window behavior.
- Selection/property switching edge cases.
- Long-image and mixed-annotation performance.
- Copy/Save, dirty-state, and Navigator integration.
- Linux and macOS runtime verification.
- User documentation and secure-redaction wording review.

Slice 6 adds no annotation tool.

### 17.9 Slice gate

Every slice must:

- Build on the previously landed slice rather than designing in parallel
  against a competing model.
- Complete its document, app state, canvas, flatten, and test path.
- Meet its automated and runtime acceptance criteria before the next tool
  family begins.
- Avoid deferred roadmap features.

## 18. Automated Verification

Every applicable slice tests:

- Constructors, validation, and style equality.
- Create, replace, delete, undo, and redo.
- Bounds, shape-specific hit testing, handles, and Navigator anchors.
- Draft preview and release commit using the same geometry.
- Live render commands and flattened output semantics.
- Full-resolution coordinates under zoomed or downscaled long-image display.
- Copy and Save including every committed annotation while excluding editor
  overlays.
- Existing Opaque Redaction security behavior.
- Keyboard routing, active tool state, contextual properties, and More routing.
- Existing automation and workbench consumers.

Family-specific suites include:

- Two-point: zero-length rejection, snapping, endpoint editing, and arrowhead
  bounds.
- Box: reverse drag, minimum size, aspect constraint, resize handles, and
  ellipse hit testing.
- Freehand: single-point and short strokes, simplification tolerance, large
  point counts, and alpha compositing.
- Pixelate: block boundaries, edge clipping, move/resize resampling, cache
  invalidation, and prevention of recursive effect sampling.

The complete program retains a long-image test with at least 100 mixed
annotations, matching the existing history-limit scale.

## 19. Performance And Runtime Verification

- Freehand slices record simplification input/output point counts and
  long-stroke render time so path growth is bounded and observable.
- Pixelate records preview-cache hit/miss behavior and full-resolution flatten
  cost.
- Live rendering continues to cull committed annotations outside the visible
  viewport.
- Full-resolution flatten occurs only for explicit output or tests.
- This program does not change `rollshot-core` stitching paths and therefore
  does not trigger the stitching benchmark workflow.

Runtime verification is required on Linux and macOS for:

- Two-row toolbar responsiveness and More behavior.
- Tooltips, shortcuts, and active-state visibility.
- Pointer creation and editing gestures.
- Inline Text editing and contextual style controls.
- Clipboard, Save As, dirty state, and Copy Original.
- Zoom, pan, Navigator jumps, and long-image responsiveness.
- Opaque Redaction versus Pixelate wording and output behavior.

Capture Overlay is unchanged. Platform risk is limited to the shared Result
Workspace and existing native clipboard/file-dialog integration.

## 20. Umbrella Completion Criteria

The umbrella program is complete only when:

1. All six slices have landed through their own approved specs and plans.
2. Every committed annotation supports its complete vertical lifecycle.
3. Existing Number, Text, Redaction, automation, and workbench behavior remains
   compatible.
4. The two-row responsive toolbar meets the approved wide and narrow behavior.
5. Live rendering and flattened output pass family-specific consistency tests.
6. Secure Redaction remains fully opaque and clearly distinguished from
   Pixelate.
7. Long-image mixed-annotation tests and Linux/macOS runtime verification pass.
8. No deferred feature was required to make the committed program coherent.

Only then does this umbrella specification become a historical snapshot.
