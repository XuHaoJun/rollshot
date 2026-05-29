# iced_layershell Feasibility Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove (or disprove) on KDE 6 Wayland that `iced_layershell` can be the native Linux layer-shell capture overlay stack, running on a spawned thread inside a Tauri-class process, before any product integration.

**Architecture:** Throwaway, isolated spike crate at `spikes/layershell-feasibility/` (NOT a workspace member, deleted after the decision). Each task is a milestone that builds up the overlay's required capabilities, ordered so the highest-risk assumptions are tested first. Verification is primarily **observation on KDE 6 Wayland hardware** plus a few semi-automated checks; the deliverable is a written decision doc, not shippable code.

**Tech Stack:** Rust, `iced_layershell = "0.18"` + `iced = "0.14"` (wlr-layer-shell via `layershellev`/calloop), `wry` (Tauri's webview, for the coexistence test), `rollshot-capture` + `rollshot-core` (path deps, for the capture-integration milestone).

---

## Spike Ground Rules (read before starting)

- **This is a spike.** The code is throwaway. Do not optimize, abstract, or add error handling beyond what a milestone's check needs. Do not touch production crates except the one explicit, reverted change in Task 7.
- **Decision gates.** Tasks 2, 3, and 7 are hard gates. If a gate fails and cannot be resolved within its task, STOP and record the failure + recommended fallback in the findings doc — do not continue building on a broken foundation.
- **Hardware.** Every "Run on KDE 6" step must be executed on a KDE 6 Wayland session. Record the KDE Plasma / KWin / Mesa / GPU versions in the findings doc (Task 1).
- **Findings doc.** `spikes/layershell-feasibility/FINDINGS.md` is created in Task 1 and appended to after every milestone. It is the real output of this spike.
- **Maps to spec risks.** Tasks reference R1–R7 from `docs/superpowers/specs/2026-05-29-linux-wayland-layer-shell-architecture-design.md`.

---

## File Structure

- `spikes/layershell-feasibility/Cargo.toml` — standalone manifest (NOT in root workspace; uses `[workspace]` empty table to detach).
- `spikes/layershell-feasibility/FINDINGS.md` — running decision log, the spike deliverable.
- `spikes/layershell-feasibility/src/bin/overlay.rs` — the iced_layershell overlay (grows across Tasks 2,4,5,6).
- `spikes/layershell-feasibility/src/bin/coexist.rs` — wry-on-main-thread + overlay-on-spawned-thread (Task 3).
- `spikes/layershell-feasibility/src/bin/capture_check.rs` — portal monitor capture + self-capture + coordinate checks (Task 7).
- `spikes/layershell-feasibility/src/overlay_app.rs` — shared overlay `iced_layershell` app, reused by `overlay.rs` and `coexist.rs`.

---

## Task 1: Scaffold the isolated spike crate and baseline upstream example

**Files:**
- Create: `spikes/layershell-feasibility/Cargo.toml`
- Create: `spikes/layershell-feasibility/FINDINGS.md`
- Create: `spikes/layershell-feasibility/src/bin/overlay.rs` (temporary stub)

- [ ] **Step 1: Create the standalone manifest**

`spikes/layershell-feasibility/Cargo.toml`:

```toml
[package]
name = "layershell-feasibility"
version = "0.0.0"
edition = "2021"
publish = false

# Detach from the rollshot root workspace so this throwaway crate does not
# affect production build / clippy / CI.
[workspace]

[dependencies]
iced = "0.14"
iced_layershell = "0.18"

[[bin]]
name = "overlay"
path = "src/bin/overlay.rs"
```

Note: `iced_layershell` 0.18.1 and `iced` 0.14 are the versions in the local reference clone (`learn-projects/exwlshelleventloop`). If `cargo build` cannot resolve them from crates.io, fall back to path deps:
`iced_layershell = { path = "../../learn-projects/exwlshelleventloop/iced_layershell" }` (and matching `iced` path if needed). Record which form you used in FINDINGS.md.

- [ ] **Step 2: Create the findings doc with an environment header**

`spikes/layershell-feasibility/FINDINGS.md`:

```markdown
# iced_layershell Feasibility Spike — Findings

## Environment
- KDE Plasma version: <fill from `plasmashell --version`>
- KWin version: <fill from `kwin_wayland --version`>
- Mesa / GPU: <fill from `glxinfo | grep -i "opengl renderer"` or `vulkaninfo | grep deviceName`>
- Session: <confirm `echo $XDG_SESSION_TYPE` == wayland and `echo $XDG_CURRENT_DESKTOP`>
- iced_layershell dep form: <crates.io 0.18 | path to clone>

## Risk results (filled per task)
| Risk | Task | Result | Notes |
|------|------|--------|-------|
| R6 transparency/layer/Esc | 2 | | |
| R1 wgpu coexistence | 3 | | |
| R2 focus/clipboard | 3 | | |
| R6 controls/text | 4 | | |
| R6 input region / scroll passthrough | 5 | | |
| R6 preview refresh | 6 | | |
| R3 self-capture | 7 | | |
| R4 fractional scaling | 7 | | |
| R5 output match | 7 | | |
| R7 multi-monitor | 8 | | |

## Decision
<filled in Task 9>
```

- [ ] **Step 3: Copy the upstream `counter` example verbatim as a baseline smoke test**

Copy `learn-projects/exwlshelleventloop/iced_examples/counter/src/main.rs` into `src/bin/overlay.rs` unchanged for now (it is a known-working layer-shell app). This proves the toolchain + dep resolution before we write our own code.

- [ ] **Step 4: Build and run on KDE 6**

Run: `cd spikes/layershell-feasibility && cargo run --bin overlay`
Expected: a bottom-anchored panel with Increment/Decrement buttons appears on the KDE 6 Wayland session without crashing. If it does not appear or panics, this is an immediate R6 baseline failure — record in FINDINGS.md and STOP (iced_layershell does not run on this KDE 6; jump to the smithay fallback decision in Task 9).

- [ ] **Step 5: Fill the environment header in FINDINGS.md** with the real versions and the dep form used.

- [ ] **Step 6: Commit**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): scaffold isolated crate + upstream baseline"
```

---

## Task 2: Transparent fullscreen overlay covering one output, Esc to quit (R6)

**Files:**
- Create: `spikes/layershell-feasibility/src/overlay_app.rs`
- Modify: `spikes/layershell-feasibility/src/bin/overlay.rs` (replace the copied counter)
- Modify: `spikes/layershell-feasibility/Cargo.toml` (add the lib module wiring)

- [ ] **Step 1: Write the shared overlay app**

`spikes/layershell-feasibility/src/overlay_app.rs` — a transparent, output-covering overlay that exits on Esc. Confirmed API (mirrors `counter` + `input_regions` examples):

```rust
use iced::widget::{container, text};
use iced::{Color, Element, Event, Length, Task, event, keyboard};
use iced_layershell::Settings;
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

#[derive(Default)]
pub struct Overlay;

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    IcedEvent(Event),
}

fn namespace() -> String {
    "rollshot-spike-overlay".to_string()
}

fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    event::listen().map(Message::IcedEvent)
}

fn update(_state: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })) => {
            // Decision: Esc finishes/cancels. For the spike, exit the loop.
            std::process::exit(0);
        }
        _ => Task::none(),
    }
}

fn view(_state: &Overlay) -> Element<'_, Message> {
    // A faint label so we can see the surface exists; background stays transparent.
    container(text("rollshot overlay (Esc to quit)").size(24))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn style(_state: &Overlay, theme: &iced::Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    }
}

/// `start_mode` lets callers target a specific output by name (Task 8).
pub fn run(start_mode: StartMode) -> Result<(), iced_layershell::Error> {
    application(Overlay::default, namespace, update, view)
        .style(style)
        .subscription(subscription)
        .settings(Settings {
            layer_settings: LayerShellSettings {
                // Cover the whole output: anchor to all four edges, no exclusive zone.
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                layer: Layer::Overlay,
                exclusive_zone: 0,
                size: None,
                margin: (0, 0, 0, 0),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                start_mode,
                events_transparent: false,
            },
            ..Default::default()
        })
        .run()
}
```

If the exact `keyboard::Key`/`Named::Escape` path differs in iced 0.14, derive it from the `counter` example's `Message::IcedEvent(event)` handler (it prints raw events — run once, press Esc, read the printed variant). Record the confirmed path in a code comment.

- [ ] **Step 2: Wire the binary**

`spikes/layershell-feasibility/src/bin/overlay.rs`:

```rust
#[path = "../overlay_app.rs"]
mod overlay_app;

fn main() -> Result<(), iced_layershell::Error> {
    overlay_app::run(iced_layershell::settings::StartMode::Active)
}
```

- [ ] **Step 3: Build and run on KDE 6**

Run: `cd spikes/layershell-feasibility && cargo run --bin overlay`
Then open a fullscreen application (e.g. a maximized/fullscreen video or browser) behind it.

- [ ] **Step 4: Observe and record (R6)** — check each:
  - Overlay surface covers the full output.
  - Background is genuinely transparent (you can see the app behind it, not a black/grey fill).
  - Overlay renders **above** the fullscreen app.
  - Pressing **Esc** quits the process.

  Record PASS/FAIL + screenshots in FINDINGS.md under R6. If transparency or above-fullscreen layering fails, note exactly how (e.g. opaque background, or hidden behind fullscreen) — these are the smithay-fallback triggers.

- [ ] **Step 5: Commit**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): transparent output-covering overlay + Esc (R6)"
```

---

## Task 3: GATE — overlay on a spawned thread alongside a wry/webkit main loop (R1, R2)

This is the make-or-break test for the spec's in-process decision (D2). `wry` is the exact webview Tauri v2 uses on Linux (gtk3 + webkit2gtk 4.1), so it reproduces the real GPU/event-loop footprint without the full Tauri stack.

**Files:**
- Modify: `spikes/layershell-feasibility/Cargo.toml` (add `wry`, `gtk`, `tao` deps + `coexist` bin)
- Create: `spikes/layershell-feasibility/src/bin/coexist.rs`

- [ ] **Step 1: Add deps + bin target**

Append to `Cargo.toml`:

```toml
[[bin]]
name = "coexist"
path = "src/bin/coexist.rs"

[dependencies.wry]
version = "0.45"
[dependencies.tao]
version = "0.30"
```

Use `wry` + `tao` (tao is the windowing layer wry pairs with, and what Tauri uses). If these versions fail to resolve, match the versions Tauri v2 pulls in — check the workspace lockfile: `rg -n 'name = "wry"' -A1 Cargo.lock` and `rg -n 'name = "tao"' -A1 Cargo.lock` at the repo root, and pin those. Record the versions used in FINDINGS.md.

- [ ] **Step 2: Write the coexistence binary** — wry WebView on the main thread (owns gtk/webkit), iced_layershell overlay on a spawned thread:

`spikes/layershell-feasibility/src/bin/coexist.rs`:

```rust
#[path = "../overlay_app.rs"]
mod overlay_app;

use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

fn main() -> wry::Result<()> {
    // 1. Spawn the layer-shell overlay on its OWN thread (it opens its own
    //    Wayland connection and runs its own calloop loop; .run() blocks).
    std::thread::Builder::new()
        .name("overlay".into())
        .spawn(|| {
            let _ = overlay_app::run(iced_layershell::settings::StartMode::Active);
        })
        .expect("spawn overlay thread");

    // 2. Main thread brings up gtk + webkit2gtk (Tauri's footprint) via wry/tao.
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("coexist host (Tauri-class webkit)")
        .build(&event_loop)
        .unwrap();
    let _webview = WebViewBuilder::new()
        .with_html("<h1>webkit host alive</h1>")
        .build(&window)?;

    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
    });
}
```

If the wry 0.45 builder API differs (e.g. `WebViewBuilder::new(&window)`), adapt to the version you pinned — consult `wry`'s docs.rs for that version. The intent (webkit webview on main thread + overlay thread) is what matters.

- [ ] **Step 3: Build and run on KDE 6**

Run: `cd spikes/layershell-feasibility && cargo run --bin coexist`

- [ ] **Step 4: Observe and record (R1, R2) — GATE.** Check each:
  - **R1:** Both surfaces are alive simultaneously: the wry webkit window shows "webkit host alive" AND the layer-shell overlay renders, with **no GPU crash, no wgpu/EGL panic, no hang**. Watch the terminal for wgpu/Vulkan/EGL errors.
  - **R2:** The overlay receives keyboard focus enough to act on Esc; the webkit window remains usable. Note any clipboard or focus stealing.
  - Leave it running ~60s and interact with both; record stability.

  **Decision gate:**
  - PASS → in-process design (D2) is viable; continue.
  - R1 fails (GPU contention/crash) but the overlay works standalone (Task 2 passed) → record that D2 must use the **separate-process fallback**; the remaining tasks still validate the overlay itself, but note in FINDINGS that the overlay must run as its own process.
  - If only R2 issues → try `disable_clipboard()` (check iced_layershell builder/settings for the method) and `KeyboardInteractivity::OnDemand`; record the mitigation.

- [ ] **Step 5: Commit**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): GATE overlay+webkit coexistence on a thread (R1,R2)"
```

---

## Task 4: Crop rectangle + toolbar controls + text (R6 controls)

**Files:**
- Modify: `spikes/layershell-feasibility/src/overlay_app.rs`

- [ ] **Step 1: Add crop-rectangle state and pointer handling**

Extend `Overlay` to track a drag rectangle from pointer events. Use iced's `mouse` events from the existing `event::listen()` subscription:

```rust
#[derive(Default)]
pub struct Overlay {
    drag_start: Option<iced::Point>,
    crop: Option<iced::Rectangle>,
}
```

In `update`, handle `Event::Mouse(mouse::Event::ButtonPressed/CursorMoved/ButtonReleased)` to set `drag_start`, update `crop` while dragging, and finalize on release. (Derive exact event variants from the `counter` example's printed events.)

- [ ] **Step 2: Draw the crop rectangle + a toolbar**

Use an `iced::widget::canvas` (or a `Stack` with a positioned `container`) to draw the crop rectangle outline, and a `row![button("Finish"), button("Cancel")]` toolbar pinned to a corner with a solid background so it is visibly distinct (this distinct color is reused in Task 7's self-capture check). Add a `text(...)` status label to satisfy "render text".

- [ ] **Step 3: Build and run on KDE 6**

Run: `cargo run --bin overlay`

- [ ] **Step 4: Observe and record (R6)** — drag to draw a rectangle; confirm the outline tracks the drag, the toolbar buttons render with text, and "Finish"/"Cancel" are clickable (clicking Cancel exits). Record in FINDINGS.md.

- [ ] **Step 5: Commit**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): crop rectangle + toolbar controls (R6)"
```

---

## Task 5: Input region — toolbar catches clicks, crop area passes scroll through (R6)

**Files:**
- Modify: `spikes/layershell-feasibility/src/overlay_app.rs`

- [ ] **Step 1: Add a SetInputRegion action**

Follow the `input_regions` example exactly. After the crop is confirmed, set the input region to only the toolbar rectangle, leaving the crop/preview area transparent to input so scroll reaches the app behind:

```rust
use iced_layershell::actions::ActionCallback;

// In update(), once capture/scroll mode is active:
let (tx, ty, tw, th) = toolbar_bounds_px(); // the toolbar rectangle in surface px
return Task::done(Message::SetInputRegion(ActionCallback::new(move |region| {
    region.add(tx, ty, tw, th);
})));
```

`Message::SetInputRegion` is generated by `#[to_layer_message]` (confirmed in the `input_regions` example). The full-screen default is `region.add(0, 0, w, h)`.

- [ ] **Step 2: Build and run on KDE 6, behind a scrollable app**

Run: `cargo run --bin overlay`, with a long scrollable page/app behind the crop area.

- [ ] **Step 3: Observe and record (R6)** — after confirming the crop:
  - Scrolling the mouse wheel over the **crop area** scrolls the app behind (input passes through).
  - Clicking the **toolbar** still hits the overlay buttons (input caught).

  This is the core "receive crop/toolbar input without permanently blocking scroll" requirement. Record PASS/FAIL; if scroll never reaches the app even with the input region cleared over the crop area, document it (it is a KDE-specific blocker to call out).

- [ ] **Step 4: Commit**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): input-region click-through for scroll (R6)"
```

---

## Task 6: Live preview refresh driven by an external thread (R6 refresh)

**Files:**
- Modify: `spikes/layershell-feasibility/src/overlay_app.rs`
- Modify: `spikes/layershell-feasibility/src/bin/overlay.rs`

- [ ] **Step 1: Add a preview-image message + subscription bridge**

Goal: an external `std::thread` pushes a new image repeatedly and the overlay redraws. Use an iced subscription that owns the receiving end of a channel; hand the sender to a producer thread. Start from the upstream `redraw` and `counter_timer` examples for the working subscription/redraw shape.

Add:

```rust
use std::sync::mpsc;

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    IcedEvent(Event),
    NewPreview(iced::widget::image::Handle),
}
```

Bridge the std `mpsc::Receiver<image::Handle>` into iced via `iced::Subscription::run_with_id` + `iced::stream::channel` (confirm the exact constructor name against iced 0.14 — the `redraw` example uses the current subscription API). The subscription forwards received handles as `Message::NewPreview`, which `view` renders via `iced::widget::image(handle)`.

- [ ] **Step 2: Drive it from a producer thread in the binary**

In `overlay.rs`, before `run`, spawn a thread that every ~100 ms sends a freshly generated `image::Handle` (e.g. a solid color that cycles, built from raw RGBA bytes via `image::Handle::from_rgba`) down the channel. This stands in for the stitch preview.

- [ ] **Step 3: Build and run on KDE 6**

Run: `cargo run --bin overlay`

- [ ] **Step 4: Observe and record (R6)** — the preview area visibly updates ~10×/s with no flicker, tearing, or growing latency over 60s. Record frame-update smoothness in FINDINGS.md. This validates the channel-driven redraw path the real pipeline will use.

- [ ] **Step 5: Commit**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): external-thread preview refresh (R6)"
```

---

## Task 7: GATE — real portal monitor capture: self-capture, scaling, output match (R3, R4, R5)

This validates the risks that can only be exercised with real capture. It uses the existing `rollshot-capture` crate via a path dep.

**Files:**
- Modify: `spikes/layershell-feasibility/Cargo.toml` (path deps on rollshot-capture/core + `capture_check` bin)
- Create: `spikes/layershell-feasibility/src/bin/capture_check.rs`
- Modify (temporary, reverted in Step 6): `crates/rollshot-capture/src/linux/portal.rs:258-259`

- [ ] **Step 1: Add path deps + bin**

```toml
[[bin]]
name = "capture_check"
path = "src/bin/capture_check.rs"

[dependencies.rollshot-capture]
path = "../../crates/rollshot-capture"
[dependencies.rollshot-core]
path = "../../crates/rollshot-core"
[dependencies.image]
version = "0.25"
```

- [ ] **Step 2: Temporarily restrict the portal to Monitor-only (validates D4 enforcement)**

In `crates/rollshot-capture/src/linux/portal.rs:258-259`, change:

```rust
ashpd::desktop::screencast::SourceType::Monitor
    | ashpd::desktop::screencast::SourceType::Window,
```

to:

```rust
ashpd::desktop::screencast::SourceType::Monitor,
```

Run `cargo run --bin capture_check` and confirm the KDE portal picker now offers **only monitors** (no window option). Record in FINDINGS.md whether KDE honored the restriction. (This change is reverted in Step 6; the production decision lives in the overlay implementation plan.)

- [ ] **Step 3: Write the capture check binary**

`capture_check.rs`: start the overlay (Task 2/4 app) on a thread, then on the main thread drive a portal monitor capture via `rollshot-capture` (`default_backend()` → `CaptureBackend::start(CaptureOptions { region: RegionMode::FullSource, .. })` → `FrameStream::next_frame()`), grab one `CapturedFrame`, and run two checks:

  - **R3 self-capture:** the overlay's toolbar uses a unique sentinel color (e.g. RGBA `255,0,255,255`). Scan the captured frame for that exact color. Assert it is **absent** in the crop region (and ideally everywhere the overlay drew). Print the count of sentinel pixels found.
  - **R4 fractional scaling:** place a known on-screen marker (e.g. open a window showing a solid red square at a known screen position), select a crop rectangle around it in overlay/output-logical coordinates, map the crop to frame pixels by multiplying by the output scale (read scale from the frame metadata: `FrameMetadata.source_size` vs the output's logical size), crop the frame, and assert the cropped region contains the red marker. Run this at KDE display scale **100% and 150%**.

  Use `FrameMetadata` fields (`source_size`, `stride`, `effective_region`) from `rollshot-capture/src/types.rs`. Save the captured frame and the cropped frame as PNGs next to FINDINGS.md for visual inspection.

- [ ] **Step 4: Build and run on KDE 6 at 100% and 150% scale**

Run: `cargo run --bin capture_check` (once per scale setting; change scale in KDE System Settings → Display between runs).

- [ ] **Step 5: Observe and record (R3, R4, R5) — GATE.**
  - **R3:** sentinel-color pixel count in the captured frame is **0** (overlay not captured). If non-zero, the overlay IS being captured — record the count and which strategy is needed (move preview/controls fully outside the crop region; the crop region must stay fully transparent + input-region-clear during capture). This is a hard input into the Phase 3 overlay design.
  - **R4:** the cropped frame contains the red marker at **both** 100% and 150% scale. If 150% is offset/wrong, record the exact pixel offset — this is the fractional-scaling coordinate bug the spec flagged.
  - **R5:** if multi-output, confirm the overlay output matches the captured monitor (use `Stream::position()`/`size()` if needed). On single-output, note "N/A, see Task 8".

  Record results + attach the saved PNGs.

- [ ] **Step 6: Revert the production-crate change**

```bash
git checkout crates/rollshot-capture/src/linux/portal.rs
```

Confirm `git status` shows no changes under `crates/`. The Monitor-only enforcement is a *decision* recorded in the spec (D4); it is implemented for real in the overlay plan, not here.

- [ ] **Step 7: Commit (spike crate only)**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): GATE portal capture self-capture+scaling+output (R3,R4,R5)"
```

---

## Task 8: Multi-monitor behavior (R7)

**Files:**
- Modify: `spikes/layershell-feasibility/src/bin/overlay.rs`

- [ ] **Step 1: Target a specific output by name**

Use `StartMode::TargetScreen(output_name)` (confirmed in `settings.rs` tests, e.g. `"HDMI-1"`). Read the output name from `argv[1]`, defaulting to `StartMode::Active`:

```rust
fn main() -> Result<(), iced_layershell::Error> {
    let start_mode = match std::env::args().nth(1) {
        Some(name) => iced_layershell::settings::StartMode::TargetScreen(name),
        None => iced_layershell::settings::StartMode::Active,
    };
    overlay_app::run(start_mode)
}
```

- [ ] **Step 2: Run on a multi-monitor KDE 6 setup**

Run: `cargo run --bin overlay -- <output-name>` (find names via `kscreen-doctor -o` or the compositor). If only a single monitor is available, record R7 as "untested — no multi-monitor hardware" — that is an acceptable documented outcome per the roadmap.

- [ ] **Step 3: Observe and record (R7)** — the overlay appears on the intended output, transparency and the crop picker behave the same as single-monitor, and it does not spill onto the wrong output. Record any blocker.

- [ ] **Step 4: Commit**

```bash
git add spikes/layershell-feasibility
git commit -m "spike(layershell): multi-monitor output targeting (R7)"
```

---

## Task 9: Write the decision doc (the spike deliverable)

**Files:**
- Modify: `spikes/layershell-feasibility/FINDINGS.md`

- [ ] **Step 1: Fill the risk results table** with PASS / FAIL / MITIGATED / UNTESTED for every risk R1–R7, each with a one-line note and a link to the screenshot/PNG evidence.

- [ ] **Step 2: Write the Decision section** answering the spec's decision gates explicitly:
  - **Go / no-go for `iced_layershell`** as the Linux overlay stack.
  - **Process model:** in-process spawned thread (D2 primary) vs. separate-process fallback — based on the Task 3 R1/R2 result.
  - If `iced_layershell` failed core checks (R6 transparency/layer/input/refresh), **recommend switching the next spec to the smithay-client-toolkit fallback** (D1), citing which check failed.
  - List any KDE-specific behaviors/workarounds discovered (these MUST be called out per the spec's support-matrix rule).
  - List unresolved risks that the Phase 3 overlay spec must carry forward (especially any R3 self-capture mitigation or R4 scaling handling).

- [ ] **Step 3: Commit**

```bash
git add spikes/layershell-feasibility/FINDINGS.md
git commit -m "spike(layershell): decision doc — go/no-go + process model + risks"
```

- [ ] **Step 4: Hand off**

The FINDINGS decision feeds the next deliverable: the **Native Linux capture overlay** spec + plan (roadmap follow-up #3). Do not delete the spike crate until that spec has extracted everything useful from it; deletion happens when the overlay implementation lands.

---

## Self-Review (completed by plan author)

**Spec coverage:** Every Phase 2 acceptance check in the roadmap and every risk R1–R7 in the architecture spec maps to a task: transparent above-fullscreen overlay + Esc (T2/R6), draw crop rectangle + text + controls (T4/R6), live preview updates (T6/R6), input behavior without blocking scroll (T5/R6), multi-monitor (T8/R7), plus the spec-added coexistence (T3/R1,R2), self-capture (T7/R3), fractional scaling (T7/R4), and output match (T7/R5). The D4 portal Monitor-only enforcement is validated in T7 (and reverted, since it is a production decision recorded in the spec).

**Placeholder scan:** Code shown for confirmed APIs (LayerShellSettings, application builder, SetInputRegion, transparent style, StartMode). The three genuinely-unverified spots (Esc key variant, wry 0.45 builder signature, iced 0.14 subscription-channel constructor) each name the exact upstream example or lockfile entry to derive the real form from — appropriate for a spike, not a hand-wave.

**Consistency:** `overlay_app::run(StartMode)` signature is used identically by `overlay.rs`, `coexist.rs`, and Task 8. `Message` is extended additively (IcedEvent → +NewPreview) with `#[to_layer_message]` throughout. The sentinel toolbar color introduced in T4 is the same one scanned in T7.
