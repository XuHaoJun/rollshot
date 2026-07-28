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
- Produces: `CAPTION_INSTRUCTION_BASELINE` — a `const &str` in the test module
  holding today's exact static instruction text. Task 13 and Task 14 assert
  against it.

- [ ] **Step 1: Write the failing test**

Add to the existing `mod tests` in
`crates/rollshot-app/src/timeline_workspace/caption_agent.rs`:

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

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/caption_agent.rs
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

**Interfaces:**
- Consumes: the `SourceBinding` enum from Task 2.
- Produces: `ProductTaskSnapshot::new_v3(task_id, kind, source_binding, now)
  -> Result<Self, TaskContractError>` writing `store_schema_version: 3`.

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
    fn new_v3_writes_schema_three() {
        let task = ProductTaskSnapshot::new_v3(
            ProductTaskId::new_v4(),
            TaskKind::SmartRedactionAuthor,
            source_binding_fixture(),
            1_000,
        )
        .unwrap();

        assert_eq!(task.store_schema_version(), 3);
    }
```

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
  "task_id": "00000000-0000-4000-8000-000000000001",
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

Add the load test to the `#[cfg(test)] mod tests` in
`crates/rollshot-app/src/result_workspace/workbench/task_store.rs`:

```rust
    #[test]
    fn loads_pre_migration_schema_fixtures() {
        for (name, expected_version) in [
            ("task-schema-v1.json", 1u32),
            ("task-schema-v2.json", 2u32),
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
  (`ProductArtifactMetadata`)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  (promotion construction sites)

**Interfaces:**
- Consumes: nothing from earlier tasks.
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

Add the fixture helper next to the existing `source_binding_fixture`:

```rust
    fn artifact_metadata_fixture_v3(summary: ArtifactSummary) -> ProductArtifactMetadata {
        ProductArtifactMetadata::new_v3(
            ArtifactId::new_v4(),
            ArtifactRevision::new(1),
            ArtifactKind::SmartRedaction,
            1,
            "aa".repeat(32),
            source_binding_fixture(),
            ProductTaskId::new_v4(),
            TaskAttemptId::new(1),
            RunId::new_v4(),
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

Delete the old `new` constructor if no caller remains after Step 4; otherwise
convert it the same way with `run_contract: None`.

- [ ] **Step 4: Update the promotion sites**

```bash
rtk grep -rn "ProductArtifactMetadata::new" crates/ --include="*.rs"
```

Smart Redaction sites keep calling `new_v2` unchanged. No behavioral edit is
needed at this task.

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
  (`record_ready_for_review`)
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs` (the
  Smart Redaction promotion call)

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
        let task = running_task_with_contract_fixture();
        let payload = br#"{"suggestions":[]}"#.to_vec();

        let promoted = task
            .record_ready_for_review(
                artifact_metadata_for(&task, ArtifactSummary::ActionGuideCaptions {
                    suggestion_count: 0,
                }),
                payload.clone(),
                None,
                2_000,
            )
            .unwrap();

        assert_eq!(promoted.status(), TaskStatus::ReadyForReview);
        assert_eq!(promoted.pending_artifact_payload(), Some(payload.as_slice()));
    }
```

`running_task_with_contract_fixture` and `artifact_metadata_for` already have
equivalents in the existing test module around `product_task.rs:2338` and
`:2405`; reuse or adapt those rather than writing new ones.

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

- [ ] **Step 4: Update the Smart Redaction caller**

Find the call:

```bash
rtk grep -rn "record_ready_for_review(" crates/rollshot-app --include="*.rs"
```

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

- [ ] **Step 1: Search for every persisted-digest comparison**

```bash
rtk grep -rn "snapshot_digest\|document_binding_digest\|binding_digest_hex" crates/ --include="*.rs"
```

For each hit, classify it as one of:
- compares a value against itself or against another in-memory value (safe);
- copies the value into a projection or receipt (safe);
- **recomputes a digest from a loaded snapshot and compares it to a persisted
  string (unsafe — triggers the fallback).**

- [ ] **Step 2: Write the pinning test**

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
        let snapshot = authority_snapshot_fixture();
        let first = snapshot.digest().to_string();
        let receipt = snapshot.receipt(1_000);

        assert_eq!(receipt.snapshot_digest, first);
        assert_eq!(snapshot.digest(), first, "digest must be cached, not recomputed");
    }
```

If `authority_snapshot_fixture` does not exist in the module's tests, build one
from `AuthoritySnapshot::new` with the `Document` binding used by the existing
tests.

- [ ] **Step 3: Record the finding in the plan file**

Append the audit result to this task in the plan document, replacing this step's
text with the classification table produced in Step 1. Then commit the plan
change together with the test.

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

```rust
    #[test]
    fn action_guide_subject_authorizes_submit_and_rejects_image_ops() {
        let subject = AuthoritySubject::ActionGuideProject {
            project_root_sha256: [4u8; 32],
            revision: 2,
            projection_digest: "ab".repeat(32),
        };
        let run_id = RunId::new_v4();
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(
                ProductTaskId::new_v4(),
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
        let run_id = RunId::new_v4();
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(
                ProductTaskId::new_v4(),
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

- [ ] **Step 4: Keep the persisted receipt key**

In `AuthoritySnapshotReceiptV1`, the field may be renamed in Rust but the
serialized key must not change, or task JSON containing
`RunContractReceiptV1` stops loading:

```rust
    #[serde(rename = "document_binding_digest")]
    pub subject_digest: String,
```

- [ ] **Step 5: Update every caller**

```bash
rtk grep -rn "authorize_tool(\|AuthorityBinding::new(\|document_binding()" crates/ --include="*.rs"
```

Smart Redaction sites wrap their existing binding:
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
        let snapshot = snapshot_with_ceiling(DisclosureCeiling::TextMetadataOnly);
        let with_attachment = authorized_input_with_one_png();

        assert!(matches!(
            snapshot.validate_model_input(&with_attachment),
            Err(AuthorityError::DisclosureExceeded { .. })
        ));
        assert!(snapshot
            .validate_model_input(&authorized_input_without_attachments())
            .is_ok());
    }
```

Reuse the existing disclosure tests' helpers for building an
`AuthorizedModelInput`; if they are inline, factor them into
`authorized_input_with_one_png` and `authorized_input_without_attachments` and
update the existing tests to call them.

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

Then compile and fix every non-exhaustive `match` the compiler reports.

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
- Modify: `crates/rollshot-app/src/main.rs` (declare the module, open the store
  once)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:1664` (stop
  opening it here)

**Interfaces:**
- Consumes: everything from Tasks 2-10.
- Produces:
  - `crate::agent_store::{TaskStore, TaskStoreError, StoreCommitOutcome,
    Failpoint, AuditJournal, AuditStoreError}` — re-exported from
    `agent_store/mod.rs` so importers change only their path prefix.
  - `crate::agent_store::open_process_store(config_dir: &std::path::Path)
    -> Result<std::sync::Arc<TaskStore>, TaskStoreError>` — the only production
    constructor.

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

pub use audit_store::{AuditJournal, AuditStoreError};
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
compiler reports each one.

- [ ] **Step 5: Open the store once at app start**

In `crates/rollshot-app/src/result_workspace/update.rs:1664`, delete the
`TaskStore::open(&config_dir)` call and take the store from application state
instead. The application root opens it once with `open_process_store` and hands
an `Arc` clone to each workspace; `workbench.task_store` keeps its existing
`Option<std::sync::Arc<TaskStore>>` type, so only the source of the value
changes.

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

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn concurrent_audited_creates_from_two_domains_both_commit() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();

        let smart = ProductTaskSnapshot::new_v3(
            ProductTaskId::new_v4(),
            TaskKind::SmartRedactionAuthor,
            SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 0, "p".into(), None),
            1_000,
        )
        .unwrap();
        let captions = ProductTaskSnapshot::new_v3(
            ProductTaskId::new_v4(),
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

        assert!(store.load(&smart_id).is_ok());
        assert!(store.load(&captions_id).is_ok());
    }
```

- [ ] **Step 2: Run it**

Run: `rtk cargo test -p rollshot-app --features action-guide concurrent_audited_creates_from_two_domains`
Expected: PASS. If it hangs, a nested lock acquisition exists — find it with
`rtk grep -n "acquire_lock" crates/rollshot-app/src/agent_store/task_store.rs`
and confirm no locked function calls another locked function.

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
- Modify: `crates/rollshot-agent/src/skills.rs` (bundled resolver, near
  `bundled_smart_redaction_use` at `skills.rs:977-1010`)

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
    }
```

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
bounded-catalog limits apply to the pair, and add
`bundled_action_guide_captions_use()` returning the resolved `SkillUse`.

- [ ] **Step 5: Run the tests**

Run: `rtk cargo test -p rollshot-agent skills`
Expected: PASS, including the pre-existing bundled Smart Redaction tests.

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

Add the profile variant and drop the now-unnecessary `dead_code` allowance:

```rust
pub(crate) enum AgentTaskProfile {
    VisualAnnotation,
    Captions,
}
```

`AgentTaskProfile::system_prompt` returns a `&'static str`, which a
digest-bearing composed prompt cannot be. Leave `system_prompt` serving
`VisualAnnotation` only and pass the caption system prompt as an owned `String`
parameter to the profile run in Task 15. Do not change the visual annotation
path.

- [ ] **Step 4: Run to verify it passes**

Run: `rtk cargo test -p rollshot-agent caption_prompt`
Expected: PASS, both tests.

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
- Modify: `crates/rollshot-agent/src/driver.rs` (add
  `run_single_submit_with_provider` and `SingleSubmitTerminal` after
  `run_visual_annotation_with_provider`, which ends at `driver.rs:1989`)

**Interfaces:**
- Consumes: `AuthoritySubject`, `RunOperation`, `AgentTaskProfile::Captions`.
- Produces:
  - `SingleSubmitTerminal { Submitted { arguments: serde_json::Value },
    Cancelled, BudgetExhausted { dimension: BudgetDimension }, ProviderFailure,
    ProtocolFailure, AuthorityDenied { operation: RunOperation } }`
  - `SingleSubmitProfile { tool_definition: ToolDefinition, tool:
    std::sync::Arc<dyn ...>, system_prompt: String, required_operation:
    RunOperation, tracing_target: &'static str }`
  - `AgentRunner::run_single_submit_with_provider(&self, profile:
    SingleSubmitProfile, input: AuthorizedModelInput, provider: &dyn
    ProviderAdapter, budget: RunBudget, cancellation: &RunCancellation,
    authority: &AuthoritySnapshot, subject: &AuthoritySubject, audit_sink:
    Option<&dyn AuditAppendSink>) -> SingleSubmitTerminal`

- [ ] **Step 1: Write the failing tests**

Model them on the existing visual annotation driver tests near `driver.rs:2008`,
which already build a mock rig provider. Add:

```rust
    #[tokio::test]
    async fn single_submit_returns_raw_arguments_on_submit() {
        let terminal = run_caption_profile_with(vec![
            tool_call_delta_name("call-1", "submit_caption_suggestions"),
            tool_call_delta_args("call-1", r#"{"suggestions":[]}"#),
        ])
        .await;

        match terminal {
            SingleSubmitTerminal::Submitted { arguments } => {
                assert!(arguments.get("suggestions").is_some());
            }
            other => panic!("expected Submitted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn single_submit_denies_without_the_required_grant() {
        // Authority granting nothing: the submit tool must never run.
        let terminal = run_caption_profile_with_grants(BTreeSet::new()).await;

        assert!(matches!(
            terminal,
            SingleSubmitTerminal::AuthorityDenied {
                operation: RunOperation::SubmitReviewCandidate
            }
        ));
    }

    #[tokio::test]
    async fn single_submit_rejects_attachments_above_the_ceiling() {
        let terminal = run_caption_profile_with_ceiling_and_attachment(
            DisclosureCeiling::TextMetadataOnly,
        )
        .await;

        assert!(matches!(terminal, SingleSubmitTerminal::ProtocolFailure));
    }

    #[tokio::test]
    async fn single_submit_reports_cancellation_before_the_first_turn() {
        let cancellation = RunCancellation::new();
        cancellation.cancel();

        let terminal = run_caption_profile_cancelled(cancellation).await;

        assert!(matches!(terminal, SingleSubmitTerminal::Cancelled));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-agent single_submit_returns_raw_arguments`
Expected: FAIL to compile — `SingleSubmitTerminal` not found.

- [ ] **Step 3: Add the terminal and the profile input**

```rust
/// Outcome of a bounded single-submit-tool run. Semantic decoding of
/// `Submitted { arguments }` belongs to the caller: a schema-agnostic profile
/// cannot tell a suggestion batch from a model declining to suggest.
#[derive(Debug)]
pub enum SingleSubmitTerminal {
    Submitted { arguments: serde_json::Value },
    Cancelled,
    BudgetExhausted { dimension: BudgetDimension },
    ProviderFailure,
    ProtocolFailure,
    AuthorityDenied { operation: RunOperation },
}
```

- [ ] **Step 4: Copy the run body and apply exactly five substitutions**

Copy the whole body of `run_visual_annotation_with_provider`
(`driver.rs:1692`-1989) into `run_single_submit_with_provider`. Then apply these
substitutions and nothing else:

1. Every `VisualAnnotationRunTerminal::X` return becomes
   `SingleSubmitTerminal::X`. `map_budget_error_to_visual_annotation` becomes a
   new `map_budget_error_to_single_submit` with the same two arms.
2. `submit_visual_annotation_suggestions_definition()` and
   `submit_visual_annotation_suggestions_tool_arc()` become
   `profile.tool_definition.clone()` and `profile.tool.clone()`.
3. `AgentTaskProfile::VisualAnnotation.system_prompt().to_string()` becomes
   `profile.system_prompt.clone()`.
4. Every `target: "rollshot::agent::visual_annotation"` becomes
   `target: profile.tracing_target`.
5. The tool-result step's `decode_visual_annotation_terminal(&args)` call and
   its error branches are replaced by returning
   `SingleSubmitTerminal::Submitted { arguments: args }` after the authorization
   check added in Step 5.

- [ ] **Step 5: Add the four behaviors the visual annotation version lacks**

Insert the disclosure check in the pre-flight block, immediately after the
existing cancellation check:

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
                            authority.binding().run_id(),
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

`append_authority_denied` is a small private helper wrapping the existing
`AuditAppendSink` call shape already used by `run_with_provider` for
`AuthorityDenied`; read that call site and reuse it verbatim.

The skill use reaches the run through `profile.system_prompt`, which Task 14's
`compose_caption_prompt` produced from the resolved `SkillUse`. That is how the
digest enters the transcript.

- [ ] **Step 6: Run the tests**

Run: `rtk cargo test -p rollshot-agent single_submit`
Expected: PASS, all four tests.

Run: `rtk cargo test -p rollshot-agent visual_annotation`
Expected: PASS — the visual annotation path is untouched.

- [ ] **Step 7: Commit**

```bash
rtk git add -A crates/rollshot-agent
rtk git commit -m "feat(agent): add an authority-aware single-submit run profile"
```

---

## Task 16: Caption run wiring

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs:1234-1275`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs` (workspace state
  gains the store handle, the cancellation, and the task id)

**Interfaces:**
- Consumes: `open_process_store`, `new_v3`, `TaskKind::ActionGuideCaptions`,
  `AuthoritySubject`, `DisclosureCeiling::TextMetadataOnly`,
  `bundled_action_guide_captions_use`, `compose_caption_prompt`,
  `run_single_submit_with_provider`, `SingleSubmitTerminal`.
- Produces:
  - `caption_run_budget() -> RunBudget` in `rollshot-agent`
  - `caption_source_binding(context: &PreparedCaptionContext, project_root:
    Option<&std::path::Path>) -> SourceBinding`
  - `caption_authority(task_id, run_id, subject) -> Result<AuthoritySnapshot,
    String>`

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

In `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`:

```rust
    #[test]
    fn caption_authority_grants_only_submit_and_forbids_images() {
        let subject = rollshot_agent::authority::AuthoritySubject::ActionGuideProject {
            project_root_sha256: [7u8; 32],
            revision: 3,
            projection_digest: "ab".repeat(32),
        };
        let run_id = rollshot_agent::runtime::RunId::new_v4();

        let authority = caption_authority(
            rollshot_agent::product_task::ProductTaskId::new_v4(),
            run_id.clone(),
            subject.clone(),
        )
        .unwrap();

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
        let ephemeral = caption_source_binding(&ephemeral_context(), None);

        assert!(matches!(
            ephemeral,
            rollshot_agent::product_task::SourceBinding::ActionGuideEphemeralGuide { .. }
        ));
    }
```

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
    run_id: rollshot_agent::runtime::RunId,
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
   - `BudgetExhausted { dimension: BudgetDimension::WallTime }` →
     `Err(TIMEOUT_MESSAGE.to_string())` plus `record_terminal`;
   - every other terminal → `record_terminal` with the matching
     `TaskTerminal`, and the existing user-visible copy.

Every store call runs inside `tokio::task::spawn_blocking`, following
`run.rs:1090`-1105.

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

- [ ] **Step 7: Run the suites**

Run: `rtk cargo test -p rollshot-agent`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide caption`
Expected: PASS, including Task 1's baseline tests and the pre-existing
`provider_tests`.

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
        proposal.suggestions[0].id = rollshot_action::CaptionSuggestionId(u64::from(u32::MAX) + 1);
        let metadata = caption_artifact_metadata_fixture();

        assert!(caption_review_receipt(&proposal, &metadata, 5_000).is_err());
    }
```

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
        let binding = action_guide_binding_fixture();
        seed_ready_for_review_caption_task(&store, &binding);

        let restored = restore_caption_proposal(&store, &binding, 9_000);

        let (_task_id, proposal) = restored.expect("a matching task must restore");
        assert_eq!(proposal.suggestions.len(), 1);
        assert!(proposal.has_pending());
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
```

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
    /// `ReadyForReview` one becomes `Stale`. Any `Running` or `Applying` task in
    /// any domain becomes `Interrupted`, because its process is gone.
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
                (TaskStatus::Running | TaskStatus::Applying, _) => {
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

`all_task_ids` may not exist; if not, derive it from the existing
`sorted_task_entries` used by `reconcile_for_source`, reusing its filename
parsing rather than duplicating it.

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
            .map(|e| e.kind())
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
    }
```

Adapt `.kind()` to whatever accessor `committed_audit_events` returns; read its
signature at `agent_store/task_store.rs` first.

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

        let raw = std::fs::read_to_string(
            dir.path()
                .join("agent-tasks/tasks")
                .join(format!("task-{}.json", task_id.as_str())),
        )
        .unwrap();

        assert!(!raw.contains("Suggest concise Action Guide titles"),
            "the skill body must not be persisted; only its digest");
        assert!(!raw.contains("base_image_sha256"),
            "a caption binding must not carry image fields");
        assert!(!raw.contains("iVBORw0KGgo"),
            "no PNG payload may reach the task store");
    }
```

- [ ] **Step 3: Run the tests**

Run: `rtk cargo test -p rollshot-app --features action-guide caption_task_lifecycle`
Expected: PASS.

Run: `rtk cargo test -p rollshot-app --features action-guide caption_audit_journal`
Expected: PASS.

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
Smart Redaction resolver at `skills.rs:977`-1010), Task 15 Step 4 (copy the run
body and apply five named substitutions), and Task 21 (the `testing-iced-ui`
skill owns the scenario layout). Mechanical fan-out — fixture renames, importer
path updates, non-exhaustive `match` repairs — is specified as a discovery
command plus a single transformation rule, because the compiler enumerates those
sites more reliably than a hand-written list.

**Type consistency.** `SourceBinding::smart_redaction` is the constructor name
used in Tasks 2, 3, 4, and 12. `new_v3` is used for both `ProductTaskSnapshot`
(Task 4) and `ProductArtifactMetadata` (Task 5) — distinct types, same version
suffix, intentional. `AuthoritySubject` is spelled identically in Tasks 8 and 16.
`caption_source_binding`, `caption_authority`, `caption_artifact_payload`,
`caption_review_receipt`, and `restore_caption_proposal` each appear in exactly
one producing task and are consumed by name later.

One gap found and closed during review: Task 18 needs `CaptionProposal` to be
serializable, which no earlier task provided. The derive additions are now
explicit in Task 18 Step 3.
