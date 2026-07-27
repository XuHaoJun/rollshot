# Agent Foundation Slice 4: Live Job Registry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded process-local live-job registry and migrate Action Guide video import so progress, cancellation, terminal truth, and collect-once results survive transient iced message loss without adding durable recovery or visible UI changes.

**Architecture:** `rollshot-agent::jobs` provides an iced-free generic `LiveJobRegistry<P, R>` whose owner handle admits jobs, routes cancellation, retains authoritative snapshots and successful results, and exposes a monotonic watch channel. `rollshot-app` instantiates it with `VideoImportProgress` and `ImportedWorkspaceSeed`; `ImportCoordinator` remains presentation/preparation state while `CancellableChild` and `ImportedScratch` retain concrete child/scratch ownership.

**Tech Stack:** Rust 2021, `std::sync`, Tokio `watch`, `uuid` v4, iced 0.14 `Task`/`Subscription::run_with`, existing `rollshot-action` video-import process and scratch contracts, `tracing`, Cargo tests.

## Global Constraints

- Governing spec: `docs/superpowers/specs/2026-07-27-agent-foundation-live-job-registry-design.md`.
- Gate G2 is the prerequisite; do not modify the historical Slice 3 spec, plan, or decision record.
- Registry state is process-local only. Never serialize a Job record, PID, process handle, cancellation callback, result payload, path, media, or raw process output.
- V1 limits are fixed: 4 active Jobs, 4 active-or-uncollected result slots,
  128 retained terminal metadata records, 5-minute logical terminal/result
  TTL, 64 diagnostic entries, 256 bytes per entry.
- `JobKind` V1 contains only `ActionGuideVideoImport`; `JobExecutionClass` V1 contains only `LocalWorkerWithChildProcesses`.
- Admit only the direct Action Guide user action. Represent agent-task authority but return `UnsupportedAuthoritySource`; do not add or reuse a `RunOperation`.
- `ImportOperationId` remains the pre-job identity for toolchain resolution/setup. `JobId` begins only at worker admission.
- Cancellation request is not cancellation confirmation. `Cancelled` is written only after the worker returns and concrete child/scratch cleanup has run.
- A success report while `Cancelling` drops the result and terminalizes as `Cancelled`.
- Keep `CancellableChild` and `ImportedScratch` as concrete resource owners. The registry stores no PID or child handle.
- Preserve current Linux and macOS Action Guide behavior through the shared `action_guide_home` path; no platform-specific registry implementation.
- Preserve current visible layout, copy, progress presentation, supported extensions, toolchain setup, import algorithm, result workspace handoff, and error semantics.
- No golden visual baseline change. If implementation changes visible iced behavior, stop and invoke `testing-iced-ui` before that edit.
- Use `tracing` only, with stable `rollshot::agent::jobs` or `rollshot::app::action_guide::video_import` targets and privacy-safe fields.
- Use `rtk` for every shell command.
- Stop rather than weaken the design if iced task teardown destroys the only worker/terminal repair path, result movement requires cloning/type erasure, cancellation cannot distinguish requested from confirmed, the update loop must block, or Linux/macOS require divergent contracts.

---

## Engineering Review Lock

This plan received one `plan-eng-review` pass in auto mode on 2026-07-27.
Scope stayed intact: 2 new files, 6 modified files, and 6 sequential tasks do
not trigger the complexity threshold.

### Auto decisions

#### Auto decision D1 — How should the five-minute TTL reclaim memory?

Context: The registry accepts deterministic timestamps, but a background timer
would add runtime ownership solely for reclamation.

ELI10: A timer can throw old results away at exactly five minutes, but it also
creates another task that must start, stop, and be tested. Lazy expiry makes a
result unusable at five minutes and frees it on the next registry operation or
owner drop.

Stakes if we pick wrong: sensitive in-memory results may live longer than the
stated policy, or timer lifecycle bugs may outlive the registry.

Recommendation: **D1A — logical TTL with lazy physical reclamation** because it
keeps the process-local registry boring while preserving observable expiry.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

- **D1A — lazy reclamation (recommended)** `(human: ~0.5 day / AI: ~20 min; low
  risk; low maintenance)`
  ✅ `collect` cannot return an expired result, and owner drop frees all memory.
  ❌ Physical memory may remain past five minutes until the next registry call.
- **D1B — background expiry task** `(human: ~2 days / AI: ~2 hours; medium risk;
  medium maintenance)`
  ✅ Reclaims close to the wall-clock deadline without another caller.
  ❌ Adds runtime startup, timer cancellation, and fake-clock complexity.

Net: enforce expiry at every observable boundary; record lazy physical
reclamation explicitly instead of pretending a timer exists.

#### Auto decision D2 — How many heavy uncollected results may be retained?

Context: 128 terminal metadata records are cheap, but 128
`ImportedWorkspaceSeed` values can retain substantial frame metadata and
scratch trees.

ELI10: A short status card is small; an imported project is not. The registry
must reserve a result slot before starting work so success never forces it to
discard somebody else’s output.

Stakes if we pick wrong: repeated lost UI notifications can retain unboundedly
expensive scratch/results inside the nominal terminal cap.

Recommendation: **D2A — cap active plus uncollected results at four** because it
matches the active bound and prevents success-time eviction.

Completeness: D2A=10/10, D2B=6/10.

Pros / cons:

- **D2A — four reserved result slots (recommended)** `(human: ~0.5 day / AI:
  ~30 min; low risk; low maintenance)`
  ✅ Hard-bounds heavy results before launch and never evicts accepted output.
  ❌ A stale uncollected result can temporarily block a new import until expiry.
- **D2B — rely on 128 terminal records** `(human: ~0 / AI: ~0; medium risk; low
  maintenance)`
  ✅ Fewer admission checks.
  ❌ Confuses cheap metadata capacity with heavy result capacity.

Net: terminal metadata and result capacity are separate resources.

#### Auto decision D3 — What does reporter drop mean during cancellation?

Context: Cancellation can arrive while a worker is still `Starting`; a failed
`mark_running` followed by reporter drop must not leave `Cancelling` forever.

ELI10: When the worker handle disappears during cancellation, its stack has
already dropped the child and scratch owners. That is the cleanup confirmation
the registry was waiting for.

Stakes if we pick wrong: the UI detaches but the retained Job stays
`Cancelling` until expiry with no worker left to confirm it.

Recommendation: **D3A — reporter drop maps `Cancelling` to `Cancelled`** because
the reporter lease is the worker cleanup boundary.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

- **D3A — confirm cancellation on reporter drop (recommended)** `(human: ~0.5
  day / AI: ~20 min; low risk; low maintenance)`
  ✅ Prevents stuck cancellation while preserving `WorkerAbandoned` for
  `Starting|Running`.
  ❌ Relies on the worker keeping concrete resources inside the reporter’s
  unwind scope.
- **D3B — always mark drop as `WorkerAbandoned`** `(human: ~0.25 day / AI: ~10
  min; low implementation risk; medium semantic risk)`
  ✅ One simpler drop branch.
  ❌ Turns a requested-and-cleaned cancellation into a failure.

Net: state-dependent drop is explicit and matches existing RAII ownership.

#### Auto decision D4 — Should the Action Guide cutover be split?

Context: `Message`, `Effect`, coordinator, worker, and both platform callsites
share one compile-time contract.

ELI10: Splitting the change would temporarily keep two sources of terminal
truth or leave one platform uncompilable. One larger atomic task is safer than
a smaller misleading intermediate state.

Stakes if we pick wrong: stale payload messages can coexist with registry
terminal state and open a result twice.

Recommendation: **D4A — keep Task 4 atomic** because clean cutover beats a
temporary compatibility path.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

- **D4A — atomic cross-file cutover (recommended)** `(human: ~2 days / AI: ~2
  hours; medium review size; low migration risk)`
  ✅ Every caller moves together and obsolete messages disappear immediately.
  ❌ The task touches four files and must be reviewed as one unit.
- **D4B — staged dual path** `(human: ~3 days / AI: ~3 hours; high migration
  risk; high maintenance)`
  ✅ Smaller individual diffs.
  ❌ Requires a shim and two competing terminal authorities.

Net: this is one contract migration, not parallel feature work.

#### Auto decision D5 — What runtime smoke evidence is required?

Context: State-machine tests do not execute the real FFprobe/FFmpeg import
pipeline.

ELI10: Fake workers prove the traffic lights change correctly; one real fixture
proves a car can still drive through. The fixture is conditional because CI
machines may not install FFmpeg.

Stakes if we pick wrong: the registry can pass every unit test while the real
video import path fails after launch.

Recommendation: **D5A — run the existing fixture-backed import when FFmpeg is
available and record an explicit skip otherwise**.

Completeness: D5A=9/10, D5B=7/10.

Pros / cons:

- **D5A — conditional real fixture smoke (recommended)** `(human: ~0.25 day /
  AI: ~10 min; low risk; low maintenance)`
  ✅ Exercises real child processes, progress, scratch, and final seed.
  ❌ Linux evidence does not replace macOS runtime verification.
- **D5B — unit/contract suites only** `(human: ~0 / AI: ~0; low test cost;
  medium integration risk)`
  ✅ Runs everywhere without external tools.
  ❌ Leaves the concrete migrated workload unexecuted.

Net: run the thing where available and name the remaining platform risk.

### What already exists

- `ImportCoordinator` already supplies pre-job identity, presentation state,
  stale-message rejection, and source-path clearing; the plan narrows it rather
  than replacing it.
- `VideoImportCancellation`, `CancellableChild`, and `ImportedScratch` already
  supply cooperative cancellation, kill/wait, bounded stderr, RAII cleanup,
  and startup scratch scavenging; the registry routes to these owners.
- iced 0.14 `Task`, `Subscription::run_with`, and Tokio `watch` already supply
  finite worker execution, stable subscription identity, and coalesced latest
  notification; no custom executor or event bus is built.
- Slice 2 IDs and Slice 3 `AuthoritySnapshot` already represent Product Task
  correlation and immutable agent authority; V1 Job admission reuses their
  types but intentionally rejects agent starts.

### NOT in scope

- Durable Job serialization, restart reattachment, PID adoption, and remote
  receipts — process-local lifetime is the approved slice boundary.
- Agent/tool Job start — no honest dedicated `RunOperation` or product workload
  exists yet.
- Managed FFmpeg setup cancellation/atomic installation — setup remains
  pre-job and has separate supply-chain risks.
- New visible progress, copy, layout, or controls — the existing UI projection
  remains unchanged.
- Workflow DAGs, retries, queues, priorities, child agents, and parallel tool
  scheduling — none are required to migrate one live product operation.
- A new workspace crate — one shared module and one consumer do not justify a
  package boundary.

### Test coverage

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / current child reaping and scratch cleanup | ✓ | ✓ | — | no |
| Task 2 / IDs, admission, capacity, active transitions | ✓ | — | — | no |
| Task 3 / cancel, terminal, progress, collect, TTL, shutdown | ✓ | — | — | no |
| Task 4 / registry-backed shared app state and both callsites | ✓ | ✓ | — | no |
| Task 5 / races, notification loss, panic/drop, privacy | ✓ | ✓ | — | no |
| Task 6 / real FFmpeg fixture when installed | — | ✓ | ✓ | conditional |
| macOS native runtime | — | compile/shared tests | — | residual risk |

### Failure modes

| Codepath | Production failure | Test / handling | User outcome |
|---|---|---|---|
| Admission | capacity or unsupported authority | Task 2 Steps 1–6; `JobAdmissionError` | fixed bounded start error |
| Worker start | reporter lost or task join fails | Task 3 Step 3, Task 5 Step 2; `WorkerAbandoned` | fixed worker-stopped error |
| Worker body | panic | Task 4 Step 6, Task 5 Steps 2/5; `WorkerPanic` | fixed worker-stopped error |
| Progress delivery | watch updates coalesce | Task 3 Step 2, Task 5 Step 2; snapshot repair | latest progress/terminal shown |
| Cancel | request races with success | Task 3 Step 1, Task 5 Step 2; result dropped to `Cancelled` | no timeline opens |
| Child cleanup | decoder stalls | Task 1 Step 4, Task 6 Step 1; kill/wait within two seconds | cancellation completes |
| Result handoff | duplicate or expired collect | Task 3 Step 3, Task 5 Step 3; typed collect error | no duplicate timeline |
| Restart | stale scratch remains | Task 1 Step 4, Task 6 Step 1; lock-aware scavenger | cleanup on later launch |
| Privacy | payload/path reaches Debug or tracing | Task 5 Step 4; closed fields/categories | no sensitive diagnostic |

No failure mode is both silent and uncovered.

### Execution dependencies and parallelization

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1 | `rollshot-app/action_guide_home`, `rollshot-action/video_import` | — |
| Task 2 | `rollshot-agent` | Task 1 baseline |
| Task 3 | `rollshot-agent` | Task 2 |
| Task 4 | `rollshot-app/action_guide_home`, Linux/macOS product roots | Tasks 1 and 3 |
| Task 5 | `rollshot-agent`, `rollshot-app/action_guide_home` | Task 4 |
| Task 6 | all affected modules | Tasks 1–5 |

Sequential execution, no parallelization opportunity. Every later task consumes
the preceding contract or touches the same primary modules; parallel worktrees
would create avoidable merge and semantic conflicts.

### Review completion

- Step 0: Scope Challenge — accepted as-is; no complexity trigger.
- Architecture Review — 3 issues resolved by D1–D3.
- Plan Structure + Code Quality — 1 issue resolved by D4.
- Test Review — coverage table produced; 1 gap resolved by D5.
- Performance Review — 1 resource-bound issue resolved by D2.
- Unresolved decisions — 0.

Plan is locked for execution after the documentation-only planning branch is
accepted.

---

## File Structure

### New files

- `crates/rollshot-agent/src/jobs.rs` — typed Job identity/admission/state, bounded process-local registry, worker reporter, observer snapshots, cancellation, collection, retention, shutdown, privacy-safe diagnostics, and unit tests.
- `docs/superpowers/spikes/2026-07-27-live-job-registry-decision.md` — created only after implementation verification and independent review; Slice 4 gate evidence and residual risks.

### Modified files

- `crates/rollshot-agent/Cargo.toml` — add the workspace `uuid` dependency used for registry-generated `JobId`.
- `crates/rollshot-agent/src/lib.rs` — export `pub mod jobs;`.
- `crates/rollshot-app/src/action_guide_home/video_import.rs` — keep pre-job `ImportOperationId`; replace worker cancellation/result ownership with current `JobId` presentation binding and snapshot projection.
- `crates/rollshot-app/src/action_guide_home/update.rs` — own the typed registry, perform checked admission, reconcile registry snapshots, move successful results once, replace payload-bearing import messages, and build the stable iced job-watch subscription.
- `crates/rollshot-app/src/action_guide_linux_product.rs` — pass the registry-backed reporter into the shared worker and subscribe through the current `ActionGuideHome`.
- `crates/rollshot-app/src/macos_product.rs` — make the same effect/subscription cutover for `Home`, `Opening`, and `LockConflict` phases.

### Intentionally unchanged

- `crates/rollshot-action/src/video_import/process.rs` — existing direct-child kill/wait and bounded stderr remain the concrete process contract.
- `crates/rollshot-action/src/video_import/scratch.rs` — existing RAII and lock-aware startup scavenging remain the crash-cleanup contract.
- `crates/rollshot-app/src/action_guide_home/view.rs` — presentation is unchanged.
- Product Task, authority, skill, provider, artifact, managed FFmpeg, and timeline workspace files — outside this slice.

---

### Task 1: Freeze Existing Import Cancellation and Cleanup Behavior

**Files:**
- Modify: `crates/rollshot-app/src/action_guide_home/video_import.rs:124-253`
- Verify unchanged: `crates/rollshot-action/src/video_import/process.rs:596-724`
- Verify unchanged: `crates/rollshot-action/src/video_import/scratch.rs:163-279`

**Interfaces:**
- Consumes: current `ImportCoordinator::set_cancellation`, `ImportCoordinator::cancel`, `VideoImportCancellation::is_cancelled`, `CancellableChild`, and `cleanup_stale_import_scratch` behavior.
- Produces: an executable pre-migration assertion that UI detachment signals the active worker token before coordinator state is cleared; this is the preservation oracle for Task 4.

- [ ] **Step 1: Run the current focused baseline before editing**

Run:

```bash
rtk cargo test -p rollshot-action video_import
rtk cargo test -p rollshot-app --features action-guide action_guide_home::video_import::tests
```

Expected: 57 focused `rollshot-action` tests pass and 10 coordinator tests pass. Stop if cancellation reaping, stalled-decoder, scratch cleanup, or coordinator tests fail; later tasks must not absorb a baseline defect.

- [ ] **Step 2: Add a cancellation-signal preservation test**

Append inside `action_guide_home::video_import::tests`:

```rust
#[test]
fn cancel_signals_worker_before_coordinator_detaches() {
    let mut coordinator = ImportCoordinator::default();
    let id = coordinator.begin(PathBuf::from("test.mp4"));
    let cancellation = VideoImportCancellation::default();
    let observed = cancellation.clone();
    coordinator.set_cancellation(cancellation);

    coordinator.cancel(id);

    assert!(observed.is_cancelled());
    assert_eq!(coordinator.state(), ImportState::Idle);
    assert!(coordinator.operation_id().is_none());
    assert!(coordinator.pending_path().is_none());
}
```

This test passes before migration and must be rewritten—not deleted—in Task 4 to assert the same signal through `LiveJobRegistry::cancel`.

- [ ] **Step 3: Run the preservation test**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home::video_import::tests::cancel_signals_worker_before_coordinator_detaches -- --exact
```

Expected: PASS.

- [ ] **Step 4: Re-run concrete child and scratch tests**

Run:

```bash
rtk cargo test -p rollshot-action video_import::process::tests::cancellation_kills_and_waits_for_child -- --exact
rtk cargo test -p rollshot-action video_import::process::tests::drop_reaps_child_on_early_return -- --exact
rtk cargo test -p rollshot-action video_import::process::tests::analysis_cancel_interrupts_a_stalled_decoder -- --exact
rtk cargo test -p rollshot-action video_import::scratch::tests --no-fail-fast
```

Expected: all pass; stalled decoder returns inside its existing two-second assertion.

- [ ] **Step 5: Commit the preservation test**

```bash
rtk git add crates/rollshot-app/src/action_guide_home/video_import.rs
rtk git commit -m "test(action-guide): lock import cancellation handoff"
```

---

### Task 2: Typed Job Identity, Admission, and Active State

**Files:**
- Modify: `crates/rollshot-agent/Cargo.toml:9-25`
- Modify: `crates/rollshot-agent/src/lib.rs:1-10`
- Create: `crates/rollshot-agent/src/jobs.rs`

**Interfaces:**
- Consumes: `ProductTaskId`, `TaskAttemptId`, `RunId`, and `AuthoritySnapshot` from completed Slices 2 and 3; Tokio from existing `rollshot-agent` dependencies; workspace `uuid` v4.
- Produces:
  - `JobId`, `JobKind`, `JobExecutionClass`, `ProductSurface`, `JobTaskRef`, `JobOwner`, `DirectUserAction`, `JobAuthoritySource`, and checked `JobAdmission`;
  - `JobControl`, `JobState`, `JobSnapshot<P>`, `JobAdmissionError`, and
    `JobTransitionError`;
  - `LiveJobRegistry<P, R>::new`, `admit`, `snapshot`, and `mark_running` through `JobReporter<P, R>`.

- [ ] **Step 1: Add compile-failing identity and admission tests**

Create `jobs.rs` with a `#[cfg(test)] mod tests` and fixed helpers:

```rust
fn task_id() -> ProductTaskId {
    ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
}

fn run_id() -> RunId {
    RunId::parse("run-00000000-0000-4000-8000-000000000002").unwrap()
}

fn direct_admission(nonce: u64) -> JobAdmission {
    JobAdmission::action_guide_video_import(nonce)
}

fn no_op_control() -> JobControl {
    JobControl::new(|| {})
}
```

Add tests with these exact contracts:

```rust
#[test]
fn admitted_job_has_typed_unique_identity_and_exact_metadata() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let (first, _) = registry.admit(direct_admission(7), no_op_control(), 100).unwrap();
    let (second, _) = registry.admit(direct_admission(8), no_op_control(), 101).unwrap();

    assert_ne!(first, second);
    assert!(first.as_str().starts_with("job-"));
    let snapshot = registry.snapshot(&first).unwrap();
    assert_eq!(snapshot.kind(), JobKind::ActionGuideVideoImport);
    assert_eq!(
        snapshot.execution_class(),
        JobExecutionClass::LocalWorkerWithChildProcesses
    );
    assert_eq!(
        snapshot.owner(),
        &JobOwner::DirectProductAction {
            surface: ProductSurface::ActionGuideHome,
            operation_nonce: 7,
        }
    );
    assert_eq!(snapshot.state(), JobState::Starting);
    assert_eq!(snapshot.revision(), 1);
}

#[test]
fn agent_task_authority_is_represented_but_rejected_before_allocation() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let authority = authority_fixture(task_id(), TaskAttemptId::new(1), run_id());
    let task = JobTaskRef::new(task_id(), TaskAttemptId::new(1), run_id());
    let admission = JobAdmission::agent_task(
        JobKind::ActionGuideVideoImport,
        JobExecutionClass::LocalWorkerWithChildProcesses,
        authority,
        task,
    );

    assert_eq!(
        registry.admit(admission, no_op_control(), 100).unwrap_err(),
        JobAdmissionError::UnsupportedAuthoritySource
    );
    assert!(registry.list().is_empty());
}

#[test]
fn direct_authority_cannot_claim_product_task_ownership() {
    let admission = JobAdmission::for_test(
        JobKind::ActionGuideVideoImport,
        JobExecutionClass::LocalWorkerWithChildProcesses,
        JobOwner::ProductTask(JobTaskRef::new(
            task_id(),
            TaskAttemptId::new(1),
            run_id(),
        )),
        JobAuthoritySource::DirectUserAction(DirectUserAction::ActionGuideVideoImport),
    );
    let registry = LiveJobRegistry::<u32, String>::new();

    assert_eq!(
        registry.admit(admission, no_op_control(), 100).unwrap_err(),
        JobAdmissionError::OwnerAuthorityMismatch
    );
    assert!(registry.list().is_empty());
}
```

Use this real authority fixture so rejection cannot borrow an unrelated grant:

```rust
fn authority_fixture(
    task_id: ProductTaskId,
    attempt_id: TaskAttemptId,
    run_id: RunId,
) -> AuthoritySnapshot {
    let state = AnnotationStateV1 {
        width: 100,
        height: 80,
        state_id: 1,
        annotations: vec![],
    };
    let document =
        DocumentContentBinding::new([0xAB_u8; 32], &state, 1).unwrap();
    AuthoritySnapshot::new(
        AuthorityBinding::new(task_id, attempt_id, run_id, document),
        "job-test-policy-v1".into(),
        DisclosureCeiling::OcrLayoutOnly,
        false,
        BTreeSet::new(),
        BTreeSet::new(),
    )
    .unwrap()
}
```

- [ ] **Step 2: Add compile-failing active-capacity and transition tests**

```rust
#[test]
fn fifth_active_job_is_rejected_without_evicting_active_work() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let mut ids = Vec::new();
    for nonce in 0..4 {
        ids.push(
            registry
                .admit(direct_admission(nonce), no_op_control(), nonce)
                .unwrap()
                .0,
        );
    }

    assert_eq!(
        registry
            .admit(direct_admission(4), no_op_control(), 4)
            .unwrap_err(),
        JobAdmissionError::ActiveLimit { limit: 4 }
    );
    assert_eq!(registry.list().len(), 4);
    assert!(ids.iter().all(|id| registry.snapshot(id).is_some()));
}

#[test]
fn reporter_moves_starting_to_running_once() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let (id, mut reporter) = registry.admit(direct_admission(7), no_op_control(), 100).unwrap();

    reporter.mark_running(101).unwrap();
    assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Running);
    assert_eq!(registry.snapshot(&id).unwrap().revision(), 2);
    assert_eq!(
        reporter.mark_running(102).unwrap_err(),
        JobTransitionError::InvalidTransition {
            from: JobState::Running,
            operation: "mark_running",
        }
    );
}
```

- [ ] **Step 3: Run tests and verify the expected compile failure**

Run:

```bash
rtk cargo test -p rollshot-agent jobs::tests --no-fail-fast
```

Expected: FAIL because `jobs` is not exported and the listed types do not exist.

- [ ] **Step 4: Add the dependency and export the module**

Add to `crates/rollshot-agent/Cargo.toml`:

```toml
uuid = { workspace = true }
```

Add to `crates/rollshot-agent/src/lib.rs`:

```rust
pub mod jobs;
```

- [ ] **Step 5: Implement the closed V1 contracts**

Use these public shapes; keep fields private and provide read-only accessors:

```rust
pub const MAX_ACTIVE_JOBS: usize = 4;
pub const MAX_UNCOLLECTED_RESULT_SLOTS: usize = 4;
pub const MAX_TERMINAL_JOBS: usize = 128;
pub const TERMINAL_TTL_MS: u64 = 5 * 60 * 1000;
pub const MAX_DIAGNOSTIC_ENTRIES: usize = 64;
pub const MAX_DIAGNOSTIC_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    ActionGuideVideoImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobExecutionClass {
    LocalWorkerWithChildProcesses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductSurface {
    ActionGuideHome,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobTaskRef {
    task_id: ProductTaskId,
    attempt_id: TaskAttemptId,
    run_id: RunId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JobOwner {
    DirectProductAction {
        surface: ProductSurface,
        operation_nonce: u64,
    },
    ProductTask(JobTaskRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectUserAction {
    ActionGuideVideoImport,
}

pub enum JobAuthoritySource {
    DirectUserAction(DirectUserAction),
    AgentTask {
        authority_snapshot: AuthoritySnapshot,
        task: JobTaskRef,
    },
}

pub struct JobAdmission {
    kind: JobKind,
    execution_class: JobExecutionClass,
    owner: JobOwner,
    authority: JobAuthoritySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobState {
    Starting,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobAdmissionError {
    #[error("job kind and authority source do not match")]
    KindAuthorityMismatch,
    #[error("job owner and authority source do not match")]
    OwnerAuthorityMismatch,
    #[error("job authority source is unsupported")]
    UnsupportedAuthoritySource,
    #[error("job registry is shutting down")]
    ShuttingDown,
    #[error("active job limit reached: {limit}")]
    ActiveLimit { limit: usize },
    #[error("terminal job capacity reached: {limit}")]
    TerminalCapacity { limit: usize },
    #[error("active and uncollected result slots reached: {limit}")]
    ResultCapacity { limit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobTransitionError {
    #[error("job not found")]
    NotFound,
    #[error("job reporter is stale")]
    StaleReporter,
    #[error("invalid transition from {from:?} via {operation}")]
    InvalidTransition {
        from: JobState,
        operation: &'static str,
    },
    #[error("conflicting terminal report")]
    TerminalConflict,
}
```

`JobAdmission::action_guide_video_import(operation_nonce)` is the only accepted constructor. `JobAdmission::agent_task` preserves the real snapshot and task reference for fail-closed validation tests. Keep `for_test` under `#[cfg(test)]`.

`JobControl` stores `Arc<dyn Fn() + Send + Sync>` and has a manual `Debug` that prints only `JobControl(<redacted>)`. `JobId::new()` uses `format!("job-{}", uuid::Uuid::new_v4())`; `JobId::parse` validates the prefix and UUID with `uuid::Uuid::parse_str`.

Implement `LiveJobRegistry<P, R>` as a non-`Clone` owner around `Arc<Inner<P, R>>`. `Inner` contains one `Mutex<RegistryState<P, R>>`, a Tokio watch sender, and a random registry subscription key. `JobReporter<P, R>` holds the same `Arc`, its `JobId`, and a `terminal_reported` flag. Do not put user callbacks or result `R` into `JobSnapshot<P>`.

`admit(admission, control, now_ms)` must validate in this order: registry open,
authority/owner/kind match, prune eligible terminal entries, terminal capacity,
active capacity, reserved result-slot capacity, then allocate/insert. Count
every active Job as one reserved result slot plus every uncollected successful
result. Invoke no callback while holding the mutex.

- [ ] **Step 6: Run focused and full agent tests**

Run:

```bash
rtk cargo test -p rollshot-agent jobs::tests --no-fail-fast
rtk cargo test -p rollshot-agent
rtk cargo fmt --check
```

Expected: all pass. Existing `rollshot-agent` tests remain green.

- [ ] **Step 7: Commit identity and admission**

```bash
rtk git add crates/rollshot-agent/Cargo.toml crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/jobs.rs
rtk git commit -m "feat(agent): add live job admission contract"
```

---

### Task 3: Cancellation, Terminal Truth, Observation, and Retention

**Files:**
- Modify: `crates/rollshot-agent/src/jobs.rs`

**Interfaces:**
- Consumes: Task 2 `LiveJobRegistry<P, R>`, `JobReporter<P, R>`, typed metadata, active-state transitions, limits, mutex, and watch sender.
- Produces:
  - `JobReporter::report_progress`, `append_diagnostic`, `succeed`, `fail`, and `cancelled`;
  - `LiveJobRegistry::cancel`, `collect`, `list`, `observer`, `watch`, `prune`, and `shutdown`;
  - read-only `JobObserver<P, R>`, `JobWatch`, `JobCancelOutcome`,
    `JobCollectError`, bounded `JobDiagnostic`, terminal metadata, and
    last-reporter abandonment behavior.

- [ ] **Step 1: Write failing cancellation-honesty tests**

Add:

```rust
#[test]
fn cancel_requests_control_but_worker_confirms_terminal() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let registry = LiveJobRegistry::<u32, String>::new();
    let (id, mut reporter) = registry
        .admit(
            direct_admission(7),
            JobControl::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
            }),
            100,
        )
        .unwrap();
    reporter.mark_running(101).unwrap();

    assert_eq!(registry.cancel(&id, 102), JobCancelOutcome::Requested);
    assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Cancelling);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        registry.cancel(&id, 103),
        JobCancelOutcome::AlreadyRequested
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    reporter.cancelled(104).unwrap();
    assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Cancelled);
    assert_eq!(
        registry.cancel(&id, 105),
        JobCancelOutcome::AlreadyTerminal
    );
}

#[test]
fn success_racing_with_cancel_is_dropped_and_becomes_cancelled() {
    let dropped = Arc::new(AtomicBool::new(false));
    let registry = LiveJobRegistry::<u32, DropProbe>::new();
    let (id, mut reporter) = registry.admit(direct_admission(7), no_op_control(), 100).unwrap();
    reporter.mark_running(101).unwrap();
    assert_eq!(registry.cancel(&id, 102), JobCancelOutcome::Requested);

    reporter.succeed(DropProbe(dropped.clone()), 103).unwrap();

    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Cancelled);
    assert_eq!(
        registry.collect(&id, 104).unwrap_err(),
        JobCollectError::NotSucceeded
    );
}
```

`DropProbe` sets its atomic flag in `Drop`; it proves the cancelled race retains no output.

- [ ] **Step 2: Write failing progress, notification, and diagnostics tests**

```rust
#[test]
fn latest_progress_and_terminal_repair_coalesced_notifications() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let mut watch = registry.watch().receiver();
    let (id, mut reporter) = registry.admit(direct_admission(7), no_op_control(), 100).unwrap();
    reporter.mark_running(101).unwrap();
    reporter.report_progress(10, 102).unwrap();
    reporter.report_progress(20, 103).unwrap();
    reporter.succeed("seed".to_string(), 104).unwrap();

    assert!(watch.has_changed().unwrap());
    let snapshot = registry.snapshot(&id).unwrap();
    assert_eq!(snapshot.state(), JobState::Succeeded);
    assert_eq!(snapshot.progress(), Some(&20));
    assert_eq!(snapshot.revision(), 5);
}

#[test]
fn diagnostics_keep_last_64_sanitized_entries_and_count_drops() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let (id, mut reporter) = registry.admit(direct_admission(7), no_op_control(), 100).unwrap();
    reporter.mark_running(101).unwrap();
    let entry = JobDiagnostic::new(
        JobDiagnosticCategory::Worker,
        "worker lifecycle observation",
    )
    .unwrap();
    for _ in 0..65 {
        reporter.append_diagnostic(entry.clone(), 102).unwrap();
    }

    let snapshot = registry.snapshot(&id).unwrap();
    assert_eq!(snapshot.diagnostics().len(), 64);
    assert_eq!(snapshot.dropped_diagnostics(), 1);
    const TOO_LONG: &str = concat!(
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "x"
    );
    assert_eq!(TOO_LONG.len(), 257);
    assert!(matches!(
        JobDiagnostic::new(JobDiagnosticCategory::Worker, TOO_LONG),
        Err(JobDiagnosticError::TooLong { limit: 256 })
    ));
}
```

Also assert `format!("{snapshot:?}")` omits result content, callback markers, paths, PIDs, and raw log sentinel values.

- [ ] **Step 3: Write failing collect-once, expiry, capacity, abandonment, and shutdown tests**

```rust
#[test]
fn success_result_moves_once_without_clone() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let (id, mut reporter) = registry.admit(direct_admission(7), no_op_control(), 100).unwrap();
    reporter.mark_running(101).unwrap();
    reporter.succeed("seed".to_string(), 102).unwrap();

    assert_eq!(registry.collect(&id, 103).unwrap(), "seed");
    assert_eq!(
        registry.collect(&id, 104).unwrap_err(),
        JobCollectError::AlreadyCollected
    );
    assert!(registry.snapshot(&id).unwrap().result_collected());
}

#[test]
fn uncollected_result_expires_at_five_minutes() {
    let registry = LiveJobRegistry::<u32, DropProbe>::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let (id, mut reporter) = registry.admit(direct_admission(7), no_op_control(), 0).unwrap();
    reporter.mark_running(1).unwrap();
    reporter.succeed(DropProbe(dropped.clone()), 2).unwrap();

    registry.prune(2 + TERMINAL_TTL_MS);

    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(
        registry.collect(&id, 2 + TERMINAL_TTL_MS).unwrap_err(),
        JobCollectError::ResultExpired
    );
}

#[test]
fn dropping_unfinished_reporter_marks_worker_abandoned() {
    let registry = LiveJobRegistry::<u32, String>::new();
    let (id, mut reporter) = registry.admit(direct_admission(7), no_op_control(), 100).unwrap();
    reporter.mark_running(101).unwrap();
    drop(reporter);

    let snapshot = registry.snapshot(&id).unwrap();
    assert_eq!(snapshot.state(), JobState::Failed);
    assert_eq!(
        snapshot.failure_category(),
        Some(JobFailureCategory::WorkerAbandoned)
    );
}

#[test]
fn shutdown_rejects_admission_and_requests_all_active_cancellation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = LiveJobRegistry::<u32, String>::new();
    let mut reporters = Vec::new();
    for nonce in 0..4 {
        let seen = calls.clone();
        reporters.push(
            registry
                .admit(
                    direct_admission(nonce),
                    JobControl::new(move || {
                        seen.fetch_add(1, Ordering::SeqCst);
                    }),
                    nonce,
                )
                .unwrap()
                .1,
        );
    }

    let requested = registry.shutdown(10);
    assert_eq!(requested.len(), 4);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        registry
            .admit(direct_admission(9), no_op_control(), 11)
            .unwrap_err(),
        JobAdmissionError::ShuttingDown
    );
    drop(reporters);
}
```

Add one cap test that creates and collects 128 terminal Jobs sequentially,
proves the oldest collected record is pruned on the next admission, and proves
an active Job is never evicted. Add a result-slot test proving
`active_count + uncollected_success_count == 4` rejects admission with
`ResultCapacity { limit: 4 }`; after collect or TTL prune frees a slot, the same
admission succeeds. Add one terminal-cap test proving 128 retained,
uncollected, unexpired terminal records are never silently evicted.

- [ ] **Step 4: Run tests and verify failures**

Run:

```bash
rtk cargo test -p rollshot-agent jobs::tests --no-fail-fast
```

Expected: the new tests fail because terminal, observation, retention, and shutdown methods are absent.

- [ ] **Step 5: Implement the complete state machine**

Use these additional public contracts:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobCancelOutcome {
    Requested,
    AlreadyRequested,
    AlreadyTerminal,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobFailureCategory {
    ProbeFailed,
    MissingVideoStream,
    InvalidVideoMetadata,
    DecoderUnavailable,
    DecodeFailed,
    EvidenceMissing,
    ScratchIo,
    ResourceLimit,
    WorkerAbandoned,
    WorkerPanic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobDiagnosticCategory {
    Lifecycle,
    Worker,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDiagnostic {
    category: JobDiagnosticCategory,
    message: &'static str,
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobDiagnosticError {
    #[error("job diagnostic message must not be empty")]
    Empty,
    #[error("job diagnostic exceeds {limit} bytes")]
    TooLong { limit: usize },
}



#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobCollectError {
    #[error("job not found")]
    NotFound,
    #[error("job did not succeed")]
    NotSucceeded,
    #[error("job result was already collected")]
    AlreadyCollected,
    #[error("job result expired")]
    ResultExpired,
}

#[derive(Clone)]
pub struct JobObserver<P, R> {
    inner: Arc<Inner<P, R>>,
}

#[derive(Clone)]
pub struct JobWatch {
    registry_key: u64,
    receiver: tokio::sync::watch::Receiver<u64>,
}
```

Implement `JobObserver::snapshot`, `list`, and `watch` as read-only forwards
over the shared inner state; it cannot admit, cancel, collect, prune, or
shutdown. Implement `Hash` for `JobWatch` using only `registry_key`; implement
manual `Debug` for both observer/watch without inner state or receiver details.
`receiver()` returns a cloned receiver. The watch payload is a global monotonic
revision; `send_replace` after releasing the registry mutex so notification
delivery never holds the state lock.

Each record stores `Option<P>` latest progress,
`VecDeque<JobDiagnostic>`, dropped diagnostic count, `Option<R>` result,
collected/expired markers, timestamps, revision, failure category, and
`JobControl`. `JobSnapshot<P>` clones only `P` and bounded metadata. Require
`P: Clone` only on observation methods; never require `R: Clone` or `Debug`.

Reporter terminal methods update terminal state before setting
`terminal_reported = true`. `Drop` maps `Starting|Running` to
`Failed(WorkerAbandoned)` and invokes control outside the lock; it maps
`Cancelling` to `Cancelled` because reporter-stack destruction follows concrete
resource destruction. A reporter dropped after any terminal is a no-op. An
identical repeated failure/cancel report is state-idempotent; a conflicting
terminal returns `JobTransitionError::TerminalConflict`. A repeated success
drops the newly supplied `R` and returns the same conflict without replacing
the original result.

`cancel` changes `Starting|Running` to `Cancelling`, saves the request timestamp, clones the control, unlocks, invokes once, and publishes a watch revision. It never changes `Cancelling` to `Cancelled`.

`prune(now_ms)` preserves active records; removes expired terminal
results/records; remembers expired IDs in a bounded 128-entry tombstone set so
`collect` distinguishes `ResultExpired` from `NotFound`; and prunes oldest
collected terminals before uncollected successes. `admit` and `collect` invoke
the same pruning logic with their supplied `now_ms`; tests and the app may call
`prune` explicitly. `snapshot` and `list` are read-only. No timer task is
created.

`Drop for LiveJobRegistry` calls the same idempotent shutdown transition. Reporter/observer Arcs may outlive the owner, but shutdown state and cancellation requests remain visible to them.

Emit privacy-safe structured events for admission rejection, admitted,
running, cancellation requested, terminal, result collected/expired, worker
abandoned, and shutdown under target `rollshot::agent::jobs`. Fields are
limited to Job ID, kind, state, revision, failure category, counts, and numeric
timestamps/progress; never format `P`, `R`, `JobControl`, `JobAdmission`, or
diagnostic message text into tracing.

- [ ] **Step 6: Run focused concurrency and privacy checks**

Run:

```bash
rtk cargo test -p rollshot-agent jobs::tests --no-fail-fast
rtk cargo test -p rollshot-agent jobs::tests::shutdown_rejects_admission_and_requests_all_active_cancellation -- --exact
rtk cargo test -p rollshot-agent jobs::tests::latest_progress_and_terminal_repair_coalesced_notifications -- --exact
rtk cargo test -p rollshot-agent
rtk cargo fmt --check
```

Expected: all pass without sleeps except pre-existing process fixtures outside this module.

- [ ] **Step 7: Commit complete registry lifecycle**

```bash
rtk git add crates/rollshot-agent/src/jobs.rs
rtk git commit -m "feat(agent): complete live job lifecycle"
```

---

### Task 4: Atomically Migrate Action Guide Video Import

**Files:**
- Modify: `crates/rollshot-app/src/action_guide_home/video_import.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/update.rs`
- Modify: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`

**Interfaces:**
- Consumes: Task 3 `LiveJobRegistry<VideoImportProgress, ImportedWorkspaceSeed>`, `JobReporter`, `JobWatch`, snapshots, cancellation/collection outcomes, and failure categories; existing `ImportOperationId`, `VideoImportCancellation`, `import_video`, shared effect handling, and iced 0.14 `Subscription::run_with`.
- Produces:
  - `type VideoImportJobRegistry = LiveJobRegistry<VideoImportProgress, ImportedWorkspaceSeed>`;
  - registry-owned `ActionGuideHome::import_jobs` and stable job-watch subscription;
  - `ImportCoordinator` binding of pre-job operation to current `JobId`;
  - registry-backed start, progress projection, cancellation, terminal repair, and collect-once timeline opening on both platforms;
  - removal of `Message::ImportProgress`, payload-bearing `Message::ImportFinished`, and coordinator-owned worker cancellation.

- [ ] **Step 1: Rewrite coordinator tests first for Job binding and detachment**

Replace the Task 1 cancellation test and add these compile-failing tests in `video_import.rs`:

```rust
#[test]
fn bind_job_keeps_preparation_and_job_identity_distinct() {
    let mut coordinator = ImportCoordinator::default();
    let operation = coordinator.begin(PathBuf::from("test.mp4"));
    let job = JobId::parse("job-00000000-0000-4000-8000-000000000003").unwrap();

    coordinator.bind_job(operation, job.clone()).unwrap();

    assert_eq!(coordinator.operation_id(), Some(operation));
    assert_eq!(coordinator.job_id(), Some(&job));
    assert_eq!(coordinator.state(), ImportState::Preflight);
}

#[test]
fn stale_job_snapshot_cannot_replace_current_progress() {
    let mut coordinator = ImportCoordinator::default();
    let old_operation = coordinator.begin(PathBuf::from("old.mp4"));
    let old_job = JobId::parse("job-00000000-0000-4000-8000-000000000003").unwrap();
    coordinator.bind_job(old_operation, old_job.clone()).unwrap();
    coordinator.finish_idle();

    let new_operation = coordinator.begin(PathBuf::from("new.mp4"));
    let new_job = JobId::parse("job-00000000-0000-4000-8000-000000000004").unwrap();
    coordinator.bind_job(new_operation, new_job).unwrap();
    coordinator.project_progress(
        &old_job,
        progress(VideoImportPass::Extract),
    );

    assert_eq!(coordinator.operation_id(), Some(new_operation));
    assert_ne!(coordinator.state(), ImportState::ExtractingPass2);
}
```

Remove `cancellation: Option<VideoImportCancellation>` from `ImportCoordinator`; add `job_id: Option<JobId>`. `bind_job` checks the current operation and rejects mismatches with a small app-local `ImportBindingError`. `detach`/`finish_idle` clears path, operation, job, and progress.

- [ ] **Step 2: Write failing registry-backed home update tests**

Change `setup_home` to construct `ActionGuideHome::new(recent)` with its internal registry. Add deterministic helper `admit_import_for_test` that uses `ImportOperationId::get()` and returns the exact `JobId`/reporter from the home registry.

Add tests:

```rust
#[test]
fn available_toolchain_admits_once_before_worker_effect() {
    let (_dir, mut home) = setup_home();
    let operation = home
        .import_coordinator_mut()
        .begin(PathBuf::from("test.mp4"));
    let update = home.update(Message::ImportToolchainResolved {
        operation_id: operation,
        resolution: VideoImportToolchainResolution::Available(toolchain_fixture()),
    });

    let Effect::StartImport { job_id, .. } = update.effect else {
        panic!("expected registry-backed StartImport");
    };
    assert_eq!(home.import_coordinator().job_id(), Some(&job_id));
    assert_eq!(home.import_jobs().list().len(), 1);
    assert_eq!(
        home.import_jobs().snapshot(&job_id).unwrap().state(),
        JobState::Starting
    );
}

#[test]
fn registry_admission_failure_starts_no_worker() {
    let (_dir, mut home) = setup_home();
    let mut held_reporters = Vec::new();
    for nonce in 100..104 {
        held_reporters.push(home.admit_test_import(nonce).unwrap().1);
    }
    let operation = home
        .import_coordinator_mut()
        .begin(PathBuf::from("test.mp4"));
    let update = home.update(Message::ImportToolchainResolved {
        operation_id: operation,
        resolution: VideoImportToolchainResolution::Available(toolchain_fixture()),
    });

    assert!(matches!(update.effect, Effect::None));
    assert!(home.message.as_deref().unwrap().contains("Too many imports"));
    assert!(home.import_coordinator().job_id().is_none());
    drop(held_reporters);
}

#[test]
fn terminal_snapshot_opens_seed_once_even_after_notification_coalescing() {
    let (_dir, mut home) = setup_home();
    let (job_id, mut reporter) = home.bind_test_import();
    reporter.mark_running(10).unwrap();
    reporter.report_progress(progress(VideoImportPass::Analyze), 11).unwrap();
    reporter.succeed(dummy_seed(&tempfile::tempdir().unwrap()), 12).unwrap();

    let first = home.update(Message::ImportJobsChanged);
    assert!(matches!(first.effect, Effect::OpenImportedTimeline(_)));
    let second = home.update(Message::ImportJobsChanged);
    assert!(matches!(second.effect, Effect::None));
    assert!(matches!(
        home.import_jobs().collect(&job_id, 13),
        Err(JobCollectError::AlreadyCollected)
    ));
}

#[test]
fn cancel_detaches_ui_but_registry_waits_for_worker_confirmation() {
    let (_dir, mut home) = setup_home();
    let (job_id, mut reporter, observed_cancel) = home.bind_test_import_with_cancel_probe();
    reporter.mark_running(10).unwrap();

    home.update(Message::CancelImport);

    assert!(observed_cancel.load(Ordering::SeqCst));
    assert_eq!(home.import_coordinator().state(), ImportState::Idle);
    assert_eq!(
        home.import_jobs().snapshot(&job_id).unwrap().state(),
        JobState::Cancelling
    );
    reporter.cancelled(11).unwrap();
    assert_eq!(
        home.import_jobs().snapshot(&job_id).unwrap().state(),
        JobState::Cancelled
    );
}
```

Add this exact mapping-table test; `Cancelled` is a terminal path, not a
failure category:

```rust
#[test]
fn video_import_errors_map_to_stable_categories_and_existing_copy() {
    use rollshot_action::VideoImportError as Error;
    use rollshot_agent::jobs::JobFailureCategory as Category;

    let cases = [
        (Error::ProbeFailed, Some(Category::ProbeFailed), "Import failed: Video metadata could not be read."),
        (Error::MissingVideoStream, Some(Category::MissingVideoStream), "Import failed: The selected file has no readable video stream."),
        (Error::InvalidVideoMetadata, Some(Category::InvalidVideoMetadata), "Import failed: The selected video has invalid dimensions or duration."),
        (Error::DecoderUnavailable, Some(Category::DecoderUnavailable), "Import failed: The video decoder is unavailable."),
        (Error::DecodeFailed, Some(Category::DecodeFailed), "Import failed: The video could not be decoded."),
        (Error::EvidenceMissing, Some(Category::EvidenceMissing), "Import failed: Required evidence could not be extracted."),
        (Error::ScratchIo, Some(Category::ScratchIo), "Import failed: Temporary evidence storage failed."),
        (Error::ResourceLimit, Some(Category::ResourceLimit), "Import failed: The recording exceeds an internal resource bound."),
        (Error::Cancelled, None, "Import was cancelled."),
    ];

    for (error, category, message) in cases {
        assert_eq!(video_import_failure_category(&error), category);
        assert_eq!(video_import_error_message(&error), message);
    }
    assert_eq!(
        job_failure_message(Category::WorkerAbandoned),
        "Import worker stopped unexpectedly."
    );
    assert_eq!(
        job_failure_message(Category::WorkerPanic),
        "Import worker stopped unexpectedly."
    );
}
```

- [ ] **Step 3: Run app tests and verify compile failures**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home::video_import::tests --no-fail-fast
rtk cargo test -p rollshot-app --features action-guide action_guide_home::update::tests --no-fail-fast
```

Expected: FAIL because the coordinator Job binding, home registry, reporter effects, and `ImportJobsChanged` message do not exist.

- [ ] **Step 4: Implement the app type alias and coordinator projection**

At the top of `video_import.rs`, add:

```rust
use rollshot_agent::jobs::{JobId, LiveJobRegistry};

pub type VideoImportJobRegistry =
    LiveJobRegistry<rollshot_action::VideoImportProgress, rollshot_action::ImportedWorkspaceSeed>;
```

Add `ImportOperationId::get(self) -> u64`. Replace coordinator cancellation storage with `job_id`. Keep `begin`, `mark_setting_up`, and operation-based stale checks for pre-job effects. Implement:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportBindingError {
    StaleOperation,
}

pub fn bind_job(
    &mut self,
    operation_id: ImportOperationId,
    job_id: JobId,
) -> Result<(), ImportBindingError> {
    if self.operation_id != Some(operation_id) {
        return Err(ImportBindingError::StaleOperation);
    }
    self.job_id = Some(job_id);
    self.state = ImportState::Preflight;
    Ok(())
}

pub fn job_id(&self) -> Option<&JobId> {
    self.job_id.as_ref()
}

pub fn project_progress(&mut self, job_id: &JobId, progress: VideoImportProgress) {
    if self.job_id.as_ref() != Some(job_id) {
        return;
    }
    self.last_progress = Some(progress);
    self.state = match progress.pass {
        VideoImportPass::Preflight => ImportState::Preflight,
        VideoImportPass::Analyze => ImportState::AnalyzingPass1,
        VideoImportPass::Extract => ImportState::ExtractingPass2,
    };
}

pub fn detach(&mut self) {
    self.finish_idle();
}
```

`project_progress` checks `self.job_id.as_ref() == Some(job_id)` before changing state. `finish_idle` and `detach` clear the full presentation record. Preserve path privacy tests after both completion and cancellation.

- [ ] **Step 5: Replace payload-bearing messages with snapshot reconciliation**

In `update.rs`:

1. Remove `Arc<Mutex<_>>` imports used only by `ImportFinished`.
2. Remove `Message::ImportProgress` and `Message::ImportFinished`.
3. Add `Message::ImportJobsChanged`.
4. Change `Effect::StartImport` to contain `job_id`, path, toolchain,
   `VideoImportCancellation`, and `JobReporter<VideoImportProgress,
   ImportedWorkspaceSeed>`.
5. Add `import_jobs: VideoImportJobRegistry` to `ActionGuideHome` and construct it
   in `new`.
6. Add `import_jobs(&self)`, `import_job_watch(&self)`, and private
   `reconcile_import_job()`.

Admission in `ImportToolchainResolved::Available` must be exactly:

```rust
let cancellation = rollshot_action::VideoImportCancellation::default();
let control = cancellation.clone();
let admission = rollshot_agent::jobs::JobAdmission::action_guide_video_import(
    operation_id.get(),
);
let admitted = self.import_jobs.admit(
    admission,
    rollshot_agent::jobs::JobControl::new(move || control.cancel()),
    now_unix_ms(),
);
let (job_id, reporter) = match admitted {
    Ok(admitted) => admitted,
    Err(error) => {
        self.import.finish_idle();
        self.message = Some(import_admission_message(error));
        return Update::none();
    }
};
self.import
    .bind_job(operation_id, job_id.clone())
    .expect("fresh admission binds to current operation");
```

Add these app-local helpers; they are the only category-to-copy mapping:

```rust
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn video_import_failure_category(
    error: &rollshot_action::VideoImportError,
) -> Option<rollshot_agent::jobs::JobFailureCategory> {
    use rollshot_action::VideoImportError as Error;
    use rollshot_agent::jobs::JobFailureCategory as Category;
    match error {
        Error::ProbeFailed => Some(Category::ProbeFailed),
        Error::MissingVideoStream => Some(Category::MissingVideoStream),
        Error::InvalidVideoMetadata => Some(Category::InvalidVideoMetadata),
        Error::DecoderUnavailable => Some(Category::DecoderUnavailable),
        Error::DecodeFailed => Some(Category::DecodeFailed),
        Error::EvidenceMissing => Some(Category::EvidenceMissing),
        Error::ScratchIo => Some(Category::ScratchIo),
        Error::ResourceLimit => Some(Category::ResourceLimit),
        Error::Cancelled => None,
    }
}

fn video_import_error_message(error: &rollshot_action::VideoImportError) -> String {
    match error {
        rollshot_action::VideoImportError::Cancelled => error.to_string(),
        _ => format!("Import failed: {error}"),
    }
}

fn job_failure_message(
    category: rollshot_agent::jobs::JobFailureCategory,
) -> &'static str {
    use rollshot_agent::jobs::JobFailureCategory as Category;
    match category {
        Category::ProbeFailed => "Import failed: Video metadata could not be read.",
        Category::MissingVideoStream => {
            "Import failed: The selected file has no readable video stream."
        }
        Category::InvalidVideoMetadata => {
            "Import failed: The selected video has invalid dimensions or duration."
        }
        Category::DecoderUnavailable => "Import failed: The video decoder is unavailable.",
        Category::DecodeFailed => "Import failed: The video could not be decoded.",
        Category::EvidenceMissing => {
            "Import failed: Required evidence could not be extracted."
        }
        Category::ScratchIo => "Import failed: Temporary evidence storage failed.",
        Category::ResourceLimit => {
            "Import failed: The recording exceeds an internal resource bound."
        }
        Category::WorkerAbandoned | Category::WorkerPanic => {
            "Import worker stopped unexpectedly."
        }
    }
}

fn import_admission_message(
    error: rollshot_agent::jobs::JobAdmissionError,
) -> String {
    match error {
        rollshot_agent::jobs::JobAdmissionError::ActiveLimit { .. }
        | rollshot_agent::jobs::JobAdmissionError::ResultCapacity { .. }
        | rollshot_agent::jobs::JobAdmissionError::TerminalCapacity { .. } => {
            "Too many imports are still active or awaiting cleanup.".to_string()
        }
        _ => "Import could not start because authorization was rejected.".to_string(),
    }
}
```

`reconcile_import_job` clones the current `JobId`, queries one latest snapshot,
projects progress for `Starting|Running|Cancelling`, and handles terminals:

- `Succeeded`: call `collect` once, then `finish_idle`, then return
  `OpenImportedTimeline(seed)`;
- `Failed`: map the stable failure category, `finish_idle`, and return no effect;
- `Cancelled`: `finish_idle` and return no effect.

`CancelImport` clones the bound `JobId`, calls registry `cancel`, then detaches presentation. Pre-job cancellation still clears coordinator state without a registry call. Do not show a cancellation error.

- [ ] **Step 6: Make the worker report to the registry before notifying iced**

Replace `run_import_task` with a `Task::perform` whose `spawn_blocking` closure owns the reporter:

```rust
pub(crate) fn run_import_task(
    path: PathBuf,
    toolchain: rollshot_action::VideoToolchain,
    cancellation: rollshot_action::VideoImportCancellation,
    mut reporter: rollshot_agent::jobs::JobReporter<
        rollshot_action::VideoImportProgress,
        rollshot_action::ImportedWorkspaceSeed,
    >,
) -> Task<Message> {
    Task::perform(
        async move {
            let worker = tokio::task::spawn_blocking(move || {
                if reporter.mark_running(now_unix_ms()).is_err() {
                    return;
                }
                let request = rollshot_action::VideoImportRequest {
                    input: path,
                    toolchain,
                    scratch_parent: std::env::temp_dir().join("rollshot/import"),
                };
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rollshot_action::import_video(request, cancellation, |progress| {
                        let _ = reporter.report_progress(progress, now_unix_ms());
                    })
                }));
                match outcome {
                    Ok(Ok(seed)) => {
                        let _ = reporter.succeed(seed, now_unix_ms());
                    }
                    Ok(Err(rollshot_action::VideoImportError::Cancelled)) => {
                        let _ = reporter.cancelled(now_unix_ms());
                    }
                    Ok(Err(error)) => {
                        let category = video_import_failure_category(&error)
                            .expect("cancelled handled by the previous arm");
                        let _ = reporter.fail(category, now_unix_ms());
                    }
                    Err(_) => {
                        let _ = reporter.fail(
                            rollshot_agent::jobs::JobFailureCategory::WorkerPanic,
                            now_unix_ms(),
                        );
                    }
                }
            })
            .await;

            if worker.is_err() {
                tracing::event!(
                    target: "rollshot::app::action_guide::video_import",
                    tracing::Level::WARN,
                    category = "worker_join_failed",
                );
            }
            Message::ImportJobsChanged
        },
        std::convert::identity,
    )
}
```

`catch_unwind` converts a worker-body panic to `WorkerPanic` before reporter
drop. Reporter `Drop` remains the authoritative `WorkerAbandoned` fallback for
task abortion or any panic outside that guarded body. Do not put error strings
or paths in tracing.

- [ ] **Step 7: Add the stable iced 0.14 watch subscription**

Change the shared subscription to accept `&ActionGuideHome` and batch window events with a stable registry stream:

```rust
fn import_job_changes(
    watch: &rollshot_agent::jobs::JobWatch,
) -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;

    let mut receiver = watch.receiver();
    iced::stream::channel(1, async move |mut output| loop {
        if receiver.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
        if output.send(Message::ImportJobsChanged).await.is_err() {
            return;
        }
    })
}

pub fn subscription(state: &ActionGuideHome) -> iced::Subscription<Message> {
    let jobs = iced::Subscription::run_with(
        state.import_job_watch(),
        import_job_changes,
    );
    iced::Subscription::batch([
        iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Window(iced::window::Event::Focused) => {
                Some(Message::WindowFocused)
            }
            _ => None,
        }),
        jobs,
    ])
}
```

`JobWatch` hashes only the stable registry key. The stream carries no result or progress payload; every emission triggers snapshot reconciliation. The stream remains pending if the sender closes, satisfying iced 0.14’s non-ending subscription contract until the phase removes it.

- [ ] **Step 8: Cut over both platform effect and subscription callsites**

Linux `Effect::StartImport` passes `path`, `toolchain`, `cancellation`, and
`reporter` to the shared `run_import_task`; `job_id` is used only for privacy-safe
tracing/debug and coordinator correlation. Linux subscription becomes:

```rust
crate::action_guide_home::update::subscription(&state.home).map(Message::Home)
```

macOS performs the same worker call. Bind the home across all three relevant
phases:

```rust
#[cfg(feature = "action-guide")]
Phase::Home(home) | Phase::Opening(home) | Phase::LockConflict(home) => {
    action_guide_home::update::subscription(home).map(Message::HomeMsg)
}
```

Do not add a second platform subscription or worker implementation.

- [ ] **Step 9: Remove obsolete message plumbing and update privacy Debug impls**

Delete:

- coordinator `cancellation` field/accessor/setter;
- `Message::ImportProgress`;
- payload-bearing `Message::ImportFinished`;
- the mpsc progress channel and `Arc<Mutex<Option<ImportedWorkspaceSeed>>>`;
- operation-ID terminal handling that the registry replaces.

Keep `Effect::Debug` limited to operation and Job IDs and `finish_non_exhaustive`; never format path, reporter, cancellation callback, or seed. Add a test with sentinel path/result values proving `ActionGuideHome`, `Effect`, registry snapshot, and Job watch `Debug` omit them.

- [ ] **Step 10: Run the atomic migration matrix**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_home::video_import::tests --no-fail-fast
rtk cargo test -p rollshot-app --features action-guide action_guide_home::update::tests --no-fail-fast
rtk cargo test -p rollshot-action video_import --no-fail-fast
rtk cargo test -p rollshot-agent jobs::tests --no-fail-fast
rtk cargo test -p rollshot-app --features action-guide action_guide_linux_product --no-fail-fast
rtk cargo test -p rollshot-app --features action-guide macos_product --no-fail-fast
rtk cargo fmt --check
```

Expected: all pass. No old `ImportProgress`/`ImportFinished` references remain. Linux and macOS compile against the same shared worker/subscription signatures.

- [ ] **Step 11: Commit the product cutover**

```bash
rtk git add crates/rollshot-app/src/action_guide_home/video_import.rs crates/rollshot-app/src/action_guide_home/update.rs crates/rollshot-app/src/action_guide_linux_product.rs crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(action-guide): migrate import to live jobs"
```

---

### Task 5: Failure Injection, Privacy, and Shutdown Regression

**Files:**
- Modify: `crates/rollshot-agent/src/jobs.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/update.rs`
- Modify only if a real defect is exposed: `crates/rollshot-app/src/action_guide_home/video_import.rs`
- Modify only if a real defect is exposed: `crates/rollshot-action/src/video_import/process.rs`
- Modify only if a real defect is exposed: `crates/rollshot-action/src/video_import/scratch.rs`

**Interfaces:**
- Consumes: completed registry and Action Guide migration from Tasks 2–4.
- Produces: adversarial evidence for notification loss, cancellation races, worker panic/abandonment, result expiry/drop cleanup, owner shutdown, stale updates, and privacy; no new public feature.

- [ ] **Step 1: Add deterministic failure-injection helpers**

In `update.rs` tests, add a `seed_with_root` helper that returns both the
`ImportedWorkspaceSeed` and its scratch root:

```rust
fn seed_with_root(
    parent: &tempfile::TempDir,
) -> (rollshot_action::ImportedWorkspaceSeed, PathBuf) {
    let seed = dummy_seed(parent);
    let root = seed.scratch.root().to_path_buf();
    (seed, root)
}
```

Use the Task 4 `bind_test_import` and
`bind_test_import_with_cancel_probe` helpers to control reporters directly. No
real FFmpeg, thread sleep, or timing race is needed in these app state-machine
tests.

- [ ] **Step 2: Add race and notification-loss tests**

```rust
#[test]
fn notification_loss_does_not_lose_terminal_or_duplicate_collection() {
    let (_project_dir, mut home) = setup_home();
    let scratch_parent = tempfile::tempdir().unwrap();
    let (job_id, mut reporter) = home.bind_test_import();
    reporter.mark_running(10).unwrap();
    reporter
        .report_progress(progress(VideoImportPass::Analyze), 11)
        .unwrap();
    reporter
        .report_progress(progress(VideoImportPass::Extract), 12)
        .unwrap();
    reporter.succeed(dummy_seed(&scratch_parent), 13).unwrap();

    assert_eq!(
        home.import_jobs()
            .snapshot(&job_id)
            .unwrap()
            .progress()
            .unwrap()
            .pass,
        VideoImportPass::Extract
    );
    let first = home.update(Message::ImportJobsChanged);
    assert!(matches!(first.effect, Effect::OpenImportedTimeline(_)));
    let second = home.update(Message::ImportJobsChanged);
    assert!(matches!(second.effect, Effect::None));
    assert_eq!(
        home.import_jobs().collect(&job_id, 14).unwrap_err(),
        JobCollectError::AlreadyCollected
    );
}

#[test]
fn cancel_wins_against_late_success_and_drops_seed() {
    let (_project_dir, mut home) = setup_home();
    let scratch_parent = tempfile::tempdir().unwrap();
    let (seed, scratch_root) = seed_with_root(&scratch_parent);
    let (job_id, mut reporter, observed_cancel) =
        home.bind_test_import_with_cancel_probe();
    reporter.mark_running(10).unwrap();

    home.update(Message::CancelImport);
    reporter.succeed(seed, 11).unwrap();
    let update = home.update(Message::ImportJobsChanged);

    assert!(observed_cancel.load(Ordering::SeqCst));
    assert!(!scratch_root.exists());
    assert!(matches!(update.effect, Effect::None));
    assert_eq!(
        home.import_jobs().snapshot(&job_id).unwrap().state(),
        JobState::Cancelled
    );
}

#[test]
fn stale_terminal_from_old_job_cannot_open_over_new_import() {
    let (_project_dir, mut home) = setup_home();
    let (old_id, mut old_reporter) = home.bind_test_import();
    old_reporter.mark_running(10).unwrap();
    home.import_coordinator_mut().detach();
    let (new_id, mut new_reporter) = home.bind_test_import();
    new_reporter.mark_running(11).unwrap();

    old_reporter
        .fail(JobFailureCategory::DecodeFailed, 12)
        .unwrap();
    let update = home.update(Message::ImportJobsChanged);

    assert!(matches!(update.effect, Effect::None));
    assert_eq!(home.import_coordinator().job_id(), Some(&new_id));
    assert_eq!(
        home.import_jobs().snapshot(&old_id).unwrap().state(),
        JobState::Failed
    );
    assert_eq!(
        home.import_jobs().snapshot(&new_id).unwrap().state(),
        JobState::Running
    );
}

#[test]
fn reporter_drop_becomes_worker_abandoned_and_is_repairable() {
    let (_project_dir, mut home) = setup_home();
    let (job_id, mut reporter) = home.bind_test_import();
    reporter.mark_running(10).unwrap();
    drop(reporter);

    let update = home.update(Message::ImportJobsChanged);

    assert!(matches!(update.effect, Effect::None));
    assert_eq!(home.import_coordinator().state(), ImportState::Idle);
    assert_eq!(
        home.import_jobs().snapshot(&job_id).unwrap().failure_category(),
        Some(JobFailureCategory::WorkerAbandoned)
    );
    assert_eq!(
        home.message.as_deref(),
        Some("Import worker stopped unexpectedly.")
    );
}
```

- [ ] **Step 3: Add owner-drop and result-expiry cleanup tests**

Add these tests:

```rust
#[test]
fn owner_drop_requests_cancel_while_observer_and_reporter_finish_cleanup() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let registry = LiveJobRegistry::<u32, String>::new();
    let observer = registry.observer();
    let (id, mut reporter) = registry
        .admit(
            direct_admission(7),
            JobControl::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
            }),
            10,
        )
        .unwrap();
    reporter.mark_running(11).unwrap();

    drop(registry);

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(observer.snapshot(&id).unwrap().state(), JobState::Cancelling);
    reporter.cancelled(12).unwrap();
    assert_eq!(observer.snapshot(&id).unwrap().state(), JobState::Cancelled);
}

#[test]
fn expired_uncollected_seed_is_dropped_and_scratch_is_removed() {
    let (_project_dir, mut home) = setup_home();
    let scratch_parent = tempfile::tempdir().unwrap();
    let (seed, scratch_root) = seed_with_root(&scratch_parent);
    let (job_id, mut reporter) = home.bind_test_import();
    reporter.mark_running(10).unwrap();
    reporter.succeed(seed, 11).unwrap();

    home.import_jobs().prune(11 + TERMINAL_TTL_MS);

    assert!(!scratch_root.exists());
    assert_eq!(
        home.import_jobs()
            .collect(&job_id, 11 + TERMINAL_TTL_MS)
            .unwrap_err(),
        JobCollectError::ResultExpired
    );
    assert!(!format!("{:?}", home.import_jobs().watch()).contains(
        scratch_root.to_string_lossy().as_ref()
    ));
}
```

- [ ] **Step 4: Add privacy-safe formatting and tracing assertions**

In `jobs.rs` tests, define `capture_job_tracing<T>(run: impl FnOnce() -> T) ->
(T, String)` using a test-local `Arc<Mutex<Vec<u8>>>` writer and
`tracing::subscriber::with_default`; use the exact `WriteAdaptor`/subscriber
shape already present in `crates/rollshot-agent/src/driver.rs:4996-5030`.
Then add:

```rust
#[test]
fn job_debug_and_tracing_omit_control_and_result_sentinels() {
    let sentinels = [
        "/home/alice/SECRET-recording.mp4",
        "RAW-FFMPEG-SECRET",
        "api_key=SECRET",
        "SECRET-skill-body",
        "SECRET-seed-payload",
    ];
    let captured_by_control = sentinels.join("|");
    let result_payload = sentinels.join("|");
    let ((registry, id), logs) = capture_job_tracing(move || {
        let registry = LiveJobRegistry::<u32, String>::new();
        let control_secret = captured_by_control;
        let (id, mut reporter) = registry
            .admit(
                direct_admission(7),
                JobControl::new(move || {
                    std::hint::black_box(&control_secret);
                }),
                10,
            )
            .unwrap();
        reporter.mark_running(11).unwrap();
        reporter.report_progress(25, 12).unwrap();
        reporter.succeed(result_payload, 13).unwrap();
        (registry, id)
    });
    let rendered = format!(
        "{:?}{:?}{:?}",
        registry.snapshot(&id).unwrap(),
        registry.watch(),
        logs
    );

    for sentinel in sentinels {
        assert!(!rendered.contains(sentinel), "leaked sentinel: {sentinel}");
    }
    assert!(logs.contains("rollshot::agent::jobs"));
    assert!(logs.contains(id.as_str()));
    assert!(logs.contains("succeeded"));
}
```

The registry may trace only stable target, Job ID, kind, state, revision,
failure category, and numeric progress. Do not add a source-text scan as the
primary privacy test.

- [ ] **Step 5: Run focused adversarial tests**

Run:

```bash
rtk cargo test -p rollshot-agent jobs::tests --no-fail-fast
rtk cargo test -p rollshot-app --features action-guide notification_loss_does_not_lose_terminal_or_duplicate_collection -- --exact
rtk cargo test -p rollshot-app --features action-guide cancel_wins_against_late_success_and_drops_seed -- --exact
rtk cargo test -p rollshot-app --features action-guide stale_terminal_from_old_job_cannot_open_over_new_import -- --exact
rtk cargo test -p rollshot-app --features action-guide reporter_drop_becomes_worker_abandoned_and_is_repairable -- --exact
rtk cargo test -p rollshot-action video_import::process::tests::analysis_cancel_interrupts_a_stalled_decoder -- --exact
```

Expected: all pass; no test uses arbitrary sleep except the existing bounded
process polling fixture.

- [ ] **Step 6: Commit adversarial coverage and any source fix it proves necessary**

```bash
rtk git add crates/rollshot-agent/src/jobs.rs crates/rollshot-app/src/action_guide_home/update.rs crates/rollshot-app/src/action_guide_home/video_import.rs crates/rollshot-action/src/video_import/process.rs crates/rollshot-action/src/video_import/scratch.rs
rtk git commit -m "test(agent): cover live job failure boundaries"
```

Before staging, omit every listed file with no actual diff. A production edit is
allowed only when one of the new tests first demonstrated the defect.

---

### Task 6: Slice 4 Verification, Independent Review, and Gate Decision

**Files:**
- Modify only if verification exposes a Slice 4 defect: files changed in Tasks 1–5
- Create: `docs/superpowers/spikes/2026-07-27-live-job-registry-decision.md`

**Interfaces:**
- Consumes: completed Slice 4 implementation and all acceptance criteria in the governing spec.
- Produces: reproducible gate evidence, resolved independent review findings, migration/residual-risk record, and a Gate decision proposal. It does not begin Slice 5 or Slice 6.

- [ ] **Step 1: Run focused lifecycle, process, and app suites**

Run:

```bash
rtk cargo test -p rollshot-agent jobs --no-fail-fast
rtk cargo test -p rollshot-action video_import --no-fail-fast
rtk cargo test -p rollshot-app --features action-guide action_guide_home --no-fail-fast
```

Record passed/failed/ignored counts and durations in the decision record.

- [ ] **Step 2: Run affected crate regression suites**

Run:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide
```

Record exact counts. Any failure blocks the gate until fixed and rerun.

- [ ] **Step 3: Run the real video-import smoke fixture when available**

Run the existing fixture test with environment
`ROLLSHOT_TEST_FFMPEG=1`:

```bash
rtk proxy env ROLLSHOT_TEST_FFMPEG=1 cargo test -p rollshot-action video_import::tests::static_video_returns_final_frame_fallback -- --exact
```

Expected when FFmpeg/FFprobe are installed: PASS through probe, analysis,
extraction, scratch, and final `ImportedWorkspaceSeed`. If the tools are
unavailable, record the explicit skip in the Gate decision; do not report this
smoke as passing.

- [ ] **Step 4: Run formatting, lint, and privacy checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk proxy git diff --check
```

Use the repository Grep tool on
`crates/rollshot-agent/src;crates/rollshot-app/src;crates/rollshot-action/src`
with pattern `println!|eprintln!|dbg!`, then inspect every hit in changed
production paths. Expected: formatting and lint pass; no new production
`println!`, `eprintln!`, or `dbg!`; runtime privacy tests pass.

- [ ] **Step 5: Request independent code review**

Invoke `requesting-code-review` against the full Slice 4 implementation range and the governing spec. Require explicit findings for:

1. Can work launch before direct-user admission commits a `Starting` record?
2. Can any skill/model/agent task construct accepted authority or borrow an unrelated `RunOperation`?
3. Can cancellation be reported confirmed before FFmpeg children and scratch are cleaned?
4. Can notification loss, duplication, or reordering lose terminal truth or collect a result twice?
5. Can a stale Job affect a newer `ImportOperationId` or open a timeline?
6. Can reporter panic/drop leave a Job falsely Running?
7. Can active work be evicted, or an unexpired result be silently dropped at capacity?
8. Can shutdown callbacks deadlock by running under the registry mutex?
9. Can `Debug`, tracing, diagnostics, snapshots, or watch payloads leak paths, media, raw process output, callbacks, PIDs, credentials, skill bodies, or seed content?
10. Do Linux and macOS use the same registry-backed worker and subscription contract?
11. Did the slice introduce persistence, PID adoption, remote jobs, retries, scheduling, Product Task fabrication, new UI, or another non-goal?

Resolve every correctness/security finding with a focused failing test and a
separate fix commit before proceeding. Record non-blocking residual risks.

- [ ] **Step 6: Write the Gate decision record**

Create `docs/superpowers/spikes/2026-07-27-live-job-registry-decision.md` with:

```markdown
# Gate Decision: Live Job Registry Slice 4

**Status:** Proposed for user approval
**Date:** 2026-07-27
**Branch:** feat/agent-foundation-live-job-registry

## 1. Selected architecture
## 2. Admission authority matrix
## 3. Lifecycle and retention evidence
## 4. Video-import migration evidence
## 5. Cancellation, child reaping, and scratch evidence
## 6. Notification-loss and collect-once evidence
## 7. Restart, shutdown, and no-PID-adoption evidence
## 8. Privacy inspection
## 9. Verification command results
## 10. Independent review findings and resolutions
## 11. Migration and rollback
## 12. Residual risks
## 13. Scope boundary
```

Populate every section with actual commit IDs, test names/counts, measured
cancellation bound, review findings, and observed evidence. Do not copy planned
results into the record.

- [ ] **Step 7: Commit gate evidence**

```bash
rtk git add docs/superpowers/spikes/2026-07-27-live-job-registry-decision.md
rtk git commit -m "docs(agent): record live job registry gate evidence"
```

Any defect fix from review or verification must already have its own focused
commit. Never stage unrelated files.

- [ ] **Step 8: Stop for Slice 4 gate approval**

Present the decision record and current verification evidence. Do not begin
Slice 5, Slice 6, durable/remote Job recovery, agent-started Jobs, managed
FFmpeg setup work, or launch-video design until the user explicitly approves
the next scope.
