# Local Video Import to Action Guide Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let Linux and macOS users turn a supported local screen recording into a bounded, cancellable, visual-only Action Guide draft that can be reviewed, saved, reopened, and exported without retaining the original video.

**Architecture:** `rollshot-action` owns a framework-neutral two-pass FFmpeg/FFprobe importer: pass 1 drives the existing luma `Detector` at 2 fps and bounds candidate selection, while pass 2 writes at most 600 PNG evidence frames into RAII-managed scratch storage. `rollshot-app` owns executable resolution, the shared import coordinator, native picker/task wiring, and transfer of the imported seed into the existing unsaved timeline. Project and Action Guide schemas advance to version 2 so provenance and bounded warnings survive save/reopen and are disclosed by Action Guide and Issue Pack exports.

**Tech Stack:** Rust 2021, iced 0.14, `ffmpeg-sidecar`, FFmpeg/FFprobe subprocesses, `image`, `serde`/`serde_json`, existing `rustix` file locking, `tempfile` in tests, and existing `rollshot-action` detector/project/export APIs.

## Global Constraints

- Accept local `.mp4`, `.mov`, `.mkv`, and `.webm` files only; validate a readable video stream by content rather than trusting the extension.
- Fixed import constants are 2 fps analysis, 384 px analysis width, 200 generated steps, 1920 px maximum evidence long edge, and center sample plus one adjacent sample on each side.
- Every generated step is `CandidateKind::UiChanged` with `DetectReason::VisualChange`; never infer Click, Typing, Scroll, typed text, or targets.
- The original video is read-only input and is never copied, modified, deleted, persisted, uploaded, attached, logged, or included in scratch.
- Audio and subtitle processing are explicitly disabled; no transcript or audio artifact is produced.
- Memory, candidate count, and retained evidence remain bounded independently of video duration; decoder work may be linear in duration.
- All size/index arithmetic is checked. Reject an analysis frame above 64 MiB and stop evidence extraction before scratch exceeds 4 GiB with `resource_limit`; never rely on allocation failure or integer wraparound as a bound.
- Cancellation must terminate and wait for FFmpeg/FFprobe, close pipes/readers, remove scratch, and return `cancelled`; stale operation messages must be ignored.
- Runtime diagnostics use `tracing` with stable `rollshot::action::video_import` or `rollshot::app::video_import` targets and privacy-safe structured fields only.
- New projects and Action Guide sessions write schema version 2; version 1 loads with an empty warning list.
- Linux and macOS must expose the same shared coordinator and timeline behavior; platform code is limited to picker/task and product phase transitions.
- Do not add dependencies, URL import, playback, audio, transcription, tunable import settings, direct agent calls, batch import, or concurrent imports.
- `ImportedWorkspaceSeed` and `ImportedScratch` have unique ownership rather than `Clone`; stale or superseded success messages drop their seed immediately, and first save has one deterministic scratch-release point.
- Before editing iced UI code, invoke the `iced-rs` skill and use its iced 0.14 references.
- Prefix every shell command with `rtk`; do not create a worktree.

---

## File Map

**Create**

- `crates/rollshot-action/src/video_import/mod.rs` — public import request/result/progress/error API and two-pass orchestration.
- `crates/rollshot-action/src/video_import/probe.rs` — FFprobe JSON contract and privacy-safe metadata validation.
- `crates/rollshot-action/src/video_import/selection.rs` — deterministic bounded candidate and evidence-index selection.
- `crates/rollshot-action/src/video_import/process.rs` — cancellable child lifecycle, raw analysis frame reader, progress parsing, and evidence extraction.
- `crates/rollshot-action/src/video_import/scratch.rs` — unique locked scratch directories, PNG asset catalog, RAII cleanup, and stale cleanup.
- `crates/rollshot-app/src/action_guide_home/video_import.rs` — shared import coordinator state, operation IDs, worker launch, failure copy, and toolchain handoff.

**Modify**

- `crates/rollshot-action/src/models.rs`, `src/lib.rs` — imported-video provenance and public importer/warning exports.
- `crates/rollshot-action/src/project/{model.rs,store.rs,validate.rs,mod.rs}` — project schema v2 plus v1 migration.
- `crates/rollshot-action/src/step_frame_source.rs` — construct a disk-backed source from an imported catalog.
- `crates/rollshot-action/src/export/{model.rs,mod.rs,html.rs}` and `src/export/viewer.html` — session schema v2 and warning disclosure.
- `crates/rollshot-action/src/project/publish.rs` — accept migrated session manifests when publishing.
- `crates/rollshot-app/src/managed_ffmpeg.rs` — separate FFmpeg-only and FFmpeg+FFprobe resolution.
- `crates/rollshot-app/src/action_guide_home/{mod.rs,update.rs,view.rs}` — import action and processing view.
- `crates/rollshot-app/src/timeline_workspace/{mod.rs,project.rs,update.rs,guide_export.rs}` — imported constructor, persistent notice/warnings, save transfer, and scratch release.
- `crates/rollshot-app/src/timeline_workspace/view.rs` — persistent visual-only and bounded-reduction notices.
- `crates/rollshot-app/src/issue_pack.rs` — map import warnings into manifest and `issue.md`.
- `crates/rollshot-app/src/action_guide_linux_product.rs`, `src/macos_product.rs` — drive the shared coordinator on both active product paths.
- `crates/rollshot-action/examples/export_html_fixture.rs` and `scripts/html-guide-e2e/guide.spec.mjs` — browser fixture and visible-warning smoke coverage.

## Test Fixture Contracts

Keep test-only helpers beside the tests that consume them. These names in the task snippets have the following exact contracts so later implementers do not invent incompatible fixtures:

```rust
fn write_v1_project_fixture() -> tempfile::TempDir;
fn imported_snapshot(warnings: Vec<ImportWarning>) -> ProjectSnapshot;
fn load_manifest_fixture(schema_version: u32, input_source: &str) -> Result<LoadedProject, ProjectError>;
fn marker(center_id: u64, at_ms: u64) -> CandidateMarker;
fn invalid_dimensions_json() -> &'static [u8];
fn long_running_fixture_process() -> TestChildFixture;
fn fixture_process_is_alive() -> bool;
fn fixture_video(frames: &[RgbaImage], with_audio: bool) -> VideoFixture;
fn solid_frame(value: u8) -> RgbaImage;
fn settle_sequence_fixture() -> VideoFixture;
fn audio_bearing_4k_fixture() -> VideoFixture;
fn run_import(path: &Path) -> Result<ImportedWorkspaceSeed, VideoImportError>;
fn scratch_files(seed: &ImportedWorkspaceSeed) -> Vec<PathBuf>;
fn png_asset_files(seed: &ImportedWorkspaceSeed) -> Vec<PathBuf>;
fn imported_seed_fixture() -> (ImportedWorkspaceSeed, PathBuf);
fn imported_workspace_fixture() -> (TimelineWorkspace, PathBuf);
fn complete_first_save(workspace: &mut TimelineWorkspace);
fn imported_job(warnings: Vec<ImportWarning>) -> ReviewedGuideExportJob;
fn render_fixture(job: ReviewedGuideExportJob) -> tempfile::TempDir;
fn build_imported_issue_pack() -> BuiltIssuePack;
fn is_video_path(path: &Path) -> bool;
fn progress(pass: VideoImportPass) -> VideoImportProgress;
fn finish_successful_import() -> action_guide_home::Update;
fn linux_home_state() -> action_guide_linux_product::State;
fn drive_import_success<T: ImportProductHarness>(state: &mut T);
fn macos_home_product() -> macos_product::Product;
fn import_save_and_export_fixture(source_name: &str) -> PrivacyArtifacts;
fn run_fault_injected_import(fault: ImportFault) -> FaultRunResult;

enum ImportFault { ProbeFailure, Pass1Failure, Pass2Failure, Cancelled }
```

`VideoFixture` owns a temporary directory and exposes `path()`. `TestChildFixture` owns the spawned child identity used by the reap assertion. `BuiltIssuePack`, `PrivacyArtifacts`, and `FaultRunResult` are small test-only structs exposing only the fields/methods shown in the snippets. Fixture videos are created locally by the explicit FFmpeg path, use deterministic generated pixels, and never access the network.

## Engineering Review Lock (auto mode)

### Step 0: Scope Challenge

- Goal alignment: all ten tasks are necessary to reach import, review, save/reopen, export disclosure, cross-platform wiring, and privacy/cleanup completion; no task is merely nice-to-have.
- Minimum viable plan: Tasks 1-10 are the minimum approved slice because removing schema migration, scratch transfer, export disclosure, either platform adapter, or terminal cleanup would violate an explicit success criterion.
- Complexity: six net-new files, no new crate/top-level module, and ten tasks do not trigger the `>12` files / `>2` modules / `>10` tasks reduction gate.
- Search check: Rust's standard `Child` API requires the owner to wait/reap and does not reap on `Drop`; FFmpeg provides machine-readable `-progress` output and bounded filtergraph buffering; `rustix::fs::flock` provides the existing non-blocking advisory-lock primitive. The plan uses these built-ins rather than adding a process supervisor, media framework, or lock dependency.
- Completeness: the reviewed plan keeps negative paths, browser-visible disclosure, per-platform product tests, checked resource limits, and fixture-backed cancellation rather than deferring them.
- Distribution: this adds no new artifact. The existing Ubuntu/macOS CI matrix already runs Action Guide feature tests and clippy; managed-toolchain availability remains platform-dependent and is disclosed below.

### Architecture Review — 4 issues resolved

#### Auto decision D1 — Who owns FFmpeg stdout while cancellation is monitored?
Context: Task 3 originally assigned both pipes to generic reader threads while Task 4 also needed stdout as a raw frame stream.
ELI10: Two consumers cannot safely read the same pipe. If the lifecycle helper steals bytes, frames become corrupt; if nobody drains stderr, FFmpeg can block forever.
Stakes if we pick wrong: Import hangs, reports truncated frames, or leaves a decoder alive after cancel.
Recommendation: 1A because one lifecycle owner with a caller-supplied stdout consumer makes pipe ownership explicit and still guarantees kill, wait, and reader joins.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) 1A — Pass-specific stdout consumer under one child lifecycle (recommended; human: ~1 day / AI: ~30 min; low risk; low maintenance)
  ✅ Raw frames/JSON have exactly one reader and stderr/progress is drained concurrently.
  ❌ The lifecycle API is slightly more explicit than a single convenience callback.
B) 1B — Generic threads consume both pipes (human: ~4 hours / AI: ~15 min; high risk; medium maintenance)
  ✅ The helper looks compact in isolation.
  ❌ It obscures byte ownership and cannot safely support rawvideo streaming.
Net: explicit pipe ownership is the boring, diagnosable design.

#### Auto decision D2 — Should imported scratch ownership be cloneable?
Context: Tasks 4 and 8 made the seed and scratch owner cloneable even though Task 6 requires deterministic release immediately after first-save source transfer.
ELI10: Every clone is another hidden key keeping the temporary directory alive. A stale UI message could retain a key after save, so cleanup timing would depend on unrelated message lifetimes.
Stakes if we pick wrong: Saved imports can retain gigabytes of scratch until a late clone is dropped.
Recommendation: 2A because unique ownership makes stale-message drop and first-save cleanup deterministic.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) 2A — Non-`Clone` seed/scratch transferred by value (recommended; human: ~1 day / AI: ~30 min; low risk; low maintenance)
  ✅ Exactly one owner controls cleanup and stale results self-clean on drop.
  ❌ Message/effect enums carrying the seed cannot derive `Clone`.
B) 2B — Reference-counted cloneable ownership (human: ~4 hours / AI: ~15 min; medium risk; medium maintenance)
  ✅ Fewer derive changes in app messages.
  ❌ Cleanup can be delayed by invisible clones and tests become lifetime-sensitive.
Net: unique ownership trades a few derive edits for a reliable privacy/resource invariant.

#### Auto decision D3 — Where does toolchain setup live?
Context: Task 8 named setup effects but omitted resolution/download result messages, while the current managed installer extracts FFmpeg only and macOS has no pinned managed package.
ELI10: Picking a video is not enough; the home state machine must remember the pending operation while it checks, installs, retries, or cancels the two required executables. Otherwise setup can lose the selected import or accidentally start with only FFmpeg.
Stakes if we pick wrong: Setup retry loops, FFprobe is missing after a “successful” install, or Linux/macOS adapters diverge.
Recommendation: 3A because a shared home coordinator plus thin platform effect handlers preserves identical behavior and reuses the existing managed resolver/downloader.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) 3A — Explicit resolve → setup/install → retry messages in the shared coordinator (recommended; human: ~2 days / AI: ~1 hour; low risk; medium maintenance)
  ✅ One tested state machine owns retry/cancel and the managed archive installs/validates both binaries.
  ❌ Adds several explicit messages and phases.
B) 3B — Let each platform adapter resolve and improvise setup (human: ~1 day / AI: ~30 min; high risk; high maintenance)
  ✅ Fewer shared message variants initially.
  ❌ Duplicates lifecycle logic and makes parity difficult to prove.
Net: explicit shared setup state follows DRY and keeps platform code mechanical.

#### Auto decision D4 — How strict are schema-version boundaries?
Context: Adding enum variants to shared types would let a nominal v1 project/session deserialize v2-only imported provenance unless loaders validate both version and semantic combinations.
ELI10: A version number is a promise about which fields and values are legal. Accepting imported-video data under v1 can silently drop warnings and lets malformed artifacts masquerade as old compatible data.
Stakes if we pick wrong: Reopened or published guides can lose required limitations or accept unsupported future formats.
Recommendation: 4A because header-first dispatch and semantic validation make compatibility explicit.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) 4A — Accept legacy 0/1 where already supported, normalize to v2, reject unknown and v2-only values in legacy schemas (recommended; human: ~1 day / AI: ~30 min; low risk; low maintenance)
  ✅ Migration is deterministic and warnings cannot disappear behind a legacy version.
  ❌ Requires explicit project and session parser tests.
B) 4B — Rely on serde defaults alone (human: ~3 hours / AI: ~10 min; medium risk; low maintenance)
  ✅ Minimal parser code.
  ❌ Unknown versions and inconsistent legacy payloads can be accepted silently.
Net: explicit schema dispatch is small insurance at a persistence boundary.

### Plan Structure & Code Quality Review — 2 issues resolved

#### Auto decision D5 — How does reduction switch at candidate 201?
Context: Task 2 described fixed buckets but did not require replaying the first 200 candidates and used overflow-prone `u64` multiplication.
ELI10: If reduction starts only at item 201, most early events disappear except the first one. Very long timestamps can also wrap during bucket math and put events in the wrong bucket.
Stakes if we pick wrong: The “full-duration” draft is biased toward the end or non-deterministic for extreme durations.
Recommendation: 5A because replay plus checked/widened arithmetic preserves the approved deterministic coverage invariant.
Completeness: A=10/10, B=5/10.
Pros / cons:
A) 5A — Replay retained candidates into buckets and calculate in `u128` (recommended; human: ~4 hours / AI: ~20 min; low risk; low maintenance)
  ✅ Every candidate participates and multiplication cannot wrap.
  ❌ The transition has one explicit replay loop.
B) 5B — Bucket only new candidates with `u64` math (human: ~1 hour / AI: ~5 min; high risk; low maintenance)
  ✅ Less code.
  ❌ Violates beginning-to-end coverage and checked-arithmetic requirements.
Net: the complete reducer remains constant-space after one bounded 200-item replay.

#### Auto decision D6 — Should missing file declarations and verification pairs be left implicit?
Context: Timeline notices touch `view.rs`, browser disclosure needs its fixture/spec, and several implementation steps relied on later broad tests without naming their focused verification.
ELI10: Agents execute the file list and commands literally. Missing files create surprise diffs, and vague verification makes it easy to call a task green without checking its new behavior.
Stakes if we pick wrong: Tasks conflict unexpectedly or commit code that was never exercised at the intended boundary.
Recommendation: 6A because accurate declarations and explicit focused/final checks make each task independently reviewable.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) 6A — Correct file maps and name the focused Run/Expected check in every task (recommended; human: ~3 hours / AI: ~20 min; low risk; low maintenance)
  ✅ Executors know the exact diff and red/green signal.
  ❌ The plan is slightly longer.
B) 6B — Leave verification and touched files implicit (human: no extra time / AI: no extra time; medium risk; medium maintenance)
  ✅ Shorter document.
  ❌ Increases execution guesswork and hidden coupling.
Net: explicit beats clever in an agent-executed plan.

### Test Review — 1 issue resolved

#### Auto decision D7 — What closes the cross-platform and user-visible test gaps?
Context: Rust string assertions did not prove the offline reader visibly renders warnings, opt-in FFmpeg tests could silently skip, and a Linux run cannot execute the cfg-gated macOS product module.
ELI10: A warning hidden in JSON is not a warning a user can see. Likewise, a skipped fixture or uncompiled macOS module can make green local output look more complete than it is.
Stakes if we pick wrong: CI passes while the reader hides import limits or one platform path does not compile.
Recommendation: 7A because browser smoke coverage, explicit fixture skip/run signals, and the existing two-OS CI matrix cover the real boundaries.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) 7A — Add Playwright warning visibility, explicit FFmpeg fixture gating, and require both OS CI jobs (recommended; human: ~1 day / AI: ~40 min; low risk; medium maintenance)
  ✅ Tests what users see and distinguishes local platform evidence from CI evidence.
  ❌ Browser and FFmpeg fixture lanes cost extra CI/runtime time.
B) 7B — Keep Rust substring tests and current-host app tests only (human: ~3 hours / AI: ~15 min; medium risk; low maintenance)
  ✅ Faster focused test loop.
  ❌ Misses DOM behavior, silent skips, and the other cfg-gated product path.
Net: systems-over-heroes testing requires observable coverage at each platform and presentation boundary.

### Performance & Resource Review — 1 issue resolved

#### Auto decision D8 — Are frame-count bounds enough?
Context: A 600-frame bound still permits multi-gigabyte scratch, extreme-aspect analysis frames, unchecked buffer products, and avoidable full-frame duplication.
ELI10: “Only 600 files” sounds small until each file is a large image. Without byte ceilings and one-frame-at-a-time processing, a hostile or unusual video can fill disk or crash allocation.
Stakes if we pick wrong: Import can exhaust memory/disk or wrap a buffer length before cancellation can help.
Recommendation: 8A because explicit byte ceilings and streaming one owned buffer at a time bound real resources, not just item counts.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) 8A — Checked 64 MiB analysis-frame and 4 GiB scratch caps with streaming buffers (recommended; human: ~1 day / AI: ~30 min; low risk; low maintenance)
  ✅ Worst-case memory/disk behavior is testable and returns `resource_limit`.
  ❌ Extremely unusual inputs can be rejected despite a readable video stream.
B) 8B — Bound only candidates and evidence count (human: no extra time / AI: no extra time; medium risk; low maintenance)
  ✅ No additional accounting code.
  ❌ Does not bound bytes or allocation safety.
Net: byte bounds are the minimum complete definition of bounded media processing.

### Second-pass review (auto mode, 2026-07-20) — 6 issues resolved

Verified D1–D8 against the task bodies and the codebase. D2's ownership decision had not been applied to Task 8, and several executable details had drifted. Scope, complexity, and architecture were otherwise confirmed: every referenced type, function, fixture convention, and dependency exists as the plan claims.

#### Auto decision D9 — Task 8 Step 3 still specified the cloneable seed that D2 rejected
Context: D2 and Global Constraint 25 mandate unique non-`Clone` seed/scratch ownership and Task 4 Step 3 implements it, but Task 8 Step 3 described a reference-counted cloneable seed and `#[derive(Clone)]` on `Effect`.
ELI10: The plan decided exactly one owner controls gigabytes of scratch, but the UI task still described the old many-keys design. An executor reading Task 8 would have implemented the thing D2 forbade.
Stakes if we pick wrong: Saved imports retain scratch until an arbitrary last clone drops; Task 6's deterministic first-save release becomes unverifiable.
Recommendation: D9-A because it applies the already-approved D2 to Task 8 and names the exact derive ripple (11 existing `assert_eq!(update.effect, ...)` tests; no call site clones `Message`; iced does not require it).
Completeness: A=10/10, B=5/10.
Pros / cons:
A) D9-A — Move the seed by value; drop `Clone` from home `Message`, drop `Clone`/`PartialEq`/`Eq` from `Effect`, manual privacy-safe `Debug`, convert effect assertions to `matches!`, drop `Clone` from both product `Message` enums (recommended; human: ~3 hours / AI: ~30 min; low risk; low maintenance)
  ✅ Restores one owner, deterministic stale-message drop, and a compilable plan.
  ❌ Mechanical derive churn across home and both product message enums.
B) D9-B — Keep the cloneable reference-counted seed (human: ~2 hours / AI: ~20 min; medium risk; medium maintenance)
  ✅ No derive churn.
  ❌ Reverts approved D2; cleanup timing depends on invisible message lifetimes again.
Net: D2 was already approved — this applies it; explicit ownership beats convenient derives.

#### Auto decision D10 — Task 8 omitted the setup-resolution messages and pending state D3 requires
Context: D3 approved explicit resolve → setup/install → retry messages and phases, and Task 8 Step 5 expects "setup retry works", but Step 3 named no toolchain-resolved/setup-finished/retry message, no setup phase, and no field remembering the selected path across resolve → install → retry.
ELI10: The user picks a video, then the app may need to install FFmpeg+FFprobe first. Without a variable holding "which file" and messages for "setup finished / retry", each implementer invents them — the improvisation D3 outlawed.
Stakes if we pick wrong: Platform adapters diverge; the selected file can be lost across a setup retry, or import starts with FFprobe missing.
Recommendation: D10-A because naming the messages, phases, and pending field keeps platform code mechanical and preserves DRY.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) D10-A — Add `ImportToolchainResolved`, `ImportSetupFinished`, `RetryImportSetup`, `ResolvingToolchain`/`SettingUp` phases, and `pending: Option<(ImportOperationId, PathBuf)>` (recommended; human: ~2 hours / AI: ~15 min; low risk; low maintenance)
  ✅ Setup lifecycle is explicit, testable, and identical on both platforms.
  ❌ Slightly longer message enum and one more coordinator field.
B) D10-B — Leave setup messages to executor discretion (human: no extra time / AI: no extra time; high risk; medium maintenance)
  ✅ Shorter plan text.
  ❌ Guaranteed divergence from D3; "setup retry works" has no defined mechanics to test.
Net: D3 decided the what; this names the how so two platforms cannot drift.

#### Auto decision D11 — Three snippets did not compile (or failed clippy), plus one dead command flag
Context: Task 4 Step 1 used array-repeat `[solid_frame(20); 8]` on non-`Copy` `RgbaImage`; Task 3 Step 1 escaped quotes inside a raw byte string, making the "JSON" invalid and the asserted category unreachable; Task 2 Step 1 used `index as u64` on an inferred-`u64` index, tripping `clippy::unnecessary_cast` under `-D warnings`; three commands used `--no-default-features` on a crate with no features.
ELI10: Agents execute snippets and commands literally. Three would fail on first run for reasons unrelated to the intended RED signal, and the fourth pretends a feature boundary exists where there is none.
Stakes if we pick wrong: Executors silently fix the plan locally, plan and code drift, and red-step signals become ambiguous.
Recommendation: D11-A because an agent-executed plan must be literally true (explicit > clever).
Completeness: A=10/10, B=7/10.
Pros / cons:
A) D11-A — Fix all four in the plan text (recommended; human: ~30 min / AI: ~5 min; no risk)
  ✅ Every snippet compiles and every RED step fails for the intended reason.
  ❌ No cons — near hard-stop choice.
B) D11-B — Leave snippets as illustrative pseudocode (human: no extra time; medium risk)
  ✅ Zero edits now.
  ❌ Shifts ambiguity onto executors; violates the plan's own D6.
Net: the plan's own D6 says agents execute literally — these four lines were where that broke.

#### Auto decision D12 — `load_manifest_fixture` was used in Task 1 but missing from the Test Fixture Contracts
Context: Task 1 Step 1 calls `load_manifest_fixture(version, input_source)`, but the contract section that exists to prevent incompatible fixture inventions did not list it.
ELI10: The contract list stops people inventing clashing helpers. A snippet name missing from the list is exactly the drift it was created to prevent.
Stakes if we pick wrong: An executor invents a signature with the wrong return type or error category and Task 1's red step fails unexpectedly.
Recommendation: D12-A — one contract line; trivially cheap consistency.
Completeness: A=10/10, B=7/10.
Pros / cons:
A) D12-A — Add `fn load_manifest_fixture(schema_version: u32, input_source: &str) -> Result<LoadedProject, ProjectError>;` (recommended; human: ~5 min / AI: ~1 min)
  ✅ Contract list matches snippet usage exactly.
  ❌ One more line in an already long file.
B) D12-B — Treat it as a task-local helper (human: no extra time)
  ✅ No edit.
  ❌ Sets a precedent that the contract list is advisory.
Net: one line buys contract-list integrity.

#### Auto decision D13 — Task 1 specified warning-bounds validation with no red-first test
Context: Task 1 Step 3 mandates rejecting more than two warnings and duplicate variants, but Step 1 wrote no failing test for that rule.
ELI10: The plan's own rule is test-first for every new behavior. A validation rule with no test can be implemented wrong or forgotten, and no later step catches it.
Stakes if we pick wrong: A malformed v2 manifest with three or duplicate warnings persists and surfaces later as export/publish inconsistency.
Recommendation: D13-A — add one red-first test to Task 1 Step 1 using the existing `invalid_manifest` category.
Completeness: A=10/10, B=7/10.
Pros / cons:
A) D13-A — Add `manifest_rejects_unbounded_or_duplicate_import_warnings` (recommended; human: ~30 min / AI: ~5 min; low risk)
  ✅ Every Step 3 validation rule has a prior RED signal.
  ❌ One more test in an already covered task.
B) D13-B — Rely on Step 4's broad crate run (human: no extra time)
  ✅ No plan edit.
  ❌ Broad runs do not prove this rule; under-tests the boundary D4 hardened.
Net: one small test closes the only TDD hole in the schema task.

#### Auto decision D14 — Session schema had no version-boundary test symmetric to the project side
Context: D4 approved explicit version dispatch for both project and session schemas, and `project/publish.rs` parses `session.json` in Rust, but Task 7 tested only that v1 sessions default to empty warnings and named no unknown-version rejection.
ELI10: The project file got a bouncer checking IDs; the session file got a welcome mat. A future v3 session would be silently accepted by v2 code, dropping fields it does not know.
Stakes if we pick wrong: Publishing a newer-format guide silently loses data instead of failing loudly; D4's invariant holds for projects but not sessions.
Recommendation: D14-A — add a red-first session boundary test to Task 7 Step 1 and a `SessionManifest::validate` mandate to Step 3.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) D14-A — Add the test plus validate-after-parse rule (recommended; human: ~1 hour / AI: ~10 min; low risk)
  ✅ Both persistence boundaries enforce the same explicit-version contract.
  ❌ A small validation helper where serde currently does everything implicitly.
B) D14-B — Accept serde-default leniency for sessions (human: no extra time; medium risk)
  ✅ Less code.
  ❌ Contradicts approved D4; silent acceptance at the publish boundary.
Net: D4 named sessions explicitly; Task 7 now honors it.

---

### Task 1: Provenance, Warning Types, and Project Schema v2

**Files:**
- Modify: `crates/rollshot-action/src/models.rs`
- Modify: `crates/rollshot-action/src/project/model.rs`
- Modify: `crates/rollshot-action/src/project/store.rs`
- Modify: `crates/rollshot-action/src/project/validate.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/project/model.rs`
- Test: `crates/rollshot-action/src/project/store.rs`

**Interfaces:**
- Consumes: Existing `InputSourceKind`, `DegradedReason`, `ProjectManifestV1`, `ProjectSnapshot`, and atomic project store.
- Produces: `ImportWarning`, `ProjectManifestV2`, `PROJECT_SCHEMA_VERSION == 2`, and v1-to-v2 load migration used by all later tasks.

- [ ] **Step 1: Write failing serialization and migration tests**

Add tests that assert the exact wire names and legacy behavior:

```rust
#[test]
fn imported_provenance_has_stable_wire_names() {
    assert_eq!(serde_json::to_string(&InputSourceKind::ImportedVideo).unwrap(), "\"imported-video\"");
    assert_eq!(serde_json::to_string(&DegradedReason::ImportedRecording).unwrap(), "\"imported-recording\"");
    assert_eq!(serde_json::to_string(&ImportWarning::NoVisualChangesDetected).unwrap(), "\"no-visual-changes-detected\"");
    assert_eq!(serde_json::to_string(&ImportWarning::IntermediateChangesReduced).unwrap(), "\"intermediate-changes-reduced\"");
}

#[test]
fn version_one_manifest_loads_as_version_two_without_warnings() {
    let root = write_v1_project_fixture();
    let loaded = load_project(root.path()).unwrap();
    assert_eq!(loaded.manifest.schema_version, 2);
    assert!(loaded.manifest.import_warnings.is_empty());
}

#[test]
fn version_two_manifest_round_trips_import_metadata() {
    let snapshot = imported_snapshot(vec![ImportWarning::IntermediateChangesReduced]);
    let root = tempdir().unwrap();
    let commit = create_project(&snapshot, root.path()).unwrap();
    assert_eq!(commit.manifest.input_source, InputSourceKind::ImportedVideo);
    assert_eq!(commit.manifest.import_warnings, vec![ImportWarning::IntermediateChangesReduced]);
}

#[test]
fn project_loader_rejects_unknown_versions_and_v2_values_in_v1() {
    assert_eq!(load_manifest_fixture(99, "visual-only").unwrap_err().category(), "unsupported_version");
    assert_eq!(load_manifest_fixture(1, "imported-video").unwrap_err().category(), "invalid_manifest");
}

#[test]
fn manifest_rejects_unbounded_or_duplicate_import_warnings() {
    let snapshot = imported_snapshot(vec![
        ImportWarning::NoVisualChangesDetected,
        ImportWarning::NoVisualChangesDetected,
    ]);
    let root = tempdir().unwrap();
    let error = create_project(&snapshot, root.path()).unwrap_err();
    assert_eq!(error.category(), "invalid_manifest");
}
```

- [ ] **Step 2: Run the focused tests and confirm the expected failures**

Run: `rtk cargo test -p rollshot-action project::`

Expected: compilation fails because the new enum variants, warning type, and v2 manifest do not exist.

- [ ] **Step 3: Add the exact provenance and warning model**

Extend the serde-kebab-case enums and add the bounded warning enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportWarning {
    NoVisualChangesDetected,
    IntermediateChangesReduced,
}

// Add to InputSourceKind:
ImportedVideo,

// Add to DegradedReason:
ImportedRecording,
```

Set `PROJECT_SCHEMA_VERSION` to `2`. Keep `ProjectManifestV1` as the legacy wire type and add the normalized current type:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifestV2 {
    pub schema_version: u32,
    pub revision: u64,
    pub title: String,
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub enabled_outputs: EnabledOutputs,
    pub frames: Vec<ProjectFrame>,
    pub steps: Vec<ProjectStep>,
    pub import_warnings: Vec<ImportWarning>,
}

impl From<ProjectManifestV1> for ProjectManifestV2 {
    fn from(v1: ProjectManifestV1) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            revision: v1.revision,
            title: v1.title,
            capture_region: v1.capture_region,
            input_source: v1.input_source,
            input_capability: v1.input_capability,
            enabled_outputs: v1.enabled_outputs,
            frames: v1.frames,
            steps: v1.steps,
            import_warnings: Vec::new(),
        }
    }
}
```

Change `LoadedProject`, `ProjectCommit`, and all current writes to `ProjectManifestV2`; add `import_warnings: Vec<ImportWarning>` to `ProjectSnapshot`. In `read_manifest`, deserialize a header first and branch exactly on schema 1 or 2; reject every other version before structure validation.

In `validate_manifest_structure`, reject more than two warnings and reject duplicate variants. This makes the persisted warning array bounded and limits it to the enum's two stable values. The v1 branch must also reject `ImportedVideo` or `ImportedRecording`, because those values require v2 warning semantics; the v2 branch rejects any `schema_version` other than exactly 2.

- [ ] **Step 4: Run project and crate tests**

Run: `rtk cargo test -p rollshot-action project::`

Expected: all project tests pass, including v1 migration, v2 round trip, unknown-field rejection, and revision validation.

Run: `rtk cargo test -p rollshot-action`

Expected: all tests pass after updating existing snapshot/manifest fixtures with `import_warnings: Vec::new()`.

- [ ] **Step 5: Commit the schema boundary**

```bash
rtk git add crates/rollshot-action/src/models.rs crates/rollshot-action/src/lib.rs crates/rollshot-action/src/project
rtk git commit -m "feat(action-guide): add imported video provenance"
```

---

### Task 2: Deterministic Bounded Candidate Selection

**Files:**
- Create: `crates/rollshot-action/src/video_import/selection.rs`
- Create: `crates/rollshot-action/src/video_import/mod.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/video_import/selection.rs`

**Interfaces:**
- Consumes: `Detector`, `AnalysisFrame`, `CandidateMarker`, `ImportWarning`.
- Produces: fixed constants, `CandidateSelector::push/finish`, `evidence_sample_indices`, and public progress/cancellation/error types consumed by the process layer and app.

- [ ] **Step 1: Write failing selector boundary tests**

```rust
#[test]
fn two_hundred_candidates_are_not_reduced() {
    let mut selector = CandidateSelector::new(100_000);
    for index in 0..200 { selector.push(marker(index, index * 500)); }
    let result = selector.finish();
    assert_eq!(result.candidates.len(), 200);
    assert!(!result.reduced);
}

#[test]
fn candidate_201_switches_to_full_duration_reduction() {
    let mut selector = CandidateSelector::new(200_000);
    for index in 0..401 { selector.push(marker(index, index * 500)); }
    let result = selector.finish();
    assert!(result.candidates.len() <= MAX_GENERATED_STEPS);
    assert_eq!(result.candidates.first().unwrap().at_ms, 0);
    assert_eq!(result.candidates.last().unwrap().at_ms, 200_000);
    assert!(result.candidates.windows(2).all(|w| w[0].at_ms < w[1].at_ms));
    assert!(result.reduced);
    assert!(result.candidates.iter().any(|candidate| candidate.at_ms < 50_000));
    assert!(result.candidates.iter().any(|candidate| (50_000..150_000).contains(&candidate.at_ms)));
}

#[test]
fn evidence_indices_are_sorted_unique_and_bounded() {
    let indices = evidence_sample_indices(&[0, 4, 9], 10);
    assert_eq!(indices, vec![0, 1, 3, 4, 5, 8, 9]);
    assert!(indices.len() <= 3 * MAX_GENERATED_STEPS);
}
```

- [ ] **Step 2: Run the selector tests and confirm they fail**

Run: `rtk cargo test -p rollshot-action video_import::selection::tests`

Expected: compilation fails because `CandidateSelector` and fixed import constants do not exist.

- [ ] **Step 3: Implement the bounded selector and public control types**

Expose these exact constants and types from `video_import/mod.rs`:

```rust
pub const ANALYSIS_FPS: u64 = 2;
pub const ANALYSIS_WIDTH: u32 = 384;
pub const MAX_GENERATED_STEPS: usize = 200;
pub const REDUCTION_BUCKETS: usize = 198;
pub const EVIDENCE_MAX_LONG_EDGE: u32 = 1920;
pub const MAX_EVIDENCE_FRAMES: usize = 600;
pub const MAX_ANALYSIS_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SCRATCH_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoImportPass { Preflight, Analyze, Extract }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoImportProgress {
    pub pass: VideoImportPass,
    pub processed_ms: u64,
    pub total_ms: u64,
    pub retained_candidates: usize,
}

#[derive(Clone, Default)]
pub struct VideoImportCancellation(Arc<AtomicBool>);

impl VideoImportCancellation {
    pub fn cancel(&self) { self.0.store(true, Ordering::Release); }
    pub fn is_cancelled(&self) -> bool { self.0.load(Ordering::Acquire) }
}
```

Implement reduced mode as `first: Option<CandidateMarker>`, `latest: Option<CandidateMarker>`, and `[Option<CandidateMarker>; 198]`. When candidate 201 arrives, initialize reduced mode by replaying the already retained 200 candidates before pushing candidate 201. Calculate `min((u128::from(at_ms) * 198) / max(u128::from(duration_ms), 1), 197)` and convert only the bounded result to `usize`; replace an occupied bucket only with a later candidate. `finish` combines first, occupied buckets, and latest, sorts by `(at_ms, center_id)`, deduplicates identical sample timestamps, and returns an explicit invariant error in production rather than truncating silently.

- [ ] **Step 4: Run selector tests and property-style determinism coverage**

Run: `rtk cargo test -p rollshot-action video_import::selection::tests`

Expected: all selector tests pass, including repeated-run equality and maximum 600 evidence indices.

- [ ] **Step 5: Commit the pure selection unit**

```bash
rtk git add crates/rollshot-action/src/video_import crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action-guide): bound imported video candidates"
```

---

### Task 3: Probe Contract and Cancellable Child Lifecycle

**Files:**
- Create: `crates/rollshot-action/src/video_import/probe.rs`
- Create: `crates/rollshot-action/src/video_import/process.rs`
- Modify: `crates/rollshot-action/src/video_import/mod.rs`
- Test: `crates/rollshot-action/src/video_import/probe.rs`
- Test: `crates/rollshot-action/src/video_import/process.rs`

**Interfaces:**
- Consumes: explicit `VideoToolchain` paths, `VideoImportCancellation`, and the fixed constants from Task 2.
- Produces: `ProbeMetadata`, `VideoImportError`, `run_cancellable_child`, and privacy-safe command builders used by Task 4.

- [ ] **Step 1: Write failing probe, command, and reap tests**

```rust
#[test]
fn probe_command_requests_only_required_video_metadata() {
    let args = probe_args(Path::new("sentinel-source.mp4"));
    assert!(args.windows(2).any(|w| w == ["-select_streams", "v:0"]));
    assert!(args.windows(2).any(|w| w == ["-of", "json"]));
    assert!(args.iter().any(|arg| arg == "-an"));
    assert!(args.iter().any(|arg| arg == "-sn"));
}

#[test]
fn metadata_rejects_missing_stream_and_invalid_dimensions() {
    assert_eq!(parse_probe_json(br#"{"streams":[],"format":{"duration":"2.0"}}"#).unwrap_err().category(), "missing_video_stream");
    assert_eq!(parse_probe_json(invalid_dimensions_json()).unwrap_err().category(), "invalid_video_metadata");
}

#[test]
fn cancellation_kills_and_waits_for_child() {
    let fixture = long_running_fixture_process();
    let cancel = VideoImportCancellation::default();
    cancel.cancel();
    let error = run_cancellable_child(fixture, &cancel, |_| {}).unwrap_err();
    assert_eq!(error.category(), "cancelled");
    assert!(!fixture_process_is_alive());
}
```

- [ ] **Step 2: Run tests and confirm missing contracts**

Run: `rtk cargo test -p rollshot-action video_import::`

Expected: compilation fails because the probe parser, error categories, and child runner do not exist.

- [ ] **Step 3: Implement privacy-safe probe and error model**

Use this public boundary:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoToolchain { pub ffmpeg: PathBuf, pub ffprobe: PathBuf }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeMetadata {
    pub duration_ms: u64,
    pub display_width: u32,
    pub display_height: u32,
    pub rotation_degrees: i32,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum VideoImportError {
    #[error("Video metadata could not be read.")] ProbeFailed,
    #[error("The selected file has no readable video stream.")] MissingVideoStream,
    #[error("The selected video has invalid dimensions or duration.")] InvalidVideoMetadata,
    #[error("The video decoder is unavailable.")] DecoderUnavailable,
    #[error("The video could not be decoded.")] DecodeFailed,
    #[error("Required evidence could not be extracted.")] EvidenceMissing,
    #[error("Temporary evidence storage failed.")] ScratchIo,
    #[error("The recording exceeds an internal resource bound.")] ResourceLimit,
    #[error("Import was cancelled.")] Cancelled,
}
```

`category()` returns exactly the nine spec categories. Do not store child stderr, input paths, or filenames in errors. Probe with `-nostdin -v error -an -sn -dn -select_streams v:0 -show_entries stream=width,height,duration,side_data_list:format=duration -of json`; cap captured probe JSON and stderr at 1 MiB each, normalize rotation to `0/90/180/270`, swap display dimensions for 90/270, and require non-zero dimensions plus a finite positive duration. Spawn failure maps to `DecoderUnavailable`; malformed/non-zero probe output maps to the appropriate probe category without carrying raw output.

- [ ] **Step 4: Implement one cancellable process primitive**

Implement one lifecycle owner that takes the child's stdout and stderr exactly once. A caller-supplied stdout consumer owns JSON/rawvideo reads; a concurrent stderr reader drains output, parses only recognized `-progress pipe:2` records, and retains at most a 64 KiB diagnostic ring that is never logged or returned. The owner polls `try_wait`; on cancellation it calls `kill`, then `wait`, closes the pipe handles, joins the reader/consumer, and returns `Cancelled`. Every early error and non-cancel terminal path follows the same wait/join sequence before status validation, and a defensive `Drop` performs kill+wait if the normal finish path was skipped. Emit only category/pass/timestamp/count fields to `rollshot::action::video_import`.

Run: `rtk cargo test -p rollshot-action video_import::process::tests`

Expected: pipe ownership, bounded diagnostics, cancellation, non-zero exit, and child-reap tests pass without consuming caller-owned frame bytes.

- [ ] **Step 5: Run process tests**

Run: `rtk cargo test -p rollshot-action video_import::`

Expected: all tests pass for malformed JSON, missing stream, non-zero exit, broken pipe, cancellation, and child reap.

- [ ] **Step 6: Commit the process boundary**

```bash
rtk git add crates/rollshot-action/src/video_import
rtk git commit -m "feat(action-guide): add cancellable video probe"
```

---

### Task 4: Two-Pass Importer and RAII Scratch Evidence

**Files:**
- Create: `crates/rollshot-action/src/video_import/scratch.rs`
- Modify: `crates/rollshot-action/src/video_import/mod.rs`
- Modify: `crates/rollshot-action/src/video_import/process.rs`
- Modify: `crates/rollshot-action/src/step_frame_source.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/video_import/mod.rs`
- Test: `crates/rollshot-action/src/video_import/scratch.rs`

**Interfaces:**
- Consumes: Task 1 project/provenance types, Task 2 selector, Task 3 child/probe layer, existing `Detector`, `AnalysisFrame`, `LumaPlane`, `Guide`, and project PNG asset layout.
- Produces: `import_video`, `ImportedWorkspaceSeed`, `ImportedScratch`, and `ProjectFrameSource::from_catalog` consumed by the timeline.

- [ ] **Step 1: Add FFmpeg-generated fixture tests**

Create tests guarded by the existing `ROLLSHOT_TEST_FFMPEG=1` convention:

```rust
#[test]
fn static_video_returns_final_frame_fallback() {
    let fixture = fixture_video(&vec![solid_frame(20); 8], true);
    let seed = run_import(fixture.path()).unwrap();
    assert_eq!(seed.guide.steps().len(), 1);
    assert_eq!(seed.guide.steps()[0].title, "Imported recording");
    assert_eq!(seed.import_warnings, vec![ImportWarning::NoVisualChangesDetected]);
    assert_eq!(seed.guide.steps()[0].kind, CandidateKind::UiChanged);
}

#[test]
fn visual_settles_produce_only_ui_changed_steps() {
    let fixture = settle_sequence_fixture();
    let seed = run_import(fixture.path()).unwrap();
    assert!(seed.guide.steps().iter().all(|step| step.title == "UI changed"));
    assert!(seed.guide.steps().iter().all(|step| step.kind == CandidateKind::UiChanged && step.reason == DetectReason::VisualChange));
}

#[test]
fn evidence_is_scaled_bounded_and_audio_is_ignored() {
    let seed = run_import(audio_bearing_4k_fixture().path()).unwrap();
    assert!(seed.frames.len() <= MAX_EVIDENCE_FRAMES);
    assert!(seed.frames.iter().all(|frame| frame.width.max(frame.height) <= EVIDENCE_MAX_LONG_EDGE));
    assert_eq!(scratch_files(&seed), png_asset_files(&seed));
}
```

Also add cancellation tests for probe/pass 1/pass 2, mandatory center-frame failure, optional edge neighbors, rotation, >200 reduction, scratch cleanup, and a long synthetic fixture proving the retained catalog is at most 600 frames.

- [ ] **Step 2: Run pure tests, then opt-in fixtures, and confirm failures**

Run: `rtk cargo test -p rollshot-action video_import::`

Expected: compilation fails because the importer result and scratch types do not exist.

After compilation is restored, run fixtures with explicit tools:

`rtk env ROLLSHOT_TEST_FFMPEG=1 ROLLSHOT_FFMPEG=/usr/bin/ffmpeg ROLLSHOT_FFPROBE=/usr/bin/ffprobe cargo test -p rollshot-action video_import::tests -- --nocapture`

- [ ] **Step 3: Implement scratch ownership and imported seed**

Use these public types:

```rust
pub struct ImportedScratch(ImportedScratchInner);

pub struct ImportedWorkspaceSeed {
    pub guide: Guide,
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub frames: Vec<ProjectFrame>,
    pub import_warnings: Vec<ImportWarning>,
    pub scratch: ImportedScratch,
}

pub struct VideoImportRequest {
    pub input: PathBuf,
    pub toolchain: VideoToolchain,
    pub scratch_parent: PathBuf,
}

pub fn import_video(
    request: VideoImportRequest,
    cancel: VideoImportCancellation,
    progress: impl Fn(VideoImportProgress) + Send + Sync,
) -> Result<ImportedWorkspaceSeed, VideoImportError>;
```

`ImportedScratchInner` owns the unique directory, a running byte count, and an exclusive advisory lock acquired through the existing `rustix` dependency. Its `Drop` removes only that exact directory while the lock is still held, then releases the lock; stale cleanup likewise holds the acquired lock through deletion to avoid a check/delete race. `ImportedScratch::root(&self) -> &Path` exposes the derived-evidence root without exposing the source video. `cleanup_stale_import_scratch(parent)` canonicalizes the app-owned parent, considers only direct real-directory children matching `import-*`, and removes only those whose lock can be acquired. PNG assets use the existing SHA-256 content-addressed project asset naming so later save can copy them as `SnapshotFramePayload::ExistingAsset`. `ImportedScratch` and `ImportedWorkspaceSeed` intentionally do not implement `Clone` or path-revealing `Debug`.

- [ ] **Step 4: Implement pass 1 with the existing detector**

Build FFmpeg args with `-nostdin -an -sn -dn`, automatic rotation, `fps=2`, aspect-preserving `scale=384:-2`, `format=gray`, rawvideo output, and machine-readable progress on stderr. Compute the exact frame size with checked arithmetic and reject it above `MAX_ANALYSIS_FRAME_BYTES` before allocating. Reuse one owned frame buffer, read exactly one frame at a time, distinguish clean frame-boundary EOF from truncated EOF, map bytes into `LumaPlane.samples` without an additional full-frame clone, assign checked `FrameId = sample_index` and `at_ms = sample_index * 500`, call only `Detector::observe_frame`, then call `Detector::finish`. Feed candidates into `CandidateSelector`, retain only the final sample index for fallback, and coalesce timestamp-based progress when the bounded UI channel is full.

- [ ] **Step 5: Implement pass 2 and seed construction**

Generate the sorted unique center±1 indices and assert the list is at most `MAX_EVIDENCE_FRAMES`. Run one sequential FFmpeg extraction using `select` plus `scale='min(1920,iw)':'min(1920,ih)':force_original_aspect_ratio=decrease`, `-fps_mode passthrough`, `-an -sn -dn`, and numbered PNG output in a staging child of scratch. Match emitted files to an explicit ordered requested-index manifest rather than inferring source indices from output names, require every center index, allow missing out-of-range neighbors, and validate dimensions plus PNG bytes before content-addressing one file at a time. Stop before `MAX_SCRATCH_BYTES`, return `ResourceLimit`, and remove staging files. Construct chronological `CandidateStep`s whose center is the keyframe and available adjacent IDs are `nearby`, then create the editable `Guide` with `Guide::from_candidates`.

For zero candidates, select the final sample, title the single step `Imported recording`, and add `NoVisualChangesDetected`. For reduced mode, add `IntermediateChangesReduced`. Set source to `ImportedVideo`, capability to `VisualOnly { reason: ImportedRecording }`, and capture region to `(0, 0, evidence_width, evidence_height)`.

- [ ] **Step 6: Add the disk-backed frame source constructor**

```rust
impl ProjectFrameSource {
    pub fn from_catalog(root: PathBuf, frames: Vec<ProjectFrame>, byte_limit: usize) -> Self {
        Self::new(root, frames.into_iter().map(|f| (f.id, f)).collect(), byte_limit)
    }
}
```

Factor `from_loaded` through the same private `new` constructor. Do not add an in-memory fallback.

- [ ] **Step 7: Run importer and full crate tests**

Run: `rtk cargo test -p rollshot-action video_import::`

Expected: all pure command/parser/selector/scratch tests pass.

Run: `rtk env ROLLSHOT_TEST_FFMPEG=1 ROLLSHOT_FFMPEG=/usr/bin/ffmpeg ROLLSHOT_FFPROBE=/usr/bin/ffprobe cargo test -p rollshot-action video_import::tests -- --nocapture`

Expected: all local fixture tests pass; cancelled tests report no live child and no remaining scratch directory.

- [ ] **Step 8: Commit the importer**

```bash
rtk git add crates/rollshot-action/src/video_import crates/rollshot-action/src/step_frame_source.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action-guide): import visual steps from video"
```

---

### Task 5: Separate FFmpeg+FFprobe Toolchain Resolution

**Files:**
- Modify: `crates/rollshot-app/src/managed_ffmpeg.rs`
- Test: `crates/rollshot-app/src/managed_ffmpeg.rs`

**Interfaces:**
- Consumes: existing `resolve_ffmpeg`, managed manifest/download flow, `ROLLSHOT_FFMPEG`, PATH lookup.
- Produces: `resolve_video_import_toolchain() -> VideoImportToolchainResolution` while preserving FFmpeg-only export behavior.

- [ ] **Step 1: Write resolver precedence and isolation tests**

```rust
#[test]
fn import_resolution_honors_both_explicit_overrides() {
    let _ffmpeg = EnvVarGuard::set("ROLLSHOT_FFMPEG", fake_ffmpeg());
    let _ffprobe = EnvVarGuard::set("ROLLSHOT_FFPROBE", fake_ffprobe());
    assert!(matches!(resolve_video_import_toolchain(), VideoImportToolchainResolution::Available(_)));
}

#[test]
fn missing_ffprobe_does_not_break_ffmpeg_only_exports() {
    let _ffmpeg = EnvVarGuard::set("ROLLSHOT_FFMPEG", fake_ffmpeg());
    let _ffprobe = EnvVarGuard::set("ROLLSHOT_FFPROBE", "/definitely/missing/ffprobe");
    assert!(matches!(resolve_ffmpeg(), FfmpegResolution::Available(_)));
    assert!(matches!(resolve_video_import_toolchain(), VideoImportToolchainResolution::NeedsSetup(_)));
}
```

- [ ] **Step 2: Run resolver tests and confirm failure**

Run: `rtk cargo test -p rollshot-app managed_ffmpeg --features action-guide`

Expected: compilation fails because the paired resolver does not exist.

- [ ] **Step 3: Add paired paths without changing the old resolver**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VideoImportToolchainResolution {
    Available(rollshot_action::VideoToolchain),
    NeedsSetup(FfmpegSetupInfo),
}

pub(crate) fn resolve_video_import_toolchain() -> VideoImportToolchainResolution;
```

Resolve each external executable independently in override → PATH order, then require a valid pair; this supports one explicit override plus one PATH binary without accepting an invalid override silently. If no external pair exists, use the matching pair in the managed manifest. Extend managed metadata/manifest to schema 2 with `ffprobe_binary_path` and its version line from the same archive; read schema 1 only for the unchanged FFmpeg-only resolver. Change managed installation from `unpack_ffmpeg_without_extras` to the sidecar's full `unpack_ffmpeg`, validate both binaries before atomically writing the v2 manifest, and remove both on installation failure. Keep `resolve_ffmpeg()` and all existing export callers FFmpeg-only. Serialize all environment-mutating tests with the existing `ENV_LOCK` and add mixed override/PATH plus managed-v1 compatibility tests.

- [ ] **Step 4: Run resolver and app tests**

Run: `rtk cargo test -p rollshot-app managed_ffmpeg --features action-guide`

Expected: paired resolver tests and all existing FFmpeg-only tests pass.

- [ ] **Step 5: Commit paired toolchain resolution**

```bash
rtk git add crates/rollshot-app/src/managed_ffmpeg.rs
rtk git commit -m "feat(app): resolve ffprobe for video import"
```

---

### Task 6: Imported Timeline Construction and Scratch Transfer

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: `ImportedWorkspaceSeed`, `ProjectFrameSource::from_catalog`, schema v2 snapshots, existing first-save worker.
- Produces: `TimelineWorkspace::from_imported_video`, persistent warning notice, and scratch-to-saved-project ownership transfer.

- [ ] **Step 1: Write failing workspace lifecycle tests**

```rust
#[test]
fn imported_seed_opens_dirty_unsaved_workspace() {
    let (seed, scratch_path) = imported_seed_fixture();
    let workspace = TimelineWorkspace::from_imported_video(seed);
    assert!(matches!(workspace.project_session, Some(ProjectSession::Unsaved)));
    assert_eq!(workspace.save_state, ProjectSaveState::Dirty);
    assert!(workspace.persistent_notice().contains("Visual-only draft"));
    assert!(scratch_path.exists());
}

#[test]
fn first_save_switches_frame_source_then_releases_scratch() {
    let (mut workspace, scratch_path) = imported_workspace_fixture();
    complete_first_save(&mut workspace);
    assert!(!scratch_path.exists());
    assert_eq!(workspace.frame_source_root(), workspace.project_root().as_deref());
}

#[test]
fn closing_unsaved_import_releases_scratch_and_never_adds_recent() {
    let (workspace, scratch_path) = imported_workspace_fixture();
    drop(workspace);
    assert!(!scratch_path.exists());
}
```

- [ ] **Step 2: Run focused timeline tests and confirm failure**

Run: `rtk cargo test -p rollshot-app timeline_workspace --features action-guide`

Expected: compilation fails because the imported constructor and scratch owner fields do not exist.

- [ ] **Step 3: Add imported workspace state**

Add these fields to `TimelineWorkspace`:

```rust
#[cfg(feature = "action-guide")]
pub(crate) import_warnings: Vec<rollshot_action::ImportWarning>,
#[cfg(feature = "action-guide")]
pub(crate) imported_scratch: Option<rollshot_action::ImportedScratch>,
```

Implement `from_imported_video(seed)` by creating the existing guide/store presentation from `seed.guide`, creating `StepFrameSource::Project(ProjectFrameSource::from_catalog(seed.scratch.root().to_owned(), seed.frames, DEFAULT_PROJECT_FRAME_CACHE_BYTES))`, setting source/capability/region/warnings from the seed, and setting `ProjectSession::Unsaved`, `ProjectSaveState::Dirty`, and the existing visible first-save prompt. Do not call recent-project APIs.

- [ ] **Step 4: Carry warnings through snapshots and reopen**

Add `state.import_warnings.clone()` to `build_project_snapshot`; restore `loaded.manifest.import_warnings` in `from_loaded_project`. Render the visual-only disclosure whenever source is `ImportedVideo`, plus specific copy for each warning:

```text
Visual-only draft. Steps were inferred from visual changes because mouse and keyboard events were unavailable. Review before export.
No visual changes detected; the final sampled frame was used.
Intermediate visual changes were omitted to keep this draft reviewable.
```

- [ ] **Step 5: Make first-save completion switch sources before dropping scratch**

For `NewWritable` and `NewCommittedReadOnly`, rebuild `ProjectFrameSource` from the just-saved v2 manifest/root (include the manifest in `SaveWorkerOutcome` instead of only revision), assign it to `state.frame_source`, then call `state.imported_scratch.take()`. Existing-save and failed-save paths retain their current behavior; failed first save keeps scratch and remains retryable.

- [ ] **Step 6: Run timeline tests**

Run: `rtk cargo test -p rollshot-app timeline_workspace --features action-guide`

Expected: imported workspaces are unsaved/dirty, failed saves retain scratch, successful first saves release it only after source switch, and reopen preserves warnings.

- [ ] **Step 7: Commit timeline lifecycle support**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace
rtk git commit -m "feat(action-guide): open imported video drafts"
```

---

### Task 7: Action Guide and Issue Pack Warning Disclosure

**Files:**
- Modify: `crates/rollshot-action/src/export/model.rs`
- Modify: `crates/rollshot-action/src/export/mod.rs`
- Modify: `crates/rollshot-action/src/export/html.rs`
- Modify: `crates/rollshot-action/src/export/viewer.html`
- Modify: `crates/rollshot-action/src/project/publish.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/guide_export.rs`
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-action/src/export/mod.rs`
- Test: `crates/rollshot-action/src/export/html.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

**Interfaces:**
- Consumes: `ImportWarning` and timeline `import_warnings`.
- Produces: Action Guide session schema v2 plus Markdown/HTML/Issue Pack disclosure; does not add a video attachment.

- [ ] **Step 1: Write failing export compatibility and disclosure tests**

```rust
#[test]
fn v1_session_defaults_to_empty_import_warnings() {
    let parsed: SessionManifest = serde_json::from_str(V1_SESSION_JSON).unwrap();
    assert!(parsed.import_warnings.is_empty());
}

#[test]
fn session_loader_rejects_unknown_versions_and_v2_values_in_v1() {
    let future: SessionManifest = serde_json::from_str(SESSION_V99_JSON).unwrap();
    assert!(future.validate().is_err());
    let legacy_with_warnings: SessionManifest = serde_json::from_str(V1_SESSION_WITH_WARNINGS_JSON).unwrap();
    assert!(legacy_with_warnings.validate().is_err());
}

#[test]
fn imported_session_and_reader_disclose_reduction() {
    let job = imported_job(vec![ImportWarning::IntermediateChangesReduced]);
    let root = render_fixture(job);
    let session = std::fs::read_to_string(root.join("session.json")).unwrap();
    let markdown = std::fs::read_to_string(root.join("steps.md")).unwrap();
    let html = std::fs::read_to_string(root.join("index.html")).unwrap();
    assert!(session.contains("intermediate-changes-reduced"));
    assert!(markdown.contains("Intermediate visual changes were omitted"));
    assert!(html.contains("Intermediate visual changes were omitted"));
}

#[test]
fn issue_pack_discloses_import_limits_without_attaching_video() {
    let pack = build_imported_issue_pack();
    assert!(pack.issue_markdown.contains("visual changes"));
    assert!(pack.manifest.warnings.iter().any(|w| w.code == "intermediate-changes-reduced"));
    assert!(!pack.manifest.attachments.iter().any(|a| is_video_path(&a.path)));
}
```

- [ ] **Step 2: Run export tests and confirm failure**

Run: `rtk cargo test -p rollshot-action export::`

Run: `rtk cargo test -p rollshot-app issue_pack --features action-guide`

Expected: compilation or assertions fail because warning fields and disclosure text are absent.

- [ ] **Step 3: Advance the session schema and carry warnings**

Set `GUIDE_SCHEMA_VERSION` to `2`. Add `pub import_warnings: Vec<ImportWarning>` to `ReviewedGuideExportJob` and to `SessionManifest`, with `#[serde(default, skip_serializing_if = "Vec::is_empty")]` on the manifest field. Update every existing job fixture with an empty vector. `build_reviewed_export_job` clones warnings from the timeline.

Add a single `render_import_notices(&[ImportWarning]) -> String` helper in `export/mod.rs`; use equivalent structured data in `html.rs`/`viewer.html` so the offline reader shows notices before steps. Keep the v1 loader path accepting absent warnings as empty.

Add `SessionManifest::validate` mirroring the project-side dispatch (D4): reject any declared `schema_version` newer than `GUIDE_SCHEMA_VERSION`, and reject v2-only fields (a non-empty `import_warnings`) under an effective legacy version. Call it after every Rust-side session parse, including the publish path in `project/publish.rs`.

- [ ] **Step 4: Map the same warnings into Issue Pack**

Map `NoVisualChangesDetected` to code `no-visual-changes-detected` and `IntermediateChangesReduced` to code `intermediate-changes-reduced`. Insert short notices before reproduction steps in `issue.md`; reuse the existing `IssuePackWarning` manifest array. Do not add an attachment or source field. Keep evidence review/redaction gates unchanged.

- [ ] **Step 5: Run export, publishing, and issue-pack tests**

Run: `rtk cargo test -p rollshot-action`

Run: `rtk cargo test -p rollshot-app --features action-guide`

Expected: v1 sessions load empty, v2 exports contain provenance/warnings, all three human-readable artifacts disclose limitations, and no video/source identifier appears.

- [ ] **Step 6: Commit export disclosure**

```bash
rtk git add crates/rollshot-action/src/export crates/rollshot-action/src/project/publish.rs crates/rollshot-app/src/timeline_workspace/guide_export.rs crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(action-guide): disclose imported video limits"
```

---

### Task 8: Shared Import Coordinator and Home Processing UI

**Files:**
- Create: `crates/rollshot-app/src/action_guide_home/video_import.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/mod.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/update.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/view.rs`
- Test: `crates/rollshot-app/src/action_guide_home/video_import.rs`
- Test: `crates/rollshot-app/src/action_guide_home/update.rs`

**Interfaces:**
- Consumes: paired resolver, `import_video`, cancellation/progress/result types, and `TimelineWorkspace::from_imported_video`.
- Produces: shared home effects/messages/state used unchanged by Linux and macOS adapters.

- [ ] **Step 1: Invoke iced-rs and write failing coordinator tests**

Before editing any iced file, read and follow the `iced-rs` skill. Add state tests:

```rust
#[test]
fn picker_cancel_is_silent() {
    let mut home = ActionGuideHome::new_empty();
    home.update(Message::ImportRecording);
    home.update(Message::ImportPickerCancelled);
    assert_eq!(home.import.state(), ImportState::Idle);
    assert!(home.message.is_none());
}

#[test]
fn cancelled_or_superseded_operation_ignores_late_messages() {
    let mut coordinator = ImportCoordinator::default();
    let old = coordinator.begin(PathBuf::from("old.mp4"));
    coordinator.cancel(old);
    let new = coordinator.begin(PathBuf::from("new.mp4"));
    coordinator.progress(old, progress(VideoImportPass::Extract));
    assert_eq!(coordinator.operation_id(), Some(new));
    assert_ne!(coordinator.state(), ImportState::ExtractingPass2);
}

#[test]
fn success_produces_unsaved_timeline_effect() {
    let update = finish_successful_import();
    assert!(matches!(update.effect, Effect::OpenImportedTimeline(_)));
}
```

- [ ] **Step 2: Run home tests and confirm failure**

Run: `rtk cargo test -p rollshot-app action_guide_home --features action-guide`

Expected: compilation fails because the coordinator and import messages do not exist.

- [ ] **Step 3: Implement the coordinator state machine**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportState { Idle, Picking, ResolvingToolchain, SettingUp, Preflight, AnalyzingPass1, ExtractingPass2 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImportOperationId(u64);

pub struct ImportCoordinator {
    state: ImportState,
    operation_id: Option<ImportOperationId>,
    next_operation_id: u64,
    cancellation: Option<VideoImportCancellation>,
    progress: Option<VideoImportProgress>,
    pending: Option<(ImportOperationId, PathBuf)>,
}
```

Add home messages `ImportRecording`, `ImportPickerSelected(PathBuf)`, `ImportPickerCancelled`, `ImportToolchainResolved { operation_id, resolution }`, `ImportSetupFinished { operation_id, result }`, `RetryImportSetup`, `ImportProgress { operation_id, progress }`, `ImportFinished { operation_id, result }`, and `CancelImport`. Add effects `PickRecording`, `StartImport { operation_id, path, cancellation }`, `SetupImportToolchain`, and `OpenImportedTimeline(ImportedWorkspaceSeed)`. The `pending` field keeps the selected recording across the resolve → setup/install → retry/cancel sequence (D3): `resolve_video_import_toolchain()` runs first, `NeedsSetup` drives `SetupImportToolchain` plus `RetryImportSetup`, and only an `Available` pair leads to `StartImport`.

The seed moves by value exactly once through `ImportFinished` → `OpenImportedTimeline` → `TimelineWorkspace::from_imported_video`; it is never cloned (D2). Stale or superseded `ImportFinished` messages drop their seed immediately at the handler. Because the seed is non-`Clone`, remove `Clone` from the home `Message` enum and remove `Clone`/`PartialEq`/`Eq` from `Effect`; provide a manual privacy-safe `Debug` for `Effect` (variant names and operation IDs only) and for `ImportedWorkspaceSeed` (no scratch or source paths). Convert the eleven existing `assert_eq!(update.effect, ...)` assertions in this module to `matches!`. Task 9 drops the then-uncompilable `Clone` derive from the Linux and macOS product `Message` enums — no call site clones messages, and iced does not require it.

The worker is launched with `Task::run`/a bounded channel so progress messages arrive during `spawn_blocking(import_video)` rather than only at completion. Every progress/completion handler checks the operation ID first. Cancel calls the token; it does not merely drop the iced task.

- [ ] **Step 4: Implement picker and processing view**

Add `Import Recording…` between `Record New` and `Open Project…`. The picker filters `mp4`, `mov`, `mkv`, and `webm`; unsupported selected extensions return a recoverable home message before tool setup.

When the coordinator is active, replace the home body with pass copy, `processed source time / total duration`, retained count, `Processing stays on this device. Audio is ignored.`, and one Cancel button. Do not show an ETA. Map worker categories to actionable copy without embedding paths or filenames.

- [ ] **Step 5: Run coordinator and view tests**

Run: `rtk cargo test -p rollshot-app action_guide_home --features action-guide`

Expected: picker cancellation is silent, setup retry works, progress transitions are correct, cancel/stale events are ignored, failure returns to Home, and success yields an imported timeline effect.

- [ ] **Step 6: Commit shared coordinator/UI**

```bash
rtk git add crates/rollshot-app/src/action_guide_home
rtk git commit -m "feat(action-guide): add video import coordinator"
```

---

### Task 9: Linux and macOS Product Wiring

**Files:**
- Modify: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Test: `crates/rollshot-app/src/macos_product.rs`

**Interfaces:**
- Consumes: Task 8 shared effects/messages, paired toolchain setup flow, and imported timeline constructor.
- Produces: identical discoverability and lifecycle on both active product paths.

- [ ] **Step 1: Write failing adapter-path tests**

```rust
#[test]
fn linux_home_import_success_enters_timeline() {
    let mut state = linux_home_state();
    drive_import_success(&mut state);
    assert_eq!(state.phase, Phase::Timeline);
    assert!(state.timeline.as_ref().unwrap().project_recent_metadata().is_none());
}

#[test]
fn macos_home_import_success_opens_workspace_window() {
    let mut product = macos_home_product();
    drive_import_success(&mut product);
    assert!(matches!(product.phase, Phase::Timeline(_)));
    assert!(product.workspace_window.is_some());
}
```

- [ ] **Step 2: Run product-path tests and confirm missing effect handling**

Run: `rtk cargo test -p rollshot-app --features action-guide`

Expected: exhaustive effect matches fail or tests fail because import effects are not wired.

- [ ] **Step 3: Wire Linux effects and completion**

In `action_guide_linux_product.rs`, map `PickRecording` to the native async file dialog, resolve/setup the paired toolchain before `StartImport`, forward worker messages to `Message::Home`, and convert `OpenImportedTimeline(seed)` via `TimelineWorkspace::from_imported_video`. Set `Phase::Timeline`, store the workspace, and start its initial frame-load task. Do not record it in Recents. Drop the `Clone` derive from this file's `Message` enum; the wrapped home message is no longer `Clone` (D9).

- [ ] **Step 4: Wire macOS effects and completion**

In `macos_product.rs`, apply the same shared effects. On success, preserve the existing Home→Timeline window transition: create the imported workspace, open `workspace_window_settings()`, set `workspace_window`, and batch the window-open and initial-frame-load tasks. Keep macOS recording and ScreenCaptureKit paths untouched. Drop the `Clone` derive from this file's `Message` enum; the wrapped home message is no longer `Clone` (D9).

- [ ] **Step 5: Run both platform-path test suites**

Run: `rtk cargo test -p rollshot-app --features action-guide`

Expected: both paths expose the import action, share operation behavior, open an unsaved timeline, and never add scratch to Recents.

- [ ] **Step 6: Commit cross-platform wiring**

```bash
rtk git add crates/rollshot-app/src/action_guide_linux_product.rs crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(action-guide): wire video import on linux and macos"
```

---

### Task 10: Privacy, Cleanup, and End-to-End Verification

**Files:**
- Modify: `crates/rollshot-action/src/video_import/mod.rs`
- Modify: `crates/rollshot-action/src/video_import/scratch.rs`
- Modify: `crates/rollshot-app/src/action_guide_home/video_import.rs`
- Modify: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-action/src/video_import/mod.rs`
- Test: `crates/rollshot-app/src/action_guide_home/video_import.rs`

**Interfaces:**
- Consumes: the complete import path.
- Produces: startup stale-scratch cleanup, sentinel privacy tests, full verification evidence, and manual cross-platform checklist.

- [ ] **Step 1: Add sentinel privacy and terminal cleanup tests**

```rust
#[test]
fn persisted_and_exported_artifacts_never_contain_source_identity() {
    let sentinel = "SECRET-customer-recording-8f7d.mp4";
    let artifacts = import_save_and_export_fixture(sentinel);
    for bytes in artifacts.persisted_bytes() {
        assert!(!String::from_utf8_lossy(bytes).contains(sentinel));
    }
}

#[test]
fn every_terminal_outcome_reaps_children_and_removes_scratch() {
    for outcome in [ProbeFailure, Pass1Failure, Pass2Failure, Cancelled] {
        let result = run_fault_injected_import(outcome);
        assert!(result.scratch_paths().iter().all(|path| !path.exists()));
        assert_eq!(result.live_child_count(), 0);
    }
}
```

Capture tracing output with a test subscriber and assert the sentinel path, filename, decoded pixels, and child stderr do not appear. Assert scratch contains only lock/staging/PNG project assets during processing and only content-addressed PNG assets on success.

- [ ] **Step 2: Run privacy and cleanup tests**

Run: `rtk cargo test -p rollshot-action video_import`

Run: `rtk cargo test -p rollshot-app --features action-guide`

Expected: all tests pass with no leaked sentinel and no surviving child/scratch on terminal failures.

- [ ] **Step 3: Wire stale scratch cleanup at product startup**

Call `cleanup_stale_import_scratch` once during Action Guide product initialization on Linux and macOS, using the same app-owned scratch parent passed to imports. The cleanup call must be best-effort, log only `removed_count`/error category, and skip directories whose exclusive lock cannot be acquired.

- [ ] **Step 4: Run workspace verification**

Run: `rtk cargo fmt --check`

Expected: exit 0.

Run: `rtk cargo test`

Expected: exit 0 for default workspace members.

Run: `rtk cargo test -p rollshot-action`

Expected: exit 0.

Run: `rtk cargo test -p rollshot-app --features action-guide`

Expected: exit 0.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: exit 0 with no warnings.

- [ ] **Step 5: Perform native Linux runtime verification**

Run the Action Guide-enabled app on Linux and verify: picker filters, static fallback, multi-change import, pass progress, cancel during each pass, no orphan decoder, timeline warning, first save, reopen, Action Guide export, Issue Pack export, and no source video/audio in outputs. Record the tested compositor/backend and any unverified runtime limitation in the handoff.

- [ ] **Step 6: Perform native macOS runtime verification**

Run the same checklist on macOS, including native picker and workspace-window transition. If macOS hardware is unavailable, explicitly report this path as compile/test verified but not runtime verified; do not claim full cross-platform completion.

- [ ] **Step 7: Commit final hardening**

```bash
rtk git add crates/rollshot-action/src/video_import crates/rollshot-app/src/action_guide_home/video_import.rs crates/rollshot-app/src/action_guide_linux_product.rs crates/rollshot-app/src/macos_product.rs
rtk git commit -m "test(action-guide): verify private video import cleanup"
```

---

## Completion Gate

Before declaring the feature complete, invoke `superpowers:verification-before-completion`, run the Task 10 commands from a clean status, and confirm all seven approved success criteria against actual output. Then invoke `superpowers:requesting-code-review`; address findings with `superpowers:receiving-code-review`. Do not stage or commit the unrelated `learn-projects/claude-video/` directory.
