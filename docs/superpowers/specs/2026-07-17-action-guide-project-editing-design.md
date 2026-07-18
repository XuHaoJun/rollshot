# Action Guide Project Editing Design

**Date:** 2026-07-17  
**Status:** Approved design — product review (`plan-ceo-review`) and engineering review (`plan-eng-review`, auto mode) applied 2026-07-17  
**Branch:** `feat/action-guide-projects`  
**Scope:** Make newly recorded Action Guides persistent, reopenable, fully editable projects with safe derived publish outputs

## Purpose

Rollshot currently reviews an Action Guide in memory and exports a portable
folder containing flattened keyframes, Markdown, JSON metadata, and an offline
HTML reader. That folder is a safe delivery artifact, not an editable source.
After Rollshot exits, the retained nearby frames, editable annotation graph,
and Timeline Workspace state are gone. Even correcting a typo therefore
requires recording the workflow again.

This design introduces a private `.rollshot-guide/` project directory for new
Action Guides. The project preserves the current editable Guide state across
app launches. Safe HTML, Markdown, Storyboard, GIF, MP4, and Issue Pack content
remain derived outputs generated from a reviewed project snapshot.

The product thesis is simple: an Action Guide is a maintainable document, not a
one-time export.

## Product Decisions

- The primary artifact for every newly recorded Action Guide is a
  `.rollshot-guide/` project directory.
- When recording ends, Rollshot immediately prompts for the first Save before
  editing begins. The prompt is skippable (`Save later`), but the default path
  commits the recording to disk so a crash during review loses only edits,
  never the recording itself.
- A project preserves current editable state: Guide and step text, step order,
  selected keyframes, nearby replacement frames, and committed annotations.
- Reopening starts a fresh editing session. Undo/redo history, unfinished
  drafts, pending agent proposals, selected tools, modals, and other runtime UI
  state are not persisted.
- The project contains a generated `publish/` directory with safe, flattened
  outputs. Project source data never appears in an ordinary export or Issue
  Pack.
- Saving the project first commits editable state atomically. It then updates
  every enabled derivative output in the background.
- A derivative failure never rolls back or invalidates a successful project
  save.
- Existing exported Guide folders remain readable delivery artifacts. They are
  not imported or upgraded into fully editable projects because their
  annotations and replacement frames cannot be reconstructed reliably.
- Sharing a complete editable project is a separate, explicitly warned action.
  Ordinary sharing uses a safe publish copy.
- `rollshot-app action-guide` becomes an Action Guide Home. Direct CLI routes
  are `action-guide --record` and `action-guide --open PATH`.
- This is an accepted breaking change to the Action Guide command and default
  file lifecycle.

## Current Context

As of the start of this design:

- `rollshot-action::export` emits `index.html`, `steps.md`, `session.json`, and
  flattened `keyframes/*.png` from one `ReviewedGuideExportJob`.
- The public `SessionManifest` is schema version 1 and contains reviewed Guide
  metadata plus interactive hotspots, but not editable annotation documents or
  nearby replacement frames.
- `TimelineWorkspace` owns the editable `Guide`, retained `FrameStore`, and
  per-step `ActionGuidePresentation` only for the current process.
- `FrameStore` retains a bounded set of full-resolution frames around detected
  steps; each step exposes at most seven nearby replacement frames under the
  current default configuration.
- `ImageDocument::flatten_snapshot()` deliberately excludes undo/redo history
  and is the existing source for safe exported pixels.
- The active Action Guide product path is behind the existing non-default
  `action-guide` Cargo feature and is shared by the Linux and macOS Timeline
  Workspace.

The existing exported Guide contract remains a publication contract. The new
project contract is separate and private.

## User Experience

### Action Guide Home

Running:

```text
rollshot-app action-guide
```

opens a shared Action Guide Home with three primary areas:

- `Record New`
- `Open Project...`
- `Recent Projects`

The command-line shortcuts are:

```text
rollshot-app action-guide --record
rollshot-app action-guide --open /path/to/example.rollshot-guide
```

The existing `--fullscreen` flag remains valid only together with `--record`;
`action-guide --fullscreen` without `--record` is rejected at parse time.

`--open` without a path opens the project folder picker. A cancelled picker
returns to Home without changing state.

Recent Projects shows a bounded local list of the ten most recently opened or
saved projects. Each item shows the project display name and last-opened time.
Publish freshness is not shown on Home; it belongs to the open workspace.
Missing entries remain recoverable: the Home marks them unavailable and offers
removal instead of silently dropping them.

The Home does not decode project images. It uses cached local recent metadata
and validates the selected project only when opening it.

Recent-project metadata is a small schema-versioned JSON file in the existing
rollshot config directory (`etcetera` base strategy, as `daemon/config.rs`
does), holding at most ten entries of project path, display name, and
last-opened timestamp — no thumbnails, no guide content. It is loaded
leniently (malformed file → empty list) and its paths never appear in
diagnostics. Home reloads this file at boot and whenever its window regains
focus, so a Linux recording child that saves a project appears without
restarting the Home process.

Old exported Guide folders are recognized by shape: a directory containing
`session.json` but no `project.json` is a readable export, not a project; the
Open flow explains this and offers the existing open-in-reader action on its
`index.html`.

### New recording and first save

After recording finishes, Rollshot immediately prompts for the first Save.
Accepting commits the recording as a project before any editing begins.
Choosing `Save later` enters the existing Timeline Workspace as an
`Unsaved Project`; the user may review and edit before saving, accepting that
a crash before the first Save loses the recording. The prompt copy makes this
plain. Cancelling the picker after choosing Save returns to the save-first
prompt; it never silently acts as `Save later` or discards the recording.

The first `Save Project` action chooses a parent directory and project name,
then creates:

```text
<name>.rollshot-guide/
```

An existing destination is never replaced. The user must choose another name
or location. Cancelling the picker leaves the unsaved workspace intact.

After the first successful save, the primary action becomes `Save`. The header
always makes `Unsaved changes`, `Saving`, or `Saved` legible. Closing with dirty
state presents exactly:

- `Save and Close`
- `Discard Changes`
- `Cancel`

There is no cross-process crash recovery in v1. Changes made after the last
successful Save may be lost if the process crashes. The save-first prompt
bounds this loss to edits: a saved recording itself is never at risk.

### Reopening and editing

Opening a valid project restores:

- Guide title
- Step title and caption
- Current step order and prior deletions
- Stable project-local step identity
- Current keyframe selection
- Retained nearby replacement frames
- Current committed annotation graph and callout explanations
- Enabled derivative output kinds

It deliberately starts new undo/redo history and does not restore:

- The previously selected step or annotation tool
- A draft annotation or open annotation modal
- Undo/redo stacks
- Pending, rejected, or accepted-agent-review UI state
- In-flight tasks, banners, dialogs, or consent state

The Timeline Workspace preserves its existing editing surface. Project Save is
the dominant header action. Publish status is secondary and does not compete
with the keyframe and step content.

### Publish and sharing

Every successful project Save schedules regeneration of:

- The required core publish output: offline HTML, Markdown, public manifest,
  and flattened keyframes.
- Every optional derivative previously enabled in the project: Storyboard,
  GIF, and MP4.

Publish state is modeled per output, but the default UI is one aggregate
header indicator (for example `Publishing…`, `Published`, `Needs attention —
Retry`). Per-output detail is available on demand behind that indicator, where
each output reports one of:

- `Updating`
- `Current`
- `Stale`
- `Failed — Retry`

The aggregate indicator is visually distinct from the project Save state so
`Saved` and an in-progress publish can coexist without ambiguity.

Closing after the project is saved is allowed while derivatives are still
updating. Unfinished work stops and remains stale. Reopening presents `Retry
All`; it does not silently start expensive MP4 work.

`Export Safe Copy` copies only publish artifacts corresponding to the current
project revision. Issue Pack consumes the same safe reviewed snapshot. If a
required output is stale, Rollshot regenerates it before sharing rather than
silently delivering an older revision. That regeneration is a visible blocking
step with progress and a Cancel action; cancelling aborts the share and leaves
the project and its publish state unchanged.

`Share Editable Project` shares the complete `.rollshot-guide/` directory and
must warn that it may contain original, unflattened, or visually redacted
pixels. It first requires the current project revision to be saved, then copies
that committed project while excluding the ephemeral `.lock` file and any
temporary transaction files. It is visually and textually distinct from
`Export Safe Copy`.

## Project Layout

The v1 directory layout is:

```text
example.rollshot-guide/
  project.json
  assets/
    frames/
      <sha256>.png
  publish/
    index.html
    steps.md
    session.json
    keyframes/
      001.png
      002.png
    storyboard.png       # when enabled and current
    guide.gif             # when enabled and current
    summary.mp4           # when enabled and current
  publish-state.json
```

Frame assets are immutable, content-addressed PNGs. Identical retained frames
are stored once. `project.json` and `publish-state.json` use only validated
relative paths rooted inside the project.

The project directory is user-owned and portable, but its contents are an
application data contract rather than a collection of independently editable
files. Manual changes are detected through schema, reference, image-decode, and
content-hash validation.

## Versioned Project Model

`project.json` is an independent contract from the public
`publish/session.json`. Its top-level v1 data includes:

```text
ActionGuideProjectManifestV1
  schema_version = 1
  revision: u64
  title
  capture_region
  input_source
  input_capability
  enabled_outputs
  frames[]
  steps[]
```

Each frame entry includes:

- Project-local frame ID
- Capture-relative timestamp
- SHA-256 digest of the encoded PNG bytes
- Width and height

The asset path is not stored: it is always derived as
`assets/frames/<sha256>.png` from the digest. Deriving the path removes the
per-frame path-traversal surface entirely and deduplicates identical frames by
construction.

Each step entry includes:

- Stable project-local step ID
- Explicit presentation order
- Title and optional caption
- Semantic kind, detection reason, and capture-relative timestamp
- Current keyframe ID
- Ordered nearby frame IDs
- Optional history-free annotation document for the current keyframe
- Guide-specific annotation explanations keyed by persisted annotation ID.
  Snapshot construction persists only explanations whose annotation still
  exists; in-session behavior (explanations surviving temporary annotation
  deletion) is unchanged.

Step IDs are stable across Save and reopen. A newly recorded Guide initializes
them from its unique candidate IDs, but the project contract does not expose or
depend on recording-session identity. Future structural editing may allocate
new IDs monotonically, but adding steps is not part of this design.

The persisted annotation document represents only currently committed
annotations, their stable IDs, geometry, style, order, and content. It excludes
source bitmap duplication, history stacks, draft state, and UI selection.

The manifest preserves the exact editable Guide title, including an empty
value. Publish continues to use the stable `Action Guide` fallback when the
trimmed title is empty.

V1 validation requires:

- `schema_version` is exactly 1 and the first committed revision is 1.
- The Guide contains at least one step; the editor therefore disables deleting
  the final remaining step.
- Frame IDs, step IDs, and annotation IDs are unique in their scopes.
- Step presentation order is contiguous and every current or nearby frame
  reference resolves.
- A step's current keyframe appears in its ordered nearby frame list.
- Save-time validation fully decodes every newly encoded frame, verifies its
  declared dimensions and digest, and requires it to match the project
  capture-region dimensions.
- Open-time validation verifies every referenced asset's existence, encoded
  byte digest, and PNG header dimensions without materializing full RGBA
  pixels. Full pixel decode remains lazy.
- An annotation document refers to the same current keyframe as its owning
  step, and all geometry/style data is valid for that source image.

V1 rejects unknown top-level and nested manifest fields instead of guessing
their meaning (`#[serde(deny_unknown_fields)]` on every project DTO — a
deliberate contrast to the lenient public `session.json`). An unknown newer `schema_version` is not opened for editing and
produces a clear “requires a newer Rollshot version” result. Future schema
versions must define explicit migration rules. V1 does not include a legacy
export migration.

## Components and Responsibilities

### `rollshot-action::project`

Owns:

- The versioned project DTOs
- Project validation
- Runtime-independent loaded project state
- Stable IDs and frame references
- Project snapshot construction contracts
- Atomic manifest and immutable-asset persistence
- Load result and error categories

It does not own iced state, file pickers, recent-project UI, background task
presentation, or OS shell integration.

### `rollshot-image-document`

Owns a public history-free persisted annotation snapshot and conversions to and
from a fresh `ImageDocument`. The persisted representation includes the current
annotation graph and excludes undo/redo state.

Concretely: the `Annotation` enum gains a feature-gated serde derive (its
geometry, style, and ID field types are already serde-ready), and
`ImageDocument` gains a rehydrating constructor taking a shared source plus the
persisted annotation list. Rehydration restores ID allocation as
`next_id = max(annotation id) + 1` and `next_number = max(callout number) + 1`
so `AnnotationId` stability (and explanation keys) survive reload, with empty
undo/redo history.

This is the only new persistence surface in the image-document engine. It is
introduced because Action Guide project reopening cannot safely serialize the
private `ImageDocument` runtime structure.

### `rollshot-app`

Owns:

- Action Guide Home and recent-project state
- Launch routing for Home, `--record`, and `--open`
- First-save and Open Project pickers
- Project-to-Timeline and Timeline-to-project adapters
- Dirty, saving, read-only, and lock-conflict UI state
- Background publish orchestration and revision race handling
- Publish status presentation
- Safe-copy and editable-project sharing actions

The Linux and macOS product paths use the same Home and Timeline state. Their
window handoff and picker integration may differ only where required by the
active platform runner.

Window hosting model: winit permits only one event loop per process
(`EventLoopError::RecreationAttempt`), so Home and the Timeline Workspace run
as phases of one iced daemon on both platforms — macOS extends the existing
`macos_product` phase enum; Linux gains an equivalent phased daemon. On Linux,
`Record New` from Home spawns a detached `rollshot-app action-guide --record`
child process, which owns the layer-shell overlay and its own Timeline exactly
as the `--record` CLI path does today. macOS records in-loop as today.

### Existing publish pipeline

`ReviewedGuideExportJob` remains the immutable boundary for safe output.
Project Save freezes one job after committing editable state; the existing
renderer and derivative encoders consume that job. Issue Pack and Export Safe
Copy do not read project internals independently.

## Data Flow

New recording:

```text
Recording
  -> save-first prompt
     -> Save -> committed project model
     -> Save later -> unsaved project model
  -> Timeline Workspace edits
  -> validated project snapshot
  -> atomic project save
  -> immutable ReviewedGuideExportJob
  -> background core and enabled derivatives
```

Reopening:

```text
project manifest + frame catalog
  -> validate schema, references, paths, and required assets
  -> loaded project model
  -> fresh Guide and annotation documents
  -> lazy frame resolution
  -> Timeline Workspace
```

Sharing:

```text
current saved project revision
  -> current safe publish snapshot
  -> Export Safe Copy or Issue Pack
```

No sharing path copies the private project assets by default.

## Save and Transaction Semantics

### First Save and Save As

First Save and Save As build a unique temporary sibling directory, write and
validate all referenced project assets and manifest, then perform a no-replace
rename to the final `.rollshot-guide/` path — the same
`renameat_with(NOREPLACE)` + RAII temp-cleanup pattern the standalone guide
export already uses (`guide_export.rs`). A collision is recoverable and never
deletes an existing project.

Save As leaves the source project unchanged and opens the successfully created
copy as the active writable project.

### Existing project Save

Saving an existing project proceeds in this order:

1. Freeze and validate a history-free project snapshot.
2. Encode any missing immutable frame assets to uniquely named temporary files.
3. Verify each new asset's hash, dimensions, and decode result.
4. Rename validated assets into their content-addressed locations.
5. Re-read the committed manifest revision and reject the Save as an external
   modification conflict if it differs from the workspace's base revision.
6. Serialize the next monotonic revision to `project.json.tmp` and fsync it.
7. Atomically replace `project.json` via same-directory rename, then fsync the
   project directory.
8. Mark the workspace clean only after the manifest replacement succeeds.
9. Schedule publish jobs for the committed revision.

The manifest is the commit point. Assets are written before they become
reachable. A crash may leave unreferenced immutable assets, but it must not
leave the last committed manifest referencing incomplete data. Cleanup of
unreferenced assets is not required for v1 correctness.

### Publish revisions

`publish-state.json` records the last successful project revision for each
output. Runtime `Updating` state is not treated as durable success.

A publish completion carries the revision it rendered. If the project has been
saved again, the old completion cannot mark the newer output current or replace
newer content. Revision arbitration extends the workspace's existing
operation-id staleness pattern (`GuideExportState` / `apply_export_finished`).

Each output publishes through a temporary path and atomic commit appropriate
to its shape: single-file derivatives keep the existing temp-sibling + rename
pattern already implemented in the storyboard, GIF, and MP4 encoders; the core
publish output builds a temporary sibling directory, renames the current one
aside, renames the new one into place, then removes the old directory
best-effort.

All publish work runs off the iced update thread via `Task::perform` +
`spawn_blocking` — including paths that today encode synchronously in
`update()`. MP4 publishing gains a cooperative cancellation token that kills
the ffmpeg child process; other encoders check cancellation between steps.

A missing or corrupt `publish-state.json` never blocks opening the project: it
is treated as all enabled outputs stale.

Project Save success and derivative success are deliberately independent.

## Locking and Read-Only Open

Rollshot obtains one project-local writer lock before allowing edits. A second
process opening the same project offers:

- `Open Read-Only`
- `Cancel`

Read-only mode allows navigation and safe viewing but disables Save, mutation,
agent-apply actions, and derivative settings. It may export a safe copy only
from outputs already current for the committed revision; it does not start a
new publish job.

The lock is an advisory exclusive OS file lock (`fs4::FileExt::try_lock`) on a
`.lock` file inside the project directory, mirroring the existing daemon
single-instance guard (`daemon/instance.rs`). OS advisory locks release
automatically when the holding process exits or crashes, so a held lock always
means a live writer on local filesystems — no stale-lock file or PID recovery
protocol exists in v1. The revision preflight before commit rejects an edit
based on an externally changed manifest even if the original lock was lost.

Writable projects on network filesystems are unsupported in v1 because their
advisory-lock semantics cannot provide the local-filesystem single-writer
guarantee. Rollshot documents that limitation; the no-silent-overwrite promise
applies to supported local filesystems. Atomic manifest replacement still
prevents a torn manifest, but it is not presented as a substitute for reliable
locking on an unsupported filesystem.

## Data Safety

- Private frame assets may include pixels later hidden by redaction. They are
  confined to the project and editable-project sharing flow.
- Every public keyframe is rendered from the current source plus committed
  annotations and permanently flattened before writing.
- Public HTML, Markdown, manifests, Storyboards, GIFs, MP4s, Safe Copies, and
  Issue Packs never reference paths under project `assets/`.
- Removing public HTML, CSS, JavaScript, or overlays cannot reveal redacted
  source pixels.
- Project loader paths reject absolute paths, parent traversal, symlink escape,
  non-PNG frame assets, hash mismatch, invalid dimensions, and references
  outside the project root.
- Diagnostics may include schema version, revision, step ID, asset category,
  output kind, and error category. They must not include image bytes, Guide
  text, annotation text, or full paths containing the Guide title.
- `Share Editable Project` requires an explicit warning that original or
  redacted pixels may be included. That warning cannot be globally disabled in
  v1.

## Loading, Performance, and Bounds

Opening a project first loads and validates bounded textual metadata. It does
not eagerly decode every retained full-resolution frame.

Before constructing a writable workspace, Open streams every referenced asset
to verify its digest and reads its PNG header to verify dimensions. This bounds
memory and does not materialize RGBA pixels. A project that fails these checks
does not open as writable.

The first workspace presentation resolves only the selected step's current
keyframe and nearby strip. Other assets load as the user changes steps. Loading
shows an explicit step-local state rather than a blank image.

Decoded project images use a bounded cache. Cache eviction may drop decoded
pixels but never the immutable on-disk asset reference. Added memory must not
grow linearly with the total number of project steps and nearby frames.

If a digest-valid PNG nevertheless fails full pixel decode when first resolved,
the workspace immediately enters a corrupted read-only state, reports the
affected step/asset category, and disables Save and Publish. It never continues
as a partially writable project.

Home and Recent Projects never decode frame assets. Background publish processes
one step image at a time, preserving the existing bounded flatten-and-encode
goal. MP4 and other derivative work must not block iced update handling.

The lazy frame provider is a new step-frame source in `rollshot-action`: an
enum over the existing in-memory `FrameStore` (fresh recordings) and a
project-backed source (reopened projects) that decodes content-addressed PNG
assets on demand into a byte-bounded LRU cache of shared images (default
exactly 256 MiB, not a frame count). Timeline Workspace, annotation
presentation, and storyboard paths consume this source instead of touching
`FrameStore::retained` directly. Project open therefore never eagerly decodes
the retained frame set.

Publish jobs from a reopened project stay bounded the same way: the reviewed
export job gains a project-frame image variant carrying the asset digest plus
the persisted annotation snapshot, resolved (decode, flatten, encode, drop)
one step at a time inside the existing per-step render loop instead of holding
every decoded keyframe in memory when the job is frozen.

## Error Handling

| Failure | Product behavior |
| --- | --- |
| First-save picker cancelled | Keep unsaved workspace and edits |
| Destination already exists | Keep workspace; request a new path; never replace |
| Asset encode/write/hash verification fails | Leave prior project revision intact; workspace remains dirty |
| Manifest validation or atomic replace fails | Leave prior revision openable; workspace remains dirty |
| Manifest revision changed outside this writer | Reject Save as a conflict; preserve both the external committed revision and local dirty edits |
| Required asset missing, hash-mismatched, or header-invalid on open | Do not construct a writable workspace; identify affected step/asset category |
| Digest-valid asset fails lazy pixel decode | Downgrade the workspace to corrupted read-only; disable Save and Publish; identify affected step/asset category |
| Unknown newer schema | Refuse writable open; explain that a newer Rollshot is required |
| Writer lock held | Offer read-only open or cancel |
| `publish-state.json` missing or corrupt | Open normally; treat all enabled outputs as stale |
| Core publish fails | Project remains saved; core output becomes failed/stale with Retry |
| Storyboard/GIF/MP4 fails | Project remains saved; only that derivative becomes failed/stale |
| Older publish job completes late | Drop or retain it as stale; never mark the current revision current |
| Close during publish | Stop work; project remains saved; unfinished outputs remain stale |
| Safe export requested from stale revision | Regenerate required output before sharing; never silently export stale content |
| Old exported Guide selected in Open Project | Explain that it is a readable export, not an editable project; offer to open its `index.html` in the offline reader |
| Safe export regeneration cancelled | Abort the share; project and publish state unchanged |

## Testing

### Project model and persistence

- Recording-to-project snapshot preserves Guide metadata, stable IDs, current
  keyframes, nearby order, timestamps, and input capability.
- Save/load round trip reproduces the same current editable state.
- Delete and keyframe replacement state survive reopen.
- Content-addressed frames deduplicate identical images and reject hash or
  dimension mismatch.
- Open validates every referenced asset's digest and PNG header without full
  pixel decode; Save fully decodes newly encoded assets before commit.
- Unknown newer schema, malformed JSON, duplicate IDs, missing references,
  invalid order, and illegal paths fail with stable categories.
- Annotation snapshot round trips every supported annotation variant, geometry,
  style, text, order, ID, and explanation.
- Rehydrated `ImageDocument` begins with empty undo/redo history.

### Atomicity and fault injection

Fault injection uses real-filesystem manipulation in temp directories
(truncated files, pre-created destinations, removed permissions, corrupted
bytes) rather than a filesystem abstraction trait — no injectable FS layer is
introduced.

- Failure before asset commit leaves the previous manifest and assets usable.
- Failure after asset commit but before manifest commit leaves only unreachable
  assets; the previous project remains openable.
- Manifest temporary-write and atomic-replace failures keep the workspace dirty
  and preserve the previous revision.
- A changed base revision rejects Save without overwriting either the external
  committed revision or the local dirty workspace state.
- First Save and Save As collision tests prove no existing directory is
  replaced.
- A successful save increments revision exactly once.

### Publish and privacy

- Core and optional derivative statuses record the revision they rendered.
- An old revision completing after a newer Save cannot overwrite current state.
- FFmpeg unavailable, process failure, and cancellation leave the project saved
  and MP4 failed/stale.
- Safe publish keyframes contain committed annotations and do not expose pixels
  hidden by redaction.
- Public HTML and manifests contain no private project asset path.
- Safe Copy and Issue Pack contain only current safe publish artifacts.
- Editable-project sharing requires the privacy warning path.

### Loader security

- Reject `..`, absolute paths, symlink escapes, invalid content hashes,
  invalid PNG headers, and project-root escapes before writable open.
- A digest-valid asset that fails lazy full decode transitions the workspace to
  corrupted read-only and rejects Save/Publish.
- Reject a project where a required keyframe or annotation source is missing.
- Diagnostics tests assert that Guide text, annotation text, image content, and
  title-bearing paths are absent.

### Locking

- Acquiring the writer lock twice in one process fails; dropping the guard
  releases it (mirrors the existing `daemon/instance.rs` tests).
- A held lock yields the read-only offer; read-only mode rejects Save and
  publish mutations.
- A missing or corrupt `publish-state.json` opens the project with all enabled
  outputs stale.

### App state and entry points

- CLI parsing covers Home, `--record`, `--open PATH`, and `--open` with picker.
- Home actions, recent entries, unavailable paths, and picker cancellation are
  deterministic.
- The post-recording save-first prompt covers accept, `Save later`, and picker
  cancellation; `Save later` enters the unsaved workspace intact.
- Workspace tests cover Unsaved, dirty, saving, saved, read-only, lock
  conflict, and close confirmation.
- Publish UI covers the aggregate indicator plus per-output Updating, Current,
  Stale, Failed, Retry, and Retry All in the detail view.
- Share-triggered regeneration shows progress and honors Cancel without
  changing project or publish state.
- Selecting an old exported Guide in Open Project offers the offline reader
  hand-off.
- Closing a saved project during publish leaves stale status; reopening does not
  automatically start expensive work.
- Reopening creates fresh undo/redo and no pending proposal or modal state.

### Performance

- Home reads no PNG assets.
- Project open does not decode all retained frames.
- Selecting a step loads only its required current and nearby images.
- Cache-bound tests prove decoded image count and memory proxies remain bounded
  as total project step count increases.
- Background publish remains responsive and processes flattened images with
  bounded added memory.

### Cross-platform verification

- Record, first-save, close, Home reopen, edit, and Save on active Linux.
- The same lifecycle on active macOS.
- Folder picker cancellation and writer-lock behavior on both platforms.
- Safe Copy and editable-project warning on both platforms.
- Project created on Linux opens and saves on macOS, and vice versa, when image
  and schema versions are supported.

## Success Criteria

A user can record a new Action Guide, save it as a `.rollshot-guide/` project,
close Rollshot, reopen it from Home, Recent Projects, or `--open`, and recover
the current keyframe choices, nearby replacement frames, and committed
annotations. Correcting a title or caption and pressing Save commits the edit
without requiring another recording. Every enabled derivative updates against
that committed revision or clearly reports itself stale/failed.

A Safe Copy or Issue Pack contains only reviewed, flattened, current publish
content and cannot recover project source pixels. Sharing the full editable
project is an explicit, warned operation.

No failed Save corrupts the previous committed revision, no concurrent writer
silently overwrites another, and project loading does not eagerly decode the
entire retained image set.

## Explicit Non-Goals

- Editing or upgrading legacy exported Guide folders
- Reconstructing annotations from flattened PNGs
- Persisting undo/redo history
- Persisting unfinished annotation drafts, selections, dialogs, banners, or
  agent proposals
- Adding, splitting, or merging steps
- Arbitrary full-session scrubber or selecting frames outside each step's
  retained nearby set
- Autosave or crash-recovery drafts
- Cloud sync, collaboration, comments, or multi-writer editing
- Background service completion after the Rollshot process exits
- Automatically updating external Safe Copies or previously exported Issue
  Packs
- Windows file integration
- OS-level `.rollshot-guide` file association
- Workflow comparison, regression Storyboards, or automatic Guide version
  alignment

## Engineering Decisions (locked by plan-eng-review, 2026-07-17)

- **Annotation snapshot boundary:** feature-gated serde derive on the
  `Annotation` enum plus a rehydrating `ImageDocument` constructor restoring
  `next_id` / `next_number` from maxima, empty history (see
  `rollshot-image-document` component section).
- **Lazy frame provider:** step-frame source enum in `rollshot-action` over
  in-memory `FrameStore` and a project-backed, byte-bounded LRU decode cache;
  publish jobs use a lazy project-frame image variant (see Loading section).
- **Atomicity/durability:** manifest = tmp + fsync + same-dir rename + dir
  fsync; first Save/Save As = `renameat_with(NOREPLACE)` temp-sibling pattern
  from `guide_export.rs`; core publish dir = build-aside-swap; single-file
  derivatives keep their existing tmp+rename.
- **Locking:** `fs4` advisory exclusive lock on `<project>/.lock`; crash
  auto-release; no stale-lock protocol; revision-conflict preflight before
  commit; writable network-filesystem projects unsupported in v1.
- **Publish orchestration:** operation-id + revision guards extending the
  existing `GuideExportState` pattern; all encoding on `spawn_blocking`;
  cooperative cancellation with ffmpeg child kill; corrupt/missing
  `publish-state.json` → all-stale.
- **Window hosting:** one iced daemon with Home/Timeline phases per platform
  (winit forbids event-loop recreation); Linux `Record New` spawns a detached
  `--record` child process; macOS records in-loop.
- **Recent projects:** schema-versioned JSON, ≤10 entries, in the `etcetera`
  config dir; lenient load; paths excluded from diagnostics.
- **Old-export detection:** `session.json` present without `project.json` →
  readable export → offer existing open-in-reader action.
- **Content addressing:** digest of encoded PNG bytes; asset paths derived
  from digests, not stored.
- **Strictness:** `deny_unknown_fields` on all project DTOs.
- **Test seams:** real-filesystem fault injection in temp dirs; no FS
  abstraction trait.

### Aggregate publish indicator derivation

From per-output states over enabled outputs: any `Updating` → `Publishing…`;
otherwise any `Failed` or `Stale` (core or derivative alike) →
`Needs attention — Retry`; otherwise `Published`. Core-output failure is
listed first inside the detail view but does not get a distinct aggregate
state in v1.

### Recommended plan split

`superpowers:writing-plans` should produce three sequential plans:

1. **Project model and persistence** — `rollshot-action::project` DTOs,
   validation, atomic save/load, content-addressed assets, and the
   `rollshot-image-document` snapshot surface.
2. **App integration** — Home, launch routing, phased daemon hosting, pickers,
   save-first prompt, dirty/lock/read-only workspace states, lazy frame
   source wiring.
3. **Publish orchestration and sharing** — revision-arbitrated background
   publish, aggregate indicator, Safe Copy / Issue Pack / editable-project
   sharing flows.
