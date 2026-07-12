# Editor And Style Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Slice 1 of the annotation-editor program: typed Number/Text styles, canonical construction and history-safe property edits, persisted next-object defaults, and the approved responsive two-row Result Workspace toolbar while preserving fully opaque Redaction.

**Architecture:** `rollshot-image-document` remains the source of truth for committed styles, validation, history, render commands, flattening, hit bounds, and document-local number sequencing. `rollshot-app` owns persisted tool defaults and transient property transactions, with focused `annotation_defaults.rs`, `properties.rs`, and `toolbar.rs` modules feeding completed typed edits into the document. Both the live iced canvas and raster flattener continue consuming the same framework-neutral `RenderShape` values.

**Tech Stack:** Rust, `image`, `cosmic-text`, serde/TOML, iced 0.14 built-in widgets (`responsive`, `Canvas`, `stack`, `tooltip`), existing snapshot history and Result Workspace Elm architecture.

**Source specifications:**

- Umbrella: `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md`
- Approved slice spec: `docs/superpowers/specs/2026-07-12-editor-and-style-foundation-design.md`

## Global Constraints

- The source image is immutable; geometry and style values are full-resolution image-space values.
- Text sizes are exactly 14, 18, 24, and 32 image pixels; 18 is canonical.
- Number Small, Medium, and Large scale current reviewed geometry by exactly 0.75, 1.0, and 1.3; Medium preserves current appearance.
- Enabled Text backgrounds use selected RGB at fixed 85 percent alpha; the UI exposes no alpha control.
- Opaque Redaction has no style API and always replaces covered pixels with fully opaque black.
- One completed gesture or property edit creates at most one history entry; previews and cancelled edits create none.
- Tool-default edits never mutate selected annotations, and selected-annotation edits never mutate tool defaults.
- Copy and Save flatten full-resolution committed state only; drafts, menus, previews, selection, and handles are excluded.
- Result Workspace behavior is shared on Linux and macOS; runtime verification is required on both before Slice 1 can be marked Complete.
- Use iced 0.14 built-ins and Canvas; do not introduce a custom `Widget` or custom `Overlay` for this slice.
- Do not add future-tool variants, fields, toolbar placeholders, or any `rollshot-core` stitching change.
- Every shell command in this repository is prefixed with `rtk`.

## Engineering Review Record

### Step 0 — Scope challenge

The nine-task draft had three new files and no new crate or top-level module, so the complexity gate did not trigger. Every task contributes directly to the approved Slice 1 vertical contract. The minimum viable plan is the complete slice: omitting document styles, compatibility migration, defaults, transactions, toolbar, or verification would leave a toolbar-only or model-only partial feature that the umbrella explicitly rejects. The review therefore keeps the approved scope and removes no required behavior.

The search check used the repository's pinned iced 0.14 skill references rather than the open web: `responsive`, Canvas, `stack`, `float`, tooltip, and standard inputs cover the proposed UI; custom `Widget` and custom `Overlay` remain unnecessary and are known footguns for widget-tree and overlay-contract stability.

### Auto decisions

#### Auto decision D1 — How should the breaking `Annotation` migration land?

Context: Adding fields to Rust enum variants breaks every constructor immediately.

ELI10: If style fields land before all callers are updated, the repository cannot compile between tasks. A tired engineer cannot test or safely review a red commit.

Stakes if we pick wrong: Execution produces non-buildable intermediate commits and hides real regressions behind expected compiler failures.

Recommendation: **D1A — Keep Task 1 additive, then make storage/render/edit/consumer migration one atomic Task 2** because explicit, green commits are more valuable than artificially small broken commits.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

- **D1A — Atomic workspace migration (recommended; human: ~2 days / AI: ~45 min).** ✅ Every commit builds and can be reverted; ❌ Task 2 is necessarily broader.
- **D1B — Stagger enum changes across commits (human: ~1.5 days / AI: ~35 min).** ✅ Smaller textual commits; ❌ intermediate commits cannot compile.

Net: Accept one larger semantic commit to preserve the repository's executable history.

#### Auto decision D2 — How should test commands select multiple modules?

Context: Cargo accepts one positional test-name filter, but the draft supplied two or three.

ELI10: Those commands fail before running tests, so an executor could mistake command-line misuse for a red test. Chaining valid invocations makes each signal unambiguous.

Stakes if we pick wrong: TDD checkpoints are unusable and failures are misdiagnosed.

Recommendation: **D2A — Chain one valid filtered invocation per module or run the whole crate** because explicit Run/Expected pairs are non-negotiable.

Completeness: D2A=10/10, D2B=2/10.

Pros / cons:

- **D2A — Valid chained/full-crate commands (recommended; human: ~10 min / AI: ~2 min).** ✅ Commands execute exactly as written; ❌ some checkpoints run a few extra tests.
- **D2B — Keep invalid multi-filter commands.** ✅ No editing effort; ❌ no checkpoint can run successfully.

Net: Prefer slightly broader valid tests over concise invalid commands.

#### Auto decision D3 — How should shared `config.toml` writes and warnings behave?

Context: Annotation defaults share a file with daemon and provider settings.

ELI10: A direct write can truncate the whole settings file if the process or disk fails halfway through. Treating a recoverable default-save problem as an error also makes harmless editing failures look catastrophic.

Stakes if we pick wrong: Users can lose unrelated settings or see misleading failure severity.

Recommendation: **D3A — Preserve the TOML table, write/sync/rename a sibling file, and add `InlineMessage::Warning`** because the blast radius includes every Rollshot setting.

Completeness: D3A=10/10, D3B=6/10.

Pros / cons:

- **D3A — Atomic replacement plus warning semantics (recommended; human: ~1 day / AI: ~25 min).** ✅ Existing bytes survive failed writes and severity is honest; ❌ adds a small file helper and tests.
- **D3B — Direct write and reuse `Error`.** ✅ Minimal code; ❌ partial writes can clobber unrelated configuration and warnings look fatal.

Net: Spend a small amount of boring file-I/O code to contain a high-blast-radius failure.

#### Auto decision D4 — How should preview and popup state cross the view boundary?

Context: The draft asked Canvas to derive previews from `ResultWorkspace`, but `AnnotationCanvas` only receives document/editor references; Copy, More, and color popups could also overlap.

ELI10: Hidden access creates borrow and ownership confusion. Multiple booleans can make two menus open at once and stack invisible click-catchers over the canvas.

Stakes if we pick wrong: Preview code does not fit the actual API or popups steal input unpredictably.

Recommendation: **D4A — Build one preview clone in `canvas_view`, pass it explicitly, and use one `Popup` enum** because explicit data flow and single ownership are easier to reason about.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

- **D4A — Explicit preview input plus exclusive popup enum (recommended; human: ~0.5 day / AI: ~15 min).** ✅ Borrowing and event ownership are visible; ❌ adds one field and enum.
- **D4B — Let Canvas infer state and keep independent booleans.** ✅ Fewer type declarations; ❌ requires unavailable state and permits overlapping interaction layers.

Net: One extra explicit value removes two classes of UI-state ambiguity.

#### Auto decision D5 — Which missing tests are required before execution?

Context: The draft covered high-level property behavior but not picker edge math, failed atomic writes, or style edits at tall-image coordinates.

ELI10: UI sliders fail at edges, file writes fail at inconvenient times, and long screenshots expose coordinate bugs ordinary screenshots hide. These are cheap deterministic tests now and expensive bugs later.

Stakes if we pick wrong: Boundary input, configuration safety, or long-image output can regress silently.

Recommendation: **D5A — Add deterministic negative and boundary tests in the owning tasks** because AI-assisted completeness is inexpensive and well-tested code is non-negotiable.

Completeness: D5A=10/10, D5B=7/10.

Pros / cons:

- **D5A — Add all three test groups (recommended; human: ~1 day / AI: ~20 min).** ✅ Covers realistic failures without GUI timing; ❌ modestly lengthens test code.
- **D5B — Rely on manual runtime checks.** ✅ Less test code; ❌ failures become platform-dependent and harder to reproduce.

Net: Automate deterministic correctness and reserve manual checks for actual platform integration.

#### Auto decision D6 — Should toolbar and properties be split into separate tasks?

Context: Both modify `toolbar.rs`, `properties.rs`, `view.rs`, and the same popup/message routing.

ELI10: Splitting work that changes the same state machine creates merge conflicts and temporary UI states that are not useful. Keeping one vertical UI task is larger but reviewable as one behavior.

Stakes if we pick wrong: Parallel-looking tasks conflict heavily and neither produces a coherent editor row alone.

Recommendation: **D6A — Keep one UI task with smaller internal red/green steps** because files that change together should live and land together.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

- **D6A — One cohesive UI task (recommended; human: ~3 days / AI: ~60 min).** ✅ One state model and one usable deliverable; ❌ requires a careful reviewer checkpoint.
- **D6B — Split toolbar and picker tasks.** ✅ Smaller headings; ❌ both touch the same modules and create an incomplete intermediate UI.

Net: Keep the cohesive task but preserve its fine-grained test checkpoints.

### What already exists

- `ImageDocument` already owns immutable pixels, snapshots, exact state IDs, undo/redo, number sequencing, typed `EditOp`, flattening, hit testing, and Navigator anchors; the plan extends these paths rather than replacing them.
- `RenderShape` already feeds both iced Canvas and raster flattening; style values flow through it rather than creating a second renderer.
- Result Workspace already owns tools, drafts, selection, keyboard routing, inline messages, Copy/Save, and dirty state; the plan adds focused state without a new app architecture.
- `provider_config.rs` already demonstrates section-preserving TOML mutation; annotation defaults reuse the table-preservation pattern and strengthen the final write.
- iced 0.14 already provides responsive layout, Canvas, tooltips, rows, inputs, stack, and float; the plan uses those built-ins.

### Test flow diagram

```text
style values ──unit──> exact size/color mappings
      │
      v
annotation + EditOp ──unit──> validation/history/IDs/sequence
      │
      ├──> RenderShape ──unit──> bounds/hit/colors/text layout
      │                         │
      │                         v
      │                    flatten pixels
      v
compatibility consumers ──workspace compile + crate tests
      │
      v
defaults store ──tempdir integration──> preserve/write/failure
      │
      v
property state ──unit──> target/preview/apply/cancel/popup ownership
      │
      v
toolbar model + Canvas ──unit/integration──> routing/coordinates/output isolation
      │
      v
Linux + macOS smoke verification
```

### Test coverage table

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| 1 / exact color and size values | ✓ | — | — | no |
| 2 / constructors, typed edits, validation, history, sequence | ✓ | ✓ | — | no |
| 2 / shared live/flatten style semantics and Redaction opacity | ✓ | ✓ | — | no |
| 2 / automation, workbench, timeline, and OCR compatibility | ✓ | ✓ | — | no |
| 3 / defaults load, sibling preservation, atomic failure | ✓ | ✓ | — | no |
| 4 / property target, preview, Apply/Cancel, popup exclusivity | ✓ | ✓ | — | no |
| 5 / creation defaults, preview isolation, dirty state | ✓ | ✓ | — | no |
| 6 / responsive routing, picker math, tooltip metadata | ✓ | ✓ | Linux/macOS | visual polish only |
| 7 / keyboard precedence, Copy/Save isolation, tall coordinates | ✓ | ✓ | Linux/macOS | native clipboard/dialog UX only |

### Failure modes

| New codepath | Production failure | Test | Error handling / user result |
|---|---|---|---|
| Typed document edit | Wrong annotation kind or invalid next number | Task 2 Phase B Step 1 | `EditError::{WrongKind,InvalidNextNumber}`; inline error, no mutation |
| Style rendering | Optional Text plate or scaled Number bounds diverge | Task 2 Step 1 | Deterministic shared helpers; test failure prevents landing |
| Defaults load | Missing, malformed, or partially invalid TOML | Task 3 Step 1 | Canonical fallback plus one visible warning |
| Defaults save | Permission/disk/rename failure | Task 3 Steps 1–3 | Original bytes retained, in-memory value retained, one warning |
| Color transaction | Invalid hex or cancelled/outside close | Task 4 Steps 1–4 | Apply disabled or transaction cancelled; no history |
| Popup routing | Copy, More, and picker compete for input | Task 4 Step 1 and Task 6 Step 6 | Exclusive `Popup` enum; deterministic close/cancel |
| Canvas preview | Preview leaks into flatten or stale selected ID | Task 5 Steps 1–5 | Explicit one-annotation preview; output uses committed document |
| Responsive toolbar | Active tool moves to More at narrow width | Task 6 Steps 1–7 | More names and styles the active tool |
| Keyboard routing | Backspace/shortcut steals focused input | Task 7 Steps 1–3 | Focus gate and local-first `Esc`; no silent deletion |
| Platform integration | Native clipboard/dialog or layout differs | Task 7 Steps 5–6 | Runtime evidence; Handoff instead of false completion |

No failure mode is both silent and uncovered.

### Performance and resources

No new capture, stitch, pixel-conversion, streaming, or unbounded-buffer path is introduced. The only preview allocation is one selected `Annotation` clone while a color transaction is open; committed annotations continue viewport culling. Responsive toolbar modeling is constant-size work, picker Canvas geometry is tiny, config I/O occurs only on completed default edits, and full-resolution flatten remains limited to explicit output/tests. No cache or shader is justified for Slice 1.

### Task dependencies and execution strategy

| Task | Modules touched | Depends on |
|---|---|---|
| 1 | `rollshot-image-document` values | — |
| 2 | `rollshot-image-document`, Result Workspace, timeline workspace | 1 |
| 3 | Result Workspace config/state | 1, 2 |
| 4 | Result Workspace property/update state | 3 |
| 5 | Result Workspace Canvas/update/view | 4 |
| 6 | Result Workspace toolbar/properties/view/update | 5 |
| 7 | Result Workspace integration and docs | 6 |

Sequential execution, no parallelization opportunity. Tasks 2–7 share the Result Workspace or depend directly on its preceding state contracts; parallel agents would create merge conflicts and reason against stale interfaces. No task modifies the workspace root `Cargo.toml` or introduces a new artifact/distribution pipeline.

### NOT in scope

- Line, Arrow, Rectangle, Ellipse, Pen, Highlighter, and Pixelate — each belongs to a later gated slice.
- Capture Overlay, Action Guide annotation UX, and agent proposal UX — the approved product boundary is Result Workspace only.
- Editable project files or persisted annotation graphs — only next-object defaults persist.
- Alpha controls, font family, dash styles, arrow variants, corner radius, number formats, Blur, and plugin APIs — explicitly deferred by the umbrella.
- Custom iced `Widget`, custom `Overlay`, Shader, or picker cache — built-ins and a small Canvas are sufficient.
- Windows runtime behavior and new distribution artifacts — Rollshot's approved Slice 1 runtime gate is Linux and macOS and this slice adds no artifact.
- `rollshot-core` benchmarks — no stitching path changes.

---

### Task 1: Add color and style value types without changing annotations

**Files:**

- Modify: `crates/rollshot-image-document/src/geometry.rs`
- Modify: `crates/rollshot-image-document/src/style.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`

**Interfaces:**

- Produces: `Rgb8::new(u8, u8, u8)`, `Rgb8::opaque()`, and `Rgb8::with_alpha(u8)`.
- Produces: `NumberSize::{Small, Medium, Large}`, `NumberSize::scale() -> f32`, and `NumberStyle { accent: Rgb8, size: NumberSize }`.
- Produces: `TextSize::{Px14, Px18, Px24, Px32}`, `TextSize::pixels() -> f32`, and `TextStyle { font_size: TextSize, text_color: Rgb8, background: Option<Rgb8> }`.

- [ ] **Step 1: Write failing value tests**

Add tests that lock exact mappings and canonical styles:

```rust
#[test]
fn reviewed_size_mappings_are_exact() {
    assert_eq!(NumberSize::Small.scale(), 0.75);
    assert_eq!(NumberSize::Medium.scale(), 1.0);
    assert_eq!(NumberSize::Large.scale(), 1.3);
    assert_eq!(TextSize::ALL.map(TextSize::pixels), [14.0, 18.0, 24.0, 32.0]);
}

#[test]
fn canonical_styles_preserve_current_appearance() {
    assert_eq!(NumberStyle::default().accent, Rgb8::new(0xE5, 0x48, 0x4D));
    assert_eq!(NumberStyle::default().size, NumberSize::Medium);
    assert_eq!(TextStyle::default().font_size, TextSize::Px18);
    assert_eq!(TextStyle::default().text_color, Rgb8::new(0xFF, 0xFF, 0xFF));
    assert_eq!(TextStyle::default().background, Some(Rgb8::new(0x11, 0x11, 0x11)));
}

```

- [ ] **Step 2: Run the focused tests and confirm they fail**

Run: `rtk cargo test -p rollshot-image-document style::tests`

Expected: compilation fails because `Rgb8`, `NumberSize`, `TextSize`, and the style structs do not exist.

- [ ] **Step 3: Implement the minimal typed values and constructors**

Add these public contracts, deriving serde only when the crate feature is enabled:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rgb8 { pub r: u8, pub g: u8, pub b: u8 }

impl Rgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Self { Self { r, g, b } }
    pub const fn opaque(self) -> Rgba8 { Rgba8::new(self.r, self.g, self.b, 0xFF) }
    pub const fn with_alpha(self, alpha: u8) -> Rgba8 { Rgba8::new(self.r, self.g, self.b, alpha) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum NumberSize { Small, #[default] Medium, Large }

impl NumberSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];
    pub const fn scale(self) -> f32 { match self { Self::Small => 0.75, Self::Medium => 1.0, Self::Large => 1.3 } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextSize { Px14, #[default] Px18, Px24, Px32 }

impl TextSize {
    pub const ALL: [Self; 4] = [Self::Px14, Self::Px18, Self::Px24, Self::Px32];
    pub const fn pixels(self) -> f32 { match self { Self::Px14 => 14.0, Self::Px18 => 18.0, Self::Px24 => 24.0, Self::Px32 => 32.0 } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NumberStyle { pub accent: Rgb8, pub size: NumberSize }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextStyle { pub font_size: TextSize, pub text_color: Rgb8, pub background: Option<Rgb8> }
```

Implement explicit `Default` values for both style structs using the current style constants and re-export the new types from `lib.rs`. Do not change `Annotation` in this task; keeping the workspace green makes the later enum migration one explicit atomic change.

- [ ] **Step 4: Run focused tests and formatting**

Run: `rtk cargo test -p rollshot-image-document style::tests && rtk cargo fmt --check`

Expected: all focused tests pass and formatting reports no diff.

- [ ] **Step 5: Commit the style model**

```bash
rtk git add crates/rollshot-image-document/src/{geometry.rs,style.rs,lib.rs}
rtk git commit -m "feat(annotation): add typed number and text styles"
```

### Task 2: Atomically store styles, add typed edits, render them, and migrate consumers

**Files:**

- Modify: `crates/rollshot-image-document/src/shapes.rs`
- Modify: `crates/rollshot-image-document/src/hit.rs`
- Modify: `crates/rollshot-image-document/src/flatten.rs`
- Modify: `crates/rollshot-image-document/src/annotation.rs`
- Modify: `crates/rollshot-image-document/src/edit_op.rs`
- Modify: `crates/rollshot-image-document/src/document.rs`
- Modify: `crates/rollshot-image-document/src/callout_placement.rs`
- Modify: `crates/rollshot-image-document/src/navigator.rs`
- Modify: `crates/rollshot-image-document/tests/text_export.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**

- Consumes: `NumberStyle`, `TextStyle`, `NumberSize::scale()`, `TextSize::pixels()`, and `Rgb8` from Task 1.
- Produces: `Annotation::{number_callout,text_note,opaque_redaction}` canonical constructors, `Annotation::{number_callout_with_style,text_note_with_style}` explicit-style constructors, and typed style accessors.
- Produces: `number_label_px(label: &str, style: NumberStyle)`, `leader_triangle(tip, bubble, style)`, and `text_plate_rect(position, text, style)`.
- Preserves: existing `annotation_shapes`, `annotation_bounds`, `hit_test_annotation`, and `ImageDocument::flatten` public entry points.

- [ ] **Step 1: Write failing style-render contract tests**

Add tests proving geometry and colors flow through the shared render boundary:

```rust
#[test]
fn number_size_scales_render_shapes_and_bounds() {
    let small = number_with_style(NumberStyle { size: NumberSize::Small, ..Default::default() });
    let large = number_with_style(NumberStyle { size: NumberSize::Large, ..Default::default() });
    assert!(annotation_bounds(&large).width > annotation_bounds(&small).width);
    assert!(matches!(annotation_shapes(&large)[0], RenderShape::Circle { fill, .. } if fill == NumberStyle::default().accent.opaque()));
}

#[test]
fn text_style_controls_font_color_and_optional_fixed_alpha_plate() {
    let style = TextStyle { font_size: TextSize::Px32, text_color: Rgb8::new(1, 2, 3), background: Some(Rgb8::new(4, 5, 6)) };
    let shapes = annotation_shapes(&text_with_style(style));
    assert!(matches!(shapes[0], RenderShape::Rect { color, .. } if color == Rgba8::new(4, 5, 6, 217)));
    assert!(matches!(shapes[1], RenderShape::Label { px: 32.0, color, .. } if color == Rgba8::new(1, 2, 3, 255)));
}

#[test]
fn text_without_background_emits_only_the_label() {
    let style = TextStyle { background: None, ..Default::default() };
    assert_eq!(annotation_shapes(&text_with_style(style)).len(), 1);
}
```

Add a raster test comparing representative pixels from live `RenderShape` colors with flattened output and retain the existing fully opaque Redaction pixel assertion.

- [ ] **Step 2: Run the render tests and confirm failure**

Run: `rtk cargo test -p rollshot-image-document`

Expected: failures show fixed global constants still drive Number/Text shapes and bounds.

- [ ] **Step 3: Thread styles through shared geometry**

Add `style: NumberStyle` and `style: TextStyle` to the committed variants. Implement canonical constructors that inject `Default` and explicit-style constructors used by `ImageDocument`:

```rust
pub fn number_callout(id: AnnotationId, number: u32, tip: ImagePoint, bubble: ImagePoint) -> Self {
    Self::number_callout_with_style(id, number, tip, bubble, NumberStyle::default())
}

pub fn number_callout_with_style(id: AnnotationId, number: u32, tip: ImagePoint, bubble: ImagePoint, style: NumberStyle) -> Self {
    Self::NumberCallout { id, number, tip, bubble, style }
}
```

Provide the equivalent canonical/explicit Text constructors, a style-free Redaction constructor, and typed accessors. Then replace fixed-size helpers with style-aware forms:

```rust
const TEXT_BACKGROUND_ALPHA: u8 = 217;

pub fn text_plate_rect(position: ImagePoint, text: &str, style: TextStyle) -> ImageRect {
    let px = style.font_size.pixels();
    let (width, height) = measure_block(text, px, false);
    ImageRect { x: position.x, y: position.y, width: width + TEXT_NOTE_PLATE_PADDING * 2.0, height: height + TEXT_NOTE_PLATE_PADDING * 2.0 }
}

fn number_radius(style: NumberStyle) -> f32 {
    NUMBER_BUBBLE_RADIUS * style.size.scale()
}
```

Use the same scale for bubble radius, outline width, label size/minimum, leader base, and leader half-width. Build Text shapes with an optional plate followed by the label. Make bounds and hit testing call the same helpers used by rendering; do not duplicate style math in `hit.rs`.

- [ ] **Step 4: Run document rendering and export tests**

Run: `rtk cargo test -p rollshot-image-document`

Expected: all crate tests pass, including exact Redaction opacity and styled Text export.

- [ ] **Step 5: Continue directly to Phase B; do not commit a broken workspace**

Expected: the image-document crate is green, while the workspace still identifies external construction sites that Phase C must migrate before the single Task 2 commit.

#### Phase B: Add typed document edits and exact sequence history

**Files:**

- Modify: `crates/rollshot-image-document/src/edit_op.rs`
- Modify: `crates/rollshot-image-document/src/document.rs`

**Interfaces:**

- Consumes: Task 1 style types and constructors.
- Produces: styled `EditOp::AddNumberCallout`, styled `EditOp::AddTextNote`, `EditOp::UpdateNumberStyle`, `EditOp::UpdateTextStyle`, and `EditOp::SetNextNumber`.
- Produces: `ImageDocument::{add_number_callout_with_style,add_text_note_with_style,set_number_style,set_text_style,set_next_number}` while retaining canonical-style convenience add methods.
- Produces: `EditError::InvalidNextNumber`; all failed operations are atomic.

- [ ] **Step 1: Write failing typed-edit and history tests**

Add focused tests:

```rust
#[test]
fn style_edit_retains_id_and_is_one_undo_entry() {
    let mut doc = doc();
    let id = doc.add_number_callout(point(5.0), point(5.0));
    let before = doc.state_id();
    let style = NumberStyle { accent: Rgb8::new(1, 2, 3), size: NumberSize::Large };
    doc.set_number_style(id, style).unwrap();
    assert_eq!(doc.annotation(id).unwrap().number_style(), Some(style));
    assert_ne!(doc.state_id(), before);
    assert!(doc.undo());
    assert_eq!(doc.annotation(id).unwrap().number_style(), Some(NumberStyle::default()));
}

#[test]
fn next_number_is_document_local_validated_and_restored_exactly() {
    let mut doc = doc();
    assert_eq!(doc.set_next_number(0), Err(EditError::InvalidNextNumber));
    assert_eq!(doc.next_number(), 1);
    doc.set_next_number(7).unwrap();
    let id = doc.add_number_callout(point(1.0), point(1.0));
    assert!(matches!(doc.annotation(id), Some(Annotation::NumberCallout { number: 7, .. })));
    assert!(doc.undo());
    assert_eq!(doc.next_number(), 7);
    assert!(doc.undo());
    assert_eq!(doc.next_number(), 1);
}

#[test]
fn wrong_kind_style_edit_is_atomic() {
    let mut doc = doc();
    let id = doc.add_redaction(rect()).unwrap();
    let state = doc.state_id();
    assert_eq!(doc.set_text_style(id, TextStyle::default()), Err(EditError::WrongKind));
    assert_eq!(doc.state_id(), state);
    assert!(!doc.can_redo());
}
```

Extend batch tests to prove a failed style/sequence operation restores graph, sequence, state ID, and redo state.

- [ ] **Step 2: Run tests and confirm missing operations**

Run: `rtk cargo test -p rollshot-image-document document::tests`

Expected: compilation fails on the new edit variants and methods.

- [ ] **Step 3: Implement typed edits through the existing snapshot path**

Add these variants:

```rust
pub enum EditOp {
    AddNumberCallout { tip: ImagePoint, bubble: ImagePoint, style: NumberStyle },
    AddTextNote { position: ImagePoint, text: String, style: TextStyle },
    UpdateNumberStyle { id: AnnotationId, style: NumberStyle },
    UpdateTextStyle { id: AnnotationId, style: TextStyle },
    SetNextNumber { value: u32 },
    // retain existing geometry, text, redaction, and delete variants
}
```

Route single-operation methods through the existing snapshot/commit rules. Return success without committing when a style or next-number value is unchanged. Validate nonzero next-number before snapshot mutation. Update `apply_batch` referenced-ID preflight and `apply_one`; `SetNextNumber` has no referenced ID.

- [ ] **Step 4: Run document tests and clippy for the crate**

Run: `rtk cargo test -p rollshot-image-document && rtk cargo clippy -p rollshot-image-document --all-targets -- -D warnings`

Expected: all document tests and clippy pass.

- [ ] **Step 5: Continue directly to Phase C; do not commit a broken workspace**

Expected: document tests pass; external app consumers are migrated next before committing.

#### Phase C: Migrate every existing annotation consumer

**Files:**

- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-image-document/src/callout_placement.rs`
- Modify: `crates/rollshot-image-document/src/navigator.rs`

**Interfaces:**

- Consumes: canonical constructors and styled add/edit operations from Tasks 1–3.
- Produces: no new public API; all established consumers compile with canonical styles.
- Preserves: automation proposal lowering, workbench ghosts, OCR redactions, timeline annotations, draft rendering, and existing Result Workspace behavior.

- [ ] **Step 1: Convert one representative consumer test to assert canonical styles**

In Result Workspace and timeline tests, add assertions such as:

```rust
match annotation {
    Annotation::NumberCallout { style, .. } => assert_eq!(*style, NumberStyle::default()),
    other => panic!("expected number callout, got {other:?}"),
}
```

Add equivalent Text assertions and keep the Redaction match style-free.

- [ ] **Step 2: Run workspace check to enumerate every broken literal**

Run: `rtk cargo check --workspace --all-targets`

Expected: compilation failures identify every Number/Text literal missing `style` and every styled `EditOp` add missing its style value.

- [ ] **Step 3: Migrate consumers surgically**

Use constructors for transient and ghost annotations:

```rust
Annotation::number_callout(id, number, tip, bubble)
Annotation::text_note(id, position, text)
Annotation::opaque_redaction(id, bounds)
```

Use explicit canonical styles in document operations:

```rust
EditOp::AddNumberCallout { tip, bubble, style: NumberStyle::default() }
EditOp::AddTextNote { position, text, style: TextStyle::default() }
```

Do not add style state to automation proposals, Action Guide payloads, OCR redactions, or workbench contracts in this slice.

- [ ] **Step 4: Verify all consumers and feature combinations**

Run: `rtk cargo test -p rollshot-image-document && rtk cargo test -p rollshot-app && rtk cargo test -p rollshot-app --features ocr && rtk cargo check --workspace --all-targets`

Expected: all commands pass with canonical behavior preserved.

- [ ] **Step 5: Commit the complete atomic workspace migration**

```bash
rtk git add crates/rollshot-image-document crates/rollshot-app/src/result_workspace/{canvas.rs,update.rs,ocr_text.rs} crates/rollshot-app/src/timeline_workspace/{annotation.rs,update.rs}
rtk git commit -m "feat(annotation): store and edit annotation styles"
```

### Task 3: Persist typed next-object defaults without clobbering config

**Files:**

- Create: `crates/rollshot-app/src/result_workspace/annotation_defaults.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/Cargo.toml`

**Interfaces:**

- Produces: `AnnotationDefaults { number: NumberStyle, text: TextStyle }`.
- Produces: `LoadedAnnotationDefaults { values: AnnotationDefaults, warnings: Vec<String> }`.
- Produces: `load_from(path: &Path) -> LoadedAnnotationDefaults` and `save_to(path: &Path, values: &AnnotationDefaults) -> Result<(), String>`.
- Produces: `AnnotationDefaultsState { values, config_path, warning_reported }` owned by `ResultWorkspace` and `InlineMessage::Warning(String)` for non-blocking persistence/load warnings.

- [ ] **Step 1: Enable serde for shared style values and write persistence tests**

Change the app dependency to:

```toml
rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }
```

Create tests for missing fields, malformed values, and preservation:

```rust
#[test]
fn missing_fields_use_canonical_defaults() {
    write_config("[annotation_defaults.number]\nsize = \"Large\"\n");
    let loaded = load_from(&path());
    assert_eq!(loaded.values.number.size, NumberSize::Large);
    assert_eq!(loaded.values.number.accent, NumberStyle::default().accent);
    assert_eq!(loaded.values.text, TextStyle::default());
}

#[test]
fn save_preserves_unrelated_and_unknown_sections() {
    write_config("[daemon]\ncapture_region_hotkey = \"Alt+Shift+6\"\n[future]\nvalue = 9\n");
    save_to(&path(), &AnnotationDefaults::default()).unwrap();
    let table = read_table();
    assert!(table.contains_key("daemon"));
    assert!(table.contains_key("future"));
    assert!(table.contains_key("annotation_defaults"));
}

#[test]
fn failed_atomic_write_leaves_existing_config_bytes_unchanged() {
    let original = b"[daemon]\ncapture_region_hotkey = \"Alt+Shift+6\"\n";
    write_bytes(original);
    let result = save_to_with_writer(&path(), &AnnotationDefaults::default(), |_path, _bytes| {
        Err("injected atomic write failure".to_string())
    });
    assert_eq!(result, Err("injected atomic write failure".to_string()));
    assert_eq!(std::fs::read(path()).unwrap(), original);
}

#[test]
fn invalid_field_falls_back_with_one_warning() {
    write_config("[annotation_defaults.text]\nfont_size = \"Px99\"\n");
    let loaded = load_from(&path());
    assert_eq!(loaded.values.text, TextStyle::default());
    assert_eq!(loaded.warnings.len(), 1);
}
```

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `rtk cargo test -p rollshot-app annotation_defaults::tests`

Expected: module and persistence types are missing.

- [ ] **Step 3: Implement section-preserving load/save**

Deserialize each tool section with field defaults rather than deserializing the entire file strictly:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AnnotationDefaults {
    pub number: NumberStyle,
    pub text: TextStyle,
}

pub fn save_to(path: &Path, values: &AnnotationDefaults) -> Result<(), String> {
    save_to_with_writer(path, values, write_temp_sync_rename)
}

fn save_to_with_writer(
    path: &Path,
    values: &AnnotationDefaults,
    writer: impl FnOnce(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut root = read_existing_table_or_empty(path)?;
    root.insert("annotation_defaults".into(), toml::Value::try_from(values).map_err(|e| format!("serialize annotation defaults: {e}"))?);
    let text = toml::to_string_pretty(&root).map_err(|e| format!("serialize config.toml: {e}"))?;
    let parent = path.parent().ok_or_else(|| "configuration path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create config directory: {e}"))?;
    writer(path, text.as_bytes())
}
```

`write_temp_sync_rename` writes a uniquely named sibling file, calls `sync_all`, then renames it over `config.toml`; every error removes the temporary file and leaves the original bytes unchanged. On malformed whole-file TOML, load canonical defaults with one `InlineMessage::Warning` and refuse to overwrite the malformed file. On per-field decode failure, retain valid sibling fields, default only the invalid field, and return one combined warning string.

- [ ] **Step 4: Wire defaults into workspace construction with test injection**

Add `ResultWorkspace::with_loaded_defaults(...)` for tests and make product `new(...)` call `config::config_path()` then `annotation_defaults::load_from`. Store warnings as `InlineMessage::Warning` without replacing a more important initial error; retain a `warning_reported` flag for later save failures.

- [ ] **Step 5: Run persistence and workspace-construction tests**

Run: `rtk cargo test -p rollshot-app annotation_defaults::tests && rtk cargo test -p rollshot-app result_workspace::tests`

Expected: all tests pass without reading or writing the developer's real config directory.

- [ ] **Step 6: Commit defaults persistence**

```bash
rtk git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/result_workspace/{annotation_defaults.rs,mod.rs}
rtk git commit -m "feat(annotation): persist editor style defaults"
```

### Task 4: Add property targets and transactional editor state

**Files:**

- Create: `crates/rollshot-app/src/result_workspace/properties.rs`
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**

- Produces: `PropertyTarget::{NumberTool,TextTool,Annotation(AnnotationId)}`.
- Produces: `ColorProperty::{NumberAccent,TextColor,TextBackground}`.
- Produces: `ColorTransaction { target, property, original: Rgb8, preview: Rgb8, hex: String }`.
- Produces: `PropertyState { color: Option<ColorTransaction>, next_number_input: String, popup: Option<Popup> }` inside `EditorState`, where `Popup::{CopyMenu,MoreMenu,ColorPicker}` guarantees one transient popup at a time.
- Produces: messages `SetNumberSize`, `SetTextSize`, `ToggleTextBackground`, `NextNumberInputChanged`, `CommitNextNumber`, `OpenColorPicker`, `PreviewColor`, `ColorHexChanged`, `ApplyColor`, and `CancelColor`.

- [ ] **Step 1: Write pure target and transaction tests**

```rust
#[test]
fn selected_annotation_wins_over_tool_defaults_only_in_select_mode() {
    let mut state = workspace();
    let id = add_text(&mut state);
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    assert_eq!(property_target(&state), Some(PropertyTarget::Annotation(id)));
    state.editor.tool = Tool::Text;
    assert_eq!(property_target(&state), Some(PropertyTarget::TextTool));
}

#[test]
fn cancel_color_restores_preview_without_history_or_default_save() {
    let mut state = workspace_with_selected_number();
    let before = state.document.image.state_id();
    update(&mut state, Message::OpenColorPicker(ColorProperty::NumberAccent));
    update(&mut state, Message::PreviewColor(Rgb8::new(1, 2, 3)));
    update(&mut state, Message::CancelColor);
    assert_eq!(state.document.image.state_id(), before);
    assert!(state.editor.properties.color.is_none());
}
```

Add tests that creation-tool targets ignore selection, Redaction yields no property target, invalid hex cannot Apply, and opening a transaction snapshots the current selected/default color.

- [ ] **Step 2: Run focused update tests and confirm failure**

Run: `rtk cargo test -p rollshot-app result_workspace::update::tests::cancel_color -- --nocapture`

Expected: missing property module, state, and messages.

- [ ] **Step 3: Implement pure property helpers and state transitions**

Use one helper for the context boundary:

```rust
pub fn property_target(state: &ResultWorkspace) -> Option<PropertyTarget> {
    match state.editor.tool {
        Tool::Number => Some(PropertyTarget::NumberTool),
        Tool::Text => Some(PropertyTarget::TextTool),
        Tool::Select => state.editor.selection.and_then(|id| match state.document.image.annotation(id) {
            Some(Annotation::NumberCallout { .. } | Annotation::TextNote { .. }) => Some(PropertyTarget::Annotation(id)),
            _ => None,
        }),
        Tool::Redact => None,
        #[cfg(feature = "ocr")]
        Tool::OcrText => None,
    }
}
```

Keep preview values in `PropertyState`; do not clone or mutate the document for preview. Parse hex only through `parse_hex_rgb(&str) -> Result<Rgb8, &'static str>` and normalize successful input to uppercase `#RRGGBB` on Apply.

- [ ] **Step 4: Implement completed property commits**

For annotation targets, call exactly one `set_number_style` or `set_text_style`. For tool targets, update one in-memory default and call `save_to`; on failure retain the in-memory value and report only the first failure per workspace session. `CommitNextNumber` calls `ImageDocument::set_next_number` and surfaces `EditError` through `InlineMessage::Error`.

- [ ] **Step 5: Run property, history, and persistence tests**

Run: `rtk cargo test -p rollshot-app result_workspace::properties::tests && rtk cargo test -p rollshot-app result_workspace::update::tests`

Expected: property target, Apply/Cancel, one-entry history, no-op, and persistence-warning tests pass.

- [ ] **Step 6: Commit transactional property state**

```bash
rtk git add crates/rollshot-app/src/result_workspace/{properties.rs,canvas.rs,mod.rs,update.rs}
rtk git commit -m "feat(annotation): add transactional property editing"
```

### Task 5: Use defaults for creation and styles for live canvas previews

**Files:**

- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`

**Interfaces:**

- Consumes: `AnnotationDefaultsState` from Task 3 and property previews from Task 4.
- Preserves: one document edit per creation/release and one per completed inline Text session.
- Produces: `AnnotationCanvas::property_preview: Option<Annotation>` carrying at most one app-only preview clone without entering document history or flatten output.

- [ ] **Step 1: Write creation-default and preview-isolation tests**

```rust
#[test]
fn number_creation_copies_current_tool_default() {
    let mut state = workspace();
    state.annotation_defaults.values.number.size = NumberSize::Large;
    update(&mut state, Message::SelectTool(Tool::Number));
    press_move_release(&mut state, point(10.0), point(10.0));
    assert!(matches!(state.document.image.annotations()[0], Annotation::NumberCallout { style: NumberStyle { size: NumberSize::Large, .. }, .. }));
}

#[test]
fn selected_color_preview_changes_canvas_shapes_not_flattened_document() {
    let mut state = workspace_with_selected_number();
    let before = state.document.image.flatten();
    update(&mut state, Message::OpenColorPicker(ColorProperty::NumberAccent));
    update(&mut state, Message::PreviewColor(Rgb8::new(0, 255, 0)));
    assert_ne!(preview_shapes(&state), annotation_shapes(state.document.image.annotations().first().unwrap()));
    assert_eq!(state.document.image.flatten(), before);
}
```

- [ ] **Step 2: Run gesture tests and confirm default styles are not copied yet**

Run: `rtk cargo test -p rollshot-app result_workspace::update::tests::number_creation_copies_current_tool_default`

Expected: test fails because creation still uses canonical fixed-style operations.

- [ ] **Step 3: Thread defaults into completed creation**

On Number release submit:

```rust
EditOp::AddNumberCallout {
    tip,
    bubble,
    style: state.annotation_defaults.values.number,
}
```

On Text draft commit submit its captured creation style, not the current global default at commit time. Extend `TextDraft` with `style: TextStyle` so changing defaults while a draft is open cannot restyle it unexpectedly.

- [ ] **Step 4: Render preview styles app-only**

Add `properties::preview_annotation(state) -> Option<Annotation>` that clones only the selected annotation and substitutes the transaction preview style. Build it in `canvas_view` and pass it as `AnnotationCanvas::property_preview`; in `AnnotationCanvas::draw`, use that clone only when its ID matches the committed annotation being drawn. Continue passing the committed document to flatten/Copy/Save.

- [ ] **Step 5: Run gestures, canvas, flatten, and dirty-state tests**

Run: `rtk cargo test -p rollshot-app result_workspace::canvas::tests && rtk cargo test -p rollshot-app result_workspace::update::tests && rtk cargo test -p rollshot-image-document`

Expected: creation/default and preview isolation tests pass; existing gesture/history tests remain green.

- [ ] **Step 6: Commit creation and preview integration**

```bash
rtk git add crates/rollshot-app/src/result_workspace/{canvas.rs,update.rs,view.rs}
rtk git commit -m "feat(annotation): apply defaults to annotation creation"
```

### Task 6: Build the two-row responsive toolbar and property controls

**Files:**

- Create: `crates/rollshot-app/src/result_workspace/toolbar.rs`
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**

- Produces: `toolbar::view(state: &ResultWorkspace) -> Element<'_, Message>`.
- Produces: pure `ToolbarDensity::{Wide, Compact, Narrow}` and `density_for_width(f32)` used by tests and `responsive`.
- Produces: `properties::view(state) -> Element<'_, Message>` and Canvas-backed saturation/value and hue controls publishing `PreviewColor`.
- Preserves: existing action messages and safety prompts for Smart Redaction, Navigator, Reveal, Export Bug Report, Copy, Save, and feature-gated OCR.

- [ ] **Step 1: Write toolbar routing tests before moving the view**

```rust
#[test]
fn copy_and_save_never_enter_overflow() {
    for width in [640.0, 800.0, 1100.0] {
        let model = toolbar_model(&state(), width);
        assert!(model.first_row.contains(&ToolbarItem::Copy));
        assert!(model.first_row.contains(&ToolbarItem::SaveAs));
        assert!(!model.more.contains(&ToolbarItem::Copy));
        assert!(!model.more.contains(&ToolbarItem::SaveAs));
    }
}

#[test]
fn narrow_priority_preserves_select_number_and_text() {
    let model = toolbar_model(&state(), 640.0);
    assert!(model.visible_tools.starts_with(&[Tool::Select, Tool::Number, Tool::Text]));
    assert!(model.more.contains(&ToolbarItem::Redact));
}

#[test]
fn active_overflow_tool_marks_more_active_and_names_it() {
    let mut state = state();
    state.editor.tool = Tool::Redact;
    let model = toolbar_model(&state, 640.0);
    assert_eq!(model.more_active_tool, Some((Tool::Redact, "Redact")));
}
```

Test that Select with no selection has no properties, selected Number/Text show only supported controls, Redaction shows none, and creation tools show defaults.

Add deterministic picker-math tests:

```rust
#[test]
fn saturation_value_and_hue_mapping_clamp_at_every_edge() {
    assert_eq!(sv_from_point(Point::new(-1.0, 121.0), Size::new(220.0, 120.0)), (0.0, 0.0));
    assert_eq!(sv_from_point(Point::new(220.0, 0.0), Size::new(220.0, 120.0)), (1.0, 1.0));
    assert_eq!(hue_from_x(-5.0, 220.0), 0.0);
    assert_eq!(hue_from_x(225.0, 220.0), 360.0);
}
```

- [ ] **Step 2: Run toolbar tests and confirm missing model**

Run: `rtk cargo test -p rollshot-app result_workspace::toolbar::tests`

Expected: module, model, and routing helpers are missing.

- [ ] **Step 3: Extract and build first-row chrome**

Move toolbar code out of `view.rs`. Build a fixed-height first row:

```rust
row![
    button(text("Close")).on_press(Message::RequestClose),
    text(state.document.display_name()).width(Length::Fill),
    undo_button(state),
    redo_button(state),
    vertical_rule(1),
    copy_split_button(state),
    button(text(save_label(state))).on_press(Message::SaveAs),
]
.height(40)
.align_y(Alignment::Center)
```

Keep title truncation/shrink behavior ahead of right-pinned actions and preserve existing secure-sharing labels.

- [ ] **Step 4: Build responsive second-row routing**

Use a constrained responsive widget:

```rust
responsive(move |size| second_row(state, toolbar_model(state, size.width)))
    .width(Length::Fill)
    .height(Length::Fixed(40.0))
```

Wide shows Select, Number, Text, Redact, properties, More. Narrow keeps Select, Number, and Text, moves Redact into More, uses compact property widgets, and still shows More. More always contains Smart Redaction, Navigator, Reveal, Export Bug Report, and feature-gated OCR; it reuses existing messages and availability gates.

- [ ] **Step 5: Build contextual controls and visual color picker**

Use built-in buttons/pick lists/text input plus Canvas programs:

```rust
canvas(SaturationValue { hue, selected: transaction.preview })
    .width(Length::Fixed(220.0))
    .height(Length::Fixed(120.0))
```

The picker includes palette swatches, saturation/value field, hue strip, `#RRGGBB` input, preview swatch, Apply, and Cancel. Layer it with `stack`/`float`; outside interaction publishes `CancelColor`. Do not implement a custom `Overlay`. Keep widget tree shape stable: always build the same wrapper and conditionally enable/present its floating child.

- [ ] **Step 6: Add pure toolbar-model and view-construction assertions**

Keep the existing `view.rs` construction tests and add pure routing/model assertions for message availability, picker validation, and tooltip metadata. Represent every tool entry as `ToolPresentation { tool, label, shortcut, visible }`; assert every visible entry has nonempty `label` and `shortcut`. Do not add an `iced_test` dependency in this slice.

- [ ] **Step 7: Run toolbar and view tests**

Run: `rtk cargo test -p rollshot-app result_workspace::toolbar::tests && rtk cargo test -p rollshot-app result_workspace::properties::tests && rtk cargo test -p rollshot-app result_workspace::view::tests`

Expected: responsive routing, contextual properties, More active state, and picker behavior tests pass.

- [ ] **Step 8: Commit the responsive editor chrome**

```bash
rtk git add crates/rollshot-app/src/result_workspace/{toolbar.rs,properties.rs,view.rs,mod.rs,update.rs}
rtk git commit -m "feat(annotation): add responsive editor toolbar"
```

### Task 7: Lock keyboard precedence, failure behavior, and full verification

**Files:**

- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs`
- Modify: `crates/rollshot-app/src/result_workspace/toolbar.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `README.md` only if user-facing shortcuts or toolbar instructions currently document the old layout
- Modify: `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md` during execution handoff/completion, not before verification

**Interfaces:**

- Consumes: all prior tasks.
- Produces: final Slice 1 keyboard precedence and non-blocking error behavior.
- Preserves: existing Copy/Save/Copy Original, close/dirty prompts, Navigator refresh, OCR focus, and workbench transitions.

- [ ] **Step 1: Write failing integrated keyboard and failure tests**

```rust
#[test]
fn escape_resolves_property_then_draft_then_selection_then_tool_then_close() {
    let mut state = workspace_with_all_local_states();
    press_escape(&mut state); assert!(state.editor.properties.color.is_none());
    press_escape(&mut state); assert!(state.editor.drag.is_none());
    press_escape(&mut state); assert!(state.editor.selection.is_none());
    press_escape(&mut state); assert_eq!(state.editor.tool, Tool::Select);
    press_escape(&mut state); assert!(state.pending_discard.is_some());
}

#[test]
fn focused_property_input_owns_delete_and_shortcut_keys() {
    let mut state = workspace_with_selected_text();
    state.editor.properties.focus = Some(PropertyFocus::HexInput);
    assert_eq!(keyboard_message(key_event("Backspace"), &state), None);
    assert!(state.document.image.annotation(state.editor.selection.unwrap()).is_some());
}

#[test]
fn failed_default_save_warns_once_and_keeps_memory_value() {
    let mut state = workspace_with_unwritable_defaults_path();
    apply_number_default(&mut state, NumberStyle { size: NumberSize::Large, ..Default::default() });
    let first = state.message.as_ref().unwrap().text().to_owned();
    apply_number_default(&mut state, NumberStyle { size: NumberSize::Small, ..Default::default() });
    assert_eq!(state.annotation_defaults.values.number.size, NumberSize::Small);
    assert_eq!(state.message.as_ref().unwrap().text(), first);
}
```

Add integrated tests for Undo/Redo cancelling preview before history, invalid next number leaving state unchanged, Navigator refresh only after commit, Copy/Save excluding previews, and Opaque Redaction remaining black/255 after mixed edits.

Add a tall-image test that uses a downscaled display, edits Number and Text styles near the bottom in full-resolution coordinates, then asserts live render bounds and flattened pixels remain anchored to the original image-space positions.

- [ ] **Step 2: Run integrated tests and confirm failures**

Run: `rtk cargo test -p rollshot-app result_workspace::update::tests`

Expected: new precedence and warning tests fail until routing is ordered explicitly.

- [ ] **Step 3: Implement exact precedence and error routing**

At the start of `EscapePressed`, call `cancel_color_transaction(state)` and return if it cancelled. Then preserve the spec order: annotation/text draft, selection, creation tool, existing close behavior. Before Undo/Redo, cancel property preview and commit/cancel text according to the existing text contract. Gate global shortcuts while `PropertyFocus` or inline Text editor owns keyboard input.

Convert every typed edit failure into `InlineMessage::Error(error.to_string())`; do not clear selection, defaults, paths, or dirty state. Keep only one defaults-persistence warning per workspace session.

- [ ] **Step 4: Run full automated verification**

Run:

```bash
rtk cargo test
rtk cargo test -p rollshot-app --features ocr
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: every command exits 0 with no warnings. Stitching benchmarks are not required because no `rollshot-core` stitching path changes.

- [ ] **Step 5: Perform Linux runtime verification and record evidence**

Run the normal Linux Result Workspace flow and verify: wide/narrow rows, More routing and active naming, tooltips/shortcuts, Number/Text defaults, selected edits, color Apply/Cancel, inline Text focus, undo/redo, Copy/Copy Original/Save As, dirty prompts, Navigator refresh, zoom/pan on a tall capture, and fully opaque Redaction output.

Record date, build commit, environment, and pass/fail notes in the umbrella registry's implementation/verification column when transitioning to Handoff or Complete.

- [ ] **Step 6: Perform macOS runtime verification and record evidence**

Run the same checklist through the active macOS `rollshot-app` product path, including native clipboard and file dialog behavior. Record date, build commit, macOS version, and pass/fail notes. If macOS is unavailable, transition the registry to Handoff with the unchecked risks and exact next command/path; do not mark Complete.

- [ ] **Step 7: Commit final integration fixes**

```bash
rtk git add crates/rollshot-app README.md
rtk git commit -m "fix(annotation): harden editor property interactions"
```

Skip `README.md` in `git add` when no user-facing documentation change was necessary.

- [ ] **Step 8: Request code review and resolve findings**

Invoke `superpowers:requesting-code-review`. Apply accepted findings with `superpowers:receiving-code-review`, rerun Step 4, and create focused commits without amending prior commits.

- [ ] **Step 9: Update the umbrella lifecycle registry**

After fresh verification, update Slice 1 to `Complete` only if all automated checks and both platform runtime checks passed and no work remains. Otherwise use `Handoff` or, only when the documented blocked threshold is met, `Blocked`. Include commit range/PR, exact verification commands, platform evidence, remaining work, known risk, and next entry point.

- [ ] **Step 10: Commit the lifecycle record**

```bash
rtk git add docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md
rtk git commit -m "docs(annotation): record editor foundation outcome"
```

- [ ] **Step 11: Finish the development branch**

Invoke `superpowers:finishing-a-development-branch` and present the integration/handoff choices. Slice 2 implementation remains locked until the registry says Slice 1 is `Complete`.
