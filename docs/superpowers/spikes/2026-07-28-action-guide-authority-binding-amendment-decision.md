# Umbrella Amendment: Authority Binding Is a Seventh Shared Contract

**Status:** Approved by user 2026-07-28
**Date:** 2026-07-28
**Branch:** feat/action-guide-agent-foundation-captions
**Amends:**
[`docs/superpowers/specs/2026-07-28-action-guide-agent-foundation-umbrella-design.md`](../specs/2026-07-28-action-guide-agent-foundation-umbrella-design.md)
§9

## 1. Trigger

The umbrella's §17 requires an amendment when evidence changes the
shared-contract change list in §9. Exploration for the Slice A child spec found
a seventh required surgery that §9 omits, and found that §9 item 5's stated
rationale is wrong.

The umbrella's Gate B1 forbids a slice from silently absorbing a shared-contract
surprise. That rule applies to Slice A as well, so this was raised rather than
folded into the child spec.

## 2. Evidence: authority binding blocks Gate A1

Read on 2026-07-28:

- `AuthorityBinding` requires a `DocumentContentBinding`
  (`crates/rollshot-agent/src/authority.rs:75`, `:83`). It is a plain field with
  no optional path.
- `DocumentContentBinding::new` requires a base-image digest and an
  `AnnotationStateV1`, from which it computes an annotation-state digest
  (`crates/rollshot-agent/src/product_task.rs:546`).
- A caption run has neither a base image nor an annotation state. Its authority
  subject is a guide revision, not a document.

Therefore a caption run cannot construct an `AuthoritySnapshot` at all, and
Gate A1 item 1 — "binds a `RunContractReceiptV1` carrying the authority
receipt" — is unreachable without this change.

The change carries three attached sites:

| Site | Current behavior |
|---|---|
| `authority.rs:254`–`:256` | The snapshot digest hashes `base_image_digest`, `annotation_state_digest`, and `state_id` directly. |
| `authority.rs:165` | `authorize_tool` compares a caller-supplied `DocumentContentBinding` for equality. This is the per-tool-call staleness guard. |
| `authority.rs:208` | `AuthoritySnapshotReceiptV1` exposes `document_binding_digest`, which lands in durable provenance. |

## 3. Rejected alternative: a degenerate binding

Captions could construct a `DocumentContentBinding` from a zero base-image
digest and an empty annotation state. This needs no contract change and no
amendment.

It was rejected. The receipt's `document_binding_digest` is durable,
user-auditable provenance, and a zero-valued binding writes a false claim into
it. It also reduces `authorize_tool`'s staleness guard to a vacuous
constant-versus-constant comparison for every caption tool call, removing the
only per-call staleness check on that path while appearing to keep it.

## 4. Evidence: §9 item 5's rationale was wrong

`AuthoritySnapshot::validate_model_input` branches on `DisclosureCeiling` and
counts attachments only (`crates/rollshot-agent/src/authority.rs:180`–`:195`).
`OcrLayoutOnly` already rejects any attachment. A new zero-image level would
therefore behave identically to `OcrLayoutOnly` in the only place the ceiling is
enforced.

The umbrella presented the new level as if it added enforcement. It does not.
Its enforcement teeth are the grant set — a caption run never receives
`InspectPreparedImage` — and an empty prepared-capability set.

The level is still worth adding, for a narrower reason: `disclosure_ceiling` is
recorded in the durable authority receipt, and recording `OcrLayoutOnly` for a
run that never touched OCR, layout, or any image is false provenance. The
decision keeps item 5 with the corrected rationale rather than dropping it.

## 5. Umbrella changes

1. §9 gains item 7: `DocumentContentBinding` becomes domain-tagged, or
   `AuthorityBinding` holds the source binding directly. The child spec chooses
   the shape. A degenerate binding is explicitly not permitted.
2. §9 item 5's rationale is corrected: provenance honesty, not enforcement.
3. §9 item 4 is widened per §7 below.
4. §17 gains an amendment log pointing at this record.

No gate evidence changes. Gate A1's items are unchanged; item 7 is what makes
item 1 reachable. Gate B1 is unchanged: its permitted-additive list already
covers new variants, and Slice B must not need item 7's shape changed.

## 6. Affected child documents

None. No child spec or implementation plan existed when this amendment was
raised, so there is no completed child document to preserve as historical
evidence. Slice A's child spec is written against the amended §9.

## 7. Second finding: the artifact payload parameter

Found during the same exploration pass, raised and approved on the same day, and
recorded here rather than in a separate record because it is the same class of
omission in the same §9 list.

`ProductTaskSnapshot::record_ready_for_review` takes
`payload: SmartRedactionReviewPayload` and serializes it internally
(`crates/rollshot-agent/src/product_task.rs:948`). `PromotionContext` likewise
carries `PayloadSourceV1` and `PayloadProposalV1`
(`crates/rollshot-agent/src/product_task.rs:607`). A caption run cannot
construct any of them.

§9 item 4 as originally written covered only the *interpretation* of
`pending_proposal_payload`. It did not cover `pending_artifact_payload` or the
concrete parameter type, so the promotion call itself was unreachable for a
non-Smart-Redaction artifact.

Item 4 is widened to the whole artifact payload surface: both payload fields
dispatch on `ArtifactKind`, and `record_ready_for_review`'s payload parameter
becomes kind-agnostic bytes serialized by the caller, with `PromotionContext`
moving with it. No snapshot field is added, and `canonical_payload_sha256`
remains the integrity check.

### 7.1 A counter-example worth recording

Not everything on the review path needed widening. `ReviewReceipt` is reusable
unchanged: `applied_candidates` and `rejected_candidates` hold suggestion
identifiers, `resulting_document_state_id` and `resulting_document_digest` are
already `Option` and are `None` for captions, and `local_delta`'s
`moved_candidates` and `manual_additions` are honestly empty because
`CaptionProposal::apply` has no edit-then-accept path
(`crates/rollshot-action/src/caption_proposal.rs:182`).

`RunOperation` also needed no new variant: a caption run's grant set is exactly
`{SubmitReviewCandidate}`, because the guide content is composed into the prompt
before the run rather than fetched by a tool.

These are recorded because the umbrella's Gate B1 tests whether the contracts
generalize. Two of the surfaces examined generalized without change, which is
evidence in the affirmative direction and should not be lost among the seven
that did not.

## 8. Factual correction: the store is not shared across two live workspaces

Found by independent engineering review of the Slice A plan on 2026-07-28,
verified against code, and applied as a factual correction rather than an
amendment.

Umbrella §10 and child spec §3.8 both described the shared store as opened once
at application initialization and handed to both workspaces, and §10 justified
its single-instance rule by claiming Smart Redaction and Action Guide could each
drive a run at the same time within one process.

They cannot. `main.rs:74`'s `run` dispatches into mutually exclusive
`LaunchMode` branches — capture, daemon, open-image, Action Guide — and
`result_workspace::run` (`result_workspace/mod.rs:371`) and
`timeline_workspace::run` (`timeline_workspace/mod.rs:864`) are separate iced
applications that never coexist in a process.

What survives:

- the one-`TaskStore`-per-process rule, unchanged. It is now understood to be
  trivially satisfied rather than a constraint the design must work to maintain,
  and it is still worth stating because a future single-process shell would
  otherwise violate it silently;
- Slice A's two-domain concurrency test, reframed. It is a regression test that
  the lock and the audited-write path behave for two task kinds sharing one tree,
  not mitigation of a newly created intra-process race.

What was wrong: the claimed intra-process exposure. Cross-process contention is
real and predates this program; the blocking fs4 lock at `task_store.rs:797`
already existed for it.

This is recorded as a correction, not an amendment, because §17's
amendment triggers include the single-`TaskStore`-per-process rule and the rule
did not change — only its justification. The correction is logged in §17.1 so
Slice B does not plan against the withdrawn claim.
