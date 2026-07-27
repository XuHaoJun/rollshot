#![allow(dead_code, clippy::large_enum_variant)] // SP6 scaffolding

pub mod provider_config;
pub mod review;
pub mod run;
pub mod state;
pub mod view;

#[cfg(test)]
pub(crate) mod eval;

#[allow(unused_imports)]
pub use provider_config::{
    build_adapter, has_key, load_provider_config, provider_model_label, resolve_key,
    save_provider_config, KeySource, ProviderConfig, ProviderKind,
};
#[allow(unused_imports)]
pub use state::{
    proposed_edit_bounds, ActivityEntry, CandidateReview, CandidateReviewState, RunState,
    ToolCardStatus, WorkbenchError,
};

use rollshot_agent::driver::RunTerminalState;
use rollshot_agent::runtime::{RunBudget, RunEvent};
use rollshot_edit_proposal::{CandidateId, EditProposal};
use rollshot_image_document::ImageRect;
use rollshot_preset::{AutomationRevision, Preset};

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
#[derive(Debug)]
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
    pub parent_revision_id: Option<rollshot_preset::RevisionId>,
    pub revision_note: Option<String>,
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
    /// Cached result of `assemble_correction_evidence(…).is_empty()` —
    /// avoids per-frame Vec allocations in the view.
    pub corrections_non_empty: bool,
}

/// Parameters captured at Send time and consumed when disclosure is confirmed.
#[derive(Debug, Clone)]
pub struct PendingRunParams {
    pub user_message: String,
    pub image_dims: (u32, u32),
    pub active_revision_source: Option<String>,
    pub mode: RunKind,
    pub parent_revision_id: Option<rollshot_preset::RevisionId>,
    pub revision_note: Option<String>,
    pub preset_id: rollshot_preset::PresetId,
    pub preset_store_root: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Author,
    Improve,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        use rollshot_agent::domain::{AgentSession, RunId, SessionId};
        Self {
            preset: None,
            active_revision: None,
            session: AgentSession::new(SessionId::new(0), RunId::parse("run-00000000-0000-4000-8000-000000000000").unwrap()),
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
            corrections_non_empty: false,
        }
    }
}

impl WorkbenchState {
    /// Recompute the cached `corrections_non_empty` flag from current
    /// `pending_proposal` and `review`. Call after any mutation to either.
    pub fn recompute_corrections_non_empty(&mut self) {
        self.corrections_non_empty = self
            .pending_proposal
            .as_ref()
            .map(|p| !review::assemble_correction_evidence(p, &self.review).is_empty())
            .unwrap_or(false);
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
    /// Run could not start or failed before the agent produced a terminal
    /// state (e.g. vision-prepare failure). Carries the typed error so the
    /// UI can show the real message (spec §9.1 VisionPrepare row).
    RunFailed(state::WorkbenchError),
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
    CandidateUnrejected(CandidateId),
    CandidateMoved {
        id: CandidateId,
        new_bounds: ImageRect,
    },
    NextWarning,
    JumpToCandidate(CandidateId),
    AddManualCandidate {
        bounds: ImageRect,
    },
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
