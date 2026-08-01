# Launch Teaser Domain and Renderer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the headless typed launch-teaser plan, deterministic seed, derived sidecar, and fixed cancellable FFmpeg renderer to `rollshot-action`.

**Architecture:** A new `launch_teaser` module owns a strict V1 plan and validation, deterministic seed generation from a loaded project, derived-artifact persistence, text-overlay rasterization, and preview/final rendering. The renderer accepts only validated typed values, compiles a fixed FFmpeg graph, verifies the temporary MP4 with ffprobe, and atomically renames final output.

**Tech Stack:** Rust, serde, SHA-256, `image`, `rollshot-image-document` vendored fonts, FFmpeg/ffprobe through `VideoToolchain`, existing `PublishCancellation` and project-continuity contracts.

## Global Constraints

- Output is fixed to 1920×1080, 30 fps, H.264/yuv420p, 15–25 seconds, and zero audio streams.
- Preview output is fixed to 960×540 and uses the same validated plan and operation graph.
- Plans contain exactly 3–5 ordered, non-overlapping reviewed-step shots.
- Times, normalized coordinates, zoom, and speed are bounded integers; serialized floats are prohibited.
- User/model strings never become paths, arguments, filter names, expressions, codecs, or graph fragments.
- Text is rasterized to Rollshot-owned PNG overlays before FFmpeg invocation.
- Render revalidates project revision, projection digest, and motion identity before process launch.
- Cancellation removes scratch and temporary output and never leaves a false-success destination.
- Accepted plan provenance is a derived sidecar at `publish/launch-teaser-plan-v1.json`; writing it does not increment the guide revision.
- Do not change the existing reviewed-keyframe summary MP4 behavior.
- Prefix every shell command with `rtk`.

---

### Task 1: Typed plan and validation contract

**Files:**
- Create: `crates/rollshot-action/src/launch_teaser/mod.rs`
- Create: `crates/rollshot-action/src/launch_teaser/plan.rs`
- Create: `crates/rollshot-action/src/launch_teaser/error.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

**Interfaces:**
- Consumes: `project::ProjectStepId`.
- Produces:
  - `LaunchTeaserPlanV1::validate(&self) -> Result<ValidatedLaunchTeaserPlan, LaunchTeaserError>`
  - `ValidatedLaunchTeaserPlan::plan(&self) -> &LaunchTeaserPlanV1`
  - `ValidatedLaunchTeaserPlan::duration_ms(&self) -> u64`
  - `LaunchTeaserError` with stable `category() -> &'static str`.

- [ ] **Step 1: Write failing plan-contract tests**

Add tests inside `plan.rs` for a valid three-shot fixture and each externally observable boundary:

```rust
#[test]
fn valid_three_shot_plan_reports_exact_duration() {
    let plan = valid_plan();
    let validated = plan.validate().unwrap();
    assert_eq!(validated.duration_ms(), 15_000);
}

#[test]
fn plan_rejects_two_shots() {
    let mut plan = valid_plan();
    plan.shots.pop();
    assert_eq!(plan.validate().unwrap_err().category(), "shot-count");
}

#[test]
fn plan_rejects_overlapping_source_ranges() {
    let mut plan = valid_plan();
    plan.shots[1].source_start_ms = plan.shots[0].source_end_ms - 1;
    assert_eq!(plan.validate().unwrap_err().category(), "source-range");
}

#[test]
fn unknown_json_field_is_rejected() {
    let mut value = serde_json::to_value(valid_plan()).unwrap();
    value.as_object_mut().unwrap().insert("filtergraph".into(), serde_json::json!("evil"));
    assert!(serde_json::from_value::<LaunchTeaserPlanV1>(value).is_err());
}
```

Also cover schema version, 3/5/6 shots, unknown step IDs, duplicate step IDs, source bounds, focus coordinate bounds, zoom bounds, allowed speeds, transition bounds, text byte/character ceilings, duration below 15 seconds, duration above 25 seconds, and lowercase canonical SHA-256 fields.

- [ ] **Step 2: Run the focused test and observe failure**

Run: `rtk cargo test -p rollshot-action launch_teaser::plan::tests -- --nocapture`
Expected: FAIL because `launch_teaser` and its types do not exist.

- [ ] **Step 3: Implement strict DTOs and validated wrapper**

Define the public contract in `plan.rs`:

```rust
pub const LAUNCH_TEASER_SCHEMA_VERSION: u32 = 1;
pub const FINAL_WIDTH: u32 = 1920;
pub const FINAL_HEIGHT: u32 = 1080;
pub const FINAL_FPS: u32 = 30;
pub const MIN_DURATION_MS: u64 = 15_000;
pub const MAX_DURATION_MS: u64 = 25_000;
pub const MIN_SHOTS: usize = 3;
pub const MAX_SHOTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedPointV1 {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusPathV1 {
    pub start: NormalizedPointV1,
    pub end: NormalizedPointV1,
    pub zoom_permille: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedV1 { P750, P1000, P1250, P1500, P2000 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransitionV1 {
    Cut,
    Crossfade { duration_ms: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserSourceV1 {
    pub project_revision: u64,
    pub projection_digest: String,
    pub motion_sha256: String,
    pub motion_duration_ms: u64,
    pub motion_width: u32,
    pub motion_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserShotV1 {
    pub reviewed_step_id: ProjectStepId,
    pub source_start_ms: u64,
    pub source_end_ms: u64,
    pub focus_path: FocusPathV1,
    pub speed: SpeedV1,
    pub caption: String,
    pub transition: TransitionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryReadProvenanceV1 {
    pub relative_path: String,
    pub content_sha256: String,
    pub bytes_read: u64,
    pub bytes_returned: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProvenanceV1 {
    pub run_id: String,
    pub skill_package_digest: String,
    pub authority_snapshot_digest: String,
    pub repository_grant_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedEditV1 {
    pub field_path: String,
    pub source: AcceptedEditSourceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptedEditSourceV1 { DeterministicSeed, Agent, User }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserProvenanceV1 {
    pub deterministic_seed_version: u32,
    pub agent: Option<AgentProvenanceV1>,
    pub repository_reads: Vec<RepositoryReadProvenanceV1>,
    pub accepted_user_edits: Vec<AcceptedEditV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserPlanV1 {
    pub schema_version: u32,
    pub source: LaunchTeaserSourceV1,
    pub hook: String,
    pub shots: Vec<LaunchTeaserShotV1>,
    pub outro_text: String,
    pub provenance: LaunchTeaserProvenanceV1,
}
```

Use normalized coordinates `0..=10_000`, zoom `1_000..=2_000`, crossfade `100..=750 ms`, hook/outro at most 256 bytes and 120 characters, caption at most 512 bytes and 240 characters. Calculate each displayed shot duration as `(source_duration_ms * 1_000) / speed_permille`; subtract crossfade overlap between adjacent shots. Reject arithmetic overflow.

`ValidatedLaunchTeaserPlan` keeps fields private and is constructible only by `validate`.

- [ ] **Step 4: Implement stable errors and exports**

In `error.rs`, define variants and categories without private content:

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchTeaserError {
    #[error("unsupported launch teaser schema")]
    UnsupportedSchema,
    #[error("invalid launch teaser shot count")]
    ShotCount,
    #[error("invalid launch teaser source binding")]
    SourceBinding,
    #[error("invalid launch teaser source range")]
    SourceRange,
    #[error("invalid launch teaser focus path")]
    FocusPath,
    #[error("invalid launch teaser speed")]
    Speed,
    #[error("invalid launch teaser transition")]
    Transition,
    #[error("invalid launch teaser text")]
    Text,
    #[error("invalid launch teaser duration")]
    Duration,
    #[error("launch teaser arithmetic overflow")]
    ArithmeticOverflow,
}
```

Map categories exactly to `unsupported-schema`, `shot-count`, `source-binding`, `source-range`, `focus-path`, `speed`, `transition`, `text`, `duration`, and `arithmetic-overflow`.

Export the module and public DTOs from `launch_teaser/mod.rs`; add `pub mod launch_teaser;` to `lib.rs`.

- [ ] **Step 5: Run focused tests**

Run: `rtk cargo test -p rollshot-action launch_teaser::plan::tests -- --nocapture`
Expected: all plan tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/lib.rs crates/rollshot-action/src/launch_teaser
rtk git commit -m "feat(action-guide): add launch teaser plan contract"
```

---

### Task 2: Project binding and deterministic seed

**Files:**
- Create: `crates/rollshot-action/src/launch_teaser/seed.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/mod.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/error.rs`

**Interfaces:**
- Consumes: `LoadedProject`, `ActionGuideContextProjectionV1`, `MotionAssetLoad::Available`, and Task 1 plan types.
- Produces:
  - `seed_launch_teaser(loaded: &LoadedProject) -> Result<LaunchTeaserPlanV1, LaunchTeaserSeedError>`
  - `validate_launch_teaser_binding(plan: &LaunchTeaserPlanV1, loaded: &LoadedProject) -> Result<(), LaunchTeaserBindingError>`
  - `DETERMINISTIC_SEED_VERSION: u32 = 1`.

- [ ] **Step 1: Write failing deterministic-selection and stale-binding tests**

Create saved-project fixtures with real manifest steps and a test motion asset. Assert:

```rust
#[test]
fn seed_keeps_first_last_and_evenly_samples_interior() {
    let loaded = loaded_project_with_steps(8);
    let plan = seed_launch_teaser(&loaded).unwrap();
    let ids: Vec<u64> = plan.shots.iter().map(|shot| shot.reviewed_step_id.0).collect();
    assert_eq!(ids, vec![1, 2, 4, 6, 8]);
}

#[test]
fn binding_rejects_changed_motion_digest() {
    let loaded = loaded_project_with_steps(3);
    let mut plan = seed_launch_teaser(&loaded).unwrap();
    plan.source.motion_sha256 = "f".repeat(64);
    assert_eq!(
        validate_launch_teaser_binding(&plan, &loaded).unwrap_err().category(),
        "stale-motion"
    );
}
```

Also test 0–2 steps, exactly 3, exactly 5, more than 5, insufficient non-overlapping source windows, changed revision, changed projection digest, unavailable motion, and deterministic repeatability.

- [ ] **Step 2: Run the seed tests and observe failure**

Run: `rtk cargo test -p rollshot-action launch_teaser::seed::tests -- --nocapture`
Expected: FAIL because seed APIs do not exist.

- [ ] **Step 3: Implement deterministic selection and window allocation**

Use this exact selection rule:

```rust
fn selected_indices(step_count: usize) -> Vec<usize> {
    let wanted = step_count.min(MAX_SHOTS);
    if wanted == step_count {
        return (0..step_count).collect();
    }
    (0..wanted)
        .map(|slot| slot * (step_count - 1) / (wanted - 1))
        .collect()
}
```

Use target displayed durations `[5_000, 5_000, 5_000]` for three shots, `[4_000; 4]` for four shots, and `[3_500; 5]` for five shots. The fixed 1,500 ms outro is an overlay within the last shot and adds no duration; each default plan therefore lasts 15, 16, or 17.5 seconds. Allocate each source window around `step.at_ms`, clamp to motion bounds, and then shift adjacent windows without reordering so they do not overlap. If the target windows cannot be allocated, return `InsufficientMotion` with no partial plan.

Defaults:

```rust
const DEFAULT_FOCUS: FocusPathV1 = FocusPathV1 {
    start: NormalizedPointV1 { x: 5_000, y: 5_000 },
    end: NormalizedPointV1 { x: 5_000, y: 5_000 },
    zoom_permille: 1_000,
};
```

Use `SpeedV1::P1000`, `TransitionV1::Cut`, project title as hook, each reviewed step caption or title as caption, and `"Made with Rollshot"` as outro. Set provenance to seed version 1 with no agent, reads, or accepted user edits.

- [ ] **Step 4: Implement source binding validation**

Rebuild `ActionGuideContextProjectionV1` from `LoadedProject`; compare revision and digest. Compare the available `ValidatedMotionAsset` SHA-256, duration, width, and height. Confirm every referenced `ProjectStepId` still exists. Return separate privacy-safe categories `stale-project`, `stale-motion`, and `missing-step`.

- [ ] **Step 5: Run seed and plan tests**

Run: `rtk cargo test -p rollshot-action launch_teaser -- --nocapture`
Expected: all Task 1–2 tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/launch_teaser
rtk git commit -m "feat(action-guide): seed launch teaser plans"
```

---

### Task 3: Derived sidecar persistence

**Files:**
- Create: `crates/rollshot-action/src/launch_teaser/persistence.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/mod.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/error.rs`

**Interfaces:**
- Consumes: validated plan from Task 1 and a project root.
- Produces:
  - `write_launch_teaser_sidecar(project_root: &Path, artifact: &LaunchTeaserArtifactV1) -> Result<(), LaunchTeaserPersistenceError>`
  - `load_launch_teaser_sidecar(project_root: &Path) -> LaunchTeaserSidecarLoad`
  - canonical relative path `publish/launch-teaser-plan-v1.json`.

- [ ] **Step 1: Write failing atomicity and freshness tests**

Test successful round trip, unknown fields, malformed JSON, mismatched plan digest, missing sidecar, no project revision increment, stale freshness after a manifest revision change, and preservation of the previous sidecar when the atomic write fails before rename.

```rust
#[test]
fn sidecar_write_does_not_change_project_revision() {
    let fixture = saved_project_fixture();
    let before = load_project(fixture.path(), None).unwrap().manifest.revision;
    write_launch_teaser_sidecar(fixture.path(), &artifact_fixture()).unwrap();
    let after = load_project(fixture.path(), None).unwrap().manifest.revision;
    assert_eq!(before, after);
}
```

- [ ] **Step 2: Run persistence tests and observe failure**

Run: `rtk cargo test -p rollshot-action launch_teaser::persistence::tests -- --nocapture`
Expected: FAIL because persistence APIs do not exist.

- [ ] **Step 3: Implement the sidecar DTO and digest**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserArtifactV1 {
    pub schema_version: u32,
    pub plan: LaunchTeaserPlanV1,
    pub plan_sha256: String,
    pub renderer_version: u32,
    pub ffmpeg_version: String,
    pub ffprobe_version: String,
    pub output_sha256: String,
    pub rendered_at_unix_ms: i64,
}
```

Compute `plan_sha256` from canonical `serde_json::to_vec(&plan)` with domain separator `rollshot-launch-teaser-plan-v1\0`. Reject noncanonical digests and any stored digest mismatch.

- [ ] **Step 4: Implement atomic write and fail-closed load**

Create `publish/`, write a unique temp sibling with `create_new`, `sync_all`, rename to `launch-teaser-plan-v1.json`, and fsync `publish/`. Follow the existing project atomic-write pattern. `load_launch_teaser_sidecar` returns `Missing`, `Available`, `Stale`, or `Unavailable`; it never prevents the Action Guide project itself from opening.

Freshness compares current project revision, projection digest, and motion identity through `validate_launch_teaser_binding`.

- [ ] **Step 5: Run persistence tests**

Run: `rtk cargo test -p rollshot-action launch_teaser::persistence::tests -- --nocapture`
Expected: all persistence tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/launch_teaser
rtk git commit -m "feat(action-guide): persist launch teaser provenance"
```

---

### Task 4: Fixed render graph and deterministic text overlays

**Files:**
- Create: `crates/rollshot-action/src/launch_teaser/overlay.rs`
- Create: `crates/rollshot-action/src/launch_teaser/graph.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/mod.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/error.rs`

**Interfaces:**
- Consumes: `ValidatedLaunchTeaserPlan`, output profile, scratch directory.
- Produces:
  - `RenderProfile::{Preview, Final}`
  - `prepare_overlay_assets(plan: &ValidatedLaunchTeaserPlan, scratch: &Path, profile: RenderProfile) -> Result<Vec<OverlayAsset>, LaunchTeaserRenderError>`
  - `compile_ffmpeg_graph(plan: &ValidatedLaunchTeaserPlan, motion_path: &Path, overlays: &[OverlayAsset], output_path: &Path, profile: RenderProfile) -> Result<CompiledLaunchTeaserGraph, LaunchTeaserRenderError>`
  - `CompiledLaunchTeaserGraph::args(&self) -> &[OsString]`.

- [ ] **Step 1: Write failing graph-safety and overlay tests**

Tests must assert behavior, not source strings:

```rust
#[test]
fn hostile_caption_never_enters_ffmpeg_arguments() {
    let mut plan = valid_plan();
    plan.shots[0].caption = "x'];movie=/etc/passwd['y".into();
    let validated = plan.validate().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let assets = prepare_overlay_assets(&validated, scratch.path(), RenderProfile::Final).unwrap();
    let graph = compile_ffmpeg_graph(&validated, motion_path(), &assets, output_path(), RenderProfile::Final).unwrap();
    let joined = graph.args().iter().map(|s| s.to_string_lossy()).collect::<String>();
    assert!(!joined.contains("/etc/passwd"));
}
```

Also verify preview/final dimensions, unique overlay files, text byte ceilings before allocation, 16:9 crop geometry remaining in source bounds, numeric focus interpolation, speed-to-PTS conversion, cut vs crossfade topology, and zero audio mapping.

- [ ] **Step 2: Run graph tests and observe failure**

Run:

```bash
rtk cargo test -p rollshot-action launch_teaser::graph::tests -- --nocapture
rtk cargo test -p rollshot-action launch_teaser::overlay::tests -- --nocapture
```
Expected: FAIL because graph and overlay modules do not exist.

- [ ] **Step 3: Implement overlay rasterization**

Allocate transparent RGBA images at the output profile dimensions. Render fixed black plates and white text through `rollshot_image_document::draw_text_block` with the existing vendored DejaVu font path. Use fixed margins, line height, and maximum plate width. Hook appears on the first shot, each caption on its shot, and outro on the final 1,500 ms. Save files as generated names such as `overlay-000.png`; never derive filenames from text.

- [ ] **Step 4: Implement fixed graph compilation**

`CompiledLaunchTeaserGraph` owns `Vec<OsString>` and a Rollshot-generated filter graph. Generate labels by shot index only. Each shot performs trim, `setpts`, numeric crop interpolation, scale/pad, and PNG overlay. Concatenate cuts and use `xfade` only for validated crossfades. Add `-an`, H.264, yuv420p, fixed frame rate, and profile dimensions.

Store the generated graph in a scratch file with a Rollshot-generated filename and pass that path as an argument. No user/model text enters the graph because all text is already rasterized.

- [ ] **Step 5: Run graph tests**

Run:

```bash
rtk cargo test -p rollshot-action launch_teaser::graph::tests -- --nocapture
rtk cargo test -p rollshot-action launch_teaser::overlay::tests -- --nocapture
```
Expected: all graph and overlay tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/launch_teaser
rtk git commit -m "feat(action-guide): compile fixed teaser render graph"
```

---

### Task 5: Cancellable FFmpeg render and ffprobe verification

**Files:**
- Create: `crates/rollshot-action/src/launch_teaser/render.rs`
- Create: `crates/rollshot-action/src/launch_teaser/probe.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/mod.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/error.rs`

**Interfaces:**
- Consumes: loaded project, validated plan, `VideoToolchain`, `PublishCancellation`, destination, and Task 4 graph.
- Produces:
  - `render_launch_teaser(request: LaunchTeaserRenderRequest<'_>) -> Result<LaunchTeaserRenderResult, LaunchTeaserRenderError>`
  - `verify_launch_teaser_output(ffprobe: &Path, output: &Path, expected: &ValidatedLaunchTeaserPlan, profile: RenderProfile) -> Result<VerifiedLaunchTeaserOutput, LaunchTeaserRenderError>`.

- [ ] **Step 1: Write failing synthetic-video integration tests**

Use the test FFmpeg toolchain to generate a 30-second silent H.264 source with moving color blocks. Create a loaded project bound to it. Test final and preview profiles, codec, dimensions, 30 fps, duration tolerance, no audio, source-binding rejection before spawn, and cancellation leaving no destination.

```rust
#[test]
fn final_render_produces_verified_silent_mp4() {
    if !ffmpeg_available() { return; }
    let fixture = synthetic_motion_project();
    let plan = seed_launch_teaser(&fixture.loaded).unwrap();
    let result = render_launch_teaser(LaunchTeaserRenderRequest {
        loaded: &fixture.loaded,
        plan: &plan,
        toolchain: &fixture.toolchain,
        cancellation: &PublishCancellation::new(),
        destination: &fixture.output,
        profile: RenderProfile::Final,
    }).unwrap();
    assert_eq!(result.width, 1920);
    assert_eq!(result.height, 1080);
    assert_eq!(result.audio_streams, 0);
    assert!(fixture.output.is_file());
}
```

- [ ] **Step 2: Run render integration tests and observe failure**

Run: `rtk cargo test -p rollshot-action launch_teaser::render::tests -- --nocapture`
Expected: FAIL because render APIs do not exist.

- [ ] **Step 3: Implement toolchain preflight and rendering**

Before creating destination output:

1. check cancellation;
2. rebuild the project projection and validate binding;
3. validate the plan;
4. verify FFmpeg and ffprobe paths are regular executables;
5. query FFmpeg filters and require the exact fixed filter set used by Task 4;
6. create one owned scratch directory and a unique destination temp sibling;
7. rasterize overlays and compile the graph;
8. spawn FFmpeg with `-nostdin` and piped/null standard streams;
9. wait with periodic cancellation checks;
10. on cancellation, terminate the process tree and wait;
11. verify the temporary output with ffprobe;
12. sync and atomically rename only for `RenderProfile::Final`.

Preview returns its scratch-owned path through a `LaunchTeaserPreview` guard whose `Drop` removes the scratch directory. Final render returns digest and metadata after rename.

- [ ] **Step 4: Implement strict ffprobe decoding**

Decode JSON into private `#[serde(deny_unknown_fields)]` DTOs only for required fields. Require exactly one H.264 video stream, no audio streams, profile dimensions, 30 fps, and duration within one frame of the validated plan. Reject NaN, unknown frame-rate forms, extra video streams, or missing duration.

- [ ] **Step 5: Run render integration tests**

Run: `rtk cargo test -p rollshot-action launch_teaser::render::tests -- --nocapture`
Expected: all available-toolchain tests PASS; tests skip only when the explicit test preflight reports FFmpeg unavailable.

- [ ] **Step 6: Run the whole crate suite**

Run: `rtk cargo test -p rollshot-action`
Expected: all tests PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/launch_teaser
rtk git commit -m "feat(action-guide): render validated launch teasers"
```

---

### Task 6: Headless acceptance harness and public API cleanup

**Files:**
- Create: `crates/rollshot-action/tests/launch_teaser_contract.rs`
- Modify: `crates/rollshot-action/src/launch_teaser/mod.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

**Interfaces:**
- Consumes: all Tasks 1–5.
- Produces: the stable API consumed by the agent and UI plans.

- [ ] **Step 1: Add an external-crate acceptance test**

The integration test imports only public APIs. It creates a real project with persistent synthetic motion and three reviewed steps, seeds a plan, edits one bounded caption, renders a preview and final MP4, verifies output, writes the sidecar, reloads it as current, increments the project revision, and reloads it as stale.

```rust
#[test]
fn provider_free_launch_teaser_contract() {
    if !test_toolchain_available() { return; }
    let fixture = public_project_fixture();
    let mut plan = seed_launch_teaser(&fixture.loaded).unwrap();
    plan.shots[0].caption = "Review the first step".into();
    plan.provenance.accepted_user_edits.push(AcceptedEditV1 {
        field_path: "shots[0].caption".into(),
        source: AcceptedEditSourceV1::User,
    });
    plan.validate().unwrap();
    let result = render_launch_teaser(fixture.render_request(&plan)).unwrap();
    assert_eq!(result.audio_streams, 0);
}
```

- [ ] **Step 2: Run the acceptance test**

Run: `rtk cargo test -p rollshot-action --test launch_teaser_contract -- --nocapture`
Expected: PASS when FFmpeg is available; explicit preflight skip otherwise.

- [ ] **Step 3: Restrict exports to the approved contract**

Export DTOs, validation, seed, binding, render, verification result, and sidecar APIs from `launch_teaser/mod.rs`. Keep graph builders, probe DTOs, scratch guards, overlay helpers, and process helpers private.

- [ ] **Step 4: Run formatting and crate verification**

Run:

```bash
rtk cargo fmt --check
rtk cargo test -p rollshot-action
rtk cargo clippy -p rollshot-action --all-targets -- -D warnings
```

Expected: all commands PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src crates/rollshot-action/tests/launch_teaser_contract.rs
rtk git commit -m "test(action-guide): cover launch teaser contract"
```
