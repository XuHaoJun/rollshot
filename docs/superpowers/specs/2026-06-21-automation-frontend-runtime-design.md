# Automation Frontend and Runtime Design (Subproject 3)

**Date:** 2026-06-21
**Status:** Approved design
**Parent:** `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md` (Delivery Decomposition §12, subproject 3)
**Spike inputs:** `docs/superpowers/spikes/2026-06-20-spike-decisions.md`
**Prerequisites completed:** commits `3250d84cdd728e1b59c546a18636f46c39594456` and `59bac8728fea37aa4fd5551dc9b865e60ee70ada`

## 1. Summary

This subproject builds the compiler-like frontend and replaceable sandbox runtime for Rollshot automation source.

The frontend accepts a restricted JavaScript program with one explicit `main(input)` entry point and optional pure helper functions. It parses source with oxc, validates the versioned language subset, normalizes accepted source into persisted Workflow IR, calculates static costs and capability usage, and produces diagnostics suitable for both users and an agent repair loop.

The runtime executes validated canonical source in a fresh, hardened QuickJS context through a Rollshot-owned executor interface. Capabilities are supplied by a typed `AutomationHost`; this subproject provides deterministic fake hosts for tests but does not implement real OCR, layout, region-feature, or template-matching detectors.

Execution may propose the complete `ProposedEdit` CRUD surface, but every operation remains a candidate. Rust validates the output and execution policy before producing an `EditProposal`; this subproject never mutates `ImageDocument`.

## 2. Scope

### 2.1 In scope

- New `rollshot-automation` crate:
  - oxc-based parser integration.
  - Language Schema v1 and source diagnostics.
  - lexical-scope, purity, call-graph, and subset validation.
  - Workflow IR normalization and versioned serialization.
  - capability manifests, static-cost analysis, semantic summaries, and semantic diffs.
  - typed automation input, capability queries/results, execution policy, output schema, and errors.
  - `AutomationHost` and `AutomationExecutor` interfaces.
  - strict conversion from runtime output to `rollshot-edit-proposal` types.
- A focused `rollshot-edit-proposal` extension adding the candidate `label`
  required by Output Schema v1 and later visual review.
- New `rollshot-automation-rquickjs` crate:
  - hardened rquickjs executor implementation.
  - fresh runtime/context per execution.
  - frozen host API and recursively frozen input.
  - memory, stack, interruption, capability-call, output, and cancellation enforcement.
  - adversarial sandbox tests.
- Capability API v1 definitions for OCR, layout, region features, and template matching.
- Deterministic fake host implementations and end-to-end fixtures.
- Complete `ProposedEdit` CRUD output support, constrained by per-run policy.
- Exact dependency pins for parser and runtime.
- A completion handoff for downstream subprojects.

### 2.2 Out of scope

- Real OCR, layout, region-feature, or template-matching adapters.
- Agent sessions, provider adapters, Rig integration, or the bounded agent loop (subproject 4).
- Preset, revision, session, or run persistence (subproject 5).
- Workbench UI, source editor, diagnostics rendering, visual review, or app feature wiring (subproject 6).
- Improve Preset evidence and regression fixtures (subproject 7).
- Product launch, save/copy handoff, or platform runtime verification (subproject 8).
- Executing Workflow IR as an alternative runtime.
- Arbitrary JavaScript, modules, async work, user-defined mutable state, or ambient platform APIs.

## 3. Crate and Dependency Boundaries

```text
rollshot-automation
  Language Schema v1
  oxc parser adapter
  validator and diagnostics
  Workflow IR and semantic diff
  capability and cost contracts
  AutomationHost trait
  AutomationExecutor trait
  strict output decoder
  EditProposal construction
        |
        | executor interface
        v
rollshot-automation-rquickjs
  rquickjs runtime adapter
  context lockdown
  frozen rollshot API
  resource enforcement
  adversarial tests
```

`rollshot-automation` depends on:

- `oxc_allocator = "=0.137.0"`
- `oxc_ast = "=0.137.0"`
- `oxc_parser = "=0.137.0"`
- `oxc_span = "=0.137.0"`
- `rollshot-edit-proposal`
- `rollshot-image-document`
- workspace `serde`, `serde_json`, `thiserror`, and `tracing`

`rollshot-automation-rquickjs` depends on:

- `rquickjs = "=0.12.0"`
- `rollshot-automation`
- workspace `serde_json`, `thiserror`, and `tracing`

oxc is the sole parser implementation. There is no parser-backend Cargo feature and no source-level parser abstraction whose only implementation is oxc.

The executor boundary is a Rust trait because the runtime is explicitly replaceable. rquickjs types must not appear in `rollshot-automation` public APIs, persisted revisions, agent tool contracts, or UI-facing models.

Dependency upgrades are explicit engineering changes. Updating an oxc or rquickjs pin requires rerunning the complete language-contract, compatibility, and adversarial sandbox suites.

## 4. Feature Contract

Subproject 3 does not add a `rollshot-app` feature or otherwise wire the new crates into the product app.

Subproject 6 will introduce:

```toml
[features]
default = ["smart-redaction"]
smart-redaction = [
    "dep:rollshot-automation",
    "dep:rollshot-automation-rquickjs",
    # Later subproject dependencies are added when they exist.
]
```

The final contract is:

- Smart Redaction is included in default product builds.
- `--no-default-features` removes its UI entry points, messages, settings, and automation/agent dependency graph completely.
- The disabled build does not show an unavailable or upsell placeholder.
- Cargo features select the product capability, not parser or runtime behavior inside the automation crates.

The feature skeleton is intentionally deferred until the Workbench has an actual app integration point.

## 5. Source Form and Language Schema v1

### 5.1 Canonical program shape

Canonical automation source is a complete JavaScript script, not a wrapped function body or module:

```javascript
function expandBounds(rect, padding) {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}

function main(input) {
  const matches = rollshot.ocr({
    region: input.region,
    limit: 100,
  });

  const candidates = matches
    .filter((match) => match.confidence > 0.8)
    .map((match) => ({
      kind: "addRedaction",
      bounds: expandBounds(match.bounds, 8),
      confidence: match.confidence,
      label: "ocr-match",
    }));

  return { candidates };
}
```

Rules:

- Exactly one top-level `function main(input)` is required.
- `main` is synchronous, non-generator, and has exactly one parameter named `input`.
- `main` has exactly one top-level `return`, and it is the final statement.
- Any number of top-level named helper function declarations may precede or follow `main`.
- No other top-level statements or declarations are allowed.
- Source is parsed directly by oxc, so diagnostics use source-native byte spans without wrapper offsets.

### 5.2 Pure helper functions

Helpers permit temporary mathematical calculations and data-structure transformations without broadening the language into general JavaScript.

A helper:

- may read only its parameters, local `const` bindings, and other approved pure helpers;
- may create and return finite primitive, object, and array values;
- may use approved expressions, conditions, and bounded collection operators;
- may not read `input` or `rollshot`;
- may not read or capture bindings declared outside the helper;
- may not call capabilities;
- may not mutate parameters, objects, arrays, or outer state;
- may not directly or indirectly recurse.

The validator builds a helper call graph and rejects every cycle. Call depth is calculated from this acyclic graph and checked against static limits.

### 5.3 Allowed syntax

Language Schema v1 allows only:

- finite string, Boolean, null, and numeric literals;
- bounded object and array literals;
- `const` declarations with an initializer;
- direct identifier references resolved by the validator;
- direct static property access on known-safe values;
- unary `!`, unary `+`, and unary `-` on approved value types;
- bounded arithmetic `+`, `-`, `*`, `/`, and `%`;
- equality, ordering, and Boolean expressions;
- `if` statements and conditional expressions;
- direct calls to statically resolved pure helpers;
- direct calls from `main` to the four `rollshot.*` capabilities;
- direct calls to the approved pure `Math` functions;
- pure arrow callbacks used immediately by approved collection operators;
- `map`, `filter`, `some`, and `every` on statically bounded arrays;
- one final top-level return from each function.

String concatenation through `+` is allowed within configured output string limits. Division or modulo by zero is a typed runtime failure; non-finite numbers are rejected at the output and host boundaries.

The only built-in namespace exposed by Language Schema v1 is this frozen pure
subset:

```text
Math.abs
Math.ceil
Math.floor
Math.round
Math.trunc
Math.min
Math.max
Math.sqrt
Math.hypot
```

Calls use direct static names and a statically bounded argument count.
`Math.random`, transcendental functions, constants, and all other built-ins are
rejected in v1. More pure operations require a language-schema revision.

### 5.4 Rejected syntax and behavior

Language Schema v1 rejects:

- `let`, `var`, assignment, update expressions, or mutation;
- computed or optional property access;
- destructuring, spread, rest parameters, and default parameters;
- imports, exports, modules, dynamic import, or module loaders;
- async functions, promises, `await`, timers, microtasks, workers, or generators;
- classes, constructors, `new`, `this`, `super`, or private fields;
- exceptions, `throw`, `try`, or `catch`;
- `while`, `do`, `for`, `for-in`, or `for-of`;
- `reduce`, `flatMap`, sorting, or other non-allowlisted collection methods;
- function expressions or closures stored, returned, passed elsewhere, or allowed to escape an approved collection call;
- direct or indirect recursion;
- `eval`, `Function`, `Reflect`, `Proxy`, prototype access, or prototype mutation;
- unknown globals or ambient JavaScript APIs;
- dynamic capability or helper names;
- aliasing `rollshot` methods before invocation;
- calls through `.call`, `.apply`, `.bind`, or equivalent indirection.

Unsupported constructs fail validation before execution even if QuickJS would support them.

### 5.5 Name resolution and purity

The validator performs lexical name resolution rather than string matching:

- every identifier resolves to a parameter, local `const`, approved helper,
  `main`-only `input`, `main`-only `rollshot`, or the approved `Math` namespace;
- duplicate declarations and shadowing of `main`, `input`, `rollshot`, helpers, or approved built-ins are rejected;
- a helper that references `input`, `rollshot`, or a binding outside its own scope is rejected;
- callbacks may capture immutable bindings from their containing function but may not escape the immediate approved collection call;
- callback parameters and local bindings follow the same no-mutation rules.

### 5.6 Diagnostics

Every parse, subset, name-resolution, purity, call-graph, normalization, and static-cost error contains:

```rust
pub struct SourceDiagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub primary_span: SourceSpan,
    pub related: Vec<RelatedDiagnostic>,
}

pub struct SourceSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}
```

Diagnostic codes are stable within a language schema version. Messages are repair-oriented but do not become part of the compatibility contract. The original source remains canonical; the oxc AST is transient and is never persisted.

Lines and columns are one-based and refer to UTF-8 byte positions. The byte
range is half-open (`start_byte..end_byte`).

## 6. Capability API v1

### 6.1 JavaScript surface

Only `main` may call:

```javascript
rollshot.ocr(query)
rollshot.layout(query)
rollshot.regionFeatures(query)
rollshot.templateMatch(query)
```

All calls are synchronous from JavaScript's perspective. Capability objects, methods, query objects, result arrays, and result objects are recursively frozen before they become observable to automation code.

Each query includes an explicit positive integer `limit`. The validator requires a literal or statically bounded expression no greater than the installed capability contract maximum. This makes collection cardinality statically analyzable.

### 6.2 Rust host interface

```rust
pub trait AutomationHost {
    fn ocr(&mut self, query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError>;
    fn layout(&mut self, query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError>;
    fn region_features(
        &mut self,
        query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError>;
    fn template_match(
        &mut self,
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError>;
}
```

The exact query and result structs are Rollshot-owned, serializable, versioned types. They contain only bounded data needed by the language contract and never expose product internals.

Host requirements:

- enforce the query `limit` independently of JavaScript validation;
- enforce host-side allocation and payload-byte limits;
- return only finite coordinates and scores;
- avoid blocking work in the QuickJS callback;
- target less than 1 ms per callback;
- return a typed error when work must be scheduled elsewhere or exceeds the synchronous contract.

Real detector adapters are explicitly deferred. Tests use deterministic fake hosts covering success, empty results, bounded truncation, malformed host data, and typed failure.

### 6.3 Capability manifest

Validation emits a manifest containing:

- capability API version;
- each capability name used;
- source spans for each call site;
- maximum calls per capability;
- maximum result count per call;
- maximum aggregate result count;
- required input fields;
- possible proposed edit kinds.

Unknown capabilities or capability API mismatches prevent execution.

## 7. Workflow IR

### 7.1 Purpose

Workflow IR is a persisted semantic representation used for:

- capability manifests;
- static maximum-step and capability-call analysis;
- candidate amplification bounds;
- semantic summaries and diffs;
- compatibility checks;
- future workflow visualization.

Workflow IR is not executed in v1. The validated canonical JavaScript source is the only executable artifact.

### 7.2 Shape

The serializable IR contains:

```rust
pub struct WorkflowIr {
    pub ir_schema_version: IrSchemaVersion,
    pub entry: FunctionId,
    pub helpers: Vec<IrFunction>,
    pub nodes: Vec<IrNode>,
    pub output: NodeId,
    pub capability_manifest: CapabilityManifest,
    pub static_cost: StaticCost,
    pub possible_edit_kinds: BTreeSet<ProposedEditKind>,
}

pub enum IrNodeKind {
    CapabilityCall(CapabilityCallIr),
    HelperCall(HelperCallIr),
    CollectionMap(CollectionIr),
    CollectionFilter(CollectionIr),
    CollectionSome(CollectionIr),
    CollectionEvery(CollectionIr),
    Condition(ConditionIr),
    Transform(TransformIr),
    EmitCandidates(EmitCandidatesIr),
}
```

Every function and node retains its originating source span. IR IDs are deterministic for the same accepted source and schema version.

### 7.3 Static cost

Static analysis calculates upper bounds for:

- AST node count;
- source bytes and literal bytes;
- helper count and maximum helper call depth;
- capability calls by kind;
- aggregate capability results;
- collection traversals;
- transform steps;
- output candidate count;
- output bytes using configured per-field maxima.

Boundedness rules:

- every capability result is bounded by its validated query `limit`;
- `input.annotations` and any other input array carry a policy-provided maximum;
- array literals are bounded by literal length;
- `map` preserves cardinality;
- `filter` can only reduce cardinality;
- `some` and `every` return one Boolean;
- helpers preserve the bounds of values passed through their call sites;
- unsupported cardinality-changing operators are rejected;
- a data path whose upper bound cannot be proven is rejected.

Validation fails before execution when any static upper bound exceeds `ValidationLimits`.

### 7.4 Semantic summary and diff

A semantic summary describes:

- capabilities and maximum calls;
- confidence thresholds and conditions;
- region, padding, and transform changes;
- collection limits;
- possible edit kinds;
- candidate-count and output-size bounds.

A semantic diff compares normalized IR, not source formatting. It reports added or removed capabilities, changed limits or thresholds, changed transforms, changed output kinds, and static-cost deltas. It does not claim semantic equivalence for arbitrary JavaScript; both sides must be accepted Language Schema v1 programs.

## 8. Version and Compatibility Model

Validated automation records:

```rust
pub struct ValidatedAutomation {
    pub source: String,
    pub language_schema_version: LanguageSchemaVersion,
    pub ir_schema_version: IrSchemaVersion,
    pub capability_api_version: CapabilityApiVersion,
    pub output_schema_version: OutputSchemaVersion,
    pub workflow_ir: WorkflowIr,
    pub validation_summary: ValidationSummary,
}
```

All four versions are explicit strongly typed newtypes, not inferred from crate versions.

Execution requires exact installed compatibility for v1. An unsupported language, IR, capability, or output schema version returns a typed `CompatibilityError` before a runtime is created.

Ordinary UI use reads persisted Workflow IR without reparsing. Revalidation or migration is an explicit operation owned by a later persistence/upgrade design; this subproject only defines the compatibility checks and rejects unsupported artifacts.

## 9. Execution Interfaces

### 9.1 Executor trait

```rust
pub trait AutomationExecutor {
    fn execute(
        &self,
        automation: &ValidatedAutomation,
        input: &AutomationInput,
        proposal: &ProposalContext,
        host: &mut dyn AutomationHost,
        policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
    ) -> Result<AutomationExecution, ExecutionError>;
}
```

The interface is synchronous because QuickJS and the capability API are synchronous in v1. Higher layers may run it on a dedicated worker and enforce an outer timeout. No executor implementation may mutate UI or `ImageDocument`.

`ProposalContext` supplies the caller-allocated `ProposalId`, run provenance,
and base document state needed to construct the resulting `EditProposal`. These
values are not controllable by JavaScript.

### 9.2 Input

`AutomationInput` contains read-only, bounded visual context descriptors required by validated source:

- image dimensions;
- selected region when present;
- selected annotations and their geometry/type metadata;
- additional capability-specific non-sensitive handles or descriptors.

Annotation IDs cross the JavaScript boundary as canonical decimal strings, not JavaScript numbers, because `AnnotationId` is a Rust `u64` and JavaScript numbers cannot represent every `u64` exactly.

The rquickjs adapter recursively freezes the input graph. Rust remains the authority: JavaScript-side freezing is defense in depth and never replaces input validation.

### 9.3 Execution policy

```rust
pub struct ExecutionPolicy {
    pub max_wall_time: Duration,
    pub max_memory_bytes: usize,
    pub max_stack_bytes: usize,
    pub max_capability_calls: u32,
    pub max_calls_by_capability: CapabilityCallLimits,
    pub max_host_allocation_bytes: usize,
    pub max_output_bytes: usize,
    pub max_candidates: u32,
    pub max_total_redaction_area_fraction: f32,
    pub allowed_edit_kinds: BTreeSet<ProposedEditKind>,
    pub allowed_annotation_ids: BTreeSet<AnnotationId>,
}
```

The automation language supports the complete CRUD surface, but each run explicitly authorizes edit kinds. A Smart Redaction preset normally authorizes only `AddRedaction`; another product flow may authorize more kinds after explicit review-policy configuration.

Update and delete operations may reference only IDs present in both `input.annotations` and `allowed_annotation_ids`.

## 10. rquickjs Sandbox

### 10.1 Context construction

Every execution creates a new runtime and context. Production code:

- uses `Context::base()` or an equivalently minimal custom context;
- never uses `Context::full()`;
- never calls `Runtime::set_loader()`;
- installs memory and stack limits before evaluating source;
- installs an interrupt handler tied to wall-clock deadline and cancellation;
- strips ambient globals immediately after context creation.

The following globals are set to `Undefined` and read back to assert they are absent:

- `eval`
- `Function`
- `queueMicrotask`
- `globalThis`
- `Reflect`

Failure to establish or verify lockdown aborts execution with `SandboxInitializationFailed`.

### 10.2 Host API installation

Rust installs only:

- recursively frozen `input`;
- a recursively frozen `rollshot` object with the four direct methods;
- the validated automation script;
- the direct invocation of `main(input)`.

The executor confirms that `main` exists and is callable after evaluation. Runtime lookup does not weaken frontend validation: source that did not pass Language Schema v1 is never evaluated.

Capability callbacks:

- validate and decode query objects strictly;
- charge the call against global and per-capability budgets before dispatch;
- call the supplied `AutomationHost`;
- validate, truncate, and freeze returned data;
- map host errors to typed execution failures;
- never expose Rust objects or arbitrary native bindings.

### 10.3 Resource limits and cancellation

QuickJS limits cover JavaScript memory, stack, and opcode-boundary interruption. Rust-side host allocations are separately metered by host contracts and `ExecutionPolicy`.

The interrupt handler checks:

- cancellation;
- wall-clock deadline;
- executor-owned work counters.

A blocking Rust host callback cannot be pre-empted by QuickJS. Therefore production host callbacks must remain below 1 ms and must not perform detector work directly. Longer detector work belongs outside the runtime and must be supplied as bounded prepared data or through a future async architecture change.

Each failure reports typed resource usage without source, OCR text, query contents, or candidate contents.

## 11. Output Schema v1

### 11.1 Envelope and metadata

`main` returns exactly:

```javascript
{
  candidates: [/* candidate objects */]
}
```

Unknown envelope fields are rejected.

Every candidate contains:

- `kind`: required tagged-union discriminator;
- `confidence`: required finite number in `[0, 1]`;
- `label`: required non-empty bounded string;
- `rationale`: optional bounded string;
- exactly the operation-specific fields below.

Unknown, missing, or operation-inapplicable fields are rejected. The frontend
rejects duplicate keys in object literals before execution; JavaScript runtime
objects cannot preserve duplicate-key evidence for the output decoder.

### 11.2 Complete CRUD union

```javascript
{ kind: "addRedaction", bounds, confidence, label, rationale? }
{ kind: "addTextNote", position, text, confidence, label, rationale? }
{ kind: "addNumberCallout", tip, bubble, confidence, label, rationale? }
{
  kind: "updateRedactionBounds",
  annotationId: "42",
  bounds,
  confidence,
  label,
  rationale?,
}
{
  kind: "updateTextPosition",
  annotationId: "42",
  position,
  confidence,
  label,
  rationale?,
}
{
  kind: "updateText",
  annotationId: "42",
  text,
  confidence,
  label,
  rationale?,
}
{
  kind: "updateNumberPoints",
  annotationId: "42",
  tip,
  bubble,
  confidence,
  label,
  rationale?,
}
{ kind: "delete", annotationId: "42", confidence, label, rationale? }
```

Geometry uses full-resolution image coordinates:

```javascript
const point = { x, y };
const bounds = { x, y, width, height };
```

Annotation IDs are canonical unsigned decimal strings with no sign, whitespace, or leading zeroes except `"0"`.

### 11.3 Rust validation and proposal construction

The strict decoder rejects:

- malformed envelopes or union variants;
- unknown or extra fields;
- invalid strings or annotation IDs;
- non-finite coordinates or confidence;
- zero-area rectangles;
- output count or byte amplification;
- edit kinds not authorized by `ExecutionPolicy`;
- annotation IDs not authorized by `ExecutionPolicy`;
- host/output schema version mismatch.

Decoded candidates become `ProposedCandidate` values:

- candidate IDs are allocated deterministically by Rust in output order;
- operation fields map to the matching `ProposedEdit`;
- `rollshot-edit-proposal::ProposedCandidate` gains a required `label: String`
  field in this subproject;
- label, confidence, rationale, and provenance populate proposal metadata;
- proposal provenance identifies the automation run supplied by the caller;
- proposal ID and `base_document_state_id` are supplied by `ProposalContext`.

After decoding, existing `rollshot-edit-proposal::validate_policy` enforces candidate count, total redaction area, and out-of-bounds policy. Document-level validation still occurs later, atomically, when reviewed operations are applied through `ImageDocument::apply_batch`.

No successful execution changes the document or claims that the image is safe.

## 12. Errors and Privacy-Safe Diagnostics

Top-level failures remain distinguishable:

```rust
pub enum AutomationError {
    Compatibility(CompatibilityError),
    Parse(Vec<SourceDiagnostic>),
    Subset(Vec<SourceDiagnostic>),
    StaticCost(StaticCostError),
    Sandbox(SandboxError),
    Capability(CapabilityError),
    Output(OutputError),
    Policy(PolicyError),
    Cancelled,
}
```

The implementation may use more focused internal enums, but callers must retain these product-relevant categories.

Tracing requirements:

- stable explicit targets under `rollshot::automation::*`;
- structured fields for schema versions, capability kind, counts, duration, resource usage, and stable error code;
- no automation source, OCR text, template contents, query payloads, candidate contents, annotation text, or raw QuickJS exceptions in ordinary diagnostics;
- per-call or high-volume events use `trace`;
- retained diagnostics must be privacy-safe.

## 13. Data Flow

```text
JavaScript source
    |
    v
oxc parse
    |
    v
Language Schema v1 validation
  - names and scopes
  - helper purity
  - call graph
  - boundedness
    |
    v
Workflow IR + manifest + static cost
    |
    v
ValidatedAutomation
    |
    +--> semantic summary/diff for later review UI
    |
    v
AutomationExecutor + AutomationHost + ExecutionPolicy
    |
    v
fresh hardened QuickJS runtime/context
    |
    v
strict Output Schema v1 decoding
    |
    v
execution policy + proposal policy validation
    |
    v
EditProposal
```

## 14. Verification

### 14.1 Frontend unit tests

- valid `main(input)` with and without helpers;
- missing, duplicate, malformed, async, or generator `main`;
- extra top-level declarations or statements;
- allowed literals, expressions, conditions, and transformations;
- every rejected syntax class from §5.4;
- lexical name resolution and shadowing;
- helper access to `input`, `rollshot`, or outer bindings;
- direct and indirect recursion;
- callback capture and escape behavior;
- exact byte and line/column source spans;
- stable diagnostic codes;
- deterministic IR normalization and IDs;
- schema/version serde round trips;
- capability manifests for all four capabilities;
- all static-cost upper bounds and rejection thresholds;
- semantic summaries and semantic diffs;
- unknown and incompatible versions.

### 14.2 Output and proposal tests

- strict envelope decoding;
- every full CRUD variant;
- unknown, extra, and missing fields, plus frontend duplicate-key rejection;
- decimal-string `AnnotationId` edge cases through `u64::MAX`;
- non-finite geometry/confidence and zero-area rectangles;
- string and output-byte limits;
- unauthorized edit kinds;
- unauthorized annotation IDs;
- candidate ordering and deterministic ID allocation;
- conversion to matching `ProposedEdit`;
- label, confidence, and rationale propagation;
- integration with `validate_policy`.

### 14.3 Executor contract tests

Use a fake executor in `rollshot-automation` tests to establish implementation-independent behavior:

- accepted validated artifact executes;
- raw or incompatible source cannot execute;
- cancellation and typed errors are preserved;
- host calls and output are metered;
- no document mutation occurs.

### 14.4 rquickjs adversarial tests

Carry forward and productionize the spike gates:

- all five ambient globals are absent and verified;
- dynamic import cannot resolve;
- no loader is installed;
- prototype mutation does not escape a fresh context;
- `eval`, `Function`, reflection, computed access, and hidden capability invocation fail;
- infinite loops are interrupted;
- excessive allocation hits the memory limit;
- deep recursion hits the stack limit;
- cancellation interrupts JavaScript work;
- callback failures remain typed;
- host-side allocations are separately bounded;
- output amplification is rejected;
- a fresh execution cannot observe prior execution state.

### 14.5 End-to-end fixtures

Deterministic fake hosts cover:

- OCR → filter → map → redaction proposal;
- layout plus region-feature composition;
- template matching with a pure geometry helper;
- no-match execution;
- all CRUD output kinds under an allow-all policy;
- Smart Redaction policy rejecting non-redaction edits;
- capability failure;
- malformed output;
- static budget rejection before runtime creation.

### 14.6 Required commands

At minimum:

```bash
rtk cargo test -p rollshot-automation
rtk cargo test -p rollshot-automation-rquickjs
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test
```

Because both selected dependencies are pure Rust and the spike already established macOS compilation, this subproject requires normal workspace Linux CI plus macOS compile verification for both new crates. No real-display UI verification is required because this phase contains no UI.

## 15. Completion Handoff

Implementation is not complete until it adds:

`docs/superpowers/handoffs/YYYY-MM-DD-automation-frontend-runtime.md`

The handoff records:

- delivered crates and exact dependency pins;
- public APIs and examples;
- final language, IR, capability, and output schema version values;
- validation and adversarial-test evidence;
- known limitations and remaining sandbox risks;
- how subproject 4 invokes validation, dry-run, and execution from agent tools;
- how subproject 5 persists source, Workflow IR, schema versions, and validation summaries;
- how subproject 6 renders diagnostics, semantic diff, capability changes, and runtime metrics;
- how deterministic fake hosts are replaced by real capability adapters;
- explicit ownership of real OCR, layout, region-feature, and template-matching adapters;
- the deferred `smart-redaction` app feature contract;
- migration considerations for future parser/runtime upgrades.

The same completion change updates the parent design's Delivery Decomposition §12 status without rewriting its historical design decisions:

- mark subproject 3 as implemented and link the handoff;
- identify subproject 4, Bounded Agent Core, as the next phase.

The handoff and parent-phase update are committed with the final implementation change so downstream work cannot miss them.

## 16. Decisions

1. Use oxc `=0.137.0`; do not provide alternate parser features.
2. Use rquickjs `=0.12.0` behind a replaceable executor trait.
3. Split frontend/contracts and rquickjs implementation into two crates.
4. Require an explicit `function main(input)` and permit restricted top-level pure helpers.
5. Permit only statically bounded programs and collection operations.
6. Define all four parent-design capabilities in API v1, backed by fake hosts in this phase.
7. Support the complete `ProposedEdit` CRUD union, constrained by per-run operation policy.
8. Represent annotation IDs as decimal strings at the JavaScript boundary.
9. Add the candidate label required by output and visual-review contracts to
   `rollshot-edit-proposal`.
10. Execute canonical source; do not execute Workflow IR.
11. Defer the default-enabled `smart-redaction` app feature until Workbench integration.
12. Require a downstream handoff and parent-phase status update at implementation completion.

## 17. Success Criteria

This subproject is complete when:

1. A valid Language Schema v1 program parses with source-native diagnostics.
2. Pure helpers support bounded mathematics and data transformation without ambient access, mutation, or recursion.
3. Validation produces deterministic Workflow IR, capability manifests, static costs, semantic summaries, and semantic diffs.
4. An incompatible or statically unbounded artifact cannot reach the runtime.
5. The rquickjs executor establishes and verifies lockdown for every fresh execution.
6. Deterministic fake hosts exercise all four capability contracts.
7. Strict output decoding supports every `ProposedEdit` variant while enforcing per-run authorization.
8. Successful execution returns a validated `EditProposal` without mutating `ImageDocument`.
9. Resource, capability, cancellation, malformed-output, and policy failures retain typed classifications.
10. Parser/runtime pins and upgrade regression requirements are explicit.
11. Workspace tests, formatting, clippy, and macOS compile verification pass.
12. The completion handoff and parent subproject status update are present.
