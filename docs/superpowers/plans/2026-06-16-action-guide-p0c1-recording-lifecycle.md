# Action Guide P0c-1 — Recording Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the capture overlay, a user picks Action Guide (🎬), selects a region, presses Start Recording, performs a workflow, presses Finish, and the app runs deterministic detection and writes an `action-guide/` Markdown folder — on both Linux and macOS.

**Architecture:** Add `Workflow::ActionGuide` as a third toolbar-switchable workflow. A new `WorkspacePhase::Recording` drives recording controls. The overlay's `Driver` gains an action-recording consumer thread that owns a `rollshot_action::ActionRecorder` + `ActionInputSession`, teeing each `CapturedFrame` into the recorder and polling semantic input; `finalize_action()` returns a `Recording`. The overlay returns the `Recording` to `rollshot-app`, which builds a `Guide` and calls `export_guide(...)` to a default output dir. Review editing is deferred to P0c-2.

**Tech Stack:** Rust, iced 0.14, `rollshot-capture`, `rollshot-iced-overlay`, `rollshot-action`, `rollshot-app`. Feature-gated behind the existing `action-guide` Cargo feature.

**Authoritative references:** `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md` (product behavior) and `docs/superpowers/specs/2026-06-16-action-guide-p0c-delta.md` (post-refactor wiring decisions). On any conflict about *wiring*, the delta wins; about *UX*, the original spec wins.

> **Both-platform rule (AGENTS.md §8):** every overlay/UI change here must be applied to BOTH the Linux runner (`linux_runner.rs`) and the macOS component (`macos_capture.rs`). Tasks call this out explicitly. Shared logic lives in `app.rs`/`workspace.rs`/`toolbar.rs` and is exercised by both.

---

## File Structure

**Modified:**
- `crates/rollshot-capture/src/types.rs` — `Workflow::ActionGuide`, `is_supported`, `CaptureRequest::action_guide_region`.
- `crates/rollshot-iced-overlay/Cargo.toml` — feature-gated `rollshot-action` dep + `action-guide` feature.
- `crates/rollshot-iced-overlay/src/workspace.rs` — `WorkspacePhase::Recording`, `WorkspaceEffect::{StartRecording, FinishRecording}`, workspace transitions.
- `crates/rollshot-iced-overlay/src/toolbar.rs` — `ToolbarAction::ActionGuide`, label/tooltip, `actions_for` per phase, recording-controls rendering.
- `crates/rollshot-iced-overlay/src/app.rs` — `OverlayEffect::{StartRecording, FinishRecording}`, elapsed-time + capability state, dispatch.
- `crates/rollshot-iced-overlay/src/driver.rs` — `begin_action_recording`, action consumer thread, `finalize_action`.
- `crates/rollshot-iced-overlay/src/linux_runner.rs` — action result slot, effect handling, `run_action_guide`.
- `crates/rollshot-iced-overlay/src/macos_capture.rs` — action-recording mode, `HostEffect::ActionRecorded`.
- `crates/rollshot-iced-overlay/src/lib.rs` — `run_action_guide` entry, re-exports.
- `crates/rollshot-iced-overlay/src/fullscreen.rs` — reject `ActionGuide`.
- `crates/rollshot-app/src/launch.rs` — `LaunchMode::ActionGuide` launch flag.
- `crates/rollshot-app/src/main.rs` — route `ActionGuide` to record→export handler; replace probe.
- `crates/rollshot-app/src/macos_product.rs` — daemon handles action recording + export handler.
- `crates/rollshot-cli/src/cmd_action_guide.rs` — launch app in record mode (not probe).

**Created:**
- `crates/rollshot-app/src/action_export.rs` — thin `Recording -> Guide -> export_guide` handler with default output dir (P0c-2 replaces this with the Timeline Workspace).

---

## Task 1: Add `Workflow::ActionGuide` variant

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs:43-96`
- Test: same file (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/rollshot-capture/src/types.rs`:

```rust
#[test]
fn action_guide_region_is_supported_and_needs_overlay() {
    let r = CaptureRequest::action_guide_region();
    assert_eq!(r.workflow, Workflow::ActionGuide);
    assert_eq!(r.scope, CaptureScope::Region);
    assert!(r.is_supported());
    assert!(r.needs_overlay());
}

#[test]
fn action_guide_fullscreen_is_unsupported() {
    let r = CaptureRequest {
        workflow: Workflow::ActionGuide,
        scope: CaptureScope::Fullscreen,
    };
    assert!(!r.is_supported());
}

#[test]
fn action_guide_serde_roundtrip_kebab() {
    let json = serde_json::to_string(&Workflow::ActionGuide).unwrap();
    assert_eq!(json, "\"action-guide\"");
    let back: Workflow = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Workflow::ActionGuide);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-capture action_guide`
Expected: FAIL — `no variant named ActionGuide`, `no function action_guide_region`.

- [ ] **Step 3: Implement the variant and helpers**

In `crates/rollshot-capture/src/types.rs`, add the variant to `Workflow` (after `Scrolling`):

```rust
pub enum Workflow {
    Screenshot,
    #[default]
    Scrolling,
    ActionGuide,
}
```

Add the constructor inside `impl CaptureRequest` (after `scrolling_region`):

```rust
    pub const fn action_guide_region() -> Self {
        Self {
            workflow: Workflow::ActionGuide,
            scope: CaptureScope::Region,
        }
    }
```

Extend `is_supported()` to reject `ActionGuide × Fullscreen`:

```rust
    pub fn is_supported(&self) -> bool {
        !matches!(
            (self.workflow, self.scope),
            (Workflow::Scrolling, CaptureScope::Fullscreen)
                | (Workflow::ActionGuide, CaptureScope::Fullscreen)
        )
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-capture action_guide`
Expected: PASS (3 tests).

- [ ] **Step 5: Fix downstream exhaustive matches in `rollshot-capture`**

Run: `rtk cargo build -p rollshot-capture`
Fix any non-exhaustive `match self.workflow` the compiler reports (none expected outside `types.rs`, but verify). Do not touch the overlay crate yet.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-capture/src/types.rs
git commit -m "feat(capture): add Workflow::ActionGuide region workflow"
```

---

## Task 2: Add feature-gated `rollshot-action` dependency to the overlay crate

**Files:**
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`

- [ ] **Step 1: Add the optional dependency and feature**

In `crates/rollshot-iced-overlay/Cargo.toml`, under `[dependencies]`:

```toml
rollshot-action = { path = "../rollshot-action", optional = true }
```

Add (or extend) `[features]`:

```toml
[features]
action-guide = ["dep:rollshot-action"]
```

- [ ] **Step 2: Verify both builds compile**

Run: `rtk cargo build -p rollshot-iced-overlay`
Expected: PASS (feature off, no `rollshot-action` linked).

Run: `rtk cargo build -p rollshot-iced-overlay --features action-guide`
Expected: PASS (feature on, `rollshot-action` linked, currently unused — allow the warning for now; it is consumed in Task 6).

- [ ] **Step 3: Commit**

```bash
git add crates/rollshot-iced-overlay/Cargo.toml
git commit -m "build(overlay): add feature-gated rollshot-action dependency"
```

---

## Task 3: Add `WorkspacePhase::Recording` and recording transitions

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/workspace.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/rollshot-iced-overlay/src/workspace.rs`:

```rust
#[test]
fn activate_action_guide_enters_selecting_when_no_crop() {
    let mut state = WorkspaceState::new(Workflow::Screenshot);
    let effect = state.activate_workflow(Workflow::ActionGuide);
    assert_eq!(effect, WorkspaceEffect::ActivateWorkflow(Workflow::ActionGuide));
    assert_eq!(state.phase(), WorkspacePhase::Selecting);
    assert_eq!(state.active_workflow(), Workflow::ActionGuide);
}

#[test]
fn begin_recording_moves_to_recording_phase() {
    let mut state = WorkspaceState::new(Workflow::ActionGuide);
    state.set_crop(Some(CropRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }));
    state.complete_selection();
    assert_eq!(state.phase(), WorkspacePhase::Selected);
    let effect = state.begin_recording();
    assert_eq!(effect, WorkspaceEffect::StartRecording);
    assert_eq!(state.phase(), WorkspacePhase::Recording);
}

#[test]
fn finish_recording_returns_finish_effect() {
    let mut state = WorkspaceState::new(Workflow::ActionGuide);
    state.set_crop(Some(CropRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }));
    state.complete_selection();
    state.begin_recording();
    let effect = state.finish_recording();
    assert_eq!(effect, WorkspaceEffect::FinishRecording);
}
```

> If `CropRect` field names differ, match the real struct in `workspace.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide workspace`
Expected: FAIL — `no variant Recording`, `no method begin_recording`.

- [ ] **Step 3: Implement the phase, effects, and methods**

In `crates/rollshot-iced-overlay/src/workspace.rs`:

Add to `WorkspacePhase`:

```rust
pub enum WorkspacePhase {
    Selecting,
    Selected,
    ScrollingCapture,
    Recording,
}
```

Add to `WorkspaceEffect`:

```rust
pub enum WorkspaceEffect {
    None,
    ActivateWorkflow(Workflow),
    StartScrolling,
    FinishScrolling,
    FinishRegion,
    StartRecording,
    FinishRecording,
    Cancel,
}
```

Add methods on `WorkspaceState` (next to `begin_scrolling`/`finish_scrolling`):

```rust
    pub fn begin_recording(&mut self) -> WorkspaceEffect {
        self.phase = WorkspacePhase::Recording;
        self.auto_hide.accepted_frame();
        WorkspaceEffect::StartRecording
    }

    pub fn finish_recording(&mut self) -> WorkspaceEffect {
        WorkspaceEffect::FinishRecording
    }
```

`activate_workflow` already sets phase from `crop_valid` and returns
`ActivateWorkflow(workflow)`; it needs no change for `ActionGuide`. `cancel()`
already resets to `Selecting`; it covers the Recording-phase cancel too.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-iced-overlay/src/workspace.rs
git commit -m "feat(overlay): add Recording workspace phase and transitions"
```

---

## Task 4: Add `ToolbarAction::ActionGuide` and recording-phase actions

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/toolbar.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/rollshot-iced-overlay/src/toolbar.rs`:

```rust
#[test]
fn selecting_offers_action_guide_entry() {
    let actions = actions_for(WorkspacePhase::Selecting);
    assert!(actions.contains(&ToolbarAction::ActionGuide));
    assert!(actions.contains(&ToolbarAction::RegionMode));
    assert!(actions.contains(&ToolbarAction::ScrollingMode));
}

#[test]
fn recording_phase_shows_only_finish_and_cancel() {
    assert_eq!(
        actions_for(WorkspacePhase::Recording),
        vec![ToolbarAction::Finish, ToolbarAction::Cancel]
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide toolbar`
Expected: FAIL — `no variant ActionGuide`, missing `Recording` arm.

- [ ] **Step 3: Implement the variant, labels, and phase actions**

In `crates/rollshot-iced-overlay/src/toolbar.rs`:

Add to `ToolbarAction`:

```rust
pub enum ToolbarAction {
    RegionMode,
    ScrollingMode,
    ActionGuide,
    Finish,
    Cancel,
}
```

Add the `Recording` arm and the `ActionGuide` entry in `actions_for`:

```rust
pub fn actions_for(phase: WorkspacePhase) -> Vec<ToolbarAction> {
    match phase {
        WorkspacePhase::Selecting => vec![
            ToolbarAction::RegionMode,
            ToolbarAction::ScrollingMode,
            ToolbarAction::ActionGuide,
            ToolbarAction::Cancel,
        ],
        WorkspacePhase::Selected => vec![
            ToolbarAction::RegionMode,
            ToolbarAction::ScrollingMode,
            ToolbarAction::ActionGuide,
            ToolbarAction::Finish,
            ToolbarAction::Cancel,
        ],
        WorkspacePhase::ScrollingCapture => vec![
            ToolbarAction::RegionMode,
            ToolbarAction::ScrollingMode,
            ToolbarAction::Finish,
            ToolbarAction::Cancel,
        ],
        WorkspacePhase::Recording => vec![ToolbarAction::Finish, ToolbarAction::Cancel],
    }
}
```

Add label and tooltip arms:

```rust
// in action_label:
        ToolbarAction::ActionGuide => "🎬",
// in action_tooltip:
        ToolbarAction::ActionGuide => "Action Guide",
```

Extend `action_style_fn`'s `is_active` match so the 🎬 button highlights when the
active workflow is `ActionGuide`:

```rust
    let is_active = matches!(
        (action, active_workflow),
        (ToolbarAction::RegionMode, Workflow::Screenshot)
            | (ToolbarAction::ScrollingMode, Workflow::Scrolling)
            | (ToolbarAction::ActionGuide, Workflow::ActionGuide)
    );
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide toolbar`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-iced-overlay/src/toolbar.rs
git commit -m "feat(overlay): add Action Guide toolbar entry and recording actions"
```

---

## Task 5: Wire overlay effects, dispatch, elapsed time, and capability label

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Test: same file (`#[cfg(test)] mod tests` — `app::update` is unit-testable via `OverlayState`)

This task adds the shared (both-platform) state machine. The platform runners
consume the new effects in Tasks 7–8.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/rollshot-iced-overlay/src/app.rs` (follow the existing pattern that builds an `OverlayState` and calls `update`):

```rust
#[test]
fn action_guide_toolbar_activates_action_guide_workflow() {
    let mut state = test_state(); // existing helper that builds a default OverlayState
    let (effect, _) = update(
        &mut state,
        OverlayMessage::ToolbarAction(crate::toolbar::ToolbarAction::ActionGuide),
    );
    assert_eq!(state.workflow, Workflow::ActionGuide);
    assert_eq!(effect, OverlayEffect::ActivateWorkflow(Workflow::ActionGuide));
}

#[test]
fn start_recording_emits_start_recording_effect() {
    let mut state = test_state();
    state.workflow = Workflow::ActionGuide;
    state.workspace.activate_workflow(Workflow::ActionGuide);
    state.workspace.set_crop(Some(crate::workspace::CropRect {
        x: 0.0, y: 0.0, width: 10.0, height: 10.0,
    }));
    state.workspace.complete_selection();
    let (effect, _) = update(
        &mut state,
        OverlayMessage::ToolbarAction(crate::toolbar::ToolbarAction::Finish),
    );
    assert_eq!(effect, OverlayEffect::StartRecording);
    assert_eq!(state.workspace.phase(), crate::workspace::WorkspacePhase::Recording);
}

#[test]
fn finish_recording_emits_finish_recording_effect() {
    let mut state = test_state();
    state.workflow = Workflow::ActionGuide;
    state.workspace.activate_workflow(Workflow::ActionGuide);
    state.workspace.set_crop(Some(crate::workspace::CropRect {
        x: 0.0, y: 0.0, width: 10.0, height: 10.0,
    }));
    state.workspace.complete_selection();
    state.workspace.begin_recording();
    let (effect, _) = update(
        &mut state,
        OverlayMessage::ToolbarAction(crate::toolbar::ToolbarAction::Finish),
    );
    assert_eq!(effect, OverlayEffect::FinishRecording);
}
```

> If no `test_state()` helper exists, add a small one in the test module that
> constructs `OverlayState` the same way existing app tests do.

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide app::tests`
Expected: FAIL — `no variant StartRecording/FinishRecording`, dispatch missing.

- [ ] **Step 3: Add the effects and dispatch**

In `crates/rollshot-iced-overlay/src/app.rs`, add to `OverlayEffect`:

```rust
pub(crate) enum OverlayEffect {
    None,
    BeginStitch,
    FinishScrolling,
    FinishRegion,
    StartRecording,
    FinishRecording,
    Cancel,
    EnablePassthrough,
    DisablePassthrough,
    ActivateWorkflow(Workflow),
}
```

In the `OverlayMessage::ToolbarAction` match, add the `ActionGuide` arm
(alongside `RegionMode`/`ScrollingMode`):

```rust
    crate::toolbar::ToolbarAction::ActionGuide => {
        state.workflow = Workflow::ActionGuide;
        state.workspace.activate_workflow(Workflow::ActionGuide);
        clear_capture_miss_ui(state);
        (
            OverlayEffect::ActivateWorkflow(Workflow::ActionGuide),
            InputRegionMode::None,
        )
    }
```

Extend the `ToolbarAction::Finish` match to handle the Action Guide phases.
`Selected` + `ActionGuide` starts recording; `Recording` finishes it:

```rust
    crate::toolbar::ToolbarAction::Finish => match state.workspace.phase() {
        WorkspacePhase::ScrollingCapture => {
            state.workspace.finish_scrolling();
            (OverlayEffect::FinishScrolling, InputRegionMode::None)
        }
        WorkspacePhase::Selected if state.workflow == Workflow::Screenshot => {
            state.workspace.finish_region();
            (OverlayEffect::FinishRegion, InputRegionMode::None)
        }
        WorkspacePhase::Selected if state.workflow == Workflow::ActionGuide => {
            state.workspace.begin_recording();
            (OverlayEffect::StartRecording, InputRegionMode::None)
        }
        WorkspacePhase::Recording => {
            state.workspace.finish_recording();
            (OverlayEffect::FinishRecording, InputRegionMode::None)
        }
        _ => (OverlayEffect::None, InputRegionMode::None),
    },
```

In the `ButtonReleased` region-completion arm (~app.rs:649-678), the current
`match state.workflow` is exhaustive over `Screenshot`/`Scrolling`. Add an
`ActionGuide` arm that completes selection into `Selected` (waiting for the
explicit `Start Recording` press) without auto-finishing:

```rust
        let effect = match state.workflow {
            Workflow::Screenshot => {
                state.workspace.finish_region();
                OverlayEffect::FinishRegion
            }
            Workflow::Scrolling => {
                state.workspace.begin_scrolling();
                OverlayEffect::BeginStitch
            }
            Workflow::ActionGuide => OverlayEffect::None, // stay in Selected; Start Recording confirms
        };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide app::tests`
Expected: PASS.

- [ ] **Step 5: Add elapsed-time + capability label state**

Add fields to `OverlayState` (near `workflow`):

```rust
    pub(crate) recording_started: Option<std::time::Instant>,
    pub(crate) recording_capability: Option<rollshot_capture::InputCapabilityLabel>,
```

> `recording_capability` is a small display-only enum. Define it in
> `rollshot-capture/src/types.rs` as `pub enum InputCapabilityLabel { Semantic,
> VisualOnly }` (feature-independent, so the overlay can render it without the
> `action-guide` feature gating the field type). The platform runner sets it from
> the `ActionInputSession::start` result in Task 7/8.

Initialize both to `None` in `OverlayState` construction. On `StartRecording`
the platform runner sets `recording_started = Some(Instant::now())`; the
existing `OverlayMessage::Tick` handler already fires periodically during
capture — extend its guard so ticks also re-render during
`WorkspacePhase::Recording`.

- [ ] **Step 6: Render recording controls**

In the toolbar render call sites (`app.rs:393-399` and `536-542`), when
`state.workspace.phase() == WorkspacePhase::Recording`, render a recording
indicator row: a `●` indicator, the elapsed `mm:ss` derived from
`recording_started`, the capability label text (`Semantic input enabled` /
`Visual-only detection`), and the toolbar `Finish`/`Cancel` buttons. Add a
helper `pub(crate) fn elapsed_label(started: Option<Instant>) -> String` with a
unit test:

```rust
#[test]
fn elapsed_label_formats_mm_ss() {
    // construct with a known elapsed via a small seam if needed; otherwise
    // assert the None case:
    assert_eq!(elapsed_label(None), "00:00");
}
```

Below the controls, when `recording_capability == VisualOnly`, render the
persistent amber advisory text from the original spec (Linux vs macOS variants).

- [ ] **Step 7: Run all overlay tests**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/rollshot-iced-overlay/src/app.rs crates/rollshot-capture/src/types.rs
git commit -m "feat(overlay): recording effects, elapsed-time and capability controls"
```

---

## Task 6: Driver action-recording consumer thread

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Test: same file (`#[cfg(test)] mod tests`, feature-gated)

The action consumer thread owns the `ActionRecorder` and `ActionInputSession`.
Because `ActionInputSession` lives in `rollshot-app`, the overlay needs the
input source as a trait object. To keep the dependency direction clean, the
overlay accepts a boxed `rollshot_action::SemanticInputSource` from the caller
(the app passes `create_input_source()`); the overlay drives its lifecycle
inline (start/poll/stop) rather than depending on `rollshot-app`.

- [ ] **Step 1: Write the failing test**

Add a feature-gated test in `crates/rollshot-iced-overlay/src/driver.rs`:

```rust
#[cfg(all(test, feature = "action-guide"))]
mod action_tests {
    use super::*;
    use rollshot_action::{CaptureRegion, VisualOnlySource};
    use image::RgbaImage;

    #[test]
    fn finalize_action_produces_candidates_from_changing_frames() {
        let region = CaptureRegion { x: 0, y: 0, width: 64, height: 64 };
        let mut rec = ActionRecording::start(region, Box::new(VisualOnlySource::default()));
        // Push two visually different frames far enough apart in time.
        let mut a = RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]));
        let b = RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 255]));
        rec.push_frame(a.clone(), 0);
        rec.push_frame(b, 500);
        // a second black frame to settle:
        a = RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]));
        rec.push_frame(a, 1500);
        let recording = rec.finalize();
        assert!(!recording.candidates.is_empty(), "expected at least one candidate");
    }
}
```

> `ActionRecording` is a small test-and-prod-shared synchronous core extracted
> so the thread wrapper stays thin and the detection logic is unit-testable
> without real threads. `push_frame(image, at_ms)` and `finalize()` map directly
> to `ActionRecorder::ingest_frame` / `finish`. Tune the frame deltas if the
> default `DetectorConfig` needs a larger change to fire.

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide action_tests`
Expected: FAIL — `ActionRecording` undefined.

- [ ] **Step 3: Implement the synchronous core and the threaded wrapper**

In `crates/rollshot-iced-overlay/src/driver.rs`, add (feature-gated):

```rust
#[cfg(feature = "action-guide")]
pub(crate) struct ActionRecording {
    recorder: rollshot_action::ActionRecorder,
    source: Box<dyn rollshot_action::SemanticInputSource>,
    session_started: Option<std::time::SystemTime>,
}

#[cfg(feature = "action-guide")]
impl ActionRecording {
    pub(crate) fn start(
        region: rollshot_action::CaptureRegion,
        mut source: Box<dyn rollshot_action::SemanticInputSource>,
    ) -> Self {
        use rollshot_action::{DetectorConfig, StoreConfig};
        let _capability = source.start(region);
        Self {
            recorder: rollshot_action::ActionRecorder::new(
                region,
                StoreConfig::default(),
                DetectorConfig::default(),
            ),
            source,
            session_started: None,
        }
    }

    /// `at_ms` is session-relative milliseconds (monotonic from 0).
    pub(crate) fn push_frame(&mut self, image: image::RgbaImage, at_ms: u64) {
        self.recorder.ingest_frame(image, at_ms);
    }

    pub(crate) fn poll_input(&mut self) {
        for ev in self.source.poll() {
            self.recorder.ingest_event(ev);
        }
    }

    pub(crate) fn finalize(mut self) -> rollshot_action::Recording {
        self.source.stop();
        self.recorder.finish()
    }
}
```

> Confirm `SemanticInputSource::poll` returns `Vec<TimedSemanticAction>` and
> `start(region) -> InputCapability` / `stop()` exist (see
> `rollshot-action/src/input.rs`). If the trait’s poll API differs, mirror
> exactly what `action_input.rs::poll_into` does.

Add the threaded driver entry points (feature-gated) modeled on
`begin_stitch`/`finalize`:

```rust
#[cfg(feature = "action-guide")]
impl Driver {
    /// Spawn the action consumer thread: tee each new captured frame into the
    /// recorder (converting SystemTime -> session-relative ms) and poll input.
    pub(crate) fn begin_action_recording(
        &mut self,
        region: rollshot_action::CaptureRegion,
        source: Box<dyn rollshot_action::SemanticInputSource>,
    ) {
        let shared = self.shared.clone();
        let stop = self.action_stop.clone(); // Arc<AtomicBool>, add to Driver
        let (tx, rx) = std::sync::mpsc::channel();
        self.action_result = Some(rx);
        self.action_thread = Some(std::thread::spawn(move || {
            let mut rec = ActionRecording::start(region, source);
            let mut last_seq = shared.seq.load(Ordering::Relaxed);
            let mut t0: Option<std::time::SystemTime> = None;
            while !stop.load(Ordering::Relaxed) {
                let seq = shared.seq.load(Ordering::Relaxed);
                if seq != last_seq {
                    last_seq = seq;
                    if let Some(frame) = shared.latest.lock().ok().and_then(|s| s.clone()) {
                        let base = *t0.get_or_insert(frame.timestamp);
                        let at_ms = frame
                            .timestamp
                            .duration_since(base)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        let cropped = match crop_frame(&frame, region_to_capture_region(region)) {
                            Ok(c) => c.image,
                            Err(_) => frame.image,
                        };
                        rec.push_frame(cropped, at_ms);
                    }
                }
                rec.poll_input();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let _ = tx.send(rec.finalize());
        }));
    }

    /// Signal the action thread to stop and collect the finished Recording.
    pub(crate) fn finalize_action(mut self) -> Result<rollshot_action::Recording, String> {
        self.action_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.action_thread.take() {
            let _ = handle.join();
        }
        self.action_result
            .take()
            .and_then(|rx| rx.recv().ok())
            .ok_or_else(|| "action recording produced no result".to_string())
    }
}
```

> Add the fields `action_stop: Arc<AtomicBool>`, `action_thread:
> Option<JoinHandle<()>>`, `action_result: Option<Receiver<Recording>>` to
> `Driver`, all feature-gated, initialized in `start_capture`. Reuse the existing
> `crop_frame` helper; `region_to_capture_region` is a tiny adapter between the
> overlay’s region rect type and `rollshot_action::CaptureRegion` (define it
> locally). If the crop region for Action Guide is the full selected region,
> pass that rect.

- [ ] **Step 4: Run the test to verify it passes**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide action_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-iced-overlay/src/driver.rs
git commit -m "feat(overlay): driver action-recording consumer thread"
```

---

## Task 7: Linux handoff — `run_action_guide` returning a `Recording`

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Test: `crates/rollshot-iced-overlay/src/linux_runner.rs` (effect-mapping unit test)

- [ ] **Step 1: Write the failing test**

In `linux_runner.rs` tests, assert the new effects are handled (mirroring
existing effect tests; if the runner’s `update` is not directly unit-testable,
add a focused test on the effect→slot mapping helper you introduce):

```rust
#[cfg(all(test, feature = "action-guide"))]
#[test]
fn start_recording_effect_begins_action_recording() {
    // Build the runner state with an armed DRIVER_SLOT (fake driver), drive
    // OverlayEffect::StartRecording through update(), assert ACTION_RESULT_SLOT
    // is armed / driver.begin_action_recording was invoked.
    // Follow the existing BeginStitch test pattern in this file.
}
```

> If the existing tests stub the driver, extend that stub with
> `begin_action_recording`/`finalize_action` no-ops. Keep the test shape
> identical to the current `BeginStitch`/`FinishScrolling` tests.

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide --lib linux_runner`
Expected: FAIL — effects unhandled.

- [ ] **Step 3: Add the action result slot and entry point**

In `linux_runner.rs`, add a static slot beside `RESULT_SLOT` (feature-gated):

```rust
#[cfg(feature = "action-guide")]
static ACTION_RESULT_SLOT: Mutex<Option<Result<Option<rollshot_action::Recording>, String>>> =
    Mutex::new(None);
```

Handle the new effects in the runner’s `update` match (feature-gated arms):

```rust
    #[cfg(feature = "action-guide")]
    app::OverlayEffect::StartRecording => {
        if let Some(driver) = DRIVER_SLOT.lock().unwrap().as_mut() {
            let region = action_region_from(state); // selected crop -> CaptureRegion
            let source = take_action_input_source();  // set before run; see Step 4
            driver.begin_action_recording(region, source);
        }
        Task::none()
    }
    #[cfg(feature = "action-guide")]
    app::OverlayEffect::FinishRecording => {
        let outcome = match DRIVER_SLOT.lock().unwrap().take() {
            Some(driver) => driver.finalize_action().map(Some),
            None => Err("no driver for action recording".to_string()),
        };
        *ACTION_RESULT_SLOT.lock().unwrap() = Some(outcome);
        iced::exit()
    }
```

> `action_region_from` converts the workspace’s selected crop (logical) into
> `rollshot_action::CaptureRegion` (physical px). Reuse the same crop/scale math
> the `FinishRegion`/`BeginStitch` arms already use. `take_action_input_source`
> reads a `static ACTION_INPUT_SLOT: Mutex<Option<Box<dyn SemanticInputSource>>>`
> the caller fills before launching (Step 4).

`Cancel` already takes/cancels the driver; add a feature-gated branch so a
Recording-phase cancel also drops the action thread (`driver.cancel()` or set
`action_stop` and join without sending). Ensure no Recording is left in the slot
on cancel.

- [ ] **Step 4: Add the `run_action_guide` entry point**

In `linux_runner.rs`:

```rust
#[cfg(feature = "action-guide")]
pub fn run_action_guide(
    config: OverlayConfig,
    input_source: Box<dyn rollshot_action::SemanticInputSource>,
) -> Result<Option<rollshot_action::Recording>, OverlayError> {
    *ACTION_INPUT_SLOT.lock().unwrap() = Some(input_source);
    run(config)?; // same overlay loop; ActionGuide workflow drives recording
    ACTION_RESULT_SLOT
        .lock()
        .unwrap()
        .take()
        .unwrap_or(Ok(None))
        .map_err(OverlayError::Capture)
}
```

In `lib.rs`, expose it (feature-gated, Linux):

```rust
#[cfg(all(target_os = "linux", feature = "action-guide"))]
pub fn run_action_guide(
    config: OverlayConfig,
    input_source: Box<dyn rollshot_action::SemanticInputSource>,
) -> Result<Option<rollshot_action::Recording>, OverlayError> {
    linux_runner::run_action_guide(config, input_source)
}
```

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-iced-overlay/src/linux_runner.rs crates/rollshot-iced-overlay/src/lib.rs
git commit -m "feat(overlay): linux run_action_guide returning a Recording"
```

---

## Task 8: macOS handoff — action recording in the capture Component

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-iced-overlay/src/macos_capture.rs`

> macOS counterpart of Task 7 — required by AGENTS.md §8. The Component already
> owns its `Driver` directly (`self.driver`) and reports `HostEffect`.

- [ ] **Step 1: Write the failing test**

In `macos_capture.rs` tests, mirror the existing `apply_effect` tests:

```rust
#[cfg(all(test, feature = "action-guide"))]
#[test]
fn finish_recording_effect_yields_action_recorded_host_effect() {
    // Build a Component with a fake driver in action-recording mode, apply
    // OverlayEffect::FinishRecording, assert EffectOutcome::Terminal(
    //   HostEffect::ActionRecorded(_)).
    // Follow the existing FinishScrolling test pattern.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide --lib macos_capture`
Expected: FAIL — `HostEffect::ActionRecorded` undefined.

- [ ] **Step 3: Add the HostEffect variant and effect handling**

In `macos_capture.rs`, extend `HostEffect` (feature-gated variant):

```rust
pub enum HostEffect {
    None,
    Task(iced::Task<Message>),
    Completed(CaptureResult),
    #[cfg(feature = "action-guide")]
    ActionRecorded(rollshot_action::Recording),
    Cancelled,
    Fatal(String),
}
```

Add to `apply_effect`:

```rust
    #[cfg(feature = "action-guide")]
    OverlayEffect::StartRecording => {
        if let Some(driver) = self.driver.as_mut() {
            let region = self.action_region();
            let source = self.action_input_source.take()
                .unwrap_or_else(|| Box::new(rollshot_action::VisualOnlySource::default()));
            driver.begin_action_recording(region, source);
        }
        EffectOutcome::Task(Task::none())
    }
    #[cfg(feature = "action-guide")]
    OverlayEffect::FinishRecording => {
        match self.driver.take() {
            Some(driver) => match driver.finalize_action() {
                Ok(recording) => EffectOutcome::Terminal(HostEffect::ActionRecorded(recording)),
                Err(e) => { self.overlay.transient_error = Some(e); EffectOutcome::Task(Task::none()) }
            },
            None => EffectOutcome::Terminal(HostEffect::Cancelled),
        }
    }
```

> Add `action_input_source: Option<Box<dyn rollshot_action::SemanticInputSource>>`
> to `Component`, set by `Component::new` from a config field the app fills.
> `action_region()` mirrors the Linux `action_region_from` crop→`CaptureRegion`
> conversion. Recording-phase `Cancel` already drops `self.driver`; ensure the
> action thread is stopped (call a `driver.cancel()` that also sets
> `action_stop`).

Recording controls window: extend `visible_toolbar_rect()` so it also returns
the rect during `WorkspacePhase::Recording` (currently only
`ScrollingCapture`), so the macOS controls window shows the recording controls.

- [ ] **Step 4: Handle `ActionRecorded` in the macOS daemon**

In `macos_product.rs`, in the daemon `update` where `HostEffect` is handled, add
(feature-gated) an arm that takes the `Recording` and calls the shared export
handler from Task 9 (`action_export::export_recording`), logging the resulting
path, then exits or returns to idle. (P0c-2 will instead transition to
`Phase::Timeline`.)

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-iced-overlay/src/macos_capture.rs crates/rollshot-app/src/macos_product.rs
git commit -m "feat(overlay): macOS action recording host effect and daemon handling"
```

---

## Task 9: App export handler + launch wiring + CLI

**Files:**
- Create: `crates/rollshot-app/src/action_export.rs`
- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-cli/src/cmd_action_guide.rs`
- Test: `crates/rollshot-app/src/action_export.rs`

- [ ] **Step 1: Write the failing test**

In `action_export.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{ActionRecorder, CaptureRegion, DetectorConfig, StoreConfig};
    use image::RgbaImage;

    #[test]
    fn export_recording_writes_steps_md() {
        let region = CaptureRegion { x: 0, y: 0, width: 32, height: 32 };
        let mut rec = ActionRecorder::new(region, StoreConfig::default(), DetectorConfig::default());
        rec.ingest_frame(RgbaImage::from_pixel(32, 32, image::Rgba([0,0,0,255])), 0);
        rec.ingest_frame(RgbaImage::from_pixel(32, 32, image::Rgba([255,255,255,255])), 500);
        rec.ingest_frame(RgbaImage::from_pixel(32, 32, image::Rgba([0,0,0,255])), 1500);
        let recording = rec.finish();
        let tmp = std::env::temp_dir().join("rollshot-action-export-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let out = export_recording(recording, region, &tmp).unwrap();
        assert!(out.join("steps.md").exists());
        assert!(out.join("session.json").exists());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-app --features action-guide export_recording`
Expected: FAIL — module/function missing.

- [ ] **Step 3: Implement the export handler**

Create `crates/rollshot-app/src/action_export.rs`:

```rust
//! P0c-1 thin handoff: turn a finished `Recording` into a `Guide` and export it
//! to a default output directory. Replaced by the Timeline Workspace in P0c-2.

use std::path::{Path, PathBuf};

use rollshot_action::{
    export_guide, Guide, InputCapability, InputSourceKind, Recording, CaptureRegion,
};

const TARGET: &str = "rollshot::action::export";

/// Build a guide from detector candidates and export it under `out_dir`.
pub fn export_recording(
    recording: Recording,
    region: CaptureRegion,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    let Recording { candidates, store } = recording;
    let guide = Guide::from_candidates(candidates);
    export_guide(
        &guide,
        &store,
        region,
        // P0c-1 has no review; capability/source are recorded for the manifest.
        InputCapability::VisualOnly { reason: rollshot_action::DegradedReason::SourceStartFailed },
        InputSourceKind::default(),
        out_dir,
    )
    .map_err(|e| format!("export failed: {e}"))
}

/// Default output directory: a timestamped sibling under the OS pictures/temp dir.
pub fn default_out_dir(now_ms: u64) -> PathBuf {
    let base = dirs_pictures().unwrap_or_else(std::env::temp_dir);
    base.join(format!("rollshot-action-{now_ms}"))
}

fn dirs_pictures() -> Option<PathBuf> {
    // Reuse whatever the result_workspace auto-save uses for a base dir; if none,
    // fall back to temp. Keep this minimal for P0c-1.
    None
}
```

> Confirm `InputSourceKind` has a `Default` (or pick the visual-only variant
> explicitly). Confirm `export_guide`’s `capability`/`source` parameter order and
> types against `rollshot-action/src/export.rs`. The capability passed here is a
> placeholder; P0c-2 threads the real `InputCapability` from the session. Wire
> `default_out_dir`’s base to the same helper `result_workspace`/`post_capture`
> uses for auto-save so output lands in a sensible place; `now_ms` is passed in
> by the caller (avoid `SystemTime::now()` deep in the call tree — read it once at
> the call site).

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-app --features action-guide export_recording`
Expected: PASS.

- [ ] **Step 5: Add the launch mode and routing**

In `crates/rollshot-app/src/launch.rs`, add a variant (feature-gated) and parse
`--action-guide`:

```rust
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
    #[cfg(feature = "action-guide")]
    ActionGuide,
}
```

In `parse_launch_args`, add (feature-gated) before the `--action-guide-probe`
branch:

```rust
        #[cfg(feature = "action-guide")]
        if flag == "--action-guide" {
            return Ok(LaunchMode::ActionGuide);
        }
```

In `crates/rollshot-app/src/main.rs`, handle the new mode. On **Linux**:

```rust
        #[cfg(feature = "action-guide")]
        LaunchMode::ActionGuide => run_action_guide_record(),
```

with:

```rust
#[cfg(all(feature = "action-guide", target_os = "linux"))]
fn run_action_guide_record() -> Result<(), String> {
    use rollshot_capture::CaptureRequest;
    let config = /* build OverlayConfig with request = action_guide_region() */;
    let source = crate::action_input::create_input_source();
    match rollshot_iced_overlay::run_action_guide(config, source).map_err(|e| e.to_string())? {
        Some(recording) => {
            let region = /* region from config/result */;
            let now_ms = /* read once here */;
            let out = crate::action_export::export_recording(
                recording, region, &crate::action_export::default_out_dir(now_ms),
            )?;
            tracing::info!(target: "rollshot::action::export", path = %out.display(), "guide exported");
            Ok(())
        }
        None => Ok(()), // cancelled
    }
}
```

On **macOS**, route `LaunchMode::ActionGuide` into `macos_product::run` with the
`OverlayConfig.request = action_guide_region()` so the daemon boots the capture
Component in Action Guide mode; the `HostEffect::ActionRecorded` arm (Task 8)
calls `action_export::export_recording`. Register the `action_export` module in
`main.rs` (`#[cfg(feature = "action-guide")] mod action_export;`).

- [ ] **Step 6: Point the CLI at record mode**

In `crates/rollshot-cli/src/cmd_action_guide.rs`, change the launched flag from
`--action-guide-probe` to `--action-guide`:

```rust
    let status = std::process::Command::new(&app)
        .arg("--action-guide")
        .status()
```

Update the doc comment to describe the full record→export flow. Leave the probe
flag intact in the app for diagnostics.

- [ ] **Step 7: Run the full feature build + tests on the host platform**

Run: `rtk cargo test -p rollshot-app -p rollshot-cli --features action-guide`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/rollshot-app/src/action_export.rs crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs crates/rollshot-cli/src/cmd_action_guide.rs
git commit -m "feat(app): action-guide record launch mode and default export handler"
```

---

## Task 10: Feature-off + fmt + clippy gates (both platforms)

**Files:** none (verification only)

- [ ] **Step 1: Feature-off build still compiles with no new command**

Run: `rtk cargo build -p rollshot-app -p rollshot-cli -p rollshot-iced-overlay`
Expected: PASS; `--action-guide` flag is unrecognized when the feature is off
(matches existing `--action-guide-probe` behavior).

- [ ] **Step 2: fmt**

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 3: clippy with the feature on**

Run: `rtk cargo clippy --workspace --all-targets --features action-guide -- -D warnings`
Expected: clean.

- [ ] **Step 4: Full workspace test**

Run: `rtk cargo test --workspace --features action-guide`
Expected: PASS.

- [ ] **Step 5: macOS parity check (manual reasoning if not on macOS)**

Confirm Tasks 5/6/8 changes compile under `--target` reasoning for macOS: the
shared `app.rs`/`workspace.rs`/`toolbar.rs`/`driver.rs` code is platform-neutral;
`macos_capture.rs` carries the macOS-specific effect handling. If a macOS host or
CI lane is available, run the feature build there. Note in the final summary
which platform was actually exercised and which was verified by reasoning.

- [ ] **Step 6: Commit (if fmt/clippy required fixes)**

```bash
git add -A
git commit -m "chore(action-guide): satisfy fmt/clippy and feature-off gates"
```

---

## Self-Review Checklist (run before handing off)

- **Spec coverage:** toolbar entry (Task 4), region→Start Recording (Task 5),
  Recording controls + capability label + advisory (Task 5), recording lifecycle
  + detection (Tasks 6–8), export to `action-guide/` folder (Task 9), both
  platforms (Tasks 7 & 8), feature-off compile (Task 10). Deferred to P0c-2:
  Timeline Workspace review/edit, output-dir picker, real capability threading
  into the manifest.
- **Signatures to re-confirm during execution** (verify against code, fix inline
  if drifted): `SemanticInputSource::poll` return type; `CropRect` field names;
  `InputSourceKind::default`/variants; `export_guide` parameter order;
  `crop_frame` signature and region type; the `OverlayState` test constructor.
- **Type consistency:** `Workflow::ActionGuide`, `WorkspacePhase::Recording`,
  `WorkspaceEffect::{StartRecording,FinishRecording}`,
  `OverlayEffect::{StartRecording,FinishRecording}`,
  `ToolbarAction::ActionGuide`, `HostEffect::ActionRecorded`,
  `Driver::{begin_action_recording, finalize_action}`,
  `ActionRecording::{start,push_frame,poll_input,finalize}`,
  `action_export::{export_recording, default_out_dir}` — names used identically
  across tasks.

## Definition of Done (P0c-1)

- `rollshot action-guide` launches the overlay; 🎬 selects Action Guide; region
  selection confirms with Start Recording; recording controls show elapsed time +
  capability; Finish runs detection and writes an `action-guide/` folder with
  `steps.md`, `session.json`, and `keyframes/*.png`.
- Headless tests prove the consumer core produces candidates and the export
  handler writes the folder.
- `fmt --check`, `clippy -D warnings`, and `test` pass with the feature on; the
  feature-off build compiles and exposes no new command.
- Both platform paths implemented; the final summary states which was run vs.
  reasoned (AGENTS.md §8).
