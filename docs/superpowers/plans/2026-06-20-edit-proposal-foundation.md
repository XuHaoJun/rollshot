# Visual Edit Proposal Foundation (Subproject 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an atomic batch-edit transaction to `rollshot-image-document` (many operations → one undo entry) and a new `rollshot-edit-proposal` crate holding the agent-flavored review model, lowering, and product-policy validation — without putting any agent concerns into the headless document crate.

**Architecture:** Two layers across a crate boundary. `rollshot-image-document` gains a typed `EditOp` (full CRUD) and `apply_batch()` that reuses the existing private `snapshot()`/`commit()` to produce exactly one history entry; it stays headless/agent-free and keeps owning validation + final mutation. The new `rollshot-edit-proposal` crate owns `EditProposal`/`ProposedCandidate`/`ProposedEdit`/`ReviewDecision`, lowers an accepted decision to `Vec<EditOp>`, and enforces count/area/out-of-bounds product policy.

**Tech Stack:** Rust (workspace MSRV 1.94), `thiserror`, `serde` (optional feature), `image::RgbaImage`. Spec: `docs/superpowers/specs/2026-06-20-edit-proposal-foundation-design.md`.

## Global Constraints

- **Workspace MSRV is 1.94** (already declared in root `Cargo.toml`); the new crate uses `rust-version.workspace = true`. Do not change MSRV.
- **`rollshot-image-document` stays headless / agent-free.** It learns nothing about confidence, provenance, rationale, candidates, or agents. Its only new public surface is `EditOp`, `BatchOutcome`, `apply_batch`, an extra `EditError` variant, `is_finite()` helpers, and an optional `serde` feature on POD types.
- **`apply_batch` is atomic** — all-or-nothing: any failing op rolls back the whole batch (no commit, no `state_id` change).
- **One reviewed proposal = one undo entry**: `apply_batch` snapshots once and commits once; a single `undo()` reverts the whole batch.
- **`Update*`/`Delete` reference annotations that exist before the batch** (validation resolves ids against pre-batch state).
- **`rollshot-edit-proposal` must not depend on iced/app**; it depends on `rollshot-image-document` (with `features = ["serde"]`), `serde`, `thiserror`.
- Shell commands are prefixed with `rtk` (AGENTS.md §6). Tests: `rtk cargo test -p <crate>`.
- TDD: write the failing test, see it fail, implement minimally, see it pass, commit.

---

## File Structure

`crates/rollshot-image-document/` (modify):
- `src/edit_op.rs` (Create) — `EditOp` enum + `BatchOutcome`.
- `src/document.rs` (Modify) — `EditError::NonFiniteCoordinate`; `apply_batch` + private `apply_one`; free `ensure_point_finite`/`ensure_rect_finite`.
- `src/geometry.rs` (Modify) — `ImagePoint::is_finite`, `ImageRect::is_finite`; optional-serde derives on `ImagePoint`, `ImageRect`.
- `src/annotation.rs` (Modify) — optional-serde derive on `AnnotationId`.
- `src/lib.rs` (Modify) — export `EditOp`, `BatchOutcome`.
- `Cargo.toml` (Modify) — optional `serde` dependency + `serde` feature.

`crates/rollshot-edit-proposal/` (Create — new crate):
- `Cargo.toml`
- `src/lib.rs` — module wiring + exports.
- `src/proposal.rs` — `CandidateId`, `ProposalId`, `ProposedEdit`, `Provenance`, `ProvenanceSource`, `ConfidenceSummary`, `ProposedCandidate`, `EditProposal`.
- `src/review.rs` — `ReviewDecision`, `lower()`.
- `src/policy.rs` — `PolicyLimits`, `PolicyError`, `validate_policy()`.

`Cargo.toml` (root, Modify) — add `crates/rollshot-edit-proposal` to `members`.

---

## Task 1: Document-layer types — `EditOp`, `BatchOutcome`, `EditError::NonFiniteCoordinate`, `is_finite`, optional serde

**Files:**
- Create: `crates/rollshot-image-document/src/edit_op.rs`
- Modify: `crates/rollshot-image-document/src/document.rs` (EditError enum)
- Modify: `crates/rollshot-image-document/src/geometry.rs` (is_finite + serde derives)
- Modify: `crates/rollshot-image-document/src/annotation.rs` (serde derive on AnnotationId)
- Modify: `crates/rollshot-image-document/src/lib.rs` (exports)
- Modify: `crates/rollshot-image-document/Cargo.toml` (serde feature)

**Interfaces:**
- Consumes: existing `AnnotationId`, `ImagePoint`, `ImageRect` from this crate.
- Produces: `pub enum EditOp { AddRedaction{bounds:ImageRect}, AddTextNote{position:ImagePoint,text:String}, AddNumberCallout{tip:ImagePoint,bubble:ImagePoint}, UpdateRedactionBounds{id:AnnotationId,bounds:ImageRect}, UpdateTextPosition{id:AnnotationId,position:ImagePoint}, UpdateText{id:AnnotationId,text:String}, UpdateNumberPoints{id:AnnotationId,tip:ImagePoint,bubble:ImagePoint}, Delete{id:AnnotationId} }`; `pub struct BatchOutcome { pub added_ids: Vec<AnnotationId> }`; `EditError::NonFiniteCoordinate`; `ImagePoint::is_finite(&self)->bool`; `ImageRect::is_finite(&self)->bool`; optional `serde` feature.

- [ ] **Step 1: Add the `serde` feature to `Cargo.toml`**

In `crates/rollshot-image-document/Cargo.toml`, add an optional serde dependency and a feature. Add under `[dependencies]`:

```toml
serde = { workspace = true, optional = true }
```

Add (or extend) a `[features]` table:

```toml
[features]
serde = ["dep:serde"]
```

- [ ] **Step 2: Write the failing test (types + is_finite + serde round-trip)**

Append to the existing `#[cfg(test)] mod tests` in `crates/rollshot-image-document/src/document.rs` (it already imports the crate). Add:

```rust
#[test]
fn edit_op_variants_construct_and_compare() {
    let a = EditOp::AddRedaction {
        bounds: ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 10.0)),
    };
    let b = a.clone();
    assert_eq!(a, b);
    let outcome = BatchOutcome { added_ids: vec![AnnotationId(1)] };
    assert_eq!(outcome.added_ids, vec![AnnotationId(1)]);
}

#[test]
fn non_finite_coordinate_error_has_message() {
    assert_eq!(
        EditError::NonFiniteCoordinate.to_string(),
        "coordinates must be finite"
    );
}

#[test]
fn is_finite_detects_nan_and_infinity() {
    assert!(ImagePoint::new(1.0, 2.0).is_finite());
    assert!(!ImagePoint::new(f32::NAN, 2.0).is_finite());
    let good = ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(4.0, 4.0));
    assert!(good.is_finite());
    let bad = ImageRect { x: 0.0, y: 0.0, width: f32::INFINITY, height: 4.0 };
    assert!(!bad.is_finite());
}
```

Note: `ImagePoint` and `ImageRect` fields (`x`, `y`, `width`, `height`) are `pub` (verified in `geometry.rs`), so the struct literal compiles.

- [ ] **Step 3: Run the test — verify it fails**

Run: `rtk cargo test -p rollshot-image-document edit_op_variants_construct_and_compare`
Expected: FAIL to compile — `EditOp`, `BatchOutcome`, `is_finite`, `EditError::NonFiniteCoordinate` not found.

- [ ] **Step 4: Create `EditOp` + `BatchOutcome`**

Create `crates/rollshot-image-document/src/edit_op.rs`:

```rust
//! Typed, agent-free document edit operations and their batch outcome.
//! Applied atomically by `ImageDocument::apply_batch` (spec §6.5).

use crate::annotation::AnnotationId;
use crate::geometry::{ImagePoint, ImageRect};

/// A single document mutation. Add* allocate new ids; Update*/Delete reference
/// annotations that exist BEFORE the batch is applied.
#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    AddRedaction { bounds: ImageRect },
    AddTextNote { position: ImagePoint, text: String },
    AddNumberCallout { tip: ImagePoint, bubble: ImagePoint },
    UpdateRedactionBounds { id: AnnotationId, bounds: ImageRect },
    UpdateTextPosition { id: AnnotationId, position: ImagePoint },
    UpdateText { id: AnnotationId, text: String },
    UpdateNumberPoints { id: AnnotationId, tip: ImagePoint, bubble: ImagePoint },
    Delete { id: AnnotationId },
}

/// Result of a successful `apply_batch`: ids allocated for the Add* ops, in the
/// order those ops appeared in the batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchOutcome {
    pub added_ids: Vec<AnnotationId>,
}
```

- [ ] **Step 5: Add `EditError::NonFiniteCoordinate`**

In `crates/rollshot-image-document/src/document.rs`, add the variant to the existing `EditError` enum (after `WrongKind`):

```rust
    #[error("operation does not apply to this annotation kind")]
    WrongKind,
    #[error("coordinates must be finite")]
    NonFiniteCoordinate,
```

- [ ] **Step 6: Add `is_finite` + optional serde derives in `geometry.rs`**

In `crates/rollshot-image-document/src/geometry.rs`, add a `serde` cfg derive to `ImagePoint` and `ImageRect` (alongside their existing derives), e.g.:

```rust
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
```

And add methods (inside the respective `impl` blocks):

```rust
impl ImagePoint {
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl ImageRect {
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.width.is_finite() && self.height.is_finite()
    }
}
```

(If `is_finite` would land in an existing `impl ImagePoint`/`impl ImageRect` block, add the method there rather than a second block.)

- [ ] **Step 7: Add optional serde derive on `AnnotationId`**

In `crates/rollshot-image-document/src/annotation.rs`, add to `AnnotationId`'s derives:

```rust
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
```

- [ ] **Step 8: Export `EditOp` + `BatchOutcome`**

In `crates/rollshot-image-document/src/lib.rs`, add the module + export:

```rust
mod edit_op;
pub use edit_op::{BatchOutcome, EditOp};
```

(Place `mod edit_op;` with the other `mod` declarations and the `pub use` with the others.)

- [ ] **Step 9: Run tests — verify pass (default + serde feature)**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS (all existing + the 3 new tests).

Run: `rtk cargo test -p rollshot-image-document --features serde`
Expected: PASS (confirms the serde derives compile).

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): add EditOp, BatchOutcome, NonFiniteCoordinate, is_finite, optional serde"
```

---

## Task 2: `ImageDocument::apply_batch` — atomic batch transaction (one undo entry)

**Files:**
- Modify: `crates/rollshot-image-document/src/document.rs`

**Interfaces:**
- Consumes: `EditOp`, `BatchOutcome` (Task 1); existing private `snapshot()`, `commit()`, `restore()`, `allocate_id()`, `annotation_index()`, `renumber_compactly()`.
- Produces: `ImageDocument::apply_batch(&mut self, ops: Vec<EditOp>) -> Result<BatchOutcome, EditError>`.

- [ ] **Step 1: Write the failing tests**

Append to `#[cfg(test)] mod tests` in `crates/rollshot-image-document/src/document.rs`:

```rust
fn test_doc() -> ImageDocument {
    ImageDocument::new(image::RgbaImage::new(100, 100))
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
    ImageRect::from_corners(ImagePoint::new(x, y), ImagePoint::new(x + w, y + h))
}

#[test]
fn apply_batch_of_adds_is_one_undo_entry() {
    let mut d = test_doc();
    let s_before = d.state_id();
    let out = d
        .apply_batch(vec![
            EditOp::AddRedaction { bounds: rect(0.0, 0.0, 10.0, 10.0) },
            EditOp::AddRedaction { bounds: rect(20.0, 20.0, 10.0, 10.0) },
            EditOp::AddRedaction { bounds: rect(40.0, 40.0, 10.0, 10.0) },
        ])
        .expect("valid batch");
    assert_eq!(out.added_ids.len(), 3);
    assert_eq!(d.annotations().len(), 3);
    assert_eq!(d.state_id(), s_before + 1, "exactly one commit for the whole batch");
    // ONE undo restores the EXACT pre-batch state (annotations + next_number + state_id).
    assert!(d.undo());
    assert_eq!(d.annotations().len(), 0);
    assert_eq!(d.next_number(), 1, "next_number restored");
    assert_eq!(d.state_id(), 0, "state_id restored to pre-batch");
    assert!(!d.can_undo());
}

#[test]
fn apply_batch_is_atomic_on_invalid_op() {
    let mut d = test_doc();
    let state_before = d.state_id();
    let err = d
        .apply_batch(vec![
            EditOp::AddRedaction { bounds: rect(0.0, 0.0, 10.0, 10.0) },
            EditOp::AddRedaction { bounds: rect(0.0, 0.0, 0.0, 0.0) }, // zero area -> reject whole batch
        ])
        .unwrap_err();
    assert_eq!(err, EditError::ZeroArea);
    assert_eq!(d.annotations().len(), 0, "no partial mutation");
    assert_eq!(d.state_id(), state_before, "state_id unchanged");
    assert!(!d.can_undo());
}

#[test]
fn apply_batch_rejects_non_finite() {
    let mut d = test_doc();
    let err = d
        .apply_batch(vec![EditOp::AddRedaction {
            bounds: ImageRect { x: f32::NAN, y: 0.0, width: 5.0, height: 5.0 },
        }])
        .unwrap_err();
    assert_eq!(err, EditError::NonFiniteCoordinate);
    assert_eq!(d.annotations().len(), 0);
}

#[test]
fn apply_batch_empty_is_noop_without_history() {
    let mut d = test_doc();
    let out = d.apply_batch(vec![]).expect("empty ok");
    assert!(out.added_ids.is_empty());
    assert!(!d.can_undo());
    assert_eq!(d.state_id(), 0);
}

#[test]
fn apply_batch_crud_and_callout_renumber_in_one_entry() {
    let mut d = test_doc();
    // Seed two callouts (numbers 1, 2) and one redaction via the batch path.
    let seed = d
        .apply_batch(vec![
            EditOp::AddNumberCallout { tip: ImagePoint::new(1.0, 1.0), bubble: ImagePoint::new(2.0, 2.0) },
            EditOp::AddNumberCallout { tip: ImagePoint::new(3.0, 3.0), bubble: ImagePoint::new(4.0, 4.0) },
            EditOp::AddRedaction { bounds: rect(5.0, 5.0, 5.0, 5.0) },
        ])
        .expect("seed");
    let callout1 = seed.added_ids[0];
    let red = seed.added_ids[2];
    // Batch: delete callout #1 (forces renumber) + move the redaction. One entry.
    d.apply_batch(vec![
        EditOp::Delete { id: callout1 },
        EditOp::UpdateRedactionBounds { id: red, bounds: rect(50.0, 50.0, 8.0, 8.0) },
    ])
    .expect("crud batch");
    // Remaining callout renumbered to 1.
    let remaining_numbers: Vec<u32> = d
        .annotations()
        .iter()
        .filter_map(|a| match a {
            Annotation::NumberCallout { number, .. } => Some(*number),
            _ => None,
        })
        .collect();
    assert_eq!(remaining_numbers, vec![1], "exactly one callout, renumbered to 1");
    // One undo reverts BOTH the delete and the update.
    assert!(d.undo());
    assert_eq!(d.annotations().len(), 3);
}

#[test]
fn apply_batch_unknown_id_rejected() {
    let mut d = test_doc();
    let err = d
        .apply_batch(vec![EditOp::Delete { id: AnnotationId(999) }])
        .unwrap_err();
    assert_eq!(err, EditError::UnknownAnnotation);
}

#[test]
fn apply_batch_wrong_kind_rejected() {
    let mut d = test_doc();
    let id = d.add_redaction(rect(0.0, 0.0, 5.0, 5.0)).unwrap();
    let err = d
        .apply_batch(vec![EditOp::UpdateText { id, text: "x".into() }])
        .unwrap_err();
    assert_eq!(err, EditError::WrongKind);
    assert_eq!(d.annotations().len(), 1, "no mutation on reject");
}

#[test]
fn apply_batch_added_ids_follow_op_order() {
    let mut d = test_doc();
    let out = d
        .apply_batch(vec![
            EditOp::AddRedaction { bounds: rect(0.0, 0.0, 5.0, 5.0) },
            EditOp::AddTextNote { position: ImagePoint::new(2.0, 2.0), text: "a".into() },
            EditOp::AddNumberCallout { tip: ImagePoint::new(3.0, 3.0), bubble: ImagePoint::new(4.0, 4.0) },
        ])
        .expect("valid mixed adds");
    let live: Vec<_> = d.annotations().iter().map(|a| a.id()).collect();
    assert_eq!(out.added_ids, live, "added_ids match created annotations in op order");
    assert!(out.added_ids[0] < out.added_ids[1] && out.added_ids[1] < out.added_ids[2]);
}

#[test]
fn apply_batch_rejects_empty_text_atomically() {
    let mut d = test_doc();
    let err = d
        .apply_batch(vec![
            EditOp::AddRedaction { bounds: rect(0.0, 0.0, 5.0, 5.0) },
            EditOp::AddTextNote { position: ImagePoint::new(1.0, 1.0), text: "   ".into() },
        ])
        .unwrap_err();
    assert_eq!(err, EditError::EmptyText);
    assert_eq!(d.annotations().len(), 0, "whole batch rolled back");
    assert!(!d.can_undo());
}

#[test]
fn apply_batch_update_text_empty_rejected() {
    let mut d = test_doc();
    let id = d.add_text_note(ImagePoint::new(2.0, 2.0), "orig".into()).unwrap();
    let err = d.apply_batch(vec![EditOp::UpdateText { id, text: "  ".into() }]).unwrap_err();
    assert_eq!(err, EditError::EmptyText);
    match d.annotation(id).unwrap() {
        Annotation::TextNote { text, .. } => assert_eq!(text, "orig"),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn apply_batch_add_callout_rejects_non_finite() {
    let mut d = test_doc();
    let err = d
        .apply_batch(vec![EditOp::AddNumberCallout {
            tip: ImagePoint::new(f32::INFINITY, 1.0),
            bubble: ImagePoint::new(2.0, 2.0),
        }])
        .unwrap_err();
    assert_eq!(err, EditError::NonFiniteCoordinate);
    assert_eq!(d.annotations().len(), 0);
}

#[test]
fn apply_batch_exercises_text_and_callout_update_paths() {
    let mut d = test_doc();
    let seed = d
        .apply_batch(vec![
            EditOp::AddTextNote { position: ImagePoint::new(5.0, 5.0), text: "old".into() },
            EditOp::AddNumberCallout { tip: ImagePoint::new(1.0, 1.0), bubble: ImagePoint::new(2.0, 2.0) },
        ])
        .expect("seed");
    let text_id = seed.added_ids[0];
    let callout_id = seed.added_ids[1];
    d.apply_batch(vec![
        EditOp::UpdateText { id: text_id, text: "new".into() },
        EditOp::UpdateTextPosition { id: text_id, position: ImagePoint::new(9.0, 9.0) },
        EditOp::UpdateNumberPoints { id: callout_id, tip: ImagePoint::new(7.0, 7.0), bubble: ImagePoint::new(8.0, 8.0) },
    ])
    .expect("updates");
    match d.annotation(text_id).unwrap() {
        Annotation::TextNote { text, position } => {
            assert_eq!(text, "new");
            assert_eq!(*position, ImagePoint::new(9.0, 9.0));
        }
        _ => panic!("wrong kind"),
    }
    match d.annotation(callout_id).unwrap() {
        Annotation::NumberCallout { tip, bubble, .. } => {
            assert_eq!(*tip, ImagePoint::new(7.0, 7.0));
            assert_eq!(*bubble, ImagePoint::new(8.0, 8.0));
        }
        _ => panic!("wrong kind"),
    }
}
```

- [ ] **Step 2: Run tests — verify they fail**

Run: `rtk cargo test -p rollshot-image-document apply_batch`
Expected: FAIL to compile — `apply_batch` not found.

- [ ] **Step 3: Implement `apply_batch` + helpers**

In `crates/rollshot-image-document/src/document.rs`, add the import at the top (with the other `use crate::` lines):

```rust
use crate::edit_op::{BatchOutcome, EditOp};
```

Add these module-private free functions (near the top of the file, after the imports):

```rust
fn ensure_point_finite(p: &ImagePoint) -> Result<(), EditError> {
    if p.is_finite() { Ok(()) } else { Err(EditError::NonFiniteCoordinate) }
}

fn ensure_rect_finite(r: &ImageRect) -> Result<(), EditError> {
    if r.is_finite() { Ok(()) } else { Err(EditError::NonFiniteCoordinate) }
}
```

Add these methods inside `impl ImageDocument` (next to the other mutation methods):

```rust
    /// Apply many operations as ONE history entry (spec §6.5). Atomic: if any
    /// op is invalid the whole batch is rolled back — no mutation, no commit,
    /// no `state_id` change. Update*/Delete reference annotations existing
    /// before the batch. An empty batch is a no-op with no history entry.
    ///
    /// ```text
    /// ops.is_empty()? --yes--> Ok(BatchOutcome::default)   (no snapshot/commit)
    ///        | no
    ///        v
    ///   snapshot(before) once
    ///        v
    ///   for op in ops: apply_one(op)
    ///        |                       \
    ///     all Ok                    first Err(e)
    ///        v                            v
    ///   (callout deleted? renumber)  restore(before)   (no commit; state_id unchanged)
    ///        v                            v
    ///   commit(before) once            Err(e)
    ///        v
    ///   Ok(BatchOutcome { added_ids })
    /// ```
    pub fn apply_batch(&mut self, ops: Vec<EditOp>) -> Result<BatchOutcome, EditError> {
        if ops.is_empty() {
            return Ok(BatchOutcome::default());
        }
        let (w, h) = self.source.dimensions();
        let before = self.snapshot();
        let mut added_ids = Vec::new();
        let mut deleted_callout = false;
        let mut failure: Option<EditError> = None;
        for op in ops {
            if let Err(e) = self.apply_one(op, w, h, &mut added_ids, &mut deleted_callout) {
                failure = Some(e);
                break;
            }
        }
        if let Some(e) = failure {
            self.restore(before);
            return Err(e);
        }
        if deleted_callout {
            self.renumber_compactly();
        }
        self.commit(before);
        Ok(BatchOutcome { added_ids })
    }

    fn apply_one(
        &mut self,
        op: EditOp,
        w: u32,
        h: u32,
        added_ids: &mut Vec<AnnotationId>,
        deleted_callout: &mut bool,
    ) -> Result<(), EditError> {
        match op {
            EditOp::AddRedaction { bounds } => {
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.is_empty() {
                    return Err(EditError::ZeroArea);
                }
                let id = self.allocate_id();
                self.annotations.push(Annotation::OpaqueRedaction { id, bounds: clamped });
                added_ids.push(id);
            }
            EditOp::AddTextNote { position, text } => {
                ensure_point_finite(&position)?;
                if text.trim().is_empty() {
                    return Err(EditError::EmptyText);
                }
                let id = self.allocate_id();
                self.annotations.push(Annotation::TextNote { id, position: position.clamp_to(w, h), text });
                added_ids.push(id);
            }
            EditOp::AddNumberCallout { tip, bubble } => {
                ensure_point_finite(&tip)?;
                ensure_point_finite(&bubble)?;
                let id = self.allocate_id();
                let number = self.next_number;
                self.next_number += 1;
                self.annotations.push(Annotation::NumberCallout {
                    id,
                    number,
                    tip: tip.clamp_to(w, h),
                    bubble: bubble.clamp_to(w, h),
                });
                added_ids.push(id);
            }
            EditOp::UpdateRedactionBounds { id, bounds } => {
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.is_empty() {
                    return Err(EditError::ZeroArea);
                }
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::OpaqueRedaction { bounds: b, .. } => *b = clamped,
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateTextPosition { id, position } => {
                ensure_point_finite(&position)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::TextNote { position: p, .. } => *p = position.clamp_to(w, h),
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateText { id, text } => {
                if text.trim().is_empty() {
                    return Err(EditError::EmptyText);
                }
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::TextNote { text: t, .. } => *t = text,
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateNumberPoints { id, tip, bubble } => {
                ensure_point_finite(&tip)?;
                ensure_point_finite(&bubble)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::NumberCallout { tip: t, bubble: b, .. } => {
                        *t = tip.clamp_to(w, h);
                        *b = bubble.clamp_to(w, h);
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::Delete { id } => {
                let index = self.annotation_index(id)?;
                let removed = self.annotations.remove(index);
                if matches!(removed, Annotation::NumberCallout { .. }) {
                    *deleted_callout = true;
                }
            }
        }
        Ok(())
    }
```

Atomicity note: `apply_one` validates each op before mutating for that op; if a later op fails, `restore(before)` rolls back any earlier ops' mutations. `next_number` is captured in the snapshot, so `restore()` rolls it back correctly. Only `next_id` advances for any allocated ids even on rollback — acceptable, ids are monotonic and never reused (matches the existing id-stability invariant).

- [ ] **Step 4: Run tests — verify pass**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS (all existing + the 7 new `apply_batch` tests).

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document/src/document.rs
rtk git commit -m "feat(image-document): atomic apply_batch — one undo entry for a batch of EditOps"
```

---

## Task 3: New crate `rollshot-edit-proposal` — proposal model

**Files:**
- Create: `crates/rollshot-edit-proposal/Cargo.toml`
- Create: `crates/rollshot-edit-proposal/src/lib.rs`
- Create: `crates/rollshot-edit-proposal/src/proposal.rs`
- Modify: `Cargo.toml` (root workspace members)

**Interfaces:**
- Consumes: `rollshot_image_document::{AnnotationId, ImagePoint, ImageRect}` (with `serde` feature).
- Produces: `CandidateId(pub u64)`, `ProposalId(pub u64)`, `ProvenanceSource`, `Provenance`, `ConfidenceSummary` (+ `ConfidenceSummary::from_confidences(&[f32])`), `ProposedEdit` (CRUD mirror of `EditOp`, with `to_edit_op(&self)->EditOp`), `ProposedCandidate`, `EditProposal`. All `serde`-derived.

- [ ] **Step 1: Create the crate manifest**

Create `crates/rollshot-edit-proposal/Cargo.toml`:

```toml
[package]
name = "rollshot-edit-proposal"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }
serde = { workspace = true }
thiserror = { workspace = true }

[lints]
workspace = true
```

(`[lints] workspace = true` matches every other workspace crate — it opts the new crate into the workspace lint policy, incl. `unsafe_code = "forbid"`; without it the Task 5 `cargo clippy --workspace` gate would silently skip it.)

- [ ] **Step 2: Register the crate in the workspace**

In the root `Cargo.toml`, add to `[workspace] members`:

```toml
    "crates/rollshot-edit-proposal",
```

- [ ] **Step 3: Write the failing test**

Create `crates/rollshot-edit-proposal/src/proposal.rs` with ONLY this test module at first (so the test names resolve once types exist — add the test now, types in Step 5):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_image_document::{EditOp, ImagePoint, ImageRect};

    #[test]
    fn proposed_edit_lowers_to_matching_edit_op() {
        let r = ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(8.0, 8.0));
        let pe = ProposedEdit::AddRedaction { bounds: r };
        assert_eq!(pe.to_edit_op(), EditOp::AddRedaction { bounds: r });
    }

    #[test]
    fn confidence_summary_aggregates() {
        let s = ConfidenceSummary::from_confidences(&[0.2, 0.8, 0.5]);
        assert_eq!(s.count, 3);
        assert!((s.min - 0.2).abs() < 1e-6);
        assert!((s.max - 0.8).abs() < 1e-6);
        assert!((s.mean - 0.5).abs() < 1e-6);
    }

    #[test]
    fn proposal_serde_round_trip() {
        let r = ImageRect::from_corners(ImagePoint::new(1.0, 1.0), ImagePoint::new(9.0, 9.0));
        let proposal = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 7,
            candidates: vec![ProposedCandidate {
                id: CandidateId(1),
                edit: ProposedEdit::AddRedaction { bounds: r },
                confidence: 0.9,
                rationale: Some("matches email pattern".into()),
                provenance: Provenance { source: ProvenanceSource::Agent { run_id: 42 } },
            }],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Agent { run_id: 42 } },
        };
        let json = serde_json::to_string(&proposal).unwrap();
        let back: EditProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, proposal.id);
        assert_eq!(back.candidates.len(), 1);
    }

    #[test]
    fn confidence_summary_empty_slice_is_zeros() {
        let s = ConfidenceSummary::from_confidences(&[]);
        assert_eq!(s, ConfidenceSummary { min: 0.0, max: 0.0, mean: 0.0, count: 0 });
    }

    #[test]
    fn proposed_edit_lowers_remaining_variants() {
        use rollshot_image_document::AnnotationId;
        let p = ImagePoint::new(1.0, 2.0);
        let r = ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(8.0, 8.0));
        assert_eq!(
            ProposedEdit::AddTextNote { position: p, text: "x".into() }.to_edit_op(),
            EditOp::AddTextNote { position: p, text: "x".into() }
        );
        assert_eq!(
            ProposedEdit::AddNumberCallout { tip: p, bubble: p }.to_edit_op(),
            EditOp::AddNumberCallout { tip: p, bubble: p }
        );
        assert_eq!(
            ProposedEdit::UpdateRedactionBounds { id: AnnotationId(1), bounds: r }.to_edit_op(),
            EditOp::UpdateRedactionBounds { id: AnnotationId(1), bounds: r }
        );
        assert_eq!(
            ProposedEdit::UpdateTextPosition { id: AnnotationId(2), position: p }.to_edit_op(),
            EditOp::UpdateTextPosition { id: AnnotationId(2), position: p }
        );
        assert_eq!(
            ProposedEdit::UpdateText { id: AnnotationId(3), text: "y".into() }.to_edit_op(),
            EditOp::UpdateText { id: AnnotationId(3), text: "y".into() }
        );
        assert_eq!(
            ProposedEdit::UpdateNumberPoints { id: AnnotationId(4), tip: p, bubble: p }.to_edit_op(),
            EditOp::UpdateNumberPoints { id: AnnotationId(4), tip: p, bubble: p }
        );
        assert_eq!(
            ProposedEdit::Delete { id: AnnotationId(5) }.to_edit_op(),
            EditOp::Delete { id: AnnotationId(5) }
        );
    }
}
```

Add `serde_json` as a dev-dependency in `crates/rollshot-edit-proposal/Cargo.toml`:

```toml
[dev-dependencies]
serde_json = { workspace = true }
```

- [ ] **Step 4: Run the test — verify it fails**

Run: `rtk cargo test -p rollshot-edit-proposal`
Expected: FAIL to compile — `ProposedEdit`/`ConfidenceSummary`/`EditProposal`/`ProposedCandidate`/`Provenance`/`ProvenanceSource`/`CandidateId`/`ProposalId` not found in scope (the test module references types not yet defined in `proposal.rs`; they are prepended in Step 5).

- [ ] **Step 5: Implement the proposal types**

Prepend to `crates/rollshot-edit-proposal/src/proposal.rs` (above the test module):

```rust
//! Agent-flavored edit-proposal model (spec §6.3). Framework-neutral; lowers to
//! `rollshot_image_document::EditOp` on accept. No agent/LLM or UI code here.

use rollshot_image_document::{AnnotationId, EditOp, ImagePoint, ImageRect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CandidateId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProposalId(pub u64);

/// Where a proposal/candidate came from. Privacy-safe: ids/counts only, never prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProvenanceSource {
    Manual,
    Agent { run_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: ProvenanceSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceSummary {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    pub count: u32,
}

impl ConfidenceSummary {
    /// Aggregate a candidate set's confidences. An empty slice yields zeros.
    pub fn from_confidences(values: &[f32]) -> Self {
        if values.is_empty() {
            return Self { min: 0.0, max: 0.0, mean: 0.0, count: 0 };
        }
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut sum = 0.0f32;
        for &v in values {
            min = min.min(v);
            max = max.max(v);
            sum += v;
        }
        Self { min, max, mean: sum / values.len() as f32, count: values.len() as u32 }
    }
}

/// What document change a candidate proposes. Mirrors `EditOp`; v1 mainly
/// produces `AddRedaction`. Lowers to `EditOp` via `to_edit_op`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl ProposedEdit {
    /// Lower this proposal-level edit to the document-level `EditOp`.
    pub fn to_edit_op(&self) -> EditOp {
        match self {
            ProposedEdit::AddRedaction { bounds } => EditOp::AddRedaction { bounds: *bounds },
            ProposedEdit::AddTextNote { position, text } => {
                EditOp::AddTextNote { position: *position, text: text.clone() }
            }
            ProposedEdit::AddNumberCallout { tip, bubble } => {
                EditOp::AddNumberCallout { tip: *tip, bubble: *bubble }
            }
            ProposedEdit::UpdateRedactionBounds { id, bounds } => {
                EditOp::UpdateRedactionBounds { id: *id, bounds: *bounds }
            }
            ProposedEdit::UpdateTextPosition { id, position } => {
                EditOp::UpdateTextPosition { id: *id, position: *position }
            }
            ProposedEdit::UpdateText { id, text } => {
                EditOp::UpdateText { id: *id, text: text.clone() }
            }
            ProposedEdit::UpdateNumberPoints { id, tip, bubble } => {
                EditOp::UpdateNumberPoints { id: *id, tip: *tip, bubble: *bubble }
            }
            ProposedEdit::Delete { id } => EditOp::Delete { id: *id },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedCandidate {
    pub id: CandidateId,
    pub edit: ProposedEdit,
    pub confidence: f32,
    pub rationale: Option<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditProposal {
    pub id: ProposalId,
    /// `ImageDocument::state_id()` captured before the proposal is applied
    /// (provenance/staleness — recovery is via the single undo entry).
    pub base_document_state_id: u64,
    pub candidates: Vec<ProposedCandidate>,
    pub confidence_summary: ConfidenceSummary,
    pub rationale_summary: Option<String>,
    pub provenance: Provenance,
}
```

Create `crates/rollshot-edit-proposal/src/lib.rs`:

```rust
//! Visual edit-proposal foundation (spec §6.3): the review model that lowers to
//! `rollshot_image_document::EditOp`. No agent/LLM, UI, or capture code.

mod proposal;

pub use proposal::{
    CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
    Provenance, ProvenanceSource,
};
```

Confirm `serde_json` exists in `[workspace.dependencies]` of the root `Cargo.toml`; it does (used elsewhere). If `thiserror`/`serde`/`serde_json` are not yet workspace deps, add them — but they already are.

- [ ] **Step 6: Run tests — verify pass**

Run: `rtk cargo test -p rollshot-edit-proposal`
Expected: PASS (3 tests).

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-edit-proposal Cargo.toml
rtk git commit -m "feat(edit-proposal): new crate with EditProposal/ProposedCandidate/ProposedEdit model"
```

---

## Task 4: `ReviewDecision` + `lower()`

**Files:**
- Create: `crates/rollshot-edit-proposal/src/review.rs`
- Modify: `crates/rollshot-edit-proposal/src/lib.rs`

**Interfaces:**
- Consumes: `EditProposal`, `ProposedCandidate`, `ProposedEdit`, `CandidateId`, `ProposalId` (Task 3); `rollshot_image_document::EditOp`.
- Produces: `ReviewDecision { proposal_id: ProposalId, accepted: Vec<CandidateId>, rejected: Vec<CandidateId>, modified: Vec<(CandidateId, ProposedEdit)>, resulting_document_state_id: u64 }`; `pub fn lower(proposal: &EditProposal, decision: &ReviewDecision) -> Vec<EditOp>`.

- [ ] **Step 1: Write the failing test**

Create `crates/rollshot-edit-proposal/src/review.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource};
    use rollshot_image_document::{EditOp, ImagePoint, ImageRect};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect::from_corners(ImagePoint::new(x, y), ImagePoint::new(x + w, y + h))
    }
    fn candidate(id: u64, edit: ProposedEdit) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit,
            confidence: 0.9,
            rationale: None,
            provenance: Provenance { source: ProvenanceSource::Agent { run_id: 1 } },
        }
    }
    fn proposal(cands: Vec<ProposedCandidate>) -> EditProposal {
        EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: cands,
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Agent { run_id: 1 } },
        }
    }

    #[test]
    fn lower_includes_accepted_drops_rejected_preserves_order() {
        let p = proposal(vec![
            candidate(1, ProposedEdit::AddRedaction { bounds: rect(0.0, 0.0, 5.0, 5.0) }),
            candidate(2, ProposedEdit::AddRedaction { bounds: rect(10.0, 10.0, 5.0, 5.0) }),
            candidate(3, ProposedEdit::AddRedaction { bounds: rect(20.0, 20.0, 5.0, 5.0) }),
        ]);
        let decision = ReviewDecision {
            proposal_id: ProposalId(1),
            accepted: vec![CandidateId(1), CandidateId(3)],
            rejected: vec![CandidateId(2)],
            modified: vec![],
            resulting_document_state_id: 0,
        };
        let ops = lower(&p, &decision);
        assert_eq!(
            ops,
            vec![
                EditOp::AddRedaction { bounds: rect(0.0, 0.0, 5.0, 5.0) },
                EditOp::AddRedaction { bounds: rect(20.0, 20.0, 5.0, 5.0) },
            ]
        );
    }

    #[test]
    fn lower_applies_modified_override() {
        let p = proposal(vec![candidate(1, ProposedEdit::AddRedaction { bounds: rect(0.0, 0.0, 5.0, 5.0) })]);
        let decision = ReviewDecision {
            proposal_id: ProposalId(1),
            accepted: vec![CandidateId(1)],
            rejected: vec![],
            modified: vec![(CandidateId(1), ProposedEdit::AddRedaction { bounds: rect(30.0, 30.0, 9.0, 9.0) })],
            resulting_document_state_id: 0,
        };
        let ops = lower(&p, &decision);
        assert_eq!(ops, vec![EditOp::AddRedaction { bounds: rect(30.0, 30.0, 9.0, 9.0) }]);
    }

    #[test]
    fn lower_skips_unknown_accepted_and_unaccepted_modified() {
        let p = proposal(vec![candidate(1, ProposedEdit::AddRedaction { bounds: rect(0.0, 0.0, 5.0, 5.0) })]);
        let decision = ReviewDecision {
            proposal_id: ProposalId(1),
            accepted: vec![CandidateId(1), CandidateId(99)], // 99 absent from proposal -> skipped
            rejected: vec![],
            modified: vec![(CandidateId(2), ProposedEdit::AddRedaction { bounds: rect(9.0, 9.0, 1.0, 1.0) })], // id not accepted -> ignored
            resulting_document_state_id: 0,
        };
        assert_eq!(lower(&p, &decision), vec![EditOp::AddRedaction { bounds: rect(0.0, 0.0, 5.0, 5.0) }]);
    }

    #[test]
    fn review_decision_serde_round_trip() {
        use rollshot_image_document::AnnotationId;
        let decision = ReviewDecision {
            proposal_id: ProposalId(3),
            accepted: vec![CandidateId(1), CandidateId(2)],
            rejected: vec![CandidateId(9)],
            modified: vec![(
                CandidateId(2),
                ProposedEdit::UpdateRedactionBounds { id: AnnotationId(7), bounds: rect(1.0, 1.0, 4.0, 4.0) },
            )],
            resulting_document_state_id: 11,
        };
        let json = serde_json::to_string(&decision).unwrap();
        let back: ReviewDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(back, decision);
    }
}
```

- [ ] **Step 2: Run the test — verify it fails**

Run: `rtk cargo test -p rollshot-edit-proposal lower_`
Expected: FAIL to compile — `ReviewDecision`, `lower` not found.

- [ ] **Step 3: Implement `ReviewDecision` + `lower`**

Prepend to `crates/rollshot-edit-proposal/src/review.rs`:

```rust
//! Review outcome and lowering of an accepted decision to document ops.

use rollshot_image_document::EditOp;
use serde::{Deserialize, Serialize};

use crate::proposal::{CandidateId, EditProposal, ProposalId, ProposedEdit};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewDecision {
    pub proposal_id: ProposalId,
    pub accepted: Vec<CandidateId>,
    pub rejected: Vec<CandidateId>,
    /// Candidates the user edited before applying (final edit wins over the original).
    pub modified: Vec<(CandidateId, ProposedEdit)>,
    /// `ImageDocument::state_id()` after the lowered batch is applied.
    pub resulting_document_state_id: u64,
}

/// Lower an accepted decision to the document ops to hand to
/// `ImageDocument::apply_batch`. For each accepted candidate (in the proposal's
/// candidate order), use its modified edit if present, else its original; drop
/// rejected and non-accepted candidates.
pub fn lower(proposal: &EditProposal, decision: &ReviewDecision) -> Vec<EditOp> {
    proposal
        .candidates
        .iter()
        .filter(|c| decision.accepted.contains(&c.id))
        .map(|c| {
            let edit = decision
                .modified
                .iter()
                .find(|(mid, _)| *mid == c.id)
                .map(|(_, e)| e)
                .unwrap_or(&c.edit);
            edit.to_edit_op()
        })
        .collect()
}
```

Add to `crates/rollshot-edit-proposal/src/lib.rs`:

```rust
mod review;

pub use review::{lower, ReviewDecision};
```

- [ ] **Step 4: Run tests — verify pass**

Run: `rtk cargo test -p rollshot-edit-proposal`
Expected: PASS (Task 3 tests + 2 new).

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-edit-proposal
rtk git commit -m "feat(edit-proposal): ReviewDecision and lower() to document EditOps"
```

---

## Task 5: Policy validation + workspace verification

**Files:**
- Create: `crates/rollshot-edit-proposal/src/policy.rs`
- Modify: `crates/rollshot-edit-proposal/src/lib.rs`

**Interfaces:**
- Consumes: `ProposedCandidate`, `ProposedEdit`, `CandidateId` (Task 3); `rollshot_image_document::ImageRect`.
- Produces: `PolicyLimits { max_candidates: u32, max_total_area_fraction: f32, allow_out_of_bounds: bool }`; `PolicyError` (`TooManyCandidates`, `ExcessiveTotalArea`, `OutOfBounds`); `pub fn validate_policy(candidates: &[ProposedCandidate], limits: &PolicyLimits, image_dims: (u32, u32)) -> Result<(), PolicyError>`.

- [ ] **Step 1: Write the failing test**

Create `crates/rollshot-edit-proposal/src/policy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CandidateId, ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource};
    use rollshot_image_document::{ImagePoint, ImageRect};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect::from_corners(ImagePoint::new(x, y), ImagePoint::new(x + w, y + h))
    }
    fn redaction(id: u64, r: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit: ProposedEdit::AddRedaction { bounds: r },
            confidence: 0.9,
            rationale: None,
            provenance: Provenance { source: ProvenanceSource::Agent { run_id: 1 } },
        }
    }
    fn limits() -> PolicyLimits {
        PolicyLimits { max_candidates: 3, max_total_area_fraction: 0.5, allow_out_of_bounds: false }
    }

    #[test]
    fn accepts_within_all_limits() {
        let cands = vec![redaction(1, rect(0.0, 0.0, 10.0, 10.0))];
        assert!(validate_policy(&cands, &limits(), (100, 100)).is_ok());
    }

    #[test]
    fn rejects_too_many_candidates() {
        let cands: Vec<_> = (0..4).map(|i| redaction(i, rect(0.0, 0.0, 2.0, 2.0))).collect();
        assert!(matches!(
            validate_policy(&cands, &limits(), (100, 100)),
            Err(PolicyError::TooManyCandidates { count: 4, max: 3 })
        ));
    }

    #[test]
    fn rejects_excessive_total_area() {
        // 80x80 = 6400 over 100x100 = 10000 -> 0.64 > 0.5 limit.
        let cands = vec![redaction(1, rect(0.0, 0.0, 80.0, 80.0))];
        assert!(matches!(
            validate_policy(&cands, &limits(), (100, 100)),
            Err(PolicyError::ExcessiveTotalArea { .. })
        ));
    }

    #[test]
    fn rejects_out_of_bounds_when_disallowed() {
        let cands = vec![redaction(7, rect(90.0, 90.0, 30.0, 30.0))]; // extends past 100x100
        assert!(matches!(
            validate_policy(&cands, &limits(), (100, 100)),
            Err(PolicyError::OutOfBounds { candidate: CandidateId(7) })
        ));
    }

    #[test]
    fn allows_out_of_bounds_when_enabled() {
        let mut l = limits();
        l.allow_out_of_bounds = true;
        let cands = vec![redaction(7, rect(90.0, 90.0, 30.0, 30.0))];
        assert!(validate_policy(&cands, &l, (100, 100)).is_ok());
    }
}
```

- [ ] **Step 2: Run the test — verify it fails**

Run: `rtk cargo test -p rollshot-edit-proposal validate_policy`
Expected: FAIL to compile — `PolicyLimits`/`PolicyError`/`validate_policy` not found.

- [ ] **Step 3: Implement policy validation**

Prepend to `crates/rollshot-edit-proposal/src/policy.rs`:

```rust
//! Product-policy validation for a proposed candidate set (spec §9.4 limits).
//! Geometric per-op validity (zero-area, non-finite, kind) is the document
//! layer's job; this layer enforces count / total-area / out-of-bounds policy.
//!
//! Area accounting is a deliberate CONSERVATIVE upper bound: each redaction's
//! raw (un-clamped) width*height is summed independently, so overlapping
//! candidates are double-counted and off-image extent is included, and the
//! resulting fraction may exceed 1.0. This never under-reports coverage (the
//! safe direction for a redaction limit); it is NOT the exact painted-pixel
//! fraction. Geometric clamping / zero-area rejection stays the document
//! layer's job (see the §6 validation split).

use rollshot_image_document::ImageRect;
use serde::{Deserialize, Serialize};

use crate::proposal::{CandidateId, ProposedCandidate, ProposedEdit};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PolicyLimits {
    pub max_candidates: u32,
    /// Total redaction area as a fraction of the image area (0.0..=1.0).
    pub max_total_area_fraction: f32,
    pub allow_out_of_bounds: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum PolicyError {
    #[error("too many candidates: {count} exceeds limit {max}")]
    TooManyCandidates { count: u32, max: u32 },
    #[error("total redaction area fraction {fraction} exceeds limit {max}")]
    ExcessiveTotalArea { fraction: f32, max: f32 },
    #[error("candidate is out of bounds")]
    OutOfBounds { candidate: CandidateId },
}

/// Return the redaction bounds a candidate contributes, if any (only redaction
/// edits have an "area" / "bounds" for policy purposes).
fn redaction_bounds(c: &ProposedCandidate) -> Option<ImageRect> {
    match &c.edit {
        ProposedEdit::AddRedaction { bounds } | ProposedEdit::UpdateRedactionBounds { bounds, .. } => {
            Some(*bounds)
        }
        _ => None,
    }
}

pub fn validate_policy(
    candidates: &[ProposedCandidate],
    limits: &PolicyLimits,
    image_dims: (u32, u32),
) -> Result<(), PolicyError> {
    let count = candidates.len() as u32;
    if count > limits.max_candidates {
        return Err(PolicyError::TooManyCandidates { count, max: limits.max_candidates });
    }

    let (w, h) = image_dims;
    let image_area = (w as f32) * (h as f32);

    if !limits.allow_out_of_bounds {
        for c in candidates {
            if let Some(b) = redaction_bounds(c) {
                if b.x < 0.0 || b.y < 0.0 || b.x + b.width > w as f32 || b.y + b.height > h as f32 {
                    return Err(PolicyError::OutOfBounds { candidate: c.id });
                }
            }
        }
    }

    if image_area > 0.0 {
        let total: f32 = candidates
            .iter()
            .filter_map(redaction_bounds)
            .map(|b| b.width.max(0.0) * b.height.max(0.0))
            .sum();
        let fraction = total / image_area;
        if fraction > limits.max_total_area_fraction {
            return Err(PolicyError::ExcessiveTotalArea { fraction, max: limits.max_total_area_fraction });
        }
    }

    Ok(())
}
```

Add to `crates/rollshot-edit-proposal/src/lib.rs`:

```rust
mod policy;

pub use policy::{validate_policy, PolicyError, PolicyLimits};
```

Note: `ImageRect` fields `x`, `y`, `width`, `height` are `pub` (verified in `geometry.rs`), so the direct field access in `validate_policy` compiles from the `rollshot-edit-proposal` crate.

- [ ] **Step 4: Run tests — verify pass**

Run: `rtk cargo test -p rollshot-edit-proposal`
Expected: PASS (all prior + 5 policy tests).

- [ ] **Step 5: Workspace verification (MSRV 1.94)**

Run: `rtk cargo build --workspace`
Expected: PASS — new crate builds, nothing else broke.

Run: `rtk cargo test --workspace`
Expected: PASS.

Run: `rtk cargo fmt --all -- --check`
Expected: no diff.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

(If `clippy` flags the new code, fix and re-run before committing.)

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-edit-proposal
rtk git commit -m "feat(edit-proposal): validate_policy (count/area/out-of-bounds) + workspace green on 1.94"
```

---

## Self-Review

**Spec coverage** (`2026-06-20-edit-proposal-foundation-design.md`):
- §4 `EditOp` / `BatchOutcome` / `apply_batch` (atomic, one undo entry, id-reference, clamp, `NonFiniteCoordinate`) → Tasks 1–2.
- §4.3 `EditError::NonFiniteCoordinate` → Task 1 Step 5.
- §5 `rollshot-edit-proposal` types (`EditProposal`/`ProposedCandidate`/`ProposedEdit`/`Provenance`/`ConfidenceSummary`) → Task 3.
- §5.1 `lower()` → Task 4. §5.2 `validate_policy`/`PolicyLimits`/`PolicyError` → Task 5.
- §6 validation split (geometry in document `EditError`; count/area/out-of-bounds in proposal `validate_policy`) → Tasks 2 + 5. The area limit is a deliberate conservative raw/overlap-inclusive upper bound (documented on `validate_policy`), not the exact painted-pixel fraction.
- §8 decisions (crate split, full CRUD, atomic, pre-batch ids, clamp-at-document, one-undo-via-single-commit) → reflected throughout.
- §9 tests → covered across Tasks 1–5, incl. (post eng-review) the previously-missing cases: one-undo restores `next_number` + `state_id` (exactly one commit); `added_ids` order on a mixed Add* batch; EmptyText atomicity in a batch (`AddTextNote` + `UpdateText`); the `AddNumberCallout`/`UpdateText`/`UpdateTextPosition`/`UpdateNumberPoints` batch paths; `ConfidenceSummary` empty-slice; all seven non-`AddRedaction` `to_edit_op` arms; `lower` infallible-projection contract (unknown accepted / unaccepted modified skipped); and `ReviewDecision` serde round-trip (exercises `AnnotationId` serde).
- §2.2 out-of-scope (UI/agent/persistence/app-wiring) → not implemented here. Correct.

**Placeholder scan:** every code step has complete code; commands have expected output. `ImagePoint`/`ImageRect` field visibility was verified `pub` in `geometry.rs`, so no verify-and-adjust hedges remain. The one remaining judgement call — whether `mod`/`pub use` lines land beside existing ones in `lib.rs` — is a placement instruction, not a placeholder.

**Type consistency:** `EditOp` variants (Task 1) match `ProposedEdit` variants + `to_edit_op` (Task 3) and the `lower` output (Task 4); `BatchOutcome.added_ids: Vec<AnnotationId>` consistent (Tasks 1–2); `CandidateId`/`ProposalId` newtypes consistent across Tasks 3–5; `validate_policy` signature identical in interfaces and impl (Task 5).
