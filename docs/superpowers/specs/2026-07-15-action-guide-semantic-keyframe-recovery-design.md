# Action Guide Semantic Keyframe Recovery Design

**Date:** 2026-07-15
**Status:** Approved design

## Summary

Action Guide can miss an entire user step when the associated visual change is
small, transient, or never reaches the visual detector's normal settle state.
Keep the existing conservative visual-only detector, and add a bounded
semantic-event observation lane that remembers the strongest meaningful frame
near a click, typing burst, or scroll burst. A semantic event does not create a
step by itself: the window must still observe a non-zero visual response.

The work stays inside `rollshot-action`. It adds deterministic generated
fixtures that reproduce missed-step cases, recovers event-backed steps without
loosening visual-only detection, and retains the recovered peak frame as the
step keyframe.

## Problem

The current detector recognizes frame-to-frame motion, waits for a stable
settle, and compares that settled frame with a rolling baseline. It then emits
a `CandidateMarker` whose `center_id` is used directly as the keyframe.

That behavior is intentionally conservative, but it has three blind spots:

1. A small control or short text change can fall below the global changed-area
   threshold.
2. A transient popover can appear and return to the baseline before a settle,
   causing the important frame to be forgotten.
3. A semantic action and a generic visual settle can describe the same user
   operation, so adding a second detector naively would create duplicate steps.

The user preference is a hybrid policy: semantic-event-backed detection may be
more sensitive because extra candidates can be deleted during review, while
pure visual detection must remain conservative.

## Goals

- Recover click, typing, and scroll steps whose visual response is small or
  transient.
- Preserve the important peak or final stable frame as the step keyframe.
- Keep no-op clicks from creating steps when no visual response is observed.
- Preserve all existing visual-only suppression behavior.
- Produce at most one step when semantic and visual lanes observe the same
  operation.
- Keep memory and work bounded independently of session duration.
- Cover the behavior with deterministic, programmatically generated fixtures.

## Non-Goals

- Changing the Action Guide timeline UI or adding confidence badges.
- Capturing click coordinates or implementing cursor masking.
- Rescanning a full raw-frame recording after capture.
- Adding OCR, ML, or content-semantic classification.
- Deduplicating candidates across unrelated steps.
- Redesigning the nearby-frame strip for visual diversity.
- Changing platform capture or semantic-input implementations.

## Architecture

Frames continue through one `Detector`, but candidates can be supported by two
evidence lanes:

```text
frames ----+--> conservative visual lane ----------> stable visual candidate
           |
events ----+--> bounded semantic observation ------> event-backed candidate
                                                       |
                                            cooldown / merge / output
```

The visual lane retains its existing thresholds, settle rules, baseline
handling, and visual-only behavior.

A semantic event opens or extends a bounded `SemanticWindow`. The window owns
only:

- the event kind and start/deadline timestamps;
- the pre-event luma baseline;
- the strongest qualifying `PeakObservation` seen so far;
- the best stable end-state observation, when one exists; and
- scalar change metrics used to make a deterministic decision.

It does not retain an unbounded frame list. Full-resolution pixels remain in
the existing `FrameStore` ring.

For every analysis frame, the detector runs the visual lane normally and also
compares the frame with an open semantic window's pre-event baseline. Comparing
with the baseline, instead of only the immediately preceding frame, preserves
a transient peak even when later frames return to the original state.

When a semantic window closes, selection proceeds in this order:

1. If the operation reached a meaningful stable end state, choose that stable
   frame.
2. Otherwise, if the remembered peak passes the semantic meaningful-change
   rule, choose the peak frame.
3. Otherwise, emit no candidate.

Semantic and visual evidence are merged before output. If a normal visual
settle and an open semantic window refer to the same operation, the result is
one event-classified candidate, not one event candidate plus one `UiChanged`
candidate. Existing cooldown behavior still applies between distinct output
candidates.

## Semantic Windows

### Click

A click opens a window ending at the existing `click_window_ms` deadline
(600 ms by default). A later click closes the prior click decision before
opening a new window, so one window never represents two clicks. A click with
no qualifying visual response produces no step.

### Typing

Typing activity opens a window at the first event. Further typing events extend
the burst. Enter, Tab, the existing `typing_pause_ms` timeout (700 ms by
default), or recording finish closes it. The final stable frame is preferred;
when no settle occurs, the strongest qualifying response is retained.

### Scroll

The first scroll event opens a window against the pre-scroll baseline. Further
scroll events extend it. The window closes after the existing
`scroll_dwell_ms` timeout (600 ms by default), or at recording finish. A stable
end state is preferred; otherwise the peak is eligible. Returning fully to the
pre-scroll baseline without any qualifying peak still produces no step.

Only one semantic session may own a visual change at a time, retaining the
current priority order of typing, scroll, and generic settle. Click attribution
remains bounded by its click window.

## Meaningful Change

The visual lane continues to require both its normalized global diff threshold
and changed-area ratio threshold.

The semantic lane must detect localized UI changes, so it instead uses change
statistics computed in one pass over equal-sized luma planes:

- mean absolute normalized luma difference;
- number of samples whose absolute delta exceeds the existing per-sample noise
  floor;
- changed-sample ratio; and
- mean absolute delta among changed samples.

A semantic response must have non-zero normalized difference, meet a minimum
changed-sample count, and meet a minimum changed-sample intensity. It does not
need to cover a fixed percentage of the capture region.

The implementation will choose one set of private, non-user-configurable
defaults by running a deterministic threshold sweep over the fixture matrix.
The selected defaults are the most conservative values that pass every
positive fixture while preserving every negative fixture. The committed test
expectations, rather than a public tuning surface, are the behavioral contract.

Dimension mismatch is never meaningful change. If analysis dimensions change
while a semantic window is open, the detector discards that window, clears
incompatible observations, and establishes a new baseline. A structured debug
event reports the degraded transition without retaining pixels or input data.

## Candidate And Retention Flow

`CandidateMarker.center_id` remains the selected keyframe identifier. A marker
also records the latest frame identifier observed while making the decision.
This observation boundary distinguishes a normal settle whose after-window is
still arriving from a semantic peak whose required later context has already
been observed.

`ActionRecorder` computes candidate readiness from both identifiers:

- wait only for replacement frames that have not yet been observed;
- finalize immediately when the semantic window already supplies the required
  post-keyframe context; and
- never wait a second full `window_after` after a late semantic decision.

Before finalizing, the recorder verifies that the selected center still exists
in the full-resolution ring. The product's current Action Guide paths capture
at 5 fps; the default 60-frame ring must retain the 600 ms click/scroll window
and replacement context with ample margin. A regression test fixes that
default-capacity relationship. Custom callers that combine a higher frame rate
or longer semantic timing with a smaller ring are not given an implicit memory
increase. If their peak has rolled out, the candidate is dropped as bounded
loss and a structured diagnostic records the center, observation boundary, and
ring bounds. A step with missing keyframe pixels is never emitted.

The retained window and `nearby` list must contain the selected peak keyframe.
This work does not otherwise change nearby-frame ordering or selection.

## Components

### `metrics.rs`

Add a privacy-safe `ChangeStats` value and a single-pass function over two
`LumaPlane`s. Existing metric functions may delegate to it when doing so keeps
their current observable behavior unchanged. The value contains scalar counts
and magnitudes only.

### `detector.rs`

Add bounded semantic-window and peak-observation state. Integrate it with the
existing click, typing, and scroll sessions rather than building an independent
detector. Extend `CandidateMarker` with the observation boundary required for
correct retention timing. Keep output kinds and privacy-safe reasons unchanged.

### `recorder.rs`

Resolve markers according to selected center and observation boundary. Merge
overlapping semantic and visual evidence before appending a `CandidateStep`.
Keep all diagnostics on stable explicit `rollshot::*` targets with structured
privacy-safe fields.

### `frame_store.rs`

Expose the ring bounds needed to diagnose delayed semantic decisions and test
the default 5-fps/60-frame capacity relationship. Guarantee that a successfully
finalized keyframe is retained and present in its bounded nearby strip.

No app, iced, Linux-input, macOS-input, or export format changes are required.
Both platforms obtain the same behavior through the shared `rollshot-action`
engine. Click position remains `None` on both current platform sources, so the
design does not depend on coordinate masking.

## Deterministic Fixture Matrix

Fixtures are generated as small `RgbaImage` sequences in test code. Each
fixture specifies semantic event timestamps, expected candidate count and
kind, acceptable keyframe identifiers, required pixel state, and forbidden
keyframe states.

| Fixture | Sequence | Expected result |
| --- | --- | --- |
| Small checkbox | Baseline, click, localized checked state | One `Click`; checked state is keyframe |
| Transient popover | Baseline, click, popover, baseline | One `Click`; popover peak is keyframe |
| No-op click | Baseline, click, unchanged baseline | No candidate |
| Animated response | Baseline, click, transitions, stable final | One `Click`; final stable state is keyframe |
| Scroll settle | Baseline, scroll burst, shifted rows, stable | One `Scroll`; stable shifted state is keyframe |
| Typing subtle text | Baseline, typing events, localized glyph change | One `Typing`; completed text state is keyframe |
| Cursor-only visual | Baseline plus localized cursor movement, no event | No candidate |
| Spinner returns | Baseline, animation, baseline, no event | No candidate |
| Stable visual navigation | Baseline, large transition, stable, no event | One `UiChanged` |
| Dropped intermediates | Event sequence with bounded analysis drops | Deterministic result based only on frames actually analyzed; no missing-pixel step |

The dropped-intermediate fixture does not claim recovery of a frame the
detector never received. It verifies that a peak already observed by the
detector survives later queue loss, and that an unseen peak fails safely.

Shared fixture helpers paint controls, popovers, glyph-like bars, scroll rows,
and transitions programmatically. Binary PNG fixtures are not committed.

## Required Invariants

- Replaying identical frames and events produces identical candidates and
  keyframe identifiers.
- Semantic detection is more sensitive than visual-only detection without
  accepting zero visual change.
- A remembered peak remains eligible after the view returns to baseline.
- Concurrent semantic and visual evidence for one operation emits one step.
- Every emitted keyframe is retained and included in its step's `nearby` list.
- All semantic windows and frame buffers remain bounded independently of
  recording duration.
- Existing cursor-only, oscillation-to-baseline, cooldown, burst-merging, and
  visual-settle tests keep passing.

## Diagnostics And Failure Handling

Runtime diagnostics use `tracing` with stable explicit `rollshot::*` targets
and structured fields. Retained fields may include candidate kind, frame IDs,
timestamps, scalar change statistics, queue-drop counts, and ring bounds. They
must not include pixels, raw key data, typed text, device identity, or click
coordinates.

Expected degraded cases fail safely:

- dimension change: discard the incompatible semantic window and re-baseline;
- missing full-resolution peak: drop the candidate as bounded loss;
- analysis queue loss: decide only from frames actually analyzed and report
  existing drop counts; and
- recording finish: close open typing/scroll windows using the same stable-then-
  peak decision order.

## Verification

The implementation is complete when the fixture matrix and existing action
tests pass under:

```bash
rtk cargo test -p rollshot-action
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

This change does not touch `rollshot-core` stitching paths, so stitching
benchmarks are not required. It does not touch capture UI, so no platform UI
runtime verification is required; shared core behavior is exercised for both
platform integrations through the workspace build and test suite.
