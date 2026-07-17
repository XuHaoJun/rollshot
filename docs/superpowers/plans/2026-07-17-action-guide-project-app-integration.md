# Action Guide Project App Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Action Guide projects discoverable and reopenable through Home/CLI, hydrate them lazily into the existing Timeline Workspace, and provide save-first, dirty, locking, read-only, and cross-platform single-event-loop lifecycles.

**Architecture:** A new `StepFrameSource` abstracts fresh in-memory `FrameStore` images and lazily decoded project assets behind a 256 MiB cache. Shared Home and Timeline state remain platform-neutral; Linux and macOS host them as phases of one iced daemon, while Linux recording runs in a child process because the layer-shell overlay owns its own event loop. Timeline project adapters translate between Plan 1 project snapshots and runtime Guide/presentation state without persisting UI history.

**Tech Stack:** Rust 2021, iced 0.14, clap 4, rfd 0.15, etcetera 0.11, fs4 1.1, serde_json, tokio, existing Rollshot capture/overlay/action/image-document crates.

## Global Constraints

- Execute only after `2026-07-17-action-guide-project-persistence.md` is complete and its gate passes.
- Authoritative spec: `docs/superpowers/specs/2026-07-17-action-guide-project-editing-design.md`.
- Before implementing iced view/subscription work, invoke the `iced-rs` skill and use iced 0.14 signatures.
- `rollshot-app action-guide` opens Home; `--record` records; `--open [PATH]` opens a path or picker; `--fullscreen` is legal only with `--record`.
- The post-recording save-first prompt defaults to Save; `Save later` is explicit; cancelling its picker returns to the prompt.
- Home shows at most ten recent projects and never decodes project images.
- Home does not show publish freshness.
- Open validates manifest/digests/headers off the iced update thread and does not decode all RGBA frames.
- Project image cache limit is exactly 256 MiB of decoded RGBA bytes.
- A digest-valid lazy decode failure makes the entire workspace corrupted read-only and disables Save/Publish.
- A project has one writer on supported local filesystems. A second instance offers read-only or cancel.
- No autosave, crash-recovery drafts, legacy export import, OS file association, or Windows work.
- Every runtime diagnostic uses `tracing` with a stable explicit `rollshot::*` target and privacy-safe structural fields.
- Commands run from `/home/noah/rollshot` and are prefixed with `rtk`.

---

## File Structure

**Create:**

- `crates/rollshot-action/src/step_frame_source.rs` — in-memory/project frame source and bounded decoded cache.
- `crates/rollshot-app/src/action_guide_home/mod.rs` — Home state and recent-project presentation model.
- `crates/rollshot-app/src/action_guide_home/recent.rs` — recent JSON load/save/refresh.
- `crates/rollshot-app/src/action_guide_home/update.rs` — Home messages/effects.
- `crates/rollshot-app/src/action_guide_home/view.rs` — approved Record/Open/Recent hierarchy.
- `crates/rollshot-app/src/action_guide_linux_product.rs` — Linux Home/Timeline phased iced daemon and record-child launcher.
- `crates/rollshot-app/src/timeline_workspace/project.rs` — project runtime adapter, session/lock/save state, async load/save workers.

**Modify:**

- `crates/rollshot-action/src/frame_store.rs` — expose shared retained-image access needed by `StepFrameSource`.
- `crates/rollshot-action/src/guide.rs` — validated reconstruction from persisted reviewed steps.
- `crates/rollshot-action/src/lib.rs` — export `StepFrameSource` APIs.
- `crates/rollshot-app/src/main.rs` — register modules and route new launch modes.
- `crates/rollshot-app/src/launch.rs` — Home/Record/Open parsing and breaking validation.
- `crates/rollshot-app/src/platform_actions.rs` — detached record-child command construction.
- `crates/rollshot-app/src/timeline_workspace/mod.rs` — frame source, project session, save-first, dirty/read-only/corrupt state.
- `crates/rollshot-app/src/timeline_workspace/annotation.rs` — lazy persisted annotation hydration.
- `crates/rollshot-app/src/timeline_workspace/update.rs` — lazy frame tasks, mutations mark dirty, Save/Save As/close lifecycle.
- `crates/rollshot-app/src/timeline_workspace/view.rs` — Save-first modal, Save header state, read-only/corrupt messaging, final-step delete guard.
- `crates/rollshot-app/src/macos_product.rs` — Home/Open/Timeline phases and in-loop macOS record transition.
- `README.md` — new Action Guide command behavior, local-filesystem writable-project limitation, and project/share distinction.

---

### Task 1: Add the lazy, byte-bounded step frame source

**Files:**

- Create: `crates/rollshot-action/src/step_frame_source.rs`
- Modify: `crates/rollshot-action/src/frame_store.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/step_frame_source.rs`

**Interfaces:**

- Consumes: Plan 1 `LoadedProject`, `ProjectFrame`, `SnapshotFramePayload`, and asset inspection rules; existing `FrameStore`.
- Produces:
  - `StepFrameSource::{InMemory, Project}`
  - `StepFrameSource::cached(FrameId) -> Option<Arc<RgbaImage>>`
  - `StepFrameSource::load_request(FrameId) -> Option<StepFrameLoadRequest>`
  - `StepFrameSource::insert_loaded(LoadedStepFrame)`
  - `StepFrameSource::snapshot_payload(FrameId) -> Option<SnapshotFramePayload>`
  - `load_step_frame(StepFrameLoadRequest) -> Result<LoadedStepFrame, ProjectError>`
  - `ProjectFrameSource::new(LoadedProject, usize)` with `DEFAULT_PROJECT_FRAME_CACHE_BYTES = 256 * 1024 * 1024`.

- [ ] **Step 1: Write failing cache and lazy-load tests**

Tests must assert that construction decodes zero frames, a first load decodes one asset, cache hits reuse the same `Arc`, and inserting frames past 256 MiB (use a small injected test limit) evicts least-recently-used images by decoded RGBA byte size.

```rust
#[test]
fn project_source_is_lazy_and_byte_bounded() {
    let loaded = project_with_three_4x4_assets();
    let mut source = ProjectFrameSource::new_with_limit(loaded, 4 * 4 * 4 * 2);
    assert_eq!(source.cached_count_for_test(), 0);

    let first = load_step_frame(source.load_request(1).unwrap()).unwrap();
    source.insert_loaded(first);
    let first_arc = source.cached(1).unwrap();
    assert!(Arc::ptr_eq(&first_arc, &source.cached(1).unwrap()));

    let second = load_step_frame(source.load_request(2).unwrap()).unwrap();
    source.insert_loaded(second);
    let third = load_step_frame(source.load_request(3).unwrap()).unwrap();
    source.insert_loaded(third);
    assert!(source.cached(1).is_none());
    assert!(source.cached(2).is_some());
    assert!(source.cached(3).is_some());
}
```

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-action step_frame_source
```

Expected: compile failure because the module does not exist.

- [ ] **Step 3: Implement source and cache types**

Use these exact public types:

```rust
pub const DEFAULT_PROJECT_FRAME_CACHE_BYTES: usize = 256 * 1024 * 1024;

pub struct LoadedStepFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub image: Arc<RgbaImage>,
}

#[derive(Clone)]
pub struct StepFrameLoadRequest {
    pub project_root: PathBuf,
    pub frame: ProjectFrame,
}

pub enum StepFrameSource {
    InMemory(FrameStore),
    Project(ProjectFrameSource),
}
```

`ProjectFrameSource` stores root, a `BTreeMap<FrameId, ProjectFrame>`, a `BTreeMap<FrameId, Arc<RgbaImage>>`, an LRU `VecDeque<FrameId>`, current decoded bytes, and byte limit. `load_request` clones only the root and one frame descriptor into an owned request. `load_step_frame` calls Plan 1 `decode_png_asset`; it returns pixels without touching UI-owned cache state, so the owned request can run in `spawn_blocking`. `insert_loaded` updates recency and evicts until bytes <= limit. A single image larger than the limit is returned to the caller but not cached.

Add `FrameStore::retained_shared(id) -> Option<(Millis, Arc<RgbaImage>)>`; `StepFrameSource::InMemory` uses it without decoding or caching.

- [ ] **Step 4: Run tests**

```bash
rtk cargo test -p rollshot-action step_frame_source
```

Expected: lazy-load, byte-bound, digest-valid decode failure, and snapshot-payload tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/step_frame_source.rs crates/rollshot-action/src/frame_store.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): load project frames lazily"
```

---

### Task 2: Rehydrate Timeline Guide and annotation presentation from a project

**Files:**

- Modify: `crates/rollshot-action/src/guide.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Create: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/project.rs`

**Interfaces:**

- Consumes: Plan 1 loaded manifest/snapshot APIs and Task 1 `StepFrameSource`.
- Produces:
  - `Guide::from_reviewed_steps(title: String, steps: Vec<GuideStep>) -> Result<Guide, &'static str>`
  - `TimelineWorkspace::from_loaded_project(LoadedProject, ProjectOpenMode) -> Result<TimelineWorkspace, String>`
  - `build_project_snapshot(&TimelineWorkspace) -> Result<ProjectSnapshot, String>`
  - `ProjectOpenMode::{Writable, ReadOnly}`
  - `ProjectSession::{Unsaved, Saved { root, base_revision, open_mode }}`; Task 3 replaces `open_mode` with the guard-owning access state.

- [ ] **Step 1: Write failing reconstruction tests**

Add a fixture loaded from a two-step project with annotations but no decoded cache. Assert `from_loaded_project` restores Guide text/order/keyframe/nearby, selects step 1, leaves all images uncached, stores persisted annotation payloads pending, and starts clean with empty annotation undo history after step 1 is decoded/hydrated.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::project
```

Expected: compile failure because project reconstruction APIs do not exist.

- [ ] **Step 3: Add validated Guide reconstruction**

Implement:

```rust
pub fn from_reviewed_steps(
    title: String,
    steps: Vec<GuideStep>,
) -> Result<Self, &'static str> {
    if steps.is_empty() {
        return Err("empty_guide");
    }
    if steps.iter().enumerate().any(|(offset, step)| step.index != offset + 1) {
        return Err("invalid_step_order");
    }
    Ok(Self { title, steps })
}
```

Do not relax `Guide` field visibility.

- [ ] **Step 4: Make annotation presentation lazily hydratable**

Replace the map value with:

```rust
enum StepAnnotationState {
    Pending {
        keyframe: FrameId,
        persisted: rollshot_action::project::PersistedStepAnnotations,
    },
    Loaded(StepAnnotationDocument),
}
```

Add `restore_pending(source, keyframe, persisted)`, `hydrate_for_step(step, image)`, and `snapshot_for_source(source)` methods. Hydration calls `ImageDocument::from_persisted_annotations`, carries only explanations whose IDs still exist, and replaces Pending with Loaded. Existing in-memory recording behavior remains unchanged.

- [ ] **Step 5: Implement project/runtime adapters**

`from_loaded_project` maps each persisted step ID to runtime `GuideStep.source = id.0`, stores a project-backed `StepFrameSource`, installs pending annotations, and sets `ProjectSession::Saved` at the loaded revision and requested open mode. This task does not yet acquire or store a file lock. `build_project_snapshot` uses each existing `GuideStep.source` as its stable non-zero `ProjectStepId` (fresh recordings therefore inherit their unique candidate IDs; reopened projects preserve their persisted IDs), enumerates only frame IDs still referenced by surviving steps, uses `snapshot_payload`, and persists either loaded or pending annotations. It never serializes workspace modals, proposals, selection, or history.

- [ ] **Step 6: Run focused tests**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::project
```

Expected: project reconstruction and snapshot round-trip tests pass without eager decode.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/guide.rs crates/rollshot-app/src/timeline_workspace/annotation.rs crates/rollshot-app/src/timeline_workspace/project.rs crates/rollshot-app/src/timeline_workspace/mod.rs
rtk git commit -m "feat(app): rehydrate action guide projects"
```

---

### Task 3: Add writer locking and async Open/Save workers

**Files:**

- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/project.rs`

**Interfaces:**

- Consumes: Plan 1 store APIs and Task 2 adapters.
- Produces:
  - `acquire_project_writer(root: &Path) -> Result<ProjectLockResult, String>`
  - `ProjectLockResult::{Acquired(ProjectWriterGuard), AlreadyLocked}`
  - `ProjectAccess::{Writable(ProjectWriterGuard), ReadOnly, CorruptReadOnly}` replacing Task 2 `ProjectOpenMode` in `ProjectSession`
  - `OpenProjectWorkerResult::{Opened(OpenProjectResult), WriterLocked { root: PathBuf }}`
  - `load_project_worker(OpenProjectRequest) -> Result<OpenProjectWorkerResult, String>`
  - `save_project_worker(SaveProjectRequest) -> Result<ProjectCommit, String>`.

- [ ] **Step 1: Write failing lock/worker tests**

Mirror `daemon/instance.rs`: second lock reports AlreadyLocked and dropping the guard allows reacquisition. Add async-worker tests proving load/save run through `spawn_blocking`, a revision conflict preserves the dirty snapshot, and corrupt digest/header validation returns a structural frame ID without logging a path. Full PNG decode failures remain Task 1's lazy-load responsibility, not Open-worker behavior.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide project_writer
```

Expected: compile failure because lock/worker types do not exist.

- [ ] **Step 3: Implement exact locking semantics**

Open `<project>/.lock` with read/write/create/no-truncate and call `fs4::FileExt::try_lock(&file)`. `ProjectWriterGuard` owns the `File`; it never writes PID data. Map `std::io::ErrorKind::WouldBlock` to `AlreadyLocked`, and all other errors to a privacy-safe user message plus tracing category `project_lock` without full path.

- [ ] **Step 4: Implement blocking workers**

Requests own all data:

```rust
pub struct OpenProjectRequest { pub root: PathBuf, pub writable: bool }
pub struct OpenProjectResult { pub loaded: LoadedProject, pub access: ProjectAccess }
pub enum SaveDestination { FirstSave(PathBuf), Existing(PathBuf), SaveAs(PathBuf) }
pub struct SaveProjectRequest { pub snapshot: ProjectSnapshot, pub destination: SaveDestination }
```

Async wrappers call `tokio::task::spawn_blocking` and return a worker-failure category on join failure. Writable Open acquires the guard before load and returns `WriterLocked` without constructing a writable workspace when the lock is held; the host then offers Open Read-Only or Cancel and resubmits with `writable: false` only after the user chooses it. Read-only Open skips locking. First Save/Save As attempts to acquire the new project's lock immediately after the atomic directory commit; if another process wins that narrow race, return a lock-conflict outcome and retain the committed snapshot without claiming writable ownership.

- [ ] **Step 5: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide project_writer
```

Expected: locking, drop release, async load/save, and conflict tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/project.rs crates/rollshot-app/src/timeline_workspace/mod.rs
rtk git commit -m "feat(app): lock and save action guide projects"
```

---

### Task 4: Add Action Guide CLI routing and recent-project storage

**Files:**

- Modify: `crates/rollshot-app/src/launch.rs`
- Create: `crates/rollshot-app/src/action_guide_home/recent.rs`
- Create: `crates/rollshot-app/src/action_guide_home/mod.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Test: same files

**Interfaces:**

- Consumes: Task 3 Open worker.
- Produces:
  - `ActionGuideLaunch::{Home, Record { fullscreen }, Open { path: Option<PathBuf> }}`
  - `LaunchMode::ActionGuide(ActionGuideLaunch)`
  - `RecentProjects::load`, `record_open`, `remove`, `save`, max 10.

- [ ] **Step 1: Write failing CLI matrix tests**

Add exact cases:

```rust
assert_eq!(parse(&["rollshot-app", "action-guide"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Home)));
assert_eq!(parse(&["rollshot-app", "action-guide", "--record"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Record { fullscreen: false })));
assert_eq!(parse(&["rollshot-app", "action-guide", "--record", "--fullscreen"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Record { fullscreen: true })));
assert!(parse(&["rollshot-app", "action-guide", "--fullscreen"]).unwrap_err().contains("--record"));
assert!(parse(&["rollshot-app", "action-guide", "--record", "--open"]).is_err());
assert_eq!(parse(&["rollshot-app", "action-guide", "--open", "/tmp/a.rollshot-guide"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Open { path: Some(PathBuf::from("/tmp/a.rollshot-guide")) })));
```

Represent optional `--open [PATH]` with `Option<Option<PathBuf>>` and `num_args = 0..=1`.

- [ ] **Step 2: Write failing recent-file tests**

Use a temp config path. Assert malformed JSON loads empty, duplicate paths move to front, list truncates to ten, missing projects remain with `available = false`, titles/content are not stored beyond display name, and save uses temp+rename.

- [ ] **Step 3: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide launch::tests
rtk cargo test -p rollshot-app --features action-guide action_guide_home::recent
```

Expected: new CLI and recent APIs fail to compile.

- [ ] **Step 4: Implement launch and recent DTOs**

Use strict versioned recent JSON for writes but lenient top-level load:

```rust
#[derive(Serialize, Deserialize)]
struct RecentFile { schema_version: u32, entries: Vec<RecentEntry> }
#[derive(Clone, Serialize, Deserialize)]
pub struct RecentEntry { pub path: PathBuf, pub display_name: String, pub last_opened_ms: u64 }
```

Path is permitted in the local file but excluded from all tracing fields. Store under `daemon::config::rollshot_config_dir()?.join("recent-action-guides.json")`.

- [ ] **Step 5: Route modes from `main.rs`**

Register `action_guide_home` and `action_guide_linux_product`. Map Home/Open to the platform product host and Record to the existing recording function. Preserve `ActionGuideProbe` unchanged.

- [ ] **Step 6: Run tests and commit**

```bash
rtk cargo test -p rollshot-app --features action-guide launch::tests
rtk cargo test -p rollshot-app --features action-guide action_guide_home::recent
rtk git add crates/rollshot-app/src/launch.rs crates/rollshot-app/src/action_guide_home crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): route action guide projects"
```

---

### Task 5: Build the shared Action Guide Home

**Files:**

- Create: `crates/rollshot-app/src/action_guide_home/update.rs`
- Create: `crates/rollshot-app/src/action_guide_home/view.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/mod.rs`
- Modify: `crates/rollshot-app/src/platform_actions.rs`
- Test: same files

**Interfaces:**

- Consumes: Task 4 recent storage and Task 3 Open worker.
- Produces: `ActionGuideHome`, `Message`, `Effect::{RecordNew, OpenProject(PathBuf), OpenLegacyReader(PathBuf)}`, `view`, `update`, `subscription`.

- [ ] **Step 1: Invoke `iced-rs` and write failing state tests**

Test Record New, Open picker cancel, selecting available/unavailable recent entries, removing unavailable entries, reloading recent on `WindowFocused`, detecting `project.json`, detecting legacy `session.json` without `project.json`, and offering `index.html` reader handoff.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home
```

Expected: compile failure because Home update/view are absent.

- [ ] **Step 3: Implement Home state/effects**

```rust
pub struct ActionGuideHome {
    pub recent: RecentProjects,
    pub opening: bool,
    pub message: Option<String>,
}

pub enum Effect {
    None,
    RecordNew,
    OpenProject(PathBuf),
    OpenLegacyReader(PathBuf),
}
```

Use an explicit `WindowFocused` message sourced from iced window events. Detection rules are exact: `project.json` means project; otherwise `session.json` means legacy export; otherwise show invalid selection. Do not read images.

- [ ] **Step 4: Implement the approved view hierarchy**

Render `Record New` as primary, `Open Project...` as secondary, then Recent Projects with display name/time/availability only. Do not render publish freshness. Keep all project paths out of visible default cards; reveal path only in unavailable-entry detail if needed for recovery.

- [ ] **Step 5: Add detached Linux record command helper**

Add a testable `action_guide_record_command(fullscreen: bool)` that resolves the current executable and builds `action-guide --record` plus optional `--fullscreen`; spawning detaches without shell interpolation. macOS does not use this helper.

- [ ] **Step 6: Run tests and commit**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home
rtk git add crates/rollshot-app/src/action_guide_home crates/rollshot-app/src/platform_actions.rs
rtk git commit -m "feat(app): add action guide home"
```

---

### Task 6: Integrate lazy loading, save-first, dirty state, and read-only behavior into Timeline

**Files:**

- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Test: same files

**Interfaces:**

- Consumes: Tasks 1–3 runtime/project APIs.
- Produces: complete Timeline project lifecycle messages and state.

- [ ] **Step 1: Write failing lifecycle tests**

Cover:

- fresh recording starts with save-first prompt;
- Save picker cancel returns to prompt;
- `Save later` enters Unsaved Project;
- every mutation arm (title, step title/caption, delete, keyframe, annotation apply/delete/undo/redo, accepted agent proposal) marks dirty;
- last step cannot be deleted;
- first Save and existing Save transition Saving → Saved and update base revision;
- conflict keeps dirty edits and shows recoverable error;
- close dirty gives Save and Close / Discard / Cancel; picker cancel returns to workspace;
- read-only disables every mutation and Save;
- corrupt lazy decode changes access to CorruptReadOnly and disables Save.
- selecting a step schedules its current keyframe and uncached nearby strip frames, while stale completions from the previously selected step are ignored.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::tests::project_
```

Expected: new lifecycle tests fail.

- [ ] **Step 3: Add state and messages**

Add:

```rust
pub enum ProjectSaveState { Unsaved, Clean, Dirty, Saving }
pub enum FirstSavePrompt { Hidden, Visible, Picking }
pub enum CloseIntent { None, Confirming, SaveThenClose }
```

Timeline holds `StepFrameSource`, `ProjectSession`, these lifecycle states, a monotonically increasing frame-load operation ID, and a set of required/loading frame IDs for the selected step. Every mutation calls one `mark_project_dirty()` helper after the mutation succeeds.

- [ ] **Step 4: Move frame resolution to async tasks**

Selecting a project-backed step computes the current keyframe plus ordered nearby strip IDs, uses cached images immediately, increments the operation ID once, builds an owned `StepFrameLoadRequest` for each miss, and runs `load_step_frame` under `spawn_blocking` without decoding unrelated steps. The main preview shows a step-local loading state until the current keyframe arrives; nearby thumbnails fill progressively. Each completion inserts only when operation/selected-step still match, rebuilds the relevant image handle, and hydrates pending annotations only for the current keyframe. Any required-frame decode failure sets CorruptReadOnly with structural frame/step category.

- [ ] **Step 5: Implement Save and close chains**

First Save/Save As picker produces an owned snapshot before worker start. Existing Save uses base revision. On success update session root/revision/guard, clear dirty, update recent metadata, and leave publish status stale for Plan 3. `SaveThenClose` exits only after successful Save; picker cancellation or failure returns to workspace without discard.

- [ ] **Step 6: Implement view states**

Add the save-first warning copy, `Unsaved changes` / `Saving` / `Saved`, read-only lock banner, corrupt project banner, loading placeholder, and disabled mutation controls. Keep existing annotation/keyframe layout and do not add publish detail UI yet.

- [ ] **Step 7: Run tests and commit**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk git add crates/rollshot-app/src/timeline_workspace
rtk git commit -m "feat(app): save and reopen action guide projects"
```

---

### Task 7: Host Home and Timeline in one Linux iced daemon

**Files:**

- Create: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Test: `crates/rollshot-app/src/action_guide_linux_product.rs`

**Interfaces:**

- Consumes: Tasks 4–6 Home/Timeline states.
- Produces: `run(initial: ActionGuideLaunch) -> Result<(), String>` with `Phase::{Home, Opening, LockConflict, Timeline}`.

- [ ] **Step 1: Invoke `iced-rs` and write transition tests**

Test initial Home, initial Open path, Home Record effect command, Home Open → Opening → Timeline, WriterLocked → LockConflict → Open Read-Only/Cancel, legacy reader handoff remains Home, Timeline close → Home, and no second iced event loop invocation.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_linux_product
```

Expected: module absent.

- [ ] **Step 3: Implement daemon phase/message delegation**

Use one `iced::daemon` with one decorated window for Home/Timeline. `Message::{Home, Timeline, ProjectOpened, OpenReadOnly, CancelLockedOpen, WindowReady}` delegates to shared update/view/subscription functions. `Record New` spawns the detached current-executable child and leaves Home open. Open performs Plan 2's async load before switching phase; `WriterLocked` shows exactly Open Read-Only and Cancel, and only the former resubmits a read-only request.

- [ ] **Step 4: Route Linux Home/Open modes**

`main.rs` sends Home/Open to this daemon. Direct Record retains the existing overlay → recording → Timeline runner in the child process, including the save-first prompt.

- [ ] **Step 5: Run tests and commit**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_linux_product
rtk git add crates/rollshot-app/src/action_guide_linux_product.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): host action guide home on Linux"
```

---

### Task 8: Add Home/Open phases to the macOS product daemon

**Files:**

- Modify: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Test: `crates/rollshot-app/src/macos_product.rs`

**Interfaces:**

- Consumes: shared Home/Timeline states and existing macOS capture component.
- Produces: macOS `Phase::{Home, Capture, Timeline, ...}` transitions for Action Guide launch modes.

- [ ] **Step 1: Invoke `iced-rs` and write phase tests**

Cover Home launch, direct Open, direct Record, Home Record New entering in-loop Action Guide capture, capture completion showing save-first Timeline, WriterLocked → Open Read-Only/Cancel, Timeline return Home, and Open failure returning Home with message.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide macos_product::tests::action_guide_project_
```

Expected: missing Home/Open phase behavior.

- [ ] **Step 3: Extend phase/message/update/view/subscription**

Add Home, Opening, and LockConflict phase variants plus mapped messages. Reuse the existing one daemon and capture `Component`; do not spawn a macOS recording child. Home `Record New` constructs the existing Action Guide overlay config and transitions to Capture. Existing `complete_action_recording` constructs a Timeline with save-first prompt visible. Lock conflict uses the same Open Read-Only/Cancel behavior as Linux.

- [ ] **Step 4: Route macOS launch modes**

`main.rs` supplies Home/Open/Record initial intent to `macos_product::run`. Preserve fullscreen capture behavior only for Record.

- [ ] **Step 5: Run tests and commit**

```bash
rtk cargo test -p rollshot-app --features action-guide macos_product
rtk git add crates/rollshot-app/src/macos_product.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): host action guide home on macOS"
```

---

### Task 9: Document and verify the app integration slice

**Files:**

- Modify: `README.md`
- Test: workspace commands below

**Interfaces:**

- Consumes: all Plan 2 tasks.
- Produces: documented, cross-platform reopen/edit/save lifecycle ready for Plan 3 publishing.

- [ ] **Step 1: Update user-facing commands and safety copy**

Document Home, `--record`, `--record --fullscreen`, `--open [PATH]`, `.rollshot-guide/` as private editable source, Safe Copy as the eventual share artifact, save-first/Save later behavior, legacy export read-only handoff, and writable-project local-filesystem limitation. Remove examples implying bare `action-guide` immediately records.

- [ ] **Step 2: Run full Plan 2 verification**

```bash
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
rtk cargo clippy -p rollshot-action --all-targets -- -D warnings
rtk cargo clippy -p rollshot-app --all-targets --features action-guide -- -D warnings
```

Expected: all commands exit 0 with no warnings.

- [ ] **Step 3: Perform runtime checks**

On Linux: Home → Record New child → save-first Save → edit → close → Home focus refresh → reopen → replace keyframe/annotation → Save. On macOS: repeat in one daemon. Verify a second process offers read-only, a corrupt lazy frame disables Save, and a legacy export offers `index.html` reader handoff. When both platforms are available, create on each and open/Save on the other. Record unchecked platform or cross-platform runtime risk explicitly if only one OS is available.

- [ ] **Step 4: Commit**

```bash
rtk git add README.md
rtk git commit -m "docs(action-guide): document editable projects"
```

## Plan 2 Completion Gate

- Bare `action-guide` opens Home on Linux and macOS.
- Direct Record/Open routes work and invalid fullscreen combinations fail clearly.
- New recordings default to save-first and can explicitly Save later.
- Reopened projects hydrate metadata/annotations without eager image decode.
- Lazy cache is byte-bounded at 256 MiB and corruption downgrades to read-only.
- Save/Save As, dirty close, revision conflict, and writer lock behaviors are verified.
- Recent projects refresh after the Linux recording child returns focus.
- Product tests, fmt, and clippy pass before Plan 3 starts.
