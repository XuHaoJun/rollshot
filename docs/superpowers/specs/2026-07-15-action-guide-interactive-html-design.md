# Action Guide Interactive HTML Design

**Date:** 2026-07-15  
**Status:** Approved design; engineering-reviewed  
**Branch:** `feat/interactive-html-guide`  
**Scope:** Add a deterministic offline `index.html` reader to every exported Action Guide folder
**Engineering review:** 2026-07-16, auto mode

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

`NumberCallout` currently stores its number and geometry but no explanatory
text. The Timeline Workspace therefore needs Guide-specific explanation state;
the renderer cannot infer explanations from the existing annotation graph.

The standalone and Issue Pack exporters currently receive `Guide` and
`FrameStore` directly, while only the Timeline Workspace owns committed
annotation presentation. Both export paths are synchronous after the picker
returns. The implementation must replace those borrowed export inputs with one
owned reviewed export job before either path can produce the same flattened
keyframes and remain responsive.

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

For v1, `TextNote.text` is its explanation. A `NumberCallout` explanation is
optional Guide-specific presentation metadata keyed by `AnnotationId` inside
the matching step annotation document. It is editable in the Timeline
Workspace, is retained while an annotation is temporarily absent so delete-undo
restores it, and is exported only when the referenced annotation still exists.
This avoids changing the general-purpose `rollshot-image-document::Annotation`
shape solely for Action Guides.

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
index.html          HTML + CSS + JavaScript + encoded viewer-data snapshot
steps.md            human-readable portable fallback
session.json        machine-readable export metadata
keyframes/*.png     reviewed flattened step images
```

This deliberately duplicates small textual metadata between `session.json`
and `index.html`. It avoids local-file `fetch()` restrictions and reduces the
number of files that must remain together. The immutable reviewed export job is
the source for all outputs; neither serialized artifact is used to generate the
other.

## Components and Responsibilities

### Owned reviewed export job

Export begins by freezing one immutable owned job from reviewed state:

```text
ReviewedGuideExportJob
  guide title
  capture and semantic metadata
  steps[]
    index, title, caption, event metadata
    shared immutable reviewed source pixels
    current committed annotation snapshot, without history
    relative keyframe path
    interactive hotspots[]
      normalized image position and hit area
      explanation text
```

The job includes only committed current state. It excludes:

- Any separately serialized or exported copy of original pixels hidden by redaction
- Annotation history and undo/redo state
- Pending or rejected LLM suggestions
- Provider, model, prompt, and provenance data
- Raw semantic input payloads
- OCR data

For a step with a matching committed annotation document, the job owns an
immutable flatten snapshot: a shared source-pixel `Arc` plus cloned current
annotations. Otherwise it shares the retained reviewed keyframe. The worker
flattens and encodes one step at a time and drops that materialized bitmap
before continuing. Unredacted source pixels may therefore exist transiently in
the private in-memory job, as they already do in the editor, but they are never
written or embedded; every exported PNG is the final flattened result.

`rollshot-image-document` provides the history-free flatten snapshot primitive.
`rollshot-action` owns `ReviewedGuideExportJob`, hotspot data, validation, and
the deterministic folder renderer. `rollshot-app` is the adapter that joins
`Guide`, `FrameStore`, and `ActionGuidePresentation` into the owned job. This
keeps both standalone and Issue Pack exports on one renderer without moving UI
state into the Action Guide engine.

### Guide folder renderer

One deterministic renderer consumes the owned job and writes PNG, Markdown,
manifest, and HTML outputs. All representations therefore share title, order,
text, metadata, and final reviewed pixels.

The renderer:

- Does not access mutable UI state.
- Does not invoke LLM, OCR, network, clipboard, or browser APIs.
- Treats `index.html` as required, not as a best-effort optional artifact.
- Escapes all user- and model-originated text as data rather than raw markup.
- Produces deterministic content apart from explicitly supplied export-time
  metadata and unique destination naming.

The renderer accepts an exact destination directory; it does not choose
standalone naming and does not assume the fixed name `action-guide`. The app
owns standalone destination naming and post-export actions. Issue Pack owns its
outer transaction and supplies `<pack-temp>/action-guide` as the destination.

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

Viewer data is serialized as JSON in a non-executable
`<script type="application/json">` element. The renderer escapes `<`, `>`, `&`,
U+2028, and U+2029 before embedding; the viewer reads `textContent`, parses it,
and renders user content with `textContent`/DOM node APIs. No Guide text is
interpolated into executable JavaScript, CSS, element attributes, or HTML.

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

Before filesystem writes, export-job validation requires:

- At least one Guide step.
- A usable title after fallback.
- One retained reviewed keyframe per step.
- Non-empty explanation text for every hotspot.
- Finite hotspot geometry whose hit area intersects its image.

Standalone export chooses a unique final name under a short-lived exclusive
Rollshot parent-directory lock, creates a uniquely named temporary sibling,
writes every required artifact, checks again that the final path is absent,
and only then renames the directory into place. It never calls the current
replacement-style `swap_into_place` path and never deletes an existing final
directory. A conflicting external filesystem writer causes a recoverable
failure rather than replacement.

Any job construction, encode, template, write, or rename failure removes the
temporary directory, preserves the editable Timeline Workspace, and reports a
recoverable error. No final or temporary partial Guide remains.

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
- Searchable strings are normalized once at initialization; query work is
  linear in the number of steps and explanations.
- Zoom uses browser presentation transforms and does not allocate a new bitmap.
- One delegated event handler services step rows and hotspots; navigation does
  not accumulate listeners or retained DOM subtrees.
- Step changes show a short explicit loading state while the next image decodes
  instead of presenting unexplained blank content.
- The Timeline Workspace starts rendering through `Task::perform`; blocking
  flatten/PNG/filesystem work runs in `tokio::task::spawn_blocking` and reports
  completion back through a message. Export controls are disabled while that
  job is active, but editing remains available after success or failure.
- Retained frame pixels are shared into the job with `Arc<RgbaImage>`. The
  worker materializes, encodes, and drops one final step image at a time, so
  added peak memory is bounded to roughly one RGBA image, its PNG buffer, and
  small metadata rather than one flattened bitmap per Guide step.

## Engineering Review Decisions

### Auto decision D1 — Keep callout explanations in Action Guide presentation

**Context:** `TextNote` already has text; `NumberCallout` has only number and
geometry, and changing the shared annotation enum has a high cross-workspace
blast radius. **ELI10:** the numbered bubble and the sentence explaining it are
two different things today. **Stakes:** without a persisted sentence, numbered
hotspots would be empty or invented.

**Recommendation:** add an explanation map keyed by `AnnotationId` to each
Timeline step annotation document. Snapshotting joins only live explanatory
annotations, using navigator presentation order. **Completeness:** complete for
v1 because exports originate from Timeline Workspace presentation.

- Put text on every `Annotation` variant: unified, but a broad model/API change
  with unrelated result-workspace and automation churn.
- Put optional text only on `NumberCallout`: history-safe, but still couples a
  general drawing primitive to Guide semantics.
- Use the presentation map: smallest ownership-correct change; it requires the
  Timeline editor to maintain a selected-callout explanation field.

**Effort / maintenance / net:** medium / low / selected. Stale map entries are
harmless and are never exported; retaining them also preserves text across
delete-undo.

### Auto decision D2 — Use an owned cross-path export job

**Context:** only the app can see Guide data, retained frames, and committed
presentation together. Issue Pack currently lacks the presentation. **ELI10:**
both export buttons must take the same sealed box of reviewed data to the same
printer. **Stakes:** separate borrowed inputs would keep producing raw Issue
Pack keyframes or allow UI edits to race an export.

**Recommendation:** build `ReviewedGuideExportJob` once in `rollshot-app`, then
pass ownership to the `rollshot-action` renderer for standalone and Issue Pack
flows. `ActionGuideIssueAssets` must derive from this job rather than separately
from `Guide`. **Completeness:** complete; optional GIF behavior remains a
separate existing derivative and is not redefined by this feature.

- Extend the old borrowed exporter: low initial effort, but cannot safely move
  to a worker or include presentation without lifetime coupling.
- Duplicate an HTML renderer in Issue Pack: fast locally, but guarantees format
  drift and violates required-artifact rollback semantics.
- Owned common job: moderate refactor, one source of truth, straightforward
  testing.

**Effort / maintenance / net:** medium / low / selected.

### Auto decision D3 — Stream final pixels on a blocking worker

**Context:** the existing Storyboard snapshot clones one final bitmap per step;
that can approach gigabytes for long full-resolution Guides. Export is also
currently synchronous in the iced update path. **ELI10:** carry one large
picture through the machine at a time, not the whole album. **Stakes:** UI
freezes and out-of-memory failures are otherwise plausible.

**Recommendation:** share retained/source images with `Arc`, freeze annotation
lists and metadata, then flatten, encode, write, and drop one step at a time in
`spawn_blocking`. **Completeness:** complete for exporter-added memory; the
existing editor continues to own its normal retained frames.

- Reuse the Storyboard all-images snapshot: least code, unacceptable unbounded
  added memory.
- Export synchronously one step at a time: bounded memory, but freezes the UI.
- Owned shared job plus worker streaming: more foundation work, bounded memory
  and responsive lifecycle.

**Effort / maintenance / net:** medium-high / medium / selected. This requires
focused compatibility tests where retained frame image ownership changes from
owned `RgbaImage` to shared `Arc<RgbaImage>`.

### Auto decision D4 — Separate rendering from destination policy

**Context:** the current exporter always replaces `out/action-guide`; standalone
exports must be unique while Issue Pack already owns an outer transaction.
**ELI10:** the printer should print into the folder it is handed, while the two
callers decide which folder is safe. **Stakes:** reusing replacement code could
silently delete a prior Guide.

**Recommendation:** renderer takes an exact destination; standalone uses locked
name allocation and a temp sibling, while Issue Pack renders inside its outer
temp tree. Never remove a final destination. **Completeness:** complete for
Rollshot writers; an external collision is detected and returned as an error.

- Keep fixed `action-guide`: incompatible with standalone naming.
- Add flags to one exporter: compact signature but mixes naming, transaction,
  and rendering policy.
- Exact destination plus caller-owned policy: slightly more API surface, clear
  transaction ownership.

**Effort / maintenance / net:** medium / low / selected.

### Auto decision D5 — Treat clipboard as capability-detected enhancement

**Context:** clipboard writes require a secure context and user activation;
browser policy under `file://` is not identical everywhere. Local file URLs are
generally potentially trustworthy, but permission rejection remains normal.
**ELI10:** ask the browser to copy, but always show the words when it says no.
**Stakes:** false “Copied” feedback destroys trust in the core offline promise.

**Recommendation:** attempt `navigator.clipboard.writeText` only from the button
activation and only when present. On rejection, open a focused read-only
textarea/dialog, select all text, and instruct `Ctrl/Cmd+C`; do not depend on
deprecated `execCommand`. **Completeness:** complete across browser policy
differences because manual copy remains available.

- Promise Clipboard API success: simplest UI, incorrect cross-browser contract.
- Fall back to `execCommand`: sometimes works, deprecated and still policy-bound.
- Honest manual fallback: a little more UI, stable and testable.

**Effort / maintenance / net:** low / low / selected.

### Auto decision D6 — Version the exported data contract

**Context:** `session.json` has no schema version or Guide title, while embedded
viewer data becomes a second consumer of the same metadata. **ELI10:** put a
version number on the box before other tools start opening it. **Stakes:** later
container reuse would otherwise require guessing field meanings.

**Recommendation:** add `schema_version: 1` and `title` to the manifest and
viewer snapshot, keep old manifest deserialization compatible with serde
defaults, and test identical normalized step metadata across all outputs.
**Completeness:** complete for the v1 reader contract; it does not promise a
general migration framework.

- Leave the manifest unchanged: smallest diff, creates immediate ambiguity.
- Create a separate HTML-only schema: isolates viewer, duplicates semantic
  contracts.
- Version shared normalized export metadata: modest change, clear future path.

**Effort / maintenance / net:** low / low / selected.

### Auto decision D7 — Reuse platform shell integration and keep workspace state

**Context:** `result_workspace::actions::reveal` already implements macOS and
Linux reveal behavior, but Timeline export currently exits after success and
there is no shared open-file helper. **ELI10:** keep the editor open and give it
two ordinary OS buttons. **Stakes:** duplicating commands creates platform drift;
exiting loses the promised recovery and follow-up workflow.

**Recommendation:** promote the reveal helper to shared app infrastructure, add
`open_path` using `open` on macOS and `xdg-open` on Linux, and store the latest
successful `index.html`/folder paths in Timeline state. Spawn success means
“launch requested,” not proof that a browser displayed the file.
**Completeness:** complete for Rollshot's supported Linux/macOS product paths.

- Duplicate shell commands in Timeline: low effort, duplicate tests and drift.
- Add a browser-opening crate: portable abstraction, unnecessary dependency for
  two supported platforms.
- Promote current helper and add its sibling: small, consistent change.

**Effort / maintenance / net:** low / low / selected.

### Auto decision D8 — Add focused Playwright `file://` coverage

**Context:** Rust tests can validate emitted bytes and PNGs but cannot prove
keyboard, responsive DOM, clipboard fallback, or lazy image behavior. The repo
has no current browser-test project. **ELI10:** test the little offline website
in real browser engines, not only as a long string. **Stakes:** the signature
reader experience could regress while all Rust tests stay green.

**Recommendation:** add one small pinned Playwright test package under
`scripts/html-guide-e2e/`; generate fixtures through the Rust renderer and open
them with a `file://` URL in Chromium and Firefox CI projects. Keep WebKit as a
useful compatibility signal and retain real Safari as manual macOS verification.
Block non-`file:` requests in the test context. **Completeness:** complete for
automated browser behavior; OS shell actions remain Rust/platform tests.

- Rust/string tests only: no new toolchain, poor behavioral confidence.
- Hand-written WebDriver integration: stays Rust-heavy, much more harness code.
- Focused Playwright project: one extra pinned dev toolchain, best browser-level
  coverage and failure diagnostics.

**Effort / maintenance / net:** medium / medium / selected. This dependency is
test-only and does not enter Rollshot binaries or exported folders.

## Implementation Plan Constraints

The later `superpowers:writing-plans` pass must decompose work in this order:

1. Export contracts and ownership — Guide title, explanation map, shared frame
   pixels, history-free flatten snapshot, manifest v1. Verify with focused unit
   and compatibility tests.
2. Deterministic renderer and transaction — validation, sequential flatten/PNG,
   Markdown/JSON/HTML, rollback, unique destination policy. Verify with tempdir
   integration tests and adversarial strings.
3. HTML viewer — implement against a checked-in/generated v1 fixture while step
   2 stabilizes. Verify with Playwright under `file://`.
4. Timeline Workspace integration — title/explanation editing, owned job build,
   background task states, Open Guide, Show in Folder. Verify state-machine and
   platform helper tests.
5. Issue Pack integration — consume the same owned job, list `index.html` in
   assets/attachments, and preserve the outer transaction. Verify folder and ZIP
   rollback/output tests.
6. Cross-platform runtime verification — Linux and macOS export plus Chrome,
   Firefox, and Safari checks from the Testing section.

After the v1 schema fixture is fixed, renderer work (step 2), viewer behavior
(step 3), and most Timeline UI state work (step 4) may proceed in parallel.
Issue Pack integration depends on the renderer API; runtime verification
depends on all implementation tasks.

## What Already Exists

- Deterministic `Guide` step metadata and retained keyframes in
  `rollshot-action`.
- Atomic temporary-folder export tests for Markdown, manifest, and PNG assets.
- Committed Timeline annotation documents and `ImageDocument::flatten()`.
- Stable annotation navigator ordering and image-space bounds/centers.
- Annotation-aware Storyboard snapshotting, useful as behavior reference.
- Issue Pack folder/ZIP outer transactions and asset manifests.
- Linux/macOS reveal behavior in Result Workspace.
- iced async tasks and an existing `spawn_blocking` Storyboard-copy pattern.

## Explicitly Not in Scope

In addition to Non-Goals below: changing capture backends, recording, step
detection, semantic event privacy, OCR persistence, result-workspace annotation
semantics, GIF/MP4 rendering semantics, a hosted HTTP viewer, Windows shell
integration, and a general-purpose browser application framework are not part
of this work.

## Failure Modes

| Failure | Automated check | Product behavior | User visibility |
| --- | --- | --- | --- |
| Empty Guide or missing keyframe | export-job validation test | no job starts, no files | recoverable export error |
| Invalid/non-finite hotspot | validation/property tests | reject before writes | recoverable annotation validation error |
| PNG/HTML/manifest write fails | injected filesystem failure | remove temp, preserve final/workspace | export failed banner |
| Final-name collision | same-second and external-collision tests | choose suffix or fail; never delete | success path or recoverable error |
| Worker panic/cancel | iced message/state test | clear busy state; temp guard cleans up | export failed/cancelled banner |
| Issue Pack HTML failure | folder/ZIP integration test | outer transaction rolls back | Issue Pack export error |
| Missing PNG after export | Playwright fixture | only that step shows unavailable state | inline `Image unavailable` |
| Clipboard denied | Playwright permission rejection | selected manual-copy field | explicit manual-copy instructions |
| Shell opener/reveal fails | injected command-runner tests | Guide remains exported and paths retained | action-specific error, export remains successful |
| Malicious text payload | serializer and browser tests | rendered only as text | no script/markup execution |

Diagnostics for validation errors must log only structural fields such as step
index, annotation kind, and error category; they must not log Guide text or
destination paths containing the title.

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

The automated browser matrix is Chromium and Firefox. Playwright WebKit is an
additional compatibility signal, not a substitute for the manual Safari check.
Tests reject any HTTP(S) request and allow only the document plus relative
`file:` keyframes.

### Coverage map

| Layer | Required coverage |
| --- | --- |
| `rollshot-image-document` | immutable flatten snapshot equals document flatten; redaction output excludes source pixels; no history copied |
| `rollshot-action` model | Guide title default/edit; manifest v1 old-read compatibility; shared retained-image behavior |
| `rollshot-action` renderer | validation, output parity, escaping, sequential pixel processing, rollback, deterministic artifacts |
| Timeline Workspace | explanation lifecycle, snapshot isolation, busy/success/failure state, no exit after export |
| Issue Pack | same flattened keyframes and HTML, manifest/attachment listing, folder and ZIP rollback |
| HTML viewer | navigation, search, hotspots, zoom, keyboard, themes, accessibility, missing images, copy fallback, no network/lazy decode |
| Platform helpers | macOS `open`/reveal and Linux `xdg-open`/reveal command construction plus spawn errors |

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
immutable reviewed export job. Export never silently replaces earlier output,
leaves a half-built folder, or exposes pixels hidden by a redaction.

The engineering feasibility gate additionally requires that export filesystem
and pixel work run off the iced update thread, added peak bitmap memory does not
scale with step count, and both standalone and Issue Pack outputs consume the
same owned reviewed job.

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
