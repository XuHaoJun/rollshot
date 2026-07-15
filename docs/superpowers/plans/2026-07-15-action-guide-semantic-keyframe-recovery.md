# Action Guide Semantic Keyframe Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover small and transient semantic-event-backed Action Guide steps without making visual-only detection noisier, and retain the recovered important frame as the step keyframe.

**Architecture:** Keep the current conservative settle detector as the visual lane. Add bounded semantic observation windows inside the same detector; each window compares frames with its pre-event baseline, remembers one strongest peak, prefers a meaningful stable end state, and emits through the existing candidate/cooldown path. Extend marker readiness so `ActionRecorder` can retain an older peak without waiting an unnecessary second after-window.

**Tech Stack:** Rust 2021, `image` 0.25, existing `tracing` diagnostics, generated `RgbaImage`/`LumaPlane` fixtures, Cargo test/fmt/clippy.

## Global Constraints

- Keep all production changes inside `crates/rollshot-action`; do not change iced UI, capture backends, or platform semantic-input crates.
- Do not add dependencies, OCR, ML, full-session raw-frame storage, click-coordinate tracking, cursor masking, or a user-facing threshold setting.
- Preserve existing visual-only thresholds and suppression behavior.
- A semantic event alone is insufficient: a step requires a qualifying non-zero visual response.
- Semantic windows retain only one baseline, one strongest peak, and scalar metrics; memory stays bounded independently of session duration.
- Keep existing `CandidateKind`, `DetectReason`, guide, and export formats unchanged.
- Runtime diagnostics must use `tracing`, stable explicit `rollshot::*` targets, and privacy-safe structured fields.
- The current product Action Guide capture rate is 5 fps; keep the default 60-frame ring and verify its margin without silently increasing memory for custom callers.
- Use programmatically generated fixtures; do not add binary PNG fixtures.
- Do not create a worktree. Continue on `fix/action-guide-missed-frames` unless the user explicitly requests otherwise.

## File Structure

- Modify `crates/rollshot-action/src/metrics.rs`: add one-pass privacy-safe scalar change statistics.
- Modify `crates/rollshot-action/src/detector.rs`: own semantic windows, peak selection, lane merging, and observation-boundary markers.
- Modify `crates/rollshot-action/src/recorder.rs`: resolve older peak markers according to already-observed after-context.
- Modify `crates/rollshot-action/src/frame_store.rs`: expose privacy-safe ring bounds for bounded-loss diagnostics and verify default capacity.
- Modify `crates/rollshot-action/src/lib.rs`: register the generated fixture test module only under `#[cfg(test)]`.
- Create `crates/rollshot-action/src/semantic_fixture_tests.rs`: generated RGBA end-to-end fixture matrix and pixel-state assertions.

---

### Task 1: Add Single-Pass Change Statistics

**Files:**
- Modify: `crates/rollshot-action/src/metrics.rs:70-132`

**Interfaces:**
- Consumes: existing `LumaPlane`, `Rect`, and `in_mask` behavior.
- Produces: `pub(crate) struct ChangeStats` and `pub(crate) fn change_stats(a: &LumaPlane, b: &LumaPlane, mask: Option<Rect>, per_sample: f32) -> ChangeStats` for the detector.

- [ ] **Step 1: Write failing metric tests**

Add these tests to `metrics.rs`:

```rust
#[test]
fn change_stats_reports_localized_change_count_ratio_and_intensity() {
    let a = LumaPlane {
        width: 4,
        height: 2,
        samples: vec![0.0; 8],
    };
    let b = LumaPlane {
        width: 4,
        height: 2,
        samples: vec![40.0, 20.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };

    let stats = change_stats(&a, &b, None, 12.0);

    assert!((stats.normalized_mean_diff - (60.0 / 8.0 / 255.0)).abs() < 1e-6);
    assert_eq!(stats.changed_samples, 2);
    assert!((stats.changed_ratio - 0.25).abs() < 1e-6);
    assert!((stats.changed_mean_delta - 30.0).abs() < 1e-6);
}

#[test]
fn change_stats_returns_zero_for_mismatched_or_fully_masked_planes() {
    let a = LumaPlane {
        width: 2,
        height: 2,
        samples: vec![0.0; 4],
    };
    let mismatched = LumaPlane {
        width: 1,
        height: 2,
        samples: vec![255.0; 2],
    };
    assert_eq!(change_stats(&a, &mismatched, None, 12.0), ChangeStats::ZERO);

    let changed = LumaPlane {
        width: 2,
        height: 2,
        samples: vec![255.0; 4],
    };
    let full_mask = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };
    assert_eq!(
        change_stats(&a, &changed, Some(full_mask), 12.0),
        ChangeStats::ZERO
    );
}
```

- [ ] **Step 2: Run the focused tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-action metrics::tests::change_stats -- --nocapture
```

Expected: compilation fails because `ChangeStats` and `change_stats` do not exist.

- [ ] **Step 3: Implement the minimal one-pass metric**

Add above `masked_luma_diff`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChangeStats {
    pub normalized_mean_diff: f32,
    pub changed_samples: u32,
    pub changed_ratio: f32,
    pub changed_mean_delta: f32,
}

impl ChangeStats {
    pub const ZERO: Self = Self {
        normalized_mean_diff: 0.0,
        changed_samples: 0,
        changed_ratio: 0.0,
        changed_mean_delta: 0.0,
    };
}

pub(crate) fn change_stats(
    a: &LumaPlane,
    b: &LumaPlane,
    mask: Option<Rect>,
    per_sample: f32,
) -> ChangeStats {
    if a.width != b.width || a.height != b.height || a.samples.is_empty() {
        return ChangeStats::ZERO;
    }

    let mut total_delta = 0.0f32;
    let mut changed_delta = 0.0f32;
    let mut sampled = 0u32;
    let mut changed = 0u32;
    for y in 0..a.height {
        for x in 0..a.width {
            if in_mask(mask, x, y) {
                continue;
            }
            let i = (y * a.width + x) as usize;
            let delta = (a.samples[i] - b.samples[i]).abs();
            total_delta += delta;
            sampled += 1;
            if delta > per_sample {
                changed += 1;
                changed_delta += delta;
            }
        }
    }
    if sampled == 0 {
        return ChangeStats::ZERO;
    }

    ChangeStats {
        normalized_mean_diff: (total_delta / sampled as f32) / 255.0,
        changed_samples: changed,
        changed_ratio: changed as f32 / sampled as f32,
        changed_mean_delta: if changed == 0 {
            0.0
        } else {
            changed_delta / changed as f32
        },
    }
}
```

Do not refactor `masked_luma_diff` or `changed_area_ratio` in this task; preserving their exact behavior keeps the first review isolated.

- [ ] **Step 4: Run metric and crate tests**

Run:

```bash
rtk cargo test -p rollshot-action metrics::tests
rtk cargo test -p rollshot-action --lib
```

Expected: both commands pass; existing metric results remain unchanged.

- [ ] **Step 5: Commit Task 1**

```bash
rtk git add crates/rollshot-action/src/metrics.rs
rtk git commit -m "feat(action): add semantic change statistics"
```

---

### Task 2: Recover Localized And Transient Click Responses

**Files:**
- Modify: `crates/rollshot-action/src/detector.rs:1-347`
- Test: `crates/rollshot-action/src/detector.rs:350-659`

**Interfaces:**
- Consumes: `change_stats(...) -> ChangeStats` from Task 1.
- Produces: bounded `SemanticWindow`, `PeakObservation`, click peak recovery, and `CandidateMarker { observed_through_id, .. }`.

- [ ] **Step 1: Add failing click regression tests**

Add a 2×2 localized helper and the three tests below. Raise only the test visual area threshold to prove the semantic lane, not the visual lane, recovers the frame.

```rust
fn localized(base: f32, changed: f32) -> LumaPlane {
    let mut s = vec![base; 64];
    for y in 0..2 {
        for x in 0..2 {
            s[y * 8 + x] = changed;
        }
    }
    LumaPlane {
        width: 8,
        height: 8,
        samples: s,
    }
}

#[test]
fn click_recovers_localized_change_below_visual_area_threshold() {
    let mut config = cfg();
    config.area_threshold = 0.10; // localized area is 4/64 = 0.0625
    let mut det = Detector::new(config);
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(
        SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        100,
    ));

    assert!(det.observe_frame(&af(1, 200, localized(0.0, 255.0))).is_none());
    let marker = det
        .observe_frame(&af(2, 800, localized(0.0, 255.0)))
        .expect("semantic click window should recover the localized response");

    assert_eq!(marker.kind, CandidateKind::Click);
    assert_eq!(marker.reason, DetectReason::ClickConfirmed);
    assert_eq!(marker.center_id, 2); // stable state is preferred over the peak
    assert_eq!(marker.observed_through_id, 2);
}

#[test]
fn click_recovers_transient_peak_that_returns_to_baseline() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(
        SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        100,
    ));
    assert!(det.observe_frame(&af(1, 200, quadrant(0.0, 255.0))).is_none());
    assert!(det.observe_frame(&af(2, 400, uniform(0.0))).is_none());
    let marker = det
        .observe_frame(&af(3, 800, uniform(0.0)))
        .expect("remembered popover peak should survive the return to baseline");

    assert_eq!(marker.kind, CandidateKind::Click);
    assert_eq!(marker.center_id, 1);
    assert_eq!(marker.at_ms, 200);
    assert_eq!(marker.observed_through_id, 3);
}

#[test]
fn click_with_no_visual_response_still_emits_nothing_after_window_closes() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(
        SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        100,
    ));
    assert!(det.observe_frame(&af(1, 800, uniform(0.0))).is_none());
    assert!(det.finish().is_none());
}
```

- [ ] **Step 2: Run the click tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-action detector::tests::click_recovers -- --nocapture
```

Expected: compilation fails on the missing `observed_through_id`, then the tests fail because expired click windows do not emit remembered peaks.

- [ ] **Step 3: Add bounded semantic window types and thresholds**

Import Task 1 metrics and add private constants/types near `CandidateMarker`:

```rust
use crate::metrics::{
    change_stats, changed_area_ratio, masked_luma_diff, ChangeStats, LumaPlane,
};

const SEMANTIC_MIN_CHANGED_SAMPLES: u32 = 4;
const SEMANTIC_MIN_CHANGED_MEAN_DELTA: f32 = 24.0;

#[derive(Debug, Clone, Copy)]
struct PeakObservation {
    id: FrameId,
    at_ms: Millis,
    stats: ChangeStats,
}

#[derive(Debug, Clone)]
struct SemanticWindow {
    baseline: LumaPlane,
    peak: Option<PeakObservation>,
}

impl SemanticWindow {
    fn new(baseline: LumaPlane) -> Self {
        Self {
            baseline,
            peak: None,
        }
    }

    fn observe(&mut self, frame: &AnalysisFrame, per_sample: f32) {
        let stats = change_stats(&self.baseline, &frame.luma, None, per_sample);
        if !semantic_meaningful(stats) {
            return;
        }
        let replace = self
            .peak
            .is_none_or(|peak| stats.normalized_mean_diff > peak.stats.normalized_mean_diff);
        if replace {
            self.peak = Some(PeakObservation {
                id: frame.id,
                at_ms: frame.at_ms,
                stats,
            });
        }
    }

    fn choose(
        &self,
        frame: &AnalysisFrame,
        per_sample: f32,
        stable: bool,
    ) -> Option<PeakObservation> {
        let current = PeakObservation {
            id: frame.id,
            at_ms: frame.at_ms,
            stats: change_stats(&self.baseline, &frame.luma, None, per_sample),
        };
        if stable && semantic_meaningful(current.stats) {
            Some(current)
        } else {
            self.peak
        }
    }
}

fn semantic_meaningful(stats: ChangeStats) -> bool {
    stats.normalized_mean_diff > 0.0
        && stats.changed_samples >= SEMANTIC_MIN_CHANGED_SAMPLES
        && stats.changed_mean_delta >= SEMANTIC_MIN_CHANGED_MEAN_DELTA
}
```

If the workspace Rust version rejects `Option::is_none_or`, use the equivalent explicit `match`; do not raise the MSRV or add a helper abstraction solely for that call.

- [ ] **Step 4: Extend markers and detector click state**

Add the observation boundary:

```rust
pub struct CandidateMarker {
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub center_id: FrameId,
    pub observed_through_id: FrameId,
}
```

Replace `click_open_until: Option<Millis>` with these fields:

```rust
click_open_until: Option<Millis>,
click_window: Option<SemanticWindow>,
```

Initialize `click_window` to `None`. On a click event, take the most recent analysis state as the pre-event baseline and reset the prior click window:

```rust
SemanticAction::Click { .. } => {
    self.click_open_until = Some(ev.at_ms.saturating_add(self.config.click_window_ms));
    self.click_window = self
        .prev
        .clone()
        .or_else(|| self.baseline.clone())
        .map(SemanticWindow::new);
}
```

When the first analysis frame establishes the detector baseline, also create a missing click window if a click arrived before the first frame.

- [ ] **Step 5: Observe, close, and merge the click window**

After movement bookkeeping, update an open click window with the current frame. Before typing/scroll/generic decisions, close an expired click window:

```rust
if let Some(window) = self.click_window.as_mut() {
    window.observe(frame, self.config.per_sample_threshold);
}

if self.click_open_until.is_some_and(|until| frame.at_ms >= until)
    && !self.in_typing
    && !self.in_scroll
{
    self.click_open_until = None;
    if let Some(window) = self.click_window.take() {
        if let Some(selected) = window.choose(
            frame,
            self.config.per_sample_threshold,
            !self.moving,
        ) {
            self.saw_change = false;
            self.baseline = Some(luma.clone());
            if self.cooldown_ok(frame.at_ms) {
                self.last_candidate_ms = Some(frame.at_ms);
                return Some(CandidateMarker {
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: selected.at_ms,
                    center_id: selected.id,
                    observed_through_id: frame.id,
                });
            }
        }
    }
}
```

When an ordinary generic settle consumes a click, clear both `click_open_until` and `click_window`, and set `observed_through_id: frame.id`. Add `observed_through_id` to every existing marker constructor, including `finish()`, using the latest frame ID when the chosen center is the current frame.

- [ ] **Step 6: Run click and existing detector tests**

Run:

```bash
rtk cargo test -p rollshot-action detector::tests::click -- --nocapture
rtk cargo test -p rollshot-action detector::tests -- --nocapture
```

Expected: all click tests pass; existing visual-only, drag, cooldown, and settle tests remain green.

- [ ] **Step 7: Commit Task 2**

```bash
rtk git add crates/rollshot-action/src/detector.rs
rtk git commit -m "fix(action): recover transient click frames"
```

---

### Task 3: Extend Peak Recovery To Typing And Scroll Bursts

**Files:**
- Modify: `crates/rollshot-action/src/detector.rs:68-347`
- Test: `crates/rollshot-action/src/detector.rs:516-657`

**Interfaces:**
- Consumes: `SemanticWindow`, `PeakObservation`, and observation-boundary markers from Task 2.
- Produces: bounded `typing_window` and `scroll_window` with stable-then-peak closure behavior and single-owner lane priority.

- [ ] **Step 1: Add failing localized typing and transient scroll tests**

```rust
#[test]
fn typing_recovers_localized_text_change_below_visual_area_threshold() {
    let mut config = cfg();
    config.area_threshold = 0.10;
    let mut det = Detector::new(config);
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(SemanticAction::TypingActivity, 100));
    assert!(det.observe_frame(&af(1, 200, localized(0.0, 255.0))).is_none());
    let marker = det
        .observe_frame(&af(2, 900, localized(0.0, 255.0)))
        .expect("typing pause should close on the localized completed state");
    assert_eq!(marker.kind, CandidateKind::Typing);
    assert_eq!(marker.center_id, 2);
    assert_eq!(marker.observed_through_id, 2);
}

#[test]
fn typing_without_visual_response_emits_nothing() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(SemanticAction::TypingActivity, 100));
    assert!(det.observe_frame(&af(1, 900, uniform(0.0))).is_none());
    assert!(det.finish().is_none());
}

#[test]
fn scroll_recovers_qualifying_peak_even_when_view_returns_to_baseline() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(SemanticAction::ScrollActivity, 100));
    assert!(det.observe_frame(&af(1, 200, quadrant(0.0, 255.0))).is_none());
    det.observe_event(ev(SemanticAction::ScrollActivity, 250));
    assert!(det.observe_frame(&af(2, 300, uniform(0.0))).is_none());
    let marker = det
        .observe_frame(&af(3, 900, uniform(0.0)))
        .expect("qualifying scroll peak should be retained");
    assert_eq!(marker.kind, CandidateKind::Scroll);
    assert_eq!(marker.center_id, 1);
    assert_eq!(marker.observed_through_id, 3);
}
```

Replace the old `scroll_returning_to_baseline_emits_nothing_on_finish` expectation with two explicit contracts:

```rust
#[test]
fn scroll_returning_to_baseline_without_a_qualifying_peak_emits_nothing() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(SemanticAction::ScrollActivity, 100));
    det.observe_frame(&af(1, 200, one_pixel(0.0, 13.0)));
    det.observe_frame(&af(2, 300, uniform(0.0)));
    assert!(det.finish().is_none());
}
```

The qualifying-peak test is an intentional behavior change required by the approved spec; the below-threshold return remains suppressed.

- [ ] **Step 2: Run focused tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-action detector::tests::typing_recovers -- --nocapture
rtk cargo test -p rollshot-action detector::tests::scroll_recovers -- --nocapture
```

Expected: localized typing selects the current unconditional behavior for the wrong reason or fails keyframe assertions; transient scroll returns no marker.

- [ ] **Step 3: Add typing and scroll window state**

Add to `Detector` and initialize to `None`:

```rust
typing_window: Option<SemanticWindow>,
scroll_window: Option<SemanticWindow>,
```

On the first typing/scroll event, create the window from the latest luma state. Clear an outstanding click window when a typing or scroll session becomes the owner:

```rust
SemanticAction::TypingActivity => {
    if !self.in_typing {
        self.typing_window = self
            .prev
            .clone()
            .or_else(|| self.baseline.clone())
            .map(SemanticWindow::new);
        self.click_open_until = None;
        self.click_window = None;
    }
    self.in_typing = true;
    self.typing_last_at = ev.at_ms;
}
SemanticAction::ScrollActivity => {
    if !self.in_scroll {
        let baseline = self.prev.clone().or_else(|| self.baseline.clone());
        self.pre_scroll_baseline = baseline.clone();
        self.scroll_window = baseline.map(SemanticWindow::new);
        self.click_open_until = None;
        self.click_window = None;
        self.in_scroll = true;
    }
    self.scroll_last_at = ev.at_ms;
}
```

If an event precedes the first analysis frame, initialize the matching missing window when the first frame establishes the baseline.

- [ ] **Step 4: Update only the highest-priority semantic owner**

After movement bookkeeping, replace unconditional multi-window updates with owner priority:

```rust
if self.in_typing {
    if let Some(window) = self.typing_window.as_mut() {
        window.observe(frame, self.config.per_sample_threshold);
    }
} else if self.in_scroll {
    if let Some(window) = self.scroll_window.as_mut() {
        window.observe(frame, self.config.per_sample_threshold);
    }
} else if let Some(window) = self.click_window.as_mut() {
    window.observe(frame, self.config.per_sample_threshold);
}
```

This prevents one visual response from filling multiple semantic peaks.

- [ ] **Step 5: Add and implement dimension-change recovery**

Add a differently sized plane helper and regression:

```rust
fn uniform_4x4(v: f32) -> LumaPlane {
    LumaPlane {
        width: 4,
        height: 4,
        samples: vec![v; 16],
    }
}

#[test]
fn dimension_change_discards_semantic_window_and_rebaselines() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(
        SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        100,
    ));
    assert!(det.observe_frame(&af(1, 200, uniform_4x4(255.0))).is_none());
    assert!(det.observe_frame(&af(2, 800, uniform_4x4(255.0))).is_none());
    assert!(det.finish().is_none());
}
```

Replace the existing dimension-mismatch diagnostic-only branch with an early recovery branch:

```rust
if self.prev.as_ref().is_some_and(|prev| {
    prev.width != luma.width || prev.height != luma.height
}) {
    let prev = self.prev.as_ref().expect("checked above");
    tracing::debug!(
        target: TARGET_DETECTOR,
        prev_w = prev.width,
        prev_h = prev.height,
        new_w = luma.width,
        new_h = luma.height,
        "analysis frame dimensions changed; discarding semantic windows and re-baselining"
    );
    self.prev = Some(luma.clone());
    self.baseline = Some(luma.clone());
    self.moving = false;
    self.stable_count = 0;
    self.saw_change = false;
    self.click_open_until = None;
    self.click_window = None;
    self.in_typing = false;
    self.typing_force_end = false;
    self.typing_window = None;
    self.in_scroll = false;
    self.pre_scroll_baseline = None;
    self.scroll_window = None;
    return None;
}
```

Keep the existing stable `TARGET_DETECTOR`; do not log frame pixels, event values, or coordinates.

- [ ] **Step 6: Close typing and scroll with stable-then-peak selection**

Replace unconditional typing emission and current-only scroll comparison with a shared marker constructor shaped as follows:

```rust
fn semantic_marker(
    &mut self,
    window: SemanticWindow,
    frame: &AnalysisFrame,
    kind: CandidateKind,
    reason: DetectReason,
) -> Option<CandidateMarker> {
    let selected = window.choose(
        frame,
        self.config.per_sample_threshold,
        !self.moving,
    )?;
    if !self.cooldown_ok(frame.at_ms) {
        return None;
    }
    self.last_candidate_ms = Some(frame.at_ms);
    Some(CandidateMarker {
        kind,
        reason,
        at_ms: selected.at_ms,
        center_id: selected.id,
        observed_through_id: frame.id,
    })
}
```

At typing pause/force-end and scroll dwell, `take()` the corresponding window, reset the existing session flags/baseline state, then call `semantic_marker`. At recording finish, use the last analysis frame and the same method. If no semantic window exists or no qualifying observation exists, emit nothing.

Because `semantic_marker` mutates cooldown while the window/frame are borrowed, move owned values into locals before calling it; do not add `RefCell` or clone full-resolution images. In `finish()`, reconstruct an owned `AnalysisFrame { id, at_ms: at, luma }` from `self.last_frame` plus `self.prev`, then pass it through the same helper.

- [ ] **Step 7: Run all detector tests**

Run:

```bash
rtk cargo test -p rollshot-action detector::tests -- --nocapture
```

Expected: all tests pass. Specifically verify click, typing, scroll, drag, visual oscillation, dimension recovery, cooldown, and finish-flush coverage in the output.

- [ ] **Step 8: Commit Task 3**

```bash
rtk git add crates/rollshot-action/src/detector.rs
rtk git commit -m "fix(action): recover semantic burst peaks"
```

---

### Task 4: Retain Older Peak Markers Correctly

**Files:**
- Modify: `crates/rollshot-action/src/recorder.rs:20-145`
- Modify: `crates/rollshot-action/src/frame_store.rs:118-177`
- Test: `crates/rollshot-action/src/recorder.rs:150-236`
- Test: `crates/rollshot-action/src/frame_store.rs:180-268`

**Interfaces:**
- Consumes: `CandidateMarker.observed_through_id` from Tasks 2–3.
- Produces: remaining-after-frame readiness calculation and privacy-safe ring-bound diagnostics.

- [ ] **Step 1: Add failing readiness and bounded-loss tests**

In `recorder.rs`, extract a pure helper target with these tests:

```rust
#[test]
fn peak_marker_waits_only_for_after_frames_not_already_observed() {
    let marker = CandidateMarker {
        kind: CandidateKind::Click,
        reason: DetectReason::ClickConfirmed,
        at_ms: 200,
        center_id: 2,
        observed_through_id: 7,
    };
    assert_eq!(remaining_after_frames(marker, 8), 3);
}

#[test]
fn current_frame_marker_keeps_the_existing_full_after_window() {
    let marker = CandidateMarker {
        kind: CandidateKind::UiChanged,
        reason: DetectReason::VisualChange,
        at_ms: 700,
        center_id: 7,
        observed_through_id: 7,
    };
    assert_eq!(remaining_after_frames(marker, 8), 8);
}
```

In `frame_store.rs`, add:

```rust
#[test]
fn ring_bounds_report_oldest_and_newest_without_exposing_pixels() {
    let mut store = small_store();
    assert_eq!(store.ring_bounds(), None);
    for i in 0..4u64 {
        store.ingest(frame(i as u8), i * 100);
    }
    assert_eq!(store.ring_bounds(), Some((0, 3)));
}

#[test]
fn default_ring_covers_product_click_window_and_replacement_context_at_five_fps() {
    let store = StoreConfig::default();
    let click_frames = 600u64.div_ceil(200) as usize;
    let required = store.window_before + click_frames + store.window_after + 1;
    assert!(store.ring_capacity >= required);
}
```

- [ ] **Step 2: Run focused tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-action recorder::tests::peak_marker -- --nocapture
rtk cargo test -p rollshot-action frame_store::tests::ring_bounds -- --nocapture
```

Expected: compilation fails because `remaining_after_frames` and `ring_bounds` do not exist.

- [ ] **Step 3: Implement remaining-after readiness**

Add beside `Pending`:

```rust
fn remaining_after_frames(marker: CandidateMarker, window_after: u64) -> u64 {
    let already_observed = marker
        .observed_through_id
        .saturating_sub(marker.center_id);
    window_after.saturating_sub(already_observed)
}
```

Create one `queue_marker` method and use it from both `ingest_frame` and `finish`:

```rust
fn queue_marker(&mut self, marker: CandidateMarker, finishing: bool) {
    let remaining = if finishing {
        0
    } else {
        remaining_after_frames(marker, self.window_after)
    };
    self.pending.push(Pending {
        marker,
        resolve_at: self.frame_count.saturating_add(remaining),
    });
}
```

Replace each duplicated `self.pending.push(...)` block with `self.queue_marker(marker, false)` during ingestion and `self.queue_marker(marker, true)` during finish.

- [ ] **Step 4: Add ring bounds and structured bounded-loss fields**

In `FrameStore`:

```rust
pub(crate) fn ring_bounds(&self) -> Option<(FrameId, FrameId)> {
    Some((self.ring.front()?.id, self.ring.back()?.id))
}
```

Change the existing drop diagnostic without adding pixels, paths, or coordinates:

```rust
let ring_bounds = self.store.ring_bounds();
tracing::debug!(
    target: TARGET_ACTION,
    center = marker.center_id,
    observed_through = marker.observed_through_id,
    ring_oldest = ring_bounds.map(|bounds| bounds.0),
    ring_newest = ring_bounds.map(|bounds| bounds.1),
    "candidate window unavailable; dropping (bounded loss)"
);
```

- [ ] **Step 5: Add a recorder regression for an older transient peak**

Use 8×8 generated images and a click event. The transient peak occurs at frame 1, the semantic decision occurs after the view returns to baseline, and `window_after = 2` is already satisfied by decision time:

```rust
#[test]
fn transient_click_peak_is_retained_as_keyframe_after_window_closes() {
    let mut config = cfg();
    config.area_threshold = 0.10;
    let mut rec = ActionRecorder::new(region(), store_cfg(), config);
    rec.ingest_frame(black(), 0);
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        at_ms: 100,
    });
    rec.ingest_frame(localized_image(), 200); // id 1: important peak
    rec.ingest_frame(black(), 400);
    rec.ingest_frame(black(), 800); // closes click window

    let recording = rec.finish();
    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    assert_eq!(step.keyframe, 1);
    assert!(step.nearby.contains(&1));
    assert!(recording.store.retained(1).is_some());
}
```

Add `localized_image()` plus the required `MouseButton`, `SemanticAction`, and `TimedSemanticAction` test imports. Paint exactly a 2×2 white control on black.

```rust
fn localized_image() -> RgbaImage {
    let mut image = black();
    for y in 0..2 {
        for x in 0..2 {
            image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    image
}
```

- [ ] **Step 6: Run recorder, frame-store, and crate tests**

Run:

```bash
rtk cargo test -p rollshot-action recorder::tests -- --nocapture
rtk cargo test -p rollshot-action frame_store::tests -- --nocapture
rtk cargo test -p rollshot-action --lib
```

Expected: all commands pass; older peak keyframe pixels survive and existing current-frame candidates still retain their full after-window.

- [ ] **Step 7: Commit Task 4**

```bash
rtk git add crates/rollshot-action/src/recorder.rs crates/rollshot-action/src/frame_store.rs
rtk git commit -m "fix(action): retain delayed semantic peaks"
```

---

### Task 5: Add The Generated End-To-End Fixture Matrix

**Files:**
- Create: `crates/rollshot-action/src/semantic_fixture_tests.rs`
- Modify: `crates/rollshot-action/src/lib.rs:10-26`

**Interfaces:**
- Consumes: public `ActionRecorder`, `DetectorConfig`, `StoreConfig`, semantic models, and retained-frame API.
- Produces: generated fixture coverage for positive and negative product behavior; no production API.

- [ ] **Step 1: Register the test-only module and add fixture helpers**

In `lib.rs`:

```rust
#[cfg(test)]
mod semantic_fixture_tests;
```

Create `semantic_fixture_tests.rs` with deterministic paint helpers and a runner:

```rust
use image::{Rgba, RgbaImage};

use crate::{
    downsample_luma, ActionRecorder, AnalysisFrame, CandidateKind, CaptureRegion,
    Detector, DetectorConfig, MouseButton, SemanticAction, StoreConfig,
    TimedSemanticAction,
};

const W: u32 = 32;
const H: u32 = 24;

fn base() -> RgbaImage {
    RgbaImage::from_pixel(W, H, Rgba([24, 24, 24, 255]))
}

fn paint_rect(mut image: RgbaImage, x: u32, y: u32, w: u32, h: u32, v: u8) -> RgbaImage {
    for py in y..(y + h).min(H) {
        for px in x..(x + w).min(W) {
            image.put_pixel(px, py, Rgba([v, v, v, 255]));
        }
    }
    image
}

fn checkbox_checked() -> RgbaImage {
    paint_rect(base(), 2, 2, 2, 2, 240)
}

fn popover() -> RgbaImage {
    paint_rect(base(), 8, 5, 16, 10, 220)
}

fn typed_text() -> RgbaImage {
    let image = paint_rect(base(), 3, 18, 6, 1, 230);
    paint_rect(image, 10, 18, 5, 1, 230)
}

fn scrolled(offset: u32) -> RgbaImage {
    let mut image = base();
    for row in 0..4 {
        let y = (2 + row * 5 + offset) % H;
        image = paint_rect(image, 2, y, 28, 2, 80 + row as u8 * 35);
    }
    image
}

fn cursor_at(x: u32) -> RgbaImage {
    paint_rect(base(), x, 2, 1, 2, 255)
}

fn analysis(id: u64, at_ms: u64, image: &RgbaImage) -> AnalysisFrame {
    AnalysisFrame {
        id,
        at_ms,
        luma: downsample_luma(image, W),
    }
}

fn recorder(mut detector: DetectorConfig) -> ActionRecorder {
    detector.area_threshold = 0.10;
    ActionRecorder::new(
        CaptureRegion {
            x: 0,
            y: 0,
            width: W,
            height: H,
        },
        StoreConfig {
            ring_capacity: 30,
            analysis_capacity: 8,
            analysis_width: W,
            window_before: 2,
            window_after: 2,
            nearby_max: 5,
        },
        detector,
    )
}

fn click(at_ms: u64) -> TimedSemanticAction {
    TimedSemanticAction {
        action: SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        at_ms,
    }
}
```

- [ ] **Step 2: Add positive fixture tests first**

Implement these tests with explicit timestamps and pixel-state assertions:

```rust
#[test]
fn fixture_small_checkbox_is_a_click_step_with_checked_keyframe() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(checkbox_checked(), 200);
    rec.ingest_frame(checkbox_checked(), 800);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(2, 2).0[0], 240);
}

#[test]
fn fixture_transient_popover_is_retained_after_it_disappears() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(popover(), 200);
    rec.ingest_frame(base(), 400);
    rec.ingest_frame(base(), 800);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(10, 7).0[0], 220);
}
```

Add the remaining positive tests with state-identifying pixel assertions:

```rust
#[test]
fn fixture_animated_click_prefers_stable_final_state() {
    let mut rec = recorder(DetectorConfig::default());
    let transition = paint_rect(base(), 4, 4, 8, 8, 100);
    let final_state = paint_rect(base(), 4, 4, 8, 8, 240);
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(transition, 200);
    rec.ingest_frame(final_state.clone(), 300);
    rec.ingest_frame(final_state.clone(), 400);
    rec.ingest_frame(final_state, 500);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(5, 5).0[0], 240);
    assert_ne!(image.get_pixel(5, 5).0[0], 100);
}

#[test]
fn fixture_scroll_settle_uses_shifted_rows() {
    let before = scrolled(0);
    let after = scrolled(2);
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(before.clone(), 0);
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::ScrollActivity,
        at_ms: 100,
    });
    rec.ingest_frame(after.clone(), 200);
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::ScrollActivity,
        at_ms: 250,
    });
    rec.ingest_frame(after.clone(), 400);
    rec.ingest_frame(after, 900);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Scroll);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(3, 4).0[0], 80);
    assert_ne!(image.get_pixel(3, 2), before.get_pixel(3, 2));
}

#[test]
fn fixture_typing_subtle_text_uses_completed_text() {
    let completed = typed_text();
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::TypingActivity,
        at_ms: 100,
    });
    rec.ingest_frame(completed.clone(), 200);
    rec.ingest_frame(completed, 900);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Typing);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(4, 18).0[0], 230);
    assert_eq!(image.get_pixel(11, 18).0[0], 230);
}

#[test]
fn fixture_stable_visual_navigation_remains_ui_changed() {
    let navigated = paint_rect(base(), 0, 0, W, H, 180);
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_frame(navigated.clone(), 200);
    rec.ingest_frame(navigated.clone(), 400);
    rec.ingest_frame(navigated, 600);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::UiChanged);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(0, 0).0[0], 180);
}
```

- [ ] **Step 3: Run positive fixtures and confirm any remaining red cases**

Run:

```bash
rtk cargo test -p rollshot-action semantic_fixture_tests::fixture_ -- --nocapture
```

Expected before final adjustment: any mismatch identifies a concrete threshold, closure, or retention bug. Do not weaken pixel assertions to make a wrong keyframe pass.

- [ ] **Step 4: Add negative and determinism fixtures**

Add complete negative and determinism tests:

```rust
#[test]
fn fixture_no_op_click_emits_nothing() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(base(), 800);
    assert!(rec.finish().candidates.is_empty());
}

#[test]
fn fixture_click_noise_below_intensity_floor_emits_nothing() {
    let low_delta = paint_rect(base(), 2, 2, 2, 2, 40);
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(low_delta.clone(), 200);
    rec.ingest_frame(low_delta, 800);
    assert!(rec.finish().candidates.is_empty());
}

#[test]
fn fixture_cursor_only_visual_change_emits_nothing() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_frame(cursor_at(2), 200);
    rec.ingest_frame(cursor_at(3), 400);
    rec.ingest_frame(base(), 600);
    assert!(rec.finish().candidates.is_empty());
}

#[test]
fn fixture_spinner_returning_to_baseline_emits_nothing() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_frame(popover(), 200);
    rec.ingest_frame(base(), 400);
    rec.ingest_frame(popover(), 600);
    rec.ingest_frame(base(), 800);
    assert!(rec.finish().candidates.is_empty());
}

fn run_checkbox_fixture() -> Vec<crate::CandidateStep> {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(checkbox_checked(), 200);
    rec.ingest_frame(checkbox_checked(), 800);
    rec.finish().candidates
}

#[test]
fn fixture_replay_is_deterministic() {
    assert_eq!(run_checkbox_fixture(), run_checkbox_fixture());
}

#[test]
fn fixture_observed_peak_survives_analysis_id_gap() {
    let mut config = DetectorConfig::default();
    config.area_threshold = 0.10;
    let mut detector = Detector::new(config);
    let base_image = base();
    let popover_image = popover();
    detector.observe_frame(&analysis(0, 0, &base_image));
    detector.observe_event(click(100));
    assert!(detector
        .observe_frame(&analysis(1, 200, &popover_image))
        .is_none());
    let marker = detector
        .observe_frame(&analysis(9, 800, &base_image))
        .expect("observed peak must survive skipped analysis ids");
    assert_eq!(marker.center_id, 1);
    assert_eq!(marker.observed_through_id, 9);
}

#[test]
fn fixture_unseen_peak_is_not_invented() {
    let mut detector = Detector::new(DetectorConfig::default());
    let base_image = base();
    detector.observe_frame(&analysis(0, 0, &base_image));
    detector.observe_event(click(100));
    assert!(detector
        .observe_frame(&analysis(9, 800, &base_image))
        .is_none());
    assert!(detector.finish().is_none());
}
```

The two ID-gap tests deliberately distinguish “observed peak survives later queue loss” from the impossible promise of recovering an unseen frame.

- [ ] **Step 5: Fix only behavior exposed by the fixture matrix**

If fixture failures remain, adjust the private semantic constants or closure ordering in `detector.rs`. The allowed rule is:

```rust
stats.normalized_mean_diff > 0.0
    && stats.changed_samples >= SEMANTIC_MIN_CHANGED_SAMPLES
    && stats.changed_mean_delta >= SEMANTIC_MIN_CHANGED_MEAN_DELTA
```

Choose the largest minimum changed-sample count and largest minimum changed-mean delta that keep every positive fixture green while every negative fixture stays green. Do not lower the visual `DetectorConfig` defaults, add an `OR` bypass, or special-case fixture names.

- [ ] **Step 6: Run the complete action crate suite**

Run:

```bash
rtk cargo test -p rollshot-action -- --nocapture
```

Expected: all unit, fixture, export, storyboard, GIF, and video tests pass with zero failures.

- [ ] **Step 7: Commit Task 5**

```bash
rtk git add crates/rollshot-action/src/lib.rs \
  crates/rollshot-action/src/semantic_fixture_tests.rs \
  crates/rollshot-action/src/detector.rs
rtk git commit -m "test(action): cover missed semantic frames"
```

---

### Task 6: Workspace Verification And Diagnostic Audit

**Files:**
- Modify only files already touched if verification exposes a task-scoped issue.

**Interfaces:**
- Consumes: completed Tasks 1–5.
- Produces: verified workspace-compatible implementation with privacy-safe diagnostics.

- [ ] **Step 1: Audit every new tracing event**

Run:

```bash
rtk rg -n "tracing::|println!|eprintln!|dbg!" \
  crates/rollshot-action/src/detector.rs \
  crates/rollshot-action/src/recorder.rs \
  crates/rollshot-action/src/frame_store.rs
```

Expected: no `println!`, `eprintln!`, or `dbg!` in production paths. Every new tracing event uses `TARGET_DETECTOR` or `TARGET_ACTION`, structured scalar fields, and contains no pixels, typed text, raw keys, device identity, paths, or coordinates.

- [ ] **Step 2: Run focused regression suites**

```bash
rtk cargo test -p rollshot-action detector::tests -- --nocapture
rtk cargo test -p rollshot-action recorder::tests -- --nocapture
rtk cargo test -p rollshot-action semantic_fixture_tests -- --nocapture
```

Expected: all focused suites pass with zero failures.

- [ ] **Step 3: Run full required verification**

```bash
rtk cargo test -p rollshot-action
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all three commands exit 0. Stitching benchmarks are not required because no `rollshot-core` stitching path changes. Platform UI runtime checks are not required because no capture UI code changes; Linux and macOS use the same shared action engine.

- [ ] **Step 4: Inspect the final change surface**

Run:

```bash
rtk git diff HEAD~5 --stat
rtk git status --short
```

Expected: product changes are limited to `metrics.rs`, `detector.rs`, `recorder.rs`, and `frame_store.rs`; test registration/fixtures are limited to `lib.rs` and `semantic_fixture_tests.rs`; `learn-projects/claude-video/` remains untracked and unstaged.

- [ ] **Step 5: Commit verification-only fixes if needed**

If Steps 1–4 required formatting, clippy, or diagnostic corrections, stage only those task files and commit:

```bash
rtk git add crates/rollshot-action/src/metrics.rs \
  crates/rollshot-action/src/detector.rs \
  crates/rollshot-action/src/recorder.rs \
  crates/rollshot-action/src/frame_store.rs \
  crates/rollshot-action/src/lib.rs \
  crates/rollshot-action/src/semantic_fixture_tests.rs
rtk git commit -m "chore(action): finalize semantic recovery checks"
```

If no corrections were needed, do not create an empty commit.
