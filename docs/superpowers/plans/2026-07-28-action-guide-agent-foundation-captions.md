# Action Guide Caption Provenance Implementation Plan (Slice A)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put the Action Guide caption suggestion flow on the shared agent
foundation — durable task identity, immutable authority, a bundled skill, typed
artifact promotion, review receipts, restore, and durable audit — and carry the
contract surgery that makes a non-Smart-Redaction workload expressible.

**Architecture:** The shared contracts in `rollshot-agent` become domain-tagged
where they currently hardcode Smart Redaction shapes (`SourceBinding`,
`AuthoritySubject`, artifact summary, artifact payload). The app-side task store
moves out of the Smart Redaction UI module and becomes a single per-process
instance shared by both workspaces. The caption run uses a bounded single-submit
profile extracted from the visual annotation run shape, extended to thread an
authority snapshot, a skill use, and an audit sink.

**Tech Stack:** Rust, `rollshot-agent`, `rollshot-action`, `rollshot-app`, iced
0.14, `rig-core =0.40.0`, `fs4`, `serde`, `sha2`, `tokio`.

**Governing documents:**

- Spec: `docs/superpowers/specs/2026-07-28-action-guide-agent-foundation-captions-design.md`
- Umbrella: `docs/superpowers/specs/2026-07-28-action-guide-agent-foundation-umbrella-design.md`
  (plan boundary §13.3, Gate A1 §13.4)
- Amendments: `docs/superpowers/spikes/2026-07-28-action-guide-authority-binding-amendment-decision.md`

## Global Constraints

- Prefix every shell command with `rtk` (AGENTS.md §6).
- All runtime diagnostics use `tracing` with stable explicit `rollshot::*`
  targets. No `println!`, `eprintln!`, or `dbg!` (AGENTS.md §7).
- The workspace forbids `unsafe_code`. Do not add any.
- `rollshot-agent` must NOT gain a dependency on `rollshot-action`. `rollshot-app`
  is the only translator between them.
- Every task must leave `rtk cargo test -p rollshot-agent` and
  `rtk cargo test -p rollshot-app --features action-guide` green before commit.
- `rollshot-app` must also compile and test with the `action-guide` feature
  OFF. The moved store is unconditional; only Action Guide task-kind
  construction sites are feature-gated.
- Exactly one `TaskStore` instance per process. `acquire_lock` takes a blocking
  fs4 exclusive lock per operation; two instances in one process hold distinct
  file descriptors that flock treats as unrelated holders.
- No new UI surface, widget, or affordance. These exact user-visible strings must
  survive verbatim:
  - `"Caption suggestions timed out."`
  - `"Suggesting captions..."`
  - `"Configure an agent provider before suggesting captions."`
  - `"Caption suggestions failed: {error}"`
- Do not modify `run_visual_annotation_with_provider` or any visual annotation
  behavior. That is Slice B's work.
- Do not improve the caption instruction text. Task 13 moves it verbatim.

---

## Task 1: Golden baseline for today's caption prompt and failure copy

Locks the exact instruction text and user-visible strings before anything moves,
so later tasks prove preservation instead of asserting it.

**Files:**
- Test: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs` (add to the
  existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `CAPTION_INSTRUCTION_BASELINE` — a `const &str` inside this crate's
    `#[cfg(test)] mod tests`. **It is not reachable from `rollshot-agent`**
    (different crate, `#[cfg(test)]` module), so Task 13 declares its own copy
    of the same literal in `skills.rs`'s test module. Two crates, two copies,
    one literal. After Task 13, `SKILL.md` is the single durable source; both
    constants exist only to prove the move preserved it byte for byte.
  - `TIMEOUT_MESSAGE` — a `pub(crate) const &str` in `caption_agent.rs`
    (production scope, not test scope), consumed by Task 16's terminal mapping.

**Verified against code on review (2026-07-28):** the baseline literal below is
byte-identical to the `format!` template at
`crates/rollshot-app/src/timeline_workspace/caption_agent.rs:112`-115, whose
`\`-continuations join the three sentences with `\n` and append `\nSteps: {json}`
with no trailing newline. `split_once("\nSteps: ")` therefore yields exactly the
baseline.

- [ ] **Step 1: Write the failing test**

Add to the existing `mod tests` in
`crates/rollshot-app/src/timeline_workspace/caption_agent.rs` (the module at
`caption_agent.rs:361`, which already has `use super::*`):

```rust
    /// Today's exact static instruction text, captured before the skill move.
    /// Task 13 asserts the bundled SKILL.md body equals this byte for byte.
    pub(crate) const CAPTION_INSTRUCTION_BASELINE: &str = "Suggest concise Action Guide titles and one-sentence captions for these reviewed workflow steps.\nPrefer calling the submit_caption_suggestions tool. If tool calling is unavailable, return only JSON in the same schema.\nUse the source values exactly. Omit a title by using null when the current title is already good. Do not invent raw typed text.";

    #[test]
    fn prompt_baseline_is_instruction_text_then_steps() {
        let prompt = build_caption_prompt(&steps());

        let (instructions, tail) = prompt
            .split_once("\nSteps: ")
            .expect("prompt must end with a Steps: section");

        assert_eq!(
            instructions, CAPTION_INSTRUCTION_BASELINE,
            "instruction text drifted from the recorded baseline"
        );
        assert!(
            tail.starts_with('['),
            "steps payload must be a JSON array, got {tail}"
        );
    }
```

- [ ] **Step 2: Run the test to verify it passes or reveals drift**

Run: `rtk cargo test -p rollshot-app --features action-guide caption_agent::tests::prompt_baseline`
Expected: PASS. If it FAILS, the instruction text has drifted from what the spec
recorded on 2026-07-28. Stop, record the actual text in the plan and the spec,
and use the actual text as the baseline.

- [ ] **Step 3: Add the failure-copy baseline test**

Same `mod tests` module.

```rust
    #[test]
    fn timeout_copy_baseline() {
        // Task 16 replaces the timeout with a RunBudget wall_time dimension and
        // must map it back to this exact string.
        assert_eq!(
            super::TIMEOUT_MESSAGE,
            "Caption suggestions timed out.",
            "user-visible timeout copy must not change"
        );
    }
```

- [ ] **Step 3a: Run it to verify it fails**

Run: `rtk cargo test -p rollshot-app --features action-guide timeout_copy_baseline`
Expected: FAIL to compile — `cannot find value TIMEOUT_MESSAGE in the crate root`.

- [ ] **Step 4: Extract the literal into a named constant**

In `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`, above
`suggest_captions_task`:

```rust
/// User-visible copy for a caption run that ran out of time. Preserved verbatim
/// across the RunBudget migration (plan Task 16).
pub(crate) const TIMEOUT_MESSAGE: &str = "Caption suggestions timed out.";
```

Replace both existing occurrences of the literal
`"Caption suggestions timed out.".to_string()` in
`suggest_captions_with_timeout` with `TIMEOUT_MESSAGE.to_string()`.

- [ ] **Step 5: Run the tests**

Run: `rtk cargo test -p rollshot-app --features action-guide caption_agent`
Expected: PASS, including the pre-existing
`runner_times_out_quickly_in_tests`.

- [ ] **Step 5a: Pin the remaining unpinned protected string**

**Added after Task 1's first review.** The Global Constraints list four
user-visible strings that must survive the slice. Auditing them found that two
are already pinned by pre-existing assertions in
`crates/rollshot-app/src/timeline_workspace/update.rs` — `:4619`
(`"Caption suggestions failed: ..."`) and `:4720`
(`"Configure an agent provider before suggesting captions."`) — but
`"Suggesting captions..."` at `update.rs:1262` is asserted nowhere. Task 16
rewrites that handler, so nothing would catch a change to it.

Pinning it through the handler would need a provider-config harness the existing
tests do not have. Use the same shape as `TIMEOUT_MESSAGE` instead: extract the
literal into a named constant beside it and assert the constant's value.

In `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`, next to
`TIMEOUT_MESSAGE`:

```rust
/// User-visible copy shown while a caption run is in flight. Preserved verbatim
/// across the RunBudget migration (plan Task 16), which rewrites the handler
/// that sets it.
pub(crate) const RUNNING_MESSAGE: &str = "Suggesting captions...";
```

Replace the literal at `crates/rollshot-app/src/timeline_workspace/update.rs:1262`
with `super::caption_agent::RUNNING_MESSAGE.to_string()`.

Add the assertion beside `timeout_copy_baseline`:

```rust
    #[test]
    fn running_copy_baseline() {
        assert_eq!(
            super::RUNNING_MESSAGE,
            "Suggesting captions...",
            "user-visible in-flight copy must not change"
        );
    }
```

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/caption_agent.rs \
            crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "test(action): baseline caption instruction text and timeout copy"
```

---

## Task 2: `SourceBinding` becomes domain-tagged

The largest mechanical task. It must leave the whole workspace compiling and
green; behavior is unchanged.

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs:186-230` (the struct and
  its impl)
- Modify: `crates/rollshot-agent/src/continuity.rs:260-270` (source binding
  digest)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:1292-1301`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:2762`
- Modify: every `SourceBinding::new(` fixture site (see Step 4)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `SourceBinding::SmartRedaction { base_image_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32], document_state_id: u32,
    preset_id: String, active_preset_revision_id: Option<String> }`
  - `SourceBinding::ActionGuideProject { project_root_sha256: [u8; 32],
    revision: u64, projection_digest: String }`
  - `SourceBinding::ActionGuideEphemeralGuide { guide_digest: String }`
  - `SourceBinding::smart_redaction(base_image_sha256, annotation_state_sha256,
    document_state_id, preset_id, active_preset_revision_id) -> Self` —
    constructor keeping today's `new` argument order, so the fixture sweep is a
    rename.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in
`crates/rollshot-agent/src/product_task.rs`:

```rust
    #[test]
    fn source_binding_round_trips_all_variants() {
        let cases = vec![
            SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 7, "p".into(), None),
            SourceBinding::ActionGuideProject {
                project_root_sha256: [3u8; 32],
                revision: 9,
                projection_digest: "ab".repeat(32),
            },
            SourceBinding::ActionGuideEphemeralGuide {
                guide_digest: "cd".repeat(32),
            },
        ];

        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let back: SourceBinding = serde_json::from_str(&json).unwrap();
            assert_eq!(case, back, "round trip failed for {json}");
        }
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `rtk cargo test -p rollshot-agent source_binding_round_trips`
Expected: FAIL to compile — `no variant named ActionGuideProject`.

- [ ] **Step 3: Replace the struct with the enum**

In `crates/rollshot-agent/src/product_task.rs`, replace the `SourceBinding`
struct and its entire `impl` block with:

```rust
/// Domain-tagged binding identifying the source a task acts on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum SourceBinding {
    SmartRedaction {
        base_image_sha256: [u8; 32],
        annotation_state_sha256: [u8; 32],
        document_state_id: u32,
        preset_id: String,
        active_preset_revision_id: Option<String>,
    },
    ActionGuideProject {
        /// SHA-256 of the canonicalized project root path. The project manifest
        /// has no stable identity, so the path is the only one available.
        project_root_sha256: [u8; 32],
        revision: u64,
        projection_digest: String,
    },
    ActionGuideEphemeralGuide {
        guide_digest: String,
    },
}

impl SourceBinding {
    /// Constructor preserving the pre-migration argument order.
    pub fn smart_redaction(
        base_image_sha256: [u8; 32],
        annotation_state_sha256: [u8; 32],
        document_state_id: u32,
        preset_id: String,
        active_preset_revision_id: Option<String>,
    ) -> Self {
        Self::SmartRedaction {
            base_image_sha256,
            annotation_state_sha256,
            document_state_id,
            preset_id,
            active_preset_revision_id,
        }
    }

    /// Smart Redaction base-image digest, or `None` for other domains.
    pub fn smart_redaction_base_image_sha256(&self) -> Option<&[u8; 32]> {
        match self {
            Self::SmartRedaction {
                base_image_sha256, ..
            } => Some(base_image_sha256),
            _ => None,
        }
    }
}
```

Keep the existing `impl ValidateFinite for SourceBinding` returning `Ok(())`
unchanged.

- [ ] **Step 4: Sweep the fixture and call sites**

Find every site:

```bash
rtk grep -rn "SourceBinding::new(" crates/ --include="*.rs"
```

Replace each `SourceBinding::new(` with `SourceBinding::smart_redaction(`. The
argument lists are unchanged.

Then find the accessor uses:

```bash
rtk grep -rn "\.base_image_sha256()\|\.annotation_state_sha256()\|\.document_state_id()\|source_binding()\.preset_id()\|\.active_preset_revision_id()" crates/ --include="*.rs"
```

- [ ] **Step 5: Fix `continuity.rs`**

**Verified on review (2026-07-28):** adding a domain separator changes
`ContinuityProjectionV1::source_binding_digest` for existing Smart Redaction
tasks. That is safe: the projection is never persisted. Both sides of every
comparison are built in-process from a loaded snapshot — `driver.rs:792`-819
builds `projection` and compares against `expected.source_binding_digest()`,
where `expected` is itself a live projection. No stored string is compared
against a recomputed digest.

Replace the `source_binding_digest` block at
`crates/rollshot-agent/src/continuity.rs:260-270` with a per-variant hash. Each
variant gets its own domain separator so no two variants can collide:

```rust
        // Compute source binding digest. Each variant carries a distinct domain
        // separator so bindings from different domains cannot collide.
        let source_binding_digest = {
            let mut hasher = Sha256::new();
            match source_binding {
                crate::product_task::SourceBinding::SmartRedaction {
                    base_image_sha256,
                    annotation_state_sha256,
                    document_state_id,
                    preset_id,
                    active_preset_revision_id,
                } => {
                    hasher.update(b"rollshot-source-binding-smart-redaction-v1\0");
                    hasher.update(base_image_sha256);
                    hasher.update(annotation_state_sha256);
                    hasher.update(document_state_id.to_le_bytes());
                    hasher.update(preset_id.as_bytes());
                    if let Some(rev) = active_preset_revision_id {
                        hasher.update(rev.as_bytes());
                    }
                }
                crate::product_task::SourceBinding::ActionGuideProject {
                    project_root_sha256,
                    revision,
                    projection_digest,
                } => {
                    hasher.update(b"rollshot-source-binding-action-guide-project-v1\0");
                    hasher.update(project_root_sha256);
                    hasher.update(revision.to_le_bytes());
                    hasher.update(projection_digest.as_bytes());
                }
                crate::product_task::SourceBinding::ActionGuideEphemeralGuide {
                    guide_digest,
                } => {
                    hasher.update(b"rollshot-source-binding-action-guide-ephemeral-v1\0");
                    hasher.update(guide_digest.as_bytes());
                }
            }
            format!("{:x}", hasher.finalize())
        };
```

- [ ] **Step 6: Fix the two app-side consumers**

At `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:1292-1301`,
keep behavior identical for now by matching the variant explicitly. Task 3
replaces this with the comparison methods:

```rust
                TaskStatus::ReadyForReview => {
                    // Temporary: Task 3 replaces this with
                    // identity_matches / freshness_matches.
                    let (snap_base, snap_annotation) = match snapshot.source_binding() {
                        SourceBinding::SmartRedaction {
                            base_image_sha256,
                            annotation_state_sha256,
                            ..
                        } => (base_image_sha256, annotation_state_sha256),
                        _ => continue,
                    };
                    let (want_base, want_annotation) = match binding {
                        SourceBinding::SmartRedaction {
                            base_image_sha256,
                            annotation_state_sha256,
                            ..
                        } => (base_image_sha256, annotation_state_sha256),
                        _ => continue,
                    };
                    if snap_base != want_base {
                        continue;
                    }
                    if snap_annotation != want_annotation {
```

At `crates/rollshot-app/src/result_workspace/update.rs:2762`, replace the direct
accessor call:

```rust
                    if workbench.cached_base_digest
                        != source_binding.smart_redaction_base_image_sha256().copied()
```

- [ ] **Step 7: Run the full suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app`
Expected: PASS (feature off).

- [ ] **Step 8: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "refactor(agent): make SourceBinding domain-tagged"
```

---

## Task 3: `identity_matches` and `freshness_matches`

Splits "is this the same source" from "is it still valid", which is what makes a
non-image domain expressible in `reconcile_for_source`.

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs` (add to the
  `impl SourceBinding` from Task 2)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`
  (replace the Task 2 temporary block)

**Interfaces:**
- Consumes: the `SourceBinding` enum from Task 2.
- Produces:
  - `SourceBinding::identity_matches(&self, other: &SourceBinding) -> bool`
  - `SourceBinding::freshness_matches(&self, other: &SourceBinding) -> bool`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/rollshot-agent/src/product_task.rs`:

```rust
    #[test]
    fn identity_ignores_freshness_and_rejects_other_domains() {
        let a = SourceBinding::ActionGuideProject {
            project_root_sha256: [1u8; 32],
            revision: 1,
            projection_digest: "aa".repeat(32),
        };
        let same_project_newer = SourceBinding::ActionGuideProject {
            project_root_sha256: [1u8; 32],
            revision: 2,
            projection_digest: "bb".repeat(32),
        };
        let other_project = SourceBinding::ActionGuideProject {
            project_root_sha256: [9u8; 32],
            revision: 1,
            projection_digest: "aa".repeat(32),
        };
        let smart = SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 0, "p".into(), None);

        assert!(a.identity_matches(&same_project_newer));
        assert!(!a.freshness_matches(&same_project_newer));
        assert!(!a.identity_matches(&other_project));
        assert!(!a.identity_matches(&smart));
        assert!(!smart.identity_matches(&a));
    }

    #[test]
    fn smart_redaction_identity_is_base_image_freshness_is_annotation() {
        let a = SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 0, "p".into(), None);
        let edited = SourceBinding::smart_redaction([1u8; 32], [3u8; 32], 1, "p".into(), None);
        let different_image =
            SourceBinding::smart_redaction([8u8; 32], [2u8; 32], 0, "p".into(), None);

        assert!(a.identity_matches(&edited));
        assert!(!a.freshness_matches(&edited));
        assert!(!a.identity_matches(&different_image));
    }

    #[test]
    fn ephemeral_freshness_is_trivially_true_for_matching_identity() {
        let a = SourceBinding::ActionGuideEphemeralGuide {
            guide_digest: "ee".repeat(32),
        };
        let b = a.clone();

        assert!(a.identity_matches(&b));
        assert!(a.freshness_matches(&b));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent identity_ignores_freshness`
Expected: FAIL to compile — `no method named identity_matches`.

- [ ] **Step 3: Implement both methods**

Add to `impl SourceBinding`:

```rust
    /// Is this binding about the same source as `other`, regardless of how
    /// stale either is? Bindings from different domains never match.
    pub fn identity_matches(&self, other: &SourceBinding) -> bool {
        match (self, other) {
            (
                Self::SmartRedaction {
                    base_image_sha256: a,
                    ..
                },
                Self::SmartRedaction {
                    base_image_sha256: b,
                    ..
                },
            ) => a == b,
            (
                Self::ActionGuideProject {
                    project_root_sha256: a,
                    ..
                },
                Self::ActionGuideProject {
                    project_root_sha256: b,
                    ..
                },
            ) => a == b,
            (
                Self::ActionGuideEphemeralGuide { guide_digest: a },
                Self::ActionGuideEphemeralGuide { guide_digest: b },
            ) => a == b,
            _ => false,
        }
    }

    /// Given matching identity, is this binding still valid for `other`'s state?
    ///
    /// Ephemeral guides are trivially fresh: identity and freshness are the same
    /// digest. Their staleness after a restart is enforced by the store's
    /// open-time sweep, not by this comparison.
    pub fn freshness_matches(&self, other: &SourceBinding) -> bool {
        match (self, other) {
            (
                Self::SmartRedaction {
                    annotation_state_sha256: a,
                    ..
                },
                Self::SmartRedaction {
                    annotation_state_sha256: b,
                    ..
                },
            ) => a == b,
            (
                Self::ActionGuideProject {
                    revision: ra,
                    projection_digest: da,
                    ..
                },
                Self::ActionGuideProject {
                    revision: rb,
                    projection_digest: db,
                    ..
                },
            ) => ra == rb && da == db,
            (
                Self::ActionGuideEphemeralGuide { guide_digest: a },
                Self::ActionGuideEphemeralGuide { guide_digest: b },
            ) => a == b,
            _ => false,
        }
    }
```

- [ ] **Step 4: Run to verify the tests pass**

Run: `rtk cargo test -p rollshot-agent source_binding`
Expected: PASS, all four tests.

- [ ] **Step 5: Replace the temporary block in `reconcile_for_source`**

In `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`, replace
the whole `TaskStatus::ReadyForReview` comparison block written in Task 2 with:

```rust
                TaskStatus::ReadyForReview => {
                    // Different source entirely — not a restore candidate.
                    if !snapshot.source_binding().identity_matches(binding) {
                        continue;
                    }

                    // Same source, moved on — audited mark stale.
                    if !snapshot.source_binding().freshness_matches(binding) {
```

Leave the existing `mark_stale` + `transition_audited` body that follows
unchanged.

- [ ] **Step 6: Run the store suites**

Run: `rtk cargo test -p rollshot-app --features action-guide task_store`
Expected: PASS, including the pre-existing `reconcile_for_source` tests around
`task_store.rs:2222` and `:2243`.

- [ ] **Step 7: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "refactor(agent): split source identity from freshness"
```

---

## Task 4: Legacy on-disk compatibility and schema 3

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs` (custom `Deserialize` for
  `SourceBinding`; `new_v3`)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:493`
  (version guard)
- Modify: `crates/rollshot-agent/src/continuity.rs:248-251` (version guard)
- Create: `crates/rollshot-app/tests/fixtures/agent_tasks/task-schema-v1.json`
- Create: `crates/rollshot-app/tests/fixtures/agent_tasks/task-schema-v2.json`
- Create: `crates/rollshot-app/tests/fixtures/agent_tasks/task-schema-v2-ready.json`

**Interfaces:**
- Consumes: the `SourceBinding` enum from Task 2.
- Produces: `ProductTaskSnapshot::new_v3(task_id, kind, source_binding, now)
  -> Result<Self, TaskContractError>` writing `store_schema_version: 3`.

**Verified on review (2026-07-28):** the `#[serde(untagged)]` + `[u8; 32]`
approach in Step 3 was run against serde 1.0.229 / serde_json 1.0.151 (workspace
lock is 1.0.228 / 1.0.150; the `Content` buffering path is identical). All three
tagged variants round-trip, the legacy flat object maps to `SmartRedaction`, and
an unrelated object is rejected. `Content::Seq` of `Content::U64` feeds
`[u8; 32]`'s `deserialize_tuple(32, ..)` correctly. Do **not** replace this with
a hand-rolled visitor. The one real cost is error quality: a corrupt file yields
`"data did not match any variant of untagged enum Repr"` inside
`TaskStoreError::Corrupt`. Step 6 pins that as a negative test so the message is
a known quantity rather than a surprise.

`crates/rollshot-app/tests/` already holds `eval/fixtures/` and no
`tests/fixtures/*.rs`, so a fixtures-only directory is not picked up as an
integration-test target.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/rollshot-agent/src/product_task.rs`:

```rust
    #[test]
    fn legacy_flat_source_binding_deserializes_as_smart_redaction() {
        // Pre-migration on-disk shape: a flat object with no "domain" tag.
        let legacy = r#"{
            "base_image_sha256": [1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
                                  1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
            "annotation_state_sha256": [2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,
                                        2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2],
            "document_state_id": 4,
            "preset_id": "preset-001",
            "active_preset_revision_id": null
        }"#;

        let parsed: SourceBinding = serde_json::from_str(legacy).unwrap();

        assert_eq!(
            parsed,
            SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 4, "preset-001".into(), None)
        );
    }

    #[test]
    fn unrelated_object_is_rejected_not_defaulted() {
        // The untagged shim must not silently invent a SmartRedaction binding
        // from an object it does not recognize.
        let err = serde_json::from_str::<SourceBinding>(r#"{"nope":1}"#)
            .expect_err("unrelated object must not deserialize");
        assert!(
            err.to_string().contains("did not match any variant"),
            "unexpected error text: {err}"
        );
    }

    #[test]
    fn new_v3_writes_schema_three() {
        let task = ProductTaskSnapshot::new_v3(
            task_id_fixture(),
            TaskKind::SmartRedactionAuthor,
            source_binding_fixture(),
            1_000,
        )
        .unwrap();

        assert_eq!(task.store_schema_version(), 3);
    }
```

`task_id_fixture()` and `source_binding_fixture()` already exist in this module
(`product_task.rs:1510` and `:1522`). **There is no `ProductTaskId::new_v4()`,
`ArtifactId::new_v4()`, or `RunId::new_v4()`** — those three types expose only
`parse` (`product_task.rs:27`, `:59`; `domain.rs:35`). Only
`AuditEventId::new_v4()` exists (`audit.rs:62`). Production code builds ids as
`ProductTaskId::parse(format!("task-{}", uuid::Uuid::new_v4())).expect("v4 UUID
is valid")` (`result_workspace/update.rs:2411`-2415). Use the module fixtures in
unit tests and that idiom only where a fresh id is genuinely required.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent legacy_flat_source_binding`
Expected: FAIL — missing field `domain`.

- [ ] **Step 3: Add the two-arm deserializer**

Remove `Deserialize` from the `SourceBinding` derive list, keep `Serialize`, and
add below the enum:

```rust
/// Deserialization shim accepting both the tagged form and the pre-migration
/// flat Smart Redaction object. Tagged is tried first; a flat object can only
/// have been a Smart Redaction binding, because no other domain existed.
impl<'de> Deserialize<'de> for SourceBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "domain", rename_all = "snake_case")]
        enum Tagged {
            SmartRedaction {
                base_image_sha256: [u8; 32],
                annotation_state_sha256: [u8; 32],
                document_state_id: u32,
                preset_id: String,
                active_preset_revision_id: Option<String>,
            },
            ActionGuideProject {
                project_root_sha256: [u8; 32],
                revision: u64,
                projection_digest: String,
            },
            ActionGuideEphemeralGuide {
                guide_digest: String,
            },
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyFlat {
            base_image_sha256: [u8; 32],
            annotation_state_sha256: [u8; 32],
            document_state_id: u32,
            preset_id: String,
            #[serde(default)]
            active_preset_revision_id: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Tagged(Tagged),
            LegacyFlat(LegacyFlat),
        }

        Ok(match Repr::deserialize(deserializer)? {
            Repr::Tagged(Tagged::SmartRedaction {
                base_image_sha256,
                annotation_state_sha256,
                document_state_id,
                preset_id,
                active_preset_revision_id,
            }) => Self::SmartRedaction {
                base_image_sha256,
                annotation_state_sha256,
                document_state_id,
                preset_id,
                active_preset_revision_id,
            },
            Repr::Tagged(Tagged::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            }) => Self::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            },
            Repr::Tagged(Tagged::ActionGuideEphemeralGuide { guide_digest }) => {
                Self::ActionGuideEphemeralGuide { guide_digest }
            }
            Repr::LegacyFlat(flat) => Self::SmartRedaction {
                base_image_sha256: flat.base_image_sha256,
                annotation_state_sha256: flat.annotation_state_sha256,
                document_state_id: flat.document_state_id,
                preset_id: flat.preset_id,
                active_preset_revision_id: flat.active_preset_revision_id,
            },
        })
    }
}
```

- [ ] **Step 4: Add `new_v3`**

Immediately after `new_v2` in `crates/rollshot-agent/src/product_task.rs`:

```rust
    /// V3 constructor: domain-tagged source binding, kind-agnostic artifact
    /// payload. Like V2, requires a run-contract binding before promotion.
    pub fn new_v3(
        task_id: ProductTaskId,
        kind: TaskKind,
        source_binding: SourceBinding,
        now: i64,
    ) -> Result<Self, TaskContractError> {
        Ok(Self {
            store_schema_version: 3,
            snapshot_revision: 0,
            task_id,
            kind,
            source_binding,
            status: TaskStatus::Created,
            attempts: Vec::new(),
            artifact_metadata: None,
            pending_artifact_payload: None,
            pending_proposal_payload: None,
            review_receipt: None,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        })
    }
```

- [ ] **Step 5: Relax both version guards**

At `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:493`,
change `if snapshot.store_schema_version() > 2 {` to `> 3`.

At `crates/rollshot-agent/src/continuity.rs:249`, change
`if store_schema == 0 || store_schema > 2 {` to `> 3`.

- [ ] **Step 6: Add on-disk fixtures and a load test**

Create `crates/rollshot-app/tests/fixtures/agent_tasks/task-schema-v1.json` by
capturing a real pre-migration file. Generate it with the pre-migration
serializer output shape:

```json
{
  "store_schema_version": 1,
  "snapshot_revision": 0,
  "task_id": "task-00000000-0000-4000-8000-000000000001",
  "kind": "smart_redaction_author",
  "source_binding": {
    "base_image_sha256": [1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
    "annotation_state_sha256": [2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2],
    "document_state_id": 0,
    "preset_id": "preset-001",
    "active_preset_revision_id": null
  },
  "status": "created",
  "attempts": [],
  "artifact_metadata": null,
  "pending_artifact_payload": null,
  "review_receipt": null,
  "created_at_unix_ms": 1000,
  "updated_at_unix_ms": 1000
}
```

Create `task-schema-v2.json` as the same document with
`"store_schema_version": 2`.

Create `task-schema-v2-ready.json`: schema 2, `"status": "ready_for_review"`,
with a **populated** `artifact_metadata` in today's pre-migration shape — the two
flat counters at the top level of the metadata object, no `summary` key:

```json
  "artifact_metadata": {
    "artifact_id": "artifact-00000000-0000-4000-8000-000000000001",
    "artifact_revision": 1,
    "kind": "smart_redaction",
    "schema_version": 2,
    "canonical_payload_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "source_binding": { "...": "same flat object as above" },
    "task_id": "task-00000000-0000-4000-8000-000000000001",
    "attempt_id": 1,
    "run_id": "run-00000000-0000-4000-8000-000000000001",
    "proposal_id": "proposal-00000000-0000-4000-8000-000000000001",
    "provider_id": "anthropic",
    "model_id": "claude-sonnet-4-20250514",
    "run_config_digest": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    "dry_run_candidate_count": 3,
    "dry_run_affected_area": 0.42,
    "created_at_unix_ms": 1000
  }
```

Generate the exact bytes by serializing today's `ProductArtifactMetadata` rather
than hand-writing it, so field names and order are real. This fixture is the
regression net for Task 5: replacing the two flat counters with
`summary: ArtifactSummary` breaks this file's `Deserialize` unless Task 5 adds a
compatibility shim, and Task 5 Step 3a exists precisely to catch that.

Add the load test to the `#[cfg(test)] mod tests` in
`crates/rollshot-app/src/result_workspace/workbench/task_store.rs`:

```rust
    #[test]
    fn loads_pre_migration_schema_fixtures() {
        for (name, expected_version) in [
            ("task-schema-v1.json", 1u32),
            ("task-schema-v2.json", 2u32),
            ("task-schema-v2-ready.json", 2u32),
        ] {
            let raw = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/agent_tasks")
                    .join(name),
            )
            .unwrap();

            let snapshot: ProductTaskSnapshot = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("{name} failed to load: {e}"));

            assert_eq!(snapshot.store_schema_version(), expected_version);
            assert!(matches!(
                snapshot.source_binding(),
                SourceBinding::SmartRedaction { .. }
            ));
        }
    }
```

- [ ] **Step 7: Run the suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide loads_pre_migration`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "feat(agent): read pre-migration task snapshots, add schema 3"
```

---

## Task 5: `ArtifactSummary` replaces the two flat artifact fields

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs:289-450`
  (`ProductArtifactMetadata`, its custom `Deserialize`, `new`, `new_v2`,
  `new_v3`)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  (promotion construction sites)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`
  (extend the Task 4 fixture load test)
- Create: `crates/rollshot-app/tests/fixtures/agent_tasks/task-schema-v2-ready.json`
  is created in Task 4 and first exercised against the new shape here

**Interfaces:**
- Consumes: the `task-schema-v2-ready.json` fixture from Task 4.
- Produces:
  - `ArtifactSummary::SmartRedaction { dry_run_candidate_count: u32,
    dry_run_affected_area: f32 }`
  - `ArtifactSummary::ActionGuideCaptions { suggestion_count: u32 }`
  - `ProductArtifactMetadata::summary(&self) -> &ArtifactSummary`
  - `ProductArtifactMetadata::new_v3(...)` taking `summary: ArtifactSummary`
    where `new_v2` took the two flat counters, with all other parameters in the
    same order.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn artifact_summary_is_kind_specific() {
        let meta = artifact_metadata_fixture_v3(ArtifactSummary::ActionGuideCaptions {
            suggestion_count: 3,
        });

        assert_eq!(
            meta.summary(),
            &ArtifactSummary::ActionGuideCaptions { suggestion_count: 3 }
        );

        let json = serde_json::to_string(&meta).unwrap();
        assert!(
            !json.contains("dry_run_affected_area"),
            "caption artifacts must not carry Smart Redaction dry-run fields: {json}"
        );
    }
```

Add the fixture helper next to the existing `source_binding_fixture`, reusing the
module's existing id fixtures (`product_task.rs:1510`-1520):

```rust
    fn artifact_metadata_fixture_v3(summary: ArtifactSummary) -> ProductArtifactMetadata {
        // `kind` stays SmartRedaction: ArtifactKind::ActionGuideCaptions does
        // not exist until Task 10. This test only constrains `summary`.
        ProductArtifactMetadata::new_v3(
            artifact_id_fixture(),
            ArtifactRevision::new(1),
            ArtifactKind::SmartRedaction,
            1,
            "aa".repeat(32),
            source_binding_fixture(),
            task_id_fixture(),
            TaskAttemptId::new(1),
            run_id_fixture(),
            "proposal-1".to_owned(),
            "provider".to_owned(),
            "model".to_owned(),
            "cfg".to_owned(),
            summary,
            1_000,
        )
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent artifact_summary_is_kind_specific`
Expected: FAIL to compile — `ArtifactSummary` not found.

- [ ] **Step 3: Add the enum and rewire the metadata struct**

Add above `ProductArtifactMetadata`:

```rust
/// Kind-specific artifact summary. Replaces the flat Smart Redaction dry-run
/// counters that previously lived on `ProductArtifactMetadata`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtifactSummary {
    SmartRedaction {
        dry_run_candidate_count: u32,
        dry_run_affected_area: f32,
    },
    ActionGuideCaptions {
        suggestion_count: u32,
    },
}
```

In `ProductArtifactMetadata`, replace the two fields:

```rust
    summary: ArtifactSummary,
```

Add the accessor next to `kind()`:

```rust
    pub fn summary(&self) -> &ArtifactSummary {
        &self.summary
    }
```

Add `new_v3` alongside `new_v2`, identical except that the
`dry_run_candidate_count: u32, dry_run_affected_area: f32` pair is replaced by
`summary: ArtifactSummary`, and `run_contract` is set to `None`. Keep `new_v2`
as a wrapper so existing callers keep compiling:

```rust
    /// V2 compatibility wrapper. Wraps the flat dry-run counters into
    /// `ArtifactSummary::SmartRedaction`.
    #[allow(clippy::too_many_arguments)]
    pub fn new_v2(
        artifact_id: ArtifactId,
        artifact_revision: ArtifactRevision,
        kind: ArtifactKind,
        schema_version: u32,
        canonical_payload_sha256: String,
        source_binding: SourceBinding,
        task_id: ProductTaskId,
        attempt_id: TaskAttemptId,
        run_id: RunId,
        proposal_id: String,
        provider_id: String,
        model_id: String,
        run_config_digest: String,
        dry_run_candidate_count: u32,
        dry_run_affected_area: f32,
        created_at_unix_ms: i64,
        run_contract: RunContractReceiptV1,
    ) -> Self {
        let mut meta = Self::new_v3(
            artifact_id,
            artifact_revision,
            kind,
            schema_version,
            canonical_payload_sha256,
            source_binding,
            task_id,
            attempt_id,
            run_id,
            proposal_id,
            provider_id,
            model_id,
            run_config_digest,
            ArtifactSummary::SmartRedaction {
                dry_run_candidate_count,
                dry_run_affected_area,
            },
            created_at_unix_ms,
        );
        meta.run_contract = Some(run_contract);
        meta
    }
```

**Verified on review (2026-07-28):** `meta.run_contract = Some(..)` is legal.
`run_contract` is a private field of `ProductArtifactMetadata`, and `new_v2`
lives in the same `impl` block in the same module (`product_task.rs:354`-393),
where private fields are in scope. Today's `new_v2` already writes
`run_contract: Some(run_contract)` directly (`product_task.rs:391`).

**Keep the V1 `new` constructor as a wrapper too.** It has 14 live callers, all
tests: `run.rs:3485, :3815, :4664, :4827, :4918, :5271, :5422, :5568`,
`task_store.rs:1582`, `result_workspace/mod.rs:1034, :1142`,
`continuity.rs:1278`, `audit.rs:1323`, `product_task.rs:1592`. Convert it the
same way, wrapping the flat counters into `ArtifactSummary::SmartRedaction` and
leaving `run_contract: None`. Do not delete it in this slice.

- [ ] **Step 3a: Preserve on-disk compatibility for populated artifact metadata**

`ProductArtifactMetadata` is `Serialize`/`Deserialize` and is persisted inside
every `ReadyForReview`, `Completed`, and `Rejected` task file. Swapping the two
flat counters for `summary` makes every such pre-migration file fail to load with
`missing field summary`, which `read_snapshot` turns into
`TaskStoreError::Corrupt` and `reconcile_for_source` then **silently skips**
(`task_store.rs:1235`-1238) — a pending review disappears with no user-visible
error. Task 4's `task-schema-v2-ready.json` fixture is the detector.

Run first, to see the break: `rtk cargo test -p rollshot-app --features action-guide loads_pre_migration`
Expected: FAIL — `missing field summary`.

Then replace the derived `Deserialize` on `ProductArtifactMetadata` with a
one-shot compatibility impl over an all-fields DTO:

```rust
/// Deserialization shim accepting both `summary` and the pre-migration flat
/// dry-run counter pair. A file can carry one or the other, never neither.
impl<'de> Deserialize<'de> for ProductArtifactMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            artifact_id: ArtifactId,
            artifact_revision: ArtifactRevision,
            kind: ArtifactKind,
            schema_version: u32,
            canonical_payload_sha256: String,
            source_binding: SourceBinding,
            task_id: ProductTaskId,
            attempt_id: TaskAttemptId,
            run_id: RunId,
            proposal_id: String,
            provider_id: String,
            model_id: String,
            run_config_digest: String,
            #[serde(default)]
            summary: Option<ArtifactSummary>,
            #[serde(default)]
            dry_run_candidate_count: Option<u32>,
            #[serde(default)]
            dry_run_affected_area: Option<f32>,
            created_at_unix_ms: i64,
            #[serde(default)]
            run_contract: Option<RunContractReceiptV1>,
        }

        let r = Repr::deserialize(deserializer)?;
        let summary = match (r.summary, r.dry_run_candidate_count, r.dry_run_affected_area) {
            (Some(summary), _, _) => summary,
            (None, Some(dry_run_candidate_count), Some(dry_run_affected_area)) => {
                ArtifactSummary::SmartRedaction {
                    dry_run_candidate_count,
                    dry_run_affected_area,
                }
            }
            (None, _, _) => {
                return Err(serde::de::Error::missing_field("summary"));
            }
        };
        Ok(Self {
            artifact_id: r.artifact_id,
            artifact_revision: r.artifact_revision,
            kind: r.kind,
            schema_version: r.schema_version,
            canonical_payload_sha256: r.canonical_payload_sha256,
            source_binding: r.source_binding,
            task_id: r.task_id,
            attempt_id: r.attempt_id,
            run_id: r.run_id,
            proposal_id: r.proposal_id,
            provider_id: r.provider_id,
            model_id: r.model_id,
            run_config_digest: r.run_config_digest,
            summary,
            created_at_unix_ms: r.created_at_unix_ms,
            run_contract: r.run_contract,
        })
    }
}
```

Add the assertion that the shim actually maps, not just that the file parses:

```rust
    #[test]
    fn legacy_flat_dry_run_counters_become_a_smart_redaction_summary() {
        let raw = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/agent_tasks/task-schema-v2-ready.json"),
        )
        .unwrap();
        let snapshot: ProductTaskSnapshot = serde_json::from_str(&raw).unwrap();

        assert_eq!(
            snapshot.artifact_metadata().unwrap().summary(),
            &ArtifactSummary::SmartRedaction {
                dry_run_candidate_count: 3,
                dry_run_affected_area: 0.42,
            }
        );
    }
```

Run: `rtk cargo test -p rollshot-app --features action-guide legacy_flat_dry_run_counters`
Expected: PASS.

- [ ] **Step 4: Update the promotion sites**

```bash
rtk grep -rn "ProductArtifactMetadata::new" crates/ --include="*.rs"
```

Smart Redaction sites keep calling `new_v2` or `new` unchanged, because both are
now wrappers. No behavioral edit is needed at this task.

- [ ] **Step 5: Run the suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "refactor(agent): move dry-run counters into ArtifactSummary"
```

---

## Task 6: Artifact payload surface becomes kind-agnostic

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs:948-996`
  (`record_ready_for_review`) and its test module
- Modify: `crates/rollshot-agent/src/audit.rs`,
  `crates/rollshot-agent/src/continuity.rs` (test call sites only)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs` (the
  Smart Redaction promotion call at `:897`, plus test call sites)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`,
  `crates/rollshot-app/src/result_workspace/mod.rs` (test call sites only)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `record_ready_for_review(metadata: ProductArtifactMetadata,
  payload_bytes: Vec<u8>, proposal_payload: Option<Vec<u8>>, now: i64)
  -> Result<Self, TaskContractError>` — the payload is now caller-serialized
  bytes.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn promotion_accepts_caller_serialized_bytes() {
        let task = running_with_contract_fixture();
        let contract = task.active_run_contract().unwrap().clone();
        let payload = br#"{"suggestions":[]}"#.to_vec();

        let promoted = task
            .record_ready_for_review(
                v2_metadata_with_contract(&contract),
                payload.clone(),
                None,
                30,
            )
            .unwrap();

        assert_eq!(promoted.status(), TaskStatus::ReadyForReview);
        assert_eq!(promoted.pending_artifact_payload(), Some(payload.as_slice()));
    }

    #[test]
    fn promotion_rejects_an_empty_payload() {
        let task = running_with_contract_fixture();
        let contract = task.active_run_contract().unwrap().clone();

        assert!(matches!(
            task.record_ready_for_review(
                v2_metadata_with_contract(&contract),
                Vec::new(),
                None,
                30,
            ),
            Err(TaskContractError::MissingPayload)
        ));
    }
```

The two fixtures already exist with these exact names:
`running_with_contract_fixture()` at `product_task.rs:2307` (a `Running` schema-2
task with a bound run contract) and `v2_metadata_with_contract(&contract)` at
`product_task.rs:2313`. Reuse them; do not write new ones. `now: 30` matches the
rest of the module — `running_with_contract_fixture` binds at 25, so anything
below that is a `TimestampRegression`.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent promotion_accepts_caller_serialized`
Expected: FAIL to compile — expected `SmartRedactionReviewPayload`, found
`Vec<u8>`.

- [ ] **Step 3: Change the parameter type**

In `record_ready_for_review`, replace the signature's
`payload: SmartRedactionReviewPayload` with `payload_bytes: Vec<u8>`, and delete
the internal serialization line:

```rust
        let payload_bytes =
            serde_json::to_vec(&payload).map_err(|_e| TaskContractError::MissingPayload)?;
```

Reject an empty payload instead, so a promotion can never record nothing:

```rust
        if payload_bytes.is_empty() {
            return Err(TaskContractError::MissingPayload);
        }
```

Leave every other check in the function unchanged.

- [ ] **Step 4: Update every caller, in both crates**

```bash
rtk grep -rn "record_ready_for_review(" crates/ --include="*.rs"
```

There are roughly 35 call sites and **most are inside `rollshot-agent`'s own test
modules**, which the crate-scoped grep in an earlier draft of this plan missed:
`product_task.rs` (×10), `audit.rs` (×8), `continuity.rs` (×3),
`result_workspace/workbench/run.rs` (×8), `task_store.rs` (×6),
`result_workspace/mod.rs` (×2). Skipping any of them leaves
`rtk cargo test -p rollshot-agent` red.

Test sites almost all pass `payload_fixture()`. Do **not** change
`payload_fixture()`'s return type — other assertions read its fields. Instead, in
each test module that has a `payload_fixture`, add a sibling:

```rust
    fn payload_bytes_fixture() -> Vec<u8> {
        serde_json::to_vec(&payload_fixture()).expect("fixture payload serializes")
    }
```

then replace `record_ready_for_review(meta, payload_fixture(), ..)` with
`record_ready_for_review(meta, payload_bytes_fixture(), ..)`. That is the whole
transformation for test sites.

At each production site, serialize before the call:

```rust
        let payload_bytes = serde_json::to_vec(&review_payload)
            .map_err(|e| WorkbenchError::StorePersist {
                message: format!("serialize review payload: {e}"),
            })?;
```

and pass `payload_bytes` where `review_payload` was passed. `PromotionContext`,
`SmartRedactionReviewPayload`, `PayloadSourceV1`, and `PayloadProposalV1` remain
in `rollshot-agent` as Smart Redaction DTOs; only the snapshot boundary stops
knowing about them.

- [ ] **Step 5: Run the suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "refactor(agent): accept kind-agnostic artifact payload bytes"
```

---

## Task 7: Authority digest recomputation audit

Evidence-only. The spec (§3.6) requires this resolved **before** Task 8 changes
the hash formula. No production code changes here.

**Files:**
- Test: `crates/rollshot-agent/src/authority.rs` (add to its test module)

**Interfaces:**
- Consumes: nothing.
- Produces: a recorded finding. If the finding is negative, Task 8 uses the
  fallback described in its Step 3.

- [x] **Step 1: Search for every persisted-digest comparison**

```bash
rtk grep -rn "snapshot_digest\|document_binding_digest\|binding_digest_hex" crates/ --include="*.rs"
```

For each hit, classify it as one of:
- compares a value against itself or against another in-memory value (safe);
- copies the value into a projection or receipt (safe);
- **recomputes a digest from a loaded snapshot and compares it to a persisted
  string (unsafe — triggers the fallback).**

**Pre-populated from the review pass on 2026-07-28; re-verified on 2026-07-29**
against the current tree (Slices 4-6 landed in between and shifted line numbers
in `continuity.rs`, `run.rs`, `task_store.rs`, `driver.rs`; no classification
changed). `product_task.rs` sites, missed by the first pass, are added below.

| Site | Classification |
|---|---|
| `authority.rs:208`, `:269` | producer — `binding_digest_hex()` into receipt and into `DigestedSnapshotV1`. Safe |
| `authority.rs:252`-258 | the document-binding-digest formula (`binding_digest_hex`) |
| `authority.rs:261`-279 | the snapshot digest formula itself (`compute_digest` / `DigestedSnapshotV1`) — this is what Task 8 changes |
| `authority.rs:503`-505 | test, live-vs-live receipt comparison. Safe |
| `audit.rs:835`, `:1089` | copy into an audit envelope (`AuthorityAuditRefV1`). Safe |
| `audit.rs:1806` (was `:1802`) | test, compares a value against itself. Safe |
| `audit.rs:2208` (was `:2204`) | test, `bound.snapshot_digest == authority.digest()` — live snapshot, same process. Safe |
| `continuity.rs:313`-314 (was `:288`) | copy into `ContinuityProjectionV1`'s DTO (`run_contract_authority_snapshot_digest`), never recomputed. Safe |
| `continuity.rs:929`-941 | `RunContinuityManifestV1::build`'s `AuthorityMismatch`/`SkillMismatch` checks — see Note A below. Safe |
| `continuity.rs:1746`, `:1776` | test, asserts the JSON *field name* `run_contract_authority_snapshot_digest` is absent when no contract exists — a serialization-shape check, not a digest-value comparison. Not applicable |
| `run.rs:851` (was `:839`) | copy into `RunConfigFingerprintV2`. Safe |
| `run.rs:4429` (was `:4404`) | copy into fingerprint (`run_config_v2_with`). Safe |
| `run.rs:4561`-4562 (was `:4536`-4537) | test, compares a value against the same in-process fixture it was built from. Safe |
| `run.rs:4577` (was `:4552`) | test, mutates a fixture field to prove fingerprint sensitivity, not a comparison against a persisted digest. Safe |
| `task_store.rs:1724` | copy into fingerprint. Safe |
| `task_store.rs:2865` (was `:2822`) | test, compares a durably-appended-then-reread audit-journal string field (`committed_audit_events(...)`) against the in-memory fixture it was written from. The stored field is an opaque string copy end to end — no digest algorithm runs on the read path. Safe |
| `run.rs:4373`-4394 (was `:4356`), `task_store.rs:1677`, `:1680`, `continuity.rs:1216`, `:1219` (was `:1190`), `driver.rs:5984` (was `:5978`) | test fixtures constructing `AuthoritySnapshotReceiptV1` with a literal `document_binding_digest`/`snapshot_digest`. **Field-rename fan-out for Task 8 Step 4**, not digest comparisons |
| `driver.rs:5987` | test, embeds the live `authority.digest()` (captured at `:5971`) into a fixture receipt so a downstream overflow-recovery check passes. Live-vs-live, not a comparison against an independently persisted value. Safe |
| `product_task.rs:939` | struct field declaration (`RunConfigFingerprintV2.authority_snapshot_digest`). Not a comparison site |
| `product_task.rs:2607`, `:2610`, `:2674` | fixture literal / copy into fingerprint — same pattern as `run.rs`/`task_store.rs` above. Field-rename fan-out / Safe |
| `product_task.rs:2962`-3104` | tests: literal fixture inputs, determinism (same config hashed twice), serde round-trip equality, privacy scan. None recompute from a loaded snapshot. Safe |

**Note A — the continuity recovery check (§ task instructions point 2).**
`continuity.rs:929`-941 compares `inputs.authority.digest()` /
`inputs.skill_use.digest()` (cached fields, never recomputed) against a
substring of a freshly-rebuilt `ContinuityProjectionV1`'s canonical bytes. That
projection copies `contract.authority.snapshot_digest` verbatim from a
`ProductTaskSnapshot` reloaded from disk — also never recomputed. Traced to the
sole caller that constructs `Durable` (`rollshot-app`'s
`result_workspace/workbench/run.rs:1229`-1334): the `authority`/`skill_use`
objects are the same live, in-memory instances held for the entire
`run_with_provider` call, including through in-run overflow-recovery restarts,
and the run contract they are checked against was written to disk by this same
run moments earlier. Neither side recomputes a digest from a loaded snapshot;
the check fails closed (`ContextRecoveryFailure`) on any mismatch rather than
silently accepting stale data. The other two production callers of
`run_with_provider` (`result_workspace/workbench/eval/record.rs:382` and
`result_workspace/workbench/eval/layer1.rs:146`) pass `RunContinuitySource::Unavailable`
and therefore never reach the digest comparison. Safe.

**Note B — is `ContinuityProjectionV1` ever persisted (§ task instructions
point 1)?** No. It has no `Deserialize` impl and no on-disk representation;
every instance is rebuilt fresh via `TryFrom<&ProductTaskSnapshot>`
(`continuity.rs:243`). Its own digest (over `ContinuityProjectionDto` with the
`CONTINUITY_PROJECTION_DOMAIN` separator) is used only transiently — embedded
into the never-persisted `RunContinuityManifestV1` (`continuity.rs:1011`) or
truncated for a tracing log (`continuity.rs:1076`) — and is never itself
written to disk or compared against a stored value.

**Note C — structural guarantee.** `AuthoritySnapshot` cannot be reconstructed
from a persisted receipt: it has no `Deserialize` impl, and every
`AuthoritySnapshot::new` call site in the workspace builds it from full binding
fields, never from `AuthoritySnapshotReceiptV1`. The recompute-from-loaded-
snapshot pattern is closed off at the type level, independent of the site-by-
site audit above.

**Note D — other Serialize-only, JSON-is-the-hash-input DTOs.** Beyond
`DigestedSnapshotV1`, two more types play the same role, where a field
rename/reorder silently changes their digest: `ContinuityProjectionDto`
(`continuity.rs:88`, hashed with `CONTINUITY_PROJECTION_DOMAIN` into
`ContinuityProjectionV1.digest`) and `RunContinuityManifestDto`
(`continuity.rs:1132`, hashed into `RunContinuityManifestV1.digest`, explicitly
documented "Never persisted"). Neither computes the `AuthoritySnapshot` digest
Task 8 is changing — both only ever embed `AuthoritySnapshot.digest()` /
`AuthoritySnapshotReceiptV1.snapshot_digest` as an opaque string field — so
Task 8 needs no action on them, but they are flagged as the same risk pattern
for any future change to either DTO.

**Finding: clean.** No site recomputes an authority digest from a loaded snapshot
and compares it against a persisted string. Task 8 may therefore give the
`Document` arm a `b"rollshot-authority-subject-document-v1\0"` separator. Record
this verdict in Task 22 Step 2.

- [x] **Step 2: Write the pinning test**

```rust
    #[test]
    fn persisted_authority_digest_is_never_recomputed_for_comparison() {
        // A digest is stable for a given snapshot, so recomputation is
        // indistinguishable from reuse unless the formula changes. This test
        // pins the property the migration depends on: nothing outside this
        // module derives a digest to check against a stored one.
        //
        // Verified by the Step 1 audit. If a future change adds such a
        // comparison, it must also add a formula-version field.
        let snapshot = full_snapshot();
        let first = snapshot.digest().to_string();
        let receipt = snapshot.receipt(1_000);

        assert_eq!(receipt.snapshot_digest, first);
        assert_eq!(snapshot.digest(), first, "digest must be cached, not recomputed");

        // Two snapshots built from identical inputs agree, so a receipt loaded
        // from disk is comparable to a freshly built one only while the formula
        // is unchanged. That is the property the migration relies on.
        assert_eq!(full_snapshot().digest(), first);
    }
```

`full_snapshot()` already exists at `authority.rs:439` and uses the `Document`
binding fixture; there is no `authority_snapshot_fixture`. The module's other
builders are `snapshot_with`, `snapshot_with_grants`, and
`snapshot_with_disclosure` (`authority.rs:398`-437).

- [ ] **Step 3: Record the finding in the plan file**

**Done.** The classification table in Step 1 above was re-verified on 2026-07-29
against the current tree and corrected for line-number drift; the verdict is
unchanged from the 2026-07-28 pass: **clean**. See Step 1's table and Notes
A-D for the full record, including the two points the task instructions called
out for extra scrutiny (`ContinuityProjectionV1` persistence status, and the
continuity-recovery digest check's same-process/same-code-version provenance).

- [ ] **Step 4: Run the test**

Run: `rtk cargo test -p rollshot-agent persisted_authority_digest`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-agent/src/authority.rs docs/superpowers/plans/2026-07-28-action-guide-agent-foundation-captions.md
rtk git commit -m "test(agent): pin that persisted authority digests are not recomputed"
```

---

## Task 8: `AuthoritySubject` replaces the document binding

**Files:**
- Modify: `crates/rollshot-agent/src/authority.rs:71-270` (`AuthorityBinding`,
  digest, `authorize_tool`, `receipt`)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs` (authority
  construction and every `authorize_tool` caller)
- Modify: `crates/rollshot-agent/src/tools.rs` (tool dispatch passing the
  binding)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `AuthoritySubject::Document(DocumentContentBinding)`
  - `AuthoritySubject::ActionGuideProject { project_root_sha256: [u8; 32],
    revision: u64, projection_digest: String }`
  - `AuthoritySubject::ActionGuideEphemeralGuide { guide_digest: String }`
  - `AuthorityBinding::new(task_id, attempt_id, run_id, subject:
    AuthoritySubject)`
  - `AuthorityBinding::subject(&self) -> &AuthoritySubject`
  - `AuthoritySnapshot::authorize_tool(&self, run_id: &RunId, subject:
    &AuthoritySubject, required: RunOperation) -> Result<(), AuthorityError>`

- [ ] **Step 1: Write the failing tests**

These use the module's existing `task_id()` and `run_id()` fixtures
(`authority.rs:378`-384). There is no `RunId::new_v4()` or
`ProductTaskId::new_v4()`; both types expose only `parse`.

```rust
    #[test]
    fn action_guide_subject_authorizes_submit_and_rejects_image_ops() {
        let subject = AuthoritySubject::ActionGuideProject {
            project_root_sha256: [4u8; 32],
            revision: 2,
            projection_digest: "ab".repeat(32),
        };
        let run_id = run_id();
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id.clone(),
                subject.clone(),
            ),
            "rollshot-v1".to_owned(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            BTreeSet::new(),
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
        )
        .unwrap();

        assert!(snapshot
            .authorize_tool(&run_id, &subject, RunOperation::SubmitReviewCandidate)
            .is_ok());
        assert!(matches!(
            snapshot.authorize_tool(&run_id, &subject, RunOperation::InspectPreparedImage),
            Err(AuthorityError::GrantMissing { .. })
        ));
    }

    #[test]
    fn subject_mismatch_is_rejected() {
        let subject = AuthoritySubject::ActionGuideProject {
            project_root_sha256: [4u8; 32],
            revision: 2,
            projection_digest: "ab".repeat(32),
        };
        let moved_on = AuthoritySubject::ActionGuideProject {
            project_root_sha256: [4u8; 32],
            revision: 3,
            projection_digest: "cd".repeat(32),
        };
        let run_id = run_id();
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id.clone(),
                subject,
            ),
            "rollshot-v1".to_owned(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            BTreeSet::new(),
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
        )
        .unwrap();

        assert!(matches!(
            snapshot.authorize_tool(&run_id, &moved_on, RunOperation::SubmitReviewCandidate),
            Err(AuthorityError::DocumentBindingMismatch)
        ));
    }

    #[test]
    fn run_mismatch_is_checked_before_the_subject() {
        // Order matters: authorize_tool checks run_id first (authority.rs:162),
        // so a wrong run on a matching subject must not read as a subject
        // mismatch.
        let subject = AuthoritySubject::ActionGuideEphemeralGuide {
            guide_digest: "ee".repeat(32),
        };
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(task_id(), TaskAttemptId::new(1), run_id(), subject.clone()),
            "rollshot-v1".to_owned(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            BTreeSet::new(),
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
        )
        .unwrap();
        let other_run = RunId::parse("run-00000000-0000-4000-8000-000000000002").unwrap();

        assert!(matches!(
            snapshot.authorize_tool(&other_run, &subject, RunOperation::SubmitReviewCandidate),
            Err(AuthorityError::RunMismatch)
        ));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent action_guide_subject_authorizes`
Expected: FAIL to compile — `AuthoritySubject` not found.

- [ ] **Step 3: Add the enum and rewire the binding**

Add to `crates/rollshot-agent/src/authority.rs`:

```rust
/// What a run holds authority over. `Document` is the Smart Redaction subject;
/// its digest input is unchanged so existing receipts stay comparable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoritySubject {
    Document(DocumentContentBinding),
    ActionGuideProject {
        project_root_sha256: [u8; 32],
        revision: u64,
        projection_digest: String,
    },
    ActionGuideEphemeralGuide {
        guide_digest: String,
    },
}
```

In `AuthorityBinding`, rename the `document_binding: DocumentContentBinding`
field to `subject: AuthoritySubject`, update `new` accordingly, and replace the
`document_binding()` accessor with:

```rust
    pub fn subject(&self) -> &AuthoritySubject {
        &self.subject
    }
```

Replace the digest input block at `authority.rs:254-256` with a per-variant
hash. **The `Document` arm keeps today's exact input bytes** — no separator, the
three fields in the same order — unless Task 7's audit came back clean, in which
case a `b"rollshot-authority-subject-document-v1\0"` separator may be added:

```rust
        match self.binding.subject() {
            AuthoritySubject::Document(binding) => {
                hasher.update(binding.base_image_digest());
                hasher.update(binding.annotation_state_digest());
                hasher.update(binding.state_id().to_le_bytes());
            }
            AuthoritySubject::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            } => {
                hasher.update(b"rollshot-authority-subject-action-guide-project-v1\0");
                hasher.update(project_root_sha256);
                hasher.update(revision.to_le_bytes());
                hasher.update(projection_digest.as_bytes());
            }
            AuthoritySubject::ActionGuideEphemeralGuide { guide_digest } => {
                hasher.update(b"rollshot-authority-subject-action-guide-ephemeral-v1\0");
                hasher.update(guide_digest.as_bytes());
            }
        }
```

Change `authorize_tool`'s second parameter to `subject: &AuthoritySubject` and
its comparison to `if subject != self.binding.subject()`. Keep the
`AuthorityError::DocumentBindingMismatch` variant name so no error contract
changes.

- [ ] **Step 4: Keep the persisted receipt key, and do not touch the digest DTO**

In `AuthoritySnapshotReceiptV1` (`authority.rs:326`-339), the field may be renamed
in Rust but the serialized key must not change, or task JSON containing
`RunContractReceiptV1` stops loading:

```rust
    #[serde(rename = "document_binding_digest")]
    pub subject_digest: String,
```

**Leave `DigestedSnapshotV1.document_binding_digest` (`authority.rs:353`) named
exactly as it is.** That struct is `Serialize`-only and its JSON is the hash input
for `compute_digest` (`authority.rs:261`-279), so renaming the Rust field renames
the JSON key and changes **every** authority snapshot digest. There is no reason
to touch it: it is private and never read back. If it is renamed anyway, it must
carry `#[serde(rename = "document_binding_digest")]`.

The receipt rename has a four-site fan-out, all struct literals in test fixtures:
`run.rs:4356`, `task_store.rs:1677`, `continuity.rs:1190`, `driver.rs:5978`.

- [ ] **Step 5: Update every caller**

```bash
rtk grep -rn "authorize_tool(\|AuthorityBinding::new(\|document_binding()" crates/ --include="*.rs"
```

The fan-out is small and already enumerated: exactly one production
`authorize_tool` caller, `tools.rs:152`
(`snapshot.authorize_tool(&ctx.run_id, &ctx.content_binding, op)`), which becomes
`&AuthoritySubject::Document(ctx.content_binding.clone())`; one test caller at
`driver.rs:5806`. `AuthorityBinding::new` has 14 callers: `audit_store/mod.rs:939`,
`eval/record.rs:353`, `eval/layer1.rs:112`, `task_store.rs:1653`, `run.rs:1207`,
`continuity.rs:1872`, `jobs.rs:1531`, `audit.rs:1369`, `tools.rs:3464/3608/3663`,
`driver.rs:2107/5707/6426`. Smart Redaction sites wrap their existing binding:
`AuthoritySubject::Document(content_binding.clone())`.

- [ ] **Step 6: Run the suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS, including the new subject tests.

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "feat(agent): generalize authority binding to AuthoritySubject"
```

---

## Task 9: `DisclosureCeiling::TextMetadataOnly`

**Files:**
- Modify: `crates/rollshot-agent/src/authority.rs:32-42` (the enum) and
  `:176-195` (`validate_model_input`)

**Interfaces:**
- Consumes: nothing.
- Produces: `DisclosureCeiling::TextMetadataOnly`, declared first so it orders
  strictly below `OcrLayoutOnly`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn text_metadata_only_orders_below_ocr_layout() {
        assert!(DisclosureCeiling::TextMetadataOnly < DisclosureCeiling::OcrLayoutOnly);
        assert!(DisclosureCeiling::OcrLayoutOnly < DisclosureCeiling::FullScreenshot);
    }

    #[test]
    fn text_metadata_only_rejects_any_attachment() {
        let snapshot = snapshot_with_disclosure(DisclosureCeiling::TextMetadataOnly);

        assert_eq!(
            snapshot.validate_model_input(&png_input(vec![1, 2, 3, 4])),
            Err(AuthorityError::DisclosureExceeded {
                ceiling: DisclosureCeiling::TextMetadataOnly,
                attachment_count: 1,
            })
        );
        assert!(snapshot
            .validate_model_input(&input_without_attachments())
            .is_ok());
    }
```

The helpers already exist with these names and need no refactor:
`snapshot_with_disclosure(disclosure)` at `authority.rs:422`,
`png_input(attachment_bytes)` at `:460`, `input_without_attachments()` at `:476`.
There is no `snapshot_with_ceiling`, `authorized_input_with_one_png`, or
`authorized_input_without_attachments` in this module —
`authorized_input_with_one_png` exists but lives in a different crate module
(`visual_annotation.rs:956`) and is not in scope here. Asserting the full
`DisclosureExceeded { ceiling, attachment_count }` payload rather than
`matches!(.., Err(DisclosureExceeded { .. }))` matches the neighbouring
`ocr_only_rejects_any_model_attachment` test (`authority.rs:519`-530) and actually
pins that the new ceiling is reported, not `OcrLayoutOnly`.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent text_metadata_only`
Expected: FAIL to compile — no variant `TextMetadataOnly`.

- [ ] **Step 3: Add the variant first in declaration order**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureCeiling {
    /// No image-derived data at all. Guide metadata text only.
    ///
    /// Declared first so ordering comparisons place it below every other
    /// ceiling. Audited on 2026-07-28: no ordering comparison exists in the
    /// codebase today, so this position matters only for future code.
    TextMetadataOnly,
    OcrLayoutOnly,
    FullScreenshot,
}
```

- [ ] **Step 4: Add the `validate_model_input` arm**

```rust
        match self.disclosure {
            DisclosureCeiling::TextMetadataOnly | DisclosureCeiling::OcrLayoutOnly => {
                if attachment_count > 0 {
                    return Err(AuthorityError::DisclosureExceeded {
                        ceiling: self.disclosure,
                        attachment_count,
                    });
                }
            }
            DisclosureCeiling::FullScreenshot => {
                // Ceiling, not requirement: zero attachments is fine.
            }
        }
```

- [ ] **Step 5: Run the suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add -A crates/rollshot-agent
rtk git commit -m "feat(agent): add a zero-image disclosure ceiling"
```

---

## Task 10: New `TaskKind` and `ArtifactKind` variants

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs:126-172`

**Interfaces:**
- Consumes: nothing.
- Produces: `TaskKind::ActionGuideCaptions`, `ArtifactKind::ActionGuideCaptions`.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn action_guide_caption_kinds_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&TaskKind::ActionGuideCaptions).unwrap(),
            "\"action_guide_captions\""
        );
        assert_eq!(
            serde_json::to_string(&ArtifactKind::ActionGuideCaptions).unwrap(),
            "\"action_guide_captions\""
        );
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent action_guide_caption_kinds`
Expected: FAIL to compile.

- [ ] **Step 3: Add both variants**

```rust
pub enum TaskKind {
    SmartRedactionAuthor,
    SmartRedactionImprove,
    ActionGuideCaptions,
}
```

```rust
pub enum ArtifactKind {
    SmartRedaction,
    ActionGuideCaptions,
}
```

Then compile and fix every non-exhaustive `match` the compiler reports. Two are
known: `continuity.rs:528` (`TaskKind` → `&str`) and `continuity.rs:553`
(`ArtifactKind` → `&str`). Use `"action_guide_captions"` in both, matching the
serde `rename_all = "snake_case"` spelling the Step 1 test pins.

- [ ] **Step 4: Run to verify it passes**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "feat(agent): add Action Guide caption task and artifact kinds"
```

---

## Task 11: Move the store to a shared module with one instance per process

**Files:**
- Create: `crates/rollshot-app/src/agent_store/mod.rs`
- Move: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`
  → `crates/rollshot-app/src/agent_store/task_store.rs`
- Move: `crates/rollshot-app/src/result_workspace/workbench/audit_store/`
  → `crates/rollshot-app/src/agent_store/audit_store/`
- Modify: `crates/rollshot-app/src/main.rs` (declare the module)
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs` (open the store once
  in the `run` entry point at `:371` and hand the `Arc` to `WorkbenchState`)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:1664` (stop
  opening it here)

**Interfaces:**
- Consumes: everything from Tasks 2-10.
- Produces:
  - `crate::agent_store::{TaskStore, TaskStoreError, StoreCommitOutcome,
    Failpoint, TaskStoreContinuitySource}` — `pub use`, because all five are
    `pub` today (`task_store.rs:55, :72, :87, :184, :1417`).
  - `crate::agent_store::{AuditJournal, AuditStoreError, TaskAuditSink}` —
    **`pub(crate) use`, not `pub use`.** Every item in `audit_store` is declared
    `pub(crate)` (`audit_store/mod.rs:94, :190, :849`), and `pub use` of a
    `pub(crate)` item is a hard error: E0364 for the struct, E0365 for the enum.
    Verified on review by compiling the exact re-export.
  - `crate::agent_store::open_process_store(config_dir: &std::path::Path)
    -> Result<std::sync::Arc<TaskStore>, TaskStoreError>` — the only production
    constructor.

`TaskAuditSink` was absent from the earlier draft's re-export list; `run.rs:1483`-1485
imports it as `super::audit_store::TaskAuditSink`, so omitting it leaves that file
without a path after the move.

- [ ] **Step 1: Write the failing test**

Create `crates/rollshot-app/src/agent_store/mod.rs` with only the test first, so
the module exists and the test drives the move:

```rust
#[cfg(test)]
mod placement_tests {
    #[test]
    fn store_module_is_reachable_without_action_guide() {
        // The store is unconditional: only Action Guide task-kind construction
        // sites are feature-gated. This test exists in both feature configs.
        let dir = tempfile::tempdir().unwrap();
        let store = super::open_process_store(dir.path()).unwrap();

        assert!(store.tasks_dir().exists());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app store_module_is_reachable`
Expected: FAIL to compile — `open_process_store` not found.

- [ ] **Step 3: Move the files and re-export**

```bash
rtk git mv crates/rollshot-app/src/result_workspace/workbench/task_store.rs \
           crates/rollshot-app/src/agent_store/task_store.rs
rtk git mv crates/rollshot-app/src/result_workspace/workbench/audit_store \
           crates/rollshot-app/src/agent_store/audit_store
```

Write `crates/rollshot-app/src/agent_store/mod.rs`:

```rust
//! Process-wide agent task persistence.
//!
//! Exactly one [`TaskStore`] exists per process. `TaskStore::acquire_lock`
//! takes a blocking fs4 exclusive lock per operation, and two instances in one
//! process hold distinct file descriptors that flock treats as unrelated
//! holders: they block each other, and nested acquisition self-deadlocks.
//!
//! This module is unconditional. Only Action Guide task-kind construction
//! sites are gated on the `action-guide` feature.

pub mod audit_store;
pub mod task_store;

// `audit_store`'s items are all `pub(crate)`; `pub use` of a `pub(crate)` item
// is E0364/E0365, so these re-exports must be crate-visible too.
pub(crate) use audit_store::{AuditJournal, AuditStoreError, TaskAuditSink};
pub use task_store::{
    Failpoint, StoreCommitOutcome, TaskStore, TaskStoreContinuitySource, TaskStoreError,
};

/// Open the single process-wide task store.
pub fn open_process_store(
    config_dir: &std::path::Path,
) -> Result<std::sync::Arc<TaskStore>, TaskStoreError> {
    let store = TaskStore::open(config_dir)?;
    tracing::info!(
        target: "rollshot::app::agent_store",
        tasks_dir = %store.tasks_dir().display(),
        "process task store opened"
    );
    Ok(std::sync::Arc::new(store))
}
```

Declare the module in `crates/rollshot-app/src/main.rs` next to the other `mod`
declarations:

```rust
mod agent_store;
```

- [ ] **Step 4: Repoint every importer**

```bash
rtk grep -rn "workbench::task_store\|workbench::audit_store\|super::task_store\|super::audit_store\|super::super::task_store" crates/rollshot-app/src --include="*.rs"
```

Replace each with the `crate::agent_store::` path. Inside the moved files,
`super::` references to the old parent must become `crate::agent_store::` or
`crate::result_workspace::workbench::` depending on what they pointed at — the
compiler reports each one. Two useful specifics: `audit_store/mod.rs:850` and
`:854` say `super::task_store::TaskStore` and stay unchanged, because both files
move into the same new parent; `audit_store/reconcile.rs:127` uses the absolute
path `crate::result_workspace::workbench::audit_store::record::{..}` and must
become `crate::agent_store::audit_store::record::{..}`.

- [ ] **Step 5: Open the store once per process, at the workspace entry point**

**Corrected on review — there is no shared application root.** `main.rs:60`-119
dispatches on `LaunchMode` into mutually exclusive entry points, and each one
starts its own iced application: the Smart Redaction path reaches
`result_workspace::run` (`result_workspace/mod.rs:371`, via
`post_capture.rs`/`run_open_image`), and the Action Guide path reaches
`timeline_workspace::run` (`timeline_workspace/mod.rs:864`, via
`run_action_guide_record`). The two workspaces never run in the same process, so
"one instance per process" is satisfied by opening the store exactly once inside
whichever workspace root this process is running, not by a common owner.

Concretely, in `crates/rollshot-app/src/result_workspace/mod.rs`, inside `run`'s
boot closure (before `ResultWorkspace` state is handed to iced), call
`crate::agent_store::open_process_store(&config_dir)` once and stash the
`Result` on the workspace state. Then in
`crates/rollshot-app/src/result_workspace/update.rs:1664`, delete the
`TaskStore::open(&config_dir)` call and read the already-opened handle instead.
`workbench.task_store` keeps its existing `Option<std::sync::Arc<TaskStore>>`
type, so only the source of the value changes.

**Preserve the open-failure surface.** Today `update.rs:1665`-1679 sets
`wb.error = Some(WorkbenchError::StorePersist { message: format!("task store
unavailable: {e}") })` and logs at `target: "rollshot::app::agent_audit_store"`
when `open` fails. Moving the open earlier must not lose that: store the error
string alongside the handle at boot and set the same `wb.error` with the same
message on the `Message::SmartRedaction` path. Add a test that a store that
cannot be opened still produces that exact `WorkbenchError::StorePersist`
message.

Task 16 does the same thing for `timeline_workspace::run`.

- [ ] **Step 6: Run all three configurations**

Run: `rtk cargo test -p rollshot-app --features action-guide`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app`
Expected: PASS.

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add -A crates/rollshot-app
rtk git commit -m "refactor(app): move the agent task store to a shared module"
```

---

## Task 12: Two-domain concurrency test

**Files:**
- Test: `crates/rollshot-app/src/agent_store/task_store.rs` (its test module)

**Interfaces:**
- Consumes: `open_process_store`, the `SourceBinding` variants, `new_v3`.
- Produces: nothing consumed later.

- [ ] **Step 1: Write the characterization test**

This is a characterization test of the lock, not a red-green pair: the behavior it
asserts must already hold the moment Task 11 lands. There is no RED step, and
Step 2 expects PASS on the first run. The failure mode it guards against is a
hang, not a wrong value.

```rust
    #[test]
    fn concurrent_audited_creates_from_two_domains_both_commit() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();

        // Distinct deterministic ids; the tempdir is fresh so no uniqueness
        // across runs is needed. `ProductTaskId::new_v4()` does not exist.
        let smart_task_id =
            ProductTaskId::parse("task-00000000-0000-4000-8000-00000000000a").unwrap();
        let caption_task_id =
            ProductTaskId::parse("task-00000000-0000-4000-8000-00000000000b").unwrap();

        let smart = ProductTaskSnapshot::new_v3(
            smart_task_id,
            TaskKind::SmartRedactionAuthor,
            SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 0, "p".into(), None),
            1_000,
        )
        .unwrap();
        let captions = ProductTaskSnapshot::new_v3(
            caption_task_id,
            TaskKind::ActionGuideCaptions,
            SourceBinding::ActionGuideProject {
                project_root_sha256: [3u8; 32],
                revision: 1,
                projection_digest: "ab".repeat(32),
            },
            1_000,
        )
        .unwrap();

        let a = store.clone();
        let b = store.clone();
        let smart_id = smart.task_id().clone();
        let captions_id = captions.task_id().clone();

        let ha = std::thread::spawn(move || {
            a.create_audited(&smart, AuditEventId::new_v4(), 1_000)
        });
        let hb = std::thread::spawn(move || {
            b.create_audited(&captions, AuditEventId::new_v4(), 1_000)
        });

        ha.join().unwrap().expect("smart redaction create failed");
        hb.join().unwrap().expect("caption create failed");

        // Both files exist AND both survived the schema-3 load guard relaxed in
        // Task 4, with their own domains intact.
        let smart_loaded = store.load(&smart_id).expect("smart redaction load failed");
        let caption_loaded = store.load(&captions_id).expect("caption load failed");
        assert_eq!(smart_loaded.store_schema_version(), 3);
        assert_eq!(caption_loaded.kind(), TaskKind::ActionGuideCaptions);
        assert!(matches!(
            caption_loaded.source_binding(),
            SourceBinding::ActionGuideProject { revision: 1, .. }
        ));

        // Two audited creates means two journals, each with its own TaskCreated.
        for id in [&smart_id, &captions_id] {
            let kinds: Vec<_> = store
                .committed_audit_events(id)
                .unwrap()
                .into_iter()
                .map(|e| e.event().kind())
                .collect();
            assert_eq!(kinds, vec![AuditEventKindV1::TaskCreated], "{id:?}");
        }
    }
```

The test module needs `use rollshot_agent::audit::AuditEventKindV1;` alongside the
existing `AuditEventId` import.

- [ ] **Step 2: Run it**

Run: `rtk cargo test -p rollshot-app --features action-guide concurrent_audited_creates_from_two_domains`
Expected: PASS. If it hangs, a nested lock acquisition exists — find it with
`rtk grep -n "acquire_lock" crates/rollshot-app/src/agent_store/task_store.rs`
and confirm no locked function calls another locked function. Known-safe callers
today: `load` takes no lock (`task_store.rs:714`); `create_audited` (`:839`),
`transition_audited` (`:928`), `append_standalone_audit` (`:1006`),
`reconcile_task_audit` (`:1024`), and `compare_and_swap` (`:736`) each take one and
call no other locking method.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-app/src/agent_store/task_store.rs
rtk git commit -m "test(app): cover concurrent audited writes from two domains"
```

---

## Task 13: Caption skill package and resolver

**Files:**
- Create: `crates/rollshot-agent/skills/action-guide-captions/skill.toml`
- Create: `crates/rollshot-agent/skills/action-guide-captions/SKILL.md`
- Modify: `crates/rollshot-agent/src/skills.rs` (bundled resolver at
  `skills.rs:977-1019`, plus the catalog-size assertion at `:2054`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `ACTION_GUIDE_CAPTIONS_PACKAGE_ID: &str = "action-guide-captions"`
  - `bundled_action_guide_captions_use() -> Option<SkillUse>`

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/rollshot-agent/src/skills.rs`:

```rust
    /// The instruction text as it stood in `build_caption_prompt` on
    /// 2026-07-28, before the skill move. Byte-identical preservation of this
    /// text is the behavior evidence for plan Task 14.
    const CAPTION_INSTRUCTION_BASELINE: &str = "Suggest concise Action Guide titles and one-sentence captions for these reviewed workflow steps.\nPrefer calling the submit_caption_suggestions tool. If tool calling is unavailable, return only JSON in the same schema.\nUse the source values exactly. Omit a title by using null when the current title is already good. Do not invent raw typed text.";

    #[test]
    fn bundled_caption_skill_body_matches_the_recorded_instruction_text() {
        let use_ = bundled_action_guide_captions_use().expect("caption skill must resolve");

        assert_eq!(use_.package_id().as_str(), "action-guide-captions");
        assert_eq!(use_.source_authority().as_str(), "rollshot.bundled");
        assert_eq!(
            use_.body().trim_end(),
            CAPTION_INSTRUCTION_BASELINE,
            "skill body must preserve the recorded instruction text verbatim"
        );
        assert_eq!(use_.digest().len(), 64, "digest must be a hex sha256");
        assert!(
            use_.digest()
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "digest must be lowercase hex: {}",
            use_.digest()
        );
    }

    #[test]
    fn bundled_caption_skill_golden_digest_stable() {
        // Golden digest — update only when SKILL.md or skill.toml content
        // changes. Mirrors bundled_smart_redaction_golden_digest_stable
        // (skills.rs:2068). Fill in the value the first run prints; do not
        // leave this test asserting a placeholder.
        let skill_use = bundled_action_guide_captions_use().expect("caption skill must resolve");
        let expected = "<record the actual digest from the first run>";
        assert_eq!(
            skill_use.digest(),
            expected,
            "digest mismatch — if SKILL.md or skill.toml changed, update the golden digest"
        );
    }

    #[test]
    fn bundled_caption_skill_body_below_16kib() {
        let skill_use = bundled_action_guide_captions_use().unwrap();
        assert!(skill_use.body().len() <= 16 * 1024);
    }
```

`SkillCatalogLimits::v1()` caps `max_body_bytes` at 16 KiB and `max_entries` at
1000 (`skills.rs:171`-178), so registering a second bundled package is within
limits.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent bundled_caption_skill_body`
Expected: FAIL to compile — `bundled_action_guide_captions_use` not found.

- [ ] **Step 3: Create the package files**

`crates/rollshot-agent/skills/action-guide-captions/skill.toml`:

```toml
schema_version = 1
package_id = "action-guide-captions"
name = "Action Guide Captions"
description = "Suggest reviewable Action Guide titles and captions."
declared_version = "1"
main = "SKILL.md"
```

`crates/rollshot-agent/skills/action-guide-captions/SKILL.md` — exactly the
three recorded sentences, nothing added:

```markdown
Suggest concise Action Guide titles and one-sentence captions for these reviewed workflow steps.
Prefer calling the submit_caption_suggestions tool. If tool calling is unavailable, return only JSON in the same schema.
Use the source values exactly. Omit a title by using null when the current title is already good. Do not invent raw typed text.
```

- [ ] **Step 4: Add the resolver**

Mirror the existing bundled Smart Redaction resolver at `skills.rs:977-1010`.
Read that block first and follow its exact structure — a package-id constant, an
`include_str!` body, a file list, a lazily built `CatalogBuildReport`, and a
resolver returning `Option<SkillUse>`:

```rust
/// Well-known package ID for the bundled Action Guide captions skill.
pub const ACTION_GUIDE_CAPTIONS_PACKAGE_ID: &str = "action-guide-captions";

const CAPTIONS_BUNDLED_BODY: &str =
    include_str!("../skills/action-guide-captions/SKILL.md");
const CAPTIONS_BUNDLED_MANIFEST: &str =
    include_str!("../skills/action-guide-captions/skill.toml");
```

Register both bundled packages through the same catalog build so the
bounded-catalog limits apply to the pair: extend the `SkillSource::Bundled` vec
inside `BUNDLED_REPORT` (`skills.rs:983`-993) with a second
`(ACTION_GUIDE_CAPTIONS_PACKAGE_ID, vec![("skill.toml", ..), ("SKILL.md", ..)])`
tuple. Then add `bundled_action_guide_captions_use()`, a copy of
`bundled_smart_redaction_use()` (`skills.rs:1005`-1019) with the package id
swapped.

- [ ] **Step 4a: Update the pre-existing catalog-size assertion**

`bundled_smart_redaction_manifest_accepted` asserts
`report.catalog.entries.len() == 1` at `skills.rs:2054`. Registering a second
package makes that 2, so the assertion **will fail** unless it is updated in this
task. Change it to `2` and keep the surrounding `omitted_count == 0` and
`diagnostics.is_empty()` assertions, which are the ones that actually prove the
new manifest was accepted. Rename nothing else in that test.

- [ ] **Step 5: Run the tests**

Run: `rtk cargo test -p rollshot-agent skills`
Expected: PASS, including the pre-existing bundled Smart Redaction tests with the
Step 4a edit. Record the caption digest the golden test prints and substitute it.

- [ ] **Step 6: Commit**

```bash
rtk git add -A crates/rollshot-agent
rtk git commit -m "feat(agent): bundle the Action Guide captions skill"
```

---

## Task 14: Caption prompt composition

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs:150-190`
  (`compose_smart_redaction_prompt` neighborhood, `AgentTaskProfile`)

**Interfaces:**
- Consumes: `bundled_action_guide_captions_use`, `ACTION_GUIDE_CAPTIONS_PACKAGE_ID`.
- Produces:
  - `AgentTaskProfile::Captions`
  - `compose_caption_prompt(skill_use: &SkillUse) -> Result<String, DriverError>`
  - `CAPTION_SYSTEM_ENVELOPE: &str` — replaces today's inline system prompt
    literal.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn caption_prompt_wraps_the_skill_body_with_its_digest() {
        let use_ = crate::skills::bundled_action_guide_captions_use().unwrap();

        let prompt = compose_caption_prompt(&use_).unwrap();

        assert!(prompt.starts_with(CAPTION_SYSTEM_ENVELOPE));
        assert!(prompt.contains("<rollshot-skill package=\"action-guide-captions\""));
        assert!(prompt.contains(use_.digest()));
        assert!(prompt.contains("Suggest concise Action Guide titles"));
        assert!(prompt.ends_with("</rollshot-skill>"));
    }

    #[test]
    fn caption_prompt_rejects_a_foreign_package() {
        let smart = crate::skills::bundled_smart_redaction_use().unwrap();

        assert!(compose_caption_prompt(&smart).is_err());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent caption_prompt_wraps`
Expected: FAIL to compile — `compose_caption_prompt` not found.

- [ ] **Step 3: Implement the composer and the profile variant**

```rust
/// System envelope for a caption run. Replaces the inline literal that lived in
/// `caption_agent::suggest_captions_with_timeout` before the skill move.
pub(crate) const CAPTION_SYSTEM_ENVELOPE: &str =
    "You produce compact structured suggestions for Rollshot Action Guide captions.";

pub(crate) fn compose_caption_prompt(
    skill_use: &crate::skills::SkillUse,
) -> Result<String, DriverError> {
    if skill_use.package_id().as_str() != crate::skills::ACTION_GUIDE_CAPTIONS_PACKAGE_ID {
        return Err(DriverError::AgentProtocolFailure(format!(
            "unexpected skill package: {}",
            skill_use.package_id().as_str()
        )));
    }
    if skill_use.source_authority().as_str() != "rollshot.bundled" {
        return Err(DriverError::AgentProtocolFailure(format!(
            "unexpected skill authority: {}",
            skill_use.source_authority().as_str()
        )));
    }

    Ok(format!(
        "{envelope}\n\n<rollshot-skill package=\"{pkg}\" digest=\"{digest}\">\n{body}\n</rollshot-skill>",
        envelope = CAPTION_SYSTEM_ENVELOPE,
        pkg = skill_use.package_id().as_str(),
        digest = skill_use.digest(),
        body = skill_use.body(),
    ))
}
```

Add the profile variant. **Keep the `#[allow(dead_code)]` and add an arm to both
methods** — `system_prompt` and `terminal_tools` are `match self` over the enum
(`driver.rs:181`-194), so a bare new variant is a non-exhaustive-match compile
error, and a variant never constructed outside `#[cfg(test)]` code is a
`dead_code` warning that `rtk cargo clippy --workspace --all-targets -- -D
warnings` (Task 22) turns into a failure:

```rust
pub(crate) enum AgentTaskProfile {
    #[allow(dead_code)]
    VisualAnnotation,
    /// Constructed only by tests today: the caption run receives its composed,
    /// digest-bearing system prompt as an owned `String` through
    /// `SingleSubmitProfile` (Task 15), because `system_prompt` returns
    /// `&'static str`. The variant still owns the terminal-tool declaration.
    #[allow(dead_code)]
    Captions,
}

impl AgentTaskProfile {
    pub(crate) fn system_prompt(&self) -> &'static str {
        match self {
            Self::VisualAnnotation => VISUAL_ANNOTATION_SYSTEM_PROMPT,
            // Envelope only. The skill body and digest are appended by
            // `compose_caption_prompt`, which cannot return `&'static str`.
            Self::Captions => CAPTION_SYSTEM_ENVELOPE,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn terminal_tools(&self) -> &'static [&'static str] {
        match self {
            Self::VisualAnnotation => &["submit_visual_annotation_suggestions"],
            Self::Captions => &["submit_caption_suggestions"],
        }
    }
}
```

Do not change the visual annotation path: `run_visual_annotation_with_provider`
keeps calling `AgentTaskProfile::VisualAnnotation.system_prompt()`
(`driver.rs:1805`-1809) untouched.

- [ ] **Step 3a: Extend the profile-parity test**

`visual_annotation_profile_advertises_only_submit_visual_annotation_suggestions`
(`driver.rs:2007`-2016) is the existing parity test and is the only non-`cfg(test)`
reader of `terminal_tools`. Add the caption half beside it so the new variant is
actually exercised:

```rust
    #[test]
    fn caption_profile_advertises_only_submit_caption_suggestions() {
        assert_eq!(
            AgentTaskProfile::Captions.terminal_tools(),
            &["submit_caption_suggestions"],
        );
        assert_eq!(
            AgentTaskProfile::Captions.system_prompt(),
            CAPTION_SYSTEM_ENVELOPE,
        );
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `rtk cargo test -p rollshot-agent caption_prompt`
Expected: PASS, both tests.

Run: `rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings`
Expected: clean. This is checked here rather than only at Task 22 because a new
enum variant is the most likely source of a `dead_code` denial.

- [ ] **Step 5: Commit**

```bash
rtk git add -A crates/rollshot-agent
rtk git commit -m "feat(agent): compose the caption system prompt from its skill"
```

---

## Task 15: Extract the single-submit bounded profile

A mechanical extraction. `run_visual_annotation_with_provider` is NOT modified;
the new function is a parameterized sibling that captions uses and Slice B later
adopts.

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`. `run_single_submit_with_provider`
  goes inside the `impl AgentRunner` block, after
  `run_visual_annotation_with_provider` — whose body is
  `driver.rs:1692`-**1969**, with the `impl` block closing at `:1970`.
  `map_budget_error_to_single_submit` goes beside the existing free function
  `map_budget_error_to_visual_annotation` at `driver.rs:1972`-1985, outside the
  `impl`. (`driver.rs:1989` is the `#[cfg(test)]` attribute, not the end of the
  function.)
- Modify: `crates/rollshot-agent/src/visual_annotation.rs` — widen the visibility
  of the existing `ProviderAdapter` test mock so Step 1's tests can reuse it
  instead of copying it. No production code in that file changes.

**Interfaces:**
- Consumes: `AuthoritySubject`, `RunOperation`, `AgentTaskProfile::Captions`.
- Produces:
  - `SingleSubmitTerminal { Submitted { arguments: serde_json::Value },
    TextCompleted { text: String }, Cancelled,
    BudgetExhausted { dimension: BudgetDimension }, ProviderFailure,
    ProtocolFailure, AuthorityDenied { operation: RunOperation } }`
  - `SingleSubmitProfile<'a> { tool_definition: ToolDefinition, tool:
    std::sync::Arc<dyn ...>, skill_use: &'a SkillUse, system_prompt: String,
    required_operation: RunOperation, tracing_target: &'static str }`
  - `SingleSubmitProfile::from_skill(skill_use: &'a SkillUse, system_prompt:
    String, tool_definition: ToolDefinition, tool: std::sync::Arc<dyn ...>,
    required_operation: RunOperation, tracing_target: &'static str)
    -> Result<Self, DriverError>` — the only constructor. It rejects a
    `system_prompt` that does not contain `skill_use.digest()`, so a caller
    cannot pass an arbitrary prompt with no skill behind it. This is what keeps
    the compose-from-skill invariant checkable after Task 14 moved composition to
    the caller.
  - `AgentRunner::run_single_submit_with_provider(&self, profile:
    SingleSubmitProfile, input: AuthorizedModelInput, provider: &dyn
    ProviderAdapter, budget: RunBudget, cancellation: &RunCancellation,
    authority: &AuthoritySnapshot, subject: &AuthoritySubject, audit_sink:
    Option<&dyn AuditAppendSink>) -> SingleSubmitTerminal`

- [ ] **Step 1: Write the failing tests**

**Corrected on review — the model is not in `driver.rs`.** There is no test of
`run_visual_annotation_with_provider` in `driver.rs`; `driver.rs:2007` is only the
profile-parity test. `tool_call_delta_name` / `tool_call_delta_args`
(`driver.rs:2025`, `:2034`) build rig `StreamedAssistantContent` items for
`AgentRunner::run`'s `model_fn` closure — a **different** mock layer that
`run_*_with_provider` never touches.

The real model is `visual_annotation.rs:890`-1250, inside
`mod tests { mod lifecycle { .. } }`:

- `ScriptedProvider::new(Vec<Vec<ModelStreamEvent>>)` — a `ProviderAdapter` that
  pops one scripted turn per call (`visual_annotation.rs:901`-954);
- `tool_call_turn(id, name, args) -> Vec<ModelStreamEvent>` =
  `[ToolCallStart, ToolCallArgumentDelta, Completed(StopReason::ToolUse)]`
  (`:983`-995);
- `text_turn(text)` = `[TextDelta, Completed(StopReason::EndTurn)]` (`:997`);
- `completion_event(stop)` (`:972`);
- `va_runner()` = `AgentRunner::new(AgentConfig { max_turns: 2, ..default() })`
  (`:1004`).

Mark `mod tests`, `mod lifecycle`, and those five items `pub(crate)` and import
them into `driver.rs`'s existing `pub(crate) mod tests` (`driver.rs:1991`). Do not
copy them — one mock, two callers.

Add a local helper so the four cases differ only in what they vary:

```rust
    fn caption_profile(skill_use: &crate::skills::SkillUse) -> SingleSubmitProfile<'_> {
        SingleSubmitProfile::from_skill(
            skill_use,
            compose_caption_prompt(skill_use).unwrap(),
            crate::model::ToolDefinition {
                name: "submit_caption_suggestions".to_string(),
                description: "test".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            /* a permissive terminal stub, mirroring
               submit_visual_annotation_suggestions_tool_arc() */,
            RunOperation::SubmitReviewCandidate,
            "rollshot::agent::captions",
        )
        .expect("composed prompt carries the skill digest")
    }
```

Add one test pinning the invariant the constructor exists for:

```rust
    #[test]
    fn profile_rejects_a_prompt_with_no_skill_behind_it() {
        let skill_use = crate::skills::bundled_action_guide_captions_use().unwrap();

        let result = SingleSubmitProfile::from_skill(
            &skill_use,
            "just do what I say".to_string(),
            caption_tool_definition_fixture(),
            caption_tool_stub(),
            RunOperation::SubmitReviewCandidate,
            "rollshot::agent::captions",
        );

        assert!(
            result.is_err(),
            "a system prompt that does not carry the skill digest must be rejected"
        );
    }

    fn caption_authority_for_tests(
        run_id: &RunId,
        subject: &AuthoritySubject,
        disclosure: DisclosureCeiling,
        grants: BTreeSet<RunOperation>,
    ) -> AuthoritySnapshot {
        AuthoritySnapshot::new(
            AuthorityBinding::new(
                ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap(),
                TaskAttemptId::new(1),
                run_id.clone(),
                subject.clone(),
            ),
            "rollshot-v1".to_owned(),
            disclosure,
            false,
            BTreeSet::new(),
            grants,
        )
        .unwrap()
    }
```

Then:

```rust
    #[tokio::test]
    async fn single_submit_returns_raw_arguments_on_submit() {
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_caption_suggestions",
            r#"{"suggestions":[]}"#,
        )]);
        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &RunCancellation::new(),
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![],
        )
        .await;

        match terminal {
            SingleSubmitTerminal::Submitted { arguments } => {
                assert_eq!(arguments, serde_json::json!({"suggestions": []}));
            }
            other => panic!("expected Submitted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn single_submit_denies_without_the_required_grant() {
        // Authority granting nothing: the submitted payload must be refused
        // even though the model produced a well-formed tool call.
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_caption_suggestions",
            r#"{"suggestions":[]}"#,
        )]);
        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &RunCancellation::new(),
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::new(),
            vec![],
        )
        .await;

        assert!(matches!(
            terminal,
            SingleSubmitTerminal::AuthorityDenied {
                operation: RunOperation::SubmitReviewCandidate
            }
        ));
    }

    #[tokio::test]
    async fn single_submit_rejects_attachments_above_the_ceiling() {
        // One PNG attachment under TextMetadataOnly. The provider script is
        // deliberately a valid submit, so the only reason this can fail is the
        // pre-flight disclosure check.
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_caption_suggestions",
            r#"{"suggestions":[]}"#,
        )]);
        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &RunCancellation::new(),
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![vec![0x89, 0x50, 0x4E, 0x47]],
        )
        .await;

        assert!(matches!(terminal, SingleSubmitTerminal::ProtocolFailure));
        assert_eq!(
            provider.request_count(),
            0,
            "the provider must never be called once the ceiling is exceeded"
        );
    }

    #[tokio::test]
    async fn single_submit_reports_cancellation_before_the_first_turn() {
        let cancellation = RunCancellation::new();
        cancellation.cancel();
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_caption_suggestions",
            r#"{"suggestions":[]}"#,
        )]);

        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &cancellation,
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![],
        )
        .await;

        assert!(matches!(terminal, SingleSubmitTerminal::Cancelled));
        assert_eq!(provider.request_count(), 0);
    }

    #[tokio::test]
    async fn single_submit_reports_wall_time_exhaustion() {
        // `check_wall_time` uses `elapsed >= budget.wall_time`
        // (runtime.rs:234), so a zero budget trips on the first loop check
        // deterministically — no sleeping, no flake. Task 16 maps this exact
        // terminal back to "Caption suggestions timed out."
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_caption_suggestions",
            r#"{"suggestions":[]}"#,
        )]);
        let budget = RunBudget {
            wall_time: std::time::Duration::ZERO,
            ..caption_run_budget_for_tests()
        };

        let terminal = run_caption_profile(
            &provider,
            budget,
            &RunCancellation::new(),
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![],
        )
        .await;

        assert!(matches!(
            terminal,
            SingleSubmitTerminal::BudgetExhausted {
                dimension: BudgetDimension::WallTime
            }
        ));
    }

    #[tokio::test]
    async fn single_submit_reports_provider_failure() {
        let provider = ScriptedProvider::new(vec![vec![ModelStreamEvent::Error(
            crate::model::ModelError::ProviderFailure("rate limited".to_string()),
        )]]);

        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &RunCancellation::new(),
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![],
        )
        .await;

        assert!(matches!(terminal, SingleSubmitTerminal::ProviderFailure));
    }

    #[tokio::test]
    async fn single_submit_reports_protocol_failure_when_the_model_only_talks() {
        // The pre-migration caption path fell back to parsing JSON out of
        // assistant text. The single-submit profile has no text path: a
        // completion with no terminal tool call is a protocol failure
        // (driver.rs:1960). This test is the durable record of that change.
        let provider = ScriptedProvider::new(vec![text_turn(
            r#"{"suggestions":[{"source":10,"caption":"x","confidence":0.5}]}"#,
        )]);

        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &RunCancellation::new(),
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![],
        )
        .await;

        assert!(matches!(terminal, SingleSubmitTerminal::ProtocolFailure));
    }

    #[tokio::test]
    async fn single_submit_reports_protocol_failure_on_a_foreign_tool_name() {
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_visual_annotation_suggestions",
            r#"{"suggestions":[]}"#,
        )]);

        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &RunCancellation::new(),
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![],
        )
        .await;

        assert!(matches!(terminal, SingleSubmitTerminal::ProtocolFailure));
    }
```

`ScriptedProvider` already records every request it receives
(`visual_annotation.rs:940`: `self.requests.lock().unwrap().push(request)`); add a
`pub(crate) fn request_count(&self) -> usize` accessor so the two
never-called-the-provider assertions above are real rather than implied.
`caption_run_budget_for_tests()` is a local copy of the Task 16 budget shape; Task
16 replaces it with `crate::captions::caption_run_budget()` once that exists.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent single_submit_returns_raw_arguments`
Expected: FAIL to compile — `SingleSubmitTerminal` not found.

- [ ] **Step 3: Add the terminal and the profile input**

```rust
/// Outcome of a bounded single-submit-tool run. Semantic decoding belongs to the
/// caller: a schema-agnostic profile cannot tell a suggestion batch from a model
/// declining to suggest, nor a usable text payload from prose.
#[derive(Debug)]
pub enum SingleSubmitTerminal {
    Submitted { arguments: serde_json::Value },
    /// The model finished without calling the terminal tool, returning text.
    ///
    /// Handing the text back preserves the caption flow's pre-migration
    /// fallback, in which `parse_caption_response` decodes JSON from the
    /// assistant message when tool calling is unavailable — the behavior the
    /// bundled skill body still promises. A caller that has no text path maps
    /// this to a protocol failure, which is what Slice B does to keep visual
    /// annotation's behavior identical.
    TextCompleted { text: String },
    Cancelled,
    BudgetExhausted { dimension: BudgetDimension },
    ProviderFailure,
    ProtocolFailure,
    AuthorityDenied { operation: RunOperation },
}
```

- [ ] **Step 4: Copy the run body and apply exactly seven substitutions**

Copy the whole body of `run_visual_annotation_with_provider`
(`driver.rs:1692`-1969) into `run_single_submit_with_provider`. Then apply these
substitutions and nothing else:

1. Every `VisualAnnotationRunTerminal::X` return becomes
   `SingleSubmitTerminal::X`. `map_budget_error_to_visual_annotation`
   (`driver.rs:1972`-1985) becomes a new free function
   `map_budget_error_to_single_submit` with the same two arms.
2. `submit_visual_annotation_suggestions_definition()` (`:1725`) and
   `submit_visual_annotation_suggestions_tool_arc()` (`:1730`) become
   `profile.tool_definition.clone()` and `profile.tool.clone()`.
3. `AgentTaskProfile::VisualAnnotation.system_prompt().to_string()`
   (`:1805`-1809) becomes `profile.system_prompt.clone()`.
4. Every `target: "rollshot::agent::visual_annotation"` becomes
   `target: profile.tracing_target`.
5. **Both uses of the `SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS` constant become
   `profile.tool_definition.name`**: the incoming tool-name check at
   `driver.rs:1870` and the `terminal_tools` set built at `:1907`-1910. This
   substitution was missing from the earlier draft; without it a caption run
   rejects its own tool call by name at `:1878` and can never reach `Submitted`.
   The `use crate::visual_annotation::{..}` import at `:1700`-1704 drops
   `decode_visual_annotation_terminal`,
   `submit_visual_annotation_suggestions_definition`,
   `submit_visual_annotation_suggestions_tool_arc`,
   `VisualAnnotationRunTerminal`, and `SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS`
   entirely.
6. Only the `decode_visual_annotation_terminal(&pending.tool_call.function
   .arguments)` call and its two error branches (`driver.rs:1882`-1894) are
   removed; bind `let arguments = pending.tool_call.function.arguments.clone();`
   in their place. **Keep the rest of the `CallTools` arm exactly as it is** —
   the `tool_calls: 1` budget charge (`:1896`-1901), the registry
   `execute_calls` round trip (`:1903`-1946), `rig_run.tool_results`
   (`:1948`-1955), and `tracker.apply_turn()` (`:1957`). Removing them would
   make `budget.tool_calls` and `budget.result_bytes` unenforced and would leave
   the rig state machine unadvanced. Replace only the final `return decoded;`
   (`:1958`) with the Step 5 authorization check followed by
   `return SingleSubmitTerminal::Submitted { arguments };`.
7. **The `AgentRunStep::Done(_)` arm stops being a protocol failure.** At
   `driver.rs:1960`-1965 the visual annotation version returns
   `ProtocolFailure` when the model completes without a terminal tool call.
   Return the accumulated assistant text instead, so the caller keeps its
   pre-migration text fallback:

   ```rust
                   rig_core::agent::run::AgentRunStep::Done(_) => {
                       tracing::debug!(
                           target: profile.tracing_target,
                           "model completed without a terminal tool call"
                       );
                       return SingleSubmitTerminal::TextCompleted {
                           text: std::mem::take(&mut last_assistant_text),
                       };
                   }
   ```

   `last_assistant_text` is already threaded through `drive_streamed_turn`
   (`driver.rs:1746`, `:1821`), so no new plumbing is needed. Slice B maps
   `TextCompleted` to `ProtocolFailure` to preserve visual annotation's
   behavior exactly; this task does not touch that path.

- [ ] **Step 5: Add the four behaviors the visual annotation version lacks**

Insert the disclosure check in the pre-flight block, immediately after the
existing cancellation check at `driver.rs:1708`-1710 and **before**
`input.take_model_attachments()` at `:1712`. Order is load-bearing:
`validate_model_input` reads `input.attachments()` (`authority.rs:181`), and
`take_model_attachments` empties it, so a check placed after the take would pass
unconditionally.

```rust
        if let Err(err) = authority.validate_model_input(&input) {
            tracing::warn!(
                target: profile.tracing_target,
                error = %err,
                "model input exceeded the disclosure ceiling"
            );
            return SingleSubmitTerminal::ProtocolFailure;
        }
```

Insert the authorization check immediately before returning `Submitted`:

```rust
                        if let Err(err) = authority.authorize_tool(
                            authority.run_id(),
                            subject,
                            profile.required_operation,
                        ) {
                            tracing::warn!(
                                target: profile.tracing_target,
                                error = %err,
                                operation = ?profile.required_operation,
                                "submit denied by authority"
                            );
                            if let Some(sink) = audit_sink {
                                append_authority_denied(
                                    sink,
                                    authority,
                                    &profile.tool_definition.name,
                                    profile.required_operation,
                                )
                                .await;
                            }
                            return SingleSubmitTerminal::AuthorityDenied {
                                operation: profile.required_operation,
                            };
                        }
```

**`AuthoritySnapshot` has no `binding()` accessor.** All of its fields are private
and it exposes no getter for the binding (`authority.rs:106`-117); the run id
comes from `AuthoritySnapshot::run_id(&self) -> &RunId` at `authority.rs:227`.
The sibling accessors, if needed, are `task_id()` (`:217`), `attempt_id()`
(`:222`), `disclosure()` (`:237`), and `digest()` (`:247`).

`append_authority_denied` is a small private helper wrapping the existing
`AuditAppendSink` call shape at `driver.rs:1571`-1618:
`crate::audit::authority_denied_envelope(auth, tool_name, format!("{op:?}"),
crate::audit::AuditEventId::new_v4(), now)` followed by `sink.append(envelope)
.await`. Reuse that shape verbatim, including the `AuditAppendError::AppendFailed`
branch. In the single-submit profile an append failure must not silently vanish:
log at `error` on `profile.tracing_target` and still return
`SingleSubmitTerminal::AuthorityDenied`, because the denial is the run's terminal
reason (spec §6) and Task 16 maps an audit-append failure to
`TaskTerminal::AuditFailure` on the store side.

The skill use reaches the run through `profile.system_prompt`, which Task 14's
`compose_caption_prompt` produced from the resolved `SkillUse`. That is how the
digest enters the transcript.

- [ ] **Step 5a: Add the mid-stream cancellation test**

Spec §8 item 7 requires cancellation "before and mid-stream". The before case is
covered in Step 1. For mid-stream, script two turns and cancel from a wrapper
provider after the first request is observed, mirroring the
`CancelAfterNTexts` sink pattern at `driver.rs:4580`-4650:

```rust
    #[tokio::test]
    async fn single_submit_reports_cancellation_mid_stream() {
        let cancellation = RunCancellation::new();
        // A provider that cancels as soon as it is asked for a turn, then
        // returns a valid submit. The loop's next cancellation check must win.
        let provider = CancelOnFirstRequest::new(cancellation.clone(), vec![
            text_turn("thinking"),
            tool_call_turn("tc_1", "submit_caption_suggestions", r#"{"suggestions":[]}"#),
        ]);

        let terminal = run_caption_profile(
            &provider,
            caption_run_budget_for_tests(),
            &cancellation,
            DisclosureCeiling::TextMetadataOnly,
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
            vec![],
        )
        .await;

        assert!(matches!(terminal, SingleSubmitTerminal::Cancelled));
    }
```

- [ ] **Step 6: Run the tests**

Run: `rtk cargo test -p rollshot-agent single_submit`
Expected: PASS, all nine tests.

Run: `rtk cargo test -p rollshot-agent visual_annotation`
Expected: PASS — the visual annotation path is untouched. The only edit to
`visual_annotation.rs` in this task is test-module visibility plus the
`request_count` accessor.

- [ ] **Step 7: Commit**

```bash
rtk git add -A crates/rollshot-agent
rtk git commit -m "feat(agent): add an authority-aware single-submit run profile"
```

---

## Task 16: Caption run wiring

**Files:**
- Create: `crates/rollshot-agent/src/captions.rs` (`caption_run_budget`)
- Modify: `crates/rollshot-agent/src/lib.rs` (`pub mod captions;`)
- Modify: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs:1234-1275`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs` (workspace state
  gains the store handle, the cancellation, and the task id; `run`'s boot closure
  at `:864` opens the store)

**Interfaces:**
- Consumes: `open_process_store`, `new_v3`, `TaskKind::ActionGuideCaptions`,
  `AuthoritySubject`, `DisclosureCeiling::TextMetadataOnly`,
  `bundled_action_guide_captions_use`, `compose_caption_prompt`,
  `run_single_submit_with_provider`, `SingleSubmitTerminal`,
  `TIMEOUT_MESSAGE` (Task 1).
- Produces:
  - `caption_run_budget() -> RunBudget` in `rollshot-agent`
  - `project_root_digest(root: &std::path::Path) -> [u8; 32]`
  - `caption_source_binding(context: &PreparedCaptionContext, project_root:
    Option<&std::path::Path>) -> SourceBinding`
  - `caption_authority(task_id, run_id, subject) -> Result<AuthoritySnapshot,
    String>`
  - `TimelineWorkspace::task_store: Option<Arc<TaskStore>>` and
    `TimelineWorkspace::caption_cancellation: Option<RunCancellation>`

- [ ] **Step 1: Write the failing tests**

In `crates/rollshot-agent/src/visual_annotation.rs`, next to
`visual_annotation_run_budget`, the caption budget belongs in a new
`crates/rollshot-agent/src/captions.rs`. Add its test there:

```rust
    #[test]
    fn caption_budget_sends_no_attachments_and_keeps_the_thirty_second_bound() {
        let budget = caption_run_budget();

        assert_eq!(budget.attachments, 0);
        assert_eq!(budget.wall_time, std::time::Duration::from_secs(30));
        assert_eq!(budget.model_calls, 2);
        assert_eq!(budget.tool_calls, 1);
        assert_eq!(budget.output_tokens, 1_200);
        assert_eq!(budget.dry_run_attempts, 0);
        assert_eq!(budget.candidate_count, 0);
    }
```

In `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`, in the
`mod provider_tests` module at `caption_agent.rs:464` — **not `mod tests`** —
because `ephemeral_context()` (`caption_agent.rs:528`) and `guide()` (`:517`) live
there.

**Two path corrections.** `RunId` lives at `rollshot_agent::domain::RunId`
(`domain.rs:32`), not `rollshot_agent::runtime::RunId`; `runtime` re-exports
nothing. And neither `RunId` nor `ProductTaskId` has a `new_v4()` — both expose
only `parse` (`domain.rs:35`, `product_task.rs:27`).

```rust
    fn test_task_id() -> rollshot_agent::product_task::ProductTaskId {
        rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000001",
        )
        .unwrap()
    }

    fn test_run_id() -> rollshot_agent::domain::RunId {
        rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap()
    }

    #[test]
    fn caption_authority_grants_only_submit_and_forbids_images() {
        let subject = rollshot_agent::authority::AuthoritySubject::ActionGuideProject {
            project_root_sha256: [7u8; 32],
            revision: 3,
            projection_digest: "ab".repeat(32),
        };
        let run_id = test_run_id();

        let authority = caption_authority(test_task_id(), run_id.clone(), subject.clone()).unwrap();

        assert_eq!(
            authority.disclosure(),
            rollshot_agent::authority::DisclosureCeiling::TextMetadataOnly
        );
        assert!(authority
            .authorize_tool(
                &run_id,
                &subject,
                rollshot_agent::authority::RunOperation::SubmitReviewCandidate
            )
            .is_ok());
        for forbidden in [
            rollshot_agent::authority::RunOperation::InspectPreparedImage,
            rollshot_agent::authority::RunOperation::ExecuteRestrictedAutomation,
            rollshot_agent::authority::RunOperation::WriteDraft,
            rollshot_agent::authority::RunOperation::ReadDraft,
            rollshot_agent::authority::RunOperation::RequestUserInput,
        ] {
            assert!(
                authority.authorize_tool(&run_id, &subject, forbidden).is_err(),
                "caption runs must never hold {forbidden:?}"
            );
        }
    }

    #[test]
    fn source_binding_follows_the_prepared_context_origin() {
        use rollshot_agent::product_task::SourceBinding;

        // Ephemeral origin, with and without a root: always ephemeral.
        let root = tempfile::tempdir().unwrap();
        for project_root in [None, Some(root.path())] {
            match caption_source_binding(&ephemeral_context(), project_root) {
                SourceBinding::ActionGuideEphemeralGuide { guide_digest } => {
                    assert_eq!(guide_digest, "0".repeat(64));
                }
                other => panic!("expected ephemeral, got {other:?}"),
            }
        }

        // Durable origin with a root: project-bound, carrying the projection's
        // own revision and digest, and the path digest — not a placeholder.
        let (context, revision, digest) = durable_context(root.path());
        match caption_source_binding(&context, Some(root.path())) {
            SourceBinding::ActionGuideProject {
                project_root_sha256,
                revision: bound_revision,
                projection_digest,
            } => {
                assert_eq!(project_root_sha256, project_root_digest(root.path()));
                assert_eq!(bound_revision, revision);
                assert_eq!(projection_digest, digest);
            }
            other => panic!("expected project binding, got {other:?}"),
        }

        // Durable origin with no root cannot be restored, so it degrades to
        // ephemeral rather than inventing an identity.
        assert!(matches!(
            caption_source_binding(&context, None),
            SourceBinding::ActionGuideEphemeralGuide { .. }
        ));
    }

    #[test]
    fn project_root_digest_is_path_scoped_and_domain_separated() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();

        assert_eq!(project_root_digest(a.path()), project_root_digest(a.path()));
        assert_ne!(project_root_digest(a.path()), project_root_digest(b.path()));
    }
```

`durable_context(root) -> (PreparedCaptionContext, u64, String)` is a new helper:
write a minimal project under `root` with
`rollshot_action::project` save/load, build
`ActionGuideContextProjectionV1::from_loaded_project`, and return the context
alongside `projection.revision()` and `projection.digest().to_owned()`
(`project/continuity.rs:198`, `:218`). Reuse whatever project-writing helper
`rollshot-action`'s own project tests already provide rather than hand-rolling a
manifest.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent caption_budget_sends_no_attachments`
Expected: FAIL to compile — `caption_run_budget` not found.

- [ ] **Step 3: Add the budget**

Create `crates/rollshot-agent/src/captions.rs`:

```rust
//! Caption-run budget. The provider-neutral half of the Action Guide caption
//! flow; the guide model and draft types stay in `rollshot-action`, which this
//! crate must not depend on.

use crate::runtime::RunBudget;

/// Tight caption budget. Wall time and output tokens match the pre-migration
/// timeout and `max_tokens` exactly, so observable timing does not change.
pub fn caption_run_budget() -> RunBudget {
    RunBudget {
        wall_time: std::time::Duration::from_secs(30),
        model_calls: 2,
        input_tokens: 32_000,
        output_tokens: 1_200,
        cost: f64::MAX,
        tool_calls: 1,
        per_tool_calls: 1,
        argument_bytes: 4_096,
        result_bytes: 4_096,
        source_bytes: 0,
        attachments: 0,
        validation_attempts: 0,
        dry_run_attempts: 0,
        capability_calls: 0,
        candidate_count: 0,
        affected_area: 0,
    }
}
```

Declare `pub mod captions;` in `crates/rollshot-agent/src/lib.rs`.

- [ ] **Step 4: Add the app-side authority and binding helpers**

In `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`:

`OsStr::as_encoded_bytes()` is stable since Rust 1.74; the workspace pins
`rust-version = "1.94"` (root `Cargo.toml:53`) on edition 2021, so it compiles.
Verified on review.

```rust
/// SHA-256 of a canonicalized project root path. The Action Guide project
/// manifest has no stable identity, so the path is the only one available.
pub(crate) fn project_root_digest(root: &std::path::Path) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(b"rollshot-action-guide-project-root-v1\0");
    hasher.update(canonical.as_os_str().as_encoded_bytes());
    hasher.finalize().into()
}

pub(crate) fn caption_source_binding(
    context: &PreparedCaptionContext,
    project_root: Option<&std::path::Path>,
) -> rollshot_agent::product_task::SourceBinding {
    use rollshot_agent::product_task::SourceBinding;
    match (context, project_root) {
        (PreparedCaptionContext::Durable { projection, .. }, Some(root)) => {
            SourceBinding::ActionGuideProject {
                project_root_sha256: project_root_digest(root),
                revision: projection.revision(),
                projection_digest: projection.digest().to_owned(),
            }
        }
        (PreparedCaptionContext::Durable { projection, .. }, None) => {
            // A durable projection without a root cannot be restored later, so
            // bind it as ephemeral rather than inventing an identity.
            SourceBinding::ActionGuideEphemeralGuide {
                guide_digest: projection.digest().to_owned(),
            }
        }
        (PreparedCaptionContext::Ephemeral { guide_digest, .. }, _) => {
            SourceBinding::ActionGuideEphemeralGuide {
                guide_digest: guide_digest.clone(),
            }
        }
    }
}

pub(crate) fn caption_authority(
    task_id: rollshot_agent::product_task::ProductTaskId,
    run_id: rollshot_agent::domain::RunId,
    subject: rollshot_agent::authority::AuthoritySubject,
) -> Result<rollshot_agent::authority::AuthoritySnapshot, String> {
    use rollshot_agent::authority::{
        AuthorityBinding, AuthoritySnapshot, DisclosureCeiling, RunOperation,
    };
    use rollshot_agent::product_task::TaskAttemptId;

    let binding = AuthorityBinding::new(task_id, TaskAttemptId::new(1), run_id, subject);
    AuthoritySnapshot::new(
        binding,
        "rollshot-v1".to_owned(),
        DisclosureCeiling::TextMetadataOnly,
        false,
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::from([RunOperation::SubmitReviewCandidate]),
    )
    .map_err(|e| format!("build caption authority: {e}"))
}
```

- [ ] **Step 5: Replace the run body**

Rewrite `suggest_captions_with_timeout` to:

1. build the source binding and create the task with
   `ProductTaskSnapshot::new_v3(.., TaskKind::ActionGuideCaptions, ..)`, then
   `create_audited`;
2. `start_attempt` → `transition_audited`;
3. resolve `bundled_action_guide_captions_use()`; build the authority;
   `bind_run_contract` → `transition_audited`;
4. build the profile with `compose_caption_prompt(&skill_use)?`,
   `caption_tool_definition()`, and
   `RunOperation::SubmitReviewCandidate`;
5. call `run_single_submit_with_provider` with `caption_run_budget()` and the
   workspace-owned `RunCancellation`;
6. map the terminal:
   - `Submitted { arguments }` → `parse_caption_tool_args(&arguments)`, then
     Task 17's promotion;
   - `TextCompleted { text }` → `parse_caption_response(&text)`, then Task 17's
     promotion. **This is the pre-migration text fallback, preserved.** Today
     `caption_agent.rs:343`-346 decodes assistant text when no tool call
     arrives, and the bundled skill body still tells the model it may do that.
     A decode failure here is `record_terminal` with
     `TaskTerminal::AgentProtocolFailure` and today's copy, exactly as a
     malformed tool payload is;
   - `BudgetExhausted { dimension: BudgetDimension::WallTime }` →
     `Err(TIMEOUT_MESSAGE.to_string())` plus `record_terminal`;
   - every other terminal → `record_terminal` with the matching
     `TaskTerminal`, and the existing user-visible copy.

Add the fallback test alongside the mapping test in Step 5b:

```rust
    #[test]
    fn text_completion_still_decodes_captions_without_a_tool_call() {
        // Preserves the pre-migration fallback: a provider that cannot call
        // tools may return the same JSON as assistant text.
        let terminal = rollshot_agent::driver::SingleSubmitTerminal::TextCompleted {
            text: r#"{"suggestions":[{"source":10,"title":"Open Settings","caption":"The settings panel appears.","confidence":0.8,"rationale":null}]}"#
                .to_string(),
        };

        let drafts = decode_caption_terminal(&terminal).expect("text fallback must decode");

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].caption, "The settings panel appears.");
    }
```

`decode_caption_terminal` is the single function that switches on the terminal
and calls `parse_caption_tool_args` or `parse_caption_response`; put the mapping
there rather than inline in the run so both tests can reach it.

Every store call runs inside `tokio::task::spawn_blocking`, following
`run.rs:1094`-1105 (`transition_audited`) and `:1045`-1050 (`create_audited`).

- [ ] **Step 5a: Rehome the four pre-existing `provider_tests`**

`suggest_captions_with_timeout(run_id, model, adapter, context, timeout)` is
called by four tests in `mod provider_tests`
(`caption_agent.rs:559`, `:594`, `:618`, `:637`). All four break: the signature
loses `timeout` and gains a store handle, a project root, and a
`RunCancellation`, and the streaming semantics change from a raw
`adapter.stream(..)` loop to `AgentRunner::run_single_submit_with_provider`. Do
not leave them to be discovered by the compiler — decide each one here.

| Existing test | Disposition |
|---|---|
| `runner_prefers_tool_call_arguments` (`:576`) | Keep the intent. Rewrite `FakeProvider`'s script into the three-event shape the driver's rig loop needs (`ToolCallStart`, `ToolCallArgumentDelta`, `Completed(StopReason::ToolUse)`), mirroring `visual_annotation.rs:983`-995. A bare `ToolCallComplete` with no `Completed` event will not advance the run |
| `runner_returns_provider_errors` (`:610`) | Keep. `ModelStreamEvent::Error` still surfaces, now as `SingleSubmitTerminal::ProviderFailure`; assert the mapped user-visible string rather than `err.contains("rate limited")`, because the terminal no longer carries provider text |
| `runner_times_out_quickly_in_tests` (`:631`) | Replace with the budget test in Step 5b. A `delay` on the fake provider no longer drives the outcome; `RunBudget::wall_time` does |
| `runner_accepts_text_json_from_fake_provider` (`:544`) | **Delete and replace.** See below |

**The text-JSON fallback is removed by this migration.** Today
`suggest_captions_with_timeout` falls back to
`parse_caption_response(&text)` when no tool call arrives
(`caption_agent.rs:343`-346). `run_single_submit_with_provider` inherits
`run_visual_annotation_with_provider`'s behavior: a completion with no terminal
tool call returns `ProtocolFailure` (`driver.rs:1960`-1965, confirmed by
`visual_annotation.rs:1096`-1109). Replace
`runner_accepts_text_json_from_fake_provider` with a test asserting that a
text-only turn now yields the `AgentProtocolFailure` user-visible copy, and leave
a comment naming this as an intentional consequence of the migration rather than a
bug. `parse_caption_response` becomes dead on the caption run path; leave the
function and its unit tests in place (`caption_agent.rs:222`, `:390`, `:412`) —
they are still the strict decoder's contract tests — but do not add a
`#[allow(dead_code)]` without checking whether anything else calls it.

Note that `SKILL.md` still says "If tool calling is unavailable, return only JSON
in the same schema", because Task 13 preserves the instruction text verbatim by
design. Record this mismatch in Task 22's residual risks; do not fix the text in
this slice.

- [ ] **Step 5b: Test the terminal-to-copy mapping**

This is the load-bearing preservation claim of §4.4 and the only place the four
frozen strings are actually proven end to end. Task 1 pins the constant; this pins
the mapping.

```rust
    #[test]
    fn wall_time_exhaustion_reports_the_frozen_timeout_copy() {
        // The pre-migration 30s tokio timeout becomes RunBudget::wall_time.
        // Duration::ZERO trips check_wall_time deterministically
        // (runtime.rs:234 compares with >=), so this test does not sleep.
        let err = run(suggest_captions_bounded_for_tests(
            /* provider that would succeed */,
            std::time::Duration::ZERO,
        ))
        .unwrap_err();

        assert_eq!(err, super::TIMEOUT_MESSAGE);
    }

    #[test]
    fn cancellation_and_protocol_failures_keep_their_existing_copy() {
        for (terminal, expected) in expected_terminal_copy_pairs() {
            assert_eq!(map_caption_terminal(&terminal), expected);
        }
    }
```

Expose the wall-time budget as a parameter of a test-only entry point (or take the
whole `RunBudget`) so the test does not have to wait 30 seconds. Keep the
production caller on `caption_run_budget()`.

Also assert the terminal that must **not** promote: a `ProtocolFailure` or
`AuthorityDenied` run leaves the task in a `Failed`/terminal status with
`artifact_metadata() == None`, per spec §6 ("No terminal other than a validated
`Submitted` batch may promote an artifact").

- [ ] **Step 6: Own the cancellation in the workspace**

In `crates/rollshot-app/src/timeline_workspace/mod.rs`, add to the workspace
state:

```rust
    /// Cancellation for the in-flight caption run. Triggered on the existing
    /// exits — leaving the workspace, starting another run, closing the project
    /// — with no new UI affordance.
    pub(crate) caption_cancellation: Option<rollshot_agent::runtime::RunCancellation>,
```

Initialize it to `None` in every constructor, and call `cancel()` then clear it
in the handlers for those three existing exits.

- [ ] **Step 6a: Give the timeline workspace its store handle**

Task 11 Step 5 established that each process runs exactly one workspace, so the
timeline workspace opens its own store at its own root. In
`crates/rollshot-app/src/timeline_workspace/mod.rs`, inside `run`'s boot closure
(`mod.rs:864`-890), call `crate::agent_store::open_process_store(&config_dir)`
once and store the `Arc` on `TimelineWorkspace`:

```rust
    /// The single process-wide task store, opened once at workspace boot.
    /// `None` when the config directory or the store is unavailable; the caption
    /// run then reports the existing `"Caption suggestions failed: {error}"`
    /// copy rather than running unpersisted and unaudited.
    pub(crate) task_store: Option<std::sync::Arc<crate::agent_store::TaskStore>>,
```

Do **not** open the store inside the caption message handler: `open` now runs the
Task 19 sweep and `reconcile_all_task_audits`, so opening it per run would sweep
repeatedly and would violate the one-instance-per-process rule that the flock
semantics depend on. Add a test that a `None` store makes the caption request fail
with the existing `"Caption suggestions failed: ..."` copy and never calls the
provider.

- [ ] **Step 7: Run the suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide caption`
Expected: PASS, including Task 1's baseline tests and the `provider_tests` as
rehomed in Step 5a. Three of the four originals survive in rewritten form and one
is replaced; the suite must not shrink.

Run: `rtk cargo test -p rollshot-app`
Expected: PASS with the feature OFF. `crates/rollshot-agent/src/captions.rs` is
unconditional, so `caption_run_budget` compiles either way.

- [ ] **Step 8: Commit**

```bash
rtk git add -A crates/rollshot-agent crates/rollshot-app
rtk git commit -m "feat(action): run caption suggestions as a bounded audited task"
```

---

## Task 17: Artifact promotion and review receipt

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
  (promotion payload)
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs` (accept and
  reject handlers)

**Interfaces:**
- Consumes: `ArtifactSummary::ActionGuideCaptions`,
  `ArtifactKind::ActionGuideCaptions`, `record_ready_for_review`, `begin_apply`,
  `complete_apply`, `reject`, `ReviewReceipt`.
- Produces:
  - `caption_artifact_payload(proposal: &CaptionProposal) -> Vec<u8>`
  - `caption_review_receipt(proposal: &CaptionProposal, metadata:
    &ProductArtifactMetadata, now: i64) -> Result<ReviewReceipt, String>`

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn artifact_payload_carries_suggestions_and_nothing_else() {
        let proposal = caption_proposal_fixture();

        let bytes = caption_artifact_payload(&proposal);
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["suggestions"].as_array().unwrap().len(), 1);
        assert!(json.get("guide").is_none(), "no whole-guide copy: {json}");
    }

    #[test]
    fn review_receipt_partitions_decisions_and_binds_the_artifact_revision() {
        let mut proposal = caption_proposal_fixture();
        proposal.reject(rollshot_action::CaptionSuggestionId(1));
        let metadata = caption_artifact_metadata_fixture();

        let receipt = caption_review_receipt(&proposal, &metadata, 5_000).unwrap();

        assert_eq!(receipt.artifact_revision, metadata.artifact_revision());
        assert_eq!(receipt.rejected_candidates, vec![1]);
        assert!(receipt.applied_candidates.is_empty());
        assert!(receipt.local_delta.moved_candidates.is_empty());
        assert!(receipt.local_delta.manual_additions.is_empty());
        assert_eq!(receipt.resulting_document_state_id, None);
    }

    #[test]
    fn suggestion_ids_above_u32_are_rejected_not_truncated() {
        let mut proposal = caption_proposal_fixture();
        let oversized = rollshot_action::CaptionSuggestionId(u64::from(u32::MAX) + 1);
        proposal.suggestions[0].id = oversized;
        // The narrowing guard only runs for decided suggestions
        // (Accepted / Rejected / Stale). A Pending suggestion is skipped
        // entirely, so without this line the test would pass for the wrong
        // reason — `caption_review_receipt` would return Ok and `is_err()`
        // would fail.
        assert!(proposal.reject(oversized), "reject must find the mutated id");
        let metadata = caption_artifact_metadata_fixture();

        let err = caption_review_receipt(&proposal, &metadata, 5_000)
            .expect_err("an out-of-range suggestion id must be rejected");
        assert!(err.contains("exceeds u32"), "unexpected error: {err}");
    }

    #[test]
    fn accepted_suggestions_land_in_applied_not_rejected() {
        // The mirror of the reject case: without this, the Accepted arm of the
        // partition is never exercised and could be swapped with Rejected
        // without any test noticing.
        let mut proposal = caption_proposal_fixture();
        proposal.suggestions[0].status = rollshot_action::CaptionSuggestionStatus::Accepted;
        let metadata = caption_artifact_metadata_fixture();

        let receipt = caption_review_receipt(&proposal, &metadata, 5_000).unwrap();

        assert_eq!(receipt.applied_candidates, vec![1]);
        assert!(receipt.rejected_candidates.is_empty());
    }

    #[test]
    fn stale_suggestions_are_recorded_as_rejected() {
        let mut proposal = caption_proposal_fixture();
        proposal.suggestions[0].status = rollshot_action::CaptionSuggestionStatus::Stale;
        let metadata = caption_artifact_metadata_fixture();

        let receipt = caption_review_receipt(&proposal, &metadata, 5_000).unwrap();

        assert_eq!(receipt.rejected_candidates, vec![1]);
    }

    #[test]
    fn promotion_binds_the_kind_the_origin_and_the_payload_digest() {
        // Gate A1 item 2 and spec §8 item 2: both origins, with the recorded
        // binding and canonical_payload_sha256 asserted, not just the payload
        // shape.
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();

        for (label, binding) in [
            (
                "durable",
                caption_source_binding(&durable_context(root.path()).0, Some(root.path())),
            ),
            ("ephemeral", caption_source_binding(&ephemeral_context(), None)),
        ] {
            let proposal = caption_proposal_fixture();
            let bytes = caption_artifact_payload(&proposal);
            let ready = promote_caption_task_for_tests(&binding, &proposal);
            let meta = ready.artifact_metadata().expect(label);

            assert_eq!(meta.kind(), rollshot_agent::product_task::ArtifactKind::ActionGuideCaptions);
            assert_eq!(meta.source_binding(), &binding, "{label}");
            assert_eq!(
                meta.summary(),
                &rollshot_agent::product_task::ArtifactSummary::ActionGuideCaptions {
                    suggestion_count: proposal.suggestions.len() as u32,
                },
                "{label}"
            );
            assert_eq!(
                meta.canonical_payload_sha256(),
                format!("{:x}", Sha256::digest(&bytes)),
                "{label}: digest must cover exactly the promoted bytes"
            );
            assert_eq!(ready.pending_artifact_payload(), Some(bytes.as_slice()), "{label}");
        }
    }
```

`caption_proposal_fixture()` must build through
`CaptionProposal::from_agent_drafts`, which assigns
`CaptionSuggestionId(index + 1)` (`caption_proposal.rs:161`) — so a one-suggestion
fixture has id `1`, which the assertions above depend on. `suggestions`, `id`, and
`status` are all `pub` (`caption_proposal.rs:87`-95, `:102`), so the direct
mutations compile.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app --features action-guide artifact_payload_carries`
Expected: FAIL to compile — `caption_artifact_payload` not found.

- [ ] **Step 3: Implement both helpers**

```rust
#[derive(serde::Serialize)]
struct CaptionArtifactSuggestionV1 {
    id: u64,
    step_source: rollshot_action::CandidateId,
    suggested_title: Option<String>,
    suggested_caption: String,
    confidence: f32,
    rationale: Option<String>,
}

#[derive(serde::Serialize)]
struct CaptionArtifactPayloadV1 {
    schema_version: u32,
    suggestions: Vec<CaptionArtifactSuggestionV1>,
}

pub(crate) fn caption_artifact_payload(proposal: &rollshot_action::CaptionProposal) -> Vec<u8> {
    let payload = CaptionArtifactPayloadV1 {
        schema_version: 1,
        suggestions: proposal
            .suggestions
            .iter()
            .map(|s| CaptionArtifactSuggestionV1 {
                id: s.id.0,
                step_source: s.base.source,
                suggested_title: s.suggested_title.clone(),
                suggested_caption: s.suggested_caption.clone(),
                confidence: s.confidence,
                rationale: s.rationale.clone(),
            })
            .collect(),
    };
    serde_json::to_vec(&payload).expect("caption payload is always serializable")
}

pub(crate) fn caption_review_receipt(
    proposal: &rollshot_action::CaptionProposal,
    metadata: &rollshot_agent::product_task::ProductArtifactMetadata,
    now: i64,
) -> Result<rollshot_agent::product_task::ReviewReceipt, String> {
    use rollshot_action::CaptionSuggestionStatus;
    use rollshot_agent::product_task::{LocalReviewDeltaV1, ReviewReceipt};

    let narrow = |id: u64| -> Result<u32, String> {
        u32::try_from(id).map_err(|_| format!("caption suggestion id {id} exceeds u32"))
    };

    let mut applied = Vec::new();
    let mut rejected = Vec::new();
    for suggestion in &proposal.suggestions {
        match suggestion.status {
            CaptionSuggestionStatus::Accepted => applied.push(narrow(suggestion.id.0)?),
            CaptionSuggestionStatus::Rejected | CaptionSuggestionStatus::Stale => {
                rejected.push(narrow(suggestion.id.0)?)
            }
            CaptionSuggestionStatus::Pending => {}
        }
    }

    Ok(ReviewReceipt {
        artifact_id: metadata.artifact_id().clone(),
        artifact_revision: metadata.artifact_revision(),
        proposal_id: metadata.proposal_id().to_owned(),
        applied_candidates: applied,
        rejected_candidates: rejected,
        // Captions have no move or manual-add review editing:
        // CaptionProposal::apply has no edit-then-accept path.
        local_delta: LocalReviewDeltaV1 {
            moved_candidates: Vec::new(),
            manual_additions: Vec::new(),
        },
        resulting_document_state_id: None,
        resulting_document_digest: None,
        decided_at_unix_ms: now,
    })
}
```

- [ ] **Step 4: Wire promotion and the review transitions**

On a decoded batch, promote with `ArtifactKind::ActionGuideCaptions`,
`ArtifactSummary::ActionGuideCaptions { suggestion_count }`,
`canonical_payload_sha256` over `caption_artifact_payload`, and
`proposal_payload` = the serialized `CaptionProposal`.

In the accept and reject handlers in
`crates/rollshot-app/src/timeline_workspace/update.rs`: on the first decision
call `begin_apply`; when `has_pending()` becomes false, call `complete_apply`
if any suggestion was accepted, otherwise `reject`. Every transition goes
through `transition_audited` inside `spawn_blocking`.

- [ ] **Step 5: Run the suites**

Run: `rtk cargo test -p rollshot-app --features action-guide caption`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add -A crates/rollshot-app
rtk git commit -m "feat(action): promote caption suggestions as a reviewable artifact"
```

---

## Task 18: Restore into the existing review surface

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs` (project-open
  handler)

**Interfaces:**
- Consumes: `reconcile_for_source`, `caption_source_binding`, the serialized
  proposal payload.
- Produces: `restore_caption_proposal(store: &TaskStore, binding:
  &SourceBinding, now: i64) -> Option<(ProductTaskId, CaptionProposal)>`

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn restore_repopulates_the_review_surface_without_a_provider() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        // Durable ActionGuideProject binding — an ephemeral one would be swept
        // to Stale by Task 19's open-time sweep and could never restore.
        let binding = action_guide_binding_fixture();
        seed_ready_for_review_caption_task(&store, &binding);

        // Spec §8 item 6: prove no provider call, do not merely omit one.
        // `PanicProvider::stream` panics if it is ever invoked.
        let provider = PanicProvider;
        let restored = restore_caption_proposal_with_provider(&store, &binding, 9_000, &provider);

        let (_task_id, proposal) = restored.expect("a matching task must restore");
        assert_eq!(proposal.suggestions.len(), 1);
        assert!(proposal.has_pending());
        assert!(matches!(
            proposal.origin(),
            rollshot_action::CaptionProposalOrigin::DurableProject { .. }
        ));
    }

    #[test]
    fn restore_declines_and_marks_stale_when_the_revision_moved() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let binding = action_guide_binding_fixture();
        let task_id = seed_ready_for_review_caption_task(&store, &binding);
        let moved_on = bump_revision(&binding);

        assert!(restore_caption_proposal(&store, &moved_on, 9_000).is_none());
        assert_eq!(
            store.load(&task_id).unwrap().status(),
            rollshot_agent::product_task::TaskStatus::Stale
        );
    }

    #[test]
    fn restore_declines_and_leaves_other_projects_untouched() {
        // Identity mismatch is skipped, not marked stale (spec §5.3). Without
        // this, `identity_matches` and `freshness_matches` could be swapped and
        // the revision test above would still pass.
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let binding = action_guide_binding_fixture();
        let task_id = seed_ready_for_review_caption_task(&store, &binding);
        let other_project = with_different_project_root(&binding);

        assert!(restore_caption_proposal(&store, &other_project, 9_000).is_none());
        assert_eq!(
            store.load(&task_id).unwrap().status(),
            rollshot_agent::product_task::TaskStatus::ReadyForReview,
            "a different project must not stale another project's pending task"
        );
    }

    #[test]
    fn restore_is_deterministic_across_repeated_calls() {
        // Gate A1 item 4 / spec §8: "the same input twice yields the same
        // outcome". The first call must not consume or mutate the task.
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let binding = action_guide_binding_fixture();
        seed_ready_for_review_caption_task(&store, &binding);

        let first = restore_caption_proposal(&store, &binding, 9_000);
        let second = restore_caption_proposal(&store, &binding, 9_001);

        assert_eq!(first, second);
    }

    #[test]
    fn an_undecodable_stored_proposal_does_not_restore() {
        // The `Err` arm of the payload decode (Step 3) is otherwise untested,
        // and it is the one path that must not panic on a corrupt file.
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let binding = action_guide_binding_fixture();
        seed_ready_for_review_caption_task_with_payload(&store, &binding, b"not json".to_vec());

        assert!(restore_caption_proposal(&store, &binding, 9_000).is_none());
    }
```

`restore_caption_proposal_with_provider` is `restore_caption_proposal` with an
unused `&dyn ProviderAdapter` argument used only to hold the panicking mock; if
threading a provider through is judged not worth it, assert the same property with
a counting `ProviderAdapter` installed in the workspace state and
`assert_eq!(provider.request_count(), 0)` after the project-open handler runs.
Either is acceptable; silence is not.

`with_different_project_root` returns the same binding with a different
`project_root_sha256`. Both helpers, plus `bump_revision`, must produce
`SourceBinding::ActionGuideProject` values — `restore_caption_proposal` on an
`ActionGuideEphemeralGuide` binding is meaningless after Task 19.

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app --features action-guide restore_repopulates`
Expected: FAIL to compile — `restore_caption_proposal` not found.

- [ ] **Step 3: Implement the restore**

```rust
/// Look for a durable caption task ready for review against this binding.
///
/// Identity and freshness are both checked by `reconcile_for_source`, which also
/// marks a same-identity stale task through its audited path. No provider call
/// is made: the proposal comes from the stored payload.
pub(crate) fn restore_caption_proposal(
    store: &crate::agent_store::TaskStore,
    binding: &rollshot_agent::product_task::SourceBinding,
    now: i64,
) -> Option<(
    rollshot_agent::product_task::ProductTaskId,
    rollshot_action::CaptionProposal,
)> {
    let snapshot = store.reconcile_for_source(binding, now).ok().flatten()?;
    if snapshot.kind() != rollshot_agent::product_task::TaskKind::ActionGuideCaptions {
        return None;
    }
    let payload = snapshot.pending_proposal_payload()?;
    match serde_json::from_slice::<rollshot_action::CaptionProposal>(payload) {
        Ok(proposal) => Some((snapshot.task_id().clone(), proposal)),
        Err(error) => {
            tracing::warn!(
                target: "rollshot::action::caption_agent",
                error = %error,
                task_id = snapshot.task_id().as_str(),
                "stored caption proposal failed to decode; not restoring"
            );
            None
        }
    }
}
```

`CaptionProposal` and its members need `Serialize` and `Deserialize`. Add the
derives in `crates/rollshot-action/src/caption_proposal.rs` to
`CaptionProposalId`, `CaptionSuggestionId`, `CaptionProposalProvenance`,
`CaptionProposalOrigin`, `CaptionSuggestionBase`, `CaptionSuggestionStatus`,
`CaptionSuggestion`, and `CaptionProposal`.

- [ ] **Step 4: Call it when a project opens**

In the project-open handler in
`crates/rollshot-app/src/timeline_workspace/update.rs`, after the session is
established, build the binding with `caption_source_binding` and set
`state.caption_proposal` from `restore_caption_proposal`. Do not add any message
or banner: the existing review surface renders whenever
`caption_proposal` is `Some`.

- [ ] **Step 5: Run the suites**

Run: `rtk cargo test -p rollshot-app --features action-guide restore`
Expected: PASS, both tests.

Run: `rtk cargo test -p rollshot-action`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add -A crates/rollshot-action crates/rollshot-app
rtk git commit -m "feat(action): restore a pending caption proposal after restart"
```

---

## Task 19: Ephemeral sweep and interrupted reconciliation

**Files:**
- Modify: `crates/rollshot-app/src/agent_store/task_store.rs` (`open`, plus a
  new sweep function)

**Interfaces:**
- Consumes: `SourceBinding::ActionGuideEphemeralGuide`, `mark_stale`,
  `reconcile_interrupted`.
- Produces: `TaskStore::sweep_ephemeral_on_open(&self, now: i64) ->
  Result<usize, TaskStoreError>` returning how many tasks were resolved.

**Verified on review (2026-07-28):**

- `mark_stale` requires `ReadyForReview` and errors otherwise
  (`product_task.rs:1130`-1135), so the `(ReadyForReview, true)` arm is the only
  legal caller. Correct as written.
- `reconcile_interrupted` accepts `Created | Running | Applying`
  (`product_task.rs:1154`-1157) and returns `Ok(None)` for anything else. The
  sweep's match therefore leaves an orphaned `Created` task `Created` forever.
  Step 3 below adds it, with the same launch-grace guard the existing per-source
  reconciler uses.
- This overlaps `reconcile_for_source`, which **already** reconciles
  `Created | Running | Applying → Interrupted`
  (`task_store.rs:1241`-1268), including a `CREATED_INTERRUPT_GRACE_MS = 60_000`
  window for `Created` (`task_store.rs:47`, `:1248`-1252). The sweep is not
  redundant — `reconcile_for_source` only ever sees tasks whose binding is being
  matched, and it is never called for a domain the current process is not in —
  but the two must not disagree about `Created`. Factor the shared decision into
  one private helper rather than writing the rule twice.
- `load` takes no lock (`task_store.rs:714`-716) and `transition_audited` takes
  one per call (`:928`), so the sweep's load-then-transition loop cannot
  self-deadlock. It must be called from `open` **after**
  `reconcile_all_task_audits()` has returned and released its locks
  (`:250`), which is where Step 3 puts it.

**Regression risk to check explicitly.** Nothing marks `Running`/`Applying` tasks
`Interrupted` at `open` today. `TaskStore::open` is called by dozens of existing
tests, and `open_with_failpoint` routes through it (`task_store.rs:354`-361). Any
existing test that seeds a non-terminal task, drops the store, reopens it, and then
expects the earlier status — or CASes against a snapshot it captured before the
reopen — will now fail with `TaskStoreError::Conflict`. Grep for reopen patterns
before implementing:

```bash
rtk grep -rn "TaskStore::open\|open_process_store\|open_with_failpoint" crates/rollshot-app/src --include="*.rs"
```

For each site that opens the same `config_dir` twice, decide whether the new
sweep is the desired behavior (usually yes — that is a simulated restart) or
whether the test should hold one store instance. Do not weaken the sweep to make a
test pass.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn open_marks_ephemeral_ready_for_review_stale() {
        let dir = tempfile::tempdir().unwrap();
        let task_id = {
            let store = crate::agent_store::open_process_store(dir.path()).unwrap();
            seed_ephemeral_ready_for_review(&store)
        };

        // Reopening is what a restart looks like.
        let reopened = crate::agent_store::open_process_store(dir.path()).unwrap();

        assert_eq!(
            reopened.load(&task_id).unwrap().status(),
            TaskStatus::Stale,
            "an ephemeral guide has no durable target to apply to"
        );
    }

    #[test]
    fn open_marks_running_tasks_interrupted_for_both_domains() {
        let dir = tempfile::tempdir().unwrap();
        let (smart_id, caption_id) = {
            let store = crate::agent_store::open_process_store(dir.path()).unwrap();
            (seed_running_smart_redaction(&store), seed_running_caption(&store))
        };

        let reopened = crate::agent_store::open_process_store(dir.path()).unwrap();

        for id in [smart_id, caption_id] {
            assert!(matches!(
                reopened.load(&id).unwrap().status(),
                TaskStatus::Interrupted
            ));
        }
    }

    #[test]
    fn open_leaves_durable_ready_for_review_alone() {
        let dir = tempfile::tempdir().unwrap();
        let task_id = {
            let store = crate::agent_store::open_process_store(dir.path()).unwrap();
            seed_durable_ready_for_review_caption(&store)
        };

        let reopened = crate::agent_store::open_process_store(dir.path()).unwrap();

        assert_eq!(
            reopened.load(&task_id).unwrap().status(),
            TaskStatus::ReadyForReview
        );
    }

    #[test]
    fn open_leaves_terminal_tasks_alone_and_reports_zero() {
        // The sweep must be a no-op on a store that has nothing to resolve, or
        // every restart writes spurious audit events.
        let dir = tempfile::tempdir().unwrap();
        let task_id = {
            let store = crate::agent_store::open_process_store(dir.path()).unwrap();
            seed_completed_caption(&store)
        };

        let reopened = crate::agent_store::open_process_store(dir.path()).unwrap();

        assert_eq!(reopened.sweep_ephemeral_on_open(now_ms()).unwrap(), 0);
        assert_eq!(
            reopened.load(&task_id).unwrap().status(),
            TaskStatus::Completed
        );
    }

    #[test]
    fn open_is_idempotent_across_two_restarts() {
        // A second restart must not re-transition an already-swept task; the
        // sweep would otherwise fail its own CAS and log a warning every boot.
        let dir = tempfile::tempdir().unwrap();
        let task_id = {
            let store = crate::agent_store::open_process_store(dir.path()).unwrap();
            seed_ephemeral_ready_for_review(&store)
        };

        let _first = crate::agent_store::open_process_store(dir.path()).unwrap();
        let second = crate::agent_store::open_process_store(dir.path()).unwrap();

        assert_eq!(second.sweep_ephemeral_on_open(now_ms()).unwrap(), 0);
        assert_eq!(second.load(&task_id).unwrap().status(), TaskStatus::Stale);
    }

    #[test]
    fn open_appends_a_task_terminated_event_for_each_resolution() {
        // §5.1 maps both mark_stale and reconcile_interrupted at open to
        // TaskTerminated. Without this the sweep could transition state
        // silently and still pass every test above.
        let dir = tempfile::tempdir().unwrap();
        let task_id = {
            let store = crate::agent_store::open_process_store(dir.path()).unwrap();
            seed_ephemeral_ready_for_review(&store)
        };

        let reopened = crate::agent_store::open_process_store(dir.path()).unwrap();

        let kinds: Vec<_> = reopened
            .committed_audit_events(&task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.event().kind())
            .collect();
        assert!(
            kinds.contains(&AuditEventKindV1::TaskTerminated),
            "sweep must be audited, got {kinds:?}"
        );
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app --features action-guide open_marks_ephemeral`
Expected: FAIL — the task is still `ReadyForReview`.

- [ ] **Step 3: Implement the sweep**

```rust
    /// Resolve tasks that cannot survive a process boundary.
    ///
    /// `open` runs once per process, which is precisely "after a restart". A
    /// per-source matcher cannot do this job: it cannot distinguish processes.
    ///
    /// An ephemeral-origin task has no durable target to apply to, so a
    /// `ReadyForReview` one becomes `Stale`. Any `Created`, `Running`, or
    /// `Applying` task in any domain becomes `Interrupted`, because its process
    /// is gone.
    ///
    /// `Created` uses the same `CREATED_INTERRUPT_GRACE_MS` window as
    /// `reconcile_for_source`: a task created moments ago may belong to a live
    /// run whose `Created → Running` write has not landed yet. At `open` that is
    /// only reachable when a second process is mid-launch, which the
    /// one-instance rule forbids — but the guard costs nothing and keeps the two
    /// reconcilers from disagreeing.
    pub fn sweep_ephemeral_on_open(&self, now: i64) -> Result<usize, TaskStoreError> {
        let mut resolved = 0usize;

        for task_id in self.all_task_ids()? {
            let snapshot = match self.load(&task_id) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let ephemeral = matches!(
                snapshot.source_binding(),
                SourceBinding::ActionGuideEphemeralGuide { .. }
            );

            let next = match (snapshot.status(), ephemeral) {
                (TaskStatus::ReadyForReview, true) => snapshot.mark_stale(now).ok(),
                (TaskStatus::Created, _)
                    if now - snapshot.updated_at_unix_ms() < CREATED_INTERRUPT_GRACE_MS =>
                {
                    None
                }
                (TaskStatus::Created | TaskStatus::Running | TaskStatus::Applying, _) => {
                    snapshot.reconcile_interrupted(now).ok().flatten()
                }
                _ => None,
            };

            if let Some(next) = next {
                let event_id = AuditEventId::new_v4();
                if let Err(e) = self.transition_audited(&snapshot, &next, event_id, now) {
                    tracing::warn!(
                        target: "rollshot::app::agent_store",
                        error = %e,
                        task_id = task_id.as_str(),
                        "open-time sweep transition failed"
                    );
                    continue;
                }
                resolved += 1;
            }
        }

        Ok(resolved)
    }
```

`all_task_ids` does not exist. Derive it from `sorted_task_entries`
(`task_store.rs:1337`-1373), reusing the `strip_suffix(TASK_FILE_SUFFIX)` +
`ProductTaskId::parse` filename parsing that `reconcile_for_source` already does
at `:1225`-1232 rather than duplicating it. Extracting that parsing into a
private `fn task_ids(&self) -> Result<Vec<ProductTaskId>, TaskStoreError>` and
having both callers use it is the smaller diff.

The new tests need `use rollshot_agent::audit::AuditEventKindV1;` and a
`fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }` helper in the test
module (`chrono` is already a `rollshot-app` dependency).

Call the sweep from `open`, after `reconcile_all_task_audits`:

```rust
        // Audit journals are reconciled first so the sweep's own transitions
        // append onto repaired journals.
        if let Err(e) = store.sweep_ephemeral_on_open(chrono::Utc::now().timestamp_millis()) {
            tracing::warn!(
                target: "rollshot::app::agent_store",
                error = %e,
                "open-time sweep failed"
            );
        }
```

- [ ] **Step 4: Run the tests**

Run: `rtk cargo test -p rollshot-app --features action-guide open_marks`
Expected: PASS, all three tests.

Run: `rtk cargo test -p rollshot-app --features action-guide task_store`
Expected: PASS — no existing reconcile test regresses.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/agent_store/task_store.rs
rtk git commit -m "feat(app): resolve unusable tasks at store open"
```

---

## Task 20: Audit coverage and privacy assertions

**Files:**
- Test: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
- Test: `crates/rollshot-app/src/agent_store/audit_store/mod.rs`

**Interfaces:**
- Consumes: everything from Tasks 16-19, `committed_audit_events`.
- Produces: nothing consumed later.

- [ ] **Step 1: Write the coverage test**

```rust
    #[test]
    fn caption_task_lifecycle_appends_every_material_event() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_full_caption_lifecycle(&store);

        let kinds: Vec<_> = store
            .committed_audit_events(&task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.event().kind())
            .collect();

        for expected in [
            AuditEventKindV1::TaskCreated,
            AuditEventKindV1::AttemptStarted,
            AuditEventKindV1::RunContractBound,
            AuditEventKindV1::ArtifactPromoted,
            AuditEventKindV1::ReviewApplyStarted,
            AuditEventKindV1::ReviewDecisionCommitted,
        ] {
            assert!(kinds.contains(&expected), "missing {expected:?} in {kinds:?}");
        }

        // Order is part of the contract: a promotion cannot precede its
        // contract bind, and a review decision cannot precede its apply.
        let position = |k: AuditEventKindV1| kinds.iter().position(|got| *got == k).unwrap();
        assert!(position(AuditEventKindV1::TaskCreated) < position(AuditEventKindV1::AttemptStarted));
        assert!(
            position(AuditEventKindV1::RunContractBound)
                < position(AuditEventKindV1::ArtifactPromoted)
        );
        assert!(
            position(AuditEventKindV1::ReviewApplyStarted)
                < position(AuditEventKindV1::ReviewDecisionCommitted)
        );
    }
```

**Corrected accessor.** `committed_audit_events` returns
`Result<Vec<AuditEnvelopeV1>, TaskStoreError>` (`task_store.rs:1141`-1144), and
`AuditEnvelopeV1` has **no** `kind()` — its accessors are `schema_version`,
`event_id`, `occurred_at_unix_ms`, `event`, `correlation`, and
`event_payload_digest` (`audit.rs:442`-464). `kind()` lives on `AuditEventV1`
(`audit.rs:322`), so the chain is `e.event().kind()`. `AuditEventKindV1` derives
`PartialEq` (`audit.rs:237`), so `contains(&expected)` compiles.

- [ ] **Step 1a: Cover the two events a happy path cannot produce**

Gate A1 item 8 lists `TaskTerminated`, and spec §8 item 8 additionally requires
`AuthorityDenied`. Neither appears in a successful lifecycle, so the happy-path
test above cannot cover them and the earlier draft of this task left both
untested.

```rust
    #[test]
    fn a_failed_caption_run_appends_task_terminated() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        // Drive to Running, then record a terminal, as Task 16's mapping does.
        let task_id = drive_caption_lifecycle_to_terminal(
            &store,
            rollshot_agent::product_task::TaskTerminal::BudgetExhausted {
                dimension: "wall_time".to_owned(),
            },
        );

        let kinds: Vec<_> = store
            .committed_audit_events(&task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.event().kind())
            .collect();

        assert!(kinds.contains(&AuditEventKindV1::TaskTerminated), "{kinds:?}");
        assert!(
            !kinds.contains(&AuditEventKindV1::ArtifactPromoted),
            "a budget-exhausted run must never promote an artifact: {kinds:?}"
        );
    }

    #[test]
    fn an_authority_denied_submit_appends_authority_denied_and_does_not_promote() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        // Authority with an empty grant set, plus a provider that submits.
        let task_id = drive_caption_lifecycle_with_no_grants(&store);

        let kinds: Vec<_> = store
            .committed_audit_events(&task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.event().kind())
            .collect();

        assert!(kinds.contains(&AuditEventKindV1::AuthorityDenied), "{kinds:?}");
        assert!(!kinds.contains(&AuditEventKindV1::ArtifactPromoted), "{kinds:?}");
        assert_eq!(
            store.load(&task_id).unwrap().artifact_metadata(),
            None,
            "a denied submit must leave no artifact metadata"
        );
    }
```

- [ ] **Step 2: Write the privacy tests**

```rust
    #[test]
    fn caption_audit_journal_holds_no_caption_or_step_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_full_caption_lifecycle(&store);

        let journal = read_journal_to_string(dir.path(), &task_id);

        for secret in [
            "The settings panel appears.",
            "Open Settings",
            "Suggest concise Action Guide titles",
        ] {
            assert!(
                !journal.contains(secret),
                "audit journal leaked {secret:?}: {journal}"
            );
        }
    }

    #[test]
    fn caption_task_file_holds_no_image_bytes_and_no_skill_body() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_full_caption_lifecycle(&store);

        // `ProductTaskId::as_str()` already begins with "task-"
        // (product_task.rs:29), and `task_path` writes
        // `<tasks_dir>/{id}.json` (task_store.rs:398). Re-adding the prefix
        // here would look for `task-task-<uuid>.json` and panic on unwrap.
        let raw = std::fs::read_to_string(
            dir.path()
                .join("agent-tasks/tasks")
                .join(format!("{}.json", task_id.as_str())),
        )
        .unwrap();

        assert!(!raw.contains("Suggest concise Action Guide titles"),
            "the skill body must not be persisted; only its digest");
        assert!(!raw.contains("base_image_sha256"),
            "a caption binding must not carry image fields");
        assert!(!raw.contains("iVBORw0KGgo"),
            "no PNG payload may reach the task store");

        // Positive counterpart, so the three negatives above cannot all pass
        // simply because nothing was written: the digest IS present, and it is
        // the caption package that was bound.
        let snapshot = store.load(&task_id).unwrap();
        let contract = snapshot
            .attempts()
            .last()
            .unwrap()
            .run_contract()
            .expect("a caption task always binds a run contract");
        assert_eq!(contract.skill_use.package_id, "action-guide-captions");
        assert_eq!(contract.skill_use.package_digest.len(), 64);
        assert!(raw.contains(&contract.skill_use.package_digest));
        assert_eq!(
            contract.authority.disclosure_ceiling,
            rollshot_agent::authority::DisclosureCeiling::TextMetadataOnly
        );
        assert_eq!(
            contract.authority.granted_operations,
            vec![rollshot_agent::authority::RunOperation::SubmitReviewCandidate]
        );
    }
```

That last block is also the evidence for **Gate A1 item 1** ("binds a
`RunContractReceiptV1` carrying the authority receipt and the caption skill
digest"), which no other test in this plan covers.

The journal path for `read_journal_to_string` is
`<config_dir>/agent-tasks/audit/<task_id>.jsonl` — `JOURNAL_FILE_PREFIX` is
`"task-"` and `journal_path` strips and re-adds it
(`audit_store/mod.rs:30`-31, `:713`-719), so the filename is `task-<uuid>.jsonl`,
i.e. `format!("{}.jsonl", task_id.as_str())`. Same no-double-prefix rule.

- [ ] **Step 3: Run the tests**

Run: `rtk cargo test -p rollshot-app --features action-guide caption_task_lifecycle`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide caption_audit_journal`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide task_terminated authority_denied`
Expected: PASS, both Step 1a tests.

- [ ] **Step 4: Commit**

```bash
rtk git add -A crates/rollshot-app
rtk git commit -m "test(action): cover caption audit coverage and privacy bounds"
```

---

## Task 21: iced UI evidence for the restore path

**Files:**
- Whatever the `testing-iced-ui` skill's scenario layout requires.

**Interfaces:**
- Consumes: Task 18's restore.
- Produces: scenario evidence and, after independent review, any allowed
  baseline update.

- [ ] **Step 1: Invoke the repo-local skill before editing**

Invoke the `testing-iced-ui` skill. Use its auto mode; switch to human mode only
if the user explicitly asks.

- [ ] **Step 2: Build the restore scenario**

The scenario must show the timeline workspace opening a project that has a
stored `ReadyForReview` caption task, with the review surface populated and no
provider configured — proving restore needs no provider call.

- [ ] **Step 3: Send raw evidence to an independent reviewer**

The implementing agent must NOT write or approve golden baselines. Dispatch an
independent subagent started with `fork_turns="none"` and pass it the raw
scenario evidence. Auto-mode acceptance also requires the skill's behavioral
image-capability probe to report semantic inspection; pixel-only or unavailable
inspection requires a capable clean-context reviewer or explicit human mode.

- [ ] **Step 4: Commit whatever the reviewer authorized**

```bash
rtk git add -A
rtk git commit -m "test(ui): cover caption proposal restore"
```

---

## Task 22: Gate A1 evidence and final verification

**Files:**
- Create: `docs/superpowers/spikes/2026-07-28-action-guide-captions-decision.md`

**Interfaces:**
- Consumes: every prior task.
- Produces: the gate decision record the umbrella requires before Slice B starts.

- [ ] **Step 1: Run the full verification set**

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide
rtk cargo test -p rollshot-app
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all green. Record the actual counts; do not claim a pass without the
output.

- [ ] **Step 2: Write the gate decision record**

Follow the house style of
`docs/superpowers/spikes/2026-07-28-audit-observability-decision.md`: status,
date, branch, commit, then the selected architecture, a table mapping each of
Gate A1's ten items to its evidence, the Slice A extras (schema fixtures,
two-domain concurrency, prompt text assertion, authority-digest audit result),
migrations performed, and residual risks.

State explicitly whether Task 7's audit came back clean and which digest formula
was therefore adopted.

The Gate A1 evidence table must cite these tests by name; each of the ten items
has at least one:

| Gate A1 item | Evidence |
|---|---|
| 1. Durable task, new kind, run contract bound | Task 20 `caption_task_file_holds_no_image_bytes_and_no_skill_body` (the run-contract assertions), Task 20 `caption_task_lifecycle_appends_every_material_event` |
| 2. Typed artifact bound to origin | Task 17 `promotion_binds_the_kind_the_origin_and_the_payload_digest` |
| 3. Review receipt bound to artifact revision | Task 17 `review_receipt_partitions_decisions_and_binds_the_artifact_revision`, `accepted_suggestions_land_in_applied_not_rejected` |
| 4. Deterministic stale rejection | Task 3 `identity_ignores_freshness_and_rejects_other_domains`, Task 18 `restore_declines_and_marks_stale_when_the_revision_moved`, `restore_is_deterministic_across_repeated_calls` |
| 5. Reconciliation after restart | Task 19 `open_marks_ephemeral_ready_for_review_stale`, `open_marks_running_tasks_interrupted_for_both_domains`, `open_is_idempotent_across_two_restarts` |
| 6. Restore without a provider call | Task 18 `restore_repopulates_the_review_surface_without_a_provider` (panicking adapter) |
| 7. Budget and cancellation honored | Task 15 `single_submit_reports_wall_time_exhaustion`, `..._cancellation_before_the_first_turn`, `..._cancellation_mid_stream`; Task 16 `wall_time_exhaustion_reports_the_frozen_timeout_copy` |
| 8. Audit coverage, privacy-safe | Task 20 all five tests, including `a_failed_caption_run_appends_task_terminated` and `an_authority_denied_submit_appends_authority_denied_and_does_not_promote` |
| 9. Smart Redaction unregressed, V1 fixtures load | Task 4 `loads_pre_migration_schema_fixtures`, Task 5 `legacy_flat_dry_run_counters_become_a_smart_redaction_summary`, plus the whole workbench suite |
| 10. Restore path UI evidence | Task 21 |

Residual risks to record, all identified during plan review:

1. **The text-JSON caption fallback is gone.** `SKILL.md` still instructs the
   model to return bare JSON when tool calling is unavailable (preserved verbatim
   by design), but `run_single_submit_with_provider` treats a completion with no
   terminal tool call as `ProtocolFailure`. Providers without tool calling now
   fail where they previously succeeded. Task 16 Step 5a records the behavior;
   correcting the instruction text is deliberately out of scope
   (spec §9: "no caption prompt improvement").
2. **Project identity is a canonicalized path digest.** Already accepted in spec
   §10; moving a project orphans pending tasks to `Stale`.
3. **`ProductArtifactMetadata` gained a hand-written `Deserialize`.** Adding a
   field to that struct now requires editing two places. Task 5 Step 3a explains
   why; a future slice that drops V1/V2 on-disk support should delete the shim.
4. **The open-time sweep and `reconcile_for_source` both own the
   non-terminal-to-`Interrupted` rule.** Task 19 factors the decision into one
   helper; if a later slice changes one, it must change the helper.
5. **The two workspaces are separate processes.** The umbrella and child spec
   describe "one store shared into both workspaces"; in code, `main.rs` dispatches
   into mutually exclusive iced applications, so the invariant is enforced per
   workspace root. State this, so Slice B does not plan against a shared owner
   that does not exist.

- [ ] **Step 3: Request independent code review**

Use `superpowers:requesting-code-review`. The gate requires independent review
before the decision.

- [ ] **Step 4: Commit**

```bash
rtk git add docs/superpowers/spikes/2026-07-28-action-guide-captions-decision.md
rtk git commit -m "docs(agent): Gate A1 decision for Action Guide caption provenance"
```

---

## Self-Review

**Spec coverage.** Every numbered spec section maps to at least one task:

| Spec section | Task |
|---|---|
| §3.1 `SourceBinding` | 2, 3 |
| §3.2 `ArtifactSummary` | 5 |
| §3.3 payload surface | 6 |
| §3.4 new variants | 9, 10 |
| §3.5 `TextMetadataOnly` | 9 |
| §3.6 `AuthoritySubject` + digest uncertainty | 7, 8 |
| §3.7 store schema | 4 |
| §3.8 store module move | 11 |
| §4.1 single-submit profile | 15 |
| §4.2 caption skill | 13, 14 |
| §4.3 authority construction | 16 |
| §4.4 budget and preserved behavior | 1, 16 |
| §5.1 lifecycle mapping | 16, 17 |
| §5.2 batch review | 17 |
| §5.3 restore | 18 |
| §5.4 ephemeral reconciliation | 19 |
| §6 failure semantics | 15, 16 |
| §7 privacy | 20 |
| §8 Gate A1 mapping | 21, 22 |

**Placeholder scan.** Three tasks deliberately delegate rather than transcribe,
and each names the exact source to copy from: Task 13 Step 4 (mirror the bundled
Smart Redaction resolver at `skills.rs:977`-1019), Task 15 Step 4 (copy the run
body and apply six named substitutions), and Task 21 (the `testing-iced-ui`
skill owns the scenario layout). Mechanical fan-out — fixture renames, importer
path updates, non-exhaustive `match` repairs — is specified as a discovery
command plus a single transformation rule, because the compiler enumerates those
sites more reliably than a hand-written list. Where a fan-out is large enough that
a missed site leaves a suite red rather than failing to compile, the sites are now
enumerated explicitly: Task 5 (`ProductArtifactMetadata::new`, 14 sites), Task 6
(`record_ready_for_review`, ~35 sites across both crates), Task 8
(`AuthorityBinding::new`, 14 sites; the receipt field rename, 4 sites).

**Type consistency.** `SourceBinding::smart_redaction` is the constructor name
used in Tasks 2, 3, 4, and 12. `new_v3` is used for both `ProductTaskSnapshot`
(Task 4) and `ProductArtifactMetadata` (Task 5) — distinct types, same version
suffix, intentional. `AuthoritySubject` is spelled identically in Tasks 8 and 16.
`caption_source_binding`, `caption_authority`, `caption_artifact_payload`,
`caption_review_receipt`, and `restore_caption_proposal` each appear in exactly
one producing task and are consumed by name later.

One gap found and closed during the plan's own self-review: Task 18 needs
`CaptionProposal` to be serializable, which no earlier task provided. The derive
additions are now explicit in Task 18 Step 3. `CandidateId`, `FrameId`, and
`Millis` are `u64` aliases (`rollshot-action/src/models.rs:6`-10), so they need no
derives.

## Engineering review corrections (2026-07-28)

An independent engineering review verified this plan's ~40 `file.rs:line`
citations and reproduced type shapes against code. The following were wrong and
have been fixed in place. Anything not listed was confirmed correct.

**API names that do not exist:**

- `AuthoritySnapshot::binding()` — no such accessor; all fields are private
  (`authority.rs:106`-117). Task 15 now uses `authority.run_id()`
  (`authority.rs:227`).
- `ProductTaskId::new_v4()`, `ArtifactId::new_v4()`, `RunId::new_v4()` — none
  exist; those types expose only `parse` (`product_task.rs:27`, `:59`;
  `domain.rs:35`). Only `AuditEventId::new_v4()` is real (`audit.rs:62`). Fixed in
  Tasks 4, 5, 8, 12, 16.
- `AuditEnvelopeV1::kind()` — lives on `AuditEventV1`, not the envelope
  (`audit.rs:322` vs `:442`-464). Task 20 now uses `e.event().kind()`.
- `rollshot_agent::runtime::RunId` — `RunId` is in `domain` (`domain.rs:32`).
  Fixed in Task 16.
- Test-fixture names: `running_task_with_contract_fixture` →
  `running_with_contract_fixture` (`product_task.rs:2307`);
  `artifact_metadata_for` → `v2_metadata_with_contract` (`:2313`);
  `snapshot_with_ceiling` → `snapshot_with_disclosure` (`authority.rs:422`);
  `authorized_input_with_one_png` / `authorized_input_without_attachments` →
  `png_input` / `input_without_attachments` (`authority.rs:460`, `:476`);
  `authority_snapshot_fixture` → `full_snapshot` (`authority.rs:439`).

**Things that would not compile or would not stay green:**

- Task 11's `pub use audit_store::{AuditJournal, AuditStoreError}` is E0364/E0365
  — every `audit_store` item is `pub(crate)`. Now `pub(crate) use`, and
  `TaskAuditSink` (needed by `run.rs:1483`) was added to the list.
- Task 14's `AgentTaskProfile::Captions` would be a non-exhaustive match in two
  methods and a `dead_code` denial under `clippy -D warnings`. Arms added, the
  allowance kept, and a parity test added.
- Task 15's substitution list omitted `SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS`
  (`driver.rs:1870`, `:1908`), so the caption run would reject its own tool call by
  name. Sixth substitution added, and the `CallTools` arm's budget charge and
  registry round trip are now explicitly preserved.
- Task 13 breaks `bundled_smart_redaction_manifest_accepted`'s
  `entries.len() == 1` assertion (`skills.rs:2054`). Step 4a updates it.
- Task 16's four pre-existing `provider_tests` cannot survive the rewrite
  unchanged. Step 5a gives each a disposition and records the removal of the
  text-JSON fallback.
- Task 5 breaks on-disk `ProductArtifactMetadata` compatibility, which Task 4 did
  not cover. Step 3a adds a legacy-tolerant `Deserialize` and Task 4 adds a
  `ReadyForReview` fixture to detect it.
- Task 20's task-file path double-prefixed `task-`, so the `unwrap()` would panic.

**Tests that would have passed for the wrong reason:**

- Task 17's `suggestion_ids_above_u32_are_rejected_not_truncated` left the mutated
  suggestion `Pending`, which the narrowing guard skips entirely; it now rejects it
  first.
- Task 18's restore test proved nothing about "no provider call"; it now uses a
  panicking adapter, as spec §8 item 6 requires.
- Task 13's `digest().len() == 64` did not check hex and pinned no value; a golden
  digest test was added, mirroring `skills.rs:2068`.
- Task 20's negatives could all pass on an empty file; positive run-contract
  assertions were added, which double as the missing Gate A1 item 1 evidence.

**Coverage gaps closed:** Gate A1 item 8's `TaskTerminated` and spec §8's
`AuthorityDenied` (Task 20 Step 1a); budget exhaustion and mid-stream cancellation
for item 7 (Task 15 Steps 1, 5a); the durable-origin promotion path and
`canonical_payload_sha256` for item 2 (Task 17); determinism and identity-mismatch
for item 4 (Task 18); `TaskStatus::Created` in the open-time sweep (Task 19).

**Confirmed correct, do not re-litigate:** the `#[serde(untagged)]` +
`[u8; 32]` legacy deserializer (empirically verified, Task 4); `meta.run_contract =
Some(..)` from inside `new_v2` (same module, private field in scope, Task 5);
`OsStr::as_encoded_bytes()` under `rust-version = "1.94"` (Task 16);
`TaskStoreContinuitySource` exists at `task_store.rs:1417` (Task 11);
`ArtifactRevision::new` and `TaskAttemptId::new` exist with that arity
(`product_task.rs:91`, `:111`); `mark_stale` is legal from `ReadyForReview` and
illegal elsewhere (`product_task.rs:1130`); the caption instruction baseline is
byte-exact against `caption_agent.rs:112`-115; the continuity source-binding
digest is never persisted, so changing its formula is safe.

## NOT in scope

Carried from the child spec §9 and the umbrella, plus items considered and
deferred during engineering review:

| Deferred | Why |
|---|---|
| Any change to `run_visual_annotation_with_provider` | Slice B owns the migration onto the new profile; touching it here removes the falsification value of Gate B1 |
| Caption prompt improvement, caption eval harness | Spec §9. Task 13 moves the text verbatim so a later change has a clean baseline |
| Fixing `SKILL.md`'s "return only JSON" sentence now that the text fallback is gone | Would be a prompt change, which §9 forbids. Recorded as residual risk 1 in Task 22 |
| New UI surface, affordance, or copy | Umbrella constraint. Restore reuses the existing review surface; the four failure strings are frozen |
| A `new_v4()` constructor on `ProductTaskId` / `ArtifactId` / `RunId` | Tempting DRY win for the ~20 `parse(format!(..))` sites, but it is a public API addition no spec section asks for. Raise it as its own change |
| Moving `load_provider_config` / `build_adapter` out of the Smart Redaction module | Spec §10 "noted, not done". The timeline workspace keeps cross-importing them (`timeline_workspace/update.rs:1237`, `:1253`) |
| Unifying `ActionGuideContextProjectionV1` with `rollshot_agent::continuity` | Spec §9 |
| A new `RunOperation` variant | Spec §9 / §4.3: the guide is composed into the prompt, so there is nothing to read |
| Project manifest schema change (adding a stable project UUID) | Spec §3.1 rejected it; path digest accepted with `Stale` as the consequence of a move |
| Dropping the V1/V2 on-disk shims | Both compatibility deserializers (`SourceBinding`, `ProductArtifactMetadata`) exist only for pre-migration files. Deleting them is a separate, later decision |

## Execution order

Sequential execution, no parallelization opportunity. The umbrella (§7) already
forbids parallel slices; within Slice A the tasks form a single chain:

- Tasks 2-10 all rewrite the same two files (`rollshot-agent/src/product_task.rs`
  and `src/authority.rs`) and each one's fixture sweep depends on the previous
  one's shape.
- Task 11 moves files that Tasks 2-6 edit, so it cannot start until they land.
- Tasks 12-22 all depend on Task 11's module paths.
- Tasks 13 and 14 (skills + prompt) are the only pair with no file overlap with
  Tasks 2-10, but Task 14 depends on Task 13 and both are small; splitting them
  into a lane buys nothing against the merge risk on `driver.rs`.

Run `superpowers:executing-plans` task by task, in order.
