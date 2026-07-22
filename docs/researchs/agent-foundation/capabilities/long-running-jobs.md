# Long-running jobs and processes comparison

**Research date:** 2026-07-22 (Asia/Taipei)
**Status:** In Progress (Round 3 capability comparison)
**Umbrella revision:** 1
**Current Rollshot revision:** `6a4217c9672abe3541bdc21c569b2d97ae4325fb`
**Reference revisions:** Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`;
oh-my-pi `7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`; Hyperframes
`807078c7cde9d5c8403588722d1cd9397c513a0d`.
**Evidence mode:** static source, test-source, and repository-instruction
inspection. No provider, preview server, video import, render, remote Job,
disconnect, process crash, or orphan-recovery scenario was executed.

This document compares work that can outlive the model turn that started it.
It does **not** select a final Rollshot architecture.

## 1. Rollshot problem and three workload traces

The foundation question is not merely how to run a command in the background.
It is whether the host can still identify, observe, stop, collect, and clean up
that work after the initiating model turn is gone, and whether it can reconcile
the work after a UI or process boundary.

| Workload | Concrete trace | What the trace establishes |
|---|---|---|
| **Smart Redaction** | One bounded Agent Run alternates provider and serial Tool work under a finite 16-dimensional budget, then returns a typed review/failure terminal. [E:R1] | The current run wall-time and cancellation boundary are sufficient. A detached Job is **not established as necessary** by this workload [G:W1]; making every Tool call a Job would add handles, retention, and recovery policy without user value. |
| **Action Guide video import** | Both Linux and macOS route the shared `ImportCoordinator` through toolchain resolution/setup, an iced `Task`, `spawn_blocking`, FFprobe, two FFmpeg passes, progress messages, cancellation, scratch publication, and a final imported workspace seed. [E:R2-R5] | A real local media operation may outlive many UI/model turns. It needs live progress, late-message rejection, bounded diagnostics, cancellation, process reaping, and partial-output cleanup. Current source does not establish process-restart reattachment [A:R-JOB]. |
| **Deferred brag + Hyperframes** | A background preview server supports review; audio/generation can overlap frame work; local render reports stage progress and commits only a validated Artifact; hosted, Lambda, and Cloud Run renders return remote handles that can be polled for status, frames, cost, errors, and output. [E:H1-H5] | If adopted, this pressures preview-service supervision, local render Jobs, remote Job receipts, authoritative polling, cost visibility, Artifact validation, idempotent start, and recovery across a model turn or app process. It does not mandate that Rollshot ship video generation or copy Hyperframes. |

## 2. Terms and non-equivalent lifetimes

### 2.1 Three clocks that must remain separate

| Clock / unit | Owner and stopping rule | Must not imply |
|---|---|---|
| **Agent Run wall time** | The Agent Runner measures time spent in one model/Tool loop and may terminate it when its run budget or cancellation fires. | It is not a TTL for an already-started preview, FFmpeg process, cloud render, or child agent. Charging minutes of external waiting to model-loop wall time can terminate useful work while spending no model tokens. |
| **External Job lifetime** | A host Job owner starts or attaches to a process/service operation and retains its handle until terminal collection and cleanup/retention policy complete. | It is not a model turn, conversation, Workflow item, or Child Agent. The Job may be alive while no Agent Run exists. |
| **Child Agent lifetime** | A parent/registry owns a separate model context, Tool/Skill/permission resolution, budget, messages, interruption, and completion. [E:S1] | A child may start Jobs, but its transcript/status is not the Job handle and its completion does not prove an expected Artifact exists. |

A **Product Task** records bounded intent; a **Workflow** owns dependencies and
next-step routing; a **Job** owns detached/external execution; and an
**Artifact** is a validated product output. One record may reference another,
but the names are not interchangeable.

### 2.2 Host-owned Job lifecycle contract

The comparison uses six required operations. `wait` is an observation strategy,
not a seventh lifecycle phase.

1. **Start** — validate current authority and inputs; allocate a stable Job and
   attempt ID; persist an intent/idempotency key before any non-idempotent
   external effect when crash recovery is in scope; start the operation; then
   record the acknowledged local process handle or remote handle. A transport
   failure between effect and acknowledgement becomes `start_unknown`, never a
   blind retry.
2. **Observe progress** — return an authoritative status snapshot plus a
   monotonic observation cursor/version. Keep structured progress, bounded log
   references/tails, cost/usage, warnings, and partial Artifact references
   separate. Polling and subscriptions project the same Job state; dropped UI
   events cannot change completion truth.
3. **Cancel** — durably record cancellation intent when the Job can outlive the
   current process; call the authoritative owner; distinguish
   `cancel_requested`, `cancel_confirmed`, `already_terminal`, `not_found`, and
   `cancel_unknown`. A local `AbortController` or atomic flag is a live signal,
   not durable confirmation.
4. **Collect** — fetch terminal result/log receipts, validate and atomically
   publish expected Artifacts, record one collection receipt, and make repeated
   collection idempotent. Exit zero or a completion notification alone is not
   Artifact acceptance.
5. **Cleanup** — close pipes/sockets, reap child processes, remove scratch and
   partial downloads, release leases, and apply remote deletion/retention
   policy. Cleanup is idempotent and separately observable; terminal result
   retention must not depend on leaving live resources open.
6. **Reattach** — restore a stable handle and last cursor, revalidate current
   authority, query the authoritative process/service, replay missed bounded
   observations, and classify expired/missing identity as `lost` or
   `needs_reconciliation`, not `failed`. A PID without start identity, a
   Transcript, and a transport reconnect are insufficient.

```text
requested -> starting -> running -----------------> succeeded/failed
                 |          |                              |
                 |          +-> cancel_requested -------->+-> collect -> retained/cleaned
                 |                         |               |
                 +-> start_unknown         +-> cancel_unknown
                           |                         |
                           `-------- reconcile -----'

restart: load handle + cursor -> revalidate -> query/replay -> one state above
```

`lost`, `expired`, and `start_unknown` are host recovery outcomes, not evidence
that the external computation failed.

## 3. Job classes and required semantics

| Class | Canonical handle and status | Progress, logs, cost | Partial Artifacts / idempotency | Retention, crash, orphan, security |
|---|---|---|---|---|
| **Local process** | Host-generated Job/attempt ID plus an in-process child handle; optional PID/start fingerprint is diagnostic, not sufficient for restart. Status derives from spawn acknowledgement, `try_wait`/wait, exit, and confirmed kill. | Structured parser where available; bounded stdout/stderr tail or file reference; measured CPU/time/storage if needed. Provider cost normally absent. | Unique attempt scratch; publish final output only after validation. Start is safe to retry only before acknowledgement or when the process/output contract supplies deduplication. | Live owner reaps on cancel/drop/app shutdown. After owner crash, default is orphan detection and cleanup/reconciliation, not PID adoption. Command/cwd/env/credentials and log redaction are authority/privacy inputs. |
| **Preview server / interactive service** | Job ID plus bound host/port, process handle, health identity, project/workspace and start nonce. `running` requires a health check, not a spawned PID. | Health/connection count, bounded server logs, URL; cost usually host resource time. | Files remain product state, not server output. Start checks an existing matching identity before binding; a port collision is not reattachment. | Explicit stop/restart; idle/session retention policy; app restart may launch a fresh server after verifying the old identity. Default loopback binding and browser/profile/CDP policy must be explicit. |
| **Local media operation** | Product operation ID refers to one or more child processes and staged outputs; pass status is separate from child PID. | Pass/processed/total/candidate or frame counts, bounded diagnostic ring, elapsed/resource use. | Per-attempt scratch, resource caps, validate/hash/decode before marker-last publication; retry begins a new attempt and never overwrites an accepted output. | Cancellation reaches all children and reaps them; RAII handles ordinary exits, startup scavenging handles crash leftovers. Input paths/media-derived logs are sensitive. |
| **Remote Job** | Provider authority + stable remote Job ID (and execution ARN/name where needed), local attempt/idempotency key, expected outputs and observation cursor. | Provider status, stage/frame counts, cost, errors, remote log references, callbacks plus polling. | Query-by-ID/key closes ambiguous starts; collect downloads to a temporary file and validates before publish. Provider retry and product retry remain distinct. | Local process death does not cancel the remote Job. Reattach by authoritative query; handle/provider TTL and remote Artifact/log retention are recorded. Credentials, callback URLs, upload scope, egress and deletion require explicit authority. |

## 4. Current Rollshot behavior

### 4.1 Foundation boundary

The current `rollshot-agent` run owns wall-time budget and cancellation, but a
foundation Job ID/handle/store, start/observe/collect/cleanup/reattach API, and
external cost/log/retention record were **not found in the investigated scope**
[A:R-FOUNDATION]. This is a bounded absence, not a claim about every product
crate.

### 4.2 Reusable evidence from Action Guide video import

The shared app coordinator is a strong **live media-operation** reference:

- `ImportOperationId`, `ImportState`, pending path, cancellation and last
  progress are app-owned; a new/cancelled operation ignores late progress or
  completion from an old ID. Completion/cancel clears the source path and
  progress from the coordinator. [E:R2]
- Both Linux and macOS execute the same effects and `run_import_task`; this is
  shared product behavior rather than one platform-only path. [E:R3]
- `run_import_task` bridges a bounded progress channel to iced while
  `spawn_blocking` runs the synchronous import. Progress uses `try_send`, so it
  may be dropped; the blocking final message and operation-ID check are the
  live terminal authority. Worker panic becomes a typed UI error. [E:R3]
- `VideoImportCancellation` is a cloned `Arc<AtomicBool>`. `CancellableChild`
  polls, kills and waits on cancellation/error, and its `Drop` also kills and
  waits. Stdout/stderr are drained concurrently; stderr retention is capped at
  64 KiB. Tests cover cancellation reaping, early-drop reaping and a stalled
  decoder returning within two seconds. [E:R4]
- Import has preflight/analyze/extract progress, resource bounds, staging, and
  `ImportedScratch` cleanup. Tests cover cancellation scratch cleanup and fault
  outcomes; they were inspected, not run here. [E:R5]

These parts are reusable as **behavioral patterns**: stable live operation
identity, stale-event rejection, structured pass progress, cooperative signal
plus forced process reaping, bounded diagnostics, staged outputs, and cleanup
tests. They do not establish a reusable Job abstraction by themselves.

### 4.3 Managed FFmpeg: reusable supply-chain boundary, not Job management

`managed_ffmpeg.rs` resolves explicit environment overrides, PATH binaries, or
a pinned managed install; validates FFmpeg/FFprobe; and pins URL/version/
license, archive size and SHA-256. The downloaded archive is staged and hash-
checked in a `ScratchDir`, but `unpack_ffmpeg` writes directly into the live
managed `root/bin` directory. The function then validates both live binaries
and writes the versioned manifest last; on observed unpack, permission,
validation, or manifest errors it attempts to remove both binaries. [E:R6]

The pin/hash/license/validation and scratch-cleanup practices are relevant to
secure Job **prerequisites**. The module does not own video-import/render child
processes, Job status/progress/cost/log retention, cancellation, collection, or
reattachment; those concepts were **not found in its investigated source**
[A:R-MANAGED]. The manifest proves installed toolchain provenance, not a running
Job receipt.

Atomic install publication (for example, complete-tree rename), inter-setup
locking, and startup/process-crash cleanup of partial live installs were **not
found in the investigated production scope** [A:R-MANAGED-INSTALL]. This leaves
a bounded risk to test, not a demonstrated product bug: concurrent setup calls
target the same live binary/manifest paths, and process death can bypass
`ScratchDir::drop` or interrupt the live-bin-then-manifest sequence. This
comparison does not propose refactoring either app module.

## 5. Per-system behavior

### 5.1 Pi: extension processes, not a built-in Job model

Pi's uninstalled `subagent` extension spawns one `pi --mode json --no-session`
process per item. It parses message/tool-result events, emits live updates, and
accumulates turns, tokens, cache use and cost. Parallel mode caps eight items
and four subprocesses. One Tool-call abort handler sets `wasAborted`, calls
`proc.kill("SIGTERM")`, and after five seconds attempts
`proc.kill("SIGKILL")` only when `!proc.killed`; temporary prompt files are
removed in `finally`. Whether that condition provides reliable termination or
escalation was not runtime-verified. [E:P1]

This is a Child Agent example using processes, not a built-in process Job
contract. It returns no addressable process handle, durable status, retention
record, expected Artifact, idempotency key, or restart reattachment API; those
were **not found in the exact example/docs audit** [A:P-JOB]. The extension
guide instead makes each extension responsible for starting resources after
`session_start` and closing them in an idempotent `session_shutdown` hook
[E:P2]. A hard process crash cannot run that hook.

### 5.2 oh-my-pi: process-local `AsyncJobManager`

oh-my-pi has the clearest process-local Job registry in the core set. An
`AsyncJob` has ID, `bash|task` type, running/completed/failed/cancelled status,
start time, label, owner/child IDs, abort controller, promise, result/error text,
latest progress details and an optional queued flag. The manager supports
register, addressable cancel, owner-scoped list, watch/wait, progress callbacks,
completion delivery with retry, and disposal. Queued work does not consume the
default 15 running slots; terminal records are retained for five minutes by
default. [E:O1]

`resumeDeliveries` resumes suppressed completion delivery inside the same
manager; it is not restart recovery. The Jobs, controllers, timers, results and
delivery queue are process memory. Serialization, rehydration, durable
idempotency, Artifact completion, remote cost, and post-restart reattachment
were **not found in the manager/Hub audit** [A:O-JOB]. `dispose` cancels,
waits/drains for a bounded time, clears maps, and returns whether cleanup
settled; a killed process bypasses it.

### 5.3 Codex: live background terminals plus bounded exec-server recovery

Unified exec can yield a live background terminal. `BackgroundTerminalInfo`
contains item ID, process ID, command and cwd; the process manager lists and
terminates live entries. It is not a durable Job ledger and carries no progress,
cost, Artifact, idempotency, retention, or restart receipt; those fields were
**not found in the exact background-terminal/Thread reconstruction audit**
[A:C-TERMINAL]. [E:C1]

The separate exec-server is materially stronger but narrower. `initialize`
accepts `resume_session_id`. A disconnected session retains the same
`ProcessHandler` for 30 seconds (200 ms in tests), disables notifications, and
allows one connection to reattach. The client retries for 25 seconds, then
calls `process/read(after_seq = last_published_seq)` for each acknowledged
recoverable process. Output replay is capped at 1 MiB or 50,000 chunks per
process; a gap/recovery error triggers termination. TTL expiry or exec-server
shutdown also shuts down retained processes. [E:C2]

This is live transport/session reattachment, not Thread Resume or recovery
after exec-server death. Durable process serialization, a remote cost/Artifact
contract, and recovery beyond the retention window were **not found in the
exec-server/ThreadStore scope** [A:C-RECOVERY].

### 5.4 Claude Code: live Runtime Tasks and authoritative remote sidecars

The root Runtime Task registry has random typed IDs, pending/running/completed/
failed/killed status, start/end times, output path/offset, notification state,
and per-type kill. Shell/agent output is append-only in a session-scoped file
with no-follow protection and a 5 GiB cap; registry/controllers remain process
memory. A generic local shell/Task resurrection or process reattachment routine
was **not found in the investigated Runtime Task roots** [A:L-LOCAL]. [E:L1]

Remote Agent Tasks use a different recovery model. A sidecar persists only task
identity, remote session ID/type/title/command, spawn time and selected metadata;
status is deliberately fetched fresh from authoritative CCR on resume. Still-
live sessions are re-registered and polling restarts; archived/404 sessions
remove the sidecar, while auth/network failures preserve it for a later attempt.
Kill archives the remote session on its applicable path, and terminal polling
removes the sidecar. [E:L2]

This is a useful minimal remote receipt, but external launch is build-flavor
gated. A provider-neutral Job schema, generic cost record, idempotent start key,
remote Artifact validation contract, and normative remote retention/deletion
policy were **not found in the external-source remote-task audit** [A:L-REMOTE].

### 5.5 Hyperframes: preview, local render, and remote render are distinct

- `preview` is a long-running local server. Instructions require starting it as
  a background task, confirming it serves, treating an exited task as down,
  and restarting/re-handing the URL if needed. Context/selection queries attach
  to an already-running matching server and report explicit not-running,
  ambiguous-server, and port-mismatch failures. [E:H1]
- Local `render` constructs an in-process Producer Job, awaits it, renders
  stage/percent progress, and treats the Producer's `artifact validated` plus
  disk commit checkpoint as success. It scans/kills recorded orphan browser
  trees before a new render. The CLI call does not expose a stable Job handle
  for later reattachment; an external host could supervise the CLI process, but
  a built-in local reattach/retention contract was **not found in the focused
  local render/preview source** [A:H-LOCAL]. [E:H2]
- Hosted cloud render supports `--no-wait`, stable `render_id`, callback,
  polling, terminal failure text, download, and an optional idempotency key.
  Poll timeout explicitly says the remote render may still be running and gives
  a `cloud get <id>` recovery command. [E:H3]
- Lambda returns `renderId`, execution ARN and S3 URIs. `progress` exposes
  Step Functions status, overall/frame progress, Lambda count, cost breakdown,
  errors and output. Cloud Run similarly returns render/execution identity and
  exposes status, frames, cost, errors and GCS output. [E:H4, E:H5]

Across the investigated hosted/Lambda/Cloud Run command and SDK roots, a common
cancel operation, cleanup/retention contract, or provider-neutral reattachment
record was **not found** [A:H-REMOTE]. A `cancelled` status from an authoritative
service is positive observation, not evidence that these CLI roots expose a
cancel command.

## 6. Cross-system lifecycle matrix

Every negative or unknown cell names the exact bounded audit in Section 13.

| System | Handle / status | Progress / logs / cost | Cancel / collect / cleanup | Idempotency / retention / crash reattach |
|---|---|---|---|---|
| **Rollshot video import** | Live operation ID + pass state; child handles are encapsulated [E:R2,R4]. | Structured pass progress; bounded stderr ring; cost/log cursor **not found** [A:R-JOB]. | Atomic flag; child kill+wait/drop reap; scratch cleanup; collect is an in-memory workspace seed [E:R3-R5]. | Durable key/record/retention/reattach **not found** [A:R-JOB]. |
| **Pi extension example** | One closure-local subprocess/result; no returned addressable handle [A:P-JOB]. | Streamed messages/tools plus token/cache/cost totals [E:P1]. | Shared abort calls `SIGTERM`; after five seconds it attempts `SIGKILL` only when `!proc.killed`. Termination reliability is runtime-unverified [E:P1]. Expected Artifact collection **not found** [A:P-JOB]. | Built-in Job persistence/idempotency/retention/reattach **not found** [A:P-JOB]. |
| **oh-my-pi** | Addressable process-local ID/status/owner/queued flag [E:O1]. | Latest details, result/error text, watch/wait and delivery diagnostics; provider Job cost **not found** [A:O-JOB]. | Abort-controller cancel; result delivery retry; timed dispose and eviction [E:O1]. Typed Artifact collect **not found** [A:O-JOB]. | Five-minute live retention; durable serialization/idempotency/reattach **not found** [A:O-JOB]. |
| **Codex terminal** | Live item/process ID, command, cwd [E:C1]. | Process stream elsewhere; structured Job progress/cost **not found** [A:C-TERMINAL]. | Addressable terminate and live cleanup [E:C1]. Artifact collect **not found** [A:C-TERMINAL]. | Thread-restart handle recovery **not found** [A:C-TERMINAL]. |
| **Codex exec-server** | Session/process identity with acknowledged recoverable flag [E:C2]. | Bounded sequenced stdout/stderr replay; cost/Artifact progress **not found** [A:C-RECOVERY]. | Terminate RPC; gap/TTL cleanup [E:C2]. Typed Artifact collect **not found** [A:C-RECOVERY]. | 30-second live reattach; server-restart durability/idempotent start **not found** [A:C-RECOVERY]. |
| **Claude local Runtime Task** | Live Task ID/status/type/output path [E:L1]. | File/output offsets and 5 GiB cap; generic cost **not found** [A:L-LOCAL]. | Type-specific kill and terminal eviction [E:L1]. Typed Artifact collect **not found** [A:L-LOCAL]. | Generic local resurrection/idempotency/retention policy **not found** [A:L-LOCAL]. |
| **Claude remote Task** | Sidecar remote session identity; fresh authoritative status [E:L2]. | Remote log/progress and output file; generic cost **not found** [A:L-REMOTE]. | Poll/notify; applicable kill archives; sidecar removed at terminal [E:L2]. Typed Artifact validation **not found** [A:L-REMOTE]. | Sidecar reattach is positive; start idempotency and normative retention **not found** [A:L-REMOTE]. |
| **Hyperframes local/preview** | Background task/process externally; Producer Job is awaited internally [E:H1,H2]. Stable reattach handle **not found** [A:H-LOCAL]. | Preview health; render percent/stage; cost record **not found** [A:H-LOCAL]. | Local render validates/commits Artifact; common cancel/cleanup API **not found** [A:H-LOCAL]. | Orphan browser scan exists; local reattach/retention/idempotency **not found** [A:H-LOCAL]. |
| **Hyperframes remote** | Hosted render ID, Lambda ARN/render ID, Cloud Run execution name [E:H3-H5]. | Status, frames, errors, cost and output (surface-dependent) [E:H3-H5]. | Poll/get/download; common cancel/cleanup API **not found** [A:H-REMOTE]. | Hosted cloud idempotency key is positive [E:H3]; common retention/reattach schema **not found** [A:H-REMOTE]. |

## 7. State, authority, scheduling, and failure ownership

### 7.1 State and authority

The host, not the model, owns Job identity and transitions. The model may ask to
start/cancel/collect, but a Tool result or Todo cannot forge an acknowledged
handle or terminal. A local process owner controls OS handles and reaping; a
remote service controls authoritative status; the product owns Artifact
acceptance and review; current permission/consent policy controls each effect.

Persist only what recovery needs: opaque input/Artifact references, sanitized
command/config fingerprints, authority/provider identity, handle, cursors,
status, cost and receipts. Do not default to persisting screenshot bytes,
absolute source paths, raw stderr, credentials, callback secrets, or full
provider payloads.

### 7.2 Scheduling and wall-time accounting

- Agent Run wall time stops when the model/Tool loop yields its Job handle. A
  later observe/collect turn has its own run wall time.
- Job lifetime and resource/cost ceilings continue independently. Waiting may
  be implemented by UI subscription, host polling, or a bounded wait Tool; the
  model need not spend turns polling.
- Child Agent budgets and concurrency remain separate. A child can request a
  Job under explicitly allocated authority, but child termination must not
  silently orphan or cancel it; the declared Job owner decides.
- Preview servers use service slots/health policy, media operations use CPU/
  disk/process caps, and remote Jobs use provider concurrency/cost caps. One
  global integer cap cannot express all three safely.

### 7.3 Failure, cancellation, retry, and partial outputs

| Failure point | Required host outcome |
|---|---|
| Start request sent, acknowledgement lost | `start_unknown`; query by idempotency key/provider identity. Never blind-start a chargeable render. |
| Progress callback/event lost | Re-observe from authoritative state/cursor. UI progress may stall, but Job terminal cannot change. |
| Cancel signal sent, confirmation lost | `cancel_unknown`; query. Keep partial outputs quarantined until terminal reconciliation. |
| Local process exits zero, output missing/invalid | Job `failed` at collect/validation; do not publish or unlock successors. |
| Artifact committed, terminal event lost | Reconcile Artifact receipt/hash and record completion once; do not rerun. |
| App crashes with child alive | On startup, use a host-owned orphan marker/fingerprint only to terminate/quarantine; do not adopt a PID as trusted work. |
| Remote handle expired/not found | Distinguish `lost`/`expired` from execution `failed`; apply retry/user policy using expected Artifact and cost evidence. |
| Retry requested | New attempt ID, same logical operation/idempotency policy, unique scratch/output path; accepted prior Artifacts are immutable. |

## 8. Security, privacy, and retention

- Starting a Job revalidates filesystem, process, capture, network, credential,
  upload and callback authority. Reattachment also revalidates current policy;
  a persisted handle is not a permission grant.
- Resolve executable identity and toolchain provenance before execution.
  Rollshot's FFmpeg pin/hash/license/version validation is useful evidence, but
  environment overrides and PATH still require current trust policy [E:R6].
- Local children use explicit cwd/env/stdin policy, bounded output, process-tree
  termination where needed, and no-follow/containment protections for files.
  Killing only the immediate PID may leave grandchildren; Rollshot's current
  import tests establish its direct fixture process, not arbitrary process-tree
  containment [G:R-PROCESS].
- Preview defaults should bind loopback and avoid exposing CDP or a user's main
  browser profile. Hyperframes' explicit browser/profile/CDP validation is
  useful narrow evidence [E:H1].
- Remote handles, upload URLs, callback URLs, logs and cost details can leak
  project/user identity. Store redacted provider references and keep credentials
  in the provider authority boundary.
- Retention is per class: transient progress, bounded logs, Job receipts,
  partial scratch, accepted Artifacts and remote-provider data have different
  expiry/deletion rules. Deleting a conversation does not delete a remote Job
  or media Artifact.

## 9. Candidate Rollshot patterns without final selection

### Pattern A — live host operation registry

A process-local registry owns `JobId`, kind, owner, status, cancellation,
structured progress, bounded log reference, child/service handles and terminal
result until a short retention timer expires. It supervises local media
operations and preview services; shutdown cancels or deliberately detaches only
declared kinds. App messages carry Job ID/version so stale updates are ignored.

**Fit:** extends the behavioral shape demonstrated by `ImportCoordinator` and
oh-my-pi's registry without claiming restart recovery. **Costs:** lifecycle/UI
surface, process-tree policy, shutdown ordering, bounded retention, and tests.
It cannot reattach after app death and must say so explicitly.

### Pattern B — durable external Job receipt and reconciliation

Before a chargeable/remote start, persist a small Rollshot-owned receipt:
logical operation/attempt, provider authority, idempotency key, remote handle,
expected Artifacts, last cursor/status/cost, cancellation intent and retention
metadata. On resume, query the authoritative provider, collect/validate output,
and route to running, terminal, lost, or needs-reconciliation. Agent transcripts
remain optional diagnostics.

**Fit:** combines the narrow virtues of Claude's identity sidecar, Codex's
cursor replay, and Hyperframes' remote render handles. **Costs:** durable schema,
atomic start ambiguity handling, provider adapters, credentials, migrations,
remote deletion and reconciliation tests. It is unnecessary for local-only
Smart Redaction.

### Pattern C — product-owned media operation with Artifact truth

Action Guide or a future media product owns a domain operation record: input
revision, pass/attempt, toolchain fingerprint, cancellation intent, scratch/
expected Artifact references, validation receipt and terminal. The shared agent
foundation receives only bounded start/observe/cancel/collect Tools; no general
Job platform is added. Restart either scavenges local work and restarts a safe
attempt, or reattaches only to an explicitly supported remote provider.

**Fit:** keeps media privacy, progress and Artifact rules close to the product.
**Costs:** less reuse if several products converge on the same lifecycle and
observability needs. It does not turn `managed_ffmpeg` or the current coordinator
into a general scheduler.

### 9.1 Per-pattern lifecycle semantics (comparison gate)

These are candidate semantics for comparison, not selected requirements.

| Pattern | Owner | Admission / concurrency | Observe | Completion | Cancel | Failure | Retry | Artifact collect / validate / partial handling |
|---|---|---|---|---|---|---|---|---|
| **A: live registry** | App host registry owns identity, child/service handles and short terminal retention; product owns accepted Artifacts. | Revalidate authority and enforce per-kind live slots/resource caps before spawn; reject admission during shutdown. | Read Job ID/version, live status, structured progress and bounded log tail; reconnect only within the same app process. | Child exit plus output validation, or confirmed service stop, produces one retained terminal; owner death is not completion. | Record in-memory intent, signal the child/service, escalate under declared policy, reap, then report confirmed/unknown; intent is lost with the host. | Typed spawn/exit/health/resource failure; app death yields unsupported reattach/orphan reconciliation rather than fabricated `failed`. | New attempt ID after a terminal or known-no-start result; never retry an ambiguous external effect and never adopt a bare PID. | Collect staged output once, validate, then atomically publish; quarantine/delete partial scratch. A preview service normally has no output Artifact. |
| **B: durable remote receipt** | Rollshot receipt store owns intent/cursors/collection; provider owns execution status; product owns Artifact acceptance. | Persist intent/idempotency key first, then enforce provider concurrency, cost and current credential/egress authority. | Query by provider handle/key and cursor; callbacks are hints. Return authoritative status, progress/log references, cost and warnings. | Provider terminal plus a durable local collect receipt and validated expected Artifact; notification alone is insufficient. | Persist intent, request provider cancellation, query until confirmed/already-terminal/not-found/unknown, and retain the outcome. | Represent `start_unknown`, provider failure, `lost`, `expired` and collection failure distinctly; reconcile rather than infer. | Resolve ambiguity with the same key; create a new attempt only under provider/product retry policy and visible prior cost. | Download to temporary storage, verify hash/decode/schema, atomically publish and record collection; quarantine/delete partial local downloads and apply remote retention policy. |
| **C: product media operation** | Action Guide/future media domain owns operation/revision/passes and Artifact truth; a local-process or remote adapter owns execution handles. | Apply product-specific input revision, privacy, CPU/disk/process/provider and attempt limits before starting a pass. | Expose domain pass/processed/total/candidate state, bounded diagnostics and Artifact revision; stale operation/attempt updates are ignored. | Product terminal occurs only after required passes finish and the domain Artifact validates/publishes; process exit alone is insufficient. | Persist intent only when recovery is promised; fan out to all active children/provider, reap/query, and expose confirmed/unknown. | Typed preflight, pass, validation, orphan/lost and resource failures stay on the domain operation with no false success. | New pass/operation attempt with unique scratch; reuse only immutable accepted inputs and provider keys permitted by domain policy. | Validate/decode/hash staged media, publish marker-last/atomically, keep accepted Artifacts immutable, and quarantine/delete incomplete attempts. |

No pattern is selected. Patterns A and B are materially different (live-only
versus durable/authoritative recovery); Pattern C is a different ownership
choice that could use either lifecycle internally.

## 10. Non-goals and preliminary fit

This comparison does not:

- convert every foreground Tool call into a Job;
- extend Smart Redaction wall time merely so a model can poll;
- equate a Job with a Child Agent, Todo, Workflow item, terminal session, PID,
  thread, or iced `Task`;
- require process restart recovery for local FFmpeg import;
- promise exactly-once external effects without provider deduplication;
- persist raw video, screenshots, prompts or logs by default;
- design a generic distributed Workflow engine or universal Artifact store;
- refactor `ImportCoordinator`, `managed_ffmpeg`, or Hyperframes; or
- select a provider, storage backend, retention period, or final pattern.

| Pattern | Smart Redaction | Action Guide | Deferred brag + Hyperframes |
|---|---|---|---|
| **A: live registry** | More machinery than the current trace proves. | Strong live semantic match for import/preview-like operations; restart value remains unproven. | Fits local preview/render supervision, not durable cloud recovery. |
| **B: durable remote receipt** | Unjustified unless a future Tool starts remote work. | Useful only if imports/analysis become remote and chargeable. | Strong remote semantic match if the deferred workload becomes real. |
| **C: product media operation** | Unnecessary. | Natural ownership candidate; current coordinator already supplies part of the live shape. | Fits a domain-specific media feature but may duplicate shared Job plumbing. |

## 11. Measurable evaluation criteria

| Dimension | Required measure / pass criterion |
|---|---|
| **Lifecycle coverage** | For each Job kind, tests exercise acknowledged and ambiguous Start, Observe progress, Cancel, Collect, Cleanup and Reattach/explicitly-unsupported. Every transition produces one typed outcome. |
| **Run/Job separation** | Agent Run returns a handle without polling turns; external Job continues after the initiating turn. Zero Job minutes are charged as model wall time except explicit bounded wait calls. |
| **Progress truth** | Drop/duplicate/reorder every transient progress event; reconstructed status and terminal remain correct. Cursor replay has no silent gap; UI reconnect p50/p95 is measured. |
| **Cancellation** | Local direct-child fixtures are reaped within 2 s (current import test threshold); process-tree fixtures meet a separately declared SLA. Remote cancel outcomes distinguish confirmed/already-terminal/not-found/unknown. Zero accepted Artifact after confirmed cancel. |
| **Crash/reattach** | Crash before/after start acknowledgement, terminal observation and collection. Remote cases yield the same Job with zero duplicate chargeable starts; local unsupported cases become orphaned/lost and are safely scavenged. |
| **Idempotency** | Deliver start/cancel/collect commands at least twice. Zero duplicate remote render, document apply, final Artifact publish or completion notification. |
| **Artifact integrity** | 100% terminal-success references exist, decode/hash/schema validate and were atomically published; partial/truncated files never satisfy Collect. |
| **Resource bounds** | Measure peak child count, CPU, memory, scratch/disk, log bytes and queue depth at concurrency 1/2/4. No leaked child, pipe, port, temp directory or browser tree after terminal/cleanup. |
| **Cost** | Remote actual/estimated cost is visible before retry and at terminal; ambiguous starts do not double charge. Compare inline wait, background local, and remote p50/p95 latency plus total compute/currency. |
| **Retention/deletion** | At policy expiry, Job receipt/log/scratch/remote output derivatives are deleted or tombstoned within SLA; accepted product Artifacts follow their own policy. |
| **Security/privacy** | Current authority is revalidated at start and reattach; denied/expired grants fail closed. Default logs contain zero raw screenshot/video bytes, credentials, callback secrets, or full source paths. |

## 12. Evidence gaps and bounded spikes

1. Runtime-test current video-import cancellation, process-tree behavior,
   progress-channel loss, worker panic, scratch cleanup and both Linux/macOS UI
   paths. Static tests establish intent, not production behavior.
2. Spike a fake remote render API with query-by-idempotency-key. Crash before
   and after acknowledgement, terminal and Collect; prove zero duplicate starts
   and deterministic `start_unknown` reconciliation.
3. Supervise one preview fixture: port collision, early exit, health failure,
   app shutdown and stale orphan marker. Measure whether a live registry adds
   user value over simple restart.
4. Exercise managed FFmpeg setup twice concurrently and inject process death
   after archive verification, during live unpack, during validation, and
   before/while manifest write. Inventory live binaries, manifest and scratch
   after restart; this bounds [A:R-MANAGED-INSTALL] without presupposing a bug.
5. If a durable pattern remains plausible, compare one receipt Snapshot against
   an append journal for write atomicity, migration, privacy deletion and
   p50/p95 resume. Do not introduce Workflow dependencies in this spike.
6. Runtime-test OMP process death, Codex reconnect at/gap/beyond TTL, Claude
   remote sidecar auth/404/archive paths, and Hyperframes remote cancellation
   only if a Rollshot pattern depends on those behaviors.

## 13. Exact negative audits and graph limitations

The code-review graph was queried first. It covered `managed_ffmpeg.rs` but was
stale (548 indexed lines versus 925 at current HEAD) and returned zero nodes for
the current `video_import.rs` and Rollshot video-import process file. Pi,
oh-my-pi, Codex, Claude Code and Hyperframes reference roots each returned zero
communities/nodes, so bounded shell inspection followed.

- **[A:R-FOUNDATION] Rollshot agent Job boundary.** Reused Round 0/Task 7
  literal roots `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`;
  declarations/terms `Task|Todo|Workflow|Job|job[_ -]?id|process handle|reattach|idempotenc`.
  No foundation Job domain/lifecycle record was found in that scope.
- **[A:R-JOB] Rollshot import Job durability/cost audit.** Literal files
  `rollshot-app/src/action_guide_home/{video_import,update}.rs`,
  `rollshot-app/src/managed_ffmpeg.rs`, and
  `rollshot-action/src/video_import/{mod,process}.rs`; regex
  `Job|job_id|job id|reattach|rehydrate|resume|idempotenc|retention|cost|log cursor|observation cursor|process handle|persist|serialize`.
  Hits were only managed-manifest serde/persistence, recent-project tests, and
  a privacy test for persisted/exported Artifacts. A running import Job handle,
  durable status/cursor/cost/log/retention/idempotency/reattach contract was
  **not found in the investigated scope**.
- **[A:R-MANAGED] managed toolchain audit.** Literal file
  `crates/rollshot-app/src/managed_ffmpeg.rs`; direct reading of all production
  functions and the [A:R-JOB] terms. Positive fields/functions cover metadata,
  binary paths, resolution, validation, archive-in-scratch download/hash,
  live-bin unpack and manifest. A running operation lifecycle was **not found
  in this scope**.
- **[A:R-MANAGED-INSTALL] managed install publication/concurrency audit.**
  Literal production functions `download_managed_ffmpeg`, `write_manifest`,
  `ScratchDir::{new,drop}`, managed path helpers, and their app call sites;
  direct control-flow reading plus terms
  `lock|mutex|flock|create_new|rename|atomic|staging|partial|recover|scavenge`.
  Positive source puts the archive in per-call scratch, unpacks directly to
  shared `root/bin`, validates live binaries, writes the final manifest last,
  and removes binaries on handled error paths. Atomic whole-install
  publication, inter-setup locking, and cleanup/reconciliation after process
  death were **not found in the investigated production scope**; test-only
  `ENV_LOCK` guards environment-variable tests, not product setup.
- **[A:P-JOB] Pi process/Job audit.** Literal example
  `packages/coding-agent/examples/extensions/subagent/{index,agents}.ts` and
  `README.md`, plus `docs/extensions.md`; terms
  `job id|process handle|reattach|rehydrate|persist|serialize|idempotenc|retention|cost|progress|log|cancel|abort|SIGTERM|SIGKILL|spawn`.
  Hits establish spawn, stream progress/usage/cost and the exact abort handler:
  call `SIGTERM`, then after five seconds attempt `SIGKILL` only when
  `!proc.killed`. Termination/escalation reliability was not runtime-verified. No
  returned child handle, built-in Job registry, durable lifecycle, Artifact
  contract, idempotency key, retention, or restart reattachment was found.
- **[A:O-JOB] oh-my-pi Job durability/completion audit.** Literal
  `packages/coding-agent/src/async/job-manager.ts` and `src/tools/hub/`;
  direct full-file reading plus regex
  `serialize|deserialize|rehydrate|reattach|persist|session.?entry|from.?json|to.?json|idempotenc|expected.?artifact|cost`.
  Positive hits/fields establish the live manager and Hub. Durable Job
  serialization/reattachment, idempotency, typed Artifact completion, and
  provider cost were **not found in the investigated scope**.
- **[A:C-TERMINAL] Codex background-terminal audit.** Literal
  `core/src/unified_exec/{process,process_manager}.rs`,
  `core/src/codex_thread.rs`, `thread-store/src`, and rollout reconstruction;
  fields/terms `BackgroundTerminalInfo|process_id|list_processes|terminate_process|progress|cost|artifact|idempotenc|retention|reattach`.
  Positive hits establish live list/terminate. A durable terminal Job record,
  Thread reconstruction of handles, structured Job progress/cost/Artifact/
  idempotency/retention was **not found in this scope**.
- **[A:C-RECOVERY] Codex exec recovery boundary.** Literal
  `exec-server/src/{server/session_registry,client_recovery,local_process}.rs`,
  exec protocol and ThreadStore reconstruction; direct reading of
  `resume_session_id`, `after_seq`, TTL, retained-output caps and shutdown.
  Server-restart process serialization, recovery after TTL, idempotent start,
  typed Artifact progress/collection and cost were **not found in this scope**.
- **[A:L-LOCAL] Claude local Runtime Task audit.** Literal `src/Task.ts`,
  `src/tasks/LocalShellTask`, `src/tasks/LocalAgentTask`, `src/utils/task`,
  `sessionStorage.ts` and `sessionRestore.ts`; Reviewed-profile regex
  `(?:restore|resume)[A-Za-z]*(?:Task|Agent)|reattach|resurrect|sidecar` plus
  `cost|artifact|idempotenc|retention`. Explicit local-agent context resume was
  separate; generic local shell/Runtime Task resurrection, a common cost/
  Artifact/idempotency/retention contract was **not found in this scope**.
- **[A:L-REMOTE] Claude remote Job audit.** Literal
  `src/tasks/RemoteAgentTask/RemoteAgentTask.tsx`, remote helpers and remote
  sidecar functions in `sessionStorage.ts`; direct reading of registration,
  restore, poll, kill/archive and terminal removal plus terms
  `cost|idempotenc|artifact|retention|delete|archive|restore|status`.
  Positive identity/status recovery is [E:L2]. A provider-neutral schema,
  generic cost/idempotency/Artifact validation and normative retention/deletion
  contract was **not found in the external-source scope**.
- **[A:H-LOCAL] Hyperframes local process audit.** Literal
  `skills/hyperframes-cli/{SKILL.md,references/preview-render.md}` and
  `packages/cli/src/{commands/render.ts,ui/progress.ts}`; terms
  `job|handle|cancel|abort|cleanup|retention|reattach|resume|idempotenc|cost|progress|orphan`.
  Positive hits establish a long-running preview, awaited Producer Job,
  progress and orphan-browser scan. A CLI-stable local handle, later attach,
  common cancel/cleanup/retention/idempotency/cost contract was **not found**.
- **[A:H-REMOTE] Hyperframes remote lifecycle audit.** Literal CLI roots
  `commands/cloud/render.ts`, `commands/lambda/{render,progress}.ts`,
  `commands/cloudrun.ts`, plus AWS/GCP `renderTo*` and `getRenderProgress` SDK
  files; regex `cancel|abort|terminate|delete.?render|cleanup|retention|expires|reattach|rehydrate`.
  Hits were terminal/cancelled status observations and comments that runtime
  config has no `abortSignal`; no common cancel command, cleanup/retention or
  provider-neutral reattach record was found. Hosted cloud's explicit
  idempotency key remains positive evidence [E:H3].

## 14. Evidence index and limitations

- **[E:R1] Rollshot source/test source:** Round 0 `AgentRunner`, `RunBudget`,
  `RunCancellation`, typed terminals and serial Tool registry. Supports one
  bounded Agent Run; no live provider run.
- **[E:R2] Rollshot source/test source:**
  `rollshot-app/src/action_guide_home/video_import.rs` — coordinator state,
  operation identity, progress, cancellation, stale event and privacy tests.
- **[E:R3] Rollshot source/test source:** shared app
  `action_guide_home/update.rs::run_import_task`; Linux/macOS product effect
  handlers. Supports UI ownership/channel/terminal routing; UI not launched.
- **[E:R4] Rollshot source/test source:**
  `rollshot-action/src/video_import/process.rs` — `CancellableChild`,
  `run_cancellable_child`, FFmpeg passes, bounded stderr and cancellation/drop/
  stalled decoder tests. Tests inspected, not run.
- **[E:R5] Rollshot source/test source:**
  `rollshot-action/src/video_import/{mod,scratch}.rs` — pass progress, resource
  limits, staged extraction, scratch and fault/cancellation cleanup tests.
- **[E:R6] Rollshot source/test source:** `rollshot-app/src/managed_ffmpeg.rs`
  — pinned metadata, resolution, validation, archive-in-scratch download/hash,
  direct-to-live-bin unpack, handled-error binary removal, manifest-last write,
  manifest versions and scratch cleanup. Download/setup not executed; atomic
  publication, concurrent setup and process-crash cleanup remain unestablished
  [A:R-MANAGED-INSTALL].
- **[E:S1] Capability evidence:** reviewed
  `subagents-and-parallelism.md` and system profiles. Supports separate Child
  Agent context/authority/budget/completion semantics.
- **[E:P1] Pi example source:** `examples/extensions/subagent/index.ts` and
  README. Example-only, uninstalled; subprocess signals were not runtime-tested.
- **[E:P2] Pi repository docs:** `docs/extensions.md` long-lived resources and
  `session_shutdown`. Policy contract, not crash proof.
- **[E:O1] oh-my-pi source:** `src/async/job-manager.ts` and Hub Job tools.
  Supports process-local lifecycle/caps/delivery/retention; no process-death run.
- **[E:C1] Codex source/test source:** unified exec process manager and
  `BackgroundTerminalInfo`. Supports live list/terminate only.
- **[E:C2] Codex source/test source:** exec-server session registry, client
  recovery, protocol, local-process replay and focused recovery tests. Tests
  inspected, not run; deployed disconnect timing remains unknown [G:C1].
- **[E:L1] Claude source:** `src/Task.ts`, local Runtime Task/framework and
  disk output. External source only; runtime not exercised.
- **[E:L2] Claude source, gated path:** `RemoteAgentTask.tsx` and remote-agent
  metadata sidecars in `sessionStorage.ts`. Remote launch/service not exercised.
- **[E:H1] Hyperframes workflow/CLI source:** `review-loop.md` and
  `hyperframes-cli/references/preview-render.md` preview semantics.
- **[E:H2] Hyperframes CLI/Producer source:** `commands/render.ts` and
  `ui/progress.ts`; local Producer execution and Artifact commit checkpoint.
- **[E:H3] Hyperframes hosted CLI source:** `commands/cloud/render.ts` —
  idempotency key, render ID, callback/poll/get, timeout recovery and download.
- **[E:H4] Hyperframes AWS source:** Lambda render/progress CLI and SDK — handle,
  status, frames, cost/errors/output. Cloud execution not performed.
- **[E:H5] Hyperframes GCP source:** Cloud Run render/progress CLI and SDK —
  execution handle, status, frames, cost/errors/output. Cloud execution not
  performed.

**Limitations:** confidence is high for visible pinned fields, call order,
status sets, caps and exact bounded audits; medium for source plus tests that
were not run; and low-to-medium for process trees, power/crash behavior,
deployed remote retention/cost/cancellation, server-side gates and cross-platform
runtime behavior. A missing search result means only “not found in the named
scope.” The Rollshot graph lag means current video-import claims rely on direct
source inspection. Hyperframes is workload/reference evidence, not Rollshot
product behavior.

Open synthesis questions are whether any adopted workload needs recovery beyond
a live app process, which product owns Job receipts and retention, whether a
preview/media-only registry earns its complexity, and whether remote provider
contracts are stable enough for Pattern B.
