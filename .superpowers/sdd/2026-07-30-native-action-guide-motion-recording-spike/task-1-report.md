# Task 1 Report: Deterministic Desktop Workload and Strict CLI

## Status: COMPLETE

## Files Created
- `spikes/action-guide-live-ffmpeg/Cargo.toml` — isolated crate manifest with empty `[workspace]` table
- `spikes/action-guide-live-ffmpeg/src/main.rs` — `RunConfig`, strict CLI parsing (`parse_args`), `main` with validation-only exit
- `spikes/action-guide-live-ffmpeg/src/workload.rs` — deterministic desktop-like RGBA renderer (`render_frame`)

## What Was Done

### Workload Renderer
- Dark desktop background (28,28,30)
- Left sidebar with vertically scrolling text-like bars (bar y-positions shift with `frame_index % 16`)
- Main content area with a bar chart whose heights cycle with frame_index
- Moving 6×6 white cursor (diagonal traversal derived from frame_index)
- All pixel alpha = 255; all positions/colors derived deterministically from frame_index
- Passes all three workload tests: stable dimensions/alpha, deterministic per-index, >5% pixel change per second

### CLI Parsing
- Accepts: `--ffmpeg`, `--ffprobe`, `--output`, `--report` (required paths), `--width` (default 1920), `--height` (1080), `--fps` (30), `--duration-secs` (600), `--queue-capacity` (2)
- Rejects: unknown flags, missing required args, missing values, zero dimensions/fps/duration/capacity, odd width/height, non-numeric values
- `parse_args` is a pure function (`&[String] -> Result<RunConfig, String>`)
- `main` validates config and prints only numeric fields (no path leakage)

### TDD Workflow
1. Wrote workload tests first → verified compilation failure
2. Implemented RunConfig + workload renderer → fixed overflow in color arithmetic (u8 debug-mode overflow)
3. Added Debug derive to RunConfig (required by `unwrap_err()` in CLI tests)
4. All 15 tests pass (3 workload + 12 CLI)

## Test Summary
`15 passed` — 3 workload tests (dimensions/alpha, determinism, >5% change), 12 CLI tests (valid defaults, custom values, missing required, unknown flag, zero width/fps/duration/capacity, odd width/height, missing value, invalid number)

## Commit
- `4b0b9cf` — `spike(action-guide): add live encoding workload`

## Concerns
- `edition = "2024"` requires Rust 1.85+; the `[workspace]` table is empty as specified, isolating the spike from production crates.

---

## Fix Report (Review Round 1)

### Finding 1: Missing zero_height CLI validation test
**Status:** FIXED
**File:** `spikes/action-guide-live-ffmpeg/src/main.rs`
**Problem:** The spec says "Reject zero dimensions" (both width and height). Implementation correctly rejects `--height 0` but the test suite covered only `zero_width_is_error` with no corresponding `zero_height_is_error` test, creating asymmetry.
**Fix:** Added `zero_height_is_error` test using the same pattern as `zero_width_is_error` — passes `--height 0` and asserts `is_err()`.
**Verification:** Focused test `tests::zero_height_is_error` passed; full crate suite: 16 passed (was 15).
