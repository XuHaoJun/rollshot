# Preset Workbench Design (Sub-project 6)

**Date:** 2026-06-25
**Status:** Approved design
**Parent design:** `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`

This subproject (SP6) delivers the first-release Smart Redaction Preset
Workbench: the visual agent experience that turns the SP1–SP5 foundation
(`rollshot-vision`, `rollshot-agent`, `rollshot-preset`,
`rollshot-edit-proposal`, `rollshot-automation`) into a product surface the
user drives from the post-capture Result Workspace.

It covers the parent spec's first-release scope (§3.1) and success criteria
(§14): create preset, run existing preset, review/apply candidates, explicit
Improve Preset, upload disclosure, and provider controls. Session/run
persistence and resume are deferred (§2.2).

## 1. Summary

The Workbench is a **mode of the existing Result Workspace**, not a separate
application. A toolbar entry opens a preset picker; "Run existing" is a
lightweight candidate-review mode (no agent, no upload), while "Create" /
"Improve Preset" swap the workspace body to a three-pane workbench (agent
session + canvas-with-candidates + automation review) operating on the same
`ImageDocument` instance. Accepted candidates become `OpaqueRedaction`
annotations via the existing `lower()` → `ImageDocument::apply_batch` path;
safe copy/save activates automatically. Accepted automation revisions persist
via `PresetStore` (SP5).

The bounded agent runs as an `iced::Task` that spawns `run_with_provider` on
the Tokio runtime and streams `RunEvent`s back through a channel exposed as an
iced `Subscription`. The canvas stays interactive (pan/zoom) during a run;
in-progress candidate editing is frozen until the run reaches a terminal
state. Upload disclosure is per-run (app-level provider config), never
implied, never silently re-used.

### 1.1 Decisions (from brainstorming)

| # | Decision |
|---|---|
| D1 | **Scope:** full first-release workbench (create + run + review/apply + Improve Preset + disclosure). Matches parent §3.1/§14 as one deliverable; the implementation plan may decompose internally. |
| D2 | **Surface model:** Workbench is a mode of the Result Workspace (same canvas, same `ImageDocument`, same safe-export). One canvas, one document, one safe-export path. |
| D3 | **Three-pane layout:** classic three-column (agent session · canvas · automation review), always visible in Workbench mode. |
| D4 | **Candidate lifecycle:** unified proposal model — run-existing and author/improve both produce `EditProposal` candidates on the same canvas; candidates are never committed to `ImageDocument` until Apply. Accept-revision and apply-candidates are distinct actions (parent §8.3). |
| D5 | **Provider config + disclosure:** app-level config (provider/model/keychain); per-run disclosure modal before every upload (author/improve). Run-existing bypasses disclosure (no upload). |
| D6 | **Live progress:** streaming conversation + collapsible tool cards in the agent session pane, reconstructed from the `RunEvent` stream. |
| D7 | **Session persistence:** in-memory only in SP6; session/run persistence + resume is a deferred subproject. |
| D8 | **Run architecture:** `Task::stream` + channel events through a `Subscription` (Option 1). Cooperative cancellation via `RunCancellation`. |
| D9 | **Freeze rule:** in-progress candidate editing frozen during a run (pan/zoom live); fully editable after terminal. |

## 2. Scope

### 2.1 In scope

- Workbench mode of Result Workspace (three-pane, Option A).
- Entry: Smart Redaction toolbar button → preset picker → Run-existing / Create / Improve.
- Run-existing: headless automation → candidates → review/apply (no agent, no upload).
- Author/improve: bounded agent run with streaming session pane + automation review.
- Canvas candidate overlay (dashed proposals, confidence badges, select/move/resize/delete/reject, before/after).
- Per-run upload disclosure modal; app-level provider config (provider/model/keychain).
- Accept revision (→ `PresetStore`) and Apply candidates (→ `apply_batch`) as distinct actions.
- Improve Preset correction-evidence flow.
- In-memory sessions (no persistence/resume).
- Error model with correct retry per terminal state.
- Privacy-safe tracing + activity entries.
- Linux + macOS runtime verification.

### 2.2 Out of scope / carry-forward

- **Session/run persistence + resume** (parent §7.5 full resume with expired-attachment reattach) — deferred subproject; SP6 is in-memory. `RevisionProvenance.source_run_ref` is populated to enable it later.
- **Fixture regression UI** (parent §8.3 "fixture regression summary when fixtures exist") — no fixture management in SP6; hook populated, UI deferred.
- **Visual version canvas** (parent §8.6) — data model has the nodes; UI deferred.
- **Full provider-management settings UI** — minimal key-presence surface only; provider switching/keychain UX is a later product decision.
- **Budget tuning UI** — finite budget built from documented defaults; per-run budget configuration deferred.
- **`layout` capability** — permanently `capability_unavailable`; authoring guardrails reflect only `ocr` / `region_features` / `template_match` as real (OCR behind the off-by-default `ocr` feature).
- **Live preview of stitching / capture-side workbench** — Workbench is post-capture only.
- **Visual workflow diagrams** (parent §5.3) — IR semantic summary is text only.

## 3. Architecture and Integration Points

### 3.1 Workbench as a Result Workspace mode

`ResultWorkspace` (`crates/rollshot-app/src/result_workspace/mod.rs:69`) gains a
`mode: WorkspaceMode` field:

```rust
pub enum WorkspaceMode {
    Normal,                       // single canvas + Navigator (today's layout)
    Workbench(WorkbenchState),   // three-pane (D3)
}
```

The existing single-canvas + Navigator layout is `Normal`; the three-pane
layout is `Workbench`. Switching mode rebuilds `workspace_row`
(`view.rs:117`, the existing toggle-and-row-rebuild mechanic behind
`Message::ToggleNavigator`), and may resize the window on macOS
(`window::Id` in `macos_product.rs`). The `ResultDocument`, `ImageDocument`,
viewport, and safe-export plumbing are shared across both modes — the
Workbench does not duplicate canvas or save logic.

**Entry point:** a "Smart Redaction" button in the toolbar (`view.rs:58`,
beside the existing `Tool::Redact` at `view.rs:72`) opens a preset picker.
- **Run existing** → lightweight mode (canvas + candidates + review bar, no agent pane).
- **Create** / **Improve Preset** → full three-pane `Workbench` mode.

Both paths produce `EditProposal` candidates; the canvas adds a candidate
overlay via the existing `iced::widget::stack![img, overlay]` pattern
(`view.rs:216`).

### 3.2 Run architecture (Option 1)

The bounded agent runs as an `iced::Task` that spawns
`AgentRunner::run_with_provider` on the Tokio runtime. The `RunEvent` stream
(`TextChunk` / `ToolCallStart` / `ToolCallEnd`) flows through an mpsc channel
exposed via `Subscription::run_with_id`. Each event arrives as a
`Message::Workbench(WorkbenchMessage::RunEvent(...))` and is appended to the
agent session pane. The final `RunTerminalState` arrives as a terminal
message and switches the workbench sub-state.

Cancellation: the Cancel button calls `RunCancellation::cancel()` on the
shared flag; the run stops cooperatively at the next cancellation point
(next tool-call boundary / provider request). A `Terminal(Cancelled)` then
arrives. The canvas (its own message stream) stays fully interactive during a
run — the run does not lock the UI thread.

This is the standard iced 0.14 pattern; the subscription layer already uses
`Subscription::batch` + `iced::event::listen_with`
(`result_workspace/update.rs:801`).

### 3.3 Host preparation order (load-bearing seam)

Before the agent run starts, the Workbench must:

1. Build `VisualIndex` from the capture image (`VisualIndex::build`).
2. Construct an empty `RealAutomationHost::new()` (cheap, infallible).
3. Call `prepare_template_match` / `prepare_region_features` / `prepare_ocr`
   (OCR `#[cfg(feature = "ocr")]`) for the anticipated queries. Prepared
   results are cached under canonical region-rect keys; callbacks are
   lookup-by-key + truncate only (mirrors the `region_features` precedent).
4. Wrap the host in `Arc<Mutex<dyn AutomationHost>>` and register `DryRunTool`
   with the executor and host.
5. Build `ToolContext::new(session_id, initial_source, validation_limits,
   execution_policy, image_dims, &cancellation)` — note `image_dims` is
   `(u32, u32)`; the source pixels live only in `VisualIndex`, kept
   consistent by the Workbench.

Re-preparing mid-run would require re-locking the same mutex `DryRunTool`
holds during execution, so all preparation completes before the run begins.
The JS executor is `QuickJsExecutor` from `rollshot-automation-rquickjs`,
pulled in as a Workbench dependency.

### 3.4 Unchanged surfaces

`ResultDocument`, `ImageDocument`, `secure_sharing`, and the
`lower()` → `apply_batch()` path are unchanged. Applying candidates lowers
them to `OpaqueRedaction` annotations; safe-export
(`secure_sharing::has_secure_redactions`, `secure_sharing.rs:61`) activates
automatically once redactions exist — no SP6-specific wiring there.

## 4. Workbench State Model and Lifecycle

### 4.1 `WorkbenchState`

```rust
pub struct WorkbenchState {
    pub preset: Option<Preset>,
    pub active_revision: Option<AutomationRevision>,
    pub session: AgentSession,                  // in-memory (D7)
    pub run_state: RunState,
    pub live_activity: Vec<ActivityEntry>,      // reconstructed from RunEvent stream
    pub pending_proposal: Option<EditProposal>,
    pub pending_draft: Option<DraftAutomation>,
    pub review: CandidateReview,
    pub provider_config: ProviderConfig,        // app-level (D5)
    pub vision: VisionContext,                  // VisualIndex + prepared host + executor
    pub budget: RunBudget,                      // finite literal (no ergonomic ctor)
    pub error: Option<WorkbenchError>,
}
```

### 4.2 `RunState`

```rust
pub enum RunState {
    Idle,
    Running { cancellation: RunCancellation, stream_id: iced::widget::Id },
    Terminal(RunTerminalState),
}
```

- **`Idle`** — prompt composer active; Send starts a run (after disclosure for author/improve).
- **`Running`** — session pane streams `ActivityEntry`s; canvas frozen for candidate editing (pan/zoom live); Cancel visible. The subscription keyed on `stream_id` drains the channel.
- **`Terminal(RunTerminalState)`**:
  - `ReadyForReview` → fills automation-review pane (source diff vs active revision + IR summary) AND canvas candidates (the dry-run `EditProposal`). Canvas becomes fully editable.
  - `NeedsUserInput` → prompt composer refocuses with the agent's clarifying question; no candidates.
  - `BudgetExhausted` → shows which dimension hit the ceiling; retains last valid draft candidate if any.
  - `ProviderFailure` / `SourceValidationFailure` / `RuntimeFailure` / `AgentProtocolFailure` → error surface with the correct retry action (parent §7.4: separate so UI offers correct retry).
  - `Cancelled` → returns to `Idle` keeping session history.

### 4.3 Three flows through the same state machine

- **Run-existing** (lightweight, no agent pane): skip `AgentSession`/provider entirely. Build `VisualIndex` + host, run the active revision's `ValidatedAutomation` through `execute_to_proposal` directly, produce `pending_proposal` → terminal-equivalent without `pending_draft`. No disclosure (no upload).
- **Author** (full three-pane): disclosure → `AgentRunner::run_with_provider` → terminal state fills both panes.
- **Improve Preset** (full three-pane, from a prior review): assemble correction evidence (parent §6.4) into a new `AuthorizedModelInput`, then same as Author. The parent revision is the baseline for the automation diff.

### 4.4 Candidate review model

```rust
pub enum CandidateReviewState {
    Pending,
    Accepted,
    Rejected,
    Modified(ProposedEdit),
}

pub struct CandidateReview {
    pub per_candidate: BTreeMap<CandidateId, CandidateReviewState>,
}
```

Canvas gestures mutate this in place: deleting = `Rejected`, dragging =
`Modified(new bounds)`. On **Apply candidates**, `ReviewDecision` is built
from this state, `lower(proposal, decision) → Vec<EditOp>`,
`ImageDocument::apply_batch` commits them as one undo entry. On **Accept
revision** (distinct per §8.3), `pending_draft.validated` →
`PresetStore::add_revision` then `set_active_revision`.

### 4.5 Freeze rule (D9)

While `Running`, the `pending_proposal` from the *previous* run (if any) is
shown read-only; the in-flight dry-run proposal is not yet materialized.
Pan/zoom stay live. After `Terminal(ReadyForReview)`, the new proposal
replaces the old and canvas editing unlocks.

## 5. Canvas Candidate Overlay and Review Interactions

### 5.1 Rendering

A sibling layer in the existing `iced::widget::stack![img, overlay]`
(`view.rs:216`). `AnnotationCanvas::draw` (`canvas.rs:388`) currently draws
committed annotations + the in-progress drag draft. SP6 adds a third draw
pass: **proposed candidates**, with the distinct visual language:

- **Dashed border** for proposals, **solid** for accepted (committed) annotations — never conflated.
- **Confidence color:** red dashed (`#ff5050`) for ≥0.85, amber dashed (`#ff8c1a`) for low-confidence/applicability-warning. Warnings are never silently omitted (parent §8.2).
- **Confidence badge** floats above each candidate: `● score · label` (or `⚠` for warnings). Drawn via the existing canvas text path (the vendored DejaVu font `rollshot_image_document::style::FONT_REGULAR_BYTES` already used for annotation text).
- **Selected state:** solid blue border + 4 resize handles, reusing the existing `redaction_handles` / `resized_rect` / `direct_manipulation_hit` machinery (`canvas.rs:348`, `update.rs:136`). Same gestures as manual redactions: drag body = move, drag handle = resize, Del = reject, right-click/long-press = rationale popover.

The overlay program owns proposal rendering (it has the `ProposedCandidate`s),
keeping `RenderShape` for committed annotations unchanged — surgical, no
token-set churn. `ProposedCandidate` carries `confidence` / `label` /
`rationale` / `provenance` (`proposal.rs:134`), surfaced in the badge + popover.

### 5.2 Review bar

Canvas bottom bar: `Original` / `Before-After` / `Candidates` toggle +
candidate count + zoom. "Before/After" swaps the render between original
pixels and redacted render to judge coverage (toggles whether committed
annotations are drawn; candidates always drawn in Candidates mode). Reuses
existing `ViewportState` zoom/scroll.

### 5.3 Gestures → `CandidateReview`

- Click empty candidate → select (single; clear prior selection).
- Drag body/handle → `CandidateReview::Modified(bounds)`, live-updated during drag, committed on release.
- Del / Backspace → `Rejected`.
- Click empty canvas → deselect.
- Multi-select: shift-click (modifier tracking already in `ResultWorkspace.modifiers`).

### 5.4 Apply candidates

Builds `ReviewDecision { accepted, rejected, modified }` from
`CandidateReview`, `lower(proposal, decision) → Vec<EditOp>`,
`ImageDocument::apply_batch` (one undo entry). Proposals flip dashed→solid;
safe-export labels activate via `secure_sharing::has_secure_redactions`. The
proposal is then cleared from `pending_proposal` (the `ImageDocument` now
owns the committed annotations).

Before `lower`, `base_document_state_id` is re-stamped from
`ImageDocument::state_id()`: the dry-run `EditProposal` carries a hardcoded
`base_document_state_id: 0` / `ProposalId(1)` from `DryRunTool`, so the
Workbench must re-stamp against the live document before lowering (§10.5).

### 5.5 Tall stitched images

Candidate draw culls to the visible rect like committed annotations
(`canvas.rs:404`'s `intersects` check), so a 1080×20000 capture with hundreds
of candidates stays responsive. Pan/zoom unchanged.

## 6. Agent Session Pane and Streaming

### 6.1 Layout

Left pane: a vertical scrollable column of `ActivityEntry`s, oldest-at-top,
with a prompt composer pinned at the bottom. Reconstructs the conversation
from the `RunEvent` stream + `AgentSession` text (`AgentSession.exchanges()`
carries only finished user/assistant prose, not tool calls — §10.3).

### 6.2 `ActivityEntry` types

- **`UserMessage(String)`** — from the prompt composer on send.
- **`AssistantText(String)`** — accumulated from `RunEvent::TextChunk` deltas (typewriter). On terminal, reconciled against `ReadyForReview.assistant_text` / `NeedsUserInput.assistant_text` (authoritative final text).
- **`ToolCard { name, status, summary }`** — opened on `ToolCallStart`, updated on `ToolCallEnd`. `status = Running | Success | Failed`. `summary` is a **bounded** string from the tool's `ToolOutcome` (e.g. `inspect_ocr`: "12 text regions found"; `replace_source`: "437 bytes, gen 3"; `validate_source`: "ok — 1 capability call"; `dry_run`: "8 candidates, 2.3% area"; `submit_for_review`: "submitted"). Tool arguments and results are **not** shown in full (privacy: OCR text may be the PII being hidden — parent §9.6). Cards are collapsible.
- **`RunStatus { turn, budget_used, elapsed }`** — a thin status line, updated as events arrive. Budget shown as the most-consumed dimension + remaining wall-time.

### 6.3 Run status header

Top of pane: provider + model, turn N, budget meter, elapsed, **Cancel**
(visible only while `Running`). On terminal, shows the terminal-state label
(e.g. "Ready for review", "Budget exhausted — ModelCalls").

### 6.4 Prompt composer

Multi-line text input + Send button. Disabled while `Running`. In
`NeedsUserInput`, prefilled with the agent's clarifying question as context
above the composer, and Send starts a new run with the answer. Attach-visual-
context controls (selected region, selected annotations) live here per parent
§7.1 — toggles feeding the next run's `AutomationInput`.

### 6.5 Disclosure integration

Send on an author/improve run does *not* immediately start the run. It opens
the disclosure modal (§7) first; the run starts only on explicit "Send to
{provider}" confirmation. Run-existing has no disclosure.

### 6.6 Cancellation

Cancel → `RunCancellation::cancel()` on the shared flag. The run stops
cooperatively at the next cancellation point. `Terminal(Cancelled)` returns
the pane to `Idle` with session history intact. No partial draft is promoted.

### 6.7 Privacy

The pane never displays raw OCR text, full tool arguments, raw provider
responses, or image pixels — only bounded summaries, counts, durations, and
terminal states (parent §9.6). Enforced at the `ActivityEntry` construction
site (the event→entry mapping), not by filtering after the fact.

## 7. Upload Disclosure and Provider Config

### 7.1 Provider config (app-level)

```rust
pub struct ProviderConfig {
    pub provider: ProviderKind,        // Anthropic | OpenAI
    pub model: String,
    pub base_url: Option<String>,
    pub key_source: KeySource,
}
```

Stored in the existing `rollshot_config_dir()`
(`rollshot-app/src/daemon/config.rs:170`, already etcetera-based). `KeySource`
resolves the API key at run time from environment or the OS keychain (macOS
Keychain / Linux Secret Service) — never written to the config file. The
Workbench reads the active config; if no key is resolvable, the disclosure
modal shows a "configure provider" state instead of Send. No new config UI
beyond a minimal settings surface (provider/model/key presence); full
provider-management UX is out of scope (§2.2).

### 7.2 Per-run disclosure modal

Opens on Send for author/improve, before any upload (parent §9.1). Uses the
existing `iced::widget::stack` scrim+modal pattern (`view.rs:342` / `389`).

| Field | Example |
|---|---|
| Provider | Anthropic |
| Model | claude-sonnet-4-6 |
| Payload mode | Full screenshot **or** OCR/layout-only |
| Complete screenshot | ✓ attached (3.2 MB) |
| OCR/layout | ✓ included |
| Selected region | none |
| Selected annotations | none |
| Correction evidence (improve) | 2 rejected, 1 modified, 1 added |
| Estimated budget | 10 turns, 30s wall, 20k tokens |

Two explicit, distinct consent lines per parent §9.1: one for **complete
screenshot inclusion**, one for **OCR/layout-only** — never implied by a
single toggle. The primary button reads `Send to {provider}` (not a generic
"OK"); back/cancel returns to the composer without uploading.

### 7.3 Payload mode

Full-screenshot sends the image bytes as an attachment; OCR/layout-only omits
the image and sends only prepared OCR/layout summaries (the `inspect_*` tools
already bound their output). The mode chosen here is what the disclosure
reflects — they must match.

### 7.4 Resume rule (parent §7.5)

SP6 ships in-memory sessions (D7), so there is no cross-process resume. The
rule still applies *within* a session: if a visual attachment is removed or
the capture closed, a subsequent run's disclosure reflects the current
attachments — it never silently re-uses a previously-authorized payload. The
disclosure is re-shown before every run. Full session resume with
expired-attachment reattach is the deferred persistence subproject.

### 7.5 `AuthorizedModelInput` construction

From the disclosure-approved config + user message + attachment descriptors
(`AttachmentDescriptor { media_type, width, height, byte_count }`). The
`provider`/`model` strings must match what the adapter will stream (the driver
uses `input.manifest.model` as the `model_id`) — §10.7, enforced by
constructing both the adapter and the `AuthorizedModelInput` from the same
`ProviderConfig`.

### 7.6 Privacy

The disclosure modal is the single chokepoint where the user sees exactly what
leaves the machine. No run starts without passing through it (author/improve).
Run-existing bypasses it (no upload).

## 8. Automation Review Pane and Improve Preset

### 8.1 Pane contents (parent §8.3)

Shown only in `Terminal(ReadyForReview)` for author/improve:

1. **Source diff** — agent-authored JS vs the active revision's source (or empty template for a new preset). Monospace line diff over `pending_draft.source` vs `active_revision.artifact.source`.
2. **Workflow IR semantic summary** — from `rollshot-automation::diff::semantic_summary`: capability additions/removals, threshold/padding/region/limit changes, candidate-count change, static-cost change. Compact bullet list (no graph — parent §5.3 defers visual workflow diagrams).
3. **Static cost + budget** — `ValidationSummary` (source bytes, AST nodes, helper count, capability calls, max output candidates) + the run's `UsageSnapshot` (turns, tokens, wall-time, cost). Shows whether the revision is within `smart_redaction_default` policy limits.
4. **Actions** — three distinct buttons (parent §8.3: "accepting revision and applying candidates are related but distinct"):
   - **Accept revision** — `pending_draft.validated` → `PresetStore::add_revision(preset_id, new_rev_id, parent_id = active_revision_id, artifact, provenance = AgentRun { source_run_ref: Some(session_id) })`, then `set_active_revision`. Does *not* touch the current image's annotations. Offers "apply candidates now?".
   - **Ask agent to revise** — returns to the composer with the session intact; the revision request starts a new bounded run whose parent is the just-rejected draft. (Distinct from Improve Preset — mid-authoring refinement, not correction evidence.)
   - **Discard** — drops `pending_draft` + `pending_proposal`, returns to `Idle`, session history kept.

**Apply candidates** lives on the *canvas review bar* (§5.2), not here — to
reinforce that applying to the current image is separate from accepting the
reusable automation.

### 8.2 Improve Preset flow (parent §6.4, §8.5)

Entry from a prior review: the user reviews a run-existing result on the
canvas, edits candidates (reject/modify/add), then chooses **Improve Preset**.
This assembles correction evidence:

- parent automation revision (the active one)
- original proposal candidates
- user-rejected candidate ids
- user-modified candidates (with new bounds)
- user-added redactions (if explicitly included)
- optional explanatory text (from a small composer)

The disclosure modal reflects "correction evidence included" (§7.2 field).
The run's parent is the active revision; the resulting draft's automation
diff is against that parent. Reviewed exactly like initial authoring. Does
not mutate the preset directly — only a new accepted revision does (parent
§2.5).

### 8.3 Fixture regression (carry-forward)

A preset may have explicitly-approved non-sensitive fixtures. Running a
candidate revision against them and showing pass/fail is mentioned in the
parent spec (§8.3). For SP6 this is **out of scope** (no fixture management
UI); the `RevisionProvenance.source_run_ref` hook is populated so a later
subproject can build regression on top. Stated as a carry-forward, not
implemented.

## 9. Error Handling, Privacy, Verification

### 9.1 Error model (`WorkbenchError`)

Maps sub-flow failures to the correct UI retry action (parent §7.4 —
provider/source/runtime/product failures separate so the UI offers the correct
retry):

| Error | Source | UI action |
|---|---|---|
| `ProviderFailure` | `RunTerminalState::ProviderFailure` | Retry run / check config (key, network) |
| `SourceValidationFailure` | `RunTerminalState::SourceValidationFailure` | Show source-span diagnostics; agent repair loop continues within budget, else shown to user |
| `RuntimeFailure` | `RunTerminalState::RuntimeFailure` | Retry / report; sandbox errors never become generic text the agent may ignore |
| `AgentProtocolFailure` | `RunTerminalState::AgentProtocolFailure` | Retry run / report |
| `BudgetExhausted` | `RunTerminalState::BudgetExhausted` | Show which dimension; allow review if a valid draft was retained |
| `VisionPrepare` | `VisualIndex::build` / `prepare_*` failure | Block run; show capability error (`template_not_found`, `region_too_large`, etc.) |
| `Store` | `PresetStore` IO/compatibility | Block accept; show `StoreError` variant |
| `Config` | Unresolvable provider key | Block run; point to Settings |
| `Cancelled` | `RunTerminalState::Cancelled` | Not an error — return to `Idle`, session kept |

`validate_source` returns `Vec<SourceDiagnostic>` (§10.6), not a typed
enum — the error display renders structured diagnostics with source spans,
not a flattened string. The agent's `ValidateSourceTool` flattens it
internally to JSON; the Workbench surfaces the structured form for the user
by re-deriving from `pending_draft.validation_summary` + any diagnostics
captured during the agent repair loop.

### 9.2 Privacy (parent §9.5, §9.6, §9.2)

- `ActivityEntry` construction uses bounded summaries only (counts, durations, labels) — never raw OCR text, tool arguments, provider response bodies, or image pixels.
- `tracing` events use stable `rollshot::workbench::*` targets with structured fields (duration, result count, error code, terminal state, provider name, model name, budget dimension) — never OCR text, query contents, or pixels. Mirrors the OCR privacy rule validated by the §8.2 D9 privacy test.
- `Provenance`/`rationale` on candidates carries agent-supplied text; shown in the right-click popover, never in tracing.
- Persisted (SP5, already done): preset/revision metadata + accepted automation source/IR. **Not** persisted in SP6: sessions, run events, raw OCR, tool results, provider bodies, visual attachments (in-memory).

### 9.3 Verification (preview-to-accept path)

- Budget enforced by `BudgetTracker` (agent crate) during the run; the Workbench builds a finite `RunBudget` literal (no ergonomic constructor exists — §10.4; the Workbench owns a small constructor with documented defaults).
- Policy: `ExecutionPolicy::smart_redaction_default(...)` is the only constructor; the Workbench uses it. `decode_proposal` runs `validate_policy` internally (twice); no third recheck before `lower`.
- `PresetStore::add_revision` runs `ensure_compatible` (re-validate + re-parse JS) — so a corrupt/incompatible revision is rejected at store time, not just load time.
- Candidate output validation (parent §9.4): `validate_policy` rejects malformed candidates, non-finite coordinates, zero-area, excessive counts/area. Final gate before `lower`.

### 9.4 Platform verification (parent §11.7)

Two active paths — Linux `iced::application` workspace + macOS `iced::daemon`
`Phase::Workspace`. Every `WorkbenchMessage` flows through
`result_workspace::Message` and is forwarded through
`macos_product::Message::Workspace` on macOS. Manual verification covers:
disclosure, visual attachments, session + tool events, automation + visual
diffs, candidate editing, Improve Preset, cancellation, safe copy/save,
tall-stitch performance. Because `MacosProduct.document`
(`macos_product.rs:118`) retains the capture across phase transitions, the
Workbench operates on the same `ResultDocument` to preserve that invariant.

## 10. Seams the Workbench Must Bridge

From the crate-API map, made explicit for the implementation plan:

1. **Host preparation before run.** Build `VisualIndex` + prepare the host, then wrap in `Arc<Mutex>` once before the run starts. Re-preparing mid-run would re-lock the mutex `DryRunTool` holds during execution.
2. **Executor dependency.** `QuickJsExecutor` is pulled in from `rollshot-automation-rquickjs`; no concrete executor exists in the five core crates.
3. **Conversation reconstruction.** The session pane is reconstructed from the `RunEvent` stream, not `AgentSession` alone (which carries only finished user/assistant prose, not tool calls).
4. **`RunBudget` literal.** No ergonomic constructor exists (only `unlimited()`); the Workbench owns a small documented constructor for finite budgets.
5. **Proposal re-stamping.** `base_document_state_id` is re-stamped from `ImageDocument::state_id()` before `lower` (the dry-run proposal carries a hardcoded `0`).
6. **`validate_source` diagnostics.** Returns `Vec<SourceDiagnostic>`, not a typed enum; shown as structured spans, not a flattened string.
7. **`ProviderConfig` single source.** The adapter and `AuthorizedModelInput` are constructed from one `ProviderConfig` so `provider`/`model` strings match what the adapter streams.
8. **`RunEvent::TurnComplete` never emitted.** The driver emits `TextChunk` / `ToolCallStart` / `ToolCallEnd` only; turn boundaries are inferred from those patterns.
9. **`layout` permanently unavailable.** `RealAutomationHost::layout` always returns `capability_unavailable`; authoring guardrails reflect only `ocr` / `region_features` / `template_match` as real (OCR behind the off-by-default `ocr` feature).

## 11. Success Criteria

This subproject is complete when:

1. A user starts a Smart Redaction session from the Result Workspace toolbar; sees exactly what will be sent to which provider before any upload.
2. A bounded agent inspects (OCR/region/template), authors, validates, and dry-runs an automation; the session pane streams text + tool cards live; cancel works.
3. A `ReadyForReview` run fills the automation review pane (source diff + IR summary) and the canvas with editable candidates.
4. The user reviews/edits candidates on the canvas (select/move/resize/delete/reject, before/after) and applies them as one undoable `ImageDocument` transaction; safe-copy/save activates.
5. The user accepts an immutable automation revision (distinct from applying candidates); it persists via `PresetStore` and becomes the active revision.
6. The user runs an existing preset (no LLM call) and reviews/applies its candidates.
7. The user explicitly Improves Preset with correction evidence; the new revision is reviewed like initial authoring.
8. Each terminal state surfaces the correct retry action; errors never become generic ignored text.
9. No OCR text, tool args, provider responses, or image pixels appear in `tracing` events or activity entries.
10. Linux iced Result Workspace + macOS iced Result Workspace both verified (disclosure, session, diffs, candidate editing, improve, cancel, safe save, tall-stitch perf).
11. `cargo test` / `fmt` / `clippy -- -D warnings` pass on the default lane (no `ort`/models — the Workbench introduces no new `unsafe`).

## 12. Delivery Decomposition Note

This spec covers the full first-release workbench (D1). The implementation
plan (next step via `superpowers:writing-plans`) is expected to decompose
internally — e.g. Workbench mode scaffolding + state model → canvas candidate
overlay → run architecture + streaming session → disclosure + provider config
→ automation review + accept/apply → Improve Preset → platform verification —
with stable interfaces between phases. Each phase gets its own commit-worthy,
independently testable slice, mirroring the SP1–SP5 PR-phase precedent.

## References

- Parent: `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`
- Vision host: `docs/superpowers/specs/2026-06-22-rollshot-vision-runtime-host-design.md`
- OCR backend: `docs/superpowers/specs/2026-06-24-ocr-backend-design.md`
- Bounded agent core: `docs/superpowers/specs/2026-06-23-bounded-agent-core-design.md`
- Preset persistence: `docs/superpowers/specs/2026-06-23-preset-persistence-design.md`
- Automation frontend: `docs/superpowers/specs/2026-06-21-automation-frontend-runtime-design.md`
- Edit proposal foundation: `docs/superpowers/specs/2026-06-20-edit-proposal-foundation-design.md`
- `ResultWorkspace`: `crates/rollshot-app/src/result_workspace/mod.rs:69`
- `AnnotationCanvas`: `crates/rollshot-app/src/result_workspace/canvas.rs:196`
- `AgentRunner::run_with_provider`: `crates/rollshot-agent/src/driver.rs:374`
- `RunEvent`: `crates/rollshot-agent/src/runtime.rs:522`
- `RealAutomationHost`: `crates/rollshot-vision/src/host.rs:71`
- `PresetStore`: `crates/rollshot-preset/src/store.rs:41`
- `EditProposal` / `lower`: `crates/rollshot-edit-proposal/src/{proposal,review}.rs`
