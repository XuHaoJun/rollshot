# Action Guide Visual Annotation Provenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the existing per-step Action Guide visual annotation flow onto durable Product Task, authority, skill, artifact, review, restore, and audit contracts without changing its prompt, UI, or user-visible copy.

**Architecture:** Keep the bespoke visual annotation runner and existing iced state machine. Add only shared-contract variants, bind each run to a selected step/keyframe/image/annotation digest, authorize raw screenshot disclosure separately from prepared-image inspection, then reuse the Slice A task-store, audit, promotion, review, and restore mechanisms. Durable saved/clean projects restore; dirty or unsaved guides remain ephemeral and stale after restart.

**Tech Stack:** Rust, serde/serde_json, SHA-256 (`sha2`), Tokio, iced 0.14, `iced_test`, fs4-backed `TaskStore`, `rollshot-agent`, `rollshot-action`, `rollshot-app`.

## Global Constraints

- Governing spec: `docs/superpowers/specs/2026-07-29-action-guide-agent-foundation-visual-annotation-design.md`.
- Governing umbrella: `docs/superpowers/specs/2026-07-28-action-guide-agent-foundation-umbrella-design.md`.
- The current system prompt, user prompt, consent copy, terminal-to-message mapping, controls, layout, and all user-visible status/error copy remain byte-for-byte unchanged.
- Shared contracts may gain only the variants named by the spec. Existing `SourceBinding` and `ProductArtifactMetadata` variants/fields, `TaskStore` API, and audit vocabulary must not change shape.
- `rollshot-agent` must not depend on `rollshot-action`.
- The visual runner remains bespoke; do not route it through `SingleSubmitProfile`.
- A visual run grants exactly `DiscloseScreenshotAttachment` and `SubmitReviewCandidate`; captions retain only `SubmitReviewCandidate`.
- No PNG bytes, image pixels, flattened images, provider payloads, credentials, paths, raw semantic input, or full skill bodies may enter task files, artifact/proposal payloads, receipts, audit journals, or tracing.
- Durable bindings are allowed only for saved, clean projects. Dirty and unsaved workspaces use the ephemeral binding.
- Existing late-result suppression by local monotonic `run_id` stays in place; durable Product Task/attempt/`RunId` values are provenance truth.
- Runtime diagnostics use structured `tracing` with stable `rollshot::*` targets.
- No new UI surface, copy, control, or layout.
- Use `rtk` for every shell command.
- Do not update or approve golden baselines from the product-changing context. Follow `testing-iced-ui` auto mode and use an independent clean-context reviewer.
- Before Tasks 1 and 10–15, re-read `skill://iced-rs` and
  `skill://testing-iced-ui` in the executing context before touching iced
  state/update/view code.

---

## File structure and responsibility map

### New files

- `crates/rollshot-agent/skills/action-guide-visual-annotations/skill.toml` — static package descriptor.
- `crates/rollshot-agent/skills/action-guide-visual-annotations/SKILL.md` — exact current visual system prompt, with no added envelope or digest text.
- `docs/superpowers/spikes/2026-07-29-action-guide-visual-annotation-decision.md` — Gate B1 evidence, compatibility review, residual risks, and verification results; create only after implementation works and independent review finishes.

### Existing files

- `crates/rollshot-action/src/visual_annotation_proposal.rs` — serializable proposal origin/base, durable content binding, restore rebase, and review state.
- `crates/rollshot-agent/src/product_task.rs` — additive task/artifact/source-binding/summary variants and identity/freshness behavior.
- `crates/rollshot-agent/src/authority.rs` — additive raw screenshot disclosure operation and operation-oriented authorization documentation.
- `crates/rollshot-agent/src/skills.rs` — bundled visual skill registration/resolver and digest tests.
- `crates/rollshot-agent/src/driver.rs` — frozen system-prompt baseline, `VisualAnnotationProfile`, authority checks, audit-denial evidence, and bespoke runner parameters.
- `crates/rollshot-agent/src/visual_annotation.rs` — visual terminal variants and runner contract tests.
- `crates/rollshot-app/src/agent_store/task_store.rs` — exhaustive handling of the additive bindings and ephemeral open sweep; no API changes.
- `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs` — app translation boundary: digests, context, authority, durable task lifecycle, terminal mapping, promotion, restore, and privacy tests.
- `crates/rollshot-app/src/timeline_workspace/mod.rs` — visual task/review persistence state and constructor initialization.
- `crates/rollshot-app/src/timeline_workspace/update.rs` — unchanged UX state transitions plus durable worker/review/restore/stale messages.
- `crates/rollshot-app/src/timeline_workspace/view.rs` — structural/interaction/visual restore scenarios only; product layout stays unchanged.
- `crates/rollshot-app/src/timeline_workspace/project.rs` — initialize new state and trigger restore after project open/save where the existing caption restore hook runs.

### Locked interfaces

The following names are the contract between tasks:

```rust
// rollshot-action
pub enum VisualAnnotationProposalOrigin {
    DurableProject { revision: u64, projection_digest: String },
    EphemeralGuide { guide_digest: String },
}

pub struct VisualAnnotationStepBase {
    pub step_source: CandidateId,
    pub keyframe: FrameId,
    pub document_state_id: u64,
    pub image_width: u32,
    pub image_height: u32,
    pub keyframe_sha256: [u8; 32],
    pub annotation_state_sha256: [u8; 32],
}

pub fn rebase_restored(
    &mut self,
    current_step: &GuideStep,
    current_document_state_id: u64,
    image_width: u32,
    image_height: u32,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
) -> VisualAnnotationApplyOutcome;

// rollshot-agent
pub struct VisualAnnotationProfile<'a> {
    skill_use: &'a SkillUse,
}
impl<'a> VisualAnnotationProfile<'a> {
    pub fn from_skill(skill_use: &'a SkillUse) -> Result<Self, DriverError>;
    pub fn system_prompt(&self) -> &str;
}

// rollshot-app
pub(crate) fn visual_keyframe_digest(image: &image::RgbaImage) -> [u8; 32];
pub(crate) fn visual_annotation_state_digest(
    annotations: &[rollshot_image_document::Annotation],
) -> Result<[u8; 32], String>;

pub(crate) enum VisualAnnotationContextRequest {
    Durable { root: PathBuf, expected_revision: u64 },
    Ephemeral { guide: rollshot_action::Guide },
}

pub(crate) enum PreparedVisualAnnotationContext {
    Durable {
        project_root_sha256: [u8; 32],
        revision: u64,
        projection_digest: String,
    },
    Ephemeral {
        guide_digest: String,
    },
}

pub(crate) struct VisualAnnotationTaskInput {
    pub run_id: u64,
    pub step: GuideStep,
    pub document_state_id: u64,
    pub image: image::RgbaImage,
    pub annotations: Vec<rollshot_image_document::Annotation>,
}

pub(crate) struct VisualAnnotationRunSuccess {
    pub task_id: ProductTaskId,
    pub proposal: VisualAnnotationProposal,
    pub snapshot: ProductTaskSnapshot,
    pub provider_id: String,
    pub model_id: String,
}

pub(crate) enum VisualAnnotationTaskResult {
    Proposal(Box<VisualAnnotationRunSuccess>),
    NoSuggestion { reason: Option<String> },
}

pub(crate) async fn suggest_visual_annotation_task(
    input: VisualAnnotationTaskInput,
    context: VisualAnnotationContextRequest,
    store: Arc<TaskStore>,
    provider_name: String,
    model: String,
    adapter: Box<dyn ProviderAdapter>,
    cancellation: RunCancellation,
) -> Result<VisualAnnotationTaskResult, String>;
```

---

## Task 1: Freeze the existing prompt, consent, and terminal copy

This task establishes RED/green regression evidence before moving any behavior.

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs` (`VISUAL_ANNOTATION_SYSTEM_PROMPT`, tests).
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs` (`build_visual_annotation_prompt`, `map_terminal_to_result`, tests).
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs` (extract existing strings into constants used by handlers, tests).
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs` (consent presentation helper/test; no layout change).

**Interfaces:**
- Produces: `pub(crate) const VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE: &str` in `driver.rs`.
- Produces: frozen app constants used later instead of duplicating strings.
- Consumes: no Slice B interfaces.

- [ ] **Step 1: Add failing frozen-system-prompt and user-prompt tests**

In `driver.rs`, expose the current constant as `pub(crate)` under the baseline
name and add a test that initially refers to the new name before the rename.
The fixed digest pins all 2,598 current UTF-8 bytes without duplicating a
second maintainable copy inside the source file:

```rust
#[test]
fn visual_annotation_system_prompt_baseline_is_exact() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE.as_bytes())
        ),
        "7aeccfb58bd4be0ad9efbcf875724c58a8b032aa397dc8914f42ce939847c3b1",
    );
    assert_eq!(VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE.len(), 2_598);
}
```

In `visual_annotation_agent.rs`, pin the dynamic prompt:

```rust
#[test]
fn visual_annotation_user_prompt_baseline_is_exact() {
    assert_eq!(
        build_visual_annotation_prompt(&step()),
        "Inspect this reviewed Action Guide step and suggest visual annotation overlays \
(Number Callout, Text Note, or Opaque Redaction) on the attached keyframe. \
Prefer calling the submit_visual_annotation_suggestions tool. If tool calling \
is unavailable, return only JSON in the same schema. The image is the only \
source of truth. Use the step metadata as context only. \
Step source=10, keyframe=7, title=\"Open Settings\"",
    );
}
```

- [ ] **Step 2: Add failing terminal and consent-copy tests**

Test every current terminal mapping, including:

```rust
let result = map_terminal_to_result(
    VisualAnnotationRunTerminal::BudgetExhausted {
        dimension: BudgetDimension::WallTime,
    },
    7,
    &step(),
    0,
    100,
    80,
);
let VisualAnnotationTaskResult::NoSuggestion { reason } = result else {
    panic!("expected no-suggestion result");
};
assert_eq!(
    reason.as_deref(),
    Some("Visual annotation suggestion budget exhausted."),
);
```

Add assertions for provider failure, protocol failure, cancellation, and no-suggestion. Extract a pure consent-body helper in `view.rs` and assert:

```rust
assert_eq!(
    visual_consent_body("openai", "gpt-test"),
    "Rollshot will send this one reviewed keyframe to openai using gpt-test to suggest callouts, notes, or redactions. Review every suggestion before it changes your guide. Original keyframes and Issue Packs may still contain unredacted evidence.",
);
```

Pin update strings: running, ready, cancelled, generic failure, stale, accept/reject messages.

- [ ] **Step 3: Run the focused tests and observe RED**

Run:

```text
rtk cargo test -p rollshot-agent visual_annotation_system_prompt_baseline_is_exact
rtk cargo test -p rollshot-app --features action-guide visual_annotation_user_prompt_baseline_is_exact
rtk cargo test -p rollshot-app --features action-guide visual_consent_body_is_frozen
```

Expected: compile/test failure because the baseline constant and consent helper do not exist yet.

- [ ] **Step 4: Extract constants/helpers without changing bytes**

Rename the driver constant to `pub(crate) const VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE`. In app code, replace inline literals with constants whose values are byte-identical. Make `visual_consent_modal` call `visual_consent_body` without altering widget construction.

- [ ] **Step 5: Run focused and proportional tests**

Run:

```text
rtk cargo test -p rollshot-agent visual_annotation
rtk cargo test -p rollshot-app --features action-guide visual_annotation
```

Expected: all focused tests pass; no existing test changes its expected copy.

- [ ] **Step 6: Commit**

```text
rtk git add crates/rollshot-agent/src/driver.rs crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "test(action-guide): freeze visual annotation behavior"
```

---

## Task 2: Make the visual proposal durable and restart-safe

**Files:**
- Modify: `crates/rollshot-action/src/visual_annotation_proposal.rs`.
- Modify call sites/tests: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`, `mod.rs`, `update.rs`, `annotation.rs`.

**Interfaces:**
- Produces: `VisualAnnotationProposalOrigin`, `VisualAnnotationStepBase`, serde proposal types, and `rebase_restored` exactly as locked above.
- Consumes: current `GuideStep`, `ImagePoint`, `ImageRect`, and proposal validation rules.

- [ ] **Step 1: Write failing serde and restore tests**

Add tests in `visual_annotation_proposal.rs`:

```rust
#[test]
fn proposal_round_trips_without_a_guide_step_wire_shape() {
    let proposal = durable_proposal_fixture();
    let bytes = serde_json::to_vec(&proposal).unwrap();
    let json = String::from_utf8(bytes.clone()).unwrap();
    assert!(!json.contains("nearby"));
    assert!(!json.contains("at_ms"));
    assert_eq!(
        serde_json::from_slice::<VisualAnnotationProposal>(&bytes).unwrap(),
        proposal,
    );
}

#[test]
fn restore_rebase_requires_both_durable_content_digests() {
    let mut proposal = durable_proposal_fixture();
    assert_eq!(
        proposal.rebase_restored(&step(), 0, 100, 80, [1; 32], [2; 32]),
        VisualAnnotationApplyOutcome::Ready,
    );
    assert!(proposal
        .suggestions
        .iter()
        .all(|item| item.base.document_state_id == 0));

    let mut wrong_image = durable_proposal_fixture();
    assert_eq!(
        wrong_image.rebase_restored(&step(), 0, 100, 80, [9; 32], [2; 32]),
        VisualAnnotationApplyOutcome::Stale,
    );
}
```

Also test wrong annotation digest, source, keyframe, width, and height independently.

- [ ] **Step 2: Run RED**

Run:

```text
rtk cargo test -p rollshot-action proposal_round_trips_without_a_guide_step_wire_shape
rtk cargo test -p rollshot-action restore_rebase_requires_both_durable_content_digests
```

Expected: compile failure because the origin/base/rebase interface does not exist and the proposal is not deserializable.

- [ ] **Step 3: Implement the minimal serializable model**

Add serde derives to IDs, provenance, payload, draft/base/suggestion/status/proposal. Replace `origin: GuideStep` with:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VisualAnnotationProposalOrigin {
    DurableProject { revision: u64, projection_digest: String },
    EphemeralGuide { guide_digest: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VisualAnnotationStepBase {
    pub step_source: CandidateId,
    pub keyframe: FrameId,
    pub document_state_id: u64,
    pub image_width: u32,
    pub image_height: u32,
    pub keyframe_sha256: [u8; 32],
    pub annotation_state_sha256: [u8; 32],
}
```

Change `from_agent_drafts` to accept `origin`, `keyframe_sha256`, and `annotation_state_sha256`, build one shared base, and retain every existing validation. Implement `rebase_restored` so it marks pending suggestions stale on any mismatch and changes only pending `document_state_id` values on success.

- [ ] **Step 4: Migrate every constructor call**

Use explicit fixture origins and `[u8; 32]` digests. Production `suggestion_batch_to_proposal` gains the same parameters and forwards them. Do not add compatibility overloads or deprecated constructors.

- [ ] **Step 5: Run action and app focused tests**

```text
rtk cargo test -p rollshot-action visual_annotation_proposal
rtk cargo test -p rollshot-app --features action-guide visual_annotation
```

Expected: pass.

- [ ] **Step 6: Commit**

```text
rtk git add crates/rollshot-action/src/visual_annotation_proposal.rs crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/annotation.rs
rtk git commit -m "feat(action-guide): make visual proposals durable"
```

---

## Task 3: Add deterministic visual content digests and context origin

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`.
- Test: same file.

**Interfaces:**
- Produces the locked `visual_keyframe_digest`,
  `visual_annotation_state_digest`, `VisualAnnotationContextRequest`, and
  `PreparedVisualAnnotationContext` interfaces. Durable preparation computes
  and retains the project-root digest, revision, and projection digest;
  ephemeral preparation retains only the guide digest.
- Consumes: Task 2 proposal origin/base.

- [ ] **Step 1: Write failing digest vector tests**

```rust
#[test]
fn visual_keyframe_digest_is_domain_separated_and_dimension_sensitive() {
    let one = image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]));
    let two = image::RgbaImage::from_pixel(1, 2, image::Rgba([1, 2, 3, 255]));
    assert_ne!(visual_keyframe_digest(&one), visual_keyframe_digest(&two));
    assert_eq!(visual_keyframe_digest(&one), visual_keyframe_digest(&one));
}

#[test]
fn annotation_digest_is_order_and_content_sensitive() {
    let a = vec![annotation_fixture(1), annotation_fixture(2)];
    let b = vec![annotation_fixture(2), annotation_fixture(1)];
    assert_ne!(
        visual_annotation_state_digest(&a).unwrap(),
        visual_annotation_state_digest(&b).unwrap(),
    );
}
```

Add exact golden vectors:

```rust
fn digest_hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn visual_content_digest_vectors_are_stable() {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255]));
    assert_eq!(
        digest_hex(visual_keyframe_digest(&image)),
        "076499b61e7fac624835f05426686bf725b0220d24f5b2c18d2d70368ac2cbef",
    );
    assert_eq!(
        digest_hex(visual_annotation_state_digest(&[]).unwrap()),
        "c2f1bf7391acf52d4af9a694e2e4253e3fc9eafb11aaf105d8a8b1e2ffed8fd2",
    );
}
```

- [ ] **Step 2: Run RED**

```text
rtk cargo test -p rollshot-app --features action-guide visual_keyframe_digest_is_domain_separated_and_dimension_sensitive
```

Expected: missing functions.

- [ ] **Step 3: Implement exact formulas**

```rust
pub(crate) fn visual_keyframe_digest(image: &image::RgbaImage) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"rollshot-action-guide-keyframe-v1\0");
    hash.update(image.width().to_le_bytes());
    hash.update(image.height().to_le_bytes());
    hash.update(image.as_raw());
    hash.finalize().into()
}

pub(crate) fn visual_annotation_state_digest(
    annotations: &[rollshot_image_document::Annotation],
) -> Result<[u8; 32], String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(annotations)
        .map_err(|error| format!("serialize visual annotation state: {error}"))?;
    let mut hash = Sha256::new();
    hash.update(b"rollshot-action-guide-annotations-v1\0");
    hash.update(bytes);
    Ok(hash.finalize().into())
}
```

Implement durable preparation by loading the saved project in `spawn_blocking`, verifying `expected_revision`, and building `ActionGuideContextProjectionV1`. Ephemeral preparation computes the same guide digest algorithm used by captions; move `compute_guide_digest` to `pub(crate)` in `caption_agent.rs` rather than copying it.

- [ ] **Step 4: Add context drift tests**

Prove durable preparation rejects a changed revision and ephemeral preparation never carries a path. Use a temp saved project fixture; inspect only origin/digest values.

- [ ] **Step 5: Run focused tests**

```text
rtk cargo test -p rollshot-app --features action-guide visual_annotation_agent
rtk cargo test -p rollshot-app --features action-guide caption_agent
```

Expected: pass; caption digest tests remain green.

- [ ] **Step 6: Commit**

```text
rtk git add crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/caption_agent.rs
rtk git commit -m "feat(action-guide): bind visual proposal content"
```

---

## Task 4: Add visual source-binding variants and matching rules

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs`.
- Test: same file.

**Interfaces:**
- Produces: the two exact `SourceBinding` variants from the spec.
- Consumes: primitive `u64`, `[u8; 32]`, and `String` only.

- [ ] **Step 1: Write failing serde and matching tests**

Add both variants to `source_binding_round_trips_all_variants` fixture expectations before implementation. Add table-driven tests changing one field at a time:

```rust
#[test]
fn visual_project_identity_is_root_and_step_source_only() {
    let base = visual_project_binding();
    let mut fresh_change = base.clone();
    if let SourceBinding::ActionGuideVisualAnnotationProject { revision, .. } = &mut fresh_change {
        *revision += 1;
    }
    assert!(base.identity_matches(&fresh_change));
    assert!(!base.freshness_matches(&fresh_change));
}

#[test]
fn visual_binding_domains_never_alias_captions() {
    let visual = SourceBinding::ActionGuideVisualAnnotationProject {
        project_root_sha256: [3; 32],
        revision: 1,
        projection_digest: "ab".repeat(32),
        step_source: 7,
        keyframe: 9,
        keyframe_sha256: [4; 32],
        annotation_state_sha256: [5; 32],
    };
    let caption = SourceBinding::ActionGuideProject {
        project_root_sha256: [3; 32],
        revision: 1,
        projection_digest: "ab".repeat(32),
    };
    assert!(!visual.identity_matches(&caption));
}
```

Test revision, projection, keyframe ID, keyframe digest, and annotation digest independently. Test root and step source as identity differences.

- [ ] **Step 2: Run RED**

```text
rtk cargo test -p rollshot-agent visual_project_identity_is_root_and_step_source_only
```

Expected: missing enum variants.

- [ ] **Step 3: Implement variants in serialize and compatibility deserialize paths**

Add the fields exactly as approved to `SourceBinding`, the private `Tagged` compatibility enum, and its conversion match. Do not touch `LegacyFlat` or existing wire names.

- [ ] **Step 4: Implement matching**

Add explicit match arms. Durable identity compares root plus step source. Durable freshness compares revision, projection, keyframe ID, keyframe digest, and annotation digest. Ephemeral identity/freshness compares the complete variant. Cross-variant falls through to false.

- [ ] **Step 5: Run product-task suite**

```text
rtk cargo test -p rollshot-agent product_task
```

Expected: pass, including pre-migration fixtures exercised by downstream app tests later.

- [ ] **Step 6: Commit**

```text
rtk git add crates/rollshot-agent/src/product_task.rs
rtk git commit -m "feat(agent): add visual annotation source bindings"
```

---

## Task 5: Add task, artifact, summary, disclosure-operation, and sweep variants

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs`.
- Modify: `crates/rollshot-agent/src/authority.rs`.
- Modify: `crates/rollshot-app/src/agent_store/task_store.rs`.
- Test: all three modules.

**Interfaces:**
- Produces: `TaskKind::ActionGuideVisualAnnotation`, `ArtifactKind::ActionGuideVisualAnnotation`, `ArtifactSummary::ActionGuideVisualAnnotation { suggestion_count }`, and `RunOperation::DiscloseScreenshotAttachment`.
- Consumes: Task 4 source bindings.

- [ ] **Step 1: Add failing wire-name and grant tests**

```rust
#[test]
fn visual_contract_variants_have_stable_wire_names() {
    assert_eq!(
        serde_json::to_string(&TaskKind::ActionGuideVisualAnnotation).unwrap(),
        "\"action_guide_visual_annotation\"",
    );
    assert_eq!(
        serde_json::to_string(&RunOperation::DiscloseScreenshotAttachment).unwrap(),
        "\"disclose_screenshot_attachment\"",
    );
}
```

Add an authority test proving `FullScreenshot` alone does not satisfy the new operation when the grant is absent.

- [ ] **Step 2: Add failing ephemeral-sweep test**

Table-test the new ephemeral variant: abandoned `ReadyForReview` becomes
`Stale`; abandoned `Created`, `Running`, and `Applying` become `Interrupted`.
Open a second store and assert each audited transition. Add the matching
live-owner exemption for every state.

- [ ] **Step 3: Run RED**

```text
rtk cargo test -p rollshot-agent visual_contract_variants_have_stable_wire_names
rtk cargo test -p rollshot-app --features action-guide visual_ephemeral_review_stales_on_open
```

Expected: missing variants.

- [ ] **Step 4: Implement additive variants and exhaustive matches**

Insert the variants without reordering existing `DisclosureCeiling` values or changing existing serde forms. Extend only exhaustive matches and the store's ephemeral predicate:

```rust
let ephemeral = matches!(
    snapshot.source_binding(),
    SourceBinding::ActionGuideEphemeralGuide { .. }
        | SourceBinding::ActionGuideVisualAnnotationEphemeralGuide { .. }
);
```

Widen `authorize_tool` documentation to “authorize a run operation”; keep its signature unchanged.

- [ ] **Step 5: Run contract/store tests**

```text
rtk cargo test -p rollshot-agent authority
rtk cargo test -p rollshot-agent product_task
rtk cargo test -p rollshot-app --features action-guide agent_store
```

Expected: pass.

- [ ] **Step 6: Commit**

```text
rtk git add crates/rollshot-agent/src/product_task.rs crates/rollshot-agent/src/authority.rs crates/rollshot-app/src/agent_store/task_store.rs
rtk git commit -m "feat(agent): add visual annotation task contracts"
```

---

## Task 6: Bundle the frozen visual annotation skill

**Files:**
- Create: `crates/rollshot-agent/skills/action-guide-visual-annotations/skill.toml`.
- Create: `crates/rollshot-agent/skills/action-guide-visual-annotations/SKILL.md`.
- Modify: `crates/rollshot-agent/src/skills.rs`.
- Modify: `crates/rollshot-agent/src/driver.rs`.

**Interfaces:**
- Produces: `ACTION_GUIDE_VISUAL_ANNOTATIONS_PACKAGE_ID` and `bundled_action_guide_visual_annotations_use() -> Option<SkillUse>`.
- Consumes: Task 1 exact system-prompt baseline.

- [ ] **Step 1: Write failing resolver/body tests**

```rust
#[test]
fn bundled_visual_skill_body_matches_frozen_system_prompt() {
    let skill = bundled_action_guide_visual_annotations_use().unwrap();
    assert_eq!(skill.package_id().as_str(), "action-guide-visual-annotations");
    assert_eq!(skill.source_authority().as_str(), "rollshot.bundled");
    assert_eq!(skill.body(), crate::driver::VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE);
}
```

Add the exact golden digest test:

```rust
#[test]
fn bundled_visual_skill_golden_digest_is_stable() {
    let skill = bundled_action_guide_visual_annotations_use().unwrap();
    assert_eq!(
        skill.digest(),
        "00829fdce733b6b8ffd65340c2ae35b6a986055f63d3c5844cc0ee5df11f6f5e",
    );
}
```

- [ ] **Step 2: Run RED**

```text
rtk cargo test -p rollshot-agent bundled_visual_skill_body_matches_frozen_system_prompt
```

Expected: missing resolver.

- [ ] **Step 3: Create exact package files**

`skill.toml`:

```toml
schema_version = 1
package_id = "action-guide-visual-annotations"
name = "Action Guide Visual Annotations"
description = "Suggest reviewable visual annotations for one Action Guide keyframe."
declared_version = "1"
main = "SKILL.md"
```

`SKILL.md` must contain the complete Task 1 baseline and no envelope, package tag, digest, or trailing extra newline. Verify byte equality through the test, not visual inspection.

- [ ] **Step 4: Register and resolve**

Follow the static bundled table shape already used for captions: include both files, add the package constant, and expose a host-explicit resolver scoped to `rollshot.bundled`.

- [ ] **Step 5: Run skill tests and verify the golden digest**

```text
rtk cargo test -p rollshot-agent skills
```

Expected: pass with the exact body and digest above.

- [ ] **Step 6: Commit**

```text
rtk git add crates/rollshot-agent/skills/action-guide-visual-annotations crates/rollshot-agent/src/skills.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): bundle visual annotation skill"
```

---

## Task 7: Introduce the frozen `VisualAnnotationProfile`

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`.
- Test: same file.
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`
  (active caller cutover).

**Interfaces:**
- Produces: `VisualAnnotationProfile::from_skill` and `system_prompt` exactly as locked.
- Consumes: Task 6 resolver and package ID.

- [ ] **Step 1: Write failing constructor tests**

```rust
#[test]
fn visual_profile_derives_the_exact_prompt_from_the_skill() {
    let skill = bundled_action_guide_visual_annotations_use().unwrap();
    let profile = VisualAnnotationProfile::from_skill(&skill).unwrap();
    assert_eq!(profile.system_prompt(), VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE);
}

#[test]
fn visual_profile_rejects_the_wrong_package() {
    let caption = bundled_action_guide_captions_use().unwrap();
    assert!(VisualAnnotationProfile::from_skill(&caption).is_err());
}
```

Also construct a host-authority fixture with the same package ID and prove rejection.

- [ ] **Step 2: Run RED**

```text
rtk cargo test -p rollshot-agent visual_profile_derives_the_exact_prompt_from_the_skill
```

Expected: missing profile.

- [ ] **Step 3: Implement a closed constructor**

```rust
pub struct VisualAnnotationProfile<'a> {
    skill_use: &'a crate::skills::SkillUse,
}

impl<'a> VisualAnnotationProfile<'a> {
    pub fn from_skill(skill_use: &'a crate::skills::SkillUse) -> Result<Self, DriverError> {
        if skill_use.package_id().as_str() != ACTION_GUIDE_VISUAL_ANNOTATIONS_PACKAGE_ID
            || skill_use.source_authority().as_str() != "rollshot.bundled"
        {
            return Err(DriverError::AgentProtocolFailure(
                "unexpected visual annotation skill".to_owned(),
            ));
        }
        Ok(Self { skill_use })
    }

    pub fn system_prompt(&self) -> &str {
        self.skill_use.body()
    }
}
```

No constructor accepts an arbitrary prompt. Do not add a digest-bearing model envelope.

- [ ] **Step 4: Make the bespoke runner and active caller consume the profile**

Add `profile: VisualAnnotationProfile<'_>` as the first runner argument and
replace the hardcoded system prompt with `profile.system_prompt().to_owned()`.
Update runner tests to resolve the bundled skill and construct the profile.
Update `suggest_visual_annotation_task` in the same commit: resolve
`bundled_action_guide_visual_annotations_use`, construct the profile, and pass
it to the runner. Missing/wrong skill resolution uses the existing generic
failure path; do not add user copy.

- [ ] **Step 5: Prove model request bytes did not change**

Use the scripted provider request capture to assert `request.system_prompt.as_deref() == Some(VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE)` and the existing user message is unchanged.

- [ ] **Step 6: Run visual runner and active caller tests, then commit**

```text
rtk cargo test -p rollshot-agent visual_annotation
rtk cargo test -p rollshot-app --features action-guide visual_annotation_agent
rtk git add crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/visual_annotation.rs crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs
rtk git commit -m "refactor(agent): drive visual runs from bundled skill"
```

---

## Task 8: Build reusable visual authority-denial enforcement

This task adds the typed enforcement primitive without changing the active
runner signature. Task 11 performs the clean caller cutover after durable
task/run IDs exist.

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`.
- Modify: `crates/rollshot-agent/src/visual_annotation.rs`.
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`
  (exhaustive terminal mapping only).
- Test: all three files.

**Interfaces:**
- Produces terminal variants `AuthorityDenied { operation }` and
  `AuditFailure { category }`.
- Produces one private async
  `authorize_visual_operation(authority, subject, operation, operation_name,
  audit_sink) -> Result<(), VisualAnnotationRunTerminal>` helper used by
  Task 11.
- Consumes: Task 5 operation and Task 7 profile.

- [ ] **Step 1: Write failing helper denial tests**

Construct authority without `DiscloseScreenshotAttachment` and call the helper
directly:

```rust
let result = authorize_visual_operation(
    &authority_without_attachment_grant(),
    &subject,
    RunOperation::DiscloseScreenshotAttachment,
    "model_attachment",
    Some(&sink),
)
.await;
assert_eq!(
    result,
    Err(VisualAnnotationRunTerminal::AuthorityDenied {
        operation: RunOperation::DiscloseScreenshotAttachment,
    }),
);
assert_eq!(sink.authority_denied_count(), 1);
```

Repeat for wrong subject and missing `SubmitReviewCandidate`.

- [ ] **Step 2: Write failing audit-append test**

Use an audit sink that returns `AuditAppendError::AppendFailed`. Assert the
helper returns `VisualAnnotationRunTerminal::AuditFailure { category }`, not
`ProtocolFailure`.

- [ ] **Step 3: Run RED**

```text
rtk cargo test -p rollshot-agent visual_authority_helper_records_denial
rtk cargo test -p rollshot-agent visual_authority_helper_preserves_audit_failure
```

Expected: terminal variants and helper are absent.

- [ ] **Step 4: Extract one private denial recorder**

Extract the existing single-submit denial-envelope/append logic into a private
async driver helper used by the single-submit path and the new visual helper.
It takes authority, operation, operation name, tracing target, and sink. It
returns `AuditFailureCategory` only when durable denial evidence cannot be
acknowledged. This is private refactoring; audit vocabulary and public
contracts stay unchanged.

- [ ] **Step 5: Implement the visual operation helper**

Call unchanged `authority.authorize_tool(authority.run_id(), subject,
operation)`. On denial, append evidence synchronously before returning the new
typed terminal. A missing sink returns `ProtocolFailure`; it never reports an
unaudited typed denial.

- [ ] **Step 6: Keep the active app mapping exhaustive**

Map both new terminals to the existing visual-annotation failure result/copy.
Add mapping assertions; do not add or change user-visible strings. The active
runner cannot emit these variants until Task 11.

- [ ] **Step 7: Run visual, app-mapping, and caption denial tests**

```text
rtk cargo test -p rollshot-agent visual_authority_helper
rtk cargo test -p rollshot-agent single_submit
rtk cargo test -p rollshot-app --features action-guide visual_annotation_agent
```

Expected: pass; the active visual runner is still behavior-identical and the
single-submit denial path remains green.

- [ ] **Step 8: Commit**

```text
rtk git add crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/visual_annotation.rs crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs
rtk git commit -m "refactor(agent): share authority denial enforcement"
```

---
## Task 9: Build visual source binding and immutable authority in the app

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`.
- Test: same file.

**Interfaces:**
- Produces:

```rust
fn visual_source_binding(
    context: &PreparedVisualAnnotationContext,
    step_source: u64,
    keyframe: u64,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
) -> SourceBinding;

fn visual_authority(
    task_id: ProductTaskId,
    run_id: RunId,
    subject: AuthoritySubject,
) -> Result<AuthoritySnapshot, String>;
```

- Consumes: Tasks 3–5 digests, origin, bindings, operation.

- [ ] **Step 1: Write failing durable/ephemeral binding tests**
Create prepared durable and ephemeral fixtures and assert every binding field.
Assert captions and visuals with the same root never identity-match.

- [ ] **Step 2: Write failing authority tests**

```rust
let authority = visual_authority(
    task_id(),
    run_id(),
    AuthoritySubject::Document(document_binding()),
)
.unwrap();
assert_eq!(authority.disclosure(), DisclosureCeiling::FullScreenshot);
assert!(authority
    .authorize_tool(
        authority.run_id(),
        &AuthoritySubject::Document(document_binding()),
        RunOperation::DiscloseScreenshotAttachment,
    )
    .is_ok());
assert!(authority
    .authorize_tool(
        authority.run_id(),
        &AuthoritySubject::Document(document_binding()),
        RunOperation::InspectPreparedImage,
    )
    .is_err());
```

Add `caption_authority_grants_only_submit_and_forbids_images` assertion for the new operation.

- [ ] **Step 3: Run RED**

```text
rtk cargo test -p rollshot-app --features action-guide visual_authority_grants_only_attachment_and_submit
```

Expected: helper missing.

- [ ] **Step 4: Implement binding and subject construction**

Build `DocumentContentBinding` from keyframe digest and:

```rust
AnnotationStateV1 {
    width: image.width(),
    height: image.height(),
    state_id: u32::try_from(document_state_id)
        .map_err(|_| "visual annotation document state id exceeds u32".to_owned())?,
    annotations: vec![],
}
```

Use `DisclosureCeiling::FullScreenshot`, `existing_product_capture = true`, no
prepared capabilities, and exactly the two grants. Add a separate
`#[cfg(test)]` `visual_authority_with_grants` helper to exercise each denial;
keep the production `visual_authority` signature above closed and always grant
both operations.

- [ ] **Step 5: Run authority/binding tests**

```text
rtk cargo test -p rollshot-app --features action-guide visual_authority
rtk cargo test -p rollshot-app --features action-guide caption_authority
```

Expected: pass.

- [ ] **Step 6: Commit**

```text
rtk git add crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/caption_agent.rs
rtk git commit -m "feat(action-guide): bind visual run authority"
```

---

## Task 10: Prepare durable visual state and context selection

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`.
- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs`.
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`.
- Modify tests in those files.

**Interfaces:**
- Produces workspace fields `visual_annotation_task_id`,
  `visual_annotation_review_snapshot`, and
  `visual_annotation_review_persisting`.
- Produces a pure `visual_annotation_context_request(&TimelineWorkspace) ->
  VisualAnnotationContextRequest` selection helper.
- Consumes: Task 3 context request.

- [ ] **Step 1: Add failing initialization and context-selection tests**

Prove all normal, imported-video, project-open, and test constructors initialize
the three fields empty/false. Prove a saved clean workspace constructs
`VisualAnnotationContextRequest::Durable`; saved dirty and unsaved workspaces
construct `Ephemeral` with the current guide.

- [ ] **Step 2: Run RED**

```text
rtk cargo test -p rollshot-app --features action-guide visual_context_request_uses_durable_only_for_saved_clean_project
```

Expected: fields and helper are absent.

- [ ] **Step 3: Add and initialize state fields everywhere**

```rust
pub(crate) visual_annotation_task_id: Option<ProductTaskId>,
pub(crate) visual_annotation_review_snapshot: Option<ProductTaskSnapshot>,
pub(crate) visual_annotation_review_persisting: bool,
```

Do not change `VisualAnnotationSuggestionState` visible variants.

- [ ] **Step 4: Implement the pure context selector**

Return durable context only for `ProjectSession::Saved` plus
`ProjectSaveState::Clean`; otherwise clone the guide into the ephemeral
request. Do not change the active worker call in this task.

- [ ] **Step 5: Run state tests and commit**

```text
rtk cargo test -p rollshot-app --features action-guide visual_context_request
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/project.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "refactor(action-guide): prepare durable visual task state"
```

---
## Task 11: Run visual suggestions as audited durable tasks

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs` (authorized runner cutover).
- Modify: `crates/rollshot-agent/src/visual_annotation.rs` (terminal tests).
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`.
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`.
- Test: all three modules.

**Interfaces:**
- Produces the locked `VisualAnnotationTaskResult`,
  `VisualAnnotationRunSuccess`, and `suggest_visual_annotation_task`
  signatures plus:

```rust
fn promote_visual_ready_for_review(
    store: &TaskStore,
    task_id: &ProductTaskId,
    proposal: &VisualAnnotationProposal,
    provider_id: &str,
    model_id: &str,
) -> Result<ProductTaskSnapshot, String>;
```

Every success returned to iced already carries the durable `ReadyForReview`
snapshot.
- Consumes: Tasks 3, 6–10.

- [ ] **Step 1: Write a failing lifecycle test around the real worker**

Use a temp `TaskStore`, scripted provider returning one valid tool call, durable
fixture project, and real worker. Assert:

```rust
let result = suggest_visual_annotation_task(
    VisualAnnotationTaskInput {
        run_id: 7,
        step: step(),
        document_state_id: 0,
        image: image_fixture(),
        annotations: Vec::new(),
    },
    VisualAnnotationContextRequest::Durable {
        root: project_root.clone(),
        expected_revision: 3,
    },
    store.clone(),
    "test-provider".to_owned(),
    "test-model".to_owned(),
    scripted_adapter(),
    RunCancellation::new(),
)
.await
.unwrap();
let VisualAnnotationTaskResult::Proposal(success) = result else {
    panic!("expected visual proposal");
};
let loaded = store.load(&success.task_id).unwrap();
assert_eq!(loaded.kind(), TaskKind::ActionGuideVisualAnnotation);
assert_eq!(loaded.status(), &TaskStatus::ReadyForReview);
let contract = loaded.run_contract().unwrap();
assert_eq!(
    contract.skill_use.package_id,
    "action-guide-visual-annotations",
);
assert_eq!(
    contract.authority.disclosure_ceiling,
    DisclosureCeiling::FullScreenshot,
);
```

Assert audit order starts `TaskCreated`, `AttemptStarted`,
`RunContractBound`, `ArtifactPromoted`. Assert metadata kind/source,
provider/model, suggestion count, proposal ID, artifact revision,
canonical-payload hash, and run contract. Decode `pending_proposal_payload` as
`VisualAnnotationProposal` and assert equality.

- [ ] **Step 2: Write failing terminal, dispatch, and payload-privacy tests**

Drive cancellation, budget exhaustion, provider failure, protocol failure,
attachment denial, and submit denial. For each, reload the task and assert the
typed terminal and absence of artifact metadata. A counting provider must see
zero requests when attachment disclosure is denied, and exactly one request
when attachment is granted but terminal submit is denied.

Use a source image containing the byte sequence
`52 4f 4c 4c 53 48 4f 54`. Assert neither artifact nor proposal payload
contains it or the PNG signature `89 50 4e 47 0d 0a 1a 0a`.

- [ ] **Step 3: Write failing iced installation tests**

Prove consent confirmation with no task store makes no provider task and uses
the existing generic failure copy. Prove returned success stores task
ID/snapshot before showing `PendingReview`, and a late success with another
local run ID is ignored.

- [ ] **Step 4: Run RED**

```text
rtk cargo test -p rollshot-app --features action-guide real_visual_worker_binds_audited_run_contract
rtk cargo test -p rollshot-app --features action-guide visual_success_installs_task_snapshot_before_review
```

Expected: the worker has no store lifecycle and workspace state is not wired.

- [ ] **Step 5: Implement create/start/bind sequence**

Follow the exact audited order already used by captions, but keep code local to
`visual_annotation_agent.rs`:

```text
ProductTaskSnapshot::new_v3
TaskStore::create_audited
TaskAttempt::new + start_attempt
TaskStore::transition_audited
resolve bundled skill
construct subject/authority
RunContractReceiptV1 {
    authority: authority.receipt(now),
    skill_use: skill.receipt(),
    bound_at_unix_ms: now,
}
bind_run_contract
TaskStore::transition_audited
```

All blocking store operations run under `spawn_blocking`. Once task creation
succeeds, every return path calls one `persist_visual_terminal` helper. Do not
extract a cross-workload abstraction.

- [ ] **Step 6: Cut the active runner over to authority and audit**

Change the runner to the final signature:

```rust
pub async fn run_visual_annotation_with_provider(
    &self,
    profile: VisualAnnotationProfile<'_>,
    input: AuthorizedModelInput,
    provider: &dyn ProviderAdapter,
    budget: RunBudget,
    cancellation: &RunCancellation,
    authority: &AuthoritySnapshot,
    subject: &AuthoritySubject,
    audit_sink: Option<&dyn AuditAppendSink>,
) -> VisualAnnotationRunTerminal;
```

Before provider dispatch, call Task 8's helper for
`DiscloseScreenshotAttachment`, then `authority.validate_model_input(&input)`.
Before accepting the terminal submit tool, call it for
`SubmitReviewCandidate`. Pass `TaskAuditSink` into the runner and replace
`NullEventSink` in `drive_streamed_turn`; propagate typed audit failure. Delete
the old signature in the same change. Encode the same PNG, construct the same
user prompt and `AuthorizedModelInput`, and preserve terminal-to-copy mapping.

- [ ] **Step 7: Validate and promote before returning success**

Convert terminal drafts into the Task 2 proposal. Serialize it once and
populate both Product Task payload fields because the API owns them separately:

```rust
let proposal_payload = serde_json::to_vec(proposal)
    .map_err(|error| format!("serialize visual proposal: {error}"))?;
let artifact_payload = proposal_payload.clone();
let promoted = snapshot.record_ready_for_review(
    metadata,
    artifact_payload,
    Some(proposal_payload),
    now,
)?;
```

Build `ProductArtifactMetadata::new_v3` with
`ArtifactKind::ActionGuideVisualAnnotation`, schema 1,
`ArtifactSummary::ActionGuideVisualAnnotation`, the real source binding and
provider/model IDs, and the bound run contract. Persist with
`transition_audited` before returning `VisualAnnotationRunSuccess`.
Serialization, metadata, CAS, or audit failure records `RuntimeFailure` or
`AuditFailure`; no failure returns a proposal.

- [ ] **Step 8: Wire the active iced caller and install success atomically**

On consent confirmation, require `state.task_store.clone()` and call Task 10's
context selector. Copy `doc.document.annotations().to_vec()` into
`VisualAnnotationTaskInput` alongside the unchanged source image and state ID.
Keep provider/model recheck before task creation and the exact running message.
On matching success, set task ID and snapshot before `PendingReview` and the
exact ready message. On failure, clear visual task/review state and use the
existing failure mapping. Cancellation cancels the worker; the worker persists
its terminal.

- [ ] **Step 9: Run worker, state-machine, and caption regression tests**

```text
rtk cargo test -p rollshot-app --features action-guide real_visual_worker
rtk cargo test -p rollshot-app --features action-guide visual_success_installs_task_snapshot_before_review
rtk cargo test -p rollshot-app --features action-guide real_worker_persists
```

Expected: pass.

- [ ] **Step 10: Commit**

```text
rtk git add crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/visual_annotation.rs crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action-guide): run visual suggestions as durable tasks"
```

---
## Task 12: Persist ordered visual review and exact receipt

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs` (receipt helper).
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs` (ordered persistence).
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs` and `view.rs` tests.

**Interfaces:**
- Produces:

```rust
fn visual_review_receipt(
    proposal: &VisualAnnotationProposal,
    metadata: &ProductArtifactMetadata,
    resulting_document_state_id: u64,
    resulting_annotation_digest: [u8; 32],
    now: i64,
) -> Result<ReviewReceipt, String>;

async fn persist_visual_review_batch(
    store: Arc<TaskStore>,
    snapshot: ProductTaskSnapshot,
    proposal: VisualAnnotationProposal,
    resulting_document_state_id: u64,
    resulting_annotation_digest: [u8; 32],
    has_accepted: bool,
) -> Result<ProductTaskSnapshot, String>;
```

- Consumes: Task 11 promoted snapshot and Task 10 workspace fields.

- [ ] **Step 1: Write failing receipt partition test**

Construct a proposal with one accepted, one rejected, and no pending items.
Assert receipt artifact/revision/proposal IDs, applied/rejected IDs, empty local
delta, and resulting state/digest.

- [ ] **Step 2: Write failing ordered interaction tests**

Test per-item Accept, per-item Reject, Accept all, Reject all, and
persistence-in-flight suppression. Independently invalidate selected step,
keyframe, dimensions, current state ID, keyframe digest, annotation-state
digest, and artifact revision; assert no mutation or receipt. Assert Accept all
validates every pending item before applying any edit. Assert first valid
decision produces `Applying`; final decision produces `Completed` when any
accepted, otherwise `Rejected`.

- [ ] **Step 3: Run RED**

```text
rtk cargo test -p rollshot-app --features action-guide visual_review_receipt_binds_exact_artifact_revision
rtk cargo test -p rollshot-app --features action-guide visual_review_controls_do_not_emit_while_persisting
```

Expected: no receipt/persistence state.

- [ ] **Step 4: Implement receipt and persistence**

Use `begin_apply`, `complete_apply`, and `reject_apply`. The receipt uses `u32::try_from` for suggestion IDs and resulting document state; overflow is an explicit persistence error. Compute the resulting annotation digest from the actual post-apply document annotations.

- [ ] **Step 5: Route every existing decision through one scheduler**

After the current mutation/rejection logic, schedule persistence. While `visual_annotation_review_persisting`, handlers return `Update::none()`. The completion message carries task ID and drops stale results for another task. Keep every current accept/reject/stale message exact.

`DismissVisualAnnotationReview` clears only memory UI and does not invent a receipt or transition.

- [ ] **Step 6: Exercise persistence failure**

Inject a store/audit failure. Assert proposal remains visible, controls re-enable, task failure follows typed audit semantics, and the only displayed copy is the existing generic failure string.

- [ ] **Step 7: Run review tests and commit**

```text
rtk cargo test -p rollshot-app --features action-guide visual_review
rtk git add crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(action-guide): persist visual annotation review"
```

---

## Task 13: Restore matching durable visual proposals without a provider call

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`.
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`.
- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs`.
- Test: all three modules.

**Interfaces:**
- Produces:

```rust
fn restore_visual_annotation_proposal(
    store: &TaskStore,
    binding: &SourceBinding,
    current_step: &GuideStep,
    current_document_state_id: u64,
    image_width: u32,
    image_height: u32,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
    now: i64,
) -> Option<(ProductTaskSnapshot, VisualAnnotationProposal)>;

fn restore_visual_annotation_proposal_for_selected_step(
    state: &mut TimelineWorkspace,
);
```

- Consumes: Tasks 2–5 bindings/digests, Task 11 payload, Task 10 state.

- [ ] **Step 1: Write failing provider-free restore test**

Persist a real `ReadyForReview` visual task for a temp saved project, reopen it, hydrate the selected step, and use a provider adapter that panics on any call. Assert state becomes `PendingReview`, task ID/snapshot are installed, and request count remains zero.

- [ ] **Step 2: Write failing deterministic stale tests**

Independently change revision, projection digest, step source, keyframe, image pixel, annotation list, artifact kind, proposal ID, and payload digest. Assert no proposal is displayed and same-identity freshness changes audited-transition the task to `Stale`.

- [ ] **Step 3: Run RED**

```text
rtk cargo test -p rollshot-app --features action-guide opening_project_restores_visual_review_without_provider
```

Expected: no restore path.

- [ ] **Step 4: Implement restore helper**

Call unchanged `reconcile_for_source`, reject wrong task/artifact kind, decode `pending_proposal_payload`, validate artifact/proposal identity, then call `rebase_restored`. Return only on `Ready`. Log identifiers/digests only; never payload text or paths.

- [ ] **Step 5: Trigger restore at deterministic hydration points**

After project open and after selected-step/keyframe hydration, attempt restore only for saved clean projects and only if no visual run/review/persistence is active. On step selection change, dismiss the old memory review first, then restore the selected step's matching task.

- [ ] **Step 6: Run restore/state tests and commit**

```text
rtk cargo test -p rollshot-app --features action-guide restore_visual
rtk cargo test -p rollshot-app --features action-guide opening_project
rtk git add crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/project.rs
rtk git commit -m "feat(action-guide): restore visual annotation reviews"
```

---

## Task 14: Audit manual staleness, reconciliation, and privacy/failpoints

The request works end-to-end after Task 13. This task closes required failure and privacy evidence before UI/gate verification.

**Files:**
- Modify tests/helpers: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`.
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`.
- Modify tests: `crates/rollshot-app/src/agent_store/task_store.rs` and `audit_store/mod.rs`.

**Interfaces:**
- Produces named Gate B1 audit/privacy/failpoint tests.
- Consumes all prior production interfaces.

- [ ] **Step 1: Write failing manual-stale audit tests**

For manual annotation, undo, redo, keyframe replacement, and step deletion, begin with a durable pending review. Assert existing stale copy remains exact and the task reloads as `Stale` with a material terminal audit event.

- [ ] **Step 2: Write complete lifecycle audit test**

Drive success through final review and assert durable order contains exactly the applicable material events:

```text
TaskCreated
AttemptStarted
RunContractBound
ArtifactPromoted
ReviewApplyStarted
ReviewDecisionCommitted
TaskTerminated
```

Drive authority denial and assert `AuthorityDenied` precedes `TaskTerminated`
with no `ArtifactPromoted`.

- [ ] **Step 3: Write serialization and tracing privacy test**

Capture and inspect task JSON, artifact/proposal payload, receipts, audit JSONL,
and structured tracing output. Assert absence of PNG signature, sentinel RGBA
bytes, full skill body prefix, API key fixture, provider-native text, project
root string, and raw semantic input. Assert allowed IDs/digests exist.

- [ ] **Step 4: Write failpoint matrix**

Use existing store/audit failpoints at create, attempt transition, run-contract transition, promotion, begin apply, final review, and terminal append. For each, assert no false success, no duplicate review receipt, legal terminal state, and hash-chain reconciliation.

- [ ] **Step 5: Run RED then implement minimal stale scheduling**

Run the new tests first. Add one helper that takes the current visual snapshot/task ID and schedules audited `mark_stale`; call it from the existing `dismiss_stale_visual_annotation_review` paths. Do not create a second stale policy.

- [ ] **Step 6: Run audit/privacy/store suites**

```text
rtk cargo test -p rollshot-app --features action-guide visual_task_lifecycle_appends_every_material_event
rtk cargo test -p rollshot-app --features action-guide visual_task_files_hold_no_image_or_skill_body
rtk cargo test -p rollshot-app --features action-guide agent_store
```

Expected: pass.

- [ ] **Step 7: Commit**

```text
rtk git add crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/agent_store/task_store.rs crates/rollshot-app/src/agent_store/audit_store/mod.rs
rtk git commit -m "test(action-guide): cover visual provenance failures"
```

---

## Task 15: Produce iced restore evidence with independent visual review

Before this task, re-read `skill://testing-iced-ui` and `skill://iced-rs` in the executing context.

**Files:**
- Modify tests only: `crates/rollshot-app/src/timeline_workspace/view.rs`.
- Artifacts: `target/ui-artifacts/timeline-workspace/` (never commit generated actual/diff files unless the existing harness explicitly tracks an approved baseline path).

**Interfaces:**
- Produces deterministic structural, interaction, and visual evidence.
- Consumes Task 13 restored state and Task 12 persistence suppression.

- [ ] **Step 1: Add failing structural restore scenario**

Build a state restored from a real serialized visual proposal. At `1100×760`, assert visible bounds for `Suggested annotations`, Accept all, Reject all, Dismiss, per-item Accept, and per-item Reject. Assert per-item buttons differ from header controls.

- [ ] **Step 2: Add minimum and long-content scenarios**

Run the minimum scenario at the product's configured `640×420` window and the
long-content scenario at `1100×760` with 20 suggestions. Assert the review
region remains reachable through the existing scroll path, controls are not
obscured, and no new copy appears.

- [ ] **Step 3: Add interaction suppression assertion**

Set `visual_annotation_review_persisting = true`, simulate all review controls, and assert no Accept/Reject/Suggest message is emitted.

- [ ] **Step 4: Run semantic tests before screenshots**

```text
rtk cargo test -p rollshot-app --features action-guide visual_annotation_review_has_per_item_buttons
rtk cargo test -p rollshot-app --features action-guide visual_annotation_review_controls_do_not_emit_while_persisting
```

Expected: pass.

- [ ] **Step 5: Capture baseline/actual/diff evidence**

Follow the existing ignored `render_restore_caption_proposal_visual_scenario` pattern using pinned fonts, Dark theme, fixed viewports, and `Snapshot::matches_image`. Run the exact ignored visual test with `--ignored --nocapture` and record artifact paths.

- [ ] **Step 6: Send evidence to an independent clean-context reviewer**

Use a clean-context reviewer with only requirement, mode, changed files, scenario manifest, semantic output, baseline/actual/diff paths, allowed baseline paths, and exact update command. The product-changing context must not supply a verdict or edit a golden.

If semantic image capability is unavailable in that reviewer, use another capable clean-context reviewer or request explicit human mode; pixel-only acceptance is invalid.

- [ ] **Step 7: Record reviewer verdict and commit the scenario**

No tracked baseline path is allowed in this slice because the product layout is
unchanged and the existing visual harness writes comparison artifacts under
`target/`. The independent reviewer records ACCEPT or REJECT without product
file writes.

```text
rtk git add crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "test(ui): cover restored visual annotation review"
```

---

## Task 16: Verify Gate B1 and record the decision

This is the final phase after the product path and smoke scenario work.

**Files:**
- Create: `docs/superpowers/spikes/2026-07-29-action-guide-visual-annotation-decision.md`.
- Do not edit the historical Slice A spec/plan/decision.
- Do not edit the umbrella unless a non-additive discovery requires the amendment process.

**Interfaces:**
- Produces Gate B1 decision and explicit compatibility review artifact.
- Consumes all named test/reviewer evidence.

- [ ] **Step 1: Smoke the actual changed flow**

Run the deterministic scripted product scenario that opens a saved project, confirms existing consent, executes one real scripted visual provider response, promotes the artifact, reviews one suggestion, closes/reopens, and confirms no provider call on restore. Record exact command and output counts.

- [ ] **Step 2: Run full required suites**

```text
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: zero failures and zero clippy warnings. Record passed/failed/ignored counts from fresh output. No core benchmark is required because no stitching path changed.

- [ ] **Step 3: Run an independent code review**

Request review of the full branch diff against the approved spec and umbrella. Require explicit findings for correctness, privacy, task/audit crash consistency, frozen copy/prompt, and Gate B1 compatibility.

Fix every Important-or-higher finding test-first, rerun the affected focused suite, then rerun Step 2. Do not amend prior commits; create focused fix commits.

- [ ] **Step 4: Build the compatibility artifact**

Record a table comparing Slice A baseline to final declarations and APIs:

```text
Existing SourceBinding variants/fields: unchanged
Existing ProductArtifactMetadata fields/deserializer: unchanged
TaskStore public API: unchanged
AuditEventKindV1 vocabulary: unchanged
Legacy schema 1/2 fixtures: load successfully
New shared changes: only the Task 4/5 variants
```

This is review evidence, not a source-text unit test.

- [ ] **Step 5: Write the Gate B1 decision**

The decision file contains: selected architecture, all twelve Gate B1 items mapped to named evidence, migrations, compatibility artifact, privacy evidence, iced reviewer verdict, verification counts, residual risks, deferred scope, and `Gate B1: VERIFIED` only if every item is evidenced.

Do not write `VERIFIED` if any suite, review, visual verdict, or compatibility item is missing.

- [ ] **Step 6: Self-check the decision and commit**

Scan for placeholders/contradictions, verify command outputs are fresh, then:

```text
rtk git add docs/superpowers/spikes/2026-07-29-action-guide-visual-annotation-decision.md
rtk git commit -m "docs(agent): record Action Guide visual annotation gate"
```

- [ ] **Step 7: Present umbrella completion for user approval**

Summarize Gate A1 + Gate B1, migrations, residual risks, deferred scope, and confirm the launch-video boundary remains closed. Do not mark the umbrella historical until the user explicitly approves its completion decision.
