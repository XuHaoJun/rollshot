# Action Guide Semantic Keyframe Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover small and transient semantic-event-backed Action Guide steps without making visual-only detection noisier, and retain the recovered important frame as the step keyframe.

**Architecture:** Keep the current conservative settle detector as the visual lane. Add bounded semantic observation windows inside the same detector; each window compares only in-window frames with its pre-event baseline, remembers one strongest peak plus one meaningful stable observation, and emits through the existing candidate/cooldown path. Keep the detector's public marker shape unchanged; `ActionRecorder` privately records how far analysis had progressed when it queues a marker so an older peak does not wait an unnecessary second after-window.

**Tech Stack:** Rust 2021, `image` 0.25, existing `tracing` diagnostics, generated `RgbaImage`/`LumaPlane` fixtures, Cargo test/fmt/clippy.

## Global Constraints

- Keep all production changes inside `crates/rollshot-action`; do not change iced UI, capture backends, or platform semantic-input crates.
- Do not add dependencies, OCR, ML, full-session raw-frame storage, click-coordinate tracking, cursor masking, or a user-facing threshold setting.
- Preserve existing visual-only thresholds and suppression behavior.
- A semantic event alone is insufficient: a step requires a qualifying non-zero visual response.
- Semantic windows retain only one baseline, one strongest peak, one stable observation, and scalar metrics; memory stays bounded independently of session duration.
- Keep existing `CandidateKind`, `DetectReason`, guide, and export formats unchanged.
- Runtime diagnostics must use `tracing`, stable explicit `rollshot::*` targets, and privacy-safe structured fields.
- The current product Action Guide capture rate is 5 fps; keep the default 60-frame ring and verify its margin without silently increasing memory for custom callers.
- Use programmatically generated fixtures; do not add binary PNG fixtures.
- Do not create a worktree. Continue on `fix/action-guide-missed-frames` unless the user explicitly requests otherwise.

## File Structure

- Modify `crates/rollshot-action/src/metrics.rs`: add one-pass privacy-safe scalar change statistics.
- Modify `crates/rollshot-action/src/detector.rs`: own semantic windows, in-window peak/stable selection, lane merging, and finish flushing.
- Modify `crates/rollshot-action/src/recorder.rs`: privately record analysis observation boundaries and resolve older peak markers according to already-observed after-context.
- Modify `crates/rollshot-action/src/frame_store.rs`: expose privacy-safe ring bounds for bounded-loss diagnostics and verify default capacity.
- Modify `crates/rollshot-action/src/lib.rs`: register the generated fixture test module only under `#[cfg(test)]`.
- Create `crates/rollshot-action/src/semantic_fixture_tests.rs`: generated RGBA end-to-end fixture matrix and pixel-state assertions.

## What Already Exists

- `Detector` already owns semantic session timing, visual settle detection, cooldown, and candidate classification. Reuse it; do not add a second detector or state-machine dependency.
- `LumaPlane`, `masked_luma_diff`, and `changed_area_ratio` already provide downsampled visual analysis. Add one scalar aggregation beside them and preserve the existing visual lane exactly.
- `ActionRecorder` already drains analysis frames and queues `CandidateMarker`s. Store the observation boundary in its private `Pending` state rather than widening the public marker API.
- `FrameStore` already provides bounded raw-frame retention and configurable before/after windows. Add privacy-safe bounds introspection; do not add full-session storage.
- Existing detector/recorder tests provide fast synthetic coverage: the pre-change baseline is 126 passing library tests in about 1.43 seconds on this checkout.

## Architecture And State Flow

```text
semantic event
     |
     v
open one owner window (typing > scroll > click)
     |  baseline: one LumaPlane clone at session start
     |  each in-window analysis frame
     +----> meaningful? ---- no ----> ignore
     |            |
     |           yes
     |            +----> strongest peak (max normalized mean diff)
     |            +----> latest meaningful stable observation
     |
deadline / pause / dwell / recording finish
     |
     +----> choose stable, else peak, else no candidate
     |
     v
CandidateMarker (existing public shape)
     |
     v
Pending { marker, observed_through_id, resolve_at } (private recorder state)
     |
     v
retain keyframe + bounded before/after context
```

The first frame strictly after a semantic deadline closes the window before it is observed. A frame exactly at the deadline remains eligible, then closes the window. Add this ordering as a short doc-comment beside `SemanticWindow`, because attributing an unrelated post-deadline frame is a subtle regression risk.

## Test Coverage

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / one-pass localized change statistics, mismatch, full mask | ✓ | — | — | no |
| Task 2 / click localized response, transient peak, stable preference, no-op, deadline ordering, finish flush | ✓ | — | — | no |
| Task 3 / typing and scroll peaks, no visual response, owner priority, dimension reset, finish flush | ✓ | — | — | no |
| Task 4 / private observation boundary, ring bounds, older-peak retention | ✓ | ✓ | — | no |
| Task 5 / generated positive, negative, deterministic, ID-gap, production-default and product-scale downsampling fixtures | ✓ | ✓ | ✓ | no |
| Task 6 / diagnostics, crate suite, fmt, workspace clippy | ✓ | ✓ | ✓ | no |

## Failure Modes

| Failure | Coverage / handling | User-visible result |
|---|---|---|
| Unrelated frame arrives after the click deadline | Task 2 Step 1 tests the boundary; close-before-observe ordering in Step 5 | No false/misattributed step |
| Event has no visual response or only sub-threshold noise | Tasks 2–3 unit tests and Task 5 negatives | No step, intentionally silent |
| Important transient disappears before settle | Task 2 peak test and Task 4 recorder retention test | Peak remains the keyframe |
| Recording ends before a semantic deadline | Task 2 click-finish test and Task 3 finish-flush tests | Best observed state is emitted; no silent loss |
| Frame dimensions change mid-window | Task 3 dimension-reset test and structured debug branch | Window is discarded and re-baselined; no false step |
| Analysis IDs skip because the bounded queue dropped work | Task 5 observed/unseen ID-gap pair | Observed peaks survive; unseen pixels are never invented |
| Peak pixels age out of the frame ring | Task 4 ring-capacity test plus bounded-loss diagnostic | Candidate is dropped with privacy-safe debug fields |
| Semantic and visual lanes see the same operation | Task 3 owner-priority test and Task 5 single-candidate assertions | One candidate, not duplicate steps |
| Full-resolution control is diluted by 1920→384 analysis | Task 5 product-scale generated fixture | Localized action remains detectable at product settings |

No failure mode remains untested, unhandled, and silently user-visible after these additions.

## Performance And Resource Guardrails

- `SemanticWindow::observe` may scan one downsampled luma plane only while a semantic session is open; it must not clone luma or full-resolution RGBA frames per observation.
- A window clones one downsampled baseline only at session start and retains two scalar `PeakObservation`s. Memory remains O(analysis pixels), independent of session duration.
- Preserve the existing bounded analysis queue and 60-frame raw ring. Do not increase defaults to make a test pass.
- The 1920×1080 fixture is generated in memory, uses only a few frames, performs no I/O, and validates the 384-wide product downsampling path without becoming a benchmark.
- No `rollshot-core` stitch path changes, so stitching before/after benchmarks are not required.

## NOT in Scope

- Visual-only threshold changes: deferred because the stated goal is higher recall only when a semantic event supplies context.
- OCR, ML, click coordinates, cursor masking, or content-aware regions: different mechanisms with larger privacy and dependency costs.
- Raw-frame capture recovery: a frame dropped before analysis cannot be reconstructed; this plan only guarantees observed peaks.
- UI/settings work and platform input/capture changes: both platform paths already feed the shared action engine, and no capture UI behavior changes.
- New artifact, crate, dependency, CI publish lane, or distribution work: this is an internal engine behavior change, not a distributable artifact.
- Parallel task execution: sequential execution, no parallelization opportunity; Tasks 1–5 share `crates/rollshot-action` state and build on preceding private contracts.

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
- Produces: bounded `SemanticWindow`, `PeakObservation`, click peak recovery, and unchanged public `CandidateMarker` output.

- [ ] **Step 1: Add failing click regression tests**

Add a 2×2 localized helper and the four tests below. Raise only the test visual area threshold to prove the semantic lane, not the visual lane, recovers the frame.

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
    assert_eq!(marker.center_id, 1); // frame 2 is post-deadline and only closes the window
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

#[test]
fn click_peak_closes_on_finish_before_deadline() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(
        SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        100,
    ));
    assert!(det.observe_frame(&af(1, 200, localized(0.0, 255.0))).is_none());
    let marker = det.finish().expect("finish should flush the observed click peak");
    assert_eq!(marker.kind, CandidateKind::Click);
    assert_eq!(marker.center_id, 1);
}
```

- [ ] **Step 2: Run the click tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-action detector::tests::click_recovers -- --nocapture
rtk cargo test -p rollshot-action detector::tests::click_peak_closes_on_finish -- --nocapture
```

Expected: tests fail because expired/finished click windows do not emit remembered peaks and the post-deadline frame is still eligible for current-frame selection.

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
    stable: Option<PeakObservation>,
}

impl SemanticWindow {
    fn new(baseline: LumaPlane) -> Self {
        Self {
            baseline,
            peak: None,
            stable: None,
        }
    }

    /// Only call for a frame inside the semantic deadline. The first frame
    /// strictly after the deadline closes the window before observation.
    fn observe(&mut self, frame: &AnalysisFrame, per_sample: f32, stable: bool) {
        let stats = change_stats(&self.baseline, &frame.luma, None, per_sample);
        if !semantic_meaningful(stats) {
            return;
        }
        let observation = PeakObservation {
            id: frame.id,
            at_ms: frame.at_ms,
            stats,
        };
        let replace = self
            .peak
            .is_none_or(|peak| stats.normalized_mean_diff > peak.stats.normalized_mean_diff);
        if replace {
            self.peak = Some(observation);
        }
        if stable {
            self.stable = Some(observation);
        }
    }

    fn choose(&self) -> Option<PeakObservation> {
        self.stable.or(self.peak)
    }
}

fn semantic_meaningful(stats: ChangeStats) -> bool {
    stats.normalized_mean_diff > 0.0
        && stats.changed_samples >= SEMANTIC_MIN_CHANGED_SAMPLES
        && stats.changed_mean_delta >= SEMANTIC_MIN_CHANGED_MEAN_DELTA
}
```

The workspace MSRV is Rust 1.94, so `Option::is_none_or` (stable since 1.82) is available.

- [ ] **Step 4: Extend private detector click state without changing marker API**

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

- [ ] **Step 5: Observe only in-window frames, then close and merge**

Before observing, close a click window when `frame.at_ms > click_open_until`; the closing frame must not enter that window. Otherwise observe it with the current stable flag, and close after observation when `frame.at_ms == click_open_until`. Factor the take/select body into one private closure/helper so both branches and `finish()` use identical selection. The core ordering is:

```rust
let until = self.click_open_until;
if until.is_some_and(|deadline| frame.at_ms > deadline) {
    if let Some(marker) = self.close_click_window(frame.at_ms) {
        return Some(marker);
    }
} else {
    if let Some(window) = self.click_window.as_mut() {
        window.observe(frame, self.config.per_sample_threshold, !self.moving);
    }
    if until == Some(frame.at_ms) {
        if let Some(marker) = self.close_click_window(frame.at_ms) {
            return Some(marker);
        }
    }
}
```

`close_click_window` chooses `window.choose()`, applies the existing cooldown, and returns the existing four-field `CandidateMarker`. When an ordinary generic settle consumes a click, clear both click fields. In `finish()`, flush an open click window through the same selection helper before returning `None`; never require a post-deadline frame to preserve an already observed peak.

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
- Consumes: `SemanticWindow`, `PeakObservation`, and unchanged candidate markers from Task 2.
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
    assert!(det.observe_frame(&af(2, 400, localized(0.0, 255.0))).is_none());
    assert!(det.observe_frame(&af(3, 600, localized(0.0, 255.0))).is_none());
    let marker = det
        .observe_frame(&af(4, 900, localized(0.0, 255.0)))
        .expect("typing pause should close on the localized completed state");
    assert_eq!(marker.kind, CandidateKind::Typing);
    assert_eq!(marker.center_id, 3); // latest stable frame inside the pause window
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

#[test]
fn typing_supersedes_open_click_without_duplicate_candidate() {
    let mut det = Detector::new(cfg());
    det.observe_frame(&af(0, 0, uniform(0.0)));
    det.observe_event(ev(
        SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        100,
    ));
    det.observe_event(ev(SemanticAction::TypingActivity, 150));
    assert!(det.observe_frame(&af(1, 200, localized(0.0, 255.0))).is_none());
    let marker = det
        .observe_frame(&af(2, 900, localized(0.0, 255.0)))
        .expect("typing should own the shared visual response");
    assert_eq!(marker.kind, CandidateKind::Typing);
    assert!(det.finish().is_none());
}
```

The qualifying-peak test is an intentional behavior change required by the approved spec; the below-threshold return remains suppressed.

- [ ] **Step 2: Run focused tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-action detector::tests::typing_recovers -- --nocapture
rtk cargo test -p rollshot-action detector::tests::typing_without_visual_response -- --nocapture
rtk cargo test -p rollshot-action detector::tests::typing_supersedes_open_click -- --nocapture
rtk cargo test -p rollshot-action detector::tests::scroll_recovers -- --nocapture
```

Expected: the no-visual typing test fails under the old unconditional typing emission, and transient scroll returns no marker. The positive typing test may already pass for the wrong reason and is not the RED signal by itself.

- [ ] **Step 3: Add typing and scroll window state**

Add to `Detector` and initialize to `None`:

```rust
typing_window: Option<SemanticWindow>,
scroll_window: Option<SemanticWindow>,
```

On the first typing/scroll event, create the window from the latest luma state. Enforce one actual owner, not merely update priority: typing supersedes and clears scroll/click; scroll opens only when typing is inactive and clears click; click events while typing/scroll is active do not open a window. This makes `typing > scroll > click` explicit and prevents dormant windows from emitting later.

The event arms should follow this shape:

```rust
SemanticAction::TypingActivity => {
    if !self.in_typing {
        self.in_scroll = false;
        self.pre_scroll_baseline = None;
        self.scroll_window = None;
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
    if !self.in_typing && !self.in_scroll {
        let baseline = self.prev.clone().or_else(|| self.baseline.clone());
        self.pre_scroll_baseline = baseline.clone();
        self.scroll_window = baseline.map(SemanticWindow::new);
        self.click_open_until = None;
        self.click_window = None;
        self.in_scroll = true;
    }
    if !self.in_typing {
        self.scroll_last_at = ev.at_ms;
    }
}
```

If an event precedes the first analysis frame, initialize the matching missing window when the first frame establishes the baseline.

- [ ] **Step 4: Update only the highest-priority semantic owner**

After movement bookkeeping, determine whether the current owner's deadline/pause/dwell is strictly before `frame.at_ms`. If so, close the owner before observation. Otherwise update only the owner below; a frame exactly at the boundary is eligible and closes immediately afterward:

```rust
if self.in_typing {
    if let Some(window) = self.typing_window.as_mut() {
        window.observe(frame, self.config.per_sample_threshold, !self.moving);
    }
} else if self.in_scroll {
    if let Some(window) = self.scroll_window.as_mut() {
        window.observe(frame, self.config.per_sample_threshold, !self.moving);
    }
} else if let Some(window) = self.click_window.as_mut() {
        window.observe(frame, self.config.per_sample_threshold, !self.moving);
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
let dimension_change = self.prev.as_ref().and_then(|prev| {
    (prev.width != luma.width || prev.height != luma.height)
        .then_some((prev.width, prev.height))
});
if let Some((prev_w, prev_h)) = dimension_change {
    tracing::debug!(
        target: TARGET_DETECTOR,
        prev_w,
        prev_h,
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
    let selected = window.choose()?;
    if !self.cooldown_ok(frame.at_ms) {
        return None;
    }
    self.last_candidate_ms = Some(frame.at_ms);
    Some(CandidateMarker {
        kind,
        reason,
        at_ms: selected.at_ms,
        center_id: selected.id,
    })
}
```

At typing pause/force-end and scroll dwell, use the same close-before-observe boundary ordering as click, `take()` the corresponding window, reset the existing session flags/baseline state, then call `semantic_marker`. At recording finish, flush whichever single semantic owner is open, including click, using the last analysis timestamp for cooldown and the same selector. If no semantic window exists or no qualifying observation exists, emit nothing.

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
- Consumes: unchanged `CandidateMarker` values plus the current analyzed frame ID available in `ActionRecorder`.
- Produces: private observation-boundary readiness calculation and privacy-safe ring-bound diagnostics.

- [ ] **Step 1: Add failing readiness and bounded-loss tests**

In `recorder.rs`, extract a pure helper target with these tests:

```rust
#[test]
fn peak_marker_waits_only_for_after_frames_not_already_observed() {
    assert_eq!(remaining_after_frames(2, 7, 8), 3);
}

#[test]
fn current_frame_marker_keeps_the_existing_full_after_window() {
    assert_eq!(remaining_after_frames(7, 7, 8), 8);
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
rtk cargo test -p rollshot-action recorder::tests::current_frame_marker -- --nocapture
rtk cargo test -p rollshot-action frame_store::tests::ring_bounds -- --nocapture
```

Expected: compilation fails because `remaining_after_frames` and `ring_bounds` do not exist.

- [ ] **Step 3: Implement remaining-after readiness**

Keep the boundary private beside `Pending`:

```rust
struct Pending {
    marker: CandidateMarker,
    observed_through_id: FrameId,
    resolve_at: u64,
}

fn remaining_after_frames(
    center_id: FrameId,
    observed_through_id: FrameId,
    window_after: u64,
) -> u64 {
    let already_observed = observed_through_id.saturating_sub(center_id);
    window_after.saturating_sub(already_observed)
}
```

Track `last_analyzed_id: Option<FrameId>` in `ActionRecorder`, updating it for each frame passed to `Detector::observe_frame`. Create one `queue_marker` method and use it from both `ingest_frame` and `finish`:

```rust
fn queue_marker(
    &mut self,
    marker: CandidateMarker,
    observed_through_id: FrameId,
    finishing: bool,
) {
    let remaining = if finishing {
        0
    } else {
        remaining_after_frames(marker.center_id, observed_through_id, self.window_after)
    };
    self.pending.push(Pending {
        marker,
        observed_through_id,
        resolve_at: self.frame_count.saturating_add(remaining),
    });
}
```

During analysis draining, pass the current `frame.id`. For `detector.finish()`, pass `last_analyzed_id.unwrap_or(marker.center_id)`. Replace each duplicated push block with this helper. Change finalization to receive the complete `Pending`, so its private boundary remains available for diagnostics without entering `CandidateMarker`.

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
    observed_through = pending.observed_through_id,
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
- Modify if fixture evidence requires it: `crates/rollshot-action/src/detector.rs`

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

fn recorder(detector: DetectorConfig) -> ActionRecorder {
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

The shared recorder must use production `DetectorConfig::default()` thresholds. Individual detector unit tests may raise `area_threshold` only when isolating the semantic lane from the visual lane.

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

- [ ] **Step 3: Run positive fixtures and require green**

Run:

```bash
rtk cargo test -p rollshot-action semantic_fixture_tests::fixture_ -- --nocapture
```

Expected: PASS. If a fixture fails, treat it as a concrete closure, selection, or retention defect; do not weaken pixel assertions or globally tune thresholds before locating the cause.

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

Add `fixture_product_scale_checkbox_survives_1920_to_384_downsampling`: generate a 1920×1080 base plus a 24×24 high-contrast checkbox, configure `analysis_width = 384` with otherwise product-default detector thresholds, feed only the baseline, changed frame, and closure/finish frames, then assert exactly one `Click` candidate and the expected full-resolution checkbox pixels in its retained keyframe. This covers block averaging that the 32×24 fixtures intentionally do not model.

Also assert the animated-click and click→typing owner-transition cases each emit exactly one candidate, so semantic and visual lanes cannot silently duplicate an operation.

- [ ] **Step 5: Keep threshold calibration explicit and deterministic**

Treat `SEMANTIC_MIN_CHANGED_SAMPLES = 4` and `SEMANTIC_MIN_CHANGED_MEAN_DELTA = 24.0` as acceptance constants covered by boundary and fixture tests. Fix ordering/selection defects without changing them. If evidence proves a threshold must change, first add a table-driven test that records every positive/negative fixture's `ChangeStats`; enumerate candidate pairs in deterministic lexicographic order (maximize minimum changed-sample count, then minimum changed-mean delta), and choose the first pair that preserves every labeled outcome. The allowed production rule remains:

```rust
stats.normalized_mean_diff > 0.0
    && stats.changed_samples >= SEMANTIC_MIN_CHANGED_SAMPLES
    && stats.changed_mean_delta >= SEMANTIC_MIN_CHANGED_MEAN_DELTA
```

Do not lower visual `DetectorConfig` defaults, add an `OR` bypass, tune by intuition, or special-case fixture names.

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
rtk git diff 6047a00 --stat
rtk git status --short
```

Expected: since the reviewed plan base `6047a00`, product changes are limited to `metrics.rs`, `detector.rs`, `recorder.rs`, and `frame_store.rs`; test registration/fixtures are limited to `lib.rs` and `semantic_fixture_tests.rs`; this reviewed plan may also appear if not committed separately; `learn-projects/claude-video/` remains untracked and unstaged.

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
