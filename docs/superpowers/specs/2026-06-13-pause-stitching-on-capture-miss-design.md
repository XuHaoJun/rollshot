# Pause Stitching on Capture Miss Design

## Summary

When scrolling moves beyond the overlap that Rollshot can reliably match, the
interactive scrolling-capture flow must stop modifying the stitched result.
Rollshot will freeze the committed canvas and live preview, guide the user back
to the last captured edge, and resume only after the current screen reliably
matches the last successful anchor.

The recovery frame confirms continuity but is not appended. The next frame that
moves in the original capture direction may append normally.

## Problem

The overlay already detects capture misses and warns:

> Scrolling too fast. Scroll back to the captured edge and try again.

However, `Stitcher::push_frame` currently performs a mid-capture re-anchor after
two consecutive genuine `NoMatch` outcomes. This changes `last_good` to the
current frame while preserving the existing canvas, allowing stitching to
continue across a content gap. The UI therefore asks the user to recover while
the core continues stitching.

The desired behavior is strict continuity for interactive scrolling capture:

- after repeated misses, pause stitching;
- preserve the last successful anchor and committed canvas;
- wait for the user to scroll back to the captured edge;
- resume without appending the recovery frame;
- never automatically accept a content gap.

## Reference: Snow Shot

Snow Shot keeps separate feature indexes for the two captured edges. A new
frame first matches the expected edge. With its optional `tryRollback` setting,
failure on that edge causes a match attempt against the opposite edge. If
neither edge matches, Snow Shot leaves captured content unchanged and shows a
throttled warning asking the user to return to the captured edge.

Rollshot will adopt the strict no-mutation-on-miss behavior and edge-guidance
UX, but will not add Snow Shot's two-sided history indexes. Recovery searches
only the existing `last_good` anchor.

## Scope

This behavior applies to the shared iced interactive scrolling-capture path,
covering both:

- Linux native iced Wayland layer-shell overlay;
- macOS iced overlay capture path.

CLI, fixture capture, benchmarks, and other direct `rollshot-core` users retain
the existing `Stitcher::push_frame` behavior. This change does not remove the
general-purpose core mid-capture re-anchor fallback.

## Architecture

### Capture-Session Recovery Gate

The iced overlay capture driver owns a small recovery state machine in front of
the stitcher:

```text
Stitching { consecutive_misses }
  -- second consecutive genuine miss -->
Paused { captured_edge }
  -- reliable match against frozen last_good -->
Stitching { consecutive_misses: 0 }
```

The driver remains responsible for interactive capture policy. The stitcher
remains responsible for image matching and canvas construction.

### Core Read-Only Recovery Probe

`rollshot-core` exposes a read-only recovery probe on `Stitcher`. The probe:

- compares a candidate frame against the current `last_good` anchor;
- uses the same matching and verification standards as normal stitching;
- reports whether the candidate reliably overlaps the anchor;
- treats a duplicate of the anchor as a successful recovery;
- does not append content;
- does not re-anchor;
- does not mutate canvas, anchor, stats, axis lock, direction lock, last motion,
  frame counters, or per-frame stitching metrics.

Dimension mismatch, low confidence, verifier rejection, axis change, and no
match are unsuccessful recovery probes.

## State Transitions

### Normal Stitching

While the recovery gate is in `Stitching`, frames use normal
`Stitcher::push_frame`.

- `FirstFrame`, `Appended`, `Duplicate`, and `NoProgress` reset the consecutive
  miss counter.
- `NoMatch { reason: ReverseDirection }` does not increment or reset the
  counter. Normal user rollback must not trigger the pause threshold.
- Other `NoMatch` outcomes and `AxisChanged` increment the counter.
- The second consecutive genuine miss enters `Paused`.

The second miss has already been rejected by the stitcher, so it does not
modify the committed canvas. Once paused, no further frame is passed to
`push_frame` until recovery succeeds.

### Paused Recovery

While paused:

- each new frame is passed only to the read-only recovery probe;
- normal `push_frame` is not called;
- the committed canvas, `last_good`, and preview remain frozen;
- no timeout, automatic skip, or re-anchor occurs.

A reliable overlap or duplicate-anchor result immediately exits paused state.
The successful recovery frame is not appended and does not update the preview.
The next captured frame may use normal stitching.

If capture ends while paused, Rollshot returns the last successfully committed
canvas.

## UI and UX

The paused UI uses the approved edge-guidance treatment:

- keep the live stitch preview frozen;
- show the existing capture-miss warning, using its current three-second
  throttle;
- continuously highlight the last successfully captured edge while paused;
- remove the edge highlight immediately when recovery succeeds.

No separate "stitching resumed" toast is added. The disappearance of the edge
highlight and warning state communicates recovery without extra interruption.
The preview updates only after a later successful append.

The captured edge comes from the most recent successful append direction. If no
known edge exists, the warning remains usable without directional highlight.

## Components

### `rollshot-core`

- Add the read-only recovery-probe API and result type if a distinct result is
  needed for an unambiguous contract.
- Reuse existing matcher and verifier behavior without duplicating matching
  policy.
- Preserve existing `push_frame` and mid-capture re-anchor behavior for
  non-interactive callers.

### `rollshot-overlay-core`

- Add the framework-neutral recovery-gate state machine.
- Classify stitch outcomes into genuine miss, neutral rollback, and progress.
- Produce paused/recovered state needed by the shared overlay UI.
- Keep edge-guidance state separate from transient warning pulses.

### `rollshot-iced-overlay`

- Place the recovery gate in the shared capture driver before normal
  `push_frame` calls.
- In paused state, call only the read-only core recovery probe.
- Emit capture-miss transitions so the UI can keep the captured-edge highlight
  active until recovery.
- Freeze preview publication while paused and skip preview publication for the
  recovery frame.

## Error Handling and Diagnostics

- Recovery probe failure is expected control flow, not an application error.
- Add structured `tracing` events with stable `rollshot::*` targets for entering
  paused state and recovering. Include the captured edge and miss count where
  available.
- Do not log captured pixels or other privacy-sensitive image content.
- Existing fatal crop/capture errors continue to terminate the stitch thread as
  they do today.

## Testing

### Core

- Successful recovery probe leaves canvas, anchor, stats, locks, motion, frame
  counter, and metrics unchanged.
- Duplicate anchor is a successful recovery.
- No match, low confidence, verifier rejection, axis change, and dimension
  mismatch do not recover.

### Overlay Core

- First genuine miss remains in normal stitching.
- Second consecutive genuine miss enters paused state.
- Reverse direction does not increment the miss counter.
- Progress and neutral outcomes reset or preserve the counter as specified.
- Paused state exits only after a successful recovery probe result.
- Edge-guidance state remains active while paused and clears on recovery.

### Iced Overlay

- Paused frames do not call normal `push_frame`.
- Preview remains frozen while paused.
- Warning pulses retain the existing throttle.
- Captured-edge highlight remains active until recovery.
- Recovery frame does not publish a preview.
- The next successful append publishes a preview.

### Integration Sequence

Exercise this sequence for vertical scrolling:

1. Stitch normally downward.
2. Jump beyond recoverable overlap for two frames.
3. Confirm paused state and unchanged committed canvas.
4. Scroll back until the frame overlaps `last_good`.
5. Confirm immediate recovery without appending the recovery frame.
6. Scroll downward again and confirm gap-free appending resumes.

Equivalent framework-neutral state-machine coverage protects horizontal
behavior. Both Linux and macOS shared iced paths must be inspected; runtime
verification on available platforms should be recorded during implementation.

## Verification

Run:

```text
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Because the work adds a read-only API on the stitching path, capture before and
after benchmark results:

```text
rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/pause-stitching-on-capture-miss/before.jsonl
rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/pause-stitching-on-capture-miss/after.jsonl
rtk python3 scripts/bench/compare.py bench-results/runs/pause-stitching-on-capture-miss/before.jsonl bench-results/runs/pause-stitching-on-capture-miss/after.jsonl
```

Normal `push_frame` performance and output should remain unchanged within
benchmark noise because the recovery probe is called only while the interactive
capture session is paused.

## Non-Goals

- Searching multiple historical anchors.
- Matching both captured edges or allowing bidirectional extension.
- Automatically skipping content gaps.
- Removing the core mid-capture re-anchor fallback.
- Adding configuration for pause threshold or recovery behavior.
- Changing capture frame rate or automatically controlling user scrolling.
