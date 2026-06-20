# Visual Edit Proposal Foundation Design (Subproject 2)

**Date:** 2026-06-20
**Status:** Approved design
**Parent:** `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md` (Delivery Decomposition §12, subproject 2)
**Spike inputs:** `docs/superpowers/spikes/2026-06-20-spike-decisions.md`

## 1. Summary

This subproject builds the **headless data/logic foundation** that lets a reviewed set of agent-proposed visual edits be applied to an `ImageDocument` as **one undoable transaction**, without pulling any agent/LLM concerns into the deliberately framework-neutral `rollshot-image-document` crate.

It is split into two layers across a crate boundary:

1. **Document layer** — `rollshot-image-document` gains a typed `EditOp` enum (full CRUD) and an atomic `apply_batch()` primitive that applies many operations as a single history entry (one-step undo). This crate stays headless and agent-free; it continues to own annotation validation and final mutation.
2. **Proposal layer** — a new crate `rollshot-edit-proposal` owns the agent-flavored review model: `EditProposal`, `ProposedCandidate`, `ReviewDecision`, provenance/confidence metadata, lowering of an accepted decision to document `EditOp`s, and product-policy validation (count / total area / out-of-bounds). It depends on `rollshot-image-document` but not on iced/app.

Subprojects 4 (agent core), 5 (persistence), and 6 (workbench UI) depend on `rollshot-edit-proposal` rather than on `rollshot-app`.

This subproject is **spike-independent**: it requires no parser/runtime/agent decisions and can proceed in parallel with the others.

## 2. Scope

### 2.1 In scope

- Typed `EditOp` (CRUD over the three annotation kinds) in `rollshot-image-document`.
- Atomic `apply_batch()` giving **one undo entry** + one-step undo restoring the pre-batch state.
- `EditError` extensions required by batch validation.
- New crate `rollshot-edit-proposal` with the `EditProposal` / `ProposedCandidate` / `ProposedEdit` / `ReviewDecision` / `Provenance` / `ConfidenceSummary` model.
- `lower()` (accepted + modified candidates → `Vec<EditOp>`).
- Product-policy validation (`validate_policy`): candidate count, total redaction area, out-of-bounds policy.
- Unit tests for all of the above (spec §11.1).

### 2.2 Out of scope (owned by other subprojects)

- On-canvas overlay **rendering** and review **interactions** (select/move/resize/delete candidates, before/after toggle) — Subproject 6 (Workbench). This subproject defines only the candidate **data model** they manipulate.
- **Producing** proposals (agent runs, tools) — Subproject 4.
- **Persisting** proposals/revisions/sessions — Subproject 5.
- Wiring `apply_batch` into `rollshot-app`'s result workspace (the `AcceptCandidates` message, save/copy handoff) — Subproject 6.
- Improve-Preset evidence flow consuming `ReviewDecision` — Subproject 7.

## 3. Architecture

```text
rollshot-edit-proposal  (NEW — agent/review metadata, framework-neutral)
   EditProposal · ProposedCandidate · ProposedEdit · ReviewDecision · Provenance
   lower(proposal, decision) -> Vec<EditOp>
   validate_policy(candidates, limits, image_dims) -> Result<(), PolicyError>
        |
        |  depends on (lowers to EditOp; never the reverse)
        v
rollshot-image-document  (headless, agent-free — owns validation + final mutation)
   EditOp (CRUD) · BatchOutcome · ImageDocument::apply_batch (atomic, one undo entry)
```

**Boundary rule:** `rollshot-image-document` knows nothing about confidence, provenance, rationale, candidates, or agents. It receives a `Vec<EditOp>` of pure document mutations and applies them. All review/agent metadata lives one layer up.

**One-undo-entry mechanism (no new history machinery):** `apply_batch` reuses the existing private `snapshot()`/`commit()` pair — snapshot once before the batch, apply all ops, `commit()` once. This is the exact pattern `delete_annotation` already uses to group deletion + renumbering into one entry. Therefore a single `ImageDocument::undo()` restores the complete pre-batch (pre-agent) state, and `base_document_state_id` is simply the `state_id()` captured before apply (used for provenance/staleness, not for recovery).

## 4. Document Layer — `rollshot-image-document`

### 4.1 `EditOp` (new, `src/edit_op.rs`)

```rust
pub enum EditOp {
    // Create
    AddRedaction { bounds: ImageRect },
    AddTextNote { position: ImagePoint, text: String },
    AddNumberCallout { tip: ImagePoint, bubble: ImagePoint },
    // Update (id must exist BEFORE the batch)
    UpdateRedactionBounds { id: AnnotationId, bounds: ImageRect },
    UpdateTextPosition { id: AnnotationId, position: ImagePoint },
    UpdateText { id: AnnotationId, text: String },
    UpdateNumberPoints { id: AnnotationId, tip: ImagePoint, bubble: ImagePoint },
    // Delete
    Delete { id: AnnotationId },
}

pub struct BatchOutcome {
    /// AnnotationIds allocated for the Add* ops, in the order those ops appear in the batch.
    pub added_ids: Vec<AnnotationId>,
}
```

### 4.2 `apply_batch` (new, `src/document.rs`)

```rust
impl ImageDocument {
    pub fn apply_batch(&mut self, ops: Vec<EditOp>) -> Result<BatchOutcome, EditError>;
}
```

Semantics:

- **Atomic / all-or-nothing.** Every op is validated against the *current* document state **before any mutation**. If any op is invalid, return `Err(EditError)`, perform no mutation, take no snapshot, and leave `state_id` unchanged.
- **One history entry.** On success: `snapshot()` once → apply every op in order → if any `Delete` removed a `NumberCallout`, run `renumber_compactly()` once → `commit()` once. Result: exactly one undo entry, one `state_id` increment.
- **Id-reference constraint.** `Update*`/`Delete` reference annotations that exist **before** the batch. Referencing an id that an earlier `Add*` in the same batch would allocate is **not supported** (validation runs up-front against the pre-batch state, so such a reference fails as `UnknownAnnotation`). v1 batches are homogeneous adds; this constraint is explicit, not incidental.
- **Geometry behavior** mirrors the existing single-op methods: redaction bounds are clamped to the image (`ImageRect::clamp_to`) then rejected if empty (`ZeroArea`); empty/whitespace text → `EmptyText`; type mismatch on an `Update*` → `WrongKind`; unknown id → `UnknownAnnotation`. New: any non-finite coordinate (NaN/∞) → `NonFiniteCoordinate`.
- **Empty batch** (`ops.is_empty()`) is a no-op: returns `Ok(BatchOutcome { added_ids: vec![] })`, no snapshot/commit (consistent with the existing no-op-edit-skips-history behavior).

### 4.3 `EditError` extension

Add to the existing enum (`EmptyText`, `ZeroArea`, `UnknownAnnotation`, `WrongKind`):

- `NonFiniteCoordinate` — a coordinate in an op is NaN or infinite.

(`OutOfBounds` is **not** added here — out-of-bounds is handled by deterministic clamping at this layer, with stricter reject-policy living in the proposal layer per §6.)

### 4.4 Exports

`src/lib.rs` adds `pub use edit_op::{EditOp, BatchOutcome};`.

## 5. Proposal Layer — `rollshot-edit-proposal` (new crate)

Framework-neutral. Depends on `rollshot-image-document` + `serde` (for the persistence subproject) + `thiserror`. No iced/app/agent dependencies.

```rust
pub struct CandidateId(pub u64);
pub struct ProposalId(pub u64);

/// What document change a candidate proposes. Mirrors the EditOp kinds an agent
/// may propose. v1 primarily produces AddRedaction; the enum is general (CRUD).
pub enum ProposedEdit {
    AddRedaction { bounds: ImageRect },
    AddTextNote { position: ImagePoint, text: String },
    AddNumberCallout { tip: ImagePoint, bubble: ImagePoint },
    UpdateRedactionBounds { id: AnnotationId, bounds: ImageRect },
    UpdateTextPosition { id: AnnotationId, position: ImagePoint },
    UpdateText { id: AnnotationId, text: String },
    UpdateNumberPoints { id: AnnotationId, tip: ImagePoint, bubble: ImagePoint },
    Delete { id: AnnotationId },
}

pub struct Provenance {
    /// Identifies the run/automation that produced this (opaque to subproject 2;
    /// populated by subproject 4). Kept privacy-safe: ids/counts, never prompts.
    pub source: ProvenanceSource,
}
pub enum ProvenanceSource { Manual, Agent { run_id: u64 } }

pub struct ConfidenceSummary { pub min: f32, pub max: f32, pub mean: f32, pub count: u32 }

pub struct ProposedCandidate {
    pub id: CandidateId,
    pub edit: ProposedEdit,
    pub confidence: f32,
    pub rationale: Option<String>,
    pub provenance: Provenance,
}

pub struct EditProposal {
    pub id: ProposalId,
    pub base_document_state_id: u64,   // ImageDocument::state_id() before apply
    pub candidates: Vec<ProposedCandidate>,
    pub confidence_summary: ConfidenceSummary,
    pub rationale_summary: Option<String>,
    pub provenance: Provenance,
}

pub struct ReviewDecision {
    pub proposal_id: ProposalId,
    pub accepted: Vec<CandidateId>,
    pub rejected: Vec<CandidateId>,
    /// Candidates the user edited on the canvas before applying (subproject 6
    /// mutates transient state and records the final edit here).
    pub modified: Vec<(CandidateId, ProposedEdit)>,
    pub resulting_document_state_id: u64,  // ImageDocument::state_id() after apply
}
```

### 5.1 Lowering

```rust
/// Produce the document-level ops for an accepted decision: for each accepted
/// (or modified) candidate, take its ProposedEdit (modified overrides original)
/// and convert to the matching EditOp, in candidate order. Rejected candidates
/// are dropped. Returns the batch to hand to ImageDocument::apply_batch.
pub fn lower(proposal: &EditProposal, decision: &ReviewDecision) -> Vec<EditOp>;
```

### 5.2 Policy validation (spec §9.4 product limits)

```rust
pub struct PolicyLimits {
    pub max_candidates: u32,
    pub max_total_area_fraction: f32,  // total redaction area / image area
    pub allow_out_of_bounds: bool,     // false => reject candidates outside image
}
pub enum PolicyError {
    TooManyCandidates { count: u32, max: u32 },
    ExcessiveTotalArea { fraction: f32, max: f32 },
    OutOfBounds { candidate: CandidateId },
}
pub fn validate_policy(
    candidates: &[ProposedCandidate],
    limits: &PolicyLimits,
    image_dims: (u32, u32),
) -> Result<(), PolicyError>;
```

## 6. §9.4 Validation Split

| Rule | Layer |
|---|---|
| Non-finite coordinate, zero-area, unknown id, wrong kind, empty text | `rollshot-image-document` `EditError` (per-op, atomic, in `apply_batch`) |
| Excessive candidate count, excessive total redaction area, out-of-bounds reject policy | `rollshot-edit-proposal` `validate_policy` (product policy) |

Out-of-bounds: the document layer deterministically **clamps** (existing behavior, satisfies §9.4 "deterministic clamping"); the proposal layer can additionally **reject** out-of-bounds candidates when `allow_out_of_bounds == false`.

## 7. Data Flow (how the layers compose; full wiring is subprojects 4/6)

1. Agent run (subproject 4) produces an `EditProposal` with `base_document_state_id = doc.state_id()`.
2. `validate_policy` runs on the candidate set (product limits).
3. Review (subproject 6): user accepts/rejects/edits candidates → builds a `ReviewDecision` from transient candidate state.
4. `lower(proposal, decision)` → `Vec<EditOp>`.
5. `doc.apply_batch(ops)` → one undo entry; capture `resulting_document_state_id = doc.state_id()` into the `ReviewDecision`.
6. A single `doc.undo()` cleanly reverts the entire applied proposal.
7. The `ReviewDecision` (accepted/rejected/modified) is retained as Improve-Preset evidence (subproject 7).

## 8. Decisions (approved)

1. **Crate boundary:** split; new sibling crate `rollshot-edit-proposal`; document layer stays agent-free.
2. **`EditOp` granularity:** full CRUD over the three annotation kinds (general batch primitive).
3. **`apply_batch` is atomic** (all-or-nothing; no partial application).
4. **`Update*`/`Delete` reference pre-batch ids only** (explicit constraint).
5. **Out-of-bounds:** clamp at the document layer; reject-policy at the proposal layer.
6. `base_document_state_id` / one-step undo are satisfied by the single `commit()` in `apply_batch` — no separate snapshot storage.

## 9. Testing (spec §11.1)

`rollshot-image-document`:
- `apply_batch` of N adds creates exactly **one** undo entry; `undo()` once restores the exact pre-batch annotations + `next_number` + `state_id`.
- **Atomicity:** a batch containing one invalid op (zero-area / non-finite / unknown id / wrong kind / empty text) mutates nothing and leaves `state_id` unchanged.
- Batch containing a `Delete` of a `NumberCallout` renumbers remaining callouts compactly within the single entry.
- Each CRUD path (`Add*`, `Update*`, `Delete`) applies correctly in a batch.
- `BatchOutcome.added_ids` lists the new ids in op order.
- Empty batch is a no-op with no history entry.
- `NonFiniteCoordinate` rejection.

`rollshot-edit-proposal`:
- `lower()` includes accepted, applies `modified` overrides, drops `rejected`, preserves order.
- `validate_policy`: count limit, total-area limit, out-of-bounds (allow vs reject).
- `ConfidenceSummary` aggregation.
- `ReviewDecision` round-trips accepted/rejected/modified consistently with the proposal.
- `serde` round-trip of the persisted types (for subproject 5).

## 10. Dependencies & Interfaces Out

- `rollshot-image-document` exposes `EditOp`, `BatchOutcome`, `apply_batch` for `rollshot-edit-proposal` and (later) `rollshot-app`.
- `rollshot-edit-proposal` exposes the proposal/review model + `lower` + `validate_policy` for subprojects 4 (produces proposals), 5 (persists them), 6 (reviews + applies), 7 (Improve Preset).
- No change to `rollshot-image-document`'s existing single-op API; `apply_batch` is additive.
- Workspace MSRV is 1.94 (already on `main`); the new crate inherits `rust-version.workspace = true`.
