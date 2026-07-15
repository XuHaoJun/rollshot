# Pixelate Effect (Slice 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a complete, explicitly non-secure Pixelate annotation lifecycle to the Result Workspace, including deterministic immutable-source output and a responsive bounded live-preview cache.

**Architecture:** `rollshot-image-document` owns the typed annotation, validated edits, a region-local premultiplied-average mosaic kernel, render-command lowering, history, hit testing, Navigator data, and full-resolution flattening. `rollshot-app` owns creation and transform previews, the persisted 16 px default, the transactional 4–48 px property, async preview generation, a 64 MiB workspace-local LRU of iced image handles, toolbar routing, and safety copy. Live preview and flatten call the same document-crate kernel and always sample the immutable source.

**Tech Stack:** Rust workspace; `image::RgbaImage`; `rollshot-image-document`; `rollshot-app`; iced 0.14 Canvas/Image/Task; Tokio `spawn_blocking`; TOML defaults; `tracing`.

**Authority:** Approved spec [`docs/superpowers/specs/2026-07-15-pixelate-effect-design.md`](../specs/2026-07-15-pixelate-effect-design.md) under umbrella [`2026-07-12-annotation-editor-umbrella-design.md`](../specs/2026-07-12-annotation-editor-umbrella-design.md). On conflict, the approved Slice 5 spec wins over this plan.

## Global Constraints

- Prefix every shell command with `rtk`.
- Product label: `Pixelate`; shortcut: `B`; canonical tooltip: `Pixelate (B) — visual obfuscation only; use Redact to securely remove information.`
- Canonical explanatory copy: `Visual obfuscation only — use Redact to securely remove information.`
- Canonical block-size default: `16` full-resolution image pixels; accepted inclusive range: `4..=48`.
- Pixelate samples region-local average-color blocks from the immutable source; its grid starts at the raster region's top-left and partial edge blocks average only their actual pixels.
- Average transparent input in premultiplied RGBA, then convert the result back to straight RGBA.
- Pixelate is never represented as `OpaqueRedaction` and never satisfies a secure-redaction, safe-output, or OCR-mask check.
- Shift constrains creation to a square and selected resize to the original aspect ratio; Alt-from-center remains out of scope.
- Pixelate stays active after creation and does not select the new annotation.
- Live preview cache retention limit: `64 * 1024 * 1024` uncompressed bytes, with at most one currently requested oversized entry retained temporarily.
- Preview work runs off the UI update path; one generation per exact key; stale source/key/generation completions are discarded.
- Preview miss, in-flight work, or failure draws only the correct region outline; stale pixels are never reused for a different key.
- Full-resolution Copy/Save flattening never reads, waits for, or writes the preview cache; Copy Original stays byte-identical.
- Runtime diagnostics use `tracing` with stable target `rollshot::annotation` and structured fields; no `println!`, `eprintln!`, or `dbg!` in active product paths.
- No new crate, external dependency, generic effect framework, GPU shader, Timeline/Automation/Action Guide Pixelate UI, capture change, or stitching change.
- Any task editing iced UI must invoke `iced-rs` first; the workspace is pinned to iced 0.14.
- Every task uses RED → GREEN, stages explicit paths only, and makes one conventional commit. Never use `git add -A`.
- Current baseline (2026-07-15): `rtk cargo test -p rollshot-app result_workspace::tests -- --nocapture` exits 0 with 15 passed and 594 filtered out.

---

## File Structure

- Create `crates/rollshot-image-document/src/pixelate.rs`: raster-region conversion, validation constants, premultiplied-average mosaic generation, and destination application.
- Create `crates/rollshot-app/src/result_workspace/pixelate_preview.rs`: exact cache keys, generation payloads, in-flight generations, byte-accounted LRU, and pure cache tests.
- Modify `crates/rollshot-image-document/src/{annotation,document,edit_op,flatten,hit,lib,navigator,shapes}.rs`: committed model, edit/history surface, shared render commands, output, hit testing, and navigation.
- Modify `crates/rollshot-app/src/result_workspace/{annotation_defaults,canvas,mod,properties,toolbar,update,view}.rs`: tool lifecycle, defaults, properties, preview scheduling, iced drawing, keyboard and toolbar behavior.
- Modify `crates/rollshot-app/src/result_workspace/{ocr_text,secure_sharing}.rs`: negative security-boundary tests only; production matching remains Opaque Redaction-only.
- Modify `crates/rollshot-app/src/timeline_workspace/annotation.rs`: exhaustive compatibility only; no Timeline Pixelate authoring surface.
- Modify any compiler-identified exhaustive `Annotation` consumer in `crates/rollshot-app`, `crates/rollshot-automation`, or `crates/rollshot-edit-proposal` only enough to preserve its existing unsupported/generic behavior; declare the exact path in the active task before editing it. Adding `Tool::Pixelate` (Task 4) likewise requires updating every exhaustive `Tool` match in `crates/rollshot-app/src/result_workspace` (canvas, update, view, toolbar, properties, mod); Task 4's file list already covers these, so no extra catch-all is needed for `Tool`.
- Modify `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md` only for Slice 5 registry transitions and evidence; do not rewrite the historical Slice 5 spec or this plan after handoff.

## Execution And Test Flow

```text
immutable Arc<RgbaImage>
  -> ImageDocument Pixelate edit/history
  -> RenderCommand::Pixelate in graph order
       |-> full-resolution kernel -> flatten destination
       `-> exact PreviewKey -> spawn_blocking kernel -> nearest iced image

pointer/property preview (app-only)
  -> request generation N
  -> accept only exact source + key + N
  -> LRU handle or outline fallback
  -> release submits exactly one document edit
```

The plan preserves `annotation_shapes()` as a compatibility wrapper for existing vector-only consumers. New mixed vector/raster consumers use `annotation_commands()`. This avoids a speculative renderer rewrite while giving Pixelate one canonical command boundary.

---

### Task 1: Pixelate raster kernel and shared immutable source

**Files:**
- Create: `crates/rollshot-image-document/src/pixelate.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`
- Modify: `crates/rollshot-image-document/src/document.rs`
- Modify (preflight only): `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md`

**Interfaces:**
- Produces `pub const DEFAULT_PIXELATE_BLOCK_SIZE: u32 = 16`, `MIN_PIXELATE_BLOCK_SIZE = 4`, and `MAX_PIXELATE_BLOCK_SIZE = 48`.
- Produces `pub struct RasterRegion { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }` with `byte_len() -> usize`.
- Produces `pub struct PixelatedRegion { pub region: RasterRegion, pub pixels: RgbaImage }`.
- Produces `pub enum PixelateError { InvalidBounds, InvalidBlockSize(u32) }`.
- Produces `pub fn raster_region(bounds: ImageRect, source_width: u32, source_height: u32) -> Result<RasterRegion, PixelateError>`.
- Produces `pub fn pixelate_region(source: &RgbaImage, bounds: ImageRect, block_size: u32) -> Result<PixelatedRegion, PixelateError>`.
- Produces `pub(crate) fn apply_pixelate(destination: &mut RgbaImage, region: &PixelatedRegion)`.
- Produces `ImageDocument::shared_source(&self) -> Arc<RgbaImage>` while preserving `source(&self) -> &RgbaImage`.

- [ ] **Step 0: Mark Slice 5 In progress before product edits**

Change only the Slice 5 umbrella row from `Planned` to `In progress`, record branch `feat/annotation-pixelate-effect`, and state that automated and both platform runtime verification are pending.

Run: `rtk git diff --check`
Expected: exit 0; only the umbrella registry row differs.

```bash
rtk git add docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md
rtk git commit -m "docs(annotation): mark Slice 5 implementation in progress"
```

- [ ] **Step 1: Write RED kernel tests**

Register `mod pixelate;` and the public exports in `lib.rs`. Create `pixelate.rs` with the public types/signatures above and bodies that `panic!("Pixelate kernel not implemented")`, then add tests with these exact assertions:

```rust
#[test]
fn grid_is_region_local_and_partial_cells_use_actual_pixels() {
    let source = numbered_10_by_7_opaque_image();
    let result = pixelate_region(
        &source,
        ImageRect::new(1.0, 1.0, 8.0, 6.0),
        4,
    )
    .unwrap();
    assert_eq!(result.region, RasterRegion { x: 1, y: 1, width: 8, height: 6 });
    assert_block_equals_average(&source, &result, (1, 1, 4, 4));
    assert_block_equals_average(&source, &result, (5, 1, 4, 4));
    assert_block_equals_average(&source, &result, (1, 5, 4, 2));
    assert_block_equals_average(&source, &result, (5, 5, 4, 2));
}

#[test]
fn transparent_colors_are_averaged_in_premultiplied_space() {
    let source = RgbaImage::from_raw(
        2,
        1,
        vec![255, 0, 0, 255, 0, 0, 255, 0],
    )
    .unwrap();
    let result = pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 1.0), 4).unwrap();
    assert_eq!(result.pixels.get_pixel(0, 0).0, [255, 0, 0, 128]);
    assert_eq!(result.pixels.get_pixel(1, 0).0, [255, 0, 0, 128]);
}

#[test]
fn partial_alpha_is_averaged_in_premultiplied_space() {
    // Opaque red [255,0,0,255] + semi-transparent blue [0,0,255,128].
    // Premultiplied average then un-premultiply yields [170,0,85,192]:
    // alpha_sum=383, premul=[65025,0,32640], out_alpha=192,
    // out_r=round(65025/383)=170, out_b=round(32640/383)=85.
    let source = RgbaImage::from_raw(
        2,
        1,
        vec![255, 0, 0, 255, 0, 0, 255, 128],
    )
    .unwrap();
    let result = pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 1.0), 4).unwrap();
    assert_eq!(result.pixels.get_pixel(0, 0).0, [170, 0, 85, 192]);
    assert_eq!(result.pixels.get_pixel(1, 0).0, [170, 0, 85, 192]);
}

#[test]
fn validation_rejects_invalid_strength_and_empty_clamped_region() {
    let source = RgbaImage::new(8, 8);
    assert_eq!(pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 2.0), 3), Err(PixelateError::InvalidBlockSize(3)));
    assert_eq!(pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 2.0), 49), Err(PixelateError::InvalidBlockSize(49)));
    assert_eq!(pixelate_region(&source, ImageRect::new(20.0, 20.0, 2.0, 2.0), 16), Err(PixelateError::InvalidBounds));
    assert_eq!(pixelate_region(&source, ImageRect::new(f32::NAN, 0.0, 2.0, 2.0), 16), Err(PixelateError::InvalidBounds));
}
```

Run: `rtk cargo test -p rollshot-image-document pixelate::tests -- --nocapture`
Expected: FAIL because the kernel body panics.

- [ ] **Step 2: Implement the integer raster region and mosaic**

Use the existing crisp replacement-effect rounding convention: normalize, clamp, round each edge, clamp integer edges to source dimensions, reject `x1 <= x0 || y1 <= y0`. For each local block, sum `channel * alpha` in `u64`, sum alpha, round output alpha by sample count, unpremultiply RGB by summed alpha, and fill the clipped cell. The implementation must read only `source` and write only the new region image.

```rust
pub fn pixelate_region(
    source: &RgbaImage,
    bounds: ImageRect,
    block_size: u32,
) -> Result<PixelatedRegion, PixelateError> {
    if !(MIN_PIXELATE_BLOCK_SIZE..=MAX_PIXELATE_BLOCK_SIZE).contains(&block_size) {
        return Err(PixelateError::InvalidBlockSize(block_size));
    }
    let region = raster_region(bounds, source.width(), source.height())?;
    let mut pixels = RgbaImage::new(region.width, region.height);
    for local_y in (0..region.height).step_by(block_size as usize) {
        for local_x in (0..region.width).step_by(block_size as usize) {
            let cell_w = block_size.min(region.width - local_x);
            let cell_h = block_size.min(region.height - local_y);
            let sample_count = u64::from(cell_w) * u64::from(cell_h);
            let mut alpha_sum = 0_u64;
            let mut premul = [0_u64; 3];
            for y in 0..cell_h {
                for x in 0..cell_w {
                    let p = source.get_pixel(region.x + local_x + x, region.y + local_y + y).0;
                    let a = u64::from(p[3]);
                    alpha_sum += a;
                    for channel in 0..3 {
                        premul[channel] += u64::from(p[channel]) * a;
                    }
                }
            }
            let out_alpha = ((alpha_sum + sample_count / 2) / sample_count) as u8;
            let mut out = [0_u8; 4];
            out[3] = out_alpha;
            if alpha_sum != 0 {
                for channel in 0..3 {
                    out[channel] = ((premul[channel] + alpha_sum / 2) / alpha_sum) as u8;
                }
            }
            for y in 0..cell_h {
                for x in 0..cell_w {
                    pixels.put_pixel(local_x + x, local_y + y, image::Rgba(out));
                }
            }
        }
    }
    Ok(PixelatedRegion { region, pixels })
}
```

Change `ImageDocument.source` to `Arc<RgbaImage>`, wrap constructor input with `Arc::new`, keep `source()` returning `self.source.as_ref()`, and add `shared_source()` using `Arc::clone`.

Run: `rtk cargo test -p rollshot-image-document pixelate::tests -- --nocapture`
Expected: exit 0; all kernel tests pass.

- [ ] **Step 3: Verify source sharing and commit**

Add a document test that `Arc::ptr_eq(&document.shared_source(), &document.shared_source())` is true and that mutating a flattened copy leaves `document.source().as_raw()` unchanged.

Run: `rtk cargo test -p rollshot-image-document`
Expected: exit 0.

```bash
rtk git add docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md crates/rollshot-image-document/src/pixelate.rs crates/rollshot-image-document/src/lib.rs crates/rollshot-image-document/src/document.rs
rtk git commit -m "feat(annotation): add immutable-source pixelate kernel"
```

---

### Task 2: Pixelate annotation, validated edits, history, hit testing, and Navigator

**Files:**
- Modify: `crates/rollshot-image-document/src/annotation.rs`
- Modify: `crates/rollshot-image-document/src/edit_op.rs`
- Modify: `crates/rollshot-image-document/src/document.rs`
- Modify: `crates/rollshot-image-document/src/hit.rs`
- Modify: `crates/rollshot-image-document/src/navigator.rs`
- Modify: `crates/rollshot-image-document/src/shapes.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`

**Interfaces:**
- Produces `Annotation::Pixelate { id, bounds, block_size }` and `Annotation::pixelate(id, bounds, block_size)`.
- Produces `EditOp::{AddPixelate, UpdatePixelateBounds, UpdatePixelateBlockSize}`.
- Produces `ImageDocument::{add_pixelate, set_pixelate_bounds, set_pixelate_block_size}` returning the existing edit result types.
- Produces `EditError::{InvalidPixelateBounds, InvalidPixelateBlockSize(u32)}` and uses existing `WrongKind` for cross-kind updates.
- Preserves `annotation_shapes(&Pixelate) == Vec::new()` until Task 3 introduces raster commands.

- [ ] **Step 1: Write RED model/history tests**

Add tests covering default/explicit construction, stable identity, add/update/delete/undo/redo, no-op state ID, invalid 3/49/non-finite/empty bounds, wrong kind, and atomic batch rollback. Use public operations, not private mutation.

```rust
#[test]
fn pixelate_edits_are_validated_and_undo_as_single_entries() {
    let mut document = document_32_by_32();
    let id = document
        .add_pixelate(ImageRect::new(2.0, 3.0, 12.0, 10.0), 16)
        .unwrap();
    document.set_pixelate_bounds(id, ImageRect::new(4.0, 5.0, 8.0, 7.0)).unwrap();
    document.set_pixelate_block_size(id, 24).unwrap();
    assert!(matches!(document.annotation(id), Some(Annotation::Pixelate { bounds, block_size: 24, .. }) if *bounds == ImageRect::new(4.0, 5.0, 8.0, 7.0)));
    assert!(document.undo());
    assert!(matches!(document.annotation(id), Some(Annotation::Pixelate { block_size: 16, .. })));
    assert!(document.redo());
    assert!(matches!(document.annotation(id), Some(Annotation::Pixelate { block_size: 24, .. })));
}

#[test]
fn invalid_pixelate_batch_is_atomic() {
    let mut document = document_32_by_32();
    let before = document.state_id();
    let result = document.apply_batch(vec![
        EditOp::AddPixelate { bounds: ImageRect::new(1.0, 1.0, 5.0, 5.0), block_size: 16 },
        EditOp::AddPixelate { bounds: ImageRect::new(2.0, 2.0, 5.0, 5.0), block_size: 49 },
    ]);
    assert_eq!(result, Err(EditError::InvalidPixelateBlockSize(49)));
    assert!(document.annotations().is_empty());
    assert_eq!(document.state_id(), before);
    assert!(!document.can_undo());
}
```

Run: `rtk cargo test -p rollshot-image-document pixelate -- --nocapture`
Expected: compilation FAIL because Pixelate annotation/edit APIs do not exist.

- [ ] **Step 2: Implement the typed annotation and one edit path**

Add the three exact variants. Route convenience methods through `apply(EditOp::...)`. In `apply_unrecorded`, validate block size with Task 1 constants, normalize/clamp bounds with the existing document helper, map a rejected clamped region to `InvalidPixelateBounds`, allocate IDs only after all validation succeeds, and return early for exact no-op updates. Extend `id()`, `anchor()`, annotation bounds, graph-order/history snapshot logic, and every compiler-reported exhaustive match.

```rust
Pixelate {
    id: AnnotationId,
    bounds: ImageRect,
    block_size: u32,
}

AddPixelate { bounds: ImageRect, block_size: u32 },
UpdatePixelateBounds { id: AnnotationId, bounds: ImageRect },
UpdatePixelateBlockSize { id: AnnotationId, block_size: u32 },
```

Do not add color, opacity, style, effect-kind, or cached pixels to the variant.

- [ ] **Step 3: Add RED hit/Navigator tests, then implement rectangle semantics**

Tests must assert body hit, outside miss, all eight resize handles with handle precedence, `annotation_bounds == bounds`, Navigator label `Pixelate`, top-left anchor, stable-ID tie breaking, and correct undo/redo refresh.

Implement Pixelate through the same rectangle helpers as Opaque Redaction/Shape. In `annotation_shapes`, return an empty vector for Pixelate; Task 3 is the only raster lowering path.

Run: `rtk cargo test -p rollshot-image-document`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-image-document/src/annotation.rs crates/rollshot-image-document/src/edit_op.rs crates/rollshot-image-document/src/document.rs crates/rollshot-image-document/src/hit.rs crates/rollshot-image-document/src/navigator.rs crates/rollshot-image-document/src/shapes.rs crates/rollshot-image-document/src/lib.rs
rtk git commit -m "feat(annotation): add pixelate document lifecycle"
```

---

### Task 3: Mixed render commands and deterministic flatten paint order

**Files:**
- Modify: `crates/rollshot-image-document/src/shapes.rs`
- Modify: `crates/rollshot-image-document/src/flatten.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`

**Interfaces:**
- Produces `pub enum RenderCommand { Shape(RenderShape), Pixelate { bounds: ImageRect, block_size: u32 } }`.
- Produces `pub fn annotation_commands(annotation: &Annotation) -> Vec<RenderCommand>`.
- Preserves `annotation_shapes()` for vector-only compatibility by filtering `RenderCommand::Shape`.
- Flatten consumes commands in graph order and calls Task 1 `pixelate_region(source, bounds, block_size)` then `apply_pixelate(destination, &region)`.

- [ ] **Step 1: Write RED lowering and paint-order tests**

```rust
#[test]
fn pixelate_lowers_to_one_raster_command() {
    let annotation = Annotation::pixelate(AnnotationId::new(7), ImageRect::new(3.0, 4.0, 8.0, 9.0), 16);
    assert_eq!(annotation_commands(&annotation), vec![RenderCommand::Pixelate { bounds: ImageRect::new(3.0, 4.0, 8.0, 9.0), block_size: 16 }]);
}

#[test]
fn pixelate_covers_earlier_annotations_but_later_annotations_cover_pixelate() {
    let source = four_quadrant_fixture();
    let earlier = red_rectangle_over_center();
    let pixelate = pixelate_center(4);
    let later = blue_arrow_over_center();
    let only_earlier = flatten_onto(&source, &[earlier.clone(), pixelate.clone()]);
    let with_later = flatten_onto(&source, &[earlier, pixelate, later]);
    assert_eq!(only_earlier.get_pixel(4, 4), pixelated_source_center(&source).get_pixel(4, 4));
    assert_eq!(with_later.get_pixel(4, 4).0, [0, 0, 255, 255]);
}

#[test]
fn overlapping_pixelates_each_sample_original_source() {
    let source = asymmetric_source_fixture();
    let twice = flatten_onto(&source, &[pixelate_rect(0.0, 0.0, 8.0, 8.0, 4), pixelate_rect(2.0, 2.0, 6.0, 6.0, 4)]);
    let second_only = flatten_onto(&source, &[pixelate_rect(2.0, 2.0, 6.0, 6.0, 4)]);
    assert_eq!(crop(&twice, 2, 2, 6, 6), crop(&second_only, 2, 2, 6, 6));
    assert_eq!(source, asymmetric_source_fixture());
}
```

Run: `rtk cargo test -p rollshot-image-document flatten::tests -- --nocapture`
Expected: compilation FAIL because `RenderCommand`/`annotation_commands` do not exist.

- [ ] **Step 2: Implement command lowering and flatten dispatch**

Lower every existing annotation to the exact same `RenderShape` values it currently produces, wrapped in `RenderCommand::Shape`. Lower Pixelate to one raster command. Iterate annotations then commands in order; shapes use the existing raster path, Pixelate regenerates from `source` and replaces only its region in `destination`.

Do not make Timeline generate Pixelate annotations. Extend its display-only command/shape matches so all existing Timeline tests compile and render unchanged.

Run: `rtk cargo test -p rollshot-image-document`
Expected: exit 0.

Run: `rtk cargo test -p rollshot-app timeline_workspace::annotation::tests -- --nocapture`
Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-image-document/src/shapes.rs crates/rollshot-image-document/src/flatten.rs crates/rollshot-image-document/src/lib.rs crates/rollshot-app/src/timeline_workspace/annotation.rs
rtk git commit -m "feat(annotation): flatten pixelate render commands"
```

---

### Task 4: Tool, toolbar, persisted default, and property transactions

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/annotation_defaults.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs`
- Modify: `crates/rollshot-app/src/result_workspace/toolbar.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`

**Interfaces:**
- Produces `Tool::Pixelate`, `PropertyTarget::PixelateTool`, and selected `Annotation::Pixelate` property targeting.
- Produces `AnnotationDefaults.pixelate_block_size: u32` and `AnnotationDefaultsState.pixelate_block_size: u32`.
- Produces `PropertyState.pixelate_block_size: Option<u32>` and app-only `BlockSizeTransaction { target, original, preview }`.
- Produces messages `PreviewPixelateBlockSize(u32)`, `ApplyPixelateBlockSize`, and `CancelPixelateBlockSize`.

- [ ] **Step 1: Write RED defaults tests**

Add TOML tests for missing field → 16 without warning, 4/48 round-trip, string/3/49 → 16 with one existing load warning, and persistence failure retaining the in-memory value while emitting one warning.

Represent the on-disk field as `pixelate_block_size = 16`; deserialize through an `Option<toml::Value>` or existing tolerant helper so malformed types do not make the entire defaults file unreadable.

Run: `rtk cargo test -p rollshot-app annotation_defaults::tests -- --nocapture`
Expected: FAIL because the field does not exist.

- [ ] **Step 2: Implement defaults and RED toolbar/shortcut tests**

Initialize missing/invalid data with `DEFAULT_PIXELATE_BLOCK_SIZE`, validate against `MIN_PIXELATE_BLOCK_SIZE..=MAX_PIXELATE_BLOCK_SIZE`, and save the integer with existing fields.

Add tests asserting (`wide_tools`, `compact_tools`, `narrow_visible_tools`, `narrow_more_tools`, and `tool_for_shortcut` are test helpers created in this step; the shape slot reflects the default remembered shape `Tool::Rectangle` from `AnnotationDefaults::default().last_shape` — the current toolbar has no `Tool::Shapes` variant, only `Tool::Rectangle`/`Tool::Ellipse` behind a shapes menu):

```rust
assert_eq!(wide_tools(), vec![Select, Number, Text, Line, Arrow, Rectangle, Pen, Highlighter, Redact, Pixelate]);
assert_eq!(compact_tools(), vec![Select, Number, Text, Line, Arrow, Rectangle, Pen, Highlighter, Redact, Pixelate]);
assert!(!narrow_visible_tools().contains(&Pixelate));
assert!(narrow_more_tools().contains(&Pixelate));
assert_eq!(tool_for_shortcut(Key::Character("b".into()), Modifiers::default(), false), Some(Tool::Pixelate));
assert_eq!(tool_for_shortcut(Key::Character("b".into()), Modifiers::ALT, false), None);
assert_eq!(tool_for_shortcut(Key::Character("b".into()), Modifiers::default(), true), None);
```

Implement the exact order, More active treatment/name, label `Pixelate`, and canonical tooltip from Global Constraints. Add `B` to captured-key suppression.

- [ ] **Step 3: Write RED property transaction tests, then implement**

Tests must demonstrate that Pixelate tool targeting previews then persists the default without document history; selected targeting previews 24 then commits one `UpdatePixelateBlockSize`; selected editing does not change the tool default; Escape/Undo/Redo/tool change/selection change cancels the transaction; values clamp to 4..=48; both target views contain `Not secure` and current text such as `16 px`.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockSizeTarget {
    ToolDefault,
    Annotation(AnnotationId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockSizeTransaction {
    pub(crate) target: BlockSizeTarget,
    pub(crate) original: u32,
    pub(crate) preview: u32,
}
```

Slider movement mutates only this transaction. Apply writes either the in-memory default plus persistence request or one document edit. Cancellation clears it. Do not change Opaque Redaction properties.

Run: `rtk cargo test -p rollshot-app result_workspace::properties::tests -- --nocapture`
Expected: exit 0.

Run: `rtk cargo test -p rollshot-app result_workspace::toolbar::tests -- --nocapture`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/annotation_defaults.rs crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/properties.rs crates/rollshot-app/src/result_workspace/toolbar.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/view.rs
rtk git commit -m "feat(annotation): expose pixelate tool and strength"
```

---

### Task 5: Pixelate creation, selection, move, and resize lifecycle

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**
- Adds `DragState::CreatePixelate { anchor, current, block_size }`.
- Reuses `box_tool::{creation_bounds, meets_creation_threshold, moved_bounds, resized_bounds}`.
- Extends `dragged_annotation()` so Pixelate body/handle drags return a transient Pixelate with unchanged ID/block size.
- Release routes through Task 2 `add_pixelate` or `set_pixelate_bounds` exactly once.

- [ ] **Step 1: Write RED creation tests**

Add update tests for forward/reverse drag, Shift square, clamping, click/sub-pixel cancellation, Escape cancellation, persistent active tool, no auto-selection, and creation over an existing annotation without editing/selecting it.

```rust
#[test]
fn pixelate_release_commits_once_and_keeps_tool_active() {
    let mut state = workspace_100_by_100();
    state.active_tool = Tool::Pixelate;
    press(&mut state, image_point(10.0, 12.0));
    move_pointer(&mut state, image_point(40.0, 32.0), Modifiers::default());
    release(&mut state, image_point(40.0, 32.0));
    assert_eq!(state.active_tool, Tool::Pixelate);
    assert_eq!(state.selected_annotation, None);
    assert_eq!(state.document.annotations().len(), 1);
    assert!(matches!(state.document.annotations()[0], Annotation::Pixelate { bounds, block_size: 16, .. } if bounds == ImageRect::new(10.0, 12.0, 30.0, 20.0)));
    assert!(state.document.undo());
    assert!(state.document.annotations().is_empty());
}
```

Run: `rtk cargo test -p rollshot-app pixelate_creation -- --nocapture`
Expected: FAIL because press/move/release do not handle `Tool::Pixelate`.

- [ ] **Step 2: Implement app-only draft and one release edit**

At press, capture the current default. At move, compute normalized bounded geometry using existing box helpers and current Shift state. At release, reject less-than-one-raster-pixel coverage, otherwise call `add_pixelate` once. Escape drops the drag state. No pointer move mutates the document.

- [ ] **Step 3: Write RED direct-manipulation tests, then implement**

Tests cover rectangle body hit, empty miss, eight zoom-independent handles, handle precedence, body movement preserving size/block size, normal and inverted resize, Shift original-aspect resize, image clamping, Escape restoration, delete/backspace, and one undo entry per released move/resize.

Extend the existing Opaque Redaction/Shape rectangle match arms with Pixelate data preservation. Do not scale `block_size` during resize.

Run: `rtk cargo test -p rollshot-app result_workspace::canvas::tests -- --nocapture`
Expected: exit 0.

Run: `rtk cargo test -p rollshot-app result_workspace::update::tests -- --nocapture`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(annotation): add pixelate gesture lifecycle"
```

---

### Task 6: Pure preview-key and byte-bounded LRU cache

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/pixelate_preview.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`

**Interfaces:**
- Produces `PreviewKey { source_id: usize, region: RasterRegion, block_size: u32, display_scale_bits: u32 }`.
- Produces `PreviewRequest { key: PreviewKey, generation: u64 }` and `PreviewPixels { request, width, height, rgba }`.
- Produces `PreviewGenerationError::{Kernel(PixelateError), WorkerFailed}` for worker-safe completion messages.
- Produces `PixelatePreviewCache::{new, lookup, begin_request, complete, fail, retain_requested, clear_for_source, retained_bytes, is_in_flight}`.
- Cache stores iced `image::Handle` only after a completion is accepted; pure tests inspect metadata/bytes without rendering.

- [ ] **Step 1: Write RED key/in-flight/stale tests**

Tests must assert exact hit/miss, source/region/block/display-scale key inequality, only one in-flight request per key, generation increment after invalidation, rejection of old generation, source replacement clearing all entries, and scroll-only reuse of the same key.

```rust
#[test]
fn completion_requires_exact_key_and_generation() {
    let mut cache = PixelatePreviewCache::new(1024);
    let key = key(1, region(0, 0, 8, 8), 16, 1.0);
    let request = cache.begin_request(key).unwrap();
    cache.invalidate_key(key);
    assert_eq!(cache.complete(pixels_for(request, 8, 8)), Completion::Stale);
    assert!(cache.lookup(key).is_none());
}

#[test]
fn one_key_has_at_most_one_in_flight_generation() {
    let mut cache = PixelatePreviewCache::new(1024);
    let key = key(1, region(0, 0, 8, 8), 16, 1.0);
    assert!(cache.begin_request(key).is_some());
    assert!(cache.begin_request(key).is_none());
}
```

Run: `rtk cargo test -p rollshot-app pixelate_preview::tests -- --nocapture`
Expected: compilation FAIL because the module does not exist.

- [ ] **Step 2: Implement keys, generations, and accepted completion**

Derive `Eq`/`Hash` for integer fields and store display scale as `f32::to_bits()` after validating it is finite and positive. Source identity is `Arc::as_ptr(&source) as usize`. `begin_request` records the key in a `HashMap<PreviewKey, u64>` and returns `None` if already in flight. `complete` removes only a matching generation; mismatches trace `cache_outcome = "stale"` and do not mutate entries.

- [ ] **Step 3: Write RED LRU/oversized/failure tests, then implement**

Use a small injected byte limit in tests. Assert lookup refreshes recency, insert evicts least-recently-used entries, retained bytes never exceed the limit, a currently requested oversized entry may temporarily exceed it, `retain_requested(empty)` evicts that oversized entry, failure removes in-flight state, and repeated failures return `warn_user = true` only once per cache/workspace.

Account bytes as `width * height * 4` with checked multiplication. Store an increasing access clock; on pressure evict the lowest `last_used` entry not present in `requested`, then apply the single-requested-oversized exception exactly.

Run: `rtk cargo test -p rollshot-app pixelate_preview::tests -- --nocapture`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/pixelate_preview.rs crates/rollshot-app/src/result_workspace/mod.rs
rtk git commit -m "feat(annotation): add bounded pixelate preview cache"
```

---

### Task 7: Async preview scheduling and iced Canvas rendering

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/pixelate_preview.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`

**Interfaces:**
- Produces synchronous worker `generate_preview(source: Arc<RgbaImage>, request: PreviewRequest) -> Result<PreviewPixels, PreviewGenerationError>`.
- Adds `Message::PixelatePreviewReady(PreviewRequest, Result<PreviewPixels, PreviewGenerationError>)`.
- Adds `ResultWorkspace.pixelate_previews: PixelatePreviewCache`.
- Canvas consumes exact handles through `canvas::Image::new(handle).filter_method(FilterMethod::Nearest).snap(true)` in annotation graph order.

- [ ] **Step 1: Write RED scheduler tests**

Tests must cover visible committed annotations plus active draft/direct-manipulation/property preview requests, newly visible annotations after scrolling, no new key for scroll-only repositioning, geometry/block-size/undo/redo/source/display-scale invalidation, no duplicate task for in-flight key, and stale completion rejection.

Define one pure collector:

```rust
pub(crate) fn requested_pixelate_keys(
    document: &ImageDocument,
    transient_annotations: &[Annotation],
    visible_image_bounds: ImageRect,
    display_scale: f32,
) -> Vec<PreviewKey>
```

It preserves graph order, filters by intersection with `visible_image_bounds`, appends the current transient replacement/draft once, and deduplicates exact keys.

Run: `rtk cargo test -p rollshot-app pixelate_preview_scheduling -- --nocapture`
Expected: FAIL because scheduling is not wired.

- [ ] **Step 2: Implement worker tasks and completion acceptance**

For every missing requested key, call `begin_request`, clone `document.shared_source()`, and return:

```rust
Task::perform(
    async move {
        match tokio::task::spawn_blocking(move || generate_preview(source, request)).await {
            Ok(result) => result,
            Err(_) => Err(PreviewGenerationError::WorkerFailed),
        }
    },
    move |result| Message::PixelatePreviewReady(request, result),
)
```

`generate_preview` calls the Task 1 kernel at full source resolution, then downsizes the generated region only when `display_scale < 1.0`, using `image::imageops::FilterType::Nearest`. It returns RGBA bytes; the update thread constructs the iced handle after exact completion acceptance. Batch generated tasks with the existing update task; do not add a subscription.

Full-resolution-then-downsize is required so the preview block grid matches flatten semantics (spec §8/§9); the cost is a transient `region.width * region.height * 4`-byte allocation off-thread during generation. This transient is NOT covered by the 64 MiB retained-byte cap (which bounds only cached handles); it is bounded by the Pixelate annotation's source-clamped region rather than the viewport-visible intersection, freed when `generate_preview` returns, and isolated on the `spawn_blocking` worker so it never blocks the UI thread. For typical screenshot regions this is a few MB; an extreme full-long-image Pixelate can transiently allocate tens of MB before downsize. Task 9's long-image timing evidence must measure this path, and Slice 5 cannot advance from Handoff if the allocation causes unacceptable interaction latency or memory pressure on either platform.

Trace lookup/request/completion at `trace` with `source_id`, region dimensions, `block_size`, `cache_outcome`, `generated_bytes`, and elapsed microseconds. Trace successful generation at `debug` target `rollshot::annotation`.

- [ ] **Step 3: Write RED Canvas-order/fallback tests, then implement drawing**

Switch Result Workspace committed drawing from `annotation_shapes` to `annotation_commands`. Preserve the borrowed Freehand path. For `RenderCommand::Pixelate`, draw only an exact cached handle at its raster-region rectangle with nearest filtering; otherwise draw the existing selection-colored rectangular outline and no fill. Draw later commands afterward. Draw transient draft/property/direct-manipulation Pixelate with its transient key, then selection handles.

Tests assert vector → Pixelate → vector order, exact-key-only lookup, miss/in-flight/failure outline, no stale translated/scaled handle, live partial edges matching flattened output at scale 1 and 0.5, and one warning after repeated failures.

Run: `rtk cargo test -p rollshot-app result_workspace::canvas::tests -- --nocapture`
Expected: exit 0.

Run: `rtk cargo test -p rollshot-app pixelate_preview -- --nocapture`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/pixelate_preview.rs crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/view.rs
rtk git commit -m "feat(annotation): render cached pixelate previews"
```

---

### Task 8: Output, security, compatibility, scale, and retained diagnostics

**Files:**
- Modify: `crates/rollshot-image-document/src/flatten.rs`
- Modify: `crates/rollshot-app/src/result_workspace/secure_sharing.rs`
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Modify only if compilation identifies an exhaustive consumer: exact affected files under `crates/rollshot-app`, `crates/rollshot-automation`, or `crates/rollshot-edit-proposal`

**Interfaces:**
- No new product surface. This task locks negative boundaries and end-to-end output contracts.
- Full flatten logs one structured `debug` event per Pixelate command with region, block size, and elapsed time.

- [ ] **Step 1: Write RED security-boundary tests**

Add a document containing only Pixelate and assert:

```rust
assert!(!has_secure_redactions(&document));
assert_eq!(secure_copy_label(&document), ordinary_copy_label());
assert!(ocr_redaction_masks(document.image_document()).is_empty());
assert_eq!(document.image_document().source().as_raw(), original_bytes.as_slice());
```

Then add Opaque Redaction beside Pixelate and assert secure classification/mask comes only from the Opaque Redaction bounds. Production match logic must remain explicitly `Annotation::OpaqueRedaction`, not a shared “obfuscation” category.

Run: `rtk cargo test -p rollshot-app secure_sharing::tests -- --nocapture`
Expected: FAIL until test helpers/exhaustive consumers handle Pixelate; security assertions must pass without broadening production matching.

- [ ] **Step 2: Add Copy/Save/cache-isolation tests and flatten timing**

Tests assert committed full-resolution Pixelate appears in Copy and Save even with an empty/failed/in-flight preview cache; Copy Original is byte-identical; draft/selection/handles/fallback/property preview never enter output; moving/resizing/block-size editing changes output by immutable-source resampling; repeated flatten leaves source bytes unchanged.

Wrap each Pixelate flatten kernel call with `Instant::now()` and one `tracing::debug!` event:

```rust
tracing::debug!(
    target: "rollshot::annotation",
    operation = "pixelate_flatten",
    raster_width = region.region.width,
    raster_height = region.region.height,
    block_size,
    elapsed_us = started.elapsed().as_micros() as u64,
    "flattened pixelate annotation"
);
```

- [ ] **Step 3: Expand mixed long-image and compatibility tests**

Change the existing 100-annotation long-image fixture distribution without increasing its count so it contains all eight committed kinds: Number, Text, Opaque Redaction, TwoPoint Line/Arrow, Shape Rectangle/Ellipse, Freehand Pen/Highlighter, and Pixelate. Assert flatten finishes, graph order/IDs remain stable, Navigator contains Pixelate, and cache state does not affect it.

Compile every feature/default consumer. Unsupported surfaces must ignore/reject Pixelate through their existing typed boundary and gain no toolbar, proposal, payload, or automation API.

Run: `rtk cargo test -p rollshot-image-document`
Expected: exit 0.

Run: `rtk cargo test -p rollshot-app`
Expected: exit 0.

Run: `rtk cargo check -p rollshot-app --features action-guide`
Expected: exit 0.

- [ ] **Step 4: Commit**

Stage every declared exact path, including only compiler-required exhaustive consumers, then:

```bash
rtk git commit -m "test(annotation): lock pixelate output boundaries"
```

---

### Task 9: Full verification, performance evidence, runtime handoff, and registry

**Files:**
- Modify: `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md`
- Do not modify the approved Slice 5 spec or this implementation plan.

**Interfaces:**
- Produces verification evidence and the correct umbrella lifecycle state.
- Does not mark Slice 5 Complete until both Linux and macOS runtime checklists and user acceptance are recorded.

- [ ] **Step 1: Run format and targeted regression suites**

```bash
rtk cargo fmt --all --check
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-app result_workspace::tests -- --nocapture
rtk cargo test -p rollshot-app timeline_workspace::annotation::tests -- --nocapture
```

Expected: every command exits 0; the Result Workspace filter reports at least the 15 baseline tests plus new Pixelate tests.

- [ ] **Step 2: Run workspace verification**

```bash
rtk cargo test
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk git diff --check
```

Expected: every command exits 0 with no warnings. No stitching benchmark is run because no `rollshot-core` stitching path changed.

- [ ] **Step 3: Record fresh performance evidence**

Run the ignored Pixelate timing harness introduced alongside Task 8 tests:

```bash
rtk cargo test -p rollshot-app pixelate_timing_evidence -- --ignored --nocapture
```

Expected: exit 0 and structured output/test capture for (a) 1920×1080 block-16 miss, (b) exact-key hit without a kernel invocation, (c) move/resize/block invalidation on a long screenshot, and (d) full-resolution flatten of the 100-annotation fixture. Assert retained bytes are `<= 67_108_864` except while the test's single currently requested oversized entry is active; assert hit count does not increase kernel invocation count.

- [ ] **Step 4: Perform Linux and macOS runtime checklist**

On each platform verify all eight spec §16 groups: toolbar density/tooltip/shortcut; forward/reverse/repeated/Shift/cancel creation; body selection/eight handles/move/resize/clamp/delete/history; 16 and 4–48 persistence/transactions; local grid/partial edge/resampling/overlap order; responsive cache hit/miss/fallback tracing; non-secure copy versus Redact/OCR behavior; Navigator/Copy/Save/Copy Original/dirty/zoom/pan/long-image/native clipboard/dialog.

Expected: every item passes on Linux and macOS. If either platform cannot be exercised, keep Slice 5 at `Handoff`, name the unchecked platform and risk, and do not claim completion.

- [ ] **Step 5: Update the umbrella registry with evidence**

Set Slice 5 to:

- `Complete` only if automated checks, performance evidence, both platform checklists, and user acceptance all pass; or
- `Handoff` if implementation/automated verification pass but runtime or user acceptance remains.

Record branch, implementation commit range, exact verification commands, performance evidence, runtime status, and remaining risk. Leave Slice 6 `Not started`.

Run: `rtk git diff --check`
Expected: exit 0; only the registry evidence changes.

```bash
rtk git add docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md
rtk git commit -m "docs(annotation): record Slice 5 implementation handoff"
```

---

## Final Acceptance Matrix

| Requirement | Primary task | Required evidence |
|---|---:|---|
| Region-local premultiplied average kernel, clipping, partial cells | 1 | Kernel unit tests |
| Typed annotation, validation, atomic history, identity | 2 | Document lifecycle tests |
| Immutable-source paint order and overlapping effects | 3 | Flatten command tests |
| Label, `B`, wide/compact/narrow routing, 16 and 4–48 property | 4 | Defaults/toolbar/property tests |
| Create, Shift, selection, move, eight handles, resize, delete | 5 | Canvas/update lifecycle tests |
| Exact key, one in-flight, stale rejection, 64 MiB LRU | 6 | Pure cache tests |
| Off-thread generation, visibility scheduling, outline fallback | 7 | Scheduler/Canvas tests |
| Pixelate remains non-secure; Copy/Save independent of cache | 8 | Security/output/compatibility tests |
| Workspace quality gates and both platform paths | 9 | Commands, timing evidence, runtime checklist |

Slice 5 is not complete merely because the code compiles. Completion requires every row above, both platform runtime passes, the umbrella transition, and no remaining required Slice 5 work.
