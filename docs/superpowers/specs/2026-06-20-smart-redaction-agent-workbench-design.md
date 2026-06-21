# Smart Redaction Agent Workbench Design

**Date:** 2026-06-20  
**Status:** Approved design  
**Initial product use case:** Smart Redaction Presets

## 1. Summary

Rollshot will build its own visual agent experience rather than delegate the
workflow to an external agent host.

The first product use case is Smart Redaction Presets: a user describes what
should be hidden, a bounded agent authors a reusable automation, Rollshot tests
it against the current image, and the user reviews both the automation change
and the proposed visual result.

The reusable platform underneath this feature consists of:

- An agent session with conversation, visual context, tool activity, and
  resumable runs.
- A bounded agent driver built on an LLM/provider abstraction.
- Typed visual-inspection tools.
- A structured edit-proposal protocol that never directly mutates the image
  document.
- Immutable automation revisions.
- A restricted JavaScript authoring format.
- Parser and validator stages that normalize accepted JavaScript into a
  Rollshot-owned Workflow IR.
- A replaceable sandbox executor for running validated automation.
- Two review surfaces: automation diff and visual-result diff.

JavaScript source is the canonical user- and agent-authored artifact. Workflow
IR is the semantic representation used for validation, summaries, graphing,
capability manifests, static cost checks, and semantic diff. The IR is not the
execution engine. A sandbox executor runs validated source.

The exact JavaScript parser and runtime are intentionally not fixed by this
design. `rquickjs` 0.12.x is an initial runtime candidate, but parser/runtime
selection requires a focused spike. The architecture must permit replacement
without changing the product, agent, revision, or review models.

## 2. Product Principles

### 2.1 Rollshot owns the complete experience

The product includes the agent session, visual context, automation authoring,
review, correction, revision history, and safe export flow.

MCP is not part of this product architecture. A future MCP interface, if ever
added, must not define the internal document or agent model.

### 2.2 Smart Redaction is the first automation use case

Smart Redaction is not an isolated AI feature. It exercises reusable visual
agent capabilities:

- Understanding an image, selected region, and selected annotations.
- Calling bounded OCR, layout, color, region, and template tools.
- Producing typed visual edits.
- Comparing proposed and current results.
- Asking the user for clarification.
- Revising an artifact in response to feedback.

Smart Redaction adds a reusable automation artifact on top of that foundation.
Ordinary agent-assisted annotation editing may later reuse the same session,
visual context, edit proposal, and review components without creating a saved
automation.

### 2.3 The agent proposes; the product applies

The agent cannot directly mutate `ImageDocument` or UI state. It may:

- Inspect bounded visual context.
- Replace a draft automation source.
- Request validation and dry-runs.
- Submit a valid draft for review.

Visual changes are represented as typed `EditProposal` operations. Rollshot
applies reviewed operations to `ImageDocument` as one semantic transaction.

### 2.4 Human review remains mandatory

Neither successful automation execution nor a high-confidence candidate set
claims that the screenshot is safe.

The user can inspect, delete, move, or resize proposed redactions before
applying them. Existing manual annotation and secure-sharing controls remain
available.

### 2.5 Corrections do not silently train the preset

Manual correction of proposed candidates affects only the current image.

The user must explicitly choose **Improve Preset** to send the correction
evidence into another bounded agent run. That run creates a new draft
automation revision and goes through automation and visual review again.

## 3. Scope

### 3.1 First-release scope

- Named Smart Redaction Presets.
- Agent sessions for creating and improving presets.
- Explicit provider, model, and payload disclosure before each upload.
- Full-screenshot and OCR/layout-only input modes.
- Optional selected-region and selected-annotation context.
- Bounded visual inspection tools.
- Restricted JavaScript source authored by the agent.
- Parser, subset validation, Workflow IR normalization, and semantic
  diagnostics.
- Replaceable sandbox executor and frozen versioned `rollshot` capability API.
- Automation validation and dry-run.
- Immutable automation revisions with a linear first-release UI.
- Automation source diff and Workflow IR semantic summary.
- Visual candidate diff with direct candidate editing.
- Explicit Improve Preset flow.
- Applying reviewed edits to `ImageDocument` as one transaction.
- Existing safe copy/save integration.
- Privacy-safe session persistence and resume.

### 3.2 Deferred scope

- A general-purpose visual agent IDE.
- Arbitrary JavaScript.
- Automatically executing presets on every capture.
- Silent or unattended safe export.
- A visual node canvas showing many automation and result versions.
- Revision merging.
- Multi-agent collaboration.
- Python, Lua, Node.js, browser APIs, DOM APIs, or arbitrary native bindings.
- Model training and model downloads.
- YOLO or another object detector. The architecture reserves a capability/tool
  extension point for it.
- Automation sharing or a marketplace.
- An MCP server.

## 4. High-Level Architecture

```text
Preset Workbench / Result Workspace
        |
        +-- AgentSession
        |     +-- conversation
        |     +-- selected visual context
        |     +-- AgentRun history
        |     +-- current draft revision
        |
        +-- Bounded Agent Driver
        |     +-- provider/model adapter
        |     +-- tool registry
        |     +-- budgets and cancellation
        |     +-- domain terminal states
        |
        +-- Visual Context and Tools
        |     +-- image/selection/annotation context
        |     +-- OCR and layout
        |     +-- color, edge, and region features
        |     +-- template matching
        |     +-- future object detection
        |
        +-- Automation Frontend
        |     +-- JavaScript parser
        |     +-- restricted-subset validator
        |     +-- Workflow IR normalizer
        |     +-- semantic diff and capability manifest
        |
        +-- Sandbox Executor
        |     +-- validated source
        |     +-- frozen rollshot API
        |     +-- resource limits
        |
        +-- EditProposal
              +-- candidate annotations/redactions
              +-- provenance and confidence
              +-- review decisions
              +-- one ImageDocument transaction
```

### 4.1 Dependency boundaries

- UI code depends on agent-session and domain-result APIs, not on a specific
  LLM provider.
- The bounded driver depends on a Rollshot-owned model facade. Rig may
  implement this facade.
- The automation frontend depends on parser traits and Rollshot-owned AST
  traversal/IR types, not on UI state.
- The sandbox executor implements a Rollshot-owned executor interface.
- Visual tools call Rust capabilities. Generated JavaScript receives no direct
  access to product internals.
- `rollshot-image-document` owns annotation validation and final mutation.

### 4.2 Candidate libraries

Rig 0.39.x is the initial candidate for:

- Provider abstraction.
- Multimodal messages.
- Tool schemas and tool-call protocol.
- The sans-I/O `AgentRun` state machine.

The product must not use an unrestricted high-level `agent.prompt()` call as
its control plane. Rollshot drives `AgentRun` and maps it to product-specific
budgets and terminal states.

The parser and sandbox runtime remain spike decisions. `rquickjs` 0.12.x is a
runtime candidate, not an architectural commitment. Its Rust 1.87 requirement
must be considered against Rollshot's current Rust 1.85 declared minimum.

## 5. Restricted JavaScript Automation

### 5.1 Representation pipeline

```text
JavaScript source
    |
    v
Parser AST
    |
    v
Restricted-subset validation
    |
    v
Normalized Workflow IR
    |                     \
    |                      \--> semantic diff, graph, cost, capabilities
    v
Validated source
    |
    v
Sandbox executor
    |
    v
Candidate output validation
```

The original JavaScript source remains canonical. The parser AST is transient
compiler input. Workflow IR is persisted with the automation revision so its
semantic meaning can be reviewed without reparsing during ordinary UI use.

The persisted revision also records the parser/IR schema version and
capability API version needed to detect incompatible upgrades.

### 5.2 First-release language subset

The subset should support readable detector composition without becoming a
general programming environment.

Expected allowed constructs:

- `const` declarations.
- Literals and object/array literals within configured size limits.
- Direct identifier references.
- Direct property access on known safe values.
- Boolean, comparison, and bounded arithmetic expressions.
- `if` statements and conditional expressions.
- Pure arrow functions used only by approved collection operators.
- Approved collection operators such as `map` and `filter`.
- Calls to statically known `rollshot.*` capabilities and approved pure
  helpers.
- A statically identifiable final return value containing candidates.

Expected rejected constructs:

- `var` and mutable global state.
- Dynamic property access.
- Computed capability or method names.
- `eval`, `Function`, proxies, reflection, or prototype mutation.
- Imports, modules, filesystem, network, process, timers, async, promises,
  workers, Node.js, or DOM APIs.
- Recursion.
- `while`, `do`, unbounded `for`, generators, or user-controlled iteration.
- Class declarations and construction.
- Exceptions as control flow.
- Closures that escape their approved collection call.

The exact allowlist is a spike output and becomes a versioned language
contract. Unsupported syntax must produce source-span diagnostics suitable for
both the user and the agent repair loop.

### 5.3 Workflow IR responsibilities

Workflow IR represents semantic operations such as:

- Invoke an OCR/layout/region/template capability.
- Apply a condition or confidence threshold.
- Transform or expand bounds.
- Limit or sort candidates.
- Combine detector outputs.
- Emit candidate redactions.

It supports:

- Capability manifests.
- Static maximum-step and detector-call analysis.
- Semantic summaries.
- Semantic diffs between automation revisions.
- Future visual workflow diagrams.
- Compatibility checks against the installed Rollshot capability API.

Workflow IR is not independently executed in the first release. This avoids
creating a second runtime with subtly different JavaScript semantics.

### 5.4 Sandbox executor interface

The executor accepts only:

- Validated source.
- The matching Workflow IR/capability manifest.
- A frozen versioned host API.
- Explicit per-run resource limits.
- Read-only run input.

It returns:

- Candidate output or a typed runtime failure.
- Capability call metrics.
- Resource-usage diagnostics safe for product display.

The executor implementation must be replaceable without changing persisted
presets or agent tool contracts.

## 6. Data and Version Model

### 6.1 Preset and automation revisions

```text
Preset
  id
  name
  original_intent
  active_revision_id
  created_at
  updated_at

AutomationRevision
  id
  preset_id
  parent_id
  source
  workflow_ir
  language_schema_version
  ir_schema_version
  capability_api_version
  provenance
  validation_summary
  created_at
```

`AutomationRevision` is immutable. Every accepted agent modification creates a
new revision. `Preset.active_revision_id` selects the revision used for normal
runs.

`parent_id` preserves branch relationships for a future visual version canvas.
The first-release UI may present revisions linearly and does not implement
merge semantics.

### 6.2 Sessions and runs

```text
AgentSession
  id
  preset_id | draft_preset_id
  privacy_safe_messages
  selected_context_descriptor
  run_ids
  current_draft_revision_id
  created_at
  updated_at

AgentRun
  id
  session_id
  user_turn_id
  provider_and_model
  input_mode
  tool_events
  budget_usage
  diagnostics
  terminal_state
  proposed_revision_id
  created_at
```

An `AgentSession` is the durable conversational workbench. Each user send
starts one bounded `AgentRun`.

A run may:

- Produce no revision.
- Request user input.
- End in an error.
- Produce a draft revision ready for review.

Only user acceptance makes a draft revision active.

### 6.3 Runs, proposals, and review

```text
AutomationRun
  id
  revision_id
  input_mode
  input_descriptor
  diagnostics
  status
  edit_proposal_id

EditProposal
  id
  base_document_state_id
  operations
  confidence_summary
  rationale_summary
  provenance

ReviewDecision
  proposal_id
  accepted_operations
  rejected_operation_ids
  modified_operations
  resulting_document_state_id
```

An automation revision may be run against many images. A run and its visual
proposal are evidence, not another automation revision.

### 6.4 Improve Preset evidence

When the user explicitly chooses Improve Preset, Rollshot creates an
improvement input containing:

- Parent automation revision.
- Original proposal operations.
- User-rejected operations.
- User-modified operations.
- User-added relevant redactions, when explicitly included.
- Optional explanatory text.
- Current input mode and non-sensitive context descriptors.
- Explicitly approved fixtures, if any.

This evidence starts another bounded run. It does not directly mutate the
preset.

### 6.5 Image document transactions

`ImageDocument` currently owns semantic annotation history. Agent proposals
must extend that model with an operation batch/transaction boundary.

Applying one reviewed proposal creates one undo entry even when it contains
many annotations. Undo restores the pre-agent document state in one action.

Automation revision history and image document undo/redo remain distinct.

## 7. Agent Session and Bounded Driver

### 7.1 Session experience

The left side of the Preset Workbench is an Agent Session, not a raw log. It
contains:

- User messages.
- Agent responses.
- Collapsible tool/activity events.
- Run status and checkpoints.
- Provider/model and budget summaries.
- Attached visual context descriptors.
- A prompt composer for revisions and clarification.

The current visual proposal and automation draft shown elsewhere in the
workbench correspond to a selected run or draft revision in the session.

### 7.2 First-release agent tools

The initial tool set is intentionally narrow:

- `inspect_context_summary`
- `inspect_ocr`
- `inspect_layout`
- `inspect_region_features`
- `replace_automation_source`
- `validate_automation`
- `dry_run_automation`
- `submit_for_review`

Inspection tools require bounded queries, regions, result counts, and payload
sizes. The model cannot request arbitrary raw internal state.

`replace_automation_source` replaces the complete draft source. Source
mutation is centralized so every draft has a complete parseable artifact and
can generate a coherent diff.

Future YOLO/object-detection tools join the registry as explicit capabilities
without changing the loop or allowing arbitrary model execution.

### 7.3 Budgets

Each run has hard ceilings for:

- Model calls.
- Wall-clock duration.
- Input/output tokens and estimated cost.
- Calls per inspection tool.
- Source bytes.
- AST nodes.
- Workflow IR steps.
- Validation attempts.
- Dry-run attempts.
- Detector/capability calls.
- Candidate output count and total candidate area.

Budget state belongs to Rollshot, not only to Rig's max-turn counter.

### 7.4 Terminal states

```text
ReadyForReview
NeedsUserInput
BudgetExhausted
ProviderFailure
AgentProtocolFailure
SourceValidationFailure
RuntimeFailure
UserCancelled
```

`ReadyForReview` requires:

- The agent explicitly submitted the draft.
- The latest source parsed successfully.
- The source passed subset validation.
- Workflow IR normalization succeeded.
- Static policy checks passed.
- A sandbox dry-run completed.
- Candidate output validation passed.

`NeedsUserInput` is used for materially ambiguous intent or required visual
selection. The agent should not silently choose a high-impact interpretation.

`BudgetExhausted` may retain the last valid draft candidate for review, but it
must clearly report that the agent did not reach its intended stopping point.
An invalid draft never becomes an automation revision.

Provider, source, runtime, and product failures remain separate so the UI can
offer the correct retry action.

### 7.5 Cancellation and resume

Cancellation stops:

- The in-flight provider request.
- Pending agent tools.
- Sandbox work.

Cancellation must not persist sensitive transient context merely to support
resume.

Session resume restores privacy-safe conversation, run summaries, accepted
automation revisions, and draft metadata. It does not silently restore or
re-upload expired screenshot/OCR attachments. The UI asks the user to reattach
required visual context.

## 8. Review and Diff Experience

### 8.1 Workbench layout

The first-release workbench has three primary areas:

1. **Agent Session**
   - Conversation.
   - Tool activity.
   - Run checkpoints.
   - Prompt composer and visual attachments.
2. **Visual Result Diff**
   - Screenshot canvas.
   - Proposed candidates.
   - Confidence and exclusion states.
   - Candidate editing.
   - Original and before/after views.
3. **Automation Review**
   - JavaScript source diff.
   - Workflow IR semantic summary.
   - Capability and static-cost changes.
   - Accept, ask agent to revise, or discard.

### 8.2 Visual proposal states

Proposed candidates must be visually distinct from already accepted
annotations.

Each candidate supports:

- Select.
- Move.
- Resize.
- Delete.
- Confidence inspection.
- Rationale/provenance inspection when available.

Low-confidence or applicability-warning candidates have explicit visual
treatment and are never silently omitted from the review summary.

### 8.3 Automation review

Automation review includes:

- Source-level diff.
- Semantic changes derived from Workflow IR.
- Capability additions/removals.
- Threshold, padding, region, limit, and applicability changes.
- Current-image candidate-count change.
- Fixture regression summary when fixtures exist.

Accepting an automation revision and applying its current-image candidates are
related but distinct actions. The UI must make their effects clear.

### 8.4 Ask agent to revise

The user may continue the same Agent Session with text and optional selected
visual context, for example:

- Exclude a profile button.
- Only redact matches inside the selected panel.
- Include the selected annotation style.

Sending this message starts another bounded run. A successful run produces a
new draft revision and refreshed automation and visual diffs.

### 8.5 Improve Preset

After reviewing and correcting a normal preset run, the user can explicitly
choose Improve Preset.

Before the new upload/model call, the UI shows:

- Provider and model.
- Payload mode.
- Whether the complete screenshot, OCR/layout, selection, annotations, and
  correction summary will be sent.

The resulting revision is reviewed exactly like initial creation.

### 8.6 Future version canvas

The data model already contains the nodes needed for a future large canvas:

- Automation revisions.
- Agent runs.
- Reviewed results.

A future canvas may project those records spatially and allow side-by-side
comparison. It must not require changes to the agent tool protocol,
automation-store model, or image-document transaction model.

The first release does not implement the canvas.

## 9. Security and Privacy

### 9.1 Upload boundary

Before every provider upload, Rollshot identifies:

- Provider.
- Model.
- Payload mode.
- Complete screenshot inclusion.
- OCR/layout inclusion.
- Selected-region inclusion.
- Selected-annotation inclusion.
- Improvement correction inclusion.

Full-screenshot and OCR/layout-only modes require distinct, explicit
disclosure. Session resume never implies consent to upload again.

### 9.2 Agent boundary

- Tools are allowlisted.
- Tool parameters are schema validated.
- Inspection output is bounded.
- Unknown or disallowed tools fail through explicit agent-protocol handling.
- Budgets are enforced outside the model.
- Tool output and model payloads are not written to ordinary diagnostics.

### 9.3 Automation boundary

The parser rejects unsupported syntax before execution. The validator enforces
AST, source-size, and Workflow IR limits.

The executor exposes no:

- Filesystem.
- Network.
- Process access.
- Imports or modules.
- Timers.
- Async runtime.
- Node.js APIs.
- DOM APIs.

Each run uses a fresh execution context with:

- Memory limit.
- Stack limit.
- Wall-clock or interrupt limit.
- Capability-call limit.
- Output-count and output-size limits.

The host API is frozen, versioned, read-only except for returning candidate
data, and implemented in Rust.

### 9.4 Output boundary

Rust rejects:

- Malformed candidates.
- Non-finite coordinates.
- Zero-area rectangles.
- Out-of-bounds rectangles unless a specific capability contract permits
  deterministic clamping.
- Excessive candidate counts.
- Excessive total redaction area.
- Invalid annotation operation types.

Output validation never changes the product claim: the user must still review
the image.

### 9.5 Retention

Persist by default:

- Preset metadata.
- Accepted automation source and Workflow IR.
- Revision metadata.
- Privacy-safe session messages and run summaries.

Do not persist by default:

- Complete screenshots sent to providers.
- Raw OCR text.
- Raw tool results.
- Provider request bodies.
- Unfiltered provider responses.
- Sensitive visual attachments.

Visual attachments use ephemeral handles with explicit expiration. A fixture is
persisted only when the user explicitly marks it non-sensitive and adds it to
the preset.

### 9.6 Diagnostics

Rig or another provider library must be wrapped or configured so prompts,
automation source, OCR text, tool arguments, tool results, and model responses
do not enter general Rollshot logs.

Rollshot-owned events use stable `rollshot::*` tracing targets and structured,
privacy-safe fields such as counts, durations, terminal states, provider name,
model name, and budget usage.

## 10. Failure Semantics

### 10.1 Provider failures

- Authentication.
- Rate limit.
- Service unavailable.
- Network timeout.
- Invalid provider response.
- Request cancellation.

These offer provider-specific retry or configuration actions.

### 10.2 Agent failures

- Budget exhausted.
- Unknown or disallowed tool request.
- Invalid tool arguments.
- Tool policy denial.
- User clarification required.

These retain the session and explain whether another run can continue.

### 10.3 Source failures

- JavaScript parse error.
- Unsupported syntax.
- Workflow IR normalization failure.
- Static-cost violation.
- Capability API mismatch.

These include source spans and actionable diagnostics. When budget remains,
they are returned to the agent repair loop. When budget is exhausted, they are
shown to the user without activating the draft.

### 10.4 Runtime failures

- Memory limit.
- Stack limit.
- Timeout/interruption.
- Capability call failure.
- Detector failure.
- Malformed output.

Runtime failures do not become generic text that the agent may ignore. The
Rollshot driver decides whether they are repairable, terminal, or require user
input.

### 10.5 Product outcomes

- Candidates found.
- No confident match.
- Applicability mismatch.
- User discarded proposal.
- User accepted corrected proposal.

None of these states claims complete sensitive-data detection.

## 11. Verification

### 11.1 Pure unit tests

- Restricted JavaScript allowlist and denylist.
- Source-span diagnostics.
- AST-to-Workflow-IR normalization.
- IR schema/version compatibility.
- Semantic diff.
- Capability manifest.
- Static cost and budget calculation.
- Rectangle and edit-operation validation.
- Automation revision transitions.
- Session/run terminal-state transitions.
- `ImageDocument` proposal transaction and one-step undo.

### 11.2 Adversarial executor tests

- Sandbox escape attempts.
- Infinite or excessive work.
- Deep stack use.
- Large allocation.
- Dynamic property tricks.
- Prototype mutation.
- Hidden capability invocation.
- Candidate output amplification.
- Host callback failure.
- Cancellation during execution.

### 11.3 Agent driver tests

Use a fake completion model to drive deterministic tool-call sequences:

- Initial successful generation.
- Parse failure followed by repair.
- Runtime failure followed by repair.
- Unknown tool.
- Invalid tool arguments.
- Needs user input.
- Provider failure.
- Cancellation.
- Every budget ceiling.
- Ready-for-review requirements.
- Resume without expired visual attachments.

### 11.4 Fixture tests

- Expected candidates on known screenshots.
- Layout variations.
- No-match images.
- Applicability mismatch.
- Revision regression against approved non-sensitive fixtures.
- Improve Preset correction behavior.

### 11.5 Integration tests

- Agent draft to automation review.
- Visual proposal editing.
- Apply reviewed operations as one document transaction.
- Undo the complete agent transaction.
- Flatten reviewed redactions.
- Existing secure copy/save behavior.
- Session continuation after asking the agent to revise.

### 11.6 Provider contract tests

- Vision image payload.
- OCR/layout-only payload.
- Tool-call normalization.
- Unknown tool behavior.
- Partial and malformed provider responses.
- Usage accounting.
- Cancellation.

Recorded or fake provider fixtures should form the normal CI path. Live API
tests are optional/manual.

### 11.7 Manual verification

Because the feature changes Result Workspace behavior, verify both active
platform paths:

- Linux iced Result Workspace.
- macOS iced Result Workspace.

Verify:

- Upload disclosure.
- Visual selection attachment.
- Session conversation and tool events.
- Automation and visual diffs.
- Candidate editing.
- Improve Preset.
- Cancellation.
- Session resume.
- Safe copy/save.
- Tall stitched-image performance.

## 12. Delivery Decomposition

This architecture is intentionally broader than one implementation plan. It
must be delivered as separately reviewed subprojects with stable interfaces
between them.

Recommended sequence:

1. **Technical spikes**
   - JavaScript frontend.
   - Sandbox executor.
   - Rig integration.
   - Visual diff performance.
2. **Visual edit proposal foundation**
   - Typed annotation operations.
   - Batch transaction and one-step undo in `ImageDocument`.
   - Proposal validation and candidate overlay model.
3. **Automation frontend and runtime**
   - Restricted language contract.
   - Source diagnostics.
   - Workflow IR and semantic diff.
   - Replaceable executor interface and selected runtime.
   - **Implemented:** `docs/superpowers/handoffs/2026-06-21-automation-frontend-runtime.md`
4. **Bounded agent core**
   - **Next phase after subproject 3.**
   - Rollshot model facade.
   - Agent session/run domain model.
   - Tool registry, budgets, cancellation, and terminal states.
5. **Preset persistence**
   - Preset and immutable automation revisions.
   - Active revision selection.
   - Privacy-safe session/run persistence.
6. **Preset Workbench**
   - Agent Session UI.
   - Automation review.
   - Visual result review.
   - Upload disclosure and provider controls.
7. **Improve Preset and regression**
   - Correction evidence.
   - Repair runs.
   - Optional non-sensitive fixtures and regression summaries.
8. **Product integration**
   - Result Workspace handoff.
   - Secure copy/save.
   - Linux and macOS runtime verification.

Each item after the spikes receives its own implementation spec or plan. The
first implementation plan created from this document should cover the
technical spikes, not the complete product.

## 13. Required Technical Spikes

Implementation planning must not lock parser/runtime details before these
spikes.

### 13.1 JavaScript frontend spike

Compare parser candidates for:

- AST traversal ergonomics.
- Accurate source spans and diagnostics.
- Restricted-subset validation.
- Workflow IR normalization.
- Parser binary/compile cost.
- License.
- Maintenance activity.
- Rollshot MSRV impact.

### 13.2 Sandbox executor spike

Evaluate `rquickjs` 0.12.x and credible alternatives for:

- Fresh-context cost.
- Memory, stack, and time interruption.
- Frozen host API behavior.
- Disabling imports, timers, async, and ambient globals.
- Host callback safety.
- Cancellation.
- Binary footprint.
- Cross-platform build.
- Rust 1.87 requirement and workspace MSRV decision.

The outcome may choose a runtime other than QuickJS without changing this
design.

### 13.3 Rig integration spike

Verify Rig 0.39.x for:

- Multimodal messages plus tools.
- Manual `AgentRun` driving.
- Provider-specific structured tool behavior.
- Cancellation.
- Tool and model usage accounting.
- Privacy-safe tracing.
- Runtime provider selection behind a Rollshot-owned facade.

### 13.4 Visual diff spike

Prototype:

- Candidate overlays on ordinary and tall stitched screenshots.
- Original/before-after interaction.
- Automation source diff.
- Workflow IR semantic summary.
- Rendering and interaction latency with many candidates.

## 14. Success Criteria

The design is successfully implemented when a user can:

1. Start a Smart Redaction Preset session.
2. See exactly what will be sent to which provider.
3. Ask Rollshot to hide a natural-language target.
4. Watch a bounded agent inspect, author, validate, and dry-run an automation.
5. Continue the same session with clarifications and selected visual context.
6. Review JavaScript and semantic automation changes.
7. Review and edit proposed redaction candidates.
8. Accept an immutable automation revision.
9. Apply the reviewed candidates as one undoable image-document transaction.
10. Run the preset again without an LLM call.
11. Correct a later result without silently changing the preset.
12. Explicitly Improve Preset and review the resulting new revision.
13. Complete safe copy/save without Rollshot claiming the image is fully safe.
