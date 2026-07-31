# Native Action Guide Motion Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an explicit, default-off Action Guide option that retains the native capture region as a validated silent H.264 MP4, persists it with the project, and atomically exports it without changing Guide-only recording.

**Architecture:** Port the proven bounded latest-frame/CFR/managed-FFmpeg path into `rollshot-action`, but use the approved zero-copy `Arc<RgbaImage>` handoff at the Action Guide action-thread boundary. Treat a completed recording as an opaque validated asset: the project store owns schema migration and promotion, while `rollshot-app` owns preflight, platform launch/control state, workspace state, discard, and raw export.

**Tech Stack:** Rust, `Arc<RgbaImage>`, `crossbeam-channel`, `ffmpeg-sidecar`, managed FFmpeg/ffprobe, SHA-256, serde, iced 0.14, `iced_test::Simulator`, Linux iced layer-shell, macOS winit/tray-icon/ScreenCaptureKit.

## Global Constraints

- The 2026-07-30 native-motion design remains authoritative except for frame handoff; the 2026-07-31 zero-copy design supersedes its producer-side copy policy.
- Motion recording is explicit, session-scoped, and off by default. Disabled means no FFmpeg/ffprobe resolution, encoder process, motion queue, motion temp file, or second pixel buffer.
- Encode silent H.264 at constant 30/1 fps. No microphone, system audio, duration limit, file-size limit, playback, editing, teaser planning, or imported-video retention.
- Queue capacity is 2. Offers use `try_send`, evict the oldest queued frame on saturation, and never wait for FFmpeg.
- The action thread crops once, wraps the resulting `RgbaImage` in `Arc` once, and shares pointer clones with `ActionRecorder` and the optional motion worker. Do not refactor `FrameStore`, capture backends, or screenshot ownership beyond this boundary.
- Missing CFR ticks repeat the latest accepted image; over-rate/duplicate/late frames update the latest visual state without accelerating or shortening the session timeline. Final encoded duration differs from the supplied session duration by at most one 30 fps frame (34 ms).
- Encoder failure never fails or destroys the Guide. Partial MP4s are deleted and never promoted/exported.
- Runtime diagnostics use stable `rollshot::*` targets and structured dimensions/counts/categories only. Never log pixels, captured text, project/temp/export paths, user filenames, or FFmpeg command lines containing paths.
- Project schema advances from 2 to 3. Versions 1 and 2 load with no motion asset; every save writes schema 3. No aliases or deprecated schema-2 current-model path remain.
- The only manifest motion path is the Rollshot-generated `assets/motion/recording.mp4`. External source paths and user export paths never enter `project.json`.
- Project load/export fail the motion asset closed on missing file, digest mismatch, codec/audio/fps/dimension/duration mismatch, while preserving Guide usability.
- Raw export does not change project dirty state and uses a temp sibling plus atomic rename after full copy and validation.
- User explicitly overrode the approved macOS Gate 0 prerequisite on 2026-07-31. Linux zero-copy evidence is GO (`p99 = 17 µs`); macOS runtime evidence remains UNTESTED. Implementation may proceed, but no cross-platform-complete or macOS-runtime-verified claim is permitted until the identical macOS 10-minute run and product smoke pass.
- UI test mode is auto. Visual capability is semantic via native `read`; probe `crates/rollshot-app/tests/eval/fixtures/url_bar/image.png` passed. Pixel diff is currently none; CI is artifact-only. Product-changing agents must not approve golden baselines.
- Branch: `feat/native-action-guide-motion-recording`. Prefix every shell command with `rtk`.

---

### Task 1: Shared Action Guide frame ownership

**Files:**
- Modify: `crates/rollshot-action/src/frame_store.rs`
- Modify: `crates/rollshot-action/src/recorder.rs`
- Modify: `crates/rollshot-action/src/input.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Modify: recorder call sites reported by `lsp references` for `ActionRecorder::ingest_frame`

**Interfaces:**
- Produces: `pub type SharedActionFrame = Arc<RgbaImage>`.
- Produces: `ActionRecorder::ingest_frame(&mut self, image: SharedActionFrame, at_ms: Millis)` and `FrameStore::ingest(&mut self, image: SharedActionFrame, at_ms: Millis) -> FrameId`.
- Preserves: detector results, retained-frame bytes/count/order, and bounded `FrameStore` behavior.

- [ ] **Step 1: Find every exported-symbol caller before changing the signature**

Use `xd://lsp` references for `ActionRecorder::ingest_frame` and `FrameStore::ingest`. Record all production and test call sites in the task report; do not rely on text search for migration completeness.

- [ ] **Step 2: Write RED ownership/behavior tests**

Add a `frame_store.rs` test that keeps a `Weak<RgbaImage>`, ingests `Arc::clone(&frame)`, and proves the ring/retained path holds shared ownership without creating a different pixel allocation. Extend `recorder.rs` with a helper that runs identical frames through the old-value fixture shape and the new shared path, then asserts candidate IDs, timestamps, keyframes, nearby IDs, retained bytes, and `dropped_analysis` match.

```rust
let frame = Arc::new(quadrant(32, 32));
let weak = Arc::downgrade(&frame);
recorder.ingest_frame(Arc::clone(&frame), 100);
drop(frame);
assert!(weak.upgrade().is_some());
```

- [ ] **Step 3: Run the focused RED tests**

Run: `rtk cargo test -p rollshot-action frame_store::tests -- --nocapture`

Run: `rtk cargo test -p rollshot-action recorder::tests -- --nocapture`

Expected: compile failure because both APIs still consume `RgbaImage`.

- [ ] **Step 4: Implement the shared frame cutover**

Define `SharedActionFrame` in `frame_store.rs`, re-export it from `lib.rs`, make `FrameStore::ingest` accept it, compute luma through `image.as_ref()`, and store the same `Arc` directly in `RingFrame`. Make `ActionRecorder::ingest_frame` accept and forward the shared frame. At every non-overlay fixture/caller, wrap each newly-created image exactly once with `Arc::new`; never add a compatibility overload.

- [ ] **Step 5: Verify the shared-frame contract**

Run: `rtk cargo test -p rollshot-action frame_store::tests recorder::tests input::tests`

Expected: PASS; existing candidate and retained-keyframe assertions remain unchanged.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/frame_store.rs crates/rollshot-action/src/recorder.rs crates/rollshot-action/src/input.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "refactor(action-guide): share recorded frame ownership"
```

---

### Task 2: Bounded mailbox and CFR contract

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-action/Cargo.toml`
- Create: `crates/rollshot-action/src/motion/mod.rs`
- Create: `crates/rollshot-action/src/motion/queue.rs`
- Create: `crates/rollshot-action/src/motion/timing.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

**Interfaces:**
- Consumes: `SharedActionFrame` from Task 1.
- Produces: `MotionFrame { pub at_ms: Millis, pub image: SharedActionFrame }`.
- Produces: `MotionFrameSender::offer(&self, MotionFrame) -> MotionOfferResult`, `MotionFrameReceiver::recv()`, and `motion_frame_mailbox(capacity)`.
- Produces: `CfrScheduler::push(at_ms) -> CfrEmission`, `CfrScheduler::finish(duration_ms) -> u64`, `frames_written()`, and `duration_ms()`.

- [ ] **Step 1: Write RED latest-frame mailbox tests**

Port the retained spike cases into `motion/queue.rs`: capacity 2 queues two frames; the third returns `ReplacedOldest`; receiver order is timestamps 2 then 3; disconnected returns `Disconnected`; a receiver deliberately stalled for 100 ms cannot make 10,000 producer offers block. Assert the producer loop completes under a 1-second test deadline rather than asserting implementation internals.

```rust
assert_eq!(sender.offer(frame(1)), MotionOfferResult::Queued);
assert_eq!(sender.offer(frame(2)), MotionOfferResult::Queued);
assert_eq!(sender.offer(frame(3)), MotionOfferResult::ReplacedOldest);
assert_eq!(receiver.recv().unwrap().at_ms, 2);
assert_eq!(receiver.recv().unwrap().at_ms, 3);
```

- [ ] **Step 2: Write RED timestamp-to-CFR tests**

Cover empty finish, one frame at zero, irregular timestamps, duplicate timestamps, a late timestamp behind the written cursor, over-rate input, and final holds. `CfrEmission` must distinguish `repeat_previous` from `write_new` so a frame arriving at 100 ms does not repaint ticks 1 and 2 retroactively.

```rust
assert_eq!(scheduler.push(0), CfrEmission { repeat_previous: 0, write_new: true });
assert_eq!(scheduler.push(100), CfrEmission { repeat_previous: 2, write_new: true });
assert_eq!(scheduler.finish(134), 1);
assert!(scheduler.duration_ms().abs_diff(134) <= 34);
```

- [ ] **Step 3: Run the RED motion contract tests**

Run: `rtk cargo test -p rollshot-action motion::queue::tests motion::timing::tests`

Expected: compile failure because the motion module does not exist.

- [ ] **Step 4: Implement the minimal proven queue**

Add `crossbeam-channel` at the workspace and crate levels. Port the spike's bounded sender/receiver, retaining a receiver clone only for `try_recv` eviction. The sender must use only `try_send`/`try_recv`; no mutex, condition variable, blocking send, or unbounded channel is allowed.

- [ ] **Step 5: Implement integer-only CFR mapping**

Use `u128` for `at_ms * 30`. `push` emits prior-frame holds for unwritten ticks strictly before the arrival tick and emits the new image once at its tick. Duplicate/late input returns `write_new = false` but replaces the worker's current visual state for future ticks. `finish` derives output length from the explicit session duration, not last-frame time.

- [ ] **Step 6: Verify queue and timing contracts**

Run: `rtk cargo test -p rollshot-action motion::queue::tests motion::timing::tests`

Expected: PASS, deterministic across repeated runs.

- [ ] **Step 7: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-action/Cargo.toml crates/rollshot-action/src/lib.rs crates/rollshot-action/src/motion
rtk git commit -m "feat(action-guide): add bounded motion timing core"
```

---

### Task 3: Managed FFmpeg motion worker and validated session asset

**Files:**
- Create: `crates/rollshot-action/src/motion/error.rs`
- Create: `crates/rollshot-action/src/motion/probe.rs`
- Create: `crates/rollshot-action/src/motion/asset.rs`
- Create: `crates/rollshot-action/src/motion/worker.rs`
- Modify: `crates/rollshot-action/src/motion/mod.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

**Interfaces:**
- Reuses: existing public `VideoToolchain { ffmpeg, ffprobe }`; do not create a second toolchain resolver.
- Produces: closed `MotionFailureCategory::{ToolUnavailable, Spawn, BrokenPipe, Write, Filesystem, Finalize, Probe, Digest, Cancelled}` with `as_str()`.
- Produces: closed `MotionCodec::H264`, `MotionAudio::None`, and `MotionMetadata { sha256, duration_ms, width, height, fps_numerator, fps_denominator, codec, audio }`.
- Produces: opaque cloneable `ValidatedMotionAsset` with metadata getters and crate-visible source path; session-owned instances delete their scratch directory after the last clone drops.
- Produces: `MotionRecorder::start(toolchain, width, height) -> Result<Self, MotionFailureCategory>`, `offer(frame)`, `status() -> MotionRuntimeStatus`, `finish(session_duration_ms) -> MotionRecordingOutcome`, and `cancel()`.

- [ ] **Step 1: Write RED process/probe/failure tests**

Use small executable test doubles for spawn failure, immediate broken pipe, non-zero exit, malformed probe JSON, wrong codec, wrong fps, audio stream present, wrong dimensions, and digest failure. Assert only stable categories escape. Assert every failure removes `.part.mp4` and never returns `ValidatedMotionAsset`.

Add parser fixtures for exactly one H.264 video stream, `30/1`, zero audio streams, expected dimensions, and duration within 34 ms. Reject `h264` with audio, `29.97`, rotated/display-size mismatch, a second video stream, or missing duration.

- [ ] **Step 2: Write RED lifecycle tests with an injectable sink**

Introduce an internal `MotionSink` seam used only by worker tests. Verify: offered frame order/timestamps reach the sink; duplicate/over-rate frames do not add ticks; queue saturation still finishes to the supplied duration; a stalled sink changes no Guide producer behavior; cancellation joins/reaps and removes scratch; successful finish yields one validated file.

- [ ] **Step 3: Run the RED worker tests**

Run: `rtk cargo test -p rollshot-action motion::worker::tests motion::probe::tests motion::asset::tests`

Expected: compile failure because worker/probe/asset modules do not exist.

- [ ] **Step 4: Implement synchronous readiness and asynchronous encoding**

`MotionRecorder::start` must validate dimensions are non-zero/even, create a `rollshot/action-motion-*` session scratch directory, spawn FFmpeg synchronously, take stdin, and start stderr draining before returning `MotionRuntimeStatus::On`. Use the production arguments proven by the spike:

```text
-y -f rawvideo -pixel_format rgba -video_size WIDTHxHEIGHT -framerate 30
-i pipe:0 -an -vf format=yuv420p -c:v libx264 -preset veryfast -crf 23
-movflags +faststart -f mp4 recording.part.mp4
```

The worker thread owns FFmpeg stdin/process, the mailbox receiver, CFR scheduler, current `Arc` frame, scratch path, and error transition. The action thread owns only the non-blocking sender and shared status. Drain stderr but discard bytes; never return or log process output.

- [ ] **Step 5: Implement finish, probe, digest, and RAII cleanup**

On finish, send the explicit session duration on a separate control channel, close the frame sender, join the worker, close stdin, wait/reap, probe `.part.mp4`, compute SHA-256, rename it to `recording.mp4` inside the session scratch, and return `MotionRecordingOutcome::Ready(ValidatedMotionAsset)`. On any failure, kill/reap if needed and remove both names. `Drop`/`cancel` must not leave a child or partial file.

- [ ] **Step 6: Run focused tests and a short real-FFmpeg smoke**

Run: `rtk cargo test -p rollshot-action motion::worker::tests motion::probe::tests motion::asset::tests`

Run with managed binaries: `ROLLSHOT_TEST_FFMPEG=1 rtk cargo test -p rollshot-action motion::worker::tests::real_ffmpeg_produces_valid_silent_h264 -- --ignored --nocapture`

Expected: PASS; ffprobe reports H.264, 30/1, no audio, expected dimensions, and duration within 34 ms.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/motion crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action-guide): encode validated motion assets"
```

---

### Task 4: Project schema 3 and fail-closed motion loading

**Files:**
- Modify: `crates/rollshot-action/src/project/model.rs`
- Modify: `crates/rollshot-action/src/project/validate.rs`
- Modify: `crates/rollshot-action/src/project/store.rs`
- Modify: `crates/rollshot-action/src/project/error.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`
- Modify: all `ProjectManifestV2` call sites found by `lsp references`
- Test fixtures: existing schema-1/schema-2 project fixtures under `crates/rollshot-action/src/project/` and `crates/rollshot-app/src/timeline_workspace/`

**Interfaces:**
- Produces current `ProjectManifestV3` and `PROJECT_SCHEMA_VERSION = 3`; removes `ProjectManifestV2` as the current model after migration call sites are updated.
- Produces persisted `MotionAsset` with canonical relative path and the Task 3 metadata fields.
- Produces `MotionAssetLoad::{None, Available(ValidatedMotionAsset), Unavailable(MotionFailureCategory)}` on `LoadedProject`.
- Changes: `load_project(project_root, toolchain: Option<&VideoToolchain>) -> Result<LoadedProject, ProjectError>`.

- [ ] **Step 1: Run LSP references before the schema rename**

Use `xd://lsp` references for `ProjectManifestV2`, `ProjectSnapshot`, `ProjectCommit`, and `load_project`. The migration list must include app timeline fixtures, publish/share tests, and project-store tests.

- [ ] **Step 2: Write RED compatibility and manifest tests**

Add fixtures proving schema 1 and schema 2 load to schema 3 with `motion = None`; schema 3 round-trips all motion fields; unknown codec/audio strings fail serde; path traversal, absolute paths, backslashes, alternate canonical names, zero dimensions/fps/duration, and non-canonical SHA-256 fail structure validation.

The persisted shape is:

```json
{
  "motion": {
    "relative_path": "assets/motion/recording.mp4",
    "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
    "duration_ms": 1000,
    "width": 1920,
    "height": 1080,
    "fps_numerator": 30,
    "fps_denominator": 1,
    "codec": "h264",
    "audio": "none"
  }
}
```

- [ ] **Step 3: Write RED fail-closed load tests**

For a structurally valid schema-3 Guide, cover missing recording, non-regular/symlink asset, digest mismatch, codec mismatch, audio stream, fps mismatch, dimensions mismatch, and duration mismatch. `load_project` must return the Guide with `MotionAssetLoad::Unavailable(category)`, not fail the whole project. Frame corruption remains governed by existing project behavior.

- [ ] **Step 4: Run the RED project tests**

Run: `rtk cargo test -p rollshot-action project::model::tests project::validate::tests project::store::tests`

Expected: compile/behavior failures until schema 3 and load-state separation exist.

- [ ] **Step 5: Implement additive v1/v2 migration and current v3 model**

Deserialize by `schema_version`: v1 → v2 → v3, v2 → v3, v3 directly, other versions reject. Conversion sets `motion: None`. Rename every current-model use to `ProjectManifestV3`; leave v1/v2 structs only as private deserialization migration snapshots where possible.

- [ ] **Step 6: Implement motion validation without sacrificing the Guide**

Structure validation checks only manifest invariants. Asset validation uses no path joins from arbitrary input: require exact `assets/motion/recording.mp4`, open without following symlinks, hash bytes, and call Task 3 probe validation when a toolchain is available. Map asset-level failures into `MotionAssetLoad::Unavailable`; never substitute another file.

- [ ] **Step 7: Verify schema compatibility and load isolation**

Run: `rtk cargo test -p rollshot-action project::model::tests project::validate::tests project::store::tests`

Expected: PASS, including existing version-1/version-2 fixtures.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-action/src/project crates/rollshot-app/src/timeline_workspace
rtk git commit -m "feat(action-guide): persist motion asset metadata"
```

---

### Task 5: Atomic project promotion and raw MP4 export

**Files:**
- Create: `crates/rollshot-action/src/project/motion.rs`
- Modify: `crates/rollshot-action/src/project/model.rs`
- Modify: `crates/rollshot-action/src/project/store.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`
- Modify: `crates/rollshot-action/src/project/error.rs`

**Interfaces:**
- Adds: `ProjectSnapshot.motion: Option<ValidatedMotionAsset>`.
- Adds: `ProjectCommit.motion: Option<ValidatedMotionAsset>` pointing at the committed project-owned asset.
- Produces: `export_motion_asset(asset: &ValidatedMotionAsset, destination: &Path) -> Result<(), ProjectError>`.
- Canonical project destination: `assets/motion/recording.mp4`.

- [ ] **Step 1: Write RED create/save/save-as promotion tests**

Cover first save copying a session asset into the temp project tree before the directory commit; Save As copying a validated project asset into the new project; existing save retaining the same validated asset; digest/metadata recorded from the validated object; copy/probe/manifest/rename failures preserving the session asset for retry. Assert no source/temp/export path appears in JSON.

- [ ] **Step 2: Write RED export atomicity tests**

Verify byte-identical output; destination temp sibling is removed on copy/sync/rename failure; an existing destination remains byte-identical when failure occurs before rename; successful export syncs the file, renames only after complete copy, and leaves project state/source unchanged. Picker overwrite confirmation remains app-owned.

- [ ] **Step 3: Run RED promotion/export tests**

Run: `rtk cargo test -p rollshot-action project::motion::tests project::store::tests::motion`

Expected: compile failure because snapshot/commit/export have no motion contract.

- [ ] **Step 4: Implement canonical materialization**

For new projects, create `assets/motion`, copy from `ValidatedMotionAsset` to a temp sibling, sync, rename to `recording.mp4`, fsync the directory, then write/commit the manifest. For existing same-root assets, revalidate identity instead of copying over themselves. Never delete a session-owned source from the store; ownership changes only when app replaces its workspace state with `ProjectCommit.motion`.

- [ ] **Step 5: Implement atomic raw export**

Create a unique hidden sibling in the destination directory with `create_new`, copy bytes from the already validated asset, `sync_all`, compare copied SHA-256 with metadata, then rename over the picker-approved destination using the existing platform-safe atomic replacement convention. Guard cleanup removes only the export temp file.

- [ ] **Step 6: Verify persistence and export contracts**

Run: `rtk cargo test -p rollshot-action project::motion::tests project::store::tests`

Expected: PASS; failed saves retain the source session asset and failed export preserves project bytes.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/project
rtk git commit -m "feat(action-guide): promote and export motion assets"
```

---

### Task 6: Overlay motion sink and status contract

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs`

**Interfaces:**
- Produces: `ActionGuideRecordingOptions { pub motion_toolchain: Option<VideoToolchain> }`.
- Produces: `ActionGuideCaptureResult { recording, capability, region, motion: MotionRecordingOutcome }`; replaces all tuple returns cleanly.
- Extends: `Driver::begin_action_recording(region, source, options) -> ActionRecordingStart` where `ActionRecordingStart` carries capability and `MotionRuntimeStatus`.
- Extends: `Driver::motion_status()` for tick-driven UI projection.

- [ ] **Step 1: Write RED zero-copy tee tests**

Inject fake recorder and motion sinks at `ActionRecording`. Feed shared frames with timestamps and assert both observe the same allocation (`Arc::ptr_eq`), count, order, timestamps, and bytes. With motion disabled assert the recorder sequence is byte-identical and the motion factory is never called. With a stalled/failing motion sink assert Guide candidates and retained frames match the disabled run.

- [ ] **Step 2: Write RED completion/failure tests**

Cover successful `ActionGuideCaptureResult::motion = Ready`, mid-stream `Failed(BrokenPipe)` while Guide still finalizes, spawn failure producing no `On` status, cancel cleaning the encoder, and queue saturation not changing Guide timing. Verify final duration uses stop time (last frame timestamp plus elapsed time until stop), not merely the last offered frame.

- [ ] **Step 3: Run RED overlay tests**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide driver::tests`

Expected: compile/behavior failures until the new result/options/status contract exists.

- [ ] **Step 4: Implement crop-once/wrap-once tee**

In the action thread, replace `rec.push_frame(image, at_ms)` with:

```rust
let image = Arc::new(image);
rec.push_frame(Arc::clone(&image), at_ms);
if let Some(motion) = rec.motion_mut() {
    let _ = motion.offer(MotionFrame { at_ms, image });
}
```

Do not change `Shared.latest`, reader ownership, crop count, screenshot, or stitch paths. Track the last frame's session timestamp and local observation `Instant` so finish can hold the final image through the actual stop time.

- [ ] **Step 5: Propagate motion outcome and live status on both runners**

Replace Linux static result tuples and macOS `HostEffect::ActionRecorded` fields with `ActionGuideCaptureResult`. Tick reads `Driver::motion_status`; `On` is set only after `MotionRecorder::start` succeeds. A runtime failure becomes `Failed(category)` while recording continues and remains failed through completion.

- [ ] **Step 6: Verify overlay contracts**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide`

Expected: PASS for Linux-compiled shared/runner tests; macOS-only runtime remains explicitly unverified.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src
rtk git commit -m "feat(action-guide): tee shared frames to motion encoder"
```

---

### Task 7: Shared recording preflight and managed-tool readiness

**Files:**
- Modify: `crates/rollshot-app/src/action_guide_home/update.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/view.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/mod.rs`
- Modify: `crates/rollshot-app/src/managed_ffmpeg.rs`
- Modify: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`

**Interfaces:**
- Produces shared `RecordPreflight { keep_motion: bool, phase: RecordPreflightPhase }` in `ActionGuideHome`.
- Changes `Effect::RecordNew` to `Effect::StartRecording { motion_toolchain: Option<VideoToolchain> }`.
- Reuses `resolve_video_import_toolchain` and `download_managed_ffmpeg`; no duplicate downloader/resolver.

- [ ] **Step 1: Write RED home state-machine tests**

Cover: `RecordNew` opens preflight with unchecked motion and no effect; confirm unchecked emits `StartRecording { None }` without invoking resolution; toggling on then confirm emits resolve effect; Available emits start with toolchain; NeedsSetup shows setup/retry and no recording state; setup success re-resolves; explicit Guide-only continuation emits `None`; cancellation returns Home. Assert no previous-session preference is read or stored.

- [ ] **Step 2: Write RED preflight structural tests**

Using `iced_test::Simulator`, at 1100×760 and 640×420 assert visible text and enabled targets:

- `Keep a silent screen recording`
- `Saves the complete motion inside the Action Guide capture region with the project. No system audio or microphone.`
- no duration/file-size limit and disk-use warning
- default unchecked checkbox
- `Start recording`, `Cancel`
- setup failure actions `Retry/setup` and `Continue Guide only`

Exercise checkbox + confirm and confirm-disabled interactions by text/stable widget identity; do not use coordinates.

- [ ] **Step 3: Run RED preflight tests**

Run: `rtk cargo test -p rollshot-app --features action-guide action_guide_home::update::tests action_guide_home::view::tests`

Expected: failures because Record New still launches immediately.

- [ ] **Step 4: Implement pure shared preflight state**

Keep view pure and state mutations in `update`. Disabled confirmation returns immediately without calling any managed-FFmpeg function. Enabled confirmation resolves both FFmpeg and ffprobe in a blocking iced task, because successful recording must later probe the file.

- [ ] **Step 5: Wire both product hosts to the same effect**

Linux maps `StartRecording` to the detached child command in Task 8. macOS maps it to in-process `start_action_guide_recording` in Task 9. Both retain the toolchain only for the current launch; neither writes preferences.

- [ ] **Step 6: Verify preflight behavior**

Run: `rtk cargo test -p rollshot-app --features action-guide action_guide_home`

Expected: PASS, including proof that opt-out performs zero resolver work.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/action_guide_home crates/rollshot-app/src/managed_ffmpeg.rs crates/rollshot-app/src/action_guide_linux_product.rs crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(action-guide): add motion recording preflight"
```

---

### Task 8: Linux launch and active motion indicator

**Files:**
- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/platform_actions.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/recording_tray.rs`

**Interfaces:**
- Adds hidden/session CLI flag: `action-guide --record --keep-motion`; it requires `--record`, defaults false, and is added only by the preflight-confirmed parent.
- Changes: `ActionGuideLaunch::Record { fullscreen, keep_motion }` and `ActionGuideIntent::Record { fullscreen, keep_motion }`.
- Linux child resolves `VideoToolchain` only when `keep_motion` is true and passes `ActionGuideRecordingOptions` to the overlay.

- [ ] **Step 1: Write RED CLI/detached-command tests**

Assert no flag for opt-out, `--keep-motion` only for opt-in, clap rejects it without `--record`, and fullscreen composition is unchanged. Assert the opt-out child path never calls resolver. Resolve failure must exit before the overlay claims motion recording.

- [ ] **Step 2: Write RED Linux indicator tests**

Extend pure `recording_controls` tests for `Disabled`, `On`, and `Failed(category)`. `On` renders persistent `Motion recording on`; failed renders `Screen recording failed; Action Guide is still recording.` and never renders the on copy. For fullscreen SNI, fake tray state updates title/tooltip to equivalent on/failed semantics while Finish remains available.

- [ ] **Step 3: Run RED Linux tests**

Run: `rtk cargo test -p rollshot-app --features action-guide launch::tests platform_actions::tests`

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide app::tests recording_tray::tests`

Expected: failures until CLI propagation and indicators exist.

- [ ] **Step 4: Implement child launch and safe readiness failure**

Parent spawns the same executable with `--keep-motion`; never serialize tool paths. The child re-resolves the already-preflighted managed toolchain immediately before creating the overlay. If that race fails, return a stable setup error and do not enter recording. Opt-out constructs options with `None` without touching the resolver.

- [ ] **Step 5: Implement layer-shell and SNI status projection**

`recording_controls` reads motion status from overlay state on existing 250 ms ticks. Fullscreen tray exposes a mutation method backed by the tray handle; update only title/tooltip/status text, not finish/cancel semantics. Do not add a second subscription.

- [ ] **Step 6: Verify Linux paths**

Run: `rtk cargo test -p rollshot-app --features action-guide launch::tests platform_actions::tests`

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide`

Expected: PASS; opt-out path has no FFmpeg work and failed status cannot regress to on.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/launch.rs crates/rollshot-app/src/platform_actions.rs crates/rollshot-app/src/main.rs crates/rollshot-iced-overlay/src
rtk git commit -m "feat(action-guide): wire Linux motion recording status"
```

---

### Task 9: macOS in-process launch and tray status

**Files:**
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/macos_recording_tray.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs`

**Interfaces:**
- Consumes `VideoToolchain` directly from shared preflight; no CLI/double resolution on macOS.
- `macos_recording_tray::Guard::set_motion_status(MotionRuntimeStatus)` updates persistent tray title/tooltip/menu state.
- `HostEffect::ActionRecorded(ActionGuideCaptureResult)` carries success/failure into the workspace.

- [ ] **Step 1: Write RED macOS host-state tests**

In platform-neutral testable helpers, cover toolchain propagation into `Component::new`, motion-disabled options, tray `On`/`Failed` text mapping, runtime failure remaining failed, and completion carrying the motion outcome into `TimelineWorkspace`. Keep native tray construction behind target cfg; test the state mapper on Linux CI.

- [ ] **Step 2: Run RED macOS contract tests**

Run: `rtk cargo test -p rollshot-app --features action-guide macos_product::tests macos_recording_tray::tests`

Expected: compile/behavior failures until options/result/status are propagated.

- [ ] **Step 3: Implement in-process option/result propagation**

Extend `action_guide_record_config`/`start_action_guide_recording` with `Option<VideoToolchain>`, pass it to the capture component, project live status during the existing capture subscription, and construct the timeline from `ActionGuideCaptureResult`. Dropping/cancelling the component must cancel the encoder and delete session partials.

- [ ] **Step 4: Implement macOS tray semantics**

Keep Finish and Cancel event IDs unchanged. Motion enabled/on adds `Motion recording on`; failure changes to `Screen recording failed — Action Guide continues`. Never show the on text before `MotionRecorder::start` returns success or after a failure.

- [ ] **Step 5: Verify compile-time macOS contracts**

Run: `rtk cargo test -p rollshot-app --features action-guide macos_product::tests macos_recording_tray::tests`

Expected: PASS on Linux for cfg-independent state logic. Record macOS native runtime as UNTESTED until Task 12 hardware evidence.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/macos_product.rs crates/rollshot-app/src/macos_recording_tray.rs crates/rollshot-iced-overlay/src/macos_capture.rs
rtk git commit -m "feat(action-guide): wire macOS motion recording status"
```

---

### Task 10: Workspace motion state, save/discard, and Save recording

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/motion.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Modify: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`

**Interfaces:**
- Produces `WorkspaceMotion::{None, Ready(ValidatedMotionAsset), Failed(MotionFailureCategory), Unavailable(MotionFailureCategory)}`.
- `TimelineWorkspace::new` consumes `MotionRecordingOutcome`; `from_loaded_project` consumes `MotionAssetLoad`.
- Adds messages/effects for `SaveRecordingRequested`, picker result, worker result, and late-result operation IDs.
- Project snapshot includes Ready motion; successful `ProjectCommit` replaces the session-owned asset with project-owned validation.

- [ ] **Step 1: Write RED workspace lifecycle tests**

Cover native Ready starts `Dirty`; Disabled preserves the current Unsaved behavior; Failed keeps the Guide saveable and displays stable copy; successful project save replaces session ownership with project ownership and becomes Clean; failed save retains the session asset; close/save preserves it until save completes; explicit discard drops/deletes only session scratch; reopening Available enables export; Unavailable keeps Guide usable and disables export.

- [ ] **Step 2: Write RED raw-export update tests**

Cover picker cancel, success, failure/retry, stale worker result ignored by operation ID, and destination overwrite delegated to `rfd`. Assert success/failure never changes `save_state`, `base_revision`, Guide revision, or dirty status.

- [ ] **Step 3: Write RED workspace structural tests**

At 1100×760 and 640×420 assert:

- Ready: duration, dimensions, `30 fps`, `Silent H.264`, and enabled `Save recording…`.
- Failed: `Guide created; screen recording could not be saved.` plus stable category copy, no export button.
- Unavailable/corrupt: Guide controls remain usable and export is disabled.
- None: no motion metadata or export affordance.

- [ ] **Step 4: Run RED workspace tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::motion::tests timeline_workspace::tests`

Expected: compile/behavior failures until workspace motion state exists.

- [ ] **Step 5: Implement workspace ownership transitions**

Store only the opaque `ValidatedMotionAsset`, never a raw path. `build_project_snapshot` clones it. On save success consume `ProjectCommit.motion` and drop the session clone; on failure leave state untouched. `ConfirmDiscard` closes the workspace normally—the last session clone's RAII guard deletes scratch. Do not explicitly delete project-owned assets on discard.

- [ ] **Step 6: Implement Save recording picker/worker**

Use `rfd::AsyncFileDialog` with MP4 filter and overwrite confirmation. Run `rollshot_action::project::export_motion_asset` in `spawn_blocking`; project only stable UI errors. Keep current project dirty state exactly unchanged.

- [ ] **Step 7: Verify workspace contracts**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`

Expected: PASS for save retry, reopen, corrupt asset, discard, and raw-export invariants.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace crates/rollshot-app/src/action_guide_linux_product.rs crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(action-guide): manage and export motion recordings"
```

---

### Task 11: Deterministic iced visual scenarios and independent review

**Files:**
- Modify: existing `iced_test::Simulator` scenario modules in `crates/rollshot-app/src/action_guide_home/` and `crates/rollshot-app/src/timeline_workspace/`
- Modify: overlay structural scenario tests in `crates/rollshot-iced-overlay/src/app.rs`
- Generated evidence only: `target/ui-artifacts/action-guide-motion/`
- Baseline paths: only paths explicitly emitted by the scenario command and approved by the independent reviewer

**Interfaces:**
- Consumes deterministic states from Tasks 7–10.
- Produces baseline/actual/diff evidence for preflight, active on, active failed, workspace Ready, workspace Failed, and workspace Unavailable at default/minimum sizes.

- [ ] **Step 1: Add deterministic scenario manifest**

Pin dark theme, bundled fonts, fixtures, and viewports 1100×760 and 640×420. Emit a machine-readable manifest naming state, viewport, expected key text, baseline, actual, and diff paths. Keep structural assertions primary; screenshots supplement them.

- [ ] **Step 2: Run structural and image scenarios**

Run: `rtk cargo test -p rollshot-app --features action-guide action_guide_motion_ui_scenarios -- --ignored --nocapture`

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide motion_indicator_ui_scenarios -- --ignored --nocapture`

Expected: structural assertions PASS and raw PNG artifacts exist for every manifest row. Missing pixel-diff support is reported, not silently treated as semantic acceptance.

- [ ] **Step 3: Semantically inspect every actual image**

Use native `read` on each actual PNG. Record whether copy is visible, unclipped, non-overlapping, and understandable at both viewports. Any unexplained layout/copy problem returns to the owning product task; do not approve around it.

- [ ] **Step 4: Dispatch the required clean-context baseline reviewer**

Start an independent reviewer with no inherited turns. Provide only requirement, auto mode, changed files, scenario manifest, baseline/actual/diff artifacts, semantic test output, allowed baseline paths, and exact update command. The product-changing agent must not provide its verdict and must not write/approve baselines.

Expected: reviewer either rejects with concrete scenario evidence or accepts and updates only allowed baseline paths. Pixel-only review is insufficient; use a semantic-capable clean reviewer.

- [ ] **Step 5: Re-run accepted scenarios**

Run the two scenario commands again after any approved baseline update.

Expected: PASS with the reviewer-approved baseline set and unchanged structural assertions.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app crates/rollshot-iced-overlay
rtk git commit -m "test(action-guide): cover motion recording UI states"
```

---

### Task 12: End-to-end verification and platform risk record

**Files:**
- Modify only if observed behavior requires a product fix: files from Tasks 1–11
- Update retained evidence after runtime runs: `spikes/action-guide-live-ffmpeg/FINDINGS.md` only when new macOS evidence actually exists

**Interfaces:**
- Verifies the complete opt-out, opt-in, failure, persistence, reopen, export, and discard flows.
- Does not change the approved product scope or relax gates.

- [ ] **Step 1: Run focused crate suites**

```bash
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-iced-overlay --features action-guide
rtk cargo test -p rollshot-app --features action-guide
```

Expected: PASS.

- [ ] **Step 2: Run repository formatting and lint checks**

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS with no warnings.

- [ ] **Step 3: Smoke the Linux opt-out path on hardware**

Launch the Action Guide Home, choose Record New, leave motion unchecked, record/stop a Guide, and inspect processes/files. Verify no FFmpeg/ffprobe resolution or process, no motion scratch/file, unchanged stop/Esc/scroll/detector behavior, and a saveable Guide.

- [ ] **Step 4: Smoke the Linux opt-in success path on hardware**

Enable motion, record changing desktop content, verify persistent `Motion recording on`, stop, inspect Ready metadata, save project, reopen, `Save recording…`, compare SHA-256 with `assets/motion/recording.mp4`, and run ffprobe for H.264, 30/1, zero audio, dimensions, and duration delta ≤34 ms.

- [ ] **Step 5: Smoke Linux failure/save/discard paths**

Break the encoder after start and verify indicator changes to failed and never returns to on; Guide remains saveable. Repeat a successful session, force project-save failure, verify retry retains temp asset; then exercise close→save and close→discard, verifying discard removes only session scratch. Force raw-export failure and verify project asset/destination preservation.

- [ ] **Step 6: Run the Linux layer-shell UI smoke**

Exercise region and fullscreen Action Guide paths on the current AMD workstation, including minimum/default surfaces, stop/Esc/scroll, SNI indicator, failed indicator, workspace Ready/Failed, picker, and close decisions. Record exact observed coverage.

- [ ] **Step 7: Run macOS hardware gates when a machine is available**

First run the retained identical 10-minute zero-copy spike to `macos-10m-arc.json/mp4`; all original gates must pass. Then run the real ScreenCaptureKit product-path scenarios from Steps 3–5, including tray/status and save/reopen/export. If unavailable, record macOS as UNTESTED and do not claim cross-platform completion.

- [ ] **Step 8: Request final code review**

Use `superpowers:requesting-code-review` against both approved specs, this plan, and the complete branch diff. Resolve correctness findings, then repeat every affected focused/smoke check.

- [ ] **Step 9: Final verification commit if fixes/evidence changed files**

```bash
rtk git add crates/rollshot-action crates/rollshot-iced-overlay crates/rollshot-app spikes/action-guide-live-ffmpeg/FINDINGS.md
rtk git commit -m "fix(action-guide): address motion recording verification"
```

Skip this commit when verification made no file changes.

---

## Self-review

- **Spec coverage:** Preflight/consent → Task 7; zero work when disabled → Tasks 7–8; zero-copy action-thread tee → Tasks 1 and 6; bounded latest-frame/CFR → Tasks 2–3; FFmpeg failure isolation and diagnostics → Tasks 3 and 6; Linux/macOS indicators → Tasks 8–9; schema/migration/validation → Tasks 4–5; save/discard ownership → Tasks 5 and 10; raw atomic export → Tasks 5 and 10; structural/visual scenarios and independent baselines → Task 11; focused suites and both hardware paths → Task 12.
- **Approved amendment:** No production-side full-frame copy remains; `Arc` ownership is confined to `ActionRecorder`/overlay action-thread interfaces.
- **Override visibility:** macOS Gate 0 is not silently treated as passed. The user override permits implementation planning only; final claims remain platform-limited until real evidence exists.
- **Type consistency:** `SharedActionFrame`, `MotionFrame`, `ValidatedMotionAsset`, `MotionRecordingOutcome`, `MotionAssetLoad`, `ActionGuideRecordingOptions`, `ActionGuideCaptureResult`, and `WorkspaceMotion` are introduced once and consumed under the same names in later tasks.
- **Placeholder scan:** No prohibited implementation placeholders; hardware-gated macOS evidence has an explicit UNTESTED outcome rather than a fabricated pass.
