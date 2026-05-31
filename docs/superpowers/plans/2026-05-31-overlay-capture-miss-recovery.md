# Overlay Capture-Miss Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add snow-shot-style capture-miss recovery feedback to both Rollshot capture overlays and verify that scrolling back to the captured edge can resume stitching.

**Architecture:** Shared capture-miss state lives in `rollshot-overlay-core`; both Rust capture paths convert `StitchOutcome` into the same state model. The webview path exposes structured status to React, while the native Linux path emits event messages to iced. Core stitching changes are gated by a reproduction test.

**Tech Stack:** Rust workspace crates (`rollshot-core`, `rollshot-overlay-core`, `rollshot-overlay`, `rollshot-app/src-tauri`), React 19 + Vitest in `crates/rollshot-app`, iced layer-shell overlay on Linux.

---

## File Structure

- Create: `crates/rollshot-overlay-core/src/capture_miss.rs`
  - Shared `StitchProgressSignal`, `CapturedEdge`, `PreviewRecoveryAffordance`, `CaptureMissState`, and `CaptureMissTracker`.
- Modify: `crates/rollshot-overlay-core/Cargo.toml`
  - Add `serde` because `CapturedEdge` is serialized through Tauri status.
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

```rust
use std::time::{Duration, Instant};

pub const CAPTURE_MISS_WARNING: &str =
    "Scrolling too fast. Scroll back to the captured edge and try again.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchProgressSignal {
    Accepted { edge: CapturedEdge },
    Missed { edge: CapturedEdge },
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturedEdge {
    Top,
    Bottom,
    Left,
    Right,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewRecoveryAffordance {
    pub active: bool,
    pub edge: CapturedEdge,
    pub processing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureMissState {
    pub active: bool,
    pub warn: bool,
    pub edge: CapturedEdge,
    pub affordance: PreviewRecoveryAffordance,
}

#[derive(Debug)]
pub struct CaptureMissTracker {
    active: bool,
    edge: CapturedEdge,
    last_warning_at: Option<Instant>,
    throttle: Duration,
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
            CaptureMissState {
                active: false,
                warn: false,
                edge: CapturedEdge::Unknown,
                affordance: PreviewRecoveryAffordance {
                    active: false,
                    edge: CapturedEdge::Unknown,
                    processing: false,
                },
            }
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

- [ ] **Step 5: Add serde dependency**

Modify `crates/rollshot-overlay-core/Cargo.toml`:

```toml
[dependencies]
image = { workspace = true }
serde = { workspace = true }
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

Add imports:

```rust
use rollshot_overlay_core::capture_miss::{
    CapturedEdge, CaptureMissState, CaptureMissTracker, StitchProgressSignal,
    CAPTURE_MISS_WARNING,
};
```

Add fields to `AppSession`:

```rust
capture_miss_tracker: CaptureMissTracker,
capture_miss_state: CaptureMissState,
```

Because `AppSession` currently derives `Default`, implement `Default` manually:

Remove `#[derive(Default)]` from `AppSession`, then add:

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
            capture_miss_tracker: CaptureMissTracker::new(Duration::from_millis(3000)),
            capture_miss_state: CaptureMissTracker::new(Duration::from_millis(3000)).state(),
            final_image: None,
            output_path: None,
            error: None,
        }
    }
}
```

- [ ] **Step 5: Convert `StitchOutcome` to shared signals**

Add a local helper in `session.rs`:

```rust
fn captured_edge_from_direction(direction: rollshot_core::AppendDirection) -> CapturedEdge {
    match direction {
        rollshot_core::AppendDirection::Top => CapturedEdge::Top,
        rollshot_core::AppendDirection::Bottom => CapturedEdge::Bottom,
        rollshot_core::AppendDirection::Left => CapturedEdge::Left,
        rollshot_core::AppendDirection::Right => CapturedEdge::Right,
    }
}

fn progress_signal_from_outcome(outcome: &StitchOutcome) -> StitchProgressSignal {
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
```

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

In `start_stitching()` and `reset_capture_state()`, reset both tracker and state:

```rust
self.capture_miss_tracker = CaptureMissTracker::new(Duration::from_millis(3000));
self.capture_miss_state = self.capture_miss_tracker.state();
```

- [ ] **Step 7: Clear warning pulses after frontend status polling**

Add this method to `AppSession`:

```rust
fn clear_capture_miss_warning(&mut self) {
    self.capture_miss_state.warn = false;
}
```

Modify `SharedSession::status()`:

```rust
pub fn status(&self) -> Result<SessionStatus, String> {
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

Add an effect:

```tsx
useEffect(() => {
  if (status.state !== 'stitching' || !status.capture_miss_warning) return
  setCaptureMissToast(status.capture_miss_message)
  const timer = window.setTimeout(() => setCaptureMissToast(null), 3000)
  return () => window.clearTimeout(timer)
}, [status])
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

- [ ] **Step 1: Add the native event type and conversion tests**

In `driver.rs`, add imports:

```rust
use rollshot_core::{AppendDirection, StitchConfig, StitchOutcome, Stitcher};
use rollshot_overlay_core::capture_miss::{
    CapturedEdge, CaptureMissState, CaptureMissTracker, StitchProgressSignal,
};
```

Add:

```rust
#[derive(Debug, Clone)]
pub enum LiveOverlayEvent {
    Preview(ImageHandle),
    CaptureMiss(CaptureMissState),
}

fn captured_edge_from_direction(direction: AppendDirection) -> CapturedEdge {
    match direction {
        AppendDirection::Top => CapturedEdge::Top,
        AppendDirection::Bottom => CapturedEdge::Bottom,
        AppendDirection::Left => CapturedEdge::Left,
        AppendDirection::Right => CapturedEdge::Right,
    }
}

fn progress_signal_from_outcome(outcome: &StitchOutcome) -> StitchProgressSignal {
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
```

Add tests in `driver.rs`:

```rust
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
```

- [ ] **Step 2: Run native driver tests and verify they fail or do not compile**

Run:

```bash
rtk cargo test -p rollshot-overlay no_match_outcome_maps_to_missed_signal duplicate_outcome_maps_to_idle_signal -- --nocapture
```

Expected: FAIL until imports/types are wired correctly.

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

In `begin_stitch`, create a tracker before the loop:

```rust
let mut capture_miss_tracker = CaptureMissTracker::new(Duration::from_millis(3000));
```

After `let outcome = stitcher.push_frame(cropped.image);`, update and emit:

```rust
let capture_miss_state =
    capture_miss_tracker.update(progress_signal_from_outcome(&outcome), Instant::now());
if capture_miss_state.active || capture_miss_state.warn {
    let _ = preview_tx.unbounded_send(LiveOverlayEvent::CaptureMiss(capture_miss_state));
}
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

Build the column in this order: warning, toolbar, preview, recovery marker. Keep passing it through `place_outside_crop`.

- [ ] **Step 6: Run native overlay tests**

Run:

```bash
rtk cargo test -p rollshot-overlay no_match_outcome_maps_to_missed_signal duplicate_outcome_maps_to_idle_signal -- --nocapture
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
