# Action Guide P0a — `rollshot-action` Crate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the platform-neutral `rollshot-action` crate: data models, a pushed-frame store with bounded ring/analysis/retention, a deterministic visual+event step detector, the editable guide model, and the portable Markdown/PNG/`session.json` exporter — all fixture-testable with zero platform, UI, or capture-backend dependencies.

**Architecture:** `rollshot-action` is driven by *pushed* `image::RgbaImage` frames (plus privacy-filtered `TimedSemanticAction`s), never by a `FrameStream` or capture backend, so it stays platform-neutral and CI-testable on every host. Frames flow `ingest → full-res ring buffer + downsampled analysis queue → detector → retained candidate windows → guide → export`. The capture producer never blocks: the analysis queue is latest-useful and drops intermediate work under load while the candidate-window store retains the frames needed for keyframes. Every type carries only privacy-filtered semantic data (no raw key codes, typed text, device names, or paths). This crate is P0a Increment 1 of two; the `rollshot-app` integration is Plan 2 and the platform semantic-input crates are P0b.

**Tech Stack:** Rust (workspace crate, edition 2021), `image = 0.25`, `serde` + `serde_json`, `thiserror`, `tracing`; `rtk cargo test` / `fmt` / `clippy`.

**Spec:** `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`

**Scope:** This plan covers ONLY the `rollshot-action` crate (spec §`rollshot-action`). It does NOT touch `rollshot-app`, `rollshot-cli`, the overlay, the `action-guide` Cargo feature wiring, `SendFrameStream` extraction, or any platform input source — those are Plan 2 (`rollshot-app` integration) and P0b (platform crates). The crate always compiles under the workspace `unsafe_code = "forbid"` lint.

---

## Key Design Decisions (read before starting)

1. **No dependency on `rollshot-capture`.** The spec mandates the crate "does not depend on `FrameStream` or any capture backend." `rollshot-capture` pulls in platform backends (PipeWire/ashpd on Linux, scap/objc2 on macOS). To stay light and platform-neutral, `rollshot-action` defines its **own** plain `CaptureRegion` value type. (`CaptureRegion` does not exist in `rollshot-capture`; its `Region` is locally aliased as `CaptureRegion` only inside the overlay. Plan 2 converts `rollshot_capture::Region → rollshot_action::CaptureRegion` at the app boundary.)

2. **Time is `Millis` (monotonic ms since recording start), not `SystemTime`.** The recorder/app assigns `at_ms` to frames and events. This keeps detection deterministic and fixture-testable, avoids clock APIs, and is privacy-safe in `session.json`.

3. **Image metrics are reimplemented locally.** All luma/downsample/diff helpers in `rollshot-core` are private and behind heavy stitching deps. The crate computes its own BT.601 luma (`0.299R + 0.587G + 0.114B`), block-average downsample, masked luma diff, and changed-area ratio over `image` directly.

4. **`VisualOnlySource` carries a `DegradedReason`.** P0a has no platform source, so the recorder uses `VisualOnlySource::new(DegradedReason::SourceStartFailed)` ("no source started"). In P0b, when a platform source fails, the app constructs `VisualOnlySource::new(reason)` with the real reason. The 4 spec `DegradedReason` variants are NOT expanded.

5. **Export serializes a dedicated `SessionManifest`, never `ActionSession` directly.** This guarantees `session.json` contains only steps/timestamps/reasons/capability and never raw or aggregated input events.

---

## Interface Contract (locked — every task must match these signatures)

These are the canonical public types and signatures. Tasks define them incrementally but must not drift from these names.

```rust
// ----- models.rs -----
pub type Millis = u64;       // ms since recording start
pub type FrameId = u64;      // monotonic per session
pub type CandidateId = u64;  // monotonic per session

pub struct CaptureRegion { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }
pub struct Point { pub x: i32, pub y: i32 }
pub enum MouseButton { Left, Right, Middle, Other }
pub enum SemanticKey { Enter, Tab }
pub enum SemanticAction {
    Click { button: MouseButton, position: Option<Point> },
    ScrollActivity,
    TypingActivity,
    SemanticKey(SemanticKey),
}
pub struct TimedSemanticAction { pub action: SemanticAction, pub at_ms: Millis }
pub enum InputSourceKind { LinuxEvdev, MacosCgEvent, VisualOnly }
pub enum DegradedReason { PermissionDenied, NoInputDevice, SourceStartFailed, RuntimeFailure }
pub enum InputCapability { SemanticEvents, VisualOnly { reason: DegradedReason } }
pub enum CandidateKind { Click, Typing, Scroll, UiChanged }
pub enum DetectReason { ClickConfirmed, TypingSettled, ScrollSettled, VisualChange }
pub struct FrameRef { pub id: FrameId, pub at_ms: Millis }
pub struct CandidateStep { pub id: CandidateId, pub kind: CandidateKind, pub reason: DetectReason,
                           pub at_ms: Millis, pub keyframe: FrameId, pub nearby: Vec<FrameId> }
pub struct GuideStep { pub index: usize, pub title: String, pub kind: CandidateKind,
                       pub reason: DetectReason, pub at_ms: Millis, pub keyframe: FrameId,
                       pub nearby: Vec<FrameId>, pub source: CandidateId }
pub fn default_title(kind: CandidateKind) -> &'static str;

// ----- error.rs -----
pub enum DetectError { /* reserved; detection returns Result for app-level preservation */ }
pub enum ExportError { Io { .. }, Encode { .. }, Empty }

// ----- input.rs -----
pub trait SemanticInputSource: Send {
    fn start(&mut self, region: CaptureRegion) -> Result<InputCapability, DegradedReason>;
    fn poll(&mut self) -> Vec<TimedSemanticAction>;
    fn stop(&mut self);
}
pub struct VisualOnlySource { /* reason */ }
impl VisualOnlySource { pub fn new(reason: DegradedReason) -> Self }

// ----- metrics.rs -----
pub struct Rect { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }
pub struct LumaPlane { pub width: u32, pub height: u32, pub samples: Vec<f32> }
pub fn downsample_luma(image: &image::RgbaImage, target_width: u32) -> LumaPlane;
pub fn masked_luma_diff(a: &LumaPlane, b: &LumaPlane, mask: Option<Rect>) -> f32;          // [0,1]
pub fn changed_area_ratio(a: &LumaPlane, b: &LumaPlane, mask: Option<Rect>, per_sample: f32) -> f32; // [0,1]

// ----- frame_store.rs -----
pub struct StoreConfig { pub ring_capacity, analysis_capacity, analysis_width,
                         pub window_before, window_after, nearby_max }
pub struct AnalysisFrame { pub id: FrameId, pub at_ms: Millis, pub luma: LumaPlane }
pub struct RetainedFrame { pub id: FrameId, pub at_ms: Millis, pub image: image::RgbaImage }
pub struct FrameStore { /* ring + analysis queue + retained map */ }
impl FrameStore {
    pub fn new(config: StoreConfig) -> Self;
    pub fn ingest(&mut self, image: image::RgbaImage, at_ms: Millis) -> FrameId; // pushes ring + analysis, never blocks
    pub fn take_analysis(&mut self) -> Option<AnalysisFrame>;
    pub fn dropped_analysis(&self) -> u64;
    pub fn retain_window(&mut self, center_id: FrameId) -> Vec<FrameId>;
    pub fn retained(&self, id: FrameId) -> Option<&RetainedFrame>;
    pub fn nearby(&self, window: &[FrameId], keyframe: FrameId) -> Vec<FrameId>;
}

// ----- events.rs -----
pub struct EventAggregator { /* coalesce window */ }
impl EventAggregator {
    pub fn new(coalesce_window_ms: Millis) -> Self;
    pub fn push(&mut self, action: TimedSemanticAction);
    pub fn drain(&mut self) -> Vec<TimedSemanticAction>;
}

// ----- detector.rs -----
pub struct DetectorConfig { pub diff_threshold, area_threshold, per_sample_threshold,
                            pub cooldown_ms, click_window_ms, typing_pause_ms,
                            pub scroll_dwell_ms, stable_frames }
pub struct CandidateMarker { pub kind: CandidateKind, pub reason: DetectReason,
                             pub at_ms: Millis, pub center_id: FrameId }
pub struct Detector { /* streaming state */ }
impl Detector {
    pub fn new(config: DetectorConfig) -> Self;
    pub fn observe_event(&mut self, ev: TimedSemanticAction);
    pub fn observe_frame(&mut self, frame: &AnalysisFrame) -> Option<CandidateMarker>;
    pub fn finish(&mut self) -> Option<CandidateMarker>;
}

// ----- recorder.rs -----
// `finish` returns the FrameStore too: export needs the retained keyframe
// pixels. Events are fed straight to the detector (recency preserved);
// EventAggregator is an upstream coalescer for platform sources (P0b), not in
// this in-process detection path.
pub struct Recording { pub candidates: Vec<CandidateStep>, pub store: FrameStore }
pub struct ActionRecorder { /* store + detector + pending + candidates */ }
impl ActionRecorder {
    pub fn new(region: CaptureRegion, store: StoreConfig, det: DetectorConfig) -> Self;
    pub fn ingest_frame(&mut self, image: image::RgbaImage, at_ms: Millis);
    pub fn ingest_event(&mut self, ev: TimedSemanticAction);
    pub fn finish(self) -> Recording;
    pub fn dropped_analysis(&self) -> u64;
}

// ----- guide.rs -----
pub struct Guide { /* steps + frame store handle */ }
impl Guide {
    pub fn from_candidates(candidates: Vec<CandidateStep>) -> Self;
    pub fn steps(&self) -> &[GuideStep];
    pub fn rename(&mut self, index: usize, title: String) -> bool;
    pub fn delete(&mut self, index: usize) -> bool;        // renumbers remaining
    pub fn replace_keyframe(&mut self, index: usize, frame: FrameId) -> bool; // must be in step.nearby
}

// ----- export.rs -----
pub struct SessionManifest { /* capability, source kind, region, steps metadata — NO events */ }
pub fn export_guide(guide: &Guide, store: &FrameStore, region: CaptureRegion,
                    capability: InputCapability, source: InputSourceKind,
                    out_dir: &std::path::Path) -> Result<std::path::PathBuf, ExportError>;
```

---

## File Structure

All under `crates/rollshot-action/`:

| File | Responsibility |
|------|----------------|
| `Cargo.toml` | Crate manifest (workspace-inherited deps + lints) |
| `src/lib.rs` | Crate doc + module decls + public re-exports |
| `src/models.rs` | Geometry, semantic-event, capability, candidate, and guide-step types; `default_title` |
| `src/error.rs` | `DetectError`, `ExportError` (thiserror) |
| `src/diagnostics.rs` | `rollshot::action*` tracing target consts |
| `src/input.rs` | `SemanticInputSource` trait + `VisualOnlySource` no-op |
| `src/metrics.rs` | Luma downsample, masked diff, changed-area ratio |
| `src/frame_store.rs` | Ring buffer + analysis queue + candidate-window retention + nearby/keyframe |
| `src/events.rs` | `EventAggregator` — privacy-safe burst coalescing |
| `src/detector.rs` | Deterministic streaming detector + candidate rules |
| `src/recorder.rs` | `ActionRecorder` orchestrator → `CandidateStep`s |
| `src/guide.rs` | Editable `Guide` model (rename/delete/replace keyframe) |
| `src/export.rs` | `SessionManifest` + atomic Markdown/PNG/JSON export |

Each task adds its module + re-exports to `lib.rs` so every commit compiles and tests pass.

---

## Task 1: Crate scaffold + workspace wiring

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/rollshot-action/Cargo.toml`
- Create: `crates/rollshot-action/src/lib.rs`

- [ ] **Step 1: Add the crate to the workspace members list**

In `/home/noah/rollshot/Cargo.toml`, add `"crates/rollshot-action",` to the `members` array (keep it grouped with the other `crates/*` entries):

```toml
[workspace]
members = [
    "crates/rollshot-core",
    "crates/rollshot-image-document",
    "crates/rollshot-action",
    "crates/rollshot-capture",
    "crates/rollshot-cli",
    "crates/rollshot-app",
    "crates/rollshot-iced-overlay",
    "crates/rollshot-overlay-core",
    "crates/rollshot-macos-oneshot",
]
```

- [ ] **Step 2: Create the crate manifest**

`crates/rollshot-action/Cargo.toml` (mirror the `rollshot-image-document` pattern — inherit metadata + deps + lints from the workspace):

```toml
[package]
name = "rollshot-action"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
image = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 3: Create `src/lib.rs` with the responsibility-boundary doc and a smoke test**

```rust
//! Platform-neutral Action Guide engine: frame ingestion, deterministic step
//! detection, the editable guide model, and export. Owns no windows, dialogs,
//! platform permissions, native event APIs, or capture backend — it is driven
//! by *pushed* `image::RgbaImage` frames plus privacy-filtered semantic events,
//! so it is fully fixture-testable on every CI host. Every public type carries
//! only privacy-filtered data: never raw key codes, typed text, device names,
//! or device paths. See `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        // Scaffold smoke test; modules are added by later tasks.
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 4: Verify the workspace builds and the crate test passes**

Run: `rtk cargo test -p rollshot-action`
Expected: PASS (`crate_builds`), and the crate is now part of the workspace build.

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 5: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-action/Cargo.toml crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): scaffold platform-neutral rollshot-action crate

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Core data models + serde + privacy-by-construction

**Files:**
- Create: `crates/rollshot-action/src/models.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/models.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing model tests**

Add to the bottom of `src/models.rs` (create the file with this test block first):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_action_serde_round_trips_kebab_case() {
        let actions = [
            SemanticAction::Click { button: MouseButton::Left, position: Some(Point { x: 3, y: 4 }) },
            SemanticAction::Click { button: MouseButton::Right, position: None },
            SemanticAction::ScrollActivity,
            SemanticAction::TypingActivity,
            SemanticAction::SemanticKey(SemanticKey::Enter),
        ];
        for a in actions {
            let json = serde_json::to_string(&a).expect("serialize");
            let back: SemanticAction = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(a, back);
        }
        // kebab-case for unit/struct variants and nested keys.
        assert_eq!(serde_json::to_string(&SemanticAction::ScrollActivity).unwrap(), "\"scroll-activity\"");
    }

    #[test]
    fn input_capability_serde_round_trips() {
        let cap = InputCapability::VisualOnly { reason: DegradedReason::PermissionDenied };
        let json = serde_json::to_string(&cap).unwrap();
        assert!(json.contains("visual-only"), "json = {json}");
        assert!(json.contains("permission-denied"), "json = {json}");
        assert_eq!(serde_json::from_str::<InputCapability>(&json).unwrap(), cap);
        assert_eq!(serde_json::to_string(&InputCapability::SemanticEvents).unwrap(), "\"semantic-events\"");
    }

    #[test]
    fn default_titles_match_spec_labels() {
        assert_eq!(default_title(CandidateKind::Click), "Click");
        assert_eq!(default_title(CandidateKind::Typing), "Enter text");
        assert_eq!(default_title(CandidateKind::Scroll), "Scroll");
        assert_eq!(default_title(CandidateKind::UiChanged), "UI changed");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action models`
Expected: FAIL — `SemanticAction` / `default_title` etc. not defined.

- [ ] **Step 3: Implement the models**

Add above the test module in `src/models.rs`:

```rust
//! Platform-neutral Action Guide data models. These types carry only
//! privacy-filtered semantic information: never raw key codes, typed text,
//! device names, or device paths.

/// Milliseconds since recording start. Monotonic; assigned by the recorder.
pub type Millis = u64;
/// Monotonic identifier for a retained frame within one session.
pub type FrameId = u64;
/// Monotonic identifier for a detector candidate within one session.
pub type CandidateId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticKey {
    Enter,
    Tab,
}

/// A privacy-filtered semantic input action. Deliberately carries no raw key
/// code, no Unicode text, and no device identity — ordinary typing collapses to
/// `TypingActivity`; only Enter/Tab survive as semantic keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticAction {
    Click { button: MouseButton, position: Option<Point> },
    ScrollActivity,
    TypingActivity,
    SemanticKey(SemanticKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimedSemanticAction {
    pub action: SemanticAction,
    pub at_ms: Millis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputSourceKind {
    LinuxEvdev,
    MacosCgEvent,
    VisualOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DegradedReason {
    /// macOS Input Monitoring denied, or Linux evdev ACL missing.
    PermissionDenied,
    /// Linux: no readable `/dev/input/event*` device.
    NoInputDevice,
    /// Source could not start (tap creation failed, no reader opened, or — in
    /// P0a — no platform semantic source is wired into the build).
    SourceStartFailed,
    /// Source started but failed mid-session (null tap, all readers died).
    RuntimeFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputCapability {
    SemanticEvents,
    VisualOnly { reason: DegradedReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateKind {
    Click,
    Typing,
    Scroll,
    UiChanged,
}

/// Privacy-safe reason a candidate was created. Never carries coordinates,
/// key values, or text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetectReason {
    ClickConfirmed,
    TypingSettled,
    ScrollSettled,
    VisualChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FrameRef {
    pub id: FrameId,
    pub at_ms: Millis,
}

/// A detector output: one retained candidate with its chosen keyframe and a
/// bounded, ordered set of nearby frames for replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateStep {
    pub id: CandidateId,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe: FrameId,
    pub nearby: Vec<FrameId>,
}

/// A reviewable, editable guide step. `index` is 1-based and renumbered on
/// delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideStep {
    pub index: usize,
    pub title: String,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe: FrameId,
    pub nearby: Vec<FrameId>,
    pub source: CandidateId,
}

/// Default deterministic label for a candidate kind (spec §Timeline Workspace).
pub fn default_title(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::Click => "Click",
        CandidateKind::Typing => "Enter text",
        CandidateKind::Scroll => "Scroll",
        CandidateKind::UiChanged => "UI changed",
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

Replace the `#[cfg(test)] mod tests { ... }` smoke block in `src/lib.rs` with the module + re-exports (keep the crate doc comment at the top):

```rust
mod models;

pub use models::{
    default_title, CandidateId, CandidateKind, CandidateStep, CaptureRegion, DegradedReason,
    DetectReason, FrameId, FrameRef, GuideStep, InputCapability, InputSourceKind, Millis,
    MouseButton, Point, SemanticAction, SemanticKey, TimedSemanticAction,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action models`
Expected: PASS (3 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/models.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add privacy-filtered Action Guide data models

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Errors + tracing targets

**Files:**
- Create: `crates/rollshot-action/src/error.rs`
- Create: `crates/rollshot-action/src/diagnostics.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/error.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing error tests**

Create `src/error.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_error_messages_are_descriptive() {
        let io = ExportError::Io {
            path: "out/steps.md".into(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        };
        assert!(io.to_string().contains("out/steps.md"), "{io}");
        assert_eq!(ExportError::Empty.to_string(), "cannot export a guide with no steps");
    }

    #[test]
    fn detect_error_message_is_actionable() {
        let err = DetectError::Failed { message: "frame decode failed".to_string() };
        assert_eq!(err.to_string(), "detection failed: frame decode failed");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action error`
Expected: FAIL — `ExportError` not defined.

- [ ] **Step 3: Implement the error types**

Add above the test module in `src/error.rs`:

```rust
//! Typed errors for the Action Guide engine. Detection and export return
//! `Result` so the app can preserve the session and surface an actionable error
//! instead of writing a partial export.

/// Detection failure. Reserved so `ActionRecorder::finish`-style entry points
/// can return `Result` in the app integration without a breaking change; the
/// P0a in-process detector does not currently produce these, but the type fixes
/// the seam (spec §Failure Handling: "detection returns a `Result`").
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("detection failed: {message}")]
    Failed { message: String },
}

/// Export failure. On any error, the exporter leaves no partial `action-guide/`
/// directory and the editable session stays intact (spec §Export).
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("export I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to encode PNG at {path}: {source}")]
    Encode {
        path: String,
        #[source]
        source: image::ImageError,
    },
    #[error("cannot export a guide with no steps")]
    Empty,
}
```

- [ ] **Step 4: Create the tracing targets**

Create `src/diagnostics.rs` (stable explicit `rollshot::*` targets per AGENTS.md §7):

```rust
//! Stable explicit tracing targets for the Action Guide engine. Diagnostics
//! record only capability, source category, counts, and lifecycle outcomes —
//! never key values, typed text, click coordinates, frame contents, or paths.

pub(crate) const TARGET_ACTION: &str = "rollshot::action";
pub(crate) const TARGET_EXPORT: &str = "rollshot::action::export";
```

(Only these two targets are declared because only the recorder and exporter emit
diagnostics. Add a `rollshot::action::detector` target later only if the
detector starts emitting `trace`-level events — an unused const would fail
clippy `-D warnings`.)

- [ ] **Step 5: Wire modules into `lib.rs`**

Add to `src/lib.rs`:

```rust
mod diagnostics;
mod error;

pub use error::{DetectError, ExportError};
```

(`diagnostics` stays private — only used internally. `TARGET_ACTION` is used by
the recorder (Task 10) and `TARGET_EXPORT` by the exporter (Task 12), so neither
const is dead by the time the crate is complete.)

- [ ] **Step 6: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action error`
Expected: PASS.

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/error.rs crates/rollshot-action/src/diagnostics.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add typed errors and tracing targets

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: `SemanticInputSource` trait + `VisualOnlySource`

This fixes the platform seam before any platform code exists (spec §Implementation Increments P0a). The P0b crates (`rollshot-linux-input`, `rollshot-macos-input`) will implement this same trait.

**Files:**
- Create: `crates/rollshot-action/src/input.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/input.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing input-source tests**

Create `src/input.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CaptureRegion, DegradedReason, InputCapability};

    fn region() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 100, height: 80 }
    }

    #[test]
    fn visual_only_source_starts_visual_only_and_polls_empty() {
        let mut src = VisualOnlySource::new(DegradedReason::SourceStartFailed);
        let cap = src.start(region()).expect("visual-only start never errors");
        assert_eq!(cap, InputCapability::VisualOnly { reason: DegradedReason::SourceStartFailed });
        assert!(src.poll().is_empty());
        src.stop();
        assert!(src.poll().is_empty());
    }

    #[test]
    fn visual_only_source_preserves_p0b_fallback_reason() {
        let mut src = VisualOnlySource::new(DegradedReason::PermissionDenied);
        let cap = src.start(region()).unwrap();
        assert_eq!(cap, InputCapability::VisualOnly { reason: DegradedReason::PermissionDenied });
    }

    #[test]
    fn semantic_input_source_is_object_safe_and_send() {
        // Compile-time proof the trait is usable as `Box<dyn SemanticInputSource>`
        // and is `Send` (so it can move onto the app's input thread).
        fn assert_send<T: Send>() {}
        assert_send::<Box<dyn SemanticInputSource>>();
        let _boxed: Box<dyn SemanticInputSource> =
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action input`
Expected: FAIL — `SemanticInputSource` / `VisualOnlySource` not defined.

- [ ] **Step 3: Implement the trait and the no-op source**

Add above the test module in `src/input.rs`:

```rust
//! The cross-platform semantic-input seam. `rollshot-action` depends only on
//! this trait; platform implementations live in the P0b crates and push
//! privacy-filtered, burst-aggregated actions. `VisualOnlySource` is the no-op
//! used when no semantic source is available — P0a always uses it.

use crate::models::{CaptureRegion, DegradedReason, InputCapability, TimedSemanticAction};

pub trait SemanticInputSource: Send {
    /// Begin observing input for `region`. On `Err`, the caller falls back to
    /// `InputCapability::VisualOnly { reason }` and recording continues.
    fn start(&mut self, region: CaptureRegion) -> Result<InputCapability, DegradedReason>;
    /// Drain semantic actions observed since the last poll. Never returns raw
    /// key codes, typed text, device names, or device paths.
    fn poll(&mut self) -> Vec<TimedSemanticAction>;
    /// Disable the source and release any native resources.
    fn stop(&mut self);
}

/// No-op source: produces no semantic events and always reports visual-only.
/// P0a uses `DegradedReason::SourceStartFailed` ("no platform source wired");
/// in P0b the app constructs it with the real fallback reason when a platform
/// source fails.
#[derive(Debug, Clone, Copy)]
pub struct VisualOnlySource {
    reason: DegradedReason,
}

impl VisualOnlySource {
    pub fn new(reason: DegradedReason) -> Self {
        Self { reason }
    }
}

impl Default for VisualOnlySource {
    fn default() -> Self {
        Self { reason: DegradedReason::SourceStartFailed }
    }
}

impl SemanticInputSource for VisualOnlySource {
    fn start(&mut self, _region: CaptureRegion) -> Result<InputCapability, DegradedReason> {
        Ok(InputCapability::VisualOnly { reason: self.reason })
    }

    fn poll(&mut self) -> Vec<TimedSemanticAction> {
        Vec::new()
    }

    fn stop(&mut self) {}
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

```rust
mod input;

pub use input::{SemanticInputSource, VisualOnlySource};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action input`
Expected: PASS (3 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/input.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add SemanticInputSource seam and VisualOnlySource

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Visual metrics — luma downsample, masked diff, changed-area ratio

Reimplemented locally (the `rollshot-core` equivalents are private behind stitching deps). BT.601 weights match `rollshot-core/src/matcher.rs:1190`. `div_ceil` is stable on the workspace MSRV; manual `(a + b - 1) / b` would trip clippy's `manual_div_ceil`.

**Files:**
- Create: `crates/rollshot-action/src/metrics.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/metrics.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing metrics tests**

Create `src/metrics.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    #[test]
    fn downsample_keeps_dims_when_target_exceeds_source() {
        let img = solid(8, 6, [255, 255, 255, 255]);
        let plane = downsample_luma(&img, 384);
        assert_eq!((plane.width, plane.height), (8, 6));
        // White luma ≈ 255.
        assert!((plane.samples[0] - 255.0).abs() < 0.5);
    }

    #[test]
    fn identical_planes_have_zero_diff_and_zero_changed_area() {
        let a = downsample_luma(&solid(8, 8, [10, 20, 30, 255]), 384);
        let b = a.clone();
        assert_eq!(masked_luma_diff(&a, &b, None), 0.0);
        assert_eq!(changed_area_ratio(&a, &b, None, 12.0), 0.0);
    }

    #[test]
    fn changed_quadrant_yields_expected_area_ratio() {
        // 8x8 black; flip the top-left 4x4 quadrant to white.
        let base = solid(8, 8, [0, 0, 0, 255]);
        let mut changed = base.clone();
        for y in 0..4 {
            for x in 0..4 {
                changed.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let a = downsample_luma(&base, 384);
        let b = downsample_luma(&changed, 384);
        // 16 of 64 samples changed.
        assert!((changed_area_ratio(&a, &b, None, 12.0) - 0.25).abs() < 1e-6);
        assert!(masked_luma_diff(&a, &b, None) > 0.0);
    }

    #[test]
    fn mask_excludes_changed_region_from_metrics() {
        let base = solid(8, 8, [0, 0, 0, 255]);
        let mut changed = base.clone();
        for y in 0..4 {
            for x in 0..4 {
                changed.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let a = downsample_luma(&base, 384);
        let b = downsample_luma(&changed, 384);
        let mask = Some(Rect { x: 0, y: 0, width: 4, height: 4 });
        assert_eq!(changed_area_ratio(&a, &b, mask, 12.0), 0.0);
        assert_eq!(masked_luma_diff(&a, &b, mask), 0.0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action metrics`
Expected: FAIL — `downsample_luma` etc. not defined.

- [ ] **Step 3: Implement the metrics**

Add above the test module in `src/metrics.rs`:

```rust
//! Deterministic, allocation-light visual metrics over `image::RgbaImage`.
//! Used by the detector on downsampled luma planes. BT.601 luma weights.

use image::RgbaImage;

/// A rectangle in downsampled-plane sample coordinates (used as a cursor mask).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A downsampled luma plane: row-major `f32` samples in `[0, 255]`.
#[derive(Debug, Clone, PartialEq)]
pub struct LumaPlane {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<f32>,
}

#[inline]
fn luma(r: u8, g: u8, b: u8) -> f32 {
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// Block-average downsample to luma. The plane width is at most `target_width`
/// (block size `ceil(src_width / target_width)`, min 1); aspect ratio is
/// preserved. If `target_width >= src_width`, the block size is 1 (no
/// downsample), so small fixtures map 1:1 to luma samples.
pub fn downsample_luma(image: &RgbaImage, target_width: u32) -> LumaPlane {
    let sw = image.width();
    let sh = image.height();
    if sw == 0 || sh == 0 || target_width == 0 {
        return LumaPlane { width: 0, height: 0, samples: Vec::new() };
    }
    let block = sw.div_ceil(target_width).max(1);
    let width = sw.div_ceil(block);
    let height = sh.div_ceil(block);
    let mut samples = Vec::with_capacity((width * height) as usize);
    for by in 0..height {
        for bx in 0..width {
            let x0 = bx * block;
            let y0 = by * block;
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for y in y0..(y0 + block).min(sh) {
                for x in x0..(x0 + block).min(sw) {
                    let p = image.get_pixel(x, y).0;
                    sum += luma(p[0], p[1], p[2]);
                    count += 1;
                }
            }
            samples.push(if count > 0 { sum / count as f32 } else { 0.0 });
        }
    }
    LumaPlane { width, height, samples }
}

#[inline]
fn in_mask(mask: Option<Rect>, x: u32, y: u32) -> bool {
    match mask {
        Some(r) => x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height,
        None => false,
    }
}

/// Mean absolute luma difference over unmasked samples, normalized to `[0, 1]`.
/// Returns `0.0` on dimension mismatch or empty planes.
pub fn masked_luma_diff(a: &LumaPlane, b: &LumaPlane, mask: Option<Rect>) -> f32 {
    if a.width != b.width || a.height != b.height || a.samples.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0f32;
    let mut count = 0u32;
    for y in 0..a.height {
        for x in 0..a.width {
            if in_mask(mask, x, y) {
                continue;
            }
            let i = (y * a.width + x) as usize;
            sum += (a.samples[i] - b.samples[i]).abs();
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f32) / 255.0
    }
}

/// Fraction of unmasked samples whose absolute luma delta exceeds
/// `per_sample` (in `[0, 255]` units). Result in `[0, 1]`.
pub fn changed_area_ratio(a: &LumaPlane, b: &LumaPlane, mask: Option<Rect>, per_sample: f32) -> f32 {
    if a.width != b.width || a.height != b.height || a.samples.is_empty() {
        return 0.0;
    }
    let mut changed = 0u32;
    let mut count = 0u32;
    for y in 0..a.height {
        for x in 0..a.width {
            if in_mask(mask, x, y) {
                continue;
            }
            let i = (y * a.width + x) as usize;
            if (a.samples[i] - b.samples[i]).abs() > per_sample {
                changed += 1;
            }
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        changed as f32 / count as f32
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

```rust
mod metrics;

pub use metrics::{changed_area_ratio, downsample_luma, masked_luma_diff, LumaPlane, Rect};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action metrics`
Expected: PASS (4 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/metrics.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add luma downsample and changed-area visual metrics

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Frame store — ring buffer, analysis queue, candidate-window retention

Implements the bounded storage from spec §Frame Pipeline And Temporary Storage. The capture producer never blocks: `ingest` always returns, dropping the oldest analysis frame under load while the full-res ring and retained windows stay bounded.

**Files:**
- Create: `crates/rollshot-action/src/frame_store.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/frame_store.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing frame-store tests**

Create `src/frame_store.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn frame(v: u8) -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([v, v, v, 255]))
    }

    fn small_store() -> FrameStore {
        FrameStore::new(StoreConfig {
            ring_capacity: 10,
            analysis_capacity: 4,
            analysis_width: 384,
            window_before: 2,
            window_after: 3,
            nearby_max: 3,
        })
    }

    #[test]
    fn ring_buffer_is_bounded_and_overwrites_oldest() {
        let mut store = small_store();
        for i in 0..15u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        assert_eq!(store.ring.len(), 10);
        // Oldest retained ring frame is id 5 (15 ingested, capacity 10).
        assert_eq!(store.ring.front().unwrap().id, 5);
        assert_eq!(store.ring.back().unwrap().id, 14);
    }

    #[test]
    fn analysis_queue_drops_intermediate_under_load_without_blocking_capture() {
        let mut store = small_store();
        // Ingest far more than analysis_capacity WITHOUT draining (slow detector).
        for i in 0..20u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        // Queue stays bounded; intermediate analysis work was dropped.
        assert_eq!(store.analysis.len(), 4);
        assert_eq!(store.dropped_analysis(), 16);
        // Latest-useful: the newest frame is still queued for the detector.
        assert_eq!(store.analysis.back().unwrap().id, 19);
    }

    #[test]
    fn take_analysis_returns_oldest_queued_then_none() {
        let mut store = small_store();
        store.ingest(frame(1), 100);
        store.ingest(frame(2), 200);
        assert_eq!(store.take_analysis().unwrap().id, 0);
        assert_eq!(store.take_analysis().unwrap().id, 1);
        assert!(store.take_analysis().is_none());
    }

    #[test]
    fn retain_window_copies_before_and_after_and_survives_ring_eviction() {
        let mut store = small_store();
        for i in 0..8u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        // center=4: window_before 2 -> {2,3}, center 4, window_after 3 -> {5,6,7}.
        let ids = store.retain_window(4);
        assert_eq!(ids, vec![2, 3, 4, 5, 6, 7]);
        assert!(store.retained(2).is_some());
        assert!(store.retained(0).is_none());
        // Evict the ring well past those ids; retained frames persist.
        for i in 8..30u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        assert!(store.ring.iter().all(|f| f.id != 2));
        assert!(store.retained(2).is_some(), "retained window must outlive the ring");
        assert_eq!(store.retained(4).unwrap().at_ms, 400);
    }

    #[test]
    fn nearby_is_bounded_and_ordered_and_centered_on_keyframe() {
        let store = small_store();
        let window = vec![2u64, 3, 4, 5, 6, 7];
        // nearby_max = 3, keyframe 4 -> centered window [3,4,5].
        assert_eq!(store.nearby(&window, 4), vec![3, 4, 5]);
        // Small windows are returned whole.
        assert_eq!(store.nearby(&[9, 10], 9), vec![9, 10]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action frame_store`
Expected: FAIL — `FrameStore` not defined.

- [ ] **Step 3: Implement the frame store**

Add above the test module in `src/frame_store.rs`:

```rust
//! Bounded temporary frame storage: a continuously-overwritten full-resolution
//! ring buffer, a latest-useful downsampled analysis queue that drops
//! intermediate work under load (so capture never blocks), and long-lived
//! retained candidate windows copied out of the ring. All bounds are fixed and
//! independent of session length (spec §Fixed Bounds And Capture Rate).

use std::collections::{BTreeMap, VecDeque};

use image::RgbaImage;

use crate::metrics::{downsample_luma, LumaPlane};
use crate::models::{FrameId, Millis};

#[derive(Debug, Clone)]
pub struct StoreConfig {
    /// Full-res rolling window depth (continuously overwritten).
    pub ring_capacity: usize,
    /// Downsampled analysis queue cap; oldest dropped under load.
    pub analysis_capacity: usize,
    /// Target downsample width for analysis luma planes.
    pub analysis_width: u32,
    /// Frames retained before a candidate center.
    pub window_before: usize,
    /// Frames retained after a candidate center.
    pub window_after: usize,
    /// Max frames in a nearby-replacement strip.
    pub nearby_max: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            ring_capacity: 60,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 4,
            window_after: 8,
            nearby_max: 7,
        }
    }
}

#[derive(Debug, Clone)]
struct RingFrame {
    id: FrameId,
    at_ms: Millis,
    image: RgbaImage,
}

/// A downsampled luma frame queued for the detector.
#[derive(Debug, Clone)]
pub struct AnalysisFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub luma: LumaPlane,
}

/// A full-resolution frame retained around a candidate window.
#[derive(Debug, Clone)]
pub struct RetainedFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub image: RgbaImage,
}

pub struct FrameStore {
    config: StoreConfig,
    ring: VecDeque<RingFrame>,
    analysis: VecDeque<AnalysisFrame>,
    retained: BTreeMap<FrameId, RetainedFrame>,
    dropped: u64,
    next_id: FrameId,
}

impl FrameStore {
    pub fn new(config: StoreConfig) -> Self {
        Self {
            config,
            ring: VecDeque::new(),
            analysis: VecDeque::new(),
            retained: BTreeMap::new(),
            dropped: 0,
            next_id: 0,
        }
    }

    /// Push a cropped full-res frame. Stores it in the ring and enqueues a
    /// downsampled analysis frame. Never blocks: if the analysis queue is full,
    /// the oldest queued frame is dropped (latest-useful). Returns the frame id.
    pub fn ingest(&mut self, image: RgbaImage, at_ms: Millis) -> FrameId {
        let id = self.next_id;
        self.next_id += 1;
        let luma = downsample_luma(&image, self.config.analysis_width);

        self.ring.push_back(RingFrame { id, at_ms, image });
        if self.ring.len() > self.config.ring_capacity {
            self.ring.pop_front();
        }

        self.analysis.push_back(AnalysisFrame { id, at_ms, luma });
        if self.analysis.len() > self.config.analysis_capacity {
            self.analysis.pop_front();
            self.dropped += 1;
        }
        id
    }

    /// Pop the oldest queued analysis frame for the detector, if any.
    pub fn take_analysis(&mut self) -> Option<AnalysisFrame> {
        self.analysis.pop_front()
    }

    /// Count of analysis frames dropped under load (for diagnostics).
    pub fn dropped_analysis(&self) -> u64 {
        self.dropped
    }

    /// Copy `[center - window_before, center + window_after]` (clamped to what
    /// is currently in the ring) into long-lived retained storage. Returns the
    /// retained ids in time order. Empty if the center has already rolled out
    /// of the ring.
    pub fn retain_window(&mut self, center_id: FrameId) -> Vec<FrameId> {
        let Some(idx) = self.ring.iter().position(|f| f.id == center_id) else {
            return Vec::new();
        };
        let lo = idx.saturating_sub(self.config.window_before);
        let hi = (idx + self.config.window_after).min(self.ring.len() - 1);
        let mut ids = Vec::new();
        for f in self.ring.iter().take(hi + 1).skip(lo) {
            self.retained.entry(f.id).or_insert_with(|| RetainedFrame {
                id: f.id,
                at_ms: f.at_ms,
                image: f.image.clone(),
            });
            ids.push(f.id);
        }
        ids
    }

    /// Look up a retained frame by id.
    pub fn retained(&self, id: FrameId) -> Option<&RetainedFrame> {
        self.retained.get(&id)
    }

    /// A bounded, time-ordered subset of `window` (size <= `nearby_max`)
    /// centered on `keyframe`, for the replacement strip.
    pub fn nearby(&self, window: &[FrameId], keyframe: FrameId) -> Vec<FrameId> {
        if window.is_empty() {
            return Vec::new();
        }
        let max = self.config.nearby_max.max(1);
        if window.len() <= max {
            return window.to_vec();
        }
        let idx = window.iter().position(|&f| f == keyframe).unwrap_or(0);
        let half = max / 2;
        let mut lo = idx.saturating_sub(half);
        if lo + max > window.len() {
            lo = window.len() - max;
        }
        window[lo..lo + max].to_vec()
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

```rust
mod frame_store;

pub use frame_store::{AnalysisFrame, FrameStore, RetainedFrame, StoreConfig};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action frame_store`
Expected: PASS (5 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/frame_store.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add bounded frame store with non-blocking analysis queue

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Event aggregator — privacy-safe burst coalescing

De-noises the semantic stream before detection: consecutive `TypingActivity` (and `ScrollActivity`) within a short window collapse to a single representative event; `Click` and Enter/Tab pass through and break a run. Privacy is guaranteed by construction — `SemanticAction` has no field that can carry text or raw codes.

**Files:**
- Create: `crates/rollshot-action/src/events.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/events.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing aggregator tests**

Create `src/events.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MouseButton, SemanticAction, SemanticKey, TimedSemanticAction};

    fn ev(action: SemanticAction, at_ms: u64) -> TimedSemanticAction {
        TimedSemanticAction { action, at_ms }
    }

    #[test]
    fn consecutive_typing_within_window_coalesces_to_one() {
        let mut agg = EventAggregator::new(120);
        for t in [0u64, 50, 100, 150, 200] {
            agg.push(ev(SemanticAction::TypingActivity, t));
        }
        let out = agg.drain();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].at_ms, 0, "keep the earliest timestamp of the run");
    }

    #[test]
    fn a_gap_larger_than_the_window_breaks_the_run() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::TypingActivity, 0));
        agg.push(ev(SemanticAction::TypingActivity, 500));
        assert_eq!(agg.drain().len(), 2);
    }

    #[test]
    fn enter_breaks_a_typing_run() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::TypingActivity, 0));
        agg.push(ev(SemanticAction::TypingActivity, 50));
        agg.push(ev(SemanticAction::SemanticKey(SemanticKey::Enter), 60));
        agg.push(ev(SemanticAction::TypingActivity, 70));
        let out = agg.drain();
        let kinds: Vec<_> = out.iter().map(|e| e.action).collect();
        assert_eq!(
            kinds,
            vec![
                SemanticAction::TypingActivity,
                SemanticAction::SemanticKey(SemanticKey::Enter),
                SemanticAction::TypingActivity,
            ]
        );
    }

    #[test]
    fn clicks_pass_through_and_break_runs() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::TypingActivity, 0));
        agg.push(ev(SemanticAction::Click { button: MouseButton::Left, position: None }, 10));
        agg.push(ev(SemanticAction::TypingActivity, 20));
        assert_eq!(agg.drain().len(), 3);
    }

    #[test]
    fn scroll_and_typing_runs_are_independent() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::ScrollActivity, 0));
        agg.push(ev(SemanticAction::ScrollActivity, 40));
        agg.push(ev(SemanticAction::TypingActivity, 60)); // different kind -> new event
        agg.push(ev(SemanticAction::TypingActivity, 90));
        let out = agg.drain();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].action, SemanticAction::ScrollActivity);
        assert_eq!(out[1].action, SemanticAction::TypingActivity);
    }

    #[test]
    fn aggregated_events_never_carry_text_or_raw_codes() {
        let json = serde_json::to_string(&ev(SemanticAction::TypingActivity, 5)).unwrap();
        assert_eq!(json, r#"{"action":"typing-activity","at_ms":5}"#);
        for forbidden in ["text", "unicode", "keycode", "key_code", "device"] {
            assert!(!json.contains(forbidden), "leaked {forbidden}: {json}");
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action events`
Expected: FAIL — `EventAggregator` not defined.

- [ ] **Step 3: Implement the aggregator**

Add above the test module in `src/events.rs`:

```rust
//! Privacy-safe burst coalescing for the semantic event stream. Consecutive
//! `TypingActivity` / `ScrollActivity` events within `window` ms collapse into
//! a single representative event (earliest timestamp). `Click` and Enter/Tab
//! pass through unchanged and end any in-progress activity run. The privacy
//! boundary is the `SemanticAction` shape itself — there is no field able to
//! carry typed text or raw key codes.

use crate::models::{Millis, SemanticAction, TimedSemanticAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivityKind {
    Typing,
    Scroll,
}

fn activity_kind(action: &SemanticAction) -> Option<ActivityKind> {
    match action {
        SemanticAction::TypingActivity => Some(ActivityKind::Typing),
        SemanticAction::ScrollActivity => Some(ActivityKind::Scroll),
        SemanticAction::Click { .. } | SemanticAction::SemanticKey(_) => None,
    }
}

pub struct EventAggregator {
    window: Millis,
    out: Vec<TimedSemanticAction>,
    last_kind: Option<ActivityKind>,
    last_at: Millis,
}

impl EventAggregator {
    pub fn new(coalesce_window_ms: Millis) -> Self {
        Self {
            window: coalesce_window_ms,
            out: Vec::new(),
            last_kind: None,
            last_at: 0,
        }
    }

    pub fn push(&mut self, ev: TimedSemanticAction) {
        match activity_kind(&ev.action) {
            Some(kind)
                if self.last_kind == Some(kind)
                    && ev.at_ms.saturating_sub(self.last_at) <= self.window =>
            {
                // Fold into the in-progress run; slide the window anchor.
                self.last_at = ev.at_ms;
            }
            Some(kind) => {
                self.out.push(ev);
                self.last_kind = Some(kind);
                self.last_at = ev.at_ms;
            }
            None => {
                self.out.push(ev);
                self.last_kind = None;
                self.last_at = ev.at_ms;
            }
        }
    }

    /// Take all coalesced events accumulated so far. Run state persists, so a
    /// run split across drains still coalesces.
    pub fn drain(&mut self) -> Vec<TimedSemanticAction> {
        std::mem::take(&mut self.out)
    }
}

impl Default for EventAggregator {
    fn default() -> Self {
        Self::new(120)
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

```rust
mod events;

pub use events::EventAggregator;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action events`
Expected: PASS (6 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/events.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add privacy-safe event burst aggregator

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Deterministic detector — visual movement/settle core + suppression

The streaming detector core (spec §Deterministic Detection). It tracks frame-to-frame motion, waits for a stable settle, and emits at most one candidate when the settled state differs meaningfully from the rolling baseline. Cursor-only motion and animation oscillation never settle to a new state → no steps. Task 9 adds event-aware classification (click/typing/scroll); this task is the visual-only spine.

**Files:**
- Create: `crates/rollshot-action/src/detector.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/detector.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing detector-core tests**

Create `src/detector.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_store::AnalysisFrame;
    use crate::metrics::LumaPlane;
    use crate::models::{CandidateKind, DetectReason};

    fn cfg() -> DetectorConfig {
        DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            per_sample_threshold: 12.0,
            cooldown_ms: 0,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        }
    }

    fn uniform(v: f32) -> LumaPlane {
        LumaPlane { width: 8, height: 8, samples: vec![v; 64] }
    }
    fn quadrant(base: f32, q: f32) -> LumaPlane {
        let mut s = vec![base; 64];
        for y in 0..4 {
            for x in 0..4 {
                s[y * 8 + x] = q;
            }
        }
        LumaPlane { width: 8, height: 8, samples: s }
    }
    fn one_pixel(base: f32, p: f32) -> LumaPlane {
        let mut s = vec![base; 64];
        s[0] = p;
        LumaPlane { width: 8, height: 8, samples: s }
    }
    fn af(id: u64, at: u64, luma: LumaPlane) -> AnalysisFrame {
        AnalysisFrame { id, at_ms: at, luma }
    }

    /// Feed frames, collect every emitted marker (does not call finish()).
    fn run(det: &mut Detector, frames: Vec<AnalysisFrame>) -> Vec<CandidateMarker> {
        frames.iter().filter_map(|f| det.observe_frame(f)).collect()
    }

    #[test]
    fn change_then_settle_emits_one_ui_changed_candidate() {
        let mut det = Detector::new(cfg());
        let frames = vec![
            af(0, 0, uniform(0.0)),       // baseline
            af(1, 100, quadrant(0.0, 255.0)), // change begins (moving)
            af(2, 200, quadrant(0.0, 255.0)), // stable 1
            af(3, 300, quadrant(0.0, 255.0)), // stable 2 -> settle -> candidate
        ];
        let markers = run(&mut det, frames);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::UiChanged);
        assert_eq!(markers[0].reason, DetectReason::VisualChange);
        assert_eq!(markers[0].center_id, 3);
        assert_eq!(markers[0].at_ms, 300);
    }

    #[test]
    fn identical_frames_emit_no_candidate() {
        let mut det = Detector::new(cfg());
        let frames = (0..6u64).map(|i| af(i, i * 100, uniform(20.0))).collect();
        assert!(run(&mut det, frames).is_empty());
    }

    #[test]
    fn tiny_localized_change_is_below_area_threshold_and_emits_nothing() {
        // A blinking caret / small cursor: 1 of 64 samples flips each frame.
        let mut det = Detector::new(cfg());
        let frames = vec![
            af(0, 0, uniform(0.0)),
            af(1, 100, one_pixel(0.0, 255.0)),
            af(2, 200, uniform(0.0)),
            af(3, 300, one_pixel(0.0, 255.0)),
            af(4, 400, uniform(0.0)),
        ];
        assert!(run(&mut det, frames).is_empty());
    }

    #[test]
    fn oscillation_returning_to_baseline_emits_nothing_even_on_finish() {
        // Spinner-like A<->B that never settles and ends back at baseline A.
        let mut det = Detector::new(cfg());
        let frames = vec![
            af(0, 0, uniform(0.0)),            // A baseline
            af(1, 100, quadrant(0.0, 255.0)),  // B
            af(2, 200, uniform(0.0)),          // A
            af(3, 300, quadrant(0.0, 255.0)),  // B
            af(4, 400, uniform(0.0)),          // A (ends on baseline)
        ];
        let mut markers = run(&mut det, frames);
        if let Some(m) = det.finish() {
            markers.push(m);
        }
        assert!(markers.is_empty(), "oscillation back to baseline is not a step");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action detector`
Expected: FAIL — `Detector` not defined.

- [ ] **Step 3: Implement the detector core**

Add above the test module in `src/detector.rs`:

```rust
//! Deterministic visual step detector. Streaming state machine over downsampled
//! luma frames: detects frame-to-frame motion, waits for a stable settle, and
//! emits a candidate only when the settled state differs meaningfully from the
//! rolling baseline. Cursor-only motion stays below the area threshold and
//! animation that returns to baseline never produces a new stable state, so
//! neither creates a step. Event-aware classification is added in the next
//! task; this core emits `UiChanged` / `VisualChange`.

use crate::frame_store::AnalysisFrame;
use crate::metrics::{changed_area_ratio, masked_luma_diff, LumaPlane};
use crate::models::{CandidateKind, DetectReason, FrameId, Millis};

#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Normalized luma diff above which two frames are "different".
    pub diff_threshold: f32,
    /// Changed-area ratio above which a difference is "meaningful".
    pub area_threshold: f32,
    /// Per-sample luma delta (0..255) counted as a changed sample.
    pub per_sample_threshold: f32,
    /// Minimum ms between successive candidates (debounce).
    pub cooldown_ms: Millis,
    /// Window after a click in which a settle is attributed to the click.
    pub click_window_ms: Millis,
    /// Idle gap that ends a typing burst.
    pub typing_pause_ms: Millis,
    /// Dwell after scroll input before a scroll candidate may form.
    pub scroll_dwell_ms: Millis,
    /// Consecutive low-diff frames required to call the view "settled".
    pub stable_frames: u32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            diff_threshold: 0.012,
            area_threshold: 0.04,
            per_sample_threshold: 12.0,
            cooldown_ms: 400,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        }
    }
}

/// A detected candidate, centered on the settled keyframe. Carries no
/// coordinates, key values, or text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateMarker {
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub center_id: FrameId,
}

pub struct Detector {
    config: DetectorConfig,
    prev: Option<LumaPlane>,
    baseline: Option<LumaPlane>,
    moving: bool,
    stable_count: u32,
    saw_change: bool,
    last_candidate_ms: Option<Millis>,
    last_frame: Option<(FrameId, Millis)>,
}

impl Detector {
    pub fn new(config: DetectorConfig) -> Self {
        Self {
            config,
            prev: None,
            baseline: None,
            moving: false,
            stable_count: 0,
            saw_change: false,
            last_candidate_ms: None,
            last_frame: None,
        }
    }

    fn cooldown_ok(&self, at_ms: Millis) -> bool {
        match self.last_candidate_ms {
            Some(prev) => at_ms.saturating_sub(prev) >= self.config.cooldown_ms,
            None => true,
        }
    }

    /// True if `luma` differs meaningfully (diff + area) from the rolling
    /// baseline.
    fn meaningful_vs_baseline(&self, luma: &LumaPlane) -> bool {
        match &self.baseline {
            Some(b) => {
                masked_luma_diff(b, luma, None) > self.config.diff_threshold
                    && changed_area_ratio(b, luma, None, self.config.per_sample_threshold)
                        > self.config.area_threshold
            }
            None => true,
        }
    }

    /// Observe one analysis frame; returns a candidate if one settles here.
    pub fn observe_frame(&mut self, frame: &AnalysisFrame) -> Option<CandidateMarker> {
        let luma = &frame.luma;
        self.last_frame = Some((frame.id, frame.at_ms));

        // Initialize the baseline on the first frame.
        if self.baseline.is_none() {
            self.baseline = Some(luma.clone());
            self.prev = Some(luma.clone());
            return None;
        }

        let changed = match &self.prev {
            Some(prev) => {
                masked_luma_diff(prev, luma, None) > self.config.diff_threshold
                    && changed_area_ratio(prev, luma, None, self.config.per_sample_threshold)
                        > self.config.area_threshold
            }
            None => false,
        };

        let mut marker = None;

        if changed {
            self.moving = true;
            self.saw_change = true;
            self.stable_count = 0;
        } else if self.moving {
            self.stable_count += 1;
            if self.stable_count >= self.config.stable_frames {
                // Settled. Emit only if meaningfully different from baseline.
                if self.meaningful_vs_baseline(luma) && self.saw_change && self.cooldown_ok(frame.at_ms)
                {
                    self.last_candidate_ms = Some(frame.at_ms);
                    marker = Some(CandidateMarker {
                        kind: CandidateKind::UiChanged,
                        reason: DetectReason::VisualChange,
                        at_ms: frame.at_ms,
                        center_id: frame.id,
                    });
                }
                self.moving = false;
                self.saw_change = false;
                self.baseline = Some(luma.clone());
            }
        }

        self.prev = Some(luma.clone());
        marker
    }

    /// Flush a final candidate if recording ends mid-change on a state that
    /// still differs from baseline.
    pub fn finish(&mut self) -> Option<CandidateMarker> {
        if self.moving && self.saw_change {
            let (Some(luma), Some((id, at))) = (self.prev.clone(), self.last_frame) else {
                return None;
            };
            if self.meaningful_vs_baseline(&luma) && self.cooldown_ok(at) {
                self.moving = false;
                self.saw_change = false;
                self.last_candidate_ms = Some(at);
                return Some(CandidateMarker {
                    kind: CandidateKind::UiChanged,
                    reason: DetectReason::VisualChange,
                    at_ms: at,
                    center_id: id,
                });
            }
        }
        None
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

```rust
mod detector;

pub use detector::{CandidateMarker, Detector, DetectorConfig};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action detector`
Expected: PASS (4 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/detector.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add deterministic visual settle detector core

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Detector event awareness — click / typing / scroll / drag

Extends the detector with the semantic-event rules from spec §Deterministic Detection. Click settles within a confirmation window become `Click`; typing bursts merge into one `Typing` candidate ending on pause/Enter/Tab/finish; scrolling suppresses candidates until a settled dwell with a meaningful change vs the pre-scroll state; a drag collapses to its stable end state. The Task 8 visual-only tests stay green.

**Files:**
- Modify: `crates/rollshot-action/src/detector.rs` (replace `struct Detector` + `impl Detector`; add `observe_event`)
- Test: `crates/rollshot-action/src/detector.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Add the failing event-rule tests**

Add these helpers and tests inside the existing `#[cfg(test)] mod tests` in `src/detector.rs` (alongside the Task 8 tests; add the new imports at the top of the test module):

```rust
    use crate::models::{MouseButton, SemanticAction, SemanticKey, TimedSemanticAction};

    fn ev(action: SemanticAction, at: u64) -> TimedSemanticAction {
        TimedSemanticAction { action, at_ms: at }
    }

    #[test]
    fn click_then_visual_settle_is_a_confirmed_click() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::Click { button: MouseButton::Left, position: None }, 100));
        let frames = vec![
            af(1, 150, quadrant(0.0, 255.0)),
            af(2, 250, quadrant(0.0, 255.0)),
            af(3, 350, quadrant(0.0, 255.0)), // settle within click window [100, 700]
        ];
        let markers: Vec<_> = frames.iter().filter_map(|f| det.observe_frame(f)).collect();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::Click);
        assert_eq!(markers[0].reason, DetectReason::ClickConfirmed);
    }

    #[test]
    fn click_without_visual_change_is_not_a_step() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::Click { button: MouseButton::Left, position: None }, 100));
        let frames = vec![af(1, 150, uniform(0.0)), af(2, 250, uniform(0.0))];
        let markers: Vec<_> = frames.iter().filter_map(|f| det.observe_frame(f)).collect();
        assert!(markers.is_empty());
    }

    #[test]
    fn typing_burst_merges_into_one_step_ending_on_pause() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 255.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 200));
        let mut markers = Vec::new();
        for f in [
            af(2, 200, quadrant(0.0, 255.0)),
            af(3, 300, quadrant(0.0, 255.0)),  // settle, suppressed (in typing)
            af(4, 1000, quadrant(0.0, 255.0)), // pause >= 700ms from last typing -> Typing step
        ] {
            if let Some(m) = det.observe_frame(&f) {
                markers.push(m);
            }
        }
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::Typing);
        assert_eq!(markers[0].reason, DetectReason::TypingSettled);
    }

    #[test]
    fn enter_ends_a_typing_burst() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 50));
        det.observe_frame(&af(1, 50, quadrant(0.0, 255.0)));
        det.observe_event(ev(SemanticAction::SemanticKey(SemanticKey::Enter), 60));
        let m = det.observe_frame(&af(2, 100, quadrant(0.0, 255.0)));
        assert_eq!(m.map(|m| m.kind), Some(CandidateKind::Typing));
    }

    #[test]
    fn scroll_emits_one_step_only_after_settle_with_meaningful_change() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0))); // pre-scroll baseline A
        det.observe_event(ev(SemanticAction::ScrollActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 100.0))); // moving
        det.observe_event(ev(SemanticAction::ScrollActivity, 200));
        det.observe_frame(&af(2, 200, quadrant(0.0, 200.0))); // moving
        det.observe_event(ev(SemanticAction::ScrollActivity, 300));
        let mut markers = Vec::new();
        for f in [
            af(3, 300, quadrant(0.0, 255.0)), // moving
            af(4, 400, quadrant(0.0, 255.0)), // stable 1
            af(5, 500, quadrant(0.0, 255.0)), // stable 2 -> settle, suppressed (in scroll)
            af(6, 1000, quadrant(0.0, 255.0)), // dwell >= 600ms past last scroll -> Scroll step
        ] {
            if let Some(m) = det.observe_frame(&f) {
                markers.push(m);
            }
        }
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::Scroll);
        assert_eq!(markers[0].reason, DetectReason::ScrollSettled);
    }

    #[test]
    fn drag_collapses_to_one_step_at_the_stable_end_state() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::Click { button: MouseButton::Left, position: None }, 50));
        let mut markers = Vec::new();
        for f in [
            af(1, 100, quadrant(0.0, 50.0)),  // drag motion
            af(2, 200, quadrant(0.0, 100.0)),
            af(3, 300, quadrant(0.0, 150.0)),
            af(4, 400, quadrant(0.0, 200.0)),
            af(5, 500, quadrant(0.0, 200.0)), // stable 1
            af(6, 600, quadrant(0.0, 200.0)), // stable 2 -> settle (within click window) -> one step
        ] {
            if let Some(m) = det.observe_frame(&f) {
                markers.push(m);
            }
        }
        assert_eq!(markers.len(), 1, "drag must not create intermediate steps");
        assert_eq!(markers[0].kind, CandidateKind::Click);
        assert_eq!(markers[0].center_id, 6);
    }

    #[test]
    fn tab_ends_a_typing_burst() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 50));
        det.observe_frame(&af(1, 50, quadrant(0.0, 255.0)));
        det.observe_event(ev(SemanticAction::SemanticKey(SemanticKey::Tab), 60));
        let m = det.observe_frame(&af(2, 100, quadrant(0.0, 255.0)));
        assert_eq!(m.map(|m| m.kind), Some(CandidateKind::Typing));
    }

    #[test]
    fn typing_burst_closes_on_finish_when_no_pause_occurs() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 255.0)));
        // No terminating pause / Enter / Tab; recording ends -> finish flushes Typing.
        let m = det.finish();
        assert_eq!(m.map(|m| m.kind), Some(CandidateKind::Typing));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action detector`
Expected: FAIL — `observe_event` not defined / new behaviors missing.

- [ ] **Step 3: Replace the detector struct and impl with the event-aware version**

Keep the module doc, `DetectorConfig`, and `CandidateMarker` from Task 8. Edit the existing models import line to add the two new names — it becomes `use crate::models::{CandidateKind, DetectReason, FrameId, Millis, SemanticAction, TimedSemanticAction};` (do **not** add a second `use crate::models::...` line — that is a duplicate-import compile error). Then replace the entire `pub struct Detector { .. }` and its `impl Detector { .. }` with the following (the `motion` free function goes just above the struct):

```rust
/// Frame-to-frame motion test: meaningful diff AND meaningful changed area.
fn motion(a: &LumaPlane, b: &LumaPlane, config: &DetectorConfig) -> bool {
    masked_luma_diff(a, b, None) > config.diff_threshold
        && changed_area_ratio(a, b, None, config.per_sample_threshold) > config.area_threshold
}

pub struct Detector {
    config: DetectorConfig,
    prev: Option<LumaPlane>,
    baseline: Option<LumaPlane>,
    moving: bool,
    stable_count: u32,
    saw_change: bool,
    last_candidate_ms: Option<Millis>,
    last_frame: Option<(FrameId, Millis)>,
    // event sessions
    click_open_until: Option<Millis>,
    in_typing: bool,
    typing_last_at: Millis,
    typing_force_end: bool,
    in_scroll: bool,
    scroll_last_at: Millis,
    pre_scroll_baseline: Option<LumaPlane>,
}

impl Detector {
    pub fn new(config: DetectorConfig) -> Self {
        Self {
            config,
            prev: None,
            baseline: None,
            moving: false,
            stable_count: 0,
            saw_change: false,
            last_candidate_ms: None,
            last_frame: None,
            click_open_until: None,
            in_typing: false,
            typing_last_at: 0,
            typing_force_end: false,
            in_scroll: false,
            scroll_last_at: 0,
            pre_scroll_baseline: None,
        }
    }

    fn cooldown_ok(&self, at_ms: Millis) -> bool {
        match self.last_candidate_ms {
            Some(prev) => at_ms.saturating_sub(prev) >= self.config.cooldown_ms,
            None => true,
        }
    }

    fn meaningful_vs_baseline(&self, luma: &LumaPlane) -> bool {
        match &self.baseline {
            Some(b) => motion(b, luma, &self.config),
            None => true,
        }
    }

    fn click_consume(&mut self, at_ms: Millis) -> bool {
        match self.click_open_until {
            Some(until) if at_ms <= until => {
                self.click_open_until = None;
                true
            }
            _ => false,
        }
    }

    /// Observe a privacy-filtered semantic event. Opens click windows and
    /// typing/scroll sessions; never inspects key values or text.
    pub fn observe_event(&mut self, ev: TimedSemanticAction) {
        match ev.action {
            SemanticAction::Click { .. } => {
                self.click_open_until = Some(ev.at_ms.saturating_add(self.config.click_window_ms));
            }
            SemanticAction::TypingActivity => {
                self.in_typing = true;
                self.typing_last_at = ev.at_ms;
            }
            SemanticAction::SemanticKey(_) => {
                if self.in_typing {
                    self.typing_last_at = ev.at_ms;
                    self.typing_force_end = true;
                }
            }
            SemanticAction::ScrollActivity => {
                if !self.in_scroll {
                    self.in_scroll = true;
                    self.pre_scroll_baseline = self.baseline.clone();
                }
                self.scroll_last_at = ev.at_ms;
            }
        }
    }

    pub fn observe_frame(&mut self, frame: &AnalysisFrame) -> Option<CandidateMarker> {
        let luma = &frame.luma;
        self.last_frame = Some((frame.id, frame.at_ms));

        if self.baseline.is_none() {
            self.baseline = Some(luma.clone());
            self.prev = Some(luma.clone());
            return None;
        }

        // --- movement bookkeeping (runs every frame) ---
        let changed = match &self.prev {
            Some(prev) => motion(prev, luma, &self.config),
            None => false,
        };
        let mut settled_this_frame = false;
        if changed {
            self.moving = true;
            self.saw_change = true;
            self.stable_count = 0;
        } else if self.moving {
            self.stable_count += 1;
            if self.stable_count >= self.config.stable_frames {
                settled_this_frame = true;
                self.moving = false;
            }
        }
        self.prev = Some(luma.clone());

        // --- candidate decision (priority: typing > scroll > generic settle) ---

        // 1. Typing burst ends on Enter/Tab or a long enough pause.
        if self.in_typing
            && (self.typing_force_end
                || frame.at_ms.saturating_sub(self.typing_last_at) >= self.config.typing_pause_ms)
        {
            self.in_typing = false;
            self.typing_force_end = false;
            self.saw_change = false;
            self.baseline = Some(luma.clone());
            if self.cooldown_ok(frame.at_ms) {
                self.last_candidate_ms = Some(frame.at_ms);
                return Some(CandidateMarker {
                    kind: CandidateKind::Typing,
                    reason: DetectReason::TypingSettled,
                    at_ms: frame.at_ms,
                    center_id: frame.id,
                });
            }
            return None;
        }

        // 2. Scroll ends after a settled dwell; compare to the pre-scroll state.
        if self.in_scroll
            && frame.at_ms.saturating_sub(self.scroll_last_at) >= self.config.scroll_dwell_ms
            && !self.moving
        {
            let meaningful = match &self.pre_scroll_baseline {
                Some(b) => motion(b, luma, &self.config),
                None => self.meaningful_vs_baseline(luma),
            };
            self.in_scroll = false;
            self.saw_change = false;
            self.baseline = Some(luma.clone());
            if meaningful && self.cooldown_ok(frame.at_ms) {
                self.last_candidate_ms = Some(frame.at_ms);
                return Some(CandidateMarker {
                    kind: CandidateKind::Scroll,
                    reason: DetectReason::ScrollSettled,
                    at_ms: frame.at_ms,
                    center_id: frame.id,
                });
            }
            return None;
        }

        // 3. Generic settle. Suppressed while a typing/scroll session owns the
        // change; otherwise becomes a Click (if within a click window) or a
        // plain visual change.
        if settled_this_frame && !self.in_typing && !self.in_scroll {
            let meaningful = self.meaningful_vs_baseline(luma);
            self.baseline = Some(luma.clone());
            self.saw_change = false;
            if meaningful && self.cooldown_ok(frame.at_ms) {
                self.last_candidate_ms = Some(frame.at_ms);
                let (kind, reason) = if self.click_consume(frame.at_ms) {
                    (CandidateKind::Click, DetectReason::ClickConfirmed)
                } else {
                    (CandidateKind::UiChanged, DetectReason::VisualChange)
                };
                return Some(CandidateMarker { kind, reason, at_ms: frame.at_ms, center_id: frame.id });
            }
            return None;
        }

        // Settle suppressed by an open session: still advance the baseline so
        // the session end compares against the latest stable state.
        if settled_this_frame {
            self.baseline = Some(luma.clone());
            self.saw_change = false;
        }

        None
    }

    pub fn finish(&mut self) -> Option<CandidateMarker> {
        // An open typing burst closes into one step at recording finish.
        if self.in_typing {
            self.in_typing = false;
            self.typing_force_end = false;
            let Some((id, at)) = self.last_frame else {
                return None;
            };
            if self.cooldown_ok(at) {
                self.last_candidate_ms = Some(at);
                return Some(CandidateMarker {
                    kind: CandidateKind::Typing,
                    reason: DetectReason::TypingSettled,
                    at_ms: at,
                    center_id: id,
                });
            }
            return None;
        }
        // A visual change still in progress flushes if it differs from baseline.
        if self.moving && self.saw_change {
            let (Some(luma), Some((id, at))) = (self.prev.clone(), self.last_frame) else {
                return None;
            };
            if self.meaningful_vs_baseline(&luma) && self.cooldown_ok(at) {
                self.moving = false;
                self.saw_change = false;
                self.last_candidate_ms = Some(at);
                return Some(CandidateMarker {
                    kind: CandidateKind::UiChanged,
                    reason: DetectReason::VisualChange,
                    at_ms: at,
                    center_id: id,
                });
            }
        }
        None
    }
}
```

- [ ] **Step 4: Run the full detector test suite (old + new) to verify it passes**

Run: `rtk cargo test -p rollshot-action detector`
Expected: PASS — the 4 Task 8 core tests plus the 8 event-rule tests.

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/detector.rs
rtk git commit -m "feat(action): add click/typing/scroll/drag detection rules

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: `ActionRecorder` orchestrator → `CandidateStep`s

Ties the pieces together: pushed frames flow into the store and detector; detector markers are held until enough after-frames exist, then resolved into `CandidateStep`s with a retained keyframe + nearby strip. `finish` hands back both the candidates and the `FrameStore` (export needs the retained pixels). Events go straight to the detector to preserve recency.

**Files:**
- Create: `crates/rollshot-action/src/recorder.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/recorder.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing recorder tests**

Create `src/recorder.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::StoreConfig;
    use crate::models::{CandidateKind, CaptureRegion};
    use image::{Rgba, RgbaImage};

    fn region() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 8, height: 8 }
    }
    fn black() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))
    }
    fn quadrant() -> RgbaImage {
        let mut img = black();
        for y in 0..4 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }
    fn cfg() -> DetectorConfig {
        DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            per_sample_threshold: 12.0,
            cooldown_ms: 0,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        }
    }
    fn store_cfg() -> StoreConfig {
        StoreConfig {
            ring_capacity: 30,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 2,
            window_after: 2,
            nearby_max: 3,
        }
    }

    #[test]
    fn visual_only_recording_produces_one_deterministic_step_with_retained_keyframe() {
        let mut rec = ActionRecorder::new(region(), store_cfg(), cfg());
        let frames = [black(), quadrant(), quadrant(), quadrant(), quadrant(), quadrant(), quadrant()];
        for (i, f) in frames.into_iter().enumerate() {
            rec.ingest_frame(f, i as u64 * 100);
        }
        let recording = rec.finish();
        assert_eq!(recording.candidates.len(), 1);
        let step = &recording.candidates[0];
        assert_eq!(step.kind, CandidateKind::UiChanged);
        assert!(!step.nearby.is_empty() && step.nearby.len() <= 3);
        assert!(step.nearby.windows(2).all(|w| w[0] < w[1]), "nearby is time-ordered");
        assert!(step.nearby.contains(&step.keyframe));
        assert!(recording.store.retained(step.keyframe).is_some());
    }

    #[test]
    fn ingest_never_blocks_and_every_keyframe_survives_a_burst() {
        let mut rec = ActionRecorder::new(region(), store_cfg(), cfg());
        rec.ingest_frame(black(), 0);
        for i in 1..40u64 {
            rec.ingest_frame(quadrant(), i * 100);
        }
        let recording = rec.finish();
        assert!(!recording.candidates.is_empty());
        for step in &recording.candidates {
            assert!(
                recording.store.retained(step.keyframe).is_some(),
                "every step keyframe must be retained for export"
            );
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action recorder`
Expected: FAIL — `ActionRecorder` not defined.

- [ ] **Step 3: Implement the recorder**

Add above the test module in `src/recorder.rs`:

```rust
//! Orchestrates a recording: pushes cropped frames into the bounded store,
//! drives the detector, and resolves detector markers into `CandidateStep`s
//! once enough after-frames exist to retain a stable window. The producer never
//! blocks — the store absorbs and bounds bursts (see `FrameStore`). `finish`
//! returns the candidates plus the store so export can read keyframe pixels.

use image::RgbaImage;

use crate::detector::{CandidateMarker, Detector, DetectorConfig};
use crate::diagnostics::TARGET_ACTION;
use crate::frame_store::{FrameStore, StoreConfig};
use crate::models::{CandidateId, CandidateStep, CaptureRegion, Millis, TimedSemanticAction};

/// Output of a finished recording: detected candidates and the frame store that
/// still holds their retained keyframe/nearby pixels.
pub struct Recording {
    pub candidates: Vec<CandidateStep>,
    pub store: FrameStore,
}

struct Pending {
    marker: CandidateMarker,
    resolve_at: u64,
}

pub struct ActionRecorder {
    #[allow(dead_code)] // surfaced in session.json by the app (Plan 2)
    region: CaptureRegion,
    store: FrameStore,
    detector: Detector,
    window_after: u64,
    frame_count: u64,
    pending: Vec<Pending>,
    candidates: Vec<CandidateStep>,
    next_candidate_id: CandidateId,
}

impl ActionRecorder {
    pub fn new(region: CaptureRegion, store: StoreConfig, det: DetectorConfig) -> Self {
        let window_after = store.window_after as u64;
        Self {
            region,
            store: FrameStore::new(store),
            detector: Detector::new(det),
            window_after,
            frame_count: 0,
            pending: Vec::new(),
            candidates: Vec::new(),
            next_candidate_id: 0,
        }
    }

    /// Push one cropped full-resolution frame. Always returns immediately.
    pub fn ingest_frame(&mut self, image: RgbaImage, at_ms: Millis) {
        self.store.ingest(image, at_ms);
        self.frame_count += 1;
        while let Some(frame) = self.store.take_analysis() {
            if let Some(marker) = self.detector.observe_frame(&frame) {
                self.pending.push(Pending {
                    marker,
                    resolve_at: self.frame_count + self.window_after,
                });
            }
        }
        self.resolve_ready();
    }

    /// Feed a privacy-filtered semantic event to the detector. (P0a never calls
    /// this — `VisualOnlySource` produces none — but P0b wires real events here.)
    pub fn ingest_event(&mut self, ev: TimedSemanticAction) {
        self.detector.observe_event(ev);
    }

    pub fn dropped_analysis(&self) -> u64 {
        self.store.dropped_analysis()
    }

    pub fn finish(mut self) -> Recording {
        while let Some(frame) = self.store.take_analysis() {
            if let Some(marker) = self.detector.observe_frame(&frame) {
                self.pending.push(Pending { marker, resolve_at: self.frame_count });
            }
        }
        if let Some(marker) = self.detector.finish() {
            self.pending.push(Pending { marker, resolve_at: self.frame_count });
        }
        for p in std::mem::take(&mut self.pending) {
            self.finalize(p.marker);
        }
        Recording { candidates: self.candidates, store: self.store }
    }

    fn resolve_ready(&mut self) {
        let now = self.frame_count;
        let mut still = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            if p.resolve_at <= now {
                self.finalize(p.marker);
            } else {
                still.push(p);
            }
        }
        self.pending = still;
    }

    fn finalize(&mut self, marker: CandidateMarker) {
        let window = self.store.retain_window(marker.center_id);
        if window.is_empty() {
            tracing::debug!(
                target: TARGET_ACTION,
                center = marker.center_id,
                "candidate window unavailable; dropping (bounded loss)"
            );
            return;
        }
        let keyframe = if window.contains(&marker.center_id) {
            marker.center_id
        } else {
            *window.last().expect("non-empty window")
        };
        let nearby = self.store.nearby(&window, keyframe);
        let id = self.next_candidate_id;
        self.next_candidate_id += 1;
        self.candidates.push(CandidateStep {
            id,
            kind: marker.kind,
            reason: marker.reason,
            at_ms: marker.at_ms,
            keyframe,
            nearby,
        });
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

```rust
mod recorder;

pub use recorder::{ActionRecorder, Recording};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action recorder`
Expected: PASS (2 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/recorder.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add ActionRecorder orchestrator producing candidate steps

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: Editable guide model — rename / delete / replace keyframe

The reviewable guide (spec §Timeline Workspace P0 operations). Built from candidates with default labels; supports rename, delete (renumbers), and replacing a step's keyframe with one of its nearby frames.

**Files:**
- Create: `crates/rollshot-action/src/guide.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/guide.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing guide tests**

Create `src/guide.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CandidateKind, CandidateStep, DetectReason};

    fn cand(id: u64, kind: CandidateKind, keyframe: u64, nearby: Vec<u64>) -> CandidateStep {
        CandidateStep {
            id,
            kind,
            reason: DetectReason::VisualChange,
            at_ms: id * 100,
            keyframe,
            nearby,
        }
    }

    #[test]
    fn from_candidates_numbers_steps_and_applies_default_titles() {
        let g = Guide::from_candidates(vec![
            cand(0, CandidateKind::Click, 5, vec![4, 5, 6]),
            cand(1, CandidateKind::Scroll, 12, vec![11, 12, 13]),
        ]);
        assert_eq!(g.steps()[0].index, 1);
        assert_eq!(g.steps()[0].title, "Click");
        assert_eq!(g.steps()[1].index, 2);
        assert_eq!(g.steps()[1].title, "Scroll");
        assert_eq!(g.steps()[0].source, 0);
    }

    #[test]
    fn delete_renumbers_remaining_steps() {
        let mut g = Guide::from_candidates(vec![
            cand(0, CandidateKind::Click, 5, vec![5]),
            cand(1, CandidateKind::Scroll, 12, vec![12]),
            cand(2, CandidateKind::UiChanged, 20, vec![20]),
        ]);
        assert!(g.delete(2));
        assert_eq!(g.steps().iter().map(|s| s.index).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(g.steps()[1].kind, CandidateKind::UiChanged);
        assert!(!g.delete(99));
    }

    #[test]
    fn rename_persists_and_unknown_index_is_rejected() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);
        assert!(g.rename(1, "Open Preferences".to_string()));
        assert_eq!(g.steps()[0].title, "Open Preferences");
        assert!(!g.rename(99, "x".to_string()));
    }

    #[test]
    fn replace_keyframe_only_accepts_a_nearby_frame() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![4, 5, 6])]);
        assert!(g.replace_keyframe(1, 6));
        assert_eq!(g.steps()[0].keyframe, 6);
        assert!(!g.replace_keyframe(1, 99), "frame not in nearby strip is rejected");
        assert_eq!(g.steps()[0].keyframe, 6);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action guide`
Expected: FAIL — `Guide` not defined.

- [ ] **Step 3: Implement the guide model**

Add above the test module in `src/guide.rs`:

```rust
//! The editable, reviewable guide model. Holds ordered steps and supports the
//! P0 workspace operations: rename, delete (with renumbering), and replacing a
//! step's keyframe with one of its retained nearby frames. UI lives in the app;
//! this is the headless model it drives.

use crate::models::{default_title, CandidateStep, FrameId, GuideStep};

pub struct Guide {
    steps: Vec<GuideStep>,
}

impl Guide {
    /// Build a guide from detector candidates, assigning 1-based order and
    /// deterministic default titles.
    pub fn from_candidates(candidates: Vec<CandidateStep>) -> Self {
        let steps = candidates
            .into_iter()
            .enumerate()
            .map(|(i, c)| GuideStep {
                index: i + 1,
                title: default_title(c.kind).to_string(),
                kind: c.kind,
                reason: c.reason,
                at_ms: c.at_ms,
                keyframe: c.keyframe,
                nearby: c.nearby,
                source: c.id,
            })
            .collect();
        Self { steps }
    }

    pub fn steps(&self) -> &[GuideStep] {
        &self.steps
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Set a step's title. Returns false if `index` is unknown.
    pub fn rename(&mut self, index: usize, title: String) -> bool {
        match self.steps.iter_mut().find(|s| s.index == index) {
            Some(step) => {
                step.title = title;
                true
            }
            None => false,
        }
    }

    /// Delete a step and renumber the remainder. Returns false if `index` is
    /// unknown.
    pub fn delete(&mut self, index: usize) -> bool {
        let before = self.steps.len();
        self.steps.retain(|s| s.index != index);
        if self.steps.len() == before {
            return false;
        }
        for (i, step) in self.steps.iter_mut().enumerate() {
            step.index = i + 1;
        }
        true
    }

    /// Replace a step's keyframe with `frame`, which must be in that step's
    /// nearby strip. Returns false if the index is unknown or `frame` is not a
    /// retained nearby frame.
    pub fn replace_keyframe(&mut self, index: usize, frame: FrameId) -> bool {
        match self.steps.iter_mut().find(|s| s.index == index) {
            Some(step) if step.nearby.contains(&frame) => {
                step.keyframe = frame;
                true
            }
            _ => false,
        }
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

```rust
mod guide;

pub use guide::Guide;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action guide`
Expected: PASS (4 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/guide.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add editable guide model with rename/delete/replace-keyframe

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: Exporter — Markdown + PNG keyframes + `session.json` (atomic)

Writes the portable `action-guide/` folder (spec §Export). Everything is built in a temporary sibling directory and renamed into place only after every file succeeds; any failure rolls back the temp dir, leaving no partial export and the editable session intact. `session.json` serializes a dedicated `SessionManifest` so it can never contain raw or aggregated input events.

**Files:**
- Create: `crates/rollshot-action/src/export.rs`
- Modify: `crates/rollshot-action/src/frame_store.rs` (add the `#[cfg(test)]` retained-id accessor)
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/export.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing export tests**

Create `src/export.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::{FrameStore, StoreConfig};
    use crate::guide::Guide;
    use crate::models::{
        CandidateKind, CandidateStep, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
    };
    use crate::recorder::ActionRecorder;
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir()
            .join(format!("rollshot-action-{label}-{nanos}-{}", std::process::id()));
        std::fs::create_dir_all(&p).expect("create temp dir");
        p
    }

    fn region() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 8, height: 8 }
    }
    fn black() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))
    }
    fn quadrant() -> RgbaImage {
        let mut img = black();
        for y in 0..4 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }

    /// A real recording yielding exactly one step + a store retaining its frames.
    fn one_step_recording() -> (Guide, FrameStore) {
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            per_sample_threshold: 12.0,
            cooldown_ms: 0,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        };
        let store = StoreConfig {
            ring_capacity: 30,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 2,
            window_after: 2,
            nearby_max: 3,
        };
        let mut rec = ActionRecorder::new(region(), store, det);
        for (i, f) in [black(), quadrant(), quadrant(), quadrant(), quadrant(), quadrant(), quadrant()]
            .into_iter()
            .enumerate()
        {
            rec.ingest_frame(f, i as u64 * 100);
        }
        let recording = rec.finish();
        assert_eq!(recording.candidates.len(), 1);
        (Guide::from_candidates(recording.candidates.clone()), recording.store)
    }

    #[test]
    fn export_writes_portable_folder_with_matching_markdown_and_keyframes() {
        let (guide, store) = one_step_recording();
        let out = temp_dir("export-ok");
        let dir = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::VisualOnly { reason: crate::models::DegradedReason::SourceStartFailed },
            InputSourceKind::VisualOnly,
            &out,
        )
        .expect("export succeeds");

        assert_eq!(dir, out.join("action-guide"));
        assert!(dir.join("steps.md").exists());
        assert!(dir.join("session.json").exists());
        assert!(dir.join("keyframes/001.png").exists());

        let md = std::fs::read_to_string(dir.join("steps.md")).unwrap();
        assert!(md.contains("![](keyframes/001.png)"), "md = {md}");
        // Markdown references exactly the exported keyframe files.
        let png_count = std::fs::read_dir(dir.join("keyframes")).unwrap().count();
        assert_eq!(md.matches("![](keyframes/").count(), png_count);
        assert_eq!(png_count, guide.steps().len());

        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn session_json_has_capability_and_no_raw_input_fields() {
        let (guide, store) = one_step_recording();
        let out = temp_dir("export-json");
        let dir = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::VisualOnly { reason: crate::models::DegradedReason::SourceStartFailed },
            InputSourceKind::VisualOnly,
            &out,
        )
        .unwrap();

        let json = std::fs::read_to_string(dir.join("session.json")).unwrap();
        let parsed: SessionManifest = serde_json::from_str(&json).expect("manifest parses");
        assert_eq!(parsed.input_source, InputSourceKind::VisualOnly);
        assert_eq!(parsed.steps.len(), 1);
        assert!(json.contains("visual-only"));
        for forbidden in ["semantic", "events", "text", "keycode", "device", "typing-activity"] {
            assert!(!json.contains(forbidden), "session.json leaked {forbidden}: {json}");
        }

        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn export_is_atomic_a_midway_failure_leaves_no_folder_and_preserves_the_guide() {
        let (_guide, store) = one_step_recording();
        let kf = store
            .retained_ids_for_test()
            .into_iter()
            .next()
            .expect("a retained frame exists");
        // Step 1 is exportable; step 2's keyframe is not retained -> fails mid-export.
        let guide = Guide::from_candidates(vec![
            CandidateStep {
                id: 0,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 0,
                keyframe: kf,
                nearby: vec![kf],
            },
            CandidateStep {
                id: 1,
                kind: CandidateKind::UiChanged,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                keyframe: 999_999, // not retained -> injected failure
                nearby: vec![999_999],
            },
        ]);
        let out = temp_dir("export-atomic");
        let result = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::VisualOnly { reason: crate::models::DegradedReason::SourceStartFailed },
            InputSourceKind::VisualOnly,
            &out,
        );

        assert!(result.is_err(), "export must fail");
        assert!(!out.join("action-guide").exists(), "no partial folder");
        assert!(!out.join(".action-guide.tmp").exists(), "temp dir rolled back");
        assert_eq!(guide.steps().len(), 2, "editable guide is preserved");

        let _ = std::fs::remove_dir_all(&out);
    }
}
```

- [ ] **Step 2: Add the test-only retained-id accessor to `FrameStore`**

The atomicity test needs a real retained id. Add this small `#[cfg(test)]` accessor to `impl FrameStore` in `src/frame_store.rs` (it is compiled only for tests):

```rust
    #[cfg(test)]
    pub fn retained_ids_for_test(&self) -> Vec<crate::models::FrameId> {
        self.retained.keys().copied().collect()
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-action export`
Expected: FAIL — `export_guide` / `SessionManifest` not defined.

- [ ] **Step 4: Implement the exporter**

Add above the test module in `src/export.rs`:

```rust
//! Portable guide export. Builds `action-guide/{steps.md, session.json,
//! keyframes/*.png}` in a temporary sibling directory and renames it into place
//! only after every file is written. Any failure rolls back the temp dir, so
//! there is never a partial export and the editable session is preserved.
//! `session.json` serializes only step metadata + capability — never raw input.

use std::path::{Path, PathBuf};

use crate::diagnostics::TARGET_EXPORT;
use crate::error::ExportError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;
use crate::models::{CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind, Millis};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionManifest {
    pub region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub steps: Vec<ManifestStep>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestStep {
    pub index: usize,
    pub title: String,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe_file: String,
}

/// Export `guide` into `out_dir/action-guide/`. Returns the created directory.
pub fn export_guide(
    guide: &Guide,
    store: &FrameStore,
    region: CaptureRegion,
    capability: InputCapability,
    source: InputSourceKind,
    out_dir: &Path,
) -> Result<PathBuf, ExportError> {
    if guide.is_empty() {
        return Err(ExportError::Empty);
    }
    let final_dir = out_dir.join("action-guide");
    let tmp_dir = out_dir.join(".action-guide.tmp");

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|source| ExportError::Io {
            path: tmp_dir.display().to_string(),
            source,
        })?;
    }

    // Build everything in the temp dir; roll back the whole dir on any error.
    if let Err(err) = build(guide, store, region, capability, source, &tmp_dir) {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        tracing::debug!(target: TARGET_EXPORT, "export failed; temp dir rolled back");
        return Err(err);
    }

    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir).map_err(|source| ExportError::Io {
            path: final_dir.display().to_string(),
            source,
        })?;
    }
    std::fs::rename(&tmp_dir, &final_dir).map_err(|source| ExportError::Io {
        path: final_dir.display().to_string(),
        source,
    })?;
    tracing::info!(target: TARGET_EXPORT, steps = guide.steps().len(), "export complete");
    Ok(final_dir)
}

fn build(
    guide: &Guide,
    store: &FrameStore,
    region: CaptureRegion,
    capability: InputCapability,
    source: InputSourceKind,
    tmp: &Path,
) -> Result<(), ExportError> {
    let keyframes = tmp.join("keyframes");
    std::fs::create_dir_all(&keyframes).map_err(|source| ExportError::Io {
        path: keyframes.display().to_string(),
        source,
    })?;

    let mut md = String::from("# Action Guide\n\n");
    let mut steps = Vec::new();

    for (i, step) in guide.steps().iter().enumerate() {
        let n = i + 1;
        let file_name = format!("{n:03}.png");
        let rel = format!("keyframes/{file_name}");
        let frame = store.retained(step.keyframe).ok_or_else(|| ExportError::Io {
            path: rel.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "keyframe pixels not retained",
            ),
        })?;
        let png_path = keyframes.join(&file_name);
        frame
            .image
            .save_with_format(&png_path, image::ImageFormat::Png)
            .map_err(|source| ExportError::Encode {
                path: png_path.display().to_string(),
                source,
            })?;
        md.push_str(&format!("{n}. {}\n\n   ![]({rel})\n\n", step.title));
        steps.push(ManifestStep {
            index: step.index,
            title: step.title.clone(),
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            keyframe_file: rel,
        });
    }

    std::fs::write(tmp.join("steps.md"), md).map_err(|source| ExportError::Io {
        path: tmp.join("steps.md").display().to_string(),
        source,
    })?;

    let manifest = SessionManifest {
        region,
        input_source: source,
        input_capability: capability,
        steps,
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| ExportError::Io {
        path: "session.json".to_string(),
        source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
    })?;
    std::fs::write(tmp.join("session.json"), json).map_err(|source| ExportError::Io {
        path: tmp.join("session.json").display().to_string(),
        source,
    })?;
    Ok(())
}
```

- [ ] **Step 5: Wire the module into `lib.rs`**

```rust
mod export;

pub use export::{export_guide, ManifestStep, SessionManifest};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-action export`
Expected: PASS (3 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/export.rs crates/rollshot-action/src/frame_store.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add atomic Markdown/PNG/session.json exporter

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: Whole-workspace verification

No new code — prove the crate integrates cleanly and breaks nothing.

- [ ] **Step 1: Run the full crate test suite**

Run: `rtk cargo test -p rollshot-action`
Expected: PASS — all module tests green.

- [ ] **Step 2: Confirm the rest of the workspace still builds and tests**

Run: `rtk cargo test --workspace`
Expected: PASS — `rollshot-action` is now a member; no existing crate regressed.

- [ ] **Step 3: Format and lint**

Run: `rtk cargo fmt --check`
Expected: no diff.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings. (If clippy flags an unused import or a `&Vec` argument, fix it in the owning module and amend that task's commit — do not suppress with `#[allow]` except the two deliberate ones already in the plan: `ActionRecorder.region` and the `#[cfg(test)]` accessor.)

- [ ] **Step 4: Confirm platform-neutrality**

The crate depends only on `image`, `serde`, `serde_json`, `thiserror`, `tracing` and inherits `unsafe_code = "forbid"`. Verify there is no dependency on `rollshot-capture` or any platform crate:

Run: `rtk grep -n "rollshot-capture\|rollshot_capture" crates/rollshot-action/`
Expected: no matches.

- [ ] **Step 5: (No commit)** Verification only. If any step required a fix, it was committed in its owning task.

---

## Self-Review

**1. Spec coverage** (spec §`rollshot-action` Unit And Fixture Tests → task):

| Spec requirement | Task(s) |
|------------------|---------|
| Semantic classification never exposes typed text / raw key codes | 2 (serde shape), 7 (privacy assertion) |
| Typing bursts merge on pause / Enter / Tab / finish | 9 (all four tested directly on `Detector`: pause, Enter, Tab, and unsettled-finish) |
| Scroll candidates only after settle + meaningful change | 9 |
| Click candidates require stable visual confirmation | 9 (confirmed click + click-without-change) |
| Cursor-only movement and repeated animation → no steps | 8 |
| Visual-only fixtures produce deterministic guide steps | 8, 10 |
| Nearby-frame selection bounded and ordered | 6, 10 |
| Rename / delete / replace-keyframe update the guide | 11 |
| Markdown references exactly the exported filenames | 12 |
| `session.json` has capability/metadata, no raw input | 12 (manifest + forbidden-field scan), 2 |
| Capture never blocked under load; window store retains keyframes | 6 (drop + retention survival), 10 (keyframe survives burst) |
| Export is atomic; failure leaves no folder, preserves session | 12 |
| Detector failure preserves session; no partial export | 3 (`DetectError` type + render test), 12 (atomic export rollback). The P0a in-process detector is infallible — pure arithmetic over luma, no I/O — so there is no failure path to exercise in this crate; `ActionRecorder::finish` returns `Recording`, not `Result`. The `DetectError` type fixes the seam for a future fallible detector, whose session-preservation wiring is Plan 2. |

Two spec items are intentionally **out of scope** for this crate and belong to Plan 2 (app): the launch/toolbar/workspace tests and the `SendFrameStream` extraction. They are listed in the spec's "App And Workspace Tests", not "`rollshot-action` Unit And Fixture Tests".

**2. Placeholder scan:** No `TODO`/`unimplemented!`/"add error handling"/"similar to Task N" — every step has complete code. The only `#[allow]`s are the two deliberate, documented ones.

**3. Type consistency:** The candidate flow uses one set of names end to end — `CandidateMarker { kind, reason, at_ms, center_id }` (detector) → `CandidateStep { id, kind, reason, at_ms, keyframe, nearby }` (recorder) → `GuideStep { index, title, kind, reason, at_ms, keyframe, nearby, source }` (guide) → `ManifestStep { index, title, kind, reason, at_ms, keyframe_file }` (export). `InputCapability::VisualOnly { reason }`, `CaptureRegion`, `FrameId`/`Millis`/`CandidateId`, and `StoreConfig`/`DetectorConfig` field names match the Interface Contract in every task. `Recording { candidates, store }` is the single hand-off type from recorder to guide+export.

**4. Determinism:** No `SystemTime::now()`/`Instant`/RNG in production paths — all timing is the caller-supplied `Millis`. (`SystemTime::now()` appears only in `#[cfg(test)]` temp-dir helpers, matching `rollshot-capture/src/fixture.rs`.)

---

## Next Plans (write after this crate lands and is green)

This plan delivers the platform-neutral engine only. When Tasks 1–13 are complete and verified, **prompt to write the next plan** — do not start it from this document:

1. **Plan 2 — `rollshot-app` Action Guide integration (P0a Increment 2).** Wires the engine into the product: the `action-guide` Cargo feature, the `rollshot action-guide` CLI command, the 🎬 toolbar entry (`Workflow::ActionGuide` via `ActivateWorkflow`), the region-only (stitch-free) result path, the app-owned frame reader thread pushing cropped `RgbaImage`s into `ActionRecorder` (converting `rollshot_capture::Region → rollshot_action::CaptureRegion`), the recording controls + visual-only advisory, the Action Guide Timeline Workspace, and the export-directory handoff. It must extend `CaptureRequest::is_supported()` to reject `ActionGuide × Fullscreen`, and lift the audited `SendFrameStream` wrapper into `rollshot-capture` (spec §`rollshot-capture`). After Plan 2, P0a is fully shippable (CI-testable, no unsafe FFI, no platform permissions).

2. **P0b — platform semantic-input crates.** `rollshot-linux-input` (evdev, read-only) and `rollshot-macos-input` (CoreGraphics event tap, unsafe-isolation crate), each implementing this crate's `SemanticInputSource` trait so detection upgrades from `VisualOnly` to `SemanticEvents` with **no change to `rollshot-action`**. P0b adds the `rollshot-macos-input` crate to the unsafe-allowed lint set, the README evdev-ACL instructions, and the manual platform-permission verification.
   - **Reference (do not copy):** `learn-projects/CrossMacro` (waycrate/CrossMacro is a cross-platform mouse/keyboard event-capture library) is a useful structural reference for the evdev and CoreGraphics event-tap paths. It is **GPLv3 — a learning reference only**; the Rollshot implementation must not copy its source (spec §Platform Input Sources). Also see `obs-studio` and `scap` (already in `learn-projects/`) for capture-side patterns.

When ready, say so and I'll launch `superpowers:writing-plans` for Plan 2.
