# Capture Diagnostics & Consumer-Side Frame Pacing

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add post-capture summary diagnostics and consumer-side frame pacing to prevent fast manual scrolling from breaking stitching continuity on PipeWire portal captures.

**Architecture:** Two changes to `cmd_capture.rs`: (1) after the capture loop, compute and print summary statistics (interval p50/p90/max, max accepted motion, longest NoMatch run, warnings); (2) inside the capture loop, enforce a minimum interval between processed frames by skipping frames that arrive too soon after the last accepted frame. Both features are consumer-side, no changes to the PipeWire producer or FrameQueue.

**Tech Stack:** Rust, clap (CLI args), serde_json (report), rollshot-cli crate

**Issue:** `docs/issues/2026-05-23-fast-scroll-capture-sampling-gap.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/rollshot-cli/src/cmd_capture.rs` | Modify | Add diagnostics summary + frame pacing logic |
| `crates/rollshot-cli/src/args.rs` | Modify | Add `--min-interval-ms` CLI arg |
| `crates/rollshot-cli/tests/capture_fixture.rs` | Modify | Add tests for diagnostics output and frame pacing |

---

### Task 1: Capture Summary Diagnostics

Print a summary block to stderr after the capture loop completes. This covers item 1 from the issue: max/p50/p90 `capture_interval_ms`, max accepted `abs(dy)`, longest consecutive NoMatch run, and a warning when motion or NoMatch runs suggest scroll is too fast.

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs:200-218`
- Test: `crates/rollshot-cli/tests/capture_fixture.rs`

- [ ] **Step 1: Write the failing test — diagnostics appear on stderr**

Add to `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
#[test]
fn rollshot_capture_fixture_prints_diagnostics_summary() {
    let tempdir = temp_dir("diagnostics-summary");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("capture_interval_ms"),
        "diagnostics should include capture interval stats; stderr = {stderr}"
    );
    assert!(
        stderr.contains("max_accepted_dy"),
        "diagnostics should include max accepted motion; stderr = {stderr}"
    );
    assert!(
        stderr.contains("longest_no_match_run"),
        "diagnostics should include longest NoMatch run; stderr = {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-cli --test capture_fixture rollshot_capture_fixture_prints_diagnostics_summary`
Expected: FAIL — stderr does not contain `capture_interval_ms`

- [ ] **Step 3: Write the failing test — diagnostics suppressed by --quiet**

Add to `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
#[test]
fn rollshot_capture_quiet_suppresses_diagnostics() {
    let tempdir = temp_dir("quiet-diagnostics");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .arg("--quiet")
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        !stderr.contains("capture_interval_ms"),
        "quiet mode should suppress diagnostics; stderr = {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 4: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-cli --test capture_fixture rollshot_capture_quiet_suppresses_diagnostics`
Expected: FAIL (but may pass trivially since diagnostics aren't printed yet — that's fine, this test locks correct quiet behavior once diagnostics are added)

- [ ] **Step 5: Implement diagnostics summary**

In `crates/rollshot-cli/src/cmd_capture.rs`, add a `print_diagnostics_summary` function and call it after the capture loop, before writing the PNG. The function should:

1. Collect all `capture_interval_ms` values from the report frames
2. Sort them and compute p50/p90/max
3. Scan report frames for max accepted `abs(dy)` (from `Appended` estimates)
4. Compute the longest consecutive `NoMatch` run
5. Print to stderr (respecting `--quiet`)
6. Print a warning if max accepted dy > viewport height / 2 (approximated from the stitcher) or if longest NoMatch run >= 5

Add this function after `write_report`:

```rust
fn print_diagnostics_summary(report: &CaptureMatchReport, quiet: bool) {
    if quiet {
        return;
    }

    let intervals: Vec<f64> = report
        .frames
        .iter()
        .filter_map(|f| f.capture_interval_ms)
        .collect();

    let (p50, p90, max_interval) = if intervals.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        let mut sorted = intervals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p50 = sorted[sorted.len() / 2];
        let p90 = sorted[(sorted.len() as f64 * 0.9) as usize];
        let max = sorted.last().copied().unwrap_or(0.0);
        (p50, p90, max)
    };

    let max_accepted_dy = report
        .frames
        .iter()
        .filter(|f| f.outcome == "Appended")
        .filter_map(|f| f.estimate.as_ref())
        .map(|e| e.dy.unsigned_abs())
        .max()
        .unwrap_or(0);

    let mut longest_no_match_run: u32 = 0;
    let mut current_run: u32 = 0;
    for frame in &report.frames {
        if frame.outcome == "NoMatch" {
            current_run += 1;
            longest_no_match_run = longest_no_match_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    eprintln!(
        "--- capture diagnostics ---\n\
         capture_interval_ms: p50={p50:.1} p90={p90:.1} max={max_interval:.1}\n\
         max_accepted_dy: {max_accepted_dy}\n\
         longest_no_match_run: {longest_no_match_run}"
    );

    if longest_no_match_run >= 5 {
        eprintln!(
            "warning: {longest_no_match_run} consecutive NoMatch frames — \
             scroll may be too fast for the capture cadence"
        );
    }
}
```

Call it in `run()` after the capture loop ends (after line 201, before `let stitched = ...`):

```rust
    print_diagnostics_summary(&report, args.quiet);
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-cli --test capture_fixture`
Expected: all tests PASS including `rollshot_capture_fixture_prints_diagnostics_summary` and `rollshot_capture_quiet_suppresses_diagnostics`

- [ ] **Step 7: Also add diagnostics to the JSON report**

Extend `CaptureMatchReport` with a summary section so `--debug-match-report` includes the same stats. Add a `summary` field:

```rust
#[derive(Debug, Serialize)]
struct CaptureMatchReport {
    frames: Vec<CaptureFrameReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<CaptureSummary>,
}

#[derive(Debug, Serialize)]
struct CaptureSummary {
    capture_interval_p50_ms: f64,
    capture_interval_p90_ms: f64,
    capture_interval_max_ms: f64,
    max_accepted_dy: u32,
    longest_no_match_run: u32,
    frames_under_20ms: usize,
    total_frames: usize,
    appended: u32,
    duplicates: u32,
    no_match: u32,
    no_progress: u32,
}
```

Populate the summary before writing the report. Extract the interval/motion/run computation into a shared helper `compute_summary` that both `print_diagnostics_summary` and the report serialization use.

- [ ] **Step 8: Run all tests**

Run: `rtk cargo test -p rollshot-cli`
Expected: all tests PASS

- [ ] **Step 9: Commit**

```bash
rtk git add crates/rollshot-cli/src/cmd_capture.rs crates/rollshot-cli/tests/capture_fixture.rs
git commit -m "feat(capture): add post-capture diagnostics summary to stderr and JSON report"
```

---

### Task 2: Consumer-Side Frame Pacing

Add a `--min-interval-ms` CLI flag (default 0 = disabled) that makes the capture loop skip frames whose timestamp is less than N ms after the last *processed* frame. This is the consumer-side equivalent of wayscrollshot's 45ms `CAPTURE_INTERVAL`. When enabled, frames that arrive too soon are counted as "skipped" and reported in diagnostics.

**Files:**
- Modify: `crates/rollshot-cli/src/args.rs:26-82`
- Modify: `crates/rollshot-cli/src/cmd_capture.rs:85-201`
- Test: `crates/rollshot-cli/tests/capture_fixture.rs`

- [ ] **Step 1: Add `--min-interval-ms` CLI arg**

In `crates/rollshot-cli/src/args.rs`, add to `CaptureArgs`:

```rust
    /// Minimum milliseconds between processed frames. Frames arriving
    /// sooner are skipped. Helps maintain stitchable overlap during fast
    /// scrolling. 0 disables pacing (default).
    #[arg(long, default_value_t = 0)]
    pub min_interval_ms: u64,
```

- [ ] **Step 2: Write the failing test — pacing skips frames**

The fixture backend delivers frames with zero interval. With `--min-interval-ms 1000` (1 second), only the first frame should be processed because subsequent frames arrive "instantly" (< 1000ms apart). The fixture has 4 frames, so captured=4 but the stitcher should only see 1.

However — the fixture backend timestamps frames at dequeue time via `SystemTime::now()`, and the stitch loop is synchronous. Adjacent frames will have timestamps only microseconds apart. So `--min-interval-ms 1000` should cause 3 frames to be skipped.

Add to `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
#[test]
fn rollshot_capture_pacing_skips_fast_frames() {
    let tempdir = temp_dir("pacing-skip");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--min-interval-ms", "1000"])
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(
        stdout.contains("captured 4 frames"),
        "all frames should be captured; stdout = {stdout}"
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("pacing_skipped"),
        "diagnostics should report pacing skips; stderr = {stderr}"
    );
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-cli --test capture_fixture rollshot_capture_pacing_skips_fast_frames`
Expected: FAIL — unknown arg `--min-interval-ms`

- [ ] **Step 4: Write the failing test — zero interval means no skipping**

```rust
#[test]
fn rollshot_capture_pacing_zero_means_no_skip() {
    let tempdir = temp_dir("pacing-zero");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--min-interval-ms", "0"])
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(
        stdout.contains("appended 3"),
        "all frames should stitch; stdout = {stdout}"
    );
}
```

- [ ] **Step 5: Implement frame pacing in the capture loop**

In `crates/rollshot-cli/src/cmd_capture.rs`, modify the `run()` function:

1. Add a `pacing_skipped` counter and a `last_processed_timestamp` tracker before the loop.
2. Inside the `Ok(frame) =>` arm, after recording `capture_interval_ms` and before calling `stitcher.push_frame()`, check if the frame should be skipped:

```rust
    let min_interval = Duration::from_millis(args.min_interval_ms);
    let mut pacing_skipped: u32 = 0;
    let mut last_processed_timestamp: Option<SystemTime> = None;

    loop {
        match stream.next_frame() {
            Ok(frame) => {
                let capture_interval_ms = previous_capture_timestamp.and_then(|previous| {
                    frame.timestamp.duration_since(previous).ok().map(duration_ms)
                });
                previous_capture_timestamp = Some(frame.timestamp);

                // Frame pacing: skip frames that arrive too soon
                if args.min_interval_ms > 0 {
                    if let Some(last_ts) = last_processed_timestamp {
                        if let Ok(elapsed) = frame.timestamp.duration_since(last_ts) {
                            if elapsed < min_interval {
                                pacing_skipped += 1;
                                captured += 1;
                                report.frames.push(CaptureFrameReport {
                                    frame_index: report.frames.len(),
                                    outcome: "PacingSkipped".to_string(),
                                    reason: None,
                                    estimate: None,
                                    capture_interval_ms,
                                    stitch_elapsed_ms: 0.0,
                                });
                                if captured >= args.max_frames {
                                    break;
                                }
                                continue;
                            }
                        }
                    }
                }

                // ... existing dump/stitch logic ...
                last_processed_timestamp = Some(frame.timestamp);
```

Set `last_processed_timestamp = Some(frame.timestamp)` after the stitch call (for every frame that is NOT pacing-skipped).

3. Add `pacing_skipped` to the `CaptureSummary` struct and include it in `print_diagnostics_summary`.

4. Include it in the final stdout summary line:
```rust
    Ok(format!(
        "captured {captured} frames, appended {appended} \
         (duplicates {duplicates}, no-progress {no_progress}, \
         no-match {no_match}, pacing-skipped {pacing_skipped})\n\
         output: {out} ({w}x{h})\n",
        ...
    ))
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-cli --test capture_fixture`
Expected: all tests PASS

- [ ] **Step 7: Run full test suite**

Run: `rtk cargo test --workspace`
Expected: all tests PASS

- [ ] **Step 8: Run clippy**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings

- [ ] **Step 9: Commit**

```bash
rtk git add crates/rollshot-cli/src/args.rs crates/rollshot-cli/src/cmd_capture.rs crates/rollshot-cli/tests/capture_fixture.rs
git commit -m "feat(capture): add --min-interval-ms consumer-side frame pacing"
```

---

### Task 3: Update Issue Status

- [ ] **Step 1: Update the issue document**

In `docs/issues/2026-05-23-fast-scroll-capture-sampling-gap.md`, update the status from `open` to `in-progress` and add a "Progress" section after "Proposed Resolution" noting that items 1 (diagnostics) and partial item 2 (consumer-side pacing) are implemented. Item 2's auto-scroll mode and items 3-4 remain open.

- [ ] **Step 2: Commit**

```bash
rtk git add docs/issues/2026-05-23-fast-scroll-capture-sampling-gap.md
git commit -m "docs(issues): update fast-scroll issue status to in-progress"
```
