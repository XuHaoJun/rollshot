# Pixelate Effect Design

**Date:** 2026-07-15  
**Status:** Approved design  
**Program:** Annotation Editor  
**Slice:** 5 — Pixelate Effect

## 1. Purpose And Authority

This slice adds a complete Pixelate annotation lifecycle to the Result
Workspace. It builds on the landed Editor And Style Foundation, Two-Point
Tools, Box Tools, and Freehand Tools slices and extends their document,
defaults, properties, toolbar, gesture, history, rendering, preview, and
output systems instead of introducing competing systems.

This design is subordinate to
[`2026-07-12-annotation-editor-umbrella-design.md`](2026-07-12-annotation-editor-umbrella-design.md)
and builds on
[`2026-07-14-freehand-tools-design.md`](2026-07-14-freehand-tools-design.md).
The umbrella's program invariants remain authoritative. If implementation
discovers a conflict with an umbrella invariant, this slice stops until the
umbrella is revised with user approval.

Research input comes from
[`annotation-tools-reference-survey.md`](../../researchs/annotation-tools-reference-survey.md)
and the checked-out Snow Shot, Flameshot, mark-shot, and KDE Spectacle
sources. The Pixelate findings that shape this design are:

- mark-shot uses a rectangular Mosaic annotation, stores a per-annotation
  block size, defaults to 14 image pixels, exposes a 4–48 slider, and computes
  region-local average-color blocks from its immutable frozen frame.
- KDE Spectacle exposes Pixelate as a rectangular effect with one Strength
  property. Selected-effect slider changes are coalesced into a committed
  edit rather than producing one edit per slider movement.
- Snow Shot keeps parameterized effect sprites and refreshes them when
  geometry or effect strength changes, which supports a keyed preview-cache
  design.
- Flameshot's default pseudo-pixelation deliberately avoids sampling the
  selected interior to provide a different security property. Rollshot does
  not adopt that exception: Opaque Redaction already owns the secure-removal
  promise, while Pixelate remains ordinary visual obfuscation.

## 2. Goals

- Add a Pixelate annotation backed by immutable-source average-block
  sampling.
- Support creation, live preview, selection, movement, eight-direction
  resize, delete, undo/redo, Navigator, Copy, Save, and full-resolution
  flattening.
- Expose one persisted next-object default and selected-object property:
  block size.
- Keep Pixelate visibly and structurally separate from Opaque Redaction.
- Use one framework-neutral raster-effect command and one mosaic kernel for
  live-preview and flattened-output semantics.
- Keep live interaction responsive on long screenshots through a bounded,
  invalidation-correct preview cache.
- Record cache behavior and pixelation costs through retained structured
  tracing.

## 3. Non-Goals

- Blur, pseudo-pixelation, secure mosaic, effect-type selection, arbitrary
  filters, or a generic plugin effect system.
- Color, opacity, blend-mode, feathering, shape, or border controls for
  Pixelate.
- Pixelate proposals in automation, workbench, Action Guide, Timeline, or
  Capture Overlay product surfaces.
- Multi-selection, group transforms, rotation, layer reorder, or an editable
  project format.
- Alt-from-center creation or resize.
- Changes to capture backends, platform overlay runners, or stitching core.
- Reclassifying Pixelate output as securely redacted content.

## 4. Approved Product Decisions

- The product label is `Pixelate`, not Mosaic, Blur, or Redact.
- Pixelate is visual obfuscation only. The canonical explanatory copy is:
  `Visual obfuscation only — use Redact to securely remove information.`
- Opaque Redaction remains the only annotation with a secure-removal promise.
- Pixelate uses region-local average-color blocks sampled from the immutable
  source.
- The canonical block-size default is `16` full-resolution image pixels.
- The accepted block-size range is the inclusive integer range `4..=48`.
- Block size is independent of viewport scale and annotation bounds. Moving
  or resizing a Pixelate annotation never scales its block size.
- The block grid starts at the annotation region's rasterized top-left. A
  move or resize therefore resamples both the source region and its local
  grid.
- Pixelate uses shortcut `B`, matching Flameshot's shortcut for its Pixelate
  tool and avoiding Rollshot's existing `P` shortcut for Pen.
- Pixelate remains active after successful creation and does not select the
  new annotation.
- Shift constrains creation to a square and selected resize to the original
  aspect ratio. Alt-from-center remains deferred.
- The selected design is region-local CPU mosaic plus a byte-bounded preview
  cache. Whole-image mosaic pyramids and a custom GPU shader are rejected for
  this slice because of long-image memory cost and duplicated live/flatten
  sampling semantics respectively.

## 5. Ownership And Architecture

```text
rollshot-app
  active Pixelate tool / persisted 16 px default / draft / selection
  Shift constraints / property transaction / preview requests + cache
  toolbar + More / safety copy / iced image-handle rendering
                              |
                              | completed edits
                              v
rollshot-image-document
  Pixelate annotation / validation / history / bounds + hit testing
  Navigator / RenderCommand::Pixelate / average-block mosaic kernel
  deterministic immutable-source full-resolution flatten
```

### 5.1 `rollshot-image-document` owns

- The Pixelate annotation's stable ID, image-space bounds, and block size.
- Bounds and block-size validation.
- Add, bounds-update, block-size-update, delete, and history semantics.
- Rectangle bounds, body hit testing, resize handles, and Navigator data.
- Framework-neutral Pixelate render-command lowering.
- Integer raster-region conversion and average-block sampling semantics.
- Full-resolution flattening from the immutable source.

### 5.2 `rollshot-app` owns

- The active Pixelate tool, pointer draft, selection, modifiers, and transient
  edits.
- The persisted next-object block-size default.
- Transactional selected-object block-size preview and commit.
- Preview-cache keys, in-flight request tracking, eviction, and iced image
  handles.
- Toolbar density routing, shortcut, tooltip, and the `Not secure` property
  label.
- Cache-failure warning and temporary region-outline fallback.

The preview cache is app state, not document state. No preview pixels or cache
metadata enter snapshots, history, flattening, or serialization.

## 6. Document Model And Edit Contract

The conceptual annotation variant is:

```rust
Annotation::Pixelate {
    id: AnnotationId,
    bounds: ImageRect,
    block_size: u32,
}
```

Pixelate is not a style of Opaque Redaction and is not represented through a
generic property bag or a generic raster-effect kind. The graph stores only
the parameters required to deterministically regenerate the effect; it never
stores copied or already-pixelated pixels.

The typed edit surface adds:

```rust
EditOp::AddPixelate {
    bounds: ImageRect,
    block_size: u32,
}

EditOp::UpdatePixelateBounds {
    id: AnnotationId,
    bounds: ImageRect,
}

EditOp::UpdatePixelateBlockSize {
    id: AnnotationId,
    block_size: u32,
}
```

Canonical convenience methods may wrap these operations, but they must not
create a second validation or history path.

Validation rules are:

- Bounds coordinates and dimensions are finite.
- Bounds are normalized by the app and clamped to the source by the document.
- Bounds covering less than one source pixel after clamping are rejected.
- Block size is an integer in `4..=48` inclusive.
- An update addressed to another annotation kind returns `WrongKind`.
- A no-op update creates no history entry or state-ID change.

Any rejected standalone or batch operation leaves annotations, IDs, counters,
history, redo state, and state ID unchanged.

## 7. Mosaic Sampling Semantics

Pixelate rasterization first converts the clamped `ImageRect` to the same
crisp integer-pixel coverage used by other axis-aligned replacement effects.
The local block grid begins at that raster region's top-left.

For each block:

1. Intersect the nominal `block_size × block_size` cell with the raster
   region and source bounds.
2. Read only the immutable source pixels inside that intersection.
3. Average in premultiplied RGBA space.
4. Convert the result back to straight RGBA and fill the whole intersected
   cell with that value.

Premultiplied averaging avoids color fringes when a source contains
transparent pixels. Captured screenshots are normally opaque, but the kernel
remains correct for any valid `RgbaImage`.

Right and bottom edge cells may be smaller than `block_size`; their average
uses only their actual pixels. A valid region smaller than the block size
produces one average-color block. The kernel is deterministic and contains no
random sampling.

## 8. Render Commands And Paint Order

The framework-neutral boundary expands from vector-only `RenderShape` output
to an explicit command:

```rust
enum RenderCommand {
    Shape(RenderShape),
    Pixelate {
        bounds: ImageRect,
        block_size: u32,
    },
}
```

`annotation_commands()` lowers each committed annotation in graph order.
Existing vector annotations keep their established geometry and paint order;
the command wrapper does not change their appearance. Existing optimized
paths, such as borrowed Freehand point rendering in iced, may remain as
semantically equivalent fast paths.

Both live rendering and flattening obey these Pixelate rules:

- Sampling always reads the immutable source, never the partially flattened
  destination, a preview texture, or another Pixelate result.
- The generated mosaic writes over the destination at the Pixelate
  annotation's graph position.
- Earlier annotations inside the region are therefore covered by the
  Pixelate result.
- Later annotations render over the Pixelate result.
- Overlapping Pixelate annotations do not recursively increase pixelation;
  each independently samples the immutable source.

This preserves meaningful annotation paint order without allowing recursive
effect sampling.

## 9. Live Preview Cache

The iced canvas draws a cached mosaic image at the Pixelate command's position
in the normal annotation order. A cache key contains:

```text
source identity
integer raster region
block size
display/downscale scale
```

The cache is scoped to one Result Workspace, uses least-recently-used
eviction, and accounts for uncompressed preview bytes. Its limit is `64 MiB`.
One currently requested entry may temporarily exceed that limit when a single
valid visible preview is larger; other entries are evicted first, and the
oversized entry is not retained after it is no longer requested.

Preview generation runs outside the UI update path against shared read-only
source ownership. Each key has at most one in-flight generation. Completion
messages carry the complete key and request generation. A completion is
accepted only if the workspace source and current requested key still match;
otherwise it is discarded without changing the displayed cache entry.

Cache lookups and requests cover visible committed Pixelate annotations plus
the active Pixelate draft, direct-manipulation preview, or property preview.
Missing visible entries are requested from existing document/viewport update
paths; no parallel subscription is introduced.

The following changes invalidate or replace the relevant key:

- Create-draft bounds.
- Selected movement or resize bounds.
- Tool-default or selected-object block-size preview.
- Commit, delete, undo, and redo.
- Workspace/source replacement.
- Display downscale or zoom representation changes that alter the preview
  image scale.

Scroll-only movement does not invalidate a region image, but newly visible
Pixelate annotations must be requested. Cache misses, generation failures,
and in-flight requests render only the region outline. Stale pixels from a
different key are never stretched or translated as a substitute.

Full-resolution flatten never reads, waits for, or writes the preview cache.

## 10. Creation And Direct Manipulation

### 10.1 Creation

- Press stores the image-space anchor and current 16 px default.
- Pointer movement updates a normalized, image-bounded draft.
- Shift constrains the draft to a square using the existing box-creation
  constraint and image-bound clamping.
- Release commits one `AddPixelate` when the region covers at least one source
  pixel.
- A click or sub-pixel region cancels without history.
- Escape cancels the draft without history.
- A successful creation leaves Pixelate active and does not select the new
  annotation.
- A creation tool never selects or edits an existing annotation under the
  pointer. Existing-object editing requires Select.

### 10.2 Selection, movement, and resize

- Select body hit testing uses the rectangular region.
- A selected Pixelate displays eight zoom-independent resize handles.
- Body movement keeps the whole region inside image bounds and preserves its
  width, height, and block size.
- Handle dragging normalizes inverted drags and clamps to image bounds.
- Shift resize preserves the original aspect ratio.
- Move or resize pointer movement remains app-only preview state; release
  submits one `UpdatePixelateBounds`.
- Escape restores the committed annotation.
- Delete and Backspace use the existing selected-annotation deletion path.

## 11. Toolbar, Defaults, And Properties

The second-row wide/compact order becomes:

```text
Select Number Text Line Arrow Shapes Pen Highlighter Redact Pixelate
```

Narrow density keeps the umbrella's existing visible priority and puts
Pixelate in More with the other lower-frequency tools. If Pixelate is active
inside More, More shows active treatment and the current tool name.

The toolbar item is:

```text
label: Pixelate
shortcut: B
tooltip: Pixelate (B) — visual obfuscation only; use Redact to securely remove information.
```

Captured keyboard input blocks `B` like every existing tool shortcut.

`AnnotationDefaults` adds one serde-defaulted integer `pixelate_block_size`.
Its canonical default is 16. Missing fields use 16 without warning; malformed,
non-integer, or out-of-range values use 16 and report one load warning through
the existing defaults-warning path. Persistence failure retains the in-memory
value and uses the existing one-warning behavior.

The only contextual properties are:

```text
Block size  [4 .. 48 integer slider, current value shown as “16 px”]  Not secure
```

- With the Pixelate creation tool active, the slider previews and then
  persists the next-object default. It creates no document history.
- With a Pixelate annotation selected, the slider previews through app-only
  state. Release submits one `UpdatePixelateBlockSize` and one history entry.
- Editing a selected annotation never changes the tool default.
- Escape, Undo, Redo, tool switch, or selection switch cancels an unfinished
  block-size transaction and clears its preview request.
- The `Not secure` label is continuously visible for both tool-default and
  selected-Pixelate targets. It is not a modal or dismissible warning.

Opaque Redaction continues to expose no properties capable of weakening its
security semantics.

## 12. History, Dirty State, And Errors

One successful create, move, resize, block-size edit, or delete creates at
most one history entry. Pointer movements, slider preview values, cache
results, selection, and active-tool changes create none.

Undo and redo restore Pixelate identity, bounds, block size, graph order, and
document state ID. A successful Pixelate edit participates in existing dirty
state and Navigator refresh behavior. A failed or cancelled edit changes
neither.

Document validation failures use a Pixelate-specific invalid-bounds or
invalid-block-size error and the existing non-blocking inline error path.
Preview-cache failure does not alter the document. It leaves the temporary
outline visible, retries after a relevant key change, and reports at most one
non-blocking warning per workspace so repeated rendering does not spam the
user.

An obsolete async result is expected cancellation-by-supersession: it is
discarded and traced at `trace` level, not reported as a user-visible error.

## 13. Security And Compatibility Boundaries

Pixelate must never satisfy secure-redaction checks. In particular:

- `has_secure_redactions` continues to match only Opaque Redaction.
- OCR redaction masks continue to include only Opaque Redaction.
- Safe Copy/Save labels and issue-pack safety gates do not change because a
  Pixelate annotation exists.
- Copy Original remains the immutable source.
- Product copy never calls Pixelate secure, safe, removed, or redacted.

Every exhaustive `Annotation` consumer must be inspected. Existing app,
automation proposal lowering, workbench, Timeline, Action Guide, and tests
must either lower Pixelate through the shared document contract where they
already consume generic annotations or handle it as an unsupported product
surface without adding speculative Pixelate UI/API.

Capture Overlay, Linux/macOS overlay runners, capture backends, and stitching
are unchanged. Linux and macOS share the same Result Workspace implementation.

## 14. Automated Verification

### 14.1 Document model and history

- Canonical 16 px construction and explicit valid block-size construction.
- Rejection of 3, 49, non-finite bounds, and zero-area-after-clamp bounds.
- Add, bounds update, block-size update, delete, undo, and redo.
- Stable identity and exact graph-order restoration.
- No-op update without state-ID or history change.
- Atomic batch rollback for invalid Pixelate operations.

### 14.2 Geometry and Navigator

- Rectangular body hit and empty-region miss.
- Eight resize handles and handle precedence over body.
- Image-bounded body movement.
- Normal and inverted resize.
- Shift square creation and aspect-preserving resize.
- Pixelate bounds for viewport culling.
- Navigator label `Pixelate`, top-left anchor, stable-ID tie breaking, and
  undo/redo refresh.

### 14.3 Mosaic kernel and paint order

- Deterministic average from a known-color block.
- Premultiplied-alpha averaging.
- Region-local grid anchoring away from image origin.
- Right and bottom partial cells.
- Source-edge clipping.
- Region smaller than block size.
- Move, resize, and block-size resampling from the new immutable-source
  region.
- Overlapping Pixelate annotations do not sample each other.
- Earlier annotation coverage and later annotation visibility.
- Source bytes remain unchanged after repeated flattening.

### 14.4 App interaction and properties

- Wide/compact visibility, narrow More routing, More active state, label,
  tooltip, and `B` shortcut.
- Captured input and Alt-modified key handling.
- Reverse drag, Shift constraint, minimum gesture, Escape cancellation, and
  persistent active tool.
- Select movement, resize, deletion, undo, redo, and image-bound clamping.
- 16 px default, missing/invalid config handling, persistence round-trip, and
  failure warning.
- Tool-default block-size transaction without history.
- Selected-object preview, release commit as one history entry, cancellation,
  and isolation from the tool default.
- `Not secure` property copy for tool and selected annotation.

### 14.5 Preview cache and output

- Exact-key hit and miss behavior.
- Invalidation for geometry, block size, undo/redo, source, and display scale.
- Scroll visibility requests without unnecessary key invalidation.
- One in-flight request per key and stale-generation rejection.
- LRU recency and 64 MiB accounting, including one-current-oversized-entry
  behavior.
- Failure outline and one-warning behavior.
- Copy and Save use committed full-resolution Pixelate without waiting for or
  reading preview cache.
- Copy Original stays byte-identical.
- Draft, selection, handles, outline fallback, and property preview never
  enter flattened output.
- Live-preview block boundaries and partial edges agree with flattening at
  the displayed scale.

### 14.6 Compatibility and scale

- Existing Number, Text, Opaque Redaction, TwoPoint, Shape, and Freehand
  suites remain unchanged and pass.
- Opaque Redaction remains the only secure annotation and continues to
  replace pixels completely opaquely.
- Automation, workbench, Timeline, Action Guide, Copy, Save, Navigator, and
  dirty-state contract suites pass.
- The long-image suite contains at least 100 mixed annotations and includes
  all eight committed annotation kinds.

Workspace verification runs:

```bash
rtk cargo test
rtk cargo fmt --all --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk git diff --check
```

This slice does not touch `rollshot-core` stitching paths and does not run the
stitching benchmark workflow.

## 15. Performance And Diagnostics

Preview generation and full-resolution Pixelate flattening record structured
events with stable `rollshot::annotation` targets. Fields include source
identity, raster width/height, block size, cache outcome, generated bytes,
and elapsed microseconds or milliseconds as appropriate. Per-frame and cache
lookup detail uses `trace`; completed generation and explicit output timing
uses `debug`.

The implementation records fresh timing for:

- A 1920×1080 Pixelate region at block size 16 on cache miss and hit.
- Move, resize, and block-size invalidation on a long screenshot.
- Full-resolution flatten of the 100-mixed-annotation long-image fixture.

Cache hits must not rerun the mosaic kernel. Cache accounting must remain
within the approved 64 MiB retained-byte limit except for the explicitly
defined single currently requested oversized entry. Full-resolution flatten
continues to run only for explicit output or tests.

## 16. Linux And macOS Runtime Verification

Both platform Result Workspace paths verify:

1. Pixelate placement after Redact, active state, complete tooltip, `B`
   shortcut, and More routing at Wide, Compact, and Narrow widths.
2. Forward and reverse region creation, repeated creation with persistent
   active tool, Shift-square creation, sub-pixel cancellation, and Escape.
3. Selection by body, empty-region miss, eight handles, whole-region movement,
   normal and Shift-constrained resize, image-edge clamping, deletion, undo,
   and redo.
4. 16 px default and 4–48 slider behavior, restart persistence, selected
   preview and one-step undo, transaction cancellation, and isolation from
   the tool default.
5. Region-local block alignment, partial edge blocks, source resampling after
   movement/resize, and overlapping Pixelate paint order.
6. Responsive cache behavior during typical and long-image gestures; cache
   hit/miss and generation timing appear in tracing; misses and failures show
   only the correct region outline.
7. `Not secure` and tooltip wording remain visibly distinct from Redact;
   Pixelate alone does not trigger safe-output labels or OCR redaction masks.
8. Navigator, Copy, Save As, Copy Original, dirty state, zoom, pan, long-image
   coordinates, native clipboard, and file-dialog handoff.

Capture Overlay is unchanged. Platform risk is confined to the shared Result
Workspace plus existing native clipboard and file-dialog integrations.

## 17. Completion Criteria

Slice 5 is complete only when:

1. Pixelate satisfies its complete create-through-output lifecycle with a
   stable annotation identity and one-entry edit semantics.
2. The region-local average-block kernel, edge clipping, premultiplied-alpha
   behavior, and immutable-source sampling pass automated verification.
3. Movement, eight-direction resize, Shift constraints, block-size property,
   persisted default, selection, deletion, undo/redo, and Navigator pass
   automated and platform runtime verification.
4. The preview cache stays within its defined retention bound, rejects stale
   results, invalidates every approved mutation path, and cannot influence
   full-resolution output.
5. Live preview and deterministic flatten agree on grid, block, clipping, and
   paint-order semantics.
6. Pixelate remains explicitly non-secure while Opaque Redaction remains the
   only secure-redaction annotation and all existing consumers remain
   compatible.
7. All required automated checks and both platform runtime checklists pass,
   the umbrella registry records the transition, and no required Slice 5 work
   remains.

Only then may Slice 6 implementation begin.
