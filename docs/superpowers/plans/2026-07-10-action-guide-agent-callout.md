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

## What Already Exists

- `AuthorizedModelInput` already owns attachment count/type/size authorization and privacy-safe `Debug`; extend it instead of creating a second image-input gate.
- `ModelRequest`, `ProviderAdapter`, `AnthropicAdapter`, and `OpenAIAdapter` already form the Rollshot-owned provider boundary while using Rig internally; preserve that boundary.
- Rig `AgentRun` already provides sans-I/O turn/tool-call sequencing; reuse it for the callout profile instead of writing an app-local loop.
- `RunBudget`, `BudgetTracker`, and `RunCancellation` already cover model, token, tool, attachment, and wall-time limits; define a tight callout budget with these types.
- `CaptionProposal` already establishes provenance and stale proposal status patterns; mirror the pattern without forcing callouts into the caption model.
- `ImageDocument::state_id`, `add_number_callout`, undo/redo, `annotation_bounds`, and Number Callout style tokens already solve committed-edit identity, history, collision bounds, and visuals.
- Timeline Workspace already loads provider configuration, owns per-step `ImageDocument`s, and renders the shared Linux/macOS annotation modal; extend these flows instead of adding a second modal.

## NOT in Scope

- Multi-step or batch callout generation: selected-step-only keeps image authorization, cost, and review bounded.
- More than one suggestion per run: numbering and partial acceptance are deferred until single-target quality is validated.
- Text Note or Opaque Redaction suggestions: redaction carries separate privacy claims and review requirements.
- Agent-selected bubble positions or numbers: Rollshot retains deterministic layout and document numbering.
- User-entered callout prompts: the first version infers intent from reviewed step metadata.
- URL, file-ID, video, PDF, or arbitrary media transport: the provider contract accepts one authorized PNG/JPEG only.
- Replacing Rollshot request/event/error contracts with Rig types: Rig remains internal transport/state machinery.
- Provider/model capability discovery UI: unsupported image input remains a recoverable error until provider metadata exists.
- Telemetry, batch-cost analytics, and hosted evaluation infrastructure: the phase uses contract tests and one privacy-safe smoke test.

---

## File Structure

- Modify `crates/rollshot-agent/src/domain.rs`: convert authorized attachments into redacted provider-neutral model attachments.
- Modify `crates/rollshot-agent/src/model.rs`: define `ModelAttachment` and add `attachments` to `ModelRequest`.
- Modify `crates/rollshot-agent/src/provider.rs`: convert attachments to Rig `UserContent::Image` for existing Anthropic/OpenAI adapters.
- Modify `crates/rollshot-agent/src/driver.rs`: preserve existing request literals, extract task profiles, and add the callout runner by reusing streamed-turn machinery.
- Create `crates/rollshot-agent/src/callout.rs`: bounded callout task profile, terminal schema, output validation, and Rig turn/tool lifecycle.
- Modify `crates/rollshot-agent/src/lib.rs`: export Rollshot-owned callout runner/output types.
- Create `crates/rollshot-action/src/callout_proposal.rs`: step-bound proposal, provenance, accept/reject/stale policy.
- Modify `crates/rollshot-action/src/lib.rs`: export callout proposal types.
- Create `crates/rollshot-image-document/src/callout_placement.rs`: deterministic bubble-placement function.
- Modify `crates/rollshot-image-document/src/lib.rs`: export the placement function.
- Create `crates/rollshot-app/src/timeline_workspace/callout_agent.rs`: encode the selected keyframe, authorize it, and run the bounded callout task.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`: callout run/proposal/modal state.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`: request, cancellation, load, accept, reject, close, and stale transitions.
- Modify `crates/rollshot-app/src/timeline_workspace/annotation.rs`: ghost-callout Canvas rendering.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`: selected-step action and modal loading/review controls.
- Modify `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`: keep the existing caption request explicitly text-only.

---

### Task 1: Provider-Neutral Authorized Image Attachments

**Files:**
- Modify: `crates/rollshot-agent/src/model.rs`
- Modify: `crates/rollshot-agent/src/domain.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`

**Interfaces:**
- Produces: `ModelAttachment`, `ModelRequest::attachments`, and consuming `AuthorizedModelInput::take_model_attachments()`.
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
    let mut input = AuthorizedModelInput::new(
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

    let attachments = input.take_model_attachments();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].media_type(), MediaType::Png);
    assert_eq!(attachments[0].bytes(), &[1, 2, 3, 4]);
    assert!(input.attachments().is_empty());
}
```

Add two authorization regression tests before implementation:

```rust
#[test]
fn rejects_declared_byte_count_that_does_not_match_payload() {
    let error = AuthorizedModelInput::new(
        "anthropic".into(), "m".into(), "p".into(),
        vec![AttachmentDescriptor {
            media_type: MediaType::Png,
            width: 1,
            height: 1,
            byte_count: 1,
        }],
        vec![vec![1, 2]],
    ).unwrap_err();
    assert_eq!(error, InputError::ByteCountMismatch { declared: 1, actual: 2 });
}

#[test]
fn rejects_zero_sized_attachment_dimensions() {
    let error = AuthorizedModelInput::new(
        "anthropic".into(), "m".into(), "p".into(),
        vec![AttachmentDescriptor {
            media_type: MediaType::Png,
            width: 0,
            height: 1,
            byte_count: 1,
        }],
        vec![vec![1]],
    ).unwrap_err();
    assert_eq!(error, InputError::InvalidDimensions { width: 0, height: 1 });
}
```

- [ ] **Step 5: Add the authorized conversion and run tests**

Add `InputError::ByteCountMismatch { declared, actual }` and `InputError::InvalidDimensions { width, height }`. In `AuthorizedModelInput::new`, compare each descriptor with its paired payload using checked `usize` to `u64` conversion, reject zero dimensions, and calculate limits from the actual verified lengths. This closes the existing trust gap where a caller could declare one byte while supplying a much larger buffer.

Add to `AuthorizedModelInput`:

```rust
pub(crate) fn take_model_attachments(&mut self) -> Vec<crate::model::ModelAttachment> {
    self.manifest.descriptors.iter().zip(std::mem::take(&mut self.attachments))
        .map(|(descriptor, bytes)| crate::model::ModelAttachment::new(
            descriptor.media_type,
            descriptor.width,
            descriptor.height,
            std::sync::Arc::from(bytes),
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
rtk git add crates/rollshot-agent/src/model.rs crates/rollshot-agent/src/domain.rs crates/rollshot-agent/src/driver.rs crates/rollshot-app/src/timeline_workspace/caption_agent.rs
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
    let last = request.chat_history.iter().last().expect("image message");
    let Message::User { content } = last else { panic!("last message must be user image") };
    assert!(matches!(
        content.iter().next().expect("image content"),
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

Assert an empty attachment list produces the exact previous chat history. Assert `format!("{:?}", image_request())` does not contain the raw byte sentinel or a base64 representation of it. Add this JPEG mapping test so both allowed media types are covered:

```rust
#[test]
fn jpeg_attachment_maps_to_rig_jpeg() {
    let attachment = crate::model::ModelAttachment::new(
        crate::domain::MediaType::Jpeg,
        1,
        1,
        std::sync::Arc::from([0xff_u8, 0xd8_u8]),
    );
    assert!(matches!(
        attachment_to_rig(&attachment),
        UserContent::Image(rig_core::message::Image {
            media_type: Some(rig_core::message::ImageMediaType::JPEG),
            ..
        })
    ));
}
```

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

### Task 3: Extract Bounded Agent Task Profile

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`

**Interfaces:**
- Consumes: the existing Smart Redaction system prompt and terminal-tool set.
- Produces: internal `AgentTaskProfile` lookups used by existing behavior and Task 4.

- [ ] **Step 1: Write failing profile parity tests**

Add tests asserting `SmartRedaction.system_prompt()` equals `SMART_REDACTION_SYSTEM_PROMPT`, its terminal tools are exactly `submit_for_review` and `request_user_input`, and `Callout` advertises only `submit_callout_suggestion`.

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `rtk cargo test -p rollshot-agent task_profile`

Expected: FAIL because `AgentTaskProfile` does not exist.

- [ ] **Step 3: Add the internal profile enum**

```rust
pub(crate) enum AgentTaskProfile {
    SmartRedaction,
    Callout,
}

impl AgentTaskProfile {
    pub(crate) fn system_prompt(&self) -> &'static str {
        match self {
            Self::SmartRedaction => SMART_REDACTION_SYSTEM_PROMPT,
            Self::Callout => CALLOUT_SYSTEM_PROMPT,
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

Define `CALLOUT_SYSTEM_PROMPT` beside the existing Smart Redaction prompt in `driver.rs`; Task 4 uses the profile accessor and does not duplicate the prompt.

Route the existing runner through `AgentTaskProfile::SmartRedaction`; do not change its public signature or terminal mapping.

- [ ] **Step 4: Run the complete existing agent suite**

Run: `rtk cargo test -p rollshot-agent`

Expected: PASS, proving the structural refactor preserves current behavior.

- [ ] **Step 5: Commit the behavior-preserving refactor**

```bash
rtk git add crates/rollshot-agent/src/driver.rs
rtk git commit -m "refactor(agent): extract bounded task profile"
```

---

### Task 4: Bounded Callout Agent Profile

**Files:**
- Create: `crates/rollshot-agent/src/callout.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Interfaces:**
- Consumes: `AuthorizedModelInput`, `ProviderAdapter`, `RunBudget`, `RunCancellation`, and Rig `AgentRun`.
- Produces: `AgentRunner::run_callout_with_provider(input, provider, budget, cancellation) -> CalloutRunTerminal`.

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

- [ ] **Step 5: Implement the callout runner**

Keep callout data/schema/decoding in `callout.rs`, but implement the loop as `AgentRunner::run_callout_with_provider` in `driver.rs` so it reuses the existing private streamed-turn assembly, budget charging, cancellation checks, and Rig tool-result threading. Do not copy `run_model_turn_with_provider` into the new module. Drive `rig_core::agent::run::AgentRun` with `max_turns(2)`, `AgentTaskProfile::Callout`, and exactly one tool definition. Own the input mutably, charge one attachment before the first model call, attach `input.take_model_attachments()` only to the first `ModelRequest`, and use an empty attachment list on any second turn. This moves the authorized PNG buffer into `Arc<[u8]>` without cloning it; Rig conversion performs the single transport copy required by `image_raw`.

Expose:

```rust
pub async fn run_callout_with_provider(
    &self,
    input: crate::domain::AuthorizedModelInput,
    provider: &dyn crate::ProviderAdapter,
    budget: crate::runtime::RunBudget,
    cancellation: &crate::runtime::RunCancellation,
) -> CalloutRunTerminal
```

Map detailed provider/protocol messages only to privacy-safe tracing events; terminal values carry no provider payload or prompt text.

Define and use this exact product budget rather than `RunBudget::unlimited()`:

```rust
pub fn callout_run_budget() -> crate::runtime::RunBudget {
    crate::runtime::RunBudget {
        wall_time: std::time::Duration::from_secs(30),
        model_calls: 2,
        input_tokens: 32_000,
        output_tokens: 1_000,
        cost: f64::MAX,
        tool_calls: 1,
        per_tool_calls: 1,
        argument_bytes: 4_096,
        result_bytes: 4_096,
        source_bytes: 0,
        attachments: 1,
        validation_attempts: 0,
        dry_run_attempts: 0,
        capability_calls: 0,
        candidate_count: 0,
        affected_area: 0,
    }
}
```

- [ ] **Step 6: Export types, run contract/privacy tests, and commit**

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

### Task 5: Step-Bound Callout Proposal Policy

**Files:**
- Create: `crates/rollshot-action/src/callout_proposal.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

**Interfaces:**
- Consumes: `GuideStep`, `CandidateId`, `FrameId`, `ImagePoint`, and agent draft output.
- Produces: `CalloutProposal`, `CalloutSuggestion`, `CalloutApplyOutcome`, and base-state matching.

- [ ] **Step 1: Write failing proposal construction tests**

Cover a valid in-bounds tip, non-finite tip, edge-exclusive bounds (`x == width` is invalid), invalid confidence, oversized rationale, trimmed rationale, and agent provenance.

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

Use these explicit construction failures:

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CalloutProposalError {
    #[error("callout tip must be finite")]
    NonFiniteTip,
    #[error("callout tip is outside the source image")]
    TipOutOfBounds,
    #[error("callout confidence must be finite and within 0..=1")]
    InvalidConfidence,
    #[error("callout rationale exceeds 500 characters")]
    RationaleTooLong,
}
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
rtk git add crates/rollshot-action/src/callout_proposal.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add callout proposal policy"
```

---

### Task 6: Deterministic Bubble Placement

**Files:**
- Create: `crates/rollshot-image-document/src/callout_placement.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`

**Interfaces:**
- Consumes: tip, image dimensions, and committed `Annotation` bounds.
- Produces: `place_number_callout_bubble(...) -> ImagePoint`.

- [ ] **Step 1: Write failing placement tests**

Cover center preference for upper-right, each image corner, overlap avoidance with an existing Number Callout, deterministic upper-right tie-breaking, and a tiny image that requires clamping. Tests must derive bubble radius from the existing Number Callout style token rather than introducing a second visual radius.

- [ ] **Step 2: Run tests and verify they fail**

Run: `rtk cargo test -p rollshot-image-document callout_placement`

Expected: FAIL because the module does not exist.

- [ ] **Step 3: Implement the pure placement function**

Expose:

```rust
pub fn place_number_callout_bubble(
    tip: ImagePoint,
    image_width: u32,
    image_height: u32,
    annotations: &[Annotation],
) -> ImagePoint
```

Keep placement constants private because this phase has one product behavior: offset `NUMBER_BUBBLE_RADIUS * 2.5`, bubble extent `NUMBER_BUBBLE_RADIUS + NUMBER_BUBBLE_OUTLINE_WIDTH`, and tip protection radius `NUMBER_BUBBLE_RADIUS`. Generate candidates in upper-right, upper-left, lower-right, lower-left order. Score axis-aligned bubble bounds against the protected tip square and existing `annotation_bounds`; choose minimum overlap with stable first-candidate tie-breaking. Clamp the upper-right candidate when none fit.

- [ ] **Step 4: Run crate tests and commit**

Run: `rtk cargo test -p rollshot-image-document`

Expected: PASS.

```bash
rtk git add crates/rollshot-image-document/src/callout_placement.rs crates/rollshot-image-document/src/lib.rs
rtk git commit -m "feat(document): place suggested callout bubbles"
```

---

### Task 7: Timeline Callout Agent Orchestration

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/callout_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: selected `GuideStep`, original retained keyframe, current annotation `state_id`, provider adapter/config, and Task 4 runner.
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
let image_width = input.image.width();
let image_height = input.image.height();
let mut png = Vec::new();
image::DynamicImage::ImageRgba8(input.image)
    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
    .map_err(|error| format!("PNG encode failed: {error}"))?;
```

Construct one `AuthorizedModelInput` descriptor, invoke `AgentRunner::new(AgentConfig { max_turns: 2, ..Default::default() }).run_callout_with_provider(...)` with `callout_run_budget()`, and map only `Suggested` into `CalloutProposal::from_agent_draft`. Map `NoSuggestion` separately; map cancelled/budget/provider/protocol terminals to fixed recoverable messages without forwarding sensitive provider text.

- [ ] **Step 4: Add workspace state and cancellation ownership**

Add one state enum instead of independent booleans/options that can represent contradictory states:

```rust
pub(crate) enum CalloutSuggestionState {
    Idle,
    Running {
        run_id: u64,
        cancellation: rollshot_agent::runtime::RunCancellation,
    },
    Pending(rollshot_action::CalloutProposal),
    NoSuggestion { reason: Option<String> },
    Failed { message: String },
}

pub(crate) callout_suggestion: CalloutSuggestionState,
pub(crate) callout_agent_run_id: u64,
```

Initialize them to `Idle` and `0`. Add an ASCII state-transition diagram as a doc comment above the enum (`Idle -> Running -> Pending/NoSuggestion/Failed -> Idle`) and these state tests in `update.rs` using the existing `ws` and `synthetic_recording` helpers:

```rust
#[test]
fn new_workspace_has_idle_callout_state() {
    let state = ws(synthetic_recording(1));
    assert!(matches!(state.callout_suggestion, CalloutSuggestionState::Idle));
    assert_eq!(state.callout_agent_run_id, 0);
}

#[test]
fn replacing_keyframe_discards_pending_callout() {
    let mut state = ws(synthetic_recording(1));
    state.callout_suggestion = CalloutSuggestionState::Pending(callout_proposal(&state));
    let replacement = state.strip.iter().map(|frame| frame.id)
        .find(|id| Some(*id) != state.selected_step().map(|step| step.keyframe))
        .expect("nearby replacement");
    let _ = update(&mut state, Message::ReplaceKeyframe(replacement));
    assert!(matches!(state.callout_suggestion, CalloutSuggestionState::Idle));
}
```

Define the test-only `callout_proposal(&TimelineWorkspace)` helper in Task 7 alongside the other proposal fixtures, using the selected step, its presentation document `state_id`, and image dimensions.

- [ ] **Step 5: Add request/load/cancel update messages**

Add:

```rust
SuggestCalloutRequested,
CancelCalloutSuggestion,
RejectCalloutSuggestion,
AcceptCalloutSuggestion,
```

Add this run-aware completion message:

```rust
CalloutSuggestionLoaded {
    run_id: u64,
    result: Result<super::callout_agent::CalloutTaskResult, String>,
},
```

`SuggestCalloutRequested` must ensure a selected step, create its presentation document, snapshot `state_id`, clone the original retained image, load/build the configured provider exactly as caption suggestions do, open the annotation session, store `Running { run_id, cancellation }`, and launch `Task::perform`. Capture `run_id` in the task mapper. The loaded arm must accept a result only when the current state is `Running` with the same `run_id`; this prevents a cancelled or timed-out older request from overwriting a newer proposal.

- [ ] **Step 6: Add focused update tests**

Cover missing selection, missing provider key, duplicate request suppression, successful proposal storage, no-suggestion message, cancellation cleanup, keyframe replacement cleanup, document edit staleness, reject with no mutation, accept with one undoable callout, and this late-result race:

```rust
#[test]
fn stale_callout_run_completion_cannot_replace_newer_run() {
    let mut state = ws(synthetic_recording(1));
    state.callout_agent_run_id = 2;
    state.callout_suggestion = CalloutSuggestionState::Running {
        run_id: 2,
        cancellation: rollshot_agent::runtime::RunCancellation::new(),
    };
    let old = Ok(super::callout_agent::CalloutTaskResult::Proposal(
        callout_proposal_with_run(&mut state, 1)
    ));

    let _ = update(&mut state, Message::CalloutSuggestionLoaded {
        run_id: 1,
        result: old,
    });

    assert!(matches!(
        state.callout_suggestion,
        CalloutSuggestionState::Running { run_id: 2, .. }
    ));
}
```

- [ ] **Step 7: Run update tests and commit**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update`

Expected: PASS.

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/callout_agent.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): run selected-step callout suggestions"
```

---

### Task 8: Ghost Preview and Modal Review Controls

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

Add `pub suggested: Option<Annotation>` to `NumberAnnotationCanvas`. Render committed annotations with existing colors, then render the suggestion with reduced alpha and a small `Suggested` label above the canvas. Do not invent dashed-path geometry and do not add the ghost to `ImageDocument`.

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

### Task 9: Cross-Crate Verification and Real-Provider Smoke Test

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

With an already configured vision-capable Anthropic or OpenAI model, open an Action Guide, select one step, request a callout, and verify the ghost appears without changing undo state. Accept it, open Storyboard preview and confirm the callout is present, close preview, undo the callout, reopen preview and confirm it is absent. Confirm the original keyframe remains unchanged throughout.

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

---

## Test Coverage Matrix

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / authorized image metadata, byte counts, limits, redacted Debug | yes | no | no | no |
| Task 2 / PNG/JPEG conversion into Rig requests; text-only regression | yes | provider request fixture | no | no |
| Task 3 / Smart Redaction profile parity after structural extraction | yes | full agent suite | no | no |
| Task 4 / bounded turns, terminal schema, no-suggestion, cancel, budgets | yes | scripted provider + Rig loop | no | no |
| Task 5 / proposal validation, provenance, status, restored `state_id` | yes | no | no | no |
| Task 6 / placement corners, overlap, ties, tiny images | yes | no | no | no |
| Task 7 / PNG authorization, provider setup, run identity, stale acceptance | yes | Timeline update flow | no | no |
| Task 8 / ghost purity, modal controls, accept/reject/close | yes | shared iced state flow | no | visual appearance |
| Task 9 / complete selected-step workflow with vision provider | prior suites | cross-crate suites | one real-provider smoke | provider/model matrix |

No automated test depends on network, real time, screen capture, or native GUI state. Scripted provider streams and synthetic recordings cover CI; the real-provider check is explicitly isolated to Task 9.

## Failure Modes

| New codepath | Realistic production failure | Test coverage | Planned handling | User-visible result |
|---|---|---|---|---|
| Authorized attachment creation | descriptor lies about byte count or dimensions | Task 1 Steps 4-5 | `InputError::ByteCountMismatch` / `InvalidDimensions` | recoverable suggestion failure |
| Rig image conversion | provider/model rejects image content | Task 2 request fixtures; Task 7 terminal mapping | `ModelError` maps to `CalloutRunTerminal::ProviderFailure` | retry/close error, never text guess |
| Bounded callout loop | model ends without terminal tool or sends malformed/multiple calls | Task 4 Steps 1-6 | `ProtocolFailure` | recoverable suggestion failure |
| Budget/cancellation | model stalls or user closes modal | Task 4 lifecycle tests; Task 7 update tests | wall-time/turn/token/attachment budget and one `RunCancellation` | cancelled or timeout message; no proposal |
| Async completion | old cancelled run finishes after a newer run | Task 7 Step 6 | compare loaded `run_id` with current run | old result silently ignored by design; current run remains visible |
| Proposal acceptance | step, keyframe, dimensions, or document state changed | Task 5 Steps 4-5; Task 7 Step 6 | mark `Stale`, never edit document | regenerate message |
| Bubble placement | target lies near edge or overlaps annotations | Task 6 Steps 1-4 | deterministic alternatives then clamp | valid in-bounds ghost/annotation |
| Ghost rendering | preview accidentally mutates document/history | Task 8 Steps 1-3 | construct temporary `Annotation` only | no hidden edit; Accept remains explicit |
| Provider credentials | configured provider has no resolvable key | Task 7 Step 6 | existing provider setup guard | clear configuration message |
| PNG encoding | selected frame cannot encode or exceeds authorized byte limit | Task 7 Steps 1-3 | `Result` error before provider call | recoverable suggestion failure |

No identified failure is both untested and silently unhandled.

## Performance and Resource Bounds

- This path handles one user-selected frame per explicit request, not a capture hot loop.
- PNG encoding runs inside the iced async task, not in the synchronous `update` arm; the UI enters `Running` before encoding begins.
- The retained frame clone is the one unavoidable full-RGBA ownership transfer into the `'static` task. Do not create another RGBA clone during encoding.
- `AuthorizedModelInput::take_model_attachments()` moves the encoded PNG into `Arc<[u8]>`; do not clone the PNG before provider conversion.
- Existing authorization caps encoded input at 10 MiB per attachment and one callout run caps attachments at one.
- The provider conversion makes one byte copy for Rig `image_raw`; no base64 string is retained by Rollshot after request construction.
- At most two model calls and one tool call can be in flight for a run, with a 30-second wall-time ceiling.
- Cancellation drops task-owned frame/PNG buffers when the provider future exits. The late-result `run_id` guard prevents state resurrection even if provider cancellation is not immediate.
- Bubble placement is O(number of committed annotations) over four candidates; no spatial index is warranted for the small per-step annotation graph.

## Task Dependencies and Execution Strategy

| Task | Modules touched | Depends on |
|---|---|---|
| 1: Authorized attachments | `rollshot-agent`, caption request literal in `rollshot-app` | — |
| 2: Rig image conversion | `rollshot-agent` | 1 |
| 3: Task-profile extraction | `rollshot-agent` | 1 (request literals must compile) |
| 4: Bounded callout runner | `rollshot-agent` | 2, 3 |
| 5: Proposal policy | `rollshot-action` | — |
| 6: Bubble placement | `rollshot-image-document` | — |
| 7: App orchestration | `rollshot-app` | 4, 5, 6 |
| 8: Ghost/modal UX | `rollshot-app` | 7 |
| 9: Verification | workspace | 8 |

Parallel-safe conceptual lanes:

- Lane A: Task 1 → Task 2 → Task 3 → Task 4, sequential because all modify `rollshot-agent`.
- Lane B: Task 5, independent in `rollshot-action`.
- Lane C: Task 6, independent in `rollshot-image-document`.
- Lane D: Task 7 → Task 8 → Task 9 after A, B, and C complete.

Repository rules prohibit worktrees unless explicitly requested, and concurrent commits in one shared checkout are unsafe. Execute lanes A, B, and C with fresh subagents but serialize their edit/commit phases on this branch; then execute Lane D sequentially. There are no root workspace-member changes. Lane B already depends on `rollshot-image-document` at the Cargo level but does not edit Lane C's files, so source conflicts are not expected.
