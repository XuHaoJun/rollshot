# Artifacts, review, and provenance

**Status:** In Progress — Round 4 capability comparison; no final selection

**Date:** 2026-07-22

**Scope:** Product artifacts, expected outputs, validation, review, revision, publish/archive, and provenance across Rollshot, Pi, Oh My Pi (OMP), Codex, Claude Code, and the deferred Brag/Hyperframes workload.

**Method:** Code is the source of truth for Rollshot. External-system claims use the reviewed system profiles and their exact audits. Hyperframes and Brag are workload evidence, not evidence of a Rollshot implementation.

**Pinned workload evidence:** Brag `357a805e`; Hyperframes `807078c7`.

This is a comparison artifact, not an architecture decision. It identifies candidate patterns and measurable boundaries without selecting a final product-wide artifact model.

## 1. Why this capability matters

An agent can write a file, return a tool result, or announce that a worker finished. None of those events, by themselves, establish that Rollshot has a usable product artifact.

A product artifact needs enough typed state to answer:

1. What product object is this, and which revision is it?
2. Which schema and compatibility contract apply?
3. What inputs, source state, configuration, skills, tools, model, and provider produced it?
4. Which deterministic checks ran, and what evidence did they produce?
5. Did a user approve, reject, or correct it?
6. Is it a draft, a reviewed revision, a published output, or an archived/deleted record?
7. Was it an expected output whose acceptance can complete a workflow?
8. What may be retained, redacted, exported, or deleted?

The distinction is load-bearing for all three target workloads:

- **Smart Redaction:** generated automation and proposed edits must remain reviewable before deterministic document mutation.
- **Action Guide:** captions, annotations, project revisions, and exports must be tied to the project revision they describe.
- **Deferred Brag/Hyperframes:** worker-created files and completion notifications must not silently become accepted production assets.

## 2. Hard distinction: Typed artifact versus ambient file/output

| Concept | Definition | Examples | Completion meaning |
|---|---|---|---|
| **Product Artifact** | A product-owned, typed object with stable identity, schema/version, lifecycle state, provenance, validation evidence, review decision when required, and an explicit storage/retention contract. | A reviewed redaction proposal revision; an Action Guide project revision; a published guide export with a receipt. | May satisfy a declared expected output, but only after its required validation and acceptance policy passes. |
| **Ambient file** | Bytes at a path without a product-owned artifact record. | Worker-created HTML, JSON, image, log, or temporary render. | File existence proves only that bytes exist. |
| **Log/transcript** | Operational or conversational evidence about execution. | Agent event stream, assistant text, subagent notification, provider transcript. | Useful provenance/evidence; not acceptance. |
| **Tool output** | A result returned by a tool invocation. | Validation diagnostics, image metadata, OCR matches, render status. | Input to validation or provenance; not automatically an artifact. |
| **Path/URI** | A locator. | Filesystem path, `artifact://` spill URI, project directory. | Locates data; does not establish type, integrity, review, or ownership. |
| **Skill/agent output** | Content emitted while executing a skill or an agent run. | Draft source, recommendation, generated caption, generated frame. | A candidate input. It becomes a Product Artifact only through an explicit product ingestion/validation path. |

Two rules follow:

> Skill output and agent output do **not** automatically become Product Artifacts.

> A review notification, worker completion message, or “ready for review” terminal state does **not** mean the user accepted the result.

These rules avoid conflating transport success, execution success, validation success, and product acceptance.

## 3. Comparison contract

The following is a normalized comparison lens, not a claim that Rollshot already implements a generic `ProductArtifact` type.

| Dimension | Minimum question | Candidate normalized field or relation |
|---|---|---|
| Identity | Which logical artifact and immutable revision? | `artifact_id`, `revision_id`, optional `parent_revision_id` |
| Kind and schema | How is it decoded and compatibility-checked? | `kind`, `artifact_schema_version`, domain API/schema versions |
| Mutability | Can this record change in place? | Immutable revision plus mutable head/status pointer |
| Storage | Where are authoritative bytes and metadata? | Product-owned store reference, content digest, atomic commit receipt |
| Retention | When may payload/evidence be retained, redacted, archived, or deleted? | Sensitivity class, retention policy, archive/delete state |
| Expected output | Which workflow obligation can this satisfy? | `expected_output_id`, required kind/schema/review policy |
| Completion | What exact event fulfills the obligation? | Accepted revision receipt, not path existence or notification |
| Source binding | Which document/project/input revision was used? | Source object ID, source revision/state ID, input digests |
| Validation | Which deterministic checks ran against which revision? | Typed validation receipt with checker/version/result/evidence reference |
| Review | Who decided what, when, and against which revision? | Typed user decision receipt: approve/reject/correct |
| Producer provenance | Which skill/tool/model/provider/config produced the draft? | Versioned producer chain with privacy-bounded parameters |
| Publication | Which reviewed revision was exported/published? | Publication receipt linking output digest to source revision |

A locator can appear inside this contract, but a locator is not the contract.

## 4. Lifecycle and decision semantics

```text
Draft
  │ deterministic validation succeeds
  ▼
Validated ── validation fails ──▶ Draft/Invalid
  │ review requested
  ▼
ReadyForReview
  ├── user rejects ─────────────▶ Rejected
  ├── user corrects ────────────▶ Corrected Draft ──▶ new Revision
  └── user approves ────────────▶ Approved Revision
                                      │ deterministic publication/export
                                      ▼
                                   Published
                                      │ retention transition
                                      ▼
                                   Archived/Deleted
```

The labels describe distinct evidence:

- **Validation evidence** says a named deterministic checker evaluated a named revision under a named version/configuration and records the outcome.
- **User decision evidence** says a user approved, rejected, or corrected a particular proposal/revision. It must not be synthesized from validation success.
- **Correction** creates a new candidate revision or an explicit modified-decision record. It must not rewrite the reviewed bytes without changing identity.
- **Publication evidence** binds an output digest and publication format to the approved source revision.
- **Archive/deletion evidence** changes availability/retention state; it does not erase the fact that a decision once existed unless policy requires full erasure.

An implementation may use fewer persisted states, but it must preserve these semantic distinctions wherever they affect user trust or workflow completion.

## 5. Current Rollshot facts

### 5.1 Existing typed objects

| Rollshot object | Current strength | Current boundary or audited gap |
|---|---|---|
| `ValidatedAutomation` | Stores canonical source, language-schema version, capability-API version, output-schema version, IR, and validation summary. `ensure_compatible` revalidates and rejects version/normalization drift. [R1] | It is a domain artifact, not a generic Product Artifact. Generic artifact identity, expected-output, and review-receipt terms were not found in the bounded agent/edit/workbench roots. [A:R-GENERIC] |
| `AutomationRevision` | Immutable revision identity, preset identity, parent revision, creation time, provenance, and a `ValidatedAutomation`; the preset carries the mutable active-revision pointer. Store reads re-check compatibility and writes atomically. [R2] | Revision provenance is narrower than the full skill/tool/model/provider/source/config/user-decision chain. Exact fields were not found in the bounded artifact-domain roots. [A:R-PROV] |
| `EditProposal` | Typed proposal/candidate IDs, base document state ID, candidates, confidence/rationale summary, and manual or agent-run provenance; serializable. [R3] | Provenance is only `Manual` or `Agent { run_id }`; it does not itself retain validation, producer versions, or a user-decision receipt. [R3][A:R-PROV] |
| `ReviewDecision` | Serializable partition of accepted, rejected, and modified candidates plus resulting document state ID. Lowering converts only accepted/modified candidates into document edits. [R4] | The workbench constructs and applies it, then clears pending state; storage roots searched did not contain `ReviewDecision` or pending proposal/draft types. This is a bounded persistence gap, not a repository-wide impossibility claim. [R5][A:R-REVIEW-STORE] |
| `ImageDocument` | Stable state ID, atomic batch application, reference validation, and bounded undo/redo snapshots; failed batches restore the prior state. Flattening is explicit. [R6] | History is memory-local and document flattening is a copy/save operation; the crate does not define product artifact retention, review, or publication lifecycle. [R6][A:R-LIFECYCLE] |
| Smart Redaction `DraftState`/`ReadyForReview` | Generation-bound validation, policy, and dry-run evidence; submit requires current-generation evidence and matching source. The terminal value carries proposal, automation, session/generation, budget use, and assistant text. [R7] | `ReadyForReview` is readiness, not acceptance. The workbench currently restamps the dry-run proposal to the live document state immediately before apply, which protects the active path from a stale base but means the proposal's earlier base is not retained there as enduring lineage. [R5][R7] |
| Action Guide `ProjectManifestV2` | Deny-unknown typed schema, project revision, frame order and SHA-256/dimensions, ordered steps, annotations/explanations, capture/input metadata, outputs, and import warnings. Project saves validate and use atomic/no-replace commit paths; existing saves enforce revision comparison. [R8] | The manifest is a strong domain artifact, but no product-wide expected-output/review/provenance envelope was found in the bounded generic audit. [A:R-GENERIC] |
| Action Guide `PublishStateV1` | Records output kind and last successful project revision; load reconciles state with present outputs and reports freshness only for the current revision. [R9] | GIF/MP4 presence checks establish regular-file presence, not content digest or media decode validity; therefore “current” is not a complete publication-integrity receipt. [R9] |
| `CaptionProposal` / `VisualAnnotationProposal` | Bind proposals to run ID and source step/keyframe/document state; validate stale bases and support pending/accepted/rejected/stale outcomes. Visual proposals validate geometry and policy before apply. [R10] | Proposal persistence/serde/store terms were not found in the two proposal source files, and proposal types were not found in searched project/export/preset/provider stores. [A:R-ACTION-PROP][A:R-REVIEW-STORE] |
| Agent events, assistant text, and tool results | Preserve execution information and can carry evidence into a terminal state. [R7] | No generic Product Artifact/expected-output/completion-receipt contract was found in the bounded agent/edit/workbench roots. These outputs remain operational data unless a product path explicitly promotes them. [A:R-GENERIC] |

### 5.2 Current strengths

Rollshot already has several ingredients worth preserving:

1. **Compatibility is executable.** `ValidatedAutomation` is not just source plus a version label; load/execution can re-run compatibility checks against canonical source and exact normalized artifacts. [R1]
2. **Automation revisions are immutable.** The revision record has independent identity and lineage while the preset head remains mutable. [R2]
3. **Mutation is deterministic and atomic.** Proposals lower to `ImageDocument` operations; a failed batch restores the previous state and a successful batch produces one history transition. [R4][R6]
4. **Smart Redaction binds evidence to a generation.** New drafts invalidate old validation/dry-run evidence, preventing stale evidence from authorizing a newer source. [R7]
5. **Action Guide projects are revisioned, validated, and atomically stored.** Frame digests and revision conflict checks make project state substantially stronger than an ambient folder. [R8]
6. **Action proposals encode staleness.** Caption and visual proposals bind to base content/state rather than assuming a current step indefinitely. [R10]

### 5.3 Current gaps, bounded precisely

The following are not claims that no related symbol exists anywhere in the repository. They are exact results from named roots:

- **No generic artifact contract in active agent/edit/workbench roots:** the exact terms in [A:R-GENERIC] returned no matches.
- **No full producer-provenance fields in the bounded artifact-domain records:** [A:R-PROV] returned no matches for skill/tool/model/provider/config/source digest and decision actor/time field families.
- **No retention/archive lifecycle fields in the bounded domain roots:** [A:R-LIFECYCLE] returned no matches.
- **No retained workbench review record in searched stores:** [A:R-REVIEW-STORE] returned no matches for review/pending proposal types.
- **No proposal serialization/store contract in the Action proposal files:** [A:R-ACTION-PROP] found only a test name containing “stored,” not serde/persist/store APIs.

These gaps matter only if Rollshot chooses to make reviewed outputs durable, resumable, exportable, or workflow-completing across sessions.

## 6. Cross-system comparison

### 6.1 Capability/status facts

| System | What exists | Product Artifact status | Review/provenance boundary |
|---|---|---|---|
| **Rollshot** | Typed automation artifacts/revisions, edit proposals/decisions, revisioned Action Guide manifests, publish freshness, and staleness-aware Action proposals. [R1–R10] | Domain-specific pieces exist. A generic Product Artifact and expected-output contract was not found in bounded roots. [A:R-GENERIC] | Review decisions exist in memory/serde contracts, but retained workbench decision lineage and broad producer provenance were not found in the named audits. [A:R-REVIEW-STORE][A:R-PROV] |
| **Pi** | Sessions and extension-local structured outputs/files exist. [P1] | A built-in typed artifact registry with expected completion, review, revision, and provenance was not found by the profile's exact artifact audit. [P1:A4] | Any richer artifact/review semantics are extension-owned; the reviewed profile did not establish a built-in general contract. [P1:A4] |
| **OMP** | Task outputs and session spill references such as `artifact://` exist. [P2] | The profile found operational spill/task outputs, not a generic typed product-artifact revision/lineage/expected/review model. [P2:A4] | A spill URI is a transport/storage locator; the reviewed audit did not establish product acceptance semantics. [P2:A4] |
| **Codex** | Tool outputs and a narrow image-generation persisted-artifact path exist. [P3] | Generic `Artifact` type/ID/store was absent from the profile's exact roots; image generation is a narrow extension path. [P3:A6] | No general review/provenance lifecycle is established by that narrow path in the reviewed profile. [P3:A6] |
| **Claude Code** | Heterogeneous files, task outputs, transcripts, plans, and mailboxes exist. [P4] | A general Artifact declaration was absent from the profile's exact audit roots. [P4:A1] | Files/mailboxes can coordinate work but do not establish a general acceptance or revision contract in the reviewed evidence. [P4:A1] |
| **Hyperframes workload** | Production packets, block/assets, per-frame outputs, review gates, assembly, verification, and delivery rules. [H1–H3] | This is a workload protocol, not a general artifact store and not a Rollshot implementation. [H1–H3] | Explicit plan/sketch/final-look approvals are required; worker notification is best-effort and expected-file existence is checked separately. [H2][H3] |
| **Brag workload** | A skill workflow for producing a brag/launch video using Hyperframes evidence. [B1] | Skill instructions and generated files are workload inputs/outputs, not automatically Rollshot Product Artifacts. [B1][H1] | Review gates come from the production protocol; skill completion does not replace user approval. [B1][H2] |

### 6.2 Contract dimensions

| System | Identity/schema | Mutability/storage | Retention | Expected output/completion |
|---|---|---|---|---|
| Rollshot automation preset | Revision/preset IDs and explicit automation schema/API versions. [R1][R2] | Immutable revision plus mutable active head; atomic file store. [R2] | Product-artifact retention/archive fields not found in bounded roots. [A:R-LIFECYCLE] | Generic expected-output/completion receipt not found in bounded roots. [A:R-GENERIC] |
| Rollshot Action Guide | Manifest schema v2, project revision, frame digests. [R8] | Atomic project save and revision conflict; mutable project head through new manifest revision. [R8] | Product-artifact retention/archive fields not found in bounded Action roots. [A:R-LIFECYCLE] | Publish freshness links output kind to project revision, but no generic expected-output contract was found. [R9][A:R-GENERIC] |
| Pi | Session/extension-specific identities where defined. [P1] | Ordinary files or extension-owned persistence. [P1] | Generic artifact retention contract not established by exact artifact audit. [P1:A4] | Generic expected-artifact completion contract not found. [P1:A4] |
| OMP | Task/session identifiers and spill locators. [P2] | Session/task storage and `artifact://` spill. [P2] | Generic product-artifact retention contract not established. [P2:A4] | Task completion/output exists, but typed product-artifact acceptance was not found. [P2:A4] |
| Codex | Tool-specific identities; narrow generated-image persistence. [P3] | Tool/runtime-specific storage. [P3] | Generic artifact retention contract not established in audited roots. [P3:A6] | Generic expected-artifact completion contract not found. [P3:A6] |
| Claude Code | Task/file/transcript/plan/mailbox identities. [P4] | Heterogeneous filesystem and task persistence. [P4] | Generic artifact retention contract not established in audited roots. [P4:A1] | Generic expected-artifact acceptance contract not found. [P4:A1] |
| Hyperframes | Named packets, blocks, frames, HTML/motion JSON, renders. [H1–H3] | Files in a prescribed production layout; edits re-enter touched stages. [H1] | General retention/deletion policy is not specified in the inspected workload files. [A:H-RETENTION] | Expected-file presence unblocks dispatch; later deterministic checks and explicit review gates govern acceptance/delivery. [H2][H3] |

The cross-system result is consistent: execution frameworks commonly retain messages, tool output, task status, or files, but the reviewed evidence does not show a ready-made general Product Artifact contract that Rollshot can adopt without product-specific semantics.

## 7. Validation evidence is not a user decision

### 7.1 Validation receipt

A durable validation receipt should minimally bind:

- artifact/revision identity;
- checker or policy identity and version;
- relevant configuration digest;
- source/input revision or digest;
- result (`pass`, `fail`, or bounded warning set);
- structured findings or an evidence reference;
- execution time and, where meaningful, deterministic runtime version.

Validation answers “did the deterministic contract pass?” It does not answer “does the user want this change?”

### 7.2 User decision receipt

A durable review receipt should minimally bind:

- exact proposal/artifact revision reviewed;
- source document/project revision reviewed against;
- decision actor (privacy-preserving local identity is sufficient);
- decision (`approve`, `reject`, or `correct`);
- accepted/rejected/modified candidate partition when applicable;
- resulting revision/state ID after deterministic apply;
- decision time;
- optional reason, subject to privacy and retention policy.

Rollshot's `ReviewDecision` already models the candidate partition and resulting document state. [R4] The missing question is whether/where such a receipt should be retained for resumability, provenance, export, and deletion—not whether the UI can apply a proposal today.

### 7.3 Notification versus acceptance

The Hyperframes dispatch contract makes the distinction concrete: a child notification is best-effort; the orchestrator waits for the expected artifact path, and absence triggers one bounded redispatch. [H3] Even then, file presence is only a dispatch condition. Production checks and explicit review gates remain separate. [H1][H2]

Rollshot should retain the same semantic separation if it adds expected outputs:

```text
worker notification
  != bytes exist
  != schema valid
  != policy valid
  != user accepted
  != published/delivered
```

## 8. Provenance model

### 8.1 Provenance chain to compare

| Provenance element | Why it matters | Minimum privacy-bounded form | Current Rollshot evidence/gap |
|---|---|---|---|
| Skill | Reproduces the workflow instruction set and declared capability. | Stable skill ID plus version/digest; no copied private skill body by default. | Exact skill ID/version/digest fields not found in bounded artifact-domain records. [A:R-PROV] |
| Tool | Explains which deterministic or side-effecting operation produced evidence/output. | Stable tool name/contract version and invocation/result digests; redact sensitive arguments. | Runtime tool names exist operationally, but exact artifact provenance field families were absent in the bounded artifact records. [A:R-PROV] |
| Model | Explains nondeterministic judgment source and supports later evaluation. | Provider-scoped model ID/version or deployment alias captured at run time. | Exact model provenance fields not found in bounded artifact-domain records. [A:R-PROV] |
| Provider | Distinguishes adapters and provider behavior. | Provider ID and adapter/contract version; never credentials. | Exact provider provenance fields not found in bounded artifact-domain records. [A:R-PROV] |
| Source/input | Prevents a valid result for old bytes being applied to new bytes. | Source object ID/revision/state ID and content/config digests as appropriate. | Strong partial support: document state IDs, project revisions, frame SHA-256, automation source and generation binding. [R1][R3][R7][R8][R10] |
| Configuration | Reproduces limits, policies, model/tool settings, and export choices. | Canonical privacy-filtered config digest plus versioned policy IDs. | Automation validation limits and version fields exist; a cross-artifact config provenance field family was not found. [R1][A:R-PROV] |
| Validation | Demonstrates deterministic checks against the produced revision. | Typed receipt with checker/version/outcome/evidence reference. | Strong in-memory/domain evidence for automation and project validation; no generic receipt contract found. [R1][R7][R8][A:R-GENERIC] |
| User decision | Establishes acceptance, rejection, or correction. | Actor, decision, time, exact reviewed revision, resulting revision. | `ReviewDecision` contains the candidate partition/resulting state, but decision actor/time and retained store linkage were not found in bounded audits. [R4][A:R-PROV][A:R-REVIEW-STORE] |

### 8.2 Provenance is not a transcript dump

Provenance should be structured and minimized. It should not require retaining full prompts, screenshots, provider payloads, raw OCR text, credentials, or complete transcripts. A digest/reference may be sufficient when the underlying data has a stricter deletion policy. Conversely, a digest alone is insufficient when a user needs to inspect the evidence behind a review decision; the contract should say which evidence remains viewable.

### 8.3 Revision rule

If any material producer input changes—source revision, skill version, tool contract, model/provider, policy/configuration, or user correction—the resulting candidate should receive a new revision identity. A mutable head may point to the latest revision; prior accepted/rejected revisions remain addressable until retention policy removes them.

## 9. Privacy, redaction, deletion, and export

Artifacts join product data and agent execution data, so their lifecycle must be explicit.

### 9.1 Data classes

| Class | Examples | Default handling candidate |
|---|---|---|
| Product payload | Screenshot pixels, guide frames, annotations, automation source, exported media. | Product-owned storage; user-visible delete/export controls; no provider upload without the active operation's authorization. |
| Review evidence | Proposal overlays, validation findings, accepted/rejected candidate partition. | Retain only as long as needed for undo/resume/history policy; redact sensitive previews when possible. |
| Provenance metadata | IDs, versions, hashes, timestamps, decision relation. | Retain longer than raw payload only if policy permits; avoid secrets and raw prompt bodies. |
| Operational telemetry | Logs, traces, timing, token/budget use. | Separate retention from Product Artifact retention; never treat logs as the authoritative artifact. |
| Credentials/provider secrets | API keys, tokens, headers. | Never include in artifact payload, provenance, validation receipt, or export. |

### 9.2 Redaction

- Redaction must operate on both payload and evidence surfaces. A thumbnail or rejected candidate can leak the same pixels as the final artifact.
- Provenance arguments should use allowlisted fields or digests, not blindly serialized tool/provider requests.
- Smart Redaction provenance must not retain original sensitive text merely to prove that a redaction was proposed.
- Export should disclose whether provenance/review metadata is embedded, sidecar-only, or omitted.

### 9.3 Deletion

Deletion needs a declared scope:

- logical artifact only;
- one immutable revision;
- all revisions and mutable heads;
- payload bytes but retained minimal tombstone/decision metadata;
- derived previews, validation evidence, exports, caches, and provider-side data where controllable.

The current bounded artifact-domain search found no retention/tombstone/archive/delete timestamp field families. [A:R-LIFECYCLE] This makes deletion/retention an open design area, not a reason to retrofit it implicitly into logs.

### 9.4 Export

A trustworthy export receipt should state:

- source artifact/revision;
- format and format/schema version;
- output content digest and size;
- deterministic exporter/version/configuration;
- validation performed on the produced bytes;
- whether provenance/review metadata was included;
- publication time and destination class, without leaking sensitive absolute paths when exported/shareable.

Action Guide's current publish state is a useful freshness pointer, but GIF/MP4 regular-file presence is weaker than such a receipt. [R9]

## 10. Judgment-to-deterministic-execution boundary

### 10.1 Smart Redaction

| Phase | Agent/model judgment | Deterministic Rollshot responsibility | Product/user decision |
|---|---|---|---|
| Draft | Propose restricted automation source and candidate intent. | Parse/normalize/validate source; enforce language, capability, resource, and policy limits. [R1][R7] | None yet. |
| Dry run | No authority to mutate the document. | Execute in hardened runtime, validate output/proposal policy, bind evidence to session generation and source. [R7] | Decide whether result is worth reviewing. |
| Review | May explain candidates and rationale. | Render exact proposal against exact document state; preserve candidate IDs and modifications. [R3–R7] | Approve, reject, or correct each candidate. |
| Apply | No direct mutation authority. | Lower accepted/modified candidates to typed document operations and apply atomically. [R4][R6] | The user's decision authorizes this deterministic apply. |
| Save revision | May suggest a preset name/note. | Revalidate source, create immutable automation revision, update active head atomically. [R1][R2] | User chooses whether to retain/reuse the automation. |

The current generation-bound evidence path is a strong foundation. A future durable artifact path should preserve the original source/document binding rather than relying only on the workbench's apply-time restamp. [R5][R7]

### 10.2 Action Guide

| Phase | Agent/model judgment | Deterministic Rollshot responsibility | Product/user decision |
|---|---|---|---|
| Propose | Suggest caption text, visual annotations, and rationale. | Bind to run/source step/keyframe/document state; validate geometry, policy, duplicates, and staleness. [R10] | Accept, reject, edit, or request a rebase. |
| Apply | No direct project mutation authority. | Apply accepted changes against the exact base and create a new document/project revision. [R8][R10] | Approval authorizes deterministic mutation. |
| Save | May suggest structure or wording. | Validate project tree/assets/digests, enforce revision comparison, atomically commit manifest/project. [R8] | User chooses save/overwrite flow within revision rules. |
| Publish | May recommend output formats. | Render/export, validate produced bytes, bind receipt to exact project revision, update freshness. [R9] | User chooses publication/export destination. |

Action Guide proposals already encode stale-base semantics, but durable proposal/decision provenance remains an audited gap in the searched stores. [R10][A:R-REVIEW-STORE]

### 10.3 Deferred Brag/Hyperframes

| Phase | Agent/worker judgment | Deterministic orchestration responsibility | Product/user decision |
|---|---|---|---|
| Plan/packet | Translate the brief into production intent, shot plan, and frame assignments. [B1][H1] | Validate packet completeness, naming, dependencies, and bounded dispatch plan. | Approve the plan checkpoint. [H2] |
| Sketch/look | Generate sketch frames, scene HTML, motion specs, or variants. [H1–H3] | Verify expected paths, required paired files, dimensions/duration/configuration, and record producer provenance. | Approve sketch/final-look checkpoints. [H2] |
| Render/assemble | Generate frames/audio and assemble. | Check duration synchronization, artifact integrity, render settings, and bounded retries. [H1] | No implicit acceptance from worker completion. |
| Deliver | Recommend final selection. | Verify final bytes, content digest, format, and link to approved source assets/revisions. | Explicit delivery acceptance/export. |

Hyperframes explicitly treats notification as best-effort and path existence as a separate wait condition. [H3] Rollshot should go one step further if this workload is implemented: expected-file presence must feed typed validation and user review before a frame/render becomes a Product Artifact.

The inspected Hyperframes sources also contain a bounded orchestration ambiguity noted elsewhere in this research: generic dispatch guidance and general-video frame-worker guidance differ on worker grouping/waves. That remains a workload-integration question; this capability does not choose a dispatch policy.

## 11. Candidate Rollshot patterns (no final selection)

### Pattern A — Bounded reviewed-proposal envelope

Wrap existing domain proposals without introducing a universal payload format:

```text
ReviewedProposal<T>
  proposal_id
  base_object_id + base_revision/state_id
  proposal_schema_version
  producer_provenance
  validation_receipts[]
  payload: T
  review_decision?
  resulting_revision/state_id?
```

**Fit:** Smart Redaction and Action Guide caption/annotation proposals.

**Strength:** Smallest change; preserves typed domain payloads and existing deterministic lowering.

**Risk/open question:** Does not by itself unify publication, archive/delete, or expected-output completion.

### Pattern B — Revisioned Product Artifact manifest/ledger

Generalize the successful preset and Action Guide revision pattern:

```text
ArtifactHead { artifact_id, active_revision_id, lifecycle_status }
ArtifactRevision {
  revision_id, parent_revision_id, kind, schema_version,
  payload_ref + digest, source_bindings[], producer_provenance,
  validation_receipts[], review_receipt?, retention_policy
}
PublicationReceipt { revision_id, output_digest, exporter, validation }
```

**Fit:** Durable cross-session artifacts, revision history, export, archive/delete, and lineage.

**Strength:** Clear immutability and shared lifecycle vocabulary; aligns with `AutomationRevision` and revisioned Action projects. [R2][R8]

**Risk/open question:** A universal ledger may be excessive for ephemeral proposals and can centralize sensitive metadata unless privacy boundaries are strict.

### Pattern C — Expected-artifact ledger with acceptance receipts

Keep artifact storage domain-owned and add a workflow-level obligation:

```text
ExpectedOutput {
  expected_output_id, task/run_id, required_kind/schema,
  source_revision, validator_policy, review_policy, status
}
CompletionReceipt {
  expected_output_id, artifact_revision_id,
  validation_receipts[], review_receipt?, completed_at
}
```

**Fit:** Deferred Hyperframes multi-worker orchestration and long-running export/publish jobs.

**Strength:** Makes “done” inspectable and prevents notifications/paths from completing tasks.

**Risk/open question:** Adds coordination machinery that Smart Redaction's current single-workbench flow may not need.

### Pattern comparison

| Criterion | Pattern A | Pattern B | Pattern C |
|---|---|---|---|
| Reuses current proposal/domain types | Strong | Moderate | Moderate |
| Cross-session revision lineage | Partial | Strong | Delegated to domain artifact |
| Review receipt | Strong | Strong | References required receipt |
| Publication/archive/delete | Weak | Strong | Policy/reference only |
| Expected-output completion | Weak | Can host it, but not primary | Strong |
| Smart Redaction fit | Strong | Possible, heavier | Usually unnecessary alone |
| Action Guide fit | Strong for proposals | Strong for project/export | Useful for background publish |
| Hyperframes fit | Useful per-frame | Useful for durable assets | Strong for orchestration |

No pattern is selected in Round 4. They can also compose—for example, Pattern A proposal payloads can produce Pattern B revisions, while Pattern C records that an approved revision fulfilled a workflow obligation.

## 12. Non-goals

- Treating every file, log line, transcript, or tool result as a Product Artifact.
- Uploading local artifacts or provenance to a provider by default.
- Retaining full prompts, screenshots, OCR text, credentials, or provider payloads solely for provenance.
- Giving an agent, skill, subagent, or tool unilateral approval authority over user-visible edits.
- Replacing `ImageDocument`, `EditProposal`, `AutomationRevision`, or Action Guide manifest types with untyped JSON blobs.
- Designing a distributed content-addressed store before a concrete workload needs one.
- Making notification delivery or filesystem existence equivalent to workflow completion.
- Selecting a final artifact architecture in this comparison.
- Resolving Hyperframes worker-wave policy or implementing the deferred workload here.

## 13. Measurable acceptance criteria for a future implementation

1. **Identity:** 100% of persisted reviewed artifacts have stable logical and immutable revision IDs.
2. **Compatibility:** Loading a revision with unsupported artifact/domain schema or altered canonical source fails closed in tests.
3. **Source binding:** Applying a proposal to a different document/project revision fails deterministically with no partial mutation.
4. **Validation:** Every accepted artifact revision that requires validation names the checker/policy version and records pass/fail evidence.
5. **Review:** Every user-visible agent edit applied to a document/project has an approve/reject/correct decision bound to the exact proposal revision; `ReadyForReview` alone cannot satisfy this check.
6. **Atomicity:** Failed apply/save/publish leaves the prior authoritative revision and head unchanged.
7. **Revision:** Correction or material producer-input change creates a new revision ID and retains parent lineage while policy permits.
8. **Expected output:** A task cannot become complete from notification or path existence; its test requires a compatible artifact revision plus required validation/review receipts.
9. **Provenance:** Fixture tests can recover skill/tool/model/provider/source/config version identifiers without exposing credentials or raw sensitive arguments.
10. **Privacy:** Export/deletion tests enumerate payload, preview/evidence, provenance, derived output, and cache effects; secrets never serialize into artifact records.
11. **Publication integrity:** Published Action Guide media freshness requires produced-byte validation/digest, not only regular-file presence.
12. **Resume:** After restart, a pending reviewed workflow either restores the exact proposal/evidence/source binding or explicitly expires it; it never silently applies against current state.
13. **Auditability:** A user can inspect why a revision exists, what deterministic checks passed, and what they decided without reading raw runtime logs.
14. **Bounded overhead:** An ephemeral rejected proposal can be deleted without forcing indefinite retention of its full payload or transcript.

## 14. Open questions and targeted spikes

1. **Retention UX:** Should rejected proposals disappear immediately, remain until document history expires, or be user-configurable? Current bounded code does not answer this. [A:R-LIFECYCLE]
2. **Decision durability:** Does Smart Redaction need restartable review/history now, or only once agent edits leave the single workbench session? [A:R-REVIEW-STORE]
3. **Restamp lineage:** Should dry-run preserve an original base-state record and create a distinct revalidation/rebase receipt instead of overwriting the proposal base before apply? [R5][R7]
4. **Action proposal persistence:** Should pending/stale caption and annotation proposals live in the project manifest, a sidecar, or remain ephemeral? [A:R-ACTION-PROP]
5. **Publish validation:** Which deterministic media checks and digest policy are sufficient for GIF/MP4 publication receipts? [R9]
6. **Provider/model identifiers:** What identifiers are stable and useful across Anthropic/OpenAI adapters without pretending provider aliases are immutable model versions? [A:R-PROV]
7. **Skill provenance:** Should provenance use installed skill package digest, declared semantic version, source commit, or all available identifiers?
8. **Deletion:** Is a minimal tombstone allowed after “delete artifact,” and which user-visible promise governs derived exports/backups?
9. **Hyperframes integration:** Resolve the dispatch worker-wave ambiguity before making expected-output cardinality part of a product contract.

## 15. Exact audits

The negative claims above use bounded audits. “No matches” means only the named terms were absent from the named roots at the inspected revision.

### [A:R-GENERIC] Generic artifact and expected-output contract

```text
rtk rg -n -i \
  'product[_ ]?artifact|artifact_id|artifact_schema_version|expected[_ ]?(artifact|output)|completion_receipt|review_receipt' \
  crates/rollshot-agent/src \
  crates/rollshot-edit-proposal/src \
  crates/rollshot-app/src/result_workspace/workbench
```

Result: no matches.

### [A:R-PROV] Full producer/user-decision provenance fields

```text
rtk rg -n -i \
  'skill_(id|version|digest|authority|revision)|tool_(name|id|version|schema_version|digest)|model_(id|version|revision)|provider_(id|version|revision)|config_(id|version|digest|revision)|source_(hash|digest|revision)|decision_(actor|at)|reviewed_(by|at)' \
  crates/rollshot-edit-proposal/src/{proposal,review}.rs \
  crates/rollshot-automation/src/{frontend/mod,policy}.rs \
  crates/rollshot-preset/src/domain.rs \
  crates/rollshot-action/src/{caption_proposal,visual_annotation_proposal}.rs \
  crates/rollshot-action/src/project/model.rs
```

Result: no matches. This does not negate existing partial source/run provenance described in §5 and §8.

### [A:R-REVIEW-STORE] Review/pending proposal types in searched stores

```text
rtk rg -n \
  'ReviewDecision|CaptionProposal|VisualAnnotationProposal|pending_proposal|pending_draft' \
  crates/rollshot-preset/src \
  crates/rollshot-action/src/project \
  crates/rollshot-action/src/export \
  crates/rollshot-agent/src/provider.rs
```

Result: no matches.

### [A:R-ACTION-PROP] Action proposal persistence surface

```text
rtk rg -n \
  '#\[derive.*Serialize|#\[derive.*Deserialize|save|load|persist|store|archive|publish|retention|expires|delete_' \
  crates/rollshot-action/src/{caption_proposal,visual_annotation_proposal}.rs
```

Result: only a test name containing `trimmed_rationale_is_stored`; no serde/store contract was found.

### [A:R-LIFECYCLE] Retention/archive/delete fields

```text
rtk rg -n -i \
  'retention(_policy)?|retention_until|expires_at|tombstone|archived_at|deleted_at|redaction_policy|export_policy' \
  crates/rollshot-edit-proposal/src \
  crates/rollshot-automation/src \
  crates/rollshot-preset/src \
  crates/rollshot-action/src/project \
  crates/rollshot-action/src/caption_proposal.rs \
  crates/rollshot-action/src/visual_annotation_proposal.rs
```

Result: no matches.

### [A:H-RETENTION] Hyperframes workload retention boundary

The inspected pinned `production-loop.md`, `review-loop.md`, `subagent-dispatch.md`, general-video skill, and frame-worker delta specify production, review, dispatch, and expected files, but do not specify a general retention/deletion policy. This is a bounded source review, not a claim about every file in the Hyperframes repository.

### External profile audits

- **[P1:A4]** Pi reviewed profile, Artifact/status exact audit.
- **[P2:A4]** Oh My Pi reviewed profile, Artifact/status exact audit.
- **[P3:A6]** Codex reviewed profile, generic Artifact exact audit.
- **[P4:A1]** Claude Code reviewed profile, general Artifact declaration audit.

## 16. Evidence index

### Rollshot source and tests

- **[R1]** `crates/rollshot-automation/src/frontend/mod.rs`; `crates/rollshot-automation/src/executor.rs`; compatibility/frontend tests in the same crate. `ValidatedAutomation`, schema/API versions, canonical validation, and `ensure_compatible`.
- **[R2]** `crates/rollshot-preset/src/domain.rs`; `crates/rollshot-preset/src/store.rs`. Immutable automation revisions, parent lineage, active revision, provenance, compatibility checks, and atomic storage.
- **[R3]** `crates/rollshot-edit-proposal/src/proposal.rs`. Proposal/candidate identity, base state, confidence/rationale, provenance, and serialization.
- **[R4]** `crates/rollshot-edit-proposal/src/review.rs`. Review decision partition and deterministic lowering.
- **[R5]** `crates/rollshot-app/src/result_workspace/workbench/{state,run,review}.rs`. Pending proposal/draft handling, candidate review state, apply path, and proposal restamping.
- **[R6]** `crates/rollshot-image-document/src/document.rs` and its tests. State identity, atomic batch application, rollback, undo/redo, and flattening.
- **[R7]** `crates/rollshot-agent/src/{runtime,driver}.rs` and driver/runtime tests. Generation-bound evidence, dry-run, submit rules, and `ReadyForReview`.
- **[R8]** `crates/rollshot-action/src/project/{model,assets,store,error}.rs` and project tests. Manifest v2, revisions, frame hashes, validation, conflict handling, and atomic save.
- **[R9]** `crates/rollshot-action/src/project/publish.rs` and tests. Publish-state schema, revision freshness, and current output-presence checks.
- **[R10]** `crates/rollshot-action/src/{caption_proposal,visual_annotation_proposal}.rs` and tests. Proposal source binding, lifecycle, validation, rejection, and rebase behavior.

### Research context

- `docs/researchs/agent-foundation/README.md` — umbrella governance, terminology, evidence, and document lifecycle.
- `docs/researchs/agent-foundation/00-rollshot-baseline-workloads.md` — Round 0 workloads and success boundaries.
- `docs/researchs/agent-foundation/capabilities/persistence-checkpoint-resume.md` — persistence and immutable revision precedents.
- `docs/researchs/agent-foundation/capabilities/skills-and-extensions.md` — skill identity/trust boundary.
- `docs/researchs/agent-foundation/capabilities/task-todo-workflow-state.md` — task completion and workflow-state comparison.
- `docs/researchs/agent-foundation/capabilities/tools-and-scheduling.md` — tool execution evidence and scheduling boundary.
- `docs/researchs/agent-foundation/capabilities/long-running-jobs.md` — durable job/output relationship.
- `docs/researchs/agent-foundation/capabilities/subagents-and-parallelism.md` — subagent completion/aggregation boundary.
- `docs/researchs/agent-foundation/capabilities/context-compaction.md`, `memory.md`, `permissions-and-sandboxing.md`, and `budgets-cancellation-retries.md` — retention, authority, recovery, and bounded-execution context.

### Reviewed system profiles

- **[P1]** `docs/researchs/agent-foundation/systems/pi.md` — reviewed Pi profile, especially exact audit A4.
- **[P2]** `docs/researchs/agent-foundation/systems/oh-my-pi.md` — reviewed OMP profile, especially exact audit A4.
- **[P3]** `docs/researchs/agent-foundation/systems/codex.md` — reviewed Codex profile, especially exact audit A6.
- **[P4]** `docs/researchs/agent-foundation/systems/claude-code.md` — reviewed Claude Code profile, especially exact audit A1.

### Workload evidence

- **[B1]** `docs/ideas/2026-07-22-agent-skills-action-guide-launch-video.md`; `learn-projects/brag/skills/brag/SKILL.md` at Brag `357a805e`.
- **[H1]** Hyperframes `core/production-loop.md` and general-video skill at `807078c7`: dependency staging, frame/audio production, synchronization, assembly, verification, delivery, and edit-loop re-entry.
- **[H2]** Hyperframes `core/review-loop.md` at `807078c7`: plan, sketch, and final-look gates; explicit approval before final render.
- **[H3]** Hyperframes `core/subagent-dispatch.md`, general-video skill, and frame-worker delta at `807078c7`: best-effort notification, expected-file waits, paired HTML/motion outputs, and bounded redispatch.

## 17. Limitations

- This is a source/profile comparison, not a runtime evaluation of Pi, OMP, Codex, Claude Code, Brag, or Hyperframes.
- External negatives are limited to the exact reviewed-profile audits and roots cited; extensions, unreleased versions, or uninspected code may add narrower mechanisms.
- Rollshot negatives are limited to the exact commands and roots in §15. Similar fields under different names may exist elsewhere, but no evidence found here justifies treating them as a coherent Product Artifact contract.
- No storage migration, privacy UX, schema, or API has been selected or implemented.
- No final choice among Patterns A–C is made.
- The Hyperframes workload is deferred and its worker-wave ambiguity remains unresolved.
- Performance, storage growth, encryption-at-rest, backup behavior, and provider-side deletion need separate measurement/design if durable artifacts are selected.
