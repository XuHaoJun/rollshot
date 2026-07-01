# OCR Selectable Text Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first-version OCR Text tool with redaction-safe, selectable OCR text in the result workspace.

**Architecture:** Preserve OCR quadrilateral geometry through `rollshot-ocr` -> `rollshot-automation` -> `rollshot-vision`, then add a result-workspace `ocr_text` model that normalizes reading order, masks redacted text, owns selection state, and prepares tiled product OCR. The UI adds a compile-time-gated `Tool::OcrText` and a custom iced `advanced::Widget` (`OcrTextLayer`) stacked over the image/annotation canvas inside the existing scrollable image surface.

**Tech Stack:** Rust, iced 0.14 with `advanced`, `rollshot-ocr` RapidOCR/ONNX, `rollshot-vision`, `rollshot-automation`, `rollshot-image-document`, `arboard` clipboard.

---

## Source Spec

Implement against:

- `docs/superpowers/specs/2026-06-30-ocr-selectable-text-design.md`

Critical locked decisions from the spec:

- OCR Text is compile-time gated behind `rollshot-app/ocr`; builds without `ocr` omit the toolbar item.
- Redaction masking is a hard data invariant: OCR text covered by redaction annotations is never rendered, selectable, or copyable.
- v1 targets axis-aligned line/block and cross-line selection. Rotated character selection and low-confidence interaction tiers are deferred.
- Ctrl/Cmd+C copies selected OCR text only. With no selection, show "No OCR text selected".
- Copy-all is a separate explicit in-mode action.
- Long captures use deterministic vertical tiles with clean seam merging.

## Plan Review Addendum (2026-07-01)

Auto-mode engineering review locked these corrections before execution:

- `OcrTextLayer` is only stacked while `Tool::OcrText` is active. A prepared OCR cache must not leave an event-catching text layer above the annotation canvas after switching back to Select/Number/Text/Redact.
- `OcrTextLayer` owns pointer-drag state in its iced `Tree` state. Cursor movement without a held left button must not mutate OCR selection.
- `OcrTextLayer` receives the current visible image rect and only builds/draws/hit-tests paragraphs for visible OCR items. Copy/select-all still operate on the full `OcrTextDocument`.
- Use existing `ImageRect::contains`; do not add a duplicate `contains_point` helper.
- Add the spec-required Escape behavior: in OCR Text mode, Escape clears OCR selection first; a second Escape leaves OCR Text mode and returns to Select.
- Keep new workspace tests in the existing `update.rs` test module and use the existing `workspace()` helper name.

### NOT in scope

- Rotated character-level selection: deferred by the source spec; v1 remains deterministic axis-aligned line/block selection.
- Persisting OCR text in `ImageDocument`, result files, or history: source spec requires OCR data to stay ephemeral.
- Generic OCR settings UI: deferred; this plan only wires product OCR behind the compile-time `ocr` feature.
- Webview/DOM selection: explicitly excluded by the source spec and inconsistent with Rollshot's iced app.
- Low-confidence interaction tiers: deferred to a later product iteration.

### What already exists

- `rollshot-ocr` already wraps RapidOCR and returns native-coordinate detections; this plan extends its contract with quadrilateral points rather than rebuilding OCR.
- `rollshot-vision::RealAutomationHost` already prepares OCR from `VisualIndex`; this plan reuses it for product OCR tiles instead of Smart Redaction workbench helpers.
- `result_workspace::viewport` already owns zoom/scroll geometry; this plan adds small pure transform helpers and reuses existing `visible_image_rect`.
- The result workspace already stacks image + annotation canvas and has update tests; this plan adds OCR Text mode to that surface instead of adding another window or document layer.
- `rollshot-image-document::ImageRect` already provides `contains` and `intersects`; OCR text code should reuse those methods.

### Failure Modes

| Codepath | Production failure | Test coverage | Error handling / user result |
|---|---|---|---|
| OCR engine init | ONNX/RapidOCR session cannot initialize | Task 3 / Step 5 feature build; Task 7 failure-state tests | `ProductOcrError::SessionInit`, inline error, returns to Select |
| OCR detect | Model inference fails for a tile | Task 3 / Step 5 feature build; Task 7 failure-state tests | `ProductOcrError::Detect`, inline error, returns to Select |
| OCR region | Bad/oversized region or empty image | Task 3 tile tests + feature build | `ProductOcrError::InvalidRegion`, inline error |
| Tiling | Long capture drops or duplicates seam text | Task 3 / Step 1 seam tests; Task 8 manual long screenshot check | `merge_tile_items` removes high-IoU duplicates; failures are visible in copy/manual verification |
| Redaction refresh | Redacted OCR remains selectable/copyable | Task 2 redaction tests + Task 7 refresh tests | Redaction signature rebuilds visible document and invalidates stale selections |
| Iced OCR layer | Hover changes selection or steals annotation events | Task 5 build check + Task 6 active-mode gate + Task 8 manual OCR drag check | Widget captures only during OCR drag and only when OCR Text mode is active |
| Clipboard text | Platform clipboard open/write fails | Task 7 copy-without-selection test; existing action pattern | `CopyOcrFinished(Err)` shows inline error |

Critical gaps after review: none, provided the corrections in this addendum are implemented as written.

### Test Coverage

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / OCR quad preservation through OCR + vision | ✓ | ✓ | — | no |
| Task 2 / reading order, redaction filtering, reverse selection | ✓ | — | — | no |
| Task 3 / vertical tiling and seam merge | ✓ | — | — | no |
| Task 4 / tool state, keyboard copy/select-all, Escape | ✓ | — | — | no |
| Task 5 / hit-test helper and custom widget compile path | ✓ | — | — | no |
| Task 6 / viewport transform and active-mode layer gate | ✓ | — | — | no |
| Task 7 / privacy-safe debug, redaction refresh, empty copy | ✓ | — | — | no |
| Task 8 / full app with OCR enabled | — | — | ✓ | yes |

### Parallelization Strategy

Sequential execution, no parallelization opportunity. Most tasks touch the same primary module family (`crates/rollshot-app/src/result_workspace/`) and later tasks depend on the OCR model/state from earlier tasks.

## File Structure

Create:

- `crates/rollshot-app/src/result_workspace/ocr_text.rs`  
  Product OCR state, product OCR preparation, tiling, seam merging, redaction filtering, reading order, selection ranges, selected/copy-all formatting, pure tests.

- `crates/rollshot-app/src/result_workspace/ocr_layer.rs`  
  Custom iced `advanced::Widget` for visible OCR text, character hit-testing, highlight drawing, pointer capture, visible-region culling, and text cursor interaction.

Modify:

- `crates/rollshot-ocr/src/lib.rs`  
  Preserve OCR quadrilateral points in `OcrDetection`.

- `crates/rollshot-automation/src/capability.rs`  
  Add quadrilateral geometry to `OcrMatch`.

- `crates/rollshot-vision/src/host.rs`  
  Map OCR quadrilateral points from crop-local to full-image coordinates.

- `crates/rollshot-agent/src/tools.rs`  
  Include OCR quadrilateral geometry in summarized tool output without logging raw OCR text.

- `crates/rollshot-agent/src/driver.rs`  
  Update tests/fixtures constructing `OcrMatch`.

- `crates/rollshot-app/Cargo.toml`  
  Add iced `advanced` feature for `OcrTextLayer`.

- `crates/rollshot-app/src/result_workspace/mod.rs`  
  Register `ocr_text`/`ocr_layer`, add `ResultWorkspace::ocr_text`.

- `crates/rollshot-app/src/result_workspace/canvas.rs`  
  Add `Tool::OcrText` behind `#[cfg(feature = "ocr")]`.

- `crates/rollshot-app/src/result_workspace/actions.rs`  
  Add `copy_text`.

- `crates/rollshot-app/src/result_workspace/update.rs`  
  Add OCR messages, OCR prepare task, OCR selection state transitions, keyboard copy/select-all, redaction-mask refresh.

- `crates/rollshot-app/src/result_workspace/view.rs`  
  Add compile-time-gated toolbar item, stack `OcrTextLayer`, and contextual copy-all action in OCR Text mode.

- `crates/rollshot-app/src/result_workspace/viewport.rs`  
  Add pure transform helpers used by the OCR layer and tests.

---

### Task 1: Preserve OCR Quadrilateral Geometry

**Files:**
- Modify: `crates/rollshot-ocr/src/lib.rs`
- Modify: `crates/rollshot-automation/src/capability.rs`
- Modify: `crates/rollshot-vision/src/host.rs`
- Modify: `crates/rollshot-agent/src/tools.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`

- [ ] **Step 1: Add failing tests for OCR quadrilateral preservation**

In `crates/rollshot-ocr/src/lib.rs`, add this test beside `upscale_inversion_keeps_native_coords`:

```rust
#[test]
fn detection_preserves_four_point_quad_after_upscale_inversion() {
    let mut engine = OcrEngine::new().unwrap();
    let img = text_image("acct 12345");
    let detections = engine.detect(&img, &OcrRegionQuery::default()).unwrap();
    let first = detections.first().expect("expected OCR text");

    assert_eq!(first.quad.len(), 4);
    for p in first.quad {
        assert!(p.0.is_finite());
        assert!(p.1.is_finite());
        assert!(p.0 >= 0.0);
        assert!(p.1 >= 0.0);
        assert!(p.0 <= img.width() as f32);
        assert!(p.1 <= img.height() as f32);
    }
}
```

In `crates/rollshot-vision/src/host.rs`, add this test in the OCR test section:

```rust
#[cfg(feature = "ocr")]
#[test]
fn prepare_then_ocr_maps_quad_to_full_image_bounds() {
    use rollshot_automation::{AutomationHost, OcrQuery, Region};
    use rollshot_image_document::ImageRect;

    let image = text_scene("secret 123");
    let index = VisualIndex::build(image).unwrap();
    let mut host = RealAutomationHost::new();
    let query = OcrQuery {
        region: Region::Rect {
            bounds: ImageRect {
                x: 10.0,
                y: 10.0,
                width: 240.0,
                height: 80.0,
            },
        },
        limit: 10,
    };

    host.prepare_ocr(&index, &query).unwrap();
    let matches = host.ocr(query).unwrap();
    let first = matches.first().expect("expected OCR match");

    assert_eq!(first.quad.len(), 4);
    assert!(first.quad.iter().all(|p| p.x >= 10.0 && p.y >= 10.0));
    assert!(first.bounds.x >= 10.0);
    assert!(first.bounds.y >= 10.0);
}
```

- [ ] **Step 2: Run tests and verify they fail for missing `quad`**

Run:

```bash
rtk cargo test -p rollshot-ocr detection_preserves_four_point_quad_after_upscale_inversion
rtk cargo test -p rollshot-vision --features ocr prepare_then_ocr_maps_quad_to_full_image_bounds
```

Expected:

- `rollshot-ocr` fails with `no field 'quad' on type OcrDetection`.
- `rollshot-vision` fails with `no field 'quad' on type OcrMatch`.

- [ ] **Step 3: Add `quad` to `OcrDetection` and preserve scaled coordinates**

In `crates/rollshot-ocr/src/lib.rs`, change `OcrDetection`:

```rust
pub struct OcrDetection {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub quad: [(f32, f32); 4],
    pub text: String,
    pub confidence: f32,
}
```

In `OcrEngine::detect`, replace:

```rust
if block.box_points.is_empty() {
    continue;
}
let (x, y, w, h) = aabb(&block.box_points);
```

with:

```rust
if block.box_points.len() != 4 {
    continue;
}
let (x, y, w, h) = aabb(&block.box_points);
let quad = [
    (
        block.box_points[0].x as f32 / scale,
        block.box_points[0].y as f32 / scale,
    ),
    (
        block.box_points[1].x as f32 / scale,
        block.box_points[1].y as f32 / scale,
    ),
    (
        block.box_points[2].x as f32 / scale,
        block.box_points[2].y as f32 / scale,
    ),
    (
        block.box_points[3].x as f32 / scale,
        block.box_points[3].y as f32 / scale,
    ),
];
```

Then include `quad` in the pushed detection:

```rust
out.push(OcrDetection {
    x,
    y,
    w,
    h,
    quad,
    text: block.text.clone(),
    confidence: block.text_score,
});
```

- [ ] **Step 4: Add `quad` to automation `OcrMatch`**

In `crates/rollshot-automation/src/capability.rs`, change `OcrMatch`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrMatch {
    pub bounds: ImageRect,
    pub quad: [ImagePoint; 4],
    pub text: String,
    pub confidence: f32,
}
```

- [ ] **Step 5: Map crop-local quad coordinates in vision host**

In `crates/rollshot-vision/src/host.rs`, inside `prepare_ocr`, replace the `Some(OcrMatch { ... })` block with:

```rust
let quad = d.quad.map(|(x, y)| ImagePoint {
    x: x + ox,
    y: y + oy,
});
Some(OcrMatch {
    bounds,
    quad,
    text: d.text,
    confidence: d.confidence,
})
```

- [ ] **Step 6: Update test fixtures and tool summaries constructing `OcrMatch`**

Search:

```bash
rtk rg -n "OcrMatch \\{" crates/rollshot-agent crates/rollshot-automation crates/rollshot-vision crates/rollshot-app
```

For each test fixture, add:

```rust
quad: [
    ImagePoint { x: bounds.x, y: bounds.y },
    ImagePoint { x: bounds.x + bounds.width, y: bounds.y },
    ImagePoint {
        x: bounds.x + bounds.width,
        y: bounds.y + bounds.height,
    },
    ImagePoint { x: bounds.x, y: bounds.y + bounds.height },
],
```

In `crates/rollshot-agent/src/tools.rs`, extend `OcrMatchSummary`:

```rust
pub struct OcrMatchSummary {
    pub bounds: rollshot_image_document::ImageRect,
    pub quad: [rollshot_image_document::ImagePoint; 4],
    pub text: String,
    pub confidence: f32,
}
```

and map:

```rust
OcrMatchSummary {
    bounds: m.bounds,
    quad: m.quad,
    text: m.text,
    confidence: m.confidence,
}
```

- [ ] **Step 7: Run focused OCR contract tests**

Run:

```bash
rtk cargo test -p rollshot-ocr detection_preserves_four_point_quad_after_upscale_inversion
rtk cargo test -p rollshot-vision --features ocr prepare_then_ocr_maps_quad_to_full_image_bounds
rtk cargo test -p rollshot-agent ocr
```

Expected: all pass.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-ocr/src/lib.rs crates/rollshot-automation/src/capability.rs crates/rollshot-vision/src/host.rs crates/rollshot-agent/src/tools.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(ocr): preserve OCR quadrilateral geometry"
```

---

### Task 2: Add Pure OCR Text Model

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`

- [ ] **Step 1: Create failing pure-model tests**

Create `crates/rollshot-app/src/result_workspace/ocr_text.rs` with this initial test module and minimal imports:

```rust
use std::hash::{Hash, Hasher};

use rollshot_image_document::{Annotation, AnnotationId, ImagePoint, ImageRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OcrItemId(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> ImageRect {
        ImageRect { x, y, width, height }
    }

    fn quad(bounds: ImageRect) -> [ImagePoint; 4] {
        [
            ImagePoint { x: bounds.x, y: bounds.y },
            ImagePoint { x: bounds.x + bounds.width, y: bounds.y },
            ImagePoint {
                x: bounds.x + bounds.width,
                y: bounds.y + bounds.height,
            },
            ImagePoint { x: bounds.x, y: bounds.y + bounds.height },
        ]
    }

    fn item(id: u64, text: &str, bounds: ImageRect) -> OcrTextItem {
        OcrTextItem {
            id: OcrItemId(id),
            text: text.into(),
            confidence: 0.95,
            bounds,
            quad: quad(bounds),
        }
    }

    #[test]
    fn normalized_order_groups_lines_top_to_bottom_left_to_right() {
        let items = vec![
            item(3, "second", rect(10.0, 50.0, 60.0, 12.0)),
            item(2, "world", rect(80.0, 10.0, 50.0, 12.0)),
            item(1, "hello", rect(10.0, 11.0, 50.0, 12.0)),
        ];

        let doc = OcrTextDocument::from_items(items, &[]);
        assert_eq!(doc.copy_all_text(), "hello world\nsecond");
    }

    #[test]
    fn redaction_intersections_are_not_copyable() {
        let items = vec![
            item(1, "visible", rect(10.0, 10.0, 50.0, 12.0)),
            item(2, "secret", rect(10.0, 40.0, 50.0, 12.0)),
        ];
        let redactions = vec![Annotation::OpaqueRedaction {
            id: AnnotationId(1),
            bounds: rect(8.0, 38.0, 60.0, 18.0),
        }];

        let doc = OcrTextDocument::from_items(items, &redactions);
        assert_eq!(doc.copy_all_text(), "visible");
        assert!(doc.visible_items().iter().all(|item| item.text != "secret"));
    }

    #[test]
    fn reverse_selection_copies_same_text_as_forward_selection() {
        let items = vec![
            item(1, "alpha", rect(10.0, 10.0, 50.0, 12.0)),
            item(2, "beta", rect(70.0, 10.0, 40.0, 12.0)),
            item(3, "gamma", rect(10.0, 40.0, 60.0, 12.0)),
        ];
        let doc = OcrTextDocument::from_items(items, &[]);

        let forward = OcrSelection::range(TextCursor::new(0, 1), TextCursor::new(2, 3));
        let backward = OcrSelection::range(TextCursor::new(2, 3), TextCursor::new(0, 1));

        assert_eq!(
            doc.selected_text(&forward),
            doc.selected_text(&backward)
        );
        assert_eq!(doc.selected_text(&forward), "lpha beta\ngam");
    }
}
```

- [ ] **Step 2: Wire the module so tests compile against real names**

In `crates/rollshot-app/src/result_workspace/mod.rs`, add:

```rust
pub(crate) mod ocr_text;
```

- [ ] **Step 3: Run tests and verify missing model types fail**

Run:

```bash
rtk cargo test -p rollshot-app ocr_text
```

Expected: failures for missing `OcrTextItem`, `OcrTextDocument`, `OcrSelection`, and `TextCursor`.

- [ ] **Step 4: Implement pure OCR model**

In `crates/rollshot-app/src/result_workspace/ocr_text.rs`, above the test module, add:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextItem {
    pub id: OcrItemId,
    pub text: String,
    pub confidence: f32,
    pub bounds: ImageRect,
    pub quad: [ImagePoint; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCursor {
    pub item_index: usize,
    pub char_index: usize,
}

impl TextCursor {
    pub fn new(item_index: usize, char_index: usize) -> Self {
        Self {
            item_index,
            char_index,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrSelection {
    pub anchor: TextCursor,
    pub focus: TextCursor,
}

impl OcrSelection {
    pub fn range(anchor: TextCursor, focus: TextCursor) -> Self {
        Self { anchor, focus }
    }

    pub fn normalized(self) -> (TextCursor, TextCursor) {
        if (self.anchor.item_index, self.anchor.char_index)
            <= (self.focus.item_index, self.focus.char_index)
        {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextDocument {
    visible_items: Vec<OcrTextItem>,
    line_break_after: Vec<bool>,
}

impl OcrTextDocument {
    pub fn from_items(items: Vec<OcrTextItem>, redactions: &[Annotation]) -> Self {
        let mut visible_items: Vec<OcrTextItem> = items
            .into_iter()
            .filter(|item| !is_redacted(item.bounds, redactions))
            .collect();
        visible_items.sort_by(reading_order);

        let mut line_break_after = vec![false; visible_items.len()];
        for index in 0..visible_items.len().saturating_sub(1) {
            let current = visible_items[index].bounds;
            let next = visible_items[index + 1].bounds;
            line_break_after[index] = !same_line(current, next);
        }

        Self {
            visible_items,
            line_break_after,
        }
    }

    pub fn visible_items(&self) -> &[OcrTextItem] {
        &self.visible_items
    }

    pub fn copy_all_text(&self) -> String {
        self.text_for_range(TextCursor::new(0, 0), self.end_cursor())
    }

    pub fn selected_text(&self, selection: &OcrSelection) -> String {
        let (start, end) = selection.normalized();
        self.text_for_range(start, end)
    }

    pub fn end_cursor(&self) -> TextCursor {
        match self.visible_items.last() {
            Some(item) => TextCursor::new(
                self.visible_items.len() - 1,
                item.text.chars().count(),
            ),
            None => TextCursor::new(0, 0),
        }
    }

    pub fn selection_is_valid(&self, selection: &OcrSelection) -> bool {
        let (start, end) = selection.normalized();
        let Some(last) = self.visible_items.len().checked_sub(1) else {
            return false;
        };
        start.item_index <= last && end.item_index <= last
    }

    fn text_for_range(&self, start: TextCursor, end: TextCursor) -> String {
        if self.visible_items.is_empty() {
            return String::new();
        }

        let mut out = String::new();
        for index in start.item_index..=end.item_index.min(self.visible_items.len() - 1) {
            let item = &self.visible_items[index];
            let start_char = if index == start.item_index {
                start.char_index
            } else {
                0
            };
            let end_char = if index == end.item_index {
                end.char_index
            } else {
                item.text.chars().count()
            };
            if start_char < end_char {
                out.push_str(&slice_chars(&item.text, start_char, end_char));
            }
            if index < end.item_index && index < self.visible_items.len() - 1 {
                if self.line_break_after[index] {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
        }
        out.trim().to_string()
    }
}

fn is_redacted(bounds: ImageRect, redactions: &[Annotation]) -> bool {
    redactions.iter().any(|annotation| match annotation {
        Annotation::OpaqueRedaction { bounds: redaction, .. } => bounds.intersects(redaction),
        _ => false,
    })
}

pub fn redaction_signature(redactions: &[Annotation]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for annotation in redactions {
        if let Annotation::OpaqueRedaction { id, bounds } = annotation {
            id.hash(&mut hasher);
            bounds.x.to_bits().hash(&mut hasher);
            bounds.y.to_bits().hash(&mut hasher);
            bounds.width.to_bits().hash(&mut hasher);
            bounds.height.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn reading_order(a: &OcrTextItem, b: &OcrTextItem) -> std::cmp::Ordering {
    if same_line(a.bounds, b.bounds) {
        a.bounds
            .x
            .partial_cmp(&b.bounds.x)
            .unwrap_or(std::cmp::Ordering::Equal)
    } else {
        a.bounds
            .y
            .partial_cmp(&b.bounds.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

fn same_line(a: ImageRect, b: ImageRect) -> bool {
    let a_mid = a.y + a.height / 2.0;
    let b_mid = b.y + b.height / 2.0;
    (a_mid - b_mid).abs() <= a.height.max(b.height) * 0.6
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}
```

- [ ] **Step 5: Run pure model tests**

Run:

```bash
rtk cargo test -p rollshot-app ocr_text
```

Expected: all `ocr_text` pure tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/ocr_text.rs
rtk git commit -m "feat(app): add OCR text model"
```

---

### Task 3: Add Product OCR Preparation And Tiling

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`

- [ ] **Step 1: Add failing tests for tiling and seam merging**

In `ocr_text.rs`, add tests:

```rust
#[test]
fn vertical_tiles_overlap_and_cover_full_height() {
    let tiles = vertical_tiles(1200, 40_000, 16_000_000, 64);

    assert_eq!(tiles.first().unwrap().y, 0);
    assert_eq!(tiles.last().unwrap().y + tiles.last().unwrap().height, 40_000);
    for pair in tiles.windows(2) {
        let first_bottom = pair[0].y + pair[0].height;
        assert!(pair[1].y < first_bottom);
    }
}

#[test]
fn seam_merge_removes_duplicate_text_with_high_iou() {
    let bounds = ImageRect {
        x: 10.0,
        y: 100.0,
        width: 120.0,
        height: 24.0,
    };
    let duplicate = OcrTextItem {
        id: OcrItemId(99),
        text: "duplicate".into(),
        confidence: 0.90,
        bounds,
        quad: [
            ImagePoint { x: 10.0, y: 100.0 },
            ImagePoint { x: 130.0, y: 100.0 },
            ImagePoint { x: 130.0, y: 124.0 },
            ImagePoint { x: 10.0, y: 124.0 },
        ],
    };
    let merged = merge_tile_items(vec![
        OcrTextItem { id: OcrItemId(1), ..duplicate.clone() },
        OcrTextItem { id: OcrItemId(2), ..duplicate },
    ]);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].text, "duplicate");
}
```

- [ ] **Step 2: Run tests and verify helper functions are missing**

Run:

```bash
rtk cargo test -p rollshot-app ocr_text
```

Expected: missing `vertical_tiles` and `merge_tile_items`.

- [ ] **Step 3: Implement tile planning and seam merging**

In `ocr_text.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrTile {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn vertical_tiles(width: u32, height: u32, max_area: u64, overlap: u32) -> Vec<OcrTile> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let tile_height = ((max_area / width.max(1) as u64) as u32).max(1).min(height);
    let step = tile_height.saturating_sub(overlap).max(1);
    let mut tiles = Vec::new();
    let mut y = 0;
    loop {
        let remaining = height - y;
        let h = tile_height.min(remaining);
        tiles.push(OcrTile {
            x: 0,
            y,
            width,
            height: h,
        });
        if y + h >= height {
            break;
        }
        y = (y + step).min(height - 1);
    }
    tiles
}

pub fn merge_tile_items(mut items: Vec<OcrTextItem>) -> Vec<OcrTextItem> {
    items.sort_by(reading_order);
    let mut merged: Vec<OcrTextItem> = Vec::new();
    'items: for item in items {
        for existing in &merged {
            if existing.text == item.text && iou(existing.bounds, item.bounds) >= 0.80 {
                continue 'items;
            }
        }
        merged.push(item);
    }
    for (index, item) in merged.iter_mut().enumerate() {
        item.id = OcrItemId(index as u64);
    }
    merged
}

fn iou(a: ImageRect, b: ImageRect) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let ix = (ax2.min(bx2) - a.x.max(b.x)).max(0.0);
    let iy = (ay2.min(by2) - a.y.max(b.y)).max(0.0);
    let intersection = ix * iy;
    let union = a.width * a.height + b.width * b.height - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}
```

- [ ] **Step 4: Add product OCR preparation behind `ocr` feature**

In `ocr_text.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductOcrError {
    Disabled,
    SessionInit,
    Detect,
    InvalidRegion,
}

impl ProductOcrError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Disabled => "OCR is not available in this build",
            Self::SessionInit => "OCR session initialization failed",
            Self::Detect => "OCR detection failed",
            Self::InvalidRegion => "OCR region is invalid",
        }
    }
}

#[cfg(feature = "ocr")]
pub fn prepare_product_ocr(image: &image::RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError> {
    use rollshot_automation::{AutomationHost, OcrQuery, Region};
    use rollshot_vision::{rect::MAX_OCR_AREA, RealAutomationHost, VisualIndex};

    let index = VisualIndex::build(image.clone()).map_err(|_| ProductOcrError::InvalidRegion)?;
    let mut host = RealAutomationHost::new();
    let tiles = vertical_tiles(index.width(), index.height(), MAX_OCR_AREA, 64);
    let mut items = Vec::new();

    for tile in tiles {
        let query = OcrQuery {
            region: Region::Rect {
                bounds: ImageRect {
                    x: tile.x as f32,
                    y: tile.y as f32,
                    width: tile.width as f32,
                    height: tile.height as f32,
                },
            },
            limit: 5_000,
        };
        host.prepare_ocr(&index, &query).map_err(|error| match error {
            rollshot_automation::CapabilityError::Failed { code: "ocr_session_init" } => {
                ProductOcrError::SessionInit
            }
            rollshot_automation::CapabilityError::Failed { code: "ocr_detect" } => {
                ProductOcrError::Detect
            }
            _ => ProductOcrError::InvalidRegion,
        })?;

        for m in host.ocr(query).map_err(|_| ProductOcrError::Detect)? {
            let id = OcrItemId(items.len() as u64);
            items.push(OcrTextItem {
                id,
                text: m.text,
                confidence: m.confidence,
                bounds: m.bounds,
                quad: m.quad,
            });
        }
    }

    Ok(merge_tile_items(items))
}

#[cfg(not(feature = "ocr"))]
pub fn prepare_product_ocr(_image: &image::RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError> {
    Err(ProductOcrError::Disabled)
}
```

- [ ] **Step 5: Run focused app tests**

Run:

```bash
rtk cargo test -p rollshot-app ocr_text
rtk cargo test -p rollshot-app --features ocr ocr_text
```

Expected: pure tests pass in both builds.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/ocr_text.rs
rtk git commit -m "feat(app): prepare tiled product OCR"
```

---

### Task 4: Add OCR Text Workspace State, Tool, Messages, And Clipboard

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/actions.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

- [ ] **Step 1: Add failing workspace tests**

In `crates/rollshot-app/src/result_workspace/update.rs`, add tests in the existing test module:

```rust
#[cfg(feature = "ocr")]
#[test]
fn selecting_ocr_tool_clears_annotation_drag_and_requests_prepare() {
    let mut state = workspace();
    state.editor.drag = Some(DragState::Pan {
        last_pointer: Point::new(10.0, 10.0),
    });

    let _ = update(&mut state, Message::SelectTool(Tool::OcrText));

    assert_eq!(state.editor.tool, Tool::OcrText);
    assert!(state.editor.drag.is_none());
    assert!(state.ocr_text.is_preparing_or_ready());
}

#[cfg(feature = "ocr")]
#[test]
fn canvas_press_in_ocr_text_mode_does_not_start_annotation_drag() {
    let mut state = workspace();
    state.editor.tool = Tool::OcrText;
    state.ocr_text.set_ready_for_tests(vec![crate::result_workspace::ocr_text::OcrTextItem {
        id: crate::result_workspace::ocr_text::OcrItemId(0),
        text: "secret".into(),
        confidence: 0.95,
        bounds: ImageRect {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 18.0,
        },
        quad: [
            ImagePoint { x: 10.0, y: 10.0 },
            ImagePoint { x: 90.0, y: 10.0 },
            ImagePoint { x: 90.0, y: 28.0 },
            ImagePoint { x: 10.0, y: 28.0 },
        ],
    }]);

    let _ = handle_canvas_pressed(&mut state, ImagePoint::new(12.0, 12.0), Instant::now());

    assert!(state.editor.drag.is_none());
    assert!(state.ocr_text.selection().is_none());
}

#[cfg(feature = "ocr")]
#[test]
fn command_c_in_ocr_mode_maps_to_copy_ocr_selection() {
    let msg = map_key_press(
        &keyboard::Key::Character("c".into()),
        keyboard::Modifiers::CTRL,
        false,
        Tool::OcrText,
    );

    assert_eq!(msg, Some(Message::CopyOcrSelection));
}

#[cfg(feature = "ocr")]
#[test]
fn command_a_in_ocr_mode_maps_to_select_all_ocr_text() {
    let msg = map_key_press(
        &keyboard::Key::Character("a".into()),
        keyboard::Modifiers::CTRL,
        false,
        Tool::OcrText,
    );

    assert_eq!(msg, Some(Message::SelectAllOcrText));
}

#[cfg(feature = "ocr")]
#[test]
fn escape_clears_ocr_selection_before_leaving_ocr_mode() {
    let mut state = workspace();
    state.editor.tool = Tool::OcrText;
    state.ocr_text.set_ready_for_tests(vec![crate::result_workspace::ocr_text::OcrTextItem {
        id: crate::result_workspace::ocr_text::OcrItemId(0),
        text: "secret".into(),
        confidence: 0.95,
        bounds: ImageRect {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 18.0,
        },
        quad: [
            ImagePoint { x: 10.0, y: 10.0 },
            ImagePoint { x: 90.0, y: 10.0 },
            ImagePoint { x: 90.0, y: 28.0 },
            ImagePoint { x: 10.0, y: 28.0 },
        ],
    }]);
    state.ocr_text.set_selection(Some(crate::result_workspace::ocr_text::OcrSelection::range(
        crate::result_workspace::ocr_text::TextCursor::new(0, 0),
        crate::result_workspace::ocr_text::TextCursor::new(0, 3),
    )));

    let _ = update(&mut state, Message::EscapePressed);
    assert_eq!(state.editor.tool, Tool::OcrText);
    assert!(state.ocr_text.selection().is_none());

    let _ = update(&mut state, Message::EscapePressed);
    assert_eq!(state.editor.tool, Tool::Select);
}
```

- [ ] **Step 2: Run tests and verify missing variants/fields fail**

Run:

```bash
rtk cargo test -p rollshot-app --features ocr ocr_text_mode
rtk cargo test -p rollshot-app --features ocr command_c_in_ocr_mode_maps_to_copy_ocr_selection
```

Expected: missing `Tool::OcrText`, `Message::CopyOcrSelection`, `ResultWorkspace::ocr_text`, and test helper methods.

- [ ] **Step 3: Enable iced advanced feature**

In `crates/rollshot-app/Cargo.toml`, change:

```toml
iced = { version = "0.14", features = ["canvas", "image", "tokio"] }
```

to:

```toml
iced = { version = "0.14", features = ["advanced", "canvas", "image", "tokio"] }
```

- [ ] **Step 4: Add OCR state to `ResultWorkspace`**

In `crates/rollshot-app/src/result_workspace/mod.rs`, add:

```rust
#[cfg(feature = "ocr")]
pub(crate) mod ocr_layer;
```

Add a field to `ResultWorkspace`:

```rust
#[cfg(feature = "ocr")]
pub ocr_text: ocr_text::OcrTextState,
```

Initialize it in `with_max_texture_dim`:

```rust
#[cfg(feature = "ocr")]
ocr_text: ocr_text::OcrTextState::idle(),
```

- [ ] **Step 5: Add OCR Text tool**

In `crates/rollshot-app/src/result_workspace/canvas.rs`, change `Tool`:

```rust
pub enum Tool {
    Select,
    Number,
    Text,
    Redact,
    #[cfg(feature = "ocr")]
    OcrText,
}
```

Update `direct_manipulation_hit` and the canvas pointer handlers in `update.rs` so annotation canvas gestures are inert while OCR Text mode is active:

```rust
#[cfg(feature = "ocr")]
Tool::OcrText => None,
```

In `handle_canvas_pressed`, add a `Tool::OcrText => Task::none()` match arm. `OcrTextLayer` owns OCR selection pointer messages; the annotation canvas must not start pan, annotation edit, or redaction gestures in OCR mode.

- [ ] **Step 6: Add OCR text state methods**

In `ocr_text.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum OcrTextStatus {
    Idle,
    Preparing,
    Ready(OcrTextDocument),
    Failed(ProductOcrError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextState {
    status: OcrTextStatus,
    selection: Option<OcrSelection>,
    raw_items: Vec<OcrTextItem>,
    redaction_signature: u64,
}

impl OcrTextState {
    pub fn idle() -> Self {
        Self {
            status: OcrTextStatus::Idle,
            selection: None,
            raw_items: Vec::new(),
            redaction_signature: 0,
        }
    }

    pub fn begin_prepare(&mut self) {
        self.status = OcrTextStatus::Preparing;
        self.selection = None;
        self.raw_items.clear();
        self.redaction_signature = 0;
    }

    pub fn finish_prepare(&mut self, items: Vec<OcrTextItem>, redactions: &[Annotation]) {
        self.redaction_signature = redaction_signature(redactions);
        self.raw_items = items.clone();
        self.status = OcrTextStatus::Ready(OcrTextDocument::from_items(items, redactions));
        self.selection = None;
    }

    pub fn fail_prepare(&mut self, error: ProductOcrError) {
        self.status = OcrTextStatus::Failed(error);
        self.selection = None;
        self.raw_items.clear();
        self.redaction_signature = 0;
    }

    pub fn is_preparing_or_ready(&self) -> bool {
        matches!(&self.status, OcrTextStatus::Preparing | OcrTextStatus::Ready(_))
    }

    pub fn document(&self) -> Option<&OcrTextDocument> {
        match &self.status {
            OcrTextStatus::Ready(document) => Some(document),
            _ => None,
        }
    }

    pub fn selection(&self) -> Option<&OcrSelection> {
        self.selection.as_ref()
    }

    pub fn set_selection(&mut self, selection: Option<OcrSelection>) {
        self.selection = selection;
    }

    pub fn refresh_redactions(&mut self, redactions: &[Annotation]) {
        if !matches!(&self.status, OcrTextStatus::Ready(_)) {
            return;
        }
        let signature = redaction_signature(redactions);
        if signature == self.redaction_signature {
            return;
        }

        self.redaction_signature = signature;
        let document = OcrTextDocument::from_items(self.raw_items.clone(), redactions);
        self.selection = self.selection.filter(|selection| document.selection_is_valid(selection));
        self.status = OcrTextStatus::Ready(document);
    }

    #[cfg(test)]
    pub fn set_ready_for_tests(&mut self, items: Vec<OcrTextItem>) {
        self.raw_items = items.clone();
        self.status = OcrTextStatus::Ready(OcrTextDocument::from_items(items, &[]));
        self.redaction_signature = 0;
        self.selection = None;
    }
}
```

- [ ] **Step 7: Add OCR messages**

In `update.rs`, add variants:

```rust
#[cfg(feature = "ocr")]
OcrPrepared(Result<Vec<super::ocr_text::OcrTextItem>, super::ocr_text::ProductOcrError>),
#[cfg(feature = "ocr")]
OcrSelectionStarted(super::ocr_text::TextCursor),
#[cfg(feature = "ocr")]
OcrSelectionChanged(super::ocr_text::TextCursor),
#[cfg(feature = "ocr")]
OcrSelectionFinished(super::ocr_text::TextCursor),
#[cfg(feature = "ocr")]
SelectAllOcrText,
#[cfg(feature = "ocr")]
CopyOcrSelection,
#[cfg(feature = "ocr")]
CopyAllOcrText,
#[cfg(feature = "ocr")]
CopyOcrFinished(Result<(), String>),
```

- [ ] **Step 8: Add clipboard text helper**

In `actions.rs`, add:

```rust
pub fn copy_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("clipboard error: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard write error: {e}"))
}
```

- [ ] **Step 9: Implement OCR update transitions**

In `update.rs`, add helper functions:

```rust
#[cfg(feature = "ocr")]
fn redactions(document: &ImageDocument) -> Vec<Annotation> {
    document
        .annotations()
        .iter()
        .filter(|annotation| matches!(annotation, Annotation::OpaqueRedaction { .. }))
        .cloned()
        .collect()
}

#[cfg(feature = "ocr")]
fn prepare_ocr_task(state: &mut super::ResultWorkspace) -> Task<Message> {
    state.ocr_text.begin_prepare();
    let image = state.document.image.source().clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || super::ocr_text::prepare_product_ocr(&image))
                .await
                .unwrap_or(Err(super::ocr_text::ProductOcrError::Detect))
        },
        Message::OcrPrepared,
    )
}
```

In the `Message::SelectTool(tool)` branch, add:

```rust
#[cfg(feature = "ocr")]
if tool == Tool::OcrText {
    commit_text_draft(state);
    state.editor.drag = None;
    state.editor.selection = None;
    state.editor.tool = Tool::OcrText;
    if state.ocr_text.document().is_none() {
        return prepare_ocr_task(state);
    }
    return Task::none();
}
```

Add message handling:

```rust
#[cfg(feature = "ocr")]
Message::OcrPrepared(Ok(items)) => {
    let redactions = redactions(&state.document.image);
    state.ocr_text.finish_prepare(items, &redactions);
    state.message = None;
    Task::none()
}
#[cfg(feature = "ocr")]
Message::OcrPrepared(Err(error)) => {
    state.ocr_text.fail_prepare(error.clone());
    state.editor.tool = Tool::Select;
    state.message = Some(InlineMessage::Error(error.message().to_string()));
    Task::none()
}
#[cfg(feature = "ocr")]
Message::OcrSelectionStarted(cursor) => {
    state.ocr_text.set_selection(Some(super::ocr_text::OcrSelection::range(cursor, cursor)));
    Task::none()
}
#[cfg(feature = "ocr")]
Message::OcrSelectionChanged(cursor) | Message::OcrSelectionFinished(cursor) => {
    if let Some(selection) = state.ocr_text.selection().copied() {
        state.ocr_text.set_selection(Some(super::ocr_text::OcrSelection::range(
            selection.anchor,
            cursor,
        )));
    }
    Task::none()
}
#[cfg(feature = "ocr")]
Message::SelectAllOcrText => {
    if let Some(document) = state.ocr_text.document() {
        state.ocr_text.set_selection(Some(super::ocr_text::OcrSelection::range(
            super::ocr_text::TextCursor::new(0, 0),
            document.end_cursor(),
        )));
    }
    Task::none()
}
#[cfg(feature = "ocr")]
Message::CopyOcrSelection => {
    let Some(document) = state.ocr_text.document() else {
        state.message = Some(InlineMessage::Error("No OCR text selected".into()));
        return Task::none();
    };
    let Some(selection) = state.ocr_text.selection() else {
        state.message = Some(InlineMessage::Error("No OCR text selected".into()));
        return Task::none();
    };
    let text = document.selected_text(selection);
    if text.is_empty() {
        state.message = Some(InlineMessage::Error("No OCR text selected".into()));
        return Task::none();
    }
    Task::done(Message::CopyOcrFinished(super::actions::copy_text(&text)))
}
#[cfg(feature = "ocr")]
Message::CopyAllOcrText => {
    let Some(document) = state.ocr_text.document() else {
        state.message = Some(InlineMessage::Error("No OCR text available".into()));
        return Task::none();
    };
    let text = document.copy_all_text();
    if text.is_empty() {
        state.message = Some(InlineMessage::Error("No OCR text available".into()));
        return Task::none();
    }
    Task::done(Message::CopyOcrFinished(super::actions::copy_text(&text)))
}
#[cfg(feature = "ocr")]
Message::CopyOcrFinished(Ok(())) => {
    state.message = Some(InlineMessage::success("Copied OCR text".into()));
    Task::none()
}
#[cfg(feature = "ocr")]
Message::CopyOcrFinished(Err(error)) => {
    state.message = Some(InlineMessage::Error(error));
    Task::none()
}
```

- [ ] **Step 10: Route Escape, keyboard copy, and select-all in OCR mode**

In the `Message::EscapePressed` branch, before the existing pending-action / copy-menu / text-draft / drag / annotation-selection chain, add:

```rust
#[cfg(feature = "ocr")]
if state.editor.tool == Tool::OcrText {
    if state.ocr_text.selection().is_some() {
        state.ocr_text.set_selection(None);
        return Task::none();
    }
    state.editor.tool = Tool::Select;
    return Task::none();
}
```

Change `map_key_press` signature to include current tool:

```rust
pub(crate) fn map_key_press(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    captured: bool,
    tool: Tool,
) -> Option<Message>
```

Update subscription call:

```rust
map_key_press(&key, modifiers, status == iced::event::Status::Captured, state.editor.tool)
```

In `map_key_press`, before the existing command match:

```rust
#[cfg(feature = "ocr")]
if tool == Tool::OcrText && command {
    if let keyboard::Key::Character(c) = key {
        return match c.as_str() {
            "a" => Some(Message::SelectAllOcrText),
            "c" => Some(Message::CopyOcrSelection),
            _ => None,
        };
    }
}
```

Update existing tests that call `map_key_press` by passing `Tool::Select`.

- [ ] **Step 11: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app --features ocr ocr_text_mode
rtk cargo test -p rollshot-app --features ocr command_c_in_ocr_mode_maps_to_copy_ocr_selection
rtk cargo test -p rollshot-app --features ocr command_a_in_ocr_mode_maps_to_select_all_ocr_text
rtk cargo test -p rollshot-app --features ocr escape_clears_ocr_selection_before_leaving_ocr_mode
rtk cargo test -p rollshot-app map_key_press
```

Expected: all pass.

- [ ] **Step 12: Commit**

```bash
rtk git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/actions.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/ocr_text.rs
rtk git commit -m "feat(app): add OCR text workspace state"
```

---

### Task 5: Implement `OcrTextLayer` Custom Widget

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/ocr_layer.rs`
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`

- [ ] **Step 1: Add pure hit-test tests for text cursor mapping**

In `ocr_text.rs`, add:

```rust
#[test]
fn axis_aligned_hit_test_maps_x_to_character_index() {
    let item = item(1, "secret", rect(10.0, 10.0, 60.0, 12.0));
    assert_eq!(
        character_index_for_axis_aligned_item(&item, ImagePoint { x: 10.0, y: 12.0 }),
        0
    );
    assert_eq!(
        character_index_for_axis_aligned_item(&item, ImagePoint { x: 40.0, y: 12.0 }),
        3
    );
    assert_eq!(
        character_index_for_axis_aligned_item(&item, ImagePoint { x: 70.0, y: 12.0 }),
        6
    );
}
```

- [ ] **Step 2: Implement deterministic fallback hit-test helper**

In `ocr_text.rs`, add:

```rust
pub fn character_index_for_axis_aligned_item(item: &OcrTextItem, point: ImagePoint) -> usize {
    let chars = item.text.chars().count();
    if chars == 0 || item.bounds.width <= 0.0 {
        return 0;
    }
    let t = ((point.x - item.bounds.x) / item.bounds.width).clamp(0.0, 1.0);
    ((chars as f32) * t).round() as usize
}
```

This helper is a deterministic fallback for tests and for OCR boxes whose iced paragraph hit-test returns `None`.

- [ ] **Step 3: Create `OcrTextLayer` skeleton**

Create `crates/rollshot-app/src/result_workspace/ocr_layer.rs`:

```rust
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, Clipboard, Shell};
use iced::{alignment, Color, Element, Event, Length, Point, Rectangle, Size};
use rollshot_image_document::{ImagePoint, ImageRect};

use super::ocr_text::{OcrSelection, OcrTextDocument, TextCursor};
use super::update::Message;

pub struct OcrTextLayer<'a> {
    document: Option<&'a OcrTextDocument>,
    selection: Option<OcrSelection>,
    scale: f32,
    visible: ImageRect,
    width: f32,
    height: f32,
}

#[derive(Default)]
struct State<Renderer>
where
    Renderer: text::Renderer,
{
    paragraphs: Vec<(usize, Renderer::Paragraph)>,
    dragging: bool,
}

pub fn ocr_text_layer(
    document: Option<&OcrTextDocument>,
    selection: Option<OcrSelection>,
    scale: f32,
    visible: ImageRect,
    size: Size,
) -> OcrTextLayer<'_> {
    OcrTextLayer {
        document,
        selection,
        scale,
        visible,
        width: size.width,
        height: size.height,
    }
}

impl<'a, MessageT, Theme, Renderer> Widget<MessageT, Theme, Renderer> for OcrTextLayer<'a>
where
    MessageT: From<Message> + Clone + 'a,
    Renderer: renderer::Renderer + text::Renderer<Font = iced::Font>,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer>::default())
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer>>();
        state.paragraphs.clear();
        if let Some(document) = self.document {
            for (index, item) in document.visible_items().iter().enumerate() {
                if !item.bounds.intersects(&self.visible) {
                    continue;
                }
                state.paragraphs.push((index, Renderer::Paragraph::with_text(text::Text {
                    content: item.text.as_str(),
                    bounds: Size::new(item.bounds.width * self.scale, item.bounds.height * self.scale),
                    size: iced::Pixels((item.bounds.height * self.scale).max(8.0)),
                    line_height: text::LineHeight::Relative(1.0),
                    font: renderer.default_font(),
                    align_x: text::Alignment::Left,
                    align_y: alignment::Vertical::Center,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                })));
            }
        }
        layout::Node::new(Size::new(self.width, self.height))
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, MessageT>,
        _viewport: &Rectangle,
    ) {
        let Some(document) = self.document else {
            return;
        };
        let state = tree.state.downcast_mut::<State<Renderer>>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(local) = cursor.position_over(layout.bounds()) else {
                    return;
                };
                let point = ImagePoint {
                    x: local.x / self.scale,
                    y: local.y / self.scale,
                };
                if let Some(hit) = hit_test(document, &state.paragraphs, self.scale, point) {
                    state.dragging = true;
                    shell.publish(Message::OcrSelectionStarted(hit).into());
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.dragging {
                    return;
                }
                let Some(local) = cursor.position_over(layout.bounds()) else {
                    shell.capture_event();
                    return;
                };
                let point = ImagePoint {
                    x: local.x / self.scale,
                    y: local.y / self.scale,
                };
                if let Some(hit) = hit_test(document, &state.paragraphs, self.scale, point) {
                    shell.publish(Message::OcrSelectionChanged(hit).into());
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    return;
                }
                state.dragging = false;
                let Some(local) = cursor.position_over(layout.bounds()) else {
                    shell.capture_event();
                    return;
                };
                let point = ImagePoint {
                    x: local.x / self.scale,
                    y: local.y / self.scale,
                };
                if let Some(hit) = hit_test(document, &state.paragraphs, self.scale, point) {
                    shell.publish(Message::OcrSelectionFinished(hit).into());
                }
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(document) = self.document else {
            return;
        };
        let state = tree.state.downcast_ref::<State<Renderer>>();
        let origin = layout.bounds().position();

        draw_selection(
            renderer,
            document,
            self.selection,
            self.scale,
            origin,
            state.paragraphs.iter().map(|(index, _)| *index),
        );

        for (index, paragraph) in &state.paragraphs {
            let item = &document.visible_items()[*index];
            let position = Point::new(
                origin.x + item.bounds.x * self.scale,
                origin.y + item.bounds.y * self.scale,
            );
            renderer.fill_paragraph(paragraph, position, Color::TRANSPARENT, *viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let Some(document) = self.document else {
            return mouse::Interaction::default();
        };
        let Some(local) = cursor.position_over(layout.bounds()) else {
            return mouse::Interaction::default();
        };
        let state = tree.state.downcast_ref::<State<Renderer>>();
        let point = ImagePoint {
            x: local.x / self.scale,
            y: local.y / self.scale,
        };
        if hit_test(document, &state.paragraphs, self.scale, point).is_some() {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, MessageT, Theme, Renderer> From<OcrTextLayer<'a>> for Element<'a, MessageT, Theme, Renderer>
where
    MessageT: From<Message> + Clone + 'a,
    Renderer: renderer::Renderer + text::Renderer<Font = iced::Font> + 'a,
    Theme: 'a,
{
    fn from(layer: OcrTextLayer<'a>) -> Self {
        Element::new(layer)
    }
}
```

- [ ] **Step 4: Add layer helper functions**

Append to `ocr_layer.rs`:

```rust
fn hit_test<ParagraphT>(
    document: &OcrTextDocument,
    paragraphs: &[(usize, ParagraphT)],
    scale: f32,
    point: ImagePoint,
) -> Option<TextCursor>
where
    ParagraphT: Paragraph<Font = iced::Font>,
{
    for (index, paragraph) in paragraphs {
        let item = &document.visible_items()[*index];
        if !item.bounds.contains(point) {
            continue;
        }
        let local = Point::new(
            (point.x - item.bounds.x) * scale,
            (point.y - item.bounds.y) * scale,
        );
        let char_index = paragraph
            .hit_test(local)
            .map(|hit| hit.cursor())
            .unwrap_or_else(|| super::ocr_text::character_index_for_axis_aligned_item(item, point));
        return Some(TextCursor::new(*index, char_index));
    }
    None
}

fn draw_selection<Renderer>(
    renderer: &mut Renderer,
    document: &OcrTextDocument,
    selection: Option<OcrSelection>,
    scale: f32,
    origin: Point,
    visible_indices: impl IntoIterator<Item = usize>,
) where
    Renderer: renderer::Renderer,
{
    let Some(selection) = selection else {
        return;
    };
    let (start, end) = selection.normalized();
    let color = Color {
        r: 0.10,
        g: 0.42,
        b: 0.95,
        a: 0.28,
    };
    for index in visible_indices {
        let item = &document.visible_items()[index];
        if index < start.item_index || index > end.item_index {
            continue;
        }
        let chars = item.text.chars().count().max(1);
        let start_frac = if index == start.item_index {
            start.char_index as f32 / chars as f32
        } else {
            0.0
        };
        let end_frac = if index == end.item_index {
            end.char_index as f32 / chars as f32
        } else {
            1.0
        };
        if end_frac <= start_frac {
            continue;
        }
        let x = item.bounds.x + item.bounds.width * start_frac;
        let w = item.bounds.width * (end_frac - start_frac);
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: origin.x + x * scale,
                    y: origin.y + item.bounds.y * scale,
                    width: w * scale,
                    height: item.bounds.height * scale,
                },
                ..renderer::Quad::default()
            },
            color,
        );
    }
}
```

Use the existing `ImageRect::contains` method for point hit-testing. Do not add a duplicate geometry helper in `ocr_text.rs`.

- [ ] **Step 5: Fix text color to visible and clip correctly**

In `draw`, replace:

```rust
renderer.fill_paragraph(paragraph, position, Color::TRANSPARENT, *viewport);
```

with:

```rust
renderer.fill_paragraph(
    paragraph,
    position,
    Color {
        r: 0.05,
        g: 0.05,
        b: 0.05,
        a: 0.01,
    },
    *viewport,
);
```

This keeps OCR text barely visible while preserving the selection target. If design review wants fully invisible selectable text, set alpha to `0.0` only after verifying text hit-testing still works visually with highlights.

- [ ] **Step 6: Run build check**

Run:

```bash
rtk cargo check -p rollshot-app --features ocr
```

Expected: compiles.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/ocr_layer.rs crates/rollshot-app/src/result_workspace/ocr_text.rs
rtk git commit -m "feat(app): add OCR text selection layer"
```

---

### Task 6: Wire OCR Text Layer Into Result Workspace UI

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-app/src/result_workspace/viewport.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

- [ ] **Step 1: Add viewport transform tests**

In `viewport.rs`, add tests:

```rust
#[test]
fn image_rect_to_canvas_rect_scales_without_scroll() {
    let rect = image_rect_to_canvas_rect(
        rollshot_image_document::ImageRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        },
        2.0,
    );

    assert_eq!(rect.x, 20.0);
    assert_eq!(rect.y, 40.0);
    assert_eq!(rect.width, 60.0);
    assert_eq!(rect.height, 80.0);
}
```

- [ ] **Step 2: Add transform helper**

In `viewport.rs`, add:

```rust
pub fn image_rect_to_canvas_rect(
    rect: rollshot_image_document::ImageRect,
    scale: f32,
) -> iced::Rectangle {
    iced::Rectangle {
        x: rect.x * scale,
        y: rect.y * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}
```

- [ ] **Step 3: Add compile-time-gated toolbar item**

In `view.rs`, replace the toolbar row with a small builder so the OCR item is gated:

```rust
let mut tools = row![
    button(text("Close")).on_press(Message::RequestClose),
    text(state.document.display_name()).width(Length::Fill),
    tool_button(ICON_SELECT, "Select", "V", Tool::Select, state),
    tool_button(ICON_NUMBER, "Number", "N", Tool::Number, state),
    tool_button(ICON_TEXT, "Text", "T", Tool::Text, state),
    tool_button(ICON_REDACT, "Redact", "R", Tool::Redact, state),
];

#[cfg(feature = "ocr")]
{
    tools = tools.push(tool_button("OCR", "OCR Text", "O", Tool::OcrText, state));
}

tools = tools.push(button(text("Smart Redaction")).on_press(Message::SmartRedaction));
```

Then append the existing undo/redo/navigator/copy/save/reveal controls to `tools`.

Also update keyboard tool shortcuts in `map_key_press`:

```rust
#[cfg(feature = "ocr")]
"o" => Some(Message::SelectTool(Tool::OcrText)),
```

- [ ] **Step 4: Stack `OcrTextLayer` above annotation canvas only in OCR Text mode**

In `canvas_view`, after `overlay`, build:

```rust
let layered = iced::widget::stack![img, overlay];
```

as:

```rust
let layered = iced::widget::stack![img, overlay];

#[cfg(feature = "ocr")]
let layered = {
    if state.editor.tool == Tool::OcrText {
        let visible = super::canvas::visible_image_rect(
            state.viewport.scroll_offset,
            state.viewport_bounds,
            geometry.scale,
            geometry.image_origin,
        );
        let ocr_layer = super::ocr_layer::ocr_text_layer(
            state.ocr_text.document(),
            state.ocr_text.selection().copied(),
            geometry.scale,
            visible,
            geometry.rendered_size,
        );
        iced::widget::stack![layered, ocr_layer]
    } else {
        layered
    }
};
```

Keep the existing inline text editor stack above this `layered` value. The final z-order must remain:

```text
image -> annotation canvas -> OCR Text layer (only in OCR Text mode) -> inline text editor
```

- [ ] **Step 5: Add contextual copy-all action in status bar**

In `status_bar`, build the row into a mutable row and push the OCR action only in OCR mode:

```rust
let mut status = row![
    text(dims),
    text(zoom_label).width(Length::Fill),
    button(text("Fit Width")).on_press(Message::SetZoom(ZoomMode::FitWidth)),
    button(text("Fit Window")).on_press(Message::SetZoom(ZoomMode::FitWindow)),
    button(text("Fit Height")).on_press(Message::SetZoom(ZoomMode::FitHeight)),
    button(text("100%")).on_press(Message::SetZoom(ZoomMode::ActualSize)),
    button(text("-")).on_press(Message::ZoomStep(ZoomDirection::Out)),
    button(text("+")).on_press(Message::ZoomStep(ZoomDirection::In)),
];

#[cfg(feature = "ocr")]
if state.editor.tool == Tool::OcrText {
    status = status.push(button(text("Copy all OCR text")).on_press(Message::CopyAllOcrText));
}

status.spacing(8).align_y(Alignment::Center).into()
```

- [ ] **Step 6: Refresh redaction mask after document edits**

In `update.rs`, after `refresh_navigator(state);` in `update`, add:

```rust
#[cfg(feature = "ocr")]
refresh_ocr_redaction_mask(state);
```

Add helper:

```rust
#[cfg(feature = "ocr")]
fn refresh_ocr_redaction_mask(state: &mut super::ResultWorkspace) {
    let redactions = redactions(&state.document.image);
    state.ocr_text.refresh_redactions(&redactions);
}
```

This relies on the `raw_items` and `redaction_signature` fields added to `OcrTextState` in Task 4. The refresh method must not rebuild the visible OCR document unless the redaction signature changed.

- [ ] **Step 7: Run UI compile checks**

Run:

```bash
rtk cargo check -p rollshot-app
rtk cargo check -p rollshot-app --features ocr
rtk cargo test -p rollshot-app viewport
```

Expected: all pass.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/view.rs crates/rollshot-app/src/result_workspace/viewport.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/ocr_text.rs
rtk git commit -m "feat(app): wire OCR text tool into workspace"
```

---

### Task 7: Tighten Redaction Safety, Empty States, And Privacy

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-vision/src/host.rs`

- [ ] **Step 1: Add redaction safety regression tests**

In `ocr_text.rs`, add:

```rust
#[test]
fn redacted_item_cannot_be_selected_after_initial_filtering() {
    let items = vec![
        item(1, "public", rect(0.0, 0.0, 60.0, 12.0)),
        item(2, "private", rect(0.0, 30.0, 70.0, 12.0)),
    ];
    let redactions = vec![Annotation::OpaqueRedaction {
        id: AnnotationId(9),
        bounds: rect(0.0, 25.0, 90.0, 25.0),
    }];
    let doc = OcrTextDocument::from_items(items, &redactions);

    assert_eq!(doc.copy_all_text(), "public");
    assert!(doc.visible_items().iter().all(|item| item.text != "private"));
}

#[test]
fn redaction_refresh_removes_stale_selection() {
    let items = vec![
        item(1, "public", rect(0.0, 0.0, 60.0, 12.0)),
        item(2, "private", rect(0.0, 30.0, 70.0, 12.0)),
    ];
    let mut state = OcrTextState::idle();
    state.finish_prepare(items, &[]);
    state.set_selection(Some(OcrSelection::range(
        TextCursor::new(1, 0),
        TextCursor::new(1, 7),
    )));

    let redactions = vec![Annotation::OpaqueRedaction {
        id: AnnotationId(9),
        bounds: rect(0.0, 25.0, 90.0, 25.0),
    }];
    state.refresh_redactions(&redactions);

    assert!(state.selection().is_none());
    assert_eq!(state.document().unwrap().copy_all_text(), "public");
}
```

In `update.rs`, add:

```rust
#[cfg(feature = "ocr")]
#[test]
fn copy_without_ocr_selection_shows_error() {
    let mut state = workspace();
    state.editor.tool = Tool::OcrText;
    state.ocr_text.set_ready_for_tests(vec![]);

    let _ = update(&mut state, Message::CopyOcrSelection);

    assert_eq!(
        state.message.as_ref().map(InlineMessage::text),
        Some("No OCR text selected")
    );
}
```

- [ ] **Step 2: Run tests and verify failures**

Run:

```bash
rtk cargo test -p rollshot-app --features ocr redacted_item_cannot_be_selected_after_initial_filtering
rtk cargo test -p rollshot-app --features ocr redaction_refresh_removes_stale_selection
rtk cargo test -p rollshot-app --features ocr copy_without_ocr_selection_shows_error
```

Expected:

- `redacted_item_cannot_be_selected_after_initial_filtering` fails until redaction-aware filtering excludes covered OCR items.
- `redaction_refresh_removes_stale_selection` fails until redaction-aware filtering refreshes after redaction changes.
- `copy_without_ocr_selection_shows_error` fails until empty OCR selection copy is routed to the inline error.

- [ ] **Step 3: Ensure OCR state Debug does not print raw text**

In `ocr_text.rs`, remove `#[derive(Debug)]` from `OcrTextItem`, `OcrTextDocument`, `OcrTextStatus`, and `OcrTextState` if present, and implement:

```rust
impl std::fmt::Debug for OcrTextItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrTextItem")
            .field("id", &self.id)
            .field("confidence", &self.confidence)
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for OcrTextDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrTextDocument")
            .field("visible_item_count", &self.visible_items.len())
            .finish()
    }
}

impl std::fmt::Debug for OcrTextStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OcrTextStatus::Idle => "Idle",
            OcrTextStatus::Preparing => "Preparing",
            OcrTextStatus::Ready(_) => "Ready",
            OcrTextStatus::Failed(_) => "Failed",
        })
    }
}

impl std::fmt::Debug for OcrTextState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrTextState")
            .field("status", &self.status_name())
            .field("has_selection", &self.selection.is_some())
            .field("raw_item_count", &self.raw_items.len())
            .finish()
    }
}
```

Add:

```rust
impl OcrTextState {
    fn status_name(&self) -> &'static str {
        match &self.status {
            OcrTextStatus::Idle => "idle",
            OcrTextStatus::Preparing => "preparing",
            OcrTextStatus::Ready(_) => "ready",
            OcrTextStatus::Failed(_) => "failed",
        }
    }
}
```

- [ ] **Step 4: Audit tracing for raw OCR text**

Run:

```bash
rtk rg -n "ocr.*text|text.*ocr|m\\.text|item\\.text|selected_text" crates/rollshot-app/src crates/rollshot-vision/src crates/rollshot-agent/src
```

Expected: raw OCR text appears only in data mapping, tests, selected/copy formatting, or clipboard write paths. No `tracing::*` event includes raw OCR text.

- [ ] **Step 5: Run focused safety tests**

Run:

```bash
rtk cargo test -p rollshot-app --features ocr ocr_text
rtk cargo test -p rollshot-app --features ocr redaction_refresh_removes_stale_selection
rtk cargo test -p rollshot-app --features ocr copy_without_ocr_selection_shows_error
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/ocr_text.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/view.rs crates/rollshot-vision/src/host.rs
rtk git commit -m "fix(app): enforce redaction-safe OCR text"
```

---

### Task 8: Full Verification

**Files:**
- No source edits unless verification exposes a defect.

- [ ] **Step 1: Format**

Run:

```bash
rtk cargo fmt --check
```

Expected: command exits 0.

- [ ] **Step 2: Default app/workspace tests**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-automation
rtk cargo test -p rollshot-vision
```

Expected: command exits 0.

- [ ] **Step 3: OCR feature tests**

Run:

```bash
rtk cargo test -p rollshot-ocr
rtk cargo test -p rollshot-vision --features ocr
rtk cargo test -p rollshot-app --features ocr ocr_text
```

Expected: command exits 0.

- [ ] **Step 4: Clippy**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: command exits 0. If `rollshot-ocr` dependency setup makes full-workspace clippy unsuitable on the local machine, run:

```bash
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk cargo clippy -p rollshot-app --features ocr --all-targets -- -D warnings
rtk cargo clippy -p rollshot-vision --features ocr --all-targets -- -D warnings
```

Expected: command exits 0.

- [ ] **Step 5: Manual UI verification**

Run the app with OCR enabled:

```bash
rtk cargo run -p rollshot-app --features ocr -- open crates/rollshot-app/tests/eval/fixtures/account_ids/image.png
```

Expected:

- OCR Text tool appears.
- Selecting OCR Text starts OCR.
- Annotation tools do not react while OCR Text is active.
- Dragging over recognized text shows highlight.
- Ctrl/Cmd+C copies selected OCR text.
- Ctrl/Cmd+C with no selection shows "No OCR text selected".
- Ctrl/Cmd+A selects all unredacted OCR text.
- Escape clears OCR selection first; pressing Escape again leaves OCR Text mode and returns to Select.
- "Copy all OCR text" copies all unredacted OCR text.
- Adding a redaction over OCR text removes that text from overlay, selection, and copy.
- Long screenshots have clean selectable text across tile boundaries.

- [ ] **Step 6: Stop on verification fixes**

If verification required source changes, stop before committing and inspect the exact diff:

```bash
rtk git status --short
rtk git diff
```

Expected: use the diff to add a new focused repair task to this plan, then execute and commit that repair as its own task. If no changes were required, do not create an empty commit.
