# iced_layershell Feasibility Spike — Findings

## Environment
- KDE Plasma version: not installed on this machine (headless VM)
- KWin version: not installed on this machine (headless VM)
- Mesa / GPU: VMware SVGA II Adapter (Mesa 25.2.8, libgl1-mesa-dri)
- Session: XDG_SESSION_TYPE=tty (not Wayland — runtime tests require a Plasma Wayland session)
- iced_layershell dep form: crates.io 0.18 (iced 0.14 + iced_layershell 0.18, resolved from crates.io)
- wry: 0.55.1, tao: 0.35.3 (for coexist bin — matches Tauri v2's webkit2gtk footprint)

## Risk results (filled per task)
| Risk | Task | Result | Notes |
|------|------|--------|-------|
| R6 transparency/layer/Esc | 2 | compiles | Transparent fullscreen overlay compiles. `Color::TRANSPARENT` style, `Layer::Overlay`, all-anchors sizing, `KeyboardInteractivity::Exclusive`. Esc via `keyboard::Key::Named(keyboard::key::Named::Escape)` confirmed at compile time. Runtime observation requires KDE 6 hardware. |
| R1 wgpu coexistence | 3 | compiles | wry 0.55.1 + tao 0.35.3 webkit2gtk webview on main thread + iced_layershell overlay on spawned thread. `build_gtk` used for Wayland-compatible GTK embedding. GTK init handled by tao's EventLoop. Runtime observation requires KDE 6 hardware. |
| R2 focus/clipboard | 3 | compiles | Overlay thread uses `KeyboardInteractivity::Exclusive`; main thread runs tao event loop with `ControlFlow::Wait`. Both threads share the same process; clipboard/focus contention is a runtime concern. Compilation verified; runtime requires KDE 6. |
| R6 controls/text | 4 | compiles | Crop rectangle drag via mouse events (`ButtonPressed`/`CursorMoved`/`ButtonReleased`). Canvas `Program` trait draws rect outline with `stroke_rectangle`. Toolbar with "Finish"/"Cancel" buttons + status text pinned top-right via `Stack`. Toolbar uses sentinel magenta `Color::from_rgba(1.0, 0.0, 1.0, 1.0)` solid background (scanned by Task 7). Requires `iced` feature `canvas`. Note: `ButtonPressed` lacks cursor position in iced 0.14 — drag start is deferred to first `CursorMoved`. |
| R6 input region / scroll passthrough | 5 | compiles | `SetInputRegion(ActionCallback)` from `#[to_layer_message]` confirmed at compile time. Callback receives `&WlRegion`, calls `region.add(x, y, w, h)` to restrict input to the toolbar rectangle. After crop confirmation, the overlay emits `SetInputRegion` to set click-through on the crop area, leaving only the toolbar (top-left ~300x50px, matching the toolbar's drawn position) receptive to input. `events_transparent: false` on the surface; the compositor handles the rest. Runtime scroll passthrough requires KDE 6 Wayland. |
| R6 preview refresh | 6 | compiles | An async `iced::futures::channel::mpsc::unbounded` channel bridges an external producer thread into iced via `Subscription::run` + `rx.map(Message::NewPreview)`. The receiver is parked in a `static Mutex<Option<UnboundedReceiver>>`, consumed once by the subscription. The stream yields (parks) between frames rather than blocking the executor, so `event::listen()` keeps flowing; the producer uses non-blocking `unbounded_send`. Producer thread sends `image::Handle::from_rgba(200,200,...)` every 100ms cycling R/G/B. `Message::NewPreview(handle)` stores the handle; `view` renders it with `iced::widget::image` beneath the crop canvas. Subscription is batched with `event::listen()`. `iced` feature `image` required. |
| R3 self-capture | 7 | compiles | `capture_check` binary spawns overlay on thread, drives portal monitor capture on main thread, scans for sentinel magenta RGBA(255,0,255,255) in captured frame. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R4 fractional scaling | 7 | compiles | Binary reports `source_size`, `effective_region`, and frame pixel dimensions from `FrameMetadata`. Coordinate mapping between overlay logical coords and frame pixel coords requires output scale factor. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R5 output match | 7 | compiles | Binary reports `pixel_format`, `stride`, `backend` from `FrameMetadata` and saves captured frame as PNG. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R7 multi-monitor | 8 | compiles | `main` reads output name from `argv[1]`; if present, uses `StartMode::TargetScreen(name)`, otherwise `StartMode::Active`. Code targets output by name via `StartMode::TargetScreen`; runtime multi-monitor test requires KDE 6 with multiple outputs. |

## Decision

### 1. Go / no-go for `iced_layershell` as the Linux overlay stack

**CONDITIONAL GO.**

Compilation-level evidence is strongly positive: `iced_layershell` 0.18.1 + `iced` 0.14 resolve cleanly from crates.io, all required APIs exist at the type level, and the overlay, input region, keyboard, and channel-driven preview APIs all compile without workarounds. The three spike binaries (`overlay`, `coexist`, `capture_check`) build successfully, and production crate tests (90) continue to pass.

However, **zero runtime verification** has occurred. The critical behaviors — transparency vs opaque fill, above-fullscreen layering, GPU coexistence between wry and iced, and input region scroll passthrough — are all observable only on a real KDE Plasma 6 Wayland session. Compilation confirms API availability; it does not confirm compositor behavior.

**Recommendation:** Proceed with `iced_layershell` as the primary Linux overlay path. The runtime verification tasks (R6 transparency, R1 GPU coexistence, R3 self-capture, R4 scaling) **must** be completed on real KDE 6 Wayland hardware before committing to the full overlay implementation plan. If any of those fail at runtime, the smithay-client-toolkit fallback (see §3) becomes the path.

### 2. Process model

**Primary: in-process spawned thread. Separate-process fallback as insurance.**

The `coexist.rs` binary proves the API shape: overlay on a spawned `std::thread`, wry/tao webview on the main thread. Key details confirmed at compile time:

- `build_gtk` (not `build`) is required for Wayland-compatible GTK embedding — `build` targets X11 only.
- `tao::EventLoop::new()` handles `gtk::init` automatically, so wry's GTK requirement is satisfied without explicit initialization.
- Both threads share the same process; no IPC is needed for the overlay-to-webview data path.

If R1 GPU coexistence fails at runtime (e.g., wgpu/EGL contention between iced's renderer and webkit2gtk's GPU context), the separate-process fallback — overlay in its own process, communicating via a socket or shared memory — remains viable. That fallback is not spiked here; it would need its own feasibility check.

### 3. smithay-client-toolkit fallback trigger

**No compilation-level triggers found.** `iced_layershell` provides all needed APIs at the type level: transparent surface, overlay layer, exclusive keyboard, input region control, and output targeting.

The fallback triggers are **runtime-only**:

- Transparency failure: overlay surface is opaque (black/grey fill) despite `Color::TRANSPARENT` and ARGB surface config.
- Layering failure: overlay does not appear above fullscreen applications.
- GPU contention: iced and wry cannot share a GPU context in-process (EGL/wgpu conflicts).
- Input region failure: `SetInputRegion` does not produce click-through behavior on KWin.

If any of these are observed during runtime testing on KDE 6 Wayland, the next step is to evaluate `smithay-client-toolkit` as a lower-level alternative that bypasses `iced_layershell`'s abstraction.

### 4. KDE-specific behaviors and workarounds discovered

- **`build_gtk` vs `build`:** Wayland requires `build_gtk`; `build` is X11-only. This is a gotcha that would surface as a runtime failure on Wayland if missed.
- **`ButtonPressed` lacks cursor position:** In iced 0.14, `ButtonPressed` mouse events do not include the cursor position. Drag start must be deferred to the first `CursorMoved` event. This is an API limitation, not a KDE-specific issue.
- **GTK init automatic:** `tao::EventLoop::new()` calls `gtk::init` internally, so wry's GTK dependency is satisfied without explicit setup. No conflict with iced's own initialization.
- **`SetInputRegion` via `to_layer_message`:** The `#[to_layer_message]` macro generates the `SetInputRegion(ActionCallback)` variant. The callback receives `&WlRegion`, enabling compositor-level input restriction. This is the correct Wayland-native approach for click-through regions.

### 5. Unresolved risks for Phase 3 overlay spec

| Risk | Status | What's ready | What's pending |
|------|--------|--------------|----------------|
| R3 self-capture | Compiles | Sentinel color scanning logic; `capture_check` binary | Runtime test: does portal capture include the overlay? Is the sentinel pixel absent from the captured frame? |
| R4 fractional scaling | Compiles | Coordinate mapping code structure; `FrameMetadata` reports `source_size` and `effective_region` | Runtime test at 100% and 150% scaling: do logical overlay coords map correctly to frame pixel coords? |
| R6 input region | Compiles | `SetInputRegion` API; `WlRegion.add()` for restricting input rectangles | Runtime test: does scroll passthrough work? Does click-through on the crop area behave correctly on KWin? |
| R6 preview refresh | Compiles | Channel-driven `Subscription::run` pattern; `mpsc` bridge | Runtime test: is refresh smooth? What's the latency from external send to overlay render? |
| R1 GPU coexistence | Compiles | Spawned-thread model; `build_gtk` for Wayland | Runtime test: can iced and wry share GPU context without EGL/wgpu conflicts on Mesa/VMware? |

All runtime tests require a KDE Plasma 6 Wayland session with hardware GPU access (not a headless VM).
