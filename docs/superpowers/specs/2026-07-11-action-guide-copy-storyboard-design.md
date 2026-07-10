# Action Guide Copy Storyboard Design

**Date:** 2026-07-11
**Status:** Approved design
**Scope:** Copy one export-quality Action Guide Storyboard from the existing preview modal

## Goal

Let a user copy the reviewed Action Guide Storyboard directly from its preview modal and paste the full-quality image into Slack, Linear, GitHub, Messages, Preview, or another image-aware destination without first saving a PNG.

## Product Decisions

- The only new entry point is inside the existing Storyboard preview modal.
- The copied bitmap uses export-quality `StoryboardOptions::default()`, not the smaller preview bitmap.
- Copy runs in a background iced task so full-resolution rendering and the clipboard backend do not block the UI update loop.
- Success and failure feedback stays inside the preview modal.
- Success does not close the modal.
- The modal never permanently retains a full-resolution Storyboard bitmap.
- Linux and macOS reuse the existing arboard image-clipboard path.

## Non-Goals

- A Timeline header Copy action.
- Multiple Copy entry points.
- Copying the reduced preview bitmap.
- Automatic downsampling or quality selection.
- Clipboard history or clipboard format selection.
- Text, Markdown, HTML, PDF, or file-reference clipboard payloads.
- Replacing Export PNG.
- Adding a new platform-specific clipboard backend.
- Compact or grid Storyboard layouts.

## Architecture

```text
Preview Storyboard requested
        |
        v
render preview-size Storyboard
        |
        v
StoryboardPreviewState
  iced image handle
  dimensions / step count
  copy_state = Idle
        |
   user clicks Copy
        |
        v
snapshot current reviewed Storyboard inputs
  GuideStep metadata
  flattened annotated keyframes
        |
        v
iced Task::perform
  render_storyboard_steps(export defaults)
        |
        v
shared image_clipboard::copy_rgba_image
  arboard::Clipboard::set_image
        |
        v
CopyCompleted { operation_id, result }
        |
        +-- success -> Copied -> delayed Idle
        +-- failure -> Failed + Retry
```

Ownership boundaries:

- `rollshot-action` remains the deterministic Storyboard renderer. This phase adds no second layout or raster path.
- `rollshot-app::image_clipboard` becomes the shared app-level image clipboard boundary.
- Result Workspace calls the shared helper with no behavior change.
- Timeline Workspace snapshots reviewed Storyboard input, starts the background task, and owns modal state transitions.
- The preview modal stores only its display handle, metadata, and copy lifecycle. It does not store the export-quality bitmap.
- Original keyframes and `ImageDocument` history are never mutated by preview, copy, retry, success, or failure.

## Shared Clipboard Boundary

Move the current arboard implementation from `result_workspace::actions::copy_image` into an app-level module:

```rust
pub(crate) fn copy_rgba_image(image: &image::RgbaImage) -> Result<(), String>;
```

The helper:

- Creates `arboard::Clipboard`.
- Creates `arboard::ImageData` with the exact image width, height, and RGBA row bytes.
- Borrows the `RgbaImage` byte buffer for the duration of `set_image`.
- Returns a recoverable error when clipboard creation or image writing fails.
- Never logs pixels or clipboard payloads.

The Result Workspace changes only its call site. Existing safe-copy/original-copy policy, messages, and flattened payload selection remain unchanged.

## Copy Input Snapshot

The background task cannot borrow `TimelineWorkspace`, so Copy builds an owned snapshot at click time. The input is:

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
```

Snapshot rules:

- Preserve the current reviewed Guide order.
- Copy the current title and trimmed optional caption.
- Use `ImageDocument::flatten()` when a matching step document has committed annotations.
- Otherwise clone the retained reviewed keyframe.
- Fail before starting the background render if the Guide is empty or a reviewed keyframe is missing.
- Do not retain references to Guide, FrameStore, presentation state, or modal state.

The modal blocks Timeline editing while open, but snapshotting at Copy time keeps ownership and retry semantics explicit. Retry creates a fresh snapshot.

## Background Render and Clipboard Write

The task converts owned steps into borrowed `StoryboardStep` inputs and calls:

```rust
render_storyboard_steps(&steps, StoryboardOptions::default())
```

It then passes `StoryboardRenderResult::image` to `copy_rgba_image`. It does not encode PNG, write a temporary file, or alter export/save state.

Preview rendering continues using:

```rust
StoryboardOptions {
    max_width: 800,
    max_canvas_pixels: 12_000_000,
    ..StoryboardOptions::default()
}
```

Copy rendering therefore matches Export PNG defaults rather than the display bitmap.

## Modal State

Use one explicit state enum:

```rust
pub(crate) enum StoryboardCopyState {
    Idle,
    Copying { operation_id: u64 },
    Copied { operation_id: u64 },
    Failed { operation_id: u64, message: String },
}
```

`TimelineWorkspace` owns a monotonically increasing `storyboard_copy_operation_id`. `StoryboardPreviewState` owns the current `StoryboardCopyState`.

State transitions:

```text
Idle ---------------------------> Copying(id)
Copied -------------------------> Copying(new id)
Failed -------------------------> Copying(new id)
Copying(id) -- success ---------> Copied(id)
Copying(id) -- failure ---------> Failed(id)
Copied(id) -- 2 second delay ---> Idle
Any state -- modal close -------> preview state dropped
```

Rules:

- A click is ignored while the state is `Copying`.
- Every new request receives a fresh operation ID.
- Completion applies only when the preview modal still exists and its current `Copying` ID matches.
- Delayed success clearing applies only when the current `Copied` ID matches.
- Closing the modal does not need to cancel arboard. A late result is harmless because the modal is absent or the ID is stale.
- Retry rebuilds the snapshot and starts a new operation.

## Modal UX

Footer actions:

```text
[Copy Image / Copying... / Copied / Retry] [Export PNG] [Close]
```

- `Copy Image` is enabled in `Idle`.
- `Copying...` is disabled.
- `Copied` is disabled and returns to `Copy Image` after two seconds.
- `Retry` is enabled after failure.
- Export PNG remains enabled independently of Copy state.
- Close always remains available, including while copying.
- Failure text appears immediately above the footer, not in the Timeline global banner.
- The preview image, dimensions, and step count remain unchanged.
- No nested card, second modal, or platform-specific control is introduced.

## Error Handling

- Empty Guide: do not start the task; show a recoverable preview-local error.
- Missing reviewed keyframe: do not start the task; show a recoverable preview-local error.
- Canvas pixel limit exceeded: show a recoverable copy error; do not downsample automatically.
- Clipboard unavailable or clipboard image write rejected: show `Couldn't copy Storyboard` with Retry.
- Duplicate click while copying: ignore it.
- Completion for an older operation ID: ignore it.
- Completion after modal close: ignore it.
- Delayed success clear after modal close or a newer operation: ignore it.

Tracing uses the stable `rollshot::app::storyboard_copy` target with structured fields limited to operation ID, width, height, step count, and result category. It must not include pixels, caption/title content, paths, clipboard payloads, or backend payloads.

## Performance and Resource Bounds

- Copy is an explicit one-shot operation, not a capture or per-frame hot loop.
- Snapshot creation owns one final image per reviewed step. Annotated steps require flattening; unannotated steps require one retained-frame clone.
- Snapshot clone/flatten work occurs synchronously because the background task requires owned inputs. Moving that work off-thread would require shared ownership changes to FrameStore and ImageDocument and is outside this phase.
- Storyboard layout/raster and clipboard write execute outside the synchronous iced update path; these are the dominant variable-cost operations.
- The task holds one step-image snapshot plus one final canvas at peak.
- The renderer's default `max_canvas_pixels = 24_000_000` remains the hard final-canvas limit.
- No full-resolution bitmap is retained after clipboard completion.
- No temporary PNG or filesystem I/O is introduced.

## Testing

### Shared Clipboard Helper

- A pure conversion helper produces the correct `arboard::ImageData` width, height, and RGBA bytes.
- Empty and small synthetic images preserve exact byte ordering.
- Headless CI does not instantiate or write the real system clipboard.

### Timeline Snapshot and Render

- Snapshot preserves reviewed step order and indices.
- Snapshot uses current titles and trimmed optional captions.
- Snapshot uses flattened annotated images.
- Snapshot uses retained keyframes for unannotated steps.
- Empty Guide and missing keyframe return typed/recoverable failures.
- Copy render uses `StoryboardOptions::default()` and produces export dimensions, not preview dimensions.
- Renderer failure prevents clipboard invocation.

### Modal State

- `Idle -> Copying -> Copied -> Idle` uses one matching operation ID.
- Failure becomes `Failed` and Retry allocates a newer ID.
- Duplicate Copy while `Copying` starts no second task.
- Older completion cannot replace a newer state.
- Completion after close is ignored.
- Older delayed clear cannot erase a newer result.
- Copy does not mutate Guide, presentation documents, annotations, or undo/redo history.

### Regression

- Result Workspace existing Copy state and payload tests continue to pass after the helper move.
- Storyboard preview and Export PNG tests continue to pass.
- Action Guide tests run with the `action-guide` feature.

## Platform Verification

Linux and macOS use the same `rollshot-app` helper and Timeline modal, but clipboard runtime behavior remains platform-owned.

- Linux: paste the copied Storyboard into an image-aware application under the active Wayland/KDE product environment.
- macOS: paste into Preview, Messages, or an issue editor.
- Verify alpha, dimensions, titles, captions, annotations, and visual parity with Export PNG.
- Verify the UI stays responsive during a multi-step copy.
- If only one platform is available, the final implementation report must name the untested platform and its remaining clipboard runtime risk.

## Acceptance Criteria

- The Storyboard preview modal offers `Copy Image`.
- Copy places an export-quality Storyboard on the system clipboard.
- The copied content reflects current reviewed order, titles, captions, keyframes, and committed annotations.
- Export-quality Storyboard layout/raster and clipboard writing do not block the iced update loop.
- Success shows `Copied` locally and returns to idle after two seconds.
- Failure remains in the modal and can be retried.
- Closing or retrying cannot allow a stale completion to overwrite current state.
- Copy does not mutate Guide, keyframes, annotation documents, or history.
- Export PNG and Result Workspace Copy behavior remain unchanged.
