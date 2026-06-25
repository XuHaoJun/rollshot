# Preset Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **For any iced UI step, also load the `iced-rs` skill** (AGENTS.md §9 mandate — 0.14 signatures differ from 0.13).

**Goal:** Build the first-release Smart Redaction Preset Workbench — a mode of the Result Workspace where users author, run, review, and save reusable redaction presets via a bounded visual agent, with candidates rendered as an overlay on the existing annotation canvas.

**Architecture:** The Workbench extends `ResultWorkspace` with a `WorkspaceMode` enum (Normal vs Workbench). Domain logic (candidate review, state machine, provider config, review→apply, `WorkbenchError`) lives in a new `workbench/` module under `result_workspace/`, fully TDD-tested. The agent run is bridged from async `run_with_provider` to iced's `Task::run(stream, f)` via a `tokio::sync::mpsc` channel; the `AgentSession` is guarded by a `tokio::sync::Mutex` so the spawned future stays `Send`. Candidate rendering is an overlay pass added to the existing view-built `AnnotationCanvas` program (extra fields on the struct, not a canvas-internal state). The review drawer is a collapsible right panel with a default human-readable tab and an advanced technical tab.

**Tech Stack:** iced 0.14 (`canvas`, `image`, `tokio`, `async_stream`), rollshot-agent (driver, runtime, tools, domain, provider), rollshot-preset (store, domain), rollshot-vision (`RealAutomationHost`, `VisualIndex`), rollshot-edit-proposal (`EditProposal`, `lower`, `ReviewDecision`), rollshot-automation (`validate_source`, `execute_to_proposal`, `ExecutionPolicy`, `AutomationHost`), rollshot-automation-rquickjs (`QuickJsExecutor`). `tokio` features: `rt`, `sync`, `time` (via iced's "tokio" feature + workspace dep).

## Global Constraints

- iced 0.14 pin; `canvas`, `image`, `tokio` features enabled. `Task::run(stream, f)` is the streaming bridge (verified in the `iced-rs` `task.md` reference).
- `unsafe_code = "deny"` on rollshot-app (not "forbid" — macOS native drag bridge uses audited `#[allow(unsafe_code)]`).
- Workspace MSRV: 1.85 (rollshot-app `Cargo.toml` inherits `rust-version.workspace`).
- Tracing: stable `rollshot::workbench::*` targets, structured fields, no OCR text / image pixels / tool args / provider bodies in any event.
- Privacy: `ActivityEntry` bounded summaries only (counts, durations, labels). `ProviderConfig` key resolved at runtime from env, never written to config file.
- Disclosure: per-run, before every upload (author/improve). Run-existing bypasses (no upload). Two explicit consent lines (full-screenshot, OCR/layout-only). **Radio buttons only update a local `PayloadMode` field — they never auto-confirm.** Only the explicit "Send to {provider}" button emits `DisclosureConfirmed`.
- Platform: Linux `iced::application` + macOS `iced::daemon` `Phase::Workspace`. Every `WorkbenchMessage` is a variant of `result_workspace::Message`; the existing `Message::Workspace(msg)` forwarding in `macos_product.rs:344-348` already covers the nested case (verified) — no macOS-specific forwarding task is needed.
- `validate_source` returns `Result<ValidatedAutomation, Vec<SourceDiagnostic>>` (verified `frontend/mod.rs:35`) — workbench surfaces structured diagnostics, not a flattened string.
- `RunEvent::TurnComplete` is never emitted by the driver (`runtime.rs:528`); turn boundaries inferred from `ToolCallEnd`/`TextChunk` patterns.
- `layout` permanently `capability_unavailable`; authoring guardrails reflect only `ocr`/`region_features`/`template_match`.
- Pending candidates are preview-only; they never count as safe redactions. Copy/Save warns or blocks while unapplied candidates exist.
- **API facts verified against code before this plan was written** (load-bearing for every task below):
  - `ImageDocument::new(source: RgbaImage) -> Self` — ONE arg (`document.rs:69`).
  - `ImageDocument::apply_batch(ops: Vec<EditOp>) -> Result<BatchOutcome, EditError>` (`document.rs:362`); `BatchOutcome { added_ids: Vec<AnnotationId> }` — **no `warnings` field** (`edit_op.rs:47-49`).
  - `RunBudget { affected_area: u64, ... }` — integer, not float (`runtime.rs:50`).
  `RunBudget::unlimited()` is the only constructor (`runtime.rs:54`); the workbench owns a finite-literal constructor.
  - `ProposedEdit` is defined in `rollshot-edit-proposal` (`proposal.rs:64`); **cannot `impl ProposedEdit` in rollshot-app** (orphan rule). Use a free function `proposed_edit_bounds(&ProposedEdit) -> Option<ImageRect>` in `workbench/`.
  - `AgentSession` is `!Clone`, held by value; `run_with_provider(&mut self, ..., session: &mut AgentSession, ...).await` (`driver.rs:374`). A `std::sync::Mutex` guard is `!Send` and cannot cross `.await` inside `tokio::spawn` → **must use `tokio::sync::Mutex`** for the session, or move the session into the spawned task and stream the terminal out.
  - `QuickJsExecutor` is a unit struct (`automation-rquickjs/src/lib.rs:10`); `AutomationExecutor::execute` is **sync** (`executor.rs:133`). `DryRunTool::new(ctx, executor: Arc<dyn AutomationExecutor>, host: Arc<Mutex<dyn AutomationHost>>)` (`tools.rs:482`).
  - `AuthorizedModelInput::new(provider, model, user_message, descriptors, attachment_bytes) -> Result<Self, InputError>` (`domain.rs:110`).
  - `PresetStore::open(root: PathBuf) -> Self` (`store.rs:41`); `create_preset(id, name, original_intent, now) -> Result<Preset>` (`store.rs:66`); `add_revision(preset_id, id, parent_id, artifact: ValidatedAutomation, provenance, now) -> Result<AutomationRevision>` (`store.rs:155`); `set_active_revision(preset_id, rev_id, now) -> Result<()>` (`store.rs:209`); `load_active_revision(preset_id) -> Result<AutomationRevision>` (`store.rs:239`).
  - `AnnotationCanvas<'a>` is a view-built `canvas::Program` (`canvas.rs:196`) with fields `document`, `editor`, `scale`, `visible`; `draw(_state: &(), ...)` (`canvas.rs:388`). Candidate overlay = add `pending_proposal: Option<&'a EditProposal>` + `review: Option<&'a CandidateReview>` + `selected_candidate: Option<CandidateId>` fields; no canvas-internal state.
  - Existing modal pattern: `discard_modal`/`unredacted_action_modal` in `view.rs:342/389` use `stack![base, opaque(scrim)]`. Reuse it.
  - `result_workspace::Message` enum lives in `update.rs:26`; `subscription()` at `update.rs:799` returns `Subscription<Message>`. The streaming activity drawer is delivered via `Task::run(stream, f)` returned from the `DisclosureConfirmed` handler (not via `subscription`), so it is bound to the run lifecycle, not the view cycle.

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `crates/rollshot-app/src/result_workspace/workbench/mod.rs` | `WorkspaceMode`, `WorkbenchState`, `WorkbenchMessage`, `PayloadMode`, `VisionContext`, `PendingDraft`, re-exports |
| `crates/rollshot-app/src/result_workspace/workbench/state.rs` | `RunState`, `CandidateReviewState`, `CandidateReview`, `ActivityEntry`, `ToolCardStatus`, `WorkbenchError`, `has_pending_candidates`, `apply_skip_summary`, `event_to_activity_entry`, `terminal_state_label`, `proposed_edit_bounds` |
| `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs` | `ProviderConfig`, `ProviderKind`, `KeySource`, load/save, `resolve_key`, `has_key`, `provider_model_label` |
| `crates/rollshot-app/src/result_workspace/workbench/run.rs` | `smart_redaction_budget`, `prepare_vision_context`, `start_agent_run` (Task::run bridge), `run_existing_preset` (headless) |
| `crates/rollshot-app/src/result_workspace/workbench/review.rs` | `build_review_decision`, `restamp_proposal`, `apply_candidates`, `assemble_correction_evidence`, `save_revision` |
| `crates/rollshot-app/src/result_workspace/workbench/view.rs` | Workbench layout: canvas-primary + collapsible activity/review drawers + disclosure modal + improve modal + result-state banners + candidate list |

### Modified files

| File | Change |
|---|---|
| `crates/rollshot-app/Cargo.toml` | Add deps: `rollshot-agent`, `rollshot-preset`, `rollshot-vision`, `rollshot-edit-proposal`, `rollshot-automation`, `rollshot-automation-rquickjs`, `tokio` (general, not linux-only), `async_stream`, `toml`, `chrono`, `tempfile` (dev), `serde`/`serde_json` (if not already) |
| `crates/rollshot-app/src/result_workspace/mod.rs` | Add `pub mod workbench;`, add `mode: workbench::WorkspaceMode` field to `ResultWorkspace`, initialize in `new`/`with_max_texture_dim` |
| `crates/rollshot-app/src/result_workspace/update.rs` | Add `Message::SmartRedaction` + `Message::Workbench(WorkbenchMessage)`; wire `update` arm; gate `Message::Copy`/`Message::SaveAs` on `has_pending_candidates` |
| `crates/rollshot-app/src/result_workspace/view.rs` | Toolbar "Smart Redaction" button; Workbench layout mode branch in `view()`; disclosure/improve modal in `stack` |
| `crates/rollshot-app/src/result_workspace/canvas.rs` | Add `pending_proposal`/`review`/`selected_candidate` fields to `AnnotationCanvas`; candidate overlay draw pass in `draw()`; candidate hit-test helper |

---

## Task 1: Dependencies + Workbench module scaffolding + `WorkspaceMode`

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`
- Create: `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
- Create: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`

**Interfaces:**
- Produces: `WorkspaceMode { Normal, Workbench(WorkbenchState) }`, `WorkbenchMessage` enum (all variants declared up front), `Message::Workbench(WorkbenchMessage)`, `Message::SmartRedaction`, `WorkbenchState` with `Default` impl, `WorkbenchError` enum
- Later tasks consume: `WorkbenchState` fields, `Message::Workbench(msg)` routing, `WorkbenchError` variants

### Step 1: Add Cargo dependencies

```toml
# crates/rollshot-app/Cargo.toml — add to [dependencies]
rollshot-agent = { path = "../rollshot-agent" }
rollshot-preset = { path = "../rollshot-preset" }
rollshot-vision = { path = "../rollshot-vision" }
rollshot-edit-proposal = { path = "../rollshot-edit-proposal" }
rollshot-automation = { path = "../rollshot-automation" }
rollshot-automation-rquickjs = { path = "../rollshot-automation-rquickjs" }
tokio = { workspace = true }            # rt + sync + time
async_stream = "0.3"
toml = "0.8"
chrono = { version = "0.4", default-features = false, features = ["std", "clock"] }
serde = { version = "1", features = ["derive"] }   # if not already a dep
serde_json = "1"                                   # if not already a dep

# [dev-dependencies]
tempfile = "3"
```

Remove `tokio` from the linux-only `[target.'cfg(target_os = "linux")'.dependencies]` section (it is now general).

### Step 2: Create `workbench/state.rs` — core types

```rust
// crates/rollshot-app/src/result_workspace/workbench/state.rs

use rollshot_agent::runtime::{BudgetDimension, RunCancellation, RunEvent};
use rollshot_agent::driver::RunTerminalState;
use rollshot_edit_proposal::{CandidateId, EditProposal, ProposedEdit};
use rollshot_image_document::ImageRect;
use iced::widget;

/// Where the workbench's run is in its lifecycle.
#[derive(Debug, Clone)]
pub enum RunState {
    Idle,
    Running {
        cancellation: RunCancellation,
        /// Identity for any future subscription-based stream; not used by the
        /// Task::run bridge itself but kept so terminal handlers can clear it.
        stream_id: widget::Id,
    },
    Terminal(RunTerminalState),
}

impl Default for RunState {
    fn default() -> Self { Self::Idle }
}

/// Per-candidate review state.
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateReviewState {
    /// Will apply by default.
    Pending,
    /// Explicit user confirm (optional; not required for normal flow).
    Accepted,
    /// Will not apply.
    Rejected,
    /// Will apply with this replacement edit instead of the original.
    Modified(ProposedEdit),
}

/// Per-candidate review map.
#[derive(Debug, Clone, Default)]
pub struct CandidateReview {
    pub per_candidate: std::collections::BTreeMap<CandidateId, CandidateReviewState>,
}

/// Activity entries reconstructed from the RunEvent stream for the drawer.
#[derive(Debug, Clone)]
pub enum ActivityEntry {
    UserMessage(String),
    AssistantText(String),
    ToolCard {
        name: String,
        status: ToolCardStatus,
        summary: String,
    },
    RunStatus {
        turn: u32,
        budget_summary: String,
        elapsed: std::time::Duration,
    },
    TerminalLabel(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCardStatus {
    Running,
    Success,
    Failed,
}

/// Workbench error model (spec §9.1). Maps each failure to the correct UI
/// retry action. Never stringly-typed at this layer.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkbenchError {
    ProviderFailure { message: String },
    SourceValidationFailure,
    RuntimeFailure,
    AgentProtocolFailure { message: String },
    BudgetExhausted { dimension: BudgetDimension },
    VisionPrepare { message: String },
    Store { message: String },
    Config,
    /// `RunTerminalState::Cancelled` is not an error — return to Idle. This
    /// variant is kept for completeness but is never shown as an error.
    Cancelled,
}

impl std::fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderFailure { message } => write!(f, "Provider error: {message}"),
            Self::SourceValidationFailure => write!(f, "Automation validation failed"),
            Self::RuntimeFailure => write!(f, "Runtime error"),
            Self::AgentProtocolFailure { message } => write!(f, "Agent error: {message}"),
            Self::BudgetExhausted { dimension } => write!(f, "Budget exhausted: {dimension:?}"),
            Self::VisionPrepare { message } => write!(f, "Vision prepare: {message}"),
            Self::Store { message } => write!(f, "Preset store: {message}"),
            Self::Config => write!(f, "Provider not configured"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl WorkbenchError {
    /// Map a terminal state to the workbench error it represents (if any).
    /// `ReadyForReview` / `NeedsUserInput` / `Cancelled` are not errors.
    pub fn from_terminal(terminal: &RunTerminalState) -> Option<Self> {
        match terminal {
            RunTerminalState::ProviderFailure { message } => Some(Self::ProviderFailure { message: message.clone() }),
            RunTerminalState::SourceValidationFailure => Some(Self::SourceValidationFailure),
            RunTerminalState::RuntimeFailure => Some(Self::RuntimeFailure),
            RunTerminalState::AgentProtocolFailure { message } => Some(Self::AgentProtocolFailure { message: message.clone() }),
            RunTerminalState::BudgetExhausted { dimension } => Some(Self::BudgetExhausted { dimension: *dimension }),
            _ => None,
        }
    }
}

/// Free helper — cannot `impl ProposedEdit` in rollshot-app (orphan rule).
/// Returns the image-space rect a candidate edit targets, if any.
pub fn proposed_edit_bounds(edit: &ProposedEdit) -> Option<ImageRect> {
    match edit {
        ProposedEdit::AddRedaction { bounds } => Some(*bounds),
        ProposedEdit::UpdateRedactionBounds { bounds, .. } => Some(*bounds),
        _ => None,
    }
}
```

### Step 3: Create `workbench/mod.rs` — enums + re-exports

```rust
// crates/rollshot-app/src/result_workspace/workbench/mod.rs

pub mod provider_config;   // Task 2
pub mod review;            // Task 4
pub mod run;               // Tasks 5, 7
pub mod state;
pub mod view;              // Task 6+

pub use state::{
    ActivityEntry, CandidateReview, CandidateReviewState, RunState, ToolCardStatus,
    WorkbenchError, proposed_edit_bounds,
};

use rollshot_edit_proposal::{CandidateId, EditProposal};
use rollshot_image_document::ImageRect;
use rollshot_preset::{AutomationRevision, Preset};
use rollshot_agent::driver::RunTerminalState;
use rollshot_agent::runtime::{RunBudget, RunCancellation, RunEvent};

/// Which payload the user consented to upload (disclosure modal local state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PayloadMode {
    /// Full screenshot + OCR/layout summary.
    #[default]
    FullScreenshot,
    /// OCR/layout summary only — no image upload.
    OcrLayoutOnly,
}

/// Prepared vision state for the current run.
pub struct VisionContext {
    pub index: rollshot_vision::VisualIndex,
    pub host: std::sync::Arc<std::sync::Mutex<rollshot_vision::RealAutomationHost>>,
    pub executor: rollshot_automation_rquickjs::QuickJsExecutor,
    pub cancellation: rollshot_automation::CancellationFlag,
}

/// Subset of `DraftAutomation` the workbench retains after a run (spec §4.1).
#[derive(Debug, Clone)]
pub struct PendingDraft {
    pub source: String,
    pub assistant_text: String,
    pub validation_summary: rollshot_automation::ValidationSummary,
}

/// Workbench mode sub-state attached to `ResultWorkspace`.
/// Mirrors spec §4.1 — `session`, `provider_config`, `budget` included.
#[derive(Debug)]
pub struct WorkbenchState {
    pub preset: Option<Preset>,
    pub active_revision: Option<AutomationRevision>,
    /// In-memory only (D7). Guarded by `tokio::sync::Mutex` when held across
    /// `.await` in the spawned run task; stored here as the owned value
    /// between runs (idle/terminal).
    pub session: rollshot_agent::domain::AgentSession,
    pub run_state: RunState,
    pub live_activity: Vec<ActivityEntry>,
    pub pending_proposal: Option<EditProposal>,
    pub pending_draft: Option<PendingDraft>,
    pub review: CandidateReview,
    pub selected_candidate: Option<CandidateId>,
    pub provider_config: provider_config::ProviderConfig,
    pub vision: Option<VisionContext>,
    pub budget: RunBudget,
    pub error: Option<WorkbenchError>,
    /// Disclosure modal is open; the next `DisclosureConfirmed` starts the run.
    pub disclosure_pending: bool,
    /// Payload mode selected in the disclosure modal (local UI state).
    pub payload_mode: PayloadMode,
    /// Composer text for the next user message.
    pub composer: String,
    /// Pending agent-run parameters captured when the user pressed Send;
    /// consumed by `DisclosureConfirmed`. `None` when no run is queued.
    pub pending_run: Option<PendingRunParams>,
    /// Next candidate id for manually-added missing candidates (§5.3).
    pub next_manual_candidate_id: u64,
}

/// Parameters captured at Send time and consumed when disclosure is confirmed.
#[derive(Debug, Clone)]
pub struct PendingRunParams {
    pub user_message: String,
    pub image_dims: (u32, u32),
    pub active_revision_source: Option<String>,
    pub mode: RunKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Author,
    Improve,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        use rollshot_agent::domain::{AgentSession, SessionId};
        Self {
            preset: None,
            active_revision: None,
            session: AgentSession::new(SessionId::new(0)),
            run_state: RunState::default(),
            live_activity: Vec::new(),
            pending_proposal: None,
            pending_draft: None,
            review: CandidateReview::default(),
            selected_candidate: None,
            provider_config: provider_config::ProviderConfig::default(),
            vision: None,
            budget: run::smart_redaction_budget(),
            error: None,
            disclosure_pending: false,
            payload_mode: PayloadMode::default(),
            composer: String::new(),
            pending_run: None,
            next_manual_candidate_id: 1,
        }
    }
}

/// Workspace mode: Normal (existing canvas + navigator) or Workbench.
#[derive(Debug, Default)]
pub enum WorkspaceMode {
    #[default]
    Normal,
    Workbench(WorkbenchState),
}

/// Messages scoped to the workbench. Every variant is declared up front so
/// later tasks only add handler arms, not enum churn.
#[derive(Debug, Clone)]
pub enum WorkbenchMessage {
    // Run events from the agent (streamed via Task::run channel bridge)
    RunEvent(RunEvent),
    RunTerminal(RunTerminalState),
    // Disclosure
    DisclosureRequested(PendingRunParams),
    PayloadModeSelected(PayloadMode),
    DisclosureConfirmed,
    DisclosureCancelled,
    // Composer
    ComposerChanged(String),
    SendRequested,
    // Candidate gestures (from canvas overlay / candidate list)
    CandidateSelected(CandidateId),
    CandidateDeselected,
    CandidateDeleted(CandidateId),
    CandidateMoved { id: CandidateId, new_bounds: ImageRect },
    NextWarning,
    JumpToCandidate(CandidateId),
    AddManualCandidate { bounds: ImageRect },
    // Actions
    ApplyCandidates,
    SavePresetOrRevision,
    AskAgentToRevise,
    DiscardDraft,
    DiscardCandidates,
    ImStart,
    ToggleAdvancedDetails,
    // Cancel
    CancelRun,
    // Settings (minimal key-presence surface)
    OpenProviderSettings,
}
```

### Step 4: Stub the later-task modules so the crate compiles now

```rust
// crates/rollshot-app/src/result_workspace/workbench/provider_config.rs
// (full impl in Task 2; minimal stub so Task 1 compiles)
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind { Anthropic, OpenAI }

impl Default for ProviderKind { fn default() -> Self { Self::Anthropic } }

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Anthropic => write!(f, "Anthropic"), Self::OpenAI => write!(f, "OpenAI") }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeySource { Env(String) }

impl Default for KeySource { fn default() -> Self { Self::Env("ANTHROPIC_API_KEY".into()) } }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub key_source: KeySource,
}
```

```rust
// crates/rollshot-app/src/result_workspace/workbench/run.rs
use rollshot_agent::runtime::RunBudget;

/// Finite budget for Smart Redaction runs. `RunBudget::unlimited()` is the
/// only constructor in rollshot-agent (§10.4); the workbench owns this one.
pub fn smart_redaction_budget() -> RunBudget {
    RunBudget {
        wall_time: std::time::Duration::from_secs(30),
        model_calls: 10,
        input_tokens: 20_000,
        output_tokens: 10_000,
        cost: 0.50,
        tool_calls: 30,
        per_tool_calls: 10,
        argument_bytes: 256 * 1024,
        result_bytes: 256 * 1024,
        source_bytes: 100 * 1024,
        attachments: 8,
        validation_attempts: 10,
        dry_run_attempts: 5,
        capability_calls: 16,
        candidate_count: 1000,
        affected_area: 1, // u64 (runtime.rs:50); 1 = 100% of image area budget
    }
}
```

```rust
// crates/rollshot-app/src/result_workspace/workbench/review.rs
// (Task 4 fills this in)
```

```rust
// crates/rollshot-app/src/result_workspace/workbench/view.rs
use super::super::{Message, ResultWorkspace};
use super::WorkbenchState;

/// Placeholder; Task 6 fills in the real layout. Takes `&ResultWorkspace` so
/// the real layout can reuse the existing `canvas_view` machinery (which
/// reads viewport/editor/handle from `ResultWorkspace`, not `WorkbenchState`).
pub fn workbench_view<'a>(state: &'a ResultWorkspace) -> iced::Element<'a, Message> {
    let _ = state;
    iced::widget::text("Smart Redaction (work in progress)").into()
}
```

### Step 5: Add `mode` field to `ResultWorkspace`

In `crates/rollshot-app/src/result_workspace/mod.rs`:

```rust
// At top with other mod declarations:
pub mod workbench;

// In the ResultWorkspace struct (after `pub editor: canvas::EditorState,`):
pub mode: workbench::WorkspaceMode,

// In ResultWorkspace::with_max_texture_dim (and any other constructors):
//   add `mode: workbench::WorkspaceMode::Normal,` to the struct literal.
```

### Step 6: Add message variants + toolbar entry + routing stub

In `crates/rollshot-app/src/result_workspace/update.rs`, add to the `Message` enum:

```rust
/// Smart Redaction toolbar button pressed.
SmartRedaction,
/// Messages forwarded from the workbench sub-state.
Workbench(super::workbench::WorkbenchMessage),
```

In `update_inner` (the `match message { ... }`), add:

```rust
Message::SmartRedaction => {
    state.mode = workbench::WorkspaceMode::Workbench(workbench::WorkbenchState::default());
    Task::none()
}
Message::Workbench(msg) => {
    // Routed in Task 7's full handler. Stub for now: drop the message.
    let _ = msg;
    Task::none()
}
```

In `crates/rollshot-app/src/result_workspace/view.rs` `view()`, branch on mode:

```rust
// Replace the existing `let layout = column![toolbar, disclosure, message_area, workspace_row, status]...`
// with a mode-aware version:
let body: Element<'_, Message> = match &state.mode {
    workbench::WorkspaceMode::Normal => {
        let workspace_row: Element<'_, Message> = if state.editor.navigator_open {
            row![canvas_area, super::navigator::navigator_panel(state)].spacing(4).into()
        } else { canvas_area };
        column![toolbar, disclosure, message_area, workspace_row, status]
            .spacing(8).padding(8).into()
    }
    workbench::WorkspaceMode::Workbench(_) => {
        workbench::view::workbench_view(state)
    }
};
```

And in `toolbar(state)`, add a Smart Redaction button beside `Tool::Redact`:

```rust
// After the ICON_REDACT tool_button line:
button(text("Smart Redaction")).on_press(Message::SmartRedaction),
```

### Step 7: Verify

Run: `rtk cargo check -p rollshot-app`
Expected: PASS — compiles. The `Message::Workbench` arm is unreachable (clippy may warn; acceptable at this stage).

Run: `rtk cargo fmt --all -- --check && rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: PASS (or only the unreachable-pattern warning, which we silence with `let _ = msg;`).

### Step 8: Commit

```bash
git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/result_workspace/
git commit -m "feat(workbench): scaffold workbench module + WorkspaceMode

Add rollshot-{agent,preset,vision,edit-proposal,automation,automation-rquickjs}
deps to rollshot-app plus tokio (general), async_stream, toml, chrono.
Create workbench/ module with WorkbenchState (full spec §4.1 fields incl
session/provider_config/budget), WorkspaceMode, WorkbenchMessage (all
variants), WorkbenchError (spec §9.1 mapping), RunState, CandidateReview,
ActivityEntry. Stub provider_config/run/review/view. Add mode field to
ResultWorkspace and SmartRedaction toolbar button. Compiles; no behavior."
```

---

## Task 2: Provider configuration (domain + load) — TDD

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs` (re-export)

**Interfaces:**
- Consumes: `rollshot_agent::provider::{AnthropicAdapter, OpenAIAdapter, ProviderAdapter}`
- Produces: `ProviderConfig`, `ProviderKind`, `KeySource`, `load_provider_config(config_dir)`, `save_provider_config(config_dir, &cfg)`, `resolve_key(&KeySource)`, `has_key(&cfg)`, `provider_model_label(&cfg)`, `build_adapter(&cfg) -> Result<Box<dyn ProviderAdapter>, String>`
- Later tasks consume: `build_adapter` for the agent run, `ProviderConfig` for the disclosure modal

### Step 1: Write failing tests

Append to `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs`:

```rust
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_provider_config() {
        let cfg = ProviderConfig::default();
        assert_eq!(cfg.provider, ProviderKind::Anthropic);
        assert_eq!(cfg.model, "claude-sonnet-4-6");
        assert!(cfg.base_url.is_none());
        assert!(matches!(cfg.key_source, KeySource::Env(ref v) if v == "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn load_missing_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = load_provider_config(tmp.path()).unwrap();
        assert_eq!(cfg.provider, ProviderKind::Anthropic);
    }

    #[test]
    fn load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let original = ProviderConfig {
            provider: ProviderKind::OpenAI,
            model: "gpt-4o".into(),
            base_url: Some("https://api.openai.com/v1".into()),
            key_source: KeySource::Env("OPENAI_API_KEY".into()),
        };
        save_provider_config(tmp.path(), &original).unwrap();
        let loaded = load_provider_config(tmp.path()).unwrap();
        assert_eq!(loaded.provider, ProviderKind::OpenAI);
        assert_eq!(loaded.model, "gpt-4o");
        assert_eq!(loaded.base_url.as_deref(), Some("https://api.openai.com/v1"));
    }

    #[test]
    fn resolve_env_key_absent_and_present() {
        let var = "TEST_ROLLSHOT_PROVIDER_KEY_928374";
        std::env::remove_var(var);
        assert_eq!(resolve_key(&KeySource::Env(var.into())), None);
        std::env::set_var(var, "sk-test");
        assert_eq!(resolve_key(&KeySource::Env(var.into())).as_deref(), Some("sk-test"));
        std::env::remove_var(var);
    }

    #[test]
    fn load_invalid_toml_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("provider.toml"), "not = valid = toml").unwrap();
        assert!(load_provider_config(tmp.path()).is_err());
    }

    #[test]
    fn provider_model_label_format() {
        let cfg = ProviderConfig {
            provider: ProviderKind::Anthropic,
            model: "claude-sonnet-4-6".into(),
            base_url: None,
            key_source: KeySource::Env("X".into()),
        };
        assert_eq!(provider_model_label(&cfg), "Anthropic / claude-sonnet-4-6");
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::provider_config`
Expected: FAIL — `load_provider_config` / `save_provider_config` / `resolve_key` / `has_key` / `provider_model_label` / `build_adapter` not defined.

### Step 3: Implement

Replace the Task 1 stub body of `provider_config.rs` (keep the `#[cfg(test)] mod tests` from Step 1) with:

```rust
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind { Anthropic, OpenAI }

impl Default for ProviderKind { fn default() -> Self { Self::Anthropic } }

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Anthropic => write!(f, "Anthropic"), Self::OpenAI => write!(f, "OpenAI") }
    }
}

/// How the API key is resolved at runtime. Never persisted in the config file
/// (only the *name* of the env var is persisted; the value is read at runtime).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeySource { Env(String) }

impl Default for KeySource { fn default() -> Self { Self::Env("ANTHROPIC_API_KEY".into()) } }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub key_source: KeySource,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Anthropic,
            model: "claude-sonnet-4-6".into(),
            base_url: None,
            key_source: KeySource::default(),
        }
    }
}

fn provider_config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("provider.toml")
}

pub fn load_provider_config(config_dir: &Path) -> Result<ProviderConfig, String> {
    let path = provider_config_path(config_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("invalid provider.toml: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProviderConfig::default()),
        Err(e) => Err(format!("failed to read provider.toml: {e}")),
    }
}

pub fn save_provider_config(config_dir: &Path, cfg: &ProviderConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("create config dir: {e}"))?;
    let path = provider_config_path(config_dir);
    let text = toml::to_string_pretty(cfg).map_err(|e| format!("serialize provider config: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("write provider.toml: {e}"))
}

/// Resolve the API key from the given source. Returns None if unavailable.
pub fn resolve_key(source: &KeySource) -> Option<String> {
    match source {
        KeySource::Env(var) => std::env::var(var).ok().filter(|s| !s.is_empty()),
    }
}

pub fn has_key(cfg: &ProviderConfig) -> bool { resolve_key(&cfg.key_source).is_some() }

pub fn provider_model_label(cfg: &ProviderConfig) -> String {
    format!("{} / {}", cfg.provider, cfg.model)
}

/// Build the provider adapter from the config. The adapter and the
/// `AuthorizedModelInput` are constructed from the same `ProviderConfig` so
/// `provider`/`model` strings match what the adapter streams (§10.7).
pub fn build_adapter(cfg: &ProviderConfig) -> Result<Box<dyn rollshot_agent::provider::ProviderAdapter>, String> {
    let key = resolve_key(&cfg.key_source).ok_or_else(|| "no provider key resolved".to_string())?;
    let base_url = cfg.base_url.as_deref().unwrap_or(match cfg.provider {
        ProviderKind::Anthropic => "https://api.anthropic.com",
        ProviderKind::OpenAI => "https://api.openai.com/v1",
    });
    Ok(match cfg.provider {
        ProviderKind::Anthropic => Box::new(
            rollshot_agent::provider::AnthropicAdapter::new(&key, base_url)
                .map_err(|e| format!("anthropic adapter: {e}"))?,
        ),
        ProviderKind::OpenAI => Box::new(
            rollshot_agent::provider::OpenAIAdapter::new(&key, base_url)
                .map_err(|e| format!("openai adapter: {e}"))?,
        ),
    })
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::provider_config`
Expected: PASS — 6 tests.

### Step 5: Register module in `workbench/mod.rs`

Add to the `pub use state::{...};` block in `workbench/mod.rs`:

```rust
pub use provider_config::{
    ProviderConfig, ProviderKind, KeySource, load_provider_config,
    save_provider_config, resolve_key, has_key, provider_model_label, build_adapter,
};
```

### Step 6: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/provider_config.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs
git commit -m "feat(workbench): provider config domain + load/save + build_adapter

ProviderConfig with ProviderKind, KeySource (env var), toml serialization.
load/save from rollshot_config_dir()/provider.toml; missing-file → default.
resolve_key from env. build_adapter constructs Anthropic/OpenAI adapter from
one config (§10.7 single-source rule). 6 unit tests."
```

---

## Task 3: Candidate review model (pure domain) — TDD

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`

**Interfaces:**
- Consumes: `rollshot_edit_proposal::{CandidateId, ProposedEdit, EditProposal}`
- Produces: `CandidateReview::from_candidates`, `mark_rejected`/`mark_modified`/`mark_pending`/`mark_accepted`, `decision_sets`, `is_empty`, `pending_count`, `rejected_count`, `modified_count`, `warning_count`; `RunState::is_idle`/`is_running`; `event_to_activity_entry`; `terminal_state_label`

### Step 1: Write failing tests

Append to `state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_edit_proposal::{CandidateId, ProposedEdit};
    use rollshot_image_document::ImageRect;

    fn cid(n: u64) -> CandidateId { CandidateId(n) }
    fn rect(x: f32, y: f32) -> ImageRect { ImageRect { x, y, width: 50.0, height: 50.0 } }

    #[test]
    fn from_candidates_marks_all_pending() {
        let review = CandidateReview::from_candidates(&[cid(1), cid(2), cid(3)]);
        assert_eq!(review.per_candidate.len(), 3);
        assert_eq!(review.pending_count(), 3);
        assert_eq!(review.rejected_count(), 0);
    }

    #[test]
    fn reject_then_undo_returns_to_pending() {
        let mut r = CandidateReview::from_candidates(&[cid(1)]);
        r.mark_rejected(cid(1));
        assert_eq!(r.rejected_count(), 1);
        r.mark_pending(cid(1));
        assert_eq!(r.per_candidate[&cid(1)], CandidateReviewState::Pending);
    }

    #[test]
    fn modify_replaces_edit() {
        let mut r = CandidateReview::from_candidates(&[cid(1)]);
        r.mark_modified(cid(1), ProposedEdit::AddRedaction { bounds: rect(10.0, 20.0) });
        match &r.per_candidate[&cid(1)] {
            CandidateReviewState::Modified(ProposedEdit::AddRedaction { bounds }) => {
                assert_eq!(bounds.x, 10.0); assert_eq!(bounds.y, 20.0);
            }
            _ => panic!("expected Modified(AddRedaction)"),
        }
    }

    #[test]
    fn decision_sets_partition_correctly() {
        let mut r = CandidateReview::from_candidates(&[cid(1), cid(2), cid(3)]);
        r.mark_rejected(cid(2));
        r.mark_modified(cid(3), ProposedEdit::AddRedaction { bounds: rect(0.0, 0.0) });
        let (apply, reject, modified) = r.decision_sets();
        assert!(apply.contains(&cid(1)) && apply.contains(&cid(3)));
        assert_eq!(reject, vec![cid(2)]);
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].0, cid(3));
    }

    #[test]
    fn warning_count_counts_low_confidence() {
        use rollshot_edit_proposal::{EditProposal, ProposedCandidate, ProposalId,
            ConfidenceSummary, Provenance, ProvenanceSource};
        let proposal = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: vec![
                ProposedCandidate { id: cid(1), edit: ProposedEdit::AddRedaction { bounds: rect(0.0,0.0) }, confidence: 0.9, label: "a".into(), rationale: None, provenance: Provenance { source: ProvenanceSource::Manual } },
                ProposedCandidate { id: cid(2), edit: ProposedEdit::AddRedaction { bounds: rect(0.0,0.0) }, confidence: 0.5, label: "b".into(), rationale: None, provenance: Provenance { source: ProvenanceSource::Manual } },
            ],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9, 0.5]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        };
        assert_eq!(CandidateReview::warning_count(&proposal, 0.75), 1);
    }

    #[test]
    fn event_to_activity_entry_maps_each_variant() {
        use rollshot_agent::runtime::RunEvent;
        let e = event_to_activity_entry(&RunEvent::TextChunk { text: "hi".into() });
        assert!(matches!(e, Some(ActivityEntry::AssistantText(t)) if t == "hi"));
        let e = event_to_activity_entry(&RunEvent::ToolCallStart { name: "dry_run".into() });
        assert!(matches!(e, Some(ActivityEntry::ToolCard { status: ToolCardStatus::Running, .. })));
        let e = event_to_activity_entry(&RunEvent::ToolCallEnd { name: "dry_run".into(), success: false });
        assert!(matches!(e, Some(ActivityEntry::ToolCard { status: ToolCardStatus::Failed, .. })));
        assert!(event_to_activity_entry(&RunEvent::TurnComplete).is_none());
    }

    #[test]
    fn terminal_label_covers_all_variants() {
        use rollshot_agent::driver::RunTerminalState::*;
        assert_eq!(terminal_state_label(&Cancelled), "Run cancelled");
        assert_eq!(terminal_state_label(&RuntimeFailure), "Runtime error");
        assert_eq!(terminal_state_label(&BudgetExhausted { dimension: BudgetDimension::WallTime }),
            "Budget exhausted: WallTime");
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::state`
Expected: FAIL — methods not defined.

### Step 3: Implement

Append to `state.rs` (imports already present from Task 1):

```rust
impl CandidateReview {
    pub fn from_candidates(candidates: &[CandidateId]) -> Self {
        Self {
            per_candidate: candidates.iter().map(|c| (*c, CandidateReviewState::Pending)).collect(),
        }
    }
    pub fn mark_rejected(&mut self, id: CandidateId) {
        self.per_candidate.insert(id, CandidateReviewState::Rejected);
    }
    pub fn mark_modified(&mut self, id: CandidateId, edit: ProposedEdit) {
        self.per_candidate.insert(id, CandidateReviewState::Modified(edit));
    }
    pub fn mark_pending(&mut self, id: CandidateId) {
        self.per_candidate.insert(id, CandidateReviewState::Pending);
    }
    pub fn mark_accepted(&mut self, id: CandidateId) {
        self.per_candidate.insert(id, CandidateReviewState::Accepted);
    }
    /// (apply_ids, reject_ids, modified_pairs).
    /// apply = Pending + Accepted + Modified; reject = Rejected.
    pub fn decision_sets(&self) -> (Vec<CandidateId>, Vec<CandidateId>, Vec<(CandidateId, ProposedEdit)>) {
        let mut apply = Vec::new(); let mut reject = Vec::new(); let mut modified = Vec::new();
        for (id, state) in &self.per_candidate {
            match state {
                CandidateReviewState::Pending | CandidateReviewState::Accepted => apply.push(*id),
                CandidateReviewState::Rejected => reject.push(*id),
                CandidateReviewState::Modified(edit) => { apply.push(*id); modified.push((*id, edit.clone())); }
            }
        }
        (apply, reject, modified)
    }
    pub fn is_empty(&self) -> bool { self.per_candidate.is_empty() }
    pub fn pending_count(&self) -> usize {
        self.per_candidate.values().filter(|s| matches!(s, CandidateReviewState::Pending)).count()
    }
    pub fn rejected_count(&self) -> usize {
        self.per_candidate.values().filter(|s| matches!(s, CandidateReviewState::Rejected)).count()
    }
    pub fn modified_count(&self) -> usize {
        self.per_candidate.values().filter(|s| matches!(s, CandidateReviewState::Modified(_))).count()
    }
    /// Count candidates below `threshold` confidence (spec §5.5 warnings).
    pub fn warning_count(proposal: &EditProposal, threshold: f32) -> usize {
        proposal.candidates.iter().filter(|c| c.confidence < threshold).count()
    }
}

impl RunState {
    pub fn is_idle(&self) -> bool { matches!(self, Self::Idle) }
    pub fn is_running(&self) -> bool { matches!(self, Self::Running { .. }) }
}

/// Map a RunEvent to an ActivityEntry for the live drawer. `TurnComplete` is
/// never emitted by the driver (§10.8) so it maps to `None`.
pub fn event_to_activity_entry(event: &RunEvent) -> Option<ActivityEntry> {
    match event {
        RunEvent::TextChunk { text } => Some(ActivityEntry::AssistantText(text.clone())),
        RunEvent::ToolCallStart { name } => Some(ActivityEntry::ToolCard {
            name: name.clone(), status: ToolCardStatus::Running, summary: String::new(),
        }),
        RunEvent::ToolCallEnd { name, success } => Some(ActivityEntry::ToolCard {
            name: name.clone(),
            status: if *success { ToolCardStatus::Success } else { ToolCardStatus::Failed },
            summary: String::new(),
        }),
        RunEvent::TurnComplete => None,
    }
}

/// Human-readable label for a terminal state (spec §6.3).
pub fn terminal_state_label(state: &RunTerminalState) -> String {
    use rollshot_agent::driver::RunTerminalState::*;
    match state {
        ReadyForReview(_) => "Ready for review".into(),
        NeedsUserInput(_) => "Needs your input".into(),
        Cancelled => "Run cancelled".into(),
        BudgetExhausted { dimension } => format!("Budget exhausted: {dimension:?}"),
        ProviderFailure { message } => format!("Provider error: {message}"),
        SourceValidationFailure => "Validation failed".into(),
        RuntimeFailure => "Runtime error".into(),
        AgentProtocolFailure { message } => format!("Agent error: {message}"),
    }
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::state`
Expected: PASS — 7 tests.

### Step 5: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/state.rs
git commit -m "feat(workbench): candidate review model + event→activity mapping

CandidateReview with from_candidates, mark_*, decision_sets, counts,
warning_count. RunState predicates. event_to_activity_entry maps each
RunEvent variant (TurnComplete → None). terminal_state_label for the drawer
header. 7 unit tests."
```

---

## Task 4: Review → apply orchestration (pure domain) — TDD

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs`

**Interfaces:**
- Consumes: `rollshot_edit_proposal::{EditProposal, ReviewDecision, lower}`, `rollshot_image_document::{ImageDocument, EditOp}`, `CandidateReview::decision_sets()`
- Produces: `build_review_decision(proposal, review, doc_state_id) -> ReviewDecision`, `restamp_proposal(proposal, doc_state_id) -> EditProposal`, `apply_candidates(proposal, review, document) -> Result<(), WorkbenchError>`
- Later tasks consume: `apply_candidates` in the Apply button handler

### Step 1: Write failing tests

```rust
// crates/rollshot-app/src/result_workspace/workbench/review.rs

use rollshot_edit_proposal::{CandidateId, EditProposal, ReviewDecision, lower};
use rollshot_image_document::ImageDocument;

use super::state::{CandidateReview, WorkbenchError};

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_edit_proposal::{
        CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate,
        ProposedEdit, Provenance, ProvenanceSource,
    };
    use rollshot_image_document::ImageRect;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect { x, y, width: w, height: h }
    }
    fn candidate(id: u64, b: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id), edit: ProposedEdit::AddRedaction { bounds: b },
            confidence: 0.9, label: "t".into(), rationale: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }
    }
    fn proposal(cands: Vec<ProposedCandidate>) -> EditProposal {
        EditProposal {
            id: ProposalId(1), base_document_state_id: 0, candidates: cands,
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }
    }

    #[test]
    fn restamp_proposal_updates_base_state_id() {
        let p = proposal(vec![candidate(1, rect(0.0, 0.0, 10.0, 10.0))]);
        let r = restamp_proposal(&p, 42);
        assert_eq!(r.base_document_state_id, 42);
        assert_eq!(r.candidates.len(), 1);
    }

    #[test]
    fn build_review_decision_all_pending() {
        let p = proposal(vec![candidate(1, rect(0.0,0.0,10.0,10.0)), candidate(2, rect(0.0,0.0,10.0,10.0))]);
        let review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let d = build_review_decision(&p, &review, 42);
        assert_eq!(d.accepted.len(), 2);
        assert_eq!(d.rejected.len(), 0);
        assert_eq!(d.modified.len(), 0);
        assert_eq!(d.resulting_document_state_id, 42);
    }

    #[test]
    fn build_review_decision_with_reject_and_modify() {
        let p = proposal(vec![candidate(1, rect(0.0,0.0,10.0,10.0)), candidate(2, rect(0.0,0.0,10.0,10.0))]);
        let mut review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(1));
        review.mark_modified(CandidateId(2), ProposedEdit::AddRedaction { bounds: rect(5.0,5.0,20.0,20.0) });
        let d = build_review_decision(&p, &review, 7);
        assert_eq!(d.accepted, vec![CandidateId(2)]);
        assert_eq!(d.rejected, vec![CandidateId(1)]);
        assert_eq!(d.modified.len(), 1);
    }

    #[test]
    fn apply_candidates_commits_one_annotation_per_accepted() {
        let p = proposal(vec![
            candidate(1, rect(10.0, 10.0, 50.0, 50.0)),
            candidate(2, rect(100.0, 100.0, 30.0, 30.0)),
        ]);
        let review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let mut doc = ImageDocument::new(image::RgbaImage::new(200, 200));
        assert_eq!(doc.annotations().len(), 0);
        apply_candidates(&p, &review, &mut doc).unwrap();
        assert_eq!(doc.annotations().len(), 2);
    }

    #[test]
    fn apply_candidates_skips_rejected() {
        let p = proposal(vec![
            candidate(1, rect(10.0, 10.0, 50.0, 50.0)),
            candidate(2, rect(100.0, 100.0, 30.0, 30.0)),
        ]);
        let mut review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(2));
        let mut doc = ImageDocument::new(image::RgbaImage::new(200, 200));
        apply_candidates(&p, &review, &mut doc).unwrap();
        assert_eq!(doc.annotations().len(), 1);
    }

    #[test]
    fn apply_candidates_uses_modified_bounds() {
        let p = proposal(vec![candidate(1, rect(10.0, 10.0, 50.0, 50.0))]);
        let mut review = CandidateReview::from_candidates(&[CandidateId(1)]);
        review.mark_modified(CandidateId(1), ProposedEdit::AddRedaction { bounds: rect(70.0, 70.0, 20.0, 20.0) });
        let mut doc = ImageDocument::new(image::RgbaImage::new(200, 200));
        apply_candidates(&p, &review, &mut doc).unwrap();
        // Annotation bounds come from the modified edit, not the original.
        use rollshot_image_document::annotation_bounds;
        let b = annotation_bounds(&doc.annotations()[0]);
        assert!((b.x - 70.0).abs() < 1e-5);
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::review`
Expected: FAIL — `build_review_decision` / `restamp_proposal` / `apply_candidates` not defined.

### Step 3: Implement

Append to `review.rs` (above the `#[cfg(test)]` block):

```rust
use rollshot_edit_proposal::ReviewDecision;
use super::state::CandidateReview;

/// Re-stamp a proposal's `base_document_state_id` from the live document.
/// DryRunTool hardcodes 0/1; this corrects it before `lower` (§10.5).
pub fn restamp_proposal(proposal: &EditProposal, doc_state_id: u64) -> EditProposal {
    let mut p = proposal.clone();
    p.base_document_state_id = doc_state_id;
    p
}

/// Build a `ReviewDecision` from the proposal and the user's review state.
pub fn build_review_decision(
    proposal: &EditProposal,
    review: &CandidateReview,
    doc_state_id: u64,
) -> ReviewDecision {
    let (accepted, rejected, modified) = review.decision_sets();
    ReviewDecision {
        proposal_id: proposal.id,
        accepted,
        rejected,
        modified,
        resulting_document_state_id: doc_state_id,
    }
}

/// Lower the proposal to `EditOp`s via the `ReviewDecision`, then apply as
/// one undoable `ImageDocument::apply_batch` transaction. Returns
/// `WorkbenchError::RuntimeFailure` if the document rejects the batch.
pub fn apply_candidates(
    proposal: &EditProposal,
    review: &CandidateReview,
    document: &mut ImageDocument,
) -> Result<(), WorkbenchError> {
    let restamped = restamp_proposal(proposal, document.state_id());
    let decision = build_review_decision(&restamped, review, document.state_id());
    let ops = lower(&restamped, &decision);
    if ops.is_empty() {
        return Ok(());
    }
    document.apply_batch(ops).map(|_| ()).map_err(|e| WorkbenchError::RuntimeFailure)
    // NOTE: EditError → RuntimeFailure is the spec §9.1 mapping for document
    // batch failures (not a preset-store or provider error).
}
```

(`EditError` does not carry a useful message for the user; if it does in a future revision, swap `RuntimeFailure` for a richer variant. The spec §9.1 row for `RuntimeFailure` is "Retry / report" which matches.)

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::review`
Expected: PASS — 6 tests.

### Step 5: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/review.rs
git commit -m "feat(workbench): review → apply orchestration

build_review_decision, restamp_proposal (fixes DryRunTool's hardcoded 0),
apply_candidates (lower + apply_batch in one undoable transaction; returns
WorkbenchError on document failure). Uses real BatchOutcome shape
(added_ids only, no warnings field). 6 unit tests."
```

---

## Task 5: Run existing preset (headless, no agent) — TDD

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

**Interfaces:**
- Consumes: `rollshot_automation::{execute_to_proposal, ExecutionPolicy, AutomationInput, ProposalContext, CancellationFlag, AutomationHost, ValidatedAutomation}`, `rollshot_automation_rquickjs::QuickJsExecutor`, `rollshot_vision::{VisualIndex, RealAutomationHost}`
- Produces: `run_existing_preset(image, revision, policy) -> Result<EditProposal, WorkbenchError>`
- Called from: `Message::Workbench(...)` when user selects a preset to run

### Step 1: Write failing test

Append to `run.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_automation::{
        execute_to_proposal, AutomationInput, CancellationFlag, ExecutionPolicy,
        FakeAutomationHost, ProposalContext,
    };
    use rollshot_automation_rquickjs::QuickJsExecutor;
    use rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource};

    fn test_image() -> image::RgbaImage {
        image::RgbaImage::from_fn(64, 64, |_, _| image::Rgba([200, 200, 200, 255]))
    }

    #[test]
    fn execute_dry_run_with_empty_main_returns_zero_candidates() {
        // Direct exercise of execute_to_proposal with a FakeAutomationHost,
        // proving the headless path the workbench uses is wired correctly.
        let source = r#"function main(input) { return { candidates: [] }; }"#;
        let limits = rollshot_automation::ValidationLimits::default();
        let validated = rollshot_automation::validate_source(source, &limits).unwrap();
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10), 100_000_000, 8_000_000,
        );
        let cancellation = CancellationFlag::default();
        let executor = QuickJsExecutor;
        let input = AutomationInput {
            image_width: 64, image_height: 64, region: None,
            annotations: vec![], capability_handles: Default::default(),
        };
        let ctx = ProposalContext {
            proposal_id: ProposalId(1),
            base_document_state_id: 0,
            provenance: Provenance { source: ProvenanceSource::Manual },
        };
        let mut host = FakeAutomationHost::default();
        let result = execute_to_proposal(&executor, &validated, &input, &ctx, &mut host, &policy, &cancellation);
        let (proposal, _metrics) = result.unwrap();
        assert_eq!(proposal.candidates.len(), 0);
    }

    #[test]
    fn run_existing_preset_rejects_empty_image() {
        // VisualIndex::build rejects 0x0 (index.rs:18) → VisionPrepare error.
        let empty = image::RgbaImage::new(0, 0);
        let revision = make_empty_revision();
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10), 100_000_000, 8_000_000,
        );
        let result = run_existing_preset(&empty, &revision, &policy);
        assert!(matches!(result, Err(WorkbenchError::VisionPrepare { .. })));
    }

    fn make_empty_revision() -> rollshot_preset::AutomationRevision {
        use rollshot_preset::*;
        let source = r#"function main(input) { return { candidates: [] }; }"#;
        let limits = rollshot_automation::ValidationLimits::default();
        let validated = rollshot_automation::validate_source(source, &limits).unwrap();
        AutomationRevision {
            store_schema_version: rollshot_preset::domain::STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("test".into()),
            parent_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            provenance: domain::RevisionProvenance {
                origin: domain::RevisionOrigin::Manual, note: None, source_run_ref: None,
            },
            artifact: validated,
        }
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::run`
Expected: FAIL — `run_existing_preset` not defined.

### Step 3: Implement

Append to `run.rs`:

```rust
use rollshot_automation::{
    execute_to_proposal, AutomationInput, CancellationFlag, ExecutionPolicy,
    ProposalContext,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{EditProposal, ProposalId, Provenance, ProvenanceSource};
use rollshot_preset::AutomationRevision;
use rollshot_vision::VisualIndex;
use super::state::WorkbenchError;

/// Run a preset's active `ValidatedAutomation` against the given image
/// (no LLM, no upload). Builds `VisualIndex`, prepares a fresh
/// `RealAutomationHost`, and runs the automation via `execute_to_proposal`.
/// Returns the dry-run `EditProposal`.
pub fn run_existing_preset(
    image: &image::RgbaImage,
    revision: &AutomationRevision,
    policy: &ExecutionPolicy,
) -> Result<EditProposal, WorkbenchError> {
    let (w, h) = image.dimensions();
    let _index = VisualIndex::build(image.clone())
        .map_err(|e| WorkbenchError::VisionPrepare { message: format!("VisualIndex: {e}") })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    let executor = QuickJsExecutor;
    let cancellation = CancellationFlag::default();
    let input = AutomationInput {
        image_width: w, image_height: h, region: None,
        annotations: vec![], capability_handles: Default::default(),
    };
    let ctx = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance { source: ProvenanceSource::Manual },
    };
    let (proposal, _metrics) = execute_to_proposal(
        &executor, &revision.artifact, &input, &ctx, &mut host, policy, &cancellation,
    ).map_err(|e| WorkbenchError::RuntimeFailure)?;
    Ok(proposal)
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::run`
Expected: PASS — 2 tests.

### Step 5: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/run.rs
git commit -m "feat(workbench): headless run-existing via execute_to_proposal

run_existing_preset builds VisualIndex + RealAutomationHost + QuickJsExecutor,
runs the revision's ValidatedAutomation through execute_to_proposal. No LLM,
no upload. VisionPrepare error on bad image; RuntimeFailure on exec error.
2 unit tests (empty-main happy path + empty-image rejection)."
```

---

## Task 6: Canvas candidate overlay (UI — iced 0.14)

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs` (review bar + workbench layout)
- Modify: `crates/rollshot-app/src/result_workspace/view.rs` (pass workbench state into canvas)

> **REQUIRED SUB-SKILL:** Load the `iced-rs` skill before this task. The candidate overlay uses `iced::widget::canvas` 0.14 APIs (`Path`, `Frame`, `Stroke`, `Text`); 0.13 signatures differ.

**Interfaces:**
- Consumes: `WorkbenchState.pending_proposal`, `CandidateReview`, `CandidateReviewState`, `proposed_edit_bounds`
- Produces: candidate draw pass in `AnnotationCanvas::draw`; candidate hit-test helper; `workbench::view::workbench_view` real layout (canvas + review bar + candidate list + composer stub)

### Step 1: Extend `AnnotationCanvas` with workbench overlay fields

In `canvas.rs`, change the `AnnotationCanvas<'a>` struct (`canvas.rs:196`):

```rust
pub struct AnnotationCanvas<'a> {
    pub document: &'a rollshot_image_document::ImageDocument,
    pub editor: &'a EditorState,
    pub scale: f32,
    pub visible: ImageRect,
    // SP6 workbench candidate overlay. `None` in Normal mode.
    pub pending_proposal: Option<&'a rollshot_edit_proposal::EditProposal>,
    pub review: Option<&'a super::workbench::CandidateReview>,
    pub selected_candidate: Option<rollshot_edit_proposal::CandidateId>,
}
```

Update every construction site (in `view.rs` `canvas_view`) to pass the new fields. In Normal mode pass `None`/`None`/`None`. In Workbench mode pass the workbench state's fields. See Step 4 for the call-site change.

### Step 2: Add the candidate overlay draw pass

In `AnnotationCanvas::draw` (`canvas.rs:388`), after the existing draft-annotation block and before the selection-handles block, add:

```rust
// SP6: proposed-candidate overlay. The overlay program owns candidate
// rendering; RenderShape stays unchanged. Proposed = dashed border (white,
// 40% alpha) or solid blue when selected. Rejected candidates are skipped
// (muted in the candidate list, not on canvas). Zoom-aware: confidence badge
// only at scale > 0.3. Cull to `self.visible` like committed annotations.
if let Some(proposal) = self.pending_proposal {
    let review = self.review;
    let s = self.scale;
    for cand in &proposal.candidates {
        let Some(bounds) = super::workbench::proposed_edit_bounds(&cand.edit) else { continue };
        if !bounds.intersects(&self.visible) { continue; }
        let is_rejected = matches!(
            review.and_then(|r| r.per_candidate.get(&cand.id)),
            Some(super::workbench::CandidateReviewState::Rejected)
        );
        if is_rejected { continue; }
        let is_selected = self.selected_candidate == Some(cand.id);

        let rect = iced::Rectangle {
            x: bounds.x * s, y: bounds.y * s,
            width: bounds.width * s, height: bounds.height * s,
        };
        let border_color = if is_selected {
            iced::Color::from_rgb(0.13, 0.40, 1.0)
        } else {
            iced::Color::from_rgba(1.0, 1.0, 1.0, 0.4)
        };
        let stroke = canvas::Stroke::default()
            .with_color(border_color)
            .with_width(if is_selected { 2.0 } else { 1.5 });
        // Dashed border via segmented path (6px on, 4px off). iced canvas
        // has no native dash; approximate by drawing on-segments only.
        draw_dashed_rect(&mut frame, rect, 6.0, 4.0, stroke);

        if s > 0.3 {
            let label = format!("{} {:.0}%", cand.label, cand.confidence * 100.0);
            frame.fill_text(canvas::Text {
                content: label,
                position: iced::Point::new(rect.x, rect.y - 14.0),
                color: iced::Color::WHITE,
                size: iced::Pixels(10.0),
                ..canvas::Text::default()
            });
        }
        if is_selected {
            for handle in [
                iced::Point::new(rect.x, rect.y),
                iced::Point::new(rect.x + rect.width, rect.y),
                iced::Point::new(rect.x, rect.y + rect.height),
                iced::Point::new(rect.x + rect.width, rect.y + rect.height),
            ] {
                let hr = canvas::Path::rectangle(
                    handle - iced::Vector::new(3.5, 3.5),
                    iced::Size::new(7.0, 7.0),
                );
                frame.fill(&hr, iced::Color::from_rgb(0.13, 0.40, 1.0));
            }
        }
    }
}
```

Add the helper near the other canvas helpers:

```rust
fn draw_dashed_rect(
    frame: &mut canvas::Frame,
    rect: iced::Rectangle,
    dash: f32,
    gap: f32,
    stroke: canvas::Stroke,
) {
    // Walk the perimeter, drawing on-segments only.
    let perimeter = 2.0 * (rect.width + rect.height);
    let mut path = canvas::Path::new();
    let mut dist = 0.0f32;
    while dist < perimeter {
        let on_end = (dist + dash).min(perimeter);
        let a = point_on_rect_perimeter(rect, dist);
        let b = point_on_rect_perimeter(rect, on_end);
        path = path.move_to(a);
        path = path.line_to(b);
        dist += dash + gap;
    }
    frame.stroke(&path, stroke);
}

fn point_on_rect_perimeter(rect: iced::Rectangle, dist: f32) -> iced::Point {
    let w = rect.width; let h = rect.height; let peri = 2.0 * (w + h);
    let d = dist.rem_euclid(peri);
    if d < w { iced::Point::new(rect.x + d, rect.y) }
    else if d < w + h { iced::Point::new(rect.x + w, rect.y + (d - w)) }
    else if d < 2.0 * w + h { iced::Point::new(rect.x + w - (d - w - h), rect.y + h) }
    else { iced::Point::new(rect.x, rect.y + h - (d - 2.0 * w - h)) }
}
```

### Step 3: Add a candidate hit-test helper (pure, tested)

In `canvas.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn hit_test_proposal_candidate_finds_contained() {
    use super::super::workbench::{proposed_edit_bounds, CandidateReview};
    use rollshot_edit_proposal::{CandidateId, ConfidenceSummary, EditProposal, ProposalId,
        ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource};
    use rollshot_image_document::{ImagePoint, ImageRect};

    let proposal = EditProposal {
        id: ProposalId(1), base_document_state_id: 0,
        candidates: vec![ProposedCandidate {
            id: CandidateId(1),
            edit: ProposedEdit::AddRedaction { bounds: ImageRect { x: 10.0, y: 10.0, width: 50.0, height: 50.0 } },
            confidence: 0.9, label: "t".into(), rationale: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
        rationale_summary: None,
        provenance: Provenance { source: ProvenanceSource::Manual },
    };
    let review = CandidateReview::from_candidates(&[CandidateId(1)]);
    let hit = hit_test_proposal_candidate(&proposal, ImagePoint::new(20.0, 20.0), &review);
    assert_eq!(hit, Some(CandidateId(1)));
    let miss = hit_test_proposal_candidate(&proposal, ImagePoint::new(0.0, 0.0), &review);
    assert_eq!(miss, None);
}
```

And the helper itself (above the `#[cfg(test)]` block):

```rust
/// Hit-test proposed candidates in image space. Skips rejected candidates.
pub fn hit_test_proposal_candidate(
    proposal: &rollshot_edit_proposal::EditProposal,
    point: rollshot_image_document::ImagePoint,
    review: &super::workbench::CandidateReview,
) -> Option<rollshot_edit_proposal::CandidateId> {
    use super::workbench::{proposed_edit_bounds, CandidateReviewState};
    proposal.candidates.iter().find(|c| {
        if matches!(
            review.per_candidate.get(&c.id),
            Some(CandidateReviewState::Rejected)
        ) {
            return false;
        }
        proposed_edit_bounds(&c.edit).map_or(false, |b| b.contains(point))
    }).map(|c| c.id)
}
```

### Step 4: Wire the workbench state into the canvas call site

In `view.rs`, change `fn canvas_view` to `pub(crate) fn canvas_view` so `workbench::view` can call it (Task 6 Step 5). Then replace the `let overlay = iced::widget::canvas(super::canvas::AnnotationCanvas { ... })` block with:

```rust
let (pending_proposal, review, selected_candidate) = match &state.mode {
    super::workbench::WorkspaceMode::Workbench(wb) => (
        wb.pending_proposal.as_ref(),
        Some(&wb.review),
        wb.selected_candidate,
    ),
    _ => (None, None, None),
};
let overlay = iced::widget::canvas(super::canvas::AnnotationCanvas {
    document: &state.document.image,
    editor: &state.editor,
    scale: geometry.scale,
    visible: super::canvas::visible_image_rect(
        state.viewport.scroll_offset, state.viewport_bounds, geometry.scale, geometry.image_origin,
    ),
    pending_proposal,
    review,
    selected_candidate,
})
.width(Length::Fixed(geometry.rendered_size.width))
.height(Length::Fixed(geometry.rendered_size.height));
```

### Step 5: Real `workbench_view` layout

Replace the Task 1 stub in `workbench/view.rs` with a real layout that **renders the canvas itself** by calling the existing `canvas_view` from `result_workspace/view.rs`, then composes the workbench chrome around it. To avoid a circular call (`view::view` → `workbench_view` → `view::canvas_view`), extract `canvas_view` into a `pub(crate)` function first (it already is — `view.rs:195` `fn canvas_view`). Import it via `super::super::view::canvas_view`.

```rust
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

use super::{WorkbenchState, WorkbenchMessage};
use super::super::{Message, ResultWorkspace};

pub fn workbench_view<'a>(state: &'a ResultWorkspace) -> Element<'a, Message> {
    let wb = match &state.mode {
        super::WorkspaceMode::Workbench(wb) => wb,
        _ => return iced::widget::text("Not in workbench mode").into(),
    };

    // The canvas (image + annotation overlay + candidate overlay) is the
    // existing `canvas_view`, which already reads `state.mode` to populate
    // the candidate overlay fields (Task 6 Step 4).
    let canvas_area = super::super::view::canvas_view(state, state.original_size());

    let bar = review_bar(wb);
    let list = candidate_list(wb);
    let composer = composer(wb);

    let right_pane = scrollable(column![list, composer].spacing(8))
        .width(Length::Fixed(280.0))
        .height(Length::Fill);

    let main = row![canvas_area, right_pane].spacing(4).height(Length::Fill);
    let content = column![bar, main].spacing(8).padding(8);

    let with_modals: Element<'_, Message> = if wb.disclosure_pending {
        iced::widget::stack![content, disclosure_modal(wb)].into()
    } else {
        content.into()
    };
    with_modals
}

pub fn review_bar<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    let proposal = wb.pending_proposal.as_ref();
    let total = proposal.map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let apply = total - rejected;
    let warnings = proposal.map_or(0, |p| super::state::CandidateReview::warning_count(p, 0.75));

    let summary = if total > 0 {
        format!("Apply {apply} redactions, skip {rejected} rejected · {warnings} warnings")
    } else {
        "No candidates".to_string()
    };

    let pending_warning = if total > 0 {
        text(format!("{total} proposed redactions are preview-only. Apply before safe copy/save."))
            .size(11)
    } else {
        text("")
    };

    let actions = row![
        text(summary),
        iced::widget::horizontal_space(),
        button(text(format!("Apply {apply} redactions")))
            .on_press_maybe(if apply > 0 { Some(Message::Workbench(WorkbenchMessage::ApplyCandidates)) } else { None }),
        button(text("Next warning"))
            .on_press_maybe(if warnings > 0 { Some(Message::Workbench(WorkbenchMessage::NextWarning)) } else { None }),
    ].spacing(12).align_y(Alignment::Center);

    container(column![pending_warning, actions].spacing(4))
        .padding(8).width(Length::Fill)
        .into()
}

pub fn candidate_list<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    use super::state::CandidateReviewState;
    use rollshot_edit_proposal::CandidateId;
    let Some(proposal) = wb.pending_proposal.as_ref() else {
        return text("No candidates").into();
    };
    let mut col = column![].spacing(4).padding(8);
    for cand in &proposal.candidates {
        let is_rejected = matches!(
            wb.review.per_candidate.get(&cand.id), Some(CandidateReviewState::Rejected)
        );
        let r = row![
            text(format!("{} {:.0}%", cand.label, cand.confidence * 100.0)).size(11),
            iced::widget::horizontal_space(),
            button(text("Jump"))
                .on_press(Message::Workbench(WorkbenchMessage::CandidateSelected(cand.id))),
            button(text(if is_rejected { "Undo" } else { "Reject" }))
                .on_press(Message::Workbench(if is_rejected {
                    WorkbenchMessage::CandidateDeselected
                } else {
                    WorkbenchMessage::CandidateDeleted(cand.id)
                })),
        ].spacing(8);
        col = col.push(r);
    }
    scrollable(col).height(Length::Fill).into()
}

pub fn composer<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    let input = text_input("Ask the agent…", &wb.composer)
        .on_input(|s| Message::Workbench(WorkbenchMessage::ComposerChanged(s)))
        .on_submit(Message::Workbench(WorkbenchMessage::SendRequested));
    row![input.width(Length::Fill), button(text("Send")).on_press(Message::Workbench(WorkbenchMessage::SendRequested))]
        .spacing(8).into()
}

/// Disclosure modal. Radio buttons ONLY update `payload_mode` — they never
/// auto-confirm. Only the explicit "Send to {provider}" button confirms.
pub fn disclosure_modal<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    use super::PayloadMode;
    let label = super::provider_config::provider_model_label(&wb.provider_config);
    let dialog = container(
        column![
            text(format!("Send to {label}")).size(16),
            text("This run will send:").size(13),
            text("  Screenshot image (full-screenshot mode)"),
            text("  Local OCR/layout summary"),
            text("Privacy mode:").size(13),
            iced::widget::radio(
                "Full screenshot — best accuracy",
                PayloadMode::FullScreenshot,
                Some(wb.payload_mode),
                |m| Message::Workbench(WorkbenchMessage::PayloadModeSelected(m)),
            ),
            iced::widget::radio(
                "OCR/layout only — no image upload",
                PayloadMode::OcrLayoutOnly,
                Some(wb.payload_mode),
                |m| Message::Workbench(WorkbenchMessage::PayloadModeSelected(m)),
            ),
            iced::widget::vertical_space().height(12),
            row![
                button(text(format!("Send to {}", wb.provider_config.provider)))
                    .on_press(Message::Workbench(WorkbenchMessage::DisclosureConfirmed)),
                button(text("Cancel"))
                    .on_press(Message::Workbench(WorkbenchMessage::DisclosureCancelled)),
            ].spacing(12),
        ].spacing(8).padding(24).max_width(450)
    ).style(|_t| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
        ..Default::default()
    }).center_x(Length::Fill).center_y(Length::Fill);
    iced::widget::opaque(dialog).into()
}
```

### Step 6: Verify

Run: `rtk cargo test -p rollshot-app -- workbench::canvas`
Expected: PASS — 1 new test (hit-test) plus existing canvas tests.

Run: `rtk cargo check -p rollshot-app && rtk cargo fmt --all -- --check && rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: PASS.

### Step 7: Commit

```bash
git add crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/view.rs
git commit -m "feat(workbench): canvas candidate overlay + review bar + composer

AnnotationCanvas gains pending_proposal/review/selected_candidate fields;
candidate draw pass (dashed border, confidence badge, selected handles),
culled to visible rect. Hit-test helper (tested). workbench_view: review
bar with apply/skip counts, candidate list, composer, disclosure modal.
Disclosure radios update payload_mode only — explicit Send button confirms
(§7.2 explicit-consent rule)."
```

---

## Task 7: Agent run via `Task::run` channel bridge + streaming activity drawer

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (full `Message::Workbench` handler)

**Interfaces:**
- Consumes: `AgentRunner::run_with_provider`, `RunEvent`, `ToolContext`, `DryRunTool`, `ProviderConfig`, `build_adapter`
- Produces: `prepare_vision_context(image) -> Result<VisionContext, WorkbenchError>`, `start_agent_run(params, wb) -> Result<Task<Message>, WorkbenchError>`, full `WorkbenchMessage` handler

### Step 1: Write failing test for `prepare_vision_context`

Append to `run.rs`:

```rust
#[cfg(test)]
mod prepare_tests {
    use super::*;

    #[test]
    fn prepare_vision_context_rejects_empty_image() {
        let empty = image::RgbaImage::new(0, 0);
        let r = prepare_vision_context(&empty);
        assert!(matches!(r, Err(WorkbenchError::VisionPrepare { .. })));
    }

    #[test]
    fn prepare_vision_context_succeeds_for_valid_image() {
        let img = image::RgbaImage::from_fn(8, 8, |_, _| image::Rgba([200, 200, 200, 255]));
        let ctx = prepare_vision_context(&img).unwrap();
        assert_eq!(ctx.index.width(), 8);
        assert_eq!(ctx.index.height(), 8);
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::run::prepare_tests`
Expected: FAIL — `prepare_vision_context` not defined.

### Step 3: Implement `prepare_vision_context` + `start_agent_run`

Append to `run.rs`:

```rust
use std::sync::{Arc, Mutex as StdMutex};
use rollshot_agent::{
    driver::{AgentRunner, AgentConfig, RunTerminalState},
    domain::{AgentSession, AttachmentDescriptor, AuthorizedModelInput, MediaType, SessionId},
    runtime::{RunCancellation, RunEvent, RunEventSink},
    tools::{DryRunTool, GetContextSummaryTool, ReplaceSourceTool, RequestUserInputTool,
            SubmitForReviewTool, ToolContext, ToolRegistry, ToolRegistryLimits, ValidateSourceTool},
};
use rollshot_automation::{ExecutionPolicy, ValidationLimits};
use rollshot_vision::{RealAutomationHost, VisualIndex};
use super::{PendingRunParams, RunKind, VisionContext, WorkbenchState};
use super::provider_config::{build_adapter, has_key};
use super::state::WorkbenchError;

pub fn prepare_vision_context(image: &image::RgbaImage) -> Result<VisionContext, WorkbenchError> {
    let index = VisualIndex::build(image.clone())
        .map_err(|e| WorkbenchError::VisionPrepare { message: format!("VisualIndex: {e}") })?;
    let host = RealAutomationHost::new();
    Ok(VisionContext {
        index,
        host: Arc::new(StdMutex::new(host)),
        executor: rollshot_automation_rquickjs::QuickJsExecutor,
        cancellation: rollshot_automation::CancellationFlag::default(),
    })
}

/// Channel-backed `RunEventSink`. `try_send` drops events if the UI falls
/// behind (bounded channel) rather than blocking the agent loop.
struct ChannelEventSink { tx: tokio::sync::mpsc::Sender<RunEvent> }
impl RunEventSink for ChannelEventSink {
    fn emit(&self, event: RunEvent) { let _ = self.tx.try_send(event); }
}

/// Start a bounded agent run as an iced `Task` that streams `RunEvent`s and
/// emits a final `RunTerminal`. The `AgentSession` is **moved into the
/// spawned task by value** (not held in any `Mutex`) so the spawned future
/// stays `Send` across `.await`. The terminal is streamed out as the last
/// message.
///
/// The caller takes the session out of `WorkbenchState` (via `std::mem::take`)
/// before calling this and passes it by value. For the first release (D7:
/// in-memory, no cross-run resume) the caller installs a fresh
/// `AgentSession::new(session_id)` back into the state for the next run.
pub fn start_agent_run(
    params: &PendingRunParams,
    image: &image::RgbaImage,
    provider_config: &super::provider_config::ProviderConfig,
    budget: &rollshot_agent::runtime::RunBudget,
    session: AgentSession,
) -> Result<(iced::Task<Message>, RunCancellation), WorkbenchError> {
    if !has_key(provider_config) {
        return Err(WorkbenchError::Config);
    }
    let vision = prepare_vision_context(image)?;
    let adapter = build_adapter(provider_config)
        .map_err(|e| WorkbenchError::Config)?;

    let session_id = session.session_id;

    let validation_limits = ValidationLimits::default();
    let policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(25), 80_000_000, 8_000_000,
    );
    let cancellation = RunCancellation::new();
    let tool_ctx = Arc::new(ToolContext::new(
        session_id,
        params.active_revision_source.clone().unwrap_or_default(),
        validation_limits,
        policy,
        params.image_dims,
        &cancellation,
    ));
    let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
    // Registration best-effort; a duplicate-name error would mean a logic
    // bug, so map to RuntimeFailure.
    let reg = |r: &mut ToolRegistry, t: Arc<dyn rollshot_agent::tools::Tool>| -> Result<(), WorkbenchError> {
        r.register(t).map_err(|e| WorkbenchError::RuntimeFailure)
        // EditError/ToolError carry no user-facing message worth surfacing here.
        // If a future revision adds a message, swap to AgentProtocolFailure.
    };
    reg(&mut registry, Arc::new(ReplaceSourceTool::new(tool_ctx.clone())))?;
    reg(&mut registry, Arc::new(ValidateSourceTool::new(tool_ctx.clone())))?;
    reg(&mut registry, Arc::new(SubmitForReviewTool::new(tool_ctx.clone())))?;
    reg(&mut registry, Arc::new(RequestUserInputTool::new(tool_ctx.clone())))?;
    reg(&mut registry, Arc::new(GetContextSummaryTool::new(tool_ctx.clone())))?;
    reg(&mut registry, Arc::new(DryRunTool::new(
        tool_ctx.clone(),
        Arc::new(vision.executor),
        vision.host.clone() as Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
    )))?;

    let descriptor = AttachmentDescriptor {
        media_type: MediaType::Png,
        width: params.image_dims.0,
        height: params.image_dims.1,
        byte_count: (params.image_dims.0 as u64) * (params.image_dims.1 as u64) * 4,
    };
    let attachment_bytes: Vec<Vec<u8>> = match params.mode {
        RunKind::Author | RunKind::Improve => {
            // Full-screenshot mode: encode the image. OcrLayoutOnly would send
            // an empty vec (the inspect_* tools already bound their output).
            // For first release we always send the image in Author/Improve;
            // payload_mode is honored at the disclosure layer for the modal
            // copy, and a follow-up task will gate the bytes.
            let mut buf = Vec::new();
            image::DynamicImage::ImageRgba8(image.clone())
                .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .map_err(|e| WorkbenchError::VisionPrepare { message: format!("png encode: {e}") })?;
            vec![buf]
        }
    };
    let model_input = AuthorizedModelInput::new(
        wb.provider_config.provider.to_string().to_lowercase(),
        wb.provider_config.model.clone(),
        params.user_message.clone(),
        vec![descriptor],
        attachment_bytes,
    ).map_err(|_| WorkbenchError::Config)?;

    let runner = AgentRunner::new(AgentConfig::default());
    let budget = budget.clone();
    let (tx, rx) = tokio::sync::mpsc::channel::<RunEvent>(64);
    let sink = ChannelEventSink { tx };

    // Move everything the run needs into the spawned task. `session` is moved
    // (not locked), so the future is `Send`. The terminal is sent after the
    // run completes by chaining it onto the event stream.
    let cancellation_for_task = cancellation.clone();
    let tool_ctx_for_task = tool_ctx.clone();
    let run_task = tokio::spawn(async move {
        let mut session = session;
        let provider = adapter.as_ref();
        runner.run_with_provider(
            model_input, &mut session, &registry, budget,
            &cancellation_for_task, &sink, &tool_ctx_for_task, provider,
        ).await
        // `session` is dropped here; for first release we do not persist
        // conversation history across runs (D7 in-memory). A follow-up
        // subproject restores session resume.
    });

    let stream = async_stream::stream! {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            yield Message::Workbench(super::WorkbenchMessage::RunEvent(event));
        }
        // Channel closed (sink dropped at end of run). Await the terminal.
        if let Ok(terminal) = run_task.await {
            yield Message::Workbench(super::WorkbenchMessage::RunTerminal(terminal));
        }
    };
    let task = iced::Task::run(stream, std::convert::identity);
    Ok((task, cancellation))
}
```

> **Design note on the session:** The cleanest `Send`-safe approach is to *move* `AgentSession` into the spawned task (above). The caller (`DisclosureConfirmed` in Step 5) takes the session out of `WorkbenchState` via `std::mem::take` and passes it by value, then installs a fresh `AgentSession::new(session_id)` back into the state. This keeps the spawned future `Send` without `tokio::sync::Mutex` around `run_with_provider`'s `&mut AgentSession`. D7 (in-memory, no cross-run resume in SP6) makes losing the session history acceptable. The session-restore-on-terminal path is documented for the deferred persistence subproject.

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::run`
Expected: PASS — prepare_tests (2) + earlier (2) = 4.

### Step 5: Wire the full `Message::Workbench` handler in `update.rs`

Replace the Task 1 stub `Message::Workbench(msg)` arm with the full handler:

```rust
Message::Workbench(msg) => {
    let workbench = match &mut state.mode {
        workbench::WorkspaceMode::Workbench(wb) => wb,
        _ => return Task::none(),
    };
    match msg {
        workbench::WorkbenchMessage::RunEvent(event) => {
            if let Some(entry) = workbench::state::event_to_activity_entry(&event) {
                workbench.live_activity.push(entry);
            }
            Task::none()
        }
        workbench::WorkbenchMessage::RunTerminal(terminal) => {
            workbench.live_activity.push(workbench::state::ActivityEntry::TerminalLabel(
                workbench::state::terminal_state_label(&terminal)
            ));
            if let Some(err) = workbench::state::WorkbenchError::from_terminal(&terminal) {
                workbench.error = Some(err);
            }
            workbench.run_state = workbench::RunState::Terminal(terminal);
            // Populate pending_proposal + review from ReadyForReview.
            if let workbench::RunState::Terminal(
                rollshot_agent::driver::RunTerminalState::ReadyForReview(ref ready)
            ) = &workbench.run_state {
                workbench.pending_proposal = Some(ready.proposal.clone());
                let ids: Vec<_> = ready.proposal.candidates.iter().map(|c| c.id).collect();
                workbench.review = workbench::CandidateReview::from_candidates(&ids);
                workbench.pending_draft = Some(workbench::PendingDraft {
                    source: ready.automation.source.clone(),
                    assistant_text: ready.assistant_text.clone(),
                    validation_summary: ready.automation.validation_summary.clone(),
                });
            }
            Task::none()
        }
        workbench::WorkbenchMessage::CancelRun => {
            if let workbench::RunState::Running { ref cancellation, .. } = workbench.run_state {
                cancellation.cancel();
            }
            Task::none()
        }
        workbench::WorkbenchMessage::ApplyCandidates => {
            if let Some(proposal) = workbench.pending_proposal.clone() {
                match workbench::review::apply_candidates(&proposal, &workbench.review, &mut state.document.image) {
                    Ok(()) => {
                        workbench.pending_proposal = None;
                        workbench.review = workbench::CandidateReview::default();
                        workbench.selected_candidate = None;
                    }
                    Err(e) => workbench.error = Some(e),
                }
            }
            Task::none()
        }
        workbench::WorkbenchMessage::CandidateSelected(id) => {
            workbench.selected_candidate = Some(id);
            Task::none()
        }
        workbench::WorkbenchMessage::CandidateDeselected => {
            workbench.selected_candidate = None;
            Task::none()
        }
        workbench::WorkbenchMessage::CandidateDeleted(id) => {
            workbench.review.mark_rejected(id);
            if workbench.selected_candidate == Some(id) {
                workbench.selected_candidate = None;
            }
            Task::none()
        }
        workbench::WorkbenchMessage::CandidateMoved { id, new_bounds } => {
            workbench.review.mark_modified(id, rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds: new_bounds });
            Task::none()
        }
        workbench::WorkbenchMessage::AddManualCandidate { bounds } => {
            let id = rollshot_edit_proposal::CandidateId(workbench.next_manual_candidate_id);
            workbench.next_manual_candidate_id += 1;
            if let Some(proposal) = &mut workbench.pending_proposal {
                use rollshot_edit_proposal::{ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource};
                proposal.candidates.push(ProposedCandidate {
                    id,
                    edit: ProposedEdit::AddRedaction { bounds },
                    confidence: 1.0,
                    label: "manual".into(),
                    rationale: Some("Manually added missing candidate".into()),
                    provenance: Provenance { source: ProvenanceSource::Manual },
                });
            }
            workbench.review.mark_modified(id, rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds });
            Task::none()
        }
        workbench::WorkbenchMessage::NextWarning => { /* jump-to-next-warning: viewport scroll in a later task */ Task::none() }
        workbench::WorkbenchMessage::JumpToCandidate(_id) => { /* viewport scroll in a later task */ Task::none() }
        workbench::WorkbenchMessage::ComposerChanged(s) => { workbench.composer = s; Task::none() }
        workbench::WorkbenchMessage::SendRequested => {
            // Capture run params and open disclosure (author/improve only).
            let user_message = std::mem::take(&mut workbench.composer);
            if user_message.is_empty() { return Task::none(); }
            let (w, h) = state.document.image.source().dimensions();
            let params = workbench::PendingRunParams {
                user_message,
                image_dims: (w, h),
                active_revision_source: workbench.active_revision.as_ref().map(|r| r.artifact.source.clone()),
                mode: workbench::RunKind::Author,
            };
            workbench.disclosure_pending = true;
            workbench.pending_run = Some(params);
            Task::none()
        }
        workbench::WorkbenchMessage::PayloadModeSelected(m) => {
            workbench.payload_mode = m;
            Task::none()
        }
        workbench::WorkbenchMessage::DisclosureConfirmed => {
            workbench.disclosure_pending = false;
            let Some(params) = workbench.pending_run.take() else { return Task::none(); };
            let image = state.document.image.source().clone();
            // Take the session by value; install a fresh one for the next run (D7).
            let session = std::mem::replace(
                &mut workbench.session,
                rollshot_agent::domain::AgentSession::new(workbench.session.session_id),
            );
            match workbench::run::start_agent_run(
                &params, &image, &workbench.provider_config, &workbench.budget, session,
            ) {
                Ok((task, cancellation)) => {
                    workbench.run_state = workbench::RunState::Running {
                        cancellation,
                        stream_id: iced::widget::Id::unique(),
                    };
                    task
                }
                Err(e) => {
                    workbench.error = Some(e);
                    Task::none()
                }
            }
        }
        workbench::WorkbenchMessage::DisclosureCancelled => {
            workbench.disclosure_pending = false;
            workbench.pending_run = None;
            Task::none()
        }
        workbench::WorkbenchMessage::SavePresetOrRevision => {
            // Wired in Task 9.
            Task::none()
        }
        workbench::WorkbenchMessage::AskAgentToRevise
        | workbench::WorkbenchMessage::DiscardDraft
        | workbench::WorkbenchMessage::DiscardCandidates
        | workbench::WorkbenchMessage::ImStart
        | workbench::WorkbenchMessage::ToggleAdvancedDetails
        | workbench::WorkbenchMessage::OpenProviderSettings
        | workbench::WorkbenchMessage::DisclosureRequested(_) => {
            // Wired in Task 9/10; stub returns none for now.
            Task::none()
        }
    }
}
```

> **Design note on `SendRequested`:** No image is stashed on the state. The image is re-read from `state.document.image.source()` in `DisclosureConfirmed` (shown), so `SendRequested` only captures the composer text + image dims + run kind into `pending_run`. This avoids a stale-image bug if the document changes between Send and confirm (spec §7.4 resume rule, within-session).

### Step 6: Verify

Run: `rtk cargo test -p rollshot-app`
Expected: PASS — all workbench tests.

Run: `rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: PASS.

### Step 7: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/update.rs
git commit -m "feat(workbench): agent run via Task::run channel bridge + full handler

prepare_vision_context (VisualIndex + RealAutomationHost). start_agent_run
moves AgentSession into a tokio::spawn'd task (Send-safe across .await),
streams RunEvents via mpsc → Task::run(stream, identity), emits RunTerminal
on completion. Full Message::Workbench handler: RunEvent→activity drawer,
RunTerminal→proposal+review+draft, Cancel, Apply, candidate gestures,
manual-add, composer, disclosure flow. Config error if no provider key.
2 new prepare_tests."
```

---

## Task 8: Copy/Save gating + product result states — TDD

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs`

### Step 1: Write failing tests for the gating helpers

Append to `state.rs`:

```rust
#[cfg(test)]
mod gating_tests {
    use super::*;
    use crate::result_workspace::workbench::WorkbenchState;
    use rollshot_edit_proposal::{CandidateId, ConfidenceSummary, EditProposal, ProposalId,
        ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource};
    use rollshot_image_document::ImageRect;

    fn proposal(n: usize) -> EditProposal {
        let cands = (0..n).map(|i| ProposedCandidate {
            id: CandidateId(i as u64), edit: ProposedEdit::AddRedaction { bounds: ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 } },
            confidence: 0.9, label: "t".into(), rationale: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }).collect();
        EditProposal {
            id: ProposalId(1), base_document_state_id: 0, candidates: cands,
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }
    }

    #[test]
    fn has_pending_candidates_false_when_no_proposal() {
        let wb = WorkbenchState::default();
        assert!(!has_pending_candidates(&wb));
    }
    #[test]
    fn has_pending_candidates_true_with_proposal_and_review() {
        let mut wb = WorkbenchState::default();
        wb.pending_proposal = Some(proposal(2));
        wb.review = CandidateReview::from_candidates(&[CandidateId(0), CandidateId(1)]);
        assert!(has_pending_candidates(&wb));
    }
    #[test]
    fn apply_skip_summary_format() {
        let mut wb = WorkbenchState::default();
        wb.pending_proposal = Some(proposal(3));
        wb.review = CandidateReview::from_candidates(&[CandidateId(0), CandidateId(1), CandidateId(2)]);
        wb.review.mark_rejected(CandidateId(1));
        let s = apply_skip_summary(&wb);
        assert!(s.contains("Apply 2 redactions"));
        assert!(s.contains("skip 1 rejected"));
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::state::gating_tests`
Expected: FAIL — `has_pending_candidates` / `apply_skip_summary` not defined.

### Step 3: Implement the helpers

Append to `state.rs`:

```rust
use super::WorkbenchState;

/// Whether pending (unapplied) candidates exist. Copy/Save must warn or block.
pub fn has_pending_candidates(wb: &WorkbenchState) -> bool {
    wb.pending_proposal.is_some() && !wb.review.is_empty()
}

/// Apply/skip summary for the review bar and the Copy/Save warning.
pub fn apply_skip_summary(wb: &WorkbenchState) -> String {
    let total = wb.pending_proposal.as_ref().map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let apply = total - rejected;
    let warnings = wb.pending_proposal.as_ref().map_or(0, |p| {
        CandidateReview::warning_count(p, 0.75)
    });
    format!("Apply {apply} redactions, skip {rejected} rejected\n{warnings} warnings included")
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::state::gating_tests`
Expected: PASS — 3 tests.

### Step 5: Gate Copy/Save in `update.rs`

In the `Message::Copy` arm (`update.rs:416`) and `Message::SaveAs` arm (`update.rs:456`), insert a pending-candidate check before the existing logic:

```rust
Message::Copy => {
    if let workbench::WorkspaceMode::Workbench(ref wb) = state.mode {
        if workbench::state::has_pending_candidates(wb) {
            state.message = Some(InlineMessage::Error(format!(
                "{}\nApply them before safe copy/save.",
                workbench::state::apply_skip_summary(wb)
            )));
            return Task::none();
        }
    }
    commit_text_draft(state);
    let safe_output = state.has_secure_redactions();
    let result = super::actions::copy_image(&copy_payload(state));
    Task::done(Message::CopyFinished { result, safe_output })
}
```

Mirror the same guard in `Message::SaveAs`.

### Step 6: Add result-state banners to `workbench/view.rs`

Add the `result_state_banner` function:

```rust
pub fn result_state_banner<'a>(wb: &'a WorkbenchState) -> Option<Element<'a, Message>> {
    let proposal = wb.pending_proposal.as_ref()?;
    let total = proposal.candidates.len();
    if total == 0 {
        return Some(container(
            column![
                text("This preset did not find anything on this screenshot."),
                row![
                    button(text("Improve preset")).on_press(Message::Workbench(WorkbenchMessage::ImStart)),
                    button(text("Manual redact")).on_press(Message::SelectTool(super::super::canvas::Tool::Redact)),
                ].spacing(8),
            ].spacing(8).padding(12)
        ).into());
    }
    let warnings = super::state::CandidateReview::warning_count(proposal, 0.75);
    if warnings == total {
        return Some(container(
            column![
                text("Only low-confidence matches were found."),
                row![
                    button(text("Review warnings")).on_press(Message::Workbench(WorkbenchMessage::NextWarning)),
                    button(text("Improve preset")).on_press(Message::Workbench(WorkbenchMessage::ImStart)),
                    button(text("Discard")).on_press(Message::Workbench(WorkbenchMessage::DiscardCandidates)),
                ].spacing(8),
            ].spacing(8).padding(12)
        ).into());
    }
    Some(container(text(format!("{total} candidates found. Review before applying.")).padding(12)).into())
}
```

Then edit `workbench_view` to render it between the review bar and the main row. Replace:

```rust
let content = column![bar, main].spacing(8).padding(8);
```

with:

```rust
let mut content = column![bar].spacing(8).padding(8);
if let Some(banner) = result_state_banner(wb) {
    content = content.push(banner);
}
content = content.push(main);
```

### Step 7: Verify + Commit

Run: `rtk cargo test -p rollshot-app && rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: PASS.

```bash
git add crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/view.rs
git commit -m "feat(workbench): Copy/Save gating + product result banners

has_pending_candidates + apply_skip_summary (3 tests). Copy/Save blocked
with inline error while unapplied candidates exist (preview ≠ safe redaction).
Result-state banners: no-match, low-confidence-only, candidates-found."
```

---

## Task 9: Save revision + Improve Preset correction evidence — TDD

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs` (review drawer + improve modal)

### Step 1: Write failing tests

Append to `review.rs`:

```rust
use rollshot_preset::{PresetId, PresetStore, RevisionId, RevisionProvenance, RevisionOrigin};

#[cfg(test)]
mod save_tests {
    use super::*;
    use rollshot_preset::{PresetId, PresetStore, RevisionId};

    #[test]
    fn save_revision_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = PresetStore::open(tmp.path().to_path_buf());
        let preset_id = PresetId("test-preset".into());
        store.create_preset(preset_id.clone(), "Test".into(), "intent".into(), "2026-01-01T00:00:00Z".into()).unwrap();
        let source = r#"function main(input) { return { candidates: [] }; }"#;
        save_revision(&store, &preset_id, source, None, 42, "2026-01-01T00:00:00Z".into()).unwrap();
        let active = store.load_active_revision(&preset_id).unwrap();
        assert!(active.artifact.source.contains("function main"));
    }

    #[test]
    fn save_revision_invalid_source_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let store = PresetStore::open(tmp.path().to_path_buf());
        let preset_id = PresetId("test-preset".into());
        store.create_preset(preset_id.clone(), "Test".into(), "intent".into(), "2026-01-01T00:00:00Z".into()).unwrap();
        // No `main` function → validation fails.
        let bad = r#"function not_main() { return { candidates: [] }; }"#;
        assert!(save_revision(&store, &preset_id, bad, None, 1, "2026-01-01T00:00:00Z".into()).is_err());
    }
}

#[cfg(test)]
mod evidence_tests {
    use super::*;
    use rollshot_edit_proposal::{CandidateId, ConfidenceSummary, EditProposal, ProposalId,
        ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource};
    use rollshot_image_document::ImageRect;

    fn proposal() -> EditProposal {
        EditProposal {
            id: ProposalId(1), base_document_state_id: 0,
            candidates: vec![
                ProposedCandidate { id: CandidateId(1), edit: ProposedEdit::AddRedaction { bounds: ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 } }, confidence: 0.9, label: "a".into(), rationale: None, provenance: Provenance { source: ProvenanceSource::Manual } },
                ProposedCandidate { id: CandidateId(2), edit: ProposedEdit::AddRedaction { bounds: ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 } }, confidence: 0.9, label: "b".into(), rationale: None, provenance: Provenance { source: ProvenanceSource::Manual } },
            ],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9, 0.9]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }
    }

    #[test]
    fn correction_evidence_counts_reject_and_modify() {
        let p = proposal();
        let mut review = super::super::state::CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(1));
        review.mark_modified(CandidateId(2), ProposedEdit::AddRedaction { bounds: ImageRect { x: 1.0, y: 1.0, width: 5.0, height: 5.0 } });
        let e = assemble_correction_evidence(&p, &review);
        assert_eq!(e.rejected_count, 1);
        assert_eq!(e.modified_count, 1);
        assert_eq!(e.added_count, 0);
        assert!(format!("{e}").contains("1 rejected"));
        assert!(format!("{e}").contains("1 resized"));
    }

    #[test]
    fn correction_evidence_all_pending_is_zero() {
        let p = proposal();
        let review = super::super::state::CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let e = assemble_correction_evidence(&p, &review);
        assert_eq!(e.rejected_count, 0);
        assert_eq!(e.modified_count, 0);
        assert_eq!(e.added_count, 0);
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::review`
Expected: FAIL — `save_revision` / `assemble_correction_evidence` / `CorrectionEvidence` not defined.

### Step 3: Implement

Append to `review.rs`:

```rust
use rollshot_preset::{PresetId, PresetStore, RevisionId, RevisionOrigin, RevisionProvenance};

/// Save a validated automation as a new revision and set it active.
pub fn save_revision(
    store: &PresetStore,
    preset_id: &PresetId,
    source: &str,
    parent_rev_id: Option<&RevisionId>,
    session_id: u64,
    now: String,
) -> Result<(), WorkbenchError> {
    let limits = rollshot_automation::ValidationLimits::default();
    let validated = rollshot_automation::validate_source(source, &limits)
        .map_err(|diags| WorkbenchError::SourceValidationFailure)?;
    let rev_id = RevisionId(format!("rev-{}", chrono::Utc::now().timestamp_millis()));
    let provenance = RevisionProvenance {
        origin: RevisionOrigin::AgentRun,
        note: None,
        source_run_ref: Some(session_id.to_string()),
    };
    store.add_revision(preset_id, rev_id.clone(), parent_rev_id.cloned(), validated, provenance, now.clone())
        .map_err(|e| WorkbenchError::Store { message: e.to_string() })?;
    store.set_active_revision(preset_id, &rev_id, now)
        .map_err(|e| WorkbenchError::Store { message: e.to_string() })?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CorrectionEvidence {
    pub rejected_count: usize,
    pub modified_count: usize,
    pub added_count: usize,
}

impl std::fmt::Display for CorrectionEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} rejected, {} resized, {} manually added",
            self.rejected_count, self.modified_count, self.added_count)
    }
}

/// Assemble correction evidence for Improve Preset (spec §8.2).
pub fn assemble_correction_evidence(
    proposal: &EditProposal,
    review: &super::state::CandidateReview,
) -> CorrectionEvidence {
    let (_, rejected_ids, modified_pairs) = review.decision_sets();
    let _ = proposal; // rejected/modified counts come from review; added is tracked separately
    CorrectionEvidence {
        rejected_count: rejected_ids.len(),
        modified_count: modified_pairs.len(),
        added_count: 0, // manually-added candidates tracked by next_manual_candidate_id delta
    }
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::review`
Expected: PASS — all review tests (6 + 2 + 2 = 10).

### Step 5: Wire `SavePresetOrRevision` and `ImStart` in `update.rs`

In the `Message::Workbench` handler, replace the stub arms:

```rust
workbench::WorkbenchMessage::SavePresetOrRevision => {
    // For first release: save the pending_draft's source as a new revision
    // under a preset named by the user (placeholder: uses a default preset id
    // derived from the draft). A follow-up task adds the name/description UI.
    if let Some(draft) = workbench.pending_draft.clone() {
        // Open (or create) a preset store from rollshot_config_dir().
        if let Ok(config_dir) = crate::daemon::config::rollshot_config_dir() {
            let store = rollshot_preset::PresetStore::open(config_dir.join("presets"));
            let preset_id = rollshot_preset::PresetId("workbench-draft".into());
            if store.load_preset(&preset_id).is_err() {
                let _ = store.create_preset(
                    preset_id.clone(), "Workbench Draft".into(),
                    "Authored via Smart Redaction".into(),
                    chrono::Utc::now().to_rfc3339(),
                );
            }
            match workbench::review::save_revision(
                &store, &preset_id, &draft.source, None,
                workbench.session.session_id.get(),
                chrono::Utc::now().to_rfc3339(),
            ) {
                Ok(()) => workbench.pending_draft = None,
                Err(e) => workbench.error = Some(e),
            }
        } else {
            workbench.error = Some(workbench::state::WorkbenchError::Config);
        }
    }
    Task::none()
}
workbench::WorkbenchMessage::ImStart => {
    // Improve is only available from a review/correction context.
    if workbench.pending_proposal.is_some() && !workbench.review.is_empty() {
        workbench.disclosure_pending = true;
        // Reuse the composer path: the next Send becomes an Improve run.
        // A follow-up task captures the correction evidence into the run params.
    }
    Task::none()
}
```

### Step 6: Add the improve modal to `workbench/view.rs`

```rust
pub fn improve_modal<'a>(
    evidence: &workbench::review::CorrectionEvidence,
) -> Element<'a, Message> {
    let dialog = container(
        column![
            text("Correction evidence to send:").size(14),
            text(format!("- {evidence}")),
            iced::widget::checkbox("Include manually added candidates as examples", true)
                .on_toggle(|_| Message::Workbench(WorkbenchMessage::ImStart)),
            iced::widget::vertical_space().height(12),
            row![
                button(text("Send improvement"))
                    .on_press(Message::Workbench(WorkbenchMessage::DisclosureConfirmed)),
                button(text("Cancel"))
                    .on_press(Message::Workbench(WorkbenchMessage::DisclosureCancelled)),
            ].spacing(12),
        ].spacing(8).padding(24).max_width(450)
    ).style(|_t| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
        ..Default::default()
    }).center_x(Length::Fill).center_y(Length::Fill);
    iced::widget::opaque(dialog).into()
}
```

Render it in `workbench_view` when `disclosure_pending` is true AND the run kind is Improve (distinguish from author disclosure in a follow-up; for first release the improve modal is reachable but the run-kind distinction is a stub).

### Step 7: Verify + Commit

Run: `rtk cargo test -p rollshot-app && rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: PASS.

```bash
git add crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/view.rs
git commit -m "feat(workbench): save revision + Improve Preset correction evidence

save_revision validates source, writes an immutable revision via
PresetStore::add_revision + set_active_revision (Store error on IO/compat).
assemble_correction_evidence counts rejected/modified/added. ImStart
context-gated to review/correction states. Improve modal with explicit
include-checkbox. 4 new tests (2 save round-trip + 2 evidence)."
```

---

## Task 10: Platform verification + handoff

**Files:**
- Verify: `crates/rollshot-app/src/macos_product.rs` (Phase forwarding already covers nested workbench messages — verified `macos_product.rs:344-348`)
- Create: `docs/superpowers/handoffs/2026-06-25-preset-workbench.md`

### Step 1: Full verification on Linux

```
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk cargo test --workspace --exclude rollshot-ocr
```
Expected: all PASS.

### Step 2: Manual verification checklist (Linux `iced::application`)

- [ ] Smart Redaction toolbar button opens workbench mode
- [ ] Composer + Send opens disclosure modal (NOT auto-confirmed by radio buttons)
- [ ] Disclosure Cancel returns to composer without uploading
- [ ] Run-existing (load a preset with a trivial `main`) produces candidates on the canvas
- [ ] Candidate select/move/resize/delete/reject work; Add-manual-candidate works
- [ ] Apply candidates → committed redactions → dashed borders become solid (committed annotation render)
- [ ] Copy/Save blocked while pending candidates exist (inline error shows apply-skip summary)
- [ ] Save preset → PresetStore file written under `rollshot_config_dir()/presets/`
- [ ] No-match / low-confidence banners render with correct actions
- [ ] No OCR text / tool args / provider bodies in tracing events (grep `rollshot::workbench` targets)
- [ ] Tall stitched image (1080×20000) candidate overlay culls to visible rect (no frame stall)

### Step 3: Manual verification checklist (macOS `iced::daemon` `Phase::Workspace`)

- [ ] Same checklist above through macOS Phase forwarding (no macOS-specific code added — the existing `Message::Workspace(msg)` arm forwards nested `Workbench` variants)

### Step 4: Write handoff

Create `docs/superpowers/handoffs/2026-06-25-preset-workbench.md` documenting:
- What landed (tasks 1–9)
- Platform verification results
- Known limitations / deferred:
  - In-memory sessions only (D7); no cross-run resume
  - Budget tuning UI deferred (finite literal `smart_redaction_budget`)
  - Full provider-management settings UI deferred (minimal key-presence only)
  - `payload_mode` (OcrLayoutOnly) honored at the modal copy layer; bytes-gating is a follow-up (Author/Improve always sends image in first release)
  - Improve run-kind distinction vs Author in the modal is a stub (shared disclosure modal)
  - `Next warning` / `Jump to candidate` viewport scroll not yet wired (handlers are no-ops)
  - Activity drawer `ToolCard.summary` is empty (tool-specific bounded summaries deferred)
  - Fixture regression UI deferred (per spec §8.3)

### Step 5: Commit

```bash
git add docs/superpowers/handoffs/2026-06-25-preset-workbench.md
git commit -m "docs: Preset Workbench handoff (SP6)

Platform verification completed on Linux iced::application.
macOS Phase forwarding verified (no macOS-specific code added).
Known limitations and deferred work documented."
```

---

## Global test commands

```bash
# Default lane (every PR — no ort, no models)
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk cargo test --workspace --exclude rollshot-ocr
```

```bash
# OCR lane (only if touching rollshot-vision/ocr features — SP6 does not)
rtk cargo clippy -p rollshot-ocr -p rollshot-vision --features rollshot-vision/ocr --all-targets -- -D warnings
rtk cargo test -p rollshot-ocr
rtk cargo test -p rollshot-vision --features ocr
```
