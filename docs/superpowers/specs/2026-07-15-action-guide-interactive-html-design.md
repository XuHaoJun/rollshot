# Action Guide Interactive HTML Design

**Date:** 2026-07-15  
**Status:** Approved design  
**Branch:** `feat/interactive-html-guide`  
**Scope:** Add a deterministic offline `index.html` reader to every exported Action Guide folder

## Purpose

Rollshot already exports reviewed Action Guides as Markdown, JSON metadata, and
PNG keyframes, with separate GIF, Storyboard, and MP4 outputs. This design adds
an offline interactive reader to the existing folder format so a recipient can
open the reviewed Guide, control the reading pace, jump between steps, inspect
explanatory annotations, search, zoom, and copy step text without installing
Rollshot or starting a local server.

The reader is a general Action Guide container. It does not introduce
bug-report-, onboarding-, release-, or visual-regression-specific modes.

## Product Decisions

- Preserve the existing multi-file Action Guide folder as the primary export.
- Add `index.html` as a required part of every Action Guide folder, including
  standalone exports and Action Guides nested inside Issue Packs.
- Keep `steps.md`, `session.json`, and `keyframes/*.png` as first-class sibling
  artifacts.
- Add an editable Guide-level title. New exports include it in Markdown,
  manifest data, and the HTML reader.
- Generate HTML deterministically from reviewed state. Export never invokes an
  LLM or OCR.
- Inline the viewer HTML, CSS, JavaScript, and safe viewer-data snapshot into
  `index.html`; reference PNG keyframes by relative path.
- Do not use `fetch()`, a local server, remote assets, or network requests.
- Use a desktop layout with a left step list, a large central keyframe, and
  anchored explanatory popovers.
- Make only annotations with non-empty explanatory text interactive. Purely
  visual annotations remain visual.
- Permanently flatten redactions into exported pixels. The HTML must never
  contain or reference the obscured original pixels.
- Search Guide title, step titles, captions, and interactive annotation text.
  OCR search is deferred.
- Follow the operating-system color scheme. A manual theme switch is deferred.
- A standalone export never overwrites an earlier export. It creates a unique
  title-and-time-based folder.
- A successful standalone export leaves the Timeline Workspace open and offers
  `Open Guide` and `Show in Folder` actions.

## Current Context

The current `rollshot-action` exporter atomically builds:

```text
action-guide/
  steps.md
  session.json
  keyframes/
    001.png
    002.png
```

`GuideStep` already owns reviewed title, caption, semantic metadata, keyframe,
and source identity. Guide-level title does not yet exist.

Committed per-step annotation documents currently live in the Timeline
Workspace presentation state rather than in `Guide`. The existing Storyboard
snapshot path establishes the relevant safety pattern: use
`ImageDocument::flatten()` for a matching committed annotation document and use
the reviewed retained keyframe otherwise.

OCR snippets are not currently persisted in the Action Guide export contract.

## User Experience

### Editing and export

The Timeline Workspace provides an editable Guide title. Trimming an empty
title at export time yields the stable fallback `Action Guide`.

For a standalone export, the user chooses a parent directory. Rollshot creates:

```text
<safe-guide-title>-<YYYY-MM-DD-HHMMSS>/
  index.html
  steps.md
  session.json
  keyframes/
    001.png
    002.png
```

If the generated name already exists, Rollshot appends a deterministic numeric
suffix. It never replaces an existing export.

After success, the Timeline Workspace remains open and displays clear success
feedback with:

- `Open Guide`
- `Show in Folder`

Cancelling the directory picker changes no state and creates no files.

An Issue Pack writes the same required files under its existing
`action-guide/` directory. The Issue Pack continues to own its outer temporary
directory and atomic commit lifecycle.

### Initial reader state

Opening `index.html` displays the first step immediately. It does not show a
cover page, format explanation, or empty welcome screen.

The desktop layout contains:

- Left: Guide title, search field, progress, and step list.
- Center: the current reviewed keyframe at the largest practical size.
- Bottom controls: step title, caption, Copy Step Text, previous/next, and zoom.

The keyframe is the dominant content. Controls and metadata must not compete
with it visually.

On narrow screens, the step list becomes a drawer. Explanatory annotation text
moves below the image rather than covering a large portion of the keyframe.

### Step navigation

Selecting a step in the list or using previous/next replaces the current image,
title, caption, progress, and hotspots as one state transition. Switching steps
closes any open annotation popover.

The reader supports:

- `Left Arrow` / `Right Arrow`: previous/next step.
- `+` / `-` / `0`: zoom in, zoom out, reset zoom.
- `/`: focus search.
- `Escape`: close the open popover; otherwise clear search.

Global shortcuts do not run while the user is typing or operating a control
that owns the key.

### Annotation interaction

All committed annotations appear in the flattened keyframe pixels.

An annotation receives a hotspot only when it carries non-empty explanatory
text and is an explanatory callout or text annotation. Lines, arrows,
rectangles, highlights, and other visual-only shapes do not create empty or
invented interactions.

A hotspot is a keyboard-focusable semantic button aligned to the annotation's
exported image-space target. Selecting it opens one anchored popover containing
the annotation explanation. Selecting another hotspot replaces the open
popover. Selecting empty image space, pressing `Escape`, or changing steps
closes it.

Zoom and responsive layout must preserve hotspot alignment. Reduced-motion mode
removes non-essential popover and step-transition animation.

### Search

Search is case-insensitive over:

- Guide title
- Step title
- Step caption
- Interactive annotation explanation text

Results identify matching steps. Selecting a result navigates to the step and
highlights the matching visible text. A match in annotation text also selects
the corresponding hotspot so its explanation is visible.

A Guide-title match appears once as a Guide-level result. Selecting it returns
to the first step and highlights the title in the reader header; it does not
manufacture one duplicate result per step.

Search never inspects PNG pixels and does not run OCR.

### Copy Step Text

Copy Step Text copies the current step's visible textual content: step number,
title, non-empty caption, and interactive annotation explanations in the
annotation document's stable presentation order.

Success changes the control to an explicit temporary `Copied` state. If the
Clipboard API is unavailable or rejected under `file://`, the reader selects
the same text for manual copying and displays an honest fallback message. It
must not report success when no copy occurred.

### Theme and accessibility

The reader follows `prefers-color-scheme` and provides no manual theme
selector. It follows `prefers-reduced-motion`.

The document uses semantic landmarks, controls, headings, labels, and visible
focus states. The step list, viewer controls, search results, and hotspots are
fully keyboard operable in a logical focus order. A skip link moves focus to
the current step content.

When JavaScript is disabled, a `<noscript>` fallback directs the reader to
`steps.md`.

## Output Approach

The selected approach is one generated HTML entry point plus existing image
assets:

```text
index.html          HTML + CSS + JavaScript + encoded viewer snapshot
steps.md            human-readable portable fallback
session.json        machine-readable export metadata
keyframes/*.png     reviewed flattened step images
```

This deliberately duplicates small textual metadata between `session.json`
and `index.html`. It avoids local-file `fetch()` restrictions and reduces the
number of files that must remain together. The reviewed in-memory snapshot is
the source for all outputs; neither serialized artifact is used to generate the
other.

## Components and Responsibilities

### Reviewed Guide snapshot

Export begins by constructing one immutable owned snapshot of reviewed state:

```text
ReviewedGuideSnapshot
  guide title
  capture and semantic metadata
  steps[]
    index, title, caption, event metadata
    final flattened image
    relative keyframe path
    interactive hotspots[]
      normalized image position and hit area
      explanation text
```

The snapshot includes only committed current state. It excludes:

- Original pixels hidden by redaction
- Annotation history and undo/redo state
- Pending or rejected LLM suggestions
- Provider, model, prompt, and provenance data
- Raw semantic input payloads
- OCR data

For a step with a matching committed annotation document, snapshotting uses the
document's flattened result. Otherwise it clones the retained reviewed
keyframe. This follows the existing Storyboard snapshot model.

### Guide folder renderer

One deterministic renderer consumes the snapshot and writes PNG, Markdown,
manifest, and HTML outputs. All representations therefore share title, order,
text, metadata, and final reviewed pixels.

The renderer:

- Does not access mutable UI state.
- Does not invoke LLM, OCR, network, clipboard, or browser APIs.
- Treats `index.html` as required, not as a best-effort optional artifact.
- Escapes all user- and model-originated text as data rather than raw markup.
- Produces deterministic content apart from explicitly supplied export-time
  metadata and unique destination naming.

The subsequent engineering review must choose concrete crate placement, Rust
API signatures, and ownership types without changing the fixed architectural
boundary: snapshot reviewed state once, then render every artifact from that
snapshot.

### HTML viewer

The generated reader contains inline CSS and JavaScript plus safely encoded
viewer data. It loads only relative keyframe paths. It does not use:

- `fetch()`
- Remote fonts, scripts, styles, or images
- CDN resources
- Service workers
- Analytics or telemetry
- Local storage

Viewer state exists only for the current browser session.

## Data Safety

Text serialization must prevent `</script>` and equivalent payloads from
escaping the embedded data context. Viewer rendering uses text nodes or
equivalent escaped APIs; Guide content is never assigned as trusted HTML.

Every exported keyframe is the final reviewed flattened image. Redaction is a
pixel-level export operation, not an HTML overlay. Removing CSS, JavaScript, or
DOM elements must not reveal hidden content.

Hotspot metadata contains only geometry required for hit-testing and the
reviewed explanation text. It does not contain annotation history or a second
copy of hidden pixels.

Runtime diagnostics may include non-sensitive structural fields such as step
count, result category, or destination class. They must not include image
pixels, paths containing Guide titles, title/caption/annotation content,
clipboard payloads, provider data, or raw input events.

## Export Lifecycle and Error Handling

Before filesystem writes, snapshot validation requires:

- At least one Guide step.
- A usable title after fallback.
- One retained reviewed keyframe per step.
- Non-empty explanation text for every hotspot.
- Finite hotspot geometry whose hit area intersects its image.

Standalone export writes to a uniquely named temporary sibling directory. It
writes every required artifact and only then renames the directory into place.
Any snapshot, encode, template, write, or rename failure removes the temporary
directory, preserves the editable Timeline Workspace, and reports a recoverable
error. No final or temporary partial Guide remains.

Issue Pack export uses the same required Guide renderer inside the existing
outer Issue Pack transaction. A failure to generate `index.html` fails and
rolls back the Guide/Issue Pack export rather than shipping a folder that only
appears complete.

If an exported folder is later damaged:

- A missing or undecodable PNG shows `Image unavailable` for that step.
- Other steps and textual content remain usable.
- Failed copy falls back to selected text.
- JavaScript-disabled readers are directed to `steps.md`.

## Performance and Resource Bounds

- Initial reader load decodes only the current keyframe and may preload the
  immediately adjacent keyframes. It does not decode the full Guide.
- Search operates only on embedded textual metadata.
- Zoom uses browser presentation transforms and does not allocate a new bitmap.
- Step changes show a short explicit loading state while the next image decodes
  instead of presenting unexplained blank content.
- The exporter owns one reviewed flattened image per step in its immutable
  snapshot. The engineering review must confirm peak-memory bounds and whether
  the current Storyboard snapshot ownership pattern is acceptable for long
  Guides.

## Testing

### Snapshot and renderer tests

- Guide title editing, trimming, empty fallback, and safe folder slug.
- Step order and title/caption/semantic metadata match across Markdown,
  manifest, and HTML data.
- Annotated steps use flattened reviewed images; unannotated steps use retained
  reviewed keyframes.
- Exported redacted pixels do not equal the obscured source pixels.
- Only eligible non-empty explanatory annotations become hotspots.
- Hotspot geometry remains aligned across image dimensions and zoom transforms.
- Adversarial title, caption, and annotation strings cannot escape embedded
  data or inject HTML.
- Missing keyframes, invalid hotspots, write errors, and rename errors roll back
  without final or temporary residue.
- Same-title same-second standalone exports receive numeric suffixes and retain
  both outputs.
- Standalone exports and Issue Pack Action Guides both contain `index.html`.

### Browser tests under `file://`

- The first step renders without a server and produces no network request.
- Step list, previous/next controls, and keyboard shortcuts stay synchronized.
- Search finds and highlights title, caption, and annotation matches.
- Annotation hotspots open, replace, and close anchored popovers.
- Zoom and responsive layout preserve hotspot alignment.
- Copy reports real success and provides a manual fallback on rejection.
- Missing PNG produces a recoverable step-local state.
- Dark/light, reduced-motion, semantic focus order, skip link, and narrow-screen
  behavior work as specified.
- Initial load does not decode all keyframes.

### Runtime verification

- Export one reviewed Action Guide through the active Linux product path.
- Export one reviewed Action Guide through the active macOS product path.
- Confirm success leaves the Timeline Workspace open and both post-export
  actions work.
- Open standalone output through `file://` in Chrome and Firefox.
- Open standalone output through `file://` in Safari on macOS.
- Move the complete folder and confirm relative assets still resolve.
- Export an Issue Pack, then open its `action-guide/index.html` after moving or
  extracting the complete pack.

## Success Criteria

A recipient without network access, a local server, an LLM, Rollshot, or any
other runtime can double-click `index.html` and fully read the reviewed Guide.
The reader supports navigation, eligible annotation explanations, search,
zoom, keyboard operation, and honest copy feedback. Every artifact reflects one
reviewed snapshot. Export never silently replaces earlier output, leaves a
half-built folder, or exposes pixels hidden by a redaction.

## Non-Goals

- Single-file HTML with embedded PNG payloads
- OCR search
- Manual theme selection
- Editing an exported Guide
- LLM or OCR work during export
- Hosting, publishing, analytics, comments, or collaboration
- Bug evidence, onboarding, release walkthrough, or visual-regression-specific
  reader modes
- PDF or print-specific layout
- Deep links or cross-Guide navigation
- Changes to recording, semantic-event ingestion, or step detection
- A second platform-specific HTML reader
