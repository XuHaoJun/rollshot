# Automation Frontend and Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the versioned restricted-JavaScript frontend and hardened rquickjs executor that turn validated automation source into a policy-checked `EditProposal` without mutating `ImageDocument`.

**Architecture:** Add a framework-neutral `rollshot-automation` crate containing source diagnostics, oxc-backed validation, Workflow IR, capability contracts, static cost, semantic diff, strict output decoding, and executor/host interfaces. Add a separate `rollshot-automation-rquickjs` crate that implements the executor interface with a fresh locked-down QuickJS runtime per run. Keep real detector adapters, app feature wiring, persistence, agent orchestration, and UI outside this phase.

**Tech Stack:** Rust 2021, oxc `=0.137.0`, rquickjs `=0.12.0`, serde/serde_json, thiserror, tracing, existing `rollshot-edit-proposal` and `rollshot-image-document`.

**Reference implementations (verified-working spike code — the API source of truth):**
The inline oxc and rquickjs snippets in this plan are *illustrative*. They sketch
intent and the public `rollshot-automation` contract, but several use shorthand
that may not match the exact oxc `0.137.0` / rquickjs `0.12` APIs (e.g. AST node
shapes, error-inspection helpers, builder methods). Before writing any oxc or
rquickjs code, read the committed spike crates, which compiled and passed all
gates against these exact pinned versions on Linux and macOS (PR #60):

- `spikes/js-frontend/` with `--features oxc` — verified oxc `0.137.0` parsing and
  typed-AST traversal (`Statement::*`, `SourceType::default().with_script(true)`,
  span extraction). Source of truth for Tasks 4–6.
- `spikes/sandbox-executor/` — verified rquickjs `0.12` lockdown, `set_interrupt_handler`,
  `set_memory_limit`, `set_max_stack_size`, host-callback marshalling, and
  `Err → catchable JS exception`. Source of truth for Tasks 10–11.

When a plan snippet and the spike disagree on an API detail, **the spike wins**
(it compiled); when they disagree on a *policy* detail (what to allow/reject),
**this plan wins**. Do **not** port the spike's string-prefix or partial-traversal
shortcuts (those were tree-sitter-candidate scaffolding) — only its verified API usage.

---

## File Structure

### Existing files modified

- `Cargo.toml`
  - Register both new workspace crates.
  - Add exact workspace dependency pins for oxc and rquickjs.
- `Cargo.lock`
  - Resolve the production dependency graph for the two new crates.
- `crates/rollshot-edit-proposal/src/proposal.rs`
  - Add required `label` metadata to `ProposedCandidate`.
  - Update proposal serialization tests.
- `crates/rollshot-edit-proposal/src/policy.rs`
  - Update candidate fixtures for the required label.
- `crates/rollshot-edit-proposal/src/review.rs`
  - Update candidate fixtures for the required label.
- `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`
  - At completion, mark Delivery Decomposition subproject 3 implemented and link the handoff.

### `rollshot-automation` files created

- `crates/rollshot-automation/Cargo.toml`
  - Frontend crate dependencies and crate metadata.
- `crates/rollshot-automation/src/lib.rs`
  - Public modules and stable re-exports only.
- `crates/rollshot-automation/src/version.rs`
  - Strongly typed schema versions and installed v1 constants.
- `crates/rollshot-automation/src/diagnostic.rs`
  - Source spans, stable diagnostic codes, related diagnostics, and source-location conversion.
- `crates/rollshot-automation/src/capability.rs`
  - Capability names, typed queries/results, limits, and manifest records.
- `crates/rollshot-automation/src/input.rs`
  - Read-only bounded automation input and selected-annotation descriptors.
- `crates/rollshot-automation/src/policy.rs`
  - Validation limits, execution limits, allowed edit kinds, and proposal context.
- `crates/rollshot-automation/src/host.rs`
  - `AutomationHost`, `CapabilityError`, and deterministic fake host.
- `crates/rollshot-automation/src/ir.rs`
  - Serializable Workflow IR, node kinds, static cost, summaries, and semantic diff model.
- `crates/rollshot-automation/src/frontend/mod.rs`
  - `validate_source` orchestration and `ValidatedAutomation`.
- `crates/rollshot-automation/src/frontend/parse.rs`
  - oxc parsing and parser-diagnostic conversion.
- `crates/rollshot-automation/src/frontend/validate.rs`
  - Language Schema v1 source shape, lexical scope, purity, recursion, and boundedness checks.
- `crates/rollshot-automation/src/frontend/normalize.rs`
  - Accepted-AST normalization into deterministic Workflow IR.
- `crates/rollshot-automation/src/diff.rs`
  - Semantic summary and IR-to-IR diff generation.
- `crates/rollshot-automation/src/output.rs`
  - Strict Output Schema v1 decoding and conversion to `EditProposal`.
- `crates/rollshot-automation/src/executor.rs`
  - `AutomationExecutor`, cancellation flag, execution metrics/result, and top-level typed errors.
- `crates/rollshot-automation/tests/frontend_contract.rs`
  - Public frontend contract tests.
- `crates/rollshot-automation/tests/output_contract.rs`
  - Strict full-CRUD output tests.
- `crates/rollshot-automation/tests/executor_contract.rs`
  - Executor-implementation-independent tests.
- `crates/rollshot-automation/tests/fixtures/*.js`
  - Accepted and rejected Language Schema v1 fixtures.

### `rollshot-automation-rquickjs` files created

- `crates/rollshot-automation-rquickjs/Cargo.toml`
  - Exact rquickjs dependency and frontend crate dependency.
- `crates/rollshot-automation-rquickjs/src/lib.rs`
  - `QuickJsExecutor` public surface.
- `crates/rollshot-automation-rquickjs/src/lockdown.rs`
  - Restricted context creation, dangerous-global stripping, and verification.
- `crates/rollshot-automation-rquickjs/src/bridge.rs`
  - Frozen input/capability installation and typed host callback marshalling.
- `crates/rollshot-automation-rquickjs/src/execution.rs`
  - Runtime limits, interrupt handler, source evaluation, `main(input)` invocation, metrics, and error mapping.
- `crates/rollshot-automation-rquickjs/tests/lockdown.rs`
  - Ambient capability and fresh-runtime isolation tests.
- `crates/rollshot-automation-rquickjs/tests/resources.rs`
  - Timeout, cancellation, memory, stack, host-allocation, and output-amplification tests.
- `crates/rollshot-automation-rquickjs/tests/end_to_end.rs`
  - Validated source through fake capabilities to `EditProposal`.

### Documentation created

- `docs/superpowers/handoffs/2026-06-21-automation-frontend-runtime.md`
  - Public API, schema versions, verification evidence, downstream integration instructions, and remaining risks.

---

### Task 1: Add candidate labels to the proposal foundation

**Files:**
- Modify: `crates/rollshot-edit-proposal/src/proposal.rs`
- Modify: `crates/rollshot-edit-proposal/src/policy.rs`
- Modify: `crates/rollshot-edit-proposal/src/review.rs`

- [ ] **Step 1: Update the serialization test first**

In `proposal_serde_round_trip`, add a required label and assert it survives serialization:

```rust
let proposal = EditProposal {
    id: ProposalId(1),
    base_document_state_id: 7,
    candidates: vec![ProposedCandidate {
        id: CandidateId(1),
        edit: ProposedEdit::AddRedaction { bounds: r },
        confidence: 0.9,
        label: "email".into(),
        rationale: Some("matches email pattern".into()),
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 42 },
        },
    }],
    confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
    rationale_summary: None,
    provenance: Provenance {
        source: ProvenanceSource::Agent { run_id: 42 },
    },
};
let json = serde_json::to_string(&proposal).unwrap();
let back: EditProposal = serde_json::from_str(&json).unwrap();
assert_eq!(back.candidates[0].label, "email");
```

- [ ] **Step 2: Run the focused test and confirm the compile failure**

Run:

```bash
rtk cargo test -p rollshot-edit-proposal proposal_serde_round_trip
```

Expected: FAIL because `ProposedCandidate` has no `label` field.

- [ ] **Step 3: Add the required field**

Change `ProposedCandidate` to:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedCandidate {
    pub id: CandidateId,
    pub edit: ProposedEdit,
    pub confidence: f32,
    pub label: String,
    pub rationale: Option<String>,
    pub provenance: Provenance,
}
```

Add `label: "test".into(),` to every existing `ProposedCandidate` fixture in:

- `proposal.rs`
- `policy.rs`
- `review.rs`

- [ ] **Step 4: Run the proposal crate tests**

Run:

```bash
rtk cargo test -p rollshot-edit-proposal
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-edit-proposal/src/proposal.rs crates/rollshot-edit-proposal/src/policy.rs crates/rollshot-edit-proposal/src/review.rs
rtk git commit -m "feat(edit-proposal): add candidate labels"
```

---

### Task 2: Scaffold the automation crates and schema-version contracts

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/rollshot-automation/Cargo.toml`
- Create: `crates/rollshot-automation/src/lib.rs`
- Create: `crates/rollshot-automation/src/version.rs`
- Create: `crates/rollshot-automation/src/diagnostic.rs`
- Create: `crates/rollshot-automation-rquickjs/Cargo.toml`
- Create: `crates/rollshot-automation-rquickjs/src/lib.rs`

- [ ] **Step 1: Add a version round-trip integration test**

Create `crates/rollshot-automation/tests/frontend_contract.rs`:

```rust
use rollshot_automation::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
};

#[test]
fn installed_schema_versions_are_explicit_and_round_trip() {
    assert_eq!(LANGUAGE_SCHEMA_V1, LanguageSchemaVersion(1));
    assert_eq!(IR_SCHEMA_V1, IrSchemaVersion(1));
    assert_eq!(CAPABILITY_API_V1, CapabilityApiVersion(1));
    assert_eq!(OUTPUT_SCHEMA_V1, OutputSchemaVersion(1));

    let json = serde_json::to_string(&(
        LANGUAGE_SCHEMA_V1,
        IR_SCHEMA_V1,
        CAPABILITY_API_V1,
        OUTPUT_SCHEMA_V1,
    ))
    .unwrap();
    let decoded: (
        LanguageSchemaVersion,
        IrSchemaVersion,
        CapabilityApiVersion,
        OutputSchemaVersion,
    ) = serde_json::from_str(&json).unwrap();
    assert_eq!(
        decoded,
        (
            LANGUAGE_SCHEMA_V1,
            IR_SCHEMA_V1,
            CAPABILITY_API_V1,
            OUTPUT_SCHEMA_V1,
        )
    );
}
```

- [ ] **Step 2: Register the crates and exact dependency pins**

Add workspace members:

```toml
"crates/rollshot-automation",
"crates/rollshot-automation-rquickjs",
```

Add workspace dependencies:

```toml
oxc_allocator = "=0.137.0"
oxc_ast = "=0.137.0"
oxc_parser = "=0.137.0"
oxc_span = "=0.137.0"
rquickjs = "=0.12.0"
```

Create `crates/rollshot-automation/Cargo.toml`:

```toml
[package]
name = "rollshot-automation"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
oxc_allocator = { workspace = true }
oxc_ast = { workspace = true }
oxc_parser = { workspace = true }
oxc_span = { workspace = true }
rollshot-edit-proposal = { path = "../rollshot-edit-proposal" }
rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }

[lints]
workspace = true
```

Create `crates/rollshot-automation-rquickjs/Cargo.toml`:

```toml
[package]
name = "rollshot-automation-rquickjs"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
rquickjs = { workspace = true }
rollshot-automation = { path = "../rollshot-automation" }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
rollshot-edit-proposal = { path = "../rollshot-edit-proposal" }

[lints]
workspace = true
```

- [ ] **Step 3: Add strong schema-version types**

Create `version.rs`:

```rust
use serde::{Deserialize, Serialize};

macro_rules! version_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub u16);
    };
}

version_type!(LanguageSchemaVersion);
version_type!(IrSchemaVersion);
version_type!(CapabilityApiVersion);
version_type!(OutputSchemaVersion);

pub const LANGUAGE_SCHEMA_V1: LanguageSchemaVersion = LanguageSchemaVersion(1);
pub const IR_SCHEMA_V1: IrSchemaVersion = IrSchemaVersion(1);
pub const CAPABILITY_API_V1: CapabilityApiVersion = CapabilityApiVersion(1);
pub const OUTPUT_SCHEMA_V1: OutputSchemaVersion = OutputSchemaVersion(1);
```

- [ ] **Step 4: Add source diagnostic primitives**

Create `diagnostic.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    ParseError,
    MissingMain,
    DuplicateMain,
    InvalidMainSignature,
    InvalidTopLevel,
    UnsupportedSyntax,
    UnknownIdentifier,
    ForbiddenShadowing,
    HelperImpurity,
    RecursiveHelper,
    EscapingClosure,
    UnboundedCollection,
    DuplicateObjectKey,
    StaticLimitExceeded,
    NormalizationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl SourceSpan {
    pub fn from_offsets(source: &str, start_byte: u32, end_byte: u32) -> Self {
        let locate = |offset: u32| {
            let prefix = &source.as_bytes()[..offset as usize];
            let line = prefix.iter().filter(|&&byte| byte == b'\n').count() as u32 + 1;
            let column = prefix
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map(|index| prefix.len() - index)
                .unwrap_or(prefix.len() + 1) as u32;
            (line, column)
        };
        let (start_line, start_column) = locate(start_byte);
        let (end_line, end_column) = locate(end_byte);
        Self {
            start_byte,
            end_byte,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelatedDiagnostic {
    pub message: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDiagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub primary_span: SourceSpan,
    pub related: Vec<RelatedDiagnostic>,
}
```

- [ ] **Step 5: Add public module skeletons**

Create `lib.rs`:

```rust
mod diagnostic;
mod version;

pub use diagnostic::{
    DiagnosticCode, DiagnosticSeverity, RelatedDiagnostic, SourceDiagnostic, SourceSpan,
};
pub use version::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
};
```

Create the temporary rquickjs `lib.rs`:

```rust
#![doc = "Hardened rquickjs executor for rollshot-automation."]

#[derive(Debug, Default)]
pub struct QuickJsExecutor;
```

- [ ] **Step 6: Run the new contract test**

Run:

```bash
rtk cargo test -p rollshot-automation installed_schema_versions_are_explicit_and_round_trip
rtk cargo check -p rollshot-automation-rquickjs
```

Expected: PASS.

- [ ] **Step 7: Confirm disabled app builds do not gain the new dependencies**

Run:

```bash
rtk cargo tree -p rollshot-app --no-default-features
```

Expected: output contains none of `rollshot-automation`, `oxc_parser`, `rquickjs`, or `rig-core`.

- [ ] **Step 8: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-automation crates/rollshot-automation-rquickjs
rtk git commit -m "build(automation): scaffold frontend and runtime crates"
```

---

### Task 3: Define capability, input, host, and policy contracts

**Files:**
- Create: `crates/rollshot-automation/src/capability.rs`
- Create: `crates/rollshot-automation/src/input.rs`
- Create: `crates/rollshot-automation/src/policy.rs`
- Create: `crates/rollshot-automation/src/host.rs`
- Modify: `crates/rollshot-automation/src/lib.rs`
- Modify: `crates/rollshot-automation/tests/frontend_contract.rs`

- [ ] **Step 1: Write public-contract tests**

Append:

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rollshot_automation::{
    AnnotationDescriptor, AutomationHost, AutomationInput, CapabilityError, ExecutionPolicy,
    FakeAutomationHost, OcrMatch, OcrQuery, ProposedEditKind, Region, ValidationLimits,
};
use rollshot_image_document::{AnnotationId, ImagePoint, ImageRect};

#[test]
fn fake_host_enforces_query_limits() {
    let bounds =
        ImageRect::from_corners(ImagePoint::new(1.0, 2.0), ImagePoint::new(11.0, 12.0));
    let mut host = FakeAutomationHost::default();
    host.ocr_results = vec![
        OcrMatch {
            bounds,
            text: "one".into(),
            confidence: 0.9,
        },
        OcrMatch {
            bounds,
            text: "two".into(),
            confidence: 0.8,
        },
    ];
    let results = host
        .ocr(OcrQuery {
            region: Region::Full,
            limit: 1,
        })
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn smart_redaction_policy_allows_only_add_redaction() {
    let policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_millis(250),
        8 * 1024 * 1024,
        256 * 1024,
    );
    assert_eq!(
        policy.allowed_edit_kinds,
        BTreeSet::from([ProposedEditKind::AddRedaction])
    );
    assert!(policy.allowed_annotation_ids.is_empty());
}

#[test]
fn automation_input_carries_string_safe_annotation_ids() {
    let input = AutomationInput {
        image_width: 100,
        image_height: 80,
        region: Some(Region::Rect {
            bounds: ImageRect::from_corners(
                ImagePoint::new(0.0, 0.0),
                ImagePoint::new(50.0, 40.0),
            ),
        }),
        annotations: vec![AnnotationDescriptor {
            id: AnnotationId(u64::MAX),
            kind: "redaction".into(),
            bounds: Some(ImageRect::from_corners(
                ImagePoint::new(1.0, 1.0),
                ImagePoint::new(2.0, 2.0),
            )),
        }],
        capability_handles: BTreeMap::new(),
    };
    assert_eq!(input.annotations[0].id.0.to_string(), u64::MAX.to_string());
    let value = serde_json::to_value(&input).unwrap();
    assert_eq!(
        value["annotations"][0]["id"],
        serde_json::Value::String(u64::MAX.to_string())
    );
    assert_eq!(value["region"]["kind"], "rect");
}
```

- [ ] **Step 2: Run tests and verify missing API failures**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract
```

Expected: FAIL with unresolved imports for capability, input, host, and policy types.

- [ ] **Step 3: Implement capability contracts**

Create `capability.rs` with:

```rust
use std::collections::BTreeSet;

use rollshot_image_document::{ImagePoint, ImageRect};
use serde::{Deserialize, Serialize};

use crate::{CapabilityApiVersion, SourceSpan, CAPABILITY_API_V1};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityName {
    Ocr,
    Layout,
    RegionFeatures,
    TemplateMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Region {
    Full,
    Rect { bounds: ImageRect },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrQuery {
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrMatch {
    pub bounds: ImageRect,
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutQuery {
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRegion {
    pub bounds: ImageRect,
    pub role: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionFeaturesQuery {
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionFeatures {
    pub bounds: ImageRect,
    pub dominant_rgba: [u8; 4],
    pub edge_density: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMatchQuery {
    pub template_handle: String,
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMatch {
    pub bounds: ImageRect,
    pub score: f32,
    pub anchor: ImagePoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCallManifest {
    pub capability: CapabilityName,
    pub source_span: SourceSpan,
    pub max_calls: u32,
    pub max_results_per_call: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub capability_api_version: CapabilityApiVersion,
    pub calls: Vec<CapabilityCallManifest>,
    pub required_input_fields: BTreeSet<String>,
    pub max_aggregate_results: u32,
}

impl Default for CapabilityManifest {
    fn default() -> Self {
        Self {
            capability_api_version: CAPABILITY_API_V1,
            calls: Vec::new(),
            required_input_fields: BTreeSet::new(),
            max_aggregate_results: 0,
        }
    }
}
```

- [ ] **Step 4: Implement bounded input and policies**

Create `input.rs`:

```rust
use std::collections::BTreeMap;

use rollshot_image_document::{AnnotationId, ImageRect};
use serde::{Deserialize, Serialize};

use crate::Region;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationDescriptor {
    #[serde(with = "annotation_id_string")]
    pub id: AnnotationId,
    pub kind: String,
    pub bounds: Option<ImageRect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationInput {
    pub image_width: u32,
    pub image_height: u32,
    pub region: Option<Region>,
    pub annotations: Vec<AnnotationDescriptor>,
    pub capability_handles: BTreeMap<String, String>,
}

mod annotation_id_string {
    use rollshot_image_document::AnnotationId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(id: &AnnotationId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&id.0.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AnnotationId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(serde::de::Error::custom("invalid canonical annotation id"));
        }
        value
            .parse::<u64>()
            .map(AnnotationId)
            .map_err(serde::de::Error::custom)
    }
}
```

Create `policy.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rollshot_edit_proposal::{PolicyLimits, ProposalId, Provenance};
use rollshot_image_document::AnnotationId;
use serde::{Deserialize, Serialize};

use crate::CapabilityName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposedEditKind {
    AddRedaction,
    AddTextNote,
    AddNumberCallout,
    UpdateRedactionBounds,
    UpdateTextPosition,
    UpdateText,
    UpdateNumberPoints,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationLimits {
    pub max_source_bytes: usize,
    pub max_ast_nodes: u32,
    pub max_literal_bytes: usize,
    pub max_helpers: u32,
    pub max_helper_call_depth: u32,
    pub max_capability_calls: u32,
    pub max_collection_traversals: u32,
    pub max_candidates: u32,
    pub max_output_bytes: usize,
    pub max_input_annotations: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPolicy {
    pub max_wall_time: Duration,
    pub max_memory_bytes: usize,
    pub max_stack_bytes: usize,
    pub max_capability_calls: u32,
    pub max_calls_by_capability: BTreeMap<CapabilityName, u32>,
    pub max_host_allocation_bytes: usize,
    pub max_output_bytes: usize,
    pub proposal_limits: PolicyLimits,
    pub allowed_edit_kinds: BTreeSet<ProposedEditKind>,
    pub allowed_annotation_ids: BTreeSet<AnnotationId>,
}

impl ExecutionPolicy {
    pub fn smart_redaction_default(
        max_wall_time: Duration,
        max_memory_bytes: usize,
        max_stack_bytes: usize,
    ) -> Self {
        Self {
            max_wall_time,
            max_memory_bytes,
            max_stack_bytes,
            max_capability_calls: 16,
            max_calls_by_capability: BTreeMap::new(),
            max_host_allocation_bytes: 4 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            proposal_limits: PolicyLimits {
                max_candidates: 1_000,
                max_total_area_fraction: 1.0,
                allow_out_of_bounds: false,
            },
            allowed_edit_kinds: BTreeSet::from([ProposedEditKind::AddRedaction]),
            allowed_annotation_ids: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalContext {
    pub proposal_id: ProposalId,
    pub base_document_state_id: u64,
    pub provenance: Provenance,
}
```

- [ ] **Step 5: Implement the host trait and fake host**

Create `host.rs`:

```rust
use crate::{
    LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures, RegionFeaturesQuery,
    TemplateMatch, TemplateMatchQuery,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityError {
    #[error("capability input is invalid: {code}")]
    InvalidInput { code: &'static str },
    #[error("capability limit exceeded")]
    LimitExceeded,
    #[error("capability failed: {code}")]
    Failed { code: &'static str },
}

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

#[derive(Debug, Default)]
pub struct FakeAutomationHost {
    pub ocr_results: Vec<OcrMatch>,
    pub layout_results: Vec<LayoutRegion>,
    pub region_feature_results: Vec<RegionFeatures>,
    pub template_results: Vec<TemplateMatch>,
    pub failure: Option<CapabilityError>,
}

impl FakeAutomationHost {
    fn take_bounded<T: Clone>(&self, values: &[T], limit: u32) -> Result<Vec<T>, CapabilityError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(values.iter().take(limit as usize).cloned().collect())
    }
}

impl AutomationHost for FakeAutomationHost {
    fn ocr(&mut self, query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
        self.take_bounded(&self.ocr_results, query.limit)
    }

    fn layout(&mut self, query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError> {
        self.take_bounded(&self.layout_results, query.limit)
    }

    fn region_features(
        &mut self,
        query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        self.take_bounded(&self.region_feature_results, query.limit)
    }

    fn template_match(
        &mut self,
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        self.take_bounded(&self.template_results, query.limit)
    }
}
```

- [ ] **Step 6: Re-export the contract**

Add modules and re-exports to `lib.rs`:

```rust
mod capability;
mod host;
mod input;
mod policy;

pub use capability::*;
pub use host::{AutomationHost, CapabilityError, FakeAutomationHost};
pub use input::{AnnotationDescriptor, AutomationInput};
pub use policy::{ExecutionPolicy, ProposalContext, ProposedEditKind, ValidationLimits};
```

- [ ] **Step 7: Run the contract suite**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-automation
rtk git commit -m "feat(automation): define capability and policy contracts"
```

---

### Task 4: Parse source and enforce the canonical `main(input)` shape

**Files:**
- Create: `crates/rollshot-automation/src/frontend/mod.rs`
- Create: `crates/rollshot-automation/src/frontend/parse.rs`
- Create: `crates/rollshot-automation/src/frontend/validate.rs`
- Modify: `crates/rollshot-automation/src/lib.rs`
- Create: `crates/rollshot-automation/tests/fixtures/valid_main.js`
- Create: `crates/rollshot-automation/tests/fixtures/missing_main.js`
- Create: `crates/rollshot-automation/tests/fixtures/duplicate_main.js`
- Create: `crates/rollshot-automation/tests/fixtures/invalid_main_signature.js`
- Create: `crates/rollshot-automation/tests/fixtures/invalid_top_level.js`
- Modify: `crates/rollshot-automation/tests/frontend_contract.rs`

- [ ] **Step 1: Add canonical source fixtures**

`valid_main.js`:

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
  const matches = rollshot.ocr({ region: input.region, limit: 10 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: expandBounds(match.bounds, 8),
      confidence: match.confidence,
      label: "ocr-match",
    })),
  };
}
```

`missing_main.js`:

```javascript
function helper(value) {
  return value;
}
```

`duplicate_main.js`:

```javascript
function main(input) {
  return { candidates: [] };
}

function main(input) {
  return { candidates: [] };
}
```

`invalid_main_signature.js`:

```javascript
async function main(context, extra) {
  return { candidates: [] };
}
```

`invalid_top_level.js`:

```javascript
const leaked = 1;

function main(input) {
  return { candidates: [] };
}
```

- [ ] **Step 2: Add shape-validation tests**

Append:

```rust
use rollshot_automation::{validate_source, DiagnosticCode};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn assert_has_code(source: &str, expected: DiagnosticCode) {
    let diagnostics = validate_source(source, &ValidationLimits::default()).unwrap_err();
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == expected),
        "missing {expected:?}: {diagnostics:#?}"
    );
}

#[test]
fn accepts_explicit_main_and_pure_helper_shape() {
    let validated =
        validate_source(&fixture("valid_main.js"), &ValidationLimits::default()).unwrap();
    assert_eq!(validated.source, fixture("valid_main.js"));
}

#[test]
fn rejects_missing_duplicate_and_malformed_main() {
    assert_has_code(&fixture("missing_main.js"), DiagnosticCode::MissingMain);
    assert_has_code(&fixture("duplicate_main.js"), DiagnosticCode::DuplicateMain);
    assert_has_code(
        &fixture("invalid_main_signature.js"),
        DiagnosticCode::InvalidMainSignature,
    );
}

#[test]
fn rejects_non_function_top_level_statement() {
    assert_has_code(
        &fixture("invalid_top_level.js"),
        DiagnosticCode::InvalidTopLevel,
    );
}
```

Implement `Default` for `ValidationLimits` with:

```rust
impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 64 * 1024,
            max_ast_nodes: 10_000,
            max_literal_bytes: 32 * 1024,
            max_helpers: 32,
            max_helper_call_depth: 16,
            max_capability_calls: 32,
            max_collection_traversals: 64,
            max_candidates: 1_000,
            max_output_bytes: 1024 * 1024,
            max_input_annotations: 1_000,
        }
    }
}
```

- [ ] **Step 3: Run the tests and verify the missing frontend**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract accepts_explicit_main_and_pure_helper_shape
```

Expected: FAIL because `validate_source` and `ValidatedAutomation` do not exist.

- [ ] **Step 4: Implement parser ownership and parser diagnostics**

In `frontend/parse.rs`, define a callback-based parser owner so the arena-backed AST never escapes:

```rust
use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use crate::{DiagnosticCode, DiagnosticSeverity, SourceDiagnostic, SourceSpan};

pub(super) fn with_program<T>(
    source: &str,
    use_program: impl for<'a> FnOnce(&Program<'a>) -> T,
) -> Result<T, Vec<SourceDiagnostic>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        source,
        SourceType::default().with_script(true),
    )
    .with_options(ParseOptions::default())
    .parse();

    if !parsed.diagnostics.is_empty() {
        return Err(parsed
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                let span = diagnostic.labels.as_slice().first().map_or(
                    SourceSpan::from_offsets(source, 0, source.len() as u32),
                    |label| {
                        let start = label.offset() as u32;
                        SourceSpan::from_offsets(source, start, start + label.len())
                    },
                );
                SourceDiagnostic {
                    code: DiagnosticCode::ParseError,
                    severity: DiagnosticSeverity::Error,
                    message: diagnostic.message.to_string(),
                    primary_span: span,
                    related: Vec::new(),
                }
            })
            .collect());
    }

    Ok(use_program(&parsed.program))
}
```

- [ ] **Step 5: Implement source-shape validation**

In `frontend/validate.rs`, add:

```rust
use oxc_ast::ast::{BindingPattern, FormalParameterKind, Function, Statement};
use oxc_span::GetSpan;

use crate::{
    DiagnosticCode, DiagnosticSeverity, SourceDiagnostic, SourceSpan, ValidationLimits,
};

pub(super) struct ShapeValidation {
    pub diagnostics: Vec<SourceDiagnostic>,
    pub function_names: Vec<String>,
}

pub(super) fn validate_shape(
    source: &str,
    program: &oxc_ast::ast::Program<'_>,
    limits: &ValidationLimits,
) -> ShapeValidation {
    let mut diagnostics = Vec::new();
    let mut function_names = Vec::new();
    let mut main_count = 0_u32;

    if source.len() > limits.max_source_bytes {
        diagnostics.push(error(
            source,
            DiagnosticCode::StaticLimitExceeded,
            0,
            source.len() as u32,
            "source exceeds the configured byte limit",
        ));
    }

    for statement in &program.body {
        let Statement::FunctionDeclaration(function) = statement else {
            let span = statement.span();
            diagnostics.push(error(
                source,
                DiagnosticCode::InvalidTopLevel,
                span.start,
                span.end,
                "top level may contain only named function declarations",
            ));
            continue;
        };

        let Some(id) = &function.id else {
            diagnostics.push(error(
                source,
                DiagnosticCode::InvalidTopLevel,
                function.span.start,
                function.span.end,
                "top-level functions must be named",
            ));
            continue;
        };
        function_names.push(id.name.to_string());

        if id.name == "main" {
            main_count += 1;
            validate_main(source, function, &mut diagnostics);
        } else {
            validate_helper_signature(source, function, &mut diagnostics);
        }
    }

    match main_count {
        0 => diagnostics.push(error(
            source,
            DiagnosticCode::MissingMain,
            0,
            source.len() as u32,
            "define exactly one synchronous function main(input)",
        )),
        1 => {}
        _ => diagnostics.push(error(
            source,
            DiagnosticCode::DuplicateMain,
            0,
            source.len() as u32,
            "define only one function named main",
        )),
    }

    if function_names.len().saturating_sub(1) > limits.max_helpers as usize {
        diagnostics.push(error(
            source,
            DiagnosticCode::StaticLimitExceeded,
            0,
            source.len() as u32,
            "helper count exceeds the configured limit",
        ));
    }

    ShapeValidation {
        diagnostics,
        function_names,
    }
}

fn validate_main(
    source: &str,
    function: &Function<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let parameter_is_input = function.params.kind == FormalParameterKind::FormalParameter
        && function.params.items.len() == 1
        && matches!(
            &function.params.items[0].pattern,
            BindingPattern::BindingIdentifier(identifier) if identifier.name == "input"
        );
    let body_has_final_return = function.body.as_ref().is_some_and(|body| {
        body.statements.len() == 1
            && matches!(body.statements.last(), Some(Statement::ReturnStatement(_)))
            || body.statements.len() > 1
                && matches!(body.statements.last(), Some(Statement::ReturnStatement(_)))
                && body
                    .statements
                    .iter()
                    .filter(|statement| matches!(statement, Statement::ReturnStatement(_)))
                    .count()
                    == 1
    });

    if function.r#async || function.generator || !parameter_is_input || !body_has_final_return {
        diagnostics.push(error(
            source,
            DiagnosticCode::InvalidMainSignature,
            function.span.start,
            function.span.end,
            "main must be synchronous function main(input) with one final top-level return",
        ));
    }
}

fn validate_helper_signature(
    source: &str,
    function: &Function<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if function.r#async || function.generator || function.body.is_none() {
        diagnostics.push(error(
            source,
            DiagnosticCode::UnsupportedSyntax,
            function.span.start,
            function.span.end,
            "helpers must be synchronous non-generator functions with bodies",
        ));
    }
}

fn error(
    source: &str,
    code: DiagnosticCode,
    start: u32,
    end: u32,
    message: &str,
) -> SourceDiagnostic {
    SourceDiagnostic {
        code,
        severity: DiagnosticSeverity::Error,
        message: message.into(),
        primary_span: SourceSpan::from_offsets(source, start, end),
        related: Vec::new(),
    }
}
```

- [ ] **Step 6: Add the frontend orchestration**

Create `frontend/mod.rs`:

```rust
mod parse;
mod validate;

use serde::{Deserialize, Serialize};

use crate::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    SourceDiagnostic, ValidationLimits, CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1,
    OUTPUT_SCHEMA_V1,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatedAutomation {
    pub source: String,
    pub language_schema_version: LanguageSchemaVersion,
    pub ir_schema_version: IrSchemaVersion,
    pub capability_api_version: CapabilityApiVersion,
    pub output_schema_version: OutputSchemaVersion,
}

pub fn validate_source(
    source: &str,
    limits: &ValidationLimits,
) -> Result<ValidatedAutomation, Vec<SourceDiagnostic>> {
    parse::with_program(source, |program| {
        let result = validate::validate_shape(source, program, limits);
        if result.diagnostics.is_empty() {
            Ok(ValidatedAutomation {
                source: source.into(),
                language_schema_version: LANGUAGE_SCHEMA_V1,
                ir_schema_version: IR_SCHEMA_V1,
                capability_api_version: CAPABILITY_API_V1,
                output_schema_version: OUTPUT_SCHEMA_V1,
            })
        } else {
            Err(result.diagnostics)
        }
    })?
}
```

Export:

```rust
mod frontend;
pub use frontend::{validate_source, ValidatedAutomation};
```

- [ ] **Step 7: Run shape and span tests**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract
```

Expected: PASS for the new shape cases and existing contract tests.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-automation
rtk git commit -m "feat(automation): validate canonical source shape"
```

---

### Task 5: Enforce Language Schema v1 purity and denylist

**Files:**
- Modify: `crates/rollshot-automation/src/frontend/validate.rs`
- Modify: `crates/rollshot-automation/src/frontend/mod.rs`
- Create: `crates/rollshot-automation/tests/fixtures/reject_mutation.js`
- Create: `crates/rollshot-automation/tests/fixtures/reject_loop.js`
- Create: `crates/rollshot-automation/tests/fixtures/reject_dynamic_access.js`
- Create: `crates/rollshot-automation/tests/fixtures/reject_ambient.js`
- Create: `crates/rollshot-automation/tests/fixtures/reject_helper_capability.js`
- Create: `crates/rollshot-automation/tests/fixtures/reject_indirect_recursion.js`
- Create: `crates/rollshot-automation/tests/fixtures/reject_escaping_closure.js`
- Create: `crates/rollshot-automation/tests/fixtures/reject_duplicate_key.js`
- Modify: `crates/rollshot-automation/tests/frontend_contract.rs`

- [ ] **Step 1: Add one fixture for each validator responsibility**

`reject_mutation.js`:

```javascript
function main(input) {
  let candidates = [];
  candidates.push(input.region);
  return { candidates };
}
```

`reject_loop.js`:

```javascript
function main(input) {
  while (true) {
  }
  return { candidates: [] };
}
```

`reject_dynamic_access.js`:

```javascript
function main(input) {
  const method = "ocr";
  const matches = rollshot[method]({ region: input.region, limit: 10 });
  return { candidates: matches };
}
```

`reject_ambient.js`:

```javascript
function main(input) {
  const value = Reflect.get(input, "region");
  return { candidates: value };
}
```

`reject_helper_capability.js`:

```javascript
function inspect(region) {
  return rollshot.ocr({ region, limit: 10 });
}

function main(input) {
  return { candidates: inspect(input.region) };
}
```

`reject_indirect_recursion.js`:

```javascript
function first(value) {
  return second(value);
}

function second(value) {
  return first(value);
}

function main(input) {
  return { candidates: first(input.region) };
}
```

`reject_escaping_closure.js`:

```javascript
function main(input) {
  const callback = (value) => value;
  return { candidates: callback };
}
```

`reject_duplicate_key.js`:

```javascript
function main(input) {
  return { candidates: [], candidates: [] };
}
```

- [ ] **Step 2: Add denylist and purity tests**

Append:

```rust
#[test]
fn rejects_mutation_loops_dynamic_access_and_ambient_globals() {
    assert_has_code(&fixture("reject_mutation.js"), DiagnosticCode::UnsupportedSyntax);
    assert_has_code(&fixture("reject_loop.js"), DiagnosticCode::UnsupportedSyntax);
    assert_has_code(
        &fixture("reject_dynamic_access.js"),
        DiagnosticCode::UnsupportedSyntax,
    );
    assert_has_code(
        &fixture("reject_ambient.js"),
        DiagnosticCode::UnknownIdentifier,
    );
}

#[test]
fn rejects_impure_recursive_and_escaping_helpers() {
    assert_has_code(
        &fixture("reject_helper_capability.js"),
        DiagnosticCode::HelperImpurity,
    );
    assert_has_code(
        &fixture("reject_indirect_recursion.js"),
        DiagnosticCode::RecursiveHelper,
    );
    assert_has_code(
        &fixture("reject_escaping_closure.js"),
        DiagnosticCode::EscapingClosure,
    );
}

#[test]
fn rejects_duplicate_object_keys_before_runtime() {
    assert_has_code(
        &fixture("reject_duplicate_key.js"),
        DiagnosticCode::DuplicateObjectKey,
    );
}
```

The validator built in Step 4 is the security boundary. It MUST be fully
test-driven *here*, not retrofitted later. Add the authoritative adversarial
denylist table now, as a RED test that fails before Step 4 and must stay green
afterward (Task 12 only re-audits it, it does not introduce it):

```rust
#[test]
fn language_schema_v1_denylist_is_complete() {
    let cases = [
        ("function main(input){ return eval('1'); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ return Function('return 1')(); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ return Reflect.get(input, 'region'); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ return new Proxy({}, {}); }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return input?.region; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return input['region']; }", DiagnosticCode::UnsupportedSyntax),
        ("import value from 'x'; function main(input){ return value; }", DiagnosticCode::InvalidTopLevel),
        ("export function main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidTopLevel),
        ("function main(input){ return import('x'); }", DiagnosticCode::UnsupportedSyntax),
        ("async function main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidMainSignature),
        ("function main(input){ return Promise.resolve([]); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ setTimeout(() => {}, 1); return { candidates: [] }; }", DiagnosticCode::UnknownIdentifier),
        ("function* main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidMainSignature),
        ("class X {} function main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidTopLevel),
        ("function main(input){ return new Array(); }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ try { return { candidates: [] }; } catch (error) { return { candidates: [] }; } }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ for (;;) {} return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ do {} while (true); return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ for (const value of []) {} return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ const { region } = input; return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { ...input, candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function helper(...values){ return values; } function main(input){ return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function helper(value = 1){ return value; } function main(input){ return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: [].reduce((a,b) => a, []) }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: [].flatMap((x) => x) }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: [].sort() }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: helper.call(null, input) }; } function helper(x){ return x; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return unknownGlobal; }", DiagnosticCode::UnknownIdentifier),
    ];
    for (source, code) in cases {
        assert_has_code(source, code);
    }
}
```

Each `DiagnosticCode` paired above is the *contract*: the implementation in
Step 4 must make each case fail with that exact code, not merely with some
error. If a case is genuinely ambiguous (more than one code is defensible),
fix the expectation here and record the rationale in the commit — do not loosen
`assert_has_code` to accept any code.

- [ ] **Step 3: Run the new tests and confirm they fail**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract rejects_
```

Expected: FAIL because shape validation does not yet traverse function bodies.

- [ ] **Step 4: Implement a lexical validator with explicit contexts**

Add to `validate.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FunctionKind {
    Main,
    Helper,
}

struct FunctionFacts {
    name: String,
    calls: Vec<(String, SourceSpan)>,
}

struct BodyValidator<'a> {
    source: &'a str,
    helper_names: std::collections::BTreeSet<String>,
    diagnostics: Vec<SourceDiagnostic>,
    facts: Vec<FunctionFacts>,
    ast_nodes: u32,
    literal_bytes: usize,
}
```

Implement exhaustive statement and expression visitors over the pinned oxc AST.

**This is the security boundary — it MUST be allowlist (default-deny), not
denylist.** Every statement-kind and expression-kind match must end in a
catch-all arm that emits `DiagnosticCode::UnsupportedSyntax` (or the more
specific code where one applies) and NEVER a silent `_ => {}` that lets an
unrecognized node through. A new oxc node kind, or one a developer forgot, must
fail closed (rejected), never fail open (allowed). The acceptance criterion for
this step is the `language_schema_v1_denylist_is_complete` table added in Step 2:
every case must reject with its exact paired `DiagnosticCode`. The bullets below
enumerate what is *allowed* and the *specific* codes for common rejections;
they are not the full set of things to reject — the catch-all covers the rest.

The match arms must:

- allow `const` declarations only;
- reject `let`, `var`, assignment, update expressions, all loop statements, classes, `new`, `this`, `super`, `throw`, `try`, async, generators, spread, destructuring, optional/computed access, imports, exports, and unsupported statements;
- resolve identifiers against parameters, local `const` bindings, helper names, `input`/`rollshot` in `main`, and `Math`;
- reject `input` and `rollshot` in helpers;
- permit direct helper calls and direct `rollshot.ocr`, `rollshot.layout`, `rollshot.regionFeatures`, and `rollshot.templateMatch` calls from `main`;
- permit only `Math.abs`, `ceil`, `floor`, `round`, `trunc`, `min`, `max`, `sqrt`, and `hypot`;
- permit immediate arrow callbacks only as arguments to `.map`, `.filter`, `.some`, or `.every`;
- reject an arrow function assigned to a variable, returned, or passed to another call;
- reject duplicate object-literal keys;
- count every visited AST node and literal byte.

Use the following helper-call cycle detector after visiting every function:

```rust
fn detect_cycles(
    source: &str,
    facts: &[FunctionFacts],
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let graph: std::collections::BTreeMap<_, _> = facts
        .iter()
        .map(|fact| {
            (
                fact.name.as_str(),
                fact.calls
                    .iter()
                    .map(|(name, span)| (name.as_str(), *span))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();

    fn visit<'a>(
        node: &'a str,
        graph: &std::collections::BTreeMap<&'a str, Vec<(&'a str, SourceSpan)>>,
        visiting: &mut Vec<&'a str>,
        visited: &mut std::collections::BTreeSet<&'a str>,
        source: &str,
        diagnostics: &mut Vec<SourceDiagnostic>,
    ) {
        if visited.contains(node) {
            return;
        }
        visiting.push(node);
        if let Some(edges) = graph.get(node) {
            for (next, span) in edges {
                if visiting.contains(next) {
                    diagnostics.push(SourceDiagnostic {
                        code: DiagnosticCode::RecursiveHelper,
                        severity: DiagnosticSeverity::Error,
                        message: format!("helper recursion cycle reaches {next}"),
                        primary_span: *span,
                        related: Vec::new(),
                    });
                } else {
                    visit(next, graph, visiting, visited, source, diagnostics);
                }
            }
        }
        visiting.pop();
        visited.insert(node);
        let _ = source;
    }

    let mut visited = std::collections::BTreeSet::new();
    for node in graph.keys().copied() {
        visit(
            node,
            &graph,
            &mut Vec::new(),
            &mut visited,
            source,
            diagnostics,
        );
    }
}
```

The visitor implementation must use typed oxc enum variants. Do not port the spike's string-prefix or partial traversal shortcuts.

- [ ] **Step 5: Add AST and literal limit failures**

After body traversal:

```rust
if validator.ast_nodes > limits.max_ast_nodes {
    validator.diagnostics.push(error(
        source,
        DiagnosticCode::StaticLimitExceeded,
        0,
        source.len() as u32,
        "AST node count exceeds the configured limit",
    ));
}
if validator.literal_bytes > limits.max_literal_bytes {
    validator.diagnostics.push(error(
        source,
        DiagnosticCode::StaticLimitExceeded,
        0,
        source.len() as u32,
        "literal bytes exceed the configured limit",
    ));
}
detect_cycles(source, &validator.facts, &mut validator.diagnostics);
```

Merge body diagnostics with shape diagnostics in `validate_source`.

- [ ] **Step 6: Run the frontend contract suite**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract
```

Expected: PASS.

- [ ] **Step 7: Run clippy on the frontend crate**

Run:

```bash
rtk cargo clippy -p rollshot-automation --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-automation
rtk git commit -m "feat(automation): enforce restricted JavaScript"
```

---

### Task 6: Normalize accepted source into deterministic Workflow IR

**Files:**
- Create: `crates/rollshot-automation/src/ir.rs`
- Create: `crates/rollshot-automation/src/frontend/normalize.rs`
- Modify: `crates/rollshot-automation/src/frontend/mod.rs`
- Modify: `crates/rollshot-automation/src/frontend/validate.rs`
- Modify: `crates/rollshot-automation/src/lib.rs`
- Modify: `crates/rollshot-automation/tests/frontend_contract.rs`

- [ ] **Step 1: Add IR, manifest, and static-cost assertions**

Append:

```rust
use rollshot_automation::{CapabilityName, IrNodeKind};

#[test]
fn valid_source_normalizes_to_deterministic_ir_and_costs() {
    let source = fixture("valid_main.js");
    let first = validate_source(&source, &ValidationLimits::default()).unwrap();
    let second = validate_source(&source, &ValidationLimits::default()).unwrap();
    assert_eq!(first.workflow_ir, second.workflow_ir);
    assert!(first
        .workflow_ir
        .nodes
        .iter()
        .any(|node| matches!(node.kind, IrNodeKind::CapabilityCall(_))));
    assert!(first
        .workflow_ir
        .capability_manifest
        .calls
        .iter()
        .any(|call| call.capability == CapabilityName::Ocr));
    assert_eq!(first.workflow_ir.static_cost.max_output_candidates, 10);
}

#[test]
fn rejects_collection_without_provable_bound() {
    let source = r#"
function main(input) {
  return {
    candidates: input.unknown.map((value) => value),
  };
}
"#;
    assert_has_code(source, DiagnosticCode::UnboundedCollection);
}
```

- [ ] **Step 2: Run and verify missing IR fields**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract valid_source_normalizes_to_deterministic_ir_and_costs
```

Expected: FAIL because `ValidatedAutomation` has no `workflow_ir`.

- [ ] **Step 3: Define the persisted IR**

Create `ir.rs`:

```rust
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    CapabilityManifest, CapabilityName, IrSchemaVersion, ProposedEditKind, SourceSpan,
    IR_SCHEMA_V1,
};

pub type NodeId = u32;
pub type FunctionId = u32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrFunction {
    pub id: FunctionId,
    pub name: String,
    pub source_span: SourceSpan,
    pub max_call_depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrNode {
    pub id: NodeId,
    pub kind: IrNodeKind,
    pub source_span: SourceSpan,
    pub max_cardinality: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCallIr {
    pub capability: CapabilityName,
    pub literal_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperCallIr {
    pub helper: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionIr {
    pub input: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionIr {
    pub expression_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformIr {
    pub expression_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmitCandidatesIr {
    pub input: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticCost {
    pub ast_nodes: u32,
    pub literal_bytes: usize,
    pub helper_count: u32,
    pub max_helper_call_depth: u32,
    pub max_capability_calls: u32,
    pub max_aggregate_capability_results: u32,
    pub max_collection_traversals: u32,
    pub max_output_candidates: u32,
    pub max_output_bytes: usize,
}

impl WorkflowIr {
    pub fn empty() -> Self {
        Self {
            ir_schema_version: IR_SCHEMA_V1,
            entry: 0,
            helpers: Vec::new(),
            nodes: Vec::new(),
            output: 0,
            capability_manifest: CapabilityManifest::default(),
            static_cost: StaticCost {
                ast_nodes: 0,
                literal_bytes: 0,
                helper_count: 0,
                max_helper_call_depth: 0,
                max_capability_calls: 0,
                max_aggregate_capability_results: 0,
                max_collection_traversals: 0,
                max_output_candidates: 0,
                max_output_bytes: 0,
            },
            possible_edit_kinds: BTreeSet::new(),
        }
    }
}
```

- [ ] **Step 4: Return validated semantic facts from the body validator**

Extend the validator result with:

```rust
pub(super) struct ValidationFacts {
    pub ast_nodes: u32,
    pub literal_bytes: usize,
    pub functions: Vec<FunctionFacts>,
}
```

Make `validate_program` return `(Vec<SourceDiagnostic>, ValidationFacts)` so normalization consumes already-validated facts and never repeats policy decisions.

- [ ] **Step 5: Implement deterministic normalization**

Create `frontend/normalize.rs`. Walk the accepted AST in source order and:

- assign function IDs in declaration order, with `main` as `entry`;
- assign node IDs monotonically in expression evaluation order;
- add `CapabilityCall` nodes for the four direct `rollshot.*` calls;
- require each capability query's `limit` field to be a positive integer literal;
- add collection nodes for `map`, `filter`, `some`, and `every`;
- add helper-call, condition, transform, and final emit nodes;
- propagate cardinality from capability limits and bounded input arrays;
- infer possible edit kinds from literal `kind` values in candidate objects;
- calculate call depth from the acyclic helper graph;
- calculate manifest and static costs;
- reject any unknown cardinality with `DiagnosticCode::UnboundedCollection`;
- compare every calculated limit with `ValidationLimits`.

Use:

```rust
pub(super) fn normalize(
    source: &str,
    program: &oxc_ast::ast::Program<'_>,
    facts: &ValidationFacts,
    limits: &ValidationLimits,
) -> Result<WorkflowIr, Vec<SourceDiagnostic>>
```

Node and function IDs must be derived only from validated source order; never use hash-map iteration order.

- [ ] **Step 6: Persist Workflow IR in the validated artifact**

Change `ValidatedAutomation`:

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

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub source_bytes: usize,
    pub ast_nodes: u32,
    pub helper_count: u32,
    pub capability_calls: u32,
    pub max_output_candidates: u32,
}
```

Update `validate_source` to parse once, run validation, normalize only when diagnostics are empty, and return normalization diagnostics otherwise.

**Lifetime constraint (do not fight the borrow checker here):** the oxc AST is
allocated in the arena owned by `parse::with_program` and may not escape its
closure. Both `validate::validate_program` and `normalize::normalize` borrow
`&Program<'a>`, so **both must run inside the single `with_program` callback** —
you cannot return `ValidationFacts` or `&Program` out and normalize afterward.
The callback owns the full decision: validate → if empty, normalize → build
`ValidatedAutomation`; on diagnostics from either stage, return them. Only owned,
`'static` values (`ValidatedAutomation`, `Vec<SourceDiagnostic>`) cross the
closure boundary. Sketch:

```rust
parse::with_program(source, |program| {
    let (mut diagnostics, facts) = validate::validate_program(source, program, limits);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    match normalize::normalize(source, program, &facts, limits) {
        Ok(workflow_ir) => Ok(ValidatedAutomation { /* source, versions, workflow_ir, summary */ }),
        Err(mut normalization_diagnostics) => {
            diagnostics.append(&mut normalization_diagnostics);
            Err(diagnostics)
        }
    }
})?
```

- [ ] **Step 7: Run frontend and serialization tests**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-automation
rtk git commit -m "feat(automation): normalize workflow IR"
```

---

### Task 7: Add semantic summaries, diffs, and compatibility checks

**Files:**
- Create: `crates/rollshot-automation/src/diff.rs`
- Create: `crates/rollshot-automation/src/executor.rs`
- Modify: `crates/rollshot-automation/src/lib.rs`
- Modify: `crates/rollshot-automation/tests/frontend_contract.rs`

- [ ] **Step 1: Add semantic-diff and compatibility tests**

Append:

```rust
use rollshot_automation::{
    ensure_compatible, semantic_diff, semantic_summary, CompatibilityError, SemanticChange,
};

#[test]
fn semantic_diff_reports_capability_limit_change() {
    let before = validate_source(
        "function main(input) { const x = rollshot.ocr({ region: input.region, limit: 10 }); return { candidates: x }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let after = validate_source(
        "function main(input) { const x = rollshot.ocr({ region: input.region, limit: 20 }); return { candidates: x }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let diff = semantic_diff(&before.workflow_ir, &after.workflow_ir);
    assert!(diff
        .changes
        .iter()
        .any(|change| matches!(change, SemanticChange::CapabilityLimitChanged { before: 10, after: 20, .. })));
    assert!(semantic_summary(&after.workflow_ir)
        .lines
        .iter()
        .any(|line| line.contains("ocr") && line.contains("20")));
}

#[test]
fn semantic_diff_reports_threshold_change() {
    let before = validate_source(
        "function main(input) { const x = rollshot.ocr({ region: input.region, limit: 10 }).filter((m) => m.confidence > 0.8); return { candidates: x }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let after = validate_source(
        "function main(input) { const x = rollshot.ocr({ region: input.region, limit: 10 }).filter((m) => m.confidence > 0.9); return { candidates: x }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    assert!(semantic_diff(&before.workflow_ir, &after.workflow_ir)
        .changes
        .iter()
        .any(|change| matches!(change, SemanticChange::ConditionChanged { .. })));
}

#[test]
fn incompatible_schema_is_rejected_before_execution() {
    let mut automation =
        validate_source(&fixture("valid_main.js"), &ValidationLimits::default()).unwrap();
    automation.language_schema_version = LanguageSchemaVersion(99);
    assert_eq!(
        ensure_compatible(&automation),
        Err(CompatibilityError::Language {
            installed: LANGUAGE_SCHEMA_V1,
            artifact: LanguageSchemaVersion(99),
        })
    );
}
```

- [ ] **Step 2: Run and confirm missing APIs**

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract semantic_
rtk cargo test -p rollshot-automation --test frontend_contract incompatible_
```

Expected: FAIL with unresolved semantic and compatibility APIs.

- [ ] **Step 3: Implement semantic summary and diff**

Create `diff.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::{CapabilityName, IrNodeKind, WorkflowIr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSummary {
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticDiff {
    pub changes: Vec<SemanticChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticChange {
    CapabilityAdded { capability: CapabilityName },
    CapabilityRemoved { capability: CapabilityName },
    CapabilityLimitChanged {
        capability: CapabilityName,
        before: u32,
        after: u32,
    },
    EditKindAdded { kind: crate::ProposedEditKind },
    EditKindRemoved { kind: crate::ProposedEditKind },
    MaxOutputCandidatesChanged { before: u32, after: u32 },
    StaticCostChanged {
        before_steps: u32,
        after_steps: u32,
    },
    ConditionChanged {
        before: String,
        after: String,
    },
    TransformChanged {
        before: String,
        after: String,
    },
}

pub fn semantic_summary(ir: &WorkflowIr) -> SemanticSummary {
    let mut lines = ir
        .capability_manifest
        .calls
        .iter()
        .map(|call| {
            format!(
                "{:?}: at most {} call(s), {} result(s) per call",
                call.capability, call.max_calls, call.max_results_per_call
            )
        })
        .collect::<Vec<_>>();
    lines.push(format!(
        "at most {} output candidate(s)",
        ir.static_cost.max_output_candidates
    ));
    for node in &ir.nodes {
        match &node.kind {
            IrNodeKind::Condition(condition) => {
                lines.push(format!("condition: {}", condition.expression_summary));
            }
            IrNodeKind::Transform(transform) => {
                lines.push(format!("transform: {}", transform.expression_summary));
            }
            _ => {}
        }
    }
    lines.push(format!("possible edit kinds: {:?}", ir.possible_edit_kinds));
    SemanticSummary { lines }
}

pub fn semantic_diff(before: &WorkflowIr, after: &WorkflowIr) -> SemanticDiff {
    let limits = |ir: &WorkflowIr| {
        ir.nodes
            .iter()
            .filter_map(|node| match &node.kind {
                IrNodeKind::CapabilityCall(call) => Some((call.capability, call.literal_limit)),
                _ => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before_limits = limits(before);
    let after_limits = limits(after);
    let mut changes = Vec::new();

    for (capability, limit) in &before_limits {
        match after_limits.get(capability) {
            None => changes.push(SemanticChange::CapabilityRemoved {
                capability: *capability,
            }),
            Some(after_limit) if after_limit != limit => {
                changes.push(SemanticChange::CapabilityLimitChanged {
                    capability: *capability,
                    before: *limit,
                    after: *after_limit,
                });
            }
            Some(_) => {}
        }
    }
    for capability in after_limits.keys() {
        if !before_limits.contains_key(capability) {
            changes.push(SemanticChange::CapabilityAdded {
                capability: *capability,
            });
        }
    }
    for kind in before.possible_edit_kinds.difference(&after.possible_edit_kinds) {
        changes.push(SemanticChange::EditKindRemoved { kind: *kind });
    }
    for kind in after.possible_edit_kinds.difference(&before.possible_edit_kinds) {
        changes.push(SemanticChange::EditKindAdded { kind: *kind });
    }
    if before.static_cost.max_output_candidates != after.static_cost.max_output_candidates {
        changes.push(SemanticChange::MaxOutputCandidatesChanged {
            before: before.static_cost.max_output_candidates,
            after: after.static_cost.max_output_candidates,
        });
    }
    if before.nodes.len() != after.nodes.len() {
        changes.push(SemanticChange::StaticCostChanged {
            before_steps: before.nodes.len() as u32,
            after_steps: after.nodes.len() as u32,
        });
    }
    let conditions = |ir: &WorkflowIr| {
        ir.nodes
            .iter()
            .filter_map(|node| match &node.kind {
                IrNodeKind::Condition(value) => Some(value.expression_summary.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let transforms = |ir: &WorkflowIr| {
        ir.nodes
            .iter()
            .filter_map(|node| match &node.kind {
                IrNodeKind::Transform(value) => Some(value.expression_summary.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for (before, after) in conditions(before).into_iter().zip(conditions(after)) {
        if before != after {
            changes.push(SemanticChange::ConditionChanged { before, after });
        }
    }
    for (before, after) in transforms(before).into_iter().zip(transforms(after)) {
        if before != after {
            changes.push(SemanticChange::TransformChanged { before, after });
        }
    }
    SemanticDiff { changes }
}
```

- [ ] **Step 4: Add exact compatibility failures**

Create `executor.rs` initially with:

```rust
use crate::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    ValidatedAutomation, CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1,
    OUTPUT_SCHEMA_V1,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompatibilityError {
    #[error("language schema mismatch")]
    Language {
        installed: LanguageSchemaVersion,
        artifact: LanguageSchemaVersion,
    },
    #[error("IR schema mismatch")]
    Ir {
        installed: IrSchemaVersion,
        artifact: IrSchemaVersion,
    },
    #[error("capability API mismatch")]
    Capability {
        installed: CapabilityApiVersion,
        artifact: CapabilityApiVersion,
    },
    #[error("output schema mismatch")]
    Output {
        installed: OutputSchemaVersion,
        artifact: OutputSchemaVersion,
    },
}

pub fn ensure_compatible(
    automation: &ValidatedAutomation,
) -> Result<(), CompatibilityError> {
    if automation.language_schema_version != LANGUAGE_SCHEMA_V1 {
        return Err(CompatibilityError::Language {
            installed: LANGUAGE_SCHEMA_V1,
            artifact: automation.language_schema_version,
        });
    }
    if automation.ir_schema_version != IR_SCHEMA_V1 {
        return Err(CompatibilityError::Ir {
            installed: IR_SCHEMA_V1,
            artifact: automation.ir_schema_version,
        });
    }
    if automation.capability_api_version != CAPABILITY_API_V1 {
        return Err(CompatibilityError::Capability {
            installed: CAPABILITY_API_V1,
            artifact: automation.capability_api_version,
        });
    }
    if automation.output_schema_version != OUTPUT_SCHEMA_V1 {
        return Err(CompatibilityError::Output {
            installed: OUTPUT_SCHEMA_V1,
            artifact: automation.output_schema_version,
        });
    }
    Ok(())
}
```

- [ ] **Step 5: Export and run tests**

Export:

```rust
mod diff;
mod executor;
pub use diff::{semantic_diff, semantic_summary, SemanticChange, SemanticDiff, SemanticSummary};
pub use executor::{ensure_compatible, CompatibilityError};
```

Run:

```bash
rtk cargo test -p rollshot-automation --test frontend_contract
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-automation
rtk git commit -m "feat(automation): add semantic review metadata"
```

---

### Task 8: Strictly decode all output edit kinds into an `EditProposal`

**Files:**
- Create: `crates/rollshot-automation/src/output.rs`
- Modify: `crates/rollshot-automation/src/lib.rs`
- Create: `crates/rollshot-automation/tests/output_contract.rs`

- [ ] **Step 1: Write full-CRUD decoding tests**

Create `output_contract.rs` with helpers and the core success test:

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rollshot_automation::{
    decode_proposal, ExecutionPolicy, OutputError, ProposalContext, ProposedEditKind,
};
use rollshot_edit_proposal::{ProposalId, ProposedEdit, Provenance, ProvenanceSource};
use rollshot_image_document::AnnotationId;

fn context() -> ProposalContext {
    ProposalContext {
        proposal_id: ProposalId(7),
        base_document_state_id: 11,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 42 },
        },
    }
}

fn allow_all() -> ExecutionPolicy {
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    policy.allowed_edit_kinds = BTreeSet::from([
        ProposedEditKind::AddRedaction,
        ProposedEditKind::AddTextNote,
        ProposedEditKind::AddNumberCallout,
        ProposedEditKind::UpdateRedactionBounds,
        ProposedEditKind::UpdateTextPosition,
        ProposedEditKind::UpdateText,
        ProposedEditKind::UpdateNumberPoints,
        ProposedEditKind::Delete,
    ]);
    policy.allowed_annotation_ids = BTreeSet::from([AnnotationId(42)]);
    policy
}

#[test]
fn decodes_complete_crud_union_in_output_order() {
    let json = r#"{
      "candidates": [
        {"kind":"addRedaction","bounds":{"x":1.0,"y":2.0,"width":3.0,"height":4.0},"confidence":0.9,"label":"secret"},
        {"kind":"addTextNote","position":{"x":5.0,"y":6.0},"text":"note","confidence":0.8,"label":"note"},
        {"kind":"addNumberCallout","tip":{"x":7.0,"y":8.0},"bubble":{"x":9.0,"y":10.0},"confidence":0.7,"label":"step"},
        {"kind":"updateRedactionBounds","annotationId":"42","bounds":{"x":2.0,"y":3.0,"width":4.0,"height":5.0},"confidence":0.6,"label":"resize"},
        {"kind":"updateTextPosition","annotationId":"42","position":{"x":4.0,"y":5.0},"confidence":0.6,"label":"move"},
        {"kind":"updateText","annotationId":"42","text":"changed","confidence":0.6,"label":"text"},
        {"kind":"updateNumberPoints","annotationId":"42","tip":{"x":1.0,"y":1.0},"bubble":{"x":2.0,"y":2.0},"confidence":0.6,"label":"points"},
        {"kind":"delete","annotationId":"42","confidence":0.5,"label":"remove"}
      ]
    }"#;
    let proposal = decode_proposal(json, (100, 100), &context(), &allow_all()).unwrap();
    assert_eq!(proposal.id, ProposalId(7));
    assert_eq!(proposal.base_document_state_id, 11);
    assert_eq!(proposal.candidates.len(), 8);
    assert!(matches!(
        proposal.candidates[0].edit,
        ProposedEdit::AddRedaction { .. }
    ));
    assert!(matches!(
        proposal.candidates[7].edit,
        ProposedEdit::Delete {
            id: AnnotationId(42)
        }
    ));
    assert_eq!(proposal.candidates[0].label, "secret");
}
```

Add rejection tests:

```rust
#[test]
fn rejects_unknown_fields_and_noncanonical_annotation_ids() {
    let unknown = r#"{"candidates":[{"kind":"delete","annotationId":"42","confidence":0.5,"label":"x","extra":true}]}"#;
    assert!(matches!(
        decode_proposal(unknown, (100, 100), &context(), &allow_all()),
        Err(OutputError::Malformed { .. })
    ));

    for id in ["+42", "042", " 42", "18446744073709551616"] {
        let json = format!(
            r#"{{"candidates":[{{"kind":"delete","annotationId":"{id}","confidence":0.5,"label":"x"}}]}}"#
        );
        assert!(matches!(
            decode_proposal(&json, (100, 100), &context(), &allow_all()),
            Err(OutputError::InvalidAnnotationId { .. })
        ));
    }
}

#[test]
fn rejects_unauthorized_edit_kind_and_annotation_id() {
    let delete = r#"{"candidates":[{"kind":"delete","annotationId":"42","confidence":0.5,"label":"x"}]}"#;
    let redaction_only = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    assert_eq!(
        decode_proposal(delete, (100, 100), &context(), &redaction_only),
        Err(OutputError::EditKindDenied {
            kind: ProposedEditKind::Delete,
        })
    );
}
```

- [ ] **Step 2: Run and confirm missing decoder**

Run:

```bash
rtk cargo test -p rollshot-automation --test output_contract
```

Expected: FAIL because output decoding APIs do not exist.

- [ ] **Step 3: Define deny-unknown-fields wire types**

Create `output.rs` with private serde types:

```rust
use rollshot_edit_proposal::{
    validate_policy, CandidateId, ConfidenceSummary, EditProposal, ProposedCandidate,
    ProposedEdit,
};
use rollshot_image_document::{AnnotationId, ImagePoint, ImageRect};
use serde::Deserialize;

use crate::{ExecutionPolicy, ProposalContext, ProposedEditKind};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEnvelope {
    candidates: Vec<WireCandidate>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum WireCandidate {
    AddRedaction {
        bounds: WireRect,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
    AddTextNote {
        position: WirePoint,
        text: String,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
    AddNumberCallout {
        tip: WirePoint,
        bubble: WirePoint,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
    UpdateRedactionBounds {
        annotation_id: String,
        bounds: WireRect,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
    UpdateTextPosition {
        annotation_id: String,
        position: WirePoint,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
    UpdateText {
        annotation_id: String,
        text: String,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
    UpdateNumberPoints {
        annotation_id: String,
        tip: WirePoint,
        bubble: WirePoint,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
    Delete {
        annotation_id: String,
        confidence: f32,
        label: String,
        rationale: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePoint {
    x: f32,
    y: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}
```

Because internally tagged enums do not reliably reject variant-specific unknown fields by themselves, deserialize each candidate first as `serde_json::Value`, read the literal `kind`, then deserialize into a dedicated `#[serde(deny_unknown_fields)]` struct for that variant. Do not rely only on the enum declaration above; retain it only if tests prove every extra-field case is rejected.

- [ ] **Step 4: Implement strict validation and conversion**

Add:

```rust
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OutputError {
    #[error("output exceeds byte limit")]
    TooLarge,
    #[error("malformed output: {code}")]
    Malformed { code: &'static str },
    #[error("invalid annotation id")]
    InvalidAnnotationId { value: String },
    #[error("invalid finite range: {field}")]
    InvalidNumber { field: &'static str },
    #[error("edit kind denied")]
    EditKindDenied { kind: ProposedEditKind },
    #[error("annotation id denied")]
    AnnotationDenied { id: AnnotationId },
    #[error("proposal policy rejected output")]
    Policy(#[from] rollshot_edit_proposal::PolicyError),
}

fn parse_annotation_id(value: &str) -> Result<AnnotationId, OutputError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(OutputError::InvalidAnnotationId {
            value: value.into(),
        });
    }
    value
        .parse::<u64>()
        .map(AnnotationId)
        .map_err(|_| OutputError::InvalidAnnotationId {
            value: value.into(),
        })
}

fn point(value: WirePoint) -> Result<ImagePoint, OutputError> {
    if !value.x.is_finite() || !value.y.is_finite() {
        return Err(OutputError::InvalidNumber { field: "point" });
    }
    Ok(ImagePoint::new(value.x, value.y))
}

fn rect(value: WireRect) -> Result<ImageRect, OutputError> {
    if !value.x.is_finite()
        || !value.y.is_finite()
        || !value.width.is_finite()
        || !value.height.is_finite()
        || value.width <= 0.0
        || value.height <= 0.0
    {
        return Err(OutputError::InvalidNumber { field: "bounds" });
    }
    Ok(ImageRect::from_corners(
        ImagePoint::new(value.x, value.y),
        ImagePoint::new(value.x + value.width, value.y + value.height),
    ))
}

fn validate_metadata(
    confidence: f32,
    label: &str,
    rationale: Option<&str>,
) -> Result<(), OutputError> {
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(OutputError::InvalidNumber {
            field: "confidence",
        });
    }
    if label.trim().is_empty() || label.len() > 128 {
        return Err(OutputError::Malformed {
            code: "invalid_label",
        });
    }
    if rationale.is_some_and(|text| text.len() > 2_048) {
        return Err(OutputError::Malformed {
            code: "rationale_too_long",
        });
    }
    Ok(())
}
```

Implement `decode_proposal` so it:

1. Rejects `json.len() > policy.max_output_bytes`.
2. Strictly decodes the envelope and each tagged variant.
3. Validates metadata, text lengths, finite geometry, and edit-specific fields.
4. Checks `allowed_edit_kinds`.
5. Checks every update/delete ID against `allowed_annotation_ids`.
6. Allocates `CandidateId(index as u64 + 1)` in output order.
7. Copies `ProposalContext.provenance` to proposal and candidates.
8. Computes `ConfidenceSummary`.
9. Calls `validate_policy`.

The function signature is:

```rust
pub fn decode_proposal(
    json: &str,
    image_dims: (u32, u32),
    context: &ProposalContext,
    policy: &ExecutionPolicy,
) -> Result<EditProposal, OutputError>
```

- [ ] **Step 5: Run output tests**

Run:

```bash
rtk cargo test -p rollshot-automation --test output_contract
```

Expected: PASS.

- [ ] **Step 6: Run proposal tests to catch model regressions**

Run:

```bash
rtk cargo test -p rollshot-edit-proposal
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-automation
rtk git commit -m "feat(automation): decode typed edit proposals"
```

---

### Task 9: Complete the executor contract and fake implementation tests

**Files:**
- Modify: `crates/rollshot-automation/src/executor.rs`
- Modify: `crates/rollshot-automation/src/lib.rs`
- Create: `crates/rollshot-automation/tests/executor_contract.rs`

- [ ] **Step 1: Add executor-independent tests**

Create:

```rust
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rollshot_automation::{
    ensure_compatible, AutomationExecution, AutomationExecutor, AutomationHost,
    AutomationInput, CancellationFlag, ExecutionError, ExecutionMetrics, ExecutionPolicy,
    FakeAutomationHost, ProposalContext, ValidationLimits, validate_source,
};
use rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource};

struct EchoExecutor;

impl AutomationExecutor for EchoExecutor {
    fn execute(
        &self,
        automation: &rollshot_automation::ValidatedAutomation,
        input: &AutomationInput,
        _proposal: &ProposalContext,
        _host: &mut dyn AutomationHost,
        _policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
    ) -> Result<AutomationExecution, ExecutionError> {
        ensure_compatible(automation)?;
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        Ok(AutomationExecution {
            output_json: r#"{"candidates":[]}"#.into(),
            metrics: ExecutionMetrics {
                duration: Duration::ZERO,
                capability_calls: 0,
                output_bytes: 17,
                interrupted: false,
            },
        })
    }
}

#[test]
fn executor_contract_checks_compatibility_and_cancellation() {
    let automation = validate_source(
        "function main(input) { return { candidates: [] }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let input = AutomationInput {
        image_width: 1,
        image_height: 1,
        region: None,
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let context = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 1 },
        },
    };
    let policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    let cancellation = CancellationFlag::new();
    cancellation.cancel();
    let result = EchoExecutor.execute(
        &automation,
        &input,
        &context,
        &mut FakeAutomationHost::default(),
        &policy,
        &cancellation,
    );
    assert_eq!(result, Err(ExecutionError::Cancelled));
}
```

- [ ] **Step 2: Run and confirm missing executor types**

Run:

```bash
rtk cargo test -p rollshot-automation --test executor_contract
```

Expected: FAIL with missing executor trait and result types.

- [ ] **Step 3: Implement the complete executor interface**

Extend `executor.rs`:

```rust
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use crate::{
    AutomationHost, AutomationInput, CapabilityError, ExecutionPolicy, OutputError,
    ProposalContext, ValidatedAutomation,
};

#[derive(Debug, Clone, Default)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionMetrics {
    pub duration: Duration,
    pub capability_calls: u32,
    pub output_bytes: usize,
    pub interrupted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationExecution {
    pub output_json: String,
    pub metrics: ExecutionMetrics,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SandboxError {
    #[error("sandbox initialization failed: {code}")]
    Initialization { code: &'static str },
    #[error("sandbox memory limit")]
    MemoryLimit,
    #[error("sandbox stack limit")]
    StackLimit,
    #[error("sandbox timeout")]
    Timeout,
    #[error("sandbox interrupted")]
    Interrupted,
    #[error("sandbox evaluation failed: {code}")]
    Evaluation { code: &'static str },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),
    #[error(transparent)]
    Sandbox(#[from] SandboxError),
    #[error(transparent)]
    Capability(#[from] CapabilityError),
    #[error(transparent)]
    Output(#[from] OutputError),
    #[error("execution cancelled")]
    Cancelled,
}

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

Remove unused imports in the test and export all executor types.

- [ ] **Step 4: Add a convenience execute-and-decode helper**

Add:

```rust
pub fn execute_to_proposal(
    executor: &dyn AutomationExecutor,
    automation: &ValidatedAutomation,
    input: &AutomationInput,
    proposal: &ProposalContext,
    host: &mut dyn AutomationHost,
    policy: &ExecutionPolicy,
    cancellation: &CancellationFlag,
) -> Result<(rollshot_edit_proposal::EditProposal, ExecutionMetrics), ExecutionError> {
    let execution = executor.execute(
        automation,
        input,
        proposal,
        host,
        policy,
        cancellation,
    )?;
    let edit_proposal = crate::decode_proposal(
        &execution.output_json,
        (input.image_width, input.image_height),
        proposal,
        policy,
    )?;
    Ok((edit_proposal, execution.metrics))
}
```

- [ ] **Step 5: Run executor and full frontend tests**

Run:

```bash
rtk cargo test -p rollshot-automation
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-automation
rtk git commit -m "feat(automation): define executor contract"
```

---

### Task 10: Establish QuickJS lockdown in production code

**Files:**
- Modify: `crates/rollshot-automation-rquickjs/src/lib.rs`
- Create: `crates/rollshot-automation-rquickjs/src/lockdown.rs`
- Create: `crates/rollshot-automation-rquickjs/tests/lockdown.rs`

- [ ] **Step 1: Port lockdown gates as production tests**

Create tests covering:

```rust
use rquickjs::{Value, Undefined};
use rollshot_automation_rquickjs::LockedContext;

const STRIPPED: &[&str] = &[
    "eval",
    "Function",
    "queueMicrotask",
    "globalThis",
    "Reflect",
];

#[test]
fn dangerous_base_globals_are_stripped_and_verified() {
    let locked = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked.with(|ctx| {
        let globals = ctx.globals();
        for name in STRIPPED {
            let value: Value = globals.get(*name).unwrap();
            assert!(value.is_undefined(), "{name} is still present");
        }
    });
}

#[test]
fn ambient_platform_globals_are_absent() {
    let locked = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked.with(|ctx| {
        let globals = ctx.globals();
        for name in [
            "fetch",
            "XMLHttpRequest",
            "WebSocket",
            "setTimeout",
            "setInterval",
            "Promise",
            "Proxy",
            "require",
            "process",
            "Deno",
            "Bun",
            "Worker",
            "document",
            "window",
        ] {
            let value: Value = globals.get(name).unwrap();
            assert!(value.is_undefined(), "{name} is unexpectedly present");
        }
    });
}
```

Also port the spike's fresh-runtime marker and prototype-isolation tests, but run them through `LockedContext`, not raw contexts.

- [ ] **Step 2: Run and verify missing lockdown implementation**

Run:

```bash
rtk cargo test -p rollshot-automation-rquickjs --test lockdown
```

Expected: FAIL because `LockedContext` does not exist.

- [ ] **Step 3: Implement fresh restricted runtimes**

Create `lockdown.rs`:

```rust
use rquickjs::context::intrinsic;
use rquickjs::{Context, Runtime, Undefined, Value};
use rollshot_automation::SandboxError;

const STRIPPED_GLOBALS: &[&str] = &[
    "eval",
    "Function",
    "queueMicrotask",
    "globalThis",
    "Reflect",
];

pub struct LockedContext {
    runtime: Runtime,
    context: Context,
}

impl LockedContext {
    pub fn new(memory_bytes: usize, stack_bytes: usize) -> Result<Self, SandboxError> {
        let runtime = Runtime::new().map_err(|_| SandboxError::Initialization {
            code: "runtime_create",
        })?;
        runtime.set_memory_limit(memory_bytes);
        runtime.set_max_stack_size(stack_bytes);
        let context = Context::builder()
            .with::<intrinsic::Eval>()
            .with::<intrinsic::Json>()
            .build(&runtime)
            .map_err(|_| SandboxError::Initialization {
                code: "context_create",
            })?;

        context.with(|ctx| {
            let globals = ctx.globals();
            for name in STRIPPED_GLOBALS {
                globals
                    .set(*name, Undefined)
                    .map_err(|_| SandboxError::Initialization {
                        code: "strip_global",
                    })?;
                let value: Value =
                    globals
                        .get(*name)
                        .map_err(|_| SandboxError::Initialization {
                            code: "verify_global",
                        })?;
                if !value.is_undefined() {
                    return Err(SandboxError::Initialization {
                        code: "global_remains",
                    });
                }
            }
            Ok(())
        })?;

        Ok(Self { runtime, context })
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn with<T>(&self, callback: impl for<'js> FnOnce(rquickjs::Ctx<'js>) -> T) -> T {
        self.context.with(callback)
    }
}
```

Do not call `Context::full()` or `Runtime::set_loader()` anywhere in the crate.

- [ ] **Step 4: Export lockdown only for crate tests and executor internals**

In `lib.rs`:

```rust
mod lockdown;

pub use lockdown::LockedContext;

#[derive(Debug, Default)]
pub struct QuickJsExecutor;
```

- [ ] **Step 5: Run lockdown tests**

Run:

```bash
rtk cargo test -p rollshot-automation-rquickjs --test lockdown
```

Expected: PASS.

- [ ] **Step 6: Prove forbidden constructors are absent from production source**

Run:

```bash
rtk rg -n 'Context::full|set_loader' crates/rollshot-automation-rquickjs
```

Expected: no matches outside the negative assertion text in this plan.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-automation-rquickjs
rtk git commit -m "feat(automation): harden QuickJS context"
```

---

### Task 11: Bridge capabilities and execute validated source in QuickJS

**Files:**
- Create: `crates/rollshot-automation-rquickjs/src/bridge.rs`
- Create: `crates/rollshot-automation-rquickjs/src/execution.rs`
- Modify: `crates/rollshot-automation-rquickjs/src/lib.rs`
- Create: `crates/rollshot-automation-rquickjs/tests/end_to_end.rs`
- Create: `crates/rollshot-automation-rquickjs/tests/resources.rs`

- [ ] **Step 1: Add an end-to-end host bridge test**

Create a valid OCR automation test:

```rust
use std::time::Duration;

use rollshot_automation::{
    execute_to_proposal, AutomationInput, CancellationFlag, ExecutionPolicy,
    FakeAutomationHost, OcrMatch, ProposalContext, Region, ValidationLimits, validate_source,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource};
use rollshot_image_document::{ImagePoint, ImageRect};

#[test]
fn ocr_capability_produces_redaction_proposal() {
    let source = r#"
function expandBounds(rect, padding) {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}
function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 5 });
  return {
    candidates: matches
      .filter((match) => match.confidence > 0.8)
      .map((match) => ({
        kind: "addRedaction",
        bounds: expandBounds(match.bounds, 1),
        confidence: match.confidence,
        label: "ocr-match",
      })),
  };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: 100,
        image_height: 100,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let mut host = FakeAutomationHost {
        ocr_results: vec![OcrMatch {
            bounds: ImageRect::from_corners(
                ImagePoint::new(10.0, 10.0),
                ImagePoint::new(20.0, 20.0),
            ),
            text: "secret@example.com".into(),
            confidence: 0.95,
        }],
        ..Default::default()
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 9,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 3 },
        },
    };
    let policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    let (result, metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].label, "ocr-match");
    assert_eq!(metrics.capability_calls, 1);
}
```

- [ ] **Step 2: Run and confirm executor/bridge failure**

Run:

```bash
rtk cargo test -p rollshot-automation-rquickjs --test end_to_end ocr_capability_produces_redaction_proposal
```

Expected: FAIL because `QuickJsExecutor` does not implement `AutomationExecutor`.

- [ ] **Step 3: Implement JSON-based frozen value installation**

Create `bridge.rs` with:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use std::collections::BTreeMap;

use rquickjs::{Ctx, Function, Object, Value};
use rollshot_automation::{
    AutomationHost, AutomationInput, CapabilityError, CapabilityName, ExecutionPolicy, Region,
};

pub(crate) struct BridgeState<'a> {
    pub host: &'a mut dyn AutomationHost,
    pub policy: &'a ExecutionPolicy,
    pub capability_calls: u32,
    /// Per-capability call counts, checked against `policy.max_calls_by_capability`.
    pub calls_by_capability: BTreeMap<CapabilityName, u32>,
    pub host_allocation_bytes: usize,
    /// Out-of-band typed error. A host callback that fails (capability error,
    /// host-allocation limit, global/per-capability call limit, cancellation)
    /// stores the typed Rust error here and then raises a *content-free* JS
    /// exception to unwind `main`. After `locked.with(...)` returns, the
    /// executor checks `pending_error` FIRST and maps it to the exact
    /// `ExecutionError` variant. The JS exception message is never parsed to
    /// recover the category — that is what this field is for.
    pub pending_error: Option<CapabilityError>,
}

pub(crate) fn install_input<'js>(
    ctx: &Ctx<'js>,
    input: &AutomationInput,
) -> rquickjs::Result<Value<'js>> {
    let json = serde_json::to_string(input)
        .map_err(|error| rquickjs::Error::new_from_js_message("rust", "json", error.to_string()))?;
    let value: Value = ctx.json_parse(json)?;
    deep_freeze(ctx, value.clone())?;
    ctx.globals().set("input", value.clone())?;
    Ok(value)
}

fn deep_freeze<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<()> {
    if let Some(object) = value.as_object() {
        let keys = object.keys::<String>().collect::<rquickjs::Result<Vec<_>>>()?;
        for key in keys {
            let child: Value = object.get(key)?;
            deep_freeze(ctx, child)?;
        }
        let object_constructor: Object = ctx.globals().get("Object")?;
        let freeze: Function = object_constructor.get("freeze")?;
        freeze.call::<_, ()>((object.clone(),))?;
    }
    Ok(())
}
```

The serde contracts from Task 3 must produce these exact JavaScript shapes:

```json
{"kind":"full"}
```

and:

```json
{"kind":"rect","bounds":{"x":0.0,"y":0.0,"width":10.0,"height":10.0}}
```

- [ ] **Step 4: Install direct host functions**

Add `install_rollshot` that creates one object and four `Function::new` callbacks. Each callback must:

1. Decode one query object using strict serde types.
2. Charge the global and per-capability counters before host dispatch, and
   reject when a limit is exceeded.
   - Global: increment `capability_calls`; reject if it exceeds
     `policy.max_capability_calls`.
   - Per-capability: increment `calls_by_capability[name]`; reject if it exceeds
     `policy.max_calls_by_capability[name]` **when that key is present**. An
     absent key means "no per-capability cap — global cap only" (the
     `smart_redaction_default` map is empty, so by default only the global cap
     applies). Document this empty-map semantics in a doc-comment on
     `ExecutionPolicy::max_calls_by_capability`.
3. Reject cancellation before host dispatch.
4. Call `AutomationHost`.
5. Validate finite returned values.
6. Serialize results and charge `host_allocation_bytes`.
7. Reject allocation beyond `policy.max_host_allocation_bytes`.
8. Parse the result into a JavaScript value.
9. Recursively freeze the result.

Every rejection in steps 2, 3, 6, and 7 follows the out-of-band protocol: store
the typed error in `BridgeState.pending_error` (`CapabilityError::LimitExceeded`
for call-count / host-allocation limits; leave `pending_error` as the
host-returned error for step 4 failures), then raise a content-free JS exception
to unwind. Cancellation in step 3 is signalled by an exception too; the executor
distinguishes it by re-checking the cancellation flag after `locked.with`, ahead
of `pending_error`.

Use:

```rust
pub(crate) fn install_rollshot<'js>(
    ctx: &Ctx<'js>,
    state: Rc<RefCell<BridgeState<'_>>>,
) -> rquickjs::Result<Object<'js>>
```

Install method names exactly:

```rust
rollshot.set("ocr", ocr_function)?;
rollshot.set("layout", layout_function)?;
rollshot.set("regionFeatures", region_features_function)?;
rollshot.set("templateMatch", template_match_function)?;
let object_constructor: Object = ctx.globals().get("Object")?;
let freeze: Function = object_constructor.get("freeze")?;
freeze.call::<_, ()>((rollshot.clone(),))?;
ctx.globals().set("rollshot", rollshot.clone())?;
```

Map errors to stable JavaScript exception messages containing only error codes, never query or OCR contents.

- [ ] **Step 5: Add resource and cancellation tests**

Create:

```rust
use std::time::Duration;

use rollshot_automation::{
    AutomationExecutor, AutomationInput, CancellationFlag, ExecutionError, ExecutionPolicy,
    FakeAutomationHost, ProposalContext, SandboxError, ValidationLimits, validate_source,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource};

fn input() -> AutomationInput {
    AutomationInput {
        image_width: 10,
        image_height: 10,
        region: None,
        annotations: Vec::new(),
        capability_handles: Default::default(),
    }
}

fn context() -> ProposalContext {
    ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 1 },
        },
    }
}

fn policy() -> ExecutionPolicy {
    ExecutionPolicy::smart_redaction_default(
        Duration::from_millis(25),
        4 * 1024 * 1024,
        128 * 1024,
    )
}

#[test]
fn pre_cancelled_execution_never_runs() {
    let automation = validate_source(
        "function main(input) { return { candidates: [] }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let cancellation = CancellationFlag::new();
    cancellation.cancel();
    let result = QuickJsExecutor.execute(
        &automation,
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &cancellation,
    );
    assert_eq!(result, Err(ExecutionError::Cancelled));
}
```

Add runtime-only adversarial fixtures by constructing `ValidatedAutomation` from a valid source and replacing its source in the test after validation. This intentionally tests defense-in-depth runtime limits without weakening production validation:

```rust
fn runtime_payload(source: &str) -> rollshot_automation::ValidatedAutomation {
    let mut automation = validate_source(
        "function main(input) { return { candidates: [] }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    automation.source = source.into();
    automation
}

#[test]
fn interrupt_stops_infinite_runtime_payload() {
    let result = QuickJsExecutor.execute(
        &runtime_payload("function main(input) { while (true) {} }"),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert!(matches!(
        result,
        Err(ExecutionError::Sandbox(SandboxError::Timeout))
            | Err(ExecutionError::Sandbox(SandboxError::Interrupted))
    ));
}
```

Add these exact tests:

```rust
#[test]
fn memory_limit_stops_runtime_allocation() {
    let mut limits = policy();
    limits.max_wall_time = Duration::from_secs(1);
    limits.max_memory_bytes = 1024 * 1024;
    let result = QuickJsExecutor.execute(
        &runtime_payload(
            "function main(input) { const a = []; while (true) { a.push(new Array(1000).fill(1)); } }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &limits,
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Sandbox(SandboxError::MemoryLimit))
    );
}

#[test]
fn stack_limit_stops_runtime_recursion() {
    let result = QuickJsExecutor.execute(
        &runtime_payload(
            "function recurse() { return recurse(); } function main(input) { return recurse(); }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Sandbox(SandboxError::StackLimit))
    );
}

#[test]
fn host_allocation_limit_rejects_large_capability_result() {
    let source = r#"
function main(input) {
  rollshot.ocr({ region: { kind: "full" }, limit: 1 });
  return { candidates: [] };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let mut host = FakeAutomationHost {
        ocr_results: vec![rollshot_automation::OcrMatch {
            bounds: rollshot_image_document::ImageRect::from_corners(
                rollshot_image_document::ImagePoint::new(0.0, 0.0),
                rollshot_image_document::ImagePoint::new(1.0, 1.0),
            ),
            text: "x".repeat(4_096),
            confidence: 1.0,
        }],
        ..Default::default()
    };
    let mut limits = policy();
    limits.max_host_allocation_bytes = 128;
    assert_eq!(
        QuickJsExecutor.execute(
            &automation,
            &input(),
            &context(),
            &mut host,
            &limits,
            &CancellationFlag::new(),
        ),
        Err(ExecutionError::Capability(
            rollshot_automation::CapabilityError::LimitExceeded,
        ))
    );
}

#[test]
fn output_byte_limit_is_enforced_before_decoding() {
    let mut limits = policy();
    limits.max_output_bytes = 32;
    let result = QuickJsExecutor.execute(
        &runtime_payload(
            "function main(input) { return { candidates: [], padding: 'x'.repeat(1024) }; }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &limits,
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Output(
            rollshot_automation::OutputError::TooLarge,
        ))
    );
}

#[test]
fn fresh_execution_does_not_observe_prior_global_state() {
    let executor = QuickJsExecutor;
    let first = executor.execute(
        &runtime_payload(
            "var __rollshot_marker = 1; function main(input) { return { candidates: [] }; }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert!(first.is_ok());

    let second = executor
        .execute(
            &runtime_payload(
                "function main(input) { return { candidates: typeof __rollshot_marker === 'undefined' ? [] : [{ kind: 'delete', annotationId: '1', confidence: 1, label: 'leak' }] }; }",
            ),
            &input(),
            &context(),
            &mut FakeAutomationHost::default(),
            &policy(),
            &CancellationFlag::new(),
        )
        .unwrap();
    assert_eq!(second.output_json, r#"{"candidates":[]}"#);
}
```

- [ ] **Step 6: Run and confirm missing execution**

Run:

```bash
rtk cargo test -p rollshot-automation-rquickjs --test resources
```

Expected: FAIL because `QuickJsExecutor` does not implement the trait.

- [ ] **Step 7: Implement runtime execution**

Create `execution.rs`:

```rust
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rquickjs::{Function, Value};
use rollshot_automation::{
    ensure_compatible, AutomationExecution, AutomationExecutor, AutomationHost,
    AutomationInput, CancellationFlag, ExecutionError, ExecutionMetrics, ExecutionPolicy,
    ProposalContext, SandboxError, ValidatedAutomation,
};

use crate::bridge::{install_input, install_rollshot, BridgeState};
use crate::lockdown::LockedContext;

impl AutomationExecutor for crate::QuickJsExecutor {
    fn execute(
        &self,
        automation: &ValidatedAutomation,
        input: &AutomationInput,
        _proposal: &ProposalContext,
        host: &mut dyn AutomationHost,
        policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
    ) -> Result<AutomationExecution, ExecutionError> {
        ensure_compatible(automation)?;
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }

        let started = Instant::now();
        let deadline = started + policy.max_wall_time;
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_for_handler = Arc::clone(&interrupted);
        let cancellation_for_handler = cancellation.clone();

        let locked = LockedContext::new(policy.max_memory_bytes, policy.max_stack_bytes)?;
        locked.runtime().set_interrupt_handler(Some(Box::new(move || {
            let should_stop = cancellation_for_handler.is_cancelled()
                || Instant::now() >= deadline;
            if should_stop {
                interrupted_for_handler.store(true, Ordering::SeqCst);
            }
            should_stop
        })));

        let state = Rc::new(RefCell::new(BridgeState {
            host,
            policy,
            capability_calls: 0,
            calls_by_capability: std::collections::BTreeMap::new(),
            host_allocation_bytes: 0,
            pending_error: None,
        }));

        let output_json = locked.with(|ctx| {
            let input_value = install_input(&ctx, input)
                .map_err(|_| SandboxError::Initialization { code: "input" })?;
            install_rollshot(&ctx, Rc::clone(&state))
                .map_err(|_| SandboxError::Initialization { code: "host_api" })?;
            ctx.eval::<(), _>(automation.source.as_bytes())
                .map_err(|_| SandboxError::Evaluation { code: "source" })?;
            let main: Function = ctx
                .globals()
                .get("main")
                .map_err(|_| SandboxError::Evaluation { code: "missing_main" })?;
            let value: Value = main
                .call((input_value,))
                .map_err(|_| SandboxError::Evaluation { code: "main_call" })?;
            ctx.json_stringify(value)
                .map_err(|_| SandboxError::Evaluation { code: "stringify" })?
                .ok_or(SandboxError::Evaluation { code: "undefined_output" })?
                .to_string()
                .map_err(|_| SandboxError::Evaluation { code: "utf8_output" })
        });

        let duration = started.elapsed();
        // Classification ORDER matters — the tests assert exact variants, so the
        // first matching cause wins. Resolve in this order:
        //   1. cancellation flag set          → ExecutionError::Cancelled
        //   2. bridge stored a typed error    → ExecutionError::Capability(..)
        //   3. interrupt fired + deadline hit  → SandboxError::Timeout
        //   4. runtime memory/stack exception  → SandboxError::MemoryLimit / StackLimit
        //   5. any other eval failure          → SandboxError::Evaluation { code }
        //   6. success                         → check output byte ceiling, then Ok
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        if let Some(error) = state.borrow_mut().pending_error.take() {
            return Err(ExecutionError::Capability(error));
        }
        if output_json.is_err()
            && interrupted.load(Ordering::SeqCst)
            && duration >= policy.max_wall_time
        {
            return Err(ExecutionError::Sandbox(SandboxError::Timeout));
        }
        let output_json = output_json?;
        if output_json.len() > policy.max_output_bytes {
            return Err(ExecutionError::Output(
                rollshot_automation::OutputError::TooLarge,
            ));
        }
        let capability_calls = state.borrow().capability_calls;
        tracing::debug!(
            target: "rollshot::automation::executor",
            duration_ms = duration.as_millis() as u64,
            capability_calls,
            output_bytes = output_json.len(),
            interrupted = interrupted.load(Ordering::SeqCst),
            "automation execution completed"
        );
        Ok(AutomationExecution {
            metrics: ExecutionMetrics {
                duration,
                capability_calls,
                output_bytes: output_json.len(),
                interrupted: interrupted.load(Ordering::SeqCst),
            },
            output_json,
        })
    }
}
```

The `.map_err(|_| SandboxError::Evaluation { code })` calls inside the closure
above are a **skeleton, not the final mapping**. As written they collapse memory
exhaustion, stack overflow, and host-limit rejections all into `Evaluation`,
which makes `memory_limit_stops_runtime_allocation`,
`stack_limit_stops_runtime_recursion`, and `host_allocation_limit_rejects_large_capability_result`
(all of which assert *exact* variants) FAIL. You must replace the generic
mapping with a typed classification by inspecting `rquickjs::Error` and runtime
exception state. Cross-reference the verified spike (`spikes/sandbox-executor/`)
for the exact rquickjs `0.12` error shapes — it already drives memory, stack,
and interrupt limits to completion.

Final mapping (the post-closure ordering above resolves 1–3; this list is how to
detect 4–5 from `rquickjs::Error`):

- cancellation flag set → `ExecutionError::Cancelled` (post-closure check 1);
- bridge stored a typed error in `pending_error` → `ExecutionError::Capability` (check 2);
- interrupt fired and deadline reached → `SandboxError::Timeout` (check 3);
- `rquickjs::Error::Allocation` (memory-limit OOM) → `SandboxError::MemoryLimit`;
- stack-overflow exception (rquickjs surfaces a `RangeError`-class exception when
  `set_max_stack_size` is exceeded) → `SandboxError::StackLimit`;
- all other runtime failures → privacy-safe `SandboxError::Evaluation { code }`.

The bridge stores typed Rust errors out-of-band in `BridgeState.pending_error`;
do not parse exception message text to recover error categories. Memory and
stack causes are the one exception — they originate inside QuickJS (no Rust
callback runs), so they must be recovered from `rquickjs::Error` directly, as
the spike does.

- [ ] **Step 8: Verify runtime compilation has only the required intrinsics**

The production context construction must remain:

```rust
Context::builder()
    .with::<rquickjs::context::intrinsic::Eval>()
    .with::<rquickjs::context::intrinsic::Json>()
    .build(&runtime)
```

Retain the five-global stripping and verification around this builder. Do not
enable `Promise`, `Proxy`, timers, loaders, or `Context::full()`.

- [ ] **Step 9: Add deterministic coverage for the remaining capabilities**

Add these tests to `end_to_end.rs` using the same setup as
`ocr_capability_produces_redaction_proposal`:

```rust
fn run_source(
    source: &str,
    host: &mut FakeAutomationHost,
    allowed_kind: ProposedEditKind,
) -> Result<
    (rollshot_edit_proposal::EditProposal, ExecutionMetrics),
    ExecutionError,
> {
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: 100,
        image_height: 100,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 9,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 3 },
        },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    policy.allowed_edit_kinds.insert(allowed_kind);
    execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        host,
        &policy,
        &CancellationFlag::new(),
    )
}

#[test]
fn layout_capability_produces_text_note() {
    let source = r#"
function main(input) {
  const regions = rollshot.layout({ region: input.region, limit: 1 });
  return {
    candidates: regions.map((region) => ({
      kind: "addTextNote",
      position: { x: region.bounds.x, y: region.bounds.y },
      text: region.role,
      confidence: region.confidence,
      label: "layout-region",
    })),
  };
}
"#;
    let bounds = ImageRect::from_corners(
        ImagePoint::new(5.0, 6.0),
        ImagePoint::new(20.0, 16.0),
    );
    let mut host = FakeAutomationHost {
        layout_results: vec![LayoutRegion {
            bounds,
            role: "dialog".into(),
            confidence: 0.9,
        }],
        ..Default::default()
    };
    let (proposal, metrics) =
        run_source(source, &mut host, ProposedEditKind::AddTextNote).unwrap();
    assert!(matches!(
        &proposal.candidates[0].edit,
        ProposedEdit::AddTextNote { position, text }
            if *position == ImagePoint::new(5.0, 6.0) && text == "dialog"
    ));
    assert_eq!(metrics.capability_calls, 1);
}

#[test]
fn region_features_capability_uses_pure_geometry_helper() {
    let source = r#"
function expand(rect) {
  return {
    x: rect.x - 1,
    y: rect.y - 1,
    width: rect.width + 2,
    height: rect.height + 2,
  };
}
function main(input) {
  const features = rollshot.regionFeatures({ region: input.region, limit: 1 });
  return {
    candidates: features.map((feature) => ({
      kind: "addRedaction",
      bounds: expand(feature.bounds),
      confidence: 0.9,
      label: "feature-region",
    })),
  };
}
"#;
    let mut host = FakeAutomationHost {
        region_feature_results: vec![RegionFeatures {
            bounds: ImageRect::from_corners(
                ImagePoint::new(10.0, 10.0),
                ImagePoint::new(20.0, 20.0),
            ),
            dominant_rgba: [0, 0, 0, 255],
            edge_density: 0.5,
        }],
        ..Default::default()
    };
    let (proposal, metrics) =
        run_source(source, &mut host, ProposedEditKind::AddRedaction).unwrap();
    assert!(matches!(
        proposal.candidates[0].edit,
        ProposedEdit::AddRedaction { bounds }
            if bounds
                == ImageRect::from_corners(
                    ImagePoint::new(9.0, 9.0),
                    ImagePoint::new(21.0, 21.0),
                )
    ));
    assert_eq!(metrics.capability_calls, 1);
}

#[test]
fn template_match_capability_produces_number_callout() {
    let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: "profile",
    region: input.region,
    limit: 1,
  });
  return {
    candidates: matches.map((match) => ({
      kind: "addNumberCallout",
      tip: match.anchor,
      bubble: { x: match.bounds.x, y: match.bounds.y },
      confidence: match.score,
      label: "template-match",
    })),
  };
}
"#;
    let mut host = FakeAutomationHost {
        template_results: vec![TemplateMatch {
            bounds: ImageRect::from_corners(
                ImagePoint::new(30.0, 40.0),
                ImagePoint::new(50.0, 60.0),
            ),
            score: 0.95,
            anchor: ImagePoint::new(35.0, 45.0),
        }],
        ..Default::default()
    };
    let (proposal, metrics) =
        run_source(source, &mut host, ProposedEditKind::AddNumberCallout).unwrap();
    assert!(matches!(
        proposal.candidates[0].edit,
        ProposedEdit::AddNumberCallout { tip, bubble }
            if tip == ImagePoint::new(35.0, 45.0)
                && bubble == ImagePoint::new(30.0, 40.0)
    ));
    assert_eq!(metrics.capability_calls, 1);
}

#[test]
fn capability_error_remains_typed() {
    let source = r#"
function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 1 });
  return { candidates: matches };
}
"#;
    let mut host = FakeAutomationHost {
        failure: Some(CapabilityError::Failed {
            code: "fixture_failure",
        }),
        ..Default::default()
    };
    assert_eq!(
        run_source(source, &mut host, ProposedEditKind::AddRedaction),
        Err(ExecutionError::Capability(CapabilityError::Failed {
            code: "fixture_failure",
        }))
    );
}
```

Import `ExecutionError`, `ExecutionMetrics`, `LayoutRegion`, `ProposedEditKind`,
`RegionFeatures`, `TemplateMatch`, and `ProposedEdit` for these tests.

- [ ] **Step 10: Run resource and end-to-end tests**

Run:

```bash
rtk cargo test -p rollshot-automation-rquickjs --test resources
rtk cargo test -p rollshot-automation-rquickjs --test end_to_end
rtk cargo test -p rollshot-automation-rquickjs --test lockdown
```

Expected: PASS.

- [ ] **Step 11: Verify privacy-safe tracing**

Run:

```bash
rtk rg -n 'tracing::' crates/rollshot-automation crates/rollshot-automation-rquickjs
```

Expected: every event has an explicit `rollshot::automation::*` target and fields contain only counts, versions, durations, capability kind, terminal category, and stable error code.

- [ ] **Step 12: Commit the green bridge/executor slice**

The bridge and executor are one green vertical slice:

```bash
rtk git add crates/rollshot-automation-rquickjs
rtk git commit -m "feat(automation): execute in hardened QuickJS"
```

---

### Task 12: Complete adversarial and cross-crate regression coverage

**Files:**
- Modify: `crates/rollshot-automation-rquickjs/tests/lockdown.rs`
- Modify: `crates/rollshot-automation-rquickjs/tests/resources.rs`
- Modify: `crates/rollshot-automation-rquickjs/tests/end_to_end.rs`
- Modify: `crates/rollshot-automation/tests/frontend_contract.rs`
- Modify: `crates/rollshot-automation/tests/output_contract.rs`

- [ ] **Step 1: Add parser/runtime upgrade contract assertions**

Add tests that assert:

```rust
#[test]
fn dependency_contract_versions_are_locked() {
    assert_eq!(rollshot_automation::LANGUAGE_SCHEMA_V1.0, 1);
    assert_eq!(rollshot_automation::IR_SCHEMA_V1.0, 1);
    assert_eq!(rollshot_automation::CAPABILITY_API_V1.0, 1);
    assert_eq!(rollshot_automation::OUTPUT_SCHEMA_V1.0, 1);
}
```

Add this serde compatibility test:

```rust
#[test]
fn validated_automation_round_trips_with_ir_and_versions() {
    let validated =
        validate_source(&fixture("valid_main.js"), &ValidationLimits::default()).unwrap();
    let json = serde_json::to_string_pretty(&validated).unwrap();
    let decoded: rollshot_automation::ValidatedAutomation =
        serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, validated);
    assert_eq!(decoded.language_schema_version, LANGUAGE_SCHEMA_V1);
    assert_eq!(decoded.workflow_ir.ir_schema_version, IR_SCHEMA_V1);
    assert_eq!(
        decoded.workflow_ir.capability_manifest.capability_api_version,
        CAPABILITY_API_V1
    );
}
```

- [ ] **Step 2: Add every design adversarial case**

Frontend tests must explicitly cover:

- `eval`, `Function`, `Reflect`, `Proxy`, prototype access;
- optional/computed properties;
- imports, exports, dynamic import;
- async, promises, timers, generators;
- classes, constructors, exceptions;
- every loop form;
- destructuring, spread, rest/default parameters;
- `reduce`, `flatMap`, sort;
- `.call`, `.apply`, `.bind`;
- unknown globals;
- direct and indirect recursion;
- helper capture of `input`, `rollshot`, and outer state;
- unbounded capability query limits;
- static AST, literal, helper-depth, traversal, candidate, and output limits.

The authoritative `language_schema_v1_denylist_is_complete` table now lives in
Task 5 (co-located with the validator it drives). In this task, **do not
re-introduce it** — instead confirm it still passes after the runtime work, and
add any frontend adversarial case from the bullet list above that the Task 5
table does not yet cover (each new case must assert an exact `DiagnosticCode`).
If you find an uncovered case, add it to the Task 5 table, not here, so the
denylist stays in one place.

Runtime tests must explicitly cover:

- all five stripped globals;
- absent network/platform/DOM globals;
- dynamic module resolution failure with no loader;
- prototype and global isolation across fresh executions;
- infinite work, stack depth, memory pressure;
- pre-cancellation and in-flight cancellation;
- host callback error;
- host-side allocation limit;
- global/per-capability call limits;
- malformed and amplified output.

Use these fixed test names so the coverage can be audited from test output:

```text
dangerous_base_globals_are_stripped_and_verified
ambient_platform_globals_are_absent
dynamic_import_does_not_resolve_external_module
fresh_execution_does_not_observe_prior_global_state
interrupt_stops_infinite_runtime_payload
memory_limit_stops_runtime_allocation
stack_limit_stops_runtime_recursion
pre_cancelled_execution_never_runs
in_flight_cancellation_interrupts_execution
capability_error_remains_typed
host_allocation_limit_rejects_large_capability_result
global_capability_call_limit_is_enforced
per_capability_call_limit_is_enforced
output_byte_limit_is_enforced_before_decoding
```

Most of these names already have bodies (Tasks 10–11). Four do **not** and must
be written here — they are named above but were never implemented, and two are
the most security-relevant tests in the suite (in-flight cancellation is the
carry-forward host-call-preemption risk from the spike; the call-count limits
guard against capability-call amplification). Implement them concretely:

```rust
#[test]
fn dynamic_import_does_not_resolve_external_module() {
    // No loader is ever registered (Task 10 forbids set_loader), so import()
    // must fail at runtime rather than resolve a module. The frontend already
    // rejects import() statically; this is the defense-in-depth runtime proof.
    let result = QuickJsExecutor.execute(
        &runtime_payload("function main(input) { return import('fs'); }"),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert!(matches!(result, Err(ExecutionError::Sandbox(_))));
}

#[test]
fn in_flight_cancellation_interrupts_execution() {
    // Cancel from another thread while a long-running payload is executing.
    // The interrupt handler observes the flag at an opcode boundary and aborts;
    // the post-closure cancellation check maps it to Cancelled.
    let cancellation = CancellationFlag::new();
    let canceller = cancellation.clone();
    let handle = std::thread::spawn(move || {
        // Busy-wait briefly so execution is in-flight, then cancel. Deterministic
        // because the payload cannot terminate on its own.
        for _ in 0..1_000_000 { std::hint::spin_loop(); }
        canceller.cancel();
    });
    let mut long = policy();
    long.max_wall_time = Duration::from_secs(5); // long enough that cancel wins
    let result = QuickJsExecutor.execute(
        &runtime_payload("function main(input) { while (true) {} }"),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &long,
        &cancellation,
    );
    handle.join().unwrap();
    assert_eq!(result, Err(ExecutionError::Cancelled));
}

#[test]
fn global_capability_call_limit_is_enforced() {
    // main() calls ocr in a bounded .map; cap the global budget below the count.
    let source = r#"
function main(input) {
  const a = rollshot.ocr({ region: input.region, limit: 1 });
  const b = rollshot.ocr({ region: input.region, limit: 1 });
  const c = rollshot.ocr({ region: input.region, limit: 1 });
  return { candidates: [] };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let mut policy = policy();
    policy.max_capability_calls = 2;
    let result = QuickJsExecutor.execute(
        &automation, &input(), &context(),
        &mut FakeAutomationHost::default(), &policy, &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Capability(CapabilityError::LimitExceeded))
    );
}

#[test]
fn per_capability_call_limit_is_enforced() {
    // Global budget is generous; the per-capability cap for Ocr is the binding
    // constraint. Proves the BTreeMap-keyed limit and the "present key" semantics.
    let source = r#"
function main(input) {
  const a = rollshot.ocr({ region: input.region, limit: 1 });
  const b = rollshot.ocr({ region: input.region, limit: 1 });
  return { candidates: [] };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let mut policy = policy();
    policy.max_capability_calls = 16;
    policy
        .max_calls_by_capability
        .insert(rollshot_automation::CapabilityName::Ocr, 1);
    let result = QuickJsExecutor.execute(
        &automation, &input(), &context(),
        &mut FakeAutomationHost::default(), &policy, &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Capability(CapabilityError::LimitExceeded))
    );
}
```

If a chosen interrupt/cancellation timing proves flaky on CI, widen the margins
(more spin iterations, longer `max_wall_time`) rather than weakening the
assertion — `in_flight_cancellation_interrupts_execution` must assert exactly
`ExecutionError::Cancelled`, not a looser `matches!`.

Output tests must explicitly cover:

- all eight edit kinds;
- unknown envelope, candidate, geometry, and operation fields;
- missing required fields;
- confidence outside `[0, 1]`;
- empty/oversized label and oversized rationale/text;
- NaN/infinity injected through direct decoder unit values where JSON cannot encode them;
- zero-area and out-of-bounds rectangles;
- candidate count and total redaction area;
- every canonical/noncanonical `AnnotationId` boundary.

Add this malformed-output table in addition to the full-CRUD success test:

```rust
#[test]
fn strict_output_schema_rejects_invalid_shapes() {
    let invalid = [
        r#"{"candidates":[],"extra":true}"#,
        r#"{"candidates":[{"kind":"unknown","confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1,"height":1,"extra":1},"confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1,"height":1},"confidence":2,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":0,"height":1},"confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1,"height":1},"confidence":1,"label":""}]}"#,
        r#"{"candidates":[{"kind":"delete","annotationId":"01","confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"updateText","annotationId":"42","confidence":1,"label":"x"}]}"#,
    ];
    for json in invalid {
        assert!(decode_proposal(json, (100, 100), &context(), &allow_all()).is_err());
    }
}
```

Add these separate tests:

```text
rejects_label_over_128_bytes
rejects_rationale_over_2048_bytes
rejects_text_over_the_configured_output_string_limit
rejects_out_of_bounds_redaction_when_policy_disallows_it
rejects_candidate_count_over_policy_limit
rejects_total_redaction_area_over_policy_limit
rejects_nan_point_in_private_wire_conversion_test
rejects_infinite_rect_in_private_wire_conversion_test
accepts_annotation_id_zero
accepts_annotation_id_u64_max
```

Each rejection test asserts the exact `OutputError` or wrapped `PolicyError`
variant, not only `is_err()`.

- [ ] **Step 3: Run the two crate suites**

Run:

```bash
rtk cargo test -p rollshot-automation
rtk cargo test -p rollshot-automation-rquickjs
```

Expected: PASS with no ignored tests.

- [ ] **Step 4: Confirm no test-only parser/runtime alternatives entered production**

Run:

```bash
rtk cargo tree -p rollshot-automation
rtk cargo tree -p rollshot-automation-rquickjs
```

Expected:

- frontend tree contains oxc `0.137.0` and no tree-sitter, swc, or boa;
- runtime tree contains rquickjs `0.12.0`;
- neither crate contains Rig;
- no bindgen package appears in the rquickjs default-feature tree.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-automation crates/rollshot-automation-rquickjs
rtk git commit -m "test(automation): cover language and sandbox contracts"
```

---

### Task 13: Write the handoff, update the parent phase, and verify the workspace

**Files:**
- Create: `docs/superpowers/handoffs/2026-06-21-automation-frontend-runtime.md`
- Modify: `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`

- [ ] **Step 1: Write the completion handoff**

Create the handoff with these concrete sections:

```markdown
# Automation Frontend and Runtime Handoff

**Completed subproject:** Parent §12, Subproject 3
**Next subproject:** Subproject 4 — Bounded Agent Core

## Delivered crates

- `rollshot-automation`
- `rollshot-automation-rquickjs`

## Locked dependencies

- oxc `=0.137.0`
- rquickjs `=0.12.0`

## Installed schema versions

- Language Schema: 1
- Workflow IR Schema: 1
- Capability API: 1
- Output Schema: 1

## Public integration sequence

1. Call `validate_source(source, validation_limits)`.
2. Present `workflow_ir`, `semantic_summary`, and `semantic_diff`.
3. Dry-run with `execute_to_proposal`.
4. Keep the returned `EditProposal` transient until human review.
5. Apply only reviewed operations through `rollshot_edit_proposal::lower` and `ImageDocument::apply_batch`.

## Subproject 4 handoff

The bounded agent core owns `replace_automation_source`, `validate_automation`,
`dry_run_automation`, and `submit_for_review`. It supplies `ProposalContext`,
`ExecutionPolicy`, cancellation, and a prepared `AutomationHost`; it must not
expose rquickjs types.

## Subproject 5 handoff

Persist canonical source, Workflow IR, all four schema versions, capability
manifest, static cost, validation summary, and immutable revision provenance.
Do not persist oxc ASTs, runtime contexts, raw OCR, or raw host results.

## Subproject 6 handoff

Add default feature `smart-redaction`; hide every related UI/dependency when
disabled. Render source diagnostics, semantic summary/diff, capability changes,
static costs, execution metrics, and proposal candidates.

## Deferred real capability adapters

Real OCR, layout, region-feature, and template-match implementations remain
unimplemented. Production adapters must prepare bounded results outside the
QuickJS callback and keep host callbacks below 1 ms.

## Remaining risks

- QuickJS interrupts do not pre-empt a blocking Rust callback.
- QuickJS memory limits exclude Rust allocations; host allocation limits are separate.
- Parser/runtime upgrades require all frontend and adversarial suites.

## Verification evidence

Record the exact commands, platform/compiler, test counts, and outcomes from
the final verification step below.
```

Replace the final verification-evidence prose with actual command outputs and test counts. Do not claim macOS runtime verification unless it was run on macOS; record macOS compile evidence separately.

- [ ] **Step 2: Mark parent subproject 3 implemented**

Under Delivery Decomposition item 3, add:

```markdown
   - **Implemented:** `docs/superpowers/handoffs/2026-06-21-automation-frontend-runtime.md`
```

Under item 4, add:

```markdown
   - **Next phase after subproject 3.**
```

Do not rewrite any other historical design content.

- [ ] **Step 3: Run crate-level verification**

Run:

```bash
rtk cargo test -p rollshot-edit-proposal
rtk cargo test -p rollshot-automation
rtk cargo test -p rollshot-automation-rquickjs
```

Expected: PASS.

- [ ] **Step 4: Run workspace formatting and lint verification**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test
```

Expected: PASS.

- [ ] **Step 5: Verify exact pins and app feature isolation**

Run:

```bash
rtk cargo tree -p rollshot-automation
rtk cargo tree -p rollshot-automation-rquickjs
rtk cargo tree -p rollshot-app --no-default-features
```

Expected:

- oxc resolves exactly to `0.137.0`;
- rquickjs resolves exactly to `0.12.0`;
- disabled app tree contains neither automation crate, oxc, rquickjs, nor Rig.

- [ ] **Step 6: Run macOS compile verification**

On a macOS runner:

```bash
rtk cargo test -p rollshot-automation --no-run
rtk cargo test -p rollshot-automation-rquickjs --no-run
```

Expected: PASS on the supported macOS target. Record runner OS, architecture, rustc version, and commit in the handoff.

- [ ] **Step 7: Update handoff evidence and commit**

```bash
rtk git add docs/superpowers/handoffs/2026-06-21-automation-frontend-runtime.md docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md
rtk git commit -m "docs(automation): hand off frontend and runtime"
```

- [ ] **Step 8: Verify the final branch state**

Run:

```bash
rtk git status --short --branch
rtk git log --oneline main..HEAD
```

Expected: clean worktree and a sequence of focused conventional commits matching the tasks above.
