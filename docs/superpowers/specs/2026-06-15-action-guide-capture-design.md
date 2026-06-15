# Action Guide Capture Design

## Summary

Add a cross-platform Action Guide recording workflow that captures a short
desktop task, detects deterministic semantic steps, lets the user review those
steps in a dedicated timeline workspace, and exports a portable Markdown guide
with referenced keyframe images.

The P0 workflow is:

```text
rollshot action-guide
    -> choose a capture region
    -> record frames plus privacy-filtered semantic input events
    -> finish recording
    -> detect and merge candidate steps
    -> review in Action Guide Timeline Workspace
    -> export steps.md + keyframes + session.json
```

Action Guide is a new app-level workflow, not another scrolling-stitching mode.
It reuses Rollshot's existing platform capture and region-selection paths but
does not create a stitcher or overload the existing single-image Result
Workspace.

P0 supports Linux and macOS. Semantic input events improve detection when
available, but input monitoring is optional: if permission or platform input
capture fails, recording continues in visual-only mode with a persistent
warning.

## Goals

- Record a short selected desktop region as timestamped RGBA frames.
- Observe privacy-filtered semantic input events during the active recording.
- Generate a useful deterministic first draft without an LLM, OCR, or
  accessibility integration.
- Prefer event-backed candidates while still working in visual-only mode.
- Provide a dedicated timeline workspace for reviewing generated steps.
- Let users rename, delete, and replace a step's keyframe with a nearby frame.
- Export a self-contained folder with Markdown, PNG keyframes, and session
  metadata.
- Support Linux and macOS with explicit platform permission behavior.
- Keep frame capture independent from detector and export latency.

## Non-Goals

- Recording or exporting MP4, WebM, audio, or full-fidelity video.
- Adding, merging, or splitting steps manually in P0.
- Free-form Markdown editing inside Rollshot.
- GIF or HTML export.
- LLM, OCR, accessibility-tree, DOM, or window-title integration.
- Persisting raw keyboard events, typed text, device paths, or device names.
- A daemon, system tray, global shortcut, Polkit helper, or automatic Linux
  ACL setup.
- Input injection, macro playback, or macOS Accessibility/PostEvent permission.
- Making absolute click coordinates required for detection or export.
- Reusing the existing single-image Result Workspace for a multi-step guide.

## Product Behavior

### Starting A Recording

P0 adds a CLI command compiled behind an `action-guide` Cargo feature:

```text
rollshot action-guide
```

The CLI launches `rollshot-app` into an app-level Action Guide workflow. Action
Guide does not become a `CaptureMode` variant because Region, Scrolling, and
Fullscreen describe image-acquisition workflows, while Action Guide
coordinates capture, input observation, detection, review, and export.
Builds without the Cargo feature expose no new command or app launch intent.

The recording flow:

1. Open the existing platform-appropriate region-selection UI.
2. Let the user select the region to document.
3. Start the frame stream and semantic input source for the selected region.
4. Show compact recording controls with elapsed time, input capability, and
   `Finish` / `Cancel`.
5. On `Finish`, stop both sources and run deterministic detection.
6. Open the Action Guide Timeline Workspace with generated steps.

`Cancel` discards the temporary session and opens no workspace. P0 does not
define a global hotkey; recording controls remain the authoritative way to
finish or cancel.

### Recording State And Warning

The recording controls always show one input capability:

- `Semantic input enabled`
- `Visual-only detection`

Visual-only mode is degraded but valid. A persistent amber advisory explains
the platform-specific remedy:

- Linux: `Input events unavailable. Using visual-only step detection. See the
  README to grant temporary input-device access.`
- macOS: `Input Monitoring is unavailable. Using visual-only step detection.`
  Include `Open System Settings`.

The advisory is not a red fatal error. Recording, detection, review, and export
remain available.

### Action Guide Timeline Workspace

Finishing a recording opens a dedicated multi-step workspace:

```text
+------------------------------------------------------------------+
| warning/advisory (when degraded)          Discard   Export Guide |
+----------------------+-------------------------------------------+
| ordered step list    | selected keyframe                         |
| 1. title             |                                           |
| 2. title             | editable step title                       |
| 3. title             |                                           |
+----------------------+-------------------------------------------+
| nearby frames for replacing the selected step's keyframe         |
+------------------------------------------------------------------+
```

P0 workspace operations:

- Select a step.
- Edit its title.
- Delete it.
- Replace its keyframe by selecting one frame from the selected step's nearby
  frame strip.
- Discard the whole guide.
- Export the guide.

Replacing a keyframe does not open another window. Nearby frames appear in the
workspace's bottom strip. The strip contains only frames retained around that
step's candidate window, not a full-session scrubber.

Deleting a step immediately renumbers the remaining ordered list. Titles
default to deterministic labels such as `Click`, `Enter text`, `Scroll`, or
`UI changed`; users can replace them before export.

Markdown is generated at export time. The workspace does not display or edit a
Markdown document.

### Export

The user chooses an output directory. Rollshot creates one portable folder:

```text
action-guide/
├── steps.md
├── session.json
└── keyframes/
    ├── 001.png
    ├── 002.png
    └── 003.png
```

`steps.md` uses relative image references:

```markdown
# Action Guide

1. Open Preferences

   ![](keyframes/001.png)

2. Select Capture

   ![](keyframes/002.png)
```

Only currently selected guide keyframes are exported. Temporary nearby frames
and raw captured frames are deleted after successful export or discard.
`session.json` contains the final ordered steps, timestamps, detector reasons,
and input capability metadata, but no raw input events or sensitive device
metadata.

## Architecture

```text
rollshot-cli
    action-guide command
          |
          v
rollshot-app
    ActionGuideProduct / session lifecycle
          |
          +--> existing region selection + FrameStream
          |
          +--> rollshot-action::SemanticInputSource
          |        +--> rollshot-linux-input
          |        +--> rollshot-macos-input
          |        +--> visual-only source
          |
          v
rollshot-action
    temporary frame store
    analysis queue
    deterministic detector
    candidate merge rules
    guide model + exporter
          |
          v
rollshot-app
    Action Guide Timeline Workspace
```

### `rollshot-action`

Add a platform-neutral crate responsible for:

- Action session, frame, semantic event, candidate, and guide-step models.
- A bounded temporary full-resolution frame ring buffer and retained candidate
  windows.
- A bounded downsampled analysis queue.
- Privacy filtering and event burst aggregation.
- Cursor-masked luma difference and changed-area metrics.
- Deterministic candidate generation and merge/suppression rules.
- Nearby-frame selection for each retained candidate.
- Markdown, PNG keyframe, and `session.json` export.

It must not own windows, dialogs, platform permissions, or native event APIs.
It must be usable with fixture frames and fixture semantic events in tests.

### `rollshot-app`

`rollshot-app` owns the Action Guide product lifecycle:

- Route the app-level Action Guide launch intent.
- Reuse the active Linux and macOS region-selection paths.
- Start and stop frame and semantic-input sources together.
- Surface semantic-input capability and degraded warnings.
- Transition from recording to detection to timeline workspace.
- Own the timeline workspace and export-directory interaction.
- Delete temporary session data on discard or terminal failure.

This is separate from the existing Result Workspace because a guide owns an
ordered collection of images and metadata rather than one editable image.

### `rollshot-capture`

`rollshot-capture` remains responsible only for frame acquisition:

- Reuse existing platform capture backends and `FrameStream`.
- Supply timestamped RGBA frames for the selected region.
- Do not own semantic input events or Action Guide models.
- Do not start the scrolling stitcher for Action Guide.

The Action Guide consumer must copy or retain frames without blocking the
capture producer. When processing falls behind, the bounded analysis queue may
drop intermediate analysis work, while the temporary frame store retains the
frames required around known event/candidate windows.

### `rollshot-macos-input`

Add a narrow unsafe-isolation crate for macOS CoreGraphics input observation.
Its public API is safe and exposes:

- Input Monitoring permission status/request/open-settings operations.
- A session-scoped, listen-only input monitor.
- Semantic native events needed by `rollshot-action`.
- Explicit startup and runtime failure reasons.

This follows the existing `rollshot-macos-oneshot` unsafe-isolation precedent.
It does not expose raw CoreGraphics handles or input injection.

### `rollshot-linux-input`

Add a narrow Linux crate that owns evdev device discovery, read-only readers,
and native-event classification. Its public API exposes only semantic events
and explicit startup/runtime failure reasons. It does not expose device paths,
device names, or raw key events to consumers.

Keeping both platform sources outside `rollshot-action` preserves that crate's
platform-neutral boundary. CrossMacro is a GPLv3 learning reference only; the
Rollshot implementation must not copy its source.

## Core Model

The exact Rust representation may follow repository conventions, but the
cross-platform contract is:

```rust
pub enum SemanticAction {
    Click {
        button: MouseButton,
        position: Option<Point>,
    },
    ScrollActivity,
    TypingActivity,
    SemanticKey(SemanticKey),
}

pub enum SemanticKey {
    Enter,
    Tab,
}

pub enum InputSourceKind {
    LinuxEvdev,
    MacosCgEvent,
    VisualOnly,
}

pub enum InputCapability {
    SemanticEvents,
    VisualOnly { reason: DegradedReason },
}

pub struct ActionSession {
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub frames: Vec<FrameRef>,
    pub semantic_events: Vec<TimedSemanticAction>,
    pub candidates: Vec<CandidateStep>,
    pub guide_steps: Vec<GuideStep>,
}
```

`position` is optional. macOS may provide an absolute click position. Linux P0
does not depend on a compositor-specific pointer-position provider and
therefore normally emits `None`. Detector behavior and export validity must not
depend on a position being present.

Raw native events may exist transiently inside a platform source long enough
to classify them, but they must not enter `ActionSession`, diagnostics, or
`session.json`.

## Platform Input Sources

### Linux

Linux observes global input through read-only evdev access to
`/dev/input/event*`. This works under KDE Wayland because it reads kernel input
devices rather than relying on Wayland, XWayland, or a KWin input API.

P0 uses manual, temporary ACL setup documented in `README.md`. The guide must
include:

- How to identify the current user's relevant input devices.
- A `setfacl` command that grants the current user read access.
- How to verify access.
- How to remove the ACL after use.
- A clear warning that input-device read access can expose sensitive activity
  to any process running as that user.
- The fact that ACLs may disappear after reboot or device recreation.

Rollshot does not invoke `sudo`, `pkexec`, Polkit, or a privileged daemon.

The evdev source:

- Opens readable event devices only for the active Action Guide session.
- Classifies click, scroll, ordinary typing activity, and semantic keys.
- Does not persist raw key codes, device paths, or device names.
- Does not require absolute pointer coordinates.
- Drops to visual-only mode if no usable device can be opened or all readers
  fail.

### macOS

macOS uses a CoreGraphics event tap:

- `CGEventTapCreate` with `HIDEventTap`, `HeadInsertEventTap`, and
  `ListenOnly`.
- A dedicated background thread that owns the CoreFoundation run loop.
- An event mask for mouse buttons, scroll, key down/up, and modifier changes.
- No Unicode text extraction and no input modification.
- Re-enable the tap after `TapDisabledByTimeout`.
- Stop by disabling the tap, stopping the run loop, joining the thread, and
  releasing native objects.

Action Guide requires Screen Recording permission to obtain frames. Semantic
input additionally uses Input Monitoring / ListenEvent permission. These are
independent:

- Missing Screen Recording permission is a capture failure.
- Missing Input Monitoring permission is a visual-only degradation.

Request Input Monitoring just in time when Action Guide recording starts. Do
not request Accessibility or PostEvent permission. If permission remains
unavailable, offer `Open System Settings` and explain that macOS may require
restarting Rollshot before semantic input becomes available.

A null event tap or persistent runtime failure transitions the semantic input
source to visual-only while frame recording continues.

macOS may emit `position: Some(point)` for clicks. The P0 detector may use it
to focus local visual confirmation, but no required rule may assume it exists.

## Privacy And Security

Action Guide observes input only between recording start and stop. The privacy
boundary is semantic classification:

- Consecutive ordinary keyboard activity becomes one `TypingActivity`.
- Enter and Tab may be retained as semantic keys.
- Actual typed text is never read through Unicode APIs or persisted.
- General raw key codes, modifiers, device names, and device paths are not
  persisted.
- Diagnostics record capability, platform source category, degraded reason,
  counts, and lifecycle outcome only.
- Diagnostics never record key values, typed text, click coordinates, frame
  contents, or sensitive output paths.

Linux README instructions must explain the security consequences of evdev ACL
access. macOS UI must describe why Input Monitoring is requested and that
Action Guide remains usable without it.

## Frame Pipeline And Temporary Storage

The capture producer must never wait for analysis or export:

```text
FrameStream
    -> crop to the selected region
    -> bounded full-resolution frame ring buffer
    -> bounded downsampled analysis queue
    -> detector
```

The full-resolution ring buffer is continuously overwritten; Action Guide does
not retain the complete recording. When an event or visual candidate opens a
candidate window, the relevant before/after frames are copied from the ring
buffer into bounded temporary candidate storage. These frames are temporary
session assets, not a video export. The implementation retains enough frames
to:

- Select a stable keyframe after an event or visual candidate.
- Show a small nearby-frame replacement strip for every retained step.
- Export the user's chosen keyframes.

The analysis queue is latest-useful-work oriented and may drop redundant
intermediate analysis frames when overloaded. Candidate retention and analysis
backpressure policies must have explicit fixed bounds. Dropped analysis work
must not block or terminate capture.

Temporary session data is deleted on discard, cancellation, successful export,
or recoverable cleanup at the next app start after an abnormal termination.

## Deterministic Detection

P0 uses lightweight visual metrics plus semantic event timing:

- Downsampled, cursor-masked normalized luma difference.
- Changed-area ratio.
- Rolling baseline and cooldown/debounce.
- Semantic event bonuses.
- Stability checks after an event or visual peak.

P0 deliberately excludes SSIM/DSSIM, histogram delta, OCR, accessibility, and
LLM resolution.

Required rules:

- **Click:** create at most one candidate after a click when a stable visual
  change appears. A click with no confirming visual change does not
  automatically become a step.
- **Typing:** merge ordinary typing activity and associated visual changes
  until a pause of at least `700 ms`, Enter, Tab, or recording finish.
- **Scroll:** do not create steps while frames continue moving. Create at most
  one candidate after a stable dwell of `500-800 ms` if the settled viewport
  differs meaningfully from the pre-scroll state.
- **Drag:** collapse activity into start/end states and prefer the stable end
  state. P0 does not create intermediate drag steps.
- **Cursor motion:** never creates a step by itself.
- **Animation suppression:** blinking carets, loading spinners, and repeated
  localized oscillation without a stable state do not create steps.
- **Visual-only:** apply the same visual stability and suppression rules
  without semantic event bonuses.

For each retained candidate, choose the most stable post-action frame as the
default keyframe and retain a small bounded set of nearby frames before and
after it for replacement.

## Failure Handling

- Frame-source startup or Screen Recording permission failure is fatal because
  no guide can be recorded.
- Semantic-input startup or runtime failure is non-fatal and transitions to
  visual-only mode with an advisory.
- Analysis backlog drops redundant analysis work rather than blocking capture.
- Detector failure after recording preserves the temporary session long enough
  to show an actionable error or retry; it does not export a partial folder
  silently.
- Export writes to a temporary sibling directory and renames it into place
  only after Markdown, JSON, and all PNG files succeed.
- Export failure leaves the timeline workspace and temporary session intact so
  the user can retry or choose another directory.
- Cancellation or discard removes temporary session assets and produces no
  export.

Runtime diagnostics use stable explicit `rollshot::*` targets and structured,
privacy-safe fields.

## Testing

### `rollshot-action` Unit And Fixture Tests

- Semantic input classification never exposes typed text or raw key codes.
- Typing bursts merge on pause, Enter, Tab, and recording finish.
- Scroll candidates appear only after settle and meaningful visual change.
- Click candidates require stable visual confirmation.
- Cursor-only movement and repeated animation fixtures produce no steps.
- Visual-only fixtures produce deterministic guide steps.
- Nearby-frame selection is bounded and ordered.
- Rename, delete, and keyframe replacement update the final guide model.
- Markdown references exactly the exported keyframe filenames.
- `session.json` contains capability/degraded metadata and no raw input fields.

### Platform Input Tests

- Linux evdev classification maps fixture events into semantic actions without
  retaining device metadata.
- Linux no-device, permission-denied, and reader-failure paths become
  visual-only.
- macOS permission APIs distinguish ListenEvent from Accessibility/PostEvent.
- macOS event-tap callbacks classify events without Unicode text extraction.
- macOS tap timeout is re-enabled.
- macOS null-tap and runtime-failure paths become visual-only.

Native permission behavior requires manual platform verification because
automated tests cannot reliably manipulate Linux device ACLs or macOS TCC.

### App And Workspace Tests

- `rollshot action-guide` launches the Action Guide workflow rather than a
  scrolling or single-image result flow.
- Region selection uses the active Linux and macOS platform paths.
- Recording finish stops frame and input sources before detection.
- Visual-only advisory remains visible during recording and in the workspace.
- Selecting a nearby frame replaces the keyframe without opening another
  window.
- Delete renumbers steps; rename persists into export.
- Export failure preserves editable workspace state.
- Successful export and discard clean temporary assets.

### Manual Verification

- On KDE Wayland without ACL access, record and export a guide in visual-only
  mode and verify the persistent warning.
- Grant the documented temporary ACL, restart recording, and verify click,
  scroll, typing activity, Enter, and Tab improve candidate timing.
- Remove the ACL using the documented command.
- On macOS, verify Screen Recording denial remains fatal while Input Monitoring
  denial degrades to visual-only.
- On macOS, grant Input Monitoring, retry or restart as instructed, and verify
  semantic events are observed only during an active recording.
- On both platforms, record at least ten representative short workflows,
  replace nearby keyframes, and open exported Markdown with valid relative PNG
  links.

## Acceptance Criteria

- Linux and macOS can complete the full region-select, record, detect, review,
  and export workflow.
- The user can rename, delete, and replace nearby keyframes before export.
- Export creates one portable folder containing `steps.md`, `session.json`, and
  ordered `keyframes/*.png`.
- Markdown image links are relative and valid.
- Missing Linux evdev access or macOS Input Monitoring never prevents recording
  or export; it visibly enables visual-only mode.
- Missing macOS Screen Recording permission remains an explicit capture
  failure.
- No actual typed text, raw key codes, device names, or device paths are
  persisted or logged.
- Capture is never blocked by detection or export work.
- Deterministic detection and export work offline without an LLM or API key.
- At least ten representative short workflows can be recorded without
  crashing.
- Existing Region, Scrolling, Fullscreen, and Result Workspace behavior remains
  unchanged.

## Deferred Work

- Privileged Linux daemon, Polkit integration, persistent permission setup, and
  future system-tray/global-hotkey ownership.
- Full-session scrubber and manual Add Step.
- Merge and split editing.
- Free-form Markdown editing.
- GIF, HTML, MP4, and WebM export.
- OCR, accessibility trees, window/app metadata, LLM resolution, and automatic
  labels.
- Cross-platform absolute pointer-position support.
- Input injection or macro playback.
