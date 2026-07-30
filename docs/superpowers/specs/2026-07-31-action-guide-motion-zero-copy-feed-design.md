# Action Guide Motion Recording — Zero-Copy Frame Feed Design

**Date:** 2026-07-31
**Status:** Approved; awaiting Gate 0 spike re-run
**Area:** Action Guide, capture, overlay frame flow
**Branch:** `feat/native-action-guide-motion-recording`
**Supersedes:** the producer-side frame-handoff portion of
[`2026-07-30-native-action-guide-motion-recording-design.md`](2026-07-30-native-action-guide-motion-recording-design.md)
**Trigger:** NO-GO finding in
[`spikes/action-guide-live-ffmpeg/FINDINGS.md`](../../../spikes/action-guide-live-ffmpeg/FINDINGS.md)

## 1. Decision and relationship to the prior design

The 2026-07-30 native motion recording design made production implementation
conditional on a feasibility spike. That spike ran on the Linux AMD workstation
and returned **NO-GO**: producer p99 clone+offer latency was 1557 µs against
the 1000 µs hard gate. Every other hard gate passed — the managed-FFmpeg H.264
encoder sustained 30 fps for ten minutes with zero dropped frames, zero
saturated queue windows, bounded memory, exact timeline fidelity, a valid
probe, and atomic cleanup.

The failed cost is the full-frame buffer clone on the producer thread, not the
encoder. This design therefore changes exactly one decision from the approved
spec: the producer-side frame handoff becomes shared ownership instead of an
encoder-owned copy. Everything else in the 2026-07-30 spec is unchanged and
remains authoritative:

- product thesis, goals, and non-goals (§2–§4);
- preflight opt-in, recording indicator, and completion UX (§6);
- `MotionAsset` schema, persistence, and export invariants (§7.4);
- timing and backpressure rules (§8);
- failure and recovery matrix (§9);
- privacy and authority constraints (§10);
- verification requirements (§12);
- rollout policy (§13);
- follow-on sequence (§14).

Where this document and the 2026-07-30 spec conflict on the frame handoff
(§7.1 queue payload, §7.2 copy policy), this document wins. The 2026-07-30
spec is not edited retroactively; it remains the snapshot of what was approved
then.

## 2. Root cause and selected direction

The spike producer cloned an 8 MiB RGBA buffer on every offer. That clone
alone costs ~1.3 ms at p50 — structurally above the 1 ms p99 gate, independent
of encoder speed.

The 2026-07-30 spec (§7.2) deferred a shared-ownership change "before there
is evidence it is needed." The NO-GO spike is that evidence. The user approved
the zero-copy direction on 2026-07-31 over two alternatives:

- **Platform-native encoders (VA-API / VideoToolbox):** rejected. Two encoder
  backends to build and verify, and a frame copy or GPU upload still occurs
  somewhere — the producer-side cost this design must remove does not
  disappear by moving the encoder to the GPU.
- **Hybrid native encoders + zero-copy feed:** rejected. Maximum complexity
  for headroom the measurements do not demand; the shared encoder already met
  every throughput gate.

The selected direction keeps the proven managed-FFmpeg/x264 encoder and
removes the per-offer buffer clone via `Arc` shared ownership at the action
thread boundary.

## 3. Goals

1. Make the motion-frame offer cost negligible (pointer clone plus refcount)
   so the producer p99 ≤ 1 ms gate is achievable by construction.
2. Keep the managed-FFmpeg encoder path, queue policy, timestamp
   normalization, and failure semantics from the 2026-07-30 spec unchanged.
3. Confine ownership changes to the Action Guide action-thread boundary; no
   changes to `FrameStore`, capture backends, or the normal screenshot path.
4. Re-validate with the retained spike harness against the identical hard
   gates on both platforms before any production implementation.

## 4. Non-goals

- VA-API, VideoToolbox, Vulkan video, or any GPU-side encoder.
- A DMA-BUF capture path or PipeWire zero-copy capture.
- A `FrameStore`-wide ownership refactor.
- Relaxing the 1 ms producer p99 gate or any other hard gate.
- Any change to detector behavior, retained keyframes, semantic input,
  controls, or output when motion recording is disabled.

## 5. Frame flow

Current (approved spec, encoder-owned copy):

```text
cropped frame (owned)
├── ActionRecorder::ingest_frame (moved)
└── encoder offer: full 8 MiB buffer clone  ← failed gate
```

New (this design, shared ownership):

```text
crop once into Arc<RgbaImage>
├── ActionRecorder::ingest_frame(Arc clone)
└── bounded latest-frame queue offer (Arc clone)
    └── FFmpeg worker thread
        ├── pixel-format / stride conversion
        └── pipe write
```

Rules:

- The crop itself is unchanged: one owned buffer produced per frame, exactly
  as today. It is wrapped in `Arc` once, at the action-thread boundary.
- `ActionRecorder::ingest_frame` accepts the shared frame. Its retained
  keyframe behavior is unchanged; it already bounds retention, so shared
  ownership does not extend pixel lifetimes beyond existing keyframe
  lifetimes plus any in-flight encoder references.
- The bounded latest-frame queue holds `Arc` frames. Capacity, replace-oldest
  policy, and saturation semantics are unchanged from the spike and the
  approved spec.
- All CPU-heavy encoder work — RGBA conversion, stride handling, and pipe
  writes — stays on the FFmpeg worker thread, never on the action thread.
- Offering a frame still never waits for FFmpeg. Detector input and retained
  keyframes keep priority.
- When motion recording is disabled, the frame is still cropped and wrapped
  once for the recorder (a single pointer-sized allocation); no queue, worker,
  or encoder is engaged and capture behavior is otherwise identical to today.

## 6. Crate responsibilities

Unchanged from the 2026-07-30 spec §7, with these deltas:

- `rollshot-action`: `ActionRecorder::ingest_frame` takes the shared frame
  type; the motion worker's queue payload is the shared frame type instead of
  an owned pixel buffer. The worker, finalization, probe, digest, and temp
  asset lifecycle are unchanged.
- `rollshot-iced-overlay`: the action thread wraps the cropped frame in `Arc`
  once and hands clones to the recorder and, when enabled, the motion sink.
- `rollshot-app`: unchanged — preflight, launch option, indicators, workspace
  state, project save/discard integration, and atomic export.

## 7. Memory bound argument

Shared ownership must not reintroduce unbounded memory through the back door:

- The queue is bounded; held `Arc` references are bounded by queue capacity
  plus one in-worker frame.
- `ActionRecorder` retention stays bounded by its existing `StoreConfig`.
- A stuck encoder worker holds at most queue-capacity frames; finalization
  and failure paths drop them.
- Worst-case added residency over the no-motion path is therefore
  `(queue_capacity + 1) × frame_bytes`, identical to the approved design's
  bounded queue — the spike measured 2 slots × 8 MiB.

## 8. Gate 0 spike re-run

The retained harness at `spikes/action-guide-live-ffmpeg/` is modified in
place: the producer offer path changes from buffer clone to `Arc` clone. All
other parameters — workload, resolution, fps, duration, queue capacity,
FFmpeg options, gate thresholds, and the 60 s memory warm-up — stay identical
so results are comparable to the NO-GO run.

### 8.1 Hard gates (unchanged)

- producer p99 frame-offer latency ≤ 1 ms;
- encoder sustains real-time output without persistent queue saturation;
- output duration differs from the source timeline by at most one frame;
- Rollshot-side memory remains bounded after warm-up;
- stop/finalize leaves one valid MP4 on success and no partial output on
  failure;
- the resulting file probes as H.264, 30 fps, silent, expected dimensions.

### 8.2 Platform evidence (unchanged)

- Linux: runtime/hardware evidence on the current AMD workstation.
- macOS: runtime/hardware evidence in the actual ScreenCaptureKit product
  environment. The macOS run was never dispatched in the first spike; it is
  required here before production implementation.

### 8.3 Decision rule (unchanged)

All hard gates pass on both platforms: proceed to the production
implementation plan under the 2026-07-30 spec as amended here. Any fatal gate
fails: record a NO-GO finding, stop, and escalate to the platform-native
encoder design — the same fallback the original decision tree named. Gates
are not silently relaxed.

## 9. Verification deltas

The 2026-07-30 verification sections §12.1–§12.5 carry over wholesale. This
design adds:

- contract tests that the recorder feed and the encoder feed observe the same
  frame sequence from one shared buffer (same count, same order, same
  timestamps);
- a test that disabling motion recording leaves the recorder feed
  byte-identical to the pre-change path;
- a test that a stalled encoder worker holds at most queue-capacity shared
  frames and drops them on finalization.

## 10. Approved decisions

- The zero-copy ownership direction replaces the encoder-owned copy; the
  managed-FFmpeg encoder is retained.
- The ownership change is confined to the action-thread boundary.
- The spike is re-run with identical gates on both platforms as Gate 0.
- The 2026-07-30 spec remains authoritative for everything except the frame
  handoff.
- Platform-native encoding remains the named fallback if this design's spike
  fails; it is not prebuilt scope.
