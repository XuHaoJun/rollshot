# iced_layershell Feasibility Spike — Findings

## Environment

### Compile environment (Tasks 1–8 authored here)
- KDE Plasma version: not installed on this machine (headless VM)
- KWin version: not installed on this machine (headless VM)
- Mesa / GPU: VMware SVGA II Adapter (Mesa 25.2.8, libgl1-mesa-dri)
- Session: XDG_SESSION_TYPE=tty (not Wayland — runtime tests require a Plasma Wayland session)
- iced_layershell dep form: crates.io 0.18 (iced 0.14 + iced_layershell 0.18, resolved from crates.io)
- wry: 0.55.1, tao: 0.35.3 (for coexist bin — matches Tauri v2's webkit2gtk footprint)

### Runtime environment (KDE 6 Wayland — where the runtime gates were exercised, 2026-05-29)
- KDE Plasma version: plasmashell 6.6.5
- KWin version: kwin 6.6.5
- GPU: NVIDIA GeForce RTX 5070 Ti (proprietary driver) — note: NVIDIA is the
  hardest case for wgpu (iced) + webkit2gtk GPU coexistence, so R1 evidence here
  is strong.
- Session: XDG_SESSION_TYPE=wayland, XDG_CURRENT_DESKTOP=KDE

## Risk results (filled per task)

> **Runtime update (2026-05-29, KDE 6.6.5 Wayland / NVIDIA):** the runtime gates
> that were "compiles only" are now exercised on real hardware. Results recorded
> below; see the per-risk runtime notes and the updated Decision section.

| Risk | Task | Result | Notes |
|------|------|--------|-------|
| R6 transparency/layer/Esc | 2 | **PASS (runtime)** | `overlay` bin on KDE 6.6.5: transparent above-everything overlay renders, Esc quits, top-left toolbar button responds, and scroll passes through the crop region to windows below. Standalone (no competing toplevel) Esc works before *and* after scroll-through activation. Earlier compile evidence: `Color::TRANSPARENT` style, `Layer::Overlay`, all-anchors sizing, `KeyboardInteractivity::Exclusive`, Esc via `keyboard::key::Named::Escape`. |
| R1 wgpu coexistence | 3 | **PASS (runtime)** | `coexist` bin on KDE 6.6.5 / NVIDIA: iced_layershell overlay (wgpu) on a spawned thread + webkit2gtk toplevel (wry 0.55.1 / tao 0.35.3, `build_gtk`) on the main thread coexist in one process with no crash. NVIDIA is the worst-case GPU for this contention, so the PASS is strong evidence for the in-process D2 model. |
| R2 focus/clipboard | 3 | **CAVEAT (runtime)** | `coexist` only: after crop confirm fires `SetInputRegion` (scroll-through active), scrolling/clicking through onto the **webkit toplevel** lets KWin move keyboard focus to that regular toplevel, overriding the layer's `Exclusive` grab — Esc then goes to the webkit window, not the overlay. Clicking the toolbar (the one rect still in the input region) returns pointer focus to the layer and re-grabs keyboard, so Esc works again. **Does not occur in `overlay` (no competing toplevel).** This is focus contention between an exclusive-keyboard layer and a *focusable* toplevel in the same process. Production impact depends on whether the Tauri host window is left focusable during the overlay phase — see Decision §5 (R2). |
| R6 controls/text | 4 | compiles | Crop rectangle drag via mouse events (`ButtonPressed`/`CursorMoved`/`ButtonReleased`). Canvas `Program` trait draws rect outline with `stroke_rectangle`. Toolbar with "Finish"/"Cancel" buttons + status text pinned top-right via `Stack`. Toolbar uses sentinel magenta `Color::from_rgba(1.0, 0.0, 1.0, 1.0)` solid background (scanned by Task 7). Requires `iced` feature `canvas`. Note: `ButtonPressed` lacks cursor position in iced 0.14 — drag start is deferred to first `CursorMoved`. |
| R6 input region / scroll passthrough | 5 | compiles | `SetInputRegion(ActionCallback)` from `#[to_layer_message]` confirmed at compile time. Callback receives `&WlRegion`, calls `region.add(x, y, w, h)` to restrict input to the toolbar rectangle. After crop confirmation, the overlay emits `SetInputRegion` to set click-through on the crop area, leaving only the toolbar (top-left ~300x50px, matching the toolbar's drawn position) receptive to input. `events_transparent: false` on the surface; the compositor handles the rest. Runtime scroll passthrough requires KDE 6 Wayland. |
| R6 preview refresh | 6 | **PASS (runtime)** | Confirmed on KDE 6.6.5: the `overlay` bin's 200×200 swatch cycles red→green→blue continuously, so the external-thread `mpsc` → `Subscription::run` preview channel drives repeated overlay redraws live. Underlying mechanism: An async `iced::futures::channel::mpsc::unbounded` channel bridges an external producer thread into iced via `Subscription::run` + `rx.map(Message::NewPreview)`. The receiver is parked in a `static Mutex<Option<UnboundedReceiver>>`, consumed once by the subscription. The stream yields (parks) between frames rather than blocking the executor, so `event::listen()` keeps flowing; the producer uses non-blocking `unbounded_send`. Producer thread sends `image::Handle::from_rgba(200,200,...)` every 100ms cycling R/G/B. `Message::NewPreview(handle)` stores the handle; `view` renders it with `iced::widget::image` beneath the crop canvas. Subscription is batched with `event::listen()`. `iced` feature `image` required. |
| R3 self-capture | 7 | **FAIL — expected (runtime)** | `capture_check` on KDE 6.6.5: sentinel magenta IS present in the captured frame → the overlay is captured. This is inherent, not a bug: KDE's screencast portal captures the **composited monitor output** via PipeWire, and a layer-shell overlay surface is composited into it. Linux has no per-surface exclude-from-capture (confirmed by `crates/rollshot-app/src-tauri/src/overlay.rs:51-54`, where Linux returns `OverlayExclusion::Unsupported`, vs. Windows' `WDA_EXCLUDEFROMCAPTURE`). **Caveat:** `capture_check.rs:61-65` scans the *whole frame*, but the spike plan (Task 7 Step 5) asked for "absent *in the crop region*". Whole-frame will always find the toolbar magenta; it does NOT answer whether overlay chrome leaks *into the user's selected crop region* (the crop interior is transparent + only a stroke border is drawn). Mitigation for Phase 3 = hide overlay chrome during capture frames (place all chrome outside the crop rect and crop to the region, or unmap/transparent-ify the overlay during capture — the flameshot/spectacle pattern). See Decision §5 (R3). |
| R4 fractional scaling | 7 | compiles | Binary reports `source_size`, `effective_region`, and frame pixel dimensions from `FrameMetadata`. Coordinate mapping between overlay logical coords and frame pixel coords requires output scale factor. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R5 output match | 7 | compiles | Binary reports `pixel_format`, `stride`, `backend` from `FrameMetadata` and saves captured frame as PNG. Compilation verified; runtime capture requires KDE 6 Wayland portal. |
| R7 multi-monitor | 8 | compiles | `main` reads output name from `argv[1]`; if present, uses `StartMode::TargetScreen(name)`, otherwise `StartMode::Active`. Code targets output by name via `StartMode::TargetScreen`; runtime multi-monitor test requires KDE 6 with multiple outputs. |

## Decision

### 1. Go / no-go for `iced_layershell` as the Linux overlay stack

**GO** (was CONDITIONAL GO — runtime verification completed 2026-05-29 on KDE 6.6.5 Wayland / NVIDIA RTX 5070 Ti).

Compilation-level evidence was already strongly positive: `iced_layershell` 0.18.1 + `iced` 0.14 resolve cleanly from crates.io, all required APIs exist at the type level, and the overlay, input region, keyboard, and channel-driven preview APIs all compile without workarounds. The three spike binaries (`overlay`, `coexist`, `capture_check`) build successfully, and production crate tests (90) continue to pass.

**Runtime verification has now occurred** on real KDE 6.6.5 Wayland hardware, and the two scariest risks PASS:

- **R6 (transparency / above-everything layering / Esc / scroll passthrough):** PASS. The `overlay` bin renders a transparent above-everything layer; Esc quits; the toolbar button responds; scroll passes through the crop region. This was the core "does iced_layershell actually behave on KWin" question.
- **R1 (GPU coexistence):** PASS on **NVIDIA** — the worst-case GPU. iced's wgpu renderer (spawned thread) and webkit2gtk (main thread) coexist in one process with no crash. This validates the in-process D2 process model; the separate-process fallback is not needed on this evidence.

Two runtime issues remain and are well-understood, neither a blocker:

- **R2 (focus):** a focusable toplevel coexisting with the exclusive-keyboard layer can steal keyboard focus on click/scroll-through. Triggered in `coexist` (visible webkit toplevel), absent in `overlay`. Resolution is a production design choice — see §5 (R2).
- **R3 (self-capture):** the overlay is composited into portal/PipeWire frames; Linux has no per-surface exclude-from-capture. Expected; mitigated by hiding chrome during capture — see §5 (R3).

**Recommendation:** Proceed with `iced_layershell` as the primary Linux overlay path, in-process (D2). The remaining Phase 3 work is (a) decide the host-window focus model (R2) and (b) implement hide-chrome-during-capture (R3) — both are design/implementation tasks, not feasibility unknowns. The smithay-client-toolkit fallback (§3) is **not** triggered.

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
| R1 GPU coexistence | **PASS (runtime, NVIDIA)** | Spawned-thread model; `build_gtk` for Wayland | Nothing for feasibility. Optional: re-confirm on Mesa/AMD/Intel, but NVIDIA was the worst case. |
| R6 input region / Esc / transparency | **PASS (runtime)** | `SetInputRegion`, `WlRegion.add()`, transparent layer, Esc | Nothing for feasibility. |
| R2 focus | **CAVEAT (runtime)** — design decision, see below | `KeyboardInteractivity::Exclusive`; input-region click-through | Phase 3 must decide the host-window focus model (recommendation below). |
| R3 self-capture | **FAIL — expected (runtime)** | Sentinel scan; `capture_check` binary | Phase 3 must implement hide-chrome-during-capture; and tighten the check to scan only the crop region (recommendation below). |
| R4 fractional scaling | **Not yet run** | Coordinate mapping code; `FrameMetadata.source_size`/`effective_region` | Runtime test at 100% and 150% scaling — NOT exercised in this session. |
| R6 preview refresh | **PASS (runtime)** | Channel-driven `Subscription::run`; `mpsc` bridge | Confirmed updating live (swatch cycles R→G→B). Send→render latency not quantified, but visible refresh works. |
| R5 output match / R7 multi-monitor | Compiles, not run | `StartMode::TargetScreen`; `Stream::position()`/`size()` | Single-output session — multi-monitor not exercised. |

### R2 production determination (investigated 2026-05-29)

**The focus steal is a `coexist` test artifact under the recommended production
design, and becomes a real risk only if the design leaves the Tauri host window
focusable during the overlay phase.** Reasoning from the architecture spec
(`docs/superpowers/specs/2026-05-29-linux-wayland-layer-shell-architecture-design.md`):

- The production capture phase is **modal**: Tauri spawns the `rollshot-overlay`
  thread, the layer-shell surface drives crop → scroll → Esc, and only *after*
  the overlay exits does Tauri open the save dialog (spec Data Flow, D5). This
  mirrors today's behavior, where the Tauri overlay window *is* the single
  capture surface (`overlay.rs` covers the monitor and takes focus).
- The `coexist` spike deliberately keeps a **visible, focusable webkit toplevel**
  on screen at the same time — that is what's needed to exercise R1 (GPU
  coexistence), and it is exactly what lets KWin steal keyboard focus from the
  exclusive-keyboard layer. The production flow does not require a focusable
  toplevel to be on screen during capture.
- Crucially, R1 only needs the **GPU/webkit context to be alive**, not the
  window to be **visible/focusable**. So Phase 3 can keep webkit's context alive
  while hiding (or making non-focusable) the Tauri host window during the
  overlay phase — satisfying R1 while avoiding the R2 focus steal.

**Recommendation (R2):** during the layer-shell overlay phase, hide or
de-focus the Tauri host window (`window.hide()` / non-activating), matching the
current modal single-surface behavior. The spec's R2 mitigation note already
anticipated this ("`disable_clipboard()` + tune `keyboard_interactivity` +
input region as needed"). If a future design *requires* a visible Tauri window
concurrent with the overlay, R2 must be re-tested as a real blocker and a
keyboard re-grab strategy added.

### R3 mitigation recommendation

Linux has no exclude-from-capture (`overlay.rs:51-54`). Phase 3 must hide
overlay chrome during the actual capture frames — either (a) keep all chrome
(toolbar, crop border) outside the crop rect and crop the captured frame to the
user's region (chrome falls outside the crop), or (b) unmap / fully
transparent-ify the overlay during each capture frame and restore after (the
flameshot / spectacle pattern). Also tighten the spike's self-capture check to
scan **only the crop region** (per Task 7 Step 5), since whole-frame scanning
will always find the toolbar and cannot answer the real question (does chrome
leak *into* the user's selected region?).

All remaining "Not yet run" items above require a KDE 6 Wayland session and, for
R4, switching display scale between runs. They were not exercised in the
2026-05-29 session.
