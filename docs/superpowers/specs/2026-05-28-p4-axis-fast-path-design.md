# P4 Axis Fast Path Design

## Goal

Reduce matcher work in steady-state scrolling by trying the locked main axis first, while preserving the existing conservative dual-axis search as the fallback for suspicious or changing motion.

This implements P4 from `docs/stitching-rollshot-optimizations-2.md`: "Axis-Locked Fast Path". The roadmap is the design input; current code remains the source of truth for names, behavior, and integration points.

## Approved Decisions

- The fast path is always enabled when `locked_axis` exists. There is no feature gate and no public `enabled` config.
- Suspicious cross-axis sentinel results fall back to the existing dual-axis search. They do not directly produce `NoMatch`.
- `locked_axis = None` continues to use the existing dual-axis search.
- The first implementation should keep the change matcher-local. It should not alter canvas topology, verifier thresholds, duplicate handling, stitcher anchor updates, capture code, CLI behavior, or app behavior.
- The cross-axis probe radius should be derived privately from `max_cross_axis_px` and remain larger than the tolerance. A new public config type is not required for P4.

## Current Context

The relevant code is in `crates/rollshot-core/src/matcher.rs`:

- `estimate_motion` is the orchestration point. It receives `prev: &PreparedFrame`, `curr: &PreparedFrame`, `locked_axis: Option<ScrollAxis>`, `last_motion`, config, and metrics.
- `coarse_candidates`, `template_candidates`, and `edge_projection_candidates` currently search both `SearchAxis::Vertical` and `SearchAxis::Horizontal`, even when `locked_axis` is set.
- `rank_verified_candidates` already applies `candidate_matches_axis` and `PixelOverlapVerifier`.
- `fast_ncc_score_shifted` already provides the full-resolution NCC primitive needed for a cheap sentinel.
- Test-only `SearchBudget` already counts coarse scores, NCC calls, NCC pixel visits, and verifier calls.

`crates/rollshot-core/src/stitcher.rs` remains responsible for final direction classification with `validate_with_lock`, including `AxisChanged` and `CrossAxisTooLarge` outcomes.

## Design

### Fast-Path Flow

`estimate_motion` should keep the current dual-axis flow intact and add a locked-axis attempt before it:

```text
if dimensions mismatch:
  return DimensionMismatch

if locked_axis exists:
  try locked main axis only
  if candidate verifies and cross-axis sentinel is not suspicious:
    return candidate

run existing dual-axis search
```

The fallback path is the current behavior. This means the optimization can skip work on steady frames without losing the existing recovery behavior for axis changes, drift, or ambiguous frames.

### Axis-Scoped Candidate Gathering

The matcher should split the current candidate builders into axis-scoped helpers:

- `coarse_candidates_for_axes(..., axes: &[SearchAxis], ...)`
- `template_candidates_for_axes(..., axes: &[SearchAxis], ...)`
- `edge_projection_candidates_for_axes(..., axes: &[SearchAxis], ...)`

The existing public-in-module wrappers keep using both axes. The fast path passes a one-element array containing the locked main axis. This keeps the old path easy to compare and avoids duplicating matcher logic.

### Cross-Axis Sentinel

After the fast path finds a verifier-approved candidate, the sentinel probes a small cross-axis NCC window around the same main-axis offset.

For locked vertical:

```text
main offset = candidate.dy
probe dx in [-cross_axis_probe_radius, +cross_axis_probe_radius]
score NCC(dx, main offset)
```

For locked horizontal:

```text
main offset = candidate.dx
probe dy in [-cross_axis_probe_radius, +cross_axis_probe_radius]
score NCC(main offset, dy)
```

The sentinel returns:

```rust
struct CrossAxisCheck {
    estimated_cross_px: i32,
    residual_score: f32,
    suspicious: bool,
}
```

`cross_axis_probe_radius` should be a private matcher helper derived from `max_cross_axis_px`, for example `max_cross_axis_px * 2`. It must be larger than the tolerated cross-axis movement so the sentinel can actually observe drift beyond tolerance without adding public config.

The check is suspicious when the best cross-axis offset exceeds `max_cross_axis_px` or improves meaningfully over the zero-cross/main-axis candidate. The exact probe radius and residual threshold should be private to `matcher.rs`, conservative, and covered by tests. Suspicious means "fast path not trustworthy"; it does not mean "reject the frame".

Because the probe uses a narrow radius, the added work is bounded. With a private radius of `max_cross_axis_px * 2` and the default tolerance of `6`, that is 25 offsets, much cheaper than running both full template axes in steady state.

### Config Surface

No new public feature gate is added.

The first implementation should derive sentinel radius privately from existing `StitchConfig::max_cross_axis_px` and keep using existing `StitchConfig::axis_ratio_threshold` / `max_cross_axis_px` for final validation. If later benchmarks show the sentinel needs independent tuning, that can be a separate follow-up with evidence.

### Metrics

Existing `StitchMetrics` fields are sufficient:

- `coarse_candidates`
- `ncc_offsets_scored`
- `ncc_pixel_visits`
- `template_ncc_us`
- `edge_projection_us`
- `verifier_us`

No new metric is required for P4. The fast path should increment the same counters as the old path so benchmark comparisons remain apples-to-apples.

## Behavior Guarantees

These behaviors must remain unchanged:

- `DimensionMismatch` returns before matcher work and does not update stitcher state.
- `Duplicate` handling stays outside the matcher.
- `locked_axis = None` uses the old dual-axis flow.
- No fast-path candidate, verifier failure, or suspicious sentinel falls back to the old dual-axis flow.
- `AxisChanged` remains discoverable through fallback dual-axis search.
- `CrossAxisTooLarge` remains handled by existing stitcher validation after a candidate is returned.
- `OverlapVerificationFailed` behavior does not change because fast-path candidates still pass through `PixelOverlapVerifier`.
- Output should remain byte-identical for non-suspicious steady sequences and visually unchanged overall.

## Testing Strategy

Add focused matcher unit tests in `crates/rollshot-core/src/matcher.rs`:

- `locked_vertical_uses_main_axis_fast_path`: steady locked vertical motion returns the same candidate as the old dual-axis path with fewer NCC calls or offsets.
- `cross_axis_drift_falls_back_to_dual_axis`: a frame with cross-axis drift is not accepted solely by the main-axis fast path.
- `axis_changed_is_still_reported`: a locked vertical matcher can still surface a horizontal candidate through fallback.
- `ambiguous_first_motion_still_rejected`: unlocked ambiguous first motion continues to reject as before.

Use the existing `SearchBudget` to compare structural work. For the "old path" comparison, tests should call an internal helper that runs the dual-axis path directly. Do not add a public feature gate for testing.

Run broader verification:

```bash
rtk cargo test -p rollshot-core
rtk cargo fmt --check
rtk cargo clippy -p rollshot-core --all-targets -- -D warnings
```

Because this touches `rollshot-core` stitching paths, benchmark before and after:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p4-axis-fast-path/after.jsonl
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p4-axis-fast-path/before.jsonl \
    bench-results/runs/p4-axis-fast-path/after.jsonl
```

The before benchmark should be captured before code changes.

## Acceptance Criteria

- Steady locked vertical and horizontal sequences score fewer NCC offsets than the old dual-axis path.
- Matcher p95 latency decreases on steady locked-axis benchmark fixtures.
- Golden fixture outcomes remain unchanged.
- Diagonal or drifting frames are not mis-appended by the fast path.
- `AxisChanged` / `CrossAxisTooLarge` semantics do not regress.
- No new public config gate is introduced.

## Non-Goals

- No image pyramid work.
- No feature fallback or HNSW changes.
- No verifier threshold changes.
- No stitcher state-machine rewrite.
- No UI, CLI, or capture changes.
- No public API for toggling P4.
