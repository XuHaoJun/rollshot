# rollshot v0.5 — CLI-Launched Interactive Capture Design

> 狀態：draft  
> 取代：`docs/superpowers/specs/2026-05-23-rollshot-v05-interactive-capture-design.md`  
> 前置：v0.4 FAST+KNN fallback capture/stitch pipeline 已可用  
> 參考：`learn-projects/snow-shot`、`learn-projects/tauri-template`

---

## 1. 問題

v0.4 的 capture command 是臨時 headless interface：

```text
rollshot capture --output out.png --fps 5 --max-frames 100 --region "X,Y WxH"
```

它可以驗證 backend 和 stitcher，但不適合一般截長圖工作：

1. 使用者必須事先知道 region、fps、max-frames。
2. KDE 6 portal region picker 的 tooltip 可能污染 first frame。
3. 沒有 live preview，使用者不知道實際截到哪個 source。
4. 沒有互動式 stop，只能等 `--max-frames`。
5. CLI flow 不能自然支援 save/copy/result preview。

v0.5 要把 `rollshot capture` 變成正式 interactive entrypoint。純 CLI 行為保留，但只在 `--headless` 下使用。

---

## 2. 成功條件

v0.5 第一個可交付版本完成時：

```text
rollshot capture
→ CLI 啟動 rollshot-app
→ GUI 觸發 portal/source picker
→ GUI 顯示 live preview
→ 使用者拖拉選區
→ 使用者開始 capture 並手動捲動內容
→ 使用者按 Stop
→ GUI 產生 stitched PNG 並可保存
```

`rollshot capture --headless --output out.png ...` 繼續支援目前自動化和測試用途。

---

## 3. 固定需求

### 3.1 CLI entrypoint

`rollshot capture` 預設是 interactive mode。它不要求 `--output`，也不直接執行 headless stitch loop。

`rollshot capture --headless` 是純 CLI mode。它沿用現有 options：

- `--output` required
- `--backend`
- `--region`
- `--fps`
- `--max-frames`
- `--show-cursor`
- diagnostics / debug report flags

如果使用者在 non-headless mode 傳入 headless-only flags，CLI 可以把有意義的設定轉交 GUI；不能安全轉交的 debug flags 應該報錯，而不是 silently ignore。

### 3.2 GUI ownership

Interactive session 由 `rollshot-app` 擁有。`rollshot-cli` 只負責啟動 GUI process，不參與 Tauri IPC、capture state、stitch state 或 save/copy flow。

### 3.3 Dependency boundary

`rollshot-cli` 不直接依賴 Tauri。這讓 `--headless` 路徑維持輕量，也避免 CLI binary 被 GUI runtime 和 web assets 綁死。

---

## 4. 推薦架構

### 4.1 Workspace shape

```text
crates/
  rollshot-core/       # stitching engine; v0.5 不為 GUI 特化
  rollshot-capture/    # capture backends, FrameStream, crop utility
  rollshot-cli/        # clap entrypoint; headless runner + GUI launcher
  rollshot-app/        # Tauri app; interactive session owner
```

`rollshot-app` 會從目前 placeholder 改造成 Tauri v2 app。參考 `tauri-template` 的乾淨 scaffold：`src-tauri/` 放 Rust app，`src/` 放 React frontend。參考 `snow-shot` 的 screenshot/scroll screenshot patterns：動態建立透明視窗、用 Tauri command 傳 binary frame data。

### 4.2 Process boundary

```text
rollshot capture
  └─ rollshot-cli
       ├─ parse shared launch flags
       ├─ find rollshot-app binary
       └─ spawn rollshot-app --capture [serialized launch options]

rollshot-app
  ├─ Tauri runtime
  ├─ capture backend via rollshot-capture
  ├─ stitcher via rollshot-core
  └─ React overlay/control UI
```

CLI launcher errors must be direct and actionable:

- `rollshot-app` binary not found
- GUI failed to start
- unsupported host/session for interactive capture
- invalid combination of `--headless` and GUI-only flags

### 4.3 Shared launch options

Use a small serializable launch struct in `rollshot-capture`. Both `rollshot-cli` and `rollshot-app` already depend on capture concepts, so a new shared crate is not justified for v0.5.

Initial fields:

```rust
struct InteractiveLaunchOptions {
    backend: String,
    fps: u32,
    show_cursor: bool,
}
```

Do not add configuration for preview layout, hotkeys, clipboard, or multi-monitor polish in v0.5 unless implementation proves it is required.

---

## 5. Interactive Session Design

### 5.1 State machine

```rust
enum SessionState {
    Idle,
    SelectingSource,
    Previewing {
        latest_frame: Option<FramePreview>,
    },
    SelectingRegion {
        latest_frame: FramePreview,
    },
    Stitching {
        region: Region,
        stats: StitchStats,
    },
    Done {
        image_size: Size,
        output_path: Option<PathBuf>,
    },
    Failed {
        message: String,
    },
}
```

The backend owns the real `FrameStream`; the frontend sees only serializable state and preview buffers.

### 5.2 Data flow

```text
Tauri command: start_capture
  → backend.start(CaptureOptions)
  → reader task keeps latest frame
  → frontend polls get_latest_preview at a bounded cadence

Tauri command: confirm_region
  → validate frontend region against frame dimensions
  → store region for crop/stitch

Tauri command: start_stitching
  → background stitch loop consumes latest frames
  → crop_frame(frame, region)
  → Stitcher::push_frame(cropped.image)
  → emit stats / thumbnail updates

Tauri command: stop_capture
  → stop reader/stitch loop
  → keep final image in backend state

Tauri command: save_image
  → write PNG to selected path
```

The UI never receives full-resolution frames during stitching. It receives resized preview images and stitch stats. Final image stays in Rust state until save/copy.

### 5.3 Frame transfer

Initial implementation uses Tauri v2 binary IPC:

```text
RgbaImage → resize preview → PNG encode → tauri::ipc::Response::new(buffer)
frontend invoke<ArrayBuffer>() → Blob URL → <img> or canvas
```

This is supported by the snow-shot pattern for screenshot and scroll screenshot commands. SharedBuffer / zero-copy transfer is explicitly out of v0.5 unless preview IPC becomes the measured bottleneck.

### 5.4 Region selection

The first version uses a Tauri transparent overlay window and a Canvas2D selection layer:

- display the live preview of the selected source
- draw dimmed outside region
- draw resize handles
- show region dimensions
- confirm or cancel

The selected region is in source-frame pixel coordinates. HiDPI conversion must be tested explicitly because browser CSS pixels, Tauri window coordinates, and captured frame pixels can differ.

---

## 6. Platform Strategy

### 6.1 Linux Wayland

Linux Wayland is the primary validation target.

Portal source selection remains in `rollshot-capture`. The GUI should prefer source/window selection through portal and do its own region selection after frames arrive. This avoids relying on KDE's portal region picker for precise crop and avoids the tooltip-first-frame issue by delaying stitch start until after GUI region confirmation.

PipeWire has no general window-exclusion mechanism. Therefore:

- During region selection, overlay UI can exist because stitching has not started.
- During active stitching, UI must not overlap the selected capture region.
- If the selected region is effectively full-screen and no safe place exists for controls, v0.5 should fall back to a minimal non-overlapping control surface or require keyboard/terminal stop only if that path is explicitly verified.

The old spec's blanket "hide overlay for Linux full-screen" is too broad for the main design. Treat full-screen Linux as a risk path with a conservative fallback, not as the baseline UX.

### 6.2 macOS

macOS remains supported through the existing ScreenCaptureKit/scap backend when built with the macOS feature. The app can later use exclusion mechanisms for app windows, but v0.5 should not depend on macOS-only exclusion to make the core flow work.

The primary macOS requirement is that the GUI architecture does not block using SCK in `rollshot-app`.

### 6.3 Windows

Windows is out of v0.5 scope.

---

## 7. Frontend Scope

Use React because both references already use React and the tauri-template is React-oriented.

Use shadcn/Tailwind as scaffold defaults, not as product requirements. The interactive capture UI needs only a small set of controls:

- source/capture status
- selection overlay
- Start
- Stop
- Save
- Cancel

Use Node.js 24 through an app-local mise config and use pnpm for frontend package management, matching snow-shot's package-manager choice. Use Vite for the v0.5 app scaffold rather than snow-shot's Rsbuild/TanStack Router stack; the v0.5 UI is a small single-screen capture flow, and shadcn/Tailwind's Vite path is the lower-risk default.

Avoid building a general settings shell, tray app, updater, command palette, global preferences, or i18n in v0.5.

---

## 8. Error Handling

Interactive errors should be shown in the GUI when Tauri has started:

- capture backend unavailable
- portal denied/cancelled
- no frames received
- invalid selected region
- stitcher produced no output
- save failed

Launcher errors happen before GUI startup and should print to stderr with non-zero exit:

- `rollshot-app` not found
- child process spawn failed
- invalid CLI flag combination

Headless errors keep the current CLI error behavior.

---

## 9. Testing And Verification

### 9.1 Rust unit tests

Add focused tests for:

- CLI argument behavior:
  - `rollshot capture` no longer requires `--output`
  - `rollshot capture --headless` requires `--output`
  - GUI-incompatible debug flags are rejected outside headless mode
- launcher option serialization
- `crop_frame` bounds and metadata updates
- session state transitions that do not require a real portal

### 9.2 Frontend tests

Add lightweight tests only where they catch real risk:

- region coordinate conversion
- selection drag/resize math
- command wrapper result handling

### 9.3 Manual Linux verification

On KDE 6 Wayland:

1. `rollshot capture` starts GUI through CLI.
2. Portal source selection succeeds.
3. Live preview appears.
4. Region selection matches captured source pixels, including HiDPI.
5. Stitching starts only after region confirmation.
6. Stop produces a PNG.
7. GUI controls do not appear in stitched region for normal region capture.
8. `rollshot capture --headless --output out.png ...` still works.

### 9.4 Quality gates

For Rust changes:

```text
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

For frontend changes, use the package manager selected by the app scaffold and provide equivalent typecheck/test/build checks in the implementation plan.

---

## 10. Implementation Plan Strategy

Implementation must be split into three separate plan documents. Do not collapse this spec into one large implementation plan.

Each plan must produce working, testable software on its own, and the next plan should be written only after the previous plan is implemented and verified. This matters because the GUI/portal/HiDPI behavior in Plan 2 can change the exact session-state details needed by Plan 3.

### Plan 1: CLI Launcher and Headless Split

Scope:

- change `rollshot capture` into the interactive entrypoint
- add `rollshot capture --headless`
- make `--output` required only for headless mode
- add GUI launcher discovery/spawn behavior
- preserve existing headless capture behavior and tests

Do not build the Tauri UI in this plan.

### Plan 2: Tauri App Scaffold, Live Preview, and Region Selection

Scope:

- convert `rollshot-app` from placeholder to Tauri v2 app
- wire React frontend scaffold
- call capture backend from Tauri commands
- display bounded-cadence live preview
- implement source-pixel region selection and HiDPI tests

Do not implement full stitching lifecycle in this plan.

### Plan 3: Interactive Stitch, Stop, Save, and Minimal Polish

Scope:

- crop selected region
- run stitch loop under GUI session state
- stop on user action
- keep final image in backend state
- save PNG
- add only the minimal polish needed to complete and verify the workflow

Plan 3 may include the conservative full-screen Linux fallback if Plan 2 proves it is needed.

---

## 11. Milestones

### Milestone 1: Launcher and headless split

Goal: `rollshot capture` launches `rollshot-app`; `--headless` still runs current CLI flow.

Verification:

- CLI tests cover `--output` and `--headless` behavior.
- launcher failures are clear.
- existing headless capture tests still pass.

### Milestone 2: Tauri scaffold, live preview, and region selection

Goal: GUI starts capture backend, receives frames, displays preview, and returns a valid source-pixel region.

Verification:

- `rollshot-app` builds as a Tauri app.
- KDE 6 Wayland manual test sees live frame.
- HiDPI region conversion is tested.
- no stitching starts before region confirmation.

### Milestone 3: Interactive stitching and stop

Goal: selected region is cropped, stitched, stopped by user, and saved as PNG.

Verification:

- manual scroll capture produces usable output.
- final image stays in backend state until saved.
- headless capture still passes existing tests.

### Milestone 4: Minimal polish

Goal: improve only the workflow gaps found in Milestones 1-3.

Allowed polish:

- better progress text
- one safe stop shortcut if needed
- clearer save/cancel states
- conservative full-screen Linux fallback

Not included:

- auto-scroll
- Windows support
- general settings UI
- tray/background app
- command palette
- updater
- SharedBuffer optimization

---

## 12. Differences From The Previous Spec

This replacement keeps the previous spec's main product direction but changes the design discipline:

1. `rollshot capture` launching GUI is a fixed requirement, not an open option.
2. `rollshot-cli` is a thin launcher for interactive mode and does not depend on Tauri.
3. `rollshot-app` owns the interactive session end to end.
4. shadcn/Tailwind are scaffold preferences, not architecture constraints.
5. Frame IPC starts with binary ArrayBuffer and defers SharedBuffer until measured need.
6. Linux full-screen capture is a risk path with a fallback, not the baseline UX.
7. Implementation is explicitly split into three plan documents with verification between plans.
