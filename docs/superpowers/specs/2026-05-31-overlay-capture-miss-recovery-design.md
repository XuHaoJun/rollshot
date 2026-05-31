# Overlay Capture-Miss Recovery UX (Design Spec)

Status: approved design spec.
Issue: `docs/issues/2026-05-30-overlay-capture-miss-recovery-ux.md`.
Reference: `learn-projects/snow-shot`.

All file:line references are evidence captured during design and may drift;
verify against code before relying on them.

## Goal

When live scrolling outruns stitching, Rollshot must behave like snow-shot:
surface a throttled "scrolling too fast" warning, make the live preview show
where the captured edge is, and allow the user to recover by scrolling back to
the reconnectable edge. This applies to both capture UI paths:

- Linux native layer-shell overlay: `crates/rollshot-overlay`.
- macOS/webview overlay path: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
  backed by `crates/rollshot-app/src-tauri/src/session.rs`.

The fix is not Linux-only. Linux native overlay was introduced after the issue
was filed, so the shared webview path must receive the same behavior.

## Reference Behavior

snow-shot uses three relevant patterns:

1. Match failure is reported to the user as a throttled warning. In
   `learn-projects/snow-shot/src/pages/draw/components/drawToolbar/components/tools/scrollScreenshotTool/index.tsx`,
   `edge_position === undefined` calls `showCaptureMissMessage()`. The warning
   is throttled to the leading edge every 3000 ms and uses
   `draw.scrollScreenshot.captureMiss`: "滚动过快，请滚动到已截取的边缘再次尝试".
2. The live preview is not just the final stitched image. It is a thumbnail
   strip with a `captuer-edge-mask` overlay that visually marks the currently
   reconnectable captured edge, plus a loading spinner while processing.
3. The Rust service has a rollback-style recovery attempt. If matching fails
   against the first direction and `try_rollback` is enabled, it tries the other
   image index before returning a miss.

Rollshot should copy the UX intent, not snow-shot's full thumbnail-list data
structure. Rollshot's existing live preview remains a single stitched preview,
but it gains captured-edge affordance and processing/miss state.

## Non-Goals

- No automatic re-anchor that silently skips missing content unless a core
  recovery test proves it is the minimal correct fix. A blind re-anchor could
  hide gaps in the output.
- No new settings UI for thresholds, throttling, or recovery behavior.
- No replacement of Rollshot's current preview pipeline with snow-shot's
  thumbnail-list architecture.
- No retroactive edits to historical specs/plans in `docs/superpowers/`.

## Core Decisions

### D1. Shared state lives in `rollshot-overlay-core`

Add a platform-independent capture-miss tracker under
`crates/rollshot-overlay-core`. This crate already holds shared overlay preview
and crop visual logic and has no iced/Tauri/webview dependencies.

The shared unit exposes a small signal API rather than forcing UI code to
understand `rollshot_core::StitchOutcome` directly:

```rust
pub enum StitchProgressSignal {
    Accepted,
    Missed,
    Idle,
}

pub struct CaptureMissTracker;

pub enum CaptureMissEvent {
    None,
    ShowWarning,
    ClearWarning,
}
```

Expected semantics:

- `Accepted` covers `StitchOutcome::FirstFrame` and `StitchOutcome::Appended`.
  It clears the miss state.
- `Missed` covers `StitchOutcome::NoMatch`. It enters miss state and emits a
  warning only when the throttle allows it.
- `Idle` covers `Duplicate`, `NoProgress`, and non-fatal pauses. It does not
  enter miss state and does not clear an active miss.
- `AxisChanged` is treated as `Missed` because it is not an accepted append.

Both Rust capture paths convert `StitchOutcome` to `StitchProgressSignal` at
the boundary where they already call `Stitcher::push_frame`.

### D2. Webview status carries structured miss state

The webview/macOS path already stores `last_stitch_outcome` as a display string
in `AppSession` and exposes it through `SessionStatus::Stitching`
(`crates/rollshot-app/src-tauri/src/session.rs`, `crates/rollshot-app/src/api/capture.ts`).
Do not make TypeScript parse this string.

Extend `SessionStatus::Stitching` with these structured fields from the shared
tracker:

```rust
capture_miss: bool,
capture_miss_warning: bool,
```

`capture_miss` means the latest stitch state is waiting for overlap to be
reconnected. `capture_miss_warning` is a leading-edge pulse that the frontend
uses to show a snow-shot-style warning toast. The frontend may also keep a
local timestamp to avoid duplicate rendering, but Rust owns the canonical state.

### D3. Native overlay preview channel becomes event-based

The Linux native driver currently sends only `iced::widget::image::Handle`
through `preview_tx`. Replace this with an overlay event enum with these
variants:

```rust
pub enum LiveOverlayEvent {
    Preview(ImageHandle),
    CaptureMiss {
        active: bool,
        warn: bool,
    },
}
```

`Driver::begin_stitch` updates the shared tracker after each
`Stitcher::push_frame`, emits miss events when state changes or when the
warning pulse fires, and continues emitting preview frames when a full image is
available. `overlay.rs` stores the current miss state in `Overlay`.

### D4. Warning is transient and throttled, snow-shot style

Warning behavior:

- Text: "Scrolling too fast. Scroll back to the captured edge and try again."
- Trigger: first `NoMatch` when not throttled.
- Throttle: leading edge, approximately 3000 ms, matching snow-shot.
- Clear: next accepted stitch (`FirstFrame` or `Appended`) clears active miss.
- `Duplicate` and `NoProgress` do not warn.

The exact UI primitive differs by platform:

- Webview path can use an existing lightweight toast/dialog dependency if one
  already exists; otherwise render a transient overlay warning in the capture UI
  without adding a new global UI library.
- Native iced overlay renders an equivalent transient warning in the existing
  outside-crop chrome. It must never draw inside the crop region because the
  overlay can be captured by PipeWire.

### D5. Live preview gains a captured-edge affordance

Both platform paths should add a snow-shot-inspired preview affordance:

- Show an active processing indicator while the stitch loop is handling frames.
- When `capture_miss` is active, overlay a mask/marker on the live preview to
  indicate the captured edge the user should scroll back toward.
- Keep the affordance outside the captured crop for Linux native overlay.
- Do not replace Rollshot's stitched preview with snow-shot's thumbnail strip.

The shared part should be the state/geometry model, not framework rendering.
`rollshot-overlay-core` exposes this platform-independent model:

```rust
pub enum CapturedEdge {
    Top,
    Bottom,
    Left,
    Right,
    Unknown,
}

pub struct PreviewRecoveryAffordance {
    pub active: bool,
    pub edge: CapturedEdge,
    pub processing: bool,
}
```

If the stitch estimate provides a direction on miss, use it to infer the edge.
If not, render a generic edge/processing affordance rather than guessing.

### D6. Recovery correctness is gated by a core test

The issue report originally assumed that scrolling back to the stranded anchor
should recover because `last_good` is not updated on `NoMatch`. The user later
reported a stronger symptom: even after scrolling back to the broken point, the
live preview remains completely frozen.

Implementation must first add a focused test or diagnostic that captures this:

```text
accepted frames -> gap / NoMatch -> frame overlapping the stranded anchor again
```

Expected result:

- If Rollshot appends after the reconnecting frame, recovery is already correct
  in `rollshot-core`; the implementation stays in shared state + UI.
- If Rollshot still returns `NoMatch` because direction locking or rollback
  behavior rejects the reconnecting frame, add the smallest core recovery change
  required. The design model is snow-shot's `try_rollback`: attempt a recovery
  match against a valid reconnectable edge before declaring unrecoverable miss.

Any core behavior change must preserve output integrity. It must not silently
drop skipped content and continue as if nothing happened.

## Data Flow

```text
Stitcher::push_frame
  -> StitchOutcome
  -> path-local conversion to StitchProgressSignal
  -> rollshot-overlay-core::CaptureMissTracker
  -> structured state

Webview/macOS:
  AppSession::push_stitch_frame
    -> SessionStatus::Stitching with capture_miss and capture_miss_warning
    -> CaptureOverlay.tsx
    -> toast + live-preview captured-edge affordance

Linux native:
  Driver::begin_stitch thread
    -> LiveOverlayEvent::CaptureMiss { active, warn }
    -> overlay.rs state
    -> transient warning + live-preview captured-edge affordance
```

## Testing Strategy

Required tests:

- `rollshot-overlay-core`: unit tests for `CaptureMissTracker`.
  - `NoMatch` emits a warning and activates miss state.
  - Repeated `NoMatch` within throttle stays active but does not warn again.
  - `Duplicate` and `NoProgress` do not warn.
  - `Appended` clears active miss.
- `rollshot-app/src-tauri/src/session.rs`: tests that webview
  `SessionStatus::Stitching` exposes `capture_miss` and clears it after
  accepted frames.
- `crates/rollshot-overlay/src/driver.rs`: tests for event emission where
  practical, using a testable extraction if the live threaded driver is too
  coupled to real capture.
- `CaptureOverlay.test.tsx`: renders the warning/affordance when
  `capture_miss` is active and does not rely on parsing `last_outcome`.
- `rollshot-core`: recovery test for miss then scroll-back reconnect before any
  core change.

Verification commands:

```bash
rtk cargo test
rtk cargo fmt --check
rtk pnpm --dir crates/rollshot-app test
rtk pnpm --dir crates/rollshot-app run typecheck
```

If `rollshot-core` stitching behavior changes, also run the benchmark workflow
from `docs/bench.md` for the affected stitching path.

## Runtime Acceptance

Manual acceptance on both platform paths:

- Scroll slowly: preview updates normally, no warning.
- Scroll too fast: warning appears once, then is throttled.
- Continue scrolling away: preview clearly shows recovery/miss state instead of
  silently freezing.
- Scroll back to the captured edge: preview resumes updating.
- Press Esc/finish while miss state is active: final image/save handoff still
  works as before.

Linux-specific:

- Warning and preview affordance remain outside the crop and do not become part
  of the stitched image.

macOS/webview-specific:

- Existing overlay exclusion behavior remains intact.

## Implementation Constraints

- The implementation plan may adjust exact struct and method names to match
  current module boundaries, but it must preserve the fields and semantics in
  this spec.
- If a test shows core recovery already works, do not add a core behavior
  change.
- If a test shows direction-lock recovery is the blocker, keep the core patch
  narrow and document why it does not hide skipped content.
