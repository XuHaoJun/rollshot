# Native Action Guide Motion Recording Feasibility Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Determine whether Rollshot's existing `ffmpeg-sidecar` path can encode a 1920×1080, 30 fps, silent Action Guide RGBA stream in real time without blocking the capture producer or growing Rollshot memory without bound.

**Architecture:** Build an isolated standalone Rust crate under `spikes/action-guide-live-ffmpeg/`. A deterministic desktop-like frame producer clones each offered RGBA frame into a bounded latest-frame mailbox; a worker normalizes timestamps to CFR and writes raw RGBA to Rollshot's managed FFmpeg. The spike emits machine-readable metrics and a GO/NO-GO decision; production crates remain unchanged.

**Tech Stack:** Rust 2024, `ffmpeg-sidecar = 2.5.2`, `image = 0.25`, `crossbeam-channel = 0.5`, `serde`, `serde_json`, `tracing`, FFmpeg/ffprobe, H.264/libx264.

## Global Constraints

- Production crates must remain unchanged; the spike is a standalone crate with an empty `[workspace]` table.
- Use the same raw RGBA pipe, H.264 options, 30 fps timing, bounded latest-frame policy, and atomic-output lifecycle proposed by the approved design.
- Use a representative changing desktop workload; flat-color or static frames are invalid performance evidence.
- Runtime diagnostics use stable `rollshot::*` targets and structured fields; never log pixels, paths, filenames, or FFmpeg command lines containing paths.
- The producer path must never block on the encoder. Frame cloning plus offer has a hard p99 limit of 1 ms.
- Persistent saturation means any complete five-second window drops more than 10% of offered frames; the hard gate requires zero persistently saturated windows.
- Memory is bounded when, after the first warm-up minute, self RSS grows by no more than 64 MiB peak-to-trough and the least-squares RSS slope is no more than 1 MiB/minute.
- Output duration must differ from the source timeline by no more than one 30 fps frame (34 ms).
- Linux and macOS require runtime/hardware evidence. Compile success is not runtime evidence.
- Stop at the first failed hard gate. Record NO-GO evidence; do not begin production implementation.
- Shell commands in this repository are prefixed with `rtk`.

---

## File Structure

- `spikes/action-guide-live-ffmpeg/Cargo.toml` — isolated dependency pins and empty workspace boundary.
- `spikes/action-guide-live-ffmpeg/src/main.rs` — strict CLI parsing, run orchestration, report serialization, and exit status.
- `spikes/action-guide-live-ffmpeg/src/workload.rs` — deterministic desktop-like RGBA workload with continuous scroll, cursor, panel, and text-bar changes.
- `spikes/action-guide-live-ffmpeg/src/pipeline.rs` — bounded latest-frame mailbox, timestamp-to-CFR scheduler, FFmpeg process lifecycle, and atomic output.
- `spikes/action-guide-live-ffmpeg/src/metrics.rs` — producer latency percentiles, five-second saturation windows, RSS samples/slope, ffprobe validation, and gate evaluation.
- `spikes/action-guide-live-ffmpeg/FINDINGS.md` — environment, exact commands, evidence levels, results, and GO/NO-GO handoff.
- `spikes/action-guide-live-ffmpeg/reports/` — gitignored runtime JSON reports and MP4 artifacts; findings cite their paths but do not commit large outputs.

No root `Cargo.toml`, production crate, project schema, capture path, or UI file changes in this plan.

---

### Task 1: Deterministic Desktop Workload and Strict CLI

**Files:**
- Create: `spikes/action-guide-live-ffmpeg/Cargo.toml`
- Create: `spikes/action-guide-live-ffmpeg/src/main.rs`
- Create: `spikes/action-guide-live-ffmpeg/src/workload.rs`

**Interfaces:**
- Consumes: explicit `--ffmpeg`, `--ffprobe`, `--output`, and `--report` paths plus numeric run configuration.
- Produces: `RunConfig`, `workload::render_frame(&RunConfig, frame_index) -> image::RgbaImage`, and a strict CLI that rejects unknown or missing arguments.

- [ ] **Step 1: Create the isolated crate manifest**

```toml
[package]
name = "action-guide-live-ffmpeg-spike"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
crossbeam-channel = "0.5"
ffmpeg-sidecar = { version = "=2.5.2", default-features = false, features = ["download_ffmpeg"] }
image = { version = "0.25", default-features = false }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
tempfile = "3"

[workspace]
```

- [ ] **Step 2: Write workload tests before the renderer**

Add these tests to `src/workload.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RunConfig {
        RunConfig::for_test(320, 180, 30)
    }

    #[test]
    fn frame_dimensions_and_alpha_are_stable() {
        let frame = render_frame(&config(), 17);
        assert_eq!(frame.dimensions(), (320, 180));
        assert!(frame.pixels().all(|pixel| pixel.0[3] == 255));
    }

    #[test]
    fn workload_is_deterministic_but_changes_each_second() {
        let cfg = config();
        let first = render_frame(&cfg, 0);
        assert_eq!(first, render_frame(&cfg, 0));
        assert_ne!(first, render_frame(&cfg, cfg.fps as u64));
    }

    #[test]
    fn workload_changes_more_than_a_cursor_patch() {
        let cfg = config();
        let first = render_frame(&cfg, 0);
        let next = render_frame(&cfg, cfg.fps as u64);
        let changed = first
            .pixels()
            .zip(next.pixels())
            .filter(|(left, right)| left != right)
            .count();
        assert!(changed > (cfg.width as usize * cfg.height as usize) / 20);
    }
}
```

- [ ] **Step 3: Run the workload tests and verify they fail**

Run:

```bash
rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml workload::tests
```

Expected: compilation fails because `RunConfig` and `render_frame` do not exist.

- [ ] **Step 4: Implement the deterministic desktop-like renderer**

Define the shared configuration in `src/main.rs`:

```rust
#[derive(Clone)]
pub(crate) struct RunConfig {
    pub ffmpeg: std::path::PathBuf,
    pub ffprobe: std::path::PathBuf,
    pub output: std::path::PathBuf,
    pub report: std::path::PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_secs: u64,
    pub queue_capacity: usize,
}

impl RunConfig {
    #[cfg(test)]
    pub(crate) fn for_test(width: u32, height: u32, fps: u32) -> Self {
        Self {
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            output: "out.mp4".into(),
            report: "report.json".into(),
            width,
            height,
            fps,
            duration_secs: 2,
            queue_capacity: 2,
        }
    }
}
```

In `src/workload.rs`, render a dark desktop background, two contrasting window
panels, a vertically scrolling list of text-like bars, a changing chart region,
and a moving cursor. Derive every position and color from `frame_index`; do not
use random numbers. Fill every pixel alpha with 255. The renderer must touch at
least 5% of pixels between frames one second apart, as enforced by the test.

- [ ] **Step 5: Implement strict CLI parsing**

Accept exactly:

```text
--ffmpeg PATH
--ffprobe PATH
--output PATH
--report PATH
--width U32          default 1920
--height U32         default 1080
--fps U32            default 30
--duration-secs U64  default 600
--queue-capacity N   default 2
```

Reject zero dimensions, zero fps, zero duration, zero queue capacity, odd width
or height, missing values, and unknown flags. Test parsing as a pure function.
The initial `main` validates the config and exits without printing path fields;
Task 3 replaces that behavior with the run.

- [ ] **Step 6: Run the crate tests**

Run:

```bash
rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml
```

Expected: all workload and CLI tests pass.

- [ ] **Step 7: Commit the workload and CLI**

```bash
rtk git add spikes/action-guide-live-ffmpeg/Cargo.toml spikes/action-guide-live-ffmpeg/src/main.rs spikes/action-guide-live-ffmpeg/src/workload.rs
rtk git commit -m "spike(action-guide): add live encoding workload"
```

---

### Task 2: Bounded Latest-Frame Pipeline and CFR Scheduler

**Files:**
- Create: `spikes/action-guide-live-ffmpeg/src/pipeline.rs`
- Modify: `spikes/action-guide-live-ffmpeg/src/main.rs`

**Interfaces:**
- Consumes: `RunConfig` and owned `image::RgbaImage` values with session-relative `at_ms`.
- Produces:
  - `pipeline::LatestFrameSender::offer(frame: TimedFrame) -> OfferResult`
  - `pipeline::LatestFrameReceiver::recv() -> Result<TimedFrame, crossbeam_channel::RecvError>`
  - `pipeline::CfrScheduler::push(at_ms: u64) -> u64`
  - `pipeline::CfrScheduler::finish(duration_ms: u64) -> u64`
  - `pipeline::run_encoder(config: RunConfig, receiver: LatestFrameReceiver) -> Result<EncoderSummary, PipelineError>`

- [ ] **Step 1: Write failing mailbox and scheduler tests**

```rust
#[test]
fn latest_mailbox_replaces_oldest_without_waiting() {
    let (sender, receiver) = latest_frame_mailbox(1);
    assert_eq!(sender.offer(frame(0)), OfferResult::Queued);
    assert_eq!(sender.offer(frame(33)), OfferResult::ReplacedOldest);
    assert_eq!(receiver.recv().unwrap().at_ms, 33);
}

#[test]
fn cfr_scheduler_holds_last_frame_across_timestamp_gap() {
    let mut scheduler = CfrScheduler::new(30);
    assert_eq!(scheduler.push(0), 1);
    assert_eq!(scheduler.push(100), 3);
    assert_eq!(scheduler.finish(134), 1);
    assert_eq!(scheduler.frames_written(), 5);
}

#[test]
fn scheduler_duration_is_within_one_frame() {
    let mut scheduler = CfrScheduler::new(30);
    scheduler.push(0);
    scheduler.push(997);
    scheduler.finish(1_000);
    assert!((scheduler.duration_ms() as i64 - 1_000).abs() <= 34);
}
```

`CfrScheduler::push` returns the number of output ticks caused by an arrival:
the worker writes the prior image for earlier ticks and the new image on the
arrival tick. The first push writes the initial frame.

- [ ] **Step 2: Run the focused tests and verify failure**

```bash
rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml pipeline::tests
```

Expected: compilation fails because the mailbox and scheduler types are absent.

- [ ] **Step 3: Implement the bounded latest-frame mailbox**

Use `crossbeam_channel::bounded(capacity)`. Keep a clone of the receiver in the
sender solely for eviction. `offer` performs `try_send`; on `Full(frame)`, call
`try_recv` once to evict the oldest item and then `try_send(frame)` once. It
never calls blocking `send`.

```rust
pub(crate) struct TimedFrame {
    pub at_ms: u64,
    pub image: image::RgbaImage,
}
```

The sender takes ownership of `TimedFrame`; it never clones pixels internally.
Return one of:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OfferResult {
    Queued,
    ReplacedOldest,
    Disconnected,
}
```

The mailbox owns at most `queue_capacity` frame buffers. `Disconnected` stops
new motion offers and is counted as a failed motion offer.

- [ ] **Step 4: Implement integer-only CFR scheduling**

Represent the next output tick as a frame index. Compare timestamps using
`tick_index * 1_000` against `at_ms * fps` in `u128` arithmetic to avoid drift
and overflow. Never derive timing from wall-clock sleep inside the encoder.
`finish(duration_ms)` emits the prior frame until the encoded timeline reaches
the requested duration.

- [ ] **Step 5: Implement FFmpeg process lifecycle**

Build `FfmpegCommand::new_with_path(&config.ffmpeg)` with these arguments:

```text
-y
-f rawvideo
-pixel_format rgba
-video_size WIDTHxHEIGHT
-framerate FPS
-i pipe:0
-an
-vf format=yuv420p
-c:v libx264
-preset veryfast
-crf 23
-movflags +faststart
-f mp4
OUTPUT.tmp.mp4
```

Drain stderr on its own thread before writing frames, matching the existing
`rollshot-action/src/video.rs` deadlock prevention. `take_stdin()` owns the raw
pipe. On write, flush, wait, or non-zero-exit failure: close stdin, kill/wait as
needed, join stderr, remove the temp file, and return a typed `PipelineError`.
Only rename the temp sibling to the requested output after a successful exit.

`EncoderSummary` records frames written, process ID, FFmpeg exit status,
source duration, encoded duration, and a privacy-safe error category. It never
stores stderr text or a path in the serialized report.

- [ ] **Step 6: Add Unix failure-cleanup tests**

Create a temporary executable shell script containing `#!/bin/sh\nexit 7\n`,
run the encoder against it under `#[cfg(unix)]`, and assert:

```rust
assert!(matches!(result, Err(PipelineError::Exit { .. })));
assert!(!config.output.exists());
assert!(!temp_output_path(&config.output).exists());
```

Also test a nonexistent executable maps to `PipelineError::Spawn` without an
output file.

- [ ] **Step 7: Run pipeline tests**

```bash
rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml pipeline::tests
```

Expected: mailbox, CFR, spawn-failure, exit-failure, and cleanup tests pass.

- [ ] **Step 8: Commit the bounded pipeline**

```bash
rtk git add spikes/action-guide-live-ffmpeg/src/main.rs spikes/action-guide-live-ffmpeg/src/pipeline.rs
rtk git commit -m "spike(action-guide): pipe bounded frames to ffmpeg"
```

---

### Task 3: Metrics, Hard-Gate Evaluator, and Short Runtime Proof

**Files:**
- Create: `spikes/action-guide-live-ffmpeg/src/metrics.rs`
- Modify: `spikes/action-guide-live-ffmpeg/src/main.rs`
- Modify: `spikes/action-guide-live-ffmpeg/src/pipeline.rs`

**Interfaces:**
- Consumes: offer latency samples, offer outcomes, one-second self/FFmpeg RSS samples, encoder summary, and ffprobe JSON.
- Produces:
  - `metrics::RunReport`
  - `metrics::evaluate(report: RunReport) -> GateDecision`
  - process exit 0 for GO and 2 for NO-GO
  - one pretty JSON report at `RunConfig.report`

- [ ] **Step 1: Write failing gate-evaluation tests**

```rust
#[test]
fn gate_rejects_slow_producer_p99() {
    let report = fixture_report().with_offer_latencies_us(vec![200; 99]
        .into_iter().chain([1_100]).collect());
    assert_eq!(evaluate(report).decision, Decision::NoGo);
    assert!(evaluate(report).failed_gates.contains(&Gate::ProducerP99));
}

#[test]
fn gate_rejects_persistent_five_second_saturation() {
    let report = fixture_report().with_window(OfferWindow {
        offered: 150,
        replaced_or_dropped: 16,
    });
    assert!(evaluate(report).failed_gates.contains(&Gate::QueueSaturation));
}

#[test]
fn gate_rejects_unbounded_memory_growth() {
    let report = fixture_report().with_post_warmup_rss_mib(vec![100, 120, 145, 170]);
    assert!(evaluate(report).failed_gates.contains(&Gate::MemoryBound));
}

#[test]
fn gate_accepts_duration_difference_of_one_frame() {
    let report = fixture_report().with_durations(10_000, 10_034);
    assert!(!evaluate(report).failed_gates.contains(&Gate::Duration));
}
```

- [ ] **Step 2: Run evaluator tests and verify failure**

```bash
rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml metrics::tests
```

Expected: compilation fails because report and gate types are absent.

- [ ] **Step 3: Implement metrics collection**

Measure producer latency from immediately before `RgbaImage::clone()` through
`LatestFrameSender::offer`. Store microsecond samples for percentile analysis.
Bucket offers into exact session-relative five-second windows. Sample self RSS
and FFmpeg RSS once per second with:

```rust
fn rss_kib(pid: u32) -> std::io::Result<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()?;
    let text = std::str::from_utf8(&output.stdout)
        .map_err(std::io::Error::other)?;
    text.trim().parse::<u64>().map_err(std::io::Error::other)
}
```

Use `FfmpegChild::as_inner().id()` for the child PID. Record sampling failures
as `UNTESTED` for the memory gate; an untested hard gate yields NO-GO, not PASS.

- [ ] **Step 4: Implement ffprobe validation**

Run the explicit `--ffprobe` executable with:

```text
-v error
-show_entries stream=codec_name,codec_type,width,height,avg_frame_rate
-show_entries format=duration
-of json
OUTPUT
```

Deserialize only those fields. Require exactly one H.264 video stream, no audio
stream, expected dimensions, average frame rate `30/1`, and duration within
34 ms. Serialize derived values, never raw probe output or paths.

- [ ] **Step 5: Implement objective hard gates**

`GateDecision` is GO only when all are true:

```text
producer p99 clone+offer <= 1_000 µs
zero five-second windows with dropped/replaced ratio > 10%
post-warm-up self RSS peak-to-trough <= 64 MiB
post-warm-up self RSS least-squares slope <= 1 MiB/min
output duration delta <= 34 ms
FFmpeg exited successfully and atomic output exists
probe reports H.264, 30/1 fps, expected dimensions, and no audio stream
```

The report includes every raw numeric aggregate needed to audit the decision,
plus environment strings from `uname -a`, `rustc --version`, `cargo --version`,
`ffmpeg -version` first line, and `ffprobe -version` first line. Do not include
executable or output paths.

- [ ] **Step 6: Wire the real-time producer and worker in `main`**

Start the encoder worker first. For `duration_secs * fps` frames:

1. render the deterministic frame outside the timed section;
2. sleep until the frame's absolute `Instant` deadline;
3. start the offer timer;
4. clone the frame and offer it with `at_ms = frame_index * 1_000 / fps`;
5. stop the offer timer and record the outcome;
6. sample RSS on second boundaries.

After the final offer, disconnect the sender, finalize the encoder to exactly
`duration_secs * 1_000`, probe the output, evaluate gates, atomically write the
JSON report, and exit 0 for GO or 2 for NO-GO. Operational errors exit 1 and
still write a report with a stable failure category when the report path is
available.

- [ ] **Step 7: Run automated tests**

```bash
rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml
```

Expected: workload, CLI, mailbox, scheduler, cleanup, metrics, probe-parser, and
gate-evaluator tests all pass.

- [ ] **Step 8: Run a five-second real FFmpeg proof**

```bash
rtk cargo run --release --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml -- \
  --ffmpeg "$ROLLSHOT_FFMPEG" \
  --ffprobe "$ROLLSHOT_FFPROBE" \
  --output spikes/action-guide-live-ffmpeg/reports/short.mp4 \
  --report spikes/action-guide-live-ffmpeg/reports/short.json \
  --width 1920 --height 1080 --fps 30 --duration-secs 5 --queue-capacity 2
```

Expected: exit 0, `short.json` says `GO`, `short.mp4` probes as silent H.264
1920×1080 at 30 fps, and no temp sibling remains.

- [ ] **Step 9: Commit metrics and short-runtime proof**

Add `reports/` to `spikes/action-guide-live-ffmpeg/.gitignore`; do not stage the
JSON or MP4 artifacts.

```bash
rtk git add spikes/action-guide-live-ffmpeg/.gitignore spikes/action-guide-live-ffmpeg/src/main.rs spikes/action-guide-live-ffmpeg/src/pipeline.rs spikes/action-guide-live-ffmpeg/src/metrics.rs
rtk git commit -m "spike(action-guide): evaluate live encoding gates"
```

---

### Task 4: Linux and macOS Hardware Evidence and Decision

**Files:**
- Create: `spikes/action-guide-live-ffmpeg/FINDINGS.md`
- Runtime-only: `spikes/action-guide-live-ffmpeg/reports/linux-10m.json`
- Runtime-only: `spikes/action-guide-live-ffmpeg/reports/linux-10m.mp4`
- Runtime-only: `spikes/action-guide-live-ffmpeg/reports/macos-10m.json`
- Runtime-only: `spikes/action-guide-live-ffmpeg/reports/macos-10m.mp4`

**Interfaces:**
- Consumes: the release spike binary and explicit managed FFmpeg/ffprobe paths on each platform.
- Produces: one evidence-backed GO/NO-GO decision in `FINDINGS.md`; GO authorizes a new production implementation plan, while NO-GO requires a platform-native encoder design.

- [ ] **Step 1: Create the active findings record before running hardware evidence**

Use this exact initial status:

```markdown
# Action Guide Live FFmpeg Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: Native Action Guide Motion Recording design Gate 0
- Started: 2026-07-30
- Last updated: 2026-07-30

## Decision

Determine whether Rollshot's managed FFmpeg can encode a 1920×1080, 30 fps,
silent, desktop-like RGBA Action Guide stream in real time without blocking the
capture producer or growing Rollshot memory without bound.

## Environment

Environment evidence is recorded separately for Linux and macOS. A platform
remains UNTESTED until its ten-minute runtime command completes on hardware.

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| Producer blocking | hard | runtime/hardware | UNTESTED | p99 clone+offer <= 1 ms |
| Persistent queue saturation | hard | runtime/hardware | UNTESTED | no five-second window above 10% replacement/drop |
| Self memory growth | hard | runtime/hardware | UNTESTED | <= 64 MiB range and <= 1 MiB/min slope after warm-up |
| Timeline fidelity | hard | runtime/hardware | UNTESTED | duration delta <= 34 ms |
| Media contract | hard | runtime/hardware | UNTESTED | H.264, 1920x1080, 30/1, no audio |
| Atomic cleanup | hard | automated/runtime | UNTESTED | no partial output on failure |

## Observations

No hardware run has been recorded.

## Final Recommendation

- Go / no-go: UNTESTED
- Supporting evidence: none yet
- Rejected alternatives: none yet
- Fallback triggers: any failed hard gate requires a platform-native encoder design
- Remaining risks: Linux and macOS runtime behavior
- Product handoff: do not write the production implementation plan before the decision
```

- [ ] **Step 2: Run the ten-minute Linux hardware command**

On the current AMD Linux workstation:

```bash
rtk cargo run --release --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml -- \
  --ffmpeg "$ROLLSHOT_FFMPEG" \
  --ffprobe "$ROLLSHOT_FFPROBE" \
  --output spikes/action-guide-live-ffmpeg/reports/linux-10m.mp4 \
  --report spikes/action-guide-live-ffmpeg/reports/linux-10m.json \
  --width 1920 --height 1080 --fps 30 --duration-secs 600 --queue-capacity 2
```

Expected for GO: exit 0 and every hard gate in `linux-10m.json` passes. If exit
2 or any hard gate fails, stop, record NO-GO, and do not run lower-priority
experiments to rescue the selected architecture.

- [ ] **Step 3: Record Linux evidence immediately**

Copy exact environment strings and aggregate metrics from `linux-10m.json` into
`FINDINGS.md`. Mark each row `PASS` or `FAIL`, evidence
`runtime/hardware`, and cite `reports/linux-10m.json`. Do not commit the JSON or
MP4.

- [ ] **Step 4: Run the ten-minute macOS hardware command if Linux passed**

On a macOS machine using the product's ScreenCaptureKit-capable environment:

```bash
rtk cargo run --release --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml -- \
  --ffmpeg "$ROLLSHOT_FFMPEG" \
  --ffprobe "$ROLLSHOT_FFPROBE" \
  --output spikes/action-guide-live-ffmpeg/reports/macos-10m.mp4 \
  --report spikes/action-guide-live-ffmpeg/reports/macos-10m.json \
  --width 1920 --height 1080 --fps 30 --duration-secs 600 --queue-capacity 2
```

Expected for GO: exit 0 and every hard gate in `macos-10m.json` passes. If the
required macOS environment is unavailable, record all macOS hard gates as
`UNTESTED`; the overall decision remains UNTESTED and production planning does
not start.

- [ ] **Step 5: Record the final decision**

Set:

- **GO** only if Linux and macOS pass every hard gate;
- **NO-GO** immediately on the first failed hard gate; or
- **UNTESTED** if either required platform lacks runtime evidence.

For GO, set `Lifecycle: retained-reference`, state that a production
implementation plan may now be written against the approved design, and carry
forward observed CPU, FFmpeg RSS, drop counts, and any soft caveats. For NO-GO,
set `Lifecycle: retained-reference`, name the failed gate, and hand off to a
new platform-native encoder design. For UNTESTED, keep `Lifecycle: active`.

- [ ] **Step 6: Verify the standalone spike and findings consistency**

```bash
rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml
rtk cargo fmt --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml --check
rtk cargo clippy --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml --all-targets -- -D warnings
```

Expected: all tests pass, formatting is clean, and clippy reports no warnings.
Confirm every number quoted in `FINDINGS.md` exists in the cited report and the
overall decision matches the hard-gate rule.

- [ ] **Step 7: Commit the decision record**

```bash
rtk git add spikes/action-guide-live-ffmpeg/Cargo.lock spikes/action-guide-live-ffmpeg/FINDINGS.md
rtk git commit -m "spike(action-guide): decide live ffmpeg feasibility"
```

Do not stage `reports/`. If the decision is GO, return to
`superpowers:writing-plans` and write the production implementation plan for
`docs/superpowers/specs/2026-07-30-native-action-guide-motion-recording-design.md`.
If the decision is NO-GO, return to `superpowers:brainstorming` for the
platform-native fallback. If UNTESTED, obtain the missing hardware evidence
before either transition.
