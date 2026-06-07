# Linux Save Dialog Handoff Design

## Status

Approved design. This spec is live for the next implementation plan.

## Problem

The Linux Result Review workspace is a full-screen layer-shell overlay. Opening
the synchronous native Save dialog while that overlay remains open can place the
dialog behind the overlay, leaving the user unable to see or interact with it.

Spectacle avoids this conflict by hiding and destroying its full-screen capture
windows before continuing into export UI. Rollshot should follow the same
window-lifecycle principle on Linux.

## Scope

- Change Linux Result Review `Save` only.
- Keep Linux `Copy` and `Close` behavior unchanged.
- Keep the macOS Result Review output flow unchanged.
- Do not reopen Result Review after Save dialog cancellation or failure.

## User Experience

When the user clicks `Save` in Linux Result Review:

1. The Result Review workspace closes.
2. The completed full-resolution capture returns to `rollshot-app` with a
   request to open Save As.
3. `rollshot-app` opens the native Save dialog after the layer-shell overlay has
   exited.
4. A successful save ends the capture successfully.
5. Cancelling the Save dialog ends the capture without writing a file.
6. A dialog or file-write failure prints the error and exits unsuccessfully.

The Save dialog must never coexist with the Linux layer-shell Result Review
workspace.

## Architecture

The overlay result must distinguish a normal completed capture from a completed
capture that requires app-owned Save As handling. The exact type names may
follow existing style, but the boundary must carry both the full-resolution
`CaptureResult` and the requested post-overlay action.

Representative shape:

```rust
pub enum OverlayOutcome {
    Completed(CaptureResult),
    SaveAs(CaptureResult),
    Cancelled,
}
```

On Linux, Result Review `Save` stores the final capture as a Save As handoff and
exits iced. It must not call `rfd` from `linux_runner`.

`rollshot-app` receives the handoff after `run_overlay` returns, opens the
native Save dialog, and writes the PNG when a destination is selected.

On macOS, Result Review `Save` continues using the existing in-workspace output
path.

## Error Handling

- Missing final image during Linux Save remains an in-workspace error and does
  not close the overlay.
- Save dialog cancellation returns success without writing a file.
- PNG write failure returns an application error after the overlay has closed.
- Clipboard failures retain the current Result Review behavior.

## Testing

- Linux Result Review Save requests a post-overlay Save As handoff and exits.
- Linux Result Review Save does not invoke the overlay output service.
- `rollshot-app` opens Save As for the handoff result.
- Save As cancellation ends without writing a file.
- Save As success writes a decodable PNG.
- Existing Copy, Close, and macOS output tests remain unchanged and passing.

## Manual Verification

- Linux: complete a normal screenshot, click Save, and confirm Result Review
  closes before the Save dialog appears.
- Linux: repeat with a scrolling screenshot.
- Linux: cancel Save As and confirm Rollshot exits without reopening Result
  Review.
- Linux: save the PNG and confirm the file contains the full-resolution result.
- macOS: confirm Result Review Save behavior is unchanged.
