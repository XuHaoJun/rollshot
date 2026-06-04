# No-Tray Focus Recovery Design

## Status

Approved design. This spec is live for the next implementation plan.

## Problem

After the user selects a crop and Rollshot enters live scrolling capture, clicking
another window can cause the overlay to stop receiving `Esc`. When that happens,
the user can be unable to finish or cancel capture from the keyboard.

This is a focus/keyboard-delivery problem, not a system tray problem. The design
must avoid adding a system tray for this fix.

## Reference Findings

Flameshot treats capture shortcuts as overlay-local. When the capture widget
loses focus, it tells the user keyboard shortcuts will not work until they click
back into the capture UI. It does not use a global `Esc` hook for active capture.

wayscrollshot documents the same limitation on Wayland: keyboard shortcuts only
work when the overlay is focused. It provides a clickable control bar as the
reliable fallback.

Snow Shot mostly attempts focus restoration by calling `setFocus()` when capture
UI is shown or when passthrough is toggled. Its screenshot `Esc` path remains
window/webview-focus dependent.

The common lesson is: `Esc` should remain a convenient focused-overlay shortcut,
but it should not be the only way to leave active capture.

## Goals

- Keep the fix inside the overlay flow; do not introduce a system tray.
- Preserve `Esc` as the fast path when the overlay receives keyboard events.
- Prevent active capture from becoming stuck when `Esc` is not delivered.
- Make focus loss understandable instead of silent.
- Keep controls outside the crop so they are not included in captured frames.
- Apply the same user-facing behavior to Linux and macOS iced overlay paths.

## Non-Goals

- Do not implement a global `Esc` hook.
- Do not add a system tray, menu bar item, daemon command, or DBus/IPC control.
- Do not redesign the capture UI beyond the minimal control chrome needed here.
- Do not change stitching behavior, crop mapping, capture backends, or save
  handoff.

## Design

Use a Flameshot-like focus recovery model plus Rollshot-specific explicit
controls.

In the confirmed-crop capture phase, upgrade the existing capture chrome from a
text-only status label into a minimal control strip:

- Status text:
  - focused/normal: `Capturing - scroll the target`
  - focus-loss known: `Shortcuts paused - click Rollshot controls to restore Esc`
- Buttons:
  - `Finish`: finalizes capture, equivalent to the current focused `Esc` behavior.
  - `Cancel`: cancels capture and exits without producing a result.

The control strip must continue to use the existing outside-crop placement logic
so it does not overlap the selected crop interior. If there is not enough room
outside the crop for the full strip, use a compact control strip that prioritizes
`Finish` and `Cancel` over preview/status content.

Clicking the control strip should attempt to restore focus to the overlay where
the platform runner supports it. `Esc` remains best-effort and local to the
overlay; the explicit buttons are the reliable no-tray escape path.

## Platform Behavior

### Linux

The Linux iced overlay currently uses a layer-shell overlay with exclusive
keyboard interactivity and narrows the input region to the toolbar area after
crop confirmation. The implementation should keep the explicit controls inside
that input region and verify that clicking the controls can still finish/cancel
after the user has clicked the target window.

If reliable focus-loss detection is not available through iced/layer-shell, the
Linux path may always show the neutral status text and still provide `Finish` /
`Cancel`. Do not block the fix on perfect focus-loss detection.

### macOS

The macOS iced overlay enables mouse passthrough after crop confirmation so the
target can receive scroll input. The implementation should keep or restore a
clickable overlay control area and attempt to refocus the overlay when the user
clicks the controls.

If macOS focus-loss detection is unreliable without native event monitoring,
prefer the explicit controls over platform-specific global key listeners.

## State And Data Flow

The overlay state already tracks `crop_confirmed` and maps `Esc` to
`OverlayEffect::Finish` in the confirmed phase. The control strip should expose a
capture-phase finish action that returns `OverlayEffect::Finish` while
`crop_confirmed` is true. This can be a new overlay message or a careful
adjustment to the existing finish handling; it must not regress the selection
phase's empty-crop validation. `Cancel` should return `OverlayEffect::Cancel`.

The platform runners continue to own side effects:

- `Finish` finalizes the `Driver`, stores the result, and exits the iced app.
- `Cancel` cancels the `Driver`, stores `Ok(None)`, and exits the iced app.

The shared overlay UI should remain in `rollshot-iced-overlay` so Linux and
macOS behavior stays aligned.

## UX Copy

Use short operational copy:

- Normal: `Capturing - scroll the target`
- Focus-loss known: `Shortcuts paused - click Rollshot controls to restore Esc`
- Buttons: `Finish`, `Cancel`

Avoid explanatory paragraphs inside the overlay.

## Testing And Verification

Automated tests should cover the shared overlay state where practical:

- Capture-phase `Finish` control returns `OverlayEffect::Finish`.
- `Cancel` message returns `OverlayEffect::Cancel`.
- Capture-phase view includes explicit `Finish` and `Cancel` controls.
- Existing `Esc` behavior remains unchanged.
- Outside-crop placement tests remain valid.

Manual runtime verification should cover:

- Linux: select a crop, click/scroll the target window, then finish with the
  overlay control button.
- Linux: select a crop, press `Esc` while overlay shortcuts still work.
- Linux: cancel from the overlay control button.
- macOS: select a crop, click/scroll the target window, then finish with the
  overlay control button.
- macOS: cancel from the overlay control button.

Run the relevant Rust checks after implementation:

- `rtk cargo test -p rollshot-iced-overlay`
- Broader workspace checks if implementation touches shared crates beyond the
  overlay.
