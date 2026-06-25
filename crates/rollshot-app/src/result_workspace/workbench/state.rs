#![allow(dead_code)] // SP6 scaffolding: all types used by later tasks

use rollshot_agent::driver::RunTerminalState;
use rollshot_agent::runtime::{BudgetDimension, RunCancellation};
use rollshot_edit_proposal::{CandidateId, ProposedEdit};
use rollshot_image_document::ImageRect;

/// Where the workbench's run is in its lifecycle.
#[derive(Debug, Clone, Default)]
pub enum RunState {
    #[default]
    Idle,
    Running {
        cancellation: RunCancellation,
    },
    Terminal(RunTerminalState),
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
    ProviderFailure {
        message: String,
    },
    SourceValidationFailure,
    RuntimeFailure,
    AgentProtocolFailure {
        message: String,
    },
    BudgetExhausted {
        dimension: BudgetDimension,
    },
    VisionPrepare {
        message: String,
    },
    Store {
        message: String,
    },
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
            RunTerminalState::ProviderFailure { message } => Some(Self::ProviderFailure {
                message: message.clone(),
            }),
            RunTerminalState::SourceValidationFailure => Some(Self::SourceValidationFailure),
            RunTerminalState::RuntimeFailure => Some(Self::RuntimeFailure),
            RunTerminalState::AgentProtocolFailure { message } => {
                Some(Self::AgentProtocolFailure {
                    message: message.clone(),
                })
            }
            RunTerminalState::BudgetExhausted { dimension } => Some(Self::BudgetExhausted {
                dimension: *dimension,
            }),
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
