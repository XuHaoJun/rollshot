# Launch Teaser Workflow Feasibility Concierge Design

**Date:** 2026-08-01  
**Status:** Approved design  
**Area:** Agent skills, Action Guide, launch-video discovery  
**Branch:** `docs/launch-teaser-feasibility`  
**Source idea:**
[`docs/ideas/2026-07-22-agent-skills-action-guide-launch-video.md`](../../ideas/2026-07-22-agent-skills-action-guide-launch-video.md)

## 1. Decision

Rollshot may restart work on the launch-video idea, but the next step is a
Phase 0 workflow-feasibility concierge experiment, not product implementation.

The trustworthy-skills prerequisite is now satisfied. Rollshot has a bounded
static skill catalog, immutable skill-use receipts, explicit authority,
budgets, cancellation, durable audit evidence, and reviewable artifacts. Smart
Redaction, Action Guide captions, and Action Guide visual annotations exercise
those contracts. Native Action Guide motion recording also retains the dynamic
source material that the deferred idea identified as an open prerequisite.

Those foundations prove that the experiment can start. They do not prove that
users want launch teasers, that agent-selected shots are good, or that Rollshot
should build a renderer.

This experiment answers only:

> Given a reviewed Action Guide, its native motion recording, and an explicitly
> authorized project repository, can a Rollshot-style creative workflow
> reliably produce a 15–25 second launch teaser, and which parts of a
> Hyperframes quality-ceiling result cannot be expressed by a deliberately
> narrow deterministic operation set?

A successful experiment permits demand-validation design. It never directly
authorizes an MVP.

## 2. Current-state evidence

The decision was made against these revisions:

- Rollshot: `1eb2db5a4cf7ff23da7bb1d2deea9fa7b38429e4`
- brag: `357a805e76a93a528ac6cccac28c8da3e893272b`
- Hyperframes: `807078c7cde9d5c8403588722d1cd9397c513a0d`

Relevant Rollshot evidence:

- The agent-foundation umbrella records user-confirmed Gate G3 completion and
  defines a separate discovery/design workflow as the launch-video restart
  condition.
- `crates/rollshot-agent/src/skills.rs` implements the bounded static catalog,
  immutable `SkillUse`, digest-pinned invocation, and `SkillUseReceiptV1`.
- The bundled catalog contains Smart Redaction, Action Guide captions, and
  Action Guide visual annotations skills.
- The Action Guide caption and visual-annotation product paths bind authority
  and skill receipts, run with budgets and cancellation, promote durable
  reviewable artifacts, and retain privacy-safe audit evidence.
- Commit `1d2ab8808f71028abe9655e795aaf588304889c8` adds native Action Guide
  motion recording, validation, persistence, and export.
- Focused verification on 2026-08-01 passed:
  - `rtk cargo test -p rollshot-agent skills::`: 53 passed.
  - `rtk cargo test -p rollshot-action motion::`: 58 passed, 1 ignored.

Current reference-workflow evidence:

- brag remains a narrow creative-director workflow: inspect a project, choose
  an angle, write a 15–25 second storyboard, hand composition to Hyperframes,
  validate, render, and write share copy.
- Hyperframes is now broader than the original idea snapshot: capture, brand
  token extraction, audio services, per-frame HTML composition, worker fan-out,
  Studio review, and local or cloud rendering all sit in its launch workflow.
- Hyperframes therefore serves as a quality ceiling and research instrument.
  Its runtime and workflow are not presumed to be Rollshot product
  dependencies.

## 3. Product boundary

### 3.1 In scope

- Two recent Rollshot feature cases.
- Two recent external-developer feature cases.
- One shared evidence and story-development process per case.
- Two landscape, 15–25 second versions per case:
  - a brag/Hyperframes quality-ceiling version;
  - a constrained deterministic-operations version.
- Internal operational review only.
- Source provenance, privacy boundaries, effort measurement, failure
  classification, and operation-gap analysis.
- A final verdict of `PROCEED_TO_DEMAND_VALIDATION`,
  `REPEAT_AFTER_FOUNDATION_FIX`, or `STOP`.

### 3.2 Explicitly out of scope

- Product code, a production `LaunchTeaserPlan` schema, or a Rollshot renderer.
- User-installable or remote skills.
- A Hyperframes fork, adapter, or product dependency.
- Agent automation of shot selection or copy generation.
- A claim that Action Guide-only is better than repository inspection.
- Validation that repository access can remain optional.
- Viewer-comprehension, creator-satisfaction, sharing-intent, actual-sharing,
  retention, conversion, or willingness-to-pay evidence.
- Approval to build an MVP.
- Vertical or square output, voiceover, generative audio, multi-template
  authoring, or a nonlinear editor.

### 3.3 Known evidence limitation

Every case begins with both Action Guide evidence and authorized repository
inspection. The experiment therefore validates only the combined workflow. It
cannot attribute value separately to Action Guide or repository inspection.
That comparison remains mandatory in a later validation phase.

Internal operational review also cannot establish whether a viewer understands
or values the result. A Phase 0 pass means the workflow is technically and
operationally plausible, not that the product thesis is validated.

## 4. Case entry contract

A case is eligible only when all of the following exist before production
starts:

1. a reviewed Action Guide with stable project revision;
2. reviewed step IDs, timestamps, captions, and keyframe references;
3. a validated native motion asset with identity, duration, resolution, and
   codec metadata;
4. explicit repository authorization with an allowlisted root or narrower file
   scope;
5. an allowlist of brand assets and captured content that may appear in the
   teaser;
6. a denylist of private screens, text, files, secrets, identities, and claims;
7. an agreement covering temporary artifact retention and deletion; and
8. enough motion coverage around candidate reviewed steps to attempt a
   15–25 second story without fabricated footage.

Cases are selected by this contract, not by apparent visual quality or ease of
composition. Both external cases must represent projects that the experiment
operator did not build.

Failure to satisfy the entry contract rejects the case before repository or
media ingestion. Replacing a rejected case is allowed only before production
starts and must be recorded.

## 5. Artifact model

The experiment uses research artifacts, not production Rollshot contracts.
Names below describe required content; an execution plan may choose exact file
formats and paths.

### 5.1 Case manifest

The immutable manifest records:

- case ID and internal/external classification;
- Action Guide project revision;
- reviewed step and source-asset identities;
- motion metadata and digest;
- authorized repository root and read scope;
- media allowlist and privacy denylist;
- tool and reference revisions;
- retention/deletion agreement; and
- experiment-protocol revision.

### 5.2 Evidence ledger

The ledger records only source-supported creative facts:

- feature purpose;
- official product and feature terminology;
- candidate claims and their repository sources;
- reviewed Action Guide steps and motion ranges demonstrating each claim;
- brand tokens and allowed assets;
- uncertainty, conflicts, and forbidden material; and
- the exact repository files read.

Unsupported or ambiguous claims are excluded rather than softened into generic
marketing language.

### 5.3 Shared creative brief

Both render branches receive the same story truth:

- hook;
- landscape format and 15–25 second target;
- three to five story beats;
- source clip candidates;
- caption intent;
- result shot and outro; and
- audio direction.

The brief fixes claims, beats, and source material. It does not force both
branches to use the same animation implementation.

### 5.4 Ceiling plan and render

The ceiling branch uses the current brag creative workflow and Hyperframes
composition workflow. It may use Hyperframes' motion vocabulary and supported
production facilities, but it may not introduce a claim, source asset, or
repository fact absent from the evidence ledger.

Hyperframes receives only the evidence ledger, shared brief, selected clips,
and selected brand assets. It does not receive the source repository root.

### 5.5 Constrained plan candidate and render

The constrained branch expresses the shared beats with only:

- trim;
- crop;
- focus or pan;
- speed adjustment;
- text overlay;
- simple transition; and
- optional fixed music bed.

The research-only plan candidate records, per shot:

- source motion time range;
- crop or focus region;
- speed;
- caption;
- transition intent; and
- audio cue.

It may also record the hook, target duration, result shot, and outro. It must not
be treated as a stable production schema.

The operator may use existing external editing tools to execute these
operations. Arbitrary HTML, JavaScript, custom shaders, generated code, or a
new Rollshot renderer are forbidden in this branch.

### 5.6 Case comparison record

The record captures:

- intake, evidence, planning, asset preparation, composition, rendering, and
  revision time;
- operator time separately from machine time;
- considered and rejected shots with reasons;
- unavailable or unusable source evidence;
- every effect the constrained vocabulary could not express;
- whether each gap is story-critical or polish-only;
- Hyperframes-only runtime, network, cloud, audio, or worker capabilities
  actually used; and
- privacy, provenance, timing, and tool failures.

## 6. Authority, privacy, and retention

Repository access is optional at the product-idea level but mandatory for this
combined-workflow experiment because that was the approved Phase 0 choice. It
remains explicit, bounded, and visible.

The experiment operator may inspect only the authorized repository scope.
Exact files read are appended to the evidence ledger. Extracted facts and
selected assets cross into the composition workspace; the repository itself
does not.

External tools receive the minimum case bundle needed for their branch. An
external tool request for broader filesystem, network, publishing, media, or
credential authority is denied unless that exact authority was approved in the
case manifest. Tool convenience never expands authority.

The following never enter Rollshot git:

- source repositories;
- raw captures or motion assets;
- private case manifests;
- credentials;
- full prompts or provider conversations;
- unredacted files-read details that reveal private paths; or
- private rendered videos.

The repository may receive only a privacy-reviewed aggregate report and
explicitly approved public examples. Private workspaces are deleted according
to each case agreement. Deletion completion is recorded.

Any unauthorized disclosure invalidates the entire experiment and produces a
fail-closed result. The affected case is not silently removed from scoring.

## 7. Execution sequence

### 7.1 Freeze the protocol

Before case production:

1. record Rollshot, brag, Hyperframes, FFmpeg, and relevant tool revisions;
2. freeze the constrained operation vocabulary;
3. freeze case forms, effort categories, and pass thresholds;
4. define isolated workspace and deletion procedures; and
5. verify the mechanical media-inspection commands.

The protocol cannot gain operations or relax thresholds after seeing results.

### 7.2 Select and authorize four cases

Select two Rollshot and two external cases against the entry contract. Complete
repository, media, privacy, and retention authorization before ingestion.

### 7.3 Run the first Rollshot case

Use the first internal case to verify procedure, artifact naming, timestamp
mapping, and render commands. The run may reveal procedural defects, but it may
not change the operation vocabulary or pass thresholds.

If the procedure changes, discard the affected research outputs and rerun that
case from a clean workspace under the frozen corrected protocol.

### 7.4 Run the remaining cases

Each case follows the same order:

1. intake and privacy check;
2. motion coverage and timestamp check;
3. evidence ledger;
4. shared creative brief;
5. ceiling plan and render;
6. constrained plan and render;
7. mechanical verification;
8. case comparison record; and
9. private-workspace retention or deletion action.

Project-specific assets or decisions from an earlier case do not become an
unrecorded template for later cases.

### 7.5 Aggregate and decide

Aggregate the four case records into:

- a case outcome matrix;
- an operation-gap matrix;
- stage-by-stage operator and machine-time distributions;
- systemic versus case-specific failures;
- privacy and provenance results; and
- one permitted verdict.

The report must not recommend `BUILD_MVP`.

## 8. Failure handling

The workflow stops for the affected case when:

- repository scope is absent or ambiguous;
- a source digest or project revision does not match;
- motion timestamps cannot be aligned with reviewed steps;
- a selected claim cannot be traced to authorized evidence;
- required motion was not retained;
- a tool requires unapproved authority; or
- forbidden content appears in an intermediate artifact.

No missing frame, claim, logo, or product state is fabricated.

Failure classification is exclusive and source-oriented:

- `CASE_INPUT`: this case lacks required evidence despite a valid protocol;
- `ACTION_GUIDE_EVIDENCE`: reviewed steps or metadata cannot support the story;
- `MOTION_TIMING_OR_RETENTION`: dynamic source ranges are missing or
  misaligned;
- `REPOSITORY_PROVENANCE`: claims or terminology cannot be authorized and
  traced;
- `CREATIVE_PLANNING`: the shared brief cannot form a 15–25 second story;
- `CONSTRAINED_EXPRESSIVENESS_STORY`: a required story beat needs a forbidden
  operation;
- `CONSTRAINED_EXPRESSIVENESS_POLISH`: only visual polish needs a forbidden
  operation;
- `CEILING_TOOL_OPERATIONAL`: brag or Hyperframes fails independently of
  Rollshot evidence;
- `PRIVACY_OR_AUTHORITY`: any disclosure or authority violation; or
- `PROTOCOL`: the experiment procedure itself is defective.

A ceiling-tool failure does not become a Rollshot evidence failure. A
constrained failure cannot be patched with arbitrary code; it is recorded as an
expressiveness result.

## 9. Mechanical verification

Every output video is inspected with `ffprobe` or an equivalent source-verified
media probe. Verification requires:

- decodable MP4;
- 1920×1080 landscape output;
- duration from 15 through 25 seconds inclusive;
- expected audio presence or absence; and
- every source range within the validated motion duration.

Every case additionally requires:

- manifest, ledger, shared brief, two branch plans, two video outputs, and case
  comparison record;
- consistent source IDs, digests, and files-read provenance across artifacts;
- only allowlisted operations in the constrained plan;
- a zero-count forbidden-content checklist;
- recorded operator time and machine time; and
- recorded retention or deletion action.

The experiment makes no automated assertion about visual quality, viewer
comprehension, sharing intent, or user satisfaction.

## 10. Phase 0 gate

The verdict is `PROCEED_TO_DEMAND_VALIDATION` only when all of these hold:

1. both external cases complete successfully;
2. at least one Rollshot case completes successfully, for at least three of
   four successful dual-version cases;
3. the only permitted failed case is a case-specific source problem, not a
   systemic Action Guide, motion, provenance, creative, or rendering failure;
4. every successful constrained version expresses all shared story beats;
5. every remaining constrained-versus-ceiling gap is polish-only;
6. median operator time for intake, ledger, shared brief, constrained plan, and
   constrained asset preparation is at most four hours per case; and
7. there are zero privacy or provenance violations.

The verdict is `REPEAT_AFTER_FOUNDATION_FIX` when a bounded, identifiable
Rollshot foundation gap prevents a valid test and can be corrected without
changing the product thesis or importing a general video platform.

The verdict is `STOP` when the workflow cannot form a coherent short story from
reviewed evidence, the narrow operation set has a systemic story-critical gap,
the authority model is operationally untenable, or passing would require
silently broadening the experiment.

## 11. Required next phase after a pass

A pass leads to a separate demand-validation design. That phase must add:

- creator review for factual accuracy, privacy, and usefulness;
- review by people who did not build the feature;
- sharing intent or actual-sharing evidence;
- an Action Guide-only versus repository-enriched comparison;
- explicit testing that repository access can remain optional; and
- a product/CEO gate before any production `LaunchTeaserPlan` or renderer
  design.

Only that later evidence can decide whether a narrow Rollshot MVP deserves a
specification.

## 12. Completion artifacts

Phase 0 is complete after all four selected cases have run to either a
successful dual-version result or a classified terminal failure. It produces:

1. four case manifests;
2. an evidence ledger and shared creative brief for every case that passes
   intake, or a terminal intake-failure record identifying the missing
   prerequisite;
3. ceiling and constrained plans plus MP4s for every successful case;
4. a branch-specific terminal failure record for every attempted render that
   does not produce its plan or MP4;
5. four case comparison or terminal failure records;
6. one operation-gap matrix;
7. one aggregate effort and failure report;
8. one privacy-reviewed retention/deletion record; and
9. one permitted Phase 0 verdict.

Private artifacts may remain outside the repository. The committed aggregate
must make every conclusion auditable without exposing private source material.
