# Agent Foundation Slice 5: Context Continuity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover bounded agent context from authoritative Product Task, artifact, and Action Guide project state, with one deterministic Smart Redaction overflow restart and exact stale-result rejection.

**Architecture:** Normal context boundaries load and canonically project durable Product Task or Action Guide state into a fresh empty-history request. Smart Redaction receives a separate run-local `RunContinuityManifestV1`; the first provider-neutral context overflow may replace the whole private Rig history and retry once without resetting identity, authority, skill, evidence, cancellation, turn, or budget state. Action Guide captions are the active proof workload: durable clean projects use revision-bound projections, while unsaved/dirty projects retain explicitly ephemeral behavior.

**Tech Stack:** Rust 2021, serde/serde_json, sha2, tokio, Rig 0.40 provider adapters, wiremock, `rollshot-agent`, `rollshot-action`, and `rollshot-app`.

## Global Constraints

- Governing spec: `docs/superpowers/specs/2026-07-28-agent-foundation-context-continuity-design.md`.
- Artifact/project re-projection is authoritative; transcript prose, `AgentSession`, transient events, and provider state are never recovery sources.
- No transcript persistence, semantic memory, model summary, provider-native compaction, selective pruning, workflow DAG, child agent, or durable in-flight run recovery.
- `ActionGuideContextProjectionV1`: at most 200 ordered steps, at most 4,096 UTF-8 bytes in the guide title or any projected step title/caption, and at most 256 KiB canonical serialized bytes.
- A project-backed caption proposal binds exact project revision plus projection digest; unsaved/dirty proposals remain ephemeral.
- No unchecked caption apply entry point remains; `apply` and `apply_all` require `CaptionApplyContext`.
- Only an explicitly classified `ModelError::ContextOverflow` may trigger recovery; ordinary failures never retry.
- At most one overflow retry. A second overflow is terminal.
- Replace the whole private Rig history; never retain an unmatched tool call or result.
- Each provider dispatch consumes one model-call budget unit, including overflow failures. Token/cost usage is charged only when reported.
- Retry reuses the same task, attempt, run, `ToolContext`, `AuthoritySnapshot`, `SkillUse`, cancellation token, wall-time budget, and accumulated turn count.
- Authority, consent, permission, and approval are never reconstructed from a projection or prose; every tool call retains the existing authority check.
- Do not persist the emergency manifest, caption proposal, transcript, provider continuation, or projection prose.
- No projection, serialization, `Debug`, event, tracing, or diagnostic output may contain pixels, raw semantic input, paths, credentials, full skill bodies, full proposal/artifact payloads, source/tool-result bodies, user/assistant prose, authority grants, or provider-native state.
- Runtime diagnostics use privacy-safe structured `tracing` with stable `rollshot::*` targets.
- No visible iced UI/layout/copy change is planned. If implementation changes visible behavior, invoke `iced-rs` and `testing-iced-ui` before that edit.
- Before changing any exported symbol, run LSP references and update every caller in the same task; leave no compatibility shim or deprecated alias.
- Keep `rig-core = "=0.40.0"`; add no dependency.

---

### Task 1: Canonical Product Task continuity projection

**Files:**
- Create: `crates/rollshot-agent/src/continuity.rs`
- Modify: `crates/rollshot-agent/src/lib.rs:1-13`
- Modify: `crates/rollshot-agent/src/product_task.rs:286-427` (`ProductArtifactMetadata` accessors only if needed)
- Test: `crates/rollshot-agent/src/continuity.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Consumes: `ProductTaskSnapshot`, `TaskAttempt`, `RunContractReceiptV1`, `ProductArtifactMetadata`, `ReviewReceipt`, and existing canonical SHA-256 conventions.
- Produces:
  - `pub struct ContinuityProjectionV1`
  - `pub enum ReviewContinuityStateV1`
  - `pub enum ContinuityProjectionError`
  - `impl TryFrom<&ProductTaskSnapshot> for ContinuityProjectionV1`
  - direct read-only task/run/artifact/review accessors, `canonical_bytes() -> &[u8]`, and `digest() -> &str`

Task/run/artifact serialization DTOs remain private. Do not expose a family of
public reference structs for a single projection consumer.

- [ ] **Step 1: Use LSP to confirm the Product Task access surface**

Run symbol/reference queries for `ProductTaskSnapshot`, `ProductArtifactMetadata`, and `ReviewReceipt`. Confirm the projection can use public accessors for task, attempt, source, status, payload digest, active run contract, and review receipt. Add only the missing read-only metadata accessors (`kind`, `schema_version`, `attempt_id`) to `ProductArtifactMetadata`; do not expose payload bytes or provider/model IDs through the projection.

- [ ] **Step 2: Write failing canonical and privacy tests**

Create `continuity.rs`, export it from `lib.rs`, and add tests with these exact contracts:

```rust
#[test]
fn same_snapshot_has_stable_projection_bytes_and_digest() {
    let snapshot = ready_v2_snapshot();
    let first = ContinuityProjectionV1::try_from(&snapshot).unwrap();
    let second = ContinuityProjectionV1::try_from(&snapshot).unwrap();

    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first.snapshot_revision(), snapshot.snapshot_revision());
    assert_eq!(first.artifact_revision().unwrap().get(), 1);
    assert_eq!(first.review_state(), ReviewContinuityStateV1::PendingExactRevision);
}

#[test]
fn projection_debug_and_json_omit_payload_and_authority_grants() {
    let secret = "SECRET-PROPOSAL-PAYLOAD";
    let snapshot = ready_v2_snapshot_with_proposal_bytes(secret.as_bytes().to_vec());
    let projection = ContinuityProjectionV1::try_from(&snapshot).unwrap();
    let rendered = format!("{projection:?}{}", String::from_utf8_lossy(projection.canonical_bytes()));

    assert!(!rendered.contains(secret));
    assert!(!rendered.contains("granted_operations"));
    assert!(!rendered.contains("provider_id"));
    assert!(!rendered.contains("model_id"));
}
```

Add table-driven cases for `Created`, bound `Running`, `ReadyForReview`, `Applying`, accepted/rejected review, `NeedsUserInput`, `Cancelled`, `Interrupted`, `Stale`, and failed terminals. Add malformed fixtures for mismatched task/attempt/run, artifact revision, review artifact, run contract, and schema version. Add exact 4,096/4,097-byte retained-string cases and a canonical 64 KiB/64 KiB-plus-one boundary case.

- [ ] **Step 3: Run the focused tests and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-agent continuity
```

Expected: compilation fails because the projection types and constructors do not exist.

- [ ] **Step 4: Implement the bounded projection**

Use private serializable DTOs with fixed field order and `serde(deny_unknown_fields)` on any deserializable V1 type. The public projection owns only IDs, closed enums, schema/revision numbers, digests, and bounded timestamps. Serialize once during construction and retain the canonical bytes in the immutable value, so repeated digest/comparison calls do not allocate or reserialize. Hash with a domain separator and the existing allocation-efficient `LowerHex` convention:

```rust
const CONTINUITY_PROJECTION_SCHEMA_V1: u32 = 1;
const CONTINUITY_PROJECTION_DOMAIN: &[u8] = b"rollshot-task-continuity-v1\0";

fn digest_projection(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(CONTINUITY_PROJECTION_DOMAIN);
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
```

`TryFrom<&ProductTaskSnapshot>` must:

1. accept current store schema 1 and 2, reject zero or greater than 2;
2. bind the last attempt only when present;
3. copy authority snapshot digest/policy revision and skill package/digest from the active run contract, never the grant vectors or skill body;
4. verify artifact task/attempt/run/source binding and active run contract equality;
5. derive review state from `TaskStatus`, artifact metadata, and exact `ReviewReceipt` revision;
6. canonicalize and cap every copied string at the existing 4,096-byte Product Task bound;
7. reject canonical serialized projections larger than 64 KiB; and
8. compute the digest once and cache it in the immutable value.

- [ ] **Step 5: Run focused and Product Task suites**

Run:

```bash
rtk cargo test -p rollshot-agent continuity
rtk cargo test -p rollshot-agent product_task
```

Expected: all continuity and Product Task tests pass.

- [ ] **Step 6: Commit the task**

```bash
rtk git add crates/rollshot-agent/src/continuity.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/product_task.rs
rtk git commit -m "feat(agent): add durable continuity projection"
```

---

### Task 2: Bounded Action Guide project projection

**Files:**
- Create: `crates/rollshot-action/src/project/continuity.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs:1-19`
- Test: `crates/rollshot-action/src/project/continuity.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Consumes: validated `LoadedProject`, `ProjectManifestV2`, `ProjectStep`, `Guide`, and existing project structural validation.
- Produces:
  - `pub struct ActionGuideContextProjectionV1`
  - `pub struct ActionGuideProjectedStepV1`
  - `pub enum ActionGuideProjectionError`
  - `ActionGuideContextProjectionV1::from_loaded_project(&LoadedProject)`
  - `revision()`, `digest() -> &str`, `canonical_bytes() -> &[u8]`, `steps()`, and `to_guide()`

- [ ] **Step 1: Write failing projection-bound tests**

Add tests that create a real project through `create_project`, reload it through `load_project`, and then assert:

```rust
#[test]
fn loaded_revision_projects_without_paths_pixels_or_annotations() {
    let (_temp, loaded) = saved_project_fixture(7);
    let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
    let json = String::from_utf8_lossy(projection.canonical_bytes());

    assert_eq!(projection.revision(), 7);
    assert_eq!(projection.steps().len(), loaded.manifest.steps.len());
    assert!(!json.contains(loaded.root.to_string_lossy().as_ref()));
    assert!(!json.contains("annotations"));
    assert!(!json.contains("frames"));
    assert!(!json.contains("sha256"));
}

#[test]
fn same_revision_reloads_to_identical_projection() {
    let (temp, loaded) = saved_project_fixture(3);
    let first = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
    drop(loaded);
    let reopened = load_project(temp.path()).unwrap();
    let second = ActionGuideContextProjectionV1::from_loaded_project(&reopened).unwrap();
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.digest(), second.digest());
}
```

Add exact boundary cases: 200 steps accepted; 201 rejected; a 4,096-byte guide title, step title, and caption accepted; 4,097 bytes rejected for each; exactly 256 KiB canonical bytes accepted; one byte over rejected. Use UTF-8 byte counts, not character counts.

- [ ] **Step 2: Run the focused tests and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-action project::continuity
```

Expected: compilation fails because the continuity module and projection types do not exist.

- [ ] **Step 3: Implement canonical projection and guide reconstruction**

Project only these fields: project revision/title, ordered step ID/order/keyframe/title/caption/kind/reason/timestamp. Re-run `validate_manifest_structure` inside the constructor, sort only by the already-validated `order`, and reject duplicate/non-contiguous order instead of repairing it.

Use:

```rust
pub const MAX_PROJECTED_STEPS: usize = 200;
pub const MAX_PROJECTED_TEXT_BYTES: usize = 4_096;
pub const MAX_PROJECTED_BYTES: usize = 256 * 1024;
const ACTION_GUIDE_PROJECTION_DOMAIN: &[u8] = b"rollshot-action-guide-continuity-v1\0";
```

Serialize once during construction; cache the bounded canonical bytes and digest
inside the immutable projection rather than reserializing on every accessor.

`to_guide()` maps `ProjectStepId.0` back to `GuideStep::source` exactly as `timeline_workspace::project::from_loaded_project` does. It does not carry `nearby`, annotations, frame hashes, frame dimensions, capture region, input source, warnings, enabled outputs, or project root into the model projection.

- [ ] **Step 4: Run focused and project suites**

Run:

```bash
rtk cargo test -p rollshot-action project::continuity
rtk cargo test -p rollshot-action project
```

Expected: all projection and existing project tests pass.

- [ ] **Step 5: Commit the task**

```bash
rtk git add crates/rollshot-action/src/project/continuity.rs crates/rollshot-action/src/project/mod.rs
rtk git commit -m "feat(action): project durable caption context"
```

---

### Task 3: Revision-bound caption proposal contract

**Files:**
- Modify: `crates/rollshot-action/src/caption_proposal.rs:4-189`
- Modify: `crates/rollshot-action/src/lib.rs:31-34`
- Test: `crates/rollshot-action/src/caption_proposal.rs:191-372`
- Modify: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs:127-219`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs:1067-1133,4476-4668`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs:905-1175`

**Interfaces:**
- Consumes: `ActionGuideContextProjectionV1`, `Guide`, and existing per-step stale bases.
- Produces:
  - `pub enum CaptionProposalOrigin { DurableProject { revision: u64, projection_digest: String }, EphemeralGuide { guide_digest: String } }`
  - `pub enum CaptionApplyContext { DurableProject { revision: u64, projection_digest: String, clean: bool }, EphemeralGuide }`
  - `CaptionProposal::from_agent_drafts(id, run_id, origin, guide, drafts)`
  - checked `apply(&mut self, &mut Guide, &CaptionApplyContext, CaptionSuggestionId)`
  - checked `apply_all(&mut self, &mut Guide, &CaptionApplyContext)`

- [ ] **Step 1: Use LSP references before changing exported methods**

Run LSP references for `CaptionProposal::from_agent_drafts`, `CaptionProposal::apply`, and `CaptionProposal::apply_all`. Record every caller in `rollshot-action`, `rollshot-app`, and tests. This task updates every caller to the checked API with explicit ephemeral context; Task 4 then replaces the saved-clean branch with durable projection without leaving the repository uncompilable between commits.

- [ ] **Step 2: Write failing origin and stale tests**

Add these contracts:

```rust
#[test]
fn durable_proposal_requires_exact_clean_revision_and_digest() {
    let mut guide = guide_fixture();
    let mut proposal = proposal_fixture(
        &guide,
        CaptionProposalOrigin::DurableProject {
            revision: 4,
            projection_digest: "a".repeat(64),
        },
    );

    let stale_revision = CaptionApplyContext::DurableProject {
        revision: 5,
        projection_digest: "a".repeat(64),
        clean: true,
    };
    assert_eq!(proposal.apply(&mut guide, &stale_revision, CaptionSuggestionId(1)), CaptionApplyOutcome::Stale);
    assert_eq!(guide.steps()[0].caption, "Before");
}

#[test]
fn ephemeral_proposal_preserves_step_local_stale_semantics() {
    let mut guide = two_step_guide_fixture();
    let mut proposal = proposal_fixture(
        &guide,
        CaptionProposalOrigin::EphemeralGuide { guide_digest: "b".repeat(64) },
    );
    guide.set_title_and_caption(2, "Changed elsewhere".into(), "Elsewhere".into());

    assert_eq!(proposal.apply(&mut guide, &CaptionApplyContext::EphemeralGuide, CaptionSuggestionId(1)), CaptionApplyOutcome::Applied);
}
```

Also cover exact clean durable apply success, dirty durable context, digest mismatch including same-revision project replacement, origin-kind mismatch, changed step base, `apply_all` with all checks performed before the first mutation, and redacted `Debug` containing no guide text.

- [ ] **Step 3: Run the focused suite and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-action caption_proposal
```

Expected: compilation fails because proposal origin/apply context do not exist and method signatures are unchanged.

- [ ] **Step 4: Implement the checked contract**

Validate origin at construction: revision must be non-zero; digests must be exactly 64 lowercase hexadecimal bytes. Store origin once on `CaptionProposal`; keep suggestion provenance unchanged.

For durable `apply_all`, first validate the shared apply context and collect all still-current suggestions, then mutate. If the shared revision/digest/clean check fails, mark every pending suggestion `Stale` and apply none. Per-step drift may still stale individual suggestions after the shared check.

Delete the unchecked method signatures rather than adding wrappers.

- [ ] **Step 5: Update every caller and run affected suites**

Update every `rollshot-action` fixture and every existing app callsite to pass
`CaptionProposalOrigin::EphemeralGuide` plus `CaptionApplyContext::EphemeralGuide`.
Compute the provenance digest with a deterministic test/app helper, but retain
the current step-local apply behavior. Run:

```bash
rtk cargo test -p rollshot-action caption_proposal
rtk cargo test -p rollshot-app --features action-guide caption_agent
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
```

Expected: all affected tests pass; no unchecked constructor/apply callsite remains.

- [ ] **Step 6: Commit the task**

```bash
rtk git add crates/rollshot-action/src/caption_proposal.rs crates/rollshot-action/src/lib.rs crates/rollshot-app/src/timeline_workspace/caption_agent.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/mod.rs
rtk git commit -m "feat(action): bind captions to explicit context"
```

---

### Task 4: Migrate the active Action Guide caption path

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs:5-219`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs:120-133,1067-1180`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs:293-388`
- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs:218-301,318-421`
- Test: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs:222-510`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs:4476-4668`
- Test: `crates/rollshot-app/src/timeline_workspace/mod.rs:1177-2760` (`project_lifecycle` tests)

**Interfaces:**
- Consumes: Task 2 projection, Task 3 origin/apply context, existing `load_project_worker`, `ProjectSession`, `ProjectSaveState`, provider config, timeout, and operation ID.
- Produces private app contracts:
  - `enum CaptionContextRequest { Durable { root: PathBuf, expected_revision: u64 }, Ephemeral { guide: Guide } }`
  - `enum PreparedCaptionContext { Durable(ActionGuideContextProjectionV1), Ephemeral { guide: Guide, guide_digest: String } }`
  - `Message::CaptionContextPrepared { run_id: u64, result: Result<PreparedCaptionContext, String> }`
  - `prepare_caption_context_task(...)`
  - updated `suggest_captions_task(run_id, model, adapter, PreparedCaptionContext)`

- [ ] **Step 1: Write failing close/reload and race tests**

Add a real-store fixture and fake provider request recorder. The primary test must execute this sequence:

```rust
#[test]
fn saved_project_reloads_fresh_context_and_rejects_revision_plus_one() {
    let (mut state, project_root) = saved_workspace_fixture_at_revision(1);
    let run_id = begin_caption_prepare(&mut state);
    let prepared = prepare_caption_context_task_for_test(
        run_id,
        CaptionContextRequest::Durable { root: project_root.clone(), expected_revision: 1 },
    ).unwrap();

    let request = recorded_caption_request(prepared.clone());
    assert!(request.history.is_empty());

    let mut proposal = proposal_from_prepared(prepared);
    state.project_session = Some(ProjectSession::Saved {
        root: project_root,
        base_revision: 2,
        access: ProjectAccess::Writable,
    });
    let context = state.caption_apply_context(&proposal);
    assert_eq!(proposal.apply(&mut state.guide, &context, CaptionSuggestionId(1)), CaptionApplyOutcome::Stale);
}
```

Also test: durable load fails; loaded revision differs from captured revision; workspace becomes dirty before `CaptionContextPrepared`; stale prepare run ID is ignored; provider is not launched on any prepare failure; unsaved and dirty workspaces create `EphemeralGuide`; late proposal after mutation cannot apply; previous visible copy remains unchanged.

Add one async active-path smoke test that drives
`SuggestCaptionsRequested → CaptionContextPrepared → CaptionSuggestionsLoaded
→ AcceptCaptionSuggestion` through `update`, using a real saved project and the
fake provider. Assert one empty-history provider request, one visible proposal,
and one applied caption. Do not satisfy this test by calling private preparation
helpers directly.

- [ ] **Step 2: Run the focused tests and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide caption_agent
rtk cargo test -p rollshot-app --features action-guide timeline_workspace::tests::project_lifecycle
```

Expected: tests fail because the prepare-stage message/contracts do not exist.

- [ ] **Step 3: Implement the two-stage durable dispatch**

At `SuggestCaptionsRequested`:

1. increment `caption_agent_run_id`;
2. capture `Durable { root, expected_revision }` only for `ProjectSession::Saved` plus `ProjectSaveState::Clean`;
3. otherwise capture `Ephemeral { guide: state.guide.clone() }`;
4. schedule `prepare_caption_context_task`;
5. do not construct the provider adapter or launch the provider yet.

At `CaptionContextPrepared`:

1. ignore stale `run_id`;
2. for durable input, recheck the same saved root, exact base revision, and `Clean` state;
3. fail without fallback if any check changed;
4. build the existing provider adapter and launch `suggest_captions_task`;
5. retain existing timeout, progress message, review UI, and error copy.

The durable preparation worker calls `load_project` in `spawn_blocking`, then constructs `ActionGuideContextProjectionV1`. It returns no project root inside `PreparedCaptionContext`.

For ephemeral input, compute a canonical guide digest only for provenance; do not use it to add a global stale rule.

- [ ] **Step 4: Route every apply through `CaptionApplyContext`**

Add `TimelineWorkspace::caption_apply_context(&self, proposal: &CaptionProposal) -> CaptionApplyContext`. Durable context reports the current project revision/digest and `clean` flag; ephemeral context returns `EphemeralGuide`. Call it before taking the mutable proposal borrow, then pass it to both `apply` and `apply_all`.

Update every app test fixture constructing a proposal. Remove every old unchecked callsite found by LSP.

- [ ] **Step 5: Run the active caption and timeline suites**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide caption_agent
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo test -p rollshot-action caption_proposal
```

Expected: all suites pass; recorded durable provider requests have zero history messages.

- [ ] **Step 6: Commit the migration**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/caption_agent.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/project.rs
rtk git commit -m "feat(app): reproject durable caption context"
```

---

### Task 5: Provider-neutral context overflow classification

**Files:**
- Modify: `crates/rollshot-agent/src/model.rs:140-149`
- Modify: `crates/rollshot-agent/src/provider.rs:65-202,250-343`
- Modify: `crates/rollshot-agent/tests/provider_contract.rs:131-498,649-990`
- Modify: `crates/rollshot-agent/tests/fixtures/provider_streams.json`

**Interfaces:**
- Consumes: Rig 0.40 `CompletionError::provider_response_status()` and `provider_response_json()`, existing wiremock fixtures, and private adapter selection.
- Produces: `ModelError::ContextOverflow` and private `ProviderFlavor`/classification helpers; no provider-native public DTO.

- [ ] **Step 1: Write failing HTTP classification contracts**

Create exact 400 fixtures:

```json
{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 210000 tokens > 200000 maximum"}}
```

```json
{"error":{"message":"This model's maximum context length is 128000 tokens.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}}
```

Add wiremock tests asserting both adapters return `Err(ModelError::ContextOverflow)` during stream establishment. Add a scripted stream test proving the same typed error survives after a stream has emitted partial content, without exposing provider-native details. Add lookalike tests that must not classify:

- Anthropic 400 `invalid_request_error` with `message = "invalid image"`;
- Anthropic 500 with the prompt-too-long text;
- OpenAI 400 without `code`;
- OpenAI 400 with `code = "max_tokens"`;
- any 401, 429, 500, malformed JSON, or transport failure.

Assert `format!("{error:?}")` contains no raw response body or numeric token counts for the overflow variant.

- [ ] **Step 2: Run provider contracts and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-agent --test provider_contract context_overflow
```

Expected: compilation fails because `ModelError::ContextOverflow` does not exist.

- [ ] **Step 3: Implement private exact classifiers**

Change `rig_to_model_error` to receive a private `ProviderFlavor`. Before the existing mapping, inspect only preserved non-success response status/JSON.

Use exact rules:

```rust
fn is_openai_context_overflow(status: Option<StatusCode>, json: Option<&serde_json::Value>) -> bool {
    status == Some(StatusCode::BAD_REQUEST)
        && json.and_then(|v| v.pointer("/error/code")).and_then(Value::as_str)
            == Some("context_length_exceeded")
}

fn is_anthropic_context_overflow(status: Option<StatusCode>, json: Option<&serde_json::Value>) -> bool {
    let message = json.and_then(|v| v.pointer("/error/message")).and_then(Value::as_str);
    status == Some(StatusCode::BAD_REQUEST)
        && json.and_then(|v| v.pointer("/error/type")).and_then(Value::as_str)
            == Some("invalid_request_error")
        && message.is_some_and(|text| text.starts_with("prompt is too long:"))
}
```

Use Rig's re-exported HTTP status type; do not add an `http` dependency. Pass the flavor into `stream_to_model_events` so preserved errors during stream consumption use the same classifier. Do not classify free-form `ProviderError`/`ResponseError` strings without preserved status and JSON.

- [ ] **Step 4: Run the full provider contract suite**

Run:

```bash
rtk cargo test -p rollshot-agent --test provider_contract
```

Expected: all provider contracts pass, including the lookalike failures.

- [ ] **Step 5: Commit the task**

```bash
rtk git add crates/rollshot-agent/src/model.rs crates/rollshot-agent/src/provider.rs crates/rollshot-agent/tests/provider_contract.rs crates/rollshot-agent/tests/fixtures/provider_streams.json
rtk git commit -m "feat(agent): classify provider context overflow"
```

---

### Task 6: Typed emergency manifest and model-dispatch budget

**Files:**
- Modify: `crates/rollshot-agent/src/continuity.rs`
- Modify: `crates/rollshot-agent/src/runtime.rs:29-93,193-462`
- Modify: `crates/rollshot-agent/src/tools.rs:637-721`
- Test: `crates/rollshot-agent/src/continuity.rs` (`#[cfg(test)]` manifest tests)
- Test: `crates/rollshot-agent/src/runtime.rs:620-1060`

**Interfaces:**
- Consumes: Task 1 projection, `ToolContext`, `DraftState`, `RunBudget`, `UsageSnapshot`, `AuthoritySnapshot`, `SkillUse`, and executable tool names.
- Produces:
  - `pub(crate) enum RunContinuityStageV1 { Drafting, NeedsValidation, NeedsDryRun, ReadyToSubmit }`
  - `pub(crate) struct EvidenceContinuityV1`
  - `pub(crate) struct BudgetContinuityV1`
  - `pub(crate) struct RunContinuityManifestV1`
  - `pub enum ContextRecoveryError`
  - `ToolContext::continuity_state()` returning a privacy-bounded snapshot
  - `BudgetTracker::charge_model_dispatch()`
  - canonical deterministic restart text

- [ ] **Step 1: Write failing budget-at-dispatch tests**

Add:

```rust
#[test]
fn model_dispatch_is_committed_even_when_no_usage_arrives() {
    let mut tracker = BudgetTracker::new(
        RunBudget { model_calls: 1, ..RunBudget::unlimited() },
        Instant::now(),
    );
    tracker.charge_model_dispatch().unwrap();
    assert_eq!(tracker.used().model_calls, 1);
    assert!(matches!(tracker.charge_model_dispatch(), Err(BudgetError::Exceeded(BudgetDimension::ModelCalls))));
}
```

Add a regression proving token/tool counters already pending in `turn` are not committed or altered by `charge_model_dispatch`; the method updates only committed `used.model_calls`.

- [ ] **Step 2: Write failing manifest construction tests**

Use a bound running V2 task plus a `ToolContext` fixture. Cover all four stage derivations and add:

```rust
#[test]
fn manifest_ignores_old_generation_evidence_and_omits_source() {
    let fixture = continuity_fixture();
    fixture.tool_ctx.draft.lock().unwrap().record_evidence(
        EvidenceKind::Validation,
        0,
        Instant::now(),
    ).unwrap();
    fixture.tool_ctx.draft.lock().unwrap().next_generation().unwrap();

    let manifest = RunContinuityManifestV1::build(fixture.inputs()).unwrap();
    assert_eq!(manifest.stage(), RunContinuityStageV1::NeedsValidation);
    assert!(manifest.evidence().is_empty());
    let debug = format!("{manifest:?}{:?}", fixture.tool_ctx.continuity_state());
    assert!(!debug.contains(fixture.secret_source));
}
```

Add stale task revision, task/attempt/run, source binding, authority digest/policy, skill package/digest, terminal/pending review, unavailable store, oversized canonical manifest, non-finite cost, and cancelled-before-build cases. Separately inject current-generation validation/dry-run evidence with its corresponding `last_validated`, proposal, metrics, or source cache missing; that inconsistent current claim must return `ContextRecoveryError::StaleEvidence`.

- [ ] **Step 3: Run focused tests and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-agent continuity
rtk cargo test -p rollshot-agent runtime::tests::model_dispatch
```

Expected: compilation fails because manifest, tool snapshot, and dispatch charging do not exist.

- [ ] **Step 4: Implement privacy-bounded state snapshots**

`ToolContext::continuity_state()` must lock each field in a fixed order and copy only:

- run ID, content-binding digest, draft generation;
- evidence kinds and source generations;
- booleans/digests for current generation's validated and dry-run state;
- whether pending review exists; and
- the derived next stage.

Hash validation/dry-run state using code-owned canonical metadata (kind, generation, candidate count, affected area where available). Do not serialize source, validated program, proposal, metrics debug text, capability handles, or pending review content.

`BudgetContinuityV1` copies every limit and committed-used counter. Encode `Duration` as `(secs, nanos)` and finite cost as IEEE-754 bits for canonical hashing; deterministic projection text formats the numeric values through code-owned labels.

`RunContinuityManifestV1::build` compares freshly loaded `ContinuityProjectionV1` with the run's expected task/attempt/run/source/run-contract references, current authority and skill, then derives the manifest and digest. Cap canonical manifest bytes at 64 KiB.

- [ ] **Step 5: Implement dispatch charging**

`charge_model_dispatch()` checks `used.model_calls + 1` against the limit and increments only committed `used.model_calls`. It does not inspect, apply, or clear the per-turn accumulator. Completion charging in Task 7 must set `model_calls: 0` so a completed request is not double-counted.

- [ ] **Step 6: Run focused suites**

Run:

```bash
rtk cargo test -p rollshot-agent continuity
rtk cargo test -p rollshot-agent runtime
rtk cargo test -p rollshot-agent tools
```

Expected: all focused tests pass.

- [ ] **Step 7: Commit the task**

```bash
rtk git add crates/rollshot-agent/src/continuity.rs crates/rollshot-agent/src/runtime.rs crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): add emergency continuity manifest"
```

---

### Task 7: One bounded whole-history overflow restart

**Files:**
- Modify: `crates/rollshot-agent/src/continuity.rs` (state-source trait/context)
- Modify: `crates/rollshot-agent/src/driver.rs:245-281,535-697,922-1163`
- Modify: `crates/rollshot-agent/src/runtime.rs` (terminal variants)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs:937-1390`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs:120-180,438-470`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs:95-145,230-252`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:1880-1980,2330-2420`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs:100-165`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/record.rs:340-400`
- Test: `crates/rollshot-agent/src/driver.rs:1668-5497`
- Test: `crates/rollshot-agent/tests/provider_contract.rs:1030-1660`

**Interfaces:**
- Consumes: `ModelError::ContextOverflow`, Task 6 manifest, `BudgetTracker::charge_model_dispatch`, and an app-supplied snapshot source.
- Produces:
  - object-safe `pub trait ContinuitySnapshotSource`
  - `pub enum RunContinuitySource { Durable { expected: ContinuityProjectionV1, source: Arc<dyn ContinuitySnapshotSource> }, Unavailable }`
  - `DriverError::ContextOverflow`
  - `RunTerminalState::ContextOverflow`
  - `RunTerminalState::ContextRecoveryFailure { category: ContextRecoveryFailureCategory }`
  - one-retry state in `AgentRunner::run_with_provider`

The trait method is object-safe and asynchronous without a new dependency:

```rust
pub trait ContinuitySnapshotSource: Send + Sync {
    fn load(
        self: std::sync::Arc<Self>,
        task_id: ProductTaskId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ProductTaskSnapshot, ContextRecoveryError>> + Send>>;
}
```

- [ ] **Step 1: Use LSP references for `run_with_provider` and terminal enums**

Find every caller and every exhaustive match on `RunTerminalState` and `DriverError`. List app, eval, test, and state-label callsites. This task migrates every caller to an explicit `RunContinuitySource::Unavailable` and every terminal match in the same commit; Task 8 replaces active stored runs with a durable source.

- [ ] **Step 2: Write failing deterministic fake-provider tests**

Use a `RecordingProvider` with queued establishment/stream results and an in-memory `ContinuitySnapshotSource`. Required tests:

```rust
#[tokio::test]
async fn first_overflow_restarts_with_manifest_and_no_old_tool_pairs() {
    let provider = RecordingProvider::new(vec![
        ProviderScript::EstablishmentError(ModelError::ContextOverflow),
        ProviderScript::CompletedToolCall(submit_for_review_call()),
    ]);
    let terminal = run_with_continuity_fixture(&provider).await;

    assert!(matches!(terminal, RunTerminalState::ReadyForReview(_)));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].history.iter().all(|message| !matches!(
        message,
        ModelMessage::AssistantToolCall { .. } | ModelMessage::ToolResult { .. }
    )));
    assert!(requests[1].history.iter().any(is_manifest_projection));
}

#[tokio::test]
async fn second_overflow_is_terminal_and_never_dispatches_third_call() {
    let provider = RecordingProvider::new(vec![
        ProviderScript::EstablishmentError(ModelError::ContextOverflow),
        ProviderScript::EstablishmentError(ModelError::ContextOverflow),
    ]);
    let terminal = run_with_continuity_fixture(&provider).await;
    assert_eq!(terminal, RunTerminalState::ContextOverflow);
    assert_eq!(provider.requests().len(), 2);
}
```

Add mid-stream overflow after text and partial tool arguments; stale task/artifact/skill/authority/source/evidence; cancellation before build, while a blocking snapshot load is pending, and before retry dispatch; pending review/terminal wins; model-call budget allows no second dispatch; ordinary provider failure gets zero retries; completed tools are not invoked twice; max-turn count is shared across Rig instances; final `AgentSession` has one completed product exchange and no manifest/pre-overflow prose. Add a table-driven continuity check covering every `UsageSnapshot` dimension: seed committed usage and wall elapsed before overflow, capture the same tracker in the retry, and prove no limit or counter resets; the retry may change only counters for work actually dispatched/executed after that point.

- [ ] **Step 3: Run focused driver tests and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-agent driver::tests::context_
```

Expected: compilation fails because overflow terminals, continuity source, and retry path do not exist.

- [ ] **Step 4: Preserve typed model errors through the driver**

Map `ModelError::ContextOverflow` to `DriverError::ContextOverflow` at stream establishment, stream item, and `ModelStreamEvent::Error`. Keep other mappings unchanged. Buffer per-turn text/tool arguments locally as today; on any error, discard the buffers and do not mutate Rig, `last_assistant_text`, session, draft, proposal, or artifact state.

Charge `tracker.charge_model_dispatch()` immediately before each `provider.stream` call. On successful completion charge only reported input/output tokens and cost, with `model_calls: 0`.

Add an ASCII state diagram beside the retry loop in `driver.rs` showing normal
dispatch, first overflow validation, whole-history replacement, second overflow,
and terminal precedence. Keep it synchronized with the state-machine tests.

- [ ] **Step 5: Implement the one-retry state machine**

In `run_with_provider`, own:

```rust
let mut overflow_retry_used = false;
let mut model_turns_started = 0usize;
```

Increment `model_turns_started` whenever `next_step()` emits `CallModel`, before
the provider dispatch. The overflow attempt therefore consumes both one Rig
turn and one model-call budget unit even though it has no completed response.

Before manifest construction, if `model_turns_started >= config.max_turns`,
return the same bounded `AgentProtocolFailure` category used by Rig's existing
max-turn boundary; do not build a zero-turn retry. If the model-call budget
cannot fund another dispatch, return `BudgetExhausted(ModelCalls)` without
building or dispatching the retry.

On the first `DriverError::ContextOverflow`:

1. set the guard before awaiting recovery;
2. check cancellation;
3. load the exact task through `RunContinuitySource`;
4. rebuild and validate `ContinuityProjectionV1` and `RunContinuityManifestV1`;
5. derive fixed deterministic restart text;
6. replace `rig_run` with a new `AgentRun` using only that restart user message;
7. set its remaining max turns to `config.max_turns.saturating_sub(model_turns_started)`;
8. clear transient per-turn assistant text; and
9. continue the loop.

An unavailable source or any manifest failure maps to a privacy-bounded `ContextRecoveryFailureCategory`; do not expose the raw store or provider error. A second overflow returns `ContextOverflow` without building another manifest.

- [ ] **Step 6: Update all callers and run affected suites**

Update every product/eval caller to pass `RunContinuitySource::Unavailable`.
Update exhaustive terminal matches by routing both new typed terminals through
the existing generic failure presentation; add no user-visible copy and expose
no store/provider details. Run:

```bash
rtk cargo test -p rollshot-agent driver::tests::context_
rtk cargo test -p rollshot-agent --test provider_contract
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app result_workspace
```

Expected: agent and product suites pass; every `run_with_provider` caller
supplies an explicit source and the repository is buildable at this commit.

- [ ] **Step 7: Commit the task**

```bash
rtk git add crates/rollshot-agent/src/continuity.rs crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/runtime.rs crates/rollshot-agent/tests/provider_contract.rs crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs crates/rollshot-app/src/result_workspace/workbench/eval/record.rs
rtk git commit -m "feat(agent): retry one context overflow"
```

---

### Task 8: Wire Product Task source into active runs

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:130-153,531-556`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs:937-1390`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs:100-165`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/record.rs:340-400`
- Test: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:818-1742`
- Test: `crates/rollshot-app/src/result_workspace/workbench/run.rs:3290-4310`

**Interfaces:**
- Consumes: Task 1 projection and Task 7 `ContinuitySnapshotSource`/`RunContinuitySource`.
- Produces:
  - `TaskStoreContinuitySource(Arc<TaskStore>)`
  - durable run source passed to `AgentRunner::run_with_provider`

- [ ] **Step 1: Write failing TaskStore source tests**

Add async tests for exact load, missing, corrupt, unsupported schema, and no path leakage:

```rust
#[tokio::test]
async fn continuity_source_loads_exact_bound_running_snapshot() {
    let (store, snapshot) = persisted_bound_running_fixture();
    let source: Arc<dyn ContinuitySnapshotSource> =
        Arc::new(TaskStoreContinuitySource::new(store));
    let loaded = source.load(snapshot.task_id().clone()).await.unwrap();
    assert_eq!(loaded.snapshot_revision(), snapshot.snapshot_revision());
}
```

Map TaskStore errors into closed categories; `format!("{error:?}")` must not
include config/task paths or raw corrupt bytes.

- [ ] **Step 2: Write failing active-run binding tests**

Cover: run-contract CAS followed by exact reload; snapshot changes between CAS
and reload; missing/corrupt store; no store; source passed into the runner; and
the source reloading a changed snapshot at overflow. A present store that fails
the post-CAS exact load must persist a typed terminal and make zero provider
requests. Eval fixtures must use deterministic in-memory sources when they
exercise recovery.

- [ ] **Step 3: Run focused tests and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::task_store
rtk cargo test -p rollshot-app result_workspace::workbench::run
```

Expected: tests fail because TaskStore does not implement the source and stored
runs still pass `Unavailable`.

- [ ] **Step 4: Implement the async TaskStore bridge**

`TaskStoreContinuitySource` owns `Arc<TaskStore>`. Its trait method clones the
store and uses `tokio::task::spawn_blocking` for `load`. Map errors:

- `NotFound` → `ContextRecoveryError::MissingTask`;
- `UnsupportedSchema` → `UnsupportedSchema`;
- `Corrupt`, `TaskIdMismatch`, `SnapshotTooLarge`, unsafe/symlink/non-regular → `CorruptTask`;
- join/lock/I/O/durability errors → `SourceUnavailable`.

No path or original error string enters the public error.

- [ ] **Step 5: Build continuity context after run-contract CAS**

In `start_agent_run`, after the run-contract CAS succeeds, load the exact bound
snapshot and construct `ContinuityProjectionV1`. Build:

```rust
RunContinuitySource::Durable {
    expected: projection,
    source: Arc::new(TaskStoreContinuitySource::new(store.clone())),
}
```

If no TaskStore exists, retain `RunContinuitySource::Unavailable`; normal
execution remains available, but a future overflow terminates as recovery
unavailable. If the post-bind exact load/projection fails while a store exists,
persist a typed terminal and do not launch the provider.

Pass the source into `run_with_provider`. Replace eval `Unavailable` inputs with
deterministic in-memory sources when the scenario exercises provider recovery.

- [ ] **Step 6: Run active-run and agent suites**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo test -p rollshot-agent
```

Expected: all suites pass; stored active runs carry durable sources and
store-less runs remain explicitly unavailable.

- [ ] **Step 7: Commit the integration**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/task_store.rs crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs crates/rollshot-app/src/result_workspace/workbench/eval/record.rs
rtk git commit -m "feat(app): bind active runs to durable continuity"
```

---

### Task 9: Validate restored review projections

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs:937-1390`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:1880-1980,2330-2420`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs:95-145,230-252`
- Test: `crates/rollshot-app/src/result_workspace/workbench/run.rs:3290-4310`
- Test: `crates/rollshot-app/src/result_workspace/mod.rs:950-1080`

**Interfaces:**
- Consumes: Task 1 `ContinuityProjectionV1` and existing TaskStore restore/apply CAS.
- Produces: explicit exact projection validation before restored review display,
  accept, or reject.

- [ ] **Step 1: Write failing restore-without-session tests**

Extend the existing stale restored review tests:

1. persist `ReadyForReview` with proposal payload;
2. construct a new workspace with a new empty `AgentSession`;
3. restore through TaskStore;
4. require `ContinuityProjectionV1` exact artifact/review binding before exposing apply;
5. mutate artifact revision or review receipt and prove restore/apply is rejected.

Also cover accepted/rejected receipt mismatch, corrupt projection, and a payload
digest mismatch at the same artifact revision. Assert the restored display uses
the stored typed proposal/payload and never projection text or previous session
exchanges.

- [ ] **Step 2: Run focused tests and confirm RED**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::tests::stale_restored_review
```

Expected: new stale projection cases fail because restore still relies on the
older source-binding checks.

- [ ] **Step 3: Validate projection before display**

Before caching/restoring a `ReadyForReview` snapshot, construct
`ContinuityProjectionV1`, verify `PendingExactRevision`, and compare its exact
artifact ID/revision/digest to the payload/proposal being restored. On failure,
reuse the existing stale-review UI path; add no copy.

- [ ] **Step 4: Revalidate before review mutation**

Before apply/reject CAS, reconstruct the projection from the current loaded
snapshot and repeat the exact check. A mismatch performs no proposal/document
mutation and returns the existing stale path.

- [ ] **Step 5: Run result-workspace suites**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace
rtk cargo test -p rollshot-agent continuity
```

Expected: all restore/apply tests pass with a fresh empty `AgentSession`.

- [ ] **Step 6: Commit restore validation**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/mod.rs
rtk git commit -m "feat(app): validate restored review continuity"
```

---

### Task 10: Gate verification, independent review, and decision record

**Files:**
- Create: `docs/superpowers/spikes/2026-07-28-context-continuity-decision.md`
- Modify only if verification exposes a Slice 5 regression: files already named in Tasks 1–9

**Interfaces:**
- Consumes: completed implementation, governing spec acceptance criteria, test outputs, and independent review findings.
- Produces: Slice 5 gate decision with exact verification, recovery measurements, migration, residual risks, and deferred scope.

- [ ] **Step 1: Run focused contract suites**

Run:

```bash
rtk cargo test -p rollshot-agent continuity
rtk cargo test -p rollshot-agent --test provider_contract
rtk cargo test -p rollshot-action project::continuity
rtk cargo test -p rollshot-action caption_proposal
rtk cargo test -p rollshot-app --features action-guide caption_agent
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo test -p rollshot-app result_workspace
```

Expected: all focused suites pass. Record passed/failed/ignored counts exactly.

- [ ] **Step 2: Run full affected-crate regression suites**

Run:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide
```

Expected: all affected-crate suites pass. If the stalled-decoder fixture reports `decoder_unavailable`, rerun the exact test once and the filtered video-import suite once, record both first and repeat outcomes, and do not label the first run clean.

- [ ] **Step 3: Run formatting and lint gates**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk proxy git diff --check
```

Expected: formatting, clippy, and whitespace checks pass with zero warnings/errors.

- [ ] **Step 4: Capture deterministic recovery measurements**

Run the focused tests with output enabled:

```bash
rtk cargo test -p rollshot-agent continuity::tests::recovery_measurements -- --exact --nocapture
rtk cargo test -p rollshot-action project::continuity::tests::recovery_measurements -- --exact --nocapture
```

Record for Product Task, Action Guide clean restart, and emergency restart:

- canonical input bytes;
- projection bytes;
- reference count;
- provider history message count;
- task/project revision and digest equality; and
- number of overflow retries.

Required results: same-revision bytes/digests equal; Action Guide prior-history count is zero; emergency retained call/result count is zero; retry count is at most one.

- [ ] **Step 5: Run privacy inspection**

Inspect changed production code for runtime diagnostics and serialize/debug paths. Run privacy sentinel tests from Tasks 1, 2, 5, 6, 7, 8, and 9. Confirm no `println!`, `eprintln!`, or `dbg!` was added to active product paths and every retained tracing event uses a stable `rollshot::*` target.

- [ ] **Step 6: Request independent code review**

Invoke `superpowers:requesting-code-review`. Require the reviewer to answer:

1. Can any transcript/model prose recreate task, artifact, authority, consent, permission, or approval state?
2. Can a stale task/artifact/skill/authority/project revision pass projection or apply?
3. Can an ordinary provider failure or lookalike error trigger retry?
4. Can more than one overflow retry occur?
5. Can partial text/tool arguments or a completed side effect be replayed/promoted?
6. Can the second Rig instance reset model calls, tokens, tools, wall time, or max turns?
7. Can any tool bypass the current `AuthoritySnapshot` after restart?
8. Can a durable caption request launch from dirty, mismatched, missing, or corrupt project state?
9. Can any unchecked caption apply callsite remain?
10. Can paths, pixels, semantic input, prose, full skill/payload, grants, credentials, or provider state leak through projection/manifest/debug/tracing?
11. Did the slice introduce transcript persistence, memory, native compaction, pruning, workflow, visual UI, or another non-goal?

Resolve every correctness/security finding before proceeding. Re-run the smallest affected focused suite after each fix, then rerun Steps 1–3 once.

- [ ] **Step 7: Write the Slice 5 gate decision**

Create `docs/superpowers/spikes/2026-07-28-context-continuity-decision.md` with:

- selected architecture and non-goals;
- exact Product Task and Action Guide projection contracts;
- overflow classifier matrix and retry state machine;
- task/project stale-rejection evidence;
- side-effect/protocol/budget/cancellation evidence;
- deterministic recovery measurements;
- privacy inspection;
- every verification command and exact result;
- independent reviewer provenance/findings/resolutions;
- migration and rollback;
- the initial planning-time non-reproduced Slice 4 stalled-decoder result plus implementation-time evidence;
- residual risks and deferred scope; and
- a Gate decision explicitly named “Slice 5 Context Continuity,” not “Gate G4.”

- [ ] **Step 8: Commit the verified gate record and any review fixes**

Stage only Slice 5 implementation/test files and the decision record. Use one fix commit per logical reviewer finding before the final docs commit. Then run:

```bash
rtk git add docs/superpowers/spikes/2026-07-28-context-continuity-decision.md
rtk git commit -m "docs(agent): record context continuity gate"
```

Stop after the gate decision. Do not begin Slice 6.

---

## Engineering Review Record (auto mode)

### Auto decisions

**Auto decision D1 — Keep the full gate scope.**
Context: The plan has 10 ordered tasks but only three new files and no new crate
or distributable. ELI10: Removing projection, provider, retry, restore, or gate
work would make the proof claim stronger than the implementation. The task count
comes from separating unsafe commit boundaries, not independent feature creep.
Stakes if wrong: Slice 5 could pass without an end-to-end authoritative recovery
proof. Recommendation: keep all tasks because every task maps to acceptance
criteria 1–14. Completeness: keep = 10/10, defer a gate component = 6/10.
Pros / cons: A) Keep (recommended) — ✅ complete gate, ❌ larger review surface.
B) Defer a component — ✅ smaller diff, ❌ invalidates the approved gate.
Net: retain the minimum complete proof, not a smaller incomplete feature.

**Auto decision D2 — Minimize the public projection API.**
Context: Task 1 originally exposed four nested reference DTOs used only through
one projection. ELI10: Public types become promises future code must support;
private DTOs can change without breaking callers. Stakes if wrong: an internal
serialization shape becomes permanent API debt. Recommendation: expose one
immutable projection, one review enum, one error, and direct accessors.
Completeness: both options 10/10. Pros / cons: A) Minimal API (recommended) —
✅ smaller compatibility surface, ❌ more direct accessors. B) Public nested
DTOs — ✅ mirrors JSON, ❌ unnecessary long-term API. Net: explicit and boring
beats a public type family with one consumer.

**Auto decision D3 — Keep the snapshot-source trait.**
Context: `rollshot-agent` cannot depend on the app-owned TaskStore. ELI10: The
trait is the narrow plug socket that lets the core ask the product for a fresh
snapshot without importing product storage. Stakes if wrong: dependency
inversion breaks or tests require filesystem coupling. Recommendation: keep the
object-safe trait and one production plus one in-memory test implementation.
Note: options differ in kind, not coverage. Pros / cons: A) Trait (recommended)
— ✅ acyclic/testable, ❌ one boxed future per overflow. B) App callback plumbing
— ✅ fewer named types, ❌ closure-heavy signatures and weaker diagnostics.
Net: the trait spends little complexity at a real crate boundary.

**Auto decision D4 — Count started model turns across Rig replacement.**
Context: Rig 0.40 increments `current_turn` when `next_step` emits `CallModel`,
including a request that later overflows. ELI10: A failed oversized request still
used one allowed attempt; restarting must not hand it back. Stakes if wrong:
overflow grants extra model turns beyond configuration. Recommendation: track
started turns, return existing max-turn failure when none remain, and preserve
separate model-call budget exhaustion. Completeness: started-turn accounting =
10/10, completed-only = 5/10. Pros / cons: A) Started turns (recommended) —
✅ matches pinned Rig, ❌ explicit counter. B) Completed turns — ✅ simpler,
❌ resets a consumed turn. Net: mirror the dependency's exact state machine.

**Auto decision D5 — Keep every intermediate commit buildable.**
Context: Task 7 changes a required public method argument used by product code.
ELI10: A commit that compiles only after the next commit cannot be safely
reviewed, bisected, or reverted. Stakes if wrong: `git bisect` lands on a broken
workspace. Recommendation: migrate all callers to explicit `Unavailable` and
reuse existing generic failure presentation in Task 7, then replace active stored runs with durable sources in Task 8.
Completeness: buildable cutover = 10/10, deferred callers = 4/10. Pros / cons:
A) Same-commit cutover (recommended) — ✅ atomic, ❌ touches app callsites.
B) Fix later — ✅ smaller first diff, ❌ broken branch boundary. Net: atomic API
cutover is required for maintainability.

**Auto decision D6 — Split active-run wiring from restore validation.**
Context: The original Task 8 mixed async source integration and review restore
semantics across the result workspace. ELI10: These fail differently and should
be reviewable/revertible separately. Stakes if wrong: a storage bridge regression
and stale-review regression become one hard-to-bisect commit. Recommendation:
Tasks 8 and 9, followed by gate Task 10. Completeness: both options 10/10.
Pros / cons: A) Split (recommended) — ✅ focused tests/rollback, ❌ one extra
task. B) Combined — ✅ fewer commits, ❌ oversized integration boundary.
Net: separate two coherent state transitions.

**Auto decision D7 — Cache canonical bytes and use existing hex formatting.**
Context: projections are compared and rendered more than once, and the original
digest snippet allocated one `String` per hash byte. ELI10: Canonical data should
be created once; recalculating it can waste memory and risks accidental drift.
Stakes if wrong: avoidable allocations on every recovery comparison.
Recommendation: cache bounded bytes/digest in immutable projections and use
`format!(\"{:x}\", digest)`. Completeness: both 10/10. Pros / cons: A) Cache
(recommended) — ✅ one serialization/allocation, ❌ stores at most bounded
64/256 KiB. B) Recompute — ✅ smaller struct, ❌ repeated work. Net: bounded
cached truth is safer and cheaper.

**Auto decision D8 — Add an active-path caption smoke test.**
Context: helper tests can pass while iced update/message wiring is broken.
ELI10: The proof must press the real logical button path, not test only pieces
on a bench. Stakes if wrong: durable caption continuity ships disconnected.
Recommendation: drive the full update chain with a real project and fake
provider; no visual baseline because layout/copy do not change. Completeness:
active path = 10/10, helpers only = 7/10. Pros / cons: A) Active-path test
(recommended) — ✅ catches wiring/races, ❌ larger fixture. B) Helpers only —
✅ faster setup, ❌ misses integration. Net: one deterministic smoke test closes
the highest product-path gap.

**Auto decision D9 — Document the retry state machine beside the code.**
Context: whole-history replacement has cancellation, budget, stale, terminal,
and second-overflow precedence. ELI10: A small diagram lets a maintainer see
which exits win without reconstructing a long loop. Stakes if wrong: later edits
silently reorder a safety boundary. Recommendation: add and test one synchronized
ASCII diagram in `driver.rs`. Completeness: both 10/10. Pros / cons: A) Diagram
(recommended) — ✅ legible invariants, ❌ must be maintained. B) Tests only —
✅ no comment drift, ❌ harder review. Net: this state machine is complex enough
to justify one maintained diagram.

### What already exists

- `ProductTaskSnapshot`, artifact metadata, review receipts, run contracts, and
  TaskStore CAS already own durable revision truth; Tasks 1, 8, and 9 project
  and validate them rather than creating another store.
- `AuthoritySnapshot`, `SkillUse`, `ToolContext`, `DraftState`, and
  `BudgetTracker` already own live run invariants; Tasks 6–7 retain those
  instances rather than reconstructing grants or evidence.
- Rig 0.40 `AgentRun` already guarantees tool-call/result pairing inside one
  run; Task 7 replaces the whole run rather than pruning its private history.
- Action Guide `load_project`, manifest validation, saved/dirty state, caption
  proposals, and step-base stale checks already exist; Tasks 2–4 reuse them.
- Anthropic/OpenAI adapters already preserve non-success status/body through
  Rig helpers; Task 5 adds only private exact classification.
- Existing result-workspace stale-review and terminal UI paths are reused; no
  new visible workflow or persistence layer is introduced.

### NOT in scope

- Transcript persistence, semantic/retrieval memory, summaries, and handoff
  documents — authoritative typed product state is the selected source.
- Provider-native compaction, context editing, continuation tokens, or cache
  policy — would couple public behavior to one provider.
- Selective tool-result pruning or artifact spilling — needs separate measured
  protocol/retention design.
- Durable in-flight run/process recovery, workflows, or child agents — Slice 5
  covers context overflow within one live bounded run.
- Action Guide visual-annotation migration, autosave, or draft persistence —
  caption projection is the bounded proof workload.
- New UI layout/copy/goldens and launch-video behavior — no user-facing workflow
  is required for this foundation.
- New crate, binary, package, or deployment pipeline — this slice adds no
  distributable artifact.

### Test coverage

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| 1 / Product Task canonical projection and privacy | ✓ | ✓ | — | no |
| 2 / validated Action Guide projection and bounds | ✓ | ✓ (real store) | — | no |
| 3 / revision/digest apply contract | ✓ | ✓ (app callers) | — | no |
| 4 / durable caption reload, race, request, apply | ✓ | ✓ | ✓ (update chain) | no |
| 5 / Anthropic/OpenAI overflow classification | ✓ | ✓ (wiremock) | — | no |
| 6 / manifest, evidence, privacy, budget dispatch | ✓ | ✓ | — | no |
| 7 / replacement, protocol, authority, budget, retry | ✓ | ✓ (fake provider/source) | — | no |
| 8 / TaskStore source and active-run binding | ✓ | ✓ (real store) | — | no |
| 9 / restore without session and stale review | ✓ | ✓ (real store) | — | no |
| 10 / complete gate and deterministic measurements | ✓ | ✓ | ✓ (focused active paths) | no |

### Failure modes

| Codepath | Production failure | Test / handling | User-visible result |
|---|---|---|---|
| Product Task projection | inconsistent artifact/review reference | Task 1 Step 2 / `ContinuityProjectionError` | typed recovery/stale result |
| Action Guide projection | missing/corrupt/oversize project | Task 2 Step 1 and Task 4 Step 1 / `ActionGuideProjectionError` | existing caption error copy |
| Caption apply | project becomes dirty or R+1 | Tasks 3–4 / `CaptionApplyOutcome::Stale` | existing stale review behavior |
| Provider classification | vendor changes error shape | Task 5 lookalikes / ordinary `ModelError` fallback | existing provider failure |
| Manifest build | source/evidence/authority/skill changed | Task 6 Step 2 / `ContextRecoveryFailureCategory` | bounded recovery failure |
| Snapshot load | I/O blocks or cancellation wins | Task 7 Step 2 and Task 8 Step 1 / cancellation select + closed source error | cancelled or recovery failed |
| Retry | second overflow or no model/turn budget | Task 7 Step 2 / typed overflow, budget, or max-turn terminal | existing generic failure presentation |
| Stream | overflow after partial text/tool args | Task 7 Step 2 / discard local buffers and replace whole Rig | no proposal/artifact promotion |
| Active run binding | snapshot changes after CAS | Task 8 Step 2 / exact projection mismatch | typed terminal, zero request |
| Review restore | payload/review digest stale at same revision | Task 9 Step 1 / exact revalidation before display and CAS | existing stale-review path |

Critical silent gaps: none.

### Dependency and parallelization

| Task | Modules touched | Depends on |
|---|---|---|
| 1 | `rollshot-agent` continuity/task | — |
| 2 | `rollshot-action` project | — |
| 3 | `rollshot-action` proposal, app timeline | 2 |
| 4 | app timeline | 2, 3 |
| 5 | `rollshot-agent` provider | — |
| 6 | `rollshot-agent` continuity/runtime/tools | 1 |
| 7 | `rollshot-agent` driver, app result workspace | 5, 6 |
| 8 | app result workspace store/run | 1, 7 |
| 9 | app result workspace restore | 1, 8 |
| 10 | all affected modules and gate record | 1–9 |

Parallel lanes:

- Lane A: Task 1 → Task 6 → Task 7 → Task 8 → Task 9.
- Lane B: Task 2 → Task 3 → Task 4.
- Lane C: Task 5, then joins Lane A before Task 7.
- Launch Tasks 1, 2, and 5 in parallel. Task 6 follows Task 1; Task 3 follows
  Task 2. Task 4 may run beside Task 6. Task 7 waits for Tasks 5 and 6. Tasks
  8–9 are sequential because they share result-workspace state. Task 10 waits
  for all lanes.
- Conflict flag: Tasks 3–4 and 7 touch different app modules but the same crate;
  parallel workers must not run crate-wide validation until merged. Tasks 7–9
  share result-workspace files and must remain sequential.
- No workspace-root membership/dependency edit serializes the lanes.

### Review completion

- Step 0 Scope Challenge — accepted as the minimum complete gate; 3 creates,
  existing-file modifications only otherwise, no new crate/artifact.
- Architecture Review — 3 issues resolved (public API, turn semantics, state
  diagram).
- Plan Structure + Code Quality — 2 issues resolved (atomic API cutover,
  split integration/restore tasks).
- Test Review — coverage table produced; 1 gap resolved with active-path smoke.
- Performance Review — 1 issue resolved (cached canonical bytes/efficient hex).
- NOT in scope — written.
- What already exists — written.
- Failure modes — 0 critical silent gaps.
- Parallelization — 3 lanes; initial three-way parallel wave, ordered joins.
- Unresolved decisions — 0.

Plan is locked in. Execute with `superpowers:subagent-driven-development`
using the dependency lanes above, or `superpowers:executing-plans` sequentially.
