# Phase 4: Tauri Save Handoff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Linux, drive the native Wayland layer-shell overlay (`rollshot-overlay`) from the Tauri app and feed its finalized image into the existing save-dialog flow, completing the roadmap's end-to-end capture-to-save behavior.

**Architecture:** Model A (frontend-orchestrated). A new async Tauri command `run_native_capture` runs `run_overlay` on a dedicated thread and stores the resulting image as the session's final image; the frontend then reuses the existing `save()` dialog + `save_image` path. Linux selects this path via a backend flag `uses_native_overlay()`; Windows/macOS keep the webview capture UI unchanged. The host window stays hidden on Linux (R2) so the exclusive-keyboard overlay keeps focus.

**Tech Stack:** Rust (Tauri v2, `tokio::sync::oneshot`, `rollshot-overlay`), TypeScript/React (Vitest), `@tauri-apps/plugin-dialog`.

**Spec:** `docs/superpowers/specs/2026-05-30-phase4-tauri-save-handoff-design.md`

---

## Notes on spec refinements (read before starting)

Two small, deliberate refinements to the spec's P4.3 / P4.5 wording, made for correctness and layering. They preserve the spec's intent:

- **P4.5 thread cleanup:** the spec described a `JoinHandle` slot in `SharedSession` plus a `Drop`-time join. This plan instead keeps the overlay thread's `JoinHandle` **local to `run_native_capture`**, which `.await`s the result and then joins it. The host therefore always joins the thread (never orphans it) — that *is* the cleanup. A `Drop`-time join is intentionally omitted because the overlay thread can block on user input, and a blocking join in `Drop` would hang process shutdown.
- **P4.3 handoff signature:** `store_capture_result` takes `(image: RgbaImage, stats: StitchStats)` (core types `AppSession` already uses) rather than `rollshot_overlay::CaptureResult`, so `session.rs` gains no dependency on the overlay crate. The command destructures `CaptureResult` at the boundary.

## File Structure

**Rust (`crates/rollshot-app/src-tauri`):**
- `Cargo.toml` — add the `rollshot-overlay` dependency.
- `src/native_capture.rs` (new) — `overlay_config()` (fps floor), `uses_native_overlay()` command, `run_native_capture()` async command. Owns the native-overlay handoff.
- `src/session.rs` — add `AppSession::set_final_image()` + `SharedSession::store_capture_result()`.
- `src/lib.rs` — register the new commands; gate the host-window overlay setup to non-Linux (R2).

**Frontend (`crates/rollshot-app/src`):**
- `api/capture.ts` — add `runNativeCapture()` and `usesNativeOverlay()` wrappers.
- `api/save.ts` (new) — `promptSaveStitchedPng()` shared save helper.
- `components/CaptureOverlay.tsx` — refactor `saveCurrentImage` to call the shared helper.
- `components/NativeCaptureFlow.tsx` (new) — the Linux orchestration component.
- `App.tsx` — branch on `usesNativeOverlay()` between `NativeCaptureFlow` and `CaptureOverlay`.

All shell commands are prefixed with `rtk` per the repo convention.

---

## Task 1: Add the `rollshot-overlay` dependency to the Tauri app

**Files:**
- Modify: `crates/rollshot-app/src-tauri/Cargo.toml:16-27`

- [ ] **Step 1: Add the dependency**

In `[dependencies]`, after the `rollshot-overlay-core` line (`Cargo.toml:20`), add:

```toml
rollshot-overlay = { path = "../../rollshot-overlay" }
```

(`rollshot-overlay` compiles to a stub on non-Linux and only builds `iced`/`iced_layershell` under `cfg(target_os = "linux")`, so this dependency is safe on every platform.)

- [ ] **Step 2: Verify it builds**

Run: `rtk cargo build -p rollshot-app`
Expected: builds successfully (the crate resolves; no code uses it yet).

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-app/src-tauri/Cargo.toml
rtk git commit -m "build(app): depend on rollshot-overlay for native capture"
```

---

## Task 2: `store_capture_result` on the session

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/session.rs` (add `AppSession::set_final_image`, `SharedSession::store_capture_result`, and tests)

This task is independent and compiles on its own; it lands the session-side handoff seam the native command (Task 3) calls into.

- [ ] **Step 1: Write the failing tests**

In `crates/rollshot-app/src-tauri/src/session.rs`, inside the existing `#[cfg(test)] mod tests` block, add these two tests (place them after `save_image_writes_final_png`, around `session.rs:982`):

```rust
    #[test]
    fn store_capture_result_sets_done_image() {
        use rollshot_core::StitchStats;

        let session = SharedSession::new();
        let image = RgbaImage::from_pixel(40, 90, Rgba([1, 2, 3, 255]));

        let done = session
            .store_capture_result(image, StitchStats::default())
            .expect("store capture result");

        assert_eq!(done.image_width, 40);
        assert_eq!(done.image_height, 90);
        assert_eq!(done.output_path, None);

        match session.status().expect("status") {
            SessionStatus::Done {
                image_width,
                image_height,
                output_path,
            } => {
                assert_eq!(image_width, 40);
                assert_eq!(image_height, 90);
                assert_eq!(output_path, None);
            }
            other => panic!("expected done status, got {other:?}"),
        }
    }

    #[test]
    fn store_capture_result_then_save_writes_png() {
        use rollshot_core::StitchStats;

        let dir = std::env::temp_dir()
            .join(format!("rollshot-native-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create tempdir");
        let out = dir.join("native.png");

        let session = SharedSession::new();
        session
            .store_capture_result(
                RgbaImage::from_pixel(60, 120, Rgba([9, 9, 9, 255])),
                StitchStats::default(),
            )
            .expect("store capture result");

        let saved = session.save_image(&out).expect("save png");

        assert_eq!(saved.output_path, Some(out.to_string_lossy().to_string()));
        let decoded = image::open(&out).expect("decode saved png");
        assert_eq!(decoded.width(), 60);
        assert_eq!(decoded.height(), 120);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-app store_capture_result`
Expected: FAIL — `no method named store_capture_result found` (compile error).

- [ ] **Step 3: Implement `set_final_image` and `store_capture_result`**

In `crates/rollshot-app/src-tauri/src/session.rs`, add this method to the `impl AppSession` block (after `save_image`, around `session.rs:247`):

```rust
    fn set_final_image(&mut self, image: RgbaImage, stats: StitchStats) -> DoneImageDto {
        let done = DoneImageDto {
            image_width: image.width(),
            image_height: image.height(),
            output_path: self.output_path.clone(),
        };
        self.final_image = Some(image);
        self.stitch_stats = StitchStatsDto::from(stats);
        self.error = None;
        done
    }
```

Then add this method to the `impl SharedSession` block (after `save_image`, around `session.rs:600`):

```rust
    /// Store a finalized capture (from the native Linux overlay) as the session
    /// final image so the existing save flow can write it. Generic handoff
    /// (spec D5): not specific to PNG saving.
    pub fn store_capture_result(
        &self,
        image: RgbaImage,
        stats: StitchStats,
    ) -> Result<DoneImageDto, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        Ok(inner.set_final_image(image, stats))
    }
```

(`RgbaImage` is already imported at `session.rs:9`; `StitchStats` at `session.rs:14`. No new imports outside the test module.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-app store_capture_result`
Expected: PASS — both `store_capture_result_sets_done_image` and `store_capture_result_then_save_writes_png` pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src-tauri/src/session.rs
rtk git commit -m "feat(app): session store_capture_result handoff seam"
```

---

## Task 3: `native_capture` module + command registration

**Files:**
- Create: `crates/rollshot-app/src-tauri/src/native_capture.rs`
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs:1-9` (declare `mod native_capture;`)
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs:46-60` (register the two commands)

This task compiles on its own because Task 2 added `store_capture_result`. Registering the commands in the same task keeps them from tripping the `dead_code` lint.

- [ ] **Step 1: Create the module with the fps-floor builder, the flag command, the async handoff command, and tests**

Create `crates/rollshot-app/src-tauri/src/native_capture.rs`:

```rust
use std::sync::Arc;

use rollshot_capture::InteractiveLaunchOptions;
use rollshot_overlay::{run_overlay, OverlayConfig};

use crate::session::{DoneImageDto, SharedSession};

/// Minimum capture fps for the native overlay path. The live stitcher is
/// throughput-sensitive: in a debug build, lower fps lets fast scrolling outrun
/// the matcher and stall the live stitch (Phase 3 finding). The CLI/launch
/// default (`fps = 5`) predates the native overlay, so floor it here. This does
/// not affect the Windows/macOS webview path.
const NATIVE_OVERLAY_MIN_FPS: u32 = 30;

/// Build the native overlay config from the launch options, flooring fps so the
/// live stitch stays smooth.
fn overlay_config(options: &InteractiveLaunchOptions) -> OverlayConfig {
    OverlayConfig {
        backend: options.backend.clone(),
        fps: options.fps.max(NATIVE_OVERLAY_MIN_FPS),
        show_cursor: options.show_cursor,
    }
}

/// Whether this build uses the native Wayland layer-shell overlay (Linux) or
/// the webview capture UI (Windows/macOS). Drives the frontend's top-level
/// branch instead of a JS platform check.
#[tauri::command]
pub fn uses_native_overlay() -> bool {
    cfg!(target_os = "linux")
}

/// Linux save handoff (Phase 4): run the native layer-shell overlay to capture
/// + stitch, then store the finalized image as the session's final image so the
/// existing save flow can write it. `run_overlay` blocks its thread for the
/// whole session, so it runs on a dedicated thread and the result is awaited
/// without blocking the async runtime. Named generically (spec D5): this is the
/// capture handoff, not "save PNG".
#[tauri::command]
pub async fn run_native_capture(
    session: tauri::State<'_, Arc<SharedSession>>,
    options: InteractiveLaunchOptions,
) -> Result<Option<DoneImageDto>, String> {
    let config = overlay_config(&options);
    let (tx, rx) = tokio::sync::oneshot::channel();

    // run_overlay blocks (portal negotiation + first-frame wait + iced loop);
    // a dedicated std::thread keeps the Tauri async worker free.
    let handle = std::thread::spawn(move || {
        let _ = tx.send(run_overlay(config));
    });

    let outcome = rx
        .await
        .map_err(|_| "native overlay thread ended without a result".to_string())?;

    // The overlay returned, so the thread is finishing; join it so the host
    // never orphans it (roadmap Phase 4 thread-cleanup item).
    let _ = handle.join();

    match outcome {
        Ok(Some(result)) => {
            let done = session.store_capture_result(result.image, result.stats)?;
            Ok(Some(done))
        }
        Ok(None) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{overlay_config, uses_native_overlay};
    use rollshot_capture::InteractiveLaunchOptions;

    #[test]
    fn overlay_config_floors_fps_at_30() {
        let config = overlay_config(&InteractiveLaunchOptions {
            backend: "linux-portal".to_string(),
            fps: 5,
            show_cursor: true,
        });
        assert_eq!(config.backend, "linux-portal");
        assert_eq!(config.fps, 30);
        assert!(config.show_cursor);
    }

    #[test]
    fn overlay_config_keeps_higher_fps() {
        let config = overlay_config(&InteractiveLaunchOptions {
            backend: "auto".to_string(),
            fps: 60,
            show_cursor: false,
        });
        assert_eq!(config.fps, 60);
        assert_eq!(config.backend, "auto");
        assert!(!config.show_cursor);
    }

    #[test]
    fn uses_native_overlay_matches_target_os() {
        assert_eq!(uses_native_overlay(), cfg!(target_os = "linux"));
    }
}
```

- [ ] **Step 2: Declare the module**

In `crates/rollshot-app/src-tauri/src/lib.rs`, add `mod native_capture;` to the module list at the top (after `mod launch;`, `lib.rs:4`):

```rust
mod commands;
#[cfg(test)]
mod css_token_sync;
mod launch;
mod native_capture;
mod overlay;
mod scroll;
mod session;
#[cfg(target_os = "linux")]
mod webkit_workaround;
```

- [ ] **Step 3: Register the two commands**

In `crates/rollshot-app/src-tauri/src/lib.rs`, add the new commands to `tauri::generate_handler!` (`lib.rs:46-60`), after `commands::overlay_exclusion,`:

```rust
            commands::overlay_exclusion,
            native_capture::run_native_capture,
            native_capture::uses_native_overlay,
            scroll::set_input_passthrough,
```

- [ ] **Step 4: Verify it builds and the module's unit tests pass**

Run: `rtk cargo build -p rollshot-app && rtk cargo test -p rollshot-app native_capture`
Expected: builds with no warnings; `overlay_config_floors_fps_at_30`, `overlay_config_keeps_higher_fps`, `uses_native_overlay_matches_target_os` PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src-tauri/src/native_capture.rs \
            crates/rollshot-app/src-tauri/src/lib.rs
rtk git commit -m "feat(app): native capture handoff command (run_native_capture) + flag"
```

---

## Task 4: Keep the Linux host window hidden (R2)

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs:5,15,36-45`

- [ ] **Step 1: Gate the `overlay` module and `Manager` import to non-Linux**

In `crates/rollshot-app/src-tauri/src/lib.rs`, change the `mod overlay;` declaration (`lib.rs:5`) to:

```rust
#[cfg(not(target_os = "linux"))]
mod overlay;
```

Remove the top-level `use tauri::Manager;` line (`lib.rs:15`) — it is moved into the non-Linux setup helper in Step 2. Keep `use std::sync::Arc;`, `use launch::LaunchMode;`, and `use session::SharedSession;`.

- [ ] **Step 2: Add a platform-split host-window setup helper**

In `crates/rollshot-app/src-tauri/src/lib.rs`, add these two `#[cfg]`-split free functions (place them above `pub fn run()`):

```rust
#[cfg(not(target_os = "linux"))]
fn setup_host_window(app: &mut tauri::App, shared_session: &Arc<SharedSession>) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let overlay_exclusion = overlay::configure_overlay_window(&window);
        shared_session.set_overlay_exclusion(overlay_exclusion);
    }
}

#[cfg(target_os = "linux")]
fn setup_host_window(_app: &mut tauri::App, _shared_session: &Arc<SharedSession>) {
    // R2: the native layer-shell overlay (run_native_capture) owns capture
    // input via an exclusive-keyboard layer surface. The host window must stay
    // hidden/unfocused so it cannot steal that focus (KWin would, per the Phase
    // 2/3 spike). The webview is still created (tauri.conf.json visible:false)
    // so its GPU context stays alive for wgpu/webkit coexistence (R1); we simply
    // never show or focus it.
}
```

- [ ] **Step 3: Use the helper in `setup`**

In `crates/rollshot-app/src-tauri/src/lib.rs`, replace the `.setup(...)` block (`lib.rs:36-45`) with:

```rust
        .setup({
            let shared_session = Arc::clone(&shared_session);
            move |app| {
                setup_host_window(app, &shared_session);
                Ok(())
            }
        })
```

- [ ] **Step 4: Verify it builds and the whole crate's tests pass**

Run: `rtk cargo build -p rollshot-app && rtk cargo test -p rollshot-app`
Expected: builds with no warnings; all existing tests plus the new `native_capture` / `store_capture_result` tests PASS.

- [ ] **Step 5: Verify clippy and fmt are clean for the crate**

Run: `rtk cargo clippy -p rollshot-app --all-targets -- -D warnings && rtk cargo fmt --check`
Expected: no warnings, no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src-tauri/src/lib.rs
rtk git commit -m "feat(app): keep Linux host window hidden during native overlay (R2)"
```

---

## Task 5: Frontend API wrappers

**Files:**
- Modify: `crates/rollshot-app/src/api/capture.ts:97-99` (add two wrappers)
- Modify: `crates/rollshot-app/src/api/capture.test.ts` (add two tests)

- [ ] **Step 1: Write the failing tests**

In `crates/rollshot-app/src/api/capture.test.ts`, add these tests inside the `describe('capture api wrappers', ...)` block (after the `saves final image to selected path` test, `capture.test.ts:37`):

```ts
  it('runs native capture and returns the done image dto', async () => {
    const { runNativeCapture } = await import('./capture')
    invokeMock.mockResolvedValueOnce({
      image_width: 800,
      image_height: 1200,
      output_path: null,
    })

    await expect(
      runNativeCapture({ backend: 'auto', fps: 30, show_cursor: false }),
    ).resolves.toEqual({ image_width: 800, image_height: 1200, output_path: null })
    expect(invokeMock).toHaveBeenCalledWith('run_native_capture', {
      options: { backend: 'auto', fps: 30, show_cursor: false },
    })
  })

  it('returns null when native capture is cancelled', async () => {
    const { runNativeCapture } = await import('./capture')
    invokeMock.mockResolvedValueOnce(null)

    await expect(
      runNativeCapture({ backend: 'auto', fps: 30, show_cursor: false }),
    ).resolves.toBeNull()
  })

  it('reads the native overlay capability flag', async () => {
    const { usesNativeOverlay } = await import('./capture')
    invokeMock.mockResolvedValueOnce(true)

    await expect(usesNativeOverlay()).resolves.toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('uses_native_overlay')
  })
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk pnpm --dir crates/rollshot-app test -- --run src/api/capture.test.ts`
Expected: FAIL — `runNativeCapture`/`usesNativeOverlay` are not exported.

- [ ] **Step 3: Add the wrappers**

In `crates/rollshot-app/src/api/capture.ts`, add after `saveImage` (`capture.ts:99`):

```ts
export async function runNativeCapture(
  options: InteractiveLaunchOptions,
): Promise<DoneImageDto | null> {
  return await invoke<DoneImageDto | null>('run_native_capture', { options })
}

export async function usesNativeOverlay(): Promise<boolean> {
  return await invoke<boolean>('uses_native_overlay')
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk pnpm --dir crates/rollshot-app test -- --run src/api/capture.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/api/capture.ts crates/rollshot-app/src/api/capture.test.ts
rtk git commit -m "feat(app): add runNativeCapture and usesNativeOverlay api wrappers"
```

---

## Task 6: Shared `promptSaveStitchedPng` helper + CaptureOverlay refactor

**Files:**
- Create: `crates/rollshot-app/src/api/save.ts`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx:1-2,166-183`

- [ ] **Step 1: Create the shared save helper**

Create `crates/rollshot-app/src/api/save.ts`:

```ts
import { save } from '@tauri-apps/plugin-dialog'
import { saveImage } from './capture'

/**
 * Open the native save dialog for the stitched PNG and, if the user picks a
 * path, write it via the existing `save_image` command. Shared by the webview
 * (CaptureOverlay) and native (NativeCaptureFlow) capture paths so there is one
 * save-dialog implementation. Returns nothing; if the user cancels the dialog,
 * nothing is written.
 */
export async function promptSaveStitchedPng(
  onMessage?: (message: string) => void,
): Promise<void> {
  const selected = await save({
    title: 'Save stitched PNG',
    defaultPath: 'rollshot.png',
    filters: [{ name: 'PNG image', extensions: ['png'] }],
  })
  if (selected) {
    const done = await saveImage(selected)
    onMessage?.(done.output_path ? `Saved ${done.output_path}` : 'Saved image')
  }
}
```

- [ ] **Step 2: Refactor `CaptureOverlay.saveCurrentImage` to use the helper**

In `crates/rollshot-app/src/components/CaptureOverlay.tsx`, remove the `save` import from `@tauri-apps/plugin-dialog` (`CaptureOverlay.tsx:2`) and add the helper import alongside the other local imports (near `CaptureOverlay.tsx:20-25`):

```ts
import { promptSaveStitchedPng } from '../api/save'
```

Then replace `saveCurrentImage` (`CaptureOverlay.tsx:166-183`) with:

```ts
  const saveCurrentImage = useCallback(async (closeAfter: boolean) => {
    try {
      await promptSaveStitchedPng(setMessage)
      if (closeAfter) {
        await closeOverlay()
      }
    } catch (error) {
      setMessage(String(error))
    }
  }, [closeOverlay])
```

(Behavior is unchanged: the dialog config and `saveImage` call now live in the helper; `setMessage` still receives the saved/`error` message; `closeAfter` still triggers `closeOverlay`.)

- [ ] **Step 3: Run the existing CaptureOverlay tests to verify no regression**

Run: `rtk pnpm --dir crates/rollshot-app test -- --run src/components/CaptureOverlay.test.tsx`
Expected: PASS — the existing tests (`finishes stitching and opens the save dialog on Escape`, `closes after Escape when the save dialog is cancelled`, etc.) still pass because the helper calls the same mocked `save()` + `saveImage()`.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src/api/save.ts crates/rollshot-app/src/components/CaptureOverlay.tsx
rtk git commit -m "refactor(app): extract shared promptSaveStitchedPng save helper"
```

---

## Task 7: `NativeCaptureFlow` component

**Files:**
- Create: `crates/rollshot-app/src/components/NativeCaptureFlow.tsx`
- Create: `crates/rollshot-app/src/components/NativeCaptureFlow.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `crates/rollshot-app/src/components/NativeCaptureFlow.test.tsx`:

```tsx
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { NativeCaptureFlow } from './NativeCaptureFlow'

const api = vi.hoisted(() => ({
  launchOptions: vi.fn(),
  runNativeCapture: vi.fn(),
}))
const saveApi = vi.hoisted(() => ({
  promptSaveStitchedPng: vi.fn(),
}))
const win = vi.hoisted(() => ({
  close: vi.fn(),
}))

vi.mock('../api/capture', () => api)
vi.mock('../api/save', () => saveApi)
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ close: win.close }),
}))

const reactActGlobal = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT: boolean
}
reactActGlobal.IS_REACT_ACT_ENVIRONMENT = true

async function flush() {
  for (let i = 0; i < 6; i += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

describe('NativeCaptureFlow', () => {
  let container: HTMLDivElement
  let root: Root

  beforeEach(() => {
    vi.clearAllMocks()
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    api.launchOptions.mockResolvedValue({ backend: 'auto', fps: 30, show_cursor: false })
    saveApi.promptSaveStitchedPng.mockResolvedValue(undefined)
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
  })

  it('opens the save flow and closes the window when capture finishes', async () => {
    api.runNativeCapture.mockResolvedValue({
      image_width: 800,
      image_height: 1200,
      output_path: null,
    })

    await act(async () => {
      root.render(<NativeCaptureFlow />)
    })
    await flush()

    expect(api.runNativeCapture).toHaveBeenCalledWith({
      backend: 'auto',
      fps: 30,
      show_cursor: false,
    })
    expect(saveApi.promptSaveStitchedPng).toHaveBeenCalledTimes(1)
    expect(win.close).toHaveBeenCalledTimes(1)
  })

  it('closes without saving when capture is cancelled', async () => {
    api.runNativeCapture.mockResolvedValue(null)

    await act(async () => {
      root.render(<NativeCaptureFlow />)
    })
    await flush()

    expect(saveApi.promptSaveStitchedPng).not.toHaveBeenCalled()
    expect(win.close).toHaveBeenCalledTimes(1)
  })

  it('closes the window when capture fails', async () => {
    api.runNativeCapture.mockRejectedValue(new Error('portal denied'))

    await act(async () => {
      root.render(<NativeCaptureFlow />)
    })
    await flush()

    expect(saveApi.promptSaveStitchedPng).not.toHaveBeenCalled()
    expect(win.close).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk pnpm --dir crates/rollshot-app test -- --run src/components/NativeCaptureFlow.test.tsx`
Expected: FAIL — `NativeCaptureFlow` does not exist.

- [ ] **Step 3: Implement the component**

Create `crates/rollshot-app/src/components/NativeCaptureFlow.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { launchOptions, runNativeCapture } from '../api/capture'
import { promptSaveStitchedPng } from '../api/save'

/**
 * Linux capture path (Phase 4). The native Wayland layer-shell overlay
 * (run_native_capture) owns the crop/scroll/preview UI and blocks until the
 * user finishes (Esc) or cancels. On finish we reuse the shared save dialog;
 * then the single-shot session ends by closing the (hidden) host window. The
 * host window stays hidden on Linux (R2), so this component's DOM is never
 * visible — it is the orchestrator, not the UI.
 */
export function NativeCaptureFlow() {
  const [message, setMessage] = useState('Starting capture')
  const startedRef = useRef(false)

  useEffect(() => {
    if (startedRef.current) {
      return
    }
    startedRef.current = true

    void (async () => {
      try {
        const options = await launchOptions()
        const done = await runNativeCapture(options)
        if (done) {
          setMessage(`Stitched ${done.image_width}x${done.image_height}`)
          await promptSaveStitchedPng(setMessage)
        }
      } catch (error) {
        setMessage(String(error))
      } finally {
        await getCurrentWindow().close()
      }
    })()
  }, [])

  return (
    <main className="capture-overlay">
      <div className="capture-status">{message}</div>
    </main>
  )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk pnpm --dir crates/rollshot-app test -- --run src/components/NativeCaptureFlow.test.tsx`
Expected: PASS — all three tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/components/NativeCaptureFlow.tsx \
            crates/rollshot-app/src/components/NativeCaptureFlow.test.tsx
rtk git commit -m "feat(app): NativeCaptureFlow orchestrates the Linux capture-to-save flow"
```

---

## Task 8: `App` branch between native and webview paths

**Files:**
- Modify: `crates/rollshot-app/src/App.tsx`
- Create: `crates/rollshot-app/src/App.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `crates/rollshot-app/src/App.test.tsx`:

```tsx
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'

const api = vi.hoisted(() => ({
  usesNativeOverlay: vi.fn(),
}))
const renders = vi.hoisted(() => ({
  native: vi.fn(),
  webview: vi.fn(),
}))

vi.mock('./api/capture', () => api)
vi.mock('./components/NativeCaptureFlow', () => ({
  NativeCaptureFlow: () => {
    renders.native()
    return null
  },
}))
vi.mock('./components/CaptureOverlay', () => ({
  CaptureOverlay: () => {
    renders.webview()
    return null
  },
}))

const reactActGlobal = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT: boolean
}
reactActGlobal.IS_REACT_ACT_ENVIRONMENT = true

async function flush() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

describe('App', () => {
  let container: HTMLDivElement
  let root: Root

  beforeEach(() => {
    vi.clearAllMocks()
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
  })

  it('renders NativeCaptureFlow when the backend uses the native overlay', async () => {
    api.usesNativeOverlay.mockResolvedValue(true)

    await act(async () => {
      root.render(<App />)
    })
    await flush()

    expect(renders.native).toHaveBeenCalled()
    expect(renders.webview).not.toHaveBeenCalled()
  })

  it('renders CaptureOverlay when the backend uses the webview overlay', async () => {
    api.usesNativeOverlay.mockResolvedValue(false)

    await act(async () => {
      root.render(<App />)
    })
    await flush()

    expect(renders.webview).toHaveBeenCalled()
    expect(renders.native).not.toHaveBeenCalled()
  })

  it('falls back to the webview overlay when the capability query fails', async () => {
    api.usesNativeOverlay.mockRejectedValue(new Error('ipc down'))

    await act(async () => {
      root.render(<App />)
    })
    await flush()

    expect(renders.webview).toHaveBeenCalled()
    expect(renders.native).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk pnpm --dir crates/rollshot-app test -- --run src/App.test.tsx`
Expected: FAIL — `App` still renders `CaptureOverlay` unconditionally, so the native/fallback assertions fail.

- [ ] **Step 3: Implement the branch**

Replace `crates/rollshot-app/src/App.tsx` with:

```tsx
import { useEffect, useState } from 'react'
import { usesNativeOverlay } from './api/capture'
import { CaptureOverlay } from './components/CaptureOverlay'
import { NativeCaptureFlow } from './components/NativeCaptureFlow'

type CaptureMode = 'loading' | 'native' | 'webview'

export default function App() {
  const [mode, setMode] = useState<CaptureMode>('loading')

  useEffect(() => {
    usesNativeOverlay()
      .then((native) => setMode(native ? 'native' : 'webview'))
      .catch(() => setMode('webview'))
  }, [])

  if (mode === 'loading') {
    return null
  }
  return mode === 'native' ? <NativeCaptureFlow /> : <CaptureOverlay />
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk pnpm --dir crates/rollshot-app test -- --run src/App.test.tsx`
Expected: PASS — all three tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/App.tsx crates/rollshot-app/src/App.test.tsx
rtk git commit -m "feat(app): branch App between native and webview capture paths"
```

---

## Task 9: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Rust workspace gates**

Run: `rtk cargo test`
Expected: all workspace tests PASS.

Run: `rtk cargo fmt --check`
Expected: no diff.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings. (No `rollshot-core` stitching path changed, so the benchmark gate does not apply.)

- [ ] **Step 2: Frontend gates**

Run: `rtk pnpm --dir crates/rollshot-app run typecheck`
Expected: no type errors.

Run: `rtk pnpm --dir crates/rollshot-app test`
Expected: all tests PASS (capture api, CaptureOverlay, NativeCaptureFlow, App, plus the rest).

Run: `rtk pnpm --dir crates/rollshot-app run build`
Expected: build succeeds.

- [ ] **Step 3: Commit any formatting fixups (if needed)**

```bash
rtk git add -A
rtk git commit -m "chore(app): verification fixups for phase4 save handoff"
```

(Skip this commit if the working tree is already clean.)

---

## Task 10: Manual KDE 6 Wayland runtime acceptance (deferred, document results)

**Files:** none (manual runtime check on a KDE 6 Wayland session; record outcomes against the roadmap)

This mirrors the Phase 3 deferral: the full `run_native_capture` path drives `iced_layershell` + the xdg portal, which cannot be unit-tested. Run these on a KDE 6 Wayland session and record pass/fail (and any blocker) in the roadmap's Phase 4 status note.

- [ ] **Step 1: Build the app and launch capture**

Build the Tauri app per `README.md` (e.g. `rtk pnpm --dir crates/rollshot-app run tauri build --debug`), then launch a capture session through the CLI launcher (`rollshot` capture subcommand / `rollshot-app --capture <json>`).

- [ ] **Step 2: Walk the end-to-end flow (roadmap acceptance checks 1-5)**

Verify: native layer-shell overlay appears above fullscreen apps; drag a crop; scroll the target while the live preview updates; press Esc; the Tauri save dialog opens; save the PNG; confirm the output matches the current behavior (open the PNG).

- [ ] **Step 3: Verify R2 focus and the hidden-window save dialog (spec P4.4 open item)**

Verify the overlay receives keyboard/crop input (the hidden host window does not steal focus), and that the save dialog opens correctly with the host window hidden.

- [ ] **Step 4: Verify the cancel paths (roadmap acceptance check 4)**

Verify: pressing Esc before confirming a crop exits without a save dialog and leaves no capture session running; cancelling the save dialog does not lose process control and exits cleanly.

- [ ] **Step 5: Record results**

Update the Phase 4 status note in `docs/linux-wayland-layer-shell-roadmap.md` with the runtime outcomes (PASS/FAIL + any blocker). R5/R7 multi-output, R4 fractional scaling, and the live-stitch stall remain the deferred follow-ups named in the spec's Non-Goals.

---

## Self-Review

**Spec coverage:**
- P4.1 (frontend-orchestrated, reuse JS save flow) → Tasks 6 (shared helper), 7 (NativeCaptureFlow), 8 (App branch).
- P4.2 (native replaces webview on Linux; backend flag) → Task 3 (`uses_native_overlay`), Task 8 (App branch). CaptureOverlay + session pipeline kept (only the save helper extracted in Task 6).
- P4.3 (handoff API `run_native_capture` + `store_capture_result`, D5 naming; image stays in Rust; reuse `save_image`) → Task 2 (`store_capture_result`), Task 3 (`run_native_capture`), Task 5 (api wrappers).
- P4.4 (R2 host window hidden on Linux; webview alive for R1) → Task 4.
- P4.5 (overlay thread lifecycle/cleanup) → Task 3 (await + join in `run_native_capture`); deviation from the literal slot/Drop design is documented in "Notes on spec refinements".
- P4.6 (fps floor 30) → Task 3 (`overlay_config`, with tests).
- P4.7 (single-shot lifecycle; clean cancel paths) → Task 7 (close on done/cancel/error), Task 10 (manual verify).
- Acceptance checks (automated): #8 fps floor → Task 3; #9 `store_capture_result` → Task 2; #10 `uses_native_overlay` → Task 3; #11 `NativeCaptureFlow` → Task 7; #12 `App` branch → Task 8. Checks 1-7 (manual KDE) → Task 10.
- Non-Goals (R5/R7, live-stitch stall, R4) → not implemented; recorded in Task 10, Step 5.

**Placeholder scan:** no TBD/TODO; every code step contains complete code; commands have expected output.

**Type consistency:** `run_native_capture(options) -> DoneImageDto | null`, `usesNativeOverlay() -> boolean`, `store_capture_result(image, stats) -> Result<DoneImageDto, String>`, `overlay_config(&InteractiveLaunchOptions) -> OverlayConfig`, `promptSaveStitchedPng(onMessage?)` are used consistently across Rust, the API wrappers, and the components. `InteractiveLaunchOptions`/`DoneImageDto` match the existing `api/capture.ts` types.
