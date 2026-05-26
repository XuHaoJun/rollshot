# Native Scroll Passthrough Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Make manual scroll stitching use native user input through the overlay instead of enigo-generated wheel events.

**Architecture:** After a region is confirmed and stitching starts, the Tauri overlay window stays mouse-through while a frontend timer polls status and preview frames. Esc finishes stitching and opens the existing save dialog instead of canceling capture. Enigo remains removable from the manual path; auto-scroll is out of scope.

**Tech Stack:** Tauri v2, React/Vitest, Rust commands, existing rollshot capture/session APIs.

---

### Task 1: Frontend Behavior Tests

**Files:**
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`

- [x] **Step 1: Add a mocked Tauri window method**

Update the `@tauri-apps/api/window` mock so tests can assert `setIgnoreCursorEvents`:

```ts
const tauriWindow = vi.hoisted(() => ({
  outerPosition: vi.fn(() => Promise.resolve({ x: 0, y: 0 })),
  scaleFactor: vi.fn(() => Promise.resolve(2)),
  setIgnoreCursorEvents: vi.fn(() => Promise.resolve(undefined)),
}))
```

- [x] **Step 2: Add tests for native passthrough and Esc save**

Add tests showing that stitching mode enables cursor passthrough and that Esc stops stitching, opens the save dialog, and saves when a path is selected.

- [x] **Step 3: Run tests and verify failure**

Run: `rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay`

Expected: new tests fail because `setIgnoreCursorEvents` is not called persistently and Esc still cancels.

### Task 2: Native Passthrough API

**Files:**
- Modify: `crates/rollshot-app/src/api/capture.ts`
- Modify: `crates/rollshot-app/src-tauri/src/scroll.rs`
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs`

- [x] **Step 1: Add a tiny frontend wrapper**

Add:

```ts
export async function setInputPassthrough(enabled: boolean): Promise<void> {
  await invoke('set_input_passthrough', { enabled })
}
```

- [x] **Step 2: Add a Tauri command**

Add:

```rust
#[tauri::command]
pub async fn set_input_passthrough(window: tauri::Window, enabled: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(enabled)
        .map_err(|err| format!("failed to set input passthrough: {err}"))
}
```

Register it in `tauri::generate_handler!`.

### Task 3: CaptureOverlay Flow

**Files:**
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`

- [x] **Step 1: Enable passthrough during stitching**

When `status.state === 'stitching'`, call `setInputPassthrough(true)` and clean up with `setInputPassthrough(false)` when leaving stitching or unmounting.

- [x] **Step 2: Replace wheel-driven scrolling**

Remove the `onWheel` manual scroll path from the overlay. Let user wheel events reach the underlying app while the existing 160ms status/preview poll keeps observing frames.

- [x] **Step 3: Make Esc finish and save**

Change the global Esc handler to call the same finish-and-save flow as stop/save: stop stitching, fetch final preview, open the save dialog, and save if the user chooses a path.

- [x] **Step 4: Run frontend verification**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay
rtk pnpm --dir crates/rollshot-app run typecheck
```

### Task 4: Rust Verification

**Files:**
- Verify only unless compiler reports a needed cleanup.

- [x] **Step 1: Run Rust checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo test -p rollshot-app
```

- [x] **Step 2: Remove dead enigo manual path if unused**

If `scroll_through` is no longer referenced, remove its frontend wrapper, Rust command registration, `EnigoState`, and the enigo dependency from `crates/rollshot-app/src-tauri/Cargo.toml`.
