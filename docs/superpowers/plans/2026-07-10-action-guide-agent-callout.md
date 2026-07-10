# Action Guide Agent Callout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user request, preview, accept, or reject one agent-suggested Number Callout for the currently selected Action Guide step.

**Architecture:** Extend Rollshot's provider-neutral model request with one authorized image attachment and convert it to Rig image content inside the existing Anthropic/OpenAI adapters. Add a bounded callout runner profile and a step-bound proposal contract, then integrate the result as a non-mutating ghost in the shared iced annotation modal; accepted proposals become one normal `ImageDocument` edit.

**Tech Stack:** Rust, Rig 0.39.0 internal message/provider transport, existing Rollshot bounded-agent contracts, `image` PNG encoding, iced 0.14 Canvas, `rollshot-image-document` history.

## Global Constraints

- Work on the existing `feat/action-guide-agent-callout` branch; do not create a worktree.
- Prefix every shell command with `rtk`.
- Keep Rig types internal to `rollshot-agent`; no Rig type may cross Rollshot's public request, event, authorization, error, budget, or privacy boundaries.
- Support exactly one authorized PNG or JPEG attachment for a callout run; do not add URL, file-ID, video, PDF, or general-purpose media input.
- The agent returns at most one tip or `no_suggestion`; it never chooses the bubble or annotation number.
- Do not expose automation, OCR, template, edit-proposal, or image-inspection tools to the callout run.
- Do not mutate original retained keyframes or commit a ghost proposal before acceptance.
- Runtime diagnostics use stable `rollshot::*` targets and structured fields; never log prompts, attachment bytes, rationale, raw tool arguments, API keys, provider payloads, or image coordinates.
- The Timeline Workspace and annotation modal remain shared between Linux and macOS; introduce no platform-specific behavior.
- Follow TDD: each behavior starts with a focused failing test and ends with the smallest implementation that passes it.

---

## File Structure

- Modify `crates/rollshot-agent/src/domain.rs`: convert authorized attachments into redacted provider-neutral model attachments.
- Modify `crates/rollshot-agent/src/model.rs`: define `ModelAttachment` and add `attachments` to `ModelRequest`.
- Modify `crates/rollshot-agent/src/provider.rs`: convert attachments to Rig `UserContent::Image` for existing Anthropic/OpenAI adapters.
- Create `crates/rollshot-agent/src/callout.rs`: bounded callout task profile, terminal schema, output validation, and Rig turn/tool lifecycle.
- Modify `crates/rollshot-agent/src/lib.rs`: export Rollshot-owned callout runner/output types.
- Create `crates/rollshot-action/src/callout_proposal.rs`: step-bound proposal, provenance, accept/reject/stale policy.
- Modify `crates/rollshot-action/src/lib.rs`: export callout proposal types.
- Create `crates/rollshot-image-document/src/callout_placement.rs`: deterministic bubble-placement function.
- Modify `crates/rollshot-image-document/src/lib.rs`: export the placement function/options.
- Create `crates/rollshot-app/src/timeline_workspace/callout_agent.rs`: encode the selected keyframe, authorize it, and run the bounded callout task.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`: callout run/proposal/modal state.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`: request, cancellation, load, accept, reject, close, and stale transitions.
- Modify `crates/rollshot-app/src/timeline_workspace/annotation.rs`: ghost-callout Canvas rendering.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`: selected-step action and modal loading/review controls.

---

### Task 1: Provider-Neutral Authorized Image Attachments

**Files:**
- Modify: `crates/rollshot-agent/src/model.rs`
- Modify: `crates/rollshot-agent/src/domain.rs`
- Modify: every `ModelRequest { ... }` literal under `crates/rollshot-agent/src/` and `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`

**Interfaces:**
- Produces: `ModelAttachment`, `ModelRequest::attachments`, and `AuthorizedModelInput::model_attachments()`.
- Consumes: existing `MediaType`, `AttachmentDescriptor`, and validated attachment bytes.

- [ ] **Step 1: Write failing model attachment redaction tests**

Add to `model.rs` tests:

```rust
#[test]
fn model_attachment_debug_redacts_bytes() {
    let attachment = ModelAttachment::new(
        crate::domain::MediaType::Png,
        2,
        3,
        std::sync::Arc::from(b"PRIVATE_IMAGE_BYTES".as_slice()),
    );

    let debug = format!("{attachment:?}");
    assert!(debug.contains("Png"));
    assert!(debug.contains("byte_count"));
    assert!(!debug.contains("PRIVATE_IMAGE_BYTES"));
}

#[test]
fn text_request_has_no_attachments() {
    let request = ModelRequest {
        model: "m".into(),
        prompt: "p".into(),
        history: vec![],
        turn: 1,
        tool_definitions: vec![],
        system_prompt: None,
        max_tokens: None,
        attachments: vec![],
    };
    assert!(request.attachments.is_empty());
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `rtk cargo test -p rollshot-agent model_attachment_debug_redacts_bytes`

Expected: FAIL because `ModelAttachment` and `ModelRequest::attachments` do not exist.

- [ ] **Step 3: Add the minimal attachment type**

Add to `model.rs` before `ModelRequest`:

```rust
#[derive(Clone, PartialEq, Eq)]
pub struct ModelAttachment {
    media_type: crate::domain::MediaType,
    width: u32,
    height: u32,
    bytes: std::sync::Arc<[u8]>,
}

impl ModelAttachment {
    pub(crate) fn new(
        media_type: crate::domain::MediaType,
        width: u32,
        height: u32,
        bytes: std::sync::Arc<[u8]>,
    ) -> Self {
        Self { media_type, width, height, bytes }
    }

    pub(crate) fn media_type(&self) -> crate::domain::MediaType { self.media_type }
    pub(crate) fn width(&self) -> u32 { self.width }
    pub(crate) fn height(&self) -> u32 { self.height }
    pub(crate) fn bytes(&self) -> &[u8] { &self.bytes }
}

impl std::fmt::Debug for ModelAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelAttachment")
            .field("media_type", &self.media_type)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("byte_count", &self.bytes.len())
            .field("bytes", &"<redacted-attachment>")
            .finish()
    }
}
```

Add `pub attachments: Vec<ModelAttachment>` to `ModelRequest`. Update all existing request literals with `attachments: vec![]`; do not introduce a builder solely for this change.

- [ ] **Step 4: Write the failing authorized conversion test**

Add to `domain.rs` tests:

```rust
#[test]
fn authorized_input_builds_model_attachments_without_revalidation() {
    let input = AuthorizedModelInput::new(
        "anthropic".into(),
        "vision-model".into(),
        "inspect".into(),
        vec![AttachmentDescriptor {
            media_type: MediaType::Png,
            width: 2,
            height: 3,
            byte_count: 4,
        }],
        vec![vec![1, 2, 3, 4]],
    ).unwrap();

    let attachments = input.model_attachments();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].media_type(), MediaType::Png);
    assert_eq!(attachments[0].bytes(), &[1, 2, 3, 4]);
}
```

- [ ] **Step 5: Add the authorized conversion and run tests**

Add to `AuthorizedModelInput`:

```rust
pub(crate) fn model_attachments(&self) -> Vec<crate::model::ModelAttachment> {
    self.manifest.descriptors.iter().zip(&self.attachments)
        .map(|(descriptor, bytes)| crate::model::ModelAttachment::new(
            descriptor.media_type,
            descriptor.width,
            descriptor.height,
            std::sync::Arc::from(bytes.clone()),
        ))
        .collect()
}
```

Run: `rtk cargo test -p rollshot-agent model_attachment`

Expected: PASS, including redacted Debug output.

- [ ] **Step 6: Run the crate tests and commit**

Run: `rtk cargo test -p rollshot-agent`

Expected: PASS.

```bash
rtk git add crates/rollshot-agent/src/model.rs crates/rollshot-agent/src/domain.rs crates/rollshot-agent/src crates/rollshot-app/src/timeline_workspace/caption_agent.rs
rtk git commit -m "feat(agent): add authorized model attachments"
```

---

### Task 2: Rig Image Conversion in Existing Provider Adapters

**Files:**
- Modify: `crates/rollshot-agent/src/provider.rs`

**Interfaces:**
- Consumes: `ModelRequest::attachments` from Task 1.
- Produces: `build_completion_request` and `build_openai_completion_request` with a final Rig user message containing raw PNG/JPEG image content.

- [ ] **Step 1: Write failing common and OpenAI request tests**

Add these assertions for both builders:

Use this shared request helper:

```rust
fn image_request() -> ModelRequest {
    ModelRequest {
        model: "vision-model".into(),
        prompt: "Locate the target".into(),
        history: vec![],
        turn: 1,
        tool_definitions: vec![],
        system_prompt: None,
        max_tokens: Some(100),
        attachments: vec![crate::model::ModelAttachment::new(
            crate::domain::MediaType::Png,
            1,
            1,
            std::sync::Arc::from([1_u8, 2_u8]),
        )],
    }
}

fn assert_has_raw_png(request: CompletionRequest) {
    let last = request.chat_history.last();
    let Message::User { content } = last else { panic!("last message must be user image") };
    assert!(matches!(
        content.first(),
        UserContent::Image(rig_core::message::Image {
            data: rig_core::message::DocumentSourceKind::Raw(bytes),
            media_type: Some(rig_core::message::ImageMediaType::PNG),
            ..
        }) if bytes == &vec![1, 2]
    ));
}

#[test]
fn provider_request_contains_image() {
    assert_has_raw_png(build_completion_request(image_request()).unwrap());
}

#[test]
fn openai_provider_request_contains_image_and_serial_tool_calls() {
    let request = build_openai_completion_request(image_request()).unwrap();
    assert_eq!(
        request.additional_params,
        Some(serde_json::json!({"parallel_tool_calls": false}))
    );
    assert_has_raw_png(request);
}
```

- [ ] **Step 2: Run tests and verify the image assertion fails**

Run: `rtk cargo test -p rollshot-agent provider_request_contains_image`

Expected: FAIL because `build_completion_request` ignores attachments.

- [ ] **Step 3: Convert authorized attachments to Rig images**

Add a private conversion:

```rust
fn attachment_to_rig(attachment: &crate::model::ModelAttachment) -> UserContent {
    let media_type = match attachment.media_type() {
        crate::domain::MediaType::Png => rig_core::message::ImageMediaType::PNG,
        crate::domain::MediaType::Jpeg => rig_core::message::ImageMediaType::JPEG,
    };
    UserContent::image_raw(attachment.bytes().to_vec(), Some(media_type), None)
}
```

In `build_completion_request`, after adding non-empty prompt text, append one user message containing all converted attachments:

```rust
if !request.attachments.is_empty() {
    let images = request.attachments.iter().map(attachment_to_rig).collect::<Vec<_>>();
    chat_history.push(Message::User {
        content: rig_core::OneOrMany::many(images)
            .map_err(|e| ModelError::ProtocolFailure(e.to_string()))?,
    });
}
```

Keep raw bytes internal and let Rig's existing provider conversions perform provider-specific encoding.

- [ ] **Step 4: Add text-only regression and redaction assertions**

Assert an empty attachment list produces the exact previous chat history. Assert `format!("{:?}", image_request())` does not contain the raw byte sentinel or a base64 representation of it.

- [ ] **Step 5: Run provider and full crate tests**

Run:

```bash
rtk cargo test -p rollshot-agent provider
rtk cargo test -p rollshot-agent
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-agent/src/provider.rs
rtk git commit -m "feat(agent): send authorized images to providers"
```

---

### Task 3: Bounded Callout Agent Profile

**Files:**
- Create: `crates/rollshot-agent/src/callout.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Interfaces:**
- Consumes: `AuthorizedModelInput`, `ProviderAdapter`, `RunBudget`, `RunCancellation`, and Rig `AgentRun`.
- Produces: `run_callout_with_provider(input, provider, budget, cancellation) -> CalloutRunTerminal`.

- [ ] **Step 1: Write failing terminal payload validation tests**

Create `callout.rs` with tests for these exact public Rollshot-owned types:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutAgentSuggestion {
    pub x: f32,
    pub y: f32,
    pub confidence: f32,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalloutNoSuggestion { NoClearTarget { reason: Option<String> } }

#[derive(Debug, Clone, PartialEq)]
pub enum CalloutRunTerminal {
    Suggested(CalloutAgentSuggestion),
    NoSuggestion(CalloutNoSuggestion),
    Cancelled,
    BudgetExhausted { dimension: crate::runtime::BudgetDimension },
    ProviderFailure,
    ProtocolFailure,
}
```

Tests must cover valid suggestion, valid no-suggestion, non-finite coordinates, confidence outside `0.0..=1.0`, empty/oversized rationale, and multiple terminal calls. Use a 500-character maximum for rationale/reason.

- [ ] **Step 2: Run tests and verify they fail**

Run: `rtk cargo test -p rollshot-agent callout::tests`

Expected: FAIL because the module and decoder do not exist.

- [ ] **Step 3: Add the tagged terminal schema and decoder**

Use this serde input shape internally:

```rust
#[derive(serde::Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
enum SubmitCalloutArgs {
    Suggestion { tip: Tip, confidence: f32, rationale: Option<String> },
    NoSuggestion { reason: Option<String> },
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Tip { x: f32, y: f32 }
```

Implement `decode_submission(&serde_json::Value) -> Result<CalloutRunTerminal, String>` with finite/range/length validation and trimmed optional text. Do not clamp invalid input.

- [ ] **Step 4: Write failing Rig lifecycle tests**

Use a scripted `ProviderAdapter` stream to cover:

- one tool call returns `Suggested`;
- `no_suggestion` returns a successful terminal;
- completion without submission returns `ProtocolFailure`;
- a second terminal tool in the same response is rejected;
- cancellation before stream returns `Cancelled`;
- attachment usage greater than the budget returns `BudgetExhausted { Attachments }`;
- more than two model turns returns a turn-budget failure.

The advertised tool name must be exactly `submit_callout_suggestion`, with `additionalProperties: false` at every object level.

- [ ] **Step 5: Extract the two task-profile variables from the existing driver**

Add an internal profile enum to `driver.rs`:

```rust
pub(crate) enum AgentTaskProfile {
    SmartRedaction,
    Callout,
}

impl AgentTaskProfile {
    pub(crate) fn system_prompt(&self) -> &'static str {
        match self {
            Self::SmartRedaction => SMART_REDACTION_SYSTEM_PROMPT,
            Self::Callout => crate::callout::CALLOUT_SYSTEM_PROMPT,
        }
    }

    pub(crate) fn terminal_tools(&self) -> &'static [&'static str] {
        match self {
            Self::SmartRedaction => &["submit_for_review", "request_user_input"],
            Self::Callout => &["submit_callout_suggestion"],
        }
    }
}
```

Replace the hard-coded Smart Redaction prompt and terminal-tool set in shared model/tool turns with profile lookups. Existing `run_with_provider` always passes `SmartRedaction`, so its public behavior and terminal states remain unchanged.

- [ ] **Step 6: Implement the callout runner**

In `callout.rs`, drive `rig_core::agent::run::AgentRun` with `max_turns(2)`, the callout profile, and exactly one tool definition. Charge one attachment before the first model call, attach `input.model_attachments()` only to the first `ModelRequest`, and use an empty attachment list on any second turn.

Expose:

```rust
pub async fn run_callout_with_provider(
    input: crate::domain::AuthorizedModelInput,
    provider: &dyn crate::ProviderAdapter,
    budget: crate::runtime::RunBudget,
    cancellation: &crate::runtime::RunCancellation,
) -> CalloutRunTerminal
```

Map detailed provider/protocol messages only to privacy-safe tracing events; terminal values carry no provider payload or prompt text.

- [ ] **Step 7: Export types, run contract/privacy tests, and commit**

Add `pub mod callout;` and re-export the function/types from `lib.rs`.

Run:

```bash
rtk cargo test -p rollshot-agent callout
rtk cargo test -p rollshot-agent provider_contract
rtk cargo test -p rollshot-agent privacy
rtk cargo test -p rollshot-agent
```

Expected: PASS.

```bash
rtk git add crates/rollshot-agent/src/callout.rs crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/lib.rs
rtk git commit -m "feat(agent): add bounded callout suggestion run"
```

---

### Task 4: Step-Bound Callout Proposal Policy

**Files:**
- Create: `crates/rollshot-action/src/callout_proposal.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Modify: `crates/rollshot-action/Cargo.toml` only if `rollshot-image-document` is not already a dependency

**Interfaces:**
- Consumes: `GuideStep`, `CandidateId`, `FrameId`, `ImagePoint`, and agent draft output.
- Produces: `CalloutProposal`, `CalloutSuggestion`, `CalloutApplyOutcome`, and base-state matching.

- [ ] **Step 1: Write failing proposal construction tests**

Cover a valid in-bounds tip, missing step source, non-finite tip, edge-exclusive bounds (`x == width` is invalid), invalid confidence, trimmed rationale, and agent provenance.

Define the draft and constructor exactly as:

```rust
pub struct CalloutSuggestionDraft {
    pub tip: rollshot_image_document::ImagePoint,
    pub confidence: f32,
    pub rationale: Option<String>,
}

pub fn from_agent_draft(
    id: CalloutProposalId,
    run_id: u64,
    step: &GuideStep,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
    draft: CalloutSuggestionDraft,
) -> Result<CalloutProposal, CalloutProposalError>
```

- [ ] **Step 2: Run tests and verify they fail**

Run: `rtk cargo test -p rollshot-action callout_proposal`

Expected: FAIL because the module does not exist.

- [ ] **Step 3: Implement the proposal types and validation**

Use:

```rust
pub struct CalloutSuggestionBase {
    pub step_source: CandidateId,
    pub keyframe: FrameId,
    pub document_state_id: u64,
    pub image_width: u32,
    pub image_height: u32,
}

pub enum CalloutSuggestionStatus { Pending, Accepted, Rejected, Stale }
pub enum CalloutApplyOutcome { Ready, Missing, Stale, NotPending }
```

Keep the proposal independent of committed annotation storage. It owns one suggestion, not a vector.

- [ ] **Step 4: Write failing staleness transition tests**

Test exact matches and each mismatch independently: missing step, changed source, replaced keyframe, different document `state_id`, different dimensions, rejected proposal, and already accepted proposal. Also test that restoring the captured `state_id` makes the base match again.

- [ ] **Step 5: Implement accept/reject policy without mutating a document**

Expose:

```rust
pub fn validate_acceptance(
    &mut self,
    step: Option<&GuideStep>,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
) -> CalloutApplyOutcome

pub fn mark_applied(&mut self);
pub fn reject(&mut self) -> bool;
```

`validate_acceptance` returns `Ready` only as authorization for the app to perform the edit; the app must call `mark_applied` after `ImageDocument::add_number_callout` succeeds. Any base mismatch marks `Stale`.

- [ ] **Step 6: Export, test, and commit**

Run: `rtk cargo test -p rollshot-action callout_proposal`

Expected: PASS.

```bash
rtk git add crates/rollshot-action/src/callout_proposal.rs crates/rollshot-action/src/lib.rs crates/rollshot-action/Cargo.toml Cargo.lock
rtk git commit -m "feat(action): add callout proposal policy"
```

---

### Task 5: Deterministic Bubble Placement

**Files:**
- Create: `crates/rollshot-image-document/src/callout_placement.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`

**Interfaces:**
- Consumes: tip, image dimensions, and committed `Annotation` bounds.
- Produces: `place_number_callout_bubble(...) -> ImagePoint`.

- [ ] **Step 1: Write failing placement tests**

Use a fixed `CalloutPlacementOptions` with `offset = 32.0`, `bubble_radius = 12.0`, and `tip_protection_radius = 16.0`. Cover center preference for upper-right, each image corner, overlap avoidance with an existing Number Callout, deterministic upper-right tie-breaking, and a tiny image that requires clamping.

- [ ] **Step 2: Run tests and verify they fail**

Run: `rtk cargo test -p rollshot-image-document callout_placement`

Expected: FAIL because the module does not exist.

- [ ] **Step 3: Implement the pure placement function**

Expose:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalloutPlacementOptions {
    pub offset: f32,
    pub bubble_radius: f32,
    pub tip_protection_radius: f32,
}

pub fn place_number_callout_bubble(
    tip: ImagePoint,
    image_width: u32,
    image_height: u32,
    annotations: &[Annotation],
    options: CalloutPlacementOptions,
) -> ImagePoint
```

Generate candidates in upper-right, upper-left, lower-right, lower-left order. Score axis-aligned bubble bounds against the protected tip square and annotation render bounds; choose minimum overlap with stable first-candidate tie-breaking. Clamp the upper-right candidate when none fit.

- [ ] **Step 4: Run crate tests and commit**

Run: `rtk cargo test -p rollshot-image-document`

Expected: PASS.

```bash
rtk git add crates/rollshot-image-document/src/callout_placement.rs crates/rollshot-image-document/src/lib.rs
rtk git commit -m "feat(document): place suggested callout bubbles"
```

---

### Task 6: Timeline Callout Agent Orchestration

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/callout_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: selected `GuideStep`, original retained keyframe, current annotation `state_id`, provider adapter/config, and Task 3 runner.
- Produces: `suggest_callout_task(...) -> CalloutTaskResult` and workspace run state.

- [ ] **Step 1: Write failing prompt and PNG authorization tests**

In `callout_agent.rs`, test that the prompt contains source/index/title/caption/kind but no raw typed text field, and that a 2x3 RGBA keyframe becomes one PNG `AttachmentDescriptor` whose byte count matches the encoded bytes.

Use:

```rust
pub(crate) struct CalloutTaskInput {
    pub run_id: u64,
    pub step: rollshot_action::GuideStep,
    pub document_state_id: u64,
    pub image: image::RgbaImage,
}

pub(crate) enum CalloutTaskResult {
    Proposal(rollshot_action::CalloutProposal),
    NoSuggestion { reason: Option<String> },
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide callout_agent`

Expected: FAIL because the module does not exist.

- [ ] **Step 3: Implement image authorization and terminal mapping**

Encode with:

```rust
let mut png = Vec::new();
image::DynamicImage::ImageRgba8(input.image.clone())
    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
    .map_err(|error| format!("PNG encode failed: {error}"))?;
```

Construct one `AuthorizedModelInput` descriptor, invoke `run_callout_with_provider`, and map only `Suggested` into `CalloutProposal::from_agent_draft`. Map `NoSuggestion` separately; map cancelled/budget/provider/protocol terminals to fixed recoverable messages without forwarding sensitive provider text.

- [ ] **Step 4: Add workspace state and cancellation ownership**

Add to `TimelineWorkspace`:

```rust
pub(crate) callout_proposal: Option<rollshot_action::CalloutProposal>,
pub(crate) callout_suggestion_running: bool,
pub(crate) callout_agent_run_id: u64,
pub(crate) callout_cancellation: Option<rollshot_agent::runtime::RunCancellation>,
```

Initialize them to `None`, `false`, `0`, and `None`. Add these state tests in `update.rs` using the existing `ws` and `synthetic_recording` helpers:

```rust
#[test]
fn new_workspace_has_idle_callout_state() {
    let state = ws(synthetic_recording(1));
    assert!(state.callout_proposal.is_none());
    assert!(!state.callout_suggestion_running);
    assert_eq!(state.callout_agent_run_id, 0);
    assert!(state.callout_cancellation.is_none());
}

#[test]
fn replacing_keyframe_discards_pending_callout() {
    let mut state = ws(synthetic_recording(1));
    state.callout_proposal = Some(callout_proposal(&state));
    let replacement = state.strip.iter().map(|frame| frame.id)
        .find(|id| Some(*id) != state.selected_step().map(|step| step.keyframe))
        .expect("nearby replacement");
    let _ = update(&mut state, Message::ReplaceKeyframe(replacement));
    assert!(state.callout_proposal.is_none());
}
```

Define the test-only `callout_proposal(&TimelineWorkspace)` helper in Task 6 alongside the other proposal fixtures, using the selected step, its presentation document `state_id`, and image dimensions.

- [ ] **Step 5: Add request/load/cancel update messages**

Add:

```rust
SuggestCalloutRequested,
CalloutSuggestionLoaded(Result<super::callout_agent::CalloutTaskResult, String>),
CancelCalloutSuggestion,
RejectCalloutSuggestion,
AcceptCalloutSuggestion,
```

`SuggestCalloutRequested` must ensure a selected step, create its presentation document, snapshot `state_id`, clone the original retained image, load/build the configured provider exactly as caption suggestions do, open the annotation session, store one cancellation token, and launch `Task::perform`.

- [ ] **Step 6: Add focused update tests**

Cover missing selection, missing provider key, duplicate request suppression, successful proposal storage, no-suggestion message, cancellation cleanup, keyframe replacement cleanup, document edit staleness, reject with no mutation, and accept with one undoable callout.

- [ ] **Step 7: Run update tests and commit**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update`

Expected: PASS.

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/callout_agent.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): run selected-step callout suggestions"
```

---

### Task 7: Ghost Preview and Modal Review Controls

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: pending `CalloutProposal`, runner state, placement function, and existing annotation modal.
- Produces: distinct non-mutating ghost rendering plus Accept/Reject/Retry/Close UI.

- [ ] **Step 1: Write failing ghost projection tests**

Add a pure helper in `annotation.rs`:

```rust
pub(crate) fn suggested_callout_annotation(
    document: &ImageDocument,
    suggestion: &rollshot_action::CalloutSuggestion,
    width: u32,
    height: u32,
) -> Annotation
```

Test that it uses `document.next_number()`, preserves the proposal tip, uses `place_number_callout_bubble`, and does not change `state_id`, annotations, undo, or redo state.

- [ ] **Step 2: Run the test and verify it fails**

Run: `rtk cargo test -p rollshot-app --features action-guide suggested_callout_annotation`

Expected: FAIL because the helper does not exist.

- [ ] **Step 3: Extend the Canvas with an optional ghost**

Add `pub suggested: Option<Annotation>` to `NumberAnnotationCanvas`. Render committed annotations with existing colors, then render the suggestion with reduced alpha and a dashed/segmented outline implemented with iced Canvas paths. Do not add it to `ImageDocument`.

- [ ] **Step 4: Add the selected-step and modal controls**

In the detail panel, add `Suggest Callout` beside `Annotate Step`, disabled while a run is active. In the annotation modal:

- show `Suggesting callout...` and `Cancel` while running;
- disable Number/Text/Redaction, undo, redo, and canvas mutation while running;
- show `Suggested` plus `Accept` and `Reject` for a pending proposal;
- retain manual tools after rejection;
- show Retry/Close after no-suggestion or recoverable failure.

Use existing button styles and compact typography; do not create nested cards or a second annotation modal.

- [ ] **Step 5: Make modal close use the cancellation path**

Route both scrim close and Close through `CancelCalloutSuggestion` when a run is active, then close the annotation session. Closing with a pending proposal discards it. `AnnotationDone` must also clear pending callout state.

- [ ] **Step 6: Add view/state regression tests**

Test the pure state predicates used by the view: mutation disabled while running, pending proposal enables Accept/Reject, rejected proposal restores manual tools, accepted proposal disappears, and closing cancels/discards. Keep widget rendering assertions limited to existing project patterns.

- [ ] **Step 7: Run app tests and commit**

Run: `rtk cargo test -p rollshot-app --features action-guide`

Expected: PASS.

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/annotation.rs crates/rollshot-app/src/timeline_workspace/view.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): review suggested callouts in annotation modal"
```

---

### Task 8: Cross-Crate Verification and Real-Provider Smoke Test

**Files:**
- Modify only files required to correct failures introduced by Tasks 1-7; do not perform adjacent refactors.

**Interfaces:**
- Consumes: the complete vertical slice.
- Produces: verified provider contracts, privacy guarantees, shared UI build, and recorded runtime risk.

- [ ] **Step 1: Run formatting and focused suites**

```bash
rtk cargo fmt --check
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-app --features action-guide
```

Expected: all commands PASS.

- [ ] **Step 2: Run workspace clippy**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS with no warnings.

- [ ] **Step 3: Run a privacy sentinel audit**

Run the agent privacy tests with a unique fake image-byte sentinel and confirm captured diagnostics, terminal Debug, session Debug, and adapter errors do not contain it:

`rtk cargo test -p rollshot-agent privacy -- --nocapture`

Expected: PASS; sentinel absent from emitted output.

- [ ] **Step 4: Perform one real-provider smoke test**

With an already configured vision-capable Anthropic or OpenAI model, open an Action Guide, select one step, request a callout, verify the ghost appears, accept it, undo it, and export Storyboard preview. Confirm the original keyframe remains unchanged and the exported Storyboard contains the accepted callout before undo only.

Record only provider name, model name, pass/fail, and failure category in the PR notes. Do not record the screenshot, prompt, rationale, coordinates, or provider payload.

- [ ] **Step 5: Inspect the final diff and commit only necessary fixes**

Run:

```bash
rtk git diff --check
rtk git status --short
rtk git diff --stat main...HEAD
```

If verification required code fixes, inspect `rtk git status --short`, stage each listed path explicitly with `rtk git add path/to/file`, then commit:

```bash
rtk git commit -m "fix(action): harden agent callout integration"
```

If no fixes were needed, do not create an empty commit.
