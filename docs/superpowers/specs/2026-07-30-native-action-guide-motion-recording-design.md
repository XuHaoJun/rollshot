# Native Action Guide Motion Recording Design

**Date:** 2026-07-30
**Status:** Approved; awaiting implementation plan
**Area:** Action Guide, capture, project assets, video export
**Branch:** `feat/native-action-guide-motion-recording`
**Motivating idea:**
[`docs/ideas/2026-07-22-agent-skills-action-guide-launch-video.md`](../../ideas/2026-07-22-agent-skills-action-guide-launch-video.md)

## 1. Decision and prerequisite status

Rollshot will first add native, silent motion recording to Action Guide and let
users export the raw MP4. This is the first source-material slice for the
launch-video idea. It deliberately does not implement teaser planning or
rendering.

The user approved completion of the Action Guide Agent Foundation migration on
2026-07-30 after Gate A1 and Gate B1 were verified. The migration umbrella is
therefore a historical snapshot. The trustworthy-skills prerequisite for
restarting launch-video discovery is satisfied, but it does not authorize a
renderer without a separate design.

The selected implementation direction is a shared asynchronous FFmpeg encoder
fed from the existing cropped Action Guide frame stream. A disposable
performance spike is a hard gate before production implementation. A failed
spike stops this design's implementation and requires a separate
platform-native encoder design.

## 2. Product thesis

A reviewed Action Guide should retain the real interaction evidence needed to
produce a polished launch teaser later. Today native recording retains bounded
keyframes, and the existing MP4 exporter builds a still-frame workflow summary.
It does not preserve motion between steps.

This slice gives that motion evidence an immediate, understandable use: the
user may explicitly retain a silent screen recording with the guide and export
that recording as an MP4. It does not turn Rollshot into a general recorder or
video editor.

## 3. Goals

1. Allow users to opt in before a native Action Guide recording to retain a
   silent video of the same capture region.
2. Use one platform-neutral recording contract and encoder path for the active
   Linux and macOS Action Guide products.
3. Keep the existing Action Guide detector, retained keyframes, semantic input,
   controls, and output unchanged when motion recording is disabled.
4. Prevent encoder work or failure from blocking or destroying the Action Guide.
5. Persist a validated, project-owned motion asset on the same session-relative
   timeline as guide steps.
6. Let users atomically export the project-owned recording through **Save
   recording…**.
7. Establish a motion-asset contract that a later imported-video slice and a
   later launch-teaser slice can consume without changing its shape.

## 4. Non-goals

- Agent shot selection, creative direction, or repository inspection.
- `LaunchTeaserPlan`, teaser review UI, teaser rendering, captions,
  transitions, pan/zoom, music, or audio mixing.
- Imported-video retention or migration in this slice.
- System audio, microphone capture, voiceover, or audio permissions.
- A standalone recorder mode, recording library, playback studio, timeline
  editor, or media browser.
- Platform-native encoders unless the shared-encoder spike fails.
- A Rollshot-defined duration or file-size limit.
- Remote rendering, cloud storage, publishing, or Hyperframes integration.

## 5. Current-state constraints

- `rollshot-iced-overlay::Driver` already crops the active capture frame and
  sends it to `rollshot_action::ActionRecorder` on a dedicated action thread.
- `ActionRecorder` owns bounded detection and retained-frame behavior; it must
  remain non-blocking.
- `rollshot-action` already uses `ffmpeg-sidecar` for MP4 export and video
  import tooling.
- The current project schema is version 2. It persists reviewed frames and
  steps but no continuous-motion asset.
- Imported-video completion intentionally clears the external source path and
  preserves extracted project frames only.
- The existing summary MP4 is explicitly a reviewed-keyframe workflow summary,
  not raw screen recording.
- Linux and macOS use different product launch/control paths, but both active
  Action Guide recording paths pass through the iced overlay driver. The shared
  frame stream is therefore the preferred integration boundary.

## 6. User experience

### 6.1 Recording preflight

After **Record new**, Rollshot presents a compact preflight choice. The new
option is off by default:

> **Keep a silent screen recording**
> Saves the complete motion inside the Action Guide capture region with the
> project. No system audio or microphone.

The copy also states that Rollshot imposes no duration or file-size limit, so
recording continues to use disk until the user stops or the encoder/filesystem
can no longer write.

When the option is disabled, Rollshot does not resolve FFmpeg, start an encoder,
allocate a motion-frame copy, create a recording temp file, or change current
capture behavior.

When the option is enabled, Rollshot resolves the existing managed FFmpeg
before capture starts. If FFmpeg cannot be resolved or launched, the user may
retry/setup or explicitly continue with Guide-only recording. Rollshot must not
show a motion-recording state before the encoder is ready.

### 6.2 Recording state

The existing stop, Esc, and scroll behavior remains unchanged. A persistent
**Motion recording on** indicator appears in the active platform control
surface:

- Linux: the iced layer-shell Action Guide overlay.
- macOS: the Action Guide recording tray/control path.

If motion encoding fails during capture, the indicator changes to a failed
state. Action Guide capture continues.

### 6.3 Completion and export

On success, the Action Guide workspace shows motion metadata and a secondary
**Save recording…** action. It does not add playback or editing controls.

On encoder failure, the workspace states that the Guide was created but the
screen recording could not be saved. The message uses a stable, actionable
failure category without exposing internal paths or process output.

**Save recording…** atomically copies the validated project asset to a
user-selected MP4 path. Exporting the raw recording does not mark the Action
Guide project saved and does not clear its dirty state.

## 7. Architecture

```text
Overlay captured frame
└── crop to Action Guide region
    ├── existing ActionRecorder
    │   └── Guide + retained keyframes
    └── opt-in only: one encoder-owned copy
        └── bounded latest-frame queue
            └── shared FFmpeg motion worker
                └── session temp MP4
                    └── validated project motion asset
                        └── Save recording… atomic export
```

### 7.1 `rollshot-action`

`rollshot-action` owns the platform-neutral motion-recording contract and FFmpeg
worker because it already owns Action Guide recording, project persistence, and
video tooling. The implementation reuses the current `ffmpeg-sidecar` process
wrapper and toolchain resolution patterns rather than adding another FFmpeg
abstraction.

The motion worker:

- receives owned frame data plus session-relative `at_ms` values;
- accepts frames through a bounded, non-blocking latest-frame queue;
- normalizes irregular capture cadence to constant-frame-rate output;
- writes to a session temp path;
- finalizes FFmpeg, probes the result, computes the digest, and returns validated
  metadata;
- removes incomplete temp output on failed finalization.

The in-memory queue is bounded even though output duration and file size are
not. Unlimited recording means no product-defined stop threshold, not unbounded
RAM.

### 7.2 `rollshot-iced-overlay`

The existing action thread gains an optional motion sink. It crops each source
frame once for the Action Guide region, keeps the existing `ActionRecorder`
feed, and offers one encoder-owned pixel copy only when motion recording is
enabled.

Offering a frame must never wait for FFmpeg. A full queue replaces or drops
motion work according to the latest-frame policy. Detector input and retained
keyframes take priority.

This design does not refactor `FrameStore` or all capture frames to shared
ownership. The optional frame copy is accepted only if the spike proves its
cost safe. A broader ownership refactor would add contract surgery before there
is evidence it is needed.

### 7.3 `rollshot-app`

The app owns:

- the preflight opt-in and FFmpeg readiness flow;
- propagation of the motion-recording launch option on Linux and macOS;
- platform-specific recording indicators with common semantics;
- workspace success/failure state;
- project save/discard integration; and
- the **Save recording…** file picker and atomic export command.

The launch option is explicit and session-scoped. It is not a hidden global
preference and is not inferred from previous sessions.

### 7.4 Project persistence

The project schema advances from version 2 to a new version with an optional
motion asset. Version 1 and version 2 projects continue to load with no motion
asset. The migration is additive from the user's perspective; saving an older
project writes the current schema through the existing upgrade path.

The persisted metadata is equivalent to:

```text
MotionAsset
├── relative_path
├── sha256
├── duration_ms
├── width
├── height
├── fps_numerator
├── fps_denominator
├── codec = h264
└── audio = none
```

The exact Rust type names are implementation-plan details. The invariants are
not:

- `relative_path` is project-relative, validated, and generated by Rollshot;
- no external source path or user-selected export path enters the manifest;
- `duration_ms` uses the same zero point as guide step timestamps;
- width and height describe the encoded display dimensions;
- the codec and audio declarations are closed enums, not arbitrary strings;
- load and export verify file presence, digest, and probed media metadata;
- mismatch fails closed and never silently substitutes another asset.

A newly finished recording first exists as a session-owned temp asset. It marks
the workspace dirty. Saving the project promotes it through the existing
project commit lifecycle. Closing an unsaved workspace uses the existing
save/discard decision; only explicit discard deletes the temp asset.

## 8. Timing and backpressure

The initial target is H.264, silent, constant 30 fps. Frames retain their
existing session-relative millisecond timestamps.

The encoder maps offered frames onto 30 fps ticks:

- a newer frame replaces the prior visual state at its timestamp;
- missing ticks repeat the last accepted frame;
- frames that arrive faster than the next output tick may be dropped;
- queue saturation may reduce motion fidelity but must not shorten or accelerate
  the recording timeline;
- final duration differs from the captured session duration by no more than one
  output frame.

The design accepts short visual holds under encoder pressure. It rejects
blocking capture, changing guide timing, or allowing an unbounded queue.

## 9. Failure and recovery

| Failure | Required result |
|---|---|
| FFmpeg unavailable before start | Do not start motion capture; offer setup/retry or explicit Guide-only continuation |
| Encoder spawn failure | Same as unavailable; never show a false recording indicator |
| Queue saturation | Drop/replace motion frames; never block ActionRecorder |
| FFmpeg pipe/process failure | Stop motion recording, change indicator to failed, continue Guide |
| Filesystem write failure, including full disk | Stop motion recording, continue Guide, report stable category |
| Finalize/probe/digest failure | Remove incomplete temp MP4 and preserve Guide |
| Project save failure | Keep the temp asset while the workspace remains open so the user can retry |
| Explicit discard | Delete the session temp asset |
| Raw export failure | Preserve the project-owned asset and allow retry to another path |
| Missing/corrupt asset after reopen | Fail closed; keep the Guide usable and disable export for that asset |

A partial MP4 is never promoted, exported, or offered to later teaser work.

## 10. Privacy and authority

The opt-in is required because a continuous recording retains materially more
captured content than reviewed keyframes. Consent applies only to the selected
Action Guide capture region and the current session.

The feature adds no audio permission, repository access, network access, model
disclosure, or agent authority. Motion recording is not an agent tool in this
slice.

Runtime diagnostics use stable `rollshot::*` targets and structured fields.
They may contain dimensions, duration, frame/drop counts, and stable failure
categories. They must not contain pixels, project paths, temp paths, export
paths, user filenames, FFmpeg command lines containing paths, or captured text.

## 11. Feasibility spike

Before production implementation, create an isolated
`spikes/action-guide-live-ffmpeg/` experiment. It does not join the workspace and
production crates do not depend on it.

### 11.1 Decision

Can Rollshot's managed FFmpeg encode the existing cropped RGBA Action Guide
stream at 1920×1080 and 30 fps without blocking capture or growing Rollshot
memory without bound?

### 11.2 Workload

Use a representative desktop-change recording rather than flat synthetic
frames. Run for ten minutes to expose sustained queue and memory behavior. Use
the same `ffmpeg-sidecar` version, raw RGBA pipe, H.264 options, queue policy,
and timestamp normalization proposed for production.

### 11.3 Hard gates

- producer p99 frame-offer latency is at most 1 ms;
- the encoder sustains real-time output without persistent queue saturation;
- output duration differs from the source timeline by at most one frame;
- Rollshot-side memory remains bounded after warm-up;
- stop/finalize leaves one valid MP4 on success and no partial output on failure;
- the resulting file probes as H.264, 30 fps, silent, and the expected dimensions.

Linux requires runtime/hardware evidence on the current AMD workstation. macOS
requires runtime/hardware evidence in the actual ScreenCaptureKit product
environment. Compilation on either platform is not runtime evidence.

### 11.4 Decision rule

All hard gates pass: proceed with the shared encoder. Any fatal gate fails:
record a NO-GO finding, stop production implementation, and open a separate
design for platform-native encoding. Do not silently relax the gates or mix a
platform-specific fallback into this design.

## 12. Verification

### 12.1 Motion contract tests

- timestamp-to-CFR mapping for empty, one-frame, irregular, duplicate, late, and
  over-rate input;
- bounded latest-frame behavior and deterministic duration;
- a stalled encoder sink cannot block the ActionRecorder producer;
- Guide candidates and retained keyframes match the no-motion path;
- spawn, broken-pipe, write, cancellation, finalize, probe, and digest failures;
- atomic temp-to-asset promotion and incomplete-file cleanup.

### 12.2 Project compatibility tests

- current version 1 and version 2 fixtures still load;
- new motion metadata round-trips through save/reopen;
- project save promotes the temp asset and records its digest;
- project-relative path traversal, missing file, digest mismatch, codec mismatch,
  audio mismatch, and probe mismatch fail closed;
- explicit discard removes only the session temp asset;
- failed project save retains retryable temp state while the workspace is open.

### 12.3 Export tests

- raw export is byte-identical to the validated project asset;
- export writes a temp sibling and atomically renames it only after the copy
  succeeds; the file picker owns overwrite confirmation, and failure preserves
  any prior destination;
- failed export leaves the project asset unchanged;
- `ffprobe` confirms H.264, silent audio state, dimensions, frame rate, and
  duration.

### 12.4 Product integration tests

Both active platform paths must cover:

1. opt out and record a Guide with no FFmpeg work;
2. opt in, resolve FFmpeg, record, stop, and see success metadata;
3. save the project, reopen it, export the MP4, and verify the digest;
4. fail the encoder during recording and confirm the Guide remains saveable;
5. close an unsaved successful recording and exercise save versus discard;
6. confirm the recording indicator never claims success after failure.

Linux requires an iced layer-shell hardware smoke run. macOS requires a real
ScreenCaptureKit product-path smoke run. Without both, the implementation must
state the unchecked platform risk and may not claim cross-platform completion.

Because this changes user-visible iced UI, implementation must load `iced-rs`
and `testing-iced-ui` before editing. Structural scenarios cover the preflight,
active indicator, success state, and failed state at default and minimum window
sizes. Raw visual evidence and any baseline decision follow the independent
clean-context reviewer workflow.

### 12.5 Repository checks

Run focused suites for `rollshot-action`, `rollshot-iced-overlay`, and
`rollshot-app --features action-guide`, then workspace formatting and clippy.
Tests must defend observable contracts rather than source text or incidental
implementation choices.

## 13. Rollout and compatibility

The opt-in defaults off. Existing users and projects retain current behavior.
The new schema is written only by the upgraded application and remains readable
through Rollshot's normal forward-only project migration policy.

No telemetry or remote rollout mechanism is added. The spike is retained as
historical evidence after its decision is consumed and is never imported by
production code.

## 14. Follow-on sequence

After this slice ships and both platforms prove the motion-asset contract:

1. add imported-video retention by copying an authorized source into the same
   project-owned asset contract;
2. run launch-teaser product validation using reviewed Action Guide timestamps
   plus retained motion;
3. design the bounded `LaunchTeaserPlan`, review surface, and deterministic
   renderer as a separate workflow;
4. keep optional repository-read authority separate unless product evidence
   shows it materially improves claims or shot selection.

Passing this design does not approve any of those follow-on slices.

## 15. Approved decisions

- Native and imported motion sources remain separate implementation slices.
- Native recording comes first.
- The recording is silent.
- Consent is an explicit, default-off preflight opt-in.
- The first standalone output is raw MP4 export.
- Encoder failure preserves the Action Guide and reports recording failure.
- Rollshot sets no duration or file-size limit; memory and queue bounds remain
  mandatory.
- Shared asynchronous FFmpeg is preferred, conditional on the spike.
- Platform-native encoding is a separate fallback design, not prebuilt scope.
