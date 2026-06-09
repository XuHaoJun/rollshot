# Capture Result Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Rollshot's immediate save-dialog handoff with a Snow Shot-inspired capture workspace that supports coordinated toolbar/live-preview placement, activity auto-hide, Result Review, Save, Copy, and Close.

**Architecture:** Add a framework-neutral chrome-placement engine to `rollshot-overlay-core`, then split the iced overlay's workspace state, toolbar, result viewport, and output services into focused modules. Linux and macOS runners continue to own platform window/input-region effects, while the shared workspace owns phases and user actions. Save and clipboard output run before the overlay exits so cancellation or failure can return to Result Review.

**Tech Stack:** Rust, iced 0.14, iced_layershell 0.18, `image`, `rfd`, `arboard`, existing Rollshot capture/stitching crates.

---

## Engineering Review Addendum (2026-06-07)

Cross-cutting risks and shared diagrams surfaced during plan review. Resolve the
risks inside the tasks they reference; do not let them stay implicit.

### Workspace state machine

```
            valid release / Enter
 Selecting ───────────────────────▶ Selected ◀──────────────┐
     ▲                               │  │  │                 │ screenshot mode
     │ empty selection               │  │  │ Save/Copy       │ (stop+discard
     └───────────────────────────────┘  │  ▼                 │  scroll result)
                                         │  prepare screenshot│
                          scrolling mode │      ▼             │
                          [streaming     │   PerformOutput    │
                           driver +      ▼                    │
                           portal]   ScrollingCapture ────────┘
                                         │  │  │
                              Finish     │  │  │ Save/Copy (finalize first)
                                  ┌──────┘  │  ▼
                                  ▼         │  PerformOutput
                             ResultReview ◀─┘
                                  │ Save/Copy
                                  ▼
                             PerformOutput
                                  │
              ┌───────────────────┴───────────────────┐
        success │                                cancel / failure │
              ▼                                              ▼
            exit (workspace closes)                 back to ResultReview
                                                     (+ transient error)

 Esc / Cancel / Close  ─────────────────────────────────────▶ exit
```

### Chrome placement decision tree (Task 1)

```
place_chrome(viewport, crop, req{toolbar, preview?, margin, spacing}):
  outside bands = {Bottom, Top, Left, Right} rects around crop
  toolbar_band  = first of [Bottom, Top, Left, Right] whose band fits `toolbar`
  if no band fits toolbar ............................ ActivityAutoHide{over-crop}
  if req.preview is None ............................. Separate{toolbar_band, None}
  pref = largest-area band ≠ toolbar_band that fits preview
  if pref exists ..................................... Separate{toolbar_band, pref}
  elif toolbar_band fits BOTH without overlap ........ Separate (same band, stacked)
  elif exactly one band can host chrome .............. Combined{band}
  else ............................................... ActivityAutoHide
```

### Risks to resolve during execution

- **R-A: Save dialog inside the live iced loop (Task 5).** The current app runs
  `rfd` in a *separate helper process* (`rollshot-app/src/main.rs`) precisely
  because a synchronous native dialog conflicts with the iced/winit run loop
  (deadlock / no-show on macOS, frozen overlay everywhere). Do NOT lock a
  synchronous `OutputService::save_as`. Spike `rfd::AsyncFileDialog` driven by an
  `iced::Task` first; fall back to retaining the helper-process for *save* if the
  async path misbehaves on macOS.
- **R-B: On-demand scrolling acquisition re-runs the portal picker (Tasks 6/7).**
  `Driver::start_capture` runs the Wayland portal handshake and blocks up to 5s.
  Today capture starts *before* the overlay exists so the screen-share picker is
  never self-captured. Acquiring a streaming driver from the toolbar while the
  fullscreen overlay is up means the picker reappears over the overlay and the
  first frames may capture it. Accept and document this, or scope the switch as
  "re-runs portal selection." It must not be silent.
- **R-C: Subscription reactivity (Tasks 6/7).** `subscription()` gates the preview
  stream + tick on the static `CAPTURE_MODE`. Mid-session mode switches must move
  the mode into observable `WorkspaceState`, re-key the subscription off it, and
  register a fresh `PREVIEW_RX` on scrolling activation.

## File Structure

### Create

- `crates/rollshot-overlay-core/src/chrome_placement.rs`
  - Pure geometry for toolbar/live-preview placement and viewport clamping.
- `crates/rollshot-iced-overlay/src/workspace.rs`
  - Workspace phases, workflow state, activity auto-hide timing, and transitions.
- `crates/rollshot-iced-overlay/src/toolbar.rs`
  - Shared iced toolbar rendering and drag messages.
- `crates/rollshot-iced-overlay/src/result_review.rs`
  - Final-image viewport sizing and Result Review rendering.
- `crates/rollshot-iced-overlay/src/output.rs`
  - Save As dialog, PNG writing, and full-resolution clipboard output.

### Modify

- `crates/rollshot-overlay-core/src/lib.rs`
  - Export the placement module.
- `crates/rollshot-iced-overlay/Cargo.toml`
  - Add `rfd` and `arboard`; enable iced advanced widget support only if required
    by the drag implementation.
- `crates/rollshot-iced-overlay/src/lib.rs`
  - Register modules and keep the public capture contract.
- `crates/rollshot-iced-overlay/src/app.rs`
  - Delegate workspace state, toolbar, placement, Result Review, and output
    effects; remove the current single-stack chrome layout.
- `crates/rollshot-iced-overlay/src/driver.rs`
  - Emit accepted-stitch activity separately from preview handles.
- `crates/rollshot-iced-overlay/src/linux_runner.rs`
  - Acquire workflow resources on demand, execute workflow/output effects, and
    update layer-shell input regions from calculated visible toolbar bounds.
- `crates/rollshot-iced-overlay/src/macos_runner.rs`
  - Acquire workflow resources on demand, execute workflow/output effects, and
    keep only visible toolbar bounds clickable while capture is active.
- `crates/rollshot-app/src/main.rs`
  - Stop opening the unconditional post-overlay Save Dialog.
- `crates/rollshot-app/src/launch.rs`
  - Remove the internal `--save-dialog-temp` launch mode after its caller is
    deleted.
- `crates/rollshot-app/src/save.rs`
  - Delete after Save As and PNG output move into the overlay.
- `Cargo.lock`
  - Record dependency changes.

## Task 1: Add The Framework-Neutral Chrome Placement Engine

**Files:**
- Create: `crates/rollshot-overlay-core/src/chrome_placement.rs`
- Modify: `crates/rollshot-overlay-core/src/lib.rs`

- [ ] **Step 1: Write failing placement tests**

Add tests covering the required priority, independent preview placement,
combined fallback, no-space fallback, and clamping:

```rust
#[test]
fn toolbar_uses_bottom_top_left_right_priority() {
    let viewport = Rect::new(0.0, 0.0, 1000.0, 800.0);
    let toolbar = Size::new(260.0, 48.0);

    assert_eq!(
        place_chrome(viewport, Rect::new(250.0, 200.0, 500.0, 300.0), req(toolbar, None))
            .toolbar_band(),
        Some(Band::Bottom)
    );
    assert_eq!(
        place_chrome(viewport, Rect::new(250.0, 550.0, 500.0, 230.0), req(toolbar, None))
            .toolbar_band(),
        Some(Band::Top)
    );
    assert_eq!(
        place_chrome(viewport, Rect::new(300.0, 10.0, 680.0, 780.0), req(toolbar, None))
            .toolbar_band(),
        Some(Band::Left)
    );
    assert_eq!(
        place_chrome(viewport, Rect::new(10.0, 10.0, 680.0, 780.0), req(toolbar, None))
            .toolbar_band(),
        Some(Band::Right)
    );
}

#[test]
fn preview_uses_different_largest_band_when_available() {
    let placement = place_chrome(
        Rect::new(0.0, 0.0, 1200.0, 900.0),
        Rect::new(250.0, 180.0, 600.0, 500.0),
        req(Size::new(280.0, 48.0), Some(Size::new(240.0, 320.0))),
    );
    assert_eq!(placement.toolbar_band(), Some(Band::Bottom));
    assert_eq!(placement.preview_band(), Some(Band::Right));
    assert!(!placement.toolbar_rect().intersects(placement.preview_rect().unwrap()));
}

#[test]
fn one_band_combines_without_duplicating_toolbar() {
    let placement = place_chrome(
        Rect::new(0.0, 0.0, 1000.0, 800.0),
        Rect::new(0.0, 0.0, 720.0, 800.0),
        req(Size::new(240.0, 48.0), Some(Size::new(240.0, 500.0))),
    );
    assert!(matches!(placement, ChromePlacement::Combined { band: Band::Right, .. }));
}

#[test]
fn no_outside_space_uses_activity_auto_hide() {
    let placement = place_chrome(
        Rect::new(0.0, 0.0, 1000.0, 800.0),
        Rect::new(0.0, 0.0, 1000.0, 800.0),
        req(Size::new(260.0, 48.0), Some(Size::new(240.0, 500.0))),
    );
    assert!(matches!(placement, ChromePlacement::ActivityAutoHide { .. }));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-overlay-core chrome_placement
```

Expected: FAIL because `chrome_placement` and its types do not exist.

- [ ] **Step 3: Implement the placement data model and algorithm**

Implement a pure module with no iced dependency:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Bottom,
    Top,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeRequirements {
    pub toolbar: Size,
    pub preview: Option<Size>,
    pub margin: f32,
    pub spacing: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChromePlacement {
    Separate {
        toolbar_band: Band,
        toolbar: Rect,
        preview_band: Option<Band>,
        preview: Option<Rect>,
    },
    Combined {
        band: Band,
        toolbar: Rect,
        preview: Rect,
    },
    ActivityAutoHide {
        overlay_toolbar: Rect,
        overlay_preview: Option<Rect>,
    },
}
```

Implement `place_chrome`, `clamp_rect`, band rectangles, complete-fit checks,
the fixed toolbar priority, largest-different-band preview selection, and
combined layout. Keep all placement policy in this module.

- [ ] **Step 4: Export the module and run its tests**

Add:

```rust
pub mod chrome_placement;
```

Run:

```bash
rtk cargo test -p rollshot-overlay-core chrome_placement
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-overlay-core/src/chrome_placement.rs crates/rollshot-overlay-core/src/lib.rs
rtk git commit -m "feat(overlay): add coordinated chrome placement"
```

## Task 2: Introduce Explicit Workspace Phases And Activity Auto-Hide

**Files:**
- Create: `crates/rollshot-iced-overlay/src/workspace.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`

- [ ] **Step 1: Write failing workspace transition tests**

Create tests that use deterministic `Instant` values:

```rust
#[test]
fn screenshot_release_enters_selected_instead_of_finishing() {
    let mut state = WorkspaceState::new(CaptureMode::Screenshot);
    state.set_crop(valid_crop());
    assert_eq!(state.complete_selection(), WorkspaceEffect::None);
    assert_eq!(state.phase(), WorkspacePhase::Selected);
}

#[test]
fn accepted_activity_hides_no_space_chrome_until_idle_deadline() {
    let now = Instant::now();
    let mut visibility = ActivityAutoHide::new(Duration::from_millis(500));
    visibility.accepted_frame(now);
    assert!(!visibility.visible(now + Duration::from_millis(499)));
    assert!(visibility.visible(now + Duration::from_millis(500)));
}

#[test]
fn toolbar_interaction_keeps_auto_hide_visible() {
    let now = Instant::now();
    let mut visibility = ActivityAutoHide::new(Duration::from_millis(500));
    visibility.accepted_frame(now);
    visibility.set_interacting(true);
    assert!(visibility.visible(now));
}

#[test]
fn switching_modes_requests_new_workflow_resources() {
    let mut state = WorkspaceState::new(CaptureMode::Screenshot);
    state.set_crop(valid_crop());
    state.complete_selection();
    assert_eq!(
        state.activate_mode(CaptureMode::Scrolling),
        WorkspaceEffect::ActivateMode(CaptureMode::Scrolling)
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
rtk cargo test -p rollshot-iced-overlay workspace
```

Expected: FAIL because the workspace module does not exist.

- [ ] **Step 3: Implement workspace phases and effects**

Define:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePhase {
    Selecting,
    Selected,
    ScrollingCapture,
    ResultReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAction {
    Save,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEffect {
    None,
    ActivateMode(CaptureMode),
    StartScrolling,
    StopScrolling { discard: bool },
    FinalizeScrolling { output: Option<OutputAction> },
    PrepareScreenshot { output: Option<OutputAction> },
    PerformOutput(OutputAction),
    Cancel,
}
```

Add `ToolbarPosition::{Automatic, Manual(Rectangle)}` and
`ActivityAutoHide`. Make selection changes clear manual placement. Keep image
pixels out of this module; it owns state transitions, not capture resources.

Relationship to `OverlayState` (review S4): embed `WorkspaceState` inside the
existing `OverlayState` rather than replacing it. `OverlayState` keeps the
render-side fields (`crop`, `preview`/handles, `frozen`, `window_size`);
`WorkspaceState` owns phase, active `CaptureMode`, `ToolbarPosition`,
`ChromePlacement`, and `ActivityAutoHide`. The active mode must live here (not in
the runner's `CAPTURE_MODE` static) so the subscription can re-key off it on a
mid-session switch (review R-C).

- [ ] **Step 4: Replace `crop_confirmed` decisions with workspace phase checks**

Update `OverlayState`, messages, and `update` so:

- valid mouse release enters `Selected`;
- mode toolbar actions request `ActivateMode` so the runner can acquire the
  required resource on demand;
- scrolling begins only after the runner has activated scrolling mode;
- `Finish` in `ScrollingCapture` requests finalize into Result Review;
- `Esc` cancels;
- accepted activity updates auto-hide state;
- Result Review never restarts capture implicitly.

Do not render the new toolbar yet. Preserve a temporary minimal view so the
crate remains buildable.

- [ ] **Step 5: Run focused tests**

```bash
rtk cargo test -p rollshot-iced-overlay workspace
rtk cargo test -p rollshot-iced-overlay app::tests
rtk cargo fmt --check
```

Expected: PASS, with old immediate-finish tests updated to assert `Selected`.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/workspace.rs crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/app.rs
rtk git commit -m "feat(overlay): add capture workspace phases"
```

## Task 3: Emit Accepted Stitch Activity Independently From Preview

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`

- [ ] **Step 1: Write failing driver event tests**

Add:

```rust
#[test]
fn accepted_signal_emits_activity_even_when_preview_is_unavailable() {
    assert_eq!(
        live_events_for_signal(StitchProgressSignal::Accepted {
            edge: CapturedEdge::Bottom
        }, false),
        vec![LiveEventKind::AcceptedActivity]
    );
}

#[test]
fn missed_signal_does_not_emit_accepted_activity() {
    // NOTE: `StitchProgressSignal::Missed` carries `{ edge: CapturedEdge }`
    // (see rollshot-overlay-core/src/capture_miss.rs) — there is no `reason`
    // field and no `NoMatchReason::NoReliableCandidate` variant. Match the real
    // shape so this compiles.
    assert!(!should_emit_accepted_activity(&StitchProgressSignal::Missed {
        edge: CapturedEdge::Unknown
    }));
}
```

- [ ] **Step 2: Run the focused test to verify it fails**

```bash
rtk cargo test -p rollshot-iced-overlay accepted_activity
```

Expected: FAIL because the activity event does not exist.

- [ ] **Step 3: Add the event and emit it before preview generation**

Extend:

```rust
pub enum LiveOverlayEvent {
    AcceptedActivity(Instant),
    Preview(ImageHandle),
    CaptureMiss(CaptureMissState),
}
```

For every accepted stitch signal, send `AcceptedActivity(Instant::now())`
regardless of whether preview generation returns a handle. Handle the event in
`app::update` by updating `ActivityAutoHide`.

- [ ] **Step 4: Run tests**

```bash
rtk cargo test -p rollshot-iced-overlay driver
rtk cargo test -p rollshot-iced-overlay workspace
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/driver.rs crates/rollshot-iced-overlay/src/app.rs
rtk git commit -m "feat(overlay): report accepted stitch activity"
```

## Task 4: Render The Draggable Toolbar And Coordinated Live Preview

**Files:**
- Create: `crates/rollshot-iced-overlay/src/toolbar.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`

- [ ] **Step 1: Write failing toolbar-state tests**

Cover action availability and dragging:

```rust
#[test]
fn scrolling_toolbar_includes_finish_but_selected_toolbar_does_not() {
    assert!(!actions_for(WorkspacePhase::Selected).contains(&ToolbarAction::Finish));
    assert!(actions_for(WorkspacePhase::ScrollingCapture).contains(&ToolbarAction::Finish));
}

#[test]
fn result_review_toolbar_only_contains_output_and_close_actions() {
    assert_eq!(
        actions_for(WorkspacePhase::ResultReview),
        vec![ToolbarAction::Save, ToolbarAction::Copy, ToolbarAction::Close]
    );
}

#[test]
fn drag_is_clamped_and_marks_position_manual() {
    let position = finish_drag(
        Rectangle::new(Point::new(990.0, 790.0), Size::new(260.0, 48.0)),
        Rectangle::new(Point::ORIGIN, Size::new(1000.0, 800.0)),
    );
    assert_eq!(position, ToolbarPosition::Manual(Rectangle::new(
        Point::new(740.0, 752.0),
        Size::new(260.0, 48.0),
    )));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
rtk cargo test -p rollshot-iced-overlay toolbar
```

Expected: FAIL because the toolbar module does not exist.

- [ ] **Step 3: Implement toolbar actions and rendering**

Create a compact icon-button toolbar with tooltips:

```rust
pub enum ToolbarAction {
    ScreenshotMode,
    ScrollingMode,
    Finish,
    Save,
    Copy,
    Cancel,
    Close,
}
```

Render only actions valid for the current phase. Include a drag handle and
active mode styling. Use iced mouse events to emit drag start/move/end messages;
do not add a general-purpose drag abstraction.

- [ ] **Step 4: Replace the current single chrome stack**

**Behavior change:** the toolbar is NO LONGER stacked above the live preview by
default. The old single-column layout (`column![toolbar, …, preview]` placed in
one band, `app.rs:487-497`) always put the toolbar above the preview. After this
task they are placed independently — toolbar and preview go to different bands
whenever space permits. Toolbar-above-preview survives ONLY as the constrained
`Combined` fallback (when a single band must host both), per the spec's combined
orientation rules.

In `app::view`:

- measure known toolbar/preview requirement constants;
- call `rollshot_overlay_core::chrome_placement::place_chrome`;
- render toolbar and preview in their separate or combined rectangles;
- render activity-auto-hide chrome over the crop only when visible;
- render capture-miss messages as non-reserving floating messages;
- remove only what `app::view` owns: `choose_chrome_band`, `place_outside_crop`,
  and the magenta toolbar.

Do NOT delete `toolbar_input_rect` or `capture_chrome_input_rect` in this task:
`linux_runner.rs` (`capture_chrome_input_rect`) and `macos_runner.rs`
(`toolbar_input_rect`) still call them, so removing them here would break the
crate build. Their call sites are replaced in Tasks 6 and 7; delete the
functions there once unused.

Use one toolbar widget instance per view. Combined layout must not duplicate it.

- [ ] **Step 5: Run tests**

```bash
rtk cargo test -p rollshot-iced-overlay toolbar
rtk cargo test -p rollshot-iced-overlay app::tests
rtk cargo test -p rollshot-overlay-core chrome_placement
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/Cargo.toml crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/toolbar.rs crates/rollshot-iced-overlay/src/app.rs Cargo.lock
rtk git commit -m "feat(overlay): add coordinated capture toolbar"
```

## Task 5: Add Result Review And Full-Resolution Output Services

**Files:**
- Create: `crates/rollshot-iced-overlay/src/result_review.rs`
- Create: `crates/rollshot-iced-overlay/src/output.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing Result Review sizing tests**

```rust
#[test]
fn vertical_result_fits_width_and_scrolls_vertically() {
    let layout = review_layout(Size::new(800.0, 3000.0), Size::new(600.0, 500.0));
    assert_eq!(layout.axis, ReviewScrollAxis::Vertical);
    assert_eq!(layout.rendered.width, 600.0);
    assert!(layout.rendered.height > 500.0);
}

#[test]
fn horizontal_result_fits_height_and_scrolls_horizontally() {
    let layout = review_layout(Size::new(3000.0, 800.0), Size::new(600.0, 500.0));
    assert_eq!(layout.axis, ReviewScrollAxis::Horizontal);
    assert_eq!(layout.rendered.height, 500.0);
    assert!(layout.rendered.width > 600.0);
}
```

- [ ] **Step 2: Write failing output service tests**

Use injectable services rather than opening native UI in tests:

```rust
#[test]
fn cancelled_save_keeps_result_review() {
    let mut output = FakeOutput::save_cancelled();
    assert_eq!(
        perform_output(&mut output, OutputAction::Save, &image()),
        OutputOutcome::Cancelled
    );
}

#[test]
fn copy_receives_full_resolution_rgba() {
    let mut output = FakeOutput::default();
    perform_output(&mut output, OutputAction::Copy, &image());
    assert_eq!(output.copied_dimensions, Some((1200, 2400)));
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
rtk cargo test -p rollshot-iced-overlay result_review
rtk cargo test -p rollshot-iced-overlay output
```

Expected: FAIL because the modules do not exist.

- [ ] **Step 4: Implement Result Review**

Create `ReviewLayout` and render the final image inside an iced `scrollable`.
Normal images aspect-fit initially. Vertical long images fit width; horizontal
long images fit height. Keep the original `RgbaImage` in overlay state and use
an iced image handle only for rendering.

Memory note (review P1): the full-res `RgbaImage` plus its iced `Handle` is two
copies of a potentially large image (a long scroll can be ~100 MB). Build the
`Handle` once when entering Result Review and reuse the cheap clone per redraw —
do not rebuild it each `view()`. No hard cap in v1 (zoom/minimap are deferred).

- [ ] **Step 5: Implement Save and Copy services**

Add:

```rust
pub trait OutputService {
    fn save_as(&mut self, image: &RgbaImage) -> Result<SaveOutcome, String>;
    fn copy(&mut self, image: &RgbaImage) -> Result<(), String>;
}

pub enum SaveOutcome {
    Saved(PathBuf),
    Cancelled,
}
```

Production `copy` uses `arboard::Clipboard::set_image` with full-resolution RGBA
bytes. Keep errors as strings for transient overlay messages.

**Caveat (review R-P2):** `arboard` is a new workspace dependency. On Wayland it
forks a `wl-clipboard` server that retains ownership after exit (good). On X11
the clipboard is cleared when the process exits unless a clipboard manager runs;
since the overlay exits immediately after a successful Copy, X11 sessions may see
an empty clipboard. Rollshot is Wayland-first on Linux, so this is acceptable for
v1 — note it in the Task 9 manual checklist.

**Save must not block the iced loop (review R-A — do this before writing
`save_as`).** The current app runs `rfd` in a *separate helper process*
(`rollshot-app/src/main.rs`) specifically because a synchronous native Save
dialog conflicts with a running iced/winit event loop (on macOS the modal can
fail to present or deadlock; everywhere it freezes the overlay). The synchronous
`save_as` signature above is therefore the production-unsafe path.

Spike first, then pick one:

1. **Preferred:** `rfd::AsyncFileDialog` returned as a future and driven by an
   `iced::Task` in the runner, so the loop stays live. The trait then exposes the
   future (or the runner owns the async call directly) rather than a blocking
   `save_as`. The PNG write still happens via the synchronous `write_png` helper
   once a path resolves.
2. **Fallback:** retain the helper-process for *save* on macOS only, keeping the
   blocking `save_as` for Linux where it is known to work post-loop.

Keep the *tested* surface (`perform_output` + `FakeOutput`) synchronous and pure;
only the production dialog invocation needs the async/`Task` treatment. Record the
chosen approach in the implementation report.

- [ ] **Step 6: Connect output outcomes to workspace transitions**

Make the outcome→phase decision a pure `WorkspaceState`/`output` function that
returns a `WorkspaceEffect` (review S3): the platform runner stays a thin
translator and the transitions below are unit-tested here rather than only
manually in Tasks 6/7. The riskiest spec rules (cancel/fail stay in Result
Review) must have automated coverage.

- `Finish` finalizes into Result Review.
- `Finish` with no usable stitched result (`finalize` returns `Err`, e.g.
  "stitcher produced no output") stays in Scrolling Capture and shows a transient
  error — it must NOT exit (spec lines 419-420).
- `Save`/`Copy` during Scrolling Capture finalize first, then output.
- `Save`/`Copy` during Selected prepare the normal screenshot first, then
  output.
- Save cancellation enters or remains in Result Review.
- Output failure enters or remains in Result Review and shows a transient error.
- Successful output exits.

Add tests asserting each transition via the pure decision function with a
`FakeOutput` (saved / cancelled / failed) and a fake finalize result
(some-image / empty):

```rust
#[test]
fn save_cancel_returns_to_result_review() { /* OutputOutcome::Cancelled -> ResultReview */ }
#[test]
fn output_failure_stays_in_result_review_with_error() { /* Err -> ResultReview + transient */ }
#[test]
fn successful_output_exits() { /* Saved/Copied -> exit effect */ }
#[test]
fn finish_with_empty_stitch_stays_in_scrolling_capture() { /* Err -> ScrollingCapture + error */ }
```

- [ ] **Step 7: Run tests**

```bash
rtk cargo test -p rollshot-iced-overlay result_review
rtk cargo test -p rollshot-iced-overlay output
rtk cargo test -p rollshot-iced-overlay workspace
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/Cargo.toml crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/app.rs crates/rollshot-iced-overlay/src/result_review.rs crates/rollshot-iced-overlay/src/output.rs Cargo.lock
rtk git commit -m "feat(overlay): add result review and output actions"
```

## Task 6: Update Linux Runner Effects And Input Regions

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`

- [ ] **Step 1: Write failing Linux runner helper tests**

Extract pure input-region decisions and test them:

```rust
#[test]
fn scrolling_input_region_only_contains_visible_toolbar() {
    let region = input_region_for(&workspace_with_visible_toolbar(rect(10, 20, 260, 48)));
    assert_eq!(region, Some(rect(10, 20, 260, 48)));
}

#[test]
fn hidden_auto_hide_toolbar_has_no_input_region() {
    assert_eq!(input_region_for(&workspace_with_hidden_auto_hide()), None);
}

#[test]
fn result_review_accepts_input_across_crop_and_toolbar() {
    assert_eq!(input_mode_for(WorkspacePhase::ResultReview), InputMode::FullOverlay);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner::tests
```

Expected: FAIL because the helpers do not exist.

- [ ] **Step 3: Execute workspace effects without exiting prematurely**

Update Linux effect handling:

- refactor startup-only `acquire_resource` into reusable activation helpers;
- when activating scrolling, stop/discard the current workflow, acquire a fresh
  streaming driver through the existing factory, then begin stitching for the
  existing crop;
  - **Portal re-handshake (review R-B):** `Driver::start_capture` runs the
    Wayland portal picker and blocks up to 5s. Acquiring it from the toolbar
    while the overlay is already up means the screen-share picker reappears over
    the overlay and early frames may capture it. This task accepts that
    behavior; document it as a known limitation in the implementation report.
    Do not silently swallow it.
  - **Subscription re-key (review R-C):** `subscription()` gates the preview
    stream + tick on the active mode. Drive it from `WorkspaceState`'s mode (not
    only the `CAPTURE_MODE` static) and register a fresh `PREVIEW_RX` on
    scrolling activation so the live preview reconnects after a switch.
- when activating screenshot, cancel the streaming driver, acquire a fresh
  one-shot capture through the existing factory, rebuild the frozen image
  handle, and keep the existing crop;
- surface activation failure as a transient workspace error without exiting;
- start scrolling on `StartScrolling`;
- finalize and store the image in overlay state on `FinalizeScrolling`;
- crop the one-shot image and store it on `PrepareScreenshot`;
- execute output and exit only on successful Save/Copy or Cancel/Close;
- restore full overlay interaction in Result Review;
- recalculate input region after placement or auto-hide visibility changes;
- **during an active toolbar drag (review P3):** widen the layer-shell input
  region to the full overlay so pointer move/release events keep arriving as the
  toolbar leaves its resting rect; re-clamp to the visible-toolbar rect on
  release. A per-frame input region matching only the resting rect would drop the
  drag mid-gesture.

Also delete `app::capture_chrome_input_rect` here once this runner no longer
calls it (deferred from Task 4, review S1).

Do not write `RESULT_SLOT` for intermediate Result Review transitions.

- [ ] **Step 4: Run Linux tests**

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner::tests
rtk cargo test -p rollshot-iced-overlay
rtk cargo fmt --check
```

Expected: PASS on Linux.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/linux_runner.rs crates/rollshot-iced-overlay/src/app.rs
rtk git commit -m "feat(overlay): integrate Linux capture workspace"
```

## Task 7: Update macOS Runner Effects And Clickable Toolbar Window

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/macos_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_window.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`

- [ ] **Step 1: Write failing macOS runner helper tests**

Test platform-independent window decisions under `cfg(test)`:

```rust
#[test]
fn controls_window_tracks_visible_toolbar_rect() {
    assert_eq!(
        controls_window_action(None, Some(rect(20, 30, 260, 48))),
        ControlsWindowAction::Open(rect(20, 30, 260, 48))
    );
}

#[test]
fn controls_window_closes_while_auto_hide_is_hidden() {
    assert_eq!(
        controls_window_action(Some(rect(20, 30, 260, 48)), None),
        ControlsWindowAction::Close
    );
}

#[test]
fn result_review_disables_passthrough() {
    assert_eq!(
        passthrough_action(WorkspacePhase::ScrollingCapture, WorkspacePhase::ResultReview),
        PassthroughAction::Disable
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

On macOS:

```bash
rtk cargo test -p rollshot-iced-overlay macos_runner::tests
```

Expected: FAIL because the helpers do not exist.

- [ ] **Step 3: Execute shared workspace effects**

Mirror Linux's phase/effect behavior. Keep overlay mouse passthrough active
during scrolling, but open/move/close the controls window to exactly match the
currently visible toolbar rectangle. Disable passthrough and close the controls
window before Result Review.

Use the existing macOS resource factories for on-demand activation:

- scrolling activation acquires a new `Driver`;
- screenshot activation acquires a fresh one-shot ScreenCaptureKit image and
  rebuilds the frozen handle;
- activation errors remain in the workspace and do not exit the app.

Apply the same subscription re-key as Linux (review R-C): drive the preview
stream off `WorkspaceState`'s mode and register a fresh `PREVIEW_RX` on scrolling
activation. The ScreenCaptureKit permission prompt is the macOS analogue of R-B —
if a fresh `Driver` triggers it mid-session, document the behavior rather than
hiding it.

Delete `app::toolbar_input_rect` here once this runner no longer calls it
(deferred from Task 4, review S1).

- [ ] **Step 4: Verify macOS compilation and tests**

On macOS:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo fmt --check
```

Expected: PASS.

On Linux, verify the macOS edits do not affect the active target:

```bash
rtk cargo test -p rollshot-iced-overlay
```

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/macos_runner.rs crates/rollshot-iced-overlay/src/macos_window.rs crates/rollshot-iced-overlay/src/app.rs
rtk git commit -m "feat(overlay): integrate macOS capture workspace"
```

## Task 8: Remove The Unconditional App-Level Save Handoff

**Files:**
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/launch.rs`
- Delete: `crates/rollshot-app/src/save.rs`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing app behavior tests**

Extract the post-overlay decision into a testable helper:

```rust
#[test]
fn completed_overlay_does_not_open_another_save_dialog() {
    assert_eq!(post_overlay_action(Ok(Some(capture_result()))), PostOverlayAction::ExitSuccess);
}

#[test]
fn cancelled_overlay_exits_successfully() {
    assert_eq!(post_overlay_action(Ok(None)), PostOverlayAction::ExitCancelled);
}

#[test]
fn save_dialog_temp_mode_is_no_longer_accepted() {
    assert!(parse_launch_args(["rollshot-app", "--save-dialog-temp", "/tmp/a.png"])
        .is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
rtk cargo test -p rollshot-app
```

Expected: FAIL because `post_overlay_action` does not exist and current behavior
opens the save helper.

- [ ] **Step 3: Simplify app completion**

Remove `handle_capture_result`, `save_result_via_helper`, `SaveDialogTemp`, and
the internal `--save-dialog-temp` path. Delete `save.rs`. After `run_overlay`,
print only completion/cancellation status; Save and Copy have already been
handled inside the workspace.

Remove `rfd` and `image` from `rollshot-app` when no remaining code uses them.

- [ ] **Step 4: Run app tests**

```bash
rtk cargo test -p rollshot-app
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add -A crates/rollshot-app Cargo.lock
rtk git commit -m "refactor(app): let capture workspace own output"
```

## Task 9: Cross-Platform Verification And Documentation Alignment

**Files:**
- Modify only if behavior documentation is now inaccurate:
  - `README.md`

- [ ] **Step 1: Run focused workspace tests**

```bash
rtk cargo test -p rollshot-overlay-core
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-app
```

Expected: PASS.

- [ ] **Step 2: Run workspace-wide Rust verification**

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Perform Linux runtime verification**

Verify:

1. Normal selection enters Selected rather than Save Dialog.
2. Toolbar automatic order is bottom, top, left, right.
3. Toolbar dragging is clamped and selection changes reset it.
4. Scrolling preview avoids the toolbar when another band fits.
5. One-band layout contains one toolbar and one preview.
6. Full-screen scrolling hides chrome during accepted activity and reveals it
   after `500ms` idle.
7. Finish enters Result Review.
8. Save cancellation remains in Result Review (and the Save dialog did not freeze
   the overlay — review R-A).
9. Copy writes a full-resolution image. On X11, confirm the clipboard survives
   overlay exit or note the manager dependency (review P2); Wayland is the
   primary path.
10. Close exits without output.
11. Switching to scrolling mode mid-session: confirm the portal/screen-share
    picker behavior and whether early frames capture it (review R-B); record the
    observed behavior.

- [ ] **Step 4: Perform macOS runtime verification**

Repeat the Linux checklist and additionally verify:

1. Target scrolling still works while overlay passthrough is enabled.
2. Only the visible toolbar area is clickable during scrolling.
3. Result Review disables passthrough and accepts image scrolling/toolbar drag.

If macOS runtime verification cannot be performed in the current environment,
record that exact residual risk in the final implementation report.

- [ ] **Step 5: Update README only if current instructions are wrong**

Document that capture now remains in a workspace and Save/Copy are explicit
toolbar actions. Do not document deferred editor features.

- [ ] **Step 6: Commit documentation changes if any**

```bash
rtk git add README.md
rtk git commit -m "docs: describe capture result workspace"
```

Skip this commit when README does not need changes.
