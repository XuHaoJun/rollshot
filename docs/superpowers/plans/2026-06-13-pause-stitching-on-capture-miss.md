# Pause Stitching on Capture Miss Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pause interactive scrolling capture after two consecutive genuine misses, preserve the last successful anchor and canvas, and resume only after the user scrolls back to a reliably matching frame.

**Architecture:** Keep general-purpose `Stitcher::push_frame` behavior unchanged, but add a preserve-anchor push API and a read-only recovery probe for the iced capture driver. Replace the current first-miss-active tracker with a framework-neutral two-miss recovery gate, wire it into the shared Linux/macOS iced driver, and render a persistent captured-edge guide while paused.

**Tech Stack:** Rust, `image`, `tracing`, iced canvas/widgets, existing Rollshot matcher/verifier and benchmark harness.

---

## File Structure

- Modify `crates/rollshot-core/src/types.rs`: define the public recovery-probe result.
- Modify `crates/rollshot-core/src/lib.rs`: export the recovery-probe result.
- Modify `crates/rollshot-core/src/stitcher.rs`: add preserve-anchor push and read-only recovery-probe APIs while retaining existing `push_frame`.
- Create `crates/rollshot-core/tests/recovery_probe.rs`: verify strict push and probe behavior.
- Modify `crates/rollshot-overlay-core/src/capture_miss.rs`: implement the two-miss recovery gate and warning throttle.
- Modify `crates/rollshot-overlay-core/src/tokens.rs`: define the shared captured-edge guide visual tokens.
- Modify `crates/rollshot-iced-overlay/src/driver.rs`: route normal frames through preserve-anchor push and paused frames through recovery probe.
- Modify `crates/rollshot-iced-overlay/src/app.rs`: persist paused edge state and draw the edge guide.

### Task 1: Capture the Core Benchmark Baseline

**Files:**
- Create locally, do not commit: `bench-results/runs/pause-stitching-on-capture-miss/before.jsonl`

- [ ] **Step 1: Run the baseline benchmark before product-code changes**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
  --out bench-results/runs/pause-stitching-on-capture-miss/before.jsonl
```

Expected: command succeeds and writes JSONL results for all benchmark scenarios.

- [ ] **Step 2: Confirm the baseline artifact exists**

Run:

```bash
rtk test -s bench-results/runs/pause-stitching-on-capture-miss/before.jsonl
```

Expected: exit status `0`.

### Task 2: Add Strict Push and Read-Only Recovery Probe APIs

**Files:**
- Modify: `crates/rollshot-core/src/types.rs`
- Modify: `crates/rollshot-core/src/lib.rs`
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Create: `crates/rollshot-core/tests/recovery_probe.rs`

- [ ] **Step 1: Write failing tests for preserve-anchor push and recovery probing**

Create `crates/rollshot-core/tests/recovery_probe.rs` with focused tests using
`tests/common::{crop_frame, make_scroll_canvas}`:

```rust
mod common;

use common::{crop_frame, make_scroll_canvas};
use image::{Rgba, RgbaImage};
use rollshot_core::{RecoveryProbeResult, StitchConfig, StitchOutcome, Stitcher};

#[test]
fn preserving_anchor_push_never_reanchors_after_repeated_misses() {
    let canvas = make_scroll_canvas(320, 1800);
    let anchor = crop_frame(&canvas, 96, 320);
    let next = crop_frame(&canvas, 192, 320);
    let bad = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));
    let mut stitcher = Stitcher::new(StitchConfig::default());

    assert_eq!(
        stitcher.push_frame_preserving_anchor(crop_frame(&canvas, 0, 320)),
        StitchOutcome::FirstFrame
    );
    assert!(matches!(
        stitcher.push_frame_preserving_anchor(anchor),
        StitchOutcome::Appended { .. }
    ));
    let before = stitcher.stats();

    for _ in 0..4 {
        assert!(matches!(
            stitcher.push_frame_preserving_anchor(bad.clone()),
            StitchOutcome::NoMatch { .. }
        ));
    }

    assert_eq!(stitcher.stats(), before);
    assert!(matches!(
        stitcher.push_frame_preserving_anchor(next),
        StitchOutcome::Appended { .. }
    ));
}

#[test]
fn recovery_probe_accepts_duplicate_and_reverse_overlap_without_mutation() {
    let canvas = make_scroll_canvas(320, 1800);
    let anchor = crop_frame(&canvas, 96, 320);
    let reverse_overlap = crop_frame(&canvas, 32, 320);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    stitcher.push_frame(crop_frame(&canvas, 0, 320));
    stitcher.push_frame(anchor.clone());
    let stats = stitcher.stats();
    let metrics_frame_index = stitcher.last_metrics().frame_index;
    let metrics_outcome = stitcher.last_metrics().outcome;

    assert_eq!(
        stitcher.probe_recovery(&anchor),
        RecoveryProbeResult::Recovered
    );
    assert_eq!(
        stitcher.probe_recovery(&reverse_overlap),
        RecoveryProbeResult::Recovered
    );
    assert_eq!(stitcher.stats(), stats);
    assert_eq!(stitcher.last_metrics().frame_index, metrics_frame_index);
    assert_eq!(stitcher.last_metrics().outcome, metrics_outcome);
}

#[test]
fn recovery_probe_rejects_unrelated_and_dimension_mismatched_frames() {
    let canvas = make_scroll_canvas(320, 1800);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    stitcher.push_frame(crop_frame(&canvas, 0, 320));
    stitcher.push_frame(crop_frame(&canvas, 96, 320));

    let unrelated = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));
    let wrong_size = RgbaImage::from_pixel(200, 320, Rgba([255, 255, 255, 255]));
    assert_eq!(
        stitcher.probe_recovery(&unrelated),
        RecoveryProbeResult::Missed
    );
    assert_eq!(
        stitcher.probe_recovery(&wrong_size),
        RecoveryProbeResult::Missed
    );
}

// Parity guard: a frame the normal push path would accept as an on-axis
// append must probe as `Recovered`, and a genuine non-overlapping frame the
// push path rejects must probe as `Missed`. Pins probe vs push so the shared
// evaluation core cannot drift apart. Uses two independent stitchers so the
// push-side acceptance does not perturb the probe-side anchor.
#[test]
fn probe_recovery_agrees_with_push_accept_reject() {
    let canvas = make_scroll_canvas(320, 1800);
    let forward = crop_frame(&canvas, 96, 320);
    let unrelated = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));

    let mut pushed = Stitcher::new(StitchConfig::default());
    pushed.push_frame(crop_frame(&canvas, 0, 320));
    let mut probed = Stitcher::new(StitchConfig::default());
    probed.push_frame(crop_frame(&canvas, 0, 320));

    assert!(matches!(
        pushed.push_frame(forward.clone()),
        StitchOutcome::Appended { .. }
    ));
    assert_eq!(probed.probe_recovery(&forward), RecoveryProbeResult::Recovered);

    assert!(matches!(
        pushed.push_frame(unrelated.clone()),
        StitchOutcome::NoMatch { .. }
    ));
    assert_eq!(probed.probe_recovery(&unrelated), RecoveryProbeResult::Missed);
}
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core --test recovery_probe
```

Expected: compilation fails because `RecoveryProbeResult`,
`push_frame_preserving_anchor`, and `probe_recovery` do not exist.

- [ ] **Step 3: Define and export the recovery result**

Add to `crates/rollshot-core/src/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryProbeResult {
    Recovered,
    Missed,
}
```

Export it from `crates/rollshot-core/src/lib.rs` alongside `NoMatchReason` and
the other public types.

- [ ] **Step 4: Split push behavior by re-anchor policy**

In `crates/rollshot-core/src/stitcher.rs`, keep existing callers behaviorally
unchanged and add the strict interactive-capture entry point:

```rust
pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
    self.push_frame_with_reanchor(frame, true)
}

pub fn push_frame_preserving_anchor(&mut self, frame: RgbaImage) -> StitchOutcome {
    self.push_frame_with_reanchor(frame, false)
}

fn push_frame_with_reanchor(
    &mut self,
    frame: RgbaImage,
    allow_reanchor: bool,
) -> StitchOutcome {
    // Move the current push_frame body here.
    // Gate reanchor_candidate creation and both reanchor calls on allow_reanchor.
}
```

The strict path must still update ordinary per-frame metrics and return the same
`StitchOutcome`; it only suppresses stale-first-frame and mid-capture re-anchor.

- [ ] **Step 5: Implement the read-only recovery probe**

Add `Stitcher::probe_recovery(&self, frame: &RgbaImage) -> RecoveryProbeResult`.
Use a local `StitchMetrics`, the existing duplicate signature check,
`PreparedFrame`, `estimate_motion`, axis validation, and
`PixelOverlapVerifier`. Accept:

```rust
RecoveryProbeResult::Recovered
```

when the frame is a duplicate of `last_good` or produces a verifier-passing
candidate on the locked axis. Do not apply the locked-direction rejection:
recovery requires recognizing the user's reverse scroll back toward the anchor.
Return `Missed` for absent anchor, dimension mismatch, matcher rejection,
ambiguous/cross-axis motion, axis change, low confidence, insufficient overlap,
or verifier disagreement.

The method must take `&self`, must not call `push_frame`, and must not update
canvas, anchor, stats, locks, last motion, frame counter, or `last_metrics`.

**DRY — do not fork the matching policy.** `push_frame_inner` already encodes
the full accept ladder (duplicate → `estimate_motion` → confidence →
direction/axis → min-append → verifier). Re-typing that ladder inside
`probe_recovery` creates two copies of acceptance policy that will silently
drift, exactly what the design's "reuse existing matcher and verifier behavior
without duplicating matching policy" forbids. Extract the read-only evaluation
core of `push_frame_inner` into a private helper, e.g.

```rust
enum FrameEvaluation { Duplicate, Append { /* candidate, direction, overlap */ }, Reject(NoMatchReason) }

fn evaluate_frame(
    &self,
    anchor: &PreparedFrame,
    frame: &RgbaImage,
    metrics: &mut StitchMetrics,
    enforce_direction_lock: bool,
) -> FrameEvaluation;
```

`push_frame_inner` calls it with `enforce_direction_lock = true` and then mutates
on `Append`; `probe_recovery` calls it with `enforce_direction_lock = false`,
maps `Duplicate`/`Append → Recovered` and `Reject → Missed`, and mutates
nothing. If a clean extraction proves too invasive for one commit, the fallback
is to keep the duplicated ladder but pin parity with the cross-check test below
— note the duplication explicitly in a code comment so the drift risk is visible.

Performance note: building `PreparedFrame` for the candidate requires an owned
`RgbaImage`, so the probe clones `frame` once per paused frame. This is
acceptable because the probe runs *only* while paused (not in the steady-state
hot loop); do not add the clone to the normal push path.

- [ ] **Step 6: Run focused core tests**

Run:

```bash
rtk cargo test -p rollshot-core --test recovery_probe
rtk cargo test -p rollshot-core --test lazy_load_robust
rtk cargo test -p rollshot-core --test stitcher
```

Expected: all tests pass; the existing general-purpose mid-capture re-anchor
test remains green.

- [ ] **Step 7: Commit the core API**

```bash
rtk git add crates/rollshot-core/src/types.rs crates/rollshot-core/src/lib.rs crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests/recovery_probe.rs
rtk git commit -m "feat(core): add strict capture recovery APIs"
```

### Task 3: Replace First-Miss Tracking with a Two-Miss Recovery Gate

**Files:**
- Modify: `crates/rollshot-overlay-core/src/capture_miss.rs`

- [ ] **Step 1: Replace tracker tests with the approved state transitions**

Add or rewrite tests in `capture_miss.rs` to cover:

```rust
#[test]
fn second_consecutive_genuine_miss_enters_paused_state() {
    let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
    gate.update(
        StitchProgressSignal::Accepted { edge: CapturedEdge::Bottom },
        t(0),
    );
    // The miss-signal edge is intentionally `Unknown`; the paused edge below
    // must come from the last *accepted* append (`captured_edge`), proving the
    // gate sources the guide edge from progress, not from the failed frame.
    assert!(
        !gate
            .update(StitchProgressSignal::Missed { edge: CapturedEdge::Unknown }, t(10))
            .active
    );
    let paused = gate.update(StitchProgressSignal::Missed { edge: CapturedEdge::Unknown }, t(20));
    assert!(paused.active);
    assert!(paused.warn);
    assert_eq!(paused.edge, CapturedEdge::Bottom);
}

#[test]
fn reverse_direction_is_neutral_and_preserves_miss_count() {
    let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
    gate.update(StitchProgressSignal::Missed { edge: CapturedEdge::Unknown }, t(0));
    let state = gate.update(StitchProgressSignal::ReverseDirection, t(10));
    assert!(!state.active);
    assert!(
        gate
            .update(StitchProgressSignal::Missed { edge: CapturedEdge::Unknown }, t(20))
            .active
    );
}

#[test]
fn paused_gate_clears_only_after_recovery() {
    let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
    gate.update(StitchProgressSignal::Missed { edge: CapturedEdge::Unknown }, t(0));
    gate.update(StitchProgressSignal::Missed { edge: CapturedEdge::Unknown }, t(10));
    assert!(gate.update_recovery(false, t(20)).active);
    assert!(!gate.update_recovery(true, t(30)).active);
}
```

Also retain tests for warning throttling and immediate warning after a later
fresh pause.

**Update the existing mapping test (same file):** `no_match_outcome_maps_to_missed_signal`
currently feeds `NoMatchReason::ReverseDirection` and asserts it maps to `Missed`.
After this task it must assert `StitchProgressSignal::ReverseDirection`. Add a
companion test that a *non*-reverse `NoMatch` (e.g. `OverlapVerificationFailed`)
and an `AxisChanged` outcome both map to `Missed { .. }`, so the genuine-miss
classification is pinned.

- [ ] **Step 2: Run overlay-core tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-overlay-core capture_miss
```

Expected: tests fail because the current tracker activates on the first miss and
has no recovery-specific update method.

- [ ] **Step 3: Implement the recovery-gate semantics**

Change `StitchProgressSignal` to make policy explicit. **Keep the change
additive** — split `ReverseDirection` out as a new variant but leave `Missed`'s
existing `{ edge }` payload in place:

```rust
pub enum StitchProgressSignal {
    Accepted { edge: CapturedEdge },
    Missed { edge: CapturedEdge },
    ReverseDirection,
    Idle,
}
```

Rationale (architecture/blast-radius): `StitchProgressSignal` is public and
`rollshot-iced-overlay/src/driver.rs` is its only consumer (the `session.rs`
webview path no longer exists — tauri was removed). Three driver unit tests
construct `Missed { edge: ... }`. Dropping the `edge` field would break the
driver crate's compilation and force driver edits into this task, defeating the
Task 2 ∥ Task 3 parallelism and leaving a non-green intermediate commit. Adding
`ReverseDirection` is purely additive: driver matches `Accepted { .. }` only, so
the new variant compiles cleanly and every existing driver test keeps passing.
The gate ignores the miss `edge` and derives the guide edge from `captured_edge`
(set on accepted appends) per the spec; the retained `Missed.edge` is harmless.

Update `progress_signal_from_outcome` so `NoMatchReason::ReverseDirection` maps
to `ReverseDirection`, other `NoMatch` and `AxisChanged` map to `Missed { .. }`
(edge derivation may stay as-is or collapse to `Unknown` — the gate does not use
it), and accepted/idle mappings remain intact.

Add an ASCII state-machine doc-comment above `CaptureMissTracker` mirroring the
`Stitching → Paused → Stitching` diagram from the design doc, so the two-miss /
recovery transitions are legible at the definition site.

Note the pre-existing `PreviewRecoveryAffordance` / `affordance` field on
`CaptureMissState` is currently dead (constructed, read nowhere). Keep
constructing it as today; do not expand or remove it in this task (out of scope
— flag only).

Extend `CaptureMissTracker` with:

```rust
consecutive_misses: u8,
captured_edge: CapturedEdge,
```

Note `captured_edge` is the spec-correct successor to the existing `edge` field
(which today stores the *miss* estimate). Reuse/rename the existing `edge` field
rather than carrying both — `state()` and `update()` should report
`captured_edge`, so there is a single source of truth for the guide edge.

and add:

```rust
pub fn active(&self) -> bool;
pub fn update_recovery(&mut self, recovered: bool, now: Instant) -> CaptureMissState;
```

Rules:

- known accepted edges update `captured_edge` and reset miss count;
- `Idle` resets miss count;
- `ReverseDirection` is neutral and leaves miss count unchanged;
- first `Missed` increments without activating;
- second `Missed` activates and warns;
- paused recovery misses remain active and pulse warnings through the existing
  throttle;
- successful recovery clears active state, miss count, edge, and warning clock.

- [ ] **Step 4: Run overlay-core tests**

Run:

```bash
rtk cargo test -p rollshot-overlay-core capture_miss
```

Expected: all capture-miss tests pass.

- [ ] **Step 5: Commit the recovery gate**

```bash
rtk git add crates/rollshot-overlay-core/src/capture_miss.rs
rtk git commit -m "feat(overlay-core): add two-miss recovery gate"
```

### Task 4: Wire the Shared Iced Driver to Strict Stitching and Recovery Probes

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`

- [ ] **Step 1: Add a testable per-frame processing helper and failing sequence tests**

Introduce tests around a private helper that returns:

```rust
struct ProcessedFrame {
    signal: Option<StitchProgressSignal>,
    capture_miss: CaptureMissState,
    publish_preview: bool,
    publish_activity: bool,
}
```

Add a vertical integration test with this exact sequence:

```rust
normal anchor -> successful append -> unrelated miss -> unrelated miss
-> paused unrelated frame -> recovery overlap -> next forward append
```

The test drives `process_frame(&mut stitcher, &mut gate, frame, now)` directly
with deterministic frames — append/overlap frames cropped from a tall scroll
canvas and a solid-color frame for the genuine miss. `driver.rs`'s test module
has no `make_scroll_canvas` helper today (the existing `scrolling_frame` helper
produces `CapturedFrame`s, not bare `RgbaImage`s); add a small local
`RgbaImage` canvas/crop helper or lift the shared one — do not depend on
`rollshot-core`'s `tests/common`.

Assert after entering paused:

- stats and canvas dimensions stay unchanged across additional unrelated frames;
- `publish_preview` and `publish_activity` are false;
- the committed canvas is preserved while paused — assert directly on
  `stitcher.full_image()` dimensions / `stitcher.stats()` (NOT via
  `Driver::finalize`, which spins real reader/stitch threads and is not
  reachable from this helper-level unit test);
- recovery overlap clears active state but still does not publish preview;
- the next forward append publishes preview and grows stats.

- [ ] **Step 2: Run the driver tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay driver
```

Expected: the new sequence test fails because all frames currently call normal
`push_frame`.

- [ ] **Step 3: Implement the per-frame routing helper**

Add a private helper with this behavior:

```rust
fn process_frame(
    stitcher: &mut Stitcher,
    gate: &mut CaptureMissTracker,
    frame: image::RgbaImage,
    now: Instant,
) -> ProcessedFrame {
    if gate.active() {
        let recovered =
            stitcher.probe_recovery(&frame) == rollshot_core::RecoveryProbeResult::Recovered;
        return ProcessedFrame {
            signal: None,
            capture_miss: gate.update_recovery(recovered, now),
            publish_preview: false,
            publish_activity: false,
        };
    }

    let outcome = stitcher.push_frame_preserving_anchor(frame);
    let signal = progress_signal_from_outcome(&outcome);
    let capture_miss = gate.update(signal, now);
    ProcessedFrame {
        signal: Some(signal),
        capture_miss,
        publish_preview: should_emit_preview(&signal),
        publish_activity: should_emit_accepted_activity(&signal),
    }
}
```

Use structured `tracing` on inactive-to-active and active-to-inactive
transitions with `TARGET_STITCH`, `edge`, and miss/recovery state fields.

- [ ] **Step 4: Replace the live stitch-loop routing**

In `Driver::begin_stitch`, call `process_frame` after cropping. Keep
`spotlight_edge` updated only from accepted known edges. Emit:

- `CaptureMiss` on active transitions or warning pulses;
- `AcceptedActivity` only when `publish_activity`;
- `Preview` only when `publish_preview`.

Paused frames and the successful recovery frame must not call `preview_handle`.

- [ ] **Step 5: Run driver and shared overlay tests**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay driver
rtk cargo test -p rollshot-iced-overlay
```

Expected: all tests pass.

- [ ] **Step 6: Commit shared driver behavior**

```bash
rtk git add crates/rollshot-iced-overlay/src/driver.rs
rtk git commit -m "fix(overlay): pause stitching until capture recovers"
```

### Task 5: Render Persistent Captured-Edge Guidance

**Files:**
- Modify: `crates/rollshot-overlay-core/src/tokens.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`

- [ ] **Step 1: Add failing state and edge-geometry tests**

In `app.rs`, add tests that:

- `LiveOverlayEvent::CaptureMiss(active=true, edge=Bottom)` stores the active
  recovery edge;
- a clearing event removes it immediately;
- a pure warning timeout hides only the toast and does not clear an active edge;
- `recovery_edge_line(crop, edge)` returns the correct two endpoints for
  `Top`, `Bottom`, `Left`, and `Right`, and returns `None` for `Unknown`.

Use this helper contract:

```rust
fn recovery_edge_line(
    crop: Rectangle,
    edge: CapturedEdge,
) -> Option<(Point, Point)>;
```

- [ ] **Step 2: Run app tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay app
```

Expected: compilation fails because active recovery-edge state and
`recovery_edge_line` do not exist.

- [ ] **Step 3: Add shared recovery-edge visual tokens**

Add to `crates/rollshot-overlay-core/src/tokens.rs`:

```rust
pub const RECOVERY_EDGE: Rgba = Rgba::new(0xf5, 0x9e, 0x0b, 1.0);
pub const RECOVERY_EDGE_WIDTH: f32 = 4.0;
```

Extend the token tests to assert the expected CSS color.

- [ ] **Step 4: Store paused edge state in the iced overlay**

Add to `OverlayState`:

```rust
pub(crate) capture_miss_active: bool,
pub(crate) capture_miss_edge: CapturedEdge,
```

On every `LiveOverlayEvent::CaptureMiss(miss)`, update these fields from
`miss.active` and `miss.edge`. **Critical:** today's handler only mutates state
*inside* `if miss.warn { ... }`. The recovery (clearing) event arrives with
`active = false, warn = false`, so the `capture_miss_active`/`capture_miss_edge`
assignment must move *outside* the `if miss.warn` guard — otherwise the guide
never clears on recovery. Keep the toast (`capture_miss_warn` +
`capture_miss_message_expires_at`) update inside the `if miss.warn` branch.
Preserve the existing three-second toast expiry; the tick handler must not clear
active edge state. Add a test asserting a `CaptureMiss { active: false }` event
with `warn: false` clears `capture_miss_active`.

- [ ] **Step 5: Draw the captured-edge guide**

Pass active edge state into `CropCanvas`. While confirmed scrolling capture is
paused, draw a `RECOVERY_EDGE_WIDTH` amber line directly on the corresponding
edge of the crop rectangle using `recovery_edge_line`. Draw nothing for
`CapturedEdge::Unknown`.

- [ ] **Step 6: Run overlay-core and iced app tests**

Run:

```bash
rtk cargo test -p rollshot-overlay-core tokens
rtk cargo test -p rollshot-iced-overlay app
rtk cargo test -p rollshot-iced-overlay
```

Expected: all tests pass.

- [ ] **Step 7: Commit edge guidance**

```bash
rtk git add crates/rollshot-overlay-core/src/tokens.rs crates/rollshot-iced-overlay/src/app.rs
rtk git commit -m "feat(overlay): highlight captured edge while paused"
```

### Task 6: Verify Both Shared Platform Paths and Workspace Quality

**Files:**
- Inspect: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Inspect: `crates/rollshot-iced-overlay/src/macos_capture.rs`
- Modify only if tests expose a required shared-event wiring correction.

- [ ] **Step 1: Confirm both platform runners consume the shared event stream**

Run:

```bash
rtk rg -n "preview_stream\\(|LiveEvent|begin_stitch" \
  crates/rollshot-iced-overlay/src/linux_runner.rs \
  crates/rollshot-iced-overlay/src/macos_capture.rs
```

Expected: both paths call the shared `Driver::begin_stitch` and consume the
shared preview/live-event subscription; no platform-specific recovery logic is
needed.

- [ ] **Step 2: Run workspace tests**

Run:

```bash
rtk cargo test
```

Expected: all workspace tests pass.

- [ ] **Step 3: Run formatting and lint checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both commands pass without warnings.

- [ ] **Step 4: Runtime-check the available platform paths**

On each available runtime platform, manually exercise:

```text
normal scroll -> scroll beyond overlap -> paused warning + edge guide
-> scroll back to last captured edge -> guide clears
-> resume original direction -> preview grows without a content gap
```

Expected: Linux and/or macOS behavior matches the approved sequence; record any
unavailable platform in the completion summary.

- [ ] **Step 5: Capture the after benchmark**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
  --out bench-results/runs/pause-stitching-on-capture-miss/after.jsonl
```

Expected: benchmark succeeds and normal `push_frame` output hashes remain
unchanged.

- [ ] **Step 6: Compare benchmark results**

Run:

```bash
rtk python3 scripts/bench/compare.py \
  bench-results/runs/pause-stitching-on-capture-miss/before.jsonl \
  bench-results/runs/pause-stitching-on-capture-miss/after.jsonl
```

Expected: no correctness drift and no unexplained regression above the
benchmark harness threshold.

- [ ] **Step 7: Commit any verification-only fixes**

If verification required code fixes, stage only those files and commit:

```bash
rtk git commit -m "fix(overlay): address capture recovery verification"
```

If no fixes were required, do not create an empty commit.

### Task 7: Final Change Review

**Files:**
- Review all changed product and test files from Tasks 2-6.

- [ ] **Step 1: Update the code knowledge graph**

Run the code-review-graph `build_or_update_graph` MCP tool for
`/home/noah/rollshot`.

Expected: incremental graph update succeeds.

- [ ] **Step 2: Run graph-assisted change review**

Run code-review-graph `detect_changes` and `get_affected_flows` against the
branch base.

Expected: review confirms the shared `start_stitching` flow is affected and
does not reveal untested high-risk callers.

- [ ] **Step 3: Inspect the final diff**

Run:

```bash
rtk git status --short
rtk git diff main...HEAD --stat
rtk git diff --check main...HEAD
```

Expected: only scoped recovery, UI, test, spec, and plan changes are present;
`git diff --check` succeeds.

- [ ] **Step 4: Record platform runtime-verification limits**

In the implementation completion summary, state which of Linux and macOS were
runtime-tested. If only one platform was available, explicitly record the
unchecked counterpart and residual runtime risk; both still share the tested
iced driver and app state path.
