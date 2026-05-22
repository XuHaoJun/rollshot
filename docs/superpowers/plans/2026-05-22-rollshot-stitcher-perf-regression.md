# Rollshot Stitcher Perf Regression Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the LinearScroll v2 capture-time performance regression so real-resolution capture frames do not look hung, and add regression coverage that is stable on GitHub-hosted CI.

**Architecture:** Keep `rollshot-core` responsible for platform-independent motion estimation, but change the matcher from exhaustive full-resolution sweeps to a bounded coarse-to-fine pipeline. Add default-on `rollshot capture` progress output on stderr so a slow frame is visible immediately, use structural matcher budget tests as the stable CI gate, and keep wall-clock perf smoke as a manual/release check because GitHub-hosted runner timing varies.

**Tech Stack:** Rust 2021, `image` 0.25, `rayon` 1.10 in `rollshot-core`, existing `clap` CLI, GitHub Actions `ubuntu-24.04`/`macos-14`.

---

## Assumptions

- `capture` progress is default-on and writes to stderr. The final summary remains stdout.
- `--quiet` disables progress output for scripts that need clean stderr.
- The stable gate is algorithmic: count searched offsets, full-resolution NCC calls, and approximate full-resolution NCC pixel visits through a test-only instrumentation entrypoint. This is deterministic across machines and keeps production `estimate_motion` free of counter writes.
- The wall-clock smoke test is release-mode, ignored by default, and runnable in a manual workflow with a generous threshold. It is not part of ordinary `cargo test --workspace`.
- The matcher still returns `AxisChanged` when a locked vertical sequence sees reliable horizontal movement, but it no longer performs a full 2D exhaustive search to do so.
- AKAZE behavior remains a fallback after non-AKAZE candidates fail. This plan does not change AKAZE internals.

## File Structure

- Modify: `Cargo.toml`
  Add workspace dependency `rayon = "1.10"`.
- Modify: `crates/rollshot-core/Cargo.toml`
  Add `rayon = { workspace = true }`.
- Modify: `crates/rollshot-core/src/matcher.rs`
  Add test-only matcher search-budget instrumentation, large-frame tests, bounded coarse-to-fine search, and rayon parallel scoring. Keep the public crate API unchanged.
- Modify: `crates/rollshot-core/src/types.rs`
  Keep default config compatible, but tighten `max_search_ratio` only after coarse-to-fine tests pass.
- Modify: `crates/rollshot-cli/src/args.rs`
  Add `CaptureArgs::quiet`.
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
  Add default-on progress output with per-frame timing.
- Modify: `crates/rollshot-cli/tests/capture_fixture.rs`
  Cover default progress and `--quiet`.
- Modify: `README.md`
  Document progress output, `--quiet`, release-mode macOS capture guidance, and perf smoke commands.
- Create: `.github/workflows/matcher-perf.yml`
  Add a manual `workflow_dispatch` release-mode perf smoke on `ubuntu-24.04`.

---

### Task 1: Default-On Capture Progress Output

**Files:**
- Modify: `crates/rollshot-cli/src/args.rs`
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Modify: `crates/rollshot-cli/tests/capture_fixture.rs`
- Modify: `README.md`

- [ ] **Step 1: Add failing tests for stderr progress and quiet mode**

Append these tests to `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
#[test]
fn rollshot_capture_prints_default_progress_to_stderr() {
    let tempdir = temp_dir("progress-stderr");
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
        .args(["--max-frames", "2"])
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("captured 2 frames"), "stdout = {stdout}");

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("frame 1/2: stitching..."), "stderr = {stderr}");
    assert!(stderr.contains("FirstFrame"), "stderr = {stderr}");
    assert!(stderr.contains("frame 2/2: stitching..."), "stderr = {stderr}");
    assert!(stderr.contains("elapsed="), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_quiet_suppresses_progress_stderr() {
    let tempdir = temp_dir("progress-quiet");
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
        .args(["--max-frames", "2"])
        .arg("--quiet")
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert_eq!(stderr, "");

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run the new tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-cli rollshot_capture_prints_default_progress_to_stderr
rtk cargo test -p rollshot-cli rollshot_capture_quiet_suppresses_progress_stderr
```

Expected: the first test fails because stderr is empty; the second test fails because `--quiet` is unknown.

- [ ] **Step 3: Add `--quiet` to capture args**

In `crates/rollshot-cli/src/args.rs`, add this field to `CaptureArgs` after `show_cursor`:

```rust
    /// Suppress per-frame capture progress on stderr.
    #[arg(long, default_value_t = false)]
    pub quiet: bool,
```

- [ ] **Step 4: Add progress formatting in `cmd_capture.rs`**

In `crates/rollshot-cli/src/cmd_capture.rs`, add this import:

```rust
use std::time::{Duration, Instant};
```

Add these helper functions below `write_dump_frame`:

```rust
fn log_capture_start(index: u32, max_frames: u32) {
    eprintln!("frame {index}/{max_frames}: stitching...");
}

fn log_capture_progress(index: u32, max_frames: u32, outcome: &StitchOutcome, elapsed: Duration) {
    eprintln!(
        "frame {}/{}: {} elapsed={:.3}s",
        index,
        max_frames,
        outcome_label(outcome),
        elapsed.as_secs_f64()
    );
}

fn outcome_label(outcome: &StitchOutcome) -> &'static str {
    match outcome {
        StitchOutcome::FirstFrame => "FirstFrame",
        StitchOutcome::Appended { .. } => "Appended",
        StitchOutcome::Duplicate => "Duplicate",
        StitchOutcome::NoMatch { .. } => "NoMatch",
        StitchOutcome::NoProgress { .. } => "NoProgress",
        StitchOutcome::AxisChanged { .. } => "AxisChanged",
    }
}
```

Then replace the existing `match stitcher.push_frame(frame.image) { ... }` block with:

```rust
                if !args.quiet {
                    log_capture_start(captured, args.max_frames);
                }
                let stitch_started = Instant::now();
                let outcome = stitcher.push_frame(frame.image);
                let stitch_elapsed = stitch_started.elapsed();
                match &outcome {
                    StitchOutcome::FirstFrame => {}
                    StitchOutcome::Appended { .. } => appended += 1,
                    StitchOutcome::Duplicate => duplicates += 1,
                    StitchOutcome::NoMatch { .. } => no_match += 1,
                    StitchOutcome::NoProgress { .. } => no_progress += 1,
                    StitchOutcome::AxisChanged { .. } => no_match += 1,
                }
                if !args.quiet {
                    log_capture_progress(captured, args.max_frames, &outcome, stitch_elapsed);
                }
```

- [ ] **Step 5: Run the progress tests**

Run:

```bash
rtk cargo test -p rollshot-cli rollshot_capture_prints_default_progress_to_stderr
rtk cargo test -p rollshot-cli rollshot_capture_quiet_suppresses_progress_stderr
```

Expected: both pass.

- [ ] **Step 6: Update README capture usage**

In `README.md`, add this paragraph after the useful smoke commands:

```markdown
`rollshot capture` prints per-frame progress to stderr by default:
`frame N/MAX: OUTCOME elapsed=SECONDS`. The final capture summary and output
path remain on stdout. Pass `--quiet` to suppress progress output when stderr
must stay empty for scripts.
```

- [ ] **Step 7: Commit**

Run:

```bash
rtk git add crates/rollshot-cli/src/args.rs crates/rollshot-cli/src/cmd_capture.rs crates/rollshot-cli/tests/capture_fixture.rs README.md
rtk git commit -m "feat(cli): show capture progress by default"
```

---

### Task 2: Add Matcher Search-Budget Instrumentation And Failing Large-Frame Gate

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

This task starts the matcher red-to-green group. Do not commit after Task 2 or
Task 3; the group becomes independently shippable at the end of Task 4, after
the structural budget test passes.

- [ ] **Step 1: Add the failing budget test**

Inside `#[cfg(test)] mod tests` in `crates/rollshot-core/src/matcher.rs`, add this test:

```rust
    #[test]
    fn large_pair_stays_within_structural_search_budget() {
        let canvas = make_textured_canvas(1470, 900);
        let prev = crop(&canvas, 0, 660);
        let curr = crop(&canvas, 110, 660);
        let config = StitchConfig::default();

        let mut budget = SearchBudget::default();
        let candidate = unwrap_candidate(estimate_motion_with_budget(
            &prev,
            &curr,
            None,
            (0, 0),
            &config,
            &mut budget,
        ));

        assert_eq!(candidate.dx, 0);
        assert!(
            (candidate.dy - 110).abs() <= 3,
            "dy = {} (expected ~110)",
            candidate.dy
        );
        assert!(
            budget.coarse_score_calls <= 4096,
            "coarse_score_calls = {}",
            budget.coarse_score_calls
        );
        assert!(
            budget.full_res_ncc_calls <= 192,
            "full_res_ncc_calls = {}",
            budget.full_res_ncc_calls
        );
        assert!(
            budget.full_res_ncc_pixel_visits <= 60_000_000,
            "full_res_ncc_pixel_visits = {}",
            budget.full_res_ncc_pixel_visits
        );
    }
```

- [ ] **Step 2: Run the test and verify it fails to compile**

Run:

```bash
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget
```

Expected: compile fails because `SearchBudget` and `estimate_motion_with_budget` do not exist.

- [ ] **Step 3: Add test-only `SearchBudget` and wrapper entry point**

Near `MotionSearchOutcome` in `crates/rollshot-core/src/matcher.rs`, add:

```rust
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SearchBudget {
    coarse_score_calls: u64,
    full_res_ncc_calls: u64,
    full_res_ncc_pixel_visits: u64,
    verifier_calls: u64,
}

#[cfg(test)]
static ACTIVE_SEARCH_BUDGET: std::sync::Mutex<Option<SearchBudget>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn estimate_motion_with_budget(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    budget: &mut SearchBudget,
) -> MotionSearchOutcome {
    let result =
        with_search_budget(|| estimate_motion(prev, curr, locked_axis, last_motion, config));
    *budget = take_search_budget();
    result
}

#[cfg(test)]
fn with_search_budget<R>(f: impl FnOnce() -> R) -> R {
    {
        let mut active = ACTIVE_SEARCH_BUDGET.lock().expect("search budget mutex poisoned");
        assert!(active.is_none(), "nested search budgets are not supported");
        *active = Some(SearchBudget::default());
    }
    f()
}

#[cfg(test)]
fn take_search_budget() -> SearchBudget {
    ACTIVE_SEARCH_BUDGET
        .lock()
        .expect("search budget mutex poisoned")
        .take()
        .unwrap_or_default()
}

#[cfg(test)]
fn with_active_search_budget(f: impl FnOnce(&mut SearchBudget)) {
    let mut active = ACTIVE_SEARCH_BUDGET
        .lock()
        .expect("search budget mutex poisoned");
    if let Some(budget) = active.as_mut() {
        f(budget);
    }
}
```

- [ ] **Step 4: Count current coarse, NCC, and verifier work in test builds only**

Before each `coarse_mad` call in `coarse_candidates`, add:

```rust
            #[cfg(test)]
            with_active_search_budget(|budget| budget.coarse_score_calls += 1);
```

After `x1/y1` are computed in `ncc_score_shifted` and before the sum loops, add:

```rust
    #[cfg(test)]
    with_active_search_budget(|budget| {
        budget.full_res_ncc_calls += 1;
        budget.full_res_ncc_pixel_visits += u64::from(x1 - x0) * u64::from(y1 - y0) * 2;
    });
```

Before `verifier.verify(prev, curr, &candidate)` in `rank_verified_candidates`, add:

```rust
        #[cfg(test)]
        with_active_search_budget(|budget| budget.verifier_calls += 1);
```

Do not change production helper signatures in this step. `estimate_motion`,
`coarse_candidates`, `template_candidates`, `search_template_axis`,
`rank_verified_candidates`, and `ncc_score_shifted` keep their existing
non-budget parameters.

- [ ] **Step 5: Run the budget test and verify it fails by assertion**

Run:

```bash
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget -- --nocapture
```

Expected: compile passes, but the test fails because current exhaustive search exceeds at least one budget threshold.

- [ ] **Step 6: Leave the red test uncommitted**

Run:

```bash
rtk git diff -- crates/rollshot-core/src/matcher.rs
```

Expected: diff shows the new budget test and instrumentation. Do not commit
yet; Task 3 and Task 4 complete the green implementation.

---

### Task 3: Replace Exhaustive Coarse Search With Axis-Separated Coarse Seeds

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

This task is the second phase of the Task 2-4 red-to-green group. Do not commit
after this task; the large budget test is allowed to remain red until Task 4.

- [ ] **Step 1: Add helper tests for axis-separated coarse offsets**

Inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn coarse_axis_offsets_are_bounded_for_large_frames() {
        let offsets = coarse_axis_offsets(2205, 0, 32);
        assert_eq!(offsets.first().copied(), Some(0));
        assert!(offsets.contains(&2205));
        assert!(offsets.contains(&-2205));
        assert!(
            offsets.len() <= 141,
            "offset count should stay bounded, got {}",
            offsets.len()
        );
    }
```

- [ ] **Step 2: Run the helper test and verify it fails to compile**

Run:

```bash
rtk cargo test -p rollshot-core coarse_axis_offsets_are_bounded_for_large_frames
```

Expected: compile fails because `coarse_axis_offsets` does not exist.

- [ ] **Step 3: Add bounded offset helper**

Add this constant and helper near `signed_predict_iter`:

```rust
const COARSE_AXIS_STRIDE: i32 = 8;

fn coarse_axis_offsets(max_abs: i32, predict: i32, step: i32) -> Vec<i32> {
    let max_abs = max_abs.max(0);
    let step = step.max(1);
    let predict = predict.clamp(-max_abs, max_abs);
    let mut out = Vec::new();
    out.push(predict);

    let mut delta = step;
    while delta <= max_abs {
        if predict + delta <= max_abs {
            out.push(predict + delta);
        }
        if predict - delta >= -max_abs {
            out.push(predict - delta);
        }
        delta += step;
    }

    if !out.contains(&max_abs) {
        out.push(max_abs);
    }
    if max_abs != 0 && !out.contains(&-max_abs) {
        out.push(-max_abs);
    }

    out
}
```

- [ ] **Step 4: Replace 2D coarse candidate generation**

Replace `coarse_candidates` with:

```rust
fn coarse_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let step = COARSE_DOWNSAMPLE_STEP as i32;
    let (sample_w, sample_h) = coarse_sample_dimensions(width, height, COARSE_DOWNSAMPLE_STEP);
    let prev_samples = coarse_samples(prev_gray, width, height, COARSE_DOWNSAMPLE_STEP);
    let curr_samples = coarse_samples(curr_gray, width, height, COARSE_DOWNSAMPLE_STEP);
    let max_dx = ((width as f32 * config.max_search_ratio) as i32 / step).max(0);
    let max_dy = ((height as f32 * config.max_search_ratio) as i32 / step).max(0);

    let mut out = Vec::new();
    for axis in search_axes(locked_axis) {
        let max_offset = match axis {
            SearchAxis::Vertical => max_dy,
            SearchAxis::Horizontal => max_dx,
        };
        if let Some(candidate) = coarse_axis_candidate(
            &prev_samples,
            &curr_samples,
            sample_w,
            sample_h,
            *axis,
            max_offset,
            0,
        ) {
            out.push(candidate);
        }
    }

    out.into_iter()
        .map(|mut candidate| {
            candidate.dx *= step;
            candidate.dy *= step;
            candidate
        })
        .filter(|candidate| candidate_matches_axis(candidate.dx, candidate.dy, locked_axis, config))
        .collect()
}
```

Add this helper below `coarse_candidates`:

```rust
fn coarse_axis_candidate(
    prev_samples: &[f32],
    curr_samples: &[f32],
    sample_w: u32,
    sample_h: u32,
    axis: SearchAxis,
    max_offset: i32,
    predicted: i32,
) -> Option<MotionCandidate> {
    let mut scored = Vec::new();
    for offset in coarse_axis_offsets(max_offset, predicted, COARSE_AXIS_STRIDE) {
        if offset == 0 {
            continue;
        }
        let (dx, dy) = match axis {
            SearchAxis::Vertical => (0, offset),
            SearchAxis::Horizontal => (offset, 0),
        };
        #[cfg(test)]
        with_active_search_budget(|budget| budget.coarse_score_calls += 1);
        let diff = coarse_mad(prev_samples, curr_samples, sample_w, sample_h, dx, dy, 1);
        if diff.is_finite() {
            scored.push((diff, dx, dy));
        }
    }

    scored.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    let (best_score, best_dx, best_dy) = *scored.first()?;
    let second = scored.get(1).map(|(score, _, _)| *score);
    Some(candidate(best_dx, best_dy, MatchMethod::Coarse, best_score, second))
}
```

- [ ] **Step 5: Run focused matcher tests**

Run:

```bash
rtk cargo test -p rollshot-core matcher:: -- --nocapture
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget -- --nocapture
```

Expected: matcher behavior tests still pass. The large budget test may still fail on full-resolution NCC calls; coarse call count should now be under threshold.

- [ ] **Step 6: Leave the partial perf fix uncommitted**

Run:

```bash
rtk git diff -- crates/rollshot-core/src/matcher.rs
```

Expected: diff shows bounded coarse search. Do not commit yet; Task 4 completes
the green implementation and commits the whole matcher budget fix.

---

### Task 4: Replace Global Template Sweep With Coarse-Seeded Full-Resolution Refinement

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add refinement window tests**

Inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn refinement_offsets_stay_near_seed() {
        let radius = template_refine_radius(&StitchConfig::default());
        assert!(
            radius >= COARSE_DOWNSAMPLE_STEP as i32 * COARSE_AXIS_STRIDE,
            "radius = {radius}"
        );

        let offsets = refinement_offsets(220, 990, radius);
        assert_eq!(offsets.first().copied(), Some(220));
        assert!(offsets.contains(&(220 - radius)));
        assert!(offsets.contains(&(220 + radius)));
        assert!(!offsets.contains(&0));
        assert!(
            offsets.len() <= (radius * 2 + 1) as usize,
            "len = {}",
            offsets.len()
        );
    }
```

- [ ] **Step 2: Run the refinement test and verify it fails to compile**

Run:

```bash
rtk cargo test -p rollshot-core refinement_offsets_stay_near_seed
```

Expected: compile fails because `refinement_offsets` does not exist.

- [ ] **Step 3: Add refinement helper**

Add this helper near `coarse_axis_offsets`:

```rust
fn refinement_offsets(seed: i32, max_abs: i32, radius: i32) -> Vec<i32> {
    let seed = seed.clamp(-max_abs, max_abs);
    let radius = radius.max(0);
    let start = (seed - radius).max(-max_abs);
    let end = (seed + radius).min(max_abs);
    let mut out = Vec::with_capacity((end - start + 1).max(0) as usize);
    out.push(seed);
    for delta in 1..=radius {
        if seed + delta <= end {
            out.push(seed + delta);
        }
        if seed - delta >= start {
            out.push(seed - delta);
        }
    }
    out
}
```

- [ ] **Step 4: Thread coarse seeds into template candidates**

In `estimate_motion`, split coarse candidates into a variable:

```rust
    let coarse = coarse_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        config,
    );
    candidates.extend(coarse.iter().copied());
    candidates.extend(template_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        last_motion,
        &coarse,
        config,
    ));
```

Update `template_candidates` signature:

```rust
fn template_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
```

Inside `template_candidates`, replace `predicted_offset(*axis, last_motion)` with:

```rust
            template_seed(*axis, last_motion, coarse),
```

Add this helper:

```rust
fn template_seed(axis: SearchAxis, last_motion: (i32, i32), coarse: &[MotionCandidate]) -> i32 {
    coarse
        .iter()
        .find_map(|candidate| match axis {
            SearchAxis::Vertical if candidate.dx == 0 => Some(candidate.dy),
            SearchAxis::Horizontal if candidate.dy == 0 => Some(candidate.dx),
            _ => None,
        })
        .unwrap_or_else(|| predicted_offset(axis, last_motion))
}
```

- [ ] **Step 5: Make `search_template_axis` use refinement offsets only**

In `search_template_axis`, replace:

```rust
    for offset in signed_predict_iter(max_offset, last_offset) {
```

Add this helper in `matcher.rs`:

```rust
fn template_refine_radius(_config: &StitchConfig) -> i32 {
    COARSE_DOWNSAMPLE_STEP as i32 * COARSE_AXIS_STRIDE + 8
}
```

Then use:

```rust
    for offset in refinement_offsets(last_offset, max_offset, template_refine_radius(config)) {
```

- [ ] **Step 6: Run matcher and budget tests**

Run:

```bash
rtk cargo test -p rollshot-core matcher:: -- --nocapture
rtk cargo test -p rollshot-core --test stitcher horizontal_after_vertical_lock_is_rejected_as_axis_change -- --nocapture
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget -- --nocapture
```

Expected: all matcher tests pass, locked-axis opposite-axis probing still returns
`AxisChanged`, and the large-frame structural budget test passes.

- [ ] **Step 7: Run core tests**

Run:

```bash
rtk cargo test -p rollshot-core
```

Expected: all pass, with the existing ignored test still ignored.

- [ ] **Step 8: Commit**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs
rtk git commit -m "perf(core): bound matcher search budget"
```

---

### Task 5: Parallelize Bounded Scoring With Rayon

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-core/Cargo.toml`
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add rayon dependencies**

In workspace `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
rayon = "1.10"
```

In `crates/rollshot-core/Cargo.toml`, add to `[dependencies]`:

```toml
rayon = { workspace = true }
```

- [ ] **Step 2: Run check and verify rayon is available**

Run:

```bash
rtk cargo check -p rollshot-core
```

Expected: pass.

- [ ] **Step 3: Import rayon traits**

At the top of `crates/rollshot-core/src/matcher.rs`, add:

```rust
use rayon::prelude::*;
```

- [ ] **Step 4: Make coarse scoring parallel and deterministic**

In `coarse_axis_candidate`, replace the scoring loop with:

```rust
    let offsets: Vec<i32> = coarse_axis_offsets(max_offset, predicted, COARSE_AXIS_STRIDE)
        .into_iter()
        .filter(|offset| *offset != 0)
        .collect();
    #[cfg(test)]
    with_active_search_budget(|budget| budget.coarse_score_calls += offsets.len() as u64);

    let mut scored: Vec<_> = offsets
        .into_par_iter()
        .filter_map(|offset| {
            let (dx, dy) = match axis {
                SearchAxis::Vertical => (0, offset),
                SearchAxis::Horizontal => (offset, 0),
            };
            let diff = coarse_mad(prev_samples, curr_samples, sample_w, sample_h, dx, dy, 1);
            diff.is_finite().then_some((diff, dx, dy))
        })
        .collect();
```

Keep the existing deterministic sort after collection.

- [ ] **Step 5: Make template scoring parallel with local budget accumulation**

In `search_template_axis`, replace the mutable best loop with:

```rust
    let offsets = refinement_offsets(last_offset, max_offset, template_refine_radius(config));
    let scored: Vec<_> = offsets
        .into_par_iter()
        .filter_map(|offset| {
            let score = match axis {
                SearchAxis::Vertical => ncc_score_shifted(
                    prev_gray,
                    curr_gray,
                    width,
                    height,
                    region,
                    0,
                    offset,
                ),
                SearchAxis::Horizontal => ncc_score_shifted(
                    prev_gray,
                    curr_gray,
                    width,
                    height,
                    region,
                    offset,
                    0,
                ),
            };
            score.is_finite().then_some((score, offset))
        })
        .collect();

    let mut scored: Vec<(f32, i32)> = scored
        .into_iter()
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let (best_score, best_offset) = match scored.first().copied() {
        Some(best) => best,
        None => return None,
    };
    let second_score = scored.get(1).map(|(score, _)| *score).unwrap_or(f32::MIN);
```

Remove the old `best_offset`, `best_score`, `second_score` loop variables.

- [ ] **Step 6: Run matcher tests**

Run:

```bash
rtk cargo test -p rollshot-core matcher:: -- --nocapture
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget -- --nocapture
```

Expected: all pass. Budget counters should match or improve compared with Task 4.

- [ ] **Step 7: Run core tests and clippy**

Run:

```bash
rtk cargo test -p rollshot-core
rtk cargo clippy -p rollshot-core --all-targets -- -D warnings
```

Expected: both pass.

- [ ] **Step 8: Commit**

Run:

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-core/Cargo.toml crates/rollshot-core/src/matcher.rs
rtk git commit -m "perf(core): parallelize bounded matcher scoring"
```

---

### Task 6: Add Manual Release Perf Smoke And Workflow

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`
- Create: `.github/workflows/matcher-perf.yml`
- Modify: `README.md`

- [ ] **Step 1: Add ignored wall-clock smoke test**

Inside `#[cfg(test)] mod tests` in `crates/rollshot-core/src/matcher.rs`, add:

```rust
    #[test]
    #[ignore = "release-mode perf smoke; run manually with --ignored --nocapture"]
    fn large_retina_pair_perf_smoke() {
        let canvas = make_textured_canvas(2940, 1800);
        let prev = crop(&canvas, 0, 1320);
        let curr = crop(&canvas, 220, 1320);
        let config = StitchConfig::default();

        let started = std::time::Instant::now();
        let outcome = estimate_motion(&prev, &curr, None, (0, 0), &config);
        let elapsed = started.elapsed();
        let candidate = unwrap_candidate(outcome);

        let mut budget = SearchBudget::default();
        let budget_candidate = unwrap_candidate(estimate_motion_with_budget(
            &prev,
            &curr,
            None,
            (0, 0),
            &config,
            &mut budget,
        ));

        println!(
            "large_retina_pair_perf_smoke: elapsed={:.3}s parallelism={} candidate={:?} budget_candidate={:?} budget={:?}",
            elapsed.as_secs_f64(),
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            candidate,
            budget_candidate,
            budget
        );

        assert_eq!(candidate.dx, 0);
        assert_eq!(budget_candidate.dx, candidate.dx);
        assert_eq!(budget_candidate.dy, candidate.dy);
        assert!(
            (candidate.dy - 220).abs() <= 3,
            "dy = {} (expected ~220)",
            candidate.dy
        );

        if std::env::var_os("ROLLSHOT_PERF_STRICT").is_some() {
            assert!(
                elapsed.as_secs_f64() < 1.0,
                "release perf smoke exceeded 1.0s: elapsed={elapsed:?}, budget={budget:?}"
            );
        }
    }
```

- [ ] **Step 2: Run the ignored smoke manually without strict threshold**

Run:

```bash
rtk cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```

Expected: pass and print elapsed, available parallelism, candidate, and budget.

- [ ] **Step 3: Run the ignored smoke with strict threshold**

Run:

```bash
rtk env ROLLSHOT_PERF_STRICT=1 cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```

Expected: pass on the local machine if it is roughly comparable to GitHub-hosted `ubuntu-24.04`; if it fails locally but elapsed is close, do not loosen the structural budget test. Instead set the manual workflow threshold after a GitHub run records actual hosted-runner elapsed.

- [ ] **Step 4: Add manual matcher perf workflow**

Create `.github/workflows/matcher-perf.yml`:

```yaml
name: Matcher Perf Smoke

on:
  workflow_dispatch:

jobs:
  matcher-perf:
    name: Matcher release perf smoke
    runs-on: ubuntu-24.04
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Run strict release perf smoke
        env:
          ROLLSHOT_PERF_STRICT: "1"
        run: cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```

- [ ] **Step 5: Update README perf documentation**

Add this section before `## GitHub Actions`:

```markdown
## Matcher Performance Checks

The ordinary test suite includes a structural matcher budget test for a
retina-sized synthetic frame pair. It checks searched offsets and
full-resolution NCC work instead of wall-clock time, so it is stable across
developer machines and GitHub-hosted runners.

For a release-mode wall-clock smoke check, run:

```bash
cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```

To enforce the current hosted-runner threshold locally or in the manual
`Matcher Perf Smoke` GitHub workflow, set:

```bash
rtk env ROLLSHOT_PERF_STRICT=1 cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```
```

Also update `## GitHub Actions` with:

```markdown
`.github/workflows/matcher-perf.yml` is manual-only and runs the release-mode
large-frame matcher smoke on `ubuntu-24.04`. It complements the deterministic
structural budget test in the normal suite.
```

- [ ] **Step 6: Commit**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs .github/workflows/matcher-perf.yml README.md
rtk git commit -m "test(core): add manual matcher perf smoke"
```

---

### Task 7: Tune Defaults After The Algorithmic Fix

**Files:**
- Modify: `crates/rollshot-core/src/types.rs`
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add a default search ratio assertion**

In `crates/rollshot-core/src/types.rs`, update `default_config_picks_auto_hybrid` with:

```rust
        assert_eq!(cfg.max_search_ratio, 0.4);
```

- [ ] **Step 2: Run the config test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-core default_config_picks_auto_hybrid
```

Expected: fails because the current default is `0.75`.

- [ ] **Step 3: Tighten default `max_search_ratio`**

In `StitchConfig::default`, change:

```rust
            max_search_ratio: 0.75,
```

to:

```rust
            max_search_ratio: 0.4,
```

- [ ] **Step 4: Run direction, golden fixture, and budget tests**

Run:

```bash
rtk cargo test -p rollshot-core matcher:: -- --nocapture
rtk cargo test -p rollshot-core --test golden_fixtures
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget -- --nocapture
```

Expected: all pass. If a fixture with large motion fails, keep `0.4` and adjust only that test's local `StitchConfig` if the fixture intentionally needs an unusually large search.

- [ ] **Step 5: Commit**

Run:

```bash
rtk git add crates/rollshot-core/src/types.rs crates/rollshot-core/src/matcher.rs
rtk git commit -m "perf(core): tighten default matcher search window"
```

---

### Task 8: Final Verification

**Files:**
- Read-only verification across the workspace.

- [ ] **Step 1: Format check**

Run:

```bash
rtk cargo fmt --all -- --check
```

Expected: pass.

- [ ] **Step 2: Workspace tests**

Run:

```bash
rtk cargo test --workspace
```

Expected: pass, with the release perf smoke ignored unless explicitly selected.

- [ ] **Step 3: AKAZE feature tests**

Run:

```bash
rtk cargo test --workspace --features akaze
```

Expected: pass.

- [ ] **Step 4: Workspace clippy**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: pass.

- [ ] **Step 5: Release perf smoke**

Run:

```bash
rtk cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```

Expected: pass and print elapsed, available parallelism, candidate, and budget.

- [ ] **Step 6: CLI fixture smoke with visible progress**

Run:

```bash
rtk cargo run -p rollshot-cli -- capture --backend fixture --fixture crates/rollshot-core/tests/fixtures/linearscroll_v2/linear_vertical_down/frames --max-frames 3 --output target/test-artifacts/fixture-progress.png
```

Expected: stderr prints `frame 1/3`, `frame 2/3`, and `frame 3/3`; stdout prints the final captured/appended summary and output path.

- [ ] **Step 7: CLI fixture smoke with quiet mode**

Run:

```bash
rtk cargo run -p rollshot-cli -- capture --backend fixture --fixture crates/rollshot-core/tests/fixtures/linearscroll_v2/linear_vertical_down/frames --max-frames 3 --quiet --output target/test-artifacts/fixture-quiet.png
```

Expected: no per-frame progress on stderr; stdout still prints the final summary.

- [ ] **Step 8: Commit verification-only fixes if needed**

If formatting, clippy, or docs wording required small corrections, commit them:

```bash
rtk git add Cargo.toml Cargo.lock README.md .github/workflows/matcher-perf.yml crates/rollshot-core crates/rollshot-cli
rtk git commit -m "chore: finalize stitcher perf regression fix"
```

If no files changed during verification, do not create an empty commit.

---

## NOT In Scope

- Capture backend queue, dropped-frame, and backpressure instrumentation:
  this plan fixes the matcher CPU regression and adds per-frame stitch
  visibility, but it does not redesign capture buffering or report backend
  frame drops.
- New capture backends or platform permission flows: the regression is inside
  `rollshot-core` matching and CLI feedback, not backend startup.
- User-selectable matcher strategies: `AutoHybrid` remains internal per the
  LinearScroll v2 design.

---

## Self-Review Checklist

- The plan skips a separate spec by user approval and records the key design decisions here.
- CLI progress is default-on, stderr-only, and suppressible with `--quiet`.
- The stable CI gate is structural, not wall-clock based.
- The wall-clock test is ignored by default and strict only when `ROLLSHOT_PERF_STRICT=1`.
- The matcher fix reduces algorithmic work before adding rayon.
- README updates are included.
- Capture backend queue/drop/backpressure instrumentation is explicitly deferred.
- Every shell command is prefixed with `rtk`.
