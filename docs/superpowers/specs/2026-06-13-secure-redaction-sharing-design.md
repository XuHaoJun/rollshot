# Secure Redaction Sharing Design

**Status:** Approved design  
**Date:** 2026-06-13  
**Scope:** Result Workspace secure-sharing contract for existing opaque redactions  
**Revision (2026-06-13):** User-facing tool renamed `Secure Redact` → `Redact`;
all user-facing copy uses "Safe" (never "Secure"); the retained-original notice
changed from a once-per-session message to a persistent derived disclosure.
"Secure redaction" is retained only as an internal state concept.

## Product Thesis

Rollshot already replaces pixels covered by an `OpaqueRedaction` when it
flattens an image. This feature turns that rendering behavior into a clear,
enforced product contract:

> When a Result Workspace contains a secure redaction, Rollshot's normal Copy
> and Save As actions produce a flattened image that does not contain the
> covered original pixels.

The first release guarantees safe sharing. It does not guarantee secure
deletion. Rollshot continues to retain the unredacted in-memory source and any
original auto-saved file, and it tells the user when an action exposes that
original.

## Goals

- Make secure redaction the clear purpose of the existing opaque black
  redaction tool.
- Make normal Copy and Save As outputs safe by construction whenever a
  redaction exists.
- Clearly distinguish safe flattened outputs from the retained unredacted
  original.
- Prevent a safe export from accidentally overwriting the unredacted original.
- Require explicit confirmation every time the user copies or reveals the
  unredacted original.
- Keep Linux and macOS Result Workspace behavior consistent.

## Non-Goals

- Secure deletion or proof that no unredacted pixels remain on the device.
- Deleting or overwriting the original auto-save.
- Blur, pixelate, white blocks, custom redaction colors, or other redaction
  styles.
- OCR, text-line detection, or automatic detection of email addresses, tokens,
  phone numbers, URLs, IP addresses, or other sensitive information.
- Changing capture completion, auto-save, Linux workspace launch, or the macOS
  thumbnail flow.
- Adding a user-selectable secure mode.

## Existing Foundation

The implementation keeps the existing image-document model:

- `Annotation::OpaqueRedaction` stores a bounded image-space rectangle.
- The annotation renders as the shared full-alpha black `REDACTION_FILL`.
- Flattening clones the full-resolution source and replaces covered pixels.
- The immutable source remains available for undoable, non-destructive editing.
- Normal Copy already flattens the document.
- Save As already flattens when annotations exist.
- `ResultDocument::source_path` identifies the original auto-save, while
  `last_export_path` identifies the latest successful Save As output.

This design strengthens routing, naming, confirmation, and overwrite
protection. It does not replace the annotation or rasterization architecture.

## State Model

The Result Workspace derives whether it has secure redactions directly from
the current annotation graph:

```text
has_secure_redactions =
    any annotation is Annotation::OpaqueRedaction
```

This is derived state and is not stored separately. Adding, deleting, undoing,
or redoing a redaction immediately changes the workspace's secure-redaction
state.

The retained-original disclosure is likewise derived state, not a stored flag:
it is shown whenever `has_secure_redactions` is true and `source_path` exists.
It does not affect document dirty state or undo history.

The workspace also supports a pending unredacted-action confirmation with one
of two purposes:

- copy the unredacted original;
- reveal the unredacted original.

No confirmation can be permanently disabled.

## User Experience

### Redaction Tool

The toolbar names the existing tool `Redact`. Its shortcut remains `R`. The
tool continues to create solid black opaque rectangles with the existing move,
resize, delete, undo, and redo behavior.

User-facing copy never uses the word "Secure". The product promise is
communicated as "Safe" exports plus an honest disclosure that the original is
retained. ("Secure redaction" remains an internal state concept only; see
State Model.)

While at least one redaction exists and `source_path` exists, Rollshot shows a
persistent, low-key inline disclosure near the primary output actions:

`Unredacted original remains saved. Safe exports are flattened.`

The disclosure is derived from current state, not a one-time notice: it appears
whenever a redaction exists alongside a retained original, and disappears when
either condition no longer holds. When `source_path` does not exist there is no
retained original to disclose, so no disclosure is shown.

### Contextual Primary Actions

When no secure redaction exists, the toolbar keeps the general labels:

- `Copy`
- `Save As`
- `Reveal`

When at least one secure redaction exists, the primary output actions become:

- `Copy Safe Image`
- `Save Safe Image As`

The workspace does not show a persistent banner claiming the result is safe;
the contextual action labels carry that guarantee at the decision point. It
does show the retained-original disclosure described above when an unredacted
original exists, because that caveat is the one fact the "Safe" labels do not
convey on their own.

### Safe Copy

`Copy Safe Image` copies the full-resolution flattened document. On success it
shows:

`Copied safe flattened image`

The action does not clear dirty state.

### Safe Save As

`Save Safe Image As` writes the full-resolution flattened document.

The default filename adds `-redacted` immediately before the extension. For
example:

```text
Rollshot 2026-06-13.png
Rollshot 2026-06-13-redacted.png
```

If the selected destination equals the current `source_path`, Rollshot refuses
the write and shows a persistent error:

`Safe export cannot overwrite the unredacted original. Choose another location.`

The rejected operation does not change the source file, annotations, dirty
state, saved-state marker, or `last_export_path`. The user may reopen the save
dialog and choose another destination.

After a successful safe save, Rollshot updates `last_export_path` and the
saved-state marker and shows:

`Saved safe flattened image`

### Unredacted Original Actions

When a secure redaction exists, the copy-menu action is named:

`Copy Unredacted Original…`

Selecting it opens a confirmation that explicitly states that the action will
copy content hidden by redactions. Confirming copies the immutable source.
Cancelling has no side effect. Every attempt requires a new confirmation.

When a secure redaction exists, no safe export exists, and `source_path`
exists, the reveal action is named:

`Reveal Unredacted Original…`

Selecting it opens a confirmation that explicitly states that the revealed
file contains content hidden by redactions. Confirming reveals `source_path`.
Cancelling has no side effect. Every attempt requires a new confirmation.

### Reveal Routing

Reveal labels and destinations follow this priority:

1. When `last_export_path` exists and the document contains secure redactions,
   show `Reveal Last Safe Export` and reveal `last_export_path`.
2. When no safe export exists, secure redactions exist, and `source_path`
   exists, show `Reveal Unredacted Original…` and require confirmation before
   revealing `source_path`.
3. When no secure redaction exists, preserve the existing `Reveal` behavior.
4. When no durable path exists, Reveal remains disabled.

`Reveal Last Safe Export` intentionally refers to the last file written to
disk. It does not imply that the file includes subsequent unsaved edits.

If all secure redactions are deleted or undone, the toolbar immediately returns
to the general labels and behavior.

## Data Flow And Safety Rules

### Derived Secure State

Result Workspace code owns the product-level secure-sharing policy. It queries
the image document's current annotations to determine whether secure
redactions exist. The image-document crate remains responsible for annotation
semantics and flattened rendering.

### Copy Routing

Normal Copy continues to use the flattened full-resolution document. The
payload behavior is unchanged; secure-redaction state controls the user-facing
label and success message.

The unredacted copy route continues to clone the immutable source, but it is
reachable only after the explicit unredacted-action confirmation when secure
redactions exist.

### Save Routing

When secure redactions exist, Save As always uses the flattened
full-resolution document. Destination validation runs before dispatching the
write. A destination that identifies `source_path` is rejected.

Path equality uses the strongest practical local comparison available before
writing. The minimum required guarantee is rejection when the selected path is
the same path value as `source_path`; implementation planning must also assess
normalization and alias cases without claiming protection the platform cannot
reliably provide.

When no secure redaction exists, existing Save As payload behavior remains
unchanged.

### Safe Output Boundary

The secure-sharing guarantee covers images Rollshot places on the clipboard
through the normal Copy action and images Rollshot writes through Safe Save
As. These outputs contain only flattened RGBA pixels encoded as PNG. They must
not include annotation layers, original image bytes, or metadata that embeds
the source.

The guarantee does not cover:

- `Copy Unredacted Original…`;
- `Reveal Unredacted Original…`;
- the original auto-save;
- copies, backups, snapshots, or caches managed outside Rollshot;
- content outside the redaction rectangles.

## Confirmation And Error Behavior

The existing discard confirmation remains separate from unredacted-action
confirmation. Each confirmation has a single explicit purpose, and confirming
one cannot trigger another.

Unredacted-action confirmations are blocking dialogs. Their copy must name the
dangerous action and state that hidden content will be exposed. They offer only
cancel and the explicit action; they do not offer a persistent bypass.

Safe-export overwrite rejection is a persistent inline error because the user
must choose a new destination. Existing asynchronous save failures continue to
use the current error path.

The retained-original disclosure is informational and non-blocking. It is
persistent derived state rather than an expiring notice: it remains visible
while a redaction and a retained original coexist, and is not dismissed by
adding, undoing, or re-adding redactions.

## Platform Behavior

Linux and macOS use the shared Result Workspace for this feature, so labels,
confirmations, routing, and output guarantees must match.

Capture completion remains unchanged:

- Linux continues to auto-save the unredacted result before opening the
  workspace.
- macOS continues to auto-save the unredacted result and show the thumbnail
  before the workspace.
- Auto-save failure continues to open an unsaved workspace.

The feature must not imply that the existing auto-save is redacted or deleted.

## Testing And Acceptance

### Image Document

- Flattening a secure redaction replaces every covered pixel with
  `[0, 0, 0, 255]`.
- Flattening leaves the immutable source pixels unchanged.
- Existing redaction geometry, resize, move, hit-test, undo, and redo tests
  continue to pass.

### Workspace State And Labels

- Adding, deleting, undoing, and redoing redactions updates derived secure
  state immediately.
- Toolbar, copy-menu, and reveal labels match the routing rules.
- Removing all redactions restores general labels and behavior.
- The retained-original disclosure is visible whenever a redaction and a
  retained `source_path` coexist, and is absent when either is missing
  (including unsaved captures with no original).
- No user-facing label, message, or confirmation copy contains the word
  "Secure".

### Copy And Save

- Safe Copy uses the full-resolution flattened payload and shows the approved
  success message.
- Safe Save As uses the full-resolution flattened payload and shows the
  approved success message.
- The default safe filename inserts `-redacted` before the extension.
- Selecting `source_path` as a safe-export destination performs no write and
  preserves annotations, dirty state, saved-state marker, and
  `last_export_path`.
- Successful safe save updates `last_export_path` and the saved-state marker.
- A produced safe PNG contains no metadata, layers, or original bytes from
  which covered pixels can be restored.

### Unredacted Actions

- Copying the unredacted original with secure redactions requires confirmation.
- Revealing the unredacted original with secure redactions requires
  confirmation.
- Cancelling either confirmation has no side effect.
- Completing or cancelling a confirmation does not suppress later
  confirmations.
- After a safe export, Reveal routes to `last_export_path` and is labeled
  `Reveal Last Safe Export`.

### Platform Acceptance

- Run the Result Workspace secure-redaction flow once on Linux.
- Run the Result Workspace secure-redaction flow once on macOS.
- Confirm Linux auto-save launch and macOS auto-save thumbnail behavior remain
  unchanged.

## Success Criteria

The feature is complete when a user can add a secure redaction, understand
that the original remains available, copy or save a clearly identified
flattened safe image, and cannot accidentally copy, reveal, or overwrite the
unredacted original without an explicit warning or refusal.
