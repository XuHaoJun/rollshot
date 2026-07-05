# Core Large-Scroll Stitching Design

## Goal

Allow `rollshot-core` to stitch large single-step scrolls that are close to a
page jump but still have enough verifiable overlap. This targets macOS Terminal
and similar apps where one trackpad or mouse gesture can move most of the
visible region.

The change is core-only. It does not alter macOS capture, overlay UI, capture
timing, or user-facing text.

## Problem

The matcher already has a relaxed recovery pass for fast scrolls, but that pass
uses a fixed `RELAXED_SEARCH_RATIO` of `0.85`. A frame jump beyond 85% of the
viewport can still be valid if the remaining overlap is at least
`StitchConfig::min_overlap` and the pixels verify cleanly.

Example: for a 900 px-tall crop and `min_overlap = 64`, a vertical offset up to
836 px is geometrically verifiable. The current fixed 85% ceiling searches only
to 765 px, so offsets in the 766-836 px range can be missed and reported as a
stitching miss even though they are not truly too fast.

## Approach

Keep the existing matcher flow:

1. Normal matcher candidates.
2. Relaxed large-scroll recovery.
3. Feature fallback.

Only adjust the relaxed large-scroll recovery search window. Instead of a fixed
ratio, compute the relaxed limit from the frame dimension and
`StitchConfig::min_overlap`:

- vertical max offset: `height - min_overlap`
- horizontal max offset: `width - min_overlap`

The relaxed pass should still run only after normal candidate verification
fails. Any relaxed candidate must still pass the existing
`rank_verified_candidates` and `PixelOverlapVerifier` checks. This keeps the
behavior bounded by measurable overlap rather than accepting large offsets
blindly.

## Non-Goals

- Do not lower the default `min_overlap`.
- Do not add macOS-specific logic.
- Do not read or synthesize scroll-wheel deltas.
- Do not change public API unless implementation proves it necessary.
- Do not alter UI wording or capture controls.

## Strict Test Requirements

Add focused `rollshot-core` tests that prove both acceptance and rejection:

1. A terminal-like vertical canvas with line/text texture should stitch a
   near-page jump that is beyond the old 85% ceiling but still leaves at least
   `min_overlap` pixels of overlap.
2. The accepted case must assert:
   - outcome is `StitchOutcome::Appended`
   - direction is `AppendDirection::Bottom`
   - axis is `ScrollAxis::Vertical`
   - added pixels are within a narrow tolerance of the known offset
   - computed overlap height is at least `config.min_overlap`
3. A negative case must use an offset that leaves less than `min_overlap`
   overlap and assert:
   - outcome is not `StitchOutcome::Appended`
   - outcome is not `StitchOutcome::NoProgress`
   - `StitchStats::frame_count` and canvas dimensions do not grow
   - any returned best estimate does not produce an accepted overlap below the
     configured floor
4. Existing fast-scroll regression coverage must continue to pass.

## Verification

Run:

```bash
rtk cargo test -p rollshot-core large_scroll
rtk cargo test -p rollshot-core
```

If implementation touches shared matcher behavior beyond the relaxed recovery
window, also run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

No performance benchmark is required for the design itself. If the
implementation materially increases candidate counts or verifier work in
`rollshot-core` stitching paths, capture before/after benchmark numbers using
the repo benchmark workflow.
