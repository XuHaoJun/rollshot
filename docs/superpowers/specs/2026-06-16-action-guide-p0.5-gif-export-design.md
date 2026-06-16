# Action Guide P0.5 — Basic Summary GIF Export

Status: Approved design
Date: 2026-06-16
Primary reference: `docs/feature-discovery/2026-06-14-action-guide-capture-roadmap.md`
(§4 In scope, §Phase 4 "GIF strategy", §11 Risks)

## 1. Why this exists

P0 (the deterministic Action Guide MVP) is complete and verified on Linux: record
a workflow, detect steps, review/edit them in the Timeline Workspace, and export
`action-guide/{steps.md, session.json, keyframes/*.png}`. The user judged
detection quality good enough and chose to **skip Phase 1** (the detector
benchmark harness — pure internal measurement, no user-facing value; the
existing `detector.rs` unit tests remain the regression floor).

The next increment with direct user value is a **basic summary GIF**: a single
animated GIF assembled from the final guide's keyframes, so the guide can be
pasted somewhere that benefits from motion (issue, chat, README) as a visual
companion to the Markdown.

This is deliberately labelled **P0.5**, not the full Phase 4 GIF. It honours the
roadmap's hard constraint:

> Generate a compact summary GIF from final guide steps — NOT from the raw
> full-fps recording. This keeps file size predictable and reinforces the core
> value: workflow summary, not video recording. Markdown stays the primary
> output.

## 2. Goals / Non-goals

### Goals

- One animated GIF built from the final (possibly user-edited) guide keyframes.
- Triggered explicitly by the user; never blocks or alters Markdown export.
- Predictable file size.
- Works on both platform paths (shared Timeline Workspace module).
- Fully fixture-tested in `rollshot-action`, no new capture/storage capability.

### Non-goals (out of scope; deferred to Phase 4)

- Burned-in captions / step number / title text.
- Mouse / click / action-target markers or crops.
- Variable per-step dwell based on real elapsed time.
- Folding GIF into the main guide-export folder/flow.
- HTML / MP4 / WebM / clipboard export.
- GIF generated from the raw frame stream (explicitly rejected).

## 3. Decisions (from brainstorming)

1. **No captions** — frames are the keyframes only. The GIF is a visual
   companion to `steps.md`, not a standalone readable artifact. Text rendering
   (font, layout) is Phase 4 and would wrongly couple `rollshot-action` to text
   rasterization.
2. **Separate "Export GIF" button** — opt-in, next to "Export Guide". Markdown
   export stays fast and untouched; satisfies the roadmap criterion "GIF
   generation is optional and does not block Markdown export."
3. **Fixed dwell, infinite loop** — 1.5 s per step. Real-time-proportional dwell
   is rejected: a step where the user paused 30 s would freeze that frame for
   30 s.
4. **Downscale to a max width** — 800 px (only downscale, preserve aspect) for
   predictable file size.

## 4. Architecture

GIF generation is a pure transform over the final guide keyframes. It lives in
the framework-neutral `rollshot-action` crate (new module `src/gif.rs`), a
sibling of `export.rs`. The app layer (`rollshot-app`) owns only the UI button
and the file dialog. This is the natural extension of the existing P0 split:
`rollshot-action` owns `Guide` + `FrameStore`; the app owns windows and files.

```text
TimelineWorkspace (guide + store)
   │  user clicks "Export GIF"
   ▼
rfd save dialog  →  out_path (default name "summary.gif")
   │
   ▼
rollshot_action::export_gif(&guide, &store, GifOptions::default(), &out_path)
   │  for each step: retained keyframe -> downscale -> GIF frame (fixed delay)
   │  encode to in-memory buffer (Repeat::Infinite)
   ▼
atomic single-file write (temp sibling -> rename)
```

### 4.1 Public API (`rollshot-action`)

```rust
// src/gif.rs

/// Tunables for summary-GIF assembly. `Default` is the P0.5 "basic" profile.
pub struct GifOptions {
    /// Per-frame display time, milliseconds (`u32` to match `image`'s
    /// `Delay::from_numer_denom_ms`).
    pub frame_dwell_ms: u32, // default 1500
    /// Frames wider than this are downscaled (aspect preserved); never upscaled.
    pub max_width: u32,      // default 800
}

impl Default for GifOptions {
    fn default() -> Self { Self { frame_dwell_ms: 1500, max_width: 800 } }
}

/// Encode the guide's keyframes into an infinitely-looping GIF at `out_path`.
/// One frame per guide step, in order, using each step's *current* keyframe.
/// Writes atomically (temp sibling + rename); on any error nothing is left at
/// `out_path` and the editable guide/store are untouched.
pub fn export_gif(
    guide: &Guide,
    store: &FrameStore,
    opts: GifOptions,
    out_path: &Path,
) -> Result<(), GifError>;
```

`GifError` is added next to `ExportError` in `src/error.rs` and re-exported from
`lib.rs`:

```rust
pub enum GifError {
    Empty,                              // guide has no steps
    KeyframeMissing { index: usize },   // a step's keyframe is not retained
    Encode { source: image::ImageError },
    Io { path: String, source: std::io::Error },
}
```

### 4.2 Encoding

- Dependency: in `crates/rollshot-action/Cargo.toml`, change the image dep to
  `image = { workspace = true, features = ["gif"] }`. Cargo feature unification
  adds the `gif` codec where it's declared without editing the workspace-wide
  default features or adding a new crate.
- For each step in `guide.steps()`: look up `store.retained(step.keyframe)`
  (→ `KeyframeMissing { index }` if absent); if `image.width() > opts.max_width`,
  downscale with `image::imageops::resize` (Triangle filter) preserving aspect
  ratio; otherwise use as-is.
- Encode with `image::codecs::gif::GifEncoder` into an in-memory `Vec<u8>`,
  `set_repeat(Repeat::Infinite)`, one `Frame::from_parts(rgba, 0, 0,
  Delay::from_numer_denom_ms(frame_dwell_ms, 1))` per step.
- Frames are uniform in size by construction (all keyframes come from the same
  capture region → identical dimensions → identical downscaled dimensions).
- Atomic write: write the buffer to a temp sibling (`<out_path>` + `.tmp`
  suffix), then rename onto `out_path` (overwriting any previous file). On any
  failure remove the temp file and return the error.

## 5. UI integration (`rollshot-app/src/timeline_workspace/`)

The Timeline Workspace module is shared: the Linux `run()` entry and the macOS
`macos_product.rs` `Phase::Timeline` both drive the same `update`/`view`.
Changes to the shared module cover **both** platforms; messages route through
`timeline_workspace::update(...).map(Message::Timeline)` on macOS and directly on
Linux.

### 5.1 `view.rs`

Add an "Export GIF" button (`button::secondary`) in the header, immediately
before "Export Guide". No other layout change.

### 5.2 `update.rs`

Add messages and handlers:

```rust
ExportGifRequested,
ExportGifPathChosen(Option<PathBuf>),
```

- `ExportGifRequested` → clear `state.message`; `Task::perform(pick_gif_save_path(picker_default_dir()), Message::ExportGifPathChosen)`.
- `ExportGifPathChosen(None)` → `Task::none()` (cancelled picker).
- `ExportGifPathChosen(Some(path))` → call `export_gif(&guide, &store,
  GifOptions::default(), &path)`:
  - `Ok` → `state.message = Some("GIF saved to <path>")`; `Task::none()` —
    **the window stays open** (so the user can still Export Guide).
  - `Err` → `state.message = Some("GIF export failed: …")`; `Task::none()`.

`pick_gif_save_path` mirrors the existing `pick_export_dir`:
`rfd::AsyncFileDialog::new().set_directory(default_dir).set_file_name("summary.gif").add_filter("GIF image", &["gif"]).save_file()`.

The success/error text reuses the existing `state.message` banner (with its
Dismiss button). Guide export keeps its current behaviour unchanged (exits on
success); GIF export intentionally does not exit.

## 6. Error handling

- Empty guide → `GifError::Empty`, surfaced as an inline banner; nothing written.
- A missing retained keyframe → `GifError::KeyframeMissing`; nothing written.
- GIF export is fully independent of `export_guide` — a GIF failure never
  affects Markdown/keyframe export and vice versa (separate button, separate
  code path, separate output file).
- Atomic single-file write: a partial/failed encode never leaves a half-written
  `summary.gif`.

## 7. Testing

### `rollshot-action` (`gif.rs`)

- Reuse the export-test recording fixture (real detector-produced candidates
  with retained frames). `export_gif` to a temp path:
  - file exists and is non-empty;
  - decode it back with `image::codecs::gif::GifDecoder` and assert frame
    count == `guide.steps().len()`;
  - assert each frame width ≤ `max_width`.
- A guide wider than `max_width` is downscaled; a guide already narrower is left
  at native width.
- Empty guide → `Err(GifError::Empty)` and no file written.

### `rollshot-app` (`timeline_workspace/update.rs`)

- `ExportGifPathChosen(Some(tmp))` writes `summary.gif`, sets a success banner,
  and returns without exiting (window stays open).
- `ExportGifPathChosen(None)` is a no-op.
- Empty guide path surfaces an inline error and writes nothing.

### Verification commands

- `rtk cargo test -p rollshot-action`
- `rtk cargo test -p rollshot-app --features action-guide`
- `rtk cargo fmt --check`
- `rtk cargo clippy --workspace --all-targets -- -D warnings`

## 8. Platform note (AGENTS.md §8)

All changes are in shared code: `rollshot-action/src/gif.rs` (platform-neutral)
and the shared `timeline_workspace/` module used by both the Linux `run()` path
and the macOS `Phase::Timeline` path. No platform-specific branch is touched.
The macOS path is therefore covered by construction (shared `update`/`view` +
the existing `rfd` save dialog), but it is **not runtime-verified** in this work
(development host is Linux). Remaining macOS risk: the `rfd` save dialog and the
async `Task` round-trip behave as they already do for the existing Linux export
path, but the macOS daemon's Timeline phase has not been exercised with the new
message at runtime.

## 9. Suggested PR sequence

1. `rollshot-action`: add `image` `gif` feature, `GifError`, `src/gif.rs`
   (`GifOptions` + `export_gif`) with unit tests; export from `lib.rs`.
2. `rollshot-app`: add the `ExportGifRequested` / `ExportGifPathChosen` messages,
   `pick_gif_save_path`, the success-banner handling, and the `view.rs` button,
   with update tests.

Both increments are independently testable; (2) depends on (1).
