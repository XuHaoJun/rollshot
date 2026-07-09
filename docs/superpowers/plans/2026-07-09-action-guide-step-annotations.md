# Action Guide Step Annotations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a manual number-callout annotation MVP for Action Guide steps and render those callouts in Storyboard preview/export without mutating retained keyframes.

**Architecture:** Keep annotation state app-owned inside Timeline Workspace, keyed by stable `GuideStep.source` and bound to the step's current `keyframe`. Refactor `rollshot-action` Storyboard rendering to accept generic image-backed steps, then let `rollshot-app` flatten annotated `ImageDocument`s only when rendering preview/export/Issue Pack Storyboard.

**Tech Stack:** Rust, `rollshot-action`, `rollshot-app` with the `action-guide` feature, `rollshot-image-document`, iced 0.14 built-in widgets plus Canvas, Cargo tests through `rtk`.

## Global Constraints

- This plan implements PRD Phase P4 only: lightweight per-step annotations.
- P1 Issue Pack Storyboard integration, P2 Storyboard preview, and P3 step captions are already present.
- MVP supports number callouts only. Text notes, redactions, highlights, layout controls, and agent proposals are explicitly out of scope.
- Annotations are non-destructive. Retained keyframe pixels in `FrameStore` must never be mutated.
- Annotation state is keyed by `GuideStep.source`, not the renumbered 1-based `GuideStep.index`.
- Replacing a step keyframe clears that step's annotations and shows a non-blocking banner.
- Deleting a step drops its annotation state.
- Storyboard preview, Storyboard export, and Issue Pack Storyboard export must use flattened annotated step images.
- Guide folder export keeps original keyframes and may omit annotation metadata in this phase.
- Issue Pack keyframes remain original reviewed evidence images. UI copy must not imply the whole pack is redacted or annotation-flattened.
- Use `tracing` for runtime diagnostics in product paths with explicit `rollshot::*` targets.
- Always prefix shell commands with `rtk`.

---

## File Structure

- Modify `crates/rollshot-action/src/storyboard.rs`
  - Add `StoryboardStep<'a>`.
  - Add `render_storyboard_steps(...)`.
  - Make `render_storyboard(...)` adapt `Guide + FrameStore` into generic steps.
  - Keep `export_storyboard(...)` source-compatible.
- Modify `crates/rollshot-action/src/lib.rs`
  - Re-export `render_storyboard_steps` and `StoryboardStep`.
- Create `crates/rollshot-app/src/timeline_workspace/annotation.rs`
  - Owns `ActionGuidePresentation`, per-step `ImageDocument`s, annotation session state, and small Canvas program for number callout placement.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`
  - Add `presentation: ActionGuidePresentation`.
  - Add `annotation_session: Option<StepAnnotationSession>`.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`
  - Add annotation messages.
  - Clear annotations on keyframe replacement and deletion.
  - Render Storyboard through annotated flattened images.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`
  - Add `Annotate Step` button.
  - Add annotation modal.
  - Reuse existing `stack + opaque` modal pattern.
- Modify `crates/rollshot-app/src/issue_pack.rs`
  - Add annotated Storyboard export path for `export_*_with_action_guide(...)`.

No new crates are needed.

---

## Review Lock-In

### Scope Challenge

The smallest useful P4 slice is "number callout on selected step, reflected in Storyboard artifacts." A full annotation editor would duplicate Result Workspace too early. Redactions are also intentionally excluded because Issue Pack still includes original keyframes, which would create false safety expectations.

### Key Assumptions

- `GuideStep.source: CandidateId` is stable for a step after deletion renumbers `index`; use it for presentation state.
- `ImageDocument::flatten()` is the authoritative non-destructive rendering path for annotations.
- 3 to 8 Action Guide steps are expected for the common workflow, so cloning raw keyframes into a render scratch vector is acceptable for P4.
- The annotation modal can use a simple fixed-fit image preview instead of the full Result Workspace zoom/pan system.
- P4 does not persist annotations in `session.json`; exported Storyboard artifacts are the deliverable.

### What Already Exists

- `rollshot-action::render_storyboard(...)` and `export_storyboard(...)` already render reviewed guide state and captions.
- `GuideStep` already exposes `source`, `index`, `caption`, and `keyframe`.
- `rollshot-image-document::ImageDocument` already supports number callouts, undo/redo history, immutable source pixels, and flattening.
- Result Workspace already demonstrates iced Canvas annotation rendering, but its update messages and workspace state are tied to Result Workspace.
- Timeline Workspace already uses `stack + opaque` modals for Issue Pack, Storyboard preview, FFmpeg setup, and discard.

### NOT in scope

- Text note editing: number callouts prove the per-step annotation path without text-editor focus and layout complexity.
- Opaque redaction UI: Issue Pack still includes original keyframes, so redaction UX would imply a safety guarantee this phase does not provide.
- Agent-suggested callouts: agent proposals need a manual review primitive first.
- Annotation persistence in guide/session JSON: P4 exports flattened Storyboards only; editable annotation persistence is a later product decision.
- Annotated keyframe files in Issue Pack: keyframes remain reviewed evidence originals; only `storyboard.png` is annotation-flattened.
- Live preview refresh while the annotation modal is open: preview regenerates when opened or exported.
- Full Result Workspace embedding: the MVP needs a small number-callout modal, not the full image editor state machine.
- Keyboard shortcuts for annotation tools: button-driven UI is enough for the first shipping slice.

### Test Coverage Table

```text
Task / behavior                                                Unit  Integ  UI smoke  Manual
-------------------------------------------------------------  ----  -----  --------  ------
Task 1 / generic Storyboard steps render raw images             yes   no     no        no
Task 1 / existing Guide+FrameStore export remains compatible    yes   yes    no        no
Task 2 / presentation state creates docs keyed by source        yes   no     no        no
Task 2 / delete prunes docs, keyframe replace clears doc        yes   no     no        no
Task 3 / modal number drag commits ImageDocument callout        yes   no     no        no
Task 3 / cancel/close leaves committed docs intact              yes   no     no        no
Task 4 / preview/export use flattened annotated images          yes   yes    no        no
Task 5 / Issue Pack Storyboard uses annotated image             no    yes    no        no
Task 6 / annotation modal view builds                           no    no     yes       no
Final / focused cargo tests                                     no    yes    no        no
Final / desktop smoke for annotate -> preview -> export         no    no     no        yes
```

### Failure Modes

```text
Risk                                               Test coverage                 Handling / user visibility
-------------------------------------------------  ----------------------------  ------------------------------------------------------------
Selected step has missing retained keyframe         Task 3 / Step 1              Opening annotation fails with a banner; workspace stays open.
Keyframe replaced after annotation                  Task 4 / Step 1              Clear that step's doc and show "Step annotations were cleared because the keyframe changed."
Step deleted                                        Task 4 / Step 6              Remove doc for that step's source during successful delete; no hidden orphan state remains.
Storyboard render missing keyframe                  Existing Storyboard tests    Existing StoryboardError::KeyframeMissing remains recoverable and visible.
Flattened annotated storyboard exceeds pixel limit  Existing Storyboard tests    Existing StoryboardError::CanvasTooLarge remains authoritative and visible in preview/export.
Issue Pack Storyboard export fails                  Task 5 / Step 4              Existing storyboard_export_failed warning path remains non-fatal and visible in manifest warnings.
Annotation session source disappears                Task 3 / Step 3              Close the annotation session and show a banner instead of panicking.
Annotation modal closed                             Task 3 / Step 1              Keep committed callouts; no extra "apply" state exists.
Tiny click without drag                             Task 3 / Step 1              Commit a stamped callout where tip == bubble.
Direct Storyboard PNG write partially fails         Task 4 / Step 3              Write to a temporary sibling and rename only after encode succeeds.
```

### Parallelization

```text
Task                                            Modules touched                                      Depends on
----------------------------------------------  ---------------------------------------------------  ----------
Task 1: Generic Storyboard step rendering        crates/rollshot-action/                             —
Task 2: Timeline presentation state              crates/rollshot-app/src/timeline_workspace/         Task 1
Task 3: Number callout annotation modal          crates/rollshot-app/src/timeline_workspace/         Task 2
Task 4: Annotated Storyboard preview/export      crates/rollshot-app/src/timeline_workspace/         Task 3
Task 5: Annotated Storyboard in Issue Pack       crates/rollshot-app/src/issue_pack.rs, timeline_*   Task 4
Task 6: Verification                             workspace                                           Tasks 1-5
```

Sequential execution is preferred. Task 1 is isolated in `rollshot-action`, but every later task builds on Timeline Workspace state and should be landed in order to avoid UI/update conflicts.

### Test Diagram

```text
Task 1: generic renderer
        |
        v
Task 2: per-step presentation docs keyed by GuideStep.source
        |
        v
Task 3: annotation modal commits ImageDocument number callouts
        |
        v
Task 4: preview/export render flattened annotated step images
        |
        v
Task 5: Issue Pack receives the same annotated Storyboard image
        |
        v
Task 6: focused suites + app suite + clippy + manual smoke
```

### Auto Review Decisions

Auto decision D1 — Use Canvas with iced 0.14-correct APIs
Context: The plan's annotation surface is a small 2D overlay with pointer events.
ELI10: We need a drawing layer on top of an image. Iced already has Canvas for this, but the field names in `canvas::Text` must match iced 0.14 or the plan will fail at compile time.
Stakes if we pick wrong: The first UI task fails on compile errors before behavior can be tested.
Recommendation: 1A because Canvas is the boring built-in for this job and the plan should use `align_x` / `align_y`.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
1A) Canvas with corrected iced 0.14 snippets (recommended): effort human ~15 min / AI ~5 min; risk low; maintenance low.
  ✅ Matches the repo's existing Result Workspace Canvas approach.
  ❌ Still duplicates a small amount of drawing code.
1B) Custom advanced widget: effort human ~1 day / AI ~45 min; risk medium; maintenance medium.
  ✅ Could package hit-testing and drawing more tightly.
  ❌ Spends complexity on a one-tool MVP.
Net: Canvas keeps the implementation boring while removing known API errors.

Auto decision D2 — Avoid panics in annotation session update
Context: Task 3 used `expect(...)` in a product update path when looking up the session's step.
ELI10: A UI message can arrive after state has changed. If code assumes the step still exists and panics, the app can crash instead of closing the modal cleanly.
Stakes if we pick wrong: A stale annotation message could crash the Action Guide workspace.
Recommendation: 2A because explicit stale-session handling is safer and only a few lines.
Completeness: A=10/10, B=5/10
Pros / cons:
2A) Handle stale session with a banner (recommended): effort human ~20 min / AI ~5 min; risk low; maintenance low.
  ✅ Turns a possible crash into visible recoverable state.
  ❌ Adds one more branch in the update function.
2B) Keep `expect(...)`: effort none; risk medium; maintenance low.
  ✅ Shorter code.
  ❌ Relies on UI modal layering to prevent all stale messages.
Net: Explicit handling follows the repo preference for edge cases over fragile shortcuts.

Auto decision D3 — Prove annotated pixels are used
Context: Task 4's preview test only checked that a preview exists.
ELI10: A preview can open even if it ignores annotations. The test must compare output before and after adding a callout so it catches the real bug.
Stakes if we pick wrong: The feature can ship with a working modal but an unannotated Storyboard.
Recommendation: 3A because the acceptance criterion is about flattened pixels, not modal state.
Completeness: A=10/10, B=6/10
Pros / cons:
3A) Compare unannotated vs annotated Storyboard pixels (recommended): effort human ~30 min / AI ~10 min; risk low; maintenance low.
  ✅ Directly proves the output artifact changes after annotation.
  ❌ Pixel-diff tests can be brittle if fixtures are too small, so use the existing stable synthetic fixture.
3B) Keep preview-exists assertion: effort none; risk high; maintenance low.
  ✅ Simple test.
  ❌ Does not test the core requirement.
Net: The stronger test is the only one that protects the user-visible artifact.

Auto decision D4 — Preserve atomic Storyboard file writes
Context: Replacing `export_storyboard(...)` in Timeline export would otherwise write PNGs directly.
ELI10: If PNG encoding fails halfway, direct writes can leave a broken file at the target path. The old exporter promised no target file on error, so the app should keep that behavior.
Stakes if we pick wrong: A failed export can leave a corrupt image that looks like a valid saved Storyboard.
Recommendation: 4A because it preserves the existing export contract.
Completeness: A=10/10, B=7/10
Pros / cons:
4A) Add app-side atomic write helper (recommended): effort human ~30 min / AI ~10 min; risk low; maintenance low.
  ✅ Keeps "no file left on error" behavior for annotated exports.
  ❌ Duplicates a tiny private helper from `rollshot-action`.
4B) Write directly with `save_with_format`: effort none; risk medium; maintenance low.
  ✅ Shorter implementation.
  ❌ Can leave partial output on encode or rename failures.
Net: A small helper is worth preserving the file-write contract.

Auto decision D5 — Add task commit steps
Context: The plan is a superpowers execution plan, but tasks did not end with commit steps.
ELI10: Each task needs a clean stopping point so reviewers can inspect one logical change at a time. Without commits, subagents tend to pile unrelated work together.
Stakes if we pick wrong: Review becomes harder and rollback boundaries are unclear.
Recommendation: 5A because atomic commits are part of the plan format and reduce execution risk.
Completeness: A=10/10, B=6/10
Pros / cons:
5A) Add a commit step to Tasks 1-5 (recommended): effort human ~10 min / AI ~3 min; risk low; maintenance low.
  ✅ Creates clean review and rollback boundaries.
  ❌ Adds repetitive boilerplate.
5B) Leave commits to the executor: effort none; risk medium; maintenance low.
  ✅ Shorter plan.
  ❌ Inconsistent with the required plan format.
Net: Explicit commit boundaries make the agent workflow safer.

---

### Task 1: Add Generic Storyboard Step Rendering

**Files:**
- Modify: `crates/rollshot-action/src/storyboard.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/storyboard.rs`

**Interfaces:**
- Consumes: existing `Guide`, `FrameStore`, `StoryboardOptions`, `StoryboardError`.
- Produces:
  - `pub struct StoryboardStep<'a> { pub index: usize, pub title: &'a str, pub caption: Option<&'a str>, pub image: &'a RgbaImage }`
  - `pub fn render_storyboard_steps(steps: &[StoryboardStep<'_>], opts: StoryboardOptions) -> Result<StoryboardRenderResult, StoryboardError>`
  - Existing `render_storyboard(...)` and `export_storyboard(...)` remain source-compatible.

- [ ] **Step 1: Write failing tests for generic rendering**

Add these tests inside `#[cfg(test)] mod tests` in `crates/rollshot-action/src/storyboard.rs`:

```rust
    #[test]
    fn renders_storyboard_from_explicit_steps() {
        let image = quadrant();
        let steps = vec![StoryboardStep {
            index: 1,
            title: "Click Save",
            caption: Some("The dialog closes but the value is not persisted."),
            image: &image,
        }];

        let result = render_storyboard_steps(
            &steps,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render explicit steps");

        assert_eq!(result.width, 320);
        assert_eq!(result.step_count, 1);
        assert_eq!(result.image.width(), result.width);
        assert_eq!(result.image.height(), result.height);
        assert!(
            result.image.pixels().any(|pixel| pixel.0 != [255, 255, 255, 255]),
            "render should contain card, text, and image pixels"
        );
    }

    #[test]
    fn explicit_step_render_rejects_empty_steps() {
        let result = render_storyboard_steps(&[], StoryboardOptions::default());

        assert!(matches!(result, Err(StoryboardError::Empty)));
    }
```

- [ ] **Step 2: Run tests to verify the API is missing**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: FAIL to compile with errors equivalent to:

```text
cannot find struct, variant or union type `StoryboardStep` in this scope
cannot find function `render_storyboard_steps` in this scope
```

- [ ] **Step 3: Add `StoryboardStep` and route `render_storyboard(...)` through it**

In `crates/rollshot-action/src/storyboard.rs`, add this near `StoryboardRenderResult`:

```rust
#[derive(Debug, Clone, Copy)]
pub struct StoryboardStep<'a> {
    pub index: usize,
    pub title: &'a str,
    pub caption: Option<&'a str>,
    pub image: &'a RgbaImage,
}
```

Change `render_storyboard(...)` to validate retained frames and call the new function:

```rust
pub fn render_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError> {
    if guide.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let mut steps = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let retained = store
            .retained(step.keyframe)
            .ok_or(StoryboardError::KeyframeMissing { index: i + 1 })?;
        steps.push(StoryboardStep {
            index: step.index,
            title: &step.title,
            caption: non_empty_caption(&step.caption),
            image: &retained.image,
        });
    }

    render_storyboard_steps(&steps, opts)
}
```

Move the current canvas assembly body into:

```rust
pub fn render_storyboard_steps(
    steps: &[StoryboardStep<'_>],
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError> {
    if steps.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let canvas_width = opts.max_width;
    let card_width = canvas_width
        .checked_sub(opts.outer_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;
    let content_width = card_width
        .checked_sub(opts.card_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;

    let mut cards = Vec::with_capacity(steps.len());
    for step in steps {
        let image = downscale(step.image, content_width);
        let label = step_label(step.index, step.title, opts.show_titles);
        let label = fit_label(&label, content_width as f32);
        let (_, label_height) = measure_block(&label, LABEL_FONT_PX, true);
        let label_height = label_height.ceil() as u32;

        let caption = step
            .caption
            .and_then(non_empty_caption)
            .map(|caption| fit_caption(caption, content_width as f32));
        let caption_height = caption
            .as_ref()
            .map(|caption| measure_block(caption, CAPTION_FONT_PX, false).1.ceil() as u32)
            .unwrap_or(0);
        let text_height = if caption.is_some() {
            label_height
                .checked_add(CAPTION_GAP)
                .and_then(|height| height.checked_add(caption_height))
                .ok_or(StoryboardError::CanvasTooLarge)?
        } else {
            label_height
        };
        let card_height = opts
            .card_padding
            .checked_mul(2)
            .and_then(|height| height.checked_add(text_height))
            .and_then(|height| height.checked_add(LABEL_GAP))
            .and_then(|height| height.checked_add(image.height()))
            .ok_or(StoryboardError::CanvasTooLarge)?;
        cards.push(Card {
            label,
            caption,
            image,
            height: card_height,
        });
    }

    render_cards(cards, opts, canvas_width, card_width)
}
```

Extract the existing final canvas-height calculation and draw loop into private `render_cards(...)`:

```rust
fn render_cards(
    cards: Vec<Card>,
    opts: StoryboardOptions,
    canvas_width: u32,
    card_width: u32,
) -> Result<StoryboardRenderResult, StoryboardError> {
    let mut canvas_height = opts
        .outer_padding
        .checked_mul(2)
        .ok_or(StoryboardError::CanvasTooLarge)?;
    for (i, card) in cards.iter().enumerate() {
        if i > 0 {
            canvas_height = canvas_height
                .checked_add(opts.card_spacing)
                .ok_or(StoryboardError::CanvasTooLarge)?;
        }
        canvas_height = canvas_height
            .checked_add(card.height)
            .ok_or(StoryboardError::CanvasTooLarge)?;
    }
    let canvas_pixels = (canvas_width as u64)
        .checked_mul(canvas_height as u64)
        .ok_or(StoryboardError::CanvasTooLarge)?;
    if canvas_pixels > opts.max_canvas_pixels {
        return Err(StoryboardError::CanvasTooLarge);
    }

    let mut canvas = RgbaImage::from_pixel(canvas_width, canvas_height, WHITE);
    let mut y = opts.outer_padding;
    for (i, card) in cards.iter().enumerate() {
        draw_card(&mut canvas, opts.outer_padding, y, card_width, card.height);
        draw_card_content(&mut canvas, &opts, y, card);

        y += card.height;
        if i + 1 < cards.len() {
            y += opts.card_spacing;
        }
    }

    Ok(StoryboardRenderResult {
        image: canvas,
        width: canvas_width,
        height: canvas_height,
        step_count: cards.len(),
    })
}
```

Extract the per-card drawing body into `draw_card_content(...)`:

```rust
fn draw_card_content(canvas: &mut RgbaImage, opts: &StoryboardOptions, y: u32, card: &Card) {
    let content_x = opts.outer_padding + opts.card_padding;
    let mut content_y = y + opts.card_padding;
    draw_text_block(
        canvas,
        ImagePoint::new(content_x as f32, content_y as f32),
        &card.label,
        LABEL_FONT_PX,
        true,
        TEXT_COLOR,
    );
    let (_, label_height) = measure_block(&card.label, LABEL_FONT_PX, true);
    content_y += label_height.ceil() as u32;
    if let Some(caption) = &card.caption {
        content_y += CAPTION_GAP;
        draw_text_block(
            canvas,
            ImagePoint::new(content_x as f32, content_y as f32),
            caption,
            CAPTION_FONT_PX,
            false,
            CAPTION_COLOR,
        );
        let (_, caption_height) = measure_block(caption, CAPTION_FONT_PX, false);
        content_y += caption_height.ceil() as u32;
    }
    content_y += LABEL_GAP;
    image::imageops::replace(
        canvas,
        &card.image,
        i64::from(content_x),
        i64::from(content_y),
    );
}
```

- [ ] **Step 4: Re-export new API**

In `crates/rollshot-action/src/lib.rs`, extend the existing storyboard export:

```rust
pub use storyboard::{
    export_storyboard, render_storyboard, render_storyboard_steps, StoryboardExportResult,
    StoryboardOptions, StoryboardRenderResult, StoryboardStep,
};
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: PASS.

- [ ] **Step 6: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-action/src/storyboard.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "refactor(action): add generic storyboard step renderer"
```

Expected: commit succeeds with only Task 1 files staged.

---

### Task 2: Add Timeline Presentation State

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/mod.rs`

**Interfaces:**
- Consumes: `GuideStep.source`, `GuideStep.keyframe`, `FrameStore::retained(...)`, `ImageDocument`.
- Produces:
  - `ActionGuidePresentation`
  - `StepAnnotationDocument`
  - `StepAnnotationSession`
  - `TimelineWorkspace::presentation`
  - `TimelineWorkspace::annotation_session`

- [ ] **Step 1: Create failing presentation-state tests**

Create `crates/rollshot-app/src/timeline_workspace/annotation.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{
        CandidateKind, CandidateStep, DetectReason, FrameStore, Guide, StoreConfig,
    };

    fn frame_store_with_two_frames() -> FrameStore {
        let mut store = FrameStore::new(StoreConfig::default());
        let first = store.ingest(RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255])), 0);
        let second = store.ingest(RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255])), 100);
        store.retain_window(first);
        store.retain_window(second);
        store
    }

    fn guide() -> Guide {
        Guide::from_candidates(vec![CandidateStep {
            id: 42,
            kind: CandidateKind::Click,
            reason: DetectReason::VisualChange,
            at_ms: 100,
            keyframe: 0,
            nearby: vec![0, 1],
        }])
    }

    #[test]
    fn document_for_step_is_keyed_by_source_and_uses_current_keyframe() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();

        let doc = presentation
            .document_for_step(step, &store)
            .expect("document exists");

        assert_eq!(doc.source, 42);
        assert_eq!(doc.keyframe, 0);
        assert_eq!(doc.document.source().dimensions(), (8, 8));
        assert!(!presentation.has_annotations(step.source));
    }

    #[test]
    fn clear_for_keyframe_change_removes_only_matching_step() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();
        let doc = presentation.document_for_step(step, &store).unwrap();
        doc.document.add_number_callout(
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
            rollshot_image_document::ImagePoint::new(4.0, 4.0),
        );

        assert!(presentation.clear_for_source(step.source));
        assert!(!presentation.has_annotations(step.source));
        assert!(!presentation.clear_for_source(step.source));
    }

    #[test]
    fn retain_sources_prunes_deleted_steps() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();
        presentation.document_for_step(step, &store).unwrap();

        presentation.retain_sources(std::iter::empty());

        assert!(!presentation.has_annotations(step.source));
    }
}
```

- [ ] **Step 2: Run tests to verify missing module/types**

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace::annotation --features action-guide
```

Expected: FAIL to compile because `annotation` module and types do not exist.

- [ ] **Step 3: Implement presentation state**

Add the production content at the top of `crates/rollshot-app/src/timeline_workspace/annotation.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};

use iced::widget::image;
use image::RgbaImage;
use rollshot_action::{CandidateId, FrameId, FrameStore, GuideStep};
use rollshot_image_document::{ImageDocument, ImagePoint};

pub(crate) struct StepAnnotationDocument {
    pub source: CandidateId,
    pub keyframe: FrameId,
    pub document: ImageDocument,
}

#[derive(Default)]
pub(crate) struct ActionGuidePresentation {
    docs: BTreeMap<CandidateId, StepAnnotationDocument>,
}

impl ActionGuidePresentation {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn document_for_step(
        &mut self,
        step: &GuideStep,
        store: &FrameStore,
    ) -> Option<&mut StepAnnotationDocument> {
        let needs_new = self
            .docs
            .get(&step.source)
            .map_or(true, |doc| doc.keyframe != step.keyframe);
        if needs_new {
            let frame = store.retained(step.keyframe)?;
            self.docs.insert(
                step.source,
                StepAnnotationDocument {
                    source: step.source,
                    keyframe: step.keyframe,
                    document: ImageDocument::new(frame.image.clone()),
                },
            );
        }
        self.docs.get_mut(&step.source)
    }

    pub(crate) fn doc(&self, source: CandidateId) -> Option<&StepAnnotationDocument> {
        self.docs.get(&source)
    }

    pub(crate) fn has_annotations(&self, source: CandidateId) -> bool {
        self.docs
            .get(&source)
            .is_some_and(|doc| !doc.document.annotations().is_empty())
    }

    pub(crate) fn clear_for_source(&mut self, source: CandidateId) -> bool {
        self.docs.remove(&source).is_some()
    }

    pub(crate) fn retain_sources(&mut self, sources: impl IntoIterator<Item = CandidateId>) {
        let keep: BTreeSet<_> = sources.into_iter().collect();
        self.docs.retain(|source, _| keep.contains(source));
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AnnotationDraft {
    Number { tip: ImagePoint, bubble: ImagePoint },
}

pub(crate) struct StepAnnotationSession {
    pub source: CandidateId,
    pub keyframe: FrameId,
    pub handle: image::Handle,
    pub width: u32,
    pub height: u32,
    pub draft: Option<AnnotationDraft>,
}

impl StepAnnotationSession {
    pub(crate) fn new(source: CandidateId, keyframe: FrameId, image: &RgbaImage) -> Self {
        Self {
            source,
            keyframe,
            handle: super::build_handle(image),
            width: image.width(),
            height: image.height(),
            draft: None,
        }
    }
}
```

- [ ] **Step 4: Wire module and state fields**

In `crates/rollshot-app/src/timeline_workspace/mod.rs`, add:

```rust
mod annotation;
```

Add these fields to `TimelineWorkspace`:

```rust
    pub(crate) presentation: annotation::ActionGuidePresentation,
    pub(crate) annotation_session: Option<annotation::StepAnnotationSession>,
```

Initialize them in `TimelineWorkspace::new(...)`:

```rust
            presentation: annotation::ActionGuidePresentation::new(),
            annotation_session: None,
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace --features action-guide
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/annotation.rs crates/rollshot-app/src/timeline_workspace/mod.rs
rtk git commit -m "feat(action): add guide step annotation state"
```

Expected: commit succeeds with only Task 2 files staged.

---

### Task 3: Add Number Callout Annotation Modal

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**
- Consumes: `ActionGuidePresentation::document_for_step(...)`, `ImageDocument::add_number_callout(...)`.
- Produces:
  - `Message::AnnotateStepRequested`
  - `Message::AnnotationCanvasPressed(ImagePoint)`
  - `Message::AnnotationCanvasMoved(ImagePoint)`
  - `Message::AnnotationCanvasReleased(ImagePoint)`
  - `Message::AnnotationDone`
  - `Message::AnnotationCancel`
  - `NumberAnnotationCanvas<'a>`

- [ ] **Step 1: Write failing update tests**

Add tests in `crates/rollshot-app/src/timeline_workspace/update.rs`:

```rust
    #[test]
    fn annotate_step_opens_session_for_selected_keyframe() {
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::AnnotateStepRequested);

        let session = state.annotation_session.as_ref().expect("session open");
        let step = state.selected_step().unwrap();
        assert_eq!(session.source, step.source);
        assert_eq!(session.keyframe, step.keyframe);
        assert_eq!(session.width, 32);
        assert_eq!(session.height, 32);
    }

    #[test]
    fn annotation_drag_commits_number_callout() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;

        let _ = update(
            &mut state,
            Message::AnnotationCanvasPressed(rollshot_image_document::ImagePoint::new(4.0, 4.0)),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasMoved(rollshot_image_document::ImagePoint::new(20.0, 20.0)),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(20.0, 20.0)),
        );

        assert!(state.presentation.has_annotations(source));
        let doc = state.presentation.doc(source).unwrap();
        assert_eq!(doc.document.annotations().len(), 1);
    }

    #[test]
    fn annotation_done_closes_session_without_dropping_document() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );

        let _ = update(&mut state, Message::AnnotationDone);

        assert!(state.annotation_session.is_none());
        assert!(state.presentation.has_annotations(source));
    }
```

- [ ] **Step 2: Add messages and run failing tests**

Add enum variants to `Message`:

```rust
    AnnotateStepRequested,
    AnnotationCanvasPressed(rollshot_image_document::ImagePoint),
    AnnotationCanvasMoved(rollshot_image_document::ImagePoint),
    AnnotationCanvasReleased(rollshot_image_document::ImagePoint),
    AnnotationDone,
    AnnotationCancel,
```

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace --features action-guide
```

Expected: FAIL because update arms and view wiring are missing.

- [ ] **Step 3: Implement update arms**

Add imports in `update.rs`:

```rust
use rollshot_image_document::ImagePoint;
```

Add match arms:

```rust
        Message::AnnotateStepRequested => {
            state.message = None;
            let Some(step) = state.selected_step().cloned() else {
                state.message = Some("Select a step before annotating.".to_string());
                return Task::none();
            };
            match state.presentation.document_for_step(&step, &state.store) {
                Some(doc) => {
                    tracing::info!(
                        target: "rollshot::action::annotation",
                        source = step.source,
                        keyframe = step.keyframe,
                        "annotation session opened"
                    );
                    state.annotation_session = Some(super::annotation::StepAnnotationSession::new(
                        step.source,
                        step.keyframe,
                        doc.document.source(),
                    ));
                }
                None => {
                    state.message = Some("Cannot annotate this step because its keyframe is unavailable.".to_string());
                }
            }
            Task::none()
        }
        Message::AnnotationCanvasPressed(point) => {
            if let Some(session) = &mut state.annotation_session {
                session.draft = Some(super::annotation::AnnotationDraft::Number {
                    tip: clamp_annotation_point(point, session.width, session.height),
                    bubble: clamp_annotation_point(point, session.width, session.height),
                });
            }
            Task::none()
        }
        Message::AnnotationCanvasMoved(point) => {
            if let Some(session) = &mut state.annotation_session {
                if let Some(super::annotation::AnnotationDraft::Number { bubble, .. }) =
                    &mut session.draft
                {
                    *bubble = clamp_annotation_point(point, session.width, session.height);
                }
            }
            Task::none()
        }
        Message::AnnotationCanvasReleased(point) => {
            let Some(session) = &mut state.annotation_session else {
                return Task::none();
            };
            let release = clamp_annotation_point(point, session.width, session.height);
            let source = session.source;
            let tip = match session.draft.take() {
                Some(super::annotation::AnnotationDraft::Number { tip, .. }) => tip,
                None => release,
            };
            let Some(step) = state
                .guide
                .steps()
                .iter()
                .find(|step| step.source == source)
                .cloned()
            else {
                state.annotation_session = None;
                state.message =
                    Some("Annotation session closed because the step no longer exists.".to_string());
                return Task::none();
            };
            if let Some(doc) = state.presentation.document_for_step(&step, &state.store) {
                doc.document.add_number_callout(tip, release);
            }
            Task::none()
        }
        Message::AnnotationDone => {
            state.annotation_session = None;
            Task::none()
        }
        Message::AnnotationCancel => {
            state.annotation_session = None;
            Task::none()
        }
```

Add helper:

```rust
fn clamp_annotation_point(point: ImagePoint, width: u32, height: u32) -> ImagePoint {
    point.clamp_to(width, height)
}
```

- [ ] **Step 4: Add Canvas program**

In `annotation.rs`, add:

```rust
use iced::widget::{canvas, text};
use iced::{alignment, mouse, Color, Point, Rectangle, Renderer, Theme};
use rollshot_image_document::{annotation_shapes, Annotation, RenderShape, TextAnchor};

pub(crate) struct NumberAnnotationCanvas<'a> {
    pub document: &'a ImageDocument,
    pub draft: Option<AnnotationDraft>,
    pub scale: f32,
}

impl NumberAnnotationCanvas<'_> {
    fn image_point(&self, local: Point) -> ImagePoint {
        ImagePoint::new(local.x / self.scale, local.y / self.scale)
    }

    fn draw_annotation(&self, frame: &mut canvas::Frame, annotation: &Annotation) {
        for shape in annotation_shapes(annotation) {
            match shape {
                RenderShape::Circle {
                    center,
                    radius,
                    fill,
                    outline_width,
                    outline,
                } => {
                    let path = canvas::Path::circle(
                        Point::new(center.x * self.scale, center.y * self.scale),
                        radius * self.scale,
                    );
                    frame.fill(&path, rgba(fill));
                    frame.stroke(
                        &path,
                        canvas::Stroke::default()
                            .with_color(rgba(outline))
                            .with_width(outline_width * self.scale),
                    );
                }
                RenderShape::Triangle { points, color } => {
                    let path = canvas::Path::new(|builder| {
                        builder.move_to(Point::new(points[0].x * self.scale, points[0].y * self.scale));
                        builder.line_to(Point::new(points[1].x * self.scale, points[1].y * self.scale));
                        builder.line_to(Point::new(points[2].x * self.scale, points[2].y * self.scale));
                        builder.close();
                    });
                    frame.fill(&path, rgba(color));
                }
                RenderShape::Label {
                    anchor,
                    anchor_kind: TextAnchor::Center,
                    content,
                    px,
                    bold,
                    color,
                } => {
                    frame.fill_text(canvas::Text {
                        content,
                        position: Point::new(anchor.x * self.scale, anchor.y * self.scale),
                        color: rgba(color),
                        size: iced::Pixels(px * self.scale),
                        align_x: text::Alignment::Center,
                        align_y: alignment::Vertical::Center,
                        font: if bold {
                            iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::with_name(rollshot_image_document::style::FONT_FAMILY_NAME)
                            }
                        } else {
                            iced::Font::with_name(rollshot_image_document::style::FONT_FAMILY_NAME)
                        },
                        ..canvas::Text::default()
                    });
                }
                RenderShape::Rect { .. } | RenderShape::Label { .. } => {}
            }
        }
    }
}

fn rgba(c: rollshot_image_document::Rgba8) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a as f32 / 255.0)
}

impl canvas::Program<super::Message> for NumberAnnotationCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<super::Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let local = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(super::Message::AnnotationCanvasPressed(
                    self.image_point(local),
                )).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let local = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(super::Message::AnnotationCanvasMoved(
                    self.image_point(local),
                )))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let local = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(super::Message::AnnotationCanvasReleased(
                    self.image_point(local),
                )).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for annotation in self.document.annotations() {
            self.draw_annotation(&mut frame, annotation);
        }
        if let Some(AnnotationDraft::Number { tip, bubble }) = self.draft {
            let draft = Annotation::NumberCallout {
                id: rollshot_image_document::AnnotationId(0),
                number: self.document.annotations().len() as u32 + 1,
                tip,
                bubble,
            };
            self.draw_annotation(&mut frame, &draft);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::Crosshair
    }
}
```

- [ ] **Step 5: Add view button and modal**

In `detail_panel(...)`, insert before `Delete step`:

```rust
                button(text("Annotate Step"))
                    .on_press(Message::AnnotateStepRequested)
                    .style(button::secondary),
```

In `view(...)`, add the annotation modal layer before Storyboard preview:

```rust
    let body = if state.annotation_session.is_some() {
        annotation_modal(body, state)
    } else {
        body
    };
```

Add:

```rust
fn annotation_modal<'a>(
    base: Element<'a, Message>,
    state: &'a TimelineWorkspace,
) -> Element<'a, Message> {
    let session = state.annotation_session.as_ref().expect("checked by caller");
    let doc = state
        .presentation
        .doc(session.source)
        .expect("session has presentation doc");
    let max_w = 720.0;
    let max_h = 480.0;
    let scale = (max_w / session.width as f32)
        .min(max_h / session.height as f32)
        .min(1.0)
        .max(0.1);
    let rendered = iced::Size::new(session.width as f32 * scale, session.height as f32 * scale);
    let img = image(session.handle.clone())
        .width(Length::Fixed(rendered.width))
        .height(Length::Fixed(rendered.height));
    let overlay = iced::widget::canvas(super::annotation::NumberAnnotationCanvas {
        document: &doc.document,
        draft: session.draft,
        scale,
    })
    .width(Length::Fixed(rendered.width))
    .height(Length::Fixed(rendered.height));

    let dialog_view = container(
        column![
            row![
                text("Annotate Step").size(18),
                Space::new().width(Length::Fill),
                text("Number callout").size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            container(iced::widget::stack![img, overlay])
                .width(Length::Fixed(rendered.width))
                .height(Length::Fixed(rendered.height))
                .style(container::rounded_box),
            row![
                button(text("Done"))
                    .on_press(Message::AnnotationDone)
                    .style(button::primary),
                button(text("Close")).on_press(Message::AnnotationCancel),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(780.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog_view))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color { a: 0.8, ..Color::BLACK }.into()),
                    ..container::Style::default()
                }),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::AnnotationCancel),
    );

    stack![base, scrim].into()
}
```

- [ ] **Step 6: Extend view smoke test**

In `view_builds_for_selected_empty_and_discard_states`, add:

```rust
        let mut annotated = ws(recording_from_frames(), InputCapability::SemanticEvents);
        let _ = crate::timeline_workspace::update::update(
            &mut annotated,
            Message::AnnotateStepRequested,
        );
        assert!(annotated.annotation_session.is_some());
        let _ = view(&annotated);
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace --features action-guide
```

Expected: PASS.

- [ ] **Step 8: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/annotation.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(action): annotate guide steps with number callouts"
```

Expected: commit succeeds with only Task 3 files staged.

---

### Task 4: Render Annotated Images In Storyboard Preview And Export

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: `render_storyboard_steps(...)`, `StoryboardStep<'_>`, `ImageDocument::flatten()`.
- Produces:
  - `render_timeline_storyboard(...)`
  - Preview/export Storyboard paths use flattened annotated images.

- [ ] **Step 1: Write failing tests for annotated rendering**

Add tests in `update.rs`:

```rust
    #[test]
    fn storyboard_render_uses_flattened_annotation_pixels() {
        let mut state = ws(recording_from_frames());
        let before = render_timeline_storyboard(&state, storyboard_preview_options())
            .expect("render before annotation")
            .image;
        let source = state.selected_step().unwrap().source;
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        assert!(state.presentation.has_annotations(source));

        let after = render_timeline_storyboard(&state, storyboard_preview_options())
            .expect("render after annotation")
            .image;

        assert_ne!(
            before.as_raw(),
            after.as_raw(),
            "annotated render should differ from raw keyframe render"
        );
    }

    #[test]
    fn replacing_keyframe_clears_step_annotations_and_shows_banner() {
        let mut state = ws(recording_from_frames());
        let source = state.selected_step().unwrap().source;
        let replacement = state.strip.iter().find(|f| {
            Some(f.id) != state.selected_step().map(|step| step.keyframe)
        }).expect("replacement frame").id;
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        assert!(state.presentation.has_annotations(source));

        let _ = update(&mut state, Message::ReplaceKeyframe(replacement));

        assert!(!state.presentation.has_annotations(source));
        assert_eq!(
            state.message.as_deref(),
            Some("Step annotations were cleared because the keyframe changed.")
        );
    }
```

- [ ] **Step 2: Update Storyboard imports**

In `update.rs`, change the import to include generic renderer pieces:

```rust
use rollshot_action::{
    export_gif, export_guide, export_video, render_storyboard_steps, GifOptions, StoryboardOptions,
    StoryboardRenderResult, StoryboardStep, VideoOptions,
};
```

Remove direct `export_storyboard` and `render_storyboard` imports once helper functions replace them.

- [ ] **Step 3: Add annotated render helper**

Add this helper near `storyboard_preview_options()`:

```rust
fn render_timeline_storyboard(
    state: &TimelineWorkspace,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, rollshot_action::StoryboardError> {
    if state.guide.is_empty() {
        return Err(rollshot_action::StoryboardError::Empty);
    }

    let mut images = Vec::with_capacity(state.guide.steps().len());
    for (i, step) in state.guide.steps().iter().enumerate() {
        let frame = state
            .store
            .retained(step.keyframe)
            .ok_or(rollshot_action::StoryboardError::KeyframeMissing { index: i + 1 })?;
        let image = match state.presentation.doc(step.source) {
            Some(doc) if doc.keyframe == step.keyframe && !doc.document.annotations().is_empty() => {
                doc.document.flatten()
            }
            _ => frame.image.clone(),
        };
        images.push(image);
    }

    let steps: Vec<_> = state
        .guide
        .steps()
        .iter()
        .zip(images.iter())
        .map(|(step, image)| StoryboardStep {
            index: step.index,
            title: &step.title,
            caption: {
                let caption = step.caption.trim();
                (!caption.is_empty()).then_some(caption)
            },
            image,
        })
        .collect();

    render_storyboard_steps(&steps, opts)
}

fn write_storyboard_png(
    state: &TimelineWorkspace,
    path: &Path,
) -> Result<StoryboardRenderResult, rollshot_action::StoryboardError> {
    let rendered = render_timeline_storyboard(state, StoryboardOptions::default())?;
    write_storyboard_png_atomic(path, &rendered.image)?;
    Ok(rendered)
}

fn write_storyboard_png_atomic(
    path: &Path,
    image: &image::RgbaImage,
) -> Result<(), rollshot_action::StoryboardError> {
    let tmp = path.with_extension("png.tmp");
    image
        .save_with_format(&tmp, image::ImageFormat::Png)
        .map_err(|source| {
            let _ = std::fs::remove_file(&tmp);
            rollshot_action::StoryboardError::Encode {
                path: tmp.display().to_string(),
                source,
            }
        })?;
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        rollshot_action::StoryboardError::Io {
            path: path.display().to_string(),
            source,
        }
    })
}
```

Add this export-path test near the preview render test:

```rust
    #[test]
    fn storyboard_export_error_leaves_no_target_file() {
        let state = ws(recording_from_frames());
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("missing-parent");
        let target = target_dir.join("storyboard.png");

        let result = write_storyboard_png(&state, &target);

        assert!(result.is_err());
        assert!(!target.exists());
        assert!(!target.with_extension("png.tmp").exists());
    }
```

Remove the direct-write version:

```rust
fn write_storyboard_png(
    state: &TimelineWorkspace,
    path: &Path,
) -> Result<StoryboardRenderResult, rollshot_action::StoryboardError> {
    let rendered = render_timeline_storyboard(state, StoryboardOptions::default())?;
    rendered
        .image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|source| rollshot_action::StoryboardError::Encode {
            path: path.display().to_string(),
            source,
        })?;
    Ok(rendered)
}
```

- [ ] **Step 4: Route preview through annotated helper**

In `Message::PreviewStoryboardRequested`, replace `render_storyboard(...)` with:

```rust
match render_timeline_storyboard(state, storyboard_preview_options()) {
```

Keep existing preview success/error handling.

- [ ] **Step 5: Route export through annotated helper**

In `Message::ExportStoryboardPathChosen(Some(path))`, replace `export_storyboard(...)` with:

```rust
match write_storyboard_png(state, &path) {
    Ok(result) => {
        tracing::info!(
            target: "rollshot::action::export",
            path = %path.display(),
            steps = result.step_count,
            width = result.width,
            height = result.height,
            "storyboard exported"
        );
        state.message = Some(format!("Storyboard saved to {}", path.display()));
    }
```

Keep the existing error arm.

- [ ] **Step 6: Clear annotations on delete and keyframe replacement**

In `Message::DeleteStep`, capture the deleted source before calling `guide.delete(...)`:

```rust
let deleted_source = state.selected_step().map(|step| step.source);
```

After successful delete:

```rust
if let Some(source) = deleted_source {
    state.presentation.clear_for_source(source);
}
state
    .presentation
    .retain_sources(state.guide.steps().iter().map(|step| step.source));
```

In `Message::ReplaceKeyframe(frame)`, capture source and clear on success:

```rust
let source = state.selected_step().map(|step| step.source);
if state.guide.replace_keyframe(index, frame) {
    state.rebuild_selection_handles();
    if let Some(source) = source {
        if state.presentation.clear_for_source(source) {
            state.message = Some(
                "Step annotations were cleared because the keyframe changed.".to_string(),
            );
        }
    }
}
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace --features action-guide
```

Expected: PASS.

- [ ] **Step 8: Commit Task 4**

Run:

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): render annotated storyboards"
```

Expected: commit succeeds with only Task 4 files staged.

---

### Task 5: Use Annotated Storyboard In Issue Pack

**Files:**
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

**Interfaces:**
- Consumes: `ActionGuideExportSource<'_>`.
- Produces:
  - Optional `storyboard_image: Option<RgbaImage>` in `ActionGuideExportSource<'_>`.
  - Issue Pack Storyboard writes annotated flattened image when supplied.

- [ ] **Step 1: Extend export source model**

In `issue_pack.rs`, change `ActionGuideExportSource<'a>` under `#[cfg(feature = "action-guide")]`:

```rust
pub(crate) struct ActionGuideExportSource<'a> {
    pub guide: &'a rollshot_action::Guide,
    pub store: &'a rollshot_action::FrameStore,
    pub region: rollshot_action::CaptureRegion,
    pub capability: rollshot_action::InputCapability,
    pub source_kind: rollshot_action::InputSourceKind,
    pub include_gif: bool,
    pub storyboard_image: Option<image::RgbaImage>,
}
```

Update every existing `ActionGuideExportSource` struct literal in `issue_pack.rs` tests and `timeline_workspace/update.rs` to include `storyboard_image: None` before adding the annotated Timeline path.

- [ ] **Step 2: Write Storyboard image in Issue Pack when supplied**

In the Storyboard generation section of `build_folder(...)`, replace the direct `export_storyboard(...)` call with:

```rust
let storyboard_path = tmp_dir.join("action-guide/storyboard.png");
let storyboard_result = if let Some(image) = action.storyboard_image.as_ref() {
    write_optional_storyboard_image(&storyboard_path, image).map_err(|error| error.to_string())
} else {
    rollshot_action::export_storyboard(
        action.guide,
        action.store,
        rollshot_action::StoryboardOptions::default(),
        &storyboard_path,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
};
if let Err(error) = storyboard_result {
    warnings.push(IssuePackWarning {
        code: "storyboard_export_failed".to_string(),
        message: format!("Storyboard export failed: {error}"),
    });
}
```

Add this private helper near `build_folder(...)`:

```rust
#[cfg(feature = "action-guide")]
fn write_optional_storyboard_image(
    path: &Path,
    image: &RgbaImage,
) -> Result<(), image::ImageError> {
    let tmp = path.with_extension("png.tmp");
    match image.save_with_format(&tmp, image::ImageFormat::Png) {
        Ok(()) => {
            if let Err(error) = std::fs::rename(&tmp, path) {
                let _ = std::fs::remove_file(&tmp);
                return Err(image::ImageError::IoError(error));
            }
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            Err(error)
        }
    }
}
```

- [ ] **Step 3: Supply annotated image from Timeline Workspace**

In `timeline_workspace/update.rs`, update the existing helper that builds `ActionGuideExportSource` so it renders annotated Storyboard once:

```rust
fn timeline_issue_pack_action(
    state: &TimelineWorkspace,
) -> crate::issue_pack::ActionGuideExportSource<'_> {
    let include_gif = state
        .issue_pack
        .as_ref()
        .is_some_and(|dialog| dialog.include_gif);
    let storyboard_image =
        render_timeline_storyboard(state, StoryboardOptions::default()).ok().map(|r| r.image);
    crate::issue_pack::ActionGuideExportSource {
        guide: &state.guide,
        store: &state.store,
        region: state.region,
        capability: state.capability,
        source_kind: state.source_kind,
        include_gif,
        storyboard_image,
    }
}
```

- [ ] **Step 4: Add integration test for supplied Storyboard image**

Add this test inside the existing `#[cfg(all(test, feature = "action-guide"))] mod action_guide_tests` in `issue_pack.rs`:

```rust
    #[test]
    fn action_guide_issue_pack_uses_supplied_storyboard_image() {
        let (input, guide, store, region, capability, source_kind) = action_input();
        let temp = tempfile::tempdir().unwrap();
        let storyboard = image::RgbaImage::from_pixel(16, 16, image::Rgba([17, 34, 51, 255]));
        let source = ActionGuideExportSource {
            guide: &guide,
            store: &store,
            region,
            capability,
            source_kind,
            include_gif: false,
            storyboard_image: Some(storyboard),
        };

        let result =
            export_folder_with_action_guide(&input, Some(source), temp.path()).expect("issue pack");

        let decoded = image::ImageReader::open(result.directory.join("action-guide/storyboard.png"))
            .unwrap()
            .decode()
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [17, 34, 51, 255]);
    }
```

- [ ] **Step 5: Run Issue Pack tests**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack --features action-guide
```

Expected: PASS.

- [ ] **Step 6: Commit Task 5**

Run:

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(issue-pack): use annotated action guide storyboard"
```

Expected: commit succeeds with only Task 5 files staged.

---

### Task 6: Final Verification And Manual Smoke

**Files:**
- No code changes unless verification exposes failures.

**Interfaces:**
- Consumes: completed Tasks 1-5.
- Produces: verified P4 slice.

- [ ] **Step 1: Format check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 2: Focused crate tests**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
rtk cargo test -p rollshot-app timeline_workspace --features action-guide
rtk cargo test -p rollshot-app issue_pack --features action-guide
```

Expected: PASS.

- [ ] **Step 3: Wider app test pass**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide
```

Expected: PASS.

- [ ] **Step 4: Clippy if focused tests pass**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Manual desktop smoke**

Run the app with the Action Guide path available in the normal project workflow. Verify:

```text
1. Record an Action Guide with at least one step.
2. Select a step.
3. Click Annotate Step.
4. Click-drag a number callout.
5. Click Done.
6. Click Preview Storyboard and confirm the callout appears.
7. Export Storyboard and confirm the PNG includes the callout.
8. Export Bug Report and confirm action-guide/storyboard.png includes the callout.
9. Confirm action-guide/keyframes/*.png remain unannotated originals.
10. Replace the step keyframe and confirm the banner says annotations were cleared.
```

Expected: all ten checks pass.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-09-action-guide-step-annotations.md`.

Two execution options:

**1. Subagent-Driven (recommended)** - Dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** - Execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
