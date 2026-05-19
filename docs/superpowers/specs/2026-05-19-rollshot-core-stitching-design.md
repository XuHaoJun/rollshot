# Rollshot Core Stitching Design

Date: 2026-05-19

## Scope

This phase builds the first real `rollshot-core` implementation and wires it to
the existing `rollshot stitch-folder` command.

It accepts already-captured image frames from disk, estimates vertical scroll
offsets between consecutive frames, appends newly visible content, and writes a
stitched PNG. It does not capture the screen.

## Goals

- Make `rollshot-core` operate on `image::RgbaImage`.
- Add a `Stitcher` API that can be reused by future capture backends.
- Implement duplicate detection so repeated frames do not append content.
- Implement template-based vertical offset matching with a content-aware region
  of interest.
- Preserve the last good anchor frame after a bad or unmatchable frame.
- Add deterministic synthetic fixture tests for normal scrolling, duplicates,
  small scrolls, sticky headers, and bad frames.
- Make `rollshot stitch-folder <frames-dir> --output <png>` produce a real PNG.

## Non-Goals

- No KDE Wayland portal or PipeWire backend.
- No macOS ScreenCaptureKit backend.
- No OBS or scap dependency in the Rust workspace.
- No OpenCV ORB, FAST, HNSW, or imageproc dependency in this phase.
- No GUI, overlay selector, clipboard output, progress UI, or preview UI.
- No golden real-screen capture fixtures.

## Reference Projects

`learn-projects/wayscrollshot` is available as a reference for algorithm shape,
session behavior, and failure handling. This phase may study these files:

- `learn-projects/wayscrollshot/src/stitch.rs`
- `learn-projects/wayscrollshot/src/session.rs`
- `learn-projects/wayscrollshot/src/types.rs`

The implementation should not copy OBS code. `learn-projects/obs-studio` and
`learn-projects/scap` remain backend references for later phases and are not
used by this core stitching phase.

The new code should be written for rollshot's API boundaries, not as a direct
module port of wayscrollshot.

## Architecture

`rollshot-core` owns all platform-independent stitching behavior. It has no
knowledge of CLI parsing, capture backends, portals, PipeWire, ScreenCaptureKit,
or desktop environments.

The crate should expose:

```rust
pub struct Stitcher;
pub struct StitchConfig;
pub struct StitchStats;
pub enum MatchAlgorithm;
pub enum StitchOutcome;
pub struct OffsetEstimate;
```

The main flow is:

```text
RgbaImage frame
→ duplicate detection
→ template/content ROI offset matching
→ append bottom slice when progress is accepted
→ updated stitched RgbaImage
```

The CLI remains a consumer of the core crate:

```text
frames directory
→ sorted input images
→ Stitcher::push_frame(frame)
→ final stitched PNG
```

## Core Modules

`crates/rollshot-core/src/lib.rs` should re-export the public API.

`crates/rollshot-core/src/types.rs` owns public value types:

- `StitchConfig`
- `MatchAlgorithm`
- `StitchOutcome`
- `StitchStats`
- `OffsetEstimate`

`crates/rollshot-core/src/stitcher.rs` owns `Stitcher` state and `push_frame`.

`crates/rollshot-core/src/duplicate.rs` owns frame signatures and duplicate
detection.

`crates/rollshot-core/src/matcher.rs` owns grayscale conversion, content ROI,
NCC scoring, second-best margin checks, overlap verification, and offset
estimation.

`crates/rollshot-core/src/image_ext.rs` owns small image operations needed by
the stitcher, such as appending a bottom slice.

Test-only fixture helpers may live inside `crates/rollshot-core/tests/fixtures`
or private test modules. They should generate images programmatically so the
first phase does not depend on binary fixture files.

## Public Types

`MatchAlgorithm` starts with one production algorithm:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAlgorithm {
    Template,
}
```

`StitchConfig` should include:

```rust
pub struct StitchConfig {
    pub algorithm: MatchAlgorithm,
    pub min_overlap: u32,
    pub min_append: u32,
    pub accept_diff: f32,
    pub match_width: u32,
    pub duplicate_threshold: f32,
}
```

`StitchOutcome` should distinguish the core decisions:

```rust
pub enum StitchOutcome {
    FirstFrame,
    Appended { added: u32 },
    NoProgress,
    NoMatch { confidence: f32 },
    Duplicate,
}
```

`OffsetEstimate` should include:

```rust
pub struct OffsetEstimate {
    pub dy: i32,
    pub confidence: f32,
    pub method: MatchAlgorithm,
}
```

Confidence follows the existing rollshot MVP convention: lower is better.
`confidence > StitchConfig::accept_diff` means the match is rejected.

## Stitcher Behavior

On the first frame, `Stitcher::push_frame` stores the frame as both the full
image and the last good frame. It returns `StitchOutcome::FirstFrame`.

For later frames:

1. Reject dimension mismatches as `NoMatch`.
2. Run duplicate detection against the last good frame.
3. Estimate the vertical offset from the last good frame to the new frame.
4. Return `NoMatch` if confidence is above `accept_diff`.
5. Return `NoProgress` if the accepted offset is below `min_append`.
6. Append the bottom `dy` pixels of the new frame to the full image.
7. Update the last good frame only after `Appended`, `NoProgress`, or
   `Duplicate` decisions that are safe to treat as part of the same stream.

Bad frames must not poison the anchor. If a frame cannot be matched, the next
frame should still be compared against the last good frame, not the bad frame.

## Duplicate Detection

Duplicate detection should be cheap and deterministic.

The detector should:

- sample or downscale each frame to a small grayscale signature
- compare signatures with mean absolute difference
- return `Duplicate` when the difference is below `duplicate_threshold`

The goal is to avoid repeatedly appending while the user has not scrolled.
Duplicate detection does not need perceptual hashing or external dependencies
in this phase.

## Template Matching

Template matching should work on grayscale pixels and assume vertical scrolling.

The matcher should:

- require equal frame dimensions
- ignore the top and bottom bands of the frame when choosing content
- ignore small side bands to avoid borders and scrollbars
- choose a template from the current frame's content ROI
- search for that template in the previous frame along the y axis
- prefer offsets near the previous accepted offset
- use normalized cross-correlation or an equivalent normalized score
- track the best and second-best scores
- reject ambiguous matches when the best score is too close to the second-best
- verify the estimated overlap with a mean absolute difference check

The matcher does not need horizontal scrolling, rotation, scaling, ORB, or GPU
processing.

## CLI Behavior

`rollshot stitch-folder` becomes a real command:

```bash
rollshot stitch-folder <frames-dir> --output <png>
```

Behavior:

- read regular files in `<frames-dir>`
- keep supported image extensions: `.png`, `.jpg`, `.jpeg`
- sort paths lexicographically
- decode each file with the `image` crate
- convert decoded images to `RgbaImage`
- push each frame into `Stitcher`
- save the final stitched image as PNG
- print a short summary including input frame count, appended frame count, and
  output path

Errors should be explicit:

- missing input directory
- no supported images found
- image decode failure with the source path
- no stitched output available
- output save failure with the destination path

The command should keep the existing bootstrap help shape, but the old
"not available in bootstrap phase" response should be removed.

## Dependencies

Add the `image` crate to the workspace dependencies if it is not already there,
using the MVP design's PNG/JPEG feature set:

```toml
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
```

`rollshot-core` depends on `image`.

`rollshot-cli` may depend on `image` directly for file decoding/saving, or call
a small core helper if that keeps the code simpler. The CLI should not own the
stitching algorithm.

Do not add OpenCV, hora, imageproc, rayon, PipeWire, DBus, scap, or OBS-related
dependencies in this phase.

## Testing Strategy

Core tests are synthetic and OS-independent.

Required `rollshot-core` tests:

- first frame initializes the stitched image
- duplicate frame returns `Duplicate` and does not increase height
- small scroll below `min_append` returns `NoProgress`
- normal scroll appends the expected number of pixels
- a bad frame returns `NoMatch`
- a good frame after a bad frame still appends from the last good anchor
- sticky-header synthetic frames still append the expected amount

Synthetic frames should be generated from deterministic long images. A helper
can build a tall image with repeated text-like bands, colored blocks, and line
patterns, then crop viewport-sized frames at known y offsets.

CLI smoke tests should create temporary input frames, run `run(["rollshot",
"stitch-folder", dir, "--output", out])` or the test binary, then verify:

- the output file exists
- the saved image dimensions match the expected stitched dimensions
- the output text includes a useful summary

Workspace verification remains:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Risks

Template matching can be ambiguous on low-feature pages. This phase mitigates
that with content ROI, second-best margin checks, and overlap verification, but
it does not try to solve every real-world page.

Sticky headers can corrupt the top portion of a template. This phase mitigates
that by ignoring top and side bands before choosing the template.

Overbuilding algorithm variants would slow the project before the end-to-end
debug loop exists. This phase keeps one algorithm and makes it well tested.

## Completion Criteria

- `rollshot-core` exposes a reusable `Stitcher` API.
- `cargo test -p rollshot-core` covers the required synthetic cases.
- `rollshot stitch-folder <frames-dir> --output <png>` writes a real PNG.
- `cargo test --workspace` passes on Linux and macOS hosted CI.
- No real capture backend or platform-specific capture dependency is added.
