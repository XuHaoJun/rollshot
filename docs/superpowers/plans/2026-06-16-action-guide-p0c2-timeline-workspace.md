# Action Guide P0c-2 — Timeline Workspace + Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Insert a dedicated Action Guide Timeline Workspace between detection and export: from a finished `Recording`, the user reviews ordered steps, renames/deletes a step, replaces a step's keyframe from its nearby-frame strip, then either discards the guide or picks an output directory and exports an `action-guide/` Markdown folder — on both Linux and macOS. This replaces P0c-1's direct-export handler.

**Architecture:** Add a sibling `crates/rollshot-app/src/timeline_workspace/` module mirroring `result_workspace/`'s Elm shape (`mod.rs` state/constructor/Linux `run`, `update.rs` `Message`+`update`+`subscription`, `view.rs` `view`). `TimelineWorkspace` owns the edited `rollshot_action::Guide` plus the `FrameStore` (moved out of the `Recording`), the `CaptureRegion`, `InputCapability`, and `InputSourceKind`. Editing calls the existing `Guide::{rename,delete,replace_keyframe}` methods; export calls `rollshot_action::export_guide` directly. **Linux:** `main.rs::run_action_guide_record` boots `timeline_workspace::run(...)` instead of exporting. **macOS:** the `MacosProduct` daemon gains a `Phase::Timeline(TimelineWorkspace)` and `Message::Timeline(..)`, and the `HostEffect::ActionRecorded` arm transitions into it (mirroring `Phase::Workspace`). P0c-1's `action_export.rs` is deleted once both platforms route through the workspace.

**Tech Stack:** Rust, iced 0.14 (`canvas`, `image`, `tokio` already enabled in `rollshot-app`), `rfd` 0.15 (folder picker, already a dep), `dirs` 6 (default dir, already a dep), `rollshot-action`, `rollshot-iced-overlay`. All new code is feature-gated behind the existing `action-guide` Cargo feature.

**Authoritative references:** `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md` (product behavior — Timeline Workspace §, Export §, Session Lifecycle §) and `docs/superpowers/specs/2026-06-16-action-guide-p0c-delta.md` (Decision 4 defines the P0c-2 split and DoD). On any conflict about *wiring*, the delta wins; about *UX*, the original spec wins.

> **Both-platform rule (AGENTS.md §8):** the shared workspace module (Tasks 1–4) is platform-neutral and exercised by both. The Linux handoff (Task 4) and macOS handoff (Task 5) are split deliberately and called out explicitly. The implementing host is Linux; Task 6 states which platform was run vs. reasoned.

> **iced 0.14 rule (AGENTS.md §9):** before implementing `view.rs` (Task 4) and the macOS daemon `view`/`subscription` delegation (Task 5), invoke the `iced-rs` skill. The view code below is modeled on the live 0.14 patterns in `crates/rollshot-app/src/result_workspace/view.rs`; cross-check `horizontal_space`/`Space::new`, `scrollable::Direction`, `button::{primary,secondary,danger}`, `container::{rounded_box,center}`, `stack!`, `mouse_area`, and `image::Handle::from_rgba` against the skill before finalizing.

---

## Verified-current integration seams (commit `7008fc8`)

- **`rollshot-action` Guide/store/export API** (`crates/rollshot-action/src/`):
  - `Recording { pub candidates: Vec<CandidateStep>, pub store: FrameStore }` (`recorder.rs:16`).
  - `Guide::from_candidates(Vec<CandidateStep>) -> Guide` (`guide.rs:15`); `Guide::steps(&self) -> &[GuideStep]` (`guide.rs:33`); `Guide::is_empty(&self) -> bool` (`guide.rs:37`); `Guide::rename(&mut self, index: usize, title: String) -> bool` (1-based; `guide.rs:42`); `Guide::delete(&mut self, index: usize) -> bool` (1-based, renumbers; `guide.rs:54`); `Guide::replace_keyframe(&mut self, index: usize, frame: FrameId) -> bool` (frame must be in the step's `nearby`; `guide.rs:69`).
  - `GuideStep { pub index: usize, pub title: String, pub kind: CandidateKind, pub reason: DetectReason, pub at_ms: Millis, pub keyframe: FrameId, pub nearby: Vec<FrameId>, pub source: CandidateId }` (`models.rs:132`).
  - `CandidateStep { pub id: CandidateId, pub kind: CandidateKind, pub reason: DetectReason, pub at_ms: Millis, pub keyframe: FrameId, pub nearby: Vec<FrameId> }` (`models.rs:120`) — all fields public (used to build synthetic test fixtures).
  - `FrameStore::new(StoreConfig) -> FrameStore` (`frame_store.rs:76`); `FrameStore::retained(&self, id: FrameId) -> Option<&RetainedFrame>` (`frame_store.rs:150`); `RetainedFrame { pub id: FrameId, pub at_ms: Millis, pub image: image::RgbaImage }` (`frame_store.rs:60`).
  - `export_guide(guide: &Guide, store: &FrameStore, region: CaptureRegion, capability: InputCapability, source: InputSourceKind, out_dir: &Path) -> Result<PathBuf, ExportError>` (`export.rs:36`). Writes `out_dir/action-guide/{steps.md,session.json,keyframes/*.png}` via a temp sibling + atomic rename. `ExportError::Empty` when the guide has no steps (`error.rs:30`).
  - Types: `FrameId = u64`, `Millis = u64`, `CandidateId = u64` (`models.rs:6-10`); `CaptureRegion { x:i32, y:i32, width:u32, height:u32 }`; `InputCapability::{SemanticEvents, VisualOnly{reason}}`; `InputSourceKind::{LinuxEvdev, MacosCgEvent, VisualOnly}`; `CandidateKind::{Click,Typing,Scroll,UiChanged}`; `DetectReason::{ClickConfirmed,TypingSettled,ScrollSettled,VisualChange}`. All re-exported from `rollshot_action` (`lib.rs:24-34`).
- **`result_workspace` Elm shape to mirror** (`crates/rollshot-app/src/result_workspace/`):
  - `mod.rs` holds the state struct + `pub fn new(...)` + `#[cfg(target_os="linux")] pub fn run(...) -> Result<(),String>` booting `iced::application(boot, update, view).subscription(subscription).font(..).window(..).run()`.
  - `update.rs` holds `pub enum Message`, `pub(crate) fn update(&mut state, Message) -> Task<Message>`, `pub(crate) fn subscription(&state) -> Subscription<Message>` (uses `iced::window::close_requests().map(|_id| Message::RequestClose)`). Exit is `iced::exit()` returned from `update` (works in both the Linux standalone app and the macOS daemon).
  - `actions.rs::prompt_save_as` uses `rfd::AsyncFileDialog::new().set_directory(d).set_file_name(n).save_file().await` — the folder-picker analog is `.pick_folder().await`.
- **P0c-1 handoff being replaced:**
  - Linux: `crates/rollshot-app/src/main.rs:133-171` `run_action_guide_record()` calls `rollshot_iced_overlay::run_action_guide(config, source) -> Result<Option<(Recording, InputCapability, CaptureRegion)>, OverlayError>` (`lib.rs:92`), then `action_export::export_recording(...)`. P0c-2 boots the workspace instead.
  - macOS: `crates/rollshot-app/src/macos_product.rs:320-348` daemon `update` arm `HostEffect::ActionRecorded(recording, capability, region) => { export_recording(...); iced::exit() }`. P0c-2 transitions to `Phase::Timeline` instead.
  - `crates/rollshot-app/src/action_export.rs` = `export_recording(Recording, ..) -> Result<PathBuf,String>` + `default_out_dir(now_ms)`. Both call sites above are the only users; deleted in Task 6.
- **macOS daemon shape** (`macos_product.rs`): `enum Phase { Capture(Component), Thumbnail(ThumbnailState), Workspace(ResultWorkspace) }` (`:98`); `enum Message { Capture(..), Workspace(result_workspace::Message), Thumbnail*, WorkspaceWindowReady(window::Id), .. }` (`:68`); `update` (`:310`) forwards `Message::Workspace` → `result_workspace::update(ws,msg).map(Message::Workspace)`; `view` (`:573`), `subscription` (`:586`), `theme`/`style` (`:607`,`:614`, both use `Phase::Capture` + `_ =>`); `complete_capture` (`:470`) closes capture windows + `component.shutdown()` then `open_presentation_window`; `workspace_window_settings()` (`:292`) = `{ size 1100x760, min 640x420, decorations true, resizable true, exit_on_close_request false }`; `WorkspaceWindowReady(id)` arm (`:460`) records `product.workspace_window = Some(id)`. `Component::new(&config, #[cfg(feature="action-guide")] Option<Box<dyn SemanticInputSource>>)` already threads the input source (`:139`).

---

## File Structure

**Created:**
- `crates/rollshot-app/src/timeline_workspace/mod.rs` — `TimelineWorkspace` state, `StripFrame`, `new`, `build_handle`, `rebuild_selection_handles`, `selected_step`, and the Linux `run` entry. Owns the `Guide` + `FrameStore` + region/capability/source_kind.
- `crates/rollshot-app/src/timeline_workspace/update.rs` — `Message`, `update`, `subscription`, the synchronous `export_to` helper, `picker_default_dir`, and the async `pick_export_dir` folder picker.
- `crates/rollshot-app/src/timeline_workspace/view.rs` — `view`: header (advisory + Discard/Export), inline message row, step list, keyframe + title editor + delete, nearby-frame strip, and the discard confirmation modal.

**Modified:**
- `crates/rollshot-app/src/main.rs` — register `mod timeline_workspace;`; Task 4 rewrites Linux `run_action_guide_record` to boot the workspace; Task 6 removes `mod action_export;`.
- `crates/rollshot-app/src/macos_product.rs` — `Phase::Timeline`, `Message::Timeline`, `complete_action_recording`, and `update`/`view`/`subscription`/`open_presentation_window` delegation.

**Deleted (Task 6):**
- `crates/rollshot-app/src/action_export.rs` — superseded by the workspace (its `export_recording`/`default_out_dir` have no remaining callers once both platforms route through `timeline_workspace`).

> **Transient-warning policy (matches the P0c-1 plan):** per-task verification runs `cargo test` (warnings allowed). Some workspace fields/functions are unused until a later task consumes them (e.g. `region`/`capability`/`source_kind` until export in Task 3 and the header in Task 4; `run`/`view` until Task 4). The single `clippy -D warnings` gate runs only at Task 6, after both platforms are wired and `action_export.rs` is removed. Do not add `#[allow(dead_code)]`; let Task 6 prove the tree is clean.

---

## Task 1: Timeline workspace module skeleton — state, constructor, selection cache

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Create: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/main.rs` (register module)
- Test: `crates/rollshot-app/src/timeline_workspace/mod.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Register the module in `main.rs`**

In `crates/rollshot-app/src/main.rs`, next to the existing `#[cfg(feature = "action-guide")] mod action_export;` (lines 1–2), add:

```rust
#[cfg(feature = "action-guide")]
mod timeline_workspace;
```

- [ ] **Step 2: Create the minimal `update.rs` so `mod.rs` compiles**

Create `crates/rollshot-app/src/timeline_workspace/update.rs` with just the message type, a `SelectStep`/`DismissMessage` `update`, and an empty `subscription`. (Editing, discard, and export arms are added in Tasks 2–3; the full view in Task 4.)

```rust
use iced::Task;

use super::TimelineWorkspace;

#[derive(Debug, Clone)]
pub enum Message {
    SelectStep(usize),
    DismissMessage,
}

pub fn update(state: &mut TimelineWorkspace, message: Message) -> Task<Message> {
    match message {
        Message::SelectStep(index) => {
            if state.guide.steps().iter().any(|s| s.index == index) {
                state.selected = Some(index);
                state.rebuild_selection_handles();
            }
            Task::none()
        }
        Message::DismissMessage => {
            state.message = None;
            Task::none()
        }
    }
}

pub fn subscription(_state: &TimelineWorkspace) -> iced::Subscription<Message> {
    iced::Subscription::none()
}
```

- [ ] **Step 3: Write the failing constructor test**

Create `crates/rollshot-app/src/timeline_workspace/mod.rs` with the test module first so the build fails on missing items, then implement in Step 4. Add this test body (place it after the code in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{
        ActionRecorder, CandidateKind, CandidateStep, CaptureRegion, DetectReason, DetectorConfig,
        FrameStore, InputCapability, InputSourceKind, Recording, StoreConfig,
    };

    fn region_32() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 32, height: 32 }
    }

    fn black_32() -> RgbaImage {
        RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]))
    }

    fn white_quadrant_32() -> RgbaImage {
        let mut img = black_32();
        for y in 0..16 {
            for x in 0..16 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }

    /// A real recording with retained frames (detector-produced candidates), so
    /// keyframe/nearby handles resolve. Mirrors the P0c-1 export fixture.
    pub(super) fn recording_from_frames() -> Recording {
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut rec = ActionRecorder::new(region_32(), StoreConfig::default(), det);
        rec.ingest_frame(black_32(), 0);
        for i in 1..=6 {
            rec.ingest_frame(white_quadrant_32(), i * 100);
        }
        let recording = rec.finish();
        assert!(
            !recording.candidates.is_empty(),
            "detector fixture should produce at least one candidate"
        );
        recording
    }

    /// A synthetic recording with `n` hand-built candidates and an empty store
    /// (no retained frames). Used by pure update-logic tests that don't assert
    /// on image handles.
    pub(super) fn synthetic_recording(n: usize) -> Recording {
        let candidates = (0..n)
            .map(|i| {
                let base = (i as u64) * 10;
                CandidateStep {
                    id: i as u64,
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: (i as u64) * 100,
                    keyframe: base + 1,
                    nearby: vec![base, base + 1, base + 2],
                }
            })
            .collect();
        Recording {
            candidates,
            store: FrameStore::new(StoreConfig::default()),
        }
    }

    fn workspace(recording: Recording) -> TimelineWorkspace {
        TimelineWorkspace::new(
            recording,
            region_32(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn new_selects_first_step_and_builds_handles() {
        let ws = workspace(recording_from_frames());
        assert!(!ws.guide.steps().is_empty());
        assert_eq!(ws.selected, Some(1));
        assert!(
            ws.keyframe_handle.is_some(),
            "first step keyframe should resolve from the retained store"
        );
        assert!(!ws.strip.is_empty(), "nearby strip should have frames");
    }

    #[test]
    fn new_with_empty_recording_selects_nothing() {
        let ws = workspace(synthetic_recording(0));
        assert!(ws.guide.steps().is_empty());
        assert_eq!(ws.selected, None);
        assert!(ws.keyframe_handle.is_none());
        assert!(ws.strip.is_empty());
    }
}
```

- [ ] **Step 4: Implement `mod.rs` (state + constructor + selection cache)**

Put this above the test module in `crates/rollshot-app/src/timeline_workspace/mod.rs`:

```rust
//! P0c-2 Action Guide Timeline Workspace: review and edit a detected guide
//! (select / rename / delete a step, replace a keyframe from the nearby strip),
//! then export it to a chosen directory. A sibling of `result_workspace/`,
//! reachable only when the `action-guide` feature is built. Replaces P0c-1's
//! direct-export handler.
//!
//! Session-lifecycle tail (original spec §Session Lifecycle):
//!
//! ```text
//! Reviewing  (rename / delete / replace keyframe)
//!    |  Discard -> Discarded (exit; FrameStore dropped)
//!    v  Export Guide -> pick directory
//! Exporting  (export_guide writes a temp sibling, then atomic rename)
//!    |  error -> back to Reviewing (inline message; session intact)
//!    v
//! Done  (exit; temporary assets dropped on app exit)
//! ```

mod update;

pub use update::{subscription, update, Message};

use rollshot_action::{
    CaptureRegion, FrameId, FrameStore, Guide, GuideStep, InputCapability, InputSourceKind,
    Recording,
};

/// One nearby-strip thumbnail: a retained frame id and its prebuilt iced handle.
pub(crate) struct StripFrame {
    pub id: FrameId,
    pub handle: iced::widget::image::Handle,
}

/// The Action Guide review/export workspace. Owns the editable guide and the
/// frame store moved out of the finished `Recording`.
pub struct TimelineWorkspace {
    pub(crate) guide: Guide,
    pub(crate) store: FrameStore,
    pub(crate) region: CaptureRegion,
    pub(crate) capability: InputCapability,
    pub(crate) source_kind: InputSourceKind,
    /// 1-based index of the selected step, or `None` when the guide is empty.
    pub(crate) selected: Option<usize>,
    /// Inline banner (export error / advisory). `None` when clear.
    pub(crate) message: Option<String>,
    /// True while the discard confirmation modal is shown.
    pub(crate) pending_discard: bool,
    /// Cached handle for the selected step's current keyframe.
    pub(crate) keyframe_handle: Option<iced::widget::image::Handle>,
    /// Cached nearby-strip thumbnails for the selected step.
    pub(crate) strip: Vec<StripFrame>,
}

impl TimelineWorkspace {
    /// Build the workspace from a finished recording. Selects step 1 (if any)
    /// and primes the selection handle cache.
    pub fn new(
        recording: Recording,
        region: CaptureRegion,
        capability: InputCapability,
        source_kind: InputSourceKind,
    ) -> Self {
        let Recording { candidates, store } = recording;
        let guide = Guide::from_candidates(candidates);
        let selected = (!guide.is_empty()).then_some(1);
        let mut ws = Self {
            guide,
            store,
            region,
            capability,
            source_kind,
            selected,
            message: None,
            pending_discard: false,
            keyframe_handle: None,
            strip: Vec::new(),
        };
        ws.rebuild_selection_handles();
        ws
    }

    /// The currently selected step, if any.
    pub(crate) fn selected_step(&self) -> Option<&GuideStep> {
        let index = self.selected?;
        self.guide.steps().iter().find(|s| s.index == index)
    }

    /// Recompute the cached keyframe handle and nearby strip for the current
    /// selection. Called after any change to `selected` or to a keyframe.
    pub(crate) fn rebuild_selection_handles(&mut self) {
        self.keyframe_handle = None;
        self.strip.clear();
        let Some(step) = self.selected_step() else {
            return;
        };
        let keyframe = step.keyframe;
        let nearby = step.nearby.clone();
        if let Some(frame) = self.store.retained(keyframe) {
            self.keyframe_handle = Some(build_handle(&frame.image));
        }
        for id in nearby {
            if let Some(frame) = self.store.retained(id) {
                let handle = build_handle(&frame.image);
                self.strip.push(StripFrame { id, handle });
            }
        }
    }
}

/// Build an iced image handle from a retained RGBA frame.
pub(crate) fn build_handle(image: &image::RgbaImage) -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_rgba(image.width(), image.height(), image.as_raw().clone())
}
```

> The borrow in `rebuild_selection_handles` is split deliberately: read `step.keyframe`/`step.nearby` (clone the small `Vec<FrameId>`) before the `&mut self.store.retained(..)` reads, so the immutable `selected_step()` borrow ends before mutating `keyframe_handle`/`strip`.

- [ ] **Step 5: Run the constructor tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`
Expected: PASS (`new_selects_first_step_and_builds_handles`, `new_with_empty_recording_selects_nothing`).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-app/src/timeline_workspace/ crates/rollshot-app/src/main.rs
git commit -m "feat(app): timeline workspace state and constructor"
```

---

## Task 2: Editing operations — select, rename, delete, replace keyframe

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: same file (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Add a test module at the bottom of `crates/rollshot-app/src/timeline_workspace/update.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_workspace::tests::{recording_from_frames, synthetic_recording};
    use crate::timeline_workspace::TimelineWorkspace;
    use rollshot_action::{CaptureRegion, InputCapability, InputSourceKind};

    fn ws(recording: rollshot_action::Recording) -> TimelineWorkspace {
        TimelineWorkspace::new(
            recording,
            CaptureRegion { x: 0, y: 0, width: 32, height: 32 },
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn select_step_changes_selection() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(2));
        assert_eq!(state.selected, Some(2));
    }

    #[test]
    fn select_out_of_range_is_ignored() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::SelectStep(99));
        assert_eq!(state.selected, Some(1));
    }

    #[test]
    fn title_changed_renames_selected_step() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::TitleChanged("Open Preferences".to_string()));
        assert_eq!(state.selected_step().unwrap().title, "Open Preferences");
    }

    #[test]
    fn delete_step_renumbers_and_clamps_selection() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(3));
        let _ = update(&mut state, Message::DeleteStep);
        assert_eq!(state.guide.steps().len(), 2);
        // Steps are renumbered 1..=2; selection clamps to the new last step.
        assert_eq!(state.selected, Some(2));
        assert!(state.guide.steps().iter().all(|s| s.index <= 2));
    }

    #[test]
    fn delete_last_remaining_step_clears_selection() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::DeleteStep);
        assert!(state.guide.steps().is_empty());
        assert_eq!(state.selected, None);
    }

    #[test]
    fn replace_keyframe_swaps_to_a_nearby_frame() {
        let mut state = ws(synthetic_recording(1));
        let step = state.selected_step().unwrap();
        // synthetic step 1: keyframe = 1, nearby = [0, 1, 2].
        assert_eq!(step.keyframe, 1);
        let target = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();
        let _ = update(&mut state, Message::ReplaceKeyframe(target));
        assert_eq!(state.selected_step().unwrap().keyframe, target);
    }

    #[test]
    fn replace_keyframe_rejects_frame_outside_nearby() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::ReplaceKeyframe(9999));
        assert_eq!(state.selected_step().unwrap().keyframe, 1);
    }

    #[test]
    fn delete_on_real_recording_keeps_handles_consistent() {
        // Real store so rebuild_selection_handles resolves frames; ensures the
        // delete path's handle rebuild does not panic.
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::DeleteStep);
        // No assertion on handle contents (opaque); reaching here = no panic.
    }
}
```

> `crate::timeline_workspace::tests::{recording_from_frames, synthetic_recording}` are the `pub(super)` fixtures defined in `mod.rs` (Task 1). They are reachable from the `update.rs` test module via the crate path.

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`
Expected: FAIL — `no variant TitleChanged/DeleteStep/ReplaceKeyframe`.

- [ ] **Step 3: Add the editing variants and update arms**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, extend `Message` (keep `SelectStep`/`DismissMessage`):

```rust
#[derive(Debug, Clone)]
pub enum Message {
    SelectStep(usize),
    TitleChanged(String),
    DeleteStep,
    ReplaceKeyframe(rollshot_action::FrameId),
    DismissMessage,
}
```

Add the new arms inside `update`'s match (before `DismissMessage`):

```rust
        Message::TitleChanged(title) => {
            if let Some(index) = state.selected {
                state.guide.rename(index, title);
            }
            Task::none()
        }
        Message::DeleteStep => {
            if let Some(index) = state.selected {
                if state.guide.delete(index) {
                    let len = state.guide.steps().len();
                    state.selected = if len == 0 { None } else { Some(index.min(len)) };
                    state.rebuild_selection_handles();
                }
            }
            Task::none()
        }
        Message::ReplaceKeyframe(frame) => {
            if let Some(index) = state.selected {
                if state.guide.replace_keyframe(index, frame) {
                    state.rebuild_selection_handles();
                }
            }
            Task::none()
        }
```

> `Guide::rename` updates `GuideStep.title` in place, so the `text_input` (Task 4) reads the live value straight from `selected_step().title` — no separate edit buffer. `Guide::delete` already renumbers remaining steps to `1..=len`, so a clamped `selected` index still resolves.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`
Expected: PASS (8 new + Task 1's 2).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-app/src/timeline_workspace/update.rs
git commit -m "feat(app): timeline workspace select/rename/delete/replace-keyframe"
```

---

## Task 3: Discard + export flow (folder picker, atomic export, error recovery)

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `update.rs`:

```rust
    #[test]
    fn discard_requested_shows_modal_then_cancel_clears_it() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::DiscardRequested);
        assert!(state.pending_discard);
        let _ = update(&mut state, Message::CancelDiscard);
        assert!(!state.pending_discard);
    }

    #[test]
    fn close_requested_also_prompts_discard() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::CloseRequested);
        assert!(state.pending_discard);
    }

    #[test]
    fn export_dir_chosen_writes_guide_folder_and_clears_message() {
        let mut state = ws(recording_from_frames());
        state.message = Some("stale".to_string());
        let tmp = std::env::temp_dir().join("rollshot-timeline-export-ok");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let _ = update(&mut state, Message::ExportDirChosen(Some(tmp.clone())));
        assert!(tmp.join("action-guide/steps.md").exists());
        assert!(tmp.join("action-guide/session.json").exists());
        assert!(state.message.is_none(), "successful export clears the banner");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn export_empty_guide_sets_error_and_writes_nothing() {
        let mut state = ws(synthetic_recording(0));
        let tmp = std::env::temp_dir().join("rollshot-timeline-export-empty");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let _ = update(&mut state, Message::ExportDirChosen(Some(tmp.clone())));
        assert!(!tmp.join("action-guide").exists(), "empty guide must not write a folder");
        assert!(state.message.is_some(), "export failure surfaces an inline message");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn export_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportDirChosen(None));
        assert!(state.message.is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`
Expected: FAIL — `no variant DiscardRequested/CancelDiscard/CloseRequested/ConfirmDiscard/ExportRequested/ExportDirChosen`.

- [ ] **Step 3: Add the imports, variants, update arms, and helpers**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, update the top-of-file imports:

```rust
use std::path::{Path, PathBuf};

use iced::Task;
use rollshot_action::export_guide;

use super::TimelineWorkspace;
```

Extend `Message`:

```rust
#[derive(Debug, Clone)]
pub enum Message {
    SelectStep(usize),
    TitleChanged(String),
    DeleteStep,
    ReplaceKeyframe(rollshot_action::FrameId),
    DiscardRequested,
    CloseRequested,
    CancelDiscard,
    ConfirmDiscard,
    ExportRequested,
    ExportDirChosen(Option<PathBuf>),
    DismissMessage,
}
```

Add the new arms inside `update`'s match (before `DismissMessage`):

```rust
        Message::DiscardRequested | Message::CloseRequested => {
            state.pending_discard = true;
            Task::none()
        }
        Message::CancelDiscard => {
            state.pending_discard = false;
            Task::none()
        }
        Message::ConfirmDiscard => iced::exit(),
        Message::ExportRequested => {
            state.message = None;
            Task::perform(pick_export_dir(picker_default_dir()), Message::ExportDirChosen)
        }
        Message::ExportDirChosen(None) => Task::none(),
        Message::ExportDirChosen(Some(dir)) => match export_to(state, &dir) {
            Ok(out) => {
                tracing::info!(
                    target: "rollshot::action::export",
                    path = %out.display(),
                    "guide exported"
                );
                iced::exit()
            }
            Err(error) => {
                tracing::error!(
                    target: "rollshot::action::export",
                    %error,
                    "guide export failed"
                );
                state.message = Some(error);
                Task::none()
            }
        },
```

Add the helpers below `subscription` (replace the empty `subscription` body with the window-close subscription):

```rust
pub fn subscription(_state: &TimelineWorkspace) -> iced::Subscription<Message> {
    iced::window::close_requests().map(|_id| Message::CloseRequested)
}

/// Export the (possibly edited) guide into `out_dir/action-guide/`.
fn export_to(state: &TimelineWorkspace, out_dir: &Path) -> Result<PathBuf, String> {
    export_guide(
        &state.guide,
        &state.store,
        state.region,
        state.capability,
        state.source_kind,
        out_dir,
    )
    .map_err(|e| format!("export failed: {e}"))
}

/// Initial directory for the folder picker: the user's Pictures dir, or temp.
fn picker_default_dir() -> PathBuf {
    dirs::picture_dir().unwrap_or_else(std::env::temp_dir)
}

async fn pick_export_dir(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}
```

> Export runs synchronously in `update` because the workspace exits immediately on success — there is no post-export interaction to keep responsive (unlike `result_workspace`'s async save). `export_guide` is itself atomic (temp sibling + rename), so a mid-write failure leaves no `action-guide/` folder and the workspace stays open with the error banner (spec §Failure Handling, §Export). The folder *picker* is async via `Task::perform`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-app/src/timeline_workspace/update.rs
git commit -m "feat(app): timeline workspace discard + directory-picked export"
```

---

## Task 4: View, Linux `run` entry, and Linux handoff rewire

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs` (`mod view;` + `run`)
- Modify: `crates/rollshot-app/src/main.rs` (Linux `run_action_guide_record`)
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs` (smoke build)

> **Invoke the `iced-rs` skill before this task** (AGENTS.md §9). The view mirrors `result_workspace/view.rs` 0.14 patterns; verify each widget/style fn against the skill.

- [ ] **Step 1: Create `view.rs` with the full workspace view**

Create `crates/rollshot-app/src/timeline_workspace/view.rs`:

```rust
use iced::widget::{
    button, column, container, horizontal_space, image, mouse_area, row, scrollable, stack, text,
    text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Theme};

use super::{Message, TimelineWorkspace};

pub fn view(state: &TimelineWorkspace) -> Element<'_, Message> {
    let body: Element<Message> = column![
        header(state),
        message_row(state),
        main_area(state),
        strip_row(state),
    ]
    .spacing(8)
    .padding(12)
    .into();

    if state.pending_discard {
        discard_modal(body)
    } else {
        body
    }
}

fn header(state: &TimelineWorkspace) -> Element<'_, Message> {
    let advisory: Element<Message> = match state.capability {
        rollshot_action::InputCapability::VisualOnly { .. } => {
            text("Visual-only detection.").size(13).into()
        }
        rollshot_action::InputCapability::SemanticEvents => Space::new().into(),
    };
    row![
        advisory,
        horizontal_space(),
        button(text("Discard"))
            .on_press(Message::DiscardRequested)
            .style(button::secondary),
        button(text("Export Guide"))
            .on_press(Message::ExportRequested)
            .style(button::primary),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn message_row(state: &TimelineWorkspace) -> Element<'_, Message> {
    match &state.message {
        Some(msg) => container(
            row![
                text(msg.clone()).width(Length::Fill),
                button(text("Dismiss")).on_press(Message::DismissMessage),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(8)
        .into(),
        None => Space::new().into(),
    }
}

fn main_area(state: &TimelineWorkspace) -> Element<'_, Message> {
    row![step_list(state), detail_panel(state)]
        .spacing(8)
        .height(Length::Fill)
        .into()
}

fn step_list(state: &TimelineWorkspace) -> Element<'_, Message> {
    let mut col = column![].spacing(4);
    for step in state.guide.steps() {
        let selected = state.selected == Some(step.index);
        let label = text(format!("{}. {}", step.index, step.title));
        col = col.push(
            button(label)
                .width(Length::Fill)
                .on_press(Message::SelectStep(step.index))
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                }),
        );
    }
    container(scrollable(col))
        .width(Length::FillPortion(2))
        .height(Length::Fill)
        .into()
}

fn detail_panel(state: &TimelineWorkspace) -> Element<'_, Message> {
    let content: Element<Message> = match state.selected_step() {
        Some(step) => {
            let keyframe: Element<Message> = match &state.keyframe_handle {
                Some(handle) => image(handle.clone()).into(),
                None => text("(keyframe unavailable)").into(),
            };
            column![
                container(keyframe).height(Length::Fill).center_x(Length::Fill),
                text_input("Step title", &step.title).on_input(Message::TitleChanged),
                button(text("Delete step"))
                    .on_press(Message::DeleteStep)
                    .style(button::danger),
            ]
            .spacing(8)
            .into()
        }
        None => container(text("No steps detected."))
            .center(Length::Fill)
            .into(),
    };
    container(content)
        .width(Length::FillPortion(3))
        .height(Length::Fill)
        .into()
}

fn strip_row(state: &TimelineWorkspace) -> Element<'_, Message> {
    let current = state.selected_step().map(|s| s.keyframe);
    let mut strip = row![].spacing(6);
    for frame in &state.strip {
        let selected = current == Some(frame.id);
        strip = strip.push(
            button(image(frame.handle.clone()).width(Length::Fixed(96.0)))
                .on_press(Message::ReplaceKeyframe(frame.id))
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                }),
        );
    }
    container(
        scrollable(strip)
            .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default())),
    )
    .height(Length::Fixed(120.0))
    .into()
}

fn discard_modal(base: Element<'_, Message>) -> Element<'_, Message> {
    let dialog = container(
        column![
            text("Discard this guide?").size(18),
            text("The recording and all detected steps will be deleted.").size(13),
            row![
                button(text("Cancel")).on_press(Message::CancelDiscard),
                button(text("Discard"))
                    .on_press(Message::ConfirmDiscard)
                    .style(button::danger),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .style(container::rounded_box);

    let scrim = mouse_area(
        container(dialog)
            .center(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(
                    Color {
                        a: 0.8,
                        ..Color::BLACK
                    }
                    .into(),
                ),
                ..container::Style::default()
            }),
    )
    .on_press(Message::CancelDiscard);

    stack![base, scrim].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_workspace::tests::{recording_from_frames, synthetic_recording};
    use crate::timeline_workspace::TimelineWorkspace;
    use rollshot_action::{CaptureRegion, InputCapability, InputSourceKind};

    fn ws(recording: rollshot_action::Recording, capability: InputCapability) -> TimelineWorkspace {
        TimelineWorkspace::new(
            recording,
            CaptureRegion { x: 0, y: 0, width: 32, height: 32 },
            capability,
            InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn view_builds_for_selected_empty_and_discard_states() {
        // Selected step with real handles + semantic header.
        let selected = ws(recording_from_frames(), InputCapability::SemanticEvents);
        let _ = view(&selected);

        // Visual-only advisory + inline message + discard modal.
        let mut degraded = ws(
            synthetic_recording(2),
            InputCapability::VisualOnly {
                reason: rollshot_action::DegradedReason::PermissionDenied,
            },
        );
        degraded.message = Some("export failed: disk full".to_string());
        degraded.pending_discard = true;
        let _ = view(&degraded);

        // Empty guide / no selection.
        let empty = ws(synthetic_recording(0), InputCapability::SemanticEvents);
        let _ = view(&empty);
    }
}
```

> The view builds and drops an `Element` (pure widget-tree construction, no renderer) to catch API misuse and panics. Confirm `center_x(Length::Fill)`, `container::center`, `scrollable::Scrollbar::default()`, and `stack!` signatures via the `iced-rs` skill; adjust to the 0.14 forms used in `result_workspace/view.rs` if they differ.

- [ ] **Step 2: Wire `view` and the Linux `run` into `mod.rs`**

In `crates/rollshot-app/src/timeline_workspace/mod.rs`, add `mod view;` next to `mod update;`, and extend the re-export:

```rust
mod update;
mod view;

pub use update::{subscription, update, Message};
pub use view::view;
```

Append the Linux entry point (after the `build_handle` fn, before the test module):

```rust
/// Boot the timeline workspace as a standalone iced app (Linux). Blocks until
/// the user exports (then exits) or discards/closes (then exits).
#[cfg(target_os = "linux")]
pub fn run(
    recording: Recording,
    region: CaptureRegion,
    capability: InputCapability,
    source_kind: InputSourceKind,
) -> Result<(), String> {
    use std::sync::{Arc, Mutex};

    let boot_data = Arc::new(Mutex::new(Some((recording, region, capability, source_kind))));
    let boot = move || {
        let (recording, region, capability, source_kind) = boot_data
            .lock()
            .unwrap()
            .take()
            .expect("timeline workspace boot data already consumed");
        (
            TimelineWorkspace::new(recording, region, capability, source_kind),
            iced::Task::none(),
        )
    };

    iced::application(boot, update, view)
        .title("Rollshot — Action Guide")
        .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
        .font(rollshot_image_document::style::FONT_BOLD_BYTES)
        .subscription(subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1100.0, 760.0),
            min_size: Some(iced::Size::new(640.0, 420.0)),
            decorations: true,
            resizable: true,
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Rewire the Linux launch handler in `main.rs`**

Replace the body of `run_action_guide_record` (`crates/rollshot-app/src/main.rs:133-171`, the `#[cfg(all(feature = "action-guide", target_os = "linux"))]` one) so it boots the workspace instead of exporting directly:

```rust
#[cfg(all(feature = "action-guide", target_os = "linux"))]
fn run_action_guide_record() -> Result<(), String> {
    use rollshot_capture::CaptureRequest;
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: "auto".to_string(),
        fps: 5,
        show_cursor: false,
        request: CaptureRequest::action_guide_region(),
        target_output_name: None,
    };
    let source = crate::action_input::create_input_source();
    match rollshot_iced_overlay::run_action_guide(config, source).map_err(|e| e.to_string())? {
        Some((recording, capability, region)) => {
            let source_kind = match capability {
                rollshot_action::InputCapability::VisualOnly { .. } => {
                    rollshot_action::InputSourceKind::VisualOnly
                }
                _ => rollshot_action::InputSourceKind::LinuxEvdev,
            };
            crate::timeline_workspace::run(recording, region, capability, source_kind)
        }
        None => Ok(()),
    }
}
```

> This drops the `action_export`/`default_out_dir` call and the `now_ms` computation from the Linux path. `action_export.rs` is still used by the macOS arm until Task 5; do not delete it yet. Recording config (`fps: 5`) is unchanged — capture-rate tuning is out of P0c-2 scope.

- [ ] **Step 4: Run the build + view smoke test**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`
Expected: PASS (including `view_builds_for_selected_empty_and_discard_states`).

If the view smoke test or build reveals an iced 0.14 API mismatch, fix it against the `iced-rs` skill and the patterns in `result_workspace/view.rs`, then re-run.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-app/src/timeline_workspace/ crates/rollshot-app/src/main.rs
git commit -m "feat(app): timeline workspace view + Linux run + launch wiring"
```

---

## Task 5: macOS handoff — `Phase::Timeline` in the product daemon

**Files:**
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-app/src/macos_product.rs` (`#[cfg(test)] mod tests`)

> macOS counterpart of Task 4 — required by AGENTS.md §8. The daemon already owns the capture `Component` and receives `HostEffect::ActionRecorded`; this routes it into a new `Phase::Timeline` instead of exporting + exiting. Compiles/tests run on macOS or by reasoning (Task 6 records which).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/rollshot-app/src/macos_product.rs`:

```rust
    #[cfg(feature = "action-guide")]
    #[test]
    fn complete_action_recording_enters_timeline_phase() {
        use image::{Rgba, RgbaImage};
        use rollshot_action::{ActionRecorder, CaptureRegion, DetectorConfig, StoreConfig};

        let region = CaptureRegion { x: 0, y: 0, width: 32, height: 32 };
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut rec = ActionRecorder::new(region, StoreConfig::default(), det);
        rec.ingest_frame(RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255])), 0);
        for i in 1..=6 {
            let mut img = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]));
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                }
            }
            rec.ingest_frame(img, i * 100);
        }
        let recording = rec.finish();

        let mut product = product_in_capture_phase();
        let _ = complete_action_recording(
            &mut product,
            recording,
            rollshot_action::InputCapability::SemanticEvents,
            region,
        );
        assert!(matches!(product.phase, Phase::Timeline(_)));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run (on a macOS host): `rtk cargo test -p rollshot-app --features action-guide --lib macos_product`
Expected: FAIL — `no variant Phase::Timeline`, `complete_action_recording` undefined. (On Linux this module is `#[cfg(target_os = "macos")]`; verify by reasoning per AGENTS.md §8 and run the build under a macOS target/CI lane if available.)

- [ ] **Step 3: Add `Phase::Timeline`, `Message::Timeline`, and the transition helper**

In `crates/rollshot-app/src/macos_product.rs`:

Add the import near the top (with the other module `use`s):

```rust
#[cfg(feature = "action-guide")]
use crate::timeline_workspace::{self, TimelineWorkspace};
```

Extend `Phase` (`:98`):

```rust
#[allow(clippy::large_enum_variant)]
pub enum Phase {
    Capture(Component),
    Thumbnail(ThumbnailState),
    Workspace(ResultWorkspace),
    #[cfg(feature = "action-guide")]
    Timeline(TimelineWorkspace),
}
```

Extend `Message` (`:68`), after the `Workspace(..)` variant:

```rust
    /// A Timeline Workspace (Action Guide) message.
    #[cfg(feature = "action-guide")]
    Timeline(timeline_workspace::Message),
```

Add the transition helper (place near `complete_capture`, `:470`):

```rust
/// Close the capture-owned windows, build the Timeline Workspace from the
/// finished recording, enter `Phase::Timeline`, and open the workspace window —
/// all inside the one daemon (mirrors `complete_capture`).
#[cfg(feature = "action-guide")]
fn complete_action_recording(
    product: &mut MacosProduct,
    recording: rollshot_action::Recording,
    capability: rollshot_action::InputCapability,
    region: rollshot_action::CaptureRegion,
) -> Task<Message> {
    let mut close_tasks = Vec::new();
    if let Phase::Capture(component) = &mut product.phase {
        if let Some(id) = component.overlay_window() {
            close_tasks.push(window::close(id));
        }
        if let Some(id) = component.controls_window() {
            close_tasks.push(window::close(id));
        }
        component.shutdown();
    }

    let source_kind = match capability {
        rollshot_action::InputCapability::VisualOnly { .. } => {
            rollshot_action::InputSourceKind::VisualOnly
        }
        _ => rollshot_action::InputSourceKind::MacosCgEvent,
    };
    product.phase = Phase::Timeline(TimelineWorkspace::new(
        recording,
        region,
        capability,
        source_kind,
    ));

    let (id, open) = window::open(workspace_window_settings());
    product.workspace_window = Some(id);
    close_tasks.push(open.map(Message::WorkspaceWindowReady));
    Task::batch(close_tasks)
}
```

- [ ] **Step 4: Replace the `HostEffect::ActionRecorded` arm and add `Message::Timeline` delegation**

Replace the daemon `update` arm at `:320-348` (the `HostEffect::ActionRecorded` block that exported + `iced::exit()`) with:

```rust
                #[cfg(feature = "action-guide")]
                HostEffect::ActionRecorded(recording, capability, region) => {
                    complete_action_recording(product, recording, capability, region)
                }
```

Add a `Message::Timeline` arm next to the `Message::Workspace` arm (`:356`):

```rust
        #[cfg(feature = "action-guide")]
        Message::Timeline(msg) => {
            let Phase::Timeline(workspace) = &mut product.phase else {
                return Task::none();
            };
            timeline_workspace::update(workspace, msg).map(Message::Timeline)
        }
```

- [ ] **Step 5: Add `Phase::Timeline` arms to `view`, `subscription`, and `open_presentation_window`**

In `view` (`:573`), add before the final `Phase::Capture(_)` fallback:

```rust
        #[cfg(feature = "action-guide")]
        Phase::Timeline(workspace) => timeline_workspace::view(workspace).map(Message::Timeline),
```

In `subscription` (`:586`), add an arm:

```rust
        #[cfg(feature = "action-guide")]
        Phase::Timeline(workspace) => {
            timeline_workspace::subscription(workspace).map(Message::Timeline)
        }
```

In `open_presentation_window` (`:496`), add an arm so a Timeline phase opens the workspace window (it is only reached via `complete_action_recording`, which already opens the window, so this is a defensive parallel to `Phase::Workspace`):

```rust
        #[cfg(feature = "action-guide")]
        Phase::Timeline(_) => {
            let (id, open) = window::open(workspace_window_settings());
            product.workspace_window = Some(id);
            open.map(Message::WorkspaceWindowReady)
        }
```

> `theme` (`:607`) and `style` (`:614`) match `Phase::Capture` then `_ =>`, so `Phase::Timeline` is already covered (Dark theme + default style, matching `Phase::Workspace`). `MacosProduct::workspace()` (`:225`) uses `_ => None`, so it needs no change. No new `unreachable!`/`select_presentation` arms are required — `Phase::Timeline` is entered only from `HostEffect::ActionRecorded`.

- [ ] **Step 6: Run tests**

Run (macOS host): `rtk cargo test -p rollshot-app --features action-guide --lib macos_product`
Expected: PASS (`complete_action_recording_enters_timeline_phase`).
If no macOS host is available, confirm the changes compile by reasoning (all new arms are feature-gated and mirror the existing `Phase::Workspace` delegation) and note this in Task 6's summary.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-app/src/macos_product.rs
git commit -m "feat(app): macOS Phase::Timeline for action-guide review/export"
```

---

## Task 6: Remove the superseded `action_export.rs`, then fmt/clippy/feature gates

**Files:**
- Delete: `crates/rollshot-app/src/action_export.rs`
- Modify: `crates/rollshot-app/src/main.rs` (remove `mod action_export;`)

- [ ] **Step 1: Confirm `action_export` has no remaining callers**

Run: `rtk grep -rn "action_export" crates/`
Expected: matches only in `main.rs` (the `mod action_export;` declaration) — both the Linux handler (Task 4) and the macOS arm (Task 5) now route through `timeline_workspace`. If any other production call site remains, stop and reconcile before deleting.

- [ ] **Step 2: Delete the module and its declaration**

```bash
git rm crates/rollshot-app/src/action_export.rs
```

In `crates/rollshot-app/src/main.rs`, remove the two-line declaration:

```rust
#[cfg(feature = "action-guide")]
mod action_export;
```

- [ ] **Step 3: Feature-on build + tests**

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS (the `export_recording_writes_steps_md` test was deleted with `action_export.rs`; export coverage now lives in `timeline_workspace::update::tests`).

- [ ] **Step 4: Feature-off build still compiles with no new command**

Run: `rtk cargo build -p rollshot-app -p rollshot-cli`
Expected: PASS; `--action-guide` is unrecognized when the feature is off (`launch.rs` only parses it under `#[cfg(feature = "action-guide")]`), and `timeline_workspace` is not compiled.

- [ ] **Step 5: fmt**

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 6: clippy with the feature on**

Run: `rtk cargo clippy --workspace --all-targets --features action-guide -- -D warnings`
Expected: clean. Fix any dead-code/unused-import warnings deferred from earlier tasks here.

- [ ] **Step 7: Full workspace test**

Run: `rtk cargo test --workspace --features action-guide`
Expected: PASS.

- [ ] **Step 8: macOS parity check**

The shared module (`timeline_workspace/`) is platform-neutral. `macos_product.rs` carries the macOS-specific `Phase::Timeline` wiring (Task 5). If a macOS host or CI lane is available, run `rtk cargo clippy -p rollshot-app --all-targets --features action-guide -- -D warnings` and `rtk cargo test -p rollshot-app --features action-guide` there. Record in the final summary which platform was actually exercised and which was verified by reasoning (AGENTS.md §8).

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "chore(action-guide): remove superseded action_export; fmt/clippy/feature gates"
```

---

## Self-Review Checklist (run before handing off)

- **Spec coverage** (original spec §Action Guide Timeline Workspace, §Export; delta Decision 4):
  - Select a step → `Message::SelectStep` (Task 2), step-list buttons (Task 4). ✓
  - Edit its title → `Message::TitleChanged` → `Guide::rename`, `text_input` bound to live title (Tasks 2, 4). ✓
  - Delete it (renumber) → `Message::DeleteStep` → `Guide::delete` (renumbers), selection clamp (Task 2); Delete button (Task 4). ✓
  - Replace keyframe from the nearby strip (no new window) → `Message::ReplaceKeyframe` → `Guide::replace_keyframe`, bottom strip of `step.nearby` thumbnails (Tasks 2, 4). ✓
  - Discard the whole guide → Discard button → confirm modal → `ConfirmDiscard` → `iced::exit()` (Tasks 3, 4). ✓
  - Export: user chooses output directory → `rfd` `pick_folder` → `export_guide` writes `action-guide/{steps.md,session.json,keyframes/*.png}` (Task 3). ✓
  - Degraded advisory shown in the workspace header (Task 4 `header`); capability/source threaded into the export manifest (Tasks 1, 3). ✓
  - Both platforms: Linux standalone `run` (Task 4), macOS `Phase::Timeline` (Task 5). ✓
  - Markdown generated at export time only; workspace shows no Markdown (the workspace never builds Markdown — only `export_guide` does). ✓
- **Placeholder scan:** no `TODO`/`TBD`/"implement later"; every code step shows full code. The only forward references are the documented transient warnings resolved at Task 6. ✓
- **Type consistency:** identical names across tasks — `TimelineWorkspace`, `StripFrame`, `Message::{SelectStep,TitleChanged,DeleteStep,ReplaceKeyframe,DiscardRequested,CloseRequested,CancelDiscard,ConfirmDiscard,ExportRequested,ExportDirChosen,DismissMessage}`, `update`, `subscription`, `view`, `run`, `build_handle`, `rebuild_selection_handles`, `selected_step`, `export_to`, `picker_default_dir`, `pick_export_dir`, `Phase::Timeline`, `Message::Timeline`, `complete_action_recording`. The `Message` enum grows across Tasks 1→2→3 (variants added, never renamed). ✓
- **Signatures re-confirmed against code** (commit `7008fc8`): `Guide::{steps,rename,delete,replace_keyframe,is_empty,from_candidates}`; `FrameStore::retained`; `RetainedFrame.image`; `export_guide(&Guide,&FrameStore,CaptureRegion,InputCapability,InputSourceKind,&Path)`; `iced::widget::image::Handle::from_rgba`; `iced::window::close_requests`; `rfd::AsyncFileDialog::pick_folder`; macOS `Phase`/`Message`/`update`/`view`/`subscription`/`complete_capture`/`workspace_window_settings`/`Component::{overlay_window,controls_window,shutdown}`. View-only 0.14 widget/style fns are flagged for `iced-rs`-skill confirmation in Task 4.

## Review Outputs

### NOT in scope (unchanged from original spec Deferred Work + delta)

- Merge/split editing, manual Add Step, full-session scrubber, free-form Markdown editing.
- GIF/HTML/MP4/WebM export; OCR/a11y/LLM/window-title labels.
- Capture-rate change (P0c-1's `fps: 5` is untouched), memory ceiling / frame-store paging.
- Global hotkey, cross-platform absolute pointer position.
- Re-export after edit beyond a single directory pick; "reveal in file manager" after export.
- Recoverable cleanup of temporary session assets after abnormal termination (spec §Frame Pipeline) — out of P0c-2; assets are dropped on app exit.

### What already exists (reused, not rebuilt)

- `rollshot-action` `Guide`/`FrameStore`/`export_guide` editing+export API — called directly.
- `result_workspace/` Elm shape (mod/update/view split, Linux `run` boot closure, `iced::window::close_requests` subscription, `iced::exit()` for termination, `rfd` dialog pattern) — mirrored.
- macOS `MacosProduct` daemon (`Phase`, `Message` forwarding, `complete_capture`, `workspace_window_settings`, `WorkspaceWindowReady`) — extended with a Timeline phase.
- P0c-1 overlay handoff (`run_action_guide` → `(Recording, InputCapability, CaptureRegion)`; `HostEffect::ActionRecorded`) — consumed unchanged; only the app-side sink changes from "export" to "review then export".
- `rollshot-app` deps `rfd`/`dirs`/`iced[image]` and the `action-guide` feature (pulls `rollshot-action`) — already present; no `Cargo.toml` change.

### Failure modes

| New codepath | Realistic failure | Test coverage | Error handling | User-visible? |
|---|---|---|---|---|
| Export to chosen dir | Disk full / not writable | `export_empty_guide_sets_error_and_writes_nothing` (error path); `export_dir_chosen_writes_guide_folder_and_clears_message` (happy path) | `export_guide` is atomic (temp+rename); `update` sets inline message, stays in Reviewing | Yes (banner) |
| Export empty guide | Guide emptied by deletes | `export_empty_guide_sets_error_and_writes_nothing` | `ExportError::Empty` → message; no folder written | Yes (banner) |
| Folder picker cancelled | User dismisses dialog | `export_cancelled_picker_is_a_no_op` | `ExportDirChosen(None)` → no-op | No |
| Replace keyframe | Frame id not in `nearby` | `replace_keyframe_rejects_frame_outside_nearby` | `Guide::replace_keyframe` returns `false`; no-op | No (only valid strip ids are clickable) |
| Keyframe handle build | Selected keyframe not retained | `new_*` / handle smoke via `view_builds_*` | `rebuild_selection_handles` leaves `keyframe_handle = None`; view shows "(keyframe unavailable)" | Yes (placeholder text) |
| Delete then export | Selection index stale after renumber | `delete_step_renumbers_and_clamps_selection` | selection clamped to `[1, len]` or `None` | No |
| macOS phase transition | `ActionRecorded` arm | `complete_action_recording_enters_timeline_phase` | closes capture windows, enters `Phase::Timeline`, opens window | N/A (internal) |

**Noted gap (acceptable for P0c-2):** the `dirs::picture_dir()` fallback to `std::env::temp_dir()` is silent — if Pictures is unavailable the picker opens in temp without telling the user. Low impact (the user still picks any directory in the dialog); not worth surfacing.

### Worktree / subagent parallelization strategy

Sequential, no parallelization. Tasks 1→2→3 build the shared module incrementally (each depends on the prior's types). Task 4 (view + Linux wiring) depends on the full `Message` enum from Task 3. Task 5 (macOS) depends on the module being complete (Task 4). Task 6 (delete `action_export` + gates) depends on both platform handlers being rewired (Tasks 4–5). Every task touches `crates/rollshot-app/src/timeline_workspace/` or the two platform handlers, so there is no conflict-free split.

### Completion summary

Plan written:            `docs/superpowers/plans/2026-06-16-action-guide-p0c2-timeline-workspace.md`
Tasks in plan:           6
Files Create/Modify/Del: 3 create / 2 modify / 1 delete
Increments:              shared module (1–3) → Linux view+wiring (4) → macOS wiring (5) → cleanup+gates (6)

## Definition of Done (P0c-2, per delta Decision 4)

- From a finished recording, the Timeline Workspace opens with ordered steps; the user can select a step, edit its title, delete it (with renumbering), and replace its keyframe by clicking a nearby-frame thumbnail — no extra window.
- Discard exits without writing anything; Export Guide opens a directory picker and writes a portable `action-guide/` folder (`steps.md` with relative PNG links, `session.json`, `keyframes/*.png`); export failure keeps the workspace open with an inline message.
- Implemented on both platforms: Linux standalone `timeline_workspace::run`, macOS `Phase::Timeline` in the product daemon. P0c-1's direct-export handler (`action_export.rs`) is removed.
- `fmt --check`, `clippy -D warnings` (feature on), and `test --workspace --features action-guide` pass; the feature-off build compiles and exposes no new command.
- The final summary states which platform was run vs. reasoned (AGENTS.md §8).
