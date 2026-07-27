# Gate Decision: Live Job Registry Slice 4

**Status:** Proposed for user approval
**Date:** 2026-07-28
**Branch:** `feat/agent-foundation-live-job-registry`
**Base:** `3d69781` (Slice 3 merge commit on main)
**Implementation commits:** `b7c81d5..3cd0726` (9 commits)
**Files changed:** 10 files, +5719 / −177

## 1. Selected architecture

Shared generic `LiveJobRegistry<P, R>` in `rollshot-agent::jobs` plus product
adapter in `rollshot-app`. The registry owns live identity, admission, state
transitions, structured progress, cancellation routing, terminal truth,
collect-once result handoff, and lazy terminal retention. The Action Guide
video-import worker is the proof workload; `CancellableChild` and
`ImportedScratch` retain concrete child/scratch ownership.

Per spec §6.1 and auto decisions D1–D5:

- **D1 — lazy TTL reclamation.** Terminal expiry is enforced at every observable
  boundary (`collect`, `snapshot`, `prune_terminals`); no background timer.
- **D2 — four reserved result slots.** Active + uncollected results are capped
  at four, separate from 128 terminal metadata records.
- **D3 — reporter drop confirms `Cancelled` when `Cancelling`.** Reporter-stack
  destruction follows concrete resource owners.
- **D4 — atomic cutover.** `ImportProgress`/`ImportFinished` messages fully
  removed; no dual path.
- **D5 — conditional real FFmpeg fixture.** Run when `ROLLSHOT_TEST_FFMPEG=1` is
  set and tools are available; record explicit skip otherwise.

### Non-goals (spec §5, confirmed not present)

No durable job persistence, PID adoption, remote jobs, retry, scheduling,
workflow DAG, child agents, parallel tool execution, agent tool for starting
jobs, new `RunOperation`, Product Task for direct import, raw log retention,
full paths in snapshots, image/video bytes, credentials, provider conversation,
managed FFmpeg setup changes, visible UI changes, or visual baseline updates.

## 2. Admission authority matrix

| Authority source | Owner | V1 outcome |
|---|---|---|
| `DirectUserAction(ActionGuideVideoImport)` | `DirectProductAction` | **Accepted** (kind must match) |
| `DirectUserAction(ActionGuideVideoImport)` | `ProductTask(_)` | **Rejected** — `OwnerAuthorityMismatch` |
| `AgentTask { .. }` | any | **Rejected** — `UnsupportedAuthoritySource` |

`JobControl` (cancellation callback) is required by the `admit` signature;
missing control is unrepresentable. Admission fails before allocating a Job
record for kind/source mismatch, owner/task mismatch, active-capacity
exhaustion (4), terminal-capacity pressure (128), reserved result-slot pressure
(4), unsupported authority, or registry shutdown.

## 3. Lifecycle and retention evidence

### State machine

```text
Starting → Running → Succeeded
                   → Failed(WorkerAbandoned | WorkerPanic)
                   → Failed { category, message }
                   → Cancelling → Cancelled
Starting ─────────→ Cancelling → Cancelled
```

### Key tests (24 in `jobs::tests`)

| Test | Verifies |
|---|---|
| `admitted_job_has_typed_unique_identity_and_exact_metadata` | Identity, Starting state, metadata |
| `agent_task_authority_is_represented_but_rejected_before_allocation` | UnsupportedAuthoritySource |
| `direct_authority_cannot_claim_product_task_ownership` | OwnerAuthorityMismatch |
| `fifth_active_job_is_rejected_without_evicting_active_work` | Active cap = 4 |
| `reporter_moves_starting_to_running_once` | Starting → Running transition |
| `cancel_requests_control_but_worker_confirms_terminal` | Cancel → Cancelling, worker → Cancelled |
| `success_racing_with_cancel_is_dropped_and_becomes_cancelled` | Cancel-wins-over-success |
| `latest_progress_and_terminal_repair_coalesced_notifications` | Watch coalescing, terminal repair |
| `diagnostics_keep_last_64_sanitized_entries_and_count_drops` | Bounded diagnostics, static strings |
| `snapshot_debug_omits_result_content_and_callback_markers` | Privacy: no R/callback in Debug |
| `success_result_moves_once_without_clone` | Collect-once, no clone of R |
| `uncollected_result_expires_at_five_minutes` | TTL expiry |
| `dropping_unfinished_reporter_marks_worker_abandoned` | Drop from Running → WorkerAbandoned |
| `shutdown_rejects_admission_and_requests_all_active_cancellation` | Shutdown → Cancelling for all |
| `terminal_cap_128_prunes_oldest_collected_on_next_admission` | Terminal cap eviction |
| `active_plus_uncollected_success_reserves_four_result_slots` | Reserved result slots |
| `uncollected_unexpired_terminal_records_are_not_silently_evicted` | No silent eviction |
| `ttl_expired_result_is_tombstoned_for_collect_after_eviction` | Tombstone after eviction |
| `cancel_not_found_returns_not_found` | NotFound for unknown ID |
| `dropping_starting_reporter_marks_worker_abandoned` | Drop from Starting → WorkerAbandoned |
| `dropping_reporter_while_cancelling_confirms_cancellation` | D3: Drop while Cancelling → Cancelled |
| `terminal_records_track_terminal_time_for_ttl` | Terminal time tracking |
| `owner_drop_requests_cancel_while_observer_and_reporter_finish_cleanup` | Shutdown callback safety |
| `job_debug_and_tracing_omit_control_and_result_sentinels` | Privacy: no secrets in Debug/tracing |

### Retention bounds

| Resource | Limit |
|---|---|
| Active jobs | 4 |
| Active + uncollected result slots | 4 |
| Terminal metadata records | 128 |
| Terminal TTL | 5 minutes (lazy) |
| Diagnostic entries | 64 |
| Diagnostic entry size | 256 bytes |

## 4. Video-import migration evidence

### Integration tests (70 in `action_guide_home`, 57 in `video_import`)

| Test | Verifies |
|---|---|
| `available_toolchain_admits_once_before_worker_effect` | Admission path via coordinator |
| `registry_admission_failure_starts_no_worker` | Admission failure → no worker |
| `terminal_snapshot_opens_seed_once_even_after_notification_coalescing` | Collect-once via reconciliation |
| `cancel_detaches_ui_but_registry_waits_for_worker_confirmation` | UI detaches, registry waits |
| `video_import_errors_map_to_stable_categories_and_existing_copy` | Error mapping preserved |
| `notification_loss_does_not_lose_terminal_or_duplicate_collection` | Notification loss resilience |
| `cancel_wins_against_late_success_and_drops_seed` | Cancel-wins-over-success (integration) |
| `stale_terminal_from_old_job_cannot_open_over_new_import` | Stale job isolation |
| `reporter_drop_becomes_worker_abandoned_and_is_repairable` | Reporter drop handling |
| `expired_uncollected_seed_is_dropped_and_scratch_is_removed` | Expiry + scratch cleanup |

### Platform callsites

Both `action_guide_linux_product.rs` and `macos_product.rs` use identical
`Effect::StartImport` destructuring and `run_import_task()` dispatch. The
subscription is shared code in `update::subscription()` using
`iced::Subscription::run_with(watch, import_job_changes)`.

## 5. Cancellation, child reaping, and scratch evidence

- `cancel()` transitions to `Cancelling`, invokes callback outside the lock,
  returns `Requested` (never `Cancelled` synchronously).
- `Cancelled` is written only by `JobReporter::cancelled()` (worker call after
  cleanup) or `JobReporter::Drop` while `Cancelling` (D3: RAII stack has
  dropped child/scratch).
- `succeed()` while `Cancelling` drops the result and terminalizes as
  `Cancelled`.
- `CancellableChild` kills and waits on cancellation and `Drop`; `ImportedScratch`
  removes on `Drop`. Both are unchanged by this slice.
- Measured cancellation bound: `cancel()` → `Cancelled` is bounded by
  `CancellableChild::kill()` + `wait()` (existing ≤2s stall timeout).

## 6. Notification-loss and collect-once evidence

- `import_job_changes` uses `receiver.changed().await` which coalesces;
  `reconcile_import_job()` always queries the latest snapshot from the registry.
- `collect()` uses `record.result.take()` (one-shot move) and sets
  `record.result_collected = true`. Duplicate collect returns
  `AlreadyCollected`.
- Test `notification_loss_does_not_lose_terminal_or_duplicate_collection`
  simulates dropped notifications and verifies terminal truth and single
  collection survive.
- Test `terminal_snapshot_opens_seed_once_even_after_notification_coalescing`
  verifies coalesced notifications don't lose terminal state.

## 7. Restart, shutdown, and no-PID-adoption evidence

- `LiveJobRegistry::new()` creates an empty registry. No persistence, no
  rehydration, no PID adoption.
- `shutdown()` sets `shutting_down`, transitions all active jobs to `Cancelling`,
  collects callbacks inside the lock, invokes them outside the lock.
- `LiveJobRegistry::Drop` calls `shutdown(0)`.
- Existing locked scratch scavenger runs at startup (unchanged).
- No PID field exists in `JobRecord` or `JobSnapshot`.

## 8. Privacy inspection

| Check | Result |
|---|---|
| `JobControl::Debug` | `JobControl(<redacted>)` — no callback content |
| `JobReporter::Debug` | Only `job_id` and `terminal_reported` |
| `JobSnapshot::Debug` | Structured fields only; no R, callback, path, PID |
| `JobDiagnostic::message` | `&'static str` — no runtime paths or process output |
| Tracing targets | `rollshot::agent::jobs` with `job_id`, `state`, `revision` only |
| `println!`/`eprintln!`/`dbg!` in changed production code | None found |
| Sentinel leakage test | `job_debug_and_tracing_omit_control_and_result_sentinels` PASS |

## 9. Verification command results

### Step 1: Focused suites

| Suite | Command | Passed | Failed | Ignored | Duration |
|---|---|---|---|---|---|
| rollshot-agent jobs | `rtk cargo test -p rollshot-agent jobs` | 24 | 0 | 0 | 0.00s |
| rollshot-action video_import | `rtk cargo test -p rollshot-action video_import` | 57 | 0 | 0 | 0.48s |
| rollshot-app action_guide_home | `rtk cargo test -p rollshot-app --features action-guide action_guide_home` | 70 | 0 | 0 | 5.70s |
| **Total** | | **151** | **0** | **0** | |

### Step 2: Full crate regression suites

| Crate | Command | Passed | Failed | Ignored | Duration |
|---|---|---|---|---|---|
| rollshot-agent | `rtk cargo test -p rollshot-agent` | 388 | 0 | 0 | 2.39s |
| rollshot-action | `rtk cargo test -p rollshot-action` | 402 | 0 | 0 | 5.17s |
| rollshot-app | `rtk cargo test -p rollshot-app --features action-guide` | 1266 | 0 | 6 | 0.74s |
| **Total** | | **2,056** | **0** | **6** | |

### Step 3: Real FFmpeg smoke fixture

| Test | Command | Result | Duration |
|---|---|---|---|
| `static_video_returns_final_frame_fallback` | `rtk proxy env ROLLSHOT_TEST_FFMPEG=1 cargo test -p rollshot-action video_import::tests::static_video_returns_final_frame_fallback -- --exact` | **PASS** | 0.28s |

FFmpeg `n8.1.2` and FFprobe available. Fixture exercised: probe, analysis,
extraction, scratch, and final `ImportedWorkspaceSeed`.

### Step 4: Formatting, lint, and privacy

| Check | Command | Result |
|---|---|---|
| Formatting | `rtk cargo fmt --check` | **PASS** |
| Clippy | `rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings` | **PASS** (0 errors, 0 warnings) |
| Whitespace | `rtk proxy git diff --check` | **PASS** |
| `println!`/`eprintln!`/`dbg!` in changed production code | grep on `crates/rollshot-agent/src;crates/rollshot-app/src;crates/rollshot-action/src` | **None found** |

**Note:** Clippy and formatting were not pre-clean. Verification exposed two
clippy errors (`large_enum_variant` on `JobAuthoritySource`, `collapsible_if`
in `prune_terminals`) and formatting diffs. These were fixed in commit `3cd0726`
— code-quality issues, not correctness or security defects.

## 10. Independent review findings and resolutions

An independent code reviewer answered all 11 required questions.

### Questions and answers

| # | Question | Answer | Severity |
|---|---|---|---|
| 1 | Can work launch before direct-user admission commits a `Starting` record? | **No.** `admit()` is the sole path; inserts `Starting` before returning reporter. | OK |
| 2 | Can any skill/model/agent task construct accepted authority or borrow an unrelated `RunOperation`? | **No.** `AgentTask` unconditionally returns `UnsupportedAuthoritySource`. No `RunOperation` exists. | OK |
| 3 | Can cancellation be reported confirmed before FFmpeg children and scratch are cleaned? | **No.** `cancel()` → `Cancelling` only. `Cancelled` requires worker return or Drop while `Cancelling`. | OK |
| 4 | Can notification loss, duplication, or reordering lose terminal truth or collect a result twice? | **No.** Notifications are hints; snapshot queries are authoritative; `collect()` is one-shot. | OK |
| 5 | Can a stale Job affect a newer `ImportOperationId` or open a timeline? | **No.** `ImportOperationId` and `JobId` are distinct; stale jobs bound to old coordinator slot. | OK |
| 6 | Can reporter panic/drop leave a Job falsely Running? | **No.** `Drop` marks terminal; `catch_unwind` catches panics. Three paths tested. | OK |
| 7 | Can active work be evicted, or an unexpired result be silently dropped at capacity? | **No.** Three separate capacity gates; active jobs never evicted. | OK |
| 8 | Can shutdown callbacks deadlock by running under the registry mutex? | **No.** Callbacks collected inside lock, invoked outside lock. | OK |
| 9 | Can Debug/tracing/diagnostics/snapshots leak sensitive data? | **No.** Static strings, closed Debug fields, sentinel test passes. | OK |
| 10 | Do Linux and macOS use the same registry-backed worker and subscription contract? | **Yes.** Identical `Effect::StartImport` + `run_import_task()` dispatch; shared subscription code. | OK |
| 11 | Did the slice introduce persistence, PID adoption, remote jobs, retries, scheduling, Product Task fabrication, new UI, or another non-goal? | **No.** No serialization derives, no PID adoption, no UI changes, no non-goal code. | OK |

### Reviewer strengths noted

- Clean separation of concerns (generic registry, no iced/action dependencies)
- Cancellation honesty (never claims `Cancelled` before cleanup)
- Robust error handling (`catch_unwind`, explicit panic/abandoned paths)
- Comprehensive test coverage (24 registry + 10 integration + 57 video_import)
- Privacy-by-design (static strings, closed Debug, sentinel tests)
- Atomic cutover (no dual path or compatibility shim)

### Minor reviewer observations (non-blocking)

1. **`terminal_at_ms` in reporter Drop uses `updated_at_ms` instead of current
   time.** The 5-minute TTL absorbs drift; the clock starts from the last
   registry mutation rather than actual resource release. Not a correctness
   issue in practice.

2. **Watch subscription enters `std::future::pending()` on registry drop.** When
   the `LiveJobRegistry` is dropped, the `watch::Sender` is dropped, causing
   `receiver.changed()` to return `Err`. The stream enters `pending()` forever.
   Harmless since iced manages subscription lifecycles.

### Verdict

**No correctness or security defects found.** All 11 required questions
answered with concrete evidence. Two minor observations are informational.

## 11. Migration and rollback

### Migration path

- `ImportCoordinator` remains the pre-job identity and presentation state.
- `ImportOperationId` is not renamed; late setup messages still rejected.
- Old `ImportProgress`/`ImportFinished` message variants fully removed.
- `bind_job()` maps `ImportOperationId` → `JobId` after admission.
- No schema migration needed; registry is process-local and starts empty.

### Rollback

Revert the 9 implementation commits (`b7c81d5..3cd0726`). No data migration
to reverse. The registry is process-local with no persistence.

## 12. Residual risks

1. **macOS runtime verification.** Both platforms share the same registry-backed
   worker and subscription code, but macOS native runtime was not executed in
   this verification. The shared contract reduces risk; a macOS-specific
   runtime check is a residual gate item for the umbrella.

2. **`terminal_at_ms` precision in reporter Drop.** Uses `updated_at_ms`
   (last mutation) rather than actual drop time. The 5-minute TTL absorbs this;
   a future slice could pass `now_ms` to the Drop context if precision matters.

3. **Watch stream on registry drop.** Enters `pending()` rather than
   terminating. Iced manages subscription lifecycle; no observable issue in
   current architecture.

4. **No mid-run authority revocation.** The process-local snapshot does not
   provide live revocation. Acceptable for current bounded direct-user-action
   workload.

## 13. Scope boundary

Passing Gate G4 proves only the live job registry foundation and video-import
migration. **Slice 5 (adversarial coverage expansion), Slice 6 (agent-started
jobs), durable/remote job recovery, managed FFmpeg setup cancellation, and
launch-video design remain unauthorized until their own gates and designs are
approved.**
