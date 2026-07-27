# Rollshot Agent Foundation Slice 3: Authority and Static Skills Design

**Date:** 2026-07-27  
**Status:** Approved for planning in brainstorming auto mode  
**Area:** Agent foundation / authority / static instruction skills  
**Governing design:**
[`2026-07-26-agent-foundation-umbrella-design.md`](./2026-07-26-agent-foundation-umbrella-design.md)

## 1. Purpose

This slice adds the minimum trustworthy authority and instruction-skill
foundation to the existing bounded Smart Redaction author/improve workflow.
Rollshot will construct one immutable authority snapshot for each run, enforce
that snapshot before every tool call, resolve one bounded host-owned instruction
skill into an immutable `SkillUse`, and bind the resulting authority and skill
provenance to the Product Task and review artifact introduced by Slice 2.

The proof is one bundled Smart Redaction instruction skill. It reuses the
current tools, provider-neutral driver, budgets, cancellation, validation,
dry-run, artifact promotion, and explicit review. It adds no user-visible UI and
no new Smart Redaction product behavior.

## 2. Readiness and current-code drift

Slice 2 is merged on `main` as `1745133` / PR #103. Its historical Gate G1
decision record still says "Proposed for user approval" and must not be edited.
The merge plus this explicit request to plan the next slice supplies the
progression decision for this workflow. Current verification on 2026-07-27 also
passed:

- `rtk cargo test -p rollshot-agent`: 284 passed;
- `rtk cargo test -p rollshot-app task_store`: 30 passed.

Current code confirms the research gap still exists:

- `ToolRegistry` is an availability, schema, limit, serial-order, and
  cancellation boundary, but `Tool::call` and `execute_calls` receive no
  authority object;
- `AuthorizedModelInput` records upstream disclosure decisions but does not
  represent an immutable run authority snapshot;
- Smart Redaction instructions are a monolithic constant in `driver.rs`;
- the product directly composes the Smart Redaction tool registry in
  `result_workspace/workbench/run.rs`;
- Slice 2 persists Product Task, attempt, run, document, proposal, artifact, and
  review identity, but not authority or skill-use provenance; and
- there is no skill catalog, manifest, invocation record, digest pin, or skill
  resource resolver.

The Slice 2 implementation therefore satisfies the dependency needed by this
slice, while its concrete persistence and launch paths replace the older
research baseline.

## 3. Goals

1. Construct an immutable, privacy-safe `AuthoritySnapshot` from current
   Product intent, disclosure consent ceiling, current document content
   binding, policy revision, prepared capability availability, and exact
   run-local operation grants.
2. Make every registered tool declare typed authority requirements and make
   `ToolRegistry` enforce them before dispatch, independently of model-visible
   tool availability or skill content.
3. Add a bounded static host catalog for declarative instruction packages with
   deterministic ordering, source precedence, strict metadata, safe local-path
   handling, immutable resolved bytes, and content digests.
4. Add an explicit host invocation contract that resolves an exact package and
   digest into one immutable `SkillUse`; no model search or routing is needed.
5. Move reusable Smart Redaction author/improve instructions and examples into
   one bundled skill while retaining a small Rollshot-owned non-overridable
   system envelope.
6. Persist privacy-safe authority and skill-use receipts before the first model
   or tool operation, then bind the same receipts to the promoted review
   artifact.
7. Prove that skill text, metadata, catalog membership, and invocation cannot
   grant filesystem, network, process, image-disclosure, or Product-document
   mutation authority.

## 4. Non-goals

This slice does not add:

- a marketplace, package installer, dependency solver, package publisher,
  update service, project-local discovery, or user-managed skill settings;
- model-selected or semantic skill routing, skill search, implicit invocation,
  or a model-visible list/read skill tool;
- remote, executor, orchestrator, MCP, or environment-bound skill providers;
- scripts, inline shell expansion, JavaScript/Wasm/native extensions, hooks,
  tool registration by skills, or skill-defined policy;
- a general operating-system sandbox, live authority broker, approval cache,
  credential broker, revocation service, or durable authority lease;
- runtime permission prompts, new screen capture, new image disclosure,
  filesystem operations, network operations, subprocesses, publishing, or
  direct document mutation;
- durable full skill bodies, model transcripts, image pixels, OCR text,
  credentials, or local source paths;
- a workflow DAG, retry system, child agents, jobs, context compaction, or audit
  event store; or
- any user-visible iced UI change or visual-baseline update.

## 5. Alternatives considered

### 5.1 Selected: immutable authority snapshot plus static host catalog

The Product builds a typed snapshot for one run. A bounded host catalog resolves
an exact instruction package into immutable bytes and a digest. Tools retain
their existing executors, but the registry rejects a call unless the snapshot
contains every declared requirement.

This is the smallest approach that satisfies the umbrella: it fits the current
run-local registry, preserves Product ownership, supports deterministic
provenance, and proves one skill without creating a provider or plugin system.

### 5.2 Rejected: compiled prompt constant plus authority checks

Keeping the Smart Redaction text in `driver.rs` and merely attaching a digest
would be smaller, but it would not prove manifest validation, bounded catalog
loading, explicit invocation, containment failures, or static-skill provenance.
It would not satisfy Gate G2.

### 5.3 Rejected: authority broker plus provider-based skill catalog

Operation tokens and Host/Executor/Orchestrator providers would improve remote
handoff and revocation, but no current workload requires them. They add
credential, environment, networking, caching, retry, and availability semantics
explicitly deferred by the umbrella.

## 6. Architecture

```text
Product-owned Smart Redaction request
        |
        +--> Product Task / attempt / run / document binding (Slice 2)
        |
        +--> bundled + allowlisted host skill sources
        |        |
        |        `--> bounded StaticSkillCatalog snapshot
        |                    |
        |                    `--> exact host invocation --> SkillUse
        |
        +--> disclosure ceiling + policy revision + prepared capabilities
        |                    |
        |                    `--> immutable AuthoritySnapshot
        |
        +--> persist RunContractReceipt before provider/tool work
        |
        v
Rollshot system envelope + bounded invoked skill body
        |
        v
provider-neutral AgentRunner
        |
        v
model requests an advertised tool
        |
        v
ToolRegistry checks declared requirements against AuthoritySnapshot
        | deny                         | allow
        v                              v
typed fail-closed terminal       existing executor/tool
                                       |
                                       v
                         validate -> dry-run -> review proposal
                                       |
                                       v
                    Product Artifact + authority/skill receipts
                                       |
                                       v
                              explicit user review
```

The four states stay distinct:

1. **Catalog availability:** a validated package is present in the immutable
   catalog snapshot.
2. **Invocation:** the Product explicitly selects an exact package revision and
   receives a `SkillUse`.
3. **Authority grant:** the Product creates an independent operation allow-set
   in `AuthoritySnapshot`.
4. **Execution:** `ToolRegistry` admits a concrete tool call and the existing
   executor enforces its own validation, budget, cancellation, and output
   boundaries.

No transition from (1) or (2) creates (3).

## 7. Authority model

### 7.1 Snapshot identity and ownership

`AuthoritySnapshot` is a closed, immutable value created by Product code and
shared by immutable reference for one Agent Run. It contains:

- Product Task ID, attempt ID, and run ID;
- the exact `DocumentContentBinding` / `SourceBinding` used by Slice 2;
- a stable authority schema version and Product policy revision;
- the selected model-disclosure ceiling (`ocr_layout_only` or
  `full_screenshot`) as Product consent evidence, never as permission to widen
  the actual payload;
- Product-owned evidence that the screenshot is already captured for this
  document; no new OS Screen Capture request is made;
- prepared capability availability for region features, OCR, layout, and
  template matching;
- the exact set of granted `RunOperation` values; and
- a canonical privacy-safe digest.

The snapshot contains no image bytes, OCR text, model key, provider-native
value, transcript, arbitrary path, or skill body. It has no setter, interior
mutability, merge operation, or "grant from text" parser.

The current Smart Redaction path does not acquire a protected OS service during
an agent run. Its OS-permission evidence is therefore explicit
`existing_product_capture`; the slice must not pretend an old OS permission is
a fresh run grant. A later capture/input/job slice must define its own live
lease or revocation semantics rather than stretching this snapshot.

### 7.2 Closed operation vocabulary

The first schema uses a closed enum whose variants describe current Smart
Redaction operations only:

- `ReadDraft`;
- `WriteDraft`;
- `InspectPreparedImage`;
- `ExecuteRestrictedAutomation`;
- `SubmitReviewCandidate`; and
- `RequestUserInput`.

These operations authorize access only to objects already bound into the run.
They do not represent general filesystem read/write, network, process,
credential, screen-capture acquisition, export/publish, or Product-document
mutation. Adding such a variant is a public authority-contract change and
requires a new design decision.

### 7.3 Tool declaration and enforcement

Every `Tool` implementation returns a static, typed set of required
`RunOperation` values. Test tools must declare requirements too. Before calling
`Tool::call`, `ToolRegistry` checks that:

1. the snapshot task/run/document binding matches the active tool context;
2. every declared operation is granted;
3. the call is still within existing cancellation and registry limits; and
4. the registered tool is the exact tool requested by the model.

A missing requirement returns a typed `ToolError::AuthorityDenied` containing
only bounded operation and tool identifiers. The tool body is never entered,
subsequent calls in the batch do not execute, no fallback registry is built, and
the driver maps the result to an honest fail-closed terminal. A denial never
adds a grant, widens payload mode, retries with ambient authority, or changes
skill availability.

Tool advertisement remains an availability decision. Production composition
normally advertises only the intended narrow set, but contract tests must prove
that even an advertised and registered tool cannot execute without its grant.
This preserves defense in depth instead of equating visibility with authority.

### 7.4 Provider-input ceiling

Before the first provider call, the runner verifies that the
`AuthorizedModelInput` attachment descriptors and bytes do not exceed the
snapshot disclosure ceiling. `ocr_layout_only` admits no screenshot attachment;
`full_screenshot` is a ceiling, not a requirement to send bytes. This slice does
not change the current Smart Redaction attachment-delivery behavior.

A mismatch fails before stream establishment. Skill text cannot alter the
snapshot, `AuthorizedModelInput`, or this check.

## 8. Static skill catalog

### 8.1 Package shape

A package contains exactly two immediate regular files:

- `skill.toml` — UTF-8 manifest;
- `SKILL.md` — UTF-8 main instruction body.

The V1 manifest has only:

- `schema_version = 1`;
- `package_id` — stable lowercase ASCII kebab-case, at most 64 bytes;
- `name` — display name, at most 64 Unicode scalar values;
- `description` — metadata summary, at most 512 bytes;
- optional `declared_version` — display metadata, at most 64 bytes; and
- `main = "SKILL.md"`.

Unknown fields, duplicate fields, unsupported schema versions, invalid IDs,
missing descriptions, alternate main paths, non-UTF-8 data, and metadata limits
are hard failures for that package. The manifest has no tool list,
`allowed-tools`, permissions, hooks, scripts, model selection, shell expansion,
or policy fields.

### 8.2 Sources and deterministic precedence

The host supplies an ordered source list:

1. Rollshot-bundled packages;
2. explicitly configured host-owned roots in caller order.

No project directory, ancestor traversal, home-directory convention, or ambient
plugin path is discovered automatically. Catalog entries are ordered by source
tier, source index, then package ID. Bundled packages win a duplicate package
ID. Among host roots, the earlier explicit root wins. Every shadowed duplicate
produces a bounded collision diagnostic; replacement is never silent.

A malformed optional package is omitted with a typed diagnostic and cannot
change authority or the precedence of an already selected package. Failure of
the required Smart Redaction package aborts the run before provider/tool work;
there is no fallback to the old monolithic prompt.

### 8.3 Bounds

V1 hard limits are:

- at most 1,000 catalog entries;
- at most 2 files per package and one directory level below a root;
- at most 4 KiB for `skill.toml`;
- at most 16 KiB for `SKILL.md`;
- at most 128 KiB of accepted catalog metadata; and
- exactly one main resource per package.

The loader stops accepting entries once a hard aggregate bound is reached and
returns the deterministic accepted set plus an explicit omission count and
bounded diagnostics. The selected package body must fit in full; bodies are
never silently truncated because truncation would change instructions while
obscuring the digest.

A 1,000-entry deterministic resource test proves bounded ordering, counts,
bytes, and completion under a documented test threshold. The threshold is a
regression signal, not a real-time scheduling guarantee.

### 8.4 Containment and file safety

For host roots on supported Unix product platforms, loading is descriptor-
relative and no-follow:

- the caller explicitly opens the allowlisted root;
- package names are single validated path components;
- package directories and both files are opened relative to their parent with
  no-follow semantics;
- every opened object is verified by metadata as the expected directory or
  regular file;
- absolute paths, separators, `.`/`..`, symlinks, FIFOs, sockets, devices, and
  extra main-resource paths are rejected; and
- limits are checked while reading, not only after allocating the complete
  file.

This shape avoids recursive traversal and closes validation/read symlink swaps
for the two allowed files. Unsupported platform implementations fail closed
rather than falling back to ambient path joins.

Bundled package bytes are compiled into the binary and enter the same manifest,
limit, digest, and invocation validation pipeline without acquiring an ambient
filesystem path.

### 8.5 Digest and immutable catalog snapshot

The package digest is SHA-256 over a domain-separated canonical V1 sequence of
security-relevant manifest fields and exact `SKILL.md` bytes. The declared
version is descriptive; the digest identifies observed content.

A `StaticSkillCatalog` owns immutable resolved bytes and metadata. It never
rereads a path during the run. Refreshing the host catalog creates a new
snapshot; it cannot mutate an existing `SkillUse`.

## 9. Invocation and prompt composition

### 9.1 Explicit host invocation

The Product invokes a skill with an exact request:

- authority/source ID;
- package ID;
- optional expected package digest; and
- invocation kind `host_explicit`.

The catalog returns `SkillUse` containing source authority, package ID, main
resource ID, package digest, optional declared version, invocation kind,
resolution timestamp, and immutable bounded body bytes. Unknown, unavailable,
or digest-mismatched requests fail with typed errors. A digest mismatch never
substitutes the current package.

There is no invocation argument language in V1. Smart Redaction author versus
improve mode remains Product Task/run input and current reviewed evidence, not a
skill permission or mutable package setting.

### 9.2 Smart Redaction prompt split

The existing monolithic Smart Redaction prompt is split into:

1. a small Rollshot-owned system envelope that fixes task scope, instruction
   precedence, disclosure boundaries, available-tool truth, refusal behavior,
   and the rule that skill content grants no authority; and
2. the exact bundled skill body containing the JavaScript authoring guide,
   inspection loop, authoring loop, improve guidance, and examples.

The runner composes these once per run and uses the same composed prompt on each
provider turn. The skill body is delimited and identified by package ID and
digest. It cannot replace the system envelope or tool definitions.

Prompt regression tests preserve the current author and improve instructions
and ensure all existing example programs still validate. The product uses the
same bundled skill for both modes; no UI selector is added.

## 10. Product Task and artifact provenance

### 10.1 Run contract receipt

Before the first provider call or tool call, the Product persists a
`RunContractReceiptV1` on the active `TaskAttempt` through exact CAS. It contains
only:

- task, attempt, and run correlation;
- authority snapshot schema/policy version and digest;
- disclosure-ceiling label;
- document content-binding digest;
- sorted granted operation identifiers;
- `SkillUseReceiptV1` with source authority, package ID, main resource ID,
  package digest, optional declared version, and invocation kind; and
- creation timestamp.

The receipt excludes the skill body, manifest path, screenshot, OCR text,
transcript, provider key, and tool arguments/results. Binding is legal only once
while the matching attempt is `Running`. A conflicting receipt or changed
source binding is stale and fails before execution.

The launch sequence is therefore:

1. persist Slice 2 `Running` snapshot;
2. resolve the required bundled skill and prepare current capabilities;
3. construct immutable authority and skill-use values;
4. CAS-bind the run contract receipt;
5. construct the registry and model input;
6. start provider/tool work.

Setup failures before step 4 persist an honest terminal under existing Slice 2
rules. No external model or tool effect occurs without the bound receipt.

### 10.2 Artifact binding

A successful `ReadyForReview` artifact copies the exact authority digest and
`SkillUseReceiptV1` from the attempt receipt into artifact metadata. The new
run-config V2 fingerprint includes both, so artifact provenance is content-
bound rather than merely adjacent metadata. Artifact promotion rejects any
receipt mismatch.

Review restore and apply continue to use Slice 2 task/artifact/document
revision checks. A matching skill digest never overrides a stale document or
artifact revision.

### 10.3 Persistence migration

Slice 3 bumps Product Task store schema to V2 and artifact/run-config schema to
V2. Existing V1 snapshots remain readable through explicit serde defaults or a
bounded V1-to-current loader path. Existing V1 ReadyForReview artifacts remain
reviewable under their original semantics, but are identified as having no
skill-use receipt; they are never relabeled as Slice 3 skill-backed runs.

All new Smart Redaction attempts require a bound V1 run-contract receipt before
provider/tool execution. Stores reject schema versions newer than V2. No
historical file is rewritten merely by startup reconciliation.

## 11. Failure semantics

New failures are typed at their owning boundary:

- authority: identity mismatch, document mismatch, disclosure mismatch, missing
  operation, unsupported authority schema;
- catalog: invalid manifest, catalog/metadata limit, duplicate collision,
  unsupported source/platform;
- resource: invalid component, containment violation, symlink rejected,
  special file rejected, body too large, invalid UTF-8;
- invocation: unavailable authority, unknown package/resource, digest mismatch,
  stale revision; and
- persistence: conflicting run receipt, missing receipt at promotion, receipt
  mismatch, unsupported V2 schema.

Rules:

- all required-skill, snapshot, persistence, and authority failures occur before
  model/tool execution where possible and fail closed;
- a malformed optional catalog entry never broadens the remaining catalog or
  operation grants;
- no failure falls back to the old inline prompt, ambient filesystem access, a
  broader payload mode, or an unguarded tool call;
- cancellation after skill resolution but before a tool call consumes no new
  authority and uses the existing run terminal;
- cancellation during a tool uses the existing shared cancellation source and
  does not trigger an implicit retry;
- partial provider or tool output cannot become a successful artifact; and
- stale task, document, authority, skill, or artifact identity is never silently
  substituted.

## 12. Privacy and diagnostics

Durable data may contain opaque task/run IDs, schema/policy versions, package
and resource IDs, content digests, operation labels, bounded error categories,
and accepted review decisions. It must not contain full skill bodies, local
catalog paths, image pixels, OCR text, user messages, provider credentials,
provider-native conversation state, or unrestricted tool data.

All new runtime diagnostics use stable explicit `rollshot::*` targets and
structured fields. Catalog and invocation events may log source class, package
ID, digest prefix or full digest, counts, and bounded error code. They never log
body content or ambient paths. Custom `Debug` implementations or redacted
fields cover any type that owns skill bytes or sensitive Product input.

Privacy tests inspect serialization, `Debug`, and tracing-visible error strings.

## 13. Testing strategy

### 13.1 Authority contracts

Tests are written before implementation and prove:

- every production Smart Redaction tool declares expected requirements;
- advertised + registered but ungranted tools never enter `Tool::call`;
- multi-requirement tools fail when any one operation is absent;
- task/run/document and disclosure mismatches fail before provider/tool work;
- denied calls stop the serial batch and do not mutate draft, evidence, review,
  image, or Product document state;
- cancellation and existing argument/result/call limits remain effective; and
- adversarial skill text naming filesystem, network, process, image disclosure,
  or document mutation cannot change the snapshot or execute an absent tool.

### 13.2 Catalog and invocation contracts

Tests cover:

- manifest parsing, unknown fields, identifier and metadata limits;
- exact deterministic ordering and duplicate precedence;
- 1,000-entry bounds, metadata budget, omission count, and regression threshold;
- absolute/parent/separator rejection;
- root, package, manifest, and body symlinks;
- symlink replacement between enumeration and open where the platform permits
  deterministic injection;
- FIFO/socket/special-file rejection;
- oversize manifest/body with bounded reads;
- invalid UTF-8;
- canonical digest golden vectors and manifest/body change sensitivity;
- immutable snapshot behavior after backing-file replacement;
- exact invocation, unknown package, digest mismatch, and no substitution; and
- custom `Debug`/serialization privacy.

### 13.3 Product integration and regression

Tests cover:

- persistence-before-provider/tool ordering;
- one-time run-contract CAS binding and stale/conflicting receipt rejection;
- V1 snapshot loading and V2 round trip without startup rewrite;
- artifact/run-config V2 binding to exact authority and skill receipts;
- author and improve modes using the same bundled skill;
- unchanged budgets, cancellation, validation, dry-run, proposal, artifact,
  review, and stale-document behavior;
- the existing prompt examples still validating after migration into
  `SKILL.md`;
- no user-visible UI changes; and
- no provider-specific or Rig types entering public authority/skill contracts.

Required commands include affected focused tests, `rtk cargo test -p
rollshot-agent`, relevant `rollshot-app` workbench/task-store tests, `rtk cargo
fmt --check`, and risk-appropriate clippy. Independent code review remains a
Gate G2 requirement.

## 14. Gate G2 acceptance

Gate G2 passes only when current evidence shows all of the following:

1. the required bundled Smart Redaction package is explicitly selected and
   boundedly loaded for author and improve runs;
2. catalog order, limits, duplicate behavior, containment, symlink, special-file,
   oversize, UTF-8, and digest behavior are executable tests;
3. the attempt receipt is persisted before provider/tool work and the promoted
   artifact carries the exact same skill digest and authority digest;
4. every tool call receives an independent pre-dispatch authority check;
5. skill content and metadata cannot add tools, grants, scripts, filesystem,
   network, process, image-disclosure, or document-mutation authority;
6. budget, cancellation, validation, dry-run, proposal review, artifact
   staleness, and apply/reject semantics remain unchanged;
7. V1 Product Task records remain readable and new V2 records are
   crash-consistent under the existing exact-CAS store;
8. durable and diagnostic provenance is privacy-safe and retains no full skill
   body;
9. affected tests, formatting, risk-appropriate lint, and independent review
   pass; and
10. residual risks and migration evidence are recorded in the Gate G2 decision.

Passing Gate G2 proves only the trustworthy minimum skill foundation. It does
not authorize launch-video work, remote skill providers, executable extensions,
or Phase 3 implementation.

## 15. Residual risks and stop conditions

- A run-local snapshot does not provide mid-run OS/policy revocation. This is
  acceptable for the current already-captured, bounded Smart Redaction run; a
  workload requiring live revocation must stop and design a broker/lease.
- The static host loader is intentionally constrained to two files and one
  directory level. A need for arbitrary resources, project-local discovery, or
  cross-environment IDs is a new design, not a reason to loosen containment.
- The current Smart Redaction provider attachment-delivery behavior is outside
  this slice. Any attempt to change which pixels are uploaded must stop for a
  separate disclosure review.
- If robust no-follow descriptor-relative loading cannot be implemented on both
  active product platforms without an unsafe or ambient-path fallback, stop and
  run a bounded platform spike rather than weakening the contract.
- If the Product Task V2 migration cannot preserve existing pending V1 review
  artifacts, stop and revise the migration before enabling skill-backed runs.
