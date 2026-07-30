# Action Guide Motion Zero-Copy Feed — Gate 0 Spike Re-run Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-run the retained live-FFmpeg spike with an `Arc`-shared frame offer to prove the producer p99 ≤ 1 ms gate is achievable, producing a GO/NO-GO decision for the zero-copy motion feed design.

**Architecture:** Modify `spikes/action-guide-live-ffmpeg/` in place: `TimedFrame.image` becomes `Arc<RgbaImage>`, the producer's timed section becomes wrap-once + `Arc::clone` + offer, and the identical 10-minute workload is re-run against the identical hard gates. The encoder, queue policy, CFR scheduler, gates, and report format are unchanged so results are comparable to the NO-GO run.

**Tech Stack:** Rust, `crossbeam-channel`, `image` (`RgbaImage`), `ffmpeg-sidecar`-style managed FFmpeg via raw RGBA stdin pipe (existing spike code).

**Spec:** [`docs/superpowers/specs/2026-07-31-action-guide-motion-zero-copy-feed-design.md`](../specs/2026-07-31-action-guide-motion-zero-copy-feed-design.md) (amends the frame handoff of [`2026-07-30-native-action-guide-motion-recording-design.md`](../specs/2026-07-30-native-action-guide-motion-recording-design.md)).

## Global Constraints

- Spike isolation: the crate stays standalone with an empty `[workspace]` table; NEVER add it to the root workspace; production crates stay unchanged.
- Hard gates (verbatim from spec §8.1, unchanged from the NO-GO run):
  - producer p99 frame-offer latency ≤ 1000 µs;
  - no 5-second window with > 10% replaced/dropped offers (queue saturation);
  - output duration differs from source timeline by ≤ 34 ms (one frame at 30 fps);
  - self RSS after 60 s warm-up: peak-to-trough ≤ 64 MiB, slope ≤ 1 MiB/min;
  - success leaves one valid MP4 via atomic rename; failure leaves no partial output;
  - ffprobe: H.264, 1920×1080, 30/1 fps, 0 audio streams.
- Workload parameters unchanged: 1920×1080, 30 fps, 600 s, queue capacity 2, same FFmpeg binary and options.
- Preserve the original NO-GO artifacts: do NOT overwrite `spikes/action-guide-live-ffmpeg/reports/linux-10m.json` or `linux-10m.mp4`; the re-run writes new files (`linux-10m-arc.*`).
- Branch: `feat/native-action-guide-motion-recording`. Commit prefix: `spike(action-guide): ...`.
- Prefix all shell commands with `rtk` (AGENTS.md §6).
- Decision rule: GO requires all gates passing on BOTH Linux (this AMD workstation) and macOS (ScreenCaptureKit product environment). Linux-only pass = decision pending, NOT GO. Any fatal gate failure = NO-GO, stop, escalate to the platform-native encoder design.

---

### Task 1: Zero-copy offer path

**Files:**
- Modify: `spikes/action-guide-live-ffmpeg/src/pipeline.rs` (TimedFrame struct ~line 15, `use` block ~line 1-5, test helper `frame()` ~line 389, mailbox tests ~line 400)
- Modify: `spikes/action-guide-live-ffmpeg/src/main.rs` (producer timed section ~line 208-215)
- Modify: `spikes/action-guide-live-ffmpeg/src/metrics.rs:316` (gate comment only)

**Interfaces:**
- Consumes: existing `pipeline::latest_frame_mailbox`, `pipeline::TimedFrame`, `workload::render_frame(&RunConfig, u64) -> RgbaImage`.
- Produces:
  - `pub(crate) struct TimedFrame { pub at_ms: u64, pub image: Arc<RgbaImage> }`
  - Producer timed section: `Arc::new(frame_image)` once, `Arc::clone` per offer. `LatestFrameSender::offer`, `LatestFrameReceiver::recv`, `run_encoder`, `CfrScheduler` signatures unchanged. `run_encoder` already reads pixels via `frame.image.as_raw()`, which derefs through `Arc` unchanged.

- [ ] **Step 1: Write the failing zero-copy contract test**

Add to `#[cfg(test)] mod tests` in `spikes/action-guide-live-ffmpeg/src/pipeline.rs`:

```rust
#[test]
fn offer_shares_allocation_without_copy() {
    let (sender, receiver) = latest_frame_mailbox(2);
    let image = std::sync::Arc::new(RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])));
    let offered = TimedFrame {
        at_ms: 0,
        image: std::sync::Arc::clone(&image),
    };
    assert_eq!(sender.offer(offered), OfferResult::Queued);
    let received = receiver.recv().unwrap();
    assert!(std::sync::Arc::ptr_eq(&image, &received.image));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml offer_shares_allocation_without_copy`
Expected: FAIL (compile error) — `TimedFrame.image` is `RgbaImage`, `Arc<RgbaImage>` mismatches.

- [ ] **Step 3: Change `TimedFrame` to shared ownership**

In `spikes/action-guide-live-ffmpeg/src/pipeline.rs`:

Add `use std::sync::Arc;` to the `use` block at the top of the file, then change the struct:

```rust
pub(crate) struct TimedFrame {
    pub at_ms: u64,
    pub image: Arc<RgbaImage>,
}
```

Update the test helper `frame(at_ms)` (~line 389):

```rust
fn frame(at_ms: u64) -> TimedFrame {
    TimedFrame {
        at_ms,
        image: Arc::new(RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]))),
    }
}
```

No other `pipeline.rs` changes: `run_encoder` reads `frame.image.as_raw()` (Arc derefs to `RgbaImage`), and the mailbox moves `TimedFrame` by value regardless of payload type.

- [ ] **Step 4: Change the producer timed section to share + offer**

In `spikes/action-guide-live-ffmpeg/src/main.rs`, replace the timed section (~lines 208-215):

```rust
        // Timed section: share and offer. The wrap models the production
        // crop-then-wrap-once at the action-thread boundary; the offer is an
        // Arc clone (pointer + refcount), never a pixel copy.
        let offer_start = Instant::now();
        let shared = Arc::new(frame_image);
        let timed = pipeline::TimedFrame {
            at_ms: frame_index * 1_000 / config.fps as u64,
            image: Arc::clone(&shared),
        };
        let result = sender.offer(timed);
        drop(shared);
        let offer_us = offer_start.elapsed().as_micros() as u64;
```

`Arc` is already imported in `main.rs` (used for `ffmpeg_child_pid`).

In `spikes/action-guide-live-ffmpeg/src/metrics.rs:316`, update the comment only:

```rust
    // Gate 1: Producer p99 share+offer <= 1_000 µs
```

- [ ] **Step 5: Run the full spike test suite**

Run: `rtk cargo test --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml`
Expected: PASS — all pre-existing tests plus `offer_shares_allocation_without_copy` (43 total).

- [ ] **Step 6: Format and lint**

Run: `rtk cargo fmt --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml -- --check`
Expected: PASS (run without `--check` to apply if needed).

Run: `rtk cargo clippy --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml --all-targets -- -D warnings`
Expected: PASS. Note: the `#[allow(dead_code)]` attributes in `pipeline.rs` are stale (the producer loop already consumes this API); remove them only if clippy or rustc flags them — do not refactor beyond this task's change.

- [ ] **Step 7: Commit**

```bash
rtk git add spikes/action-guide-live-ffmpeg/src/pipeline.rs spikes/action-guide-live-ffmpeg/src/main.rs spikes/action-guide-live-ffmpeg/src/metrics.rs
rtk git commit -m "spike(action-guide): share frames via Arc in offer path"
```

---

### Task 2: Linux 10-minute re-run against identical gates

**Files:**
- Create (generated, gitignored): `spikes/action-guide-live-ffmpeg/reports/linux-10m-arc.json`, `spikes/action-guide-live-ffmpeg/reports/linux-10m-arc.mp4`
- Modify: none (runtime evidence only)

**Interfaces:**
- Consumes: Task 1 binary; the managed FFmpeg paths via `$ROLLSHOT_FFMPEG` / `$ROLLSHOT_FFPROBE` (same resolution the original run used).
- Produces: exit status 0 (GO) or 2 (NO-GO), plus the JSON report consumed by Task 4.

- [ ] **Step 1: Confirm the original artifacts are preserved**

Run: `rtk proxy ls -l spikes/action-guide-live-ffmpeg/reports/`
Expected: `linux-10m.json` and `linux-10m.mp4` present. The re-run writes `linux-10m-arc.*`; if the original files are missing, STOP and flag it — the NO-GO evidence must not be clobbered.

- [ ] **Step 2: Build release binary**

Run: `rtk cargo build --release --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml`
Expected: PASS.

- [ ] **Step 3: Run the 10-minute workload**

Resolve the managed FFmpeg paths the same way the original run did (`$ROLLSHOT_FFMPEG`, `$ROLLSHOT_FFPROBE`); then:

```bash
rtk cargo run --release --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml -- \
  --ffmpeg $ROLLSHOT_FFMPEG --ffprobe $ROLLSHOT_FFPROBE \
  --output spikes/action-guide-live-ffmpeg/reports/linux-10m-arc.mp4 \
  --report spikes/action-guide-live-ffmpeg/reports/linux-10m-arc.json \
  --width 1920 --height 1080 --fps 30 --duration-secs 600 --queue-capacity 2
```

Expected: exit status 0 (all gates pass). Exit status 2 = hard gate failure — record which gates from stderr, proceed directly to Task 4 with a NO-GO; do NOT relax thresholds or re-run with tweaked parameters.

- [ ] **Step 4: Verify gate evidence in the report**

`RunReport` has no precomputed p99 field; `gate_decision` is the evaluated verdict and `offer_latencies_us` holds the raw samples. Run:

```bash
rtk proxy jq '{decision: .gate_decision.decision, failed: .gate_decision.failed_gates, frames_written: .encoder_frames_written, offers: (.offer_latencies_us | length)}' spikes/action-guide-live-ffmpeg/reports/linux-10m-arc.json
rtk proxy jq '(.offer_latencies_us | sort) as $s | {p50: $s[((($s|length)-1)*0.50|floor)], p99: $s[((($s|length)-1)*0.99|ceil)], max: $s[-1]}' spikes/action-guide-live-ffmpeg/reports/linux-10m-arc.json
```

Expected: `decision == "Go"`, `failed == []`, `frames_written == 18000`, `offers == 18000`, p99 well under 1000 µs (Arc share is pointer + refcount; expect single-digit µs).

- [ ] **Step 5: Commit the report artifact if the spike tracks reports**

Check whether `reports/` is gitignored (`spikes/action-guide-live-ffmpeg/.gitignore` currently ignores one entry). If reports are tracked, commit the new JSON (not the MP4):

```bash
rtk git add spikes/action-guide-live-ffmpeg/reports/linux-10m-arc.json
rtk git commit -m "spike(action-guide): record linux zero-copy gate evidence"
```

If reports are gitignored, skip this step and note the artifact path for Task 4.

---

### Task 3: macOS re-run (hardware-gated)

**Files:**
- Create (generated): `spikes/action-guide-live-ffmpeg/reports/macos-10m-arc.json`, `spikes/action-guide-live-ffmpeg/reports/macos-10m-arc.mp4`

**Interfaces:**
- Consumes: Task 1 code on a macOS machine with the managed FFmpeg.
- Produces: macOS gate evidence, or an explicit UNTESTED record.

- [ ] **Step 1: Check macOS hardware availability**

The current workstation is Linux-only. Ask the user for a macOS machine with the ScreenCaptureKit product environment (the spec requires runtime/hardware evidence there; compilation is not evidence). If no macOS environment is available, record UNTESTED in Task 4 and stop — the decision stays pending; do NOT declare GO from Linux alone.

- [ ] **Step 2: Run the identical workload on macOS**

Same command as Task 2 Step 3, with report/output paths `macos-10m-arc.json` / `macos-10m-arc.mp4`, on the macOS machine.

Expected: exit status 0. Exit status 2 = NO-GO; record failed gates.

---

### Task 4: FINDINGS.md update and final decision

**Files:**
- Modify: `spikes/action-guide-live-ffmpeg/FINDINGS.md`

**Interfaces:**
- Consumes: `reports/linux-10m-arc.json` (Task 2), `reports/macos-10m-arc.json` or UNTESTED (Task 3).
- Produces: the GO/NO-GO/pending decision and product handoff consumed by the next planning round.

- [ ] **Step 1: Record the re-run in FINDINGS.md**

Update `spikes/action-guide-live-ffmpeg/FINDINGS.md`:

- Add the re-run environment block (OS, CPU, rustc, FFmpeg/ffprobe versions, exact command, exit status) under `## Environment`, labeled `### Linux zero-copy re-run (runtime/hardware)`.
- Update `## Risk Results`: keep the original NO-GO table as the historical record (mark it superseded), add a new table for the zero-copy run with PASS/FAIL per gate and the `linux-10m-arc.json` artifact path. Fill the macOS table from Task 3, or leave every row UNTESTED with the reason.
- Add an `## Observations` subsection for the re-run: frames produced, offer outcomes, p50/p99/max offer latency, saturation windows, self RSS range/slope, FFmpeg RSS, probe result — same shape as the existing observations, numbers from `linux-10m-arc.json`.
- Update `## Final Recommendation`:
  - All gates pass on both platforms → **GO**; product handoff: write the production implementation plan for the 2026-07-30 spec as amended by the 2026-07-31 zero-copy design.
  - Any fatal gate fails → **NO-GO**; record the failed gate and escalate: the next design is platform-native encoding, per the decision tree in both specs.
  - Linux pass + macOS UNTESTED → decision **pending**; state that production planning may proceed only for work that does not require the macOS gate, and the macOS run remains a hard prerequisite for cross-platform completion claims.
- Keep the original NO-GO run's observations intact — do not rewrite history; mark it as the superseded first run.
- Update the `Last updated` date.

- [ ] **Step 2: Commit**

```bash
rtk git add spikes/action-guide-live-ffmpeg/FINDINGS.md
rtk git commit -m "spike(action-guide): decide zero-copy offer feasibility"
```

---

## Self-review notes

- Spec coverage: spec §8.1 gates → Task 2/3 (identical thresholds in Global Constraints); §8.2 platform evidence → Task 2 (Linux), Task 3 (macOS); §8.3 decision rule → Global Constraints + Task 4; §5 frame-flow change → Task 1; comparability requirement (identical parameters) → Global Constraints + Task 2 Step 3.
- Production implementation is deliberately NOT planned here: it is gated on the GO decision (spec §8.3). A GO hands off to a separate production plan.
