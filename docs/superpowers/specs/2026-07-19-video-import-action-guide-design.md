# Local Video Import to Action Guide — Design

**Status:** Approved design
**Date:** 2026-07-19
**Branch:** `feat/video-import-action-guide`

## Summary

Rollshot will import an existing local screen recording into the Action Guide
timeline as a visual-only draft. The importer will detect meaningful visual
settles, extract a bounded set of derived keyframes, and open the existing
review workspace. The user can then edit, annotate, save, and export the guide
or an Issue Pack through existing workflows.

The original video remains a read-only input while import runs. Rollshot does
not copy it into the project, retain a reference to it, process its audio, or
send it to an agent. Successful projects and exports contain only reviewed
derived image evidence and privacy-safe provenance.

## Product Thesis

QA teams and customers often report bugs with recordings produced by Loom,
QuickTime, OBS, or other tools. Requiring Rollshot to have recorded the original
session excludes common real-world evidence. The valuable Rollshot behavior is
not generic video analysis; it is turning an external recording into a
reviewable, editable, redaction-aware, agent-ready evidence artifact.

The signature moment is entering the existing timeline with a useful draft
while being told exactly what Rollshot knows: steps came from visual changes,
not from observed mouse or keyboard events.

## Goals

- Add a discoverable **Import Recording…** action to the Action Guide home on
  Linux and macOS.
- Accept local `.mp4`, `.mov`, `.mkv`, and `.webm` files, subject to FFmpeg
  codec support and content validation.
- Detect visual changes with the existing Rollshot luma metrics and
  deterministic visual detector semantics.
- Produce at most 200 automatically generated, chronologically ordered draft
  steps with coverage from the beginning through the end of the recording.
- Label automatically generated steps as `UI changed`; never claim Click,
  Typing, or Scroll without user edits.
- Open the result in the existing unsaved Action Guide timeline and preserve
  existing edit, keyframe, annotation, save, publish, Action Guide export, and
  Issue Pack export behavior.
- Keep working memory, retained-frame count, and scratch storage bounded
  independently of video duration. Decode work may grow linearly with duration
  but remains observable and cancellable.
- Show meaningful progress, support cancellation during both passes, and leave
  no partial project after cancellation or failure.
- Persist privacy-safe imported-video provenance so reopened projects and
  exports continue to identify the guide as visual-only.

## Non-Goals

- Video URLs, network downloads, `yt-dlp`, authentication, or hosted-video
  integrations.
- Copying the original video into a project or retaining an external path to
  it after import.
- Audio extraction, playback, transcription, captions, or Whisper integration.
- Automatically inferring Click, Typing, Scroll, typed text, or input targets.
- User-tunable sampling, detector, scaling, or step-limit settings.
- Video playback, trimming, seeking, or general video editing UI.
- Direct agent analysis or automatic model upload.
- Background, batch, or concurrent video imports.
- Replacing the live Action Guide recording path.

## User Experience

### Action Guide home

The home presents three peer actions:

1. **Record New**
2. **Import Recording…**
3. **Open Project…**

Import opens a file picker filtered to `.mp4`, `.mov`, `.mkv`, and `.webm`.
The filter is for discoverability only. The importer probes the selected file
and rejects files without a readable video stream even if their extension is
accepted. Cancelling the picker is a no-op.

### Preflight and FFmpeg setup

Import requires a verified FFmpeg/FFprobe toolchain. If it is unavailable, the
existing managed FFmpeg setup experience appears before analysis starts. The
managed toolchain resolver must return explicit paths for both executables;
the importer never performs its own PATH or download logic.

Preflight reads only container and video-stream metadata needed for import:
duration, dimensions, time base, and stream availability. It rejects an empty
file, a missing video stream, invalid dimensions, an unreadable duration, or a
decoder/probe failure with a recoverable message on the Action Guide home.

### Processing

The processing view replaces the home content and shows:

- `Finding visual changes — Pass 1 of 2` or
  `Extracting evidence — Pass 2 of 2`;
- processed source time and total duration;
- the current retained candidate count;
- a statement that processing is local and audio is ignored; and
- one **Cancel** action.

The UI does not promise a wall-clock completion time. Progress is based on
source timestamps reported by the worker. Messages carry an import operation
ID; late progress or completion from a cancelled or superseded operation is
ignored.

### Timeline result

Successful import opens the normal unsaved timeline. A persistent notice says:

> Visual-only draft. Steps were inferred from visual changes because mouse and
> keyboard events were unavailable. Review before export.

All detector-created steps use `CandidateKind::UiChanged`,
`DetectReason::VisualChange`, and the default title `UI changed`. Users may
rename, recaption, delete, reorder, annotate, and replace keyframes with the
existing tools. A user edit may describe a Click, Typing, or Scroll action, but
Rollshot does not make that claim automatically.

If no meaningful visual settle is found, import still succeeds with one step
using the final sampled frame. Its title is `Imported recording`, its kind and
reason remain `UiChanged`/`VisualChange`, and the workspace shows a
`No visual changes detected` warning.

If more than 200 candidates occur, the importer reduces them deterministically
across the full duration and shows a persistent warning that intermediate
changes were omitted. The warning must not imply that every action was
captured.

The existing first-save prompt remains authoritative. Import does not silently
choose a project destination or add an unsaved scratch workspace to Recent
Projects.

## Architecture

```text
Action Guide Home
      │ local file + operation ID
      ▼
Shared app import coordinator ────────────────┐
      │ verified FFmpeg/FFprobe paths         │ progress / cancel / result
      ▼                                       │
rollshot-action video importer                │
      │                                       │
      ├─ Pass 1: 2 fps, 384 px analysis ──────┤
      │          existing luma metrics        │
      │          existing visual Detector     │
      │          bounded candidate selector   │
      │                                       │
      └─ Pass 2: selected sample indices ─────┘
                 max long edge 1920 px
                 center ± 1 sampled frame
                 disk-backed derived evidence
      │
      ▼
Imported workspace seed
      │ guide + scratch frame source + warnings + provenance
      ▼
Existing Timeline Workspace
      │
      ├─ Save / Save As / project publishing
      ├─ Action Guide exports
      └─ Issue Pack export + evidence review
```

The importer reuses the existing `Detector` rather than feeding decoded video
through the complete live `ActionRecorder`. `ActionRecorder` retains
full-resolution candidate windows in memory for the duration of a live
recording. That storage path is inappropriate for videos of unbounded duration.
The offline adapter therefore shares detection semantics but uses a two-pass,
disk-backed evidence path.

### Component responsibilities

#### `rollshot-action`: video importer

A new framework-neutral import module owns:

- probe result validation;
- the FFmpeg command contracts for both passes;
- decoding analysis frames and assigning source timestamps;
- driving the existing visual `Detector` without semantic input events;
- bounded candidate selection;
- selected-frame extraction and derived-frame indexing;
- progress events and structured warnings;
- cancellation checks, child termination, and exit-status validation; and
- construction of an imported workspace seed.

The module receives explicit executable and input paths. It does not resolve
tools, show UI, persist recent projects, or execute exports.

#### `rollshot-app`: shared import coordinator

The shared app layer owns the import operation state machine:

```text
Idle → Picking → Preflight → AnalyzingPass1 → ExtractingPass2
  ▲          cancel/failure ────────────────────────────────┘
  └────────────────────────────── Success → Timeline
```

It maps structured worker failures to user-facing messages, assigns operation
IDs, ignores stale events, and transfers scratch ownership to the timeline on
success. Linux and macOS product adapters expose this same coordinator from
their existing Action Guide home phases; platform-specific code is limited to
the product phase transition and native picker/task wiring already used by the
app.

#### managed FFmpeg

Keep the existing FFmpeg-only resolver for current export paths and add a video
import toolchain resolver containing FFmpeg and FFprobe. This avoids making an
unrelated video/GIF export fail merely because FFprobe is absent. The managed
manifest validates both paths from the same pinned distribution. For external
tools, the importer resolver accepts `ROLLSHOT_FFMPEG` and a new
`ROLLSHOT_FFPROBE` override; without overrides it resolves both tools on PATH.
If only one tool is valid, video import reports setup required while existing
FFmpeg-only exports retain their current behavior.

#### timeline workspace

The timeline gains a constructor for an `ImportedWorkspaceSeed` backed by a
`StepFrameSource` rooted in importer scratch storage. It does not fabricate a
live `Recording` or copy all decoded frames into `FrameStore`.

The seed contains:

- the generated `Guide`;
- the capture region `(0, 0, decoded_width, decoded_height)` after evidence
  scaling;
- an imported-video input source and visual-only capability;
- the derived frame index and disk-backed source;
- import warnings; and
- an RAII scratch owner.

On first save, existing project asset writing copies only referenced derived
frames into the destination. After the saved project has its own frame source,
the workspace releases importer scratch. Closing an unsaved workspace also
releases scratch.

## Two-Pass Import Algorithm

### Fixed v1 constants

These are product constants, not preferences:

| Constant | Value |
|---|---:|
| Analysis sampling rate | 2 fps |
| Analysis width | 384 px |
| Maximum generated steps | 200 |
| Evidence maximum long edge | 1920 px |
| Nearby evidence | center sample plus one sample before and after |

FFmpeg preserves aspect ratio and never upscales a source whose long edge is
already below 1920 px. Rotation metadata is applied so dimensions and pixels
match what users see in a normal player. Audio and subtitle streams are
explicitly disabled.

### Pass 1: visual detection

FFmpeg emits timestamped, rotation-corrected frames at 2 fps with an analysis
width of 384 px. The importer converts each frame to the existing luma analysis
shape and calls the existing `Detector::observe_frame`. It never calls
`observe_event`, so every candidate is visual-only.

Candidate storage is bounded as follows:

1. Retain every candidate while the count is at most 200.
2. When candidate 201 arrives, switch to reduced mode.
3. Reserve the first candidate and continually track the latest candidate.
4. Divide the probed duration into 198 equal time buckets. Retain the most
   recent candidate in each occupied bucket.
5. At completion, combine the first candidate, occupied buckets, and latest
   candidate; deduplicate by timestamp; sort chronologically; and renumber.

This uses constant memory, preserves beginning-to-end coverage, and is
deterministic for the same decoded frame stream. Reduced mode always produces
the `IntermediateChangesReduced` warning.

The importer also retains the final sampled timestamp for the zero-change
fallback. It does not retain analysis pixels after a frame has been processed.

### Pass 2: evidence extraction

For every selected candidate, the importer requests the candidate sample and
the immediately adjacent 2 fps samples when they exist. A single sequential
FFmpeg pass emits only those selected sample indices, rotation-corrected and
scaled to a maximum 1920 px long edge. Thus pass 2 produces at most 600 derived
frames regardless of source duration.

Derived frames are encoded as PNG assets in a unique importer scratch
directory. The center sample is the keyframe; adjacent samples form the
existing nearby replacement strip. Frame IDs and `at_ms` values are stable and
monotonic within the imported workspace.

If a selected evidence frame cannot be produced, import fails rather than
silently creating a step with missing evidence. If an adjacent frame falls
outside the source range, it is simply absent; the center frame is mandatory.

## Provenance and Project Compatibility

Imported guides must not masquerade as degraded live recordings. Add
`InputSourceKind::ImportedVideo` and a corresponding visual-only reason
`DegradedReason::ImportedRecording`. The names are serialized as
`imported-video` and `imported-recording`.

Because project manifests deny unknown fields and older readers cannot
understand the new enum values, project persistence advances to schema version
2. The loader continues to read version 1 and maps it without behavior changes;
new saves write version 2. Project schema version 2 also adds a bounded
`import_warnings` array containing only `no-visual-changes-detected` and/or
`intermediate-changes-reduced`; recorded projects write an empty array. The
workspace snapshot carries this array so the notice survives Save, Save As,
close, and reopen. No source path, source filename, codec name, original
dimensions, or other identifying video metadata is added to the manifest.

Action Guide session schema advances to version 2 as well. Its loader accepts
version 1 with empty import warnings, while new exports carry imported-video
source, visual-only capability, and the bounded warning array. The offline HTML
reader and `steps.md` display the warnings before the steps. Issue Pack
generation maps the same warnings into its manifest and adds a short notice
before reproduction steps in `issue.md`. This disclosure is required so an
agent never mistakes a reduced draft for a complete action history. No raw
video asset or new attachment type is added.

## Cancellation and Cleanup

Cancellation is available during preflight and both passes. The worker must:

1. observe the shared cancellation token;
2. terminate the active FFmpeg or FFprobe child;
3. wait for the child to exit;
4. close pipes and background readers;
5. remove the unique scratch directory; and
6. return a structured cancelled result.

Dropping the UI task alone is not cancellation. No decoder child may survive a
cancelled operation.

Each scratch directory holds an exclusive lock for its active lifetime. Normal
RAII cleanup removes it on cancel, failure, saved-project transfer, or unsaved
workspace close. At startup, Rollshot may delete importer scratch directories
whose lock can be acquired, covering process crashes without deleting another
live process's data. Scratch directories never appear in Recent Projects.

## Error Model

Worker errors use stable categories and privacy-safe messages:

- `probe_failed`
- `missing_video_stream`
- `invalid_video_metadata`
- `decoder_unavailable`
- `decode_failed`
- `evidence_missing`
- `scratch_io`
- `resource_limit`
- `cancelled`

The app may show the selected file's display name in transient UI, but tracing,
manifests, exports, and error categories must not contain the source path or
filename. Runtime diagnostics use stable `rollshot::action::video_import` and
`rollshot::app::video_import` targets with structured, privacy-safe fields such
as pass number, processed milliseconds, selected count, warning category, and
error category.

Picker cancellation stays silent. Setup-required delegates to the existing
managed FFmpeg flow. All other preflight or worker failures return to the home
with a dismissible, actionable message; the source file is never modified.

## Trust and Privacy Requirements

- All decoding and visual detection run locally.
- FFmpeg is invoked with audio and subtitle processing disabled.
- The original video is opened read-only and is never copied, modified,
  deleted, uploaded, included in scratch, or included in a project/export.
- Source path and filename are not logged or persisted.
- Imported provenance and reduction/no-change warnings survive save and reopen.
- Existing Action Guide keyframe review and Issue Pack evidence/redaction review
  remain required before their respective exports.
- A cancelled or failed import cannot appear successful and cannot leave a
  user-visible partial project.

## Testing Strategy

### `rollshot-action` unit and fixture tests

Generate local video fixtures with FFmpeg during tests; do not require network
access. Cover:

- a static recording producing the final-frame fallback and warning;
- one and multiple visual settle sequences with deterministic timestamps;
- rotation metadata and aspect-preserving evidence scaling;
- containers accepted by the picker and content-based stream validation;
- audio-bearing input proving no audio artifact or transcript is produced;
- exactly 200 and more than 200 candidates, including deterministic bucket
  reduction and beginning-to-end coverage;
- mandatory center-frame failure versus optional out-of-range adjacent frames;
- cancellation during probe, pass 1, and pass 2;
- decoder non-zero exit, broken pipe, and malformed progress output;
- child-process reap and scratch cleanup on every terminal outcome; and
- a long synthetic recording demonstrating bounded candidate memory and a
  maximum of 600 evidence frames independent of duration.

### project and export tests

- Version 1 projects still load unchanged.
- Imported projects save as schema version 2 and reopen with imported-video
  provenance, visual-only capability, warnings, steps, and derived frames.
- Save/Save As copies only referenced PNG evidence and releases importer
  scratch after the project frame source is active.
- Action Guide and Issue Pack exports include reviewed derived evidence and no
  original-video attachment or source identifier.
- Existing evidence-review and redaction gates still block unreviewed exports.

### app state tests

Test the shared coordinator for:

- picker cancellation;
- setup-required and setup retry;
- pass/progress transitions;
- cancellation and stale event rejection;
- recoverable failure returning to Home;
- static fallback and reduced-candidate warnings;
- successful transition into an unsaved timeline; and
- close/save ownership transfer for scratch evidence.

Both Linux and macOS product-path tests must expose **Import Recording…** and
drive the same coordinator lifecycle. Manual runtime verification covers the
native picker, progress rendering, cancellation, timeline transition, project
save, and Issue Pack export on both platforms.

### privacy diagnostics tests

Use sentinel paths, filenames, and pixel content to assert they do not appear
in tracing output, manifests, exported markdown/JSON, or user-independent error
strings. Assert that no audio or transcript file is created in scratch.

## Success Criteria

The feature is complete when:

1. A user on Linux or macOS can import a supported local recording from the
   Action Guide home.
2. Import provides bounded two-pass progress and can be cancelled without a
   surviving child process or scratch artifact.
3. The resulting timeline contains at most 200 honest visual-only steps with
   derived evidence covering the recording duration.
4. Static recordings still yield one reviewable final-frame step.
5. Saving and reopening preserves imported provenance and warnings without any
   dependency on the original video.
6. Existing Action Guide and Issue Pack exports work from the reviewed draft
   and never include the original video, source identifier, or audio.
7. Automated tests demonstrate deterministic detection/reduction, bounded
   retained evidence, schema compatibility, cleanup, cross-platform state
   behavior, and privacy-safe diagnostics.
