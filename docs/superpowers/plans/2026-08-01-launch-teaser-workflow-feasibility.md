# Launch Teaser Workflow Feasibility Concierge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute a four-case, dual-render concierge experiment that determines whether reviewed Action Guides plus authorized repository evidence can support a constrained 15–25 second launch-teaser workflow and whether the next step should be demand validation.

**Architecture:** Keep all private repositories, recordings, intermediate plans, and videos in an isolated research workspace outside Rollshot git. Each case is reduced to an evidence ledger and shared creative brief, then rendered through a brag/Hyperframes quality-ceiling branch and a fixed-operation constrained branch. Commit only a privacy-reviewed aggregate report and redacted matrices after all four cases terminate successfully or with a classified failure.

**Tech Stack:** Rollshot Action Guide project export and native motion asset; current local brag and Hyperframes checkouts; FFmpeg/ffprobe; Markdown, JSON, and CSV research artifacts; git only for the redacted aggregate.

## Global Constraints

- Governing spec: `docs/superpowers/specs/2026-08-01-launch-teaser-workflow-feasibility-design.md`.
- This is an experiment, not product implementation. Do not modify Rust crates or add a production `LaunchTeaserPlan` or renderer.
- Use exactly two Rollshot cases and two external-developer cases.
- Every case begins with a reviewed Action Guide, validated native motion asset, and explicit repository authorization.
- Every successful case produces a 1920×1080 landscape MP4 from 15 through 25 seconds inclusive in both branches.
- Constrained operations are exactly trim, crop, focus/pan, speed adjustment, text overlay, simple transition, and optional fixed music bed.
- The constrained branch must not use arbitrary HTML, JavaScript, custom shaders, generated composition code, or a new Rollshot renderer.
- Hyperframes is a research quality ceiling, not a product dependency.
- Hyperframes receives a sanitized case bundle, never the source repository root.
- Record exact repository files read and source provenance. Do not invent missing claims, frames, assets, or product states.
- Any privacy or authority violation invalidates the entire experiment and forces a fail-closed verdict.
- Private repositories, raw captures, motion assets, credentials, prompts, private paths, and private videos never enter Rollshot git.
- The experiment may return only `PROCEED_TO_DEMAND_VALIDATION`, `REPEAT_AFTER_FOUNDATION_FIX`, or `STOP`; never `BUILD_MVP`.
- A pass requires both external cases, at least one Rollshot case, no story-critical constrained-operation gap, median constrained preparation time at most four hours, and zero privacy/provenance violations.
- Do not use a git worktree; repository rules prohibit worktrees unless explicitly requested.
- Prefix every shell command with `rtk`.
- This is an investigation. The videos, case records, and aggregate verdict are the proof; do not add automated product tests.

## File and workspace structure

No private case file is created under the Rollshot repository. Resolve the private root once:

```text
$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/
├── protocol/
│   ├── protocol-lock.json
│   ├── constrained-operations.json
│   └── templates/
│       ├── case-manifest.json
│       ├── constrained-plan.json
│       └── comparison.md
├── cases/
│   ├── rs-01/
│   ├── rs-02/
│   ├── ext-01/
│   └── ext-02/
└── aggregate/
    ├── case-outcomes.csv
    ├── operation-gaps.csv
    ├── effort.csv
    ├── retention-deletion.csv
    └── private-report.md
```

Each case directory owns one case only:

```text
<case-id>/
├── case-manifest.json
├── intake/
│   ├── action-guide/
│   ├── motion/
│   └── selected-assets/
├── evidence-ledger.md
├── creative-brief.md
├── ceiling/
│   ├── plan.md
│   ├── composition/
│   ├── video.mp4
│   └── run-record.md
├── constrained/
│   ├── plan.json
│   ├── workspace/
│   ├── video.mp4
│   └── run-record.md
├── comparison.md
├── terminal-failure.json
└── retention-deletion.json
```

`terminal-failure.json` is absent for a successful case. A failed case may omit downstream artifacts that were never validly produced.

The only new repository artifacts created during execution are:

- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-aggregate.md`
- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-case-outcomes.csv`
- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-operation-gaps.csv`
- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-effort.csv`

The committed CSV files use opaque case IDs only. They contain no private path, repository name, participant identity, unpublished product name, raw claim, prompt, URL, credential, or media digest.

---

### Task 1: Freeze the experiment protocol

**Files:**
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/protocol-lock.json`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/constrained-operations.json`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/templates/case-manifest.json`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/templates/constrained-plan.json`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/templates/comparison.md`

**Interfaces:**
- Consumes: approved design spec and the current Rollshot, brag, Hyperframes, FFmpeg, and ffprobe installations.
- Produces: immutable `protocol_revision`, exact tool revisions, case IDs, constrained operation vocabulary, artifact templates, and mechanical media checks used by every later task.

- [ ] **Step 1: Resolve and protect the private workspace**

Set an absolute path outside `/home/noah/rollshot`, create it with owner-only permissions, and resolve its canonical path before writing any case data:

```bash
rtk mkdir -p "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/templates"
rtk mkdir -p "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/cases"
rtk mkdir -p "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate"
rtk chmod 700 "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT"
rtk realpath "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT"
```

Expected: `realpath` is outside the Rollshot repository. If it is inside the repository, stop and choose a new root before writing case data.

- [ ] **Step 2: Record exact tool and reference revisions**

Collect:

```bash
rtk git -C /home/noah/rollshot rev-parse HEAD
rtk git -C /home/noah/rollshot/learn-projects/brag rev-parse HEAD
rtk git -C /home/noah/rollshot/learn-projects/hyperframes rev-parse HEAD
rtk ffmpeg -version
rtk ffprobe -version
rtk npx hyperframes --version
```

Write `protocol-lock.json` with this exact shape:

```json
{
  "schema_version": 1,
  "protocol_revision": "phase0-v1",
  "frozen_at_utc": "<RFC3339 UTC timestamp>",
  "rollshot_revision": "<40 hex>",
  "brag_revision": "<40 hex>",
  "hyperframes_revision": "<40 hex>",
  "ffmpeg_version": "<first version line>",
  "ffprobe_version": "<first version line>",
  "hyperframes_cli_version": "<version>",
  "case_ids": ["rs-01", "rs-02", "ext-01", "ext-02"],
  "pass_thresholds": {
    "required_external_successes": 2,
    "required_total_successes": 3,
    "max_median_constrained_prep_operator_minutes": 240,
    "max_privacy_or_provenance_violations": 0,
    "max_story_critical_operation_gaps": 0
  }
}
```

Do not continue if any revision or executable version is unavailable. Classify that as `PROTOCOL`, repair the environment, and regenerate the lock before case intake.

- [ ] **Step 3: Freeze the constrained operation vocabulary**

Write `constrained-operations.json` exactly:

```json
{
  "schema_version": 1,
  "protocol_revision": "phase0-v1",
  "allowed_operations": [
    "trim",
    "crop",
    "focus_pan",
    "speed_adjustment",
    "text_overlay",
    "simple_transition",
    "fixed_music_bed"
  ],
  "forbidden_runtime_classes": [
    "arbitrary_html",
    "javascript",
    "custom_shader",
    "generated_composition_code",
    "new_rollshot_renderer"
  ]
}
```

Hash the file and append its SHA-256 to `protocol-lock.json` as `constrained_operations_sha256`. Any later edit changes the protocol and invalidates prior cases.

- [ ] **Step 4: Create the case-manifest template**

Write `templates/case-manifest.json` with all required fields and empty case-specific values:

```json
{
  "schema_version": 1,
  "protocol_revision": "phase0-v1",
  "case_id": "",
  "cohort": "rollshot_or_external",
  "action_guide": {
    "project_revision": "",
    "reviewed_step_ids": [],
    "reviewed_step_timestamps_ms": [],
    "caption_refs": [],
    "keyframe_refs": []
  },
  "motion": {
    "sha256": "",
    "duration_ms": 0,
    "width": 0,
    "height": 0,
    "codec": ""
  },
  "repository_authority": {
    "authorized_root": "",
    "allowed_paths": [],
    "forbidden_paths": [],
    "authorized_at_utc": "",
    "authorized_by": ""
  },
  "media_authority": {
    "allowed_assets": [],
    "forbidden_content": []
  },
  "retention": {
    "delete_by_utc": "",
    "may_publish": false
  },
  "files_read": [],
  "intake_status": "pending"
}
```

Private absolute paths and participant identity may appear here because the file stays outside git.

- [ ] **Step 5: Create the constrained-plan template**

Write `templates/constrained-plan.json`:

```json
{
  "schema_version": 1,
  "protocol_revision": "phase0-v1",
  "case_id": "",
  "hook": "",
  "format": {"width": 1920, "height": 1080},
  "target_duration_ms": 0,
  "shots": [
    {
      "shot_id": "shot-01",
      "story_beat_id": "",
      "source_start_ms": 0,
      "source_end_ms": 0,
      "operations": [],
      "crop_or_focus": null,
      "speed": 1.0,
      "caption": null,
      "transition": "cut",
      "audio_cue": null
    }
  ],
  "result_shot_id": "",
  "outro": "",
  "music_bed": null
}
```

Every `operations` value must come from the frozen allowlist. Source times are absolute within the validated native motion asset.

- [ ] **Step 6: Create the comparison template**

Write `templates/comparison.md` with these required headings:

```markdown
# Case comparison: <case-id>

## Terminal status
## Artifact completeness
## Operator minutes by stage
## Machine minutes by stage
## Considered and rejected shots
## Unavailable or unusable source evidence
## Constrained operation gaps
## Hyperframes-only capabilities actually used
## Privacy, authority, provenance, and timing findings
## Case-specific versus systemic classification
## Retention or deletion action
```

- [ ] **Step 7: Verify the frozen protocol**

Run JSON parsing and hash checks:

```bash
rtk jq -e '.protocol_revision == "phase0-v1" and (.case_ids | length == 4)' "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/protocol-lock.json"
rtk jq -e '.allowed_operations | length == 7' "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/constrained-operations.json"
rtk sha256sum "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/protocol/constrained-operations.json"
```

Expected: both `jq` commands exit 0 and the checksum equals `constrained_operations_sha256` in the lock.

- [ ] **Step 8: Record the protocol checkpoint**

Do not commit private templates. Record in the session log that `phase0-v1` is frozen, including the protocol lock path and constrained-operations checksum. Later tasks must cite both.

---

### Task 2: Select and authorize the four cases

**Files:**
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/cases/{rs-01,rs-02,ext-01,ext-02}/case-manifest.json`
- Create outside git on rejection: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/cases/<case-id>/terminal-failure.json`

**Interfaces:**
- Consumes: `phase0-v1` templates and four candidate Action Guide projects.
- Produces: four authorized manifests that meet the entry contract, or a classified rejection before media/repository ingestion.

- [ ] **Step 1: Screen two Rollshot candidates**

For each candidate, confirm before copying data:

```text
[ ] Reviewed Action Guide revision is stable.
[ ] Reviewed step IDs, timestamps, captions, and keyframes exist.
[ ] Validated native motion asset exists.
[ ] Candidate steps have usable motion before and after their timestamps.
[ ] Repository read scope is explicitly approved.
[ ] Allowed and forbidden captured content is listed.
[ ] Retention/deletion deadline is set.
```

Choose exactly two that pass. Assign opaque IDs `rs-01` and `rs-02`; do not encode feature names in directory names.

- [ ] **Step 2: Screen two external candidates**

Apply the same checklist. Additionally require:

```text
[ ] The experiment operator did not build the project.
[ ] The participant controls the repository and captured material.
[ ] Repository access is limited to an explicit root or allowlisted paths.
[ ] The participant understands that this phase does not evaluate usefulness or sharing intent.
[ ] Deletion and publication terms are explicit.
```

Choose exactly two that pass. Assign `ext-01` and `ext-02`.

- [ ] **Step 3: Populate each manifest before ingestion**

Copy the template and fill every field except `files_read`. Set `intake_status` to `authorized`. Never use blank digests, zero motion dimensions, an empty reviewed-step list, or an implicit repository root.

- [ ] **Step 4: Verify each motion asset and record metadata**

For each case, run:

```bash
rtk sha256sum "<motion-path>"
rtk ffprobe -v error -show_entries format=duration -show_entries stream=codec_name,width,height -of json "<motion-path>"
```

Expected: a stable SHA-256, positive duration, positive dimensions, and a named video codec. Copy exact values into the manifest.

- [ ] **Step 5: Verify manifest completeness without exposing values**

For each manifest:

```bash
rtk jq -e '
  .protocol_revision == "phase0-v1" and
  (.action_guide.reviewed_step_ids | length > 0) and
  (.motion.sha256 | length == 64) and
  (.motion.duration_ms > 0) and
  (.motion.width > 0) and
  (.motion.height > 0) and
  (.repository_authority.authorized_root | length > 0) and
  (.retention.delete_by_utc | length > 0) and
  .intake_status == "authorized"
' "<case-manifest-path>"
```

Expected: exit 0 for all four cases.

- [ ] **Step 6: Handle a rejected candidate correctly**

If a candidate fails before production, write:

```json
{
  "schema_version": 1,
  "case_id": "<case-id>",
  "stage": "intake",
  "category": "CASE_INPUT",
  "privacy_or_authority_violation": false,
  "reason": "<specific missing entry prerequisite>",
  "terminal_at_utc": "<RFC3339 UTC timestamp>"
}
```

Replace the candidate before any case-production task starts. Keep the opaque case ID and note replacement in the private aggregate log.

- [ ] **Step 7: Record the case-selection checkpoint**

Confirm four authorized manifests exist and that neither private paths nor participant details were written inside the Rollshot repository. Do not commit.

---

### Task 3: Run the `rs-01` protocol-validation case

**Files:**
- Create outside git: all `cases/rs-01/` artifacts defined in the workspace structure.

**Interfaces:**
- Consumes: authorized `rs-01` manifest, reviewed Action Guide, native motion, repository scope, and `phase0-v1` protocol.
- Produces: a successful dual-version case comparison or classified terminal failure; verifies that the procedure itself is executable before the remaining cases run.

- [ ] **Step 1: Revalidate protocol and case identity**

Verify the constrained-operations checksum still matches the lock, the Action Guide revision is unchanged, and the motion SHA-256 matches the manifest. On mismatch, stop with `PROTOCOL` or `CASE_INPUT`; do not update the manifest to the new value.

- [ ] **Step 2: Build the evidence ledger**

Read only authorized repository paths. Append each exact file opened to `case-manifest.json.files_read`. Write `evidence-ledger.md` with these sections:

```markdown
# Evidence ledger: rs-01
## Feature purpose
## Official terminology
## Supported claims
## Reviewed steps and demonstrating motion ranges
## Brand tokens and allowed assets
## Uncertainty and conflicts
## Forbidden material
## Exact repository files read
```

Every supported claim names both its repository source and at least one reviewed Action Guide step/motion range. Exclude unsupported claims.

- [ ] **Step 3: Check motion coverage before story planning**

For every candidate reviewed step, verify `0 <= source_start_ms < source_end_ms <= motion.duration_ms`. Record unusable steps and exact reasons in the ledger. Stop with `MOTION_TIMING_OR_RETENTION` if no three-beat 15–25 second story remains.

- [ ] **Step 4: Write the shared creative brief**

Write `creative-brief.md` with:

```markdown
# Creative brief: rs-01
## Hook
## Format and duration
## Story beats
## Source clip candidates
## Caption intent
## Result shot and outro
## Audio direction
## Evidence exclusions
```

Use three to five beats. Each beat cites a supported claim and source range. The total target duration is 15–25 seconds.

- [ ] **Step 5: Render the ceiling version**

Load and follow the pinned brag workflow at `learn-projects/brag/skills/brag/SKILL.md`. Give it only the sanitized ledger, shared brief, selected clips, and selected brand assets. Use Hyperframes from the pinned local revision; do not provide the repository root.

Write the final brag/Hyperframes plan to `ceiling/plan.md`, composition under `ceiling/composition/`, output to `ceiling/video.mp4`, and exact commands, network services, audio services, worker use, operator minutes, and machine minutes to `ceiling/run-record.md`.

Stop this branch as `CEILING_TOOL_OPERATIONAL` if its own workflow cannot complete. Do not broaden authority or patch Hyperframes in Rollshot.

- [ ] **Step 6: Render the constrained version**

Translate the same story beats into `constrained/plan.json`. Validate every operation against `constrained-operations.json`. Execute only the seven allowed operation classes with existing external media tools. Write exact commands and effort to `constrained/run-record.md`; output `constrained/video.mp4`.

If a story beat requires a forbidden operation, stop and classify it as `CONSTRAINED_EXPRESSIVENESS_STORY`. If only polish is unavailable, finish the video and record `CONSTRAINED_EXPRESSIVENESS_POLISH` in the comparison.

- [ ] **Step 7: Mechanically verify both MP4s**

Run for each output:

```bash
rtk ffprobe -v error -show_entries format=duration -show_entries stream=codec_type,codec_name,width,height -of json "<video.mp4>"
```

Expected:

```text
video stream decodes
width = 1920
height = 1080
15.0 <= duration <= 25.0
audio stream presence matches creative-brief.md
```

Also verify every constrained source range is within the native motion duration and every constrained operation is allowlisted.

- [ ] **Step 8: Write the comparison and classify gaps**

Copy the comparison template to `comparison.md` and complete every heading. Classify each constrained gap as exactly `story-critical` or `polish-only`. Record operator minutes separately for intake, ledger, shared brief, constrained plan, constrained asset preparation, constrained composition, ceiling composition, and revision.

- [ ] **Step 9: Validate the procedure checkpoint**

Confirm naming, timestamp mapping, commands, and artifact checks are executable. If the procedure itself was defective, classify `PROTOCOL`, correct only the procedure, delete the invalid case outputs, and rerun `rs-01` from Step 1. Do not change the constrained operation set or pass thresholds.

- [ ] **Step 10: Record retention state**

Write `retention-deletion.json` with case ID, retained artifact classes, deletion deadline, publication permission, and current action. Do not commit any `rs-01` artifact.

---

### Task 4: Run the `rs-02` case under the frozen protocol

**Files:**
- Create outside git: all `cases/rs-02/` artifacts defined in the workspace structure.

**Interfaces:**
- Consumes: successful protocol checkpoint from Task 3 and authorized `rs-02` sources.
- Produces: one independent Rollshot dual-version result or classified terminal failure without changing the protocol.

- [ ] **Step 1: Revalidate immutable inputs**

Verify `phase0-v1`, constrained-operations checksum, Action Guide revision, and motion SHA-256. Stop on mismatch; do not adopt drift.

- [ ] **Step 2: Produce the evidence ledger**

Read only authorized paths, append every opened file to `files_read`, and write the same eight required ledger sections used by `rs-01`. Bind every claim to repository evidence and a reviewed step/motion range.

- [ ] **Step 3: Prove usable motion coverage**

Validate all candidate ranges against motion duration. Require enough usable ranges for three to five story beats and a 15–25 second target without fabricated footage.

- [ ] **Step 4: Produce the shared creative brief**

Write hook, format/duration, three to five beats, clip candidates, caption intent, result/outro, audio direction, and evidence exclusions. Do not reuse project-specific copy or assets from `rs-01`.

- [ ] **Step 5: Produce the ceiling branch**

Run the pinned brag and Hyperframes workflows against the sanitized case bundle only. Save the exact plan, composition, MP4, capabilities used, commands, and effort in the `ceiling/` paths.

- [ ] **Step 6: Produce the constrained branch**

Translate the same beats to the frozen constrained-plan shape, validate the allowlist, render with only allowed operations, and save the plan, MP4, commands, and effort in `constrained/`.

- [ ] **Step 7: Verify outputs and plan ranges**

Use the Task 3 ffprobe command. Require decodable 1920×1080 MP4s from 15 through 25 seconds, expected audio presence, source ranges within motion duration, and only allowlisted operations.

- [ ] **Step 8: Complete the comparison or terminal failure**

Complete every comparison heading. If the case fails, write `terminal-failure.json` with stage, one failure category from spec §8, privacy flag, specific reason, and terminal timestamp. Never fabricate missing downstream artifacts.

- [ ] **Step 9: Record retention state**

Write `retention-deletion.json`. Do not commit any `rs-02` artifact.

---

### Task 5: Run the `ext-01` case under external authority

**Files:**
- Create outside git: all `cases/ext-01/` artifacts defined in the workspace structure.

**Interfaces:**
- Consumes: authorized external manifest and frozen protocol.
- Produces: the first external generalization result, with exact authority and deletion evidence.

- [ ] **Step 1: Reconfirm external authorization immediately before access**

Confirm the participant still controls the repository/media, the authorized root and allowed paths are unchanged, publication permission is unchanged, and the deletion deadline is active. Record confirmation time in the private manifest.

- [ ] **Step 2: Revalidate immutable media and protocol**

Verify protocol checksum, Action Guide revision, and motion SHA-256. A mismatch stops the case; it does not authorize reading the changed repository or recording.

- [ ] **Step 3: Produce the evidence ledger with exact files-read provenance**

Read only authorized paths. Record each exact file. Write all required ledger sections. Each claim must cite an authorized file and a reviewed step/motion range. Do not copy source prose beyond what is needed for the ledger.

- [ ] **Step 4: Validate motion coverage and write the shared brief**

Require valid ranges for a three-to-five-beat, 15–25 second story. Write the shared brief from the ledger only. Exclude every denied screen, identity, secret, or unsupported claim.

- [ ] **Step 5: Produce the sanitized ceiling branch**

Copy only selected clips, allowed assets, the ledger, and brief into the ceiling workspace. Run pinned brag/Hyperframes there. Record any requested network, cloud, audio, worker, or filesystem capability before use; deny anything outside the manifest.

- [ ] **Step 6: Produce the constrained branch**

Write the constrained plan from the same beats, validate operations and ranges, render only allowlisted operations, and capture commands and effort.

- [ ] **Step 7: Verify both outputs and scan forbidden content**

Run the ffprobe checks. Manually inspect the case denylist against the ledger, plans, run records, and rendered frames. Any forbidden content is `PRIVACY_OR_AUTHORITY`, invalidates the entire experiment, and stops all remaining case work.

- [ ] **Step 8: Complete comparison or terminal failure**

Write every comparison section or a typed terminal failure. Explicitly label findings as case-specific or systemic.

- [ ] **Step 9: Apply retention/deletion agreement**

Write `retention-deletion.json`. If the agreement requires immediate deletion after analysis, delete raw repository copies, clips, and compositions now; retain only the minimum private comparison evidence allowed by the agreement. Record completion time.

---

### Task 6: Run the `ext-02` case under external authority

**Files:**
- Create outside git: all `cases/ext-02/` artifacts defined in the workspace structure.

**Interfaces:**
- Consumes: second authorized external manifest and frozen protocol.
- Produces: the second mandatory external result and completes the four-case evidence set.

- [ ] **Step 1: Reconfirm authorization and retention terms**

Reconfirm repository/media control, scope, publication permission, and deletion deadline immediately before access. Stop if any term is ambiguous or withdrawn.

- [ ] **Step 2: Revalidate protocol, project revision, and motion digest**

Require an unchanged protocol checksum, Action Guide revision, and native motion SHA-256. Do not normalize drift into the manifest.

- [ ] **Step 3: Produce the source-bound evidence ledger**

Read only authorized paths and append exact files read. Bind official terminology and claims to repository sources and reviewed motion ranges. Record all uncertainty and forbidden material.

- [ ] **Step 4: Validate motion and write the shared brief**

Require enough valid source ranges for the fixed story shape and duration. Write hook, beats, clip candidates, captions, result/outro, audio, and exclusions from the ledger.

- [ ] **Step 5: Produce the ceiling branch from a sanitized bundle**

Provide no repository root to brag or Hyperframes. Record the plan, composition, MP4, exact capabilities used, commands, network/service use, operator time, and machine time.

- [ ] **Step 6: Produce the constrained branch**

Write and validate the constrained plan, render only allowlisted operations, and capture exact commands and effort. Record forbidden-operation needs without adding them.

- [ ] **Step 7: Verify media, ranges, operations, and forbidden content**

Run the same ffprobe checks, validate all plan ranges and operations, and inspect the denylist against every artifact. Any privacy/authority violation stops the whole experiment.

- [ ] **Step 8: Complete comparison or terminal failure**

Complete all comparison headings or write the exact terminal failure. Mark every operation gap story-critical or polish-only.

- [ ] **Step 9: Apply retention/deletion agreement**

Write the retention/deletion record and perform any due deletion before aggregation.

---

### Task 7: Aggregate the four cases and compute the gate

**Files:**
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/case-outcomes.csv`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/operation-gaps.csv`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/effort.csv`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/retention-deletion.csv`
- Create outside git: `$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/private-report.md`

**Interfaces:**
- Consumes: four successful comparisons or typed terminal failures.
- Produces: deterministic gate inputs, private aggregate, and one permitted verdict.

- [ ] **Step 1: Build the case outcome matrix**

Use this exact header:

```csv
case_id,cohort,intake_status,ceiling_status,constrained_status,terminal_category,case_specific,privacy_or_authority_violation,story_critical_gap,final_case_success
```

There must be exactly four data rows, one per opaque case ID. `final_case_success` is true only when both verified MP4s and the complete comparison exist.

- [ ] **Step 2: Build the operation-gap matrix**

Use:

```csv
case_id,story_beat_id,requested_effect,allowed_operation_available,gap_class,story_blocked,hyperframes_capability_used
```

Include one row per identified gap. `gap_class` is `story-critical` or `polish-only`. Do not include private product copy or asset names.

- [ ] **Step 3: Build the effort matrix**

Use:

```csv
case_id,stage,operator_minutes,machine_minutes
```

Required constrained-preparation stages are `intake`, `ledger`, `shared_brief`, `constrained_plan`, and `constrained_asset_prep`. Keep ceiling composition separate.

- [ ] **Step 4: Build the retention/deletion matrix**

Use:

```csv
case_id,raw_repo_copy_status,raw_motion_status,composition_status,private_render_status,delete_by_utc,last_action_utc
```

This private file may describe artifact classes but must not contain private paths.

- [ ] **Step 5: Compute pass inputs without subjective quality scoring**

Calculate:

```text
external_successes = successful ext-01 + successful ext-02
total_successes = successful rs-01 + rs-02 + ext-01 + ext-02
privacy_violations = count where privacy_or_authority_violation = true
story_critical_gaps = count where story_blocked = true
median_constrained_prep_minutes = median per-case sum of the five required stages
systemic_failure = any failed case whose failure is not case-specific
```

Do not score attractiveness, viewer comprehension, creator satisfaction, or sharing intent.

- [ ] **Step 6: Select exactly one verdict**

Use these rules in order:

```text
if privacy_violations > 0:
    STOP
else if a bounded Rollshot foundation defect prevented a valid test and no
        product-thesis expansion is needed:
    REPEAT_AFTER_FOUNDATION_FIX
else if external_successes == 2
     and total_successes >= 3
     and story_critical_gaps == 0
     and median_constrained_prep_minutes <= 240
     and not systemic_failure:
    PROCEED_TO_DEMAND_VALIDATION
else:
    STOP
```

Never emit `BUILD_MVP`.

- [ ] **Step 7: Write the private aggregate report**

Use these headings:

```markdown
# Launch Teaser Phase 0 private aggregate
## Protocol and revisions
## Four-case outcomes
## Motion and Action Guide evidence findings
## Repository provenance findings
## Creative planning findings
## Constrained operation gaps
## Ceiling-only capabilities actually used
## Operator and machine effort
## Privacy, authority, and retention
## Systemic versus case-specific failures
## Verdict calculation
## Required next step
```

Every conclusion cites an opaque case ID and matrix row, not a private name or path.

- [ ] **Step 8: Verify aggregate row counts and verdict inputs**

Run:

```bash
rtk wc -l "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/case-outcomes.csv"
rtk wc -l "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/effort.csv"
rtk sort "$ROLLSHOT_LAUNCH_TEASER_EXPERIMENT_ROOT/aggregate/case-outcomes.csv" | rtk uniq -c
```

Expected: `case-outcomes.csv` has five lines including header and each case ID appears exactly once. Inspect the effort matrix to confirm all successful cases contain the five constrained-preparation stages.

---

### Task 8: Redact, independently review, and commit the aggregate

**Files:**
- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-aggregate.md`
- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-case-outcomes.csv`
- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-operation-gaps.csv`
- Create: `docs/researchs/launch-teaser-phase0/2026-08-01-effort.csv`
- Read only: private aggregate files from Task 7.

**Interfaces:**
- Consumes: complete private aggregate and retention/deletion matrix.
- Produces: privacy-safe, auditable repository evidence and the Phase 0 verdict commit.

- [ ] **Step 1: Produce redacted CSVs**

Copy only the approved columns and opaque IDs. Remove all names, paths, URLs, claims, captions, asset identifiers, digests, timestamps that identify a participant, and free-form failure prose. Keep typed categories and numeric effort.

The committed case-outcome CSV header remains:

```csv
case_id,cohort,intake_status,ceiling_status,constrained_status,terminal_category,case_specific,privacy_or_authority_violation,story_critical_gap,final_case_success
```

The operation-gap and effort headers remain those from Task 7.

- [ ] **Step 2: Write the redacted aggregate report**

Use the Task 7 report headings, but include only aggregate facts, opaque case IDs, typed categories, operation names, counts, durations, and the verdict. Add explicit limitations:

```text
This phase did not test viewer comprehension, creator satisfaction, sharing
intent, actual sharing, willingness to pay, Action Guide-only evidence, or
repository-optional operation. A pass authorizes demand-validation design only.
```

List the pinned Rollshot, brag, Hyperframes, FFmpeg, and ffprobe revisions/versions. Do not embed private video stills or links.

- [ ] **Step 3: Run a privacy string scan**

Build a local forbidden-string list containing participant names, repository names, absolute private roots, URLs, product codenames, credential prefixes, distinctive unpublished claims, and private asset names. Search the four proposed committed files for every exact string. Expected: zero matches.

Also search for common secret patterns and absolute paths:

```bash
rtk git diff --check
```

Use the repository search tool, not shell grep, for regex scans of the proposed files. Any match blocks the commit until removed.

- [ ] **Step 4: Obtain independent privacy/provenance review**

Give a clean-context reviewer only:

- the governing spec;
- the four proposed committed files;
- the forbidden-string categories, not private forbidden values; and
- the statement that raw media and repositories must remain outside git.

Acceptance requires a written verdict that the committed aggregate is auditable without exposing private sources and that the verdict follows the numeric gate. The reviewer must not receive or approve raw private artifacts.

- [ ] **Step 5: Verify retention/deletion actions**

Before commit, confirm every case has a current retention/deletion record and every due deletion is complete. A missed deletion blocks completion even if the experiment otherwise passes.

- [ ] **Step 6: Stage only the redacted aggregate**

```bash
rtk git add docs/researchs/launch-teaser-phase0/2026-08-01-aggregate.md
rtk git add docs/researchs/launch-teaser-phase0/2026-08-01-case-outcomes.csv
rtk git add docs/researchs/launch-teaser-phase0/2026-08-01-operation-gaps.csv
rtk git add docs/researchs/launch-teaser-phase0/2026-08-01-effort.csv
rtk git diff --staged --check
rtk git status --short
```

Expected: exactly four staged files under `docs/researchs/launch-teaser-phase0/`; no private workspace artifact is present.

- [ ] **Step 7: Commit the Phase 0 verdict**

Use one of these messages according to the evidence:

```bash
rtk git commit -m "docs(video): record launch teaser feasibility results"
```

The report body contains the exact verdict. Do not encode participant or project identity in the commit message.

- [ ] **Step 8: Deliver the bounded conclusion**

Report:

```text
Verdict: <one permitted verdict>
Successful dual-version cases: <N>/4
External successes: <N>/2
Story-critical constrained gaps: <N>
Median constrained preparation: <N> minutes
Privacy/provenance violations: <N>
Committed aggregate: <path and commit>
Private retention/deletion status: <complete or exact blocker>
Next authorized step: <demand-validation design, bounded foundation repair, or stop>
```

Do not describe Phase 0 as product validation or recommend implementation unless a later approved demand-validation workflow supplies that evidence.
