# Linux Wayland Layer-Shell Architecture (Phase 1 Spec)

Status: design spec for Phase 1 of `docs/linux-wayland-layer-shell-roadmap.md`.
Scope: lock crate/module boundaries, process model, coordinate model, handoff
concept, support matrix, and how follow-up specs/plans split. **No production
code is produced in this phase.** Output of this phase is (1) this spec and
(2) an `iced_layershell` feasibility spike plan.

All file:line references in this document are evidence captured during design
and may drift; verify against code before relying on them.

## Goal

Replace the Linux Tauri fullscreen capture overlay with a native Wayland
layer-shell capture flow on KDE 6 Wayland, preserving current user behavior:

```text
crop select -> scroll with live stitching preview -> Esc -> Tauri save dialog
```

Capture (`rollshot-capture`) and stitching (`rollshot-core`) are **not**
rewritten. This refactor swaps the Linux UI shell, not the pipeline.

## Locked Decisions

### D1. Toolkit: `iced_layershell` primary, `smithay-client-toolkit` fallback

Honors the roadmap default. `iced_layershell` gives a retained-mode GUI
(widgets, text, layout) so crop rectangle, controls, and live preview are
ergonomic. The feasibility spike is the decision gate; if it fails on KDE 6
(transparency, input region, preview refresh, layer behavior, or the
coexistence risks in R1–R3 below), the next spec switches to the fallback
stack (`smithay-client-toolkit` + `tiny-skia`), for which `wayscrollshot` is a
proven reference (see References).

### D2. Process model: overlay runs on a spawned thread inside the Tauri process

Chosen over a separate process. Evidence that this is viable (source at
`learn-projects/exwlshelleventloop`):

- `iced_layershell` builds on `layershellev`, which uses **calloop +
  wayland-client, not winit** (`layershellev/src/lib.rs:2338-2362`,
  `:2425-2431`). There is **no main-thread assertion** anywhere — unlike winit.
- The Wayland `Connection` is `Send + Sync` and is created on whatever thread
  runs the loop; `iced_layershell` opens its **own** connection
  (`Connection::connect_to_env()`, `layershellev/src/lib.rs:2097-2101`),
  independent of the connection Tauri/GTK already holds. Two independent
  Wayland connections in one process are allowed.
- `iced_layershell`'s entry point `.run()` **blocks the calling thread**
  (`iced_layershell/src/build_pattern/application.rs:578-620`), so it runs on a
  dedicated `std::thread` while the Tauri GTK/GLib loop keeps the main thread.
- External threads (the capture/stitch pipeline) push messages into the running
  overlay via a calloop `Channel` / `IcedProxy`
  (`iced_layershell/src/multi_window.rs:70-71`, `proxy.rs:32-50`) or an iced
  `Subscription`.

This avoids the "two GUI runtimes fighting over the main thread" problem that
would have blocked an in-process design built on winit-based toolkits.

**Fallback:** if the spike finds an unresolvable coexistence problem on KDE 6
(see R1–R3), fall back to running the overlay as a **separate process** that
Tauri launches and that returns the finalized image over IPC. The handoff
contract (D5) is defined so this fallback does not change later specs' shape.

### D3. New crate `rollshot-overlay` (Linux-only, Tauri-free)

```text
rollshot-overlay  (new, Linux-only, MUST NOT depend on Tauri)
  +- iced_layershell    -> layer-shell overlay UI (crop picker, preview, controls, Esc)
  +- rollshot-capture   -> portal/PipeWire frames (unchanged)
  +- rollshot-core      -> Stitcher (unchanged)

crates/rollshot-app/src-tauri
  +- [Linux] depends on rollshot-overlay; starts it on a spawned thread;
  |          receives the finalized image; opens the existing save dialog
  +- [macOS] existing flow unchanged
```

Hard constraint (from roadmap): `rollshot-core` and `rollshot-capture` MUST NOT
depend on Tauri or `rollshot-overlay`.

`rollshot-overlay` owns its own capture+stitch driver loop, mirroring the
existing headless pattern in `rollshot-cli/src/cmd_capture.rs:100-202`
(reader thread + latest-wins frame slot + stitch loop calling
`Stitcher::push_frame`). This driver logic is **already duplicated** between
`cmd_capture.rs` and `crates/rollshot-app/src-tauri/src/session.rs` (reader +
`stitch_loop` + `push_stitch_frame`, `session.rs:374-561`). Phase 1 does **not**
force extracting a shared driver — that is a refactor outside the current
request. The duplication is recorded here; the overlay implementation plan
(follow-up #3) decides whether to extract a shared driver or reimplement.

### D4. Crop picker model: transparent see-through, monitor/full-source only

The overlay is a transparent layer-shell surface anchored to a single output;
the user drags a crop rectangle over the live screen (the `slurp` /
`wayscrollshot` model). The first production path supports **monitor /
full-source capture only**. **Arbitrary window capture is out of scope** for
this roadmap (see Non-Goals and the coordinate model below).

**Monitor-only is enforced at the portal layer**, so the window-coordinate
problem never arises on the in-scope path:

- **Primary (request-time restriction):** request only
  `SourceType::Monitor` in `select_sources`. Today the code requests
  `Monitor | Window` (`rollshot-capture/src/linux/portal.rs:258-259`); restrict
  it to `Monitor` so the portal/KDE picker offers only monitors and a window
  cannot be selected.
- **Defensive (post-start detection):** the `types` argument is a hint a
  backend *may* ignore (KDE honors it, but defense-in-depth). When building
  stream info (`portal.rs:280-286`, which today reads only
  `pipe_wire_node_id()`), also read `Stream::source_type()`; if it is `Window`,
  return a `CaptureError` the overlay surfaces to the user.

This is a **small, allowed change to `rollshot-capture`** (the `SourceType`
bitmask plus reading `source_type`). It is explicitly *not* a capture rewrite,
but it does touch the capture crate, so it is called out here rather than hidden
under "capture unchanged". `Stream::position()` / `Stream::size()` from ashpd
are also available and are the likely inputs for the output-matching constraint
(R5).

### D5. Handoff contract: generic, not "save PNG only"

When the user presses Esc, the overlay finalizes stitching
(`Stitcher::full_image()`, `rollshot-core/src/stitcher.rs:357-359`) and hands a
result back to the Tauri main thread; the overlay event loop then exits. The
result type is named generically:

```rust
struct CaptureResult { image: RgbaImage, stats: StitchStats, /* ... */ }
// handoff: on_capture_finished(CaptureResult)
```

Tauri stores `image` as the session's final image and opens the **existing**
save dialog + `save_image` flow (`src-tauri/src/session.rs:233-247`), unchanged.
The name MUST NOT bake in "save PNG only" — future editor / image / video / GIF
/ multi-output flows must be able to reuse the same handoff concept (those
features are out of scope here).

In the D2 fallback (separate process), `CaptureResult` is serialized over IPC
(e.g. the overlay writes a temp file path + summary on exit) instead of an
in-process channel; the concept and Tauri-side save flow are identical.

## Coordinate Model (the core risk for D4)

Three coordinate spaces are involved:

1. **Overlay / output logical coordinates** — pointer coordinates from
   layer-shell, in surface-local logical pixels. KDE commonly uses **fractional
   scaling** (e.g. 125%, 150%).
2. **Output device pixels** — logical x scale.
3. **Captured-frame pixel space** — the PipeWire stream's actual buffer,
   described by `FrameMetadata { source_size, stride, effective_region, ... }`
   (`rollshot-capture/src/types.rs:47-53`).

**Monitor/full-source transform (in scope):** the overlay is anchored to a
specific output and covers it; the captured frame is that same monitor. The
crop rectangle in output-logical coordinates maps to frame pixel coordinates by
multiplying by the output scale, then clamping to `source_size`. `source_size`
from frame metadata validates the mapping. The crop is applied to the frame in
pixel space before `Stitcher::push_frame` (the existing app already crops in
frame space, `session.rs:199-212`).

**Why window capture is out of scope:** the screen-space crop rectangle cannot
be reliably mapped into a captured *window* buffer because (a) the portal
deliberately does not expose the window's on-screen geometry, (b) the window can
move or resize mid-capture while the overlay is anchored to the output, and
(c) client-side decorations / shadows / HiDPI buffers are not 1:1 with what
appears under the picker. A future spec that wants window capture would need to
prove a reliable mapping — most likely by switching to a "crop on captured
content" picker (display the captured buffer and let the user crop that), which
is a different UX and out of scope now.

**Output-matching constraint:** the overlay MUST be anchored to the same output
that the portal/PipeWire source actually captures. The spike must determine how
to obtain/confirm this mapping on KDE 6 (portal source selection vs. overlay
output anchoring).

## Data Flow

```text
[Linux start capture]
 src-tauri detects platform -> spawns rollshot-overlay thread
   crop select (transparent layer-shell, anchored to output)   <- user drags box
   confirm region -> start capture+stitch driver
     CaptureBackend::start -> FrameStream::next_frame    (rollshot-capture; public
                                                          API unchanged, internal
                                                          SelectSources restricted to
                                                          Monitor per D4)
     crop frame in pixel space (output scale -> frame px) (coordinate model)
     Stitcher::push_frame                                 (rollshot-core, unchanged)
     full_image() -> downscale -> channel -> overlay redraw (live preview)
   Esc -> finalize stitch -> full_image() -> CaptureResult -> back to Tauri thread
 Tauri main thread -> existing save dialog -> save_image (unchanged)
```

Stable contracts that do **not** change:

- capture: `CaptureBackend::start(CaptureOptions) -> Box<dyn FrameStream>`,
  `FrameStream::next_frame() -> Result<CapturedFrame>`
  (`rollshot-capture/src/backend.rs:4-12`).
- core: `Stitcher::push_frame(RgbaImage) -> StitchOutcome`,
  `Stitcher::full_image() -> Option<&RgbaImage>`
  (`rollshot-core/src/stitcher.rs:42-54`, `:357-359`).
- save: Tauri `save_image(path)` -> `AppSession::save_image`
  (`src-tauri/src/session.rs:233-247`).

## Support Matrix and Non-Goals

Primary target: **KDE 6 Wayland**.
Opportunistic: wlroots-based compositors, niri, Hyprland, sway.
Not promised: GNOME Wayland, X11, hard compatibility for non-KDE compositors.

**Rule:** any KDE-specific behavior or workaround MUST be called out explicitly
in the relevant spec/plan. KDE-specific code MUST NOT be hidden inside a
supposedly generic layer-shell backend.

Out of scope (from roadmap): Tauri image editor, clipboard integration, video
export, GIF export, multi-output export UI, settings UI redesign, GNOME Wayland,
X11, hard non-KDE guarantees, and arbitrary window-capture crop mapping unless a
later spec proves it reliable. First production path is monitor/full-source
capture only.

## Risks -> Spike Acceptance Checks

| # | Risk | Spike must verify on KDE 6 |
|---|------|-----------------------------|
| R1 | GPU/wgpu context contention (Tauri webkit GPU vs. iced wgpu in one process) | overlay thread initializes wgpu while a Tauri window stays alive and functional, no crash |
| R2 | Clipboard / focus conflict between GTK and overlay | overlay keyboard/focus works; use `disable_clipboard()` and tune `keyboard_interactivity` + input region as needed |
| R3 | Self-capture (overlay appears in portal/PipeWire frames; Linux has no native exclude-from-capture, `src-tauri/src/overlay.rs:50-53`) | preview/controls placed **outside** the crop region; crop region rendered fully transparent + click-through during scrolling; verify captured frames contain no overlay pixels |
| R4 | KDE fractional scaling -> off-by-scale crop | crop rectangle maps correctly to frame pixels on a fractionally-scaled (e.g. 150%) KDE monitor |
| R5 | Overlay anchored to wrong output vs. captured source | overlay output matches the portal/PipeWire captured monitor; likely via ashpd `Stream::position()` / `size()` to identify the selected output |
| R6 | KDE 6 layer behavior (transparent overlay above fullscreen apps, input region, repeated preview refresh) | roadmap Phase 2 acceptance checks |
| R7 | Multi-monitor | predictable behavior on >=1 multi-monitor setup, or documented blocker |

Decision gates:

- If the spike passes (incl. R1–R3), use `iced_layershell` in-process (D2).
- If R1–R3 are unresolvable in-process but `iced_layershell` itself works, fall
  back to the separate-process variant of D2.
- If `iced_layershell` fails core layer/transparency/input/refresh checks
  (R6) outright, switch the next spec to the `smithay-client-toolkit` fallback
  (D1).

## Follow-Up Specs and Plans (roadmap order, not merged)

1. **This architecture spec** (done).
2. **`iced_layershell` feasibility spike** (spec + plan) — immediate next
   deliverable; it is the decision gate before product integration.
3. **Native Linux capture overlay** (spec + plan) — replaces the Tauri
   fullscreen crop picker + live preview; decides the shared-driver question
   from D3.
4. **Tauri save handoff** (spec + plan) — preserves current end-user save
   behavior using D5.

Do not merge these into one implementation plan. The first unknown is toolkit
+ coexistence feasibility, so the spike remains a gate.

## References

- Roadmap: `docs/linux-wayland-layer-shell-roadmap.md`.
- `iced_layershell` source (process-model evidence):
  `learn-projects/exwlshelleventloop` — `layershellev/src/lib.rs`
  (`:2338-2362` run loop, `:2425-2431` calloop EventLoop, `:2097-2101` own
  connection, `:1354-1373` layer-shell builder props, `:2219-2223` /`:1296-1299`
  input region / `events_transparent`),
  `iced_layershell/src/build_pattern/application.rs:578-620` (`.run()` blocks),
  `iced_layershell/src/multi_window.rs:70-71` + `proxy.rs:32-50` (external
  message channel). Pull iced upstream source if subscription/redraw details are
  needed during the spike.
- `wayscrollshot` (fallback-stack reference, `learn-projects/wayscrollshot`):
  layer config `src/overlay.rs:696-698`, input-region masking
  `src/overlay.rs:174-189`, multi-output placement `src/overlay/placement.rs`,
  `tiny-skia` rendering. Note: its `grim`/wlr-screencopy **capture** path does
  not work on KDE — only its layer-shell **UI** pattern is the reference; we
  keep portal/PipeWire capture.
