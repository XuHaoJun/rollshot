# Phase 4: Tauri Save Handoff (Design Spec)

Status: design spec for Phase 4 of `docs/linux-wayland-layer-shell-roadmap.md`.
Scope: wire the native Linux layer-shell overlay (`rollshot-overlay`, Phase 3)
into the Tauri app so the existing save-dialog flow runs after capture, and
resolve the Phase 3 carry-over items that belong to the Tauri host (R2 focus,
driver/overlay thread cleanup). This is a **wiring** spec — it does not modify
the overlay crate, the capture pipeline, or stitching.

All file:line references are evidence captured during design and may drift;
verify against code before relying on them.

## Goal

Preserve the current end-user behavior after the native Linux capture finishes,
completing the roadmap's end-to-end flow on KDE 6 Wayland:

```text
crop select -> scroll with live stitching preview -> Esc -> Tauri save dialog
```

Today the native overlay is driven only by the throwaway harness binary
(`crates/rollshot-overlay/src/bin/capture_overlay.rs`), which saves the result
straight to a PNG file. Phase 4 replaces that harness with the real Tauri host:
on Linux the app runs `run_overlay`, then hands the finalized image to the
**existing** save dialog + `save_image` flow.

## Non-Goals (this phase)

Carried-over Phase 3 items that are **explicitly deferred** to separate
follow-ups and are NOT addressed here:

- **R5/R7 multi-output** (overlay output == captured monitor; multi-monitor
  predictability). Remains a Phase 3 runtime-acceptance follow-up.
- **Live-stitch stall durable fix** (re-anchor after a `NoMatch` streak in
  `rollshot-core`). This phase keeps the Phase 3 mitigation only (see P4.6).
- **R4 fractional scaling** runtime verification (100%/150%).

Also out of scope (from the roadmap): Tauri image editor, clipboard, video/GIF
export, multi-output export UI, settings UI redesign, GNOME Wayland, X11, hard
non-KDE guarantees, arbitrary window-capture crop mapping.

## Locked Decisions

### P4.1 Orchestration: frontend-driven, reuse the existing JS save flow

The frontend orchestrates the Linux flow and reuses the **existing** save-dialog
path (`@tauri-apps/plugin-dialog` `save()` + the `save_image` command +
`AppSession::save_image`, `src-tauri/src/session.rs:233-247`). The native overlay
produces the image; the only new responsibility is getting that image into
`AppSession`'s final-image slot, after which the current save flow runs
unchanged.

Rejected alternative — a backend-only flow that opens the save dialog from Rust
via `tauri_plugin_dialog`'s Rust API — was declined because it would create a
second save-dialog implementation (JS for Windows/macOS, Rust for Linux) and
diverge the Linux save UX from the other platforms. Model A keeps **one**
save-dialog implementation.

### P4.2 Native overlay replaces the webview capture UI on Linux only

On Linux the app uses the native layer-shell overlay (`run_overlay`) instead of
the webview crop/stitch UI (`CaptureOverlay.tsx` + the `AppSession`
capture/stitch threads). Windows and macOS keep the existing webview flow
unchanged.

The branch is selected by a backend capability flag, **not** a JS platform
check (avoids adding `@tauri-apps/plugin-os` and keeps the decision
unit-testable in Rust, matching the existing "capabilities provided by the
backend" style, e.g. `overlay_exclusion`, `commands.rs:91-96`):

```rust
#[tauri::command]
fn uses_native_overlay() -> bool   // true under cfg(target_os = "linux"), else false
```

The webview capture pipeline in `session.rs` (`start_capture`,
`start_stitching`, `stitch_loop`, etc.) is **kept** (it serves Windows/macOS)
but is simply not invoked on Linux. `CaptureOverlay.tsx` is not modified other
than the shared-helper extraction in P4.3. This keeps the change surgical: no
deletion of cross-platform code.

### P4.3 Handoff API: `run_native_capture` + `store_capture_result` (D5 naming)

Two new seams, both named generically per architecture-spec **D5** (must not
bake in "save PNG only"):

```rust
// src-tauri/src/native_capture.rs  (new, Linux-only behavior)
#[tauri::command]
async fn run_native_capture(
    session: State<'_, Arc<SharedSession>>,
    options: InteractiveLaunchOptions,
) -> Result<Option<DoneImageDto>, String>;
// Ok(Some(done)) -> finished, image stored as the session final image
// Ok(None)       -> user cancelled (Esc before crop / Cancel) — nothing stored
// Err(msg)       -> capture/overlay failure

// src-tauri/src/session.rs  (new method on SharedSession)
fn store_capture_result(&self, result: CaptureResult) -> DoneImageDto;
// sets inner.final_image + stitch stats; session status becomes Done
```

The finalized image bytes stay in Rust (`AppSession.final_image`); the frontend
never carries them. The frontend triggers the write through the **unchanged**
`save_image(path)` command, so the saved PNG is byte-for-byte the current
behavior (same `RgbaImage::save_with_format(.., Png)` encoder,
`session.rs:233-247`).

Shared save helper (minimal, justified refactor for P4.1 reuse): extract the
save-dialog logic currently inline in `CaptureOverlay.tsx`
(`saveCurrentImage`, `CaptureOverlay.tsx:166-183`) into one helper, e.g.
`src/api/save.ts`:

```ts
// promptSaveStitchedPng(): open save() dialog, call saveImage(path) if chosen.
```

Both `CaptureOverlay` (webview path) and the new `NativeCaptureFlow` (Linux
path) call this one helper. No second save implementation is introduced.

### P4.4 R2 focus: the Tauri host window stays hidden on Linux

The native overlay uses `KeyboardInteractivity::Exclusive`
(`overlay.rs:601`). The Phase 2/3 spike showed KWin lets a focusable Tauri
toplevel steal keyboard focus from the exclusive-keyboard layer. Mitigation:

- The main window is already created hidden (`tauri.conf.json` `"visible":
  false`). On Linux, `setup` MUST NOT show or focus it — i.e. skip the
  `configure_overlay_window` show/focus path (`overlay.rs:3-12`, `lib.rs:36-44`)
  on `cfg(target_os = "linux")`. The window (and its webview) stays alive so the
  R1 GPU/wgpu coexistence holds and the orchestration JS keeps running; it is
  never mapped as a focusable surface.
- The native save dialog opens **after** `run_overlay` returns (the overlay
  surface is already destroyed), so there is no focus contention at save time.

Open runtime-acceptance item (verify on KDE 6): `save()` opens correctly with
the host window hidden — the xdg-desktop-portal FileChooser is its own surface,
so this is expected to work, but it must be confirmed on the target session.

### P4.5 Overlay-thread lifecycle and cleanup

`run_overlay` blocks its thread for the whole session (it runs
`Driver::start_capture` synchronously — portal negotiation + first-frame wait —
then the iced event loop, `overlay.rs:576-622`). The Tauri host therefore runs
it on a dedicated `std::thread` and must not orphan it (roadmap Phase 4 item:
"the long-lived Tauri host must join or abort it").

Design:

- `run_native_capture` spawns the overlay thread and awaits its result over a
  one-shot channel (so the Tauri async runtime worker is never blocked). The
  thread's `JoinHandle` is stored in `SharedSession` (a `Mutex<Option<...>>`
  slot, mirroring the existing `reader`/`stitcher` slots, `session.rs:320-327`).
- On normal finish / cancel / error the result arrives over the channel and the
  command joins the handle — no orphan.
- `run_overlay`'s own safety net (`driver.cancel()` if the loop exits without
  finalize/cancel, `overlay.rs:609-613`) guarantees the PipeWire stream and the
  reader/stitch threads are torn down **before** it returns. Once the overlay
  thread returns, capture is fully clean.
- `SharedSession::Drop` (and `stop_capture`) best-effort `join()` the overlay
  handle so an in-flight session is not orphaned on shutdown.

Bound on the un-cancellable window: a thread blocked inside `run_overlay` cannot
be force-aborted, but the blocking portal phase is bounded — the first-frame
wait times out at 5s (`driver.rs:161`, `wait_for_source_size`) and cancelling
the portal picker returns an `Err` that ends `run_overlay`. After the overlay
surface exists, Esc/Cancel always end it.

### P4.6 fps floor of 30 on the native path

The native overlay's live stitching is throughput-sensitive: in a debug build,
low fps lets fast scrolling outrun the matcher and stall the live stitch (Phase
3 finding; the harness defaults to 30fps,
`bin/capture_overlay.rs:13-17`). The CLI/launch default is `fps = 5`
(`launch.rs:20`), which predates the native overlay.

`run_native_capture` builds `OverlayConfig` from `InteractiveLaunchOptions` and
**floors fps at 30** (`fps.max(30)`), so a higher explicit value is still
honored but the stall is not reintroduced. This does not touch the
Windows/macOS launch default. The `OverlayConfig`-from-`InteractiveLaunchOptions`
mapping (including the floor) is a pure function and is unit-tested.

### P4.7 Single-shot lifecycle; clean cancel paths

The app is a single capture session per launch (the CLI spawns
`rollshot-app --capture <json>` per capture, `cmd_capture_launcher.rs:50-72`;
the current flow ends with `window.close()`, `CaptureOverlay.tsx:133-140`).
After the save flow completes (saved, or save dialog cancelled, or capture
cancelled), the app closes the window and exits.

Cancel/error must never leave a capture session running (roadmap acceptance):

- `Ok(None)` (Esc before crop, or the overlay Cancel button): no save dialog;
  close window, exit.
- `Err(msg)`: surface the message briefly, then close window, exit.
- `save()` returns `null` (user cancelled the save dialog): no `save_image`
  call; close window, exit cleanly (matches existing `saveCurrentImage`,
  `CaptureOverlay.tsx:173-179`).

## Component Boundaries

```text
rollshot-overlay            UNCHANGED — run_overlay(OverlayConfig)
  run_overlay -> Ok(Some(CaptureResult { image, stats })) | Ok(None) | Err

crates/rollshot-app/src-tauri  (Linux glue, new)
  native_capture.rs   run_native_capture (async cmd): build OverlayConfig
                      (fps floor 30), spawn overlay std::thread, await result,
                      store_capture_result, join thread, return DoneImageDto
  session.rs          + store_capture_result(CaptureResult) -> DoneImageDto
                      + overlay-thread JoinHandle slot + Drop/stop join
  commands.rs         + uses_native_overlay() -> bool
  lib.rs / overlay.rs  Linux: skip show/focus of the host window (R2)

crates/rollshot-app/src  (frontend)
  App.tsx             branch on usesNativeOverlay(): NativeCaptureFlow | CaptureOverlay
  components/NativeCaptureFlow.tsx  (new) run_native_capture -> promptSaveStitchedPng | close
  api/save.ts         (new) promptSaveStitchedPng() — shared by both flows
  api/capture.ts      + runNativeCapture(), usesNativeOverlay() wrappers
```

Hard constraints preserved (from roadmap / architecture spec): `rollshot-core`
and `rollshot-capture` gain no Tauri/overlay dependency; `rollshot-overlay`
gains no Tauri dependency; the capture pipeline and stitching are not rewritten.

## Data Flow

### Normal path (Linux)

```text
CLI spawns rollshot-app --capture <json>
 -> Tauri starts; main window created hidden; on Linux NOT shown/focused (R2),
    webview loads + runs JS, wgpu/webkit GPU context alive (R1)
 -> App queries usesNativeOverlay() = true -> renders NativeCaptureFlow
 -> runNativeCapture(launchOptions)
      Rust: spawn thread { run_overlay(OverlayConfig { backend, fps>=30, show_cursor }) }
            portal picker on a clean desktop -> user Shares -> first frame
            -> layer-shell overlay above fullscreen -> drag crop -> confirm
            -> live stitch preview (grow-then-follow) -> Esc
      run_overlay -> Ok(Some(CaptureResult))
      command: store_capture_result -> final_image (status Done); join thread
            -> Ok(Some(DoneImageDto))
 -> promptSaveStitchedPng(): save() native dialog -> saveImage(path)
      -> save_image -> AppSession::save_image writes PNG (output matches current)
 -> window.close() -> app exits
```

### Cancel / error paths

```text
run_overlay -> Ok(None)   : command Ok(None) -> NativeCaptureFlow closes window, exits
run_overlay -> Err(msg)   : command Err(msg) -> show message -> close window, exits
save() -> null            : no saveImage -> close window, exits cleanly
```

Stable contracts that do not change: `run_overlay` (Phase 3), `save_image(path)`
-> `AppSession::save_image`, and the `Stitcher`/capture APIs.

## Acceptance Checks

Roadmap Phase 4 (the handoff behavior):

1. User presses Esc during stitching -> stitching stops.
2. The Tauri save dialog opens.
3. User can save the stitched PNG; output matches the current behavior.
4. Cancelling the save dialog does not lose process control or leave the capture
   session running.

End-to-end (KDE 6 Wayland, manual runtime acceptance — deferred to a KDE
session as in Phase 3):

5. Native layer-shell overlay appears above fullscreen apps; crop -> scroll ->
   live preview -> Esc -> save dialog -> saved PNG.
6. R2: the overlay receives keyboard/crop input; the hidden host window does not
   steal focus.
7. The save dialog opens correctly with the host window hidden (P4.4 open item).

Automated (this phase):

8. `OverlayConfig`-from-`InteractiveLaunchOptions` mapping floors fps at 30
   (unit test).
9. `store_capture_result` transitions the session to `Done` and a subsequent
   `save_image` writes the expected PNG (Rust unit test).
10. `uses_native_overlay()` returns true on Linux, false elsewhere (cfg-gated
    unit test).
11. `NativeCaptureFlow`: `Some(done)` -> `save()` + `saveImage` then close;
    `None` -> close, no save; `save()` null -> no `saveImage`, still closes
    (vitest, mirroring `CaptureOverlay.test.tsx:247-305`).
12. `App` renders `NativeCaptureFlow` when `usesNativeOverlay()` is true and
    `CaptureOverlay` otherwise (vitest).

## Testing Strategy

- **Rust unit tests** target the seams, not the iced/portal runtime:
  - pure `OverlayConfig` builder (fps floor, field mapping);
  - `store_capture_result` -> `Done` + `save_image` writes PNG (extend the
    existing save tests, `session.rs:953-982`);
  - `uses_native_overlay()` cfg behavior.
  - The full `run_native_capture` (spawns iced + portal) is exercised by manual
    KDE acceptance, not unit tests.
- **Frontend (vitest)** for `NativeCaptureFlow` and the `App` branch, mocking
  `runNativeCapture`/`usesNativeOverlay`/`save`/`saveImage` exactly as
  `CaptureOverlay.test.tsx` already mocks the capture API + dialog.
- **Workspace gates** (per AGENTS.md): `cargo test`, `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`; for the app,
  `pnpm --dir crates/rollshot-app run typecheck`, `pnpm test`, `pnpm run build`.
  No `rollshot-core` stitching paths change, so the bench gate does not apply.

## Follow-Up

Phase 4 is the last roadmap phase. Items intentionally left open after it:

- R5/R7 multi-output and R4 fractional-scaling runtime acceptance (Phase 3
  carry-over).
- The `rollshot-core` re-anchor robustness fix for the live-stitch stall.
- Retiring the `capture_overlay` harness binary once the Tauri path is the
  runtime-accepted entry point (optional cleanup, not required by this phase).

## References

- Roadmap: `docs/linux-wayland-layer-shell-roadmap.md` (Phase 4 + "Carried into
  Phase 4").
- Phase 1 architecture spec (D5 handoff, D2 process model):
  `docs/superpowers/specs/2026-05-29-linux-wayland-layer-shell-architecture-design.md`.
- Phase 3 spec/plan (native overlay):
  `docs/superpowers/specs/2026-05-30-native-linux-capture-overlay-design.md`,
  `docs/superpowers/plans/2026-05-30-native-linux-capture-overlay.md`.
- Overlay crate: `crates/rollshot-overlay/src/lib.rs` (`run_overlay`,
  `CaptureResult`, `OverlayConfig`), `src/overlay.rs` (event loop, teardown
  safety net), `src/driver.rs` (capture/stitch driver, first-frame timeout),
  `src/bin/capture_overlay.rs` (harness this phase replaces).
- Tauri host: `crates/rollshot-app/src-tauri/src/session.rs` (`AppSession`,
  `save_image`, thread slots), `commands.rs`, `lib.rs`, `overlay.rs`,
  `launch.rs`.
- Frontend save flow: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
  (`saveCurrentImage`, Esc handling), `src/api/capture.ts`.
