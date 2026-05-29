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
| R6 input region / scroll passthrough | 5 | compiles | `SetInputRegion(ActionCallback)` from `#[to_layer_message]` confirmed at compile time. Callback receives `&WlRegion`, calls `region.add(x, y, w, h)` to restrict input to the toolbar rectangle. After crop confirmation, the overlay emits `SetInputRegion` to set click-through on the crop area, leaving only the toolbar (top-right ~300x50px) receptive to input. `events_transparent: false` on the surface; the compositor handles the rest. Runtime scroll passthrough requires KDE 6 Wayland. |
| R6 preview refresh | 6 | compiles | `std::sync::mpsc` channel bridges an external producer thread into iced via `Subscription::run` + `futures::stream::unfold`. The receiver is parked in a `static Mutex<Option<Receiver>>`, consumed once by the subscription. Producer thread sends `image::Handle::from_rgba(200,200,...)` every 100ms cycling R/G/B. `Message::NewPreview(handle)` stores the handle; `view` renders it with `iced::widget::image` beneath the crop canvas. Subscription is batched with `event::listen()`. `iced` feature `image` required. |
| R3 self-capture | 7 | compiles | `capture_check` binary spawns overlay on thread, drives portal monitor capture on main thread, scans for sentinel magenta RGBA(255,0,255,255) in captured frame. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R4 fractional scaling | 7 | compiles | Binary reports `source_size`, `effective_region`, and frame pixel dimensions from `FrameMetadata`. Coordinate mapping between overlay logical coords and frame pixel coords requires output scale factor. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R5 output match | 7 | compiles | Binary reports `pixel_format`, `stride`, `backend` from `FrameMetadata` and saves captured frame as PNG. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R7 multi-monitor | 8 | | |

## Decision
<filled in Task 9>
