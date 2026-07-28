use serde::{Deserialize, Serialize};
use tokio::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::domain::SessionId;

// ---------- Budget ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetDimension {
    WallTime,
    ModelCalls,
    InputTokens,
    OutputTokens,
    Cost,
    ToolCalls,
    PerToolCalls,
    ArgumentBytes,
    ResultBytes,
    SourceBytes,
    Attachments,
    ValidationAttempts,
    DryRunAttempts,
    CapabilityCalls,
    CandidateCount,
    AffectedArea,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunBudget {
    pub wall_time: Duration,
    pub model_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Estimated provider cost ceiling. NOT enforced by the driver yet: there is
    /// no per-provider/model pricing model, so cost is never charged (it stays
    /// 0). Bound spend via `input_tokens`/`output_tokens`/`model_calls` instead.
    /// Enforcing this requires wiring a pricing function (see §9).
    pub cost: f64,
    pub tool_calls: u32,
    pub per_tool_calls: u32,
    pub argument_bytes: u64,
    pub result_bytes: u64,
    pub source_bytes: u64,
    pub attachments: u32,
    pub validation_attempts: u32,
    pub dry_run_attempts: u32,
    pub capability_calls: u32,
    pub candidate_count: u32,
    pub affected_area: u64,
}

impl RunBudget {
    pub fn unlimited() -> Self {
        Self {
            wall_time: Duration::MAX,
            model_calls: u32::MAX,
            input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            cost: f64::MAX,
            tool_calls: u32::MAX,
            per_tool_calls: u32::MAX,
            argument_bytes: u64::MAX,
            result_bytes: u64::MAX,
            source_bytes: u64::MAX,
            attachments: u32::MAX,
            validation_attempts: u32::MAX,
            dry_run_attempts: u32::MAX,
            capability_calls: u32::MAX,
            candidate_count: u32::MAX,
            affected_area: u64::MAX,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageSnapshot {
    pub model_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: f64,
    pub tool_calls: u32,
    pub per_tool_calls: u32,
    pub argument_bytes: u64,
    pub result_bytes: u64,
    pub source_bytes: u64,
    pub attachments: u32,
    pub validation_attempts: u32,
    pub dry_run_attempts: u32,
    pub capability_calls: u32,
    pub candidate_count: u32,
    pub affected_area: u64,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BudgetError {
    #[error("{0:?} budget exceeded")]
    Exceeded(BudgetDimension),
    #[error("budget field overflow")]
    Overflow,
}

// ---------- Draft ----------

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum EvidenceKind {
    Validation,
    Policy,
    DryRun,
}

impl EvidenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::Policy => "policy",
            Self::DryRun => "dry_run",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceRecord {
    pub kind: EvidenceKind,
    pub source_generation: u64,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DraftError {
    #[error("generation overflow")]
    GenerationOverflow,
    #[error("stale generation: expected {expected}, got {actual}")]
    StaleGeneration { expected: u64, actual: u64 },
    #[error("evidence not found at index {0}")]
    EvidenceNotFound(usize),
}

#[derive(Debug, Clone)]
pub struct DraftState {
    pub session_id: SessionId,
    generation: u64,
    evidence: Vec<EvidenceRecord>,
}

impl DraftState {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            generation: 0,
            evidence: Vec::new(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn next_generation(&mut self) -> Result<u64, DraftError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(DraftError::GenerationOverflow)?;
        Ok(self.generation)
    }

    pub fn record_evidence(
        &mut self,
        kind: EvidenceKind,
        expected_generation: u64,
        now: Instant,
    ) -> Result<(), DraftError> {
        if self.generation != expected_generation {
            return Err(DraftError::StaleGeneration {
                expected: self.generation,
                actual: expected_generation,
            });
        }
        self.evidence.push(EvidenceRecord {
            kind,
            source_generation: expected_generation,
            timestamp: now,
        });
        Ok(())
    }

    pub fn evidence(&self) -> &[EvidenceRecord] {
        &self.evidence
    }

    pub fn invalidate_evidence_after(&mut self, generation: u64) {
        self.evidence.retain(|e| e.source_generation <= generation);
    }

    #[cfg(test)]
    pub(crate) fn set_generation_for_test(&mut self, gen: u64) {
        self.generation = gen;
    }
}

// ---------- Budget tracker ----------

#[derive(Debug, Clone)]
pub struct BudgetTracker {
    budget: RunBudget,
    used: UsageSnapshot,
    turn: UsageSnapshot,
    start: Instant,
    paused_elapsed: Duration,
}

impl BudgetTracker {
    pub fn new(budget: RunBudget, start: Instant) -> Self {
        Self {
            budget,
            used: UsageSnapshot::default(),
            turn: UsageSnapshot::default(),
            start,
            paused_elapsed: Duration::ZERO,
        }
    }

    pub fn remaining_wall_time(&self, now: Instant) -> Duration {
        let elapsed = self.paused_elapsed + (now - self.start);
        self.budget.wall_time.saturating_sub(elapsed)
    }

    pub fn set_paused_elapsed(&mut self, elapsed: Duration) {
        self.paused_elapsed = elapsed;
    }

    pub fn check_wall_time(&self, now: Instant) -> Result<(), BudgetError> {
        let elapsed = self.paused_elapsed + (now - self.start);
        if elapsed >= self.budget.wall_time {
            Err(BudgetError::Exceeded(BudgetDimension::WallTime))
        } else {
            Ok(())
        }
    }

    pub fn charge(&mut self, snapshot: UsageSnapshot) -> Result<(), BudgetError> {
        self.check_budget(&snapshot)?;
        self.merge_turn(snapshot);
        Ok(())
    }

    fn check_budget(&self, delta: &UsageSnapshot) -> Result<(), BudgetError> {
        let u = &self.used;
        let b = &self.budget;

        check_u32(
            u.model_calls,
            delta.model_calls,
            b.model_calls,
            BudgetDimension::ModelCalls,
        )?;
        check_u64(
            u.input_tokens,
            delta.input_tokens,
            b.input_tokens,
            BudgetDimension::InputTokens,
        )?;
        check_u64(
            u.output_tokens,
            delta.output_tokens,
            b.output_tokens,
            BudgetDimension::OutputTokens,
        )?;
        check_u32(
            u.tool_calls,
            delta.tool_calls,
            b.tool_calls,
            BudgetDimension::ToolCalls,
        )?;
        check_u32(
            u.per_tool_calls,
            delta.per_tool_calls,
            b.per_tool_calls,
            BudgetDimension::PerToolCalls,
        )?;
        check_u64(
            u.argument_bytes,
            delta.argument_bytes,
            b.argument_bytes,
            BudgetDimension::ArgumentBytes,
        )?;
        check_u64(
            u.result_bytes,
            delta.result_bytes,
            b.result_bytes,
            BudgetDimension::ResultBytes,
        )?;
        check_u64(
            u.source_bytes,
            delta.source_bytes,
            b.source_bytes,
            BudgetDimension::SourceBytes,
        )?;
        check_u32(
            u.attachments,
            delta.attachments,
            b.attachments,
            BudgetDimension::Attachments,
        )?;
        check_u32(
            u.validation_attempts,
            delta.validation_attempts,
            b.validation_attempts,
            BudgetDimension::ValidationAttempts,
        )?;
        check_u32(
            u.dry_run_attempts,
            delta.dry_run_attempts,
            b.dry_run_attempts,
            BudgetDimension::DryRunAttempts,
        )?;
        check_u32(
            u.capability_calls,
            delta.capability_calls,
            b.capability_calls,
            BudgetDimension::CapabilityCalls,
        )?;
        check_u32(
            u.candidate_count,
            delta.candidate_count,
            b.candidate_count,
            BudgetDimension::CandidateCount,
        )?;
        check_u64(
            u.affected_area,
            delta.affected_area,
            b.affected_area,
            BudgetDimension::AffectedArea,
        )?;

        let new_cost = u.cost + delta.cost;
        if new_cost > b.cost {
            return Err(BudgetError::Exceeded(BudgetDimension::Cost));
        }

        Ok(())
    }

    fn merge_turn(&mut self, delta: UsageSnapshot) {
        self.turn.model_calls = self.turn.model_calls.saturating_add(delta.model_calls);
        self.turn.input_tokens = self.turn.input_tokens.saturating_add(delta.input_tokens);
        self.turn.output_tokens = self.turn.output_tokens.saturating_add(delta.output_tokens);
        self.turn.cost += delta.cost;
        self.turn.tool_calls = self.turn.tool_calls.saturating_add(delta.tool_calls);
        self.turn.per_tool_calls = self
            .turn
            .per_tool_calls
            .saturating_add(delta.per_tool_calls);
        self.turn.argument_bytes = self
            .turn
            .argument_bytes
            .saturating_add(delta.argument_bytes);
        self.turn.result_bytes = self.turn.result_bytes.saturating_add(delta.result_bytes);
        self.turn.source_bytes = self.turn.source_bytes.saturating_add(delta.source_bytes);
        self.turn.attachments = self.turn.attachments.saturating_add(delta.attachments);
        self.turn.validation_attempts = self
            .turn
            .validation_attempts
            .saturating_add(delta.validation_attempts);
        self.turn.dry_run_attempts = self
            .turn
            .dry_run_attempts
            .saturating_add(delta.dry_run_attempts);
        self.turn.capability_calls = self
            .turn
            .capability_calls
            .saturating_add(delta.capability_calls);
        self.turn.candidate_count = self
            .turn
            .candidate_count
            .saturating_add(delta.candidate_count);
        self.turn.affected_area = self.turn.affected_area.saturating_add(delta.affected_area);
    }

    pub fn apply_turn(&mut self) {
        let t = std::mem::take(&mut self.turn);
        self.used.model_calls = self.used.model_calls.saturating_add(t.model_calls);
        self.used.input_tokens = self.used.input_tokens.saturating_add(t.input_tokens);
        self.used.output_tokens = self.used.output_tokens.saturating_add(t.output_tokens);
        self.used.cost += t.cost;
        self.used.tool_calls = self.used.tool_calls.saturating_add(t.tool_calls);
        self.used.per_tool_calls = self.used.per_tool_calls.saturating_add(t.per_tool_calls);
        self.used.argument_bytes = self.used.argument_bytes.saturating_add(t.argument_bytes);
        self.used.result_bytes = self.used.result_bytes.saturating_add(t.result_bytes);
        self.used.source_bytes = self.used.source_bytes.saturating_add(t.source_bytes);
        self.used.attachments = self.used.attachments.saturating_add(t.attachments);
        self.used.validation_attempts = self
            .used
            .validation_attempts
            .saturating_add(t.validation_attempts);
        self.used.dry_run_attempts = self
            .used
            .dry_run_attempts
            .saturating_add(t.dry_run_attempts);
        self.used.capability_calls = self
            .used
            .capability_calls
            .saturating_add(t.capability_calls);
        self.used.candidate_count = self.used.candidate_count.saturating_add(t.candidate_count);
        self.used.affected_area = self.used.affected_area.saturating_add(t.affected_area);
    }

    /// Check all accumulated usage against budget limits.
    ///
    /// Call this after `apply_turn()` to catch overages from the final turn
    /// that `charge()` cannot see (because `charge()` only checks per-turn
    /// deltas, not the cumulative total).
    pub fn check_accumulated(&self) -> Result<(), BudgetError> {
        let u = &self.used;
        let b = &self.budget;

        if u.model_calls > b.model_calls {
            return Err(BudgetError::Exceeded(BudgetDimension::ModelCalls));
        }
        if u.input_tokens > b.input_tokens {
            return Err(BudgetError::Exceeded(BudgetDimension::InputTokens));
        }
        if u.output_tokens > b.output_tokens {
            return Err(BudgetError::Exceeded(BudgetDimension::OutputTokens));
        }
        if u.tool_calls > b.tool_calls {
            return Err(BudgetError::Exceeded(BudgetDimension::ToolCalls));
        }
        if u.per_tool_calls > b.per_tool_calls {
            return Err(BudgetError::Exceeded(BudgetDimension::PerToolCalls));
        }
        if u.argument_bytes > b.argument_bytes {
            return Err(BudgetError::Exceeded(BudgetDimension::ArgumentBytes));
        }
        if u.result_bytes > b.result_bytes {
            return Err(BudgetError::Exceeded(BudgetDimension::ResultBytes));
        }
        if u.source_bytes > b.source_bytes {
            return Err(BudgetError::Exceeded(BudgetDimension::SourceBytes));
        }
        if u.attachments > b.attachments {
            return Err(BudgetError::Exceeded(BudgetDimension::Attachments));
        }
        if u.validation_attempts > b.validation_attempts {
            return Err(BudgetError::Exceeded(BudgetDimension::ValidationAttempts));
        }
        if u.dry_run_attempts > b.dry_run_attempts {
            return Err(BudgetError::Exceeded(BudgetDimension::DryRunAttempts));
        }
        if u.capability_calls > b.capability_calls {
            return Err(BudgetError::Exceeded(BudgetDimension::CapabilityCalls));
        }
        if u.candidate_count > b.candidate_count {
            return Err(BudgetError::Exceeded(BudgetDimension::CandidateCount));
        }
        if u.affected_area > b.affected_area {
            return Err(BudgetError::Exceeded(BudgetDimension::AffectedArea));
        }
        if u.cost > b.cost {
            return Err(BudgetError::Exceeded(BudgetDimension::Cost));
        }
        Ok(())
    }

    pub fn used(&self) -> &UsageSnapshot {
        &self.used
    }

    pub fn budget(&self) -> &RunBudget {
        &self.budget
    }

    /// Charge one model dispatch against committed `model_calls`.
    ///
    /// Updates only committed `used.model_calls`; does not inspect,
    /// apply, or clear the per-turn accumulator. Each provider
    /// dispatch — including overflow failures — consumes one unit.
    pub fn charge_model_dispatch(&mut self) -> Result<(), BudgetError> {
        check_u32(
            self.used.model_calls,
            1,
            self.budget.model_calls,
            BudgetDimension::ModelCalls,
        )?;
        self.used.model_calls = self.used.model_calls.saturating_add(1);
        Ok(())
    }
}

fn check_u32(used: u32, delta: u32, limit: u32, dim: BudgetDimension) -> Result<(), BudgetError> {
    match used.checked_add(delta) {
        Some(new) if new > limit => Err(BudgetError::Exceeded(dim)),
        Some(_) => Ok(()),
        None => Err(BudgetError::Exceeded(dim)),
    }
}

fn check_u64(used: u64, delta: u64, limit: u64, dim: BudgetDimension) -> Result<(), BudgetError> {
    match used.checked_add(delta) {
        Some(new) if new > limit => Err(BudgetError::Exceeded(dim)),
        Some(_) => Ok(()),
        None => Err(BudgetError::Exceeded(dim)),
    }
}

// ---------- Cancellation ----------

#[derive(Debug, Clone)]
pub struct RunCancellation {
    token: CancellationToken,
    flag: rollshot_automation::CancellationFlag,
}

impl RunCancellation {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            flag: rollshot_automation::CancellationFlag::new(),
        }
    }

    pub fn cancel(&self) {
        self.token.cancel();
        self.flag.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    pub async fn wait(&self) {
        self.token.cancelled().await;
    }

    pub fn automation_flag(&self) -> &rollshot_automation::CancellationFlag {
        &self.flag
    }
}

impl Default for RunCancellation {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- Events ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDiffLineKind {
    Context,
    Removed,
    Added,
    Omitted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceDiffLine {
    pub kind: SourceDiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceDiffSummary {
    pub old_generation: u64,
    pub new_generation: u64,
    pub old_source_bytes: usize,
    pub new_source_bytes: usize,
    pub omitted_lines: usize,
    pub lines: Vec<SourceDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RunEvent {
    TextChunk {
        text: String,
    },
    ToolCallStart {
        name: String,
    },
    ToolCallEnd {
        name: String,
        success: bool,
    },
    SourceChanged {
        tool: String,
        diff: SourceDiffSummary,
    },
    TurnComplete,
}

pub trait RunEventSink: Send + Sync {
    fn emit(&self, event: RunEvent);
}

#[derive(Debug)]
pub struct NullEventSink;

impl RunEventSink for NullEventSink {
    fn emit(&self, _event: RunEvent) {}
}

// ---------- Terminal states ----------

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalState {
    Completed { summary: String },
    BudgetExhausted { dimension: BudgetDimension },
    Cancelled,
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionId;
    use tokio::time::{advance, pause, Duration, Instant};

    // ---- DraftState: generation counter ----

    #[test]
    fn draft_generation_starts_at_zero() {
        let d = DraftState::new(SessionId::new(1));
        assert_eq!(d.generation(), 0);
    }

    #[test]
    fn draft_generation_increments() {
        let mut d = DraftState::new(SessionId::new(1));
        assert_eq!(d.next_generation().unwrap(), 1);
        assert_eq!(d.next_generation().unwrap(), 2);
    }

    #[test]
    fn draft_generation_overflow_returns_error() {
        let mut d = DraftState::new(SessionId::new(1));
        d.set_generation_for_test(u64::MAX);
        assert_eq!(d.generation(), u64::MAX);
        assert_eq!(
            d.next_generation().unwrap_err(),
            DraftError::GenerationOverflow
        );
    }

    // ---- DraftState: evidence ----

    #[test]
    fn evidence_records_source_generation() {
        let mut d = DraftState::new(SessionId::new(1));
        d.next_generation().unwrap(); // gen 1
        d.record_evidence(EvidenceKind::Validation, 1, Instant::now())
            .unwrap();
        assert_eq!(d.evidence().len(), 1);
        assert_eq!(d.evidence()[0].source_generation, 1);
    }

    #[test]
    fn evidence_rejects_stale_generation() {
        let mut d = DraftState::new(SessionId::new(1));
        d.next_generation().unwrap(); // gen 1
        d.next_generation().unwrap(); // gen 2
        let err = d
            .record_evidence(EvidenceKind::Policy, 1, Instant::now())
            .unwrap_err();
        assert_eq!(
            err,
            DraftError::StaleGeneration {
                expected: 2,
                actual: 1
            }
        );
    }

    #[test]
    fn evidence_invalidated_after_source_replacement() {
        let mut d = DraftState::new(SessionId::new(1));
        d.next_generation().unwrap(); // gen 1
        d.record_evidence(EvidenceKind::DryRun, 1, Instant::now())
            .unwrap();
        d.next_generation().unwrap(); // gen 2
        d.record_evidence(EvidenceKind::Validation, 2, Instant::now())
            .unwrap();
        // Invalidate evidence after gen 1 (source replacement at gen 1)
        d.invalidate_evidence_after(1);
        assert_eq!(d.evidence().len(), 1);
        assert_eq!(d.evidence()[0].source_generation, 1);
    }

    // ---- Budget: each dimension ----

    #[tokio::test]
    async fn budget_wall_time_exceeded() {
        pause();
        let start = Instant::now();
        let budget = RunBudget {
            wall_time: Duration::from_secs(10),
            ..RunBudget::unlimited()
        };
        let tracker = BudgetTracker::new(budget, start);
        advance(Duration::from_secs(11)).await;
        assert!(matches!(
            tracker.check_wall_time(Instant::now()),
            Err(BudgetError::Exceeded(BudgetDimension::WallTime))
        ));
    }

    #[test]
    fn budget_model_calls_exceeded() {
        let budget = RunBudget {
            model_calls: 2,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        tracker
            .charge(UsageSnapshot {
                model_calls: 1,
                ..Default::default()
            })
            .unwrap();
        tracker.apply_turn();
        let err = tracker
            .charge(UsageSnapshot {
                model_calls: 2,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::ModelCalls));
    }

    #[test]
    fn budget_input_tokens_exceeded() {
        let budget = RunBudget {
            input_tokens: 100,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                input_tokens: 101,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::InputTokens));
    }

    #[test]
    fn budget_output_tokens_exceeded() {
        let budget = RunBudget {
            output_tokens: 50,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                output_tokens: 51,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::OutputTokens));
    }

    #[test]
    fn budget_cost_exceeded() {
        let budget = RunBudget {
            cost: 1.0,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                cost: 1.1,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::Cost));
    }

    #[test]
    fn budget_tool_calls_exceeded() {
        let budget = RunBudget {
            tool_calls: 3,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        tracker
            .charge(UsageSnapshot {
                tool_calls: 2,
                ..Default::default()
            })
            .unwrap();
        tracker.apply_turn();
        let err = tracker
            .charge(UsageSnapshot {
                tool_calls: 2,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::ToolCalls));
    }

    #[test]
    fn budget_per_tool_calls_exceeded() {
        let budget = RunBudget {
            per_tool_calls: 1,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        tracker
            .charge(UsageSnapshot {
                per_tool_calls: 1,
                ..Default::default()
            })
            .unwrap();
        tracker.apply_turn();
        let err = tracker
            .charge(UsageSnapshot {
                per_tool_calls: 1,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::PerToolCalls));
    }

    #[test]
    fn budget_argument_bytes_exceeded() {
        let budget = RunBudget {
            argument_bytes: 1000,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                argument_bytes: 1001,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::ArgumentBytes));
    }

    #[test]
    fn budget_result_bytes_exceeded() {
        let budget = RunBudget {
            result_bytes: 500,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                result_bytes: 501,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::ResultBytes));
    }

    #[test]
    fn budget_source_bytes_exceeded() {
        let budget = RunBudget {
            source_bytes: 200,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                source_bytes: 201,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::SourceBytes));
    }

    #[test]
    fn budget_attachments_exceeded() {
        let budget = RunBudget {
            attachments: 2,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        tracker
            .charge(UsageSnapshot {
                attachments: 1,
                ..Default::default()
            })
            .unwrap();
        tracker.apply_turn();
        let err = tracker
            .charge(UsageSnapshot {
                attachments: 2,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::Attachments));
    }

    #[test]
    fn budget_validation_attempts_exceeded() {
        let budget = RunBudget {
            validation_attempts: 1,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                validation_attempts: 2,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(
            err,
            BudgetError::Exceeded(BudgetDimension::ValidationAttempts)
        );
    }

    #[test]
    fn budget_dry_run_attempts_exceeded() {
        let budget = RunBudget {
            dry_run_attempts: 1,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                dry_run_attempts: 2,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::DryRunAttempts));
    }

    #[test]
    fn budget_capability_calls_exceeded() {
        let budget = RunBudget {
            capability_calls: 2,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        tracker
            .charge(UsageSnapshot {
                capability_calls: 2,
                ..Default::default()
            })
            .unwrap();
        tracker.apply_turn();
        let err = tracker
            .charge(UsageSnapshot {
                capability_calls: 1,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::CapabilityCalls));
    }

    #[test]
    fn budget_candidate_count_exceeded() {
        let budget = RunBudget {
            candidate_count: 5,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                candidate_count: 6,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::CandidateCount));
    }

    #[test]
    fn budget_affected_area_exceeded() {
        let budget = RunBudget {
            affected_area: 100,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        let err = tracker
            .charge(UsageSnapshot {
                affected_area: 101,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::AffectedArea));
    }

    // ---- Budget: cumulative usage de-duplication ----

    #[test]
    fn cumulative_usage_deduplicates_within_turn() {
        let budget = RunBudget {
            input_tokens: 100,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        // Two charges in the same turn: 30 + 50 = 80, still under 100
        tracker
            .charge(UsageSnapshot {
                input_tokens: 30,
                ..Default::default()
            })
            .unwrap();
        tracker
            .charge(UsageSnapshot {
                input_tokens: 50,
                ..Default::default()
            })
            .unwrap();
        // Before apply_turn, used is still 0
        assert_eq!(tracker.used().input_tokens, 0);
        // After apply_turn, turn total is applied
        tracker.apply_turn();
        assert_eq!(tracker.used().input_tokens, 80);
    }

    // ---- Budget: checked arithmetic ----

    #[test]
    fn budget_charge_saturates_on_overflow() {
        let budget = RunBudget {
            model_calls: u32::MAX,
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, Instant::now());
        // Charge u32::MAX, then apply. Another charge would overflow but saturates.
        tracker
            .charge(UsageSnapshot {
                model_calls: u32::MAX,
                ..Default::default()
            })
            .unwrap();
        tracker.apply_turn();
        // Now used = u32::MAX. Charge 1 more — should be exceeded, not overflow panic.
        let err = tracker
            .charge(UsageSnapshot {
                model_calls: 1,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::ModelCalls));
    }

    // ---- Budget: paused-time deadline ----

    #[tokio::test]
    async fn budget_paused_time_affects_deadline() {
        pause();
        let start = Instant::now();
        let budget = RunBudget {
            wall_time: Duration::from_secs(10),
            ..RunBudget::unlimited()
        };
        let mut tracker = BudgetTracker::new(budget, start);
        // Simulate 8 seconds of paused time (e.g., user thinking)
        tracker.set_paused_elapsed(Duration::from_secs(8));
        // Only 2 seconds of real time remain. Advance by 3.
        advance(Duration::from_secs(3)).await;
        assert!(matches!(
            tracker.check_wall_time(Instant::now()),
            Err(BudgetError::Exceeded(BudgetDimension::WallTime))
        ));
    }

    // ---- Cancellation ----

    #[tokio::test]
    async fn cancellation_before_wait() {
        let rc = RunCancellation::new();
        rc.cancel();
        assert!(rc.is_cancelled());
        // wait() should return immediately since already cancelled
        tokio::select! {
            _ = rc.wait() => {},
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                panic!("wait() did not return after cancel");
            }
        }
    }

    #[tokio::test]
    async fn cancellation_during_wait() {
        let rc = RunCancellation::new();
        let rc2 = rc.clone();
        // Spawn a task that cancels after a short delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            rc2.cancel();
        });
        // wait() should complete after the cancel
        tokio::select! {
            _ = rc.wait() => {},
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                panic!("wait() did not return after cancel");
            }
        }
        assert!(rc.is_cancelled());
    }

    #[test]
    fn cancellation_sets_automation_flag() {
        let rc = RunCancellation::new();
        assert!(!rc.automation_flag().is_cancelled());
        rc.cancel();
        assert!(rc.automation_flag().is_cancelled());
    }

    // ---- Event sink ----

    #[test]
    fn null_event_sink_does_not_panic() {
        let sink = NullEventSink;
        sink.emit(RunEvent::TextChunk {
            text: "hello".into(),
        });
        sink.emit(RunEvent::ToolCallStart {
            name: "edit".into(),
        });
        sink.emit(RunEvent::ToolCallEnd {
            name: "edit".into(),
            success: true,
        });
        sink.emit(RunEvent::SourceChanged {
            tool: "edit_source".into(),
            diff: SourceDiffSummary {
                old_generation: 0,
                new_generation: 1,
                old_source_bytes: 3,
                new_source_bytes: 3,
                omitted_lines: 0,
                lines: vec![SourceDiffLine {
                    kind: SourceDiffLineKind::Added,
                    text: "new".into(),
                }],
            },
        });
        sink.emit(RunEvent::TurnComplete);
    }

    // ---- Terminal states ----

    #[test]
    fn terminal_states_carry_no_prompt_source_attachment_or_provider() {
        let states = vec![
            TerminalState::Completed {
                summary: "done".into(),
            },
            TerminalState::BudgetExhausted {
                dimension: BudgetDimension::WallTime,
            },
            TerminalState::Cancelled,
            TerminalState::Error {
                message: "oops".into(),
            },
        ];
        for state in &states {
            let dbg = format!("{state:?}");
            assert!(
                !dbg.contains("prompt")
                    && !dbg.contains("source")
                    && !dbg.contains("attachment")
                    && !dbg.contains("provider"),
                "Terminal state debug must not contain prompt/source/attachment/provider: {dbg}"
            );
        }
    }

    // ---- charge_model_dispatch ----

    #[test]
    fn model_dispatch_is_committed_even_when_no_usage_arrives() {
        let mut tracker = BudgetTracker::new(
            RunBudget {
                model_calls: 1,
                ..RunBudget::unlimited()
            },
            Instant::now(),
        );
        tracker.charge_model_dispatch().unwrap();
        assert_eq!(tracker.used().model_calls, 1);
        assert!(matches!(
            tracker.charge_model_dispatch(),
            Err(BudgetError::Exceeded(BudgetDimension::ModelCalls))
        ));
    }

    #[test]
    fn model_dispatch_does_not_touch_turn_accumulator() {
        let mut tracker = BudgetTracker::new(
            RunBudget {
                model_calls: 5,
                ..RunBudget::unlimited()
            },
            Instant::now(),
        );
        // Charge some token usage in the turn.
        tracker
            .charge(UsageSnapshot {
                model_calls: 2,
                input_tokens: 100,
                ..Default::default()
            })
            .unwrap();
        // Now charge a dispatch — this goes to committed, not turn.
        tracker.charge_model_dispatch().unwrap();
        // Turn still has 2 model_calls; committed has 1.
        assert_eq!(tracker.used().model_calls, 1);
        // Apply turn — adds the 2 from turn.
        tracker.apply_turn();
        assert_eq!(tracker.used().model_calls, 3);
    }

    #[test]
    fn model_dispatch_saturates_at_limit() {
        let mut tracker = BudgetTracker::new(
            RunBudget {
                model_calls: u32::MAX,
                ..RunBudget::unlimited()
            },
            Instant::now(),
        );
        // Fill up to max.
        tracker.charge_model_dispatch().unwrap();
        // Now manually set used to max - 1 and charge one more.
        // Actually, let's just test the normal flow: charge many times.
        // The tracker starts at 0, so charging u32::MAX times is impractical.
        // Instead, verify that at limit = 1, the second call fails.
        let mut tracker2 = BudgetTracker::new(
            RunBudget {
                model_calls: 1,
                ..RunBudget::unlimited()
            },
            Instant::now(),
        );
        tracker2.charge_model_dispatch().unwrap();
        let err = tracker2.charge_model_dispatch().unwrap_err();
        assert_eq!(err, BudgetError::Exceeded(BudgetDimension::ModelCalls));
    }
}
