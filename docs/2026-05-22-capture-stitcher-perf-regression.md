# Capture pipeline appears hung on macOS — stitcher perf regression

Status: open
Severity: high (blocks the macOS happy path of `rollshot capture`)
Reporter: noah
Date: 2026-05-22

## TL;DR

After the AutoHybrid matcher landed (commit `1b571da`, "linearscroll plan2
AutoHybrid matcher"), running `rollshot capture` on a real macOS host with the
default `StitchConfig` looks completely frozen. The capture is not actually
hung — `scap` is producing frames and the stitcher is making forward progress —
but each frame-pair match takes long enough on retina-scale input that a
10-frame run in `cargo run` (debug profile) does not finish in any tolerable
time, and the CLI prints nothing until the very end.

The regression bisects to the matcher rewrite, not to the macOS backend.

## Symptom

```
$ cargo run -p rollshot-cli --no-default-features --features macos-sck -- \
    capture \
    --backend macos-sck \
    --region "0,120 1470x660" \
    --fps 1 \
    --max-frames 10 \
    --dump-frames target/test-artifacts/macos_frames \
    --output target/test-artifacts/macos_cropped.png
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.85s
     Running `target/debug/rollshot capture ...`
^C   # ← no output, no screen-recording prompt, killed manually
```

Observations from the user-facing side:

- No stdout/stderr between `Running` and `^C`.
- No macOS screen-sharing prompt (the user expected one).
- Process looks frozen.

Observations after attaching `ps`:

- Process is `RN` (runnable, not waiting), 100% CPU, single thread.
- `target/test-artifacts/macos_frames/` accumulates `frame_0000.png` …
  `frame_0006.png` (~3 MB each, 2940×1320), then stops.
- The recording indicator does appear briefly in the menu bar during the
  initial frame burst.
- A `--max-frames 1` run on the same machine finishes in seconds and writes a
  valid 2940×1320 PNG.

So: capture works, stitching is the bottleneck, and the CLI gives no progress
signal so the user cannot tell.

## Environment

| Field             | Value                                        |
| ----------------- | -------------------------------------------- |
| Host              | MacBook Air, Apple M3, 8 cores               |
| OS                | macOS 26.5                                   |
| Toolchain         | `cargo run` → debug profile, unoptimized     |
| Features          | `--no-default-features --features macos-sck` |
| Backend           | `macos-sck` (scap / ScreenCaptureKit)        |
| `scap` version    | `zed-scap 0.0.8-zed` (unchanged in range)    |
| Screen Recording  | already granted (probe confirms)             |

`rollshot probe` on this host:

```
backends:
  - macos-sck (available): scap macOS capture is available
      os: macos
      scap_supported: true
      screen_recording_permission: granted
```

Permission being already granted is why no system prompt shows up — `scap`
silently starts capturing. That is correct behavior, just easy to misread as
"nothing happened".

## Reproduction

1. Be on macOS with Screen Recording permission already granted for the
   terminal that runs `cargo`.
2. From repo root:
   ```
   cargo run -p rollshot-cli --no-default-features --features macos-sck -- \
       capture --backend macos-sck \
       --region "0,120 1470x660" \
       --fps 1 --max-frames 10 \
       --dump-frames /tmp/rollshot_frames \
       --output /tmp/rollshot_out.png
   ```
3. Watch: no output for many minutes. `/tmp/rollshot_frames/` fills with 6–8
   PNGs then stops while the process keeps burning CPU.

`--max-frames 1` finishes quickly and confirms the capture stack is fine.

## Bisect

| Commit     | Behavior                                                  |
| ---------- | --------------------------------------------------------- |
| `b36f347`  | `fix(capture): compile macos screencapturekit backend` — capture runs end-to-end, output PNG is written. |
| `1b571da`  | `feat: linearscroll plan2 AutoHybrid matcher` — regression starts here. |
| `cf9044a`  | `feat: linearscroll plan3` (current `main`) — same symptom. |

Between `b36f347` and `main`:

- `crates/rollshot-capture/**` is **unchanged**.
- `zed-scap` lockfile entry is **unchanged** (`0.0.8-zed`, identical checksum).
- `crates/rollshot-cli/src/cmd_capture.rs` only changed match arms for new
  `StitchOutcome` variants — no behavioral change.
- `crates/rollshot-core/src/matcher.rs` was rewritten from ~7 lines of plan-1
  code into the 1069-line AutoHybrid pipeline.

So the regression is entirely inside `rollshot-core` stitching, not in the
capture backend.

## Root cause

`Stitcher::push_frame` calls `matcher::estimate_motion`, which now runs the
following on every frame pair before accepting a match:

1. `coarse_candidates` — grid search over both axes.
2. `template_candidates` — full-resolution NCC sweep along the predicted axis
   and the cross axis.
3. `edge_projection_candidates` — additional projection-based candidates.
4. `rank_verified_candidates` — `PixelOverlapVerifier::verify` over each
   surviving candidate.
5. Optional AKAZE fallback (not enabled in the user's command).

The expensive step in the default config is `search_template_axis`
(`crates/rollshot-core/src/matcher.rs:263-335`). For each axis it iterates
`signed_predict_iter(max_offset, last_offset)` and calls
`ncc_score_shifted(..)` for every offset.

With the **default `StitchConfig`** (`crates/rollshot-core/src/types.rs:200-224`):

```rust
max_search_ratio: 0.75,
match_width: 512,
```

and the **actual frame size delivered by scap** (`2940 × 1320` — retina 2× of
the requested `1470 × 660` region; the user only sees the logical region
size), the work per frame-pair is approximately:

- Vertical sweep: `min(height - min_overlap, height * 0.75)` ≈ **990 offsets**,
  each NCC over a 512×1320 window.
- Horizontal sweep: `min(width - min_overlap, width * 0.75)` ≈ **2205
  offsets**, each NCC over a 512×1320 window.
- Then verifier passes for every candidate that survives filtering.

That is on the order of `~3 × 10^9` float multiply-add operations per pair,
single-threaded, **in a debug build with no optimizations**. Empirically: 100%
CPU on M3 for >150 seconds and still running, with one frame-pair appended.

Compounding factor — `cmd_capture::run`
(`crates/rollshot-cli/src/cmd_capture.rs`) only prints anything after the loop
finishes. There is no per-frame log, no progress bar, no "captured frame N",
nothing. So a slow inner loop is indistinguishable from a deadlock from the
outside, which is what made the user reach for `^C`.

## Why pre-`1b571da` worked

The plan-1 matcher was a single coarse+template path tuned to small images
(test fixtures are tens to a few hundred pixels per side). It happened to scale
acceptably to a 2940×1320 frame because the inner loops were ~10× cheaper than
the AutoHybrid pipeline. The new pipeline is correct and well-tested on the
fixture suite, but those fixtures are nowhere near retina-screen resolution,
so the cost on real macOS input was never exercised in CI.

## Workarounds

Confirmed locally:

- `cargo run --release` — release optimizations bring NCC down by 10-50×.
  A single-frame run already passes in debug; multi-frame runs are likely
  tolerable in release. **This is what users should do today.**
- `--max-frames 1` finishes correctly in debug and writes a valid PNG, so the
  pipeline can be smoke-tested without the perf cliff.

These are mitigations, not fixes. A debug build of `rollshot capture` on real
macOS resolution should still complete in a reasonable time — the developer
loop depends on it.

## Suggested fixes (for triage)

In rough order of bang-for-buck:

1. **Coarse-to-fine search.** Run `template_candidates` on a downsampled
   pyramid (e.g. 4× or 8× downscale) to localize the offset, then refine in a
   small window at full resolution. Drops the inner cost by `O(scale²)` without
   changing accuracy materially.
2. **Tighter default `max_search_ratio`.** `0.75` means "the match can be at
   75% of frame size", which is rarely useful for continuous scrolling. A
   default closer to `0.3–0.4`, with per-call override for first-frame search,
   would cut the offset count 2–3×.
3. **Downsample the inputs to `estimate_motion`.** The matcher's accuracy on
   real-world scrolls is dominated by the coarse stage; the template stage
   does not need raw retina pixels.
4. **Per-frame progress logging in `cmd_capture::run`.** Even a single
   `eprintln!("frame {idx}: {outcome:?} in {elapsed:?}")` would have made this
   user-visible immediately and removed "is it hung?" as a question. Cheap and
   independently useful.
5. **Parallelize the offset sweep.** `ncc_score_shifted` per offset is
   embarrassingly parallel; `rayon` over the offset iterator on M3 (8 cores)
   would buy ~6–7× by itself. Lower priority than (1)/(3) because algorithmic
   wins dominate.
6. **CI/fixture coverage for real-resolution input.** Add a fixture pair at
   2940×1320 (or similar retina scale) so the matcher's per-frame budget gets
   exercised in `cargo test` and any future regression shows up before users
   hit it.

(1) and (4) are probably the smallest diffs that close the issue.

## Out of scope / not the cause

- macOS Screen Recording permission — confirmed `granted`.
- `scap` / `zed-scap` version — unchanged between working and broken commits.
- `rollshot-capture` source — unchanged in the regression range.
- AKAZE — feature is **off** in the user's command; the AKAZE fallback path
  isn't even entered. The regression reproduces with `--no-default-features
  --features macos-sck` only.
- `--dump-frames` — removing it does not change the symptom; frames are
  already written before the stitcher work begins.
