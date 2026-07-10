# Action Guide Copy Storyboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a non-blocking `Copy Image` action to the existing Action Guide Storyboard preview modal that writes the export-quality Storyboard to the system clipboard.

**Architecture:** Extract the existing arboard image write into an app-level helper shared by Result Workspace and Timeline Workspace. Snapshot reviewed Storyboard steps into an owned input, render it with export defaults in an iced background task, and protect modal feedback with an explicit operation-ID state machine.

**Tech Stack:** Rust, iced 0.14 `Task`, arboard 3.6.1 (resolved from the manifest's `"3.4"` requirement), `image::RgbaImage`, existing `rollshot_action::render_storyboard_steps`.

## Global Constraints

- Work on `feat/action-guide-copy-storyboard`; do not create a worktree.
- Prefix every shell command with `rtk`.
- Before modifying iced UI files, invoke the repository `iced-rs` skill and use iced 0.14 APIs.
- Keep one Copy entry point inside the existing Storyboard preview modal.
- Copy export-quality `StoryboardOptions::default()` output; never copy the reduced preview bitmap.
- Do not add platform-specific clipboard backends, automatic downsampling, temp files, or a second Storyboard renderer.
- Do not mutate Guide, FrameStore, keyframes, `ImageDocument`, annotations, undo/redo, or save/export state.
- Use stable `rollshot::app::storyboard_copy` tracing with structured fields; never log pixels, title/caption text, paths, or clipboard payloads.
- Check the shared Linux and macOS Timeline paths. If only one clipboard runtime can be tested, record the unchecked platform and risk.

## What Already Exists

- `result_workspace::actions::copy_image` already converts `RgbaImage` to arboard RGBA image data and writes it; move this code instead of creating another backend.
- Result Workspace already owns safe/original copy policy and `CopyFinished` messaging; preserve those behaviors and change only the helper path.
- `render_timeline_storyboard`, `render_storyboard_steps`, preview options, and export defaults already produce the exact Storyboard content; refactor input ownership without adding a renderer.
- `ActionGuidePresentation` already selects flattened annotated frames while `FrameStore` owns reviewed raw keyframes; reuse the same selection rules.
- The preview modal and its Export/Close actions already exist in shared Linux/macOS iced code.
- arboard 3.6.1 explicitly permits Clipboard instances on separate threads. Its Linux backend owns platform-specific clipboard hosting; retain the current `set_image` semantics rather than adding a Rollshot clipboard daemon.

## NOT in Scope

- Timeline-header or keyboard-shortcut Copy entry points: the preview modal is the single reviewed-output entry point.
- Reduced-resolution, adaptive, or user-selectable clipboard quality: Copy must match Export PNG defaults.
- Persistent global Clipboard ownership or a Rollshot clipboard daemon: existing arboard behavior remains authoritative.
- Clipboard formats other than one RGBA image: no Markdown, HTML, text, PDF, or file-reference payload.
- Compact/grid layout, batch sharing, telemetry, or clipboard history: none is required to copy the current Storyboard.
- Refactoring FrameStore or ImageDocument to `Arc`: synchronous snapshot clone/flatten remains bounded to an explicit Copy click.

---

## File Structure

- Create `crates/rollshot-app/src/image_clipboard.rs`: shared arboard image conversion/write boundary.
- Modify `crates/rollshot-app/src/main.rs`: register the app-level clipboard module.
- Modify `crates/rollshot-app/src/result_workspace/actions.rs`: remove the local clipboard implementation.
- Modify `crates/rollshot-app/src/result_workspace/update.rs`: call the shared helper without changing Result Workspace policy.
- Create `crates/rollshot-app/src/timeline_workspace/storyboard_copy.rs`: owned step snapshot, export-quality render, and injectable copy task.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`: register the module and add copy state/operation counter.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`: reuse the owned Storyboard input, start Copy, handle completion/retry/late results, and clear success feedback.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`: render modal-local Copy states and error feedback.

---

### Task 1: Shared Image Clipboard Boundary

**Files:**
- Create: `crates/rollshot-app/src/image_clipboard.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/result_workspace/actions.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**
- Consumes: `&image::RgbaImage`.
- Produces: `image_data(&RgbaImage) -> arboard::ImageData<'_>` and `copy_rgba_image(&RgbaImage) -> Result<(), String>`.

- [ ] **Step 1: Write failing pure conversion tests**

Add `mod image_clipboard;` to `main.rs`, then create `image_clipboard.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_data_preserves_dimensions_and_rgba_order() {
        let image = image::RgbaImage::from_raw(
            2,
            1,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
        ).unwrap();

        let data = image_data(&image);

        assert_eq!(data.width, 2);
        assert_eq!(data.height, 1);
        assert_eq!(data.bytes.as_ref(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn image_data_supports_empty_image_without_touching_clipboard() {
        let image = image::RgbaImage::new(0, 0);
        let data = image_data(&image);
        assert_eq!((data.width, data.height), (0, 0));
        assert!(data.bytes.is_empty());
    }
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `rtk cargo test -p rollshot-app image_clipboard::tests --no-default-features`

Expected: FAIL because `image_clipboard` and `image_data` do not exist.

- [ ] **Step 3: Add the shared helper**

Implement:

```rust
use std::borrow::Cow;

pub(crate) fn image_data(image: &image::RgbaImage) -> arboard::ImageData<'_> {
    arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Borrowed(image.as_raw().as_slice()),
    }
}

pub(crate) fn copy_rgba_image(image: &image::RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("clipboard error: {error}"))?;
    clipboard
        .set_image(image_data(image))
        .map_err(|error| format!("clipboard write error: {error}"))
}
```

- [ ] **Step 4: Run conversion tests and verify they pass**

Run: `rtk cargo test -p rollshot-app image_clipboard::tests --no-default-features`

Expected: PASS without opening a real clipboard.

- [ ] **Step 5: Move Result Workspace call sites**

Delete `copy_image` and its `Cow` import from `result_workspace/actions.rs`. Replace all three `super::actions::copy_image(...)` calls in `result_workspace/update.rs` with `crate::image_clipboard::copy_rgba_image(...)`. Do not change `copy_payload`, `copy_original_payload`, secure-redaction checks, `safe_output`, or `CopyFinished` handling.

- [ ] **Step 6: Run Result Workspace regression tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace --no-default-features
rtk cargo test -p rollshot-app image_clipboard --no-default-features
```

Expected: PASS; no headless test writes the system clipboard.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/image_clipboard.rs crates/rollshot-app/src/main.rs crates/rollshot-app/src/result_workspace/actions.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "refactor(app): share image clipboard helper"
```

---

### Task 2: Owned Storyboard Copy Pipeline

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/storyboard_copy.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: reviewed `Guide`, `FrameStore`, `ActionGuidePresentation`, `StoryboardOptions`, and a copy callback.
- Produces: `StoryboardCopyInput`, `StoryboardCopyResult`, `snapshot_storyboard`, `render_storyboard_input`, and `render_and_copy`.

- [ ] **Step 1: Write failing snapshot tests**

Register `mod storyboard_copy;` and create these types:

```rust
pub(crate) struct StoryboardCopyStep {
    pub index: usize,
    pub title: String,
    pub caption: Option<String>,
    pub image: image::RgbaImage,
}

pub(crate) struct StoryboardCopyInput {
    pub steps: Vec<StoryboardCopyStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StoryboardCopyResult {
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}
```

Add tests using the existing synthetic recording/presentation patterns:

```rust
fn workspace_with_steps(count: usize) -> super::TimelineWorkspace {
    super::TimelineWorkspace::new(
        crate::timeline_workspace::tests::synthetic_recording(count),
        rollshot_action::CaptureRegion { x: 0, y: 0, width: 32, height: 32 },
        rollshot_action::InputCapability::SemanticEvents,
        rollshot_action::InputSourceKind::LinuxEvdev,
    )
}

fn add_callout_to_first_step(state: &mut super::TimelineWorkspace) {
    let step = state.guide.steps()[0].clone();
    let document = state.presentation.document_for_step(&step, &state.store).unwrap();
    document.document.add_number_callout(
        rollshot_image_document::ImagePoint::new(2.0, 2.0),
        rollshot_image_document::ImagePoint::new(8.0, 8.0),
    );
}

#[test]
fn snapshot_preserves_reviewed_order_titles_and_trimmed_captions() {
    let mut state = workspace_with_steps(2);
    state.guide.rename(1, "Open Settings".into());
    state.guide.set_caption(1, "  Show the panel.  ".into());

    let input = snapshot_storyboard(&state.guide, &state.store, &state.presentation).unwrap();

    assert_eq!(input.steps.iter().map(|step| step.index).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(input.steps[0].title, "Open Settings");
    assert_eq!(input.steps[0].caption.as_deref(), Some("Show the panel."));
}

#[test]
fn snapshot_flattens_annotations_without_mutating_document() {
    let mut state = workspace_with_steps(1);
    add_callout_to_first_step(&mut state);
    let source = state.guide.steps()[0].source;
    let before = state.presentation.doc(source).unwrap().document.state_id();

    let input = snapshot_storyboard(&state.guide, &state.store, &state.presentation).unwrap();

    assert_ne!(input.steps[0].image, state.store.retained(state.guide.steps()[0].keyframe).unwrap().image);
    assert_eq!(state.presentation.doc(source).unwrap().document.state_id(), before);
}
```

Also add explicit empty-Guide and missing-keyframe tests expecting `StoryboardError::Empty` and `StoryboardError::KeyframeMissing`.

- [ ] **Step 2: Run snapshot tests and verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide storyboard_copy::tests`

Expected: FAIL because snapshot functions do not exist.

- [ ] **Step 3: Implement the owned snapshot**

Implement:

```rust
pub(crate) fn snapshot_storyboard(
    guide: &rollshot_action::Guide,
    store: &rollshot_action::FrameStore,
    presentation: &super::annotation::ActionGuidePresentation,
) -> Result<StoryboardCopyInput, rollshot_action::StoryboardError>
```

Return `Empty` before allocation. Iterate Guide order, resolve each retained keyframe, flatten only a matching document with committed annotations, clone raw retained images otherwise, trim captions, and store owned strings/images.

- [ ] **Step 4: Implement one renderer adapter and refactor existing callers**

Add:

```rust
pub(crate) fn render_storyboard_input(
    input: &StoryboardCopyInput,
    options: rollshot_action::StoryboardOptions,
) -> Result<rollshot_action::StoryboardRenderResult, rollshot_action::StoryboardError> {
    let steps = input.steps.iter().map(|step| rollshot_action::StoryboardStep {
        index: step.index,
        title: &step.title,
        caption: step.caption.as_deref(),
        image: &step.image,
    }).collect::<Vec<_>>();
    rollshot_action::render_storyboard_steps(&steps, options)
}
```

Refactor `render_timeline_storyboard` in `update.rs` to call `snapshot_storyboard` then `render_storyboard_input`. Preview and Export PNG must keep their existing option arguments and behavior.

- [ ] **Step 5: Write failing export-quality copy-task tests**

Add an injectable synchronous core and thin async wrapper:

```rust
pub(crate) fn render_and_copy_with(
    input: StoryboardCopyInput,
    copy: impl FnOnce(&image::RgbaImage) -> Result<(), String>,
) -> Result<StoryboardCopyResult, String>;

pub(crate) async fn render_and_copy(
    input: StoryboardCopyInput,
) -> Result<StoryboardCopyResult, String>;
```

Tests must assert the callback receives the exact pixels produced by `render_storyboard_input(input, StoryboardOptions::default())`, annotations are present, clipboard failure is returned with the `Couldn't copy Storyboard:` prefix, and a renderer error never invokes the callback. Use `Rc<RefCell<Option<RgbaImage>>>` for the synchronous test callback and `Cell<bool>` for the no-call assertion; do not access the real clipboard.

```rust
#[test]
fn copy_pipeline_matches_export_quality_pixels() {
    let input = one_step_input();
    let expected = render_storyboard_input(
        &input,
        rollshot_action::StoryboardOptions::default(),
    ).unwrap();
    let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = std::rc::Rc::clone(&captured);

    let result = render_and_copy_with(input, move |image| {
        *output.borrow_mut() = Some(image.clone());
        Ok(())
    }).unwrap();

    assert_eq!(captured.borrow().as_ref(), Some(&expected.image));
    assert_eq!((result.width, result.height), (expected.width, expected.height));
    assert_eq!(result.step_count, expected.step_count);
}

#[test]
fn renderer_failure_does_not_call_clipboard() {
    let called = std::cell::Cell::new(false);
    let result = render_and_copy_with(
        StoryboardCopyInput { steps: vec![] },
        |_| { called.set(true); Ok(()) },
    );
    assert!(result.is_err());
    assert!(!called.get());
}
```

Define `one_step_input()` in the same test module with a 4x4 synthetic RGBA image, title, and caption so the expected output is deterministic.

- [ ] **Step 6: Implement the copy task**

`render_and_copy_with` renders with `StoryboardOptions::default()`, maps the typed renderer error to a safe string, invokes the supplied callback once, and returns only `StoryboardCopyResult`. Construct the metadata before the callback returns, then drop `StoryboardRenderResult::image` inside the worker; never send the bitmap through an iced message.

`render_and_copy` must explicitly move synchronous CPU/clipboard work to Tokio's blocking pool:

```rust
pub(crate) async fn render_and_copy(
    input: StoryboardCopyInput,
) -> Result<StoryboardCopyResult, String> {
    tokio::task::spawn_blocking(move || {
        render_and_copy_with(input, crate::image_clipboard::copy_rgba_image)
    })
    .await
    .map_err(|_| "Storyboard copy worker failed.".to_string())?
}
```

Prefix clipboard failures inside `render_and_copy_with` with `Couldn't copy Storyboard:`. Do not use `arboard::SetExtLinux::wait()`: waiting forever would prevent the iced completion message, and the existing app helper already relies on arboard's default Linux hosting behavior.

- [ ] **Step 7: Run focused and regression tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide storyboard_copy
rtk cargo test -p rollshot-app --features action-guide preview_storyboard
rtk cargo test -p rollshot-app --features action-guide export_storyboard
```

Expected: PASS; preview remains smaller and Copy uses export defaults.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/storyboard_copy.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "refactor(action): add owned storyboard render input"
```

---

### Task 3: Copy Operation State Machine

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: Task 2 `snapshot_storyboard` and `render_and_copy`.
- Produces: `StoryboardCopyState` and copy request/completion/clear messages.

- [ ] **Step 1: Write failing state initialization and transition tests**

Define the desired enum in the tests:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoryboardCopyState {
    Idle,
    Copying { operation_id: u64 },
    Copied { operation_id: u64 },
    Failed { operation_id: u64, message: String },
}
```

Add tests proving a newly opened preview is `Idle`, the first request becomes `Copying { operation_id: 1 }`, a duplicate request does not increment the ID, matching success becomes `Copied`, and matching failure becomes `Failed`.

Use the existing `ws` helper in `update.rs` tests and add:

```rust
fn workspace_with_open_preview() -> TimelineWorkspace {
    let mut state = ws(synthetic_recording(1));
    let _ = update(&mut state, Message::PreviewStoryboardRequested);
    assert!(state.storyboard_preview.is_some());
    state
}

#[test]
fn storyboard_copy_state_starts_idle_when_preview_opens() {
    let state = workspace_with_open_preview();
    assert_eq!(
        state.storyboard_preview.unwrap().copy_state,
        StoryboardCopyState::Idle
    );
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide storyboard_copy_state_starts_idle_when_preview_opens`

Expected: FAIL because the preview state and messages do not contain Copy lifecycle state.

- [ ] **Step 3: Add state and messages**

Add `copy_state: StoryboardCopyState` to `StoryboardPreviewState` and `storyboard_copy_operation_id: u64` to `TimelineWorkspace`. Initialize preview state with `Idle` and the workspace counter with `0`.

Place this maintained state diagram in the enum's doc comment:

```text
Idle ───────────────► Copying(id) ──success──► Copied(id) ──delay──► Idle
 ▲                         │
 │                         └─failure────────► Failed(id) ──retry──► Copying(new id)
 └──────────────── modal close drops the entire preview state ────────────────
```

Add messages:

```rust
CopyStoryboardRequested,
CopyStoryboardFinished {
    operation_id: u64,
    result: Result<super::storyboard_copy::StoryboardCopyResult, String>,
},
ClearStoryboardCopyFeedback { operation_id: u64 },
```

- [ ] **Step 4: Implement request and completion guards**

On request:

1. Require an open preview whose state is not `Copying`.
2. Build a fresh snapshot; on snapshot failure allocate a new ID and store `Failed` without starting a task.
3. Increment the counter with `saturating_add(1)`.
4. Store `Copying { operation_id }`.
5. Start `Task::perform(render_and_copy(input), move |result| CopyStoryboardFinished { operation_id, result })`.

On completion, apply only when the preview still exists and its current state is `Copying` with the same ID. Success stores `Copied`, emits privacy-safe success tracing, and returns a two-second delayed task:

```rust
Task::perform(
    async { tokio::time::sleep(std::time::Duration::from_secs(2)).await },
    move |_| Message::ClearStoryboardCopyFeedback { operation_id },
)
```

Failure stores `Failed` and returns no delayed task. Clear applies only to matching `Copied`. Modal close drops the preview; late messages become no-ops.

- [ ] **Step 5: Add stale and retry tests**

Add exact tests for:

```rust
fn copy_result() -> super::storyboard_copy::StoryboardCopyResult {
    super::storyboard_copy::StoryboardCopyResult {
        width: 1200,
        height: 800,
        step_count: 1,
    }
}

#[test]
fn older_copy_completion_cannot_replace_newer_operation() {
    let mut state = workspace_with_open_preview();
    state.storyboard_copy_operation_id = 2;
    state.storyboard_preview.as_mut().unwrap().copy_state =
        StoryboardCopyState::Copying { operation_id: 2 };

    let _ = update(&mut state, Message::CopyStoryboardFinished {
        operation_id: 1,
        result: Ok(copy_result()),
    });

    assert_eq!(
        state.storyboard_preview.unwrap().copy_state,
        StoryboardCopyState::Copying { operation_id: 2 }
    );
}

#[test]
fn completion_after_preview_close_is_ignored() {
    let mut state = workspace_with_open_preview();
    let _ = update(&mut state, Message::PreviewStoryboardClosed);
    let _ = update(&mut state, Message::CopyStoryboardFinished {
        operation_id: 1,
        result: Ok(copy_result()),
    });
    assert!(state.storyboard_preview.is_none());
}
```

Also test Retry allocates a newer ID and an older delayed clear cannot erase a newer `Copied` or `Failed` state.

- [ ] **Step 6: Run state tests and commit**

Run: `rtk cargo test -p rollshot-app --features action-guide storyboard_copy`

Expected: PASS.

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): manage storyboard copy lifecycle"
```

---

### Task 4: Preview Modal Copy Controls

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: Task 3 `StoryboardCopyState` and messages.
- Produces: modal-local Copy/Copying/Copied/Retry controls and failure text.

- [ ] **Step 1: Invoke iced 0.14 guidance before UI edits**

Invoke the repository `iced-rs` skill. Confirm existing `button`, `column`, `row`, `Space`, and modal stack patterns; do not introduce `iced::advanced::Widget` or a second modal.

- [ ] **Step 2: Extract and test pure modal presentation state**

Add in `view.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StoryboardCopyPresentation<'a> {
    label: &'a str,
    enabled: bool,
    error: Option<&'a str>,
}

fn storyboard_copy_presentation(state: &super::StoryboardCopyState) -> StoryboardCopyPresentation<'_>;
```

Tests:

```rust
#[test]
fn copy_presentation_matches_lifecycle() {
    assert_eq!(storyboard_copy_presentation(&StoryboardCopyState::Idle).label, "Copy Image");
    assert!(!storyboard_copy_presentation(&StoryboardCopyState::Copying { operation_id: 1 }).enabled);
    assert_eq!(storyboard_copy_presentation(&StoryboardCopyState::Copied { operation_id: 1 }).label, "Copied");
    let failed = StoryboardCopyState::Failed { operation_id: 1, message: "clipboard unavailable".into() };
    assert_eq!(storyboard_copy_presentation(&failed).label, "Retry");
    assert_eq!(storyboard_copy_presentation(&failed).error, Some("clipboard unavailable"));
}
```

- [ ] **Step 3: Run presentation test and verify it fails**

Run: `rtk cargo test -p rollshot-app --features action-guide copy_presentation_matches_lifecycle`

Expected: FAIL because the presentation helper does not exist.

- [ ] **Step 4: Implement modal controls**

In `storyboard_preview_modal`:

- render preview-local error text immediately above the footer only when `presentation.error` exists;
- add the Copy button before Export PNG;
- use `.on_press_maybe(presentation.enabled.then_some(Message::CopyStoryboardRequested))`;
- keep Export PNG enabled regardless of Copy state;
- keep Close and scrim close available while copying;
- preserve the existing preview image, dimensions, step count, width, height, and styling.

Use the existing button styles. Do not add a card, spinner dependency, or Timeline global banner.

- [ ] **Step 5: Run UI/state tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide copy_presentation
rtk cargo test -p rollshot-app --features action-guide storyboard_preview
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/view.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): copy storyboard from preview"
```

---

### Task 5: Verification and Platform Clipboard Smoke Tests

**Files:**
- Modify only files required to correct failures introduced by Tasks 1-4.

**Interfaces:**
- Consumes: the complete Copy Storyboard vertical slice.
- Produces: verified shared clipboard regression safety and documented platform runtime coverage.

- [ ] **Step 1: Run focused and cross-crate tests**

```bash
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 2: Run workspace clippy**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS with no warnings.

- [ ] **Step 3: Verify Linux clipboard runtime**

On the active Linux/Wayland product path:

1. Open an Action Guide with titles, a caption, and at least one annotation.
2. Open Preview Storyboard and click Copy Image.
3. Confirm the modal remains responsive and shows `Copied`.
4. Paste into an image-aware target.
5. Export PNG and compare dimensions and visible pixels with the pasted image.
6. Trigger a second Copy and close the modal immediately; confirm no stale UI appears.

Record pass/fail and target application only. Do not record image contents.

- [ ] **Step 4: Verify macOS clipboard runtime when available**

Repeat Step 3 through the shared macOS Timeline path and paste into Preview, Messages, or an issue editor. If macOS is unavailable, record it as unchecked in the final report; do not claim cross-platform runtime completion.

- [ ] **Step 5: Inspect the final diff**

Run:

```bash
rtk git diff --check
rtk git status --short
rtk git diff --stat main...HEAD
```

Expected: only the planned Copy Storyboard files plus necessary verification fixes are changed.

- [ ] **Step 6: Commit verification fixes only when needed**

If verification required code changes, stage each exact path shown by `rtk git status --short` and commit:

```bash
rtk git commit -m "fix(action): harden storyboard clipboard copy"
```

If no fix was required, do not create an empty commit.

---

## Test Coverage Matrix

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / RGBA-to-arboard dimensions and byte order | yes | no | no | no |
| Task 1 / Result Workspace helper migration | existing unit/state tests | app module regression | no | real paste unchanged |
| Task 2 / reviewed order, captions, raw/flattened frames | yes | Timeline synthetic recording | no | no |
| Task 2 / Copy pixels equal export-quality render | exact synthetic pixel equality | renderer adapter | no | no |
| Task 2 / renderer and clipboard errors | yes | injectable callback | no | no |
| Task 3 / ID allocation, duplicate suppression, retry, late completion/clear | yes | Timeline update state | no | no |
| Task 4 / modal labels, enablement, and local error | yes | preview modal state | no | visual layout |
| Task 5 / Linux and macOS clipboard interoperability | prior automated suites | full app flow | platform paste smoke | target-app behavior |

Automated tests use synthetic images, synthetic recordings, direct messages, and injected copy callbacks. They do not require a compositor, display server, native clipboard, wall-clock sleep, network, or real capture session.

## Failure Modes

| New codepath | Production failure | Test | Handling | User result |
|---|---|---|---|---|
| Shared clipboard conversion | dimensions/bytes are mapped incorrectly | Task 1 Steps 1-4 | pure borrowed `ImageData` conversion | prevented by exact unit assertion |
| Result Workspace helper move | safe/original copy policy changes accidentally | Task 1 Steps 5-6 | call sites only; payload selection untouched | existing messages and policy remain |
| Storyboard snapshot | Guide empty or reviewed keyframe missing | Task 2 Steps 1-3 | typed `StoryboardError` before task launch | modal-local retryable failure |
| Export-quality render | final canvas exceeds 24M pixels | Task 2 Steps 5-7 | renderer error, callback not invoked | `Failed` with Retry; no downsample |
| Blocking worker | worker panics or Tokio join fails | Task 2 Step 6 | fixed `Storyboard copy worker failed.` error | modal-local failure |
| Clipboard backend | unavailable, occupied, or rejects image | Task 2 Steps 5-7; Task 5 smoke | prefixed recoverable error | `Couldn't copy Storyboard` and Retry |
| Concurrent/retried Copy | older completion arrives after newer request | Task 3 Step 5 | operation-ID equality guard | old result silently ignored by design |
| Modal close | task finishes after preview state is dropped | Task 3 Step 5 | absent modal guard | no stale UI resurrection |
| Delayed success clear | timer from old success fires during newer state | Task 3 Step 5 | operation-ID equality guard | newer state remains visible |
| Linux clipboard hosting | pasted data disappears or target rejects image/png | Task 5 Step 3 | arboard default hosting plus runtime paste check | implementation cannot claim Linux completion if smoke fails |
| macOS pasteboard | background-thread write or target paste fails | Task 5 Step 4 | arboard Send/Sync contract plus runtime paste check | platform risk explicitly reported |

No identified failure is both untested and silently unhandled.

## Performance and Resource Review

- Snapshot clone/flatten is synchronous and explicit; it is the minimum ownership transfer needed for a `'static` iced task without refactoring FrameStore/ImageDocument to shared ownership.
- Storyboard layout, rasterization, and arboard write run in `tokio::task::spawn_blocking`, not directly inside an async future or iced update.
- The copy pipeline owns one image per reviewed step and one final canvas. The final canvas remains capped at 24,000,000 pixels (about 96 MiB RGBA).
- The final bitmap is consumed inside the blocking worker and only width/height/step count return through the iced message queue.
- `ImageData` borrows the final bitmap for `set_image`; Rollshot creates no extra RGBA copy. Linux arboard may internally encode PNG, which is platform-owned and bounded by the final-canvas cap.
- Duplicate clicks are suppressed while `Copying`, so one modal cannot create parallel full-canvas renders or competing clipboard writes.
- Bubble/layout algorithms and capture/stitching hot paths are unchanged; no benchmark suite is required.

## Execution Strategy

Sequential execution, no parallelization opportunity. All behavioral tasks modify `crates/rollshot-app`, Tasks 2-4 repeatedly touch Timeline Workspace state/update/view, and each task consumes the previous task's interface. Fresh subagents may execute tasks one at a time, but concurrent edits or commits in the shared checkout would create avoidable conflicts. No root workspace or distribution artifact changes are planned.
