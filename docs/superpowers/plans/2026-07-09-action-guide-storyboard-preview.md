# Action Guide Storyboard Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an explicit Timeline Workspace preview modal for the current Action Guide Storyboard before the user exports the PNG.

**Architecture:** First split `rollshot-action` Storyboard rendering into an in-memory render API plus the existing file-writing export wrapper. Then store a preview image handle in `TimelineWorkspace` state and render it in an iced `stack + opaque` modal using the existing Timeline modal pattern.

**Tech Stack:** Rust, `rollshot-action`, `rollshot-app` with the `action-guide` feature, `image::RgbaImage`, iced 0.14 built-in widgets, Cargo tests through `rtk`.

## Global Constraints

- This plan implements PRD Phase P2 only: Storyboard preview before export.
- P1 is already complete in commit `69eff2ef2dc4f99279f488e6778f2c1f3d4c0be0`.
- Keep `Export Storyboard` behavior and file output unchanged.
- Preview must use reviewed `Guide` state: current step order, titles, deletions, and keyframe replacements.
- Preview must not mutate `Guide`, `FrameStore`, export files, Issue Pack state, or selection state.
- Empty guide and missing keyframe errors must be recoverable and leave the Timeline Workspace open.
- Use smaller preview render options than export: `max_width = 800`, `max_canvas_pixels = 12_000_000`, with other options from `StoryboardOptions::default()`.
- Do not add captions, annotations, layout controls, copy-to-clipboard, caching, async cloning, or agent behavior in this phase.
- Do not introduce custom iced widgets or custom overlays. Use built-in widgets and the existing `stack + opaque` modal style.
- Runtime diagnostics in product paths must use `tracing` with explicit `rollshot::*` targets.

---

## File Structure

- Modify `crates/rollshot-action/src/storyboard.rs`
  - Add `StoryboardRenderResult`.
  - Add `render_storyboard(...)`.
  - Keep `export_storyboard(...)` as a file-writing wrapper.
  - Move current canvas assembly logic into the render function.
  - Add render-specific tests.
- Modify `crates/rollshot-action/src/lib.rs`
  - Re-export `render_storyboard` and `StoryboardRenderResult`.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`
  - Add preview modal state.
  - Reuse `build_handle(...)` for rendered preview images.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`
  - Add preview request/close messages.
  - Render preview synchronously with bounded preview options.
  - Add update tests for success, error, and state refresh after edits.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`
  - Add `Preview Storyboard` button.
  - Add preview modal rendering.
  - Extend existing view smoke test to cover preview modal state.

No new crates or modules are needed.

---

## Review Lock-In

### Scope Challenge

The smallest shippable unit is the preview modal. It requires an in-memory render API because the current renderer only writes a PNG to disk, and preview should not create user-visible files or temp export artifacts. It does not require captions, annotations, copy image, layout controls, or live updates while the modal is open.

### Key Assumptions

- `Guide` and `FrameStore` are not `Clone`, so preview rendering stays synchronous in `update(...)` for this phase.
- The preview render is explicitly user-triggered and bounded to `max_width = 800`, which is acceptable for the expected 3 to 8 step workflow. If profiling later shows UI stalls, a later phase can add a snapshot type for async rendering.
- The preview should refresh when opened, not update live while already open. After a title or keyframe change, closing and reopening preview shows current state.
- Pressing `Export PNG` inside the modal can reuse `Message::ExportStoryboardRequested`; that closes the preview first, then opens the save dialog.

### What Already Exists

- `rollshot_action::export_storyboard(...)` renders the reviewed `Guide + FrameStore` into a PNG and already enforces empty-guide, missing-keyframe, and canvas-size errors.
- `TimelineWorkspace` already owns `guide`, `store`, current selection state, keyframe handles, and a reusable `build_handle(...)` helper.
- `timeline_workspace/view.rs` already uses iced `stack`, `opaque`, `mouse_area`, and `container::rounded_box` for modals.
- `timeline_workspace/update.rs` already has export tests for guide, GIF, MP4, Storyboard, and Issue Pack flows.

### NOT In Scope

- Storyboard captions.
- Per-step annotations.
- Storyboard layout modes.
- Copy Storyboard.
- Issue Pack changes.
- Background rendering or preview cache invalidation.
- Toolbar/menu consolidation.
- Changes to Linux/macOS capture overlay behavior.

### Test Coverage Table

```text
Task / behavior                                             Unit  Integ  UI smoke  Manual only
----------------------------------------------------------  ----  -----  --------  -----------
Task 1 / render_storyboard returns image metadata            yes   no     no        no
Task 1 / export_storyboard still writes identical PNG path   yes   no     no        no
Task 1 / empty guide and missing keyframe stay recoverable   yes   no     no        no
Task 2 / preview request stores image handle and metadata    yes   no     no        no
Task 2 / preview error sets banner and leaves state usable   yes   no     no        no
Task 2 / preview close clears modal state                    yes   no     no        no
Task 2 / reopening after title/keyframe edits refreshes      yes   no     no        no
Task 3 / header and preview modal view build                 no    no     yes       no
Task 3 / modal Export PNG reuses existing export flow        no    no     yes       no
Final / app and action test suites                           no    yes    no        no
Final / manual desktop preview interaction                   no    no     no        yes
```

### Failure Modes

- Empty guide: `render_storyboard(...)` returns `StoryboardError::Empty`; Timeline update sets `state.message = Some("Storyboard preview failed: ...")` and leaves `state.storyboard_preview = None`.
- Missing keyframe: `render_storyboard(...)` returns `StoryboardError::KeyframeMissing`; Timeline update logs `rollshot::action::preview`, shows a recoverable banner, and leaves the workspace open.
- Canvas too large: preview uses smaller options, but still handles `StoryboardError::CanvasTooLarge` as a recoverable preview error.
- Export regression: `export_storyboard(...)` remains the only writer and reuses `render_storyboard(...)`, so existing export tests continue to verify file output and temporary-file cleanup.
- Stale preview after edits while modal is open: not supported in P2. The modal is a snapshot. Closing and reopening regenerates from current state; this is covered by an update test.

### Parallelization

Task 1 can be implemented independently. Tasks 2 and 3 both touch Timeline Workspace files and should be sequential after Task 1 to avoid conflicts.

---

### Task 1: Extract In-Memory Storyboard Render API

**Files:**
- Modify: `crates/rollshot-action/src/storyboard.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/storyboard.rs`

**Interfaces:**
- Consumes: existing `Guide`, `FrameStore`, `StoryboardOptions`, and `StoryboardError`.
- Produces:
  - `pub struct StoryboardRenderResult { pub image: RgbaImage, pub width: u32, pub height: u32, pub step_count: usize }`
  - `pub fn render_storyboard(guide: &Guide, store: &FrameStore, opts: StoryboardOptions) -> Result<StoryboardRenderResult, StoryboardError>`
  - Existing `pub fn export_storyboard(...) -> Result<StoryboardExportResult, StoryboardError>` remains source-compatible.

- [ ] **Step 1: Write failing render API tests**

Add these tests inside `#[cfg(test)] mod tests` in `crates/rollshot-action/src/storyboard.rs`:

```rust
    #[test]
    fn renders_storyboard_in_memory_without_writing_a_file() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 2);
        assert!(guide.rename(1, "Open settings".to_string()));
        assert!(guide.rename(2, "Save changes".to_string()));

        let result = render_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render storyboard");

        assert_eq!(result.width, 320);
        assert_eq!(result.image.width(), result.width);
        assert_eq!(result.image.height(), result.height);
        assert_eq!(result.step_count, 2);
        assert!(
            result
                .image
                .pixels()
                .any(|pixel| pixel.0 != [255, 255, 255, 255]),
            "render should contain labels/cards/images"
        );
    }

    #[test]
    fn render_empty_guide_is_rejected() {
        let recording = recording();
        let guide = Guide::from_candidates(Vec::new());

        let result = render_storyboard(&guide, &recording.store, StoryboardOptions::default());

        assert!(matches!(result, Err(StoryboardError::Empty)));
    }

    #[test]
    fn render_missing_keyframe_is_rejected() {
        let store = FrameStore::new(StoreConfig::default());
        let guide = guide_with_steps(999, 1);

        let result = render_storyboard(&guide, &store, StoryboardOptions::default());

        assert!(matches!(
            result,
            Err(StoryboardError::KeyframeMissing { index: 1 })
        ));
    }
```

- [ ] **Step 2: Run tests to verify the new API is missing**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: FAIL to compile with errors equivalent to:

```text
cannot find function `render_storyboard` in this scope
```

- [ ] **Step 3: Add render result type and move canvas assembly into render_storyboard**

In `crates/rollshot-action/src/storyboard.rs`, add this type after `StoryboardExportResult`:

```rust
#[derive(Debug, Clone)]
pub struct StoryboardRenderResult {
    pub image: RgbaImage,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}
```

Replace the body of `export_storyboard(...)` with a wrapper and add `render_storyboard(...)` containing the current canvas-building logic:

```rust
pub fn export_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
    out_path: &Path,
) -> Result<StoryboardExportResult, StoryboardError> {
    let rendered = render_storyboard(guide, store, opts)?;
    write_png_atomic(out_path, &rendered.image)?;
    Ok(StoryboardExportResult {
        path: out_path.to_path_buf(),
        width: rendered.width,
        height: rendered.height,
        step_count: rendered.step_count,
    })
}

pub fn render_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError> {
    if guide.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let canvas_width = opts.max_width;
    let card_width = canvas_width
        .checked_sub(opts.outer_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;
    let content_width = card_width
        .checked_sub(opts.card_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;

    let mut cards = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let retained = store
            .retained(step.keyframe)
            .ok_or(StoryboardError::KeyframeMissing { index: i + 1 })?;
        let image = downscale(&retained.image, content_width);
        let label = step_label(i + 1, &step.title, opts.show_titles);
        let label = fit_label(&label, content_width as f32);
        let (_, label_height) = measure_block(&label, LABEL_FONT_PX, true);
        let label_height = label_height.ceil() as u32;
        let card_height = opts
            .card_padding
            .checked_mul(2)
            .and_then(|height| height.checked_add(label_height))
            .and_then(|height| height.checked_add(LABEL_GAP))
            .and_then(|height| height.checked_add(image.height()))
            .ok_or(StoryboardError::CanvasTooLarge)?;
        cards.push(Card {
            label,
            image,
            height: card_height,
        });
    }

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

        let content_x = opts.outer_padding + opts.card_padding;
        let mut content_y = y + opts.card_padding;
        draw_text_block(
            &mut canvas,
            ImagePoint::new(content_x as f32, content_y as f32),
            &card.label,
            LABEL_FONT_PX,
            true,
            TEXT_COLOR,
        );
        let (_, label_height) = measure_block(&card.label, LABEL_FONT_PX, true);
        content_y += label_height.ceil() as u32 + LABEL_GAP;
        image::imageops::replace(
            &mut canvas,
            &card.image,
            i64::from(content_x),
            i64::from(content_y),
        );

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

- [ ] **Step 4: Re-export the render API**

In `crates/rollshot-action/src/lib.rs`, replace the storyboard export line with:

```rust
pub use storyboard::{
    export_storyboard, render_storyboard, StoryboardExportResult, StoryboardOptions,
    StoryboardRenderResult,
};
```

- [ ] **Step 5: Run action storyboard tests**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: PASS.

- [ ] **Step 6: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-action/src/storyboard.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "refactor(action): expose storyboard render result"
```

---

### Task 2: Add Timeline Preview State And Update Logic

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: `rollshot_action::render_storyboard`, `StoryboardOptions`, `TimelineWorkspace::guide`, `TimelineWorkspace::store`, and `build_handle(...)`.
- Produces:
  - `pub(crate) struct StoryboardPreviewState`
  - `TimelineWorkspace { storyboard_preview: Option<StoryboardPreviewState>, ... }`
  - `Message::PreviewStoryboardRequested`
  - `Message::PreviewStoryboardClosed`

- [ ] **Step 1: Write failing update tests**

Add these tests inside `#[cfg(test)] mod tests` in `crates/rollshot-app/src/timeline_workspace/update.rs`:

```rust
    #[test]
    fn preview_storyboard_request_stores_rendered_preview() {
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::PreviewStoryboardRequested);

        let preview = state.storyboard_preview.as_ref().expect("preview state");
        assert_eq!(preview.step_count, state.guide.steps().len());
        assert_eq!(preview.width, 800);
        assert!(preview.height > 0);
        assert!(state.message.is_none(), "unexpected banner: {:?}", state.message);
    }

    #[test]
    fn preview_storyboard_empty_guide_sets_recoverable_message() {
        let mut state = ws(synthetic_recording(0));

        let _ = update(&mut state, Message::PreviewStoryboardRequested);

        assert!(state.storyboard_preview.is_none());
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|message| message.contains("Storyboard preview failed")),
            "failure banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn preview_storyboard_close_clears_preview_state() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::PreviewStoryboardRequested);
        assert!(state.storyboard_preview.is_some());

        let _ = update(&mut state, Message::PreviewStoryboardClosed);

        assert!(state.storyboard_preview.is_none());
    }

    #[test]
    fn preview_storyboard_reopen_reflects_renamed_steps() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::PreviewStoryboardRequested);
        let first_height = state.storyboard_preview.as_ref().unwrap().height;

        let _ = update(&mut state, Message::PreviewStoryboardClosed);
        let _ = update(
            &mut state,
            Message::TitleChanged("A much longer title that changes label measurement".to_string()),
        );
        let _ = update(&mut state, Message::PreviewStoryboardRequested);

        let second = state.storyboard_preview.as_ref().expect("preview state");
        assert_eq!(second.step_count, state.guide.steps().len());
        assert_eq!(second.width, 800);
        assert!(second.height >= first_height);
    }
```

- [ ] **Step 2: Run tests to verify state and messages are missing**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update
```

Expected: FAIL to compile with errors equivalent to:

```text
no variant or associated item named `PreviewStoryboardRequested` found for enum `Message`
no field `storyboard_preview` on type `TimelineWorkspace`
```

- [ ] **Step 3: Add preview state to TimelineWorkspace**

In `crates/rollshot-app/src/timeline_workspace/mod.rs`, add this state type near `IssuePackDialog` and `FfmpegSetupDialog`:

```rust
#[derive(Debug, Clone)]
pub(crate) struct StoryboardPreviewState {
    pub handle: iced::widget::image::Handle,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}
```

Add this field to `TimelineWorkspace`:

```rust
    /// Storyboard preview modal state, if open.
    pub(crate) storyboard_preview: Option<StoryboardPreviewState>,
```

Initialize it in `TimelineWorkspace::new(...)`:

```rust
            storyboard_preview: None,
```

- [ ] **Step 4: Add preview messages and imports**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, change the `rollshot_action` import to include `render_storyboard`:

```rust
use rollshot_action::{
    export_gif, export_guide, export_storyboard, export_video, render_storyboard, GifOptions,
    StoryboardOptions, VideoOptions,
};
```

Add these variants to `Message` near the existing Storyboard export messages:

```rust
    PreviewStoryboardRequested,
    PreviewStoryboardClosed,
```

- [ ] **Step 5: Add preview options helper**

Add this helper near the picker helpers in `update.rs`:

```rust
fn storyboard_preview_options() -> StoryboardOptions {
    StoryboardOptions {
        max_width: 800,
        max_canvas_pixels: 12_000_000,
        ..StoryboardOptions::default()
    }
}
```

- [ ] **Step 6: Implement update handling**

Add these match arms before `Message::ExportStoryboardRequested`:

```rust
        Message::PreviewStoryboardRequested => {
            state.message = None;
            match render_storyboard(&state.guide, &state.store, storyboard_preview_options()) {
                Ok(rendered) => {
                    tracing::info!(
                        target: "rollshot::action::preview",
                        steps = rendered.step_count,
                        width = rendered.width,
                        height = rendered.height,
                        "storyboard preview rendered"
                    );
                    state.storyboard_preview = Some(super::StoryboardPreviewState {
                        handle: super::build_handle(&rendered.image),
                        width: rendered.width,
                        height: rendered.height,
                        step_count: rendered.step_count,
                    });
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::preview",
                        %error,
                        "storyboard preview failed"
                    );
                    state.storyboard_preview = None;
                    state.message = Some(format!("Storyboard preview failed: {error}"));
                }
            }
            Task::none()
        }
        Message::PreviewStoryboardClosed => {
            state.storyboard_preview = None;
            Task::none()
        }
```

Change the existing export request arm so exporting from the modal closes the modal before opening the save dialog:

```rust
        Message::ExportStoryboardRequested => {
            state.message = None;
            state.storyboard_preview = None;
            Task::perform(
                pick_storyboard_save_path(picker_default_dir()),
                Message::ExportStoryboardPathChosen,
            )
        }
```

- [ ] **Step 7: Run Timeline update tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update
```

Expected: PASS.

- [ ] **Step 8: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): render storyboard preview state"
```

---

### Task 3: Add Preview Button And Modal View

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**
- Consumes: `TimelineWorkspace::storyboard_preview`, `Message::PreviewStoryboardRequested`, `Message::PreviewStoryboardClosed`, and existing `Message::ExportStoryboardRequested`.
- Produces:
  - Header button label `Preview Storyboard`.
  - Modal title `Preview Storyboard`.
  - Modal buttons `Export PNG` and `Close`.

- [ ] **Step 1: Write failing view smoke coverage**

Extend `view_builds_for_selected_empty_and_discard_states` in `crates/rollshot-app/src/timeline_workspace/view.rs` with this block before the empty-guide case:

```rust
        // Storyboard preview modal.
        let mut preview = ws(recording_from_frames(), InputCapability::SemanticEvents);
        crate::timeline_workspace::update::update(
            &mut preview,
            Message::PreviewStoryboardRequested,
        );
        assert!(preview.storyboard_preview.is_some());
        let _ = view(&preview);
```

- [ ] **Step 2: Run the view test before implementation**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::view
```

Expected: FAIL until Task 2 exists, or PASS if Task 2 is already implemented but the modal has not been asserted visually. Continue to Step 3 either way.

- [ ] **Step 3: Add header button**

In `header(...)`, insert `Preview Storyboard` before `Export Storyboard`:

```rust
        button(text("Preview Storyboard"))
            .on_press(Message::PreviewStoryboardRequested)
            .style(button::secondary),
        button(text("Export Storyboard"))
            .on_press(Message::ExportStoryboardRequested)
            .style(button::secondary),
```

- [ ] **Step 4: Layer preview modal in view**

In `view(...)`, after the Issue Pack modal block and before the FFmpeg modal block, add:

```rust
    let body = if state.storyboard_preview.is_some() {
        storyboard_preview_modal(body, state)
    } else {
        body
    };
```

- [ ] **Step 5: Add the preview modal function**

Add this function near the existing modal functions in `view.rs`:

```rust
fn storyboard_preview_modal<'a>(
    base: Element<'a, Message>,
    state: &'a TimelineWorkspace,
) -> Element<'a, Message> {
    let preview = state.storyboard_preview.as_ref().expect("checked by caller");
    let preview_image = image(preview.handle.clone())
        .width(Length::Fill)
        .height(Length::Shrink);

    let dialog_view = container(
        column![
            row![
                text("Preview Storyboard").size(18),
                Space::new().width(Length::Fill),
                text(format!(
                    "{} steps · {}×{}",
                    preview.step_count, preview.width, preview.height
                ))
                .size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            container(scrollable(preview_image))
                .width(Length::Fill)
                .height(Length::Fixed(520.0))
                .style(container::rounded_box),
            row![
                button(text("Export PNG"))
                    .on_press(Message::ExportStoryboardRequested)
                    .style(button::primary),
                button(text("Close")).on_press(Message::PreviewStoryboardClosed),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(760.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog_view))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_theme: &Theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.8,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..container::Style::default()
                }),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::PreviewStoryboardClosed),
    );

    stack![base, scrim].into()
}
```

- [ ] **Step 6: Run Timeline view tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::view
```

Expected: PASS.

- [ ] **Step 7: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(action): preview storyboard before export"
```

---

## Final Verification

- [ ] **Step 1: Run focused action tests**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: PASS.

- [ ] **Step 2: Run focused Timeline Workspace tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
```

Expected: PASS.

- [ ] **Step 3: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 4: Run clippy if the branch is otherwise green**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS. If this is too slow because of optional OCR or platform-specific lanes, record the exact failure and rerun the narrower feature-gated app/action clippy command selected by the maintainer.

- [ ] **Step 5: Manual UI smoke**

Run the Action Guide product path on a platform with the `action-guide` feature enabled, record a short guide, then verify:

```text
1. Preview Storyboard opens a modal.
2. The modal shows the current reviewed steps.
3. Close returns to the Timeline Workspace without losing edits.
4. Rename a step, reopen Preview Storyboard, and verify the preview refreshes.
5. Replace a keyframe, reopen Preview Storyboard, and verify the preview refreshes.
6. Export PNG from the modal opens the existing save dialog and writes the PNG.
7. Empty or broken guide preview failures show a banner and keep the workspace open.
```

---

## Self-Review

- Spec coverage: Covers PRD P2 acceptance criteria: preview from Timeline Workspace, current reviewed steps, renamed titles, replaced keyframes, recoverable failure, workspace remains open.
- Placeholder scan: No incomplete-marker text or unspecified implementation steps remain.
- Type consistency: `StoryboardRenderResult`, `StoryboardPreviewState`, and `Message::*` names are defined before they are consumed by later tasks.
- Scope check: P3 captions, P4 annotations, and P5 agent proposals are explicitly out of scope to keep this branch independently shippable.
