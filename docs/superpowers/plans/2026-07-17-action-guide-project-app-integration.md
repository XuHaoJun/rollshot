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
- Project-backed workspaces disable the existing standalone Guide/GIF/Storyboard/MP4/Issue Pack actions until Plan 3 supplies bounded project-frame publishing; fresh in-memory recording behavior remains available.
- Lazy project decode work has at most two active `spawn_blocking` jobs per workspace. Queued stale generations are discarded before decode, and renderer handles are retained only for the selected keyframe plus its nearby strip.
- A digest-valid lazy decode failure makes the entire workspace corrupted read-only and disables Save/Publish.
- A project has one writer on supported local filesystems. A second instance offers read-only or cancel.
- Worker boundaries preserve typed project, lock, join, destination, and revision-conflict outcomes; user copy is derived only at the UI boundary.
- No autosave, crash-recovery drafts, legacy export import, OS file association, or Windows work.
- Plan 2 does not enable the non-default `action-guide` feature in release artifacts; existing feature-gated CI remains the distribution boundary for this slice.
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
- `crates/rollshot-action/src/recorder.rs` — allocate non-zero candidate IDs for project-stable step identity.
- `crates/rollshot-action/src/lib.rs` — export `StepFrameSource` APIs.
- `crates/rollshot-app/src/main.rs` — register modules and route new launch modes.
- `crates/rollshot-app/src/launch.rs` — Home/Record/Open parsing and breaking validation.
- `crates/rollshot-app/src/platform_actions.rs` — detached record-child command construction.
- `crates/rollshot-app/src/timeline_workspace/mod.rs` — frame source, project session, save-first, dirty/read-only/corrupt state.
- `crates/rollshot-app/src/timeline_workspace/annotation.rs` — lazy persisted annotation hydration.
- `crates/rollshot-app/src/timeline_workspace/guide_export.rs` — preserve in-memory-only standalone export behavior after the frame-source migration.
- `crates/rollshot-app/src/timeline_workspace/storyboard_copy.rs` — preserve in-memory-only storyboard behavior after the frame-source migration.
- `crates/rollshot-app/src/timeline_workspace/update.rs` — lazy frame tasks, mutations mark dirty, Save/Save As/close lifecycle.
- `crates/rollshot-app/src/timeline_workspace/view.rs` — Save-first modal, Save header state, read-only/corrupt messaging, final-step delete guard.
- `crates/rollshot-app/src/macos_product.rs` — Home/Open/Timeline phases and in-loop macOS record transition.
- `README.md` — new Action Guide command behavior, local-filesystem writable-project limitation, and project/share distinction.

## Engineering Review Lock-In

### Step 0: Scope Challenge

- **Goal alignment:** all retained tasks contribute directly to Home discovery, lazy reopen, editable Timeline state, safe Save/close behavior, or the required Linux/macOS host lifecycle.
- **Complexity check:** 7 net-new files, 0 new crates, and 10 tasks after review; the automatic reduction threshold is not triggered.
- **Minimum viable slice:** keep lazy frames, adapters, locking/workers, recents/Home, Timeline lifecycle, both platform hosts, final CLI routing, and verification. Publishing, sharing, and release-feature enablement remain deferred.
- **Framework check:** iced 0.14 already supplies one-daemon phase hosting, `Task::perform`, window-event subscriptions, and `window::open`; clap derives `Option<Option<PathBuf>>` as a `0..=1` optional value; fs4 1.1 returns `TryLockError`; Tokio warns that `spawn_blocking` work cannot be aborted after start and needs an explicit concurrency bound for CPU-heavy decode.
- **Distribution check:** no new binary/library artifact is introduced. The existing non-default `action-guide` feature and existing Linux/macOS feature CI remain unchanged.

### What already exists

- `FrameStore` already owns retained frames as `Arc<RgbaImage>`; `StepFrameSource::InMemory` reuses those allocations through a shared accessor.
- Plan 1 owns strict project DTOs, digest/header inspection, lazy full decode, atomic create/Save/Save As, and revision conflicts; this plan adds app adapters instead of rebuilding persistence.
- `TimelineWorkspace`, `ActionGuidePresentation`, operation IDs, and standalone export state already provide the editing surface and stale-result precedent; this plan extends those paths surgically.
- `daemon/instance.rs` already demonstrates fs4 1.1 guard ownership and `TryLockError::{WouldBlock, Error}` handling; project locking mirrors it.
- `macos_product.rs` already owns the single iced daemon and capture-to-Timeline transition; this plan adds phases instead of starting another event loop.
- `platform_actions.rs` already centralizes shell-free process construction; the Linux recording child extends it with native `OsString` arguments and explicit reaping.
- `.github/workflows/ci.yml` already runs the `action-guide` feature tests and clippy on both Ubuntu and macOS.

### NOT in scope

- Plan 3 publish freshness, publish regeneration, Safe Copy, Issue Pack-from-project, and editable-project sharing — they require the separate bounded publishing model.
- Legacy export import or editable reconstruction — flattened exports do not contain source annotations or nearby frames.
- Autosave, crash-recovery drafts, collaboration, multi-writer merge, and network-filesystem writer guarantees — v1 remains explicit manual Save with one local writer.
- Windows, OS file association, and release-artifact enablement of the non-default feature — this slice stays behind existing feature/build policy.
- Refactoring unrelated synchronous legacy exporters or existing path-bearing diagnostics — project-backed workspaces gate those actions rather than expanding this plan.

### Data and host flows

```text
Plan 1 LoadedProject
  ├─ manifest ─▶ Guide + pending annotations + EnabledOutputs
  └─ root/frame catalog ─▶ ProjectFrameSource (0 decoded frames)
                              │ select step / generation N
                              ▼
                      cache hits + owned misses
                              │ max 2 active decodes
                              ▼
                       LoadedStepFrame results
                              │ generation/step still current?
                    stale ────┴──── current
                     drop            ├─ cache by decoded RGBA bytes
                                     └─ build selected handles + hydrate annotations
```

```text
shared Home/Timeline state          platform host policy
──────────────────────────          ────────────────────
Home Effect::RecordNew       ─────▶ Linux: reaped child / macOS: Capture phase
Home Effect::OpenProject     ─────▶ Opening ─▶ LockConflict | Timeline
Timeline Effect::CloseWorkspace ─▶ phased host: Home / direct-record child: exit

Only the host opens/closes windows or exits the iced daemon.
```

```text
first Save / Save As worker
snapshot ─▶ Plan 1 no-replace commit ─▶ acquire committed .lock
                 │                            ├─ acquired: clean writable session
                 │                            └─ lost race/error: clean committed read-only session
                 └─ failure before commit: dirty unsaved/source session unchanged
```

### Test coverage map

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / lazy decode, true LRU, byte accounting, full snapshot frame | ✓ | — | — | no |
| Task 2 / Guide, stable non-zero IDs, pending annotation hydration, snapshot round trip | ✓ | ✓ | — | no |
| Task 3 / lock guard, typed Open/Save outcomes, revision and post-commit lock conflicts | ✓ | ✓ | — | no |
| Task 4 / recent JSON ordering, corruption, availability, atomic replacement | ✓ | — | — | no |
| Task 5 / Home actions, focus refresh, project/legacy inspection, child command/reaping | ✓ | ✓ | — | no |
| Task 6 / save-first, dirty/mutation matrix, close effect, read-only/corrupt, stale loads | ✓ | ✓ | — | no |
| Task 7 / Linux Home/Open/Timeline phases and child policy | ✓ | ✓ | — | no |
| Task 8 / macOS Home/Capture/Open/Timeline phases | ✓ | ✓ | — | no |
| Task 9 / complete CLI matrix and final platform routing | ✓ | ✓ | ✓ | no |
| Task 10 / feature suite, fmt, clippy, real Linux/macOS lifecycle | — | ✓ | ✓ | cross-platform runtime |

### Failure modes

| New codepath | Realistic failure | Test / handling / user result |
|---|---|---|
| Lazy frame decode | digest-valid PNG cannot materialize pixels | Task 1 lazy-error test + Task 6 transition test; typed frame ID; corrupted read-only banner, never silent |
| Cache insertion | duplicate/replacement accounting exceeds the limit | Task 1 replacement/oversized/true-LRU tests; checked byte math and eviction; no user-visible failure |
| Project reconstruction | zero/duplicate step identity or invalid order | Task 2 negative tests; `Guide::from_reviewed_steps` error; Open fails clearly |
| Writable Open | `.lock` is held or cannot be opened | Task 3 lock tests; `WriterLocked` or typed lock error; read-only/cancel or clear failure |
| Existing Save | disk revision changed | Task 3 worker conflict test + Task 6 lifecycle test; `RevisionConflict`; dirty edits preserved |
| First Save / Save As | directory commits but lock acquisition loses the race | Task 3 post-commit outcome + Task 6 UI test; committed read-only session; explicit banner |
| Recent metadata | malformed JSON or interrupted replace | Task 4 tests; malformed loads empty and prior complete file survives failed replace; Home remains usable |
| Home selection | network/missing directory stalls or disappears | Task 5 blocking inspection and unavailable tests; Home shows recoverable error, no silent removal |
| Linux recording child | spawn fails or child exits without being waited | Task 5 command/reaper tests; Home banner on spawn failure; child is reaped off update thread |
| Timeline close | save picker cancels or Save fails | Task 6 effect tests; workspace stays open and dirty; host receives no close effect |
| Async frame selection | previous step completes late | Task 6 generation tests; stale result discarded before insertion/handle rebuild |
| Platform phase transition | stale window message arrives after phase change | Tasks 7–8 transition tests; ignored safely; one daemon remains alive |

No reviewed codepath has an untested, unhandled, silent failure.

### Task dependencies and parallel lanes

| Task | Modules touched | Depends on |
|---|---|---|
| 1: frame source | `crates/rollshot-action/` | Plan 1 |
| 2: runtime adapters | `crates/rollshot-action/`, `crates/rollshot-app/src/timeline_workspace/` | 1 |
| 3: locks/workers | `crates/rollshot-app/src/timeline_workspace/` | 2 |
| 4: recents | `crates/rollshot-app/src/action_guide_home/` | Plan 1 |
| 5: Home | `crates/rollshot-app/src/action_guide_home/`, `platform_actions.rs` | 3, 4 |
| 6: Timeline lifecycle | `crates/rollshot-app/src/timeline_workspace/` | 1, 2, 3 |
| 7: Linux host | `crates/rollshot-app/src/action_guide_linux_product.rs`, `main.rs` module registration | 5, 6 |
| 8: macOS host | `crates/rollshot-app/src/macos_product.rs` | 5, 6 |
| 9: CLI routing | `crates/rollshot-app/src/launch.rs`, `main.rs` | 7, 8 |
| 10: docs/verification | `README.md` | 9 |

- Lane A: Task 1 → Task 2 → Task 3 → Task 6 (sequential; shared action/Timeline contracts).
- Lane B: Task 4 (parallel with Task 1), then Task 5 after Tasks 3 + 4.
- Lanes C/D: Tasks 7 and 8 in parallel after Tasks 5 + 6; they touch separate platform modules.
- Conflict flag: Tasks 7 and 8 both live under `crates/rollshot-app/src/`; Task 7's `main.rs` edit is registration-only and Task 8 must not touch it. Coordinate or serialize if the executor cannot guarantee file ownership.
- Final lane: Task 9 → Task 10 after both platform hosts exist.
- Repository policy forbids worktrees unless explicitly requested. Parallel agents may edit only the disjoint modules above; Tasks 2/3/6/9/10 are serialized. No task modifies root `Cargo.toml`.

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
  - `StepFrameSource::cached(&mut self, FrameId) -> Option<Arc<RgbaImage>>` (cache hits refresh true LRU recency)
  - `StepFrameSource::load_request(FrameId) -> Option<StepFrameLoadRequest>`
  - `StepFrameSource::insert_loaded(LoadedStepFrame)`
  - `StepFrameSource::snapshot_frame(FrameId) -> Option<SnapshotFrame>` (includes per-frame timestamp and payload)
  - `StepFrameSource::in_memory() -> Option<&FrameStore>` for existing in-memory-only exporters
  - `load_step_frame(StepFrameLoadRequest) -> Result<LoadedStepFrame, ProjectError>`
  - `ProjectFrameSource::from_loaded(&LoadedProject, usize)` with `DEFAULT_PROJECT_FRAME_CACHE_BYTES = 256 * 1024 * 1024`.

- [ ] **Step 1: Register the module and write failing cache/lazy-load tests**

Add `pub mod step_frame_source;` and the intended re-exports in `lib.rs` so the test file is compiled during the RED run. Tests must assert that construction borrows then copies only the root/frame catalog and decodes zero frames; a first load decodes one asset; cache hits reuse the same `Arc` and refresh recency; insertion past 256 MiB (use a small injected test limit) evicts by decoded RGBA byte size; replacing a cached ID does not double-count bytes; a single oversized image is returned but not cached; checked byte math cannot wrap; a digest-valid decode failure remains typed; and `snapshot_frame` preserves timestamp plus `Pixels`/`ExistingAsset` payload identity.

```rust
#[test]
fn project_source_is_lazy_and_byte_bounded() {
    let loaded = project_with_three_4x4_assets();
    let mut source = ProjectFrameSource::from_loaded(&loaded, 4 * 4 * 4 * 2);
    assert_eq!(source.cached_count_for_test(), 0);

    let first = load_step_frame(source.load_request(1).unwrap()).unwrap();
    source.insert_loaded(first);
    let first_arc = source.cached(1).unwrap();
    assert!(Arc::ptr_eq(&first_arc, &source.cached(1).unwrap()));

    let second = load_step_frame(source.load_request(2).unwrap()).unwrap();
    source.insert_loaded(second);
    let first_arc_again = source.cached(1).unwrap();
    assert!(Arc::ptr_eq(&first_arc, &first_arc_again));
    let third = load_step_frame(source.load_request(3).unwrap()).unwrap();
    source.insert_loaded(third);
    assert!(source.cached(1).is_some());
    assert!(source.cached(2).is_none());
    assert!(source.cached(3).is_some());
}
```

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-action step_frame_source
```

Expected: compile failure because registered frame-source types and methods are not implemented.

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

`ProjectFrameSource` stores root, a `BTreeMap<FrameId, ProjectFrame>`, a `BTreeMap<FrameId, Arc<RgbaImage>>`, an LRU `VecDeque<FrameId>`, current decoded bytes, and byte limit. `from_loaded` clones only root and frame descriptors, not steps/annotations. `load_request` clones only the root and one frame descriptor into an owned request. `load_step_frame` calls Plan 1 `decode_png_asset`; it returns pixels without touching UI-owned cache state, so the owned request can run in `spawn_blocking`. `cached(&mut self, ...)` moves a hit to the LRU tail. `insert_loaded` subtracts any replaced entry, uses checked `width * height * 4` conversion, updates recency, and evicts until bytes <= limit. A single image larger than the limit is returned to the caller but not cached.

Add `FrameStore::retained_shared(id) -> Option<(Millis, Arc<RgbaImage>)>`; `StepFrameSource::InMemory` uses it without decoding or caching. `snapshot_frame` constructs Plan 1's complete `SnapshotFrame`, avoiding a second timestamp lookup in app code.

- [ ] **Step 4: Run tests**

```bash
rtk cargo test -p rollshot-action step_frame_source
```

Expected: lazy-load, true-LRU, byte-accounting, oversized-frame, digest-valid decode failure, and complete snapshot-frame tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/step_frame_source.rs crates/rollshot-action/src/frame_store.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): load project frames lazily"
```

---

### Task 2: Rehydrate Timeline Guide and annotation presentation from a project

**Files:**

- Modify: `crates/rollshot-action/src/guide.rs`
- Modify: `crates/rollshot-action/src/recorder.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Create: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/project.rs`

**Interfaces:**

- Consumes: Plan 1 loaded manifest/snapshot APIs and Task 1 `StepFrameSource`.
- Produces:
  - `Guide::from_reviewed_steps(title: String, steps: Vec<GuideStep>) -> Result<Guide, &'static str>`
  - `ProjectAdapterError::{InvalidGuide { category }, MissingFrame { frame_id }, InvalidAnnotations { step_id, category }}`
  - `TimelineWorkspace::from_loaded_project(LoadedProject, ProjectOpenMode) -> Result<TimelineWorkspace, ProjectAdapterError>`
  - `build_project_snapshot(&TimelineWorkspace) -> Result<ProjectSnapshot, ProjectAdapterError>`
  - `ProjectOpenMode::{Writable, ReadOnly}`
  - `ProjectSession::{Unsaved, Saved { root, base_revision, open_mode }}`; Task 3 replaces `open_mode` with the guard-owning access state.

- [ ] **Step 1: Register the adapter and write failing reconstruction/identity tests**

Register `timeline_workspace::project` in `timeline_workspace/mod.rs` so the new test file participates in the RED run. Add a fixture loaded from a two-step project with annotations but no decoded cache. Assert `from_loaded_project` restores Guide text/order/keyframe/nearby and enabled-output settings, selects step 1, leaves all images uncached, stores persisted annotation payloads pending, and starts clean with empty annotation undo history after step 1 is decoded/hydrated. Add a recorder test proving generated candidate IDs begin at 1 and a reconstruction test rejecting zero/duplicate step sources.

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
    let mut sources = std::collections::BTreeSet::new();
    if steps.iter().any(|step| step.source == 0 || !sources.insert(step.source)) {
        return Err("invalid_step_source");
    }
    Ok(Self { title, steps })
}
```

Do not relax `Guide` field visibility. Change `ActionRecorder::next_candidate_id` to start at 1 so fresh recordings can use candidate identity directly as the schema's required non-zero `ProjectStepId`; use checked increment and add the focused recorder regression test.

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

`from_loaded_project` first builds `ProjectFrameSource::from_loaded(&loaded, limit)`, then maps each persisted step ID to runtime `GuideStep.source = id.0`, stores enabled outputs, installs pending annotations, and sets `ProjectSession::Saved` at the loaded revision and requested open mode. This task does not yet acquire or store a file lock. `build_project_snapshot` uses each existing `GuideStep.source` as its stable non-zero `ProjectStepId` (fresh recordings inherit 1-based candidate IDs; reopened projects preserve persisted IDs), enumerates only frame IDs still referenced by surviving steps, uses `snapshot_frame`, and persists either loaded or pending annotations. Missing frames and invalid annotation snapshots remain structural `ProjectAdapterError` values until the UI maps them to copy. It never serializes workspace modals, proposals, selection, or history.

- [ ] **Step 6: Run focused tests**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::project
```

Expected: project reconstruction, enabled-output preservation, non-zero identity, and snapshot round-trip tests pass without eager decode.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/guide.rs crates/rollshot-action/src/recorder.rs crates/rollshot-app/src/timeline_workspace/annotation.rs crates/rollshot-app/src/timeline_workspace/project.rs crates/rollshot-app/src/timeline_workspace/mod.rs
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
  - `acquire_project_writer(root: &Path) -> Result<ProjectLockResult, ProjectWorkerError>`
  - `ProjectLockResult::{Acquired(ProjectWriterGuard), AlreadyLocked}`
  - `ProjectAccess::{Writable(ProjectWriterGuard), ReadOnly, CorruptReadOnly}` replacing Task 2 `ProjectOpenMode` in `ProjectSession`
  - `OpenProjectWorkerResult::{Opened(OpenProjectResult), WriterLocked { root: PathBuf }}`
  - `ProjectWorkerError::{Project(ProjectError), Lock { category }, Join { category }}` with structural accessors and UI-only message mapping
  - `SaveProjectWorkerResult::{ExistingSaved(ProjectCommit), NewWritable { commit, guard }, NewCommittedReadOnly { commit, category }}`
  - `load_project_worker(OpenProjectRequest) -> Result<OpenProjectWorkerResult, ProjectWorkerError>`
  - `save_project_worker(SaveProjectRequest) -> Result<SaveProjectWorkerResult, ProjectWorkerError>`.

- [ ] **Step 1: Write failing lock/worker tests**

Mirror `daemon/instance.rs`: second lock reports AlreadyLocked and dropping the guard allows reacquisition. Add async-worker tests proving load/save are submitted through the owned-request `spawn_blocking` wrapper, a revision conflict stays typed and preserves the caller-owned dirty snapshot, corrupt digest/header validation returns a structural frame ID without logging a path, and first Save/Save As return either a guard-owning writable outcome or an explicit committed-read-only outcome if post-commit lock acquisition loses the narrow race. Full PNG decode failures remain Task 1's lazy-load responsibility, not Open-worker behavior.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide project_writer
```

Expected: compile failure because lock/worker types do not exist.

- [ ] **Step 3: Implement exact locking semantics**

Open `<project>/.lock` with read/write/create/no-truncate and call `fs4::FileExt::try_lock(&file)`. `ProjectWriterGuard` owns the `File`; it never writes PID data. Match fs4 1.1 exactly: `Err(fs4::TryLockError::WouldBlock)` becomes `AlreadyLocked`, while `Err(fs4::TryLockError::Error(error))` becomes `ProjectWorkerError::Lock { category: "project_lock" }`. Tracing records only the stable target, category, and error kind — never the full path.

- [ ] **Step 4: Implement blocking workers**

Requests own all data:

```rust
pub struct OpenProjectRequest { pub root: PathBuf, pub writable: bool }
pub struct OpenProjectResult { pub loaded: LoadedProject, pub access: ProjectAccess }
pub enum SaveDestination { FirstSave(PathBuf), Existing(PathBuf), SaveAs(PathBuf) }
pub struct SaveProjectRequest { pub snapshot: ProjectSnapshot, pub destination: SaveDestination }
```

Async wrappers call `tokio::task::spawn_blocking` and preserve `ProjectError` rather than stringifying it; join failure maps to `Join { category: "project_worker_join" }`. Writable Open acquires the guard before load and returns `WriterLocked` without constructing a writable workspace when the lock is held; the host then offers Open Read-Only or Cancel and resubmits with `writable: false` only after the user chooses it. Read-only Open skips locking.

Existing Save leaves the guard in UI-owned `ProjectSession` while the owned snapshot/path run in the worker and returns `ExistingSaved`. First Save/Save As attempts to acquire the new project's `.lock` immediately after Plan 1's atomic directory commit and returns the guard in `NewWritable`. If another process wins that narrow post-commit race, return `NewCommittedReadOnly`: the UI marks the committed revision clean, switches to read-only, and explains that the project was saved but writable ownership was not acquired. Never report this outcome as a failed/dirty Save.

- [ ] **Step 5: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide project_writer
```

Expected: locking, drop release, typed async load/save, revision conflict, and both post-commit lock outcomes pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/project.rs crates/rollshot-app/src/timeline_workspace/mod.rs
rtk git commit -m "feat(app): lock and save action guide projects"
```

---

### Task 4: Add recent-project storage

**Files:**

- Create: `crates/rollshot-app/src/action_guide_home/recent.rs`
- Create: `crates/rollshot-app/src/action_guide_home/mod.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Test: same files

**Interfaces:**

- Consumes: existing daemon config-directory helper.
- Produces: `RecentProjects::load`, `record_open_at`, `remove`, `refresh_availability`, and `save`, max 10.

- [ ] **Step 1: Register the module and write failing recent-file tests**

Create `action_guide_home/mod.rs` with `pub(crate) mod recent;` and register `action_guide_home` behind `action-guide` in `main.rs` so RED tests are compiled. Use a temp config path and injected `now_ms`. Assert malformed/unsupported JSON loads empty, duplicate exact paths move to front, list truncates to ten, missing projects remain with `available = false`, display names are the only title-like content stored, and save uses unique temp + sync + same-directory rename without destroying a prior complete file under real-filesystem failure. Do not canonicalize paths or read project manifests/images during recent-list load.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home::recent
```

Expected: compile failure because recent storage does not exist.

- [ ] **Step 3: Implement versioned recent DTOs**

Use strict versioned recent JSON for writes but lenient whole-file load:

```rust
#[derive(Serialize, Deserialize)]
struct RecentFile { schema_version: u32, entries: Vec<RecentEntry> }
#[derive(Clone, Serialize, Deserialize)]
pub struct RecentEntry { pub path: PathBuf, pub display_name: String, pub last_opened_ms: u64 }
```

`record_open_at(path, display_name, now_ms)` keeps time deterministic and moves an exact path match to the front. `refresh_availability` performs metadata-only checks. Path is permitted in the local file but excluded from all tracing fields. Store under `daemon::config::rollshot_config_dir()?.join("recent-action-guides.json")`; write a unique temp sibling, `sync_all`, rename, then best-effort sync the parent directory.

- [ ] **Step 4: Verify registration does not change launch behavior**

Keep only the `action_guide_home` module registration added for RED. Do not change `LaunchMode` or route Home/Open yet; final CLI activation waits until both platform hosts compile in Task 9.

- [ ] **Step 5: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home::recent
rtk cargo test -p rollshot-app --features action-guide launch::tests
```

Expected: recent storage tests pass and existing launch tests remain unchanged.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/action_guide_home crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): track recent action guide projects"
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
- Produces:
  - `ActionGuideIntent::{Home, Record { fullscreen }, Open { path: Option<PathBuf> }}` as the host-facing intent used before Task 9 activates the CLI.
  - `SelectedDirectoryKind::{Project(PathBuf), LegacyReader(PathBuf), Invalid}` from the blocking shape-inspection helper.
  - `ActionGuideHome`, `Message`, `Effect::{None, PickProject, InspectSelection(PathBuf), RecordNew, OpenProject(PathBuf), OpenLegacyReader(PathBuf)}`, `view`, `update`, and `subscription`.

- [ ] **Step 1: Invoke `iced-rs`, register Home modules, and write failing state tests**

Add `update` and `view` declarations to `action_guide_home/mod.rs` before the RED run. Test Record New, Open picker cancel, selecting available/unavailable recent entries, removing unavailable entries, reloading recent on the existing global `WindowFocused` event path, and applying typed background inspection results for `project.json`, legacy `session.json` without `project.json`, `index.html` reader handoff, missing paths, and invalid selections. Inspection tests use a blocking fake and prove no filesystem inspection occurs inside `update`.

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
    PickProject,
    InspectSelection(PathBuf),
    RecordNew,
    OpenProject(PathBuf),
    OpenLegacyReader(PathBuf),
}
```

Extend the host's existing iced window-event subscription with an explicit `WindowFocused` message; do not add a parallel subscription for the same event family. The host maps `PickProject` to `rfd::AsyncFileDialog`, and maps `InspectSelection` to an owned `spawn_blocking` request. Detection rules are exact: `project.json` means project; otherwise `session.json` means legacy export; otherwise invalid. Do not read manifests or images during shape inspection.

- [ ] **Step 4: Implement the approved view hierarchy**

Render `Record New` as primary, `Open Project...` as secondary, then Recent Projects with display name/time/availability only. Do not render publish freshness. Keep all project paths out of visible default cards; reveal path only in unavailable-entry detail if needed for recovery.

- [ ] **Step 5: Add detached Linux record command helper**

Add a testable `action_guide_record_command(fullscreen: bool)` that resolves the current executable and builds native `OsString` program/arguments for `action-guide --record` plus optional `--fullscreen`; never lossy-convert paths and never use shell interpolation. `spawn_action_guide_record` returns promptly and moves `Child::wait` to a named dedicated reaper thread so the long-lived Home does not accumulate zombies and iced/Tokio shutdown does not wait on a long-running `spawn_blocking` task. Spawn failure returns to Home; reaper failure emits only a privacy-safe tracing category. macOS does not use this helper.

- [ ] **Step 6: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home
rtk cargo test -p rollshot-app --features action-guide platform_actions
```

Expected: Home state, background inspection, window-focus refresh, native command, and child-reaping tests pass.

- [ ] **Step 7: Commit**

```bash
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
- Modify: `crates/rollshot-app/src/timeline_workspace/guide_export.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/storyboard_copy.rs`
- Test: same files

**Interfaces:**

- Consumes: Tasks 1–3 runtime/project APIs.
- Produces:
  - complete Timeline project lifecycle messages and state;
  - `Update { task: Task<Message>, effect: Effect }` and `Effect::{None, CloseWorkspace}` so shared state never chooses daemon exit vs. Home;
  - a bounded `FrameLoadCoordinator` with generation token and two decode permits.

- [ ] **Step 1: Write failing lifecycle tests**

Cover:

- fresh recording starts with save-first prompt;
- Save picker cancel returns to prompt;
- `Save later` enters Unsaved Project;
- every mutation arm (title, step title/caption, delete, keyframe, annotation apply/delete/undo/redo, accepted agent proposal) marks dirty;
- last step cannot be deleted;
- first Save and existing Save transition Saving → Saved and update base revision;
- conflict keeps dirty edits and shows recoverable error;
- first Save/Save As post-commit lock loss becomes clean committed read-only with a clear banner;
- close dirty gives Save and Close / Discard / Cancel; picker cancel returns to workspace;
- `CloseWorkspace` is emitted only after clean close, explicit discard, or successful Save-and-Close; picker cancellation/failure emits no host effect;
- read-only disables every mutation and Save;
- corrupt lazy decode changes access to CorruptReadOnly and disables Save.
- selecting a step schedules its current keyframe and uncached nearby strip frames; at most two decodes run; queued stale generations skip decode; stale completions from the previously selected step are ignored; and renderer-handle count never exceeds current keyframe + nearby.
- project-backed workspaces reject/hide existing standalone Guide, GIF, Storyboard, MP4, and Issue Pack actions, while `StepFrameSource::InMemory` keeps their current behavior.

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
pub enum Effect { None, CloseWorkspace }
```

Timeline replaces the direct `FrameStore` field with `StepFrameSource`, holds `ProjectSession`, enabled outputs, these lifecycle states, a `FrameLoadCoordinator`, and sets of required/loading frame IDs for the selected step. Fresh recordings use `StepFrameSource::InMemory` and show the save-first prompt; reopened projects use `Project`. Every persisted mutation passes a central `can_mutate()` gate and calls one `mark_project_dirty()` helper only after the mutation succeeds. Draft selection/tool/modal changes do not mark dirty. Update tests enumerate every current mutation arm, including annotation explanation changes and both caption/visual-agent apply paths.

Return `Update { task, effect }` from shared Timeline update logic. The existing direct-record Timeline runner maps `CloseWorkspace` to `iced::exit`; the Linux/macOS phased hosts map it to Home. No nested update arm calls `iced::exit` directly.

- [ ] **Step 4: Move frame resolution to async tasks**

Selecting a project-backed step clears old renderer handles, computes the current keyframe plus ordered nearby strip IDs, uses cached images immediately, and advances the shared atomic generation once. Each miss awaits one of two workspace semaphore permits, rechecks the generation before starting `spawn_blocking`, and then runs `load_step_frame` without decoding unrelated steps. Tokio blocking work already started is not treated as cancellable. The main preview shows a step-local loading state until the current keyframe arrives; nearby thumbnails fill progressively.

Each completion first verifies generation/selected-step. A stale result is dropped without cache insertion or handle creation. A current result inserts into the byte-bounded cache, builds only the selected step's relevant handle, and hydrates pending annotations only for the current keyframe. Any required-frame decode failure sets CorruptReadOnly with structural frame/step category. Add a module doc-comment diagram for generation, permits, stale-drop, cache, and hydration; keep it synchronized with the diagram in this plan.

- [ ] **Step 5: Implement Save and close chains**

First Save/Save As picker produces an owned snapshot before worker start. Existing Save uses base revision while the UI retains its writer guard. Handle every typed Task 3 outcome explicitly: normal success updates root/revision/guard and clears dirty; `NewCommittedReadOnly` also clears dirty but installs read-only access and a warning; revision/destination/worker failure preserves dirty state. Update recent metadata only after a committed outcome and leave publish status stale for Plan 3. `SaveThenClose` emits `CloseWorkspace` only after a committed outcome; picker cancellation or pre-commit failure returns to the workspace without discard.

- [ ] **Step 6: Implement view states**

Add the save-first warning copy, `Unsaved changes` / `Saving` / `Saved`, read-only lock banner, committed-but-read-only warning, corrupt project banner, loading placeholder, and disabled mutation controls. Gate mutation in update as well as view. Hide/disable existing standalone output actions when `StepFrameSource::in_memory()` is `None`; adapt `guide_export.rs` and `storyboard_copy.rs` to request the in-memory source explicitly instead of assuming a `store` field. Keep the existing annotation/keyframe layout and do not add publish detail UI yet.

- [ ] **Step 7: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
```

Expected: lifecycle, mutation authorization/dirty matrix, bounded lazy loading, host close effect, committed-read-only, annotation hydration, and in-memory legacy-output regression tests pass.

- [ ] **Step 8: Commit**

```bash
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

- Consumes: Tasks 4–6 Home/Timeline states and `ActionGuideIntent`.
- Produces: `run(initial: ActionGuideIntent) -> Result<(), String>` with `Phase::{Home, Opening, LockConflict, Timeline}`.

- [ ] **Step 1: Invoke `iced-rs`, register the Linux module, and write transition tests**

Register `action_guide_linux_product` behind `cfg(all(target_os = "linux", feature = "action-guide"))` in `main.rs` so the RED test file is compiled, without routing to it yet. Test initial Home, initial Open path/picker, Home Record effect command, Home Open → Opening → Timeline, WriterLocked → LockConflict → Open Read-Only/Cancel, legacy reader handoff remains Home, Timeline `CloseWorkspace` → Home, stale worker/window messages are ignored, and no second iced event loop invocation.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_linux_product
```

Expected: compile failure because the registered Linux host types and phase logic are not implemented.

- [ ] **Step 3: Implement daemon phase/message delegation**

Use one `iced::daemon` with one decorated window for Home/Timeline and add a module doc-comment phase diagram. `Message::{Home, Timeline, SelectionInspected, ProjectOpened, OpenReadOnly, CancelLockedOpen, WindowReady}` delegates to shared update/view/subscription functions and interprets their host effects. `Record New` starts the reaped current-executable child and leaves Home open. Open performs typed Task 3 async load before switching phase; `WriterLocked` shows exactly Open Read-Only and Cancel, and only the former resubmits a read-only request. Timeline `CloseWorkspace` returns to Home without closing or recreating the decorated window.

- [ ] **Step 4: Keep the host dormant until final routing**

Keep only the `main.rs` module registration added for RED; do not route launch modes yet. Task 9 activates the host only after the macOS host is also ready. Direct Record will retain the existing overlay → recording → Timeline runner in the child process, including the save-first prompt.

- [ ] **Step 5: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_linux_product
```

Expected: pure phase/effect tests pass and the new module contains no nested daemon invocation.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/action_guide_linux_product.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): host action guide home on Linux"
```

---

### Task 8: Add Home/Open phases to the macOS product daemon

**Files:**

- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-app/src/macos_product.rs`

**Interfaces:**

- Consumes: shared Home/Timeline states, `ActionGuideIntent`, and existing macOS capture component.
- Produces: macOS `Phase::{Home, Capture, Timeline, ...}` transitions for Action Guide launch modes.

- [ ] **Step 1: Invoke `iced-rs` and write phase tests**

Cover Home launch, direct Open/picker, direct Record, Home Record New entering in-loop Action Guide capture, capture completion showing save-first Timeline, WriterLocked → Open Read-Only/Cancel, Timeline `CloseWorkspace` returning Home, Open failure returning Home with message, stale window/worker messages, and exactly one daemon invocation.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide macos_product::tests::action_guide_project_
```

Expected: missing Home/Open phase behavior.

- [ ] **Step 3: Extend phase/message/update/view/subscription**

Add Home, Opening, and LockConflict phase variants plus mapped messages and update the module's existing phase diagram. Reuse the existing one daemon and capture `Component`; do not spawn a macOS recording child. Add an Action Guide boot path that can construct Home/Open state without eagerly constructing `Component`. Home `Record New` constructs the existing Action Guide overlay config and transitions to Capture within update; existing `complete_action_recording` constructs a Timeline with save-first prompt visible. Lock conflict uses the same Open Read-Only/Cancel behavior as Linux, and Timeline close effects return to Home.

- [ ] **Step 4: Keep the host API ready for final routing**

Expose an Action Guide host entry that accepts `ActionGuideIntent`, but do not edit `main.rs` yet. Preserve the existing generic capture entry and fullscreen capture behavior; Task 9 performs the final routing switch atomically across platforms.

- [ ] **Step 5: Run tests**

```bash
rtk cargo test -p rollshot-app --features action-guide macos_product
```

Expected: macOS phase/effect tests pass under `--features action-guide`, while existing screenshot/OCR product tests remain green.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(app): host action guide home on macOS"
```

---

### Task 9: Activate the Action Guide CLI and platform routing

**Files:**

- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Test: same files

**Interfaces:**

- Consumes: Tasks 7–8 platform hosts and Task 5 `ActionGuideIntent`.
- Produces:
  - `ActionGuideLaunch::{Home, Record { fullscreen }, Open { path: Option<PathBuf> }}`
  - `LaunchMode::ActionGuide(ActionGuideLaunch)`
  - final Linux/macOS routing with `ActionGuideProbe` unchanged.

- [ ] **Step 1: Write the failing CLI and route matrix**

Add exact parse cases:

```rust
assert_eq!(parse(&["rollshot-app", "action-guide"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Home)));
assert_eq!(parse(&["rollshot-app", "action-guide", "--record"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Record { fullscreen: false })));
assert_eq!(parse(&["rollshot-app", "action-guide", "--record", "--fullscreen"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Record { fullscreen: true })));
assert!(parse(&["rollshot-app", "action-guide", "--fullscreen"]).unwrap_err().contains("--record"));
assert!(parse(&["rollshot-app", "action-guide", "--record", "--open"]).is_err());
assert_eq!(parse(&["rollshot-app", "action-guide", "--open"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Open { path: None })));
assert_eq!(parse(&["rollshot-app", "action-guide", "--open", "/tmp/a.rollshot-guide"]), Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Open { path: Some(PathBuf::from("/tmp/a.rollshot-guide")) })));
```

Represent optional `--open [PATH]` with `Option<Option<PathBuf>>`; clap derives `num_args = 0..=1`. Add pure route-selection tests proving Linux Home/Open use the Linux host, Linux Record uses the existing child overlay path, macOS all three intents use the one product daemon, and Probe is unchanged.

- [ ] **Step 2: Run tests and verify failure**

```bash
rtk cargo test -p rollshot-app --features action-guide launch::tests
rtk cargo test -p rollshot-app --features action-guide action_guide_route
```

Expected: new CLI variants and route-selection helpers do not exist.

- [ ] **Step 3: Implement strict parsing and atomic routing switch**

Use clap conflicts/requires so `--record` and `--open` are exclusive and `--fullscreen` requires `--record`. Map parsed variants to Task 5 `ActionGuideIntent`. Register `action_guide_linux_product` only on Linux. Home/Open route into the platform phased host; Linux Record retains `run_action_guide_record`; macOS Home/Open/Record all enter the existing single product daemon through its Action Guide intent entry. Preserve `ActionGuideProbe` exactly.

- [ ] **Step 4: Run focused and cross-platform feature tests**

```bash
rtk cargo test -p rollshot-app --features action-guide launch::tests
rtk cargo test -p rollshot-app --features action-guide action_guide_route
rtk cargo check -p rollshot-app --all-targets --features action-guide
```

Expected: CLI and route tests pass, and the feature compiles for the active target without an unhandled launch variant.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): route action guide projects"
```

---

### Task 10: Document and verify the app integration slice

**Files:**

- Modify: `README.md`
- Test: workspace commands below

**Interfaces:**

- Consumes: Tasks 1–9.
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
- Lazy cache is byte-bounded at 256 MiB, active decode concurrency is capped at two, selected renderer handles are bounded, and corruption downgrades to read-only.
- Save/Save As, committed-but-read-only lock race, dirty close host effects, revision conflict, and writer lock behaviors are verified.
- Recent projects refresh after the Linux recording child returns focus.
- Project-backed workspaces cannot accidentally call in-memory-only legacy output paths before Plan 3.
- Product tests, fmt, and clippy pass before Plan 3 starts.
