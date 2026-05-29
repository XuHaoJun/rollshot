# Linux Wayland Layer-Shell Roadmap

Status: approved roadmap, not an implementation plan.

## Goal

Replace the Linux Tauri fullscreen capture overlay with a native Wayland
layer-shell capture flow for KDE 6 Wayland, while preserving the current user
behavior:

```text
crop select -> scroll with live stitching preview -> Esc -> Tauri save dialog
```

The roadmap exists to decompose the work into smaller superpowers specs and
plans. Each phase below should get its own spec/plan before code is changed.

## Support Boundary

Primary target:

- KDE 6 Wayland.

Opportunistic targets:

- Other Wayland compositors that support layer-shell, such as wlroots-based
  compositors, niri, Hyprland, and sway.

Not promised by this roadmap:

- GNOME Wayland.
- X11.
- A hard compatibility guarantee for non-KDE compositors.

Rule: if any implementation needs KDE-specific behavior or a KDE workaround,
the relevant spec and plan must call that out explicitly. KDE-specific code must
not be hidden inside a supposedly generic layer-shell backend.

## Current Problem

The current Tauri overlay owns too many Linux capture responsibilities:

- Crop selection.
- Fullscreen transparent overlay behavior.
- Stitching live preview.
- Overlay controls.
- Save flow handoff.

On KDE 6 Wayland, fullscreen/always-on-top overlay behavior is not reliable
enough for capture UX. The Linux capture overlay needs to use Wayland
layer-shell directly instead of relying on a normal Tauri window.

## Architecture Direction

Keep the capture pipeline reusable, but move the Linux capture UI out of the
Tauri fullscreen window.

```text
Linux KDE 6 Wayland

  layer-shell capture UI
    - crop picker
    - live stitching preview
    - finish/cancel controls
    - Esc finish event

  rollshot-capture
    - portal / PipeWire capture

  rollshot-core
    - stitching

  Tauri
    - save dialog handoff for current roadmap
    - future settings/result/editor host
```

Tauri remains part of Linux, but it should not be required for fullscreen
capture overlay behavior. In this roadmap, Tauri is only required to preserve
the current save dialog behavior after capture finishes.

## Renderer And Toolkit Direction

First candidate:

- `iced_layershell`.

Fallback:

- Direct `smithay-client-toolkit` / `wayland-client` layer-shell code with a
  simpler renderer such as `tiny-skia`.

The first implementation spec must be a feasibility spike. Do not start product
integration until the spike proves the chosen toolkit can handle the KDE 6
Wayland requirements.

## Roadmap Phases

### Phase 1: Architecture Spec

Purpose:

- Lock the crate/module boundaries.
- Define the handoff between native Linux capture UI and Tauri.
- Define the support matrix and non-goals.
- Define how future superpowers specs/plans should be split.

Expected output:

- A superpowers spec describing the Linux capture architecture.
- A superpowers plan for the first feasibility spike.

No production code should be required in this phase.

### Phase 2: `iced_layershell` Feasibility Spike

Purpose:

- Prove whether `iced_layershell` can be the Linux layer-shell capture UI stack.

KDE 6 Wayland acceptance checks:

- Create a transparent layer-shell overlay above fullscreen apps.
- Draw a crop rectangle.
- Render text and simple controls.
- Show a live preview image and update it repeatedly.
- Configure input behavior so the overlay can receive crop/toolbar input without
  permanently blocking scroll input to the target content.
- Handle Esc as a finish/cancel control path.
- Behave predictably on at least one multi-monitor setup, or document the
  blocking issue.

Decision gate:

- If the spike passes, use `iced_layershell` for the native Linux overlay.
- If the spike fails on transparency, input region, preview refresh, or KDE 6
  layer behavior, switch the next spec to the fallback stack.

Status (2026-05-29): **DONE — GO.** Runtime-verified on KDE 6.6.5 Wayland /
NVIDIA RTX 5070 Ti. R1 GPU coexistence and R6 (transparency / above-fullscreen
layer / Esc / scroll passthrough / live-preview refresh) PASS; R2 focus and R3
self-capture are documented with mitigations (carried into Phase 3 below). The
`smithay-client-toolkit` fallback is NOT triggered. Decision record + full risk
table: `spikes/layershell-feasibility/FINDINGS.md`. The throwaway spike crate
(`spikes/layershell-feasibility/`, workspace-detached) is kept as the Phase 3
implementation reference.

### Phase 3: Native Linux Capture Overlay

Purpose:

- Replace the Linux Tauri fullscreen crop picker and live stitching overlay.

Scope:

- Native layer-shell crop picker.
- Native layer-shell live stitching preview.
- Finish/cancel controls.
- Esc finishes stitching.
- Existing `rollshot-capture` and `rollshot-core` remain the capture/stitching
  engines.

Constraints:

- Do not rewrite stitching.
- Do not make `rollshot-core` or `rollshot-capture` depend on Tauri or the
  overlay UI.
- Keep the first production path focused on monitor/full-source capture unless a
  spec proves window-capture coordinate mapping is reliable.

Acceptance checks:

- On KDE 6 Wayland, the overlay appears above fullscreen apps.
- The user can select a crop region.
- The user can scroll the target content while stitching is active.
- The live stitching preview updates during scrolling.
- Pressing Esc finishes stitching and triggers the save handoff.

Status (2026-05-29): **DONE — implementation complete; KDE 6 runtime acceptance pending.**

The `rollshot-overlay` crate is built, tested, and integrated. All unit tests pass
(6 tests: 5 coords + 1 driver core). Workspace-wide verification clean (332 tests,
clippy, fmt). The harness binary (`capture_overlay`) compiles. Runtime acceptance on
KDE 6 Wayland is deferred to the next available KDE 6 session.

Carried over from the Phase 2 spike (must address in the Phase 3 spec/plan):

- **R2 focus (design decision):** during the overlay phase, hide or de-focus the
  Tauri host window so a focusable toplevel cannot steal keyboard focus from the
  exclusive-keyboard layer (the `coexist` spike showed KWin doing this). Keep
  webkit's GPU context alive for R1, but its window need not be visible/focusable.
- **R3 self-capture (must implement):** Linux has no exclude-from-capture, so the
  overlay is composited into portal/PipeWire frames. Hide overlay chrome during
  capture frames — keep chrome outside the crop rect and crop to the region, or
  unmap / transparent-ify the overlay per capture frame (flameshot/spectacle
  pattern).
- **Runtime tests not yet run in the spike** (exercise during Phase 3): R4
  fractional scaling at 100% and 150% (the D4 coordinate-mapping risk), R5
  output match (overlay output == captured monitor), R7 multi-monitor.

Carried into Phase 4:

- **Tauri wiring:** `src-tauri` Linux branch spawns `run_overlay` on a thread
  and feeds `CaptureResult.image` into `AppSession`'s final-image + save flow.
- **Tauri save dialog handoff (D5).**
- **R2:** hide / de-focus the Tauri host window during the overlay phase.
- **R5/R7 multi-output follow-up** from Phase 3 acceptance (if not resolved).

### Phase 4: Tauri Save Handoff

Purpose:

- Preserve the current end-user behavior after native Linux capture finishes.

Scope:

- Native Linux overlay produces or finalizes the stitched image.
- Tauri opens the existing save dialog flow.
- Saved PNG output matches the current behavior.

Acceptance checks:

- User presses Esc during stitching.
- Stitching stops.
- Tauri save dialog opens.
- User can save the stitched PNG.
- Canceling the save dialog does not lose process control or leave the capture
  session running.

Naming rule:

- The handoff API should not be named in a way that permanently assumes "save PNG
  only". Future editor/image/video/GIF/multi-output flows should be able to
  reuse the same handoff concept, but those features are not in this roadmap.

## Explicitly Out Of Scope

- Tauri image editor.
- Clipboard integration.
- Video export.
- GIF export.
- Multi-output export UI.
- Settings UI redesign.
- GNOME Wayland support.
- X11 support.
- Hard support guarantees for non-KDE compositors.
- Arbitrary window-capture crop mapping unless a later spec proves it reliable.

## End-To-End Acceptance

The roadmap is complete when this flow works on KDE 6 Wayland:

```text
1. User starts Rollshot capture.
2. Native layer-shell overlay appears above fullscreen apps.
3. User drags a crop region.
4. User scrolls target content.
5. Live stitching preview remains visible and updates.
6. User presses Esc.
7. Tauri save dialog opens.
8. Saved PNG output matches current behavior.
```

## Follow-Up Specs And Plans

Create and approve separate superpowers specs/plans in this order:

1. Linux layer-shell architecture spec.
2. `iced_layershell` feasibility spike spec and plan.
3. Native Linux capture overlay spec and plan.
4. Tauri save handoff spec and plan.

Do not merge these into one large implementation plan. The first unknown is
toolkit feasibility, so the spike must remain a decision gate before product
integration.
