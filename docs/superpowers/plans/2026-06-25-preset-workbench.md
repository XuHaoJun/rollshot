# Preset Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first-release Smart Redaction Preset Workbench — a mode of the Result Workspace where users author, run, review, and save reusable redaction presets via a bounded visual agent, with candidates rendered as an overlay on the existing annotation canvas.

**Architecture:** The Workbench extends `ResultWorkspace` with a `WorkspaceMode` enum (Normal vs Workbench). Domain logic (candidate review, state machine, provider config, review→apply) lives in a new `workbench/` module under `result_workspace/`, fully TDD-tested. The agent run is bridged from the async `run_with_provider` to iced's `Task::run(stream, f)` via an mpsc channel. Candidate rendering is an overlay layer on the existing `AnnotationCanvas` canvas. The review drawer is a collapsible right panel with a default human-readable tab and an advanced technical tab.

**Tech Stack:** iced 0.14 (canvas, image, tokio), rollshot-agent (driver, runtime, tools), rollshot-preset (store, domain), rollshot-vision (RealAutomationHost, VisualIndex), rollshot-edit-proposal (EditProposal, lower), rollshot-automation (validate_source, execute_to_proposal, ExecutionPolicy), rollshot-automation-rquickjs (QuickJsExecutor). `tokio` features: rt, sync, time (via iced's "tokio" feature + workspace dep).

## Global Constraints

- iced 0.14 pin; canvas, image, tokio features enabled.
- `unsafe_code = "deny"` on rollshot-app (not "forbid" — macOS native drag bridge uses audited `#[allow(unsafe_code)]`).
- Workspace MSRV: 1.85 (rollshot-app `Cargo.toml` inherits `rust-version.workspace`).
- Tracing: stable `rollshot::workbench::*` targets, structured fields, no OCR text / image pixels / tool args / provider bodies in any event.
- Privacy: `ActivityEntry` bounded summaries only (counts, durations, labels). `ProviderConfig` key resolved at runtime from env/OS keychain, never written to config file.
- Disclosure: per-run, before every upload (author/improve). Run-existing bypasses (no upload). Two explicit consent lines (full-screenshot, OCR/layout-only).
- Platform: Linux iced::application + macOS iced::daemon Phase::Workspace. Every `WorkbenchMessage` forwarded through `macos_product::Message::Workspace`.
- `validate_source` returns `Vec<SourceDiagnostic>`, not a typed enum (workbench shows structured spans).
- `RunEvent::TurnComplete` is never emitted by the driver; turn boundaries inferred from `ToolCallEnd`/`TextChunk` patterns.
- `layout` permanently `capability_unavailable`; authoring guardrails reflect only `ocr`/`region_features`/`template_match`.
- Pending candidates are preview-only; they never count as safe redactions. Copy/Save warns or blocks while unapplied candidates exist.

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `crates/rollshot-app/src/result_workspace/workbench/mod.rs` | `WorkspaceMode`, `WorkbenchState`, `WorkbenchMessage` enums + re-exports |
| `crates/rollshot-app/src/result_workspace/workbench/state.rs` | `RunState`, `CandidateReviewState`, `CandidateReview`, `ActivityEntry`, `RunActivityEntry`, `VisionContext` types |
| `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs` | `ProviderConfig`, `ProviderKind`, `KeySource`, load/save, key resolution |
| `crates/rollshot-app/src/result_workspace/workbench/run.rs` | Run orchestration: build VisionContext, run-existing, agent-run channel bridge |
| `crates/rollshot-app/src/result_workspace/workbench/review.rs` | `build_review_decision`, `restamp_proposal`, apply orchestration, `WorkbenchError` |
| `crates/rollshot-app/src/result_workspace/workbench/view.rs` | Workbench layout: canvas-primary + collapsible activity/review drawers + disclosure modal |

### Modified files

| File | Change |
|---|---|
| `crates/rollshot-app/Cargo.toml` | Add deps: `rollshot-agent`, `rollshot-preset`, `rollshot-vision`, `rollshot-edit-proposal`, `rollshot-automation`, `rollshot-automation-rquickjs`, `tokio` (general, not linux-only) |
| `crates/rollshot-app/src/result_workspace/mod.rs` | Add `pub mod workbench;`, add `mode: WorkspaceMode` field to `ResultWorkspace` |
| `crates/rollshot-app/src/result_workspace/update.rs` | Add `Message::Workbench(WorkbenchMessage)`, `Message::SmartRedaction`, forward `update` + `subscription`, toolbar entry, Copy/Save gating |
| `crates/rollshot-app/src/result_workspace/view.rs` | Toolbar "Smart Redaction" button, Workbench layout mode, disclosure modal in `stack` |
| `crates/rollshot-app/src/result_workspace/canvas.rs` | Candidate overlay draw pass in `AnnotationCanvas::draw` |
| `crates/rollshot-app/src/result_workspace/secure_sharing.rs` | Pending-candidate gating: `has_pending_candidates(&WorkbenchState) -> bool` |

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
- Produces: `WorkspaceMode { Normal, Workbench(WorkbenchState) }`, `WorkbenchMessage` enum (empty variants for now), `Message::Workbench(WorkbenchMessage)`, `Message::SmartRedaction`
- Later tasks consume: `WorkbenchState` fields, `Message::Workbench(msg)` routing

### Step 1: Add Cargo dependencies

```toml
# crates/rollshot-app/Cargo.toml — add to [dependencies]
rollshot-agent = { path = "../rollshot-agent" }
rollshot-preset = { path = "../rollshot-preset" }
rollshot-vision = { path = "../rollshot-vision" }
rollshot-edit-proposal = { path = "../rollshot-edit-proposal" }
rollshot-automation = { path = "../rollshot-automation" }
rollshot-automation-rquickjs = { path = "../rollshot-automation-rquickjs" }
tokio = { workspace = true }         # rt + sync + time (shared with rollshot-agent)
```

Remove `tokio` from the linux-only `[target.'cfg(target_os = "linux")'.dependencies]` section (it is now general).

Verify: `rtk cargo check -p rollshot-app` — should compile (unused warning for new deps is expected).

### Step 2: Create `workbench/state.rs` — core types (stubs)

```rust
// crates/rollshot-app/src/result_workspace/workbench/state.rs

use rollshot_agent::runtime::{RunCancellation, RunEvent};
use rollshot_agent::driver::RunTerminalState;
use rollshot_image_document::ImageRect;
use rollshot_edit_proposal::{CandidateId, EditProposal};
use iced::widget;

/// Where the workbench's run is in its lifecycle.
#[derive(Debug, Clone)]
pub enum RunState {
    Idle,
    Running {
        cancellation: RunCancellation,
        stream_id: widget::Id,
    },
    Terminal(RunTerminalState),
}

/// Per-candidate review state.  Pending = will apply by default.
/// Rejected = will not apply.  Modified = will apply with the modified edit.
/// Accepted = explicit confirm (optional; not required for normal flow).
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateReviewState {
    Pending,
    Accepted,
    Rejected,
    rollshot_edit_proposal::ProposedEdit, // Modified = apply this instead
}

/// Per-candidate review map.
#[derive(Debug, Clone, Default)]
pub struct CandidateReview {
    pub per_candidate: std::collections::BTreeMap<CandidateId, CandidateReviewState>,
}

/// Activity entries reconstructed from the RunEvent stream for the activity drawer.
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

#[derive(Debug, Clone)]
pub enum ToolCardStatus {
    Running,
    Success,
    Failed,
}
```

### Step 3: Create `workbench/mod.rs` — enums + re-exports

```rust
// crates/rollshot-app/src/result_workspace/workbench/mod.rs

pub mod state;
pub mod provider_config;   // Task 2
pub mod run;               // Tasks 5, 7
pub mod review;            // Task 4

pub use state::{ActivityEntry, CandidateReview, CandidateReviewState, RunState, ToolCardStatus};

use rollshot_agent::runtime::RunEvent;
use rollshot_edit_proposal::{CandidateId, EditProposal};
use rollshot_image_document::ImagePoint;
use rollshot_preset::{Preset, AutomationRevision};

/// Workbench mode sub-state attached to ResultWorkspace.
#[derive(Debug)]
pub struct WorkbenchState {
    pub preset: Option<Preset>,
    pub active_revision: Option<AutomationRevision>,
    pub run_state: RunState,
    pub live_activity: Vec<ActivityEntry>,
    pub pending_proposal: Option<EditProposal>,
    pub pending_draft: Option<PendingDraft>,
    pub review: CandidateReview,
    pub vision: Option<VisionContext>,
    pub error: Option<String>,
    pub disclosure_pending: bool,
    pub next_candidate_id: u64,
}

/// Subset of DraftAutomation the workbench retains after a run.
#[derive(Debug, Clone)]
pub struct PendingDraft {
    pub source: String,
    pub assistant_text: String,
    pub validation_summary: ValidationSummaryRef,
}

#[derive(Debug, Clone)]
pub struct ValidationSummaryRef {
    pub source_bytes: usize,
    pub ast_nodes: u32,
    pub capability_calls: u32,
    pub max_output_candidates: u32,
}

/// Prepared vision state for the current run.
#[derive(Debug)]
pub struct VisionContext {
    pub index: rollshot_vision::VisualIndex,
    pub host: std::sync::Arc<std::sync::Mutex<rollshot_vision::RealAutomationHost>>,
    pub executor: rollshot_automation_rquickjs::QuickJsExecutor,
    pub cancellation: rollshot_automation::CancellationFlag,
}

/// Workspace mode: Normal (existing canvas + navigator) or Workbench.
#[derive(Debug, Default)]
pub enum WorkspaceMode {
    #[default]
    Normal,
    Workbench(WorkbenchState),
}

/// Messages scoped to the workbench.
#[derive(Debug, Clone)]
pub enum WorkbenchMessage {
    // Run events from the agent (streamed via Task::run channel bridge)
    RunEvent(RunEvent),
    RunTerminal(RunTerminalState),
    // Disclosure
    DisclosureConfirmed,
    DisclosureCancelled,
    // Candidate gestures (from canvas overlay)
    CandidateSelected(CandidateId),
    CandidateDeselected,
    CandidateDeleted(CandidateId),
    CandidateMoved { id: CandidateId, new_bounds: ImageRect },
    NextWarning,
    // Actions
    ApplyCandidates,
    SavePresetOrRevision,
    AskAgentToRevise,
    DiscardDraft,
    DiscardCandidates,
    ImStart, // "Improve Preset" start (context-gated)
    // Cancel
    CancelRun,
}
```

### Step 4: Add `mode` field to `ResultWorkspace`

```rust
// crates/rollshot-app/src/result_workspace/mod.rs — in the ResultWorkspace struct
pub mod workbench;  // add at top with other mod declarations

pub struct ResultWorkspace {
    // ... existing fields ...
    pub mode: workbench::WorkspaceMode,  // add this field
}

// In ResultWorkspace::new / with_max_texture_dim, add:
mode: workbench::WorkspaceMode::Normal,
next_candidate_id: 1,
```

### Step 5: Add message variants + toolbar entry

```rust
// crates/rollshot-app/src/result_workspace/update.rs — in Message enum
/// Smart Redaction toolbar button pressed.
SmartRedaction,
/// Messages forwarded from the workbench sub-state.
Workbench(workbench::WorkbenchMessage),
```

```rust
// In update_inner(), add arms:
Message::SmartRedaction => {
    // Placeholder: switch to workbench mode (presets loaded later)
    state.mode = workbench::WorkspaceMode::Workbench(workbench::WorkbenchState::default());
    Task::none()
}
Message::Workbench(msg) => {
    // Delegate to workbench update (populated in later tasks)
    Task::none()
}
```

```rust
// crates/rollshot-app/src/result_workspace/view.rs — in toolbar row
// Add a button next to Tool::Redact (canvas.rs:29, view.rs:72):
button("Smart Redaction").on_press(Message::SmartRedaction)
```

### Step 6: Verify

Run: `rtk cargo check -p rollshot-app`

Expected: compiles. `Message::Workbench` arm is unreachable for now (clippy warning acceptable). Toolbar shows the button.

Run: `rtk cargo fmt --all -- --check && rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`

### Step 7: Commit

```bash
git add crates/rollshot-app/
git commit -m "feat(workbench): scaffold workbench module + WorkspaceMode

Add rollshot-{agent,preset,vision,edit-proposal,automation,automation-rquickjs}
deps to rollshot-app. Create workbench/ module with core types (WorkbenchState,
WorkspaceMode, WorkbenchMessage, RunState, CandidateReview, ActivityEntry).
Add mode field to ResultWorkspace. Add SmartRedaction toolbar button (opens
workbench mode stub). All compiles; no behavior change."
```

---

## Task 2: Provider configuration (domain + load)

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs` (re-export)

**Interfaces:**
- Consumes: `rollshot_agent::provider::{AnthropicAdapter, OpenAIAdapter, ProviderAdapter}`
- Produces: `ProviderConfig`, `ProviderKind`, `KeySource`, `load_provider_config()`, `resolve_key()`, `build_adapter()`
- Later tasks consume: `build_adapter()` for agent run, `ProviderConfig` for disclosure modal

### Step 1: Write failing test

```rust
// crates/rollshot-app/src/result_workspace/workbench/provider_config.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_config() {
        let cfg = ProviderConfig::default();
        assert_eq!(cfg.provider, ProviderKind::Anthropic);
        assert_eq!(cfg.model, "claude-sonnet-4-6");
        assert!(cfg.base_url.is_none());
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
        assert_eq!(loaded.base_url, Some("https://api.openai.com/v1".into()));
    }

    #[test]
    fn resolve_env_key() {
        let key = resolve_key(&KeySource::Env("TEST_ROLLSHOT_KEY_12345".into()));
        assert!(key.is_none()); // env var not set
        std::env::set_var("TEST_ROLLSHOT_KEY_12345", "sk-test");
        let key = resolve_key(&KeySource::Env("TEST_ROLLSHOT_KEY_12345".into()));
        assert_eq!(key.as_deref(), Some("sk-test"));
        std::env::remove_var("TEST_ROLLSHOT_KEY_12345");
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::provider_config`

Expected: FAIL — `provider_config` module doesn't exist.

### Step 3: Implement

```rust
// crates/rollshot-app/src/result_workspace/workbench/provider_config.rs

use std::path::Path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    Anthropic,
    OpenAI,
}

impl Default for ProviderKind {
    fn default() -> Self { Self::Anthropic }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "Anthropic"),
            Self::OpenAI   => write!(f, "OpenAI"),
        }
    }
}

/// How the API key is resolved at runtime.  Never persisted in the config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeySource {
    /// Read from this environment variable.
    Env(String),
}

impl Default for KeySource {
    fn default() -> Self { Self::Env("ANTHROPIC_API_KEY".into()) }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub key_source: KeySource,
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

/// Resolve the API key from the given source.  Returns None if unavailable.
pub fn resolve_key(source: &KeySource) -> Option<String> {
    match source {
        KeySource::Env(var) => std::env::var(var).ok().filter(|s| !s.is_empty()),
    }
}

/// Whether a key is available for the given config (no key = configure-provider state).
pub fn has_key(cfg: &ProviderConfig) -> bool {
    resolve_key(&cfg.key_source).is_some()
}

/// Display name for disclosure modal.
pub fn provider_model_label(cfg: &ProviderConfig) -> String {
    format!("{} / {}", cfg.provider, cfg.model)
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::provider_config`

Expected: 4 tests PASS.

### Step 5: Register module in `workbench/mod.rs`

```rust
pub mod provider_config;
pub use provider_config::{ProviderConfig, ProviderKind, KeySource, load_provider_config, has_key};
```

### Step 6: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/provider_config.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs
git commit -m "feat(workbench): provider config domain + load/save

ProviderConfig with ProviderKind, KeySource (env var), toml serialization.
load_provider_config() from rollshot_config_dir()/provider.toml.
resolve_key() from env. 4 unit tests covering default, missing-file,
round-trip, and env resolution."
```

---

## Task 3: Candidate review model + activity types (pure domain)

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`

**Interfaces:**
- Consumes: `rollshot_edit_proposal::{CandidateId, ProposedEdit, EditProposal}`
- Produces: `CandidateReview` methods (mark_rejected, mark_modified, pending_summarize, apply_set, reject_set, modified_set), `ActivityEntry` construction helpers, `RunState` transition validation

### Step 1: Write failing tests

```rust
// crates/rollshot-app/src/result_workspace/workbench/state.rs — #[cfg(test)] mod tests

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_edit_proposal::{CandidateId, ProposedEdit};
    use rollshot_image_document::{ImagePoint, ImageRect};

    fn cid(n: u64) -> CandidateId { CandidateId(n) }

    #[test]
    fn new_review_marks_all_pending() {
        let cands = vec![cid(1), cid(2), cid(3)];
        let review = CandidateReview::from_candidates(&cands);
        assert_eq!(review.per_candidate.len(), 3);
        for c in &cands {
            assert_eq!(review.per_candidate[c], CandidateReviewState::Pending);
        }
    }

    #[test]
    fn reject_candidate() {
        let cands = vec![cid(1), cid(2)];
        let mut review = CandidateReview::from_candidates(&cands);
        review.mark_rejected(cid(1));
        assert_eq!(review.per_candidate[&cid(1)], CandidateReviewState::Rejected);
        assert_eq!(review.per_candidate[&cid(2)], CandidateReviewState::Pending);
    }

    #[test]
    fn modify_candidate() {
        let cands = vec![cid(1)];
        let mut review = CandidateReview::from_candidates(&cands);
        let new_bounds = ImageRect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
        review.mark_modified(cid(1), ProposedEdit::AddRedaction { bounds: new_bounds });
        match &review.per_candidate[&cid(1)] {
            CandidateReviewState::Modified(edit) => match edit {
                ProposedEdit::AddRedaction { bounds } => {
                    assert_eq!(bounds.x, 10.0);
                }
                _ => panic!("expected AddRedaction"),
            },
            _ => panic!("expected Modified"),
        }
    }

    #[test]
    fn undo_reject_returns_to_pending() {
        let cands = vec![cid(1)];
        let mut review = CandidateReview::from_candidates(&cands);
        review.mark_rejected(cid(1));
        review.mark_pending(cid(1));
        assert_eq!(review.per_candidate[&cid(1)], CandidateReviewState::Pending);
    }

    #[test]
    fn apply_set_and_reject_set() {
        let cands = vec![cid(1), cid(2), cid(3)];
        let mut review = CandidateReview::from_candidates(&cands);
        review.mark_rejected(cid(2));
        let new_bounds = ImageRect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 };
        review.mark_modified(cid(3), ProposedEdit::AddRedaction { bounds: new_bounds });

        let (apply_ids, reject_ids, modified) = review.decision_sets();
        assert_eq!(apply_ids.len(), 2); // cid(1) Pending + cid(3) Modified
        assert!(apply_ids.contains(&cid(1)));
        assert!(apply_ids.contains(&cid(3)));
        assert_eq!(reject_ids, vec![cid(2)]);
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].0, cid(3));
    }

    #[test]
    fn warning_count() {
        // A warning candidate is one with confidence < threshold
        let entries = vec![
            ActivityEntry::AssistantText("ok".into()),
        ];
        assert_eq!(ActivityEntry::count_warnings(&entries), 0);
    }

    #[test]
    fn run_state_transitions() {
        let mut rs = RunState::Idle;
        assert!(rs.is_idle());
        // Cannot transition Idle → Terminal directly (Running required)
        // This is enforced by code structure, not a runtime check.
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::state`

Expected: FAIL — methods not defined.

### Step 3: Implement

```rust
// In workbench/state.rs — replace the stub CandidateReviewState and add impls

use rollshot_edit_proposal::{CandidateId, EditProposal, ProposedEdit};
use rollshot_image_document::ImageRect;
use std::collections::BTreeMap;

impl CandidateReview {
    /// Initialize from an EditProposal's candidate ids.
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

    /// Returns (apply_ids, reject_ids, modified_pairs) for building a ReviewDecision.
    /// apply_ids = Pending + Accepted + Modified candidates.
    /// reject_ids = Rejected candidates.
    /// modified = (id, replacement edit) for Modified candidates.
    pub fn decision_sets(&self) -> (Vec<CandidateId>, Vec<CandidateId>, Vec<(CandidateId, ProposedEdit)>) {
        let mut apply = Vec::new();
        let mut reject = Vec::new();
        let mut modified = Vec::new();
        for (id, state) in &self.per_candidate {
            match state {
                CandidateReviewState::Pending | CandidateReviewState::Accepted => apply.push(*id),
                CandidateReviewState::Rejected => reject.push(*id),
                CandidateReviewState::Modified(edit) => {
                    apply.push(*id);
                    modified.push((*id, edit.clone()));
                }
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
}

impl RunState {
    pub fn is_idle(&self) -> bool { matches!(self, RunState::Idle) }
    pub fn is_running(&self) -> bool { matches!(self, RunState::Running { .. }) }
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::state`

Expected: 7 tests PASS.

### Step 5: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/state.rs
git commit -m "feat(workbench): candidate review model + state types

CandidateReview with from_candidates, mark_rejected/modified/pending,
decision_sets for building ReviewDecision. RunState transitions.
ActivityEntry types for the streaming activity drawer. 7 unit tests."
```

---

## Task 4: Review → apply orchestration (pure domain)

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/review.rs`

**Interfaces:**
- Consumes: `rollshot_edit_proposal::{EditProposal, ReviewDecision, lower}`, `rollshot_image_document::{ImageDocument, EditOp}`, `CandidateReview::decision_sets()`
- Produces: `build_review_decision(proposal, review, doc_state_id) -> ReviewDecision`, `restamp_proposal(proposal, doc_state_id) -> EditProposal`, `apply_candidates(proposal, review, document) -> Result<(), String>`
- Later tasks consume: `apply_candidates` in the Apply button handler

### Step 1: Write failing tests

```rust
// crates/rollshot-app/src/result_workspace/workbench/review.rs

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_edit_proposal::*;
    use rollshot_image_document::{ImageDocument, ImageRect};

    fn test_proposal() -> EditProposal {
        EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0, // DryRunTool hardcodes 0
            candidates: vec![
                ProposedCandidate {
                    id: CandidateId(1),
                    edit: ProposedEdit::AddRedaction {
                        bounds: ImageRect { x: 10.0, y: 10.0, width: 50.0, height: 50.0 },
                    },
                    confidence: 0.9,
                    label: "test".into(),
                    rationale: None,
                    provenance: Provenance { source: ProvenanceSource::Manual },
                },
                ProposedCandidate {
                    id: CandidateId(2),
                    edit: ProposedEdit::AddRedaction {
                        bounds: ImageRect { x: 100.0, y: 100.0, width: 30.0, height: 30.0 },
                    },
                    confidence: 0.85,
                    label: "test2".into(),
                    rationale: None,
                    provenance: Provenance { source: ProvenanceSource::Manual },
                },
            ],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9, 0.85]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }
    }

    #[test]
    fn restamp_proposal_updates_base_state_id() {
        let proposal = test_proposal();
        assert_eq!(proposal.base_document_state_id, 0);
        let restamped = restamp_proposal(&proposal, 42);
        assert_eq!(restamped.base_document_state_id, 42);
        // Candidates unchanged
        assert_eq!(restamped.candidates.len(), 2);
    }

    #[test]
    fn build_review_decision_all_pending() {
        let proposal = test_proposal();
        let review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let decision = build_review_decision(&proposal, &review, 42);
        assert_eq!(decision.accepted.len(), 2); // both Pending
        assert_eq!(decision.rejected.len(), 0);
        assert_eq!(decision.modified.len(), 0);
        assert_eq!(decision.resulting_document_state_id, 42);
    }

    #[test]
    fn build_review_decision_with_reject_and_modify() {
        let proposal = test_proposal();
        let mut review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(1));
        let new_bounds = ImageRect { x: 5.0, y: 5.0, width: 100.0, height: 100.0 };
        review.mark_modified(CandidateId(2), ProposedEdit::AddRedaction { bounds: new_bounds });
        let decision = build_review_decision(&proposal, &review, 42);
        assert_eq!(decision.accepted, vec![CandidateId(2)]);
        assert_eq!(decision.rejected, vec![CandidateId(1)]);
        assert_eq!(decision.modified.len(), 1);
        assert_eq!(decision.modified[0].0, CandidateId(2));
    }

    #[test]
    fn apply_candidates_produces_edit_ops() {
        let proposal = test_proposal();
        let review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let mut doc = ImageDocument::new(
            image::RgbaImage::new(200, 200),
            "test".into(),
            0, 0, 0, 0,
        ).unwrap();
        let result = apply_candidates(&proposal, &review, &mut doc);
        assert!(result.is_ok());
        // Both candidates committed → 2 OpaqueRedaction annotations
        assert_eq!(doc.annotations().len(), 2);
    }

    #[test]
    fn apply_candidates_reject_skips() {
        let proposal = test_proposal();
        let mut review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(2));
        let mut doc = ImageDocument::new(
            image::RgbaImage::new(200, 200),
            "test".into(),
            0, 0, 0, 0,
        ).unwrap();
        apply_candidates(&proposal, &review, &mut doc).unwrap();
        assert_eq!(doc.annotations().len(), 1); // only cid(1)
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::review`

Expected: FAIL — `review.rs` doesn't exist.

### Step 3: Implement

```rust
// crates/rollshot-app/src/result_workspace/workbench/review.rs

use rollshot_edit_proposal::{
    CandidateId, EditProposal, ProposedEdit, ReviewDecision, lower,
};
use rollshot_image_document::{EditOp, ImageDocument, AnnotationId};

use super::state::{CandidateReview, CandidateReviewState};

/// Build a ReviewDecision from the proposal and the user's review state.
/// accepted = Pending + Accepted + Modified; rejected = Rejected; modified pairs.
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

/// Re-stamp a proposal's base_document_state_id from the live document.
/// The dry-run EditProposal carries hardcoded 0/1 from DryRunTool;
/// this must be corrected before lower() to match the real document.
pub fn restamp_proposal(proposal: &EditProposal, doc_state_id: u64) -> EditProposal {
    let mut p = proposal.clone();
    p.base_document_state_id = doc_state_id;
    p
}

/// Lower the proposal to EditOps via ReviewDecision, then apply as one
/// undoable transaction via ImageDocument::apply_batch.
pub fn apply_candidates(
    proposal: &EditProposal,
    review: &CandidateReview,
    document: &mut ImageDocument,
) -> Result<(), String> {
    let restamped = restamp_proposal(proposal, document.state_id());
    let decision = build_review_decision(&restamped, review, document.state_id());
    let ops = lower(&restamped, &decision);
    if ops.is_empty() {
        return Ok(()); // nothing to apply
    }
    match document.apply_batch(&ops) {
        rollshot_image_document::BatchOutcome { added_ids: _, warnings: _ } => Ok(()),
    }
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::review`

Expected: 5 tests PASS.

### Step 5: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs
git commit -m "feat(workbench): review → apply orchestration

build_review_decision, restamp_proposal (fixes DryRunTool's hardcoded 0),
apply_candidates (lower + apply_batch in one undoable transaction).
5 unit tests covering all-pending, reject+modify, apply, and reject-skip."
```

---

## Task 5: Run existing preset (headless, no agent)

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs` (create)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs` (register)

**Interfaces:**
- Consumes: `rollshot_preset::PresetStore::load_active_revision`, `rollshot_vision::{VisualIndex, RealAutomationHost}`, `rollshot_automation::{validate_source, execute_to_proposal, ExecutionPolicy, AutomationInput, ProposalContext, CancellationFlag}`, `rollshot_automation_rquickjs::QuickJsExecutor`
- Produces: `run_existing_preset(image, preset, store, policy) -> Result<EditProposal, String>`
- Called from: `Message::Workbench(WorkbenchMessage::...)` when user selects a preset to run

### Step 1: Write failing test

```rust
// crates/rollshot-app/src/result_workspace/workbench/run.rs

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_vision::TemplateStore;
    use rollshot_automation_rquickjs::QuickJsExecutor;

    // Use a programmatically generated fixture image (small solid color).
    fn test_image() -> image::RgbaImage {
        image::RgbaImage::from_fn(64, 64, |_, _| image::Rgba([200, 200, 200, 255]))
    }

    #[test]
    fn run_with_fake_host_produces_proposal() {
        let img = test_image();
        let index = VisualIndex::build(img).unwrap();
        let mut host = FakeAutomationHost::default();
        // The test automation just returns zero candidates (empty main).
        let source = r#"function main(input) { return { candidates: [] }; }"#;
        let limits = ValidationLimits::default();
        let validated = validate_source(source, &limits).unwrap();
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );
        let cancellation = CancellationFlag::default();
        let executor = QuickJsExecutor;
        let input = AutomationInput {
            image_width: 64,
            image_height: 64,
            region: None,
            annotations: vec![],
            capability_handles: Default::default(),
        };
        let ctx = ProposalContext {
            proposal_id: rollshot_edit_proposal::ProposalId(1),
            base_document_state_id: 0,
            provenance: rollshot_edit_proposal::Provenance {
                source: rollshot_edit_proposal::ProvenanceSource::Manual,
            },
        };
        let result = execute_to_proposal(
            &executor,
            &validated,
            &input,
            &ctx,
            &mut host,
            &policy,
            &cancellation,
        );
        assert!(result.is_ok());
        let (proposal, _metrics) = result.unwrap();
        assert_eq!(proposal.candidates.len(), 0);
    }
}
```

### Step 2: Run test to verify it fails

Run: `rtk cargo test -p rollshot-app -- workbench::run`

Expected: FAIL — `run.rs` doesn't exist.

### Step 3: Implement

```rust
// crates/rollshot-app/src/result_workspace/workbench/run.rs

use rollshot_automation::{
    execute_to_proposal, AutomationInput, CancellationFlag, ExecutionPolicy,
    FakeAutomationHost, ProposalContext, ValidatedAutomation,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{EditProposal, Provenance, ProvenanceSource, ProposalId};
use rollshot_preset::{Preset, PresetStore, PresetId, AutomationRevision};
use rollshot_vision::VisualIndex;

/// Run a preset's active ValidatedAutomation against the given image (no LLM).
/// Returns the dry-run EditProposal with candidates.
pub fn run_existing_preset(
    image: &image::RgbaImage,
    revision: &AutomationRevision,
    policy: &ExecutionPolicy,
) -> Result<EditProposal, String> {
    let _index = VisualIndex::build(image.clone())
        .map_err(|e| format!("vision index build: {e}"))?;

    // For headless run-existing, use FakeAutomationHost (no prepare_ needed;
    // the automation calls rollshot.templateMatch etc. which are prepared
    // if the automation uses them). In a real run, the host IS prepared;
    // for the headless path we need the real host + preparation.
    // This is handled by the full VisionContext in Task 7; for now stub.
    // The test uses FakeAutomationHost directly via execute_to_proposal.
    Err("run_existing_preset: full vision prep requires Task 7".into())
}

/// Convenience: execute_to_proposal with QuickJsExecutor + FakeAutomationHost
/// (used in tests and the headless no-vision path).
pub fn execute_dry_run(
    validated: &ValidatedAutomation,
    input: &AutomationInput,
    host: &mut dyn rollshot_automation::AutomationHost,
    policy: &ExecutionPolicy,
) -> Result<(EditProposal, rollshot_automation::ExecutionMetrics), String> {
    let executor = QuickJsExecutor;
    let cancellation = CancellationFlag::default();
    let ctx = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance { source: ProvenanceSource::Manual },
    };
    execute_to_proposal(&executor, validated, input, &ctx, host, policy, &cancellation)
        .map_err(|e| format!("dry-run: {e}"))
}
```

### Step 4: Run test to verify it passes

Run: `rtk cargo test -p rollshot-app -- workbench::run`

Expected: 1 test PASS (the `execute_dry_run` path with FakeAutomationHost).

### Step 5: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs
git commit -m "feat(workbench): headless dry-run via execute_to_proposal

execute_dry_run() wraps QuickJsExecutor + execute_to_proposal.
Stub run_existing_preset (full vision prep in Task 7). 1 unit test."
```

---

## Task 6: Canvas candidate overlay (UI — iced)

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs` (review bar stub)

**Interfaces:**
- Consumes: `WorkbenchState.pending_proposal: Option<EditProposal>`, `CandidateReview`, `CandidateReviewState`
- Produces: Candidate draw pass in `AnnotationCanvas::draw`, gesture→`WorkbenchMessage` mapping

### Step 1: Add candidate draw pass to `AnnotationCanvas::draw`

In `canvas.rs`'s `Program::draw` impl (`canvas.rs:388`), after the existing annotation draw loop and the draft annotation, add a third pass for proposed candidates when in workbench mode.

```rust
// In AnnotationCanvas::draw (canvas.rs), after drawing committed annotations
// and before drawing selection handles:

// SP6: Draw proposed candidates from the workbench pending_proposal.
// The overlay owns candidate rendering — RenderShape stays unchanged.
// Proposed: dashed border.  Selected: solid blue + handles.
// Zoom-aware: at small zoom show cluster counts; at medium show label;
// on hover/selection show confidence badge.
if let Some(proposal) = &state.pending_proposal {
    for cand in &proposal.candidates {
        let bounds = cand.edit.bounds(); // helper: extract ImageRect from ProposedEdit
        let is_selected = state.selected_candidate == Some(cand.id);
        let review_state = state.candidate_review
            .as_ref()
            .and_then(|r| r.per_candidate.get(&cand.id));

        if matches!(review_state, Some(CandidateReviewState::Rejected)) {
            continue; // skip rejected (muted in candidate list, not on canvas)
        }

        let rect = iced::Rectangle {
            x: bounds.x * scale + offset.x,
            y: bounds.y * scale + offset.y,
            width: bounds.width * scale,
            height: bounds.height * scale,
        };

        // Dashed border (proposed): draw via segmented Path lines
        // (iced canvas Stroke does not support dash patterns natively).
        // Segments: 6px on, 4px off along the rectangle perimeter.
        let border_color = if is_selected {
            iced::Color::from_rgb(0.13, 0.40, 1.0) // blue
        } else {
            iced::Color::from_rgba(1.0, 1.0, 1.0, 0.4) // white dashed
        };
        let border_style = if is_selected {
            iced::widget::canvas::Stroke::default()
                .with_color(border_color)
                .with_width(2.0)
        } else {
            iced::widget::canvas::Stroke::default()
                .with_color(border_color)
                .with_width(1.5)
        };
        // Draw dashed border via segmented line path (6px on, 4px off)
        let perimeter = 2.0 * (rect.width + rect.height);
        let dash_len = 6.0;
        let gap_len = 4.0;
        let mut path = iced::widget::canvas::Path::new();
        let mut dist = 0.0f32;
        while dist < perimeter {
            let on_end = (dist + dash_len).min(perimeter);
            let p_start = rect_point_on_perimeter(rect, dist);
            let p_end = rect_point_on_perimeter(rect, on_end);
            path = path.move_to(p_start);
            path = path.line_to(p_end);
            dist += dash_len + gap_len;
        }
        frame.stroke(&path, border_style);

        // Confidence badge (medium+ zoom)
        if scale > 0.3 {
            let label = format!("{} {:.0}%", cand.label, cand.confidence * 100.0);
            let badge_y = rect.y - 14.0;
            frame.fill_text(iced::widget::canvas::Text {
                content: label,
                position: iced::Point::new(rect.x, badge_y),
                color: iced::Color::WHITE,
                size: iced::Pixels(10.0),
                ..Default::default()
            });
        }

        // Resize handles (selected only)
        if is_selected {
            for handle in [
                iced::Point::new(rect.x, rect.y),
                iced::Point::new(rect.x + rect.width, rect.y),
                iced::Point::new(rect.x, rect.y + rect.height),
                iced::Point::new(rect.x + rect.width, rect.y + rect.height),
            ] {
                let handle_rect = iced::widget::canvas::Path::rectangle(
                    handle - iced::Vector::new(3.5, 3.5),
                    iced::Size::new(7.0, 7.0),
                );
                frame.fill(&handle_rect, iced::Color::from_rgb(0.13, 0.40, 1.0));
            }
        }
    }
}
```

### Step 2: Add gesture routing to `update.rs`

In `handle_canvas_pressed` / `handle_canvas_moved` / `handle_canvas_released`, add candidate hit-testing when in Workbench mode:

```rust
// In handle_canvas_pressed (update.rs), before existing annotation hit-test:
if let workspace::WorkspaceMode::Workbench(ref mut wb) = state.mode {
    if let Some(ref proposal) = wb.pending_proposal {
        // Hit-test proposed candidates (only when not frozen)
        if !wb.run_state.is_running() {
            let hit = hit_test_proposal_candidate(proposal, point, &wb.review);
            if let Some(candidate_id) = hit {
                wb.review.mark_accepted(candidate_id); // or toggle selection
                return Task::done(Message::Workbench(
                    workspace::WorkbenchMessage::CandidateSelected(candidate_id)
                ));
            }
        }
    }
}
```

```rust
// Helper: hit-test against proposed candidates (reuse ImageRect containment)
fn hit_test_proposal_candidate(
    proposal: &EditProposal,
    point: ImagePoint,
    _review: &CandidateReview,
) -> Option<CandidateId> {
    proposal.candidates.iter().find(|c| {
        c.edit.bounds().contains(point) // helper on ProposedEdit
    }).map(|c| c.id)
}
```

`ProposedEdit::bounds()` helper (add to `review.rs` or `state.rs`):

```rust
impl ProposedEdit {
    pub fn bounds(&self) -> Option<ImageRect> {
        match self {
            Self::AddRedaction { bounds } => Some(*bounds),
            Self::UpdateRedactionBounds { bounds, .. } => Some(*bounds),
            _ => None,
        }
    }
}
```

### Step 3: Review bar stub in `workbench/view.rs`

```rust
// crates/rollshot-app/src/result_workspace/workbench/view.rs

use iced::widget::{button, column, container, row, text};
use iced::{Length, Task};

use super::super::{Message, WorkbenchMessage};
use super::WorkbenchState;

/// Canvas review bar: Original / Before-After / Candidates toggle +
/// candidate counts + primary actions.
pub fn review_bar<'a>(wb: &'a WorkbenchState) -> iced::Element<'a, Message> {
    let proposal = wb.pending_proposal.as_ref();
    let total = proposal.map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let warnings = proposal.map_or(0, |p| p.candidates.iter()
        .filter(|c| c.confidence < 0.75)
        .count()
    );
    let apply_count = total - rejected;
    let reviewed = wb.review.rejected_count() + wb.review.modified_count();

    let bar = row![
        text(format!("Apply {apply_count} redactions, skip {rejected} rejected")),
        text(format!("{warnings} warnings included")),
        text(format!("Reviewed {reviewed} / {total}")),
        iced::widget::horizontal_space(),
        if apply_count > 0 {
            button(text(format!("Apply {apply_count} redactions")))
                .on_press(Message::Workbench(WorkbenchMessage::ApplyCandidates))
        } else {
            button(text("No candidates"))
        },
        button(text("Next warning"))
            .on_press_maybe(if warnings > 0 { Some(Message::Workbench(WorkbenchMessage::NextWarning)) } else { None }),
        button(text("Details"))
            .on_press(Message::Workbench(WorkbenchMessage::ShowAdvancedDetails)),
    ]
    .spacing(12)
    .padding(8)
    .width(Length::Fill);

    // Pending-candidate-only preview warning
    let mut col = iced::widget::column![];
    if total > 0 {
        col = col.push(
            text(format!("{total} proposed redactions are preview-only. Apply before safe copy/save."))
                .size(11)
        );
    }
    col = col.push(container(bar)
        .style(|_t| iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
            ..Default::default()
        }));

    col.into()
}
```

### Step 4: Candidate list/drawer (§5.5)

```rust
// In workbench/view.rs — add

/// Candidate list/drawer grouped by warnings → rejected → modified → manual → viewport order.
/// Each row supports jump-to-candidate, reject/undo, and rationale popover.
pub fn candidate_list<'a>(wb: &'a WorkbenchState) -> iced::Element<'a, Message> {
    use rollshot_edit_proposal::CandidateId;
    let proposal = match wb.pending_proposal.as_ref() {
        Some(p) => p,
        None => return text("No candidates").into(),
    };

    // Group candidates by category
    let mut warnings: Vec<&rollshot_edit_proposal::ProposedCandidate> = Vec::new();
    let mut rejected: Vec<&rollshot_edit_proposal::ProposedCandidate> = Vec::new();
    let mut modified: Vec<&rollshot_edit_proposal::ProposedCandidate> = Vec::new();
    let mut normal: Vec<&rollshot_edit_proposal::ProposedCandidate> = Vec::new();

    for cand in &proposal.candidates {
        let state = wb.review.per_candidate.get(&cand.id);
        match state {
            Some(CandidateReviewState::Rejected) => rejected.push(cand),
            Some(CandidateReviewState::Modified(_)) => modified.push(cand),
            _ if cand.confidence < 0.75 => warnings.push(cand),
            _ => normal.push(cand),
        }
    }

    let mut col = iced::widget::column![].spacing(4).padding(8);

    let group_label = |label: &str, count: usize| -> iced::Element<'_, Message> {
        text(format!("{label} ({count})")).size(12).into()
    };

    if !warnings.is_empty() {
        col = col.push(group_label("Warnings", warnings.len()));
        for cand in &warnings {
            col = col.push(candidate_row(cand, wb));
        }
    }
    if !rejected.is_empty() {
        col = col.push(group_label("Rejected", rejected.len()));
        for cand in &rejected {
            col = col.push(candidate_row(cand, wb));
        }
    }
    if !modified.is_empty() {
        col = col.push(group_label("Modified", modified.len()));
        for cand in &modified {
            col = col.push(candidate_row(cand, wb));
        }
    }
    for cand in &normal {
        col = col.push(candidate_row(cand, wb));
    }

    container(col).height(Length::Fill).into()
}

fn candidate_row<'a>(
    cand: &'a rollshot_edit_proposal::ProposedCandidate,
    wb: &'a WorkbenchState,
) -> iced::Element<'a, Message> {
    let is_rejected = matches!(
        wb.review.per_candidate.get(&cand.id),
        Some(CandidateReviewState::Rejected)
    );
    let row = row![
        text(format!("{} {:.0}%", cand.label, cand.confidence * 100.0)).size(11),
        iced::widget::horizontal_space(),
        button(text("Jump"))
            .on_press(Message::Workbench(WorkbenchMessage::CandidateSelected(cand.id))),
        button(text(if is_rejected { "Undo" } else { "Reject" }))
            .on_press(Message::Workbench(
                if is_rejected {
                    WorkbenchMessage::CandidateDeselected // undo reject
                } else {
                    WorkbenchMessage::CandidateDeleted(cand.id)
                }
            )),
    ].spacing(8).padding(4);

    container(row)
        .style(|_t| iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgba(
                0.1, 0.1, 0.1, if is_rejected { 0.3 } else { 0.5 }
            ))),
            border: iced::Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}
```

### Step 5: Add `ShowAdvancedDetails` and `JumpToCandidate` message variants

In `workbench/mod.rs`, add to `WorkbenchMessage`:
```rust
ShowAdvancedDetails,
JumpToCandidate(CandidateId),
```

In `update.rs` `Message::Workbench` handler:
```rust
WorkbenchMessage::ShowAdvancedDetails => {
    // toggle advanced details drawer visibility
    Task::none()
}
WorkbenchMessage::JumpToCandidate(id) => {
    // scroll canvas viewport to candidate bounds
    // (viewport::scroll_to_point)
    Task::none()
}
```

### Step 6: "Add missing candidate" gesture

In the canvas gesture routing (Task 6 `handle_canvas_pressed`), when no candidate hit and no annotation hit in workbench mode, create a new "missing candidate" evidence item:

```rust
// In handle_canvas_pressed, after annotation hit-test misses:
if let WorkspaceMode::Workbench(ref mut wb) = state.mode {
    if !wb.run_state.is_running() {
        // No hit on any candidate or annotation → "Add missing candidate"
        let new_id = CandidateId(wb.next_candidate_id);
        wb.next_candidate_id += 1;
        wb.review.mark_modified(new_id, ProposedEdit::AddRedaction {
            bounds: ImageRect { x: point.x - 25.0, y: point.y - 25.0, width: 50.0, height: 50.0 },
        });
        // Also add to pending_proposal as a new candidate for rendering
        if let Some(ref mut proposal) = wb.pending_proposal {
            proposal.candidates.push(ProposedCandidate {
                id: new_id,
                edit: ProposedEdit::AddRedaction {
                    bounds: ImageRect { x: point.x - 25.0, y: point.y - 25.0, width: 50.0, height: 50.0 },
                },
                confidence: 1.0, // manually added = certain
                label: "manual".into(),
                rationale: Some("Manually added missing candidate".into()),
                provenance: Provenance { source: ProvenanceSource::Manual },
            });
        }
    }
}
```

This creates a new `CandidateReviewState::Modified` for a manually-added evidence item, not an immediate committed annotation (§5.3).

### Step 7: Manual verification

- Enter workbench mode (Smart Redaction button)
- No candidates yet → review bar shows "No candidates"
- Compile + fmt + clippy pass

### Step 5: Commit

```bash
git add crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/workbench/review.rs
git commit -m "feat(workbench): canvas candidate overlay + review bar

Third draw pass in AnnotationCanvas::draw for proposed candidates:
dashed border, confidence badge, selected+handles. Gesture routing for
candidate hit-test on canvas press. Review bar with candidate count +
Apply N redactions button."
```

---

## Task 7: Agent run architecture + streaming activity drawer

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs` (event→entry mapping)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs` (activity drawer)

**Interfaces:**
- Consumes: `AgentRunner::run_with_provider`, `RunEvent`, `ToolContext`, `DryRunTool`, `ProviderConfig`
- Produces: `start_agent_run(config, provider_cfg, image_dims, session_id, user_message, ...) -> Task<Message>`, `RunEvent → ActivityEntry` mapping, `Task::run` channel bridge pattern

### Step 1: Event→ActivityEntry mapping (pure, testable)

```rust
// crates/rollshot-app/src/result_workspace/workbench/state.rs — add

use rollshot_agent::runtime::RunEvent;

/// Map a RunEvent to an ActivityEntry for the live activity drawer.
/// Reconstructs the conversation from the event stream (AgentSession
/// only stores finished user/assistant prose, not tool calls — §10.3).
pub fn event_to_activity_entry(event: &RunEvent) -> Option<ActivityEntry> {
    match event {
        RunEvent::TextChunk { text } => Some(ActivityEntry::AssistantText(text.clone())),
        RunEvent::ToolCallStart { name } => Some(ActivityEntry::ToolCard {
            name: name.clone(),
            status: ToolCardStatus::Running,
            summary: String::new(),
        }),
        RunEvent::ToolCallEnd { name, success } => Some(ActivityEntry::ToolCard {
            name: name.clone(),
            status: if *success { ToolCardStatus::Success } else { ToolCardStatus::Failed },
            summary: String::new(), // summary populated by tool-specific mapping
        }),
        RunEvent::TurnComplete => None, // driver never emits this (§10.8)
    }
}

/// Compute a human-readable label for a terminal state.
pub fn terminal_state_label(state: &rollshot_agent::driver::RunTerminalState) -> String {
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

Test:

```rust
#[test]
fn event_mapping_text_chunk() {
    let entry = event_to_activity_entry(&RunEvent::TextChunk { text: "hello".into() });
    match entry {
        Some(ActivityEntry::AssistantText(t)) => assert_eq!(t, "hello"),
        _ => panic!("expected AssistantText"),
    }
}

#[test]
fn event_mapping_tool_call() {
    let entry = event_to_activity_entry(&RunEvent::ToolCallStart { name: "inspect_ocr".into() });
    match entry {
        Some(ActivityEntry::ToolCard { name, status, .. }) => {
            assert_eq!(name, "inspect_ocr");
            assert!(matches!(status, ToolCardStatus::Running));
        }
        _ => panic!("expected ToolCard"),
    }
}

#[test]
fn terminal_labels() {
    assert_eq!(terminal_state_label(&RunTerminalState::Cancelled), "Run cancelled");
    assert_eq!(
        terminal_state_label(&RunTerminalState::BudgetExhausted { dimension: BudgetDimension::WallTime }),
        "Budget exhausted: WallTime"
    );
}
```

### Step 2: Agent run channel bridge

```rust
// crates/rollshot-app/src/result_workspace/workbench/run.rs — add

use rollshot_agent::{
    driver::{AgentRunner, AgentConfig, RunTerminalState, ReadyForReview},
    domain::{AgentSession, SessionId, AuthorizedModelInput, MediaType, AttachmentDescriptor},
    runtime::{
        RunBudget, RunCancellation, RunEvent, RunEventSink, NullEventSink,
    },
    tools::{
        ToolContext, ToolRegistry, ToolRegistryLimits,
        ReplaceSourceTool, ValidateSourceTool, DryRunTool,
        SubmitForReviewTool, RequestUserInputTool, GetContextSummaryTool,
    },
    provider::{AnthropicAdapter, OpenAIAdapter, ProviderAdapter},
};
use rollshot_automation::{
    ExecutionPolicy, ValidationLimits, CancellationFlag,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_vision::{VisualIndex, RealAutomationHost};
use std::sync::{Arc, Mutex};

use super::super::Message;
use super::super::WorkbenchMessage;

struct ChannelEventSink {
    tx: tokio::sync::mpsc::Sender<RunEvent>,
}

impl RunEventSink for ChannelEventSink {
    fn emit(&self, event: RunEvent) {
        // try_send — drop events if the channel is full (UI lag)
        let _ = self.tx.try_send(event);
    }
}

/// Build the standard finite RunBudget for Smart Redaction runs.
/// RunBudget has no ergonomic constructor (only unlimited() — §10.4).
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
        affected_area: 1.0,
    }
}

/// Prepare a VisionContext from the capture image.
/// Builds VisualIndex + RealAutomationHost, prepares template/region/ocr.
pub fn prepare_vision_context(
    image: &image::RgbaImage,
) -> Result<super::VisionContext, String> {
    let index = VisualIndex::build(image.clone())
        .map_err(|e| format!("VisualIndex build: {e}"))?;
    let mut host = RealAutomationHost::new();
    // Prepare template_match for full-image queries (capabilities may be
    // needed by the automation). Actual preparation is keyed on the
    // automation's capability manifest; for now prepare nothing (the
    // automation will request what it needs and get capability_unavailable
    // if not prepared — the dry-run handles that gracefully).
    let cancellation = CancellationFlag::default();
    Ok(super::VisionContext {
        index,
        host: Arc::new(Mutex::new(host)),
        executor: QuickJsExecutor,
        cancellation,
    })
}

/// Start a bounded agent run as an iced Task with streaming RunEvents.
/// Returns a Task that emits Message::Workbench(WorkbenchMessage::RunEvent(...))
/// and a final Message::Workbench(WorkbenchMessage::RunTerminal(...)).
pub fn start_agent_run(
    provider_cfg: &super::provider_config::ProviderConfig,
    user_message: String,
    session_id: u64,
    image_dims: (u32, u32),
    active_revision_source: Option<&str>,
    vision_ctx: super::VisionContext,
) -> Result<Task<Message>, String> {
    use super::provider_config::{resolve_key, has_key};
    if !has_key(provider_cfg) {
        return Err("no provider key configured".into());
    }
    let api_key = resolve_key(&provider_cfg.key_source)
        .ok_or("key resolution failed")?;

    let adapter: Box<dyn ProviderAdapter> = match provider_cfg.provider {
        super::provider_config::ProviderKind::Anthropic => Box::new(
            AnthropicAdapter::new(&api_key, provider_cfg.base_url.as_deref().unwrap_or("https://api.anthropic.com"))
                .map_err(|e| format!("adapter: {e}"))?
        ),
        super::provider_config::ProviderKind::OpenAI => Box::new(
            OpenAIAdapter::new(&api_key, provider_cfg.base_url.as_deref().unwrap_or("https://api.openai.com/v1"))
                .map_err(|e| format!("adapter: {e}"))?
        ),
    };

    let initial_source = active_revision_source.unwrap_or("").to_string();
    let runner = AgentRunner::new(AgentConfig::default());
    let budget = smart_redaction_budget();
    let cancellation = RunCancellation::new();

    // Build tool context
    let validation_limits = ValidationLimits::default();
    let policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(25),
        80_000_000,
        8_000_000,
    );
    let tool_ctx = Arc::new(ToolContext::new(
        SessionId::new(session_id),
        initial_source.clone(),
        validation_limits.clone(),
        policy.clone(),
        image_dims,
        &cancellation,
    ));

    // Register tools
    let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
    registry.register(Arc::new(ReplaceSourceTool::new(tool_ctx.clone())))
        .map_err(|e| format!("register replace_source: {e}"))?;
    registry.register(Arc::new(ValidateSourceTool::new(tool_ctx.clone())))
        .map_err(|e| format!("register validate: {e}"))?;
    registry.register(Arc::new(SubmitForReviewTool::new(tool_ctx.clone())))
        .map_err(|e| format!("register submit: {e}"))?;
    registry.register(Arc::new(RequestUserInputTool::new(tool_ctx.clone())))
        .map_err(|e| format!("register request_input: {e}"))?;
    registry.register(Arc::new(GetContextSummaryTool::new(tool_ctx.clone())))
        .map_err(|e| format!("register context_summary: {e}"))?;
    // DryRunTool needs executor + host from VisionContext
    registry.register(Arc::new(DryRunTool::new(
        tool_ctx.clone(),
        Arc::new(vision_ctx.executor.clone()), // QuickJsExecutor is zero-size
        vision_ctx.host.clone(),
    ))).map_err(|e| format!("register dry_run: {e}"))?;

    // Build AuthorizedModelInput
    let descriptor = AttachmentDescriptor {
        media_type: MediaType::Png,
        width: image_dims.0,
        height: image_dims.1,
        byte_count: (image_dims.0 as u64) * (image_dims.1 as u64) * 4,
    };
    let model_input = AuthorizedModelInput::new(
        provider_cfg.provider.to_string().to_lowercase(),
        provider_cfg.model.clone(),
        user_message,
        vec![descriptor],
        vec![], // attachment bytes populated by the caller for full-screenshot mode
    ).map_err(|e| format!("model input: {e}"))?;

    // Channel bridge: mpsc → Task::run
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RunEvent>(64);
    let sink = ChannelEventSink { tx };
    let cancellation_for_task = cancellation.clone();
    let tool_ctx_for_task = tool_ctx.clone();
    let adapter_ref: &dyn ProviderAdapter = &*adapter;

    // Spawn the async agent run.  Task::run streams from the channel.
    // The agent loop runs until a terminal state; then we send RunTerminal.
    let session = AgentSession::new(SessionId::new(session_id));

    // Use Task::perform to run the agent, then Task::run to stream events.
    // Since run_with_provider is the single async call, we wrap it.
    // The channel bridge: receiver stream → Task::run → messages.
    let stream = async_stream::stream! {
        // Run the agent (async, produces a terminal state)
        let terminal = runner.run_with_provider(
            model_input,
            &mut session,
            &registry,
            budget,
            &cancellation_for_task,
            &sink,
            &tool_ctx_for_task,
            adapter_ref,
        ).await;

        // Drain any remaining events
        drop(sink); // close sender → stream ends
        while let Some(event) = rx.recv().await {
            yield event;
        }
        // Emit terminal as a final message
    };

    // This is a simplified sketch — the actual implementation must handle
    // the stream → message mapping.  Use Task::run(stream, |event| ...)
    // where event is RunEvent mapped to WorkbenchMessage::RunEvent.
    // The terminal state arrives via a special RunTerminal message sent
    // after the run completes.
    Err("full agent run wiring requires async_stream; implement in step below".into())
}
```

### Step 3: Add `async_stream` dependency + implement stream bridge

```toml
# crates/rollshot-app/Cargo.toml
async-stream = "0.3"
```

Refine the `start_agent_run` return to use `Task::run(stream, |event| Message::Workbench(WorkbenchMessage::RunEvent(event)))`:

```rust
pub fn start_agent_run(...) -> Task<Message> {
    // ... build adapter, tools, registry, tool_ctx, model_input as above ...

    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<RunEvent>(64);
    let sink = ChannelEventSink { tx: event_tx };

    // Spawn the run in a background task (tokio::spawn via iced's runtime)
    let session_mut = Arc::new(Mutex::new(session));
    let run_handle = tokio::spawn(async move {
        let mut session = session_mut.lock().unwrap();
        runner.run_with_provider(
            model_input, &mut session, &registry, budget,
            &cancellation, &sink, &tool_ctx, &*adapter,
        ).await
    });

    // Stream: map channel events → WorkbenchMessage::RunEvent,
    // then append RunTerminal when the run completes.
    let stream = async_stream::stream! {
        let mut rx = event_rx;
        while let Some(event) = rx.recv().await {
            yield Message::Workbench(WorkbenchMessage::RunEvent(event));
        }
        // Channel closed → run finished. Get terminal state.
        if let Ok(terminal) = run_handle.await {
            yield Message::Workbench(WorkbenchMessage::RunTerminal(terminal));
        }
    };

    Task::run(stream, std::convert::identity)
}
```

### Step 4: Activity drawer view

```rust
// In workbench/view.rs — add

pub fn activity_drawer<'a>(wb: &'a WorkbenchState) -> iced::Element<'a, Message> {
    let mut col = iced::widget::column![].spacing(6).padding(8).width(Length::Fill);

    for entry in &wb.live_activity {
        match entry {
            ActivityEntry::UserMessage(text) => {
                col = col.push(
                    container(text(text.as_str()))
                        .padding(6)
                        .style(|_t| iced::widget::container::Style {
                            background: Some(iced::Background::Color(iced::Color::from_rgba(0.2, 0.4, 0.8, 0.3))),
                            border: iced::Border { radius: 6.0.into(), ..Default::default() },
                            ..Default::default()
                        })
                );
            }
            ActivityEntry::AssistantText(text) => {
                col = col.push(text(text.as_str()));
            }
            ActivityEntry::ToolCard { name, status, summary } => {
                let icon = match status {
                    ToolCardStatus::Running => "...",
                    ToolCardStatus::Success => "OK",
                    ToolCardStatus::Failed => "ERR",
                };
                col = col.push(
                    container(row![text(icon), text(name.as_str()), text(summary.as_str())].spacing(6))
                        .padding(4)
                        .style(|_t| iced::widget::container::Style {
                            background: Some(iced::Background::Color(iced::Color::from_rgba(0.15, 0.15, 0.15, 0.5))),
                            border: iced::Border { radius: 4.0.into(), ..Default::default() },
                            ..Default::default()
                        })
                );
            }
            ActivityEntry::RunStatus { turn, budget_summary, elapsed } => {
                col = col.push(
                    text(format!("Turn {turn} · {budget_summary} · {:.1}s", elapsed.as_secs_f32()))
                        .size(11)
                );
            }
            ActivityEntry::TerminalLabel(label) => {
                col = col.push(text(label.as_str()).size(12));
            }
        }
    }

    // Run status header
    if let RunState::Running { .. } = &wb.run_state {
        col = col.push(iced::widget::horizontal_rule(1));
        col = col.push(
            button("Cancel").on_press(Message::Workbench(WorkbenchMessage::CancelRun))
        );
    }

    container(col).height(Length::Fill).into()
}
```

### Step 5: Wire Message::Workbench handlers

In `update.rs` `update_inner()`, implement the `Message::Workbench(msg)` arm:

```rust
Message::Workbench(msg) => {
    let workbench = match &mut state.mode {
        WorkspaceMode::Workbench(wb) => wb,
        _ => return Task::none(),
    };
    match msg {
        WorkbenchMessage::RunEvent(event) => {
            if let Some(entry) = event_to_activity_entry(&event) {
                workbench.live_activity.push(entry);
            }
            Task::none()
        }
        WorkbenchMessage::RunTerminal(terminal) => {
            workbench.live_activity.push(ActivityEntry::TerminalLabel(
                terminal_state_label(&terminal)
            ));
            workbench.run_state = RunState::Terminal(terminal);
            // Populate pending_proposal from ReadyForReview if applicable
            if let RunState::Terminal(RunTerminalState::ReadyForReview(ref ready)) = workbench.run_state {
                workbench.pending_proposal = Some(ready.proposal.clone());
                workbench.review = CandidateReview::from_candidates(
                    &ready.proposal.candidates.iter().map(|c| c.id).collect::<Vec<_>>()
                );
            }
            Task::none()
        }
        WorkbenchMessage::CancelRun => {
            if let RunState::Running { ref cancellation, .. } = workbench.run_state {
                cancellation.cancel();
            }
            Task::none()
        }
        WorkbenchMessage::ApplyCandidates => {
            if let Some(ref proposal) = workbench.pending_proposal {
                if let Err(e) = review::apply_candidates(proposal, &workbench.review, &mut state.document.image) {
                    workbench.error = Some(e);
                } else {
                    workbench.pending_proposal = None;
                    workbench.review = CandidateReview::default();
                }
            }
            Task::none()
        }
        WorkbenchMessage::CandidateSelected(id) => {
            // update selection state for highlight
            Task::none()
        }
        WorkbenchMessage::CandidateDeselected => { Task::none() }
        WorkbenchMessage::CandidateDeleted(id) => {
            workbench.review.mark_rejected(id);
            Task::none()
        }
        WorkbenchMessage::CandidateMoved { id, new_bounds } => {
            workbench.review.mark_modified(id, rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds: new_bounds });
            Task::none()
        }
        WorkbenchMessage::NextWarning => { Task::none() } // jump to next warning
        WorkbenchMessage::DisclosureConfirmed => { Task::none() }
        WorkbenchMessage::DisclosureCancelled => { Task::none() }
        WorkbenchMessage::SavePresetOrRevision => { Task::none() }
        WorkbenchMessage::AskAgentToRevise => { Task::none() }
        WorkbenchMessage::DiscardDraft => { Task::none() }
        WorkbenchMessage::DiscardCandidates => { Task::none() }
        WorkbenchMessage::ImStart => { Task::none() }
    }
}
```

### Step 6: Wire subscription for the streaming events

The channel bridge emits events via `Task::run`, which iced delivers as `Message::Workbench(...)` through the normal `update` loop. No separate `subscription` needed — `Task::run` handles it. Add the start-run handler:

```rust
// In Message::SmartRedaction arm (update.rs), after setting workbench mode:
// If user selects "Create new preset" from the picker → start agent run.
// For now, the picker is a future UI; SmartRedaction switches mode.
// The actual run starts when the user sends from the prompt composer.
```

### Step 7: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/Cargo.toml
git commit -m "feat(workbench): agent run via Task::run channel bridge + activity drawer

start_agent_run spawns run_with_provider in tokio::spawn, streams RunEvents
via mpsc channel through Task::run(stream, identity). Activity drawer renders
streaming text + tool cards + cancel button. RunTerminal populates
pending_proposal + candidate review. ApplyCandidates wired."
```

---

## Task 8: Upload disclosure modal

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (disclosure flow)

### Step 1: Disclosure modal view

```rust
// In workbench/view.rs — add

pub fn disclosure_modal<'a>(
    provider_cfg: &'a ProviderConfig,
    image_dims: (u32, u32),
) -> iced::Element<'a, Message> {
    let label = provider_config::provider_model_label(provider_cfg);
    let img_mb = (image_dims.0 as f64 * image_dims.1 as f64 * 4.0) / (1024.0 * 1024.0);

    let content = iced::widget::column![
        text(format!("Send to {label}")).size(16),
        text("This run will send:").size(13),
        text(format!("  ✓ Screenshot image, {:.1} MB", img_mb)),
        text("  ✓ Local OCR/layout summary"),
        text("  — Selected region: none"),
        text("  — Selected annotations: none"),
        iced::widget::vertical_space().height(12),
        text("Privacy mode:").size(13),
        iced::widget::radio("Full screenshot — best accuracy", true, Some(true), |selected| {
            // mode selection toggled (selected is the new bool)
            Message::Workbench(WorkbenchMessage::DisclosureConfirmed)
        }),
        iced::widget::radio("OCR/layout only — no image upload", false, Some(true), |selected| {
            Message::Workbench(WorkbenchMessage::DisclosureConfirmed)
        }),
        iced::widget::vertical_space().height(12),
        row![
            button(text(format!("Send to {}", provider_cfg.provider)))
                .on_press(Message::Workbench(WorkbenchMessage::DisclosureConfirmed)),
            button(text("Cancel"))
                .on_press(Message::Workbench(WorkbenchMessage::DisclosureCancelled)),
        ].spacing(12),
    ].spacing(8).padding(24).max_width(450);

    // Layer over workbench with scrim (existing modal pattern: view.rs:342/389)
    iced::widget::stack![
        // Base layer (empty — the workbench is behind the scrim)
        iced::widget::text(""),
        container(content)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_t| iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
                ..Default::default()
            })
    ].into()
}
```

### Step 2: Disclosure flow in update.rs

```rust
WorkbenchMessage::DisclosureConfirmed => {
    workbench.disclosure_pending = false;
    // Start the agent run now that disclosure is confirmed
    match run::start_agent_run(
        &provider_config,
        user_message,
        session_id,
        image_dims,
        active_revision_source,
        vision_ctx,
    ) {
        Ok(task) => {
            workbench.run_state = RunState::Running {
                cancellation: RunCancellation::new(),
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
WorkbenchMessage::DisclosureCancelled => {
    workbench.disclosure_pending = false;
    Task::none()
}
```

### Step 3: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/update.rs
git commit -m "feat(workbench): upload disclosure modal

Per-run disclosure with provider/model, payload mode, image size.
DisclosureConfirmed starts the agent run; DisclosureCancelled
returns to composer. Uses existing scrim+modal stack pattern."
```

---

## Task 9: Automation review drawer + save revision

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs` (save orchestration)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

### Step 1: Save revision orchestration (TDD)

```rust
// In review.rs — add

use rollshot_preset::{PresetStore, PresetId, RevisionId, RevisionProvenance, RevisionOrigin};

/// Save a validated automation as a new preset or revision.
pub fn save_revision(
    store: &PresetStore,
    preset_id: &PresetId,
    source: &str,
    parent_rev_id: Option<&RevisionId>,
    session_id: u64,
) -> Result<(), String> {
    let limits = ValidationLimits::default();
    let validated = rollshot_automation::validate_source(source, &limits)
        .map_err(|diags| format!("validation failed: {} diagnostics", diags.len()))?;

    let rev_id = RevisionId(format!("rev-{}", chrono::Utc::now().timestamp_millis()));
    let provenance = RevisionProvenance {
        origin: RevisionOrigin::AgentRun,
        note: None,
        source_run_ref: Some(session_id.to_string()),
    };
    store.add_revision(preset_id, rev_id.clone(), parent_rev_id.cloned(), validated, provenance, chrono::Utc::now().to_rfc3339())
        .map_err(|e| format!("save revision: {e}"))?;
    store.set_active_revision(preset_id, &rev_id, chrono::Utc::now().to_rfc3339())
        .map_err(|e| format!("set active: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod save_tests {
    use super::*;

    #[test]
    fn save_revision_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = PresetStore::open(tmp.path().to_path_buf());
        let preset_id = PresetId("test-preset".into());
        store.create_preset(preset_id.clone(), "Test".into(), "test intent".into(), "2026-01-01T00:00:00Z".into()).unwrap();

        let source = r#"function main(input) { return { candidates: [] }; }"#;
        save_revision(&store, &preset_id, source, None, 42).unwrap();

        let active = store.load_active_revision(&preset_id).unwrap();
        assert!(active.artifact.source.contains("function main"));
    }
}
```

### Step 2: Review drawer view

```rust
// In workbench/view.rs — add

pub fn review_drawer<'a>(wb: &'a WorkbenchState) -> iced::Element<'a, Message> {
    let proposal = wb.pending_proposal.as_ref();
    let total = proposal.map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let warnings = proposal.map_or(0, |p| p.candidates.iter()
        .filter(|c| c.confidence < 0.75)
        .count()
    );

    let mut col = iced::widget::column![].spacing(8).padding(12).width(Length::Fill);

    // Preset draft summary
    if let Some(ref draft) = wb.pending_draft {
        col = col.push(text("Preset draft").size(14));
        col = col.push(text(&draft.assistant_text).size(12));
        col = col.push(iced::widget::vertical_space().height(8));
    }

    // Current screenshot summary
    col = col.push(text(format!("{total} candidates, {warnings} warnings")).size(13));
    col = col.push(text(format!("{} rejected", rejected)).size(12));

    // Two cards: This screenshot / Reusable preset
    col = col.push(iced::widget::vertical_space().height(12));

    // This screenshot card
    let apply_count = total - rejected;
    col = col.push(container(
        iced::widget::column![
            text("This screenshot").size(13),
            text(format!("{apply_count} proposed redactions")),
            button(text(format!("Apply {apply_count} redactions")))
                .on_press(Message::Workbench(WorkbenchMessage::ApplyCandidates)),
            button(text("Discard candidates"))
                .on_press(Message::Workbench(WorkbenchMessage::DiscardCandidates)),
        ].spacing(4).padding(8)
    ).style(|_t| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(0.1, 0.2, 0.3, 0.3))),
        border: iced::Border { radius: 6.0.into(), width: 1.0, color: iced::Color::from_rgba(0.5, 0.5, 0.5, 0.3) },
        ..Default::default()
    }));

    // Reusable preset card
    col = col.push(container(
        iced::widget::column![
            text("Reusable preset").size(13),
            text("New detector draft is ready"),
            button(text("Save preset"))
                .on_press(Message::Workbench(WorkbenchMessage::SavePresetOrRevision)),
            button(text("Ask agent to revise"))
                .on_press(Message::Workbench(WorkbenchMessage::AskAgentToRevise)),
            button(text("Discard draft"))
                .on_press(Message::Workbench(WorkbenchMessage::DiscardDraft)),
        ].spacing(4).padding(8)
    ).style(|_t| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(0.1, 0.15, 0.1, 0.3))),
        border: iced::Border { radius: 6.0.into(), width: 1.0, color: iced::Color::from_rgba(0.5, 0.5, 0.5, 0.3) },
        ..Default::default()
    }));

    // Advanced details tab (source diff, IR summary)
    col = col.push(iced::widget::vertical_space().height(12));
    col = col.push(text("Advanced details").size(12));
    if let Some(ref draft) = wb.pending_draft {
        col = col.push(
            container(text(format!("Source: {} bytes, {} AST nodes",
                draft.validation_summary.source_bytes,
                draft.validation_summary.ast_nodes
            )).size(11))
                .padding(4)
        );
    }

    container(col).height(Length::Fill).into()
}
```

### Step 3: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/update.rs
git commit -m "feat(workbench): review drawer + save revision

Review drawer with preset draft summary, current screenshot summary,
two action cards (this screenshot / reusable preset), and advanced
details (source bytes, AST nodes). save_revision orchestration:
validate_source → add_revision → set_active_revision."
```

---

## Task 10: Improve Preset flow

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs` (Improve modal)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (Improve handler)

### Step 1: Correction evidence assembly (TDD)

```rust
// In review.rs — add

/// Assemble correction evidence for Improve Preset.
/// Returns a summary string and the evidence to include in the next run.
pub fn assemble_correction_evidence(
    proposal: &EditProposal,
    review: &CandidateReview,
) -> CorrectionEvidence {
    let (_, rejected_ids, modified_pairs) = review.decision_sets();
    let rejected_candidates: Vec<_> = proposal.candidates.iter()
        .filter(|c| rejected_ids.contains(&c.id))
        .collect();
    CorrectionEvidence {
        rejected_count: rejected_candidates.len(),
        modified_count: modified_pairs.len(),
        added_count: 0, // added candidates counted from canvas gestures
    }
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
```

### Step 2: Improve modal view

```rust
// In workbench/view.rs — add

pub fn improve_modal<'a>(evidence: &CorrectionEvidence) -> iced::Element<'a, Message> {
    let content = iced::widget::column![
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
    ].spacing(8).padding(24).max_width(450);

    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_t| iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
            ..Default::default()
        })
        .into()
}
```

### Step 3: Improve context gate

```rust
// In update.rs — WorkbenchMessage::ImStart handler:
WorkbenchMessage::ImStart => {
    // Only available when there IS a review/correction context
    // (prior run + some rejected/modified/added candidates)
    if workbench.pending_proposal.is_some() && !workbench.review.is_empty() {
        workbench.disclosure_pending = true; // opens improve flow
    }
    Task::none()
}
```

### Step 4: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/update.rs
git commit -m "feat(workbench): Improve Preset correction-evidence flow

CorrectionEvidence assembled from rejected/modified/added candidates.
Improve modal shows evidence summary with explicit include checkbox.
Context-gated: only available when prior run + review exist."
```

---

## Task 11: Copy/Save gating + product result states

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs` (gating helper)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (gating in Copy/Save handlers)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs` (result states)

### Step 1: Pending-candidate gating (TDD)

```rust
// In state.rs — add

/// Whether pending (unapplied) candidates exist.  Copy/Save must warn or block.
pub fn has_pending_candidates(wb: &WorkbenchState) -> bool {
    wb.pending_proposal.is_some() && !wb.review.is_empty()
}

/// Apply-skip summary for the review bar.
pub fn apply_skip_summary(wb: &WorkbenchState) -> String {
    let total = wb.pending_proposal.as_ref().map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let apply = total - rejected;
    let warnings = wb.pending_proposal.as_ref().map_or(0, |p|
        p.candidates.iter().filter(|c| c.confidence < 0.75).count()
    );
    format!("Apply {apply} redactions, skip {rejected} rejected\n{warnings} warnings included")
}
```

### Step 2: Gate Copy/Save in update.rs

In the existing `Message::Copy` and `Message::SaveAs` handlers, add:

```rust
Message::Copy => {
    if let WorkspaceMode::Workbench(ref wb) = state.mode {
        if has_pending_candidates(wb) {
            state.message = Some(InlineMessage::Error(format!(
                "{}\nApply them before safe copy/save.",
                apply_skip_summary(wb)
            )));
            return Task::none();
        }
    }
    // ... existing copy logic ...
}
```

### Step 3: Product result states view

```rust
// In workbench/view.rs — add

pub fn result_state_banner<'a>(wb: &'a WorkbenchState) -> Option<iced::Element<'a, Message>> {
    let proposal = wb.pending_proposal.as_ref()?;
    let total = proposal.candidates.len();

    if total == 0 {
        return Some(container(
            iced::widget::column![
                text("This preset did not find anything on this screenshot."),
                row![
                    button(text("Improve preset"))
                        .on_press(Message::Workbench(WorkbenchMessage::ImStart)),
                    button(text("Manual redact"))
                        .on_press(Message::SelectTool(super::super::canvas::Tool::Redact)),
                ].spacing(8),
            ].spacing(8).padding(12)
        ).into());
    }

    let warnings = proposal.candidates.iter().filter(|c| c.confidence < 0.75).count();
    if warnings == total {
        return Some(container(
            iced::widget::column![
                text("Only low-confidence matches were found."),
                row![
                    button(text("Review warnings"))
                        .on_press(Message::Workbench(WorkbenchMessage::NextWarning)),
                    button(text("Improve preset"))
                        .on_press(Message::Workbench(WorkbenchMessage::ImStart)),
                    button(text("Discard"))
                        .on_press(Message::Workbench(WorkbenchMessage::DiscardCandidates)),
                ].spacing(8),
            ].spacing(8).padding(12)
        ).into());
    }

    Some(container(
        text(format!("{total} candidates found. Review before applying."))
            .padding(12)
    ).into())
}
```

### Step 4: Commit

```bash
git add crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/update.rs
git commit -m "feat(workbench): Copy/Save gating + product result states

Pending candidates warn or block Copy/Save (preview ≠ safe redactions).
Product result banners: no-match, low-confidence-only, candidates-found.
apply_skip_summary for the review bar and warning messages."
```

---

## Task 12: Platform verification + handoff

**Files:**
- Modify: `crates/rollshot-app/src/macos_product.rs` (Phase::Workbench forwarding)
- Verify: `crates/rollshot-app/src/result_workspace/mod.rs` (update/view/subscription triplet)
- Create: `docs/superpowers/handoffs/2026-06-25-preset-workbench.md`

### Step 1: macOS Phase forwarding

In `macos_product.rs`, add `Message::Workspace(result_workspace::Message::Workbench(msg))` forwarding through the existing `Phase::Workspace` arm (mirrors how `Message::Workspace(msg)` is already forwarded at `macos_product.rs:344-348`).

Verify that `result_workspace::update::subscription()` returns the event-stream subscription when in Workbench mode (the streaming activity drawer needs it).

### Step 2: Full platform verification checklist

Run on Linux:
```
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk cargo test --workspace --exclude rollshot-ocr
```

Manual verification (Linux iced::application):
- [ ] Smart Redaction toolbar button opens workbench mode
- [ ] Disclosure modal shows correct provider/model/attachment info
- [ ] Run-existing (no agent) produces candidates on the canvas
- [ ] Agent run streams text + tool cards in activity drawer
- [ ] Cancel works during agent run
- [ ] ReadyForReview fills review drawer + canvas candidates
- [ ] Candidate select/move/resize/delete works
- [ ] Apply candidates → committed redactions → safe copy/save activates
- [ ] Save preset → PresetStore → loads as active revision
- [ ] Copy/Save blocked while pending candidates exist
- [ ] No-match / low-confidence banners with correct actions
- [ ] No OCR text / tool args / provider bodies in tracing events
- [ ] Tall stitched image performance (1080×20000 region, candidates cull)

Manual verification (macOS iced::daemon Phase::Workspace):
- [ ] Same checklist above through macOS Phase forwarding

### Step 3: Write handoff

```bash
git add docs/superpowers/handoffs/2026-06-25-preset-workbench.md
git commit -m "docs: Preset Workbench handoff (SP6)

Platform verification completed on Linux iced::application.
macOS Phase forwarding verified. Known limitations documented."
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
# OCR lane (OCR + vision integration tests)
rtk cargo clippy -p rollshot-ocr -p rollshot-vision --features rollshot-vision/ocr --all-targets -- -D warnings
rtk cargo test -p rollshot-ocr
rtk cargo test -p rollshot-vision --features ocr
```
