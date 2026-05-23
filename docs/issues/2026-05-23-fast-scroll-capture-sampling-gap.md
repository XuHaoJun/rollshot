---
title: Fast manual scrolling can outpace capture sampling and break stitching
status: in-progress
date: 2026-05-23
severity: high
reporter: noah
tags: [capture, stitcher, linux-portal, diagnostics]
---

# Fast manual scrolling can outpace capture sampling and break stitching

## TL;DR

`rollshot capture --backend linux-portal --region portal` can lose stitching
continuity when the user scrolls faster than the effective capture/stitch
cadence. The first frame is present, but adjacent usable frames can differ by
hundreds of pixels; once the last accepted anchor is too far from later frames,
the matcher falls into repeated `FeatureLowInliers` and the final stitched PNG
appears to skip large early sections.

## Symptom / Context

Observed on Linux portal capture after the FAST+KNN fallback landed in
`f6e06c2` and after follow-up diagnostics in `925bccf`.

Representative command:

```bash
cargo run --release -p rollshot-cli -- capture \
  --backend linux-portal \
  --region portal \
  --max-frames 100 \
  --output target/test-artifacts/linux_portal.png \
  --dump-frames target/test-artifacts/linux_frames \
  --debug-match-report target/test-artifacts/linux_capture_report.json
```

Important observations from `linux_capture_report.json`:

- `frames`: 100
- `Appended`: 25
- `Duplicate`: 27
- `NoProgress`: 6
- `NoMatch`: 41
- All `NoMatch` frames reported `FeatureLowInliers`.
- `capture_interval_ms`:
  - min: ~3.8 ms
  - p50: ~53 ms
  - p90: ~171 ms
  - max: ~760 ms
  - 42 of 99 intervals were under 20 ms, so the portal stream is not behaving
    like a stable 5 FPS sampler even when `--fps` is omitted.
- `stitch_elapsed_ms`:
  - p50: ~62 ms
  - p90: ~107 ms
  - max: ~109 ms

Accepted estimates include large vertical jumps:

```text
frame 10: dy=422, method=Edge,      overlap_h=974
frame 22: dy=556, method=Coarse,    overlap_h=840
frame 34: dy=672, method=Coarse,    overlap_h=724
frame 45: dy=687, method=FastHnsw,  overlap_h=709
```

After those large jumps, the run enters long stretches of `FeatureLowInliers`.
This is consistent with adjacent usable captures no longer sharing enough
stable content for the matcher to recover.

## Analysis

This is not primarily a missing-first-frame bug. The first frame is captured
and accepted. The failure mode is that manual scroll velocity can exceed the
effective sampling cadence of the capture pipeline.

Relevant current behavior:

- `rollshot capture` defaults to `--fps 5` in
  `crates/rollshot-cli/src/args.rs`.
- `CaptureOptions::default()` also uses `fps: 5` in
  `crates/rollshot-capture/src/types.rs`.
- The Linux PipeWire format advertises framerate as a range with preferred
  `options.fps`, but allows up to 360 FPS in
  `crates/rollshot-capture/src/linux/pipewire.rs`.
- The CLI capture loop is synchronous at the consumer level:
  `stream.next_frame()` then `stitcher.push_frame(...)` in
  `crates/rollshot-cli/src/cmd_capture.rs`.
- The Linux PipeWire producer is decoupled behind `FrameQueue`; commit
  `925bccf` changed full-queue behavior to preserve older queued frames and
  added `--debug-match-report`, but it does not control scroll velocity or
  guarantee stitchable overlap between captures.

What has been ruled out:

- The first frame is not absent.
- Changing queue overflow from "drop oldest" to "drop newest" prevents one
  way of deleting the beginning, but it does not fix cases where captured
  frames themselves are too far apart in content coordinates.
- An unbounded FIFO alone would preserve more frames, but if the producer only
  observes large content jumps during fast manual scrolling, the FIFO only
  preserves more unstitchable frames. It also risks large memory growth on
  high-resolution captures.

Reference-project notes:

- `wayscrollshot` captures synchronously via `grim`; slow stitching slows the
  next capture, so it has no producer queue that can silently drop the early
  chronological sequence.
- `snow-shot` uses an asynchronous FIFO and FAST/HNSW-style matching, but its
  auto-scroll cadence is around 150 ms and it is event-driven rather than a
  blind fixed-FPS capture loop.

## Proposed Resolution

Prefer fixing the sampling/scroll-control problem before tuning the matcher.

1. Add capture summary diagnostics.
   - Print max/p50/p90 `capture_interval_ms`.
   - Print max accepted `abs(dx/dy)`.
   - Print longest consecutive `NoMatch` run.
   - Print a warning when accepted motion exceeds a threshold such as half the
     viewport or when consecutive `FeatureLowInliers` suggests scroll is too
     fast.

2. Add a controlled auto-scroll mode.
   - Capture an initial baseline frame and accept it before scrolling.
   - Scroll by a bounded step, for example 100-250 px.
   - Capture/stitch after each step or at a controlled cadence.
   - Stop when no new content is appended or when the user cancels.
   - This mirrors the reliable part of reference tools: control velocity
     instead of trying to infer arbitrary manual scroll speed after the fact.

3. Revisit capture buffering after diagnostics.
   - Replace the fixed queue size of 3 with a memory-bounded FIFO if real runs
     show the producer is capturing stitchable intermediate frames that are
     being dropped.
   - Track queue depth and dropped frame counts.
   - Do not use a silent unbounded FIFO in production; large portal frames can
     consume memory very quickly.

4. Only then consider matcher improvements.
   - Larger motion recovery or better feature fallback can help marginal
     cases, but it cannot reliably reconstruct content when adjacent sampled
     frames do not overlap enough.

## Progress

- **Item 1 (diagnostics):** Done. Post-capture summary prints
  `capture_interval_ms` p50/p90/max, `max_accepted_dy`,
  `longest_no_match_run`, and a warning when consecutive NoMatch >= 5.
  Same stats included in `--debug-match-report` JSON via `CaptureSummary`.
- **Item 1.5 (consumer-side frame pacing):** Done. `--min-interval-ms`
  flag skips frames arriving sooner than the threshold after the last
  processed frame. Default 0 (disabled). Reported as `PacingSkipped`
  in per-frame progress and diagnostics summary.
- **Item 2 (auto-scroll):** Not started.
- **Item 3 (capture buffering):** Not started.
- **Item 4 (matcher improvements):** Not started.

## Open Questions

- Should `capture` become primarily auto-scroll driven, with manual scrolling
  treated as a best-effort/debug mode?
- What maximum per-frame scroll delta should rollshot recommend or enforce for
  a typical 1440 px-tall portal region?
- Should `--fps` remain exposed as a user-facing option, or should it become an
  expert/debug control once auto-scroll exists?
- Should the Linux portal backend request a fixed framerate instead of a wide
  range, or is portal/compositor negotiation too inconsistent for that to be
  reliable?
