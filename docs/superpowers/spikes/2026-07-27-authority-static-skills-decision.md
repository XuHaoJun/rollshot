# Gate G2 Decision: Authority and Static Skills Slice 3

**Status:** Proposed for user approval
**Date:** 2026-07-27
**Branch:** `feat/agent-foundation-authority-static-skills`
**Base:** `000c3ee` (docs commit on main)
**Merge base with main:** `1745133833cb4d87429f84fac6d27ff76f6096a9`
**Implementation commits:** `dea0080..6ac4ebe` (12 commits) + `69520fe` (clippy/format fix)

## 1. Selected architecture

Immutable `AuthoritySnapshot` plus bounded `StaticSkillCatalog`, per spec §5.1. The Product constructs one authority snapshot per run, enforces it before every tool call via `ToolRegistry`, resolves one bundled Smart Redaction instruction skill into an immutable `SkillUse`, and binds authority and skill provenance receipts to the Product Task and promoted review artifact.

### Non-goals (spec §4, confirmed not present)

No marketplace, package installer, dependency solver, remote provider, executable extension, script shortcut, skill-defined policy, model-routed skill search, general OS sandbox, live authority broker, credential broker, runtime permission prompt, new screen capture, durable full skill body, model transcript, pixel/OCR/credential storage, workflow DAG, retry system, child agents, user-visible UI change, or visual-baseline update.

## 2. Authority operation matrix per production tool

| Tool | Required operations |
|------|-------------------|
| `replace_source` | `ReadDraft`, `WriteDraft` |
| `read_current_source` | `ReadDraft` |
| `edit_source` | `ReadDraft`, `WriteDraft` |
| `validate_source` | `ReadDraft` |
| `inspect_prepared_image` | `InspectPreparedImage` |
| `submit_review_candidate` | `SubmitReviewCandidate` |
| `request_user_input` | `RequestUserInput` |

Enforcement: `execute_single` checks every `required_operation()` against `AuthoritySnapshot.authorize_tool()` immediately after cancellation check, before per-tool counters increment, before tool body entry. Missing grant returns `ToolError::AuthorityDenied` and halts the batch.

## 3. Catalog evidence

### Source/limit/order/containment/digest

- **Bundled source:** `SmartSource::Bundled` with `smart-redaction` package from `include_str!` of `skills/smart-redaction/skill.toml` and `SKILL.md`.
- **Limits:** `SkillCatalogLimits::v1()` — max 1000 entries, 128 KiB metadata, 16 KiB manifest, 16 KiB body.
- **Order:** Entries sorted by `(source_tier, source_index, package_id)`. Bundled tier wins duplicates.
- **Containment (Unix):** `HostSkillRoot::open` uses `OFlags::NOFOLLOW` on every `openat`. `load_host_packages` uses `NOFOLLOW` for `skill.toml` and `SKILL.md`, verifies `FileType::RegularFile` via `fstat`, rejects special files (FIFO, socket). Path components reject `..`.
- **Digest:** `compute_package_digest` hashes `"rollshot-skill-v1\0" + manifest + body` via SHA-256. Golden digest `26c33ddd...` verified stable in test.
- **Tests:** 47 skills tests covering scale (1000/1001), metadata budget, dedup, deterministic order, digest stability, symlinks, FIFO, socket, oversize manifest/body, stale UTF-8, host-root validation.

## 4. Smart Redaction author/improve invocation evidence

- `bundled_smart_redaction_use()` resolves via `StaticSkillCatalog::invoke` with `SkillAuthorityId("rollshot.bundled")`, `SkillPackageId("smart-redaction")`, `SkillInvocationKind::HostExplicit`.
- `compose_smart_redaction_prompt` validates package_id and source_authority, wraps body in `<rollshot-skill>` XML with digest attribute, prepends system envelope.
- Driver uses same prompt composition for both author and improve runs.
- Attack-body injection test proves: skill body cannot modify grants, registry, or authority boundary (driver.rs `attack_body_cannot_elevate_authority_or_modify_registry`).

## 5. Persistence-before-execution and V1/V2 migration evidence

### Persistence-before-execution

- `bind_run_contract` requires `TaskStatus::Running`, non-terminal attempt, matching `run_id`/`task_id`/`attempt_id`, and non-regressing timestamp.
- V2 `record_ready_for_review` rejects promotion if `store_schema_version >= 2 && run_contract.is_none()`.
- The `RunContractReceiptV1` is persisted to the task store via `compare_and_swap` before provider/tool work begins.

### V1 no-rewrite

- V1 tasks (`store_schema_version == 1`) load and promote without a run contract. `record_ready_for_review` has no contract gate for schema < 2.
- Test: `v1_ready_task_survives_round_trip_without_contract` confirms V1 tasks serialize/deserialize identically with `run_contract: None`.

### V2 receipt binding

- V2 tasks created via `ProductTaskSnapshot::new_v2` with `store_schema_version = 2`.
- `bind_run_contract` is idempotent on identical receipts, rejects conflicting receipts (`RunContractConflict`).
- Promotion carries `ProductArtifactMetadata::new_v2` with the exact same `RunContractReceiptV1`.

## 6. Artifact task/attempt/run/document/authority/skill trace

- `ProductArtifactMetadata` (V2) carries: `artifact_id`, `artifact_revision`, `kind`, `task_id`, `attempt_id`, `run_id`, `proposal_id`, `provider_id`, `model_id`, `run_config_digest`, `canonical_payload_sha256`, and `run_contract: RunContractReceiptV1`.
- `RunContractReceiptV1` contains: `authority: AuthoritySnapshotReceiptV1` (with `snapshot_digest`, `granted_operations`, `disclosure_ceiling`, `document_binding_digest`) and `skill_use: SkillUseReceiptV1` (with `package_digest`, `source_authority`, `package_id`).
- `canonical_config_v2_digest` includes `authority_snapshot_digest` and `skill_use.package_digest` in the V2 config fingerprint, ensuring artifact provenance is cryptographically bound.
- Tests confirm: changing authority digest or skill digest changes the config digest; artifact metadata carries the exact contract.

## 7. Privacy inspection

- `SkillUse::Debug` redacts body as `"<redacted>"`.
- `SkillUseReceiptV1` (the serialized DTO) carries `package_digest` but no body.
- `AuthoritySnapshotReceiptV1` carries no image bytes, OCR text, credentials, provider-native values, skill body, or ambient catalog path.
- `ProductTaskSnapshot::Debug` shows `pending_artifact_payload` length, not content.
- **No `println!`, `eprintln!`, or `dbg!` in production code** (`crates/rollshot-agent/src`, `crates/rollshot-app/src`).
- Serialization privacy test (`v2_ready_task_json_omits_skill_body_and_sensitive_fields`) confirms: no skill body text, no injected body text, no `api_key`/`password`/`secret`, no `/home/` paths, no raw body field in contract JSON.

## 8. Verification command results

### Step 1: Focused security and migration matrix

| Test filter | Passed | Ignored | Notes |
|------------|--------|---------|-------|
| `authority::tests` | 15 | 0 | denial-before-entry, grant, binding, disclosure |
| `skills::tests` | 47 | 0 | catalog limits/order/containment/digest |
| `tools::tests` | 59 | 0 | authority enforcement, batch halt |
| `product_task::tests` | 67 | 0 | bind/contract, V1/V2 migration, conflict |
| `task_store` | 36 | 0 | persistence, CAS, schema compat |
| `result_workspace::workbench` | 156 | 3 | artifact correlation, provenance, privacy |
| **Total** | **380** | **3** | |

### Step 2: Affected crate regression suites

| Crate | Passed | Ignored | Suites |
|-------|--------|---------|--------|
| rollshot-edit-proposal | 15 | 0 | 2 |
| rollshot-automation | 37 | 0 | 5 |
| rollshot-automation-rquickjs | 24 | 0 | 5 |
| rollshot-agent | 799 | 6 | 3 |
| rollshot-vision | 60 | 0 | 5 |
| rollshot-app | 378 | 0 | 3 |
| **Total** | **1,313** | **6** | |

### Step 3: Formatting, lint, and privacy

| Check | Result |
|-------|--------|
| `cargo fmt --check` | PASS |
| `cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings` | PASS (0 errors, 0 warnings) |
| `git diff --check` | PASS (no whitespace errors) |
| `println!`/`eprintln!`/`dbg!` in production code | None found |
| Full skill body in serialization | Not present (verified by test) |
| Ambient catalog path / pixels / OCR / credentials in Product paths | Not present |

**Note:** Clippy and formatting fixes were needed in new Slice 3 code. The 18 clippy errors (type_complexity, dead_code, manual_strip, single_match, useless_format, len_zero) and formatting diffs were resolved in commit `69520fe`. All were code-quality issues in new test and production code, not correctness or security defects.

## 9. Independent review questions and findings

| # | Question | Finding |
|---|---------|---------|
| 1 | Can provider/tool work begin before run-contract CAS is commit-visible? | **No.** `bind_run_contract` operates on the immutable `ProductTaskSnapshot` which uses CAS. V2 promotion requires the contract to be bound. The `AuthoritySnapshot` is constructed from the persisted receipt. |
| 2 | Can an advertised tool execute without every declared operation? | **No.** `execute_single` iterates all `required_operations()` and returns `AuthorityDenied` if any check fails. Tool body is never entered. |
| 3 | Can skill manifest/body/catalog membership mutate grants or registry membership? | **No.** `StaticSkillCatalog` is immutable after construction. `AuthoritySnapshot` is independently constructed. `ToolRegistry` is built from code, not skill content. Attack-body injection test proves this. |
| 4 | Can host-root loading follow a symlink or reopen an ambient path after validation? | **No.** All `open`/`openat` calls use `OFlags::NOFOLLOW`. `fstat` verifies `FileType::RegularFile` for manifest and body. Special files (FIFO, socket) are rejected. |
| 5 | Can a digest mismatch substitute current bytes? | **No.** `invoke()` compares `expected_digest` against `entry.digest` and returns `SkillError::DigestMismatch` on mismatch. |
| 6 | Can V1 pending artifacts still load/review without synthesized provenance? | **Yes, correctly.** V1 tasks have `run_contract: None` in metadata. `record_ready_for_review` has no contract gate for schema < 2. V1 round-trip test confirms. |
| 7 | Can V2 promotion use a receipt different from the active attempt? | **No.** `record_ready_for_review` checks `metadata.attempt_id == last_attempt.attempt_id` and `metadata.run_id() == last_attempt.run_id`. Mismatches return `ConflictingAttempt` or `RunMismatch`. |
| 8 | Can author/improve diverge in package digest or bypass stale checks? | **No.** Both paths use `bundled_smart_redaction_use()` → same catalog → same digest. Stale checks use `snapshot_revision` CAS. |
| 9 | Can durable/debug/tracing output leak body/path/pixels/OCR/credentials? | **No.** `SkillUse::Debug` redacts body. Receipt DTO has no body. `AuthoritySnapshot` has no sensitive data. Serialization tests confirm. |
| 10 | Did the slice introduce executable extension, script shortcut, routing, UI, job, retry, or deferred platform capability? | **No.** Only TOML parsing for manifests, file I/O for loading. No script execution, extension mechanism, routing, UI changes, or deferred capabilities. |

**No correctness or security defects found.** Clippy/formatting issues were code-quality only and resolved before this record.

## 10. Residual risks (spec §15)

1. **No mid-run OS/policy revocation.** The run-local snapshot does not provide live revocation. Acceptable for current bounded Smart Redaction run; a workload requiring live revocation must design a broker/lease.
2. **Static host loader constrained to two files, one directory level.** A need for arbitrary resources or project-local discovery is a new design.
3. **Provider attachment-delivery behavior is outside this slice.** Changing which pixels are uploaded requires a separate disclosure review.
4. **Platform containment.** If no-follow descriptor-relative loading cannot be implemented safely on both platforms, stop for a bounded platform spike.
5. **V1-to-V2 migration.** If V2 migration cannot preserve existing pending V1 review artifacts, stop and revise before enabling skill-backed runs. (Verified: V1 tasks load and promote correctly alongside V2 tasks.)

## 11. Scope boundary

Passing Gate G2 proves only the trustworthy minimum skill foundation. **Phase 3 implementation, launch-video work, remote skill providers, executable extensions, and any deferred capability remain unauthorized until their own gates and designs are approved.**
