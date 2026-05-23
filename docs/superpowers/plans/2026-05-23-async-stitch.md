# Async Stitch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple the capture consumer from the stitcher by introducing a reader thread that continuously drains PipeWire frames into a latest-wins FrameSlot, while the main thread stitches at its own pace (~15fps).

**Architecture:** Reader thread reads from `stream.next_frame()` in a tight loop and overwrites the latest frame in a `FrameSlot`. Main thread takes from the slot, stitches, and reports. Remove `--min-interval-ms` and `pacing_skipped` since stitch time naturally paces.

**Tech Stack:** Rust std::thread, Mutex, Condvar

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/rollshot-cli/src/frame_slot.rs` | Create | FrameSlot struct: store, take_blocking, signal_end/error |
| `crates/rollshot-cli/src/cmd_capture.rs` | Modify | Spawn reader thread, use FrameSlot, remove pacing logic |
| `crates/rollshot-cli/src/args.rs` | Modify | Remove `--min-interval-ms` |
| `crates/rollshot-cli/src/lib.rs` or `mod.rs` | Modify | Add `mod frame_slot;` |
| `crates/rollshot-cli/tests/capture_fixture.rs` | Modify | Remove pacing tests, update diagnostics tests |

---

### Task 1: FrameSlot

Create `crates/rollshot-cli/src/frame_slot.rs` with the FrameSlot type.

**Files:**
- Create: `crates/rollshot-cli/src/frame_slot.rs`
- Modify: `crates/rollshot-cli/src/main.rs` or `lib.rs` (add `mod frame_slot;`)

- [ ] **Step 1: Find the module root and add the module declaration**

Check whether the crate uses `main.rs` or `lib.rs` + `main.rs`. Add `mod frame_slot;` in the appropriate file.

- [ ] **Step 2: Create FrameSlot**

Create `crates/rollshot-cli/src/frame_slot.rs`:

```rust
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use rollshot_capture::types::{CapturedFrame, CaptureError};

struct SlotState {
    frame: Option<CapturedFrame>,
    total_produced: u32,
    end: bool,
    error: Option<String>,
}

pub struct FrameSlot {
    inner: Mutex<SlotState>,
    condvar: Condvar,
}

impl FrameSlot {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(SlotState {
                frame: None,
                total_produced: 0,
                end: false,
                error: None,
            }),
            condvar: Condvar::new(),
        }
    }

    pub fn store(&self, frame: CapturedFrame) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        state.frame = Some(frame);
        state.total_produced += 1;
        self.condvar.notify_one();
    }

    pub fn signal_end(&self) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        state.end = true;
        self.condvar.notify_one();
    }

    pub fn signal_error(&self, msg: String) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        state.error = Some(msg);
        self.condvar.notify_one();
    }

    pub fn take_blocking(&self, timeout: Duration) -> Result<CapturedFrame, CaptureError> {
        let state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        let (mut state, wait_result) = self
            .condvar
            .wait_timeout_while(state, timeout, |s| {
                s.frame.is_none() && !s.end && s.error.is_none()
            })
            .unwrap_or_else(PoisonError::into_inner);

        if let Some(frame) = state.frame.take() {
            return Ok(frame);
        }
        if let Some(msg) = state.error.take() {
            return Err(CaptureError::Backend(anyhow::anyhow!(msg)));
        }
        if state.end {
            return Err(CaptureError::EndOfStream);
        }
        if wait_result.timed_out() {
            return Err(CaptureError::Backend(anyhow::anyhow!(
                "no frame within {timeout:?}"
            )));
        }
        Err(CaptureError::EndOfStream)
    }

    pub fn total_produced(&self) -> u32 {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .total_produced
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `rtk cargo check -p rollshot-cli`

- [ ] **Step 4: Commit**

```
feat(cli): add FrameSlot for async stitch reader-stitcher decoupling
```

---

### Task 2: Async capture loop

Rewrite `cmd_capture.rs` to spawn a reader thread and use FrameSlot. Remove `--min-interval-ms` and all pacing logic.

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Modify: `crates/rollshot-cli/src/args.rs`

- [ ] **Step 1: Remove `--min-interval-ms` from args.rs**

Delete the `min_interval_ms` field from `CaptureArgs`.

- [ ] **Step 2: Rewrite the capture loop in cmd_capture.rs**

Major changes to `run()`:

1. Remove all pacing-related variables (`pacing_skipped`, `min_interval`, `last_processed_timestamp`, `SystemTime` import if unused).

2. Remove `PacingSkipped` from the report and diagnostics.

3. After creating `stream`, spawn a reader thread:

```rust
let slot = Arc::new(FrameSlot::new());
let slot_reader = Arc::clone(&slot);

let reader_handle = std::thread::spawn(move || {
    loop {
        match stream.next_frame() {
            Ok(frame) => slot_reader.store(frame),
            Err(CaptureError::EndOfStream) => {
                slot_reader.signal_end();
                break;
            }
            Err(err) => {
                slot_reader.signal_error(format!("{err}"));
                break;
            }
        }
    }
});
```

Note: `stream` must be moved into the reader thread. This means `stream` can no longer be used on the main thread. The `Box<dyn FrameStream>` must be `Send`. Check if `FrameStream` requires `Send` — if not, add `Send` bound.

4. Replace the main loop body:

```rust
loop {
    match slot.take_blocking(Duration::from_secs(5)) {
        Ok(frame) => {
            let capture_interval_ms = previous_capture_timestamp.and_then(|previous| {
                frame.timestamp.duration_since(previous).ok().map(duration_ms)
            });
            previous_capture_timestamp = Some(frame.timestamp);

            if let Some(dir) = args.dump_frames.as_ref() {
                write_dump_frame(dir, captured, &frame.image)?;
            }
            captured += 1;
            if !args.quiet {
                log_capture_start(captured, args.max_frames);
            }
            let stitch_started = Instant::now();
            let outcome = stitcher.push_frame(frame.image);
            let stitch_elapsed = stitch_started.elapsed();
            // ... same report/logging logic as before, minus pacing ...
            if captured >= args.max_frames {
                break;
            }
        }
        Err(CaptureError::EndOfStream) => break,
        Err(err) => return Err(CliError::from_capture(err)),
    }
}
```

5. After the loop, join the reader thread (ignore its result — it may have exited already):

```rust
drop(slot); // release Arc so reader can detect if needed
let _ = reader_handle.join();
```

6. Update `CaptureSummary`: replace `pacing_skipped` with `frames_read: u32` (from `slot.total_produced()`). Update `compute_summary` and `print_diagnostics_summary` accordingly.

7. Update the stdout summary format to show `frames_read` instead of `pacing-skipped`.

- [ ] **Step 3: Check FrameStream is Send**

`Box<dyn FrameStream>` must be moved into the reader thread. Check the `FrameStream` trait. If it's not `Send`, either:
- Add `Send` bound to the trait: `pub trait FrameStream: Send { ... }`
- Or wrap the stream in a way that makes it sendable

The `LinuxPortalFrameStream` contains `PipeWireConnection` which has PipeWire types. Check if these are `Send`. If not, the reader thread needs to be spawned before `stream` is created, or the `FrameStream` trait needs adjustment.

If `FrameStream` is NOT Send, an alternative approach: keep the reader on the main thread and spawn the stitcher on a separate thread. The stitcher only needs `Stitcher` (which is `Send`) and the `FrameSlot`.

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `rtk cargo test -p rollshot-cli`
Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

- [ ] **Step 5: Commit**

```
feat(capture): async stitch with reader thread and FrameSlot

Replace the synchronous next_frame → stitch loop with a reader
thread that continuously drains PipeWire into a latest-wins FrameSlot.
The main thread stitches at its own pace, always getting the freshest
frame. Remove --min-interval-ms (stitch time naturally paces).
```

---

### Task 3: Update tests

Remove pacing-specific tests, update diagnostics tests for the new summary format.

**Files:**
- Modify: `crates/rollshot-cli/tests/capture_fixture.rs`

- [ ] **Step 1: Remove pacing tests**

Delete `rollshot_capture_pacing_skips_fast_frames` and `rollshot_capture_pacing_zero_means_no_skip`.

- [ ] **Step 2: Update diagnostics test**

The `rollshot_capture_fixture_prints_diagnostics_summary` test should still pass — it checks for `capture_interval_ms`, `max_accepted_dy`, `longest_no_match_run` in stderr. These are still printed.

If the test checks for `pacing_skipped`, update it to check for `frames_read` instead.

- [ ] **Step 3: Update stdout assertions**

Tests that check stdout for `pacing-skipped` need updating. The new format includes `frames-read N` instead.

Tests that check `captured N frames` may need updating if the semantics changed (captured now means stitched, not read from stream).

For the fixture backend, the reader thread reads all 4 frames. The stitcher processes all 4 (FirstFrame + 3 Appended). So `captured 4 frames` should still work. `frames-read 4` is also 4 for fixture (no frames are discarded since stitch is fast for small test images).

- [ ] **Step 4: Run full test suite**

Run: `rtk cargo test --workspace`
Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

- [ ] **Step 5: Commit**

```
test(cli): update capture tests for async stitch
```
