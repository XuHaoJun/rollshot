# Evaluation: Unify Rollshot's GUI on iced (drop Tauri/React from the capture/UI path)

**Date:** 2026-06-03
**Status:** Forward-looking research / decision note (see `AGENTS.md` §10 — *not* a frozen
superpowers spec, and not yet an implementation plan).
**Decision:** **Adopt Approach A — finish the iced migration.** Rewrite the macOS
Tauri/webview overlay as an iced overlay; build the future image editor and settings
pages in iced as well. One GUI framework across all platforms.
**Scope of this note:** evaluation + recommendation only. No migration is performed
here; an implementation plan is a separate, later step.

---

## 1. TL;DR

Rollshot is **already ~50% migrated to iced**. The Linux capture overlay
(`rollshot-overlay`) is a production iced + `iced_layershell` application — it carries
all of the recent overlay/stitch-preview hardening (PRs #25–#28). The remaining
non-iced UI is the **macOS overlay**, which is React-in-a-Tauri-webview. The two
renderers already share their brain (`rollshot-overlay-core`, framework-neutral).

The stated driver — *"two overlay codebases"* — is therefore precisely **one iced
renderer (Linux) and one React renderer (macOS) of the same `overlay-core` logic.**
The cheapest correct fix is not "adopt a new framework"; it is **render the macOS
overlay with iced too**, using a plain transparent always-on-top window (no
layer-shell needed off Wayland). That collapses the duplication at the root, removes
the entire web toolchain, and gives a single GUI stack for the planned image editor,
settings, and system tray — and for the future Windows target.

---

## 2. Drivers and constraints (from the requester)

- **Primary pain:** maintaining **two overlay codebases** (native iced on Linux vs.
  React/webview on macOS) — the dual-path burden called out in `AGENTS.md` §7.
- **Platforms:** Linux + macOS today; **Windows planned** later.
- **Future direction:** image editor and settings pages should be **iced** too —
  one unified GUI framework (no JS/web layer).
- **Reference for "rich features":** `learn-projects/snow-shot` (annotation tools,
  OCR/translate, pin-to-screen, scrolling capture, global hotkeys, tray).

---

## 3. Current architecture (verified against code)

### Framework-neutral core (survives any GUI decision)

| Crate | Role | GUI deps |
|---|---|---|
| `rollshot-core` | Stitching IP (matcher, canvas, verifier, stitcher) | none |
| `rollshot-capture` | Capture backends: Linux portal/PipeWire, macOS ScreenCaptureKit (`scap`, feature-gated) | none |
| `rollshot-overlay-core` | **Shared overlay UI logic**: live-preview viewport generator (`preview.rs`), capture-miss recovery (`capture_miss.rs`), crop visual tokens (`tokens.rs`). Its own doc comment: *"No iced / Tauri / webview deps."* | none |

### Linux capture path — **already iced**

- `rollshot-overlay` (`Cargo.toml`): `iced = "0.14"` (features `canvas`, `image`,
  `tokio`) + `iced_layershell = "0.18"`, Linux-gated.
- `overlay.rs` (~768 LoC) is built on `iced_layershell::build_pattern::application`,
  `to_layer_message`, `LayerShellSettings` (anchored full-screen `Layer::Overlay`,
  exclusive keyboard). `driver.rs` (~546) runs portal capture + the reader thread;
  `coords.rs` (~208) handles coordinate mapping. Renders crop + live stitch preview
  on iced's canvas/wgpu.
- The Tauri host webview is **created but hidden** on Linux (`lib.rs`
  `setup_host_window`): kept alive only so its GPU context coexists with wgpu; never
  shown or focused.

### macOS / non-Linux capture path — React + Tauri webview

- Frontend (~3.3K LoC TS/TSX): `CaptureOverlay.tsx` (391), `SelectionLayer.tsx`,
  `RegionOverlay.tsx`, `overlay/placement.ts` (308), `region/geometry.ts` (107),
  `App.css`. React 19 + Tailwind 4 + Radix + lucide.
- `src-tauri/src/session.rs` (1,386 LoC): webview capture/session state, save,
  final-preview orchestration (parts `#[cfg(not(target_os = "linux"))]`).
- Tauri command surface in `lib.rs` (`start_capture`, `confirm_region`,
  `get_stitch_preview`, `get_final_preview`, `save_image`, …).

### The duplication, precisely

```
                         rollshot-overlay-core   (one source of truth: viewport, crop tokens, capture-miss)
                          /                    \
   Linux: iced renderer  <                      >  macOS: React renderer
   (rollshot-overlay,                              (CaptureOverlay.tsx + placement.ts,
    iced + iced_layershell)                         orchestrated by Tauri session.rs)
```

Two renderers of one logic. Every capture-UI change must be made twice and kept in
sync — the exact friction `AGENTS.md` §7 documents.

---

## 4. Why iced is the right consolidation target

This is **not** a green-field framework bet. The hardest surface — a transparent,
always-on-top, full-screen Wayland overlay with live wgpu image streaming — is
**already shipping on iced** in this repo, refined across PRs #25–#28. Finishing the
migration is lower-risk than the typical "should we adopt iced" question because the
risky part is already answered in production.

External corroboration of iced's production-readiness for a full GUI:
**Pop!\_OS 24.04 LTS (released 2025-12-11) ships System76's COSMIC desktop**, whose
toolkit `libcosmic` is built on iced. (Note: `libcosmic` itself is **not** an option
for Rollshot — it is Linux-DE-focused with no real macOS/Windows story. The Rollshot
stack is **vanilla iced + `iced_layershell` + `tray-icon` + `global-hotkey`**.)

### Component fitness

| Capability | Verdict | Detail |
|---|---|---|
| Wayland overlay | ✅ Done | `iced_layershell` 0.18.1 tracks iced 0.14; in production here. |
| macOS overlay | ✅ Feasible | Transparent, borderless, always-on-top iced window (winit). **No layer-shell off Wayland.** |
| Windows (future) | ✅ Feasible | Same transparent-topmost window as macOS. One toolkit everywhere — simpler than today's split. |
| Capture | ✅ Untouched | `rollshot-capture` already abstracts portal (Linux) vs ScreenCaptureKit (macOS). |
| System tray | ✅ Crate | `tray-icon` (the crate the Tauri ecosystem itself builds on) + iced daemon mode. SNI on Linux, NSStatusItem on macOS, Shell_NotifyIcon on Windows. |
| Global hotkeys | ✅ Crate | `global-hotkey` (what Tauri's global-shortcut plugin wraps). Cross-platform. |
| Image editor / annotation | ⚠️ Real build cost | iced `canvas` is viable and consistent with the overlay (which already uses it), but a layered editor (text/arrow/mosaic/undo) is hand-built vs. grabbing an HTML-canvas lib. Accepted as a deliberate cost of one unified stack. |
| Settings pages | ✅ Easy | Forms/lists/toggles are squarely in iced's wheelhouse. |
| Maturity | ⚠️ label, ✅ practice | iced 0.14 README still says "experimental," but COSMIC ships on it and Rollshot already runs it on its overlay. |

---

## 5. Target architecture (after migration)

```
   rollshot-core / rollshot-capture / rollshot-overlay-core   (unchanged, framework-neutral)
                                  |
                         one iced overlay view
                    (Element<Message> + update logic)
                    /             |               \
        Linux runner        macOS runner        Windows runner (future)
   iced_layershell::      iced/winit window    iced/winit window
   application            transparent + AOT    transparent topmost
   (Layer::Overlay)       + Cocoa patch        (+ opt. Win32 patch)
                                  |
                  iced daemon host  ──  tray-icon + global-hotkey
                                  |
              future iced pages: image editor, settings, gallery
```

- **One overlay view** written against **plain iced** widget/`Element<Message>` types;
  only the **top-level runner** differs per platform (layer-shell vs. normal window).
- `rollshot-overlay-core` stays the single source of truth — unchanged.
- Capture stays in `rollshot-capture`. macOS capture/session orchestration currently
  in `session.rs` moves into an iced-driven flow analogous to Linux's `driver.rs`
  (detailed in §6).
- Tray, hotkeys, and the future editor/settings live in the same iced process.

### Per-platform layering

The split is confined to the **window shell + a small native patch**; everything above
it is shared. Linux is already implemented; macOS and Windows are the targets.

| Layer | Linux — KDE 6 / Wayland ✅ done | macOS — target | Windows — future target |
|---|---|---|---|
| UI (widgets, canvas, preview) | iced | iced | iced |
| Window shell | `iced_layershell` (`Layer::Overlay`, exclusive input) | iced / winit window (transparent, borderless, `AlwaysOnTop`) | iced / winit window (transparent, borderless, topmost) |
| Native window patch | none — layer-shell covers anchoring + input region | **Cocoa**: `setCollectionBehavior:` + `setHasShadow:` via `objc2` — fork-free (§7) | **likely none**; optional `WS_EX_TOOLWINDOW`/`WS_EX_NOACTIVATE` via `windows-sys` — fork-free (§7) |

**Shared across all three (the bulk of the code):** application **state**, the
**message** enum, **components**/widgets, **commands**, and the **capture + stitch
flow** — i.e. `rollshot-overlay-core`, the `Driver` engine, `Stitcher`, and
`rollshot-capture` backends. Only the window-shell row and the native-patch row differ
per platform.

### What gets deleted

- Tauri runtime + `@tauri-apps/*` and `tauri-plugin-dialog`.
- The React frontend (~3.3K LoC) and its toolchain (Vite, pnpm, Tailwind, Radix,
  vitest) for the app.
- WebKitGTK coexistence hacks: `webkit_workaround.rs` and the "keep the hidden
  webview alive so wgpu and webkit don't fight" dance in `lib.rs` /
  `tauri.conf.json`. Removing this is itself a maintenance win.

---

## 6. Cost of migrating the macOS overlay (Tauri → iced)

**It is not "rewrite `session.rs` in iced."** The Linux iced path already contains a
tested reimplementation of the macOS capture/stitch state machine. `driver.rs`'s own
comments say so: *"matches the Tauri app default, session.rs:188-190"* (`driver.rs:67`)
and *"Mirrors the crop+push+finalize of session.rs:199-212,214-231"* (`driver.rs:76`).
`session.rs` (`SharedSession`/`AppSession`) and `rollshot-overlay/driver.rs` (`Driver`)
are two implementations of the same pipeline; only the **transport** differs:

| | macOS (`session.rs`) | Linux (`driver.rs`) |
|---|---|---|
| UI gets state via | React **polls** `session_status` every 160 ms | `Driver` **pushes** `LiveOverlayEvent` over an iced mpsc channel |
| Preview delivery | `Stitcher` → **PNG-encode** → IPC blob → `URL.createObjectURL` | `Stitcher` → **`ImageHandle::from_rgba`** straight into wgpu |
| Capture backend | scap / ScreenCaptureKit via `rollshot-capture` | portal / PipeWire via `rollshot-capture` |

So macOS **adopts `Driver`** and **deletes** the poll + PNG-over-IPC + DTO layer. Note
~half of `session.rs`'s 1,386 lines are tests; the superseded production code is ~700
lines, most of it deleted rather than ported.

### Reuse / New / Delete

**Reusable as-is (the expensive IP, already framework-neutral):** `rollshot-core`,
`rollshot-overlay-core` (viewport preview, capture-miss, tokens), `coords.rs`
(crop→frame), `rollshot-capture` (scap behind `BackendKind`/`FrameStream`), the
`Driver` engine, and the iced `view` (crop rect, handles, preview, toolbar) once
decoupled from layer-shell. *Verify:* scap is a registered `BackendKind`, and its
stream's `Send`-ness (the `SendStream` unsafe-Send in `driver.rs:44-58` is justified
for PipeWire's `Rc` handles; macOS may need none or its own).

**New code (the actual work):**

1. **macOS window runner** — plain `iced::application`/`daemon` with
   `window::Settings { transparent, decorations: false, level: AlwaysOnTop }` sized to
   the screen, replacing `iced_layershell`'s builder. (Window-API feasibility: §7.)
2. **Make `overlay.rs` runner-agnostic** — split `view`/`update`/state from the
   layer-shell-specific message/runner so both runners drive the same code. **This is
   the central refactor (§8 #1).**
3. **macOS capture/permission flow** — ScreenCaptureKit TCC permission (+ display pick
   if needed) replacing the Linux portal-handshake ordering.
4. **macOS input passthrough** — `window::enable/disable_mouse_passthrough` (native,
   §7); the toolbar-clickable-during-passthrough case → a second always-interactive
   toolbar window.
5. **Final-preview / save / "done" state in iced** — replaces `getFinalPreview` +
   `promptSaveStitchedPng` + the React "done" overlay; save dialog via `rfd`. This is
   the hand-off point to the editor (§9).

**Deleted (negative cost):** the React overlay (`CaptureOverlay.tsx` + `SelectionLayer`/
`RegionOverlay`/`OverlayToolbar`/`AdaptiveStitchPreview` + `placement.ts` + `geometry.ts`
+ `App.css` + tests, ~3.3K TS); `api/capture.ts`/`save.ts`; `session.rs`'s Tauri-facing
half (polling commands, `encode_preview_*`, the `*Dto`/`SessionStatus` IPC types);
`commands.rs`, Tauri `overlay.rs`, `webkit_workaround.rs`; the Tauri runtime,
`tauri-plugin-dialog`, and the JS toolchain.

**Bonus dedup:** `placement.ts` (dynamic preview placement, 308 LoC) is TS-only today
and mirrored in `overlay.rs`; unifying on iced collapses it — but the Rust placement
must reach parity with the TS version.

### Effort

- **Spike (de-risk §8 #1 + the §7 window behavior): ~2–4 days.**
- **macOS overlay to parity: ~2–4 weeks** (one engineer), dominated by the
  runner-agnostic refactor + macOS window behavior, **not** the capture/stitch logic
  (`Driver` already owns it).
- **Net LoC shrinks** (one renderer, not two; large IPC layer deleted). The
  stitching/capture/preview IP is untouched.

---

## 7. macOS / Windows window-API parity: native vs. native patch

The macOS runner needs five window properties. Three are first-class iced APIs; two
need a small **fork-free** native patch (iced exposes the raw window handle, and the
app already depends on `raw-window-handle = "0.6"`):

| # | Need | iced native? | Mechanism |
|---|---|:---:|---|
| 1 | transparent + no decorations | ✅ | `window::Settings { transparent: true, decorations: false }` |
| 2 | AlwaysOnTop / floating level | ✅ | `Settings { level: AlwaysOnTop }` + runtime `window::set_level` |
| 3 | collectionBehavior (all Spaces / fullscreen aux) | ❌ | raw `NSWindow` `setCollectionBehavior:` — patch |
| 4 | ignoresMouseEvents (click-through toggle) | ✅ | `window::enable_mouse_passthrough` / `disable_mouse_passthrough` (winit `set_cursor_hittest`; macOS + Windows) |
| 5 | hasShadow = false | ❌ | raw `NSWindow` `setHasShadow:` — patch |

**Fork-free patch path:** `window::raw_window_handle(id)` / `window::run(id, |w| …)`
yields the `AppKitWindowHandle` (`ns_view`) → `view.window()` → `NSWindow`, then call the
setters via `objc2`/`objc2-app-kit`. Applied **once on window-open**, on the **main
thread** (where iced tasks already run). ~20–40 lines, macOS-gated, coupled to AppKit
(stable) not iced internals, so it survives iced upgrades. snow-shot already pulls in
`objc2`/`objc2-app-kit`/`objc2-foundation` and does exactly this kind of NSWindow work.

**iced vs Tauri delta is one line.** Tauri natively covers 1, 2, 4 and **5**
(`set_shadow`), but **not 3** — collectionBehavior needs a raw `ns_window()` objc patch
in Tauri too. iced covers 1, 2, 4 and needs the patch for 3 **and** 5. The only extra
thing iced costs is `setHasShadow:false` — one more setter inside a patch helper you
must write for collectionBehavior regardless of framework. Not material.

**Whole-window caveat (#4).** `enable_mouse_passthrough` is whole-window (like
`setIgnoresMouseEvents` itself). "Toolbar clickable while the rest is click-through" →
a second, always-interactive toolbar window (iced multi-window). Linux gets per-region
passthrough free via layer-shell `input_region`; macOS/Windows don't. This is a UX
architecture decision for the spike, not an API gap.

**Windows — is a patch needed?** For these five: **essentially no.** iced covers
transparent / decorations / level / mouse-passthrough natively; there is no Spaces /
collectionBehavior concept on Windows; and a borderless (`decorations: false`) window
has no drop shadow by default. *Optional* behavioral patches — `WS_EX_TOOLWINDOW` (hide
from taskbar / Alt-Tab), `WS_EX_NOACTIVATE` (don't steal focus) — are fork-free via the
already-present `windows-sys` dep (raw `Win32WindowHandle` → `HWND` →
`SetWindowLongPtrW`). Treat Windows patching as optional polish, not a blocker.

---

## 8. Risks and caveats

1. **`iced_layershell` is a parallel API to mainline iced.** It has its own
   `build_pattern::application` and `to_layer_message`. Sharing one overlay across
   layer-shell (Linux) and normal-window (macOS/Win) runtimes requires the widget
   tree + `update` to be written against **plain iced** types, with a thin
   per-platform runner. The `Element<Message>` type is identical, so this is
   achievable — but it is the **main engineering subtlety**. De-risk with a parity
   check: transparency, input regions / passthrough, multi-monitor, and wgpu image
   streaming on the macOS normal-window path.
2. **GNOME Wayland.** Mutter does not implement `wlr-layer-shell` (only KWin +
   wlroots-based compositors do; the requester targets **KDE 6 / Wayland**, which is
   fine). **This is an existing constraint** — Rollshot's overlay is layer-shell today
   — *not* introduced by this migration. Document as a known limitation (KDE/wlroots,
   or portal/X11 fallback); do not let it gate the decision.
3. **macOS port effort.** Reimplement `session.rs`'s capture/session/save/
   final-preview orchestration plus retire the React overlay (~3.3K TS) in iced —
   sized in §6. Net UI code is expected to **shrink** (one renderer, not two); the
   stitching/capture IP is untouched.
4. **Image editor effort.** The annotation editor is the one place the web stack was
   genuinely easier. Building it on iced `canvas` is a deliberate, accepted cost of a
   single unified GUI (decision + de-risking spike in §9). Stay consistent with the
   overlay's existing canvas usage.
5. **iced API churn.** iced pre-1.0 still breaks between minors; `iced_layershell`
   must keep pace (0.18.1 currently tracks 0.14). Pin versions; budget for periodic
   upgrades. Mitigated by COSMIC's large-scale dependence keeping iced 0.14 healthy.

---

## 9. Decision point: build the image editor in iced (not React)

The next planned work is the preview / image editor. Because the editor is the piece
that most tempts a return to the web stack — and the piece that most firmly *locks it
in* — this is the decisive fork for the whole unification.

**Decision: build the editor in iced.**

Rationale, in priority order:

1. **The editor is the point of no return.** Overlays are bounded; editors grow
   without bound (cf. snow-shot's editor). Build it in React and you get one of two
   endings: (a) later migrate a mature, stateful editor React→iced — far costlier and
   riskier than writing it in iced from scratch today; or (b) never migrate, and the
   webview/Tauri stack is pinned alive forever by the editor — the "one GUI framework"
   goal dies. Building it in iced now pays the cost at the moment of lowest sunk cost
   (the editor is still zero lines), and it is what makes phase 3 (§10, retire
   Tauri/React) actually possible.
2. **An iced editor reuses the overlay's existing canvas muscle.** The hard editor
   interactions already exist on iced canvas/wgpu in the overlay: rubber-band
   selection, handle drag/resize, coordinate mapping, live image rendering
   (`rollshot-overlay/coords.rs`, `overlay.rs`, plus `rollshot-overlay-core`). An
   annotation editor is those primitives + annotation objects (arrow/rect/text/mosaic)
   + undo/redo. iced shares them directly; a React editor would re-derive the
   interaction logic in TS and share nothing with the iced overlay — maximizing the
   duplication we set out to remove.
3. **iced explicitly targets this app class.** Its `canvas` docs name "a Paint clone,
   a CAD application" as intended uses; `text_editor`/`text_input` (multi-line,
   clipboard, IME) + canvas `Text` + `stack` layering are all present; `iced-code-editor`
   is canvas-based prior art. This is a "do it," not a "can it be done."

**The one real risk to de-risk first: in-canvas *editable text* UX.** This is what the
web stack (contenteditable) makes trivial and iced makes hand-built. The pieces exist
(`text_editor`/`text_input` layered over canvas via `stack`), so this is *validate the
UX*, not *prove feasibility*.

**Recommended before committing the full editor — a 1–2 day iced editor spike** proving
only:

1. A text annotation: place a text box → edit inline → move/resize it.
2. An arrow or rectangle object: select / move / resize (reusing the overlay's handle
   logic).
3. Undo/redo + export the composition to an image.

If the spike passes, commit fully to iced. If it snags (only plausibly on text UX), you
have spent two days, not a rewrite, to learn it.

**When React would win:** only if "ship the editor fast" outranked "unify the
framework." That is the opposite of the stated direction (all-platform iced, one GUI,
future Windows), so for these goals iced is the correct call.

---

## 10. Suggested phased path (for the later implementation plan — not executed here)

1. **Spike (de-risk):** a throwaway macOS iced window proving transparency +
   always-on-top + input passthrough + wgpu live-preview streaming, driven by
   `rollshot-overlay-core`. Confirms §8 #1 and the §7 window behavior before committing.
2. **macOS overlay port:** refactor `rollshot-overlay`'s view/update to be
   runner-agnostic; add a macOS runner (transparent iced window + Cocoa patch) and move
   macOS capture orchestration out of `session.rs` into the iced flow. Reach
   overlay/stitch/save parity with Linux.
3. **Retire Tauri/React from capture:** delete the webview overlay, the React app,
   the web toolchain, and the WebKitGTK coexistence hacks. The host becomes an iced
   process (daemon-capable).
4. **Tray + hotkeys:** integrate `tray-icon` + `global-hotkey` on the iced host.
5. **Rich features in iced:** settings, gallery, then the image/annotation editor on
   iced `canvas` (§9).
6. **Windows:** reuse the macOS transparent-window runner (Win32 patch optional, §7).

Each phase is independently shippable and keeps the framework-neutral core untouched.

---

## 11. Alternatives considered (and why not)

- **B — keep Tauri host, run iced overlay as a child process on macOS** (mirroring
  how Linux launches the native overlay). Smaller change, keeps a webview escape
  hatch for the editor — but preserves "two stacks," the WebKitGTK baggage, and
  contradicts the goal of one GUI framework. Viable only as a phase-1 fallback if the
  spike (step 1) surfaces blockers.
- **C — status quo, push more logic into `overlay-core`.** Cheapest; thins both
  renderers and reduces drift. But it still maintains two renderers and does nothing
  for Windows, tray, or the iced editor/settings direction. Insufficient for the
  stated goals.

---

## 12. Sources

- iced — repo & site (v0.14, Dec 2025; "experimental"): <https://github.com/iced-rs/iced>, <https://iced.rs/>, <https://docs.rs/iced/>
- `iced-layershell` (0.18.1 targets iced 0.14): <https://crates.io/crates/iced-layershell>, <https://docs.rs/iced-layershell/>
- iced window APIs (§7): `Settings` <https://docs.rs/iced/latest/iced/window/settings/struct.Settings.html>, window module + `enable/disable_mouse_passthrough` + `raw_window_handle`/`run` <https://docs.rs/iced/latest/iced/window/index.html>, mouse-passthrough request <https://github.com/iced-rs/iced/issues/2283>, winit `set_cursor_hittest` <https://github.com/rust-windowing/winit/pull/2232>
- iced vs Tauri (architecture/maturity): <https://buildwithrust.com/iced-vs-tauri-2-we-built-the-same-app-twice-in-rust>
- COSMIC / libcosmic on iced; Pop!\_OS 24.04 LTS (2025-12-11): <https://en.wikipedia.org/wiki/COSMIC_desktop>, <https://blog.system76.com/post/cosmic-alpha-5-released/>
- `wlr-layer-shell` & GNOME/Mutter gap: <https://wayland.app/protocols/wlr-layer-shell-unstable-v1>, <https://gitlab.gnome.org/GNOME/mutter/-/issues/973>
- System tray for iced/winit (`tray-icon`, `tray`): <https://lib.rs/crates/tray>, <https://github.com/Ciantic/trayicon-rs>
- Rust/Wayland screenshot tools (prior art): <https://github.com/waycrate/wayshot>, <https://github.com/Thirdwinter/foamshot>, <https://sr.ht/~whynothugo/shotman/>
- iced editor building blocks (§9): canvas "Paint clone / CAD" use case <https://docs.rs/iced/latest/iced/widget/canvas/index.html>, `text_editor` <https://docs.rs/iced/latest/iced/widget/struct.TextEditor.html>, `iced-code-editor` prior art <https://crates.io/crates/iced-code-editor>
