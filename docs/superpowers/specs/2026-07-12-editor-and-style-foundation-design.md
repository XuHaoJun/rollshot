# Editor And Style Foundation Design

**Date:** 2026-07-12

**Status:** Draft — pending written-spec review

**Slice:** 1 — Editor And Style Foundation

**Umbrella:**
[`2026-07-12-annotation-editor-umbrella-design.md`](2026-07-12-annotation-editor-umbrella-design.md)

## 1. Purpose

This slice establishes the annotation editor's shared style, property-editing,
default-persistence, and responsive-toolbar foundations. It improves Number
Callout and Text Note styling while preserving Opaque Redaction's safety
contract. Later tool-family slices extend these foundations instead of adding
parallel property, default, history, or toolbar systems.

This design is subordinate to the approved umbrella. The umbrella's scope,
invariants, ownership boundaries, and cross-slice acceptance criteria remain
binding.

## 2. Scope

This slice delivers:

- Typed styles for Number Callout and Text Note.
- Canonical constructors and migrated compatibility consumers.
- Typed property edits with stable IDs and one-entry undo semantics.
- A document-local, editable next-number sequence.
- Per-tool next-object defaults persisted across sessions.
- Selected-object property editing.
- A compact common palette and custom color picker.
- The approved two-row responsive Result Workspace toolbar.
- Responsive More routing for existing secondary actions.
- Safety isolation for Opaque Redaction.

This slice does not add Line, Arrow, Rectangle, Ellipse, Pen, Highlighter, or
Pixelate. It adds no placeholder toolbar buttons or speculative model fields
for those tools. It does not change Capture Overlay, Action Guide annotation
UX, automation proposal UX, or stitching.

## 3. Decisions

### 3.1 Typed styles and typed edits

The document uses explicit `NumberStyle` and `TextStyle` values and explicit
style edit operations. It does not expose a universal property bag, generic
property patch, or unrestricted whole-annotation replacement operation.

The exact Rust names belong to the implementation plan, but the public model
must express:

```rust
struct NumberStyle {
    accent: Rgba8,
    size: NumberSize,
}

enum NumberSize {
    Small,
    Medium,
    Large,
}

struct TextStyle {
    font_size: TextSize,
    text_color: Rgba8,
    background: Option<Rgb8>,
}
```

`TextSize` is a bounded stepped numeric value with 14, 18, 24, and 32
image-pixel choices; 18 pixels remains canonical. Number Small, Medium, and
Large map to 0.75, 1.0, and 1.3 times the current reviewed bubble, outline,
label, and leader geometry; Medium therefore preserves today's appearance.
Text background color stores RGB only; enabled backgrounds render with fixed
85 percent alpha. There is no background-opacity control in this slice.

Opaque Redaction remains style-free. No public style or edit API can change its
black, fully opaque source-pixel replacement behavior.

### 3.2 Canonical constructors

`rollshot-image-document` owns canonical defaults and constructors for Number
Callout, Text Note, and Opaque Redaction. Constructors validate completed
values and are the only source of reviewed default styles.

Existing Result Workspace, timeline/workbench, automation proposal lowering,
and tests migrate away from direct struct literals where a canonical
constructor applies. Consumers may supply explicit reviewed styles when their
contract requires it, but they do not duplicate default constants.

No persisted annotation-graph migration is required because Result Workspace
documents remain session-only.

### 3.3 Document-local number sequence

Each new document starts with next number 1. The user may set the next number
for the active document. The value is document history, not an app default, and
is not persisted across captures.

Number creation, deletion, compact renumbering, explicit next-number changes,
undo, and redo preserve the current sequence contract. Undo restores exact
prior numbers and the exact prior next-number value. A successful edit after
undo clears redo history.

## 4. Document Editing And History

The document exposes typed completed operations for:

- Adding Number, Text, and Opaque Redaction annotations.
- Updating Number geometry and style.
- Updating Text geometry, content, and style.
- Updating Opaque Redaction geometry.
- Setting the document-local next number.
- Deleting an annotation.

A successful style edit retains the annotation ID and adds exactly one history
entry. Text content editing remains one entry per completed inline-edit
session, not per keystroke.

The document independently rejects:

- Non-finite geometry or size values.
- Unsupported or out-of-range Text sizes.
- Invalid colors.
- Zero or invalid next-number values.
- Operations whose annotation ID or variant does not match the requested edit.

Rejection is atomic: annotation graph, number sequence, history, redo state,
document state ID, and dirty state remain unchanged.

## 5. App State And Defaults

### 5.1 Editor defaults

`ResultWorkspace` owns a typed editor-default model. Slice 1 contains Number
and Text defaults; later slices extend the same model.

Creating an annotation copies the active tool's current defaults into the new
annotation. Editing a selected annotation never changes a tool default.

Defaults are stored under an `annotation_defaults` section in Rollshot's
existing `config.toml`. Loading and saving must:

- Preserve unrelated configuration sections and unknown fields.
- Resolve a missing section or field to the document crate's canonical
  default.
- Reject malformed values without blocking the editor.
- Keep successfully changed in-memory defaults when persistence fails.
- Show at most one non-blocking persistence warning per workspace session.

The implementation plan must define an atomic-enough write strategy consistent
with existing configuration handling and tests; this design does not introduce
a second annotation settings file.

### 5.2 Property contexts

Contextual controls follow these rules:

- A creation tool edits its next-object defaults.
- Select with a selected annotation edits only that annotation.
- Select without a selection shows no property cluster.
- Opaque Redaction shows no style properties.
- A creation tool never selects an existing object under the pointer.

Number properties are accent color, Small/Medium/Large size, and document-local
next number. Text properties are stepped font size, text color, background
on/off, and background color.

### 5.3 Property transactions

Discrete controls commit immediately and produce one default update or one
document history entry.

The custom color picker is an explicit transaction:

1. Opening captures the original value.
2. Palette, saturation/value, hue, or hex changes update an app-only preview.
3. Apply commits one default update or one typed document edit.
4. Cancel, outside close, tool change, or relevant `Esc` restores the original
   preview value and creates no history entry.

The picker includes the common palette, a saturation/value field, hue control,
hex input, current-color preview, Apply, and Cancel. It does not expose alpha.

Drafts, previews, open menus, selection, and active tool remain app-only and
never enter flattened output or document history. Dirty state and Navigator
refresh only after a successful document commit.

## 6. Toolbar And Responsive Behavior

The Result Workspace uses two rows built from iced 0.14 standard widgets.
`responsive` selects width-aware composition. Standard rows, buttons,
tooltips, inputs, and sliders implement controls. More and the color picker use
built-in floating or stacked composition; this slice does not require a custom
widget or custom `iced::advanced::overlay::Overlay`.

### 6.1 First row

The approximately 40-pixel first row contains:

```text
Close | title                         Undo Redo | Copy dropdown | Save As
```

Close and title remain left aligned. Undo/Redo are visually separated from
output actions. Copy and Save As remain pinned right at every supported width
and never enter overflow. Long titles truncate before displacing pinned
actions.

### 6.2 Second row

The approximately 36–40-pixel second row contains implemented annotation tools,
then contextual properties, with More at the trailing edge.

Slice 1 exposes Select, Number, Text, and Redact. It does not expose future-tool
placeholders. Under width pressure, Redact may enter More after preserving
Select, Number, and Text. Property controls use their compact representations
before implemented tools move to More.

Existing low-frequency actions move to More:

- Smart Redaction.
- Navigator.
- Reveal.
- Export Bug Report.
- Feature-gated OCR Text.

Moving these actions does not change their behavior, availability, safety
prompts, feature gates, or workbench transitions.

Later slices extend the same routing policy using the umbrella priority order.
If an active creation tool is inside More, More uses active styling and shows
the active tool name. More entries retain tooltip and shortcut information.

## 7. Interaction Routing

Select remains the default tool. Creation tools remain active after successful
creation. Delete and Backspace delete the selected annotation when no more
local text/property interaction owns the key.

`Esc` resolves the most local state first:

1. Cancel an open property transaction, including the color picker.
2. Cancel an active annotation or inline-text draft.
3. Clear selection.
4. Switch an active creation tool to Select.
5. Apply existing workspace close and dirty-state behavior.

Tool changes close transient menus and cancel uncommitted property previews.
Undo or redo first cancels an active property preview, then operates on
committed document history. Keyboard routing must not let a toolbar shortcut
steal input from the inline text editor or a focused property input.

## 8. Rendering And Output

The existing framework-neutral render boundary expands only as needed to carry
Number and Text style values. Live iced rendering and full-resolution flatten
consume the same style, geometry, text-layout, and compositing semantics.

Number size presets map deterministically to bubble, outline, label, and leader
geometry in image-space pixels. Text size maps deterministically to text
layout, plate padding, hit bounds, and Navigator anchor behavior. Enabled Text
backgrounds use their selected RGB color at fixed 85 percent alpha.

Draft property previews may render through app-only commands, but selection,
hover, handles, drafts, and uncommitted styles never appear in Copy or Save.
Copy Original continues to use the immutable source. Opaque Redaction flattening
remains fully opaque.

## 9. Error Handling

- A rejected document edit leaves document, history, selection, and dirty state
  unchanged and shows a non-blocking inline error.
- Invalid hex or numeric input stays within the open transaction and cannot be
  applied until valid.
- A default-load failure uses canonical defaults and reports one warning.
- A default-save failure retains in-memory defaults and reports one warning.
- Closing a picker or menu cannot commit a partially valid value.
- Existing Copy, Save, clipboard, and file-dialog failures retain their current
  non-destructive behavior.

## 10. Verification

### 10.1 Automated verification

Tests cover:

- Style equality, canonical defaults, constructors, and invalid values.
- Typed create, property, geometry, text, sequence, and delete operations.
- Stable annotation IDs and one-entry undo/redo behavior.
- Exact number-sequence restoration and redo clearing.
- Compatibility callers in Result Workspace, timeline/workbench, automation
  lowering, and their tests.
- Default loading, missing-field fallback, malformed values, unrelated-config
  preservation, and one-warning persistence failures.
- Creation defaults versus selected-object properties.
- Color preview, Apply, Cancel, outside close, tool change, and `Esc`.
- Wide and narrow toolbar routing, More active state, tooltips, shortcuts, and
  focused-input keyboard precedence.
- Live render-command and flattened-output consistency for every Number size
  and representative Text styles.
- Opaque Redaction remaining fully opaque and inaccessible to style edits.
- Full-resolution image coordinates under zoomed and downscaled long-image
  display.

The normal Rust verification is:

```text
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

No `rollshot-core` stitching path changes are planned, so stitching benchmarks
are not required.

### 10.2 Runtime verification

Linux and macOS runtime checks cover:

- Wide and narrow two-row layout and More routing.
- Active tool visibility, tooltips, and shortcuts.
- Number and Text creation with changed defaults.
- Selected-object property editing and one-step undo/redo.
- Custom-color preview, Apply, and Cancel.
- Inline Text editing and keyboard focus.
- Copy, Copy Original, Save As, dirty state, and Navigator refresh.
- Zoom, pan, and long-image behavior.
- Opaque Redaction safety and output opacity.

Both platforms use the shared Result Workspace path. If one platform cannot be
run in the implementation environment, the slice cannot be marked Complete;
the umbrella registry must record Handoff or Blocked with the unchecked runtime
risk.

## 11. Completion Criteria

Slice 1 is complete only when:

1. Number and Text styles are explicit, validated, editable, and rendered
   consistently live and flattened.
2. Opaque Redaction remains style-free and fully opaque.
3. Existing consumers use canonical construction without behavior regressions.
4. Defaults persist safely in `config.toml` without overwriting unrelated data.
5. Completed property interactions create at most one history entry.
6. The approved responsive two-row toolbar and More behavior pass automated
   and Linux/macOS runtime verification.
7. All required automated checks pass and no required Slice 1 work remains.

Completion unlocks brainstorming for Slice 2. It does not authorize Slice 2
implementation without its own approved specification and plan.
