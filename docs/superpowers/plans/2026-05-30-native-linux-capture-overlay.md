# Native Linux Capture Overlay — Implementation Plan (Phase 3)

> **For agentic workers:** REQUIRED SUB-SKILL: use
> `superpowers:subagent-driven-development` (recommended) or
> `superpowers:executing-plans` to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-05-30-native-linux-capture-overlay-design.md`
(read it first — it locks P3.1–P3.7). Parent: the Phase 1 architecture spec.
Decision gate cleared by `spikes/layershell-feasibility/FINDINGS.md`.

**Goal:** ship `crates/rollshot-overlay` — a Linux-only `iced_layershell`
capture overlay that owns a minimal capture+stitch driver and returns a
finalized `CaptureResult`, verified by a standalone harness binary on KDE 6
Wayland. Tauri-free. `src-tauri` wiring + save dialog are Phase 4.

**Tech stack:** Rust, `iced_layershell = "0.18"` + `iced = "0.14"`
(canvas, image), `rollshot-capture` + `rollshot-core` (path deps),
`image = "0.25"`.

---

## Ground Rules (read before starting)

- **This is production code**, not a spike. Follow `AGENTS.md`: simplest thing
  that passes the checks, surgical changes, match existing style.
- **No `std::process::exit`** in the crate (P3.3) — Phase 4 runs inside Tauri.
- **Do not touch** `cmd_capture.rs` or `session.rs` (P3.2 — no shared-driver
  refactor). The only `rollshot-capture` change is the D4 edit in Task 2.
- **`rollshot-core` / `rollshot-capture` MUST NOT depend on `rollshot-overlay`**
  (D3). `rollshot-overlay` MUST NOT depend on Tauri.
- **Port, don't reinvent:** `spikes/layershell-feasibility/src/overlay_app.rs`
  is the working UI prototype; port it, then adapt for the real driver + R3.
- **Hardware:** every "Run on KDE 6" step needs a KDE 6 Wayland session
  (current dev box: Plasma 6.6.5 / KWin 6.6.5 / NVIDIA).
- **Verify per `AGENTS.md` §7:** `rtk cargo test`, `rtk cargo fmt --check`,
  `rtk cargo clippy --workspace --all-targets -- -D warnings` where risk
  justifies.

## File Structure

```text
crates/rollshot-overlay/Cargo.toml
crates/rollshot-overlay/src/lib.rs          run_overlay, CaptureResult, OverlayConfig, OverlayError
crates/rollshot-overlay/src/coords.rs       map_crop_to_frame (pure, unit-tested)  [Task 3]
crates/rollshot-overlay/src/driver.rs       reader + stitch loop + finalize        [Task 4]
crates/rollshot-overlay/src/overlay.rs      iced_layershell app                    [Task 5]
crates/rollshot-overlay/src/bin/capture_overlay.rs   harness binary                [Task 7]
Cargo.toml (root)                            add workspace member                  [Task 1]
crates/rollshot-capture/src/linux/portal.rs  D4 monitor-only (permanent)           [Task 2]
```

---

## Task 1: Scaffold the `rollshot-overlay` crate as a workspace member

**Files:** create `crates/rollshot-overlay/Cargo.toml`,
`crates/rollshot-overlay/src/lib.rs`; modify root `Cargo.toml`.

- [ ] **Step 1: Cargo.toml with Linux-gated layer-shell deps**

```toml
[package]
name = "rollshot-overlay"
version = "0.1.0"
edition = "2021"

[dependencies]
image = { version = "0.25", features = ["png"] }
rollshot-capture = { path = "../rollshot-capture" }
rollshot-core = { path = "../rollshot-core" }

[target.'cfg(target_os = "linux")'.dependencies]
iced = { version = "0.14", features = ["canvas", "image"] }
iced_layershell = "0.18"
```

- [ ] **Step 2: Linux-gated lib skeleton**

`src/lib.rs`: declare the public types (`CaptureResult`, `OverlayConfig`,
`OverlayError`) cross-platform, but gate `run_overlay` + the `mod overlay/driver/coords`
behind `#[cfg(target_os = "linux")]` so non-Linux builds an empty-ish lib.
Provide a non-Linux `run_overlay` stub returning `OverlayError::Unsupported` so
the symbol exists everywhere.

- [ ] **Step 3: Add to root workspace** `members` in root `Cargo.toml`.

- [ ] **Step 4: Verify**

`rtk cargo build --workspace` (Linux: real build; the crate compiles).
`rtk cargo fmt --check`. Confirm `rollshot-capture`/`-core` did NOT gain a dep
on `rollshot-overlay` (`rtk cargo tree -p rollshot-core | rtk grep overlay`
returns nothing).

- [ ] **Step 5: Commit** `feat(overlay): scaffold rollshot-overlay crate (Phase 3)`

---

## Task 2: D4 — make portal monitor-only (permanent production change)

**Files:** modify `crates/rollshot-capture/src/linux/portal.rs`.

- [ ] **Step 1:** At `select_sources` (~`:258-259`) request
  `SourceType::Monitor` only (remove `| SourceType::Window`).

- [ ] **Step 2:** Where stream info is built (~`:280-286`), also read
  `Stream::source_type()`; if a started stream reports `Window`, return a
  `CaptureError` with a clear message. (Verify the exact ashpd accessor name.)

- [ ] **Step 3: Verify** `rtk cargo test -p rollshot-capture` passes. Add/confirm
  a unit for the defensive `Window -> CaptureError` path if reachable without a
  live portal. Manual (KDE 6): launch any capture; confirm the portal picker
  offers **monitors only**.

- [ ] **Step 4: Commit** `feat(capture): restrict portal sources to Monitor (D4)`

---

## Task 3: Coordinate mapping `coords::map_crop_to_frame` (R4) + unit tests

**Files:** create `crates/rollshot-overlay/src/coords.rs`.

- [ ] **Step 1:** Implement the pure mapping from spec P3.5: logical crop rect +
  overlay logical size + `source_size` → `rollshot_capture::Region` in frame
  pixels, clamped to `source_size`. No I/O, no iced types in the signature
  (take plain numbers / a small local struct) so it tests cross-platform.

- [ ] **Step 2: Unit tests** at 100%, 125%, 150% scale, plus a region that
  clamps at the output edge, plus a zero-size guard. These tests are the R4
  safety net.

- [ ] **Step 3: Verify** `rtk cargo test -p rollshot-overlay coords`.

- [ ] **Step 4: Commit** `feat(overlay): crop->frame coordinate mapping with scale tests (R4)`

---

## Task 4: Minimal capture+stitch driver (P3.2) + fixture test

**Files:** create `crates/rollshot-overlay/src/driver.rs`.

- [ ] **Step 1:** Implement the driver mirroring `session.rs:374-561` but minimal:
  - `start(config, region_px)`: spawn reader thread
    (`BackendKind::from_cli_flag` → `backend.start(CaptureOptions { region: FullSource, fps, show_cursor, .. })` → `next_frame` into a latest-wins slot with a seq counter), and a stitch thread (on new seq: `crop_frame(&frame, region_px)` → `Stitcher::push_frame`).
  - a preview hook: after each `push_frame`, downscale `Stitcher::full_image()`
    and push an `image::Handle` into an `mpsc` sender (the channel the overlay
    subscribes to — proven by the spike R6).
  - `finalize() -> Result<CaptureResult>`: stop both threads (join), take
    `Stitcher::full_image()` + `stats()` → `CaptureResult`.
  - Use `StitchConfig::default()` with `min_overlap = 32` (match `session.rs:188-190`).

- [ ] **Step 2: Integration test** with `FixtureBackend` + the `scrolling_frame`
  fixture pattern (copy the generator idea from `session.rs` tests): drive a few
  frames, finalize, assert `CaptureResult.image` dims + `stats.frame_count`.
  This makes the driver CI-verifiable without KDE.

- [ ] **Step 3: Verify** `rtk cargo test -p rollshot-overlay driver`,
  `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`.

- [ ] **Step 4: Commit** `feat(overlay): minimal in-crate capture+stitch driver (P3.2)`

---

## Task 5: Port the overlay UI; wire crop-confirm and live preview (R3, R6)

**Files:** create `crates/rollshot-overlay/src/overlay.rs` (port from
`spikes/layershell-feasibility/src/overlay_app.rs`).

- [ ] **Step 1:** Port the spike overlay: transparent layer surface
  (`Color::TRANSPARENT`, `Layer::Overlay`, all-anchors, `KeyboardInteractivity::Exclusive`),
  crop-drag canvas, toolbar (Finish/Cancel + status), preview via `mpsc` +
  `Subscription::run`.

- [ ] **Step 2: Wire the driver.** On crop **confirm** (Finish): call
  `coords::map_crop_to_frame` to get the frame-pixel region, then
  `driver.start(region_px)`; switch to the scrolling phase; apply
  `SetInputRegion` so only the toolbar stays interactive (rest scroll-through —
  spike R6 PASS).

- [ ] **Step 3: R3 (P3.4).** During the scrolling phase draw **nothing inside
  the crop region**: stop drawing the selection border, and position the
  live-preview panel + toolbar **outside** the crop region. The crop interior
  stays transparent + scroll-through. Handle the full-output crop edge case by
  relocating/hiding the preview (documented fallback: per-frame hide).

- [ ] **Step 4: Verify** `rtk cargo build -p rollshot-overlay`,
  `rtk cargo fmt --check`, `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`.
  (Runtime behavior verified in Task 7.)

- [ ] **Step 5: Commit** `feat(overlay): port layer-shell UI + driver wiring + R3 chrome rules`

---

## Task 6: `run_overlay` entry point + clean event-loop exit (P3.3)

**Files:** modify `crates/rollshot-overlay/src/lib.rs`,
`crates/rollshot-overlay/src/overlay.rs`.

- [ ] **Step 1: Clean exit.** Replace the spike's `std::process::exit(0)` on Esc
  with a clean `iced_layershell` shutdown: on Esc, `driver.finalize()`, stash the
  `CaptureResult` in a shared slot, then emit the layer-shell close/exit action
  so `.run()` returns. **Confirm the exact `iced_layershell` action** (window
  close / loop exit message) against the dep source
  (`learn-projects/exwlshelleventloop`). If no clean exit exists, STOP and record
  it as a Phase 3 blocker (do NOT fall back to `process::exit`).

- [ ] **Step 2:** Implement `run_overlay(OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>`:
  set up the driver + preview channel, run the iced app (blocks), then read the
  stashed result — `Ok(Some(result))` on Esc-finish, `Ok(None)` on cancel,
  `Err` on failure.

- [ ] **Step 3: Verify** `rtk cargo build -p rollshot-overlay`. (Functional check
  in Task 7.)

- [ ] **Step 4: Commit** `feat(overlay): run_overlay entry + clean iced exit (P3.3)`

---

## Task 7: Harness binary + KDE 6 acceptance (GATE)

**Files:** create `crates/rollshot-overlay/src/bin/capture_overlay.rs`.

- [ ] **Step 1:** Harness: build `OverlayConfig` (backend from `argv`, default
  "auto"), call `run_overlay`, and on `Ok(Some(result))` save
  `result.image` as a PNG + print `result.stats`. `Ok(None)` → print
  "cancelled". This binary stands in for Tauri.

- [ ] **Step 2: Run on KDE 6 — roadmap Phase 3 acceptance checks:**
  1. Overlay appears above fullscreen apps.
  2. Crop region selectable.
  3. Scroll the target content during stitching.
  4. Live preview updates while scrolling.
  5. Esc finishes → harness saves a PNG that matches the scrolled content.

- [ ] **Step 3: Carried runtime checks (GATE):**
  - **R3:** scan the saved/captured frame for the sentinel color **inside the
    mapped crop region only** → must be 0 (tighter than the spike's whole-frame
    scan). Record the count.
  - **R4:** run at **100% and 150%** display scale; confirm the saved PNG region
    matches the on-screen selection (no off-by-scale offset). Record any offset.
  - **R5/R7:** single-output is the verified path; if a multi-monitor setup is
    available, confirm the overlay anchors to the captured output, else document
    the blocker.

- [ ] **Step 4: Record results** in a short `crates/rollshot-overlay/NOTES.md`
  (or append to FINDINGS) — this is the Phase 3 runtime evidence.

- [ ] **Step 5: Commit** `feat(overlay): harness binary + KDE 6 acceptance results (Phase 3 GATE)`

---

## Task 8: Wrap up + Phase 4 carryover

- [ ] **Step 1: Full verify** `rtk cargo test`, `rtk cargo fmt --check`,
  `rtk cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 2: Update the roadmap** (`docs/linux-wayland-layer-shell-roadmap.md`):
  mark Phase 3 status + carry into Phase 4: (a) `src-tauri` Linux branch spawns
  `run_overlay` on a thread and feeds `CaptureResult.image` into `AppSession`;
  (b) the save dialog handoff (D5); (c) **R2:** hide / de-focus the Tauri host
  window during the overlay phase (FINDINGS §5); (d) any R5/R7 multi-output
  follow-up.

- [ ] **Step 3: Commit** `docs(roadmap): Phase 3 overlay done; carry Tauri wiring + R2 into Phase 4`

---

## Success Criteria

- `rollshot-overlay` builds in the workspace (Linux real, non-Linux empty-lib).
- `coords` + `driver` unit/integration tests pass in CI (no KDE needed).
- `rollshot-capture` D4 change is permanent; its tests pass; picker is
  monitor-only.
- On KDE 6: crop → scroll → live preview → Esc → `run_overlay` returns a
  `CaptureResult` and the harness saves a correct PNG (roadmap Phase 3 flow).
- R3 sentinel count inside the crop region is 0; R4 correct at 100% and 150%.
- No `std::process::exit` in the crate; no Tauri dependency; `cmd_capture.rs` /
  `session.rs` untouched.
