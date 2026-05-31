# Overlay Capture-Miss Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add snow-shot-style capture-miss recovery feedback to both Rollshot capture overlays and verify that scrolling back to the captured edge can resume stitching.

**Architecture:** Shared capture-miss state lives in `rollshot-overlay-core`; both Rust capture paths convert `StitchOutcome` into the same state model. The webview path exposes structured status to React, while the native Linux path emits event messages to iced. Core stitching changes are gated by a reproduction test.

**Tech Stack:** Rust workspace crates (`rollshot-core`, `rollshot-overlay-core`, `rollshot-overlay`, `rollshot-app/src-tauri`), React 19 + Vitest in `crates/rollshot-app`, iced layer-shell overlay on Linux.

---

## Engineering Review (applied 2026-05-31)

This plan was reviewed and edited in place. Changes are marked inline with
`> [eng-review]` callouts. Summary of what changed and why:

- **R1 (DRY):** Shared throttle constant `CAPTURE_MISS_THROTTLE` + `Default`
  impls for `CaptureMissTracker`/`CaptureMissState`. Removes the `3000` magic
  number repeated across 4 call sites and the awkward
  `CaptureMissTracker::new(..).state()` double-construct.
- **R2 (DRY / architecture):** `captured_edge_from_direction` and
  `progress_signal_from_outcome` were duplicated byte-for-byte in `session.rs`
  and `driver.rs`. Moved into `rollshot-overlay-core::capture_miss` (which gains
  a `rollshot-core` dep — verified acyclic). This is what the Goal's "both
  convert into the same state model" actually requires.
- **R3 (correctness, webview):** The single `[status]`-keyed toast effect would
  clear its own dismiss timer on the next poll (read-clear flips
  `capture_miss_warning` to false 160 ms later), so the toast would **never
  auto-dismiss**. Split into a set-effect + an expire-effect keyed on the toast
  value. Added a "toast disappears after 3 s" assertion that catches it.
- **R4 (correctness, native — silent failure):** The driver only emitted
  `CaptureMiss` when `active || warn`, so the **clearing/recovery edge was never
  sent** and the native "Scroll back…" marker would stay forever after a miss.
  Now emits on any active-flag change (rising AND falling) plus warn pulses.
- **R5 (correctness, native):** `toolbar_input_rect` assumes the toolbar sits at
  the band origin; prepending the warning above it would shift the Stop/Save
  buttons out of the interactive input region (clicks pass through). Chrome order
  changed to toolbar-first.
- **R6 (dependency footgun):** `iced::time::every` needs an async-runtime feature
  that `rollshot-overlay`'s iced (`["canvas","image"]`) does not enable. Added a
  Cargo step + a no-new-dependency fallback.
- **R7 (UX edge):** `last_warning_at` was not reset on `Accepted`, so a fresh
  miss within 3 s of a successful reconnect was silently throttled. Reset it; new
  test added.

---

## File Structure

- Create: `crates/rollshot-overlay-core/src/capture_miss.rs`
  - Shared `StitchProgressSignal`, `CapturedEdge`, `PreviewRecoveryAffordance`, `CaptureMissState`, `CaptureMissTracker`, the `CAPTURE_MISS_THROTTLE` constant, and the shared `captured_edge_from_direction` / `progress_signal_from_outcome` converters (R1, R2).
- Modify: `crates/rollshot-overlay-core/Cargo.toml`
  - Add `serde` (`CapturedEdge` is serialized through Tauri status) and `rollshot-core` (the shared `StitchOutcome` → signal converter lives here; R2). Acyclic: `rollshot-core` has no overlay dep.
- Modify: `crates/rollshot-overlay/Cargo.toml`
  - Enable the iced async-runtime feature needed by `iced::time::every`, or take the fallback in Task 5 (R6).
- Modify: `crates/rollshot-overlay-core/src/lib.rs`
  - Export the new shared module.
- Modify: `crates/rollshot-core/tests/stitcher.rs`
  - Add a diagnostic test for miss then scroll-back reconnect.
- Modify if the diagnostic fails: `crates/rollshot-core/src/stitcher.rs`
  - Add the smallest recovery behavior needed to make the diagnostic pass without re-anchoring past missing content.
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`
  - Track capture-miss state in `AppSession`, expose it through `SessionStatus::Stitching`, and clear warning pulses after `SharedSession::status()`.
- Modify: `crates/rollshot-app/src/api/capture.ts`
  - Add typed fields for the structured capture-miss status.
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
  - Render a transient snow-shot-style warning and pass recovery affordance state to the preview.
- Modify: `crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx`
  - Render captured-edge mask/processing affordance over the existing stitched preview.
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`
  - Test warning and preview affordance behavior.
- Modify: `crates/rollshot-app/src/App.css`
  - Add capture-miss warning and preview-affordance styles.
- Modify: `crates/rollshot-overlay/src/driver.rs`
  - Emit preview/status events from the live stitch thread using the shared tracker.
- Modify: `crates/rollshot-overlay/src/overlay.rs`
  - Store native capture-miss state and render warning/preview affordance outside the crop.

---

### Task 1: Shared Capture-Miss State

**Files:**
- Create: `crates/rollshot-overlay-core/src/capture_miss.rs`
- Modify: `crates/rollshot-overlay-core/src/lib.rs`
- Modify: `crates/rollshot-overlay-core/Cargo.toml`

- [ ] **Step 1: Write the shared-state tests**

Create `crates/rollshot-overlay-core/src/capture_miss.rs` with the API skeleton and these tests:

> **[eng-review R1]** Added `CAPTURE_MISS_THROTTLE` and `Default` impls so the
> `3000 ms` value lives in exactly one place and call sites use `::default()`.
> `CaptureMissState::default()` IS the inactive state (all-false + `Unknown`
> edge), replacing the throwaway-tracker `.state()` construction.
>
> **[eng-review R2]** Added the shared `rollshot-core` import + the
> `captured_edge_from_direction` / `progress_signal_from_outcome` converters here
> (previously duplicated in `session.rs` and `driver.rs`).

```rust
use std::time::{Duration, Instant};

use rollshot_core::{AppendDirection, StitchOutcome};

pub const CAPTURE_MISS_WARNING: &str =
    "Scrolling too fast. Scroll back to the captured edge and try again.";

/// One warning toast at most per this window (R1: single source of truth).
pub const CAPTURE_MISS_THROTTLE: Duration = Duration::from_millis(3000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchProgressSignal {
    Accepted { edge: CapturedEdge },
    Missed { edge: CapturedEdge },
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturedEdge {
    Top,
    Bottom,
    Left,
    Right,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreviewRecoveryAffordance {
    pub active: bool,
    pub edge: CapturedEdge,
    pub processing: bool,
}

/// `Default` is the inactive state: not active, not warning, `Unknown` edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureMissState {
    pub active: bool,
    pub warn: bool,
    pub edge: CapturedEdge,
    pub affordance: PreviewRecoveryAffordance,
}

/// Convert a stitch append direction into the captured edge the user must
/// scroll back toward. Shared by both capture paths (R2).
pub fn captured_edge_from_direction(direction: AppendDirection) -> CapturedEdge {
    match direction {
        AppendDirection::Top => CapturedEdge::Top,
        AppendDirection::Bottom => CapturedEdge::Bottom,
        AppendDirection::Left => CapturedEdge::Left,
        AppendDirection::Right => CapturedEdge::Right,
    }
}

/// Map a `StitchOutcome` to the progress signal that drives the tracker.
/// Shared by `session.rs` (webview) and `driver.rs` (native) (R2).
pub fn progress_signal_from_outcome(outcome: &StitchOutcome) -> StitchProgressSignal {
    match outcome {
        StitchOutcome::FirstFrame => StitchProgressSignal::Accepted {
            edge: CapturedEdge::Unknown,
        },
        StitchOutcome::Appended { direction, .. } => StitchProgressSignal::Accepted {
            edge: captured_edge_from_direction(*direction),
        },
        StitchOutcome::NoMatch { best_estimate, .. } => StitchProgressSignal::Missed {
            edge: best_estimate
                .map(|estimate| captured_edge_from_direction(estimate.direction))
                .unwrap_or(CapturedEdge::Unknown),
        },
        StitchOutcome::AxisChanged { estimate, .. } => StitchProgressSignal::Missed {
            edge: captured_edge_from_direction(estimate.direction),
        },
        StitchOutcome::Duplicate | StitchOutcome::NoProgress { .. } => StitchProgressSignal::Idle,
    }
}

#[derive(Debug)]
pub struct CaptureMissTracker {
    active: bool,
    edge: CapturedEdge,
    last_warning_at: Option<Instant>,
    throttle: Duration,
}

impl Default for CaptureMissTracker {
    fn default() -> Self {
        Self::new(CAPTURE_MISS_THROTTLE)
    }
}

impl CaptureMissTracker {
    pub fn new(throttle: Duration) -> Self {
        Self {
            active: false,
            edge: CapturedEdge::Unknown,
            last_warning_at: None,
            throttle,
        }
    }

    pub fn update(&mut self, _signal: StitchProgressSignal, _now: Instant) -> CaptureMissState {
        self.state()
    }

    pub fn state(&self) -> CaptureMissState {
        CaptureMissState {
            active: self.active,
            warn: false,
            edge: self.edge,
            affordance: PreviewRecoveryAffordance {
                active: self.active,
                edge: self.edge,
                processing: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    #[test]
    fn missed_enters_active_state_and_warns() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        assert!(state.active);
        assert!(state.warn);
        assert_eq!(state.edge, CapturedEdge::Bottom);
        assert!(state.affordance.active);
        assert_eq!(state.affordance.edge, CapturedEdge::Bottom);
    }

    #[test]
    fn repeated_misses_are_throttled_but_stay_active() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(1000),
        );

        assert!(state.active);
        assert!(!state.warn);
    }

    #[test]
    fn missed_warns_again_after_throttle_window() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(3001),
        );

        assert!(state.active);
        assert!(state.warn);
    }

    #[test]
    fn idle_does_not_create_or_clear_miss_state() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let idle = tracker.update(StitchProgressSignal::Idle, t(0));
        assert!(!idle.active);
        assert!(!idle.warn);

        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Top,
            },
            t(10),
        );
        let idle_after_miss = tracker.update(StitchProgressSignal::Idle, t(20));
        assert!(idle_after_miss.active);
        assert!(!idle_after_miss.warn);
        assert_eq!(idle_after_miss.edge, CapturedEdge::Top);
    }

    #[test]
    fn accepted_clears_active_miss_state() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        let state = tracker.update(
            StitchProgressSignal::Accepted {
                edge: CapturedEdge::Bottom,
            },
            t(10),
        );

        assert!(!state.active);
        assert!(!state.warn);
        assert_eq!(state.edge, CapturedEdge::Unknown);
        assert!(!state.affordance.active);
    }

    // R7: a fresh miss right after a successful reconnect must warn again, not
    // be silently throttled by the pre-recovery `last_warning_at`.
    #[test]
    fn miss_after_recovery_warns_immediately() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );
        let _ = tracker.update(
            StitchProgressSignal::Accepted {
                edge: CapturedEdge::Bottom,
            },
            t(100),
        );

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(200),
        );

        assert!(state.active);
        assert!(state.warn, "miss after recovery must warn within throttle window");
    }

    // R2: the shared outcome→signal converter, exercised here so both capture
    // paths inherit the coverage instead of each re-testing the mapping.
    #[test]
    fn no_match_outcome_maps_to_missed_signal() {
        let outcome = StitchOutcome::NoMatch {
            reason: rollshot_core::NoMatchReason::ReverseDirection,
            best_estimate: None,
        };
        assert_eq!(
            progress_signal_from_outcome(&outcome),
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown
            }
        );
    }

    #[test]
    fn duplicate_outcome_maps_to_idle_signal() {
        assert_eq!(
            progress_signal_from_outcome(&StitchOutcome::Duplicate),
            StitchProgressSignal::Idle
        );
    }

    #[test]
    fn appended_outcome_maps_accepted_edge_from_direction() {
        assert_eq!(
            captured_edge_from_direction(AppendDirection::Bottom),
            CapturedEdge::Bottom
        );
    }
}
```

- [ ] **Step 2: Run the targeted test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-overlay-core capture_miss -- --nocapture
```

Expected: FAIL because `CaptureMissTracker::update` currently returns the unchanged inactive state.

- [ ] **Step 3: Implement the tracker**

Replace the `update` body with:

```rust
pub fn update(&mut self, signal: StitchProgressSignal, now: Instant) -> CaptureMissState {
    match signal {
        StitchProgressSignal::Accepted { .. } => {
            self.active = false;
            self.edge = CapturedEdge::Unknown;
            // R7: forget the last warning time so a miss right after recovery
            // warns immediately rather than being throttled by the old pulse.
            self.last_warning_at = None;
            CaptureMissState::default()
        }
        StitchProgressSignal::Missed { edge } => {
            self.active = true;
            self.edge = edge;
            let warn = match self.last_warning_at {
                Some(last) => now.duration_since(last) >= self.throttle,
                None => true,
            };
            if warn {
                self.last_warning_at = Some(now);
            }
            CaptureMissState {
                active: true,
                warn,
                edge,
                affordance: PreviewRecoveryAffordance {
                    active: true,
                    edge,
                    processing: false,
                },
            }
        }
        StitchProgressSignal::Idle => self.state(),
    }
}
```

- [ ] **Step 4: Export the module**

Modify `crates/rollshot-overlay-core/src/lib.rs`:

```rust
pub mod capture_miss;
pub mod preview;
pub mod tokens;
```

- [ ] **Step 5: Add serde + rollshot-core dependencies**

Modify `crates/rollshot-overlay-core/Cargo.toml` (R2). `serde` carries the
`derive` feature from the workspace (verified in root `Cargo.toml`), and
`rollshot-core` hosts `StitchOutcome`/`AppendDirection` for the shared
converter. This edge is acyclic — `rollshot-core` has no overlay dependency.

```toml
[dependencies]
image = { workspace = true }
serde = { workspace = true }
rollshot-core = { path = "../rollshot-core" }
```

- [ ] **Step 6: Run the shared-state tests**

Run:

```bash
rtk cargo test -p rollshot-overlay-core capture_miss -- --nocapture
```

Expected: PASS.

---

### Task 2: Core Recovery Diagnostic Gate

**Files:**
- Modify: `crates/rollshot-core/tests/stitcher.rs`
- Modify only if this task fails: `crates/rollshot-core/src/stitcher.rs`

- [ ] **Step 1: Add the diagnostic test**

Append this test near `bad_frame_returns_no_match_and_preserves_anchor` in `crates/rollshot-core/tests/stitcher.rs`:

```rust
#[test]
fn scroll_back_after_reverse_direction_miss_can_reconnect_to_last_good_anchor() {
    let canvas = make_scroll_canvas(320, 1800);
    let first = crop_frame(&canvas, 0, 320);
    let appended = crop_frame(&canvas, 96, 320);
    let reverse = crop_frame(&canvas, 32, 320);
    let reconnected = crop_frame(&canvas, 192, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(appended) {
        StitchOutcome::Appended { direction, .. } => {
            assert_eq!(direction, AppendDirection::Bottom);
        }
        other => panic!("expected initial bottom append, got {other:?}"),
    }

    match stitcher.push_frame(reverse) {
        StitchOutcome::NoMatch { reason, .. } => {
            assert_eq!(reason, NoMatchReason::ReverseDirection);
        }
        other => panic!("expected reverse-direction miss, got {other:?}"),
    }

    let stats_after_miss = stitcher.stats();
    assert_eq!(stats_after_miss.frame_count, 2);

    match stitcher.push_frame(reconnected) {
        StitchOutcome::Appended {
            direction, added, ..
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert!((92..=100).contains(&added), "added = {added}");
        }
        other => panic!("expected append after reconnecting to anchor, got {other:?}"),
    }

    assert_eq!(stitcher.stats().frame_count, 3);
}
```

- [ ] **Step 2: Run the diagnostic**

Run:

```bash
rtk cargo test -p rollshot-core scroll_back_after_reverse_direction_miss_can_reconnect_to_last_good_anchor -- --nocapture
```

Expected: PASS. If it passes, do not modify `rollshot-core/src/stitcher.rs` for this issue. Continue to Task 3.

> **[eng-review]** This is a characterization/regression test and is expected to
> pass with no core change: `push_frame` only advances `last_good` on `Appended`,
> so a `ReverseDirection` `NoMatch` leaves the anchor at y=96, and the y=192
> reconnect is a normal +96 bottom append — the same anchor-preservation
> mechanism already proven by `bad_frame_returns_no_match_and_preserves_anchor`.
> The Step 4 core patch branch is therefore almost certainly dead; keep it gated.
> Caveat: the `assert_eq!(reason, NoMatchReason::ReverseDirection)` is
> matcher-behavior-dependent — if the matcher returns a different `NoMatchReason`
> for the y=32 reverse frame, the test fails on *characterization* (not on
> recovery), and Step 4's guidance correctly says to leave core untouched.

- [ ] **Step 3: If the diagnostic fails, stop and inspect the actual failure**

Run this extra focused command before editing core:

```bash
rtk cargo test -p rollshot-core scroll_back_after_reverse_direction_miss_can_reconnect_to_last_good_anchor -- --nocapture
```

Expected evidence to capture in the task notes:

```text
the failing outcome variant
the NoMatchReason, if any
whether stats.frame_count stayed at 2
```

- [ ] **Step 4: If and only if Step 2 fails because reconnect still returns `ReverseDirection`, add a guarded recovery path**

Modify `crates/rollshot-core/src/stitcher.rs` at the `locked_direction` rejection:

```rust
if let Some(locked_dir) = self.locked_direction {
    if direction != locked_dir {
        self.last_metrics
            .set_no_match(NoMatchReason::ReverseDirection);
        return StitchOutcome::NoMatch {
            reason: NoMatchReason::ReverseDirection,
            best_estimate: build_estimate(
                anchor.rgba(),
                curr.rgba(),
                &candidate,
                self.config.axis_ratio_threshold,
            ),
        };
    }
}
```

Do not add an auto re-anchor here. The permitted implementation is a narrow rollback-style retry only if investigation shows the current candidate is selected from the wrong edge while a same-direction reconnect candidate exists. Reuse existing matcher/candidate code instead of inventing a second matcher. Keep the output append path unchanged.

- [ ] **Step 5: Re-run the diagnostic and nearby stitcher tests**

Run:

```bash
rtk cargo test -p rollshot-core scroll_back_after_reverse_direction_miss_can_reconnect_to_last_good_anchor bad_frame_returns_no_match_and_preserves_anchor normal_scroll_appends_bottom_and_locks_vertical_axis -- --nocapture
```

Expected: PASS.

---

### Task 3: Webview Rust Session Status

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`

- [ ] **Step 1: Add failing session tests**

In the `#[cfg(test)] mod tests` section of `session.rs`, add imports:

```rust
use rollshot_overlay_core::capture_miss::{CapturedEdge, CAPTURE_MISS_WARNING};
```

Add this helper next to `scrolling_frame`:

```rust
fn blank_frame(width: u32, height: u32) -> CapturedFrame {
    CapturedFrame {
        image: RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255])),
        timestamp: SystemTime::UNIX_EPOCH,
        metadata: FrameMetadata::fake(),
    }
}
```

Add these tests:

```rust
#[test]
fn status_reports_capture_miss_after_no_match() {
    let mut session = AppSession::new();
    session.store_frame_for_test(scrolling_frame(0));
    session
        .confirm_region(RegionDto {
            x: 0,
            y: 0,
            width: 80,
            height: 80,
        })
        .expect("confirm region");
    session.start_stitching_for_test().expect("start stitching");

    session
        .push_stitch_frame_for_test(scrolling_frame(0))
        .expect("first frame");
    session
        .push_stitch_frame_for_test(blank_frame(80, 80))
        .expect("miss frame");

    match session.status() {
        SessionStatus::Stitching {
            capture_miss,
            capture_miss_warning,
            capture_miss_edge,
            capture_miss_message,
            ..
        } => {
            assert!(capture_miss);
            assert!(capture_miss_warning);
            assert_eq!(capture_miss_edge, CapturedEdge::Unknown);
            assert_eq!(capture_miss_message, CAPTURE_MISS_WARNING);
        }
        other => panic!("expected stitching status, got {other:?}"),
    }
}

#[test]
fn accepted_frame_clears_capture_miss_status() {
    let mut session = AppSession::new();
    session.store_frame_for_test(scrolling_frame(0));
    session
        .confirm_region(RegionDto {
            x: 0,
            y: 0,
            width: 80,
            height: 80,
        })
        .expect("confirm region");
    session.start_stitching_for_test().expect("start stitching");

    session
        .push_stitch_frame_for_test(scrolling_frame(0))
        .expect("first frame");
    session
        .push_stitch_frame_for_test(blank_frame(80, 80))
        .expect("miss frame");
    session
        .push_stitch_frame_for_test(scrolling_frame(8))
        .expect("recovered frame");

    match session.status() {
        SessionStatus::Stitching { capture_miss, .. } => {
            assert!(!capture_miss);
        }
        other => panic!("expected stitching status, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run the session tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app status_reports_capture_miss_after_no_match accepted_frame_clears_capture_miss_status -- --nocapture
```

Expected: FAIL because `SessionStatus::Stitching` has no capture-miss fields yet.

- [ ] **Step 3: Add capture-miss fields to `SessionStatus::Stitching`**

Modify the enum variant in `session.rs`:

```rust
Stitching {
    frame_width: u32,
    frame_height: u32,
    region: RegionDto,
    stats: StitchStatsDto,
    last_outcome: Option<String>,
    capture_miss: bool,
    capture_miss_warning: bool,
    capture_miss_edge: rollshot_overlay_core::capture_miss::CapturedEdge,
    capture_miss_message: &'static str,
},
```

`CapturedEdge` already derives `serde::Serialize` in Task 1, and `rollshot-overlay-core`
already has the required `serde` dependency from Task 1.

- [ ] **Step 4: Store tracker state in `AppSession`**

Add imports (R2: the converters now come from the shared crate, so no local
`progress_signal_from_outcome` is defined in `session.rs`):

```rust
use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker, CAPTURE_MISS_WARNING,
};
```

Add fields to `AppSession`:

```rust
capture_miss_tracker: CaptureMissTracker,
capture_miss_state: CaptureMissState,
```

Because `AppSession` currently derives `Default`, implement `Default` manually.
Remove `#[derive(Default)]` from `AppSession`, then add (R1: use the shared
`::default()` helpers — no `3000` literal, no throwaway tracker):

```rust
impl Default for AppSession {
    fn default() -> Self {
        Self {
            latest_frame: None,
            latest_frame_seq: 0,
            selected_region: None,
            stitcher: None,
            stitch_stats: StitchStatsDto::from(StitchStats::default()),
            last_stitch_outcome: None,
            capture_miss_tracker: CaptureMissTracker::default(),
            capture_miss_state: CaptureMissState::default(),
            final_image: None,
            output_path: None,
            error: None,
        }
    }
}
```

- [ ] **Step 5: Convert `StitchOutcome` to shared signals**

> **[eng-review R2]** The previously-duplicated `captured_edge_from_direction`
> and `progress_signal_from_outcome` now live in
> `rollshot_overlay_core::capture_miss` and are imported (Step 4). Nothing to
> define locally here — go straight to wiring the tracker update.

In `push_stitch_frame`, after `let outcome = stitcher.push_frame(cropped.image);`, update the tracker:

```rust
self.capture_miss_state = self
    .capture_miss_tracker
    .update(progress_signal_from_outcome(&outcome), std::time::Instant::now());
```

- [ ] **Step 6: Include the new fields in `status()` and reset them**

In `AppSession::status()`, populate:

```rust
capture_miss: self.capture_miss_state.active,
capture_miss_warning: self.capture_miss_state.warn,
capture_miss_edge: self.capture_miss_state.edge,
capture_miss_message: CAPTURE_MISS_WARNING,
```

In `start_stitching()` and `reset_capture_state()`, reset both tracker and state (R1):

```rust
self.capture_miss_tracker = CaptureMissTracker::default();
self.capture_miss_state = CaptureMissState::default();
```

- [ ] **Step 7: Clear warning pulses after frontend status polling**

Add this method to `AppSession`:

```rust
fn clear_capture_miss_warning(&mut self) {
    self.capture_miss_state.warn = false;
}
```

Modify `SharedSession::status()`. The read-clear makes `warn` a one-shot pulse:
the frontend sees it on exactly one poll, then it is cleared.

> **[eng-review R8]** This is a deliberate side effect in a getter-shaped method.
> It is safe because `SharedSession::status()` has exactly **one** production
> consumer — the `session_status` command (`commands.rs`). Document that
> invariant in a comment so a second poller is never added without revisiting the
> pulse semantics. The webview toast (Task 4, R3) depends on this one-shot
> behavior to dismiss correctly.

```rust
pub fn status(&self) -> Result<SessionStatus, String> {
    // NOTE (R8): single consumer only (the `session_status` command). The
    // capture-miss `warn` flag is a one-shot pulse cleared on read; a second
    // poller would swallow the pulse before the frontend sees it.
    let mut inner = self
        .inner
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let status = inner.status();
    inner.clear_capture_miss_warning();
    Ok(status)
}
```

- [ ] **Step 8: Run Rust session tests**

Run:

```bash
rtk cargo test -p rollshot-app status_reports_capture_miss_after_no_match accepted_frame_clears_capture_miss_status -- --nocapture
```

Expected: PASS.

---

### Task 4: Webview Frontend Warning and Preview Affordance

**Files:**
- Modify: `crates/rollshot-app/src/api/capture.ts`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
- Modify: `crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`
- Modify: `crates/rollshot-app/src/App.css`

- [ ] **Step 1: Add TS status fields**

Modify the `SessionStatus` stitching variant in `capture.ts`:

```ts
export type CapturedEdge = 'top' | 'bottom' | 'left' | 'right' | 'unknown'
```

```ts
  | {
      state: 'stitching'
      frame_width: number
      frame_height: number
      region: RegionDto
      stats: StitchStatsDto
      last_outcome: string | null
      capture_miss: boolean
      capture_miss_warning: boolean
      capture_miss_edge: CapturedEdge
      capture_miss_message: string
    }
```

- [ ] **Step 2: Add failing frontend test**

In `CaptureOverlay.test.tsx`, add:

```tsx
it('shows capture miss warning and preview affordance while stitching is disconnected', async () => {
  api.sessionStatus.mockResolvedValue({
    state: 'stitching',
    frame_width: 1000,
    frame_height: 500,
    region: { x: 100, y: 50, width: 400, height: 200 },
    stats: { frame_count: 3, total_width: 400, total_height: 900, last_append: 200 },
    last_outcome: 'no match: ReverseDirection',
    capture_miss: true,
    capture_miss_warning: true,
    capture_miss_edge: 'bottom',
    capture_miss_message: 'Scrolling too fast. Scroll back to the captured edge and try again.',
  } satisfies SessionStatus)
  api.getStitchPreview.mockResolvedValue(new Blob(['png'], { type: 'image/png' }))

  act(() => root.render(<CaptureOverlay />))
  await flush()
  await act(async () => {
    await vi.advanceTimersByTimeAsync(160)
  })

  expect(container.querySelector('.capture-miss-toast')?.textContent).toContain(
    'Scrolling too fast',
  )
  expect(container.querySelector('.preview-recovery-mask')).not.toBeNull()

  // R3: the toast must auto-dismiss after its 3s window even though the next
  // status poll already flipped capture_miss_warning back to false. This guards
  // against the dismiss timer being torn down by the [status]-keyed effect.
  api.sessionStatus.mockResolvedValue({
    state: 'stitching',
    frame_width: 1000,
    frame_height: 500,
    region: { x: 100, y: 50, width: 400, height: 200 },
    stats: { frame_count: 4, total_width: 400, total_height: 1000, last_append: 100 },
    last_outcome: 'appended 100px Bottom',
    capture_miss: false,
    capture_miss_warning: false,
    capture_miss_edge: 'unknown',
    capture_miss_message: 'Scrolling too fast. Scroll back to the captured edge and try again.',
  } satisfies SessionStatus)
  await act(async () => {
    await vi.advanceTimersByTimeAsync(320) // two more poll ticks flip warn->false
  })
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000) // dismiss window elapses
  })
  expect(container.querySelector('.capture-miss-toast')).toBeNull()
})
```

- [ ] **Step 3: Run the frontend test and verify it fails**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay.test.tsx
```

Expected: FAIL because the UI does not render capture-miss elements yet.

- [ ] **Step 4: Extend `AdaptiveStitchPreview` props and rendering**

Modify `AdaptiveStitchPreview.tsx`:

```tsx
import type { CapturedEdge } from '../api/capture'
import type { PreviewPlacement } from '../overlay/placement'

type AdaptiveStitchPreviewProps = {
  imageUrl: string | null
  status: string
  placement: PreviewPlacement
  captureMiss?: boolean
  capturedEdge?: CapturedEdge
  processing?: boolean
}
```

Inside the rendered `.adaptive-stitch-preview`, after the image:

```tsx
{captureMiss ? (
  <div className={`preview-recovery-mask preview-recovery-mask-${capturedEdge ?? 'unknown'}`}>
    <span>Scroll back to the captured edge</span>
  </div>
) : null}
{processing ? <div className="preview-processing-indicator" aria-label="Stitching" /> : null}
```

- [ ] **Step 5: Add transient warning state in `CaptureOverlay`**

In `CaptureOverlay.tsx`, add:

```tsx
const [captureMissToast, setCaptureMissToast] = useState<string | null>(null)
```

Add two effects (R3). They must be separate: a single `[status]`-keyed effect
that both sets the toast and owns the dismiss timer tears its own timer down on
the very next poll — `capture_miss_warning` is a one-shot pulse (Task 3, R8), so
the next poll's cleanup runs `clearTimeout` before the 3s elapses and the toast
never disappears. Keying the expire timer on the toast value instead makes it
survive subsequent polls.

```tsx
// Show the toast when a warning pulse arrives.
useEffect(() => {
  if (status.state === 'stitching' && status.capture_miss_warning) {
    setCaptureMissToast(status.capture_miss_message)
  }
}, [status])

// Dismiss it ~3s after it was last (re)shown. Keyed on the toast value, NOT on
// `status`, so an intervening poll that flips warn->false cannot cancel it.
useEffect(() => {
  if (!captureMissToast) return
  const timer = window.setTimeout(() => setCaptureMissToast(null), 3000)
  return () => window.clearTimeout(timer)
}, [captureMissToast])
```

Pass new props:

```tsx
<AdaptiveStitchPreview
  imageUrl={stitchPreviewUrl}
  status={stats}
  placement={placement}
  captureMiss={status.capture_miss}
  capturedEdge={status.capture_miss_edge}
  processing={status.state === 'stitching'}
/>
```

Render the toast near the top-level of `<main>`:

```tsx
{captureMissToast ? <div className="capture-miss-toast">{captureMissToast}</div> : null}
```

- [ ] **Step 6: Add CSS**

Append to `App.css`:

```css
.capture-miss-toast {
  position: fixed;
  left: 50%;
  top: 56px;
  transform: translateX(-50%);
  max-width: min(520px, calc(100vw - 32px));
  padding: 8px 12px;
  border: 1px solid rgba(251, 191, 36, 0.45);
  border-radius: 6px;
  background: rgba(120, 53, 15, 0.94);
  color: #fffbeb;
  font-size: 13px;
  line-height: 1.35;
  box-shadow: 0 14px 28px rgba(0, 0, 0, 0.3);
  pointer-events: none;
  z-index: 20;
}

.preview-recovery-mask {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 8px;
  background: rgba(15, 23, 42, 0.38);
  color: #fffbeb;
  font-size: 12px;
  line-height: 1.25;
  text-align: center;
  pointer-events: none;
}

.preview-recovery-mask-bottom {
  align-items: end;
}

.preview-recovery-mask-top {
  align-items: start;
}

.preview-recovery-mask-left {
  justify-content: start;
}

.preview-recovery-mask-right {
  justify-content: end;
}

.preview-processing-indicator {
  position: absolute;
  right: 8px;
  top: 8px;
  width: 10px;
  height: 10px;
  border-radius: 999px;
  background: #38bdf8;
  box-shadow: 0 0 0 0 rgba(56, 189, 248, 0.65);
  animation: preview-processing-pulse 1.2s ease-out infinite;
}

@keyframes preview-processing-pulse {
  0% {
    box-shadow: 0 0 0 0 rgba(56, 189, 248, 0.65);
  }
  100% {
    box-shadow: 0 0 0 12px rgba(56, 189, 248, 0);
  }
}
```

- [ ] **Step 7: Run frontend tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay.test.tsx
```

Expected: PASS.

---

### Task 5: Native Linux Driver Events

**Files:**
- Modify: `crates/rollshot-overlay/src/driver.rs`
- Modify: `crates/rollshot-overlay/src/overlay.rs`
- Modify (R6): `crates/rollshot-overlay/Cargo.toml` — add the iced `time`/async-runtime feature for `iced::time::every` (or take the R6 fallback and leave it unchanged).

- [ ] **Step 1: Add the native event type and the emit-decision seam**

> **[eng-review R2]** The `StitchOutcome` → signal converters are now shared in
> `rollshot_overlay_core::capture_miss` — import them, don't redefine. The
> mapping tests live in Task 1.
>
> **[eng-review R4]** The driver previously emitted `CaptureMiss` only when
> `active || warn`, so the **clearing edge was never sent** and the native
> "Scroll back…" marker would never disappear after a recovery. The emit
> decision is extracted into a pure `should_emit_capture_miss` so the
> rising/falling/steady/warn cases are unit-testable (the threaded `begin_stitch`
> loop itself is not).

In `driver.rs`, add imports (`StitchOutcome` is new; `AppendDirection` is no
longer needed locally):

```rust
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};
use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker,
};
```

Add:

```rust
#[derive(Debug, Clone)]
pub enum LiveOverlayEvent {
    Preview(ImageHandle),
    CaptureMiss(CaptureMissState),
}

/// R4: emit on any active-flag transition (rising OR falling — so the recovery
/// edge clears the native marker) and on every warn pulse. Returns whether this
/// state is worth sending given the last `active` we emitted.
fn should_emit_capture_miss(state: &CaptureMissState, last_active: bool) -> bool {
    state.warn || state.active != last_active
}
```

Add tests in `driver.rs`:

```rust
#[test]
fn capture_miss_emit_on_rising_edge() {
    let state = CaptureMissState {
        active: true,
        warn: false,
        ..Default::default()
    };
    assert!(should_emit_capture_miss(&state, false));
}

#[test]
fn capture_miss_emit_on_clearing_edge() {
    // R4: recovery must reach the overlay so the marker disappears.
    let state = CaptureMissState::default(); // active=false, warn=false
    assert!(should_emit_capture_miss(&state, true));
}

#[test]
fn capture_miss_emit_skipped_when_steady_active() {
    let state = CaptureMissState {
        active: true,
        warn: false,
        ..Default::default()
    };
    assert!(!should_emit_capture_miss(&state, true));
}

#[test]
fn capture_miss_emit_on_warn_pulse_when_active_unchanged() {
    let state = CaptureMissState {
        active: true,
        warn: true,
        ..Default::default()
    };
    assert!(should_emit_capture_miss(&state, true));
}
```

- [ ] **Step 2: Run native driver tests and verify they fail or do not compile**

Run:

```bash
rtk cargo test -p rollshot-overlay capture_miss_emit -- --nocapture
```

Expected: FAIL (does not compile) until `should_emit_capture_miss`,
`LiveOverlayEvent`, and the imports are wired in.

- [ ] **Step 3: Change the driver channel type**

In `Driver`, change:

```rust
preview_tx: UnboundedSender<ImageHandle>,
```

to:

```rust
preview_tx: UnboundedSender<LiveOverlayEvent>,
```

Update `start_capture` parameter type the same way.

In `begin_stitch`, create a tracker and the last-emitted-active flag before the
loop (R1: `::default()`):

```rust
let mut capture_miss_tracker = CaptureMissTracker::default();
let mut last_capture_miss_active = false;
```

After `let outcome = stitcher.push_frame(cropped.image);`, update and emit on
any change or warn pulse (R4):

```rust
let capture_miss_state =
    capture_miss_tracker.update(progress_signal_from_outcome(&outcome), Instant::now());
if should_emit_capture_miss(&capture_miss_state, last_capture_miss_active) {
    let _ = preview_tx.unbounded_send(LiveOverlayEvent::CaptureMiss(capture_miss_state));
}
last_capture_miss_active = capture_miss_state.active;
```

When sending preview handles, change:

```rust
let _ = preview_tx.unbounded_send(handle);
```

to:

```rust
let _ = preview_tx.unbounded_send(LiveOverlayEvent::Preview(handle));
```

- [ ] **Step 4: Update native overlay subscription and state**

In `overlay.rs`, change `PREVIEW_RX` to:

```rust
static PREVIEW_RX: Mutex<Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>> =
    Mutex::new(None);
```

Add fields to `Overlay`:

```rust
capture_miss: bool,
capture_miss_warn: bool,
capture_miss_edge: rollshot_overlay_core::capture_miss::CapturedEdge,
capture_miss_message_expires_at: Option<std::time::Instant>,
```

Change `Message::NewPreview(image::Handle)` to:

```rust
LiveEvent(crate::driver::LiveOverlayEvent),
Tick,
```

Map the stream with `Message::LiveEvent`. Change `subscription` to batch the existing event listener, the live-event stream, and a 250 ms tick:

```rust
fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    iced::Subscription::batch([
        event::listen().map(Message::IcedEvent),
        preview_stream(),
        iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::Tick),
    ])
}
```

> **[eng-review R6]** `rollshot-overlay`'s iced features are `["canvas",
> "image"]` — that does **not** include the `time`/async-runtime feature
> `iced::time::every` needs, so this will fail to compile as written. Before
> relying on the tick, add the feature in `crates/rollshot-overlay/Cargo.toml`:
>
> ```toml
> iced = { version = "0.14", features = ["canvas", "image", "tokio"] }
> ```
>
> Verify it compiles cleanly under `iced_layershell` (Step 6's `cargo test`
> build is the gate). **Fallback if the timer feature conflicts with
> `iced_layershell`'s loop:** drop the `Tick`/`every` subscription and clear the
> warning when the *next* `LiveEvent` arrives carrying `warn == false` (the
> driver now emits the clearing edge per R4). Tradeoff: if the user stops
> scrolling entirely the banner lingers until the next event — acceptable
> because recovery requires scrolling back, which itself produces events. State
> which path you took in the final notes.

In `update`, handle:

```rust
Message::LiveEvent(crate::driver::LiveOverlayEvent::Preview(handle)) => {
    state.preview = Some(handle);
    Task::none()
}
Message::LiveEvent(crate::driver::LiveOverlayEvent::CaptureMiss(miss)) => {
    state.capture_miss = miss.active;
    state.capture_miss_edge = miss.edge;
    if miss.warn {
        state.capture_miss_warn = true;
        state.capture_miss_message_expires_at =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
    }
    Task::none()
}
Message::Tick => {
    if state
        .capture_miss_message_expires_at
        .is_some_and(|deadline| std::time::Instant::now() >= deadline)
    {
        state.capture_miss_warn = false;
        state.capture_miss_message_expires_at = None;
    }
    Task::none()
}
```

- [ ] **Step 5: Render native warning and preview affordance outside crop**

In the capture-phase `chrome` column, render the warning before the toolbar when `capture_miss_warn` is true:

```rust
let warning: Option<Element<'_, Message>> = state.capture_miss_warn.then(|| {
    container(text(rollshot_overlay_core::capture_miss::CAPTURE_MISS_WARNING).size(14))
        .padding(8)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                120.0 / 255.0,
                53.0 / 255.0,
                15.0 / 255.0,
                0.94,
            ))),
            text_color: Some(Color::from_rgb(
                255.0 / 255.0,
                251.0 / 255.0,
                235.0 / 255.0,
            )),
            ..Default::default()
        })
        .into()
});
```

When `state.preview` exists and `state.capture_miss` is true, add a small text marker adjacent to the preview in the same outside-crop chrome stack:

```rust
let recovery_marker: Option<Element<'_, Message>> = state.capture_miss.then(|| {
    text("Scroll back to the captured edge")
        .size(13)
        .style(|_theme| iced::widget::text::Style {
            color: Some(Color::from_rgb(
                255.0 / 255.0,
                251.0 / 255.0,
                235.0 / 255.0,
            )),
        })
        .into()
});
```

Build the column in this order: **toolbar, warning, preview, recovery marker**, then pass it through `place_outside_crop`.

> **[eng-review R5]** The toolbar MUST be the first column element. `toolbar_input_rect`
> makes only a `TOOLBAR_W × TOOLBAR_H` rect at the band origin interactive
> (input passthrough). Prepending the warning above the toolbar shifts the
> Stop/Save buttons down by the warning's height, out of that rect — clicks would
> pass through to the underlying app and the toolbar would be dead during the
> warning window. Keeping the toolbar at the band origin preserves the existing
> `toolbar_input_rect` contract; the warning/marker render below it.
>
> Note (layout): the warning + marker add height the preview-size calc does not
> account for, so on a tight band they can extend past the chosen band. This is
> cosmetic only — all chrome stays outside the crop via `place_outside_crop`, so
> it never enters the stitched image. Confirm visually in Task 7 Step 1.

- [ ] **Step 6: Run native overlay tests**

Run (this also compiles the iced `time` feature change from R6, surfacing any
`iced_layershell` conflict before manual acceptance):

```bash
rtk cargo test -p rollshot-overlay capture_miss_emit -- --nocapture
```

Expected: PASS.

---

### Task 6: Full Verification

**Files:**
- No new files unless earlier tasks required the guarded core recovery patch.

- [ ] **Step 1: Run Rust tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 2: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 3: Run frontend tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
```

Expected: PASS.

- [ ] **Step 4: Run frontend typecheck**

Run:

```bash
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: PASS.

- [ ] **Step 5: If `rollshot-core/src/stitcher.rs` changed, run benchmark verification**

Run only if Task 2 required a core behavior change:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/capture-miss-recovery/after.jsonl
```

Then compare against a before run captured before the core edit:

```bash
rtk python3 scripts/bench/compare.py bench-results/runs/capture-miss-recovery/before.jsonl bench-results/runs/capture-miss-recovery/after.jsonl
```

Expected: no unacceptable stitching throughput or quality regression. Record the comparison summary in the final implementation notes.

---

### Task 7: Manual Runtime Acceptance

**Files:**
- No source edits.

- [ ] **Step 1: Linux native overlay acceptance**

Run the native overlay path on KDE/Wayland in debug first:

```bash
rtk cargo run -p rollshot-overlay --bin capture_overlay
```

Expected:

- Slow scroll updates preview normally.
- Fast scroll shows the capture-miss warning once, then throttles.
- Preview chrome shows a captured-edge/recovery affordance instead of appearing silently frozen.
- Scrolling back to the captured edge resumes preview updates.
- Esc finalizes or cancels as before.
- Warning/affordance is outside the crop and not included in the stitched image.

- [ ] **Step 2: Webview/macOS path acceptance**

On macOS, run the Tauri capture flow:

```bash
rtk pnpm --dir crates/rollshot-app run tauri:dev
```

Expected:

- Slow scroll updates preview normally.
- Fast scroll shows the capture-miss warning once, then throttles.
- Preview shows the recovery affordance.
- Scrolling back to the captured edge resumes preview updates.
- Existing overlay exclusion behavior remains intact.

- [ ] **Step 3: Record unchecked platform risk**

If either Linux native or macOS/webview runtime acceptance cannot be run locally, record the exact unchecked path and reason in the final implementation response.
