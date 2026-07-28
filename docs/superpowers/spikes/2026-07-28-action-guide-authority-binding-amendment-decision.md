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
3. §17 gains an amendment log pointing at this record.

No gate evidence changes. Gate A1's items are unchanged; item 7 is what makes
item 1 reachable. Gate B1 is unchanged: its permitted-additive list already
covers new variants, and Slice B must not need item 7's shape changed.

## 6. Affected child documents

None. No child spec or implementation plan existed when this amendment was
raised, so there is no completed child document to preserve as historical
evidence. Slice A's child spec is written against the amended §9.
