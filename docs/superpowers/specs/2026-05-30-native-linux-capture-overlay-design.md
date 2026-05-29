# Native Linux Capture Overlay (Phase 3 Spec)

Status: design spec for Phase 3 of `docs/linux-wayland-layer-shell-roadmap.md`.
Parent spec: `docs/superpowers/specs/2026-05-29-linux-wayland-layer-shell-architecture-design.md`
(Phase 1 — locks D1–D5, the coordinate model, and the support matrix).
Decision gate cleared by: `spikes/layershell-feasibility/FINDINGS.md`
(Phase 2 spike — GO on KDE 6.6.5 Wayland / NVIDIA).

All file:line references are evidence captured during design and may drift;
verify against code before relying on them.

## Goal

Replace the Linux Tauri webview-driven crop picker + live stitching preview with
a native `iced_layershell` overlay that owns its own capture+stitch driver and
hands a finalized image back to the caller. Preserve current user behavior:

```text
crop select -> scroll with live stitching preview -> Esc -> (Phase 4: Tauri save dialog)
```

This phase swaps the Linux capture **UI shell** and adds the driver that feeds
it. `rollshot-capture` and `rollshot-core` are **not** rewritten (one small,
spec-sanctioned D4 change to `rollshot-capture` aside).

## Scope (locked by the Phase 3 kickoff decision)

**In scope — Phase 3 builds and verifies, Tauri-free:**

- New crate `rollshot-overlay` (Linux-only): the `iced_layershell` overlay UI +
  a minimal in-crate capture+stitch driver.
- The D4 `rollshot-capture` change (monitor-only source selection), this time as
  a **real, permanent** production change (the spike reverted it).
- The `CaptureResult` handoff type and a blocking `run_overlay() -> Result<CaptureResult>`
  entry point with a **clean** event-loop exit (no `std::process::exit`).
- A standalone **harness binary** in the crate that runs the overlay and, on
  finish, saves `CaptureResult.image` as a PNG. This binary is the KDE 6
  acceptance vehicle; it stands in for Tauri.

**Deferred to Phase 4 (explicitly NOT in this phase):**

- `src-tauri` Linux platform branch (spawn the overlay thread, receive
  `CaptureResult`).
- The Tauri save dialog + `save_image` handoff.
- Hiding / de-focusing the Tauri host window during the overlay phase (the R2
  production mitigation — see Phase 2 FINDINGS §5). Phase 3's harness has no
  competing Tauri toplevel, so R2 does not arise here.

## Inherited Decisions (from Phase 1 + the spike — not re-litigated here)

- **D1 toolkit:** `iced_layershell` (spike: GO). Fallback `smithay-client-toolkit`
  is not triggered.
- **D2 process model:** overlay on a spawned thread; `.run()` blocks that thread.
  R1 GPU coexistence PASS on NVIDIA. (Phase 3 runs the overlay on a thread inside
  the harness; Phase 4 runs it on a thread inside Tauri.)
- **D3 crate:** `rollshot-overlay`, Linux-only, **MUST NOT depend on Tauri**.
  `rollshot-core` / `rollshot-capture` MUST NOT depend on it.
- **D4 monitor-only:** enforced at the portal layer.
- **D5 handoff:** generic `CaptureResult { image, stats }`, not "save PNG only".
- **Coordinate model:** crop rect in output-logical coords maps to frame pixel
  coords by the output scale, clamped to `source_size`.

## Phase 3 Decisions (this spec locks them)

### P3.1 — `rollshot-overlay` crate layout

```text
crates/rollshot-overlay/                 (new, workspace member, Linux-only body)
  Cargo.toml
  src/
    lib.rs        -> pub run_overlay(cfg) -> Result<CaptureResult>; pub CaptureResult
    overlay.rs    -> iced_layershell app (crop picker, live preview, toolbar, Esc)
                     [ported from spikes/layershell-feasibility/src/overlay_app.rs]
    driver.rs     -> minimal capture+stitch driver (reader + stitch loop + finalize)
    coords.rs     -> crop logical -> frame pixel mapping (pure, unit-tested)
  src/bin/
    capture_overlay.rs -> harness: run_overlay() then save PNG (KDE 6 acceptance)
```

Cross-platform build rule: `rollshot-overlay` is a workspace member, but its
layer-shell deps live under `[target.'cfg(target_os = "linux")'.dependencies]`
and the crate body is `#[cfg(target_os = "linux")]`-gated, so
`cargo build --workspace` succeeds on macOS/Windows (the crate compiles to an
empty lib there). KDE-specific behavior, if any, must be called out per the
support-matrix rule.

### P3.2 — Driver: reimplement a minimal driver inside the crate (DECIDED)

This resolves the D3 open question. `rollshot-overlay::driver` reimplements the
latest-wins reader + stitch-loop pattern (mirroring
`crates/rollshot-app/src-tauri/src/session.rs:374-561`), scoped to exactly what
the overlay needs:

- a reader thread: `backend.start(CaptureOptions { region: FullSource, .. })`
  then `next_frame()` into a latest-wins slot (seq counter), per
  `session.rs:410-428`;
- a stitch thread: on new seq, `crop_frame(&frame, region)` →
  `Stitcher::push_frame`, per `session.rs:535-561` and `:199-212`;
- `finalize()`: stop both threads, `Stitcher::full_image()` → `CaptureResult`.

**We do NOT extract a shared driver** or touch `cmd_capture.rs` /
`session.rs`. The triplicate driver logic is a known, recorded cost; a future
refactor may unify it, but that is out of this phase's surgical scope (Phase 1
D3 explicitly left this choice to this plan).

### P3.3 — `CaptureResult` handoff + clean exit

```rust
pub struct CaptureResult {
    pub image: RgbaImage,
    pub stats: StitchStats,   // rollshot_core::StitchStats
}

pub struct OverlayConfig {
    pub backend: String,      // e.g. "auto" / "linux-portal" (BackendKind::from_cli_flag)
    pub fps: u32,
    pub show_cursor: bool,
    // output targeting: see P3.6
}

/// Blocks the calling thread (iced `.run()` blocks). Returns the finalized
/// capture, or Ok(None) if the user cancelled, or Err on failure.
pub fn run_overlay(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>;
```

On Esc the overlay must: tell the driver to finalize, stash the `CaptureResult`
in a shared slot, and request a **clean** `iced_layershell` event-loop exit so
`run_overlay` returns it. The spike used `std::process::exit(0)` — that is
**banned** in the crate (it would kill the Tauri process in Phase 4). The exact
clean-exit action (`iced_layershell` window-close / loop-exit message) is
confirmed in the plan (Task 6); if no clean exit exists, that is a Phase 3
blocker to record, not a `process::exit` workaround.

Cancel (toolbar Cancel / Esc before any region is confirmed) returns `Ok(None)`.

### P3.4 — R3 self-capture mitigation: chrome-outside-crop (DECIDED)

Linux has no exclude-from-capture (`src-tauri/src/overlay.rs:51-54` →
`OverlayExclusion::Unsupported`; confirmed by the spike: the overlay IS
composited into portal frames). Strategy, matching Phase 1 R3:

The driver captures the **full monitor** frame, then `crop_frame` crops to the
user's region in pixel space **before** `push_frame`. Therefore anything the
overlay draws **outside** the user's crop region never reaches the stitcher.
The rule is then simply:

> **During the scrolling/capture phase, the overlay draws NOTHING inside the
> user's crop region.** The crop interior is fully transparent and
> click/scroll-through; the live-preview panel and the toolbar are positioned
> **outside** the crop region. The selection border is drawn only during the
> crop-selection phase, not during capture.

Edge case — a crop region that fills the output leaves no room for chrome
outside it. Phase 3 handles this by relocating/clipping chrome to avoid the
region where possible; if impossible, the live preview is hidden during capture
(controls remain reachable via the input region). A per-frame hide (unmap /
fully transparent-ify the whole overlay around each captured frame — the
flameshot/spectacle pattern) is the **documented fallback** if chrome-outside
proves insufficient on KDE 6, but it is not the default because syncing a hide
to a specific PipeWire frame is unreliable.

Verification tightens the spike's check: scan for the sentinel color **only
within the mapped crop region** of captured frames (the spike's whole-frame scan
could not answer the real question — see FINDINGS R3 caveat).

### P3.5 — Coordinate mapping (R4)

`coords::map_crop_to_frame` is pure and unit-tested:

```text
scale_x = source_size.width  / overlay_logical_width
scale_y = source_size.height / overlay_logical_height
frame_region = Region {
    x:      round(crop_logical.x * scale_x),
    y:      round(crop_logical.y * scale_y),
    width:  round(crop_logical.w * scale_x),
    height: round(crop_logical.h * scale_y),
}.clamp_to(source_size)
```

`source_size` comes from `FrameMetadata` (`rollshot-capture/src/types.rs:47-53`
— verify exact field). `overlay_logical_*` is the layer surface's logical size
on the anchored output. Unit tests cover 100%, 125%, and 150% scale, including
clamp-at-edge. This function is the R4 risk surface; getting it right in pure
code (testable cross-platform) is the point of isolating it.

### P3.6 — Output anchoring (R5 / R7)

The overlay MUST anchor to the same output the portal captures (Phase 1
output-matching constraint). Mechanism:

- `iced_layershell` `StartMode::TargetScreen(name)` anchors to a named output;
  `StartMode::Active` uses the focused output.
- The portal does not hand back an output **name**; `ashpd`
  `Stream::position()` / `Stream::size()` give the captured monitor's geometry,
  which can be matched to a `wl_output`'s geometry to recover the name.

Phase 3 targets **single-output** as the primary verified path
(`StartMode::Active`). Multi-output geometry-matching is specified here but its
runtime verification (R5/R7) is carried forward as a known KDE 6 test — the
spike ran single-output only. If multi-output matching proves unreliable on
KDE 6, document the blocker (roadmap permits this for R7).

### P3.7 — D4 capture change (permanent this time)

In `rollshot-capture/src/linux/portal.rs`:

- `select_sources` (`:258-259`): request `SourceType::Monitor` **only** (drop
  `| Window`).
- Stream-info build (`:280-286`): also read `Stream::source_type()`; if a
  started stream is `Window`, return a `CaptureError` (defense-in-depth — KDE
  honors the request-time hint, but the type arg is advisory).

This is the one sanctioned change to `rollshot-capture`. It is NOT reverted
(unlike the spike's Task 7). `rollshot-capture` tests must still pass; the
public API (`CaptureBackend::start`, `FrameStream::next_frame`) is unchanged.

## Data Flow (Phase 3)

```text
harness binary (stands in for Tauri in Phase 3)
  run_overlay(config)            [blocks this thread]
    iced_layershell overlay (transparent, anchored to output)
      crop-select phase: user drags box; border drawn; scroll NOT yet captured
      confirm region -> coords::map_crop_to_frame(logical -> frame px)
                     -> driver.start(region_px)
        driver reader thread: backend.start(FullSource) -> next_frame -> latest slot
        driver stitch thread: crop_frame(region_px) -> Stitcher::push_frame
        driver -> full_image() -> downscale -> mpsc -> overlay redraw (live preview,
                                                       drawn OUTSIDE crop region)
      Esc -> driver.finalize() -> Stitcher::full_image() -> CaptureResult
           -> stash result + request clean iced exit
  run_overlay returns Ok(Some(CaptureResult))
  harness: save CaptureResult.image as PNG + print stats
```

Stable contracts unchanged: `CaptureBackend::start` / `FrameStream::next_frame`
(`rollshot-capture/src/backend.rs:4-12`), `Stitcher::push_frame` /
`Stitcher::full_image` (`rollshot-core/src/stitcher.rs:42-54`, `:357-359`),
`crop_frame` (`rollshot-capture`).

## Public API Surface (the crate's contract for Phase 4)

- `run_overlay(OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>`
- `struct CaptureResult { image: RgbaImage, stats: StitchStats }`
- `struct OverlayConfig { backend, fps, show_cursor, /* output target */ }`

Phase 4 will call `run_overlay` from a spawned thread inside Tauri and feed
`CaptureResult.image` into the existing `AppSession` final-image + save flow.
Nothing in this surface mentions Tauri or "PNG only" (D5).

## Verification Strategy

Cross-platform automated (CI-able, no KDE needed):

- `coords::map_crop_to_frame` unit tests at 100% / 125% / 150% + edge clamp (R4).
- `driver` unit/integration test with `FixtureBackend` (the same
  `scrolling_frame` fixture pattern as `session.rs` tests) → asserts a stitched
  image with expected dims and frame count.
- `rollshot-capture` D4: existing tests pass; a unit covering the defensive
  `source_type == Window -> CaptureError` path.
- `cargo build --workspace` succeeds on the empty-lib non-Linux path.

Manual KDE 6 Wayland acceptance (the harness binary) — roadmap Phase 3 checks:

1. Overlay appears above fullscreen apps.
2. User selects a crop region.
3. User scrolls target content while stitching is active.
4. Live stitching preview updates during scrolling.
5. Esc finishes stitching and `run_overlay` returns a `CaptureResult` (harness
   saves the PNG) — the handoff fires.

Plus carried-over runtime checks:

- **R3:** sentinel scan **inside the mapped crop region** == 0.
- **R4:** run at 100% and 150% display scale; the saved PNG matches the selected
  on-screen region (no off-by-scale offset).
- **R5/R7:** single-output verified; multi-output documented (matched or
  blocker).

## Out of Scope / Non-Goals (this phase)

- Tauri integration and the save dialog (Phase 4).
- R2 host-window focus mitigation (Phase 4; absent in the harness).
- Arbitrary window capture (roadmap non-goal; monitor-only via D4).
- Shared-driver extraction / refactor of `cmd_capture.rs` / `session.rs` (P3.2).
- GNOME, X11, hard non-KDE guarantees.

## Open Risks Carried Into Implementation

- **Clean iced_layershell exit:** must replace the spike's `process::exit`;
  exact action confirmed in plan Task 6. Blocker if absent.
- **Full-output crop edge case** for R3 chrome placement (P3.4) — fallback is
  documented per-frame hide.
- **Multi-output anchoring (R5/R7):** geometry-matching unverified on KDE 6.

## References

- Phase 1 spec: `docs/superpowers/specs/2026-05-29-linux-wayland-layer-shell-architecture-design.md`.
- Phase 2 spike findings: `spikes/layershell-feasibility/FINDINGS.md`; working
  prototype `spikes/layershell-feasibility/src/overlay_app.rs` (port source).
- Current Tauri driver to mirror: `crates/rollshot-app/src-tauri/src/session.rs:199-212,374-561`.
- D4 change site: `crates/rollshot-capture/src/linux/portal.rs:258-259,280-286`.
- Roadmap: `docs/linux-wayland-layer-shell-roadmap.md` (Phase 3).
