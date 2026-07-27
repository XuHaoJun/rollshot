# Agent Foundation Slice 4: Live Job Registry Design

**Date:** 2026-07-27
**Status:** Approved in brainstorming auto mode
**Area:** Agent foundation / process-local job lifecycle
**Governing umbrella:**
[`2026-07-26-agent-foundation-umbrella-design.md`](2026-07-26-agent-foundation-umbrella-design.md)
**Previous gate evidence:**
[`2026-07-27-authority-static-skills-decision.md`](../spikes/2026-07-27-authority-static-skills-decision.md)

## 1. Decision summary

Slice 4 adds one bounded, process-local live-job registry to
`rollshot-agent` and migrates the existing Action Guide video-import worker as
the proof workload. The registry owns live identity, admission, state,
structured progress, cancellation routing, terminal truth, collect-once result
handoff, and short terminal retention. The video-import worker continues to own
its concrete FFprobe/FFmpeg child handles and scratch resources because its
existing RAII cancellation and reaping behavior is already the correct
resource boundary.

The registry is not durable. It stores no PID, command line, source path, raw
log, media bytes, or provider transcript; it cannot reattach after application
restart. Startup creates an empty registry and reuses the existing locked
scratch scavenger. A prior-process PID is never adopted.

The current proof remains a direct user-initiated product action. No agent tool
is added for starting a job. Product Task correlation is represented in the
shared contract but is absent for this Action Guide job. An agent-started job
must later present a current immutable authority snapshot and a dedicated job
operation grant; that admission source is deliberately unsupported in this
slice rather than borrowing an unrelated Slice 3 grant.

## 2. Gate G2 readiness and current-code drift

Planning may proceed because the Slice 3 implementation is merged on `main` as
`3d69781` / PR #104, its decision record contains Gate G2 evidence and an
independent review with no correctness or security defects, and the user has
explicitly requested the next stage. Current verification on 2026-07-27 also
passed:

- `rtk cargo test -p rollshot-agent`: 378 tests passed;
- `rtk cargo test -p rollshot-action video_import`: 57 tests passed; and
- `rtk cargo test -p rollshot-app --features action-guide action_guide_home::video_import::tests`:
  10 tests passed.

Material drift from the 2026-07-22 long-running-jobs research:

- `AuthoritySnapshot`, `RunOperation`, durable Product Task V2 receipts, and
  static skill provenance now exist in `rollshot-agent`;
- Action Guide video import still uses `ImportCoordinator`, an iced `Task`,
  `spawn_blocking`, an atomic cancellation token, and operation-ID-based stale
  message rejection;
- `CancellableChild` still kills and waits on cancellation and `Drop`, bounds
  stderr to 64 KiB, and has cancellation/drop/stalled-decoder tests;
- `ImportedScratch` still owns a lock and removes itself on `Drop`, while both
  Linux and macOS startup paths call the stale-scratch scavenger; and
- no reusable Job ID, registry, authoritative process-local snapshot, terminal
  retention, collect-once result, or restart reattachment contract exists.

The Slice 3 gate record still says “Proposed for user approval.” The merged PR,
current passing evidence, and this explicit request to plan the next stage are
treated as the Gate G2 progression decision. The historical record is not
rewritten.

## 3. Problem

Action Guide video import already behaves like a live local-media operation, but
its lifecycle truth is split between UI state, an iced task channel, a blocking
worker, cancellation state, child-process RAII, and a completion message. The
initiating screen must remain present to interpret messages, terminal output is
not queryable independently of delivery, and no reusable host contract can
identify, observe, cancel, collect, retain, or clean up live work.

A model turn is the wrong lifetime owner. A Product Task is also the wrong
replacement: it records durable intent and artifact review, while this slice
needs only live process-local execution. The registry must therefore be a small
host lifecycle boundary, not a workflow scheduler or durable job ledger.

## 4. Goals

1. Define typed `JobId`, kind, owner, optional Product Task correlation,
   execution class, lifecycle state, progress, bounded diagnostics, terminal,
   cancellation, and collection outcomes.
2. Admit work only from an explicit product authority source and reject missing,
   mismatched, stale, or unsupported authority before worker launch.
3. Make registry state authoritative within the current process even when
   transient iced notifications are coalesced or dropped.
4. Preserve video import cancellation, child reaping, scratch cleanup, resource
   limits, error UX, and late-result rejection.
5. Retain terminal metadata briefly and retain a successful in-memory result
   until it is collected once or expires.
6. Cancel active work on registry shutdown and classify abandoned workers
   honestly without claiming cancellation before cleanup is confirmed.
7. Start empty after restart, scavenge unlocked stale import scratch, and never
   adopt a process by PID.

## 5. Non-goals

This slice does not add:

- durable job persistence, rehydration, reconnectable event replay, or remote
  job receipts;
- PID persistence, PID adoption, process discovery, or a cross-platform process
  supervisor;
- a workflow DAG, dependencies, scheduling, retries, queues, priorities, child
  agents, parallel tool execution, or model polling turns;
- a generic artifact store or automatic artifact promotion;
- an agent tool that starts video import or any other job;
- a new `RunOperation` merely to make an unused agent admission path appear
  complete;
- a Product Task for direct Action Guide import;
- raw stdout/stderr retention, full paths in snapshots, image/video bytes,
  semantic input, credentials, or provider conversation state;
- changes to managed FFmpeg installation, video selection algorithms, imported
  workspace format, capture behavior, or visible UI; or
- process-restart recovery guarantees that a process-local registry cannot
  provide.

## 6. Considered approaches

### 6.1 Selected: shared generic registry plus product adapter

Add the lifecycle contract and generic registry in `rollshot-agent`, then
instantiate it in `rollshot-app` with typed video-import progress and result.
The app maps direct user consent into admission, maps `VideoImportCancellation`
into a cancellation callback, and lets the existing worker retain concrete
child/scratch ownership.

This keeps the state machine reusable without making `rollshot-agent` depend on
`rollshot-action` or iced. Generic progress/result types avoid type erasure and
avoid copying an `ImportedWorkspaceSeed`. It also preserves the established
child-process boundary instead of moving FFmpeg details into an agent crate.

### 6.2 Rejected: generalize only `ImportCoordinator`

An app-only coordinator change would be smaller, but no other workload could use
its lifecycle contract and terminal truth would remain coupled to one screen.
It would not satisfy the umbrella’s reusable registry requirement.

### 6.3 Rejected: create a new workspace crate

A `rollshot-live-job` crate would provide a clean package boundary, but Slice 4
has one proof workload and no second consumer. A new crate adds dependency,
release, lint, and public-API surface without evidence that the module cannot
remain coherent in `rollshot-agent`.

### 6.4 Rejected: make every job a durable Product Task

This would conflate durable product intent/review with live resource ownership,
force Action Guide import through Smart Redaction-specific Task kinds and source
bindings, and create crash-reconciliation scope explicitly excluded by the
umbrella.

## 7. Architecture and ownership

```text
Action Guide user selects video
        │
        ▼
ImportCoordinator resolves toolchain (pre-job operation identity)
        │
        ▼
Product builds DirectUserAction admission + cancellation callback
        │
        ▼
LiveJobRegistry::admit → JobId + JobReporter
        │                         │
        │                         └── blocking import worker
        │                              ├── CancellableChild owns/reaps FFmpeg
        │                              ├── ImportedScratch owns staged files
        │                              └── reports progress/terminal/result
        │
        ├── snapshot/list/cancel/collect
        └── monotonic watch revision → iced subscription → UI projection
```

Ownership is strict:

- `LiveJobRegistry` owns live Job records, admission validation, cancellation
  routing, monotonic revisions, terminal classification, successful result
  retention, collect-once semantics, and terminal pruning.
- `JobReporter` is the worker’s capability to update exactly one Job. It cannot
  mutate another Job or admit new work.
- `ImportCoordinator` owns only presentation/preparation state and the mapping
  from its pre-job `ImportOperationId` to the admitted `JobId`. It is not
  terminal truth after admission.
- `CancellableChild` continues to own each concrete child handle and pipe and
  must kill/wait before reporting `Cancelled` or returning a terminal failure.
- `ImportedScratch` continues to own staged output and cleanup. The accepted
  `ImportedWorkspaceSeed` remains an in-memory product result, not a generic
  agent artifact.
- Product code owns authority construction and result consumption. Skills and
  model prose own neither.

## 8. Shared public contracts

The implementation adds `pub mod jobs` in `rollshot-agent` with these semantic
contracts. Exact Rust spelling may follow existing style, but the distinctions
and invariants are fixed.

### 8.1 Identity and metadata

- `JobId`: opaque `job-<UUID>` identifier, generated once at successful
  admission and never derived from PID or array position.
- `JobKind`: closed V1 enum containing only `ActionGuideVideoImport`.
- `JobExecutionClass`: closed V1 enum containing `LocalWorkerWithChildProcesses`.
- `JobOwner`: either:
  - `DirectProductAction { surface, operation_nonce }`; or
  - `ProductTask(JobTaskRef)`.
- `JobTaskRef`: exact `ProductTaskId`, `TaskAttemptId`, and `RunId` correlation.
  It is metadata, not permission and not proof of artifact approval.

`ImportOperationId` remains a distinct, app-local preparation identity. It is
not renamed to `JobId`: toolchain resolution/setup happens before job admission,
and late setup messages still need rejection. Once a job is admitted, the
coordinator stores both identities until presentation detaches or terminal
collection completes.

### 8.2 Admission authority

`JobAdmission` is a checked value containing kind, owner, execution class, and
one `JobAuthoritySource`:

- `DirectUserAction(ActionGuideVideoImport)` is accepted only for
  `JobKind::ActionGuideVideoImport` with a direct-product owner and no Product
  Task reference. It represents the current picker action and is constructed by
  product code, never from a skill body or model output.
- `AgentTask { authority_snapshot, task_ref }` is represented but rejected as
  `UnsupportedAuthoritySource` in V1. No existing `RunOperation` honestly grants
  detached Job start. A later workload must add a dedicated operation and exact
  task/run binding before enabling this source.

Admission fails before allocating a Job record or launching work for missing
control, kind/source mismatch, owner/task mismatch, terminal-capacity pressure,
active-capacity exhaustion, or unsupported authority. No fallback converts an
invalid agent admission into direct user authority.

This is the Gate G2 dependency: static skill availability and content cannot
create direct-product authority, mutate the registry, or borrow
`ExecuteRestrictedAutomation` to start work.

### 8.3 Lifecycle state

`JobState` is closed and typed:

```text
Starting → Running → Succeeded
                   → Failed
                   → Cancelling → Cancelled
Starting ─────────→ Cancelling → Cancelled
```

Rules:

- admission creates `Starting`;
- the worker reports `Running` only after it owns its worker lease and before
  external process work;
- `cancel` changes an active record to `Cancelling`, invokes the cancellation
  callback outside the registry lock, and returns a typed outcome;
- only the worker may confirm `Cancelled`, after import cancellation has
  returned and child/scratch cleanup has run;
- a worker success report received while `Cancelling` drops the result and
  terminalizes as `Cancelled`, because worker return confirms resource cleanup
  but the cancellation boundary forbids result acceptance;
- worker lease loss before a terminal report becomes
  `Failed(WorkerAbandoned)`, never success;
- every terminal is immutable; duplicate identical reports are idempotent,
  while conflicting terminal reports return a typed conflict;
- late progress and terminal writes cannot revive a terminal or mutate a
  different Job.

`cancel` returns `Requested`, `AlreadyRequested`, `AlreadyTerminal`, or
`NotFound`. It never reports confirmed cancellation synchronously.

### 8.4 Progress, diagnostics, and observation

`LiveJobRegistry<P, R>` is generic over structured progress `P` and successful
result `R`. It stores only the latest progress value, not an unbounded history.
Every accepted transition/progress update increments a per-Job monotonic
`revision`; the registry also emits a coalescible process-local watch revision.

A `JobSnapshot<P>` contains identity, metadata, state, latest progress, bounded
failure category, start/update/terminal times, cancellation-request time,
collection status, and revision. It never contains `R`, a cancellation callback,
path, PID, child handle, or raw log.

Diagnostics are bounded to 64 sanitized entries of at most 256 bytes each.
Video import records stable categories and numeric summaries only; it does not
copy the existing 64 KiB stderr ring into the registry. Overflow drops the
oldest entry and increments a dropped count.

Subscribers treat notifications as hints. On any notification they query the
latest snapshot; coalesced, duplicate, reordered, or dropped transient
notifications cannot change terminal truth.

### 8.5 Result collection and retention

A successful `R` is held inside the registry and omitted from snapshots and
`Debug`. `collect(JobId)` moves it out exactly once and records collection in
terminal metadata. Repeated collection returns `AlreadyCollected`; collection
before success returns a typed state error.

V1 limits are fixed constants, not user configuration:

- maximum 4 active Jobs per registry;
- maximum 128 retained terminal records;
- terminal/result TTL of 5 minutes after terminal time; and
- the diagnostic bounds above.

Admission prunes expired terminal records first. If the terminal cap remains
full, the oldest collected terminal is pruned before an uncollected success.
An uncollected successful result may be dropped only at TTL expiry; expiry is an
explicit `ResultExpired` collection outcome. Active Jobs are never evicted.
Tests inject timestamps directly rather than sleeping.

## 9. Action Guide migration

### 9.1 Shared state

`ActionGuideHome` receives one shared video-import registry instance. Both Linux
and macOS product constructors pass the same type and use the shared
`action_guide_home` update/worker path. No platform-specific lifecycle variant
is introduced.

The existing toolchain resolution/setup remains pre-job work under
`ImportOperationId`. When a toolchain is available:

1. create `VideoImportCancellation`;
2. build the checked direct-user admission;
3. admit the Job with a cancellation callback wrapping that token;
4. bind the returned `JobId` to the current import operation;
5. launch the existing blocking import with its `JobReporter`; and
6. transition the reporter to `Running` before invoking `import_video`.

Admission failure produces the existing bounded user-facing error path and does
not spawn the worker.

### 9.2 Progress and terminal projection

The worker reports `VideoImportProgress` directly to the registry. The app’s
iced subscription observes registry revisions and sends a small
`JobRegistryChanged(JobId)` message. Update code queries the snapshot and:

- projects the latest progress into `ImportCoordinator` when the bound
  operation/job is still current;
- ignores stale updates for presentation while leaving registry truth intact;
- on success, calls collect once and opens the imported timeline;
- on failure, shows the existing error category/message and detaches the
  presentation; and
- on confirmed cancellation, returns presentation to idle without showing an
  error.

The worker writes terminal state before notifying iced. A lost/coalesced
notification is repairable from the registry snapshot. The old
`ImportProgress` and payload-bearing `ImportFinished` channel messages are
removed in the cutover; no compatibility alias remains.

### 9.3 Cancellation and shutdown

`CancelImport` asks the registry to cancel the bound Job. Presentation may
return to idle immediately, but the registry remains `Cancelling` until the
worker confirms cleanup. A new import receives a new operation and Job ID, so
late updates cannot affect it.

Registry shutdown requests cancellation for every active Job and invalidates
new admission. The worker lease plus existing `CancellableChild` and scratch
RAII provide in-process cleanup. Tests wait up to the existing two-second child
reaping bound; the production registry does not busy-wait or block the iced
update loop.

A hard application-process death cannot run registry shutdown. On the next
launch, both platform paths create an empty registry and run the existing
lock-aware stale import scratch cleanup. Locked/live scratch is skipped; a
later launch may collect it after the owner disappears. No persisted PID or Job
record is read, and no old process is attached or signalled by guessed identity.

## 10. Errors and privacy

Shared errors are typed by boundary:

- `JobAdmissionError`: invalid/missing control, kind-authority mismatch,
  owner-task mismatch, unsupported authority, shutting down, active limit, or
  terminal capacity;
- `JobTransitionError`: not found, invalid transition, stale reporter, or
  terminal conflict;
- `JobCancelOutcome`: requested, already requested, already terminal, not found;
- `JobCollectError`: not found, not succeeded, already collected, result
  expired; and
- `JobFailureCategory`: bounded stable categories including worker abandoned,
  worker panic, cancellation cleanup failure, and mapped video-import error
  categories.

No speculative global error abstraction is introduced. User-facing strings stay
in `rollshot-app`; the registry stores stable categories.

`Debug`, snapshot, tracing, and diagnostics must omit:

- input and scratch paths;
- video/image bytes or decoded frames;
- raw FFmpeg/FFprobe output;
- cancellation closures, child handles, PIDs, commands, and environment;
- `ImportedWorkspaceSeed` content;
- credentials, model input, skill bodies, and provider data.

Runtime diagnostics use stable `rollshot::agent::jobs` and
`rollshot::app::action_guide::video_import` targets with Job ID, kind, state,
revision, category, and numeric progress only.

## 11. Testing strategy

Implementation is test-driven and must add observable contract coverage before
production changes.

### 11.1 Registry state-machine tests

Cover:

- unique typed ID and exact metadata binding;
- fail-closed missing, mismatched, stale, and unsupported admission;
- active limit at 1/2/4 and fifth-job rejection;
- every allowed lifecycle transition and every forbidden transition;
- cancellation request versus confirmed cancellation;
- no success/result after cancellation request;
- worker lease abandonment;
- stale reporter and cross-Job update rejection;
- progress revision monotonicity under duplicate/reordered notifications;
- dropped/coalesced watch notification repaired from snapshot;
- bounded diagnostics and dropped-entry count;
- collect-once result movement without cloning;
- 5-minute TTL, 128-terminal cap, pruning order, and result expiry; and
- shutdown rejection of admission plus cancellation of all active Jobs.

Use deterministic timestamps and synchronization barriers, not sleeps, except
for the existing bounded child-process fixture.

### 11.2 Video-import preservation tests

Before migration, retain or strengthen current tests proving:

- cancellation kills and waits for direct children;
- `CancellableChild::Drop` kills and waits;
- stalled decoder cancellation returns within two seconds;
- cancellation and faults remove scratch;
- stale operation progress/completion cannot affect a newer import; and
- startup cleanup removes unlocked stale scratch and skips locked live scratch.

After migration, add shared app tests proving:

- exactly one worker starts after successful admission;
- admission failure starts no worker;
- progress comes from registry snapshots, not delivery history;
- cancellation remains `Cancelling` until worker cleanup confirmation;
- a terminal written before a lost notification is still collected once;
- stale Job updates cannot open a timeline or replace current progress;
- success after cancellation is not collected/opened;
- source paths and result payloads are absent from snapshots, `Debug`, tracing,
  and diagnostics; and
- Linux and macOS handlers use the same registry-backed start path.

No golden visual baseline changes are expected because layout, copy, and visible
interaction remain unchanged. If implementation changes visible iced behavior,
the iced UI testing workflow becomes mandatory before that edit.

### 11.3 Verification commands

At minimum:

```bash
rtk cargo test -p rollshot-agent jobs
rtk cargo test -p rollshot-action video_import
rtk cargo test -p rollshot-app --features action-guide action_guide_home
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
```

Independent code review must inspect lifecycle honesty, cancellation/reaping,
result retention, admission authority, privacy, and both platform callsites
before the slice gate decision.

## 12. Acceptance criteria and Slice 4 gate

Slice 4 passes only when all are true:

1. `Starting`, `Running`, `Succeeded`, `Failed`, `Cancelling`, and `Cancelled`
   are typed and covered by deterministic state-machine tests.
2. Each admitted Job has a stable `JobId`, kind, owner, execution class, and an
   explicit authority source; missing or mismatched authority fails before
   launch.
3. Direct Action Guide import has no Product Task reference, while the shared
   metadata can represent an exact task/attempt/run reference without treating
   it as authority.
4. No agent/skill path can start a Job in V1, and no unrelated Slice 3 grant is
   reused.
5. Structured progress is bounded and terminal truth remains queryable after
   transient notification loss.
6. Cancellation reaches the video-import token; child cancellation/reaping
   stays within the existing two-second bound; confirmed cancellation is not
   reported before worker cleanup.
7. Successful result collection is exactly once; cancelled, failed, stale, or
   expired results never open a timeline.
8. Active Jobs are bounded; terminal records and uncollected results expire
   after five minutes; no active Job is evicted.
9. Registry shutdown requests cancellation for all active work; startup begins
   empty, scavenges unlocked stale scratch, and performs no PID adoption or
   durable reattachment.
10. Snapshots, `Debug`, diagnostics, serialization, and tracing contain no
    source paths, media bytes, raw process output, credentials, skill bodies, or
    result payloads.
11. The existing Linux and macOS Action Guide import paths use the same shared
    registry-backed worker and preserve current product behavior.
12. A decision record captures verification, independent review, migration,
    residual risks, and deferred scope.

## 13. Stop and rollback conditions

Stop and revise this design rather than weakening it if:

- an iced task can be dropped in a way that also destroys the only worker
  ownership or terminal repair path;
- `ImportedWorkspaceSeed` cannot be retained and moved exactly once without a
  clone or type erasure;
- cancellation cannot distinguish request from confirmed child cleanup;
- the shared registry would need to store paths, raw logs, media, PIDs, or
  concrete child handles to function;
- the macOS and Linux product paths require divergent lifecycle contracts;
- registry shutdown requires blocking the iced update loop; or
- a Product Task or new agent grant is required only to force the current
  direct-user import into an agent-shaped authority model.

Rollback is a clean reversal to the current `ImportCoordinator` message path.
No durable migration exists, because the registry is process-local and its
state is never serialized.

## 14. Residual risks and deferred work

- Hard process death cannot confirm cancellation or run destructors. Startup
  scratch scavenging is the bounded recovery; child-process restart adoption is
  explicitly unsupported.
- The current proof has one Job kind and one active product flow. A second kind
  may justify moving the module to a dedicated crate, but not before measured
  coupling appears.
- Direct-user admission is an internal product authority boundary, not an OS
  security sandbox. Agent-started Jobs remain disabled until a real workload
  defines a dedicated grant and task binding.
- The registry retains in-memory results only. Durable or remote results require
  Slice 2 artifact promotion or a separately approved remote-job design.
- Managed FFmpeg setup remains pre-job and is not cancellable through this
  registry. Concurrent setup and crash-atomic installation remain outside this
  slice.
- macOS runtime verification may remain unavailable on the Linux workstation;
  shared-code tests and callsite review reduce but do not eliminate that
  platform risk.

## 15. Implementation-plan boundary

The implementation plan must:

1. lock down current import cancellation, reaping, scratch, and late-message
   behavior before registry edits;
2. add registry state-machine/admission/retention tests before implementation;
3. implement the shared process-local registry without iced or
   `rollshot-action` dependencies;
4. migrate the shared Action Guide import path atomically, updating both Linux
   and macOS effect handlers;
5. remove obsolete payload-bearing progress/terminal message plumbing in the
   same cutover;
6. verify cancellation, worker abandonment, notification loss, collect-once
   success, shutdown, stale scratch cleanup, and privacy;
7. run independent code review before writing the Slice 4 gate decision; and
8. stop at the Gate decision without beginning Slice 5 or Slice 6.
