# Region OCR to Clipboard Design

## Summary

Add a quick text-capture flow that lets a user select a screen region, runs
OCR on the captured pixels, writes the recognized text to the clipboard, and
prints the same text to standard output. The flow is available through a new
`rollshot-app ocr` command and, in OCR-enabled builds, through the daemon tray
and a second global shortcut on Linux and macOS.

The existing region-capture shortcuts remain unchanged:

- Linux capture: `Alt+Shift+6`; text capture: `Alt+Shift+7`.
- macOS capture: `Command+Shift+6`; text capture: `Command+Shift+7`.

## Goals

- Reuse the existing single-screenshot region-selection overlay.
- Skip the result workspace and copy recognized text immediately.
- Give CLI callers the recognized text on stdout as well as the clipboard.
- Expose the same operation from both daemon tray implementations and global
  shortcuts.
- Preserve the current off-by-default OCR build boundary.
- Never overwrite the clipboard when capture, OCR, or text production fails.

## Non-goals

- Fullscreen or scrolling OCR modes.
- OCR language selection or OCR engine configuration.
- Editing or reviewing recognized text before copying it.
- Changing the existing region capture behavior or shortcuts.
- Enabling OCR for every default workspace build.

## User Experience

### CLI

`rollshot-app ocr` starts a screenshot-region selection. It accepts the
capture backend and cursor-visibility options used by the underlying capture
path, but does not expose workflow or scope options: its workflow is always a
single region screenshot.

After a successful selection, Rollshot recognizes the selected image, orders
the matches for reading, writes the resulting UTF-8 text to the clipboard,
and writes the identical text plus a trailing newline to stdout.

Canceling with Esc is a normal cancellation: it produces no stdout, does not
touch the clipboard, and does not show an error. Empty recognition, OCR
initialization or detection failure, and clipboard failure produce a clear
stderr message and a nonzero exit status. The previous clipboard content is
preserved whenever Rollshot has no non-empty text ready to write.

### Daemon

OCR-enabled daemon builds add a `Capture Text` tray action. The action and the
platform text-capture shortcut launch the same `rollshot-app ocr` path as the
CLI. The daemon child requests graphical feedback: success produces a brief
`Text copied` notification; failure produces a concise error notification.
Cancellation produces no notification.

The daemon continues to allow at most one active capture child. A normal
capture and a text capture therefore cannot display overlapping overlays.

Builds without the `ocr` feature do not show `Capture Text` and do not
register the text-capture shortcut. Invoking the CLI command in such a build
returns an explicit `OCR is not available in this build` error.

## Architecture

### Launch and capture purpose

Add an OCR launch mode and CLI subcommand. Lower it to the existing
`screenshot + region` capture request plus an explicit post-capture purpose.
The normal purpose presents the result workspace; the OCR purpose sends the
in-memory `RgbaImage` directly to the quick-OCR application service. No
temporary image is written.

Both active platform capture paths must be checked and wired:

- Linux returns the selected `CaptureResult` from the iced layer-shell
  overlay before post-capture handling.
- macOS returns the selected `CaptureResult` through its iced
  ScreenCaptureKit product flow.

The platform-specific capture mechanisms remain unchanged; only the
post-capture destination differs.

### Shared OCR application service

Move or wrap the product OCR preparation, match ordering, and text assembly
currently used by the result workspace behind a UI-independent service. The
service accepts an `RgbaImage` and returns non-empty text or a typed product
OCR error. The result workspace and quick-OCR flow use the same ordering and
formatting implementation.

Text assembly preserves the existing product rules: items on the same visual
line are separated by one space, different lines by a newline, and the final
value is trimmed. An empty trimmed value is an error rather than clipboard
content.

Clipboard writing is a separate boundary with a replaceable test
implementation. Production uses `arboard`. The quick-OCR coordinator calls
the clipboard only after OCR has returned non-empty text, then returns that
same string to the CLI output layer.

OCR work runs off the UI/event-loop thread. OCR text must never be included in
tracing events. Privacy-safe diagnostics may include the operation stage,
typed error, image dimensions, and recognized item count.

### Daemon events and launch kinds

Add a `CaptureText` semantic event and an explicit launch kind shared by the
daemon core and process launcher. Do not represent the distinction as a
boolean or reconstruct it from argument strings.

The process launcher maps the existing region-capture kind to the current
capture arguments and the new text-capture kind to the OCR command plus an
internal graphical-feedback option. Child lifetime, process-group cleanup,
monotonic capture IDs, and stale-exit protection remain shared.

Linux portal shortcut registration and macOS `global-hotkey` registration
each manage two shortcut IDs and route them to distinct daemon events. Both
tray adapters add `Capture Text` next to `Capture Region` and retain `Quit`.
If text-shortcut registration fails, the tray action remains available and
the failure is logged without recognized content.

### Configuration

Extend daemon configuration with `capture_text_hotkey`. Platform defaults are
`Alt+Shift+7` on Linux and `Command+Shift+7` on macOS. Each daemon shortcut
field is optional at the deserialization boundary and independently falls
back to its platform default, so existing configuration files containing only
`capture_region_hotkey` continue to work without migration.

An invalid configured shortcut falls back only that action and produces a
clear warning. The existing capture shortcut remains unaffected by an invalid
text shortcut, and vice versa.

## Error Handling

- Cancellation is represented separately from failure and has no side effect.
- OCR-disabled, session initialization, detection, invalid region, empty
  result, and clipboard failure remain distinguishable typed failures until
  the CLI/feedback boundary renders a user-facing message.
- Clipboard contents are never cleared as a preparatory step.
- Stdout is reserved for successful OCR text. Diagnostics and errors use
  stderr or privacy-safe tracing.
- Graphical feedback is produced by the OCR child, avoiding a new daemon-child
  IPC protocol. Direct CLI use does not open graphical error UI.

## Testing

### Unit and contract tests

- CLI parsing accepts `ocr` and its supported capture flags and rejects or
  omits irrelevant workflow/scope combinations.
- Feature-disabled launch returns the documented error.
- OCR service tests cover reading order, line joining, trimming, empty output,
  session/detection failure, and privacy-safe error rendering.
- Quick-OCR coordinator tests prove that clipboard and returned stdout text
  are identical on success and that the clipboard is not called on capture,
  OCR, or empty-result failure.
- Clipboard failure returns failure without emitting successful stdout.
- Daemon configuration tests cover both platform defaults, legacy config,
  independent overrides, and independent invalid-value fallback.
- Daemon core tests route both semantic events to the correct launch kind,
  enforce one active child across both kinds, and preserve existing exit and
  cleanup behavior.
- Process-launcher tests verify normal capture arguments and OCR arguments.
- Linux and macOS shortcut and tray tests verify both event mappings and menu
  actions.
- Diagnostics tests ensure recognized text is absent from errors and logs.

### Verification

Run the relevant OCR-feature tests as well as the ordinary workspace tests,
format check, and clippy. Verify that a build without the OCR feature still
builds and tests without pulling the OCR runtime into default members.

Manually verify on Linux and macOS that the existing `...+6` shortcut is
unchanged and that the new `...+7` shortcut and `Capture Text` tray item select
a region, copy the same text emitted by the CLI path, handle Esc silently, and
preserve existing clipboard content on failure.

This change does not touch `rollshot-core` stitching paths, so stitching
benchmarks are not required.
