---
title: Overlay should surface capture-miss and guide scroll-back recovery (snow-shot style)
status: open
date: 2026-05-30
severity: medium
reporter: noah
tags: [overlay, stitcher, linux-portal, ux]
---

# Overlay should surface capture-miss and guide scroll-back recovery (snow-shot style)

## TL;DR

In the native Linux capture overlay (`rollshot-overlay`), scrolling faster than
the matcher can keep up produces a frame that does not overlap the stitched
canvas. The `Stitcher` returns `NoMatch` and — correctly — keeps its anchor
(`last_good`) where it was. But the overlay gives **no feedback**: the live
preview silently freezes and never resumes until Esc, even though the user is
still scrolling. snow-shot solves the same situation at the UX layer: it detects
the miss and tells the user *"scrolling too fast, scroll back to the captured
edge and retry"*. Because the anchor stays valid, scrolling back re-establishes
overlap and stitching resumes. We should add the equivalent capture-miss hint to
the overlay. **Out of scope for the current `feat/native-linux-capture-overlay`
branch; filed for a later pass.**

## Symptom / Context

Repro on KDE 6 Wayland via the harness: scroll fast, pause, then scroll slowly —
the live preview stops updating and does not recover.

The driver's temporary per-frame `StitchOutcome` logging (since removed,
commit `778f274`) captured it clearly. Debug build, fps=15:

```text
... Appended { dy: 96, ... } stitched=980x1503
NoMatch { reason: ReverseDirection, best_estimate: dy=-96,  overlap_h=584 } 980x1503
Appended { dy: 64, ... } stitched=980x1567
NoMatch { reason: ReverseDirection, best_estimate: dy=-32,  overlap_h=648 } 980x1567
NoMatch { reason: ReverseDirection, best_estimate: dy=-160, overlap_h=520 } 980x1567
NoMatch { reason: ReverseDirection, best_estimate: dy=-544, overlap_h=136 } 980x1567
NoMatch { reason: ReverseDirection, best_estimate: dy=-576, overlap_h=104 } 980x1567
... (frozen at 980x1567 until Esc) ...
saved capture_overlay_result.png: 980x1567 (8 frames)
```

The `best_estimate` overlap height collapses (584 → 136 → 104) as the user keeps
scrolling away from the stranded anchor: the new frames sit hundreds of px past
the last accepted content, so there is almost no shared region left and the only
candidate offset is a reverse (`Top`) one, which is rejected.

Build mode matters: this reproduces in **debug** builds (the unoptimized matcher
is slow, so latest-wins skips far between processed frames → big jumps). In
**release** the matcher keeps up, jumps stay small, the anchor keeps advancing,
and capture is smooth — so this is latent, not constant.

## Analysis

Same root condition as
[`2026-05-23-fast-scroll-capture-sampling-gap.md`](./2026-05-23-fast-scroll-capture-sampling-gap.md)
(fast scroll → adjacent usable frames don't overlap → matcher `NoMatch` /
`FeatureLowInliers`), but a **different layer and mitigation**:

- That issue is about the **capture/CLI sampling** mechanism: prevent the gap
  via diagnostics, frame pacing, controlled auto-scroll, and buffering.
- This issue is about the **overlay's user-facing recovery**: when a gap does
  happen, don't freeze silently — detect it and guide the user to recover.

Why recovery is possible without a core change:

- `Stitcher::push_frame` returns `NoMatch` on an early path that does **not**
  touch `last_good` (`crates/rollshot-core/src/stitcher.rs`, `push_frame_inner`);
  the anchor only advances on an accepted append. So the anchor remains valid.
- Therefore scrolling **back up** until the live frame overlaps the
  last-stitched content re-establishes a forward (`Bottom`) match → appending
  resumes. No re-anchor logic is required for the user-driven recovery path.

The overlay already has the signal it needs: the stitch thread receives the
`StitchOutcome` from every `push_frame` (`crates/rollshot-overlay/src/driver.rs`,
`begin_stitch`). It just doesn't currently communicate match state to the UI —
only the preview image handle is sent.

Reference — snow-shot does exactly this:

- `learn-projects/snow-shot/.../scrollScreenshotTool/index.tsx`: when the Rust
  side returns `edge_position === undefined` (match failed) it calls
  `showCaptureMissMessage()`.
- `learn-projects/snow-shot/src/messages/zhHans/draw.ts:104`:
  `"draw.scrollScreenshot.captureMiss": "滚动过快,请滚动到已截取的边缘再次尝试"`.

## Proposed Resolution

Preferred — **overlay-only capture-miss hint (mirrors snow-shot), no core
change:**

1. In the driver stitch thread, track consecutive `NoMatch` since the last
   `Appended`. Ignore `Duplicate` / `NoProgress` (those are pauses, not misses).
2. After a small streak (e.g. 2), emit a `CaptureMiss` status to the overlay;
   on the next `Appended`, emit `Ok` to clear it. Plumb this alongside the
   preview — either a small enum on the existing preview channel
   (`Frame(Handle) | Status(...)`) or a second channel.
3. In `overlay.rs`, carry the status into `Overlay` state and render a hint in
   the chrome (next to the toolbar, outside the crop per R3) — e.g. *"scrolling
   too fast — scroll back to the captured edge to continue"* — that clears once
   stitching re-syncs.

This works in debug *and* release and matches the reference UX.

Alternative / complementary — **core auto re-anchor (separate, sanctioned
`rollshot-core` change):** after N consecutive `NoMatch`, re-seed the anchor
from a recent frame so stitching resumes without user action. More automatic but
leaves a gap in the output (skipped content) and is a stitching-quality decision;
should get its own spec. The overlay hint is the lower-risk first step and does
not preclude this.

Already shipped as partial mitigation on `feat/native-linux-capture-overlay`
(reduce how often the gap occurs, not recovery):

- `60f12d8` bounded grow-then-follow preview (so updates are at least visible).
- `778f274` harness defaults to fps=30 + a debug-build hint to run `--release`.

## Open Questions

- Hint wording + language: match the overlay's current English toolbar
  (*"Capturing — scroll the target, Esc to finish"*), Chinese, or bilingual?
- Trigger sensitivity: warn after 2 consecutive misses (snappier, may blip on a
  one-frame miss) or 3 (steadier)? Debounce like snow-shot's leading-edge
  throttle?
- Should this and `2026-05-23-fast-scroll-capture-sampling-gap.md` be unified
  under one "fast-scroll robustness" track, or kept as capture-layer vs
  overlay-layer mitigations?
- Is the user-driven scroll-back hint sufficient, or do we also want the core
  auto re-anchor? (They are complementary, not mutually exclusive.)
