# Launch Teaser Agent and Repository Authority Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bundled launch-teaser skill that can return a strict review proposal and optionally read explicitly authorized project text through a bounded, auditable auxiliary tool.

**Architecture:** Generalize the existing bounded single-submit runner to support nonterminal auxiliary tools before one terminal submission. A descriptor-relative repository reader owns a per-run grant, enforces denylist and byte/file ceilings, and records privacy-safe receipts. The launch-teaser skill returns a strict provider-neutral patch DTO; product code remains responsible for mapping it onto and validating `rollshot-action` plans.

**Tech Stack:** Rust, existing provider-neutral agent facade, Rig turn state machine, static skills catalog, `ToolRegistry`, immutable authority snapshots, rustix descriptor-relative file access, serde/schemars, SHA-256, durable product-task and audit contracts.

## Global Constraints

- Repository access is optional and requires a new explicit grant for each run.
- One canonical workspace root and explicit relative file/directory entries define the grant.
- Symlinks, traversal, special files, paths outside the root, binary files, sensitive names, and VCS internals are rejected.
- Limits are fixed: at most 64 files, 64 KiB read per file, 512 KiB total read, and 256 KiB total returned text.
- Allowed extensions are `md`, `txt`, `rs`, `toml`, `json`, `yaml`, `yml`, `js`, `jsx`, `ts`, `tsx`, `css`, `html`, `swift`, `m`, `mm`, `c`, `cc`, `cpp`, `h`, `hpp`, `go`, `py`, and `java`.
- Deny components include `.git`, `.hg`, `.svn`, `.env`, `.ssh`, `secrets`, `credentials`, and case-insensitive names ending in `.key`, `.pem`, `.p12`, or `.pfx`.
- Absolute paths never enter model input, tool results, shareable artifacts, receipts, Debug output, tracing fields, or user-visible errors.
- The auxiliary reader can read only; it cannot write, execute, use a shell, access the network, or launch a process.
- The terminal launch-teaser tool remains the only successful terminal action.
- Agent output is a proposal; it never mutates an Action Guide plan or renders.
- Existing Smart Redaction, caption, and visual-annotation behavior and receipts remain compatible.
- Prefix every shell command with `rtk`.

---

### Task 1: Auxiliary tools in the bounded single-submit runner

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/src/tools.rs`
- Modify: `crates/rollshot-agent/src/authority.rs`

**Interfaces:**
- Consumes: existing `SingleSubmitProfile`, `Tool`, `ToolRegistry`, authority, budget, cancellation, and audit sink.
- Produces:
  - `SingleSubmitAuxiliaryTool { definition, tool }`
  - `SingleSubmitProfile::with_auxiliary_tools(Vec<SingleSubmitAuxiliaryTool>) -> Result<Self, DriverError>`
  - existing `AgentRunner::run_single_submit_with_provider(&self, profile: SingleSubmitProfile<'_>, input: AuthorizedModelInput, provider: &dyn ProviderAdapter, budget: RunBudget, cancellation: &RunCancellation, authority: &AuthoritySnapshot, subject: &AuthoritySubject, audit_sink: Option<&dyn AuditAppendSink>) -> SingleSubmitTerminal` terminal contract.

- [ ] **Step 1: Write failing auxiliary-tool lifecycle tests**

Add driver tests using a counting read tool and terminal submit tool:

```rust
#[tokio::test]
async fn auxiliary_success_threads_result_then_terminal_submits() {
    let reader = counting_auxiliary_tool(RunOperation::ReadAuthorizedWorkspaceFile);
    let profile = caption_profile(skill_use()).with_auxiliary_tools(vec![reader.profile()]).unwrap();
    let provider = auxiliary_then_terminal_provider();
    let terminal = runner().run_single_submit_with_provider(
        profile, input(), &provider, budget(), &RunCancellation::new(),
        &authority_with_read_and_submit(), &subject(), Some(&audit_sink()),
    ).await;
    assert!(matches!(terminal, SingleSubmitTerminal::Submitted { .. }));
    assert_eq!(reader.call_count(), 1);
}
```

Also test unknown tool, terminal mixed with auxiliary calls, terminal called twice, missing auxiliary grant, denial audit failure, multiple auxiliary calls in one batch, auxiliary recoverable result, auxiliary hard error, cancellation before and after an auxiliary call, argument/result/tool-call budget charging, duplicate tool names, and existing terminal-only caption behavior.

- [ ] **Step 2: Run focused driver tests and observe failure**

Run: `rtk cargo test -p rollshot-agent driver::tests::single_submit -- --nocapture`
Expected: FAIL because auxiliary profiles and `ReadAuthorizedWorkspaceFile` do not exist.

- [ ] **Step 3: Add the operation and auxiliary profile type**

Add `RunOperation::ReadAuthorizedWorkspaceFile` without changing existing serialized variant names.

Define:

```rust
pub struct SingleSubmitAuxiliaryTool {
    pub definition: crate::model::ToolDefinition,
    pub tool: std::sync::Arc<dyn crate::tools::Tool>,
}
```

`with_auxiliary_tools` rejects duplicate names and any auxiliary name equal to the terminal name. It verifies `definition.name == tool.name()` and preserves the terminal skill/digest constructor invariant.

- [ ] **Step 4: Generalize the tool loop without weakening terminal semantics**

Build one registry and definition list from auxiliary tools followed by the terminal tool. In `CallTools`:

- reject unknown names;
- reject a batch containing the terminal tool plus any other call;
- authorize every auxiliary tool operation against the supplied Action Guide `subject` before its body runs;
- record any denial through the existing audit path;
- execute auxiliary calls serially, thread their results back into Rig, apply budget charges, and continue the model loop;
- execute the terminal tool only when it is the sole call, then return `Submitted` exactly as today.

Do not use `ToolContext` for the Action Guide subject; the single-submit runner already receives the correct `AuthoritySubject` explicitly.

- [ ] **Step 5: Run driver and existing provider-contract tests**

Run:

```bash
rtk cargo test -p rollshot-agent driver::tests::single_submit -- --nocapture
rtk cargo test -p rollshot-agent provider_contract -- --nocapture
```

Expected: all tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/tools.rs crates/rollshot-agent/src/authority.rs
rtk git commit -m "feat(agent): allow bounded auxiliary submit tools"
```

---

### Task 2: Descriptor-relative repository grant and reader

**Files:**
- Create: `crates/rollshot-agent/src/repository.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`
- Modify: `crates/rollshot-agent/Cargo.toml`

**Interfaces:**
- Produces:
  - `RepositoryReadLimits::v1()`
  - `RepositoryReadGrant::open(root: &Path, entries: Vec<String>, limits: RepositoryReadLimits) -> Result<Self, RepositoryReadError>`
  - `RepositoryReadGrant::receipt() -> RepositoryReadGrantReceiptV1`
  - `RepositoryReadTool::new(grant: RepositoryReadGrant, cancellation: RunCancellation) -> RepositoryReadToolHandle`
  - `RepositoryReadToolHandle::tool() -> Arc<dyn Tool>`
  - `RepositoryReadToolHandle::receipts() -> Vec<RepositoryReadReceiptV1>`.

- [ ] **Step 1: Write failing filesystem-boundary tests**

Use temp roots and Unix symlinks. Cover exact files, recursive directories, root escape, `..`, absolute entries, symlink file, symlink directory, FIFO/special file, denied components, denied suffixes, binary NUL content, unsupported extensions, file limit, per-file limit, total-read limit, total-return limit, truncation, cancellation, and concurrent receipt ordering.

```rust
#[tokio::test]
async fn symlink_inside_grant_cannot_escape_root() {
    let fixture = repository_fixture();
    std::os::unix::fs::symlink(fixture.outside.path(), fixture.root.join("docs/link")).unwrap();
    let grant = RepositoryReadGrant::open(&fixture.root, vec!["docs".into()], RepositoryReadLimits::v1()).unwrap();
    let handle = RepositoryReadTool::new(grant, RunCancellation::new());
    let outcome = handle.tool().call(&serde_json::json!({"path":"docs/link/secret.txt"})).await.unwrap();
    assert!(matches!(outcome, ToolOutcome::Recoverable { .. }));
    assert!(handle.receipts().is_empty());
}
```

Assert all errors, Debug output, result JSON, receipts, and tracing-capture fixtures omit the absolute temp root.

- [ ] **Step 2: Run repository tests and observe failure**

Run: `rtk cargo test -p rollshot-agent repository::tests -- --nocapture`
Expected: FAIL because the module does not exist.

- [ ] **Step 3: Implement validated grant identities**

Define strict DTOs:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryReadGrantReceiptV1 {
    pub schema_version: u32,
    pub root_identity_sha256: String,
    pub grant_sha256: String,
    pub entries: Vec<String>,
    pub limits: RepositoryReadLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryReadReceiptV1 {
    pub relative_path: String,
    pub content_sha256: String,
    pub bytes_read: u64,
    pub bytes_returned: u64,
    pub truncated: bool,
}
```

Validate each relative entry as normalized UTF-8 path components with no empty, dot, parent, root, or platform-prefix component. Sort and deduplicate entries before hashing. Hash canonical root identity and canonical grant bytes with separate domain separators. Keep the actual root path private and redact it from `Debug`.

- [ ] **Step 4: Implement no-follow descriptor-relative traversal**

Follow the `skills::HostSkillRoot` rustix pattern: open the root directory once, traverse each component with no-follow directory descriptors, reject non-regular targets, and read through the final descriptor with ceilings. Do not canonicalize a candidate path and then reopen by absolute pathname.

Directory grants permit descendants only after every component passes the denylist. Read cancellation and aggregate counters before opening, during bounded reads, and before returning content.

- [ ] **Step 5: Implement the typed tool and receipt collector**

Tool schema:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadAuthorizedProjectTextArgs {
    path: String,
}
```

Tool result JSON contains `path`, `content`, `content_sha256`, `bytes_read`, `bytes_returned`, and `truncated`. `required_operations()` returns only `ReadAuthorizedWorkspaceFile`. Append a receipt only after a successful or explicitly truncated read; rejected reads produce no receipt.

- [ ] **Step 6: Run repository tests and full skills security tests**

Run:

```bash
rtk cargo test -p rollshot-agent repository::tests -- --nocapture
rtk cargo test -p rollshot-agent skills -- --nocapture
```

Expected: all tests PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-agent/Cargo.toml crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/repository.rs
rtk git commit -m "feat(agent): add bounded repository reader"
```

---

### Task 3: Launch-teaser patch schema and terminal tool

**Files:**
- Create: `crates/rollshot-agent/src/launch_teaser.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Interfaces:**
- Produces:
  - `LaunchTeaserPatchV1`
  - `parse_launch_teaser_patch(&Value) -> Result<LaunchTeaserPatchV1, LaunchTeaserPatchError>`
  - `launch_teaser_submit_definition() -> ToolDefinition`
  - `launch_teaser_run_budget() -> RunBudget`.

- [ ] **Step 1: Write failing strict-decoder tests**

Test an empty patch, hook/outro changes, shot order, per-shot range/focus/speed/caption/transition changes, duplicate step IDs, missing order members, unknown fields, floats, negative integers, oversized text, unsupported speed, unsupported transition, and more than five shots.

```rust
#[test]
fn arbitrary_filtergraph_is_rejected() {
    let value = serde_json::json!({
        "hook": null,
        "outro_text": null,
        "shot_order": [1, 2, 3],
        "shots": [{"reviewed_step_id": 1, "filtergraph": "movie=/etc/passwd"}]
    });
    assert!(parse_launch_teaser_patch(&value).is_err());
}
```

- [ ] **Step 2: Run patch tests and observe failure**

Run: `rtk cargo test -p rollshot-agent launch_teaser::tests -- --nocapture`
Expected: FAIL because patch APIs do not exist.

- [ ] **Step 3: Implement provider-neutral DTOs**

Define DTOs using primitives only; `rollshot-agent` must not depend on `rollshot-action`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserPatchV1 {
    pub hook: Option<String>,
    pub outro_text: Option<String>,
    pub shot_order: Vec<u64>,
    pub shots: Vec<LaunchTeaserShotPatchV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserShotPatchV1 {
    pub reviewed_step_id: u64,
    pub source_start_ms: Option<u64>,
    pub source_end_ms: Option<u64>,
    pub focus_start_x: Option<u16>,
    pub focus_start_y: Option<u16>,
    pub focus_end_x: Option<u16>,
    pub focus_end_y: Option<u16>,
    pub zoom_permille: Option<u16>,
    pub speed_permille: Option<u16>,
    pub caption: Option<String>,
    pub transition: Option<LaunchTeaserTransitionPatchV1>,
}
```

Validate the same numeric and text ceilings as the domain plan. Require `shot_order` to contain 3–5 unique IDs and `shots` to contain at most one patch per ordered ID. Product mapping performs final source/project/duration validation.

- [ ] **Step 4: Implement terminal definition and bounded budget**

Create `submit_launch_teaser_plan` with the schemars-generated strict schema. Use the caption/visual single-submit budgets as the pattern but allow repository auxiliary calls within fixed tool-call, argument-byte, result-byte, model-call, attachment, token, and wall-time ceilings. No budget dimension is left permissive.

- [ ] **Step 5: Run patch tests**

Run: `rtk cargo test -p rollshot-agent launch_teaser::tests -- --nocapture`
Expected: all patch tests PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/launch_teaser.rs
rtk git commit -m "feat(agent): define launch teaser proposals"
```

---

### Task 4: Bundled launch-teaser skill and profile

**Files:**
- Create: `crates/rollshot-agent/skills/action-guide-launch-teaser/skill.toml`
- Create: `crates/rollshot-agent/skills/action-guide-launch-teaser/SKILL.md`
- Modify: `crates/rollshot-agent/src/skills.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`

**Interfaces:**
- Produces:
  - `ACTION_GUIDE_LAUNCH_TEASER_PACKAGE_ID`
  - `bundled_action_guide_launch_teaser_use() -> Option<SkillUse>`
  - `compose_launch_teaser_prompt(&SkillUse) -> Result<String, DriverError>`
  - `launch_teaser_profile(&SkillUse, Vec<SingleSubmitAuxiliaryTool>) -> Result<SingleSubmitProfile<'_>, DriverError>`.

- [ ] **Step 1: Write failing catalog and prompt tests**

Assert the package appears in metadata discovery, resolves by authority/package ID, has a stable digest receipt, is rejected on authority/package mismatch, and produces a prompt containing the exact digest and fixed safety envelope.

- [ ] **Step 2: Run skill tests and observe failure**

Run:

```bash
rtk cargo test -p rollshot-agent skills::tests -- --nocapture
rtk cargo test -p rollshot-agent driver::tests::launch_teaser -- --nocapture
```
Expected: FAIL because the package/profile do not exist.

- [ ] **Step 3: Add the strict manifest**

`skill.toml`:

```toml
schema_version = 1
package_id = "action-guide-launch-teaser"
name = "Action Guide Launch Teaser"
description = "Propose bounded launch teaser shot and copy edits from reviewed Action Guide evidence."
declared_version = "1.0.0"
main = "SKILL.md"
```

- [ ] **Step 4: Write bounded skill instructions**

`SKILL.md` must direct the model to:

- treat reviewed steps and motion ranges as the only footage authority;
- use repository reads only for terminology and supported copy;
- never claim a read occurred unless the tool returned it;
- keep 3–5 reviewed step IDs;
- return only through `submit_launch_teaser_plan`;
- avoid private data, unsupported claims, arbitrary code, paths, commands, and render instructions;
- prefer no change over an unsupported suggestion.

- [ ] **Step 5: Bundle and compose the profile**

Add the package to `BUNDLED_REPORT`, a resolver matching existing bundled skill helpers, and a fixed system envelope. The profile uses Task 3 terminal definition/tool and Task 1 auxiliary tools. It requires `SubmitReviewCandidate`; each repository auxiliary tool separately requires `ReadAuthorizedWorkspaceFile`.

- [ ] **Step 6: Run skill and driver tests**

Run:

```bash
rtk cargo test -p rollshot-agent skills -- --nocapture
rtk cargo test -p rollshot-agent driver::tests::launch_teaser -- --nocapture
```

Expected: all tests PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-agent/skills/action-guide-launch-teaser crates/rollshot-agent/src/skills.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): bundle launch teaser skill"
```

---

### Task 5: Durable launch-teaser task and artifact contracts

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs`
- Modify: `crates/rollshot-agent/src/continuity.rs`
- Modify: `crates/rollshot-agent/src/audit.rs`

**Interfaces:**
- Produces:
  - `TaskKind::ActionGuideLaunchTeaser`
  - `ArtifactKind::ActionGuideLaunchTeaser`
  - `SourceBinding::ActionGuideLaunchTeaserProject { project_root_sha256, revision, projection_digest, motion_sha256 }`
  - `ArtifactSummary::ActionGuideLaunchTeaser { changed_field_count, repository_read_count }`
  - backward-compatible `RunContractReceiptV1 { authority, skill_use, bound_at_unix_ms, repository_grant }`.

- [ ] **Step 1: Write failing identity, freshness, receipt, and privacy tests**

Test exact serde names, root identity matching, revision/projection/motion freshness, non-aliasing with captions/visual annotations, summary counts, continuity digest changes, and Debug/JSON privacy.

```rust
#[test]
fn teaser_motion_change_is_same_identity_but_stale() {
    let base = teaser_binding([1; 32], 7, "aa", "bb");
    let changed = teaser_binding([1; 32], 7, "aa", "cc");
    assert!(base.identity_matches(&changed));
    assert!(!base.freshness_matches(&changed));
}
```

- [ ] **Step 2: Run product-task tests and observe failure**

Run:

```bash
rtk cargo test -p rollshot-agent product_task::tests -- --nocapture
rtk cargo test -p rollshot-agent continuity::tests -- --nocapture
```
Expected: FAIL because teaser variants do not exist.

- [ ] **Step 3: Add variants and compatibility decoding**

Extend tagged enums and their compatibility deserializers without changing existing variant serialization. Identity is project-root digest; freshness is revision + projection + motion digest. Update exhaustive matches in continuity and audit metadata.

Add `repository_grant: Option<RepositoryReadGrantReceiptV1>` to `RunContractReceiptV1` with `#[serde(default, skip_serializing_if = "Option::is_none")]`. Existing constructors and current workloads set `None`, preserving their JSON. Add a constructor used by launch-teaser runs that sets `Some(grant.receipt())`; durable task attempts and product artifact metadata continue using the single established run-contract field.

- [ ] **Step 4: Run durable-contract tests**

Run:

```bash
rtk cargo test -p rollshot-agent product_task::tests -- --nocapture
rtk cargo test -p rollshot-agent continuity::tests -- --nocapture
rtk cargo test -p rollshot-agent audit::tests -- --nocapture
```

Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-agent/src/product_task.rs crates/rollshot-agent/src/continuity.rs crates/rollshot-agent/src/audit.rs
rtk git commit -m "feat(agent): persist launch teaser proposals"
```

---

### Task 6: End-to-end bounded skill contract

**Files:**
- Create: `crates/rollshot-agent/tests/launch_teaser_contract.rs`
- Modify: `crates/rollshot-agent/src/launch_teaser.rs`
- Modify: `crates/rollshot-agent/src/repository.rs`

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces: independently verified provider-neutral launch-teaser proposal flow for the product plan.

- [ ] **Step 1: Add a scripted-provider acceptance test**

Create an authorized temp repository containing allowed terminology plus denied secrets. The scripted provider first calls `read_authorized_project_text`, then submits a strict patch. Assert the terminal patch, grant receipt, exact read receipt, skill receipt, cancellation behavior, and absence of forbidden content/path data.

```rust
#[tokio::test]
async fn authorized_repository_read_then_review_submission() {
    let fixture = contract_fixture();
    let terminal = fixture.run().await;
    assert!(matches!(terminal, SingleSubmitTerminal::Submitted { .. }));
    let receipts = fixture.reader.receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].relative_path, "README.md");
    assert!(!fixture.audit_text().contains(fixture.root_string()));
}
```

Add a second acceptance test with no repository grant: the model sees no read tool and still submits a valid Action Guide-only patch.

- [ ] **Step 2: Run the acceptance test**

Run: `rtk cargo test -p rollshot-agent --test launch_teaser_contract -- --nocapture`
Expected: PASS.

- [ ] **Step 3: Run agent verification**

Run:

```bash
rtk cargo fmt --check
rtk cargo test -p rollshot-agent
rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings
```

Expected: all commands PASS.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-agent/src crates/rollshot-agent/tests/launch_teaser_contract.rs
rtk git commit -m "test(agent): cover bounded launch teaser skill"
```
